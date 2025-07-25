//! Export functionality for saving diffs and patches
//!
//! This module provides functionality to export diffs in various formats
//! to files or other outputs.

use std::fs;
use std::io::Write;
use std::path::Path;
use anyhow::Result;
use crate::diff::{DiffResult, DiffFormatter, DiffFormat};
use crate::core::FileEvent;

/// Export configuration
#[derive(Debug, Clone)]
pub struct ExportConfig {
    pub format: DiffFormat,
    pub include_stats: bool,
    pub include_metadata: bool,
    pub width: Option<usize>, // For side-by-side format
}

impl Default for ExportConfig {
    fn default() -> Self {
        Self {
            format: DiffFormat::Unified,
            include_stats: true,
            include_metadata: true,
            width: Some(120),
        }
    }
}

/// Handles exporting diffs to various formats and destinations
pub struct DiffExporter {
    config: ExportConfig,
}

impl DiffExporter {
    pub fn new(config: ExportConfig) -> Self {
        Self { config }
    }
    
    pub fn with_format(format: DiffFormat) -> Self {
        Self {
            config: ExportConfig {
                format,
                ..Default::default()
            }
        }
    }
    
    /// Export a single diff result to a file
    pub fn export_diff<P: AsRef<Path>>(
        &self,
        result: &DiffResult,
        old_path: &Path,
        new_path: &Path,
        output_path: P,
    ) -> Result<()> {
        let mut content = String::new();
        
        // Add metadata if requested
        if self.config.include_metadata {
            content.push_str(&self.format_metadata(old_path, new_path));
            content.push_str("\n\n");
        }
        
        // Add stats if requested  
        if self.config.include_stats {
            content.push_str(&format!("Changes: {}\n\n", DiffFormatter::format_stats(result)));
        }
        
        // Add the diff content
        content.push_str(&DiffFormatter::format(
            result,
            self.config.format,
            old_path,
            new_path,
            self.config.width,
        ));
        
        fs::write(output_path.as_ref(), content)?;
        Ok(())
    }
    
    /// Export multiple file events as a single patch
    pub fn export_multifile_patch<P: AsRef<Path>>(
        &self,
        events: &[FileEvent],
        output_path: P,
    ) -> Result<()> {
        let mut content = String::new();
        
        // Add header
        if self.config.include_metadata {
            content.push_str(&format!(
                "Multi-file patch containing {} files\n",
                events.len()
            ));
            content.push_str(&format!(
                "Generated at: {}\n\n",
                chrono::Utc::now().format("%Y-%m-%d %H:%M:%S UTC")
            ));
        }
        
        // Process each file event
        for (i, event) in events.iter().enumerate() {
            if i > 0 {
                content.push_str("\n\n");
            }
            
            content.push_str(&self.format_file_event(event));
        }
        
        fs::write(output_path.as_ref(), content)?;
        Ok(())
    }
    
    /// Export to a writer (for streaming or custom outputs)
    pub fn export_diff_to_writer<W: Write>(
        &self,
        result: &DiffResult,
        old_path: &Path,
        new_path: &Path,
        writer: &mut W,
    ) -> Result<()> {
        if self.config.include_metadata {
            writeln!(writer, "{}", self.format_metadata(old_path, new_path))?;
            writeln!(writer)?;
        }
        
        if self.config.include_stats {
            writeln!(writer, "Changes: {}", DiffFormatter::format_stats(result))?;
            writeln!(writer)?;
        }
        
        write!(writer, "{}", DiffFormatter::format(
            result,
            self.config.format,
            old_path,
            new_path,
            self.config.width,
        ))?;
        
        Ok(())
    }
    
    /// Create a patch bundle (tar/zip) with multiple patches
    pub fn create_patch_bundle<P: AsRef<Path>>(
        &self,
        events: &[FileEvent],
        bundle_path: P,
    ) -> Result<()> {
        // For now, just create a directory with individual patch files
        let bundle_dir = bundle_path.as_ref();
        fs::create_dir_all(bundle_dir)?;
        
        for (i, event) in events.iter().enumerate() {
            let filename = format!("{:03}_{}.patch", 
                i + 1, 
                event.path.file_name()
                    .and_then(|s| s.to_str())
                    .unwrap_or("unknown")
            );
            
            let patch_path = bundle_dir.join(filename);
            let patch_content = self.format_file_event(event);
            fs::write(patch_path, patch_content)?;
        }
        
        // Write a manifest file
        let manifest_content = self.create_manifest(events);
        fs::write(bundle_dir.join("manifest.txt"), manifest_content)?;
        
        Ok(())
    }
    
    fn format_metadata(&self, old_path: &Path, new_path: &Path) -> String {
        format!(
            "Diff between {} and {}\nGenerated at: {}",
            old_path.display(),
            new_path.display(),
            chrono::Utc::now().format("%Y-%m-%d %H:%M:%S UTC")
        )
    }
    
    fn format_file_event(&self, event: &FileEvent) -> String {
        let mut content = String::new();
        
        // Add event metadata
        content.push_str(&format!("File: {}\n", event.path.display()));
        content.push_str(&format!("Event: {:?}\n", event.kind));
        content.push_str(&format!("Timestamp: {}\n", 
            chrono::DateTime::<chrono::Utc>::from(event.timestamp)
                .format("%Y-%m-%d %H:%M:%S UTC")
        ));
        
        // Add diff if available
        if let Some(ref diff) = event.diff {
            content.push_str("\n");
            content.push_str(diff);
        }
        
        content
    }
    
    fn create_manifest(&self, events: &[FileEvent]) -> String {
        let mut content = String::new();
        
        content.push_str(&format!("Patch Bundle Manifest\n"));
        content.push_str(&format!("Generated at: {}\n", 
            chrono::Utc::now().format("%Y-%m-%d %H:%M:%S UTC")
        ));
        content.push_str(&format!("Total files: {}\n\n", events.len()));
        
        for (i, event) in events.iter().enumerate() {
            content.push_str(&format!(
                "{:03}. {} ({:?})\n",
                i + 1,
                event.path.display(),
                event.kind
            ));
        }
        
        content
    }
}

/// Predefined export presets
impl DiffExporter {
    /// Create an exporter for Git-style patches
    pub fn git_patch() -> Self {
        Self::with_format(DiffFormat::GitPatch)
    }
    
    /// Create an exporter for unified diffs
    pub fn unified() -> Self {
        Self::with_format(DiffFormat::Unified)
    }
    
    /// Create an exporter for side-by-side comparison
    pub fn side_by_side(width: usize) -> Self {
        Self::new(ExportConfig {
            format: DiffFormat::SideBySide,
            width: Some(width),
            ..Default::default()
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;
    use crate::diff::{DiffGenerator, DiffAlgorithmType};
    use crate::core::events::FileEventKind;
    use std::time::SystemTime;

    #[test]
    fn test_export_diff() {
        let temp_dir = TempDir::new().unwrap();
        let output_path = temp_dir.path().join("test.patch");
        
        let generator = DiffGenerator::new(DiffAlgorithmType::Myers);
        let result = generator.generate("old\nline", "new\nline");
        
        let exporter = DiffExporter::unified();
        exporter.export_diff(&result, 
            Path::new("old.txt"), 
            Path::new("new.txt"), 
            &output_path
        ).unwrap();
        
        let content = fs::read_to_string(output_path).unwrap();
        assert!(content.contains("--- old.txt"));
        assert!(content.contains("+++ new.txt"));
    }
    
    #[test] 
    fn test_export_multifile_patch() {
        let temp_dir = TempDir::new().unwrap();
        let output_path = temp_dir.path().join("multi.patch");
        
        let event = FileEvent {
            path: Path::new("test.txt").to_path_buf(),
            kind: FileEventKind::Modified,
            timestamp: SystemTime::now(),
            diff: Some("--- a\n+++ b\n@@ -1 +1 @@\n-old\n+new".to_string()),
            content_preview: None,
            origin: crate::core::ChangeOrigin::Unknown,
            confidence: None,
            batch_id: None,
        };
        
        let exporter = DiffExporter::unified();
        exporter.export_multifile_patch(&[event], &output_path).unwrap();
        
        let content = fs::read_to_string(output_path).unwrap();
        assert!(content.contains("Multi-file patch"));
        assert!(content.contains("test.txt"));
    }
}