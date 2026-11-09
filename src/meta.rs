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
#[derive(Clone, Debug, Default)]
pub struct Line {
    pub at_ms: Option<u64>,
    pub text: String,
    /// When each word is sung (timed lyrics only): from word stamps
    /// ("<mm:ss.xx>", enhanced LRC) when the lyrics have them, otherwise
    /// spread over the line by word length.
    pub words: Vec<Word>,
}

/// A word (or a stamped run of text) in a line.
#[derive(Clone, Debug, PartialEq)]
pub struct Word {
    pub start_ms: u64,
    pub end_ms: u64,
    /// Byte range in the line's text.
    pub range: std::ops::Range<usize>,
}

/// Splits a line with word stamps ("<00:12.50>word") into its text and the
/// stamps' (time, byte offset in that text).
fn word_stamps(rest: &str) -> (String, Vec<(u64, usize)>) {
    let mut out = String::new();
    let mut stamps = Vec::new();
    let mut s = rest;
    while let Some(open) = s.find('<') {
        let Some(len) = s[open..].find('>') else { break };
        match parse_stamp(&s[open + 1..open + len]) {
            Some(ms) => {
                out.push_str(&s[..open]);
                stamps.push((ms, out.len()));
            }
            None => out.push_str(&s[..open + len + 1]),
        }
        s = &s[open + len + 1..];
    }
    out.push_str(s);
    // Trim, keeping the offsets right.
    let lead = out.len() - out.trim_start().len();
    let text = out.trim().to_string();
    let stamps = stamps.into_iter().map(|(ms, at)| (ms, at.saturating_sub(lead).min(text.len()))).collect();
    (text, stamps)
}

/// Word timings for a line from `start` until `next` (the next line).
fn line_words(text: &str, start: u64, next: Option<u64>, stamps: &[(u64, usize)]) -> Vec<Word> {
    // Stamped: each run of text from one stamp to the next.
    if !stamps.is_empty() {
        let mut marks: Vec<(u64, usize)> = Vec::new();
        if stamps[0].1 > 0 {
            marks.push((start, 0));
        }
        marks.extend_from_slice(stamps);
        let mut words = Vec::new();
        for (k, &(ms, at)) in marks.iter().enumerate() {
            let (end_ms, end_at) = marks.get(k + 1).map(|&(m, a)| (m, a)).unwrap_or((next.unwrap_or(ms + 800).max(ms), text.len()));
            let seg = &text[at..end_at];
            let a = at + (seg.len() - seg.trim_start().len());
            let b = at + seg.trim_end().len();
            if a < b {
                words.push(Word { start_ms: ms, end_ms: end_ms.max(ms + 1), range: a..b });
            }
        }
        return words;
    }
    // Unstamped: the singing takes most of the gap to the next line (but
    // not ages, if a pause follows), each word by its length.
    let chars = text.chars().count() as u64;
    if chars == 0 {
        return Vec::new();
    }
    let gap = next.map(|n| n.saturating_sub(start)).unwrap_or(4000);
    let dur = (gap * 9 / 10).min(chars * 95 + 300).max(300);
    let mut spans = Vec::new();
    let mut i = 0;
    for w in text.split_whitespace() {
        let a = text[i..].find(w).map(|o| i + o).unwrap_or(i);
        spans.push(a..a + w.len());
        i = a + w.len();
    }
    let weight = |r: &std::ops::Range<usize>| text[r.clone()].chars().count() as u64 + 1;
    let total: u64 = spans.iter().map(weight).sum::<u64>().max(1);
    let mut t = 0u64;
    spans
        .into_iter()
        .map(|r| {
            let w = weight(&r);
            let word = Word { start_ms: start + dur * t / total, end_ms: start + dur * (t + w) / total, range: r };
            t += w;
            word
        })
        .collect()
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
    let mut timed: Vec<(Line, Vec<(u64, usize)>)> = Vec::new();
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
                plain.push(Line { at_ms: None, text: raw.trim().to_string(), words: Vec::new() });
            }
        } else {
            let (text, word_marks) = word_stamps(rest);
            for ms in stamps {
                timed.push((Line { at_ms: Some(ms), text: text.clone(), words: Vec::new() }, word_marks.clone()));
            }
        }
    }
    if !timed.is_empty() {
        timed.sort_by_key(|l| l.0.at_ms);
        let starts: Vec<u64> = timed.iter().map(|l| l.0.at_ms.unwrap_or(0)).collect();
        let lines = timed
            .into_iter()
            .enumerate()
            .map(|(i, (mut l, marks))| {
                // A repeated line (several stamps) only keeps word stamps for its first time.
                let marks: Vec<(u64, usize)> = if marks.first().is_some_and(|m| m.0 + 1000 < starts[i]) { Vec::new() } else { marks };
                l.words = line_words(&l.text, starts[i], starts.get(i + 1).copied(), &marks);
                l
            })
            .collect();
        return Some(Lyrics { lines, synced: true });
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

#[cfg(test)]
mod tests {
    use super::*;

    fn words(l: &Line) -> Vec<(&str, u64, u64)> {
        l.words.iter().map(|w| (&l.text[w.range.clone()], w.start_ms, w.end_ms)).collect()
    }

    #[test]
    fn word_stamps_are_used() {
        let ly = parse("[00:10.00] <00:10.00>Oh, <00:10.50>when it <00:11.20>all <00:12.00>\n[00:14.00] next").unwrap();
        let l = &ly.lines[0];
        assert_eq!(l.text, "Oh, when it all");
        assert_eq!(words(l), vec![("Oh,", 10_000, 10_500), ("when it", 10_500, 11_200), ("all", 11_200, 12_000)]);
    }

    #[test]
    fn unstamped_words_are_spread_by_length() {
        let ly = parse("[00:01.00] I'm telling you all\n[00:05.00] next").unwrap();
        let w = words(&ly.lines[0]);
        assert_eq!(w.iter().map(|w| w.0).collect::<Vec<_>>(), ["I'm", "telling", "you", "all"]);
        assert_eq!(w[0].1, 1000, "first word starts with the line");
        assert!(w.windows(2).all(|p| p[0].2 == p[1].1), "back to back: {w:?}");
        assert!(w[3].2 <= 4600, "done before the next line: {w:?}");
        assert!(w[1].2 - w[1].1 > w[2].2 - w[2].1, "longer word, longer time");
    }

    #[test]
    fn long_pause_after_a_line_is_not_stretched() {
        let ly = parse("[00:00.00] short line\n[00:30.00] next").unwrap();
        assert!(ly.lines[0].words.last().unwrap().end_ms < 3000);
    }

    #[test]
    fn empty_and_plain_lines_have_no_words() {
        let ly = parse("[00:01.00] \n[00:02.00] la").unwrap();
        assert!(ly.lines[0].words.is_empty());
        assert!(parse("just text").unwrap().lines[0].words.is_empty());
    }
}
