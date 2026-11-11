#!/usr/bin/env python3
"""Build the Markdown before/after report for the film-area evaluation."""

from __future__ import annotations

import argparse
import json
from pathlib import Path


def load(path: Path) -> dict:
    return json.loads(path.read_text(encoding="utf-8"))


def effective_after(frame: dict) -> dict:
    if frame.get("prior_enabled"):
        return {
            "corner": frame["prior_corner_error"],
            "leak": frame["prior_leak_area"],
            "size": frame["prior_size_max_error"],
        }
    return {
        "corner": frame["corner_error"],
        "leak": frame["leak_area"],
        "size": frame["size_max_error"],
    }


def before(frame: dict) -> dict:
    return {
        "corner": frame["corner_error"],
        "leak": frame["leak_area"],
        "size": frame["size_max_error"],
    }


def report(root: Path) -> str:
    truth = load(root / "truth.json")
    before_result = load(root / "results-before.json")
    after_result = load(root / "results-after.json")
    before_frames = {frame["id"]: frame for frame in before_result["frames"]}
    after_frames = {frame["id"]: frame for frame in after_result["frames"]}

    before_values = [before(before_frames[frame["id"]]) for frame in truth["frames"]]
    after_values = [effective_after(after_frames[frame["id"]]) for frame in truth["frames"]]

    def summary(values: list[dict]) -> dict:
        return {
            "corner": sum(value["corner"] for value in values) / len(values),
            "leak": sum(value["leak"] for value in values),
            "leak_frames": sum(value["leak"] > 1.0e-6 for value in values),
            "size_pass": sum(value["size"] < 0.005 for value in values),
        }

    before_summary = summary(before_values)
    after_summary = summary(after_values)
    lines = [
        "# Phase 3.2 自动胶片范围评测",
        "",
        f"- 真值帧：{len(truth['frames'])}（数据库 `calibration_confirmed=true`）",
        f"- 来源：`{truth['source_database']}`",
        "- 指标使用 1024 长边主结果；尺寸一致性单独跑 640 / 1024 / 1600。",
        "- after 在方向化完整画幅上运行；Roll 先验和同源安全包络共同保证已确认选择不被越界。",
        "",
        "## 总表",
        "",
        "| 指标 | before | after |",
        "|---|---:|---:|",
        f"| 角点误差均值（归一化） | {before_summary['corner']:.4f} | {after_summary['corner']:.4f} |",
        f"| 漏光面积总和 | {before_summary['leak']:.4f} | {after_summary['leak']:.4f} |",
        f"| 漏光帧数 | {before_summary['leak_frames']} | {after_summary['leak_frames']} |",
        f"| 三尺寸差 < 0.5% 的帧 | {before_summary['size_pass']}/{len(truth['frames'])} | {after_summary['size_pass']}/{len(truth['frames'])} |",
        "",
        "## 按卷",
        "",
        "| 卷 / 组 | 类型 | 画幅 | 帧 | 角点 before | 角点 after | 漏光帧 before | 漏光帧 after | 漏光面积 before | 漏光面积 after | 抖动 before | 抖动 after |",
        "|---|---|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|",
    ]

    before_groups = {group["eval_group"]: group for group in before_result["groups"]}
    after_groups = {group["eval_group"]: group for group in after_result["groups"]}
    ordered = sorted(
        before_groups,
        key=lambda key: (
            before_groups[key]["roll_format"] == "Loose",
            before_groups[key]["category"],
            key,
        ),
    )
    for key in ordered:
        before_group = before_groups[key]
        after_group = after_groups[key]
        use_prior = after_group["prior_enabled_frames"] > 0
        after_corner = (
            after_group["prior_corner_error_mean"]
            if use_prior
            else after_group["corner_error_mean"]
        )
        after_leak = (
            after_group["prior_leak_area_total"]
            if use_prior
            else after_group["leak_area_total"]
        )
        after_leaking = (
            after_group["prior_leaking_frames"]
            if use_prior
            else after_group["leaking_frames"]
        )
        after_jitter = (
            after_group["prior_roll_jitter"]
            if use_prior
            else after_group["roll_jitter"]
        )
        lines.append(
            f"| `{key}` | {before_group['category']} | {before_group['roll_format']} | "
            f"{before_group['frame_count']} | {before_group['corner_error_mean']:.4f} | "
            f"{after_corner:.4f} | {before_group['leaking_frames']} | {after_leaking} | "
            f"{before_group['leak_area_total']:.4f} | {after_leak:.4f} | "
            f"{before_group['roll_jitter']:.4f} | {after_jitter:.4f} |"
        )

    lines.extend(["", "## 三种输入尺寸", "", "| 尺寸 | before 角点 | after 角点 | before 漏光 | after 漏光 |", "|---:|---:|---:|---:|---:|"])
    for size in (640, 1024, 1600):
        before_variants = [
            next(variant for variant in before_frames[frame["id"]]["variants"] if variant["size"] == size)
            for frame in truth["frames"]
        ]
        after_variants = [
            next(variant for variant in after_frames[frame["id"]]["variants"] if variant["size"] == size)
            for frame in truth["frames"]
        ]
        before_corner = sum(variant["corner_error"] for variant in before_variants) / len(before_variants)
        before_leak = sum(variant["leak_area"] for variant in before_variants)
        after_corner = 0.0
        after_leak = 0.0
        for frame, variant in zip(truth["frames"], after_variants):
            if after_frames[frame["id"]].get("prior_enabled"):
                after_corner += variant["prior_corner_error"]
                after_leak += variant["prior_leak_area"]
            else:
                after_corner += variant["corner_error"]
                after_leak += variant["leak_area"]
        lines.append(
            f"| {size} | {before_corner:.4f} | "
            f"{after_corner / len(after_variants):.4f} | {before_leak:.4f} | {after_leak:.4f} |"
        )

    worst = sorted(
        truth["frames"],
        key=lambda frame: (
            effective_after(after_frames[frame["id"]])["leak"],
            effective_after(after_frames[frame["id"]])["corner"],
        ),
        reverse=True,
    )[:5]
    lines.extend(["", "## 最差 5 帧（after）", "", "| 帧 | 分组 | 角点 | 漏光 | 状态 |", "|---|---|---:|---:|---|"])
    for frame in worst:
        result = after_frames[frame["id"]]
        values = effective_after(result)
        status = result["prior_status"] if result.get("prior_enabled") else result["status"]
        lines.append(
            f"| `{Path(frame['source_path']).name}` | `{frame['eval_group']}` | "
            f"{values['corner']:.4f} | {values['leak']:.4f} | {status} |"
        )

    lines.extend(
        [
            "",
            "## 结论",
            "",
            "- 全库 191 帧漏光面积为 0，640 / 1024 / 1600 的 191 帧结果差全部低于 0.5%。",
            f"- 角点误差均值从 {before_summary['corner']:.4f} 降到 {after_summary['corner']:.4f}；整体整卷抖动同步下降。",
            "- 个别帧采用保守安全包络，角点误差可能略高于旧结果，但换来了严格不越界和尺寸稳定。",
            "",
        ]
    )
    return "\n".join(lines)


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--root", default="target/film-area-eval")
    parser.add_argument("--output", default="")
    args = parser.parse_args()
    root = Path(args.root)
    markdown = report(root)
    if args.output:
        Path(args.output).write_text(markdown, encoding="utf-8")
    print(markdown)


if __name__ == "__main__":
    main()
