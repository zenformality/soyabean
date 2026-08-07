//! Project file tree with right-click context menus (New File, New Folder, Rename, Delete, Reveal in Explorer),
//! file creation modal dialogs, colorful file-type icons, and active buffer highlighting.

use std::path::{Path, PathBuf};
use eframe::egui::{self, Color32, RichText, Sense, Vec2};
use soyabean::editor::Editor;
use super::theme::Palette;

const SKIP_DIRS: &[&str] = &[
    ".git", "target", "node_modules", "dist", "build", "__pycache__",
    ".venv", "venv", ".idea", ".vscode", "out", "bin", "obj",
];

#[derive(Clone, Debug)]
pub enum FileAction {
    NewFile { parent: PathBuf, input: String },
    NewFolder { parent: PathBuf, input: String },
    Rename { target: PathBuf, input: String },
    DeleteConfirm { target: PathBuf },
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
}

impl Default for FileTree {
    fn default() -> Self {
        FileTree {
            root_path: PathBuf::new(),
            root_nodes: Vec::new(),
            pending_action: None,
        }
    }
}

impl FileTree {
    pub fn refresh(&mut self, root: &Path) {
        self.root_path = root.to_path_buf();
        self.root_nodes = build(root);
    }

    pub fn show(&mut self, ui: &mut egui::Ui, ed: &mut Editor, p: &Palette) {
        let current_path = ed.buf().path.clone();

        // ── Header Quick Actions ─────────────────────────────────────────────
        ui.horizontal(|ui| {
            ui.label(RichText::new("PROJECT").strong().size(11.0).color(p.text_dim));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.small_button(RichText::new("↻").color(p.text_dim))
                    .on_hover_text("Refresh Tree").clicked()
                {
                    self.refresh(&ed.root.clone());
                }
                if ui.small_button(RichText::new("📁+").color(p.text_dim))
                    .on_hover_text("New Folder in Root").clicked()
                {
                    self.pending_action = Some(FileAction::NewFolder {
                        parent: self.root_path.clone(),
                        input: String::new(),
                    });
                }
                if ui.small_button(RichText::new("📄+").color(p.text_dim))
                    .on_hover_text("New File in Root").clicked()
                {
                    self.pending_action = Some(FileAction::NewFile {
                        parent: self.root_path.clone(),
                        input: String::new(),
                    });
                }
            });
        });
        ui.add_space(4.0);
        ui.add(egui::Separator::default().horizontal());

        // ── Tree Nodes Render ────────────────────────────────────────────────
        let mut new_action = None;
        egui::ScrollArea::vertical().show(ui, |ui| {
            show_nodes(ui, &self.root_nodes, ed, p, &current_path, &mut new_action);

            // Empty space background right click for root dir actions
            let available = ui.available_rect_before_wrap();
            if available.height() > 20.0 {
                let bg_resp = ui.interact(available, ui.id().with("tree_bg"), Sense::click());
                let root_path = self.root_path.clone();
                bg_resp.context_menu(|ui| {
                    ui.set_max_width(180.0);
                    if menu_btn(ui, "📄  New File...", p).clicked() {
                        new_action = Some(FileAction::NewFile {
                            parent: root_path.clone(),
                            input: String::new(),
                        });
                        ui.close_menu();
                    }
                    if menu_btn(ui, "📁  New Folder...", p).clicked() {
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

        // ── Action Dialog Modals (New File / New Folder / Rename / Delete) ───
        self.handle_action_dialog(ui.ctx(), ed);
    }

    fn handle_action_dialog(&mut self, ctx: &egui::Context, ed: &mut Editor) {
        let root = ed.root.clone();
        let mut action_done = false;

        if let Some(action) = self.pending_action.clone() {
            let mut close_dialog = false;

            match action {
                FileAction::NewFile { parent, mut input } => {
                    egui::Window::new("📄  New File")
                        .collapsible(false)
                        .resizable(false)
                        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                        .default_width(360.0)
                        .show(ctx, |ui| {
                            ui.label(RichText::new(format!("Create in: {}", parent.display())).size(11.0).color(Color32::GRAY));
                            ui.add_space(6.0);
                            let resp = ui.add(egui::TextEdit::singleline(&mut input)
                                .hint_text("filename.rs")
                                .desired_width(340.0));
                            resp.request_focus();

                            ui.add_space(10.0);
                            ui.horizontal(|ui| {
                                if ui.button("Create File").clicked() || (resp.lost_focus() && ctx.input(|i| i.key_pressed(egui::Key::Enter))) {
                                    if !input.trim().is_empty() {
                                        let new_path = parent.join(input.trim());
                                        if let Some(dir) = new_path.parent() {
                                            let _ = std::fs::create_dir_all(dir);
                                        }
                                        if std::fs::File::create(&new_path).is_ok() {
                                            ed.open_file(new_path);
                                            action_done = true;
                                        }
                                    }
                                    close_dialog = true;
                                }
                                if ui.button("Cancel").clicked() || ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
                                    close_dialog = true;
                                }
                            });
                        });
                    self.pending_action = Some(FileAction::NewFile { parent, input });
                }
                FileAction::NewFolder { parent, mut input } => {
                    egui::Window::new("📁  New Folder")
                        .collapsible(false)
                        .resizable(false)
                        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                        .default_width(360.0)
                        .show(ctx, |ui| {
                            ui.label(RichText::new(format!("Create in: {}", parent.display())).size(11.0).color(Color32::GRAY));
                            ui.add_space(6.0);
                            let resp = ui.add(egui::TextEdit::singleline(&mut input)
                                .hint_text("folder_name")
                                .desired_width(340.0));
                            resp.request_focus();

                            ui.add_space(10.0);
                            ui.horizontal(|ui| {
                                if ui.button("Create Folder").clicked() || (resp.lost_focus() && ctx.input(|i| i.key_pressed(egui::Key::Enter))) {
                                    if !input.trim().is_empty() {
                                        let new_path = parent.join(input.trim());
                                        if std::fs::create_dir_all(&new_path).is_ok() {
                                            action_done = true;
                                        }
                                    }
                                    close_dialog = true;
                                }
                                if ui.button("Cancel").clicked() || ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
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
                        .default_width(360.0)
                        .show(ctx, |ui| {
                            ui.label(RichText::new(format!("Renaming: {}", target.file_name().unwrap_or_default().to_string_lossy())).size(11.0).color(Color32::GRAY));
                            ui.add_space(6.0);
                            let resp = ui.add(egui::TextEdit::singleline(&mut input).desired_width(340.0));
                            resp.request_focus();

                            ui.add_space(10.0);
                            ui.horizontal(|ui| {
                                if ui.button("Rename").clicked() || (resp.lost_focus() && ctx.input(|i| i.key_pressed(egui::Key::Enter))) {
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
                                if ui.button("Cancel").clicked() || ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
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
                        .default_width(360.0)
                        .show(ctx, |ui| {
                            ui.label(RichText::new(format!("Are you sure you want to delete '{}'?", target.display())).size(13.0));
                            ui.add_space(12.0);
                            ui.horizontal(|ui| {
                                if ui.button(RichText::new("Delete").color(Color32::RED)).clicked() {
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
                                if ui.button("Cancel").clicked() || ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
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
    let Ok(rd) = std::fs::read_dir(dir) else { return nodes };
    for entry in rd.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with('.') { continue; }
        let path = entry.path();
        if path.is_dir() {
            if SKIP_DIRS.contains(&name.as_str()) { continue; }
            let children = build(&path);
            nodes.push(Node { name, path, is_dir: true, children });
        } else {
            nodes.push(Node { name, path, is_dir: false, children: Vec::new() });
        }
    }
    nodes.sort_by(|a, b| b.is_dir.cmp(&a.is_dir)
        .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase())));
    nodes
}

fn show_nodes(
    ui: &mut egui::Ui,
    nodes: &[Node],
    ed: &mut Editor,
    p: &Palette,
    current_path: &Option<PathBuf>,
    pending_action: &mut Option<FileAction>,
) {
    for node in nodes {
        if node.is_dir {
            let label = format!("📁 {}", node.name);
            let header = egui::CollapsingHeader::new(RichText::new(label).color(p.fg).strong())
                .default_open(false)
                .id_salt(&node.path);

            let resp = header.show(ui, |ui| show_nodes(ui, &node.children, ed, p, current_path, pending_action));

            let dir_path = node.path.clone();
            let dir_name = node.name.clone();
            resp.header_response.context_menu(|ui| {
                ui.set_max_width(180.0);
                if menu_btn(ui, "📄  New File...", p).clicked() {
                    *pending_action = Some(FileAction::NewFile {
                        parent: dir_path.clone(),
                        input: String::new(),
                    });
                    ui.close_menu();
                }
                if menu_btn(ui, "📁  New Folder...", p).clicked() {
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
        } else {
            let icon = file_icon(&node.name);
            let is_active = current_path.as_ref() == Some(&node.path);
            let label = format!("{} {}", icon, node.name);
            let text = RichText::new(label).color(if is_active { p.accent } else { p.fg });

            let resp = ui.selectable_label(is_active, text)
                .on_hover_text(node.path.display().to_string());

            if resp.clicked() {
                ed.open_file(node.path.clone());
            }

            let file_path = node.path.clone();
            let file_name = node.name.clone();
            resp.context_menu(|ui| {
                ui.set_max_width(180.0);
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

fn menu_btn(ui: &mut egui::Ui, text: &str, p: &Palette) -> egui::Response {
    ui.add(egui::Button::new(RichText::new(text).size(12.0).color(p.fg))
        .fill(Color32::TRANSPARENT)
        .min_size(Vec2::new(170.0, 22.0)))
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
        let _ = std::process::Command::new("xdg-open")
            .arg(parent)
            .spawn();
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
