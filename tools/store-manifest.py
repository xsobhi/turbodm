"""The stores' copy of the extension's manifest (tools/build-extension.sh): without the "key"
that fixes the ID of the copy loaded by hand, and with the video scripts left out of YouTube,
since the Chrome Web Store doesn't allow downloading YouTube's videos (background.js and
popup.js leave YouTube out for the store's copy too)."""
import json
import sys

YOUTUBE = ["*://*.youtube.com/*", "*://youtube.com/*", "*://*.youtube-nocookie.com/*", "*://youtu.be/*"]

path = sys.argv[1]
with open(path, encoding="utf-8") as f:
    manifest = json.load(f)
manifest.pop("key", None)
for scripts in manifest.get("content_scripts", []):
    scripts["exclude_matches"] = YOUTUBE
with open(path, "w", encoding="utf-8", newline="\n") as f:
    json.dump(manifest, f, indent=2)
    f.write("\n")
