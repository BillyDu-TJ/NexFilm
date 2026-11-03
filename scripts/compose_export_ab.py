"""Compose the export A/B panels into one labelled image for eyeball review.

Reads the three panels written by
`cargo test --lib export_yellowing_visual_ab -- --ignored`
(target/diag-export-parity/<stem>-ab-{canvas,export,legacy}.jpg) and writes
a side-by-side composite next to the reported sample so the caller can judge
the canvas-vs-export colour difference directly.
"""
from __future__ import annotations

import sys
from pathlib import Path

from PIL import Image, ImageDraw, ImageFont

ROOT = Path(__file__).resolve().parent.parent
PANEL_DIR = ROOT / "target" / "diag-export-parity"
OUT_PATH = ROOT / "test_picture" / "导出发黄" / "0218-对比-画布vs导出.jpg"

LABELS = [
    ("ab-canvas.jpg", "1 管线中(画布所见)"),
    ("ab-export.jpg", "2 导出后(当前 main)"),
    ("ab-legacy.jpg", "3 修复前导出链(报修的发黄)"),
]

HEADER = 56
SEPARATOR = 6
TARGET_HEIGHT = 1080

FONT_CANDIDATES = [
    r"C:\Windows\Fonts\msyh.ttc",
    r"C:\Windows\Fonts\simhei.ttf",
    "/System/Library/Fonts/PingFang.ttc",
    "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
]


def load_font(size: int) -> ImageFont.FreeTypeFont | None:
    for candidate in FONT_CANDIDATES:
        if Path(candidate).exists():
            try:
                return ImageFont.truetype(candidate, size)
            except OSError:
                continue
    return None


def main() -> int:
    stem = sys.argv[1] if len(sys.argv) > 1 else "0218-"
    panels = []
    for suffix, label in LABELS:
        path = PANEL_DIR / f"{stem}-{suffix}"
        if not path.exists():
            raise SystemExit(f"missing panel: {path}")
        panels.append((Image.open(path).convert("RGB"), label))

    scaled = []
    for image, label in panels:
        ratio = TARGET_HEIGHT / image.height
        scaled.append((image.resize((round(image.width * ratio), TARGET_HEIGHT), Image.LANCZOS), label))

    font = load_font(34)
    total_width = sum(image.width for image, _ in scaled) + SEPARATOR * (len(scaled) - 1)
    composite = Image.new("RGB", (total_width, TARGET_HEIGHT + HEADER), (24, 24, 28))
    draw = ImageDraw.Draw(composite)
    x = 0
    for image, label in scaled:
        composite.paste(image, (x, HEADER))
        if font is not None:
            draw.text((x + 12, 12), label, fill=(240, 240, 240), font=font)
        else:
            draw.rectangle((x + 8, 10, x + 8 + 340, 46), fill=(60, 60, 70))
        x += image.width + SEPARATOR

    OUT_PATH.parent.mkdir(parents=True, exist_ok=True)
    composite.save(OUT_PATH, quality=92)
    print(f"[AB] composite written: {OUT_PATH} ({composite.width}x{composite.height})")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
