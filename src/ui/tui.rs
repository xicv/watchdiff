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
use crate::review::{ReviewSession, ReviewAction, ReviewNavigationAction};
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
    Review,
    Summary,
}

/// Search mode state for fuzzy file search
#[derive(Debug, Clone, Default)]
pub struct SearchState {
    pub query: String,
    pub filtered_files: Vec<PathBuf>,
    pub selected_index: usize,
    pub preview_scroll: usize,
    /// Debouncing for search performance
    pub last_update: Option<std::time::Instant>,
    pub pending_query: Option<String>,
}

/// Summary mode state for change summary view
#[derive(Debug, Clone)]
pub struct SummaryState {
    pub selected_file_index: usize,
    pub time_filter: crate::core::SummaryTimeFrame,
    pub origin_filter: Option<crate::core::ChangeOrigin>,
    pub view_mode: SummaryViewMode,
    pub diff_scroll: usize,
    pub last_refresh: std::time::Instant,
    pub current_summary: Option<crate::core::ChangeSummary>,
}

/// Different view modes within the summary
#[derive(Debug, Clone, PartialEq)]
pub enum SummaryViewMode {
    Overview,  // Show statistics and file list
    FileDetail, // Show selected file's diff
}

impl Default for SummaryState {
    fn default() -> Self {
        Self {
            selected_file_index: 0,
            time_filter: crate::core::SummaryTimeFrame::LastDay,
            origin_filter: None,
            view_mode: SummaryViewMode::Overview,
            diff_scroll: 0,
            last_refresh: std::time::Instant::now(),
            current_summary: None,
        }
    }
}

impl SummaryState {
    pub fn move_up(&mut self) {
        if self.selected_file_index > 0 {
            self.selected_file_index -= 1;
        }
    }
    
    pub fn move_down(&mut self, max_items: usize) {
        if self.selected_file_index + 1 < max_items {
            self.selected_file_index += 1;
        }
    }
    
    pub fn toggle_view_mode(&mut self) {
        self.view_mode = match self.view_mode {
            SummaryViewMode::Overview => SummaryViewMode::FileDetail,
            SummaryViewMode::FileDetail => SummaryViewMode::Overview,
        };
    }
    
    pub fn cycle_time_filter(&mut self) {
        self.time_filter = match self.time_filter {
            crate::core::SummaryTimeFrame::LastHour => crate::core::SummaryTimeFrame::LastDay,
            crate::core::SummaryTimeFrame::LastDay => crate::core::SummaryTimeFrame::LastWeek,
            crate::core::SummaryTimeFrame::LastWeek => crate::core::SummaryTimeFrame::All,
            crate::core::SummaryTimeFrame::All => crate::core::SummaryTimeFrame::LastHour,
            crate::core::SummaryTimeFrame::Custom(_) => crate::core::SummaryTimeFrame::LastHour,
        };
        self.last_refresh = std::time::Instant::now(); // Trigger refresh
    }
    
    pub fn get_selected_file(&self) -> Option<&crate::core::FileSummaryEntry> {
        self.current_summary.as_ref()?.files.get(self.selected_file_index)
    }
    
    pub fn scroll_diff_up(&mut self) {
        if self.diff_scroll > 0 {
            self.diff_scroll -= 1;
        }
    }
    
    pub fn scroll_diff_down(&mut self) {
        self.diff_scroll += 1;
    }
}

impl SearchState {
    /// Update search query with debouncing
    pub fn update_query_debounced(&mut self, new_query: String) {
        self.pending_query = Some(new_query);
        self.last_update = Some(std::time::Instant::now());
    }
    
    /// Check if enough time has passed to process pending query
    pub fn should_update_search(&self) -> bool {
        if let (Some(last_time), Some(_)) = (self.last_update, &self.pending_query) {
            std::time::Instant::now().duration_since(last_time) > std::time::Duration::from_millis(300)
        } else {
            false
        }
    }
    
    /// Apply pending query update if debounce time has passed
    pub fn apply_pending_update(&mut self) -> bool {
        if self.should_update_search() {
            if let Some(pending) = self.pending_query.take() {
                self.query = pending;
                self.selected_index = 0; // Reset selection when query changes
                return true;
            }
        }
        false
    }
    
    /// Optimized search with caching - called from TuiApp
    pub fn update_filtered_files_optimized(
        &mut self,
        all_files: &std::collections::HashSet<PathBuf>,
        events: &[&crate::core::HighlightedFileEvent],
        search_cache: &mut crate::performance::SearchResultCache,
    ) {
        // Calculate hash of all files to detect file set changes
        let all_files_hash = self.calculate_files_hash(all_files);
        
        if self.query.is_empty() {
            // Show all files when no query
            self.filtered_files = all_files.iter().cloned().collect();
            search_cache.clear();
        } else if search_cache.can_use_incremental(&self.query, all_files_hash) {
            // Use incremental search - filter from previous results
            let base_results = search_cache.get_incremental_base();
            let mut scored_files: Vec<(PathBuf, i32)> = base_results
                .iter()
                .filter_map(|(path, _)| {
                    let score = self.fuzzy_match(path);
                    if score > 0 {
                        Some((path.clone(), score))
                    } else {
                        None
                    }
                })
                .collect();

            // Sort by score and recent activity
            self.sort_search_results(&mut scored_files, events);
            
            // Update cache and extract paths
            search_cache.update(self.query.clone(), scored_files.clone(), all_files_hash);
            self.filtered_files = scored_files.into_iter().map(|(path, _)| path).collect();
        } else {
            // Full search - no cache benefit
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
            
            // Sort by score and recent activity
            self.sort_search_results(&mut scored_files, events);
            
            // Update cache and extract paths
            search_cache.update(self.query.clone(), scored_files.clone(), all_files_hash);
            self.filtered_files = scored_files.into_iter().map(|(path, _)| path).collect();
        }
        
        // Reset selection if out of bounds
        if self.selected_index >= self.filtered_files.len() {
            self.selected_index = 0;
        }
    }

    /// Legacy method for backward compatibility
    pub fn update_filtered_files(&mut self, all_files: &std::collections::HashSet<PathBuf>, events: &[&crate::core::HighlightedFileEvent]) {
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

    /// Calculate a hash of all files for cache invalidation
    fn calculate_files_hash(&self, all_files: &std::collections::HashSet<PathBuf>) -> u64 {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};
        
        let mut hasher = DefaultHasher::new();
        let mut sorted_files: Vec<_> = all_files.iter().collect();
        sorted_files.sort(); // Ensure consistent hash regardless of iteration order
        
        for file in sorted_files {
            file.hash(&mut hasher);
        }
        
        hasher.finish()
    }

    /// Sort search results by score and recent activity
    fn sort_search_results(&self, scored_files: &mut Vec<(PathBuf, i32)>, events: &[&crate::core::HighlightedFileEvent]) {
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
        let mut new_query = self.pending_query.clone().unwrap_or_else(|| self.query.clone());
        new_query.push(c);
        self.update_query_debounced(new_query);
    }
    
    pub fn remove_char(&mut self) {
        let mut new_query = self.pending_query.clone().unwrap_or_else(|| self.query.clone());
        new_query.pop();
        self.update_query_debounced(new_query);
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
    pub summary_state: SummaryState,
    pub review_session: Option<ReviewSession>,
    pub performance_cache: crate::performance::PerformanceCache,
    pub syntax_highlighter: crate::highlight::SyntaxHighlighter,
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
            summary_state: SummaryState::default(),
            review_session: None,
            performance_cache: crate::performance::PerformanceCache::new(),
            syntax_highlighter: crate::highlight::SyntaxHighlighter::new(),
        }
    }

    pub fn run<B: Backend>(mut self, terminal: &mut Terminal<B>) -> io::Result<()> {
        loop {
            terminal.draw(|f| self.ui(f))?;

            // Handle file watcher events with debouncing
            match self.watcher.recv_timeout(Duration::from_millis(50)) {
                Ok(AppEvent::FileChanged(file_event)) => {
                    // Add to debouncer instead of processing immediately
                    self.performance_cache.event_debouncer.add_event(file_event);
                }
                Ok(AppEvent::Quit) => {
                    self.should_quit = true;
                }
                Ok(_) => {}
                Err(_) => {} // Timeout, continue
            }

            // Process debounced events that are ready
            let ready_events = self.performance_cache.event_debouncer.get_ready_events();
            for file_event in ready_events {
                // Invalidate caches for changed files
                self.performance_cache.invalidate_file(&file_event.path);
                
                // Add event to state
                self.state.add_event(file_event);
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
                        
                        // Handle review mode keys
                        if self.app_mode == AppMode::Review {
                            if self.handle_review_keys(&key) {
                                continue; // Key was handled by review mode
                            }
                        }
                        
                        // Handle summary mode keys
                        if self.app_mode == AppMode::Summary {
                            if self.handle_summary_keys(&key) {
                                continue; // Key was handled by summary mode
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
                                    AppMode::Review => {
                                        // Exit review mode
                                        self.app_mode = AppMode::Normal;
                                    }
                                    AppMode::Summary => {
                                        // Exit summary mode
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
                            KeyCode::Char('r') => {
                                // Enter review mode
                                self.enter_review_mode();
                            },
                            KeyCode::Char('s') => {
                                // Enter summary mode
                                self.app_mode = AppMode::Summary;
                                self.summary_state = SummaryState::default();
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
                                // Only allow scrolling if there are long paths that need it
                                if !self.state.watched_files.is_empty() {
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
            AppMode::Review => {
                self.render_review_mode(f);
                return;
            }
            AppMode::Summary => {
                self.render_summary_mode(f);
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
                for event in events.iter().skip(start_idx).take(end_idx - start_idx) {
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

        // Get confidence and origin indicators
        let (confidence_symbol, confidence_color) = if let Some(ref confidence) = event.confidence {
            match confidence.level {
                crate::core::ConfidenceLevel::Safe => ("🟢", Color::Green),
                crate::core::ConfidenceLevel::Review => ("🟡", Color::Yellow), 
                crate::core::ConfidenceLevel::Risky => ("🔴", Color::Red),
            }
        } else {
            ("⚪", Color::Gray)
        };

        let origin_info = match &event.origin {
            crate::core::ChangeOrigin::Human => ("👤", "HUMAN", Color::Cyan),
            crate::core::ChangeOrigin::AIAgent { tool_name, .. } => ("🤖", tool_name.as_str(), Color::Magenta),
            crate::core::ChangeOrigin::Tool { name } => ("🔧", name.as_str(), Color::Blue),
            crate::core::ChangeOrigin::Unknown => ("❓", "UNKNOWN", Color::Gray),
        };

        // Modern header with confidence and origin indicators
        lines.push(Line::from(vec![
            Span::styled(format!("[{}] ", time_str), Style::default().fg(Color::Rgb(100, 100, 100))),
            Span::styled(confidence_symbol, Style::default().fg(confidence_color)),
            Span::styled(format!(" {} {} ", event_symbol, event_type), 
                Style::default().fg(color).bg(bg_color).add_modifier(Modifier::BOLD)),
            Span::styled(format!(" {} ", origin_info.0), Style::default().fg(origin_info.2)),
            Span::styled(format!("{} ", origin_info.1), Style::default().fg(origin_info.2).add_modifier(Modifier::ITALIC)),
            Span::styled(format!(" {} ", event.path.display()), 
                Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
        ]));
        
        // Add confidence details if available
        if let Some(ref confidence) = event.confidence {
            if !confidence.reasons.is_empty() {
                let reasons_text = confidence.reasons.join(", ");
                lines.push(Line::from(vec![
                    Span::styled("| ", Style::default().fg(Color::Rgb(60, 60, 60))),
                    Span::styled(format!("Confidence: {:.1}% - {}", confidence.score * 100.0, reasons_text), 
                        Style::default().fg(Color::Rgb(150, 150, 150)).add_modifier(Modifier::ITALIC)),
                ]));
            }
        }

        // Add batch information if available
        if let Some(ref batch_id) = event.batch_id {
            lines.push(Line::from(vec![
                Span::styled("| ", Style::default().fg(Color::Rgb(60, 60, 60))),
                Span::styled(format!("Batch: {}", batch_id), 
                    Style::default().fg(Color::Rgb(120, 120, 120)).add_modifier(Modifier::ITALIC)),
            ]));
        }

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
                
                // Apply horizontal scrolling to the full path display
                let full_path = path.display().to_string();
                // Use a reasonable max width for horizontal scrolling instead of full terminal width
                // This makes scrolling visible on wide terminals
                let max_display_width = 120; // Maximum characters to display before scrolling
                let available_width = (area.width.saturating_sub(6) as usize).min(max_display_width);
                
                // Debug: Store available width for title display
                let _debug_available_width = available_width;
                
                let displayed_path = if full_path.len() > available_width {
                    // Apply scroll position to long paths
                    if self.file_list_scroll > 0 {
                        // Calculate how much we can actually scroll for this specific path
                        let max_scroll_for_path = full_path.len().saturating_sub(available_width.saturating_sub(1)); // -1 for ellipsis space
                        let actual_scroll = self.file_list_scroll.min(max_scroll_for_path);
                        
                        if actual_scroll > 0 {
                            let start_idx = actual_scroll;
                            let end_idx = (start_idx + available_width.saturating_sub(1)).min(full_path.len());
                            format!("…{}", &full_path[start_idx..end_idx])
                        } else {
                            // Can't scroll this path, just truncate normally
                            format!("{}…", &full_path[..available_width.saturating_sub(1)])
                        }
                    } else {
                        // No scroll, just truncate
                        format!("{}…", &full_path[..available_width.saturating_sub(1)])
                    }
                } else {
                    // Short path, no truncation needed
                    full_path
                };
                
                ListItem::new(Line::from(vec![
                    Span::styled("📄 ", Style::default().fg(Color::Cyan)),
                    Span::styled(displayed_path, style),
                ]))
            })
            .collect();

        let list = List::new(files)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::Rgb(80, 80, 80)))
                    .title(format!(" 📁 Watched Files ({}) (←→ to scroll) [scroll:{} w:{}] ", 
                        self.state.watched_files.len(), 
                        self.file_list_scroll,
                        (area.width.saturating_sub(6) as usize).min(120) // Show the actual available width used
                    ))
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
            Span::styled(" to search, ", Style::default().fg(Color::Rgb(150, 150, 150))),
            Span::styled(" s ", Style::default().fg(Color::White).bg(Color::Magenta).add_modifier(Modifier::BOLD)),
            Span::styled(" for summary, ", Style::default().fg(Color::Rgb(150, 150, 150))),
            Span::styled(" r ", Style::default().fg(Color::White).bg(Color::Blue).add_modifier(Modifier::BOLD)),
            Span::styled(" for review | ", Style::default().fg(Color::Rgb(150, 150, 150))),
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

    fn render_review_mode(&mut self, f: &mut Frame) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .margin(1)
            .constraints([
                Constraint::Min(3),         // Review header with stats
                Constraint::Percentage(60), // Current change diff
                Constraint::Percentage(25), // Hunk list
                Constraint::Min(3),         // Review controls help
            ])
            .split(f.area());

        self.render_review_header(f, chunks[0]);
        self.render_review_diff(f, chunks[1]);
        self.render_review_hunks(f, chunks[2]);
        self.render_review_controls(f, chunks[3]);
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
        // Show pending query for immediate visual feedback, fall back to committed query
        let display_query = self.search_state.pending_query
            .as_ref()
            .unwrap_or(&self.search_state.query);
        
        // Create input text with visual cursor indicator
        let prefix = "🔍 ";
        let input_text = format!("{}{}█", prefix, display_query);
        
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
        let cursor_x = area.x + 1 + prefix.chars().count() as u16 + display_query.len() as u16 + 1;
        let cursor_y = area.y + 1;
        
        // Ensure cursor is within bounds
        if cursor_x < area.x + area.width - 1 {
            f.set_cursor_position((cursor_x, cursor_y));
        }
    }

    fn render_search_results(&mut self, f: &mut Frame, area: Rect) {
        // Apply pending query updates if debounce time has passed
        let should_refresh = self.search_state.apply_pending_update();
        
        // Only update filtered files if query changed or this is first time
        if should_refresh || self.search_state.filtered_files.is_empty() {
            // Convert VecDeque to slice for compatibility
            let events_slice: Vec<_> = self.state.highlighted_events.iter().collect();
            self.search_state.update_filtered_files_optimized(
                &self.state.watched_files,
                &events_slice,
                &mut self.performance_cache.search_results,
            );
        }
        
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
        let selected_file = self.search_state.get_selected_file().cloned();
        
        if let Some(file_path) = selected_file {
            // Try to read file content using performance cache
            match self.performance_cache.file_content.get_content(&file_path) {
                Ok(content) => {
                    let language = self.syntax_highlighter
                        .get_language_from_path(&file_path)
                        .unwrap_or_else(|| "Plain Text".to_string());
                    
                    // Check if file has recent changes for diff preview
                    let recent_event = self.state.highlighted_events
                        .iter()
                        .find(|e| e.path == file_path);
                    
                    if let Some(event) = recent_event {
                        self.render_diff_preview(f, area, &file_path, &content, event);
                    } else {
                        self.render_file_content_preview(f, area, &file_path, &content, &language);
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

    fn render_file_content_preview(&mut self, f: &mut Frame, area: Rect, file_path: &std::path::Path, content: &str, language: &str) {
        let visible_height = area.height as usize - 2; // Account for borders
        let lines: Vec<&str> = content.lines().collect();
        
        let start_line = self.search_state.preview_scroll;
        let end_line = (start_line + visible_height).min(lines.len());
        
        // Always highlight entire content for proper syntax context
        // The LRU cache will handle memory management efficiently
        let highlighted_content = self.performance_cache.syntax_highlight.get_highlighted_content(
            &file_path.to_path_buf(),
            content,
            language,
            &self.syntax_highlighter,
        );
        
        let visible_lines: Vec<Line> = (start_line..end_line)
            .map(|absolute_line_idx| {
                let line_num = absolute_line_idx + 1;
                let line_num_span = Span::styled(
                    format!("{:4} │ ", line_num), 
                    Style::default().fg(Color::Rgb(100, 100, 100))
                );
                
                let mut spans = vec![line_num_span];
                
                // Get highlighted spans for this line from the pre-highlighted content
                // Always use absolute index since we now highlight entire content
                let highlight_idx = absolute_line_idx;
                
                if let Some(line_spans) = highlighted_content.get(highlight_idx) {
                    for (style, text) in line_spans {
                        spans.push(Span::styled(text.clone(), style.clone()));
                    }
                } else if let Some(plain_line) = lines.get(absolute_line_idx) {
                    // Fallback to plain text if highlighting failed
                    spans.push(Span::raw(*plain_line));
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
        let popup_area = self.centered_rect(80, 75, f.area());

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
                Span::styled("Summary Mode", Style::default().fg(Color::Magenta).add_modifier(Modifier::BOLD)),
                Span::styled(" (Press s):", Style::default())
            ]),
            Line::from(""),
            Line::from(vec![
                Span::styled("  s          ", Style::default().fg(Color::Magenta).add_modifier(Modifier::BOLD)),
                Span::styled("- Enter summary mode", Style::default())
            ]),
            Line::from(vec![
                Span::styled("  ↑/↓, j/k   ", Style::default().fg(Color::Magenta).add_modifier(Modifier::BOLD)),
                Span::styled("- Navigate files (overview) / scroll diff (detail)", Style::default())
            ]),
            Line::from(vec![
                Span::styled("  Enter      ", Style::default().fg(Color::Magenta).add_modifier(Modifier::BOLD)),
                Span::styled("- View selected file's diff", Style::default())
            ]),
            Line::from(vec![
                Span::styled("  Esc        ", Style::default().fg(Color::Magenta).add_modifier(Modifier::BOLD)),
                Span::styled("- Back to overview / exit summary", Style::default())
            ]),
            Line::from(vec![
                Span::styled("  t          ", Style::default().fg(Color::Magenta).add_modifier(Modifier::BOLD)),
                Span::styled("- Cycle time filter (Hour/Day/Week/All)", Style::default())
            ]),
            Line::from(vec![
                Span::styled("  o          ", Style::default().fg(Color::Magenta).add_modifier(Modifier::BOLD)),
                Span::styled("- Cycle origin filter (Human/AI/Tool/All)", Style::default())
            ]),
            Line::from(vec![
                Span::styled("  r          ", Style::default().fg(Color::Magenta).add_modifier(Modifier::BOLD)),
                Span::styled("- Force refresh summary", Style::default())
            ]),
            Line::from(""),
            Line::from(vec![
                Span::styled("Review Mode", Style::default().fg(Color::Blue).add_modifier(Modifier::BOLD)),
                Span::styled(" (Press r):", Style::default())
            ]),
            Line::from(""),
            Line::from(vec![
                Span::styled("  r          ", Style::default().fg(Color::Blue).add_modifier(Modifier::BOLD)),
                Span::styled("- Enter review mode", Style::default())
            ]),
            Line::from(vec![
                Span::styled("  a/d        ", Style::default().fg(Color::Blue).add_modifier(Modifier::BOLD)),
                Span::styled("- Accept/reject current change", Style::default())
            ]),
            Line::from(vec![
                Span::styled("  s          ", Style::default().fg(Color::Blue).add_modifier(Modifier::BOLD)),
                Span::styled("- Skip current change", Style::default())
            ]),
            Line::from(vec![
                Span::styled("  n/p        ", Style::default().fg(Color::Blue).add_modifier(Modifier::BOLD)),
                Span::styled("- Next/previous change", Style::default())
            ]),
            Line::from(vec![
                Span::styled("  j/k        ", Style::default().fg(Color::Blue).add_modifier(Modifier::BOLD)),
                Span::styled("- Next/previous hunk", Style::default())
            ]),
            Line::from(vec![
                Span::styled("  1-5        ", Style::default().fg(Color::Blue).add_modifier(Modifier::BOLD)),
                Span::styled("- Apply filter presets", Style::default())
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
            Line::from("• Change summary with statistics and filtering"),
            Line::from("• AI origin detection and confidence scoring"),
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
        // Only allow scrolling if there are files to scroll
        if !self.state.watched_files.is_empty() {
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
        // In diff view context, move to rightmost position of file list
        // Set to a high value, the render function will clamp it appropriately
        self.file_list_scroll = 1000; // Will be clamped during rendering
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
    
    /// Enter interactive review mode
    fn enter_review_mode(&mut self) {
        if self.review_session.is_none() {
            let mut session = ReviewSession::new();
            
            // Add all current events to the review session
            for event in &self.state.events {
                session.add_change(event.clone());
            }
            
            // Only enter review mode if there are changes to review
            if !session.changes.is_empty() {
                self.review_session = Some(session);
                self.app_mode = AppMode::Review;
            }
        } else {
            // Resume existing review session
            self.app_mode = AppMode::Review;
        }
    }
    
    /// Handle keyboard input in review mode
    fn handle_review_keys(&mut self, key: &crossterm::event::KeyEvent) -> bool {
        use crossterm::event::KeyCode;
        
        match key.code {
            // Accept current hunk/change
            KeyCode::Char('a') => {
                self.review_accept_current();
                true
            }
            // Reject current hunk/change
            KeyCode::Char('d') => {
                self.review_reject_current();
                true
            }
            // Skip current hunk/change
            KeyCode::Char('s') => {
                self.review_skip_current();
                true
            }
            // Accept all hunks in current change
            KeyCode::Char('A') => {
                self.review_accept_all_current();
                true
            }
            // Reject all hunks in current change
            KeyCode::Char('D') => {
                self.review_reject_all_current();
                true
            }
            // Navigate to next change
            KeyCode::Char('n') | KeyCode::Right => {
                self.review_next_change();
                true
            }
            // Navigate to previous change
            KeyCode::Char('p') | KeyCode::Left => {
                self.review_previous_change();
                true
            }
            // Navigate to next hunk
            KeyCode::Char('j') | KeyCode::Down => {
                self.review_next_hunk();
                true
            }
            // Navigate to previous hunk
            KeyCode::Char('k') | KeyCode::Up => {
                self.review_previous_hunk();
                true
            }
            // Jump to next risky change
            KeyCode::Char('R') => {
                self.review_next_risky();
                true
            }
            // Jump to first unreviewed
            KeyCode::Char('u') => {
                self.review_first_unreviewed();
                true
            }
            // Toggle filters
            KeyCode::Char('f') => {
                self.review_toggle_filters();
                true
            }
            // Filter presets (1-5 keys)
            KeyCode::Char('1') => {
                self.apply_filter_preset(0);
                true
            }
            KeyCode::Char('2') => {
                self.apply_filter_preset(1);
                true
            }
            KeyCode::Char('3') => {
                self.apply_filter_preset(2);
                true
            }
            KeyCode::Char('4') => {
                self.apply_filter_preset(3);
                true
            }
            KeyCode::Char('5') => {
                self.apply_filter_preset(4);
                true
            }
            // Session management
            KeyCode::Char('S') => {
                self.save_review_session();
                true
            }
            KeyCode::Char('L') => {
                self.show_session_list();
                true
            }
            // Show help
            KeyCode::Char('?') => {
                // Could show review-specific help
                self.app_mode = AppMode::Help;
                true
            }
            _ => false, // Let other keys pass through to main handler
        }
    }
    
    /// Review action implementations
    fn review_accept_current(&mut self) {
        let hunk_id = if let Some(ref session) = self.review_session {
            session.get_current_hunk().map(|h| h.id.clone())
        } else {
            None
        };
        
        if let (Some(hunk_id), Some(ref mut session)) = (hunk_id, &mut self.review_session) {
            if let Some(current_change) = session.get_current_change_mut() {
                current_change.accept_hunk(&hunk_id);
            }
        }
    }
    
    fn review_reject_current(&mut self) {
        let hunk_id = if let Some(ref session) = self.review_session {
            session.get_current_hunk().map(|h| h.id.clone())
        } else {
            None
        };
        
        if let (Some(hunk_id), Some(ref mut session)) = (hunk_id, &mut self.review_session) {
            if let Some(current_change) = session.get_current_change_mut() {
                current_change.reject_hunk(&hunk_id);
            }
        }
    }
    
    fn review_skip_current(&mut self) {
        let hunk_id = if let Some(ref session) = self.review_session {
            session.get_current_hunk().map(|h| h.id.clone())
        } else {
            None
        };
        
        if let (Some(hunk_id), Some(ref mut session)) = (hunk_id, &mut self.review_session) {
            if let Some(current_change) = session.get_current_change_mut() {
                current_change.skip_hunk(&hunk_id);
            }
        }
    }
    
    fn review_accept_all_current(&mut self) {
        if let Some(ref mut session) = self.review_session {
            if let Some(current_change) = session.get_current_change_mut() {
                current_change.accept_all();
            }
        }
    }
    
    fn review_reject_all_current(&mut self) {
        if let Some(ref mut session) = self.review_session {
            if let Some(current_change) = session.get_current_change_mut() {
                current_change.reject_all();
            }
        }
    }
    
    fn review_next_change(&mut self) {
        if let Some(ref mut session) = self.review_session {
            session.navigate(ReviewNavigationAction::NextChange);
        }
    }
    
    fn review_previous_change(&mut self) {
        if let Some(ref mut session) = self.review_session {
            session.navigate(ReviewNavigationAction::PreviousChange);
        }
    }
    
    fn review_next_hunk(&mut self) {
        if let Some(ref mut session) = self.review_session {
            session.navigate(ReviewNavigationAction::NextHunk);
        }
    }
    
    fn review_previous_hunk(&mut self) {
        if let Some(ref mut session) = self.review_session {
            session.navigate(ReviewNavigationAction::PreviousHunk);
        }
    }
    
    fn review_next_risky(&mut self) {
        if let Some(ref mut session) = self.review_session {
            session.navigate(ReviewNavigationAction::NextRiskyChange);
        }
    }
    
    fn review_first_unreviewed(&mut self) {
        if let Some(ref mut session) = self.review_session {
            session.navigate(ReviewNavigationAction::FirstUnreviewed);
        }
    }
    
    fn review_toggle_filters(&mut self) {
        if let Some(ref mut session) = self.review_session {
            // Toggle between different filter states
            if session.filters.show_only_risky {
                session.filters.show_only_risky = false;
                session.filters.show_only_ai_changes = true;
            } else if session.filters.show_only_ai_changes {
                session.filters.show_only_ai_changes = false;
            } else {
                session.filters.show_only_risky = true;
            }
        }
    }
    
    /// Apply a filter preset by index
    fn apply_filter_preset(&mut self, preset_index: usize) {
        if let Some(ref mut session) = self.review_session {
            let presets = ReviewSession::get_default_presets();
            if let Some(preset) = presets.get(preset_index) {
                session.apply_filter_preset(preset);
            }
        }
    }
    
    /// Save current review session to disk
    fn save_review_session(&mut self) {
        if let Some(ref session) = self.review_session {
            // Try to save to current directory or a default location
            let base_dir = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
            match session.save_to_disk(&base_dir) {
                Ok(saved_path) => {
                    // Could show a success message - for now just continue silently
                    let _ = saved_path;
                }
                Err(_) => {
                    // Could show an error message - for now just continue silently
                }
            }
        }
    }
    
    /// Show list of saved sessions (placeholder for future implementation)
    fn show_session_list(&mut self) {
        // For now, just return - in the future this could show a session picker
        // that allows loading saved sessions
    }
    
    /// Render the review mode header with session stats and current file info
    fn render_review_header(&mut self, f: &mut Frame, area: Rect) {
        let session = match &self.review_session {
            Some(s) => s,
            None => {
                let no_session = Paragraph::new("No active review session")
                    .block(Block::default().borders(Borders::ALL).title(" Review Mode "));
                f.render_widget(no_session, area);
                return;
            }
        };
        
        let stats = session.get_review_stats();
        let current_change = session.get_current_change();
        
        // Create filter indicator
        let filter_text = self.get_active_filters_text(&session.filters);
        
        let header_text = if let Some(change) = current_change {
            let confidence_text = if let Some(ref conf) = change.event.confidence {
                format!(" {:.0}%", conf.score * 100.0)
            } else {
                " N/A".to_string()
            };
            
            let origin_text = match &change.event.origin {
                crate::core::ChangeOrigin::AIAgent { tool_name, .. } => format!("🤖 {}", tool_name),
                crate::core::ChangeOrigin::Human => "👤 Human".to_string(),
                crate::core::ChangeOrigin::Tool { name } => format!("🔧 {}", name),
                crate::core::ChangeOrigin::Unknown => "❓ Unknown".to_string(),
            };
            
            let mut lines = vec![
                format!(
                    "📁 {} | {} | Confidence:{} | Progress: {}/{} ({:.1}%)",
                    change.event.path.display(),
                    origin_text,
                    confidence_text,
                    stats.total - stats.pending,
                    stats.total,
                    stats.completion_percentage()
                )
            ];
            
            if !filter_text.is_empty() {
                lines.push(format!("🔍 Filters: {}", filter_text));
            }
            
            lines.join("\n")
        } else {
            let mut lines = vec![
                format!(
                    "No changes to review | Progress: {}/{} ({:.1}%)",
                    stats.total - stats.pending,
                    stats.total,
                    stats.completion_percentage()
                )
            ];
            
            if !filter_text.is_empty() {
                lines.push(format!("🔍 Filters: {}", filter_text));
            }
            
            lines.join("\n")
        };
        
        let header = Paragraph::new(header_text)
            .block(Block::default()
                .borders(Borders::ALL)
                .title(" 🔍 Interactive Review Mode ")
                .title_style(Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)))
            .wrap(Wrap { trim: true });
        
        f.render_widget(header, area);
    }
    
    /// Get text description of active filters
    fn get_active_filters_text(&self, filters: &crate::review::ReviewFilters) -> String {
        let mut active_filters = Vec::new();
        
        if filters.show_only_risky {
            active_filters.push("Risky Only".to_string());
        }
        if filters.show_only_ai_changes {
            active_filters.push("AI Only".to_string());
        }
        if filters.show_only_pending {
            active_filters.push("Pending Only".to_string());
        }
        if filters.exclude_reviewed {
            active_filters.push("Exclude Reviewed".to_string());
        }
        if let Some(ref level) = filters.confidence_level {
            active_filters.push(format!("Confidence: {:?}", level));
        }
        if let Some(threshold) = filters.confidence_threshold {
            active_filters.push(format!("Threshold: {:.0}%", threshold * 100.0));
        }
        if let Some(ref pattern) = filters.file_pattern {
            active_filters.push(format!("Pattern: {}", pattern));
        }
        if let Some(min) = filters.min_hunks {
            active_filters.push(format!("Min Hunks: {}", min));
        }
        if let Some(max) = filters.max_hunks {
            active_filters.push(format!("Max Hunks: {}", max));
        }
        
        if active_filters.is_empty() {
            String::new()
        } else {
            active_filters.join(", ")
        }
    }
    
    /// Render the current change's diff with hunk highlighting
    fn render_review_diff(&mut self, f: &mut Frame, area: Rect) {
        let session = match &self.review_session {
            Some(s) => s,
            None => return,
        };
        
        let current_change = match session.get_current_change() {
            Some(c) => c,
            None => {
                let empty = Paragraph::new("No changes to review")
                    .block(Block::default().borders(Borders::ALL).title(" Current Change "));
                f.render_widget(empty, area);
                return;
            }
        };
        
        let current_hunk = session.get_current_hunk();
        let mut lines = Vec::new();
        
        // Show file header
        lines.push(Line::from(vec![
            Span::styled(format!("--- {}", current_change.event.path.display()), 
                Style::default().fg(Color::Red)),
        ]));
        lines.push(Line::from(vec![
            Span::styled(format!("+++ {}", current_change.event.path.display()), 
                Style::default().fg(Color::Green)),
        ]));
        
        // Show hunks with highlighting for current hunk
        for (_hunk_idx, hunk) in current_change.hunks.iter().enumerate() {
            let is_current_hunk = current_hunk.map(|h| h.id == hunk.id).unwrap_or(false);
            let action = current_change.review_actions.get(&hunk.id).unwrap_or(&ReviewAction::Pending);
            
            // Hunk header with review status
            let status_symbol = match action {
                ReviewAction::Accept => "✅",
                ReviewAction::Reject => "❌", 
                ReviewAction::Skip => "⏭️",
                ReviewAction::Pending => "⏳",
            };
            
            let header_style = if is_current_hunk {
                Style::default().bg(Color::DarkGray).fg(Color::White).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::Cyan)
            };
            
            lines.push(Line::from(vec![
                Span::styled(format!("{} {} ", status_symbol, hunk.header), header_style),
            ]));
            
            // Show hunk lines
            for line in &hunk.lines {
                let line_style = if is_current_hunk {
                    if line.starts_with('+') {
                        Style::default().fg(Color::Green).bg(Color::Rgb(0, 25, 0))
                    } else if line.starts_with('-') {
                        Style::default().fg(Color::Red).bg(Color::Rgb(25, 0, 0))
                    } else {
                        Style::default().bg(Color::Rgb(10, 10, 10))
                    }
                } else {
                    if line.starts_with('+') {
                        Style::default().fg(Color::Green)
                    } else if line.starts_with('-') {
                        Style::default().fg(Color::Red)
                    } else {
                        Style::default().fg(Color::Gray)
                    }
                };
                
                lines.push(Line::from(vec![
                    Span::styled(line.clone(), line_style),
                ]));
            }
            lines.push(Line::from(""));
        }
        
        let diff_widget = Paragraph::new(lines)
            .block(Block::default()
                .borders(Borders::ALL)
                .title(" Current Change Diff ")
                .title_style(Style::default().fg(Color::Cyan)))
            .wrap(Wrap { trim: true });
        
        f.render_widget(diff_widget, area);
    }
    
    /// Render the list of hunks with their review status
    fn render_review_hunks(&mut self, f: &mut Frame, area: Rect) {
        let session = match &self.review_session {
            Some(s) => s,
            None => return,
        };
        
        let current_change = match session.get_current_change() {
            Some(c) => c,
            None => return,
        };
        
        let current_hunk = session.get_current_hunk();
        let items: Vec<ListItem> = current_change.hunks.iter().enumerate().map(|(idx, hunk)| {
            let is_current = current_hunk.map(|h| h.id == hunk.id).unwrap_or(false);
            let action = current_change.review_actions.get(&hunk.id).unwrap_or(&ReviewAction::Pending);
            
            let status_symbol = match action {
                ReviewAction::Accept => "✅",
                ReviewAction::Reject => "❌",
                ReviewAction::Skip => "⏭️", 
                ReviewAction::Pending => "⏳",
            };
            
            let hunk_type_symbol = match hunk.hunk_type {
                crate::review::HunkType::Addition => "+",
                crate::review::HunkType::Deletion => "-",
                crate::review::HunkType::Modification => "~",
                crate::review::HunkType::Context => " ",
            };
            
            let text = format!("{} {} Hunk {} ({}:{})", 
                status_symbol, hunk_type_symbol, idx + 1, hunk.old_start, hunk.new_start);
            
            let style = if is_current {
                Style::default().bg(Color::DarkGray).fg(Color::White)
            } else {
                Style::default()
            };
            
            ListItem::new(text).style(style)
        }).collect();
        
        let hunks_list = List::new(items)
            .block(Block::default()
                .borders(Borders::ALL)
                .title(" Hunks ")
                .title_style(Style::default().fg(Color::Yellow)));
        
        f.render_widget(hunks_list, area);
    }
    
    /// Render the review controls help
    fn render_review_controls(&mut self, f: &mut Frame, area: Rect) {
        let controls_lines = vec![
            "Review: a=Accept | d=Reject | s=Skip | A=Accept All | D=Reject All",
            "Navigate: n/p=Next/Prev Change | j/k=Next/Prev Hunk | R=Next Risky | u=First Unreviewed",
            "Filter Presets: 1=Risky | 2=AI | 3=Pending | 4=Low Confidence | 5=Large Changes",
            "Session: S=Save | L=Load | f=Toggle Filters | ?=Help | q=Exit"
        ];
        
        let controls = Paragraph::new(controls_lines.join("\n"))
            .block(Block::default()
                .borders(Borders::ALL)
                .title(" Controls ")
                .title_style(Style::default().fg(Color::Green)))
            .wrap(Wrap { trim: true });
        
        f.render_widget(controls, area);
    }

    fn render_summary_mode(&mut self, f: &mut Frame) {
        // Refresh summary if needed
        self.refresh_summary_if_needed();

        match self.summary_state.view_mode {
            SummaryViewMode::Overview => {
                self.render_summary_overview(f);
            }
            SummaryViewMode::FileDetail => {
                self.render_summary_file_detail(f, f.area());
            }
        }
    }

    fn refresh_summary_if_needed(&mut self) {
        // Refresh every 5 seconds or when time filter changes
        let should_refresh = self.summary_state.current_summary.is_none() ||
            std::time::Instant::now().duration_since(self.summary_state.last_refresh) > std::time::Duration::from_secs(5);

        if should_refresh {
            let mut filters = crate::core::SummaryFilters::default();
            filters.time_frame = self.summary_state.time_filter;
            
            if let Some(ref origin) = self.summary_state.origin_filter {
                filters.include_origins = vec![origin.clone()];
            }

            self.summary_state.current_summary = Some(self.state.generate_summary(&filters));
            self.summary_state.last_refresh = std::time::Instant::now();
        }
    }

    fn render_summary_overview(&mut self, f: &mut Frame) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .margin(1)
            .constraints([
                Constraint::Length(6),      // Summary stats
                Constraint::Min(10),        // File list
                Constraint::Length(3),      // Controls help
            ])
            .split(f.area());

        self.render_summary_stats(f, chunks[0]);
        self.render_summary_file_list(f, chunks[1]);
        self.render_summary_controls(f, chunks[2]);
    }

    fn render_summary_stats(&self, f: &mut Frame, area: Rect) {
        let summary = match &self.summary_state.current_summary {
            Some(s) => s,
            None => {
                let loading = Paragraph::new("Loading summary...")
                    .block(Block::default().borders(Borders::ALL).title(" Summary "));
                f.render_widget(loading, area);
                return;
            }
        };

        let stats = &summary.stats;
        let timeframe_text = match self.summary_state.time_filter {
            crate::core::SummaryTimeFrame::LastHour => "Last Hour",
            crate::core::SummaryTimeFrame::LastDay => "Last Day",
            crate::core::SummaryTimeFrame::LastWeek => "Last Week",
            crate::core::SummaryTimeFrame::All => "All Time",
            crate::core::SummaryTimeFrame::Custom(_) => "Custom",
        };

        let stats_text = vec![
            Line::from(vec![
                Span::styled("📊 Change Summary", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
                Span::styled(format!(" ({})", timeframe_text), Style::default().fg(Color::Gray)),
            ]),
            Line::from(""),
            Line::from(vec![
                Span::styled("Total Files: ", Style::default().fg(Color::White)),
                Span::styled(format!("{}", stats.total_files), Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
                Span::styled("  Changes: ", Style::default().fg(Color::White)),
                Span::styled(format!("{}", stats.total_changes), Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
            ]),
            Line::from(vec![
                Span::styled("🟢 Created: ", Style::default().fg(Color::Green)),
                Span::styled(format!("{}", stats.files_created), Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
                Span::styled("  🟡 Modified: ", Style::default().fg(Color::Yellow)),
                Span::styled(format!("{}", stats.files_modified), Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
                Span::styled("  🔴 Deleted: ", Style::default().fg(Color::Red)),
                Span::styled(format!("{}", stats.files_deleted), Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)),
            ]),
        ];

        let stats_widget = Paragraph::new(stats_text)
            .block(Block::default()
                .borders(Borders::ALL)
                .title(" Summary Statistics "));

        f.render_widget(stats_widget, area);
    }

    fn render_summary_file_list(&mut self, f: &mut Frame, area: Rect) {
        let summary = match &self.summary_state.current_summary {
            Some(s) => s,
            None => return,
        };

        let files: Vec<ListItem> = summary.files
            .iter()
            .enumerate()
            .map(|(i, file)| {
                let (event_symbol, color) = match &file.change_type {
                    crate::core::FileEventKind::Created => ("●", Color::Green),
                    crate::core::FileEventKind::Modified => ("●", Color::Yellow),
                    crate::core::FileEventKind::Deleted => ("●", Color::Red),
                    crate::core::FileEventKind::Moved { .. } => ("●", Color::Blue),
                };

                let origin_symbol = match &file.changed_by {
                    crate::core::ChangeOrigin::Human => "👤",
                    crate::core::ChangeOrigin::AIAgent { .. } => "🤖",
                    crate::core::ChangeOrigin::Tool { .. } => "🔧",
                    crate::core::ChangeOrigin::Unknown => "❓",
                };

                let _confidence_color = match &file.confidence_level {
                    Some(crate::core::ConfidenceLevel::Safe) => Color::Green,
                    Some(crate::core::ConfidenceLevel::Review) => Color::Yellow,
                    Some(crate::core::ConfidenceLevel::Risky) => Color::Red,
                    None => Color::Gray,
                };

                let time_ago = if let Ok(duration) = std::time::SystemTime::now().duration_since(file.changed_at) {
                    if duration.as_secs() < 60 {
                        format!("{}s ago", duration.as_secs())
                    } else if duration.as_secs() < 3600 {
                        format!("{}m ago", duration.as_secs() / 60)
                    } else if duration.as_secs() < 86400 {
                        format!("{}h ago", duration.as_secs() / 3600)
                    } else {
                        format!("{}d ago", duration.as_secs() / 86400)
                    }
                } else {
                    "now".to_string()
                };

                let style = if i == self.summary_state.selected_file_index {
                    Style::default().bg(Color::DarkGray).fg(Color::White)
                } else {
                    Style::default()
                };

                let path_display = file.path.to_string_lossy();
                let truncated_path = if path_display.len() > 50 {
                    format!("...{}", &path_display[path_display.len() - 47..])
                } else {
                    path_display.to_string()
                };

                ListItem::new(Line::from(vec![
                    Span::styled(format!("{} ", event_symbol), Style::default().fg(color)),
                    Span::styled(format!("{} ", origin_symbol), Style::default()),
                    Span::styled(truncated_path, style.fg(Color::White)),
                    Span::styled(format!(" [{}]", time_ago), style.fg(Color::Gray)),
                    if file.change_count > 1 {
                        Span::styled(format!(" ({}×)", file.change_count), style.fg(Color::Cyan))
                    } else {
                        Span::raw("")
                    },
                ])).style(style)
            })
            .collect();

        let file_list = List::new(files)
            .block(Block::default()
                .borders(Borders::ALL)
                .title(" Files "))
            .highlight_style(Style::default().add_modifier(Modifier::BOLD));

        f.render_widget(file_list, area);
    }

    fn render_summary_file_detail(&mut self, f: &mut Frame, area: Rect) {
        // Clone the selected file to avoid borrow checker issues
        let selected_file = match self.summary_state.get_selected_file() {
            Some(file) => file.clone(),
            None => {
                let no_file = Paragraph::new("No file selected")
                    .block(Block::default().borders(Borders::ALL).title(" File Detail "));
                f.render_widget(no_file, area);
                return;
            }
        };

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .margin(1)
            .constraints([
                Constraint::Length(4),      // File info
                Constraint::Min(10),        // Diff view
                Constraint::Length(2),      // Controls
            ])
            .split(area);

        self.render_file_info(f, chunks[0], &selected_file);
        self.render_file_diff(f, chunks[1], &selected_file);
        self.render_file_detail_controls(f, chunks[2]);
    }

    fn render_file_info(&self, f: &mut Frame, area: Rect, file: &crate::core::FileSummaryEntry) {
        let (event_symbol, event_type, color) = match &file.change_type {
            crate::core::FileEventKind::Created => ("●", "CREATED", Color::Green),
            crate::core::FileEventKind::Modified => ("●", "MODIFIED", Color::Yellow),
            crate::core::FileEventKind::Deleted => ("●", "DELETED", Color::Red),
            crate::core::FileEventKind::Moved { .. } => ("●", "MOVED", Color::Blue),
        };

        let origin_text = match &file.changed_by {
            crate::core::ChangeOrigin::Human => "👤 Human",
            crate::core::ChangeOrigin::AIAgent { tool_name, .. } => &format!("🤖 {}", tool_name),
            crate::core::ChangeOrigin::Tool { name } => &format!("🔧 {}", name),
            crate::core::ChangeOrigin::Unknown => "❓ Unknown",
        };

        let time_display = match file.changed_at.duration_since(std::time::UNIX_EPOCH) {
            Ok(duration) => {
                let datetime = std::time::SystemTime::UNIX_EPOCH + duration;
                // Simple timestamp formatting
                format!("{:?}", datetime)
            }
            Err(_) => "Unknown time".to_string(),
        };

        let info_text = vec![
            Line::from(vec![
                Span::styled(format!("{} {} ", event_symbol, event_type), Style::default().fg(color).add_modifier(Modifier::BOLD)),
                Span::styled(file.path.to_string_lossy(), Style::default().fg(Color::White)),
            ]),
            Line::from(vec![
                Span::styled("Changed by: ", Style::default().fg(Color::Gray)),
                Span::styled(origin_text, Style::default().fg(Color::Cyan)),
                Span::styled(format!("  At: {}", time_display), Style::default().fg(Color::Gray)),
            ]),
        ];

        let info_widget = Paragraph::new(info_text)
            .block(Block::default().borders(Borders::ALL).title(" File Information "));

        f.render_widget(info_widget, area);
    }

    fn render_file_diff(&mut self, f: &mut Frame, area: Rect, file: &crate::core::FileSummaryEntry) {
        let diff_text = if file.has_diff {
            // Try to find the actual event to get the diff
            let event = self.state.events.iter()
                .find(|e| e.path == file.path)
                .and_then(|e| e.diff.as_ref());

            match event {
                Some(diff) => {
                    let lines: Vec<&str> = diff.lines().collect();
                    let start_line = self.summary_state.diff_scroll;
                    let end_line = (start_line + area.height as usize - 2).min(lines.len());
                    
                    lines[start_line..end_line].join("\n")
                }
                None => {
                    if let Some(ref preview) = file.preview {
                        format!("Preview:\n{}", preview)
                    } else {
                        "No diff available".to_string()
                    }
                }
            }
        } else {
            match &file.change_type {
                crate::core::FileEventKind::Created => "File was created",
                crate::core::FileEventKind::Deleted => "File was deleted",
                _ => "No diff available",
            }.to_string()
        };

        let diff_widget = Paragraph::new(diff_text)
            .block(Block::default().borders(Borders::ALL).title(" Diff "))
            .wrap(Wrap { trim: true });

        f.render_widget(diff_widget, area);
    }

    fn render_summary_controls(&self, f: &mut Frame, area: Rect) {
        let controls_text = "Controls: j/k=Navigate | Enter=View Detail | t=Time Filter | o=Origin Filter | q=Exit";
        
        let controls = Paragraph::new(controls_text)
            .block(Block::default().borders(Borders::ALL))
            .alignment(Alignment::Center);

        f.render_widget(controls, area);
    }

    fn render_file_detail_controls(&self, f: &mut Frame, area: Rect) {
        let controls_text = "Controls: j/k=Scroll Diff | Esc=Back to Overview | q=Exit";
        
        let controls = Paragraph::new(controls_text)
            .alignment(Alignment::Center);

        f.render_widget(controls, area);
    }

    /// Handle keyboard input in summary mode
    fn handle_summary_keys(&mut self, key: &crossterm::event::KeyEvent) -> bool {
        match key.code {
            KeyCode::Up | KeyCode::Char('k') => {
                match self.summary_state.view_mode {
                    SummaryViewMode::Overview => {
                        self.summary_state.move_up();
                    }
                    SummaryViewMode::FileDetail => {
                        self.summary_state.scroll_diff_up();
                    }
                }
                true
            }
            KeyCode::Down | KeyCode::Char('j') => {
                match self.summary_state.view_mode {
                    SummaryViewMode::Overview => {
                        let max_items = self.summary_state.current_summary
                            .as_ref()
                            .map(|s| s.files.len())
                            .unwrap_or(0);
                        self.summary_state.move_down(max_items);
                    }
                    SummaryViewMode::FileDetail => {
                        self.summary_state.scroll_diff_down();
                    }
                }
                true
            }
            KeyCode::Enter => {
                if self.summary_state.view_mode == SummaryViewMode::Overview {
                    self.summary_state.view_mode = SummaryViewMode::FileDetail;
                    self.summary_state.diff_scroll = 0; // Reset scroll when entering detail view
                }
                true
            }
            KeyCode::Esc => {
                if self.summary_state.view_mode == SummaryViewMode::FileDetail {
                    self.summary_state.view_mode = SummaryViewMode::Overview;
                } else {
                    // Exit summary mode if already in overview
                    self.app_mode = AppMode::Normal;
                }
                true
            }
            KeyCode::Char('t') => {
                // Cycle through time filters
                self.summary_state.cycle_time_filter();
                true
            }
            KeyCode::Char('o') => {
                // Cycle through origin filters
                self.summary_state.origin_filter = match &self.summary_state.origin_filter {
                    None => Some(crate::core::ChangeOrigin::Human),
                    Some(crate::core::ChangeOrigin::Human) => Some(crate::core::ChangeOrigin::AIAgent {
                        tool_name: "Any AI".to_string(),
                        process_id: None,
                    }),
                    Some(crate::core::ChangeOrigin::AIAgent { .. }) => Some(crate::core::ChangeOrigin::Tool {
                        name: "Any Tool".to_string(),
                    }),
                    Some(crate::core::ChangeOrigin::Tool { .. }) => Some(crate::core::ChangeOrigin::Unknown),
                    Some(crate::core::ChangeOrigin::Unknown) => None,
                };
                self.summary_state.last_refresh = std::time::Instant::now(); // Trigger refresh
                true
            }
            KeyCode::PageUp => {
                match self.summary_state.view_mode {
                    SummaryViewMode::Overview => {
                        // Move up by 10 files
                        for _ in 0..10 {
                            self.summary_state.move_up();
                        }
                    }
                    SummaryViewMode::FileDetail => {
                        // Scroll diff up by 10 lines
                        for _ in 0..10 {
                            self.summary_state.scroll_diff_up();
                        }
                    }
                }
                true
            }
            KeyCode::PageDown => {
                match self.summary_state.view_mode {
                    SummaryViewMode::Overview => {
                        // Move down by 10 files
                        let max_items = self.summary_state.current_summary
                            .as_ref()
                            .map(|s| s.files.len())
                            .unwrap_or(0);
                        for _ in 0..10 {
                            self.summary_state.move_down(max_items);
                        }
                    }
                    SummaryViewMode::FileDetail => {
                        // Scroll diff down by 10 lines
                        for _ in 0..10 {
                            self.summary_state.scroll_diff_down();
                        }
                    }
                }
                true
            }
            KeyCode::Home => {
                match self.summary_state.view_mode {
                    SummaryViewMode::Overview => {
                        self.summary_state.selected_file_index = 0;
                    }
                    SummaryViewMode::FileDetail => {
                        self.summary_state.diff_scroll = 0;
                    }
                }
                true
            }
            KeyCode::End => {
                match self.summary_state.view_mode {
                    SummaryViewMode::Overview => {
                        let max_items = self.summary_state.current_summary
                            .as_ref()
                            .map(|s| s.files.len().saturating_sub(1))
                            .unwrap_or(0);
                        self.summary_state.selected_file_index = max_items;
                    }
                    SummaryViewMode::FileDetail => {
                        // Set to a high value, the render function will handle bounds
                        self.summary_state.diff_scroll = 9999;
                    }
                }
                true
            }
            KeyCode::Char('r') => {
                // Force refresh summary
                self.summary_state.last_refresh = std::time::Instant::now();
                true
            }
            _ => false, // Key not handled by summary mode
        }
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