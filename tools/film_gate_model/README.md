# film_gate_model

Training for the two small networks in the film-area detector. No images,
labels or checkpoints live in this repository: you bring your own labelled
captures.

* **Corner verifier** (4,209 parameters): looks at the four corners of a ranked
  box. Only the two best ranked boxes can pass, and a box that passes is High.
  Each patch is white-balanced on the colour outside the box, so the tint of the
  film base does not matter. Training adds thin-slab copies of each frame, which
  teach it faint true corners and boxes with one side on a line in the picture.
* **Box scorer** (ensemble of 3, 20,307 parameters): sees the whole frame at 1/16
  scale with a box marked on it. When no box passes the verifier it picks the box
  returned with Low confidence. It never changes a High result.

Both forward passes are hand-written Rust in `src/film_border.rs`. `export.py`
writes the weights to `src/film_border_verifier_weights.rs`.

## Data layout

`--work <dir>` must sit directly inside the data root:

```text
<root>/manifest.json            [{"roll": ..., "preview": "previews/<roll>/<stem>.jpg"}, ...]
<root>/<work>/labels/FROZEN.json {"<roll>/<stem>": {"roll": ..., "quad": [[x, y] x 4], "provenance": ...}}
```

Quads are TL, TR, BR, BL in pixels of a 640x424 preview. A `provenance` of
`excluded` skips the frame. Models are written to `<work>/models`, one per
held-out roll plus `ALL`, which is the one to ship.

## Reproduce

```bash
W=<root>/<work>
cd tools/film_gate_eval && cargo build --release
./target/release/film_gate_eval --manifest manifest_for_film_gate_eval.json --out $W/runs/pool.jsonl --candidates
./target/release/box_pool manifest_for_film_gate_eval.json $W/runs/box_pool.jsonl 24
cd ../film_gate_model
python train.py --work $W verifier --pool $W/runs/pool.jsonl --norm base --slabs 2
python train.py --work $W box_scorer --pool $W/runs/box_pool.jsonl
python export.py $W/models/verifier_ALL.pt ../../src/film_border_verifier_weights.rs
```

`export.py` reads `box_scorer_<fold>.pt` from next to `verifier_<fold>.pt`.
Python needs torch, numpy and Pillow.
