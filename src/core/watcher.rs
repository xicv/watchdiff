use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::Duration;
use notify::{Event, RecommendedWatcher, RecursiveMode, Watcher};
use anyhow::{Result, Context};
use super::{FileEvent, FileEventKind, filter::FileFilter};
use super::events::AppEvent;
use crate::ai::{AIDetector, ConfidenceScorer};
use crate::config::WatchDiffConfig;

pub struct FileWatcher {
    _watcher: RecommendedWatcher,
    event_rx: Receiver<AppEvent>,
    filter: FileFilter,
}

impl FileWatcher {
    pub fn new<P: AsRef<Path>>(path: P) -> Result<Self> {
        Self::with_config(path, WatchDiffConfig::default())
    }
    
    pub fn with_config<P: AsRef<Path>>(path: P, config: WatchDiffConfig) -> Result<Self> {
        let path = path.as_ref();
        let filter = FileFilter::new(path)?;
        
        let (tx, rx) = mpsc::channel::<notify::Result<Event>>();
        let (event_tx, event_rx) = mpsc::channel::<AppEvent>();

        // Create the notify watcher
        let mut watcher = notify::recommended_watcher(tx)
            .context("Failed to create file system watcher")?;

        watcher
            .watch(path, RecursiveMode::Recursive)
            .context("Failed to start watching directory")?;

        let filter_clone = FileFilter::new(path)?;
        let config_clone = config.clone();

        // Spawn background thread to process notify events
        thread::spawn(move || {
            let mut previous_contents = std::collections::HashMap::<PathBuf, String>::new();
            let mut last_event_time = std::collections::HashMap::<PathBuf, std::time::Instant>::new();
            let mut ai_detector = AIDetector::new();
            let confidence_scorer = ConfidenceScorer::new();
            
            // Diff cache: (old_hash, new_hash) -> diff_result
            let mut diff_cache = std::collections::HashMap::<(u64, u64), String>::new();
            let cache_size_limit = config_clone.cache.diff_cache_size;
            let debounce_duration = config_clone.watcher.event_debounce_duration();

            while let Ok(result) = rx.recv() {
                match result {
                    Ok(event) => {
                        // Debounce rapid events on the same path
                        let now = std::time::Instant::now();
                        
                        for path in event.paths {
                            // Filter out ignored files
                            if !filter_clone.should_watch(&path) {
                                continue;
                            }
                            
                            // Debounce: ignore events that happen too quickly after the previous one
                            if let Some(last_time) = last_event_time.get(&path) {
                                if now.duration_since(*last_time) < debounce_duration {
                                    continue;  // Skip this event as it's too soon
                                }
                            }
                            last_event_time.insert(path.clone(), now);

                            let file_event = match event.kind {
                                notify::EventKind::Create(_) => {
                                    let mut fe = FileEvent::new(path.clone(), FileEventKind::Created);
                                    
                                    // For new files, read content for preview
                                    if filter_clone.is_text_file(&path) {
                                        if let Ok(content) = std::fs::read_to_string(&path) {
                                            let preview = if content.len() > 200 {
                                                format!("{}...", &content[..200])
                                            } else {
                                                content.clone()
                                            };
                                            fe = fe.with_preview(preview);
                                            previous_contents.insert(path.clone(), content);
                                        }
                                    }
                                    Some(fe)
                                }
                                notify::EventKind::Modify(_) => {
                                    let mut fe = FileEvent::new(path.clone(), FileEventKind::Modified);
                                    
                                    // Generate diff for modified files
                                    if filter_clone.is_text_file(&path) {
                                        if let Ok(new_content) = std::fs::read_to_string(&path) {
                                            if let Some(old_content) = previous_contents.get(&path) {
                                                // Skip if content hasn't actually changed
                                                if *old_content == new_content {
                                                    continue;
                                                }
                                                
                                                // Use hash-based diff caching
                                                let old_hash = Self::hash_content(old_content);
                                                let new_hash = Self::hash_content(&new_content);
                                                let cache_key = (old_hash, new_hash);
                                                
                                                let diff = if let Some(cached_diff) = diff_cache.get(&cache_key) {
                                                    // Use cached diff
                                                    cached_diff.clone()
                                                } else {
                                                    // Generate new diff and cache it
                                                    let new_diff = crate::diff::generate_unified_diff(old_content, &new_content, &path, &path);
                                                    diff_cache.insert(cache_key, new_diff.clone());
                                                    
                                                    // Limit cache size to prevent memory growth
                                                    if diff_cache.len() > cache_size_limit {
                                                        // Clear cache when it exceeds limit
                                                        let cleanup_threshold = (cache_size_limit as f32 * config_clone.cache.cleanup_threshold) as usize;
                                                        if diff_cache.len() > cleanup_threshold {
                                                            diff_cache.clear();
                                                        }
                                                    }
                                                    
                                                    new_diff
                                                };
                                                
                                                fe = fe.with_diff(diff);
                                            } else {
                                                // First time seeing this file - show a preview instead of empty diff
                                                let preview = if new_content.len() > 200 {
                                                    format!("{}...", &new_content[..200])
                                                } else {
                                                    new_content.clone()
                                                };
                                                fe = fe.with_preview(preview);
                                            }
                                            previous_contents.insert(path.clone(), new_content);
                                        }
                                    }
                                    Some(fe)
                                }
                                notify::EventKind::Remove(_) => {
                                    previous_contents.remove(&path);
                                    Some(FileEvent::new(path.clone(), FileEventKind::Deleted))
                                }
                                _ => None,
                            };

                            if let Some(mut fe) = file_event {
                                // Detect change origin using AI detector
                                let origin = ai_detector.detect_change_origin();
                                fe = fe.with_origin(origin.clone());

                                // Detect batch changes
                                if let Some(batch_id) = ai_detector.detect_batch_change(&path, &origin) {
                                    fe = fe.with_batch_id(batch_id);
                                }

                                // Score confidence if we have diff content
                                if let Some(ref diff) = fe.diff {
                                    let confidence = confidence_scorer.score_change(diff, &path);
                                    fe = fe.with_confidence(confidence);
                                }

                                if event_tx.send(AppEvent::FileChanged(fe)).is_err() {
                                    break; // Receiver dropped, exit thread
                                }
                            }
                        }
                    }
                    Err(err) => {
                        tracing::error!("File watcher error: {}", err);
                    }
                }
            }
        });

        Ok(Self {
            _watcher: watcher,
            event_rx,
            filter,
        })
    }

    pub fn try_recv(&self) -> Result<AppEvent, std::sync::mpsc::TryRecvError> {
        self.event_rx.try_recv()
    }

    pub fn recv(&self) -> Result<AppEvent, std::sync::mpsc::RecvError> {
        self.event_rx.recv()
    }

    pub fn recv_timeout(&self, timeout: Duration) -> Result<AppEvent, std::sync::mpsc::RecvTimeoutError> {
        self.event_rx.recv_timeout(timeout)
    }

    pub fn get_initial_files(&self) -> Result<Vec<PathBuf>> {
        self.filter.get_watchable_files()
    }
    
    /// Hash content for diff caching
    fn hash_content(content: &str) -> u64 {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};
        
        let mut hasher = DefaultHasher::new();
        content.hash(&mut hasher);
        hasher.finish()
    }
}

pub fn start_ticker(sender: Sender<AppEvent>) {
    thread::spawn(move || {
        loop {
            thread::sleep(Duration::from_millis(100));
            if sender.send(AppEvent::Tick).is_err() {
                break;
            }
        }
    });
}