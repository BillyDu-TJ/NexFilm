"""Final acceptance review for the Rust three-layer prescribed response.

Part 1 checks the shipping Rust renders against the offline prototype's
predictions and the C1 references (frame-mean R/G, B/G - the convergence
criterion). Part 2 composes the eye-decision montage: shipping | Rust
three-layer | C1, each frame oriented the way its registration found it, so
the comparison is like with like.
"""

import json
import os
import sys

sys.path.insert(0, "target/pylibs")

import importlib.util  # noqa: E402

import numpy as np  # noqa: E402
from PIL import Image, ImageDraw  # noqa: E402

sys.stdout.reconfigure(encoding="utf-8")
_here = os.path.dirname(os.path.abspath(__file__))
_spec = importlib.util.spec_from_file_location(
    "verify27", os.path.join(_here, "verify27_joint_calib.py")
)
verify27 = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(verify27)

OUT = "target/prescribed-response"
SHIPPING = "target/prescribed-response-shipping"
RAFDIR = "test_picture/富士raf"
TAGS = [str(1942 + step) for step in range(11)]
LUMA = np.array([0.2126, 0.7152, 0.0722], dtype=np.float64)


def load(path, size=(512, 512)):
    return np.asarray(
        Image.open(path).convert("RGB").resize(size, Image.LANCZOS), dtype=np.float64
    ) / 255.0


def load_render(tag, root):
    frame = os.path.join(root, f"raf-{tag}-after-frame.png")
    if os.path.exists(frame):
        return load(frame)
    full = load(os.path.join(root, f"raf-{tag}-after.png"))
    m, n = full.shape[0], full.shape[1]
    return full[m // 5 : m - m // 5, n // 5 : n - n // 5]


def frame_mean_chroma(image):
    mean = image.reshape(-1, 3).mean(axis=0)
    return mean[0] / mean[1], mean[2] / mean[1]


def main():
    with open(f"{OUT}/profile.json", encoding="utf-8") as handle:
        data = json.load(handle)

    print("帧均值 R/G, B/G   （Rust 三层落地 vs 离线原型 vs C1）")
    print(f"{'帧':>5} | {'出货':>13} | {'Rust落地':>13} | {'C1':>13}")
    summary = []
    for tag in TAGS:
        shipping = load_render(tag, SHIPPING)
        rust = load_render(tag, OUT)
        ref = load(f"{RAFDIR}/Capture One Catalog{tag}.tif")
        cells = []
        for image in (shipping, rust, ref):
            rg, bg = frame_mean_chroma(image)
            cells.append(f"{rg:.3f}, {bg:.3f}")
        summary.append((tag, cells))
        print(f"{tag:>5} | {cells[0]:>13} | {cells[1]:>13} | {cells[2]:>13}")
    rust_rg = [float(c[1].split(",")[0]) for _, c in summary]
    print(f"\nRust 落地 R/G 散布: {min(rust_rg):.3f} - {max(rust_rg):.3f}")
    print("（离线原型预测: 0.973 - 1.105；出货基线: 0.796 - 1.128）")

    # The eye-decision montage. Each frame is oriented onto its reference the
    # way the registration found it, so every row compares like with like.
    cell = 360
    pad = 6
    label_h = 22
    titles = ["当前出货", "Rust 三层落地", "C1 参照"]
    width = len(titles) * (cell + pad) + pad
    height = len(TAGS) * (cell + label_h + pad) + pad
    canvas = Image.new("RGB", (width, height), (24, 24, 24))
    draw = ImageDraw.Draw(canvas)
    for column, title in enumerate(titles):
        draw.text((pad + column * (cell + pad) + 4, 4), title, fill=(240, 240, 240))
    for row, tag in enumerate(TAGS):
        rust = load_render(tag, OUT)
        ref = load(f"{RAFDIR}/Capture One Catalog{tag}.tif")
        aligned, _rot, _flip, _scale, _shift, corr = verify27.register(rust, ref)
        if corr < 0.5:
            aligned = rust
        shipping = load_render(tag, SHIPPING)
        top = pad + label_h + row * (cell + label_h + pad)
        draw.text((pad + 4, top + 2), f"raf-{tag}", fill=(180, 220, 255))
        for column, image in enumerate([shipping, aligned, ref]):
            image8 = Image.fromarray(
                (np.clip(image, 0, 1) * 255).astype(np.uint8)
            ).resize((cell, cell), Image.LANCZOS)
            canvas.paste(image8, (pad + column * (cell + pad), top + label_h))
    out_path = f"{OUT}/_review-final-three-layer.png"
    canvas.save(out_path)
    print(f"\nwrote {out_path}")


if __name__ == "__main__":
    main()
