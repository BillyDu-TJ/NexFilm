use image::{DynamicImage, GenericImageView, GrayImage};
use imageproc::contours::find_contours_with_threshold;
use imageproc::contrast::{otsu_level, threshold};
use imageproc::edges::canny;
use imageproc::filter::gaussian_blur_f32;
use imageproc::geometry::{approximate_polygon_dp, arc_length};
use imageproc::gradients::{horizontal_sobel, vertical_sobel};
use imageproc::point::Point;
use serde::Serialize;

const DEFAULT_INSET: f32 = 0.0;
const MIN_AREA_RATIO: f64 = 0.025;
const MAX_AREA_RATIO: f64 = 0.98;
const MAX_SIDE_RATIO: f64 = 6.0;
const MAX_DIAGONAL_RATIO: f64 = 3.0;
const MIN_GRADIENT_PROMINENCE: f64 = 1.22;
const MAX_ANALYSIS_EDGE: u32 = 640;
const AUTO_DETECTION_INSET: f32 = 0.01;
const STANDARD_FRAME_ASPECTS: [f64; 6] =
    [3.0 / 2.0, 1.0, 4.0 / 3.0, 6.0 / 7.0, 6.0 / 9.0, 65.0 / 24.0];
const ASPECT_SCORE_SIGMA: f64 = 0.20;
const ROLL_PRIOR_WINDOW: f32 = 0.05;
const ROLL_PRIOR_DETECTED_INSET: f32 = 0.02;
const ROLL_PRIOR_INHERITED_INSET: f32 = 0.03;

/// Gate refinement: how far inward (as a fraction of the edge's own span) a
/// rebate walk may travel before it is declared a misdetection.
const REFINE_MAX_INSET_FRACTION: f32 = 0.20;
/// The zone between the film edge and the gate that the walk may cross before
/// the base-like run has to start. Dense edge lines sit directly inside a
/// film-boundary quad and must not stop the walk on their own.
const REFINE_SKIP_LIMIT_FRACTION: f32 = 0.06;
/// A rebate run shorter than this fraction of the span is picture content that
/// happens to be uniform, not film base, so the edge keeps its original line.
const REFINE_MIN_RUN_FRACTION: f32 = 0.008;
/// Median per-channel colour distance (0-255) below which a line still reads
/// as the same base-like material as the boundary band. Rebate-to-picture
/// transitions are gradual, so this sits below the mid-transition distance;
/// the render-side clip tolerates a few pixels of residue.
const REFINE_MATCH_MAX_DISTANCE: f32 = 8.0;
/// Consecutive mismatching samples that end a matching run. Small enough to
/// keep the gate's own frame line inside the area, large enough to survive
/// thin rebate markings such as edge print.
const REFINE_RUN_BREAK_SAMPLES: u32 = 3;
/// Sample positions along the edge are kept away from the corners, where the
/// neighbouring sides' rebate mixes into the measurement.
const REFINE_EDGE_SPAN_TRIM: f32 = 0.08;

#[derive(Clone, Copy)]
struct FittedLine {
    /// Dependent coordinate = slope * independent coordinate + intercept.
    slope: f64,
    intercept: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum DetectionConfidence {
    High,
    Low,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct FilmBorderDetection {
    /// Normalized [top-left, top-right, bottom-right, bottom-left] coordinates.
    pub points: [[f32; 2]; 4],
    pub confidence: DetectionConfidence,
    pub status: &'static str,
}

#[derive(Clone, Debug, PartialEq)]
pub struct FilmAreaPrior {
    /// Median of the samples, normalized like a detected film area.
    pub points: [[f32; 2]; 4],
    pub sample_count: usize,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FilmAreaPriorSample {
    pub points: [[f32; 2]; 4],
    pub confirmed: bool,
    pub confidence: DetectionConfidence,
}

impl FilmBorderDetection {
    pub fn fallback() -> Self {
        Self {
            points: [
                [DEFAULT_INSET, DEFAULT_INSET],
                [1.0 - DEFAULT_INSET, DEFAULT_INSET],
                [1.0 - DEFAULT_INSET, 1.0 - DEFAULT_INSET],
                [DEFAULT_INSET, 1.0 - DEFAULT_INSET],
            ],
            confidence: DetectionConfidence::Low,
            status: "fallback",
        }
    }
}

/// Detect a film gate from a cached thumbnail. This function never opens the
/// source image and therefore cannot trigger a RAW decode.
pub fn detect_film_border(thumbnail: &DynamicImage) -> FilmBorderDetection {
    detect_film_border_with_format(thumbnail, None)
}

/// Detect a film gate with an optional Roll format hint. The format only
/// changes candidate weighting and confidence; an unusual but visually strong
/// candidate is never discarded.
pub fn detect_film_border_with_format(
    thumbnail: &DynamicImage,
    roll_format: Option<&str>,
) -> FilmBorderDetection {
    let (width, height) = thumbnail.dimensions();
    if width < 16 || height < 16 {
        return FilmBorderDetection::fallback();
    }
    let preferred_aspect = roll_format.and_then(expected_frame_aspect);

    let gray = analysis_grayscale(thumbnail);
    let (width, height) = gray.dimensions();
    let blurred = gaussian_blur_f32(&gray, 1.4);

    // Film gates are often not closed contours after thresholding: sprocket
    // holes and image texture break or join their edges. Locate the four long
    // gate edges from robust Sobel projections first, then fit each edge.
    if let Some(points) = detect_from_gradient_projections(&blurred) {
        let aspect_score = candidate_aspect_score(&points, preferred_aspect);
        return detected_result(
            refine_to_gate(thumbnail, points),
            width,
            height,
            "detected_gradient",
            aspect_score,
        );
    }

    let binary = threshold(&blurred, otsu_level(&blurred));
    let otsu_edges = canny(&binary, 25.0, 75.0);
    let grayscale_edges = canny(&blurred, 18.0, 58.0);

    let best = find_best_quad(&grayscale_edges, width, height, preferred_aspect)
        .into_iter()
        .chain(find_best_quad(&otsu_edges, width, height, preferred_aspect))
        .max_by(|left, right| left.1.total_cmp(&right.1));

    let Some((points, _, aspect_score)) = best else {
        return FilmBorderDetection::fallback();
    };

    detected_result(
        refine_to_gate(thumbnail, points),
        width,
        height,
        "detected_contour",
        aspect_score,
    )
}

/// Detect a frame with a Roll-level spatial prior. This is intended for batch
/// and import workers; an interactive one-off detection must keep using
/// `detect_film_border_with_format`.
pub fn detect_film_border_with_prior(
    thumbnail: &DynamicImage,
    roll_format: Option<&str>,
    prior: Option<&FilmAreaPrior>,
) -> FilmBorderDetection {
    let detection = detect_film_border_with_format(thumbnail, roll_format);
    let Some(prior) = prior else {
        return detection;
    };
    apply_roll_prior(detection, prior)
}

/// Build a Roll prior only when its source frames agree. Confirmed frames win;
/// without one, three mutually consistent High detections are required.
pub fn film_area_prior_from_samples(samples: &[FilmAreaPriorSample]) -> Option<FilmAreaPrior> {
    let confirmed = samples
        .iter()
        .filter(|sample| sample.confirmed)
        .copied()
        .collect::<Vec<_>>();
    let highs = samples
        .iter()
        .filter(|sample| !sample.confirmed && sample.confidence == DetectionConfidence::High)
        .copied()
        .collect::<Vec<_>>();
    let source = if !confirmed.is_empty() {
        confirmed
    } else if highs.len() >= 3 {
        highs
    } else {
        return None;
    };

    let points = median_prior_points(&source);
    let consistent = source.iter().all(|sample| {
        sample
            .points
            .iter()
            .flatten()
            .zip(points.iter().flatten())
            .all(|(actual, prior)| (actual - prior).abs() <= ROLL_PRIOR_WINDOW)
    });
    consistent.then_some(FilmAreaPrior {
        points,
        sample_count: source.len(),
    })
}

fn median_prior_points(samples: &[FilmAreaPriorSample]) -> [[f32; 2]; 4] {
    std::array::from_fn(|corner| {
        std::array::from_fn(|axis| {
            let mut values = samples
                .iter()
                .map(|sample| sample.points[corner][axis])
                .collect::<Vec<_>>();
            values.sort_by(f32::total_cmp);
            values[values.len() / 2]
        })
    })
}

fn apply_roll_prior(
    mut detection: FilmBorderDetection,
    prior: &FilmAreaPrior,
) -> FilmBorderDetection {
    let inside_window = detection.confidence == DetectionConfidence::High
        && !detection.status.contains("fallback")
        && detection
            .points
            .iter()
            .flatten()
            .zip(prior.points.iter().flatten())
            .all(|(actual, expected)| (actual - expected).abs() <= ROLL_PRIOR_WINDOW);

    if inside_window {
        for (point, prior_point) in detection.points.iter_mut().zip(prior.points) {
            for axis in 0..2 {
                point[axis] = point[axis].clamp(
                    prior_point[axis] - ROLL_PRIOR_WINDOW,
                    prior_point[axis] + ROLL_PRIOR_WINDOW,
                );
            }
        }
        detection.points = inset_points(detection.points, ROLL_PRIOR_DETECTED_INSET);
        detection.status = "detected_roll_prior";
        return detection;
    }

    let conservative = inset_points(prior.points, ROLL_PRIOR_INHERITED_INSET);
    FilmBorderDetection {
        points: conservative,
        confidence: DetectionConfidence::Low,
        status: "prior_inherited",
    }
}

/// Walk each detected edge inward from the film boundary to the actual frame
/// gate.
///
/// A gradient detector reliably finds the film-body boundary — the border
/// between dense film and an open light panel is the strongest line in any
/// scan — but the gate sits one rebate band further in, and a quad left on the
/// boundary keeps the rebate inside the area. After inversion the rebate
/// renders as dark or white bands around the picture and its densities widen
/// the analysis window until the frame looks flat.
///
/// The rebate is uniform film base, so the walk measures the median colour
/// distance between lines parallel to the edge and the boundary band's own
/// colour, and stops the edge at the first sustained deviation: the gate.
/// Everything is sampled from the colour thumbnail; no RAW decode happens.
fn refine_to_gate(thumbnail: &DynamicImage, points: [Point<i32>; 4]) -> [Point<i32>; 4] {
    let Some(rgb) = working_rgb(thumbnail) else {
        return points;
    };

    let corners: [[f32; 2]; 4] = points.map(|point| [point.x as f32, point.y as f32]);
    // Per-edge inward insets. A genuine rebate band surrounds the picture on
    // every side, so a trustworthy refinement trims at least three edges; one
    // or two trims mean the walk found a uniform picture border and must not
    // be applied.
    let edges: [Option<f32>; 4] = std::array::from_fn(|index| {
        let a = corners[index];
        let b = corners[(index + 1) % 4];
        let opposite = corners[(index + 2) % 4];
        edge_gate_inset(&rgb, a, b, opposite)
    });
    if edges.iter().filter(|inset| inset.is_some()).count() < 3 {
        return points;
    }

    // Rebuild the corners from the refined edge lines: each new corner is the
    // intersection of its two adjacent edges, so tilted quads stay consistent.
    let mut refined_edge_lines: [Option<([f32; 2], [f32; 2])>; 4] = [None; 4];
    for index in 0..4 {
        let Some(inset) = edges[index] else { continue };
        let a = corners[index];
        let b = corners[(index + 1) % 4];
        let opposite = corners[(index + 2) % 4];
        let edge = [b[0] - a[0], b[1] - a[1]];
        let edge_length = (edge[0] * edge[0] + edge[1] * edge[1]).sqrt();
        if edge_length < 8.0 {
            continue;
        }
        let along = [edge[0] / edge_length, edge[1] / edge_length];
        let inward = inward_normal(a, b, opposite);
        refined_edge_lines[index] =
            Some(([a[0] + inward[0] * inset, a[1] + inward[1] * inset], along));
    }
    let mut result = corners;
    for index in 0..4 {
        let previous = refined_edge_lines[(index + 3) % 4];
        let current = refined_edge_lines[index];
        let corner = match (previous, current) {
            (Some((point_a, direction_a)), Some((point_b, direction_b))) => {
                intersect_lines_f32(point_a, direction_a, point_b, direction_b)
                    .unwrap_or(corners[index])
            }
            (Some((point, direction)), None) | (None, Some((point, direction))) => {
                project_onto_line(corners[index], point, direction)
            }
            (None, None) => corners[index],
        };
        result[index] = corner;
    }

    let (width, height) = rgb.dimensions();
    let clamped: [[f32; 2]; 4] = result.map(|[x, y]| {
        [
            x.clamp(0.0, width.saturating_sub(1) as f32),
            y.clamp(0.0, height.saturating_sub(1) as f32),
        ]
    });
    let area = polygon_area_f32(&clamped);
    let image_area = (width as f32) * (height as f32);
    if area / image_area.max(1.0) < MIN_AREA_RATIO as f32 {
        // A walk that collapsed the quad found picture structure it cannot
        // reason about; keep the detected boundary instead.
        return points;
    }

    clamped.map(|[x, y]| Point::new(x.round() as i32, y.round() as i32))
}

/// Inward inset of one edge to its frame gate, or `None` when the walk finds no
/// rebate band behind the edge.
fn edge_gate_inset(
    rgb: &image::RgbImage,
    a: [f32; 2],
    b: [f32; 2],
    opposite: [f32; 2],
) -> Option<f32> {
    let edge = [b[0] - a[0], b[1] - a[1]];
    let edge_length = (edge[0] * edge[0] + edge[1] * edge[1]).sqrt();
    if edge_length < 8.0 {
        return None;
    }
    let inward = inward_normal(a, b, opposite);

    let max_inset = edge_length * REFINE_MAX_INSET_FRACTION;
    let skip_limit = edge_length * REFINE_SKIP_LIMIT_FRACTION;
    let min_run = (edge_length * REFINE_MIN_RUN_FRACTION).max(2.0);
    let profile = colour_profile(rgb, a, b, inward, edge_length, 0.0, max_inset);

    // The rebate reads as whatever material sits a few pixels inside the
    // detected boundary: past the dense film-edge line, before the gate.
    let anchor_start = (max_inset * 0.02).max(2.0);
    let anchor_end = (max_inset * 0.06).max(6.0);
    let mut reference = [0.0f32; 3];
    {
        let mut count = 0usize;
        for &(offset, colour, _) in profile.iter() {
            if offset >= anchor_start && offset <= anchor_end {
                for channel in 0..3 {
                    reference[channel] += colour[channel];
                }
                count += 1;
            }
        }
        if count == 0 {
            return None;
        }
        reference = reference.map(|value| value / count as f32);
    }
    let distance_to_reference = |colour: [f32; 3]| -> f32 {
        (0..3)
            .map(|channel| (colour[channel] - reference[channel]).abs())
            .sum::<f32>()
            / 3.0
    };

    // Walk inward from the boundary. Tolerate the initial dense edge line, then
    // follow the base-like run and end it at the gate.
    let mut run_start: Option<f32> = None;
    let mut last_match: Option<f32> = None;
    let mut mismatch_streak = 0u32;
    for &(offset, colour, _) in profile.iter() {
        let matching = distance_to_reference(colour) <= REFINE_MATCH_MAX_DISTANCE;
        if run_start.is_none() {
            if matching {
                run_start = Some(offset);
                last_match = Some(offset);
                mismatch_streak = 0;
            } else if offset > skip_limit {
                // No rebate band behind this edge: the detection either sits on
                // the gate already or found content the walk cannot classify.
                return None;
            }
            continue;
        }
        if matching {
            mismatch_streak = 0;
            last_match = Some(offset);
        } else {
            mismatch_streak += 1;
            if mismatch_streak >= REFINE_RUN_BREAK_SAMPLES {
                break;
            }
        }
    }

    let (start, end) = (run_start?, last_match?);
    // A run that reaches the end of the search never found the gate line. That
    // is the shape of a uniform picture interior (a white polygon, a clear
    // sky), not of a rebate bounded by a frame gate, so the edge keeps its
    // detected line.
    if end >= max_inset - 2.0 {
        return None;
    }
    (end - start >= min_run).then_some(end)
}

fn inward_normal(a: [f32; 2], b: [f32; 2], opposite: [f32; 2]) -> [f32; 2] {
    let edge = [b[0] - a[0], b[1] - a[1]];
    let length = (edge[0] * edge[0] + edge[1] * edge[1]).sqrt().max(1.0);
    let along = [edge[0] / length, edge[1] / length];
    let midpoint = [(a[0] + b[0]) * 0.5, (a[1] + b[1]) * 0.5];
    let to_opposite = [opposite[0] - midpoint[0], opposite[1] - midpoint[1]];
    let positive = [-along[1], along[0]];
    let negative = [along[1], -along[0]];
    let dot = |normal: [f32; 2]| normal[0] * to_opposite[0] + normal[1] * to_opposite[1];
    if dot(positive) >= dot(negative) {
        positive
    } else {
        negative
    }
}

fn intersect_lines_f32(
    point_a: [f32; 2],
    direction_a: [f32; 2],
    point_b: [f32; 2],
    direction_b: [f32; 2],
) -> Option<[f32; 2]> {
    let denominator = direction_a[0] * direction_b[1] - direction_a[1] * direction_b[0];
    if denominator.abs() < 1.0e-6 {
        return None;
    }
    let delta = [point_b[0] - point_a[0], point_b[1] - point_a[1]];
    let t = (delta[0] * direction_b[1] - delta[1] * direction_b[0]) / denominator;
    Some([
        point_a[0] + direction_a[0] * t,
        point_a[1] + direction_a[1] * t,
    ])
}

fn project_onto_line(point: [f32; 2], line_point: [f32; 2], direction: [f32; 2]) -> [f32; 2] {
    let delta = [point[0] - line_point[0], point[1] - line_point[1]];
    let t = delta[0] * direction[0] + delta[1] * direction[1];
    [
        line_point[0] + direction[0] * t,
        line_point[1] + direction[1] * t,
    ]
}

/// Sample lines parallel to one edge at increasing inward offsets. For each
/// offset the median colour and that line's own colour spread are reported;
/// medians reject sprocket holes and edge print markings.
fn colour_profile(
    rgb: &image::RgbImage,
    a: [f32; 2],
    b: [f32; 2],
    inward: [f32; 2],
    edge_length: f32,
    start_offset: f32,
    max_offset: f32,
) -> Vec<(f32, [f32; 3], f32)> {
    let (width, height) = rgb.dimensions();
    let sample_count = ((edge_length * 0.15) as usize).clamp(24, 96);
    let mut offsets = Vec::new();
    let mut offset = start_offset;
    while offset <= max_offset {
        offsets.push(offset);
        offset += 1.0;
    }

    let mut buffers = vec![Vec::with_capacity(sample_count); offsets.len()];
    for sample in 0..sample_count {
        let t = REFINE_EDGE_SPAN_TRIM
            + (1.0 - 2.0 * REFINE_EDGE_SPAN_TRIM) * (sample as f32 / (sample_count - 1) as f32);
        let base = [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t];
        for (index, &o) in offsets.iter().enumerate() {
            let point = [base[0] + inward[0] * o, base[1] + inward[1] * o];
            if point[0] < 0.0
                || point[1] < 0.0
                || point[0] >= width as f32
                || point[1] >= height as f32
            {
                continue;
            }
            if let Some(colour) = sample_bilinear(rgb, point[0], point[1]) {
                buffers[index].push(colour);
            }
        }
    }

    let mut profile = Vec::with_capacity(offsets.len());
    for (index, buffer) in buffers.into_iter().enumerate() {
        if buffer.len() < sample_count / 3 {
            continue;
        }
        let median = |mut values: Vec<f32>| -> f32 {
            let middle = values.len() / 2;
            values.select_nth_unstable_by_key(middle, |value| value.to_bits());
            values[middle]
        };
        let channels = std::array::from_fn(|channel| {
            median(buffer.iter().map(|colour| colour[channel]).collect())
        });
        let spread = median(
            buffer
                .iter()
                .map(|colour| {
                    (0..3)
                        .map(|channel| (colour[channel] - channels[channel]).abs())
                        .sum::<f32>()
                        / 3.0
                })
                .collect(),
        );
        profile.push((offsets[index], channels, spread));
    }
    profile
}

fn sample_bilinear(rgb: &image::RgbImage, x: f32, y: f32) -> Option<[f32; 3]> {
    let (width, height) = rgb.dimensions();
    if x < 0.0 || y < 0.0 || x >= width as f32 || y >= height as f32 {
        return None;
    }
    let x0 = x.floor() as u32;
    let y0 = y.floor() as u32;
    let x1 = (x0 + 1).min(width - 1);
    let y1 = (y0 + 1).min(height - 1);
    let fx = x - x0 as f32;
    let fy = y - y0 as f32;
    let get = |px: u32, py: u32| -> [f32; 3] {
        let pixel = rgb.get_pixel(px, py).0;
        [pixel[0] as f32, pixel[1] as f32, pixel[2] as f32]
    };
    let top: [f32; 3] = std::array::from_fn(|channel| {
        get(x0, y0)[channel] * (1.0 - fx) + get(x1, y0)[channel] * fx
    });
    let bottom: [f32; 3] = std::array::from_fn(|channel| {
        get(x0, y1)[channel] * (1.0 - fx) + get(x1, y1)[channel] * fx
    });
    Some(std::array::from_fn(|channel| {
        top[channel] * (1.0 - fy) + bottom[channel] * fy
    }))
}

fn working_rgb(thumbnail: &DynamicImage) -> Option<image::RgbImage> {
    let (width, height) = thumbnail.dimensions();
    let longest = width.max(height);
    if longest < 16 {
        return None;
    }
    let working = if longest > 1024 {
        let scale = 1024.0 / longest as f64;
        thumbnail.resize_exact(
            ((width as f64 * scale).round() as u32).max(16),
            ((height as f64 * scale).round() as u32).max(16),
            image::imageops::FilterType::Triangle,
        )
    } else {
        thumbnail.clone()
    };
    Some(working.to_rgb8())
}

fn polygon_area_f32(points: &[[f32; 2]; 4]) -> f32 {
    let mut twice_area = 0.0f32;
    for index in 0..4 {
        let left = points[index];
        let right = points[(index + 1) % 4];
        twice_area += left[0] * right[1] - right[0] * left[1];
    }
    twice_area.abs() / 2.0
}

fn analysis_grayscale(thumbnail: &DynamicImage) -> GrayImage {
    let source = thumbnail.to_luma8();
    let (width, height) = thumbnail.dimensions();
    let longest_edge = width.max(height);
    if longest_edge <= MAX_ANALYSIS_EDGE {
        return source;
    }

    let scale = MAX_ANALYSIS_EDGE as f64 / longest_edge as f64;
    let analysis_width = (width as f64 * scale).round().max(16.0) as u32;
    let analysis_height = (height as f64 * scale).round().max(16.0) as u32;
    let mut gray = GrayImage::new(analysis_width, analysis_height);
    let mut sums = vec![0u64; analysis_width as usize];
    for y in 0..analysis_height {
        let source_y0 = y * height / analysis_height;
        let source_y1 = ((y + 1) * height / analysis_height)
            .max(source_y0 + 1)
            .min(height);
        sums.fill(0);
        for x in 0..analysis_width {
            let source_x0 = x * width / analysis_width;
            let source_x1 = ((x + 1) * width / analysis_width)
                .max(source_x0 + 1)
                .min(width);
            for source_y in source_y0..source_y1 {
                for source_x in source_x0..source_x1 {
                    sums[x as usize] += u64::from(source.get_pixel(source_x, source_y)[0]);
                }
            }
        }
        for x in 0..analysis_width {
            let source_x0 = x * width / analysis_width;
            let source_x1 = ((x + 1) * width / analysis_width)
                .max(source_x0 + 1)
                .min(width);
            let count = u64::from(source_y1 - source_y0) * u64::from(source_x1 - source_x0);
            gray.put_pixel(x, y, image::Luma([(sums[x as usize] / count) as u8]));
        }
    }
    gray
}

fn find_best_quad(
    edges: &image::GrayImage,
    width: u32,
    height: u32,
    preferred_aspect: Option<f64>,
) -> Option<([Point<i32>; 4], f64, f64)> {
    let mut best: Option<([Point<i32>; 4], f64, f64)> = None;
    for contour in find_contours_with_threshold::<i32>(&edges, 0) {
        if contour.points.len() < 12 {
            continue;
        }

        let perimeter = arc_length(&contour.points, true);
        if perimeter <= 0.0 {
            continue;
        }

        let mut quad = None;
        for epsilon_ratio in [0.01, 0.015, 0.02, 0.025, 0.03, 0.04, 0.05] {
            let polygon =
                approximate_polygon_dp(&contour.points, (perimeter * epsilon_ratio).max(1.0), true);
            if polygon.len() == 4 {
                quad = Some([polygon[0], polygon[1], polygon[2], polygon[3]]);
                break;
            }
        }

        let Some(points) = quad.map(order_quad) else {
            continue;
        };
        let area = polygon_area(&points);
        if !validate_quad(&points, width, height, area) {
            continue;
        }
        let aspect_score = candidate_aspect_score(&points, preferred_aspect);
        let score = area * (0.82 + 0.30 * aspect_score);
        if best
            .as_ref()
            .is_none_or(|(_, best_score, _)| score > *best_score)
        {
            best = Some((points, score, aspect_score));
        }
    }

    best
}

fn detect_from_gradient_projections(gray: &image::GrayImage) -> Option<[Point<i32>; 4]> {
    let (width, height) = gray.dimensions();
    if width < 32 || height < 32 {
        return None;
    }

    let x_gradient = horizontal_sobel(gray);
    let y_gradient = vertical_sobel(gray);
    let left_projection = gradient_projection_x(&x_gradient, height, -1);
    let right_projection = gradient_projection_x(&x_gradient, height, 1);
    let top_projection = gradient_projection_y(&y_gradient, width, -1);
    let bottom_projection = gradient_projection_y(&y_gradient, width, 1);

    // Search almost the entire opposing halves. Small gates can sit well inside
    // the old fixed bands, while a cropped scan may place an edge near a side.
    let (left_x, left_peak, left_prominence) = strongest_projection(&left_projection, 0.015, 0.49)?;
    let (right_x, right_peak, right_prominence) =
        strongest_projection(&right_projection, 0.51, 0.985)?;
    let (top_y, top_peak, top_prominence) = strongest_projection(&top_projection, 0.015, 0.49)?;
    let (bottom_y, bottom_peak, bottom_prominence) =
        strongest_projection(&bottom_projection, 0.51, 0.985)?;

    let prominences = [
        left_prominence,
        right_prominence,
        top_prominence,
        bottom_prominence,
    ];
    let mean_prominence = prominences.iter().sum::<f64>() / prominences.len() as f64;
    let strong_edges = prominences
        .iter()
        .filter(|prominence| **prominence >= MIN_GRADIENT_PROMINENCE)
        .count();
    // A single missing edge can still be a genuine scan, but accepting three
    // arbitrary peaks makes subject edges look like the film gate and corrupts
    // the analysis bounds. Require all four gate edges for the automatic path;
    // otherwise fall back to manual framing.
    if strong_edges < 4 || mean_prominence < MIN_GRADIENT_PROMINENCE {
        return None;
    }

    let vertical_radius = (width as f64 * 0.035).round().max(3.0) as i32;
    let horizontal_radius = (height as f64 * 0.035).round().max(3.0) as i32;
    let left = fit_vertical_edge(
        &x_gradient,
        left_x,
        top_y,
        bottom_y,
        vertical_radius,
        left_peak,
        -1,
    );
    let right = fit_vertical_edge(
        &x_gradient,
        right_x,
        top_y,
        bottom_y,
        vertical_radius,
        right_peak,
        1,
    );
    let top = fit_horizontal_edge(
        &y_gradient,
        top_y,
        left_x,
        right_x,
        horizontal_radius,
        top_peak,
        -1,
    );
    let bottom = fit_horizontal_edge(
        &y_gradient,
        bottom_y,
        left_x,
        right_x,
        horizontal_radius,
        bottom_peak,
        1,
    );

    let points = [
        intersect_lines(left, top)?,
        intersect_lines(right, top)?,
        intersect_lines(right, bottom)?,
        intersect_lines(left, bottom)?,
    ];
    let area = polygon_area(&points);
    validate_quad(&points, width, height, area).then_some(points)
}

fn gradient_projection_x(
    gradient: &image::ImageBuffer<image::Luma<i16>, Vec<i16>>,
    height: u32,
    polarity: i32,
) -> Vec<f64> {
    let start_y = (height as f64 * 0.08).round() as u32;
    let end_y = (height as f64 * 0.92).round() as u32;
    let values = (0..gradient.width())
        .map(|x| {
            let mut samples = (start_y..end_y)
                .map(|y| (gradient.get_pixel(x, y)[0] as f64 * polarity as f64).max(0.0))
                .collect::<Vec<_>>();
            upper_projection_quantile(&mut samples)
        })
        .collect::<Vec<_>>();
    smooth_projection(&values, (gradient.width() / 128).max(1) as usize)
}

fn gradient_projection_y(
    gradient: &image::ImageBuffer<image::Luma<i16>, Vec<i16>>,
    width: u32,
    polarity: i32,
) -> Vec<f64> {
    let start_x = (width as f64 * 0.08).round() as u32;
    let end_x = (width as f64 * 0.92).round() as u32;
    let values = (0..gradient.height())
        .map(|y| {
            let mut samples = (start_x..end_x)
                .map(|x| (gradient.get_pixel(x, y)[0] as f64 * polarity as f64).max(0.0))
                .collect::<Vec<_>>();
            upper_projection_quantile(&mut samples)
        })
        .collect::<Vec<_>>();
    smooth_projection(&values, (gradient.height() / 128).max(1) as usize)
}

fn upper_projection_quantile(samples: &mut [f64]) -> f64 {
    if samples.is_empty() {
        return 0.0;
    }
    let index = ((samples.len() - 1) as f64 * 0.8).round() as usize;
    samples.select_nth_unstable_by(index, |left, right| left.total_cmp(right));
    samples[index]
}

fn smooth_projection(values: &[f64], radius: usize) -> Vec<f64> {
    let mut prefix = Vec::with_capacity(values.len() + 1);
    prefix.push(0.0);
    for value in values {
        prefix.push(prefix.last().copied().unwrap_or_default() + value);
    }
    (0..values.len())
        .map(|index| {
            let start = index.saturating_sub(radius);
            let end = (index + radius + 1).min(values.len());
            (prefix[end] - prefix[start]) / (end - start).max(1) as f64
        })
        .collect()
}

fn strongest_projection(
    values: &[f64],
    start_ratio: f64,
    end_ratio: f64,
) -> Option<(i32, f64, f64)> {
    let start = (values.len() as f64 * start_ratio).floor() as usize;
    let end = ((values.len() as f64 * end_ratio).ceil() as usize).min(values.len());
    if end <= start + 2 {
        return None;
    }

    let (relative_index, peak) = values[start..end]
        .iter()
        .copied()
        .enumerate()
        .max_by(|left, right| left.1.total_cmp(&right.1))?;
    let mut baseline = values[start..end].to_vec();
    let baseline = median_f64(&mut baseline);
    let prominence = (peak + 1.0) / (baseline + 1.0);
    (peak >= 6.0).then_some(((start + relative_index) as i32, peak, prominence))
}

fn median_f64(samples: &mut [f64]) -> f64 {
    if samples.is_empty() {
        return 0.0;
    }
    let middle = samples.len() / 2;
    samples.select_nth_unstable_by(middle, |left, right| left.total_cmp(right));
    samples[middle]
}

fn fit_vertical_edge(
    gradient: &image::ImageBuffer<image::Luma<i16>, Vec<i16>>,
    base_x: i32,
    start_y: i32,
    end_y: i32,
    radius: i32,
    peak: f64,
    polarity: i32,
) -> FittedLine {
    let samples = (start_y.max(1)..end_y.min(gradient.height() as i32 - 1))
        .step_by(2)
        .filter_map(|y| {
            strongest_local_x(gradient, base_x, y, radius, polarity)
                .filter(|(_, strength)| *strength >= peak * 0.32)
                .map(|(x, strength)| (y as f64, x as f64, strength))
        })
        .collect::<Vec<_>>();
    fit_line(&samples, base_x as f64)
}

fn fit_horizontal_edge(
    gradient: &image::ImageBuffer<image::Luma<i16>, Vec<i16>>,
    base_y: i32,
    start_x: i32,
    end_x: i32,
    radius: i32,
    peak: f64,
    polarity: i32,
) -> FittedLine {
    let samples = (start_x.max(1)..end_x.min(gradient.width() as i32 - 1))
        .step_by(2)
        .filter_map(|x| {
            strongest_local_y(gradient, x, base_y, radius, polarity)
                .filter(|(_, strength)| *strength >= peak * 0.32)
                .map(|(y, strength)| (x as f64, y as f64, strength))
        })
        .collect::<Vec<_>>();
    fit_line(&samples, base_y as f64)
}

fn strongest_local_x(
    gradient: &image::ImageBuffer<image::Luma<i16>, Vec<i16>>,
    center_x: i32,
    y: i32,
    radius: i32,
    polarity: i32,
) -> Option<(i32, f64)> {
    ((center_x - radius).max(1)..=(center_x + radius).min(gradient.width() as i32 - 2))
        .map(|x| {
            (
                x,
                (gradient.get_pixel(x as u32, y as u32)[0] as f64 * polarity as f64).max(0.0),
            )
        })
        .max_by(|left, right| left.1.total_cmp(&right.1))
}

fn strongest_local_y(
    gradient: &image::ImageBuffer<image::Luma<i16>, Vec<i16>>,
    x: i32,
    center_y: i32,
    radius: i32,
    polarity: i32,
) -> Option<(i32, f64)> {
    ((center_y - radius).max(1)..=(center_y + radius).min(gradient.height() as i32 - 2))
        .map(|y| {
            (
                y,
                (gradient.get_pixel(x as u32, y as u32)[0] as f64 * polarity as f64).max(0.0),
            )
        })
        .max_by(|left, right| left.1.total_cmp(&right.1))
}

fn fit_line(samples: &[(f64, f64, f64)], fallback_intercept: f64) -> FittedLine {
    if samples.len() < 8 {
        return FittedLine {
            slope: 0.0,
            intercept: fallback_intercept,
        };
    }

    let weight_sum = samples.iter().map(|sample| sample.2).sum::<f64>();
    let mean_x = samples
        .iter()
        .map(|sample| sample.0 * sample.2)
        .sum::<f64>()
        / weight_sum;
    let mean_y = samples
        .iter()
        .map(|sample| sample.1 * sample.2)
        .sum::<f64>()
        / weight_sum;
    let variance = samples
        .iter()
        .map(|sample| sample.2 * (sample.0 - mean_x).powi(2))
        .sum::<f64>();
    if variance <= f64::EPSILON {
        return FittedLine {
            slope: 0.0,
            intercept: fallback_intercept,
        };
    }

    let covariance = samples
        .iter()
        .map(|sample| sample.2 * (sample.0 - mean_x) * (sample.1 - mean_y))
        .sum::<f64>();
    let slope = (covariance / variance).clamp(-0.25, 0.25);
    FittedLine {
        slope,
        intercept: mean_y - slope * mean_x,
    }
}

fn intersect_lines(vertical: FittedLine, horizontal: FittedLine) -> Option<Point<i32>> {
    let denominator = 1.0 - vertical.slope * horizontal.slope;
    if denominator.abs() < 0.2 {
        return None;
    }
    let x = (vertical.slope * horizontal.intercept + vertical.intercept) / denominator;
    let y = horizontal.slope * x + horizontal.intercept;
    (x.is_finite() && y.is_finite()).then_some(Point::new(x.round() as i32, y.round() as i32))
}

fn detected_result(
    points: [Point<i32>; 4],
    width: u32,
    height: u32,
    status: &'static str,
    aspect_score: f64,
) -> FilmBorderDetection {
    let width_scale = (width.saturating_sub(1)).max(1) as f32;
    let height_scale = (height.saturating_sub(1)).max(1) as f32;
    let mut normalized = points.map(|point| {
        [
            (point.x as f32 / width_scale).clamp(0.0, 1.0),
            (point.y as f32 / height_scale).clamp(0.0, 1.0),
        ]
    });
    normalized = inset_automatic_points(normalized);
    FilmBorderDetection {
        points: normalized,
        confidence: if aspect_score >= 0.35 {
            DetectionConfidence::High
        } else {
            DetectionConfidence::Low
        },
        status,
    }
}

fn inset_automatic_points(points: [[f32; 2]; 4]) -> [[f32; 2]; 4] {
    inset_points(points, AUTO_DETECTION_INSET)
}

fn inset_points(points: [[f32; 2]; 4], inset: f32) -> [[f32; 2]; 4] {
    let center = points.iter().fold([0.0f32; 2], |sum, point| {
        [sum[0] + point[0], sum[1] + point[1]]
    });
    let center = [center[0] * 0.25, center[1] * 0.25];
    let scale = 1.0 - 2.0 * inset;
    points.map(|point| {
        [
            center[0] + (point[0] - center[0]) * scale,
            center[1] + (point[1] - center[1]) * scale,
        ]
    })
}

fn expected_frame_aspect(roll_format: &str) -> Option<f64> {
    let normalized = roll_format.trim().to_ascii_lowercase();
    if normalized.contains("half-frame") || normalized.contains("half frame") {
        return Some(4.0 / 3.0);
    }
    if normalized.contains("xpan") {
        return Some(65.0 / 24.0);
    }
    if normalized == "135" || normalized.starts_with("135 ") || normalized.starts_with("35mm") {
        return Some(3.0 / 2.0);
    }
    if normalized.contains("6x4.5") || normalized.contains("645") {
        return Some(4.0 / 3.0);
    }
    if normalized.contains("6x6") {
        return Some(1.0);
    }
    if normalized.contains("6x7") {
        return Some(6.0 / 7.0);
    }
    if normalized.contains("6x9") {
        return Some(6.0 / 9.0);
    }
    if normalized.contains("6x12") {
        return Some(6.0 / 12.0);
    }
    if normalized.contains("6x17") {
        return Some(6.0 / 17.0);
    }
    if normalized.contains("4x5") {
        return Some(4.0 / 5.0);
    }
    if normalized.contains("5x7") {
        return Some(5.0 / 7.0);
    }
    if normalized.contains("8x10") {
        return Some(8.0 / 10.0);
    }
    None
}

fn candidate_aspect_score(points: &[Point<i32>; 4], preferred_aspect: Option<f64>) -> f64 {
    let corners = points.map(|point| [point.x as f64, point.y as f64]);
    let side =
        |left: [f64; 2], right: [f64; 2]| (right[0] - left[0]).hypot(right[1] - left[1]).max(1.0);
    let horizontal = (side(corners[0], corners[1]) + side(corners[3], corners[2])) * 0.5;
    let vertical = (side(corners[0], corners[3]) + side(corners[1], corners[2])) * 0.5;
    let actual = (horizontal / vertical).max(1.0 / 50.0);
    let standard = STANDARD_FRAME_ASPECTS
        .iter()
        .map(|expected| aspect_match_score(actual, *expected))
        .fold(0.0, f64::max);
    let Some(preferred) = preferred_aspect else {
        return standard;
    };
    let preferred = aspect_match_score(actual, preferred);
    0.68 * preferred + 0.32 * standard
}

fn aspect_match_score(actual: f64, expected: f64) -> f64 {
    let actual = if actual < 1.0 {
        1.0 / actual.max(1.0e-6)
    } else {
        actual
    };
    let expected = if expected < 1.0 {
        1.0 / expected.max(1.0e-6)
    } else {
        expected
    };
    let distance = (actual / expected).ln() / ASPECT_SCORE_SIGMA;
    (-distance * distance).exp()
}

fn order_quad(points: [Point<i32>; 4]) -> [Point<i32>; 4] {
    let center_x = points.iter().map(|point| point.x as f64).sum::<f64>() / 4.0;
    let center_y = points.iter().map(|point| point.y as f64).sum::<f64>() / 4.0;
    let mut ordered = points;
    ordered.sort_by(|left, right| {
        let left_angle = (left.y as f64 - center_y).atan2(left.x as f64 - center_x);
        let right_angle = (right.y as f64 - center_y).atan2(right.x as f64 - center_x);
        left_angle.total_cmp(&right_angle)
    });

    let top_left = ordered
        .iter()
        .enumerate()
        .min_by_key(|(_, point)| point.x + point.y)
        .map(|(index, _)| index)
        .unwrap_or(0);
    std::array::from_fn(|index| ordered[(top_left + index) % 4])
}

fn polygon_area(points: &[Point<i32>; 4]) -> f64 {
    let twice_area = points
        .iter()
        .zip(points.iter().cycle().skip(1))
        .take(4)
        .map(|(left, right)| left.x as i64 * right.y as i64 - right.x as i64 * left.y as i64)
        .sum::<i64>();
    twice_area.unsigned_abs() as f64 / 2.0
}

fn distance(left: Point<i32>, right: Point<i32>) -> f64 {
    let dx = (right.x - left.x) as f64;
    let dy = (right.y - left.y) as f64;
    dx.hypot(dy)
}

fn validate_quad(points: &[Point<i32>; 4], width: u32, height: u32, area: f64) -> bool {
    if points.iter().any(|point| {
        point.x < 0 || point.y < 0 || point.x >= width as i32 || point.y >= height as i32
    }) {
        return false;
    }

    let image_area = width as f64 * height as f64;
    let area_ratio = area / image_area.max(1.0);
    if !(MIN_AREA_RATIO..=MAX_AREA_RATIO).contains(&area_ratio) {
        return false;
    }

    let sides = [
        distance(points[0], points[1]),
        distance(points[1], points[2]),
        distance(points[2], points[3]),
        distance(points[3], points[0]),
    ];
    let min_side = sides.iter().copied().fold(f64::INFINITY, f64::min);
    let max_side = sides.iter().copied().fold(0.0_f64, f64::max);
    if min_side < width.min(height) as f64 * 0.04 || max_side / min_side > MAX_SIDE_RATIO {
        return false;
    }

    let diagonal_a = distance(points[0], points[2]);
    let diagonal_b = distance(points[1], points[3]);
    let diagonal_ratio = diagonal_a.max(diagonal_b) / diagonal_a.min(diagonal_b).max(1.0);
    diagonal_ratio <= MAX_DIAGONAL_RATIO
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::imageops::FilterType;
    use image::{DynamicImage, GrayImage, Luma};
    use imageproc::drawing::{draw_filled_rect_mut, draw_polygon_mut};
    use imageproc::rect::Rect;

    fn textured_film_strip() -> GrayImage {
        let mut image = GrayImage::from_pixel(320, 210, Luma([178]));
        draw_filled_rect_mut(&mut image, Rect::at(48, 38).of_size(226, 136), Luma([62]));

        for x in (22..300).step_by(30) {
            draw_filled_rect_mut(&mut image, Rect::at(x, 9).of_size(14, 19), Luma([235]));
            draw_filled_rect_mut(&mut image, Rect::at(x, 182).of_size(14, 19), Luma([235]));
        }
        for y in (52..168).step_by(16) {
            draw_filled_rect_mut(&mut image, Rect::at(70, y).of_size(175, 4), Luma([90]));
        }
        image
    }

    #[test]
    fn blank_thumbnail_fallback_keeps_the_full_frame() {
        let image = DynamicImage::ImageLuma8(GrayImage::from_pixel(320, 200, Luma([16])));
        let result = detect_film_border(&image);

        assert_eq!(result.confidence, DetectionConfidence::Low);
        assert_eq!(result.points, FilmBorderDetection::fallback().points);
    }

    #[test]
    fn detects_a_small_centered_gate() {
        let mut image = GrayImage::from_pixel(400, 260, Luma([184]));
        draw_filled_rect_mut(&mut image, Rect::at(154, 88).of_size(92, 84), Luma([58]));
        for y in (96..168).step_by(10) {
            draw_filled_rect_mut(&mut image, Rect::at(166, y).of_size(66, 3), Luma([86]));
        }

        let result = detect_film_border(&DynamicImage::ImageLuma8(image));

        assert_eq!(result.confidence, DetectionConfidence::High);
        assert!((result.points[0][0] - 154.0 / 399.0).abs() < 0.04);
        assert!((result.points[0][1] - 88.0 / 259.0).abs() < 0.04);
        assert!((result.points[2][0] - 246.0 / 399.0).abs() < 0.04);
        assert!((result.points[2][1] - 172.0 / 259.0).abs() < 0.04);
    }

    #[test]
    fn detects_a_clear_quadrilateral() {
        let mut image = GrayImage::from_pixel(400, 260, Luma([8]));
        let polygon = [
            Point::new(42, 35),
            Point::new(355, 28),
            Point::new(370, 224),
            Point::new(32, 232),
        ];
        draw_polygon_mut(&mut image, &polygon, Luma([240]));

        let result = detect_film_border(&DynamicImage::ImageLuma8(image));

        assert_eq!(result.confidence, DetectionConfidence::High);
        assert!(result.points[0][0] < 0.2);
        assert!(result.points[0][1] < 0.2);
        assert!(result.points[2][0] > 0.8);
        assert!(result.points[2][1] > 0.8);
    }

    #[test]
    fn detects_inner_gate_in_a_textured_film_strip() {
        let result = detect_film_border(&DynamicImage::ImageLuma8(textured_film_strip()));

        assert_eq!(result.confidence, DetectionConfidence::High);
        assert_eq!(result.status, "detected_gradient");
        assert!((result.points[0][0] - 48.0 / 319.0).abs() < 0.03);
        assert!((result.points[0][1] - 38.0 / 209.0).abs() < 0.03);
        assert!((result.points[2][0] - 274.0 / 319.0).abs() < 0.03);
        assert!((result.points[2][1] - 174.0 / 209.0).abs() < 0.03);
    }

    #[test]
    fn large_preview_keeps_gate_accuracy_after_analysis_downsampling() {
        let large =
            image::imageops::resize(&textured_film_strip(), 1600, 1050, FilterType::Nearest);
        let dynamic = DynamicImage::ImageLuma8(large);
        let analysis = analysis_grayscale(&dynamic);

        assert_eq!(analysis.dimensions(), (640, 420));

        let result = detect_film_border(&dynamic);
        assert_eq!(result.confidence, DetectionConfidence::High);
        assert_eq!(result.status, "detected_gradient");
        assert!((result.points[0][0] - 48.0 / 319.0).abs() < 0.03);
        assert!((result.points[0][1] - 38.0 / 209.0).abs() < 0.03);
        assert!((result.points[2][0] - 274.0 / 319.0).abs() < 0.03);
        assert!((result.points[2][1] - 174.0 / 209.0).abs() < 0.03);
    }

    #[test]
    fn analysis_downsampling_averages_source_area_instead_of_point_sampling() {
        let mut image = GrayImage::from_pixel(1280, 640, Luma([0]));
        for y in 0..640 {
            for x in (0..1280).step_by(2) {
                image.put_pixel(x, y, Luma([255]));
            }
        }

        let analysis = analysis_grayscale(&DynamicImage::ImageLuma8(image));

        assert_eq!(analysis.dimensions(), (640, 320));
        assert!(analysis
            .pixels()
            .all(|pixel| (pixel[0] as i32 - 128).abs() <= 1));
    }

    #[test]
    fn roll_formats_map_to_the_standard_frame_prior() {
        assert_eq!(expected_frame_aspect("135"), Some(3.0 / 2.0));
        assert_eq!(expected_frame_aspect("135 Half-frame"), Some(4.0 / 3.0));
        assert_eq!(expected_frame_aspect("120 (6x6)"), Some(1.0));
        assert_eq!(expected_frame_aspect("120 (6x7)"), Some(6.0 / 7.0));
        assert_eq!(expected_frame_aspect("120 (6x9)"), Some(6.0 / 9.0));
        assert_eq!(expected_frame_aspect("XPan"), Some(65.0 / 24.0));
        assert_eq!(expected_frame_aspect("APS"), None);
    }

    #[test]
    fn aspect_prior_weights_candidates_without_rejecting_unusual_frames() {
        let three_by_two = [
            Point::new(0, 0),
            Point::new(300, 0),
            Point::new(300, 200),
            Point::new(0, 200),
        ];
        let square = [
            Point::new(0, 0),
            Point::new(200, 0),
            Point::new(200, 200),
            Point::new(0, 200),
        ];

        let square_as_three_by_two = candidate_aspect_score(&square, Some(3.0 / 2.0));
        let square_as_square = candidate_aspect_score(&square, Some(1.0));
        assert!(square_as_square > square_as_three_by_two);
        assert!(square_as_three_by_two > 0.0);
        assert!(candidate_aspect_score(&three_by_two, Some(3.0 / 2.0)) > 0.98);
    }

    #[test]
    fn automatic_detection_is_inset_by_one_percent_at_every_edge() {
        let points = [
            Point::new(0, 0),
            Point::new(99, 0),
            Point::new(99, 99),
            Point::new(0, 99),
        ];

        let result = detected_result(points, 100, 100, "synthetic", 1.0);

        let expected = [[0.01, 0.01], [0.99, 0.01], [0.99, 0.99], [0.01, 0.99]];
        for (point, expected) in result.points.iter().zip(expected) {
            assert!((point[0] - expected[0]).abs() < 1.0e-5);
            assert!((point[1] - expected[1]).abs() < 1.0e-5);
        }
    }

    fn prior_sample(points: [[f32; 2]; 4], confirmed: bool) -> FilmAreaPriorSample {
        FilmAreaPriorSample {
            points,
            confirmed,
            confidence: DetectionConfidence::High,
        }
    }

    #[test]
    fn roll_prior_requires_three_high_results_without_a_confirmed_frame() {
        let points = [[0.1, 0.1], [0.9, 0.1], [0.9, 0.9], [0.1, 0.9]];
        assert!(film_area_prior_from_samples(&[
            prior_sample(points, false),
            prior_sample(points, false),
        ])
        .is_none());

        let prior = film_area_prior_from_samples(&[
            prior_sample(points, false),
            prior_sample(points, false),
            prior_sample(points, false),
        ])
        .expect("three consistent High results form a prior");
        assert_eq!(prior.points, points);
        assert_eq!(prior.sample_count, 3);
    }

    #[test]
    fn one_confirmed_frame_is_enough_but_disagreement_disables_the_prior() {
        let points = [[0.1, 0.1], [0.9, 0.1], [0.9, 0.9], [0.1, 0.9]];
        assert!(film_area_prior_from_samples(&[prior_sample(points, true)]).is_some());

        let shifted = [[0.2, 0.1], [1.0, 0.1], [1.0, 0.9], [0.2, 0.9]];
        assert!(film_area_prior_from_samples(&[
            prior_sample(points, true),
            prior_sample(shifted, true),
        ])
        .is_none());
    }

    #[test]
    fn roll_prior_clamps_local_refinement_and_inherits_on_failure() {
        let prior = FilmAreaPrior {
            points: [[0.1, 0.1], [0.9, 0.1], [0.9, 0.9], [0.1, 0.9]],
            sample_count: 3,
        };
        let local = FilmBorderDetection {
            points: [[0.125, 0.1], [0.9, 0.1], [0.9, 0.9], [0.1, 0.9]],
            confidence: DetectionConfidence::High,
            status: "detected_gradient",
        };
        assert_eq!(
            apply_roll_prior(local, &prior).status,
            "detected_roll_prior"
        );

        let failed = FilmBorderDetection::fallback();
        let inherited = apply_roll_prior(failed, &prior);
        assert_eq!(inherited.confidence, DetectionConfidence::Low);
        assert_eq!(inherited.status, "prior_inherited");
        assert_eq!(
            inherited.points,
            inset_points(prior.points, ROLL_PRIOR_INHERITED_INSET)
        );
    }

    /// A synthetic scan: open light panel outside the film body, a uniform
    /// pale rebate band inside the film edge, and textured picture content
    /// inside the gate. The gradient detector lands on the film-body boundary;
    /// the gate refinement has to walk each edge in to the rebate's end.
    fn boundary_scan_thumbnail() -> DynamicImage {
        let mut image = image::RgbImage::from_pixel(400, 260, image::Rgb([244, 244, 244]));
        // Film body: rebate from (40, 30) to (360, 230).
        draw_filled_rect_mut(
            &mut image,
            Rect::at(40, 30).of_size(320, 200),
            image::Rgb([212, 172, 132]),
        );
        // Picture gate: textured content from (60, 50) to (340, 210).
        for y in 50..210 {
            for x in 60..340 {
                let value = ((x * 5 + y * 9) % 97) as u8;
                image.put_pixel(
                    x,
                    y,
                    image::Rgb([40 + value, 60 + value / 2, 80 + value / 3]),
                );
            }
        }
        DynamicImage::ImageRgb8(image)
    }

    #[test]
    fn refinement_walks_edges_in_to_the_frame_gate() {
        let thumbnail = boundary_scan_thumbnail();
        let boundary = [
            Point::new(40, 30),
            Point::new(359, 30),
            Point::new(359, 229),
            Point::new(40, 229),
        ];

        let refined = refine_to_gate(&thumbnail, boundary);

        // The gate sits 20 px inside the film boundary on every side.
        let expected_corners = [
            Point::new(60, 50),
            Point::new(339, 50),
            Point::new(339, 209),
            Point::new(60, 209),
        ];
        for (corner, expected) in refined.iter().zip(expected_corners.iter()) {
            assert!(
                (corner.x - expected.x).abs() <= 3 && (corner.y - expected.y).abs() <= 3,
                "corner {corner:?} vs {expected:?}"
            );
        }
    }

    #[test]
    fn refinement_keeps_a_quad_that_already_sits_on_a_uniform_picture() {
        // The gate is flush with the detected quad, and the picture interior is
        // one uniform colour: there is no rebate band to walk through, so every
        // edge must keep its detected line.
        let mut image = image::RgbImage::from_pixel(400, 260, image::Rgb([244, 244, 244]));
        draw_filled_rect_mut(
            &mut image,
            Rect::at(60, 50).of_size(280, 160),
            image::Rgb([190, 150, 120]),
        );
        let thumbnail = DynamicImage::ImageRgb8(image);
        let gate = [
            Point::new(60, 50),
            Point::new(339, 50),
            Point::new(339, 209),
            Point::new(60, 209),
        ];

        let refined = refine_to_gate(&thumbnail, gate);

        assert_eq!(refined, gate);
    }

    #[test]
    fn rejects_extreme_perspective() {
        let points = [
            Point::new(10, 10),
            Point::new(390, 10),
            Point::new(210, 30),
            Point::new(190, 30),
        ];
        assert!(!validate_quad(&points, 400, 260, polygon_area(&points)));
    }
}
