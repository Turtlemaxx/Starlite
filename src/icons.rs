//! Crisp Lucide icons. Each SVG is rasterized at the exact physical pixel
//! size it is drawn at (cached per size) and placed on the pixel grid, so
//! icons stay sharp at any scale, including mid-animation.

use std::collections::HashMap;

use egui::load::{SizeHint, SizedTexture};
use egui::{Color32, Pos2, Rect, TextureHandle, TextureOptions, Ui, Vec2};

macro_rules! icon_table {
    ($($name:literal),* $(,)?) => {
        static ICONS: &[(&str, &[u8])] = &[
            $(($name, include_bytes!(concat!("../assets/icons/", $name, ".svg")))),*
        ];
    };
}

icon_table!(
    "check", "chevron-left", "cloud", "coffee", "compass", "disc-3", "download",
    "external-link", "flame", "flower", "gem", "guitar", "headphones", "heart", "heart-fill",
    "image-plus", "leaf", "list-plus", "loader-circle", "mic-vocal", "moon", "pause",
    "piano", "play", "plus", "radio", "repeat", "repeat-1", "rotate-cw", "settings",
    "skip-back", "skip-forward", "sparkles", "star", "sun", "trash", "volume-1",
    "volume-2", "volume-x", "x", "zap", "minus", "square", "copy", "maximize-2", "minimize-2",
    "cloudy", "shuffle", "search", "sliders-horizontal", "bubbles", "palette", "file-music",
    "list-x", "server", "rotate-ccw", "pencil", "grip-vertical",
);

/// The static name of any bundled icon (covers can use any of them).
pub fn known(name: &str) -> Option<&'static str> {
    ICONS.iter().map(|(n, _)| *n).find(|n| *n == name)
}

/// Icons offered as playlist covers.
pub const PRESETS: &[&str] = &[
    "disc-3", "headphones", "mic-vocal", "guitar", "piano", "radio", "heart", "star",
    "flame", "zap", "cloud", "moon", "sun", "leaf", "coffee", "gem", "sparkles", "flower",
];

#[derive(Default)]
pub struct Icons {
    cache: HashMap<(&'static str, u32), TextureHandle>,
}

impl Icons {
    fn texture(&mut self, ui: &Ui, name: &'static str, px: u32) -> Option<TextureHandle> {
        if let Some(t) = self.cache.get(&(name, px)) {
            return Some(t.clone());
        }
        let bytes = ICONS.iter().find(|(n, _)| *n == name)?.1;
        let img = egui_extras::image::load_svg_bytes_with_size(bytes, Some(SizeHint::Size(px, px))).ok()?;
        let tex = ui.ctx().load_texture(format!("icon:{name}:{px}"), img, TextureOptions::LINEAR);
        self.cache.insert((name, px), tex.clone());
        Some(tex)
    }

    /// Paint `name` centred on `center`, `size` points tall.
    pub fn paint(&mut self, ui: &Ui, name: &'static str, center: Pos2, size: f32, tint: Color32) {
        self.paint_rotated(ui, name, center, size, tint, 0.0);
    }

    pub fn paint_rotated(&mut self, ui: &Ui, name: &'static str, center: Pos2, size: f32, tint: Color32, angle: f32) {
        if tint.a() == 0 || size < 1.0 {
            return;
        }
        let ppp = ui.ctx().pixels_per_point();
        let px = (size * ppp).round().max(1.0) as u32;
        let Some(tex) = self.texture(ui, name, px) else { return };
        let side = px as f32 / ppp;
        // Snap the top-left corner to a physical pixel.
        let min = center - Vec2::splat(side * 0.5);
        let min = Pos2::new((min.x * ppp).round() / ppp, (min.y * ppp).round() / ppp);
        let rect = Rect::from_min_size(min, Vec2::splat(side));
        let mut img = egui::Image::from_texture(SizedTexture::new(tex.id(), rect.size())).tint(tint);
        if angle != 0.0 {
            img = img.rotate(angle, Vec2::splat(0.5));
        }
        img.paint_at(ui, rect);
    }
}
