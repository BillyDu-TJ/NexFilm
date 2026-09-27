"""Apply the calibrated film profile plus a bounded per-frame normalizer, and
compare against the shipping render and the C1 references.

Three-layer architecture, offline simulation:

  1. profile   - fixed per-band log-chroma gains (the film's crossover),
                 calibrated once with each frame's white balance projected out
                 (`verify27_joint_calib.py`); applied everywhere the evidence
                 marker fires.
  3. normalize - a per-frame bounded chroma gain pulled toward neutral, made
                 robust by choosing between two estimators (mid-tone gray-world
                 and gray-edge), applied luma-preservingly. This layer carries
                 what no constant can: the frame's own white-balance random
                 draw. Clamped to +-GRAY_BOUND so a scene's genuine colour
                 survives.

The estimator bake-off scores each estimator's implied correction against the
white balance C1 actually chose per frame (the nuisance values from the
calibration) - the estimator closest to those decisions is the one worth
shipping. 1949, whose reference is a C1-stylized sunset, is the protection
probe: the normalizer must keep our neutral render neutral, not chase the
reference.
"""

import json
import os
import sys

sys.path.insert(0, "target/pylibs")

import numpy as np  # noqa: E402
from PIL import Image, ImageDraw  # noqa: E402

import importlib.util  # noqa: E402

_spec = importlib.util.spec_from_file_location(
    "verify27", os.path.join(os.path.dirname(__file__), "verify27_joint_calib.py")
)
verify27 = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(verify27)

sys.stdout.reconfigure(encoding="utf-8")
RAFDIR = "test_picture/富士raf"
OUT = "target/prescribed-response"
SHIPPING = "target/prescribed-response-shipping"
TAGS = [str(1942 + step) for step in range(11)]
LUMA = np.array([0.2126, 0.7152, 0.0722], dtype=np.float64)
GRAY_BOUND = 0.08
LAMBDAS = (1.0, 0.7)


def load_display(path, size=(512, 512)):
    return np.asarray(
        Image.open(path).convert("RGB").resize(size, Image.LANCZOS),
        dtype=np.float64,
    ) / 255.0


def log_chroma(image, mask):
    c = np.log(np.clip(image[mask], 1.0 / 255, 1.0))
    return float(np.median(c[:, 0] - c[:, 1])), float(np.median(c[:, 2] - c[:, 1]))


def midtone_mask(luma):
    lo, hi = np.quantile(luma, [0.25, 0.75])
    return (luma >= lo) & (luma <= hi) & (luma > 0.01)


def gray_edge_mask(luma):
    """Luminance edges inside the mid-tone window, away from the canvas
    margin. Edges are robust to large uniform colour fields (a red shirt's
    interior does not vote), and the mid-tone window keeps bright skies from
    dominating the blue vote. The bake-off against C1's per-frame decisions
    scored this variant best and stable across working resolutions."""
    height, width = luma.shape
    inner = np.zeros_like(luma, dtype=bool)
    my, mx = int(height * 0.04), int(width * 0.04)
    inner[my : height - my, mx : width - mx] = True
    gy, gx = np.gradient(luma)
    grad = np.hypot(gx, gy)
    threshold = np.quantile(grad, 0.6)
    mt = midtone_mask(luma)
    return inner & (grad > threshold) & mt


def estimate_cast(image, estimator):
    luma = image @ LUMA
    mask = midtone_mask(luma) if estimator == "midtone" else gray_edge_mask(luma)
    return log_chroma(image, mask)


def normalize(image, estimator, lam, bound=GRAY_BOUND):
    lr, lb = estimate_cast(image, estimator)
    gr = np.exp(np.clip(-lam * lr, -bound, bound))
    gb = np.exp(np.clip(-lam * lb, -bound, bound))
    k = 0.2126 * gr + 0.7152 + 0.0722 * gb
    return np.stack(
        [image[:, :, 0] * gr / k, image[:, :, 1] / k, image[:, :, 2] * gb / k], axis=2
    ), (gr, gb)


class Profile:
    def __init__(self, data):
        self.centers = np.asarray(data["band_centers"], dtype=np.float64)
        self.log_rg = np.asarray(data["log_rg"], dtype=np.float64)
        self.log_bg = np.asarray(data["log_bg"], dtype=np.float64)

    def gains(self, luma):
        rg = np.interp(luma, self.centers, self.log_rg)
        bg = np.interp(luma, self.centers, self.log_bg)
        return np.exp(rg), np.exp(bg)


def apply_profile(image, profile):
    luma = image @ LUMA
    gr, gb = profile.gains(luma)
    k = 0.2126 * gr + 0.7152 + 0.0722 * gb
    return np.stack(
        [image[:, :, 0] * gr / k, image[:, :, 1] / k, image[:, :, 2] * gb / k], axis=2
    )


def frame_mean_chroma(image):
    mean = image.reshape(-1, 3).mean(axis=0)
    return mean[0] / mean[1], mean[2] / mean[1]


def main():
    with open(f"{OUT}/profile.json", encoding="utf-8") as handle:
        data = json.load(handle)
    profile = Profile(data)
    good = set(data["good_frames"])

    frames = {}
    for tag in TAGS:
        frame_path = f"{OUT}/raf-{tag}-after-frame.png"
        if os.path.exists(frame_path):
            source = load_display(frame_path)
        else:
            full = load_display(f"{OUT}/raf-{tag}-after.png")
            m, n = full.shape[0], full.shape[1]
            source = full[m // 5 : m - m // 5, n // 5 : n - n // 5]
        shipping_path = f"{SHIPPING}/raf-{tag}-after-frame.png"
        if os.path.exists(shipping_path):
            shipping = load_display(shipping_path)
        else:
            full = load_display(f"{SHIPPING}/raf-{tag}-after.png")
            m, n = full.shape[0], full.shape[1]
            shipping = full[m // 5 : m - m // 5, n // 5 : n - n // 5]
        ref = load_display(f"{RAFDIR}/Capture One Catalog{tag}.tif")
        aligned, _, _, _, _, corr = verify27.register(source, ref)
        frames[tag] = {
            "shipping": shipping,
            "ref": ref,
            "rule5": source,
            "aligned": aligned,
            "corr": corr,
        }

    # Estimator bake-off: implied per-frame correction vs C1's actual decision.
    print("估计器对比（估计的校正 vs C1 实际白平衡决策，log 域，越小越接近）:")
    profiled = {
        tag: apply_profile(d["rule5"], profile) for tag, d in frames.items()
    }
    for estimator in ("midtone", "gray_edge"):
        errs = []
        for tag in sorted(good):
            lr, lb = estimate_cast(profiled[tag], estimator)
            wr, wb = data["wb"][tag]
            errs.append((abs(lr - wr), abs(lb - wb)))
            print(
                f"  {tag}: 估计 ({lr:+.3f},{lb:+.3f})  C1 决策 ({wr:+.3f},{wb:+.3f})"
            )
        e = np.asarray(errs)
        print(f"  -> {estimator}: 中位误差 logR/G {np.median(e[:,0]):.3f}  logB/G {np.median(e[:,1]):.3f}")

    # Metrics per variant.
    print("\n帧均值 R/G, B/G（收敛到 1 附近 + 帧间收敛是判据）:")
    print(f"{'帧':>5} | {'出货':>13} | {'+档案':>13} | {'+档案+归一(λ=1)':>16} | {'C1':>13}")
    for tag in TAGS:
        d = frames[tag]
        normed, gains = normalize(profiled[tag], "gray_edge", 1.0)
        cells = []
        for image in (d["shipping"], profiled[tag], normed, d["ref"]):
            if image is None:
                cells.append(" " * 13)
            else:
                rg, bg = frame_mean_chroma(image)
                cells.append(f"{rg:.3f}, {bg:.3f}".rjust(13))
        extra = ""
        if tag == "1949":
            extra = f"   <- 1949 增益 {gains[0]:.3f},{gains[1]:.3f}（保护探针）"
        print(f"{tag:>5} | {cells[0]} | {cells[1]} | {cells[2]:>16} | {cells[3]}{extra}")

    # Binned chroma deltas on the fitted frames (registration exact there).
    print("\n逐档色度残差中位（|Δlog R/G|, |Δlog B/G|，档2-9，配准精确的帧）:")
    BANDS = 8
    for tag in sorted(good):
        d = frames[tag]
        normed, _ = normalize(profiled[tag], "gray_edge", 1.0)
        normed_aligned, _, _, _, _, _ = verify27.register(normed, d["ref"])
        row = []
        for name, image in (("出货", d["shipping"]), ("新", normed_aligned)):
            ol = image @ LUMA
            rl = d["ref"] @ LUMA
            edges = np.quantile(rl, np.linspace(0, 1, BANDS + 1))
            edges[0], edges[-1] = -0.01, 1.01
            d_raw_r, d_raw_b = [], []
            for i in range(2, BANDS - 2):
                m = (rl >= edges[i]) & (rl < edges[i + 1])
                if m.sum() < 2000:
                    continue
                oc = np.log(np.clip(image[m], 1 / 255, 1))
                rc = np.log(np.clip(d["ref"][m], 1 / 255, 1))
                d_raw_r.append(np.median(oc[:, 0] - oc[:, 1]) - np.median(rc[:, 0] - rc[:, 1]))
                d_raw_b.append(np.median(oc[:, 2] - oc[:, 1]) - np.median(rc[:, 2] - rc[:, 1]))
            if d_raw_r:
                # Project out the frame's constant (its WB choice) from both
                # sides; the residual is the shape mismatch per band.
                dr = np.abs(np.asarray(d_raw_r) - np.median(d_raw_r))
                db = np.abs(np.asarray(d_raw_b) - np.median(d_raw_b))
                row.append(f"{name} R {np.median(dr):.3f} B {np.median(db):.3f}")
        print(f"  {tag}: " + "   |   ".join(row))

    # Montage: shipping | profile+normalize | C1.
    cell = 360
    pad = 6
    label_h = 22
    columns = ["当前出货", "新: 档案+有界归一", "C1 参照"]
    width = len(columns) * (cell + pad) + pad
    height = len(TAGS) * (cell + label_h + pad) + pad
    canvas = Image.new("RGB", (width, height), (24, 24, 24))
    draw = ImageDraw.Draw(canvas)
    for column, title in enumerate(columns):
        draw.text((pad + column * (cell + pad) + 4, 4), title, fill=(240, 240, 240))
    for row, tag in enumerate(TAGS):
        d = frames[tag]
        normed, _ = normalize(profiled[tag], "gray_edge", 1.0)
        top = pad + label_h + row * (cell + label_h + pad)
        draw.text((pad + 4, top + 2), f"raf-{tag}", fill=(180, 220, 255))
        for column, image in enumerate([d["shipping"], normed, d["ref"]]):
            if image is None:
                continue
            image8 = Image.fromarray((np.clip(image, 0, 1) * 255).astype(np.uint8))
            canvas.paste(image8.resize((cell, cell), Image.LANCZOS),
                         (pad + column * (cell + pad), top + label_h))
    out_path = f"{OUT}/_review-three-layer.png"
    canvas.save(out_path)
    print(f"\nwrote {out_path}")


if __name__ == "__main__":
    main()
