//! YouTube through Google's official YouTube Data API v3.
//!
//! Two ways to authorize (Settings → YouTube):
//! * **Sign in with Google** (default): OAuth 2.0 for installed apps. The
//!   browser opens Google's sign-in page, Google redirects back to a one-off
//!   local port, and the code is exchanged with PKCE. Needs the app's OAuth
//!   client (created once by whoever builds Starlite): compile-time
//!   `STARLITE_GOOGLE_CLIENT_ID` / `STARLITE_GOOGLE_CLIENT_SECRET`, or the
//!   client JSON downloaded from Google Cloud saved as
//!   `%APPDATA%\Starlite\google_oauth.json`.
//! * **API key**: a key from Google Cloud pasted into Settings.
//!
//! Playback itself happens in YouTube's official embedded player (see
//! `ytplayer.rs`); this module only fetches charts, playlists and metadata.

use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use base64::Engine;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::library::{data_dir, Source, Track};

const API: &str = "https://www.googleapis.com/youtube/v3";
const AUTH_URL: &str = "https://accounts.google.com/o/oauth2/v2/auth";
const TOKEN_URL: &str = "https://oauth2.googleapis.com/token";
const SCOPE: &str = "https://www.googleapis.com/auth/youtube.readonly";

/// Playlist searches used for "Top Playlists" (one is picked per refresh).
const PLAYLIST_QUERIES: &[&str] = &[
    "top hits", "today's top hits", "hip hop hits", "pop hits", "rap hits", "dance hits", "rnb hits",
    "chill hits", "workout hits", "party hits", "throwback hits", "indie hits", "latin hits", "rock hits",
];

/// A YouTube playlist shown on Explore.
#[derive(Clone, Debug)]
pub struct RemotePlaylist {
    pub id: String,
    pub title: String,
    pub artist: String,
    pub artwork: Option<String>,
    pub permalink: Option<String>,
    pub track_count: u64,
}

/// Minimal xorshift so each refresh shows a different mix.
pub struct Rng(pub u64);
impl Rng {
    pub fn seeded() -> Rng {
        let t = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default();
        Rng(t.as_nanos() as u64 | 1)
    }
    pub fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    pub fn shuffle<T>(&mut self, v: &mut [T]) {
        for i in (1..v.len()).rev() {
            let j = (self.next() % (i as u64 + 1)) as usize;
            v.swap(i, j);
        }
    }
}

/// Channels that publish YouTube Music's official chart playlists.
const OFFICIAL: &[&str] = &["YouTube Music", "YouTube Charts", "Music", "YouTube"];

fn country_name(code: &str) -> &'static str {
    match code {
        "DE" => "Germany", "AT" => "Austria", "CH" => "Switzerland", "US" => "United States", "GB" => "United Kingdom",
        "FR" => "France", "IT" => "Italy", "ES" => "Spain", "NL" => "Netherlands", "BE" => "Belgium", "PL" => "Poland",
        "SE" => "Sweden", "NO" => "Norway", "DK" => "Denmark", "FI" => "Finland", "TR" => "Turkey", "CA" => "Canada",
        "AU" => "Australia", "BR" => "Brazil", "MX" => "Mexico", "IN" => "India", "JP" => "Japan", "KR" => "South Korea",
        _ => "Global",
    }
}

#[derive(Clone, Default, Serialize, Deserialize)]
struct Saved {
    api_key: Option<String>,
    access: Option<String>,
    refresh: Option<String>,
    expires_at: u64,
    account: Option<String>,
}

#[derive(Clone)]
struct OAuthClient {
    id: String,
    secret: String,
}

pub struct YouTube {
    agent: ureq::Agent,
    saved: Mutex<Saved>,
    pub region: String,
    pub cancel_sign_in: AtomicBool,
}

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs()
}

fn saved_file() -> PathBuf {
    data_dir().join("youtube.json")
}

pub fn enc(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

fn random_token(bytes: usize) -> String {
    let mut buf = vec![0u8; bytes];
    let _ = getrandom::getrandom(&mut buf);
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(buf)
}

fn http_err(e: ureq::Error) -> String {
    match e {
        ureq::Error::Status(code, resp) => {
            let body = resp.into_string().unwrap_or_default();
            let v: Value = serde_json::from_str(&body).unwrap_or(Value::Null);
            let reason = v["error"]["errors"][0]["reason"].as_str().unwrap_or("");
            let msg = v["error"]["message"].as_str().or(v["error_description"].as_str()).unwrap_or("");
            match reason {
                "quotaExceeded" | "dailyLimitExceeded" => "YouTube's daily limit for this key is used up. It resets at midnight Pacific time.".into(),
                _ if reason == "keyInvalid" || msg.contains("API key not valid") => "YouTube didn't accept this API key. Check it in Settings.".into(),
                _ if code == 401 => "Your Google sign-in expired. Sign in again in Settings.".into(),
                _ => format!("YouTube returned {code}{}", if msg.is_empty() { String::new() } else { format!(": {msg}") }),
            }
        }
        ureq::Error::Transport(t) => format!("Network error: {t}"),
    }
}

fn pct_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() + 1 {
            if let Some(v) = s.get(i + 1..i + 3).and_then(|h| u8::from_str_radix(h, 16).ok()) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(if b[i] == b'+' { b' ' } else { b[i] });
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// "PT3M21S" → milliseconds.
fn iso_duration_ms(s: &str) -> u64 {
    let mut total = 0u64;
    let mut num = 0u64;
    for c in s.chars() {
        match c {
            '0'..='9' => num = num * 10 + c.to_digit(10).unwrap_or(0) as u64,
            'H' => {
                total += num * 3600;
                num = 0;
            }
            'M' => {
                total += num * 60;
                num = 0;
            }
            'S' => {
                total += num;
                num = 0;
            }
            _ => num = 0,
        }
    }
    total * 1000
}

fn best_thumb(sn: &Value) -> Option<String> {
    let t = &sn["thumbnails"];
    ["high", "medium", "standard", "default"].iter().find_map(|k| t[*k]["url"].as_str().map(String::from))
}

/// Strip label noise like " - Topic" and "VEVO" from channel names.
fn clean_artist(s: &str) -> String {
    let s = s.trim_end_matches(" - Topic").trim();
    let s = s.strip_suffix("VEVO").unwrap_or(s).trim();
    s.to_string()
}

fn video_track(id: &str, sn: &Value, duration: Option<&str>) -> Track {
    Track {
        id: format!("youtube:{id}"),
        title: sn["title"].as_str().unwrap_or("Untitled").to_string(),
        artist: clean_artist(sn["videoOwnerChannelTitle"].as_str().or(sn["channelTitle"].as_str()).unwrap_or("YouTube")),
        album: None,
        artwork: best_thumb(sn),
        permalink: Some(format!("https://www.youtube.com/watch?v={id}")),
        duration_ms: duration.map(iso_duration_ms).unwrap_or(0),
        source: Source::YouTube { video_id: id.to_string() },
    }
}

/// The user's region (e.g. "DE") from the Windows locale, for local charts.
fn windows_region() -> String {
    use windows_sys::Win32::Globalization::GetUserDefaultLocaleName;
    let mut buf = [0u16; 85];
    let n = unsafe { GetUserDefaultLocaleName(buf.as_mut_ptr(), buf.len() as i32) };
    let name = String::from_utf16_lossy(&buf[..(n.max(1) as usize - 1)]);
    name.rsplit('-').next().filter(|r| r.len() == 2 && r.chars().all(|c| c.is_ascii_uppercase())).unwrap_or("US").to_string()
}

impl YouTube {
    pub fn new() -> YouTube {
        let saved = std::fs::read_to_string(saved_file()).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
        YouTube {
            agent: ureq::AgentBuilder::new()
                .timeout_connect(Duration::from_secs(10))
                .timeout_read(Duration::from_secs(30))
                .user_agent("Starlite/0.1")
                .build(),
            saved: Mutex::new(saved),
            region: std::env::var("STARLITE_REGION").unwrap_or_else(|_| windows_region()),
            cancel_sign_in: AtomicBool::new(false),
        }
    }

    fn save(&self) {
        let _ = std::fs::create_dir_all(data_dir());
        if let Ok(s) = serde_json::to_string(&*self.saved.lock().unwrap()) {
            let _ = std::fs::write(saved_file(), s);
        }
    }

    fn oauth_client() -> Option<OAuthClient> {
        if let (Some(id), Some(secret)) = (option_env!("STARLITE_GOOGLE_CLIENT_ID"), option_env!("STARLITE_GOOGLE_CLIENT_SECRET")) {
            return Some(OAuthClient { id: id.into(), secret: secret.into() });
        }
        // The JSON Google Cloud lets you download for a "Desktop app" client.
        let v: Value = serde_json::from_str(&std::fs::read_to_string(data_dir().join("google_oauth.json")).ok()?).ok()?;
        let c = if v["installed"].is_object() { &v["installed"] } else { &v };
        Some(OAuthClient { id: c["client_id"].as_str()?.into(), secret: c["client_secret"].as_str().unwrap_or("").into() })
    }

    /// Whether "Sign in with Google" is available in this build.
    pub fn can_sign_in(&self) -> bool {
        Self::oauth_client().is_some()
    }

    pub fn configured(&self) -> bool {
        let s = self.saved.lock().unwrap();
        s.refresh.is_some() || s.api_key.as_deref().is_some_and(|k| !k.is_empty())
    }

    pub fn account(&self) -> Option<String> {
        let s = self.saved.lock().unwrap();
        s.refresh.as_ref().map(|_| s.account.clone().unwrap_or_else(|| "your Google account".into()))
    }

    pub fn api_key(&self) -> String {
        self.saved.lock().unwrap().api_key.clone().unwrap_or_default()
    }

    pub fn set_api_key(&self, key: &str) {
        self.saved.lock().unwrap().api_key = Some(key.trim().to_string()).filter(|k| !k.is_empty());
        self.save();
    }

    pub fn sign_out(&self) {
        let refresh = {
            let mut s = self.saved.lock().unwrap();
            let r = s.refresh.take();
            s.access = None;
            s.account = None;
            s.expires_at = 0;
            r
        };
        self.save();
        if let Some(r) = refresh {
            let _ = self.agent.post("https://oauth2.googleapis.com/revoke").send_form(&[("token", &r)]);
        }
    }

    /// Full browser sign-in. Blocks until done, failed, cancelled or timed out.
    pub fn sign_in(&self, open: impl Fn(String)) -> Result<String, String> {
        let client = Self::oauth_client().ok_or("Sign in with Google isn't set up in this build")?;
        self.cancel_sign_in.store(false, Ordering::SeqCst);
        let listener = TcpListener::bind("127.0.0.1:0").map_err(|e| e.to_string())?;
        let port = listener.local_addr().map_err(|e| e.to_string())?.port();
        listener.set_nonblocking(true).map_err(|e| e.to_string())?;
        let redirect = format!("http://127.0.0.1:{port}");
        let verifier = random_token(48);
        let challenge = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
        let state = random_token(16);
        open(format!(
            "{AUTH_URL}?client_id={}&redirect_uri={}&response_type=code&scope={}&code_challenge={challenge}&code_challenge_method=S256&access_type=offline&prompt=consent&state={state}",
            enc(&client.id),
            enc(&redirect),
            enc(SCOPE)
        ));

        // Wait for Google to redirect the browser back to us.
        let start = Instant::now();
        let request = loop {
            if self.cancel_sign_in.load(Ordering::SeqCst) {
                return Err("Sign-in cancelled".into());
            }
            if start.elapsed() > Duration::from_secs(300) {
                return Err("Sign-in timed out".into());
            }
            match listener.accept() {
                Ok((mut stream, _)) => {
                    let _ = stream.set_nonblocking(false);
                    let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
                    let mut buf = [0u8; 4096];
                    let n = stream.read(&mut buf).unwrap_or(0);
                    let req = String::from_utf8_lossy(&buf[..n]).to_string();
                    let ok = req.contains("code=");
                    let page = if ok {
                        "<h2 style='font-family:Segoe UI'>Signed in. You can close this tab and go back to Starlite.</h2>"
                    } else {
                        "<h2 style='font-family:Segoe UI'>Sign-in didn't finish. Go back to Starlite and try again.</h2>"
                    };
                    let _ = write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{page}", page.len());
                    if req.starts_with("GET /?") {
                        break req;
                    }
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => std::thread::sleep(Duration::from_millis(100)),
                Err(e) => return Err(e.to_string()),
            }
        };
        let query = request.split_whitespace().nth(1).unwrap_or("").trim_start_matches("/?").to_string();
        let param = |k: &str| query.split('&').find_map(|kv| kv.strip_prefix(&format!("{k}=")).map(String::from));
        if param("state").as_deref() != Some(state.as_str()) {
            return Err("Sign-in failed (state mismatch). Try again.".into());
        }
        if let Some(e) = param("error") {
            return Err(if e == "access_denied" { "Sign-in was cancelled in the browser".into() } else { format!("Google sign-in error: {e}") });
        }
        let code = param("code").ok_or("Google sent no sign-in code")?;
        let code = pct_decode(&code);
        let v: Value = self
            .agent
            .post(TOKEN_URL)
            .send_form(&[
                ("client_id", &client.id),
                ("client_secret", &client.secret),
                ("code", &code),
                ("code_verifier", &verifier),
                ("grant_type", "authorization_code"),
                ("redirect_uri", &redirect),
            ])
            .map_err(http_err)?
            .into_json()
            .map_err(|e| e.to_string())?;
        {
            let mut s = self.saved.lock().unwrap();
            s.access = v["access_token"].as_str().map(String::from);
            s.refresh = v["refresh_token"].as_str().map(String::from).or(s.refresh.take());
            s.expires_at = now() + v["expires_in"].as_u64().unwrap_or(3600);
        }
        self.save();
        // Show which account is connected.
        let name = self
            .get_json("channels?part=snippet&mine=true")
            .ok()
            .and_then(|c| c["items"][0]["snippet"]["title"].as_str().map(String::from))
            .unwrap_or_else(|| "your Google account".into());
        self.saved.lock().unwrap().account = Some(name.clone());
        self.save();
        Ok(name)
    }

    fn access_token(&self) -> Result<Option<String>, String> {
        let (access, refresh, exp) = {
            let s = self.saved.lock().unwrap();
            (s.access.clone(), s.refresh.clone(), s.expires_at)
        };
        let Some(refresh) = refresh else { return Ok(None) };
        if let Some(a) = access.filter(|_| exp > now() + 60) {
            return Ok(Some(a));
        }
        let client = Self::oauth_client().ok_or("Sign in with Google isn't set up in this build")?;
        let v: Value = self
            .agent
            .post(TOKEN_URL)
            .send_form(&[("client_id", &client.id), ("client_secret", &client.secret), ("refresh_token", &refresh), ("grant_type", "refresh_token")])
            .map_err(http_err)?
            .into_json()
            .map_err(|e| e.to_string())?;
        let a = v["access_token"].as_str().ok_or("Google sent no access token")?.to_string();
        {
            let mut s = self.saved.lock().unwrap();
            s.access = Some(a.clone());
            s.expires_at = now() + v["expires_in"].as_u64().unwrap_or(3600);
        }
        self.save();
        Ok(Some(a))
    }

    fn get_json(&self, path: &str) -> Result<Value, String> {
        let url = format!("{API}/{path}");
        let req = if let Some(tok) = self.access_token()? {
            self.agent.get(&url).set("Authorization", &format!("Bearer {tok}"))
        } else {
            let key = self.api_key();
            if key.is_empty() {
                return Err("Sign in with Google or add a YouTube API key in Settings".into());
            }
            self.agent.get(&format!("{url}&key={}", enc(&key)))
        };
        req.call().map_err(http_err)?.into_json().map_err(|e| e.to_string())
    }

    /// Cheap call that checks an API key works (1 quota unit).
    pub fn test(&self) -> Result<(), String> {
        self.get_json(&format!("videoCategories?part=id&regionCode={}", self.region)).map(|_| ())
    }

    /// The official most-popular music videos in the user's region (1 unit).
    pub fn charts(&self) -> Result<Vec<Track>, String> {
        let v = self.get_json(&format!(
            "videos?part=snippet,contentDetails&chart=mostPopular&videoCategoryId=10&regionCode={}&maxResults=50",
            self.region
        ))?;
        Ok(v["items"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|it| Some(video_track(it["id"].as_str()?, &it["snippet"], it["contentDetails"]["duration"].as_str())))
                    .collect()
            })
            .unwrap_or_default())
    }

    fn search_playlists(&self, q: &str) -> Result<Vec<(RemotePlaylist, bool)>, String> {
        let v = self.get_json(&format!("search?part=snippet&type=playlist&maxResults=25&regionCode={}&q={}", self.region, enc(q)))?;
        Ok(v["items"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|it| {
                        let id = it["id"]["playlistId"].as_str()?;
                        let sn = &it["snippet"];
                        let channel = sn["channelTitle"].as_str().unwrap_or("");
                        let official = OFFICIAL.contains(&channel);
                        Some((
                            RemotePlaylist {
                                id: id.to_string(),
                                title: sn["title"].as_str().unwrap_or("Playlist").to_string(),
                                artist: if official { "YouTube Music".into() } else { clean_artist(channel) },
                                artwork: best_thumb(sn),
                                permalink: Some(format!("https://music.youtube.com/playlist?list={id}")),
                                track_count: 0,
                            },
                            official,
                        ))
                    })
                    .collect()
            })
            .unwrap_or_default())
    }

    /// YouTube Music's chart playlists (Top 100 Songs / Music Videos, your
    /// country + global) first, then a random hits mix. ~300 quota units.
    pub fn chart_playlists(&self, rng: &mut Rng) -> Result<Vec<RemotePlaylist>, String> {
        let country = country_name(&self.region);
        let mut official: Vec<RemotePlaylist> = Vec::new();
        let mut other: Vec<RemotePlaylist> = Vec::new();
        let hits = PLAYLIST_QUERIES[(rng.next() % PLAYLIST_QUERIES.len() as u64) as usize];
        let queries = [format!("Top 100 Songs {country}"), "Top 100 Music Videos Global".to_string(), hits.to_string()];
        for q in &queries {
            for (pl, is_official) in self.search_playlists(q)? {
                if official.iter().chain(other.iter()).any(|x| x.id == pl.id) {
                    continue;
                }
                if is_official && pl.title.to_lowercase().contains("top") {
                    official.push(pl);
                } else {
                    other.push(pl);
                }
            }
        }
        rng.shuffle(&mut official);
        rng.shuffle(&mut other);
        let mut all = official;
        all.extend(other);
        all.truncate(20);
        // Song counts for the cards (1 unit).
        let ids: Vec<&str> = all.iter().map(|p| p.id.as_str()).collect();
        if !ids.is_empty() {
            if let Ok(v) = self.get_json(&format!("playlists?part=contentDetails&maxResults=50&id={}", ids.join(","))) {
                for it in v["items"].as_array().cloned().unwrap_or_default() {
                    if let (Some(id), Some(n)) = (it["id"].as_str(), it["contentDetails"]["itemCount"].as_u64()) {
                        if let Some(p) = all.iter_mut().find(|p| p.id == id) {
                            p.track_count = n;
                        }
                    }
                }
            }
        }
        Ok(all)
    }

    /// Charts + chart playlists, shuffled so every refresh looks different.
    pub fn explore(&self) -> Result<(Vec<Track>, Vec<RemotePlaylist>), String> {
        let mut rng = Rng::seeded();
        let mut charts = self.charts()?;
        rng.shuffle(&mut charts);
        let pls = self.chart_playlists(&mut rng).unwrap_or_default();
        Ok((charts, pls))
    }

    /// Up to 100 songs of a playlist, with durations (2-3 units).
    pub fn playlist_tracks(&self, playlist_id: &str) -> Result<Vec<Track>, String> {
        let mut out: Vec<(String, Value)> = Vec::new();
        let mut page = String::new();
        for _ in 0..2 {
            let v = self.get_json(&format!("playlistItems?part=snippet&maxResults=50&playlistId={}{}", enc(playlist_id), page))?;
            for it in v["items"].as_array().cloned().unwrap_or_default() {
                let sn = &it["snippet"];
                if let Some(id) = sn["resourceId"]["videoId"].as_str() {
                    let title = sn["title"].as_str().unwrap_or("");
                    if title != "Private video" && title != "Deleted video" {
                        out.push((id.to_string(), sn.clone()));
                    }
                }
            }
            match v["nextPageToken"].as_str() {
                Some(t) => page = format!("&pageToken={t}"),
                None => break,
            }
        }
        // One extra call per 50 songs for their durations.
        let mut durations = std::collections::HashMap::new();
        for chunk in out.chunks(50) {
            let ids: Vec<&str> = chunk.iter().map(|(id, _)| id.as_str()).collect();
            if let Ok(v) = self.get_json(&format!("videos?part=contentDetails&id={}", ids.join(","))) {
                for it in v["items"].as_array().cloned().unwrap_or_default() {
                    if let (Some(id), Some(d)) = (it["id"].as_str(), it["contentDetails"]["duration"].as_str()) {
                        durations.insert(id.to_string(), d.to_string());
                    }
                }
            }
        }
        Ok(out.iter().map(|(id, sn)| video_track(id, sn, durations.get(id).map(String::as_str))).collect())
    }
}
