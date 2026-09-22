//! Minimal DNG (TIFF/EP) writer for exported scans.
//!
//! Two shapes are emitted:
//!
//! * a mosaic DNG that re-wraps the camera RAW CFA samples of the source scan
//!   without touching a single value, so the original capture stays available in
//!   a standard, documented container;
//! * a LinearRaw DNG that carries the developed positive as linear-light
//!   16-bit RGB in a declared colour space.
//!
//! The writer only produces classic (32-bit offset) little-endian TIFF files
//! with a single IFD and a single strip. Tags are written in ascending order,
//! as the specification requires, and every 16-bit sample keeps the byte order
//! declared in the file header. Encoding returns bytes; the export pipeline
//! stores them with the same staged, atomic write it uses for every format.

use crate::raw_backend::{CfaPattern, RawMosaic};

const TYPE_BYTE: u16 = 1;
const TYPE_ASCII: u16 = 2;
const TYPE_SHORT: u16 = 3;
const TYPE_LONG: u16 = 4;
const TYPE_RATIONAL: u16 = 5;
const TYPE_UNDEFINED: u16 = 7;
const TYPE_SRATIONAL: u16 = 10;

const PHOTOMETRIC_CFA: u16 = 32803;
const PHOTOMETRIC_LINEAR_RAW: u16 = 34892;

/// DNG `CalibrationIlluminant` codes used by the exported files.
const ILLUMINANT_D65: u16 = 21;
const ILLUMINANT_D50: u16 = 23;

/// Rational denominator for the colour matrices. Adobe's own converter writes
/// the same precision, and it keeps the values inside `i32` with room to spare.
const MATRIX_DENOMINATOR: f64 = 10_000.0;

#[derive(Clone, Debug, PartialEq)]
enum Value {
    Byte(Vec<u8>),
    Ascii(String),
    Short(Vec<u16>),
    Long(Vec<u32>),
    Rational(Vec<[u32; 2]>),
    SRational(Vec<[i32; 2]>),
    Undefined(Vec<u8>),
}

impl Value {
    fn type_code(&self) -> u16 {
        match self {
            Self::Byte(_) => TYPE_BYTE,
            Self::Ascii(_) => TYPE_ASCII,
            Self::Short(_) => TYPE_SHORT,
            Self::Long(_) => TYPE_LONG,
            Self::Rational(_) => TYPE_RATIONAL,
            Self::SRational(_) => TYPE_SRATIONAL,
            Self::Undefined(_) => TYPE_UNDEFINED,
        }
    }

    fn count(&self) -> u32 {
        match self {
            Self::Byte(values) => values.len() as u32,
            Self::Ascii(value) => value.len() as u32 + 1,
            Self::Short(values) => values.len() as u32,
            Self::Long(values) => values.len() as u32,
            Self::Rational(values) => values.len() as u32,
            Self::SRational(values) => values.len() as u32,
            Self::Undefined(values) => values.len() as u32,
        }
    }

    /// Tag payload in file byte order. ASCII values are NUL terminated here so
    /// the stored byte count always matches `count()`.
    fn bytes(&self) -> Vec<u8> {
        match self {
            Self::Byte(values) => values.clone(),
            Self::Ascii(value) => {
                let mut bytes = value.as_bytes().to_vec();
                bytes.push(0);
                bytes
            }
            Self::Short(values) => values.iter().flat_map(|v| v.to_le_bytes()).collect(),
            Self::Long(values) => values.iter().flat_map(|v| v.to_le_bytes()).collect(),
            Self::Rational(values) => values
                .iter()
                .flat_map(|[numerator, denominator]| {
                    [numerator.to_le_bytes(), denominator.to_le_bytes()].concat()
                })
                .collect(),
            Self::SRational(values) => values
                .iter()
                .flat_map(|[numerator, denominator]| {
                    [numerator.to_le_bytes(), denominator.to_le_bytes()].concat()
                })
                .collect(),
            Self::Undefined(values) => values.clone(),
        }
    }
}

fn align4(value: usize) -> usize {
    (value + 3) & !3
}

fn long_value(value: f32) -> u32 {
    value.round().clamp(0.0, u32::MAX as f32) as u32
}

fn positive_rational(value: f32) -> [u32; 2] {
    let denominator = MATRIX_DENOMINATOR;
    let numerator = (f64::from(value) * denominator).round().max(0.0) as u32;
    [numerator, denominator as u32]
}

fn signed_rational(value: f64) -> [i32; 2] {
    let scaled = (value * MATRIX_DENOMINATOR).round();
    let bounded = scaled.clamp(i32::MIN as f64, i32::MAX as f64) as i32;
    [bounded, MATRIX_DENOMINATOR as i32]
}

/// Assemble one classic TIFF directory plus its strip data.
///
/// `entries` must not contain the strip tags; the writer adds `StripOffsets`
/// and `StripByteCounts` itself once the layout is known.
fn build_tiff(mut entries: Vec<(u16, Value)>, rows_per_strip: u32, strip: &[u8]) -> Vec<u8> {
    entries.push((273, Value::Long(vec![0])));
    entries.push((279, Value::Long(vec![strip.len() as u32])));
    entries.push((278, Value::Long(vec![rows_per_strip])));
    entries.sort_by_key(|(tag, _)| *tag);
    assert!(
        entries.windows(2).all(|pair| pair[0].0 < pair[1].0),
        "DNG tags must be unique and ascending"
    );

    let ifd_offset = 8usize;
    let ifd_size = 2 + entries.len() * 12 + 4;
    // Lay out the payloads of every tag whose value does not fit in the four
    // inline bytes before the strip data is placed.
    let mut value_offset = ifd_offset + ifd_size;
    let mut external_offsets = vec![None::<usize>; entries.len()];
    for (index, (_, value)) in entries.iter().enumerate() {
        let bytes = value.bytes();
        if bytes.len() > 4 {
            value_offset = align4(value_offset);
            external_offsets[index] = Some(value_offset);
            value_offset += align4(bytes.len());
        }
    }
    let data_offset = align4(value_offset);
    let strip_offset = data_offset as u32;

    let mut out = vec![0u8; data_offset];
    out[0..2].copy_from_slice(b"II");
    out[2..4].copy_from_slice(&42u16.to_le_bytes());
    out[4..8].copy_from_slice(&(ifd_offset as u32).to_le_bytes());
    out[ifd_offset..ifd_offset + 2].copy_from_slice(&(entries.len() as u16).to_le_bytes());

    for (index, (tag, value)) in entries.iter().enumerate() {
        let bytes = value.bytes();
        let entry_offset = ifd_offset + 2 + index * 12;
        out[entry_offset..entry_offset + 2].copy_from_slice(&tag.to_le_bytes());
        out[entry_offset + 2..entry_offset + 4].copy_from_slice(&value.type_code().to_le_bytes());
        out[entry_offset + 4..entry_offset + 8].copy_from_slice(&value.count().to_le_bytes());
        if bytes.len() <= 4 {
            out[entry_offset + 8..entry_offset + 8 + bytes.len()].copy_from_slice(&bytes);
        } else {
            let payload_offset = external_offsets[index].expect("external payload was laid out");
            out[entry_offset + 8..entry_offset + 12]
                .copy_from_slice(&(payload_offset as u32).to_le_bytes());
            out[payload_offset..payload_offset + bytes.len()].copy_from_slice(&bytes);
        }
    }
    // `StripOffsets` was reserved with a placeholder because its value is the
    // end of the metadata area, which is only known once every payload has been
    // placed.
    let strip_entry = entries
        .iter()
        .position(|(tag, _)| *tag == 273)
        .expect("strip offset tag is present");
    let strip_entry_offset = ifd_offset + 2 + strip_entry * 12;
    out[strip_entry_offset + 8..strip_entry_offset + 12]
        .copy_from_slice(&strip_offset.to_le_bytes());

    out.extend_from_slice(strip);
    out
}

/// EXIF style `YYYY:MM:DD HH:MM:SS` timestamp for a Unix epoch second value.
pub(crate) fn exif_datetime(epoch_seconds: i64) -> String {
    let days = epoch_seconds.div_euclid(86_400);
    let seconds_of_day = epoch_seconds.rem_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    let hour = seconds_of_day / 3600;
    let minute = (seconds_of_day % 3600) / 60;
    let second = seconds_of_day % 60;
    format!("{year:04}:{month:02}:{day:02} {hour:02}:{minute:02}:{second:02}")
}

/// Howard Hinnant's `civil_from_days`, which is exact for the whole `i64` range
/// the application can produce from the system clock.
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let day_of_era = (z - era * 146_097) as u64;
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let year = year_of_era as i64 + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_prime = (5 * day_of_year + 2) / 153;
    let day = (day_of_year - (153 * month_prime + 2) / 5 + 1) as u32;
    let month = if month_prime < 10 {
        month_prime + 3
    } else {
        month_prime - 9
    } as u32;
    (if month <= 2 { year + 1 } else { year }, month, day)
}

/// TIFF orientation that displays the stored mosaic the way LibRaw reads it.
///
/// The table is the inverse of LibRaw's own `Orientation` parsing
/// (`flip = "50132467"[orientation & 7] - '0'`).
fn tiff_orientation(libraw_flip: u16) -> u16 {
    match libraw_flip & 7 {
        1 => 2,
        2 => 4,
        3 => 3,
        4 => 5,
        5 => 8,
        6 => 6,
        7 => 7,
        _ => 1,
    }
}

/// CFA plane index for a LibRaw colour code (`0`=R, `1`=G, `2`=B, `3`=G2).
fn cfa_plane(libraw_colour: usize) -> u8 {
    match libraw_colour {
        0 => 0,
        2 => 2,
        _ => 1,
    }
}

/// LibRaw's CFA colour code for a raw coordinate, anchored at the mosaic's
/// origin exactly like the decoder's own demosaic.
fn cfa_colour(filters: u32, row: i32, column: i32) -> usize {
    ((filters >> (((((row << 1) & 14) | (column & 1)) << 1) as u32)) & 3) as usize
}

pub(crate) struct CfaDngRequest<'a> {
    pub(crate) mosaic: &'a RawMosaic,
    /// `ColorMatrix1` rows for the R, G and B planes: XYZ (D65) to camera.
    pub(crate) xyz_to_camera: Option<[[f32; 3]; 3]>,
    /// Illuminant the colour matrix is calibrated for. `None` means D65, which
    /// is what LibRaw's camera matrices are resolved for.
    pub(crate) calibration_illuminant: Option<u16>,
    /// As-shot neutral in camera coordinates, or `None` when LibRaw resolved no
    /// usable white balance.
    pub(crate) as_shot_neutral: Option<[f32; 3]>,
    pub(crate) make: &'a str,
    pub(crate) model: &'a str,
    /// Optional roll information stored in `ImageDescription`.
    pub(crate) description: Option<&'a str>,
    pub(crate) software: &'a str,
    pub(crate) timestamp: &'a str,
}

/// Re-wrap an unpacked camera RAW mosaic as a DNG. Raw sample values are copied
/// unchanged; black level, white level, CFA phase and the active area are
/// declared so a reader reproduces the same geometry the decoder measured.
pub(crate) fn cfa_dng_bytes(request: &CfaDngRequest<'_>) -> Result<Vec<u8>, String> {
    let mosaic = request.mosaic;
    let CfaPattern::Bayer { filters } = request.mosaic.metadata.cfa else {
        return Err(
            "Only Bayer camera RAW mosaics can be written as a raw DNG; export this frame as a linear DNG instead"
                .to_string(),
        );
    };
    if mosaic.samples.len() != mosaic.width as usize * mosaic.height as usize {
        return Err("The RAW mosaic does not match its declared dimensions".to_string());
    }
    let [mut left, mut top, mut active_width, mut active_height] = mosaic.metadata.active_area;
    if active_width == 0
        || active_height == 0
        || left + active_width > mosaic.width
        || top + active_height > mosaic.height
    {
        return Err("The RAW mosaic has an invalid active area".to_string());
    }
    // Every dcraw-derived reader forces even Bayer margins and shifts the CFA
    // phase with them. Declaring the already-normalised rectangle keeps the
    // exported geometry a fixed point of that rule, so the active area a reader
    // derives is exactly the rectangle this file states.
    if top % 2 == 1 {
        top += 1;
        active_height = active_height.saturating_sub(1);
    }
    if left % 2 == 1 {
        left += 1;
        active_width = active_width.saturating_sub(1);
    }
    if active_width == 0 || active_height == 0 {
        return Err("The RAW active area is too small to export".to_string());
    }

    let positions = [(0, 0), (0, 1), (1, 0), (1, 1)];
    let cfa_pattern: Vec<u8> = positions
        .iter()
        .map(|(row, column)| cfa_plane(cfa_colour(filters, *row, *column)))
        .collect();
    // One `BlackLevel` value is the form every reader agrees on for a mosaic:
    // a repeat-pattern tag would hide the values from this application's own
    // decoder, while the per-channel corrections it replaces are a fraction of
    // one raw level on real capture hardware. The four CFA positions are
    // averaged, and the result is rounded because LibRaw exposes the black
    // level as an integer.
    let black_level: f32 = (positions
        .iter()
        .map(|(row, column)| {
            let channel = cfa_colour(filters, *row, *column).min(3);
            mosaic.metadata.black_level[channel]
        })
        .sum::<f32>()
        / positions.len() as f32)
        .round();
    let white_level = mosaic
        .metadata
        .white_level
        .iter()
        .copied()
        .fold(0.0f32, f32::max);
    let white_level = white_level.max(1.0);
    // Without a resolved camera matrix the file is explicitly uncalibrated
    // rather than silently claiming a profile it does not have.
    let illuminant = match (request.xyz_to_camera, request.calibration_illuminant) {
        (Some(_), Some(illuminant)) => illuminant,
        (Some(_), None) => ILLUMINANT_D65,
        (None, _) => 0,
    };

    let mut entries = vec![
        (254u16, Value::Long(vec![0])),
        (256, Value::Long(vec![mosaic.width])),
        (257, Value::Long(vec![mosaic.height])),
        (258, Value::Short(vec![16])),
        (259, Value::Short(vec![1])),
        (262, Value::Short(vec![PHOTOMETRIC_CFA])),
        (271, Value::Ascii(request.make.to_string())),
        (272, Value::Ascii(request.model.to_string())),
        (
            274,
            Value::Short(vec![tiff_orientation(mosaic.metadata.orientation)]),
        ),
        (277, Value::Short(vec![1])),
        (284, Value::Short(vec![1])),
        (305, Value::Ascii(request.software.to_string())),
        (306, Value::Ascii(request.timestamp.to_string())),
        (33421, Value::Short(vec![2, 2])),
        (33422, Value::Byte(cfa_pattern)),
        (50706, Value::Byte(vec![1, 4, 0, 0])),
        (50707, Value::Byte(vec![1, 1, 0, 0])),
        (
            50708,
            Value::Ascii(unique_camera_model(request.make, request.model)),
        ),
        (50710, Value::Byte(vec![0, 1, 2])),
        (50711, Value::Short(vec![1])),
        (50714, Value::Rational(vec![positive_rational(black_level)])),
        (50717, Value::Long(vec![long_value(white_level)])),
        (50778, Value::Short(vec![illuminant])),
        (
            50829,
            Value::Long(vec![top, left, top + active_height, left + active_width]),
        ),
    ];
    if let Some(matrix) = request.xyz_to_camera {
        entries.push((
            50721,
            Value::SRational(
                matrix
                    .iter()
                    .flat_map(|row| row.iter())
                    .map(|value| signed_rational(f64::from(*value)))
                    .collect(),
            ),
        ));
    }
    if let Some(neutral) = request.as_shot_neutral {
        entries.push((
            50728,
            Value::Rational(neutral.map(positive_rational).to_vec()),
        ));
    }
    if let Some(description) = request.description {
        entries.push((270, Value::Ascii(description.to_string())));
    }

    let strip: Vec<u8> = mosaic
        .samples
        .iter()
        .flat_map(|sample| sample.to_le_bytes())
        .collect();
    Ok(build_tiff(entries, mosaic.height, &strip))
}

pub(crate) struct LinearDngRequest<'a> {
    pub(crate) width: u32,
    pub(crate) height: u32,
    /// Interleaved 16-bit linear RGB samples, `width * height * 3` values.
    pub(crate) rgb16: &'a [u16],
    /// `ColorMatrix1`: XYZ (D50) to the space the samples are stored in.
    pub(crate) xyz_to_space: [f64; 9],
    pub(crate) icc_profile: Option<&'a [u8]>,
    pub(crate) camera_model: &'a str,
    /// Optional roll information stored in `ImageDescription`.
    pub(crate) description: Option<&'a str>,
    pub(crate) software: &'a str,
    pub(crate) timestamp: &'a str,
}

/// Write already-linear RGB as a LinearRaw DNG.
pub(crate) fn linear_dng_bytes(request: &LinearDngRequest<'_>) -> Result<Vec<u8>, String> {
    let expected = request.width as usize * request.height as usize * 3;
    if request.rgb16.len() != expected {
        return Err("The linear image does not match its declared dimensions".to_string());
    }
    let mut entries = vec![
        (254u16, Value::Long(vec![0])),
        (256, Value::Long(vec![request.width])),
        (257, Value::Long(vec![request.height])),
        (258, Value::Short(vec![16, 16, 16])),
        (259, Value::Short(vec![1])),
        (262, Value::Short(vec![PHOTOMETRIC_LINEAR_RAW])),
        (271, Value::Ascii("NexFilm".to_string())),
        (272, Value::Ascii(request.camera_model.to_string())),
        (274, Value::Short(vec![1])),
        (277, Value::Short(vec![3])),
        (284, Value::Short(vec![1])),
        (305, Value::Ascii(request.software.to_string())),
        (306, Value::Ascii(request.timestamp.to_string())),
        (50706, Value::Byte(vec![1, 4, 0, 0])),
        (50707, Value::Byte(vec![1, 1, 0, 0])),
        (
            50708,
            Value::Ascii(unique_camera_model("NexFilm", request.camera_model)),
        ),
        (
            50721,
            Value::SRational(
                request
                    .xyz_to_space
                    .iter()
                    .map(|v| signed_rational(*v))
                    .collect(),
            ),
        ),
        // The exported samples are already neutralised, so the as-shot neutral
        // is unity for every channel.
        (50728, Value::Rational(vec![[1, 1], [1, 1], [1, 1]])),
        (50778, Value::Short(vec![ILLUMINANT_D50])),
    ];
    if let Some(profile) = request.icc_profile {
        entries.push((34675, Value::Undefined(profile.to_vec())));
    }
    if let Some(description) = request.description {
        entries.push((270, Value::Ascii(description.to_string())));
    }

    let strip: Vec<u8> = request
        .rgb16
        .iter()
        .flat_map(|sample| sample.to_le_bytes())
        .collect();
    Ok(build_tiff(entries, request.height, &strip))
}

/// `UniqueCameraModel` is limited to 63 characters plus the terminator.
fn unique_camera_model(make: &str, model: &str) -> String {
    let combined = format!("{} {}", make.trim(), model.trim())
        .trim()
        .to_string();
    let candidate = if combined.is_empty() {
        "NexFilm Scan".to_string()
    } else {
        combined
    };
    candidate.chars().take(63).collect()
}

/// Colour tags copied from an existing DNG.
#[derive(Debug, Clone, PartialEq, Default)]
pub(crate) struct DngColorMetadata {
    pub(crate) xyz_to_camera: Option<[[f32; 3]; 3]>,
    pub(crate) calibration_illuminant: Option<u16>,
    pub(crate) as_shot_neutral: Option<[f32; 3]>,
}

/// Read `ColorMatrix1`, `CalibrationIlluminant1` and `AsShotNeutral` from a DNG.
///
/// LibRaw resolves its camera matrix while *opening* a proprietary RAW file, but
/// only from the DNG's own tags during processing. Re-wrapping a camera DNG
/// therefore has to carry those tags over from the source file, or the export
/// would lose the colour calibration the source already declared.
///
/// Classic (32-bit offset) TIFF only; anything unreadable yields `None` so the
/// caller can keep the export uncalibrated instead of failing it.
pub(crate) fn read_dng_color_metadata(path: &std::path::Path) -> Option<DngColorMetadata> {
    let bytes = std::fs::read(path).ok()?;
    if bytes.len() < 8 {
        return None;
    }
    let little = match &bytes[..2] {
        b"II" => true,
        b"MM" => false,
        _ => return None,
    };
    let read_u16 = |offset: usize| -> Option<u16> {
        let raw: [u8; 2] = bytes.get(offset..offset + 2)?.try_into().ok()?;
        Some(if little {
            u16::from_le_bytes(raw)
        } else {
            u16::from_be_bytes(raw)
        })
    };
    let read_u32 = |offset: usize| -> Option<u32> {
        let raw: [u8; 4] = bytes.get(offset..offset + 4)?.try_into().ok()?;
        Some(if little {
            u32::from_le_bytes(raw)
        } else {
            u32::from_be_bytes(raw)
        })
    };
    if read_u16(2)? != 42 {
        return None;
    }
    let ifd_offset = read_u32(4)? as usize;
    let entry_count = read_u16(ifd_offset)? as usize;
    // Tag values up to four bytes live inside the entry; anything longer is
    // stored at the offset the entry points to.
    let value_bytes = |entry: usize, type_code: u16, count: usize| -> Option<Vec<u8>> {
        let unit = match type_code {
            3 => 2usize,
            4 => 4,
            5 | 10 => 8,
            _ => return None,
        };
        let total = unit.checked_mul(count)?;
        if total <= 4 {
            bytes
                .get(entry + 8..entry + 8 + total)
                .map(|raw| raw.to_vec())
        } else {
            let offset = read_u32(entry + 8)? as usize;
            bytes
                .get(offset..offset.checked_add(total)?)
                .map(|raw| raw.to_vec())
        }
    };
    let mut metadata = DngColorMetadata::default();
    for index in 0..entry_count.min(512) {
        let entry = ifd_offset + 2 + index * 12;
        let tag = read_u16(entry)?;
        let type_code = read_u16(entry + 2)?;
        let count = read_u32(entry + 4)? as usize;
        let Some(raw) = value_bytes(entry, type_code, count) else {
            continue;
        };
        let fraction = |index: usize| -> Option<f64> {
            let entry = raw.get(index * 8..index * 8 + 8)?;
            let numerator_bytes: [u8; 4] = entry[..4].try_into().ok()?;
            let denominator_bytes: [u8; 4] = entry[4..8].try_into().ok()?;
            let signed = type_code == 10;
            let numerator = match (little, signed) {
                (true, true) => i32::from_le_bytes(numerator_bytes) as f64,
                (true, false) => u32::from_le_bytes(numerator_bytes) as f64,
                (false, true) => i32::from_be_bytes(numerator_bytes) as f64,
                (false, false) => u32::from_be_bytes(numerator_bytes) as f64,
            };
            let denominator = if little {
                u32::from_le_bytes(denominator_bytes)
            } else {
                u32::from_be_bytes(denominator_bytes)
            } as f64;
            (denominator != 0.0).then_some(numerator / denominator)
        };
        match (tag, type_code, count) {
            (50721, 10, 9) => {
                let values: Vec<f64> = (0..9).map(fraction).collect::<Option<_>>()?;
                metadata.xyz_to_camera = Some([
                    [values[0] as f32, values[1] as f32, values[2] as f32],
                    [values[3] as f32, values[4] as f32, values[5] as f32],
                    [values[6] as f32, values[7] as f32, values[8] as f32],
                ]);
            }
            (50778, 3, 1) => {
                metadata.calibration_illuminant = Some(if little {
                    u16::from_le_bytes(raw.get(..2)?.try_into().ok()?)
                } else {
                    u16::from_be_bytes(raw.get(..2)?.try_into().ok()?)
                });
            }
            (50728, 5, 3) => {
                let values: Vec<f64> = (0..3).map(fraction).collect::<Option<_>>()?;
                metadata.as_shot_neutral =
                    Some([values[0] as f32, values[1] as f32, values[2] as f32]);
            }
            _ => {}
        }
    }
    (metadata.xyz_to_camera.is_some()
        || metadata.calibration_illuminant.is_some()
        || metadata.as_shot_neutral.is_some())
    .then_some(metadata)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::raw_backend::{CaptureConditions, RawMetadata};

    fn mosaic(filters: u32, black: [f32; 4], white: [f32; 4]) -> RawMosaic {
        // LibRaw rejects raw frames smaller than 22 pixels on either axis, so
        // the fixture stays above that floor to exercise the real reader.
        let width = 36u32;
        let height = 26u32;
        let samples = (0..(width * height) as u16)
            .map(|index| index * 7 + 3)
            .collect();
        RawMosaic {
            width,
            height,
            samples,
            metadata: RawMetadata {
                cfa: CfaPattern::Bayer { filters },
                active_area: [2, 1, width - 4, height - 2],
                raw_pitch_bytes: width * 2,
                black_level: black,
                white_level: white,
                masked_areas: Vec::new(),
                masked_pixels: Vec::new(),
                orientation: 0,
                iso: Some(100.0),
                exposure_seconds: Some(1.0 / 60.0),
                camera_id: "TestCam|TestModel".to_string(),
                libraw_version: "0.20.1".to_string(),
                capture_conditions: CaptureConditions::default(),
            },
        }
    }

    /// Read one little-endian IFD back with a deliberately independent parser so
    /// the assertions do not depend on the writer's own layout helpers.
    fn read_ifd(bytes: &[u8]) -> Vec<(u16, u16, u32, Vec<u8>)> {
        assert_eq!(&bytes[0..2], b"II");
        assert_eq!(u16::from_le_bytes([bytes[2], bytes[3]]), 42);
        let ifd_offset = u32::from_le_bytes(bytes[4..8].try_into().unwrap()) as usize;
        let count = u16::from_le_bytes([bytes[ifd_offset], bytes[ifd_offset + 1]]) as usize;
        let mut entries = Vec::new();
        for index in 0..count {
            let offset = ifd_offset + 2 + index * 12;
            let tag = u16::from_le_bytes([bytes[offset], bytes[offset + 1]]);
            let type_code = u16::from_le_bytes([bytes[offset + 2], bytes[offset + 3]]);
            let value_count = u32::from_le_bytes(bytes[offset + 4..offset + 8].try_into().unwrap());
            let unit = match type_code {
                1 | 2 | 6 | 7 => 1usize,
                3 | 8 => 2,
                4 | 9 | 11 => 4,
                5 | 10 | 12 => 8,
                other => panic!("unexpected TIFF type {other}"),
            };
            let payload_len = unit * value_count as usize;
            let payload = if payload_len <= 4 {
                bytes[offset + 8..offset + 8 + payload_len].to_vec()
            } else {
                let value_offset =
                    u32::from_le_bytes(bytes[offset + 8..offset + 12].try_into().unwrap()) as usize;
                bytes[value_offset..value_offset + payload_len].to_vec()
            };
            entries.push((tag, type_code, value_count, payload));
        }
        entries
    }

    fn entry<'a>(
        entries: &'a [(u16, u16, u32, Vec<u8>)],
        tag: u16,
    ) -> &'a (u16, u16, u32, Vec<u8>) {
        entries
            .iter()
            .find(|entry| entry.0 == tag)
            .unwrap_or_else(|| panic!("tag {tag} is missing"))
    }

    /// LibRaw reads files, so the round-trip tests need one on disk.
    fn write_temp(name: &str, bytes: &[u8]) -> std::path::PathBuf {
        let path = std::env::temp_dir().join(name);
        std::fs::write(&path, bytes).unwrap();
        path
    }

    #[test]
    fn cfa_dng_declares_geometry_and_raw_samples() {
        // RGGB, as dcraw encodes it: R G / G B.
        let filters = 0x94949494;
        let mosaic = mosaic(filters, [512.0, 513.0, 514.0, 515.0], [16_383.0; 4]);
        let request = CfaDngRequest {
            mosaic: &mosaic,
            xyz_to_camera: Some([[0.67, -0.2, 0.03], [-0.4, 1.2, 0.1], [0.02, -0.15, 0.8]]),
            calibration_illuminant: None,
            as_shot_neutral: Some([0.5, 1.0, 0.8]),
            make: "TestCam",
            model: "TestModel",
            description: Some("Film: Test Film"),
            software: "NexFilm Engine test",
            timestamp: "2026:09:18 10:00:00",
        };
        let bytes = cfa_dng_bytes(&request).unwrap();
        let entries = read_ifd(&bytes);

        assert_eq!(
            u32::from_le_bytes(entry(&entries, 256).3[..4].try_into().unwrap()),
            36
        );
        assert_eq!(
            u32::from_le_bytes(entry(&entries, 257).3[..4].try_into().unwrap()),
            26
        );
        assert_eq!(
            u16::from_le_bytes([entry(&entries, 262).3[0], entry(&entries, 262).3[1]]),
            PHOTOMETRIC_CFA
        );
        assert_eq!(entry(&entries, 33422).3, vec![0, 1, 1, 2]);
        assert_eq!(entry(&entries, 50706).3, vec![1, 4, 0, 0]);
        assert_eq!(
            entry(&entries, 270).3,
            b"Film: Test Film\0".to_vec(),
            "the roll note travels in ImageDescription"
        );
        // The declared active area is aligned to even Bayer margins, so a
        // reader cannot shift it behind our back.
        assert_eq!(
            entry(&entries, 50829).3,
            [
                2u32.to_le_bytes(),
                2u32.to_le_bytes(),
                25u32.to_le_bytes(),
                34u32.to_le_bytes(),
            ]
            .concat()
        );
        assert_eq!(
            entry(&entries, 50710).3,
            vec![0, 1, 2],
            "CFA plane colours follow the Adobe convention"
        );
        // One black level is declared for the mosaic, averaged over the four
        // CFA positions: R, G, G, B.
        let black = entry(&entries, 50714);
        assert_eq!(black.2, 1);
        let values: Vec<f32> = black
            .3
            .chunks_exact(8)
            .map(|chunk| {
                let numerator = u32::from_le_bytes(chunk[..4].try_into().unwrap());
                let denominator = u32::from_le_bytes(chunk[4..8].try_into().unwrap());
                numerator as f32 / denominator.max(1) as f32
            })
            .collect();
        assert_eq!(values, vec![513.0]);
        let matrix = entry(&entries, 50721);
        assert_eq!(matrix.1, TYPE_SRATIONAL);
        assert_eq!(matrix.2, 9);
        let neutral = entry(&entries, 50728);
        assert_eq!(neutral.1, TYPE_RATIONAL);
        assert_eq!(neutral.2, 3);

        // The strip payload is the untouched mosaic, little endian.
        let strip_offset =
            u32::from_le_bytes(entry(&entries, 273).3[..4].try_into().unwrap()) as usize;
        let strip_count =
            u32::from_le_bytes(entry(&entries, 279).3[..4].try_into().unwrap()) as usize;
        assert_eq!(strip_count, mosaic.samples.len() * 2);
        let strip = &bytes[strip_offset..strip_offset + strip_count];
        let decoded: Vec<u16> = strip
            .chunks_exact(2)
            .map(|chunk| u16::from_le_bytes([chunk[0], chunk[1]]))
            .collect();
        assert_eq!(decoded, mosaic.samples);
    }

    #[test]
    fn cfa_dng_rejects_non_bayer_mosaics() {
        let mut mosaic = mosaic(0x94949494, [0.0; 4], [16_383.0; 4]);
        mosaic.metadata.cfa = CfaPattern::XTrans;
        let request = CfaDngRequest {
            mosaic: &mosaic,
            xyz_to_camera: None,
            calibration_illuminant: None,
            as_shot_neutral: None,
            make: "",
            model: "",
            description: None,
            software: "NexFilm Engine test",
            timestamp: "2026:09:18 10:00:00",
        };
        let error = cfa_dng_bytes(&request).unwrap_err();
        assert!(error.contains("Bayer"), "{error}");
    }

    #[test]
    fn linear_dng_carries_linear_rgb_and_colour_metadata() {
        let width = 3u32;
        let height = 2u32;
        let rgb16: Vec<u16> = (0..(width * height * 3) as u16).map(|v| v * 11).collect();
        let identity = [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0];
        let request = LinearDngRequest {
            width,
            height,
            rgb16: &rgb16,
            xyz_to_space: identity,
            icc_profile: Some(&[0xde, 0xad, 0xbe, 0xef]),
            camera_model: "NexFilm Linear sRGB",
            description: None,
            software: "NexFilm Engine test",
            timestamp: "2026:09:18 10:00:00",
        };
        let bytes = linear_dng_bytes(&request).unwrap();
        let entries = read_ifd(&bytes);

        assert_eq!(
            u16::from_le_bytes([entry(&entries, 262).3[0], entry(&entries, 262).3[1]]),
            PHOTOMETRIC_LINEAR_RAW
        );
        assert_eq!(entry(&entries, 258).2, 3, "three 16-bit samples per pixel");
        assert_eq!(
            u16::from_le_bytes([entry(&entries, 277).3[0], entry(&entries, 277).3[1]]),
            3
        );
        assert_eq!(
            u16::from_le_bytes([entry(&entries, 50778).3[0], entry(&entries, 50778).3[1]]),
            ILLUMINANT_D50
        );
        assert_eq!(entry(&entries, 34675).3, vec![0xde, 0xad, 0xbe, 0xef]);
        let strip_offset =
            u32::from_le_bytes(entry(&entries, 273).3[..4].try_into().unwrap()) as usize;
        let strip_count =
            u32::from_le_bytes(entry(&entries, 279).3[..4].try_into().unwrap()) as usize;
        assert_eq!(strip_count, rgb16.len() * 2);
        let decoded: Vec<u16> = bytes[strip_offset..strip_offset + strip_count]
            .chunks_exact(2)
            .map(|chunk| u16::from_le_bytes([chunk[0], chunk[1]]))
            .collect();
        assert_eq!(decoded, rgb16);
    }

    #[test]
    fn orientation_table_inverts_libraws_parser() {
        for flip in 0..8u16 {
            let orientation = tiff_orientation(flip);
            let round_trip = "50132467".as_bytes()[(orientation & 7) as usize] - b'0';
            assert_eq!(u16::from(round_trip), flip, "flip {flip} must round trip");
        }
    }

    /// The exported DNG has to be readable by the same decoder that reads camera
    /// RAW files. This round trip proves the CFA phase, the active area, the
    /// orientation and every raw sample survive the container change.
    #[test]
    fn libraw_reads_back_the_written_cfa_dng() {
        let filters = 0xb4b4b4b4; // GRBG, so the pattern is not the default one.
        let mut source = mosaic(filters, [600.0, 601.0, 602.0, 603.0], [16_300.0; 4]);
        source.metadata.orientation = 6;
        let request = CfaDngRequest {
            mosaic: &source,
            xyz_to_camera: Some([[0.67, -0.2, 0.03], [-0.4, 1.2, 0.1], [0.02, -0.15, 0.8]]),
            calibration_illuminant: None,
            as_shot_neutral: Some([0.6, 1.0, 0.7]),
            make: "NexFilm",
            model: "Round Trip",
            description: None,
            software: "NexFilm Engine test",
            timestamp: "2026:09:18 10:00:00",
        };
        let path = write_temp(
            "nexfilm-cfa-dng-round-trip.dng",
            &cfa_dng_bytes(&request).unwrap(),
        );

        let read = crate::raw_backend::decode_raw_mosaic(&path)
            .expect("LibRaw must accept the exported DNG");
        assert_eq!((read.width, read.height), (source.width, source.height));
        assert_eq!(
            read.samples, source.samples,
            "raw samples must be untouched"
        );
        // Odd origins are aligned up to even Bayer margins, exactly as every
        // dcraw-derived reader would do, so the declared rectangle survives.
        assert_eq!(read.metadata.active_area, [2, 2, 32, 23]);
        assert_eq!(read.metadata.orientation, source.metadata.orientation);
        let CfaPattern::Bayer {
            filters: read_filters,
        } = read.metadata.cfa
        else {
            panic!("the DNG must stay a Bayer mosaic");
        };
        let positions = [(0, 0), (0, 1), (1, 0), (1, 1)];
        let expected: Vec<usize> = positions
            .iter()
            .map(|(row, column)| cfa_plane(cfa_colour(filters, *row, *column)) as usize)
            .collect();
        let actual: Vec<usize> = positions
            .iter()
            .map(|(row, column)| cfa_plane(cfa_colour(read_filters, *row, *column)) as usize)
            .collect();
        assert_eq!(
            actual, expected,
            "the CFA phase must survive the round trip"
        );
        // The declared black level is the CFA-position average, and the decoder
        // must report exactly that value for every channel.
        let expected_black = (positions
            .iter()
            .map(|(row, column)| {
                let channel = cfa_colour(filters, *row, *column).min(3);
                source.metadata.black_level[channel]
            })
            .sum::<f32>()
            / positions.len() as f32)
            .round();
        for channel in 0..4 {
            assert!(
                (read.metadata.black_level[channel] - expected_black).abs() < 0.01,
                "black level changed for channel {channel}: {:?} != {expected_black}",
                read.metadata.black_level
            );
        }
        assert!(
            (read.metadata.white_level[0] - source.metadata.white_level[0]).abs() < 0.5,
            "white level changed"
        );
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn exif_timestamp_matches_known_values() {
        assert_eq!(exif_datetime(0), "1970:01:01 00:00:00");
        assert_eq!(exif_datetime(1_700_000_000), "2023:11:14 22:13:20");
        assert_eq!(exif_datetime(1_756_000_000), "2025:08:24 01:46:40");
    }

    /// A camera DNG has to hand its own colour tags to the re-wrap: LibRaw
    /// resolves a proprietary RAW file's matrix while opening it, but only reads
    /// a DNG's `ColorMatrix1` during processing, so the exporter falls back to
    /// the file itself. This closes that loop on an exported file.
    #[test]
    fn the_written_cfa_dng_carries_colour_tags_a_re_wrap_can_reuse() {
        let filters = 0x94949494;
        let matrix = [[0.67, -0.2, 0.03], [-0.4, 1.2, 0.1], [0.02, -0.15, 0.8]];
        let mosaic = mosaic(filters, [512.0; 4], [16_383.0; 4]);
        let request = CfaDngRequest {
            mosaic: &mosaic,
            xyz_to_camera: Some(matrix),
            calibration_illuminant: None,
            as_shot_neutral: Some([0.55, 1.0, 0.75]),
            make: "NexFilm",
            model: "Source Reader",
            description: None,
            software: "NexFilm Engine test",
            timestamp: "2026:09:18 10:00:00",
        };
        let path = write_temp(
            "nexfilm-cfa-dng-colour-tags.dng",
            &cfa_dng_bytes(&request).unwrap(),
        );

        let metadata = read_dng_color_metadata(&path).expect("the colour tags must be readable");
        assert_eq!(metadata.calibration_illuminant, Some(ILLUMINANT_D65));
        for (read_row, written_row) in metadata
            .xyz_to_camera
            .expect("ColorMatrix1 must survive")
            .iter()
            .zip(matrix.iter())
        {
            for (read_value, written_value) in read_row.iter().zip(written_row.iter()) {
                assert!(
                    (read_value - written_value).abs() < 0.01,
                    "colour matrix changed: {metadata:?} != {matrix:?}"
                );
            }
        }
        let neutral = metadata
            .as_shot_neutral
            .expect("AsShotNeutral must survive");
        for (read_value, written_value) in neutral.iter().zip([0.55f32, 1.0, 0.75].iter()) {
            assert!(
                (read_value - written_value).abs() < 0.02,
                "as-shot neutral changed: {neutral:?}"
            );
        }

        // The mosaic still comes from LibRaw: that is the half the decoder owns.
        let source = crate::raw_backend::read_raw_container_source(&path)
            .expect("the exporter must be able to re-read its own raw DNG");
        assert_eq!(source.mosaic.samples, mosaic.samples);
        assert_eq!(source.make, "NexFilm");
        assert_eq!(source.model, "Source Reader");
        std::fs::remove_file(&path).ok();
    }

    /// The white balance has to survive the container change. The tag stores
    /// the neutral, and the decoder reports the multipliers it derives from it,
    /// so a re-wrap that stores the wrong member of the reciprocal pair returns
    /// a different balance than the capture it came from.
    #[test]
    fn cfa_dng_keeps_the_white_balance_of_the_capture() {
        // A camera whose as-shot multipliers are R 2.0, G 1.0, B 1.5, in the
        // four-channel R, G1, B, G2 form LibRaw reports.
        let neutral = crate::raw_backend::normalized_as_shot_neutral([2.0, 1.0, 1.5, 1.0]).unwrap();
        let mosaic = mosaic(0x94949494, [512.0; 4], [16_383.0; 4]);
        let request = CfaDngRequest {
            mosaic: &mosaic,
            xyz_to_camera: Some([[0.67, -0.2, 0.03], [-0.4, 1.2, 0.1], [0.02, -0.15, 0.8]]),
            calibration_illuminant: None,
            as_shot_neutral: Some(neutral),
            make: "NexFilm",
            model: "White Balance",
            description: None,
            software: "NexFilm Engine test",
            timestamp: "2026:09:21 10:00:00",
        };
        let path = write_temp(
            "nexfilm-cfa-dng-white-balance.dng",
            &cfa_dng_bytes(&request).unwrap(),
        );

        let read = crate::raw_backend::read_raw_container_source(&path)
            .expect("the exported raw DNG must be readable");
        let reported = read
            .as_shot_neutral
            .expect("the decoder must report the stored white balance");
        for channel in 0..3 {
            assert!(
                (reported[channel] - neutral[channel]).abs() < 0.02,
                "the white balance changed: {reported:?} != {neutral:?}"
            );
        }
        std::fs::remove_file(&path).ok();
    }

    /// An exported linear DNG must be readable by the decoder the application
    /// itself would use if the file were imported again.
    #[test]
    fn libraw_reads_the_written_linear_dng_as_a_rgb_image() {
        use crate::raw_backend::{DecodeOptions, WhiteBalancePolicy};

        let width = 32u32;
        let height = 24u32;
        let rgb16: Vec<u16> = (0..(width * height * 3) as u32)
            .map(|index| (index % 40_000) as u16)
            .collect();
        let request = LinearDngRequest {
            width,
            height,
            rgb16: &rgb16,
            xyz_to_space: crate::color_science::xyz_d50_to_color_space_matrix(
                crate::color_science::ColorSpaceId::ProPhotoRgb,
            ),
            icc_profile: None,
            camera_model: "NexFilm Linear",
            description: None,
            software: "NexFilm Engine test",
            timestamp: "2026:09:18 10:00:00",
        };
        let path = write_temp(
            "nexfilm-linear-dng-libraw.dng",
            &linear_dng_bytes(&request).unwrap(),
        );

        let options = DecodeOptions {
            half_size: false,
            demosaic_quality: 3,
            output_bps: 16,
            no_auto_bright: true,
            output_color: 0,
            linear_gamma: true,
            use_camera_wb: false,
        };
        let decoded = crate::raw_backend::extract_camera_rgb_with_policy(
            &path,
            &options,
            WhiteBalancePolicy::NormalizedAsShot,
        )
        .expect("LibRaw must accept the exported linear DNG");
        assert_eq!(
            (decoded.width, decoded.height),
            (width as u16, height as u16)
        );
        assert_eq!(decoded.colors, 3);
        assert_eq!(decoded.bits, 16);
        assert!(
            decoded.data.iter().any(|byte| *byte != 0),
            "the decoded linear image must carry the exported samples"
        );
        std::fs::remove_file(&path).ok();
    }
}
