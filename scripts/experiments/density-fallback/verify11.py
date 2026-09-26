"""How much tone curve does the fallback path need to stop looking washed out?

Fits one fixed exponent (display = luma**k) on the aligned overlap between our
render and the Capture One reference. A single exponent that works for all three
frames is a preset, not a per-frame estimate.
"""

import sys

sys.path.insert(0, "target/pylibs")
sys.path.insert(0, "scripts/experiments/density-fallback")

import numpy as np  # noqa: E402
from PIL import Image  # noqa: E402

from verify10 import RENDERDIR, TAGS, align, luma, read_render, srgb_decode  # noqa: E402

sys.stdout.reconfigure(encoding="utf-8")
RAFDIR = "test_picture/富士raf"


def overlap(ours, reference):
    _, dx, dy = align(ours, reference)
    y0, x0 = max(0, dy), max(0, dx)
    y1 = min(reference.shape[0], ours.shape[0] + dy)
    x1 = min(reference.shape[1], ours.shape[1] + dx)
    return (srgb_decode(np.clip(ours[y0 - dy:y1 - dy, x0 - dx:x1 - dx], 0, 1)),
            srgb_decode(np.clip(reference[y0:y1, x0:x1], 0, 1)))


def main():
    print(f"{'frame':<7}{'需要的指数 k':>14}{'对齐前 p5/50/95':>26}"
          f"{'加 k 后 p5/50/95':>26}{'参照 p5/50/95':>26}")
    exponents = []
    for tag in TAGS:
        reference = np.asarray(
            Image.open(f"{RAFDIR}/Capture One Catalog{tag}.tif").convert("RGB"),
            dtype=np.float32) / 255.0
        ours = read_render(f"{RENDERDIR}/raf-{tag}-after-frame.png")
        ours_lin, ref_lin = overlap(ours, reference)
        ours_level = luma(ours_lin)
        ref_level = luma(ref_lin)
        best = None
        for k in np.arange(0.6, 2.6, 0.05):
            stretched = np.clip(ours_level, 0, 1) ** k
            # Compare distributions, which is what "washed out" means visually.
            quantiles = np.linspace(0.01, 0.99, 40)
            delta = np.mean(np.abs(np.quantile(stretched, quantiles)
                                   - np.quantile(ref_level, quantiles)))
            if best is None or delta < best[0]:
                best = (delta, float(k))
        exponents.append(best[1])
        stretched = np.clip(ours_level, 0, 1) ** best[1]
        print(f"{tag:<7}{best[1]:>14.2f}"
              f"{np.round(np.percentile(ours_level, [5, 50, 95]), 3)!s:>26}"
              f"{np.round(np.percentile(stretched, [5, 50, 95]), 3)!s:>26}"
              f"{np.round(np.percentile(ref_level, [5, 50, 95]), 3)!s:>26}")
    print()
    print(f"三帧需要的指数：{np.round(exponents, 2)}"
          f"  中位 {np.median(exponents):.2f}")
    print("（1.00 表示现行映射；越大表示越需要压暗中间调、加反差）")


if __name__ == "__main__":
    main()
