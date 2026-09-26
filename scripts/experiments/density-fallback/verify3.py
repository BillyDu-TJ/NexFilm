"""Sensitivity of the RAF result to the film-base estimate, plus the two file
claims about the Lightroom panoramas."""

import sys

sys.path.insert(0, "target/pylibs")

import numpy as np  # noqa: E402
import rawpy  # noqa: E402
import tifffile  # noqa: E402
from PIL import Image  # noqa: E402

from verify2 import (PROPHOTO_TO_SRGB, W, absorption, app_style_base, co_sited,
                     display, srgb_decode, SRGB_TO_PROPHOTO)  # noqa: E402

sys.stdout.reconfigure(encoding="utf-8")

RAF = "test_picture/富士raf/Capture One Catalog1942.raf"
REF = "test_picture/富士raf/Capture One Catalog1942.tif"


def chroma(linear_prophoto):
    flat = linear_prophoto.reshape(-1, 3)
    green = np.maximum(flat[:, 1], 1e-6)
    luma = flat @ W
    mid = (luma > 0.02) & (luma < 0.6)
    channel = np.median(flat, axis=0)
    return (channel[0] / channel[1], channel[2] / channel[1],
            float(np.median(np.abs(flat[mid, 0] / green[mid] - 1.0))),
            float(np.median(np.abs(flat[mid, 2] / green[mid] - 1.0))))


def main():
    with rawpy.imread(RAF) as raw:
        rgb = raw.postprocess(half_size=True, output_bps=16, gamma=(1, 1),
                              no_auto_bright=True, use_camera_wb=False,
                              output_color=rawpy.ColorSpace.ProPhoto)
        cam_wb = np.asarray(raw.camera_whitebalance, dtype=np.float32)
    gains = cam_wb[:3] / cam_wb[:3].max()
    lin = rgb.astype(np.float32) / 65535.0 * gains

    height, width = lin.shape[:2]
    margin_y, margin_x = int(height * 0.04), int(width * 0.04)
    inner = lin[margin_y:height - margin_y, margin_x:width - margin_x]
    d_abs = absorption(inner)
    mine = app_style_base(inner)
    theirs = np.array([1.482, 1.678, 2.025])
    print("my base           :", np.round(mine, 3))
    print("their corrected   :", np.round(theirs, 3))
    print("their earlier     :", np.round(np.array([1.86, 2.08, 2.03]), 3))
    print()

    encoded_ref = np.asarray(Image.open(REF).convert("RGB"), dtype=np.float32) / 255.0
    ref = srgb_decode(encoded_ref) @ SRGB_TO_PROPHOTO.T
    print(f"{'base used':<34}{'R/G':>7}{'B/G':>7}{'|R/G-1|':>9}{'|B/G-1|':>9}")
    print(f"{'Capture One reference':<34}{chroma(ref)[0]:>7.3f}{chroma(ref)[1]:>7.3f}"
          f"{chroma(ref)[2]:>9.3f}{chroma(ref)[3]:>9.3f}")

    for label, base in [
        ("my own base", mine),
        ("their corrected base", theirs),
        ("their first-round base", np.array([1.86, 2.08, 2.03])),
        ("my base, blue -0.15", mine - np.array([0, 0, 0.15])),
        ("my base, blue +0.15", mine + np.array([0, 0, 0.15])),
        ("my base, R -0.15", mine - np.array([0.15, 0, 0])),
    ]:
        relative = d_abs - base
        low, high = co_sited(relative.reshape(-1, 3)[::5])
        render = np.clip((relative - low) / (high - low), 0, 1)
        shown = display(render).astype(np.float32) / 255.0
        values = chroma(srgb_decode(shown) @ SRGB_TO_PROPHOTO.T)
        print(f"{label:<34}{values[0]:>7.3f}{values[1]:>7.3f}"
              f"{values[2]:>9.3f}{values[3]:>9.3f}")

    print()
    print("Lightroom panorama file claims")
    for path, label in [("test_picture/lr合并/_DSC7587-2-Pano.tif", "new export"),
                        ("test_picture/lr合并/_DSC7587-Pano.tif", "old auto-WB export")]:
        arr = tifffile.imread(path)
        total = arr.shape[0] * arr.shape[1]
        zeros = [float((arr[:, :, channel] == 0).mean() * 100) for channel in range(3)]
        clipped = [float((arr[:, :, channel] == 65535).mean() * 100) for channel in range(3)]
        inner = arr[400:arr.shape[0] - 400, 400:arr.shape[1] - 400]
        inner_zero = [float((inner[:, :, channel] == 0).mean() * 100) for channel in range(3)]
        print(f"  {label:<22}{arr.shape} 0-value % R/G/B {np.round(zeros, 2)} "
              f"(frame interior {np.round(inner_zero, 2)})  "
              f"65535-value % {np.round(clipped, 2)}")


if __name__ == "__main__":
    main()
