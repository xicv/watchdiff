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
use crate::core::{AppEvent, AppState, FileEventKind, FileWatcher, HighlightedFileEvent};
use std::time::Instant;

/// Vim mode for enhanced navigation
#[derive(Debug, Clone, PartialEq)]
pub enum VimMode {
    Normal,
    Disabled,
}

/// Stores vim key sequence state for multi-key commands
#[derive(Debug, Clone, Default)]
pub struct VimKeySequence {
    pub keys: String,
    pub last_key_time: Option<Instant>,
}

impl VimKeySequence {
    pub fn push_key(&mut self, key: char) {
        // Reset if too much time has passed (1 second timeout)
        if let Some(last_time) = self.last_key_time {
            if last_time.elapsed().as_secs() > 1 {
                self.keys.clear();
            }
        }
        
        self.keys.push(key);
        self.last_key_time = Some(Instant::now());
        
        // Limit sequence length to prevent memory issues
        if self.keys.len() > 10 {
            self.keys.clear();
        }
    }
    
    pub fn clear(&mut self) {
        self.keys.clear();
        self.last_key_time = None;
    }
    
    pub fn matches(&self, sequence: &str) -> bool {
        self.keys == sequence
    }
}

/// Strip ANSI escape codes from a string
fn strip_ansi_codes(input: &str) -> String {
    let mut result = String::new();
    let mut chars = input.chars().peekable();
    
    while let Some(ch) = chars.next() {
        if ch == '\x1b' && chars.peek() == Some(&'[') {
            // Skip the escape sequence
            chars.next(); // consume '['
            while let Some(ch) = chars.next() {
                if ch.is_ascii_alphabetic() {
                    break;
                }
            }
        } else {
            result.push(ch);
        }
    }
    
    result
}

pub struct TuiApp {
    pub state: AppState,
    pub watcher: FileWatcher,
    pub list_state: ListState,
    pub should_quit: bool,
    pub diff_scroll: usize,
    pub file_list_scroll: usize,
    pub vim_mode: VimMode,
    pub vim_key_sequence: VimKeySequence,
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
            vim_mode: VimMode::Disabled, // Start with vim mode disabled
            vim_key_sequence: VimKeySequence::default(),
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
                        // Handle vim mode toggle and key sequences
                        if self.handle_vim_keys(&key) {
                            continue; // Key was handled by vim mode
                        }
                        
                        match key.code {
                            KeyCode::Char('q') | KeyCode::Esc => {
                                // Toggle vim mode with Esc if not already quitting
                                if self.vim_mode == VimMode::Disabled {
                                    self.vim_mode = VimMode::Normal;
                                    self.vim_key_sequence.clear();
                                } else {
                                    self.should_quit = true;
                                }
                            },
                            KeyCode::Char('h') | KeyCode::F(1) => self.state.toggle_help(),
                            KeyCode::Up | KeyCode::Char('k') => {
                                if self.diff_scroll > 0 {
                                    self.diff_scroll -= 1;
                                }
                            }
                            KeyCode::Down | KeyCode::Char('j') => {
                                let max_scroll = self.state.events.len().saturating_sub(1);
                                if self.diff_scroll < max_scroll {
                                    self.diff_scroll += 1;
                                }
                            }
                            KeyCode::PageUp => {
                                self.diff_scroll = self.diff_scroll.saturating_sub(10);
                            }
                            KeyCode::PageDown => {
                                let max_scroll = self.state.events.len().saturating_sub(1);
                                self.diff_scroll = (self.diff_scroll + 10).min(max_scroll);
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
                                let max_scroll = self.state.watched_files.len().saturating_sub(1);
                                if self.file_list_scroll < max_scroll {
                                    self.file_list_scroll += 1;
                                }
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
            // Ensure scroll position is within bounds
            let max_scroll = events.len().saturating_sub(1);
            if self.diff_scroll > max_scroll {
                self.diff_scroll = max_scroll;
            }
            
            let start_idx = self.diff_scroll.min(events.len());
            let end_idx = (start_idx + visible_height).min(events.len());
            
            // Only slice if we have a valid range
            if start_idx < events.len() && start_idx <= end_idx {
                for event in &events[start_idx..end_idx] {
                    lines.extend(self.format_highlighted_file_event(event));
                    lines.push(Line::from(""));
                }
            }
        }

        let paragraph = Paragraph::new(lines)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::Rgb(80, 80, 80)))
                    .title(" 📊 Changes (↑↓ to scroll, PgUp/PgDn, Home/End) ")
                    .title_style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))
            )
            .wrap(Wrap { trim: true })
            .scroll((0, 0));

        f.render_widget(paragraph, area);

        // Render scrollbar
        if events.len() > visible_height {
            let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight)
                .begin_symbol(Some("↑"))
                .end_symbol(Some("↓"));
            let safe_position = self.diff_scroll.min(events.len().saturating_sub(1));
            let mut scrollbar_state = ScrollbarState::new(events.len())
                .position(safe_position);
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

        let (event_symbol, event_type, color, bg_color) = match &event.kind {
            FileEventKind::Created => ("●", "CREATED", Color::Green, Color::Rgb(0, 40, 0)),
            FileEventKind::Modified => ("●", "MODIFIED", Color::Yellow, Color::Rgb(40, 40, 0)),
            FileEventKind::Deleted => ("●", "DELETED", Color::Red, Color::Rgb(40, 0, 0)),
            FileEventKind::Moved { .. } => ("●", "MOVED", Color::Blue, Color::Rgb(0, 0, 40)),
        };

        // Modern header with better visual separation
        lines.push(Line::from(vec![
            Span::styled(format!("[{}] ", time_str), Style::default().fg(Color::Rgb(100, 100, 100))),
            Span::styled(format!(" {} {} ", event_symbol, event_type), 
                Style::default().fg(color).bg(bg_color).add_modifier(Modifier::BOLD)),
            Span::styled(format!(" {} ", event.path.display()), 
                Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
        ]));
        
        // Add a subtle separator line
        lines.push(Line::from(Span::styled("|--", Style::default().fg(Color::Rgb(60, 60, 60)))));

        // Use syntax-highlighted diff if available, otherwise fallback to basic coloring
        if let Some(ref highlighted_diff) = event.highlighted_diff {
            // Strip ANSI escape codes and render with basic styling
            for line in highlighted_diff.lines().take(20) {
                let prefix = "| ";
                let clean_line = strip_ansi_codes(line);
                lines.push(Line::from(vec![
                    Span::styled(prefix, Style::default().fg(Color::Rgb(60, 60, 60))),
                    Span::raw(clean_line)
                ]));
            }
        } else if let Some(diff) = &event.diff {
            // Improved diff coloring with better visual hierarchy
            for line in diff.lines().take(20) {
                let prefix = "| ";
                let styled_line = if let Some(stripped) = line.strip_prefix('+') {
                    vec![
                        Span::styled(prefix, Style::default().fg(Color::Rgb(60, 60, 60))),
                        Span::styled("+", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
                        Span::styled(stripped, Style::default().fg(Color::Rgb(150, 255, 150)).bg(Color::Rgb(0, 25, 0))),
                    ]
                } else if let Some(stripped) = line.strip_prefix('-') {
                    vec![
                        Span::styled(prefix, Style::default().fg(Color::Rgb(60, 60, 60))),
                        Span::styled("-", Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)),
                        Span::styled(stripped, Style::default().fg(Color::Rgb(255, 150, 150)).bg(Color::Rgb(25, 0, 0))),
                    ]
                } else if line.starts_with("@@") {
                    vec![
                        Span::styled(prefix, Style::default().fg(Color::Rgb(60, 60, 60))),
                        Span::styled(line, Style::default().fg(Color::Cyan).bg(Color::Rgb(0, 20, 30)).add_modifier(Modifier::BOLD)),
                    ]
                } else {
                    vec![
                        Span::styled(prefix, Style::default().fg(Color::Rgb(60, 60, 60))),
                        Span::styled(line, Style::default().fg(Color::Rgb(200, 200, 200))),
                    ]
                };
                lines.push(Line::from(styled_line));
            }
        }

        // Use syntax-highlighted preview if available, otherwise fallback to basic preview
        if let Some(ref highlighted_preview) = event.highlighted_preview {
            lines.push(Line::from(vec![
                Span::styled("|-- ", Style::default().fg(Color::Rgb(60, 60, 60))),
                Span::styled("Preview", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
            ]));
            for line in highlighted_preview.lines().take(5) {
                let clean_line = strip_ansi_codes(line);
                lines.push(Line::from(vec![
                    Span::styled("|   ", Style::default().fg(Color::Rgb(60, 60, 60))),
                    Span::raw(clean_line)
                ]));
            }
        } else if let Some(preview) = &event.content_preview {
            // Improved preview with better formatting
            lines.push(Line::from(vec![
                Span::styled("|-- ", Style::default().fg(Color::Rgb(60, 60, 60))),
                Span::styled("Preview", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
            ]));
            for line in preview.lines().take(5) {
                lines.push(Line::from(vec![
                    Span::styled("|   ", Style::default().fg(Color::Rgb(60, 60, 60))),
                    Span::styled(line, Style::default().fg(Color::Rgb(180, 180, 180)))
                ]));
            }
        }

        // Add a closing separator
        lines.push(Line::from(Span::styled("`--", Style::default().fg(Color::Rgb(60, 60, 60)))));
        
        lines
    }

    fn render_file_list(&mut self, f: &mut Frame, area: Rect) {
        let files: Vec<ListItem> = self.state.watched_files
            .iter()
            .enumerate()
            .map(|(i, path)| {
                let style = if i % 2 == 0 {
                    Style::default().fg(Color::Rgb(220, 220, 220))
                } else {
                    Style::default().fg(Color::Rgb(180, 180, 180)).bg(Color::Rgb(20, 20, 25))
                };
                
                let filename = path.file_name()
                    .and_then(|n| n.to_str())
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| path.display().to_string());
                let parent = path.parent()
                    .map(|p| p.display().to_string())
                    .unwrap_or_default();
                
                ListItem::new(Line::from(vec![
                    Span::styled("📄 ", Style::default().fg(Color::Cyan)),
                    Span::styled(filename, style.add_modifier(Modifier::BOLD)),
                    if !parent.is_empty() {
                        Span::styled(format!(" ({})", parent), Style::default().fg(Color::Rgb(120, 120, 120)))
                    } else {
                        Span::raw("")
                    }
                ]))
            })
            .collect();

        let list = List::new(files)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::Rgb(80, 80, 80)))
                    .title(format!(" 📁 Watched Files ({}) (←→ to scroll) ", self.state.watched_files.len()))
                    .title_style(Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD))
            )
            .highlight_style(Style::default().bg(Color::Rgb(0, 50, 100)).add_modifier(Modifier::BOLD));

        f.render_stateful_widget(list, area, &mut self.list_state);
    }

    fn render_status(&self, f: &mut Frame, area: Rect) {
        // Create vim mode indicator
        let vim_indicator = match self.vim_mode {
            VimMode::Normal => {
                let mut spans = vec![
                    Span::styled(" VIM ", Style::default().fg(Color::Black).bg(Color::Yellow).add_modifier(Modifier::BOLD)),
                ];
                // Show key sequence if any
                if !self.vim_key_sequence.keys.is_empty() {
                    spans.push(Span::styled(
                        format!(" {} ", self.vim_key_sequence.keys),
                        Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
                    ));
                }
                spans
            }
            VimMode::Disabled => vec![
                Span::styled(" ESC ", Style::default().fg(Color::White).bg(Color::Gray).add_modifier(Modifier::BOLD)),
                Span::styled(" for vim mode", Style::default().fg(Color::Rgb(150, 150, 150))),
            ],
        };
        
        let mut first_line = vec![
            Span::styled("⌨️  Press ", Style::default().fg(Color::Rgb(150, 150, 150))),
            Span::styled(" q ", Style::default().fg(Color::White).bg(Color::Red).add_modifier(Modifier::BOLD)),
            Span::styled(" to quit, ", Style::default().fg(Color::Rgb(150, 150, 150))),
            Span::styled(" h ", Style::default().fg(Color::White).bg(Color::Green).add_modifier(Modifier::BOLD)),
            Span::styled(" for help | ", Style::default().fg(Color::Rgb(150, 150, 150))),
        ];
        first_line.extend(vim_indicator);
        
        let status_text = vec![
            Line::from(first_line),
            Line::from(vec![
                Span::styled("📊 Events: ", Style::default().fg(Color::Rgb(150, 150, 150))),
                Span::styled(
                    self.state.events.len().to_string(),
                    Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)
                ),
                Span::styled(" | 📁 Files watched: ", Style::default().fg(Color::Rgb(150, 150, 150))),
                Span::styled(
                    self.state.watched_files.len().to_string(),
                    Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)
                ),
                // Show navigation hints based on vim mode
                match self.vim_mode {
                    VimMode::Normal => Span::styled(" | hjkl:move gg:top G:bottom", Style::default().fg(Color::Rgb(120, 120, 120))),
                    VimMode::Disabled => Span::styled(" | ↑↓←→:move", Style::default().fg(Color::Rgb(120, 120, 120))),
                },
            ]),
        ];

        let status = Paragraph::new(status_text)
            .block(Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Rgb(80, 80, 80)))
                .title(" ℹ️  Status ")
                .title_style(Style::default().fg(Color::Magenta).add_modifier(Modifier::BOLD)))
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
            Line::from(vec![
                Span::styled("Vim Mode", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
                Span::styled(" (Press Esc to toggle):", Style::default())
            ]),
            Line::from(""),
            Line::from(vec![
                Span::styled("  h, j, k, l  ", Style::default().fg(Color::Magenta).add_modifier(Modifier::BOLD)),
                Span::styled("- Move left, down, up, right", Style::default())
            ]),
            Line::from(vec![
                Span::styled("  gg         ", Style::default().fg(Color::Magenta).add_modifier(Modifier::BOLD)),
                Span::styled("- Go to top", Style::default())
            ]),
            Line::from(vec![
                Span::styled("  G          ", Style::default().fg(Color::Magenta).add_modifier(Modifier::BOLD)),
                Span::styled("- Go to bottom", Style::default())
            ]),
            Line::from(vec![
                Span::styled("  w, b       ", Style::default().fg(Color::Magenta).add_modifier(Modifier::BOLD)),
                Span::styled("- Jump forward/backward (5 lines)", Style::default())
            ]),
            Line::from(vec![
                Span::styled("  0, $       ", Style::default().fg(Color::Magenta).add_modifier(Modifier::BOLD)),
                Span::styled("- Go to start/end of line", Style::default())
            ]),
            Line::from(vec![
                Span::styled("  Ctrl+d/u   ", Style::default().fg(Color::Magenta).add_modifier(Modifier::BOLD)),
                Span::styled("- Half page down/up", Style::default())
            ]),
            Line::from(vec![
                Span::styled("  Ctrl+f/b   ", Style::default().fg(Color::Magenta).add_modifier(Modifier::BOLD)),
                Span::styled("- Full page down/up", Style::default())
            ]),
            Line::from(vec![
                Span::styled("  i          ", Style::default().fg(Color::Magenta).add_modifier(Modifier::BOLD)),
                Span::styled("- Exit vim mode", Style::default())
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
    
    /// Handle vim mode key sequences and navigation
    fn handle_vim_keys(&mut self, key: &crossterm::event::KeyEvent) -> bool {
        if self.vim_mode == VimMode::Disabled {
            return false;
        }
        
        use crossterm::event::{KeyCode, KeyModifiers};
        
        match key.code {
            // Handle Ctrl+key combinations first (before the general char pattern)
            KeyCode::Char('d') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.vim_half_page_down();
                return true;
            }
            KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.vim_half_page_up();
                return true;
            }
            KeyCode::Char('f') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.vim_page_down();
                return true;
            }
            KeyCode::Char('b') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.vim_page_up();
                return true;
            }
            KeyCode::Char(c) => {
                // Handle regular character keys
                match c {
                    // Disable vim mode
                    'i' => {
                        self.vim_mode = VimMode::Disabled;
                        self.vim_key_sequence.clear();
                        return true;
                    }
                    // Basic vim movements
                    'h' => {
                        self.vim_move_left();
                        return true;
                    }
                    'j' => {
                        self.vim_move_down();
                        return true;
                    }
                    'k' => {
                        self.vim_move_up();
                        return true;
                    }
                    'l' => {
                        self.vim_move_right();
                        return true;
                    }
                    // Word movements (adapted for diff context)
                    'w' => {
                        self.vim_word_forward();
                        return true;
                    }
                    'b' => {
                        self.vim_word_backward();
                        return true;
                    }
                    // Line movements
                    '0' => {
                        self.vim_line_start();
                        return true;
                    }
                    '$' => {
                        self.vim_line_end();
                        return true;
                    }
                    // Handle multi-character sequences
                    'g' | 'G' => {
                        self.vim_key_sequence.push_key(c);
                        self.handle_vim_sequence();
                        return true;
                    }
                    _ => {
                        // Clear sequence for unrecognized keys
                        self.vim_key_sequence.clear();
                        return false;
                    }
                }
            }
            _ => {
                // Clear sequence for unrecognized keys
                self.vim_key_sequence.clear();
                return false;
            }
        }
    }
    
    /// Handle vim multi-character sequences like 'gg' and 'G'
    fn handle_vim_sequence(&mut self) {
        if self.vim_key_sequence.matches("gg") {
            self.vim_goto_top();
            self.vim_key_sequence.clear();
        } else if self.vim_key_sequence.matches("G") {
            self.vim_goto_bottom();
            self.vim_key_sequence.clear();
        }
        // Clear if we have an incomplete sequence that's too old
        else if let Some(last_time) = self.vim_key_sequence.last_key_time {
            if last_time.elapsed().as_millis() > 500 {
                self.vim_key_sequence.clear();
            }
        }
    }
    
    /// Vim movement implementations
    fn vim_move_up(&mut self) {
        if self.diff_scroll > 0 {
            self.diff_scroll -= 1;
        }
    }
    
    fn vim_move_down(&mut self) {
        let max_scroll = self.state.events.len().saturating_sub(1);
        if self.diff_scroll < max_scroll {
            self.diff_scroll += 1;
        }
    }
    
    fn vim_move_left(&mut self) {
        if self.file_list_scroll > 0 {
            self.file_list_scroll -= 1;
        }
    }
    
    fn vim_move_right(&mut self) {
        let max_scroll = self.state.watched_files.len().saturating_sub(1);
        if self.file_list_scroll < max_scroll {
            self.file_list_scroll += 1;
        }
    }
    
    fn vim_word_forward(&mut self) {
        // Move down by 5 lines (word-like movement in diff context)
        let max_scroll = self.state.events.len().saturating_sub(1);
        self.diff_scroll = (self.diff_scroll + 5).min(max_scroll);
    }
    
    fn vim_word_backward(&mut self) {
        // Move up by 5 lines (word-like movement in diff context)
        self.diff_scroll = self.diff_scroll.saturating_sub(5);
    }
    
    fn vim_line_start(&mut self) {
        // In diff view context, move to leftmost position
        self.file_list_scroll = 0;
    }
    
    fn vim_line_end(&mut self) {
        // In diff view context, move to rightmost position
        let max_scroll = self.state.watched_files.len().saturating_sub(1);
        self.file_list_scroll = max_scroll;
    }
    
    fn vim_goto_top(&mut self) {
        self.diff_scroll = 0;
    }
    
    fn vim_goto_bottom(&mut self) {
        self.diff_scroll = self.state.events.len().saturating_sub(1);
    }
    
    fn vim_half_page_down(&mut self) {
        let max_scroll = self.state.events.len().saturating_sub(1);
        self.diff_scroll = (self.diff_scroll + 10).min(max_scroll);
    }
    
    fn vim_half_page_up(&mut self) {
        self.diff_scroll = self.diff_scroll.saturating_sub(10);
    }
    
    fn vim_page_down(&mut self) {
        let max_scroll = self.state.events.len().saturating_sub(1);
        self.diff_scroll = (self.diff_scroll + 20).min(max_scroll);
    }
    
    fn vim_page_up(&mut self) {
        self.diff_scroll = self.diff_scroll.saturating_sub(20);
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