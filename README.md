<div align="center">

# soyabean

![soyabean logo](logo.png)

A minimal yet powerful **terminal code IDE** with integrated GUI, written in Rust.

[![GitHub release](https://img.shields.io/github/v/release/zenformality/soyabean?style=for-the-badge&label=version&color=2ea44f)](https://github.com/zenformality/soyabean/releases/latest)
[![GitHub License](https://img.shields.io/github/license/zenformality/soyabean?style=for-the-badge&color=555)](LICENSE)
[![Build Status](https://img.shields.io/github/actions/workflow/status/zenformality/soyabean/release.yml?style=for-the-badge&label=build)](https://github.com/zenformality/soyabean/actions/workflows/release.yml)
![Rust](https://shields.io)
[![Platform](https://img.shields.io/badge/platform-Windows%20%7C%20Linux%20%7C%20macOS-lightgrey?style=for-the-badge)](#downloads)
[![Lines of Code](https://img.shields.io/tokei/lines/github/zenformality/soyabean?style=for-the-badge&color=informational)](src)

</div>

---

## Quick Start

### Installation

| Platform | Method |
|----------|--------|
| **Windows** | Download `soyabean-1.0.1-x64-setup.exe` from [Releases](https://github.com/zenformality/soyabean/releases/latest) and run the installer |
| **Linux** | Download `soyabean-1.0.1-x86_64.AppImage`, make executable, and run |
| **macOS** | Build from source (see below) |
| **From Source** | `cargo install --git https://github.com/zenformality/soyabean` |

### Run

```bash
# TUI version
soyabean [FILE...]

# GUI version (modern IDE with file tree, terminal, themes)
soyabean_gui [FILE...]
```

---

## Features

### Modern GUI (`soyabean_gui`)
- **VS Code-style file explorer** with drag & drop, context menus (New File/Folder, Rename, Delete, Reveal)
- **Integrated terminal** (PowerShell on Windows) with OSC 7 cwd tracking and command history
- **Tabbed editing** with file-type icons, current-line highlight, configurable cursor blink
- **Themes**: Zed Dark / Light / Tokyo Night — switch via Command Palette (`Ctrl+Shift+P`)
- **Command palette** for fuzzy command search
- **Breadcrumbs** navigation bar
- **Status bar** with mode, encoding, cursor position, git branch

### Core Editing (TUI + GUI)
- **Syntax highlighting** for 30+ languages (Rust, C/C++, Python, JS/TS, Go, Java, JSON, TOML, YAML, HTML, CSS, Shell, etc.) with multi-line block comments
- **Fuzzy file finder** (`Ctrl+P`) with smart scoring (filename + word-boundary bonuses)
- **Multiple buffers** with instant switching (`Alt+Left/Right`, `Alt+1..9`)
- **Incremental search** (`Ctrl+F`) with smart case, wrap-around, live matching; `F3`/`Shift+F3` repeat
- **Undo / Redo** with keystroke coalescing (`Ctrl+Z` / `Ctrl+Y`)
- **Selections**: mouse drag, `Shift`+movement; copy/cut/paste with system clipboard mirroring
- **Smart editing**: auto-indent on Enter, extra indent after `{ ( [ :`, tab-aware Backspace, block indent/dedent, duplicate/move/delete line, word selection

### Mouse & Unicode
- Click to place cursor, drag to select, wheel to scroll
- Unicode-aware (wide chars, tabs), CRLF/LF detection + preservation
- Line numbers, current-line highlight, smart Home key

### File & Media
- **Image preview** (PNG/JPG/GIF/BMP/WebP/ICO/TIFF) scaled to fit
- **Audio playback** (MP3/WAV/OGG/FLAC/M4A) with play/pause/stop and volume
- **Drag & drop** in file tree to move files/folders

### Integrated Terminal (GUI)
- PowerShell (Windows) / `$SHELL` (Linux/macOS)
- OSC 7 cwd tracking — terminal updates sidebar on `cd`
- Command bar with cwd display, history (Up/Down), Enter/Run to execute
- `Ctrl+` toggles terminal drawer

---

## Keybindings

| Key | Action |
|-----|--------|
| `Ctrl+P` / `Ctrl+O` | Fuzzy open file |
| `Ctrl+S` | Save (prompts if untitled) |
| `Ctrl+N` / `Ctrl+W` | New / Close buffer |
| `Ctrl+F` | Find |
| `F3` / `Shift+F3` | Find next / previous |
| `Ctrl+G` | Go to line |
| `Ctrl+Z` / `Ctrl+Y` | Undo / Redo |
| `Ctrl+C` / `Ctrl+X` / `Ctrl+V` | Copy / Cut / Paste (whole line if no selection) |
| `Ctrl+D` | Select word / Duplicate line |
| `Ctrl+K` | Delete line |
| `Alt+Up/Down` | Move line up / down |
| `Alt+Left/Right` | Switch buffer |
| `Alt+1..9` | Jump to buffer N |
| `Ctrl+Shift+P` | Command Palette |
| `Ctrl+\`` | Toggle terminal |
| `Ctrl+Q` | Quit (twice to discard unsaved) |
| `F1` | Show full shortcuts list |

---

## Downloads

Latest release: **[v1.0.1](https://github.com/zenformality/soyabean/releases/tag/v1.0.1)**

| Platform | Artifact | SHA256 |
|----------|----------|--------|
| Windows (installer) | `soyabean-1.0.1-x86_64-setup.exe` | `*.sha256` |
| Windows (portable) | `soyabean-1.0.1-x86_64.zip` | `*.sha256` |
| Linux (AppImage) | `soyabean-1.0.1-x86_64.AppImage` | `*.sha256` |
| Linux (tarball) | `soyabean-1.0.1-x86_64.tar.xz` | `*.sha256` |
| Source | `source.tar.gz` | `*.sha256` |

All artifacts include `sha256.sum` for verification.

---

## Build from Source

```bash
# Prerequisites: Rust 1.97+, Git
git clone https://github.com/zenformality/soyabean
cd soyabean

# TUI only (minimal deps)
cargo build --release --bin soyabean

# GUI (needs ALSA on Linux: sudo apt install libasound2-dev)
cargo build --release --bin soyabean_gui
```

Binaries land in `target/release/`.

---

## Architecture

~2,500 lines across modules:

| Module | Role |
|--------|------|
| `buffer.rs` | Text buffer, cursor, selection, undo/redo, search |
| `syntax.rs` | Table-driven per-line tokenizer with block-comment carry-over |
| `finder.rs` | Workspace scan + fuzzy subsequence scoring |
| `draw.rs` | Diff-free frame renderer with 24-bit colors |
| `editor.rs` | Modes, keymap, event loop |
| `media_view.rs` | Image preview + audio player (rodio) |
| `file_tree.rs` | Modern file explorer with drag & drop |
| `term.rs` | Embedded PTY terminal with ANSI parser |
| `editor_view.rs` | Syntax-highlighted editor pane |
| `soyabean_gui.rs` | Main GUI application |

**Dependencies**: `crossterm`, `eframe/egui`, `image`, `portable-pty`, `rodio`, `unicode-width`

---

## License

MIT — see [LICENSE](LICENSE) for details.

---

<div align="center">
<sub>Built with Rust · Powered by egui · Inspired by Zed & VS Code</sub>
</div>
