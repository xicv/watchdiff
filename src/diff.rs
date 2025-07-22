use std::path::Path;
use similar::{ChangeTag, TextDiff};
use crate::highlight::{SyntaxHighlighter, is_likely_text_file};

pub fn generate_diff<P: AsRef<Path>>(old: &str, new: &str, path: P) -> String {
    let path = path.as_ref();
    
    // Check if this file should have syntax highlighting
    if is_likely_text_file(&path) {
        generate_highlighted_diff(old, new, &path)
    } else {
        generate_basic_diff(old, new, &path)
    }
}

pub fn generate_highlighted_diff<P: AsRef<Path>>(old: &str, new: &str, path: P) -> String {
    let path = path.as_ref();
    let diff = TextDiff::from_lines(old, new);
    
    let highlighter = SyntaxHighlighter::new();
    let language = highlighter.get_language_from_path(&path);
    
    let mut result = Vec::new();
    result.push(format!("--- {}", path.display()));
    result.push(format!("+++ {}", path.display()));
    
    for (idx, group) in diff.grouped_ops(3).iter().enumerate() {
        if idx > 0 {
            result.push("...".to_string());
        }
        
        let hunk_header = format!(
            "@@ -{},{} +{},{} @@",
            group.first().unwrap().old_range().start + 1,
            group.first().unwrap().old_range().len(),
            group.first().unwrap().new_range().start + 1,
            group.first().unwrap().new_range().len()
        );
        result.push(hunk_header);
        
        for op in group {
            for change in diff.iter_changes(op) {
                let content = change.to_string_lossy();
                let content = content.trim_end(); // Remove trailing newline
                
                let (sign, base_color) = match change.tag() {
                    ChangeTag::Delete => ("-", "\x1b[31m"), // Red
                    ChangeTag::Insert => ("+", "\x1b[32m"), // Green
                    ChangeTag::Equal => (" ", ""),  // No color for unchanged
                };
                
                // Apply syntax highlighting if we have a language
                if let Some(ref lang) = language {
                    if change.tag() != ChangeTag::Equal {
                        // For added/deleted lines, combine syntax highlighting with diff colors
                        let highlighted = highlighter.get_terminal_highlighted(content, lang);
                        result.push(format!("{}{}{}\x1b[0m", sign, base_color, highlighted));
                    } else {
                        // For unchanged lines, use full syntax highlighting
                        let highlighted = highlighter.get_terminal_highlighted(content, lang);
                        result.push(format!("{}{}", sign, highlighted));
                    }
                } else {
                    // Fallback to basic coloring
                    result.push(format!("{}{}{}\x1b[0m", sign, base_color, content));
                }
            }
        }
    }
    
    result.join("\n")
}

pub fn generate_basic_diff<P: AsRef<Path>>(old: &str, new: &str, path: P) -> String {
    let path = path.as_ref();
    let diff = TextDiff::from_lines(old, new);
    
    let mut result = Vec::new();
    result.push(format!("--- {}", path.display()));
    result.push(format!("+++ {}", path.display()));
    
    for (idx, group) in diff.grouped_ops(3).iter().enumerate() {
        if idx > 0 {
            result.push("...".to_string());
        }
        
        let hunk_header = format!(
            "@@ -{},{} +{},{} @@",
            group.first().unwrap().old_range().start + 1,
            group.first().unwrap().old_range().len(),
            group.first().unwrap().new_range().start + 1,
            group.first().unwrap().new_range().len()
        );
        result.push(hunk_header);
        
        for op in group {
            for change in diff.iter_changes(op) {
                let (sign, style_prefix) = match change.tag() {
                    ChangeTag::Delete => ("-", "\x1b[31m"), // Red
                    ChangeTag::Insert => ("+", "\x1b[32m"), // Green
                    ChangeTag::Equal => (" ", "\x1b[37m"),  // White
                };
                
                let mut line = format!("{}{}", sign, style_prefix);
                line.push_str(&change.to_string_lossy());
                line.push_str("\x1b[0m"); // Reset color
                result.push(line);
            }
        }
    }
    
    result.join("\n")
}

pub fn generate_unified_diff<P: AsRef<Path>>(old: &str, new: &str, path: P) -> String {
    let path = path.as_ref();
    let diff = TextDiff::from_lines(old, new);
    
    format!(
        "--- {}\n+++ {}\n{}",
        path.display(),
        path.display(),
        diff.unified_diff().header("", "").to_string()
    )
}

pub fn generate_side_by_side_diff<P: AsRef<Path>>(old: &str, new: &str, path: P, width: usize) -> String {
    let path = path.as_ref();
    let diff = TextDiff::from_lines(old, new);
    
    let mut result = Vec::new();
    result.push(format!("File: {}", path.display()));
    result.push("─".repeat(width));
    
    let column_width = (width - 3) / 2; // Account for separator
    
    for change in diff.iter_all_changes() {
        let line = change.to_string_lossy();
        let line = line.trim_end(); // Remove trailing newline
        
        match change.tag() {
            ChangeTag::Equal => {
                let truncated = if line.len() > column_width {
                    format!("{}…", &line[..column_width-1])
                } else {
                    format!("{:<width$}", line, width = column_width)
                };
                result.push(format!("{} │ {}", truncated, truncated));
            }
            ChangeTag::Delete => {
                let truncated = if line.len() > column_width {
                    format!("{}…", &line[..column_width-1])
                } else {
                    format!("{:<width$}", line, width = column_width)
                };
                result.push(format!("\x1b[31m{}\x1b[0m │ {:<width$}", truncated, "", width = column_width));
            }
            ChangeTag::Insert => {
                let truncated = if line.len() > column_width {
                    format!("{}…", &line[..column_width-1])
                } else {
                    format!("{:<width$}", line, width = column_width)
                };
                result.push(format!("{:<width$} │ \x1b[32m{}\x1b[0m", "", truncated, width = column_width));
            }
        }
    }
    
    result.join("\n")
}

pub fn get_diff_stats(old: &str, new: &str) -> DiffStats {
    let diff = TextDiff::from_lines(old, new);
    
    let mut stats = DiffStats::default();
    
    for change in diff.iter_all_changes() {
        match change.tag() {
            ChangeTag::Equal => stats.unchanged += 1,
            ChangeTag::Delete => stats.removed += 1,
            ChangeTag::Insert => stats.added += 1,
        }
    }
    
    stats
}

#[derive(Debug, Default, Clone)]
pub struct DiffStats {
    pub added: usize,
    pub removed: usize,
    pub unchanged: usize,
}

impl DiffStats {
    pub fn total_changes(&self) -> usize {
        self.added + self.removed
    }
    
    pub fn total_lines(&self) -> usize {
        self.added + self.removed + self.unchanged
    }
    
    pub fn change_percentage(&self) -> f64 {
        if self.total_lines() == 0 {
            0.0
        } else {
            (self.total_changes() as f64 / self.total_lines() as f64) * 100.0
        }
    }
}