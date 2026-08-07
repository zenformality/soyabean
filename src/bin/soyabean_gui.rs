//! soyabean — native GUI entry point.
//! Modernized UI inspired by VS Code and Zed with integrated terminal support,
//! folder & file creation context menus, themes (Zed Dark, Light, Tokyo Night),
//! welcome screen, command palette, and credits to zenx.

mod editor_view;
mod file_tree;
mod term;
mod theme;

use std::path::PathBuf;
use std::time::Duration;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use eframe::egui::{self, FontId, Margin, RichText, Rounding, Stroke, Vec2};
use soyabean::buffer::visual_col;
use soyabean::editor::{self, Editor, Mode};

use theme::{Palette, Theme};

// ── App state ────────────────────────────────────────────────────────────────

#[derive(Clone, Copy)]
enum Cmd {
    Key(KeyCode, KeyModifiers),
    SaveAs,
    ZoomIn,
    ZoomOut,
    ResetZoom,
    ToggleTheme,
    ToggleTerminal,
    ExternalTerminal,
    Welcome,
    About,
    Shortcuts,
    RefreshTree,
    NewFileModal,
    NewFolderModal,
}

#[derive(Clone, Copy)]
enum FindAction {
    Next,
    Prev,
    Replace,
    ReplaceAll,
}

struct GuiApp {
    ed:             Editor,
    tree:           file_tree::FileTree,
    terminal:       Option<term::Terminal>,
    show_terminal:  bool,
    theme:          Theme,
    show_welcome:   bool,
    show_about:     bool,
    show_shortcuts: bool,
    confirm_close:  Option<usize>,
    logo_tex:       Option<egui::TextureHandle>,
    recent_files:   Vec<PathBuf>,
    font_scale:     f32,
    show_palette:   bool,
    palette_query:  String,
    palette_sel:    usize,
    prev_mode:      Mode,
    find_focus_req: bool,
    cmd_input:      String,
    cmd_history:    Vec<String>,
    cmd_history_idx: usize,
    term_input_focused: bool,
}

impl GuiApp {
    fn new(args: &[String]) -> Self {
        let ed = editor::Editor::new(args)
            .unwrap_or_else(|_| editor::Editor::new(&[]).expect("editor init"));
        let mut tree = file_tree::FileTree::default();
        tree.refresh(&ed.root);
        let terminal = None;
        let show_terminal = false;
        let show_welcome = args.is_empty() || ed.bufs.iter().all(|b| b.path.is_none());
        GuiApp {
            ed,
            tree,
            terminal,
            show_terminal,
            theme: Theme::Dark,
            show_welcome,
            show_about: false,
            show_shortcuts: false,
            confirm_close: None,
            logo_tex: None,
            recent_files: Vec::new(),
            font_scale: 1.0,
            show_palette: false,
            palette_query: String::new(),
            palette_sel: 0,
            prev_mode: Mode::Edit,
            find_focus_req: false,
            cmd_input: String::new(),
            cmd_history: Vec::new(),
            cmd_history_idx: 0,
            term_input_focused: false,
        }
    }

    // ── logo loading ──────────────────────────────────────────────────────

    fn ensure_logo(&mut self, ctx: &egui::Context) {
        if self.logo_tex.is_none() {
            let bytes = include_bytes!("../../logo.png");
            if let Ok(img) = image::load_from_memory(bytes) {
                let rgba = img.to_rgba8();
                let (w, h) = rgba.dimensions();
                let color_img = egui::ColorImage::from_rgba_unmultiplied(
                    [w as usize, h as usize], &rgba.into_raw(),
                );
                self.logo_tex = Some(ctx.load_texture(
                    "soyabean-logo", color_img, egui::TextureOptions::default(),
                ));
            }
        }
    }

    // ── input ─────────────────────────────────────────────────────────────

    fn handle_events(&mut self, ctx: &egui::Context, events: &[egui::Event], mods: egui::Modifiers) {
        if self.ed.mode != self.prev_mode {
            self.find_focus_req = matches!(self.ed.mode, Mode::Find | Mode::Replace);
        }
        self.prev_mode = self.ed.mode;

        let has_copy  = events.iter().any(|e| matches!(e, egui::Event::Copy));
        let has_cut   = events.iter().any(|e| matches!(e, egui::Event::Cut));
        let has_paste = events.iter().any(|e| matches!(e, egui::Event::Paste(_)));

        // ── command palette (modal) ───────────────────────────────────────
        if self.show_palette {
            for e in events {
                if let egui::Event::Key { key, pressed: true, .. } = e {
                    match key {
                        egui::Key::ArrowUp => {
                            self.palette_sel = self.palette_sel.saturating_sub(1);
                        }
                        egui::Key::ArrowDown => {
                            let n = self.palette_matches().len();
                            self.palette_sel = (self.palette_sel + 1).min(n.saturating_sub(1));
                        }
                        egui::Key::Enter => {
                            self.show_palette = false;
                            if let Some((_, c)) = self.palette_matches().get(self.palette_sel) {
                                self.exec_cmd(*c);
                            }
                        }
                        egui::Key::Escape => self.show_palette = false,
                        _ => {}
                    }
                }
            }
            return;
        }

        // ── find / replace overlay (modal) ────────────────────────────────
        if matches!(self.ed.mode, Mode::Find | Mode::Replace) {
            for e in events {
                if let egui::Event::Key { key, pressed: true, modifiers, .. } = e {
                    let ctrl = modifiers.ctrl || modifiers.command;
                    let shift = modifiers.shift;
                    match key {
                        egui::Key::Escape => {
                            self.ed.on_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
                        }
                        egui::Key::Enter => {
                            if self.ed.mode == Mode::Replace {
                                if ctrl {
                                    self.ed.replace_all();
                                } else {
                                    self.ed.replace_current();
                                }
                            } else {
                                self.ed.find_next_step();
                            }
                        }
                        egui::Key::F3 => {
                            let m = if shift { KeyModifiers::SHIFT } else { KeyModifiers::NONE };
                            self.ed.on_key(KeyEvent::new(KeyCode::F(3), m));
                        }
                        egui::Key::ArrowUp => self.ed.find_prev_step(),
                        egui::Key::ArrowDown => self.ed.find_next_step(),
                        _ => {}
                    }
                }
            }
            return;
        }

        // ── normal editing ────────────────────────────────────────────────
        // When the terminal UI (command bar or terminal body) has focus, input
        // must not reach the editor buffer.
        let term_body_focused = self.show_terminal
            && self.terminal.as_ref().map_or(false, |t| t.focused);
        let term_ui_focused = self.show_terminal && self.term_input_focused;
        for e in events {
            // Ctrl+` still toggles the terminal closed even while it is focused.
            let is_term_toggle = matches!(e,
                egui::Event::Key { key: egui::Key::Backtick, pressed: true, modifiers, .. }
                if modifiers.ctrl || modifiers.command);
            if !is_term_toggle && (term_body_focused || term_ui_focused) {
                if term_body_focused {
                    if let Some(t) = &mut self.terminal {
                        t.on_keys(&[e.clone()]);
                    }
                }
                continue;
            }
            match e {
                egui::Event::Key { key, pressed: true, modifiers, .. } => {
                    let ctrl = modifiers.ctrl || modifiers.command;
                    if ctrl && *key == egui::Key::C && !has_copy { self.do_copy(ctx, false); continue; }
                    if ctrl && *key == egui::Key::X && !has_cut  { self.do_copy(ctx, true);  continue; }
                    if ctrl && *key == egui::Key::V && !has_paste {
                        let t = self.ed.clipboard_text().to_string();
                        if t.is_empty() { self.ed.msg("Clipboard is empty"); }
                        else { self.ed.on_paste(&t); }
                        continue;
                    }
                    if ctrl && *key == egui::Key::P && modifiers.shift {
                        self.open_palette();
                        continue;
                    }
                    if ctrl && *key == egui::Key::Backtick {
                        self.show_terminal = !self.show_terminal;
                        continue;
                    }
                    if ctrl && *key == egui::Key::S && modifiers.shift {
                        self.ed.input.clear();
                        self.ed.mode = Mode::SaveAs;
                        continue;
                    }
                    if ctrl && *key == egui::Key::T && modifiers.shift {
                        self.show_terminal = !self.show_terminal;
                        continue;
                    }
                    if ctrl && (*key == egui::Key::Plus || *key == egui::Key::Equals) {
                        self.font_scale = (self.font_scale * 1.15).clamp(0.6, 2.5);
                        continue;
                    }
                    if ctrl && *key == egui::Key::Minus {
                        self.font_scale = (self.font_scale / 1.15).clamp(0.6, 2.5);
                        continue;
                    }
                    if ctrl && *key == egui::Key::Num0 {
                        self.font_scale = 1.0;
                        continue;
                    }
                    let has_mod = ctrl || modifiers.alt;
                    if let Some(ck) = to_ckey(*key, *modifiers) {
                        let printable = matches!(ck.code, KeyCode::Char(ch) if is_printable_char(ch));
                        if has_mod || !printable {
                            self.ed.on_key(ck);
                        }
                    }
                }
                egui::Event::Copy        => self.do_copy(ctx, false),
                egui::Event::Cut         => self.do_copy(ctx, true),
                egui::Event::Paste(s)    => self.ed.on_paste(s),
                egui::Event::Text(s) => {
                    if mods.ctrl || mods.command || mods.alt { continue; }
                    for ch in s.chars() {
                        if ch == '\n' || ch == '\r' { continue; }
                        self.ed.on_key(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE));
                    }
                }
                _ => {}
            }
        }
    }

    fn do_copy(&mut self, ctx: &egui::Context, cut: bool) {
        let text = self.ed.copy(cut);
        if !text.is_empty() { ctx.copy_text(text); }
    }

    fn send_key(&mut self, code: KeyCode, mods: KeyModifiers) {
        self.ed.on_key(KeyEvent::new(code, mods));
    }

    // ── terminal command history ─────────────────────────────────────────

    fn recall_cmd(&mut self, delta: isize) {
        if self.cmd_history.is_empty() {
            return;
        }
        let len = self.cmd_history.len();
        let idx = if delta < 0 {
            self.cmd_history_idx.saturating_sub(1)
        } else {
            (self.cmd_history_idx + 1).min(len)
        };
        self.cmd_input = self.cmd_history.get(idx).cloned().unwrap_or_default();
        self.cmd_history_idx = idx;
    }

    // ── command palette ───────────────────────────────────────────────────

    fn open_palette(&mut self) {
        self.show_palette = true;
        self.palette_query.clear();
        self.palette_sel = 0;
    }

    fn command_list(&self) -> Vec<(String, Cmd)> {
        use KeyModifiers as M;
        let ctrl = M::CONTROL;
        let alt  = M::ALT;
        vec![
            ("Open File… (Ctrl+P)".to_string(),                Cmd::Key(KeyCode::Char('p'), ctrl)),
            ("New File (Ctrl+N)".to_string(),                  Cmd::Key(KeyCode::Char('n'), ctrl)),
            ("New File in Directory…".to_string(),             Cmd::NewFileModal),
            ("New Folder in Directory…".to_string(),           Cmd::NewFolderModal),
            ("Save (Ctrl+S)".to_string(),                      Cmd::Key(KeyCode::Char('s'), ctrl)),
            ("Save As… (Ctrl+Shift+S)".to_string(),            Cmd::SaveAs),
            ("Close Buffer (Ctrl+W)".to_string(),              Cmd::Key(KeyCode::Char('w'), ctrl)),
            ("Toggle Integrated Terminal (Ctrl+`)".to_string(),Cmd::ToggleTerminal),
            ("Open OS Terminal Window".to_string(),            Cmd::ExternalTerminal),
            ("Find… (Ctrl+F)".to_string(),                     Cmd::Key(KeyCode::Char('f'), ctrl)),
            ("Find & Replace… (Ctrl+H)".to_string(),           Cmd::Key(KeyCode::Char('h'), ctrl)),
            ("Go to Line… (Ctrl+G)".to_string(),               Cmd::Key(KeyCode::Char('g'), ctrl)),
            ("Toggle Line Comment (Ctrl+/)".to_string(),       Cmd::Key(KeyCode::Char('/'), ctrl)),
            ("Duplicate Line (Ctrl+D)".to_string(),            Cmd::Key(KeyCode::Char('d'), ctrl)),
            ("Delete Line (Ctrl+K)".to_string(),               Cmd::Key(KeyCode::Char('k'), ctrl)),
            ("Move Line Up (Alt+↑)".to_string(),               Cmd::Key(KeyCode::Up, alt)),
            ("Move Line Down (Alt+↓)".to_string(),             Cmd::Key(KeyCode::Down, alt)),
            ("Undo (Ctrl+Z)".to_string(),                      Cmd::Key(KeyCode::Char('z'), ctrl)),
            ("Redo (Ctrl+Y)".to_string(),                      Cmd::Key(KeyCode::Char('y'), ctrl)),
            ("Next Buffer (Alt+→)".to_string(),                Cmd::Key(KeyCode::Right, alt)),
            ("Previous Buffer (Alt+←)".to_string(),            Cmd::Key(KeyCode::Left, alt)),
            ("Zoom In (Ctrl+=)".to_string(),                   Cmd::ZoomIn),
            ("Zoom Out (Ctrl+-)".to_string(),                  Cmd::ZoomOut),
            ("Reset Zoom (Ctrl+0)".to_string(),                Cmd::ResetZoom),
            ("Switch Theme (Dark / Light / Tokyo)".to_string(),Cmd::ToggleTheme),
            ("Refresh File Tree".to_string(),                  Cmd::RefreshTree),
            ("Welcome Screen".to_string(),                     Cmd::Welcome),
            ("About soyabean".to_string(),                     Cmd::About),
            ("Keyboard Shortcuts (F1)".to_string(),            Cmd::Shortcuts),
        ]
    }

    fn palette_matches(&self) -> Vec<(String, Cmd)> {
        let mut scored: Vec<(i64, String, Cmd)> = self.command_list().into_iter()
            .filter_map(|(label, c)| {
                let l = label.clone();
                soyabean::finder::fuzzy_score(&self.palette_query, &l).map(|s| (s, l, c))
            })
            .collect();
        scored.sort_by(|a, b| b.0.cmp(&a.0));
        scored.into_iter().map(|(_, l, c)| (l, c)).collect()
    }

    fn exec_cmd(&mut self, c: Cmd) {
        match c {
            Cmd::Key(code, mods) => self.send_key(code, mods),
            Cmd::SaveAs => {
                self.ed.input.clear();
                self.ed.mode = Mode::SaveAs;
            }
            Cmd::NewFileModal => {
                self.tree.pending_action = Some(file_tree::FileAction::NewFile {
                    parent: self.ed.root.clone(),
                    input: String::new(),
                });
            }
            Cmd::NewFolderModal => {
                self.tree.pending_action = Some(file_tree::FileAction::NewFolder {
                    parent: self.ed.root.clone(),
                    input: String::new(),
                });
            }
            Cmd::ZoomIn => self.font_scale = (self.font_scale * 1.15).clamp(0.6, 2.5),
            Cmd::ZoomOut => self.font_scale = (self.font_scale / 1.15).clamp(0.6, 2.5),
            Cmd::ResetZoom => self.font_scale = 1.0,
            Cmd::ToggleTheme => {
                self.theme = match self.theme {
                    Theme::Dark => Theme::Light,
                    Theme::Light => Theme::TokyoNight,
                    Theme::TokyoNight => Theme::Dark,
                };
            }
            Cmd::ToggleTerminal => self.show_terminal = !self.show_terminal,
            Cmd::ExternalTerminal => launch_terminal(),
            Cmd::Welcome => self.show_welcome = true,
            Cmd::About => self.show_about = true,
            Cmd::Shortcuts => self.show_shortcuts = true,
            Cmd::RefreshTree => {
                let root = self.ed.root.clone();
                self.tree.refresh(&root);
            }
        }
    }

    // ── menu bar ───────────────────────────────────────────────────────────

    fn menu_bar(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let p = self.theme.palette();
        ui.visuals_mut().override_text_color = Some(p.fg);
        ui.spacing_mut().item_spacing.x = 2.0;

        let mk = |label: &str| RichText::new(label).size(12.5).color(p.fg);

        egui::menu::bar(ui, |ui| {
            ui.menu_button(mk("File"), |ui| {
                ui.set_min_width(230.0);
                if ui.button("Open File…\tCtrl+P").clicked() {
                    self.send_key(KeyCode::Char('p'), KeyModifiers::CONTROL);
                    ui.close_menu();
                }
                if ui.button("New Buffer\tCtrl+N").clicked() {
                    self.send_key(KeyCode::Char('n'), KeyModifiers::CONTROL);
                    ui.close_menu();
                }
                if ui.button("New File in Directory…").clicked() {
                    self.tree.pending_action = Some(file_tree::FileAction::NewFile {
                        parent: self.ed.root.clone(),
                        input: String::new(),
                    });
                    ui.close_menu();
                }
                if ui.button("New Folder in Directory…").clicked() {
                    self.tree.pending_action = Some(file_tree::FileAction::NewFolder {
                        parent: self.ed.root.clone(),
                        input: String::new(),
                    });
                    ui.close_menu();
                }
                ui.separator();
                if ui.button("Save\tCtrl+S").clicked() {
                    self.send_key(KeyCode::Char('s'), KeyModifiers::CONTROL);
                    ui.close_menu();
                }
                if ui.button("Save As…\tCtrl+Shift+S").clicked() {
                    self.ed.input.clear();
                    self.ed.mode = Mode::SaveAs;
                    ui.close_menu();
                }
                ui.separator();
                if ui.button("Close Buffer\tCtrl+W").clicked() {
                    self.send_key(KeyCode::Char('w'), KeyModifiers::CONTROL);
                    ui.close_menu();
                }
                ui.separator();
                if ui.button("Exit").clicked() {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
            });

            ui.menu_button(mk("Edit"), |ui| {
                ui.set_min_width(230.0);
                if ui.button("Undo\tCtrl+Z").clicked() {
                    self.send_key(KeyCode::Char('z'), KeyModifiers::CONTROL);
                    ui.close_menu();
                }
                if ui.button("Redo\tCtrl+Y").clicked() {
                    self.send_key(KeyCode::Char('y'), KeyModifiers::CONTROL);
                    ui.close_menu();
                }
                ui.separator();
                if ui.button("Cut").clicked() {
                    self.do_copy(ctx, true);
                    ui.close_menu();
                }
                if ui.button("Copy").clicked() {
                    self.do_copy(ctx, false);
                    ui.close_menu();
                }
                if ui.button("Paste").clicked() {
                    let t = self.ed.clipboard_text().to_string();
                    if t.is_empty() { self.ed.msg("Clipboard is empty"); }
                    else { self.ed.on_paste(&t); }
                    ui.close_menu();
                }
                ui.separator();
                if ui.button("Find…\tCtrl+F").clicked() {
                    self.send_key(KeyCode::Char('f'), KeyModifiers::CONTROL);
                    ui.close_menu();
                }
                if ui.button("Find & Replace…\tCtrl+H").clicked() {
                    self.send_key(KeyCode::Char('h'), KeyModifiers::CONTROL);
                    ui.close_menu();
                }
                if ui.button("Go to Line…\tCtrl+G").clicked() {
                    self.send_key(KeyCode::Char('g'), KeyModifiers::CONTROL);
                    ui.close_menu();
                }
                ui.separator();
                if ui.button("Toggle Line Comment\tCtrl+/").clicked() {
                    self.send_key(KeyCode::Char('/'), KeyModifiers::CONTROL);
                    ui.close_menu();
                }
                if ui.button("Duplicate Line\tCtrl+D").clicked() {
                    self.send_key(KeyCode::Char('d'), KeyModifiers::CONTROL);
                    ui.close_menu();
                }
                if ui.button("Delete Line\tCtrl+K").clicked() {
                    self.send_key(KeyCode::Char('k'), KeyModifiers::CONTROL);
                    ui.close_menu();
                }
                if ui.button("Move Line Up\tAlt+↑").clicked() {
                    self.send_key(KeyCode::Up, KeyModifiers::ALT);
                    ui.close_menu();
                }
                if ui.button("Move Line Down\tAlt+↓").clicked() {
                    self.send_key(KeyCode::Down, KeyModifiers::ALT);
                    ui.close_menu();
                }
            });

            ui.menu_button(mk("View"), |ui| {
                ui.set_min_width(200.0);
                if ui.button("Command Palette\tCtrl+Shift+P").clicked() {
                    self.open_palette();
                    ui.close_menu();
                }
                if ui.button("Zoom In\tCtrl+Plus").clicked() {
                    self.font_scale = (self.font_scale * 1.15).clamp(0.6, 2.5);
                    ui.close_menu();
                }
                if ui.button("Zoom Out\tCtrl+Minus").clicked() {
                    self.font_scale = (self.font_scale / 1.15).clamp(0.6, 2.5);
                    ui.close_menu();
                }
                if ui.button("Reset Zoom\tCtrl+0").clicked() {
                    self.font_scale = 1.0;
                    ui.close_menu();
                }
                ui.separator();
                if ui.button("Toggle Theme").clicked() {
                    self.theme = match self.theme {
                        Theme::Dark => Theme::Light,
                        Theme::Light => Theme::TokyoNight,
                        Theme::TokyoNight => Theme::Dark,
                    };
                    ui.close_menu();
                }
                if ui.button("Refresh File Tree").clicked() {
                    let root = self.ed.root.clone();
                    self.tree.refresh(&root);
                    ui.close_menu();
                }
                if ui.button("Welcome Screen").clicked() {
                    self.show_welcome = true;
                    ui.close_menu();
                }
            });

            ui.menu_button(mk("Terminal"), |ui| {
                if ui.button("Toggle Integrated Terminal\tCtrl+`").clicked() {
                    self.show_terminal = !self.show_terminal;
                    ui.close_menu();
                }
                if ui.button("Open OS Terminal Window").clicked() {
                    launch_terminal();
                    ui.close_menu();
                }
            });

            ui.menu_button(mk("Help"), |ui| {
                if ui.button("Shortcuts\tF1").clicked() {
                    self.show_shortcuts = true;
                    ui.close_menu();
                }
                if ui.button("About soyabean").clicked() {
                    self.show_about = true;
                    ui.close_menu();
                }
            });
        });
    }

    // ── toolbar ───────────────────────────────────────────────────────────

    fn toolbar(&mut self, ui: &mut egui::Ui) {
        let p = self.theme.palette();
        ui.visuals_mut().override_text_color = Some(p.fg);

        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;

            ui.add_space(4.0);
            ui.label(RichText::new("🌱 soyabean").color(p.accent).size(15.0).strong());
            ui.add(egui::Separator::default().vertical());

            if icon_btn(ui, "🔍", "Command Palette (Ctrl+Shift+P)", p).clicked() {
                self.open_palette();
            }

            ui.add(egui::Separator::default().vertical());

            if icon_btn(ui, "📂", "Open File (Ctrl+P)", p).clicked() {
                self.send_key(KeyCode::Char('p'), KeyModifiers::CONTROL);
            }
            if icon_btn(ui, "💾", "Save (Ctrl+S)", p).clicked() {
                self.send_key(KeyCode::Char('s'), KeyModifiers::CONTROL);
            }
            if icon_btn(ui, "📄+", "New Buffer (Ctrl+N)", p).clicked() {
                self.send_key(KeyCode::Char('n'), KeyModifiers::CONTROL);
            }
            if icon_btn(ui, "✕", "Close Buffer (Ctrl+W)", p).clicked() {
                self.send_key(KeyCode::Char('w'), KeyModifiers::CONTROL);
            }

            ui.add(egui::Separator::default().vertical());

            if icon_btn(ui, "⟲", "Undo (Ctrl+Z)", p).clicked() {
                self.send_key(KeyCode::Char('z'), KeyModifiers::CONTROL);
            }
            if icon_btn(ui, "⟳", "Redo (Ctrl+Y)", p).clicked() {
                self.send_key(KeyCode::Char('y'), KeyModifiers::CONTROL);
            }

            ui.add(egui::Separator::default().vertical());

            if icon_btn(ui, "🔎", "Find (Ctrl+F)", p).clicked() {
                self.send_key(KeyCode::Char('f'), KeyModifiers::CONTROL);
            }
            if icon_btn(ui, "⇄", "Find & Replace (Ctrl+H)", p).clicked() {
                self.send_key(KeyCode::Char('h'), KeyModifiers::CONTROL);
            }

            ui.add(egui::Separator::default().vertical());

            let term_lbl = if self.show_terminal { "🖥  Terminal ✓" } else { "🖥  Terminal" };
            if ui.add(egui::Button::new(RichText::new(term_lbl).size(12.0).color(p.fg))
                .fill(if self.show_terminal { p.button_hover } else { p.button_bg }))
                .on_hover_text("Toggle Integrated Terminal (Ctrl+`)")
                .clicked()
            {
                self.show_terminal = !self.show_terminal;
            }

            ui.add(egui::Separator::default().vertical());

            let theme_lbl = match self.theme {
                Theme::Dark => "🌙 Dark",
                Theme::Light => "☀ Light",
                Theme::TokyoNight => "🌌 Tokyo",
            };
            if ui.add(egui::Button::new(RichText::new(theme_lbl).size(12.0).color(p.fg)))
                .on_hover_text("Switch Theme (Dark / Light / Tokyo Night)")
                .clicked()
            {
                self.theme = match self.theme {
                    Theme::Dark => Theme::Light,
                    Theme::Light => Theme::TokyoNight,
                    Theme::TokyoNight => Theme::Dark,
                };
            }

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if icon_btn(ui, "🏠", "Welcome Screen", p).clicked() {
                    self.show_welcome = true;
                }
                if icon_btn(ui, "ℹ", "About soyabean", p).clicked() {
                    self.show_about = true;
                }
                if icon_btn(ui, "⌨", "Shortcuts (F1)", p).clicked() {
                    self.show_shortcuts = true;
                }
            });
        });
    }

    // ── tab bar ───────────────────────────────────────────────────────────

    fn tab_bar(&mut self, ui: &mut egui::Ui) {
        let p = self.theme.palette();
        let tabs: Vec<(usize, String, bool)> = self.ed.bufs.iter().enumerate()
            .map(|(i, b)| (i, b.display_name(), b.dirty && !b.is_scratch))
            .collect();

        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 3.0;
            let mut click: Option<usize> = None;
            let mut close: Option<usize> = None;

            for (i, name, dirty) in &tabs {
                let active = *i == self.ed.cur;
                let bg = if active { p.tab_active_bg } else { p.tab_inactive_bg };
                let fg = if active { p.accent } else { p.text_dim };
                let text = format!("{}{}", if *dirty { "● " } else { "" }, name);

                egui::Frame::none()
                    .fill(bg)
                    .inner_margin(Margin::symmetric(10.0, 5.0))
                    .rounding(Rounding { nw: 6.0, ne: 6.0, sw: 0.0, se: 0.0 })
                    .stroke(Stroke::new(1.0_f32, if active { p.accent } else { p.border }))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            if ui.selectable_label(active, RichText::new(&text).color(fg).strong()).clicked() {
                                click = Some(*i);
                            }
                            if ui.small_button(RichText::new("×").color(p.text_dim)).clicked() {
                                close = Some(*i);
                            }
                        });
                    });
            }

            if ui.small_button(RichText::new("  +  ").color(p.text_dim)).clicked() {
                self.ed.bufs.push(soyabean::buffer::Buffer::empty());
                self.ed.cur = self.ed.bufs.len() - 1;
            }

            if let Some(i) = click  { self.ed.cur = i; }
            if let Some(i) = close  { self.close_buffer(i); }
        });
    }

    fn close_buffer(&mut self, i: usize) {
        let dirty = self.ed.bufs.get(i).is_some_and(|b| b.dirty && !b.is_scratch);
        if dirty && self.confirm_close != Some(i) {
            self.confirm_close = Some(i);
            self.ed.msg("Unsaved changes — click close again to discard");
            return;
        }
        self.confirm_close = None;
        self.ed.bufs.remove(i);
        if self.ed.bufs.is_empty() { self.ed.bufs.push(soyabean::buffer::Buffer::empty()); }
        if self.ed.cur >= self.ed.bufs.len() { self.ed.cur = self.ed.bufs.len() - 1; }
    }

    // ── breadcrumbs ───────────────────────────────────────────────────────

    fn breadcrumbs(&mut self, ui: &mut egui::Ui) {
        let p = self.theme.palette();
        ui.horizontal(|ui| {
            let b = self.ed.buf();
            match &b.path {
                Some(path) => {
                    let rel = path.strip_prefix(&self.ed.root).unwrap_or(path);
                    ui.label(RichText::new(format!("🗂  {}", rel.display()))
                        .size(11.0).color(p.text_dim));
                }
                None => {
                    ui.label(RichText::new("🗂  untitled").size(11.0).color(p.text_dim));
                }
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.colored_label(p.text_dim,
                    format!("Zoom: {}%", (self.font_scale * 100.0).round() as i32));
            });
        });
    }

    // ── status bar ────────────────────────────────────────────────────────

    fn status_bar(&mut self, ui: &mut egui::Ui) {
        let p = self.theme.palette();
        ui.horizontal(|ui| {
            match self.ed.mode {
                Mode::Find | Mode::Replace | Mode::Goto | Mode::SaveAs => {
                    ui.colored_label(p.accent, self.ed.prompt_label());
                    ui.colored_label(p.fg,     self.ed.input.clone());
                }
                _ => {
                    let show_hint = self.ed.status.as_ref()
                        .map_or(true, |(_, t)| t.elapsed().as_secs() >= 5);
                    if let Some((msg, at)) = &self.ed.status {
                        if at.elapsed().as_secs() < 5 {
                            ui.colored_label(p.fg, msg.clone());
                        }
                    }
                    if show_hint {
                        ui.colored_label(p.text_dim,
                            " ^S save  ^P open  ^F find  ^N new  ^` terminal  F1 help");
                    }
                }
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let b   = self.ed.buf();
                let vc  = visual_col(&b.lines[b.cy], b.cx);
                let wc: usize = b.lines.iter().map(|l| l.split_whitespace().count()).sum();
                ui.colored_label(p.text_dim, format!("{} words", wc));
                ui.colored_label(p.text_dim, "|");
                ui.colored_label(p.text_dim, format!("Ln {}, Col {}", b.cy + 1, vc + 1));
                ui.colored_label(p.text_dim, "|");
                ui.colored_label(p.text_dim, if b.crlf { "CRLF" } else { "LF" });
                ui.colored_label(p.text_dim, "|");
                ui.colored_label(p.accent,   b.lang.name);
                ui.colored_label(p.text_dim, "|");
                let dirty = if b.dirty { " ●" } else { "" };
                ui.colored_label(p.fg, format!("{}{}", b.display_name(), dirty));
            });
        });
    }

    // ── finder overlay ────────────────────────────────────────────────────

    fn show_finder_overlay(&mut self, ctx: &egui::Context) {
        let p   = self.theme.palette();
        let root = self.ed.root.clone();
        let mut sel: Option<usize> = None;

        egui::Window::new("🔍  Open file")
            .id(egui::Id::new("finder-overlay"))
            .anchor(egui::Align2::CENTER_TOP, [0.0, 40.0])
            .collapsible(false)
            .resizable(false)
            .default_width(540.0)
            .frame(egui::Frame::window(ctx.style().as_ref())
                .fill(p.panel_bg)
                .stroke(Stroke::new(1.0_f32, p.accent))
                .rounding(Rounding::same(10.0))
                .inner_margin(Margin::same(14.0)))
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.colored_label(p.accent, "▸ ");
                    ui.colored_label(p.fg, self.ed.finder.query.clone());
                    ui.add_space(8.0);
                    ui.colored_label(p.text_dim,
                        format!("{} / {} files", self.ed.finder.matched.len(), self.ed.finder.files.len()));
                });
                ui.add(egui::Separator::default().horizontal());
                egui::ScrollArea::vertical().max_height(300.0).show(ui, |ui| {
                    for i in 0..self.ed.finder.matched.len() {
                        let idx  = self.ed.finder.matched[i];
                        let path = &self.ed.finder.files[idx];
                        let active = i == self.ed.finder.sel;
                        let fg = if active { p.accent } else { p.fg };
                        let fname_start = path.rfind('/').map(|x| x + 1).unwrap_or(0);
                        let (dir, name) = path.split_at(fname_start);
                        let label = egui::text::LayoutJob::simple(
                            format!("{}{}", dir, name), FontId::monospace(13.0), fg, 0.0);
                        if ui.selectable_label(active, label).clicked() {
                            sel = Some(i);
                        }
                    }
                });
                ui.add_space(4.0);
                ui.colored_label(p.text_dim, "↑↓ navigate  Enter open  Esc cancel");
            });

        if let Some(i) = sel {
            self.ed.finder.sel = i;
            self.ed.mode = Mode::Edit;
            if let Some(p2) = self.ed.finder.selected_path(&root) {
                self.track_recent(p2.clone());
                self.ed.open_file(p2);
            }
        }
    }

    fn track_recent(&mut self, path: PathBuf) {
        self.recent_files.retain(|p| p != &path);
        self.recent_files.insert(0, path);
        self.recent_files.truncate(8);
    }

    // ── find / replace overlay ────────────────────────────────────────────

    fn show_find_replace_overlay(&mut self, ctx: &egui::Context) {
        let p = self.theme.palette();
        let replacing = self.ed.mode == Mode::Replace;
        let mut q   = self.ed.input.clone();
        let mut rep = self.ed.replace_input.clone();
        let mut action: Option<FindAction> = None;

        let title = if replacing { "🔍  Find & Replace" } else { "🔍  Find" };
        egui::Window::new(title)
            .id(egui::Id::new("find-replace-overlay"))
            .title_bar(false)
            .anchor(egui::Align2::CENTER_TOP, [0.0, 40.0])
            .collapsible(false)
            .resizable(false)
            .default_width(580.0)
            .frame(egui::Frame::window(ctx.style().as_ref())
                .fill(p.panel_bg)
                .stroke(Stroke::new(1.0_f32, p.accent))
                .rounding(Rounding::same(10.0))
                .inner_margin(Margin::same(14.0)))
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.colored_label(p.text_dim, "Find");
                    ui.add_space(6.0);
                    let resp = ui.add(egui::TextEdit::singleline(&mut q)
                        .id(egui::Id::new("find-input"))
                        .desired_width(460.0)
                        .text_color(p.fg));
                    if self.find_focus_req {
                        resp.request_focus();
                    }
                });
                if replacing {
                    ui.add_space(4.0);
                    ui.horizontal(|ui| {
                        ui.colored_label(p.text_dim, "Repl");
                        ui.add_space(6.0);
                        ui.add(egui::TextEdit::singleline(&mut rep)
                            .id(egui::Id::new("replace-input"))
                            .desired_width(460.0)
                            .text_color(p.fg));
                    });
                }
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    let matches = self.ed.buf().count_matches(&q);
                    ui.colored_label(p.text_dim, format!("{matches} matches"));
                    ui.add_space(10.0);
                    if ui.button(RichText::new("◀ Prev").color(p.fg)).clicked() {
                        action = Some(FindAction::Prev);
                    }
                    if ui.button(RichText::new("Next ▶").color(p.fg)).clicked() {
                        action = Some(FindAction::Next);
                    }
                    if replacing {
                        if ui.button(RichText::new("Replace").color(p.fg)).clicked() {
                            action = Some(FindAction::Replace);
                        }
                        if ui.button(RichText::new("Replace All").color(p.fg)).clicked() {
                            action = Some(FindAction::ReplaceAll);
                        }
                    }
                });
                ui.add_space(4.0);
                ui.colored_label(p.text_dim, if replacing {
                    "Enter replace & next   Ctrl+Enter replace all   F3 next   Shift+F3 prev   Esc close"
                } else {
                    "Enter / F3 next   Shift+F3 prev   Esc close"
                });
            });

        self.find_focus_req = false;
        if q != self.ed.input {
            self.ed.set_find_query(&q);
        }
        if rep != self.ed.replace_input {
            self.ed.set_replace_query(&rep);
        }
        match action {
            Some(FindAction::Next) => self.ed.find_next_step(),
            Some(FindAction::Prev) => self.ed.find_prev_step(),
            Some(FindAction::Replace) => self.ed.replace_current(),
            Some(FindAction::ReplaceAll) => self.ed.replace_all(),
            None => {}
        }
    }

    // ── command palette overlay ───────────────────────────────────────────

    fn show_command_palette(&mut self, ctx: &egui::Context) {
        let p = self.theme.palette();
        let matches = self.palette_matches();
        self.palette_sel = self.palette_sel.min(matches.len().saturating_sub(1));
        let mut run: Option<Cmd> = None;

        egui::Window::new("palette")
            .id(egui::Id::new("command-palette"))
            .title_bar(false)
            .anchor(egui::Align2::CENTER_TOP, [0.0, 40.0])
            .collapsible(false)
            .resizable(false)
            .default_width(540.0)
            .frame(egui::Frame::window(ctx.style().as_ref())
                .fill(p.panel_bg)
                .stroke(Stroke::new(1.0_f32, p.accent))
                .rounding(Rounding::same(10.0))
                .inner_margin(Margin::same(14.0)))
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.colored_label(p.accent, "❯ ");
                    let resp = ui.add(egui::TextEdit::singleline(&mut self.palette_query)
                        .id(egui::Id::new("palette-input"))
                        .desired_width(460.0)
                        .text_color(p.fg));
                    resp.request_focus();
                });
                ui.add(egui::Separator::default().horizontal());
                egui::ScrollArea::vertical().max_height(320.0).show(ui, |ui| {
                    for (i, (label, cmd)) in matches.iter().enumerate() {
                        let active = i == self.palette_sel;
                        let fg = if active { p.accent } else { p.fg };
                        if ui.selectable_label(active, RichText::new(label).color(fg)).clicked() {
                            run = Some(*cmd);
                        }
                    }
                });
                ui.add_space(4.0);
                ui.colored_label(p.text_dim, "↑↓ navigate  Enter run  Esc close");
            });

        if let Some(c) = run {
            self.exec_cmd(c);
        }
    }

    // ── welcome screen ────────────────────────────────────────────────────

    fn show_welcome_screen(&mut self, ctx: &egui::Context) {
        let p = self.theme.palette();
        egui::Window::new("welcome")
            .title_bar(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .collapsible(false)
            .resizable(false)
            .default_width(580.0)
            .frame(egui::Frame::window(ctx.style().as_ref())
                .fill(p.panel_bg)
                .stroke(Stroke::new(1.5_f32, p.accent))
                .rounding(Rounding::same(16.0))
                .inner_margin(Margin::same(30.0)))
            .show(ctx, |ui| {
                ui.vertical_centered(|ui| {
                    if let Some(tex) = &self.logo_tex {
                        let size = Vec2::new(88.0, 88.0);
                        ui.add(egui::Image::new((tex.id(), size)));
                        ui.add_space(8.0);
                    } else {
                        ui.label(RichText::new("🌱").size(64.0));
                        ui.add_space(4.0);
                    }

                    ui.label(RichText::new("soyabean").size(32.0).strong().color(p.welcome_title));
                    ui.label(RichText::new("Next-Gen Fast & Lightweight Code IDE").size(14.0).color(p.welcome_sub));
                    ui.add_space(4.0);
                    ui.label(RichText::new("crafted by zenx").size(12.0).italics().color(p.text_dim));
                    ui.add_space(16.0);

                    ui.label(RichText::new("Theme Selection").size(13.0).strong().color(p.fg));
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        ui.add_space(40.0);
                        if ui.add(egui::Button::new(RichText::new("🌙 Dark").size(13.0))
                            .fill(if self.theme == Theme::Dark { p.accent } else { p.button_bg })
                            .min_size(Vec2::new(140.0, 32.0))).clicked()
                        {
                            self.theme = Theme::Dark;
                        }
                        ui.add_space(8.0);
                        if ui.add(egui::Button::new(RichText::new("☀ Light").size(13.0))
                            .fill(if self.theme == Theme::Light { p.accent } else { p.button_bg })
                            .min_size(Vec2::new(140.0, 32.0))).clicked()
                        {
                            self.theme = Theme::Light;
                        }
                        ui.add_space(8.0);
                        if ui.add(egui::Button::new(RichText::new("🌌 Tokyo").size(13.0))
                            .fill(if self.theme == Theme::TokyoNight { p.accent } else { p.button_bg })
                            .min_size(Vec2::new(140.0, 32.0))).clicked()
                        {
                            self.theme = Theme::TokyoNight;
                        }
                    });

                    ui.add_space(18.0);
                    ui.add(egui::Separator::default().horizontal());
                    ui.add_space(12.0);

                    ui.label(RichText::new("Quick Actions").size(13.0).strong().color(p.fg));
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        ui.add_space(40.0);
                        if ui.add(egui::Button::new(RichText::new("📂 Open File").size(13.0))
                            .min_size(Vec2::new(150.0, 34.0))).clicked() {
                            self.show_welcome = false;
                            self.send_key(KeyCode::Char('p'), KeyModifiers::CONTROL);
                        }
                        ui.add_space(10.0);
                        if ui.add(egui::Button::new(RichText::new("📄 New File").size(13.0))
                            .min_size(Vec2::new(150.0, 34.0))).clicked() {
                            self.show_welcome = false;
                            self.send_key(KeyCode::Char('n'), KeyModifiers::CONTROL);
                        }
                        ui.add_space(10.0);
                        if ui.add(egui::Button::new(RichText::new("🖥 Terminal").size(13.0))
                            .min_size(Vec2::new(150.0, 34.0))).clicked() {
                            self.show_welcome = false;
                            self.show_terminal = true;
                        }
                    });

                    let recents = self.recent_files.clone();
                    if !recents.is_empty() {
                        ui.add_space(14.0);
                        ui.label(RichText::new("Recent Files").size(13.0).strong().color(p.fg));
                        ui.add_space(4.0);
                        let mut open: Option<PathBuf> = None;
                        for path in &recents {
                            let name = path.file_name()
                                .map(|n| n.to_string_lossy().to_string())
                                .unwrap_or_default();
                            if ui.link(RichText::new(format!("  📄 {}", name)).color(p.accent)).clicked() {
                                open = Some(path.clone());
                            }
                        }
                        if let Some(p2) = open {
                            self.show_welcome = false;
                            self.ed.open_file(p2);
                        }
                    }

                    ui.add_space(16.0);
                    ui.add(egui::Separator::default().horizontal());
                    ui.add_space(10.0);

                    if ui.add(egui::Button::new(
                            RichText::new("  Start Coding  →  ").size(14.0).strong())
                        .fill(p.accent)
                        .min_size(Vec2::new(220.0, 38.0))).clicked()
                    {
                        self.show_welcome = false;
                    }
                });
            });
    }

    // ── about dialog ──────────────────────────────────────────────────────

    fn show_about_dialog(&mut self, ctx: &egui::Context) {
        let p = self.theme.palette();
        let mut open = self.show_about;
        egui::Window::new("About soyabean")
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .default_width(360.0)
            .frame(egui::Frame::window(ctx.style().as_ref())
                .fill(p.panel_bg)
                .stroke(Stroke::new(1.0_f32, p.accent))
                .rounding(Rounding::same(12.0))
                .inner_margin(Margin::same(24.0)))
            .show(ctx, |ui| {
                ui.vertical_centered(|ui| {
                    if let Some(tex) = &self.logo_tex {
                        ui.add(egui::Image::new((tex.id(), Vec2::new(72.0, 72.0))));
                        ui.add_space(8.0);
                    } else {
                        ui.label(RichText::new("🌱").size(48.0));
                    }
                    ui.label(RichText::new("soyabean").size(24.0).strong().color(p.accent));
                    ui.label(RichText::new("v0.2.0 Modernized Edition").size(12.0).color(p.text_dim));
                    ui.add_space(10.0);
                    ui.label(RichText::new("A fast, modern code IDE with integrated terminal & folder tools")
                        .size(13.0).color(p.fg));
                    ui.add_space(8.0);
                    ui.add(egui::Separator::default().horizontal());
                    ui.add_space(8.0);
                    ui.label(RichText::new("crafted by  zenx  ⚡").size(14.0).strong().color(p.accent));
                    ui.add_space(4.0);
                    ui.label(RichText::new("Built with Rust + egui + portable-pty").size(11.0).color(p.text_dim));
                    ui.add_space(12.0);
                });
            });
        self.show_about = open;
    }

    // ── keyboard shortcuts ────────────────────────────────────────────────

    fn show_shortcuts_dialog(&mut self, ctx: &egui::Context) {
        let p = self.theme.palette();
        let mut open = self.show_shortcuts;
        egui::Window::new("⌨  Keyboard Shortcuts")
            .open(&mut open)
            .collapsible(false)
            .resizable(true)
            .default_size([440.0, 520.0])
            .frame(egui::Frame::window(ctx.style().as_ref())
                .fill(p.panel_bg)
                .stroke(Stroke::new(1.0_f32, p.border))
                .rounding(Rounding::same(10.0))
                .inner_margin(Margin::same(16.0)))
            .show(ctx, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    shortcut_section(ui, p, "Files & Project", &[
                        ("Ctrl+P",          "Fuzzy file opener"),
                        ("Ctrl+Shift+P",    "Command palette"),
                        ("Ctrl+S",          "Save file"),
                        ("Ctrl+N",          "New buffer"),
                        ("Ctrl+W",          "Close buffer"),
                        ("Right-click tree","New File, Folder, Rename, Delete"),
                        ("Alt+← / →",       "Previous / next buffer"),
                    ]);
                    shortcut_section(ui, p, "Terminal", &[
                        ("Ctrl+` / Ctrl+~", "Toggle integrated terminal"),
                        ("Ctrl+Shift+T",    "Toggle integrated terminal"),
                        ("Ctrl+C in Term",  "Interrupt command / kill"),
                    ]);
                    shortcut_section(ui, p, "Editing", &[
                        ("Ctrl+Z / Y",      "Undo / redo"),
                        ("Ctrl+C / X / V",  "Copy / cut / paste"),
                        ("Ctrl+D",          "Select word / duplicate line"),
                        ("Ctrl+K",          "Delete line"),
                        ("Ctrl+/",          "Toggle line comment"),
                        ("Alt+↑ / ↓",       "Move line up / down"),
                        ("Tab / Shift+Tab", "Indent / dedent selection"),
                    ]);
                    shortcut_section(ui, p, "Search & Go", &[
                        ("Ctrl+F",          "Find overlay"),
                        ("Ctrl+H",          "Find & replace"),
                        ("F3 / Shift+F3",   "Find next / previous"),
                        ("Ctrl+G",          "Go to line"),
                    ]);
                });
            });
        self.show_shortcuts = open;
    }
}

// ── eframe::App ───────────────────────────────────────────────────────────────

impl eframe::App for GuiApp {
    fn raw_input_hook(&mut self, _ctx: &egui::Context, raw_input: &mut egui::RawInput) {
        // egui-winit swallows Ctrl+C/Ctrl+X into `Event::Copy`/`Event::Cut` before
        // the terminal can see them. When the terminal has focus, restore them as
        // real key events so the shell receives SIGINT / 0x18.
        let term_focused = self.show_terminal
            && self.terminal.as_ref().map_or(false, |t| t.focused);
        if !term_focused {
            return;
        }
        for e in &mut raw_input.events {
            let key = match e {
                egui::Event::Copy => egui::Key::C,
                egui::Event::Cut => egui::Key::X,
                _ => continue,
            };
            *e = egui::Event::Key {
                key,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::COMMAND,
            };
        }
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.ensure_logo(ctx);
        self.theme.apply(ctx);

        if ctx.input(|i| i.key_pressed(egui::Key::F1)) {
            self.show_shortcuts = true;
        }

        let events = ctx.input(|i| i.events.clone());
        let mods   = ctx.input(|i| i.modifiers);
        if !self.show_welcome {
            self.handle_events(ctx, &events, mods);
        }

        let p = self.theme.palette();

        // ── Menu bar ──────────────────────────────────────────────────
        egui::TopBottomPanel::top("menubar")
            .frame(egui::Frame::none().fill(p.panel_bg)
                .inner_margin(Margin::symmetric(6.0, 2.0))
                .stroke(Stroke::new(1.0_f32, p.border)))
            .show(ctx, |ui| {
                self.menu_bar(ui, ctx);
            });

        // ── Top bar (toolbar) ─────────────────────────────────────────
        egui::TopBottomPanel::top("toolbar")
            .frame(egui::Frame::none().fill(p.panel_bg)
                .inner_margin(Margin::symmetric(6.0, 5.0))
                .stroke(Stroke::new(1.0_f32, p.border)))
            .show(ctx, |ui| {
                self.toolbar(ui);
            });

        // ── Tabs panel ────────────────────────────────────────────────
        egui::TopBottomPanel::top("tabs")
            .frame(egui::Frame::none().fill(p.sidebar_bg)
                .inner_margin(Margin::symmetric(6.0, 3.0)))
            .show(ctx, |ui| {
                self.tab_bar(ui);
            });

        // ── Status bar ────────────────────────────────────────────────
        egui::TopBottomPanel::bottom("status")
            .frame(egui::Frame::none().fill(p.panel_bg)
                .inner_margin(Margin::symmetric(8.0, 4.0))
                .stroke(Stroke::new(1.0_f32, p.border)))
            .show(ctx, |ui| {
                self.status_bar(ui);
            });

        // ── Integrated Terminal Bottom Drawer ──────────────────────────
        if self.show_terminal {
            if self.terminal.is_none() {
                let root = self.ed.root.clone();
                let ctx = ctx.clone();
                self.terminal = term::Terminal::spawn(&root, &ctx);
            }
            egui::TopBottomPanel::bottom("integrated_terminal")
                .resizable(true)
                .default_height(220.0)
                .min_height(90.0)
                .frame(egui::Frame::none().fill(p.panel_bg)
                    .inner_margin(Margin::symmetric(6.0, 4.0))
                    .stroke(Stroke::new(1.0_f32, p.border)))
                .show(ctx, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("TERMINAL").size(11.0).color(p.text_dim));
                        if let Some(t) = &self.terminal {
                            ui.label(RichText::new(&t.title).size(11.0).color(p.text_dim));
                        }
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.small_button("✕").on_hover_text("Close terminal").clicked() {
                                if let Some(t) = &mut self.terminal {
                                    t.kill();
                                }
                                self.terminal = None;
                                self.show_terminal = false;
                                self.term_input_focused = false;
                            }
                        });
                        if let Some(t) = &self.terminal {
                            if t.is_exited() {
                                ui.label(RichText::new("process exited").size(11.0).color(p.text_dim));
                            }
                        }
                    });
                    ui.separator();

                    // ── Command bar: cwd + command input ─────────────────
                    let cwd = self.terminal.as_ref().map(|t| t.cwd.display().to_string());
                    let mut submit = false;
                    let mut recall_at_end = false;
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("❯").size(15.0).color(p.accent));
                        if let Some(c) = &cwd {
                            ui.add_sized(Vec2::new(240.0, 18.0),
                                egui::Label::new(RichText::new(c).size(11.5).color(p.text_dim))
                                    .truncate())
                                .on_hover_text(c);
                        }
                        if self.term_input_focused {
                            let up = ui.input(|i| i.key_pressed(egui::Key::ArrowUp));
                            let down = ui.input(|i| i.key_pressed(egui::Key::ArrowDown));
                            if up {
                                self.recall_cmd(-1);
                                recall_at_end = true;
                            } else if down {
                                self.recall_cmd(1);
                                recall_at_end = true;
                            }
                        }
                        let edit_w = (ui.available_width() - 30.0).max(80.0);
                        let resp = ui.add_sized(Vec2::new(edit_w, 20.0),
                            egui::TextEdit::singleline(&mut self.cmd_input)
                                .font(egui::TextStyle::Monospace)
                                .hint_text("Type a command… (Enter to run)")
                                .cursor_at_end(recall_at_end));
                        self.term_input_focused = resp.has_focus();
                        if resp.has_focus() {
                            if ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                                submit = true;
                            }
                            if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                                ui.memory_mut(|m| m.request_focus(egui::Id::new("soyabean-terminal")));
                            }
                            if ui.input(|i| i.events.iter().any(|e| matches!(e, egui::Event::Copy))) {
                                self.cmd_input.clear();
                            }
                        }
                        if ui.add(egui::Button::new(RichText::new("▶").size(13.0))
                            .min_size(Vec2::new(24.0, 20.0))).clicked()
                        {
                            submit = true;
                        }
                    });
                    if submit {
                        let cmd = self.cmd_input.trim().to_string();
                        if !cmd.is_empty() {
                            let mut line = cmd.clone();
                            line.push('\r');
                            line.push('\n');
                            if let Some(t) = &mut self.terminal {
                                t.write(line.as_bytes());
                            }
                            self.cmd_history.push(cmd);
                            self.cmd_history_idx = self.cmd_history.len();
                            self.cmd_input.clear();
                        }
                    }
                    ui.separator();
                    if let Some(t) = &mut self.terminal {
                        t.show(ui, p);
                    }
                });
        }

        // ── File tree sidebar ─────────────────────────────────────────
        egui::SidePanel::left("tree")
            .resizable(true)
            .default_width(220.0)
            .min_width(140.0)
            .frame(egui::Frame::none().fill(p.sidebar_bg)
                .inner_margin(Margin::same(8.0))
                .stroke(Stroke::new(1.0_f32, p.border)))
            .show(ctx, |ui| {
                self.tree.show(ui, &mut self.ed, p);
            });

        // ── Breadcrumbs ───────────────────────────────────────────────
        egui::TopBottomPanel::top("breadcrumbs")
            .frame(egui::Frame::none().fill(p.panel_bg)
                .inner_margin(Margin::symmetric(10.0, 4.0))
                .stroke(Stroke::new(1.0_f32, p.border)))
            .show(ctx, |ui| {
                self.breadcrumbs(ui);
            });

        // ── Editor pane ───────────────────────────────────────────────
        egui::CentralPanel::default()
            .frame(egui::Frame::none().fill(p.bg))
            .show(ctx, |ui| {
                editor_view::show(ui, &mut self.ed, p, self.font_scale);
            });

        // ── Overlays ──────────────────────────────────────────────────
        if self.ed.mode == Mode::Finder {
            self.show_finder_overlay(ctx);
        }
        if matches!(self.ed.mode, Mode::Find | Mode::Replace) {
            self.show_find_replace_overlay(ctx);
        }
        if self.show_palette {
            self.show_command_palette(ctx);
        }
        if self.show_welcome {
            self.show_welcome_screen(ctx);
        }
        if self.show_about {
            self.show_about_dialog(ctx);
        }
        if self.show_shortcuts {
            self.show_shortcuts_dialog(ctx);
        }

        ctx.request_repaint_after(Duration::from_millis(150));
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn icon_btn<'a>(ui: &mut egui::Ui, icon: &str, tip: &str, p: &Palette) -> egui::Response {
    ui.add(egui::Button::new(RichText::new(icon).size(14.0).color(p.fg))
        .fill(p.button_bg)
        .min_size(Vec2::new(28.0, 26.0)))
        .on_hover_text(tip)
}

fn shortcut_section(ui: &mut egui::Ui, p: &Palette, title: &str, rows: &[(&str, &str)]) {
    ui.add_space(6.0);
    ui.label(RichText::new(title).strong().color(p.accent).size(13.0));
    ui.add_space(4.0);
    for (key, desc) in rows {
        ui.horizontal(|ui| {
            egui::Frame::none()
                .fill(p.badge_bg)
                .rounding(Rounding::same(4.0))
                .inner_margin(Margin::symmetric(6.0, 2.0))
                .show(ui, |ui| {
                    ui.label(RichText::new(*key).size(11.0).color(p.fg)
                        .family(egui::FontFamily::Monospace));
                });
            ui.label(RichText::new(*desc).size(12.0).color(p.text_dim));
        });
    }
    ui.add(egui::Separator::default().horizontal());
}

fn launch_terminal() {
    let exe = std::env::current_exe().unwrap_or_default();
    let exe_str = exe.to_string_lossy().to_string();
    #[cfg(target_os = "windows")]
    let _ = std::process::Command::new("cmd")
        .args(["/c", "start", "cmd", "/k", &exe_str])
        .spawn();
    #[cfg(target_os = "macos")]
    let _ = std::process::Command::new("open")
        .args(["-a", "Terminal", &exe_str])
        .spawn();
    #[cfg(all(unix, not(target_os = "macos")))]
    let _ = std::process::Command::new("x-terminal-emulator")
        .args(["-e", &exe_str])
        .spawn();
}

fn to_ckey(key: egui::Key, mods: egui::Modifiers) -> Option<KeyEvent> {
    use egui::Key as K;
    let mut m = KeyModifiers::NONE;
    if mods.shift                  { m |= KeyModifiers::SHIFT;   }
    if mods.ctrl || mods.command   { m |= KeyModifiers::CONTROL; }
    if mods.alt                    { m |= KeyModifiers::ALT;     }
    let code = match key {
        K::A => KeyCode::Char('a'), K::B => KeyCode::Char('b'), K::C => KeyCode::Char('c'),
        K::D => KeyCode::Char('d'), K::E => KeyCode::Char('e'), K::F => KeyCode::Char('f'),
        K::G => KeyCode::Char('g'), K::H => KeyCode::Char('h'), K::I => KeyCode::Char('i'),
        K::J => KeyCode::Char('j'), K::K => KeyCode::Char('k'), K::L => KeyCode::Char('l'),
        K::M => KeyCode::Char('m'), K::N => KeyCode::Char('n'), K::O => KeyCode::Char('o'),
        K::P => KeyCode::Char('p'), K::Q => KeyCode::Char('q'), K::R => KeyCode::Char('r'),
        K::S => KeyCode::Char('s'), K::T => KeyCode::Char('t'), K::U => KeyCode::Char('u'),
        K::V => KeyCode::Char('v'), K::W => KeyCode::Char('w'), K::X => KeyCode::Char('x'),
        K::Y => KeyCode::Char('y'), K::Z => KeyCode::Char('z'),
        K::Num0 => KeyCode::Char('0'), K::Num1 => KeyCode::Char('1'),
        K::Num2 => KeyCode::Char('2'), K::Num3 => KeyCode::Char('3'),
        K::Num4 => KeyCode::Char('4'), K::Num5 => KeyCode::Char('5'),
        K::Num6 => KeyCode::Char('6'), K::Num7 => KeyCode::Char('7'),
        K::Num8 => KeyCode::Char('8'), K::Num9 => KeyCode::Char('9'),
        K::ArrowUp    => KeyCode::Up,        K::ArrowDown  => KeyCode::Down,
        K::ArrowLeft  => KeyCode::Left,      K::ArrowRight => KeyCode::Right,
        K::Escape     => KeyCode::Esc,       K::Tab        => KeyCode::Tab,
        K::Backspace  => KeyCode::Backspace, K::Enter      => KeyCode::Enter,
        K::Space      => KeyCode::Char(' '), K::Delete     => KeyCode::Delete,
        K::Home       => KeyCode::Home,      K::End        => KeyCode::End,
        K::PageUp     => KeyCode::PageUp,    K::PageDown   => KeyCode::PageDown,
        K::Insert     => KeyCode::Insert,
        K::F1  => KeyCode::F(1),  K::F2  => KeyCode::F(2),  K::F3  => KeyCode::F(3),
        K::F4  => KeyCode::F(4),  K::F5  => KeyCode::F(5),  K::F6  => KeyCode::F(6),
        K::F7  => KeyCode::F(7),  K::F8  => KeyCode::F(8),  K::F9  => KeyCode::F(9),
        K::F10 => KeyCode::F(10), K::F11 => KeyCode::F(11), K::F12 => KeyCode::F(12),
        K::Minus => KeyCode::Char('-'), K::Plus => KeyCode::Char('+'),
        K::Slash => KeyCode::Char('/'), K::Backslash => KeyCode::Char('\\'),
        K::Comma => KeyCode::Char(','), K::Period => KeyCode::Char('.'),
        K::Semicolon => KeyCode::Char(';'), K::Quote => KeyCode::Char('\''),
        _ => return None,
    };
    Some(KeyEvent::new(code, m))
}

fn is_printable_char(c: char) -> bool {
    !c.is_control() && !matches!(c, '\u{00AD}' | '\u{200B}' | '\u{FEFF}')
}

// ── Entry point ───────────────────────────────────────────────────────────────

fn main() -> eframe::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1280.0, 800.0])
            .with_min_inner_size([640.0, 420.0])
            .with_title("soyabean — Modern Code IDE by zenx"),
        ..Default::default()
    };
    eframe::run_native(
        "soyabean",
        options,
        Box::new(move |_cc| Ok(Box::new(GuiApp::new(&args)))),
    )
}
