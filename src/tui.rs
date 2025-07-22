use std::io;
use std::time::Duration;
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::{Backend, CrosstermBackend},
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{
        Block, Borders, Clear, List, ListItem, ListState, Paragraph, Scrollbar,
        ScrollbarOrientation, ScrollbarState, Wrap,
    },
    Frame, Terminal,
};
use crate::{AppEvent, AppState, FileEvent, FileEventKind, FileWatcher, HighlightedFileEvent};

pub struct TuiApp {
    pub state: AppState,
    pub watcher: FileWatcher,
    pub list_state: ListState,
    pub should_quit: bool,
    pub diff_scroll: usize,
    pub file_list_scroll: usize,
}

impl TuiApp {
    pub fn new(watcher: FileWatcher) -> Self {
        let initial_files = watcher.get_initial_files().unwrap_or_default();
        let mut state = AppState::default();
        
        for file in initial_files {
            state.watched_files.insert(file);
        }

        Self {
            state,
            watcher,
            list_state: ListState::default(),
            should_quit: false,
            diff_scroll: 0,
            file_list_scroll: 0,
        }
    }

    pub fn run<B: Backend>(mut self, terminal: &mut Terminal<B>) -> io::Result<()> {
        loop {
            terminal.draw(|f| self.ui(f))?;

            // Handle file watcher events
            match self.watcher.recv_timeout(Duration::from_millis(50)) {
                Ok(AppEvent::FileChanged(file_event)) => {
                    self.state.add_event(file_event);
                }
                Ok(AppEvent::Quit) => {
                    self.should_quit = true;
                }
                Ok(_) => {}
                Err(_) => {} // Timeout, continue
            }

            // Handle keyboard input
            if event::poll(Duration::from_millis(50))? {
                if let Event::Key(key) = event::read()? {
                    if key.kind == KeyEventKind::Press {
                        match key.code {
                            KeyCode::Char('q') | KeyCode::Esc => self.should_quit = true,
                            KeyCode::Char('h') | KeyCode::F(1) => self.state.toggle_help(),
                            KeyCode::Up | KeyCode::Char('k') => {
                                if self.diff_scroll > 0 {
                                    self.diff_scroll -= 1;
                                }
                            }
                            KeyCode::Down | KeyCode::Char('j') => {
                                self.diff_scroll += 1;
                            }
                            KeyCode::PageUp => {
                                self.diff_scroll = self.diff_scroll.saturating_sub(10);
                            }
                            KeyCode::PageDown => {
                                self.diff_scroll += 10;
                            }
                            KeyCode::Home => {
                                self.diff_scroll = 0;
                            }
                            KeyCode::End => {
                                self.diff_scroll = self.state.events.len().saturating_sub(1);
                            }
                            KeyCode::Left => {
                                if self.file_list_scroll > 0 {
                                    self.file_list_scroll -= 1;
                                }
                            }
                            KeyCode::Right => {
                                self.file_list_scroll += 1;
                            }
                            _ => {}
                        }
                    }
                }
            }

            if self.should_quit {
                break;
            }
        }

        Ok(())
    }

    fn ui(&mut self, f: &mut Frame) {
        if self.state.show_help {
            self.render_help(f);
            return;
        }

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .margin(1)
            .constraints([
                Constraint::Percentage(70), // Diff log
                Constraint::Percentage(25), // File list
                Constraint::Min(3),         // Status bar
            ])
            .split(f.area());

        self.render_diff_log(f, chunks[0]);
        self.render_file_list(f, chunks[1]);
        self.render_status(f, chunks[2]);
    }

    fn render_diff_log(&mut self, f: &mut Frame, area: Rect) {
        let events = &self.state.highlighted_events;
        
        let mut lines = Vec::new();
        let visible_height = area.height as usize - 2; // Account for borders
        
        if events.is_empty() {
            lines.push(Line::from(vec![
                Span::styled("Watching for file changes...", Style::default().fg(Color::Gray))
            ]));
        } else {
            let start_idx = self.diff_scroll;
            let end_idx = (start_idx + visible_height).min(events.len());
            
            for event in &events[start_idx..end_idx] {
                lines.extend(self.format_highlighted_file_event(event));
                lines.push(Line::from(""));
            }
        }

        let paragraph = Paragraph::new(lines)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(" Changes (↑↓ to scroll, PgUp/PgDn, Home/End) ")
                    .title_style(Style::default().fg(Color::Cyan))
            )
            .wrap(Wrap { trim: true })
            .scroll((0, 0));

        f.render_widget(paragraph, area);

        // Render scrollbar
        if events.len() > visible_height {
            let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight)
                .begin_symbol(Some("↑"))
                .end_symbol(Some("↓"));
            let mut scrollbar_state = ScrollbarState::new(events.len())
                .position(self.diff_scroll);
            f.render_stateful_widget(
                scrollbar,
                area.inner(ratatui::layout::Margin { vertical: 1, horizontal: 1 }),
                &mut scrollbar_state,
            );
        }
    }

    fn format_highlighted_file_event<'a>(&self, event: &'a HighlightedFileEvent) -> Vec<Line<'a>> {
        let mut lines = Vec::new();
        
        let timestamp = event.timestamp
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        
        let time_str = format!("{:02}:{:02}:{:02}", 
            (timestamp % 86400) / 3600,
            (timestamp % 3600) / 60,
            timestamp % 60
        );

        let (event_type, color) = match &event.kind {
            FileEventKind::Created => ("CREATED", Color::Green),
            FileEventKind::Modified => ("MODIFIED", Color::Yellow),
            FileEventKind::Deleted => ("DELETED", Color::Red),
            FileEventKind::Moved { .. } => ("MOVED", Color::Blue),
        };

        lines.push(Line::from(vec![
            Span::styled(format!("[{}] ", time_str), Style::default().fg(Color::Gray)),
            Span::styled(format!("{} ", event_type), Style::default().fg(color).add_modifier(Modifier::BOLD)),
            Span::styled(event.path.display().to_string(), Style::default().fg(Color::White)),
        ]));

        // Use syntax-highlighted diff if available, otherwise fallback to basic coloring
        if let Some(ref highlighted_diff) = event.highlighted_diff {
            // Parse ANSI escape codes and render as styled text
            for line in highlighted_diff.lines().take(20) {
                lines.push(Line::from(Span::raw(line)));
            }
        } else if let Some(diff) = &event.diff {
            // Fallback to basic diff coloring
            for line in diff.lines().take(20) {
                if line.starts_with('+') {
                    lines.push(Line::from(Span::styled(line, Style::default().fg(Color::Green))));
                } else if line.starts_with('-') {
                    lines.push(Line::from(Span::styled(line, Style::default().fg(Color::Red))));
                } else if line.starts_with("@@") {
                    lines.push(Line::from(Span::styled(line, Style::default().fg(Color::Cyan))));
                } else {
                    lines.push(Line::from(Span::styled(line, Style::default().fg(Color::White))));
                }
            }
        }

        // Use syntax-highlighted preview if available, otherwise fallback to basic preview
        if let Some(ref highlighted_preview) = event.highlighted_preview {
            lines.push(Line::from(Span::styled("Preview:", Style::default().fg(Color::Cyan))));
            for line in highlighted_preview.lines().take(5) {
                lines.push(Line::from(Span::raw(format!("  {}", line))));
            }
        } else if let Some(preview) = &event.content_preview {
            // Fallback to basic preview
            lines.push(Line::from(Span::styled("Preview:", Style::default().fg(Color::Cyan))));
            for line in preview.lines().take(5) {
                lines.push(Line::from(Span::styled(
                    format!("  {}", line), 
                    Style::default().fg(Color::Gray)
                )));
            }
        }

        lines
    }

    fn render_file_list(&mut self, f: &mut Frame, area: Rect) {
        let files: Vec<ListItem> = self.state.watched_files
            .iter()
            .enumerate()
            .map(|(i, path)| {
                let style = if i % 2 == 0 {
                    Style::default()
                } else {
                    Style::default().bg(Color::DarkGray)
                };
                
                ListItem::new(Line::from(vec![
                    Span::styled(
                        path.display().to_string(),
                        style.fg(Color::White)
                    )
                ]))
            })
            .collect();

        let list = List::new(files)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(format!(" Watched Files ({}) (←→ to scroll) ", self.state.watched_files.len()))
                    .title_style(Style::default().fg(Color::Yellow))
            )
            .highlight_style(Style::default().add_modifier(Modifier::BOLD));

        f.render_stateful_widget(list, area, &mut self.list_state);
    }

    fn render_status(&self, f: &mut Frame, area: Rect) {
        let status_text = vec![
            Line::from(vec![
                Span::styled("Press ", Style::default()),
                Span::styled("q", Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)),
                Span::styled(" to quit, ", Style::default()),
                Span::styled("h", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
                Span::styled(" for help, ", Style::default()),
                Span::styled("↑↓", Style::default().fg(Color::Blue).add_modifier(Modifier::BOLD)),
                Span::styled(" to scroll diff", Style::default()),
            ]),
            Line::from(vec![
                Span::styled("Events: ", Style::default()),
                Span::styled(
                    self.state.events.len().to_string(),
                    Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)
                ),
                Span::styled(" | Files watched: ", Style::default()),
                Span::styled(
                    self.state.watched_files.len().to_string(),
                    Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)
                ),
            ]),
        ];

        let status = Paragraph::new(status_text)
            .block(Block::default().borders(Borders::ALL).title(" Status "))
            .alignment(Alignment::Center);

        f.render_widget(status, area);
    }

    fn render_help(&self, f: &mut Frame) {
        let popup_area = self.centered_rect(80, 60, f.area());

        let help_text = vec![
            Line::from(vec![
                Span::styled("WatchDiff - File Watching Tool", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))
            ]),
            Line::from(""),
            Line::from("Keyboard Shortcuts:"),
            Line::from(""),
            Line::from(vec![
                Span::styled("  q, Esc     ", Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)),
                Span::styled("- Quit the application", Style::default())
            ]),
            Line::from(vec![
                Span::styled("  h, F1      ", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
                Span::styled("- Show/hide this help", Style::default())
            ]),
            Line::from(vec![
                Span::styled("  ↑, k       ", Style::default().fg(Color::Blue).add_modifier(Modifier::BOLD)),
                Span::styled("- Scroll diff log up", Style::default())
            ]),
            Line::from(vec![
                Span::styled("  ↓, j       ", Style::default().fg(Color::Blue).add_modifier(Modifier::BOLD)),
                Span::styled("- Scroll diff log down", Style::default())
            ]),
            Line::from(vec![
                Span::styled("  PgUp       ", Style::default().fg(Color::Blue).add_modifier(Modifier::BOLD)),
                Span::styled("- Scroll diff log up (fast)", Style::default())
            ]),
            Line::from(vec![
                Span::styled("  PgDn       ", Style::default().fg(Color::Blue).add_modifier(Modifier::BOLD)),
                Span::styled("- Scroll diff log down (fast)", Style::default())
            ]),
            Line::from(vec![
                Span::styled("  Home       ", Style::default().fg(Color::Blue).add_modifier(Modifier::BOLD)),
                Span::styled("- Go to top of diff log", Style::default())
            ]),
            Line::from(vec![
                Span::styled("  End        ", Style::default().fg(Color::Blue).add_modifier(Modifier::BOLD)),
                Span::styled("- Go to bottom of diff log", Style::default())
            ]),
            Line::from(vec![
                Span::styled("  ←, →       ", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
                Span::styled("- Scroll file list", Style::default())
            ]),
            Line::from(""),
            Line::from("Features:"),
            Line::from(""),
            Line::from("• Real-time file change monitoring"),
            Line::from("• Respects .gitignore patterns"),
            Line::from("• Shows diffs for text file changes"),
            Line::from("• Scrollable diff log and file list"),
            Line::from("• High performance with async processing"),
        ];

        let paragraph = Paragraph::new(help_text)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(" Help ")
                    .title_style(Style::default().fg(Color::Cyan))
            )
            .wrap(Wrap { trim: true });

        f.render_widget(Clear, popup_area);
        f.render_widget(paragraph, popup_area);
    }

    fn format_file_event<'a>(&self, event: &'a FileEvent) -> Vec<Line<'a>> {
        let mut lines = Vec::new();
        
        let timestamp = event.timestamp
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        
        let time_str = format!("{:02}:{:02}:{:02}", 
            (timestamp % 86400) / 3600,
            (timestamp % 3600) / 60,
            timestamp % 60
        );

        let (event_type, color) = match &event.kind {
            FileEventKind::Created => ("CREATED", Color::Green),
            FileEventKind::Modified => ("MODIFIED", Color::Yellow),
            FileEventKind::Deleted => ("DELETED", Color::Red),
            FileEventKind::Moved { .. } => ("MOVED", Color::Blue),
        };

        lines.push(Line::from(vec![
            Span::styled(format!("[{}] ", time_str), Style::default().fg(Color::Gray)),
            Span::styled(format!("{} ", event_type), Style::default().fg(color).add_modifier(Modifier::BOLD)),
            Span::styled(event.path.display().to_string(), Style::default().fg(Color::White)),
        ]));

        if let Some(diff) = &event.diff {
            for line in diff.lines().take(20) { // Limit lines shown
                if line.starts_with('+') {
                    lines.push(Line::from(Span::styled(line, Style::default().fg(Color::Green))));
                } else if line.starts_with('-') {
                    lines.push(Line::from(Span::styled(line, Style::default().fg(Color::Red))));
                } else if line.starts_with("@@") {
                    lines.push(Line::from(Span::styled(line, Style::default().fg(Color::Cyan))));
                } else {
                    lines.push(Line::from(Span::styled(line, Style::default().fg(Color::White))));
                }
            }
        }

        if let Some(preview) = &event.content_preview {
            lines.push(Line::from(Span::styled("Preview:", Style::default().fg(Color::Cyan))));
            for line in preview.lines().take(5) {
                lines.push(Line::from(Span::styled(
                    format!("  {}", line), 
                    Style::default().fg(Color::Gray)
                )));
            }
        }

        lines
    }

    fn centered_rect(&self, percent_x: u16, percent_y: u16, r: Rect) -> Rect {
        let popup_layout = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Percentage((100 - percent_y) / 2),
                Constraint::Percentage(percent_y),
                Constraint::Percentage((100 - percent_y) / 2),
            ])
            .split(r);

        Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Percentage((100 - percent_x) / 2),
                Constraint::Percentage(percent_x),
                Constraint::Percentage((100 - percent_x) / 2),
            ])
            .split(popup_layout[1])[1]
    }
}

pub fn setup_terminal() -> Result<Terminal<CrosstermBackend<io::Stdout>>, io::Error> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    Terminal::new(backend)
}

pub fn restore_terminal(terminal: &mut Terminal<CrosstermBackend<io::Stdout>>) -> Result<(), io::Error> {
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()
}