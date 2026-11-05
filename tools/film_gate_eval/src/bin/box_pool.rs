//! Dump the ranked format boxes of each manifest image, for training and
//! analysing the box scorer, with the scorer's preference for the first 12 boxes: `box_pool <manifest.json> <out.jsonl> [count]`.

use std::io::Write;

use film_gate_eval::film_border::film_border_box_pool;
use serde::Deserialize;

#[derive(Deserialize)]
struct Manifest {
    items: Vec<Item>,
}

#[derive(Deserialize)]
struct Item {
    id: String,
    image: String,
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 3 {
        eprintln!("usage: box_pool <manifest.json> <out.jsonl> [count]");
        std::process::exit(2);
    }
    let count = args.get(3).map_or(128, |value| value.parse().expect("count"));
    let manifest: Manifest =
        serde_json::from_str(&std::fs::read_to_string(&args[1]).expect("manifest")).expect("json");
    let mut out = std::io::BufWriter::new(std::fs::File::create(&args[2]).expect("out"));
    for item in manifest.items {
        let Ok(image) = image::open(&item.image) else {
            continue;
        };
        let (boxes, preference) = film_border_box_pool(&image, count);
        let boxes: Vec<_> = boxes
            .into_iter()
            .map(|(coarse, score, quad)| serde_json::json!({"box": coarse, "score": score, "quad": quad}))
            .collect();
        let line = serde_json::json!({"id": item.id, "boxes": boxes, "preference": preference});
        writeln!(out, "{line}").expect("write");
    }
}
