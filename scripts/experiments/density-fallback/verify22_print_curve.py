"""Fit the prescribed print curve from the C1 references and preview it.

Fits three per-channel display curves (ours -> C1) from the well-registered
frames, applies them to every frame's rule-5 render, and composes a review
grid (ours | curve-applied | C1). The curves are a calibration of the film's
print response through this pipeline, measured once from the references -
the same way a film preset is built - not a per-frame estimation.
"""

import os
import sys

sys.path.insert(0, "target/pylibs")

import numpy as np  # noqa: E402
from PIL import Image, ImageDraw  # noqa: E402

sys.stdout.reconfigure(encoding="utf-8")
RAFDIR = "test_picture/富士raf"
OUT = "target/prescribed-response"
TAGS = [str(1942 + step) for step in range(11)]
SIZE = 512
BINS = 20
CORRELATION_FLOOR = 0.60

LUMA = np.array([0.2126, 0.7152, 0.0722], dtype=np.float64)


def load(path):
    return np.asarray(
        Image.open(path).convert("RGB").resize((SIZE, SIZE), Image.LANCZOS),
        dtype=np.float64,
    ) / 255.0


def register(ours, ref):
    ref_l = ref @ LUMA
    best = (None, -2.0)
    for rot in range(4):
        candidate = np.rot90(ours, rot)
        cand_l = candidate @ LUMA
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


def fit_curves(pairs_by_frame):
    """Per-channel display curve ours -> C1, pooled over the given frames.

    Robust fit: sort all pixels by our value and take a wide rolling median of
    the reference value, sampled at quantile points - that tracks the response
    without per-bin noise, then a running max enforces that a print curve is
    non-decreasing.
    """
    tables = []
    for channel in range(3):
        ours_all = np.concatenate(
            [p[0][:, :, channel].ravel() for p in pairs_by_frame]
        )
        ref_all = np.concatenate(
            [p[1][:, :, channel].ravel() for p in pairs_by_frame]
        )
        order = np.argsort(ours_all)
        ours_sorted = ours_all[order]
        ref_sorted = ref_all[order]
        window = max(1, len(ref_sorted) // 50)
        kernel = np.ones(window) / window
        smooth = np.convolve(ref_sorted, kernel, mode="valid")
        xs = np.quantile(ours_sorted, np.linspace(0.02, 0.98, BINS))
        idx = np.searchsorted(ours_sorted, xs).clip(0, len(smooth) - 1)
        ys = smooth[idx]
        ys = np.maximum.accumulate(ys)
        tables.append((xs, ys))
    return tables


def apply_curve(image, tables):
    out = image.copy()
    for channel, (xs, ys) in enumerate(tables):
        values = out[:, :, channel]
        out[:, :, channel] = np.interp(values, xs, ys, left=ys[0], right=ys[-1])
    return out


def main():
    frames = {}
    correlations = {}
    for tag in TAGS:
        ours_path = f"{OUT}/raf-{tag}-after-frame.png"
        if not os.path.exists(ours_path):
            ours_path = f"{OUT}/raf-{tag}-after.png"
        ours = load(ours_path)
        ref = load(f"{RAFDIR}/Capture One Catalog{tag}.tif")
        aligned, corr = register(ours, ref)
        correlations[tag] = corr
        frames[tag] = (ours, aligned, ref)
    good = [t for t in TAGS if correlations[t] >= CORRELATION_FLOOR]
    print("配准:", ", ".join(f"{t}:{correlations[t]:.3f}" for t in TAGS))
    print(f"参与拟合（≥{CORRELATION_FLOOR}）: {good}")

    pairs = [(frames[t][1], frames[t][2]) for t in good]
    tables = fit_curves(pairs)
    for channel, name in enumerate("RGB"):
        xs, ys = tables[channel]
        print(f"\n{name} 曲线（{len(xs)} 点）:")
        print("  " + " ".join(f"{x:.2f}->{y:.2f}" for x, y in zip(xs, ys)))

    # Per-frame consistency of the fitted curve: median |frame residual|.
    print("\n逐帧曲线残差（我们经曲线后 vs C1，通道中位绝对差）:")
    for t in good:
        aligned, ref = frames[t][1], frames[t][2]
        applied = apply_curve(aligned, tables)
        residuals = [
            np.median(np.abs(applied[:, :, c] - ref[:, :, c])) for c in range(3)
        ]
        before = [
            np.median(np.abs(aligned[:, :, c] - ref[:, :, c])) for c in range(3)
        ]
        print(
            f"  {t}: R {before[0]:.3f}->{residuals[0]:.3f}  "
            f"G {before[1]:.3f}->{residuals[1]:.3f}  B {before[2]:.3f}->{residuals[2]:.3f}"
        )

    # Review grid: ours | curve-applied | C1.
    cell = 380
    pad = 6
    label_h = 24
    width = 3 * (cell + pad) + pad
    height = len(TAGS) * (cell + label_h + pad) + pad
    canvas = Image.new("RGB", (width, height), (24, 24, 24))
    draw = ImageDraw.Draw(canvas)
    for column, title in enumerate(["规则5 原样", "规则5 + 打样曲线", "C1 参照"]):
        draw.text((pad + column * (cell + pad) + 4, 4), title, fill=(240, 240, 240))
    for row, tag in enumerate(TAGS):
        ours, _, ref = frames[tag]
        top = pad + label_h + row * (cell + label_h + pad)
        draw.text((pad + 4, top + 2), f"raf-{tag}", fill=(180, 220, 255))
        applied = apply_curve(ours, tables)
        for column, image in enumerate([ours, applied, ref]):
            image8 = Image.fromarray((np.clip(image, 0, 1) * 255).astype(np.uint8))
            left = pad + column * (cell + pad)
            canvas.paste(image8, (left, top + label_h))
    out_path = f"{OUT}/_review-print-curve.png"
    canvas.save(out_path)
    print(f"\nwrote {out_path}")


if __name__ == "__main__":
    main()
