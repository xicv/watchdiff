use crate::core::events::{ChangeOrigin, ChangeConfidence, ConfidenceLevel};
use crate::config::AiConfig;
use std::collections::HashMap;
use std::path::Path;
use std::process::Command;

pub struct AIDetector {
    known_ai_tools: HashMap<String, String>,
    active_processes: HashMap<u32, String>,
    batch_detector: BatchChangeDetector,
}

pub struct BatchChangeDetector {
    recent_changes: Vec<ChangeEvent>,
    current_batch_id: Option<String>,
    last_batch_time: std::time::Instant,
    config: AiConfig,
}

#[derive(Clone)]
struct ChangeEvent {
    timestamp: std::time::Instant,
    origin: ChangeOrigin,
}

impl Default for AIDetector {
    fn default() -> Self {
        let mut known_ai_tools = HashMap::new();
        known_ai_tools.insert("claude".to_string(), "Claude Code".to_string());
        known_ai_tools.insert("gemini".to_string(), "Gemini CLI".to_string());
        known_ai_tools.insert("cursor".to_string(), "Cursor".to_string());
        known_ai_tools.insert("copilot".to_string(), "GitHub Copilot".to_string());
        known_ai_tools.insert("codeium".to_string(), "Codeium".to_string());
        known_ai_tools.insert("tabnine".to_string(), "TabNine".to_string());

        Self {
            known_ai_tools,
            active_processes: HashMap::new(),
            batch_detector: BatchChangeDetector::with_config(AiConfig::default()),
        }
    }
}

impl AIDetector {
    pub fn new() -> Self {
        Self::default()
    }
    
    pub fn with_config(config: AiConfig) -> Self {
        let mut detector = Self::default();
        detector.batch_detector = BatchChangeDetector::with_config(config);
        detector
    }

    pub fn detect_change_origin(&mut self) -> ChangeOrigin {
        self.scan_active_processes();

        if let Some((pid, tool_name)) = self.find_active_ai_tool() {
            ChangeOrigin::AIAgent {
                tool_name: tool_name.clone(),
                process_id: Some(pid),
            }
        } else {
            ChangeOrigin::Unknown
        }
    }

    pub fn detect_batch_change(&mut self, path: &std::path::Path, origin: &ChangeOrigin) -> Option<String> {
        self.batch_detector.process_change(path, origin)
    }

    fn scan_active_processes(&mut self) {
        self.active_processes.clear();

        // Only scan processes in non-test environments
        #[cfg(not(test))]
        {
            if cfg!(target_os = "macos") || cfg!(target_os = "linux") {
                if let Ok(output) = Command::new("ps")
                    .args(&["-eo", "pid,comm"])
                    .output()
                {
                    if let Ok(ps_output) = String::from_utf8(output.stdout) {
                        for line in ps_output.lines().skip(1) {
                            if let Some((pid_str, comm)) = line.trim().split_once(' ') {
                                if let Ok(pid) = pid_str.parse::<u32>() {
                                    let comm = comm.trim().to_lowercase();
                                    for (tool_key, tool_name) in &self.known_ai_tools {
                                        if comm.contains(tool_key) {
                                            self.active_processes.insert(pid, tool_name.clone());
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    fn find_active_ai_tool(&self) -> Option<(u32, &String)> {
        self.active_processes
            .iter()
            .next()
            .map(|(pid, name)| (*pid, name))
    }
}

pub struct ConfidenceScorer {
    pattern_rules: Vec<PatternRule>,
}

struct PatternRule {
    pattern: String,
    confidence_impact: f32,
    reason: String,
}

impl Default for ConfidenceScorer {
    fn default() -> Self {
        let pattern_rules = vec![
            PatternRule {
                pattern: r"import.*unused".to_string(),
                confidence_impact: -0.3,
                reason: "Unused import detected".to_string(),
            },
            PatternRule {
                pattern: r"TODO|FIXME|XXX".to_string(),
                confidence_impact: -0.2,
                reason: "TODO/FIXME comment found".to_string(),
            },
            PatternRule {
                pattern: r"console\.log|print\(|println!".to_string(),
                confidence_impact: -0.1,
                reason: "Debug output detected".to_string(),
            },
            PatternRule {
                pattern: r"\.unwrap\(\)".to_string(),
                confidence_impact: -0.2,
                reason: "Unsafe unwrap() usage".to_string(),
            },
            PatternRule {
                pattern: r"unsafe\s*\{".to_string(),
                confidence_impact: -0.4,
                reason: "Unsafe code block".to_string(),
            },
            PatternRule {
                pattern: r"#\[allow\(.*\)\]".to_string(),
                confidence_impact: -0.1,
                reason: "Lint warning suppression".to_string(),
            },
        ];

        Self { pattern_rules }
    }
}

impl ConfidenceScorer {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn score_change(&self, diff: &str, file_path: &Path) -> ChangeConfidence {
        let mut base_score = 0.8; // Start with high confidence
        let mut reasons = Vec::new();

        // Check for problematic patterns in diff
        for rule in &self.pattern_rules {
            if let Ok(regex) = regex::Regex::new(&rule.pattern) {
                if regex.is_match(diff) {
                    base_score += rule.confidence_impact;
                    reasons.push(rule.reason.clone());
                }
            }
        }

        // File type specific scoring
        if let Some(extension) = file_path.extension().and_then(|e| e.to_str()) {
            match extension {
                "rs" | "py" | "js" | "ts" => {
                    // These languages have good AI support
                    base_score += 0.1;
                }
                "c" | "cpp" | "asm" => {
                    // Lower-level languages are riskier for AI
                    base_score -= 0.2;
                    reasons.push("Low-level language detected".to_string());
                }
                _ => {}
            }
        }

        // Large change penalty
        let line_count = diff.lines().count();
        if line_count > 100 {
            base_score -= 0.2;
            reasons.push("Large change detected".to_string());
        } else if line_count > 50 {
            base_score -= 0.1;
            reasons.push("Medium-sized change".to_string());
        }

        // Clamp score between 0.0 and 1.0
        base_score = base_score.max(0.0).min(1.0);

        let level = if base_score >= 0.7 {
            ConfidenceLevel::Safe
        } else if base_score >= 0.4 {
            ConfidenceLevel::Review
        } else {
            ConfidenceLevel::Risky
        };

        ChangeConfidence {
            level,
            score: base_score,
            reasons,
        }
    }
}

impl BatchChangeDetector {
    pub fn new() -> Self {
        Self::with_config(AiConfig::default())
    }
    
    pub fn with_config(config: AiConfig) -> Self {
        Self {
            recent_changes: Vec::new(),
            current_batch_id: None,
            last_batch_time: std::time::Instant::now(),
            config,
        }
    }

    pub fn process_change(&mut self, _path: &std::path::Path, origin: &ChangeOrigin) -> Option<String> {
        let now = std::time::Instant::now();
        
        // Clean up old changes using configured max age
        self.recent_changes.retain(|change| {
            now.duration_since(change.timestamp) < self.config.batch_max_age_duration()
        });

        // Create change event
        let change_event = ChangeEvent {
            timestamp: now,
            origin: origin.clone(),
        };

        // Check if this should start a new batch or continue existing one
        let should_start_new_batch = self.should_start_new_batch(&change_event);
        
        if should_start_new_batch {
            // Generate new batch ID
            use std::time::{SystemTime, UNIX_EPOCH};
            let epoch_time = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default();
            let batch_id = format!("batch_{}", epoch_time.as_millis());
            self.current_batch_id = Some(batch_id.clone());
            self.last_batch_time = now;
            
            // Clear old changes and start fresh
            self.recent_changes.clear();
            self.recent_changes.push(change_event);
            
            return Some(batch_id);
        } else if self.is_part_of_current_batch(&change_event) {
            // Add to existing batch
            self.recent_changes.push(change_event);
            return self.current_batch_id.clone();
        }

        // Add change but no batch
        self.recent_changes.push(change_event);
        None
    }

    fn should_start_new_batch(&self, change: &ChangeEvent) -> bool {
        // Start new batch if:
        // 1. No current batch
        // 2. Time gap > 5 seconds since last batch activity
        // 3. AI agent is detected (likely start of AI session)
        
        if self.current_batch_id.is_none() {
            return matches!(change.origin, ChangeOrigin::AIAgent { .. });
        }

        let time_since_last_batch = change.timestamp.duration_since(self.last_batch_time);
        
        // New batch if gap is too large
        if time_since_last_batch > self.config.batch_time_gap_duration() {
            return matches!(change.origin, ChangeOrigin::AIAgent { .. });
        }

        false
    }

    fn is_part_of_current_batch(&self, change: &ChangeEvent) -> bool {
        if self.current_batch_id.is_none() {
            return false;
        }

        // Check if this change is related to recent changes in the batch
        let time_threshold = self.config.batch_time_gap_duration();
        
        // Must be within time threshold
        let time_since_last = change.timestamp.duration_since(self.last_batch_time);
        if time_since_last > time_threshold {
            return false;
        }

        // Check if from same origin type (AI agent changes group together)
        match (&change.origin, &self.recent_changes.last().map(|c| &c.origin)) {
            (ChangeOrigin::AIAgent { .. }, Some(ChangeOrigin::AIAgent { .. })) => true,
            (ChangeOrigin::Human, Some(ChangeOrigin::Human)) => false, // Human changes don't batch
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::time::{Duration, Instant};

    #[test]
    fn test_ai_detector_creation() {
        let detector = AIDetector::new();
        
        // Should have known AI tools registered
        assert!(detector.known_ai_tools.contains_key("claude"));
        assert!(detector.known_ai_tools.contains_key("gemini"));
        assert!(detector.known_ai_tools.contains_key("cursor"));
        assert!(detector.known_ai_tools.contains_key("copilot"));
    }

    #[test]
    fn test_ai_detector_unknown_origin_when_no_ai_tools() {
        let mut detector = AIDetector::new();
        
        // Without any AI processes running, should return Unknown
        let origin = detector.detect_change_origin();
        assert!(matches!(origin, ChangeOrigin::Unknown));
    }

    #[test]
    fn test_confidence_scorer_creation() {
        let scorer = ConfidenceScorer::new();
        
        // Should have pattern rules configured
        assert!(!scorer.pattern_rules.is_empty());
    }

    #[test]
    fn test_confidence_scorer_safe_code() {
        let scorer = ConfidenceScorer::new();
        let diff = "+fn hello_world() {\n+    println!(\"Hello, world!\");\n+}";
        let path = PathBuf::from("src/main.rs");
        
        let confidence = scorer.score_change(diff, &path);
        
        // Simple clean code should be relatively safe
        assert!(confidence.score > 0.5);
        assert!(matches!(confidence.level, ConfidenceLevel::Safe | ConfidenceLevel::Review));
    }

    #[test]
    fn test_confidence_scorer_risky_patterns() {
        let scorer = ConfidenceScorer::new();
        
        // Test unsafe code detection
        let unsafe_diff = "+unsafe {\n+    *ptr = 42;\n+}";
        let path = PathBuf::from("src/lib.rs");
        let confidence = scorer.score_change(unsafe_diff, &path);
        
        assert!(confidence.score < 0.7); // Should be lower confidence
        assert!(confidence.reasons.iter().any(|r| r.contains("Unsafe code")));
        
        // Test unwrap detection
        let unwrap_diff = "+let result = some_function().unwrap();";
        let confidence = scorer.score_change(unwrap_diff, &path);
        
        assert!(confidence.score < 0.8);
        assert!(confidence.reasons.iter().any(|r| r.contains("unwrap")));
        
        // Test debug output detection
        let debug_diff = "+println!(\"Debug: {:?}\", value);";
        let confidence = scorer.score_change(debug_diff, &path);
        
        assert!(confidence.reasons.iter().any(|r| r.contains("Debug output")));
    }

    #[test]
    fn test_confidence_scorer_file_type_bonus() {
        let scorer = ConfidenceScorer::new();
        let simple_diff = "+let x = 42;";
        
        // Rust file should get a bonus
        let rust_path = PathBuf::from("src/main.rs");
        let rust_confidence = scorer.score_change(simple_diff, &rust_path);
        
        // C file should get a penalty
        let c_path = PathBuf::from("src/main.c");
        let c_confidence = scorer.score_change(simple_diff, &c_path);
        
        assert!(rust_confidence.score > c_confidence.score);
        assert!(c_confidence.reasons.iter().any(|r| r.contains("Low-level language")));
    }

    #[test]
    fn test_confidence_scorer_large_change_penalty() {
        let scorer = ConfidenceScorer::new();
        let path = PathBuf::from("src/main.rs");
        
        // Small change
        let small_diff = "+let x = 42;";
        let small_confidence = scorer.score_change(small_diff, &path);
        
        // Large change (over 100 lines)
        let large_diff = (0..101).map(|i| format!("+line {}", i)).collect::<Vec<_>>().join("\n");
        let large_confidence = scorer.score_change(&large_diff, &path);
        
        assert!(small_confidence.score > large_confidence.score);
        assert!(large_confidence.reasons.iter().any(|r| r.contains("Large change")));
    }

    #[test]
    fn test_confidence_level_thresholds() {
        let scorer = ConfidenceScorer::new();
        let path = PathBuf::from("test.rs");
        
        // Test that confidence levels are assigned correctly based on score
        // We can't easily control the exact score, but we can test the logic
        let confidence = scorer.score_change("+fn safe_function() {}", &path);
        
        match confidence.level {
            ConfidenceLevel::Safe => assert!(confidence.score >= 0.7),
            ConfidenceLevel::Review => assert!(confidence.score >= 0.4 && confidence.score < 0.7),
            ConfidenceLevel::Risky => assert!(confidence.score < 0.4),
        }
    }

    #[test]
    fn test_batch_change_detector_creation() {
        let detector = BatchChangeDetector::new();
        
        assert!(detector.recent_changes.is_empty());
        assert!(detector.current_batch_id.is_none());
    }

    #[test]
    fn test_batch_change_detector_ai_agent_starts_batch() {
        let mut detector = BatchChangeDetector::new();
        let path = PathBuf::from("test.rs");
        let ai_origin = ChangeOrigin::AIAgent {
            tool_name: "Claude Code".to_string(),
            process_id: Some(123),
        };
        
        let batch_id = detector.process_change(&path, &ai_origin);
        
        assert!(batch_id.is_some());
        assert!(detector.current_batch_id.is_some());
        assert_eq!(detector.recent_changes.len(), 1);
    }

    #[test]
    fn test_batch_change_detector_human_changes_dont_batch() {
        let mut detector = BatchChangeDetector::new();
        let path = PathBuf::from("test.rs");
        let human_origin = ChangeOrigin::Human;
        
        let batch_id = detector.process_change(&path, &human_origin);
        
        assert!(batch_id.is_none());
        assert!(detector.current_batch_id.is_none());
    }

    #[test]
    fn test_batch_change_detector_ai_changes_group_together() {
        let mut detector = BatchChangeDetector::new();
        let ai_origin = ChangeOrigin::AIAgent {
            tool_name: "Claude Code".to_string(),
            process_id: Some(123),
        };
        
        // First AI change starts a batch
        let path1 = PathBuf::from("file1.rs");
        let batch_id1 = detector.process_change(&path1, &ai_origin);
        assert!(batch_id1.is_some());
        
        // Second AI change within time window should join the batch
        let path2 = PathBuf::from("file2.rs");
        let batch_id2 = detector.process_change(&path2, &ai_origin);
        assert_eq!(batch_id1, batch_id2);
        
        assert_eq!(detector.recent_changes.len(), 2);
    }

    #[test]
    fn test_batch_change_detector_time_gap_creates_new_batch() {
        let mut detector = BatchChangeDetector::new();
        let ai_origin = ChangeOrigin::AIAgent {
            tool_name: "Claude Code".to_string(),
            process_id: Some(123),
        };
        
        // First change
        let path1 = PathBuf::from("file1.rs");
        let batch_id1 = detector.process_change(&path1, &ai_origin);
        assert!(batch_id1.is_some());
        
        // Simulate time gap by manually updating last_batch_time
        detector.last_batch_time = Instant::now() - Duration::from_secs(10);
        
        // Add a small delay to ensure different timestamp
        std::thread::sleep(Duration::from_millis(1));
        
        // Second change after gap should start new batch
        let path2 = PathBuf::from("file2.rs");
        let batch_id2 = detector.process_change(&path2, &ai_origin);
        
        assert!(batch_id2.is_some());
        assert_ne!(batch_id1, batch_id2);
    }

    #[test]
    fn test_batch_change_detector_cleanup_old_changes() {
        let mut detector = BatchChangeDetector::new();
        let ai_origin = ChangeOrigin::AIAgent {
            tool_name: "Claude Code".to_string(),
            process_id: Some(123),
        };
        
        // Add a change and manually set its timestamp to be old
        let path = PathBuf::from("test.rs");
        detector.process_change(&path, &ai_origin);
        
        // Manually age the change
        if let Some(change) = detector.recent_changes.get_mut(0) {
            change.timestamp = Instant::now() - Duration::from_secs(35);
        }
        
        // Process a new change, which should trigger cleanup
        let new_path = PathBuf::from("new_test.rs");
        detector.process_change(&new_path, &ai_origin);
        
        // Old change should be cleaned up, only new change should remain
        assert_eq!(detector.recent_changes.len(), 1);
        // Verify the most recent change has the correct origin
        assert!(matches!(detector.recent_changes[0].origin, ChangeOrigin::AIAgent { .. }));
    }

    #[test]
    fn test_batch_change_detector_different_origins_dont_batch() {
        let mut detector = BatchChangeDetector::new();
        
        let ai_origin = ChangeOrigin::AIAgent {
            tool_name: "Claude Code".to_string(),
            process_id: Some(123),
        };
        let tool_origin = ChangeOrigin::Tool { name: "rustfmt".to_string() };
        
        // First AI change starts batch
        let path1 = PathBuf::from("file1.rs");
        let batch_id1 = detector.process_change(&path1, &ai_origin);
        assert!(batch_id1.is_some());
        
        // Tool change should not join AI batch
        let path2 = PathBuf::from("file2.rs");
        let batch_id2 = detector.process_change(&path2, &tool_origin);
        assert!(batch_id2.is_none());
    }
}