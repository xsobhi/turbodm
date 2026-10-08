#!/bin/bash
# Gather what the Windows installer and portable zip ship into dist/windows/TurboDM (after
# `cargo build --release` and tools/build-extension.sh): bin/turbodm.exe, which uses only
# Windows' own libraries, and the browser extension.
set -euo pipefail
cd "$(dirname "$0")/../.."
OUT=dist/windows/TurboDM
EXE=${1:-target/release/turbodm.exe}
rm -rf "$OUT"
mkdir -p "$OUT/bin" "$OUT/extension"
cp "$EXE" "$OUT/bin/"
cp -r dist/chrome dist/firefox "$OUT/extension/"
[ -f dist/turbodm-firefox.xpi ] && cp dist/turbodm-firefox.xpi "$OUT/extension/"
cp README.md LICENSE "$OUT/"
echo "bundled into $OUT ($(du -sh "$OUT" | cut -f1))"
