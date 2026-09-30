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
HOST=com.xsobhi.turbodm
FIREFOX_ID=turbodm@xsobhi.github.io
CHROME_ID=edlkglikdjabdhlailjmdopnlocdegbe

install -Dm755 target/release/turbodm "$BIN"
install -Dm644 data/turbodm.svg "$DATA/icons/hicolor/scalable/apps/turbodm.svg"
mkdir -p "$DATA/applications"
sed "s|@BIN@|$BIN|" data/turbodm.desktop > "$DATA/applications/turbodm.desktop"

# Native-messaging host: lets only the TurboDM extension start/talk to the app
firefox_host() {
    mkdir -p "$1"
    printf '{"name":"%s","description":"TurboDM","path":"%s","type":"stdio","allowed_extensions":["%s"]}\n' \
        "$HOST" "$BIN" "$FIREFOX_ID" > "$1/$HOST.json"
}
chromium_host() {
    mkdir -p "$1"
    printf '{"name":"%s","description":"TurboDM","path":"%s","type":"stdio","allowed_origins":["chrome-extension://%s/"]}\n' \
        "$HOST" "$BIN" "$CHROME_ID" > "$1/$HOST.json"
}
has() { command -v "$1" >/dev/null 2>&1; }
if has firefox || [ -d "$HOME/.mozilla" ]; then firefox_host "$HOME/.mozilla/native-messaging-hosts"; fi
# "config dir:browser command" - set up browsers that are installed or have a profile
for pair in google-chrome:google-chrome google-chrome-beta:google-chrome-beta chromium:chromium \
            chromium:chromium-browser BraveSoftware/Brave-Browser:brave-browser \
            microsoft-edge:microsoft-edge vivaldi:vivaldi; do
    dir=${pair%%:*}; cmd=${pair#*:}
    if has "$cmd" || [ -d "$CONF/$dir" ]; then chromium_host "$CONF/$dir/NativeMessagingHosts"; fi
done

tools/build-extension.sh
rm -rf "$DATA/turbodm/extension"
mkdir -p "$DATA/turbodm/extension"
cp -r dist/firefox dist/chrome "$DATA/turbodm/extension/"
command -v update-desktop-database >/dev/null && update-desktop-database "$DATA/applications" || true

cat <<MSG

TurboDM installed: $BIN  (in your app menu as "TurboDM")

Browser extension:
  Chrome / Brave / Chromium: open chrome://extensions (brave://extensions), enable
    "Developer mode", "Load unpacked" -> $DATA/turbodm/extension/chrome
  Firefox: about:debugging#/runtime/this-firefox -> "Load Temporary Add-on" ->
    $DATA/turbodm/extension/firefox/manifest.json   (see README for permanent install)
MSG
