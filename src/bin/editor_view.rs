//! Theme-aware editor pane: renders a Buffer with syntax colours, current-line
//! highlight, selection and a blinking cursor. Input is handled by the app.

use eframe::egui::text::LayoutJob;
use eframe::egui::{
    self, pos2, vec2, Align2, Color32, Id, Pos2, Rect, Rounding, Sense, Stroke, TextFormat,
    TextStyle,
};
use soyabean::buffer::{ch_width, cx_at_vcol, visual_col, Buffer};
use soyabean::editor::{Editor, Mode};
use soyabean::syntax::{self, Tok};

use super::theme::Palette;

fn tok_color(t: Tok, p: &Palette) -> Color32 {
    match t {
        Tok::Normal => p.syn_normal,
        Tok::Keyword => p.syn_keyword,
        Tok::Type => p.syn_type,
        Tok::Str => p.syn_str,
        Tok::Comment => p.syn_comment,
        Tok::Number => p.syn_number,
        Tok::Func => p.syn_func,
        Tok::Punct => p.syn_punct,
    }
}

pub fn show(ui: &mut egui::Ui, ed: &mut Editor, p: &Palette, font_scale: f32) {
    let id = Id::new("soyabean-editor");
    ed.buf_mut().ensure_syntax();

    let desired = ui.available_size();
    let (rect, _resp) = ui.allocate_exact_size(desired, Sense::hover());

    let fonts = ui.fonts(|f| f.clone());
    let mut font_id = TextStyle::Monospace.resolve(ui.style());
    font_id.size = (font_id.size * font_scale).max(6.0);
    let row_h = ui.fonts(|f| f.row_height(&font_id));
    let cw = ui.fonts(|f| f.glyph_width(&font_id, 'W')).max(1.0);

    let gw = ed.gutter_w();
    let text_w_chars = ((rect.width() / cw) as usize).saturating_sub(gw).max(1);
    let rows = ((rect.height() / row_h).floor() as usize).max(1);
    ed.set_size(gw + text_w_chars, rows + 2);

    let text_rect = Rect::from_min_size(rect.min, vec2(rect.width(), rows as f32 * row_h));
    let content_left = rect.left() + gw as f32 * cw;

    // Word under the cursor → highlight all its occurrences.
    let hl_word = if ed.mode == Mode::Edit {
        word_at_cursor(ed.buf())
    } else {
        None
    };

    // ── mouse ─────────────────────────────────────────────────────────────
    if ed.mode == Mode::Edit {
        let resp = ui.interact(text_rect, id, Sense::click_and_drag());
        // I-beam cursor when hovering the editor text area
        if resp.hovered() {
            ui.ctx()
                .output_mut(|o| o.cursor_icon = egui::CursorIcon::Text);
        }
        let mut clicked = false;
        if resp.clicked() || resp.drag_started() {
            if let Some(pos) = resp.interact_pointer_pos() {
                cursor_from_pos(ed, pos, text_rect, gw as f32, cw, row_h, false);
                clicked = true;
            }
        } else if resp.dragged() {
            if let Some(pos) = resp.interact_pointer_pos() {
                cursor_from_pos(ed, pos, text_rect, gw as f32, cw, row_h, true);
                clicked = true;
            }
        }
        if clicked {
            ui.ctx().memory_mut(|m| m.request_focus(id));
        }
        if resp.hovered() {
            let scroll = ui.input(|i| i.raw_scroll_delta.y);
            if scroll.abs() > 0.0 {
                scroll_rows(ed, -scroll, rows);
            }
        }
    }

    // ── paint ─────────────────────────────────────────────────────────────
    let painter = ui.painter_at(text_rect);

    // Background fill
    painter.rect_filled(text_rect, 0.0, p.bg);

    // Gutter border line
    painter.line_segment(
        [
            pos2(content_left - 4.0, text_rect.top()),
            pos2(content_left - 4.0, text_rect.bottom()),
        ],
        Stroke::new(1.0_f32, p.border),
    );

    let b = ed.buf();
    let sel = b.sel_range();
    let col_off = b.col_off;

    for row in 0..rows {
        let y = b.row_off + row;
        let y_px = text_rect.top() + row as f32 * row_h;

        if y >= b.lines.len() {
            painter.text(
                pos2(text_rect.left() + 4.0, y_px),
                Align2::LEFT_TOP,
                "~",
                font_id.clone(),
                p.text_dim,
            );
            continue;
        }

        // current-line highlight
        if y == b.cy && sel.is_none() {
            painter.rect_filled(
                Rect::from_min_max(
                    pos2(text_rect.left(), y_px),
                    pos2(text_rect.right(), y_px + row_h),
                ),
                Rounding::same(2.0),
                p.curline,
            );
        }

        let line = &b.lines[y];

        // highlight every occurrence of the word under the cursor
        if let Some(w) = &hl_word {
            let mut from = 0usize;
            while let Some(rel) = line[from..].find(w) {
                let b = from + rel;
                let e = b + w.len();
                let ci = line[..b].chars().count();
                let v0 = visual_col(line, ci);
                let v1 = v0 + w.chars().count();
                if v1 > col_off && v0 < col_off + text_w_chars {
                    let x0 = content_left + (v0 as f32 - col_off as f32) * cw;
                    let x1 = content_left + (v1 as f32 - col_off as f32) * cw;
                    painter.rect_filled(
                        Rect::from_min_max(pos2(x0, y_px), pos2(x1, y_px + row_h)),
                        Rounding::same(2.0),
                        p.word_hl,
                    );
                }
                from = e;
                if from >= line.len() {
                    break;
                }
            }
        }
        let state = b.line_states.get(y).copied().unwrap_or_default();
        let (toks, _) = syntax::highlight_line(line, b.lang, state);

        // Gutter
        let gcol = if y == b.cy { p.gutter_cur } else { p.gutter_fg };
        let mut gj = LayoutJob::default();
        let num = format!("{:>width$} ", y + 1, width = gw - 1);
        gj.append(
            &num,
            0.0,
            TextFormat {
                font_id: font_id.clone(),
                color: gcol,
                ..Default::default()
            },
        );
        let gg = fonts.layout_job(gj);
        painter.galley(pos2(text_rect.left() + 2.0, y_px), gg, gcol);

        // Content
        let in_sel = |ci: usize| -> bool {
            match sel {
                Some(((sy, sx), (ey, ex))) => {
                    (y > sy || (y == sy && ci >= sx)) && (y < ey || (y == ey && ci < ex))
                }
                None => false,
            }
        };

        let mut cj = LayoutJob::default();
        let mut vcol = 0usize;
        let mut first_vcol: Option<usize> = None;
        for (ci, ch) in line.chars().enumerate() {
            let cw_ch = ch_width(ch, vcol);
            if vcol + cw_ch <= col_off {
                vcol += cw_ch;
                continue;
            }
            if vcol >= col_off + text_w_chars {
                break;
            }
            if first_vcol.is_none() {
                first_vcol = Some(vcol);
            }
            let fg = tok_color(toks.get(ci).copied().unwrap_or(Tok::Normal), p);
            let bg = if in_sel(ci) {
                p.selection
            } else {
                Color32::TRANSPARENT
            };
            let s: String = if ch == '\t' {
                " ".repeat(cw_ch)
            } else {
                ch.to_string()
            };
            cj.append(
                &s,
                0.0,
                TextFormat {
                    font_id: font_id.clone(),
                    color: fg,
                    background: bg,
                    ..Default::default()
                },
            );
            vcol += cw_ch;
        }
        let galley = fonts.layout_job(cj);
        let x = content_left + (first_vcol.unwrap_or(0) as f32 - col_off as f32) * cw;
        let clip = Rect::from_min_max(
            pos2(content_left, text_rect.top()),
            pos2(text_rect.right(), text_rect.bottom()),
        );
        painter
            .with_clip_rect(clip)
            .galley(pos2(x, y_px), galley, p.syn_normal);
    }

    // ── cursor ────────────────────────────────────────────────────────────
    if ed.mode == Mode::Edit {
        let b = ed.buf();
        let vc = visual_col(&b.lines[b.cy], b.cx);
        if b.cy >= b.row_off
            && b.cy < b.row_off + rows
            && vc >= col_off
            && vc < col_off + text_w_chars
        {
            let t = ui.input(|i| i.time);
            // Slower blink: ~1.2 Hz period (833ms) like VS Code
            if (t * 1.2).fract() < 0.5 {
                let x = content_left + (vc as f32 - col_off as f32) * cw;
                let y = text_rect.top() + (b.cy - b.row_off) as f32 * row_h;
                painter.rect_filled(
                    Rect::from_min_size(pos2(x, y), vec2(2.5, row_h)),
                    Rounding::same(1.0),
                    p.cursor_col,
                );
            }
        }
    }
}

fn cursor_from_pos(
    ed: &mut Editor,
    pos: Pos2,
    text_rect: Rect,
    gw: f32,
    cw: f32,
    row_h: f32,
    sel: bool,
) {
    let row = ((pos.y - text_rect.top()) / row_h).floor() as i64;
    let b = ed.buf_mut();
    let max = b.lines.len() as i64 - 1;
    let cy = (b.row_off as i64 + row).clamp(0, max.max(0)) as usize;
    let x = pos.x - (text_rect.left() + gw * cw);
    let col = (x / cw).floor() as i64 + b.col_off as i64;
    let cx = cx_at_vcol(&b.lines[cy], col.max(0) as usize);
    b.set_cursor(cy, cx, sel);
    ed.ensure_visible();
}

fn scroll_rows(ed: &mut Editor, delta_rows: f32, rows: usize) {
    let b = ed.buf_mut();
    let max = b.lines.len().saturating_sub(rows);
    b.row_off = (b.row_off as i64 + delta_rows.round() as i64).clamp(0, max as i64) as usize;
}

fn is_id(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

fn word_at_cursor(b: &Buffer) -> Option<String> {
    let line = b.cur_line();
    let chars: Vec<char> = line.chars().collect();
    if chars.is_empty() {
        return None;
    }
    let i = b.cx.min(chars.len() - 1);
    if !is_id(chars[i]) {
        return None;
    }
    let mut s = i;
    while s > 0 && is_id(chars[s - 1]) {
        s -= 1;
    }
    let mut e = i;
    while e < chars.len() && is_id(chars[e]) {
        e += 1;
    }
    let w: String = chars[s..e].iter().collect();
    if w.chars().count() <= 2 {
        return None;
    }
    Some(w)
}
