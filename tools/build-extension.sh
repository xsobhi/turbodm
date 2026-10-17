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
signed=$(ls packaging/firefox/turbodm-*.xpi 2>/dev/null | sort -V | tail -1)
if [ -f "packaging/firefox/turbodm-$version.xpi" ]; then
    cp "packaging/firefox/turbodm-$version.xpi" dist/turbodm-firefox.xpi
elif [ -n "$signed" ]; then
    # this version isn't signed yet: Firefox gets the last signed one meanwhile (it won't
    # install an unsigned one)
    echo "warning: no signed Firefox extension $version, shipping $signed" >&2
    cp "$signed" dist/turbodm-firefox.xpi
elif command -v zip >/dev/null 2>&1; then
    (cd dist/firefox && zip -qr ../turbodm-firefox.xpi .) # unsigned: for about:debugging only
fi
# The stores give the extension an ID of their own: their copy is without the "key" that fixes
# the ID of the one loaded by hand, and leaves YouTube out (upload it as docs/extension-stores.md
# says)
rm -rf dist/store dist/turbodm-extension-*-for-stores.zip
cp -r dist/chrome dist/store
python3 tools/store-manifest.py dist/store/manifest.json 2>/dev/null || python tools/store-manifest.py dist/store/manifest.json
if command -v zip >/dev/null 2>&1; then
    (cd dist/store && zip -qr "../turbodm-extension-$version-for-stores.zip" .)
fi
echo "extension built in dist/ (chrome ID: edlkglikdjabdhlailjmdopnlocdegbe)"
