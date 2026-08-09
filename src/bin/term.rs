//! Embedded terminal: a real PTY (via `portable-pty`) with a compact VT-style
//! ANSI parser and an egui renderer. Supports colour (16/256/truecolour),
//! cursor motion, erase, titles and scrollback — enough for everyday shell use.

use std::collections::VecDeque;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::thread;

use eframe::egui::text::LayoutJob;
use eframe::egui::{self, pos2, vec2, Color32, FontId, Id, Rect, Sense, Stroke, TextFormat};
use unicode_width::UnicodeWidthChar;

use super::theme::Palette;

const SCROLLBACK: usize = 1500;

const ANSI16: [Color32; 16] = [
    Color32::from_rgb(14, 17, 22),    // 0 black
    Color32::from_rgb(224, 108, 117), // 1 red
    Color32::from_rgb(152, 195, 121), // 2 green
    Color32::from_rgb(209, 154, 102), // 3 yellow
    Color32::from_rgb(97, 175, 239),  // 4 blue
    Color32::from_rgb(198, 120, 221), // 5 magenta
    Color32::from_rgb(86, 182, 194),  // 6 cyan
    Color32::from_rgb(220, 223, 228), // 7 white
    Color32::from_rgb(127, 132, 142), // 8 bright black
    Color32::from_rgb(240, 113, 120), // 9 bright red
    Color32::from_rgb(152, 195, 121), // 10 bright green
    Color32::from_rgb(229, 192, 123), // 11 bright yellow
    Color32::from_rgb(97, 175, 239),  // 12 bright blue
    Color32::from_rgb(198, 120, 221), // 13 bright magenta
    Color32::from_rgb(86, 182, 194),  // 14 bright cyan
    Color32::from_rgb(255, 255, 255), // 15 bright white
];

fn ansi256(i: u8) -> Color32 {
    match i {
        0..=15 => ANSI16[i as usize],
        16..=231 => {
            let v = i - 16;
            let (r, g, b) = (v / 36, (v % 36) / 6, v % 6);
            let lvl = |x: u8| if x == 0 { 0 } else { 55 + x * 40 };
            Color32::from_rgb(lvl(r), lvl(g), lvl(b))
        }
        _ => {
            let g = 8 + (i - 232) * 10;
            Color32::from_rgb(g, g, g)
        }
    }
}

#[derive(Clone, Copy, Default)]
struct Style {
    fg: Option<Color32>,
    bg: Option<Color32>,
    bold: bool,
    underline: bool,
    reverse: bool,
}

#[derive(Clone, Copy)]
struct Cell {
    c: char,
    style: Style,
}

impl Default for Cell {
    fn default() -> Self {
        Cell {
            c: ' ',
            style: Style::default(),
        }
    }
}

#[derive(Clone)]
struct Line {
    cells: Vec<Cell>,
}

enum EscState {
    Start,
    Csi(Vec<u8>),
    Osc(Vec<u8>),
    // `(` / `)` charset select: swallow the next byte.
    Charset,
}

pub struct Terminal {
    pty: Box<dyn portable_pty::MasterPty + Send>,
    writer: Option<Box<dyn Write + Send>>,
    child: Option<Box<dyn portable_pty::Child + Send + Sync>>,
    rx: mpsc::Receiver<Vec<u8>>,
    rows: usize,
    cols: usize,
    screen: Vec<Line>,
    history: VecDeque<Line>,
    cur: (usize, usize),
    saved_cur: (usize, usize),
    style: Style,
    esc: Option<EscState>,
    utf: Vec<u8>,
    pub title: String,
    pub cwd: PathBuf,
    exited: bool,
    pub focused: bool,
    scroll_off: usize,
}

impl Terminal {
    pub fn spawn(cwd: &Path, ctx: &egui::Context) -> Option<Terminal> {
        let pty_system = portable_pty::native_pty_system();
        let pair = pty_system
            .openpty(portable_pty::PtySize {
                rows: 24,
                cols: 80,
                pixel_width: 0,
                pixel_height: 0,
            })
            .ok()?;
        let (shell, args) = build_shell_command();
        let mut cmd = portable_pty::CommandBuilder::new(shell);
        for a in args {
            cmd.arg(a);
        }
        if cwd.exists() {
            cmd.cwd(cwd);
        }
        let child = pair.slave.spawn_command(cmd).ok()?;
        drop(pair.slave);
        let pty = pair.master;
        let mut reader = pty.try_clone_reader().ok()?;
        let writer = pty.take_writer().ok()?;
        let (tx, rx) = mpsc::channel();
        let repaint = ctx.clone();
        thread::spawn(move || {
            let mut buf = [0u8; 8192];
            loop {
                match reader.read(&mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        if tx.send(buf[..n].to_vec()).is_err() {
                            break;
                        }
                        repaint.request_repaint();
                    }
                }
            }
        });
        let mut t = Terminal {
            pty,
            writer: Some(writer),
            child: Some(child),
            rx,
            rows: 24,
            cols: 80,
            screen: Vec::new(),
            history: VecDeque::new(),
            cur: (0, 0),
            saved_cur: (0, 0),
            style: Style::default(),
            esc: None,
            utf: Vec::new(),
            title: String::new(),
            cwd: cwd.to_path_buf(),
            exited: false,
            focused: false,
            scroll_off: 0,
        };
        t.resize(80, 24);
        Some(t)
    }

    pub fn write(&mut self, bytes: &[u8]) {
        if let Some(w) = self.writer.as_mut() {
            let _ = w.write_all(bytes);
            let _ = w.flush();
        }
    }

    pub fn kill(&mut self) {
        if let Some(c) = self.child.as_mut() {
            let _ = c.kill();
        }
    }

    pub fn is_exited(&self) -> bool {
        self.exited
    }

    fn resize(&mut self, cols: usize, rows: usize) {
        let cols = cols.max(2);
        let rows = rows.max(2);
        if cols != self.cols {
            for l in self.screen.iter_mut() {
                l.cells.resize(cols, Cell::default());
            }
            for l in self.history.iter_mut() {
                l.cells.resize(cols, Cell::default());
            }
            self.cols = cols;
        }
        if rows != self.rows {
            if rows > self.rows {
                while self.screen.len() < rows {
                    self.screen.push(Self::blank_line(cols));
                }
            } else {
                while self.screen.len() > rows {
                    let top = self.screen.remove(0);
                    self.history.push_back(top);
                    while self.history.len() > SCROLLBACK {
                        self.history.pop_front();
                    }
                }
            }
            self.rows = rows;
        }
        self.cur.0 = self.cur.0.min(self.rows - 1);
        self.cur.1 = self.cur.1.min(self.cols - 1);
    }

    fn blank_line(cols: usize) -> Line {
        Line {
            cells: vec![Cell::default(); cols],
        }
    }

    fn check_exit(&mut self) {
        if !self.exited {
            if let Some(c) = self.child.as_mut() {
                if c.try_wait().ok().flatten().is_some() {
                    self.exited = true;
                }
            }
        }
    }

    // ── ANSI parsing ─────────────────────────────────────────────────────

    fn process(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.feed_byte(b);
        }
        self.check_exit();
    }

    fn feed_byte(&mut self, b: u8) {
        match self.esc.take() {
            Some(EscState::Csi(mut buf)) => {
                buf.push(b);
                if (0x40..0x80).contains(&b) {
                    self.csi(buf);
                } else {
                    self.esc = Some(EscState::Csi(buf));
                }
            }
            Some(EscState::Osc(mut buf)) => {
                if b == 0x07 {
                    self.osc(&buf);
                } else if buf.last() == Some(&0x1b) && b == b'\\' {
                    buf.pop();
                    self.osc(&buf);
                } else if buf.len() < 1024 {
                    buf.push(b);
                    self.esc = Some(EscState::Osc(buf));
                }
            }
            Some(EscState::Charset) => {}
            Some(EscState::Start) => match b {
                b'[' => self.esc = Some(EscState::Csi(Vec::with_capacity(8))),
                b']' => self.esc = Some(EscState::Osc(Vec::with_capacity(16))),
                b'(' | b')' => self.esc = Some(EscState::Charset),
                b'7' => self.saved_cur = self.cur,
                b'8' => self.cur = self.saved_cur,
                b'M' => self.reverse_index(),
                b'D' => self.line_feed(),
                b'E' => {
                    self.cur.1 = 0;
                    self.line_feed();
                }
                b'c' => {
                    self.history.clear();
                    for r in 0..self.rows {
                        self.screen[r]
                            .cells
                            .iter_mut()
                            .for_each(|x| *x = Cell::default());
                    }
                    self.cur = (0, 0);
                    self.style = Style::default();
                }
                _ => {}
            },
            None => {
                if b == 0x1b {
                    self.esc = Some(EscState::Start);
                } else {
                    self.feed_plain(b);
                }
            }
        }
    }

    fn reverse_index(&mut self) {
        if self.cur.0 == 0 {
            self.screen.insert(0, Self::blank_line(self.cols));
            self.screen.pop();
        } else {
            self.cur.0 -= 1;
        }
    }

    fn feed_plain(&mut self, b: u8) {
        if b == 0x07 {
            return; // bell
        }
        if b == 0x18 || b == 0x1a {
            self.utf.clear();
            return; // CAN / SUB
        }
        match b {
            0x08 => self.cur.1 = self.cur.1.saturating_sub(1),
            0x09 => self.cur.1 = ((self.cur.1 / 8) + 1) * 8,
            0x0a | 0x0b | 0x0c => self.line_feed(),
            0x0d => self.cur.1 = 0,
            0x00..=0x1f => {}
            0x7f => {}
            _ => self.feed_utf8(b),
        }
        self.cur.1 = self.cur.1.min(self.cols - 1);
    }

    fn feed_utf8(&mut self, b: u8) {
        if b < 0x80 {
            self.utf.clear();
            self.put_char(b as char);
            return;
        }
        self.utf.push(b);
        match std::str::from_utf8(&self.utf) {
            Ok(s) => {
                if let Some(c) = s.chars().next() {
                    self.put_char(c);
                }
                self.utf.clear();
            }
            Err(_) => {
                let expected = match self.utf[0] {
                    0xc0..=0xdf => 2,
                    0xe0..=0xef => 3,
                    0xf0..=0xf7 => 4,
                    _ => 0,
                };
                if expected > 0 && self.utf.len() >= expected {
                    self.utf.clear();
                }
            }
        }
    }

    fn put_char(&mut self, c: char) {
        let w = UnicodeWidthChar::width(c).unwrap_or(0).max(1);
        if w == 2 && self.cur.1 + 2 > self.cols {
            self.cur.1 = 0;
            self.line_feed();
        } else if self.cur.1 + w > self.cols {
            self.cur.1 = 0;
            self.line_feed();
        }
        let style = self.style;
        {
            let line = &mut self.screen[self.cur.0];
            line.cells[self.cur.1] = Cell { c, style };
            if w == 2 && self.cur.1 + 1 < self.cols {
                line.cells[self.cur.1 + 1] = Cell { c: '\0', style };
            }
        }
        self.cur.1 = (self.cur.1 + w).min(self.cols);
        if self.scroll_off == 0 {
            self.scroll_off = 0;
        }
    }

    fn line_feed(&mut self) {
        if self.cur.0 == self.rows - 1 {
            let top = self.screen.remove(0);
            self.history.push_back(top);
            while self.history.len() > SCROLLBACK {
                self.history.pop_front();
            }
            self.screen.push(Self::blank_line(self.cols));
        } else {
            self.cur.0 += 1;
        }
    }

    fn csi(&mut self, buf: Vec<u8>) {
        let finalb = *buf.last().unwrap_or(&b'm');
        let mut i = 0;
        if matches!(buf[0], b'?' | b'>' | b'=') {
            i = 1;
        }
        let mut params: Vec<u32> = Vec::new();
        let mut cur = 0u32;
        for &b in &buf[i..buf.len() - 1] {
            match b {
                b'0'..=b'9' => cur = cur.saturating_mul(10).saturating_add((b - b'0') as u32),
                b';' | b':' => {
                    params.push(cur);
                    cur = 0;
                }
                _ => {}
            }
        }
        if !buf[i..buf.len() - 1].is_empty() {
            params.push(cur);
        }
        match finalb {
            b'A' => {
                self.cur.0 = self
                    .cur
                    .0
                    .saturating_sub(param(&params, 0, 1).max(1) as usize)
            }
            b'B' => {
                self.cur.0 = (self.cur.0 + param(&params, 0, 1).max(1) as usize).min(self.rows - 1)
            }
            b'C' => {
                self.cur.1 = (self.cur.1 + param(&params, 0, 1).max(1) as usize).min(self.cols - 1)
            }
            b'D' => {
                self.cur.1 = self
                    .cur
                    .1
                    .saturating_sub(param(&params, 0, 1).max(1) as usize)
            }
            b'G' => {
                self.cur.1 = (param(&params, 0, 1) as usize)
                    .saturating_sub(1)
                    .min(self.cols - 1)
            }
            b'd' => {
                self.cur.0 = (param(&params, 0, 1) as usize)
                    .saturating_sub(1)
                    .min(self.rows - 1)
            }
            b'H' | b'f' => {
                self.cur.0 = (param(&params, 0, 1) as usize)
                    .saturating_sub(1)
                    .min(self.rows - 1);
                self.cur.1 = (param(&params, 1, 1) as usize)
                    .saturating_sub(1)
                    .min(self.cols - 1);
            }
            b'J' => self.erase_display(param(&params, 0, 0)),
            b'K' => self.erase_line(param(&params, 0, 0)),
            b'm' => self.sgr(params),
            b's' => self.saved_cur = self.cur,
            b'u' => self.cur = self.saved_cur,
            b'r' => {}
            b'h' | b'l' | b'g' | b'@' | b'P' | b'X' | b'S' | b'T' | b'n' | b't' => {}
            _ => {}
        }
    }

    fn sgr(&mut self, params: Vec<u32>) {
        if params.is_empty() {
            self.style = Style::default();
            return;
        }
        let mut it = params.iter();
        while let Some(&p) = it.next() {
            match p {
                0 => self.style = Style::default(),
                1 => self.style.bold = true,
                4 => self.style.underline = true,
                7 => self.style.reverse = true,
                22 => self.style.bold = false,
                24 => self.style.underline = false,
                27 => self.style.reverse = false,
                30..=37 => self.style.fg = Some(ANSI16[(p - 30) as usize]),
                40..=47 => self.style.bg = Some(ANSI16[(p - 40) as usize]),
                90..=97 => self.style.fg = Some(ANSI16[(p - 90 + 8) as usize]),
                100..=107 => self.style.bg = Some(ANSI16[(p - 100 + 8) as usize]),
                39 => self.style.fg = None,
                49 => self.style.bg = None,
                38 | 48 => {
                    let is_fg = p == 38;
                    match it.next() {
                        Some(5) => {
                            if let Some(&idx) = it.next() {
                                let c = ansi256(idx as u8);
                                if is_fg {
                                    self.style.fg = Some(c);
                                } else {
                                    self.style.bg = Some(c);
                                }
                            }
                        }
                        Some(2) => {
                            let r = it.next().copied().unwrap_or(0) as u8;
                            let g = it.next().copied().unwrap_or(0) as u8;
                            let b = it.next().copied().unwrap_or(0) as u8;
                            let c = Color32::from_rgb(r, g, b);
                            if is_fg {
                                self.style.fg = Some(c);
                            } else {
                                self.style.bg = Some(c);
                            }
                        }
                        _ => {}
                    }
                }
                _ => {}
            }
        }
    }

    fn erase_line(&mut self, mode: u32) {
        let (r, c) = (self.cur.0, self.cur.1);
        let line = &mut self.screen[r];
        match mode {
            0 => line.cells[c..]
                .iter_mut()
                .for_each(|x| *x = Cell::default()),
            1 => line.cells[..=c]
                .iter_mut()
                .for_each(|x| *x = Cell::default()),
            _ => line.cells.iter_mut().for_each(|x| *x = Cell::default()),
        }
    }

    fn erase_display(&mut self, mode: u32) {
        match mode {
            0 => {
                self.erase_line(0);
                for r in self.cur.0 + 1..self.rows {
                    self.screen[r]
                        .cells
                        .iter_mut()
                        .for_each(|x| *x = Cell::default());
                }
            }
            1 => {
                for r in 0..self.cur.0 {
                    self.screen[r]
                        .cells
                        .iter_mut()
                        .for_each(|x| *x = Cell::default());
                }
                self.erase_line(1);
            }
            _ => {
                self.history.clear();
                for r in 0..self.rows {
                    self.screen[r]
                        .cells
                        .iter_mut()
                        .for_each(|x| *x = Cell::default());
                }
                self.cur = (0, 0);
            }
        }
    }

    fn osc(&mut self, buf: &[u8]) {
        let raw = if buf.first() == Some(&0x1b) {
            &buf[1..]
        } else {
            buf
        };
        let semi = raw.iter().position(|&b| b == b';').unwrap_or(raw.len());
        let code: u32 = std::str::from_utf8(&raw[..semi])
            .ok()
            .and_then(|s| s.trim().parse().ok())
            .unwrap_or(0);
        let body = if semi < raw.len() {
            &raw[semi + 1..]
        } else {
            &[]
        };
        match code {
            0 | 2 => {
                if let Ok(s) = std::str::from_utf8(body) {
                    self.title = s.trim().to_string();
                }
            }
            7 => {
                if let Ok(s) = std::str::from_utf8(body) {
                    if let Some(p) = osc7_path(s.trim()) {
                        self.cwd = p;
                    }
                }
            }
            _ => {}
        }
    }

    // ── scrollback ───────────────────────────────────────────────────────

    fn total_lines(&self) -> usize {
        self.history.len() + self.rows
    }

    fn line_at(&self, i: usize) -> Option<&Line> {
        if i < self.history.len() {
            self.history.get(i)
        } else {
            self.screen.get(i - self.history.len())
        }
    }

    fn visible_start(&self) -> usize {
        let total = self.total_lines();
        let base = total.saturating_sub(self.rows);
        base.saturating_sub(self.scroll_off)
    }

    // ── keyboard input ───────────────────────────────────────────────────

    pub fn on_keys(&mut self, events: &[egui::Event]) {
        for e in events {
            match e {
                // egui emits a `Key` event AND a `Text` event for every printable
                // character (including space), so printable input must come from
                // `Text` only to avoid double-sending.
                egui::Event::Text(s) => self.write(s.as_bytes()),
                egui::Event::Paste(s) => self.write(s.as_bytes()),
                egui::Event::Key {
                    key,
                    pressed: true,
                    modifiers,
                    ..
                } => {
                    let ctrl = modifiers.ctrl || modifiers.command;
                    let alt = modifiers.alt;
                    let shift = modifiers.shift;
                    if ctrl {
                        if let Some(c) = key_char(*key) {
                            let c = (c as u8) & 0x1f;
                            let mut out = vec![c];
                            if alt {
                                out.insert(0, 0x1b);
                            }
                            self.write(&out);
                            continue;
                        }
                    }
                    let seq: &[u8] = match key {
                        egui::Key::Enter => b"\r",
                        egui::Key::Backspace => b"\x7f",
                        egui::Key::Tab => {
                            if shift {
                                b"\x1b[Z"
                            } else {
                                b"\t"
                            }
                        }
                        egui::Key::Escape => b"\x1b",
                        egui::Key::Delete => b"\x1b[3~",
                        egui::Key::Insert => b"\x1b[2~",
                        egui::Key::Home => b"\x1b[H",
                        egui::Key::End => b"\x1b[F",
                        egui::Key::PageUp => b"\x1b[5~",
                        egui::Key::PageDown => b"\x1b[6~",
                        egui::Key::ArrowUp => b"\x1b[A",
                        egui::Key::ArrowDown => b"\x1b[B",
                        egui::Key::ArrowRight => b"\x1b[C",
                        egui::Key::ArrowLeft => b"\x1b[D",
                        egui::Key::F1 => b"\x1bOP",
                        egui::Key::F2 => b"\x1bOQ",
                        egui::Key::F3 => b"\x1bOR",
                        egui::Key::F4 => b"\x1bOS",
                        egui::Key::F5 => b"\x1b[15~",
                        egui::Key::F6 => b"\x1b[17~",
                        egui::Key::F7 => b"\x1b[18~",
                        egui::Key::F8 => b"\x1b[19~",
                        egui::Key::F9 => b"\x1b[20~",
                        egui::Key::F10 => b"\x1b[21~",
                        egui::Key::F11 => b"\x1b[23~",
                        egui::Key::F12 => b"\x1b[24~",
                        // Space and other printable chars arrive as `Event::Text`.
                        _ => continue,
                    };
                    let mut out: Vec<u8> = Vec::with_capacity(4);
                    if alt && !seq.starts_with(&[0x1b]) {
                        out.push(0x1b);
                    }
                    out.extend_from_slice(seq);
                    self.write(&out);
                }
                _ => {}
            }
        }
    }

    // ── rendering ────────────────────────────────────────────────────────

    pub fn show(&mut self, ui: &mut egui::Ui, p: &Palette) {
        while let Ok(bytes) = self.rx.try_recv() {
            self.process(&bytes);
        }

        let id = Id::new("soyabean-terminal");
        let avail = ui.available_size();
        let (rect, resp) = ui.allocate_exact_size(avail, Sense::click_and_drag());
        if resp.clicked() {
            ui.memory_mut(|m| m.request_focus(id));
        }
        self.focused = resp.has_focus();

        let font_id = FontId::monospace(13.0);
        let cw = ui.fonts(|f| f.glyph_width(&font_id, 'M')).max(1.0);
        let row_h = ui.fonts(|f| f.row_height(&font_id)).max(1.0);
        let cols = (rect.width() / cw).floor() as usize;
        let rows = (rect.height() / row_h).floor() as usize;
        if cols != self.cols || rows != self.rows {
            self.resize(cols, rows);
            let _ = self.pty.resize(portable_pty::PtySize {
                rows: rows.max(2) as u16,
                cols: cols.max(2) as u16,
                pixel_width: 0,
                pixel_height: 0,
            });
        }

        if resp.hovered() {
            let scroll = ui.input(|i| i.raw_scroll_delta.y);
            if scroll.abs() > 0.0 {
                let n = 3;
                if scroll > 0.0 {
                    self.scroll_off =
                        (self.scroll_off + n).min(self.total_lines().saturating_sub(self.rows));
                } else {
                    self.scroll_off = self.scroll_off.saturating_sub(n);
                }
            }
        }

        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 0.0, p.bg);

        let start = self.visible_start();
        let fg_default = p.syn_normal;
        let mut row = 0usize;
        for i in start..start + self.rows {
            let y = rect.top() + row as f32 * row_h;
            let line = self.line_at(i).cloned().unwrap_or_else(|| Line {
                cells: vec![Cell::default(); self.cols],
            });
            self.paint_line(&painter, p, &line, rect, y, &font_id, fg_default);
            row += 1;
        }

        // cursor
        if self.focused && self.scroll_off == 0 {
            let (cr, cc) = self.cur;
            if let Some(line) = self.screen.get(cr) {
                let c = line.cells.get(cc).copied().unwrap_or_default();
                let (fg, bg) = resolved(c.style, p);
                let x = rect.left() + cc as f32 * cw;
                let y = rect.top() + cr as f32 * row_h;
                let blink = (ui.input(|i| i.time) * 1.7).fract() < 0.6;
                if blink {
                    painter.rect_filled(
                        Rect::from_min_size(pos2(x, y), vec2(cw, row_h)),
                        0.0,
                        fg.gamma_multiply(0.55),
                    );
                } else {
                    painter.rect_filled(
                        Rect::from_min_size(pos2(x, y + row_h - 2.0_f32), vec2(cw, 2.0_f32)),
                        0.0,
                        p.cursor_col,
                    );
                }
                let _ = bg;
            }
        }
    }

    fn paint_line(
        &self,
        painter: &egui::Painter,
        p: &Palette,
        line: &Line,
        rect: Rect,
        y: f32,
        font_id: &FontId,
        fg_default: Color32,
    ) {
        let mut job = LayoutJob::default();
        let mut run = String::new();
        let mut key: Option<(Color32, Option<Color32>, bool)> = None;

        let flush = |job: &mut LayoutJob,
                     run: &mut String,
                     key: &Option<(Color32, Option<Color32>, bool)>| {
            if run.is_empty() {
                return;
            }
            let (fg, bg, ul) = key.unwrap_or((fg_default, None, false));
            job.append(
                run,
                0.0,
                TextFormat {
                    font_id: font_id.clone(),
                    color: fg,
                    background: bg.unwrap_or(Color32::TRANSPARENT),
                    underline: if ul {
                        Stroke::new(1.0_f32, fg)
                    } else {
                        Stroke::NONE
                    },
                    ..Default::default()
                },
            );
            run.clear();
        };

        for cell in &line.cells {
            let (fg, bg) = resolved(cell.style, p);
            let k = (fg, bg, cell.style.underline);
            if key != Some(k) {
                flush(&mut job, &mut run, &key);
                key = Some(k);
            }
            run.push(if cell.c == '\0' { ' ' } else { cell.c });
        }
        flush(&mut job, &mut run, &key);

        let clip = Rect::from_min_max(rect.min, rect.max);
        painter.with_clip_rect(clip).galley(
            pos2(rect.left(), y),
            painter.layout_job(job),
            fg_default,
        );
    }
}

fn resolved(style: Style, p: &Palette) -> (Color32, Option<Color32>) {
    let fg = style.fg.unwrap_or(p.syn_normal);
    let bg = style.bg;
    if style.reverse {
        let f = bg.unwrap_or(p.bg);
        let b = Some(fg);
        (f, b)
    } else {
        (fg, bg)
    }
}

fn param(params: &[u32], i: usize, default: u32) -> u32 {
    params.get(i).copied().unwrap_or(default)
}

fn key_char(key: egui::Key) -> Option<char> {
    use egui::Key as K;
    match key {
        K::A => Some('a'),
        K::B => Some('b'),
        K::C => Some('c'),
        K::D => Some('d'),
        K::E => Some('e'),
        K::F => Some('f'),
        K::G => Some('g'),
        K::H => Some('h'),
        K::I => Some('i'),
        K::J => Some('j'),
        K::K => Some('k'),
        K::L => Some('l'),
        K::M => Some('m'),
        K::N => Some('n'),
        K::O => Some('o'),
        K::P => Some('p'),
        K::Q => Some('q'),
        K::R => Some('r'),
        K::S => Some('s'),
        K::T => Some('t'),
        K::U => Some('u'),
        K::V => Some('v'),
        K::W => Some('w'),
        K::X => Some('x'),
        K::Y => Some('y'),
        K::Z => Some('z'),
        K::Num0 => Some('0'),
        K::Num1 => Some('1'),
        K::Num2 => Some('2'),
        K::Num3 => Some('3'),
        K::Num4 => Some('4'),
        K::Num5 => Some('5'),
        K::Num6 => Some('6'),
        K::Num7 => Some('7'),
        K::Num8 => Some('8'),
        K::Num9 => Some('9'),
        K::Space => Some(' '),
        _ => None,
    }
}

// On Windows prefer PowerShell (with an OSC 7 prompt so the GUI can track `cd`),
// falling back to cmd.exe when PowerShell is not installed.
#[cfg(target_os = "windows")]
const PS_OSC7_PROMPT: &str = "function global:prompt { [Console]::Write(([char]27 + ']7;file://' + $env:COMPUTERNAME + '/' + ($PWD.Path -replace '\\\\','/') + [char]27 + '\\')); 'PS ' + $PWD.Path + '> ' }";

#[cfg(target_os = "windows")]
fn build_shell_command() -> (String, Vec<String>) {
    if std::env::var_os("PSModulePath").is_some() {
        (
            "powershell.exe".to_string(),
            vec![
                "-NoLogo".to_string(),
                "-NoExit".to_string(),
                "-Command".to_string(),
                PS_OSC7_PROMPT.to_string(),
            ],
        )
    } else {
        (
            std::env::var("COMSPEC").unwrap_or_else(|_| "cmd.exe".to_string()),
            Vec::new(),
        )
    }
}

#[cfg(not(target_os = "windows"))]
fn build_shell_command() -> (String, Vec<String>) {
    (
        std::env::var("SHELL").unwrap_or_else(|_| "/bin/bash".to_string()),
        Vec::new(),
    )
}

/// Parse an OSC 7 cwd report: `7;file://<host>/<path>`.
fn osc7_path(s: &str) -> Option<PathBuf> {
    let s = s.strip_prefix("file://")?;
    let host_end = s.find('/')?;
    let path_part = percent_decode(&s[host_end + 1..]);
    let path_part = path_part.trim();
    if path_part.is_empty() {
        return None;
    }
    #[cfg(target_os = "windows")]
    let p = path_part.replace('/', "\\");
    #[cfg(not(target_os = "windows"))]
    let p = path_part.to_string();
    Some(PathBuf::from(p))
}

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if i + 2 < b.len() && b[i] == b'%' {
            if let (Some(h), Some(l)) = (hex(b[i + 1]), hex(b[i + 2])) {
                out.push(h * 16 + l);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn hex(c: u8) -> Option<u8> {
    match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        b'A'..=b'F' => Some(c - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn osc7_parses_windows_path() {
        let p = osc7_path("file://LAPTOP/C:/Users/Me/some%20dir").expect("parse");
        #[cfg(target_os = "windows")]
        assert_eq!(p, PathBuf::from("C:\\Users\\Me\\some dir"));
        #[cfg(not(target_os = "windows"))]
        assert_eq!(p, PathBuf::from("C:/Users/Me/some dir"));
    }

    #[test]
    fn osc7_ignores_bad_input() {
        assert!(osc7_path("").is_none());
        assert!(osc7_path("file://").is_none());
        assert!(osc7_path("file://host").is_none());
    }

    #[test]
    fn percent_decode_handles_escapes() {
        assert_eq!(percent_decode("a%20b%2Fc"), "a b/c");
        assert_eq!(percent_decode("plain"), "plain");
        assert_eq!(percent_decode("100%"), "100%");
        assert_eq!(percent_decode("%zz"), "%zz");
    }
}
