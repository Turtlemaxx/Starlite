//! Tracks, playlists and their persistence (%APPDATA%\Starlite\library.json).
//!
//! Two playlists always exist and can't be removed:
//! * Liked Music: songs you heart.
//! * Downloaded Music: the audio files in the `Downloaded songs` folder.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

pub const LIKED_ID: &str = "liked";
pub const DOWNLOADED_ID: &str = "downloaded";

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub enum Source {
    File { path: PathBuf },
    YouTube { video_id: String },
    /// A song on the user's Navidrome server (streamed from it).
    Navidrome { song_id: String, suffix: String },
    /// A song on the user's Jellyfin server (streamed from it).
    Jellyfin { item_id: String },
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Track {
    pub id: String,
    pub title: String,
    pub artist: String,
    #[serde(default)]
    pub album: Option<String>,
    pub artwork: Option<String>,
    /// Link back to the song on YouTube.
    pub permalink: Option<String>,
    pub duration_ms: u64,
    pub source: Source,
}

impl Track {
    pub fn youtube_id(&self) -> Option<&str> {
        match &self.source {
            Source::YouTube { video_id } => Some(video_id),
            _ => None,
        }
    }

    /// Where the song comes from, for the credit line.
    pub fn source_name(&self) -> Option<&'static str> {
        match self.source {
            Source::YouTube { .. } => Some("YouTube"),
            Source::File { .. } | Source::Navidrome { .. } | Source::Jellyfin { .. } => None,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub enum Cover {
    /// A Lucide icon name from `icons::PRESETS`.
    Preset(String),
    /// An image file copied into the app's covers folder.
    Image(PathBuf),
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Playlist {
    pub id: String,
    pub name: String,
    pub cover: Cover,
    #[serde(default)]
    pub tracks: Vec<Track>,
    /// Built-in playlists can't be deleted or renamed.
    #[serde(default)]
    pub fixed: bool,
}

/// A Navidrome or Jellyfin playlist added to your playlists in the sidebar.
/// Only the link is kept: its songs load from the server when you open it.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct NdLink {
    /// Which server it's on (Navidrome in files from before Jellyfin).
    #[serde(default)]
    pub server: crate::stream::Server,
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub owner: String,
    #[serde(default)]
    pub artwork: Option<String>,
    #[serde(default)]
    pub track_count: u64,
}

impl NdLink {
    pub fn from_remote(server: crate::stream::Server, p: &crate::youtube::RemotePlaylist) -> NdLink {
        NdLink { server, id: p.id.clone(), title: p.title.clone(), owner: p.artist.clone(), artwork: p.artwork.clone(), track_count: p.track_count }
    }
    pub fn to_remote(&self) -> crate::youtube::RemotePlaylist {
        crate::youtube::RemotePlaylist { id: self.id.clone(), title: self.title.clone(), artist: self.owner.clone(), artwork: self.artwork.clone(), permalink: None, track_count: self.track_count }
    }
}

#[derive(Default, Serialize, Deserialize)]
pub struct Library {
    pub playlists: Vec<Playlist>,
    /// Navidrome playlists shown with your own.
    #[serde(default)]
    pub nd_links: Vec<NdLink>,
    #[serde(skip)]
    pub downloaded: Vec<Track>,
}

pub fn data_dir() -> PathBuf {
    // Debug: STARLITE_DATA_DIR points the app at a scratch library.
    if let Ok(d) = std::env::var("STARLITE_DATA_DIR") {
        return PathBuf::from(d);
    }
    std::env::var("APPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("."))
        .join("Starlite")
}

/// The app used to be called LiteStar: copy its data (library, theme,
/// logins, covers) into the new Starlite folder once. The old folder stays.
pub fn migrate_old_data() {
    if std::env::var("STARLITE_DATA_DIR").is_ok() {
        return;
    }
    let Ok(appdata) = std::env::var("APPDATA") else { return };
    let old = PathBuf::from(&appdata).join("LiteStar");
    let new = PathBuf::from(&appdata).join("Starlite");
    if new.exists() || !old.is_dir() {
        return;
    }
    fn copy_dir(from: &Path, to: &Path) {
        let _ = std::fs::create_dir_all(to);
        let Ok(rd) = std::fs::read_dir(from) else { return };
        for e in rd.flatten() {
            let p = e.path();
            let dest = to.join(e.file_name());
            if p.is_dir() {
                copy_dir(&p, &dest);
            } else {
                // Files in use (e.g. by a running LiteStar) are skipped.
                let _ = std::fs::copy(&p, &dest);
            }
        }
    }
    copy_dir(&old, &new);
    // Saved paths (custom playlist covers) point into the old folder.
    let lib = new.join("library.json");
    if let Ok(s) = std::fs::read_to_string(&lib) {
        let from = serde_json::to_string(&old.to_string_lossy()).unwrap_or_default();
        let to = serde_json::to_string(&new.to_string_lossy()).unwrap_or_default();
        let (from, to) = (from.trim_matches('"'), to.trim_matches('"'));
        let _ = std::fs::write(&lib, s.replace(from, to));
    }
}

/// The `Downloaded songs` folder next to the executable's working directory.
pub fn downloads_dir() -> PathBuf {
    if let Ok(d) = std::env::var("STARLITE_DOWNLOADS_DIR") {
        return PathBuf::from(d);
    }
    std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")).join("Downloaded songs")
}

fn library_file() -> PathBuf {
    data_dir().join("library.json")
}

fn now_id() -> String {
    let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default();
    format!("pl{}", t.as_millis())
}

impl Library {
    pub fn load() -> Library {
        let mut lib: Library = std::fs::read_to_string(library_file())
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        if !lib.playlists.iter().any(|p| p.id == LIKED_ID) {
            lib.playlists.insert(0, Playlist {
                id: LIKED_ID.into(),
                name: "Liked Music".into(),
                cover: Cover::Preset("heart".into()),
                tracks: Vec::new(),
                fixed: true,
            });
        }
        lib.playlists.retain(|p| p.id != DOWNLOADED_ID);
        // Navidrome covers saved by 0.1.0 still carried the login token.
        let mut cleaned = false;
        for p in &mut lib.playlists {
            for t in &mut p.tracks {
                if let Some(clean) = t.artwork.as_deref().and_then(crate::navidrome::strip_cover_token) {
                    t.artwork = Some(clean);
                    cleaned = true;
                }
            }
        }
        if cleaned {
            lib.save();
        }
        // Refresh local songs from their real tags (titles, covers can change).
        for p in &mut lib.playlists {
            for t in &mut p.tracks {
                if let Source::File { path } = &t.source {
                    if path.exists() {
                        *t = file_track(&path.clone());
                    }
                }
            }
        }
        lib.scan_downloads();
        lib
    }

    pub fn save(&self) {
        let _ = std::fs::create_dir_all(data_dir());
        if let Ok(s) = serde_json::to_string_pretty(self) {
            let tmp = library_file().with_extension("json.tmp");
            if std::fs::write(&tmp, s).is_ok() {
                let _ = std::fs::rename(&tmp, library_file());
            }
        }
    }

    /// Re-read the audio files in the `Downloaded songs` folder.
    pub fn scan_downloads(&mut self) {
        let dir = downloads_dir();
        let _ = std::fs::create_dir_all(&dir);
        let mut files: Vec<PathBuf> = std::fs::read_dir(&dir)
            .map(|rd| rd.flatten().map(|e| e.path()).collect())
            .unwrap_or_default();
        files.retain(|p| is_audio(p));
        files.sort();
        self.downloaded = files.into_iter().map(|p| file_track(&p)).collect();
    }

    pub fn liked(&self) -> &Playlist {
        self.playlists.iter().find(|p| p.id == LIKED_ID).expect("liked playlist")
    }

    pub fn is_liked(&self, t: &Track) -> bool {
        self.liked().tracks.iter().any(|x| x.id == t.id)
    }

    pub fn toggle_like(&mut self, t: &Track) {
        if let Some(p) = self.playlists.iter_mut().find(|p| p.id == LIKED_ID) {
            if let Some(i) = p.tracks.iter().position(|x| x.id == t.id) {
                p.tracks.remove(i);
            } else {
                p.tracks.insert(0, t.clone());
            }
        }
        self.save();
    }

    /// User-created playlists (not the built-in ones).
    pub fn user_playlists(&self) -> impl Iterator<Item = &Playlist> {
        self.playlists.iter().filter(|p| !p.fixed)
    }

    pub fn create(&mut self, name: String, cover: Cover) -> String {
        let id = now_id();
        self.playlists.push(Playlist { id: id.clone(), name, cover, tracks: Vec::new(), fixed: false });
        self.save();
        id
    }

    pub fn delete(&mut self, id: &str) {
        if let Some(i) = self.playlists.iter().position(|p| p.id == id && !p.fixed) {
            if let Cover::Image(path) = &self.playlists[i].cover {
                let _ = std::fs::remove_file(path);
            }
            self.playlists.remove(i);
            self.save();
        }
    }

    pub fn add_to(&mut self, id: &str, t: &Track) -> bool {
        let Some(p) = self.playlists.iter_mut().find(|p| p.id == id) else { return false };
        if p.tracks.iter().any(|x| x.id == t.id) {
            return false;
        }
        p.tracks.push(t.clone());
        self.save();
        true
    }

    /// Adds songs (skipping ones already there); how many were added.
    pub fn add_many(&mut self, id: &str, tracks: &[Track]) -> usize {
        let Some(p) = self.playlists.iter_mut().find(|p| p.id == id) else { return 0 };
        let mut n = 0;
        for t in tracks {
            if !p.tracks.iter().any(|x| x.id == t.id) {
                p.tracks.push(t.clone());
                n += 1;
            }
        }
        self.save();
        n
    }

    pub fn nd_link(&self, server: crate::stream::Server, id: &str) -> Option<&NdLink> {
        self.nd_links.iter().find(|l| l.server == server && l.id == id)
    }

    /// Adds a Navidrome playlist to your playlists, or takes it away again.
    pub fn toggle_nd_link(&mut self, server: crate::stream::Server, p: &crate::youtube::RemotePlaylist) -> bool {
        let added = if let Some(i) = self.nd_links.iter().position(|l| l.server == server && l.id == p.id) {
            self.nd_links.remove(i);
            false
        } else {
            self.nd_links.push(NdLink::from_remote(server, p));
            true
        };
        self.save();
        added
    }

    /// Keeps linked playlists' names, covers and counts up to date.
    pub fn refresh_nd_links(&mut self, server: crate::stream::Server, lists: &[crate::youtube::RemotePlaylist]) {
        let mut changed = false;
        for l in self.nd_links.iter_mut().filter(|l| l.server == server) {
            if let Some(p) = lists.iter().find(|p| p.id == l.id) {
                let fresh = NdLink::from_remote(server, p);
                if *l != fresh {
                    *l = fresh;
                    changed = true;
                }
            }
        }
        if changed {
            self.save();
        }
    }

    pub fn remove_from(&mut self, id: &str, track_id: &str) {
        if let Some(p) = self.playlists.iter_mut().find(|p| p.id == id) {
            p.tracks.retain(|t| t.id != track_id);
            self.save();
        }
    }
}

pub fn is_audio(p: &Path) -> bool {
    matches!(
        p.extension().and_then(|e| e.to_str()).map(|e| e.to_ascii_lowercase()).as_deref(),
        Some("mp3" | "flac" | "wav" | "ogg" | "m4a")
    )
}

pub fn file_track(p: &Path) -> Track {
    let m = crate::meta::read(p);
    // Real tags first; only fall back to the file name when a field is missing.
    let stem = p.file_stem().and_then(|s| s.to_str()).unwrap_or("Unknown").to_string();
    let (name_artist, name_title) = match stem.split_once(" - ") {
        Some((a, t)) => (Some(a.trim().to_string()), t.trim().to_string()),
        None => (None, stem.clone()),
    };
    Track {
        id: format!("file:{}", p.display()),
        title: m.title.unwrap_or(name_title),
        artist: m.artist.or(name_artist).unwrap_or_else(|| "Unknown artist".into()),
        album: m.album,
        artwork: m.cover.map(|c| format!("file://{}", c.display())),
        permalink: None,
        duration_ms: m.duration_ms,
        source: Source::File { path: p.to_path_buf() },
    }
}

/// Copy a user-picked cover image into the app's covers folder.
pub fn import_cover(src: &Path) -> Option<PathBuf> {
    let ext = src.extension()?.to_str()?.to_ascii_lowercase();
    if !matches!(ext.as_str(), "png" | "jpg" | "jpeg") {
        return None;
    }
    let dir = data_dir().join("covers");
    std::fs::create_dir_all(&dir).ok()?;
    let dst = dir.join(format!("{}.{ext}", now_id()));
    std::fs::copy(src, &dst).ok()?;
    Some(dst)
}
