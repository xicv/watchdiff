use std::io;
use std::time::Duration;
use std::path::PathBuf;
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

/// Application UI mode
#[derive(Debug, Clone, PartialEq)]
pub enum AppMode {
    Normal,
    Search,
    Help,
}

/// Search mode state for fuzzy file search
#[derive(Debug, Clone, Default)]
pub struct SearchState {
    pub query: String,
    pub filtered_files: Vec<PathBuf>,
    pub selected_index: usize,
    pub preview_scroll: usize,
}

impl SearchState {
    pub fn update_filtered_files(&mut self, all_files: &std::collections::HashSet<PathBuf>, events: &[crate::core::HighlightedFileEvent]) {
        if self.query.is_empty() {
            // Show all files when no query
            self.filtered_files = all_files.iter().cloned().collect();
        } else {
            // Apply fuzzy search
            let mut scored_files: Vec<(PathBuf, i32)> = all_files
                .iter()
                .filter_map(|path| {
                    let score = self.fuzzy_match(path);
                    if score > 0 {
                        Some((path.clone(), score))
                    } else {
                        None
                    }
                })
                .collect();
            
            // Sort by score (higher is better) and recent activity
            scored_files.sort_by(|a, b| {
                let score_cmp = b.1.cmp(&a.1);
                if score_cmp == std::cmp::Ordering::Equal {
                    // If scores are equal, prioritize recently changed files
                    let a_recent = events.iter().any(|e| e.path == a.0);
                    let b_recent = events.iter().any(|e| e.path == b.0);
                    b_recent.cmp(&a_recent)
                } else {
                    score_cmp
                }
            });
            
            // Extract just the paths
            self.filtered_files = scored_files.into_iter().map(|(path, _)| path).collect();
        }
        
        // Reset selection if out of bounds
        if self.selected_index >= self.filtered_files.len() {
            self.selected_index = 0;
        }
    }
    
    fn fuzzy_match(&self, path: &PathBuf) -> i32 {
        let query = self.query.to_lowercase();
        let path_str = path.to_string_lossy().to_lowercase();
        let filename = path.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .to_lowercase();
        
        // Simple fuzzy matching algorithm
        let mut score: i32 = 0;
        let mut query_chars = query.chars().peekable();
        let mut consecutive_bonus = 0;
        
        // First check filename for exact substring match (higher score)
        if filename.contains(&query) {
            score += 100;
        }
        
        // Then check full path
        if path_str.contains(&query) {
            score += 50;
        }
        
        // Character-by-character fuzzy matching
        let path_chars: Vec<char> = path_str.chars().collect();
        let mut path_idx = 0;
        
        while let Some(&query_char) = query_chars.peek() {
            if path_idx >= path_chars.len() {
                break;
            }
            
            if path_chars[path_idx] == query_char {
                score += 10 + consecutive_bonus;
                consecutive_bonus += 5; // Bonus for consecutive matches
                query_chars.next();
            } else {
                consecutive_bonus = 0;
            }
            path_idx += 1;
        }
        
        // Penalty for longer paths (prefer shorter, more specific matches)
        score = score.saturating_sub(path_str.len() as i32 / 10);
        
        // Return 0 if we didn't match all query characters
        if query_chars.peek().is_some() {
            0
        } else {
            score.max(1)
        }
    }
    
    pub fn get_selected_file(&self) -> Option<&PathBuf> {
        self.filtered_files.get(self.selected_index)
    }
    
    pub fn move_up(&mut self) {
        if self.selected_index > 0 {
            self.selected_index -= 1;
        }
    }
    
    pub fn move_down(&mut self) {
        if self.selected_index + 1 < self.filtered_files.len() {
            self.selected_index += 1;
        }
    }
    
    pub fn add_char(&mut self, c: char) {
        self.query.push(c);
    }
    
    pub fn remove_char(&mut self) {
        self.query.pop();
    }
    
    pub fn clear(&mut self) {
        self.query.clear();
        self.filtered_files.clear();
        self.selected_index = 0;
        self.preview_scroll = 0;
    }
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
    pub app_mode: AppMode,
    pub search_state: SearchState,
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
            app_mode: AppMode::Normal,
            search_state: SearchState::default(),
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
                        // Handle search mode keys first
                        if self.app_mode == AppMode::Search {
                            if self.handle_search_keys(&key) {
                                continue; // Key was handled by search mode
                            }
                        }

                        // Handle vim mode toggle and key sequences
                        if self.handle_vim_keys(&key) {
                            continue; // Key was handled by vim mode
                        }
                        
                        match key.code {
                            KeyCode::Char('q') | KeyCode::Esc => {
                                match self.app_mode {
                                    AppMode::Search => {
                                        // Exit search mode
                                        self.app_mode = AppMode::Normal;
                                        self.search_state.clear();
                                    }
                                    AppMode::Help => {
                                        // Exit help mode
                                        self.app_mode = AppMode::Normal;
                                    }
                                    AppMode::Normal => {
                                        // Toggle vim mode with Esc if not already quitting
                                        if self.vim_mode == VimMode::Disabled {
                                            self.vim_mode = VimMode::Normal;
                                            self.vim_key_sequence.clear();
                                        } else {
                                            self.should_quit = true;
                                        }
                                    }
                                }
                            },
                            KeyCode::Char('h') | KeyCode::F(1) => {
                                self.app_mode = if self.app_mode == AppMode::Help {
                                    AppMode::Normal
                                } else {
                                    AppMode::Help
                                };
                            },
                            KeyCode::Char('/') => {
                                // Enter search mode
                                self.app_mode = AppMode::Search;
                                self.search_state.clear();
                            },
                            KeyCode::Char('p') if key.modifiers.contains(crossterm::event::KeyModifiers::CONTROL) => {
                                // Enter search mode (Ctrl+P alternative)
                                self.app_mode = AppMode::Search;
                                self.search_state.clear();
                            },
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
        match self.app_mode {
            AppMode::Help => {
                self.render_help(f);
                return;
            }
            AppMode::Search => {
                self.render_search_mode(f);
                return;
            }
            AppMode::Normal => {
                // Continue with normal rendering
            }
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
            Span::styled(" for help, ", Style::default().fg(Color::Rgb(150, 150, 150))),
            Span::styled(" / ", Style::default().fg(Color::White).bg(Color::Cyan).add_modifier(Modifier::BOLD)),
            Span::styled(" to search | ", Style::default().fg(Color::Rgb(150, 150, 150))),
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

    fn render_search_mode(&mut self, f: &mut Frame) {
        // Ensure cursor is visible in search mode
        // This is handled by ratatui when we call set_cursor_position
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .margin(1)
            .constraints([
                Constraint::Length(3),      // Search input
                Constraint::Min(10),        // File list + preview
            ])
            .split(f.area());

        // Render search input
        self.render_search_input(f, chunks[0]);
        
        // Split the remaining area for file list and preview
        let content_chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Percentage(40), // File list
                Constraint::Percentage(60), // Preview
            ])
            .split(chunks[1]);

        self.render_search_results(f, content_chunks[0]);
        self.render_file_preview(f, content_chunks[1]);
    }

    fn render_search_input(&self, f: &mut Frame, area: Rect) {
        // Create input text with visual cursor indicator
        let prefix = "🔍 ";
        let input_text = format!("{}{}█", prefix, self.search_state.query);
        
        let input = Paragraph::new(input_text)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::Yellow))
                    .title(" Search Files ")
                    .title_style(Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD))
            );
        f.render_widget(input, area);
        
        // Position the terminal cursor at the end (after the visual cursor)
        // This helps with terminal cursor visibility
        let cursor_x = area.x + 1 + prefix.chars().count() as u16 + self.search_state.query.len() as u16 + 1;
        let cursor_y = area.y + 1;
        
        // Ensure cursor is within bounds
        if cursor_x < area.x + area.width - 1 {
            f.set_cursor_position((cursor_x, cursor_y));
        }
    }

    fn render_search_results(&mut self, f: &mut Frame, area: Rect) {
        // Update filtered files based on current query
        self.search_state.update_filtered_files(&self.state.watched_files, &self.state.highlighted_events);
        
        let items: Vec<ListItem> = self.search_state.filtered_files
            .iter()
            .enumerate()
            .map(|(i, path)| {
                let style = if i == self.search_state.selected_index {
                    Style::default().bg(Color::Blue).fg(Color::White).add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::White)
                };
                
                let filename = path.file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("")
                    .to_string();
                let parent = path.parent()
                    .map(|p| p.display().to_string())
                    .unwrap_or_default();

                // Check if file has recent changes
                let has_changes = self.state.highlighted_events.iter().any(|e| e.path == *path);
                let change_indicator = if has_changes { "🟡 " } else { "📄 " };
                
                ListItem::new(Line::from(vec![
                    Span::styled(change_indicator, Style::default().fg(Color::Cyan)),
                    Span::styled(filename, style.add_modifier(Modifier::BOLD)),
                    if !parent.is_empty() {
                        Span::styled(format!(" ({})", parent), Style::default().fg(Color::Rgb(120, 120, 120)))
                    } else {
                        Span::raw("")
                    }
                ]))
            })
            .collect();

        let list = List::new(items)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::Cyan))
                    .title(format!(" Files ({}/{}) ", 
                        self.search_state.filtered_files.len(),
                        self.state.watched_files.len()
                    ))
                    .title_style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))
            );

        f.render_widget(list, area);
    }

    fn render_file_preview(&mut self, f: &mut Frame, area: Rect) {
        let selected_file = self.search_state.get_selected_file();
        
        if let Some(file_path) = selected_file {
            // Try to read file content
            match std::fs::read_to_string(file_path) {
                Ok(content) => {
                    let language = crate::highlight::SyntaxHighlighter::default()
                        .get_language_from_path(file_path)
                        .unwrap_or_else(|| "Plain Text".to_string());
                    
                    // Check if file has recent changes for diff preview
                    let recent_event = self.state.highlighted_events
                        .iter()
                        .find(|e| e.path == *file_path);
                    
                    if let Some(event) = recent_event {
                        self.render_diff_preview(f, area, file_path, &content, event);
                    } else {
                        self.render_file_content_preview(f, area, file_path, &content, &language);
                    }
                }
                Err(_) => {
                    let error_text = vec![
                        Line::from(Span::styled("Cannot read file", Style::default().fg(Color::Red))),
                        Line::from(Span::styled(file_path.display().to_string(), Style::default().fg(Color::Gray))),
                    ];
                    
                    let paragraph = Paragraph::new(error_text)
                        .block(
                            Block::default()
                                .borders(Borders::ALL)
                                .border_style(Style::default().fg(Color::Red))
                                .title(" Preview ")
                                .title_style(Style::default().fg(Color::Red))
                        );
                    f.render_widget(paragraph, area);
                }
            }
        } else {
            let placeholder = Paragraph::new("Select a file to preview")
                .style(Style::default().fg(Color::Gray))
                .alignment(Alignment::Center)
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .border_style(Style::default().fg(Color::Gray))
                        .title(" Preview ")
                );
            f.render_widget(placeholder, area);
        }
    }

    fn render_file_content_preview(&self, f: &mut Frame, area: Rect, file_path: &std::path::Path, content: &str, language: &str) {
        let visible_height = area.height as usize - 2; // Account for borders
        let lines: Vec<&str> = content.lines().collect();
        
        let start_line = self.search_state.preview_scroll;
        let end_line = (start_line + visible_height).min(lines.len());
        
        // Create syntax highlighter
        let highlighter = crate::highlight::SyntaxHighlighter::default();
        
        let visible_lines: Vec<Line> = lines[start_line..end_line]
            .iter()
            .enumerate()
            .map(|(i, line)| {
                let line_num = start_line + i + 1;
                let line_num_span = Span::styled(
                    format!("{:4} │ ", line_num), 
                    Style::default().fg(Color::Rgb(100, 100, 100))
                );
                
                // Apply syntax highlighting to the line
                let highlighted_spans = highlighter.highlight_line(line, language, line_num);
                
                let mut spans = vec![line_num_span];
                for (style, text) in highlighted_spans {
                    spans.push(Span::styled(text, style));
                }
                
                Line::from(spans)
            })
            .collect();

        let paragraph = Paragraph::new(visible_lines)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::Green))
                    .title(format!(" {} [{}] (↑↓ PgUp/PgDn ←→ to scroll) ", 
                        file_path.file_name().and_then(|n| n.to_str()).unwrap_or(""),
                        language
                    ))
                    .title_style(Style::default().fg(Color::Green).add_modifier(Modifier::BOLD))
            )
            .wrap(Wrap { trim: false });

        f.render_widget(paragraph, area);
    }

    fn render_diff_preview(&self, f: &mut Frame, area: Rect, file_path: &std::path::Path, _content: &str, event: &crate::core::HighlightedFileEvent) {
        let mut lines = Vec::new();
        
        // Show file change information
        let (event_symbol, event_type, color) = match &event.kind {
            crate::core::FileEventKind::Created => ("●", "CREATED", Color::Green),
            crate::core::FileEventKind::Modified => ("●", "MODIFIED", Color::Yellow),
            crate::core::FileEventKind::Deleted => ("●", "DELETED", Color::Red),
            crate::core::FileEventKind::Moved { .. } => ("●", "MOVED", Color::Blue),
        };

        let timestamp = event.timestamp
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let time_str = format!("{:02}:{:02}:{:02}", 
            (timestamp % 86400) / 3600,
            (timestamp % 3600) / 60,
            timestamp % 60
        );

        lines.push(Line::from(vec![
            Span::styled(format!("[{}] ", time_str), Style::default().fg(Color::Rgb(100, 100, 100))),
            Span::styled(format!("{} {} ", event_symbol, event_type), Style::default().fg(color).add_modifier(Modifier::BOLD)),
        ]));
        lines.push(Line::from(""));

        // Show diff if available
        if let Some(diff) = &event.diff {
            for (i, line) in diff.lines().enumerate() {
                if i >= (area.height as usize - 6) { // Leave space for headers
                    break;
                }
                
                let styled_line = if let Some(stripped) = line.strip_prefix('+') {
                    Line::from(vec![
                        Span::styled("+", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
                        Span::styled(stripped, Style::default().fg(Color::Rgb(150, 255, 150))),
                    ])
                } else if let Some(stripped) = line.strip_prefix('-') {
                    Line::from(vec![
                        Span::styled("-", Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)),
                        Span::styled(stripped, Style::default().fg(Color::Rgb(255, 150, 150))),
                    ])
                } else if line.starts_with("@@") {
                    Line::from(Span::styled(line, Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)))
                } else {
                    Line::from(Span::styled(line, Style::default().fg(Color::Rgb(200, 200, 200))))
                };
                lines.push(styled_line);
            }
        }

        let paragraph = Paragraph::new(lines)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::Yellow))
                    .title(format!(" 🔄 {} ", 
                        file_path.file_name().and_then(|n| n.to_str()).unwrap_or("")
                    ))
                    .title_style(Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD))
            )
            .wrap(Wrap { trim: false });

        f.render_widget(paragraph, area);
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
                Span::styled("Search Mode", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
                Span::styled(" (Press / or Ctrl+P):", Style::default())
            ]),
            Line::from(""),
            Line::from(vec![
                Span::styled("  /          ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
                Span::styled("- Enter search mode", Style::default())
            ]),
            Line::from(vec![
                Span::styled("  Ctrl+P     ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
                Span::styled("- Fuzzy file search (like fzf)", Style::default())
            ]),
            Line::from(vec![
                Span::styled("  ↑/↓, j/k   ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
                Span::styled("- Navigate search results", Style::default())
            ]),
            Line::from(vec![
                Span::styled("  Enter      ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
                Span::styled("- Jump to file in diff view", Style::default())
            ]),
            Line::from(vec![
                Span::styled("  Ctrl+U/D   ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
                Span::styled("- Scroll preview up/down", Style::default())
            ]),
            Line::from(vec![
                Span::styled("  PgUp/PgDn  ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
                Span::styled("- Page preview up/down", Style::default())
            ]),
            Line::from(vec![
                Span::styled("  ←→         ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
                Span::styled("- Fine scroll preview", Style::default())
            ]),
            Line::from(vec![
                Span::styled("  Esc        ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
                Span::styled("- Exit search mode", Style::default())
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
    
    /// Jump to a specific file in the diff view and scroll to show it
    fn jump_to_file_in_diff_view(&mut self, target_file: &PathBuf) {
        // Find the most recent event for this file in the diff log
        if let Some(position) = self.state.highlighted_events
            .iter()
            .position(|event| event.path == *target_file) 
        {
            // Set the diff scroll to show this file's event at the top of the view
            self.diff_scroll = position;
            
            // Also clear any file list scroll to return to default view
            self.file_list_scroll = 0;
        } else {
            // If file not found in recent events, it means there are no recent changes
            // for this file. Scroll to top to show the most recent activity.
            self.diff_scroll = 0;
            self.file_list_scroll = 0;
        }
    }

    /// Handle search mode key input
    fn handle_search_keys(&mut self, key: &crossterm::event::KeyEvent) -> bool {
        use crossterm::event::{KeyCode, KeyModifiers};
        
        match key.code {
            KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.search_state.add_char(c);
                true
            }
            KeyCode::Backspace => {
                self.search_state.remove_char();
                true
            }
            KeyCode::Up | KeyCode::Char('k') => {
                self.search_state.move_up();
                true
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.search_state.move_down();
                true
            }
            KeyCode::Enter => {
                // Jump to selected file in diff view
                if let Some(selected_file) = self.search_state.get_selected_file().cloned() {
                    self.jump_to_file_in_diff_view(&selected_file);
                    self.app_mode = AppMode::Normal;
                    self.search_state.clear();
                }
                true
            }
            KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                // Page up in preview
                self.search_state.preview_scroll = self.search_state.preview_scroll.saturating_sub(10);
                true
            }
            KeyCode::Char('d') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                // Page down in preview
                self.search_state.preview_scroll += 10;
                true
            }
            KeyCode::PageUp => {
                // Page up in preview
                self.search_state.preview_scroll = self.search_state.preview_scroll.saturating_sub(10);
                true
            }
            KeyCode::PageDown => {
                // Page down in preview
                self.search_state.preview_scroll += 10;
                true
            }
            KeyCode::Left => {
                // Scroll left in preview (horizontal scroll)
                self.search_state.preview_scroll = self.search_state.preview_scroll.saturating_sub(1);
                true
            }
            KeyCode::Right => {
                // Scroll right/down in preview
                self.search_state.preview_scroll += 1;
                true
            }
            _ => false, // Let other keys be handled normally
        }
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
                    // Always let search key pass through to main handler
                    '/' => {
                        self.vim_key_sequence.clear();
                        return false;
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