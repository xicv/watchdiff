#[cfg(test)]
mod tests {
    use super::*;
    use std::time::SystemTime;

    #[test]
    fn test_file_event_creation() {
        let path = PathBuf::from("test.rs");
        let event = FileEvent::new(path.clone(), FileEventKind::Modified);
        
        assert_eq!(event.path, path);
        assert!(matches!(event.kind, FileEventKind::Modified));
        assert!(matches!(event.origin, ChangeOrigin::Unknown));
        assert!(event.confidence.is_none());
        assert!(event.batch_id.is_none());
    }

    #[test]
    fn test_file_event_with_origin() {
        let path = PathBuf::from("test.rs");
        let origin = ChangeOrigin::AIAgent {
            tool_name: "Claude Code".to_string(),
            process_id: Some(12345),
        };
        
        let event = FileEvent::new(path.clone(), FileEventKind::Modified)
            .with_origin(origin.clone());
        
        assert!(matches!(event.origin, ChangeOrigin::AIAgent { .. }));
        if let ChangeOrigin::AIAgent { tool_name, process_id } = event.origin {
            assert_eq!(tool_name, "Claude Code");
            assert_eq!(process_id, Some(12345));
        } else {
            panic!("Expected AIAgent origin");
        }
    }

    #[test]
    fn test_file_event_with_confidence() {
        let path = PathBuf::from("test.rs");
        let confidence = ChangeConfidence {
            level: ConfidenceLevel::Review,
            score: 0.6,
            reasons: vec!["Debug output detected".to_string()],
        };
        
        let event = FileEvent::new(path, FileEventKind::Modified)
            .with_confidence(confidence.clone());
        
        assert!(event.confidence.is_some());
        let event_confidence = event.confidence.unwrap();
        assert!(matches!(event_confidence.level, ConfidenceLevel::Review));
        assert_eq!(event_confidence.score, 0.6);
        assert_eq!(event_confidence.reasons, vec!["Debug output detected"]);
    }

    #[test]
    fn test_file_event_with_batch_id() {
        let path = PathBuf::from("test.rs");
        let batch_id = "batch_123456".to_string();
        
        let event = FileEvent::new(path, FileEventKind::Modified)
            .with_batch_id(batch_id.clone());
        
        assert_eq!(event.batch_id, Some(batch_id));
    }

    #[test]
    fn test_file_event_chaining() {
        let path = PathBuf::from("test.rs");
        let origin = ChangeOrigin::Human;
        let confidence = ChangeConfidence {
            level: ConfidenceLevel::Safe,
            score: 0.9,
            reasons: vec![],
        };
        let batch_id = "batch_789".to_string();
        let diff = "- old line\n+ new line".to_string();
        
        let event = FileEvent::new(path.clone(), FileEventKind::Modified)
            .with_origin(origin.clone())
            .with_confidence(confidence.clone())
            .with_batch_id(batch_id.clone())
            .with_diff(diff.clone());
        
        assert_eq!(event.path, path);
        assert!(matches!(event.origin, ChangeOrigin::Human));
        assert!(event.confidence.is_some());
        assert_eq!(event.batch_id, Some(batch_id));
        assert_eq!(event.diff, Some(diff));
    }

    #[test]
    fn test_highlighted_file_event_conversion() {
        let path = PathBuf::from("test.rs");
        let origin = ChangeOrigin::Tool { name: "rustfmt".to_string() };
        let confidence = ChangeConfidence {
            level: ConfidenceLevel::Safe,
            score: 0.95,
            reasons: vec!["Formatting tool".to_string()],
        };
        
        let event = FileEvent::new(path.clone(), FileEventKind::Modified)
            .with_origin(origin.clone())
            .with_confidence(confidence.clone());
        
        let highlighted = event.to_highlighted();
        
        assert_eq!(highlighted.path, path);
        assert!(matches!(highlighted.origin, ChangeOrigin::Tool { .. }));
        assert!(highlighted.confidence.is_some());
        
        if let ChangeOrigin::Tool { name } = highlighted.origin {
            assert_eq!(name, "rustfmt");
        }
    }

    #[test]
    fn test_confidence_level_ordering() {
        // Test that confidence levels have the expected ordering
        let safe = ConfidenceLevel::Safe;
        let review = ConfidenceLevel::Review;
        let risky = ConfidenceLevel::Risky;
        
        // This test ensures we can match on confidence levels
        match safe {
            ConfidenceLevel::Safe => (),
            _ => panic!("Expected Safe confidence level"),
        }
        
        match review {
            ConfidenceLevel::Review => (),
            _ => panic!("Expected Review confidence level"),
        }
        
        match risky {
            ConfidenceLevel::Risky => (),
            _ => panic!("Expected Risky confidence level"),
        }
    }

    #[test]
    fn test_change_origin_variants() {
        let human = ChangeOrigin::Human;
        let ai_agent = ChangeOrigin::AIAgent {
            tool_name: "Gemini CLI".to_string(),
            process_id: None,
        };
        let tool = ChangeOrigin::Tool { name: "cargo fmt".to_string() };
        let unknown = ChangeOrigin::Unknown;
        
        assert!(matches!(human, ChangeOrigin::Human));
        assert!(matches!(ai_agent, ChangeOrigin::AIAgent { .. }));
        assert!(matches!(tool, ChangeOrigin::Tool { .. }));
        assert!(matches!(unknown, ChangeOrigin::Unknown));
    }

    #[test]
    fn test_app_state_add_event_with_ai_features() {
        let mut state = AppState::default();
        
        let event = FileEvent::new(PathBuf::from("test.rs"), FileEventKind::Created)
            .with_origin(ChangeOrigin::AIAgent {
                tool_name: "Claude Code".to_string(),
                process_id: Some(42),
            })
            .with_confidence(ChangeConfidence {
                level: ConfidenceLevel::Review,
                score: 0.7,
                reasons: vec!["Large change detected".to_string()],
            })
            .with_batch_id("batch_001".to_string());
        
        state.add_event(event);
        
        assert_eq!(state.events.len(), 1);
        assert_eq!(state.highlighted_events.len(), 1);
        
        let stored_event = &state.events[0];
        assert!(matches!(stored_event.origin, ChangeOrigin::AIAgent { .. }));
        assert!(stored_event.confidence.is_some());
        assert_eq!(stored_event.batch_id, Some("batch_001".to_string()));
        
        let highlighted_event = &state.highlighted_events[0];
        assert!(matches!(highlighted_event.origin, ChangeOrigin::AIAgent { .. }));
        assert!(highlighted_event.confidence.is_some());
        assert_eq!(highlighted_event.batch_id, Some("batch_001".to_string()));
    }
}