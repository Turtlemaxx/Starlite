//! Loading a playlist from an .m3u / .m3u8 file (New playlist → Load .m3u).
//!
//! Each entry is a path (absolute, relative to the .m3u's folder, or a
//! file:// link). A file that isn't where the list says is looked for among
//! the downloaded songs, by file name and then by its "Artist - Title".
//! Web links and missing songs are skipped (and counted).

use std::path::{Path, PathBuf};

use crate::library::{file_track, is_audio, Track};

pub struct Imported {
    /// The list's name (its #PLAYLIST line, else the file name).
    pub name: String,
    pub tracks: Vec<Track>,
    /// Entries that couldn't be found.
    pub missing: usize,
}

/// An entry as written in the file, with its #EXTINF title when given.
struct Entry {
    location: String,
    info: Option<String>,
}

fn decode(bytes: &[u8]) -> String {
    let bytes = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(bytes);
    match std::str::from_utf8(bytes) {
        Ok(s) => s.to_string(),
        // Old .m3u files are often Windows-1252 / Latin-1.
        Err(_) => bytes.iter().map(|&b| b as char).collect(),
    }
}

fn parse(text: &str) -> (Option<String>, Vec<Entry>) {
    let mut name = None;
    let mut info = None;
    let mut out = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if let Some(rest) = line.strip_prefix('#') {
            if let Some(v) = rest.strip_prefix("EXTINF:") {
                // "#EXTINF:215,Artist - Title"
                info = v.split_once(',').map(|(_, t)| t.trim().to_string()).filter(|t| !t.is_empty());
            } else if let Some(v) = rest.strip_prefix("PLAYLIST:") {
                name = Some(v.trim().to_string()).filter(|n| !n.is_empty());
            }
            continue;
        }
        out.push(Entry { location: line.to_string(), info: info.take() });
    }
    (name, out)
}

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            let hex = |c: u8| (c as char).to_digit(16);
            if let (Some(h), Some(l)) = (hex(b[i + 1]), hex(b[i + 2])) {
                out.push((h * 16 + l) as u8);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Where an entry points on disk (None for web links).
fn local_path(loc: &str, base: &Path) -> Option<PathBuf> {
    let lower = loc.to_ascii_lowercase();
    if lower.starts_with("http://") || lower.starts_with("https://") {
        return None;
    }
    let p = if lower.starts_with("file://") {
        let rest = percent_decode(&loc[7..]);
        // file:///C:/Music/a.mp3 → C:/Music/a.mp3
        let rest = rest.strip_prefix('/').filter(|r| r.as_bytes().get(1) == Some(&b':')).map(str::to_string).unwrap_or(rest);
        PathBuf::from(rest)
    } else {
        PathBuf::from(loc)
    };
    Some(if p.is_absolute() { p } else { base.join(p) })
}

fn simple(s: &str) -> String {
    s.to_lowercase().chars().filter(|c| c.is_alphanumeric()).collect()
}

/// Reads an .m3u file. `known`: songs to look in when a path doesn't exist.
pub fn import(path: &Path, known: &[Track]) -> Result<Imported, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("Couldn't read that file ({e})"))?;
    let (name, entries) = parse(&decode(&bytes));
    if entries.is_empty() {
        return Err("That file has no songs in it".into());
    }
    let base = path.parent().unwrap_or(Path::new("."));
    let mut tracks: Vec<Track> = Vec::new();
    let mut missing = 0;
    for e in entries {
        let p = local_path(&e.location, base);
        let found = match &p {
            Some(p) if p.is_file() && is_audio(p) => Some(file_track(p)),
            _ => {
                // Not there: the same file name among known songs, then "Artist - Title".
                let file_name = p.as_ref().and_then(|p| p.file_name()).map(|n| n.to_string_lossy().to_lowercase());
                let by_name = file_name.and_then(|n| {
                    known.iter().find(|t| match &t.source {
                        crate::library::Source::File { path } => path.file_name().is_some_and(|f| f.to_string_lossy().to_lowercase() == n),
                        _ => false,
                    })
                });
                let by_info = || {
                    let info = simple(e.info.as_deref()?);
                    known.iter().find(|t| !info.is_empty() && (simple(&format!("{}{}", t.artist, t.title)) == info || simple(&t.title) == info))
                };
                by_name.or_else(by_info).cloned()
            }
        };
        match found {
            Some(t) if !tracks.iter().any(|x| x.id == t.id) => tracks.push(t),
            Some(_) => {}
            None => missing += 1,
        }
    }
    let name = name.unwrap_or_else(|| path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "Playlist".into()));
    Ok(Imported { name, tracks, missing })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_paths_titles_and_links() {
        let (name, e) = parse("#EXTM3U\n#PLAYLIST:Road trip\n#EXTINF:215,Artist - Song\nmusic/a.mp3\n\n# comment\nhttps://example.com/x.mp3\nfile:///C:/Music/My%20Song.flac\n");
        assert_eq!(name.as_deref(), Some("Road trip"));
        assert_eq!(e.len(), 3);
        assert_eq!(e[0].info.as_deref(), Some("Artist - Song"));
        assert!(e[1].info.is_none());
        let base = Path::new(r"D:\lists");
        assert_eq!(local_path(&e[0].location, base), Some(PathBuf::from(r"D:\lists\music/a.mp3")));
        assert_eq!(local_path(&e[1].location, base), None);
        assert_eq!(local_path(&e[2].location, base), Some(PathBuf::from("C:/Music/My Song.flac")));
    }

    #[test]
    fn imports_what_exists_and_counts_the_rest() {
        let dir = std::env::temp_dir().join(format!("starlite-m3u-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        std::fs::write(dir.join("sub").join("one.mp3"), b"not really audio").unwrap();
        std::fs::write(dir.join("list.m3u8"), "#EXTM3U\nsub/one.mp3\nsub/one.mp3\nmissing.mp3\nhttp://x/y.mp3\n").unwrap();
        let r = import(&dir.join("list.m3u8"), &[]).unwrap();
        assert_eq!(r.name, "list");
        assert_eq!(r.tracks.len(), 1);
        assert_eq!(r.missing, 2);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn latin1_files_still_read() {
        assert_eq!(decode(&[b'C', 0xE9, b'.', b'm', b'p', b'3']), "Cé.mp3");
        assert_eq!(decode("\u{feff}a.mp3".as_bytes()), "a.mp3");
    }
}
