#!/bin/sh
# Build and install TurboDM for the current user (no root needed, nothing autostarts).
set -eu
cd "$(dirname "$0")"
[ -f "$HOME/.cargo/env" ] && . "$HOME/.cargo/env" # rustup installs outside PATH for scripts
command -v cargo >/dev/null 2>&1 || { echo "Rust is needed: https://rustup.rs"; exit 1; }
pkg-config --exists gtk4 || { echo "GTK4 development files are needed: sudo apt install libgtk-4-dev"; exit 1; }

cargo build --release
BIN="$HOME/.local/bin/turbodm"
DATA="${XDG_DATA_HOME:-$HOME/.local/share}"
CONF="${XDG_CONFIG_HOME:-$HOME/.config}"

install -Dm755 target/release/turbodm "$BIN"
install -Dm644 data/turbodm.svg "$DATA/icons/hicolor/scalable/apps/turbodm.svg"
mkdir -p "$DATA/applications"
sed "s|@BIN@|$BIN|" data/turbodm.desktop > "$DATA/applications/turbodm.desktop"
"$BIN" --register # native-messaging host: lets only the TurboDM extension start/talk to the app

tools/build-extension.sh
rm -rf "$DATA/turbodm/extension/firefox" "$DATA/turbodm/extension/chrome" # keeps signed .xpi files
mkdir -p "$DATA/turbodm/extension"
cp -r dist/firefox dist/chrome dist/turbodm-firefox.xpi "$DATA/turbodm/extension/"
command -v update-desktop-database >/dev/null && update-desktop-database "$DATA/applications" || true

cat <<MSG

TurboDM installed: $BIN  (in your app menu as "TurboDM")

Browser extension:
  Chrome / Brave / Chromium: open chrome://extensions (brave://extensions), enable
    "Developer mode", "Load unpacked" -> $DATA/turbodm/extension/chrome
  Firefox: about:debugging#/runtime/this-firefox -> "Load Temporary Add-on" ->
    $DATA/turbodm/extension/firefox/manifest.json   (see README for permanent install)
MSG
