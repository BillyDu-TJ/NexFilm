use image::{imageops::colorops::grayscale, DynamicImage, GenericImageView, GrayImage, Pixel};
use imageproc::contours::find_contours_with_threshold;
use imageproc::contrast::{otsu_level, threshold};
use imageproc::edges::canny;
use imageproc::filter::gaussian_blur_f32;
use imageproc::geometry::{approximate_polygon_dp, arc_length};
use imageproc::gradients::{horizontal_sobel, vertical_sobel};
use imageproc::point::Point;
use serde::Serialize;

const DEFAULT_INSET: f32 = 0.1;
const MIN_AREA_RATIO: f64 = 0.12;
const MAX_AREA_RATIO: f64 = 0.98;
const MAX_SIDE_RATIO: f64 = 6.0;
const MAX_DIAGONAL_RATIO: f64 = 3.0;
const MIN_GRADIENT_PROMINENCE: f64 = 1.22;
const MAX_ANALYSIS_EDGE: u32 = 320;

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

    let gray = analysis_grayscale(thumbnail);
    let (width, height) = gray.dimensions();
    let blurred = gaussian_blur_f32(&gray, 1.4);

    // Film gates are often not closed contours after thresholding: sprocket
    // holes and image texture break or join their edges. Locate the four long
    // gate edges from robust Sobel projections first, then fit each edge.
    if let Some(points) = detect_from_gradient_projections(&blurred) {
        return detected_result(points, width, height, "detected_gradient");
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

    detected_result(points, width, height, "detected_contour")
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

    // These broad bands target the inner film gate while excluding the outer
    // thumbnail boundary and most sprocket holes.
    let (left_x, left_peak, left_prominence) = strongest_projection(&left_projection, 0.115, 0.35)?;
    let (right_x, right_peak, right_prominence) =
        strongest_projection(&right_projection, 0.65, 0.865)?;
    let (top_y, top_peak, top_prominence) = strongest_projection(&top_projection, 0.08, 0.40)?;
    let (bottom_y, bottom_peak, bottom_prominence) =
        strongest_projection(&bottom_projection, 0.60, 0.92)?;

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
    if strong_edges < 3 || mean_prominence < MIN_GRADIENT_PROMINENCE {
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
    let start_y = (height as f64 * 0.22).round() as u32;
    let end_y = (height as f64 * 0.78).round() as u32;
    let values = (0..gradient.width())
        .map(|x| {
            let mut samples = (start_y..end_y)
                .map(|y| gradient.get_pixel(x, y)[0])
                .collect::<Vec<_>>();
            (median_i16(&mut samples) * polarity as f64).max(0.0)
        })
        .collect::<Vec<_>>();
    smooth_projection(&values, (gradient.width() / 128).max(1) as usize)
}

fn gradient_projection_y(
    gradient: &image::ImageBuffer<image::Luma<i16>, Vec<i16>>,
    width: u32,
    polarity: i32,
) -> Vec<f64> {
    let start_x = (width as f64 * 0.22).round() as u32;
    let end_x = (width as f64 * 0.78).round() as u32;
    let values = (0..gradient.height())
        .map(|y| {
            let mut samples = (start_x..end_x)
                .map(|x| gradient.get_pixel(x, y)[0])
                .collect::<Vec<_>>();
            (median_i16(&mut samples) * polarity as f64).max(0.0)
        })
        .collect::<Vec<_>>();
    smooth_projection(&values, (gradient.height() / 128).max(1) as usize)
}

fn median_i16(samples: &mut [i16]) -> f64 {
    if samples.is_empty() {
        return 0.0;
    }
    let middle = samples.len() / 2;
    samples.select_nth_unstable(middle);
    samples[middle] as f64
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
    fn blank_thumbnail_uses_centered_eighty_percent_fallback() {
        let image = DynamicImage::ImageLuma8(GrayImage::from_pixel(320, 200, Luma([16])));
        let result = detect_film_border(&image);

        assert_eq!(result.confidence, DetectionConfidence::Low);
        assert_eq!(result.points, FilmBorderDetection::fallback().points);
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
