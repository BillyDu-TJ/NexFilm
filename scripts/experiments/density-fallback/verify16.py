"""Candidate tone curves for the prescribed-response path, previewed against
Capture One so the choice is a look decision with numbers next to it.

The preview applies each candidate to the displayed value, which is where the
application's own curve module sits, so the sheet is representative of what a
preset curve on this path would do.
"""

import os
import sys

sys.path.insert(0, "target/pylibs")
sys.path.insert(0, "scripts/experiments/density-fallback")

import numpy as np  # noqa: E402
from PIL import Image, ImageDraw  # noqa: E402

from verify10 import RENDERDIR, luma, read_render  # noqa: E402
from verify13 import best_orientation  # noqa: E402

sys.stdout.reconfigure(encoding="utf-8")
RAFDIR = "test_picture/富士raf"
TAGS = [str(1942 + step) for step in range(11)]
CELL = 170


def curve_a(value):  # press the middle down, the classic "too flat" fix
    return np.clip(value, 0, 1) ** 1.38


def curve_b(value):  # gentle S
    value = np.clip(value, 0, 1)
    return value * value * (3 - 2 * value) * 0.65 + value * 0.35


def curve_c(value):  # filmic: lifted black, rolled highlight, S middle
    value = np.clip(value, 0, 1)
    s = value * value * (3 - 2 * value)
    return np.clip(0.03 + 0.94 * s, 0, 1)


CANDIDATES = [("A 幂 1.38", curve_a), ("B 轻 S", curve_b), ("C 胶片 S", curve_c)]


def main():
    rows = []
    print(f"{'frame':<7}" + "".join(f"{name:>22}" for name, _ in CANDIDATES)
          + f"{'参照 5/50/95':>26}")
    for tag in TAGS:
        reference = np.asarray(
            Image.open(f"{RAFDIR}/Capture One Catalog{tag}.tif").convert("RGB"),
            dtype=np.float32) / 255.0
        framed = f"{RENDERDIR}/raf-{tag}-after-frame.png"
        path = framed if os.path.exists(framed) else f"{RENDERDIR}/raf-{tag}-after.png"
        after = read_render(path)
        _, orientation = best_orientation(after, reference)
        turns = {"id": lambda a: a, "h": lambda a: a[:, ::-1],
                 "v": lambda a: a[::-1, :], "hv": lambda a: a[::-1, ::-1]}
        after = turns[orientation](after)

        def cell(array):
            encoded = (np.clip(array, 0, 1) * 255).astype(np.uint8)
            image = Image.fromarray(encoded)
            width = max(40, int(image.width * CELL / image.height))
            return image.resize((width, CELL), Image.LANCZOS)

        cells = [cell(reference), cell(after)]
        line = f"{tag:<7}"
        for _, curve in CANDIDATES:
            curved = curve(after)
            cells.append(cell(curved))
            level = luma(curved)
            line += f"{np.round(np.percentile(level, [5, 50, 95]), 3)!s:>22}"
        rows.append((tag, cells, line))
        print(line + f"{np.round(np.percentile(luma(reference), [5, 50, 95]), 3)!s:>26}")

    width = max(sum(image.width for image in row[1]) + 60 for row in rows)
    height = len(rows) * (CELL + 24)
    sheet = Image.new("RGB", (width, height), (16, 16, 16))
    draw = ImageDraw.Draw(sheet)
    y = 0
    for tag, cells, _ in rows:
        draw.text((6, y + 5),
                  f"{tag}   Capture One | our render | +A pow1.38 | +B soft S | +C film S",
                  fill=(255, 200, 60))
        x = 0
        for image in cells:
            sheet.paste(image, (x, y + 22))
            x += image.width + 24
        y += CELL + 24
    sheet.save(f"{RENDERDIR}/_curves-menu.png")
    print("saved", sheet.size)


if __name__ == "__main__":
    main()
