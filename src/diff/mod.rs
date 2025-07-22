//! Diff generation and formatting module
//! 
//! This module provides a trait-based architecture for generating and formatting
//! diffs using different algorithms. It supports multiple diff algorithms and
//! output formats.

pub mod algorithms;
pub mod generator;
pub mod formatter;

// Re-export the main types for easier use
pub use algorithms::{
    DiffAlgorithm, DiffAlgorithmType, DiffResult, DiffHunk, DiffOperation, DiffStats,
    MyersAlgorithm, PatienceAlgorithm, LcsAlgorithm,
};

pub use generator::{DiffGenerator, DiffConfig};
pub use formatter::{DiffFormatter, DiffFormat};

/// Convenience function to generate a unified diff with default settings
pub fn generate_unified_diff<P: AsRef<std::path::Path>>(
    old: &str,
    new: &str, 
    old_path: P,
    new_path: P,
) -> String {
    let generator = DiffGenerator::default();
    let result = generator.generate(old, new);
    DiffFormatter::format_unified(&result, old_path, new_path)
}

/// Convenience function to generate a side-by-side diff with default settings
pub fn generate_side_by_side_diff<P: AsRef<std::path::Path>>(
    old: &str,
    new: &str,
    old_path: P, 
    new_path: P,
    width: usize,
) -> String {
    let generator = DiffGenerator::default();
    let result = generator.generate(old, new);
    DiffFormatter::format_side_by_side(&result, old_path, new_path, width)
}

/// Convenience function to get diff statistics
pub fn get_diff_stats(old: &str, new: &str) -> DiffStats {
    let generator = DiffGenerator::default();
    let result = generator.generate(old, new);
    result.stats
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_convenience_functions() {
        let old = "line1\nline2\nline3";
        let new = "line1\nmodified\nline3";
        
        let unified = generate_unified_diff(old, new, "old.txt", "new.txt");
        assert!(unified.contains("--- old.txt"));
        assert!(unified.contains("+modified"));
        
        let stats = get_diff_stats(old, new);
        assert_eq!(stats.lines_added, 1);
        assert_eq!(stats.lines_removed, 1);
    }
}