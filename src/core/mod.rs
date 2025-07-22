//! Core functionality module
//!
//! Contains file watching, filtering, and event handling

pub mod events;
pub mod watcher;
pub mod filter;

// Re-export main types
pub use events::{FileEvent, FileEventKind, HighlightedFileEvent, AppState, AppEvent};
pub use watcher::FileWatcher;
pub use filter::FileFilter;