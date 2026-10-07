#!/bin/sh
# Assemble the browser extension into dist/firefox, dist/chrome and dist/turbodm-firefox.xpi
set -eu
cd "$(dirname "$0")/.."
for browser in firefox chrome; do
    out="dist/$browser"
    rm -rf "$out"
    mkdir -p "$out/icons"
    cp extension/common/* "$out/"
    cp extension/icons/*.png "$out/icons/"
    cp "extension/$browser/"* "$out/" # manifest + browser-only scripts
done
# Firefox only installs extensions signed by Mozilla: ship the signed copy of this version
# (packaging/firefox/, made with `web-ext sign --channel=unlisted`) when there is one
version=$(sed -n 's/.*"version": *"\([^"]*\)".*/\1/p' extension/firefox/manifest.json | head -1)
rm -f dist/turbodm-firefox.xpi
if [ -f "packaging/firefox/turbodm-$version.xpi" ]; then
    cp "packaging/firefox/turbodm-$version.xpi" dist/turbodm-firefox.xpi
elif command -v zip >/dev/null 2>&1; then
    (cd dist/firefox && zip -qr ../turbodm-firefox.xpi .) # unsigned: for about:debugging only
fi
echo "extension built in dist/ (chrome ID: edlkglikdjabdhlailjmdopnlocdegbe)"
