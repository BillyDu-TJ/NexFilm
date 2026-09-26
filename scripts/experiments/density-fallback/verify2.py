"""Does the missing film base explain the RAF cast, and does subtracting it fix
the hue? Checked against Capture One's own 8-bit render of the same frame,
which is an independent reference (this script never touches app code).

A  = what the app does when the base estimate is rejected (base 0, content
     alignment with the +-0.20 clamp)
B  = the proposed step 1: subtract the measured base, keep the per-channel
     co-sited content window
"""

import sys

sys.path.insert(0, "target/pylibs")

import numpy as np  # noqa: E402
import rawpy  # noqa: E402
import tifffile  # noqa: E402
from PIL import Image  # noqa: E402

sys.stdout.reconfigure(encoding="utf-8")

W = np.array([0.2126, 0.7152, 0.0722])

XYZ = {
    "srgb": np.array([[0.4124564, 0.3575761, 0.1804375],
                      [0.2126729, 0.7151522, 0.0721750],
                      [0.0193339, 0.1191920, 0.9503041]]),
    "prophoto": np.array([[0.7976749, 0.1351917, 0.0313534],
                          [0.2880402, 0.7118741, 0.0000857],
                          [0.0000000, 0.0000000, 0.8252100]]),
}
D65_TO_D50 = np.array([[1.0478112, 0.0228866, -0.0501270],
                       [0.0295424, 0.9904844, -0.0170491],
                       [-0.0092345, 0.0150436, 0.7521316]])
SRGB_TO_PROPHOTO = np.linalg.inv(XYZ["prophoto"]) @ D65_TO_D50 @ XYZ["srgb"]
PROPHOTO_TO_SRGB = np.linalg.inv(SRGB_TO_PROPHOTO)

REF = "test_picture/富士raf/Capture One Catalog1942.tif"
RAF = "test_picture/富士raf/Capture One Catalog1942.raf"


def srgb_decode(encoded):
    return np.where(encoded <= 0.04045, encoded / 12.92,
                    ((encoded + 0.055) / 1.055) ** 2.4)


def srgb_encode(linear):
    value = np.clip(linear, 0.0, 1.0)
    return np.where(value <= 0.0031308, value * 12.92,
                    1.055 * value ** (1 / 2.4) - 0.055)


def absorption(linear):
    return -np.log10(np.maximum(linear, 1e-7))


def co_sited(samples, fraction=0.02):
    order = np.argsort(samples @ W)
    k = max(1, int(np.ceil(len(samples) * fraction)))
    return samples[order[:k]].mean(axis=0), samples[order[-k:]].mean(axis=0)


def share_smart_auto(d_min, d_max):
    limit = 0.20
    low, high = float(np.dot(d_min, W)), float(np.dot(d_max, W))
    centers = (np.asarray(d_min) + np.asarray(d_max)) * 0.5
    offsets = np.clip(centers - float(np.dot(centers, W)), -limit, limit)
    offsets = np.clip(offsets - float(np.dot(offsets, W)), -limit, limit)
    return low, high, offsets


def display(working_linear):
    srgb = np.clip(working_linear, 0.0, 1.0) @ PROPHOTO_TO_SRGB.T
    return (srgb_encode(srgb) * 255 + 0.5).astype(np.uint8)


def to_linear_prophoto(encoded_rgb):
    return srgb_decode(encoded_rgb) @ SRGB_TO_PROPHOTO.T


def chroma_stats(linear_prophoto, label):
    """Frame statistics: median level, and how far mid-tone pixels sit off the
    green axis. Comparable across images without pixel alignment."""
    flat = linear_prophoto.reshape(-1, 3)
    channel = np.median(flat, axis=0)
    green = np.maximum(flat[:, 1], 1e-6)
    luma = flat @ W
    mid = (luma > 0.02) & (luma < 0.6)
    rg = np.abs(flat[mid, 0] / green[mid] - 1.0)
    bg = np.abs(flat[mid, 2] / green[mid] - 1.0)
    print(f"  {label:<26} RGB {np.round(channel, 4)}  "
          f"R/G {channel[0] / channel[1]:.3f} B/G {channel[2] / channel[1]:.3f}  "
          f"|R/G-1| {np.median(rg):.3f} |B/G-1| {np.median(bg):.3f}")


def app_style_base(linear):
    """compute_frame_base_density_f32: per channel, mean of the brightest 1%,
    then density. This is the loose-frame estimator the gate rejects."""
    flat = linear.reshape(-1, 3)
    keep = np.all((flat > 0) & (flat < 0.98), axis=1)
    usable = flat[keep]
    tail = max(1, int(np.ceil(len(usable) * 0.01)))
    top = np.sort(usable, axis=0)[-tail:].mean(axis=0)
    return -np.log10(np.maximum(top, 1e-6))


def run(lin, ref, tag, out):
    print()
    print("=" * 72)
    print(tag)
    print("=" * 72)
    height, width = lin.shape[:2]
    margin_y, margin_x = int(height * 0.04), int(width * 0.04)
    inner = lin[margin_y:height - margin_y, margin_x:width - margin_x]
    d_abs = absorption(inner)
    base = app_style_base(inner)
    print("base density (brightest 1% mean per channel):", np.round(base, 3),
          " levels %:", np.round(10 ** (-base) * 100, 2))

    samples = d_abs.reshape(-1, 3)[::5]
    # A: the app's rejected-base branch (base 0 -> content alignment).
    low_a, high_a, offsets_a = share_smart_auto(*co_sited(samples))
    print("A window: luma span", round(high_a - low_a, 3),
          "offsets", np.round(offsets_a, 3))
    span_a = high_a - low_a
    lo_a = low_a + offsets_a
    render_a = np.clip((d_abs - lo_a) / span_a, 0, 1)

    # B: subtract the measured base, keep the per-channel co-sited window.
    relative = d_abs - base
    low_b, high_b = co_sited(relative.reshape(-1, 3)[::5])
    print("B window: dmin", np.round(low_b, 3), "dmax", np.round(high_b, 3),
          "span ratio R:G:B 1 :", round((high_b - low_b)[1] / (high_b - low_b)[0], 2),
          ":", round((high_b - low_b)[2] / (high_b - low_b)[0], 2))
    render_b = np.clip((relative - low_b) / (high_b - low_b), 0, 1)

    Image.fromarray(display(render_a)).save(out + "_raf_A_base_rejected.png")
    Image.fromarray(display(render_b)).save(out + "_raf_B_base_removed.png")
    # Compare everything in the same space: an sRGB encode that is decoded back
    # to linear ProPhoto, which is what the reference already is.
    a = to_linear_prophoto(display(render_a).astype(np.float32) / 255.0)
    b = to_linear_prophoto(display(render_b).astype(np.float32) / 255.0)
    print()
    print("frame statistics in linear ProPhoto (mid-tone |ratio-1| = cast)")
    chroma_stats(ref, "Capture One reference")
    chroma_stats(a, "A (base rejected)")
    chroma_stats(b, "B (base removed)")


def main():
    ref_encoded = np.asarray(Image.open(REF).convert("RGB"), dtype=np.float32) / 255.0
    ref = srgb_decode(ref_encoded) @ SRGB_TO_PROPHOTO.T
    print("reference (Capture One):", ref_encoded.shape)

    with rawpy.imread(RAF) as raw:
        rgb = raw.postprocess(half_size=True, output_bps=16, gamma=(1, 1),
                              no_auto_bright=True, use_camera_wb=False,
                              output_color=rawpy.ColorSpace.ProPhoto)
        cam_wb = np.asarray(raw.camera_whitebalance, dtype=np.float32)
    lin = rgb.astype(np.float32) / 65535.0
    print("raw decode:", lin.shape, " cam_wb", cam_wb)
    out = "scripts/experiments/density-fallback/"
    run(lin, ref, "raw levels, no white balance applied", out)
    gains = cam_wb[:3] / cam_wb[:3].max()
    run(lin * gains, ref,
        f"camera WB normalised to max channel ({np.round(gains, 3)})", out)
    print()
    print("A images:", out + "_raf_A_base_rejected.png")
    print("B images:", out + "_raf_B_base_removed.png")


if __name__ == "__main__":
    main()
