#!/usr/bin/env bash
# Starlite for Linux: one-line install
#
#   curl -fsSL https://raw.githubusercontent.com/Turtlemaxx/Starlite/main/install.sh | bash
#
# Installs what Starlite needs from your distribution (GTK, WebKitGTK for the
# YouTube player, GStreamer codecs, ALSA), then the newest Starlite release
# into your home folder: ~/.local/lib/starlite, the `starlite` command in
# ~/.local/bin and an entry in your app menu. Starlite updates itself there.
#
#   ... | bash -s -- --uninstall      removes it again (your settings stay in
#                                     ~/.local/share/Starlite)
#   STARLITE_NO_DEPS=1 ... | bash     skips the system packages
set -euo pipefail

REPO="Turtlemaxx/Starlite"
APP_DIR="$HOME/.local/lib/starlite"
BIN_DIR="$HOME/.local/bin"
MENU_DIR="${XDG_DATA_HOME:-$HOME/.local/share}/applications"

say() { printf '\033[1m%s\033[0m\n' "$*"; }
fail() { printf '\033[31m%s\033[0m\n' "$*" >&2; exit 1; }

if [ "${1:-}" = "--uninstall" ]; then
    rm -rf "$APP_DIR"
    rm -f "$BIN_DIR/starlite" "$MENU_DIR/starlite.desktop"
    command -v update-desktop-database >/dev/null 2>&1 && update-desktop-database "$MENU_DIR" >/dev/null 2>&1 || true
    say "Starlite was removed. Your settings and playlists are still in ~/.local/share/Starlite."
    exit 0
fi

[ "$(uname -s)" = "Linux" ] || fail "This installer is for Linux. On Windows, use the setup from the release page."
[ "$(uname -m)" = "x86_64" ] || fail "Starlite for Linux is built for x86_64 (this is $(uname -m))."
command -v curl >/dev/null 2>&1 || fail "Please install curl first."

# ── What Starlite needs from the system ──
install_deps() {
    local sudo=""
    [ "$(id -u)" -eq 0 ] || sudo="sudo"
    if command -v apt-get >/dev/null 2>&1; then
        # Ubuntu 24.04+ renamed the ALSA library.
        local alsa="libasound2"
        apt-cache show libasound2t64 >/dev/null 2>&1 && alsa="libasound2t64"
        $sudo apt-get update -q
        $sudo apt-get install -y -q libgtk-3-0 libwebkit2gtk-4.1-0 "$alsa" libxkbcommon-x11-0 libgl1 \
            gstreamer1.0-plugins-good gstreamer1.0-plugins-bad gstreamer1.0-libav fontconfig xdg-utils
    elif command -v dnf >/dev/null 2>&1; then
        $sudo dnf install -y gtk3 webkit2gtk4.1 alsa-lib libxkbcommon-x11 mesa-libGL \
            gstreamer1-plugins-good gstreamer1-plugins-bad-free fontconfig xdg-utils
    elif command -v pacman >/dev/null 2>&1; then
        $sudo pacman -S --needed --noconfirm gtk3 webkit2gtk-4.1 alsa-lib libxkbcommon-x11 mesa \
            gst-plugins-good gst-plugins-bad gst-libav fontconfig xdg-utils
    elif command -v zypper >/dev/null 2>&1; then
        $sudo zypper --non-interactive install gtk3 libwebkit2gtk-4_1-0 alsa libxkbcommon-x11-0 Mesa-libGL1 \
            gstreamer-plugins-good gstreamer-plugins-bad fontconfig xdg-utils
    else
        say "Couldn't tell your package manager. Starlite needs: GTK 3, WebKitGTK 4.1, ALSA, GStreamer (good, bad, libav), fontconfig."
    fi
}
if [ "${STARLITE_NO_DEPS:-}" != "1" ]; then
    say "Installing what Starlite needs (your password may be asked for)…"
    install_deps || say "Some system packages didn't install; Starlite may still run."
fi

# ── The newest release ──
say "Finding the newest Starlite…"
api="$(curl -fsSL -H 'Accept: application/vnd.github+json' "https://api.github.com/repos/$REPO/releases/latest")" || fail "Couldn't reach GitHub."
url="$(printf '%s' "$api" | grep -o '"browser_download_url": *"[^"]*-linux-x86_64\.tar\.gz"' | head -1 | sed 's/.*"\(https[^"]*\)"/\1/')"
[ -n "$url" ] || fail "The newest release has no Linux download."
case "$url" in "https://github.com/$REPO/releases/download/"*) ;; *) fail "Unexpected download address: $url" ;; esac
# "SHA-256 (Linux): `abc…`" in the release notes.
sha="$(printf '%s' "$api" | grep -o 'SHA-256 (Linux): *`[0-9A-Fa-f]\{64\}`' | head -1 | grep -o '[0-9A-Fa-f]\{64\}' | tr 'A-F' 'a-f')"
[ -n "$sha" ] || fail "The release notes have no Linux checksum."

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT
say "Downloading $(basename "$url")…"
curl -fL --progress-bar -o "$tmp/starlite.tar.gz" "$url"
got="$(sha256sum "$tmp/starlite.tar.gz" | cut -d' ' -f1)"
[ "$got" = "$sha" ] || fail "The download doesn't match the release's checksum, so it wasn't installed."

# ── Put it in place ──
mkdir -p "$APP_DIR" "$BIN_DIR" "$MENU_DIR"
tar -xzf "$tmp/starlite.tar.gz" -C "$tmp"
install -m 755 "$tmp/starlite" "$APP_DIR/starlite"
install -m 644 "$tmp/starlite.png" "$APP_DIR/starlite.png"
install -m 644 "$tmp/THIRD-PARTY-NOTICES.txt" "$APP_DIR/THIRD-PARTY-NOTICES.txt"
ln -sf "$APP_DIR/starlite" "$BIN_DIR/starlite"
cat > "$MENU_DIR/starlite.desktop" <<DESKTOP
[Desktop Entry]
Type=Application
Name=Starlite
GenericName=Music Player
Comment=Your own songs, Navidrome, Jellyfin and YouTube
Exec=$APP_DIR/starlite
Icon=$APP_DIR/starlite.png
Terminal=false
Categories=AudioVideo;Audio;Player;Music;
StartupWMClass=starlite
DESKTOP
command -v update-desktop-database >/dev/null 2>&1 && update-desktop-database "$MENU_DIR" >/dev/null 2>&1 || true

say "Starlite is installed. Open it from your app menu, or run: starlite"
case ":$PATH:" in *":$BIN_DIR:"*) ;; *) say "(Add $BIN_DIR to your PATH to use the starlite command.)" ;; esac
