# tools

Developer tooling for the film-area detector (`src/film_border.rs`). It is not
part of the app build and adds nothing to the app's dependencies.

| path | what it is |
| --- | --- |
| `film_gate_eval/` | standalone Cargo crate that runs the real detector over a labelled dataset, without the Tauri app crate |
| `film_gate_model/` | training and export of the small networks embedded in the detector |

```bash
cd tools/film_gate_eval
cargo test
cargo run --release --bin film_gate_eval -- --manifest <manifest.json> --out <results.jsonl> [--bench]
```
