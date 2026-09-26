"""Stage 2 check: one shape for every evidence-triggered frame.

  base   the film base the frame measured (kept even when the gate refused it)
  b0     ONE shared black point for all three channels
  span_c each channel keeps its own measured span

Neither a per-family constant nor a per-channel origin is needed. Compared
against what ships today and against the best possible per-channel affine.
"""

import sys

sys.path.insert(0, "target/pylibs")
sys.path.insert(0, "scripts/experiments/density-fallback")

import numpy as np  # noqa: E402

from verify2 import W, absorption, co_sited, share_smart_auto  # noqa: E402
from verify6 import raf_frame  # noqa: E402

sys.stdout.reconfigure(encoding="utf-8")
RAFS = {"1942": (100, 10, 762), "1947": (100, 10, 762),
        "1949": (90, 4, 770), "1950": (92, 2, 772)}
PANO_BOXES = [(6420, 7040, 500, 2200), (6420, 7040, 2600, 4200),
              (6420, 7040, 4500, 6300), (2520, 2810, 300, 2100),
              (3200, 5100, 190, 370), (3200, 5100, 1450, 2000)]


def binned(points, transform, bins=12, floor=0.10):
    out = transform(points)
    edges = np.quantile(out[:, 1], np.linspace(0, 1, bins + 1))
    edges[-1] += 1e-9
    index = np.clip(np.digitize(out[:, 1], edges) - 1, 0, bins - 1)
    errors = []
    for b in range(bins):
        rows = out[index == b]
        if len(rows) < 30:
            continue
        middle = np.median(rows, axis=0)
        if middle[1] < floor:
            continue
        errors.append(max(abs(middle[0] / middle[1] - 1), abs(middle[2] / middle[1] - 1)))
    return (float(np.median(errors)), float(max(errors)), len(errors)) if errors else (0.0, 0.0, 0)


def free_affine(points):
    rows, target = [], []
    for point in points:
        rows.append([point[0], 1.0, 0.0, 0.0]); target.append(point[1])
        rows.append([0.0, 0.0, point[2], 1.0]); target.append(point[1])
    u_r, v_r, u_b, v_b = np.linalg.lstsq(np.array(rows), np.array(target), rcond=None)[0]
    return lambda p: np.stack([u_r * p[:, 0] + v_r, p[:, 1], u_b * p[:, 2] + v_b], axis=1)


def unified(lo, span):
    """Share the black point, keep each channel's own span."""
    b0 = float(lo @ W)
    return lambda p: (p - b0) / span


def show(label, transform, points):
    cells = []
    for floor in (0.10, 0.30):
        median, worst, bins = binned(points, transform, floor=floor)
        cells.append(f"暗部起(>{floor:.2f}) 中位 {median * 100:5.1f}% 最差 {worst * 100:5.1f}%")
    print(f"  {label:<32} " + " | ".join(cells))


def panorama():
    print("=" * 78)
    print("拼接图 _DSC7587-2：统一形状 vs 现行 vs 模型上限")
    print("=" * 78)
    linear = np.load("scripts/experiments/density-fallback/pano_stride8.npy")
    base = absorption(np.median(linear[:24, :, :].reshape(-1, 3), axis=0))
    D = absorption(linear)
    points = np.vstack([(D[y0 // 8:y1 // 8, x0 // 8:x1 // 8] - base).reshape(-1, 3)[::5]
                        for y0, y1, x0, x1 in PANO_BOXES])
    inner = (D[400 // 8:7000 // 8, 400 // 8:6800 // 8] - base).reshape(-1, 3)
    low, high = co_sited(inner[::3])
    span = high - low
    show("现行：逐通道原点 + 逐通道跨度", lambda p: (p - low) / span, points)
    show("统一形状：共享黑点 + 逐通道跨度", unified(low, span), points)
    show("模型上限：自由逐通道仿射", free_affine(points), points)


def raf():
    print()
    print("=" * 78)
    print("欠曝 RAF：把实测片基当锚点后，统一形状的效果")
    print("=" * 78)
    print(f"  {'frame':<7}{'现行兜底':>16}{'统一形状':>16}{'模型上限':>16}")
    for tag, (x0, y0, side) in RAFS.items():
        _, block, level, chroma_neutral = raf_frame(tag, x0, y0, side)
        points = block[chroma_neutral & (level > 0.06)]
        step = max(1, len(points) // 60000)
        points = points[::step]
        low, high, offsets = share_smart_auto(*co_sited(block.reshape(-1, 3)[::3]))
        span = high - low
        line = f"  {tag:<7}"
        for transform in (lambda p: (p - (low + offsets)) / span,
                          unified(co_sited(block.reshape(-1, 3)[::3])[0], span),
                          free_affine(points)):
            median, _, _ = binned(points, transform)
            line += f"{median * 100:15.1f}%"
        print(line + f"   (n={len(points)})")


if __name__ == "__main__":
    panorama()
    raf()
