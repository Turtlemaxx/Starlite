//! Real song metadata from audio files (ID3v2 for MP3, Vorbis comments for
//! FLAC/OGG, RIFF/ID3 for WAV) via lofty: title, artist, album, duration,
//! the embedded cover picture and lyrics.
//!
//! Lyrics come from the file's lyrics tag, or from a `.lrc` file with the same
//! name next to the song. LRC timestamps ("[01:23.45] line") make them synced.

use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};

use lofty::picture::{MimeType, PictureType};
use lofty::prelude::*;

use crate::library::data_dir;

#[derive(Default, Clone)]
pub struct Meta {
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub duration_ms: u64,
    /// Embedded cover, cached as an image file.
    pub cover: Option<PathBuf>,
}

fn nonempty(s: Option<std::borrow::Cow<'_, str>>) -> Option<String> {
    s.map(|v| v.trim().to_string()).filter(|v| !v.is_empty())
}

/// Read tags + cover. Missing fields stay `None`.
pub fn read(path: &Path) -> Meta {
    let Ok(file) = lofty::read_from_path(path) else { return Meta::default() };
    let mut m = Meta { duration_ms: file.properties().duration().as_millis() as u64, ..Meta::default() };
    let Some(tag) = file.primary_tag().or_else(|| file.first_tag()) else { return m };
    m.title = nonempty(tag.title());
    m.artist = nonempty(tag.artist()).or_else(|| tag.get_string(ItemKey::AlbumArtist).map(|s| s.trim().to_string()).filter(|s| !s.is_empty()));
    m.album = nonempty(tag.album());

    let pics = tag.pictures();
    let pic = pics.iter().find(|p| p.pic_type() == PictureType::CoverFront).or_else(|| pics.first());
    if let Some(pic) = pic {
        let ext = match pic.mime_type() {
            Some(MimeType::Png) => "png",
            _ => "jpg",
        };
        let data = pic.data();
        let mut h = std::collections::hash_map::DefaultHasher::new();
        data.len().hash(&mut h);
        data[..data.len().min(4096)].hash(&mut h);
        let dir = data_dir().join("cover-cache");
        let out = dir.join(format!("{:016x}.{ext}", h.finish()));
        if out.exists() || (std::fs::create_dir_all(&dir).is_ok() && std::fs::write(&out, data).is_ok()) {
            m.cover = Some(out);
        }
    }
    m
}

/// One lyrics line; `at_ms` is set for synced (LRC) lyrics.
#[derive(Clone, Debug)]
pub struct Line {
    pub at_ms: Option<u64>,
    pub text: String,
}

#[derive(Clone, Debug, Default)]
pub struct Lyrics {
    pub lines: Vec<Line>,
    pub synced: bool,
}

/// Lyrics for a song file: `<name>.lrc` next to it wins, then the tags.
pub fn lyrics(path: &Path) -> Option<Lyrics> {
    let lrc = path.with_extension("lrc");
    if let Ok(text) = std::fs::read_to_string(&lrc) {
        if let Some(l) = parse(&text) {
            return Some(l);
        }
    }
    let file = lofty::read_from_path(path).ok()?;
    for tag in file.tags() {
        for key in [ItemKey::Lyrics, ItemKey::UnsyncLyrics] {
            if let Some(text) = tag.get_string(key) {
                if let Some(l) = parse(text) {
                    return Some(l);
                }
            }
        }
    }
    None
}

/// Parse LRC (timestamped) or plain lyrics text.
pub fn parse(text: &str) -> Option<Lyrics> {
    let mut timed: Vec<Line> = Vec::new();
    let mut plain: Vec<Line> = Vec::new();
    for raw in text.lines() {
        let mut rest = raw.trim();
        let mut stamps = Vec::new();
        // One line can carry several "[mm:ss.xx]" stamps.
        while let Some(inner) = rest.strip_prefix('[') {
            let Some(end) = inner.find(']') else { break };
            let tag = &inner[..end];
            rest = inner[end + 1..].trim_start();
            if let Some(ms) = parse_stamp(tag) {
                stamps.push(ms);
            } else {
                // [ar:Artist], [ti:Title] etc. are metadata, not lyrics.
                stamps.clear();
                rest = "";
                break;
            }
        }
        if stamps.is_empty() {
            if !raw.trim().starts_with('[') {
                plain.push(Line { at_ms: None, text: raw.trim().to_string() });
            }
        } else {
            for ms in stamps {
                timed.push(Line { at_ms: Some(ms), text: rest.to_string() });
            }
        }
    }
    if !timed.is_empty() {
        timed.sort_by_key(|l| l.at_ms);
        return Some(Lyrics { lines: timed, synced: true });
    }
    // Trim blank lines at the ends of plain lyrics.
    while plain.first().is_some_and(|l| l.text.is_empty()) {
        plain.remove(0);
    }
    while plain.last().is_some_and(|l| l.text.is_empty()) {
        plain.pop();
    }
    (!plain.is_empty()).then_some(Lyrics { lines: plain, synced: false })
}

/// "mm:ss", "mm:ss.xx" or "mm:ss:xx" → milliseconds.
fn parse_stamp(s: &str) -> Option<u64> {
    let (m, rest) = s.split_once(':')?;
    let m: u64 = m.trim().parse().ok()?;
    let (sec, frac) = match rest.split_once(['.', ':']) {
        Some((a, b)) => (a, b),
        None => (rest, ""),
    };
    let sec: u64 = sec.trim().parse().ok()?;
    let frac_ms = if frac.is_empty() {
        0
    } else {
        let digits: String = frac.chars().take(3).collect();
        let v: u64 = digits.parse().ok()?;
        match digits.len() {
            1 => v * 100,
            2 => v * 10,
            _ => v,
        }
    };
    Some(m * 60_000 + sec * 1000 + frac_ms)
}
