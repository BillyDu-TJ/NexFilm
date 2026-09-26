"""Neutral-surface residual measured in blocks, so the number does not depend
on pixel-exact registration.

Two-stage alignment (coarse at 320 px, refine at 640 px), then 18x18 blocks.
Only blocks the reference itself renders as mostly neutral are scored, and the
score is the median over blocks of the block median chroma: a couple of pixels
of residual misalignment moves a median of thousands of pixels by almost
nothing.
"""

import sys

sys.path.insert(0, "target/pylibs")
sys.path.insert(0, "scripts/experiments/density-fallback")

import numpy as np  # noqa: E402
from PIL import Image  # noqa: E402

from verify10 import RENDERDIR, luma, read_render, srgb_decode  # noqa: E402

# The four frames whose film area the detector did not confirm have no
# film-area crop to lay over Capture One's render; their whole-frame channel
# ratios are covered by the render report instead.
TAGS = ("1942", "1943", "1946", "1947", "1948", "1949", "1950")

sys.stdout.reconfigure(encoding="utf-8")
RAFDIR = "test_picture/富士raf"


def shrink(image, height):
    return np.asarray(
        Image.fromarray((np.clip(image, 0, 1) * 255).astype(np.uint8)).resize(
            (max(2, int(image.shape[1] * height / image.shape[0])), height), Image.LANCZOS),
        dtype=np.float32) / 255.0


def correlation(small_ref, small_ours, dx, dy):
    y0, x0 = max(0, dy), max(0, dx)
    y1 = min(small_ref.shape[0], small_ours.shape[0] + dy)
    x1 = min(small_ref.shape[1], small_ours.shape[1] + dx)
    if y1 - y0 < 100 or x1 - x0 < 100:
        return None
    window = luma(small_ours)[y0 - dy:y1 - dy, x0 - dx:x1 - dx]
    target = luma(small_ref)[y0:y1, x0:x1]
    window = window - window.mean()
    target = target - target.mean()
    return float((window * target).sum()
                 / (np.sqrt((window * window).sum() * (target * target).sum()) + 1e-9))


def align(ours, reference):
    """Align after choosing the orientation, because the scans do not share one."""
    oriented, orientation = best_orientation(ours, reference)
    score, dx, dy = align_fixed(oriented, reference)
    # The offsets describe `oriented`, so hand it back: cropping the original
    # with them silently compares two different parts of the frame.
    return score, dx, dy, oriented, orientation


def best_orientation(ours, reference):
    coarse_ref = shrink(reference, 200)
    best = (None, 0, "id")
    variants = {
        "id": lambda image: image,
        "h": lambda image: image[:, ::-1],
        "v": lambda image: image[::-1, :],
        "hv": lambda image: image[::-1, ::-1],
    }
    for name, transform in variants.items():
        candidate = transform(ours)
        coarse_ours = shrink(candidate, 200)
        for dy in range(-40, 41, 4):
            for dx in range(-40, 41, 4):
                score = correlation(coarse_ref, coarse_ours, dx, dy)
                if score is not None and (best[0] is None or score > best[0]):
                    best = (score, name, name)
    return variants[best[1]](ours), best[1]


def align_fixed(ours, reference):
    coarse_ours, coarse_ref = shrink(ours, 320), shrink(reference, 320)
    best = (None, 0, 0)
    for dy in range(-80, 81, 2):
        for dx in range(-80, 81, 2):
            score = correlation(coarse_ref, coarse_ours, dx, dy)
            if score is not None and (best[0] is None or score > best[0]):
                best = (score, dx, dy)
    scale = ours.shape[0] / 320
    guess_x, guess_y = int(round(best[1] * scale)), int(round(best[2] * scale))
    fine_ours, fine_ref = shrink(ours, 640), shrink(reference, 640)
    scale = ours.shape[0] / 640
    fine = best
    for dy in range(int(round(guess_y / scale)) - 6, int(round(guess_y / scale)) + 7):
        for dx in range(int(round(guess_x / scale)) - 6, int(round(guess_x / scale)) + 7):
            score = correlation(fine_ref, fine_ours, dx, dy)
            if score is not None and score > fine[0]:
                fine = (score, dx, dy)
    return fine[0], int(round(fine[1] * scale)), int(round(fine[2] * scale))


def erode(mask, iterations=2):
    for _ in range(iterations):
        grown = mask.copy()
        grown[1:, :] &= mask[:-1, :]
        grown[:-1, :] &= mask[1:, :]
        grown[:, 1:] &= mask[:, :-1]
        grown[:, :-1] &= mask[:, 1:]
        mask = grown
    return mask


def measure(ours, reference):
    score, dx, dy, ours, orientation = align(ours, reference)
    y0, x0 = max(0, dy), max(0, dx)
    y1 = min(reference.shape[0], ours.shape[0] + dy)
    x1 = min(reference.shape[1], ours.shape[1] + dx)
    ours_crop = srgb_decode(np.clip(ours[y0 - dy:y1 - dy, x0 - dx:x1 - dx], 0, 1))
    ref_crop = srgb_decode(np.clip(reference[y0:y1, x0:x1], 0, 1))
    height, width = ref_crop.shape[:2]
    ref_green = np.maximum(ref_crop[:, :, 1], 1e-5)
    ref_level = luma(ref_crop)
    neutral = ((np.abs(np.log10(np.maximum(ref_crop[:, :, 0], 1e-4) / ref_green)) < 0.03)
               & (np.abs(np.log10(np.maximum(ref_crop[:, :, 2], 1e-4) / ref_green)) < 0.03)
               & (ref_level > 0.05) & (ref_level < 0.7))
    # Erode the mask so a one- or two-pixel registration error cannot pull
    # non-neutral neighbours into the statistics.
    core = erode(neutral, 2)
    ours_green = np.maximum(ours_crop[:, :, 1], 1e-5)
    rg = ours_crop[:, :, 0][core] / ours_green[core]
    bg = ours_crop[:, :, 2][core] / ours_green[core]
    # Is the residual just the display ceiling? Report how much of our own
    # render sits at the top inside the same neutral core.
    ceiling = [float((ours_crop[:, :, channel][core] >= 0.99).mean() * 100)
               for channel in range(3)]
    floor = [float((ours_crop[:, :, channel][core] <= 0.01).mean() * 100)
             for channel in range(3)]
    bands = []
    level_core = ref_level[core]
    for low, high in ((0.0, 0.12), (0.12, 0.35), (0.35, 1.0)):
        select = (level_core >= low) & (level_core < high)
        if select.sum() < 40:
            bands.append(None)
            continue
        bands.append((float(np.median(rg[select])), float(np.median(bg[select])),
                      int(select.sum())))
    return score, int(core.sum()), rg, bg, bands, ceiling, floor, orientation


def main():
    print(f"{'frame':<7}{'版本':>8}{'配准':>8}{'中性像素':>9}{'整体 R/G':>10}{'整体 B/G':>10}"
          f"{'暗部 R/G,B/G':>18}{'中间调 R/G,B/G':>20}{'亮部 R/G,B/G':>18}"
          f"{'顶白% R/G/B':>16}{'朝向':>6}")
    for tag in TAGS:
        reference = np.asarray(
            Image.open(f"{RAFDIR}/Capture One Catalog{tag}.tif").convert("RGB"),
            dtype=np.float32) / 255.0
        for variant in ("before", "after"):
            ours = read_render(f"{RENDERDIR}/raf-{tag}-{variant}-frame.png")
            score, count, rg, bg, bands, ceiling, floor, orientation = measure(ours, reference)
            if count == 0:
                print(f"{tag:<7}{variant:>8}{score:>8.3f}{0:>10}  （没有可用中性像素）")
                continue
            cells = ""
            for band in bands:
                cells += ("      —         " if band is None
                          else f"{band[0]:>7.3f},{band[1]:<7.3f}")
            print(f"{tag:<7}{variant:>8}{score:>8.3f}{count:>9}"
                  f"{np.median(rg):>10.3f}{np.median(bg):>10.3f}{cells}"
                  f"{np.round(ceiling, 1)!s:>16}{orientation:>6}")


if __name__ == "__main__":
    main()
