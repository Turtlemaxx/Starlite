//! Navidrome (or any Subsonic-compatible server) client.
//!
//! Uses the Subsonic REST API with token auth: the password is only used once
//! to make a salted MD5 token, and only the salt + token are saved
//! (%APPDATA%\Starlite\navidrome.json), never the password itself.

use std::sync::Mutex;
use std::time::Duration;

use md5::{Digest, Md5};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::library::{Source, Track};
use crate::youtube::{enc, RemotePlaylist};

const API_VERSION: &str = "1.16.1";
const CLIENT: &str = "Starlite";

#[derive(Clone, Serialize, Deserialize)]
struct Conn {
    url: String,
    user: String,
    salt: String,
    token: String,
}

pub struct Navidrome {
    conn: Mutex<Option<Conn>>,
    agent: ureq::Agent,
}

fn file() -> std::path::PathBuf {
    crate::library::data_dir().join("navidrome.json")
}

/// "music.example.com/app/#/album" → "https://music.example.com". Without
/// a scheme, home-network addresses get http (Navidrome's default) and
/// everything else https, so the login token isn't sent unencrypted.
fn normalize(url: &str) -> String {
    let mut u = url.trim().to_string();
    if !u.contains("://") {
        let host = u.split(['/', ':', '#']).next().unwrap_or("").to_lowercase();
        let scheme = if is_local_host(&host) { "http" } else { "https" };
        u = format!("{scheme}://{u}");
    }
    if let Some(i) = u.find("/app") {
        u.truncate(i);
    }
    if let Some(i) = u.find('#') {
        u.truncate(i);
    }
    u.trim_end_matches('/').to_string()
}

fn is_local_host(host: &str) -> bool {
    if let Ok(ip) = host.parse::<std::net::Ipv4Addr>() {
        return ip.is_private() || ip.is_loopback() || ip.is_link_local();
    }
    host == "localhost" || host.ends_with(".local") || host.ends_with(".lan") || host.ends_with(".home") || !host.contains('.')
}

/// Covers are stored as `navidrome-cover:<id>`; the login is only added
/// when one is shown, so no token ends up in the saved library.
pub const COVER_SCHEME: &str = "navidrome-cover:";

/// The current login, for turning cover ids into links.
static ACTIVE: Mutex<Option<Conn>> = Mutex::new(None);

/// The link for a `navidrome-cover:` id (None when not connected).
pub fn resolve_cover(uri: &str) -> Option<String> {
    let id = uri.strip_prefix(COVER_SCHEME)?;
    let c = ACTIVE.lock().unwrap().clone()?;
    Some(Navidrome::cover_url(&c, id))
}

/// Old saved links that still carry a token → `navidrome-cover:<id>`.
pub fn strip_cover_token(uri: &str) -> Option<String> {
    if !uri.contains("/rest/getCoverArt.view?") {
        return None;
    }
    let q = uri.split('?').nth(1)?;
    let id = q.split('&').find_map(|kv| kv.strip_prefix("id="))?;
    Some(format!("{COVER_SCHEME}{id}"))
}

pub fn md5_hex(s: &str) -> String {
    Md5::digest(s.as_bytes()).iter().map(|b| format!("{b:02x}")).collect()
}

fn random_salt() -> String {
    let mut b = [0u8; 8];
    let _ = getrandom::getrandom(&mut b);
    b.iter().map(|x| format!("{x:02x}")).collect()
}

/// Decoders Starlite has; anything else is asked for as MP3 (Navidrome transcodes).
const NATIVE: [&str; 9] = ["mp3", "flac", "wav", "ogg", "oga", "m4a", "mp4", "aac", "alac"];

impl Navidrome {
    pub fn load() -> Self {
        let conn: Option<Conn> = std::fs::read_to_string(file()).ok().and_then(|s| serde_json::from_str(&s).ok());
        *ACTIVE.lock().unwrap() = conn.clone();
        Self {
            conn: Mutex::new(conn),
            agent: ureq::AgentBuilder::new().timeout_connect(Duration::from_secs(8)).timeout_read(Duration::from_secs(30)).build(),
        }
    }

    /// (server URL, user name) when connected.
    pub fn connected(&self) -> Option<(String, String)> {
        self.conn.lock().unwrap().as_ref().map(|c| (c.url.clone(), c.user.clone()))
    }

    fn auth(c: &Conn) -> String {
        format!("u={}&t={}&s={}&v={API_VERSION}&c={CLIENT}&f=json", enc(&c.user), c.token, c.salt)
    }

    fn call(&self, c: &Conn, method: &str, extra: &str) -> Result<Value, String> {
        let url = format!("{}/rest/{method}.view?{}{extra}", c.url, Self::auth(c));
        let v: Value = match self.agent.get(&url).call() {
            Ok(r) => r.into_json().map_err(|_| "That doesn't look like a Navidrome server".to_string())?,
            Err(ureq::Error::Status(404, _)) => return Err("No Navidrome server at that address".into()),
            Err(ureq::Error::Status(code, _)) => return Err(format!("The server answered with error {code}")),
            Err(ureq::Error::Transport(t)) => return Err(format!("Can't reach the server ({})", t.kind())),
        };
        let r = &v["subsonic-response"];
        if r.is_null() {
            return Err("That doesn't look like a Navidrome server".into());
        }
        if r["status"].as_str() != Some("ok") {
            let code = r["error"]["code"].as_i64().unwrap_or(0);
            return Err(match code {
                40 | 41 => "Wrong username or password".to_string(),
                _ => r["error"]["message"].as_str().unwrap_or("The server refused the request").to_string(),
            });
        }
        Ok(r.clone())
    }

    fn current(&self) -> Result<Conn, String> {
        self.conn.lock().unwrap().clone().ok_or_else(|| "Not connected to Navidrome".to_string())
    }

    /// Checks the login with the server, then remembers it.
    pub fn connect(&self, url: &str, user: &str, password: &str) -> Result<(), String> {
        if url.trim().is_empty() || user.trim().is_empty() || password.is_empty() {
            return Err("Fill in the server address, username and password".into());
        }
        let salt = random_salt();
        let c = Conn { url: normalize(url), user: user.trim().to_string(), token: md5_hex(&format!("{password}{salt}")), salt };
        self.call(&c, "ping", "")?;
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

    pub fn disconnect(&self) {
        *self.conn.lock().unwrap() = None;
        *ACTIVE.lock().unwrap() = None;
        let _ = std::fs::remove_file(file());
    }

    fn cover_url(c: &Conn, id: &str) -> String {
        format!("{}/rest/getCoverArt.view?id={}&size=400&{}", c.url, enc(id), Self::auth(c))
    }

    /// The user's playlists on the server.
    pub fn playlists(&self) -> Result<Vec<RemotePlaylist>, String> {
        let c = self.current()?;
        let r = self.call(&c, "getPlaylists", "")?;
        let empty = Vec::new();
        Ok(r["playlists"]["playlist"]
            .as_array()
            .unwrap_or(&empty)
            .iter()
            .filter_map(|p| {
                let id = p["id"].as_str()?.to_string();
                Some(RemotePlaylist {
                    title: p["name"].as_str().unwrap_or("Playlist").to_string(),
                    artist: p["owner"].as_str().unwrap_or_default().to_string(),
                    artwork: p["coverArt"].as_str().map(|a| format!("{COVER_SCHEME}{a}")),
                    permalink: None,
                    track_count: p["songCount"].as_u64().unwrap_or(0),
                    id,
                })
            })
            .collect())
    }

    /// Songs of one playlist.
    pub fn playlist_tracks(&self, id: &str) -> Result<Vec<Track>, String> {
        let c = self.current()?;
        let r = self.call(&c, "getPlaylist", &format!("&id={}", enc(id)))?;
        let empty = Vec::new();
        Ok(r["playlist"]["entry"]
            .as_array()
            .unwrap_or(&empty)
            .iter()
            .filter_map(|e| {
                let sid = e["id"].as_str()?.to_string();
                Some(Track {
                    id: format!("navidrome:{sid}"),
                    title: e["title"].as_str().unwrap_or("Unknown").to_string(),
                    artist: e["artist"].as_str().unwrap_or("Unknown artist").to_string(),
                    album: e["album"].as_str().map(String::from),
                    artwork: e["coverArt"].as_str().map(|a| format!("{COVER_SCHEME}{a}")),
                    permalink: None,
                    duration_ms: e["duration"].as_u64().unwrap_or(0) * 1000,
                    source: Source::Navidrome { song_id: sid, suffix: e["suffix"].as_str().unwrap_or("").to_lowercase() },
                })
            })
            .collect())
    }

    /// Where to stream a song from (None when not connected).
    /// A smaller download for a short preview (MP3, 160 kbps).
    pub fn preview_url(&self, song_id: &str) -> Option<String> {
        let c = self.conn.lock().unwrap().clone()?;
        Some(format!("{}/rest/stream.view?id={}&{}&format=mp3&maxBitRate=160", c.url, enc(song_id), Self::auth(&c)))
    }

    pub fn stream_url(&self, song_id: &str, suffix: &str) -> Option<String> {
        let c = self.conn.lock().unwrap().clone()?;
        let transcode = if NATIVE.contains(&suffix) { "" } else { "&format=mp3&maxBitRate=320" };
        Some(format!("{}/rest/stream.view?id={}&{}{transcode}", c.url, enc(song_id), Self::auth(&c)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_urls() {
        assert_eq!(normalize("music.example.com/app/#/album"), "https://music.example.com");
        assert_eq!(normalize("192.168.1.10:4533"), "http://192.168.1.10:4533");
        assert_eq!(normalize("localhost:4533/app/"), "http://localhost:4533");
        assert_eq!(normalize("nas.local:4533"), "http://nas.local:4533");
        assert_eq!(normalize(" https://nd.home:4533/ "), "https://nd.home:4533");
        assert_eq!(normalize("http://10.0.0.5:4533/app/"), "http://10.0.0.5:4533");
    }

    #[test]
    fn saved_covers_carry_no_token() {
        let old = "https://nd.example/rest/getCoverArt.view?id=al-7&size=400&u=me&t=abc123&s=salt&v=1.16.1&c=Starlite&f=json";
        let new = strip_cover_token(old).unwrap();
        assert_eq!(new, "navidrome-cover:al-7");
        assert!(!new.contains("abc123"));
        assert_eq!(strip_cover_token("https://i.ytimg.com/vi/x/hqdefault.jpg"), None);
    }

    #[test]
    fn subsonic_token() {
        // Example from the Subsonic API docs: password "sesame", salt "c19b2d".
        assert_eq!(md5_hex("sesamec19b2d"), "26719a1196d2a940705a59634eb18eab");
    }
}
