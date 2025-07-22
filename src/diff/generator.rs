use super::algorithms::{DiffAlgorithm, DiffAlgorithmType, DiffResult};

/// High-level diff generator that can use different algorithms
pub struct DiffGenerator {
    algorithm: Box<dyn DiffAlgorithm>,
}

impl DiffGenerator {
    /// Create a new diff generator with the specified algorithm
    pub fn new(algorithm_type: DiffAlgorithmType) -> Self {
        Self {
            algorithm: algorithm_type.create(),
        }
    }
    
    /// Create a diff generator with a custom algorithm
    pub fn with_algorithm(algorithm: Box<dyn DiffAlgorithm>) -> Self {
        Self { algorithm }
    }
    
    /// Generate a diff between old and new content
    pub fn generate(&self, old: &str, new: &str) -> DiffResult {
        self.algorithm.diff(old, new)
    }
    
    /// Get the current algorithm name
    pub fn algorithm_name(&self) -> &str {
        self.algorithm.name()
    }
    
    /// Get the current algorithm description
    pub fn algorithm_description(&self) -> &str {
        self.algorithm.description()
    }
}

impl Default for DiffGenerator {
    fn default() -> Self {
        Self::new(DiffAlgorithmType::default())
    }
}

/// Builder for configuring diff generation
pub struct DiffConfig {
    algorithm: DiffAlgorithmType,
    context_lines: usize,
}

impl DiffConfig {
    pub fn new() -> Self {
        Self {
            algorithm: DiffAlgorithmType::default(),
            context_lines: 3,
        }
    }
    
    pub fn algorithm(mut self, algorithm: DiffAlgorithmType) -> Self {
        self.algorithm = algorithm;
        self
    }
    
    pub fn context_lines(mut self, lines: usize) -> Self {
        self.context_lines = lines;
        self
    }
    
    pub fn build(self) -> DiffGenerator {
        DiffGenerator::new(self.algorithm)
    }
}

impl Default for DiffConfig {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_diff_generator() {
        let generator = DiffGenerator::new(DiffAlgorithmType::Myers);
        let result = generator.generate("a\nb\nc", "a\nx\nc");
        
        assert_eq!(generator.algorithm_name(), "Myers");
        assert_eq!(result.stats.lines_added, 1);
        assert_eq!(result.stats.lines_removed, 1);
    }
    
    #[test]
    fn test_diff_config_builder() {
        let generator = DiffConfig::new()
            .algorithm(DiffAlgorithmType::Patience)
            .context_lines(5)
            .build();
            
        assert_eq!(generator.algorithm_name(), "Patience");
    }
}