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
pub struct FileEvent {
    pub path: PathBuf,
    pub kind: FileEventKind,
    pub timestamp: SystemTime,
    pub diff: Option<String>,
    pub content_preview: Option<String>,
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
}

impl FileEvent {
    pub fn new(path: PathBuf, kind: FileEventKind) -> Self {
        Self {
            path,
            kind,
            timestamp: SystemTime::now(),
            diff: None,
            content_preview: None,
        }
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