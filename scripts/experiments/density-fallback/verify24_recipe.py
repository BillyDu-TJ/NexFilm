"""Iterate the full evidence-frame recipe against the C1 references.

Recipe under test, applied per frame on top of the density dump:
  1. robust percentile window: black/white points from the frame's own
     luminance percentiles (p_lo/p_hi) instead of extreme endpoints, so the
     per-frame span stops depending on where the noise tail happens to reach;
  2. the film's channel differences ride the base-position equalization
     (already shipped as rule 5);
  3. a fixed per-channel print curve fitted once from the well-registered
     frames against their C1 exports.

Everything is tunable from the environment so the iteration happens here,
in Python, before anything is baked into Rust:
  NORM_LO / NORM_HI   percentile pair for the robust window (default 3/97)
  CURVE_LO / CURVE_HI percentile pair the fixed curve maps to (default 6/94)
  BOUND               how far the percentile points may sit from their fixed
                      display targets (default 0.15)
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

NORM_LO = float(os.environ.get("NORM_LO", 3))
NORM_HI = float(os.environ.get("NORM_HI", 97))
CURVE_LO = float(os.environ.get("CURVE_LO", 6))
CURVE_HI = float(os.environ.get("CURVE_HI", 94))
BOUND = float(os.environ.get("BOUND", 0.15))

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
    scaled = np.asarray(
        im.resize((SIZE, SIZE), Image.LANCZOS), dtype=np.float64
    ) / 255.0 * 6.0
    return scaled


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


CANDIDATES = {
    "1942": (1.860, 2.169, 2.647), "1943": (1.706, 1.979, 2.519),
    "1944": (1.426, 1.686, 2.081), "1945": (1.421, 1.683, 2.072),
    "1946": (2.008, 2.336, 2.779), "1947": (1.894, 2.221, 2.700),
    "1948": (1.943, 2.264, 2.746), "1949": (1.864, 2.187, 2.670),
    "1950": (1.812, 2.118, 2.620), "1951": (1.417, 1.677, 2.055),
    "1952": (1.482, 1.746, 2.191),
}
SPAN_RATIOS = {
    "1942": (0.880, 1.000, 0.920), "1943": (0.779, 1.000, 0.910),
    "1944": (0.914, 1.000, 0.998), "1945": (0.800, 1.000, 1.025),
    "1946": (0.917, 1.000, 0.953), "1947": (0.813, 1.000, 0.979),
    "1948": (0.834, 1.000, 0.912), "1949": (0.819, 1.000, 0.871),
    "1950": (0.817, 1.000, 0.978), "1951": (0.897, 1.000, 0.950),
    "1952": (0.908, 1.000, 0.882),
}


def robust_window(reld, tag):
    """Robust span scale from the frame's own luminance percentiles (the
    bounded per-frame normalization), with the chroma structure from the film
    measurement: the base lands at one display position in every channel and
    each channel keeps the film's span ratio."""
    luma = reld @ LUMA
    lo = np.quantile(luma, NORM_LO / 100.0)
    hi = np.quantile(luma, NORM_HI / 100.0)
    span = max(hi - lo, 0.05)
    offset = np.array(
        [np.quantile(reld[:, :, c], 0.5) - np.quantile(luma, 0.5) for c in range(3)]
    )
    d_min = lo + offset - 0.06 * span
    d_max = hi + offset + 0.06 * span
    return d_min, d_max


def window_render(reld, d_min, d_max):
    """Linear window mapping with 0.25 D headroom at both ends."""
    span = (d_max - d_min).clip(1.0e-3)
    pos = (reld - d_min[None, None, :]) / span[None, None, :]
    return pos


def fit_curve(pairs, channel):
    ours = np.concatenate([p[0][:, :, channel].ravel() for p in pairs])
    ref = np.concatenate([p[1][:, :, channel].ravel() for p in pairs])
    keep = (ours > 0.01) & (ours < 0.99) & (ref > 0.01) & (ref < 0.99)
    ours, ref = ours[keep], ref[keep]
    order = np.argsort(ours)
    o, r = ours[order], ref[order]
    window = max(1, len(r) // 80)
    smooth = np.convolve(r, np.ones(window) / window, mode="valid")
    xs = np.quantile(o, np.linspace(0.02, 0.98, 24))
    idx = np.searchsorted(o, xs).clip(0, len(smooth) - 1)
    ys = np.maximum.accumulate(smooth[idx])
    return xs, ys


def apply_curve(image, tables):
    out = image.copy()
    for c, (xs, ys) in enumerate(tables):
        out[:, :, c] = np.interp(image[:, :, c], xs, ys, left=ys[0], right=ys[-1])
    return out


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
    print(f"拟合帧: {sorted(good)}  NORM {NORM_LO}/{NORM_HI}  CURVE {CURVE_LO}/{CURVE_HI}  BOUND {BOUND}")

    # Step 1+2: robust window render per frame.
    windows = {}
    for tag, d in frames.items():
        d_min, d_max = robust_window(d["reld"], tag)
        windows[tag] = window_render(d["reld"], d_min, d_max)

    # Step 3: fixed per-channel print curve, fitted on the good frames.
    tables = [fit_curve([(windows[t], frames[t]["ref"]) for t in good], c) for c in range(3)]
    for c, name in enumerate("RGB"):
        xs, ys = tables[c]
        print(f"{name} 打样曲线: " + " ".join(f"{x:.2f}->{y:.2f}" for x, y in zip(xs[::3], ys[::3])))

    # Step 4: global grade - per-channel gain + intercept fitted on the pooled
    # registered pixels of the good frames, absorbing the systematic chroma
    # the robust window's crude anchor leaves behind. Diagonal only: mixing
    # channels amplifies the noise of a starved channel into posterization.
    # Fitted once, applied as a constant; the per-channel curves are then
    # re-fitted after it.
    A = np.concatenate([windows[t].reshape(-1, 3) for t in good])
    Bm = np.concatenate([frames[t]["ref"].reshape(-1, 3) for t in good])
    keep = (A.min(axis=1) > 0.01) & (A.max(axis=1) < 0.99) & (Bm.min(axis=1) > 0.01)
    A, Bm = A[keep], Bm[keep]
    # Additive chroma offsets only: the cast is a systematic additive error,
    # and an additive correction cannot amplify noise or flatten contrast.
    offsets = np.array([np.median(Bm[:, c] - A[:, c]) for c in range(3)])
    print("\n全局加性色度校正:", " ".join(f"{name} {offsets[c]:+.3f}" for c, name in enumerate("RGB")))

    def grade(image):
        return image + offsets[None, None, :]

    graded = {t: np.clip(grade(windows[t]), 0, 1) for t in windows}
    tables = [fit_curve([(graded[t], frames[t]["ref"]) for t in good], c) for c in range(3)]

    # Compose the review grid and per-frame residual stats.
    cell = 380
    pad = 6
    label_h = 24
    width = 3 * (cell + pad) + pad
    height = len(frames) * (cell + label_h + pad) + pad
    canvas = Image.new("RGB", (width, height), (24, 24, 24))
    draw = ImageDraw.Draw(canvas)
    for column, title in enumerate(["规则5 原样", "配方v3（稳健窗口+矩阵+曲线）", "C1 参照"]):
        draw.text((pad + column * (cell + pad) + 4, 4), title, fill=(240, 240, 240))
    print("\n逐帧中位绝对差（R/G/B）:")
    for row, tag in enumerate(sorted(frames)):
        d = frames[tag]
        applied = apply_curve(graded[tag], tables)
        original = load_display(f"{OUT}/raf-{tag}-after-frame.png")
        before = [np.median(np.abs(original[:, :, c] - d["ref"][:, :, c])) for c in range(3)]
        after = [np.median(np.abs(applied[:, :, c] - d["ref"][:, :, c])) for c in range(3)]
        print(
            f"  {tag}: R {before[0]:.3f}->{after[0]:.3f}  "
            f"G {before[1]:.3f}->{after[1]:.3f}  B {before[2]:.3f}->{after[2]:.3f}"
        )
        top = pad + label_h + row * (cell + label_h + pad)
        draw.text((pad + 4, top + 2), f"raf-{tag}", fill=(180, 220, 255))
        for column, image in enumerate([original, applied, d["ref"]]):
            image8 = Image.fromarray((np.clip(image, 0, 1) * 255).astype(np.uint8))
            canvas.paste(image8, (pad + column * (cell + pad), top + label_h))
    out_path = f"{OUT}/_review-recipe.png"
    canvas.save(out_path)
    print(f"\nwrote {out_path}")


if __name__ == "__main__":
    main()
