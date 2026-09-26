"""Fit a luminance-indexed additive colour LUT against the C1 references.

For every luminance band the pooled registered pixels give a robust median of
ours and of C1 per channel; their difference is that band's additive
correction. Texture and noise are untouched (the correction is additive), the
correction is a constant calibration once fitted, and per-frame consistency of
the band corrections is the check that this is a property of the film through
the pipeline rather than of any single frame.
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
BANDS = 12

LUMA = np.array([0.2126, 0.7152, 0.0722], dtype=np.float64)


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


def main():
    frames = {}
    for tag in TAGS:
        render_path = f"{OUT}/raf-{tag}-after-frame.png"
        if not os.path.exists(render_path):
            continue
        render = load_display(render_path)
        ref = load_display(f"{RAFDIR}/Capture One Catalog{tag}.tif")
        rot, shift, corr = register(render, ref)
        frames[tag] = {
            "render": np.roll(np.rot90(render, rot), shift, axis=(0, 1)),
            "ref": ref,
            "corr": corr,
        }
    good = [t for t, d in frames.items() if d["corr"] >= CORRELATION_FLOOR]
    print(f"拟合帧: {sorted(good)}")

    # Per-band additive correction, pooled over the good frames.
    ours_l = np.concatenate([(frames[t]["render"] @ LUMA).ravel() for t in good])
    edges = np.quantile(ours_l, np.linspace(0, 1, BANDS + 1))
    edges[0], edges[-1] = -0.01, 1.01
    band_centers, band_lut = [], []
    per_frame_offsets = {t: [] for t in good}
    print("\n按亮度分档的加性校正（C1 中位 - 我们中位）:")
    print(f"{'档':>3}{'亮度':>7}" + "".join(f"  d{name:>5}" for name in "RGB") + "   逐帧R校正 IQR")
    for i in range(BANDS):
        m_pooled = (ours_l >= edges[i]) & (ours_l < edges[i + 1])
        row = [None] * 3
        for c in range(3):
            ours_c = np.concatenate(
                [frames[t]["render"][:, :, c].ravel() for t in good]
            )
            ref_c = np.concatenate([frames[t]["ref"][:, :, c].ravel() for t in good])
            row[c] = np.median(ref_c[m_pooled] - ours_c[m_pooled])
        for t in good:
            l = frames[t]["render"] @ LUMA
            m = (l >= edges[i]) & (l < edges[i + 1])
            if m.sum() > 300:
                per_frame_offsets[t].append(
                    [np.median(frames[t]["ref"][:, :, c][m] - frames[t]["render"][:, :, c][m])
                     for c in range(3)]
                )
        center = float(np.median(ours_l[m_pooled]))
        spread = (
            np.quantile([o[0] for o in per_frame_offsets[t] for t in good], 0.75)
            - np.quantile([o[0] for o in per_frame_offsets[t] for t in good], 0.25)
            if per_frame_offsets[good[0]] else float("nan")
        )
        band_centers.append(center)
        band_lut.append(row)
        print(
            f"{i:>3}{center:>7.3f}"
            + "".join(f"  {row[c]:+7.3f}" for c in range(3))
            + f"   {spread:6.3f}"
        )
    band_centers = np.array(band_centers)
    band_lut = np.array(band_lut)

    def apply_lut(image):
        luma = image @ LUMA
        out = image.copy()
        for c in range(3):
            out[:, :, c] = image[:, :, c] + np.interp(
                luma, band_centers, band_lut[:, c],
                left=band_lut[0, c], right=band_lut[-1, c],
            )
        return out

    print("\n逐帧中位绝对差（R/G/B，前=规则5，后=加 LUT）:")
    for tag in sorted(frames):
        d = frames[tag]
        corrected = apply_lut(d["render"])
        before = [
            np.median(np.abs(d["render"][:, :, c] - d["ref"][:, :, c]))
            for c in range(3)
        ]
        after = [
            np.median(np.abs(corrected[:, :, c] - d["ref"][:, :, c]))
            for c in range(3)
        ]
        print(
            f"  {tag}: R {before[0]:.3f}->{after[0]:.3f}  "
            f"G {before[1]:.3f}->{after[1]:.3f}  B {before[2]:.3f}->{after[2]:.3f}"
        )

    cell = 380
    pad = 6
    label_h = 24
    width = 3 * (cell + pad) + pad
    height = len(frames) * (cell + label_h + pad) + pad
    canvas = Image.new("RGB", (width, height), (24, 24, 24))
    draw = ImageDraw.Draw(canvas)
    for column, title in enumerate(["规则5 原样", "规则5 + 亮度LUT", "C1 参照"]):
        draw.text((pad + column * (cell + pad) + 4, 4), title, fill=(240, 240, 240))
    for row, tag in enumerate(sorted(frames)):
        d = frames[tag]
        corrected = apply_lut(d["render"])
        original = load_display(f"{OUT}/raf-{tag}-after-frame.png")
        top = pad + label_h + row * (cell + label_h + pad)
        draw.text((pad + 4, top + 2), f"raf-{tag}", fill=(180, 220, 255))
        for column, image in enumerate([original, corrected, d["ref"]]):
            image8 = Image.fromarray((np.clip(image, 0, 1) * 255).astype(np.uint8))
            canvas.paste(image8, (pad + column * (cell + pad), top + label_h))
    out_path = f"{OUT}/_review-luma-lut.png"
    canvas.save(out_path)
    print(f"\nwrote {out_path}")


if __name__ == "__main__":
    main()
