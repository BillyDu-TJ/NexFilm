use std::env;
use std::fs::File;
use std::io::{BufWriter, Write};
use std::process;
use std::time::Instant;

use image::GenericImageView;
use serde::{Deserialize, Serialize};

use film_gate_eval::{
    detect_film_border, film_border_candidates, film_border_ranked_quads, DetectionConfidence,
};

#[derive(Debug, Deserialize)]
struct Manifest {
    items: Vec<ManifestItem>,
}

#[derive(Debug, Deserialize)]
struct ManifestItem {
    id: String,
    image: String,
    #[serde(default)]
    label: Option<Vec<[f32; 2]>>,
}

#[derive(Debug, Serialize)]
struct Record {
    id: String,
    status: String,
    confidence: Option<&'static str>,
    points: Option<[[f32; 2]; 4]>,
    label: Option<Vec<[f32; 2]>>,
    width: Option<u32>,
    height: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
    /// With `--candidates`: the format-prior box, its corner probabilities and
    /// the gradient search result.
    #[serde(skip_serializing_if = "Option::is_none")]
    gate: Option<[[f32; 2]; 4]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    gate_corners: Option<[f32; 4]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    gradient: Option<[[f32; 2]; 4]>,
    /// With `--candidates`: every ranked box, best first.
    #[serde(skip_serializing_if = "Option::is_none")]
    ranked: Option<Vec<[[f32; 2]; 4]>>,
}

fn usage() -> String {
    "usage: film_gate_eval --manifest <path.json> --out <path.jsonl> [--limit N] [--bench] [--candidates]"
        .to_string()
}

fn parse_args() -> Result<(String, String, Option<usize>, bool, bool), String> {
    let args: Vec<String> = env::args().skip(1).collect();
    let mut manifest = None;
    let mut out = None;
    let mut limit = None;
    let mut bench = false;
    let mut candidates = false;

    let mut index = 0;
    while index < args.len() {
        let arg = args[index].as_str();
        let mut take_value = |name: &str| -> Result<String, String> {
            index += 1;
            args.get(index)
                .cloned()
                .ok_or_else(|| format!("missing value for {name}\n{}", usage()))
        };
        match arg {
            "--manifest" => manifest = Some(take_value("--manifest")?),
            "--out" => out = Some(take_value("--out")?),
            "--limit" => {
                let value = take_value("--limit")?;
                let parsed = value
                    .parse::<usize>()
                    .map_err(|_| format!("invalid --limit value: {value}"))?;
                limit = Some(parsed);
            }
            "--bench" => bench = true,
            "--candidates" => candidates = true,
            "-h" | "--help" => {
                println!("{}", usage());
                process::exit(0);
            }
            other => return Err(format!("unknown argument: {other}\n{}", usage())),
        }
        index += 1;
    }

    let manifest = manifest.ok_or_else(|| format!("--manifest is required\n{}", usage()))?;
    let out = out.ok_or_else(|| format!("--out is required\n{}", usage()))?;
    Ok((manifest, out, limit, bench, candidates))
}

fn main() {
    let (manifest_path, out_path, limit, bench, candidates) = match parse_args() {
        Ok(value) => value,
        Err(message) => {
            eprintln!("{message}");
            process::exit(2);
        }
    };

    let manifest_text = match std::fs::read_to_string(&manifest_path) {
        Ok(text) => text,
        Err(error) => {
            eprintln!("failed to read manifest {manifest_path}: {error}");
            process::exit(1);
        }
    };
    let manifest: Manifest = match serde_json::from_str(&manifest_text) {
        Ok(manifest) => manifest,
        Err(error) => {
            eprintln!("failed to parse manifest {manifest_path}: {error}");
            process::exit(1);
        }
    };

    let out_file = match File::create(&out_path) {
        Ok(file) => file,
        Err(error) => {
            eprintln!("failed to create output {out_path}: {error}");
            process::exit(1);
        }
    };
    let mut writer = BufWriter::new(out_file);

    let mut processed = 0usize;
    let mut high = 0usize;
    let mut failed = 0usize;
    let mut durations: Vec<f64> = Vec::new();

    for item in manifest.items.iter() {
        if let Some(limit) = limit {
            if processed >= limit {
                break;
            }
        }

        let record = match image::open(&item.image) {
            Ok(source) => {
                let (width, height) = source.dimensions();
                let started = Instant::now();
                let detection = detect_film_border(&source);
                if bench {
                    durations.push(started.elapsed().as_secs_f64() * 1000.0);
                }
                let confidence = match detection.confidence {
                    DetectionConfidence::High => {
                        high += 1;
                        "high"
                    }
                    DetectionConfidence::Low => "low",
                };
                let (gate, gate_corners, gradient) = if candidates {
                    let (gate, gradient) = film_border_candidates(&source);
                    (
                        gate.map(|gate| gate.0),
                        gate.map(|gate| gate.1),
                        Some(gradient.points),
                    )
                } else {
                    (None, None, None)
                };
                Record {
                    id: item.id.clone(),
                    status: detection.status.to_string(),
                    confidence: Some(confidence),
                    points: Some(detection.points),
                    label: item.label.clone(),
                    width: Some(width),
                    height: Some(height),
                    error: None,
                    gate,
                    gate_corners,
                    gradient,
                    ranked: candidates.then(|| film_border_ranked_quads(&source)),
                }
            }
            Err(error) => {
                failed += 1;
                Record {
                    id: item.id.clone(),
                    status: "error".to_string(),
                    confidence: None,
                    points: None,
                    label: item.label.clone(),
                    width: None,
                    height: None,
                    error: Some(format!("{error}")),
                    gate: None,
                    gate_corners: None,
                    gradient: None,
                    ranked: None,
                }
            }
        };

        match serde_json::to_string(&record) {
            Ok(line) => {
                if writeln!(writer, "{line}").is_err() {
                    eprintln!("failed to write to {out_path}");
                    process::exit(1);
                }
            }
            Err(error) => {
                eprintln!("failed to serialize record {}: {error}", item.id);
                process::exit(1);
            }
        }
        processed += 1;
    }

    if writer.flush().is_err() {
        eprintln!("failed to flush {out_path}");
        process::exit(1);
    }

    eprintln!(
        "film_gate_eval: processed {processed} items, {high} high confidence, {failed} failed -> {out_path}"
    );

    if bench && !durations.is_empty() {
        let mut sorted = durations.clone();
        sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let pick = |q: f64| sorted[((sorted.len() as f64 - 1.0) * q).round() as usize];
        let total: f64 = sorted.iter().sum();
        eprintln!(
            "detect_film_border: median {:.1} ms, p95 {:.1} ms, max {:.1} ms, mean {:.1} ms",
            pick(0.5),
            pick(0.95),
            sorted[sorted.len() - 1],
            total / sorted.len() as f64
        );
    }
}
