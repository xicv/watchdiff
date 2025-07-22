use std::path::Path;
use super::algorithms::{DiffResult, DiffOperation};

/// Different output formats for diffs
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiffFormat {
    Unified,
    SideBySide,
    Context,
    GitPatch,
}

/// Formats diff results into various text representations
pub struct DiffFormatter;

impl DiffFormatter {
    /// Format a diff result as unified diff
    pub fn format_unified<P: AsRef<Path>>(result: &DiffResult, old_path: P, new_path: P) -> String {
        let old_path = old_path.as_ref();
        let new_path = new_path.as_ref();
        
        let mut output = Vec::new();
        output.push(format!("--- {}", old_path.display()));
        output.push(format!("+++ {}", new_path.display()));
        
        for hunk in &result.hunks {
            // Add hunk header
            output.push(format!(
                "@@ -{},{} +{},{} @@",
                hunk.old_start + 1,
                hunk.old_len,
                hunk.new_start + 1,
                hunk.new_len
            ));
            
            // Add operations
            for op in &hunk.operations {
                match op {
                    DiffOperation::Equal(line) => {
                        output.push(format!(" {}", line.trim_end()));
                    }
                    DiffOperation::Insert(line) => {
                        output.push(format!("+{}", line.trim_end()));
                    }
                    DiffOperation::Delete(line) => {
                        output.push(format!("-{}", line.trim_end()));
                    }
                }
            }
        }
        
        output.join("\n")
    }
    
    /// Format a diff result as side-by-side comparison
    pub fn format_side_by_side<P: AsRef<Path>>(
        result: &DiffResult, 
        old_path: P, 
        new_path: P, 
        width: usize
    ) -> String {
        let old_path = old_path.as_ref();
        let new_path = new_path.as_ref();
        
        let mut output = Vec::new();
        let half_width = (width - 3) / 2; // Account for separator " | "
        
        output.push(format!(
            "{:<width$} | {}", 
            format!("--- {}", old_path.display()), 
            format!("+++ {}", new_path.display()),
            width = half_width
        ));
        output.push("-".repeat(width));
        
        for hunk in &result.hunks {
            for op in &hunk.operations {
                match op {
                    DiffOperation::Equal(line) => {
                        let content = format!("  {}", line.trim_end());
                        let truncated = Self::truncate_line(&content, half_width);
                        output.push(format!("{:<width$} | {}", truncated, truncated, width = half_width));
                    }
                    DiffOperation::Delete(line) => {
                        let content = format!("- {}", line.trim_end());
                        let truncated = Self::truncate_line(&content, half_width);
                        output.push(format!("{:<width$} | {}", truncated, " ".repeat(half_width), width = half_width));
                    }
                    DiffOperation::Insert(line) => {
                        let content = format!("+ {}", line.trim_end());
                        let truncated = Self::truncate_line(&content, half_width);
                        output.push(format!("{:<width$} | {}", " ".repeat(half_width), truncated, width = half_width));
                    }
                }
            }
        }
        
        output.join("\n")
    }
    
    /// Format as Git patch format
    pub fn format_git_patch<P: AsRef<Path>>(result: &DiffResult, old_path: P, new_path: P) -> String {
        let old_path = old_path.as_ref();
        let new_path = new_path.as_ref();
        
        let mut output = Vec::new();
        
        // Git patch header
        output.push(format!("diff --git a/{} b/{}", old_path.display(), new_path.display()));
        output.push(format!("index 0000000..1111111 100644")); // Placeholder hashes
        
        // Standard unified diff content
        output.push(Self::format_unified(result, old_path, new_path));
        
        output.join("\n")
    }
    
    /// Format diff statistics as a summary
    pub fn format_stats(result: &DiffResult) -> String {
        let stats = &result.stats;
        
        if stats.total_changes() == 0 {
            return "No changes".to_string();
        }
        
        let mut parts = Vec::new();
        
        if stats.lines_added > 0 {
            parts.push(format!("{} insertion{}", 
                stats.lines_added,
                if stats.lines_added == 1 { "" } else { "s" }
            ));
        }
        
        if stats.lines_removed > 0 {
            parts.push(format!("{} deletion{}", 
                stats.lines_removed,
                if stats.lines_removed == 1 { "" } else { "s" }
            ));
        }
        
        if stats.hunks > 0 {
            parts.push(format!("{} hunk{}", 
                stats.hunks,
                if stats.hunks == 1 { "" } else { "s" }
            ));
        }
        
        parts.join(", ")
    }
    
    /// Format with the specified format type
    pub fn format<P: AsRef<Path>>(
        result: &DiffResult,
        format: DiffFormat, 
        old_path: P,
        new_path: P,
        width: Option<usize>
    ) -> String {
        match format {
            DiffFormat::Unified => Self::format_unified(result, old_path, new_path),
            DiffFormat::SideBySide => {
                let w = width.unwrap_or(80);
                Self::format_side_by_side(result, old_path, new_path, w)
            }
            DiffFormat::GitPatch => Self::format_git_patch(result, old_path, new_path),
            DiffFormat::Context => Self::format_unified(result, old_path, new_path), // Same as unified for now
        }
    }
    
    fn truncate_line(line: &str, max_width: usize) -> String {
        if line.len() > max_width {
            if max_width > 3 {
                format!("{}...", &line[..max_width - 3])
            } else {
                line[..max_width].to_string()
            }
        } else {
            line.to_string()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diff::algorithms::{MyersAlgorithm, DiffAlgorithm};

    fn create_test_diff() -> DiffResult {
        let myers = MyersAlgorithm;
        myers.diff("line1\nline2\nline3", "line1\nmodified\nline3")
    }

    #[test]
    fn test_format_unified() {
        let result = create_test_diff();
        let formatted = DiffFormatter::format_unified(&result, "old.txt", "new.txt");
        
        assert!(formatted.contains("--- old.txt"));
        assert!(formatted.contains("+++ new.txt"));
        assert!(formatted.contains("-line2"));
        assert!(formatted.contains("+modified"));
    }

    #[test]
    fn test_format_stats() {
        let result = create_test_diff();
        let stats = DiffFormatter::format_stats(&result);
        
        assert!(stats.contains("1 insertion"));
        assert!(stats.contains("1 deletion"));
    }

    #[test]
    fn test_format_git_patch() {
        let result = create_test_diff();
        let formatted = DiffFormatter::format_git_patch(&result, "file.txt", "file.txt");
        
        assert!(formatted.contains("diff --git"));
        assert!(formatted.contains("index 0000000..1111111"));
    }
}