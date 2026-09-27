"""Decisive test: is our render a sub-crop of the reference's frame, or a
different frame of the strip altogether?

Wide-scale phase correlation (0.2x-1.0x) between the full scan render and the
C1 export. A high-correlation hit at a small scale means the scan's frame
contains the reference's scene and our detected area was just a sub-region; no
hit at any scale means we are rendering a different negative than the one the
reference shows.
"""

import os
import sys

sys.path.insert(0, "target/pylibs")

import numpy as np  # noqa: E402
from PIL import Image  # noqa: E402

sys.stdout.reconfigure(encoding="utf-8")
LUMA = np.array([0.2126, 0.7152, 0.0722], dtype=np.float64)
CANVAS = 512


def load(path):
    return np.asarray(
        Image.open(path).convert("RGB").resize((CANVAS, CANVAS), Image.LANCZOS),
        dtype=np.float64,
    ) / 255.0


def best_alignment(render, ref, scales):
    ref_l = ref @ LUMA
    b = np.fft.fft2(ref_l - ref_l.mean())
    best = (-2.0, None)
    for flip in (False, True):
        for rot in range(4):
            base = np.rot90(render[:, ::-1] if flip else render, rot)
            for scale in scales:
                sw, sh = max(8, int(CANVAS * scale)), max(8, int(CANVAS * scale))
                small = np.asarray(
                    Image.fromarray((np.clip(base, 0, 1) * 255).astype(np.uint8)).resize(
                        (sw, sh), Image.BILINEAR
                    ),
                    dtype=np.float64,
                ) / 255.0
                canvas = np.zeros((CANVAS, CANVAS, 3))
                oy, ox = (CANVAS - sh) // 2, (CANVAS - sw) // 2
                canvas[oy : oy + sh, ox : ox + sw] = small
                cand_l = canvas @ LUMA
                a = np.fft.fft2(cand_l - cand_l.mean())
                cross = np.fft.ifft2(a * np.conj(b))
                peak = np.unravel_index(np.argmax(np.abs(cross)), cross.shape)
                shift = (
                    peak[0] if peak[0] <= CANVAS // 2 else peak[0] - CANVAS,
                    peak[1] if peak[1] <= CANVAS // 2 else peak[1] - CANVAS,
                )
                moved = np.roll(canvas, shift, axis=(0, 1))
                corr = np.corrcoef((moved @ LUMA).ravel(), ref_l.ravel())[0, 1]
                if corr > best[0]:
                    best = (corr, (rot, flip, round(scale, 2), shift))
    return best


def main():
    scales = [0.2, 0.25, 0.3, 0.35, 0.4, 0.5, 0.6, 0.7, 0.85, 1.0]
    for tag in ("1944", "1945", "1946", "1942"):
        render = load(f"target/prescribed-response/raf-{tag}-after.png")
        ref = load(f"test_picture/富士raf/Capture One Catalog{tag}.tif")
        corr, params = best_alignment(render, ref, scales)
        print(f"raf-{tag}: 全扫描 vs C1 最佳 corr={corr:.3f} 参数={params}")


if __name__ == "__main__":
    main()
