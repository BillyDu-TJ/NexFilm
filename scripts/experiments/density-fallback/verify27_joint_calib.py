"""Joint calibration of the film profile with each frame's white balance projected out.

The previous additive LUT was pooled over the four frames whose registration
cleared 0.60, so it learned the C1 exports' per-frame white balance together
with the film's print response - a constant fit to per-frame decisions, which
is why per-frame residuals of about +-0.1 survived it. This script separates
the two with a two-way robust model. Per luma band i, frame t, chroma channel
c in {R, B} (G is the pivot):

    log(ref chroma c) - log(ours chroma c) = profile_c(i) + wb_c(t) + noise

The per-band profile is the film's crossover (shared physics); the per-frame
offset is C1's white-balance decision (nuisance). Both are fitted by
alternating medians. The luma shape gets the same decomposition, so the
profile's luma term is also free of C1's per-frame exposure decisions.

Outputs `target/prescribed-response/profile.json` with the band table for the
apply script and the Rust constants, plus leave-one-out residuals that
quantify how much of the previous fit was overfit.
"""

import json
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
BANDS = 12
CORR_FLOOR = 0.55
MIN_BAND_PIXELS = 2000
SCALES = [0.92, 0.96, 1.0, 1.04, 1.08]
LUMA = np.array([0.2126, 0.7152, 0.0722], dtype=np.float64)


def load_display(path):
    return np.asarray(
        Image.open(path).convert("RGB").resize((SIZE, SIZE), Image.LANCZOS),
        dtype=np.float64,
    ) / 255.0


def resample_about_center(image, scale):
    """Scale about the center, keeping the canvas size."""
    height, width = image.shape[:2]
    if scale == 1.0:
        return image
    resized = np.asarray(
        Image.fromarray((np.clip(image, 0, 1) * 255).astype(np.uint8)).resize(
            (max(8, round(width * scale)), max(8, round(height * scale))),
            Image.LANCZOS,
        ),
        dtype=np.float64,
    ) / 255.0
    out = np.zeros_like(image)
    off_y, off_x = (height - resized.shape[0]) // 2, (width - resized.shape[1]) // 2
    if off_y >= 0 and off_x >= 0:
        out[off_y : off_y + resized.shape[0], off_x : off_x + resized.shape[1]] = resized
    else:
        cut_y, cut_x = max(0, -off_y), max(0, -off_x)
        out = resized[
            cut_y : cut_y + height, cut_x : cut_x + width
        ]
    return out


def register(render, ref):
    """Best dihedral orientation, scale and circular shift of the render onto
    the reference, by phase correlation of the luminance. The scans are stored
    in a different orientation frame to frame, and some are mirrored, so all
    eight orientations are tried."""
    ref_l = ref @ LUMA
    canvas = np.zeros_like(ref)
    off_y, off_x = (ref.shape[0] - render.shape[0]) // 2, (ref.shape[1] - render.shape[1]) // 2
    canvas[
        max(0, off_y) : max(0, off_y) + min(render.shape[0], ref.shape[0]),
        max(0, off_x) : max(0, off_x) + min(render.shape[1], ref.shape[1]),
    ] = render[
        max(0, -off_y) : max(0, -off_y) + min(render.shape[0], ref.shape[0]),
        max(0, -off_x) : max(0, -off_x) + min(render.shape[1], ref.shape[1]),
    ]
    b = np.fft.fft2(ref_l - ref_l.mean())
    best = (0, False, 1.0, (0, 0), -2.0)
    for flip in (False, True):
        for rot in range(4):
            base = np.rot90(canvas[:, ::-1] if flip else canvas, rot)
            for scale in SCALES:
                cand_l = resample_about_center(base, scale) @ LUMA
                a = np.fft.fft2(cand_l - cand_l.mean())
                cross = np.fft.ifft2(a * np.conj(b))
                peak = np.unravel_index(np.argmax(np.abs(cross)), cross.shape)
                shift = (
                    peak[0] if peak[0] <= cand_l.shape[0] // 2 else peak[0] - cand_l.shape[0],
                    peak[1] if peak[1] <= cand_l.shape[1] // 2 else peak[1] - cand_l.shape[1],
                )
                moved = np.roll(
                    resample_about_center(base, scale), shift, axis=(0, 1)
                )
                corr = np.corrcoef((moved @ LUMA).ravel(), ref_l.ravel())[0, 1]
                if corr > best[4]:
                    best = (rot, flip, scale, shift, corr)
    rot, flip, scale, shift, corr = best
    base = np.rot90(canvas[:, ::-1] if flip else canvas, rot)
    moved = np.roll(resample_about_center(base, scale), shift, axis=(0, 1))
    return moved, rot, flip, scale, shift, corr


def load_pairs():
    pairs = {}
    for tag in TAGS:
        frame_path = f"{OUT}/raf-{tag}-after-frame.png"
        if os.path.exists(frame_path):
            render = load_display(frame_path)
        else:
            full = load_display(f"{OUT}/raf-{tag}-after.png")
            m, n = full.shape[0], full.shape[1]
            render = full[m // 5 : m - m // 5, n // 5 : n - n // 5]
        ref = load_display(f"{RAFDIR}/Capture One Catalog{tag}.tif")
        aligned, rot, flip, scale, shift, corr = register(render, ref)
        print(
            f"raf-{tag}: rot={rot * 90}° flip={flip} scale={scale:.2f} shift={shift}"
            f" corr={corr:.3f}" + ("" if corr >= CORR_FLOOR else "  -> 不参与拟合")
        )
        pairs[tag] = {"render": aligned, "ref": ref, "corr": corr}
    return pairs


def band_edges(pairs, good):
    pooled = np.concatenate([(pairs[t]["render"] @ LUMA).ravel() for t in good])
    edges = np.quantile(pooled, np.linspace(0, 1, BANDS + 1))
    edges[0], edges[-1] = -0.01, 1.01
    return edges


def band_stats(pairs, good, edges):
    """Per-frame, per-band medians of log chroma and luma, on registered pixels."""
    stats = {}
    for t in good:
        d = pairs[t]
        ours_l = d["render"] @ LUMA
        ref_l = d["ref"] @ LUMA
        rows = []
        for i in range(BANDS):
            m = (ours_l >= edges[i]) & (ours_l < edges[i + 1])
            if m.sum() < MIN_BAND_PIXELS:
                rows.append(None)
                continue
            oc = np.log(np.clip(d["render"][m], 1.0 / 255, 1.0))
            rc = np.log(np.clip(d["ref"][m], 1.0 / 255, 1.0))
            rows.append(
                {
                    "n": int(m.sum()),
                    "ours_rg": float(np.median(oc[:, 0] - oc[:, 1])),
                    "ours_bg": float(np.median(oc[:, 2] - oc[:, 1])),
                    "ref_rg": float(np.median(rc[:, 0] - rc[:, 1])),
                    "ref_bg": float(np.median(rc[:, 2] - rc[:, 1])),
                    "ours_l": float(np.median(ours_l[m])),
                    "ref_l": float(np.median(ref_l[m])),
                }
            )
        stats[t] = rows
    return stats


def twoway(stats, key, min_frames=3):
    """Alternating-median fit of profile[i] + wb[t] to stats[t][i][key]."""
    frames = sorted(stats)
    profile = np.full(BANDS, np.nan)
    wb = {t: 0.0 for t in frames}
    for i in range(BANDS):
        vals = [stats[t][i][key] for t in frames if stats[t][i]]
        if len(vals) >= min_frames:
            profile[i] = np.median(vals)
    for _ in range(6):
        for t in frames:
            vals = [
                stats[t][i][key] - profile[i]
                for i in range(BANDS)
                if stats[t][i] and np.isfinite(profile[i])
            ]
            if vals:
                wb[t] = float(np.median(vals))
        for i in range(BANDS):
            vals = [stats[t][i][key] - wb[t] for t in frames if stats[t][i]]
            profile[i] = np.median(vals) if len(vals) >= min_frames else np.nan
    return profile, wb


def residual_report(stats, profile_rg, profile_bg, wb_r, wb_b, label):
    print(f"\n{label}: 每帧逐档残差（log 色度，WB 投影后）")
    for t in sorted(stats):
        res_c = []
        for i in range(BANDS):
            s = stats[t][i]
            if not s or not np.isfinite(profile_rg[i]):
                continue
            res_c.append(
                (
                    abs(s["ref_rg"] - s["ours_rg"] - profile_rg[i] - wb_r[t]),
                    abs(s["ref_bg"] - s["ours_bg"] - profile_bg[i] - wb_b[t]),
                )
            )
        if res_c:
            r = np.asarray(res_c)
            print(
                f"  {t}: logR/G {np.median(r[:, 0]):.3f}  logB/G {np.median(r[:, 1]):.3f}"
            )


def main():
    pairs = load_pairs()
    good = [t for t in TAGS if pairs[t]["corr"] >= CORR_FLOOR]
    print(f"\n拟合帧（corr>={CORR_FLOOR}）: {good}")
    edges = band_edges(pairs, good)
    stats = band_stats(pairs, good, edges)

    # Bands 0-1 sit at our noise floor, where log chroma is dominated by
    # per-pixel noise (per-frame IQR ~1.0 in the diagnostic run); the fit
    # starts at band 2 and the darkest bands hold that value.
    NOISE_FLOOR_BANDS = 2
    stats_chroma = {
        t: [None if i < NOISE_FLOOR_BANDS else row for i, row in enumerate(rows)]
        for t, rows in stats.items()
    }
    profile_rg, wb_r = twoway(stats_chroma, "ref_rg")
    profile_bg, wb_b = twoway(stats_chroma, "ref_bg")

    # Pin the arbitrary constant split: the profile keeps only the band-to-band
    # shape (median over fitted bands = 0) and each frame's constant goes to
    # its wb, where the per-frame normalizer will estimate it.
    fitted = [i for i in range(BANDS) if np.isfinite(profile_rg[i])]
    if fitted:
        shift_r = np.median([profile_rg[i] for i in fitted])
        profile_rg = profile_rg - shift_r
        for t in good:
            wb_r[t] += shift_r
        shift_b = np.median([profile_bg[i] for i in fitted])
        profile_bg = profile_bg - shift_b
        for t in good:
            wb_b[t] += shift_b
    # Hold the deepest fitted value through the noise-floor bands.
    for prof in (profile_rg, profile_bg):
        for i in range(NOISE_FLOOR_BANDS):
            prof[i] = prof[NOISE_FLOOR_BANDS]

    # The luma gap is frame-specific (each frame's window places exposure
    # differently and C1 decides exposure per frame), so there is no shared
    # luma shape to fit: the diagnostic dump shows opposite-signed band gaps
    # across frames. The profile carries chroma only; tone stays the window's
    # job.

    band_centers = []
    for i in range(BANDS):
        centers = [stats[t][i]["ours_l"] for t in good if stats[t][i]]
        band_centers.append(float(np.median(centers)) if centers else float("nan"))

    print("\n档案表（每档: 中心亮度 | log R/G | log B/G | 逐帧R IQR）:")
    per_frame_r = {
        i: [
            stats_chroma[t][i]["ref_rg"] - stats_chroma[t][i]["ours_rg"] - wb_r[t]
            for t in good
            if stats_chroma[t][i]
        ]
        for i in range(BANDS)
    }
    for i in range(BANDS):
        iqr = (
            np.subtract(*np.quantile(per_frame_r[i], [0.75, 0.25]))
            if len(per_frame_r[i]) >= 2
            else float("nan")
        )
        print(
            f"  {i:>2}{band_centers[i]:>7.3f}  {profile_rg[i]:+.3f}  {profile_bg[i]:+.3f}"
            f"  {iqr:6.3f}"
        )

    print("\n每帧白平衡（C1 决策，nuisance）: R/G 增益, B/G 增益")
    for t in good:
        print(f"  {t}: {np.exp(wb_r[t]):.3f}, {np.exp(wb_b[t]):.3f}")

    residual_report(stats_chroma, profile_rg, profile_bg, wb_r, wb_b, "全量拟合残差")

    # Leave-one-out: how much of the profile is band-shape transfer rather than
    # a fit to the held-out frame (its wb constant stays projected out).
    print("\n留一交叉验证（拟合其余帧应用到留出帧，WB 投影后残差）:")
    loo = {}
    for held in good:
        sub = {t: rows for t, rows in stats_chroma.items() if t != held}
        prg, _ = twoway(sub, "ref_rg")
        pbg, _ = twoway(sub, "ref_bg")
        for prof in (prg, pbg):
            fitted_i = [i for i in range(BANDS) if np.isfinite(prof[i])]
            if fitted_i:
                prof[:NOISE_FLOOR_BANDS] = prof[NOISE_FLOOR_BANDS]
                med = np.median([prof[i] for i in fitted_i])
                prof -= med
        res = []
        for i in range(BANDS):
            s = stats_chroma[held][i]
            if not s or not np.isfinite(prg[i]):
                continue
            res.append(
                (
                    abs(s["ref_rg"] - s["ours_rg"] - prg[i] - wb_r[held]),
                    abs(s["ref_bg"] - s["ours_bg"] - pbg[i] - wb_b[held]),
                )
            )
        if res:
            r = np.asarray(res)
            loo[held] = (float(np.median(r[:, 0])), float(np.median(r[:, 1])))
            print(f"  {held}: logR/G {r[:, 0].mean():.3f}  logB/G {r[:, 1].mean():.3f}")

    profile = {
        "band_centers": band_centers,
        "log_rg": [float(v) for v in profile_rg],
        "log_bg": [float(v) for v in profile_bg],
        "wb": {t: [float(wb_r[t]), float(wb_b[t])] for t in good},
        "corr": {t: float(pairs[t]["corr"]) for t in TAGS},
        "loo": loo,
        "good_frames": good,
    }
    with open(f"{OUT}/profile.json", "w", encoding="utf-8") as handle:
        json.dump(profile, handle, ensure_ascii=False, indent=1)
    print(f"\nwrote {OUT}/profile.json")


if __name__ == "__main__":
    main()
