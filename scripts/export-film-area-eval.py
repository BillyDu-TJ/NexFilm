#!/usr/bin/env python3
"""Export confirmed film areas from a NexFilm database for evaluation.

The database and its images are user data and are never committed. This script
writes a self-contained, ignored evaluation set under target/film-area-eval so
the Rust harness can run without opening the production database.
"""

from __future__ import annotations

import argparse
import base64
import json
import os
import sqlite3
from pathlib import Path


SCANNER_MARKERS = (
    "扫描",
    "爱普森",
    "诺日士",
    "精益",
    "哈苏fff",
    "flextight",
    "scanner",
    "v700",
)


def jpeg_size(payload: bytes) -> tuple[int, int] | None:
    if payload[:2] != b"\xff\xd8":
        return None
    offset = 2
    while offset + 9 < len(payload):
        if payload[offset] != 0xFF:
            offset += 1
            continue
        marker = payload[offset + 1]
        offset += 2
        if marker in (0xD8, 0xD9):
            continue
        if offset + 2 > len(payload):
            break
        length = int.from_bytes(payload[offset : offset + 2], "big")
        if length < 2 or offset + length > len(payload):
            break
        if marker in {
            0xC0,
            0xC1,
            0xC2,
            0xC3,
            0xC5,
            0xC6,
            0xC7,
            0xC9,
            0xCA,
            0xCB,
            0xCD,
            0xCE,
            0xCF,
        }:
            height = int.from_bytes(payload[offset + 3 : offset + 5], "big")
            width = int.from_bytes(payload[offset + 5 : offset + 7], "big")
            return width, height
        offset += length
    return None


def decode_thumbnail(value: str | None) -> bytes | None:
    if not value:
        return None
    payload = value.split(",", 1)[1] if value.startswith("data:") else value
    try:
        return base64.b64decode(payload, validate=False)
    except ValueError:
        return None


def classify_source(source_path: str, roll_format: str) -> str:
    normalized = source_path.casefold().replace("\\", "/")
    extension = Path(normalized).suffix
    if extension == ".fff" or any(marker in normalized for marker in SCANNER_MARKERS):
        return "scanner"
    if roll_format.casefold().startswith("120"):
        return "camera_120"
    if roll_format.casefold().startswith("135") or roll_format.casefold() == "35mm":
        return "camera_135"
    return "other"


def evaluation_group(roll_id: str, roll_format: str, category: str, source_path: str) -> str:
    if roll_format.casefold() != "loose":
        return roll_id
    parent = Path(source_path).parent.name or "root"
    return f"loose:{category}:{parent}"


def pick_image(
    embedded: bytes | None, thumbnail: bytes | None
) -> tuple[bytes, tuple[int, int], str, bool]:
    embedded_size = jpeg_size(embedded) if embedded else None
    thumbnail_size = jpeg_size(thumbnail) if thumbnail else None
    # The stored thumbnail is the cropped render. Film-area truth lives in the
    # oriented full frame, so always start from the raw embedded preview and let
    # the harness apply GeometryState without crop.
    if embedded and embedded_size:
        return embedded, embedded_size, "embedded", True
    if thumbnail and thumbnail_size:
        return thumbnail, thumbnail_size, "thumbnail", False
    raise ValueError("frame has no decodable thumbnail")


def export(database: Path, output: Path) -> None:
    connection = sqlite3.connect(database)
    connection.row_factory = sqlite3.Row
    rows = connection.execute(
        """
        SELECT ge.roll_id, ge.file_path, ge.geom,
               ge.thumbnail_base64, ge.embedded_thumb_base64,
               r.roll_format, r.camera
        FROM image_states AS ge
        JOIN rolls AS r USING (roll_id)
        WHERE json_extract(ge.geom, '$.calibration_confirmed') = 1
          AND json_extract(ge.geom, '$.calibration_points') IS NOT NULL
        ORDER BY r.sort_order, ge.roll_id, ge.file_path
        """
    ).fetchall()

    images = output / "images"
    images.mkdir(parents=True, exist_ok=True)
    frames = []
    for index, row in enumerate(rows):
        geometry = json.loads(row["geom"])
        points = geometry.get("calibration_points")
        if not isinstance(points, list) or len(points) != 4:
            continue
        embedded = decode_thumbnail(row["embedded_thumb_base64"])
        thumbnail = decode_thumbnail(row["thumbnail_base64"])
        payload, size, source_kind, orient_with_geom = pick_image(embedded, thumbnail)
        filename = f"frame_{index:04d}.jpg"
        (images / filename).write_bytes(payload)

        roll_format = row["roll_format"] or ""
        category = classify_source(row["file_path"], roll_format)
        frames.append(
            {
                "id": f"{row['roll_id']}::{row['file_path']}",
                "roll_id": row["roll_id"],
                "roll_format": roll_format,
                "camera": row["camera"] or "",
                "category": category,
                "eval_group": evaluation_group(
                    row["roll_id"], roll_format, category, row["file_path"]
                ),
                "source_path": row["file_path"],
                "image": f"images/{filename}",
                "image_source": source_kind,
                "orient_with_geom": orient_with_geom,
                "image_size": list(size),
                "geom": geometry,
                "points": [[float(point[0]), float(point[1])] for point in points],
            }
        )

    manifest = {
        "schema_version": 1,
        "source_database": str(database.resolve()),
        "selection": "calibration_confirmed=true and calibration_points is not null",
        "frames": frames,
    }
    (output / "truth.json").write_text(
        json.dumps(manifest, ensure_ascii=False, indent=2), encoding="utf-8"
    )
    print(f"Exported {len(frames)} confirmed film areas to {output}")


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--database", default="nexfilm_user.db")
    parser.add_argument("--output", default=os.path.join("target", "film-area-eval"))
    args = parser.parse_args()
    export(Path(args.database), Path(args.output))


if __name__ == "__main__":
    main()
