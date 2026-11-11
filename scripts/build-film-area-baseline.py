#!/usr/bin/env python3
"""Turn legacy detector output into the normal film-area result schema."""

from __future__ import annotations

import argparse
import json
from collections import defaultdict
from pathlib import Path


def load(path: Path) -> dict:
    return json.loads(path.read_text(encoding="utf-8"))


def signed_area(points):
    return sum(
        points[index][0] * points[(index + 1) % len(points)][1]
        - points[(index + 1) % len(points)][0] * points[index][1]
        for index in range(len(points))
    ) / 2


def polygon_area(points):
    return abs(signed_area(points)) if len(points) >= 3 else 0.0


def ordered(points):
    result = [[float(point[0]), float(point[1])] for point in points]
    return list(reversed(result)) if signed_area(result) < 0 else result


def inside(point, edge_start, edge_end):
    cross = (
        (edge_end[0] - edge_start[0]) * (point[1] - edge_start[1])
        - (edge_end[1] - edge_start[1]) * (point[0] - edge_start[0])
    )
    return cross >= -1.0e-9


def intersection(start, end, edge_start, edge_end):
    sx, sy = end[0] - start[0], end[1] - start[1]
    ex, ey = edge_end[0] - edge_start[0], edge_end[1] - edge_start[1]
    denominator = sx * ey - sy * ex
    if abs(denominator) < 1.0e-12:
        return start
    dx, dy = edge_start[0] - start[0], edge_start[1] - start[1]
    t = (dx * ey - dy * ex) / denominator
    return [start[0] + sx * t, start[1] + sy * t]


def clip(subject, clip_polygon):
    output = subject
    for index in range(len(clip_polygon)):
        edge_start = clip_polygon[index]
        edge_end = clip_polygon[(index + 1) % len(clip_polygon)]
        input_points = output
        output = []
        if not input_points:
            break
        previous = input_points[-1]
        previous_inside = inside(previous, edge_start, edge_end)
        for current in input_points:
            current_inside = inside(current, edge_start, edge_end)
            if current_inside:
                if not previous_inside:
                    output.append(intersection(previous, current, edge_start, edge_end))
                output.append(current)
            elif previous_inside:
                output.append(intersection(previous, current, edge_start, edge_end))
            previous = current
            previous_inside = current_inside
    return output


def leakage_area(detected, truth):
    detected = ordered(detected)
    truth = ordered(truth)
    return max(0.0, polygon_area(detected) - polygon_area(clip(detected, truth)))


def corner_error(detected, truth):
    return sum(
        ((left[0] - right[0]) ** 2 + (left[1] - right[1]) ** 2) ** 0.5
        for left, right in zip(detected, truth)
    ) / 4


def mean(values):
    values = list(values)
    return sum(values) / len(values) if values else 0.0


def percentile(values, fraction):
    values = sorted(values)
    if not values:
        return 0.0
    return values[round((len(values) - 1) * fraction)]


def jitter(points):
    if len(points) < 2:
        return 0.0
    variance = 0.0
    for corner in range(4):
        for axis in range(2):
            values = [point[corner][axis] for point in points]
            average = sum(values) / len(values)
            variance += sum((value - average) ** 2 for value in values) / len(values)
    return (variance / 8.0) ** 0.5


def build(root: Path, truth: dict, legacy: dict) -> dict:
    legacy_by_id = {frame["id"]: frame for frame in legacy["frames"]}
    frames = []
    for frame in truth["frames"]:
        source = legacy_by_id[frame["id"]]
        variants = []
        for variant in source["variants"]:
            detected = variant["detected"]
            variants.append(
                {
                    "size": variant["size"],
                    "detected": detected,
                    "corner_error": corner_error(detected, frame["points"]),
                    "leak_area": leakage_area(detected, frame["points"]),
                    "status": variant["status"],
                    "confidence": variant["confidence"],
                }
            )
        primary = next(variant for variant in variants if variant["size"] == 1024)
        size_max = max(corner_error(variant["detected"], primary["detected"]) for variant in variants)
        size_mean = sum(
            corner_error(variant["detected"], primary["detected"]) for variant in variants
        ) / len(variants)
        frames.append(
            {
                "id": frame["id"],
                "roll_id": frame["roll_id"],
                "eval_group": frame["eval_group"],
                "roll_format": frame["roll_format"],
                "category": frame["category"],
                "source_path": frame["source_path"],
                "image": frame["image"],
                "truth": frame["points"],
                "detected": primary["detected"],
                "corner_error": primary["corner_error"],
                "leak_area": primary["leak_area"],
                "size_max_error": size_max,
                "size_mean_error": size_mean,
                "status": primary["status"],
                "confidence": primary["confidence"],
                "variants": variants,
            }
        )

    groups = []
    grouped = defaultdict(list)
    for frame in frames:
        grouped[frame["eval_group"]].append(frame)
    for group_name, group_frames in sorted(grouped.items()):
        groups.append(
            {
                "eval_group": group_name,
                "category": group_frames[0]["category"],
                "roll_format": group_frames[0]["roll_format"],
                "frame_count": len(group_frames),
                "corner_error_mean": mean(frame["corner_error"] for frame in group_frames),
                "corner_error_p95": percentile(
                    [frame["corner_error"] for frame in group_frames], 0.95
                ),
                "leak_area_total": sum(frame["leak_area"] for frame in group_frames),
                "leaking_frames": sum(frame["leak_area"] > 1.0e-6 for frame in group_frames),
                "roll_jitter": jitter([frame["detected"] for frame in group_frames]),
                "size_max_delta": max(
                    frame["size_max_error"] for frame in group_frames
                ),
            }
        )

    summary = {
        "label": "before-corrected",
        "source_database": truth["source_database"],
        "frame_count": len(frames),
        "corner_error_mean": mean(frame["corner_error"] for frame in frames),
        "corner_error_p95": percentile([frame["corner_error"] for frame in frames], 0.95),
        "corner_error_max": max(frame["corner_error"] for frame in frames),
        "leak_area_total": sum(frame["leak_area"] for frame in frames),
        "leak_area_mean": mean(frame["leak_area"] for frame in frames),
        "leaking_frames": sum(frame["leak_area"] > 1.0e-6 for frame in frames),
        "leak_free": not any(frame["leak_area"] > 1.0e-6 for frame in frames),
        "size_max_delta": max(frame["size_max_error"] for frame in frames),
        "size_mean_delta": mean(frame["size_mean_error"] for frame in frames),
        "size_frames_under_0_5_percent": sum(
            frame["size_max_error"] < 0.005 for frame in frames
        ),
    }
    summary["size_frames_under_0_5_percent_ratio"] = (
        summary["size_frames_under_0_5_percent"] / len(frames)
    )
    return {
        "summary": summary,
        "size_summaries": [],
        "groups": groups,
        "worst_frames": [],
        "frames": frames,
    }


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--root", default="target/film-area-eval")
    args = parser.parse_args()
    root = Path(args.root)
    result = build(root, load(root / "truth.json"), load(root / "legacy-detections.json"))
    (root / "results-before.json").write_text(
        json.dumps(result, ensure_ascii=False, indent=2), encoding="utf-8"
    )
    print(json.dumps(result["summary"], ensure_ascii=False, indent=2))


if __name__ == "__main__":
    main()
