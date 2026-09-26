"""Fit the per-channel density-to-display LUT against the C1 references.

The density dumps (`prescribed_response_density_dump`) carry each frame's
relative density on its confirmed film area. This script registers each dump
to the C1 export of the same frame (via the framed display render), fits the
per-channel curve relD -> C1 display, checks that the curve is consistent
across frames - one film stock, one capture setup, so one curve - and previews
a render driven entirely by the LUT, bypassing the linear window.

A consistent curve is a film preset: calibrated once from the references,
applied as a constant afterwards. No per-frame estimation is involved.
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
LUT_POINTS = 33
CORRELATION_FLOOR = 0.60
RELD_MIN, RELD_MAX = -1.5, 4.5

LUMA = np.array([0.2126, 0.7152, 0.0722], dtype=np.float64)


def load_display(path):
    return np.asarray(
        Image.open(path).convert("RGB").resize((SIZE, SIZE), Image.LANCZOS),
        dtype=np.float64,
    ) / 255.0


# Crop dimensions printed by the [DUMP] lines of the Rust dump run.
RELD_DIMS = {
    "1942": (3024, 2558), "1943": (714, 1078), "1944": (4140, 3104),
    "1945": (4140, 3104), "1946": (3063, 3039), "1947": (3063, 3039),
    "1948": (3037, 2247), "1949": (2959, 3052), "1950": (3050, 2649),
    "1951": (4140, 3104), "1952": (4140, 3104),
}


def load_reld(path, width, height):
    raw = np.fromfile(path, dtype="<f4", count=width * height * 3)
    raw = raw.reshape(height, width, 3).astype(np.float64)
    image = Image.fromarray((np.clip((raw + 0.0) / 6.0, 0, 1) * 255).astype(np.uint8))
    scaled = np.asarray(image.resize((SIZE, SIZE), Image.LANCZOS), dtype=np.float64) / 255.0 * 6.0
    return scaled


def registration(render, ref):
    """Rotation + phase-correlation shift that aligns `render` to `ref`."""
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


def transform(image, rot, shift):
    return np.roll(np.rot90(image, rot), shift, axis=(0, 1))


def rolling_lut(ours_values, ref_values, points=LUT_POINTS):
    """Monotone ref-value curve against our value, robust rolling median."""
    order = np.argsort(ours_values)
    o = ours_values[order]
    r = ref_values[order]
    window = max(1, len(r) // 60)
    smooth = np.convolve(r, np.ones(window) / window, mode="valid")
    xs = np.quantile(o, np.linspace(0.01, 0.99, points))
    idx = np.searchsorted(o, xs).clip(0, len(smooth) - 1)
    ys = np.maximum.accumulate(smooth[idx])
    return xs, ys


def apply_lut(image_reld, luts):
    out = np.zeros_like(image_reld)
    for c, (xs, ys) in enumerate(luts):
        out[:, :, c] = np.interp(
            image_reld[:, :, c], xs, ys, left=ys[0], right=ys[-1]
        )
    return out


def main():
    data = {}
    print("配准（渲染 vs C1，旋转/位移）:")
    for tag in TAGS:
        reld_path = f"{OUT}/reld-{tag}.f32"
        render_path = f"{OUT}/raf-{tag}-after-frame.png"
        if not os.path.exists(reld_path) or not os.path.exists(render_path):
            continue
        width, height = RELD_DIMS[tag]
        reld = load_reld(reld_path, width, height)
        render = load_display(render_path)
        ref = load_display(f"{RAFDIR}/Capture One Catalog{tag}.tif")
        rot, shift, corr = registration(render, ref)
        print(f"  {tag}: rot={rot * 90}° shift={shift} corr={corr:.3f}")
        data[tag] = {
            "reld": transform(reld, rot, shift),
            "ref": ref,
            "corr": corr,
        }

    good = [t for t, d in data.items() if d["corr"] >= CORRELATION_FLOOR]
    print(f"参与拟合（≥{CORRELATION_FLOOR}）: {sorted(good)}")

    # Per-frame LUTs at fixed relD grid points, for the consistency check.
    grid = np.linspace(0.0, 3.0, 13)
    print("\n逐帧 LUT 一致性（C1 显示值 @ relD）:")
    per_frame = {c: [] for c in range(3)}
    for tag in good:
        reld = data[tag]["reld"]
        ref = data[tag]["ref"]
        row = [tag]
        for c in range(3):
            xs, ys = rolling_lut(reld[:, :, c].ravel(), ref[:, :, c].ravel())
            values = np.interp(grid, xs, ys, left=ys[0], right=ys[-1])
            per_frame[c].append(values)
            row.append(" ".join(f"{v:.2f}" for v in values))
        print(f"  {tag}: " + " | ".join(row))
    print("  relD 网格: " + " ".join(f"{g:.2f}" for g in grid))

    # Pooled LUT over the good frames.
    luts = []
    for c in range(3):
        ours_all = np.concatenate([data[t]["reld"][:, :, c].ravel() for t in good])
        ref_all = np.concatenate([data[t]["ref"][:, :, c].ravel() for t in good])
        m = (ours_all > RELD_MIN + 0.01) & (ours_all < RELD_MAX - 0.01)
        xs, ys = rolling_lut(ours_all[m], ref_all[m])
        luts.append((xs, ys))
    for c, name in enumerate("RGB"):
        xs, ys = luts[c]
        print(f"\n{name} LUT（relD -> 显示值, {len(xs)} 点）:")
        print("  " + " ".join(f"{x:.2f}->{y:.2f}" for x, y in zip(xs, ys)))

    # Preview: render each frame from its relD through the pooled LUT.
    cell = 380
    pad = 6
    label_h = 24
    width = 3 * (cell + pad) + pad
    height = len(TAGS) * (cell + label_h + pad) + pad
    canvas = Image.new("RGB", (width, height), (24, 24, 24))
    draw = ImageDraw.Draw(canvas)
    for column, title in enumerate(["规则5 原样", "relD+LUT 直渲染", "C1 参照"]):
        draw.text((pad + column * (cell + pad) + 4, 4), title, fill=(240, 240, 240))
    for row, tag in enumerate(TAGS):
        top = pad + label_h + row * (cell + label_h + pad)
        draw.text((pad + 4, top + 2), f"raf-{tag}", fill=(180, 220, 255))
        cells = []
        if tag in data:
            lut_render = apply_lut(data[tag]["reld"], luts)
            cells.append(lut_render)
            cells.append(data[tag]["ref"])
        render_path = f"{OUT}/raf-{tag}-after-frame.png"
        if not os.path.exists(render_path):
            render_path = f"{OUT}/raf-{tag}-after.png"
        cells.insert(0, load_display(render_path))
        for column, image in enumerate(cells):
            image8 = Image.fromarray((np.clip(image, 0, 1) * 255).astype(np.uint8))
            canvas.paste(image8, (pad + column * (cell + pad), top + label_h))
    out_path = f"{OUT}/_review-density-lut.png"
    canvas.save(out_path)
    print(f"\nwrote {out_path}")


if __name__ == "__main__":
    main()
