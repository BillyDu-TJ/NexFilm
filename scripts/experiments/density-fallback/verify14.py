"""Is the remaining bright-area tint a sky question or a model error?

Splits the neutral core of each frame into its upper (sky and haze) and lower
(ground, water, concrete) parts. A clean lower half means our window carries the
neutral surfaces correctly and the residual lives where the reference chose to
neutralise a hazy sky; a dirty lower half means the model is still wrong.
"""

import sys

sys.path.insert(0, "target/pylibs")
sys.path.insert(0, "scripts/experiments/density-fallback")

import numpy as np  # noqa: E402
from PIL import Image  # noqa: E402

from verify10 import RENDERDIR, luma, read_render, srgb_decode  # noqa: E402
from verify13 import align, erode  # noqa: E402

sys.stdout.reconfigure(encoding="utf-8")
RAFDIR = "test_picture/富士raf"
TAGS = ("1942", "1948", "1949", "1950")


def main():
    print(f"{'frame':<7}{'版本':>7}{'上半(天空/雾)':>26}{'下半(地面/水面)':>26}")
    for tag in TAGS:
        reference = np.asarray(
            Image.open(f"{RAFDIR}/Capture One Catalog{tag}.tif").convert("RGB"),
            dtype=np.float32) / 255.0
        for variant in ("before", "after"):
            path = f"{RENDERDIR}/raf-{tag}-{variant}-frame.png"
            try:
                ours = read_render(path)
            except FileNotFoundError:
                print(f"{tag:<7}{variant:>7}  （没有画幅裁切版本）")
                continue
            _, dx, dy = align(ours, reference)
            y0, x0 = max(0, dy), max(0, dx)
            y1 = min(reference.shape[0], ours.shape[0] + dy)
            x1 = min(reference.shape[1], ours.shape[1] + dx)
            ours_crop = srgb_decode(np.clip(ours[y0 - dy:y1 - dy, x0 - dx:x1 - dx], 0, 1))
            ref_crop = srgb_decode(np.clip(reference[y0:y1, x0:x1], 0, 1))
            green = np.maximum(ref_crop[:, :, 1], 1e-5)
            level = luma(ref_crop)
            neutral = ((np.abs(np.log10(np.maximum(ref_crop[:, :, 0], 1e-4) / green)) < 0.03)
                       & (np.abs(np.log10(np.maximum(ref_crop[:, :, 2], 1e-4) / green)) < 0.03)
                       & (level > 0.05) & (level < 0.7))
            core = erode(neutral, 2)
            height = core.shape[0]
            ours_green = np.maximum(ours_crop[:, :, 1], 1e-5)
            cells = []
            for name, rows in (("upperv", slice(0, int(height * 0.45))),
                               ("lowerv", slice(int(height * 0.55), height))):
                select = np.zeros_like(core)
                select[rows] = core[rows]
                if select.sum() < 50:
                    cells.append(f"{name}: 无足够中性像素".rjust(26))
                    continue
                rg = np.median(ours_crop[:, :, 0][select] / ours_green[select])
                bg = np.median(ours_crop[:, :, 2][select] / ours_green[select])
                cells.append(f"{rg:.3f} / {bg:.3f}  ({int(select.sum())} px)".rjust(26))
            print(f"{tag:<7}{variant:>7}{cells[0]}{cells[1]}")


if __name__ == "__main__":
    main()
