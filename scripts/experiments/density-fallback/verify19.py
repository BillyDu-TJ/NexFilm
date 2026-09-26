"""Fit one preset per-channel span gain from the sweep, and see whether a single
preset can serve the whole batch.

For every frame we look for the (u_R, u_B) whose frame-mean channel ratios sit
closest to Capture One's own frame means. If those optima agree across frames,
the preset is real; if they scatter, a preset cannot exist and the window needs
something else.
"""

import sys

sys.path.insert(0, "target/pylibs")

import numpy as np  # noqa: E402
from PIL import Image  # noqa: E402

sys.stdout.reconfigure(encoding="utf-8")
RAFDIR = "test_picture/富士raf"


def reference_means():
    means = {}
    for step in range(11):
        tag = str(1942 + step)
        image = np.asarray(
            Image.open(f"{RAFDIR}/Capture One Catalog{tag}.tif").convert("RGB"),
            dtype=np.float32) / 255.0
        green = image[:, :, 1].mean()
        means[tag] = (image[:, :, 0].mean() / green, image[:, :, 2].mean() / green)
    return means


def main():
    means = reference_means()
    sweep = {}
    with open("scripts/experiments/density-fallback/gain-sweep.txt", encoding="utf-8") as handle:
        for line in handle:
            if not line.startswith("[GAIN] "):
                continue
            parts = line.split()
            name = parts[1]
            tag = name.split("-")[1]
            u_r = float(parts[2].split("=")[1])
            u_b = float(parts[3].split("=")[1])
            rg = float(parts[4].split("=")[1])
            bg = float(parts[5].split("=")[1])
            sweep.setdefault(tag, []).append((u_r, u_b, rg, bg))
    print(f"{'frame':<7}{'参照 R/G,B/G':>18}{'最佳 u_R':>10}{'最佳 u_B':>10}"
          f"{'该点 R/G,B/G':>18}{'残差':>9}")
    best_pairs = []
    for tag in sorted(sweep):
        ref_rg, ref_bg = means[tag]
        best = None
        for u_r, u_b, rg, bg in sweep[tag]:
            error = max(abs(rg - ref_rg), abs(bg - ref_bg))
            if best is None or error < best[0]:
                best = (error, u_r, u_b, rg, bg)
        best_pairs.append((tag, best[1], best[2], best[0]))
        print(f"{tag:<7}{ref_rg:>8.3f},{ref_bg:<9.3f}{best[1]:>10.2f}{best[2]:>10.2f}"
              f"{best[3]:>8.3f},{best[4]:<9.3f}{best[0]:>9.3f}")
    u_r_values = np.array([pair[1] for pair in best_pairs])
    u_b_values = np.array([pair[2] for pair in best_pairs])
    print()
    print(f"最佳 u_R：中位 {np.median(u_r_values):.2f}  范围 {u_r_values.min():.2f}–{u_r_values.max():.2f}")
    print(f"最佳 u_B：中位 {np.median(u_b_values):.2f}  范围 {u_b_values.min():.2f}–{u_b_values.max():.2f}")
    median_error = np.median([pair[3] for pair in best_pairs])
    print(f"各帧在各自最优点上的残差中位 {median_error:.3f}")


if __name__ == "__main__":
    main()
