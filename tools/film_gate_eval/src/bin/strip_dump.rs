//! Dump the strip profiles of each 135 image, for training the side networks:
//! `strip_dump <manifest.json> <out.jsonl>`. Each line holds the along-strip
//! profile with the shipped network's logits on it, for the equivalence check.

use std::io::Write;

use film_gate_eval::film_border::{film_border_strip_view, strip_side_logits_along};
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
        eprintln!("usage: strip_dump <manifest.json> <out.jsonl>");
        std::process::exit(2);
    }
    let manifest: Manifest =
        serde_json::from_str(&std::fs::read_to_string(&args[1]).expect("manifest")).expect("json");
    let mut out = std::io::BufWriter::new(std::fs::File::create(&args[2]).expect("out"));
    for item in manifest.items {
        let Ok(image) = image::open(&item.image) else {
            continue;
        };
        let Some(view) = film_border_strip_view(&image) else {
            writeln!(out, "{}", serde_json::json!({"id": item.id, "strip": false})).expect("write");
            continue;
        };
        let along_profile = view.along_profile();
        let line = serde_json::json!({
            "id": item.id, "strip": true, "transposed": view.transposed, "along": view.along,
            "across": view.across, "near": view.near, "far": view.far,
            "along_profile": along_profile, "along_logits": strip_side_logits_along(&along_profile),
        });
        writeln!(out, "{line}").expect("write");
    }
}
