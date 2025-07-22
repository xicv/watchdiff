use std::path::PathBuf;
use clap::{Parser, ValueEnum};

#[derive(Parser)]
#[command(name = "watchdiff")]
#[command(author = "WatchDiff Team")]
#[command(version = "0.1.0")]
#[command(about = "A high-performance file watcher with beautiful TUI showing real-time diffs")]
#[command(long_about = "WatchDiff monitors file changes in real-time, respects .gitignore patterns, and displays beautiful diffs in a terminal user interface. Perfect for development workflow monitoring.")]
pub struct Cli {
    /// Directory to watch for changes
    #[arg(value_name = "PATH", help = "Path to watch (defaults to current directory)")]
    pub path: Option<PathBuf>,

    /// Watch mode - how to handle file events
    #[arg(short, long, default_value = "auto", help = "File watching mode")]
    pub mode: WatchMode,

    /// Maximum number of events to keep in memory
    #[arg(long, default_value = "1000", help = "Maximum events to store")]
    pub max_events: usize,

    /// Enable verbose logging
    #[arg(short, long, help = "Enable verbose output")]
    pub verbose: bool,

    /// Disable colors in output
    #[arg(long, help = "Disable colored output")]
    pub no_color: bool,

    /// Show only specific file types
    #[arg(long, value_delimiter = ',', help = "File extensions to watch (e.g., rs,py,js)")]
    pub extensions: Option<Vec<String>>,

    /// Ignore additional patterns beyond .gitignore
    #[arg(long, value_delimiter = ',', help = "Additional patterns to ignore")]
    pub ignore: Option<Vec<String>>,

    /// Diff context lines
    #[arg(long, default_value = "3", help = "Number of context lines in diffs")]
    pub context: usize,

    /// Output format for non-TUI mode
    #[arg(long, default_value = "tui", help = "Output format")]
    pub output: OutputFormat,

    /// Polling interval in milliseconds (for polling mode)
    #[arg(long, default_value = "1000", help = "Polling interval in ms")]
    pub poll_interval: u64,
}

#[derive(Debug, Clone, ValueEnum)]
pub enum WatchMode {
    /// Automatic detection (native events with polling fallback)
    Auto,
    /// Use native file system events
    Native,
    /// Use polling-based watching
    Polling,
}

#[derive(Debug, Clone, ValueEnum)]
pub enum OutputFormat {
    /// Terminal user interface (default)
    Tui,
    /// JSON output for scripting
    Json,
    /// Plain text output
    Text,
    /// Compact single-line format
    Compact,
}

impl Cli {
    pub fn get_watch_path(&self) -> PathBuf {
        self.path.clone().unwrap_or_else(|| {
            std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
        })
    }

    pub fn should_watch_extension(&self, path: &std::path::Path) -> bool {
        if let Some(ref extensions) = self.extensions {
            if let Some(ext) = path.extension().and_then(|s| s.to_str()) {
                extensions.iter().any(|e| e.eq_ignore_ascii_case(ext))
            } else {
                false
            }
        } else {
            true // Watch all files if no extensions specified
        }
    }

    pub fn get_ignore_patterns(&self) -> Vec<String> {
        self.ignore.clone().unwrap_or_default()
    }

    pub fn setup_logging(&self) {
        let level = if self.verbose {
            tracing::Level::DEBUG
        } else {
            tracing::Level::INFO
        };

        tracing_subscriber::fmt()
            .with_max_level(level)
            .with_target(false)
            .with_thread_ids(false)
            .with_file(false)
            .with_line_number(false)
            .init();
    }

    pub fn validate(&self) -> Result<(), String> {
        let path = self.get_watch_path();
        
        if !path.exists() {
            return Err(format!("Path does not exist: {}", path.display()));
        }

        if !path.is_dir() {
            return Err(format!("Path is not a directory: {}", path.display()));
        }

        if self.max_events == 0 {
            return Err("Max events must be greater than 0".to_string());
        }

        if self.poll_interval == 0 {
            return Err("Poll interval must be greater than 0".to_string());
        }

        Ok(())
    }
}

impl Default for Cli {
    fn default() -> Self {
        Self {
            path: None,
            mode: WatchMode::Auto,
            max_events: 1000,
            verbose: false,
            no_color: false,
            extensions: None,
            ignore: None,
            context: 3,
            output: OutputFormat::Tui,
            poll_interval: 1000,
        }
    }
}