#!/usr/bin/env python3
"""Windows toolbar and sidebar icons (data/windows/*.ico) from the Fluent UI System Icons in
src/ui/style/icons, each in one colour, like Visual Studio's. Needs GdkPixbuf (with its SVG
loader) and Pillow: python3 tools/win-icons.py"""
import io, os, re
import gi
gi.require_version("GdkPixbuf", "2.0")
from gi.repository import GdkPixbuf
from PIL import Image

SRC, OUT = "src/ui/style/icons", "data/windows"
GREEN, RED, ORANGE, BLUE, GOLD, GREY, PURPLE, TEAL, PINK = (
    "#107C10", "#C42B1C", "#CA5010", "#0063B1", "#B58A00", "#505050", "#8764B8", "#00827F", "#C30052")
ICONS = {  # name: (svg, colour)
    "add": ("add", GREEN), "resume": ("play", GREEN), "pause": ("pause", ORANGE),
    "resume-all": ("next", GREEN), "pause-all": ("stop", RED), "delete": ("delete", RED),
    "clear": ("broom", PURPLE), "folder": ("folder_open", GOLD), "options": ("settings", GREY),
    "all": ("arrow_download", BLUE), "active": ("play", GREEN), "unfinished": ("pause", ORANGE),
    "completed": ("checkmark", GREEN), "video": ("video", PURPLE), "music": ("music_note_2", PINK),
    "documents": ("document_text", BLUE), "compressed": ("folder_zip", GOLD), "programs": ("apps", GREY),
    "images": ("image", TEAL), "other": ("document", GREY),
}
SIZES = [16, 20, 24, 32, 40, 48, 64]

os.makedirs(OUT, exist_ok=True)
for name, (svg, colour) in ICONS.items():
    text = re.sub(r'fill="#[0-9A-Fa-f]{6}"', f'fill="{colour}"', open(f"{SRC}/{svg}.svg").read())
    images = []
    for size in SIZES:
        loader = GdkPixbuf.PixbufLoader.new_with_type("svg")
        loader.set_size(size, size)
        loader.write(text.encode())
        loader.close()
        ok, png = loader.get_pixbuf().save_to_bufferv("png", [], [])
        images.append(Image.open(io.BytesIO(png)).convert("RGBA"))
    images[-1].save(f"{OUT}/{name}.ico", sizes=[(s, s) for s in SIZES], append_images=images[:-1])
print("wrote", len(ICONS), "icons to", OUT)
