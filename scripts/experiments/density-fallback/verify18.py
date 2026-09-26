"""Frame-mean chroma against the reference's own frame mean.

No alignment is involved: a whole-frame mean is exactly what "the picture looks
pink" means, and the target is whatever Capture One's own render averages, not a
flat 1.000.
"""

import os
import sys

sys.path.insert(0, "target/pylibs")

import numpy as np  # noqa: E402
from PIL import Image  # noqa: E402

sys.stdout.reconfigure(encoding="utf-8")
RAFDIR = "test_picture/富士raf"
RENDERDIR = "target/prescribed-response"
TAGS = [str(1942 + step) for step in range(11)]


def ratios(array):
    green = array[:, :, 1].mean()
    return array[:, :, 0].mean() / green, array[:, :, 2].mean() / green


def main():
    print(f"{'frame':<7}{'C1 参照':>18}{'before':>18}{'after':>18}"
          f"{'after 与参照差':>18}")
    worst = []
    for tag in TAGS:
        reference = np.asarray(
            Image.open(f"{RAFDIR}/Capture One Catalog{tag}.tif").convert("RGB"),
            dtype=np.float32) / 255.0
        ref_rg, ref_bg = ratios(reference)
        row = f"{tag:<7}{ref_rg:>8.3f},{ref_bg:<9.3f}"
        values = {}
        for variant in ("before", "after"):
            framed = f"{RENDERDIR}/raf-{tag}-{variant}-frame.png"
            path = framed if os.path.exists(framed) else f"{RENDERDIR}/raf-{tag}-{variant}.png"
            ours = np.asarray(Image.open(path).convert("RGB"), dtype=np.float32) / 255.0
            values[variant] = ratios(ours)
            rg, bg = values[variant]
            row += f"{rg:>8.3f},{bg:<9.3f}"
        after_rg, after_bg = values["after"]
        delta = max(abs(after_rg - ref_rg), abs(after_bg - ref_bg))
        worst.append((delta, tag))
        row += f"{delta:>18.3f}"
        print(row)
    worst.sort(reverse=True)
    print()
    print("与参照差距最大的帧:", ", ".join(f"{tag} ({delta:.3f})" for delta, tag in worst[:4]))


if __name__ == "__main__":
    main()
