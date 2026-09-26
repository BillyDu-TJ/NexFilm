"""Build the review sheet: for every repro frame, Capture One's render next to
our before and after renders, with our images turned the way the reference is.

The orientation is chosen by the same coarse correlation the residual
measurement uses, so the sheet and the numbers agree.
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
CELL = 200


def load(tag, variant):
    framed = f"{RENDERDIR}/raf-{tag}-{variant}-frame.png"
    path = framed if os.path.exists(framed) else f"{RENDERDIR}/raf-{tag}-{variant}.png"
    return read_render(path), os.path.exists(framed)


def main():
    rows = []
    for tag in TAGS:
        reference = np.asarray(
            Image.open(f"{RAFDIR}/Capture One Catalog{tag}.tif").convert("RGB"),
            dtype=np.float32) / 255.0
        after, framed = load(tag, "after")
        before, _ = load(tag, "before")
        _, orientation = best_orientation(after, reference)
        turns = {"id": lambda a: a, "h": lambda a: a[:, ::-1],
                 "v": lambda a: a[::-1, :], "hv": lambda a: a[::-1, ::-1]}
        turn = turns[orientation]

        def cell(array):
            encoded = (np.clip(turn(array), 0, 1) * 255).astype(np.uint8)
            image = Image.fromarray(encoded)
            width = max(40, int(image.width * CELL / image.height))
            return image.resize((width, CELL), Image.LANCZOS)

        def ratios(array):
            flat = turn(array).reshape(-1, 3).mean(axis=0)
            return flat[0] / flat[1], flat[2] / flat[1]

        before_rg, before_bg = ratios(before)
        after_rg, after_bg = ratios(after)
        rows.append((tag, framed, orientation, cell(reference),
                     cell(before), cell(after),
                     (before_rg, before_bg), (after_rg, after_bg)))

    width = max(sum(item.width for item in row[3:6]) + 80 for row in rows)
    height = len(rows) * (CELL + 26)
    sheet = Image.new("RGB", (width, height), (16, 16, 16))
    draw = ImageDraw.Draw(sheet)
    y = 0
    for tag, framed, orientation, ref, before, after, before_ratio, after_ratio in rows:
        note = "" if framed else "  (no confirmed film area: whole scan)"
        draw.text((6, y + 6),
                  f"{tag}   Capture One | before R/G {before_ratio[0]:.2f} B/G {before_ratio[1]:.2f}"
                  f" | after R/G {after_ratio[0]:.2f} B/G {after_ratio[1]:.2f}"
                  f"   (turn {orientation}){note}",
                  fill=(255, 200, 60))
        x = 0
        for image in (ref, before, after):
            sheet.paste(image, (x, y + 24))
            x += image.width + 30
        y += CELL + 26
    sheet.save(f"{RENDERDIR}/_review-raf-11-final.png")
    print("saved", sheet.size)


if __name__ == "__main__":
    main()
