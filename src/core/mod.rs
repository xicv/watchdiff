//! Core functionality module
//!
//! Contains file watching, filtering, and event handling

pub mod events;
pub mod watcher;
pub mod filter;
pub mod summary;

// Re-export main types
pub use events::{FileEvent, FileEventKind, HighlightedFileEvent, AppState, AppEvent};
pub use events::{ChangeOrigin, ChangeConfidence, ConfidenceLevel};
pub use watcher::FileWatcher;
pub use filter::FileFilter;
pub use summary::{ChangeSummary, ChangeSummaryStats, FileSummaryEntry, SummaryFilters, SummaryTimeFrame, SummaryGrouping};