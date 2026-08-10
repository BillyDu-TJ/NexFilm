//! Colour-space definitions and strictly linear RGB conversions.
//!
//! The film inversion pipeline works on linear-light RGB.  This module keeps
//! primaries, reference white, transfer functions, chromatic adaptation and
//! ICC profile generation in one place so that RAW decoding, preview and
//! export cannot silently disagree about what three channel values mean.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ColorSpaceId {
    SRgb,
    DisplayP3,
    AdobeRgb,
    Rec2020,
    ProPhotoRgb,
    /// LibRaw's native ProPhoto output is ProPhoto D65; standard ROMM RGB is D50.
    ProPhotoRgbD65,
    Aces2065,
    AcesCg,
}

/// Canonical linear-light RGB coordinates used before logarithmic film-density
/// conversion. This must stay fixed: changing primaries before `-log10(T)`
/// changes the density measurement itself rather than merely its presentation.
pub const DENSITY_CAPTURE_WORKING_SPACE: &str = "linear-srgb";
pub const DENSITY_CAPTURE_PROFILE: ColorSpaceId = ColorSpaceId::SRgb;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WorkingSpaceId {
    LinearSRgb,
    LinearDisplayP3,
    LinearAdobeRgb,
    LinearRec2020,
    LinearProPhotoRgb,
    LinearAces,
    LinearAcesCg,
}

#[derive(Clone, Copy, Debug)]
enum Transfer {
    Linear,
    SRgb,
    AdobeRgb,
    Rec2020,
    ProPhoto,
}

#[derive(Clone, Copy, Debug)]
struct Profile {
    name: &'static str,
    primaries: [[f64; 2]; 3],
    white: [f64; 2],
    transfer: Transfer,
}

const D50: [f64; 2] = [0.3457, 0.3585];
const D60: [f64; 2] = [0.32168, 0.33767];
const D65: [f64; 2] = [0.3127, 0.3290];

const PROFILES: [Profile; 8] = [
    Profile {
        name: "sRGB IEC 61966-2.1",
        primaries: [[0.64, 0.33], [0.30, 0.60], [0.15, 0.06]],
        white: D65,
        transfer: Transfer::SRgb,
    },
    Profile {
        name: "Display P3",
        primaries: [[0.68, 0.32], [0.265, 0.69], [0.15, 0.06]],
        white: D65,
        transfer: Transfer::SRgb,
    },
    Profile {
        name: "Adobe RGB (1998)",
        primaries: [[0.64, 0.33], [0.21, 0.71], [0.15, 0.06]],
        white: D65,
        transfer: Transfer::AdobeRgb,
    },
    Profile {
        name: "ITU-R BT.2020",
        primaries: [[0.708, 0.292], [0.170, 0.797], [0.131, 0.046]],
        white: D65,
        transfer: Transfer::Rec2020,
    },
    Profile {
        name: "ProPhoto RGB (ROMM RGB)",
        primaries: [[0.7347, 0.2653], [0.1596, 0.8404], [0.0366, 0.0001]],
        white: D50,
        transfer: Transfer::ProPhoto,
    },
    Profile {
        name: "LibRaw ProPhoto RGB D65",
        primaries: [[0.7347, 0.2653], [0.1596, 0.8404], [0.0366, 0.0001]],
        white: D65,
        transfer: Transfer::Linear,
    },
    Profile {
        name: "ACES2065-1 (AP0)",
        primaries: [[0.7347, 0.2653], [0.0, 1.0], [0.0001, -0.0770]],
        white: D60,
        transfer: Transfer::Linear,
    },
    Profile {
        name: "ACEScg (AP1)",
        primaries: [[0.713, 0.293], [0.165, 0.830], [0.128, 0.044]],
        white: D60,
        transfer: Transfer::Linear,
    },
];

fn profile(id: ColorSpaceId) -> Profile {
    PROFILES[match id {
        ColorSpaceId::SRgb => 0,
        ColorSpaceId::DisplayP3 => 1,
        ColorSpaceId::AdobeRgb => 2,
        ColorSpaceId::Rec2020 => 3,
        ColorSpaceId::ProPhotoRgb => 4,
        ColorSpaceId::ProPhotoRgbD65 => 5,
        ColorSpaceId::Aces2065 => 6,
        ColorSpaceId::AcesCg => 7,
    }]
}

pub fn parse_working_space(value: &str) -> Option<WorkingSpaceId> {
    match value.trim().to_ascii_lowercase().as_str() {
        "linear-srgb" | "srgb" | "linear-srgb-d65" => Some(WorkingSpaceId::LinearSRgb),
        "linear-display-p3" | "display-p3" | "p3" => Some(WorkingSpaceId::LinearDisplayP3),
        "linear-adobe-rgb" | "adobe-rgb" | "adobe-rgb-1998" | "adobergb" => {
            Some(WorkingSpaceId::LinearAdobeRgb)
        }
        "linear-rec2020" | "rec2020" | "rec.2020" | "bt2020" => Some(WorkingSpaceId::LinearRec2020),
        "linear-prophoto" | "linear-prophoto-rgb" | "prophoto" | "prophoto-rgb" => {
            Some(WorkingSpaceId::LinearProPhotoRgb)
        }
        "linear-aces" | "aces" | "aces2065" | "aces-ap0" => Some(WorkingSpaceId::LinearAces),
        "linear-acescg" | "acescg" | "aces-ap1" => Some(WorkingSpaceId::LinearAcesCg),
        _ => None,
    }
}

pub fn canonical_working_space(value: &str) -> Option<&'static str> {
    match parse_working_space(value)? {
        WorkingSpaceId::LinearSRgb => Some("linear-srgb"),
        WorkingSpaceId::LinearDisplayP3 => Some("linear-display-p3"),
        WorkingSpaceId::LinearAdobeRgb => Some("linear-adobe-rgb"),
        WorkingSpaceId::LinearRec2020 => Some("linear-rec2020"),
        WorkingSpaceId::LinearProPhotoRgb => Some("linear-prophoto-rgb"),
        WorkingSpaceId::LinearAces => Some("linear-aces"),
        WorkingSpaceId::LinearAcesCg => Some("linear-acescg"),
    }
}

pub fn parse_output_space(value: &str) -> Option<ColorSpaceId> {
    match value.trim().to_ascii_lowercase().as_str() {
        "srgb" | "s-rgb" => Some(ColorSpaceId::SRgb),
        "display-p3" | "p3" => Some(ColorSpaceId::DisplayP3),
        "adobe-rgb" | "adobe-rgb-1998" | "adobergb" => Some(ColorSpaceId::AdobeRgb),
        "rec2020" | "rec.2020" | "bt2020" => Some(ColorSpaceId::Rec2020),
        "prophoto" | "prophoto-rgb" | "romm-rgb" => Some(ColorSpaceId::ProPhotoRgb),
        "acescg" | "aces-ap1" => Some(ColorSpaceId::AcesCg),
        "aces" | "aces2065" | "aces-ap0" => Some(ColorSpaceId::Aces2065),
        _ => None,
    }
}

pub fn canonical_output_space(value: &str) -> Option<&'static str> {
    match parse_output_space(value)? {
        ColorSpaceId::SRgb => Some("srgb"),
        ColorSpaceId::DisplayP3 => Some("display-p3"),
        ColorSpaceId::AdobeRgb => Some("adobe-rgb"),
        ColorSpaceId::Rec2020 => Some("rec2020"),
        ColorSpaceId::ProPhotoRgb => Some("prophoto-rgb"),
        ColorSpaceId::ProPhotoRgbD65 => None,
        ColorSpaceId::Aces2065 => Some("aces"),
        ColorSpaceId::AcesCg => Some("acescg"),
    }
}

pub fn working_profile(id: WorkingSpaceId) -> ColorSpaceId {
    match id {
        WorkingSpaceId::LinearSRgb => ColorSpaceId::SRgb,
        WorkingSpaceId::LinearDisplayP3 => ColorSpaceId::DisplayP3,
        WorkingSpaceId::LinearAdobeRgb => ColorSpaceId::AdobeRgb,
        WorkingSpaceId::LinearRec2020 => ColorSpaceId::Rec2020,
        WorkingSpaceId::LinearProPhotoRgb => ColorSpaceId::ProPhotoRgb,
        WorkingSpaceId::LinearAces => ColorSpaceId::Aces2065,
        WorkingSpaceId::LinearAcesCg => ColorSpaceId::AcesCg,
    }
}

pub fn working_luma(value: &str) -> [f32; 3] {
    let profile_data = parse_working_space(value)
        .map(working_profile)
        .map(profile)
        .unwrap_or_else(|| profile(ColorSpaceId::SRgb));
    let matrix = native_to_xyz(profile_data);
    [matrix[3] as f32, matrix[4] as f32, matrix[5] as f32]
}

/// LibRaw's documented output-color values: sRGB=1, Adobe=2, Wide=3,
/// ProPhoto D65=4, XYZ=5 and ACES=6.  Spaces without a native LibRaw matrix use
/// linear ACES AP0 as the loss-minimising interchange space and are converted
/// below with the same matrices used for export.
pub fn libraw_output_color(value: &str) -> Result<i32, String> {
    let space = parse_working_space(value)
        .ok_or_else(|| format!("Unsupported RAW working color space: {value}"))?;
    Ok(match space {
        WorkingSpaceId::LinearSRgb => 1,
        WorkingSpaceId::LinearAdobeRgb => 2,
        WorkingSpaceId::LinearProPhotoRgb => 4,
        WorkingSpaceId::LinearAces
        | WorkingSpaceId::LinearDisplayP3
        | WorkingSpaceId::LinearRec2020
        | WorkingSpaceId::LinearAcesCg => 6,
    })
}

/// Return the actual RGB profile represented by LibRaw's linear output.
/// LibRaw's output-color 4 is ProPhoto D65, not the D50 ROMM profile used for
/// exports, so callers must convert it through this profile before storing it
/// as a standard ProPhoto working image.
pub fn libraw_native_profile(output_color: i32) -> ColorSpaceId {
    match output_color {
        1 => ColorSpaceId::SRgb,
        2 => ColorSpaceId::AdobeRgb,
        4 => ColorSpaceId::ProPhotoRgbD65,
        6 => ColorSpaceId::Aces2065,
        _ => ColorSpaceId::SRgb,
    }
}

pub fn working_requires_aces_conversion(value: &str) -> bool {
    matches!(
        parse_working_space(value),
        Some(
            WorkingSpaceId::LinearDisplayP3
                | WorkingSpaceId::LinearRec2020
                | WorkingSpaceId::LinearAcesCg
        )
    )
}

fn matrix_multiply(a: [f64; 9], b: [f64; 9]) -> [f64; 9] {
    let mut out = [0.0; 9];
    for row in 0..3 {
        for column in 0..3 {
            out[row * 3 + column] = (0..3)
                .map(|index| a[row * 3 + index] * b[index * 3 + column])
                .sum();
        }
    }
    out
}

fn matrix_vector(m: [f64; 9], v: [f64; 3]) -> [f64; 3] {
    [
        m[0] * v[0] + m[1] * v[1] + m[2] * v[2],
        m[3] * v[0] + m[4] * v[1] + m[5] * v[2],
        m[6] * v[0] + m[7] * v[1] + m[8] * v[2],
    ]
}

fn matrix_inverse(m: [f64; 9]) -> [f64; 9] {
    let det = m[0] * (m[4] * m[8] - m[5] * m[7]) - m[1] * (m[3] * m[8] - m[5] * m[6])
        + m[2] * (m[3] * m[7] - m[4] * m[6]);
    assert!(det.abs() > 1e-12, "RGB colour matrix is singular");
    [
        (m[4] * m[8] - m[5] * m[7]) / det,
        (m[2] * m[7] - m[1] * m[8]) / det,
        (m[1] * m[5] - m[2] * m[4]) / det,
        (m[5] * m[6] - m[3] * m[8]) / det,
        (m[0] * m[8] - m[2] * m[6]) / det,
        (m[2] * m[3] - m[0] * m[5]) / det,
        (m[3] * m[7] - m[4] * m[6]) / det,
        (m[1] * m[6] - m[0] * m[7]) / det,
        (m[0] * m[4] - m[1] * m[3]) / det,
    ]
}

fn white_xyz(white: [f64; 2]) -> [f64; 3] {
    [
        white[0] / white[1],
        1.0,
        (1.0 - white[0] - white[1]) / white[1],
    ]
}

fn native_to_xyz(profile: Profile) -> [f64; 9] {
    let columns = profile
        .primaries
        .map(|[x, y]| [x / y, 1.0, (1.0 - x - y) / y]);
    let primary_matrix = [
        columns[0][0],
        columns[1][0],
        columns[2][0],
        columns[0][1],
        columns[1][1],
        columns[2][1],
        columns[0][2],
        columns[1][2],
        columns[2][2],
    ];
    let scale = matrix_vector(matrix_inverse(primary_matrix), white_xyz(profile.white));
    [
        primary_matrix[0] * scale[0],
        primary_matrix[1] * scale[1],
        primary_matrix[2] * scale[2],
        primary_matrix[3] * scale[0],
        primary_matrix[4] * scale[1],
        primary_matrix[5] * scale[2],
        primary_matrix[6] * scale[0],
        primary_matrix[7] * scale[1],
        primary_matrix[8] * scale[2],
    ]
}

fn bradford_adaptation(source: [f64; 2], target: [f64; 2]) -> [f64; 9] {
    if (source[0] - target[0]).abs() < 1e-12 && (source[1] - target[1]).abs() < 1e-12 {
        return [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0];
    }
    let bradford = [
        0.8951, 0.2664, -0.1614, -0.7502, 1.7135, 0.0367, 0.0389, -0.0685, 1.0296,
    ];
    let source_lms = matrix_vector(bradford, white_xyz(source));
    let target_lms = matrix_vector(bradford, white_xyz(target));
    let diagonal = [
        target_lms[0] / source_lms[0],
        0.0,
        0.0,
        0.0,
        target_lms[1] / source_lms[1],
        0.0,
        0.0,
        0.0,
        target_lms[2] / source_lms[2],
    ];
    matrix_multiply(
        matrix_multiply(
            [
                0.9869929, -0.1470543, 0.1599627, 0.4323053, 0.5183603, 0.0492912, -0.0085287,
                0.0400428, 0.9684867,
            ],
            diagonal,
        ),
        bradford,
    )
}

fn source_to_target_matrix(source: Profile, target: Profile) -> [f64; 9] {
    let source_xyz = native_to_xyz(source);
    let target_xyz_inverse = matrix_inverse(native_to_xyz(target));
    let adaptation = bradford_adaptation(source.white, target.white);
    matrix_multiply(target_xyz_inverse, matrix_multiply(adaptation, source_xyz))
}

fn decode_transfer(value: f64, transfer: Transfer) -> f64 {
    match transfer {
        Transfer::Linear => value,
        Transfer::SRgb => {
            let value = value.max(0.0);
            if value <= 0.04045 {
                value / 12.92
            } else {
                ((value + 0.055) / 1.055).powf(2.4)
            }
        }
        Transfer::AdobeRgb => value.max(0.0).powf(563.0 / 256.0),
        Transfer::Rec2020 => {
            let value = value.max(0.0);
            const ALPHA: f64 = 1.09929682680944;
            const BETA: f64 = 0.018053968510807;
            if value < 4.5 * BETA {
                value / 4.5
            } else {
                ((value + ALPHA - 1.0) / ALPHA).powf(1.0 / 0.45)
            }
        }
        Transfer::ProPhoto => {
            let value = value.max(0.0);
            if value < 16.0 / 512.0 {
                value / 16.0
            } else {
                value.powf(1.8)
            }
        }
    }
}

fn encode_transfer(value: f64, transfer: Transfer) -> f64 {
    let value = value.max(0.0);
    match transfer {
        Transfer::Linear => value,
        Transfer::SRgb => {
            if value <= 0.0031308 {
                value * 12.92
            } else {
                1.055 * value.powf(1.0 / 2.4) - 0.055
            }
        }
        Transfer::AdobeRgb => value.powf(256.0 / 563.0),
        Transfer::Rec2020 => {
            const ALPHA: f64 = 1.09929682680944;
            const BETA: f64 = 0.018053968510807;
            if value < BETA {
                4.5 * value
            } else {
                ALPHA * value.powf(0.45) - (ALPHA - 1.0)
            }
        }
        Transfer::ProPhoto => {
            if value < 1.0 / 512.0 {
                16.0 * value
            } else {
                value.powf(1.0 / 1.8)
            }
        }
    }
}

pub fn convert_linear_rgb(rgb: [f32; 3], source: ColorSpaceId, target: ColorSpaceId) -> [f32; 3] {
    apply_linear_matrix(rgb, linear_conversion_matrix(source, target))
}

/// Precompute a linear-light RGB conversion once per image instead of
/// rebuilding the primary/adaptation matrices for every pixel.
pub fn linear_conversion_matrix(source: ColorSpaceId, target: ColorSpaceId) -> [f32; 9] {
    source_to_target_matrix(profile(source), profile(target)).map(|value| value as f32)
}

pub fn apply_linear_matrix(rgb: [f32; 3], matrix: [f32; 9]) -> [f32; 3] {
    [
        matrix[0] * rgb[0] + matrix[1] * rgb[1] + matrix[2] * rgb[2],
        matrix[3] * rgb[0] + matrix[4] * rgb[1] + matrix[5] * rgb[2],
        matrix[6] * rgb[0] + matrix[7] * rgb[1] + matrix[8] * rgb[2],
    ]
}

/// Fit linear-sRGB values into the positive transmission domain used by the
/// logarithmic film-density pipeline. Camera matrices can legitimately
/// produce signed/out-of-gamut values; converting those values directly to
/// u16 would clip each channel independently and create false color spikes.
/// The chroma vector is compressed toward the neutral axis while preserving
/// the Rec. 709 luminance used by Status M.
pub fn compress_linear_srgb_for_density(rgb: [f32; 3]) -> [f32; 3] {
    const ABSOLUTE_FLOOR: f32 = 1.0 / 65_535.0;
    // Keep the darkest channel near, but not pinned to, 2D below luminance.
    // A fixed ratio creates a false flat density plane across every pixel
    // outside the gamut. Squaring the admissible chroma scale progressively
    // desaturates more extreme values and retains a continuous result.
    // A 10% luminance floor caps the artificial channel density at about 1D
    // while retaining visible chroma separation in saturated highlights.
    const CHROMA_FLOOR_RATIO: f32 = 0.10;
    const CEILING: f32 = 1.0 - ABSOLUTE_FLOOR;
    if rgb.iter().any(|value| !value.is_finite()) {
        return [ABSOLUTE_FLOOR; 3];
    }

    let mut rgb = rgb;
    let maximum = rgb.iter().copied().fold(f32::NEG_INFINITY, f32::max);
    if maximum > CEILING {
        let scale = CEILING / maximum;
        rgb = rgb.map(|channel| channel * scale);
    }

    let luma = crate::core_math::density_luma(rgb);
    let floor = ABSOLUTE_FLOOR.max(luma.max(0.0) * CHROMA_FLOOR_RATIO);
    if luma <= floor {
        return [floor; 3];
    }

    let mut scale = 1.0f32;
    for channel in rgb {
        if channel < floor {
            scale = scale.min((luma - floor) / (luma - channel));
        }
    }
    if scale < 1.0 {
        scale *= scale;
    }
    rgb.map(|channel| (luma + (channel - luma) * scale).clamp(floor, CEILING))
}

pub fn convert_encoded_rgb(rgb: [f32; 3], source: ColorSpaceId, target: ColorSpaceId) -> [f32; 3] {
    let source_profile = profile(source);
    let target_profile = profile(target);
    let linear = rgb.map(|value| decode_transfer(value as f64, source_profile.transfer) as f32);
    let converted = convert_linear_rgb(linear, source, target);
    converted.map(|value| encode_transfer(value as f64, target_profile.transfer) as f32)
}

pub fn convert_encoded_to_linear_rgb(
    rgb: [f32; 3],
    source: ColorSpaceId,
    target: ColorSpaceId,
) -> [f32; 3] {
    convert_encoded_to_linear_rgb_with_matrix(rgb, source, linear_conversion_matrix(source, target))
}

pub fn convert_encoded_to_linear_rgb_with_matrix(
    rgb: [f32; 3],
    source: ColorSpaceId,
    matrix: [f32; 9],
) -> [f32; 3] {
    let source_profile = profile(source);
    let linear = rgb.map(|value| decode_transfer(value as f64, source_profile.transfer) as f32);
    apply_linear_matrix(linear, matrix)
}

pub fn encode_linear_rgb(rgb: [f32; 3], target: ColorSpaceId) -> [f32; 3] {
    let transfer = profile(target).transfer;
    rgb.map(|value| encode_transfer(value as f64, transfer) as f32)
}

pub fn decode_linear_rgb(rgb: [f32; 3], source: ColorSpaceId) -> [f32; 3] {
    let transfer = profile(source).transfer;
    rgb.map(|value| decode_transfer(value as f64, transfer) as f32)
}

pub fn profile_name(id: ColorSpaceId) -> &'static str {
    profile(id).name
}

/// Identify the common RGB profiles supported by the application from an ICC
/// payload. The matrix/TRC conversion itself remains defined by the static
/// profiles above; this function only maps the profile's human-readable name
/// to one of those definitions.
pub fn identify_icc_profile(icc: &[u8]) -> Option<ColorSpaceId> {
    if icc.len() < 40 || &icc[36..40] != b"acsp" {
        return None;
    }
    let searchable = icc
        .iter()
        .copied()
        .filter(|byte| *byte != 0)
        .map(|byte| byte.to_ascii_lowercase())
        .collect::<Vec<_>>();
    let searchable = String::from_utf8_lossy(&searchable);
    if searchable.contains("acescg") {
        return Some(ColorSpaceId::AcesCg);
    }
    if searchable.contains("aces2065")
        || (searchable.contains("aces") && searchable.contains("ap0"))
    {
        return Some(ColorSpaceId::Aces2065);
    }
    if searchable.contains("displayp3") || searchable.contains("display p3") {
        return Some(ColorSpaceId::DisplayP3);
    }
    if searchable.contains("adobergb") || searchable.contains("adobe rgb") {
        return Some(ColorSpaceId::AdobeRgb);
    }
    if searchable.contains("rec2020")
        || searchable.contains("rec.2020")
        || searchable.contains("bt.2020")
        || searchable.contains("2020")
    {
        return Some(ColorSpaceId::Rec2020);
    }
    if searchable.contains("prophoto") || searchable.contains("romm") {
        return Some(ColorSpaceId::ProPhotoRgb);
    }
    if searchable.contains("srgb") {
        return Some(ColorSpaceId::SRgb);
    }
    None
}

fn s15fixed16(value: f64) -> [u8; 4] {
    ((value * 65536.0).round() as i32).to_be_bytes()
}

fn xyz_tag(value: [f64; 3]) -> Vec<u8> {
    let mut out = b"XYZ ".to_vec();
    out.extend_from_slice(&[0, 0, 0, 0]);
    for component in value {
        out.extend_from_slice(&s15fixed16(component));
    }
    out
}

fn curve_tag(transfer: Transfer) -> Vec<u8> {
    let mut out = b"curv".to_vec();
    out.extend_from_slice(&[0, 0, 0, 0]);
    if matches!(transfer, Transfer::Linear) {
        out.extend_from_slice(&0u32.to_be_bytes());
        return out;
    }
    let count = 4096u32;
    out.extend_from_slice(&count.to_be_bytes());
    for index in 0..count {
        let encoded = index as f64 / (count - 1) as f64;
        let linear = decode_transfer(encoded, transfer).clamp(0.0, 1.0);
        out.extend_from_slice(&((linear * 65535.0).round() as u16).to_be_bytes());
    }
    out
}

fn text_tag(value: &str) -> Vec<u8> {
    let mut out = b"text".to_vec();
    out.extend_from_slice(&[0, 0, 0, 0]);
    out.extend_from_slice(value.as_bytes());
    out.push(0);
    out
}

fn description_tag(value: &str) -> Vec<u8> {
    let mut out = b"desc".to_vec();
    out.extend_from_slice(&[0, 0, 0, 0]);
    out.extend_from_slice(&(value.len() as u32 + 1).to_be_bytes());
    out.extend_from_slice(value.as_bytes());
    out.push(0);
    // Unicode and Macintosh description fields are optional in the v2
    // structure, but their count fields are mandatory and may be zero.
    out.extend_from_slice(&0u32.to_be_bytes());
    out.extend_from_slice(&0u32.to_be_bytes());
    out.extend_from_slice(&0u16.to_be_bytes());
    out.push(0);
    out.extend_from_slice(&[0u8; 67]);
    out
}

/// Build a compact ICC v2 matrix/TRC RGB profile.  Matrix columns are adapted
/// to the ICC PCS D50 white and the TRC curves map encoded samples to linear
/// light, so the profile describes the pixels emitted by export exactly.
pub fn build_icc_profile(id: ColorSpaceId) -> Vec<u8> {
    let source = profile(id);
    let d50_adaptation = bradford_adaptation(source.white, D50);
    let xyz = matrix_multiply(d50_adaptation, native_to_xyz(source));
    let tags = [
        (b"desc" as &[u8; 4], description_tag(source.name)),
        (b"cprt", text_tag("Copyright 2026 NexFilm")),
        (b"wtpt", xyz_tag(white_xyz(D50))),
        (b"rXYZ", xyz_tag([xyz[0], xyz[3], xyz[6]])),
        (b"gXYZ", xyz_tag([xyz[1], xyz[4], xyz[7]])),
        (b"bXYZ", xyz_tag([xyz[2], xyz[5], xyz[8]])),
        (b"rTRC", curve_tag(source.transfer)),
        (b"gTRC", curve_tag(source.transfer)),
        (b"bTRC", curve_tag(source.transfer)),
    ];
    let header_size = 128usize;
    let tag_table_size = 4 + tags.len() * 12;
    let mut output = vec![0u8; header_size + tag_table_size];
    output[4..8].copy_from_slice(b"nexf");
    output[8..12].copy_from_slice(&0x02100000u32.to_be_bytes());
    output[12..16].copy_from_slice(b"mntr");
    output[16..20].copy_from_slice(b"RGB ");
    output[20..24].copy_from_slice(b"XYZ ");
    output[36..40].copy_from_slice(b"acsp");
    output[64..68].copy_from_slice(&0u32.to_be_bytes());
    output[68..72].copy_from_slice(&s15fixed16(0.9642));
    output[72..76].copy_from_slice(&s15fixed16(1.0));
    output[76..80].copy_from_slice(&s15fixed16(0.8249));
    output[80..84].copy_from_slice(b"NEXF");
    output[128..132].copy_from_slice(&(tags.len() as u32).to_be_bytes());

    for (index, (signature, data)) in tags.iter().enumerate() {
        while output.len() % 4 != 0 {
            output.push(0);
        }
        let offset = output.len() as u32;
        let table_offset = header_size + 4 + index * 12;
        output[table_offset..table_offset + 4].copy_from_slice(*signature);
        output[table_offset + 4..table_offset + 8].copy_from_slice(&offset.to_be_bytes());
        output[table_offset + 8..table_offset + 12]
            .copy_from_slice(&(data.len() as u32).to_be_bytes());
        output.extend_from_slice(data);
    }
    let profile_size = output.len() as u32;
    output[0..4].copy_from_slice(&profile_size.to_be_bytes());
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn common_space_round_trips_are_neutral() {
        let spaces = [
            ColorSpaceId::SRgb,
            ColorSpaceId::DisplayP3,
            ColorSpaceId::AdobeRgb,
            ColorSpaceId::Rec2020,
            ColorSpaceId::ProPhotoRgb,
            ColorSpaceId::Aces2065,
            ColorSpaceId::AcesCg,
        ];
        for space in spaces {
            let linear = [0.12, 0.47, 0.83];
            let converted = convert_linear_rgb(linear, space, space);
            for channel in 0..3 {
                assert!((converted[channel] - linear[channel]).abs() < 1e-5);
            }
        }
    }

    #[test]
    fn reference_white_stays_neutral_across_adapted_spaces() {
        for space in [
            ColorSpaceId::DisplayP3,
            ColorSpaceId::AdobeRgb,
            ColorSpaceId::Rec2020,
            ColorSpaceId::ProPhotoRgb,
            ColorSpaceId::Aces2065,
            ColorSpaceId::AcesCg,
        ] {
            let white = convert_linear_rgb([1.0, 1.0, 1.0], space, ColorSpaceId::SRgb);
            for channel in white {
                assert!((channel - 1.0).abs() < 2e-4, "{space:?}: {white:?}");
            }
        }
    }

    #[test]
    fn transfer_curves_have_expected_endpoints_and_round_trip() {
        for space in [
            ColorSpaceId::SRgb,
            ColorSpaceId::DisplayP3,
            ColorSpaceId::AdobeRgb,
            ColorSpaceId::Rec2020,
            ColorSpaceId::ProPhotoRgb,
        ] {
            let encoded = [0.0, 0.18, 0.5, 1.0];
            let decoded = encoded.map(|value| decode_transfer(value, profile(space).transfer));
            let round_trip = decoded.map(|value| encode_transfer(value, profile(space).transfer));
            for index in 0..encoded.len() {
                assert!((round_trip[index] - encoded[index]).abs() < 1e-10);
            }
        }
    }

    #[test]
    fn libraw_prophoto_d65_is_adapted_to_standard_romm_d50() {
        assert_eq!(libraw_native_profile(4), ColorSpaceId::ProPhotoRgbD65);
        let adapted = convert_linear_rgb(
            [1.0, 1.0, 1.0],
            ColorSpaceId::ProPhotoRgbD65,
            ColorSpaceId::ProPhotoRgb,
        );
        for channel in adapted {
            assert!((channel - 1.0).abs() < 1e-5);
        }
        let d65_to_srgb = convert_linear_rgb(
            [0.7, 0.2, 0.1],
            ColorSpaceId::ProPhotoRgbD65,
            ColorSpaceId::SRgb,
        );
        let d50_to_srgb = convert_linear_rgb(
            [0.7, 0.2, 0.1],
            ColorSpaceId::ProPhotoRgb,
            ColorSpaceId::SRgb,
        );
        assert!(
            d65_to_srgb
                .iter()
                .zip(d50_to_srgb)
                .any(|(d65, d50)| (d65 - d50).abs() > 1e-3),
            "D65 and D50 ProPhoto paths must not be treated as identical"
        );
    }

    #[test]
    fn density_gamut_compression_preserves_in_gamut_values() {
        let rgb = [0.12, 0.47, 0.83];
        let compressed = compress_linear_srgb_for_density(rgb);
        for channel in 0..3 {
            assert!((compressed[channel] - rgb[channel]).abs() < 1e-6);
        }
    }

    #[test]
    fn density_gamut_compression_removes_signed_channels_without_losing_luma() {
        let rgb = [-0.08, 0.32, 0.54];
        let compressed = compress_linear_srgb_for_density(rgb);
        assert!(compressed.iter().all(|value| *value > 0.0 && *value < 1.0));
        let source_luma = crate::core_math::density_luma(rgb);
        let compressed_luma = crate::core_math::density_luma(compressed);
        assert!((source_luma - compressed_luma).abs() < 1e-5);
        assert!(compressed[0] > compressed_luma * 0.10);
    }

    #[test]
    fn density_gamut_compression_keeps_highlight_chromaticity() {
        let compressed = compress_linear_srgb_for_density([1.4, 0.8, 0.3]);
        assert!(compressed[0] > compressed[1] && compressed[1] > compressed[2]);
        assert!((compressed[0] - (1.0 - 1.0 / 65_535.0)).abs() < 1e-5);
        assert!(
            compressed[2] > 0.2,
            "highlight must be scaled, not neutralized"
        );
    }

    #[test]
    fn density_gamut_compression_does_not_pin_negative_values_to_one_ratio() {
        let mild = compress_linear_srgb_for_density([-0.01, 0.32, 0.54]);
        let severe = compress_linear_srgb_for_density([-0.08, 0.32, 0.54]);
        let mild_ratio = mild[0] / crate::core_math::density_luma(mild);
        let severe_ratio = severe[0] / crate::core_math::density_luma(severe);
        assert!((mild_ratio - severe_ratio).abs() > 1e-3);
    }

    #[test]
    fn icc_profile_has_valid_header_and_size() {
        for space in [
            ColorSpaceId::SRgb,
            ColorSpaceId::DisplayP3,
            ColorSpaceId::ProPhotoRgb,
        ] {
            let profile = build_icc_profile(space);
            assert_eq!(&profile[36..40], b"acsp");
            assert_eq!(
                u32::from_be_bytes(profile[8..12].try_into().unwrap()),
                0x02100000,
                "the v2 desc/TRC structures require an ICC v2 header"
            );
            assert_eq!(
                u32::from_be_bytes(profile[0..4].try_into().unwrap()) as usize,
                profile.len()
            );
            assert_eq!(u32::from_be_bytes(profile[128..132].try_into().unwrap()), 9);
        }
    }

    #[test]
    fn icc_description_uses_v2_desc_type_field_widths() {
        let profile = build_icc_profile(ColorSpaceId::DisplayP3);
        let tag_count = u32::from_be_bytes(profile[128..132].try_into().unwrap()) as usize;
        let desc_entry = (0..tag_count)
            .map(|index| 132 + index * 12)
            .find(|offset| &profile[*offset..*offset + 4] == b"desc")
            .expect("ICC profile contains a description tag");
        let desc_offset =
            u32::from_be_bytes(profile[desc_entry + 4..desc_entry + 8].try_into().unwrap())
                as usize;
        let desc = &profile[desc_offset..];
        assert_eq!(&desc[0..4], b"desc");
        let ascii_count = u32::from_be_bytes(desc[8..12].try_into().unwrap()) as usize;
        let mac_count_offset = 12 + ascii_count + 8;
        assert_eq!(
            u16::from_be_bytes(
                desc[mac_count_offset..mac_count_offset + 2]
                    .try_into()
                    .unwrap()
            ),
            0
        );
        assert_eq!(desc[mac_count_offset + 2], 0);
    }

    #[test]
    fn generated_profiles_are_identifiable_for_reimport() {
        for space in [
            ColorSpaceId::SRgb,
            ColorSpaceId::DisplayP3,
            ColorSpaceId::AdobeRgb,
            ColorSpaceId::Rec2020,
            ColorSpaceId::ProPhotoRgb,
            ColorSpaceId::Aces2065,
            ColorSpaceId::AcesCg,
        ] {
            let profile = build_icc_profile(space);
            assert_eq!(identify_icc_profile(&profile), Some(space));
        }
    }
}
