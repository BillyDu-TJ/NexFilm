"""Is the proposed "span imbalance > 2.0" gate actually clean? Measured on the
same fixture set the first investigation used: 13 LR panoramas against the
normal material (Nikon scanner TIFFs and Noritsu JPEGs)."""

import glob
import struct
import sys

sys.path.insert(0, "target/pylibs")

import numpy as np  # noqa: E402
import tifffile  # noqa: E402
from PIL import Image  # noqa: E402

from verify2 import W, absorption, co_sited  # noqa: E402

sys.stdout.reconfigure(encoding="utf-8")

SPACE_MATRIX = {
    "srgb": np.array([[0.4124564, 0.3575761, 0.1804375],
                      [0.2126729, 0.7151522, 0.0721750],
                      [0.0193339, 0.1191920, 0.9503041]]),
    "adobe": np.array([[0.5767309, 0.1855540, 0.1881852],
                       [0.2973769, 0.6273491, 0.0752741],
                       [0.0270343, 0.0706872, 0.9911085]]),
    "prophoto": np.array([[0.7976749, 0.1351917, 0.0313534],
                          [0.2880402, 0.7118741, 0.0000857],
                          [0.0000000, 0.0000000, 0.8252100]]),
}
D65_TO_D50 = np.array([[1.0478112, 0.0228866, -0.0501270],
                       [0.0295424, 0.9904844, -0.0170491],
                       [-0.0092345, 0.0150436, 0.7521316]])


def icc_space(path):
    try:
        with tifffile.TiffFile(path) as handle:
            page = handle.pages[0]
            tag = page.tags.get("ICCProfile") or page.tags.get("InterColorProfile")
            if tag is None:
                return None, None
            icc = bytes(tag.value)
    except Exception:
        return None, None
    count = struct.unpack(">I", icc[128:132])[0]
    offset, tags = 132, {}
    for _ in range(count):
        signal = icc[offset:offset + 4]
        at, size = struct.unpack(">II", icc[offset + 4:offset + 12])
        tags[signal.decode("latin1")] = (at, size)
        offset += 12
    space = None
    if "desc" in tags:
        at, size = tags["desc"]
        text = icc[at + 12:at + size].split(b"\x00")[0].decode("latin1").lower()
        for name in ("prophoto", "adobe", "srgb"):
            if name in text:
                space = name
    gamma = None
    if "rTRC" in tags:
        at, size = tags["rTRC"]
        trc = icc[at:at + size]
        if trc[:4] == b"curv" and struct.unpack(">I", trc[8:12])[0] == 1:
            gamma = struct.unpack(">H", trc[12:14])[0] / 256.0
    return space, gamma


def to_working(path, stride=3):
    """Linear working space (ProPhoto D50), same idea as the app."""
    space, gamma = icc_space(path)
    if path.lower().endswith((".jpg", ".jpeg")):
        encoded = np.asarray(Image.open(path).convert("RGB"),
                             dtype=np.float32)[::stride, ::stride] / 255.0
        linear = np.where(encoded <= 0.04045, encoded / 12.92,
                          ((encoded + 0.055) / 1.055) ** 2.4)
        space = "srgb"
    else:
        encoded = tifffile.memmap(path)[::stride, ::stride].astype(np.float32) / 65535.0
        linear = encoded ** (gamma or 2.2)
    space = space or "srgb"
    matrix = np.linalg.inv(SPACE_MATRIX["prophoto"])
    if space != "prophoto":
        matrix = matrix @ (D65_TO_D50 if space == "srgb" else np.eye(3)) @ SPACE_MATRIX[space]
    return np.maximum(linear @ matrix.T, 0.0), space, gamma or 2.2


def span_ratio(linear):
    height, width = linear.shape[:2]
    band_y, band_x = max(2, int(height * 0.03)), max(2, int(width * 0.03))
    ring = np.zeros((height, width), bool)
    ring[:band_y, :] = ring[-band_y:, :] = True
    ring[:, :band_x] = ring[:, -band_x:] = True
    inner = np.zeros((height, width), bool)
    inner[band_y * 2:height - band_y * 2, band_x * 2:width - band_x * 2] = True
    luma = linear @ W
    ring_luma = np.median(luma[ring]) if ring.any() else 0.0
    if ring_luma > np.percentile(luma[inner], 75):
        base = np.median(linear[ring], axis=0)
        source = "edge band"
    else:
        flat = linear[inner]
        usable = flat[np.all((flat > 0) & (flat < 0.98), axis=1)]
        tail = max(1, int(np.ceil(len(usable) * 0.001)))
        base = np.sort(usable, axis=0)[-tail:].mean(axis=0)
        source = "content high quantile"
    positive = np.log10(np.maximum(base, 1e-9) / np.maximum(linear[inner], 1e-9))
    samples = positive[::2]
    low, high = co_sited(samples)
    span = high - low
    return span, base, source, float(span.max() / span.min())


def main():
    groups = [
        ("LR 拼接 pano", sorted(glob.glob("test_picture/lr合并/*.tif"))),
        ("尼康扫描仪 tiff", sorted(glob.glob("test_picture/尼康扫描仪tiff/*.tif"))),
        ("诺日士 / 杂项 jpg",
         sorted(glob.glob("test_picture/诺日士jpg/*.jpg")) + ["test_picture/000021940005.jpg"]),
    ]
    print(f"{'file':<34}{'space/gamma':>14}{'base src':>22}"
          f"{'spans R/G/B':>22}{'max/min':>9}")
    for label, paths in groups:
        print(f"--- {label}")
        for path in paths:
            linear, space, gamma = to_working(path)
            span, base, source, ratio = span_ratio(linear)
            print(f"{path.split('/')[-1]:<34}{f'{space} {gamma:.2f}':>14}{source:>22}"
                  f"{'/'.join(f'{value:5.2f}' for value in span):>22}{ratio:>9.2f}")


if __name__ == "__main__":
    main()
