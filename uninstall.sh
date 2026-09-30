#!/bin/sh
# Remove TurboDM (your download list and settings are kept unless you pass --purge).
set -eu
DATA="${XDG_DATA_HOME:-$HOME/.local/share}"
CONF="${XDG_CONFIG_HOME:-$HOME/.config}"
HOST=com.xsobhi.turbodm
rm -f "$HOME/.local/bin/turbodm" "$DATA/applications/turbodm.desktop" \
      "$DATA/icons/hicolor/scalable/apps/turbodm.svg" "$HOME/.mozilla/native-messaging-hosts/$HOST.json"
for dir in google-chrome google-chrome-beta chromium BraveSoftware/Brave-Browser microsoft-edge vivaldi; do
    rm -f "$CONF/$dir/NativeMessagingHosts/$HOST.json"
done
rm -rf "$DATA/turbodm/extension"
if [ "${1:-}" = "--purge" ]; then
    rm -rf "$DATA/turbodm" "$CONF/turbodm"
fi
echo "TurboDM removed. Remove the browser extension from your browser's extensions page."
