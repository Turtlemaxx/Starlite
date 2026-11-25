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
    /// The quieter text colour, when the theme's CSS sets one.
    #[serde(skip)]
    pub text_dim: Option<[u8; 3]>,
}

impl Default for CustomTheme {
    fn default() -> Self {
        Self { background: [22, 20, 34], accent: [150, 130, 255], text: [240, 238, 248], image: None, text_dim: None }
    }
}

static CUSTOM: RwLock<Option<CustomTheme>> = RwLock::new(None);

/// The simple look is on (liquid glass off): plain greys for backgrounds.
static FLAT: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);

/// How panels are drawn.
#[derive(Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Debug, Default)]
pub enum Style {
    /// Flat, solid panels.
    #[default]
    Simple,
    /// Like Simple, but see-through: what's behind is blurred.
    Glass,
    /// Glass that bends and reflects what's behind it.
    Liquid,
}

impl Style {
    pub const ALL: [Style; 3] = [Style::Simple, Style::Glass, Style::Liquid];
    pub fn label(self) -> &'static str {
        match self {
            Style::Simple => "Simple",
            Style::Glass => "Glass",
            Style::Liquid => "Liquid glass",
        }
    }
    pub fn shader_id(self) -> i32 {
        self as i32
    }
}

/// Where the accent colour comes from.
#[derive(Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Debug, Default)]
pub enum AccentFrom {
    /// The theme's own.
    #[default]
    Theme,
    /// The cover of the song that's playing.
    Album,
    /// A colour you pick.
    Picked,
}

/// The page background.
#[derive(Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Debug, Default)]
pub enum Backdrop {
    #[default]
    Theme,
    /// Slowly drifting colours from the playing song's cover.
    Album,
}

#[derive(Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Debug, Default)]
pub enum RowSize {
    Compact,
    #[default]
    Normal,
    Large,
}

impl RowSize {
    pub const ALL: [RowSize; 3] = [RowSize::Compact, RowSize::Normal, RowSize::Large];
    pub fn label(self) -> &'static str {
        match self {
            RowSize::Compact => "Compact",
            RowSize::Normal => "Normal",
            RowSize::Large => "Large",
        }
    }
    /// Row height and cover size.
    pub fn sizes(self) -> (f32, f32) {
        match self {
            RowSize::Compact => (42.0, 30.0),
            RowSize::Normal => (54.0, 40.0),
            RowSize::Large => (68.0, 54.0),
        }
    }
}

/// The player bar's buttons that can be hidden (right to left).
pub const BAR_BUTTONS: [(&str, &str); 5] = [("download", "Download"), ("add", "Add to playlist"), ("like", "Like"), ("loop", "Loop"), ("fx", "Speed and bass")];

/// How the window looks (look.json): the panel style, and what the
/// Advanced part of Settings → Appearance changes. `None`: the look's own.
#[derive(Clone, Serialize, Deserialize, PartialEq, Debug)]
#[serde(default)]
pub struct LookPrefs {
    pub style: Style,
    /// From before there were three styles.
    #[serde(skip_serializing)]
    pub liquid_glass: Option<bool>,
    /// The Advanced options are shown.
    pub advanced: bool,
    pub accent_from: AccentFrom,
    pub accent: [u8; 3],
    /// Largest corner radius of panels (points).
    pub round: Option<f32>,
    /// How solid panels are (1: the look's own; less is more see-through).
    pub solid: Option<f32>,
    /// How much panels blur what's behind them (points).
    pub blur: Option<f32>,
    pub outline: Option<f32>,
    pub shadow: Option<f32>,
    /// A font installed on this PC.
    pub font: Option<String>,
    pub text_size: f32,
    /// Icon line width (2 is Lucide's own).
    pub icon_weight: f32,
    pub backdrop: Backdrop,
    /// Moving backgrounds (Cherry Blossom, Stars, Bubbles): speed and how many.
    pub motion_speed: f32,
    pub motion_amount: f32,
    // Layout.
    pub rows: RowSize,
    pub show_numbers: bool,
    pub show_album: bool,
    pub nav_width: f32,
    pub nav_covers: bool,
    /// Player bar buttons that are hidden (see `BAR_BUTTONS`).
    pub bar_hidden: Vec<String>,
    /// The click sound, and your own sound file for it.
    pub click: crate::sound::ClickSound,
    pub click_file: Option<PathBuf>,
}

impl Default for LookPrefs {
    fn default() -> Self {
        Self {
            style: Style::Simple,
            liquid_glass: None,
            advanced: false,
            accent_from: AccentFrom::Theme,
            accent: [150, 130, 255],
            round: None,
            solid: None,
            blur: None,
            outline: None,
            shadow: None,
            font: None,
            text_size: 1.0,
            icon_weight: 2.0,
            backdrop: Backdrop::Theme,
            motion_speed: 1.0,
            motion_amount: 1.0,
            rows: RowSize::Normal,
            show_numbers: true,
            show_album: false,
            nav_width: 232.0,
            nav_covers: true,
            bar_hidden: Vec::new(),
            click: crate::sound::ClickSound::Classic,
            click_file: None,
        }
    }
}

/// Picked accent colour for every theme (None: the theme's own).
static ACCENT: RwLock<Option<[u8; 3]>> = RwLock::new(None);
/// Text size (1 = normal), as f32 bits.
static TEXT_SCALE: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0x3f80_0000);

/// The accent every theme uses for now (None: each theme's own).
pub fn set_accent(c: Option<[u8; 3]>) {
    let mut a = ACCENT.write().unwrap();
    if *a != c {
        *a = c;
    }
}

pub fn text_scale() -> f32 {
    f32::from_bits(TEXT_SCALE.load(std::sync::atomic::Ordering::Relaxed))
}

impl LookPrefs {
    fn file() -> PathBuf {
        crate::library::data_dir().join("look.json")
    }
    pub fn load() -> Self {
        let mut p: Self = std::fs::read_to_string(Self::file()).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
        if p.liquid_glass.take() == Some(true) {
            p.style = Style::Liquid;
        }
        // Debug: STARLITE_GLASS=0 / 1 / 2 forces simple / glass / liquid glass.
        if let Ok(v) = std::env::var("STARLITE_GLASS") {
            p.style = match v.as_str() {
                "0" => Style::Simple,
                "2" => Style::Liquid,
                _ => Style::Glass,
            };
        }
        // Debug: STARLITE_ADVANCED=1 opens the Advanced options.
        if std::env::var("STARLITE_ADVANCED").is_ok() {
            p.advanced = true;
        }
        p = p.clamped();
        p.apply();
        p
    }
    /// Every value in its range (a hand-edited file can't break the window).
    pub fn clamped(mut self) -> Self {
        let f = |v: f32, lo: f32, hi: f32, or: f32| if v.is_finite() { v.clamp(lo, hi) } else { or };
        self.round = self.round.map(|v| f(v, 0.0, 40.0, 14.0));
        self.solid = self.solid.map(|v| f(v, 0.0, 3.0, 1.0));
        self.blur = self.blur.map(|v| f(v, 0.0, 48.0, 16.0));
        self.outline = self.outline.map(|v| f(v, 0.0, 3.0, 1.0));
        self.shadow = self.shadow.map(|v| f(v, 0.0, 3.0, 1.0));
        self.text_size = f(self.text_size, 0.85, 1.2, 1.0);
        self.icon_weight = f(self.icon_weight, 1.25, 2.75, 2.0);
        self.motion_speed = f(self.motion_speed, 0.0, 3.0, 1.0);
        self.motion_amount = f(self.motion_amount, 0.2, 2.0, 1.0);
        self.nav_width = f(self.nav_width, 196.0, 300.0, 232.0);
        self.font = self.font.filter(|s| !s.trim().is_empty());
        self
    }
    pub fn save(&self) {
        let f = Self::file();
        if let Some(d) = f.parent() {
            let _ = std::fs::create_dir_all(d);
        }
        if let Ok(s) = serde_json::to_string_pretty(self) {
            let _ = std::fs::write(f, s);
        }
        self.apply();
    }
    /// Puts the values the whole app reads in place.
    pub fn apply(&self) {
        FLAT.store(self.style != Style::Liquid, std::sync::atomic::Ordering::Relaxed);
        TEXT_SCALE.store(self.text_size.to_bits(), std::sync::atomic::Ordering::Relaxed);
        crate::icons::set_weight(self.icon_weight);
        if self.accent_from == AccentFrom::Picked {
            set_accent(Some(self.accent));
        } else if self.accent_from == AccentFrom::Theme {
            set_accent(None);
        }
    }
    /// The Advanced values back to the look's own (style and layout stay).
    pub fn reset_look(&mut self) {
        let d = LookPrefs::default();
        self.accent_from = d.accent_from;
        self.round = None;
        self.solid = None;
        self.blur = None;
        self.outline = None;
        self.shadow = None;
        self.font = None;
        self.text_size = d.text_size;
        self.icon_weight = d.icon_weight;
        self.backdrop = d.backdrop;
        self.motion_speed = d.motion_speed;
        self.motion_amount = d.motion_amount;
    }
    pub fn reset_layout(&mut self) {
        let d = LookPrefs::default();
        self.rows = d.rows;
        self.show_numbers = d.show_numbers;
        self.show_album = d.show_album;
        self.nav_width = d.nav_width;
        self.nav_covers = d.nav_covers;
        self.bar_hidden.clear();
    }
    pub fn shows(&self, button: &str) -> bool {
        !self.bar_hidden.iter().any(|b| b == button)
    }
}

/// A saved look: theme, colours, picture, CSS, style and everything in
/// Advanced, under a name (looks.json).
#[derive(Clone, Serialize, Deserialize, PartialEq, Debug)]
pub struct SavedLook {
    pub name: String,
    pub theme: String,
    pub custom: CustomTheme,
    pub look: LookPrefs,
    /// The Custom theme's CSS at the time (None: there was none).
    #[serde(default)]
    pub css: Option<String>,
}

pub fn load_looks() -> Vec<SavedLook> {
    std::fs::read_to_string(crate::library::data_dir().join("looks.json")).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
}

pub fn save_looks(looks: &[SavedLook]) {
    let f = crate::library::data_dir().join("looks.json");
    if let Some(d) = f.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    if let Ok(s) = serde_json::to_string_pretty(looks) {
        let _ = std::fs::write(f, s);
    }
}


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
    // (Not "theme": that's the name of the file the picked theme is saved in.)
    let dir = crate::library::data_dir().join("backgrounds");
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
                c.text_dim.map(rgb).unwrap_or_else(|| mix(c.text, c.background, 0.38))
            }
        }
    }

    /// Themes whose background moves (need a steady frame tick).
    pub fn animated(self) -> bool {
        matches!(self, Theme::Cherry | Theme::Stars | Theme::Bubbles)
    }

    /// Selected icon tint.
    pub fn accent(self) -> Color32 {
        if let Some(c) = *ACCENT.read().unwrap() {
            return rgb(c);
        }
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
        let flat = FLAT.load(std::sync::atomic::Ordering::Relaxed);
        match self {
            Theme::Light if flat => Color32::from_rgb(247, 247, 249),
            Theme::Dark if flat => Color32::from_rgb(35, 35, 38),
            Theme::Light => Color32::from_rgb(240, 240, 242),
            Theme::Dark => Color32::from_rgb(26, 26, 28),
            Theme::Bubbles => Color32::from_rgb(8, 12, 20),
            Theme::Custom => rgb(CustomTheme::get().background),
            _ => Color32::from_rgb(14, 12, 14),
        }
    }

    fn file() -> Option<PathBuf> {
        Some(crate::library::data_dir().join("theme"))
    }

    /// The name it's saved under.
    pub fn key(self) -> &'static str {
        match self {
            Theme::Light => "light",
            Theme::Dark => "dark",
            Theme::Cherry => "cherry",
            Theme::Stars => "stars",
            Theme::Bubbles => "bubbles",
            Theme::Custom => "custom",
        }
    }

    pub fn from_key(s: &str) -> Theme {
        Theme::ALL.into_iter().find(|t| t.key() == s.trim()).unwrap_or(Theme::Dark)
    }

    pub fn load() -> Theme {
        // Debug: STARLITE_THEME=light|dark|cherry|stars|bubbles|custom overrides the saved theme.
        let s = std::env::var("STARLITE_THEME")
            .ok()
            .or_else(|| Self::file().and_then(|f| std::fs::read_to_string(f).ok()))
            .unwrap_or_default();
        // Dark is the default for new installs.
        Theme::from_key(&s)
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
            let _ = std::fs::write(f, self.key());
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
    fn looks_read_old_files_and_keep_in_range() {
        let old: LookPrefs = serde_json::from_str(r#"{"liquid_glass": true}"#).unwrap();
        assert_eq!(old.liquid_glass, Some(true));
        let mut p = old;
        if p.liquid_glass.take() == Some(true) {
            p.style = Style::Liquid;
        }
        assert_eq!((p.style, p.advanced, p.rows, p.nav_width), (Style::Liquid, false, RowSize::Normal, 232.0));
        let wild = LookPrefs { round: Some(500.0), text_size: 9.0, nav_width: f32::NAN, font: Some("  ".into()), ..Default::default() }.clamped();
        assert_eq!((wild.round, wild.text_size, wild.nav_width, wild.font), (Some(40.0), 1.2, 232.0, None));
        // Written out, the old switch is gone.
        assert!(!serde_json::to_string(&LookPrefs::default()).unwrap().contains("liquid_glass"));
    }

    #[test]
    fn themes_round_trip_by_name() {
        for t in Theme::ALL {
            assert_eq!(Theme::from_key(t.key()), t);
        }
        assert_eq!(Theme::from_key("nonsense"), Theme::Dark);
    }

    #[test]
    fn custom_theme_reads_old_files() {
        let c: CustomTheme = serde_json::from_str(r#"{"accent":[1,2,3]}"#).unwrap();
        assert_eq!(c.accent, [1, 2, 3]);
        assert_eq!(c.background, CustomTheme::default().background);
    }
}
