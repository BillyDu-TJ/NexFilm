use image::{imageops::FilterType, DynamicImage, GenericImageView};
use serde::Deserialize;
use serde_json::json;
use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Deserialize)]
struct Dataset {
    #[serde(default)]
    source_database: String,
    frames: Vec<TruthFrame>,
}

#[derive(Clone, Debug, Deserialize)]
struct TruthFrame {
    id: String,
    roll_id: String,
    roll_format: String,
    category: String,
    eval_group: String,
    source_path: String,
    image: String,
    image_source: String,
    image_size: [u32; 2],
    points: [[f32; 2]; 4],
}

#[derive(Debug)]
struct FrameResult {
    truth: TruthFrame,
    detection: nexfilm_engine::film_border::FilmBorderDetection,
    corner_error: f64,
    leak_area: f64,
    prior_detection: nexfilm_engine::film_border::FilmBorderDetection,
    prior_corner_error: f64,
    prior_leak_area: f64,
    prior_enabled: bool,
    prior_sample_count: usize,
    variants: Vec<VariantResult>,
    size_max_error: f64,
    size_mean_error: f64,
    prior_size_max_error: f64,
}

#[derive(Debug)]
struct VariantResult {
    size: u32,
    detection: nexfilm_engine::film_border::FilmBorderDetection,
    corner_error: f64,
    leak_area: f64,
    prior_detection: nexfilm_engine::film_border::FilmBorderDetection,
    prior_corner_error: f64,
    prior_leak_area: f64,
}

#[test]
#[ignore]
fn film_area_eval() {
    let root = std::env::var_os("NEXFILM_FILM_AREA_EVAL_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("target/film-area-eval"));
    let dataset_path = root.join("truth.json");
    assert!(
        dataset_path.is_file(),
        "missing {}; run scripts/export-film-area-eval.py first",
        dataset_path.display()
    );
    let label =
        std::env::var("NEXFILM_FILM_AREA_EVAL_LABEL").unwrap_or_else(|_| "current".to_string());
    let dataset: Dataset =
        serde_json::from_str(&fs::read_to_string(&dataset_path).unwrap()).unwrap();
    assert!(!dataset.frames.is_empty(), "evaluation set is empty");

    let grouped = dataset.frames.iter().cloned().fold(
        BTreeMap::<String, Vec<TruthFrame>>::new(),
        |mut groups, frame| {
            groups
                .entry(frame.eval_group.clone())
                .or_default()
                .push(frame);
            groups
        },
    );
    let mut results = Vec::with_capacity(dataset.frames.len());
    for truth in &dataset.frames {
        let prior = (!truth.roll_format.eq_ignore_ascii_case("loose"))
            .then(|| {
                let samples = grouped[&truth.eval_group]
                    .iter()
                    .filter(|sample| sample.id != truth.id)
                    .map(|sample| nexfilm_engine::film_border::FilmAreaPriorSample {
                        points: sample.points,
                        confirmed: true,
                        confidence: nexfilm_engine::film_border::DetectionConfidence::High,
                    })
                    .collect::<Vec<_>>();
                nexfilm_engine::film_border::film_area_prior_from_samples(&samples)
            })
            .flatten();
        let image_path = root.join(&truth.image);
        let original = image::open(&image_path)
            .unwrap_or_else(|error| panic!("cannot open {}: {error}", image_path.display()));
        let mut variants = Vec::new();
        for size in [640, 1024, 1600] {
            let sized = resized_to_longest_edge(&original, size);
            let detection = nexfilm_engine::film_border::detect_film_border_with_format(
                &sized,
                Some(&truth.roll_format),
            );
            let prior_detection = nexfilm_engine::film_border::detect_film_border_with_prior(
                &sized,
                Some(&truth.roll_format),
                prior.as_ref(),
            );
            variants.push(VariantResult {
                size,
                corner_error: corner_error(&detection.points, &truth.points),
                leak_area: leakage_area(&detection.points, &truth.points),
                prior_corner_error: corner_error(&prior_detection.points, &truth.points),
                prior_leak_area: leakage_area(&prior_detection.points, &truth.points),
                prior_detection,
                detection,
            });
        }
        let primary = variants
            .iter()
            .find(|variant| variant.size == 1024)
            .unwrap();
        let detection = primary.detection.clone();
        let primary_corner_error = primary.corner_error;
        let leak_area = primary.leak_area;
        let prior_detection = primary.prior_detection.clone();
        let prior_corner_error = primary.prior_corner_error;
        let prior_leak_area = primary.prior_leak_area;
        let mut size_errors = Vec::new();
        let mut prior_size_errors = Vec::new();
        for variant in &variants {
            size_errors.push(corner_error(&variant.detection.points, &detection.points));
            prior_size_errors.push(corner_error(
                &variant.prior_detection.points,
                &prior_detection.points,
            ));
        }
        let size_max_error = size_errors.iter().copied().fold(0.0, f64::max);
        let size_mean_error = size_errors.iter().sum::<f64>() / size_errors.len().max(1) as f64;
        let prior_size_max_error = prior_size_errors.iter().copied().fold(0.0, f64::max);

        results.push(FrameResult {
            truth: truth.clone(),
            detection,
            corner_error: primary_corner_error,
            leak_area,
            prior_detection,
            prior_corner_error,
            prior_leak_area,
            prior_enabled: prior.is_some(),
            prior_sample_count: prior.as_ref().map_or(0, |prior| prior.sample_count),
            variants,
            size_max_error,
            size_mean_error,
            prior_size_max_error,
        });
    }

    let group_reports = group_reports(&results);
    let mut worst = results.iter().collect::<Vec<_>>();
    worst.sort_by(|left, right| {
        right
            .prior_leak_area
            .total_cmp(&left.prior_leak_area)
            .then_with(|| right.prior_corner_error.total_cmp(&left.prior_corner_error))
    });
    let worst = worst
        .into_iter()
        .take(5)
        .map(|result| {
            json!({
                "id": result.truth.id,
                "roll_id": result.truth.roll_id,
                "roll_format": result.truth.roll_format,
                "category": result.truth.category,
                "source_path": result.truth.source_path,
                "image": result.truth.image,
                "truth": result.truth.points,
                "detected": result.detection.points,
                "prior_detected": result.prior_detection.points,
                "corner_error": result.corner_error,
                "leak_area": result.leak_area,
                "prior_corner_error": result.prior_corner_error,
                "prior_leak_area": result.prior_leak_area,
                "prior_enabled": result.prior_enabled,
                "prior_sample_count": result.prior_sample_count,
                "confidence": result.detection.confidence,
                "status": result.detection.status,
                "prior_confidence": result.prior_detection.confidence,
                "prior_status": result.prior_detection.status,
            })
        })
        .collect::<Vec<_>>();

    let leaking_frames = results.iter().filter(|r| r.leak_area > 1.0e-6).count();
    let size_passing_frames = results.iter().filter(|r| r.size_max_error < 0.005).count();
    let prior_leaking_frames = results
        .iter()
        .filter(|result| result.prior_leak_area > 1.0e-6)
        .count();
    let prior_size_passing_frames = results
        .iter()
        .filter(|result| result.prior_size_max_error < 0.005)
        .count();
    let summary = json!({
        "label": label,
        "source_database": dataset.source_database,
        "frame_count": results.len(),
        "corner_error_mean": mean(results.iter().map(|r| r.corner_error)),
        "corner_error_p95": percentile(results.iter().map(|r| r.corner_error), 0.95),
        "corner_error_max": results.iter().map(|r| r.corner_error).fold(0.0, f64::max),
        "leak_area_total": results.iter().map(|r| r.leak_area).sum::<f64>(),
        "leak_area_mean": mean(results.iter().map(|r| r.leak_area)),
        "leaking_frames": leaking_frames,
        "leak_free": leaking_frames == 0,
        "size_max_delta": results.iter().map(|r| r.size_max_error).fold(0.0, f64::max),
        "size_mean_delta": mean(results.iter().map(|r| r.size_mean_error)),
        "size_frames_under_0_5_percent": size_passing_frames,
        "size_frames_under_0_5_percent_ratio":
            size_passing_frames as f64 / results.len().max(1) as f64,
        "prior_enabled_frames": results.iter().filter(|result| result.prior_enabled).count(),
        "prior_corner_error_mean": mean(results.iter().map(|result| result.prior_corner_error)),
        "prior_corner_error_p95": percentile(
            results.iter().map(|result| result.prior_corner_error),
            0.95,
        ),
        "prior_corner_error_max": results
            .iter()
            .map(|result| result.prior_corner_error)
            .fold(0.0, f64::max),
        "prior_leak_area_total": results.iter().map(|result| result.prior_leak_area).sum::<f64>(),
        "prior_leak_area_mean": mean(results.iter().map(|result| result.prior_leak_area)),
        "prior_leaking_frames": prior_leaking_frames,
        "prior_leak_free": prior_leaking_frames == 0,
        "prior_size_max_delta": results
            .iter()
            .map(|result| result.prior_size_max_error)
            .fold(0.0, f64::max),
        "prior_size_frames_under_0_5_percent": prior_size_passing_frames,
        "prior_size_frames_under_0_5_percent_ratio":
            prior_size_passing_frames as f64 / results.len().max(1) as f64,
    });
    let size_summaries = [640, 1024, 1600]
        .into_iter()
        .map(|size| {
            let variants = results
                .iter()
                .filter_map(|result| result.variants.iter().find(|variant| variant.size == size))
                .collect::<Vec<_>>();
            json!({
                "size": size,
                "corner_error_mean": mean(variants.iter().map(|variant| variant.corner_error)),
                "corner_error_p95": percentile(
                    variants.iter().map(|variant| variant.corner_error),
                    0.95,
                ),
                "leak_area_total": variants.iter().map(|variant| variant.leak_area).sum::<f64>(),
                "leaking_frames": variants.iter().filter(|variant| variant.leak_area > 1.0e-6).count(),
                "prior_corner_error_mean": mean(
                    variants.iter().map(|variant| variant.prior_corner_error),
                ),
                "prior_leak_area_total": variants.iter().map(|variant| variant.prior_leak_area).sum::<f64>(),
                "prior_leaking_frames": variants
                    .iter()
                    .filter(|variant| variant.prior_leak_area > 1.0e-6)
                    .count(),
            })
        })
        .collect::<Vec<_>>();

    let frame_reports = results
        .iter()
        .map(|result| {
            json!({
                "id": result.truth.id,
                "roll_id": result.truth.roll_id,
                "eval_group": result.truth.eval_group,
                "roll_format": result.truth.roll_format,
                "category": result.truth.category,
                "source_path": result.truth.source_path,
                "image": result.truth.image,
                "image_source": result.truth.image_source,
                "image_size": result.truth.image_size,
                "truth": result.truth.points,
                "detected": result.detection.points,
                "prior_detected": result.prior_detection.points,
                "corner_error": result.corner_error,
                "leak_area": result.leak_area,
                "prior_corner_error": result.prior_corner_error,
                "prior_leak_area": result.prior_leak_area,
                "prior_enabled": result.prior_enabled,
                "prior_sample_count": result.prior_sample_count,
                "size_max_error": result.size_max_error,
                "size_mean_error": result.size_mean_error,
                "prior_size_max_error": result.prior_size_max_error,
                "confidence": result.detection.confidence,
                "status": result.detection.status,
                "prior_confidence": result.prior_detection.confidence,
                "prior_status": result.prior_detection.status,
                "variants": result.variants.iter().map(|variant| json!({
                    "size": variant.size,
                    "detected": variant.detection.points,
                    "prior_detected": variant.prior_detection.points,
                    "corner_error": variant.corner_error,
                    "leak_area": variant.leak_area,
                    "prior_corner_error": variant.prior_corner_error,
                    "prior_leak_area": variant.prior_leak_area,
                    "confidence": variant.detection.confidence,
                    "status": variant.detection.status,
                    "prior_confidence": variant.prior_detection.confidence,
                    "prior_status": variant.prior_detection.status,
                })).collect::<Vec<_>>(),
            })
        })
        .collect::<Vec<_>>();

    let report = json!({
        "summary": summary,
        "size_summaries": size_summaries,
        "groups": group_reports,
        "worst_frames": worst,
        "frames": frame_reports,
    });
    let output = root.join(format!("results-{label}.json"));
    fs::write(&output, serde_json::to_vec_pretty(&report).unwrap()).unwrap();

    println!(
        "label={label} frames={} cold_corner={:.5} cold_leak={:.6} cold_leaking={} cold_size_pass={}/{}",
        results.len(),
        summary["corner_error_mean"].as_f64().unwrap_or_default(),
        summary["leak_area_total"].as_f64().unwrap_or_default(),
        summary["leaking_frames"].as_u64().unwrap_or_default(),
        summary["size_frames_under_0_5_percent"]
            .as_u64()
            .unwrap_or_default(),
        results.len()
    );
    println!(
        "prior_enabled={} prior_corner={:.5} prior_leak={:.6} prior_leaking={} prior_size_pass={}/{}",
        summary["prior_enabled_frames"].as_u64().unwrap_or_default(),
        summary["prior_corner_error_mean"]
            .as_f64()
            .unwrap_or_default(),
        summary["prior_leak_area_total"]
            .as_f64()
            .unwrap_or_default(),
        summary["prior_leaking_frames"]
            .as_u64()
            .unwrap_or_default(),
        summary["prior_size_frames_under_0_5_percent"]
            .as_u64()
            .unwrap_or_default(),
        results.len()
    );
    println!("wrote {}", output.display());
    for group in &group_reports {
        println!(
            "{} [{} {}] n={} cold(corner={:.5} leak={:.6} jitter={:.5}) prior(enabled={} corner={:.5} leak={:.6} jitter={:.5})",
            group["eval_group"].as_str().unwrap_or_default(),
            group["category"].as_str().unwrap_or_default(),
            group["roll_format"].as_str().unwrap_or_default(),
            group["frame_count"].as_u64().unwrap_or_default(),
            group["corner_error_mean"].as_f64().unwrap_or_default(),
            group["leak_area_total"].as_f64().unwrap_or_default(),
            group["roll_jitter"].as_f64().unwrap_or_default(),
            group["prior_enabled_frames"].as_u64().unwrap_or_default(),
            group["prior_corner_error_mean"]
                .as_f64()
                .unwrap_or_default(),
            group["prior_leak_area_total"]
                .as_f64()
                .unwrap_or_default(),
            group["prior_roll_jitter"].as_f64().unwrap_or_default(),
        );
    }
}

fn resized_to_longest_edge(image: &DynamicImage, target: u32) -> DynamicImage {
    let (width, height) = image.dimensions();
    let longest = width.max(height).max(1);
    if longest == target {
        return image.clone();
    }
    let scale = target as f64 / longest as f64;
    image.resize_exact(
        (width as f64 * scale).round().max(1.0) as u32,
        (height as f64 * scale).round().max(1.0) as u32,
        FilterType::Triangle,
    )
}

fn corner_error(left: &[[f32; 2]; 4], right: &[[f32; 2]; 4]) -> f64 {
    left.iter()
        .zip(right)
        .map(|(a, b)| {
            let dx = a[0] as f64 - b[0] as f64;
            let dy = a[1] as f64 - b[1] as f64;
            dx.hypot(dy)
        })
        .sum::<f64>()
        / 4.0
}

fn leakage_area(detected: &[[f32; 2]; 4], truth: &[[f32; 2]; 4]) -> f64 {
    let detected = ordered_polygon(*detected);
    let truth = ordered_polygon(*truth);
    let intersection = clip_polygon(&detected, &truth);
    (polygon_area(&detected) - polygon_area(&intersection)).max(0.0)
}

fn ordered_polygon(mut points: [[f32; 2]; 4]) -> Vec<[f32; 2]> {
    if signed_area(&points) < 0.0 {
        points.reverse();
    }
    points.to_vec()
}

fn polygon_area(points: &[[f32; 2]]) -> f64 {
    if points.len() < 3 {
        return 0.0;
    }
    let mut twice = 0.0;
    for index in 0..points.len() {
        let left = points[index];
        let right = points[(index + 1) % points.len()];
        twice += left[0] as f64 * right[1] as f64 - right[0] as f64 * left[1] as f64;
    }
    twice.abs() * 0.5
}

fn signed_area(points: &[[f32; 2]]) -> f64 {
    points
        .iter()
        .zip(points.iter().cycle().skip(1))
        .take(points.len())
        .map(|(left, right)| left[0] as f64 * right[1] as f64 - right[0] as f64 * left[1] as f64)
        .sum::<f64>()
        * 0.5
}

fn clip_polygon(subject: &[[f32; 2]], clip: &[[f32; 2]]) -> Vec<[f32; 2]> {
    let mut output = subject.to_vec();
    for index in 0..clip.len() {
        let edge_start = clip[index];
        let edge_end = clip[(index + 1) % clip.len()];
        let input = output;
        output = Vec::new();
        if input.is_empty() {
            break;
        }
        let mut previous = *input.last().unwrap();
        let mut previous_inside = inside(previous, edge_start, edge_end);
        for current in input {
            let current_inside = inside(current, edge_start, edge_end);
            if current_inside {
                if !previous_inside {
                    output.push(line_intersection(previous, current, edge_start, edge_end));
                }
                output.push(current);
            } else if previous_inside {
                output.push(line_intersection(previous, current, edge_start, edge_end));
            }
            previous = current;
            previous_inside = current_inside;
        }
    }
    output
}

fn inside(point: [f32; 2], edge_start: [f32; 2], edge_end: [f32; 2]) -> bool {
    let cross = (edge_end[0] - edge_start[0]) as f64 * (point[1] - edge_start[1]) as f64
        - (edge_end[1] - edge_start[1]) as f64 * (point[0] - edge_start[0]) as f64;
    cross >= -1.0e-9
}

fn line_intersection(
    start: [f32; 2],
    end: [f32; 2],
    edge_start: [f32; 2],
    edge_end: [f32; 2],
) -> [f32; 2] {
    let sx = (end[0] - start[0]) as f64;
    let sy = (end[1] - start[1]) as f64;
    let ex = (edge_end[0] - edge_start[0]) as f64;
    let ey = (edge_end[1] - edge_start[1]) as f64;
    let denominator = sx * ey - sy * ex;
    if denominator.abs() < 1.0e-12 {
        return start;
    }
    let dx = (edge_start[0] - start[0]) as f64;
    let dy = (edge_start[1] - start[1]) as f64;
    let t = (dx * ey - dy * ex) / denominator;
    [
        (start[0] as f64 + sx * t) as f32,
        (start[1] as f64 + sy * t) as f32,
    ]
}

fn group_reports(results: &[FrameResult]) -> Vec<serde_json::Value> {
    let mut groups = BTreeMap::<&str, Vec<&FrameResult>>::new();
    for result in results {
        groups
            .entry(result.truth.eval_group.as_str())
            .or_default()
            .push(result);
    }
    groups
        .into_iter()
        .map(|(group, frames)| {
            json!({
                "eval_group": group,
                "category": frames[0].truth.category,
                "roll_format": frames[0].truth.roll_format,
                "frame_count": frames.len(),
                "corner_error_mean": mean(frames.iter().map(|r| r.corner_error)),
                "corner_error_p95": percentile(frames.iter().map(|r| r.corner_error), 0.95),
                "leak_area_total": frames.iter().map(|r| r.leak_area).sum::<f64>(),
                "leaking_frames": frames.iter().filter(|r| r.leak_area > 1.0e-6).count(),
                "roll_jitter": corner_jitter(frames.iter().map(|r| r.detection.points).collect()),
                "size_max_delta": frames.iter().map(|r| r.size_max_error).fold(0.0, f64::max),
                "prior_enabled_frames": frames.iter().filter(|r| r.prior_enabled).count(),
                "prior_corner_error_mean": mean(frames.iter().map(|r| r.prior_corner_error)),
                "prior_corner_error_p95": percentile(
                    frames.iter().map(|r| r.prior_corner_error),
                    0.95,
                ),
                "prior_leak_area_total": frames.iter().map(|r| r.prior_leak_area).sum::<f64>(),
                "prior_leaking_frames": frames
                    .iter()
                    .filter(|r| r.prior_leak_area > 1.0e-6)
                    .count(),
                "prior_roll_jitter": corner_jitter(
                    frames.iter().map(|r| r.prior_detection.points).collect(),
                ),
                "prior_size_max_delta": frames
                    .iter()
                    .map(|r| r.prior_size_max_error)
                    .fold(0.0, f64::max),
            })
        })
        .collect()
}

fn corner_jitter(points: Vec<[[f32; 2]; 4]>) -> f64 {
    if points.len() < 2 {
        return 0.0;
    }
    let mut variance_sum = 0.0;
    for corner in 0..4 {
        for axis in 0..2 {
            let values = points
                .iter()
                .map(|points| points[corner][axis] as f64)
                .collect::<Vec<_>>();
            let average = values.iter().sum::<f64>() / values.len() as f64;
            variance_sum += values
                .iter()
                .map(|value| (value - average).powi(2))
                .sum::<f64>()
                / values.len() as f64;
        }
    }
    (variance_sum / 8.0).sqrt()
}

fn percentile(values: impl Iterator<Item = f64>, percentile: f64) -> f64 {
    let mut values = values.collect::<Vec<_>>();
    if values.is_empty() {
        return 0.0;
    }
    values.sort_by(f64::total_cmp);
    let index = ((values.len() - 1) as f64 * percentile).round() as usize;
    values[index]
}

fn mean(values: impl Iterator<Item = f64>) -> f64 {
    let mut count = 0usize;
    let mut sum = 0.0;
    for value in values {
        count += 1;
        sum += value;
    }
    if count == 0 {
        0.0
    } else {
        sum / count as f64
    }
}
