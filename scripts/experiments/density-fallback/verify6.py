"""Stage 0, part 2: what gains does each problem frame actually need, and can
the film base itself supply them (so a preset generalises to unseen inputs)?

For every frame we take the pixels that Capture One renders as neutral, and fit
  out_c = u_c * (D_c - b0)
so that those pixels come out neutral in our density domain. Then we compare the
fitted gains with the film base's own channel differences.
"""

import sys

sys.path.insert(0, "target/pylibs")

import numpy as np  # noqa: E402
import rawpy  # noqa: E402
from PIL import Image  # noqa: E402

from verify2 import SRGB_TO_PROPHOTO, W, absorption, app_style_base, srgb_decode  # noqa: E402

sys.stdout.reconfigure(encoding="utf-8")

RAFDIR = "test_picture/富士raf"
RAFS = {
    "1942": (100, 10, 762),
    "1947": (100, 10, 762),
    "1949": (90, 4, 770),
    "1950": (92, 2, 772),
}


def fit(points, grid=np.arange(-0.30, 0.06, 0.002)):
    """Least squares u_R, u_B for every candidate shared black point."""
    best = None
    for b0 in grid:
        rows, target = [], []
        for point in points:
            rows.append([point[0] - b0, 0.0]); target.append(point[1] - b0)
            rows.append([0.0, point[2] - b0]); target.append(point[1] - b0)
        gains, *_ = np.linalg.lstsq(np.array(rows), np.array(target), rcond=None)
        out = np.stack([gains[0] * (points[:, 0] - b0), points[:, 1] - b0,
                        gains[1] * (points[:, 2] - b0)], axis=1)
        green = np.maximum(np.abs(out[:, 1]), 1e-6)
        error = float(np.median(np.maximum(np.abs(out[:, 0] / green - 1),
                                           np.abs(out[:, 2] / green - 1))))
        if best is None or error < best[0]:
            best = (error, b0, gains)
    return best


def raf_frame(tag, x0, y0, side):
    with rawpy.imread(f"{RAFDIR}/Capture One Catalog{tag}.raf") as raw:
        rgb = raw.postprocess(half_size=True, output_bps=16, gamma=(1, 1),
                              no_auto_bright=True, use_camera_wb=False,
                              output_color=rawpy.ColorSpace.ProPhoto)
        cam_wb = np.asarray(raw.camera_whitebalance, dtype=np.float32)
    lin = rgb.astype(np.float32) / 65535.0 * (cam_wb[:3] / cam_wb[:3].max())
    del rgb
    base = app_style_base(lin)
    small = lin[::4, ::4]
    block = (absorption(small[y0:y0 + side, x0:x0 + side]) - base)[::-1, ::-1]
    reference = np.asarray(
        Image.open(f"{RAFDIR}/Capture One Catalog{tag}.tif").convert("RGB")
        .resize((side, side), Image.LANCZOS), dtype=np.float32) / 255.0
    reference = srgb_decode(reference) @ SRGB_TO_PROPHOTO.T
    green = np.maximum(reference[:, :, 1], 1e-6)
    level = reference @ W
    chroma_neutral = ((np.abs(reference[:, :, 0] / green - 1) < 0.05)
                      & (np.abs(reference[:, :, 2] / green - 1) < 0.06))
    return base, block, level, chroma_neutral


def main():
    tags = [f"{1942 + step}" for step in range(11)]
    print(f"{'frame':<8}{'base R-G':>9}{'base B-G':>9}"
          f"{'u_R':>6}{'u_B':>6}{'b0':>7}{'残余':>7}"
          f"{' | 亮部 u_R':>11}{'u_B':>6}{'残余':>7}")
    for tag in tags:
        x0, y0, side = (100, 10, 762) if tag not in ("1949", "1950") else (
            (90, 4, 770) if tag == "1949" else (92, 2, 772))
        base, block, level, chroma_neutral = raf_frame(tag, x0, y0, side)
        mask = chroma_neutral & (level > 0.02) & (level < 0.75)
        points = block[mask]
        step = max(1, len(points) // 120000)
        error, b0, gains = fit(points[::step])
        bright = mask & (level > 0.08)
        bright_points = block[bright]
        step_b = max(1, len(bright_points) // 80000)
        error_b, b0_b, gains_b = fit(bright_points[::step_b])
        print(f"{tag:<8}{base[0] - base[1]:>9.3f}{base[2] - base[1]:>9.3f}"
              f"{gains[0]:>6.2f}{gains[1]:>6.2f}{b0:>7.3f}{error * 100:>6.1f}%"
              f"{' |':>4}{gains_b[0]:>9.2f}{gains_b[1]:>6.2f}{error_b * 100:>6.1f}%")

    print()
    for tag, (x0, y0, side) in RAFS.items():
        with rawpy.imread(f"{RAFDIR}/Capture One Catalog{tag}.raf") as raw:
            rgb = raw.postprocess(half_size=True, output_bps=16, gamma=(1, 1),
                                  no_auto_bright=True, use_camera_wb=False,
                                  output_color=rawpy.ColorSpace.ProPhoto)
            cam_wb = np.asarray(raw.camera_whitebalance, dtype=np.float32)
        lin = rgb.astype(np.float32) / 65535.0 * (cam_wb[:3] / cam_wb[:3].max())
        del rgb
        base = app_style_base(lin)
        small = lin[::4, ::4]
        block = (absorption(small[y0:y0 + side, x0:x0 + side]) - base)[::-1, ::-1]
        reference = np.asarray(
            Image.open(f"{RAFDIR}/Capture One Catalog{tag}.tif").convert("RGB")
            .resize((side, side), Image.LANCZOS), dtype=np.float32) / 255.0
        reference = srgb_decode(reference) @ SRGB_TO_PROPHOTO.T
        green = np.maximum(reference[:, :, 1], 1e-6)
        level = reference @ W
        neutral = ((np.abs(reference[:, :, 0] / green - 1) < 0.05)
                   & (np.abs(reference[:, :, 2] / green - 1) < 0.06)
                   & (level > 0.02) & (level < 0.75))
        points = block[neutral]
        step = max(1, len(points) // 120000)
        points = points[::step]
        error, b0, gains = fit(points)
        print(f"  {tag}: 中性像素 {len(points)}，u_R {gains[0]:.3f} u_B {gains[1]:.3f} "
              f"b0 {b0:+.3f}，残余 {error * 100:.1f}%")

    # The panorama, using the six neutral materials as the reference set.
    linear = np.load("scripts/experiments/density-fallback/pano_stride8.npy")
    base = absorption(np.median(linear[:24, :, :].reshape(-1, 3), axis=0))
    D = absorption(linear)
    boxes = [(6420, 7040, 500, 2200), (6420, 7040, 2600, 4200), (6420, 7040, 4500, 6300),
             (2520, 2810, 300, 2100), (3200, 5100, 190, 370), (3200, 5100, 1450, 2000)]
    points = []
    for y0, y1, x0, x1 in boxes:
        block = (D[y0 // 8:y1 // 8, x0 // 8:x1 // 8] - base).reshape(-1, 3)
        points.append(block[::7])
    points = np.vstack(points)
    error, b0, gains = fit(points)
    print(f"  7587-2: 中性材料 {len(points)} 点，u_R {gains[0]:.3f} u_B {gains[1]:.3f} "
          f"b0 {b0:+.3f}，残余 {error * 100:.1f}%")


if __name__ == "__main__":
    main()
