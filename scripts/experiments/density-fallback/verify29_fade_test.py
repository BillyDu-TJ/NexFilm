"""Does a luma-faded normalizer fix the over-corrected skies?

The midtone-measured cast is a frame constant, but applying it at full
strength into the highlights pushed already-blue skies over (1942's sky went
from B/G 1.045 to 1.160 while C1's sits at 1.005). This variant fades the
normalizer's strength toward the tonal extremes - full where it was measured
(luma 0.30-0.70), down to FADE_FLOOR at 0.05 and 0.95 - and checks that the
frame-mean convergence survives and the sky calms down.
"""

import json
import os
import sys

sys.path.insert(0, "target/pylibs")

import importlib.util  # noqa: E402
import math  # noqa: E402

import numpy as np  # noqa: E402

sys.stdout.reconfigure(encoding="utf-8")
_here = os.path.dirname(os.path.abspath(__file__))
_spec = importlib.util.spec_from_file_location(
    "verify28", os.path.join(_here, "verify28_apply.py")
)
v28 = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(v28)

OUT = "target/prescribed-response"
SHIPPING = "target/prescribed-response-shipping"
FADE_FLOOR = 0.25


def fade_weights(luma):
    """1 inside [0.30, 0.70], cosine down to FADE_FLOOR at 0.05 / 0.95."""
    w = np.ones_like(luma)
    low = (luma < 0.30) & (luma > 0.05)
    high = (luma > 0.70) & (luma < 0.95)
    w[low] = 0.5 - 0.5 * np.cos(np.pi * (luma[low] - 0.05) / 0.25)
    w[high] = 0.5 - 0.5 * np.cos(np.pi * (0.95 - luma[high]) / 0.25)
    w[luma <= 0.05] = 0.0
    w[luma >= 0.95] = 0.0
    return FADE_FLOOR + (1.0 - FADE_FLOOR) * w


def normalize_faded(image, estimator="gray_edge", lam=1.0, bound=v28.GRAY_BOUND):
    lr, lb = v28.estimate_cast(image, estimator)
    gr = np.exp(np.clip(-lam * lr, -bound, bound))
    gb = np.exp(np.clip(-lam * lb, -bound, bound))
    luma = image @ v28.LUMA
    s = fade_weights(luma)
    gr_p = s * gr + (1.0 - s)
    gb_p = s * gb + (1.0 - s)
    k = 0.2126 * gr_p + 0.7152 + 0.0722 * gb_p
    out = np.stack(
        [image[:, :, 0] * gr_p / k, image[:, :, 1] / k, image[:, :, 2] * gb_p / k],
        axis=2,
    )
    return out, (float(gr), float(gb))


def region_stats(image, quantile=0.8):
    luma = image @ v28.LUMA
    mask = luma > np.quantile(luma, quantile)
    c = image[mask].mean(axis=0)
    return c[0] / c[1], c[2] / c[1]


def main():
    with open(f"{OUT}/profile.json", encoding="utf-8") as handle:
        profile = v28.Profile(json.load(handle))

    print(f"帧均值 R/G, B/G   （渐隐 floor={FADE_FLOOR}）")
    print(f"{'帧':>5} | {'出货':>13} | {'新(无渐隐)':>13} | {'新(渐隐)':>13} | {'C1':>13}")
    for tag in v28.TAGS:
        frame_path = f"{OUT}/raf-{tag}-after-frame.png"
        if os.path.exists(frame_path):
            source = v28.load_display(frame_path)
        else:
            full = v28.load_display(f"{OUT}/raf-{tag}-after.png")
            m, n = full.shape[0], full.shape[1]
            source = full[m // 5 : m - m // 5, n // 5 : n - n // 5]
        ship_path = f"{SHIPPING}/raf-{tag}-after-frame.png"
        shipping = (
            v28.load_display(ship_path)
            if os.path.exists(ship_path)
            else None
        )
        ref = v28.load_display(f"test_picture/富士raf/Capture One Catalog{tag}.tif")
        profiled = v28.apply_profile(source, profile)
        plain, _ = v28.normalize(profiled, "gray_edge", 1.0)
        faded, gains = normalize_faded(profiled)

        def cell(image):
            if image is None:
                return " " * 13
            rg, bg = v28.frame_mean_chroma(image)
            return f"{rg:.3f}, {bg:.3f}".rjust(13)

        note = ""
        if tag == "1949":
            note = f"   <- 增益 {gains[0]:.3f},{gains[1]:.3f}"
        print(
            f"{tag:>5} | {cell(shipping)} | {cell(plain)} | {cell(faded)} |"
            f" {cell(ref)}{note}"
        )

    print("\n1942 天空（最亮 20% 像素 R/G, B/G）:")
    source = v28.load_display(f"{OUT}/raf-1942-after-frame.png")
    profiled = v28.apply_profile(source, profile)
    plain, _ = v28.normalize(profiled, "gray_edge", 1.0)
    faded, _ = normalize_faded(profiled)
    ref = v28.load_display("test_picture/富士raf/Capture One Catalog1942.tif")
    ship = v28.load_display(f"{SHIPPING}/raf-1942-after-frame.png")
    for name, image in (("出货", ship), ("rule5", source), ("新无渐隐", plain), ("新渐隐", faded), ("C1", ref)):
        rg, bg = region_stats(image)
        print(f"  {name}: {rg:.3f}, {bg:.3f}")


if __name__ == "__main__":
    main()
