<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="docs/logo-white.png">
    <img src="docs/logo-black.png" width="96" alt="Starlite">
  </picture>
</p>

<h1 align="center">Starlite</h1>

<p align="center">
  Modern media player alternative.<br>
  <a href="https://github.com/Turtlemaxx/Starlite/releases/latest"><b>Download for Windows</b></a>
</p>

<p align="center">
  <img src="docs/screenshots/player-dark.png" width="820" alt="Starlite playing a playlist">
</p>

Starlite is mainly made for your **local music** and your **Navidrome** server, but it can also be connected to **YouTube Music**.

## Features

**Full screen.** Big cover on a background made from the cover's colours. Songs with lyrics (in the file's tags or a `.lrc` next to it) show them beside the cover, following along with the song. Click a line to jump there.

<p align="center"><img src="docs/screenshots/fullscreen.png" width="720" alt="Full screen Now Playing"></p>

**Mini player.** Minimize Starlite and a small card slides out of the screen edge with the cover and song. Hover it for controls. You can move it, resize it, pick its buttons, or turn it off in Settings.

<p align="center"><img src="docs/screenshots/mini-player.png" width="357" alt="Mini player"></p>

**Navidrome.** Connect your Navidrome (or any Subsonic) server and play its playlists. Only a login token is saved, never your password.

<p align="center"><img src="docs/screenshots/navidrome.png" width="720" alt="Navidrome playlists"></p>

**Discord status.** Shows what you're listening to on your Discord profile, and you choose what it shows: cover, song, artist, album, time.

<p align="center"><img src="docs/screenshots/settings-discord.png" width="720" alt="Discord settings"></p>

**And also:** your own playlists, Liked Music, local files with their real tags and covers, optional YouTube Music charts (played in YouTube's official player), shuffle, loop, and four themes: Dark, Light, Cherry Blossom and Stars.

## Install

1. Download `Starlite-0.1.1-windows-x64.zip` from [Releases](https://github.com/Turtlemaxx/Starlite/releases/latest).
2. Unzip it anywhere and run `Starlite.exe`.

Needs 64-bit Windows 10 or 11. YouTube playback uses Microsoft Edge WebView2, which Windows 11 already has.

## Setup

- **Your music:** put MP3, FLAC, WAV, OGG or M4A files in the `Downloaded songs` folder next to the app.
- **YouTube:** Settings → YouTube → paste a free [YouTube Data API key](https://console.cloud.google.com/apis/library/youtube.googleapis.com).
- **Navidrome:** Navidrome in the sidebar → server address, username, password.
- **Discord:** create an application named "Starlite" in the [Discord Developer Portal](https://discord.com/developers/applications), paste its Application ID in Settings → Discord, and upload the Starlite icon (the **Save icon…** button) under Rich Presence → Art Assets as `starlite`.

Settings, playlists and logins are stored in `%APPDATA%\Starlite`.

## Build from source

```
cargo build --release
```

Needs Rust (MSVC toolchain) on Windows. Notes on how it's built are in [docs/DEVELOPMENT.md](docs/DEVELOPMENT.md).

## Credits

Icons by [Lucide](https://lucide.dev) (ISC license, see `assets/icons/LICENSE`).
