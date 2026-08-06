# soyabean 🌱

A minimal yet powerful terminal code IDE, written in Rust. Two dependencies
(`crossterm`, `unicode-width`), a single small binary, no config files.

```
cargo build --release
./target/release/soyabean [FILE ...]
```

## Features

- **Syntax highlighting** for Rust, C, C++, Python, JavaScript, TypeScript,
  Go, Java, JSON, TOML, YAML, HTML, CSS and shell — including multi-line
  block comments
- **Fuzzy file finder** (`Ctrl+P`) over the whole workspace, with smart
  scoring (filename and word-boundary bonuses)
- **Multiple buffers** with instant switching (`Alt+←/→`, `Alt+1..9`)
- **Incremental search** (`Ctrl+F`) with smart case, wrap-around and
  live-as-you-type matching; `F3`/`Shift+F3` to repeat
- **Undo / redo** with keystroke coalescing (`Ctrl+Z` / `Ctrl+Y`)
- **Selections** with `Shift`+movement or mouse click & drag; copy / cut /
  paste, with best-effort mirroring to the system clipboard
- **Smart editing**: auto-indent on Enter, extra indent after `{ ( [ :`,
  tab-stop aware Backspace, block indent/dedent with `Tab`/`Shift+Tab`,
  duplicate line, move line up/down, delete line, select word
- **Mouse support**: click to place the cursor, drag to select, wheel to
  scroll without moving the cursor
- Unicode-aware rendering (wide characters, tabs), CRLF/LF detection and
  preservation, line numbers, current-line highlight, smart Home key

## Keybindings

Press `F1` inside the editor for the full list.

| Key | Action |
| --- | --- |
| `Ctrl+P` / `Ctrl+O` | fuzzy-open file |
| `Ctrl+S` | save (asks for a name if untitled) |
| `Ctrl+N` / `Ctrl+W` | new / close buffer |
| `Ctrl+F`, `F3` | find, find next |
| `Ctrl+G` | go to line |
| `Ctrl+Z` / `Ctrl+Y` | undo / redo |
| `Ctrl+C` / `Ctrl+X` / `Ctrl+V` | copy / cut / paste (whole line when nothing is selected) |
| `Ctrl+D` | select word / duplicate line |
| `Ctrl+K` | delete line |
| `Alt+↑/↓` | move line up / down |
| `Ctrl+Q` | quit (twice to discard unsaved changes) |

## Design

~2,000 lines across five modules:

| Module | Role |
| --- | --- |
| `buffer.rs` | text buffer, cursor, selection, undo, search |
| `syntax.rs` | table-driven per-line tokenizer with block-comment carry-over |
| `finder.rs` | workspace scan + fuzzy subsequence scoring |
| `draw.rs` | diff-free frame renderer with 24-bit colors |
| `editor.rs` | modes, keymap, event loop |

The buffer stores lines as `Vec<String>` with char-indexed cursors; undo is
snapshot-based with time-window coalescing. Highlighting is recomputed only
for visible lines, with a per-line carry state so `/* ... */` spanning
hundreds of lines still renders correctly.
