//! How songs are asked for from a server (Navidrome, Jellyfin): as they are,
//! or converted by the server to a format and bitrate you pick
//! (streaming.json). Smaller streams load faster on slow connections.

use serde::{Deserialize, Serialize};

/// The two kinds of music server Starlite plays from.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Server {
    #[default]
    Navidrome,
    Jellyfin,
}

impl Server {
    pub fn name(self) -> &'static str {
        match self {
            Server::Navidrome => "Navidrome",
            Server::Jellyfin => "Jellyfin",
        }
    }

    pub fn icon(self) -> &'static str {
        match self {
            Server::Navidrome => "cloudy",
            Server::Jellyfin => "server",
        }
    }
}

/// A cover link ready to load. Server covers are stored as an id only
/// (`navidrome-cover:…`, `jellyfin-cover:…`) and get their address and login
/// here (None: that server isn't connected).
pub fn cover_link(uri: &str) -> Option<String> {
    if uri.starts_with(crate::navidrome::COVER_SCHEME) {
        crate::navidrome::resolve_cover(uri)
    } else if uri.starts_with(crate::jellyfin::COVER_SCHEME) {
        crate::jellyfin::resolve_cover(uri)
    } else {
        Some(uri.to_string())
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Format {
    #[default]
    Mp3,
    Aac,
    Flac,
}

impl Format {
    pub const ALL: [Format; 3] = [Format::Mp3, Format::Aac, Format::Flac];

    pub fn label(self) -> &'static str {
        match self {
            Format::Mp3 => "MP3",
            Format::Aac => "AAC",
            Format::Flac => "FLAC",
        }
    }

    /// The name servers know it by (also the file ending).
    pub fn ext(self) -> &'static str {
        match self {
            Format::Mp3 => "mp3",
            Format::Aac => "aac",
            Format::Flac => "flac",
        }
    }

    /// FLAC keeps everything: there's no bitrate to pick.
    pub fn lossless(self) -> bool {
        self == Format::Flac
    }
}

/// Bitrates on offer (kbit/s).
pub const BITRATES: [u32; 6] = [96, 128, 160, 192, 256, 320];

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Debug)]
#[serde(default)]
pub struct StreamPrefs {
    /// Off: songs come as they are on the server (only formats Starlite
    /// can't play are converted, to MP3 320).
    pub transcode: bool,
    pub format: Format,
    /// kbit/s.
    pub bitrate: u32,
}

impl Default for StreamPrefs {
    fn default() -> Self {
        Self { transcode: false, format: Format::Mp3, bitrate: 192 }
    }
}

impl StreamPrefs {
    fn file() -> std::path::PathBuf {
        crate::library::data_dir().join("streaming.json")
    }

    pub fn load() -> Self {
        let mut p: Self = std::fs::read_to_string(Self::file()).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
        // The nearest bitrate on offer.
        p.bitrate = *BITRATES.iter().min_by_key(|b| b.abs_diff(p.bitrate)).unwrap();
        p
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn off_by_default_and_reads_partial_files() {
        assert!(!StreamPrefs::default().transcode);
        let p: StreamPrefs = serde_json::from_str(r#"{"transcode": true, "format": "Aac"}"#).unwrap();
        assert_eq!((p.transcode, p.format, p.bitrate), (true, Format::Aac, 192));
    }
}
