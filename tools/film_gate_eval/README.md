# film_gate_eval

Standalone evaluation harness for the production film-gate detector.

The crate compiles the app's real detector module (`src/film_border.rs`) via a
`#[path]` include, so the evaluated behaviour is exactly the shipped code. It
is a separate Cargo workspace (empty `[workspace]` table) and therefore does
not depend on the Tauri app crate building — no webkit/gtk libraries needed.

It runs the detector over a dataset and writes one JSON object per image. The
metrics (IoU / corner error against the labels) are computed separately.

## Commands

```bash
cd tools/film_gate_eval

# unit tests (includes the tests embedded in src/film_border.rs)
cargo test

# optimized build
cargo build --release

# run over a manifest
cargo run --release --bin film_gate_eval -- \
    --manifest /path/to/manifest.json \
    --out /path/to/results.jsonl \
    [--limit 100] [--bench]
```

`--limit N` processes only the first `N` items. A one-line summary (items
processed, how many were high confidence) is printed to stderr.

`--bench` additionally times `detect_film_border` per image (decoding excluded,
the app caches the decoded thumbnail) and prints median / p95 / max / mean in
milliseconds.

## Manifest schema

```json
{
  "items": [
    {
      "id": "roll-a/frame-0001",
      "image": "/abs/path/to/preview.jpg",
      "label": [[0.12, 0.15], [0.88, 0.14], [0.9, 0.86], [0.1, 0.85]]
    }
  ]
}
```

- `id`: identifier copied verbatim into the output.
- `image`: absolute path to the source image.
- `label`: four normalized `[x, y]` points in `[0..1]` in
  `[top-left, top-right, bottom-right, bottom-left]` order, in **source image**
  space. May be `null` (or omitted) for unlabelled images.

## Output schema (JSONL)

One JSON object per line:

```json
{"id":"roll-a/frame-0001","status":"detected_gradient","confidence":"high","points":[[0.20689656,0.04265403],[0.8181818,0.04265403],[0.81191224,0.9526066],[0.20376176,0.9526066]],"label":[[0.12,0.15],[0.88,0.14],[0.9,0.86],[0.1,0.85]],"width":640,"height":424}
```

- `status`: detector status string (`detected_gradient`, `fallback`, …).
- `confidence`: `"high"` or `"low"`.
- `points`: the four normalized points returned by the detector, same
  `[TL, TR, BR, BL]` order as the label.
- `label`: the input label, echoed back.
- `width` / `height`: source image dimensions.

If an image cannot be read, the line is still emitted with `"status":"error"`
and an `"error"` field; the run continues with the next item.

## box_pool

`cargo run --release --bin box_pool -- <manifest.json> <out.jsonl> 24` dumps the
ranked boxes of each frame, which `train.py box_scorer` trains on.
