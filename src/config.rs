//! Configuration management for WatchDiff
//!
//! This module provides configuration structures and defaults for various
//! components of the application including caching, file watching, and performance.

use std::time::Duration;
use serde::{Deserialize, Serialize};

/// Global configuration for WatchDiff
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WatchDiffConfig {
    /// File watcher configuration
    pub watcher: WatcherConfig,
    /// Caching configuration
    pub cache: CacheConfig,
    /// UI configuration
    pub ui: UiConfig,
    /// AI detection configuration
    pub ai: AiConfig,
}

/// Configuration for file watching
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WatcherConfig {
    /// Debounce duration for file events in milliseconds
    pub event_debounce_ms: u64,
    /// Maximum number of events to keep in memory
    pub max_events: usize,
    /// Time to keep events before cleanup
    pub max_event_age_secs: u64,
    /// Cleanup interval in seconds
    pub cleanup_interval_secs: u64,
}

/// Configuration for various caches
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheConfig {
    /// Maximum size of diff result cache
    pub diff_cache_size: usize,
    /// Maximum size of process cache for AI detection
    pub process_cache_size: usize,
    /// Maximum number of recent changes for batch detection
    pub batch_changes_limit: usize,
    /// Cache cleanup threshold (when to trigger cleanup)
    pub cleanup_threshold: f32,
}

/// Configuration for user interface
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UiConfig {
    /// Search debounce duration in milliseconds
    pub search_debounce_ms: u64,
    /// Maximum number of search results to display
    pub max_search_results: usize,
    /// Default width for side-by-side diff display
    pub default_width: usize,
}

/// Configuration for AI detection and analysis
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiConfig {
    /// How long to keep AI process information cached (seconds)
    pub process_cache_ttl_secs: u64,
    /// Maximum time gap for batch change detection (seconds)
    pub batch_time_gap_secs: u64,
    /// Maximum age for changes in batch detection (seconds)
    pub batch_max_age_secs: u64,
}

impl Default for WatchDiffConfig {
    fn default() -> Self {
        Self {
            watcher: WatcherConfig::default(),
            cache: CacheConfig::default(),
            ui: UiConfig::default(),
            ai: AiConfig::default(),
        }
    }
}

impl Default for WatcherConfig {
    fn default() -> Self {
        Self {
            event_debounce_ms: 100,
            max_events: 1000,
            max_event_age_secs: 3600, // 1 hour
            cleanup_interval_secs: 300, // 5 minutes
        }
    }
}

impl Default for CacheConfig {
    fn default() -> Self {
        Self {
            diff_cache_size: 100,
            process_cache_size: 50,
            batch_changes_limit: 100,
            cleanup_threshold: 0.8, // Cleanup when 80% full
        }
    }
}

impl Default for UiConfig {
    fn default() -> Self {
        Self {
            search_debounce_ms: 300,
            max_search_results: 1000,
            default_width: 120,
        }
    }
}

impl Default for AiConfig {
    fn default() -> Self {
        Self {
            process_cache_ttl_secs: 60, // 1 minute
            batch_time_gap_secs: 5,
            batch_max_age_secs: 30,
        }
    }
}

impl WatcherConfig {
    /// Get event debounce duration
    pub fn event_debounce_duration(&self) -> Duration {
        Duration::from_millis(self.event_debounce_ms)
    }
    
    /// Get max event age duration
    pub fn max_event_age_duration(&self) -> Duration {
        Duration::from_secs(self.max_event_age_secs)
    }
    
    /// Get cleanup interval duration
    pub fn cleanup_interval_duration(&self) -> Duration {
        Duration::from_secs(self.cleanup_interval_secs)
    }
}

impl UiConfig {
    /// Get search debounce duration
    pub fn search_debounce_duration(&self) -> Duration {
        Duration::from_millis(self.search_debounce_ms)
    }
}

impl AiConfig {
    /// Get process cache TTL duration
    pub fn process_cache_ttl_duration(&self) -> Duration {
        Duration::from_secs(self.process_cache_ttl_secs)
    }
    
    /// Get batch time gap duration
    pub fn batch_time_gap_duration(&self) -> Duration {
        Duration::from_secs(self.batch_time_gap_secs)
    }
    
    /// Get batch max age duration
    pub fn batch_max_age_duration(&self) -> Duration {
        Duration::from_secs(self.batch_max_age_secs)
    }
}

/// Configuration loading and management
impl WatchDiffConfig {
    /// Load configuration from file or use default
    pub fn load_or_default() -> Self {
        // Try to load from config file, fall back to default
        Self::default()
    }
    
    /// Load configuration from environment variables
    pub fn from_env() -> Self {
        let mut config = Self::default();
        
        // Override with environment variables if present
        if let Ok(val) = std::env::var("WATCHDIFF_DIFF_CACHE_SIZE") {
            if let Ok(size) = val.parse::<usize>() {
                config.cache.diff_cache_size = size;
            }
        }
        
        if let Ok(val) = std::env::var("WATCHDIFF_MAX_EVENTS") {
            if let Ok(max) = val.parse::<usize>() {
                config.watcher.max_events = max;
            }
        }
        
        if let Ok(val) = std::env::var("WATCHDIFF_EVENT_DEBOUNCE_MS") {
            if let Ok(ms) = val.parse::<u64>() {
                config.watcher.event_debounce_ms = ms;
            }
        }
        
        if let Ok(val) = std::env::var("WATCHDIFF_SEARCH_DEBOUNCE_MS") {
            if let Ok(ms) = val.parse::<u64>() {
                config.ui.search_debounce_ms = ms;
            }
        }
        
        config
    }
    
    /// Validate configuration values
    pub fn validate(&self) -> Result<(), String> {
        if self.cache.diff_cache_size == 0 {
            return Err("diff_cache_size must be greater than 0".to_string());
        }
        
        if self.watcher.max_events == 0 {
            return Err("max_events must be greater than 0".to_string());
        }
        
        if self.cache.cleanup_threshold <= 0.0 || self.cache.cleanup_threshold > 1.0 {
            return Err("cleanup_threshold must be between 0.0 and 1.0".to_string());
        }
        
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = WatchDiffConfig::default();
        
        assert_eq!(config.watcher.max_events, 1000);
        assert_eq!(config.cache.diff_cache_size, 100);
        assert_eq!(config.ui.search_debounce_ms, 300);
    }
    
    #[test]
    fn test_config_validation() {
        let mut config = WatchDiffConfig::default();
        assert!(config.validate().is_ok());
        
        config.cache.diff_cache_size = 0;
        assert!(config.validate().is_err());
        
        config.cache.diff_cache_size = 100;
        config.cache.cleanup_threshold = 1.5;
        assert!(config.validate().is_err());
    }
    
    #[test]
    fn test_duration_conversions() {
        let config = WatcherConfig::default();
        
        assert_eq!(config.event_debounce_duration(), Duration::from_millis(100));
        assert_eq!(config.max_event_age_duration(), Duration::from_secs(3600));
    }
    
    #[test]
    fn test_env_config_loading() {
        std::env::set_var("WATCHDIFF_DIFF_CACHE_SIZE", "200");
        std::env::set_var("WATCHDIFF_MAX_EVENTS", "2000");
        
        let config = WatchDiffConfig::from_env();
        
        assert_eq!(config.cache.diff_cache_size, 200);
        assert_eq!(config.watcher.max_events, 2000);
        
        // Cleanup
        std::env::remove_var("WATCHDIFF_DIFF_CACHE_SIZE");
        std::env::remove_var("WATCHDIFF_MAX_EVENTS");
    }
}