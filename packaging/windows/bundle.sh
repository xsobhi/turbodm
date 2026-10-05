#!/bin/bash
# Gather turbodm.exe with the GTK runtime it needs into dist/windows/TurboDM (run in an MSYS2
# shell after `cargo build --release`). Layout: bin/ (exe + DLLs), lib/ (image loaders),
# share/ (settings schemas, icons) - where GTK looks relative to the exe.
set -euo pipefail
cd "$(dirname "$0")/../.."
OUT=dist/windows/TurboDM
P="$MINGW_PREFIX"
rm -rf "$OUT"
mkdir -p "$OUT/bin" "$OUT/lib" "$OUT/share/glib-2.0/schemas" "$OUT/share/icons"

cp target/release/turbodm.exe "$OUT/bin/"
cp -r "$P/lib/gdk-pixbuf-2.0" "$OUT/lib/" # PNG/SVG loaders (symbolic icons are SVG)
find "$OUT/lib" -name "*.a" -delete

# every DLL the exe and the loaders link against, from the MSYS2 prefix
copy_deps() {
    ldd "$1" | awk '{print $3}' | grep -i "^${P}/bin/" | while read -r dll; do
        [ -f "$OUT/bin/$(basename "$dll")" ] || cp "$dll" "$OUT/bin/"
    done
}
copy_deps "$OUT/bin/turbodm.exe"
for loader in "$OUT"/lib/gdk-pixbuf-2.0/2.10.0/loaders/*.dll; do copy_deps "$loader"; done
cp "$P/bin/gdbus.exe" "$OUT/bin/" 2>/dev/null || true # GApplication / notifications

cp "$P"/share/glib-2.0/schemas/org.gtk.gtk4.*.xml "$OUT/share/glib-2.0/schemas/" 2>/dev/null || true
glib-compile-schemas "$OUT/share/glib-2.0/schemas"
cp -r "$P/share/icons/Adwaita" "$P/share/icons/hicolor" "$OUT/share/icons/"
install -Dm644 data/turbodm.svg "$OUT/share/icons/hicolor/scalable/apps/turbodm.svg"
gtk4-update-icon-cache -q -t -f "$OUT/share/icons/hicolor" || true

mkdir -p "$OUT/extension"
cp -r dist/chrome dist/firefox "$OUT/extension/"
[ -f dist/turbodm-firefox.xpi ] && cp dist/turbodm-firefox.xpi "$OUT/extension/"
cp README.md LICENSE "$OUT/"
echo "bundled into $OUT ($(du -sh "$OUT" | cut -f1))"
