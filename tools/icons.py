#!/usr/bin/env python3
"""TurboDM's icons, in one style: a rounded tile in a colour gradient, with a soft shine at the
top and a shadow under it, carrying a white symbol (Fluent UI System Icons, filled: MIT, see
data/icons/symbols/LICENSE). Writes:

  data/icons/<name>.svg, .png (64 px)    toolbar and sidebar icons
  data/turbodm.svg, data/turbodm.ico     the app's icon
  extension/icons/icon-<size>.png        the browser extension's

Needs resvg (`cargo install resvg`) on the PATH. Run from anywhere: python3 tools/icons.py
"""
import re
import struct
import subprocess
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
ICONS = ROOT / "data/icons"

# (light, dark) of each colour, Mint's green first
COLOURS = {
    "green": ("#5fd17a", "#26914a"), "blue": ("#62a8f7", "#2369d1"), "amber": ("#ffcb52", "#e88b0c"),
    "red": ("#ff7f74", "#d23838"), "purple": ("#b590f3", "#7448d3"), "teal": ("#52d3c7", "#178f88"),
    "pink": ("#f893b6", "#d43f79"), "orange": ("#ffa65e", "#e35e16"), "brown": ("#d9a56d", "#956330"),
    "slate": ("#a2abb6", "#56606c"), "yellow": ("#ffd45e", "#e8940c"),
}

# icon: (colour, symbol)
TILES = {
    "add": ("green", "add"), "resume": ("green", "play"), "pause": ("amber", "pause"),
    "resume-all": ("teal", "fast_forward"), "pause-all": ("orange", "stop"), "delete": ("red", "delete"),
    "clear": ("purple", "broom"), "folder": ("yellow", "folder_open"), "options": ("slate", "settings"),
    "all": ("green", "arrow_download"), "downloading": ("blue", "arrow_down"), "unfinished": ("amber", "clock"),
    "completed": ("green", "checkmark"), "video": ("purple", "video"), "music": ("pink", "music_note_2"),
    "documents": ("blue", "document_text"), "compressed": ("brown", "folder_zip"), "programs": ("teal", "apps"),
    "images": ("orange", "image"), "other": ("slate", "document"),
}


# the app's own symbol: a bold arrow into a tray, with two "turbo" lines (24 × 24, like the others)
BOLD_ARROW = """<g fill="none" stroke="#fff" stroke-linecap="round" stroke-linejoin="round">
      <path d="M13.5 3.5v11.5M8.5 10.5l5 5 5-5" stroke-width="2.6"/>
      <path d="M6.5 20h14" stroke-width="2.4"/>
      <path d="M3.5 7.5h3M2.5 11h3.2" stroke-width="1.7" opacity="0.8"/>
    </g>"""


def symbol(name):
    """The symbol's drawing (24 × 24), in white."""
    svg = (ICONS / "symbols" / f"{name}.svg").read_text()
    body = re.search(r"<svg[^>]*>(.*)</svg>", svg, re.S).group(1)
    return re.sub(r'fill="#[0-9a-fA-F]+"', 'fill="#fff"', body).strip()


def tile(colour, glyph, size=64, inset=5, radius=13):
    """A tile `size` square: the tile `inset` from the edges (room for its shadow)."""
    light, dark = COLOURS[colour]
    side = size - 2 * inset
    top = inset - 1  # a little higher: the shadow falls below
    scale = side * 0.6 / 24  # the symbol takes 60% of the tile
    offset = (size - 24 * scale) / 2
    return f'''<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {size} {size}">
  <defs>
    <linearGradient id="fill" x1="0" y1="0" x2="0" y2="1">
      <stop offset="0" stop-color="{light}"/><stop offset="1" stop-color="{dark}"/>
    </linearGradient>
    <linearGradient id="shine" x1="0" y1="0" x2="0" y2="1">
      <stop offset="0" stop-color="#fff" stop-opacity="0.42"/><stop offset="1" stop-color="#fff" stop-opacity="0"/>
    </linearGradient>
    <clipPath id="shape"><rect x="{inset}" y="{top}" width="{side}" height="{side}" rx="{radius}"/></clipPath>
    <filter id="shadow" x="-30%" y="-30%" width="160%" height="170%">
      <feDropShadow dx="0" dy="{size / 40:.2f}" stdDeviation="{size / 40:.2f}" flood-color="#000" flood-opacity="0.32"/>
    </filter>
    <filter id="lift" x="-30%" y="-30%" width="160%" height="170%">
      <feDropShadow dx="0" dy="{size / 64:.2f}" stdDeviation="{size / 80:.2f}" flood-color="#000" flood-opacity="0.28"/>
    </filter>
  </defs>
  <rect x="{inset}" y="{top}" width="{side}" height="{side}" rx="{radius}" fill="url(#fill)" filter="url(#shadow)"/>
  <rect x="{inset}" y="{top}" width="{side}" height="{side / 2:.1f}" fill="url(#shine)" clip-path="url(#shape)"/>
  <rect x="{inset + 0.5}" y="{top + 0.5}" width="{side - 1}" height="{side - 1}" rx="{radius - 0.5}" fill="none"
        stroke="{dark}" stroke-opacity="0.45"/>
  <g transform="translate({offset:.2f} {offset - 1:.2f}) scale({scale:.4f})" filter="url(#lift)">
    {glyph}
  </g>
</svg>
'''


def render(svg_path, png_path, size):
    subprocess.run(["resvg", "-w", str(size), "-h", str(size), str(svg_path), str(png_path)], check=True)


def ico(pngs, path):
    """An .ico of PNG images (Windows Vista and later read those at every size)."""
    header = struct.pack("<HHH", 0, 1, len(pngs))
    entries, data, offset = b"", b"", 6 + 16 * len(pngs)
    for size, png in pngs:
        byte = 0 if size >= 256 else size
        entries += struct.pack("<BBBBHHII", byte, byte, 0, 0, 1, 32, len(png), offset)
        data += png
        offset += len(png)
    path.write_bytes(header + entries + data)


def main():
    for name, (colour, sym) in TILES.items():
        svg = ICONS / f"{name}.svg"
        svg.write_text(tile(colour, symbol(sym)))
        render(svg, ICONS / f"{name}.png", 64)
    # the app's icon: the downloads' green tile, bigger and rounder
    app = ROOT / "data/turbodm.svg"
    app.write_text(tile("green", BOLD_ARROW, size=128, inset=8, radius=28))
    tmp = ROOT / "target/icon-render"
    tmp.mkdir(parents=True, exist_ok=True)
    pngs = []
    for size in (16, 20, 24, 32, 40, 48, 64, 96, 128, 256):
        out = tmp / f"{size}.png"
        render(app, out, size)
        pngs.append((size, out.read_bytes()))
    ico(pngs, ROOT / "data/turbodm.ico")
    for size in (16, 32, 48, 128):
        render(app, ROOT / f"extension/icons/icon-{size}.png", size)


if __name__ == "__main__":
    main()
