//! Lyrics from lrclib.net (free, no key): timed (LRC) lyrics for the song
//! you're playing, so they can follow along. Only the song's title, artist,
//! album and length are sent. Results are cached on disk (lyrics-cache), so
//! each song is looked up once.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::library::{data_dir, Track};
use crate::meta::{parse, Lyrics};

const API: &str = "https://lrclib.net/api";
/// "Nothing found" is asked again after this long (lrclib keeps growing).
const RETRY_MISS_SECS: u64 = 3 * 24 * 3600;

#[derive(Serialize, Deserialize, Clone)]
#[serde(default)]
pub struct LyricsPrefs {
    /// Look lyrics up on lrclib.net.
    pub online: bool,
}

impl Default for LyricsPrefs {
    fn default() -> Self {
        Self { online: true }
    }
}

impl LyricsPrefs {
    fn file() -> std::path::PathBuf {
        data_dir().join("lyrics.json")
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

/// What lrclib.net had for a song, as cached on disk.
#[derive(Serialize, Deserialize, Default, Clone, Debug, PartialEq)]
#[serde(default)]
struct Found {
    synced: Option<String>,
    plain: Option<String>,
    instrumental: bool,
    /// Unix time of the lookup.
    at: u64,
}

impl Found {
    fn is_empty(&self) -> bool {
        self.synced.is_none() && self.plain.is_none() && !self.instrumental
    }
    fn lyrics(&self) -> Option<Lyrics> {
        self.synced.as_deref().and_then(parse).filter(|l| l.synced).or_else(|| self.plain.as_deref().and_then(parse))
    }
}

/// One lrclib.net record.
#[derive(Deserialize, Default, Clone)]
#[serde(default, rename_all = "camelCase")]
struct Hit {
    duration: Option<f64>,
    instrumental: bool,
    plain_lyrics: Option<String>,
    synced_lyrics: Option<String>,
}

/// What we search for.
#[derive(Debug, PartialEq)]
struct Query {
    artist: String,
    title: String,
    album: Option<String>,
    /// Seconds, when known.
    secs: Option<u64>,
    /// How far (s) the song's length may be off: music videos run longer.
    slack: f64,
}

/// Lyrics for a track from lrclib.net (cached). None: nothing found or offline.
pub fn online(t: &Track) -> Option<Lyrics> {
    let q = query(t)?;
    let file = data_dir().join("lyrics-cache").join(format!("{}.json", cache_key(&q)));
    let now = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    if let Some(f) = std::fs::read_to_string(&file).ok().and_then(|s| serde_json::from_str::<Found>(&s).ok()) {
        if !f.is_empty() || now.saturating_sub(f.at) < RETRY_MISS_SECS {
            return f.lyrics();
        }
    }
    // A network error isn't cached, so the next play tries again.
    let mut found = lookup(&q)?;
    found.at = now;
    if let Some(d) = file.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    if let Ok(s) = serde_json::to_string(&found) {
        let _ = std::fs::write(&file, s);
    }
    found.lyrics()
}

fn cache_key(q: &Query) -> String {
    crate::navidrome::md5_hex(&format!("{}|{}|{}", q.artist.to_lowercase(), q.title.to_lowercase(), q.secs.unwrap_or(0)))
}

fn get(url: &str) -> Result<Option<serde_json::Value>, ()> {
    let ua = concat!("Starlite v", env!("CARGO_PKG_VERSION"));
    match ureq::get(url).timeout(Duration::from_secs(8)).set("User-Agent", ua).call() {
        Ok(r) => r.into_json::<serde_json::Value>().map(Some).map_err(|_| ()),
        Err(ureq::Error::Status(404, _)) => Ok(None),
        Err(_) => Err(()),
    }
}

/// Search first (quick, only lrclib's own catalogue); if that has nothing,
/// the exact lookup (which may also ask other lyrics sources). Err/None on
/// a network error.
fn lookup(q: &Query) -> Option<Found> {
    let search = |params: String| -> Option<Vec<Hit>> {
        let v = get(&format!("{API}/search?{params}")).ok()?;
        Some(v.and_then(|v| serde_json::from_value::<Vec<Hit>>(v).ok()).unwrap_or_default())
    };
    let mut hits = search(format!("track_name={}&artist_name={}", enc(&q.title), enc(&q.artist)))?;
    if pick(&hits, q).is_none() {
        hits.extend(search(format!("q={}", enc(&format!("{} {}", q.artist, q.title))))?);
    }
    if pick(&hits, q).is_none() {
        if let (Some(album), Some(secs)) = (&q.album, q.secs) {
            let url = format!("{API}/get?track_name={}&artist_name={}&album_name={}&duration={secs}", enc(&q.title), enc(&q.artist), enc(album));
            if let Ok(Some(v)) = get(&url) {
                hits.extend(serde_json::from_value::<Hit>(v).ok());
            }
        }
    }
    Some(pick(&hits, q).unwrap_or_default())
}

/// The best record: timed lyrics with a matching length first. Untimed
/// lyrics are fine from any version, timed ones only from the same length
/// (another version's timing would be off).
fn pick(hits: &[Hit], q: &Query) -> Option<Found> {
    let fits = |h: &&Hit| match (q.secs, h.duration) {
        (Some(s), Some(d)) => (s as f64 - d).abs() <= q.slack,
        _ => true,
    };
    let has = |s: &Option<String>| s.as_deref().is_some_and(|t| !t.trim().is_empty());
    if let Some(h) = hits.iter().filter(fits).find(|h| has(&h.synced_lyrics) || h.instrumental) {
        return Some(Found { synced: h.synced_lyrics.clone().filter(|_| !h.instrumental), plain: h.plain_lyrics.clone(), instrumental: h.instrumental, at: 0 });
    }
    hits.iter().find(|h| has(&h.plain_lyrics)).map(|h| Found { plain: h.plain_lyrics.clone(), ..Default::default() })
}

fn enc(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

/// Title and artist as lrclib knows them. YouTube titles are often
/// "Artist - Song (Official Video)" on the artist's channel.
fn query(t: &Track) -> Option<Query> {
    let mut artist = t.artist.trim().to_string();
    let mut title = t.title.trim().to_string();
    let yt = t.youtube_id().is_some();
    if yt {
        for suffix in [" - Topic", "VEVO", " Official"] {
            if let Some(a) = artist.strip_suffix(suffix) {
                artist = a.trim().to_string();
            }
        }
        title = strip_video_words(&title);
        if let Some((a, s)) = title.split_once(" - ") {
            artist = a.trim().to_string();
            title = s.trim().to_string();
        }
    }
    let title = title.trim().to_string();
    if title.is_empty() || artist.is_empty() {
        return None;
    }
    let secs = (t.duration_ms > 0).then(|| (t.duration_ms as f64 / 1000.0).round() as u64);
    Some(Query { artist, title, album: t.album.clone().filter(|a| !a.trim().is_empty() && !yt), secs, slack: if yt { 12.0 } else { 4.0 } })
}

/// Drops "(Official Video)", "[Lyrics]", "| 4K" and the like.
fn strip_video_words(title: &str) -> String {
    const WORDS: [&str; 12] = ["official", "video", "audio", "lyric", "visuali", "music video", "mv", "hd", "hq", "4k", "explicit", "remaster"];
    let mut out = String::new();
    let mut rest = title.split(" | ").next().unwrap_or(title);
    while let Some(open) = rest.find(['(', '[']) {
        let close_ch = if rest[open..].starts_with('(') { ')' } else { ']' };
        let Some(len) = rest[open..].find(close_ch) else { break };
        let inner = rest[open + 1..open + len].to_lowercase();
        let video = inner.split(|c: char| !c.is_alphanumeric() && c != ' ').any(|w| WORDS.iter().any(|k| w.trim() == *k || (k.len() > 3 && w.contains(k))));
        out.push_str(&rest[..open]);
        if !video {
            out.push_str(&rest[open..=open + len]);
        }
        rest = &rest[open + len + 1..];
    }
    out.push_str(rest);
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::library::Source;

    fn track(title: &str, artist: &str, source: Source) -> Track {
        Track { id: "x".into(), title: title.into(), artist: artist.into(), album: Some("The College Dropout".into()), artwork: None, permalink: None, duration_ms: 223_500, source }
    }

    #[test]
    fn youtube_titles_are_cleaned() {
        let t = track("Kanye West - All Falls Down (Official Video) [HD]", "KanyeWestVEVO", Source::YouTube { video_id: "a".into() });
        let q = query(&t).unwrap();
        assert_eq!((q.artist.as_str(), q.title.as_str()), ("Kanye West", "All Falls Down"));
        assert_eq!(q.album, None);
        let t = track("Heartless (Remix) | 4K", "Kanye West - Topic", Source::YouTube { video_id: "a".into() });
        let q = query(&t).unwrap();
        assert_eq!((q.artist.as_str(), q.title.as_str()), ("Kanye West", "Heartless (Remix)"));
    }

    #[test]
    fn files_keep_their_tags() {
        let t = track("All Falls Down", "Kanye West", Source::File { path: "a.mp3".into() });
        let q = query(&t).unwrap();
        assert_eq!(q, Query { artist: "Kanye West".into(), title: "All Falls Down".into(), album: Some("The College Dropout".into()), secs: Some(224), slack: 4.0 });
    }

    #[test]
    fn timed_lyrics_need_the_same_length() {
        let q = Query { artist: "a".into(), title: "b".into(), album: None, secs: Some(164), slack: 4.0 };
        let hit = |d: f64, synced: bool| Hit { duration: Some(d), synced_lyrics: synced.then(|| "[00:01.00] x".into()), plain_lyrics: Some("x".into()), ..Default::default() };
        let mut far = hit(234.0, true);
        far.synced_lyrics = Some("[00:09.00] far".into());
        let hits = [far.clone(), hit(165.0, true)];
        assert_eq!(pick(&hits, &q).unwrap().synced.as_deref(), Some("[00:01.00] x"));
        // Only another length has timed lyrics: use its plain text.
        let f = pick(&[far], &q).unwrap();
        assert_eq!(f.synced, None);
        assert!(f.lyrics().is_some_and(|l| !l.synced));
    }

    #[test]
    fn found_prefers_timed_lyrics() {
        let f = Found { synced: Some("[00:01.00] one\n[00:02.50] two".into()), plain: Some("one\ntwo".into()), ..Default::default() };
        let l = f.lyrics().unwrap();
        assert!(l.synced);
        assert_eq!(l.lines[1].at_ms, Some(2500));
        assert!(Found::default().lyrics().is_none());
    }
}
