# WatchDiff 🔭

A high-performance file watcher with beautiful TUI showing real-time diffs, written in Rust.

## Features

- 🚀 **High Performance**: Built with Rust for maximum speed and efficiency
- 🎨 **Beautiful TUI**: Rich terminal interface with scrollable diff log and file list  
- 🌈 **Syntax Highlighting**: Full syntax highlighting for 25+ programming languages in diffs
- 📂 **Smart Filtering**: Respects `.gitignore` patterns automatically
- 🔍 **Real-time Diffs**: Shows beautiful diffs for text file changes as they happen
- ⌨️ **Easy CLI**: Multiple output formats and intuitive keyboard shortcuts
- 🧵 **Async**: Non-blocking file watching with threaded architecture

## Installation

### From Cargo

```bash
cargo install watchdiff-tui
```

### Build from Source

```bash
git clone git@github.com:xicv/watchdiff.git
cd watchdiff
cargo build --release
./target/release/watchdiff
```

## Usage

### Basic Usage

```bash
# Watch current directory with TUI
watchdiff-tui

# Watch specific directory
watchdiff-tui /path/to/project

# Watch with text output
watchdiff-tui --output text

# Watch only specific file types
watchdiff-tui --extensions rs,py,js

# JSON output for scripting
watchdiff-tui --output json
```

## User Interface

### TUI Mode (Default)

WatchDiff features a modern, responsive terminal user interface built with ratatui. The interface is designed for maximum productivity and ease of use.

#### Interface Layout

```
┌─────────────────────────────────────────────────────────────────────────┐
│ Changes (↑↓ to scroll, PgUp/PgDn, Home/End)                           │
├─────────────────────────────────────────────────────────────────────────┤
│ [12:34:56] MODIFIED src/main.rs                                        │
│ --- src/main.rs                                                         │
│ +++ src/main.rs                                                         │
│ @@ -15,7 +15,8 @@                                                      │
│ - fn main() {                                                           │
│ + fn main() -> Result<()> {                                            │
│     let cli = Cli::parse();                                            │
│ +   println!("Starting WatchDiff...");                                │
│                                                                         │
│ [12:34:52] CREATED docs/api.md                                          │
│ Preview:                                                                │
│   # API Documentation                                                   │
│   This document describes the WatchDiff API...                         │
│                                                                         │
├─────────────────────────────────────────────────────────────────────────┤
│ Watched Files (847) (←→ to scroll)                                     │
├─────────────────────────────────────────────────────────────────────────┤
│ src/main.rs                                                             │
│ src/lib.rs                                                              │
│ src/cli.rs                                                              │
│ src/watcher.rs                                                          │
│ src/diff.rs                                                             │
│ src/tui.rs                                                              │
│ ...                                                                     │
├─────────────────────────────────────────────────────────────────────────┤
│ Status                                                                  │
├─────────────────────────────────────────────────────────────────────────┤
│ Press q to quit, h for help, ↑↓ to scroll diff                        │
│ Events: 23 | Files watched: 847                                        │
└─────────────────────────────────────────────────────────────────────────┘
```

#### Panel Breakdown

**📊 Top Panel - Changes Log (70% height)**
- Real-time scrollable feed of file changes
- Color-coded event types:
  - 🟢 **CREATED** - New files (green)
  - 🟡 **MODIFIED** - Changed files (yellow) 
  - 🔴 **DELETED** - Removed files (red)
  - 🔵 **MOVED** - Renamed/moved files (blue)
- **Full syntax highlighting** for 25+ languages including:
  - Rust, Python, JavaScript, TypeScript, Java, C/C++, Go
  - HTML, CSS, JSON, YAML, TOML, XML, Markdown
  - Bash, SQL, Dockerfile, and many more
- Unified diff format with intelligent syntax preservation
- Timestamps for each event
- Syntax-highlighted content preview for new files
- Scrollbar for long change lists

**📁 Middle Panel - File List (25% height)**
- Live list of all watched files
- File count indicator
- Alternating row colors for readability
- Horizontal scrolling for long paths
- Respects .gitignore patterns

**ℹ️ Bottom Panel - Status Bar (5% height)**
- Real-time statistics (event count, file count)
- Keyboard shortcuts reminder
- Application status indicators

#### Visual Features

- **Color Coding**: Intuitive colors for different change types
- **Responsive Layout**: Adapts to terminal size changes
- **Smooth Scrolling**: Efficient scrolling through large change logs
- **Typography**: Clear, readable text with proper spacing
- **Borders**: Clean visual separation between panels
- **Icons**: Unicode symbols for better visual hierarchy

#### Keyboard Shortcuts

| Key | Action |
|-----|--------|
| `q`, `Esc` | Quit application |
| `h`, `F1` | Toggle help screen |
| `↑`, `k` | Scroll diff log up |
| `↓`, `j` | Scroll diff log down |
| `PgUp` | Scroll diff log up (fast) |
| `PgDn` | Scroll diff log down (fast) |
| `Home` | Go to top of diff log |
| `End` | Go to bottom of diff log |
| `←`, `→` | Scroll file list |

#### Help Screen

Press `h` or `F1` to open the interactive help screen:

```
┌─────────────────────────────────────────────────────────────────────────┐
│ Help                                                                    │
├─────────────────────────────────────────────────────────────────────────┤
│ WatchDiff - File Watching Tool                                         │
│                                                                         │
│ Keyboard Shortcuts:                                                     │
│                                                                         │
│   q, Esc      - Quit the application                                   │
│   h, F1       - Show/hide this help                                    │
│   ↑, k        - Scroll diff log up                                     │
│   ↓, j        - Scroll diff log down                                   │
│   PgUp        - Scroll diff log up (fast)                              │
│   PgDn        - Scroll diff log down (fast)                            │
│   Home        - Go to top of diff log                                  │
│   End         - Go to bottom of diff log                               │
│   ←, →        - Scroll file list                                       │
│                                                                         │
│ Features:                                                               │
│                                                                         │
│ • Real-time file change monitoring                                      │
│ • Respects .gitignore patterns                                          │
│ • Shows diffs for text file changes                                     │
│ • Scrollable diff log and file list                                     │
│ • High performance with async processing                                │
└─────────────────────────────────────────────────────────────────────────┘
```

### Alternative Output Modes

For non-interactive use cases, WatchDiff supports several output formats:

#### Text Mode (`--output text`)
```bash
$ watchdiff-tui --output text
Watching: /home/user/project
Press Ctrl+C to quit
---
[12:34:56] MODIFIED src/main.rs
  - fn main() {
  + fn main() -> Result<()> {
      let cli = Cli::parse();
  +   println!("Starting WatchDiff...");

[12:34:52] CREATED docs/README.md
```

#### JSON Mode (`--output json`)  
```json
{"path":"src/main.rs","kind":"Modified","timestamp":{"secs_since_epoch":1703932496,"nanos_since_epoch":0},"diff":"--- src/main.rs\n+++ src/main.rs\n@@ -1,3 +1,4 @@\n-fn main() {\n+fn main() -> Result<()> {\n     let cli = Cli::parse();\n+    println!(\"Starting WatchDiff...\");","content_preview":null}
```

#### Compact Mode (`--output compact`)
```bash
$ watchdiff-tui --output compact
M src/main.rs
C docs/README.md  
D old_file.txt
```

**Format Legend:**
- `C` = Created
- `M` = Modified  
- `D` = Deleted
- `V` = Moved

## CLI Options

```
Options:
  -m, --mode <MODE>           File watching mode [auto|native|polling]
      --max-events <N>        Maximum events to store [default: 1000]  
  -v, --verbose              Enable verbose logging
      --no-color             Disable colored output
      --extensions <EXTS>    File extensions to watch (e.g., rs,py,js)
      --ignore <PATTERNS>    Additional patterns to ignore
      --context <N>          Number of diff context lines [default: 3]
      --output <FORMAT>      Output format [tui|json|text|compact]
      --poll-interval <MS>   Polling interval in ms [default: 1000]
```

## Examples

### Development Workflow
```bash
# Watch Rust project files only
watchdiff-tui --extensions rs,toml,md

# Watch with additional ignores
watchdiff-tui --ignore "*.log,tmp/*"

# JSON output piped to file
watchdiff-tui --output json > changes.log
```

### CI/CD Integration
```bash
# Compact format for build scripts  
watchdiff-tui --output compact --no-color
```

## Architecture

WatchDiff is built with modern Rust practices:

- **File Watching**: `notify` crate for cross-platform filesystem events
- **TUI**: `ratatui` with `crossterm` for beautiful terminal interfaces
- **Syntax Highlighting**: `syntect` crate for 25+ programming languages
- **CLI**: `clap` for robust argument parsing
- **Diffing**: `similar` crate for advanced diff algorithms
- **Filtering**: `ignore` crate for `.gitignore` support
- **Async**: `tokio` for non-blocking operations

## Performance

- Handles thousands of files efficiently
- Memory-bounded event history (configurable)
- Native filesystem events (with polling fallback)
- Optimized diff generation
- Background processing threads

## License

MIT License - see LICENSE file for details.

## Contributing

Contributions welcome! Please open issues or pull requests.