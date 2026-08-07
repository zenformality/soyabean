//! Coffee-themed palettes (Light + Dark) plus a Tokyo Night accent theme.
//! Applies clean Visuals to egui Context and exposes Palette constants for custom drawing.

use eframe::egui::{self, Color32, Rounding, Stroke};

#[derive(Clone, Copy, PartialEq, Eq, Default, Debug)]
pub enum Theme {
    #[default]
    Dark,
    Light,
    TokyoNight,
}

impl Theme {
    pub fn palette(self) -> &'static Palette {
        match self {
            Theme::Dark       => &DARK,
            Theme::Light      => &LIGHT,
            Theme::TokyoNight => &TOKYO_NIGHT,
        }
    }

    pub fn apply(self, ctx: &egui::Context) {
        let p = self.palette();
        let mut v = match self {
            Theme::Light => egui::Visuals::light(),
            _            => egui::Visuals::dark(),
        };

        v.override_text_color = Some(p.fg);
        v.window_fill          = p.panel_bg;
        v.panel_fill           = p.panel_bg;
        v.extreme_bg_color     = p.input_bg;
        v.faint_bg_color       = p.faint_bg;
        v.code_bg_color        = p.code_bg;
        v.selection.bg_fill    = p.selection;
        v.selection.stroke     = Stroke::NONE;
        v.window_rounding      = Rounding::same(10.0);
        v.window_stroke        = Stroke::new(1.0_f32, p.border);

        // non-interactive (labels, separators)
        let ni = &mut v.widgets.noninteractive;
        ni.bg_fill    = p.panel_bg;
        ni.fg_stroke  = Stroke::new(1.0_f32, p.text_dim);
        ni.bg_stroke  = Stroke::new(1.0_f32, p.border);
        ni.rounding   = Rounding::same(6.0);

        // idle buttons / inputs
        let ia = &mut v.widgets.inactive;
        ia.bg_fill   = p.button_bg;
        ia.fg_stroke = Stroke::new(1.0_f32, p.fg);
        ia.bg_stroke = Stroke::new(1.0_f32, p.border);
        ia.rounding  = Rounding::same(6.0);

        // hovered
        let ho = &mut v.widgets.hovered;
        ho.bg_fill   = p.button_hover;
        ho.fg_stroke = Stroke::new(1.5_f32, p.accent);
        ho.bg_stroke = Stroke::new(1.0_f32, p.accent);
        ho.rounding  = Rounding::same(6.0);

        // pressed / active
        let ac = &mut v.widgets.active;
        ac.bg_fill   = p.accent;
        ac.fg_stroke = Stroke::new(1.5_f32, Color32::WHITE);
        ac.bg_stroke = Stroke::new(1.0_f32, p.accent);
        ac.rounding  = Rounding::same(6.0);

        // open (combo/dropdown)
        let op = &mut v.widgets.open;
        op.bg_fill   = p.button_hover;
        op.fg_stroke = Stroke::new(1.0_f32, p.accent);

        ctx.set_visuals(v);
    }
}

pub struct Palette {
    // ─ Chrome ─
    pub bg:               Color32,
    pub panel_bg:         Color32,
    pub sidebar_bg:       Color32,
    pub input_bg:         Color32,
    pub faint_bg:         Color32,
    pub code_bg:          Color32,
    pub fg:               Color32,
    pub text_dim:         Color32,
    pub accent:           Color32,
    pub border:           Color32,
    pub button_bg:        Color32,
    pub button_hover:     Color32,
    pub selection:        Color32,
    pub tab_active_bg:    Color32,
    pub tab_inactive_bg:  Color32,
    pub welcome_title:    Color32,
    pub welcome_sub:      Color32,
    pub badge_bg:         Color32,
    // ─ Editor ─
    pub curline:          Color32,
    pub gutter_fg:        Color32,
    pub gutter_cur:       Color32,
    pub cursor_col:       Color32,
    pub word_hl:          Color32,
    // ─ Syntax ─
    pub syn_normal:       Color32,
    pub syn_keyword:      Color32,
    pub syn_type:         Color32,
    pub syn_str:          Color32,
    pub syn_comment:      Color32,
    pub syn_number:       Color32,
    pub syn_func:         Color32,
    pub syn_punct:        Color32,
}

// ── Dark Coffee ─────────────────────────────────────────────────────────────────

pub static DARK: Palette = Palette {
    bg:              Color32::from_rgb(28,  16,  8  ),
    panel_bg:        Color32::from_rgb(37,  21,  8  ),
    sidebar_bg:      Color32::from_rgb(30,  16,  6  ),
    input_bg:        Color32::from_rgb(20,  10,  4  ),
    faint_bg:        Color32::from_rgb(33,  18,  7  ),
    code_bg:         Color32::from_rgb(30,  16,  6  ),
    fg:              Color32::from_rgb(232, 213, 183),
    text_dim:        Color32::from_rgb(107, 80,  60 ),
    accent:          Color32::from_rgb(196, 120, 48 ),
    border:          Color32::from_rgb(60,  32,  16 ),
    button_bg:       Color32::from_rgb(51,  32,  16 ),
    button_hover:    Color32::from_rgb(74,  48,  24 ),
    selection:       Color32::from_rgb(77,  40,  8  ),
    tab_active_bg:   Color32::from_rgb(28,  16,  8  ),
    tab_inactive_bg: Color32::from_rgb(37,  21,  8  ),
    welcome_title:   Color32::from_rgb(196, 120, 48 ),
    welcome_sub:     Color32::from_rgb(107, 80,  60 ),
    badge_bg:        Color32::from_rgb(51,  32,  16 ),
    curline:         Color32::from_rgb(40,  22,  8  ),
    gutter_fg:       Color32::from_rgb(92,  64,  48 ),
    gutter_cur:      Color32::from_rgb(196, 168, 130),
    cursor_col:      Color32::from_rgb(196, 120, 48 ),
    word_hl:         Color32::from_rgba_premultiplied(34, 21, 8, 44),
    syn_normal:      Color32::from_rgb(232, 213, 183),
    syn_keyword:     Color32::from_rgb(232, 147, 106),
    syn_type:        Color32::from_rgb(124, 192, 110),
    syn_str:         Color32::from_rgb(212, 149, 106),
    syn_comment:     Color32::from_rgb(122, 96,  80 ),
    syn_number:      Color32::from_rgb(212, 175, 55 ),
    syn_func:        Color32::from_rgb(200, 160, 224),
    syn_punct:       Color32::from_rgb(168, 152, 120),
};

// ── Light Coffee ────────────────────────────────────────────────────────────────

pub static LIGHT: Palette = Palette {
    bg:              Color32::from_rgb(247, 239, 215),
    panel_bg:        Color32::from_rgb(237, 224, 196),
    sidebar_bg:      Color32::from_rgb(230, 215, 184),
    input_bg:        Color32::from_rgb(253, 248, 236),
    faint_bg:        Color32::from_rgb(243, 232, 205),
    code_bg:         Color32::from_rgb(230, 215, 184),
    fg:              Color32::from_rgb(44,  26,  14 ),
    text_dim:        Color32::from_rgb(140, 110, 80 ),
    accent:          Color32::from_rgb(139, 69,  19 ),
    border:          Color32::from_rgb(196, 168, 122),
    button_bg:       Color32::from_rgb(222, 199, 154),
    button_hover:    Color32::from_rgb(206, 168, 100),
    selection:       Color32::from_rgb(210, 165, 100),
    tab_active_bg:   Color32::from_rgb(247, 239, 215),
    tab_inactive_bg: Color32::from_rgb(230, 215, 184),
    welcome_title:   Color32::from_rgb(139, 69,  19 ),
    welcome_sub:     Color32::from_rgb(140, 110, 80 ),
    badge_bg:        Color32::from_rgb(222, 199, 154),
    curline:         Color32::from_rgb(236, 213, 168),
    gutter_fg:       Color32::from_rgb(160, 128, 96 ),
    gutter_cur:      Color32::from_rgb(44,  26,  14 ),
    cursor_col:      Color32::from_rgb(139, 69,  19 ),
    word_hl:         Color32::from_rgba_premultiplied(22, 11, 3, 40),
    syn_normal:      Color32::from_rgb(44,  26,  14 ),
    syn_keyword:     Color32::from_rgb(139, 0,   0  ),
    syn_type:        Color32::from_rgb(0,   107, 79 ),
    syn_str:         Color32::from_rgb(123, 56,  0  ),
    syn_comment:     Color32::from_rgb(122, 96,  69 ),
    syn_number:      Color32::from_rgb(112, 64,  0  ),
    syn_func:        Color32::from_rgb(91,  45,  140),
    syn_punct:       Color32::from_rgb(96,  78,  56 ),
};

// ── Tokyo Night (Cyberpunk Dark) ─────────────────────────────────────────────

pub static TOKYO_NIGHT: Palette = Palette {
    bg:              Color32::from_rgb(26,  27,  38 ),
    panel_bg:        Color32::from_rgb(31,  35,  53 ),
    sidebar_bg:      Color32::from_rgb(22,  22,  30 ),
    input_bg:        Color32::from_rgb(18,  19,  26 ),
    faint_bg:        Color32::from_rgb(36,  40,  59 ),
    code_bg:         Color32::from_rgb(26,  27,  38 ),
    fg:              Color32::from_rgb(169, 177, 214),
    text_dim:        Color32::from_rgb(86,  95,  137),
    accent:          Color32::from_rgb(125, 207, 255), // Cyan
    border:          Color32::from_rgb(41,  46,  66 ),
    button_bg:       Color32::from_rgb(41,  46,  66 ),
    button_hover:    Color32::from_rgb(65,  72,  104),
    selection:       Color32::from_rgb(51,  70,  117),
    tab_active_bg:   Color32::from_rgb(31,  35,  53 ),
    tab_inactive_bg: Color32::from_rgb(22,  22,  30 ),
    welcome_title:   Color32::from_rgb(187, 154, 247),
    welcome_sub:     Color32::from_rgb(125, 207, 255),
    badge_bg:        Color32::from_rgb(41,  46,  66 ),
    curline:         Color32::from_rgb(41,  46,  66 ),
    gutter_fg:       Color32::from_rgb(86,  95,  137),
    gutter_cur:      Color32::from_rgb(192, 202, 245),
    cursor_col:      Color32::from_rgb(247, 118, 142),
    word_hl:         Color32::from_rgba_premultiplied(187, 154, 247, 45),
    syn_normal:      Color32::from_rgb(192, 202, 245),
    syn_keyword:     Color32::from_rgb(187, 154, 247),
    syn_type:        Color32::from_rgb(42,  195, 222),
    syn_str:         Color32::from_rgb(158, 206, 106),
    syn_comment:     Color32::from_rgb(86,  95,  137),
    syn_number:      Color32::from_rgb(255, 158, 100),
    syn_func:        Color32::from_rgb(122, 162, 247),
    syn_punct:       Color32::from_rgb(137, 221, 254),
};
