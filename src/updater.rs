//! Update notice. On start (and every few hours) Starlite asks GitHub for
//! its latest release. When that's newer than this copy, a window offers to
//! install it: the release's zip is downloaded, checked against the SHA-256
//! in the release notes, unpacked, and the running Starlite.exe is swapped
//! for the new one and started again. Can be switched off in Settings
//! (updates.json); then nothing is asked at all.
//!
//! Only this app's own release page is ever downloaded from.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use egui::{pos2, vec2, Align2, Color32, Rect, Sense};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::app::{smooth, Starlite};
use crate::glass::{El, Kind};
use crate::icons::Icons;
use crate::widgets::{font, pill};

const LATEST: &str = "https://api.github.com/repos/Turtlemaxx/Starlite/releases/latest";
/// Downloads must come from here.
const DOWNLOADS: &str = "https://github.com/Turtlemaxx/Starlite/releases/download/";
const PAGE: &str = "https://github.com/Turtlemaxx/Starlite/releases/latest";
const VERSION: &str = env!("CARGO_PKG_VERSION");
const AGAIN: Duration = Duration::from_secs(6 * 3600);

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Debug)]
#[serde(default)]
pub struct UpdatePrefs {
    /// Look for new versions and say so.
    pub notify: bool,
}

impl Default for UpdatePrefs {
    fn default() -> Self {
        Self { notify: true }
    }
}

impl UpdatePrefs {
    fn file() -> PathBuf {
        crate::library::data_dir().join("updates.json")
    }
    pub fn load() -> Self {
        std::fs::read_to_string(Self::file()).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
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
}

#[derive(Clone, PartialEq, Debug)]
pub struct Release {
    pub version: String,
    /// The zip to download.
    url: String,
    /// What the zip's SHA-256 must be (from the release notes).
    sha256: String,
}

#[derive(Clone, PartialEq, Debug, Default)]
pub enum Phase {
    #[default]
    Idle,
    Available(Release),
    /// How far the download is (0..1).
    Installing(Release, f32),
    /// The new version is in place and has been started: this one closes.
    Restart,
    Failed(Release, String),
}

pub struct Updater {
    pub prefs: UpdatePrefs,
    phase: Arc<Mutex<Phase>>,
    /// The window is showing.
    pub open: bool,
    open_t: f32,
    /// "Later" was pressed: not again until the next start.
    later: bool,
    checked: Instant,
}

/// Is version `a` newer than `b`? ("0.10.0" is newer than "0.9.3".)
pub fn newer(a: &str, b: &str) -> bool {
    let nums = |s: &str| -> Vec<u32> { s.trim().trim_start_matches('v').split('.').map(|p| p.chars().take_while(|c| c.is_ascii_digit()).collect::<String>().parse().unwrap_or(0)).collect() };
    let (mut x, mut y) = (nums(a), nums(b));
    let n = x.len().max(y.len());
    x.resize(n, 0);
    y.resize(n, 0);
    x > y
}

/// Debug: STARLITE_UPDATE_API=http://127.0.0.1:<port>/latest asks a test
/// server on this PC instead of GitHub.
fn test_api() -> Option<String> {
    std::env::var("STARLITE_UPDATE_API").ok().filter(|u| u.starts_with("http://127.0.0.1:"))
}

/// The release in GitHub's answer, when it has everything an install needs.
fn parse_release(v: &Value, local_test: bool) -> Option<Release> {
    let version = v["tag_name"].as_str()?.trim_start_matches('v').to_string();
    let url = v["assets"].as_array()?.iter().find_map(|a| {
        let name = a["name"].as_str()?;
        (name.starts_with("Starlite-") && name.ends_with("-windows-x64.zip")).then(|| a["browser_download_url"].as_str()).flatten()
    })?;
    if !(url.starts_with(DOWNLOADS) || (local_test && url.starts_with("http://127.0.0.1:"))) {
        return None;
    }
    // "SHA-256: `ABC…`" in the notes.
    let body = v["body"].as_str().unwrap_or("");
    let at = body.find("SHA-256")?;
    let sha: String = body[at + 7..].chars().skip_while(|c| !c.is_ascii_hexdigit()).take_while(|c| c.is_ascii_hexdigit()).collect();
    (sha.len() == 64).then(|| Release { version, url: url.to_string(), sha256: sha.to_lowercase() })
}

fn latest() -> Option<Release> {
    let test = test_api();
    let r = ureq::get(test.as_deref().unwrap_or(LATEST)).timeout(Duration::from_secs(12)).set("User-Agent", concat!("Starlite v", env!("CARGO_PKG_VERSION"))).set("Accept", "application/vnd.github+json").call().ok()?;
    parse_release(&r.into_json().ok()?, test.is_some())
}

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect()
}

/// Downloads, checks and unpacks the release; returns the new Starlite.exe.
fn fetch(rel: &Release, progress: &dyn Fn(f32)) -> Result<PathBuf, String> {
    let resp = ureq::get(&rel.url).timeout(Duration::from_secs(300)).call().map_err(|e| match e {
        ureq::Error::Status(code, _) => format!("The download answered with error {code}"),
        ureq::Error::Transport(t) => format!("Couldn't download the update ({})", t.kind()),
    })?;
    let total: f32 = resp.header("Content-Length").and_then(|v| v.parse().ok()).unwrap_or(0.0);
    let mut reader = resp.into_reader().take(300 << 20);
    let (mut bytes, mut buf) = (Vec::new(), vec![0u8; 64 * 1024]);
    loop {
        match reader.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => {
                bytes.extend_from_slice(&buf[..n]);
                if total > 0.0 {
                    progress((bytes.len() as f32 / total).min(1.0));
                }
            }
            Err(e) => return Err(format!("The download broke off ({e})")),
        }
    }
    if sha256_hex(&bytes) != rel.sha256 {
        return Err("The download doesn't match the release's checksum, so it wasn't installed".into());
    }
    let dir = crate::library::data_dir().join("update");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).map_err(|e| format!("Couldn't unpack the update ({e})"))?;
    let zip = dir.join("update.zip");
    std::fs::write(&zip, &bytes).map_err(|e| format!("Couldn't save the update ({e})"))?;
    // Windows' own tar unpacks zips (from System32, not whatever is on the PATH).
    let tar = PathBuf::from(std::env::var("WINDIR").unwrap_or_else(|_| r"C:\Windows".into())).join("System32").join("tar.exe");
    let mut cmd = std::process::Command::new(tar);
    cmd.arg("-xf").arg(&zip).arg("-C").arg(&dir);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // no console window
    }
    let ok = cmd.status().map(|s| s.success()).unwrap_or(false);
    let exe = dir.join("Starlite.exe");
    if !ok || !exe.is_file() {
        return Err("Couldn't unpack the update".into());
    }
    Ok(exe)
}

/// Puts `new` where the running Starlite.exe is (which moves aside as
/// Starlite.old.exe; a running program can be renamed but not overwritten)
/// and starts it.
fn swap_and_start(new: &Path) -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let old = exe.with_file_name("Starlite.old.exe");
    let _ = std::fs::remove_file(&old);
    let cant = |e: std::io::Error| format!("Starlite can't change its own folder ({e}). Download the new version from the release page instead.");
    std::fs::rename(&exe, &old).map_err(cant)?;
    if let Err(e) = std::fs::copy(new, &exe) {
        let _ = std::fs::rename(&old, &exe);
        return Err(cant(e));
    }
    // The notices that come with it.
    let _ = std::fs::copy(new.with_file_name("THIRD-PARTY-NOTICES.txt"), exe.with_file_name("THIRD-PARTY-NOTICES.txt"));
    let mut cmd = std::process::Command::new(&exe);
    cmd.env("STARLITE_UPDATED", "1").env_remove("STARLITE_UPDATE_INSTALL").env_remove("STARLITE_SHOTS");
    if let Ok(d) = std::env::current_dir() {
        cmd.current_dir(d);
    }
    cmd.spawn().map(|_| ()).map_err(|e| format!("The new version is in place but didn't start ({e}). Start Starlite again."))
}

impl Updater {
    pub fn start(ctx: &egui::Context) -> Self {
        // What the last update left behind.
        if let Ok(exe) = std::env::current_exe() {
            let _ = std::fs::remove_file(exe.with_file_name("Starlite.old.exe"));
        }
        let _ = std::fs::remove_dir_all(crate::library::data_dir().join("update"));
        let me = Self { prefs: UpdatePrefs::load(), phase: Default::default(), open: false, open_t: 0.0, later: false, checked: Instant::now() };
        me.check(ctx, Duration::from_secs(3));
        me
    }

    /// Asks for the latest release in the background (when notices are on).
    fn check(&self, ctx: &egui::Context, wait: Duration) {
        if !self.prefs.notify {
            return;
        }
        let (phase, ctx) = (self.phase.clone(), ctx.clone());
        std::thread::spawn(move || {
            std::thread::sleep(wait);
            if let Some(rel) = latest().filter(|r| newer(&r.version, VERSION)) {
                let mut p = phase.lock().unwrap();
                if matches!(*p, Phase::Idle | Phase::Available(_)) {
                    *p = Phase::Available(rel);
                    ctx.request_repaint();
                }
            }
        });
    }

    pub fn phase(&self) -> Phase {
        self.phase.lock().unwrap().clone()
    }

    /// Every frame: opens the window when there's news, looks again now and then.
    pub fn tick(&mut self, ctx: &egui::Context) {
        if self.checked.elapsed() > AGAIN {
            self.checked = Instant::now();
            self.check(ctx, Duration::ZERO);
        }
        let phase = self.phase();
        if self.prefs.notify && !self.later && !self.open && matches!(phase, Phase::Available(_)) {
            self.open = true;
            self.open_t = 0.0;
        }
        if phase == Phase::Restart {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }

    /// Notices switched on or off in Settings.
    pub fn set_notify(&mut self, on: bool, ctx: &egui::Context) {
        self.prefs.notify = on;
        self.prefs.save();
        if on {
            self.later = false;
            self.check(ctx, Duration::ZERO);
        } else {
            self.open = false;
        }
    }

    fn install(&self, rel: Release, ctx: &egui::Context) {
        let (phase, ctx) = (self.phase.clone(), ctx.clone());
        *phase.lock().unwrap() = Phase::Installing(rel.clone(), 0.0);
        std::thread::spawn(move || {
            let done = fetch(&rel, &|f| {
                *phase.lock().unwrap() = Phase::Installing(rel.clone(), f);
                ctx.request_repaint();
            })
            .and_then(|exe| swap_and_start(&exe));
            *phase.lock().unwrap() = match done {
                Ok(()) => Phase::Restart,
                Err(e) => Phase::Failed(rel.clone(), e),
            };
            ctx.request_repaint();
        });
    }
}

impl Starlite {
    /// The update window: what's new, Later, Install.
    #[allow(clippy::too_many_arguments)]
    pub fn draw_update(&mut self, ui: &mut egui::Ui, icons: &mut Icons, ppp: f32, dt: f32, full: Rect, time: f32, nav_w: f32, animating: &mut bool) {
        let theme = self.theme;
        let ctx = ui.ctx().clone();
        let phase = self.updater.phase();
        let (rel, busy, error) = match &phase {
            Phase::Available(r) => (r.clone(), None, None),
            Phase::Installing(r, f) => (r.clone(), Some(*f), None),
            Phase::Failed(r, e) => (r.clone(), None, Some(e.clone())),
            Phase::Idle | Phase::Restart => {
                self.updater.open = phase == Phase::Restart;
                return;
            }
        };
        self.updater.open_t = (self.updater.open_t + dt * 6.0).min(1.0);
        if self.updater.open_t < 1.0 || busy.is_some() {
            *animating = true;
        }
        let ot = smooth(self.updater.open_t);

        // Block everything behind; clicking outside means "later".
        let back = ui.interact(full, ui.id().with("update_back"), Sense::click());
        let size = vec2(468.0, if error.is_some() { 262.0 } else { 232.0 });
        let area_right = self.card.map(|c| c.left()).unwrap_or(full.right());
        let center = pos2(((nav_w + area_right) * 0.5).max(full.center().x.min(area_right - size.x * 0.5 - 10.0)), full.center().y);
        let panel = Rect::from_center_size(center + vec2(0.0, 18.0 * (1.0 - ot)), size * (0.94 + 0.06 * ot));
        let mut els: Vec<El> = vec![El::rect(panel, ppp, 28.0, 1.0, 14.0, Kind::Glass, 0.0, 0.0)];
        let slot = ui.painter().add(egui::Shape::Noop);
        let p = ui.painter().clone();
        let (fg, dim) = (theme.text().gamma_multiply(ot), theme.text_dim().gamma_multiply(ot));
        let x0 = panel.left() + 28.0;
        let w = panel.width() - 56.0;

        icons.paint(ui, "download", pos2(x0 + 11.0, panel.top() + 36.0), 21.0, theme.accent().gamma_multiply(ot));
        p.text(pos2(x0 + 34.0, panel.top() + 36.0), Align2::LEFT_CENTER, "Update available", font(20.0, "bold"), fg);
        p.text(pos2(x0, panel.top() + 76.0), Align2::LEFT_CENTER, format!("Starlite {} is out. You have {VERSION}.", rel.version), font(14.5, "semibold"), fg);
        let note = "Install downloads it, swaps the app and starts it again. Your music, playlists and settings stay as they are.";
        let g = p.layout(note.to_string(), font(12.5, "regular"), dim, w);
        p.galley(pos2(x0, panel.top() + 94.0), g, dim);
        if let Some(e) = &error {
            let g = p.layout(e.clone(), font(12.5, "semibold"), Color32::from_rgb(235, 90, 100).gamma_multiply(ot), w);
            p.galley(pos2(x0, panel.top() + 138.0), g, Color32::from_rgb(235, 90, 100));
        }

        let by = panel.bottom() - 28.0 - 40.0;
        let label = match busy {
            Some(f) if f >= 0.999 => "Installing\u{2026}".to_string(),
            Some(f) => format!("Downloading {:.0} %", f * 100.0),
            None if error.is_some() => "Try again".to_string(),
            None => "Install".to_string(),
        };
        let iw = if busy.is_some() { 170.0 } else { 124.0 };
        let install_r = Rect::from_min_size(pos2(panel.right() - 28.0 - iw, by), vec2(iw, 40.0));
        let later_r = Rect::from_min_size(pos2(install_r.left() - 12.0 - 100.0, by), vec2(100.0, 40.0));
        let new_r = Rect::from_min_size(pos2(x0, by), vec2(142.0, 40.0));
        let whats_new = pill(ui, icons, &mut els, ppp, ui.id().with("upd_new"), new_r, "What's new", Some("external-link"), false, true, theme);
        let later = busy.is_none() && pill(ui, icons, &mut els, ppp, ui.id().with("upd_later"), later_r, "Later", None, false, true, theme).clicked();
        let install = pill(ui, icons, &mut els, ppp, ui.id().with("upd_install"), install_r, &label, if busy.is_some() { None } else { Some("download") }, true, true, theme);
        let mut frame = self.base_frame(full, ppp, time, nav_w);
        frame.overlay = true;
        frame.dim = 0.45 * ot;
        self.set_frame(ui, slot, full, frame, &els);

        // Debug: STARLITE_UPDATE_INSTALL=1 presses Install once.
        static AUTO: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
        let auto = std::env::var("STARLITE_UPDATE_INSTALL").is_ok() && busy.is_none() && error.is_none() && !AUTO.swap(true, std::sync::atomic::Ordering::Relaxed);
        if whats_new.clicked() {
            ctx.open_url(egui::OpenUrl::new_tab(PAGE));
        }
        if (install.clicked() || auto) && busy.is_none() {
            self.sound.click();
            self.updater.install(rel, &ctx);
        } else if busy.is_none() && (later || (back.clicked() && !back.interact_pointer_pos().is_some_and(|q| panel.contains(q))) || ui.input(|i| i.key_pressed(egui::Key::Escape))) {
            self.sound.click();
            self.updater.open = false;
            self.updater.later = true;
            // A failed try starts fresh next time.
            if error.is_some() {
                *self.updater.phase.lock().unwrap() = Phase::Idle;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_compare_by_number() {
        assert!(newer("0.3.1", "0.3.0") && newer("v0.10.0", "0.9.3") && newer("1.0", "0.9.9"));
        assert!(!newer("0.3.0", "0.3.0") && !newer("0.2.9", "0.3.0") && !newer("0.3", "0.3.0"));
    }

    fn answer(url: &str, body: &str) -> Value {
        serde_json::json!({ "tag_name": "v0.4.0", "body": body, "assets": [{ "name": "notes.txt", "browser_download_url": "https://example.com/n" }, { "name": "Starlite-0.4.0-windows-x64.zip", "browser_download_url": url }] })
    }

    #[test]
    fn a_release_needs_our_download_and_its_checksum() {
        let sha = "A1".repeat(32);
        let good = format!("{DOWNLOADS}v0.4.0/Starlite-0.4.0-windows-x64.zip");
        let r = parse_release(&answer(&good, &format!("New things.\n\nSHA-256: `{sha}`")), false).unwrap();
        assert_eq!((r.version.as_str(), r.url.as_str(), r.sha256), ("0.4.0", good.as_str(), sha.to_lowercase()));
        // No checksum in the notes: nothing to check the download against.
        assert_eq!(parse_release(&answer(&good, "New things."), false), None);
        // A download from anywhere else is never taken.
        assert_eq!(parse_release(&answer("https://evil.example/Starlite-0.4.0-windows-x64.zip", &format!("SHA-256: `{sha}`")), false), None);
        assert_eq!(parse_release(&answer("http://127.0.0.1:9/Starlite-0.4.0-windows-x64.zip", &format!("SHA-256: `{sha}`")), false), None);
    }

    #[test]
    fn checksums_are_sha256() {
        assert_eq!(sha256_hex(b"abc"), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
    }
}
