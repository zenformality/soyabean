//! Editor state machine: buffers, modes, keymap and the main event loop.

use std::io::{self, Write};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use crossterm::event::{
    self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent,
    MouseEventKind,
};
use crossterm::terminal;

use crate::buffer::{cx_at_vcol, visual_col, Buffer};
use crate::draw;
use crate::finder::Finder;

const HELP: &str = "\
soyabean — a minimal yet powerful code IDE

  FILES
    Ctrl+P / Ctrl+O   fuzzy-open a file from the workspace
    Ctrl+S            save (prompts for a name if untitled)
    Ctrl+N            new buffer
    Ctrl+W            close buffer (press twice to discard changes)
    Alt+Left/Right    previous / next buffer
    Alt+1..9          jump to buffer N
    Ctrl+Q            quit (press twice to discard changes)

  EDITING
    Ctrl+Z / Ctrl+Y   undo / redo
    Ctrl+C / X / V    copy / cut / paste (line when nothing selected)
    Ctrl+D            select word, then duplicate line
    Ctrl+K            delete line
    Alt+Up/Down       move line up / down
    Tab / Shift+Tab   indent / dedent (works on selections)
    Ctrl+A            select all
    Enter             newline with auto-indent

  NAVIGATION
    Ctrl+F            incremental search (Enter keep, Esc cancel,
                      Up/Down previous/next while open)
    F3 / Shift+F3     repeat search forward / backward
    Ctrl+G            go to line
    Ctrl+Left/Right   word left / right
    Ctrl+Home/End     start / end of file
    Home              first non-blank / column 0 (toggles)
    Shift+arrows      extend selection; mouse click/drag also works

  Terminal paste (bracketed) is supported; Ctrl+C also copies to the
  system clipboard when a clipboard tool is available.

Press Ctrl+W to close this help.";

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Edit,
    Find,
    Goto,
    SaveAs,
    Finder,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Pending {
    Quit,
    Close,
}

pub struct Editor {
    pub bufs: Vec<Buffer>,
    pub cur: usize,
    pub mode: Mode,
    pub input: String,
    pub finder: Finder,
    pub clipboard: String,
    pub status: Option<(String, Instant)>,
    pub last_search: String,
    pub root: PathBuf,
    pub size: (u16, u16),
    pending: Option<Pending>,
    saved_view: Option<(usize, usize, usize, usize)>, // cy, cx, row_off, col_off
    quit: bool,
}

impl Editor {
    pub fn new(args: &[String]) -> io::Result<Self> {
        let root = std::env::current_dir()?;
        let mut bufs = Vec::new();
        let mut errs = Vec::new();
        for a in args {
            let p = if PathBuf::from(a).is_absolute() {
                PathBuf::from(a)
            } else {
                root.join(a)
            };
            match Buffer::open(p) {
                Ok(b) => bufs.push(b),
                Err(e) => errs.push(format!("{a}: {e}")),
            }
        }
        if bufs.is_empty() {
            bufs.push(Buffer::empty());
        }
        let mut ed = Editor {
            bufs,
            cur: 0,
            mode: Mode::Edit,
            input: String::new(),
            finder: Finder::new(),
            clipboard: String::new(),
            status: None,
            last_search: String::new(),
            root,
            size: terminal::size()?,
            pending: None,
            saved_view: None,
            quit: false,
        };
        if !errs.is_empty() {
            ed.msg(errs.join("; "));
        }
        Ok(ed)
    }

    pub fn buf(&self) -> &Buffer {
        &self.bufs[self.cur]
    }

    pub fn buf_mut(&mut self) -> &mut Buffer {
        &mut self.bufs[self.cur]
    }

    pub fn msg(&mut self, s: impl Into<String>) {
        self.status = Some((s.into(), Instant::now()));
    }

    pub fn prompt_label(&self) -> &'static str {
        match self.mode {
            Mode::Find => " Find: ",
            Mode::Goto => " Go to line: ",
            Mode::SaveAs => " Save as: ",
            _ => "",
        }
    }

    pub fn gutter_w(&self) -> usize {
        let digits = self.buf().lines.len().to_string().len();
        digits.max(3) + 1
    }

    fn text_h(&self) -> usize {
        (self.size.1 as usize).saturating_sub(2).max(1)
    }

    fn text_w(&self) -> usize {
        (self.size.0 as usize).saturating_sub(self.gutter_w()).max(1)
    }

    /// Keep offsets within legal bounds (does not chase the cursor).
    pub fn clamp_scroll(&mut self, text_h: usize) {
        let b = self.buf_mut();
        let max_off = b.lines.len().saturating_sub(text_h);
        if b.row_off > max_off {
            b.row_off = max_off;
        }
    }

    fn ensure_visible(&mut self) {
        let text_h = self.text_h();
        let text_w = self.text_w();
        let b = self.buf_mut();
        if b.cy < b.row_off {
            b.row_off = b.cy;
        }
        if b.cy >= b.row_off + text_h {
            b.row_off = b.cy + 1 - text_h;
        }
        let vc = visual_col(&b.lines[b.cy], b.cx);
        if vc < b.col_off {
            b.col_off = vc;
        }
        if vc >= b.col_off + text_w {
            b.col_off = vc + 1 - text_w;
        }
    }

    pub fn run(&mut self, out: &mut impl Write) -> io::Result<()> {
        loop {
            self.size = terminal::size()?;
            draw::render(self, out)?;
            if self.quit {
                return Ok(());
            }
            if !event::poll(Duration::from_millis(250))? {
                continue;
            }
            match event::read()? {
                Event::Key(k) if matches!(k.kind, KeyEventKind::Press | KeyEventKind::Repeat) => {
                    self.on_key(k);
                }
                Event::Mouse(m) => self.on_mouse(m),
                Event::Paste(s) => self.on_paste(&s),
                Event::Resize(w, h) => self.size = (w, h),
                _ => {}
            }
        }
    }

    fn on_paste(&mut self, s: &str) {
        match self.mode {
            Mode::Edit => {
                self.buf_mut().insert_text(s);
                self.ensure_visible();
            }
            Mode::Find | Mode::Goto | Mode::SaveAs => {
                let clean: String = s.chars().filter(|c| *c != '\n' && *c != '\r').collect();
                self.input.push_str(&clean);
                if self.mode == Mode::Find {
                    self.live_search();
                }
            }
            Mode::Finder => {
                let clean: String = s.chars().filter(|c| *c != '\n' && *c != '\r').collect();
                self.finder.query.push_str(&clean);
                self.finder.refresh();
            }
        }
    }

    fn on_key(&mut self, k: KeyEvent) {
        match self.mode {
            Mode::Edit => self.key_edit(k),
            Mode::Find | Mode::Goto | Mode::SaveAs => self.key_prompt(k),
            Mode::Finder => self.key_finder(k),
        }
    }

    // ---- edit mode ------------------------------------------------------

    fn key_edit(&mut self, k: KeyEvent) {
        let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
        let alt = k.modifiers.contains(KeyModifiers::ALT);
        let shift = k.modifiers.contains(KeyModifiers::SHIFT);
        let pending = self.pending.take();
        let page = self.text_h();

        match k.code {
            // --- app ---
            KeyCode::Char('q') if ctrl => {
                let dirty = self.bufs.iter().any(|b| b.dirty && !b.is_scratch);
                if dirty && pending != Some(Pending::Quit) {
                    self.pending = Some(Pending::Quit);
                    self.msg("Unsaved changes — press Ctrl+Q again to quit without saving");
                } else {
                    self.quit = true;
                }
            }
            KeyCode::Char('s') if ctrl => self.save_current(),
            KeyCode::Char('p') | KeyCode::Char('o') if ctrl => {
                let root = self.root.clone();
                self.finder.open(&root);
                self.mode = Mode::Finder;
            }
            KeyCode::Char('n') if ctrl => {
                self.bufs.push(Buffer::empty());
                self.cur = self.bufs.len() - 1;
            }
            KeyCode::Char('w') if ctrl => {
                let b = self.buf();
                if b.dirty && !b.is_scratch && pending != Some(Pending::Close) {
                    self.pending = Some(Pending::Close);
                    self.msg("Unsaved changes — press Ctrl+W again to close without saving");
                } else {
                    self.bufs.remove(self.cur);
                    if self.bufs.is_empty() {
                        self.bufs.push(Buffer::empty());
                    }
                    if self.cur >= self.bufs.len() {
                        self.cur = self.bufs.len() - 1;
                    }
                }
            }
            KeyCode::F(1) => self.open_help(),

            // --- buffer switching ---
            KeyCode::Left if alt => {
                self.cur = (self.cur + self.bufs.len() - 1) % self.bufs.len();
            }
            KeyCode::Right if alt => {
                self.cur = (self.cur + 1) % self.bufs.len();
            }
            KeyCode::Char(c @ '1'..='9') if alt => {
                let idx = c as usize - '1' as usize;
                if idx < self.bufs.len() {
                    self.cur = idx;
                }
            }

            // --- find / goto ---
            KeyCode::Char('f') if ctrl => {
                let b = self.buf();
                self.saved_view = Some((b.cy, b.cx, b.row_off, b.col_off));
                self.input.clear();
                self.mode = Mode::Find;
            }
            KeyCode::Char('g') if ctrl => {
                self.input.clear();
                self.mode = Mode::Goto;
            }
            KeyCode::F(3) => {
                self.repeat_search(!shift);
                self.ensure_visible();
            }

            // --- undo / clipboard ---
            KeyCode::Char('z') if ctrl => {
                if !self.buf_mut().undo() {
                    self.msg("Nothing to undo");
                }
                self.ensure_visible();
            }
            KeyCode::Char('y') if ctrl => {
                if !self.buf_mut().redo() {
                    self.msg("Nothing to redo");
                }
                self.ensure_visible();
            }
            KeyCode::Char('a') if ctrl => self.buf_mut().select_all(),
            KeyCode::Char('c') if ctrl => self.copy(false),
            KeyCode::Char('x') if ctrl => self.copy(true),
            KeyCode::Char('v') if ctrl => {
                let text = self.clipboard.clone();
                if text.is_empty() {
                    self.msg("Clipboard is empty");
                } else {
                    self.buf_mut().insert_text(&text);
                }
                self.ensure_visible();
            }
            KeyCode::Char('d') if ctrl => {
                let b = self.buf_mut();
                if b.sel_range().is_none() {
                    b.select_word();
                    if b.sel_range().is_some() {
                        self.ensure_visible();
                        return;
                    }
                }
                self.buf_mut().duplicate_line();
                self.ensure_visible();
            }
            KeyCode::Char('k') if ctrl => {
                self.buf_mut().delete_line();
                self.ensure_visible();
            }

            // --- movement ---
            KeyCode::Left if ctrl => { self.buf_mut().word_left(shift); self.ensure_visible(); }
            KeyCode::Right if ctrl => { self.buf_mut().word_right(shift); self.ensure_visible(); }
            KeyCode::Home if ctrl => { self.buf_mut().doc_start(shift); self.ensure_visible(); }
            KeyCode::End if ctrl => { self.buf_mut().doc_end(shift); self.ensure_visible(); }
            KeyCode::Up if alt => { self.buf_mut().move_line(true); self.ensure_visible(); }
            KeyCode::Down if alt => { self.buf_mut().move_line(false); self.ensure_visible(); }
            KeyCode::Left => { self.buf_mut().left(shift); self.ensure_visible(); }
            KeyCode::Right => { self.buf_mut().right(shift); self.ensure_visible(); }
            KeyCode::Up => { self.buf_mut().up(shift); self.ensure_visible(); }
            KeyCode::Down => { self.buf_mut().down(shift); self.ensure_visible(); }
            KeyCode::Home => { self.buf_mut().home(shift); self.ensure_visible(); }
            KeyCode::End => { self.buf_mut().end(shift); self.ensure_visible(); }
            KeyCode::PageUp => { self.buf_mut().page_up(page, shift); self.ensure_visible(); }
            KeyCode::PageDown => { self.buf_mut().page_down(page, shift); self.ensure_visible(); }
            KeyCode::Esc => {
                self.buf_mut().anchor = None;
            }

            // --- editing ---
            KeyCode::Enter => { self.buf_mut().insert_newline(); self.ensure_visible(); }
            KeyCode::Backspace => { self.buf_mut().backspace(); self.ensure_visible(); }
            KeyCode::Delete => { self.buf_mut().delete_forward(); self.ensure_visible(); }
            KeyCode::Tab => { self.buf_mut().insert_tab(); self.ensure_visible(); }
            KeyCode::BackTab => { self.buf_mut().indent_lines(false); self.ensure_visible(); }
            KeyCode::Char(c) if !ctrl && !alt => {
                self.buf_mut().insert_char(c);
                self.ensure_visible();
            }
            _ => {}
        }
    }

    // ---- prompt modes ---------------------------------------------------

    fn key_prompt(&mut self, k: KeyEvent) {
        let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
        match k.code {
            KeyCode::Esc => {
                if self.mode == Mode::Find {
                    if let Some((cy, cx, ro, co)) = self.saved_view.take() {
                        let b = self.buf_mut();
                        b.set_cursor(cy, cx, false);
                        b.row_off = ro;
                        b.col_off = co;
                    }
                    self.buf_mut().anchor = None;
                }
                self.mode = Mode::Edit;
                self.input.clear();
            }
            KeyCode::Enter => {
                let input = self.input.clone();
                match self.mode {
                    Mode::Find => {
                        if !input.is_empty() {
                            self.last_search = input;
                        }
                        self.saved_view = None;
                    }
                    Mode::Goto => {
                        if let Ok(n) = input.trim().parse::<usize>() {
                            let b = self.buf_mut();
                            let cy = n.saturating_sub(1).min(b.lines.len() - 1);
                            b.set_cursor(cy, 0, false);
                            self.ensure_visible();
                        } else if !input.trim().is_empty() {
                            self.msg("Not a line number");
                        }
                    }
                    Mode::SaveAs => {
                        let t = input.trim();
                        if t.is_empty() {
                            self.msg("Save cancelled (empty name)");
                        } else {
                            let p = PathBuf::from(t);
                            let p = if p.is_absolute() { p } else { self.root.join(p) };
                            self.buf_mut().path = Some(p);
                            self.buf_mut().is_scratch = false;
                            self.mode = Mode::Edit;
                            self.input.clear();
                            self.save_current();
                            return;
                        }
                    }
                    _ => {}
                }
                self.mode = Mode::Edit;
                self.input.clear();
            }
            KeyCode::Backspace => {
                self.input.pop();
                if self.mode == Mode::Find {
                    self.live_search();
                }
            }
            KeyCode::Down if self.mode == Mode::Find => self.search_step(true),
            KeyCode::Up if self.mode == Mode::Find => self.search_step(false),
            KeyCode::Char('v') if ctrl => {
                let text: String = self
                    .clipboard
                    .chars()
                    .filter(|c| *c != '\n' && *c != '\r')
                    .collect();
                self.input.push_str(&text);
                if self.mode == Mode::Find {
                    self.live_search();
                }
            }
            KeyCode::Char(c) if !ctrl => {
                self.input.push(c);
                if self.mode == Mode::Find {
                    self.live_search();
                }
            }
            _ => {}
        }
    }

    fn live_search(&mut self) {
        let Some((cy, cx, ro, co)) = self.saved_view else { return };
        if self.input.is_empty() {
            let b = self.buf_mut();
            b.set_cursor(cy, cx, false);
            b.row_off = ro;
            b.col_off = co;
            return;
        }
        let q = self.input.clone();
        self.apply_match(&q, (cy, cx), true);
    }

    fn search_step(&mut self, forward: bool) {
        if self.input.is_empty() {
            return;
        }
        let q = self.input.clone();
        let b = self.buf();
        let qlen = q.chars().count();
        let from = if forward {
            (b.cy, b.cx)
        } else {
            (b.cy, b.cx.saturating_sub(qlen))
        };
        self.apply_match(&q, from, forward);
    }

    fn repeat_search(&mut self, forward: bool) {
        if self.last_search.is_empty() {
            self.msg("No previous search (Ctrl+F to search)");
            return;
        }
        let q = self.last_search.clone();
        let b = self.buf();
        let qlen = q.chars().count();
        let from = if forward {
            (b.cy, b.cx)
        } else {
            (b.cy, b.cx.saturating_sub(qlen))
        };
        self.apply_match(&q, from, forward);
    }

    fn apply_match(&mut self, q: &str, from: (usize, usize), forward: bool) {
        match self.buf().find(q, from, forward) {
            Some((y, x, len)) => {
                let b = self.buf_mut();
                b.anchor = Some((y, x));
                b.set_cursor(y, x + len, true);
                self.ensure_visible();
            }
            None => self.msg(format!("No match: {q}")),
        }
    }

    // ---- finder mode ----------------------------------------------------

    fn key_finder(&mut self, k: KeyEvent) {
        let list_h = self.text_h().saturating_sub(1).max(1);
        match k.code {
            KeyCode::Esc => self.mode = Mode::Edit,
            KeyCode::Enter => {
                let path = self.finder.selected_path(&self.root);
                self.mode = Mode::Edit;
                if let Some(p) = path {
                    self.open_file(p);
                }
            }
            KeyCode::Up => {
                self.finder.sel = self.finder.sel.saturating_sub(1);
            }
            KeyCode::Down => {
                if self.finder.sel + 1 < self.finder.matched.len() {
                    self.finder.sel += 1;
                }
            }
            KeyCode::PageUp => {
                self.finder.sel = self.finder.sel.saturating_sub(list_h);
            }
            KeyCode::PageDown => {
                self.finder.sel =
                    (self.finder.sel + list_h).min(self.finder.matched.len().saturating_sub(1));
            }
            KeyCode::Backspace => {
                self.finder.query.pop();
                self.finder.refresh();
            }
            KeyCode::Char(c) if !k.modifiers.contains(KeyModifiers::CONTROL) => {
                self.finder.query.push(c);
                self.finder.refresh();
            }
            _ => {}
        }
        // Keep selection visible in the list.
        if self.finder.sel < self.finder.scroll {
            self.finder.scroll = self.finder.sel;
        }
        if self.finder.sel >= self.finder.scroll + list_h {
            self.finder.scroll = self.finder.sel + 1 - list_h;
        }
    }

    fn open_file(&mut self, path: PathBuf) {
        // Already open? Just switch.
        if let Some(i) = self.bufs.iter().position(|b| b.path.as_ref() == Some(&path)) {
            self.cur = i;
            return;
        }
        match Buffer::open(path.clone()) {
            Ok(b) => {
                // Replace a pristine untitled buffer instead of stacking up.
                if self.bufs.len() == 1
                    && self.bufs[0].path.is_none()
                    && !self.bufs[0].dirty
                    && !self.bufs[0].is_scratch
                {
                    self.bufs[0] = b;
                    self.cur = 0;
                } else {
                    self.bufs.push(b);
                    self.cur = self.bufs.len() - 1;
                }
            }
            Err(e) => self.msg(format!("Can't open {}: {e}", path.display())),
        }
    }

    fn open_help(&mut self) {
        if let Some(i) = self.bufs.iter().position(|b| b.scratch_name == "*help*") {
            self.cur = i;
            return;
        }
        self.bufs.push(Buffer::scratch("*help*", HELP));
        self.cur = self.bufs.len() - 1;
    }

    // ---- save / clipboard -----------------------------------------------

    fn save_current(&mut self) {
        if self.buf().path.is_none() {
            self.input.clear();
            self.mode = Mode::SaveAs;
            return;
        }
        let name = self.buf().display_name();
        match self.buf_mut().save() {
            Ok(bytes) => self.msg(format!("Saved {name} ({bytes} bytes)")),
            Err(e) => self.msg(format!("Save failed: {e}")),
        }
    }

    fn copy(&mut self, cut: bool) {
        let text = match self.buf().selected_text() {
            Some(t) => t,
            None => self.buf().cur_line().to_string() + "\n",
        };
        self.clipboard = text.clone();
        os_copy(&text);
        if cut {
            let b = self.buf_mut();
            if b.sel_range().is_some() {
                b.delete_selection();
            } else {
                b.delete_line();
            }
            self.ensure_visible();
        } else {
            self.msg(format!("Copied {} chars", text.chars().count()));
        }
    }

    // ---- mouse ----------------------------------------------------------

    fn on_mouse(&mut self, m: MouseEvent) {
        if self.mode != Mode::Edit {
            if self.mode == Mode::Finder {
                match m.kind {
                    MouseEventKind::ScrollUp => self.finder.sel = self.finder.sel.saturating_sub(1),
                    MouseEventKind::ScrollDown
                        if self.finder.sel + 1 < self.finder.matched.len() =>
                    {
                        self.finder.sel += 1;
                    }
                    _ => {}
                }
            }
            return;
        }
        let text_h = self.text_h();
        let gw = self.gutter_w();
        match m.kind {
            MouseEventKind::ScrollUp => {
                let b = self.buf_mut();
                b.row_off = b.row_off.saturating_sub(3);
            }
            MouseEventKind::ScrollDown => {
                let lines = self.buf().lines.len();
                let b = self.buf_mut();
                b.row_off = (b.row_off + 3).min(lines.saturating_sub(text_h));
            }
            MouseEventKind::Down(MouseButton::Left) | MouseEventKind::Drag(MouseButton::Left) => {
                let row = m.row as usize;
                if row >= text_h {
                    return;
                }
                let drag = matches!(m.kind, MouseEventKind::Drag(_));
                let col = m.column as usize;
                let b = self.buf_mut();
                let cy = (b.row_off + row).min(b.lines.len() - 1);
                let target = col.saturating_sub(gw) + b.col_off;
                let cx = cx_at_vcol(&b.lines[cy], target);
                b.set_cursor(cy, cx, drag);
                self.ensure_visible();
            }
            _ => {}
        }
    }
}

/// Best-effort mirror to the OS clipboard so text can leave the editor.
fn os_copy(text: &str) {
    use std::process::{Command, Stdio};
    #[cfg(target_os = "windows")]
    let cmd = Command::new("clip").stdin(Stdio::piped()).stdout(Stdio::null()).stderr(Stdio::null()).spawn();
    #[cfg(target_os = "macos")]
    let cmd = Command::new("pbcopy").stdin(Stdio::piped()).stdout(Stdio::null()).stderr(Stdio::null()).spawn();
    #[cfg(all(unix, not(target_os = "macos")))]
    let cmd = Command::new("xclip")
        .args(["-selection", "clipboard"])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();
    if let Ok(mut child) = cmd {
        if let Some(mut stdin) = child.stdin.take() {
            let _ = stdin.write_all(text.as_bytes());
        }
        let _ = child.wait();
    }
}
