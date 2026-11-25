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
  **Disconnect** forgets the login. Right-click a playlist to add it to your
  playlists in the sidebar (only the link is kept; its songs still load from
  the server). A playlist opened from the Navidrome page has a back button.
- **Shuffle:** every playlist has a Shuffle button next to Play that plays it
  in random order.
- **Playlists:** create one with **+** next to *Playlists*. Give it a name and
  pick a cover: one of 18 Lucide icons, or your own PNG/JPG. Your playlists
  appear under Explore. Open one to play it, remove songs, or delete it.
  **Load .m3u** in the dialog fills it from an .m3u/.m3u8 file: paths are
  taken as written (or relative to the file); a song that moved is looked
  for among your songs by file name, then by its #EXTINF title. Web links
  and songs that can't be found are skipped and counted.
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
  wheel lets you look around for 2.5 s, and clicking a line jumps there. In
  the current line the words light up as they're sung (a soft sweep), timed
  by word stamps (`<mm:ss.xx>`, enhanced LRC) when the lyrics have them, or
  else spread over the line by word length.
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
- **Themes:** Light, Dark, Cherry Blossom, Stars, Bubbles (glass bubbles that
  rise and pop at random heights) and Custom (Settings → Appearance). Custom
  has a background, accent and text colour (each with a picker) and an
  optional PNG/JPG background picture, saved in `custom-theme.json` (the
  picture is copied into the `backgrounds` folder).
- **Theme CSS** (Custom theme → Edit CSS, `theme.css`): a real but small
  CSS. Selectors `:root` / `body`, `.panel`, `.sidebar`, `.player`;
  properties for colours (`background`, `color`, `accent-color`,
  `border-color`), a picture (`background-image: url(…)`), `border-radius`,
  `backdrop-filter: blur(…)` and `font-family` (any installed font, found
  through Windows' font list). Custom properties and `var()` work. What it
  can't use is shown with its line number next to the button. The file is
  read again whenever it's saved (looked at once a second while the Custom
  theme is on), and its values go on top of the picked colours.
- **Sidebar layout** (`src/nav.rs`, `nav.json`): the sidebar is one ordered
  list of entries (Explore, the servers, Liked, Downloaded, playlists,
  server links) with category titles between them; what comes after the
  `Bottom` marker is pinned above Settings, the rest scrolls. The pencil
  under Settings starts edit mode: drag an entry to move it (a category
  takes what's under it along, and dragging past the gap moves it between
  the scrolling and the pinned part), click a category to rename it, x
  removes it, "Add category" makes one. The + on a category starts a
  playlist under it. Settings itself can't be moved.
- **Services** (`src/services.rs`): Navidrome, Jellyfin, YouTube, Discord
  and Last.fm each have their own page in Settings (a green dot in the
  list: set up). Servers are signed in to there; the sidebar only shows
  the ones that are connected, and a server's page sends you back there
  when it isn't. On the very first start (nothing saved yet, marker file
  `welcomed`) a welcome window offers to connect Navidrome, Jellyfin or
  both at once.
- **Update notice** (`src/updater.rs`, `updates.json`): on start and every
  six hours Starlite asks GitHub for its latest release. A newer one opens
  a window with Install: the zip is downloaded (only from this repo's
  release downloads), checked against the SHA-256 in the release notes,
  unpacked with Windows' tar, the running exe is renamed to
  `Starlite.old.exe`, the new one put in its place and started. Every
  release's notes therefore need their `SHA-256:` line. Switched off under
  Settings → General; then nothing is asked.
- **Frame pacing and the flicker:** each frame of the main window starts
  just after the screen's refresh (`win::wait_for_refresh`, from the
  compositor's timing; vsync is off since this paces it). Without that,
  NVIDIA's OpenGL on Windows 11 sometimes showed a frame before it was
  drawn: the background with nothing on it, for one frame, when the window
  gained focus or the mouse came in. Measured with screen recordings: about
  one in 35-65 focus changes before, none in about 700 after.
- **Settings** (`src/settings.rs`): the categories in a list on the left
  (Appearance, Layout, Mini player; Sound: Playback, Equalizer, Effects;
  Services: one page each; General), the picked one on the right. Settings
  sit in groups of rows (`Group`): the name on the left, its control on the
  right (switch, choices, slider, button), a thin line between rows, the
  group on one panel. Pages that can be taller than the window scroll;
  glass elements in them are cut to the visible area.
- **Appearance** (`src/appearance.rs`, `look.json`): theme (each card is a
  small picture of the theme), saved looks, panels (style: Simple, flat and
  solid panels, the default (the sidebar is part of the background, so a
  theme's picture or animation runs under it); Glass, the same panels
  see-through with what's behind blurred, like Windows' Acrylic: more
  saturation, then the brightness pulled towards a fixed level so text
  reads on any background. On a plain theme (Light, Dark, Custom without a
  picture) there's nothing in the window to see, so the window itself turns
  see-through and Windows blurs the desktop behind it (`DWMSBT_TRANSIENTWINDOW`,
  frame extended over the window, the system-menu style dropped so Windows
  doesn't draw its own caption buttons; the window is created transparent
  and the shader writes alpha only in this mode); Liquid glass, which bends
  and reflects at the rim; and corners, opacity,
  blur, edges, shadows), colour (accent: theme's own, from the playing
  cover, or picked; background: theme's own or the cover's colours), text
  and icons (a font from Windows' font list, text size, icon line width)
  and, for the themes that move, their speed and amount. Untouched values
  follow the style (`None` in look.json). "Reset to defaults" in the
  corner.
- **Layout:** song row size, song numbers, album name (its own column when
  there's room), sidebar width and covers, which player bar buttons show,
  the CD shelf. **Mini player:** on / off, only while playing, always show
  the controls, cover colours, Edit (position, size, buttons).
- **General:** the click sound (Classic, Soft, your own .wav / .mp3 / .ogg,
  copied into `sounds`, first two seconds, levelled; or off), the version
  and Check for updates, update notifications, the data folder, release
  notes.
- **Saved looks** (`looks.json`): theme, Custom colours and picture (its own
  copy in `backgrounds`), the theme's CSS, style and the look values, under
  a name. Switching to one restores all of it; a theme.css the look didn't
  have is kept as `theme.backup.css`.
- **Playlist search:** a search field in every playlist's header filters by
  song, artist or album (every word must match), in the list and on the CD
  shelf. Ctrl+F focuses it, Esc clears it. Playing a found song plays the
  whole playlist from there.
- **Speed and bass:** the sliders button left of loop opens a dropdown above
  the bar with two line sliders: speed 0.5–2× (pitch follows, like a
  record; YouTube rounds to its own steps) and bass −12…+24 dB (the Bass
  effect; not for YouTube songs). Values glide, dragging ticks softly,
  double-click resets. A link leads on to the equalizer. Saved in
  `sound.json`.
- **Gapless and crossfade** (Settings → Playback): the song that follows is
  loaded ahead and handed to the mixer, which starts it on the sample after
  the current one's last (crossfade 0) or fades the two into each other
  with equal power (up to 12 s). YouTube songs can't take part.
- **Equalizer** (Settings → Equalizer): preamp and ten bands (31 Hz–16 kHz,
  ±12 dB) with the curve drawn above; the picked band can be fine-tuned in
  0.1 dB steps and its width (Q 0.3–8) set.
- **Effects** (Settings → Effects), each with a switch (all off at first)
  and its own sliders: bass, clarity, stereo widening, crossfeed, headphone
  surround, reverb, compressor, output volume / balance, limiter. With
  everything off the sound passes through bit for bit.
- **Server streams** (Settings → Playback): songs from Navidrome / Jellyfin
  come as stored, or converted by the server to MP3, AAC or FLAC at a chosen
  bitrate. A server without that converter answers with its default (often
  Opus), which Starlite can't decode and says so.
- **Jellyfin** (in the sidebar): sign in with address, username and
  password; only the access token Jellyfin returns is saved
  (`jellyfin.json`). Playlists, right-click to add one to your playlists,
  shelf previews, gapless: everything Navidrome playlists do.
- **Last.fm** (Settings → Last.fm): scrobbling with your own API key and
  shared secret; you allow Starlite in the browser (token flow), so the
  Last.fm password never reaches the app. "Now playing" when a song starts,
  a scrobble after half the song or four minutes; ones that couldn't be
  sent wait in `lastfm-queue.json`.
- **CD shelf** (Settings → Appearance, off by default, `shelf.json`): playlist
  songs stand as CD cases on a shelf, spine out (spine colour from the cover).
  Hovering one pulls it out: a small 3D jewel-case model (projected egui
  meshes) turns to face you in the middle of the page and tilts with the
  mouse, while the page behind blurs (a GL pass over Starlite's own frame,
  `src/blur.rs`). The disc slides out, about 15 s of the song play on a
  second output with the main player turned down (`src/preview.rs`): from
  the chorus (the block of timed lyric lines that repeats most), else where
  the singing starts, else the loudest stretch. Seven band-pass filters
  drive the equalizer. YouTube songs have no preview. Click a case or spine
  to play the song.

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

## How the sound gets out

`src/audio.rs` owns the device and one endless source, the mixer
(`src/engine.rs`). Songs become "decks" (decoded and resampled to the
device's rate, stereo); the mixer plays the current deck, brings the next
one in (gapless or crossfade), and runs every frame through `src/dsp.rs`
(equalizer, then the effects in a fixed order; filters in double
precision). Commands reach it over a channel, state comes back through
atomics. Shelf previews have their own output and skip all of this.

## How the panels are drawn

At most `MAX_EL` (48) elements are drawn per frame; anything past that was
dropped for the frame and blinked, so keep pages well under it. Hover
highlights that fade in and out are each their own element.

One GLSL shader (`src/glass.rs`) draws the theme background and up to 24
elements: nav bar, pills, bubbles, player bar, buttons, and the volume knob,
either as flat panels (the default) or as liquid glass. A second pass draws
dialogs and popovers over the finished UI. Nothing is captured from your
desktop, and idle Light/Dark windows draw nothing.

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
| `src/shelf.rs` | CD shelf view and the 3D case |
| `src/fx.rs` | sound settings: speed, crossfade, equalizer, effects (`sound.json`) |
| `src/dsp.rs` | the equalizer and the effects |
| `src/engine.rs` | the mixer: gapless, crossfade, then `dsp` |
| `src/settings.rs` | the Settings page: category list, grouped rows, Layout, Mini player, General |
| `src/soundpage.rs` | Settings → Playback, Equalizer, Effects |
| `src/stream.rs` | how server songs are asked for (as stored / converted) |
| `src/jellyfin.rs` | Jellyfin API client |
| `src/lastfm.rs` | Last.fm scrobbling and its Settings tab |
| `src/updater.rs` | update notice and install |
| `src/nav.rs` | the sidebar's layout list and the moves on it |
| `src/appearance.rs` | Settings → Appearance, saved looks, the font list |
| `src/services.rs` | the services' status, opening their Settings page, the first-start welcome window |
| `src/css.rs` | the Custom theme's CSS: parser and font lookup |
| `src/fxpanel.rs` | the speed / bass dropdown above the player bar |
| `src/customize.rs` | custom theme settings and the colour picker |
| `src/m3u.rs` | loading a playlist from an .m3u / .m3u8 file |
| `src/preview.rs` | song previews and equalizer levels |
| `src/blur.rs` | blur pass behind the pulled-out case |
| `src/navidrome.rs` | Navidrome / Subsonic API client |
| `src/meta.rs` | tags, cover art and lyrics from audio files |
| `src/lyrics.rs` | lyrics lookup on lrclib.net, cache |
| `src/audio.rs` | playback: device, loading songs, lining up the next one |
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
| `STARLITE_THEME=light\|dark\|cherry\|stars\|bubbles\|custom` | force a theme (doesn't change the saved one) |
| `STARLITE_NAV=mini` | start with the collapsed sidebar |
| `STARLITE_DIALOG=1` | open the New Playlist dialog |
| `STARLITE_AUTOPLAY=1` | play the first downloaded song |
| `STARLITE_DATA_DIR`, `STARLITE_DOWNLOADS_DIR` | use scratch folders instead of the real ones |
| `STARLITE_PLAYER_DEMO=1` | loop-one + volume lens state |
| `STARLITE_YT_VIDEO=<id>` | play that YouTube video at start |
| `STARLITE_FULLSCREEN=1` | open full screen Now Playing |
| `STARLITE_SEEK=<secs>` | jump there once playback starts |
| `STARLITE_TIMING=1` | print frame timings and repaint causes |
| `STARLITE_BLANKCHECK=1` | say when a drawn frame is missing everything but the background |
| `STARLITE_NOSYNC=1` | don't wait for the screen's refresh before drawing (brings the flicker back, for testing) |
| `STARLITE_NODESKTOP=1` | Glass on a plain theme without the see-through window |
| `STARLITE_MINI_OPEN=1` | keep the mini player expanded |
| `STARLITE_MINI_EDIT=1` | start in the mini player's customize mode |
| `STARLITE_PAGE=navidrome` | open on the Navidrome page |
| `STARLITE_ND_CONNECT=url\|user\|password` | connect to a (test) Navidrome server at start |
| `STARLITE_ND_PLAY=<n>` | open the first Navidrome playlist and play song n |
| `STARLITE_ND_OPEN=1` | open the first Navidrome playlist |
| `STARLITE_ND_MENU=x,y` | open the first Navidrome playlist's right-click menu there |
| `STARLITE_COLOR_PICK=0..2` | open the custom theme's colour picker (Settings) |
| `STARLITE_M3U=<file>` | with `STARLITE_DIALOG`, load that .m3u into the dialog |
| `STARLITE_SHOTS=dir,count,ms,delay` | save `count` window frames to `dir`, then quit |
| `STARLITE_MUTE=1` | start muted |
| `STARLITE_NAV_EDIT=1` | start with the sidebar in edit mode |
| `STARLITE_NAV_DRAG=entry,y` | in edit mode, drag that entry to height y once |
| `STARLITE_WELCOME=1` | show the first-start welcome window |
| `STARLITE_UPDATE_API=http://127.0.0.1:port/latest` | ask a test server on this PC for the latest release |
| `STARLITE_UPDATE_INSTALL=1` | press Install in the update window once |
| `STARLITE_GLASS=0\|1\|2` | force Simple / Glass / Liquid glass (doesn't change the saved setting) |
| `STARLITE_SETTINGS_TAB=0..11` | open that Settings page (0 Appearance, 1 Layout, 2 Mini player, 3 Playback, 4 Equalizer, 5 Effects, 6–10 Navidrome, Jellyfin, YouTube, Discord, Last.fm, 11 General) |
| `STARLITE_SETTINGS_SCROLL=<pt>` | start the Settings page scrolled down |
| `STARLITE_FONT_MENU=1` | open the font list once |
| `STARLITE_LOOK=<name>`, `STARLITE_LOOK_SAVE=<name>` | switch to / save a look at start |
| `STARLITE_EQ_SEL=0..10` | the equalizer slider picked at start |
| `STARLITE_PAGE=jellyfin`, `STARLITE_JF_CONNECT=url\|user\|password` | like the Navidrome ones |
| `STARLITE_LASTFM_API=<url>` | talk to a test server instead of Last.fm |
| `STARLITE_LASTFM_CONNECT=key\|secret` | start connecting to Last.fm at once |
| `STARLITE_LASTFM_FAST=1` | scrobble after five seconds |

## Windows and Linux

Most of Starlite is the same on both. What talks to the system lives in
`src/win.rs` (Windows) and `src/unix.rs` (Linux, used as `crate::win`
through `#[path]` in main.rs), with the same functions:

- **Linux runs on X11** (XWayland on a Wayland desktop: main.rs drops
  `WAYLAND_DISPLAY`), because Wayland lets no app place a window (the mini
  player at the screen's edge) or put another window inside its own (the
  YouTube player).
- What Windows asks the system for (focus, the monitor's size, the mouse
  over the mini player) Starlite tells `unix.rs` each frame (`note_*`).
  Without panel sizes, the work area is the whole monitor, and the mini
  player grows in place instead of sliding past the edge.
- **YouTube on Linux:** WebKitGTK through wry, as a child of the X11 window.
  GTK gets its turn every frame while the player exists. The player page is
  served from `http://127.0.0.1:<port>` (a real origin for YouTube's embed
  rules).
- Fonts: fontconfig (`fc-list`); the UI font is the first of Inter, Noto
  Sans, Cantarell, Ubuntu, Open Sans, DejaVu Sans, Liberation Sans that's
  there. Data in `$XDG_DATA_HOME/Starlite`, downloads in `~/Music/Starlite`.
- Frames: vsync on Linux; on Windows each frame waits for the screen's
  refresh instead (see *Frame pacing*). Glass that blurs the desktop is
  Windows-only.
- **Updates on Linux:** the release's `Starlite-<v>-linux-x86_64.tar.gz`,
  checked against `SHA-256 (Linux):` in the notes (the Windows line,
  `SHA-256:`, must come first: older versions take the first one).

## Releases

- **Windows zip:** `cargo build --release` with `--remap-path-prefix` for the
  source, cargo and home folders and `/PDBALTPATH:%_PDB%`, so no personal
  paths end up in the exe; zipped with `THIRD-PARTY-NOTICES.txt`.
- **Windows setup** (`setup/`, its own crate, not part of the app's build):
  `STARLITE_SETUP_PAYLOAD=<folder with Starlite.exe and THIRD-PARTY-NOTICES.txt>
  cargo build --release` in `setup/`. It installs per user into
  `%LOCALAPPDATA%\Programs\Starlite` (Start menu and desktop shortcuts, an
  entry in Settings → Apps under HKCU), copies itself as `Uninstall
  Starlite.exe`, and also runs without a window: `--silent [--dir <folder>]
  [--no-desktop]`, `--uninstall --silent [--delete-data]`.
- **Linux:** built in an Ubuntu 22.04 environment (glibc 2.35, so it runs on
  older systems too) with the GTK 3, WebKitGTK 4.1 and ALSA dev packages,
  stripped, packed as `starlite`, `starlite.png`, `THIRD-PARTY-NOTICES.txt`.
  `install.sh` (in the repo, served raw) installs the system packages, the
  newest release (checked against its checksum) into `~/.local/lib/starlite`,
  `~/.local/bin/starlite` and an app menu entry.
- The notes need `SHA-256: ` + the zip's hash, then `SHA-256 (Linux): ` + the
  tar.gz's hash (both in backticks).

## Planned

Downloads from Navidrome / Jellyfin, albums and artists from the servers.
