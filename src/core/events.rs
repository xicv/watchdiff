use std::path::PathBuf;
use std::time::SystemTime;
use std::collections::VecDeque;
use serde::{Deserialize, Serialize};
use crate::config::WatchDiffConfig;
use super::summary::{ChangeSummary, SummaryFilters};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum FileEventKind {
    Created,
    Modified,
    Deleted,
    Moved { from: PathBuf, to: PathBuf },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ChangeOrigin {
    Human,
    AIAgent { tool_name: String, process_id: Option<u32> },
    Tool { name: String },
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ConfidenceLevel {
    Safe,    // 🟢 Low risk, likely correct
    Review,  // 🟡 Medium risk, should review  
    Risky,   // 🔴 High risk, likely problematic
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChangeConfidence {
    pub level: ConfidenceLevel,
    pub score: f32,  // 0.0 (risky) to 1.0 (safe)
    pub reasons: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileEvent {
    pub path: PathBuf,
    pub kind: FileEventKind,
    pub timestamp: SystemTime,
    pub diff: Option<String>,
    pub content_preview: Option<String>,
    pub origin: ChangeOrigin,
    pub confidence: Option<ChangeConfidence>,
    pub batch_id: Option<String>,  // Groups related changes together
}

#[derive(Debug, Clone)]
pub struct HighlightedFileEvent {
    pub path: PathBuf,
    pub kind: FileEventKind,
    pub timestamp: SystemTime,
    pub diff: Option<String>,
    pub content_preview: Option<String>,
    pub highlighted_diff: Option<String>,
    pub highlighted_preview: Option<String>,
    pub origin: ChangeOrigin,
    pub confidence: Option<ChangeConfidence>,
    pub batch_id: Option<String>,
}

impl FileEvent {
    pub fn new(path: PathBuf, kind: FileEventKind) -> Self {
        Self {
            path,
            kind,
            timestamp: SystemTime::now(),
            diff: None,
            content_preview: None,
            origin: ChangeOrigin::Unknown,
            confidence: None,
            batch_id: None,
        }
    }

    pub fn with_origin(mut self, origin: ChangeOrigin) -> Self {
        self.origin = origin;
        self
    }

    pub fn with_confidence(mut self, confidence: ChangeConfidence) -> Self {
        self.confidence = Some(confidence);
        self
    }

    pub fn with_batch_id(mut self, batch_id: String) -> Self {
        self.batch_id = Some(batch_id);
        self
    }

    pub fn with_diff(mut self, diff: String) -> Self {
        self.diff = Some(diff);
        self
    }

    pub fn with_preview(mut self, preview: String) -> Self {
        self.content_preview = Some(preview);
        self
    }

    pub fn to_highlighted(&self) -> HighlightedFileEvent {
        let highlighted_event = HighlightedFileEvent {
            path: self.path.clone(),
            kind: self.kind.clone(),
            timestamp: self.timestamp,
            diff: self.diff.clone(),
            content_preview: self.content_preview.clone(),
            highlighted_diff: None,
            highlighted_preview: None,
            origin: self.origin.clone(),
            confidence: self.confidence.clone(),
            batch_id: self.batch_id.clone(),
        };

        // Skip syntax highlighting to avoid ANSI escape codes in TUI
        // The TUI will use its own built-in coloring for diff display
        // Terminal highlighting is only useful for non-TUI output modes

        highlighted_event
    }
}

impl HighlightedFileEvent {
    pub fn from_file_event(event: FileEvent) -> Self {
        event.to_highlighted()
    }
}

#[derive(Debug, Clone)]
pub enum AppEvent {
    FileChanged(FileEvent),
    Tick,
    Quit,
    ScrollUp,
    ScrollDown,
    ToggleHelp,
}

#[derive(Debug, Clone)]
pub struct AppState {
    pub events: VecDeque<FileEvent>,
    pub highlighted_events: VecDeque<HighlightedFileEvent>,
    pub scroll_offset: usize,
    pub max_events: usize,
    pub show_help: bool,
    pub watched_files: std::collections::HashSet<PathBuf>,
    /// Time-based cleanup: remove events older than this duration
    pub max_event_age: std::time::Duration,
    /// Last cleanup time to avoid frequent cleanup operations
    last_cleanup: std::time::Instant,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            events: VecDeque::new(),
            highlighted_events: VecDeque::new(),
            scroll_offset: 0,
            max_events: 1000,
            show_help: false,
            watched_files: std::collections::HashSet::new(),
            max_event_age: std::time::Duration::from_secs(3600), // 1 hour
            last_cleanup: std::time::Instant::now(),
        }
    }
}

impl AppState {
    /// Create a new AppState with configuration
    pub fn with_config(config: &WatchDiffConfig) -> Self {
        Self {
            events: VecDeque::new(),
            highlighted_events: VecDeque::new(),
            scroll_offset: 0,
            max_events: config.watcher.max_events,
            show_help: false,
            watched_files: std::collections::HashSet::new(),
            max_event_age: config.watcher.max_event_age_duration(),
            last_cleanup: std::time::Instant::now(),
        }
    }
    
    pub fn add_event(&mut self, event: FileEvent) {
        self.add_event_with_cleanup_interval(event, std::time::Duration::from_secs(300))
    }
    
    pub fn add_event_with_cleanup_interval(&mut self, event: FileEvent, cleanup_interval: std::time::Duration) {
        // Convert to highlighted event
        let highlighted = event.to_highlighted();
        
        // Add to front of deque for newest-first ordering
        self.events.push_front(event);
        self.highlighted_events.push_front(highlighted);
        
        // Maintain size limits using efficient pop_back
        while self.events.len() > self.max_events {
            self.events.pop_back();
        }
        while self.highlighted_events.len() > self.max_events {
            self.highlighted_events.pop_back();
        }
        
        // Periodic cleanup of old events
        let now = std::time::Instant::now();
        if now.duration_since(self.last_cleanup) > cleanup_interval {
            self.cleanup_old_events();
            self.last_cleanup = now;
        }
        
        self.scroll_offset = 0;
    }
    
    /// Remove events older than max_event_age to prevent indefinite memory growth
    fn cleanup_old_events(&mut self) {
        let cutoff_time = std::time::SystemTime::now() - self.max_event_age;
        
        // Remove old events from back (oldest events)
        while let Some(back_event) = self.events.back() {
            if back_event.timestamp < cutoff_time {
                self.events.pop_back();
                self.highlighted_events.pop_back();
            } else {
                break;
            }
        }
    }

    pub fn scroll_up(&mut self) {
        if self.scroll_offset > 0 {
            self.scroll_offset -= 1;
        }
    }

    pub fn scroll_down(&mut self) {
        if self.scroll_offset < self.highlighted_events.len().saturating_sub(1) {
            self.scroll_offset += 1;
        }
    }

    pub fn toggle_help(&mut self) {
        self.show_help = !self.show_help;
    }

    pub fn get_visible_events(&self, height: usize) -> Vec<&FileEvent> {
        let start = self.scroll_offset;
        let end = (start + height).min(self.events.len());
        self.events.iter().skip(start).take(end - start).collect()
    }

    pub fn get_visible_highlighted_events(&self, height: usize) -> Vec<&HighlightedFileEvent> {
        let start = self.scroll_offset;
        let end = (start + height).min(self.highlighted_events.len());
        self.highlighted_events.iter().skip(start).take(end - start).collect()
    }
    
    /// Generate a change summary from current events
    pub fn generate_summary(&self, filters: &SummaryFilters) -> ChangeSummary {
        let events: Vec<FileEvent> = self.events.iter().cloned().collect();
        ChangeSummary::from_events(&events, filters)
    }
    
    /// Generate a summary with default filters
    pub fn generate_default_summary(&self) -> ChangeSummary {
        self.generate_summary(&SummaryFilters::default())
    }
    
    /// Generate a summary for the last hour
    pub fn generate_recent_summary(&self) -> ChangeSummary {
        let mut filters = SummaryFilters::default();
        filters.time_frame = super::summary::SummaryTimeFrame::LastHour;
        self.generate_summary(&filters)
    }
    
    /// Generate a summary for a specific time frame
    pub fn generate_summary_for_timeframe(&self, timeframe: super::summary::SummaryTimeFrame) -> ChangeSummary {
        let mut filters = SummaryFilters::default();
        filters.time_frame = timeframe;
        self.generate_summary(&filters)
    }
    
    /// Generate a summary filtered by origin (who made the changes)
    pub fn generate_summary_by_origin(&self, origins: Vec<ChangeOrigin>) -> ChangeSummary {
        let mut filters = SummaryFilters::default();
        filters.include_origins = origins;
        self.generate_summary(&filters)
    }
    
    /// Get summary statistics without full summary generation (for quick stats)
    pub fn get_quick_stats(&self) -> (usize, usize, usize, usize) {
        let total_files = self.events.len();
        let mut created = 0;
        let mut modified = 0; 
        let mut deleted = 0;
        
        // Count based on most recent state of each file
        let mut file_states = std::collections::HashMap::new();
        for event in self.events.iter().rev() { // Reverse to get oldest first
            file_states.entry(&event.path).or_insert(&event.kind);
        }
        
        for kind in file_states.values() {
            match kind {
                FileEventKind::Created => created += 1,
                FileEventKind::Modified => modified += 1,
                FileEventKind::Deleted => deleted += 1,
                FileEventKind::Moved { .. } => {}, // Count as neither for quick stats
            }
        }
        
        (total_files, created, modified, deleted)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_file_event_creation() {
        let path = PathBuf::from("test.rs");
        let event = FileEvent::new(path.clone(), FileEventKind::Modified);
        
        assert_eq!(event.path, path);
        assert!(matches!(event.kind, FileEventKind::Modified));
        assert!(matches!(event.origin, ChangeOrigin::Unknown));
        assert!(event.confidence.is_none());
        assert!(event.batch_id.is_none());
    }

    #[test]
    fn test_file_event_with_origin() {
        let path = PathBuf::from("test.rs");
        let origin = ChangeOrigin::AIAgent {
            tool_name: "Claude Code".to_string(),
            process_id: Some(12345),
        };
        
        let event = FileEvent::new(path.clone(), FileEventKind::Modified)
            .with_origin(origin.clone());
        
        assert!(matches!(event.origin, ChangeOrigin::AIAgent { .. }));
        if let ChangeOrigin::AIAgent { tool_name, process_id } = event.origin {
            assert_eq!(tool_name, "Claude Code");
            assert_eq!(process_id, Some(12345));
        } else {
            panic!("Expected AIAgent origin");
        }
    }

    #[test]
    fn test_file_event_with_confidence() {
        let path = PathBuf::from("test.rs");
        let confidence = ChangeConfidence {
            level: ConfidenceLevel::Review,
            score: 0.6,
            reasons: vec!["Debug output detected".to_string()],
        };
        
        let event = FileEvent::new(path, FileEventKind::Modified)
            .with_confidence(confidence.clone());
        
        assert!(event.confidence.is_some());
        let event_confidence = event.confidence.unwrap();
        assert!(matches!(event_confidence.level, ConfidenceLevel::Review));
        assert_eq!(event_confidence.score, 0.6);
        assert_eq!(event_confidence.reasons, vec!["Debug output detected"]);
    }

    #[test]
    fn test_file_event_with_batch_id() {
        let path = PathBuf::from("test.rs");
        let batch_id = "batch_123456".to_string();
        
        let event = FileEvent::new(path, FileEventKind::Modified)
            .with_batch_id(batch_id.clone());
        
        assert_eq!(event.batch_id, Some(batch_id));
    }

    #[test]
    fn test_file_event_chaining() {
        let path = PathBuf::from("test.rs");
        let origin = ChangeOrigin::Human;
        let confidence = ChangeConfidence {
            level: ConfidenceLevel::Safe,
            score: 0.9,
            reasons: vec![],
        };
        let batch_id = "batch_789".to_string();
        let diff = "- old line\n+ new line".to_string();
        
        let event = FileEvent::new(path.clone(), FileEventKind::Modified)
            .with_origin(origin.clone())
            .with_confidence(confidence.clone())
            .with_batch_id(batch_id.clone())
            .with_diff(diff.clone());
        
        assert_eq!(event.path, path);
        assert!(matches!(event.origin, ChangeOrigin::Human));
        assert!(event.confidence.is_some());
        assert_eq!(event.batch_id, Some(batch_id));
        assert_eq!(event.diff, Some(diff));
    }

    #[test]
    fn test_highlighted_file_event_conversion() {
        let path = PathBuf::from("test.rs");
        let origin = ChangeOrigin::Tool { name: "rustfmt".to_string() };
        let confidence = ChangeConfidence {
            level: ConfidenceLevel::Safe,
            score: 0.95,
            reasons: vec!["Formatting tool".to_string()],
        };
        
        let event = FileEvent::new(path.clone(), FileEventKind::Modified)
            .with_origin(origin.clone())
            .with_confidence(confidence.clone());
        
        let highlighted = event.to_highlighted();
        
        assert_eq!(highlighted.path, path);
        assert!(matches!(highlighted.origin, ChangeOrigin::Tool { .. }));
        assert!(highlighted.confidence.is_some());
        
        if let ChangeOrigin::Tool { name } = highlighted.origin {
            assert_eq!(name, "rustfmt");
        }
    }

    #[test]
    fn test_app_state_add_event_with_ai_features() {
        let mut state = AppState::default();
        
        let event = FileEvent::new(PathBuf::from("test.rs"), FileEventKind::Created)
            .with_origin(ChangeOrigin::AIAgent {
                tool_name: "Claude Code".to_string(),
                process_id: Some(42),
            })
            .with_confidence(ChangeConfidence {
                level: ConfidenceLevel::Review,
                score: 0.7,
                reasons: vec!["Large change detected".to_string()],
            })
            .with_batch_id("batch_001".to_string());
        
        state.add_event(event);
        
        assert_eq!(state.events.len(), 1);
        assert_eq!(state.highlighted_events.len(), 1);
        
        let stored_event = &state.events[0];
        assert!(matches!(stored_event.origin, ChangeOrigin::AIAgent { .. }));
        assert!(stored_event.confidence.is_some());
        assert_eq!(stored_event.batch_id, Some("batch_001".to_string()));
        
        let highlighted_event = &state.highlighted_events[0];
        assert!(matches!(highlighted_event.origin, ChangeOrigin::AIAgent { .. }));
        assert!(highlighted_event.confidence.is_some());
        assert_eq!(highlighted_event.batch_id, Some("batch_001".to_string()));
    }
    
    #[test]
    fn test_app_state_generate_summary() {
        let mut state = AppState::default();
        
        // Add some test events
        let event1 = FileEvent::new(PathBuf::from("file1.rs"), FileEventKind::Created);
        let event2 = FileEvent::new(PathBuf::from("file2.rs"), FileEventKind::Modified);
        
        state.add_event(event1);
        state.add_event(event2);
        
        let summary = state.generate_default_summary();
        
        assert_eq!(summary.stats.total_files, 2);
        assert_eq!(summary.stats.total_changes, 2);
        assert_eq!(summary.files.len(), 2);
    }
    
    #[test]
    fn test_app_state_generate_recent_summary() {
        let mut state = AppState::default();
        
        // Add a recent event
        let recent_event = FileEvent::new(PathBuf::from("recent.rs"), FileEventKind::Created);
        state.add_event(recent_event);
        
        let summary = state.generate_recent_summary();
        
        assert_eq!(summary.stats.total_files, 1);
        assert_eq!(summary.files[0].path, PathBuf::from("recent.rs"));
    }
    
    #[test]
    fn test_app_state_quick_stats() {
        let mut state = AppState::default();
        
        // Add some events
        state.add_event(FileEvent::new(PathBuf::from("created.rs"), FileEventKind::Created));
        state.add_event(FileEvent::new(PathBuf::from("modified.rs"), FileEventKind::Modified));
        state.add_event(FileEvent::new(PathBuf::from("deleted.rs"), FileEventKind::Deleted));
        
        let (total, created, modified, deleted) = state.get_quick_stats();
        
        assert_eq!(total, 3);
        assert_eq!(created, 1);
        assert_eq!(modified, 1);
        assert_eq!(deleted, 1);
    }
    
    #[test]
    fn test_app_state_summary_by_origin() {
        let mut state = AppState::default();
        
        let human_event = FileEvent::new(PathBuf::from("human.rs"), FileEventKind::Created)
            .with_origin(ChangeOrigin::Human);
        let ai_event = FileEvent::new(PathBuf::from("ai.rs"), FileEventKind::Created)
            .with_origin(ChangeOrigin::AIAgent {
                tool_name: "Claude".to_string(),
                process_id: Some(123),
            });
            
        state.add_event(human_event);
        state.add_event(ai_event);
        
        let human_summary = state.generate_summary_by_origin(vec![ChangeOrigin::Human]);
        
        assert_eq!(human_summary.stats.total_files, 1);
        assert_eq!(human_summary.files[0].path, PathBuf::from("human.rs"));
    }
}