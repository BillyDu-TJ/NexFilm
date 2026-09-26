"""What correction would actually make our render match the reference?

Fits, per channel, a linear map from our aligned render to Capture One's on the
same pixels: ref_c ~= a_c * ours_c + b_c. The fit says whether the error is a
gain (the window's span) or an offset (the window's origin), and how big it is,
which is what the window rule has to supply.
"""

import sys

sys.path.insert(0, "target/pylibs")
sys.path.insert(0, "scripts/experiments/density-fallback")

import numpy as np  # noqa: E402
from PIL import Image  # noqa: E402

from verify10 import RENDERDIR, read_render, srgb_decode  # noqa: E402
from verify13 import align  # noqa: E402

sys.stdout.reconfigure(encoding="utf-8")
RAFDIR = "test_picture/富士raf"
TAGS = ("1942", "1945", "1948", "1950")


def main():
    print(f"{'frame':<7}{'版本':>7}{'a_R':>7}{'b_R':>8}{'a_G':>7}{'b_G':>8}"
          f"{'a_B':>7}{'b_B':>8}   残差(中位相对)")
    for tag in TAGS:
        reference = np.asarray(
            Image.open(f"{RAFDIR}/Capture One Catalog{tag}.tif").convert("RGB"),
            dtype=np.float32) / 255.0
        for variant in ("before", "after"):
            import os

            framed = f"{RENDERDIR}/raf-{tag}-{variant}-frame.png"
            if not os.path.exists(framed):
                print(f"{tag:<7}{variant:>7}  （没有画幅裁切版本，跳过）")
                continue
            ours = read_render(framed)
            score, dx, dy, ours, _ = align(ours, reference)
            y0, x0 = max(0, dy), max(0, dx)
            y1 = min(reference.shape[0], ours.shape[0] + dy)
            x1 = min(reference.shape[1], ours.shape[1] + dx)
            ours_linear = srgb_decode(np.clip(ours[y0 - dy:y1 - dy, x0 - dx:x1 - dx], 0, 1))
            ref_linear = srgb_decode(np.clip(reference[y0:y1, x0:x1], 0, 1))
            # Central 80%: stay inside the film area, away from the rebate.
            h, w = ref_linear.shape[:2]
            crop = (slice(int(h * 0.1), int(h * 0.9)), slice(int(w * 0.1), int(w * 0.9)))
            ours_linear, ref_linear = ours_linear[crop], ref_linear[crop]
            step = max(1, (ours_linear.shape[0] * ours_linear.shape[1]) // 400000)
            fits, residual = [], []
            for channel in range(3):
                x = ours_linear[:, :, channel].ravel()[::step]
                y = ref_linear[:, :, channel].ravel()[::step]
                design = np.stack([x, np.ones_like(x)], axis=1)
                a, b = np.linalg.lstsq(design, y, rcond=None)[0]
                fits.append((a, b))
                corrected = a * x + b
                green = np.maximum(ref_linear[:, :, 1].ravel()[::step], 1e-5)
                residual.append(float(np.median(np.abs(corrected / np.maximum(y, 1e-5) - 1))))
            print(f"{tag:<7}{variant:>7}{fits[0][0]:>7.3f}{fits[0][1]:>8.4f}"
                  f"{fits[1][0]:>7.3f}{fits[1][1]:>8.4f}"
                  f"{fits[2][0]:>7.3f}{fits[2][1]:>8.4f}"
                  f"   {np.round(residual, 3)!s:>20}   配准 {score:.2f}")


if __name__ == "__main__":
    main()
