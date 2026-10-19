"""Measures how seamlessly a tile wraps.

Compares the step across its edges with the step between neighbouring pixels inside it.
"""

import subprocess
import sys

MAGICK = "magick"


def mean_step(image, a, b):
    """Mean absolute alpha difference (0-255) between two one-pixel strips; the tiles' colour never varies."""
    out = subprocess.run(
        [
            MAGICK,
            "(",
            image,
            "-crop",
            a,
            "+repage",
            "-alpha",
            "extract",
            ")",
            "(",
            image,
            "-crop",
            b,
            "+repage",
            "-alpha",
            "extract",
            ")",
            "-compose",
            "difference",
            "-composite",
            "-format",
            "%[fx:mean]",
            "info:",
        ],
        capture_output=True,
        text=True,
        check=True,
    ).stdout
    return float(out) * 255


def seams(image):
    w, h = map(
        int,
        subprocess.run(
            [MAGICK, "identify", "-format", "%w %h", image], capture_output=True, text=True, check=True
        ).stdout.split(),
    )
    mid_x, mid_y = w // 2, h // 2
    return {
        "x inner": mean_step(image, f"1x{h}+{mid_x}+0", f"1x{h}+{mid_x + 1}+0"),
        "x seam": mean_step(image, f"1x{h}+{w - 1}+0", f"1x{h}+0+0"),
        "y inner": mean_step(image, f"{w}x1+0+{mid_y}", f"{w}x1+0+{mid_y + 1}"),
        "y seam": mean_step(image, f"{w}x1+0+{h - 1}", f"{w}x1+0+0"),
    }


# The worst the tiles do today is about 2.2x; a visible repeat measured 5x and more.
MAX_RATIO = 3


def worst_ratio(s):
    return max(s["x seam"] / max(s["x inner"], 1e-9), s["y seam"] / max(s["y inner"], 1e-9))


if __name__ == "__main__":
    seamed = []
    for image in sys.argv[1:]:
        s = seams(image)
        ratio = worst_ratio(s)
        print(f"{image}: " + "  ".join(f"{k} {v:.2f}" for k, v in s.items()) + f"  worst seam/inner {ratio:.1f}x")
        if ratio > MAX_RATIO:
            seamed.append(image)
    if seamed:
        sys.exit(f"these tiles show their edges (over {MAX_RATIO}x): {' '.join(seamed)}")
