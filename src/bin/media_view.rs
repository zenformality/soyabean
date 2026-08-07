//! Media viewer: image previews (scaled, texture-cached) and a small audio
//! player card built on rodio. Media files are never loaded into a text buffer.

use std::collections::HashMap;
use std::fs::File;
use std::io::BufReader;
use std::path::{Path, PathBuf};

use eframe::egui::{self, FontId, RichText, Rounding, UiBuilder, Vec2};
use rodio::{Decoder, OutputStream, OutputStreamHandle, Sink};

use super::theme::Palette;

// ── type detection ────────────────────────────────────────────────────────────

pub fn is_image_path(path: &Path) -> bool {
    matches!(
        ext(path).as_str(),
        "png" | "jpg" | "jpeg" | "gif" | "bmp" | "webp" | "ico" | "tiff" | "tif" | "pbm"
            | "pgm" | "ppm" | "pnm"
    )
}

pub fn is_audio_path(path: &Path) -> bool {
    matches!(ext(path).as_str(), "mp3" | "wav" | "ogg" | "flac" | "m4a" | "aac" | "aiff" | "opus")
}

pub fn is_media_path(path: &Path) -> bool {
    is_image_path(path) || is_audio_path(path)
}

fn ext(path: &Path) -> String {
    path.extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase()
}

// ── image preview ─────────────────────────────────────────────────────────────

pub type ImgCache = HashMap<PathBuf, (std::time::SystemTime, egui::TextureHandle)>;

/// Draw a preview of `path` scaled to fit the available space. Returns false
/// (and shows an error) if the image cannot be decoded.
pub fn show_image(
    ui: &mut egui::Ui,
    path: &Path,
    p: &Palette,
    cache: &mut ImgCache,
) -> bool {
    let avail = ui.available_size();
    let (rect, _) = ui.allocate_exact_size(avail, egui::Sense::hover());
    let painter = ui.painter_at(rect);

    // Header: file name + dimensions
    let modified = std::fs::metadata(path).and_then(|m| m.modified()).unwrap_or(std::time::SystemTime::UNIX_EPOCH);
    let tex = match cache.get(path) {
        Some((m, t)) if *m == modified => t.clone(),
        _ => {
            let loaded = load_texture(ui.ctx(), path);
            match loaded {
                Ok(t) => {
                    cache.insert(path.to_path_buf(), (modified, t.clone()));
                    t
                }
                Err(e) => {
                    painter.rect_filled(rect, 0.0, p.bg);
                    painter.text(
                        rect.center(), egui::Align2::CENTER_CENTER,
                        format!("Cannot preview image: {e}"),
                        FontId::proportional(14.0), p.text_dim,
                    );
                    return false;
                }
            }
        }
    };

    painter.rect_filled(rect, 0.0, p.bg);
    let (tw, th) = (tex.size_vec2().x, tex.size_vec2().y);
    if tw <= 0.0 || th <= 0.0 {
        return false;
    }

    // Keep aspect ratio, fit into the panel (allow upscaling of small images).
    let scale = (avail.x / tw).min(avail.y / th).min(6.0).max(0.05);
    let size = Vec2::new(tw * scale, th * scale);

    ui.allocate_new_ui(UiBuilder::new().max_rect(rect), |ui| {
        ui.centered_and_justified(|ui| {
            ui.add(egui::Image::new(&tex).fit_to_exact_size(size));
        });
    });

    let name = path.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
    painter.text(
        rect.left_top() + Vec2::new(8.0, 6.0), egui::Align2::LEFT_TOP,
        format!("{name}  ·  {tw:.0}×{th:.0}"),
        FontId::proportional(12.0), p.text_dim,
    );
    true
}

fn load_texture(ctx: &egui::Context, path: &Path) -> Result<egui::TextureHandle, String> {
    let img = image::open(path).map_err(|e| e.to_string())?;
    let rgba = img.to_rgba8();
    let (w, h) = rgba.dimensions();
    let color_img = egui::ColorImage::from_rgba_unmultiplied(
        [w as usize, h as usize],
        &rgba.into_raw(),
    );
    Ok(ctx.load_texture(
        "media-preview",
        color_img,
        egui::TextureOptions::LINEAR,
    ))
}

// ── audio player ──────────────────────────────────────────────────────────────

pub struct AudioPlayer {
    stream: Option<OutputStream>,
    _handle: Option<OutputStreamHandle>,
    sink: Option<Sink>,
    path: Option<PathBuf>,
    pub volume: f32,
}

impl AudioPlayer {
    pub fn new() -> Self {
        AudioPlayer {
            stream: None,
            _handle: None,
            sink: None,
            path: None,
            volume: 0.8,
        }
    }

    pub fn is_playing(&self) -> bool {
        self.sink.as_ref().map_or(false, |s| !s.empty() && !s.is_paused())
    }

    fn ensure_loaded(&mut self, path: &Path) -> bool {
        if self.path.as_deref() == Some(path) {
            return true;
        }
        self.stop();
        if self.stream.is_none() {
            match OutputStream::try_default() {
                Ok((s, h)) => {
                    self.stream = Some(s);
                    self._handle = Some(h);
                }
                Err(_) => return false,
            }
        }
        let handle = match self._handle.as_ref() {
            Some(h) => h.clone(),
            None => return false,
        };
        let Ok(file) = File::open(path) else { return false };
        let Ok(decoder) = Decoder::new(BufReader::new(file)) else { return false };
        match Sink::try_new(&handle) {
            Ok(sink) => {
                sink.set_volume(self.volume);
                sink.append(decoder);
                self.sink = Some(sink);
                self.path = Some(path.to_path_buf());
                true
            }
            Err(_) => false,
        }
    }

    pub fn toggle(&mut self, path: &Path) {
        if !self.ensure_loaded(path) {
            return;
        }
        let Some(sink) = &self.sink else { return };
        if sink.empty() {
            return;
        }
        if sink.is_paused() {
            sink.play();
        } else {
            sink.pause();
        }
    }

    pub fn stop(&mut self) {
        if let Some(s) = self.sink.take() {
            s.stop();
        }
        self.path = None;
    }

    pub fn set_volume(&mut self, v: f32) {
        self.volume = v.clamp(0.0, 1.5);
        if let Some(s) = &self.sink {
            s.set_volume(self.volume);
        }
    }
}

/// Draw an audio player card for `path`. Handles loading/state itself.
pub fn show_audio(ui: &mut egui::Ui, path: &Path, p: &Palette, player: &mut AudioPlayer) {
    let avail = ui.available_size();
    let (rect, _) = ui.allocate_exact_size(avail, egui::Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 0.0, p.bg);

    let name = path.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();

    let card_w = 420.0;
    let card_h = 160.0;
    let card = egui::Rect::from_center_size(rect.center(), Vec2::new(card_w, card_h));
    painter.rect_filled(card, Rounding::same(10.0), p.sidebar_bg);
    painter.rect_stroke(card, Rounding::same(10.0), egui::Stroke::new(1.0_f32, p.border));

    let playing = player.is_playing();
    ui.allocate_new_ui(UiBuilder::new().max_rect(card), |ui| {
        ui.add_space(20.0);
        ui.centered_and_justified(|ui| {
            ui.label(RichText::new("🎵").size(44.0));
        });
        ui.add_space(6.0);
        ui.centered_and_justified(|ui| {
            ui.label(RichText::new(name).size(15.0).color(p.fg).strong());
        });

        ui.add_space(16.0);
        ui.centered_and_justified(|ui| {
            ui.horizontal(|ui| {
                if ui.add_sized(
                    Vec2::new(34.0, 34.0),
                    egui::Button::new(RichText::new(if playing { "⏸" } else { "▶" }).size(16.0))
                        .fill(p.accent)
                        .rounding(Rounding::same(17.0)),
                ).clicked() {
                    player.toggle(path);
                }
                if ui.add_sized(
                    Vec2::new(34.0, 34.0),
                    egui::Button::new(RichText::new("⏹").size(15.0))
                        .fill(p.button_bg)
                        .rounding(Rounding::same(17.0)),
                ).clicked() {
                    player.stop();
                }
            });
        });

        ui.add_space(14.0);
        ui.horizontal(|ui| {
            ui.add_space(16.0);
            ui.label(RichText::new("🔉").size(13.0).color(p.text_dim));
            let mut vol = player.volume;
            ui.add_sized(
                Vec2::new(card_w - 110.0, 16.0),
                egui::Slider::new(&mut vol, 0.0..=1.5).show_value(false),
            );
            if (vol - player.volume).abs() > 0.001 {
                player.set_volume(vol);
            }
            ui.label(RichText::new(format!("{:>3}%", (player.volume * 100.0).round() as i32))
                .size(11.0).color(p.text_dim));
        });
    });

    if playing {
        ui.ctx().request_repaint_after(std::time::Duration::from_millis(250));
    }
}

impl Default for AudioPlayer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(s: &str) -> &Path {
        Path::new(s)
    }

    #[test]
    fn detects_image_extensions_case_insensitive() {
        assert!(is_image_path(p("a.png")));
        assert!(is_image_path(p("photo.JPG")));
        assert!(is_image_path(p("anim.gif")));
        assert!(is_image_path(p("icon.ico")));
        assert!(!is_image_path(p("notes.txt")));
        assert!(!is_image_path(p("song.mp3")));
    }

    #[test]
    fn detects_audio_extensions() {
        assert!(is_audio_path(p("song.mp3")));
        assert!(is_audio_path(p("clip.wav")));
        assert!(is_audio_path(p("track.flac")));
        assert!(!is_audio_path(p("pic.png")));
        assert!(!is_audio_path(p("noext")));
    }

    #[test]
    fn media_helpers_agree() {
        assert!(is_media_path(p("x.png")));
        assert!(is_media_path(p("x.mp3")));
        assert!(!is_media_path(p("main.rs")));
    }
}
