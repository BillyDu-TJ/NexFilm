"""Fit the film-print model: display = f(M · relD), per-channel monotone
curves over a 3x3 channel mixing of the relative density.

The blue channel of a starved scan is noise-dominated, so no diagonal curve
indexed by the blue channel's own value can reconstruct what C1 renders - the
information lives in the other channels' densities, which is exactly the
crosstalk a print response models. The matrix and the curves are fitted
alternately on the pooled registered pixels of the well-registered frames;
the result is a constant calibration, not a per-frame estimation.
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
CORRELATION_FLOOR = 0.60
LUT_POINTS = 33

LUMA = np.array([0.2126, 0.7152, 0.0722], dtype=np.float64)


def load_reld(tag):
    width, height = {
        "1942": (3024, 2558), "1943": (714, 1078), "1944": (4140, 3104),
        "1945": (4140, 3104), "1946": (3063, 3039), "1947": (3063, 3039),
        "1948": (3037, 2247), "1949": (2959, 3052), "1950": (3050, 2649),
        "1951": (4140, 3104), "1952": (4140, 3104),
    }[tag]
    raw = np.fromfile(
        f"{OUT}/reld-{tag}.f32", dtype="<f4", count=width * height * 3
    ).reshape(height, width, 3).astype(np.float64)
    im = Image.fromarray((np.clip(raw / 6.0, 0, 1) * 255).astype(np.uint8))
    return np.asarray(im.resize((SIZE, SIZE), Image.LANCZOS), dtype=np.float64) / 255.0 * 6.0


def load_display(path):
    return np.asarray(
        Image.open(path).convert("RGB").resize((SIZE, SIZE), Image.LANCZOS),
        dtype=np.float64,
    ) / 255.0


def register(render, ref):
    ref_l = ref @ LUMA
    best = (0, (0, 0), -2.0)
    for rot in range(4):
        candidate = np.rot90(render, rot)
        cand_l = candidate @ LUMA
        a = np.fft.fft2(cand_l - cand_l.mean())
        b = np.fft.fft2(ref_l - ref_l.mean())
        cross = np.fft.ifft2(a * np.conj(b))
        peak = np.unravel_index(np.argmax(np.abs(cross)), cross.shape)
        shift = (
            peak[0] if peak[0] <= SIZE // 2 else peak[0] - SIZE,
            peak[1] if peak[1] <= SIZE // 2 else peak[1] - SIZE,
        )
        rolled = np.roll(candidate, shift, axis=(0, 1))
        corr = np.corrcoef((rolled @ LUMA).ravel(), ref_l.ravel())[0, 1]
        if corr > best[2]:
            best = (rot, shift, corr)
    return best


def fit_curve(ours_values, ref_values):
    order = np.argsort(ours_values)
    o = ours_values[order]
    r = ref_values[order]
    window = max(1, len(r) // 80)
    smooth = np.convolve(r, np.ones(window) / window, mode="valid")
    xs = np.quantile(o, np.linspace(0.01, 0.99, LUT_POINTS))
    idx = np.searchsorted(o, xs).clip(0, len(smooth) - 1)
    ys = np.maximum.accumulate(smooth[idx])
    return xs, ys


def eval_curve(curve, values):
    xs, ys = curve
    return np.interp(values, xs, ys, left=ys[0], right=ys[-1])


def main():
    frames = {}
    for tag in TAGS:
        render_path = f"{OUT}/raf-{tag}-after-frame.png"
        if not os.path.exists(render_path):
            continue
        reld = load_reld(tag)
        render = load_display(render_path)
        ref = load_display(f"{RAFDIR}/Capture One Catalog{tag}.tif")
        rot, shift, corr = register(render, ref)
        frames[tag] = {
            "reld": np.roll(np.rot90(reld, rot), shift, axis=(0, 1)),
            "ref": ref,
            "corr": corr,
        }
    good = [t for t, d in frames.items() if d["corr"] >= CORRELATION_FLOOR]
    print(f"拟合帧: {sorted(good)}")

    A = np.concatenate([frames[t]["reld"].reshape(-1, 3) for t in good])
    B = np.concatenate([frames[t]["ref"].reshape(-1, 3) for t in good])
    keep = (B.min(axis=1) > 0.01) & (B.max(axis=1) < 0.99) & (A.min(axis=1) > -1.4)
    A, B = A[keep], B[keep]
    print(f"拟合像素: {len(A)}")

    # Alternating fit: curves for a fixed matrix, then the matrix for fixed
    # curves (through the curve inverses), three rounds.
    M = np.diag([1.0 / 0.35, 1.0 / 0.35, 1.0 / 0.35])
    curves = None
    for round_index in range(3):
        mixed = A @ M.T
        curves = [fit_curve(mixed[:, c], B[:, c]) for c in range(3)]
        # Linearize: target mixed value for each pixel = curve inverse.
        target = np.empty_like(mixed)
        for c, (xs, ys) in enumerate(curves):
            target[:, c] = np.interp(B[:, c], ys, xs, left=xs[0], right=xs[-1])
        design = np.concatenate([A, np.ones((len(A), 1))], axis=1)
        coef, *_ = np.linalg.lstsq(design, target, rcond=None)
        M = coef[:3].T
        # Rows of M must stay non-negative: a print response cannot invert a
        # channel. Clamp by projection and refit the scale.
        M = np.abs(M)
        residual = B - np.stack(
            [eval_curve(curves[c], (A @ M.T)[:, c]) for c in range(3)], axis=1
        )
        print(
            f"round {round_index}: 中位绝对残差 "
            + " ".join(f"{np.median(np.abs(residual[:, c])):.3f}" for c in range(3))
        )
    print("\n矩阵 M（relD -> 混合密度）:")
    for c, name in enumerate("RGB"):
        print(f"  {name}: {M[c, 0]:+.2f} {M[c, 1]:+.2f} {M[c, 2]:+.2f}")
    for c, name in enumerate("RGB"):
        xs, ys = curves[c]
        print(f"{name} 曲线: " + " ".join(f"{x:.2f}->{y:.2f}" for x, y in zip(xs[::4], ys[::4])))

    # Per-frame residuals of the full model.
    print("\n逐帧中位绝对残差（R/G/B）:")
    for tag in sorted(frames):
        d = frames[tag]
        mixed = d["reld"].reshape(-1, 3) @ M.T
        rendered = np.stack(
            [eval_curve(curves[c], mixed[:, c]) for c in range(3)], axis=1
        ).reshape(d["ref"].shape)
        residuals = [
            np.median(np.abs(rendered[:, :, c] - d["ref"][:, :, c])) for c in range(3)
        ]
        print(
            f"  {tag}: R {residuals[0]:.3f}  G {residuals[1]:.3f}  B {residuals[2]:.3f}"
        )

    # Review grid: rule-5 render | model render | C1.
    cell = 380
    pad = 6
    label_h = 24
    width = 3 * (cell + pad) + pad
    height = len(frames) * (cell + label_h + pad) + pad
    canvas = Image.new("RGB", (width, height), (24, 24, 24))
    draw = ImageDraw.Draw(canvas)
    for column, title in enumerate(["规则5 原样", "打样模型渲染", "C1 参照"]):
        draw.text((pad + column * (cell + pad) + 4, 4), title, fill=(240, 240, 240))
    for row, tag in enumerate(sorted(frames)):
        d = frames[tag]
        mixed = d["reld"].reshape(-1, 3) @ M.T
        rendered = np.stack(
            [eval_curve(curves[c], mixed[:, c]) for c in range(3)], axis=1
        ).reshape(d["ref"].shape)
        original = load_display(f"{OUT}/raf-{tag}-after-frame.png")
        top = pad + label_h + row * (cell + label_h + pad)
        draw.text((pad + 4, top + 2), f"raf-{tag}", fill=(180, 220, 255))
        for column, image in enumerate([original, rendered, d["ref"]]):
            image8 = Image.fromarray((np.clip(image, 0, 1) * 255).astype(np.uint8))
            canvas.paste(image8, (pad + column * (cell + pad), top + label_h))
    out_path = f"{OUT}/_review-print-model.png"
    canvas.save(out_path)
    print(f"\nwrote {out_path}")


if __name__ == "__main__":
    main()
