use std::path::PathBuf;
use std::time::SystemTime;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum FileEventKind {
    Created,
    Modified,
    Deleted,
    Moved { from: PathBuf, to: PathBuf },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
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
    pub events: Vec<FileEvent>,
    pub highlighted_events: Vec<HighlightedFileEvent>,
    pub scroll_offset: usize,
    pub max_events: usize,
    pub show_help: bool,
    pub watched_files: std::collections::HashSet<PathBuf>,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            events: Vec::new(),
            highlighted_events: Vec::new(),
            scroll_offset: 0,
            max_events: 1000,
            show_help: false,
            watched_files: std::collections::HashSet::new(),
        }
    }
}

impl AppState {
    pub fn add_event(&mut self, event: FileEvent) {
        // Convert to highlighted event
        let highlighted = event.to_highlighted();
        
        // Add to both collections
        self.events.insert(0, event);
        self.highlighted_events.insert(0, highlighted);
        
        // Maintain size limits
        if self.events.len() > self.max_events {
            self.events.truncate(self.max_events);
        }
        if self.highlighted_events.len() > self.max_events {
            self.highlighted_events.truncate(self.max_events);
        }
        
        self.scroll_offset = 0;
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

    pub fn get_visible_events(&self, height: usize) -> &[FileEvent] {
        let start = self.scroll_offset;
        let end = (start + height).min(self.events.len());
        &self.events[start..end]
    }

    pub fn get_visible_highlighted_events(&self, height: usize) -> &[HighlightedFileEvent] {
        let start = self.scroll_offset;
        let end = (start + height).min(self.highlighted_events.len());
        &self.highlighted_events[start..end]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::SystemTime;

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
}