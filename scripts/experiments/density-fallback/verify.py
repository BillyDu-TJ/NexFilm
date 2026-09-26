"""Independent re-computation of the numbers behind the two mask-removal
investigations. Read-only: it never touches application code.

Checks
  A  LR panorama: content window endpoints/spans (co-sited luma 2% tails)
  B  LR panorama: how far the "should be neutral" density ratios drift
  C  Fuji RAF batch: film-base level, density and the 1.60 gate
  D  The fallback branch: shared span + the +-0.20 channel-offset clamp
"""

import math
import struct
import sys

sys.path.insert(0, "target/pylibs")

import numpy as np  # noqa: E402
import tifffile  # noqa: E402

sys.stdout.reconfigure(encoding="utf-8")

W = np.array([0.2126, 0.7152, 0.0722], dtype=np.float64)


def density(linear):
    return -np.log10(np.maximum(linear, 1e-6))


def co_sited(samples, fraction=0.02):
    """The current rule: rank pixels by density luma, average each 2% tail per
    channel from the *same* pixels."""
    order = np.argsort(samples @ W)
    k = max(1, int(math.ceil(len(samples) * fraction)))
    return samples[order[:k]].mean(axis=0), samples[order[-k:]].mean(axis=0)


def share_smart_auto(d_min, d_max):
    """src/commands.rs::share_smart_auto_density_scale, transcribed."""
    limit = 0.20
    low = float(np.dot(d_min, W))
    high = float(np.dot(d_max, W))
    centers = (np.asarray(d_min) + np.asarray(d_max)) * 0.5
    center = float(np.dot(centers, W))
    offsets = np.clip(centers - center, -limit, limit)
    residual = float(np.dot(offsets, W))
    offsets = np.clip(offsets - residual, -limit, limit)
    return low, high, offsets


def icc_gamma(path):
    """Pull the rTRC gamma out of the TIFF ICC profile (u8Fixed8 curve)."""
    with tifffile.TiffFile(path) as handle:
        page = handle.pages[0]
        tag = page.tags.get("ICCProfile") or page.tags.get("InterColorProfile")
        if tag is None:
            return (False, None)
        icc = bytes(tag.value)
    description = icc[icc.find(b"desc"):icc.find(b"desc") + 200]
    text = "".join(chr(byte) for byte in description if 32 <= byte < 127)
    has_prophoto = b"ProPhoto" in icc or "ProPhoto" in text
    at = icc.find(b"rTRC")
    gamma = None
    if at >= 0:
        offset, size = struct.unpack(">II", icc[at + 4:at + 12])
        block = icc[offset:offset + size]
        if block[:4] == b"curv" and struct.unpack(">I", block[8:12])[0] == 1:
            gamma = struct.unpack(">H", block[12:14])[0] / 256.0
    return has_prophoto, gamma


def report(title):
    print()
    print("=" * 72)
    print(title)
    print("=" * 72)


# ----------------------------------------------------------------- A + B
PANO = "test_picture/lr合并/_DSC7587-2-Pano.tif"
PATCHES = [
    ("砂石左", 6420, 7040, 500, 2200),
    ("砂石中", 6420, 7040, 2600, 4200),
    ("砂石右", 6420, 7040, 4500, 6300),
    ("大理石檐部", 2520, 2810, 300, 2100),
    ("柱身左1", 3200, 5100, 190, 370),
    ("柱身左3", 3200, 5100, 1450, 2000),
]


def panorama():
    report("A/B. LR 拼接全景（_DSC7587-2-Pano.tif）")
    print("ICC:", icc_gamma(PANO))
    raw = tifffile.imread(PANO)
    print("size/dtype:", raw.shape, raw.dtype)
    gamma = icc_gamma(PANO)[1] or 1.8

    linear = (raw.astype(np.float32) / 65535.0) ** gamma
    del raw
    full = density(linear)

    # Film base from the rebate bands just inside the scan edge.
    top = full[60:200, :, :].reshape(-1, 3)
    bottom = full[-200:-60, :, :].reshape(-1, 3)
    base = np.median(np.concatenate([top, bottom]), axis=0)
    print("片边片基密度 (上+下带中位):", np.round(base, 4))

    # Content window: interior of the frame, co-sited 2% tails.
    inner = full[400:7000:4, 400:6800:4, :].reshape(-1, 3)
    rel = inner - base
    low, high = co_sited(rel)
    span = high - low
    print("内容窗口 dmin:", np.round(low, 3), " dmax:", np.round(high, 3))
    print("逐通道跨度:", np.round(span, 3),
          " 比 R:G:B = 1 :", round(span[1] / span[0], 2), ":",
          round(span[2] / span[0], 2))
    print("窗口提供的固定比值  R/G =", round(span[0] / span[1], 3),
          " B/G =", round(span[2] / span[1], 3))

    # "Should be neutral" materials: their own density ratios, plus a
    # robustness margin from the boxes' four quadrants.
    print()
    print(f"{'patchname':<12}{'dG':>7}{'dR/dG':>8}{'dB/dG':>8}   "
          f"{'quadrant dR/dG':>22}{'quadrant dB/dG':>22}")
    rows = []
    for name, y0, y1, x0, x1 in PATCHES:
        block = full[y0:y1:4, x0:x1:4, :].reshape(-1, 3) - base
        median = np.median(block, axis=0)
        rg, bg = median[0] / median[1], median[2] / median[1]
        rows.append((name, median[1], rg, bg))
        half_y = (y1 - y0) // 2
        half_x = (x1 - x0) // 2
        quads = []
        for dy in (0, 1):
            for dx in (0, 1):
                ys = y0 + dy * half_y
                xs = x0 + dx * half_x
                sub = full[ys:ys + half_y:4, xs:xs + half_x:4, :]
                sub = sub.reshape(-1, 3) - base
                mid = np.median(sub, axis=0)
                quads.append((mid[0] / mid[1], mid[2] / mid[1]))
        quads = np.array(quads)
        print(f"{name:<12}{median[1]:>7.3f}{rg:>8.3f}{bg:>8.3f}   "
              f"{quads[:,0].min():>10.3f}..{quads[:,0].max():<10.3f}"
              f"{quads[:,1].min():>10.3f}..{quads[:,1].max():<10.3f}")

    dark = min(rows, key=lambda row: row[1])
    light = max(rows, key=lambda row: row[1])
    print()
    print(f"最暗的材料 {dark[0]}: dR/dG {dark[2]:.3f}, dB/dG {dark[3]:.3f} "
          f"(dG {dark[1]:.3f})")
    print(f"最亮的材料 {light[0]}: dR/dG {light[2]:.3f}, dB/dG {light[3]:.3f} "
          f"(dG {light[1]:.3f})")
    print("要求的比值漂移: dR/dG "
          f"{100 * (light[2] / dark[2] - 1):+.0f}%, dB/dG "
          f"{100 * (light[3] / dark[3] - 1):+.0f}%")
    del full, linear, inner, rel


# ----------------------------------------------------------------- C + D
RAFS = [
    "test_picture/富士raf/Capture One Catalog1942.raf",
    "test_picture/富士raf/Capture One Catalog1946.raf",
    "test_picture/富士raf/Capture One Catalog1950.raf",
    "test_picture/富士raf/Capture One Catalog1952.raf",
]
OTHERS = [
    ("DSCF0895.RAF", "test_picture/DSCF0895.RAF"),
    ("_DSC7333.NEF", "test_picture/尼康nef_raw/_DSC7333.NEF"),
]


def raw_base(path):
    import rawpy

    with rawpy.imread(path) as raw:
        rgb = raw.postprocess(
            half_size=True,
            output_bps=16,
            gamma=(1, 1),
            no_auto_bright=True,
            use_camera_wb=False,
            output_color=rawpy.ColorSpace.ProPhoto,
        )
        camera_wb = raw.camera_whitebalance
    height, width = rgb.shape[:2]
    margin_y, margin_x = int(height * 0.05), int(width * 0.05)
    inner = rgb[margin_y:height - margin_y, margin_x:width - margin_x, :]
    flat = inner.reshape(-1, 3).astype(np.float64) / 65535.0
    top = np.percentile(flat, 99.9, axis=0)
    dens = -np.log10(np.maximum(top, 1e-6))
    relative = dens - dens[1]
    return {
        "levels_pct": top * 100.0,
        "density": dens,
        "relative": relative,
        "cam_wb": camera_wb,
    }


def raw_batch():
    report("C. 富士 RAF 批次的片基水平（我自己的解码：LibRaw → 线性 ProPhoto）")
    print(f"{'file':<34}{'片基占满量程 %':>18}{'片基密度':>22}{'相对 G 的偏移':>20}")
    measured = {}
    for path in RAFS + [item[1] for item in OTHERS]:
        name = path.split("/")[-1]
        data = raw_base(path)
        measured[path] = data
        print(f"{name:<34}"
              f"{'/'.join(f'{value:5.2f}' for value in data['levels_pct']):>18}"
              f"{'/'.join(f'{value:5.2f}' for value in data['density']):>22}"
              f"{'/'.join(f'{value:+5.3f}' for value in data['relative']):>20}"
              f"   cam_wb={[round(value, 1) for value in data['cam_wb']]}")
    print()
    print("闸门 FILM_BASE_MAX_DENSITY = 1.60，任一通道超过即整帧拒绝（base → 0）")
    for path, data in measured.items():
        worst = float(np.max(data["density"]))
        verdict = "拒绝" if worst > 1.60 else "通过"
        print(f"  {path.split('/')[-1]:<34} max {worst:5.2f} D → {verdict}")
    return measured


def fallback(measured):
    report("D. 兜底分支（无片基时的 share_smart_auto_density_scale）")
    import rawpy

    path = RAFS[0]
    with rawpy.imread(path) as raw:
        rgb = raw.postprocess(half_size=True, output_bps=16, gamma=(1, 1),
                              no_auto_bright=True, use_camera_wb=False,
                              output_color=rawpy.ColorSpace.ProPhoto)
    height, width = rgb.shape[:2]
    margin_y, margin_x = int(height * 0.05), int(width * 0.05)
    inner = rgb[margin_y:height - margin_y, margin_x:width - margin_x, :]
    flat = inner.reshape(-1, 3).astype(np.float32)[::3] / 65535.0
    # This branch is reached with base_density = 0, so the raw densities are
    # exactly what the window sees.
    low, high = co_sited(density(flat))
    print("内容窗口(未减片基) dmin:", np.round(low, 3),
          " dmax:", np.round(high, 3))
    low_luma, high_luma, offsets = share_smart_auto(low, high)
    print("兜底后：共享跨度 luma =", round(high_luma - low_luma, 3),
          " 通道偏移 =", np.round(offsets, 3),
          " (上限 ±0.20)")
    real = measured[RAFS[0]]["relative"]
    print("真实色罩相对 G 的偏移 =", np.round(real, 3),
          " → R 需要", round(real[0], 3), "D，超过 ±0.20 上限" if abs(real[0]) > 0.20
          else "")
    residual = real - offsets
    print("按兜底偏移对齐后的残差 =", np.round(residual, 3),
          " (R 残差 %+.3f D ≈ %.0f%% 拍摄挡位)"
          % (residual[0], abs(residual[0]) / 0.30103 * 100))


if __name__ == "__main__":
    panorama()
    batch = raw_batch()
    fallback(batch)
