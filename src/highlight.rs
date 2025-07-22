use std::path::Path;
use syntect::easy::HighlightLines;
use syntect::highlighting::{ThemeSet, Style};
use syntect::parsing::SyntaxSet;
use syntect::util::{as_24_bit_terminal_escaped, LinesWithEndings};
use ratatui::style::{Color, Modifier};

pub struct SyntaxHighlighter {
    syntax_set: SyntaxSet,
    theme_set: ThemeSet,
}

impl Default for SyntaxHighlighter {
    fn default() -> Self {
        Self::new()
    }
}

impl SyntaxHighlighter {
    pub fn new() -> Self {
        Self {
            syntax_set: SyntaxSet::load_defaults_newlines(),
            theme_set: ThemeSet::load_defaults(),
        }
    }

    pub fn get_language_from_path<P: AsRef<Path>>(&self, path: P) -> Option<String> {
        let path = path.as_ref();
        
        // First try by file extension
        if let Some(ext) = path.extension().and_then(|s| s.to_str()) {
            if let Some(syntax) = self.syntax_set.find_syntax_by_extension(ext) {
                return Some(syntax.name.clone());
            }
        }
        
        // Then try by filename
        if let Some(filename) = path.file_name().and_then(|s| s.to_str()) {
            if let Some(syntax) = self.syntax_set.find_syntax_by_name(filename) {
                return Some(syntax.name.clone());
            }
            
            // Handle special cases
            match filename.to_lowercase().as_str() {
                "dockerfile" => return Some("Dockerfile".to_string()),
                "makefile" => return Some("Makefile".to_string()),
                "cargo.toml" | "pyproject.toml" => return Some("TOML".to_string()),
                "package.json" => return Some("JSON".to_string()),
                _ => {}
            }
        }
        
        // Try by first line (for shebangs)
        None
    }

    pub fn highlight_line(&self, line: &str, language: &str, _line_number: usize) -> Vec<(ratatui::style::Style, String)> {
        let syntax = match self.syntax_set.find_syntax_by_name(language) {
            Some(syntax) => syntax,
            None => return vec![(ratatui::style::Style::default(), line.to_string())],
        };

        let theme = match self.theme_set.themes.get("base16-ocean.dark") {
            Some(theme) => theme,
            None => &self.theme_set.themes["InspiredGitHub"],
        };

        let mut highlighter = HighlightLines::new(syntax, theme);
        
        match highlighter.highlight_line(line, &self.syntax_set) {
            Ok(ranges) => {
                let mut result = Vec::new();
                for (style, text) in ranges {
                    let ratatui_style = self.convert_syntect_style_to_ratatui(style);
                    result.push((ratatui_style, text.to_string()));
                }
                result
            }
            Err(_) => vec![(ratatui::style::Style::default(), line.to_string())],
        }
    }

    pub fn highlight_code(&self, code: &str, language: &str) -> Vec<Vec<(ratatui::style::Style, String)>> {
        let syntax = match self.syntax_set.find_syntax_by_name(language) {
            Some(syntax) => syntax,
            None => return code.lines().map(|line| vec![(ratatui::style::Style::default(), line.to_string())]).collect(),
        };

        let theme = match self.theme_set.themes.get("base16-ocean.dark") {
            Some(theme) => theme,
            None => &self.theme_set.themes["InspiredGitHub"],
        };

        let mut highlighter = HighlightLines::new(syntax, theme);
        let mut result = Vec::new();

        for line in LinesWithEndings::from(code) {
            match highlighter.highlight_line(line, &self.syntax_set) {
                Ok(ranges) => {
                    let mut line_result = Vec::new();
                    for (style, text) in ranges {
                        let ratatui_style = self.convert_syntect_style_to_ratatui(style);
                        line_result.push((ratatui_style, text.to_string()));
                    }
                    result.push(line_result);
                }
                Err(_) => {
                    result.push(vec![(ratatui::style::Style::default(), line.to_string())]);
                }
            }
        }

        result
    }

    pub fn get_terminal_highlighted(&self, code: &str, language: &str) -> String {
        let syntax = match self.syntax_set.find_syntax_by_name(language) {
            Some(syntax) => syntax,
            None => return code.to_string(),
        };

        let theme = match self.theme_set.themes.get("base16-ocean.dark") {
            Some(theme) => theme,
            None => &self.theme_set.themes["InspiredGitHub"],
        };

        let mut highlighter = HighlightLines::new(syntax, theme);
        let mut result = String::new();

        for line in LinesWithEndings::from(code) {
            match highlighter.highlight_line(line, &self.syntax_set) {
                Ok(ranges) => {
                    let escaped = as_24_bit_terminal_escaped(&ranges[..], false);
                    result.push_str(&escaped);
                }
                Err(_) => {
                    result.push_str(line);
                }
            }
        }

        result
    }

    fn convert_syntect_style_to_ratatui(&self, style: Style) -> ratatui::style::Style {
        let mut ratatui_style = ratatui::style::Style::default();

        // Convert foreground color
        if style.foreground.a > 0 {
            ratatui_style = ratatui_style.fg(Color::Rgb(
                style.foreground.r,
                style.foreground.g,
                style.foreground.b,
            ));
        }

        // Convert background color
        if style.background.a > 0 {
            ratatui_style = ratatui_style.bg(Color::Rgb(
                style.background.r,
                style.background.g,
                style.background.b,
            ));
        }

        // Convert font styles
        if style.font_style.contains(syntect::highlighting::FontStyle::BOLD) {
            ratatui_style = ratatui_style.add_modifier(Modifier::BOLD);
        }
        if style.font_style.contains(syntect::highlighting::FontStyle::ITALIC) {
            ratatui_style = ratatui_style.add_modifier(Modifier::ITALIC);
        }
        if style.font_style.contains(syntect::highlighting::FontStyle::UNDERLINE) {
            ratatui_style = ratatui_style.add_modifier(Modifier::UNDERLINED);
        }

        ratatui_style
    }

    pub fn get_common_languages() -> Vec<&'static str> {
        vec![
            "Rust",
            "Python",
            "JavaScript",
            "TypeScript",
            "Java",
            "C",
            "C++",
            "C#",
            "Go",
            "Swift",
            "Kotlin",
            "PHP",
            "Ruby",
            "HTML",
            "CSS",
            "SCSS",
            "JSON",
            "YAML",
            "TOML",
            "XML",
            "Markdown",
            "Bash",
            "Fish",
            "Zsh",
            "PowerShell",
            "Dockerfile",
            "Makefile",
            "SQL",
            "GraphQL",
        ]
    }
}

// Helper function to detect if a file is likely to be binary
pub fn is_likely_text_file<P: AsRef<Path>>(path: P) -> bool {
    let path = path.as_ref();
    
    if let Some(ext) = path.extension().and_then(|s| s.to_str()) {
        let ext = ext.to_lowercase();
        matches!(ext.as_str(),
            // Source code
            "rs" | "py" | "js" | "ts" | "jsx" | "tsx" | "java" | "kt" | "swift" |
            "go" | "c" | "cpp" | "cc" | "cxx" | "h" | "hpp" | "cs" | "php" | "rb" |
            
            // Web
            "html" | "htm" | "css" | "scss" | "sass" | "less" | "vue" | "svelte" |
            
            // Config and data
            "json" | "yaml" | "yml" | "toml" | "xml" | "ini" | "conf" | "config" |
            "env" | "properties" | "cfg" | "plist" |
            
            // Documentation
            "md" | "txt" | "rst" | "adoc" | "tex" | "rtf" |
            
            // Scripts
            "sh" | "bash" | "zsh" | "fish" | "ps1" | "bat" | "cmd" |
            
            // Other
            "sql" | "graphql" | "dockerfile" | "makefile" | "cmake" | "log"
        )
    } else {
        // Files without extensions that are typically text
        if let Some(filename) = path.file_name().and_then(|s| s.to_str()) {
            matches!(filename.to_lowercase().as_str(),
                "dockerfile" | "makefile" | "cmake" | "readme" | "license" | 
                "changelog" | "authors" | "contributors" | "todo" | "news"
            )
        } else {
            false
        }
    }
}