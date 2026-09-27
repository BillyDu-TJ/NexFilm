"""Probe what the raw RAF scans actually contain around the film frame.

The user's eye report (1945 green, 1946 magenta, far beyond any clamp) plus
the montage's zoom mismatch (ours a tight crop, C1 the full frame) points at
the evidence path's geometry and base measurement, not at the profile layers.
This probe develops the raw scans neutrally and reports the layout: where the
film strip sits, what the area around the frame measures per channel, and
whether a clean orange film base is actually available in the scan.
"""

import sys

sys.path.insert(0, "target/pylibs")

import numpy as np  # noqa: E402
import rawpy  # noqa: E402
from PIL import Image, ImageDraw  # noqa: E402

sys.stdout.reconfigure(encoding="utf-8")

TAGS = ["1942", "1945", "1946"]
ROOT = "test_picture/富士raf"


def develop(tag):
    with rawpy.imread(f"{ROOT}/Capture One Catalog{tag}.raf") as raw:
        rgb = raw.postprocess(
            use_camera_wb=True,
            no_auto_bright=True,
            output_bps=16,
            gamma=(1.0, 1.0),
            demosaic_algorithm=rawpy.DemosaicAlgorithm.LINEAR,
        )
    return rgb.astype(np.float64) / 65535.0


def preview(image, path):
    h, w = image.shape[:2]
    scale = 640.0 / max(h, w)
    small = np.asarray(
        Image.fromarray((np.clip(image, 0, 1) * 255).astype(np.uint8)).resize(
            (int(w * scale), int(h * scale)), Image.BILINEAR
        )
    )
    canvas = Image.fromarray(small.copy())
    draw = ImageDraw.Draw(canvas)
    sh, sw = small.shape[:2]
    for fraction in np.linspace(0.1, 0.9, 9):
        x = int(sw * fraction)
        y = int(sh * fraction)
        draw.line([(x, 0), (x, sh)], fill=(255, 0, 255), width=1)
        draw.line([(0, y), (sw, y)], fill=(255, 0, 255), width=1)
        draw.text((x + 2, 2), f"{fraction:.1f}", fill=(255, 0, 255))
        draw.text((2, y + 2), f"{fraction:.1f}", fill=(255, 0, 255))
    canvas.save(path)
    return small


def stats(image, y0, y1, x0, x1, label):
    region = image[y0:y1, x0:x1]
    med = np.median(region.reshape(-1, 3), axis=0)
    bright = region.reshape(-1, 3)
    keep = (bright.max(axis=1) > 0.05) & (bright.max(axis=1) < 0.98)
    med_keep = np.median(bright[keep], axis=0) if keep.sum() > 100 else med
    print(
        f"    {label}: 中位 R {med[0]:.3f} G {med[1]:.3f} B {med[2]:.3f}"
        f"  | 去黑白后 R {med_keep[0]:.3f} G {med_keep[1]:.3f} B {med_keep[2]:.3f}"
        f"  R/G {med_keep[0] / max(med_keep[1], 1e-6):.2f} B/G {med_keep[2] / max(med_keep[1], 1e-6):.2f}"
        f"  (n={int(keep.sum())})"
    )


def main():
    for tag in TAGS:
        image = develop(tag)
        h, w = image.shape[:2]
        print(f"--- raf-{tag}: 原始 {w}x{h}")
        small = preview(image, f"target/prescribed-response/_probe-{tag}.png")
        sh, sw = small.shape[:2]
        # Row/column brightness profiles locate the strip and the frame.
        luma = small @ np.array([0.2126, 0.7152, 0.0722])
        rows = luma.mean(axis=1)
        cols = luma.mean(axis=0)
        bright_rows = np.where(rows > np.quantile(rows, 0.55))[0]
        bright_cols = np.where(cols > np.quantile(cols, 0.55))[0]
        print(
            f"    亮区行 {bright_rows[0] / sh:.2f}-{bright_rows[-1] / sh:.2f}"
            f"  亮区列 {bright_cols[0] / sw:.2f}-{bright_cols[-1] / sw:.2f}"
        )
        # Sample a 3x3 grid of interior regions and print each one's chroma,
        # so the base / sprocket / scene layout shows up in numbers.
        for gy in (0.2, 0.5, 0.8):
            row = []
            for gx in (0.2, 0.5, 0.8):
                y0, x0 = int(sh * gy) - 20, int(sw * gx) - 20
                region = small[y0 : y0 + 40, x0 : x0 + 40]
                med = np.median(region.reshape(-1, 3), axis=0)
                row.append(f"({gy:.1f},{gx:.1f}) R{med[0]:.2f} G{med[1]:.2f} B{med[2]:.2f}")
            print("    " + "  ".join(row))
        print(f"    预览: target/prescribed-response/_probe-{tag}.png")


if __name__ == "__main__":
    main()
