//! User interface module
//! 
//! Contains TUI and other interface-related functionality

pub mod tui;

// Re-export main types
pub use tui::{TuiApp, setup_terminal, restore_terminal};