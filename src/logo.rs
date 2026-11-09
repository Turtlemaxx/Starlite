//! The Starlite logo (top-left of the nav) and the window icon.
//!
//! Both are drawn from the vector logos in `assets/` at the exact physical
//! pixel size they're shown at, so they're as sharp as the screen allows.

use std::collections::HashMap;

use egui::load::SizeHint;
use egui::{Rect, TextureHandle, TextureOptions, Ui};

/// The white Starlite icon (1024 px PNG), e.g. for Discord's art asset.
pub static WHITE_PNG: &[u8] = include_bytes!("../assets/starlite-icon.png");

static DARK_SVG: &[u8] = include_bytes!("../assets/starlite-icon.svg");
static LIGHT_SVG: &[u8] = include_bytes!("../assets/starlite-icon-light.svg");

fn render(light: bool, px: u32) -> Option<egui::ColorImage> {
    egui_extras::image::load_svg_bytes_with_size(if light { LIGHT_SVG } else { DARK_SVG }, Some(SizeHint::Size(px, px))).ok()
}

/// The window / taskbar icon: 256 px (Windows scales it down itself). The
/// star is see-through around, so it follows the taskbar: black on a light
/// taskbar, white on a dark one.
pub fn window_icon() -> egui::IconData {
    match render(crate::win::taskbar_is_light(), 256) {
        Some(img) => egui::IconData {
            width: img.size[0] as u32,
            height: img.size[1] as u32,
            rgba: img.pixels.iter().flat_map(|c| c.to_srgba_unmultiplied()).collect(),
        },
        None => egui::IconData::default(),
    }
}

#[derive(Default)]
pub struct Logo {
    cache: HashMap<(bool, u32), TextureHandle>,
}

impl Logo {
    /// Paints the logo into `rect`; `light` picks the version for light themes.
    pub fn paint(&mut self, ui: &Ui, rect: Rect, light: bool, opacity: f32) {
        let ppp = ui.ctx().pixels_per_point();
        let px = (rect.width() * ppp).round().clamp(8.0, 1024.0) as u32;
        let key = (light, px);
        if !self.cache.contains_key(&key) {
            let Some(img) = render(light, px) else { return };
            let tex = ui.ctx().load_texture(format!("logo:{light}:{px}"), img, TextureOptions::LINEAR);
            // Sizes pass by while the sidebar animates; keep the cache small.
            if self.cache.len() > 24 {
                self.cache.clear();
            }
            self.cache.insert(key, tex);
        }
        let tex = &self.cache[&key];
        // Exactly px × px physical pixels, on the pixel grid.
        let side = px as f32 / ppp;
        let min = rect.center() - egui::Vec2::splat(side * 0.5);
        let min = egui::pos2((min.x * ppp).round() / ppp, (min.y * ppp).round() / ppp);
        let r = Rect::from_min_size(min, egui::Vec2::splat(side));
        egui::Image::from_texture(egui::load::SizedTexture::new(tex.id(), r.size()))
            .tint(egui::Color32::WHITE.gamma_multiply(opacity))
            .paint_at(ui, r);
    }
}
