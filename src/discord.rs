//! Discord Rich Presence: "Listening to Starlite" with the song, artist,
//! album cover and a time bar, over Discord's local IPC pipe.
//!
//! Needs a Discord application (its Application ID goes in Settings). The
//! Starlite icon is that application's Rich Presence art asset named
//! `starlite`. Covers must be public links: YouTube thumbnails are; local
//! songs and Navidrome songs get theirs from the iTunes catalogue (optional),
//! and Navidrome cover links (they carry the login token) are never sent.

use std::collections::HashMap;
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use discord_rich_presence::activity::{Activity, ActivityType, Assets, StatusDisplayType, Timestamps};
use discord_rich_presence::{DiscordIpc, DiscordIpcClient};
use serde::{Deserialize, Serialize};

use crate::library::{Source, Track};
use crate::youtube::enc;

/// The art asset name to upload the white Starlite icon under.
pub const ICON_ASSET: &str = "starlite";

#[derive(Serialize, Deserialize, Clone, PartialEq)]
#[serde(default)]
pub struct DiscordPrefs {
    pub enabled: bool,
    pub app_id: String,
    /// The album cover (else always the Starlite icon).
    pub show_cover: bool,
    pub show_title: bool,
    pub show_artist: bool,
    /// Album name (shown when hovering the picture).
    pub show_album: bool,
    /// Elapsed / remaining time bar.
    pub show_time: bool,
    /// Keep the status while paused.
    pub show_paused: bool,
    /// Find covers for local / Navidrome songs in the iTunes catalogue.
    pub lookup_covers: bool,
}

impl Default for DiscordPrefs {
    fn default() -> Self {
        Self {
            enabled: true,
            app_id: String::new(),
            show_cover: true,
            show_title: true,
            show_artist: true,
            show_album: true,
            show_time: true,
            show_paused: true,
            lookup_covers: true,
        }
    }
}

impl DiscordPrefs {
    fn file() -> std::path::PathBuf {
        crate::library::data_dir().join("discord.json")
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

/// What the presence should say right now (None = clear it).
#[derive(Clone, PartialEq, Debug)]
pub struct Presence {
    pub title: String,
    pub artist: String,
    pub album: Option<String>,
    /// A public cover link (YouTube) or a lookup key for the catalogue.
    pub cover: Cover,
    pub playing: bool,
    /// Song start / end in Unix ms (for the time bar), when known.
    pub start_ms: Option<i64>,
    pub end_ms: Option<i64>,
}

#[derive(Clone, PartialEq, Debug)]
pub enum Cover {
    None,
    Url(String),
    Lookup { artist: String, album: Option<String>, title: String },
}

enum Msg {
    Show(DiscordPrefs, Option<Presence>),
}

pub struct Discord {
    tx: Sender<Msg>,
    /// Shown in Settings.
    pub status: Arc<Mutex<String>>,
    last: Option<(DiscordPrefs, Option<Presence>)>,
}

fn now_ms() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0)
}

impl Discord {
    pub fn start() -> Self {
        let (tx, rx) = channel();
        let status = Arc::new(Mutex::new(String::new()));
        let st = status.clone();
        std::thread::Builder::new().name("discord-rpc".into()).spawn(move || worker(rx, st)).expect("spawn discord thread");
        Self { tx, status, last: None }
    }

    /// Call every frame with the current song; only real changes are sent.
    pub fn update(&mut self, prefs: &DiscordPrefs, track: Option<&Track>, playing: bool, pos: f64, dur: f64) {
        let presence = track.filter(|_| prefs.enabled && (playing || prefs.show_paused)).map(|t| {
            let start = now_ms() - (pos * 1000.0) as i64;
            let cover = match &t.source {
                Source::YouTube { .. } => t.artwork.clone().filter(|a| a.starts_with("https://")).map(Cover::Url).unwrap_or(Cover::None),
                // Local files and Navidrome: never send their (private) links.
                _ => Cover::Lookup { artist: t.artist.clone(), album: t.album.clone(), title: t.title.clone() },
            };
            Presence {
                title: t.title.clone(),
                artist: t.artist.clone(),
                album: t.album.clone().filter(|a| !a.is_empty()),
                cover,
                playing,
                start_ms: (playing && dur > 0.0).then_some(start),
                end_ms: (playing && dur > 0.0).then(|| start + (dur * 1000.0) as i64),
            }
        });
        let next = (prefs.clone(), presence);
        // Ignore clock drift between frames; a seek (> 2 s jump) is a change.
        let same = self.last.as_ref().is_some_and(|(lp, lpr)| {
            lp == &next.0
                && match (lpr, &next.1) {
                    (None, None) => true,
                    (Some(a), Some(b)) => {
                        let near = |x: Option<i64>, y: Option<i64>| match (x, y) {
                            (Some(x), Some(y)) => (x - y).abs() < 2000,
                            (None, None) => true,
                            _ => false,
                        };
                        Presence { start_ms: None, end_ms: None, ..a.clone() } == Presence { start_ms: None, end_ms: None, ..b.clone() }
                            && near(a.start_ms, b.start_ms)
                            && near(a.end_ms, b.end_ms)
                    }
                    _ => false,
                }
        });
        if !same {
            let _ = self.tx.send(Msg::Show(next.0.clone(), next.1.clone()));
            self.last = Some(next);
        }
    }

    pub fn status(&self) -> String {
        self.status.lock().unwrap().clone()
    }
}

/// Cover art from the iTunes catalogue (public, no key), cached.
fn lookup_cover(cache: &mut HashMap<String, Option<String>>, artist: &str, album: Option<&str>, title: &str) -> Option<String> {
    let (term, entity) = match album {
        Some(a) => (format!("{artist} {a}"), "album"),
        None => (format!("{artist} {title}"), "song"),
    };
    if let Some(hit) = cache.get(&term) {
        return hit.clone();
    }
    let url = format!("https://itunes.apple.com/search?term={}&entity={entity}&limit=1", enc(&term));
    let found = ureq::get(&url)
        .timeout(Duration::from_secs(8))
        .set("User-Agent", "Starlite")
        .call()
        .ok()
        .and_then(|r| r.into_json::<serde_json::Value>().ok())
        .and_then(|v| v["results"][0]["artworkUrl100"].as_str().map(|s| s.replace("100x100bb", "512x512bb")))
        .filter(|u| u.starts_with("https://"));
    cache.insert(term, found.clone());
    found
}

/// The activity Discord gets, following the "What to show" switches.
fn build_activity<'a>(prefs: &DiscordPrefs, p: &Presence, cover: Option<String>) -> Activity<'a> {
    let image = cover.filter(|_| prefs.show_cover).unwrap_or_else(|| ICON_ASSET.to_string());
    let hover = match (&p.album, prefs.show_album) {
        (Some(a), true) => a.clone(),
        _ => "Starlite".to_string(),
    };
    let mut details = if prefs.show_title { p.title.clone() } else { "Listening to music".to_string() };
    if !p.playing {
        details = format!("{details} (paused)");
    }
    let mut act = Activity::new()
        .activity_type(ActivityType::Listening)
        .status_display_type(if prefs.show_title { StatusDisplayType::Details } else { StatusDisplayType::Name })
        .details(details)
        .assets(Assets::new().large_image(image).large_text(hover));
    if prefs.show_artist && !p.artist.is_empty() {
        act = act.state(p.artist.clone());
    }
    if prefs.show_time {
        if let (Some(s), Some(e)) = (p.start_ms, p.end_ms) {
            act = act.timestamps(Timestamps::new().start(s).end(e));
        }
    }
    act
}

fn worker(rx: Receiver<Msg>, status: Arc<Mutex<String>>) {
    let set = |s: &str| *status.lock().unwrap() = s.to_string();
    let mut client: Option<DiscordIpcClient> = None;
    let mut client_id = String::new();
    let mut want: Option<(DiscordPrefs, Option<Presence>)> = None;
    let mut dirty = false;
    let mut last_send = Instant::now() - Duration::from_secs(60);
    let mut retry_at = Instant::now();
    let mut covers: HashMap<String, Option<String>> = HashMap::new();
    loop {
        match rx.recv_timeout(Duration::from_millis(500)) {
            Ok(Msg::Show(p, pr)) => {
                want = Some((p, pr));
                dirty = true;
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => break,
        }
        // Coalesce bursts (Discord allows ~5 updates per 20 s).
        while let Ok(Msg::Show(p, pr)) = rx.try_recv() {
            want = Some((p, pr));
            dirty = true;
        }
        let Some((prefs, presence)) = want.clone() else { continue };
        let id = prefs.app_id.trim().to_string();

        if !prefs.enabled || id.is_empty() {
            if let Some(mut c) = client.take() {
                let _ = c.clear_activity();
                let _ = c.close();
            }
            set(if !prefs.enabled { "Off" } else { "Add your Discord Application ID to turn it on" });
            dirty = false;
            continue;
        }
        if id != client_id {
            if let Some(mut c) = client.take() {
                let _ = c.close();
            }
            client_id = id.clone();
            retry_at = Instant::now();
        }
        if client.is_none() {
            if Instant::now() < retry_at {
                continue;
            }
            let mut c = DiscordIpcClient::new(&client_id);
            match c.connect() {
                Ok(()) => {
                    client = Some(c);
                    dirty = true;
                    set("Connected to Discord");
                }
                Err(_) => {
                    set("Discord isn't running (retrying)");
                    retry_at = Instant::now() + Duration::from_secs(15);
                    continue;
                }
            }
        }
        if !dirty || last_send.elapsed() < Duration::from_secs(4) {
            continue;
        }
        let Some(c) = client.as_mut() else { continue };
        let res = match &presence {
            None => c.clear_activity(),
            Some(p) => {
                let cover = if !prefs.show_cover {
                    None
                } else {
                    match &p.cover {
                        Cover::Url(u) => Some(u.clone()),
                        Cover::Lookup { artist, album, title } if prefs.lookup_covers => lookup_cover(&mut covers, artist, album.as_deref(), title),
                        _ => None,
                    }
                };
                c.set_activity(build_activity(&prefs, p, cover))
            }
        };
        last_send = Instant::now();
        match res {
            Ok(()) => {
                dirty = false;
                set("Connected to Discord");
            }
            Err(_) => {
                // Discord closed (or the ID is wrong): reconnect later.
                if let Some(mut c) = client.take() {
                    let _ = c.close();
                }
                set("Lost Discord (retrying). Check the Application ID if this stays.");
                retry_at = Instant::now() + Duration::from_secs(10);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn song() -> Presence {
        Presence {
            title: "All Falls Down".into(),
            artist: "Kanye West".into(),
            album: Some("The College Dropout".into()),
            cover: Cover::None,
            playing: true,
            start_ms: Some(1_000),
            end_ms: Some(224_000),
        }
    }

    fn json(prefs: &DiscordPrefs, p: &Presence, cover: Option<&str>) -> serde_json::Value {
        serde_json::to_value(build_activity(prefs, p, cover.map(String::from))).unwrap()
    }

    #[test]
    fn everything_on() {
        let v = json(&DiscordPrefs::default(), &song(), Some("https://img/cover.jpg"));
        assert_eq!(v["type"], 2, "Listening");
        assert_eq!(v["details"], "All Falls Down");
        assert_eq!(v["state"], "Kanye West");
        assert_eq!(v["assets"]["large_image"], "https://img/cover.jpg");
        assert_eq!(v["assets"]["large_text"], "The College Dropout");
        assert_eq!(v["timestamps"]["start"], 1_000);
        assert_eq!(v["timestamps"]["end"], 224_000);
    }

    #[test]
    fn no_cover_uses_the_starlite_icon() {
        let v = json(&DiscordPrefs::default(), &song(), None);
        assert_eq!(v["assets"]["large_image"], ICON_ASSET);
        let off = DiscordPrefs { show_cover: false, ..Default::default() };
        let v = json(&off, &song(), Some("https://img/cover.jpg"));
        assert_eq!(v["assets"]["large_image"], ICON_ASSET, "cover switched off");
    }

    #[test]
    fn hidden_fields_are_left_out() {
        let prefs = DiscordPrefs { show_title: false, show_artist: false, show_album: false, show_time: false, ..Default::default() };
        let v = json(&prefs, &song(), None);
        assert_eq!(v["details"], "Listening to music");
        assert!(v.get("state").is_none(), "no artist: {v}");
        assert!(v.get("timestamps").is_none(), "no time bar: {v}");
        assert_eq!(v["assets"]["large_text"], "Starlite");
        assert!(!v.to_string().contains("Kanye") && !v.to_string().contains("All Falls Down") && !v.to_string().contains("Dropout"));
    }

    #[test]
    fn paused_is_marked() {
        let p = Presence { playing: false, start_ms: None, end_ms: None, ..song() };
        let v = json(&DiscordPrefs::default(), &p, None);
        assert_eq!(v["details"], "All Falls Down (paused)");
        assert!(v.get("timestamps").is_none());
    }

    #[test]
    fn private_links_never_become_covers() {
        // Navidrome / local songs must be looked up, never sent as their own links.
        let mut d = Discord { tx: channel().0, status: Default::default(), last: None };
        let t = Track {
            id: "navidrome:1".into(),
            title: "X".into(),
            artist: "Y".into(),
            album: None,
            artwork: Some("https://nd.example/rest/getCoverArt.view?id=1&t=SECRETTOKEN".into()),
            permalink: None,
            duration_ms: 1000,
            source: Source::Navidrome { song_id: "1".into(), suffix: "mp3".into() },
        };
        d.update(&DiscordPrefs::default(), Some(&t), true, 0.0, 1.0);
        let (_, pr) = d.last.clone().unwrap();
        assert!(matches!(pr.unwrap().cover, Cover::Lookup { .. }));
    }
}
