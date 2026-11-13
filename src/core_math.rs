use nalgebra::Matrix3;

pub type Homography3 = [[f32; 3]; 3];

/// Rec. 709 / linear-sRGB luminance weights used by the fixed density-capture
/// domain. The green-heavy weighting is also the monochrome analysis contract.
pub const DENSITY_LUMA_COEFFICIENTS: [f32; 3] = [0.2126, 0.7152, 0.0722];

#[inline]
pub fn density_luma(rgb: [f32; 3]) -> f32 {
    rgb.iter()
        .zip(DENSITY_LUMA_COEFFICIENTS)
        .map(|(value, coefficient)| value * coefficient)
        .sum()
}

/// 返回预定义的核心去串扰矩阵 (Status M to Print Density)。
/// 这个 3x3 矩阵用于消除胶片各染料层之间的光谱串扰。
///
/// Matrix layout (Row-major initialization):
/// R:  1.0197,  0.0317,  0.0091
/// G: -0.0052,  0.8933,  0.0521
/// B:  0.0131, -0.0011,  0.9712
#[inline(always)]
pub fn status_m_crosstalk_matrix() -> Matrix3<f32> {
    Matrix3::new(
        1.0197, 0.0317, 0.0091, -0.0052, 0.8933, 0.0521, 0.0131, -0.0011, 0.9712,
    )
}

/// Apply the shader's density-domain normalization and tone curve to one
/// channel. Keeping this in Rust gives thumbnails and export a single CPU
/// contract to compare against the WebGL implementation.
#[inline]
pub fn normalize_density_channel(
    density: f32,
    d_min: f32,
    d_max: f32,
    highlights: f32,
    shadows: f32,
    gamma: f32,
) -> f32 {
    let range = d_max - d_min;
    let normalized = if range.abs() > 1e-6 {
        (density - d_min) / range
    } else {
        0.0
    };
    let gamma_corrected = normalized.clamp(0.0, 1.0).powf(1.0 / gamma.max(1e-6));
    tone_normalized_channel(gamma_corrected, highlights, shadows)
}

/// Normalize a modern-pipeline density sample while preserving highlight
/// headroom above the automatic D-Max.
///
/// The legacy path keeps the historical `[0, 1]` ceiling. The modern path needs
/// the over-range samples so the linear highlight recovery and the display
/// shoulder can reconstruct detail instead of receiving a flat white.
#[inline]
pub fn normalize_density_channel_unclamped_high(density: f32, d_min: f32, d_max: f32) -> f32 {
    let range = d_max - d_min;
    if !density.is_finite()
        || !d_min.is_finite()
        || !d_max.is_finite()
        || !range.is_finite()
        || range <= 1e-6
    {
        return 0.0;
    }
    ((density - d_min) / range).max(0.0)
}

pub const LINEAR_HIGHLIGHT_PIVOT: f32 = 0.62;
pub const LINEAR_HIGHLIGHT_SLIDER_MAX: f32 = 0.75;
pub const LINEAR_HIGHLIGHT_RECOVERY_GAIN: f32 = 1.45;
pub const LINEAR_HIGHLIGHT_BOOST_GAIN: f32 = 1.25;

/// Apply the Develop Highlights slider in linear light.
///
/// Recovery works independently on every channel above the pivot so neutral
/// input stays neutral and one channel cannot lock the other two in place.
/// Boost keeps the brightest channel in charge and scales all channels
/// together, which lets a bright highlight approach the white point without
/// changing its hue. Values at or below the pivot are returned exactly
/// unchanged.
#[inline]
pub fn apply_linear_highlight_adjustment_rgb(rgb: [f32; 3], highlights: f32) -> [f32; 3] {
    if rgb.iter().any(|value| !value.is_finite()) || !highlights.is_finite() {
        return [0.0; 3];
    }
    let rgb = rgb.map(|value| value.max(0.0));
    if highlights.abs() <= 1e-6 {
        return rgb;
    }
    if highlights < 0.0 {
        let amount = (-highlights / LINEAR_HIGHLIGHT_SLIDER_MAX).min(1.0);
        let gain = LINEAR_HIGHLIGHT_RECOVERY_GAIN * amount;
        return rgb.map(|value| {
            if value <= LINEAR_HIGHLIGHT_PIVOT {
                return value;
            }
            let distance = (value / LINEAR_HIGHLIGHT_PIVOT).ln();
            let adjusted_distance = distance / (1.0 + gain * distance);
            LINEAR_HIGHLIGHT_PIVOT * adjusted_distance.exp()
        });
    }

    let maximum = rgb[0].max(rgb[1]).max(rgb[2]);
    if maximum <= LINEAR_HIGHLIGHT_PIVOT {
        return rgb;
    }
    let amount = (highlights / LINEAR_HIGHLIGHT_SLIDER_MAX).min(1.0);
    let gain = LINEAR_HIGHLIGHT_BOOST_GAIN * amount;
    let distance = (maximum / LINEAR_HIGHLIGHT_PIVOT).ln();
    let adjusted_distance = distance * (1.0 + gain * distance / (1.0 + distance));
    let adjusted_maximum = LINEAR_HIGHLIGHT_PIVOT * adjusted_distance.exp();
    rgb.map(|value| value * (adjusted_maximum / maximum))
}

/// Apply a finite, per-channel highlight shoulder before the display matrix.
///
/// `knee` is the last value that passes through untouched and `2.0 - knee` is
/// the first value that becomes pure white. The two parabolic pieces meet with
/// matching value and slope, so the shoulder neither flattens ordinary
/// highlights nor leaves an asymptote that can never reach white.
#[inline]
pub fn finite_soft_clip_highlights_rgb(rgb: [f32; 3], knee: f32) -> [f32; 3] {
    if rgb.iter().any(|value| !value.is_finite()) || !knee.is_finite() || knee < 0.0 || knee >= 1.0
    {
        return [0.0; 3];
    }
    let clip = 2.0 - knee;
    rgb.map(|value| {
        let value = value.max(0.0);
        if value <= knee {
            value
        } else if value >= clip {
            1.0
        } else {
            let remaining = clip - value;
            1.0 - (remaining * remaining) / (4.0 * (1.0 - knee))
        }
    })
}

/// Compress a linear highlight peak into a smooth shoulder while preserving
/// the three channel ratios. Values at or below `knee` are bit-identical.
#[inline]
pub fn rolloff_highlight_shoulder_rgb(rgb: [f32; 3], knee: f32, ceiling: f32) -> [f32; 3] {
    if rgb.iter().any(|value| !value.is_finite())
        || !knee.is_finite()
        || !ceiling.is_finite()
        || ceiling <= knee
    {
        return [0.0; 3];
    }
    let rgb = rgb.map(|value| value.max(0.0));
    let maximum = rgb[0].max(rgb[1]).max(rgb[2]);
    if maximum <= knee {
        return rgb;
    }
    let span = ceiling - knee;
    let excess = maximum - knee;
    let compressed = knee + (span * excess) / (span + excess);
    rgb.map(|value| value * (compressed / maximum))
}

#[inline]
pub fn neutral_density_bounds(d_min: [f32; 3], d_max: [f32; 3]) -> (f32, f32) {
    (density_luma(d_min), density_luma(d_max))
}

/// Apply the Master D-Min/D-Max trim to the display endpoints.
///
/// The endpoints are per channel: a sampled Roll measures its own base-to-leader
/// span in every channel, so the three channels rarely share one span. Adding
/// the same raw density offset to all of them therefore changes each channel's
/// gain by a different relative amount, which tints the print (raising D-Max by
/// one uniform offset visibly turns the frame green). Scaling each channel's
/// offset by its share of the span keeps the trim equal in normalised display
/// units, where every channel receives the same exposure and contrast move.
pub fn trim_density_endpoints(
    d_min: [f32; 3],
    d_max: [f32; 3],
    min_offset: f32,
    max_offset: f32,
) -> ([f32; 3], [f32; 3]) {
    let span = [
        d_max[0] - d_min[0],
        d_max[1] - d_min[1],
        d_max[2] - d_min[2],
    ];
    let span_luma = density_luma(span);
    if !span_luma.is_finite() || span_luma <= 1.0e-4 {
        // A collapsed window has no share to scale by; keep the plain shift so
        // the sliders stay usable instead of producing infinities.
        return (
            [
                d_min[0] + min_offset,
                d_min[1] + min_offset,
                d_min[2] + min_offset,
            ],
            [
                d_max[0] + max_offset,
                d_max[1] + max_offset,
                d_max[2] + max_offset,
            ],
        );
    }
    let mut trimmed = (d_min, d_max);
    for channel in 0..3 {
        let scale = if span[channel].is_finite() {
            span[channel] / span_luma
        } else {
            1.0
        };
        trimmed.0[channel] = d_min[channel] + min_offset * scale;
        trimmed.1[channel] = d_max[channel] + max_offset * scale;
    }
    trimmed
}

#[inline]
pub fn neutralize_rgb(rgb: [f32; 3]) -> [f32; 3] {
    let luma = density_luma(rgb);
    [luma; 3]
}

#[inline]
pub fn tone_density_channel(
    density: f32,
    d_min: f32,
    d_max: f32,
    highlights: f32,
    shadows: f32,
) -> f32 {
    let range = d_max - d_min;
    let normalized = if range.abs() > 1e-6 {
        (density - d_min) / range
    } else {
        0.0
    };
    let clamped = normalized.clamp(0.0, 1.0);
    let toned = normalized
        + shadows * (1.0 - clamped).powi(2) * normalized
        + highlights * clamped.powi(2) * (1.0 - normalized);
    toned.clamp(0.0, 1.0)
}

#[inline]
pub fn tone_normalized_channel(value: f32, highlights: f32, shadows: f32) -> f32 {
    let clamped = value.clamp(0.0, 1.0);
    (value
        + shadows * (1.0 - clamped).powi(2) * value
        + highlights * clamped.powi(2) * (1.0 - value))
        .clamp(0.0, 1.0)
}

/// Display-referred contrast around mid grey. Mirrors `toneContrast` in the
/// WebGL fragment shader: the usable range stops at a 0.5/1.5 gain so the
/// weakest setting keeps half of the separation from the pivot instead of
/// flattening the frame onto it. The clamp also guards the render paths against
/// a stored value the control can no longer produce.
#[inline]
pub fn contrast_channel(value: f32, contrast: f32) -> f32 {
    let factor = 1.0 + contrast.clamp(-0.5, 0.5);
    ((value - 0.5) * factor + 0.5).clamp(0.0, 1.0)
}

/// Apply the creative controls that intentionally live after display gamma.
/// This mirrors `applyPostGammaAdjustments` in the WebGL fragment shader.
#[inline]
pub fn apply_post_gamma_adjustments(
    rgb: [f32; 3],
    highlights: f32,
    shadows: f32,
    contrast: f32,
    saturation: f32,
    temperature: f32,
    tint: f32,
) -> [f32; 3] {
    apply_post_gamma_adjustments_with_luma(
        rgb,
        highlights,
        shadows,
        contrast,
        saturation,
        temperature,
        tint,
        DENSITY_LUMA_COEFFICIENTS,
    )
}

#[inline]
pub fn apply_post_gamma_adjustments_with_luma(
    rgb: [f32; 3],
    highlights: f32,
    shadows: f32,
    contrast: f32,
    saturation: f32,
    temperature: f32,
    tint: f32,
    luma_coefficients: [f32; 3],
) -> [f32; 3] {
    let mut adjusted = rgb.map(|value| tone_normalized_channel(value, highlights, shadows));
    adjusted = adjusted.map(|value| contrast_channel(value, contrast));

    let temperature = temperature.clamp(-1.0, 1.0);
    adjusted[0] *= 1.0 + temperature * 0.20;
    adjusted[2] *= 1.0 - temperature * 0.20;

    let tint = tint.clamp(-1.0, 1.0);
    adjusted[0] *= 1.0 + tint * 0.10;
    adjusted[1] *= 1.0 - tint * 0.20;
    adjusted[2] *= 1.0 + tint * 0.10;

    let luma = adjusted
        .iter()
        .zip(luma_coefficients)
        .map(|(value, coefficient)| value * coefficient)
        .sum::<f32>();
    let saturation_factor = 1.0 + saturation.clamp(-1.0, 1.0);
    adjusted.map(|value| (luma + (value - luma) * saturation_factor).clamp(0.0, 1.0))
}

/// Reproduces `getHomography` from the WebGL frontend. The matrix maps the
/// pre-warp crop UV into the calibrated source-image UV.
pub fn shader_homography(points: [[f32; 2]; 4]) -> Homography3 {
    let [[x0, y0], [x1, y1], [x2, y2], [x3, y3]] = points;
    let dx1 = x1 - x2;
    let dx2 = x3 - x2;
    let dx3 = x0 - x1 + x2 - x3;
    let dy1 = y1 - y2;
    let dy2 = y3 - y2;
    let dy3 = y0 - y1 + y2 - y3;

    let c = x0;
    let f = y0;
    let determinant = dx1 * dy2 - dy1 * dx2;
    let (a, b, d, e, g, h) = if determinant.abs() < 1e-6 {
        (x1 - x0, x3 - x0, y1 - y0, y3 - y0, 0.0, 0.0)
    } else {
        let g = (dx3 * dy2 - dy3 * dx2) / determinant;
        let h = (dx1 * dy3 - dy1 * dx3) / determinant;
        (
            x1 - x0 + g * x1,
            x3 - x0 + h * x3,
            y1 - y0 + g * y1,
            y3 - y0 + h * y3,
            g,
            h,
        )
    };

    let min_x = x0.min(x1).min(x2).min(x3);
    let max_x = x0.max(x1).max(x2).max(x3);
    let min_y = y0.min(y1).min(y2).min(y3);
    let max_y = y0.max(y1).max(y2).max(y3);
    let scale_x = 1.0 / (max_x - min_x).max(0.001);
    let scale_y = 1.0 / (max_y - min_y).max(0.001);
    let translate_x = -min_x * scale_x;
    let translate_y = -min_y * scale_y;

    [
        [
            a * scale_x,
            b * scale_y,
            a * translate_x + b * translate_y + c,
        ],
        [
            d * scale_x,
            e * scale_y,
            d * translate_x + e * translate_y + f,
        ],
        [
            g * scale_x,
            h * scale_y,
            g * translate_x + h * translate_y + 1.0,
        ],
    ]
}

#[inline]
pub fn apply_homography(matrix: &Homography3, uv: [f32; 2]) -> Option<[f32; 2]> {
    let denominator = matrix[2][0] * uv[0] + matrix[2][1] * uv[1] + matrix[2][2];
    if !denominator.is_finite() || denominator.abs() < 1e-8 {
        return None;
    }
    let x = (matrix[0][0] * uv[0] + matrix[0][1] * uv[1] + matrix[0][2]) / denominator;
    let y = (matrix[1][0] * uv[0] + matrix[1][1] * uv[1] + matrix[1][2]) / denominator;
    (x.is_finite() && y.is_finite()).then_some([x, y])
}

/// Maps an output UV back into the pre-perspective image. The frontend uses
/// the same inverse mapping so Develop preview, sampling, thumbnails, and
/// export stay pixel-consistent.
#[inline]
pub fn apply_perspective_uv(
    uv: [f32; 2],
    vertical: f32,
    horizontal: f32,
    aspect: f32,
    scale: f32,
) -> Option<[f32; 2]> {
    let safe_scale = scale.clamp(0.5, 3.0);
    let aspect_scale = (aspect.clamp(-100.0, 100.0) * 0.0035).exp();
    let x = (uv[0] * 2.0 - 1.0) / (safe_scale * aspect_scale);
    let y = (uv[1] * 2.0 - 1.0) / safe_scale;
    let denominator = 1.0
        + horizontal.clamp(-100.0, 100.0) * 0.003 * x
        + vertical.clamp(-100.0, 100.0) * 0.003 * y;
    if !denominator.is_finite() || denominator.abs() < 1e-6 {
        return None;
    }
    let mapped = [(x / denominator + 1.0) * 0.5, (y / denominator + 1.0) * 0.5];
    (mapped[0].is_finite() && mapped[1].is_finite()).then_some(mapped)
}

/// Applies the inverse radial mapping used by the Develop lens-distortion
/// controls. Negative values correct barrel distortion; positive values
/// correct pincushion distortion.
#[inline]
pub fn apply_lens_distortion_uv(uv: [f32; 2], distortion: f32) -> Option<[f32; 2]> {
    let centered = [uv[0] * 2.0 - 1.0, uv[1] * 2.0 - 1.0];
    let radius_squared = centered[0] * centered[0] + centered[1] * centered[1];
    let factor = 1.0 + distortion.clamp(-100.0, 100.0) * 0.004 * radius_squared;
    let mapped = [
        (centered[0] * factor + 1.0) * 0.5,
        (centered[1] * factor + 1.0) * 0.5,
    ];
    mapped
        .iter()
        .all(|value| value.is_finite())
        .then_some(mapped)
}

#[inline]
pub fn shader_smoothstep(edge0: f32, edge1: f32, value: f32) -> f32 {
    let range = edge1 - edge0;
    let t = if range.abs() < 1e-8 {
        if value < edge0 {
            0.0
        } else {
            1.0
        }
    } else {
        ((value - edge0) / range).clamp(0.0, 1.0)
    };
    t * t * (3.0 - 2.0 * t)
}

#[inline]
pub fn sprocket_white_mask(luma_difference: f32, tolerance: f32, feather: f32) -> f32 {
    let transition = shader_smoothstep(
        tolerance,
        tolerance + feather + 0.0001,
        luma_difference.abs(),
    );
    (1.0 - transition).powi(3)
}

/// Length of the shared tone-curve lookup table.
///
/// The WebGL preview also samples a 1024-entry table, so a curve drawn in the
/// inspector reaches the exported file through the same maths: a monotone cubic
/// (Fritsch-Carlson) through the control points, sampled onto this grid and read
/// back with linear interpolation.
pub const TONE_CURVE_LUT_SIZE: usize = 1024;

/// Control points a single curve may carry, endpoints included.
pub const MAX_TONE_CURVE_POINTS: usize = 16;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ToneCurveLut {
    samples: [f32; TONE_CURVE_LUT_SIZE],
}

impl ToneCurveLut {
    pub fn identity() -> Self {
        let mut samples = [0.0f32; TONE_CURVE_LUT_SIZE];
        for (index, sample) in samples.iter_mut().enumerate() {
            *sample = index as f32 / (TONE_CURVE_LUT_SIZE - 1) as f32;
        }
        Self { samples }
    }

    pub fn from_points(points: &[[f32; 2]]) -> Self {
        let points = normalize_curve_points(points);
        let tangents = monotone_curve_tangents(&points);
        let mut samples = [0.0f32; TONE_CURVE_LUT_SIZE];
        for (index, sample) in samples.iter_mut().enumerate() {
            let x = index as f32 / (TONE_CURVE_LUT_SIZE - 1) as f32;
            *sample = evaluate_monotone_curve(&points, &tangents, x).clamp(0.0, 1.0);
        }
        Self { samples }
    }

    /// Linear read-back of the table. The preview samples the same grid, so a
    /// value between two entries interpolates identically on both sides.
    #[inline]
    pub fn sample(&self, x: f32) -> f32 {
        let position = x.clamp(0.0, 1.0) * (TONE_CURVE_LUT_SIZE - 1) as f32;
        let low = position.floor() as usize;
        let high = (low + 1).min(TONE_CURVE_LUT_SIZE - 1);
        let fraction = position - low as f32;
        self.samples[low] + (self.samples[high] - self.samples[low]) * fraction
    }

    pub fn samples(&self) -> &[f32; TONE_CURVE_LUT_SIZE] {
        &self.samples
    }

    pub fn is_identity(&self) -> bool {
        self.samples.iter().enumerate().all(|(index, value)| {
            (value - index as f32 / (TONE_CURVE_LUT_SIZE - 1) as f32).abs() < 1e-6
        })
    }
}

fn normalize_curve_points(points: &[[f32; 2]]) -> Vec<[f32; 2]> {
    let mut normalized: Vec<[f32; 2]> = points
        .iter()
        .filter(|point| point[0].is_finite() && point[1].is_finite())
        .map(|point| [point[0].clamp(0.0, 1.0), point[1].clamp(0.0, 1.0)])
        .collect();
    normalized.sort_by(|left, right| left[0].partial_cmp(&right[0]).unwrap());
    // Two control points may not share an x: the later one wins, which is what
    // the editor does when a dragged point lands on its neighbour.
    let mut deduplicated: Vec<[f32; 2]> = Vec::with_capacity(normalized.len());
    for point in normalized {
        match deduplicated.last_mut() {
            Some(previous) if (previous[0] - point[0]).abs() < 1e-6 => *previous = point,
            _ => deduplicated.push(point),
        }
    }
    if deduplicated.is_empty() {
        return vec![[0.0, 0.0], [1.0, 1.0]];
    }
    // The endpoints exist so the curve always spans the full input range. Only
    // their output may move, which is how a lifted black point is expressed.
    if deduplicated[0][0] > 1e-6 {
        let first = deduplicated[0][1];
        deduplicated.insert(0, [0.0, first]);
    }
    if deduplicated.last().map(|point| point[0]).unwrap_or(1.0) < 1.0 - 1e-6 {
        let last = deduplicated.last().map(|point| point[1]).unwrap_or(1.0);
        deduplicated.push([1.0, last]);
    }
    deduplicated.truncate(MAX_TONE_CURVE_POINTS);
    deduplicated
}

/// Fritsch-Carlson tangents: the classic monotone cubic that cannot overshoot
/// between two control points.
fn monotone_curve_tangents(points: &[[f32; 2]]) -> Vec<f32> {
    let count = points.len();
    if count < 2 {
        return vec![0.0; count];
    }
    let slopes: Vec<f32> = (0..count - 1)
        .map(|index| {
            let dx = points[index + 1][0] - points[index][0];
            if dx.abs() < 1e-8 {
                0.0
            } else {
                (points[index + 1][1] - points[index][1]) / dx
            }
        })
        .collect();
    let mut tangents = vec![0.0f32; count];
    tangents[0] = slopes[0];
    tangents[count - 1] = slopes[count - 2];
    for index in 1..count - 1 {
        let previous = slopes[index - 1];
        let next = slopes[index];
        if previous * next <= 0.0 {
            // A local extremum keeps a flat tangent, so the curve stays inside
            // the bracket the user drew.
            tangents[index] = 0.0;
        } else {
            tangents[index] = (previous + next) * 0.5;
        }
    }
    for index in 0..count - 1 {
        if slopes[index].abs() < 1e-8 {
            tangents[index] = 0.0;
            tangents[index + 1] = 0.0;
            continue;
        }
        let alpha = tangents[index] / slopes[index];
        let beta = tangents[index + 1] / slopes[index];
        let magnitude = alpha.hypot(beta);
        if magnitude > 3.0 {
            let scale = 3.0 / magnitude;
            tangents[index] = scale * alpha * slopes[index];
            tangents[index + 1] = scale * beta * slopes[index];
        }
    }
    tangents
}

fn evaluate_monotone_curve(points: &[[f32; 2]], tangents: &[f32], x: f32) -> f32 {
    if points.len() < 2 {
        return x;
    }
    let clamped = x.clamp(points[0][0], points[points.len() - 1][0]);
    let mut segment = points.len() - 2;
    for index in 0..points.len() - 1 {
        if clamped <= points[index + 1][0] + 1e-9 {
            segment = index;
            break;
        }
    }
    let [x0, y0] = points[segment];
    let [x1, y1] = points[segment + 1];
    let width = x1 - x0;
    if width.abs() < 1e-8 {
        return y1;
    }
    let t = ((clamped - x0) / width).clamp(0.0, 1.0);
    let t2 = t * t;
    let t3 = t2 * t;
    let h00 = 2.0 * t3 - 3.0 * t2 + 1.0;
    let h10 = t3 - 2.0 * t2 + t;
    let h01 = -2.0 * t3 + 3.0 * t2;
    let h11 = t3 - t2;
    h00 * y0 + h10 * width * tangents[segment] + h01 * y1 + h11 * width * tangents[segment + 1]
}

/// The four curves of the Develop Tone Curve panel. The per-channel curves run
/// first and the composite RGB curve reads their result, which is the order the
/// Lightroom point curve uses.
#[derive(Clone, Debug, PartialEq)]
pub struct ToneCurveSet {
    rgb: ToneCurveLut,
    red: ToneCurveLut,
    green: ToneCurveLut,
    blue: ToneCurveLut,
    identity: bool,
}

impl ToneCurveSet {
    pub fn new(rgb: &[[f32; 2]], red: &[[f32; 2]], green: &[[f32; 2]], blue: &[[f32; 2]]) -> Self {
        let set = Self {
            rgb: ToneCurveLut::from_points(rgb),
            red: ToneCurveLut::from_points(red),
            green: ToneCurveLut::from_points(green),
            blue: ToneCurveLut::from_points(blue),
            identity: false,
        };
        let identity = set.rgb.is_identity()
            && set.red.is_identity()
            && set.green.is_identity()
            && set.blue.is_identity();
        Self { identity, ..set }
    }

    pub fn identity() -> Self {
        Self {
            rgb: ToneCurveLut::identity(),
            red: ToneCurveLut::identity(),
            green: ToneCurveLut::identity(),
            blue: ToneCurveLut::identity(),
            identity: true,
        }
    }

    /// Cached at construction: the render loop asks this once per pixel.
    #[inline]
    pub fn is_identity(&self) -> bool {
        self.identity
    }

    #[inline]
    pub fn apply(&self, rgb: [f32; 3]) -> [f32; 3] {
        if self.is_identity() {
            return rgb;
        }
        let per_channel = [
            self.red.sample(rgb[0].clamp(0.0, 1.0)),
            self.green.sample(rgb[1].clamp(0.0, 1.0)),
            self.blue.sample(rgb[2].clamp(0.0, 1.0)),
        ];
        [
            self.rgb.sample(per_channel[0]),
            self.rgb.sample(per_channel[1]),
            self.rgb.sample(per_channel[2]),
        ]
    }
}

/// Radius used when a detail control is at zero would otherwise collapse to a
/// sub-pixel kernel on a bounded preview proxy.
pub const DETAIL_MIN_SIGMA: f32 = 0.4;
/// Reference long edge the user-facing radii are quoted at. The Develop preview
/// keeps its proxy near this size, so a radius of 1.0 means one preview pixel
/// there and the same share of the frame in an export of any resolution.
pub const DETAIL_REFERENCE_LONG_EDGE: f32 = 2560.0;
/// Fine-detail scale of the sharpening stage, in native pixels.
pub const DETAIL_FINE_SIGMA: f32 = 0.6;
/// Mid-frequency scale of the Texture control, in native pixels.
pub const DETAIL_TEXTURE_SIGMA: f32 = 1.6;
/// Clarity works on structure: a share of the rendered long edge.
pub const DETAIL_CLARITY_EDGE_RATIO: f32 = 0.012;
/// Downscale factor used for clarity and haze transmission.
pub const DETAIL_LOCAL_DOWNSCALE: u32 = 4;
/// Dark-channel window of the haze estimate, as a share of the rendered edge.
pub const DETAIL_DEHAZE_EDGE_RATIO: f32 = 1.0 / 16.0;
/// Dark Channel Prior omega: how much of the estimated veil is removed.
pub const DETAIL_DEHAZE_OMEGA: f32 = 0.95;
pub const DETAIL_CLARITY_GAIN: f32 = 0.8;
pub const DETAIL_TEXTURE_GAIN: f32 = 0.8;
pub const DETAIL_SHARPENING_GAIN: f32 = 0.8;
pub const DETAIL_SHARPENING_DETAIL_GAIN: f32 = 0.75;
pub const DETAIL_MASK_MIN_EDGE: f32 = 0.02;
pub const DETAIL_MASK_EDGE_SPAN: f32 = 0.30;
pub const DETAIL_MASK_SOFTNESS: f32 = 0.06;

/// Everything the Detail stage needs to describe itself. `scale` is the rendered
/// long edge divided by [`DETAIL_REFERENCE_LONG_EDGE`]: the Develop preview works
/// on a bounded proxy and the export works on the whole scan, so every radius is
/// converted into rendered pixels and both sides show the same sharpening.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DetailSettings {
    pub sharpening_amount: f32,
    pub sharpening_radius: f32,
    pub sharpening_detail: f32,
    pub sharpening_masking: f32,
    pub texture: f32,
    pub clarity: f32,
    pub dehaze: f32,
    pub scale: f32,
}

impl DetailSettings {
    pub fn neutral(scale: f32) -> Self {
        Self {
            sharpening_amount: 0.0,
            sharpening_radius: 1.0,
            sharpening_detail: 25.0,
            sharpening_masking: 0.0,
            texture: 0.0,
            clarity: 0.0,
            dehaze: 0.0,
            scale,
        }
    }

    pub fn is_neutral(&self) -> bool {
        self.sharpening_amount <= 0.0
            && self.texture.abs() < 1e-4
            && self.clarity.abs() < 1e-4
            && self.dehaze.abs() < 1e-4
    }

    /// Rendered-pixel sigma of the radius-driven sharpening blur.
    pub fn sharpening_sigma(&self) -> f32 {
        (self.sharpening_radius * self.scale).max(DETAIL_MIN_SIGMA)
    }

    pub fn fine_sigma(&self) -> f32 {
        (DETAIL_FINE_SIGMA * self.scale).max(DETAIL_MIN_SIGMA)
    }

    pub fn texture_sigma(&self) -> f32 {
        (DETAIL_TEXTURE_SIGMA * self.scale).max(DETAIL_MIN_SIGMA)
    }

    /// Sigma of the clarity blur on the downscaled working buffer.
    pub fn clarity_sigma_downscaled(&self, rendered_long_edge: f32) -> f32 {
        let sigma = DETAIL_CLARITY_EDGE_RATIO * rendered_long_edge / DETAIL_LOCAL_DOWNSCALE as f32;
        sigma.max(DETAIL_MIN_SIGMA)
    }

    /// Dark-channel window radius on the downscaled working buffer, and the
    /// radius of the box blur that softens the transmission map.
    pub fn dehaze_window_downscaled(&self, rendered_long_edge: f32) -> (usize, usize) {
        let window = DETAIL_DEHAZE_EDGE_RATIO * rendered_long_edge / DETAIL_LOCAL_DOWNSCALE as f32;
        let radius = window.round().clamp(1.0, 64.0) as usize;
        let soften = (radius / 4).max(1);
        (radius, soften)
    }
}

/// Transmission of the haze estimate for one pixel. `strength` is the slider in
/// -1..1; a negative value thickens the veil again, which is what a negative
/// Dehaze control does.
#[inline]
pub fn dehaze_transmission(dark_channel: f32, strength: f32) -> f32 {
    let omega = DETAIL_DEHAZE_OMEGA * strength.clamp(-1.0, 1.0);
    (1.0 - omega * dark_channel.clamp(0.0, 1.0)).clamp(0.2, 5.0)
}

/// Dark Channel Prior restoration with the display white as the atmospheric
/// light: I = J * t + A * (1 - t) solved for J.
#[inline]
pub fn dehaze_restore(color: [f32; 3], transmission: f32) -> [f32; 3] {
    let safe = transmission.max(1e-3);
    [
        (color[0] - 1.0) / safe + 1.0,
        (color[1] - 1.0) / safe + 1.0,
        (color[2] - 1.0) / safe + 1.0,
    ]
}

/// Masking weight of the sharpening stage: 1 everywhere at zero masking, and
/// progressively edge-only as the slider rises.
#[inline]
pub fn sharpening_mask_weight(gradient: f32, masking: f32) -> f32 {
    let masking = (masking / 100.0).clamp(0.0, 1.0);
    if masking <= 1e-4 {
        return 1.0;
    }
    let threshold = DETAIL_MASK_MIN_EDGE + masking * DETAIL_MASK_EDGE_SPAN;
    shader_smoothstep(threshold, threshold + DETAIL_MASK_SOFTNESS, gradient)
}

/// Luma gradient used by the sharpening mask: central differences one pixel
/// apart, summed over both axes. Both renderers sample the same taps.
#[inline]
pub fn detail_gradient(left: f32, right: f32, up: f32, down: f32) -> f32 {
    (right - left).abs() + (down - up).abs()
}

/// One pixel of the shared Detail stage.
///
/// Order matches the panel: the haze estimate restores the veil the scene never
/// had, Clarity and Texture add local contrast, and Sharpening comes last so it
/// reads the finished tone. Lightroom's sliders are reproduced as closely as an
/// open implementation allows; the radii below are the documented mapping.
#[allow(clippy::too_many_arguments)]
pub fn apply_detail_pixel(
    base: [f32; 3],
    fine_blur: [f32; 3],
    texture_blur: [f32; 3],
    coarse_blur: [f32; 3],
    clarity_blur: [f32; 3],
    dark_channel: f32,
    gradient: f32,
    sprocket_mask: f32,
    settings: &DetailSettings,
) -> [f32; 3] {
    if settings.is_neutral() {
        return base;
    }
    let guard = 1.0 - sprocket_mask.clamp(0.0, 1.0);
    let mut color = if settings.dehaze.abs() > 1e-4 {
        dehaze_restore(
            base,
            dehaze_transmission(dark_channel, settings.dehaze / 100.0),
        )
    } else {
        base
    };

    if settings.clarity.abs() > 1e-4 && guard > 0.0 {
        // Midtone weighting keeps Clarity off the clipped ends of the frame,
        // where a local-contrast push would only crush detail further.
        let blurred_luma = density_luma(clarity_blur);
        let midtone = (1.0 - (2.0 * blurred_luma - 1.0).powi(2)).clamp(0.0, 1.0);
        let gain = (settings.clarity / 100.0) * DETAIL_CLARITY_GAIN * midtone * guard;
        for channel in 0..3 {
            color[channel] += (base[channel] - clarity_blur[channel]) * gain;
        }
    }

    if settings.texture.abs() > 1e-4 && guard > 0.0 {
        let gain = (settings.texture / 100.0) * DETAIL_TEXTURE_GAIN * guard;
        for channel in 0..3 {
            color[channel] += (base[channel] - texture_blur[channel]) * gain;
        }
    }

    if settings.sharpening_amount > 0.0 && guard > 0.0 {
        let weight = sharpening_mask_weight(gradient, settings.sharpening_masking) * guard;
        let gain = (settings.sharpening_amount / 100.0) * DETAIL_SHARPENING_GAIN * weight;
        let detail_blend =
            (settings.sharpening_detail / 100.0).clamp(0.0, 1.0) * DETAIL_SHARPENING_DETAIL_GAIN;
        for channel in 0..3 {
            let coarse = base[channel] - coarse_blur[channel];
            let fine = base[channel] - fine_blur[channel];
            color[channel] += (coarse + fine * detail_blend) * gain;
        }
    }

    [
        color[0].clamp(0.0, 1.0),
        color[1].clamp(0.0, 1.0),
        color[2].clamp(0.0, 1.0),
    ]
}

#[cfg(test)]
mod tests {
    use super::{
        apply_detail_pixel, apply_homography, apply_lens_distortion_uv,
        apply_linear_highlight_adjustment_rgb, apply_perspective_uv, apply_post_gamma_adjustments,
        contrast_channel, dehaze_restore, dehaze_transmission, density_luma,
        finite_soft_clip_highlights_rgb, neutral_density_bounds, neutralize_rgb,
        normalize_density_channel, normalize_density_channel_unclamped_high,
        rolloff_highlight_shoulder_rgb, shader_homography, sharpening_mask_weight,
        sprocket_white_mask, trim_density_endpoints, DetailSettings, ToneCurveSet,
        LINEAR_HIGHLIGHT_PIVOT,
    };

    #[test]
    fn density_tone_curve_matches_shader_formula() {
        let neutral = normalize_density_channel(1.0, 0.0, 2.0, 0.0, 0.0, 1.0);
        assert!((neutral - 0.5).abs() < 1e-6);

        let lifted_shadows = normalize_density_channel(0.5, 0.0, 2.0, 0.0, 0.5, 1.0);
        assert!((lifted_shadows - 0.3203125).abs() < 1e-6);
    }

    #[test]
    fn density_tone_curve_is_finite_for_degenerate_ranges() {
        let output = normalize_density_channel(1.0, 0.5, 0.5, 0.0, 0.0, 0.0);
        assert!(output.is_finite());
    }

    #[test]
    fn modern_density_normalization_keeps_highlight_headroom() {
        assert_eq!(
            normalize_density_channel_unclamped_high(1.05, 0.0, 1.0),
            1.05
        );
        assert_eq!(
            normalize_density_channel_unclamped_high(1.80, 0.0, 1.0),
            1.80
        );
        assert_eq!(
            normalize_density_channel_unclamped_high(-0.2, 0.0, 1.0),
            0.0
        );
        assert_eq!(
            normalize_density_channel_unclamped_high(f32::NAN, 0.0, 1.0),
            0.0
        );
        assert_eq!(normalize_density_channel_unclamped_high(1.0, 1.0, 1.0), 0.0);
        // The legacy helper keeps its historical ceiling.
        assert_eq!(normalize_density_channel(1.8, 0.0, 1.0, 0.0, 0.0, 1.0), 1.0);
    }

    #[test]
    fn linear_highlight_adjustment_is_identity_at_and_below_the_pivot() {
        for rgb in [[0.0, 0.0, 0.0], [0.35, 0.45, 0.62], [0.62; 3]] {
            assert_eq!(apply_linear_highlight_adjustment_rgb(rgb, -0.75), rgb);
            assert_eq!(apply_linear_highlight_adjustment_rgb(rgb, 0.60), rgb);
        }
        let clamped = apply_linear_highlight_adjustment_rgb([-0.2, 0.1, 0.3], -0.75);
        assert_eq!(clamped, [0.0, 0.1, 0.3]);
    }

    #[test]
    fn linear_highlight_recovery_is_monotonic_neutral_and_per_channel() {
        let low = [0.30, LINEAR_HIGHLIGHT_PIVOT, 0.10];
        let at_pivot = apply_linear_highlight_adjustment_rgb(low, -0.75);
        assert_eq!(at_pivot, low);

        let mut previous = LINEAR_HIGHLIGHT_PIVOT;
        for value in [0.63, 0.90, 1.20, 1.80, 3.00] {
            let input = [value; 3];
            let output = apply_linear_highlight_adjustment_rgb(input, -0.75);
            assert!(output[0] > previous && output[0] < value);
            assert_eq!(output[0], output[1]);
            assert_eq!(output[1], output[2]);
            previous = output[0];
        }

        let mixed = apply_linear_highlight_adjustment_rgb([0.50, 0.90, 1.30], -0.75);
        assert_eq!(mixed[0], 0.50);
        assert!(mixed[1] < 0.90 && mixed[2] < 1.30);
        assert!(mixed[1] / mixed[0] < 0.90 / 0.50);

        let epsilon = 1e-5;
        let below =
            apply_linear_highlight_adjustment_rgb([LINEAR_HIGHLIGHT_PIVOT - epsilon; 3], -0.75);
        let above =
            apply_linear_highlight_adjustment_rgb([LINEAR_HIGHLIGHT_PIVOT + epsilon; 3], -0.75);
        assert!((above[0] - below[0]).abs() < 4.0 * epsilon);
    }

    #[test]
    fn linear_highlight_boost_never_overflows_or_inverts() {
        let mut previous = LINEAR_HIGHLIGHT_PIVOT;
        for maximum in [0.63, 0.65, 0.90, 1.30, 2.00] {
            let input = [maximum, maximum * 0.50, maximum * 0.25];
            let output = apply_linear_highlight_adjustment_rgb(input, 0.60);
            assert!(output[0] > previous && output[0] > input[0]);
            assert!(output.iter().all(|value| value.is_finite() && *value > 0.0));
            for channel in 0..3 {
                assert!((output[channel] / output[0] - input[channel] / input[0]).abs() < 1e-6);
            }
            previous = output[0];
        }
        let neutral = apply_linear_highlight_adjustment_rgb([0.9; 3], 0.60);
        assert_eq!(neutral[0], neutral[1]);
        assert_eq!(neutral[1], neutral[2]);
    }

    #[test]
    fn finite_soft_clip_is_identity_below_the_knee_and_reaches_white() {
        for rgb in [[0.0, 0.0, 0.0], [0.2, 0.6, 0.88], [0.88; 3]] {
            assert_eq!(finite_soft_clip_highlights_rgb(rgb, 0.88), rgb);
        }
        let shoulder = finite_soft_clip_highlights_rgb([0.95, 1.0, 1.05], 0.88);
        for (actual, expected) in shoulder.into_iter().zip([0.9397917, 0.97, 0.9897917]) {
            assert!((actual - expected).abs() < 1e-6);
        }
        assert_eq!(
            finite_soft_clip_highlights_rgb([1.12, 1.3, 2.0], 0.88),
            [1.0; 3]
        );
        let clamped = finite_soft_clip_highlights_rgb([-0.2, 0.5, 0.9], 0.88);
        assert!((clamped[0] - 0.0).abs() < 1e-6);
        assert!((clamped[1] - 0.5).abs() < 1e-6);
        assert!((clamped[2] - 0.8991667).abs() < 1e-6);
        assert_eq!(
            finite_soft_clip_highlights_rgb([f32::NAN, 0.5, 0.9], 0.88),
            [0.0; 3]
        );
    }

    #[test]
    fn highlight_shoulder_is_identity_below_the_knee() {
        for rgb in [[0.0, 0.0, 0.0], [0.2, 0.6, 0.8], [0.75, 0.85, 0.1]] {
            assert_eq!(rolloff_highlight_shoulder_rgb(rgb, 0.85, 0.990), rgb);
        }
    }

    #[test]
    fn highlight_shoulder_compresses_without_changing_channel_ratios() {
        let input = [0.75, 0.90, 1.25];
        let output = rolloff_highlight_shoulder_rgb(input, 0.85, 0.990);
        let maximum = output[0].max(output[1]).max(output[2]);
        assert!(maximum <= 0.990);
        assert!(maximum > 0.85);
        for channel in 0..3 {
            assert!((output[channel] / output[1] - input[channel] / input[1]).abs() < 1e-6);
        }
    }

    #[test]
    fn contrast_pivots_on_mid_grey_without_flattening_the_frame() {
        assert!((contrast_channel(0.5, 0.5) - 0.5).abs() < 1e-6);
        assert!((contrast_channel(0.75, 0.25) - 0.8125).abs() < 1e-6);
        assert!((contrast_channel(0.25, 0.25) - 0.1875).abs() < 1e-6);

        // The weakest setting keeps half of the original separation from mid
        // grey, and the clamp refuses to flatten the frame any further.
        assert!((contrast_channel(0.75, -0.5) - 0.625).abs() < 1e-6);
        assert!((contrast_channel(0.75, -4.0) - 0.625).abs() < 1e-6);
    }

    #[test]
    fn post_gamma_color_defaults_are_neutral() {
        let rgb = [0.2, 0.5, 0.8];
        let adjusted = apply_post_gamma_adjustments(rgb, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0);
        for channel in 0..3 {
            assert!((adjusted[channel] - rgb[channel]).abs() < 1e-4);
        }
    }

    #[test]
    fn minimum_saturation_produces_grayscale() {
        let adjusted = apply_post_gamma_adjustments([0.2, 0.5, 0.8], 0.0, 0.0, 0.0, -1.0, 0.0, 0.0);
        assert!((adjusted[0] - adjusted[1]).abs() < 1e-6);
        assert!((adjusted[1] - adjusted[2]).abs() < 1e-6);
    }

    #[test]
    fn neutral_perspective_preserves_uv() {
        let mapped = apply_perspective_uv([0.2, 0.8], 0.0, 0.0, 0.0, 1.0).unwrap();
        assert!((mapped[0] - 0.2).abs() < 1e-6);
        assert!((mapped[1] - 0.8).abs() < 1e-6);
    }

    #[test]
    fn perspective_and_scale_produce_finite_coordinates() {
        let mapped = apply_perspective_uv([0.0, 1.0], 50.0, -35.0, 20.0, 1.25).unwrap();
        assert!(mapped.iter().all(|value| value.is_finite()));
        assert_ne!(mapped, [0.0, 1.0]);
    }

    #[test]
    fn lens_distortion_mapping_is_identity_at_zero() {
        let mapped = apply_lens_distortion_uv([0.2, 0.8], 0.0).unwrap();
        assert!((mapped[0] - 0.2).abs() < 1e-6);
        assert!((mapped[1] - 0.8).abs() < 1e-6);
    }

    #[test]
    fn barrel_and_pincushion_corrections_move_edges_in_opposite_directions() {
        let barrel = apply_lens_distortion_uv([0.1, 0.5], -50.0).unwrap();
        let pincushion = apply_lens_distortion_uv([0.1, 0.5], 50.0).unwrap();
        assert!(barrel[0] > 0.1);
        assert!(pincushion[0] < 0.1);
    }

    #[test]
    fn black_and_white_density_bounds_are_channel_neutral() {
        let (d_min, d_max) = neutral_density_bounds([0.2, 0.4, 0.6], [1.8, 2.1, 2.4]);
        assert!((d_min - density_luma([0.2, 0.4, 0.6])).abs() < 1e-6);
        assert!((d_max - density_luma([1.8, 2.1, 2.4])).abs() < 1e-6);
        assert!(density_luma([0.0, 1.0, 0.0]) > density_luma([1.0, 0.0, 0.0]));
        assert!(density_luma([0.0, 1.0, 0.0]) > density_luma([0.0, 0.0, 1.0]));
    }

    #[test]
    fn neutralize_rgb_removes_lut_tint() {
        let neutral = neutralize_rgb([0.8, 0.5, 0.2]);
        assert!((neutral[0] - neutral[1]).abs() < 1e-6);
        assert!((neutral[1] - neutral[2]).abs() < 1e-6);
    }

    #[test]
    fn master_density_trim_stays_channel_neutral_across_unequal_spans() {
        // A sampled Roll measures a different span per channel.
        let d_min = [0.05, 0.02, -0.01];
        let d_max = [1.25, 1.52, 1.99];
        let min_offset = -0.03;
        let max_offset = 0.09;
        let span_luma = density_luma([
            d_max[0] - d_min[0],
            d_max[1] - d_min[1],
            d_max[2] - d_min[2],
        ]);
        let (trimmed_min, trimmed_max) =
            trim_density_endpoints(d_min, d_max, min_offset, max_offset);
        // A pixel that sits halfway through every channel's window must stay
        // halfway through it: the trim is exposure and contrast, never colour.
        let mut normalized_after = [0.0f32; 3];
        for channel in 0..3 {
            let density = d_min[channel] + 0.5 * (d_max[channel] - d_min[channel]);
            let before = (density - d_min[channel]) / (d_max[channel] - d_min[channel]);
            let after =
                (density - trimmed_min[channel]) / (trimmed_max[channel] - trimmed_min[channel]);
            normalized_after[channel] = after;
            assert!((before - 0.5).abs() < 1e-6);
        }
        let expected =
            (0.5 - min_offset / span_luma) / (1.0 + (max_offset - min_offset) / span_luma);
        for channel in 0..3 {
            assert!(
                (normalized_after[channel] - expected).abs() < 1e-6,
                "channel {channel} moved to {} instead of {expected}",
                normalized_after[channel]
            );
        }
        // The plain uniform shift this replaced would have split the channels.
        let shifted = [
            (d_min[0] + 0.5 * (d_max[0] - d_min[0]) - (d_min[0] + min_offset))
                / ((d_max[0] + max_offset) - (d_min[0] + min_offset)),
            (d_min[1] + 0.5 * (d_max[1] - d_min[1]) - (d_min[1] + min_offset))
                / ((d_max[1] + max_offset) - (d_min[1] + min_offset)),
            (d_min[2] + 0.5 * (d_max[2] - d_min[2]) - (d_min[2] + min_offset))
                / ((d_max[2] + max_offset) - (d_min[2] + min_offset)),
        ];
        assert!((shifted[0] - shifted[2]).abs() > 1e-3);
    }

    #[test]
    fn master_density_trim_keeps_the_plain_shift_on_equal_spans() {
        // Every route that derives one shared window per channel must keep its
        // historical behaviour.
        let (trimmed_min, trimmed_max) = trim_density_endpoints([0.1; 3], [2.0; 3], -0.2, 0.2);
        for channel in 0..3 {
            assert!((trimmed_min[channel] - -0.1).abs() < 1e-6);
            assert!((trimmed_max[channel] - 2.2).abs() < 1e-6);
        }
        let (collapsed_min, collapsed_max) = trim_density_endpoints([0.5; 3], [0.5; 3], 0.05, 0.05);
        assert!((collapsed_min[0] - 0.55).abs() < 1e-6);
        assert!((collapsed_max[0] - 0.55).abs() < 1e-6);
    }

    #[test]
    fn tint_moves_between_green_and_magenta() {
        let green = apply_post_gamma_adjustments([0.5; 3], 0.0, 0.0, 0.0, 0.0, 0.0, -1.0);
        assert!(green[1] > green[0] && green[1] > green[2]);

        let magenta = apply_post_gamma_adjustments([0.5; 3], 0.0, 0.0, 0.0, 0.0, 0.0, 1.0);
        assert!(magenta[0] > magenta[1] && magenta[2] > magenta[1]);
    }

    #[test]
    fn default_calibration_is_identity() {
        let matrix = shader_homography([[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]]);
        let output = apply_homography(&matrix, [0.25, 0.75]).unwrap();
        assert!((output[0] - 0.25).abs() < 1e-6);
        assert!((output[1] - 0.75).abs() < 1e-6);
    }

    #[test]
    fn trapezoid_calibration_maps_bounding_corners_to_quad() {
        let points = [[0.2, 0.1], [0.8, 0.2], [0.9, 0.9], [0.1, 0.8]];
        let matrix = shader_homography(points);
        let top_left = apply_homography(&matrix, [0.1, 0.1]).unwrap();
        let bottom_right = apply_homography(&matrix, [0.9, 0.9]).unwrap();
        assert!((top_left[0] - points[0][0]).abs() < 1e-5);
        assert!((top_left[1] - points[0][1]).abs() < 1e-5);
        assert!((bottom_right[0] - points[2][0]).abs() < 1e-5);
        assert!((bottom_right[1] - points[2][1]).abs() < 1e-5);
    }

    #[test]
    fn sprocket_mask_matches_shader_endpoints() {
        assert!((sprocket_white_mask(0.0, 0.1, 0.05) - 1.0).abs() < 1e-6);
        assert_eq!(sprocket_white_mask(0.2, 0.1, 0.05), 0.0);
    }

    #[test]
    fn identity_curve_passes_the_display_value_through() {
        let curves = ToneCurveSet::identity();
        assert!(curves.is_identity());
        for value in [0.0, 0.18, 0.5, 0.82, 1.0] {
            let mapped = curves.apply([value, value, value]);
            assert!(
                (mapped[0] - value).abs() < 2e-3,
                "{value} moved to {} on the identity curve",
                mapped[0]
            );
        }
    }

    #[test]
    fn point_curve_is_monotone_and_keeps_its_endpoints() {
        // A control point dragged above its neighbour's tone must not push the
        // curve backwards between them.
        let curves = ToneCurveSet::new(
            &[[0.0, 0.0], [0.35, 0.55], [0.65, 0.6], [1.0, 1.0]],
            &[[0.0, 0.0], [1.0, 1.0]],
            &[[0.0, 0.0], [1.0, 1.0]],
            &[[0.0, 0.0], [1.0, 1.0]],
        );
        let mut previous = -1.0;
        for step in 0..=100 {
            let x = step as f32 / 100.0;
            let mapped = curves.apply([x, x, x])[0];
            assert!(mapped >= previous - 1e-4, "curve decreased at {x}");
            assert!((0.0..=1.0).contains(&mapped));
            previous = mapped;
        }
        assert!(curves.apply([0.0; 3])[0].abs() < 2e-3);
        assert!((curves.apply([1.0; 3])[0] - 1.0).abs() < 2e-3);
    }

    #[test]
    fn lifted_black_point_and_channel_curves_are_separate_controls() {
        let lifted = ToneCurveSet::new(
            &[[0.0, 0.2], [1.0, 1.0]],
            &[[0.0, 0.0], [1.0, 1.0]],
            &[[0.0, 0.0], [1.0, 1.0]],
            &[[0.0, 0.0], [1.0, 1.0]],
        );
        assert!((lifted.apply([0.0; 3])[0] - 0.2).abs() < 5e-3);

        // The per-channel curve runs first and the composite curve reads its
        // result: a red lift followed by a global lift compounds.
        let channels = ToneCurveSet::new(
            &[[0.0, 0.0], [1.0, 0.5]],
            &[[0.0, 0.5], [1.0, 1.0]],
            &[[0.0, 0.0], [1.0, 1.0]],
            &[[0.0, 0.0], [1.0, 1.0]],
        );
        let mapped = channels.apply([0.0, 0.0, 0.0]);
        assert!((mapped[0] - 0.25).abs() < 5e-3, "red was {}", mapped[0]);
        assert!(mapped[1].abs() < 5e-3);
    }

    #[test]
    fn detail_stage_is_neutral_at_rest() {
        let settings = DetailSettings::neutral(1.0);
        assert!(settings.is_neutral());
        let base = [0.3, 0.4, 0.5];
        assert_eq!(
            apply_detail_pixel(base, base, base, base, base, 0.2, 0.0, 0.0, &settings),
            base
        );
    }

    #[test]
    fn sharpening_adds_local_contrast_around_a_step() {
        let mut settings = DetailSettings::neutral(1.0);
        settings.sharpening_amount = 100.0;
        settings.sharpening_detail = 0.0;
        // A dark pixel beside a bright one: the high pass is negative, so the
        // USM correction pushes it further down.
        let darkened = apply_detail_pixel(
            [0.4; 3], [0.4; 3], [0.4; 3], [0.55; 3], [0.4; 3], 0.0, 0.0, 0.0, &settings,
        );
        assert!(
            darkened[0] < 0.4,
            "expected a darker edge, got {}",
            darkened[0]
        );
    }

    #[test]
    fn masking_keeps_flat_areas_out_of_the_sharpening() {
        assert!((sharpening_mask_weight(0.0, 0.0) - 1.0).abs() < 1e-6);
        assert_eq!(sharpening_mask_weight(0.02, 100.0), 0.0);
        assert!((sharpening_mask_weight(0.6, 100.0) - 1.0).abs() < 1e-3);
        let partial = sharpening_mask_weight(0.20, 50.0);
        assert!(
            partial > 0.0 && partial < 1.0,
            "partial weight was {partial}"
        );
    }

    #[test]
    fn dehaze_pulls_the_veil_out_and_pushes_it_back() {
        let hazy = [0.6, 0.65, 0.7];
        let removed = dehaze_restore(hazy, dehaze_transmission(0.6, 0.5));
        assert!(removed[0] < hazy[0]);
        // White stays white whatever the transmission is.
        let white = dehaze_restore([1.0; 3], dehaze_transmission(0.9, 1.0));
        for channel in white {
            assert!((channel - 1.0).abs() < 1e-5);
        }
        let added = dehaze_restore(hazy, dehaze_transmission(0.6, -0.5));
        assert!(added[0] > hazy[0] || added[1] > hazy[1]);
    }

    #[test]
    fn curvature_scale_shrinks_the_detail_radii_on_a_proxy() {
        let full = DetailSettings {
            sharpening_amount: 50.0,
            scale: 1.0,
            ..DetailSettings::neutral(1.0)
        };
        let preview = DetailSettings {
            scale: 0.43,
            ..full
        };
        assert!(preview.sharpening_sigma() < full.sharpening_sigma());
        let (window, soften) = full.dehaze_window_downscaled(2560.0);
        assert!(window >= 1 && soften >= 1);
        assert!(window > soften);
    }
}
