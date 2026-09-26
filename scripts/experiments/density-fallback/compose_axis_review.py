"""Compose side-by-side review grids: C1 reference | before | after | pooled-axis.

Reads the renders the harness wrote under target/prescribed-response and lays
them out per family, so the pooled-axis window can be judged against the
Capture One reference and the previous rule in one image.
"""

import os
import sys

sys.path.insert(0, "target/pylibs")

from PIL import Image, ImageDraw  # noqa: E402

sys.stdout.reconfigure(encoding="utf-8")
ROOT = "test_picture"
OUT = "target/prescribed-response"
MODE = "rule5"
CELL = 460
PAD = 6
LABEL_H = 26


def load(path, cell):
    if not os.path.exists(path):
        return None
    image = Image.open(path).convert("RGB")
    scale = cell / max(image.size)
    size = (max(1, round(image.width * scale)), max(1, round(image.height * scale)))
    return image.resize(size, Image.LANCZOS)


def compose(rows, header, path):
    columns = max(len(row[1]) for row in rows)
    width = columns * (CELL + PAD) + PAD
    height = len(rows) * (CELL + LABEL_H + PAD) + PAD + LABEL_H
    canvas = Image.new("RGB", (width, height), (24, 24, 24))
    draw = ImageDraw.Draw(canvas)
    for column, title in enumerate(header):
        draw.text(
            (PAD + column * (CELL + PAD) + 4, 6),
            title,
            fill=(240, 240, 240),
        )
    for row_index, (title, cells) in enumerate(rows):
        top = PAD + LABEL_H + row_index * (CELL + LABEL_H + PAD)
        draw.text((PAD + 4, top + 2), title, fill=(180, 220, 255))
        for column, image in enumerate(cells):
            left = PAD + column * (CELL + PAD)
            if image is None:
                draw.rectangle(
                    [left, top + LABEL_H, left + CELL, top + LABEL_H + CELL],
                    outline=(90, 90, 90),
                )
                continue
            canvas.paste(image, (left, top + LABEL_H))
    canvas.save(path)
    print(f"wrote {path} ({width}x{height})")


def raf_rows():
    rows = []
    for step in range(11):
        tag = str(1942 + step)
        ref = load(f"{ROOT}/富士raf/Capture One Catalog{tag}.tif", CELL)
        before = load(f"{OUT}/raf-{tag}-before-frame.png", CELL) or load(
            f"{OUT}/raf-{tag}-before.png", CELL
        )
        after = load(f"{OUT}/raf-{tag}-after-rule4-frame.png", CELL) or load(
            f"{OUT}/raf-{tag}-after-rule4.png", CELL
        )
        axis = load(f"{OUT}/raf-{tag}-after-frame.png", CELL) or load(
            f"{OUT}/raf-{tag}-after.png", CELL
        )
        rows.append((f"raf-{tag}", [ref, before, after, axis]))
    return rows


def pano_rows():
    rows = []
    for number in (7569, 7571, 7573, 7575, 7577, 7579, 7581, 7583, 7585, 7587, 7589, 7591):
        before = load(f"{OUT}/pano-{number}-before-frame.png", CELL) or load(
            f"{OUT}/pano-{number}-before.png", CELL
        )
        after = load(f"{OUT}/pano-{number}-after-rule4-frame.png", CELL) or load(
            f"{OUT}/pano-{number}-after-rule4.png", CELL
        )
        axis = load(f"{OUT}/pano-{number}-after-frame.png", CELL) or load(
            f"{OUT}/pano-{number}-after.png", CELL
        )
        ref = None
        if number == 7587:
            ref = load(f"{OUT}/_ref-7587.jpg", CELL)
        rows.append((f"pano-{number}", [ref, before, after, axis]))
    return rows


def main():
    header = ["C1 参照", "before（旧兜底）", "规则4（同像素端点）", "规则5（片基落点均衡）"]
    compose(raf_rows(), header, f"{OUT}/_review-raf-11-axis-{MODE}.png")
    pano_header = ["参照（仅 7587）", "before", "规则4（同像素端点）", "规则5（片基落点均衡）"]
    compose(pano_rows(), pano_header, f"{OUT}/_review-pano-axis-{MODE}.png")


if __name__ == "__main__":
    main()
