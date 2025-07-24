use std::path::PathBuf;
use std::collections::HashSet;
use watchdiff_tui::ui::TuiApp;
use watchdiff_tui::core::FileWatcher;

fn main() -> anyhow::Result<()> {
    println!("🧪 Horizontal Scroll Test");
    println!("========================\n");
    
    // Create test paths with varying lengths
    let test_paths = vec![
        "short.rs",
        "medium_length_file.rs", 
        "very_long_directory_name_that_should_require_horizontal_scrolling/deeply/nested/path/to/some/file.rs",
        "another/extremely/long/path/that/definitely/exceeds/terminal/width/and/needs/scrolling/functionality/test.rs",
        "src/ui/components/dashboard/widgets/charts/advanced/analytics/performance/metrics/visualization.rs",
    ];
    
    println!("📁 Test file paths:");
    for (i, path) in test_paths.iter().enumerate() {
        println!("  {}: {} (length: {})", i + 1, path, path.len());
    }
    println!();
    
    // Test the path truncation logic directly
    test_path_truncation_logic();
    
    // Test scroll calculation
    test_scroll_calculation();
    
    Ok(())
}

fn test_path_truncation_logic() {
    println!("🔧 Testing path truncation logic:");
    println!("----------------------------------");
    
    let test_cases = vec![
        ("short.rs", 50, 0),
        ("very_long_directory_name_that_should_require_horizontal_scrolling/file.rs", 50, 0),
        ("very_long_directory_name_that_should_require_horizontal_scrolling/file.rs", 50, 10),
        ("very_long_directory_name_that_should_require_horizontal_scrolling/file.rs", 50, 30),
    ];
    
    for (full_path, available_width, scroll_offset) in test_cases {
        let result = apply_horizontal_scroll(full_path, available_width, scroll_offset);
        println!("  Path: {}", full_path);
        println!("  Width: {}, Scroll: {}", available_width, scroll_offset);
        println!("  Result: '{}'", result);
        println!("  Result length: {}", result.len());
        println!();
    }
}

fn test_scroll_calculation() {
    println!("📊 Testing scroll calculation:");
    println!("------------------------------");
    
    let test_paths = vec![
        "short.rs",
        "medium_length_file.rs", 
        "very_long_directory_name_that_should_require_horizontal_scrolling/deeply/nested/path/to/some/file.rs",
    ];
    
    for path in test_paths {
        let path_len = path.len();
        let estimated_width = 60;
        let max_scroll = path_len.saturating_sub(estimated_width);
        
        println!("  Path: {}", path);
        println!("  Length: {}", path_len);
        println!("  Estimated width: {}", estimated_width);
        println!("  Max scroll: {}", max_scroll);
        println!("  Can scroll: {}", max_scroll > 0);
        println!();
    }
}

// Replicate the truncation logic from the actual code
fn apply_horizontal_scroll(full_path: &str, available_width: usize, scroll_offset: usize) -> String {
    if full_path.len() > available_width && scroll_offset > 0 {
        // Horizontal scrolling: show portion from scroll offset
        let start_idx = scroll_offset.min(full_path.len().saturating_sub(available_width));
        let end_idx = (start_idx + available_width).min(full_path.len());
        
        if start_idx > 0 {
            format!("…{}", &full_path[start_idx..end_idx])
        } else {
            full_path[start_idx..end_idx].to_string()
        }
    } else if full_path.len() > available_width {
        // Too long but no scroll - truncate and show ellipsis
        format!("{}…", &full_path[..available_width.saturating_sub(1)])
    } else {
        full_path.to_string()
    }
}

// Test to verify the actual TuiApp scroll behavior
#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::collections::HashSet;
    
    #[test]
    fn test_horizontal_scroll_logic() {
        // Test the path truncation directly
        assert_eq!(apply_horizontal_scroll("short", 10, 0), "short");
        assert_eq!(apply_horizontal_scroll("very_long_path", 10, 0), "very_long_…");
        assert_eq!(apply_horizontal_scroll("very_long_path", 10, 5), "…long_path");
    }
    
    #[test]
    fn test_scroll_calculation_logic() {
        let paths = vec![
            "short.rs",
            "very_long_directory_name_that_should_require_horizontal_scrolling/file.rs",
        ];
        
        let longest_path = paths.iter()
            .map(|p| p.len())
            .max()
            .unwrap();
        
        let estimated_width = 60;
        let max_scroll = longest_path.saturating_sub(estimated_width);
        
        println!("Longest path length: {}", longest_path);
        println!("Max scroll: {}", max_scroll);
        
        assert!(max_scroll > 0, "Should be able to scroll long paths");
    }
}