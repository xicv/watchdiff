use similar::{TextDiff, ChangeTag, Algorithm};
use clap::ValueEnum;

/// Trait defining a diff algorithm interface
pub trait DiffAlgorithm: Send + Sync {
    /// Generate a diff between old and new content
    fn diff(&self, old: &str, new: &str) -> DiffResult;
    
    /// Get the algorithm name
    fn name(&self) -> &'static str;
    
    /// Get algorithm description
    fn description(&self) -> &'static str;
}

/// Result of a diff operation
#[derive(Debug, Clone)]
pub struct DiffResult {
    pub hunks: Vec<DiffHunk>,
    pub stats: DiffStats,
}

/// A single hunk (contiguous block of changes)
#[derive(Debug, Clone)]
pub struct DiffHunk {
    pub old_start: usize,
    pub old_len: usize,
    pub new_start: usize,
    pub new_len: usize,
    pub operations: Vec<DiffOperation>,
}

/// Individual diff operation
#[derive(Debug, Clone)]
pub enum DiffOperation {
    Equal(String),
    Insert(String),
    Delete(String),
}

/// Statistics about the diff
#[derive(Debug, Clone, Default)]
pub struct DiffStats {
    pub lines_added: usize,
    pub lines_removed: usize,
    pub lines_modified: usize,
    pub hunks: usize,
}

impl DiffStats {
    pub fn total_changes(&self) -> usize {
        self.lines_added + self.lines_removed
    }
    
    pub fn net_change(&self) -> isize {
        self.lines_added as isize - self.lines_removed as isize
    }
}

/// Myers diff algorithm implementation
pub struct MyersAlgorithm;

impl DiffAlgorithm for MyersAlgorithm {
    fn diff(&self, old: &str, new: &str) -> DiffResult {
        let diff = TextDiff::configure()
            .algorithm(Algorithm::Myers)
            .diff_lines(old, new);
        
        self.convert_to_result(&diff)
    }
    
    fn name(&self) -> &'static str {
        "Myers"
    }
    
    fn description(&self) -> &'static str {
        "Myers' O(ND) diff algorithm - fast and widely used"
    }
}

/// Patience diff algorithm implementation  
pub struct PatienceAlgorithm;

impl DiffAlgorithm for PatienceAlgorithm {
    fn diff(&self, old: &str, new: &str) -> DiffResult {
        let diff = TextDiff::configure()
            .algorithm(Algorithm::Patience)
            .diff_lines(old, new);
            
        self.convert_to_result(&diff)
    }
    
    fn name(&self) -> &'static str {
        "Patience"
    }
    
    fn description(&self) -> &'static str {
        "Patience diff - better for refactored code with moved blocks"
    }
}

/// LCS (Longest Common Subsequence) diff algorithm
pub struct LcsAlgorithm;

impl DiffAlgorithm for LcsAlgorithm {
    fn diff(&self, old: &str, new: &str) -> DiffResult {
        let diff = TextDiff::configure()
            .algorithm(Algorithm::Lcs)
            .diff_lines(old, new);
            
        self.convert_to_result(&diff)
    }
    
    fn name(&self) -> &'static str {
        "LCS"  
    }
    
    fn description(&self) -> &'static str {
        "Longest Common Subsequence - produces minimal diffs"
    }
}

// Shared implementation for converting similar::TextDiff to our DiffResult
trait DiffConverter {
    fn convert_to_result(&self, diff: &TextDiff<str>) -> DiffResult {
        let mut hunks = Vec::new();
        let mut stats = DiffStats::default();
        
        for (_idx, group) in diff.grouped_ops(3).iter().enumerate() {
            let mut operations = Vec::new();
            
            let old_start = group[0].old_range().start;
            let new_start = group[0].new_range().start;
            let old_len = group.iter().map(|op| op.old_range().len()).sum();
            let new_len = group.iter().map(|op| op.new_range().len()).sum();
            
            for op in group {
                for change in diff.iter_changes(op) {
                    let content = change.value().to_string();
                    
                    match change.tag() {
                        ChangeTag::Equal => {
                            operations.push(DiffOperation::Equal(content));
                        }
                        ChangeTag::Insert => {
                            operations.push(DiffOperation::Insert(content));
                            stats.lines_added += 1;
                        }
                        ChangeTag::Delete => {
                            operations.push(DiffOperation::Delete(content));
                            stats.lines_removed += 1;
                        }
                    }
                }
            }
            
            hunks.push(DiffHunk {
                old_start,
                old_len, 
                new_start,
                new_len,
                operations,
            });
        }
        
        stats.hunks = hunks.len();
        stats.lines_modified = stats.lines_added.min(stats.lines_removed);
        
        DiffResult { hunks, stats }
    }
}

impl DiffConverter for MyersAlgorithm {}
impl DiffConverter for PatienceAlgorithm {}
impl DiffConverter for LcsAlgorithm {}

/// Available diff algorithms
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum DiffAlgorithmType {
    Myers,
    Patience, 
    Lcs,
}

impl DiffAlgorithmType {
    pub fn all() -> &'static [DiffAlgorithmType] {
        &[Self::Myers, Self::Patience, Self::Lcs]
    }
    
    pub fn create(&self) -> Box<dyn DiffAlgorithm> {
        match self {
            Self::Myers => Box::new(MyersAlgorithm),
            Self::Patience => Box::new(PatienceAlgorithm),
            Self::Lcs => Box::new(LcsAlgorithm),
        }
    }
    
    pub fn name(&self) -> &'static str {
        match self {
            Self::Myers => "Myers",
            Self::Patience => "Patience",
            Self::Lcs => "LCS",
        }
    }
}

impl std::fmt::Display for DiffAlgorithmType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.name())
    }
}

impl Default for DiffAlgorithmType {
    fn default() -> Self {
        Self::Myers
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_myers_diff() {
        let myers = MyersAlgorithm;
        let old = "line1\nline2\nline3";
        let new = "line1\nmodified\nline3";
        
        let result = myers.diff(old, new);
        
        assert_eq!(result.stats.lines_added, 1);
        assert_eq!(result.stats.lines_removed, 1);
        assert!(!result.hunks.is_empty());
    }
    
    #[test]
    fn test_patience_diff() {
        let patience = PatienceAlgorithm;
        let old = "a\nb\nc\nd";
        let new = "a\nc\nb\nd";
        
        let result = patience.diff(old, new);
        assert!(!result.hunks.is_empty());
    }
    
    #[test]
    fn test_diff_stats() {
        let stats = DiffStats {
            lines_added: 5,
            lines_removed: 3,
            lines_modified: 0,
            hunks: 2,
        };
        
        assert_eq!(stats.total_changes(), 8);
        assert_eq!(stats.net_change(), 2);
    }
}