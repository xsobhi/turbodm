#!/bin/sh
# Remove TurboDM (your download list and settings are kept unless you pass --purge).
set -eu
DATA="${XDG_DATA_HOME:-$HOME/.local/share}"
CONF="${XDG_CONFIG_HOME:-$HOME/.config}"
BIN="$HOME/.local/bin/turbodm"
[ -x "$BIN" ] && "$BIN" --unregister
rm -f "$BIN" "$DATA/applications/turbodm.desktop" "$DATA/icons/hicolor/scalable/apps/turbodm.svg"
rm -rf "$DATA/turbodm/extension"
if [ "${1:-}" = "--purge" ]; then
    rm -rf "$DATA/turbodm" "$CONF/turbodm"
fi
echo "TurboDM removed. Remove the browser extension from your browser's extensions page."
