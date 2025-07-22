use std::path::{Path, PathBuf};
use ignore::WalkBuilder;
use anyhow::Result;

pub struct FileFilter {
    root_path: PathBuf,
}

impl FileFilter {
    pub fn new<P: AsRef<Path>>(root_path: P) -> Result<Self> {
        let root_path = root_path.as_ref().to_path_buf();
        Ok(Self { root_path })
    }

    pub fn should_watch<P: AsRef<Path>>(&self, path: P) -> bool {
        let path = path.as_ref();
        
        // Always ignore .git directory itself
        if path.components().any(|comp| comp.as_os_str() == ".git") {
            return false;
        }

        // Use ignore crate to check if file should be ignored
        let walker = WalkBuilder::new(&self.root_path)
            .hidden(false)
            .git_ignore(true)
            .git_global(true) 
            .git_exclude(true)
            .ignore(true)
            .parents(true)
            .build();

        for result in walker {
            match result {
                Ok(entry) => {
                    if entry.path() == path {
                        return true;
                    }
                }
                Err(_) => continue,
            }
        }
        false
    }

    pub fn get_watchable_files(&self) -> Result<Vec<PathBuf>> {
        let mut files = Vec::new();
        
        for result in WalkBuilder::new(&self.root_path)
            .hidden(false)
            .git_ignore(true)
            .git_global(true)
            .git_exclude(true)
            .ignore(true)
            .parents(true)
            .build() {
            
            match result {
                Ok(entry) => {
                    let path = entry.path();
                    if path.is_file() {
                        files.push(path.to_path_buf());
                    }
                }
                Err(err) => {
                    tracing::warn!("Error walking directory: {}", err);
                }
            }
        }
        
        Ok(files)
    }

    pub fn is_text_file<P: AsRef<Path>>(&self, path: P) -> bool {
        let path = path.as_ref();
        
        // Check file extension for common text files
        if let Some(ext) = path.extension().and_then(|s| s.to_str()) {
            matches!(ext.to_lowercase().as_str(),
                "rs" | "py" | "js" | "ts" | "jsx" | "tsx" | "html" | "css" | "scss" |
                "json" | "toml" | "yaml" | "yml" | "xml" | "md" | "txt" | "log" |
                "c" | "cpp" | "h" | "hpp" | "java" | "kt" | "swift" | "go" |
                "php" | "rb" | "sh" | "bash" | "zsh" | "fish" | "sql" | "dockerfile" |
                "makefile" | "cmake" | "config" | "conf" | "ini" | "env"
            )
        } else {
            // Check for files without extensions that are typically text
            if let Some(filename) = path.file_name().and_then(|s| s.to_str()) {
                matches!(filename.to_lowercase().as_str(),
                    "dockerfile" | "makefile" | "readme" | "license" | "changelog" |
                    "authors" | "contributors" | "todo" | "news" | "install" | "copying"
                )
            } else {
                false
            }
        }
    }
}