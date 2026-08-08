//! Text buffer: lines of text plus cursor, selection, undo/redo and edits.
//! Cursor `cx` is a *char* index into the line; conversions to byte indices
//! and visual (display) columns happen at the edges.

use std::fs;
use std::io;
use std::path::PathBuf;
use std::time::Instant;

use unicode_width::UnicodeWidthChar;

use crate::syntax::{self, Language, LineState};

pub const TAB_STOP: usize = 4;

fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// Byte index for a char index within a line.
pub fn bidx(line: &str, cx: usize) -> usize {
    line.char_indices()
        .nth(cx)
        .map(|(i, _)| i)
        .unwrap_or(line.len())
}

pub fn charlen(line: &str) -> usize {
    line.chars().count()
}

pub fn ch_width(c: char, vcol: usize) -> usize {
    if c == '\t' {
        TAB_STOP - vcol % TAB_STOP
    } else {
        UnicodeWidthChar::width(c).unwrap_or(1).max(1)
    }
}

/// Display column of char index `cx` in `line`.
pub fn visual_col(line: &str, cx: usize) -> usize {
    let mut v = 0;
    for (i, c) in line.chars().enumerate() {
        if i >= cx {
            break;
        }
        v += ch_width(c, v);
    }
    v
}

/// Char index at (or containing) display column `target`.
pub fn cx_at_vcol(line: &str, target: usize) -> usize {
    let mut v = 0;
    for (i, c) in line.chars().enumerate() {
        let w = ch_width(c, v);
        if target < v + w {
            return i;
        }
        v += w;
    }
    charlen(line)
}

struct Snapshot {
    lines: Vec<String>,
    cx: usize,
    cy: usize,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum EditKind {
    Insert,
    Delete,
    Other,
}

pub struct Buffer {
    pub lines: Vec<String>,
    pub path: Option<PathBuf>,
    pub dirty: bool,
    pub is_scratch: bool,
    pub scratch_name: String,
    pub crlf: bool,
    pub cx: usize,
    pub cy: usize,
    goal: usize,
    pub row_off: usize,
    pub col_off: usize,
    pub anchor: Option<(usize, usize)>, // (y, x) selection anchor
    pub lang: &'static Language,
    pub line_states: Vec<LineState>,
    pub syntax_dirty: bool,
    undo: Vec<Snapshot>,
    redo: Vec<Snapshot>,
    last_kind: EditKind,
    last_time: Option<Instant>,
}

impl Buffer {
    fn base() -> Self {
        Buffer {
            lines: vec![String::new()],
            path: None,
            dirty: false,
            is_scratch: false,
            scratch_name: String::new(),
            crlf: false,
            cx: 0,
            cy: 0,
            goal: 0,
            row_off: 0,
            col_off: 0,
            anchor: None,
            lang: &syntax::PLAIN,
            line_states: Vec::new(),
            syntax_dirty: true,
            undo: Vec::new(),
            redo: Vec::new(),
            last_kind: EditKind::Other,
            last_time: None,
        }
    }

    pub fn empty() -> Self {
        Self::base()
    }

    pub fn scratch(name: &str, text: &str) -> Self {
        let mut b = Self::base();
        b.is_scratch = true;
        b.scratch_name = name.to_string();
        b.lines = text
            .replace('\r', "")
            .split('\n')
            .map(String::from)
            .collect();
        if b.lines.is_empty() {
            b.lines.push(String::new());
        }
        b
    }

    /// Open a file; a nonexistent path yields an empty buffer that will be
    /// created on save.
    pub fn open(path: PathBuf) -> io::Result<Self> {
        let mut b = Self::base();
        match fs::read(&path) {
            Ok(bytes) => {
                if bytes.contains(&0) {
                    return Err(io::Error::new(io::ErrorKind::InvalidData, "binary file"));
                }
                let text = String::from_utf8_lossy(&bytes);
                b.crlf = text.contains("\r\n");
                let mut lines: Vec<String> = text
                    .replace('\r', "")
                    .split('\n')
                    .map(String::from)
                    .collect();
                if lines.len() > 1 && lines.last().is_some_and(|l| l.is_empty()) {
                    lines.pop();
                }
                if lines.is_empty() {
                    lines.push(String::new());
                }
                b.lines = lines;
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => return Err(e),
        }
        b.lang = syntax::detect(&path);
        b.path = Some(path);
        Ok(b)
    }

    pub fn save(&mut self) -> io::Result<usize> {
        let path = self
            .path
            .clone()
            .ok_or_else(|| io::Error::other("no file name"))?;
        let sep = if self.crlf { "\r\n" } else { "\n" };
        let mut content = self.lines.join(sep);
        content.push_str(sep);
        fs::write(&path, &content)?;
        self.dirty = false;
        self.is_scratch = false;
        self.lang = syntax::detect(&path);
        self.syntax_dirty = true;
        Ok(content.len())
    }

    pub fn display_name(&self) -> String {
        if self.is_scratch && !self.scratch_name.is_empty() {
            return self.scratch_name.clone();
        }
        match &self.path {
            Some(p) => p
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| p.to_string_lossy().to_string()),
            None => "untitled".to_string(),
        }
    }

    pub fn cur_line(&self) -> &str {
        &self.lines[self.cy]
    }

    pub fn ensure_syntax(&mut self) {
        if !self.syntax_dirty && self.line_states.len() == self.lines.len() {
            return;
        }
        let mut states = Vec::with_capacity(self.lines.len());
        let mut st = LineState::default();
        for line in &self.lines {
            states.push(st);
            st = syntax::highlight_line(line, self.lang, st).1;
        }
        self.line_states = states;
        self.syntax_dirty = false;
    }

    // ---- undo/redo ------------------------------------------------------

    fn snapshot(&self) -> Snapshot {
        Snapshot {
            lines: self.lines.clone(),
            cx: self.cx,
            cy: self.cy,
        }
    }

    fn push_undo(&mut self, kind: EditKind) {
        let now = Instant::now();
        let coalesce = kind != EditKind::Other
            && kind == self.last_kind
            && self
                .last_time
                .is_some_and(|t| now.duration_since(t).as_millis() < 700);
        if !coalesce {
            self.undo.push(self.snapshot());
            if self.undo.len() > 300 {
                self.undo.remove(0);
            }
        }
        self.redo.clear();
        self.last_kind = kind;
        self.last_time = Some(now);
        self.dirty = true;
        self.syntax_dirty = true;
    }

    fn restore(&mut self, s: Snapshot) {
        self.lines = s.lines;
        self.cy = s.cy.min(self.lines.len() - 1);
        self.cx = s.cx.min(charlen(&self.lines[self.cy]));
        self.goal = self.cx;
        self.anchor = None;
        self.dirty = true;
        self.syntax_dirty = true;
        self.last_kind = EditKind::Other;
        self.last_time = None;
    }

    pub fn undo(&mut self) -> bool {
        match self.undo.pop() {
            Some(s) => {
                self.redo.push(self.snapshot());
                self.restore(s);
                true
            }
            None => false,
        }
    }

    pub fn redo(&mut self) -> bool {
        match self.redo.pop() {
            Some(s) => {
                self.undo.push(self.snapshot());
                self.restore(s);
                true
            }
            None => false,
        }
    }

    // ---- selection ------------------------------------------------------

    /// Ordered selection range ((sy,sx),(ey,ex)), if any and non-empty.
    pub fn sel_range(&self) -> Option<((usize, usize), (usize, usize))> {
        let (ay, ax) = self.anchor?;
        let a = (ay, ax);
        let c = (self.cy, self.cx);
        match a.cmp(&c) {
            std::cmp::Ordering::Less => Some((a, c)),
            std::cmp::Ordering::Greater => Some((c, a)),
            std::cmp::Ordering::Equal => None,
        }
    }

    pub fn selected_text(&self) -> Option<String> {
        let ((sy, sx), (ey, ex)) = self.sel_range()?;
        if sy == ey {
            let line = &self.lines[sy];
            return Some(line[bidx(line, sx)..bidx(line, ex)].to_string());
        }
        let mut out = String::new();
        out.push_str(&self.lines[sy][bidx(&self.lines[sy], sx)..]);
        for y in sy + 1..ey {
            out.push('\n');
            out.push_str(&self.lines[y]);
        }
        out.push('\n');
        out.push_str(&self.lines[ey][..bidx(&self.lines[ey], ex)]);
        Some(out)
    }

    fn delete_sel_raw(&mut self) {
        let Some(((sy, sx), (ey, ex))) = self.sel_range() else {
            self.anchor = None;
            return;
        };
        if sy == ey {
            let line = &mut self.lines[sy];
            let (b0, b1) = (bidx(line, sx), bidx(line, ex));
            line.replace_range(b0..b1, "");
        } else {
            let head = self.lines[sy][..bidx(&self.lines[sy], sx)].to_string();
            let tail = self.lines[ey][bidx(&self.lines[ey], ex)..].to_string();
            self.lines.splice(sy..=ey, [head + &tail]);
        }
        self.cy = sy;
        self.cx = sx;
        self.goal = sx;
        self.anchor = None;
    }

    pub fn delete_selection(&mut self) {
        if self.sel_range().is_some() {
            self.push_undo(EditKind::Other);
            self.delete_sel_raw();
        }
    }

    pub fn select_all(&mut self) {
        self.anchor = Some((0, 0));
        self.cy = self.lines.len() - 1;
        self.cx = charlen(&self.lines[self.cy]);
        self.goal = self.cx;
    }

    // ---- movement -------------------------------------------------------

    fn begin_move(&mut self, sel: bool) {
        if sel {
            if self.anchor.is_none() {
                self.anchor = Some((self.cy, self.cx));
            }
        } else {
            self.anchor = None;
        }
    }

    pub fn set_cursor(&mut self, cy: usize, cx: usize, sel: bool) {
        self.begin_move(sel);
        self.cy = cy.min(self.lines.len() - 1);
        self.cx = cx.min(charlen(&self.lines[self.cy]));
        self.goal = self.cx;
    }

    pub fn left(&mut self, sel: bool) {
        self.begin_move(sel);
        if self.cx > 0 {
            self.cx -= 1;
        } else if self.cy > 0 {
            self.cy -= 1;
            self.cx = charlen(&self.lines[self.cy]);
        }
        self.goal = self.cx;
    }

    pub fn right(&mut self, sel: bool) {
        self.begin_move(sel);
        if self.cx < charlen(&self.lines[self.cy]) {
            self.cx += 1;
        } else if self.cy + 1 < self.lines.len() {
            self.cy += 1;
            self.cx = 0;
        }
        self.goal = self.cx;
    }

    pub fn up(&mut self, sel: bool) {
        self.begin_move(sel);
        if self.cy > 0 {
            self.cy -= 1;
            self.cx = self.goal.min(charlen(&self.lines[self.cy]));
        } else {
            self.cx = 0;
            self.goal = 0;
        }
    }

    pub fn down(&mut self, sel: bool) {
        self.begin_move(sel);
        if self.cy + 1 < self.lines.len() {
            self.cy += 1;
            self.cx = self.goal.min(charlen(&self.lines[self.cy]));
        } else {
            self.cx = charlen(&self.lines[self.cy]);
            self.goal = self.cx;
        }
    }

    pub fn home(&mut self, sel: bool) {
        self.begin_move(sel);
        // Smart home: toggle between first non-blank char and column 0.
        let first = self.lines[self.cy]
            .chars()
            .position(|c| !c.is_whitespace())
            .unwrap_or(0);
        self.cx = if self.cx == first { 0 } else { first };
        self.goal = self.cx;
    }

    pub fn end(&mut self, sel: bool) {
        self.begin_move(sel);
        self.cx = charlen(&self.lines[self.cy]);
        self.goal = self.cx;
    }

    pub fn doc_start(&mut self, sel: bool) {
        self.begin_move(sel);
        self.cy = 0;
        self.cx = 0;
        self.goal = 0;
    }

    pub fn doc_end(&mut self, sel: bool) {
        self.begin_move(sel);
        self.cy = self.lines.len() - 1;
        self.cx = charlen(&self.lines[self.cy]);
        self.goal = self.cx;
    }

    pub fn page_up(&mut self, page: usize, sel: bool) {
        self.begin_move(sel);
        self.cy = self.cy.saturating_sub(page);
        self.cx = self.goal.min(charlen(&self.lines[self.cy]));
        self.row_off = self.row_off.saturating_sub(page);
    }

    pub fn page_down(&mut self, page: usize, sel: bool) {
        self.begin_move(sel);
        self.cy = (self.cy + page).min(self.lines.len() - 1);
        self.cx = self.goal.min(charlen(&self.lines[self.cy]));
        self.row_off += page;
    }

    pub fn word_left(&mut self, sel: bool) {
        self.begin_move(sel);
        if self.cx == 0 {
            if self.cy > 0 {
                self.cy -= 1;
                self.cx = charlen(&self.lines[self.cy]);
            }
        } else {
            let chars: Vec<char> = self.lines[self.cy].chars().collect();
            let mut i = self.cx;
            while i > 0 && chars[i - 1].is_whitespace() {
                i -= 1;
            }
            if i > 0 {
                if is_word(chars[i - 1]) {
                    while i > 0 && is_word(chars[i - 1]) {
                        i -= 1;
                    }
                } else {
                    while i > 0 && !is_word(chars[i - 1]) && !chars[i - 1].is_whitespace() {
                        i -= 1;
                    }
                }
            }
            self.cx = i;
        }
        self.goal = self.cx;
    }

    pub fn word_right(&mut self, sel: bool) {
        self.begin_move(sel);
        let chars: Vec<char> = self.lines[self.cy].chars().collect();
        if self.cx >= chars.len() {
            if self.cy + 1 < self.lines.len() {
                self.cy += 1;
                self.cx = 0;
            }
        } else {
            let mut i = self.cx;
            while i < chars.len() && chars[i].is_whitespace() {
                i += 1;
            }
            if i < chars.len() {
                if is_word(chars[i]) {
                    while i < chars.len() && is_word(chars[i]) {
                        i += 1;
                    }
                } else {
                    while i < chars.len() && !is_word(chars[i]) && !chars[i].is_whitespace() {
                        i += 1;
                    }
                }
            }
            self.cx = i;
        }
        self.goal = self.cx;
    }

    pub fn select_word(&mut self) {
        let chars: Vec<char> = self.lines[self.cy].chars().collect();
        if chars.is_empty() {
            return;
        }
        let i = self.cx.min(chars.len() - 1);
        if !is_word(chars[i]) {
            return;
        }
        let mut s = i;
        while s > 0 && is_word(chars[s - 1]) {
            s -= 1;
        }
        let mut e = i;
        while e < chars.len() && is_word(chars[e]) {
            e += 1;
        }
        self.anchor = Some((self.cy, s));
        self.cx = e;
        self.goal = e;
    }

    // ---- editing --------------------------------------------------------

    pub fn insert_char(&mut self, c: char) {
        self.push_undo(EditKind::Insert);
        self.delete_sel_raw();
        let chars: Vec<char> = self.lines[self.cy].chars().collect();
        let at = self.cx;
        // Typing a closing char directly after its opener skips over it.
        if matches!(c, ')' | ']' | '}' | '"' | '\'' | '`') && chars.get(at) == Some(&c) {
            self.cx += 1;
            self.goal = self.cx;
            return;
        }
        let closing = match c {
            '(' => Some(')'),
            '[' => Some(']'),
            '{' => Some('}'),
            '"' | '\'' | '`' => Some(c),
            _ => None,
        };
        // Quotes don't auto-close right after a word char (apostrophes, prose).
        let quote = matches!(c, '"' | '\'' | '`');
        if quote && at > 0 && chars[at - 1].is_alphanumeric() {
            let bi = bidx(&self.lines[self.cy], at);
            self.lines[self.cy].insert(bi, c);
            self.cx += 1;
            self.goal = self.cx;
            return;
        }
        let bi = bidx(&self.lines[self.cy], at);
        if let Some(cl) = closing {
            self.lines[self.cy].insert(bi, c);
            let bi2 = bidx(&self.lines[self.cy], at + 1);
            self.lines[self.cy].insert(bi2, cl);
            self.cx += 1;
        } else {
            self.lines[self.cy].insert(bi, c);
            self.cx += 1;
        }
        self.goal = self.cx;
    }

    pub fn insert_newline(&mut self) {
        self.push_undo(EditKind::Other);
        self.delete_sel_raw();
        let bi = bidx(&self.lines[self.cy], self.cx);
        let rest = self.lines[self.cy][bi..].to_string();
        self.lines[self.cy].truncate(bi);
        let before = &self.lines[self.cy];
        let mut indent: String = before
            .chars()
            .take_while(|c| *c == ' ' || *c == '\t')
            .collect();
        let trimmed = before.trim_end();
        if trimmed.ends_with(['{', '(', '[', ':']) {
            indent.push_str(&" ".repeat(TAB_STOP));
        }
        let new_cx = charlen(&indent);
        self.lines.insert(self.cy + 1, indent + &rest);
        self.cy += 1;
        self.cx = new_cx;
        self.goal = new_cx;
    }

    pub fn insert_tab(&mut self) {
        if let Some(((sy, _), (ey, _))) = self.sel_range() {
            if sy != ey {
                self.indent_lines(true);
                return;
            }
        }
        self.push_undo(EditKind::Insert);
        self.delete_sel_raw();
        let n = TAB_STOP - self.cx % TAB_STOP;
        let bi = bidx(&self.lines[self.cy], self.cx);
        self.lines[self.cy].insert_str(bi, &" ".repeat(n));
        self.cx += n;
        self.goal = self.cx;
    }

    pub fn indent_lines(&mut self, add: bool) {
        self.push_undo(EditKind::Other);
        let (sy, ey) = match self.sel_range() {
            Some(((sy, _), (ey, ex))) => (sy, if ex == 0 && ey > sy { ey - 1 } else { ey }),
            None => (self.cy, self.cy),
        };
        for y in sy..=ey {
            if add {
                if !self.lines[y].is_empty() {
                    self.lines[y].insert_str(0, &" ".repeat(TAB_STOP));
                }
            } else {
                let strip = self.lines[y]
                    .chars()
                    .take(TAB_STOP)
                    .take_while(|c| *c == ' ')
                    .count();
                self.lines[y].replace_range(..strip, "");
                if y == self.cy {
                    self.cx = self.cx.saturating_sub(strip);
                }
                if let Some((ay, ax)) = self.anchor {
                    if ay == y {
                        self.anchor = Some((ay, ax.saturating_sub(strip)));
                    }
                }
            }
        }
        if add {
            if !self.lines[self.cy].is_empty() {
                self.cx += TAB_STOP;
            }
            if let Some((ay, ax)) = self.anchor {
                if !self.lines[ay].is_empty() {
                    self.anchor = Some((ay, ax + TAB_STOP));
                }
            }
        }
        self.cx = self.cx.min(charlen(&self.lines[self.cy]));
        self.goal = self.cx;
    }

    pub fn backspace(&mut self) {
        if self.sel_range().is_some() {
            self.push_undo(EditKind::Other);
            self.delete_sel_raw();
            return;
        }
        self.anchor = None;
        if self.cx > 0 {
            self.push_undo(EditKind::Delete);
            let line = &self.lines[self.cy];
            let before = &line[..bidx(line, self.cx)];
            // Whitespace-only prefix: delete back to the previous tab stop.
            let n = if !before.is_empty() && before.chars().all(|c| c == ' ') {
                let r = self.cx % TAB_STOP;
                if r == 0 {
                    TAB_STOP
                } else {
                    r
                }
            } else {
                1
            };
            for _ in 0..n {
                let b = bidx(&self.lines[self.cy], self.cx - 1);
                self.lines[self.cy].remove(b);
                self.cx -= 1;
            }
        } else if self.cy > 0 {
            self.push_undo(EditKind::Other);
            let cur = self.lines.remove(self.cy);
            self.cy -= 1;
            self.cx = charlen(&self.lines[self.cy]);
            self.lines[self.cy].push_str(&cur);
        }
        self.goal = self.cx;
    }

    pub fn delete_forward(&mut self) {
        if self.sel_range().is_some() {
            self.push_undo(EditKind::Other);
            self.delete_sel_raw();
            return;
        }
        self.anchor = None;
        if self.cx < charlen(&self.lines[self.cy]) {
            self.push_undo(EditKind::Delete);
            let b = bidx(&self.lines[self.cy], self.cx);
            self.lines[self.cy].remove(b);
        } else if self.cy + 1 < self.lines.len() {
            self.push_undo(EditKind::Other);
            let next = self.lines.remove(self.cy + 1);
            self.lines[self.cy].push_str(&next);
        }
    }

    /// Insert (possibly multi-line) text at the cursor, replacing selection.
    pub fn insert_text(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }
        self.push_undo(EditKind::Other);
        self.delete_sel_raw();
        let text = text.replace("\r\n", "\n").replace('\r', "\n");
        let parts: Vec<&str> = text.split('\n').collect();
        if parts.len() == 1 {
            let bi = bidx(&self.lines[self.cy], self.cx);
            self.lines[self.cy].insert_str(bi, parts[0]);
            self.cx += charlen(parts[0]);
        } else {
            let bi = bidx(&self.lines[self.cy], self.cx);
            let tail = self.lines[self.cy][bi..].to_string();
            self.lines[self.cy].truncate(bi);
            self.lines[self.cy].push_str(parts[0]);
            let last = parts[parts.len() - 1];
            let mut new_lines: Vec<String> = parts[1..parts.len() - 1]
                .iter()
                .map(|s| s.to_string())
                .collect();
            new_lines.push(last.to_string() + &tail);
            let at = self.cy + 1;
            self.lines.splice(at..at, new_lines);
            self.cy += parts.len() - 1;
            self.cx = charlen(last);
        }
        self.goal = self.cx;
    }

    pub fn delete_line(&mut self) {
        self.push_undo(EditKind::Other);
        self.anchor = None;
        if self.lines.len() == 1 {
            self.lines[0].clear();
            self.cx = 0;
        } else {
            self.lines.remove(self.cy);
            if self.cy >= self.lines.len() {
                self.cy = self.lines.len() - 1;
            }
            self.cx = self.cx.min(charlen(&self.lines[self.cy]));
        }
        self.goal = self.cx;
    }

    pub fn duplicate_line(&mut self) {
        self.push_undo(EditKind::Other);
        self.anchor = None;
        let line = self.lines[self.cy].clone();
        self.lines.insert(self.cy + 1, line);
        self.cy += 1;
    }

    pub fn move_line(&mut self, up: bool) {
        if (up && self.cy == 0) || (!up && self.cy + 1 >= self.lines.len()) {
            return;
        }
        self.push_undo(EditKind::Other);
        self.anchor = None;
        let other = if up { self.cy - 1 } else { self.cy + 1 };
        self.lines.swap(self.cy, other);
        self.cy = other;
    }

    // ---- word delete ----------------------------------------------------

    pub fn delete_word_back(&mut self) {
        if self.sel_range().is_some() {
            self.delete_selection();
            return;
        }
        self.anchor = None;
        if self.cx == 0 {
            if self.cy > 0 {
                self.push_undo(EditKind::Delete);
                let cur = self.lines.remove(self.cy);
                self.cy -= 1;
                self.cx = charlen(&self.lines[self.cy]);
                self.lines[self.cy].push_str(&cur);
            }
            self.goal = self.cx;
            return;
        }
        let chars: Vec<char> = self.lines[self.cy].chars().collect();
        let mut i = self.cx;
        while i > 0 && chars[i - 1].is_whitespace() {
            i -= 1;
        }
        if i > 0 {
            if is_word(chars[i - 1]) {
                while i > 0 && is_word(chars[i - 1]) {
                    i -= 1;
                }
            } else {
                while i > 0 && !is_word(chars[i - 1]) && !chars[i - 1].is_whitespace() {
                    i -= 1;
                }
            }
        }
        self.push_undo(EditKind::Delete);
        let b0 = bidx(&self.lines[self.cy], i);
        let b1 = bidx(&self.lines[self.cy], self.cx);
        self.lines[self.cy].replace_range(b0..b1, "");
        self.cx = i;
        self.goal = self.cx;
    }

    pub fn delete_word_fwd(&mut self) {
        if self.sel_range().is_some() {
            self.delete_selection();
            return;
        }
        self.anchor = None;
        let chars: Vec<char> = self.lines[self.cy].chars().collect();
        let n = chars.len();
        if self.cx >= n {
            if self.cy + 1 < self.lines.len() {
                self.push_undo(EditKind::Delete);
                let next = self.lines.remove(self.cy + 1);
                self.lines[self.cy].push_str(&next);
            }
            return;
        }
        let mut i = self.cx;
        while i < n && chars[i].is_whitespace() {
            i += 1;
        }
        if i < n {
            if is_word(chars[i]) {
                while i < n && is_word(chars[i]) {
                    i += 1;
                }
            } else {
                while i < n && !is_word(chars[i]) && !chars[i].is_whitespace() {
                    i += 1;
                }
            }
        }
        self.push_undo(EditKind::Delete);
        let b0 = bidx(&self.lines[self.cy], self.cx);
        let b1 = bidx(&self.lines[self.cy], i);
        self.lines[self.cy].replace_range(b0..b1, "");
    }

    // ---- comments -------------------------------------------------------

    /// Toggle `//`, `#`, … line comments on the current line or selection.
    /// Returns false when this language has no line-comment prefix.
    pub fn toggle_comment(&mut self) -> bool {
        let prefix = self.lang.line_comment;
        if prefix.is_empty() {
            return false;
        }
        let (sy, ey) = match self.sel_range() {
            Some(((sy, _), (ey, ex))) => (sy, if ex == 0 && ey > sy { ey - 1 } else { ey }),
            None => (self.cy, self.cy),
        };
        let all_commented = (sy..=ey).all(|y| {
            let t = self.lines[y].trim_start();
            t.is_empty() || t.starts_with(prefix)
        });
        let plen = prefix.chars().count();
        self.push_undo(EditKind::Other);
        for y in sy..=ey {
            let lead = self.lines[y].len() - self.lines[y].trim_start().len();
            if all_commented {
                let t = &self.lines[y][lead..];
                if let Some(mut rest) = t.strip_prefix(prefix) {
                    let dropped = rest.starts_with(' ');
                    if dropped {
                        rest = &rest[1..];
                    }
                    let mut s = self.lines[y][..lead].to_string();
                    s.push_str(rest);
                    self.lines[y] = s;
                    let removed = plen + if dropped { 1 } else { 0 };
                    if y == self.cy && self.cx >= lead {
                        self.cx = self.cx.saturating_sub(removed);
                    }
                    if let Some((ay, ax)) = self.anchor {
                        if ay == y && ax >= lead {
                            self.anchor = Some((ay, ax.saturating_sub(removed)));
                        }
                    }
                }
            } else if !self.lines[y].trim_start().is_empty() {
                self.lines[y].insert_str(lead, prefix);
                let after = &self.lines[y][lead + plen..];
                if !after.starts_with(' ') {
                    self.lines[y].insert_str(lead + plen, " ");
                }
                let added = plen + 1;
                if y == self.cy && self.cx >= lead {
                    self.cx += added;
                }
                if let Some((ay, ax)) = self.anchor {
                    if ay == y && ax >= lead {
                        self.anchor = Some((ay, ax + added));
                    }
                }
            }
        }
        self.cx = self.cx.min(charlen(&self.lines[self.cy]));
        self.goal = self.cx;
        true
    }

    // ---- replace --------------------------------------------------------

    /// Replace every (smart-case) match of `q` with `rep` as one undo step.
    pub fn replace_all(&mut self, q: &str, rep: &str) -> usize {
        if q.is_empty() {
            return 0;
        }
        let ci = !q.chars().any(|c| c.is_uppercase());
        let qn = if ci { q.to_lowercase() } else { q.to_string() };

        // First pass: collect edits so the whole operation is a single undo.
        let mut edits: Vec<(usize, Vec<(usize, usize)>)> = Vec::new();
        for (y, line) in self.lines.iter().enumerate() {
            let hay = if ci {
                line.to_lowercase()
            } else {
                line.to_string()
            };
            let mut ranges = Vec::new();
            let mut from = 0usize;
            while let Some(rel) = hay[from..].find(&qn) {
                let b = from + rel;
                let e = b + qn.len();
                ranges.push((b, e));
                from = e;
            }
            if !ranges.is_empty() {
                edits.push((y, ranges));
            }
        }
        let count: usize = edits.iter().map(|(_, r)| r.len()).sum();
        if count == 0 {
            return 0;
        }
        self.push_undo(EditKind::Other);
        for (y, ranges) in edits {
            let line = &self.lines[y];
            let mut new = String::with_capacity(line.len());
            let mut last = 0usize;
            for (b, e) in ranges {
                new.push_str(&line[last..b]);
                new.push_str(rep);
                last = e;
            }
            new.push_str(&line[last..]);
            self.lines[y] = new;
        }
        self.dirty = true;
        self.syntax_dirty = true;
        count
    }

    /// Number of (smart-case) matches of `q` across the whole buffer.
    pub fn count_matches(&self, q: &str) -> usize {
        if q.is_empty() {
            return 0;
        }
        let ci = !q.chars().any(|c| c.is_uppercase());
        let qn = if ci { q.to_lowercase() } else { q.to_string() };
        let mut total = 0usize;
        for line in &self.lines {
            let hay = if ci {
                line.to_lowercase()
            } else {
                line.to_string()
            };
            let mut from = 0usize;
            while let Some(rel) = hay[from..].find(&qn) {
                total += 1;
                from += rel + qn.len();
            }
        }
        total
    }

    // ---- search ---------------------------------------------------------

    /// Find `q` starting from `(from.0, from.1)`; wraps around. Returns
    /// (line, char-start, char-len). Case-insensitive unless the query has
    /// uppercase letters (smart case).
    pub fn find(
        &self,
        q: &str,
        from: (usize, usize),
        forward: bool,
    ) -> Option<(usize, usize, usize)> {
        if q.is_empty() {
            return None;
        }
        let ci = !q.chars().any(|c| c.is_uppercase());
        let qn = if ci { q.to_lowercase() } else { q.to_string() };
        let qlen = charlen(q);
        let n = self.lines.len();

        let matches_in = |y: usize| -> Vec<usize> {
            let hay = if ci {
                self.lines[y].to_lowercase()
            } else {
                self.lines[y].clone()
            };
            hay.match_indices(&qn)
                .map(|(b, _)| hay[..b].chars().count())
                .collect()
        };

        for step in 0..=n {
            let y = if forward {
                (from.0 + step) % n
            } else {
                (from.0 + n - step % n) % n
            };
            let ms = matches_in(y);
            let it: Box<dyn Iterator<Item = &usize>> = if forward {
                Box::new(ms.iter())
            } else {
                Box::new(ms.iter().rev())
            };
            for &x in it {
                if step == 0 {
                    if forward && x < from.1 {
                        continue;
                    }
                    if !forward && x >= from.1 {
                        continue;
                    }
                }
                return Some((y, x, qlen));
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn buf(text: &str) -> Buffer {
        let mut b = Buffer::empty();
        b.lines = text.split('\n').map(String::from).collect();
        b
    }

    #[test]
    fn insert_and_undo() {
        let mut b = Buffer::empty();
        for c in "hello".chars() {
            b.insert_char(c);
        }
        assert_eq!(b.lines[0], "hello");
        assert!(b.dirty);
        assert!(b.undo());
        assert_eq!(b.lines[0], "");
        assert!(b.redo());
        assert_eq!(b.lines[0], "hello");
    }

    #[test]
    fn newline_auto_indent() {
        let mut b = buf("    fn main() {");
        b.set_cursor(0, charlen("    fn main() {"), false);
        b.insert_newline();
        assert_eq!(b.cy, 1);
        assert_eq!(b.lines[1], "        ");
        assert_eq!(b.cx, 8);
    }

    #[test]
    fn selection_delete_multiline() {
        let mut b = buf("abc\ndef\nghi");
        b.set_cursor(0, 1, false);
        b.set_cursor(2, 2, true); // select from (0,1) to (2,2)
        assert_eq!(b.selected_text().unwrap(), "bc\ndef\ngh");
        b.delete_selection();
        assert_eq!(b.lines, vec!["ai"]);
        assert_eq!((b.cy, b.cx), (0, 1));
    }

    #[test]
    fn insert_multiline_text() {
        let mut b = buf("ab");
        b.set_cursor(0, 1, false);
        b.insert_text("1\n2\n3");
        assert_eq!(b.lines, vec!["a1", "2", "3b"]);
        assert_eq!((b.cy, b.cx), (2, 1));
    }

    #[test]
    fn backspace_joins_lines_and_dedents() {
        let mut b = buf("ab\ncd");
        b.set_cursor(1, 0, false);
        b.backspace();
        assert_eq!(b.lines, vec!["abcd"]);
        let mut b = buf("        x");
        b.set_cursor(0, 8, false);
        b.backspace();
        assert_eq!(b.lines[0], "    x");
        assert_eq!(b.cx, 4);
    }

    #[test]
    fn find_wraps_and_smartcase() {
        let b = buf("Foo bar\nbaz foo");
        // lowercase query: case-insensitive, finds Foo first from start
        assert_eq!(b.find("foo", (0, 0), true), Some((0, 0, 3)));
        // from after first match it wraps to line 1
        assert_eq!(b.find("foo", (0, 1), true), Some((1, 4, 3)));
        // uppercase query is exact
        assert_eq!(b.find("Foo", (0, 1), true), Some((0, 0, 3)));
        // backward search
        assert_eq!(b.find("foo", (1, 4), false), Some((0, 0, 3)));
        assert_eq!(b.find("nope", (0, 0), true), None);
    }

    #[test]
    fn word_motion_and_select_word() {
        let mut b = buf("let foo_bar = 42;");
        b.set_cursor(0, 0, false);
        b.word_right(false);
        assert_eq!(b.cx, 3);
        b.word_right(false);
        assert_eq!(b.cx, 11); // end of foo_bar
        b.set_cursor(0, 5, false);
        b.select_word();
        assert_eq!(b.selected_text().unwrap(), "foo_bar");
    }

    #[test]
    fn unicode_cursor_and_visual_col() {
        let mut b = buf("aあb");
        b.set_cursor(0, 3, false);
        assert_eq!(visual_col(&b.lines[0], 3), 4); // あ is double-width
        b.backspace();
        assert_eq!(b.lines[0], "aあ");
        assert_eq!(cx_at_vcol("aあb", 2), 1); // clicking inside あ
        assert_eq!(cx_at_vcol("aあb", 3), 2);
    }

    #[test]
    fn indent_dedent_selection() {
        let mut b = buf("a\nb");
        b.set_cursor(0, 0, false);
        b.set_cursor(1, 1, true);
        b.indent_lines(true);
        assert_eq!(b.lines, vec!["    a", "    b"]);
        b.indent_lines(false);
        assert_eq!(b.lines, vec!["a", "b"]);
    }

    #[test]
    fn auto_close_pairs_and_skip_over() {
        let mut b = buf("ab");
        b.set_cursor(0, 1, false);
        b.insert_char('(');
        assert_eq!(b.lines[0], "a()b"); // auto-closed
        assert_eq!(b.cx, 2); // cursor between the parens
        b.insert_char(')');
        assert_eq!(b.lines[0], "a()b"); // skipped over the auto-closed ')'
        assert_eq!(b.cx, 3);
        b.insert_char('"');
        assert_eq!(b.lines[0], "a()\"\"b");
        assert_eq!(b.cx, 4);
        b.insert_char('"');
        assert_eq!(b.lines[0], "a()\"\"b");
        assert_eq!(b.cx, 5);
        // Quote after a word char does not auto-close (apostrophes).
        let mut b = buf("don");
        b.set_cursor(0, 3, false);
        b.insert_char('\'');
        assert_eq!(b.lines[0], "don'");
    }

    #[test]
    fn delete_word_back_and_fwd() {
        let mut b = buf("one two three");
        b.set_cursor(0, 8, false); // start of "three"
        b.delete_word_back();
        assert_eq!(b.lines[0], "one three");
        b.set_cursor(0, 4, false); // "t" of three
        b.delete_word_fwd();
        assert_eq!(b.lines[0], "one ");
    }

    #[test]
    fn toggle_line_comment() {
        let mut b = buf("    let x = 1;");
        b.lang = crate::syntax::detect(std::path::Path::new("a.rs"));
        b.toggle_comment();
        assert_eq!(b.lines[0], "    // let x = 1;");
        b.toggle_comment();
        assert_eq!(b.lines[0], "    let x = 1;");
        // No line comments for plain text -> false.
        let mut b = buf("hello");
        b.lang = &crate::syntax::PLAIN;
        assert!(!b.toggle_comment());
    }

    #[test]
    fn replace_all_and_count() {
        let mut b = buf("Foo foo bar\nfoo");
        assert_eq!(b.count_matches("foo"), 3); // smart-case matches Foo too
        let n = b.replace_all("foo", "baz");
        assert_eq!(n, 3);
        assert_eq!(b.lines, vec!["baz baz bar".to_string(), "baz".to_string()]);
        // exact-case when query has uppercase
        let mut b = buf("Foo foo");
        assert_eq!(b.replace_all("Foo", "x"), 1);
        assert_eq!(b.lines, vec!["x foo"]);
    }

    #[test]
    fn tab_stops_and_line_ops() {
        let mut b = buf("xy");
        b.set_cursor(0, 1, false);
        b.insert_tab();
        assert_eq!(b.lines[0], "x   y"); // to next 4-col stop
        let mut b = buf("one\ntwo");
        b.duplicate_line();
        assert_eq!(b.lines, vec!["one", "one", "two"]);
        b.delete_line();
        assert_eq!(b.lines, vec!["one", "two"]);
        assert_eq!(b.cy, 1); // cursor lands on "two"
        b.move_line(true);
        assert_eq!(b.lines, vec!["two", "one"]);
    }
}
