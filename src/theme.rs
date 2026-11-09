//! Themes and their persistence (%APPDATA%\Starlite\theme).

use std::path::PathBuf;

use egui::Color32;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Theme {
    Light,
    Dark,
    Cherry,
    Stars,
}

impl Theme {
    pub const ALL: [Theme; 4] = [Theme::Light, Theme::Dark, Theme::Cherry, Theme::Stars];

    pub fn label(self) -> &'static str {
        match self {
            Theme::Light => "Light",
            Theme::Dark => "Dark",
            Theme::Cherry => "Cherry Blossom",
            Theme::Stars => "Stars",
        }
    }

    pub fn icon(self) -> &'static str {
        match self {
            Theme::Light => "sun",
            Theme::Dark => "moon",
            Theme::Cherry => "flower",
            Theme::Stars => "sparkles",
        }
    }

    pub fn shader_id(self) -> i32 {
        match self {
            Theme::Light => 0,
            Theme::Dark => 1,
            Theme::Cherry => 2,
            Theme::Stars => 3,
        }
    }

    pub fn is_light(self) -> bool {
        self == Theme::Light
    }

    pub fn text(self) -> Color32 {
        if self.is_light() { Color32::from_rgb(28, 28, 32) } else { Color32::from_rgb(240, 240, 242) }
    }

    pub fn text_dim(self) -> Color32 {
        match self {
            Theme::Light => Color32::from_rgb(110, 110, 118),
            Theme::Dark => Color32::from_rgb(150, 150, 158),
            Theme::Cherry => Color32::from_rgb(170, 150, 158),
            Theme::Stars => Color32::from_rgb(150, 154, 170),
        }
    }

    /// Themes whose background moves (need a steady frame tick).
    pub fn animated(self) -> bool {
        matches!(self, Theme::Cherry | Theme::Stars)
    }

    /// Selected icon tint.
    pub fn accent(self) -> Color32 {
        match self {
            Theme::Cherry => Color32::from_rgb(255, 183, 197),
            Theme::Stars => Color32::from_rgb(220, 228, 255),
            _ => self.text(),
        }
    }

    pub fn hairline(self) -> Color32 {
        if self.is_light() { Color32::from_rgb(196, 196, 202) } else { Color32::from_rgb(70, 70, 76) }
    }

    fn file() -> Option<PathBuf> {
        std::env::var("APPDATA").ok().map(|d| PathBuf::from(d).join("Starlite").join("theme"))
    }

    pub fn load() -> Theme {
        // Debug: STARLITE_THEME=light|dark|cherry|stars overrides the saved theme.
        let s = std::env::var("STARLITE_THEME")
            .ok()
            .or_else(|| Self::file().and_then(|f| std::fs::read_to_string(f).ok()))
            .unwrap_or_default();
        match s.trim() {
            "light" => Theme::Light,
            "cherry" => Theme::Cherry,
            "stars" => Theme::Stars,
            // Dark is the default for new installs.
            _ => Theme::Dark,
        }
    }

    pub fn save(self) {
        if let Some(f) = Self::file() {
            if let Some(dir) = f.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            let v = match self {
                Theme::Light => "light",
                Theme::Dark => "dark",
                Theme::Cherry => "cherry",
                Theme::Stars => "stars",
            };
            let _ = std::fs::write(f, v);
        }
    }
}
