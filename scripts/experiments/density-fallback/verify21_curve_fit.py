"""Fit the print-response curve that maps our render onto the C1 references.

For every repro frame the C1 export of the same film area exists. Register our
rule-5 render to it (orientation search + phase correlation), then collect
(ours -> C1) pairs of luminance and of R/G, B/G per luminance band. If the
mapping is consistent across all eleven frames, it is a property of the film
through this pipeline - a fixed, prescribable print curve - and not a per-frame
estimation. That is exactly how a film preset is calibrated once and reused.
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
SIZE = 512
BINS = 24

LUMA = np.array([0.2126, 0.7152, 0.0722], dtype=np.float32)


def load(path):
    return np.asarray(
        Image.open(path).convert("RGB").resize((SIZE, SIZE), Image.LANCZOS),
        dtype=np.float64,
    ) / 255.0


def register(ours, ref):
    """Return `ours` rotated/shifted to match `ref`; correlation quality."""
    ref_l = ref @ LUMA
    best = (None, -2.0)
    for rot in range(4):
        candidate = np.rot90(ours, rot)
        cand_l = candidate @ LUMA
        # Phase correlation on the mean-removed luminance.
        a = np.fft.fft2(cand_l - cand_l.mean())
        b = np.fft.fft2(ref_l - ref_l.mean())
        cross = np.fft.ifft2(a * np.conj(b))
        peak = np.unravel_index(np.argmax(np.abs(cross)), cross.shape)
        shift = [
            peak[0] if peak[0] <= SIZE // 2 else peak[0] - SIZE,
            peak[1] if peak[1] <= SIZE // 2 else peak[1] - SIZE,
        ]
        rolled = np.roll(candidate, shift, axis=(0, 1))
        corr = np.corrcoef((rolled @ LUMA).ravel(), ref_l.ravel())[0, 1]
        if corr > best[1]:
            best = (rolled, corr)
    return best


def main():
    luma_pairs = []  # (ours_luma, c1_luma, tag)
    rg_pairs = []    # (ours_rg, c1_rg, ours_luma, tag)
    bg_pairs = []
    correlations = []
    for tag in TAGS:
        ours_path = f"{OUT}/raf-{tag}-after-frame.png"
        if not os.path.exists(ours_path):
            ours_path = f"{OUT}/raf-{tag}-after.png"
        ours = load(ours_path)
        ref = load(f"{RAFDIR}/Capture One Catalog{tag}.tif")
        aligned, corr = register(ours, ref)
        correlations.append((tag, corr))
        ol = aligned @ LUMA
        rl = ref @ LUMA
        mask = (
            (ol > 0.004) & (ol < 0.996) & (rl > 0.004) & (rl < 0.996)
            & (aligned @ LUMA * 0 + aligned.min(axis=2) > 0.004)
            & (ref.min(axis=2) > 0.004)
        )
        luma_pairs.append((ol[mask], rl[mask], tag))
        rg_pairs.append(
            (aligned[mask][:, 0] / aligned[mask][:, 1],
             ref[mask][:, 0] / ref[mask][:, 1],
             ol[mask], tag)
        )
        bg_pairs.append(
            (aligned[mask][:, 2] / aligned[mask][:, 1],
             ref[mask][:, 2] / ref[mask][:, 1],
             ol[mask], tag)
        )
    print("配准质量:", ", ".join(f"{t}:{c:.3f}" for t, c in correlations))
    good = {t for t, c in correlations if c >= 0.60}
    print(f"参与拟合的帧（配准≥0.60）: {sorted(good)}")
    luma_pairs = [(a, b, t) for a, b, t in luma_pairs if t in good]
    rg_pairs = [(a, b, c, t) for a, b, c, t in rg_pairs if t in good]
    bg_pairs = [(a, b, c, t) for a, b, c, t in bg_pairs if t in good]

    # Global luminance response: median C1 luminance per bin of ours.
    all_ours = np.concatenate([p[0] for p in luma_pairs])
    all_ref = np.concatenate([p[1] for p in luma_pairs])
    print(f"\n亮度响应（{len(all_ours)} 像素，按我们的亮度分 {BINS} 档）:")
    print(f"{'我们':>7}{'C1中位':>8}{'C1的25%':>8}{'C1的75%':>8}{'跨帧离散':>9}")
    edges = np.quantile(all_ours, np.linspace(0, 1, BINS + 1))
    for i in range(BINS):
        m = (all_ours >= edges[i]) & (all_ours < edges[i + 1])
        if m.sum() < 500:
            continue
        per_frame = []
        for ours_f, ref_f, _ in luma_pairs:
            fm = m[: 0]  # placeholder; per-frame binning below
        # per-frame spread: bin each frame separately at the same ours-range
        spread = []
        for ours_f, ref_f, _ in luma_pairs:
            fm = (ours_f >= edges[i]) & (ours_f < edges[i + 1])
            if fm.sum() > 100:
                spread.append(np.median(ref_f[fm]))
        if len(spread) < 6:
            continue
        spread = np.array(spread)
        print(
            f"{edges[i]:.3f}  {np.median(all_ref[m]):.3f}  "
            f"{np.quantile(spread, 0.25):.3f}  {np.quantile(spread, 0.75):.3f}  "
            f"{np.quantile(spread, 0.75) - np.quantile(spread, 0.25):.3f}"
        )

    for label, pairs in [("R/G", rg_pairs), ("B/G", bg_pairs)]:
        ours_all = np.concatenate([p[0] for p in pairs])
        ref_all = np.concatenate([p[1] for p in pairs])
        luma_all = np.concatenate([p[2] for p in pairs])
        print(f"\n{label} 响应（按我们的亮度分档）:")
        print(f"{'我们亮度':>8}{'我们比值':>8}{'C1比值':>8}{'Δ':>8}{'跨帧IQR':>8}")
        for i in range(6):
            lo, hi = i / 6, (i + 1) / 6
            m = (luma_all >= lo) & (luma_all < hi)
            if m.sum() < 500:
                continue
            per_frame = []
            for p0, p1, pl, _ in pairs:
                fm = (pl >= lo) & (pl < hi)
                if fm.sum() > 100:
                    per_frame.append(np.median(p1[fm]) - np.median(p0[fm]))
            per_frame = np.array(per_frame) if per_frame else np.array([np.nan])
            print(
                f"{(lo + hi) / 2:.3f}  {np.median(ours_all[m]):.3f}  "
                f"{np.median(ref_all[m]):.3f}  "
                f"{np.median(ref_all[m]) - np.median(ours_all[m]):+.3f}  "
                f"{np.quantile(per_frame, 0.75) - np.quantile(per_frame, 0.25):.3f}"
            )


if __name__ == "__main__":
    main()
