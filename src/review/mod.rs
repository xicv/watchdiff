use std::collections::HashMap;
use std::path::PathBuf;
use std::fs;
use std::io;
use crate::core::{FileEvent, ConfidenceLevel, ChangeOrigin};
use serde::{Deserialize, Serialize};
use regex::Regex;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ReviewAction {
    Accept,
    Reject,
    Skip,
    Pending,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum HunkType {
    Addition,
    Deletion,
    Modification,
    Context,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiffHunk {
    pub id: String,
    pub hunk_type: HunkType,
    pub old_start: usize,
    pub old_count: usize,
    pub new_start: usize,
    pub new_count: usize,
    pub lines: Vec<String>,
    pub header: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReviewableChange {
    pub event: FileEvent,
    pub hunks: Vec<DiffHunk>,
    pub review_actions: HashMap<String, ReviewAction>, // hunk_id -> action
    pub overall_action: ReviewAction,
    pub reviewed_at: Option<std::time::SystemTime>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReviewSession {
    pub id: String,
    pub started_at: std::time::SystemTime,
    pub changes: Vec<ReviewableChange>,
    pub current_change_index: usize,
    pub current_hunk_index: usize,
    pub filters: ReviewFilters,
    pub snapshot_path: Option<PathBuf>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReviewFilters {
    pub confidence_level: Option<ConfidenceLevel>,
    pub confidence_threshold: Option<f32>, // 0.0 - 1.0
    pub show_only_risky: bool,
    pub show_only_ai_changes: bool,
    pub origin_filter: Option<ChangeOrigin>,
    pub file_pattern: Option<String>,
    pub file_regex: Option<String>,
    pub batch_filter: Option<String>,
    pub min_hunks: Option<usize>,
    pub max_hunks: Option<usize>,
    pub exclude_reviewed: bool,
    pub show_only_pending: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReviewFilterPreset {
    pub name: String,
    pub description: String,
    pub filters: ReviewFilters,
    pub shortcut_key: Option<char>,
}

#[derive(Debug, Clone)]
pub enum ReviewNavigationAction {
    NextChange,
    PreviousChange,
    NextHunk,
    PreviousHunk,
    NextRiskyChange,
    FirstUnreviewed,
    JumpToFile(PathBuf),
}

impl Default for ReviewFilters {
    fn default() -> Self {
        Self {
            confidence_level: None,
            confidence_threshold: None,
            show_only_risky: false,
            show_only_ai_changes: false,
            origin_filter: None,
            file_pattern: None,
            file_regex: None,
            batch_filter: None,
            min_hunks: None,
            max_hunks: None,
            exclude_reviewed: false,
            show_only_pending: false,
        }
    }
}

impl ReviewableChange {
    pub fn new(event: FileEvent) -> Self {
        let hunks = Self::parse_diff_into_hunks(&event.diff);
        let mut review_actions = HashMap::new();
        
        // Initialize all hunks as pending
        for hunk in &hunks {
            review_actions.insert(hunk.id.clone(), ReviewAction::Pending);
        }
        
        Self {
            event,
            hunks,
            review_actions,
            overall_action: ReviewAction::Pending,
            reviewed_at: None,
        }
    }
    
    pub fn accept_hunk(&mut self, hunk_id: &str) {
        self.review_actions.insert(hunk_id.to_string(), ReviewAction::Accept);
        self.update_overall_action();
    }
    
    pub fn reject_hunk(&mut self, hunk_id: &str) {
        self.review_actions.insert(hunk_id.to_string(), ReviewAction::Reject);
        self.update_overall_action();
    }
    
    pub fn skip_hunk(&mut self, hunk_id: &str) {
        self.review_actions.insert(hunk_id.to_string(), ReviewAction::Skip);
        self.update_overall_action();
    }
    
    pub fn accept_all(&mut self) {
        for hunk in &self.hunks {
            self.review_actions.insert(hunk.id.clone(), ReviewAction::Accept);
        }
        self.overall_action = ReviewAction::Accept;
        self.reviewed_at = Some(std::time::SystemTime::now());
    }
    
    pub fn reject_all(&mut self) {
        for hunk in &self.hunks {
            self.review_actions.insert(hunk.id.clone(), ReviewAction::Reject);
        }
        self.overall_action = ReviewAction::Reject;
        self.reviewed_at = Some(std::time::SystemTime::now());
    }
    
    fn update_overall_action(&mut self) {
        let actions: Vec<&ReviewAction> = self.review_actions.values().collect();
        
        if actions.iter().all(|&a| matches!(a, ReviewAction::Accept)) {
            self.overall_action = ReviewAction::Accept;
            self.reviewed_at = Some(std::time::SystemTime::now());
        } else if actions.iter().all(|&a| matches!(a, ReviewAction::Reject)) {
            self.overall_action = ReviewAction::Reject;
            self.reviewed_at = Some(std::time::SystemTime::now());
        } else if actions.iter().all(|&a| matches!(a, ReviewAction::Skip)) {
            self.overall_action = ReviewAction::Skip;
            self.reviewed_at = Some(std::time::SystemTime::now());
        } else if actions.iter().any(|&a| !matches!(a, ReviewAction::Pending)) {
            // Partially reviewed
            self.overall_action = ReviewAction::Pending;
        }
    }
    
    pub fn is_high_risk(&self) -> bool {
        if let Some(ref confidence) = self.event.confidence {
            matches!(confidence.level, ConfidenceLevel::Risky)
        } else {
            false
        }
    }
    
    pub fn is_ai_generated(&self) -> bool {
        matches!(self.event.origin, crate::core::ChangeOrigin::AIAgent { .. })
    }
    
    pub fn matches_filter(&self, filter: &ReviewFilters) -> bool {
        // Check confidence level filter
        if let Some(required_level) = &filter.confidence_level {
            if let Some(ref confidence) = self.event.confidence {
                if confidence.level != *required_level {
                    return false;
                }
            } else {
                return false;
            }
        }
        
        // Check confidence threshold filter
        if let Some(threshold) = filter.confidence_threshold {
            if let Some(ref confidence) = self.event.confidence {
                if confidence.score < threshold {
                    return false;
                }
            } else {
                return false;
            }
        }
        
        // Check risky-only filter
        if filter.show_only_risky && !self.is_high_risk() {
            return false;
        }
        
        // Check AI-only filter
        if filter.show_only_ai_changes && !self.is_ai_generated() {
            return false;
        }
        
        // Check origin filter
        if let Some(ref required_origin) = filter.origin_filter {
            if !self.matches_origin_filter(required_origin) {
                return false;
            }
        }
        
        // Check file pattern filter
        if let Some(ref pattern) = filter.file_pattern {
            let file_name = self.event.path.file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("");
            if !file_name.contains(pattern) {
                return false;
            }
        }
        
        // Check regex filter
        if let Some(ref regex_pattern) = filter.file_regex {
            if let Ok(regex) = Regex::new(regex_pattern) {
                let file_path = self.event.path.to_string_lossy();
                if !regex.is_match(&file_path) {
                    return false;
                }
            }
        }
        
        // Check batch filter
        if let Some(ref batch_pattern) = filter.batch_filter {
            if let Some(ref batch_id) = self.event.batch_id {
                if !batch_id.contains(batch_pattern) {
                    return false;
                }
            } else {
                return false;
            }
        }
        
        // Check hunk count filters
        let hunk_count = self.hunks.len();
        if let Some(min_hunks) = filter.min_hunks {
            if hunk_count < min_hunks {
                return false;
            }
        }
        if let Some(max_hunks) = filter.max_hunks {
            if hunk_count > max_hunks {
                return false;
            }
        }
        
        // Check review status filters
        if filter.exclude_reviewed && !matches!(self.overall_action, ReviewAction::Pending) {
            return false;
        }
        if filter.show_only_pending && !matches!(self.overall_action, ReviewAction::Pending) {
            return false;
        }
        
        true
    }
    
    fn matches_origin_filter(&self, required_origin: &ChangeOrigin) -> bool {
        match (required_origin, &self.event.origin) {
            (ChangeOrigin::Human, ChangeOrigin::Human) => true,
            (ChangeOrigin::AIAgent { .. }, ChangeOrigin::AIAgent { .. }) => true,
            (ChangeOrigin::Tool { .. }, ChangeOrigin::Tool { .. }) => true,
            (ChangeOrigin::Unknown, ChangeOrigin::Unknown) => true,
            _ => false,
        }
    }
    
    fn parse_diff_into_hunks(diff: &Option<String>) -> Vec<DiffHunk> {
        let mut hunks = Vec::new();
        
        if let Some(diff_content) = diff {
            let lines: Vec<&str> = diff_content.lines().collect();
            let mut current_hunk: Option<DiffHunk> = None;
            let mut hunk_counter = 0;
            
            for line in lines {
                if line.starts_with("@@") {
                    // Save previous hunk if exists
                    if let Some(hunk) = current_hunk.take() {
                        hunks.push(hunk);
                    }
                    
                    // Parse hunk header: @@ -old_start,old_count +new_start,new_count @@
                    let hunk_id = format!("hunk_{}", hunk_counter);
                    hunk_counter += 1;
                    
                    let (old_start, old_count, new_start, new_count) = 
                        Self::parse_hunk_header(line);
                    
                    current_hunk = Some(DiffHunk {
                        id: hunk_id,
                        hunk_type: HunkType::Modification,
                        old_start,
                        old_count,
                        new_start,
                        new_count,
                        lines: Vec::new(),
                        header: line.to_string(),
                    });
                } else if let Some(ref mut hunk) = current_hunk {
                    hunk.lines.push(line.to_string());
                    
                    // Determine hunk type based on content
                    if line.starts_with('+') && !line.starts_with("+++") {
                        hunk.hunk_type = HunkType::Addition;
                    } else if line.starts_with('-') && !line.starts_with("---") {
                        hunk.hunk_type = HunkType::Deletion;
                    }
                }
            }
            
            // Save last hunk
            if let Some(hunk) = current_hunk {
                hunks.push(hunk);
            }
        }
        
        hunks
    }
    
    fn parse_hunk_header(header: &str) -> (usize, usize, usize, usize) {
        // Parse @@ -old_start,old_count +new_start,new_count @@
        let parts: Vec<&str> = header.split_whitespace().collect();
        let mut old_start = 1;
        let mut old_count = 1;
        let mut new_start = 1;
        let mut new_count = 1;
        
        for part in parts {
            if part.starts_with('-') {
                let old_part = &part[1..];
                if let Some((start, count)) = old_part.split_once(',') {
                    old_start = start.parse().unwrap_or(1);
                    old_count = count.parse().unwrap_or(1);
                } else {
                    old_start = old_part.parse().unwrap_or(1);
                }
            } else if part.starts_with('+') {
                let new_part = &part[1..];
                if let Some((start, count)) = new_part.split_once(',') {
                    new_start = start.parse().unwrap_or(1);
                    new_count = count.parse().unwrap_or(1);
                } else {
                    new_start = new_part.parse().unwrap_or(1);
                }
            }
        }
        
        (old_start, old_count, new_start, new_count)
    }
}

impl ReviewSession {
    pub fn new() -> Self {
        Self {
            id: format!("session_{}", std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis()),
            started_at: std::time::SystemTime::now(),
            changes: Vec::new(),
            current_change_index: 0,
            current_hunk_index: 0,
            filters: ReviewFilters::default(),
            snapshot_path: None,
        }
    }
    
    /// Create a new session with a specific ID for loading
    pub fn with_id(id: String) -> Self {
        Self {
            id,
            started_at: std::time::SystemTime::now(),
            changes: Vec::new(),
            current_change_index: 0,
            current_hunk_index: 0,
            filters: ReviewFilters::default(),
            snapshot_path: None,
        }
    }
    
    /// Save session to disk
    pub fn save_to_disk(&self, base_dir: &std::path::Path) -> io::Result<PathBuf> {
        let sessions_dir = base_dir.join(".watchdiff").join("sessions");
        fs::create_dir_all(&sessions_dir)?;
        
        let session_file = sessions_dir.join(format!("{}.json", self.id));
        let session_json = serde_json::to_string_pretty(self)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        
        fs::write(&session_file, session_json)?;
        Ok(session_file)
    }
    
    /// Load session from disk
    pub fn load_from_disk(base_dir: &std::path::Path, session_id: &str) -> io::Result<Self> {
        let session_file = base_dir.join(".watchdiff").join("sessions").join(format!("{}.json", session_id));
        let session_json = fs::read_to_string(session_file)?;
        let session: ReviewSession = serde_json::from_str(&session_json)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        Ok(session)
    }
    
    /// List all saved sessions
    pub fn list_saved_sessions(base_dir: &std::path::Path) -> io::Result<Vec<String>> {
        let sessions_dir = base_dir.join(".watchdiff").join("sessions");
        if !sessions_dir.exists() {
            return Ok(Vec::new());
        }
        
        let mut sessions = Vec::new();
        for entry in fs::read_dir(sessions_dir)? {
            let entry = entry?;
            if let Some(file_name) = entry.file_name().to_str() {
                if file_name.ends_with(".json") {
                    let session_id = file_name.trim_end_matches(".json");
                    sessions.push(session_id.to_string());
                }
            }
        }
        Ok(sessions)
    }
    
    /// Delete a saved session
    pub fn delete_session(base_dir: &std::path::Path, session_id: &str) -> io::Result<()> {
        let session_file = base_dir.join(".watchdiff").join("sessions").join(format!("{}.json", session_id));
        if session_file.exists() {
            fs::remove_file(session_file)?;
        }
        Ok(())
    }
    
    /// Apply a filter preset
    pub fn apply_filter_preset(&mut self, preset: &ReviewFilterPreset) {
        self.filters = preset.filters.clone();
    }
    
    /// Get default filter presets
    pub fn get_default_presets() -> Vec<ReviewFilterPreset> {
        vec![
            ReviewFilterPreset {
                name: "Risky Changes".to_string(),
                description: "Show only high-risk changes that need careful review".to_string(),
                filters: ReviewFilters {
                    show_only_risky: true,
                    exclude_reviewed: true,
                    ..Default::default()
                },
                shortcut_key: Some('1'),
            },
            ReviewFilterPreset {
                name: "AI Changes".to_string(),
                description: "Show only changes made by AI agents".to_string(),
                filters: ReviewFilters {
                    show_only_ai_changes: true,
                    exclude_reviewed: true,
                    ..Default::default()
                },
                shortcut_key: Some('2'),
            },
            ReviewFilterPreset {
                name: "Pending Review".to_string(),
                description: "Show only changes that haven't been reviewed yet".to_string(),
                filters: ReviewFilters {
                    show_only_pending: true,
                    ..Default::default()
                },
                shortcut_key: Some('3'),
            },
            ReviewFilterPreset {
                name: "Low Confidence".to_string(),
                description: "Show changes with confidence below 50%".to_string(),
                filters: ReviewFilters {
                    confidence_threshold: Some(0.5),
                    exclude_reviewed: true,
                    ..Default::default()
                },
                shortcut_key: Some('4'),
            },
            ReviewFilterPreset {
                name: "Large Changes".to_string(),
                description: "Show changes with many hunks (>5)".to_string(),
                filters: ReviewFilters {
                    min_hunks: Some(5),
                    exclude_reviewed: true,
                    ..Default::default()
                },
                shortcut_key: Some('5'),
            },
        ]
    }
    
    pub fn add_change(&mut self, event: FileEvent) {
        let reviewable = ReviewableChange::new(event);
        self.changes.push(reviewable);
    }
    
    pub fn get_current_change(&self) -> Option<&ReviewableChange> {
        self.changes.get(self.current_change_index)
    }
    
    pub fn get_current_change_mut(&mut self) -> Option<&mut ReviewableChange> {
        self.changes.get_mut(self.current_change_index)
    }
    
    pub fn get_current_hunk(&self) -> Option<&DiffHunk> {
        self.get_current_change()?
            .hunks
            .get(self.current_hunk_index)
    }
    
    pub fn navigate(&mut self, action: ReviewNavigationAction) -> bool {
        match action {
            ReviewNavigationAction::NextChange => {
                if self.current_change_index + 1 < self.changes.len() {
                    self.current_change_index += 1;
                    self.current_hunk_index = 0;
                    true
                } else {
                    false
                }
            }
            ReviewNavigationAction::PreviousChange => {
                if self.current_change_index > 0 {
                    self.current_change_index -= 1;
                    self.current_hunk_index = 0;
                    true
                } else {
                    false
                }
            }
            ReviewNavigationAction::NextHunk => {
                if let Some(current_change) = self.get_current_change() {
                    if self.current_hunk_index + 1 < current_change.hunks.len() {
                        self.current_hunk_index += 1;
                        true
                    } else {
                        // Move to next change
                        self.navigate(ReviewNavigationAction::NextChange)
                    }
                } else {
                    false
                }
            }
            ReviewNavigationAction::PreviousHunk => {
                if self.current_hunk_index > 0 {
                    self.current_hunk_index -= 1;
                    true
                } else if self.current_change_index > 0 {
                    // Move to previous change, last hunk
                    self.current_change_index -= 1;
                    if let Some(prev_change) = self.get_current_change() {
                        self.current_hunk_index = prev_change.hunks.len().saturating_sub(1);
                    }
                    true
                } else {
                    false
                }
            }
            ReviewNavigationAction::NextRiskyChange => {
                for i in (self.current_change_index + 1)..self.changes.len() {
                    if self.changes[i].is_high_risk() {
                        self.current_change_index = i;
                        self.current_hunk_index = 0;
                        return true;
                    }
                }
                false
            }
            ReviewNavigationAction::FirstUnreviewed => {
                for i in 0..self.changes.len() {
                    if matches!(self.changes[i].overall_action, ReviewAction::Pending) {
                        self.current_change_index = i;
                        self.current_hunk_index = 0;
                        return true;
                    }
                }
                false
            }
            ReviewNavigationAction::JumpToFile(target_path) => {
                for (i, change) in self.changes.iter().enumerate() {
                    if change.event.path == target_path {
                        self.current_change_index = i;
                        self.current_hunk_index = 0;
                        return true;
                    }
                }
                false
            }
        }
    }
    
    pub fn get_filtered_changes(&self) -> Vec<(usize, &ReviewableChange)> {
        self.changes
            .iter()
            .enumerate()
            .filter(|(_, change)| change.matches_filter(&self.filters))
            .collect()
    }
    
    pub fn get_review_stats(&self) -> ReviewStats {
        let total = self.changes.len();
        let accepted = self.changes.iter()
            .filter(|c| matches!(c.overall_action, ReviewAction::Accept))
            .count();
        let rejected = self.changes.iter()
            .filter(|c| matches!(c.overall_action, ReviewAction::Reject))
            .count();
        let skipped = self.changes.iter()
            .filter(|c| matches!(c.overall_action, ReviewAction::Skip))
            .count();
        let pending = total - accepted - rejected - skipped;
        
        ReviewStats {
            total,
            accepted,
            rejected,
            skipped,
            pending,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ReviewStats {
    pub total: usize,
    pub accepted: usize,
    pub rejected: usize,
    pub skipped: usize,
    pub pending: usize,
}

impl ReviewStats {
    pub fn completion_percentage(&self) -> f32 {
        if self.total == 0 {
            100.0
        } else {
            ((self.total - self.pending) as f32 / self.total as f32) * 100.0
        }
    }
}