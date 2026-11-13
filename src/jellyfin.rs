//! Jellyfin client: sign in, list the playlists, stream their songs.
//!
//! The password is sent to the server once to sign in; only the access token
//! it answers with is saved (%APPDATA%\Starlite\jellyfin.json), never the
//! password itself.

use std::sync::Mutex;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::library::{Source, Track};
use crate::stream::StreamPrefs;
use crate::youtube::{enc, RemotePlaylist};

#[derive(Clone, Serialize, Deserialize)]
struct Conn {
    url: String,
    user: String,
    user_id: String,
    token: String,
    device_id: String,
}

pub struct Jellyfin {
    conn: Mutex<Option<Conn>>,
    agent: ureq::Agent,
}

fn file() -> std::path::PathBuf {
    crate::library::data_dir().join("jellyfin.json")
}

/// Covers are stored as `jellyfin-cover:<id>`; the server address is only
/// added when one is shown.
pub const COVER_SCHEME: &str = "jellyfin-cover:";

/// The current login, for turning cover ids into links.
static ACTIVE: Mutex<Option<Conn>> = Mutex::new(None);

/// The link for a `jellyfin-cover:` id (None when not connected).
pub fn resolve_cover(uri: &str) -> Option<String> {
    let id = uri.strip_prefix(COVER_SCHEME)?;
    let c = ACTIVE.lock().unwrap().clone()?;
    Some(format!("{}/Items/{}/Images/Primary?maxWidth=400&quality=90&api_key={}", c.url, enc(id), c.token))
}

/// What Starlite can play as it is: "container|codec" the way Jellyfin's
/// universal audio endpoint wants it. Anything else the server converts.
const DIRECT: &str = "mp3,flac,wav,ogg|vorbis,oga|vorbis,m4a|aac,m4a|alac,mp4|aac,aac";

fn random_id() -> String {
    let mut b = [0u8; 16];
    let _ = getrandom::getrandom(&mut b);
    b.iter().map(|x| format!("{x:02x}")).collect()
}

/// "jellyfin.example.com/web/#/home" → "https://jellyfin.example.com".
fn normalize(url: &str) -> String {
    let mut u = url.trim().to_string();
    if !u.contains("://") {
        let host = u.split(['/', ':', '#']).next().unwrap_or("").to_lowercase();
        let local = host.parse::<std::net::Ipv4Addr>().map(|ip| ip.is_private() || ip.is_loopback()).unwrap_or(host == "localhost" || !host.contains('.') || host.ends_with(".local") || host.ends_with(".lan"));
        u = format!("{}://{u}", if local { "http" } else { "https" });
    }
    for cut in ["/web", "#"] {
        if let Some(i) = u.find(cut) {
            u.truncate(i);
        }
    }
    u.trim_end_matches('/').to_string()
}

impl Jellyfin {
    pub fn load() -> Self {
        let conn: Option<Conn> = std::fs::read_to_string(file()).ok().and_then(|s| serde_json::from_str(&s).ok());
        *ACTIVE.lock().unwrap() = conn.clone();
        Self { conn: Mutex::new(conn), agent: ureq::AgentBuilder::new().timeout_connect(Duration::from_secs(8)).timeout_read(Duration::from_secs(30)).build() }
    }

    /// (server URL, user name) when connected.
    pub fn connected(&self) -> Option<(String, String)> {
        self.conn.lock().unwrap().as_ref().map(|c| (c.url.clone(), c.user.clone()))
    }

    /// The header Jellyfin identifies apps (and sign-ins) by.
    fn auth(device_id: &str, token: Option<&str>) -> String {
        let mut h = format!("MediaBrowser Client=\"Starlite\", Device=\"PC\", DeviceId=\"{device_id}\", Version=\"{}\"", env!("CARGO_PKG_VERSION"));
        if let Some(t) = token {
            h += &format!(", Token=\"{t}\"");
        }
        h
    }

    fn fail(e: ureq::Error) -> String {
        match e {
            ureq::Error::Status(401, _) => "Wrong username or password".to_string(),
            ureq::Error::Status(404, _) => "No Jellyfin server at that address".to_string(),
            ureq::Error::Status(code, _) => format!("The server answered with error {code}"),
            ureq::Error::Transport(t) => format!("Can't reach the server ({})", t.kind()),
        }
    }

    fn get(&self, c: &Conn, path: &str) -> Result<Value, String> {
        let r = self.agent.get(&format!("{}{path}", c.url)).set("Authorization", &Self::auth(&c.device_id, Some(&c.token))).call().map_err(|e| match e {
            ureq::Error::Status(401, _) => "Jellyfin signed you out. Connect again.".to_string(),
            e => Self::fail(e),
        })?;
        r.into_json().map_err(|_| "That doesn't look like a Jellyfin server".to_string())
    }

    fn current(&self) -> Result<Conn, String> {
        self.conn.lock().unwrap().clone().ok_or_else(|| "Not connected to Jellyfin".to_string())
    }

    /// Signs in with the server, then remembers the token it gives.
    pub fn connect(&self, url: &str, user: &str, password: &str) -> Result<(), String> {
        if url.trim().is_empty() || user.trim().is_empty() {
            return Err("Fill in the server address and username".into());
        }
        let url = normalize(url);
        let device_id = random_id();
        let r = self
            .agent
            .post(&format!("{url}/Users/AuthenticateByName"))
            .set("Authorization", &Self::auth(&device_id, None))
            .send_json(serde_json::json!({ "Username": user.trim(), "Pw": password }))
            .map_err(Self::fail)?;
        let v: Value = r.into_json().map_err(|_| "That doesn't look like a Jellyfin server".to_string())?;
        let (Some(token), Some(user_id)) = (v["AccessToken"].as_str(), v["User"]["Id"].as_str()) else {
            return Err("That doesn't look like a Jellyfin server".into());
        };
        let c = Conn { url, user: v["User"]["Name"].as_str().unwrap_or(user.trim()).to_string(), user_id: user_id.to_string(), token: token.to_string(), device_id };
        if let Ok(s) = serde_json::to_string_pretty(&c) {
            let f = file();
            if let Some(d) = f.parent() {
                let _ = std::fs::create_dir_all(d);
            }
            let _ = std::fs::write(f, s);
        }
        *ACTIVE.lock().unwrap() = Some(c.clone());
        *self.conn.lock().unwrap() = Some(c);
        Ok(())
    }

    /// Forgets the login (and tells the server the token is done with).
    pub fn disconnect(&self) {
        if let Some(c) = self.conn.lock().unwrap().take() {
            let _ = self.agent.post(&format!("{}/Sessions/Logout", c.url)).set("Authorization", &Self::auth(&c.device_id, Some(&c.token))).timeout(Duration::from_secs(4)).call();
        }
        *ACTIVE.lock().unwrap() = None;
        let _ = std::fs::remove_file(file());
    }

    /// The user's playlists on the server.
    pub fn playlists(&self) -> Result<Vec<RemotePlaylist>, String> {
        let c = self.current()?;
        let v = self.get(&c, &format!("/Items?userId={}&includeItemTypes=Playlist&recursive=true&fields=ChildCount&sortBy=SortName", enc(&c.user_id)))?;
        let empty = Vec::new();
        Ok(v["Items"]
            .as_array()
            .unwrap_or(&empty)
            .iter()
            // Video playlists aren't for a music player.
            .filter(|p| p["MediaType"].as_str().is_none_or(|m| m != "Video"))
            .filter_map(|p| {
                let id = p["Id"].as_str()?.to_string();
                Some(RemotePlaylist {
                    title: p["Name"].as_str().unwrap_or("Playlist").to_string(),
                    artist: c.user.clone(),
                    artwork: p["ImageTags"]["Primary"].as_str().map(|_| format!("{COVER_SCHEME}{id}")),
                    permalink: None,
                    track_count: p["ChildCount"].as_u64().unwrap_or(0),
                    id,
                })
            })
            .collect())
    }

    /// Songs of one playlist.
    pub fn playlist_tracks(&self, id: &str) -> Result<Vec<Track>, String> {
        let c = self.current()?;
        let v = self.get(&c, &format!("/Playlists/{}/Items?userId={}", enc(id), enc(&c.user_id)))?;
        let empty = Vec::new();
        Ok(v["Items"].as_array().unwrap_or(&empty).iter().filter(|e| e["Type"].as_str().is_none_or(|t| t == "Audio")).filter_map(track).collect())
    }

    /// Where to stream a song from (None when not connected).
    pub fn stream_url(&self, item_id: &str, prefs: &StreamPrefs) -> Option<String> {
        let c = self.conn.lock().unwrap().clone()?;
        Some(if prefs.transcode {
            convert_url(&c, item_id, prefs.format.ext(), (!prefs.format.lossless()).then_some(prefs.bitrate), None)
        } else {
            // As it is when Starlite can play it; else the server makes MP3 of it.
            format!(
                "{}/Audio/{}/universal?userId={}&deviceId={}&api_key={}&container={}&transcodingContainer=mp3&transcodingProtocol=http&audioCodec=mp3&maxStreamingBitrate=320000",
                c.url,
                enc(item_id),
                enc(&c.user_id),
                c.device_id,
                c.token,
                enc(DIRECT)
            )
        })
    }

    /// A small stream for a short preview; `{ticks}` is where it starts.
    pub fn preview_url(&self, item_id: &str) -> Option<String> {
        let c = self.conn.lock().unwrap().clone()?;
        Some(format!("{}&startTimeTicks={{ticks}}", convert_url(&c, item_id, "mp3", Some(160), None)))
    }
}

/// A stream the server converts to `ext` (at `kbps`), optionally from `start_ms`.
fn convert_url(c: &Conn, item_id: &str, ext: &str, kbps: Option<u32>, start_ms: Option<u64>) -> String {
    let codec = if ext == "ogg" { "vorbis" } else { ext };
    let mut u = format!("{}/Audio/{}/stream.{ext}?static=false&audioCodec={codec}&deviceId={}&api_key={}", c.url, enc(item_id), c.device_id, c.token);
    if let Some(k) = kbps {
        u += &format!("&audioBitRate={}", k * 1000);
    }
    if let Some(ms) = start_ms {
        // Jellyfin counts in ticks of 100 ns.
        u += &format!("&startTimeTicks={}", ms * 10_000);
    }
    u
}

fn track(e: &Value) -> Option<Track> {
    let id = e["Id"].as_str()?.to_string();
    let artists: Vec<&str> = e["Artists"].as_array().map(|a| a.iter().filter_map(|x| x.as_str()).collect()).unwrap_or_default();
    let artist = if artists.is_empty() { e["AlbumArtist"].as_str().unwrap_or("Unknown artist").to_string() } else { artists.join(", ") };
    // The song's own picture, else its album's.
    let cover = if e["ImageTags"]["Primary"].is_string() {
        Some(id.clone())
    } else if e["AlbumPrimaryImageTag"].is_string() {
        e["AlbumId"].as_str().map(String::from)
    } else {
        None
    };
    Some(Track {
        id: format!("jellyfin:{id}"),
        title: e["Name"].as_str().unwrap_or("Unknown").to_string(),
        artist,
        album: e["Album"].as_str().map(String::from),
        artwork: cover.map(|c| format!("{COVER_SCHEME}{c}")),
        permalink: None,
        duration_ms: e["RunTimeTicks"].as_u64().unwrap_or(0) / 10_000,
        source: Source::Jellyfin { item_id: id },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn addresses_are_tidied() {
        assert_eq!(normalize("jellyfin.example.com/web/#/home.html"), "https://jellyfin.example.com");
        assert_eq!(normalize("192.168.1.20:8096/"), "http://192.168.1.20:8096");
        assert_eq!(normalize("http://nas:8096/web/index.html"), "http://nas:8096");
    }

    #[test]
    fn songs_are_read_from_the_servers_answer() {
        let e: Value = serde_json::from_str(
            r#"{"Id":"abc","Name":"Neon Skyline","Artists":["Aurora Lane","Kairo"],"Album":"Signals","AlbumId":"al1",
                "AlbumPrimaryImageTag":"t","ImageTags":{},"RunTimeTicks":2150000000,"Type":"Audio"}"#,
        )
        .unwrap();
        let t = track(&e).unwrap();
        assert_eq!((t.id.as_str(), t.artist.as_str(), t.duration_ms), ("jellyfin:abc", "Aurora Lane, Kairo", 215_000));
        assert_eq!(t.artwork.as_deref(), Some("jellyfin-cover:al1"));
        assert_eq!(t.source, Source::Jellyfin { item_id: "abc".into() });
    }

    #[test]
    fn stream_links_follow_the_settings() {
        let c = Conn { url: "http://nas:8096".into(), user: "u".into(), user_id: "uid".into(), token: "tok".into(), device_id: "dev".into() };
        let u = convert_url(&c, "abc", "aac", Some(192), None);
        assert!(u.starts_with("http://nas:8096/Audio/abc/stream.aac?") && u.contains("audioCodec=aac") && u.contains("audioBitRate=192000"));
        let p = convert_url(&c, "abc", "mp3", Some(160), Some(61_500));
        assert!(p.contains("startTimeTicks=615000000"));
        assert!(!convert_url(&c, "abc", "flac", None, None).contains("audioBitRate"));
    }
}
