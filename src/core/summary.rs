//! Change summary functionality for aggregating and presenting file events
//!
//! This module provides data structures and functions for creating summaries
//! of file changes, including statistics and aggregated views.

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{Duration, SystemTime};
use serde::{Deserialize, Serialize};

use super::{FileEvent, FileEventKind, ChangeOrigin, ConfidenceLevel};

/// Statistics about changes in a summary
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChangeSummaryStats {
    pub total_files: usize,
    pub files_created: usize,
    pub files_modified: usize,
    pub files_deleted: usize,
    pub files_moved: usize,
    pub total_changes: usize,
    pub time_span: Duration,
    pub earliest_change: Option<SystemTime>,
    pub latest_change: Option<SystemTime>,
}

/// Summary entry for a single file
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileSummaryEntry {
    pub path: PathBuf,
    pub change_type: FileEventKind,
    pub changed_at: SystemTime,
    pub changed_by: ChangeOrigin,
    pub confidence_level: Option<ConfidenceLevel>,
    pub batch_id: Option<String>,
    pub change_count: usize, // Number of times this file was changed
    pub has_diff: bool,
    pub preview: Option<String>,
    /// Reference to the most recent event for this file
    pub latest_event_idx: usize,
}

/// Time-based grouping options for summary
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SummaryTimeFrame {
    LastHour,
    LastDay,
    LastWeek,
    All,
    Custom(Duration),
}

/// Grouping options for summary display
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SummaryGrouping {
    ByFile,      // Group all changes by file path
    ByTime,      // Group changes by time periods
    ByOrigin,    // Group changes by who made them
    ByBatch,     // Group changes by batch ID
    Chronological, // Show all changes in chronological order
}

/// Filter options for summary
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SummaryFilters {
    pub time_frame: SummaryTimeFrame,
    pub grouping: SummaryGrouping,
    pub include_origins: Vec<ChangeOrigin>,
    pub exclude_origins: Vec<ChangeOrigin>,
    pub min_confidence: Option<ConfidenceLevel>,
    pub file_pattern: Option<String>, // Glob pattern for file paths
}

/// Complete change summary
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChangeSummary {
    pub stats: ChangeSummaryStats,
    pub files: Vec<FileSummaryEntry>,
    pub generated_at: SystemTime,
    pub filters: Option<String>, // JSON-serialized filters used
}

impl Default for SummaryFilters {
    fn default() -> Self {
        Self {
            time_frame: SummaryTimeFrame::LastDay,
            grouping: SummaryGrouping::ByFile,
            include_origins: vec![],
            exclude_origins: vec![],
            min_confidence: None,
            file_pattern: None,
        }
    }
}

impl SummaryTimeFrame {
    /// Get the duration for this time frame
    pub fn duration(&self) -> Option<Duration> {
        match self {
            SummaryTimeFrame::LastHour => Some(Duration::from_secs(3600)),
            SummaryTimeFrame::LastDay => Some(Duration::from_secs(86400)),
            SummaryTimeFrame::LastWeek => Some(Duration::from_secs(604800)),
            SummaryTimeFrame::All => None,
            SummaryTimeFrame::Custom(duration) => Some(*duration),
        }
    }
    
    /// Check if a timestamp falls within this time frame
    pub fn includes_time(&self, timestamp: SystemTime, now: SystemTime) -> bool {
        match self.duration() {
            Some(duration) => {
                if let Ok(elapsed) = now.duration_since(timestamp) {
                    elapsed <= duration
                } else {
                    false
                }
            }
            None => true, // All includes everything
        }
    }
}

impl ChangeSummary {
    /// Create a new empty summary
    pub fn new() -> Self {
        Self {
            stats: ChangeSummaryStats {
                total_files: 0,
                files_created: 0,
                files_modified: 0,
                files_deleted: 0,
                files_moved: 0,
                total_changes: 0,
                time_span: Duration::from_secs(0),
                earliest_change: None,
                latest_change: None,
            },
            files: Vec::new(),
            generated_at: SystemTime::now(),
            filters: None,
        }
    }
    
    /// Generate a summary from a collection of file events
    pub fn from_events(events: &[FileEvent], filters: &SummaryFilters) -> Self {
        let mut summary = Self::new();
        let now = SystemTime::now();
        
        // Store filters as JSON
        if let Ok(filters_json) = serde_json::to_string(filters) {
            summary.filters = Some(filters_json);
        }
        
        // Filter events based on criteria
        let filtered_events: Vec<&FileEvent> = events
            .iter()
            .filter(|event| {
                // Time frame filter
                if !filters.time_frame.includes_time(event.timestamp, now) {
                    return false;
                }
                
                // Origin filters
                if !filters.include_origins.is_empty() 
                    && !filters.include_origins.contains(&event.origin) {
                    return false;
                }
                
                if filters.exclude_origins.contains(&event.origin) {
                    return false;
                }
                
                // Confidence filter
                if let (Some(min_confidence), Some(ref confidence)) = (filters.min_confidence.as_ref(), &event.confidence) {
                    match (min_confidence, &confidence.level) {
                        (ConfidenceLevel::Safe, _) => {}, // Safe includes all
                        (ConfidenceLevel::Review, ConfidenceLevel::Risky) => return false,
                        (ConfidenceLevel::Risky, ConfidenceLevel::Review | ConfidenceLevel::Safe) => return false,
                        _ => {}
                    }
                }
                
                // File pattern filter (basic contains check for now)
                if let Some(ref pattern) = filters.file_pattern {
                    if !event.path.to_string_lossy().contains(pattern) {
                        return false;
                    }
                }
                
                true
            })
            .collect();
        
        // Group events by file path for aggregation
        let mut file_groups: HashMap<PathBuf, Vec<&FileEvent>> = HashMap::new();
        for event in &filtered_events {
            file_groups.entry(event.path.clone()).or_default().push(event);
        }
        
        // Generate summary entries
        for (path, file_events) in file_groups {
            // Get the most recent event for this file
            let latest_event = file_events
                .iter()
                .max_by_key(|e| e.timestamp)
                .unwrap(); // Safe because we know there's at least one event
                
            let entry = FileSummaryEntry {
                path,
                change_type: latest_event.kind.clone(),
                changed_at: latest_event.timestamp,
                changed_by: latest_event.origin.clone(),
                confidence_level: latest_event.confidence.as_ref().map(|c| c.level.clone()),
                batch_id: latest_event.batch_id.clone(),
                change_count: file_events.len(),
                has_diff: latest_event.diff.is_some(),
                preview: latest_event.content_preview.clone()
                    .or_else(|| latest_event.diff.as_ref().and_then(|d| {
                        // Create a short preview from diff
                        let lines: Vec<&str> = d.lines().take(3).collect();
                        if lines.is_empty() {
                            None
                        } else {
                            Some(lines.join("\n"))
                        }
                    })),
                latest_event_idx: 0, // Will be set properly during final processing
            };
            
            summary.files.push(entry);
        }
        
        // Sort files by most recent change
        summary.files.sort_by(|a, b| b.changed_at.cmp(&a.changed_at));
        
        // Calculate statistics
        summary.stats.total_files = summary.files.len();
        summary.stats.total_changes = filtered_events.len();
        
        for file in &summary.files {
            match file.change_type {
                FileEventKind::Created => summary.stats.files_created += 1,
                FileEventKind::Modified => summary.stats.files_modified += 1,
                FileEventKind::Deleted => summary.stats.files_deleted += 1,
                FileEventKind::Moved { .. } => summary.stats.files_moved += 1,
            }
        }
        
        // Calculate time span
        if let (Some(first), Some(last)) = (summary.files.last(), summary.files.first()) {
            summary.stats.earliest_change = Some(first.changed_at);
            summary.stats.latest_change = Some(last.changed_at);
            
            if let Ok(duration) = last.changed_at.duration_since(first.changed_at) {
                summary.stats.time_span = duration;
            }
        }
        
        summary
    }
    
    /// Get files filtered by change type
    pub fn files_by_type(&self, change_type: &FileEventKind) -> Vec<&FileSummaryEntry> {
        self.files
            .iter()
            .filter(|f| std::mem::discriminant(&f.change_type) == std::mem::discriminant(change_type))
            .collect()
    }
    
    /// Get files changed by a specific origin
    pub fn files_by_origin(&self, origin: &ChangeOrigin) -> Vec<&FileSummaryEntry> {
        self.files
            .iter()
            .filter(|f| std::mem::discriminant(&f.changed_by) == std::mem::discriminant(origin))
            .collect()
    }
    
    /// Get files with a specific confidence level
    pub fn files_by_confidence(&self, level: &ConfidenceLevel) -> Vec<&FileSummaryEntry> {
        self.files
            .iter()
            .filter(|f| f.confidence_level.as_ref() == Some(level))
            .collect()
    }
    
    /// Get summary of change types as percentages
    pub fn change_type_distribution(&self) -> HashMap<String, f32> {
        let mut distribution = HashMap::new();
        let total = self.stats.total_files as f32;
        
        if total > 0.0 {
            distribution.insert("Created".to_string(), 
                (self.stats.files_created as f32 / total) * 100.0);
            distribution.insert("Modified".to_string(), 
                (self.stats.files_modified as f32 / total) * 100.0);
            distribution.insert("Deleted".to_string(), 
                (self.stats.files_deleted as f32 / total) * 100.0);
            distribution.insert("Moved".to_string(), 
                (self.stats.files_moved as f32 / total) * 100.0);
        }
        
        distribution
    }
}

impl Default for ChangeSummary {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::events::{ChangeOrigin, ChangeConfidence, ConfidenceLevel};
    use std::time::{SystemTime, Duration};

    fn create_test_event(path: &str, kind: FileEventKind, origin: ChangeOrigin) -> FileEvent {
        FileEvent {
            path: PathBuf::from(path),
            kind,
            timestamp: SystemTime::now(),
            diff: Some("test diff".to_string()),
            content_preview: Some("test preview".to_string()),
            origin,
            confidence: Some(ChangeConfidence {
                level: ConfidenceLevel::Safe,
                score: 0.8,
                reasons: vec!["Test".to_string()],
            }),
            batch_id: None,
        }
    }

    #[test]
    fn test_empty_summary() {
        let summary = ChangeSummary::new();
        assert_eq!(summary.stats.total_files, 0);
        assert_eq!(summary.files.len(), 0);
    }

    #[test]
    fn test_summary_from_events() {
        let events = vec![
            create_test_event("file1.rs", FileEventKind::Created, ChangeOrigin::Human),
            create_test_event("file2.rs", FileEventKind::Modified, ChangeOrigin::Human),
            create_test_event("file1.rs", FileEventKind::Modified, ChangeOrigin::Human), // Second change to file1
        ];
        
        let filters = SummaryFilters::default();
        let summary = ChangeSummary::from_events(&events, &filters);
        
        assert_eq!(summary.stats.total_files, 2); // Two unique files
        assert_eq!(summary.stats.total_changes, 3); // Three total changes
        assert_eq!(summary.stats.files_created, 0); // No files end in "created" state (file1 was created then modified)
        assert_eq!(summary.stats.files_modified, 2); // Both files show as modified (latest state)
        
        // Check that file1 shows change_count = 2
        let file1_entry = summary.files.iter().find(|f| f.path == PathBuf::from("file1.rs"));
        assert!(file1_entry.is_some());
        assert_eq!(file1_entry.unwrap().change_count, 2);
    }

    #[test]
    fn test_time_frame_filtering() {
        let mut old_event = create_test_event("old.rs", FileEventKind::Created, ChangeOrigin::Human);
        old_event.timestamp = SystemTime::now() - Duration::from_secs(7200); // 2 hours ago
        
        let recent_event = create_test_event("recent.rs", FileEventKind::Created, ChangeOrigin::Human);
        
        let events = vec![old_event, recent_event];
        
        let mut filters = SummaryFilters::default();
        filters.time_frame = SummaryTimeFrame::LastHour;
        
        let summary = ChangeSummary::from_events(&events, &filters);
        
        assert_eq!(summary.stats.total_files, 1); // Only recent file
        assert_eq!(summary.files[0].path, PathBuf::from("recent.rs"));
    }

    #[test]
    fn test_origin_filtering() {
        let events = vec![
            create_test_event("human.rs", FileEventKind::Created, ChangeOrigin::Human),
            create_test_event("ai.rs", FileEventKind::Created, 
                ChangeOrigin::AIAgent { tool_name: "Claude".to_string(), process_id: Some(123) }),
        ];
        
        let mut filters = SummaryFilters::default();
        filters.include_origins = vec![ChangeOrigin::Human];
        
        let summary = ChangeSummary::from_events(&events, &filters);
        
        assert_eq!(summary.stats.total_files, 1);
        assert_eq!(summary.files[0].path, PathBuf::from("human.rs"));
    }

    #[test]
    fn test_change_type_distribution() {
        let events = vec![
            create_test_event("file1.rs", FileEventKind::Created, ChangeOrigin::Human),
            create_test_event("file2.rs", FileEventKind::Created, ChangeOrigin::Human),
            create_test_event("file3.rs", FileEventKind::Modified, ChangeOrigin::Human),
            create_test_event("file4.rs", FileEventKind::Deleted, ChangeOrigin::Human),
        ];
        
        let filters = SummaryFilters::default();
        let summary = ChangeSummary::from_events(&events, &filters);
        let distribution = summary.change_type_distribution();
        
        assert_eq!(distribution.get("Created").unwrap(), &50.0); // 2/4 = 50%
        assert_eq!(distribution.get("Modified").unwrap(), &25.0); // 1/4 = 25%
        assert_eq!(distribution.get("Deleted").unwrap(), &25.0); // 1/4 = 25%
    }

    #[test]
    fn test_files_by_type() {
        let events = vec![
            create_test_event("created1.rs", FileEventKind::Created, ChangeOrigin::Human),
            create_test_event("created2.rs", FileEventKind::Created, ChangeOrigin::Human),
            create_test_event("modified.rs", FileEventKind::Modified, ChangeOrigin::Human),
        ];
        
        let filters = SummaryFilters::default();
        let summary = ChangeSummary::from_events(&events, &filters);
        
        let created_files = summary.files_by_type(&FileEventKind::Created);
        assert_eq!(created_files.len(), 2);
        
        let modified_files = summary.files_by_type(&FileEventKind::Modified);
        assert_eq!(modified_files.len(), 1);
    }
}