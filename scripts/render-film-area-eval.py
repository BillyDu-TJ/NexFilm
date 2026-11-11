#!/usr/bin/env python3
"""Render evaluation overlays for visual review.

Green is the confirmed film area used as truth; orange is the detector output.
This tool is for local review only and writes under target/film-area-eval.
"""

from __future__ import annotations

import argparse
import json
import re
from pathlib import Path

from PIL import Image, ImageDraw, ImageFont


def load(path: Path) -> dict:
    return json.loads(path.read_text(encoding="utf-8"))


def draw_polygon(
    draw: ImageDraw.ImageDraw,
    points: list[list[float]],
    color: tuple[int, int, int],
    width: int,
) -> None:
    pixels = [(point[0] * draw.im.size[0], point[1] * draw.im.size[1]) for point in points]
    draw.line(pixels + [pixels[0]], fill=color, width=width, joint="curve")


def render_one(root: Path, dataset: dict, result: dict, output: Path) -> None:
    frame = next(item for item in dataset["frames"] if item["id"] == result["id"])
    image = Image.open(root / frame["image"]).convert("RGB")
    draw = ImageDraw.Draw(image)
    line_width = max(2, round(max(image.size) / 320))
    draw_polygon(draw, result["truth"], (60, 220, 110), line_width)
    draw_polygon(draw, result["detected"], (255, 132, 40), line_width)
    label = (
        f"{frame['roll_id']} | {frame['category']} | {result['status']} | "
        f"corner={result['corner_error']:.4f} leak={result['leak_area']:.4f}"
    )
    font = ImageFont.load_default(size=max(12, round(image.width / 55)))
    draw.rectangle((0, 0, image.width, font.size * 2), fill=(0, 0, 0))
    draw.text((8, 4), label, fill=(255, 255, 255), font=font)
    image.save(output, quality=92)


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--root", default="target/film-area-eval")
    parser.add_argument("--result", required=True)
    parser.add_argument("--output", required=True)
    parser.add_argument("--worst", type=int, default=0)
    args = parser.parse_args()

    root = Path(args.root)
    dataset = load(root / "truth.json")
    result = load(root / args.result)
    output = root / args.output
    output.mkdir(parents=True, exist_ok=True)
    frames = result["frames"]
    if args.worst:
        frames = sorted(
            frames,
            key=lambda frame: (frame["leak_area"], frame["corner_error"]),
            reverse=True,
        )[: args.worst]
    for index, frame in enumerate(frames):
        source = frame["id"].split("::")[-1]
        safe_source = re.sub(r'[<>:"/\\|?*]+', "_", source)
        name = f"{index:04d}-{frame['status']}-{safe_source}.jpg"
        render_one(root, dataset, frame, output / name[:180])
    print(f"Rendered {len(frames)} overlays to {output}")


if __name__ == "__main__":
    main()
