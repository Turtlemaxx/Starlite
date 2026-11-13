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
    track_name: String,
    artist_name: String,
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
    // "v2": looked up with the looser matching (older misses are asked again).
    crate::navidrome::md5_hex(&format!("v2|{}|{}|{}", q.artist.to_lowercase(), q.title.to_lowercase(), q.secs.unwrap_or(0)))
}

/// Words that only say which version / release a song is.
const VERSION_WORDS: [&str; 21] = [
    "remaster", "explicit", "clean", "version", "edit", "mono", "stereo", "deluxe", "bonus", "feat", "ft.", "featuring", "with ", "prod", "mix",
    "live", "single", "album", "radio", "bpm", "sped up",
];

fn is_version_note(s: &str) -> bool {
    let l = s.to_lowercase();
    let l = l.trim();
    VERSION_WORDS.iter().any(|w| l.contains(w) || l.starts_with("ft ")) || (l.len() == 4 && l.chars().all(|c| c.is_ascii_digit()))
}

/// "Heartless - 2011 Remaster", "Song (feat. X) [Explicit]" → "Heartless", "Song".
fn strip_versions(title: &str) -> String {
    let mut out = String::new();
    let mut rest = title;
    while let Some(open) = rest.find(['(', '[']) {
        let close = if rest[open..].starts_with('(') { ')' } else { ']' };
        let Some(len) = rest[open..].find(close) else { break };
        out.push_str(&rest[..open]);
        if !is_version_note(&rest[open + 1..open + len]) {
            out.push_str(&rest[open..=open + len]);
        }
        rest = &rest[open + len + 1..];
    }
    out.push_str(rest);
    // A version after " - ", and a trailing "feat. X" outside brackets.
    if let Some((a, b)) = out.rsplit_once(" - ") {
        if is_version_note(b) {
            out = a.to_string();
        }
    }
    let lower = out.to_lowercase();
    if let Some(i) = [" feat. ", " feat ", " ft. ", " featuring "].iter().filter_map(|m| lower.find(m)).min() {
        out.truncate(i);
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The first artist of "A, B", "A feat. B", "A & B", "A; B", "A x B", "A / B".
fn primary_artist(artist: &str) -> String {
    let lower = artist.to_lowercase();
    let cut = [",", ";", " & ", " feat", " ft.", " ft ", " x ", " with ", "/", " • ", " and "].iter().filter_map(|m| lower.find(m)).filter(|&i| i > 0).min();
    artist[..cut.unwrap_or(artist.len())].trim().to_string()
}

/// Lowercase letters and digits only ("&" counts as "and").
fn norm(s: &str) -> String {
    s.to_lowercase().replace('&', "and").chars().filter(|c| c.is_alphanumeric()).collect()
}

/// A title without anything in brackets or version notes, normalized.
fn core(title: &str) -> String {
    let t = strip_versions(title);
    let mut out = String::new();
    let mut depth = 0;
    for c in t.chars() {
        match c {
            '(' | '[' => depth += 1,
            ')' | ']' => depth = (depth - 1).max(0),
            _ if depth == 0 => out.push(c),
            _ => {}
        }
    }
    let n = norm(&out);
    if n.is_empty() {
        norm(&t)
    } else {
        n
    }
}

/// Is this record the same song? lrclib's search is fuzzy, so check title
/// and artist (records are sometimes titled "Artist - Song").
fn same_song(h: &Hit, q: &Query) -> bool {
    let (ot, ht) = (core(&q.title), core(&h.track_name));
    let title = !ot.is_empty() && (ht.contains(&ot) || (ht.len() >= 3 && ot.contains(&ht)));
    let (oa, op, ha, hp) = (norm(&q.artist), norm(&primary_artist(&q.artist)), norm(&h.artist_name), norm(&primary_artist(&h.artist_name)));
    let artist = op.is_empty() || ha.contains(&op) || (!hp.is_empty() && oa.contains(&hp));
    title && artist
}

fn get(url: &str) -> Result<Option<serde_json::Value>, ()> {
    let ua = concat!("Starlite v", env!("CARGO_PKG_VERSION"));
    match ureq::get(url).timeout(Duration::from_secs(8)).set("User-Agent", ua).call() {
        Ok(r) => r.into_json::<serde_json::Value>().map(Some).map_err(|_| ()),
        Err(ureq::Error::Status(404, _)) => Ok(None),
        Err(_) => Err(()),
    }
}

/// The searches to try, most exact first: as tagged, without version notes
/// ("- 2011 Remaster", "[Explicit]", "feat. X"), with only the first artist,
/// then a free-text search. Duplicates are skipped.
fn searches(q: &Query) -> Vec<String> {
    let (title, clean, core_title) = (q.title.clone(), strip_versions(&q.title), {
        let s = strip_versions(&q.title);
        s.split(['(', '[']).next().unwrap_or(&s).trim().to_string()
    });
    let (artist, first) = (q.artist.clone(), primary_artist(&q.artist));
    let mut out: Vec<String> = Vec::new();
    for (t, a) in [(&clean, &artist), (&clean, &first), (&title, &artist), (&core_title, &first)] {
        if t.is_empty() || a.is_empty() {
            continue;
        }
        let s = format!("track_name={}&artist_name={}", enc(t), enc(a));
        if !out.contains(&s) {
            out.push(s);
        }
    }
    out.push(format!("q={}", enc(&format!("{first} {core_title}"))));
    out
}

/// Searches lrclib's catalogue until a record of this song turns up; then
/// the exact lookup (which may also ask other lyrics sources). None on a
/// network error.
fn lookup(q: &Query) -> Option<Found> {
    let mut hits: Vec<Hit> = Vec::new();
    for params in searches(q) {
        let v = get(&format!("{API}/search?{params}")).ok()?;
        hits.extend(v.and_then(|v| serde_json::from_value::<Vec<Hit>>(v).ok()).unwrap_or_default());
        if let Some(f) = pick(&hits, q).filter(|f| f.synced.is_some() || f.instrumental) {
            return Some(f);
        }
    }
    if let (Some(album), Some(secs)) = (&q.album, q.secs) {
        let url = format!("{API}/get?track_name={}&artist_name={}&album_name={}&duration={secs}", enc(&strip_versions(&q.title)), enc(&q.artist), enc(album));
        if let Ok(Some(v)) = get(&url) {
            hits.extend(serde_json::from_value::<Hit>(v).ok());
        }
    }
    Some(pick(&hits, q).unwrap_or_default())
}

/// The best record of this song: timed lyrics with the closest length
/// first. Untimed lyrics are fine from any version, timed ones only from
/// about the same length (another version's timing would be off).
fn pick(hits: &[Hit], q: &Query) -> Option<Found> {
    let off = |h: &Hit| match (q.secs, h.duration) {
        (Some(s), Some(d)) => (s as f64 - d).abs(),
        _ => 0.0,
    };
    let has = |s: &Option<String>| s.as_deref().is_some_and(|t| !t.trim().is_empty());
    let same: Vec<&Hit> = hits.iter().filter(|h| same_song(h, q)).collect();
    let timed = same.iter().filter(|h| off(h) <= q.slack && (has(&h.synced_lyrics) || h.instrumental)).min_by(|a, b| off(a).total_cmp(&off(b)));
    if let Some(h) = timed {
        return Some(Found { synced: h.synced_lyrics.clone().filter(|_| !h.instrumental), plain: h.plain_lyrics.clone(), instrumental: h.instrumental, at: 0 });
    }
    same.iter().find(|h| has(&h.plain_lyrics)).map(|h| Found { plain: h.plain_lyrics.clone(), ..Default::default() })
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
/// A song's artist and title as a catalogue knows them: YouTube videos
/// lose the channel's "VEVO" / "- Topic" and the title's "(Official
/// Video)", and "Artist - Song" titles are split.
pub fn clean_names(t: &Track) -> Option<(String, String)> {
    query(t).map(|q| (q.artist, q.title))
}

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
    Some(Query { artist, title, album: t.album.clone().filter(|a| !a.trim().is_empty() && !yt), secs, slack: if yt { 12.0 } else { 5.0 } })
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
        assert_eq!(q, Query { artist: "Kanye West".into(), title: "All Falls Down".into(), album: Some("The College Dropout".into()), secs: Some(224), slack: 5.0 });
    }

    #[test]
    fn timed_lyrics_need_the_same_length() {
        let q = Query { artist: "Kanye West".into(), title: "Coldest Winter".into(), album: None, secs: Some(164), slack: 4.0 };
        let hit = |d: f64, lrc: &str| Hit { track_name: "Coldest Winter".into(), artist_name: "Kanye West".into(), duration: Some(d), synced_lyrics: Some(lrc.into()), plain_lyrics: Some("x".into()), ..Default::default() };
        let far = hit(234.0, "[00:09.00] far");
        let hits = [far.clone(), hit(167.0, "[00:01.00] near"), hit(165.0, "[00:01.00] nearest")];
        assert_eq!(pick(&hits, &q).unwrap().synced.as_deref(), Some("[00:01.00] nearest"));
        // Only another length has timed lyrics: use its plain text.
        let f = pick(&[far], &q).unwrap();
        assert_eq!(f.synced, None);
        assert!(f.lyrics().is_some_and(|l| !l.synced));
    }

    #[test]
    fn other_songs_from_the_fuzzy_search_are_skipped() {
        let q = Query { artist: "Kanye West".into(), title: "Heartless".into(), album: None, secs: Some(211), slack: 5.0 };
        let rec = |t: &str, a: &str| Hit { track_name: t.into(), artist_name: a.into(), duration: Some(211.0), synced_lyrics: Some("[00:01.00] x".into()), ..Default::default() };
        assert!(!same_song(&rec("Heartless", "The Weeknd"), &q));
        assert!(!same_song(&rec("Stronger", "Kanye West"), &q));
        assert!(same_song(&rec("Heartless", "Kanye West"), &q));
        assert!(same_song(&rec("Kanye West - Heartless", "Kanye West"), &q));
        assert!(same_song(&rec("Heartless (Explicit)", "Kanye West, Kid Cudi"), &q));
        let q = Query { artist: "JAY-Z, Kanye West".into(), title: "N****s In Paris".into(), album: None, secs: None, slack: 5.0 };
        assert!(same_song(&rec("N****s in Paris", "Kanye West, Jay-Z"), &q));
    }

    #[test]
    fn version_notes_are_dropped_for_searching() {
        assert_eq!(strip_versions("Heartless - 2011 Remaster"), "Heartless");
        assert_eq!(strip_versions("Heartless (Remastered) [Explicit]"), "Heartless");
        assert_eq!(strip_versions("All Falls Down (feat. Syleena Johnson)"), "All Falls Down");
        assert_eq!(strip_versions("Runaway feat. Pusha T"), "Runaway");
        assert_eq!(strip_versions("Song - Radio Edit"), "Song");
        assert_eq!(strip_versions("Love Lockdown (Interlude)"), "Love Lockdown (Interlude)");
        assert_eq!(strip_versions("Kanye West - All Falls Down"), "Kanye West - All Falls Down");
        assert_eq!(primary_artist("Kanye West, Syleena Johnson"), "Kanye West");
        assert_eq!(primary_artist("Kanye West feat. Jay-Z"), "Kanye West");
        assert_eq!(primary_artist("Simon & Garfunkel"), "Simon");
        let s = searches(&Query { artist: "Kanye West; Kid Cudi".into(), title: "Heartless - 2011 Remaster".into(), album: None, secs: None, slack: 5.0 });
        assert_eq!(s[0], "track_name=Heartless&artist_name=Kanye%20West%3B%20Kid%20Cudi");
        assert_eq!(s[1], "track_name=Heartless&artist_name=Kanye%20West");
    }

    /// Live check against lrclib.net (not run by default):
    /// STARLITE_LYRICS_CHECK="artist|title|secs~..." cargo test -- --ignored live
    #[test]
    #[ignore]
    fn live_lookups() {
        let list = std::env::var("STARLITE_LYRICS_CHECK").unwrap_or_default();
        for item in list.split('~').filter(|s| !s.is_empty()) {
            let f: Vec<&str> = item.split('|').collect();
            let q = Query { artist: f[0].into(), title: f[1].into(), album: None, secs: f.get(2).and_then(|s| s.parse().ok()), slack: 5.0 };
            let found = lookup(&q).map(|f| if f.synced.is_some() { "timed" } else if f.plain.is_some() { "plain" } else if f.instrumental { "instrumental" } else { "MISS" }).unwrap_or("network error");
            println!("{found:>13}  {} - {}", q.artist, q.title);
        }
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
