//! Terminal rendering: text area with syntax colors and selection, status
//! bar, message/prompt bar and the fuzzy-finder overlay.

use std::io::{self, Write};

use crossterm::cursor::{Hide, MoveTo, Show};
use crossterm::queue;
use crossterm::style::{Color, Print, ResetColor, SetBackgroundColor, SetForegroundColor};
use crossterm::terminal::{Clear, ClearType};
use unicode_width::UnicodeWidthStr;

use crate::buffer::{ch_width, visual_col};
use crate::editor::{Editor, Mode};
use crate::syntax::{self, Tok};

const C_NORMAL: Color = Color::Rgb { r: 212, g: 212, b: 212 };
const C_KEYWORD: Color = Color::Rgb { r: 197, g: 134, b: 192 };
const C_TYPE: Color = Color::Rgb { r: 78, g: 201, b: 176 };
const C_STR: Color = Color::Rgb { r: 206, g: 145, b: 120 };
const C_COMMENT: Color = Color::Rgb { r: 106, g: 153, b: 85 };
const C_NUMBER: Color = Color::Rgb { r: 181, g: 206, b: 168 };
const C_FUNC: Color = Color::Rgb { r: 220, g: 220, b: 170 };
const C_PUNCT: Color = Color::Rgb { r: 154, g: 165, b: 180 };
const C_GUTTER: Color = Color::Rgb { r: 100, g: 105, b: 110 };
const C_GUTTER_CUR: Color = Color::Rgb { r: 210, g: 210, b: 210 };
const BG_SEL: Color = Color::Rgb { r: 38, g: 79, b: 120 };
const BG_CURLINE: Color = Color::Rgb { r: 40, g: 42, b: 48 };
const BG_STATUS: Color = Color::Rgb { r: 50, g: 58, b: 72 };
const FG_STATUS: Color = Color::Rgb { r: 230, g: 230, b: 230 };
const FG_DIM: Color = Color::Rgb { r: 130, g: 135, b: 140 };
const BG_FINDER_SEL: Color = Color::Rgb { r: 45, g: 70, b: 100 };
const C_ACCENT: Color = Color::Rgb { r: 120, g: 200, b: 120 };

fn tok_color(t: Tok) -> Color {
    match t {
        Tok::Normal => C_NORMAL,
        Tok::Keyword => C_KEYWORD,
        Tok::Type => C_TYPE,
        Tok::Str => C_STR,
        Tok::Comment => C_COMMENT,
        Tok::Number => C_NUMBER,
        Tok::Func => C_FUNC,
        Tok::Punct => C_PUNCT,
    }
}

fn pad_to(s: &str, width: usize) -> String {
    let w = UnicodeWidthStr::width(s);
    if w >= width {
        let mut out = String::new();
        let mut acc = 0;
        for c in s.chars() {
            let cw = unicode_width::UnicodeWidthChar::width(c).unwrap_or(1);
            if acc + cw > width {
                break;
            }
            acc += cw;
            out.push(c);
        }
        out
    } else {
        format!("{}{}", s, " ".repeat(width - w))
    }
}

pub fn render(ed: &mut Editor, out: &mut impl Write) -> io::Result<()> {
    let (w, h) = (ed.size.0 as usize, ed.size.1 as usize);
    if w < 10 || h < 4 {
        return Ok(());
    }
    let text_h = h - 2;
    let gw = ed.gutter_w();
    let text_w = w.saturating_sub(gw).max(1);

    ed.clamp_scroll(text_h);
    ed.buf_mut().ensure_syntax();

    queue!(out, Hide)?;

    if matches!(ed.mode, Mode::Finder) {
        render_finder(ed, out, w, text_h)?;
    } else {
        render_text(ed, out, gw, text_w, text_h)?;
    }
    render_status(ed, out, w, h)?;
    render_message(ed, out, w, h)?;

    // Cursor placement.
    match ed.mode {
        Mode::Edit => {
            let b = ed.buf();
            let vc = visual_col(&b.lines[b.cy], b.cx);
            if b.cy >= b.row_off
                && b.cy < b.row_off + text_h
                && vc >= b.col_off
                && vc < b.col_off + text_w
            {
                queue!(
                    out,
                    MoveTo((gw + vc - b.col_off) as u16, (b.cy - b.row_off) as u16),
                    Show
                )?;
            }
        }
        Mode::Finder => {
            let col = 4 + UnicodeWidthStr::width(ed.finder.query.as_str());
            queue!(out, MoveTo(col.min(w - 1) as u16, 0), Show)?;
        }
        Mode::Find | Mode::Goto | Mode::SaveAs => {
            let label = ed.prompt_label();
            let col = UnicodeWidthStr::width(label) + UnicodeWidthStr::width(ed.input.as_str());
            queue!(out, MoveTo(col.min(w - 1) as u16, (h - 1) as u16), Show)?;
        }
    }
    out.flush()
}

fn render_text(
    ed: &mut Editor,
    out: &mut impl Write,
    gw: usize,
    text_w: usize,
    text_h: usize,
) -> io::Result<()> {
    let b = ed.buf();
    let sel = b.sel_range();
    let num_w = gw.saturating_sub(1);

    for row in 0..text_h {
        let y = b.row_off + row;
        queue!(out, MoveTo(0, row as u16), ResetColor, Clear(ClearType::UntilNewLine))?;

        if y >= b.lines.len() {
            queue!(out, SetForegroundColor(FG_DIM), Print("~"))?;
            continue;
        }

        let cur_line = y == b.cy && sel.is_none();
        if cur_line {
            queue!(out, SetBackgroundColor(BG_CURLINE), Clear(ClearType::UntilNewLine))?;
        }

        // Gutter.
        let gcol = if y == b.cy { C_GUTTER_CUR } else { C_GUTTER };
        queue!(
            out,
            SetForegroundColor(gcol),
            Print(format!("{:>nw$} ", y + 1, nw = num_w))
        )?;

        // Line content.
        let line = &b.lines[y];
        let state = b.line_states.get(y).copied().unwrap_or_default();
        let (toks, _) = syntax::highlight_line(line, b.lang, state);

        let in_sel = |cx: usize| -> bool {
            match sel {
                Some(((sy, sx), (ey, ex))) => {
                    (y > sy || (y == sy && cx >= sx)) && (y < ey || (y == ey && cx < ex))
                }
                None => false,
            }
        };

        let mut vcol = 0usize;
        let mut cur_fg: Option<Color> = None;
        let mut cur_bg_sel: Option<bool> = None;
        for (ci, ch) in line.chars().enumerate() {
            let cw = ch_width(ch, vcol);
            if vcol + cw <= b.col_off {
                vcol += cw;
                continue;
            }
            if vcol >= b.col_off + text_w {
                break;
            }
            let selected = in_sel(ci);
            if cur_bg_sel != Some(selected) {
                if selected {
                    queue!(out, SetBackgroundColor(BG_SEL))?;
                } else if cur_line {
                    queue!(out, SetBackgroundColor(BG_CURLINE))?;
                } else {
                    queue!(out, ResetColor)?;
                    cur_fg = None;
                }
                cur_bg_sel = Some(selected);
            }
            let fg = tok_color(toks.get(ci).copied().unwrap_or(Tok::Normal));
            if cur_fg != Some(fg) {
                queue!(out, SetForegroundColor(fg))?;
                cur_fg = Some(fg);
            }
            if ch == '\t' {
                queue!(out, Print(" ".repeat(cw)))?;
            } else {
                queue!(out, Print(ch))?;
            }
            vcol += cw;
        }

        // Show selection continuing past end of line.
        if let Some(((sy, _), (ey, _))) = sel {
            if y >= sy && y < ey && visual_col(line, crate::buffer::charlen(line)) >= b.col_off {
                queue!(out, SetBackgroundColor(BG_SEL), Print(" "))?;
            }
        }
        queue!(out, ResetColor)?;
    }
    Ok(())
}

fn render_finder(ed: &Editor, out: &mut impl Write, w: usize, text_h: usize) -> io::Result<()> {
    let f = &ed.finder;
    // Query line.
    queue!(
        out,
        MoveTo(0, 0),
        ResetColor,
        Clear(ClearType::UntilNewLine),
        SetForegroundColor(C_ACCENT),
        Print(" ▸ "),
        SetForegroundColor(C_NORMAL),
        Print(&f.query),
        SetForegroundColor(FG_DIM),
        Print(format!("   ({} / {} files)", f.matched.len(), f.files.len()))
    )?;

    let list_h = text_h.saturating_sub(1);
    for row in 0..list_h {
        let idx = f.scroll + row;
        queue!(out, MoveTo(0, (row + 1) as u16), ResetColor, Clear(ClearType::UntilNewLine))?;
        if idx >= f.matched.len() {
            continue;
        }
        let path = &f.files[f.matched[idx]];
        let selected = idx == f.sel;
        if selected {
            queue!(out, SetBackgroundColor(BG_FINDER_SEL))?;
        }
        let fname_start = path.rfind('/').map(|i| i + 1).unwrap_or(0);
        let (dir, name) = path.split_at(fname_start);
        let line = format!("  {}{}", dir, name);
        let padded = pad_to(&line, w);
        // Print dir dim, name bright.
        let dir_len = 2 + dir.len();
        queue!(
            out,
            SetForegroundColor(FG_DIM),
            Print(&padded[..dir_len.min(padded.len())]),
            SetForegroundColor(if selected { FG_STATUS } else { C_NORMAL }),
            Print(&padded[dir_len.min(padded.len())..]),
            ResetColor
        )?;
    }
    Ok(())
}

fn render_status(ed: &Editor, out: &mut impl Write, w: usize, h: usize) -> io::Result<()> {
    let b = ed.buf();
    let dirty = if b.dirty { " ●" } else { "" };
    let left = format!(
        " soyabean  │  {}{}  │  {}  │  buf {}/{}",
        b.display_name(),
        dirty,
        b.lang.name,
        ed.cur + 1,
        ed.bufs.len()
    );
    let vc = visual_col(&b.lines[b.cy], b.cx);
    let right = format!(
        "Ln {}, Col {}  {}  ",
        b.cy + 1,
        vc + 1,
        if b.crlf { "CRLF" } else { "LF" }
    );
    let lw = UnicodeWidthStr::width(left.as_str());
    let rw = UnicodeWidthStr::width(right.as_str());
    let bar = if lw + rw >= w {
        pad_to(&left, w)
    } else {
        format!("{}{}{}", left, " ".repeat(w - lw - rw), right)
    };
    queue!(
        out,
        MoveTo(0, (h - 2) as u16),
        SetBackgroundColor(BG_STATUS),
        SetForegroundColor(FG_STATUS),
        Print(pad_to(&bar, w)),
        ResetColor
    )?;
    Ok(())
}

fn render_message(ed: &Editor, out: &mut impl Write, w: usize, h: usize) -> io::Result<()> {
    queue!(out, MoveTo(0, (h - 1) as u16), ResetColor, Clear(ClearType::UntilNewLine))?;
    match ed.mode {
        Mode::Find | Mode::Goto | Mode::SaveAs => {
            queue!(
                out,
                SetForegroundColor(C_ACCENT),
                Print(ed.prompt_label()),
                SetForegroundColor(C_NORMAL),
                Print(&ed.input)
            )?;
        }
        _ => {
            if let Some((msg, at)) = &ed.status {
                if at.elapsed().as_secs() < 5 {
                    queue!(
                        out,
                        SetForegroundColor(C_NORMAL),
                        Print(pad_to(msg, w.saturating_sub(1)))
                    )?;
                    return Ok(());
                }
            }
            let hints = " ^S save   ^P open   ^F find   ^N new   ^W close   F1 help   ^Q quit";
            queue!(out, SetForegroundColor(FG_DIM), Print(pad_to(hints, w.saturating_sub(1))))?;
        }
    }
    Ok(())
}
