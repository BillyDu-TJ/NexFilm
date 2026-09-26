"""Luminance-binned chroma and level comparison: our render vs the C1 reference.

The frame mean averages casts that move with luminance, and it says nothing
about tonality. This bins the pixels of the Capture One reference by its own
luminance and reports, per band, the mean luminance and the R/G and B/G of
both images - so the divergence shows up as a shape (which band is too bright,
which band drifts chroma) rather than a single number.
"""

import os
import sys

sys.path.insert(0, "target/pylibs")

import numpy as np  # noqa: E402
from PIL import Image  # noqa: E402

sys.stdout.reconfigure(encoding="utf-8")
RAFDIR = "test_picture/富士raf"
OUT = "target/prescribed-response"
TAGS = [str(1942 + step) for step in range(11)]
BANDS = 8


def load_raw(path, size=(600, 600)):
    image = Image.open(path).convert("RGB")
    return np.asarray(image.resize(size, Image.LANCZOS), dtype=np.float32) / 255.0


LUMA = np.array([0.2126, 0.7152, 0.0722], dtype=np.float32)


def best_orientation(ours, ref):
    """The repros are stored upside down relative to C1; pick the rotation
    (identity / 90 / 180 / 270) whose luminance correlates with the reference."""
    ref_small = np.asarray(
        Image.fromarray((ref * 255).astype(np.uint8)).resize((64, 64), Image.BILINEAR),
        dtype=np.float32) / 255.0
    ref_l = ref_small @ LUMA
    best, best_r = 0, -2.0
    for rot in range(4):
        candidate = np.rot90(ours, rot)
        cand_l = np.asarray(
            Image.fromarray((candidate * 255).astype(np.uint8)).resize((64, 64), Image.BILINEAR),
            dtype=np.float32) / 255.0 @ LUMA
        corr = np.corrcoef(cand_l.ravel(), ref_l.ravel())[0, 1]
        if corr > best_r:
            best, best_r = rot, corr
    return np.rot90(ours, best), best_r, best


def load(path, ref=None, size=(600, 600)):
    image = Image.open(path).convert("RGB")
    ours = np.asarray(image.resize(size, Image.LANCZOS), dtype=np.float32) / 255.0
    if ref is None:
        return ours
    ours, corr, rot = best_orientation(ours, ref)
    print(f"    朝向 rot={rot * 90}° 相关={corr:.3f}")
    return ours


def binned(ours, ref):
    """Per-luminance-band stats, banded on the reference's own luminance."""
    rl = ref @ np.array([0.2126, 0.7152, 0.0722], dtype=np.float32)
    ol = ours @ np.array([0.2126, 0.7152, 0.0722], dtype=np.float32)
    edges = np.quantile(rl, np.linspace(0, 1, BANDS + 1))
    edges[0], edges[-1] = -0.01, 1.01
    rows = []
    for i in range(BANDS):
        mask = (rl >= edges[i]) & (rl < edges[i + 1])
        if mask.sum() < 200:
            rows.append(None)
            continue
        rows.append(
            (
                float(rl[mask].mean()),
                float(ol[mask].mean()),
                float(ref[mask][:, 0].mean() / ref[mask][:, 1].mean()),
                float(ref[mask][:, 2].mean() / ref[mask][:, 1].mean()),
                float(ours[mask][:, 0].mean() / ours[mask][:, 1].mean()),
                float(ours[mask][:, 2].mean() / ours[mask][:, 1].mean()),
            )
        )
    return rows


def main():
    which = os.environ.get("BINNED_VARIANT", "after")
    print(f"variant={which}  每档: 参照亮度 | 我们的亮度(Δ) | 参照R/G,B/G | 我们R/G,B/G")
    worst_lum = []
    for tag in TAGS:
        ref = load(f"{RAFDIR}/Capture One Catalog{tag}.tif")
        ours_path = f"{OUT}/raf-{tag}-{which}-frame.png"
        if not os.path.exists(ours_path):
            # No confirmed film area: fall back to the full render, cropped to
            # its central region so the borders and margins stay out of the
            # statistics.
            full = load(f"{OUT}/raf-{tag}-{which}.png")
            m, n = full.shape[0], full.shape[1]
            ours = full[m // 5 : m - m // 5, n // 5 : n - n // 5]
            print("    （无片边框，中心 60% 裁切）")
        else:
            ours = load(ours_path, ref=ref)
        rows = binned(ours, ref)
        print(f"--- {tag}")
        for i, row in enumerate(rows):
            if row is None:
                continue
            rl, ol, rrg, rbg, org, obg = row
            print(
                f"  档{i} {rl:.3f} | {ol:.3f} ({ol - rl:+.3f}) | "
                f"{rrg:.3f},{rbg:.3f} | {org:.3f},{obg:.3f} | dR {org - rrg:+.3f} dB {obg - rbg:+.3f}"
            )
        deltas = [row[1] - row[0] for row in rows if row]
        if deltas:
            worst_lum.append((max(abs(d) for d in deltas), tag))
    worst_lum.sort(reverse=True)
    print("\n亮度偏差最大的帧:", ", ".join(f"{t} ({d:.3f})" for d, t in worst_lum))


if __name__ == "__main__":
    main()
