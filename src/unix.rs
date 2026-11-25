//! Linux counterparts of `win.rs` (same functions, used as `crate::win`).
//! Starlite runs on X11 there (XWayland under Wayland, see main.rs), so the
//! mini player can be placed and kept on top. What Windows asks the system
//! for, Starlite tells this module itself each frame (`note_*`).

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Mutex;

static FOCUSED: AtomicBool = AtomicBool::new(true);
static MINI_HOVER: AtomicBool = AtomicBool::new(false);
static MONITOR: Mutex<[f32; 2]> = Mutex::new([1920.0, 1080.0]);
static CURSOR: Mutex<(f32, f32)> = Mutex::new((0.0, 0.0));
static HZ: AtomicU32 = AtomicU32::new(60);

/// The main window has the keyboard focus (from egui).
pub fn note_focus(focused: bool) {
    FOCUSED.store(focused, Ordering::Relaxed);
}

/// The size of the main window's monitor (physical px, from egui).
pub fn note_monitor(w: f32, h: f32) {
    if w > 100.0 && h > 100.0 {
        *MONITOR.lock().unwrap() = [w, h];
    }
}

/// The mouse is over the mini player (from its own window).
pub fn note_mini_hover(over: bool) {
    MINI_HOVER.store(over, Ordering::Relaxed);
}

/// Where the mouse is on the screen (physical px, from the mini player).
pub fn note_cursor(x: f32, y: f32) {
    *CURSOR.lock().unwrap() = (x, y);
}

/// Work area and full bounds of the monitor (physical px). Without a
/// portable way to ask where panels are, both are the whole monitor.
pub fn monitor_rects(_x: i32, _y: i32) -> ([f32; 4], [f32; 4]) {
    let [w, h] = *MONITOR.lock().unwrap();
    let r = [0.0, 0.0, w, h];
    (r, r)
}

pub fn cursor_pos() -> (f32, f32) {
    *CURSOR.lock().unwrap()
}

/// Only asked about the mini player.
pub fn cursor_in_window(_hwnd: isize) -> bool {
    MINI_HOVER.load(Ordering::Relaxed)
}

/// Says "yes" so the mini player never tries to slide past the screen's
/// edge (window managers keep windows on screen): it grows in place instead.
pub fn monitor_at(_x: f32, _y: f32) -> bool {
    true
}

/// The mini player's window: there's only one, so any handle will do.
pub fn find_window(_title: &str) -> Option<isize> {
    Some(1)
}

pub fn app_in_front(_ignore: Option<isize>) -> bool {
    FOCUSED.load(Ordering::Relaxed)
}

pub fn taskbar_is_light() -> bool {
    false
}

/// Windows-only (the blur behind the window); nothing to do here.
pub fn set_backdrop(_hwnd: isize, _on: bool, _dark: bool) {}

pub fn keep_own_buttons(_hwnd: isize, _blur_on: bool) {}

pub fn style_window(_hwnd: isize, _light: bool) {}

pub fn refresh_hz() -> f64 {
    HZ.load(Ordering::Relaxed) as f64
}

pub fn refreshes_for(interval: std::time::Duration) -> u32 {
    (refresh_hz() * interval.as_secs_f64()).round().max(1.0) as u32
}

/// Vsync paces frames on Linux; there's nothing to wait for here.
pub fn wait_for_refresh() -> bool {
    false
}

/// Installed fonts from fontconfig: ("Family Style", "/path/to/file.ttf").
pub fn font_table() -> Vec<(String, String)> {
    let Ok(out) = std::process::Command::new("fc-list").args([":", "family", "style", "file"]).arg("--format=%{family[0]}\t%{style[0]}\t%{file}\n").output() else {
        return Vec::new();
    };
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter_map(|l| {
            let mut it = l.split('\t');
            let (family, style, file) = (it.next()?, it.next()?, it.next()?);
            Some((format!("{family} {style}"), file.to_string()))
        })
        .collect()
}

/// Opens a folder, file or https link with the desktop's default app.
pub fn open(target: &std::ffi::OsStr) {
    let _ = std::process::Command::new("xdg-open").arg(target).spawn();
}

/// Opens a text file for editing (the default app for it).
pub fn edit_text(path: &std::path::Path) -> bool {
    std::process::Command::new("xdg-open").arg(path).spawn().is_ok()
}

/// The user's region (e.g. "DE") from the locale, for local charts.
pub fn region() -> String {
    let lang = ["LC_ALL", "LC_MESSAGES", "LANG"].iter().find_map(|k| std::env::var(k).ok().filter(|v| !v.is_empty())).unwrap_or_default();
    lang.split(['.', '@']).next().and_then(|l| l.split('_').nth(1)).filter(|r| r.len() == 2 && r.chars().all(|c| c.is_ascii_uppercase())).unwrap_or("US").to_string()
}

/// The fonts Starlite draws with when no theme names one: the first of these
/// that's installed.
pub const UI_FONTS: &[&str] = &["Inter", "Noto Sans", "Cantarell", "Ubuntu", "Open Sans", "DejaVu Sans", "Liberation Sans"];
