use std::path::PathBuf;
use std::time::{Duration, Instant, SystemTime};
use std::collections::HashMap;
use lru::LruCache;
use ratatui::style::Style;

/// Cache for file contents to avoid repeated disk I/O
pub struct FileContentCache {
    cache: LruCache<PathBuf, CachedFileContent>,
}

/// Cached file content with metadata
#[derive(Clone)]
pub struct CachedFileContent {
    pub content: String,
    pub last_modified: SystemTime,
    pub size: u64,
}

/// Cache for syntax-highlighted content to avoid repeated highlighting
pub struct SyntaxHighlightCache {
    cache: LruCache<SyntaxCacheKey, Vec<Vec<(Style, String)>>>,
}

/// Key for syntax highlighting cache
#[derive(Hash, Eq, PartialEq, Clone)]
pub struct SyntaxCacheKey {
    pub path: PathBuf,
    pub language: String,
    pub content_hash: u64,
}

/// Cache for search results to enable incremental search
pub struct SearchResultCache {
    pub last_query: String,
    pub last_results: Vec<(PathBuf, i32)>,
    pub last_all_files_hash: u64,
}

/// Event debouncer to reduce processing overhead
pub struct EventDebouncer {
    pending_events: HashMap<PathBuf, (crate::core::FileEvent, Instant)>,
    debounce_duration: Duration,
}

impl FileContentCache {
    pub fn new(capacity: usize) -> Self {
        Self {
            cache: LruCache::new(std::num::NonZeroUsize::new(capacity).unwrap()),
        }
    }

    /// Get cached file content or read from disk if not cached/stale
    pub fn get_content(&mut self, path: &PathBuf) -> Result<String, std::io::Error> {
        // Check if we have cached content
        if let Some(cached) = self.cache.get(path) {
            // Verify cache is still valid by checking modification time
            if let Ok(metadata) = std::fs::metadata(path) {
                if let Ok(modified) = metadata.modified() {
                    if modified <= cached.last_modified {
                        return Ok(cached.content.clone());
                    }
                }
            }
        }

        // Cache miss or stale - read from disk
        let content = std::fs::read_to_string(path)?;
        let metadata = std::fs::metadata(path)?;
        let last_modified = metadata.modified().unwrap_or(SystemTime::UNIX_EPOCH);
        let size = metadata.len();

        // Cache the content
        self.cache.put(path.clone(), CachedFileContent {
            content: content.clone(),
            last_modified,
            size,
        });

        Ok(content)
    }

    /// Invalidate cache entry for a specific file
    pub fn invalidate(&mut self, path: &PathBuf) {
        self.cache.pop(path);
    }

    /// Get cache statistics
    pub fn stats(&self) -> (usize, usize) {
        (self.cache.len(), self.cache.cap().get())
    }
}

impl SyntaxHighlightCache {
    pub fn new(capacity: usize) -> Self {
        Self {
            cache: LruCache::new(std::num::NonZeroUsize::new(capacity).unwrap()),
        }
    }

    /// Get cached syntax highlighting or compute if not cached
    pub fn get_highlighted_content(
        &mut self,
        path: &PathBuf,
        content: &str,
        language: &str,
        highlighter: &crate::highlight::SyntaxHighlighter,
    ) -> Vec<Vec<(Style, String)>> {
        let content_hash = self.calculate_content_hash(content);
        let cache_key = SyntaxCacheKey {
            path: path.clone(),
            language: language.to_string(),
            content_hash,
        };

        // Check cache first
        if let Some(highlighted) = self.cache.get(&cache_key) {
            return highlighted.clone();
        }

        // Cache miss - compute highlighting
        let highlighted = highlighter.highlight_code(content, language);
        
        // Cache the result
        self.cache.put(cache_key, highlighted.clone());
        
        highlighted
    }

    /// Calculate a simple hash of content for cache key
    fn calculate_content_hash(&self, content: &str) -> u64 {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};
        
        let mut hasher = DefaultHasher::new();
        content.hash(&mut hasher);
        hasher.finish()
    }

    /// Invalidate cache entries for a specific file
    pub fn invalidate_file(&mut self, path: &PathBuf) {
        let keys_to_remove: Vec<_> = self.cache
            .iter()
            .filter(|(key, _)| key.path == *path)
            .map(|(key, _)| key.clone())
            .collect();
        
        for key in keys_to_remove {
            self.cache.pop(&key);
        }
    }

    /// Get cache statistics
    pub fn stats(&self) -> (usize, usize) {
        (self.cache.len(), self.cache.cap().get())
    }
}

impl SearchResultCache {
    pub fn new() -> Self {
        Self {
            last_query: String::new(),
            last_results: Vec::new(),
            last_all_files_hash: 0,
        }
    }

    /// Check if we can use incremental search
    pub fn can_use_incremental(&self, query: &str, all_files_hash: u64) -> bool {
        // Can use incremental if:
        // 1. New query is an extension of the previous query
        // 2. File set hasn't changed
        !self.last_query.is_empty() 
            && query.starts_with(&self.last_query)
            && all_files_hash == self.last_all_files_hash
    }

    /// Get cached results for incremental search
    pub fn get_incremental_base(&self) -> &[(PathBuf, i32)] {
        &self.last_results
    }

    /// Update cache with new results
    pub fn update(&mut self, query: String, results: Vec<(PathBuf, i32)>, all_files_hash: u64) {
        self.last_query = query;
        self.last_results = results;
        self.last_all_files_hash = all_files_hash;
    }

    /// Clear cache
    pub fn clear(&mut self) {
        self.last_query.clear();
        self.last_results.clear();
        self.last_all_files_hash = 0;
    }
}

impl EventDebouncer {
    pub fn new(debounce_duration: Duration) -> Self {
        Self {
            pending_events: HashMap::new(),
            debounce_duration,
        }
    }

    /// Add an event to the debouncer
    pub fn add_event(&mut self, event: crate::core::FileEvent) {
        let now = Instant::now();
        self.pending_events.insert(event.path.clone(), (event, now));
    }

    /// Get events that are ready to be processed (debounce period has elapsed)
    pub fn get_ready_events(&mut self) -> Vec<crate::core::FileEvent> {
        let now = Instant::now();
        let mut ready_events = Vec::new();
        
        // Find events that have been pending long enough
        let ready_paths: Vec<_> = self.pending_events
            .iter()
            .filter(|(_, (_, timestamp))| now.duration_since(*timestamp) >= self.debounce_duration)
            .map(|(path, _)| path.clone())
            .collect();
        
        // Remove ready events and collect them
        for path in ready_paths {
            if let Some((event, _)) = self.pending_events.remove(&path) {
                ready_events.push(event);
            }
        }
        
        ready_events
    }

    /// Get count of pending events
    pub fn pending_count(&self) -> usize {
        self.pending_events.len()
    }

    /// Clear all pending events
    pub fn clear(&mut self) {
        self.pending_events.clear();
    }
}

/// Combined performance cache manager
pub struct PerformanceCache {
    pub file_content: FileContentCache,
    pub syntax_highlight: SyntaxHighlightCache,
    pub search_results: SearchResultCache,
    pub event_debouncer: EventDebouncer,
}

impl PerformanceCache {
    pub fn new() -> Self {
        Self {
            file_content: FileContentCache::new(200),        // Cache up to 200 files
            syntax_highlight: SyntaxHighlightCache::new(100), // Cache up to 100 highlighted files  
            search_results: SearchResultCache::new(),
            event_debouncer: EventDebouncer::new(Duration::from_millis(100)), // 100ms debounce
        }
    }

    /// Invalidate all caches for a specific file (when file changes)
    pub fn invalidate_file(&mut self, path: &PathBuf) {
        self.file_content.invalidate(path);
        self.syntax_highlight.invalidate_file(path);
        // Search cache will be invalidated naturally when file set changes
    }

    /// Get overall cache statistics
    pub fn stats(&self) -> PerformanceCacheStats {
        let (content_size, content_cap) = self.file_content.stats();
        let (syntax_size, syntax_cap) = self.syntax_highlight.stats();
        
        PerformanceCacheStats {
            file_content_entries: content_size,
            file_content_capacity: content_cap,
            syntax_highlight_entries: syntax_size,
            syntax_highlight_capacity: syntax_cap,
            pending_events: self.event_debouncer.pending_count(),
            search_cache_active: !self.search_results.last_query.is_empty(),
        }
    }
}

#[derive(Debug)]
pub struct PerformanceCacheStats {
    pub file_content_entries: usize,
    pub file_content_capacity: usize,
    pub syntax_highlight_entries: usize,
    pub syntax_highlight_capacity: usize,
    pub pending_events: usize,
    pub search_cache_active: bool,
}