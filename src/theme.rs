//! Themes and their persistence (%APPDATA%\Starlite\theme). The custom
//! theme's colours and background picture live in `custom-theme.json`.

use std::path::PathBuf;
use std::sync::RwLock;

use egui::Color32;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Theme {
    Light,
    Dark,
    Cherry,
    Stars,
    Bubbles,
    Custom,
}

/// The custom theme: colours the user picked and an optional picture.
#[derive(Clone, Serialize, Deserialize, PartialEq, Debug)]
#[serde(default)]
pub struct CustomTheme {
    pub background: [u8; 3],
    pub accent: [u8; 3],
    pub text: [u8; 3],
    /// A picture behind everything (copied into the app's folder).
    pub image: Option<PathBuf>,
}

impl Default for CustomTheme {
    fn default() -> Self {
        Self { background: [22, 20, 34], accent: [150, 130, 255], text: [240, 238, 248], image: None }
    }
}

static CUSTOM: RwLock<Option<CustomTheme>> = RwLock::new(None);

impl CustomTheme {
    fn file() -> PathBuf {
        crate::library::data_dir().join("custom-theme.json")
    }

    /// The custom theme in use (loaded once).
    pub fn get() -> CustomTheme {
        if let Some(c) = CUSTOM.read().unwrap().as_ref() {
            return c.clone();
        }
        let c: CustomTheme = std::fs::read_to_string(Self::file()).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
        *CUSTOM.write().unwrap() = Some(c.clone());
        c
    }

    /// Use these colours now (saved separately, see `save`).
    pub fn set(c: CustomTheme) {
        *CUSTOM.write().unwrap() = Some(c);
    }

    pub fn save(&self) {
        let f = Self::file();
        if let Some(d) = f.parent() {
            let _ = std::fs::create_dir_all(d);
        }
        if let Ok(s) = serde_json::to_string_pretty(self) {
            let _ = std::fs::write(f, s);
        }
    }

    /// Light background: dark glass and dark text don't fit, so the UI goes light.
    pub fn is_light(&self) -> bool {
        luma(self.background) > 0.55
    }
}

fn luma(c: [u8; 3]) -> f32 {
    (0.2126 * c[0] as f32 + 0.7152 * c[1] as f32 + 0.0722 * c[2] as f32) / 255.0
}

fn rgb(c: [u8; 3]) -> Color32 {
    Color32::from_rgb(c[0], c[1], c[2])
}

fn mix(a: [u8; 3], b: [u8; 3], t: f32) -> Color32 {
    let m = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Color32::from_rgb(m(a[0], b[0]), m(a[1], b[1]), m(a[2], b[2]))
}

/// A picked background picture, ready for the GPU.
pub struct BgImage {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

/// Reads a background picture, scaled down to at most 2560 px.
pub fn load_bg_image(path: &std::path::Path) -> Option<BgImage> {
    let img = image::open(path).ok()?;
    let img = if img.width().max(img.height()) > 2560 { img.resize(2560, 2560, image::imageops::FilterType::Triangle) } else { img };
    let rgba = img.to_rgba8();
    Some(BgImage { width: rgba.width(), height: rgba.height(), rgba: rgba.into_raw() })
}

/// Copies a picked picture into the app's folder (PNG or JPG).
pub fn import_bg_image(src: &std::path::Path) -> Option<PathBuf> {
    let ext = src.extension()?.to_str()?.to_ascii_lowercase();
    if !matches!(ext.as_str(), "png" | "jpg" | "jpeg") {
        return None;
    }
    load_bg_image(src)?;
    let dir = crate::library::data_dir().join("theme");
    std::fs::create_dir_all(&dir).ok()?;
    let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_millis();
    let dst = dir.join(format!("background-{t}.{ext}"));
    std::fs::copy(src, &dst).ok()?;
    Some(dst)
}

impl Theme {
    pub const ALL: [Theme; 6] = [Theme::Light, Theme::Dark, Theme::Cherry, Theme::Stars, Theme::Bubbles, Theme::Custom];

    pub fn label(self) -> &'static str {
        match self {
            Theme::Light => "Light",
            Theme::Dark => "Dark",
            Theme::Cherry => "Cherry Blossom",
            Theme::Stars => "Stars",
            Theme::Bubbles => "Bubbles",
            Theme::Custom => "Custom",
        }
    }

    pub fn icon(self) -> &'static str {
        match self {
            Theme::Light => "sun",
            Theme::Dark => "moon",
            Theme::Cherry => "flower",
            Theme::Stars => "sparkles",
            Theme::Bubbles => "bubbles",
            Theme::Custom => "palette",
        }
    }

    pub fn shader_id(self) -> i32 {
        match self {
            Theme::Light => 0,
            Theme::Dark => 1,
            Theme::Cherry => 2,
            Theme::Stars => 3,
            Theme::Bubbles => 4,
            Theme::Custom => 5,
        }
    }

    pub fn is_light(self) -> bool {
        match self {
            Theme::Light => true,
            Theme::Custom => CustomTheme::get().is_light(),
            _ => false,
        }
    }

    pub fn text(self) -> Color32 {
        if self == Theme::Custom {
            return rgb(CustomTheme::get().text);
        }
        if self.is_light() { Color32::from_rgb(28, 28, 32) } else { Color32::from_rgb(240, 240, 242) }
    }

    pub fn text_dim(self) -> Color32 {
        match self {
            Theme::Light => Color32::from_rgb(110, 110, 118),
            Theme::Dark => Color32::from_rgb(150, 150, 158),
            Theme::Cherry => Color32::from_rgb(170, 150, 158),
            Theme::Stars => Color32::from_rgb(150, 154, 170),
            Theme::Bubbles => Color32::from_rgb(146, 160, 178),
            Theme::Custom => {
                let c = CustomTheme::get();
                mix(c.text, c.background, 0.38)
            }
        }
    }

    /// Themes whose background moves (need a steady frame tick).
    pub fn animated(self) -> bool {
        matches!(self, Theme::Cherry | Theme::Stars | Theme::Bubbles)
    }

    /// Selected icon tint.
    pub fn accent(self) -> Color32 {
        match self {
            Theme::Cherry => Color32::from_rgb(255, 183, 197),
            Theme::Stars => Color32::from_rgb(220, 228, 255),
            Theme::Bubbles => Color32::from_rgb(168, 222, 255),
            Theme::Custom => rgb(CustomTheme::get().accent),
            _ => self.text(),
        }
    }

    pub fn hairline(self) -> Color32 {
        if self == Theme::Custom {
            let c = CustomTheme::get();
            return mix(c.background, c.text, 0.22);
        }
        if self.is_light() { Color32::from_rgb(196, 196, 202) } else { Color32::from_rgb(70, 70, 76) }
    }

    /// The plain background colour (for painting over the background).
    pub fn background(self) -> Color32 {
        match self {
            Theme::Light => Color32::from_rgb(240, 240, 242),
            Theme::Dark => Color32::from_rgb(26, 26, 28),
            Theme::Bubbles => Color32::from_rgb(8, 12, 20),
            Theme::Custom => rgb(CustomTheme::get().background),
            _ => Color32::from_rgb(14, 12, 14),
        }
    }

    fn file() -> Option<PathBuf> {
        std::env::var("APPDATA").ok().map(|d| PathBuf::from(d).join("Starlite").join("theme"))
    }

    pub fn load() -> Theme {
        // Debug: STARLITE_THEME=light|dark|cherry|stars|bubbles|custom overrides the saved theme.
        let s = std::env::var("STARLITE_THEME")
            .ok()
            .or_else(|| Self::file().and_then(|f| std::fs::read_to_string(f).ok()))
            .unwrap_or_default();
        match s.trim() {
            "light" => Theme::Light,
            "cherry" => Theme::Cherry,
            "stars" => Theme::Stars,
            "bubbles" => Theme::Bubbles,
            "custom" => Theme::Custom,
            // Dark is the default for new installs.
            _ => Theme::Dark,
        }
    }

    pub fn save(self) {
        if std::env::var("STARLITE_THEME").is_ok() {
            // A debug override: leave the real setting alone.
            return;
        }
        if let Some(f) = Self::file() {
            if let Some(dir) = f.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            let v = match self {
                Theme::Light => "light",
                Theme::Dark => "dark",
                Theme::Cherry => "cherry",
                Theme::Stars => "stars",
                Theme::Bubbles => "bubbles",
                Theme::Custom => "custom",
            };
            let _ = std::fs::write(f, v);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn light_backgrounds_get_the_light_ui() {
        let mut c = CustomTheme::default();
        assert!(!c.is_light());
        c.background = [235, 230, 220];
        assert!(c.is_light());
    }

    #[test]
    fn custom_theme_reads_old_files() {
        let c: CustomTheme = serde_json::from_str(r#"{"accent":[1,2,3]}"#).unwrap();
        assert_eq!(c.accent, [1, 2, 3]);
        assert_eq!(c.background, CustomTheme::default().background);
    }
}
