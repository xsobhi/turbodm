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
if command -v zip >/dev/null 2>&1; then
    (cd dist/firefox && rm -f ../turbodm-firefox.xpi && zip -qr ../turbodm-firefox.xpi .)
fi
echo "extension built in dist/ (chrome ID: edlkglikdjabdhlailjmdopnlocdegbe)"
