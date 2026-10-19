"""Bakes the lab's felt texture into small raster tiles for the site.

The lab layers live SVG noise with mix-blend-mode and opacity. The site's
ground is one flat colour per theme, so each blend can be worked out ahead
of time: a plain semi-transparent tile in one colour gives the same pixels
with no filters, blend modes or extra layers at render time.
"""

import re
import subprocess
import tempfile
from pathlib import Path

HERE = Path(__file__).parent
OUT = HERE.parent / "images"
# librsvg doesn't stitch low-frequency noise into a seamless tile; Chromium does.
CHROMIUM = "chromium"
MAGICK = "magick"

PAPER, INK = (0xEF, 0xE8, 0xD8), (0x1B, 0x2A, 0x22)

# Pixels per tile edge. The soft layers have no fine detail, so they render
# small and CSS scales them back up to 1226 and 2402 px.
LAYERS = {"fibres": 307, "nap": 256, "shade": 160}

# AVIF quality. Fibres: the lowest that keeps the alpha's spread within ~5%
# of the PNG's (the faint dark fibres lose their variation below 95). The
# smooth layers need 100: below it the encoder lets the tile's edges drift
# apart and the repeat shows as straight lines.
QUALITY = {("light", "fibres"): 75, ("dark", "fibres"): 95}
DEFAULT_QUALITY = 100


def shown_as_srgb(c):
    """The on-screen value of a colour the lab's filter treated as linear light."""
    return 12.92 * c if c <= 0.0031308 else 1.055 * c ** (1 / 2.4) - 0.055


def multiplied_light_colour():
    return [p / 255 * shown_as_srgb(k / 255) for p, k in zip(PAPER, INK, strict=True)]


THEMES = {"light": (multiplied_light_colour(), 0.4), "dark": ([1.0, 1.0, 1.0], 0.06)}


def hex_colour(rgb):
    return "#" + "".join(f"{round(c * 255):02x}" for c in rgb)


def svg_size(svg):
    found = re.search(r"width='(\d+)'", svg)
    if found is None:
        raise ValueError("the tile's SVG has no width")
    return int(found.group(1))


def baked_svg(theme, layer):
    rgb, opacity = THEMES[theme]
    svg = (HERE / f"felt-{theme}-{layer}.svg").read_text()
    size = svg_size(svg)

    def bake(m):
        v = m.group(1).split()
        v[4], v[9], v[14] = (f"{c:.4f}" for c in rgb)
        v[15], v[19] = f"{float(v[15]) * opacity:.4f}", f"{float(v[19]) * opacity:.4f}"
        return f"values='{' '.join(v)}'"

    svg = re.sub(r"values='([^']+)'", bake, svg)
    svg = svg.replace(f"height='{size}'>", f"height='{size}' viewBox='0 0 {size} {size}'>", 1)
    return svg.replace("<filter id='f'", "<filter id='f' color-interpolation-filters='sRGB'", 1)


def render_native(theme, layer, png):
    """Draws the tile at its full size with Chromium, onto transparency."""
    svg = baked_svg(theme, layer)
    size = svg_size(svg)
    with tempfile.TemporaryDirectory() as tmp:
        page = Path(tmp, "tile.html")
        page.write_text(f'<html><body style="margin:0;background:transparent">{svg}</body></html>')
        subprocess.run(
            [
                CHROMIUM,
                "--headless",
                "--no-sandbox",
                "--disable-gpu",
                "--hide-scrollbars",
                "--default-background-color=00000000",
                "--force-device-scale-factor=1",
                f"--window-size={size},{size}",
                "--virtual-time-budget=5000",
                f"--screenshot={png}",
                page.as_uri(),
            ],
            check=True,
            capture_output=True,
        )
    return size


def bake(theme, layer):
    pixels = LAYERS[layer]
    quality = QUALITY.get((theme, layer), DEFAULT_QUALITY)
    rgb, _ = THEMES[theme]
    name = OUT / f"felt-{theme}-{layer}"
    with tempfile.TemporaryDirectory() as tmp:
        native = Path(tmp, "native.png")
        size = render_native(theme, layer, native)
        # Shrinking a 3x3 tiling and keeping the middle tile averages each edge
        # pixel with its wrapped-around neighbours, so the smaller tile still wraps.
        wrap_aware_shrink = [
            "-size",
            f"{3 * size}x{3 * size}",
            f"tile:{native}",
            "-resize",
            f"{3 * pixels}x{3 * pixels}!",
            "-crop",
            f"{pixels}x{pixels}+{pixels}+{pixels}",
            "+repage",
        ]
        one_colour = [
            "-alpha",
            "extract",
            "(",
            "-size",
            f"{pixels}x{pixels}",
            f"xc:{hex_colour(rgb)}",
            ")",
            "+swap",
            "-alpha",
            "off",
            "-compose",
            "CopyOpacity",
            "-composite",
        ]
        subprocess.run(
            [MAGICK, *wrap_aware_shrink, *one_colour, "-strip", "-define", "png:compression-level=9", f"{name}.png"],
            check=True,
        )
    subprocess.run([MAGICK, f"{name}.png", "-quality", str(quality), f"{name}.avif"], check=True)


if __name__ == "__main__":
    for theme in THEMES:
        for layer in LAYERS:
            bake(theme, layer)
    for f in sorted(OUT.glob("felt-*")):
        print(f"{f.stat().st_size:>7}  {f.name}")
