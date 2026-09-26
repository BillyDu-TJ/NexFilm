"""Stage 0, part 4: is the underexposed RAF batch's blue channel carrying any
usable chroma information at all?

For every pixel we take our own relative density and the Capture One render,
remove the part that lightness alone explains, and correlate what is left.
A channel that still carries colour must correlate; a channel that is only
noise cannot. Red is the control: it has signal in every one of these frames.
"""

import sys

sys.path.insert(0, "target/pylibs")
sys.path.insert(0, "scripts/experiments/density-fallback")

import numpy as np  # noqa: E402

from verify2 import W, SRGB_TO_PROPHOTO, srgb_decode  # noqa: E402
from verify6 import raf_frame  # noqa: E402

sys.stdout.reconfigure(encoding="utf-8")
RAFS = {"1942": (100, 10, 762), "1947": (100, 10, 762),
        "1949": (90, 4, 770), "1950": (92, 2, 772)}


def clean(level, value, bins=24):
    """Remove everything that a monotone function of the level can explain."""
    edges = np.quantile(level, np.linspace(0, 1, bins + 1))
    edges[-1] += 1e-9
    index = np.clip(np.digitize(level, edges) - 1, 0, bins - 1)
    out = value.copy()
    for b in range(bins):
        rows = index == b
        if rows.sum() > 10:
            out[rows] -= np.median(value[rows])
    return out


def main():
    print("每个通道：去掉『明度能解释的部分』之后，我们的密度与 Capture One 的色度还有多少相关")
    print(f"  {'frame':<7}{'红通道 corr':>14}{'蓝通道 corr':>14}{'样本数':>10}")
    import PIL.Image as PILImage  # noqa: N813

    for tag, (x0, y0, side) in RAFS.items():
        _, block, _, _ = raf_frame(tag, x0, y0, side)
        reference = srgb_decode(np.asarray(
            PILImage.open(f"test_picture/富士raf/Capture One Catalog{tag}.tif")
            .convert("RGB").resize((side, side), PILImage.LANCZOS),
            dtype=np.float32) / 255.0) @ SRGB_TO_PROPHOTO.T
        luma = reference @ W
        chart = np.stack([np.log10(np.maximum(reference[:, :, 0], 1e-6)
                                   / np.maximum(reference[:, :, 1], 1e-6)),
                          np.log10(np.maximum(reference[:, :, 2], 1e-6)
                                   / np.maximum(reference[:, :, 1], 1e-6))], axis=-1)
        mask = (luma > 0.02) & (luma < 0.8)
        step = max(1, mask.sum() // 150000)
        level = block[:, :, 1][mask][::step]
        rows = []
        for channel in (0, 2):
            ours = clean(level, block[:, :, channel][mask][::step])
            theirs = clean(luma[mask][::step],
                           chart[:, :, 0 if channel == 0 else 1][mask][::step])
            rows.append(np.corrcoef(ours, theirs)[0, 1])
        print(f"  {tag:<7}{rows[0]:>14.3f}{rows[1]:>14.3f}{mask.sum() // step:>10}")
    print()
    print("对照：正常素材的片基电平（蓝通道可用动态范围）")
    print("  RAF 批次: 0.59–0.62% 满量程   |   尼康 NEF: 40%   |   扫描仪 TIFF/诺日士: 4–33%")


if __name__ == "__main__":
    main()
