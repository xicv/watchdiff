//! WatchDiff - A file watching and diff visualization tool
//! 
//! This library provides functionality for watching file changes and displaying
//! diffs in various formats, including a terminal user interface.
//!
//! ## Architecture
//! 
//! The library is organized into several modules:
//! 
//! - `core`: File watching, filtering, and event handling
//! - `diff`: Diff generation with multiple algorithms and formatting
//! - `ui`: Terminal user interface components
//! - `export`: Export functionality for patches and diffs
//! - `highlight`: Syntax highlighting support
//! - `cli`: Command-line interface handling

pub mod ai;
pub mod cli;
pub mod core;
pub mod diff;
pub mod export;
pub mod highlight;
pub mod performance;
pub mod review;
pub mod ui;

// Re-export commonly used types for backward compatibility
pub use core::{AppState, FileEvent, FileEventKind, HighlightedFileEvent, FileWatcher, AppEvent};
pub use core::{ChangeOrigin, ChangeConfidence, ConfidenceLevel};
pub use ai::{AIDetector, ConfidenceScorer};
pub use review::{ReviewSession, ReviewableChange, ReviewAction, ReviewFilters, ReviewNavigationAction, ReviewFilterPreset};
pub use ui::{TuiApp, setup_terminal, restore_terminal};
pub use diff::{DiffGenerator, DiffAlgorithmType, DiffFormatter, DiffFormat};