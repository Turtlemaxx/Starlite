<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="docs/logo-white.png">
    <img src="docs/logo-black.png" width="96" alt="Starlite">
  </picture>
</p>

<h1 align="center">Starlite</h1>

<p align="center">
  Modern media player alternative.<br>
  <a href="https://github.com/Turtlemaxx/Starlite/releases/latest"><b>Download for Windows</b></a> · <a href="#linux"><b>Linux</b></a>
</p>

<p align="center">
  <img src="docs/screenshots/player-dark.png" width="820" alt="Starlite playing a playlist">
</p>

Starlite is mainly made for your **local music** and your **Navidrome** server, but it can also be connected to **YouTube Music**.

## Features

**Full screen.** Big cover on a background made from the cover's colours. Lyrics show beside the cover and glide along with the song, lighting up word by word. They come from [lrclib.net](https://lrclib.net) (can be turned off in Settings → Lyrics), the file's tags or a `.lrc` next to it. Click a line to jump there.

<p align="center"><img src="docs/screenshots/fullscreen.png" width="720" alt="Full screen Now Playing"></p>

**CD shelf.** Turn it on in Settings → Appearance and your playlists become CD cases on a bookshelf. Hover one and it slides out, turns to face you and tilts with your mouse, the disc slides out and a few seconds of the song play (its chorus when the lyrics show one).

**Search.** Every playlist has a search bar (song, artist or album; Ctrl+F).

**Sound.** Gapless playback, or a crossfade of up to 12 seconds. A 10-band equalizer with preamp, where each band's gain and width can be fine-tuned. Effects you can switch on one by one: bass, clarity, stereo widening, crossfeed, headphone surround, reverb, compressor, limiter, and output volume and balance. Speed and bass are a click away in the player bar. All of it works on your own files and server songs (YouTube plays in YouTube's player, untouched).

**Mini player.** Switch to another app (or minimize Starlite) and a small card slides out of the screen edge with the cover and song. Hover it for controls. You can move it, resize it, pick its buttons, show it only while music is playing, or turn it off in Settings.

<p align="center"><img src="docs/screenshots/mini-player.png" width="357" alt="Mini player"></p>

**Navidrome and Jellyfin.** Connect your server and play its playlists. Right-click one to add it to your playlists in the sidebar. Songs come as they are stored, or converted by the server to MP3, AAC or FLAC at the bitrate you pick. Only a login token is saved, never your password.

<p align="center"><img src="docs/screenshots/navidrome.png" width="720" alt="Navidrome playlists"></p>

**Discord status and Last.fm.** Shows what you're listening to on your Discord profile, and you choose what it shows: cover, song, artist, album, time. Scrobbles to Last.fm with your own API key.

<p align="center"><img src="docs/screenshots/settings-discord.png" width="720" alt="Discord settings"></p>

**Themes and looks.** Dark, Light, Cherry Blossom, Stars, Bubbles (glass bubbles that rise and pop), or your own colours, picture and CSS. Panels come in three styles: Simple (flat and solid, the default), Glass (see-through with the background blurred, readable on any background; on Light, Dark or your own colour the desktop behind the window shows through, blurred, like Windows' own apps) and Liquid glass. Settings → Appearance also changes the accent (also from the playing cover), background, corners, opacity, blur, edges, shadows, font, text and icon size; Settings → Layout the song rows, sidebar and player bar buttons; Settings → General the click sound (also your own .wav). Save any combination as a look and switch between your looks with one click.

**And also:** your own playlists (start one from an .m3u file if you like), Liked Music, local files with their real tags and covers, optional YouTube Music charts (played in YouTube's official player), shuffle and loop.

**Your sidebar.** The pencil under Settings lets you rearrange the sidebar: drag entries (Downloaded Music, your playlists, the servers…) where you want them, make your own categories, and pin things to the bottom.

**Updates.** Starlite tells you when a new version is out and installs it with one click (can be switched off in Settings → General).

## Install

### Windows

Download `Starlite-Setup-x.y.z.exe` from [Releases](https://github.com/Turtlemaxx/Starlite/releases/latest) and run it. It installs Starlite for you (no admin rights needed) with Start menu and desktop shortcuts; remove it in Settings → Apps. Or take the `.zip`, unzip it anywhere and run `Starlite.exe`.

Needs 64-bit Windows 10 or 11. YouTube playback uses Microsoft Edge WebView2, which Windows 11 already has.

### Linux

```
curl -fsSL https://raw.githubusercontent.com/Turtlemaxx/Starlite/main/install.sh | bash
```

Installs what Starlite needs from your distribution (Ubuntu/Debian, Fedora, Arch, openSUSE; your password is asked for that), then Starlite into your home folder, with an app menu entry and the `starlite` command. It updates itself like on Windows. Remove it with `... | bash -s -- --uninstall`.

Needs x86_64 and glibc 2.35 or newer (Ubuntu 22.04, Debian 12, Fedora 36 and later). Starlite runs on X11, or XWayland on a Wayland desktop, so the mini player can sit at the screen's edge. Glass on the Light and Dark themes doesn't blur the desktop on Linux (it does over the other themes).

## Setup

- **Your music:** put MP3, FLAC, WAV, OGG or M4A files in the `Downloaded songs` folder next to the app (Linux: `~/Music/Starlite`).
- **YouTube:** Settings → YouTube → paste a free [YouTube Data API key](https://console.cloud.google.com/apis/library/youtube.googleapis.com).
- **Navidrome / Jellyfin:** the first start asks for them; later it's Settings → Navidrome / Jellyfin → server address, username, password. Connected servers appear in the sidebar.
- **Last.fm:** create an API account at [last.fm/api/account/create](https://www.last.fm/api/account/create), paste its API key and shared secret in Settings → Last.fm, press Connect and allow Starlite in your browser.
- **Discord:** create an application named "Starlite" in the [Discord Developer Portal](https://discord.com/developers/applications), paste its Application ID in Settings → Discord, and upload the Starlite icon (the **Save icon…** button) under Rich Presence → Art Assets as `starlite`.

Settings, playlists and logins are stored in `%APPDATA%\Starlite` (Linux: `~/.local/share/Starlite`).

## Build from source

```
cargo build --release
```

Needs Rust (MSVC toolchain) on Windows. On Linux also the development packages of GTK 3, WebKitGTK 4.1 and ALSA (Ubuntu: `libgtk-3-dev libwebkit2gtk-4.1-dev libasound2-dev libxdo-dev`). The Windows setup is its own small program in `setup/`. Notes on how it's all built are in [docs/DEVELOPMENT.md](docs/DEVELOPMENT.md).

## Credits

Icons by [Lucide](https://lucide.dev) (ISC license, see `assets/icons/LICENSE`). The YouTube, Discord, Last.fm, Jellyfin and Navidrome logos are trademarks of their owners and only mark those services (sources in `assets/icons/BRANDS.md`); Starlite is not affiliated with them.
