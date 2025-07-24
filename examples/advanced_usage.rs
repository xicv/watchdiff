use watchdiff_tui::{
    diff::{DiffGenerator, DiffAlgorithmType, DiffFormatter},
    export::DiffExporter,
    core::{FileEvent, FileEventKind},
};
use std::path::Path;
use std::time::SystemTime;
use tempfile::TempDir;

fn main() -> anyhow::Result<()> {
    println!("🚀 WatchDiff Advanced Usage Examples\n");
    
    // Example 1: Different Diff Algorithms
    demo_diff_algorithms()?;
    
    // Example 2: Export Functionality
    demo_export_functionality()?;
    
    // Example 3: Diff Statistics
    demo_diff_statistics()?;
    
    Ok(())
}

fn demo_diff_algorithms() -> anyhow::Result<()> {
    println!("📊 Example 1: Different Diff Algorithms");
    println!("=====================================\n");
    
    let old_content = r#"
fn calculate_sum(numbers: Vec<i32>) -> i32 {
    let mut sum = 0;
    for num in numbers {
        sum += num;
    }
    sum
}

fn main() {
    let nums = vec![1, 2, 3, 4, 5];
    println!("Sum: {}", calculate_sum(nums));
}
"#;
    
    let new_content = r#"
fn calculate_sum(numbers: &[i32]) -> i32 {
    numbers.iter().sum()
}

fn calculate_average(numbers: &[i32]) -> f64 {
    if numbers.is_empty() {
        0.0
    } else {
        numbers.iter().sum::<i32>() as f64 / numbers.len() as f64
    }
}

fn main() {
    let nums = vec![1, 2, 3, 4, 5];
    println!("Sum: {}", calculate_sum(&nums));
    println!("Average: {:.2}", calculate_average(&nums));
}
"#;

    // Test different algorithms
    for algorithm in DiffAlgorithmType::all() {
        println!("Using {} algorithm:", algorithm.name());
        println!("{}", "─".repeat(40));
        
        let generator = DiffGenerator::new(*algorithm);
        let result = generator.generate(old_content, new_content);
        
        let formatted = DiffFormatter::format_unified(&result, "old.rs", "new.rs");
        println!("{}\n", formatted);
        
        println!("Stats: {}", DiffFormatter::format_stats(&result));
        println!("{}\n", "=".repeat(60));
    }
    
    Ok(())
}

fn demo_export_functionality() -> anyhow::Result<()> {
    println!("📁 Example 2: Export Functionality");
    println!("==================================\n");
    
    let temp_dir = TempDir::new()?;
    let export_path = temp_dir.path().join("example.patch");
    
    // Create a sample diff
    let generator = DiffGenerator::new(DiffAlgorithmType::Myers);
    let result = generator.generate(
        "Hello, World!\nThis is a test.",
        "Hello, Rust!\nThis is a test.\nWith more content!"
    );
    
    // Export as unified diff
    let exporter = DiffExporter::unified();
    exporter.export_diff(&result, 
        Path::new("old.txt"), 
        Path::new("new.txt"), 
        &export_path)?;
    
    println!("✅ Exported unified diff to: {}", export_path.display());
    
    // Export as Git patch
    let git_patch_path = temp_dir.path().join("git.patch");
    let git_exporter = DiffExporter::git_patch();
    git_exporter.export_diff(&result, 
        Path::new("example.txt"), 
        Path::new("example.txt"), 
        &git_patch_path)?;
    
    println!("✅ Exported Git patch to: {}", git_patch_path.display());
    
    // Create a multifile patch
    let events = vec![
        FileEvent {
            path: Path::new("src/main.rs").to_path_buf(),
            kind: FileEventKind::Modified,
            timestamp: SystemTime::now(),
            diff: Some("--- a/src/main.rs\n+++ b/src/main.rs\n@@ -1,3 +1,4 @@\n fn main() {\n+    println!(\"Hello!\");\n     println!(\"World!\");\n }".to_string()),
            content_preview: None,
        },
        FileEvent {
            path: Path::new("Cargo.toml").to_path_buf(),
            kind: FileEventKind::Modified,
            timestamp: SystemTime::now(),
            diff: Some("--- a/Cargo.toml\n+++ b/Cargo.toml\n@@ -1,4 +1,5 @@\n [package]\n name = \"example\"\n version = \"0.1.0\"\n+edition = \"2021\"".to_string()),
            content_preview: None,
        },
    ];
    
    let multifile_patch = temp_dir.path().join("multifile.patch");
    exporter.export_multifile_patch(&events, &multifile_patch)?;
    
    println!("✅ Exported multifile patch to: {}", multifile_patch.display());
    
    // Show the contents
    let content = std::fs::read_to_string(&export_path)?;
    println!("\n📄 Sample unified diff content:");
    println!("{}", "─".repeat(40));
    println!("{}", content);
    
    Ok(())
}

fn demo_diff_statistics() -> anyhow::Result<()> {
    println!("📈 Example 3: Diff Statistics");
    println!("=============================\n");
    
    let test_cases = vec![
        (
            "Simple addition",
            "line1\nline2\nline3",
            "line1\nline2\nNEW LINE\nline3"
        ),
        (
            "Replacement",
            "Hello World\nGoodbye",
            "Hello Rust\nGoodbye"
        ),
        (
            "Major refactor",
            "class OldClass {\n    method1() {}\n    method2() {}\n}",
            "class NewClass {\n    constructor() {}\n    newMethod() {}\n    anotherMethod() {}\n}"
        ),
    ];
    
    for (name, old, new) in test_cases {
        println!("Test: {}", name);
        println!("{}", "─".repeat(name.len() + 6));
        
        let generator = DiffGenerator::new(DiffAlgorithmType::Myers);
        let result = generator.generate(old, new);
        
        let stats = &result.stats;
        println!("📊 Lines added: {}", stats.lines_added);
        println!("📊 Lines removed: {}", stats.lines_removed);
        println!("📊 Total changes: {}", stats.total_changes());
        println!("📊 Net change: {:+}", stats.net_change());
        println!("📊 Hunks: {}", stats.hunks);
        
        if stats.total_changes() > 0 {
            let change_ratio = stats.lines_added as f64 / (stats.lines_added + stats.lines_removed) as f64 * 100.0;
            println!("📊 Addition ratio: {:.1}%", change_ratio);
        }
        
        println!();
    }
    
    Ok(())
}