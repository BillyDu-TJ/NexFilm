"""The neutral-surface number, re-measured with a finer registration so the
result is a measurement rather than a registration artefact."""

import sys

sys.path.insert(0, "target/pylibs")
sys.path.insert(0, "scripts/experiments/density-fallback")

import numpy as np  # noqa: E402
from PIL import Image  # noqa: E402

from verify10 import RENDERDIR, TAGS, luma, read_render, srgb_decode  # noqa: E402

sys.stdout.reconfigure(encoding="utf-8")
RAFDIR = "test_picture/富士raf"


def align(ours, reference, height=640, search=40):
    def shrink(image, h):
        return np.asarray(
            Image.fromarray((np.clip(image, 0, 1) * 255).astype(np.uint8)).resize(
                (max(2, int(image.shape[1] * h / image.shape[0])), h), Image.LANCZOS),
            dtype=np.float32) / 255.0

    small_ours, small_ref = shrink(ours, height), shrink(reference, height)
    ours_level, ref_level = luma(small_ours), luma(small_ref)
    best = (None, 0, 0)
    for dy in range(-search, search + 1):
        for dx in range(-search, search + 1):
            y0, x0 = max(0, dy), max(0, dx)
            y1 = min(small_ref.shape[0], small_ours.shape[0] + dy)
            x1 = min(small_ref.shape[1], small_ours.shape[1] + dx)
            if y1 - y0 < 150 or x1 - x0 < 150:
                continue
            window = ours_level[y0 - dy:y1 - dy, x0 - dx:x1 - dx]
            target = ref_level[y0:y1, x0:x1]
            window = window - window.mean()
            target = target - target.mean()
            score = float((window * target).sum()
                          / (np.sqrt((window * window).sum() * (target * target).sum()) + 1e-9))
            if best[0] is None or score > best[0]:
                best = (score, dx, dy)
    scale = ours.shape[0] / height
    return best[0], int(round(best[1] * scale)), int(round(best[2] * scale))


def main():
    print(f"{'frame':<7}{'版本':>8}{'配准':>8}{'中性像素':>10}"
          f"{'中位 R/G':>10}{'中位 B/G':>10}{'P90 R/G':>10}{'P90 B/G':>10}")
    for tag in TAGS:
        reference = np.asarray(
            Image.open(f"{RAFDIR}/Capture One Catalog{tag}.tif").convert("RGB"),
            dtype=np.float32) / 255.0
        for variant in ("before", "after"):
            ours = read_render(f"{RENDERDIR}/raf-{tag}-{variant}-frame.png")
            score, dx, dy = align(ours, reference)
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
            ours_green = np.maximum(ours_crop[:, :, 1], 1e-5)
            rg = np.abs(ours_crop[:, :, 0][neutral] / ours_green[neutral] - 1)
            bg = np.abs(ours_crop[:, :, 2][neutral] / ours_green[neutral] - 1)
            print(f"{tag:<7}{variant:>8}{score:>8.3f}{int(neutral.sum()):>10}"
                  f"{np.median(rg):>10.3f}{np.median(bg):>10.3f}"
                  f"{np.percentile(rg, 90):>10.3f}{np.percentile(bg, 90):>10.3f}")


if __name__ == "__main__":
    main()
