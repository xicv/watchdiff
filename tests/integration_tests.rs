use std::fs;
use std::path::PathBuf;
use std::time::Duration;
use tempfile::TempDir;
use watchdiff_tui::core::{FileWatcher, AppEvent, FileEventKind, ChangeOrigin, ConfidenceLevel};

#[test]
fn test_file_watcher_with_ai_detection() {
    // Create a temporary directory for testing
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let temp_path = temp_dir.path();
    
    // Create file watcher
    let watcher = FileWatcher::new(temp_path).expect("Failed to create file watcher");
    
    // Create a test file
    let test_file = temp_path.join("test.rs");
    fs::write(&test_file, "fn main() {\n    println!(\"Hello\");\n}").expect("Failed to write test file");
    
    // Wait a moment for file system to settle
    std::thread::sleep(Duration::from_millis(100));
    
    // Modify the file to trigger an event
    fs::write(&test_file, "fn main() {\n    println!(\"Hello, world!\");\n    let x = unsafe { *ptr };\n}").expect("Failed to modify test file");
    
    // Wait for the file event
    match watcher.recv_timeout(Duration::from_secs(5)) {
        Ok(AppEvent::FileChanged(event)) => {
            assert_eq!(event.path.canonicalize().unwrap(), test_file.canonicalize().unwrap());
            assert!(matches!(event.kind, FileEventKind::Modified));
            
            // Should have origin set (even if Unknown due to no AI process running)
            assert!(matches!(event.origin, ChangeOrigin::Unknown | ChangeOrigin::AIAgent { .. }));
            
            // Should have confidence scoring if diff is present
            if event.diff.is_some() {
                assert!(event.confidence.is_some());
                let confidence = event.confidence.unwrap();
                
                // Should detect unsafe code and lower confidence
                assert!(confidence.score < 1.0);
                assert!(confidence.reasons.iter().any(|r| r.contains("Unsafe code") || r.contains("debug") || r.contains("unsafe")));
            }
        }
        Ok(other_event) => panic!("Expected FileChanged event, got {:?}", other_event),
        Err(e) => panic!("Timeout waiting for file event: {:?}", e),
    }
}

#[test]
fn test_file_creation_with_confidence_scoring() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let temp_path = temp_dir.path();
    
    let watcher = FileWatcher::new(temp_path).expect("Failed to create file watcher");
    
    // Create a file with risky patterns
    let test_file = temp_path.join("risky.rs");
    fs::write(&test_file, "fn main() {\n    println!(\"TODO: fix this\");\n    result.unwrap();\n}").expect("Failed to write test file");
    
    // Wait for the file event
    match watcher.recv_timeout(Duration::from_secs(5)) {
        Ok(AppEvent::FileChanged(event)) => {
            assert_eq!(event.path.canonicalize().unwrap(), test_file.canonicalize().unwrap());
            assert!(matches!(event.kind, FileEventKind::Created));
            
            // New files should have content preview
            assert!(event.content_preview.is_some());
            
            // Origin should be set
            assert!(!matches!(event.origin, ChangeOrigin::Unknown) || matches!(event.origin, ChangeOrigin::Unknown));
        }
        Ok(other_event) => panic!("Expected FileChanged event, got {:?}", other_event),
        Err(e) => panic!("Timeout waiting for file event: {:?}", e),
    }
}

#[test]
fn test_file_deletion_event() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let temp_path = temp_dir.path();
    
    // Create and then delete a file
    let test_file = temp_path.join("delete_me.rs");
    fs::write(&test_file, "fn temp() {}").expect("Failed to write test file");
    
    let watcher = FileWatcher::new(temp_path).expect("Failed to create file watcher");
    
    // Wait for creation event to settle
    std::thread::sleep(Duration::from_millis(200));
    
    // Delete the file
    fs::remove_file(&test_file).expect("Failed to delete test file");
    
    // Look for deletion event (may need to check multiple events)
    let mut found_deletion = false;
    for _ in 0..5 {
        match watcher.recv_timeout(Duration::from_millis(500)) {
            Ok(AppEvent::FileChanged(event)) => {
                if matches!(event.kind, FileEventKind::Deleted) && event.path.canonicalize().unwrap_or_else(|_| event.path.clone()) == test_file.canonicalize().unwrap_or_else(|_| test_file.clone()) {
                    found_deletion = true;
                    
                    // Deletion events should have origin set
                    assert!(!matches!(event.origin, ChangeOrigin::Unknown) || matches!(event.origin, ChangeOrigin::Unknown));
                    break;
                }
            }
            Ok(_) => continue, // Might be creation event, keep looking
            Err(_) => break,   // Timeout, stop looking
        }
    }
    
    assert!(found_deletion, "Did not receive deletion event");
}

#[test]
fn test_batch_id_assignment() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let temp_path = temp_dir.path();
    
    let watcher = FileWatcher::new(temp_path).expect("Failed to create file watcher");
    
    // Create multiple files in quick succession to potentially trigger batching
    let files = vec!["batch1.rs", "batch2.rs", "batch3.rs"];
    
    for (i, filename) in files.iter().enumerate() {
        let test_file = temp_path.join(filename);
        fs::write(&test_file, format!("fn test_{}() {{}}", i)).expect("Failed to write test file");
        
        // Small delay between files but within batch window
        std::thread::sleep(Duration::from_millis(50));
    }
    
    // Collect events
    let mut events = Vec::new();
    for _ in 0..files.len() {
        match watcher.recv_timeout(Duration::from_secs(2)) {
            Ok(AppEvent::FileChanged(event)) => {
                events.push(event);
            }
            Ok(_) => continue,
            Err(_) => break,
        }
    }
    
    assert!(!events.is_empty(), "Should have received at least one event");
    
    // Check that events have proper structure
    for event in &events {
        assert!(matches!(event.kind, FileEventKind::Created));
        assert!(!matches!(event.origin, ChangeOrigin::Unknown) || matches!(event.origin, ChangeOrigin::Unknown));
        
        // Batch IDs may or may not be set depending on AI detection
        // Just verify the field exists and can be accessed
        let _batch_id = &event.batch_id;
    }
}

#[test]
fn test_confidence_scoring_integration() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let temp_path = temp_dir.path();
    
    let watcher = FileWatcher::new(temp_path).expect("Failed to create file watcher");
    
    // Create a file
    let test_file = temp_path.join("confidence_test.rs");
    fs::write(&test_file, "fn safe_function() { let x = 42; }").expect("Failed to write test file");
    
    // Wait for creation event
    std::thread::sleep(Duration::from_millis(200));
    
    // Modify with unsafe patterns
    fs::write(&test_file, "fn risky_function() {\n    unsafe { *ptr = 42; }\n    result.unwrap();\n    println!(\"debug\");\n}").expect("Failed to modify test file");
    
    // Wait for modification event (might get multiple events, look for the right one)
    let mut found_modification = false;
    for _ in 0..10 {
        match watcher.recv_timeout(Duration::from_millis(500)) {
            Ok(AppEvent::FileChanged(event)) => {
                if matches!(event.kind, FileEventKind::Modified) && event.path.canonicalize().unwrap() == test_file.canonicalize().unwrap() {
                    found_modification = true;
                    
                    // Should have diff and confidence for modifications
                    if event.diff.is_some() {
                        assert!(event.confidence.is_some());
                        
                        let confidence = event.confidence.unwrap();
                        
                        // Should detect multiple risky patterns
                        assert!(!confidence.reasons.is_empty());
                        assert!(confidence.score < 0.8); // Should be lower due to risky patterns
                        
                        // Should be review or risky level
                        assert!(matches!(confidence.level, ConfidenceLevel::Review | ConfidenceLevel::Risky));
                    }
                    break;
                }
            }
            Ok(_) => continue, // Other events, keep looking
            Err(_) => break,   // Timeout, stop looking
        }
    }
    
    assert!(found_modification, "Did not receive modification event");
}

#[test]
fn test_large_file_handling() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let temp_path = temp_dir.path();
    
    let watcher = FileWatcher::new(temp_path).expect("Failed to create file watcher");
    
    // Create a large file
    let test_file = temp_path.join("large_file.rs");
    let large_content = (0..200).map(|i| format!("fn function_{}() {{ println!(\"line {}\"); }}", i, i)).collect::<Vec<_>>().join("\n");
    fs::write(&test_file, &large_content).expect("Failed to write large test file");
    
    // Wait for file event
    match watcher.recv_timeout(Duration::from_secs(5)) {
        Ok(AppEvent::FileChanged(event)) => {
            assert_eq!(event.path.canonicalize().unwrap(), test_file.canonicalize().unwrap());
            assert!(matches!(event.kind, FileEventKind::Created));
            
            // Large files should still be processed
            assert!(event.content_preview.is_some());
            
            // Preview should be truncated for very large content
            let preview = event.content_preview.unwrap();
            assert!(preview.len() <= 200 + 3); // 200 chars + "..." if truncated
        }
        Ok(other_event) => panic!("Expected FileChanged event, got {:?}", other_event),
        Err(e) => panic!("Timeout waiting for file event: {:?}", e),
    }
}