"""Stage 0, part 3: does the proposed fallback reach the quantitative line, and
does the trigger stay silent on normal material?

Every number here is taken from medians of lightness bins, not from individual
pixels, so that sensor noise is not mistaken for a cast.

C    what ships today (panorama: per-channel co-sited window; RAF: the
     no-base fallback of shared span + clamped offsets)
P1   preset gains, black point pinned to the measured film base
P2   preset gains, one shared black point taken from the frame's low end
FREE a per-channel affine fitted to the same neutral set (upper bound for any
     per-channel linear model in this density domain)
"""

import glob
import sys

sys.path.insert(0, "target/pylibs")
sys.path.insert(0, "scripts/experiments/density-fallback")

import numpy as np  # noqa: E402
import rawpy  # noqa: E402

from verify2 import W, absorption, app_style_base, co_sited, share_smart_auto  # noqa: E402
from verify4 import span_ratio, to_working  # noqa: E402
from verify6 import raf_frame  # noqa: E402

sys.stdout.reconfigure(encoding="utf-8")
RAFDIR = "test_picture/富士raf"
RAFS = {"1942": (100, 10, 762), "1947": (100, 10, 762),
        "1949": (90, 4, 770), "1950": (92, 2, 772)}
PANO_BOXES = [(6420, 7040, 500, 2200), (6420, 7040, 2600, 4200),
              (6420, 7040, 4500, 6300), (2520, 2810, 300, 2100),
              (3200, 5100, 190, 370), (3200, 5100, 1450, 2000)]


def binned(points, transform, bins=12, floor=0.10):
    """Chroma error of the bin medians (systematic part only), skipping bins
    whose output sits in the deepest shadows, where nobody judges colour and
    where sensor noise dominates."""
    out = transform(points)
    edges = np.quantile(out[:, 1], np.linspace(0, 1, bins + 1))
    edges[-1] += 1e-9
    index = np.clip(np.digitize(out[:, 1], edges) - 1, 0, bins - 1)
    errors, absolutes = [], []
    for b in range(bins):
        rows = out[index == b]
        if len(rows) < 30:
            continue
        middle = np.median(rows, axis=0)
        if middle[1] < floor:
            continue
        errors.append(max(abs(middle[0] / middle[1] - 1), abs(middle[2] / middle[1] - 1)))
        absolutes.append(np.sqrt(((middle[0] - middle[1]) ** 2
                                  + (middle[2] - middle[1]) ** 2) / 2))
    if not errors:
        return 0.0, 0.0, 0
    return float(np.median(errors)), float(max(errors)), len(errors)


def fit_gains(points, b0):
    """Preset gains given the black-point rule: out_c = u_c * (D_c - b0)."""
    rows, target = [], []
    for point in points:
        rows.append([point[0] - b0, 0.0]); target.append(point[1] - b0)
        rows.append([0.0, point[2] - b0]); target.append(point[1] - b0)
    gains, *_ = np.linalg.lstsq(np.array(rows), np.array(target), rcond=None)
    return lambda p, g=gains, b=b0: np.stack(
        [g[0] * (p[:, 0] - b), p[:, 1] - b, g[1] * (p[:, 2] - b)], axis=1), gains


def free_affine(points):
    """Best possible per-channel affine for this neutral set (upper bound)."""
    rows, target = [], []
    for point in points:
        rows.append([point[0], 1.0, 0.0, 0.0]); target.append(point[1])
        rows.append([0.0, 0.0, point[2], 1.0]); target.append(point[1])
    u_r, v_r, u_b, v_b = np.linalg.lstsq(np.array(rows), np.array(target),
                                         rcond=None)[0]
    return lambda p: np.stack([u_r * p[:, 0] + v_r, p[:, 1], u_b * p[:, 2] + v_b], axis=1)


def report(label, transform, points):
    median, worst, bins = binned(points, transform)
    print(f"  {label:<40} 中位 {median * 100:5.1f}%  最差档 {worst * 100:5.1f}%"
          f"  ({bins} 档，已排除最深阴影)")
    return median


def panorama():
    print("=" * 78)
    print("拼接图 _DSC7587-2：六块中性材料，按明度分档后的中性残余")
    print("=" * 78)
    linear = np.load("scripts/experiments/density-fallback/pano_stride8.npy")
    base = absorption(np.median(linear[:24, :, :].reshape(-1, 3), axis=0))
    D = absorption(linear)
    points = np.vstack([(D[y0 // 8:y1 // 8, x0 // 8:x1 // 8] - base).reshape(-1, 3)[::5]
                        for y0, y1, x0, x1 in PANO_BOXES])
    inner = (D[400 // 8:7000 // 8, 400 // 8:6800 // 8] - base).reshape(-1, 3)
    low, high = co_sited(inner[::3])
    report("C  现行：逐通道同像素窗口（有片基）",
           lambda p: (p - low) / (high - low), points)
    rule_a, gains_a = fit_gains(points, 0.0)
    report(f"P1 预设增益 + 片基黑点 (u={np.round(gains_a, 3)})", rule_a, points)
    rule_b, gains_b = fit_gains(points, low[1])
    report(f"P2 预设增益 + 共享黑点=画面低端 (u={np.round(gains_b, 3)})", rule_b, points)
    report("FREE 逐通道仿射（上限）", free_affine(points), points)


def raf():
    print()
    print("=" * 78)
    print("欠曝 RAF：Capture One 认为是中性的像素，按明度分档后的中性残余")
    print("=" * 78)
    print(f"  {'frame':<7}{'C 现行':>16}{'P1 片基黑点':>16}{'P2 共享黑点':>16}"
          f"{'FREE 上限':>16}")
    collected = []
    for tag, (x0, y0, side) in RAFS.items():
        base, block, level, chroma_neutral = raf_frame(tag, x0, y0, side)
        points = block[chroma_neutral & (level > 0.06)]
        step = max(1, len(points) // 60000)
        points = points[::step]
        low, high, offsets = share_smart_auto(*co_sited(block.reshape(-1, 3)[::3]))
        span = high - low
        low_g = float(np.median(block.reshape(-1, 3)[::3][
            np.argsort(block.reshape(-1, 3)[::3] @ W)[:max(1, len(block.reshape(-1, 3)[::3]) // 50)]][:, 1]))
        rule_a, gains_a = fit_gains(points, 0.0)
        rule_b, gains_b = fit_gains(points, low_g)
        collected.append((gains_a, gains_b, low_g))
        line = f"  {tag:<7}"
        for transform in (lambda p: (p - (low + offsets)) / span,
                          rule_a, rule_b,
                          free_affine(points)):
            median, _, _ = binned(points, transform)
            line += f"{median * 100:15.1f}%"
        print(line + f"   (n={len(points)})")
    gains_a = np.mean([item[0] for item in collected], axis=0)
    gains_b = np.mean([item[1] for item in collected], axis=0)
    print(f"  11/4 帧平均预设增益：片基黑点 u_R {gains_a[0]:.3f} u_B {gains_a[1]:.3f}"
          f"  |  共享黑点 u_R {gains_b[0]:.3f} u_B {gains_b[1]:.3f}")


def flags():
    print()
    print("=" * 78)
    print("触发判定表（正常素材必须一条都不命中）")
    print("=" * 78)
    print(f"  {'素材':<30}{'跨度比':>8}{'片基最低电平':>13}   判定")
    rows = []
    for path in (sorted(glob.glob("test_picture/lr合并/*.tif"))
                 + sorted(glob.glob("test_picture/尼康扫描仪tiff/*.tif"))
                 + sorted(glob.glob("test_picture/诺日士jpg/*.jpg"))
                 + ["test_picture/000021940005.jpg"]):
        linear, _, _ = to_working(path)
        span, base, _, ratio = span_ratio(linear)
        level = float(np.min(10 ** (-absorption(base)) * 100))
        reasons = []
        if ratio > 2.0:
            reasons.append("跨度失衡")
        if level < 1.0:
            reasons.append("低信号通道")
        rows.append((path, ratio, level, reasons,
                     "正常" if ("尼康扫描仪" in path or "诺日士" in path
                                or path.endswith("000021940005.jpg")) else "问题"))
    for tag in ("1942", "1946"):
        with rawpy.imread(f"{RAFDIR}/Capture One Catalog{tag}.raf") as raw:
            rgb = raw.postprocess(half_size=True, output_bps=16, gamma=(1, 1),
                                  no_auto_bright=True, use_camera_wb=False,
                                  output_color=rawpy.ColorSpace.ProPhoto)
        linear = rgb.astype(np.float32) / 65535.0
        density = app_style_base(linear)
        level = float(np.min(10 ** (-density) * 100))
        reasons = []
        if float(np.max(density)) > 1.60:
            reasons.append("片基被闸门拒")
        if level < 1.0:
            reasons.append("低信号通道")
        rows.append((f"{RAFDIR}/Capture One Catalog{tag}.raf", float("nan"), level,
                     reasons, "问题"))
    for name in ("_DSC7333", "_DSC7344"):
        with rawpy.imread(f"test_picture/尼康nef_raw/{name}.NEF") as raw:
            rgb = raw.postprocess(half_size=True, output_bps=16, gamma=(1, 1),
                                  no_auto_bright=True, use_camera_wb=False,
                                  output_color=rawpy.ColorSpace.ProPhoto)
        linear = rgb.astype(np.float32) / 65535.0
        density = app_style_base(linear)
        low, high = co_sited(absorption(linear)[200:-200, 200:-200].reshape(-1, 3)[::3])
        ratio = float((high - low).max() / (high - low).min())
        level = float(np.min(10 ** (-density) * 100))
        reasons = []
        if ratio > 2.0:
            reasons.append("跨度失衡")
        if level < 1.0:
            reasons.append("低信号通道")
        rows.append((f"尼康nef_raw/{name}.NEF", ratio, level, reasons, "正常"))
    for path, ratio, level, reasons, group in rows:
        name = path.replace("\\", "/").split("/")[-1]
        verdict = "、".join(reasons) if reasons else "不触发"
        ratio_text = "  n/a" if np.isnan(ratio) else f"{ratio:8.2f}"
        print(f"  {group} {name:<26}{ratio_text}{level:>12.2f}%   {verdict}")


PRESET_PANO = np.array([1.86, 1.0, 0.666])
PRESET_RAF = np.array([1.11, 1.0, 0.55])

if __name__ == "__main__":
    panorama()
    raf()
    flags()
