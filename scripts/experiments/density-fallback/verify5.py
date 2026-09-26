"""Stage 0, part 1: can one per-channel affine (offset + gain) neutralize every
neutral material in the LR panorama at once, or do we need per-channel nuance?

Model A  free affine per channel (out_c = u_c * D_c + v_c)
Model B  base-anchored gains  (out_c = u_c * (D_c - base_c))
Model C  no gain at all       (out_c = D_c - base_c)
"""

import sys

sys.path.insert(0, "target/pylibs")
import numpy as np  # noqa: E402

sys.stdout.reconfigure(encoding="utf-8")

W = np.array([0.2126, 0.7152, 0.0722])
BOXES = [
    ("砂石左", 6420, 7040, 500, 2200),
    ("砂石中", 6420, 7040, 2600, 4200),
    ("砂石右", 6420, 7040, 4500, 6300),
    ("大理石檐部", 2520, 2810, 300, 2100),
    ("柱身左1", 3200, 5100, 190, 370),
    ("柱身左3", 3200, 5100, 1450, 2000),
]
CHECK = [("云A(含蓝天)", 1400, 2200, 4200, 5400), ("云B(含蓝天)", 600, 1000, 2200, 3200)]


def absorption(linear):
    return -np.log10(np.maximum(linear, 1e-7))


def material_points(D, base, y0, y1, x0, x1, bins=6):
    """Per-material densities, split by the material's own lightness so the
    fit sees how the required channel ratio moves with level."""
    block = D[y0 // 8:y1 // 8, x0 // 8:x1 // 8].reshape(-1, 3) - base
    edges = np.quantile(block[:, 1], np.linspace(0, 1, bins + 1))
    edges[-1] += 1e-9
    index = np.clip(np.digitize(block[:, 1], edges) - 1, 0, bins - 1)
    points = []
    for b in range(bins):
        rows = block[index == b]
        if len(rows) > 20:
            points.append(np.median(rows, axis=0))
    return np.array(points)


def residuals(points, transform):
    """Absolute and relative neutrality error per point."""
    out = transform(points)
    absolute = np.sqrt(((out[:, 0] - out[:, 1]) ** 2 + (out[:, 2] - out[:, 1]) ** 2).mean() / 2)
    green = np.maximum(np.abs(out[:, 1]), 1e-6)
    relative = np.maximum(np.abs(out[:, 0] / green - 1), np.abs(out[:, 2] / green - 1))
    return absolute, float(np.max(relative)), float(np.median(relative))


def chroma(points, transform):
    out = transform(points)
    green = np.maximum(out[:, 1], 1e-6)
    return out[:, 0] / green, out[:, 2] / green


def main():
    linear = np.load("scripts/experiments/density-fallback/pano_stride8.npy")
    base = np.median(linear[:24, :, :].reshape(-1, 3), axis=0)
    base = absorption(base)
    D = absorption(linear)
    print("片基密度:", np.round(base, 4))

    sets = {name: material_points(D, base, *box) for name, *box in BOXES}
    all_points = np.vstack(list(sets.values()))

    # Model A: free affine, gauge out_G = D_G.
    # Solve least squares for u_R,v_R,u_B,v_B so that out_c == out_G.
    rows_a, target_a = [], []
    for point in all_points:
        d_r, d_g, d_b = point
        rows_a.append([d_r, 1.0, 0.0, 0.0]); target_a.append(d_g)
        rows_a.append([0.0, 0.0, d_b, 1.0]); target_a.append(d_g)
    solution, *_ = np.linalg.lstsq(np.array(rows_a), np.array(target_a), rcond=None)
    u_r, v_r, u_b, v_b = solution
    affine = lambda p: np.stack([u_r * p[:, 0] + v_r, p[:, 1], u_b * p[:, 2] + v_b], axis=1)

    # Model B: base-anchored, two gains (G fixed at 1).
    def anchored(gains):
        return lambda p: p * gains
    rows_b, target_b = [], []
    for point in all_points:
        rows_b.append([point[0], 0.0]); target_b.append(point[1])
        rows_b.append([0.0, point[2]]); target_b.append(point[1])
    gains, *_ = np.linalg.lstsq(np.array(rows_b), np.array(target_b), rcond=None)

    # The window the software actually uses today.
    inner = D[400 // 8:7000 // 8, 400 // 8:6800 // 8].reshape(-1, 3) - base
    order = np.argsort(inner @ W)
    tail = max(1, int(np.ceil(len(inner) * 0.02)))
    low = inner[order[:tail]].mean(axis=0)
    high = inner[order[-tail:]].mean(axis=0)
    span = high - low
    shipped = lambda p: (p - low) / span
    print("现行窗口 dmin:", np.round(low, 3), "dmax:", np.round(high, 3),
          "span:", np.round(span, 3))
    print(f"Model A 自由仿射   u_R {u_r:.3f} v_R {v_r:+.3f} | u_B {u_b:.3f} v_B {v_b:+.3f}"
          f"  残差 {residuals(all_points, affine)[0]:.4f} D 最大相对误差 {residuals(all_points, affine)[1] * 100:.1f}%")
    print(f"Model B 片基锚定   gains R {gains[0]:.3f} B {gains[1]:.3f}"
          f"  残差 {residuals(all_points, anchored([gains[0], 1.0, gains[1]]))[0]:.4f} D 最大相对误差 {residuals(all_points, anchored([gains[0], 1.0, gains[1]]))[1] * 100:.1f}%")
    print(f"Model C 无增益     残差 {residuals(all_points, anchored([1.0, 1.0, 1.0]))[0]:.4f} D 最大相对误差 {residuals(all_points, anchored([1.0, 1.0, 1.0]))[1] * 100:.1f}%")
    print(f"现行窗口           残差 {residuals(all_points, shipped)[0]:.4f} D 最大相对误差 {residuals(all_points, shipped)[1] * 100:.1f}%")
    print()

    models = [("现行窗口", shipped), ("Model A", affine),
              ("Model B", anchored([gains[0], 1.0, gains[1]]))]

    # Model D: one shared black point in density + per-channel gains.
    best = None
    for b0 in np.arange(-0.06, 0.12, 0.0005):
        rows, target = [], []
        for point in all_points:
            rows.append([point[0] - b0, 0.0]); target.append(point[1] - b0)
            rows.append([0.0, point[2] - b0]); target.append(point[1] - b0)
        solution, *_ = np.linalg.lstsq(np.array(rows), np.array(target), rcond=None)
        transform = lambda p, b0=b0, g=solution: np.stack(
            [g[0] * (p[:, 0] - b0), p[:, 1] - b0, g[1] * (p[:, 2] - b0)], axis=1)
        error = residuals(all_points, transform)[0]
        if best is None or error < best[0]:
            best = (error, b0, solution)
    error, b0, gains_d = best
    shared = lambda p, b0=b0, g=gains_d: np.stack(
        [g[0] * (p[:, 0] - b0), p[:, 1] - b0, g[1] * (p[:, 2] - b0)], axis=1)
    print(f"Model D 共享黑点   b0 {b0:+.3f} D  u_R {gains_d[0]:.3f} u_B {gains_d[1]:.3f}"
          f"  残差 {error:.4f} D 最大相对误差 {residuals(all_points, shared)[1] * 100:.1f}%")
    models.append(("Model D", shared))
    print(f"{'材料':<12}" + "".join(f"{name:>22}" for name, _ in models))
    for name, points in sets.items():
        cells = []
        for _, transform in models:
            rg, bg = chroma(points, transform)
            cells.append(f"{np.median(rg):.3f}/{np.median(bg):.3f}".rjust(22))
        print(f"{name:<12}" + "".join(cells))
    print()
    print("(每格是 R/G ÷ B/G，中性应为 1.000/1.000)")
    print()
    print("不参与拟合的亮部（云，含蓝天，本来就不该完全中性）:")
    for name, y0, y1, x0, x1 in CHECK:
        points = material_points(D, base, y0, y1, x0, x1)
        line = f"  {name:<14}"
        for label, transform in models:
            rg, bg = chroma(points, transform)
            line += f"{label} {np.median(rg):.3f}/{np.median(bg):.3f}   "
        print(line)

    print()
    print("每个材料自身从暗到亮，需要的 B/G（拟合前，片基已减）:")
    for name, points in sets.items():
        bg = points[:, 2] / np.maximum(points[:, 1], 1e-6)
        rg = points[:, 0] / np.maximum(points[:, 1], 1e-6)
        print(f"  {name:<12} dG {points[0,1]:.3f}→{points[-1,1]:.3f}  "
              f"R/G {rg[0]:.3f}→{rg[-1]:.3f}  B/G {bg[0]:.3f}→{bg[-1]:.3f}")


if __name__ == "__main__":
    main()
