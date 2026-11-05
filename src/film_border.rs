use image::{imageops::colorops::grayscale, DynamicImage, GenericImageView, GrayImage, Pixel};
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
const MAX_ANALYSIS_EDGE: u32 = 320;

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
    let (width, height) = thumbnail.dimensions();
    if width < 16 || height < 16 {
        return FilmBorderDetection::fallback();
    }

    // The format-prior search is trusted only when the corner verifier
    // accepts the box. Otherwise the gradient search gets its turn, held to a
    // much stricter verifier bar: on film strips it often locks onto a line
    // inside the picture. A box neither of them can vouch for is still a far
    // better starting point than the gradient result, so it is handed back
    // with Low confidence.
    let gate = detect_gate_box(thumbnail);
    if let Some((gate, true)) = &gate {
        return gate_detection(gate, DetectionConfidence::High);
    }
    let gradient = detect_gradient_border(thumbnail);
    if gradient.confidence == DetectionConfidence::High
        && (gradient_is_verified(thumbnail, &gradient)
            || surround_is_uniform(thumbnail, &gradient))
    {
        return gradient;
    }
    match &gate {
        Some((gate, _)) => gate_detection(gate, DetectionConfidence::Low),
        None => FilmBorderDetection {
            confidence: DetectionConfidence::Low,
            ..gradient
        },
    }
}

fn gradient_is_verified(thumbnail: &DynamicImage, detection: &FilmBorderDetection) -> bool {
    let Some(working) = gate_working_rgb(thumbnail) else {
        return false;
    };
    let (width, height) = working.dimensions();
    let quad = detection
        .points
        .map(|point| [point[0] as f64 * width as f64, point[1] as f64 * height as f64]);
    // The gradient search also serves positive scans, where the picture is the
    // brighter side, so the verifier sees both polarities.
    gate_quad_is_sane(&quad, width as f64, height as f64)
        && [false, true].iter().any(|&inverted| {
            verify_corners_with_polarity(&working, &quad, inverted)
                .iter()
                .all(|probability| *probability >= GRADIENT_VERIFIER_THRESHOLD)
        })
}

/// A quad with one plain tone in a ring just outside it needs no verifier: a
/// masked scan, a mounted frame or a strip whose rebate runs all the way round.
/// A quad on a line inside the picture has picture in its ring.
fn surround_is_uniform(thumbnail: &DynamicImage, detection: &FilmBorderDetection) -> bool {
    let gray = analysis_grayscale(thumbnail);
    let (width, height) = gray.dimensions();
    let short = width.min(height) as f32;
    let (inner, outer) = (SURROUND_MARGIN * short, SURROUND_RING * short);
    let bound = |axis: usize, size: u32, pick_max: bool| {
        let values = detection.points.map(|point| point[axis] * size as f32);
        if pick_max {
            values.iter().fold(f32::MIN, |a, b| a.max(*b))
        } else {
            values.iter().fold(f32::MAX, |a, b| a.min(*b))
        }
    };
    let (left, right) = (bound(0, width, false), bound(0, width, true));
    let (top, bottom) = (bound(1, height, false), bound(1, height, true));

    let mut surround: Vec<u8> = gray
        .enumerate_pixels()
        .filter(|(x, y, _)| {
            let (x, y) = (*x as f32, *y as f32);
            let outside_inner =
                x < left - inner || x > right + inner || y < top - inner || y > bottom + inner;
            let inside_outer =
                x >= left - outer && x <= right + outer && y >= top - outer && y <= bottom + outer;
            outside_inner && inside_outer
        })
        .map(|(_, _, pixel)| pixel[0])
        .collect();
    if (surround.len() as f32) < SURROUND_MIN_AREA * (width * height) as f32 {
        return false;
    }
    surround.sort_unstable();
    let median = surround[surround.len() / 2] as i32;
    let matching = surround
        .iter()
        .filter(|value| (**value as i32 - median).abs() <= SURROUND_MAX_DEVIATION)
        .count();
    matching as f32 >= SURROUND_MIN_MATCH * surround.len() as f32
}

/// The candidates behind a detection, for the evaluation harness: the
/// format-prior box with its corner probabilities, and the gradient search.
#[allow(dead_code)]
#[doc(hidden)]
pub fn film_border_candidates(
    thumbnail: &DynamicImage,
) -> (Option<([[f32; 2]; 4], [f32; 4])>, FilmBorderDetection) {
    let gate = gate_working_rgb(thumbnail).and_then(|working| {
        let (gate, _) = detect_gate_box(thumbnail)?;
        let probabilities = verify_corners(&working, &gate.quad);
        Some((gate_detection(&gate, DetectionConfidence::Low).points, probabilities))
    });
    (gate, detect_gradient_border(thumbnail))
}

fn detect_gradient_border(thumbnail: &DynamicImage) -> FilmBorderDetection {
    let gray = analysis_grayscale(thumbnail);
    let (width, height) = gray.dimensions();
    let blurred = gaussian_blur_f32(&gray, 1.4);

    // Film gates are often not closed contours after thresholding: sprocket
    // holes and image texture break or join their edges. Locate the four long
    // gate edges from robust Sobel projections first, then fit each edge.
    if let Some(points) = detect_from_gradient_projections(&blurred) {
        return detected_result(refine_to_gate(thumbnail, points), width, height, "detected_gradient");
    }

    let binary = threshold(&blurred, otsu_level(&blurred));
    let otsu_edges = canny(&binary, 25.0, 75.0);
    let grayscale_edges = canny(&blurred, 18.0, 58.0);

    let best = find_best_quad(&grayscale_edges, width, height)
        .into_iter()
        .chain(find_best_quad(&otsu_edges, width, height))
        .max_by(|left, right| left.1.total_cmp(&right.1));

    let Some((points, _)) = best else {
        return FilmBorderDetection::fallback();
    };

    detected_result(
        refine_to_gate(thumbnail, points),
        width,
        height,
        "detected_contour",
    )
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
    Some([point_a[0] + direction_a[0] * t, point_a[1] + direction_a[1] * t])
}

fn project_onto_line(point: [f32; 2], line_point: [f32; 2], direction: [f32; 2]) -> [f32; 2] {
    let delta = [point[0] - line_point[0], point[1] - line_point[1]];
    let t = delta[0] * direction[0] + delta[1] * direction[1];
    [line_point[0] + direction[0] * t, line_point[1] + direction[1] * t]
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
    let top: [f32; 3] =
        std::array::from_fn(|channel| get(x0, y0)[channel] * (1.0 - fx) + get(x1, y0)[channel] * fx);
    let bottom: [f32; 3] =
        std::array::from_fn(|channel| get(x0, y1)[channel] * (1.0 - fx) + get(x1, y1)[channel] * fx);
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
    let (width, height) = thumbnail.dimensions();
    let longest_edge = width.max(height);
    if longest_edge <= MAX_ANALYSIS_EDGE {
        return grayscale(thumbnail);
    }

    let scale = MAX_ANALYSIS_EDGE as f64 / longest_edge as f64;
    let analysis_width = (width as f64 * scale).round().max(16.0) as u32;
    let analysis_height = (height as f64 * scale).round().max(16.0) as u32;
    let mut gray = GrayImage::new(analysis_width, analysis_height);
    for y in 0..analysis_height {
        let source_y = (((y as u64 * 2 + 1) * height as u64) / (analysis_height as u64 * 2))
            .min(height.saturating_sub(1) as u64) as u32;
        for x in 0..analysis_width {
            let source_x = (((x as u64 * 2 + 1) * width as u64) / (analysis_width as u64 * 2))
                .min(width.saturating_sub(1) as u64) as u32;
            gray.put_pixel(x, y, thumbnail.get_pixel(source_x, source_y).to_luma());
        }
    }
    gray
}

fn find_best_quad(
    edges: &image::GrayImage,
    width: u32,
    height: u32,
) -> Option<([Point<i32>; 4], f64)> {
    let mut best: Option<([Point<i32>; 4], f64)> = None;
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
        if validate_quad(&points, width, height, area)
            && best.as_ref().is_none_or(|(_, best_area)| area > *best_area)
        {
            best = Some((points, area));
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
) -> FilmBorderDetection {
    let width_scale = (width.saturating_sub(1)).max(1) as f32;
    let height_scale = (height.saturating_sub(1)).max(1) as f32;
    FilmBorderDetection {
        points: points.map(|point| {
            [
                (point.x as f32 / width_scale).clamp(0.0, 1.0),
                (point.y as f32 / height_scale).clamp(0.0, 1.0),
            ]
        }),
        confidence: DetectionConfidence::High,
        status,
    }
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

        assert_eq!(analysis.dimensions(), (320, 210));

        let result = detect_film_border(&dynamic);
        assert_eq!(result.confidence, DetectionConfidence::High);
        assert_eq!(result.status, "detected_gradient");
        assert!((result.points[0][0] - 48.0 / 319.0).abs() < 0.03);
        assert!((result.points[0][1] - 38.0 / 209.0).abs() < 0.03);
        assert!((result.points[2][0] - 274.0 / 319.0).abs() < 0.03);
        assert!((result.points[2][1] - 174.0 / 209.0).abs() < 0.03);
    }

    /// A synthetic scan: open light panel outside the film body, a uniform
    /// pale rebate band inside the film edge, and textured picture content
    /// inside the gate. The gradient detector lands on the film-body boundary;
    /// the gate refinement has to walk each edge in to the rebate's end.
    fn boundary_scan_thumbnail() -> DynamicImage {
        let mut image = image::RgbImage::from_pixel(400, 260, image::Rgb([244, 244, 244]));
        // Film body: rebate from (40, 30) to (360, 230).
        draw_filled_rect_mut(&mut image, Rect::at(40, 30).of_size(320, 200), image::Rgb([212, 172, 132]));
        // Picture gate: textured content from (60, 50) to (340, 210).
        for y in 50..210 {
            for x in 60..340 {
                let value = ((x * 5 + y * 9) % 97) as u8;
                image.put_pixel(x, y, image::Rgb([40 + value, 60 + value / 2, 80 + value / 3]));
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
        draw_filled_rect_mut(&mut image, Rect::at(60, 50).of_size(280, 160), image::Rgb([190, 150, 120]));
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

    /// A 135 strip at the gate's working size: light panel past the strip
    /// ends, orange rebate, a row of holes at a 56 px pitch along each edge,
    /// and a textured 425 x 283 picture (36 x 24 mm) between the rows.
    fn sprocket_strip() -> image::RgbImage {
        let mut image = image::RgbImage::from_pixel(640, 424, image::Rgb([245, 245, 245]));
        draw_filled_rect_mut(&mut image, Rect::at(40, 0).of_size(560, 424), image::Rgb([200, 140, 90]));
        for x in (60..580).step_by(56) {
            draw_filled_rect_mut(&mut image, Rect::at(x, 30).of_size(22, 32), image::Rgb([245, 245, 245]));
            draw_filled_rect_mut(&mut image, Rect::at(x, 362).of_size(22, 32), image::Rgb([245, 245, 245]));
        }
        for y in 72..355 {
            for x in 120..545 {
                let value = ((x * 7 + y * 3) % 61) as u8;
                image.put_pixel(x, y, image::Rgb([60 + value, 40 + value / 2, 30 + value / 3]));
            }
        }
        image
    }

    fn luma_of(image: &image::RgbImage) -> LumaPlane {
        LumaPlane {
            width: image.width() as usize,
            height: image.height() as usize,
            data: image
                .pixels()
                .map(|pixel| (pixel[0] as f32 + pixel[1] as f32 + pixel[2] as f32) / 3.0)
                .collect(),
        }
    }

    #[test]
    fn finds_the_sprocket_rows_of_a_135_strip() {
        let rows = sprocket_rows(&luma_of(&sprocket_strip()));

        assert_eq!(rows.len(), 2);
        assert!(rows.iter().all(|row| row.along_x));
        assert!((rows[0].centre - 46.0).abs() <= 2.0, "{}", rows[0].centre);
        assert!((rows[1].centre - 378.0).abs() <= 2.0, "{}", rows[1].centre);

        // The 120 strip has a picture and frame gaps but no holes.
        let mut strip = image::RgbImage::from_pixel(640, 424, image::Rgb([200, 140, 90]));
        draw_filled_rect_mut(&mut strip, Rect::at(130, 20).of_size(380, 384), image::Rgb([70, 50, 40]));
        assert!(sprocket_rows(&luma_of(&strip)).is_empty());
    }

    #[test]
    fn picture_boxes_stay_between_the_sprocket_rows() {
        let image = sprocket_strip();
        let rows = sprocket_rows(&luma_of(&image));
        let box_of = |x, y, width, height| GateCandidate {
            score: 0.0,
            x,
            y,
            width,
            height,
        };
        // Coarse (4 px) boxes: the picture, and the picture run down to the film edge.
        assert!(clear_of_sprockets(&box_of(30, 18, 106, 71), &rows, 4.0));
        assert!(!clear_of_sprockets(&box_of(30, 18, 106, 88), &rows, 4.0));

        for quad in ranked_gate_quads(&image) {
            let (top, bottom) = (quad[0][1].min(quad[1][1]), quad[2][1].max(quad[3][1]));
            assert!(top >= 46.0 && bottom <= 378.0, "{quad:?}");
        }
        let best = ranked_gate_quads(&image)[0];
        assert!((best[0][0] - 120.0).abs() <= 3.0 && (best[0][1] - 72.0).abs() <= 3.0, "{best:?}");
        assert!((best[2][0] - 545.0).abs() <= 3.0 && (best[2][1] - 355.0).abs() <= 3.0, "{best:?}");
    }

    #[test]
    fn verifier_patch_ignores_the_film_base_tint() {
        // The same frame on an orange and on a lavender base: the network
        // input differs only by rounding.
        let frame = |tint: [f32; 3]| {
            let mut image = image::RgbImage::from_pixel(200, 140, image::Rgb([0, 0, 0]));
            for (x, y, pixel) in image.enumerate_pixels_mut() {
                let value = if (40..160).contains(&x) && (30..110).contains(&y) {
                    60.0 + ((x * 7 + y * 3) % 50) as f32
                } else {
                    200.0
                };
                *pixel = image::Rgb(tint.map(|gain| (value * gain).round() as u8));
            }
            image
        };
        let quad = [[40.0, 30.0], [160.0, 30.0], [160.0, 110.0], [40.0, 110.0]];
        let orange = frame([1.0, 0.75, 0.55]);
        let lavender = frame([0.85, 0.75, 1.0]);
        for corner in 0..4 {
            let a = verifier_patch(&orange, &quad, corner);
            let b = verifier_patch(&lavender, &quad, corner);
            let worst = a.iter().zip(&b).map(|(a, b)| (a - b).abs()).fold(0.0, f32::max);
            assert!(worst < 0.05, "corner {corner}: {worst}");
        }
    }

    #[test]
    fn only_the_best_ranked_boxes_can_be_verified() {
        let quads: Vec<[[f64; 2]; 4]> = (0..4)
            .map(|rank| {
                let inset = rank as f64;
                [[inset, inset], [90.0, inset], [90.0, 60.0], [inset, 60.0]]
            })
            .collect();
        let rank_of = |quad: &[[f64; 2]; 4]| quad[0][0] as usize;
        let runner_up = first_verified(&quads, |quad| rank_of(quad) >= 1);
        assert_eq!(runner_up.map(rank_of), Some(1));
        assert!(first_verified(&quads, |quad| rank_of(quad) >= 2).is_none());
    }

    #[test]
    fn box_coverage_counts_partly_covered_cells() {
        // A box from 16 to 80 px across and 0 to 48 px down, on 32 px cells.
        let mask = box_coverage(&[16.0, 0.0, 80.0, 48.0], 32.0, 2, 3);
        assert_eq!(mask, vec![0.5, 1.0, 0.5, 0.25, 0.5, 0.25]);
    }

    #[test]
    fn max_pool_drops_an_odd_last_row_and_column() {
        let input: Vec<f32> = (0..15).map(|value| value as f32).collect();
        assert_eq!(max_pool2(&input, 1, 3, 5), vec![6.0, 8.0]);
    }

    #[test]
    fn box_scorer_charges_each_place_down_the_step_ranking() {
        // Two identical boxes get the same network score, so the later one
        // loses exactly the rank cost.
        let working = boundary_scan_thumbnail().resize_exact(640, 416, FilterType::Triangle).to_rgb8();
        let region = [80.0, 40.0, 560.0, 380.0];
        let geometry = [0.35, 0.6, 0.5];
        let preference = box_scorer_preference(&working, &[region, region], &[geometry, geometry]);
        assert!(preference.iter().all(|value| value.is_finite()));
        assert!((preference[0] - preference[1] - BOX_SCORER_RANK_COST).abs() < 1e-5);
        assert!((preference[0] + 2.0f32.ln()).abs() < 1e-5);
    }

    #[test]
    fn scored_choice_follows_the_scorer_beyond_its_resolution_only() {
        let quad = |left: f64, right: f64| Some([[left, 20.0], [right, 20.0], [right, 400.0], [left, 400.0]]);
        // Step order: an insane box, a box on a line inside the picture, the
        // full picture 120 px wider, and a near copy of the full picture.
        let quads = [None, quad(130.0, 390.0), quad(130.0, 510.0), quad(130.0, 498.0)];
        assert_eq!(scored_choice(&[5.0, -1.0, 0.0, -3.0], &quads), quads[2]);
        // The scorer prefers the near copy, which it cannot tell from the
        // higher-ranked full picture: the step search's box stays.
        assert_eq!(scored_choice(&[5.0, -1.0, -3.0, 0.0], &quads), quads[2]);
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

// ---------------------------------------------------------------------------
// Picture-area search with a film-format prior.
//
// A frame scanned from a strip has no light-panel border to lean on: on 120
// film the strip runs across the whole thumbnail and the picture is flanked by
// the clear base of the frame gaps and by slices of the neighbouring frames.
// The picture itself is a rectangle of a known film format, though, and each of
// its four edges is a step from bright base outside to denser picture inside.
// Searching boxes of the film formats' aspect ratios for the strongest four
// such steps finds the frame even when the picture holds straighter, stronger
// lines than its own frame. A small learned check then looks at the four
// corners of the result, and only a box that passes it is reported as High.

/// Long edge of the image the picture-area search works on.
const GATE_WORKING_EDGE: u32 = 640;
/// Block size of the coarse search image (640 -> 160 px).
const GATE_BLOCK: usize = 4;
/// Width in coarse pixels of the two means compared by the step filter.
const GATE_STEP_WIDTH: usize = 2;
/// Cap on a single step, so one very strong line cannot carry a whole side.
const GATE_STEP_CAP: f32 = 24.0;
/// Picture width range as a share of the coarse image width.
const GATE_MIN_WIDTH: f64 = 0.40;
const GATE_MAX_WIDTH: f64 = 0.95;
/// Long/short side ratios of 6x6, 6x7, 6x4.5 and 6x9 / 24x36.
const GATE_ASPECTS: [f64; 4] = [1.0, 1.25, 1.35, 1.5];
const GATE_ASPECT_TOLERANCES: [f64; 3] = [-0.04, 0.0, 0.04];
const GATE_SQUARE_TOLERANCES: [f64; 5] = [-0.03, -0.015, 0.0, 0.015, 0.03];
/// Placements kept per box size for the base-colour check, and how far apart.
const GATE_CANDIDATES_PER_SIZE: usize = 3;
const GATE_CANDIDATE_SPACING: usize = 6;
/// Boxes offered to the verifier (and to the box scorer when it accepts none),
/// best first, and how close two boxes may be in coarse pixels before the
/// weaker one is a duplicate.
const GATE_RANKED_BOXES: usize = 12;
const GATE_DUPLICATE_DISTANCE: usize = 4;
/// Bonus for the share of the image a box covers. A line inside the picture
/// gives a box smaller than the frame, sharing the frame's other sides.
const GATE_AREA_WEIGHT: f32 = 0.8;
/// Share of each side left out at both ends, where the corners blur the step.
const GATE_EDGE_TRIM: f64 = 0.1;
/// Samples along a side when it is snapped onto the step at full resolution.
const GATE_SNAP_SAMPLES: usize = 28;
const GATE_SNAP_BOX: i32 = 3;
const GATE_SNAP_MIN_STEP: f32 = 4.0;
/// Only the best ranked boxes may pass the verifier. The frame is the strongest
/// box, or the runner-up when a line in the picture outscores a faint edge;
/// every box further down that passed on the dev rolls was a coincidence on
/// picture-internal lines, and each extra box is one more chance for one.
const GATE_VERIFIED_BOXES: usize = 2;
/// Side of the square patch the corner verifier looks at.
const VERIFIER_PATCH: usize = 48;
/// A corner whose verifier probability is below this does not get High.
const VERIFIER_THRESHOLD: f32 = 0.3;
/// The bar for a gradient-search quad, which fails far more often on strips.
const GRADIENT_VERIFIER_THRESHOLD: f32 = 0.99;
/// A box whose combined side step is below this has found no picture at all.
const GATE_MIN_SCORE: f32 = 1.0;
/// Clearance kept from the quad when sampling its surround, as a fraction of the short side.
const SURROUND_MARGIN: f32 = 0.01;
const SURROUND_RING: f32 = 0.035;
/// The surround must cover this fraction of the image to say anything.
const SURROUND_MIN_AREA: f32 = 0.02;
/// Luma distance from the surround's median that still counts as plain background.
const SURROUND_MAX_DEVIATION: i32 = 10;
/// Fraction of the surround that has to read as plain background.
const SURROUND_MIN_MATCH: f32 = 0.97;
/// 135 film: the 24 mm picture height spans this many 4.75 mm sprocket pitches.
const SPROCKET_PITCHES_PER_PICTURE: f64 = 24.0 / 4.75;
/// A line through a hole row crosses at least this many holes at a regular pitch.
const SPROCKET_MIN_HOLES: usize = 5;
const SPROCKET_PITCH_TOLERANCE: f32 = 0.15;
/// Luma between the rebate and the light seen through a hole.
const SPROCKET_MIN_CONTRAST: f32 = 25.0;
/// A hole row is at least this many pitches deep across the strip (a hole is ~0.6).
const SPROCKET_MIN_ROW_DEPTH: f32 = 0.4;

struct LumaPlane {
    width: usize,
    height: usize,
    data: Vec<f32>,
}

impl LumaPlane {
    fn at(&self, x: usize, y: usize) -> f32 {
        self.data[y * self.width + x]
    }

    fn bilinear(&self, x: f64, y: f64) -> f64 {
        let x = x.clamp(0.0, self.width as f64 - 1.001);
        let y = y.clamp(0.0, self.height as f64 - 1.001);
        let x0 = x.floor() as usize;
        let y0 = y.floor() as usize;
        let fx = x - x0 as f64;
        let fy = y - y0 as f64;
        self.at(x0, y0) as f64 * (1.0 - fx) * (1.0 - fy)
            + self.at(x0 + 1, y0) as f64 * fx * (1.0 - fy)
            + self.at(x0, y0 + 1) as f64 * (1.0 - fx) * fy
            + self.at(x0 + 1, y0 + 1) as f64 * fx * fy
    }
}

/// Result of the picture-area search, in working-image pixels.
struct GateBox {
    quad: [[f64; 2]; 4],
    working_width: u32,
    working_height: u32,
}

/// Search the picture area: the first ranked box whose corners all pass the
/// verifier, or, with `false` when none does, the ranked box the box scorer
/// prefers.
fn detect_gate_box(thumbnail: &DynamicImage) -> Option<(GateBox, bool)> {
    let working = gate_working_rgb(thumbnail)?;
    let (width, height) = working.dimensions();
    let search = GateSearch::new(&working, GATE_RANKED_BOXES)?;
    let snapped: Vec<_> = search.offered().iter().map(|candidate| search.snapped(candidate)).collect();
    let quads: Vec<_> = snapped.iter().flatten().copied().collect();
    let verified = first_verified(&quads, |quad| {
        (0..4).all(|corner| {
            verifier_forward(&verifier_patch(&working, quad, corner)) >= VERIFIER_THRESHOLD
        })
    });
    let accepted = verified.is_some();
    let quad = match verified {
        Some(quad) => *quad,
        None => scored_choice(&search.preferences(&working), &snapped)?,
    };
    Some((
        GateBox {
            quad,
            working_width: width,
            working_height: height,
        },
        accepted,
    ))
}

/// The first of the best `GATE_VERIFIED_BOXES` ranked quads that `accepts`.
fn first_verified(
    quads: &[[[f64; 2]; 4]],
    accepts: impl Fn(&[[f64; 2]; 4]) -> bool,
) -> Option<&[[f64; 2]; 4]> {
    quads.iter().take(GATE_VERIFIED_BOXES).find(|quad| accepts(quad))
}

/// The format boxes of one working image, best step score first.
struct GateSearch {
    luma: LumaPlane,
    coarse_width: usize,
    coarse_height: usize,
    ranked: Vec<GateCandidate>,
}

impl GateSearch {
    /// Search and keep the best `count` distinct boxes.
    fn new(working: &image::RgbImage, count: usize) -> Option<Self> {
        let (width, height) = working.dimensions();
        let luma = LumaPlane {
            width: width as usize,
            height: height as usize,
            data: working
                .pixels()
                .map(|pixel| (pixel[0] as f32 + pixel[1] as f32 + pixel[2] as f32) / 3.0)
                .collect(),
        };
        let coarse = block_mean(&luma, GATE_BLOCK);
        if coarse.width < 32 || coarse.height < 24 {
            return None;
        }
        let mut candidates = format_box_candidates(&coarse);
        let sprockets = sprocket_rows(&luma);
        if !sprockets.is_empty() {
            let clear: Vec<GateCandidate> = candidates
                .iter()
                .copied()
                .filter(|candidate| clear_of_sprockets(candidate, &sprockets, GATE_BLOCK as f64))
                .collect();
            if !clear.is_empty() {
                candidates = clear;
            }
        }
        let ranked = rank_candidates(coarse.width, coarse.height, candidates, count);
        Some(Self {
            luma,
            coarse_width: coarse.width,
            coarse_height: coarse.height,
            ranked,
        })
    }

    /// A candidate snapped onto the steps at working resolution, if sane.
    fn snapped(&self, candidate: &GateCandidate) -> Option<[[f64; 2]; 4]> {
        let scale = GATE_BLOCK as f64;
        let (left, top) = (candidate.x as f64 * scale, candidate.y as f64 * scale);
        let (right, bottom) = (
            (candidate.x + candidate.width) as f64 * scale,
            (candidate.y + candidate.height) as f64 * scale,
        );
        snap_quad_to_steps(
            &self.luma,
            [[left, top], [right, top], [right, bottom], [left, bottom]],
            scale + 3.0,
        )
        .filter(|quad| gate_quad_is_sane(quad, self.luma.width as f64, self.luma.height as f64))
    }

    /// The boxes offered to the verifier.
    fn offered(&self) -> &[GateCandidate] {
        &self.ranked[..self.ranked.len().min(GATE_RANKED_BOXES)]
    }

    /// The box scorer's preference for each box offered to the verifier.
    fn preferences(&self, working: &image::RgbImage) -> Vec<f32> {
        let boxes = self.offered();
        let geometry: Vec<[f32; 3]> = boxes
            .iter()
            .map(|candidate| {
                [
                    (candidate.width as f32 / candidate.height as f32).ln().abs(),
                    (candidate.width * candidate.height) as f32
                        / (self.coarse_width * self.coarse_height) as f32,
                    candidate.score / (1.5 * GATE_STEP_CAP),
                ]
            })
            .collect();
        let regions: Vec<[f32; 4]> = boxes
            .iter()
            .map(|candidate| {
                let scale = GATE_BLOCK as f32;
                [
                    candidate.x as f32 * scale,
                    candidate.y as f32 * scale,
                    (candidate.x + candidate.width) as f32 * scale,
                    (candidate.y + candidate.height) as f32 * scale,
                ]
            })
            .collect();
        box_scorer_preference(working, &regions, &geometry)
    }
}

/// The first sane box in order of preference. A box ranked higher by the step
/// search that lies within one scorer cell of it is kept instead: the scorer
/// cannot tell such boxes apart, the steps can.
fn scored_choice(preference: &[f32], quads: &[Option<[[f64; 2]; 4]>]) -> Option<[[f64; 2]; 4]> {
    let mut order: Vec<usize> = (0..preference.len()).collect();
    order.sort_by(|a, b| preference[*b].total_cmp(&preference[*a]));
    let chosen = order.into_iter().find_map(|index| quads[index])?;
    let near = |quad: &[[f64; 2]; 4]| {
        (0..4).all(|corner| {
            let (a, b) = (quad[corner], chosen[corner]);
            (a[0] - b[0]).hypot(a[1] - b[1]) <= BOX_SCORER_CELL as f64
        })
    };
    quads.iter().flatten().copied().find(near)
}

/// The snapped, sane quads of the ranked format boxes, best first.
fn ranked_gate_quads(working: &image::RgbImage) -> Vec<[[f64; 2]; 4]> {
    GateSearch::new(working, GATE_RANKED_BOXES).map_or_else(Vec::new, |search| {
        search.offered().iter().filter_map(|candidate| search.snapped(candidate)).collect()
    })
}

/// A row of sprocket holes: its centre line across the strip, in working
/// pixels, and whether the strip runs along x.
struct SprocketRow {
    along_x: bool,
    centre: f64,
}

/// The sprocket-hole rows of a 135 strip. A line through a hole row crosses a
/// regular train of bright holes; enough consecutive such lines at one pitch
/// make a row. Picture content does not repeat that evenly over that depth.
fn sprocket_rows(luma: &LumaPlane) -> Vec<SprocketRow> {
    // The smallest picture the box search allows still spans 5 pitches.
    let min_pitch = 0.25 * luma.width.min(luma.height) as f64 / SPROCKET_PITCHES_PER_PICTURE;
    let mut rows = Vec::new();
    for along_x in [true, false] {
        let (lines, length) = if along_x {
            (luma.height, luma.width)
        } else {
            (luma.width, luma.height)
        };
        let pitches: Vec<Option<f32>> = (0..lines)
            .map(|line| {
                let profile: Vec<f32> = (0..length)
                    .map(|i| if along_x { luma.at(i, line) } else { luma.at(line, i) })
                    .collect();
                hole_pitch(&profile).filter(|pitch| *pitch as f64 >= min_pitch)
            })
            .collect();
        let mut line = 0;
        while line < lines {
            let Some(pitch) = pitches[line] else {
                line += 1;
                continue;
            };
            let start = line;
            while line < lines
                && pitches[line].map_or(false, |other| (other - pitch).abs() <= 0.05 * pitch)
            {
                line += 1;
            }
            if (line - start) as f32 >= SPROCKET_MIN_ROW_DEPTH * pitch {
                rows.push(SprocketRow {
                    along_x,
                    centre: (start + line) as f64 / 2.0,
                });
            }
        }
    }
    rows
}

/// The pitch of the holes along one line, if it crosses a regular train of
/// bright holes on a darker rebate.
fn hole_pitch(profile: &[f32]) -> Option<f32> {
    let mut values = profile.to_vec();
    let low = *values.select_nth_unstable_by(profile.len() / 4, f32::total_cmp).1;
    let high = *values.select_nth_unstable_by(profile.len() * 9 / 10, f32::total_cmp).1;
    if high - low < SPROCKET_MIN_CONTRAST {
        return None;
    }
    let threshold = (low + high) / 2.0;
    // Bright runs that do not touch either end: a hole cut off by the image
    // edge, or open light panel, has no reliable width.
    let mut runs: Vec<(f32, f32)> = Vec::new();
    let mut start = None;
    for (index, value) in profile.iter().enumerate() {
        match (start, *value > threshold) {
            (None, true) => start = Some(index),
            (Some(first), false) => {
                if first > 0 {
                    runs.push(((first + index) as f32 / 2.0, (index - first) as f32));
                }
                start = None;
            }
            _ => {}
        }
    }
    if runs.len() < SPROCKET_MIN_HOLES {
        return None;
    }
    let mut gaps: Vec<f32> = runs.windows(2).map(|pair| pair[1].0 - pair[0].0).collect();
    let mut widths: Vec<f32> = runs.iter().map(|run| run.1).collect();
    gaps.sort_unstable_by(f32::total_cmp);
    widths.sort_unstable_by(f32::total_cmp);
    let pitch = gaps[gaps.len() / 2];
    let regular = gaps
        .iter()
        .filter(|gap| (**gap - pitch).abs() <= SPROCKET_PITCH_TOLERANCE * pitch)
        .count();
    let duty = widths[widths.len() / 2] / pitch;
    (regular + 1 >= SPROCKET_MIN_HOLES
        && pitch <= 0.2 * profile.len() as f32
        && (0.2..=0.7).contains(&duty))
        .then_some(pitch)
}

/// A picture lies between the hole rows: a box whose side reaches past the
/// middle of a row has holes or rebate inside it.
fn clear_of_sprockets(candidate: &GateCandidate, rows: &[SprocketRow], scale: f64) -> bool {
    rows.iter().all(|row| {
        let (start, size) = if row.along_x {
            (candidate.y, candidate.height)
        } else {
            (candidate.x, candidate.width)
        };
        let (near, far) = (start as f64 * scale, (start + size) as f64 * scale);
        row.centre <= near || row.centre >= far
    })
}

/// Every ranked box as normalised points, for building training negatives.
pub fn film_border_ranked_quads(thumbnail: &DynamicImage) -> Vec<[[f32; 2]; 4]> {
    let Some(working) = gate_working_rgb(thumbnail) else {
        return Vec::new();
    };
    let (width, height) = working.dimensions();
    ranked_gate_quads(&working)
        .iter()
        .map(|quad| {
            quad.map(|point| [(point[0] / width as f64) as f32, (point[1] / height as f64) as f32])
        })
        .collect()
}

/// The first `count` ranked boxes before snapping, as coarse `[x, y, width,
/// height]` with their step score, each with its snapped quad in normalised
/// points when it is sane; and the box scorer's preference for the boxes
/// offered to the verifier.
#[doc(hidden)]
pub fn film_border_box_pool(
    thumbnail: &DynamicImage,
    count: usize,
) -> (Vec<([usize; 4], f32, Option<[[f32; 2]; 4]>)>, Vec<f32>) {
    let Some(working) = gate_working_rgb(thumbnail) else {
        return (Vec::new(), Vec::new());
    };
    let (width, height) = working.dimensions();
    let Some(search) = GateSearch::new(&working, count.max(GATE_RANKED_BOXES)) else {
        return (Vec::new(), Vec::new());
    };
    let boxes = search.ranked[..search.ranked.len().min(count)]
        .iter()
        .map(|candidate| {
            let quad = search.snapped(candidate).map(|quad| {
                quad.map(|point| [(point[0] / width as f64) as f32, (point[1] / height as f64) as f32])
            });
            ([candidate.x, candidate.y, candidate.width, candidate.height], candidate.score, quad)
        })
        .collect();
    (boxes, search.preferences(&working))
}

fn gate_working_rgb(thumbnail: &DynamicImage) -> Option<image::RgbImage> {
    let (width, height) = thumbnail.dimensions();
    let longest = width.max(height);
    if longest < 64 {
        return None;
    }
    if longest == GATE_WORKING_EDGE {
        return Some(thumbnail.to_rgb8());
    }
    let scale = GATE_WORKING_EDGE as f64 / longest as f64;
    Some(
        thumbnail
            .resize_exact(
                ((width as f64 * scale).round() as u32).max(1),
                ((height as f64 * scale).round() as u32).max(1),
                image::imageops::FilterType::Triangle,
            )
            .to_rgb8(),
    )
}

fn block_mean(plane: &LumaPlane, block: usize) -> LumaPlane {
    let width = plane.width / block;
    let height = plane.height / block;
    let mut data = vec![0.0f32; width * height];
    for y in 0..height {
        for x in 0..width {
            let mut sum = 0.0f32;
            for dy in 0..block {
                for dx in 0..block {
                    sum += plane.at(x * block + dx, y * block + dy);
                }
            }
            data[y * width + x] = sum / (block * block) as f32;
        }
    }
    LumaPlane {
        width,
        height,
        data,
    }
}

/// Bright-outside / dark-inside step maps for the four sides of a box,
/// accumulated along the side so that any side segment is two lookups.
struct StepSums {
    width: usize,
    /// (height + 1) x width, cumulative down each column.
    left: Vec<f32>,
    right: Vec<f32>,
    /// height x (width + 1), cumulative along each row.
    top: Vec<f32>,
    bottom: Vec<f32>,
}

fn step_sums(plane: &LumaPlane) -> StepSums {
    let (width, height) = (plane.width, plane.height);
    let k = GATE_STEP_WIDTH;
    let mut left = vec![0.0f32; (height + 1) * width];
    let mut right = vec![0.0f32; (height + 1) * width];
    let mut top = vec![0.0f32; height * (width + 1)];
    let mut bottom = vec![0.0f32; height * (width + 1)];

    let mut row_steps = vec![(0.0f32, 0.0f32); width * height];
    for y in 0..height {
        let mut prefix = vec![0.0f32; width + 1];
        for x in 0..width {
            prefix[x + 1] = prefix[x] + plane.at(x, y);
        }
        for x in k..=width - k {
            let outside = (prefix[x] - prefix[x - k]) / k as f32;
            let inside = (prefix[x + k] - prefix[x]) / k as f32;
            if x < width {
                row_steps[y * width + x] = (
                    (outside - inside).clamp(0.0, GATE_STEP_CAP),
                    (inside - outside).clamp(0.0, GATE_STEP_CAP),
                );
            }
        }
    }
    for y in 0..height {
        for x in 0..width {
            let (l, r) = row_steps[y * width + x];
            left[(y + 1) * width + x] = left[y * width + x] + l;
            right[(y + 1) * width + x] = right[y * width + x] + r;
        }
    }

    let mut column_steps = vec![(0.0f32, 0.0f32); width * height];
    for x in 0..width {
        let mut prefix = vec![0.0f32; height + 1];
        for y in 0..height {
            prefix[y + 1] = prefix[y] + plane.at(x, y);
        }
        for y in k..=height - k {
            let outside = (prefix[y] - prefix[y - k]) / k as f32;
            let inside = (prefix[y + k] - prefix[y]) / k as f32;
            if y < height {
                column_steps[y * width + x] = (
                    (outside - inside).clamp(0.0, GATE_STEP_CAP),
                    (inside - outside).clamp(0.0, GATE_STEP_CAP),
                );
            }
        }
    }
    for y in 0..height {
        for x in 0..width {
            let (t, b) = column_steps[y * width + x];
            top[y * (width + 1) + x + 1] = top[y * (width + 1) + x] + t;
            bottom[y * (width + 1) + x + 1] = bottom[y * (width + 1) + x] + b;
        }
    }

    StepSums {
        width,
        left,
        right,
        top,
        bottom,
    }
}

/// Every box size the film formats allow inside an image of this size.
fn format_box_sizes(width: usize, height: usize) -> Vec<(usize, usize)> {
    let mut sizes = std::collections::BTreeSet::new();
    let min_width = (GATE_MIN_WIDTH * width as f64) as usize;
    let max_width = (GATE_MAX_WIDTH * width as f64) as usize;
    for aspect in GATE_ASPECTS {
        for landscape in [true, false] {
            let square = aspect == 1.0;
            if square && !landscape {
                continue;
            }
            let tolerances: &[f64] = if square {
                &GATE_SQUARE_TOLERANCES
            } else {
                &GATE_ASPECT_TOLERANCES
            };
            for box_width in min_width..=max_width {
                for tolerance in tolerances {
                    let ratio = aspect * (1.0 + tolerance);
                    let box_height = if landscape {
                        box_width as f64 / ratio
                    } else {
                        box_width as f64 * ratio
                    }
                    .round() as usize;
                    if box_height as f64 >= 0.25 * height as f64
                        && box_height + 2 <= height
                        && box_width + 2 <= width
                    {
                        sizes.insert((box_width, box_height));
                    }
                }
            }
        }
    }
    sizes.into_iter().collect()
}

/// A box found by the step search, with its step score.
#[derive(Clone, Copy)]
struct GateCandidate {
    score: f32,
    x: usize,
    y: usize,
    width: usize,
    height: usize,
}

/// The best few placements of every film-format size, scored by the
/// bright-outside steps of their sides: the weakest side counts in full, the
/// others a quarter, so a box cannot win on three good sides alone.
fn format_box_candidates(plane: &LumaPlane) -> Vec<GateCandidate> {
    let sums = step_sums(plane);
    let (width, height) = (plane.width, plane.height);
    let column = |values: &Vec<f32>, row: usize, x: usize| values[row * width + x];
    let row_sum = |values: &Vec<f32>, y: usize, x: usize| values[y * (sums.width + 1) + x];

    let mut candidates = Vec::new();
    for (box_width, box_height) in format_box_sizes(width, height) {
        let trim_y = (GATE_EDGE_TRIM * box_height as f64) as usize;
        let trim_x = (GATE_EDGE_TRIM * box_width as f64) as usize;
        let span_y = (box_height - 2 * trim_y) as f32;
        let span_x = (box_width - 2 * trim_x) as f32;
        let mut best_of_size: Vec<GateCandidate> = Vec::with_capacity(GATE_CANDIDATES_PER_SIZE + 1);
        for y in 1..height - box_height {
            for x in 1..width - box_width {
                let left = (column(&sums.left, y + box_height - trim_y, x)
                    - column(&sums.left, y + trim_y, x))
                    / span_y;
                let right = (column(&sums.right, y + box_height - trim_y, x + box_width)
                    - column(&sums.right, y + trim_y, x + box_width))
                    / span_y;
                let top = (row_sum(&sums.top, y, x + box_width - trim_x)
                    - row_sum(&sums.top, y, x + trim_x))
                    / span_x;
                let bottom = (row_sum(&sums.bottom, y + box_height, x + box_width - trim_x)
                    - row_sum(&sums.bottom, y + box_height, x + trim_x))
                    / span_x;
                let score = left.min(right).min(top.min(bottom))
                    + 0.25 * (left + right + top + bottom);
                if score < GATE_MIN_SCORE {
                    continue;
                }
                let candidate = GateCandidate {
                    score,
                    x,
                    y,
                    width: box_width,
                    height: box_height,
                };
                keep_best_placements(&mut best_of_size, candidate);
            }
        }
        candidates.extend(best_of_size);
    }
    candidates
}

/// Keep the highest-scoring placements of one size, at most one per
/// neighbourhood, so a single strong box does not fill all the places.
fn keep_best_placements(best: &mut Vec<GateCandidate>, candidate: GateCandidate) {
    if let Some(near) = best.iter_mut().find(|other| {
        other.x.abs_diff(candidate.x) + other.y.abs_diff(candidate.y) <= GATE_CANDIDATE_SPACING
    }) {
        if candidate.score > near.score {
            *near = candidate;
        }
        return;
    }
    best.push(candidate);
    if best.len() > GATE_CANDIDATES_PER_SIZE {
        let weakest = best
            .iter()
            .enumerate()
            .min_by(|a, b| a.1.score.total_cmp(&b.1.score))
            .map_or(0, |(index, _)| index);
        best.swap_remove(weakest);
    }
}

/// The candidates in order of preference: step score plus a bonus for the
/// larger box, since a picture-internal line gives a box smaller than the
/// frame. Near-duplicates of a better box are dropped.
fn rank_candidates(
    image_width: usize,
    image_height: usize,
    candidates: Vec<GateCandidate>,
    count: usize,
) -> Vec<GateCandidate> {
    let image_area = (image_width * image_height) as f32;
    let mut scored: Vec<(f32, GateCandidate)> = candidates
        .into_iter()
        .map(|candidate| {
            let area = (candidate.width * candidate.height) as f32 / image_area;
            (candidate.score / (1.5 * GATE_STEP_CAP) + GATE_AREA_WEIGHT * area, candidate)
        })
        .collect();
    scored.sort_by(|a, b| b.0.total_cmp(&a.0));
    let mut ranked: Vec<GateCandidate> = Vec::new();
    for (_, candidate) in scored {
        let duplicate = ranked.iter().any(|other| {
            other.x.abs_diff(candidate.x) <= GATE_DUPLICATE_DISTANCE
                && other.y.abs_diff(candidate.y) <= GATE_DUPLICATE_DISTANCE
                && (other.x + other.width).abs_diff(candidate.x + candidate.width)
                    <= GATE_DUPLICATE_DISTANCE
                && (other.y + other.height).abs_diff(candidate.y + candidate.height)
                    <= GATE_DUPLICATE_DISTANCE
        });
        if !duplicate {
            ranked.push(candidate);
            if ranked.len() >= count {
                break;
            }
        }
    }
    ranked
}

/// Move each side of `quad` onto the strongest bright-outside / dark-inside
/// step within `radius` pixels, fitting a line through the steps found along
/// the side and ignoring the ones that disagree with it.
fn snap_quad_to_steps(
    plane: &LumaPlane,
    quad: [[f64; 2]; 4],
    radius: f64,
) -> Option<[[f64; 2]; 4]> {
    let radius = radius as i32;
    let centre = [
        quad.iter().map(|point| point[0]).sum::<f64>() / 4.0,
        quad.iter().map(|point| point[1]).sum::<f64>() / 4.0,
    ];
    let mut lines = [([0.0f64; 2], [0.0f64; 2]); 4];
    for side in 0..4 {
        let start = quad[side];
        let end = quad[(side + 1) % 4];
        let direction = [end[0] - start[0], end[1] - start[1]];
        let length = direction[0].hypot(direction[1]);
        if length < 4.0 {
            return None;
        }
        let unit = [direction[0] / length, direction[1] / length];
        let mut normal = [-unit[1], unit[0]];
        let middle = [start[0] + direction[0] / 2.0, start[1] + direction[1] / 2.0];
        if (centre[0] - middle[0]) * normal[0] + (centre[1] - middle[1]) * normal[1] < 0.0 {
            normal = [-normal[0], -normal[1]];
        }

        let offsets: Vec<i32> = (-radius - GATE_SNAP_BOX..=radius + GATE_SNAP_BOX).collect();
        let mut found: Vec<(f64, f64)> = Vec::new();
        for sample in 0..GATE_SNAP_SAMPLES {
            let t = 0.1 + 0.8 * sample as f64 / (GATE_SNAP_SAMPLES - 1) as f64;
            let base = [start[0] + t * direction[0], start[1] + t * direction[1]];
            let profile: Vec<f64> = offsets
                .iter()
                .map(|&offset| {
                    [-3.0, -1.5, 0.0, 1.5, 3.0]
                        .iter()
                        .map(|tangent| {
                            plane.bilinear(
                                base[0] + tangent * unit[0] + offset as f64 * normal[0],
                                base[1] + tangent * unit[1] + offset as f64 * normal[1],
                            )
                        })
                        .sum::<f64>()
                        / 5.0
                })
                .collect();
            let window = GATE_SNAP_BOX as usize;
            let mut best: Option<(f64, i32)> = None;
            for index in window..offsets.len() - window {
                if offsets[index].abs() > radius {
                    continue;
                }
                let outside = profile[index - window..index].iter().sum::<f64>() / window as f64;
                let inside =
                    profile[index + 1..index + 1 + window].iter().sum::<f64>() / window as f64;
                let step = outside - inside;
                if best.map_or(true, |current| step > current.0) {
                    best = Some((step, offsets[index]));
                }
            }
            if let Some((step, offset)) = best {
                if step >= GATE_SNAP_MIN_STEP as f64 {
                    found.push((t * length, offset as f64));
                }
            }
        }

        let (slope, intercept) = if found.len() < 4 {
            (0.0, -0.5)
        } else {
            robust_line(&found)
        };
        let near = intercept + 0.5;
        let far = slope * length + intercept + 0.5;
        let a = [start[0] + near * normal[0], start[1] + near * normal[1]];
        let b = [
            end[0] + far * normal[0],
            end[1] + far * normal[1],
        ];
        lines[side] = (a, [b[0] - a[0], b[1] - a[1]]);
    }

    let mut corners = [[0.0f64; 2]; 4];
    for corner in 0..4 {
        let (p1, d1) = lines[(corner + 3) % 4];
        let (p2, d2) = lines[corner];
        let det = d1[0] * -d2[1] - -d2[0] * d1[1];
        if det.abs() < 1e-9 {
            return None;
        }
        let rx = p2[0] - p1[0];
        let ry = p2[1] - p1[1];
        let t = (rx * -d2[1] - -d2[0] * ry) / det;
        corners[corner] = [p1[0] + t * d1[0], p1[1] + t * d1[1]];
    }
    Some(corners)
}

/// Least-squares line through (position, offset) samples, refitted three times
/// without the samples that lie further than 2.5 median deviations off it.
fn robust_line(samples: &[(f64, f64)]) -> (f64, f64) {
    let fit = |keep: &[bool]| -> (f64, f64) {
        let chosen: Vec<&(f64, f64)> = samples
            .iter()
            .zip(keep)
            .filter(|(_, keep)| **keep)
            .map(|(sample, _)| sample)
            .collect();
        let count = chosen.len() as f64;
        let mean_x = chosen.iter().map(|sample| sample.0).sum::<f64>() / count;
        let mean_y = chosen.iter().map(|sample| sample.1).sum::<f64>() / count;
        let sxx = chosen
            .iter()
            .map(|sample| (sample.0 - mean_x).powi(2))
            .sum::<f64>();
        let sxy = chosen
            .iter()
            .map(|sample| (sample.0 - mean_x) * (sample.1 - mean_y))
            .sum::<f64>();
        let slope = if sxx > 0.0 { sxy / sxx } else { 0.0 };
        (slope, mean_y - slope * mean_x)
    };

    let mut keep = vec![true; samples.len()];
    for _ in 0..3 {
        let (slope, intercept) = fit(&keep);
        let residuals: Vec<f64> = samples
            .iter()
            .map(|sample| sample.1 - (slope * sample.0 + intercept))
            .collect();
        let mut kept: Vec<f64> = residuals
            .iter()
            .zip(&keep)
            .filter(|(_, keep)| **keep)
            .map(|(residual, _)| residual.abs())
            .collect();
        let deviation = median_f64(&mut kept) + 0.5;
        let limit = (2.5 * deviation).max(1.5);
        let next: Vec<bool> = residuals.iter().map(|residual| residual.abs() <= limit).collect();
        if next.iter().filter(|keep| **keep).count() < 4 {
            keep = vec![true; samples.len()];
            break;
        }
        keep = next;
    }
    fit(&keep)
}

/// A picture area must be a convex quad inside the image.
fn gate_quad_is_sane(quad: &[[f64; 2]; 4], width: f64, height: f64) -> bool {
    if quad
        .iter()
        .any(|point| point[0] < -1.0 || point[1] < -1.0 || point[0] > width + 1.0 || point[1] > height + 1.0)
    {
        return false;
    }
    (0..4).all(|corner| {
        let a = quad[corner];
        let b = quad[(corner + 1) % 4];
        let c = quad[(corner + 2) % 4];
        (b[0] - a[0]) * (c[1] - b[1]) - (b[1] - a[1]) * (c[0] - b[0]) > 0.0
    })
}

fn gate_detection(gate: &GateBox, confidence: DetectionConfidence) -> FilmBorderDetection {
    let width = gate.working_width as f64;
    let height = gate.working_height as f64;
    FilmBorderDetection {
        points: gate.quad.map(|point| {
            [
                ((point[0] / width) as f32).clamp(0.0, 1.0),
                ((point[1] / height) as f32).clamp(0.0, 1.0),
            ]
        }),
        confidence,
        status: "detected_gate",
    }
}

// ---------------------------------------------------------------------------
// Corner verifier: a three-layer convolutional network that looks at a 48 px
// patch around each corner of a candidate picture area and says whether the
// corner sits on the picture/base boundary. Trained with
// tools/film_gate_model/train.py; the weights are generated by its export step.

include!("film_border_verifier_weights.rs");

/// Probability per corner [TL, TR, BR, BL] that it sits on the picture corner.
fn verify_corners(image: &image::RgbImage, quad: &[[f64; 2]; 4]) -> [f32; 4] {
    verify_corners_with_polarity(image, quad, false)
}

fn verify_corners_with_polarity(
    image: &image::RgbImage,
    quad: &[[f64; 2]; 4],
    inverted: bool,
) -> [f32; 4] {
    let mut out = [0.0f32; 4];
    for corner in 0..4 {
        let mut patch = verifier_patch(image, quad, corner);
        if inverted {
            let colour = 3 * VERIFIER_PATCH * VERIFIER_PATCH;
            patch[..colour].iter_mut().for_each(|value| *value = -*value);
        }
        out[corner] = verifier_forward(&patch);
    }
    out
}

/// The 4 x 48 x 48 network input for one corner: the RGB patch white-balanced
/// on the base outside the quad and normalised by its own mean and spread, and
/// a mask of the quad interior, mirrored so the interior always lies to the
/// bottom right.
fn verifier_patch(image: &image::RgbImage, quad: &[[f64; 2]; 4], corner: usize) -> Vec<f32> {
    let size = VERIFIER_PATCH;
    let (width, height) = image.dimensions();
    let half = (size / 2) as i64;
    let origin_x = quad[corner][0].floor() as i64 - half;
    let origin_y = quad[corner][1].floor() as i64 - half;

    let mut rgb = vec![0.0f32; 3 * size * size];
    let mut inside = vec![false; size * size];
    for row in 0..size {
        let y = (origin_y + row as i64).clamp(0, height as i64 - 1) as u32;
        for col in 0..size {
            let x = (origin_x + col as i64).clamp(0, width as i64 - 1) as u32;
            let pixel = image.get_pixel(x, y);
            for channel in 0..3 {
                rgb[channel * size * size + row * size + col] = pixel[channel] as f32;
            }
            let gx = (origin_x + col as i64) as f64 + 0.5;
            let gy = (origin_y + row as i64) as f64 + 0.5;
            inside[row * size + col] = (0..4).all(|side| {
                let p = quad[side];
                let q = quad[(side + 1) % 4];
                (q[0] - p[0]) * (gy - p[1]) - (q[1] - p[1]) * (gx - p[0]) >= 0.0
            });
        }
    }
    // White-balance on the base: scale the channels so the mean colour outside
    // the quad turns grey. The film stock's tint (orange, lavender, clear)
    // then drops out while colour differences inside the patch remain.
    let outside = inside.iter().filter(|inside| !**inside).count();
    if outside > 0 {
        let base: Vec<f64> = (0..3)
            .map(|channel| {
                let plane = &rgb[channel * size * size..(channel + 1) * size * size];
                let sum = plane
                    .iter()
                    .zip(&inside)
                    .filter(|(_, inside)| !**inside)
                    .map(|(value, _)| *value as f64)
                    .sum::<f64>();
                (sum / outside as f64).max(1.0)
            })
            .collect();
        let grey = base.iter().sum::<f64>() / 3.0;
        for channel in 0..3 {
            let gain = (grey / base[channel]) as f32;
            rgb[channel * size * size..(channel + 1) * size * size]
                .iter_mut()
                .for_each(|value| *value *= gain);
        }
    }
    let count = rgb.len() as f64;
    let mean = rgb.iter().map(|value| *value as f64).sum::<f64>() / count;
    let variance = rgb
        .iter()
        .map(|value| (*value as f64 - mean).powi(2))
        .sum::<f64>()
        / count;
    let spread = (variance.sqrt() + 8.0) as f32;
    let mean = mean as f32;

    let mut input = vec![0.0f32; 4 * size * size];
    for row in 0..size {
        for col in 0..size {
            let source_row = if corner >= 2 { size - 1 - row } else { row };
            let source_col = if corner == 1 || corner == 2 {
                size - 1 - col
            } else {
                col
            };
            for channel in 0..3 {
                input[channel * size * size + row * size + col] =
                    (rgb[channel * size * size + source_row * size + source_col] - mean) / spread;
            }
            input[3 * size * size + row * size + col] = if inside[source_row * size + source_col] {
                1.0
            } else {
                0.0
            };
        }
    }
    input
}

fn verifier_forward(input: &[f32]) -> f32 {
    let size = VERIFIER_PATCH;
    let layer1 = conv3x3_relu_pool(input, 4, size, &VERIFIER_C1_W, &VERIFIER_C1_B);
    let layer2 = conv3x3_relu_pool(&layer1, 8, size / 2, &VERIFIER_C2_W, &VERIFIER_C2_B);
    let layer3 = conv3x3_relu_pool(&layer2, 16, size / 4, &VERIFIER_C3_W, &VERIFIER_C3_B);
    let logit = layer3
        .iter()
        .zip(VERIFIER_FC_W.iter())
        .map(|(value, weight)| value * weight)
        .sum::<f32>()
        + VERIFIER_FC_B;
    1.0 / (1.0 + (-logit).exp())
}

/// 3x3 convolution with zero padding, ReLU and 2x2 max pooling, in the memory
/// layout PyTorch uses (channel, row, column).
fn conv3x3_relu_pool(
    input: &[f32],
    channels: usize,
    size: usize,
    weights: &[f32],
    bias: &[f32],
) -> Vec<f32> {
    let outputs = bias.len();
    let mut convolved = vec![0.0f32; outputs * size * size];
    for (output, plane) in convolved.chunks_exact_mut(size * size).enumerate() {
        plane.fill(bias[output]);
        for channel in 0..channels {
            let source = &input[channel * size * size..(channel + 1) * size * size];
            for ky in 0..3usize {
                for kx in 0..3usize {
                    let weight = weights[((output * channels + channel) * 3 + ky) * 3 + kx];
                    let row_start = 1usize.saturating_sub(ky);
                    let row_end = (size + 1 - ky).min(size);
                    let col_start = 1usize.saturating_sub(kx);
                    let col_end = (size + 1 - kx).min(size);
                    for row in row_start..row_end {
                        let y = row + ky - 1;
                        let target = &mut plane[row * size + col_start..row * size + col_end];
                        let from = &source[y * size + col_start + kx - 1..];
                        for (value, sample) in target.iter_mut().zip(from) {
                            *value += weight * sample;
                        }
                    }
                }
            }
        }
        plane.iter_mut().for_each(|value| *value = value.max(0.0));
    }
    let pooled_size = size / 2;
    let mut pooled = vec![0.0f32; outputs * pooled_size * pooled_size];
    for output in 0..outputs {
        for row in 0..pooled_size {
            for col in 0..pooled_size {
                let at = |y: usize, x: usize| convolved[output * size * size + y * size + x];
                pooled[output * pooled_size * pooled_size + row * pooled_size + col] = at(2 * row, 2 * col)
                    .max(at(2 * row, 2 * col + 1))
                    .max(at(2 * row + 1, 2 * col))
                    .max(at(2 * row + 1, 2 * col + 1));
            }
        }
    }
    pooled
}

// ---------------------------------------------------------------------------
// Box scorer: when the verifier vouches for none of the ranked boxes, the box
// returned with Low confidence is the one a small ensemble of networks prefers.
// They see the whole frame at 1/16 scale, with the box's coverage joined after
// the first layer, so they can weigh where the gaps and rebates run against a
// strong line inside a pale picture, which the step search alone prefers.
// Trained with tools/film_gate_model/train.py box_scorer.

/// Working pixels per cell of the scorer's image.
const BOX_SCORER_CELL: usize = 16;
/// Log-probability a box gives up per place it sits lower in the step ranking.
const BOX_SCORER_RANK_COST: f32 = 1.0;

/// Preference per box (higher is better): the ensemble's mean log-probability
/// over the boxes, less the rank cost. `regions` are `[left, top, right,
/// bottom]` in working pixels; `geometry` is |ln aspect|, area share and step
/// score per box.
fn box_scorer_preference(
    working: &image::RgbImage,
    regions: &[[f32; 4]],
    geometry: &[[f32; 3]],
) -> Vec<f32> {
    let width = BOX_SCORER_WIDTH;
    let image = box_scorer_image(working);
    let (rows, cols) = (working.height() as usize / BOX_SCORER_CELL, working.width() as usize / BOX_SCORER_CELL);
    let (half_rows, half_cols) = (rows / 2, cols / 2);
    let masks: Vec<Vec<f32>> = regions
        .iter()
        .map(|region| box_coverage(region, (BOX_SCORER_CELL * 2) as f32, half_rows, half_cols))
        .collect();
    let mut preference = vec![0.0f32; regions.len()];
    for model in 0..BOX_SCORER_MODELS {
        let slice = |weights: &'static [f32], size: usize| &weights[model * size..(model + 1) * size];
        let first = max_pool2(
            &conv3x3_relu(
                &image,
                3,
                rows,
                cols,
                slice(&BOX_SCORER_C1_W, width * 27),
                slice(&BOX_SCORER_C1_B, width),
                true,
            ),
            width,
            rows,
            cols,
        );
        // The image part of the second layer is the same for every box. The
        // per-box layers work on tiny maps, so they run channels-last, where
        // the inner loop covers all output channels at once.
        let channels = 2 * width;
        let shared = channels_last(
            &conv3x3_relu(
                &first,
                width,
                half_rows,
                half_cols,
                slice(&BOX_SCORER_C2_W, channels * width * 9),
                slice(&BOX_SCORER_C2_B, channels),
                false,
            ),
            channels,
        );
        let mask_taps = taps_last(slice(&BOX_SCORER_C2_MASK_W, channels * 9), channels, 1);
        let third_taps = taps_last(slice(&BOX_SCORER_C3_W, channels * channels * 9), channels, channels);
        let fourth_taps = taps_last(slice(&BOX_SCORER_C4_W, channels * channels * 9), channels, channels);
        let logits: Vec<f32> = masks
            .iter()
            .zip(geometry)
            .map(|(mask, geometry)| {
                let mut second = shared.clone();
                conv3x3_add_channels_last(&mut second, mask, half_rows, half_cols, 1, &mask_taps);
                second.iter_mut().for_each(|value| *value = value.max(0.0));
                let (rows, cols) = (half_rows / 2, half_cols / 2);
                let second = max_pool2_channels_last(&second, channels, half_rows, half_cols);
                let mut third = bias_map(slice(&BOX_SCORER_C3_B, channels), rows * cols);
                conv3x3_add_channels_last(&mut third, &second, rows, cols, channels, &third_taps);
                third.iter_mut().for_each(|value| *value = value.max(0.0));
                let mut fourth = bias_map(slice(&BOX_SCORER_C4_B, channels), rows * cols);
                conv3x3_add_channels_last(&mut fourth, &third, rows, cols, channels, &fourth_taps);
                fourth.iter_mut().for_each(|value| *value = value.max(0.0));
                let mut peak = vec![f32::MIN; channels];
                let mut mean = vec![0.0f32; channels];
                for pixel in fourth.chunks_exact(channels) {
                    for channel in 0..channels {
                        peak[channel] = peak[channel].max(pixel[channel]);
                        mean[channel] += pixel[channel] / (rows * cols) as f32;
                    }
                }
                let mut features = peak;
                features.extend(mean);
                features.extend_from_slice(geometry);
                let inputs = features.len();
                let hidden_weights = slice(&BOX_SCORER_FC1_W, BOX_SCORER_HIDDEN * inputs);
                let hidden_bias = slice(&BOX_SCORER_FC1_B, BOX_SCORER_HIDDEN);
                let output_weights = slice(&BOX_SCORER_FC2_W, BOX_SCORER_HIDDEN);
                (0..BOX_SCORER_HIDDEN)
                    .map(|unit| {
                        let row = &hidden_weights[unit * inputs..(unit + 1) * inputs];
                        let value = row.iter().zip(&features).map(|(w, x)| w * x).sum::<f32>()
                            + hidden_bias[unit];
                        value.max(0.0) * output_weights[unit]
                    })
                    .sum::<f32>()
                    + BOX_SCORER_FC2_B[model]
            })
            .collect();
        let peak = logits.iter().fold(f32::MIN, |a, b| a.max(*b));
        let log_total = peak + logits.iter().map(|logit| (logit - peak).exp()).sum::<f32>().ln();
        for (value, logit) in preference.iter_mut().zip(&logits) {
            *value += (logit - log_total) / BOX_SCORER_MODELS as f32;
        }
    }
    for (rank, value) in preference.iter_mut().enumerate() {
        *value -= BOX_SCORER_RANK_COST * rank as f32;
    }
    preference
}

/// The working image as 16 px cell means in (channel, row, column) layout,
/// normalised by its own mean and spread.
fn box_scorer_image(working: &image::RgbImage) -> Vec<f32> {
    let cell = BOX_SCORER_CELL;
    let (rows, cols) = (working.height() as usize / cell, working.width() as usize / cell);
    let mut image = vec![0.0f32; 3 * rows * cols];
    for row in 0..rows {
        for col in 0..cols {
            let mut sum = [0.0f32; 3];
            for y in row * cell..(row + 1) * cell {
                for x in col * cell..(col + 1) * cell {
                    let pixel = working.get_pixel(x as u32, y as u32);
                    for channel in 0..3 {
                        sum[channel] += pixel[channel] as f32;
                    }
                }
            }
            for channel in 0..3 {
                image[channel * rows * cols + row * cols + col] = sum[channel] / (cell * cell) as f32;
            }
        }
    }
    let count = image.len() as f64;
    let mean = image.iter().map(|value| *value as f64).sum::<f64>() / count;
    let variance = image.iter().map(|value| (*value as f64 - mean).powi(2)).sum::<f64>() / count;
    let (mean, spread) = (mean as f32, (variance.sqrt() + 8.0) as f32);
    image.iter_mut().for_each(|value| *value = (*value - mean) / spread);
    image
}

/// Share of each `cell` px square covered by the box `[left, top, right, bottom]`.
fn box_coverage(region: &[f32; 4], cell: f32, rows: usize, cols: usize) -> Vec<f32> {
    let overlap = |index: usize, low: f32, high: f32| {
        ((index as f32 + 1.0).min(high / cell) - (index as f32).max(low / cell)).clamp(0.0, 1.0)
    };
    let mut mask = vec![0.0f32; rows * cols];
    for row in 0..rows {
        let vertical = overlap(row, region[1], region[3]);
        for col in 0..cols {
            mask[row * cols + col] = vertical * overlap(col, region[0], region[2]);
        }
    }
    mask
}

/// 3x3 convolution with zero padding on a `rows` x `cols` map, in PyTorch's
/// (channel, row, column) layout, optionally followed by ReLU.
fn conv3x3_relu(
    input: &[f32],
    channels: usize,
    rows: usize,
    cols: usize,
    weights: &[f32],
    bias: &[f32],
    relu: bool,
) -> Vec<f32> {
    let plane_size = rows * cols;
    let mut output = vec![0.0f32; bias.len() * plane_size];
    for (out, plane) in output.chunks_exact_mut(plane_size).enumerate() {
        plane.fill(bias[out]);
        for channel in 0..channels {
            let source = &input[channel * plane_size..(channel + 1) * plane_size];
            for ky in 0..3usize {
                for kx in 0..3usize {
                    let weight = weights[((out * channels + channel) * 3 + ky) * 3 + kx];
                    let (col_start, col_end) = (1usize.saturating_sub(kx), (cols + 1 - kx).min(cols));
                    for row in 1usize.saturating_sub(ky)..(rows + 1 - ky).min(rows) {
                        let y = row + ky - 1;
                        let target = &mut plane[row * cols + col_start..row * cols + col_end];
                        let from = &source[y * cols + col_start + kx - 1..];
                        for (value, sample) in target.iter_mut().zip(from) {
                            *value += weight * sample;
                        }
                    }
                }
            }
        }
        if relu {
            plane.iter_mut().for_each(|value| *value = value.max(0.0));
        }
    }
    output
}

/// 2x2 max pooling that drops an odd last row or column, as PyTorch does.
fn max_pool2(input: &[f32], channels: usize, rows: usize, cols: usize) -> Vec<f32> {
    let (pooled_rows, pooled_cols) = (rows / 2, cols / 2);
    let mut pooled = vec![0.0f32; channels * pooled_rows * pooled_cols];
    for channel in 0..channels {
        let at = |y: usize, x: usize| input[channel * rows * cols + y * cols + x];
        for row in 0..pooled_rows {
            for col in 0..pooled_cols {
                pooled[(channel * pooled_rows + row) * pooled_cols + col] = at(2 * row, 2 * col)
                    .max(at(2 * row, 2 * col + 1))
                    .max(at(2 * row + 1, 2 * col))
                    .max(at(2 * row + 1, 2 * col + 1));
            }
        }
    }
    pooled
}

/// A (channel, row, column) map as (row, column, channel).
fn channels_last(input: &[f32], channels: usize) -> Vec<f32> {
    let plane = input.len() / channels;
    let mut output = vec![0.0f32; input.len()];
    for channel in 0..channels {
        for index in 0..plane {
            output[index * channels + channel] = input[channel * plane + index];
        }
    }
    output
}

/// PyTorch 3x3 weights (output, input, ky, kx) as (ky, kx, input, output).
fn taps_last(weights: &[f32], outputs: usize, inputs: usize) -> Vec<f32> {
    let mut taps = vec![0.0f32; weights.len()];
    for output in 0..outputs {
        for input in 0..inputs {
            for tap in 0..9 {
                taps[(tap * inputs + input) * outputs + output] = weights[(output * inputs + input) * 9 + tap];
            }
        }
    }
    taps
}

fn bias_map(bias: &[f32], pixels: usize) -> Vec<f32> {
    bias.iter().copied().cycle().take(bias.len() * pixels).collect()
}

/// Adds a zero-padded 3x3 convolution of a channels-last map to `output`.
fn conv3x3_add_channels_last(
    output: &mut [f32],
    input: &[f32],
    rows: usize,
    cols: usize,
    inputs: usize,
    taps: &[f32],
) {
    let outputs = output.len() / (rows * cols);
    for row in 0..rows {
        for col in 0..cols {
            let target = &mut output[(row * cols + col) * outputs..(row * cols + col + 1) * outputs];
            for ky in 0..3 {
                let Some(y) = (row + ky).checked_sub(1).filter(|y| *y < rows) else {
                    continue;
                };
                for kx in 0..3 {
                    let Some(x) = (col + kx).checked_sub(1).filter(|x| *x < cols) else {
                        continue;
                    };
                    let source = &input[(y * cols + x) * inputs..(y * cols + x + 1) * inputs];
                    let tap = &taps[(ky * 3 + kx) * inputs * outputs..(ky * 3 + kx + 1) * inputs * outputs];
                    for (value, weights) in source.iter().zip(tap.chunks_exact(outputs)) {
                        if *value == 0.0 {
                            continue;
                        }
                        for (sum, weight) in target.iter_mut().zip(weights) {
                            *sum += value * weight;
                        }
                    }
                }
            }
        }
    }
}

/// 2x2 max pooling of a channels-last map, dropping an odd last row or column.
fn max_pool2_channels_last(input: &[f32], channels: usize, rows: usize, cols: usize) -> Vec<f32> {
    let (pooled_rows, pooled_cols) = (rows / 2, cols / 2);
    let mut pooled = vec![f32::MIN; pooled_rows * pooled_cols * channels];
    for row in 0..pooled_rows * 2 {
        for col in 0..pooled_cols * 2 {
            let source = &input[(row * cols + col) * channels..(row * cols + col + 1) * channels];
            let at = ((row / 2) * pooled_cols + col / 2) * channels;
            for (value, sample) in pooled[at..at + channels].iter_mut().zip(source) {
                *value = value.max(*sample);
            }
        }
    }
    pooled
}
