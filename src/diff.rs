use std::path::Path;
use similar::{ChangeTag, TextDiff};
use crate::highlight::{SyntaxHighlighter, is_likely_text_file};

pub fn generate_diff<P: AsRef<Path>>(old: &str, new: &str, path: P) -> String {
    let path = path.as_ref();
    
    // Check if this file should have syntax highlighting
    if is_likely_text_file(path) {
        generate_highlighted_diff(old, new, path)
    } else {
        generate_basic_diff(old, new, path)
    }
}

pub fn generate_highlighted_diff<P: AsRef<Path>>(old: &str, new: &str, path: P) -> String {
    let path = path.as_ref();
    let diff = TextDiff::from_lines(old, new);
    
    let highlighter = SyntaxHighlighter::new();
    let language = highlighter.get_language_from_path(path);
    
    let mut result = Vec::new();
    result.push(format!("--- {}", path.display()));
    result.push(format!("+++ {}", path.display()));
    
    for (idx, group) in diff.grouped_ops(3).iter().enumerate() {
        if idx > 0 {
            result.push("...".to_string());
        }
        
        let mut old_line = group[0].old_range().start;
        let mut new_line = group[0].new_range().start;
        
        result.push(format!(
            "@@ -{},{} +{},{} @@",
            old_line + 1,
            group.iter().map(|op| op.old_range().len()).sum::<usize>(),
            new_line + 1,
            group.iter().map(|op| op.new_range().len()).sum::<usize>()
        ));
        
        for op in group {
            for change in diff.iter_changes(op) {
                let (sign, content) = match change.tag() {
                    ChangeTag::Delete => {
                        old_line += 1;
                        ("-", change.value())
                    },
                    ChangeTag::Insert => {
                        new_line += 1;
                        ("+", change.value())
                    },
                    ChangeTag::Equal => {
                        old_line += 1;
                        new_line += 1;
                        (" ", change.value())
                    }
                };
                
                // Try to highlight the content if we have a language
                let highlighted_content = if let Some(lang) = &language {
                    let highlighted_spans = highlighter.highlight_line(content, lang, old_line);
                    // Convert spans to plain text for now - could be enhanced later
                    highlighted_spans.into_iter().map(|(_, text)| text).collect::<String>()
                } else {
                    content.to_string()
                };
                
                result.push(format!("{}{}", sign, highlighted_content.trim_end()));
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
        
        let old_start = group[0].old_range().start;
        let new_start = group[0].new_range().start;
        let old_len = group.iter().map(|op| op.old_range().len()).sum::<usize>();
        let new_len = group.iter().map(|op| op.new_range().len()).sum::<usize>();
        
        result.push(format!(
            "@@ -{},{} +{},{} @@",
            old_start + 1, old_len,
            new_start + 1, new_len
        ));
        
        for op in group {
            for change in diff.iter_changes(op) {
                let (sign, content) = match change.tag() {
                    ChangeTag::Delete => ("-", change.value()),
                    ChangeTag::Insert => ("+", change.value()),
                    ChangeTag::Equal => (" ", change.value())
                };
                
                result.push(format!("{}{}", sign, content.trim_end()));
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
        diff.unified_diff().header("", "")
    )
}

pub fn generate_side_by_side_diff<P: AsRef<Path>>(old: &str, new: &str, path: P, width: usize) -> String {
    let path = path.as_ref();
    let diff = TextDiff::from_lines(old, new);
    
    let mut result = Vec::new();
    result.push(format!("--- {} | +++ {}", path.display(), path.display()));
    result.push("-".repeat(width));
    
    let half_width = (width - 3) / 2; // Account for separator " | "
    
    for change in diff.iter_all_changes() {
        let content = change.value().trim_end();
        match change.tag() {
            ChangeTag::Delete => {
                let left = format!("- {}", content);
                let truncated_left = if left.len() > half_width {
                    format!("{}...", &left[..half_width-3])
                } else {
                    format!("{:width$}", left, width = half_width)
                };
                result.push(format!("{} | {}", truncated_left, " ".repeat(half_width)));
            },
            ChangeTag::Insert => {
                let right = format!("+ {}", content);
                let truncated_right = if right.len() > half_width {
                    format!("{}...", &right[..half_width-3])
                } else {
                    right
                };
                result.push(format!("{} | {}", " ".repeat(half_width), truncated_right));
            },
            ChangeTag::Equal => {
                let left = format!("  {}", content);
                let right = format!("  {}", content);
                
                let truncated_left = if left.len() > half_width {
                    format!("{}...", &left[..half_width-3])
                } else {
                    format!("{:width$}", left, width = half_width)
                };
                
                let truncated_right = if right.len() > half_width {
                    format!("{}...", &right[..half_width-3])
                } else {
                    right
                };
                
                result.push(format!("{} | {}", truncated_left, truncated_right));
            }
        }
    }
    
    result.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_basic_diff() {
        let old = "line1\nline2\nline3";
        let new = "line1\nmodified line2\nline3";
        let path = "test.txt";
        
        let diff = generate_basic_diff(old, new, path);
        
        assert!(diff.contains("--- test.txt"));
        assert!(diff.contains("+++ test.txt"));
        assert!(diff.contains("-line2"));
        assert!(diff.contains("+modified line2"));
    }

    #[test]
    fn test_generate_unified_diff() {
        let old = "hello\nworld";
        let new = "hello\nrust\nworld";
        let path = "example.rs";
        
        let diff = generate_unified_diff(old, new, path);
        
        assert!(diff.contains("--- example.rs"));
        assert!(diff.contains("+++ example.rs"));
        assert!(diff.contains("+rust"));
    }

    #[test]
    fn test_generate_side_by_side_diff() {
        let old = "old line";
        let new = "new line";
        let path = "test.txt";
        let width = 80;
        
        let diff = generate_side_by_side_diff(old, new, path, width);
        
        assert!(diff.contains("--- test.txt | +++ test.txt"));
        assert!(diff.contains("- old line"));
        assert!(diff.contains("+ new line"));
    }

    #[test]
    fn test_empty_diff() {
        let content = "same content\nno changes";
        let diff = generate_basic_diff(content, content, "test.txt");
        
        // Empty diff should still have headers
        assert!(diff.contains("--- test.txt"));
        assert!(diff.contains("+++ test.txt"));
        // But no change markers at the beginning of lines (except in headers)
        let lines: Vec<&str> = diff.lines().skip(2).collect(); // Skip header lines
        for line in lines {
            assert!(!line.starts_with('+'), "Found unexpected + line: {}", line);
            assert!(!line.starts_with('-'), "Found unexpected - line: {}", line);
        }
    }
}