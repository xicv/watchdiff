use clap::Parser;
use anyhow::Result;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use watchdiff_tui::{
    cli::{Cli, OutputFormat},
    tui::{setup_terminal, restore_terminal, TuiApp},
    watcher::FileWatcher,
    AppEvent,
};

fn main() -> Result<()> {
    let cli = Cli::parse();
    
    if let Err(err) = cli.validate() {
        eprintln!("Error: {}", err);
        std::process::exit(1);
    }

    cli.setup_logging();

    let watch_path = cli.get_watch_path();
    tracing::info!("Starting WatchDiff on: {}", watch_path.display());

    match cli.output {
        OutputFormat::Tui => run_tui_mode(&cli)?,
        OutputFormat::Json => run_json_mode(&cli)?,
        OutputFormat::Text => run_text_mode(&cli)?,
        OutputFormat::Compact => run_compact_mode(&cli)?,
    }

    Ok(())
}

fn run_tui_mode(cli: &Cli) -> Result<()> {
    let watch_path = cli.get_watch_path();
    
    // Create file watcher
    let watcher = FileWatcher::new(&watch_path)?;
    
    // Setup terminal
    let mut terminal = setup_terminal()?;
    
    // Create TUI app
    let app = TuiApp::new(watcher);
    
    // Run the application
    let res = app.run(&mut terminal);
    
    // Restore terminal
    if let Err(err) = restore_terminal(&mut terminal) {
        eprintln!("Failed to restore terminal: {}", err);
    }
    
    if let Err(err) = res {
        eprintln!("Application error: {}", err);
        std::process::exit(1);
    }
    
    Ok(())
}

fn run_json_mode(cli: &Cli) -> Result<()> {
    let watch_path = cli.get_watch_path();
    let watcher = FileWatcher::new(&watch_path)?;
    
    let running = Arc::new(AtomicBool::new(true));
    let r = running.clone();
    ctrlc::set_handler(move || {
        r.store(false, Ordering::SeqCst);
    })?;
    
    while running.load(Ordering::SeqCst) {
        match watcher.recv_timeout(Duration::from_millis(100)) {
            Ok(AppEvent::FileChanged(event)) => {
                if should_include_file(&event.path, cli) {
                    println!("{}", serde_json::to_string(&event)?);
                }
            }
            Ok(AppEvent::Quit) => break,
            Ok(_) => continue, // Ignore other events
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => continue,
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
    
    Ok(())
}

fn run_text_mode(cli: &Cli) -> Result<()> {
    let watch_path = cli.get_watch_path();
    let watcher = FileWatcher::new(&watch_path)?;
    
    println!("Watching: {}", watch_path.display());
    println!("Press Ctrl+C to quit");
    println!("---");
    
    let running = Arc::new(AtomicBool::new(true));
    let r = running.clone();
    ctrlc::set_handler(move || {
        r.store(false, Ordering::SeqCst);
    })?;
    
    while running.load(Ordering::SeqCst) {
        match watcher.recv_timeout(Duration::from_millis(100)) {
            Ok(AppEvent::FileChanged(event)) => {
                if should_include_file(&event.path, cli) {
                    print_text_event(&event, cli);
                }
            }
            Ok(AppEvent::Quit) => break,
            Ok(_) => continue, // Ignore other events
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => continue,
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
    
    Ok(())
}

fn run_compact_mode(cli: &Cli) -> Result<()> {
    let watch_path = cli.get_watch_path();
    let watcher = FileWatcher::new(&watch_path)?;
    
    let running = Arc::new(AtomicBool::new(true));
    let r = running.clone();
    ctrlc::set_handler(move || {
        r.store(false, Ordering::SeqCst);
    })?;
    
    while running.load(Ordering::SeqCst) {
        match watcher.recv_timeout(Duration::from_millis(100)) {
            Ok(AppEvent::FileChanged(event)) => {
                if should_include_file(&event.path, cli) {
                    print_compact_event(&event);
                }
            }
            Ok(AppEvent::Quit) => break,
            Ok(_) => continue, // Ignore other events
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => continue,
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
    
    Ok(())
}

fn should_include_file(path: &std::path::Path, cli: &Cli) -> bool {
    cli.should_watch_extension(path)
}

fn print_text_event(event: &watchdiff_tui::FileEvent, cli: &Cli) {
    use watchdiff_tui::FileEventKind;
    
    let timestamp = event.timestamp
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    
    let time_str = format!("{:02}:{:02}:{:02}", 
        (timestamp % 86400) / 3600,
        (timestamp % 3600) / 60,
        timestamp % 60
    );

    let event_type = match &event.kind {
        FileEventKind::Created => "CREATED",
        FileEventKind::Modified => "MODIFIED",
        FileEventKind::Deleted => "DELETED",
        FileEventKind::Moved { .. } => "MOVED",
    };

    if cli.no_color {
        println!("[{}] {} {}", time_str, event_type, event.path.display());
    } else {
        let color = match &event.kind {
            FileEventKind::Created => "\x1b[32m",   // Green
            FileEventKind::Modified => "\x1b[33m",  // Yellow
            FileEventKind::Deleted => "\x1b[31m",   // Red
            FileEventKind::Moved { .. } => "\x1b[34m", // Blue
        };
        println!("[{}] {}{}\x1b[0m {}", time_str, color, event_type, event.path.display());
    }

    if let Some(diff) = &event.diff {
        for line in diff.lines().take(10) {
            if cli.no_color {
                println!("  {}", line);
            } else if line.starts_with('+') {
                println!("  \x1b[32m{}\x1b[0m", line);
            } else if line.starts_with('-') {
                println!("  \x1b[31m{}\x1b[0m", line);
            } else {
                println!("  {}", line);
            }
        }
    }
    
    println!();
}

fn print_compact_event(event: &watchdiff_tui::FileEvent) {
    use watchdiff_tui::FileEventKind;
    
    let event_type = match &event.kind {
        FileEventKind::Created => "C",
        FileEventKind::Modified => "M",
        FileEventKind::Deleted => "D",
        FileEventKind::Moved { .. } => "V",
    };

    println!("{} {}", event_type, event.path.display());
}
