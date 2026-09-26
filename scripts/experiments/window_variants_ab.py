#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""窗口端点取法 A/B 实验（独立脚本，不改动应用代码）。

背景
----
软件现在的"去色罩"= 逐通道减片基 + 逐通道线性窗口拉伸；窗口两端由
"按密度亮度排序、取最暗/最亮 2% 的同像素通道值"决定（见
src/commands.rs 的 co_sited_density_extremes / compute_content_limits_f32）。

本脚本把这段"取端点"的逻辑单独复现出来，只换"怎么取端点"，在同一批素材上
渲染，回答两个问题：

  1) 对问题素材（LR 拼接 TIFF），中性材料的残余偏色有没有变小；
  2) 对正常素材，相对现行规则被改动了多少（会不会把正常照片弄坏）。

变体
----
  V0 现行     : 按亮度排序，最暗/最亮 2% 同像素，三通道各取自己的值（逐通道端点）
  V1 共享黑端 : 黑端改成一个共享值（三通道端点的亮度值），逐通道跨度不变
  V2 全共享   : 黑端、白端都共享（三通道共用一个窗口）——文档 §10 记录过的坏版本，作对照
  V3 点取     : 同像素规则但只取最暗/最亮 0.1%（接近"一个最暗点+一个最亮点"）
  V4 逐通道分位: 三通道各自独立取 2%/98% 分位（不按亮度排序）
  V5 锚定片基 : 黑端直接锚到实测片基（密度 0），逐通道跨度不变

用法
----
    python scripts/experiments/window_variants_ab.py                # 全部样本
    python scripts/experiments/window_variants_ab.py 5.3 --stride 4  # 只跑名字含 5.3 的

输出
----
    target/window-variants/<样本>-<变体>.jpg
    target/window-variants/<样本>-compare.jpg
    target/window-variants/summary.md
"""

from __future__ import annotations

import argparse
import os
import struct
import sys

import numpy as np

try:  # Windows 控制台默认不是 UTF-8，中文报告需要显式设置
    sys.stdout.reconfigure(encoding="utf-8")
except Exception:  # pragma: no cover
    pass

try:
    import tifffile
except ImportError:  # pragma: no cover
    tifffile = None
from PIL import Image, ImageDraw

REPO = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", ".."))
OUT_DIR = os.path.join(REPO, "target", "window-variants")

# ---------------------------------------------------------------- 色彩空间矩阵

XYZ = {
    # linear RGB -> XYZ（D65）
    "srgb": np.array(
        [
            [0.4124564, 0.3575761, 0.1804375],
            [0.2126729, 0.7151522, 0.0721750],
            [0.0193339, 0.1191920, 0.9503041],
        ]
    ),
    "adobe": np.array(
        [
            [0.5767309, 0.1855540, 0.1881852],
            [0.2973769, 0.6273491, 0.0752741],
            [0.0270343, 0.0706872, 0.9911085],
        ]
    ),
    # linear ProPhoto -> XYZ（D50）
    "prophoto": np.array(
        [
            [0.7976749, 0.1351917, 0.0313534],
            [0.2880402, 0.7118741, 0.0000857],
            [0.0000000, 0.0000000, 0.8252100],
        ]
    ),
}
D65_TO_D50 = np.array(
    [
        [1.0478112, 0.0228866, -0.0501270],
        [0.0295424, 0.9904844, -0.0170491],
        [-0.0092345, 0.0150436, 0.7521316],
    ]
)
D50_TO_D65 = np.array(
    [
        [0.9555766, -0.0230393, 0.0631636],
        [-0.0282895, 1.0099416, 0.0210077],
        [0.0122982, -0.0204830, 1.3299098],
    ]
)


def file_to_working_matrix(space: str) -> np.ndarray:
    """文件基色 -> 工作域（ProPhoto D50 线性）的 3x3 矩阵。"""
    adapt = D65_TO_D50 if space in ("srgb", "adobe") else np.eye(3)
    return np.linalg.inv(XYZ["prophoto"]) @ adapt @ XYZ[space]


WORKING_TO_SRGB = np.linalg.inv(XYZ["srgb"]) @ D50_TO_D65 @ XYZ["prophoto"]

# ---------------------------------------------------------------- 输入域解析


def icc_info(path: str):
    """返回 (space, gamma)。space ∈ {'srgb','adobe','prophoto'}，读不到时返回 None。"""
    if tifffile is None or not path.lower().endswith((".tif", ".tiff", ".dng")):
        return None
    try:
        with tifffile.TiffFile(path) as t:
            tag = t.pages[0].tags.get(34675)
            icc = None if tag is None else bytes(tag.value)
    except Exception:
        return None
    if not icc:
        return None
    count = struct.unpack(">I", icc[128:132])[0]
    off, tags = 132, {}
    for _ in range(count):
        sig = icc[off : off + 4]
        o, s = struct.unpack(">II", icc[off + 4 : off + 12])
        tags[sig.decode("latin1")] = (o, s)
        off += 12
    desc = ""
    if "desc" in tags:
        o, s = tags["desc"]
        desc = icc[o + 12 : o + s].split(b"\x00")[0].decode("latin1").lower()
    space = None
    if "prophoto" in desc:
        space = "prophoto"
    elif "adobe" in desc:
        space = "adobe"
    elif "srgb" in desc:
        space = "srgb"
    gamma = None
    if "rTRC" in tags:
        o, s = tags["rTRC"]
        trc = icc[o : o + s]
        if trc[:4] == b"curv" and struct.unpack(">I", trc[8:12])[0] == 1:
            gamma = struct.unpack(">H", trc[12:14])[0] / 256.0
    return space, gamma


def srgb_to_linear(v: np.ndarray) -> np.ndarray:
    return np.where(v <= 0.04045, v / 12.92, ((v + 0.055) / 1.055) ** 2.4)


def read_encoded(path: str, stride: int) -> np.ndarray:
    """读成 float32 0..1 的编码值（不做反变换）。"""
    if path.lower().endswith((".jpg", ".jpeg", ".png")):
        a = np.asarray(Image.open(path).convert("RGB"), dtype=np.float32)[::stride, ::stride] / 255.0
        return a
    arr = tifffile.memmap(path)
    h, w = arr.shape[0], arr.shape[1]
    return arr[::stride, ::stride].astype(np.float32) / 65535.0


def load_working(path: str, stride: int):
    """返回 (工作域线性数组, space, gamma)。"""
    info = icc_info(path)
    space, gamma = info if info else (None, None)
    if path.lower().endswith((".jpg", ".jpeg", ".png")):
        space = "srgb"
    if space is None:
        space = "srgb"
    enc = read_encoded(path, stride)
    if space == "srgb" and gamma is None:
        lin = srgb_to_linear(enc)
    else:
        lin = np.power(np.maximum(enc, 1e-8), gamma if gamma else 2.2)
    lin = np.tensordot(lin, file_to_working_matrix(space).T, axes=1).astype(np.float32)
    return np.maximum(lin, 0.0), space, (gamma if gamma else 2.2)


# ---------------------------------------------------------------- 片基估计


def estimate_base(lin: np.ndarray, enc: np.ndarray):
    """片基（密度最低处）估计：片边窄带优先，其次画面内最亮分位。

    与 src/commands.rs 的 estimate_film_base_f32 同思路的简化版；返回
    (base_rgb, source)。所有变体共用同一个片基，保证对照公平。
    """
    h, w = lin.shape[0], lin.shape[1]
    luma = lin @ np.array([0.2126, 0.7152, 0.0722])
    ok = (~np.any(enc >= 0.995, axis=2)) & np.isfinite(luma) & (luma > 0)
    band = np.zeros((h, w), bool)
    by, bx = max(2, int(h * 0.03)), max(2, int(w * 0.03))
    band[:by, :] = band[-by:, :] = band[:, :bx] = band[:, -bx:] = True
    band &= ok
    if band.sum() > 1000 and np.median(luma[band]) > np.percentile(luma[ok], 75):
        return np.median(lin[band], axis=0), "film_edge_band"
    vals = luma[ok]
    if vals.size < 100:
        return np.median(lin.reshape(-1, 3), axis=0), "fallback"
    thr = np.percentile(vals, 99.5)
    sel = ok & (luma >= thr)
    return np.median(lin[sel], axis=0), "content_high_quantile"


# ---------------------------------------------------------------- 端点取法


def content_samples(D: np.ndarray, enc: np.ndarray, max_edge: int = 1024):
    """按固定网格取内容样本（对应应用的 512 分析网格，取密一点更稳）。"""
    h, w = D.shape[0], D.shape[1]
    sy = max(1, h // max_edge)
    sx = max(1, w // max_edge)
    d = D[::sy, ::sx].reshape(-1, 3)
    e = enc[::sy, ::sx].reshape(-1, 3)
    keep = (~np.any(e >= 0.995, axis=1)) & np.all(np.isfinite(d), axis=1)
    return d[keep]


LUMA = np.array([0.2126, 0.7152, 0.0722])


def endpoints(samples: np.ndarray, variant: str):
    """返回 (lo[3], hi[3])，单位是相对密度。"""
    if variant == "V4_per_channel":
        lo = np.percentile(samples, 2, axis=0)
        hi = np.percentile(samples, 98, axis=0)
        return lo, hi

    order = np.argsort(samples @ LUMA)
    n = samples.shape[0]
    frac = 0.001 if variant == "V3_point" else 0.02
    tail = max(1, int(np.ceil(n * frac)))
    lo = samples[order[:tail]].mean(axis=0)
    hi = samples[order[-tail:]].mean(axis=0)

    if variant == "V0_current":
        return lo, hi
    if variant == "V1_shared_low":
        common = float(lo @ LUMA)
        return np.full(3, common), common + (hi - lo)
    if variant == "V2_shared_all":
        return np.full(3, float(lo @ LUMA)), np.full(3, float(hi @ LUMA))
    if variant == "V3_point":
        return lo, hi
    if variant == "V5_base_anchor":
        return np.zeros(3), hi - lo  # 黑端锚定片基（密度 0），跨度沿用现行
    raise ValueError(variant)


VARIANTS = [
    ("V0_current", "现行：逐通道同像素 2% 端点"),
    ("V1_shared_low", "共享黑端（端点亮度值），逐通道跨度不变"),
    ("V2_shared_all", "黑白端都共享（对照：文档记录过的坏版本）"),
    ("V3_point", "同像素规则但只取 0.1%（近似一个最暗点+一个最亮点）"),
    ("V4_per_channel", "三通道各自独立 2%/98% 分位"),
    ("V5_base_anchor", "黑端锚定实测片基（密度 0），逐通道跨度不变"),
]

# 拼图上的标签只用 ASCII（PIL 默认字体画不出中文会变成方块）
VARIANTS_ASCII = {
    "V0_current": "current: co-sited 2% per channel",
    "V1_shared_low": "share black end, keep spans",
    "V2_shared_all": "share both ends (bad control)",
    "V3_point": "co-sited but 0.1% tails",
    "V4_per_channel": "per-channel 2%/98% quantiles",
    "V5_base_anchor": "black end pinned to film base",
}

# 跨度比（最大通道跨度/最小通道跨度）超过该值即认为"内容端点不可信"
SPAN_GATE = 2.0

# ---------------------------------------------------------------- 渲染与指标


def render(D: np.ndarray, lo, hi, gamma_out=2.2):
    span = np.maximum(hi - lo, 1e-6)
    L = np.clip((D - lo) / span, 0.0, 1.0)
    srgb_lin = np.tensordot(L, WORKING_TO_SRGB.T, axes=1)
    srgb = np.clip(srgb_lin, 0.0, 1.0) ** (1.0 / gamma_out)
    return (srgb * 255.0 + 0.5).astype(np.uint8)


def inner_view(a: np.ndarray, margin: float = 0.04):
    h, w = a.shape[0], a.shape[1]
    dy, dx = int(h * margin), int(w * margin)
    return a[dy : h - dy, dx : w - dx]


def channel_means(img: np.ndarray):
    v = inner_view(img).reshape(-1, 3).astype(np.float64).mean(axis=0)
    return v, v[0] / v[1], v[2] / v[1]


# 问题素材的人工中性面取样框（全分辨率坐标 y0,y1,x0,x1）
NEUTRAL_PATCHES = {
    "_dsc7587-2-pano.tif": [
        ("砂石左", 6420, 7040, 500, 2200),
        ("砂石中", 6420, 7040, 2600, 4200),
        ("砂石右", 6420, 7040, 4500, 6300),
        ("大理石檐部", 2520, 2810, 300, 2100),
        ("柱身左1", 3200, 5100, 190, 370),
        ("柱身左3", 3200, 5100, 1450, 2000),
    ]
}


def patch_table(img: np.ndarray, stride: int, base_name: str):
    patches = NEUTRAL_PATCHES.get(base_name)
    if not patches:
        return None
    rows = []
    for name, y0, y1, x0, x1 in patches:
        blk = img[y0 // stride : y1 // stride, x0 // stride : x1 // stride].reshape(-1, 3)
        m = np.median(blk, axis=0)
        rows.append((name, m, float(m[0] / m[1]), float(m[2] / m[1])))
    return rows


def neutral_set(img: np.ndarray, stride: int):
    """取 V0 成品里"已经接近中性"的像素坐标（亮/中调、低彩度），作为回归基准面。

    返回 (y, x) 索引数组。判据只看现行结果，因此与变体无关，可作对照。
    """
    h, w = img.shape[0], img.shape[1]
    dy, dx = int(h * 0.04), int(w * 0.04)
    sub = img[dy : h - dy, dx : w - dx].astype(np.float32)
    luma = sub @ np.array([0.2126, 0.7152, 0.0722], dtype=np.float32)
    g = np.maximum(sub[:, :, 1], 1.0)
    chroma = np.maximum(np.abs(sub[:, :, 0] / g - 1.0), np.abs(sub[:, :, 2] / g - 1.0))
    mid = (luma > 60) & (luma < 235)
    if mid.sum() < 1000:
        return None
    thr = np.percentile(chroma[mid], 25)
    mask = mid & (chroma <= thr)
    ys, xs = np.nonzero(mask)
    return ys * 1, xs * 1, dy, dx


def neutral_chroma(img: np.ndarray, ref, stride: int):
    """在参考像素集上测偏色：返回 (|R/G-1| 中位数, |B/G-1| 中位数, n)。"""
    if ref is None:
        return None
    ys, xs, dy, dx = ref
    v = img[dy : img.shape[0] - dy, dx : img.shape[1] - dx].astype(np.float32)[ys, xs]
    g = np.maximum(v[:, 1], 1.0)
    rg = np.abs(v[:, 0] / g - 1.0)
    bg = np.abs(v[:, 2] / g - 1.0)
    return float(np.median(rg)), float(np.median(bg)), int(v.shape[0])


# ---------------------------------------------------------------- 样本清单

SAMPLES = [
    # 问题素材：LR 拼接，宽色域 TIFF
    ("lr-pano", r"test_picture\lr合并\_DSC7587-2-Pano.tif"),
    ("lr-pano", r"test_picture\lr合并\_DSC7583-Pano.tif"),
    ("lr-pano", r"test_picture\lr合并\_DSC7569-Pano.tif"),
    # 正常素材：尼康扫描仪 TIFF（项目原有 fixture）
    ("normal", r"test_picture\尼康扫描仪tiff\5.3-1.tif"),
    ("normal", r"test_picture\尼康扫描仪tiff\5.3-2.tif"),
    ("normal", r"test_picture\尼康扫描仪tiff\5.3-3.tif"),
    ("normal", r"test_picture\尼康扫描仪tiff\5.3-4.tif"),
    # 正常素材：诺日士 / 杂项 JPEG
    ("normal", r"test_picture\诺日士jpg\000000070001.jpg"),
    ("normal", r"test_picture\诺日士jpg\000000070005.jpg"),
    ("normal", r"test_picture\000021940005.jpg"),
]


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("filter", nargs="?", default="", help="只跑路径里含该子串的样本")
    ap.add_argument("--stride", type=int, default=4, help="渲染降采样步长（默认 4）")
    args = ap.parse_args()
    os.makedirs(OUT_DIR, exist_ok=True)

    report = []
    for kind, rel in SAMPLES:
        path = os.path.join(REPO, rel)
        if not os.path.exists(path) or args.filter not in rel:
            continue
        base_name = os.path.basename(path).lower()
        print(f"\n=== {rel}  [{kind}]", flush=True)
        lin, space, gamma = load_working(path, args.stride)
        enc = read_encoded(path, args.stride)
        base, base_src = estimate_base(lin, enc)
        D = np.log10(np.maximum(base[None, None, :], 1e-9) / np.maximum(lin, 1e-9)).astype(np.float32)
        samples = content_samples(D, enc)
        print(
            f"    基色空间={space} gamma={gamma:.2f}  片基来源={base_src} "
            f"片基={np.round(base, 4)}  样本={samples.shape[0]}"
        )

        renders, rows = {}, []
        for name, desc in VARIANTS:
            lo, hi = endpoints(samples, name)
            renders[name] = render(D, lo, hi)

        # 中性基准面取自现行结果：所有变体在完全相同的像素集上比较偏色
        ref = neutral_set(renders["V0_current"], args.stride)
        for name, desc in VARIANTS:
            lo, hi = endpoints(samples, name)
            img = renders[name]
            mean, rg, bg = channel_means(img)
            neutral = neutral_chroma(img, ref, args.stride)
            if name == "V0_current":
                delta_txt = "基准"
            else:
                diff = np.abs(img.astype(np.int16) - renders["V0_current"].astype(np.int16))
                changed = float((diff.max(axis=2) > 3).mean() * 100)
                delta_txt = f"改动 {diff.mean():5.2f}/255, 明显改动像素 {changed:5.1f}%"
            rows.append((name, lo, hi, mean, rg, bg, delta_txt, neutral))
            Image.fromarray(img).save(os.path.join(OUT_DIR, f"{base_name[:-4]}-{name}.jpg"), quality=92)
            neutral_txt = (
                "n/a" if neutral is None else f"中性面 |R/G-1|={neutral[0]:.3f} |B/G-1|={neutral[1]:.3f}"
            )
            print(
                f"    {name:16s} lo={np.round(lo,3)} hi={np.round(hi,3)} "
                f"均值RGB={np.round(mean,1)} R/G={rg:.3f} B/G={bg:.3f}  {neutral_txt}  {delta_txt}"
            )

        # ---- 变体拼图
        thumbs = []
        for name, desc in VARIANTS:
            im = Image.fromarray(renders[name])
            im.thumbnail((430, 430))
            thumbs.append((name, VARIANTS_ASCII[name], im))
        cols, rows_n = 3, 2
        cw, ch = 440, 470
        sheet = Image.new("RGB", (cols * cw, rows_n * ch), (18, 18, 18))
        draw = ImageDraw.Draw(sheet)
        for i, (name, desc, im) in enumerate(thumbs):
            gx, gy = (i % cols) * cw, (i // cols) * ch
            sheet.paste(im, (gx + 4, gy + 26))
            draw.text((gx + 6, gy + 8), f"{name}  {desc[:44]}", fill=(255, 255, 120))
        sheet.save(os.path.join(OUT_DIR, f"{base_name[:-4]}-compare.jpg"), quality=92)

        # ---- V0 vs V1 大图（只有标了中性取样框的样本）
        if base_name in NEUTRAL_PATCHES:
            big = []
            for name in ("V0_current", "V1_shared_low"):
                im = Image.fromarray(renders[name])
                im = im.resize((min(950, im.width), int(im.height * min(950, im.width) / im.width)))
                big.append((name, im))
            w = sum(im.width for _, im in big) + 12
            h = max(im.height for _, im in big) + 26
            sheet2 = Image.new("RGB", (w, h), (18, 18, 18))
            d2 = ImageDraw.Draw(sheet2)
            x = 0
            for name, im in big:
                sheet2.paste(im, (x + 6, 24))
                d2.text((x + 8, 6), f"{name} - {VARIANTS_ASCII[name]}", fill=(255, 255, 120))
                x += im.width + 6
            sheet2.save(os.path.join(OUT_DIR, f"{base_name[:-4]}-V0vsV1.jpg"), quality=92)

        # ---- 中性面读数（只有标了取样框的样本）
        patch_rows = None
        if patch_table(renders["V0_current"], args.stride, base_name):
            patch_rows = []
            for name, desc in VARIANTS:
                t = patch_table(renders[name], args.stride, base_name)
                rg = np.array([r[2] for r in t])
                bg = np.array([r[3] for r in t])
                spread_rg = float(rg.max() / max(rg.min(), 1e-3))
                spread_bg = float(bg.max() / max(bg.min(), 1e-3))
                patch_rows.append((name, t, spread_rg, spread_bg))
            print("    中性材料读数（成品里的 R/G；1.0=中性）:")
            for name, t, spread_rg, spread_bg in patch_rows:
                print(
                    f"      {name:16s} " + " ".join(f"{r[0]}={r[2]:.3f}/{r[3]:.3f}" for r in t)
                    + f"   R/G 离散 {spread_rg:.3f}x  B/G 离散 {spread_bg:.3f}x"
                )

        report.append((rel, kind, space, gamma, base_src, base, rows, patch_rows))

    # ---- summary.md
    lines = ["# 窗口端点取法 A/B 结果", "", "生成方式：`python scripts/experiments/window_variants_ab.py`", ""]
    lines += [
        "指标说明：`均值 R/G`、`R/G`、`B/G` 取自成品（sRGB，0-255，去掉画面外 4% 边）；"
        "`改动` = 与 V0（现行规则）逐像素平均绝对差；`明显改动像素` = 任一分量差 > 3/255 的像素占比。",
        "`中性面` 指标取自现行结果里彩度最低的亮/中调像素（同一批像素在所有变体上比较），"
        "数值越小说明该变体让画面里本来就中性的地方更中性。",
        "",
        "## 变体",
        "",
    ]
    for name, desc in VARIANTS:
        lines.append(f"- `{name}`：{desc}")
    lines.append("")
    lines.append("## 一眼结论（跨度失衡比 = 三通道跨度最大值/最小值）")
    lines.append("")
    lines.append(
        f"用跨度失衡比作闸门（阈值 {SPAN_GATE:.1f}）：失衡比超过阈值才启用「共享黑端」，其余样本逐像素保持现行行为。"
    )
    lines.append("")
    lines.append(
        "| 样本 | 类别 | 跨度失衡比 | V0 中性面 R/G 偏差 | V1 中性面 R/G 偏差 | "
        "V0 中性面 B/G 偏差 | V1 中性面 B/G 偏差 | V1 明显改动像素 | 闸门 |"
    )
    lines.append("| --- | --- | --- | --- | --- | --- | --- | --- | --- |")
    for rel, kind, space, gamma, base_src, base, rows, patch_rows in report:
        by_name = {r[0]: r for r in rows}
        r0, r1 = by_name["V0_current"], by_name["V1_shared_low"]
        span0 = r0[2] - r0[1]
        imbalance = float(span0.max() / span0.min())
        n0, n1 = r0[7], r1[7]
        gate = "启用" if imbalance > SPAN_GATE else "保持现行"
        chg = r1[6].split("明显改动像素")[-1].strip()
        lines.append(
            f"| {os.path.basename(rel)} | {kind} | {imbalance:.2f}x | "
            f"{n0[0]:.3f} | {n1[0]:.3f} | {n0[1]:.3f} | {n1[1]:.3f} | {chg} | {gate} |"
        )
    lines.append("")
    for rel, kind, space, gamma, base_src, base, rows, patch_rows in report:
        lines += [
            f"## {rel}",
            "",
            f"输入域 `{space}` gamma {gamma:.2f}；片基来源 `{base_src}`，片基线性值 "
            f"`{np.round(base, 4).tolist()}`。",
            "",
            "| 变体 | lo(R,G,B) | hi(R,G,B) | 跨度比 | 均值 RGB | R/G | B/G | 相对现行改动 |",
            "| --- | --- | --- | --- | --- | --- | --- | --- |",
        ]
        for name, lo, hi, mean, rg, bg, delta, neutral in rows:
            span = hi - lo
            ratio = span / span.min()
            n_txt = (
                "n/a" if neutral is None else f"R/G 偏差 {neutral[0]:.3f}, B/G 偏差 {neutral[1]:.3f}"
            )
            lines.append(
                f"| {name} | {np.round(lo,3).tolist()} | {np.round(hi,3).tolist()} | "
                f"{np.round(ratio,2).tolist()} | {np.round(mean,1).tolist()} | {rg:.3f} | {bg:.3f} | "
                f"{delta}；中性面 {n_txt} |"
            )
        lines.append("")
        if patch_rows:
            lines.append("中性材料读数（成品里的 R/G / B/G）；离散 = 各中性框之间的最大/最小比：")
            lines.append("")
            header = "| 变体 | " + " | ".join(r[0] for r in patch_rows[0][1]) + " | R/G 离散 | B/G 离散 |"
            lines.append(header)
            lines.append("| --- | " + " | ".join("---" for _ in patch_rows[0][1]) + " | --- | --- |")
            for name, t, sr, sb in patch_rows:
                cells = " | ".join(f"{r[2]:.3f} / {r[3]:.3f}" for r in t)
                lines.append(f"| {name} | {cells} | {sr:.3f}x | {sb:.3f}x |")
            lines.append("")
    with open(os.path.join(OUT_DIR, "summary.md"), "w", encoding="utf-8") as fh:
        fh.write("\n".join(lines) + "\n")
    print(f"\n结果写入 {OUT_DIR}")


if __name__ == "__main__":
    sys.exit(main())
