use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::Path;

/// Fixed numerical contract for capture-domain correction. Epsilon is never
/// used to turn an invalid sample or denominator into a valid measurement.
pub(crate) const DENSITY_EPSILON: f32 = 1.0e-6;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum CfaPattern {
    Bayer { filters: u32 },
    XTrans,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[repr(u8)]
#[serde(rename_all = "snake_case")]
pub(crate) enum QualityFlag {
    InvalidDenominator,
    NegativeSample,
    SaturatedSample,
    SaturatedReference,
    OutOfRange,
    BadPixel,
    MissingReference,
    MissingMetadata,
    GeometryMismatch,
    ExposureMismatch,
    IsoMismatch,
    LightMismatch,
    CameraMismatch,
    CfaMismatch,
    DemosaicInsufficientNeighbors,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub(crate) struct QualityFlags(u16);

impl QualityFlags {
    fn bit(flag: QualityFlag) -> u16 {
        1u16 << flag as u8
    }

    pub(crate) fn contains(&self, flag: &QualityFlag) -> bool {
        self.0 & Self::bit(*flag) != 0
    }

    fn insert(&mut self, flag: QualityFlag) {
        self.0 |= Self::bit(flag);
    }

    pub(crate) fn iter(self) -> impl Iterator<Item = QualityFlag> {
        const FLAGS: [QualityFlag; 15] = [
            QualityFlag::InvalidDenominator,
            QualityFlag::NegativeSample,
            QualityFlag::SaturatedSample,
            QualityFlag::SaturatedReference,
            QualityFlag::OutOfRange,
            QualityFlag::BadPixel,
            QualityFlag::MissingReference,
            QualityFlag::MissingMetadata,
            QualityFlag::GeometryMismatch,
            QualityFlag::ExposureMismatch,
            QualityFlag::IsoMismatch,
            QualityFlag::LightMismatch,
            QualityFlag::CameraMismatch,
            QualityFlag::CfaMismatch,
            QualityFlag::DemosaicInsufficientNeighbors,
        ];
        FLAGS.into_iter().filter(move |flag| self.contains(flag))
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct QualityMask {
    pub(crate) valid: Vec<bool>,
    pub(crate) flags: Vec<QualityFlags>,
}

impl QualityMask {
    pub(crate) fn new(len: usize) -> Self {
        Self {
            valid: vec![true; len],
            flags: vec![QualityFlags::default(); len],
        }
    }

    pub(crate) fn invalidate(&mut self, index: usize, flag: QualityFlag) {
        if let Some(valid) = self.valid.get_mut(index) {
            *valid = false;
        }
        if let Some(flags) = self.flags.get_mut(index) {
            flags.insert(flag);
        }
    }

    pub(crate) fn summary(&self) -> QualitySummary {
        let mut summary = QualitySummary {
            total_samples: self.valid.len(),
            valid_samples: self.valid.iter().filter(|valid| **valid).count(),
            ..QualitySummary::default()
        };
        for flags in &self.flags {
            for flag in flags.iter() {
                match flag {
                    QualityFlag::InvalidDenominator => summary.invalid_denominator += 1,
                    QualityFlag::NegativeSample => summary.negative_samples += 1,
                    QualityFlag::SaturatedSample | QualityFlag::SaturatedReference => {
                        summary.saturated_samples += 1
                    }
                    QualityFlag::BadPixel => summary.bad_pixels += 1,
                    QualityFlag::MissingReference => summary.missing_reference += 1,
                    QualityFlag::OutOfRange => summary.out_of_range += 1,
                    _ => summary.reference_mismatch += 1,
                }
            }
        }
        summary
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct QualitySummary {
    pub(crate) total_samples: usize,
    pub(crate) valid_samples: usize,
    pub(crate) invalid_denominator: usize,
    pub(crate) negative_samples: usize,
    pub(crate) saturated_samples: usize,
    pub(crate) bad_pixels: usize,
    pub(crate) missing_reference: usize,
    pub(crate) out_of_range: usize,
    pub(crate) reference_mismatch: usize,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub(crate) struct CaptureDiagnostics {
    pub(crate) quality: QualitySummary,
    pub(crate) valid_min: Option<f32>,
    pub(crate) valid_max: Option<f32>,
    pub(crate) valid_mean: Option<f32>,
}

#[inline]
fn bayer_raw_channel(filters: u32, row: i32, col: i32) -> usize {
    ((filters >> ((((row << 1) & 14) | (col & 1)) << 1)) & 3) as usize
}

#[inline]
fn bayer_rgb_channel(filters: u32, row: i32, col: i32) -> usize {
    match bayer_raw_channel(filters, row, col) {
        0 => 0,
        2 => 2,
        _ => 1,
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct CaptureConditions {
    pub(crate) light_source_id: Option<String>,
    pub(crate) geometry_fingerprint: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct RawMetadata {
    pub(crate) cfa: CfaPattern,
    pub(crate) active_area: [u32; 4],
    pub(crate) raw_pitch_bytes: u32,
    pub(crate) black_level: [f32; 4],
    pub(crate) white_level: [f32; 4],
    /// LibRaw optical-black rectangles as [top, left, bottom, right].
    pub(crate) masked_areas: Vec<[u32; 4]>,
    /// Explicit bad/masked raw-sample indices supplied by Capture validation.
    pub(crate) masked_pixels: Vec<u32>,
    pub(crate) orientation: u16,
    pub(crate) iso: Option<f32>,
    pub(crate) exposure_seconds: Option<f32>,
    pub(crate) camera_id: String,
    pub(crate) libraw_version: String,
    #[serde(default)]
    pub(crate) capture_conditions: CaptureConditions,
}

impl Default for RawMetadata {
    fn default() -> Self {
        Self {
            cfa: CfaPattern::Unknown,
            active_area: [0; 4],
            raw_pitch_bytes: 0,
            black_level: [0.0; 4],
            white_level: [65535.0; 4],
            masked_areas: Vec::new(),
            masked_pixels: Vec::new(),
            orientation: 0,
            iso: None,
            exposure_seconds: None,
            camera_id: String::new(),
            libraw_version: String::new(),
            capture_conditions: CaptureConditions::default(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct RawMosaic {
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) samples: Vec<u16>,
    pub(crate) metadata: RawMetadata,
}

pub(crate) fn normalized_raw_metadata_digest(metadata: &RawMetadata) -> Result<String, String> {
    let mut normalized = metadata.clone();
    // These fields are session inputs, not claims embedded into each mosaic.
    normalized.capture_conditions = CaptureConditions::default();
    let bytes = serde_json::to_vec(&normalized)
        .map_err(|error| format!("Failed to normalize RAW metadata: {error}"))?;
    Ok(format!("{:x}", Sha256::digest(bytes)))
}

pub(crate) fn raw_geometry_fingerprint(mosaic: &RawMosaic) -> Result<String, String> {
    let geometry = (
        mosaic.width,
        mosaic.height,
        mosaic.metadata.active_area,
        mosaic.metadata.raw_pitch_bytes,
        mosaic.metadata.cfa,
        mosaic.metadata.orientation,
    );
    let bytes = serde_json::to_vec(&geometry)
        .map_err(|error| format!("Failed to normalize RAW geometry: {error}"))?;
    Ok(format!("{:x}", Sha256::digest(bytes)))
}

/// Linear camera-space RGB estimate used before ProPhoto conversion. This is
/// not Capture Corrected transmission and must not be labelled as density.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct CameraRgbEstimate {
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) pixels: Vec<f32>,
    pub(crate) camera_to_srgb: [f32; 9],
    pub(crate) label: &'static str,
}

/// Fixed-demosaic Camera Native RGB. No white balance, output color matrix, or
/// display-space conversion has been applied.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct CameraNativeTransmissionRgbF32 {
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) pixels: Vec<f32>,
    pub(crate) quality: QualityMask,
}

/// Linear positive transmission input for density math. The quality mask is
/// part of the value so invalid samples cannot be silently consumed.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct RelativeTransmissionRgbF32 {
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) transmission: Vec<f32>,
    pub(crate) quality: QualityMask,
    pub(crate) diagnostics: CaptureDiagnostics,
    /// Phase-1 deliberately uses an identity Capture Separation placeholder;
    /// this does not claim calibrated Status M or physical film density.
    pub(crate) capture_separation: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct CorrectedCfaF32 {
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) samples: Vec<f32>,
    pub(crate) metadata: RawMetadata,
    pub(crate) quality: QualityMask,
    pub(crate) diagnostics: CaptureDiagnostics,
}

/// (sample - dark) / (open - dark), performed in the capture domain.
pub(crate) fn capture_transmission(
    sample: f32,
    dark: f32,
    open: f32,
    white: f32,
) -> (f32, Vec<QualityFlag>) {
    let mut flags = Vec::new();
    if !sample.is_finite() || !dark.is_finite() || !open.is_finite() {
        flags.push(QualityFlag::NegativeSample);
        return (f32::NAN, flags);
    }
    if sample < dark {
        flags.push(QualityFlag::NegativeSample);
    }
    if sample >= white {
        flags.push(QualityFlag::SaturatedSample);
    }
    if open >= white {
        flags.push(QualityFlag::SaturatedReference);
    }
    let denominator = open - dark;
    if denominator <= DENSITY_EPSILON || !denominator.is_finite() {
        flags.push(QualityFlag::InvalidDenominator);
        return (f32::NAN, flags);
    }
    let transmission = (sample - dark) / denominator;
    if !transmission.is_finite() || transmission <= 0.0 {
        flags.push(QualityFlag::NegativeSample);
    }
    if transmission > 1.0 + DENSITY_EPSILON {
        flags.push(QualityFlag::OutOfRange);
    }
    (transmission, flags)
}

/// Validate that a reference frame is geometrically and exposure compatible.
pub(crate) fn validate_reference_compatibility(
    sample: &RawMetadata,
    reference: &RawMetadata,
) -> Result<(), Vec<QualityFlag>> {
    let mut flags = Vec::new();
    if sample.active_area != reference.active_area {
        flags.push(QualityFlag::GeometryMismatch);
    }
    if sample.cfa != reference.cfa {
        flags.push(QualityFlag::CfaMismatch);
    }
    if !sample.camera_id.is_empty()
        && !reference.camera_id.is_empty()
        && sample.camera_id != reference.camera_id
    {
        flags.push(QualityFlag::CameraMismatch);
    }
    match (sample.iso, reference.iso) {
        (Some(a), Some(b)) if (a - b).abs() > 0.5 => flags.push(QualityFlag::IsoMismatch),
        (None, _) | (_, None) => flags.push(QualityFlag::MissingMetadata),
        _ => {}
    }
    match (sample.exposure_seconds, reference.exposure_seconds) {
        (Some(a), Some(b)) if (a - b).abs() > (a.abs().max(b.abs()) * 0.01).max(1.0e-6) => {
            flags.push(QualityFlag::ExposureMismatch)
        }
        (None, _) | (_, None) => flags.push(QualityFlag::MissingMetadata),
        _ => {}
    }
    // Light and fixture geometry are calibration-session inputs. They must not
    // be copied into every RawMosaic merely to make this comparison pass.
    flags.sort_unstable_by_key(|flag| *flag as u8);
    flags.dedup();
    if flags.is_empty() {
        Ok(())
    } else {
        Err(flags)
    }
}

/// Fixed, reproducible bilinear Bayer demosaic. No camera white balance or
/// camera-to-sRGB matrix is applied here.
pub(crate) fn demosaic_bayer_fixed(
    corrected: &CorrectedCfaF32,
) -> Result<CameraNativeTransmissionRgbF32, String> {
    let CfaPattern::Bayer { filters } = corrected.metadata.cfa else {
        return Err("Measured CFA demosaic requires a Bayer CFA".to_string());
    };
    let expected = (corrected.width as usize).saturating_mul(corrected.height as usize);
    if corrected.samples.len() != expected || corrected.quality.valid.len() != expected {
        return Err("Raw mosaic and quality mask dimensions do not match".to_string());
    }
    let [left, top, active_width, active_height] = corrected.metadata.active_area;
    if active_width == 0
        || active_height == 0
        || left.saturating_add(active_width) > corrected.width
        || top.saturating_add(active_height) > corrected.height
    {
        return Err("Raw active area is outside the unpacked mosaic".to_string());
    }
    let output_len = active_width as usize * active_height as usize;
    let mut pixels = vec![f32::NAN; output_len * 3];
    let mut output_quality = QualityMask::new(output_len);
    for output_y in 0..active_height as usize {
        for output_x in 0..active_width as usize {
            let x = left as usize + output_x;
            let y = top as usize + output_y;
            let input_index = y * corrected.width as usize + x;
            let output_index = output_y * active_width as usize + output_x;
            let out = output_index * 3;
            if !corrected.quality.valid[input_index] {
                for flag in corrected.quality.flags[input_index].iter() {
                    output_quality.invalidate(output_index, flag);
                }
                continue;
            }
            let mut sums = [0.0f32; 3];
            let mut counts = [0u32; 3];
            for dy in -1i32..=1 {
                for dx in -1i32..=1 {
                    let nx = x as i32 + dx;
                    let ny = y as i32 + dy;
                    if nx < left as i32
                        || ny < top as i32
                        || nx >= left.saturating_add(active_width) as i32
                        || ny >= top.saturating_add(active_height) as i32
                    {
                        continue;
                    }
                    let n = ny as usize * corrected.width as usize + nx as usize;
                    if !corrected.quality.valid[n] {
                        continue;
                    }
                    // CFA phase is anchored in full raw coordinates. Active
                    // area offsets therefore intentionally affect the phase.
                    let c = bayer_rgb_channel(filters, ny, nx);
                    sums[c] += corrected.samples[n];
                    counts[c] += 1;
                }
            }
            if counts.iter().all(|count| *count > 0) {
                for c in 0..3 {
                    pixels[out + c] = sums[c] / counts[c] as f32;
                }
            } else {
                output_quality.invalidate(output_index, QualityFlag::DemosaicInsufficientNeighbors);
            }
        }
    }
    Ok(CameraNativeTransmissionRgbF32 {
        width: active_width,
        height: active_height,
        pixels,
        quality: output_quality,
    })
}

/// Apply dark/open-gate capture correction to CFA samples. Invalid, saturated,
/// bad-pixel and missing-reference samples remain masked instead of receiving
/// an epsilon replacement.
pub(crate) fn correct_cfa_capture(
    mosaic: &RawMosaic,
    dark: Option<&RawMosaic>,
    open: Option<&RawMosaic>,
    bad_pixels: &[usize],
) -> Result<CorrectedCfaF32, String> {
    let expected = (mosaic.width as usize).saturating_mul(mosaic.height as usize);
    if mosaic.samples.len() != expected {
        return Err("Raw mosaic sample count mismatch".to_string());
    }
    let mut quality = QualityMask::new(expected);
    let mut corrected = vec![f32::NAN; expected];
    let Some(dark) = dark else {
        for i in 0..expected {
            quality.invalidate(i, QualityFlag::MissingReference);
        }
        return Ok(corrected_capture(mosaic, corrected, quality));
    };
    let Some(open) = open else {
        for i in 0..expected {
            quality.invalidate(i, QualityFlag::MissingReference);
        }
        return Ok(corrected_capture(mosaic, corrected, quality));
    };
    for reference in [dark, open] {
        if reference.width != mosaic.width
            || reference.height != mosaic.height
            || reference.samples.len() != expected
        {
            for i in 0..expected {
                quality.invalidate(i, QualityFlag::GeometryMismatch);
            }
            return Ok(corrected_capture(mosaic, corrected, quality));
        }
        if let Err(flags) = validate_reference_compatibility(&mosaic.metadata, &reference.metadata)
        {
            for i in 0..expected {
                for flag in &flags {
                    quality.invalidate(i, *flag);
                }
            }
        }
    }
    for area in &mosaic.metadata.masked_areas {
        let [top, left, bottom, right] = *area;
        for y in top.min(mosaic.height)..bottom.min(mosaic.height) {
            for x in left.min(mosaic.width)..right.min(mosaic.width) {
                quality.invalidate(
                    y as usize * mosaic.width as usize + x as usize,
                    QualityFlag::BadPixel,
                );
            }
        }
    }
    for index in mosaic
        .metadata
        .masked_pixels
        .iter()
        .map(|index| *index as usize)
        .chain(bad_pixels.iter().copied())
    {
        quality.invalidate(index, QualityFlag::BadPixel);
    }
    for i in 0..expected {
        let channel = match mosaic.metadata.cfa {
            CfaPattern::Bayer { filters } => bayer_raw_channel(
                filters,
                (i / mosaic.width as usize) as i32,
                (i % mosaic.width as usize) as i32,
            ),
            _ => 0,
        };
        let (value, flags) = capture_transmission(
            mosaic.samples[i] as f32,
            dark.samples[i] as f32,
            open.samples[i] as f32,
            mosaic.metadata.white_level[channel.min(3)],
        );
        for flag in flags {
            quality.invalidate(i, flag);
        }
        if quality.valid[i] {
            corrected[i] = value;
        }
    }
    Ok(corrected_capture(mosaic, corrected, quality))
}

fn corrected_capture(
    mosaic: &RawMosaic,
    samples: Vec<f32>,
    quality: QualityMask,
) -> CorrectedCfaF32 {
    let mut min = f32::INFINITY;
    let mut max = f32::NEG_INFINITY;
    let mut sum = 0.0f64;
    let mut count = 0usize;
    for (value, valid) in samples.iter().zip(&quality.valid) {
        if *valid && value.is_finite() {
            min = min.min(*value);
            max = max.max(*value);
            sum += *value as f64;
            count += 1;
        }
    }
    let diagnostics = CaptureDiagnostics {
        quality: quality.summary(),
        valid_min: (count > 0).then_some(min),
        valid_max: (count > 0).then_some(max),
        valid_mean: (count > 0).then_some((sum / count as f64) as f32),
    };
    CorrectedCfaF32 {
        width: mosaic.width,
        height: mosaic.height,
        samples,
        metadata: mosaic.metadata.clone(),
        quality,
        diagnostics,
    }
}

/// Phase-1 Measured input. Capture correction and fixed demosaic are real;
/// Capture Separation remains an explicit identity placeholder and therefore
/// does not claim physical density calibration.
fn orient_camera_native(
    source: CameraNativeTransmissionRgbF32,
    orientation: u16,
) -> CameraNativeTransmissionRgbF32 {
    if !matches!(orientation, 3 | 5 | 6) {
        return source;
    }
    let (target_width, target_height) = if matches!(orientation, 5 | 6) {
        (source.height, source.width)
    } else {
        (source.width, source.height)
    };
    let mut pixels = vec![f32::NAN; target_width as usize * target_height as usize * 3];
    let mut quality = QualityMask::new(target_width as usize * target_height as usize);
    for target_y in 0..target_height {
        for target_x in 0..target_width {
            let (source_x, source_y) = match orientation {
                3 => (source.width - 1 - target_x, source.height - 1 - target_y),
                // LibRaw flip 5/6 are 90-degree counter-clockwise/clockwise.
                5 => (source.width - 1 - target_y, target_x),
                6 => (target_y, source.height - 1 - target_x),
                _ => unreachable!(),
            };
            let source_index = source_y as usize * source.width as usize + source_x as usize;
            let target_index = target_y as usize * target_width as usize + target_x as usize;
            pixels[target_index * 3..target_index * 3 + 3]
                .copy_from_slice(&source.pixels[source_index * 3..source_index * 3 + 3]);
            quality.valid[target_index] = source.quality.valid[source_index];
            quality.flags[target_index] = source.quality.flags[source_index].clone();
        }
    }
    CameraNativeTransmissionRgbF32 {
        width: target_width,
        height: target_height,
        pixels,
        quality,
    }
}

pub(crate) fn decode_capture_corrected_input(
    mosaic: &RawMosaic,
    dark: Option<&RawMosaic>,
    open: Option<&RawMosaic>,
    bad_pixels: &[usize],
) -> Result<RelativeTransmissionRgbF32, String> {
    let corrected = correct_cfa_capture(mosaic, dark, open, bad_pixels)?;
    let diagnostics = corrected.diagnostics.clone();
    let camera_native = orient_camera_native(
        demosaic_bayer_fixed(&corrected)?,
        mosaic.metadata.orientation,
    );
    Ok(RelativeTransmissionRgbF32 {
        width: camera_native.width,
        height: camera_native.height,
        transmission: camera_native.pixels,
        quality: camera_native.quality,
        diagnostics,
        capture_separation: "identity_camera_native_transmission_v2_experimental".to_string(),
    })
}

/// Compatibility backend: LibRaw dcraw_process remains responsible for camera
/// WB and demosaic. Its output is a Camera RGB estimate used only to build the
/// Smart Auto ProPhoto Estimate path.
pub(crate) fn decode_smart_auto_rgb_with_policy<P: AsRef<Path>>(
    path: P,
    options: &DecodeOptions,
    white_balance: WhiteBalancePolicy,
) -> Result<CameraRgbEstimate, String> {
    // Smart Auto runs the normalized-as-shot balance so the film mask keeps its
    // headroom instead of being clipped into the transport ceiling.
    let decoded = extract_camera_rgb_with_policy(path, options, white_balance)
        .map_err(|error| error.to_string())?;
    let colors = decoded.colors as usize;
    if colors < 3 {
        return Err(format!("LibRaw Smart Auto output has {colors} channels"));
    }
    let bytes_per_sample = match decoded.bits {
        8 => 1,
        16 => 2,
        bits => return Err(format!("Unsupported LibRaw Smart Auto bit depth: {bits}")),
    };
    let pixel_stride = colors * bytes_per_sample;
    let expected = decoded.width as usize * decoded.height as usize * pixel_stride;
    if decoded.data.len() < expected {
        return Err("LibRaw Smart Auto output is truncated".to_string());
    }
    let mut pixels = Vec::with_capacity(decoded.width as usize * decoded.height as usize * 3);
    for pixel in decoded.data[..expected].chunks_exact(pixel_stride) {
        for channel in 0..3 {
            let offset = channel * bytes_per_sample;
            let value = if bytes_per_sample == 1 {
                pixel[offset] as f32 / 255.0
            } else {
                u16::from_ne_bytes([pixel[offset], pixel[offset + 1]]) as f32 / 65535.0
            };
            pixels.push(value);
        }
    }
    Ok(CameraRgbEstimate {
        width: decoded.width as u32,
        height: decoded.height as u32,
        pixels,
        camera_to_srgb: decoded.camera_to_srgb,
        label: "Smart Auto / ProPhoto Estimate source",
    })
}

/// How the decode stage should choose the per-channel white-balance gains.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WhiteBalancePolicy {
    /// Honour `DecodeOptions::use_camera_wb`, i.e. LibRaw's as-shot multipliers
    /// or its fixed daylight fallback. Kept for the A/B diagnostics.
    #[allow(dead_code)]
    FromOptions,
    /// Keep the camera's as-shot *ratio* but scale every channel so the
    /// strongest one is unity. Per-channel gains cancel in the base-relative
    /// density domain, so this preserves the colour while guaranteeing that
    /// decoding can only attenuate: no channel is pushed into the 16-bit
    /// ceiling, which is what clipped the highlights of film negatives.
    NormalizedAsShot,
}

/// The multipliers LibRaw resolved for a file, before any policy is applied.
/// Currently read by the manual white-balance diagnostics; the same values are
/// what a future processing report will surface to explain a decode.
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct RawWhiteBalance {
    /// The camera's as-shot balance, i.e. its AsShotNeutral, when the file
    /// records one.
    pub(crate) as_shot: Option<[f32; 4]>,
    /// The fixed daylight balance LibRaw falls back to when as-shot white
    /// balance is switched off. This is not "no white balance".
    pub(crate) daylight: [f32; 4],
}

/// Turn LibRaw's as-shot multipliers into a ratio whose strongest channel is
/// exactly one. `None` means the metadata is unusable and the caller should fall
/// back to an identity balance, i.e. a true "no white balance" decode.
pub(crate) fn normalized_as_shot_gains(cam_mul: [f32; 4]) -> Option<[f32; 4]> {
    let channels = [cam_mul[0], cam_mul[1], cam_mul[2]];
    if channels
        .iter()
        .any(|value| !value.is_finite() || *value <= 0.0)
    {
        return None;
    }
    let maximum = channels.iter().copied().fold(0.0f32, f32::max);
    (maximum > 0.0).then(|| {
        // The fourth slot is the second green site on a Bayer sensor. It has to
        // follow the first green, otherwise the two green sites are scaled
        // differently and the demosaic produces a colour cast from noise.
        let second_green = if cam_mul[3].is_finite() && cam_mul[3] > 0.0 {
            cam_mul[3]
        } else {
            cam_mul[1]
        };
        [
            channels[0] / maximum,
            channels[1] / maximum,
            channels[2] / maximum,
            second_green / maximum,
        ]
    })
}

#[cfg(not(target_os = "macos"))]
mod non_macos {
    use super::{CaptureConditions, CfaPattern, RawMetadata, RawMosaic};
    pub(crate) use rawlib::{DecodeOptions, ImageFormat, RawProcessor};
    use std::ffi::CStr;
    #[cfg(not(windows))]
    use std::ffi::CString;
    use std::os::raw::{c_char, c_int, c_uchar, c_ushort};
    use std::path::Path;

    #[derive(Debug, Clone)]
    pub(crate) struct CameraRgbData {
        pub(crate) width: u16,
        pub(crate) height: u16,
        pub(crate) colors: u16,
        pub(crate) bits: u16,
        pub(crate) data: Vec<u8>,
        /// Matrix used by LibRaw for camera RGB -> linear sRGB.
        pub(crate) camera_to_srgb: [f32; 9],
    }

    enum LibRawData {}

    #[repr(C)]
    struct LibRawProcessedImage {
        image_type: c_int,
        height: c_ushort,
        width: c_ushort,
        colors: c_ushort,
        bits: c_ushort,
        data_size: u32,
        data: [c_uchar; 1],
    }

    #[repr(C)]
    struct NexFilmRawMosaicInfo {
        raw_width: u32,
        raw_height: u32,
        active_left: u32,
        active_top: u32,
        active_width: u32,
        active_height: u32,
        raw_pitch: u32,
        filters: u32,
        cfa_kind: u32,
        orientation: u32,
        black_level: [f32; 4],
        white_level: [f32; 4],
        masked_areas: [[i32; 4]; 8],
        masked_area_count: u32,
        iso: f32,
        exposure_seconds: f32,
        camera_id: [c_char; 160],
        libraw_version: [c_char; 64],
    }

    #[repr(C)]
    struct NexFilmRawWhiteBalance {
        cam_mul: [f32; 4],
        pre_mul: [f32; 4],
        camera_wb_valid: i32,
    }

    impl NexFilmRawWhiteBalance {
        fn as_shot(&self) -> Option<[f32; 4]> {
            (self.camera_wb_valid != 0).then_some(self.cam_mul)
        }
    }

    impl Default for NexFilmRawMosaicInfo {
        fn default() -> Self {
            Self {
                raw_width: 0,
                raw_height: 0,
                active_left: 0,
                active_top: 0,
                active_width: 0,
                active_height: 0,
                raw_pitch: 0,
                filters: 0,
                cfa_kind: 0,
                orientation: 0,
                black_level: [0.0; 4],
                white_level: [0.0; 4],
                masked_areas: [[0; 4]; 8],
                masked_area_count: 0,
                iso: 0.0,
                exposure_seconds: 0.0,
                camera_id: [0; 160],
                libraw_version: [0; 64],
            }
        }
    }

    extern "C" {
        fn libraw_init(flags: c_int) -> *mut LibRawData;
        fn libraw_close(data: *mut LibRawData);
        #[cfg(not(windows))]
        fn libraw_open_file(data: *mut LibRawData, path: *const c_char) -> c_int;
        #[cfg(windows)]
        fn libraw_open_wfile(data: *mut LibRawData, path: *const u16) -> c_int;
        fn libraw_unpack(data: *mut LibRawData) -> c_int;
        fn libraw_dcraw_process(data: *mut LibRawData) -> c_int;
        fn libraw_dcraw_make_mem_image(
            data: *mut LibRawData,
            error: *mut c_int,
        ) -> *mut LibRawProcessedImage;
        fn libraw_dcraw_clear_mem(image: *mut LibRawProcessedImage);
        fn libraw_strerror(error: c_int) -> *const c_char;
        fn libraw_set_half_size(data: *mut LibRawData, value: c_int);
        fn libraw_set_use_camera_wb(data: *mut LibRawData, value: c_int);
        fn libraw_set_demosaic(data: *mut LibRawData, value: c_int);
        fn libraw_set_output_bps(data: *mut LibRawData, value: c_int);
        fn libraw_set_no_auto_bright(data: *mut LibRawData, value: c_int);
        fn libraw_set_output_color(data: *mut LibRawData, value: c_int);
        fn libraw_set_gamma(data: *mut LibRawData, index: c_int, value: f32);
        fn libraw_get_rgb_cam(data: *mut LibRawData, row: c_int, column: c_int) -> f32;
        fn nexfilm_raw_mosaic_info(
            data: *mut LibRawData,
            output: *mut NexFilmRawMosaicInfo,
        ) -> c_int;
        fn nexfilm_copy_raw_mosaic(
            data: *mut LibRawData,
            output: *mut c_ushort,
            capacity: usize,
        ) -> c_int;
        fn nexfilm_raw_white_balance(
            data: *mut LibRawData,
            output: *mut NexFilmRawWhiteBalance,
        ) -> c_int;
        fn nexfilm_raw_set_user_mul(
            data: *mut LibRawData,
            multipliers: *const f32,
            count: c_int,
        ) -> c_int;
    }

    struct Processor(*mut LibRawData);

    impl Drop for Processor {
        fn drop(&mut self) {
            if !self.0.is_null() {
                unsafe { libraw_close(self.0) };
            }
        }
    }

    fn error_message(code: c_int) -> String {
        unsafe {
            let message = libraw_strerror(code);
            if message.is_null() {
                format!("LibRaw error {code}")
            } else {
                format!(
                    "LibRaw error {code}: {}",
                    CStr::from_ptr(message).to_string_lossy()
                )
            }
        }
    }

    fn check(code: c_int) -> Result<(), String> {
        if code == 0 {
            Ok(())
        } else {
            Err(error_message(code))
        }
    }

    fn open_file(data: *mut LibRawData, path: &Path) -> Result<(), String> {
        #[cfg(windows)]
        {
            use std::os::windows::ffi::OsStrExt;
            let wide = path
                .as_os_str()
                .encode_wide()
                .chain(std::iter::once(0))
                .collect::<Vec<_>>();
            return check(unsafe { libraw_open_wfile(data, wide.as_ptr()) });
        }
        #[cfg(not(windows))]
        {
            let path = CString::new(path.to_string_lossy().as_bytes())
                .map_err(|error| format!("RAW path contains a null byte: {error}"))?;
            check(unsafe { libraw_open_file(data, path.as_ptr()) })
        }
    }

    fn fixed_c_string(buffer: &[c_char]) -> String {
        let end = buffer
            .iter()
            .position(|value| *value == 0)
            .unwrap_or(buffer.len());
        let bytes = buffer[..end]
            .iter()
            .map(|value| *value as u8)
            .collect::<Vec<_>>();
        String::from_utf8_lossy(&bytes).trim().to_string()
    }

    /// LibRaw owns recognition and unpacking. The NexFilm shim only copies the
    /// unpacked CFA buffer and metadata; dcraw_process is never called here.
    pub(crate) fn decode_raw_mosaic<P: AsRef<Path>>(path: P) -> Result<RawMosaic, String> {
        let processor = Processor(unsafe { libraw_init(0) });
        if processor.0.is_null() {
            return Err("Failed to initialize LibRaw".to_string());
        }
        open_file(processor.0, path.as_ref())?;
        check(unsafe { libraw_unpack(processor.0) })?;

        let mut info = NexFilmRawMosaicInfo::default();
        check(unsafe { nexfilm_raw_mosaic_info(processor.0, &mut info) })?;
        let sample_count = (info.raw_width as usize)
            .checked_mul(info.raw_height as usize)
            .ok_or_else(|| "LibRaw mosaic dimensions overflow".to_string())?;
        if sample_count == 0 {
            return Err("LibRaw returned an empty raw mosaic".to_string());
        }
        let mut samples = vec![0u16; sample_count];
        check(unsafe {
            nexfilm_copy_raw_mosaic(processor.0, samples.as_mut_ptr(), samples.len())
        })?;

        let cfa = match info.cfa_kind {
            1 => CfaPattern::Bayer {
                filters: info.filters,
            },
            2 => CfaPattern::XTrans,
            _ => CfaPattern::Unknown,
        };
        let masked_areas = info.masked_areas[..info.masked_area_count.min(8) as usize]
            .iter()
            .filter(|area| area.iter().all(|coordinate| *coordinate >= 0))
            .map(|area| area.map(|coordinate| coordinate as u32))
            .collect();
        Ok(RawMosaic {
            width: info.raw_width,
            height: info.raw_height,
            samples,
            metadata: RawMetadata {
                cfa,
                active_area: [
                    info.active_left,
                    info.active_top,
                    info.active_width,
                    info.active_height,
                ],
                raw_pitch_bytes: info.raw_pitch,
                black_level: info.black_level,
                white_level: info.white_level,
                masked_areas,
                masked_pixels: Vec::new(),
                orientation: info.orientation.min(u16::MAX as u32) as u16,
                iso: (info.iso > 0.0 && info.iso.is_finite()).then_some(info.iso),
                exposure_seconds: (info.exposure_seconds > 0.0
                    && info.exposure_seconds.is_finite())
                .then_some(info.exposure_seconds),
                camera_id: fixed_c_string(&info.camera_id),
                libraw_version: fixed_c_string(&info.libraw_version),
                capture_conditions: CaptureConditions::default(),
            },
        })
    }

    /// Multipliers LibRaw resolved while opening the file. Exposed so reports
    /// and diagnostics can explain which balance a decode actually used.
    #[allow(dead_code)]
    pub(crate) fn read_white_balance<P: AsRef<Path>>(
        path: P,
    ) -> Result<super::RawWhiteBalance, String> {
        let processor = Processor(unsafe { libraw_init(0) });
        if processor.0.is_null() {
            return Err("Failed to initialize LibRaw".to_string());
        }
        open_file(processor.0, path.as_ref())?;
        check(unsafe { libraw_unpack(processor.0) })?;
        let mut white_balance = NexFilmRawWhiteBalance {
            cam_mul: [0.0; 4],
            pre_mul: [0.0; 4],
            camera_wb_valid: 0,
        };
        let status = unsafe { nexfilm_raw_white_balance(processor.0, &mut white_balance) };
        if status != 0 {
            return Err(format!(
                "LibRaw white-balance metadata read failed ({status})"
            ));
        }
        Ok(super::RawWhiteBalance {
            as_shot: white_balance.as_shot(),
            daylight: white_balance.pre_mul,
        })
    }

    /// Read LibRaw's as-shot multipliers and install a ratio whose strongest
    /// channel is unity. Decoding can then only attenuate, so the orange mask of
    /// a colour negative is never pushed into the 16-bit ceiling.
    fn apply_normalized_as_shot_white_balance(processor: &Processor) -> Result<(), String> {
        let mut white_balance = NexFilmRawWhiteBalance {
            cam_mul: [0.0; 4],
            pre_mul: [0.0; 4],
            camera_wb_valid: 0,
        };
        let status = unsafe { nexfilm_raw_white_balance(processor.0, &mut white_balance) };
        if status != 0 {
            return Err(format!(
                "LibRaw white-balance metadata read failed ({status})"
            ));
        }
        // Without usable as-shot metadata, fall back to an identity balance:
        // that is a true "no white balance" decode, not LibRaw's fixed daylight
        // fallback which silently re-scales the channels.
        let gains = white_balance
            .as_shot()
            .and_then(super::normalized_as_shot_gains)
            .unwrap_or([1.0; 4]);
        let status = unsafe { nexfilm_raw_set_user_mul(processor.0, gains.as_ptr(), 4) };
        if status != 0 {
            return Err(format!("LibRaw white-balance override failed ({status})"));
        }
        Ok(())
    }

    /// Decode after LibRaw's black-level, white-balance and demosaic stages,
    /// but before its output-gamut matrix. The latter is applied by the caller
    /// in f32 so signed matrix results are not clipped to unsigned 16-bit.
    pub(crate) fn extract_camera_rgb_with_policy<P: AsRef<Path>>(
        path: P,
        options: &DecodeOptions,
        white_balance: super::WhiteBalancePolicy,
    ) -> Result<CameraRgbData, String> {
        let processor = Processor(unsafe { libraw_init(0) });
        if processor.0.is_null() {
            return Err("Failed to initialize LibRaw".to_string());
        }
        open_file(processor.0, path.as_ref())?;
        unsafe {
            libraw_set_half_size(processor.0, i32::from(options.half_size));
            libraw_set_use_camera_wb(processor.0, i32::from(options.use_camera_wb));
            libraw_set_demosaic(processor.0, options.demosaic_quality);
            libraw_set_output_bps(processor.0, options.output_bps);
            libraw_set_no_auto_bright(processor.0, i32::from(options.no_auto_bright));
            libraw_set_output_color(processor.0, 0);
            if options.linear_gamma {
                libraw_set_gamma(processor.0, 0, 1.0);
                libraw_set_gamma(processor.0, 1, 1.0);
            }
        }
        check(unsafe { libraw_unpack(processor.0) })?;
        if white_balance == super::WhiteBalancePolicy::NormalizedAsShot {
            apply_normalized_as_shot_white_balance(&processor)?;
        }
        check(unsafe { libraw_dcraw_process(processor.0) })?;

        let mut camera_to_srgb = [0.0; 9];
        for row in 0..3 {
            for column in 0..3 {
                camera_to_srgb[row * 3 + column] =
                    unsafe { libraw_get_rgb_cam(processor.0, row as c_int, column as c_int) };
            }
        }

        let mut error = 0;
        let image = unsafe { libraw_dcraw_make_mem_image(processor.0, &mut error) };
        if image.is_null() {
            return Err(error_message(error));
        }
        let image_ref = unsafe { &*image };
        let result = CameraRgbData {
            width: image_ref.width,
            height: image_ref.height,
            colors: image_ref.colors,
            bits: image_ref.bits,
            data: unsafe {
                std::slice::from_raw_parts(image_ref.data.as_ptr(), image_ref.data_size as usize)
                    .to_vec()
            },
            camera_to_srgb,
        };
        unsafe { libraw_dcraw_clear_mem(image) };
        Ok(result)
    }
}

#[cfg(not(target_os = "macos"))]
#[cfg_attr(not(test), allow(unused_imports))]
pub(crate) use non_macos::{
    decode_raw_mosaic, extract_camera_rgb_with_policy, read_white_balance, DecodeOptions,
    ImageFormat, RawProcessor,
};

#[cfg(target_os = "macos")]
mod macos {
    use super::{CaptureConditions, CfaPattern, RawMetadata, RawMosaic};
    use rsraw_sys as ffi;
    use std::ffi::{CStr, CString};
    use std::fmt;
    use std::path::Path;

    #[derive(Debug, Clone)]
    pub(crate) struct RawError {
        code: i32,
        message: String,
    }

    impl fmt::Display for RawError {
        fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            write!(formatter, "LibRaw error {}: {}", self.code, self.message)
        }
    }

    impl std::error::Error for RawError {}

    impl From<RawError> for String {
        fn from(error: RawError) -> Self {
            error.to_string()
        }
    }

    type Result<T> = std::result::Result<T, RawError>;

    #[derive(Debug, Clone)]
    pub(crate) struct ThumbnailData {
        pub(crate) format: ImageFormat,
        pub(crate) width: u16,
        pub(crate) height: u16,
        pub(crate) colors: u16,
        pub(crate) bits: u16,
        pub(crate) data: Vec<u8>,
    }

    #[derive(Debug, Clone)]
    pub(crate) struct CameraRgbData {
        pub(crate) width: u16,
        pub(crate) height: u16,
        pub(crate) colors: u16,
        pub(crate) bits: u16,
        pub(crate) data: Vec<u8>,
        pub(crate) camera_to_srgb: [f32; 9],
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub(crate) enum ImageFormat {
        Jpeg,
        Bitmap,
        Unknown(i32),
    }

    impl ImageFormat {
        fn from_code(code: i32) -> Self {
            match code as u32 {
                ffi::LibRaw_image_formats_LIBRAW_IMAGE_JPEG => Self::Jpeg,
                ffi::LibRaw_image_formats_LIBRAW_IMAGE_BITMAP => Self::Bitmap,
                _ => Self::Unknown(code),
            }
        }
    }

    #[derive(Debug, Clone, Copy)]
    pub(crate) struct DecodeOptions {
        pub(crate) half_size: bool,
        pub(crate) demosaic_quality: i32,
        pub(crate) output_bps: i32,
        pub(crate) no_auto_bright: bool,
        pub(crate) output_color: i32,
        pub(crate) linear_gamma: bool,
        pub(crate) use_camera_wb: bool,
    }

    pub(crate) struct RawProcessor {
        data: *mut ffi::libraw_data_t,
    }

    impl RawProcessor {
        pub(crate) fn new() -> Result<Self> {
            let data =
                unsafe { ffi::libraw_init(ffi::LibRaw_constructor_flags_LIBRAW_OPTIONS_NONE) };
            if data.is_null() {
                return Err(RawError {
                    code: -1,
                    message: "Failed to initialize LibRaw".to_string(),
                });
            }
            Ok(Self { data })
        }

        pub(crate) fn open_file<P: AsRef<Path>>(&mut self, path: P) -> Result<()> {
            let path = path.as_ref();
            if !path.exists() {
                return Err(RawError {
                    code: -1,
                    message: format!("File does not exist: {}", path.display()),
                });
            }
            let path_text = path.to_str().ok_or_else(|| RawError {
                code: -1,
                message: format!("Invalid path encoding: {}", path.display()),
            })?;
            let c_path = CString::new(path_text).map_err(|error| RawError {
                code: -1,
                message: format!("Path contains a null byte: {error}"),
            })?;
            let status = unsafe { ffi::libraw_open_file(self.data, c_path.as_ptr()) };
            self.check(status)
        }

        pub(crate) fn unpack_thumb(&mut self) -> Result<()> {
            let status = unsafe { ffi::libraw_unpack_thumb(self.data) };
            self.check(status)
        }

        pub(crate) fn get_thumbnail(&self) -> Result<ThumbnailData> {
            let mut error_code = 0;
            let image = unsafe { ffi::libraw_dcraw_make_mem_thumb(self.data, &mut error_code) };
            self.copy_image(image, error_code)
        }

        pub(crate) fn extract_image_with_options<P: AsRef<Path>>(
            path: P,
            options: &DecodeOptions,
        ) -> Result<ThumbnailData> {
            let mut processor = Self::new()?;
            processor.open_file(path)?;
            processor.set_decode_options(options);
            let status = unsafe { ffi::libraw_unpack(processor.data) };
            processor.check(status)?;
            let status = unsafe { ffi::libraw_dcraw_process(processor.data) };
            processor.check(status)?;

            let mut error_code = 0;
            let image =
                unsafe { ffi::libraw_dcraw_make_mem_image(processor.data, &mut error_code) };
            processor.copy_image(image, error_code)
        }

        pub(crate) fn version() -> String {
            unsafe {
                let version = ffi::libraw_version();
                if version.is_null() {
                    return "unknown".to_string();
                }
                CStr::from_ptr(version).to_string_lossy().into_owned()
            }
        }

        fn set_decode_options(&mut self, options: &DecodeOptions) {
            unsafe {
                (*self.data).params.half_size = i32::from(options.half_size);
                (*self.data).params.use_camera_wb = i32::from(options.use_camera_wb);
                ffi::libraw_set_demosaic(self.data, options.demosaic_quality);
                ffi::libraw_set_output_bps(self.data, options.output_bps);
                ffi::libraw_set_no_auto_bright(self.data, i32::from(options.no_auto_bright));
                ffi::libraw_set_output_color(self.data, options.output_color);
                if options.linear_gamma {
                    ffi::libraw_set_gamma(self.data, 0, 1.0);
                    ffi::libraw_set_gamma(self.data, 1, 1.0);
                }
            }
        }

        fn copy_image(
            &self,
            image: *mut ffi::libraw_processed_image_t,
            error_code: i32,
        ) -> Result<ThumbnailData> {
            if image.is_null() {
                return Err(self.error(error_code));
            }
            let image_guard = ProcessedImageGuard(image);
            let image = unsafe { &*image_guard.0 };
            let data = unsafe {
                std::slice::from_raw_parts(image.data.as_ptr(), image.data_size as usize).to_vec()
            };
            Ok(ThumbnailData {
                format: ImageFormat::from_code(image.type_ as i32),
                width: image.width,
                height: image.height,
                colors: image.colors,
                bits: image.bits,
                data,
            })
        }

        fn check(&self, status: i32) -> Result<()> {
            if status == ffi::LibRaw_errors_LIBRAW_SUCCESS {
                Ok(())
            } else {
                Err(self.error(status))
            }
        }

        fn error(&self, code: i32) -> RawError {
            let message = unsafe {
                let message = ffi::libraw_strerror(code);
                if message.is_null() {
                    "Unknown LibRaw error".to_string()
                } else {
                    CStr::from_ptr(message).to_string_lossy().into_owned()
                }
            };
            RawError { code, message }
        }
    }

    /// Multipliers LibRaw resolved while opening the file; see the non-macOS
    /// counterpart for why this is exposed.
    #[allow(dead_code)]
    pub(crate) fn read_white_balance<P: AsRef<Path>>(path: P) -> Result<super::RawWhiteBalance> {
        let mut processor = RawProcessor::new()?;
        processor.open_file(path)?;
        let status = unsafe { ffi::libraw_unpack(processor.data) };
        processor.check(status)?;
        let (cam_mul, pre_mul) = unsafe {
            (
                [
                    (*processor.data).rawdata.color.cam_mul[0],
                    (*processor.data).rawdata.color.cam_mul[1],
                    (*processor.data).rawdata.color.cam_mul[2],
                    (*processor.data).rawdata.color.cam_mul[3],
                ],
                [
                    (*processor.data).rawdata.color.pre_mul[0],
                    (*processor.data).rawdata.color.pre_mul[1],
                    (*processor.data).rawdata.color.pre_mul[2],
                    (*processor.data).rawdata.color.pre_mul[3],
                ],
            )
        };
        let as_shot = (cam_mul[0] > 0.0 && cam_mul[1] > 0.0 && cam_mul[2] > 0.0).then_some(cam_mul);
        Ok(super::RawWhiteBalance {
            as_shot,
            daylight: pre_mul,
        })
    }

    /// Installs a white-balance ratio whose strongest channel is unity, using the
    /// as-shot multipliers LibRaw resolved while opening the file.
    fn apply_normalized_as_shot_white_balance(processor: &RawProcessor) -> Result<()> {
        let cam_mul = unsafe {
            [
                (*processor.data).rawdata.color.cam_mul[0],
                (*processor.data).rawdata.color.cam_mul[1],
                (*processor.data).rawdata.color.cam_mul[2],
                (*processor.data).rawdata.color.cam_mul[3],
            ]
        };
        let gains = super::normalized_as_shot_gains(cam_mul).unwrap_or([1.0; 4]);
        unsafe {
            for (index, value) in gains.iter().enumerate() {
                (*processor.data).params.user_mul[index] = *value;
            }
            (*processor.data).params.use_camera_wb = 0;
            (*processor.data).params.use_auto_wb = 0;
        }
        Ok(())
    }

    pub(crate) fn extract_camera_rgb_with_policy<P: AsRef<Path>>(
        path: P,
        options: &DecodeOptions,
        white_balance: super::WhiteBalancePolicy,
    ) -> Result<CameraRgbData> {
        let mut processor = RawProcessor::new()?;
        processor.open_file(path)?;
        let mut camera_options = *options;
        camera_options.output_color = 0;
        processor.set_decode_options(&camera_options);
        let status = unsafe { ffi::libraw_unpack(processor.data) };
        processor.check(status)?;
        if white_balance == super::WhiteBalancePolicy::NormalizedAsShot {
            apply_normalized_as_shot_white_balance(&processor)?;
        }
        let status = unsafe { ffi::libraw_dcraw_process(processor.data) };
        processor.check(status)?;

        let mut camera_to_srgb = [0.0; 9];
        for row in 0..3 {
            for column in 0..3 {
                camera_to_srgb[row * 3 + column] =
                    unsafe { ffi::libraw_get_rgb_cam(processor.data, row as i32, column as i32) };
            }
        }

        let mut error_code = 0;
        let image = unsafe { ffi::libraw_dcraw_make_mem_image(processor.data, &mut error_code) };
        let decoded = processor.copy_image(image, error_code)?;
        Ok(CameraRgbData {
            width: decoded.width,
            height: decoded.height,
            colors: decoded.colors,
            bits: decoded.bits,
            data: decoded.data,
            camera_to_srgb,
        })
    }

    fn fixed_c_string(buffer: &[std::os::raw::c_char]) -> String {
        let end = buffer
            .iter()
            .position(|value| *value == 0)
            .unwrap_or(buffer.len());
        let bytes = buffer[..end]
            .iter()
            .map(|value| *value as u8)
            .collect::<Vec<_>>();
        String::from_utf8_lossy(&bytes).trim().to_string()
    }

    /// Read the CFA buffer exposed by `libraw_unpack`. This deliberately does
    /// not call `libraw_dcraw_process`, apply camera white balance, or use an
    /// output-color matrix. LibRaw remains responsible for file recognition
    /// and proprietary RAW decompression.
    pub(crate) fn decode_raw_mosaic<P: AsRef<Path>>(path: P) -> Result<RawMosaic> {
        let mut processor = RawProcessor::new()?;
        processor.open_file(path)?;
        let status = unsafe { ffi::libraw_unpack(processor.data) };
        processor.check(status)?;

        let data = unsafe { &*processor.data };
        let raw = &data.rawdata;
        if raw.raw_image.is_null() {
            return Err(RawError {
                code: -1,
                message: "LibRaw did not expose a single-plane CFA mosaic".to_string(),
            });
        }
        let width = u32::from(raw.sizes.raw_width);
        let height = u32::from(raw.sizes.raw_height);
        let sample_count = (width as usize)
            .checked_mul(height as usize)
            .ok_or_else(|| RawError {
                code: -1,
                message: "LibRaw mosaic dimensions overflow".to_string(),
            })?;
        let source_stride = raw.sizes.raw_pitch as usize / std::mem::size_of::<u16>();
        if sample_count == 0 || source_stride < width as usize {
            return Err(RawError {
                code: -1,
                message: "LibRaw returned invalid raw mosaic dimensions".to_string(),
            });
        }
        let mut samples = vec![0u16; sample_count];
        for row in 0..height as usize {
            let source = unsafe {
                std::slice::from_raw_parts(raw.raw_image.add(row * source_stride), width as usize)
            };
            samples[row * width as usize..(row + 1) * width as usize].copy_from_slice(source);
        }

        let identity = &raw.iparams;
        let color = &raw.color;
        let cfa = if identity.filters == ffi::LIBRAW_XTRANS {
            CfaPattern::XTrans
        } else if identity.filters != 0 {
            CfaPattern::Bayer {
                filters: identity.filters,
            }
        } else {
            CfaPattern::Unknown
        };
        let mut black_level = [0.0; 4];
        let mut white_level = [0.0; 4];
        for channel in 0..4 {
            black_level[channel] = color.black.saturating_add(color.cblack[channel]) as f32;
            white_level[channel] = if color.linear_max[channel] > 0 {
                color.linear_max[channel] as f32
            } else {
                color.maximum as f32
            };
        }
        let masked_areas = raw
            .sizes
            .mask
            .iter()
            .filter(|area| area.iter().any(|coordinate| *coordinate != 0))
            .filter(|area| area.iter().all(|coordinate| *coordinate >= 0))
            .map(|area| area.map(|coordinate| coordinate as u32))
            .collect();
        let make = if identity.normalized_make[0] != 0 {
            fixed_c_string(&identity.normalized_make)
        } else {
            fixed_c_string(&identity.make)
        };
        let model = if identity.normalized_model[0] != 0 {
            fixed_c_string(&identity.normalized_model)
        } else {
            fixed_c_string(&identity.model)
        };

        Ok(RawMosaic {
            width,
            height,
            samples,
            metadata: RawMetadata {
                cfa,
                active_area: [
                    u32::from(raw.sizes.left_margin),
                    u32::from(raw.sizes.top_margin),
                    u32::from(raw.sizes.width),
                    u32::from(raw.sizes.height),
                ],
                raw_pitch_bytes: raw.sizes.raw_pitch,
                black_level,
                white_level,
                masked_areas,
                masked_pixels: Vec::new(),
                orientation: raw.sizes.flip.max(0).min(u16::MAX as i32) as u16,
                iso: (data.other.iso_speed > 0.0 && data.other.iso_speed.is_finite())
                    .then_some(data.other.iso_speed),
                exposure_seconds: (data.other.shutter > 0.0 && data.other.shutter.is_finite())
                    .then_some(data.other.shutter),
                camera_id: format!("{make}|{model}"),
                libraw_version: RawProcessor::version(),
                capture_conditions: CaptureConditions::default(),
            },
        })
    }

    impl Drop for RawProcessor {
        fn drop(&mut self) {
            if !self.data.is_null() {
                unsafe { ffi::libraw_close(self.data) };
            }
        }
    }

    unsafe impl Send for RawProcessor {}

    struct ProcessedImageGuard(*mut ffi::libraw_processed_image_t);

    impl Drop for ProcessedImageGuard {
        fn drop(&mut self) {
            if !self.0.is_null() {
                unsafe { ffi::libraw_dcraw_clear_mem(self.0) };
            }
        }
    }
}

#[cfg(target_os = "macos")]
#[cfg_attr(not(test), allow(unused_imports))]
pub(crate) use macos::{
    decode_raw_mosaic, extract_camera_rgb_with_policy, read_white_balance, CameraRgbData,
    DecodeOptions, ImageFormat, RawProcessor,
};

#[cfg(test)]
mod contract_tests {
    use super::*;

    #[test]
    fn normalized_as_shot_gains_only_attenuate() {
        // A Hasselblad CFV 100C style AsShotNeutral: the camera boosts green and
        // blue relative to red, so the ratio is kept but never amplified.
        let gains = normalized_as_shot_gains([0.83, 1.0, 1.54, 1.0]).unwrap();
        assert!((gains[2] - 1.0).abs() < 1.0e-6);
        assert!(gains[0] < gains[1] && gains[1] < gains[2]);
        assert!(gains.iter().all(|value| *value <= 1.0 && *value > 0.0));
        // The ratio between channels is what carries colour information.
        assert!((gains[0] / gains[2] - 0.83 / 1.54).abs() < 1.0e-6);
        // The second green site must follow the first, or the demosaic sees two
        // differently scaled greens.
        assert!((gains[1] - gains[3]).abs() < 1.0e-6);
        let split_green = normalized_as_shot_gains([3.32, 1.0, 1.54, 0.0]).unwrap();
        assert!((split_green[3] - split_green[1]).abs() < 1.0e-6);
        assert!(split_green.iter().all(|value| *value <= 1.0 + 1.0e-6));

        // Unusable metadata has to fall back to a true no-balance decode rather
        // than to LibRaw's fixed daylight multipliers.
        assert!(normalized_as_shot_gains([0.0, 0.0, 0.0, 0.0]).is_none());
        assert!(normalized_as_shot_gains([-1.0, 1.0, 1.0, 1.0]).is_none());
        assert!(normalized_as_shot_gains([f32::NAN, 1.0, 1.0, 1.0]).is_none());
    }

    fn metadata() -> RawMetadata {
        RawMetadata {
            cfa: CfaPattern::Bayer {
                filters: 0x94949494,
            },
            active_area: [0, 0, 4, 4],
            raw_pitch_bytes: 8,
            black_level: [100.0; 4],
            white_level: [4000.0; 4],
            masked_areas: Vec::new(),
            masked_pixels: Vec::new(),
            orientation: 0,
            iso: Some(100.0),
            exposure_seconds: Some(0.01),
            camera_id: "test|camera".to_string(),
            libraw_version: "0.22-test".to_string(),
            capture_conditions: CaptureConditions {
                light_source_id: Some("light-a".to_string()),
                geometry_fingerprint: Some("geometry-a".to_string()),
            },
        }
    }

    fn mosaic(value: u16) -> RawMosaic {
        RawMosaic {
            width: 4,
            height: 4,
            samples: vec![value; 16],
            metadata: metadata(),
        }
    }

    #[test]
    fn raw_metadata_contract_contains_required_libraw_fields() {
        let metadata = metadata();
        assert!(matches!(metadata.cfa, CfaPattern::Bayer { .. }));
        assert_eq!(metadata.active_area, [0, 0, 4, 4]);
        assert_eq!(metadata.black_level, [100.0; 4]);
        assert_eq!(metadata.white_level, [4000.0; 4]);
        assert_eq!(metadata.raw_pitch_bytes, 8);
        assert_eq!(metadata.iso, Some(100.0));
        assert_eq!(metadata.exposure_seconds, Some(0.01));
        assert!(!metadata.camera_id.is_empty());
        assert!(!metadata.libraw_version.is_empty());
    }

    #[test]
    fn capture_ratio_is_not_clamped_or_epsilon_repaired() {
        let (value, flags) = capture_transmission(550.0, 100.0, 1000.0, 4000.0);
        assert!((value - 0.5).abs() < 1.0e-6);
        assert!(flags.is_empty());

        let (value, flags) = capture_transmission(50.0, 100.0, 1000.0, 4000.0);
        assert!(value < 0.0);
        assert!(flags.contains(&QualityFlag::NegativeSample));

        let (value, flags) = capture_transmission(500.0, 100.0, 100.0, 4000.0);
        assert!(value.is_nan());
        assert!(flags.contains(&QualityFlag::InvalidDenominator));

        let (_, flags) = capture_transmission(4000.0, 100.0, 1000.0, 4000.0);
        assert!(flags.contains(&QualityFlag::SaturatedSample));
        assert!(flags.contains(&QualityFlag::OutOfRange));
    }

    #[test]
    fn capture_correction_masks_bad_pixels_and_missing_references() {
        let sample = mosaic(550);
        let dark = mosaic(100);
        let open = mosaic(1000);
        let corrected = correct_cfa_capture(&sample, Some(&dark), Some(&open), &[5])
            .expect("capture correction");
        assert!((corrected.samples[0] - 0.5).abs() < 1.0e-6);
        assert!(!corrected.quality.valid[5]);
        assert!(corrected.quality.flags[5].contains(&QualityFlag::BadPixel));

        let missing =
            correct_cfa_capture(&sample, None, Some(&open), &[]).expect("missing reference mask");
        assert_eq!(missing.diagnostics.quality.valid_samples, 0);
        assert!(missing
            .quality
            .flags
            .iter()
            .all(|flags| flags.contains(&QualityFlag::MissingReference)));
    }

    #[test]
    fn reference_checks_cover_real_raw_exposure_iso_camera_and_geometry() {
        let sample = metadata();
        let mut reference = metadata();
        reference.iso = Some(200.0);
        reference.exposure_seconds = Some(0.02);
        reference.camera_id = "other|camera".to_string();
        reference.active_area = [0, 0, 3, 4];
        let flags = validate_reference_compatibility(&sample, &reference).unwrap_err();
        for expected in [
            QualityFlag::IsoMismatch,
            QualityFlag::ExposureMismatch,
            QualityFlag::CameraMismatch,
            QualityFlag::GeometryMismatch,
        ] {
            assert!(flags.contains(&expected), "missing {expected:?}");
        }
    }

    #[test]
    fn capture_corrected_contract_uses_fixed_camera_native_demosaic() {
        let sample = mosaic(550);
        let dark = mosaic(100);
        let open = mosaic(1000);
        let corrected = decode_capture_corrected_input(&sample, Some(&dark), Some(&open), &[])
            .expect("capture corrected input");
        assert_eq!((corrected.width, corrected.height), (4, 4));
        assert_eq!(
            corrected.capture_separation,
            "identity_camera_native_transmission_v2_experimental"
        );
        assert!(corrected.quality.valid.iter().all(|valid| *valid));
        assert!(corrected
            .transmission
            .iter()
            .all(|value| (*value - 0.5).abs() < 1.0e-6));
    }

    #[test]
    fn active_area_offset_preserves_full_raw_cfa_phase() {
        let filters = 0x94949494;
        let width = 6usize;
        let height = 6usize;
        let mut samples = Vec::with_capacity(width * height);
        for y in 0..height {
            for x in 0..width {
                samples.push(match bayer_rgb_channel(filters, y as i32, x as i32) {
                    0 => 0.2,
                    1 => 0.5,
                    _ => 0.8,
                });
            }
        }
        let mut shifted = metadata();
        shifted.active_area = [1, 1, 4, 4];
        shifted.cfa = CfaPattern::Bayer { filters };
        let corrected = CorrectedCfaF32 {
            width: width as u32,
            height: height as u32,
            samples,
            metadata: shifted,
            quality: QualityMask::new(width * height),
            diagnostics: CaptureDiagnostics::default(),
        };

        let demosaiced = demosaic_bayer_fixed(&corrected).expect("shifted active area");
        assert_eq!((demosaiced.width, demosaiced.height), (4, 4));
        assert!(demosaiced.quality.valid.iter().all(|valid| *valid));
        for pixel in demosaiced.pixels.chunks_exact(3) {
            assert!((pixel[0] - 0.2).abs() < 1.0e-6);
            assert!((pixel[1] - 0.5).abs() < 1.0e-6);
            assert!((pixel[2] - 0.8).abs() < 1.0e-6);
        }
    }

    #[test]
    fn libraw_orientation_rotates_pixels_quality_and_dimensions_together() {
        let mut quality = QualityMask::new(6);
        quality.invalidate(5, QualityFlag::BadPixel);
        let source = CameraNativeTransmissionRgbF32 {
            width: 2,
            height: 3,
            pixels: (0..6).flat_map(|value| [value as f32; 3]).collect(),
            quality,
        };

        let oriented = orient_camera_native(source, 6);
        assert_eq!((oriented.width, oriented.height), (3, 2));
        let values = oriented
            .pixels
            .chunks_exact(3)
            .map(|pixel| pixel[0] as u32)
            .collect::<Vec<_>>();
        assert_eq!(values, vec![4, 2, 0, 5, 3, 1]);
        assert!(!oriented.quality.valid[3]);
        assert!(oriented.quality.flags[3].contains(&QualityFlag::BadPixel));
    }

    #[test]
    fn four_channel_white_levels_drive_cfa_saturation_per_site() {
        let mut sample = mosaic(550);
        let mut dark = mosaic(100);
        let mut open = mosaic(1000);
        // 2x2 raw-channel pattern [0, 1; 3, 2], repeated by LibRaw's
        // filters encoding. This distinguishes the two green white levels.
        for mosaic in [&mut sample, &mut dark, &mut open] {
            mosaic.metadata.cfa = CfaPattern::Bayer {
                filters: 0xB4B4B4B4,
            };
        }
        sample.metadata.white_level = [4000.0, 1200.0, 4000.0, 500.0];
        let corrected = correct_cfa_capture(&sample, Some(&dark), Some(&open), &[])
            .expect("four-channel saturation");
        let filters = match sample.metadata.cfa {
            CfaPattern::Bayer { filters } => filters,
            _ => unreachable!(),
        };
        let channel_one = (0..16)
            .find(|index| bayer_raw_channel(filters, index / 4, index % 4) == 1)
            .unwrap() as usize;
        let channel_three = (0..16)
            .find(|index| bayer_raw_channel(filters, index / 4, index % 4) == 3)
            .unwrap() as usize;
        assert!(corrected.quality.valid[channel_one]);
        assert!(!corrected.quality.valid[channel_three]);
        assert!(corrected.quality.flags[channel_three].contains(&QualityFlag::SaturatedSample));
    }

    #[test]
    fn xtrans_remains_an_explicit_capture_corrected_fallback() {
        let mut sample = mosaic(550);
        let mut dark = mosaic(100);
        let mut open = mosaic(1000);
        for mosaic in [&mut sample, &mut dark, &mut open] {
            mosaic.metadata.cfa = CfaPattern::XTrans;
        }
        let error = decode_capture_corrected_input(&sample, Some(&dark), Some(&open), &[])
            .expect_err("X-Trans must not use the temporary Bayer demosaic");
        assert!(error.contains("Bayer"));
    }
}
