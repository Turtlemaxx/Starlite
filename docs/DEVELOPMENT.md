# Starlite

A music player written in Rust, styled after Apple's Liquid Glass.

```
cargo run --release
```

- Drag the nav bar or the top strip to move the window. Double-click to maximize.
- Resize from the window edges. Alt+F4 closes it.

Starlite was called LiteStar before. On first start it copies the old data
folder (`%APPDATA%\LiteStar`: library, theme, logins, covers) to
`%APPDATA%\Starlite` and leaves the old one alone.

## What's in it

- **Logo and app icon:** the shooting star in `assets/` (`starlite-icon.svg`
  for the dark themes, `starlite-icon-light.svg` for Light). The sidebar logo
  and the window icon are drawn from the SVGs at their exact pixel size. The
  exe's icon (`assets/starlite.ico`, embedded by `build.rs`) only has
  48–256 px images, made from the 1024 px PNG.

- **Explore:** YouTube's official **Top Charts** (most-popular music in your
  country) and YouTube Music's **Chart Playlists** (Top 100 Songs / Music
  Videos), each a row of cards inside a glass bubble. Refresh (↻) reshuffles.
- **Navidrome** (under Explore): enter your server address, username and
  password once. Starlite checks them with the server and saves only a
  salted token (the Subsonic API's token login), never the password. Your
  server's playlists then show up as cards; open one to play it like any
  other playlist. Songs stream from your server. Formats Starlite can't decode
  (Opus, WMA, …) are asked for as MP3, which Navidrome transcodes.
  **Disconnect** forgets the login.
- **Shuffle:** every playlist has a Shuffle button next to Play that plays it
  in random order.
- **Playlists:** create one with **+** next to *Playlists*. Give it a name and
  pick a cover: one of 18 Lucide icons, or your own PNG/JPG. Your playlists
  appear under Explore. Open one to play it, remove songs, or delete it.
- **Liked Music** and **Downloaded Music** sit above Settings and can't be
  deleted. The heart in the player adds a song to Liked Music. Downloaded
  Music lists the MP3/FLAC/WAV/OGG files in the `Downloaded songs` folder.
- **Player bar:** cover, title and artist on the left (click the title to
  open the song on YouTube). Previous, play/pause and next in the center.
  Loop (off → playlist → one song), like, add to playlist, download and
  volume on the right. YouTube songs play in YouTube's official player, in a
  glass video card above the player bar.
- **Real song info:** local files show their tagged title, artist, album,
  length and embedded cover art (ID3 for MP3, Vorbis comments for FLAC/OGG).
  The file name is only a fallback.
- **Seek with the whole bar:** the player bar fills up as the song plays.
  Click or drag anywhere on it (not on a button) to jump. Hovering shows the
  time.
- **Full screen** (the ⤢ button on the right of the bar, Esc to leave): big
  cover with title, artist and album, lyrics on the side, and a background
  made from the cover's colours. Lyrics: a `.lrc` file with the same name or
  timed tags win; otherwise lrclib.net (Settings → Lyrics → Find lyrics
  online, on by default; only title, artist, album and length are sent, and
  results are cached in `lyrics-cache`), whose timed lyrics beat untimed tags.
  Timed lyrics glide along on per-line springs (lower lines a beat later), the
  wheel lets you look around for 2.5 s, and clicking a line jumps there.
  YouTube songs show their video instead of the cover.
- **Mini player:** switch to another app (or minimize Starlite) while a song
  is loaded and a small card
  slides out of the right edge of the screen, showing the album cover with
  the song name under it, on a background made from the cover's colours.
  Hover it to slide out the controls: seek, loop, previous, play/pause, next,
  like and volume. It slides back in as soon as the mouse leaves. Click the
  cover to bring Starlite back. The X hides it until you leave Starlite again.
  **Settings → Mini player → Change position & size** opens a customize mode:
  drag the card anywhere (it snaps to the nearest left or right screen edge,
  on any monitor), change its size (80–140 %) and turn each control on or off.
  Saved in `mini.json`.
- **Covers keep their shape:** album art is center-cropped to fill its spot
  instead of being stretched, and YouTube thumbnails lose their black bars.
- **Window buttons:** minimize, maximize and close at the top right (hidden in
  full screen).
- **Collapsible sidebar:** the arrow under Settings shrinks the nav bar to
  icons and playlist covers.
- **Themes:** Light, Dark, Cherry Blossom and Stars (Settings → Appearance).

## YouTube

Charts and playlists come from Google's official **YouTube Data API v3**.
Songs play in YouTube's **official embedded player** (in a WebView2 card).
YouTube requires that player to stay visible while it plays, and its ads are
untouched. They're gone if you're signed in with YouTube Premium. YouTube
songs can't be downloaded.

Connect in Settings → YouTube, either way:

- **Sign in with Google.** Whoever builds Starlite creates one free "Desktop
  app" OAuth client in Google Cloud and either builds with
  `STARLITE_GOOGLE_CLIENT_ID` / `STARLITE_GOOGLE_CLIENT_SECRET` or saves the
  downloaded client JSON as `%APPDATA%\Starlite\google_oauth.json`. Until
  Google verifies the app, sign-in works for up to 100 test users.
- **API key.** Create a free key in Google Cloud (enable YouTube Data API
  v3 → Credentials) and paste it in. Default quota: 10,000 units/day. One
  Explore refresh costs about 300.

Your country's charts are picked from the Windows region (override with
`STARLITE_REGION=US`).

## How the glass works

One GLSL shader (`src/glass.rs`) draws the theme background and up to 24 glass
elements: nav bar, pills, bubbles, player bar, buttons, and the volume lens. A
second pass draws dialogs and popovers as glass over the finished UI. Nothing
is captured from your desktop, and idle Light/Dark windows draw nothing.

Icons are rasterized at their exact on-screen pixel size (`src/icons.rs`), so
they stay sharp at any size and mid-animation.

## Files

| Path | Contents |
| --- | --- |
| `src/app.rs` | window shell, nav, playback queue, dialog, popover |
| `src/pages.rs` | Explore, playlist pages, Settings |
| `src/player.rs` | player bar (seek, controls) |
| `src/nowplaying.rs` | full screen Now Playing, lyrics, cover palette |
| `src/mini.rs` | mini player card shown while you're in another app |
| `src/navidrome.rs` | Navidrome / Subsonic API client |
| `src/meta.rs` | tags, cover art and lyrics from audio files |
| `src/lyrics.rs` | lyrics lookup on lrclib.net, cache |
| `src/audio.rs` | playback engine (rodio) |
| `src/youtube.rs` | YouTube Data API client (Google sign-in / API key) |
| `src/ytplayer.rs` | YouTube's embedded player in a WebView2 card |
| `src/library.rs` | playlists, Liked, Downloaded, `library.json` |
| `src/glass.rs` | glass shader + renderer |
| `src/logo.rs` | sidebar logo + window icon |
| `assets/starlite-icon*.svg/.png` | the logo (dark / light themes) |
| `build.rs` | embeds `assets/starlite.ico` in the exe |
| `assets/icons/` | Lucide icons + license |

## Icons

Icons come from [Lucide](https://lucide.dev) under the ISC license; some are
also MIT via Feather. They're free for public and commercial use, as long as
`assets/icons/LICENSE` ships with the project.

## Debug switches

| Env var | Effect |
| --- | --- |
| `STARLITE_PAGE=settings\|liked\|downloaded` | open on that page |
| `STARLITE_THEME=light\|dark\|cherry\|stars` | force a theme (doesn't change the saved one) |
| `STARLITE_NAV=mini` | start with the collapsed sidebar |
| `STARLITE_DIALOG=1` | open the New Playlist dialog |
| `STARLITE_AUTOPLAY=1` | play the first downloaded song |
| `STARLITE_DATA_DIR`, `STARLITE_DOWNLOADS_DIR` | use scratch folders instead of the real ones |
| `STARLITE_PLAYER_DEMO=1` | loop-one + volume lens state |
| `STARLITE_YT_VIDEO=<id>` | play that YouTube video at start |
| `STARLITE_FULLSCREEN=1` | open full screen Now Playing |
| `STARLITE_SEEK=<secs>` | jump there once playback starts |
| `STARLITE_TIMING=1` | print frame timings and repaint causes |
| `STARLITE_MINI_OPEN=1` | keep the mini player expanded |
| `STARLITE_MINI_EDIT=1` | start in the mini player's customize mode |
| `STARLITE_PAGE=navidrome` | open on the Navidrome page |
| `STARLITE_ND_CONNECT=url\|user\|password` | connect to a (test) Navidrome server at start |
| `STARLITE_ND_PLAY=<n>` | open the first Navidrome playlist and play song n |

## Planned

Navidrome (Subsonic) streaming and downloads, search.
