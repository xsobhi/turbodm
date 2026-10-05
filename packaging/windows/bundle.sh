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

# Every DLL the exe and the image loaders import, read from the files themselves (not ldd, which
# finds driver DLLs like vulkan-1.dll in the build machine's System32 and so skips them).
# Anything MSYS2 provides is bundled; the rest must be part of Windows itself.
imports() { objdump -p "$1" | awk '/DLL Name:/ {print $3}'; }
lower() { tr '[:upper:]' '[:lower:]'; }
cp "$P/bin/gdbus.exe" "$OUT/bin/" # GApplication / notifications
queue=("$OUT/bin/turbodm.exe" "$OUT/bin/gdbus.exe" "$OUT"/lib/gdk-pixbuf-2.0/2.10.0/loaders/*.dll)
system=()
while [ ${#queue[@]} -gt 0 ]; do
    file=${queue[0]}; queue=("${queue[@]:1}")
    for dll in $(imports "$file"); do
        name=$(echo "$dll" | lower)
        [ -f "$OUT/bin/$name" ] && continue
        if [ -f "$P/bin/$name" ]; then
            cp "$P/bin/$name" "$OUT/bin/$name"
            queue+=("$OUT/bin/$name")
        else
            system+=("$name")
        fi
    done
done
# what's left must come with every Windows 10/11, whatever the hardware and drivers
missing=$(printf '%s\n' "${system[@]}" | sort -u | grep -v -x -E -f packaging/windows/system-dlls.txt || true)
if [ -n "$missing" ]; then
    echo "Not bundled and not part of Windows:" $missing >&2
    exit 1
fi

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
