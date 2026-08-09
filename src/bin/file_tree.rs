//! Project file tree with right-click context menus (New File, New Folder, Rename, Delete, Reveal in Explorer),
//! file creation modal dialogs, colorful file-type icons, and active buffer highlighting.
//! Modern VS Code-style explorer: full-width hoverable rows, chevrons, indentation.

use super::theme::Palette;
use eframe::egui::{self, pos2, Align2, Color32, FontId, RichText, Rounding, Sense, Vec2};
use soyabean::editor::Editor;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

const SKIP_DIRS: &[&str] = &[
    ".git",
    "target",
    "node_modules",
    "dist",
    "build",
    "__pycache__",
    ".venv",
    "venv",
    ".idea",
    ".vscode",
    "out",
    "bin",
    "obj",
];

const ROW_H: f32 = 26.0;
const INDENT: f32 = 14.0;

#[derive(Clone, Debug)]
pub enum FileAction {
    NewFile { parent: PathBuf, input: String },
    NewFolder { parent: PathBuf, input: String },
    Rename { target: PathBuf, input: String },
    DeleteConfirm { target: PathBuf },
}

#[derive(Clone, Debug)]
pub struct DragItem {
    pub path: PathBuf,
}

#[derive(Clone)]
pub struct Node {
    pub name: String,
    pub path: PathBuf,
    pub is_dir: bool,
    pub children: Vec<Node>,
}

pub struct FileTree {
    pub root_path: PathBuf,
    pub root_nodes: Vec<Node>,
    pub pending_action: Option<FileAction>,
    expanded: HashSet<PathBuf>,
}

impl Default for FileTree {
    fn default() -> Self {
        FileTree {
            root_path: PathBuf::new(),
            root_nodes: Vec::new(),
            pending_action: None,
            expanded: HashSet::new(),
        }
    }
}

impl FileTree {
    pub fn refresh(&mut self, root: &Path) {
        self.root_path = root.to_path_buf();
        self.root_nodes = build(root);
        self.expanded.retain(|p| p.starts_with(&self.root_path));
    }

    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        ed: &mut Editor,
        p: &Palette,
        media_slot: &mut Option<PathBuf>,
    ) {
        let current_path = ed.buf().path.clone();

        // ── Header: EXPLORER + quick actions ──────────────────────────────
        ui.add_space(2.0);
        let root_path = self.root_path.clone();
        ui.horizontal(|ui| {
            ui.label(
                RichText::new("EXPLORER")
                    .size(10.5)
                    .color(p.text_dim)
                    .strong(),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if icon_btn(ui, "⟳", "Refresh Tree", p).clicked() {
                    self.refresh(&root_path);
                }
                if icon_btn(ui, "🗁", "New Folder in Root", p).clicked() {
                    self.pending_action = Some(FileAction::NewFolder {
                        parent: root_path.clone(),
                        input: String::new(),
                    });
                }
                if icon_btn(ui, "🗎", "New File in Root", p).clicked() {
                    self.pending_action = Some(FileAction::NewFile {
                        parent: root_path.clone(),
                        input: String::new(),
                    });
                }
            });
        });

        // ── Workspace folder row ──────────────────────────────────────────
        let root_name = self
            .root_path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| self.root_path.display().to_string());
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.label(
                RichText::new(format!("🗀  {}", root_name))
                    .size(12.5)
                    .color(p.fg)
                    .strong(),
            );
        })
        .response
        .on_hover_text(self.root_path.display().to_string());
        ui.add_space(4.0);
        ui.add(egui::Separator::default().horizontal());
        ui.add_space(4.0);

        // ── Tree Nodes ────────────────────────────────────────────────────
        let mut new_action = None;
        let mut needs_refresh = false;
        egui::ScrollArea::vertical().show(ui, |ui| {
            show_nodes(
                ui,
                &self.root_nodes,
                ed,
                p,
                &current_path,
                &mut new_action,
                &mut self.expanded,
                0.0,
                &mut needs_refresh,
                media_slot,
            );

            // Empty space background right click for root dir actions
            let available = ui.available_rect_before_wrap();
            if available.height() > 20.0 {
                let bg_resp = ui.interact(available, ui.id().with("tree_bg"), Sense::click());
                // Dropping onto empty space moves the item to the workspace root.
                if let Some(payload) = bg_resp.dnd_release_payload::<DragItem>() {
                    let drag = (*payload).clone();
                    if perform_move(ed, &drag, &self.root_path) {
                        needs_refresh = true;
                    }
                }
                let root_path = self.root_path.clone();
                bg_resp.context_menu(|ui| {
                    ui.set_max_width(190.0);
                    if menu_btn(ui, "🗎  New File...", p).clicked() {
                        new_action = Some(FileAction::NewFile {
                            parent: root_path.clone(),
                            input: String::new(),
                        });
                        ui.close_menu();
                    }
                    if menu_btn(ui, "🗁  New Folder...", p).clicked() {
                        new_action = Some(FileAction::NewFolder {
                            parent: root_path.clone(),
                            input: String::new(),
                        });
                        ui.close_menu();
                    }
                    ui.add(egui::Separator::default().horizontal());
                    if menu_btn(ui, "📋  Copy Root Path", p).clicked() {
                        ui.ctx().copy_text(root_path.display().to_string());
                        ui.close_menu();
                    }
                    if menu_btn(ui, "📂  Reveal in Explorer", p).clicked() {
                        reveal_in_explorer(&root_path);
                        ui.close_menu();
                    }
                });
            }
        });

        if new_action.is_some() {
            self.pending_action = new_action;
        }

        if needs_refresh {
            let root = self.root_path.clone();
            self.refresh(&root);
        }

        // ── Action Dialog Modals (New File / New Folder / Rename / Delete) ───
        self.handle_action_dialog(ui.ctx(), ed, media_slot);
    }

    fn handle_action_dialog(
        &mut self,
        ctx: &egui::Context,
        ed: &mut Editor,
        media_slot: &mut Option<PathBuf>,
    ) {
        let root = ed.root.clone();
        let mut action_done = false;

        if let Some(action) = self.pending_action.clone() {
            let mut close_dialog = false;

            match action {
                FileAction::NewFile { parent, mut input } => {
                    egui::Window::new("🗎  New File")
                        .collapsible(false)
                        .resizable(false)
                        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                        .default_width(380.0)
                        .show(ctx, |ui| {
                            ui.label(
                                RichText::new(format!("Create in: {}", parent.display()))
                                    .size(11.0)
                                    .color(Color32::GRAY),
                            );
                            ui.add_space(6.0);
                            let resp = ui.add(
                                egui::TextEdit::singleline(&mut input)
                                    .hint_text("filename.rs")
                                    .desired_width(360.0),
                            );
                            resp.request_focus();

                            ui.add_space(10.0);
                            ui.horizontal(|ui| {
                                if ui.button("Create File").clicked()
                                    || (resp.lost_focus()
                                        && ctx.input(|i| i.key_pressed(egui::Key::Enter)))
                                {
                                    if !input.trim().is_empty() {
                                        let new_path = parent.join(input.trim());
                                        if let Some(dir) = new_path.parent() {
                                            let _ = std::fs::create_dir_all(dir);
                                        }
                                        if std::fs::File::create(&new_path).is_ok() {
                                            *media_slot = None;
                                            ed.open_file(new_path);
                                            action_done = true;
                                        }
                                    }
                                    close_dialog = true;
                                }
                                if ui.button("Cancel").clicked()
                                    || ctx.input(|i| i.key_pressed(egui::Key::Escape))
                                {
                                    close_dialog = true;
                                }
                            });
                        });
                    self.pending_action = Some(FileAction::NewFile { parent, input });
                }
                FileAction::NewFolder { parent, mut input } => {
                    egui::Window::new("🗁  New Folder")
                        .collapsible(false)
                        .resizable(false)
                        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                        .default_width(380.0)
                        .show(ctx, |ui| {
                            ui.label(
                                RichText::new(format!("Create in: {}", parent.display()))
                                    .size(11.0)
                                    .color(Color32::GRAY),
                            );
                            ui.add_space(6.0);
                            let resp = ui.add(
                                egui::TextEdit::singleline(&mut input)
                                    .hint_text("folder_name")
                                    .desired_width(360.0),
                            );
                            resp.request_focus();

                            ui.add_space(10.0);
                            ui.horizontal(|ui| {
                                if ui.button("Create Folder").clicked()
                                    || (resp.lost_focus()
                                        && ctx.input(|i| i.key_pressed(egui::Key::Enter)))
                                {
                                    if !input.trim().is_empty() {
                                        let new_path = parent.join(input.trim());
                                        if std::fs::create_dir_all(&new_path).is_ok() {
                                            action_done = true;
                                        }
                                    }
                                    close_dialog = true;
                                }
                                if ui.button("Cancel").clicked()
                                    || ctx.input(|i| i.key_pressed(egui::Key::Escape))
                                {
                                    close_dialog = true;
                                }
                            });
                        });
                    self.pending_action = Some(FileAction::NewFolder { parent, input });
                }
                FileAction::Rename { target, mut input } => {
                    egui::Window::new("✏️  Rename")
                        .collapsible(false)
                        .resizable(false)
                        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                        .default_width(380.0)
                        .show(ctx, |ui| {
                            ui.label(
                                RichText::new(format!(
                                    "Renaming: {}",
                                    target.file_name().unwrap_or_default().to_string_lossy()
                                ))
                                .size(11.0)
                                .color(Color32::GRAY),
                            );
                            ui.add_space(6.0);
                            let resp =
                                ui.add(egui::TextEdit::singleline(&mut input).desired_width(360.0));
                            resp.request_focus();

                            ui.add_space(10.0);
                            ui.horizontal(|ui| {
                                if ui.button("Rename").clicked()
                                    || (resp.lost_focus()
                                        && ctx.input(|i| i.key_pressed(egui::Key::Enter)))
                                {
                                    if !input.trim().is_empty() {
                                        if let Some(parent) = target.parent() {
                                            let new_path = parent.join(input.trim());
                                            if std::fs::rename(&target, &new_path).is_ok() {
                                                for buf in &mut ed.bufs {
                                                    if buf.path.as_ref() == Some(&target) {
                                                        buf.path = Some(new_path.clone());
                                                    }
                                                }
                                                action_done = true;
                                            }
                                        }
                                    }
                                    close_dialog = true;
                                }
                                if ui.button("Cancel").clicked()
                                    || ctx.input(|i| i.key_pressed(egui::Key::Escape))
                                {
                                    close_dialog = true;
                                }
                            });
                        });
                    self.pending_action = Some(FileAction::Rename { target, input });
                }
                FileAction::DeleteConfirm { target } => {
                    egui::Window::new("🗑️  Confirm Delete")
                        .collapsible(false)
                        .resizable(false)
                        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                        .default_width(380.0)
                        .show(ctx, |ui| {
                            ui.label(
                                RichText::new(format!(
                                    "Are you sure you want to delete '{}'?",
                                    target.display()
                                ))
                                .size(13.0),
                            );
                            ui.add_space(12.0);
                            ui.horizontal(|ui| {
                                if ui
                                    .button(RichText::new("Delete").color(Color32::RED))
                                    .clicked()
                                {
                                    if target.is_dir() {
                                        let _ = std::fs::remove_dir_all(&target);
                                    } else {
                                        let _ = std::fs::remove_file(&target);
                                    }
                                    ed.bufs.retain(|b| b.path.as_ref() != Some(&target));
                                    if ed.bufs.is_empty() {
                                        ed.bufs.push(soyabean::buffer::Buffer::empty());
                                    }
                                    if ed.cur >= ed.bufs.len() {
                                        ed.cur = ed.bufs.len() - 1;
                                    }
                                    action_done = true;
                                    close_dialog = true;
                                }
                                if ui.button("Cancel").clicked()
                                    || ctx.input(|i| i.key_pressed(egui::Key::Escape))
                                {
                                    close_dialog = true;
                                }
                            });
                        });
                }
            }

            if close_dialog {
                self.pending_action = None;
            }
        }

        if action_done {
            self.refresh(&root);
        }
    }
}

fn build(dir: &Path) -> Vec<Node> {
    let mut nodes = Vec::new();
    let Ok(rd) = std::fs::read_dir(dir) else {
        return nodes;
    };
    for entry in rd.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with('.') {
            continue;
        }
        let path = entry.path();
        if path.is_dir() {
            if SKIP_DIRS.contains(&name.as_str()) {
                continue;
            }
            let children = build(&path);
            nodes.push(Node {
                name,
                path,
                is_dir: true,
                children,
            });
        } else {
            nodes.push(Node {
                name,
                path,
                is_dir: false,
                children: Vec::new(),
            });
        }
    }
    nodes.sort_by(|a, b| {
        b.is_dir
            .cmp(&a.is_dir)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    nodes
}

fn show_nodes(
    ui: &mut egui::Ui,
    nodes: &[Node],
    ed: &mut Editor,
    p: &Palette,
    current_path: &Option<PathBuf>,
    pending_action: &mut Option<FileAction>,
    expanded: &mut HashSet<PathBuf>,
    indent: f32,
    needs_refresh: &mut bool,
    media_slot: &mut Option<PathBuf>,
) {
    for node in nodes {
        if node.is_dir {
            let open = expanded.contains(&node.path);
            let resp = dir_row(ui, &node.name, open, indent, p);
            resp.dnd_set_drag_payload(DragItem {
                path: node.path.clone(),
            });
            if resp.clicked() {
                if !expanded.insert(node.path.clone()) {
                    expanded.remove(&node.path);
                }
            }
            if let Some(payload) = resp.dnd_release_payload::<DragItem>() {
                let drag = (*payload).clone();
                if perform_move(ed, &drag, &node.path) {
                    *needs_refresh = true;
                }
            }

            let dir_path = node.path.clone();
            let dir_name = node.name.clone();
            resp.context_menu(|ui| {
                ui.set_max_width(190.0);
                if menu_btn(ui, "🗎  New File...", p).clicked() {
                    *pending_action = Some(FileAction::NewFile {
                        parent: dir_path.clone(),
                        input: String::new(),
                    });
                    ui.close_menu();
                }
                if menu_btn(ui, "🗁  New Folder...", p).clicked() {
                    *pending_action = Some(FileAction::NewFolder {
                        parent: dir_path.clone(),
                        input: String::new(),
                    });
                    ui.close_menu();
                }
                ui.add(egui::Separator::default().horizontal());
                if menu_btn(ui, "✏️  Rename...", p).clicked() {
                    *pending_action = Some(FileAction::Rename {
                        target: dir_path.clone(),
                        input: dir_name.clone(),
                    });
                    ui.close_menu();
                }
                if menu_btn(ui, "🗑️  Delete", p).clicked() {
                    *pending_action = Some(FileAction::DeleteConfirm {
                        target: dir_path.clone(),
                    });
                    ui.close_menu();
                }
                ui.add(egui::Separator::default().horizontal());
                if menu_btn(ui, "📋  Copy Path", p).clicked() {
                    ui.ctx().copy_text(dir_path.display().to_string());
                    ui.close_menu();
                }
                if menu_btn(ui, "📂  Reveal in Explorer", p).clicked() {
                    reveal_in_explorer(&dir_path);
                    ui.close_menu();
                }
            });

            if open {
                show_nodes(
                    ui,
                    &node.children,
                    ed,
                    p,
                    current_path,
                    pending_action,
                    expanded,
                    indent + INDENT,
                    needs_refresh,
                    media_slot,
                );
            }
        } else {
            let icon = file_icon(&node.name);
            let is_active = current_path.as_ref() == Some(&node.path);
            let resp = file_row(ui, icon, &node.name, indent, is_active, p)
                .on_hover_text(node.path.display().to_string());
            resp.dnd_set_drag_payload(DragItem {
                path: node.path.clone(),
            });

            if resp.clicked() {
                if super::media_view::is_media_path(&node.path) {
                    *media_slot = Some(node.path.clone());
                } else {
                    *media_slot = None;
                    ed.open_file(node.path.clone());
                }
            }

            let file_path = node.path.clone();
            let file_name = node.name.clone();
            resp.context_menu(|ui| {
                ui.set_max_width(190.0);
                if menu_btn(ui, "✏️  Rename...", p).clicked() {
                    *pending_action = Some(FileAction::Rename {
                        target: file_path.clone(),
                        input: file_name.clone(),
                    });
                    ui.close_menu();
                }
                if menu_btn(ui, "🗑️  Delete", p).clicked() {
                    *pending_action = Some(FileAction::DeleteConfirm {
                        target: file_path.clone(),
                    });
                    ui.close_menu();
                }
                ui.add(egui::Separator::default().horizontal());
                if menu_btn(ui, "📋  Copy Path", p).clicked() {
                    ui.ctx().copy_text(file_path.display().to_string());
                    ui.close_menu();
                }
                if menu_btn(ui, "📂  Reveal in Explorer", p).clicked() {
                    reveal_in_explorer(&file_path);
                    ui.close_menu();
                }
            });
        }
    }
}

// ── row widgets ───────────────────────────────────────────────────────────────

fn dir_row(ui: &mut egui::Ui, name: &str, open: bool, indent: f32, p: &Palette) -> egui::Response {
    let sense = Sense::click_and_drag();
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), ROW_H), sense);
    let is_drop = resp.dnd_hover_payload::<DragItem>().is_some();
    let bg = if is_drop {
        p.selection
    } else if resp.hovered() {
        p.faint_bg
    } else {
        Color32::TRANSPARENT
    };
    if bg != Color32::TRANSPARENT {
        ui.painter().rect_filled(rect, Rounding::same(5.0), bg);
    }
    if is_drop {
        ui.painter().rect_stroke(
            rect.shrink(1.0),
            Rounding::same(5.0),
            egui::Stroke::new(1.5_f32, p.accent),
        );
    }
    let left = rect.left() + 4.0 + indent;
    let cy = rect.center().y;
    ui.painter().text(
        pos2(left, cy),
        Align2::LEFT_CENTER,
        if open { "▾" } else { "▸" },
        FontId::proportional(12.0),
        p.text_dim,
    );
    ui.painter().text(
        pos2(left + 14.0, cy),
        Align2::LEFT_CENTER,
        "📁",
        FontId::proportional(13.0),
        p.text_dim,
    );
    ui.painter().text(
        pos2(left + 34.0, cy),
        Align2::LEFT_CENTER,
        name,
        FontId::proportional(13.0),
        p.fg,
    );
    resp
}

fn file_row(
    ui: &mut egui::Ui,
    icon: &str,
    name: &str,
    indent: f32,
    is_active: bool,
    p: &Palette,
) -> egui::Response {
    let sense = Sense::click_and_drag();
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), ROW_H), sense);
    let bg = if is_active {
        p.selection
    } else if resp.hovered() {
        p.faint_bg
    } else {
        Color32::TRANSPARENT
    };
    if bg != Color32::TRANSPARENT {
        ui.painter().rect_filled(rect, Rounding::same(5.0), bg);
    }
    let left = rect.left() + 8.0 + indent;
    let cy = rect.center().y;
    ui.painter().text(
        pos2(left, cy),
        Align2::LEFT_CENTER,
        icon,
        FontId::proportional(13.0),
        if is_active { p.accent } else { p.text_dim },
    );
    ui.painter().text(
        pos2(left + 22.0, cy),
        Align2::LEFT_CENTER,
        name,
        FontId::proportional(13.0),
        p.fg,
    );
    resp
}

/// Move a dragged item into `target_dir`. Returns true if a move happened.
fn perform_move(ed: &mut Editor, drag: &DragItem, target_dir: &Path) -> bool {
    if drag.path == target_dir || target_dir.starts_with(&drag.path) {
        return false; // onto itself or into its own subtree
    }
    if drag.path.parent() == Some(target_dir) {
        return false; // already in the target folder
    }
    let Some(name) = drag.path.file_name() else {
        return false;
    };
    let dest = target_dir.join(name);
    if dest.exists() {
        return false; // don't silently overwrite
    }
    if std::fs::rename(&drag.path, &dest).is_ok() {
        ed.relocate(&drag.path, &dest);
        return true;
    }
    false
}

fn icon_btn(ui: &mut egui::Ui, glyph: &str, tip: &str, p: &Palette) -> egui::Response {
    let resp = ui.add(
        egui::Button::new(RichText::new(glyph).size(12.0).color(p.text_dim))
            .frame(false)
            .min_size(Vec2::new(22.0, 20.0)),
    );
    if resp.hovered() {
        ui.painter()
            .rect_filled(resp.rect, Rounding::same(4.0), p.faint_bg);
    }
    resp.on_hover_text(tip)
}

fn menu_btn(ui: &mut egui::Ui, text: &str, p: &Palette) -> egui::Response {
    ui.add(
        egui::Button::new(RichText::new(text).size(12.0).color(p.fg))
            .fill(Color32::TRANSPARENT)
            .min_size(Vec2::new(170.0, 24.0)),
    )
}

fn reveal_in_explorer(path: &Path) {
    #[cfg(target_os = "windows")]
    {
        let _ = std::process::Command::new("explorer")
            .args(["/select,", &path.to_string_lossy()])
            .spawn();
    }
    #[cfg(target_os = "macos")]
    {
        let _ = std::process::Command::new("open")
            .args(["-R", &path.to_string_lossy()])
            .spawn();
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let parent = path.parent().unwrap_or(path);
        let _ = std::process::Command::new("xdg-open").arg(parent).spawn();
    }
}

fn file_icon(name: &str) -> &'static str {
    let ext = name.rsplit('.').next().unwrap_or("");
    match ext {
        "rs" => "🦀",
        "toml" => "⚙",
        "md" => "📝",
        "txt" => "📄",
        "json" => "📊",
        "js" | "jsx" | "mjs" | "cjs" => "🟨",
        "ts" | "tsx" => "🔷",
        "py" | "pyw" => "🐍",
        "c" | "h" => "©",
        "cpp" | "cc" | "cxx" | "hpp" => "➕",
        "go" => "🐹",
        "java" => "☕",
        "html" | "htm" => "🌐",
        "css" | "scss" | "less" => "🎨",
        "png" | "jpg" | "jpeg" | "gif" | "svg" | "ico" => "🖼",
        "xml" | "yml" | "yaml" => "📋",
        "sh" | "bash" | "zsh" | "fish" => "🐚",
        "lock" => "🔒",
        _ => "📄",
    }
}
