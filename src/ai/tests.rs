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
        assert_eq!(detector.recent_changes[0].path, new_path);
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