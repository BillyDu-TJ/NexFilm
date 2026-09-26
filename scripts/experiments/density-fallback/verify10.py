"""Stage 3: register our render against the Capture One reference and measure
the two things the quantitative line is about - neutral-surface residual and
contrast - instead of eyeballing them.

Registration is a coarse scale-plus-offset search on downsampled luminance: the
two images show the same frame with different crops, and the scene has enough
structure (horizon, rocks, figures) for normalized cross-correlation to lock on.
"""

import sys

sys.path.insert(0, "target/pylibs")

import numpy as np  # noqa: E402
from PIL import Image  # noqa: E402

sys.stdout.reconfigure(encoding="utf-8")

W = np.array([0.2126, 0.7152, 0.0722])
RAFDIR = "test_picture/富士raf"
RENDERDIR = "target/prescribed-response"
TAGS = ("1942", "1949", "1950")


def srgb_decode(encoded):
    return np.where(encoded <= 0.04045, encoded / 12.92,
                    ((encoded + 0.055) / 1.055) ** 2.4)


def luma(image):
    return image @ W


def read_render(path):
    """The renders are 16-bit sRGB-encoded PNGs; PIL hands them over as 8-bit."""
    return np.asarray(Image.open(path).convert("RGB"), dtype=np.float32) / 255.0


def align(ours, reference, height=320, search=70):
    """Translation search on the luminance, so the two different crops of the
    same frame can be compared pixel by pixel."""
    def shrink(image):
        return np.asarray(
            Image.fromarray((np.clip(image, 0, 1) * 255).astype(np.uint8)).resize(
                (max(2, int(image.shape[1] * height / image.shape[0])), height),
                Image.LANCZOS), dtype=np.float32) / 255.0

    small_ours, small_ref = shrink(ours), shrink(reference)
    a = luma(small_ours) - luma(small_ours).mean()
    best = (None, 0, 0)
    for dy in range(-search, search + 1, 2):
        for dx in range(-search, search + 1, 2):
            y0, x0 = max(0, dy), max(0, dx)
            y1 = min(small_ref.shape[0], small_ours.shape[0] + dy)
            x1 = min(small_ref.shape[1], small_ours.shape[1] + dx)
            if y1 - y0 < 80 or x1 - x0 < 80:
                continue
            window = luma(small_ours)[y0 - dy:y1 - dy, x0 - dx:x1 - dx]
            target = luma(small_ref)[y0:y1, x0:x1]
            w = window - window.mean()
            t = target - target.mean()
            score = float((w * t).sum() / (np.sqrt((w * w).sum() * (t * t).sum()) + 1e-9))
            if best[0] is None or score > best[0]:
                best = (score, dx, dy)
    scale = ours.shape[0] / height
    return best[0], int(best[1] * scale), int(best[2] * scale)


def metrics(ours, reference):
    """Neutral-surface residual and tone, measured on the aligned overlap."""
    score, dx, dy = align(ours, reference)
    y0, x0 = max(0, dy), max(0, dx)
    y1 = min(reference.shape[0], ours.shape[0] + dy)
    x1 = min(reference.shape[1], ours.shape[1] + dx)
    ours_crop = srgb_decode(np.clip(ours[y0 - dy:y1 - dy, x0 - dx:x1 - dx], 0, 1))
    ref_crop = srgb_decode(np.clip(reference[y0:y1, x0:x1], 0, 1))
    level = luma(ref_crop)
    green = np.maximum(ref_crop[:, :, 1], 1e-5)
    neutral = ((np.abs(np.log10(np.maximum(ref_crop[:, :, 0], 1e-4) / green)) < 0.03)
               & (np.abs(np.log10(np.maximum(ref_crop[:, :, 2], 1e-4) / green)) < 0.03)
               & (level > 0.05) & (level < 0.7))
    ours_green = np.maximum(ours_crop[:, :, 1], 1e-5)
    rg = np.abs(ours_crop[:, :, 0][neutral] / ours_green[neutral] - 1)
    bg = np.abs(ours_crop[:, :, 2][neutral] / ours_green[neutral] - 1)
    ours_level = luma(ours_crop)
    return {
        "score": score,
        "n": int(neutral.sum()),
        "rg": float(np.median(rg)) if rg.size else float("nan"),
        "bg": float(np.median(bg)) if bg.size else float("nan"),
        "ours_p": np.percentile(ours_level, [5, 50, 95]),
        "ref_p": np.percentile(level, [5, 50, 95]),
    }


def main():
    print(f"{'frame':<7}{'版本':>8}{'配准相关':>10}{'中性像素':>10}{'|R/G-1|':>10}{'|B/G-1|':>10}"
          f"{'我们的 5/50/95%':>26}{'参照 5/50/95%':>26}")
    for tag in TAGS:
        reference = np.asarray(
            Image.open(f"{RAFDIR}/Capture One Catalog{tag}.tif").convert("RGB"),
            dtype=np.float32) / 255.0
        reference = srgb_decode(reference)
        rows = {}
        for variant in ("before", "after"):
            ours = read_render(f"{RENDERDIR}/raf-{tag}-{variant}-frame.png")
            rows[variant] = metrics(ours, reference)
        ours_p = rows["after"]["ours_p"]
        ref_p = rows["after"]["ref_p"]
        print(f"{tag:<7}{'after':>8}{rows['after']['score']:>10.3f}{rows['after']['n']:>10}"
              f"{rows['after']['rg']:>10.3f}{rows['after']['bg']:>10.3f}"
              f"{np.round(ours_p, 3)!s:>26}{np.round(ref_p, 3)!s:>26}")
        print(f"{'':<7}{'before':>8}{rows['before']['score']:>10.3f}{rows['before']['n']:>10}"
              f"{rows['before']['rg']:>10.3f}{rows['before']['bg']:>10.3f}"
              f"{np.round(rows['before']['ours_p'], 3)!s:>26}{np.round(rows['before']['ref_p'], 3)!s:>26}")


if __name__ == "__main__":
    main()
