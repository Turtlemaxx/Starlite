//! The sidebar's layout (nav.json): which entries come in which order, the
//! categories between them, and what's pinned to the bottom above Settings.
//! It's one list; everything after the `Bottom` marker is the pinned part.
//! The sidebar's edit mode (the pencil under Settings) changes it by
//! dragging; this file is the list itself and the moves on it.

use serde::{Deserialize, Serialize};

use crate::library::Library;
use crate::stream::Server;

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
pub enum NavItem {
    Explore,
    /// Shown while that server is connected.
    Server(Server),
    Liked,
    Downloaded,
    /// One of your playlists, by id.
    Playlist(String),
    /// A server playlist added to the sidebar.
    Link(Server, String),
    /// A category title.
    Header(String),
    /// Everything after this is pinned to the bottom.
    Bottom,
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
pub struct NavPrefs {
    pub items: Vec<NavItem>,
}

impl Default for NavPrefs {
    fn default() -> Self {
        use NavItem::*;
        Self { items: vec![Explore, Server(crate::stream::Server::Navidrome), Server(crate::stream::Server::Jellyfin), Header("Playlists".into()), Bottom, Liked, Downloaded] }
    }
}

impl NavPrefs {
    fn file() -> std::path::PathBuf {
        crate::library::data_dir().join("nav.json")
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

    /// Where the pinned part starts.
    pub fn bottom(&self) -> usize {
        self.items.iter().position(|i| *i == NavItem::Bottom).unwrap_or(self.items.len())
    }

    /// Brings the list in line with what exists: the fixed entries are
    /// there exactly once, deleted playlists go, new ones are added (after
    /// the last playlist, else under the first category). True if it changed.
    pub fn sync(&mut self, lib: &Library) -> bool {
        use NavItem::*;
        let before = self.items.clone();
        let exists = |i: &NavItem| match i {
            Playlist(id) => lib.user_playlists().any(|p| &p.id == id),
            Link(s, id) => lib.nd_link(*s, id).is_some(),
            _ => true,
        };
        // No doubles (categories may share a name), nothing that's gone.
        let mut seen: Vec<NavItem> = Vec::new();
        self.items.retain(|i| {
            let keep = exists(i) && (matches!(i, Header(_)) || !seen.contains(i));
            if keep {
                seen.push(i.clone());
            }
            keep
        });
        if !self.items.contains(&Bottom) {
            self.items.push(Bottom);
        }
        for fixed in [Explore, Server(crate::stream::Server::Navidrome), Server(crate::stream::Server::Jellyfin)] {
            if !self.items.contains(&fixed) {
                // In front, after the fixed ones already there.
                let at = self.items.iter().take_while(|i| matches!(i, Explore | Server(_))).count();
                self.items.insert(at, fixed);
            }
        }
        for fixed in [Liked, Downloaded] {
            if !self.items.contains(&fixed) {
                self.items.push(fixed);
            }
        }
        let new: Vec<NavItem> = lib.user_playlists().map(|p| Playlist(p.id.clone())).chain(lib.nd_links.iter().map(|l| Link(l.server, l.id.clone()))).filter(|i| !self.items.contains(i)).collect();
        for item in new {
            let at = self.new_playlist_spot();
            self.items.insert(at, item);
        }
        self.items != before
    }

    /// Where a new playlist goes when nothing says otherwise.
    fn new_playlist_spot(&self) -> usize {
        use NavItem::*;
        let bottom = self.bottom();
        if let Some(last) = self.items[..bottom].iter().rposition(|i| matches!(i, Playlist(_) | Link(..))) {
            return last + 1;
        }
        match self.items[..bottom].iter().position(|i| matches!(i, Header(_))) {
            Some(h) => h + self.block_len(h),
            None => bottom,
        }
    }

    /// How many entries move together from `i`: a category takes what's
    /// under it along (up to the next category or the pinned part).
    pub fn block_len(&self, i: usize) -> usize {
        match self.items.get(i) {
            Some(NavItem::Header(_)) => 1 + self.items[i + 1..].iter().take_while(|x| !matches!(x, NavItem::Header(_) | NavItem::Bottom)).count(),
            Some(_) => 1,
            None => 0,
        }
    }

    /// Moves the block at `from` so it sits before the `to`-th of the other
    /// entries (`to` counts without the block). Returns where it is now.
    pub fn move_block(&mut self, from: usize, to: usize) -> usize {
        let len = self.block_len(from);
        if len == 0 {
            return from;
        }
        let block: Vec<NavItem> = self.items.drain(from..from + len).collect();
        let to = to.min(self.items.len());
        self.items.splice(to..to, block);
        to
    }

    /// Puts a (new) playlist at the end of the category at `header`.
    pub fn place_under(&mut self, header: usize, item: NavItem) {
        if !matches!(self.items.get(header), Some(NavItem::Header(_))) {
            return;
        }
        let Some(from) = self.items.iter().position(|i| *i == item) else { return };
        let item = self.items.remove(from);
        let header = if from < header { header - 1 } else { header };
        let at = header + self.block_len(header);
        self.items.insert(at, item);
    }

    /// A new category at the end of the scrolling part. Returns where it is.
    pub fn add_header(&mut self, name: &str) -> usize {
        let at = self.bottom();
        self.items.insert(at, NavItem::Header(name.to_string()));
        at
    }
}

#[cfg(test)]
mod tests {
    use super::NavItem::*;
    use super::*;
    use crate::library::{Cover, NdLink, Playlist};

    fn lib(ids: &[&str]) -> Library {
        let mut l = Library::default();
        l.playlists.push(Playlist { id: "liked".into(), name: "Liked Music".into(), cover: Cover::Preset("heart".into()), tracks: vec![], fixed: true });
        for id in ids {
            l.playlists.push(Playlist { id: id.to_string(), name: id.to_string(), cover: Cover::Preset("disc-3".into()), tracks: vec![], fixed: false });
        }
        l
    }

    fn pl(id: &str) -> NavItem {
        Playlist(id.into())
    }

    #[test]
    fn the_default_is_the_sidebar_as_it_was() {
        let mut n = NavPrefs::default();
        assert!(n.sync(&lib(&["a", "b"])));
        assert_eq!(n.items, vec![Explore, Server(crate::stream::Server::Navidrome), Server(crate::stream::Server::Jellyfin), Header("Playlists".into()), pl("a"), pl("b"), Bottom, Liked, Downloaded]);
        // Nothing to do the second time.
        assert!(!n.sync(&lib(&["a", "b"])));
    }

    #[test]
    fn playlists_come_and_go_and_broken_files_are_mended() {
        let mut n = NavPrefs { items: vec![Header("Chill".into()), pl("a"), Header("Loud".into()), pl("b"), pl("gone"), Liked, Liked] };
        let mut l = lib(&["a", "b", "c"]);
        l.nd_links.push(NdLink { server: crate::stream::Server::Jellyfin, id: "j".into(), title: "J".into(), owner: String::new(), artwork: None, track_count: 0 });
        n.sync(&l);
        // New ones follow the last playlist; the fixed entries are back, once each.
        assert_eq!(
            n.items,
            vec![Explore, Server(crate::stream::Server::Navidrome), Server(crate::stream::Server::Jellyfin), Header("Chill".into()), pl("a"), Header("Loud".into()), pl("b"), pl("c"), Link(crate::stream::Server::Jellyfin, "j".into()), Liked, Bottom, Downloaded]
        );
    }

    #[test]
    fn a_category_moves_with_what_is_under_it() {
        let mut n = NavPrefs { items: vec![Explore, Header("A".into()), pl("a1"), pl("a2"), Header("B".into()), pl("b1"), Bottom, Liked, Downloaded] };
        assert_eq!((n.block_len(1), n.block_len(4), n.block_len(0), n.block_len(5)), (3, 2, 1, 1));
        // "B" (and b1) in front of "A": before the 1st of the others.
        assert_eq!(n.move_block(4, 1), 1);
        assert_eq!(n.items, vec![Explore, Header("B".into()), pl("b1"), Header("A".into()), pl("a1"), pl("a2"), Bottom, Liked, Downloaded]);
        // Downloaded up under Explore.
        let d = n.items.iter().position(|i| *i == Downloaded).unwrap();
        n.move_block(d, 1);
        assert_eq!(n.items[..3], [Explore, Downloaded, Header("B".into())]);
        // Explore down into the pinned part.
        n.move_block(0, n.items.len() - 1);
        assert_eq!(n.items[n.bottom() + 1..], [Liked, Explore]);
    }

    #[test]
    fn new_playlists_can_go_under_a_chosen_category() {
        let (nd, jf) = (Server(crate::stream::Server::Navidrome), Server(crate::stream::Server::Jellyfin));
        let mut n = NavPrefs { items: vec![Explore, nd, jf, Header("A".into()), pl("a1"), Header("B".into()), pl("b1"), Bottom, Liked, Downloaded] };
        n.sync(&lib(&["a1", "b1", "new"]));
        // By itself it follows the last playlist.
        assert_eq!(n.items[7], pl("new"));
        n.place_under(3, pl("new"));
        assert_eq!(n.items[3..7], [Header("A".into()), pl("a1"), pl("new"), Header("B".into())]);
        let h = n.add_header("Later");
        assert_eq!((n.items[h].clone(), n.items[h + 1].clone()), (Header("Later".into()), Bottom));
    }
}
