use nalgebra::Matrix3;

pub type Homography3 = [[f32; 3]; 3];

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

/// Apply the creative controls that intentionally live after display gamma.
/// This mirrors `applyPostGammaAdjustments` in the WebGL fragment shader.
#[inline]
pub fn apply_post_gamma_adjustments(
    rgb: [f32; 3],
    highlights: f32,
    shadows: f32,
    saturation: f32,
    temperature: f32,
    tint: f32,
) -> [f32; 3] {
    let mut adjusted = rgb.map(|value| tone_normalized_channel(value, highlights, shadows));

    let temperature = temperature.clamp(-1.0, 1.0);
    adjusted[0] *= 1.0 + temperature * 0.20;
    adjusted[2] *= 1.0 - temperature * 0.20;

    let tint = tint.clamp(-1.0, 1.0);
    adjusted[0] *= 1.0 + tint * 0.10;
    adjusted[1] *= 1.0 - tint * 0.20;
    adjusted[2] *= 1.0 + tint * 0.10;

    let luma = 0.299 * adjusted[0] + 0.587 * adjusted[1] + 0.114 * adjusted[2];
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

#[cfg(test)]
mod tests {
    use super::{
        apply_homography, apply_post_gamma_adjustments, normalize_density_channel,
        shader_homography, sprocket_white_mask,
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
    fn post_gamma_color_defaults_are_neutral() {
        let rgb = [0.2, 0.5, 0.8];
        let adjusted = apply_post_gamma_adjustments(rgb, 0.0, 0.0, 0.0, 0.0, 0.0);
        for channel in 0..3 {
            assert!((adjusted[channel] - rgb[channel]).abs() < 1e-4);
        }
    }

    #[test]
    fn minimum_saturation_produces_grayscale() {
        let adjusted = apply_post_gamma_adjustments([0.2, 0.5, 0.8], 0.0, 0.0, -1.0, 0.0, 0.0);
        assert!((adjusted[0] - adjusted[1]).abs() < 1e-6);
        assert!((adjusted[1] - adjusted[2]).abs() < 1e-6);
    }

    #[test]
    fn tint_moves_between_green_and_magenta() {
        let green = apply_post_gamma_adjustments([0.5; 3], 0.0, 0.0, 0.0, 0.0, -1.0);
        assert!(green[1] > green[0] && green[1] > green[2]);

        let magenta = apply_post_gamma_adjustments([0.5; 3], 0.0, 0.0, 0.0, 0.0, 1.0);
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
}
