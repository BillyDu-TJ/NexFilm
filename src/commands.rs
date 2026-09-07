use crate::app_state::{
    BaseColor, CalibrationCapability, CalibrationConfigProfile, CalibrationLevel,
    CalibrationPayloadIssuer, CalibrationProfileAvailability, CalibrationProfilePayload,
    CalibrationProfileView, CalibrationQualityMaskArtifact, CalibrationQualityMaskSummary,
    CalibrationReference, CalibrationReferenceKind, CalibrationReferenceSummary,
    CalibrationValidRange, CalibrationValidationReport, CalibrationValidationStatus,
    CaptureCalibrationParameters, ContentRange, ContentRangeScope, DensityAnchor,
    DensityAnchorConfidence, DensityAnchorScope, DensityAnchorSource, DensityAnchors, EngineState,
    FilmItem, FilmMode, FilmstripItem, GeometryState, PipelineProcessingReport, PipelineState,
    ProcessingContract, RenderMode, Roll, RollBaseStatus, RollCalibrationFormat,
    RollCalibrationMode, RollCalibrationStatus, RollDmaxStatus, RollFrameStatus, RollToneStatus,
    TuningParams, CALIBRATION_PROFILE_PAYLOAD_VERSION, CALIBRATION_PROFILE_SCHEMA_VERSION,
};
use crate::batch_settings::{BatchCopyResult, ImageKey};
use crate::calibration_fit::{
    fit_capture_separation, CalibrationMeasurementSet, CalibrationPatch, FitOptions,
    ReferenceDomain,
};
use crate::capability_resolver::{
    resolve_pipeline, PipelineImageKind, PipelineResolution, PipelineResolverInput, ResolverProfile,
};
use crate::color_science::{
    apply_linear_matrix, canonical_output_space, compress_linear_srgb_for_density,
    convert_encoded_to_linear_rgb_with_matrix, identify_icc_profile, linear_conversion_matrix,
    parse_output_space, ColorSpaceId, DENSITY_CAPTURE_PROFILE, DENSITY_CAPTURE_WORKING_SPACE,
};
use crate::core_math::{
    apply_homography, apply_lens_distortion_uv, apply_perspective_uv,
    apply_post_gamma_adjustments_with_luma, density_luma, neutral_density_bounds,
    normalize_density_channel, shader_homography, sprocket_white_mask, DENSITY_LUMA_COEFFICIENTS,
};
use crate::persistence::{self, RAW_DECODE_VERSION};
use crate::pipeline::FilmPipeline;
use crate::scanner_profile::{import_local_profile, record_is_current, ScannerProfileRecord};
use serde::{Deserialize, Serialize};

use base64::{engine::general_purpose, Engine as _};
use flate2::{read::ZlibDecoder, write::ZlibEncoder, Compression};
use image::{
    imageops::FilterType, GenericImageView, ImageBuffer, ImageOutputFormat, Rgb, RgbImage,
};
use rayon::prelude::*;
use rfd::FileDialog;
use rusqlite::OptionalExtension;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::io::{BufReader, Cursor, Read, Seek, SeekFrom, Write};
use std::path::Path;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock, RwLock};
use std::sync::{MutexGuard, RwLockReadGuard, RwLockWriteGuard};
use tauri::State;
use tauri::{Emitter, Manager};

static NEXT_ID: AtomicUsize = AtomicUsize::new(1);
static NEXT_CALIBRATION_ID: AtomicUsize = AtomicUsize::new(1);
static EXPORT_TEMP_ID: AtomicUsize = AtomicUsize::new(1);
static RAYON_INIT: OnceLock<()> = OnceLock::new();
static EXPORT_ACTIVE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
const EXPORT_TEMP_PREFIX: &str = ".nexfilm-part-";
const STALE_DEVELOPMENT_OPERATION: &str = "STALE_DEVELOPMENT_OPERATION";
const CHANNEL_CONTROL_SCALE: f32 = 0.5;
const LUT_CONTROL_SCALE: f32 = 0.5;

const FALLBACK_THUMB: &str = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mM8c+bMfwAIGwK9t856VAAAAABJRU5ErkJggg==";
const IMPORT_PREVIEW_LONG_EDGE: u32 = 1024;
const PROXY_LONG_EDGE: f32 = 2560.0;
const MAX_PREVIEW_PROXY_LONG_EDGE: u32 = 4096;
// The WebGL proxy is a compact transport cache, not the domain-typed f32 buffer.
// Preserve a useful signed ProPhoto range instead of clipping it to display RGB.
const PROPHOTO_TRANSPORT_MIN: f32 = -1.0;
const PROPHOTO_TRANSPORT_MAX: f32 = 3.0;

fn claim_development_generation(
    state: &EngineState,
    id: &str,
    generation: u64,
) -> Result<Arc<AtomicU64>, String> {
    let epoch = state
        .development_generations
        .entry(id.to_string())
        .or_insert_with(|| Arc::new(AtomicU64::new(0)))
        .clone();
    loop {
        let current = epoch.load(Ordering::Acquire);
        if generation < current {
            return Err(STALE_DEVELOPMENT_OPERATION.into());
        }
        if generation == current
            || epoch
                .compare_exchange(current, generation, Ordering::AcqRel, Ordering::Acquire)
                .is_ok()
        {
            return Ok(epoch);
        }
    }
}

fn ensure_current_development_generation(epoch: &AtomicU64, generation: u64) -> Result<(), String> {
    (epoch.load(Ordering::Acquire) == generation)
        .then_some(())
        .ok_or_else(|| STALE_DEVELOPMENT_OPERATION.to_string())
}

fn forward_indexed_results_in_order<T>(
    receiver: std::sync::mpsc::Receiver<(usize, T)>,
    first_index: usize,
    mut forward: impl FnMut(T) -> bool,
) {
    let mut next_index = first_index;
    let mut completed = std::collections::BTreeMap::new();
    while let Ok((index, item)) = receiver.recv() {
        completed.insert(index, item);
        while let Some(item) = completed.remove(&next_index) {
            if !forward(item) {
                return;
            }
            next_index += 1;
        }
    }
}

#[cfg(test)]
mod development_generation_tests {
    use super::{
        claim_development_generation, ensure_current_development_generation, EngineState,
        STALE_DEVELOPMENT_OPERATION,
    };

    #[test]
    fn newer_generation_rejects_stale_development_writes() {
        let state = EngineState::new();
        let first = claim_development_generation(&state, "frame-a", 1).unwrap();
        let second = claim_development_generation(&state, "frame-a", 2).unwrap();

        assert_eq!(
            ensure_current_development_generation(&first, 1).unwrap_err(),
            STALE_DEVELOPMENT_OPERATION
        );
        ensure_current_development_generation(&second, 2).unwrap();
        assert_eq!(
            claim_development_generation(&state, "frame-a", 1).unwrap_err(),
            STALE_DEVELOPMENT_OPERATION
        );
    }

    #[test]
    fn parallel_import_results_are_forwarded_in_selection_order() {
        let (sender, receiver) = std::sync::mpsc::channel();
        for result in [
            (3, "frame-4"),
            (1, "frame-2"),
            (4, "frame-5"),
            (2, "frame-3"),
        ] {
            sender.send(result).unwrap();
        }
        drop(sender);

        let mut forwarded = Vec::new();
        super::forward_indexed_results_in_order(receiver, 1, |item| {
            forwarded.push(item);
            true
        });

        assert_eq!(forwarded, ["frame-2", "frame-3", "frame-4", "frame-5"]);
    }
}

fn resize_preview_image(mut img: image::DynamicImage, max_edge: u32) -> image::DynamicImage {
    let (w, h) = img.dimensions();
    if w.max(h) > max_edge {
        let ratio = max_edge as f32 / w.max(h) as f32;
        let new_w = (w as f32 * ratio).max(1.0) as u32;
        let new_h = (h as f32 * ratio).max(1.0) as u32;
        img = img.resize_exact(new_w, new_h, image::imageops::FilterType::Triangle);
    }
    img
}

fn write_jpeg_base64(img: image::DynamicImage, quality: u8) -> Option<String> {
    let mut cursor = Cursor::new(Vec::new());
    img.write_to(&mut cursor, ImageOutputFormat::Jpeg(quality))
        .ok()?;
    Some(general_purpose::STANDARD.encode(cursor.into_inner()))
}

fn encode_preview_jpeg_base64(
    img: image::DynamicImage,
    max_edge: u32,
    quality: u8,
) -> Option<String> {
    write_jpeg_base64(resize_preview_image(img, max_edge), quality)
}

fn is_better_preview_edge(candidate: u32, current: u32, target: u32) -> bool {
    match (candidate >= target, current >= target) {
        (true, false) => true,
        (false, true) => false,
        (true, true) => candidate < current,
        (false, false) => candidate > current,
    }
}

/// Locate the JPEG preview closest to the requested edge in a TIFF/NEF file.
/// Reading the embedded JPEG avoids LibRaw's full RAW decode during import.
fn extract_tiff_jpeg_preview(path: &str, target_edge: u32) -> Option<Vec<u8>> {
    let mut file = std::fs::File::open(path).ok()?;
    let file_len = file.metadata().ok()?.len();
    if file_len < 8 {
        return None;
    }
    let mut header = [0u8; 8];
    file.read_exact(&mut header).ok()?;
    let little_endian = &header[0..2] == b"II";
    if !little_endian && &header[0..2] != b"MM" {
        return None;
    }
    let read_u16 = |bytes: &[u8]| -> u16 {
        if little_endian {
            u16::from_le_bytes([bytes[0], bytes[1]])
        } else {
            u16::from_be_bytes([bytes[0], bytes[1]])
        }
    };
    let read_u32 = |bytes: &[u8]| -> u32 {
        if little_endian {
            u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
        } else {
            u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
        }
    };
    if read_u16(&header[2..4]) != 42 {
        return None;
    }

    let read_at = |file: &mut std::fs::File, offset: u64, size: usize| -> Option<Vec<u8>> {
        if offset.checked_add(size as u64)? > file_len {
            return None;
        }
        file.seek(SeekFrom::Start(offset)).ok()?;
        let mut bytes = vec![0u8; size];
        file.read_exact(&mut bytes).ok()?;
        Some(bytes)
    };
    let mut best: Option<(u64, u64, u32)> = None;
    let probe_len = file_len.min(512 * 1024) as usize;
    let mut probe_best: Option<(usize, u32, u32, usize)> = None;
    if let Some(prefix) = read_at(&mut file, 0, probe_len) {
        let mut cursor = 0usize;
        while cursor + 4 <= prefix.len() {
            if prefix[cursor..cursor + 2] != [0xff, 0xd8] {
                cursor += 1;
                continue;
            }
            if let Some((width, height)) = jpeg_dimensions(&prefix[cursor..]) {
                if let Some(end_rel) = prefix[cursor + 2..]
                    .windows(2)
                    .position(|window| window == [0xff, 0xd9])
                {
                    let end = cursor + 2 + end_rel + 2;
                    let edge = width.max(height);
                    if probe_best.is_none_or(|(_, current_width, current_height, _)| {
                        is_better_preview_edge(edge, current_width.max(current_height), target_edge)
                    }) {
                        probe_best = Some((cursor, width, height, end));
                    }
                }
            }
            cursor += 2;
        }
        if let Some((offset, width, height, end)) = probe_best {
            best = Some((offset as u64, (end - offset) as u64, width.max(height)));
        }
    }
    let type_size = |kind: u16| -> Option<usize> {
        Some(match kind {
            1 | 2 | 6 | 7 => 1,
            3 | 8 => 2,
            4 | 9 | 11 => 4,
            5 | 10 | 12 => 8,
            _ => return None,
        })
    };
    let value_bytes =
        |file: &mut std::fs::File, entry: &[u8], kind: u16, count: u32| -> Option<Vec<u8>> {
            let size = type_size(kind)?.checked_mul(count as usize)?;
            if size <= 4 {
                Some(entry[8..8 + size].to_vec())
            } else {
                let offset = read_u32(&entry[8..12]) as u64;
                read_at(file, offset, size)
            }
        };

    let mut pending = vec![read_u32(&header[4..8]) as u64];
    let mut visited = HashSet::new();
    while let Some(ifd_offset) = pending.pop() {
        if visited.len() >= 32
            || ifd_offset == 0
            || !visited.insert(ifd_offset)
            || ifd_offset + 2 > file_len
        {
            continue;
        }
        let Some(count_bytes) = read_at(&mut file, ifd_offset, 2) else {
            continue;
        };
        let count = (read_u16(&count_bytes) as usize).min(4096);
        let Some(entries) = read_at(&mut file, ifd_offset + 2, count.saturating_mul(12) + 4) else {
            continue;
        };
        let mut jpeg_offset = None;
        let mut jpeg_length = None;
        for index in 0..count {
            let entry = &entries[index * 12..index * 12 + 12];
            let tag = read_u16(&entry[0..2]);
            let kind = read_u16(&entry[2..4]);
            let item_count = read_u32(&entry[4..8]);
            let Some(bytes) = value_bytes(&mut file, entry, kind, item_count) else {
                continue;
            };
            if (tag == 0x0201 || tag == 0x0111) && item_count >= 1 && bytes.len() >= 4 {
                jpeg_offset = Some(read_u32(&bytes[0..4]) as u64);
            } else if (tag == 0x0202 || tag == 0x0117) && item_count >= 1 && bytes.len() >= 4 {
                jpeg_length = Some(read_u32(&bytes[0..4]) as u64);
            } else if tag == 0x014a && bytes.len() >= 4 {
                for chunk in bytes.chunks_exact(4) {
                    pending.push(read_u32(chunk) as u64);
                }
            } else if tag == 0x8769 && bytes.len() >= 4 {
                pending.push(read_u32(&bytes[0..4]) as u64);
            }
        }
        if let (Some(offset), Some(length)) = (jpeg_offset, jpeg_length) {
            if length >= 4
                && offset
                    .checked_add(length)
                    .is_some_and(|end| end <= file_len)
            {
                if let Some(signature) = read_at(&mut file, offset, 2) {
                    if signature == [0xff, 0xd8] {
                        let prefix = read_at(&mut file, offset, length.min(64 * 1024) as usize)?;
                        let (width, height) = jpeg_dimensions(&prefix).unwrap_or((0, 0));
                        if width == 0 || height == 0 {
                            continue;
                        }
                        let edge = width.max(height);
                        if best.is_none_or(|(_, _, current_edge)| {
                            is_better_preview_edge(edge, current_edge, target_edge)
                        }) {
                            best = Some((offset, length, edge));
                        }
                    }
                }
            }
        }
        let next_offset = ifd_offset + 2 + count as u64 * 12;
        if let Some(next) = read_at(&mut file, next_offset, 4) {
            let next = read_u32(&next) as u64;
            if next != 0 {
                pending.push(next);
            }
        }
    }
    let (offset, length, _) = best?;
    read_at(&mut file, offset, length as usize)
}

fn jpeg_dimensions(bytes: &[u8]) -> Option<(u32, u32)> {
    if bytes.len() < 4 || bytes[0..2] != [0xff, 0xd8] {
        return None;
    }
    let mut cursor = 2usize;
    while cursor + 4 <= bytes.len() {
        if bytes[cursor] != 0xff {
            cursor += 1;
            continue;
        }
        while cursor < bytes.len() && bytes[cursor] == 0xff {
            cursor += 1;
        }
        if cursor >= bytes.len() {
            break;
        }
        let marker = bytes[cursor];
        cursor += 1;
        if marker == 0xd8 || marker == 0xd9 {
            continue;
        }
        if cursor + 2 > bytes.len() {
            break;
        }
        let segment_len = u16::from_be_bytes([bytes[cursor], bytes[cursor + 1]]) as usize;
        if segment_len < 2 || cursor + segment_len > bytes.len() {
            break;
        }
        let is_sof = matches!(marker, 0xc0..=0xc3 | 0xc5..=0xc7 | 0xc9..=0xcb | 0xcd..=0xcf);
        if is_sof && segment_len >= 7 {
            let height = u16::from_be_bytes([bytes[cursor + 3], bytes[cursor + 4]]) as u32;
            let width = u16::from_be_bytes([bytes[cursor + 5], bytes[cursor + 6]]) as u32;
            return Some((width, height));
        }
        cursor += segment_len;
    }
    None
}

fn preview_image_needs_stretch(img: &image::DynamicImage) -> bool {
    matches!(
        img,
        image::DynamicImage::ImageLuma16(_)
            | image::DynamicImage::ImageLumaA16(_)
            | image::DynamicImage::ImageRgb16(_)
            | image::DynamicImage::ImageRgba16(_)
            | image::DynamicImage::ImageRgb32F(_)
            | image::DynamicImage::ImageRgba32F(_)
    )
}

fn percentile_from_histogram(hist: &[u32], rank: u64) -> u16 {
    let mut seen = 0u64;
    for (value, count) in hist.iter().enumerate() {
        seen += *count as u64;
        if seen > rank {
            return value as u16;
        }
    }
    u16::MAX
}

fn encode_stretched_preview_jpeg_base64(
    img: image::DynamicImage,
    max_edge: u32,
    quality: u8,
) -> Option<String> {
    let img = resize_preview_image(img, max_edge);
    let rgb16 = img.to_rgb16();
    let (width, height) = rgb16.dimensions();
    let raw = rgb16.as_raw();
    if raw.is_empty() {
        return None;
    }

    let mut hist = vec![0u32; 65536];
    for &value in raw {
        hist[value as usize] += 1;
    }

    let total = raw.len() as u64;
    let clip = (total / 200).min(total.saturating_sub(1) / 2);
    let low = percentile_from_histogram(&hist, clip);
    let high = percentile_from_histogram(&hist, total.saturating_sub(clip + 1));
    if high <= low {
        return write_jpeg_base64(image::DynamicImage::ImageRgb16(rgb16), quality);
    }

    let low_f = low as f32;
    let scale = 255.0 / (high as f32 - low_f);
    let mut out = RgbImage::new(width, height);
    out.as_mut()
        .par_chunks_exact_mut(3)
        .zip(raw.par_chunks_exact(3))
        .for_each(|(dst, src)| {
            dst[0] = ((src[0] as f32 - low_f) * scale).round().clamp(0.0, 255.0) as u8;
            dst[1] = ((src[1] as f32 - low_f) * scale).round().clamp(0.0, 255.0) as u8;
            dst[2] = ((src[2] as f32 - low_f) * scale).round().clamp(0.0, 255.0) as u8;
        });

    write_jpeg_base64(image::DynamicImage::ImageRgb8(out), quality)
}

fn encode_preview_bytes_base64(bytes: &[u8], max_edge: u32, quality: u8) -> String {
    if let Ok(img) = image::load_from_memory(bytes) {
        if let Some(encoded) = encode_preview_jpeg_base64(img, max_edge, quality) {
            return encoded;
        }
    }
    general_purpose::STANDARD.encode(bytes)
}

fn is_raw_extension(path: &str) -> bool {
    matches!(
        std::path::Path::new(path)
            .extension()
            .and_then(|ext| ext.to_str())
            .map(|ext| ext.to_ascii_lowercase())
            .as_deref(),
        Some(
            "dng"
                | "nef"
                | "nrw"
                | "cr2"
                | "cr3"
                | "arw"
                | "srf"
                | "sr2"
                | "raf"
                | "rw2"
                | "orf"
                | "ori"
                | "srw"
                | "raw"
                | "3fr"
                | "erf"
                | "kdc"
                | "dcr"
                | "iiq"
                | "mos"
                | "mrw"
                | "pef"
                | "x3f"
                | "rwl"
                | "fff"
        )
    )
}

fn is_fff_extension(path: &str) -> bool {
    std::path::Path::new(path)
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("fff"))
}

fn is_dng_extension(path: &str) -> bool {
    std::path::Path::new(path)
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("dng"))
}

const FFF_SCANNER_METADATA_LIMIT: u64 = 1024 * 1024;

fn contains_ascii_identifier(bytes: &[u8], identifier: &[u8]) -> bool {
    bytes
        .windows(identifier.len())
        .any(|window| window.eq_ignore_ascii_case(identifier))
}

fn contains_hasselblad_imacon_scanner_identifier(bytes: &[u8]) -> bool {
    // Hasselblad and Imacon film scanners belong to the Flextight family.
    // Requiring that device identity prevents camera-back FFF files from being
    // classified as scans merely because both formats use a TIFF container.
    contains_ascii_identifier(bytes, b"FLEXTIGHT")
        || (contains_ascii_identifier(bytes, b"IMACON")
            && contains_ascii_identifier(bytes, b"SCANNER"))
}

fn is_scanner_fff_tiff(path: &str) -> bool {
    if !is_fff_extension(path) {
        return false;
    }
    let Ok(mut file) = std::fs::File::open(path) else {
        return false;
    };
    let Ok(file_len) = file.metadata().map(|metadata| metadata.len()) else {
        return false;
    };
    let read_len = usize::try_from(file_len.min(FFF_SCANNER_METADATA_LIMIT)).unwrap_or(0);
    if read_len < 8 {
        return false;
    }
    let mut metadata = vec![0u8; read_len];
    if file.read_exact(&mut metadata).is_err()
        || !matches!(
            metadata.get(..4),
            Some([b'I', b'I', 42, 0] | [b'M', b'M', 0, 42])
        )
    {
        return false;
    }
    contains_hasselblad_imacon_scanner_identifier(&metadata)
}

fn decode_scanner_fff_tiff_page(
    path: &str,
    page: usize,
) -> Result<ImageBuffer<Rgb<u16>, Vec<u16>>, String> {
    use tiff::decoder::{Decoder, DecodingResult, Limits};
    use tiff::ColorType;

    let file = std::fs::File::open(path)
        .map_err(|error| format!("Cannot open scanner FFF {path}: {error}"))?;
    let mut limits = Limits::default();
    limits.decoding_buffer_size = 512 * 1024 * 1024;
    limits.intermediate_buffer_size = 512 * 1024 * 1024;
    let mut decoder = Decoder::new(BufReader::new(file))
        .map_err(|error| format!("Invalid scanner FFF/TIFF {path}: {error}"))?
        .with_limits(limits);
    decoder
        .seek_to_image(page)
        .map_err(|error| format!("Cannot read scanner FFF page {page}: {error}"))?;
    let (width, height) = decoder
        .dimensions()
        .map_err(|error| format!("Cannot read scanner FFF dimensions: {error}"))?;
    let color_type = decoder
        .colortype()
        .map_err(|error| format!("Cannot read scanner FFF color type: {error}"))?;
    let pixels = decoder
        .read_image()
        .map_err(|error| format!("Cannot decode scanner FFF pixels: {error}"))?;

    let pixel_count = (width as usize)
        .checked_mul(height as usize)
        .ok_or_else(|| "Scanner FFF dimensions overflowed".to_string())?;
    match (color_type, pixels) {
        (ColorType::RGB(16), DecodingResult::U16(samples)) => {
            if samples.len() != pixel_count * 3 {
                return Err("Scanner FFF returned an invalid RGB16 buffer".into());
            }
            ImageBuffer::from_raw(width, height, samples)
                .ok_or_else(|| "Cannot construct scanner FFF RGB16 image".to_string())
        }
        (ColorType::RGBA(16), DecodingResult::U16(samples)) => {
            if samples.len() != pixel_count * 4 {
                return Err("Scanner FFF returned an invalid RGBA16 buffer".into());
            }
            let rgb = samples
                .par_chunks_exact(4)
                .flat_map_iter(|pixel| [pixel[0], pixel[1], pixel[2]])
                .collect();
            ImageBuffer::from_raw(width, height, rgb)
                .ok_or_else(|| "Cannot construct scanner FFF RGB16 image".to_string())
        }
        (ColorType::RGB(8), DecodingResult::U8(samples)) => {
            if samples.len() != pixel_count * 3 {
                return Err("Scanner FFF returned an invalid RGB8 buffer".into());
            }
            let rgb = samples
                .par_iter()
                .map(|sample| u16::from(*sample) * 257)
                .collect();
            ImageBuffer::from_raw(width, height, rgb)
                .ok_or_else(|| "Cannot construct scanner FFF RGB16 image".to_string())
        }
        (ColorType::RGBA(8), DecodingResult::U8(samples)) => {
            if samples.len() != pixel_count * 4 {
                return Err("Scanner FFF returned an invalid RGBA8 buffer".into());
            }
            let rgb = samples
                .par_chunks_exact(4)
                .flat_map_iter(|pixel| {
                    [
                        u16::from(pixel[0]) * 257,
                        u16::from(pixel[1]) * 257,
                        u16::from(pixel[2]) * 257,
                    ]
                })
                .collect();
            ImageBuffer::from_raw(width, height, rgb)
                .ok_or_else(|| "Cannot construct scanner FFF RGB16 image".to_string())
        }
        (unsupported, _) => Err(format!(
            "Unsupported scanner FFF page format: {unsupported:?}"
        )),
    }
}

#[derive(Clone, Debug)]
struct ClassicTiffDirectory {
    little_endian: bool,
    width: u32,
    height: u32,
    bits_per_sample: Vec<u16>,
    compression: u16,
    photometric: u16,
    strip_offsets: Vec<u64>,
    samples_per_pixel: u16,
    rows_per_strip: u32,
    strip_byte_counts: Vec<u64>,
    planar_configuration: u16,
    orientation: u16,
    icc_profile: Option<Vec<u8>>,
    sub_ifd_offsets: Vec<u64>,
}

#[derive(Clone, Debug)]
struct ClassicTiffEntry {
    type_code: u16,
    count: u32,
    value: [u8; 4],
}

fn classic_tiff_type_size(type_code: u16) -> Option<usize> {
    match type_code {
        1 | 2 | 6 | 7 => Some(1),
        3 | 8 => Some(2),
        4 | 9 | 11 => Some(4),
        5 | 10 | 12 => Some(8),
        _ => None,
    }
}

fn read_classic_tiff_entry_data(
    file: &mut std::fs::File,
    file_len: u64,
    little: bool,
    entry: &ClassicTiffEntry,
    max_bytes: usize,
) -> Result<Vec<u8>, String> {
    let byte_len = usize::try_from(entry.count)
        .ok()
        .and_then(|count| classic_tiff_type_size(entry.type_code)?.checked_mul(count))
        .ok_or_else(|| "TIFF tag length overflowed".to_string())?;
    if byte_len > max_bytes {
        return Err(format!(
            "TIFF tag exceeds the {max_bytes}-byte metadata limit"
        ));
    }
    if byte_len <= 4 {
        return Ok(entry.value[..byte_len].to_vec());
    }
    let offset = if little {
        u32::from_le_bytes(entry.value)
    } else {
        u32::from_be_bytes(entry.value)
    } as u64;
    let end = offset
        .checked_add(byte_len as u64)
        .ok_or_else(|| "TIFF tag offset overflowed".to_string())?;
    if end > file_len {
        return Err("TIFF tag points outside the file".into());
    }
    file.seek(SeekFrom::Start(offset))
        .map_err(|error| format!("Cannot seek to TIFF tag: {error}"))?;
    let mut data = vec![0u8; byte_len];
    file.read_exact(&mut data)
        .map_err(|error| format!("Cannot read TIFF tag: {error}"))?;
    Ok(data)
}

fn classic_tiff_unsigned_values(
    file: &mut std::fs::File,
    file_len: u64,
    little: bool,
    entry: &ClassicTiffEntry,
) -> Result<Vec<u64>, String> {
    let data = read_classic_tiff_entry_data(file, file_len, little, entry, 16 * 1024 * 1024)?;
    match entry.type_code {
        1 | 7 => Ok(data.into_iter().map(u64::from).collect()),
        3 => Ok(data
            .chunks_exact(2)
            .map(|value| {
                u64::from(if little {
                    u16::from_le_bytes([value[0], value[1]])
                } else {
                    u16::from_be_bytes([value[0], value[1]])
                })
            })
            .collect()),
        4 => Ok(data
            .chunks_exact(4)
            .map(|value| {
                u64::from(if little {
                    u32::from_le_bytes(value.try_into().expect("four-byte TIFF value"))
                } else {
                    u32::from_be_bytes(value.try_into().expect("four-byte TIFF value"))
                })
            })
            .collect()),
        _ => Err(format!(
            "Unsupported TIFF integer tag type {}",
            entry.type_code
        )),
    }
}

fn read_classic_tiff_directory_impl(
    path: &str,
    page: usize,
    explicit_ifd_offset: Option<u64>,
) -> Result<ClassicTiffDirectory, String> {
    let mut file =
        std::fs::File::open(path).map_err(|error| format!("Cannot open TIFF {path}: {error}"))?;
    let file_len = file
        .metadata()
        .map_err(|error| format!("Cannot inspect TIFF {path}: {error}"))?
        .len();
    let mut header = [0u8; 8];
    file.read_exact(&mut header)
        .map_err(|error| format!("Cannot read TIFF header: {error}"))?;
    let little = match &header[..2] {
        b"II" => true,
        b"MM" => false,
        _ => return Err("Unsupported TIFF byte order".into()),
    };
    if read_endian_u16(&header, 2, little) != Some(42) {
        return Err("BigTIFF or invalid TIFF is not supported by the streaming decoder".into());
    }
    let mut ifd_offset = explicit_ifd_offset.unwrap_or(u64::from(
        read_endian_u32(&header, 4, little).ok_or("TIFF header has no IFD offset")?,
    ));
    let target_page = if explicit_ifd_offset.is_some() {
        0
    } else {
        page
    };
    let mut entries = HashMap::<u16, ClassicTiffEntry>::new();

    for current_page in 0..=target_page {
        if ifd_offset == 0 || ifd_offset.checked_add(2).is_none_or(|end| end > file_len) {
            return Err(format!("TIFF page {target_page} does not exist"));
        }
        file.seek(SeekFrom::Start(ifd_offset))
            .map_err(|error| format!("Cannot seek to TIFF page {current_page}: {error}"))?;
        let mut count_bytes = [0u8; 2];
        file.read_exact(&mut count_bytes)
            .map_err(|error| format!("Cannot read TIFF IFD count: {error}"))?;
        let entry_count =
            usize::from(read_endian_u16(&count_bytes, 0, little).ok_or("Invalid TIFF IFD count")?);
        if entry_count > 4096 {
            return Err("TIFF IFD contains too many entries".into());
        }
        let entries_len = entry_count
            .checked_mul(12)
            .ok_or_else(|| "TIFF IFD length overflowed".to_string())?;
        let entries_end = ifd_offset
            .checked_add(2)
            .and_then(|offset| offset.checked_add(entries_len as u64))
            .and_then(|offset| offset.checked_add(4))
            .ok_or_else(|| "TIFF IFD offset overflowed".to_string())?;
        if entries_end > file_len {
            return Err("TIFF IFD extends beyond the file".into());
        }
        let mut raw_entries = vec![0u8; entries_len];
        file.read_exact(&mut raw_entries)
            .map_err(|error| format!("Cannot read TIFF IFD entries: {error}"))?;
        let mut next_bytes = [0u8; 4];
        file.read_exact(&mut next_bytes)
            .map_err(|error| format!("Cannot read next TIFF IFD offset: {error}"))?;
        if current_page == target_page {
            entries.clear();
            for raw in raw_entries.chunks_exact(12) {
                let Some(tag) = read_endian_u16(raw, 0, little) else {
                    continue;
                };
                let Some(type_code) = read_endian_u16(raw, 2, little) else {
                    continue;
                };
                let Some(count) = read_endian_u32(raw, 4, little) else {
                    continue;
                };
                entries.insert(
                    tag,
                    ClassicTiffEntry {
                        type_code,
                        count,
                        value: raw[8..12].try_into().expect("TIFF value field"),
                    },
                );
            }
            break;
        }
        ifd_offset = u64::from(if little {
            u32::from_le_bytes(next_bytes)
        } else {
            u32::from_be_bytes(next_bytes)
        });
    }

    let mut values = |tag: u16| -> Result<Option<Vec<u64>>, String> {
        entries
            .get(&tag)
            .map(|entry| classic_tiff_unsigned_values(&mut file, file_len, little, entry))
            .transpose()
    };
    let first = |tag_values: Option<Vec<u64>>, default: u64| {
        tag_values
            .and_then(|values| values.first().copied())
            .unwrap_or(default)
    };
    let width = u32::try_from(first(values(256)?, 0)).map_err(|_| "Invalid TIFF width")?;
    let height = u32::try_from(first(values(257)?, 0)).map_err(|_| "Invalid TIFF height")?;
    if width == 0 || height == 0 {
        return Err("TIFF page has invalid dimensions".into());
    }
    let bits_per_sample = values(258)?
        .unwrap_or_else(|| vec![1])
        .into_iter()
        .map(|value| u16::try_from(value).map_err(|_| "Invalid TIFF bit depth".to_string()))
        .collect::<Result<Vec<_>, _>>()?;
    let compression =
        u16::try_from(first(values(259)?, 1)).map_err(|_| "Invalid TIFF compression")?;
    let photometric =
        u16::try_from(first(values(262)?, 0)).map_err(|_| "Invalid TIFF photometric type")?;
    let strip_offsets = values(273)?.unwrap_or_default();
    let samples_per_pixel =
        u16::try_from(first(values(277)?, 1)).map_err(|_| "Invalid TIFF sample count")?;
    let rows_per_strip = u32::try_from(first(values(278)?, u64::from(height)))
        .map_err(|_| "Invalid TIFF strip height")?;
    let strip_byte_counts = values(279)?.unwrap_or_default();
    let planar_configuration =
        u16::try_from(first(values(284)?, 1)).map_err(|_| "Invalid TIFF planar configuration")?;
    let orientation =
        u16::try_from(first(values(274)?, 1)).map_err(|_| "Invalid TIFF orientation")?;
    let sub_ifd_offsets = values(330)?.unwrap_or_default();
    let icc_profile = entries.get(&34675).and_then(|entry| {
        (entry.type_code == 7)
            .then(|| {
                read_classic_tiff_entry_data(&mut file, file_len, little, entry, 16 * 1024 * 1024)
                    .ok()
            })
            .flatten()
    });

    Ok(ClassicTiffDirectory {
        little_endian: little,
        width,
        height,
        bits_per_sample,
        compression,
        photometric,
        strip_offsets,
        samples_per_pixel,
        rows_per_strip: rows_per_strip.max(1),
        strip_byte_counts,
        planar_configuration,
        orientation,
        icc_profile,
        sub_ifd_offsets,
    })
}

fn read_classic_tiff_directory(path: &str, page: usize) -> Result<ClassicTiffDirectory, String> {
    read_classic_tiff_directory_impl(path, page, None)
}

fn read_classic_tiff_subdirectory(
    path: &str,
    ifd_offset: u64,
) -> Result<ClassicTiffDirectory, String> {
    read_classic_tiff_directory_impl(path, 0, Some(ifd_offset))
}

fn read_uncompressed_tiff_row(
    file: &mut std::fs::File,
    directory: &ClassicTiffDirectory,
    row: u32,
    row_bytes: usize,
) -> Result<Vec<u16>, String> {
    let strip_index = usize::try_from(row / directory.rows_per_strip)
        .map_err(|_| "TIFF strip index overflowed")?;
    let strip_offset = *directory
        .strip_offsets
        .get(strip_index)
        .ok_or("TIFF strip offset is missing")?;
    let row_in_strip = u64::from(row % directory.rows_per_strip);
    let offset = strip_offset
        .checked_add(
            row_in_strip
                .checked_mul(row_bytes as u64)
                .ok_or("TIFF row offset overflowed")?,
        )
        .ok_or("TIFF row offset overflowed")?;
    if let Some(byte_count) = directory.strip_byte_counts.get(strip_index) {
        let strip_end = strip_offset
            .checked_add(*byte_count)
            .ok_or("TIFF strip length overflowed")?;
        if offset
            .checked_add(row_bytes as u64)
            .is_none_or(|end| end > strip_end)
        {
            return Err("TIFF scanline exceeds its strip".into());
        }
    }
    file.seek(SeekFrom::Start(offset))
        .map_err(|error| format!("Cannot seek to TIFF scanline: {error}"))?;
    let mut bytes = vec![0u8; row_bytes];
    file.read_exact(&mut bytes)
        .map_err(|error| format!("Cannot read TIFF scanline: {error}"))?;
    let bits = directory.bits_per_sample[0];
    Ok(if bits == 8 {
        bytes
            .into_iter()
            .map(|value| u16::from(value) * 257)
            .collect()
    } else {
        bytes
            .chunks_exact(2)
            .map(|value| {
                if directory.little_endian {
                    u16::from_le_bytes([value[0], value[1]])
                } else {
                    u16::from_be_bytes([value[0], value[1]])
                }
            })
            .collect()
    })
}

fn decode_uncompressed_tiff_directory_reduced(
    path: &str,
    directory: ClassicTiffDirectory,
    target_long_edge: u32,
) -> Result<ImageBuffer<Rgb<u16>, Vec<u16>>, String> {
    if directory.compression != 1
        || !matches!(directory.photometric, 2 | 34892)
        || directory.planar_configuration != 1
        || directory.orientation != 1
        || directory.samples_per_pixel < 3
        || directory.strip_offsets.is_empty()
    {
        return Err("TIFF is not an uncompressed, top-left, chunky RGB image".into());
    }
    if directory.bits_per_sample.is_empty()
        || !directory
            .bits_per_sample
            .iter()
            .all(|bits| *bits == directory.bits_per_sample[0])
        || !matches!(directory.bits_per_sample[0], 8 | 16)
    {
        return Err("TIFF streaming decoder supports uniform RGB8/RGB16 samples only".into());
    }
    let scale = (target_long_edge.max(1) as f64 / f64::from(directory.width.max(directory.height)))
        .min(1.0);
    let output_width = (f64::from(directory.width) * scale).round().max(1.0) as u32;
    let output_height = (f64::from(directory.height) * scale).round().max(1.0) as u32;
    let samples = usize::from(directory.samples_per_pixel);
    let bytes_per_sample = usize::from(directory.bits_per_sample[0] / 8);
    let row_bytes = usize::try_from(directory.width)
        .ok()
        .and_then(|width| width.checked_mul(samples))
        .and_then(|count| count.checked_mul(bytes_per_sample))
        .ok_or_else(|| "TIFF scanline length overflowed".to_string())?;
    let output_len = usize::try_from(output_width)
        .ok()
        .and_then(|width| usize::try_from(output_height).ok()?.checked_mul(width))
        .and_then(|pixels| pixels.checked_mul(3))
        .ok_or_else(|| "Reduced TIFF buffer size overflowed".to_string())?;
    let mut output = vec![0u16; output_len];
    let mut file =
        std::fs::File::open(path).map_err(|error| format!("Cannot reopen TIFF {path}: {error}"))?;

    if output_width == directory.width && output_height == directory.height {
        for output_y in 0..output_height {
            let source_row =
                read_uncompressed_tiff_row(&mut file, &directory, output_y, row_bytes)?;
            let output_start =
                usize::try_from(output_y).unwrap() * usize::try_from(output_width).unwrap() * 3;
            let output_row = &mut output
                [output_start..output_start + usize::try_from(output_width).unwrap() * 3];
            output_row
                .par_chunks_exact_mut(3)
                .zip(source_row.par_chunks_exact(samples))
                .for_each(|(target, source)| target.copy_from_slice(&source[..3]));
        }
        return ImageBuffer::from_raw(output_width, output_height, output)
            .ok_or_else(|| "Cannot construct full-size TIFF image".to_string());
    }

    let x_samples = (0..output_width)
        .map(|x| {
            let position =
                ((f64::from(x) + 0.5) * f64::from(directory.width) / f64::from(output_width) - 0.5)
                    .clamp(0.0, f64::from(directory.width - 1));
            let left = position.floor() as u32;
            (
                left,
                (left + 1).min(directory.width - 1),
                (position - f64::from(left)) as f32,
            )
        })
        .collect::<Vec<_>>();

    for output_y in 0..output_height {
        let source_y = ((f64::from(output_y) + 0.5) * f64::from(directory.height)
            / f64::from(output_height)
            - 0.5)
            .clamp(0.0, f64::from(directory.height - 1));
        let top = source_y.floor() as u32;
        let bottom = (top + 1).min(directory.height - 1);
        let vertical_weight = (source_y - f64::from(top)) as f32;
        let top_row = read_uncompressed_tiff_row(&mut file, &directory, top, row_bytes)?;
        let bottom_row = if bottom == top {
            top_row.clone()
        } else {
            read_uncompressed_tiff_row(&mut file, &directory, bottom, row_bytes)?
        };
        let row_start =
            usize::try_from(output_y).unwrap() * usize::try_from(output_width).unwrap() * 3;
        let output_row =
            &mut output[row_start..row_start + usize::try_from(output_width).unwrap() * 3];
        for (output_x, (left, right, horizontal_weight)) in x_samples.iter().copied().enumerate() {
            let left = usize::try_from(left).unwrap() * samples;
            let right = usize::try_from(right).unwrap() * samples;
            for channel in 0..3 {
                let top_value = top_row[left + channel] as f32 * (1.0 - horizontal_weight)
                    + top_row[right + channel] as f32 * horizontal_weight;
                let bottom_value = bottom_row[left + channel] as f32 * (1.0 - horizontal_weight)
                    + bottom_row[right + channel] as f32 * horizontal_weight;
                output_row[output_x * 3 + channel] =
                    (top_value * (1.0 - vertical_weight) + bottom_value * vertical_weight)
                        .round()
                        .clamp(0.0, 65535.0) as u16;
            }
        }
    }

    ImageBuffer::from_raw(output_width, output_height, output)
        .ok_or_else(|| "Cannot construct reduced TIFF image".to_string())
}

fn decode_uncompressed_tiff_reduced(
    path: &str,
    target_long_edge: u32,
) -> Result<ImageBuffer<Rgb<u16>, Vec<u16>>, String> {
    let directory = read_classic_tiff_directory(path, 0)?;
    decode_uncompressed_tiff_directory_reduced(path, directory, target_long_edge)
}

fn decode_uncompressed_linear_dng_reduced(
    path: &str,
    target_long_edge: u32,
) -> Result<ImageBuffer<Rgb<u16>, Vec<u16>>, String> {
    let root = read_classic_tiff_directory(path, 0)?;
    let mut best = None::<ClassicTiffDirectory>;
    for offset in root.sub_ifd_offsets {
        let Ok(candidate) = read_classic_tiff_subdirectory(path, offset) else {
            continue;
        };
        if candidate.photometric != 34892
            || candidate.compression != 1
            || candidate.samples_per_pixel < 3
        {
            continue;
        }
        let candidate_pixels = u64::from(candidate.width) * u64::from(candidate.height);
        let best_pixels = best
            .as_ref()
            .map(|directory| u64::from(directory.width) * u64::from(directory.height))
            .unwrap_or(0);
        if candidate_pixels > best_pixels {
            best = Some(candidate);
        }
    }
    let directory = best.ok_or("DNG has no uncompressed LinearRaw RGB SubIFD")?;
    decode_uncompressed_tiff_directory_reduced(path, directory, target_long_edge)
}

fn linearize_scanner_fff(
    mut image: ImageBuffer<Rgb<u16>, Vec<u16>>,
    target: ColorSpaceId,
) -> ImageBuffer<Rgb<u16>, Vec<u16>> {
    const FFF_INPUT_GAMMA: f32 = 1.8;
    // Scanner 3F/FFF stores three already-sampled RGB channels with a 1.8
    // transfer curve. It does not carry a standard ICC tag for its device RGB,
    // so use the same fallback primary basis as an unprofiled scanner TIFF.
    let matrix = linear_conversion_matrix(ColorSpaceId::SRgb, target);
    image.as_mut().par_chunks_exact_mut(3).for_each(|pixel| {
        let encoded = [
            pixel[0] as f32 / 65535.0,
            pixel[1] as f32 / 65535.0,
            pixel[2] as f32 / 65535.0,
        ];
        let linear = apply_linear_matrix(encoded.map(|value| value.powf(FFF_INPUT_GAMMA)), matrix);
        for channel in 0..3 {
            pixel[channel] = (linear[channel].clamp(0.0, 1.0) * 65535.0).round() as u16;
        }
    });
    image
}

fn normalize_path(path: &str) -> String {
    path.replace('\\', "/").to_lowercase()
}

fn is_direct_image_extension(path: &str) -> bool {
    matches!(
        std::path::Path::new(path)
            .extension()
            .and_then(|ext| ext.to_str())
            .map(|ext| ext.to_ascii_lowercase())
            .as_deref(),
        Some("tif" | "tiff" | "jpg" | "jpeg" | "png")
    )
}

fn contains_noritsu_identifier(bytes: &[u8]) -> bool {
    bytes
        .windows(b"NORITSU".len())
        .any(|window| window.eq_ignore_ascii_case(b"NORITSU"))
        || bytes
            .windows(b"EZ Controller".len())
            .any(|window| window.eq_ignore_ascii_case(b"EZ Controller"))
}

fn tiff_ifd0_contains_noritsu_identifier(path: &str) -> bool {
    let Ok(mut file) = std::fs::File::open(path) else {
        return false;
    };
    let Ok(file_len) = file.metadata().map(|metadata| metadata.len()) else {
        return false;
    };
    let mut header = [0u8; 8];
    if file.read_exact(&mut header).is_err() {
        return false;
    }
    let little = match &header[0..2] {
        b"II" => true,
        b"MM" => false,
        _ => return false,
    };
    if read_endian_u16(&header, 2, little) != Some(42) {
        return false;
    }
    let Some(ifd_offset) = read_endian_u32(&header, 4, little).map(u64::from) else {
        return false;
    };
    if ifd_offset.checked_add(2).is_none_or(|end| end > file_len)
        || file.seek(SeekFrom::Start(ifd_offset)).is_err()
    {
        return false;
    }
    let mut count_bytes = [0u8; 2];
    if file.read_exact(&mut count_bytes).is_err() {
        return false;
    }
    let Some(entry_count) = read_endian_u16(&count_bytes, 0, little).map(usize::from) else {
        return false;
    };
    let Some(entries_len) = entry_count.checked_mul(12) else {
        return false;
    };
    let Some(entries_end) = ifd_offset
        .checked_add(2)
        .and_then(|start| start.checked_add(entries_len as u64))
    else {
        return false;
    };
    if entries_end > file_len {
        return false;
    }
    let mut entries = vec![0u8; entries_len];
    if file.read_exact(&mut entries).is_err() {
        return false;
    }

    for entry in entries.chunks_exact(12) {
        let Some(tag) = read_endian_u16(entry, 0, little) else {
            continue;
        };
        if !matches!(tag, 271 | 272 | 305) || read_endian_u16(entry, 2, little) != Some(2) {
            continue;
        }
        let Some(count) = read_endian_u32(entry, 4, little).map(u64::from) else {
            continue;
        };
        // Device identity strings are tiny; reject unreasonable counts instead
        // of allocating from malformed metadata.
        if count == 0 || count > 4096 {
            continue;
        }
        let value = if count <= 4 {
            entry.get(8..8 + count as usize).map(<[u8]>::to_vec)
        } else {
            let Some(value_offset) = read_endian_u32(entry, 8, little).map(u64::from) else {
                continue;
            };
            let Some(value_end) = value_offset.checked_add(count) else {
                continue;
            };
            if value_end > file_len || file.seek(SeekFrom::Start(value_offset)).is_err() {
                continue;
            }
            let mut value = vec![0u8; count as usize];
            file.read_exact(&mut value).ok().map(|_| value)
        };
        if value.is_some_and(|value| contains_noritsu_identifier(&value)) {
            return true;
        }
    }
    false
}

fn is_noritsu_rendered_image(path: &str) -> bool {
    let extension = std::path::Path::new(path)
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();

    if matches!(extension.as_str(), "tif" | "tiff") {
        return tiff_ifd0_contains_noritsu_identifier(path);
    }
    if !matches!(extension.as_str(), "jpg" | "jpeg") {
        return false;
    }

    // Noritsu writes the make/model in the JPEG APP1 block near the start of
    // the file. Limit the read so Auto Invert does not scan a full-size image
    // a second time merely to select its histogram policy.
    let Ok(file) = std::fs::File::open(path) else {
        return false;
    };
    let mut header = Vec::with_capacity(256 * 1024);
    if file.take(256 * 1024).read_to_end(&mut header).is_err() {
        return false;
    }
    contains_noritsu_identifier(&header)
}

fn extract_jpeg_icc_profile(bytes: &[u8]) -> Option<Vec<u8>> {
    if bytes.len() < 2 || bytes[0..2] != [0xff, 0xd8] {
        return None;
    }
    let mut position = 2usize;
    let mut chunks = Vec::<(u8, u8, Vec<u8>)>::new();
    while position + 4 <= bytes.len() {
        if bytes[position] != 0xff {
            position += 1;
            continue;
        }
        while position < bytes.len() && bytes[position] == 0xff {
            position += 1;
        }
        if position >= bytes.len() {
            break;
        }
        let marker = bytes[position];
        position += 1;
        if marker == 0xda || marker == 0xd9 {
            break;
        }
        if marker == 0xd8 || marker == 0x01 {
            continue;
        }
        let segment_length =
            u16::from_be_bytes([*bytes.get(position)?, *bytes.get(position + 1)?]) as usize;
        if segment_length < 2 {
            break;
        }
        let segment_end = position.checked_add(segment_length)?;
        if segment_end > bytes.len() {
            break;
        }
        let payload = &bytes[position + 2..segment_end];
        if marker == 0xe2 && payload.len() >= 14 && &payload[..12] == b"ICC_PROFILE\0" {
            chunks.push((payload[12], payload[13], payload[14..].to_vec()));
        }
        position = segment_end;
    }
    let total = chunks.first()?.1;
    if total == 0 || chunks.len() != usize::from(total) {
        return None;
    }
    chunks.sort_by_key(|chunk| chunk.0);
    if chunks
        .iter()
        .enumerate()
        .any(|(index, chunk)| chunk.1 != total || usize::from(chunk.0) != index + 1)
    {
        return None;
    }
    Some(chunks.into_iter().flat_map(|(_, _, chunk)| chunk).collect())
}

fn extract_png_icc_profile(bytes: &[u8]) -> Option<Vec<u8>> {
    if !bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        return None;
    }
    let mut position = 8usize;
    while position.checked_add(12)? <= bytes.len() {
        let length = u32::from_be_bytes(bytes[position..position + 4].try_into().ok()?) as usize;
        let chunk_end = position.checked_add(12)?.checked_add(length)?;
        if chunk_end > bytes.len() {
            return None;
        }
        let chunk_type = &bytes[position + 4..position + 8];
        if chunk_type == b"iCCP" {
            let payload = &bytes[position + 8..position + 8 + length];
            let name_end = payload.iter().position(|byte| *byte == 0)?;
            if payload.get(name_end + 1) != Some(&0) {
                return None;
            }
            let mut decoder = ZlibDecoder::new(&payload[name_end + 2..]);
            let mut profile = Vec::new();
            decoder.read_to_end(&mut profile).ok()?;
            return Some(profile);
        }
        if chunk_type == b"IEND" {
            break;
        }
        position = chunk_end;
    }
    None
}

fn read_endian_u16(bytes: &[u8], offset: usize, little: bool) -> Option<u16> {
    let value = bytes.get(offset..offset + 2)?;
    Some(if little {
        u16::from_le_bytes(value.try_into().ok()?)
    } else {
        u16::from_be_bytes(value.try_into().ok()?)
    })
}

fn read_endian_u32(bytes: &[u8], offset: usize, little: bool) -> Option<u32> {
    let value = bytes.get(offset..offset + 4)?;
    Some(if little {
        u32::from_le_bytes(value.try_into().ok()?)
    } else {
        u32::from_be_bytes(value.try_into().ok()?)
    })
}

fn embedded_input_profile(path: &str) -> Option<ColorSpaceId> {
    let extension = std::path::Path::new(path)
        .extension()
        .and_then(|extension| extension.to_str())?
        .to_ascii_lowercase();
    if matches!(extension.as_str(), "tif" | "tiff") {
        let profile = read_classic_tiff_directory(path, 0).ok()?.icc_profile?;
        return identify_icc_profile(&profile);
    }
    let bytes = std::fs::read(path).ok()?;
    let profile = match extension.as_str() {
        "jpg" | "jpeg" => extract_jpeg_icc_profile(&bytes),
        "png" => extract_png_icc_profile(&bytes),
        _ => None,
    }?;
    identify_icc_profile(&profile)
}

fn is_lightweight_direct_preview(path: &str) -> bool {
    matches!(
        std::path::Path::new(path)
            .extension()
            .and_then(|ext| ext.to_str())
            .map(|ext| ext.to_ascii_lowercase())
            .as_deref(),
        Some("jpg" | "jpeg" | "png")
    )
}

fn is_tiff_extension(path: &str) -> bool {
    matches!(
        std::path::Path::new(path)
            .extension()
            .and_then(|ext| ext.to_str())
            .map(|ext| ext.to_ascii_lowercase())
            .as_deref(),
        Some("tif" | "tiff")
    )
}

fn bundled_asset_dir(app_handle: &tauri::AppHandle, kind: &str) -> std::path::PathBuf {
    if let Ok(resource_dir) = app_handle.path().resource_dir() {
        let bundled = resource_dir.join("assets").join(kind);
        if bundled.is_dir() {
            return bundled;
        }
    }
    std::path::Path::new("assets").join(kind)
}

fn decode_direct_image_preview_base64(path: &str, max_edge: u32) -> Option<String> {
    let img = image::open(path).ok()?;
    if is_tiff_extension(path) || preview_image_needs_stretch(&img) {
        encode_stretched_preview_jpeg_base64(img, max_edge, 86)
    } else {
        encode_preview_jpeg_base64(img, max_edge, 86)
    }
}

fn extract_embedded_preview_base64(path: &str, max_edge: u32) -> Option<String> {
    if let Some(preview) = extract_tiff_jpeg_preview_base64(path, max_edge) {
        return Some(preview);
    }
    let mut processor = crate::raw_backend::RawProcessor::new().ok()?;
    processor.open_file(path).ok()?;
    processor.unpack_thumb().ok()?;
    let thumb = processor.get_thumbnail().ok()?;

    if thumb.format == crate::raw_backend::ImageFormat::Jpeg {
        return Some(encode_preview_bytes_base64(&thumb.data, max_edge, 86));
    }

    if thumb.format == crate::raw_backend::ImageFormat::Bitmap {
        let width = thumb.width as u32;
        let height = thumb.height as u32;

        if thumb.colors == 3 && thumb.bits == 8 {
            if let Some(img) =
                image::ImageBuffer::<image::Rgb<u8>, _>::from_raw(width, height, thumb.data.clone())
            {
                return encode_preview_jpeg_base64(
                    image::DynamicImage::ImageRgb8(img),
                    max_edge,
                    86,
                );
            }
        } else if thumb.colors == 3 && thumb.bits == 16 {
            if let Ok(img) = rgb16_image_from_bytes(
                width,
                height,
                thumb.colors as usize,
                thumb.bits,
                &thumb.data,
            ) {
                return encode_stretched_preview_jpeg_base64(
                    image::DynamicImage::ImageRgb16(img),
                    max_edge,
                    86,
                );
            }
        }
    }

    Some(encode_preview_bytes_base64(&thumb.data, max_edge, 86))
}

fn extract_tiff_jpeg_preview_base64(path: &str, max_edge: u32) -> Option<String> {
    let bytes = extract_tiff_jpeg_preview(path, max_edge)?;
    // Keep a display-ready JPEG untouched when it already fits the target.
    if bytes.len() <= 256 * 1024
        && jpeg_dimensions(&bytes).is_some_and(|(width, height)| width.max(height) <= max_edge)
    {
        return Some(general_purpose::STANDARD.encode(bytes));
    }
    let image = image::load_from_memory(&bytes).ok()?;
    encode_preview_jpeg_base64(image, max_edge, 86)
}

fn decode_fff_fallback_preview_base64(path: &str, max_edge: u32) -> Option<String> {
    let image = decode_reduced_tiff_for_working_space(path, max_edge).ok()?;
    encode_stretched_preview_jpeg_base64(image::DynamicImage::ImageRgb16(image), max_edge, 86)
}

fn decode_scanner_fff_preview_base64(path: &str, max_edge: u32) -> Option<String> {
    let preview = decode_scanner_fff_tiff_page(path, 1).ok()?;
    let preview_edge = preview.width().max(preview.height()).min(max_edge);
    let preview = image::DynamicImage::ImageRgb16(preview).to_rgb8();
    encode_preview_jpeg_base64(image::DynamicImage::ImageRgb8(preview), preview_edge, 86)
}

fn decode_tiff_page_preview_base64(path: &str, max_edge: u32) -> Option<String> {
    // Scanner exports commonly store a compact RGB page after the full-size
    // page. Reading that page first avoids touching hundreds of megabytes of
    // pixel data during import.
    for page in 1..=4 {
        let Ok(preview) = decode_scanner_fff_tiff_page(path, page) else {
            break;
        };
        if preview.width() >= 64 && preview.height() >= 64 {
            let preview = image::DynamicImage::ImageRgb16(preview).to_rgb8();
            return encode_preview_jpeg_base64(
                image::DynamicImage::ImageRgb8(preview),
                max_edge,
                86,
            );
        }
    }
    None
}

fn decode_reduced_tiff_preview_base64(path: &str, max_edge: u32) -> Option<String> {
    let image = decode_uncompressed_tiff_reduced(path, max_edge).ok()?;
    encode_stretched_preview_jpeg_base64(image::DynamicImage::ImageRgb16(image), max_edge, 86)
}

fn decode_dng_root_preview_base64(path: &str, max_edge: u32) -> Option<String> {
    let image = decode_uncompressed_tiff_reduced(path, max_edge).ok()?;
    encode_stretched_preview_jpeg_base64(image::DynamicImage::ImageRgb16(image), max_edge, 86)
}

/// Import-stage decoder. Camera RAW is embedded-preview-only. Scanner FFF uses
/// its reduced TIFF page, with a full-page fallback only when that page is
/// absent. TIFF first uses an embedded preview, then falls back to a downscaled
/// decode because many scanner TIFFs contain no thumbnail IFD.
fn decode_import_preview_base64(path: &str, max_edge: u32) -> Option<String> {
    if is_lightweight_direct_preview(path) {
        return decode_direct_image_preview_base64(path, max_edge);
    }

    if is_raw_extension(path) {
        if is_dng_extension(path) {
            return extract_tiff_jpeg_preview_base64(path, max_edge)
                .or_else(|| decode_dng_root_preview_base64(path, max_edge))
                .or_else(|| extract_embedded_preview_base64(path, max_edge));
        }
        if is_scanner_fff_tiff(path) {
            return decode_scanner_fff_preview_base64(path, max_edge)
                .or_else(|| decode_fff_fallback_preview_base64(path, max_edge));
        }
        return extract_embedded_preview_base64(path, max_edge).or_else(|| {
            is_fff_extension(path)
                .then(|| decode_fff_fallback_preview_base64(path, max_edge))
                .flatten()
        });
    }
    if is_tiff_extension(path) {
        return extract_tiff_jpeg_preview_base64(path, max_edge)
            .or_else(|| decode_tiff_page_preview_base64(path, max_edge))
            .or_else(|| decode_reduced_tiff_preview_base64(path, max_edge))
            .or_else(|| {
                let is_small = std::fs::metadata(path)
                    .is_ok_and(|metadata| metadata.len() <= 128 * 1024 * 1024);
                is_small
                    .then(|| decode_direct_image_preview_base64(path, max_edge))
                    .flatten()
            });
    }
    None
}

fn decode_import_preview_result(path: &str, max_edge: u32) -> Result<String, String> {
    std::fs::File::open(path).map_err(|error| format!("Cannot read {path}: {error}"))?;
    if let Some(preview) = decode_import_preview_base64(path, max_edge) {
        return Ok(preview);
    }

    // Some valid RAW files contain no embedded preview. Decode a reduced
    // proxy only for that uncommon fallback; a corrupt or unsupported file
    // now produces a real import error instead of a persisted 1x1 placeholder.
    let image = decode_image_buffer(path, DecodeMode::DevelopProxy)?;
    encode_stretched_preview_jpeg_base64(image::DynamicImage::ImageRgb16(image), max_edge, 86)
        .ok_or_else(|| format!("Could not encode an import preview for {path}"))
}

fn decode_develop_preview_base64(path: &str, max_edge: u32) -> Option<String> {
    if is_lightweight_direct_preview(path) {
        return decode_direct_image_preview_base64(path, max_edge);
    }
    if is_scanner_fff_tiff(path) {
        return decode_scanner_fff_preview_base64(path, max_edge)
            .or_else(|| decode_fff_fallback_preview_base64(path, max_edge));
    }
    if is_dng_extension(path) {
        return extract_tiff_jpeg_preview_base64(path, max_edge)
            .or_else(|| decode_dng_root_preview_base64(path, max_edge));
    }
    if is_tiff_extension(path) {
        return extract_tiff_jpeg_preview_base64(path, max_edge)
            .or_else(|| decode_tiff_page_preview_base64(path, max_edge))
            .or_else(|| decode_reduced_tiff_preview_base64(path, max_edge));
    }
    extract_embedded_preview_base64(path, max_edge).or_else(|| {
        is_fff_extension(path)
            .then(|| decode_fff_fallback_preview_base64(path, max_edge))
            .flatten()
    })
}

fn build_response_buffer(
    width: u32,
    height: u32,
    base_color: &BaseColor,
    pixels: &[u16],
    is_full_proxy: bool,
    base_analyzed: bool,
) -> Vec<u8> {
    let epsilon = 1e-6_f32;
    let t_r = (base_color.base_r as f32 / 65535.0).max(epsilon);
    let t_g = (base_color.base_g as f32 / 65535.0).max(epsilon);
    let t_b = (base_color.base_b as f32 / 65535.0).max(epsilon);
    let bd_r: f32 = -t_r.log10();
    let bd_g: f32 = -t_g.log10();
    let bd_b: f32 = -t_b.log10();

    let mut out_buffer = vec![0u8; (width * height * 8) as usize + 28];
    out_buffer[0..4].copy_from_slice(&width.to_le_bytes());
    out_buffer[4..8].copy_from_slice(&height.to_le_bytes());
    out_buffer[8..12].copy_from_slice(&bd_r.to_le_bytes());
    out_buffer[12..16].copy_from_slice(&bd_g.to_le_bytes());
    out_buffer[16..20].copy_from_slice(&bd_b.to_le_bytes());
    out_buffer[20..24].copy_from_slice(&(if is_full_proxy { 1u32 } else { 0u32 }).to_le_bytes());
    out_buffer[24..28].copy_from_slice(&(if base_analyzed { 1u32 } else { 0u32 }).to_le_bytes());

    let out_slice = &mut out_buffer[28..];
    pixels
        .par_chunks(3)
        .zip(out_slice.par_chunks_mut(8))
        .for_each(|(chunk, out_chunk)| {
            out_chunk[0..2].copy_from_slice(&chunk[0].to_le_bytes());
            out_chunk[2..4].copy_from_slice(&chunk[1].to_le_bytes());
            out_chunk[4..6].copy_from_slice(&chunk[2].to_le_bytes());
            out_chunk[6..8].copy_from_slice(&65535u16.to_le_bytes());
        });
    out_buffer
}

fn build_response_buffer_from_proxy(
    proxy: &ImageBuffer<Rgb<u16>, Vec<u16>>,
    base_color: &BaseColor,
    is_full_proxy: bool,
    base_analyzed: bool,
) -> Vec<u8> {
    let (width, height) = proxy.dimensions();
    build_response_buffer(
        width,
        height,
        base_color,
        proxy.as_raw().as_slice(),
        is_full_proxy,
        base_analyzed,
    )
}

fn build_response_buffer_from_proxy_with_state(
    proxy: &ImageBuffer<Rgb<u16>, Vec<u16>>,
    base_color: &BaseColor,
    pipeline_state: &PipelineState,
    quality: Option<&crate::raw_backend::QualityMask>,
    is_full_proxy: bool,
) -> Vec<u8> {
    let (width, height) = proxy.dimensions();
    let base_density = pipeline_base_density(pipeline_state, base_color);
    let base_analyzed = pipeline_has_base(pipeline_state, base_color);
    let flags = u32::from(base_analyzed)
        | match pipeline_state.contract {
            ProcessingContract::LegacyV1 => 0,
            ProcessingContract::CaptureCorrectedV11 => 4,
            _ => 2,
        };
    let mut out = vec![0u8; (width * height * 8) as usize + 28];
    out[0..4].copy_from_slice(&width.to_le_bytes());
    out[4..8].copy_from_slice(&height.to_le_bytes());
    for channel in 0..3 {
        let start = 8 + channel * 4;
        out[start..start + 4].copy_from_slice(&base_density[channel].to_le_bytes());
    }
    out[20..24].copy_from_slice(&u32::from(is_full_proxy).to_le_bytes());
    out[24..28].copy_from_slice(&flags.to_le_bytes());
    proxy
        .as_raw()
        .par_chunks_exact(3)
        .zip(out[28..].par_chunks_exact_mut(8))
        .enumerate()
        .for_each(|(index, (pixel, target))| {
            for channel in 0..3 {
                let start = channel * 2;
                target[start..start + 2].copy_from_slice(&pixel[channel].to_le_bytes());
            }
            let valid = quality
                .filter(|_| pipeline_state.contract == ProcessingContract::CaptureCorrectedV11)
                .is_none_or(|mask| mask.valid.get(index).copied().unwrap_or(false));
            target[6..8].copy_from_slice(&(if valid { u16::MAX } else { 0 }).to_le_bytes());
        });
    out
}

fn lock_mutex<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|e| e.into_inner())
}

fn read_lock<T>(lock: &RwLock<T>) -> RwLockReadGuard<'_, T> {
    lock.read().unwrap_or_else(|e| e.into_inner())
}

fn write_lock<T>(lock: &RwLock<T>) -> RwLockWriteGuard<'_, T> {
    lock.write().unwrap_or_else(|e| e.into_inner())
}

pub fn init_background_limits() {
    RAYON_INIT.get_or_init(|| {
        let threads = std::thread::available_parallelism()
            .map(|n| n.get().saturating_sub(2).clamp(1, 4))
            .unwrap_or(2);
        let _ = rayon::ThreadPoolBuilder::new()
            .num_threads(threads)
            .thread_name(|i| format!("nexfilm-rayon-{}", i))
            .build_global();
    });
}

struct ExportActiveGuard;

impl Drop for ExportActiveGuard {
    fn drop(&mut self) {
        EXPORT_ACTIVE.store(false, Ordering::SeqCst);
    }
}

fn compute_auto_base(proxy: &ImageBuffer<Rgb<u16>, Vec<u16>>) -> BaseColor {
    let raw = proxy.as_raw();
    let total_pixels = raw.len() / 3;

    // O(N) histogram: count occurrences of each 16-bit value per channel
    let mut hist_r = vec![0u32; 65536];
    let mut hist_g = vec![0u32; 65536];
    let mut hist_b = vec![0u32; 65536];

    for px in raw.chunks_exact(3) {
        hist_r[px[0] as usize] += 1;
        hist_g[px[1] as usize] += 1;
        hist_b[px[2] as usize] += 1;
    }

    // Threshold: top 1% of pixels
    let threshold = ((total_pixels as f64 * 0.01).ceil() as usize).max(1);

    // Find 99th percentile by scanning from high to low,
    // accumulating counts until we reach threshold
    let find_percentile = |hist: &[u32]| -> u16 {
        let mut accum = 0u32;
        for (val, &count) in hist.iter().enumerate().rev() {
            accum += count;
            if accum >= threshold as u32 {
                return val as u16;
            }
        }
        65535 // fallback (should never reach here)
    };

    BaseColor {
        base_r: find_percentile(&hist_r),
        base_g: find_percentile(&hist_g),
        base_b: find_percentile(&hist_b),
    }
}

fn compute_auto_base_f32(
    proxy: &ImageBuffer<Rgb<f32>, Vec<f32>>,
    geom: &GeometryState,
) -> Result<([f32; 3], f32), String> {
    // Smart Auto estimates a display-base candidate only from the selected
    // film geometry. It is never promoted to a physical anchor.
    if geom.calibration_points.is_none() {
        // A full scan may be dominated by an open light panel, sprockets, or
        // unrelated background. Without an explicit film area there is no
        // trustworthy D-min candidate, so use content-driven mapping instead.
        return Ok(([0.0; 3], 0.0));
    }
    let mut values = collect_film_area_rgb32(proxy, None, geom, true);
    if values.len() < 8 {
        // No trustworthy base candidate is a valid low-confidence outcome;
        // callers continue with the content-driven zero-reference fallback.
        return Ok(([0.0; 3], 0.0));
    }
    values.sort_unstable_by(|a, b| {
        density_luma([-a[0].log10(), -a[1].log10(), -a[2].log10()]).total_cmp(&density_luma([
            -b[0].log10(),
            -b[1].log10(),
            -b[2].log10(),
        ]))
    });
    let tail = ((values.len() as f32 * 0.02).ceil() as usize).clamp(1, values.len());
    let mut sum = [0.0f32; 3];
    for sample in values.iter().take(tail) {
        for channel in 0..3 {
            sum[channel] += -sample[channel].log10();
        }
    }
    let density = sum.map(|v| v / tail as f32);
    let confidence =
        (values.len() as f32 / (proxy.width() * proxy.height()).max(1) as f32).clamp(0.0, 1.0);
    Ok((density, confidence))
}

fn smart_auto_exclusion_counts(
    proxy: &ImageBuffer<Rgb<f32>, Vec<f32>>,
    geom: &GeometryState,
) -> (usize, usize, usize) {
    let points =
        geom.calibration_points
            .unwrap_or([[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]]);
    let min_x = points.iter().map(|p| p[0]).fold(f32::INFINITY, f32::min);
    let max_x = points
        .iter()
        .map(|p| p[0])
        .fold(f32::NEG_INFINITY, f32::max);
    let min_y = points.iter().map(|p| p[1]).fold(f32::INFINITY, f32::min);
    let max_y = points
        .iter()
        .map(|p| p[1])
        .fold(f32::NEG_INFINITY, f32::max);
    let mut open = 0;
    let mut saturated = 0;
    let mut invalid = 0;
    for (index, pixel) in proxy.as_raw().chunks_exact(3).enumerate() {
        let x =
            (index as u32 % proxy.width()) as f32 / proxy.width().saturating_sub(1).max(1) as f32;
        let y =
            (index as u32 / proxy.width()) as f32 / proxy.height().saturating_sub(1).max(1) as f32;
        if x < min_x || x > max_x || y < min_y || y > max_y {
            open += 1;
        } else if pixel.iter().any(|v| !v.is_finite() || *v <= 0.0) {
            invalid += 1;
        } else if pixel.iter().any(|v| *v >= 0.995) {
            saturated += 1;
        }
    }
    (open, saturated, invalid)
}

fn compute_auto_base_capture_corrected(
    proxy: &ImageBuffer<Rgb<f32>, Vec<f32>>,
    quality: &crate::raw_backend::QualityMask,
    geom: &GeometryState,
) -> Result<[f32; 3], String> {
    let mut values = collect_film_area_rgb32(proxy, Some(quality), geom, true);
    if values.len() < 8 {
        return Err("Capture Corrected contains no valid film-base samples".to_string());
    }
    values.sort_unstable_by(|left, right| {
        density_luma([-left[0].log10(), -left[1].log10(), -left[2].log10()]).total_cmp(
            &density_luma([-right[0].log10(), -right[1].log10(), -right[2].log10()]),
        )
    });
    let tail = ((values.len() as f32 * 0.02).ceil() as usize).clamp(1, values.len());
    let mut density = [0.0f32; 3];
    for sample in values.iter().take(tail) {
        for channel in 0..3 {
            density[channel] += -sample[channel].log10();
        }
    }
    Ok(density.map(|value| value / tail as f32))
}

fn replace_base_anchor_preserving_history(
    anchors: &mut DensityAnchors,
    replacement: DensityAnchor,
) {
    if let Some(previous) = anchors.d_min_base.replace(replacement) {
        if !anchors.retained_records.contains(&previous) {
            anchors.retained_records.push(previous);
        }
    }
}

fn base_color_from_density(density: [f32; 3]) -> BaseColor {
    let rgb = density.map(|value| (10.0_f32.powf(-value).clamp(0.0, 1.0) * 65535.0).round() as u16);
    BaseColor {
        base_r: rgb[0],
        base_g: rgb[1],
        base_b: rgb[2],
    }
}

fn prophoto_estimate_to_transport_proxy(
    estimate: &ImageBuffer<Rgb<f32>, Vec<f32>>,
) -> ImageBuffer<Rgb<u16>, Vec<u16>> {
    let mut transport = ImageBuffer::<Rgb<u16>, Vec<u16>>::new(estimate.width(), estimate.height());
    let span = PROPHOTO_TRANSPORT_MAX - PROPHOTO_TRANSPORT_MIN;
    transport
        .as_mut()
        .par_chunks_exact_mut(3)
        .zip(estimate.as_raw().par_chunks_exact(3))
        .for_each(|(target, source)| {
            for channel in 0..3 {
                let encoded = (source[channel] - PROPHOTO_TRANSPORT_MIN) / span;
                target[channel] = (encoded.clamp(0.0, 1.0) * 65535.0).round() as u16;
            }
        });
    transport
}

#[derive(Debug, Clone)]
struct CaptureCorrectedProxyData {
    image: ImageBuffer<Rgb<f32>, Vec<f32>>,
    quality: crate::raw_backend::QualityMask,
}

fn capture_reference(
    profile: &CalibrationConfigProfile,
    kind: CalibrationReferenceKind,
) -> Result<&CalibrationReference, String> {
    let mut matches = profile
        .references
        .iter()
        .filter(|reference| reference.kind == kind);
    let reference = matches
        .next()
        .ok_or_else(|| format!("Capture Corrected reference is missing: {kind:?}"))?;
    if matches.next().is_some() {
        return Err(format!(
            "Capture Corrected reference is ambiguous: {kind:?}"
        ));
    }
    let summarized = profile
        .payload
        .reference_frames
        .iter()
        .any(|summary| summary.kind == kind && summary.reference_id == reference.reference_id);
    if !summarized {
        return Err(format!(
            "Capture Corrected reference is not covered by the verified payload: {kind:?}"
        ));
    }
    Ok(reference)
}

fn decode_capture_corrected_image_buffer(
    path: &str,
    profile: &CalibrationConfigProfile,
) -> Result<CaptureCorrectedProxyData, String> {
    if !profile.payload.capture_is_verified(RAW_DECODE_VERSION) {
        return Err(profile
            .payload
            .capture_validation_error(RAW_DECODE_VERSION)
            .unwrap_or("capture_profile_not_verified")
            .to_string());
    }
    if !is_raw_extension(path) {
        return Err(
            "Capture Corrected requires a LibRaw-supported Bayer mosaic source".to_string(),
        );
    }
    let dark_reference = capture_reference(profile, CalibrationReferenceKind::DarkFrame)?;
    let open_reference = capture_reference(profile, CalibrationReferenceKind::OpenGate)?;
    for reference in [dark_reference, open_reference] {
        if let Some(error) = verified_reference_error(profile, reference) {
            return Err(error);
        }
    }
    let dark_path = &dark_reference.file_path;
    let open_path = &open_reference.file_path;
    for reference_path in [dark_path, open_path] {
        if !is_raw_extension(reference_path) {
            return Err(format!(
                "Capture Corrected reference is not a LibRaw mosaic: {reference_path}"
            ));
        }
    }

    let sample = crate::raw_backend::decode_raw_mosaic(path)?;
    let dark = crate::raw_backend::decode_raw_mosaic(dark_path)?;
    let open = crate::raw_backend::decode_raw_mosaic(open_path)?;
    for mosaic in [&sample, &dark, &open] {
        if mosaic.metadata.libraw_version != profile.payload.libraw_version {
            return Err(format!(
                "capture_libraw_version_mismatch|expected={}|actual={}",
                profile.payload.libraw_version, mosaic.metadata.libraw_version
            ));
        }
    }
    let parameters = profile
        .payload
        .capture_parameters
        .as_ref()
        .ok_or_else(|| "capture_parameters_missing".to_string())?;
    if parameters.light_source_id != profile.light_source.trim() {
        return Err("capture_light_source_changed".to_string());
    }
    let sample_geometry = crate::raw_backend::raw_geometry_fingerprint(&sample)?;
    if sample_geometry != parameters.geometry_fingerprint {
        return Err("capture_sample_geometry_mismatch".to_string());
    }
    if capture_hardware_fingerprint(profile, &sample)? != profile.payload.hardware_fingerprint {
        return Err("capture_hardware_fingerprint_mismatch".to_string());
    }
    let bad_pixels = profile
        .payload
        .quality_mask
        .as_ref()
        .map(|quality| {
            quality
                .bad_pixel_indices
                .iter()
                .map(|index| *index as usize)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let mut capture_corrected = crate::raw_backend::decode_capture_corrected_input(
        &sample,
        Some(&dark),
        Some(&open),
        &bad_pixels,
    )?;
    if let Some(model) = profile.payload.fit_model.as_ref() {
        for (index, pixel) in capture_corrected
            .transmission
            .chunks_exact_mut(3)
            .enumerate()
        {
            if !capture_corrected.quality.valid[index] {
                continue;
            }
            let input = [pixel[0], pixel[1], pixel[2]];
            let Some(output) = crate::calibration_fit::apply_capture_separation(model, input)
            else {
                capture_corrected
                    .quality
                    .invalidate(index, crate::raw_backend::QualityFlag::OutOfRange);
                pixel.fill(f32::NAN);
                continue;
            };
            pixel.copy_from_slice(&output);
        }
        capture_corrected.capture_separation =
            "CaptureSeparation3x3_user_measurement_fit".to_string();
    }
    let range = profile
        .payload
        .valid_range
        .as_ref()
        .ok_or_else(|| "capture_valid_range_missing".to_string())?;
    for (index, pixel) in capture_corrected
        .transmission
        .chunks_exact_mut(3)
        .enumerate()
    {
        if capture_corrected.quality.valid[index]
            && (0..3).any(|channel| {
                !pixel[channel].is_finite()
                    || pixel[channel] < range.minimum_transmission[channel]
                    || pixel[channel] > range.maximum_transmission[channel]
            })
        {
            capture_corrected
                .quality
                .invalidate(index, crate::raw_backend::QualityFlag::OutOfRange);
            pixel.fill(f32::NAN);
        }
    }
    let summary = capture_corrected.quality.summary();
    if summary.valid_samples == 0 {
        return Err(
            "Capture Corrected produced no valid relative-transmission samples".to_string(),
        );
    }
    eprintln!(
        "[RAW Pipeline] Capture Corrected / Relative Transmission RGB: valid={}/{} invalid_denominator={} negative={} saturated={} bad={} out_of_range={} separation={}",
        summary.valid_samples,
        summary.total_samples,
        summary.invalid_denominator,
        summary.negative_samples,
        summary.saturated_samples,
        summary.bad_pixels,
        summary.out_of_range,
        capture_corrected.capture_separation,
    );
    eprintln!(
        "[RAW Pipeline] Capture diagnostics: min={:?} max={:?} mean={:?}",
        capture_corrected.diagnostics.valid_min,
        capture_corrected.diagnostics.valid_max,
        capture_corrected.diagnostics.valid_mean,
    );
    let image = ImageBuffer::<Rgb<f32>, Vec<f32>>::from_raw(
        capture_corrected.width,
        capture_corrected.height,
        capture_corrected.transmission,
    )
    .ok_or_else(|| "Failed to allocate Relative Transmission RGB".to_string())?;
    Ok(CaptureCorrectedProxyData {
        image,
        quality: capture_corrected.quality,
    })
}

fn resize_capture_corrected_proxy(
    source: CaptureCorrectedProxyData,
    target_long_edge: u32,
) -> CaptureCorrectedProxyData {
    let (width, height) = source.image.dimensions();
    let ratio = (target_long_edge as f32 / width.max(height).max(1) as f32).min(1.0);
    if ratio >= 0.999 {
        return source;
    }
    let target_width = (width as f32 * ratio).max(1.0) as u32;
    let target_height = (height as f32 * ratio).max(1.0) as u32;
    let mut resized = ImageBuffer::<Rgb<f32>, Vec<f32>>::new(target_width, target_height);
    let mut quality =
        crate::raw_backend::QualityMask::new(target_width as usize * target_height as usize);
    for y in 0..target_height {
        for x in 0..target_width {
            let source_x = ((x as u64 * width as u64) / target_width as u64)
                .min(width.saturating_sub(1) as u64) as usize;
            let source_y = ((y as u64 * height as u64) / target_height as u64)
                .min(height.saturating_sub(1) as u64) as usize;
            let source_index = source_y * width as usize + source_x;
            let target_index = y as usize * target_width as usize + x as usize;
            *resized.get_pixel_mut(x, y) =
                *source.image.get_pixel(source_x as u32, source_y as u32);
            if !source.quality.valid[source_index] {
                for flag in source.quality.flags[source_index].iter() {
                    quality.invalidate(target_index, flag);
                }
            }
        }
    }
    CaptureCorrectedProxyData {
        image: resized,
        quality,
    }
}

fn relative_transmission_to_transport_proxy(
    relative_transmission: &CaptureCorrectedProxyData,
) -> ImageBuffer<Rgb<u16>, Vec<u16>> {
    let mut transport = ImageBuffer::<Rgb<u16>, Vec<u16>>::new(
        relative_transmission.image.width(),
        relative_transmission.image.height(),
    );
    transport
        .as_mut()
        .par_chunks_exact_mut(3)
        .zip(relative_transmission.image.as_raw().par_chunks_exact(3))
        .zip(relative_transmission.quality.valid.par_iter())
        .for_each(|((target, source), valid)| {
            if *valid {
                for channel in 0..3 {
                    target[channel] = (source[channel].clamp(0.0, 1.0) * 65535.0).round() as u16;
                }
            } else {
                target.fill(0);
            }
        });
    transport
}

fn linear_srgb_u16_to_prophoto_f32(
    source: &ImageBuffer<Rgb<u16>, Vec<u16>>,
) -> ImageBuffer<Rgb<f32>, Vec<f32>> {
    let matrix = linear_conversion_matrix(ColorSpaceId::SRgb, ColorSpaceId::ProPhotoRgb);
    let mut converted = ImageBuffer::<Rgb<f32>, Vec<f32>>::new(source.width(), source.height());
    converted
        .as_mut()
        .par_chunks_exact_mut(3)
        .zip(source.as_raw().par_chunks_exact(3))
        .for_each(|(target, pixel)| {
            target.copy_from_slice(&apply_linear_matrix(
                [
                    pixel[0] as f32 / 65535.0,
                    pixel[1] as f32 / 65535.0,
                    pixel[2] as f32 / 65535.0,
                ],
                matrix,
            ));
        });
    converted
}

fn compute_content_limits_f32(
    proxy: &ImageBuffer<Rgb<f32>, Vec<f32>>,
    quality: Option<&crate::raw_backend::QualityMask>,
    geom: &GeometryState,
    base_density: [f32; 3],
) -> Result<AutoColorLimits, String> {
    const SAMPLE_EDGE: u32 = 512;
    let (source_width, source_height) = proxy.dimensions();
    let longest = source_width.max(source_height).max(1);
    let sample_width = ((source_width as f64 / longest as f64) * SAMPLE_EDGE as f64)
        .round()
        .max(2.0) as u32;
    let sample_height = ((source_height as f64 / longest as f64) * SAMPLE_EDGE as f64)
        .round()
        .max(2.0) as u32;
    let points =
        geom.calibration_points
            .unwrap_or([[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]]);
    let min_x = points
        .iter()
        .map(|point| point[0])
        .fold(f32::INFINITY, f32::min);
    let max_x = points
        .iter()
        .map(|point| point[0])
        .fold(f32::NEG_INFINITY, f32::max);
    let min_y = points
        .iter()
        .map(|point| point[1])
        .fold(f32::INFINITY, f32::min);
    let max_y = points
        .iter()
        .map(|point| point[1])
        .fold(f32::NEG_INFINITY, f32::max);
    let homography = shader_homography(points);
    let collect = |inside_calibration_only: bool| {
        let mut samples = Vec::new();
        for y in 0..sample_height {
            for x in 0..sample_width {
                let base_uv = [
                    x as f32 / (sample_width - 1) as f32,
                    y as f32 / (sample_height - 1) as f32,
                ];
                let crop_uv = [
                    geom.crop_rect.x + base_uv[0] * geom.crop_rect.width,
                    geom.crop_rect.y + base_uv[1] * geom.crop_rect.height,
                ];
                if inside_calibration_only
                    && (crop_uv[0] < min_x
                        || crop_uv[0] > max_x
                        || crop_uv[1] < min_y
                        || crop_uv[1] > max_y)
                {
                    continue;
                }
                let Some(perspective_uv) = apply_perspective_uv(
                    crop_uv,
                    geom.perspective_vertical,
                    geom.perspective_horizontal,
                    geom.perspective_aspect,
                    geom.perspective_scale,
                ) else {
                    continue;
                };
                let Some(oriented_uv) = apply_homography(&homography, perspective_uv) else {
                    continue;
                };
                let Some(oriented_uv) = apply_lens_distortion_uv(oriented_uv, geom.lens_distortion)
                else {
                    continue;
                };
                let source_uv =
                    map_oriented_uv_to_source(oriented_uv, source_width, source_height, geom);
                let Some(raw) = sample_rgb32_nearest_checked(proxy, quality, source_uv) else {
                    continue;
                };
                let density = if quality.is_some() {
                    if raw.iter().any(|value| !value.is_finite() || *value <= 0.0) {
                        continue;
                    }
                    raw.map(|value| -value.log10())
                } else {
                    raw.map(|value| -value.max(1e-6).log10())
                };
                let density = [
                    density[0] - base_density[0],
                    density[1] - base_density[1],
                    density[2] - base_density[2],
                ];
                if density.iter().all(|value| value.is_finite()) {
                    samples.push(density);
                }
            }
        }
        samples
    };

    let mut samples = collect(true);
    if samples.len() < 64 && geom.calibration_points.is_none() {
        samples = collect(false);
    }
    if samples.len() < 8 {
        return Err("The selected film area contains too little image data.".to_string());
    }
    let (low, high) = co_sited_density_extremes(samples)
        .ok_or_else(|| "The selected film area has no usable density range.".to_string())?;
    Ok(AutoColorLimits {
        d_min: low,
        d_max: high,
        pipeline_state: None,
    })
}

fn pipeline_base_density(state: &PipelineState, base_color: &BaseColor) -> [f32; 3] {
    state
        .density_anchors
        .d_min_base
        .as_ref()
        .filter(|anchor| anchor_matches_resolved_contract(anchor, state))
        .map(|anchor| anchor.density)
        .unwrap_or_else(|| {
            if state.contract != ProcessingContract::LegacyV1 {
                // Smart Auto's candidate is analysis metadata only. The
                // content-driven fallback uses a zero reference and maps the
                // observed scene range directly for display.
                return [0.0; 3];
            }
            [base_color.base_r, base_color.base_g, base_color.base_b]
                .map(|value| -(value as f32 / 65535.0).max(1e-6).log10())
        })
}

fn pipeline_has_base(state: &PipelineState, base_color: &BaseColor) -> bool {
    if state.contract == ProcessingContract::LegacyV1 {
        *base_color != BaseColor::default()
    } else {
        state
            .density_anchors
            .d_min_base
            .as_ref()
            .is_some_and(|anchor| anchor_matches_resolved_contract(anchor, state))
            || (state.processing_report.base_source != "unresolved"
                && !state.processing_report.base_source.is_empty())
    }
}

fn anchor_matches_resolved_contract(anchor: &DensityAnchor, state: &PipelineState) -> bool {
    if anchor.provenance.input_domain != state.contract.input_domain()
        || anchor.provenance.legacy
        || anchor.density.iter().any(|value| !value.is_finite())
    {
        return false;
    }
    if state.contract == ProcessingContract::CaptureCorrectedV11 {
        anchor
            .provenance
            .calibration_profile_id
            .as_ref()
            .is_some_and(|id| !id.trim().is_empty())
            && anchor
                .provenance
                .calibration_payload_digest
                .as_ref()
                .is_some_and(|digest| !digest.trim().is_empty())
            && anchor.provenance.raw_decode_version == Some(RAW_DECODE_VERSION)
    } else {
        anchor.provenance.calibration_profile_id.is_none()
            && anchor.provenance.calibration_payload_digest.is_none()
    }
}

fn apply_roll_density_anchor_limits(
    limits: &mut AutoColorLimits,
    anchors: &DensityAnchors,
    base_density: [f32; 3],
) {
    if anchors.has_roll_base() {
        // Net density is measured relative to the sampled film base. The
        // Film Area estimate may describe content, but it must not move D-Min.
        limits.d_min = [0.0; 3];
    }
    if let Some(full_exposure) = anchors
        .d_max_full_exposure
        .as_ref()
        .filter(|anchor| anchor.scope == DensityAnchorScope::Roll)
    {
        // Likewise a sampled leader fixes D-Max while the missing endpoint,
        // if any, remains an estimate derived from Film Area.
        limits.d_max = [
            full_exposure.density[0] - base_density[0],
            full_exposure.density[1] - base_density[1],
            full_exposure.density[2] - base_density[2],
        ];
    }
}

fn share_smart_auto_density_scale(limits: &mut AutoColorLimits) {
    let low = density_luma(limits.d_min);
    let high = density_luma(limits.d_max);
    if low.is_finite() && high.is_finite() && high > low + 1.0e-6 {
        limits.d_min = [low; 3];
        limits.d_max = [high; 3];
    }
}

fn preserve_smart_auto_content_span(limits: &mut AutoColorLimits) {
    const MIN_DISPLAY_SPAN: f32 = 0.8;
    let low = limits.d_min[0];
    let high = limits.d_max[0];
    let span = high - low;
    if !span.is_finite() || span <= 1.0e-6 || span >= MIN_DISPLAY_SPAN {
        return;
    }
    let center = (low + high) * 0.5;
    limits.d_min = [center - MIN_DISPLAY_SPAN * 0.5; 3];
    limits.d_max = [center + MIN_DISPLAY_SPAN * 0.5; 3];
}

const PRESERVE_TONE_MIN_DENSITY_SPAN: f32 = 1.9;

fn preserve_tone_density_span(limits: &mut AutoColorLimits, anchors: &DensityAnchors) {
    // Smart Auto content ranges are display estimates. Expanding a short
    // scene to a fixed physical span makes fog and low-contrast frames black.
    // Only verified roll anchors may request endpoint completion.
    if !anchors.has_roll_base() && !anchors.has_roll_full_exposure() {
        return;
    }
    for channel in 0..3 {
        let span = limits.d_max[channel] - limits.d_min[channel];
        if !span.is_finite() || span >= PRESERVE_TONE_MIN_DENSITY_SPAN {
            continue;
        }
        if anchors.has_roll_full_exposure() {
            // A verified roll D-max remains fixed; extend only the estimated
            // endpoint so short content cannot silently become Full Tone.
            limits.d_min[channel] = limits.d_max[channel] - PRESERVE_TONE_MIN_DENSITY_SPAN;
        } else {
            // Keep a sampled roll D-min (or the estimated low endpoint) fixed.
            limits.d_max[channel] = limits.d_min[channel] + PRESERVE_TONE_MIN_DENSITY_SPAN;
        }
    }
}

fn compute_pristine_proxy(
    proxy: &ImageBuffer<Rgb<u16>, Vec<u16>>,
    prophoto_estimate_proxy: Option<&ImageBuffer<Rgb<f32>, Vec<f32>>>,
    relative_transmission_proxy: Option<&ImageBuffer<Rgb<f32>, Vec<f32>>>,
    relative_transmission_quality: Option<&crate::raw_backend::QualityMask>,
    base_color: &BaseColor,
    pipeline_state: &PipelineState,
    mode: FilmMode,
) -> ImageBuffer<Rgb<f32>, Vec<f32>> {
    let pipeline = FilmPipeline::from_state(pipeline_state, base_color, [0.0, 0.0, 0.0], mode);
    let (width, height) = proxy.dimensions();
    let mut pristine = ImageBuffer::<Rgb<f32>, Vec<f32>>::new(width, height);

    let out_pixels: &mut [f32] = pristine.as_mut();

    if pipeline_state.contract == ProcessingContract::CaptureCorrectedV11 {
        if let (Some(measured), Some(quality)) = (
            relative_transmission_proxy.filter(|image| image.dimensions() == proxy.dimensions()),
            relative_transmission_quality,
        ) {
            measured
                .as_raw()
                .par_chunks_exact(3)
                .zip(quality.valid.par_iter())
                .zip(out_pixels.par_chunks_exact_mut(3))
                .for_each(|((in_px, valid), out_px)| {
                    let density =
                        pipeline.compute_relative_density(&[in_px[0], in_px[1], in_px[2]], *valid);
                    out_px.copy_from_slice(&density.unwrap_or([f32::NAN; 3]));
                });
            return pristine;
        }
    }

    if let Some(prophoto_estimate) =
        prophoto_estimate_proxy.filter(|image| image.dimensions() == proxy.dimensions())
    {
        prophoto_estimate
            .as_raw()
            .par_chunks_exact(3)
            .zip(out_pixels.par_chunks_exact_mut(3))
            .for_each(|(in_px, out_px)| {
                out_px.copy_from_slice(
                    &pipeline.compute_true_density(&[in_px[0], in_px[1], in_px[2]]),
                );
            });
    } else {
        proxy
            .as_raw()
            .par_chunks_exact(3)
            .zip(out_pixels.par_chunks_exact_mut(3))
            .for_each(|(in_px, out_px)| {
                out_px.copy_from_slice(&pipeline.compute_true_density(&[
                    in_px[0] as f32 / 65535.0,
                    in_px[1] as f32 / 65535.0,
                    in_px[2] as f32 / 65535.0,
                ]));
            });
    }

    pristine
}

#[derive(Debug, Clone, Serialize)]
pub struct AutoColorLimits {
    pub d_min: [f32; 3],
    pub d_max: [f32; 3],
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pipeline_state: Option<PipelineState>,
}

#[inline]
fn map_oriented_uv_to_source(
    uv: [f32; 2],
    source_width: u32,
    source_height: u32,
    geom: &GeometryState,
) -> [f32; 2] {
    let source_width = source_width.max(1) as f32;
    let source_height = source_height.max(1) as f32;
    let angle = if geom.angle.abs() > 0.01 {
        geom.angle.to_radians()
    } else {
        0.0
    };
    let (
        layout_width,
        layout_height,
        diagonal,
        source_offset_x,
        source_offset_y,
        crop_offset_x,
        crop_offset_y,
    ) = if angle == 0.0 {
        (source_width, source_height, 0.0, 0.0, 0.0, 0.0, 0.0)
    } else {
        let sine = angle.sin();
        let cosine = angle.cos();
        let width = (source_width * cosine.abs() + source_height * sine.abs()).ceil();
        let height = (source_width * sine.abs() + source_height * cosine.abs()).ceil();
        let diagonal = source_width.hypot(source_height).ceil();
        (
            width,
            height,
            diagonal,
            ((diagonal - source_width) / 2.0).trunc(),
            ((diagonal - source_height) / 2.0).trunc(),
            ((diagonal - width) / 2.0).trunc(),
            ((diagonal - height) / 2.0).trunc(),
        )
    };
    let turns = geom.rotate_90_count.rem_euclid(4);
    let (oriented_width, oriented_height) = if turns % 2 == 0 {
        (layout_width, layout_height)
    } else {
        (layout_height, layout_width)
    };
    let mut x = uv[0] * oriented_width;
    let mut y = uv[1] * oriented_height;
    if geom.flip_h {
        x = oriented_width - x;
    }
    if geom.flip_v {
        y = oriented_height - y;
    }
    let (rotated_x, rotated_y) = match turns {
        1 => (y, layout_height - x),
        2 => (layout_width - x, layout_height - y),
        3 => (layout_width - y, x),
        _ => (x, y),
    };
    if angle == 0.0 {
        return [rotated_x / source_width, rotated_y / source_height];
    }

    let dx = rotated_x + crop_offset_x - diagonal / 2.0;
    let dy = rotated_y + crop_offset_y - diagonal / 2.0;
    let sine = angle.sin();
    let cosine = angle.cos();
    [
        (cosine * dx + sine * dy + diagonal / 2.0 - source_offset_x) / source_width,
        (-sine * dx + cosine * dy + diagonal / 2.0 - source_offset_y) / source_height,
    ]
}

fn density_histogram_extremes(histogram: &[u32], total: usize) -> (u16, u16) {
    let spike_threshold = total as f64 * 0.10;
    let tail_threshold = total as f64 * 0.01;
    let spike_guard = total as f64 * 0.20;

    let mut low = 0u16;
    let mut accumulated = 0usize;
    for (value, count) in histogram.iter().copied().enumerate() {
        if count as f64 > spike_threshold && (accumulated as f64) < spike_guard {
            continue;
        }
        accumulated += count as usize;
        if accumulated as f64 >= tail_threshold {
            low = value as u16;
            break;
        }
    }

    let mut high = u16::MAX;
    accumulated = 0;
    for (value, count) in histogram.iter().copied().enumerate().rev() {
        if count as f64 > spike_threshold && (accumulated as f64) < spike_guard {
            continue;
        }
        accumulated += count as usize;
        if accumulated as f64 >= tail_threshold {
            high = value as u16;
            break;
        }
    }
    (low, high)
}

fn co_sited_density_extremes(mut samples: Vec<[f32; 3]>) -> Option<([f32; 3], [f32; 3])> {
    if samples.len() < 2 {
        return None;
    }
    samples.sort_unstable_by(|left, right| density_luma(*left).total_cmp(&density_luma(*right)));

    // Averaging the lowest/highest 2% centers each estimate near the existing
    // 1st/99th percentile while ensuring all three channel values come from
    // the same pixels. This preserves per-channel correction without allowing
    // unrelated colored objects to define separate channel endpoints.
    let tail_count =
        ((samples.len() as f64 * 0.02).ceil() as usize).clamp(1, (samples.len() / 2).max(1));
    let average = |tail: &[[f32; 3]]| {
        let mut sum = [0.0f64; 3];
        for sample in tail {
            for channel in 0..3 {
                sum[channel] += sample[channel] as f64;
            }
        }
        sum.map(|value| (value / tail.len() as f64) as f32)
    };
    let low = average(&samples[..tail_count]);
    let high = average(&samples[samples.len() - tail_count..]);
    (0..3)
        .all(|channel| high[channel] - low[channel] > 1e-6)
        .then_some((low, high))
}

fn compute_auto_color_limits(
    proxy: &ImageBuffer<Rgb<u16>, Vec<u16>>,
    geom: &GeometryState,
    base_color: &BaseColor,
    mode: FilmMode,
    linked_color_limits: bool,
) -> Result<AutoColorLimits, String> {
    const SAMPLE_EDGE: u32 = 512;
    let (source_width, source_height) = proxy.dimensions();
    let longest = source_width.max(source_height).max(1);
    let sample_width = ((source_width as f64 / longest as f64) * SAMPLE_EDGE as f64)
        .round()
        .max(2.0) as u32;
    let sample_height = ((source_height as f64 / longest as f64) * SAMPLE_EDGE as f64)
        .round()
        .max(2.0) as u32;
    let points =
        geom.calibration_points
            .unwrap_or([[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]]);
    let min_x = points
        .iter()
        .map(|point| point[0])
        .fold(f32::INFINITY, f32::min);
    let max_x = points
        .iter()
        .map(|point| point[0])
        .fold(f32::NEG_INFINITY, f32::max);
    let min_y = points
        .iter()
        .map(|point| point[1])
        .fold(f32::INFINITY, f32::min);
    let max_y = points
        .iter()
        .map(|point| point[1])
        .fold(f32::NEG_INFINITY, f32::max);
    let homography = shader_homography(points);
    let use_linked_color_limits = linked_color_limits && mode == FilmMode::Color;
    let pipeline = FilmPipeline::new(
        [base_color.base_r, base_color.base_g, base_color.base_b],
        [0.0; 3],
        mode,
    );

    let collect = |inside_calibration_only: bool| {
        let mut histograms = [vec![0u32; 65536], vec![0u32; 65536], vec![0u32; 65536]];
        let mut linked_samples = Vec::new();
        let mut total = 0usize;
        for y in 0..sample_height {
            for x in 0..sample_width {
                let base_uv = [
                    x as f32 / (sample_width - 1) as f32,
                    y as f32 / (sample_height - 1) as f32,
                ];
                let crop_uv = [
                    geom.crop_rect.x + base_uv[0] * geom.crop_rect.width,
                    geom.crop_rect.y + base_uv[1] * geom.crop_rect.height,
                ];
                if inside_calibration_only
                    && (crop_uv[0] < min_x
                        || crop_uv[0] > max_x
                        || crop_uv[1] < min_y
                        || crop_uv[1] > max_y)
                {
                    continue;
                }
                let Some(perspective_uv) = apply_perspective_uv(
                    crop_uv,
                    geom.perspective_vertical,
                    geom.perspective_horizontal,
                    geom.perspective_aspect,
                    geom.perspective_scale,
                ) else {
                    continue;
                };
                let Some(oriented_uv) = apply_homography(&homography, perspective_uv) else {
                    continue;
                };
                let Some(oriented_uv) = apply_lens_distortion_uv(oriented_uv, geom.lens_distortion)
                else {
                    continue;
                };
                let source_uv =
                    map_oriented_uv_to_source(oriented_uv, source_width, source_height, geom);
                let Some(raw) = sample_rgb16_nearest(proxy, source_uv) else {
                    continue;
                };
                let density = pipeline.compute_true_density(&[
                    raw[0] as f32 / 65535.0,
                    raw[1] as f32 / 65535.0,
                    raw[2] as f32 / 65535.0,
                ]);
                for channel in 0..3 {
                    let bin = (((density[channel] + 1.0) / 4.0).clamp(0.0, 1.0) * 65535.0).round()
                        as usize;
                    histograms[channel][bin] += 1;
                }
                if use_linked_color_limits {
                    linked_samples.push(density);
                }
                total += 1;
            }
        }
        (histograms, linked_samples, total)
    };

    let (mut histograms, mut linked_samples, mut total) = collect(true);
    if total < 64 {
        (histograms, linked_samples, total) = collect(false);
    }
    if total < 64 {
        return Err("The selected film area contains too little image data.".to_string());
    }

    let mut d_min = [0.0; 3];
    let mut d_max = [0.0; 3];
    let linked_bounds = use_linked_color_limits
        .then(|| co_sited_density_extremes(linked_samples))
        .flatten();
    for channel in 0..3 {
        if let Some((low, high)) = linked_bounds {
            d_min[channel] = low[channel];
            d_max[channel] = high[channel];
        } else {
            let (low, high) = density_histogram_extremes(&histograms[channel], total);
            d_min[channel] = low as f32 / 65535.0 * 4.0 - 1.0;
            d_max[channel] = high as f32 / 65535.0 * 4.0 - 1.0;
        }
    }
    Ok(AutoColorLimits {
        d_min,
        d_max,
        pipeline_state: None,
    })
}

#[tauri::command]
pub async fn open_file_dialog() -> Result<Vec<String>, String> {
    let file_paths = tauri::async_runtime::spawn_blocking(|| {
        FileDialog::new()
            .add_filter(
                "Film Scans",
                &[
                    "dng", "nef", "nrw", "cr2", "cr3", "arw", "srf", "sr2", "raf", "rw2", "orf",
                    "ori", "srw", "pef", "3fr", "erf", "kdc", "dcr", "iiq", "mos", "mrw", "x3f",
                    "rwl", "fff", "raw", "tiff", "tif", "jpg", "jpeg", "png",
                ],
            )
            .pick_files()
    })
    .await
    .map_err(|e| format!("Dialog error: {:?}", e))?;

    if let Some(paths) = file_paths {
        Ok(paths
            .into_iter()
            .map(|p| p.to_string_lossy().to_string())
            .collect())
    } else {
        Ok(Vec::new())
    }
}

#[tauri::command]
pub async fn select_export_dir() -> Result<Option<String>, String> {
    let dir_path = tauri::async_runtime::spawn_blocking(|| FileDialog::new().pick_folder())
        .await
        .map_err(|e| format!("Dialog error: {:?}", e))?;

    Ok(dir_path.map(|p| p.to_string_lossy().to_string()))
}

#[tauri::command]
pub async fn open_lut_dialog() -> Result<Option<String>, String> {
    let file_path = tauri::async_runtime::spawn_blocking(|| {
        FileDialog::new()
            .add_filter("3D LUT / JSON Config", &["cube", "json", "3dl"])
            .pick_file()
    })
    .await
    .map_err(|e| format!("Dialog error: {:?}", e))?;

    Ok(file_path.map(|p| p.to_string_lossy().to_string()))
}

#[tauri::command]
pub async fn get_builtin_luts(app_handle: tauri::AppHandle) -> Result<Vec<String>, String> {
    let mut luts = Vec::new();
    if let Ok(entries) = std::fs::read_dir(bundled_asset_dir(&app_handle, "luts")) {
        for entry in entries.filter_map(Result::ok) {
            if let Ok(file_type) = entry.file_type() {
                if file_type.is_file() {
                    let path = entry.path();
                    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
                    if ext == "cube" || ext == "json" {
                        if let Some(path_str) = path.to_str() {
                            luts.push(path_str.to_string());
                        }
                    }
                }
            }
        }
    }
    Ok(luts)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DecodeMode {
    DevelopProxy,
    ExportFull,
}

fn preview_proxy_target_long_edge(requested: Option<u32>) -> u32 {
    requested
        .unwrap_or(PROXY_LONG_EDGE as u32)
        .clamp(PROXY_LONG_EDGE as u32, MAX_PREVIEW_PROXY_LONG_EDGE)
}

fn preview_proxy_decode_mode(target_long_edge: u32) -> DecodeMode {
    if target_long_edge > PROXY_LONG_EDGE as u32 {
        DecodeMode::ExportFull
    } else {
        DecodeMode::DevelopProxy
    }
}

fn convert_linear_image(
    image: ImageBuffer<Rgb<u16>, Vec<u16>>,
    source: ColorSpaceId,
    target: ColorSpaceId,
) -> ImageBuffer<Rgb<u16>, Vec<u16>> {
    if source == target {
        return image;
    }
    let matrix = linear_conversion_matrix(source, target);
    let mut converted = ImageBuffer::<Rgb<u16>, Vec<u16>>::new(image.width(), image.height());
    converted
        .as_mut()
        .par_chunks_exact_mut(3)
        .zip(image.as_raw().par_chunks_exact(3))
        .for_each(|(target_pixel, source_pixel)| {
            let rgb = [
                source_pixel[0] as f32 / 65535.0,
                source_pixel[1] as f32 / 65535.0,
                source_pixel[2] as f32 / 65535.0,
            ];
            let converted = apply_linear_matrix(rgb, matrix);
            for channel in 0..3 {
                target_pixel[channel] =
                    (converted[channel].clamp(0.0, 1.0) * 65535.0).round() as u16;
            }
        });
    converted
}

fn convert_linear_image_in_place(
    mut image: ImageBuffer<Rgb<u16>, Vec<u16>>,
    source: ColorSpaceId,
    target: ColorSpaceId,
) -> ImageBuffer<Rgb<u16>, Vec<u16>> {
    if source == target {
        return image;
    }
    let matrix = linear_conversion_matrix(source, target);
    image.as_mut().par_chunks_exact_mut(3).for_each(|pixel| {
        let converted = apply_linear_matrix(
            [
                pixel[0] as f32 / 65535.0,
                pixel[1] as f32 / 65535.0,
                pixel[2] as f32 / 65535.0,
            ],
            matrix,
        );
        for channel in 0..3 {
            pixel[channel] = (converted[channel].clamp(0.0, 1.0) * 65535.0).round() as u16;
        }
    });
    image
}

fn decode_reduced_tiff_for_working_space(
    path: &str,
    target_long_edge: u32,
) -> Result<ImageBuffer<Rgb<u16>, Vec<u16>>, String> {
    let requested_profile = DENSITY_CAPTURE_PROFILE;
    let mut image = decode_uncompressed_tiff_reduced(path, target_long_edge)?;
    if is_scanner_fff_tiff(path) {
        return Ok(linearize_scanner_fff(image, requested_profile));
    }
    if let Some(source_profile) = embedded_input_profile(path) {
        let matrix = linear_conversion_matrix(source_profile, requested_profile);
        image.as_mut().par_chunks_exact_mut(3).for_each(|pixel| {
            let linear = convert_encoded_to_linear_rgb_with_matrix(
                [
                    pixel[0] as f32 / 65535.0,
                    pixel[1] as f32 / 65535.0,
                    pixel[2] as f32 / 65535.0,
                ],
                source_profile,
                matrix,
            );
            for channel in 0..3 {
                pixel[channel] = (linear[channel].clamp(0.0, 1.0) * 65535.0).round() as u16;
            }
        });
        return Ok(image);
    }
    Ok(convert_linear_image_in_place(
        image,
        ColorSpaceId::SRgb,
        requested_profile,
    ))
}

fn decode_tiff_for_smart_auto(
    path: &str,
    target_long_edge: u32,
) -> Result<ImageBuffer<Rgb<u16>, Vec<u16>>, String> {
    if !is_scanner_fff_tiff(path) && embedded_input_profile(path).is_none() {
        return Err("scanner_tiff_input_space_unknown".to_string());
    }
    decode_reduced_tiff_for_working_space(path, target_long_edge)
}

fn decode_reduced_dng_for_working_space(
    path: &str,
    target_long_edge: u32,
) -> Result<ImageBuffer<Rgb<u16>, Vec<u16>>, String> {
    // VueScan LinearRaw DNG stores already-interpolated, linear RGB samples.
    // Read its uncompressed SubIFD directly so a 1.6 GB scan never has to be
    // unpacked into a second full-size LibRaw buffer merely to build a proxy.
    let image = decode_uncompressed_linear_dng_reduced(path, target_long_edge)?;
    Ok(convert_linear_image_in_place(
        image,
        ColorSpaceId::SRgb,
        DENSITY_CAPTURE_PROFILE,
    ))
}

fn rgb16_image_from_bytes(
    width: u32,
    height: u32,
    colors: usize,
    bits: u16,
    bytes: &[u8],
) -> Result<ImageBuffer<Rgb<u16>, Vec<u16>>, String> {
    if colors < 3 || bits != 16 {
        return Err(format!(
            "Unexpected LibRaw output: {colors} channels at {bits} bits"
        ));
    }

    let pixel_count = (width as usize)
        .checked_mul(height as usize)
        .ok_or_else(|| "LibRaw image dimensions overflowed".to_string())?;
    let required_bytes = pixel_count
        .checked_mul(colors)
        .and_then(|samples| samples.checked_mul(std::mem::size_of::<u16>()))
        .ok_or_else(|| "LibRaw image buffer size overflowed".to_string())?;
    if bytes.len() < required_bytes {
        return Err("LibRaw returned a truncated image buffer".to_string());
    }

    let mut image_buffer = ImageBuffer::<Rgb<u16>, Vec<u16>>::new(width, height);
    image_buffer
        .as_mut()
        .par_chunks_exact_mut(3)
        .enumerate()
        .for_each(|(index, pixel)| {
            let source = index * colors * std::mem::size_of::<u16>();
            for (channel, value) in pixel.iter_mut().enumerate() {
                let offset = source + channel * std::mem::size_of::<u16>();
                *value = u16::from_ne_bytes([bytes[offset], bytes[offset + 1]]);
            }
        });
    Ok(image_buffer)
}

fn rgb32_pixels_from_bytes(
    width: u32,
    height: u32,
    colors: usize,
    bits: u16,
    bytes: &[u8],
) -> Result<(u32, u32, Vec<f32>), String> {
    if colors < 3 || bits != 16 {
        return Err(format!(
            "Unexpected LibRaw output: {colors} channels at {bits} bits"
        ));
    }
    let pixel_count = (width as usize)
        .checked_mul(height as usize)
        .ok_or_else(|| "LibRaw image dimensions overflowed".to_string())?;
    let required_bytes = pixel_count
        .checked_mul(colors)
        .and_then(|samples| samples.checked_mul(std::mem::size_of::<u16>()))
        .ok_or_else(|| "LibRaw image buffer size overflowed".to_string())?;
    if bytes.len() < required_bytes {
        return Err("LibRaw returned a truncated image buffer".to_string());
    }
    let mut pixels = vec![0.0f32; pixel_count * 3];
    pixels
        .par_chunks_exact_mut(3)
        .enumerate()
        .for_each(|(index, pixel)| {
            let source = index * colors * std::mem::size_of::<u16>();
            for (channel, value) in pixel.iter_mut().enumerate() {
                let offset = source + channel * std::mem::size_of::<u16>();
                *value = u16::from_ne_bytes([bytes[offset], bytes[offset + 1]]) as f32 / 65535.0;
            }
        });
    Ok((width, height, pixels))
}

fn raw_decode_failure_hint(path: &str) -> Option<&'static str> {
    let extension = std::path::Path::new(path)
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    if extension == "nef" {
        Some(
            "If this is a Nikon Z 8/Z 9 HE/HE* NEF, LibRaw does not support that compression yet; use a standard/lossless NEF recording option or convert the original to TIFF/DNG before importing.",
        )
    } else {
        None
    }
}

fn libraw_decode_error_message(path: &str, error: impl std::fmt::Display) -> String {
    let mut message = format!(
        "LibRaw {} cannot decode {path}: {error}",
        crate::raw_backend::RawProcessor::version()
    );
    if let Some(hint) = raw_decode_failure_hint(path) {
        message.push_str(". ");
        message.push_str(hint);
    }
    message
}

fn decode_image_buffer(
    path: &str,
    mode: DecodeMode,
) -> Result<ImageBuffer<Rgb<u16>, Vec<u16>>, String> {
    let requested_profile = DENSITY_CAPTURE_PROFILE;
    if is_scanner_fff_tiff(path) {
        let image = decode_scanner_fff_tiff_page(path, 0)?;
        // Scanner FFF contains already-interpolated, gamma-encoded RGB. It is
        // deliberately kept out of LibRaw: no demosaic and no camera WB.
        return Ok(linearize_scanner_fff(image, requested_profile));
    }
    if is_direct_image_extension(path) {
        let image = image::open(path)
            .map(|image| image.into_rgb16())
            .map_err(|error| format!("Image decode failed for {path}: {error:?}"))?;
        let extension = std::path::Path::new(path)
            .extension()
            .and_then(|extension| extension.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        // JPEG/PNG samples are encoded sRGB by default. A recognized embedded
        // ICC profile takes precedence, including for TIFFs exported by this
        // application; unprofiled scanner TIFFs retain their linear contract.
        let embedded_profile = embedded_input_profile(path);
        let encoded_source = if extension == "jpg" || extension == "jpeg" || extension == "png" {
            Some(embedded_profile.unwrap_or(ColorSpaceId::SRgb))
        } else {
            embedded_profile
        };
        if let Some(source_profile) = encoded_source {
            let matrix = linear_conversion_matrix(source_profile, requested_profile);
            let mut converted =
                ImageBuffer::<Rgb<u16>, Vec<u16>>::new(image.width(), image.height());
            converted
                .as_mut()
                .par_chunks_exact_mut(3)
                .zip(image.as_raw().par_chunks_exact(3))
                .for_each(|(target, source)| {
                    let rgb = [
                        source[0] as f32 / 65535.0,
                        source[1] as f32 / 65535.0,
                        source[2] as f32 / 65535.0,
                    ];
                    let linear =
                        convert_encoded_to_linear_rgb_with_matrix(rgb, source_profile, matrix);
                    for channel in 0..3 {
                        target[channel] =
                            (linear[channel].clamp(0.0, 1.0) * 65535.0).round() as u16;
                    }
                });
            return Ok(converted);
        }
        return Ok(convert_linear_image(
            image,
            ColorSpaceId::SRgb,
            requested_profile,
        ));
    }

    // RAW_DECODE_VERSION 8 contract: camera FFF files, including tethered
    // Hasselblad digital-back captures, use LibRaw instead of the Flextight
    // scanner path. LibRaw performs black subtraction, camera white balance and
    // demosaic in camera RGB, but its signed output-gamut matrix is applied here
    // in f32. This avoids LibRaw's unsigned-16 CLIP after convert_to_rgb().
    let options = crate::raw_backend::DecodeOptions {
        half_size: mode == DecodeMode::DevelopProxy,
        demosaic_quality: 3,
        output_bps: 16,
        no_auto_bright: true,
        output_color: 0,
        linear_gamma: true,
        use_camera_wb: true,
    };
    let decoded = crate::raw_backend::extract_camera_rgb_with_options(path, &options)
        .map_err(|error| libraw_decode_error_message(path, error))?;

    let camera_rgb = rgb16_image_from_bytes(
        decoded.width as u32,
        decoded.height as u32,
        decoded.colors as usize,
        decoded.bits,
        &decoded.data,
    )?;
    debug_assert_eq!(requested_profile, ColorSpaceId::SRgb);
    let matrix = decoded.camera_to_srgb;
    let mut converted =
        ImageBuffer::<Rgb<u16>, Vec<u16>>::new(camera_rgb.width(), camera_rgb.height());
    converted
        .as_mut()
        .par_chunks_exact_mut(3)
        .zip(camera_rgb.as_raw().par_chunks_exact(3))
        .for_each(|(target, pixel)| {
            let rgb = compress_linear_srgb_for_density(apply_linear_matrix(
                [
                    pixel[0] as f32 / 65535.0,
                    pixel[1] as f32 / 65535.0,
                    pixel[2] as f32 / 65535.0,
                ],
                matrix,
            ));
            for channel in 0..3 {
                target[channel] = (rgb[channel] * 65535.0).round() as u16;
            }
        });
    Ok(converted)
}

/// Decode the Smart Auto ProPhoto Estimate without applying the legacy
/// display-gamut compression or quantizing the camera matrix result. This
/// output is a relative display estimate, never a measured Density Input RGB.
fn decode_prophoto_estimate_image_buffer(
    path: &str,
    mode: DecodeMode,
) -> Result<ImageBuffer<Rgb<f32>, Vec<f32>>, String> {
    if !is_raw_extension(path) {
        let source = decode_image_buffer(path, mode)?;
        let matrix = linear_conversion_matrix(ColorSpaceId::SRgb, ColorSpaceId::ProPhotoRgb);
        let mut converted = ImageBuffer::<Rgb<f32>, Vec<f32>>::new(source.width(), source.height());
        converted
            .as_mut()
            .par_chunks_exact_mut(3)
            .zip(source.as_raw().par_chunks_exact(3))
            .for_each(|(target, pixel)| {
                let rgb = apply_linear_matrix(
                    [
                        pixel[0] as f32 / 65535.0,
                        pixel[1] as f32 / 65535.0,
                        pixel[2] as f32 / 65535.0,
                    ],
                    matrix,
                );
                target.copy_from_slice(&rgb);
            });
        return Ok(converted);
    }

    let options = crate::raw_backend::DecodeOptions {
        half_size: mode == DecodeMode::DevelopProxy,
        demosaic_quality: 3,
        output_bps: 16,
        no_auto_bright: true,
        output_color: 0,
        linear_gamma: true,
        use_camera_wb: true,
    };
    let decoded = crate::raw_backend::decode_smart_auto_rgb(path, &options)
        .map_err(|error| libraw_decode_error_message(path, error))?;
    let srgb_to_prophoto = linear_conversion_matrix(ColorSpaceId::SRgb, ColorSpaceId::ProPhotoRgb);
    let mut converted =
        ImageBuffer::<Rgb<f32>, Vec<f32>>::from_raw(decoded.width, decoded.height, decoded.pixels)
            .ok_or_else(|| "Failed to allocate Smart Auto ProPhoto Estimate".to_string())?;
    eprintln!("[RAW Pipeline] {}", decoded.label);
    converted
        .as_mut()
        .par_chunks_exact_mut(3)
        .for_each(|pixel| {
            let camera = [pixel[0], pixel[1], pixel[2]];
            let srgb = apply_linear_matrix(camera, decoded.camera_to_srgb);
            pixel.copy_from_slice(&apply_linear_matrix(srgb, srgb_to_prophoto));
        });
    Ok(converted)
}

fn decode_scanner_profiled_estimate_image_buffer(
    path: &str,
    mode: DecodeMode,
    profile: Option<&crate::scanner_profile::ScannerInputProfile>,
    target_long_edge: u32,
) -> Result<ImageBuffer<Rgb<f32>, Vec<f32>>, String> {
    let Some(profile) = profile.filter(|_| !is_raw_extension(path) && !is_dng_extension(path))
    else {
        return decode_prophoto_estimate_image_buffer(path, mode);
    };
    let source = if is_tiff_extension(path) || is_scanner_fff_tiff(path) {
        decode_reduced_tiff_for_working_space(path, target_long_edge)
            .or_else(|_| decode_image_buffer(path, mode))?
    } else {
        decode_image_buffer(path, mode)?
    };
    let mut linear = ImageBuffer::<Rgb<f32>, Vec<f32>>::new(source.width(), source.height());
    linear
        .as_mut()
        .par_chunks_exact_mut(3)
        .zip(source.as_raw().par_chunks_exact(3))
        .for_each(|(target, pixel)| {
            target.copy_from_slice(&[
                pixel[0] as f32 / 65535.0,
                pixel[1] as f32 / 65535.0,
                pixel[2] as f32 / 65535.0,
            ]);
        });
    profile.apply_linear_rgb_image(&mut linear)?;
    let matrix = linear_conversion_matrix(ColorSpaceId::SRgb, ColorSpaceId::ProPhotoRgb);
    linear.as_mut().par_chunks_exact_mut(3).for_each(|pixel| {
        let rgb = apply_linear_matrix([pixel[0], pixel[1], pixel[2]], matrix);
        pixel.copy_from_slice(&rgb);
    });
    Ok(linear)
}

fn reference_density_extreme(
    image: &ImageBuffer<Rgb<f32>, Vec<f32>>,
    source: DensityAnchorSource,
) -> Result<[f32; 3], String> {
    const MAX_REFERENCE_SAMPLES: usize = 1_000_000;
    let pixel_count = image.as_raw().len() / 3;
    if pixel_count == 0 {
        return Err("The reference image contains no pixels.".to_string());
    }
    let stride = (pixel_count / MAX_REFERENCE_SAMPLES).max(1);
    let sample_capacity = pixel_count.div_ceil(stride);
    let mut densities = [
        Vec::with_capacity(sample_capacity),
        Vec::with_capacity(sample_capacity),
        Vec::with_capacity(sample_capacity),
    ];
    for pixel in image.as_raw().chunks_exact(3).step_by(stride) {
        for channel in 0..3 {
            let transmission = pixel[channel];
            if transmission.is_finite() {
                densities[channel].push(-transmission.max(1e-6).log10());
            }
        }
    }
    let mut result = [0.0; 3];
    for channel in 0..3 {
        if densities[channel].is_empty() {
            return Err(format!(
                "The reference image has no finite samples in channel {channel}."
            ));
        }
        densities[channel].sort_unstable_by(|left, right| left.total_cmp(right));
        let index = match source {
            // Clear film base is the high-transmission / low-density tail.
            DensityAnchorSource::SampledFilmBase => {
                ((densities[channel].len() as f32 * 0.01).ceil() as usize)
                    .saturating_sub(1)
                    .min(densities[channel].len() - 1)
            }
            // Full exposure is the low-transmission / high-density tail.
            DensityAnchorSource::SampledFullExposure => ((densities[channel].len() as f32 * 0.99)
                .ceil() as usize)
                .saturating_sub(1)
                .min(densities[channel].len() - 1),
            _ => densities[channel].len() / 2,
        };
        result[channel] = densities[channel][index];
    }
    Ok(result)
}

fn sampled_roll_anchor(path: &str, source: DensityAnchorSource) -> Result<DensityAnchor, String> {
    let image = decode_prophoto_estimate_image_buffer(path, DecodeMode::DevelopProxy)?;
    Ok(DensityAnchor {
        density: reference_density_extreme(&image, source)?,
        source,
        scope: DensityAnchorScope::Roll,
        confidence: DensityAnchorConfidence::UserSampled,
        reference_id: Some(path.to_string()),
        provenance: crate::app_state::DensityAnchorProvenance {
            input_domain: crate::app_state::DataDomain::ProPhotoEstimate,
            raw_decode_version: Some(RAW_DECODE_VERSION),
            algorithm_version: crate::app_state::DENSITY_ANCHOR_ALGORITHM_VERSION.to_string(),
            legacy: false,
            ..Default::default()
        },
    })
}

#[tauri::command]
pub async fn analyze_roll_density_references(
    base_path: Option<String>,
    full_exposure_path: Option<String>,
) -> Result<crate::app_state::DensityAnchors, String> {
    if base_path.is_none() && full_exposure_path.is_none() {
        return Ok(Default::default());
    }
    let base_path = base_path.ok_or_else(|| {
        "A film-base reference is required before adding a full-exposure reference.".to_string()
    })?;
    tokio::task::spawn_blocking(move || {
        let base = sampled_roll_anchor(&base_path, DensityAnchorSource::SampledFilmBase)?;
        let full_exposure = full_exposure_path
            .as_deref()
            .map(|path| sampled_roll_anchor(path, DensityAnchorSource::SampledFullExposure))
            .transpose()?;
        if let Some(full) = full_exposure.as_ref() {
            if (0..3).any(|channel| full.density[channel] <= base.density[channel] + 1e-4) {
                return Err(
                    "The full-exposure reference must be denser than the film-base reference in every channel."
                        .to_string(),
                );
            }
        }
        Ok(crate::app_state::DensityAnchors {
            d_min_base: Some(base),
            d_max_full_exposure: full_exposure,
            retained_records: Vec::new(),
        })
    })
    .await
    .map_err(|error| format!("Reference analysis worker failed: {error}"))?
}

fn averaged_reference_density(
    image: &ImageBuffer<Rgb<f32>, Vec<f32>>,
    quality: Option<&crate::raw_backend::QualityMask>,
    normalized_x: f32,
    normalized_y: f32,
) -> Result<[f32; 3], String> {
    if !normalized_x.is_finite() || !normalized_y.is_finite() {
        return Err("The sample position is invalid.".to_string());
    }
    let (width, height) = image.dimensions();
    if width == 0 || height == 0 {
        return Err("The reference image contains no pixels.".to_string());
    }
    let center_x = (normalized_x.clamp(0.0, 1.0) * width.saturating_sub(1) as f32).round() as i32;
    let center_y = (normalized_y.clamp(0.0, 1.0) * height.saturating_sub(1) as f32).round() as i32;
    // A roughly 3% window contains enough pixels to average film grain while
    // remaining local on both thumbnails and full-resolution scans.
    let radius = ((width.min(height) as f32 * 0.015).round() as i32).clamp(6, 48);
    let mut channels = [Vec::new(), Vec::new(), Vec::new()];
    for y in (center_y - radius).max(0)..=(center_y + radius).min(height as i32 - 1) {
        for x in (center_x - radius).max(0)..=(center_x + radius).min(width as i32 - 1) {
            let pixel_index = y as usize * width as usize + x as usize;
            if quality
                .and_then(|mask| mask.valid.get(pixel_index))
                .is_some_and(|valid| !*valid)
            {
                continue;
            }
            let pixel = image.get_pixel(x as u32, y as u32).0;
            for channel in 0..3 {
                if pixel[channel].is_finite() && pixel[channel] > 0.0 {
                    channels[channel].push(-pixel[channel].max(1e-6).log10());
                }
            }
        }
    }
    let mut result = [0.0; 3];
    for channel in 0..3 {
        if channels[channel].len() < 16 {
            return Err("The selected area contains too few usable pixels.".to_string());
        }
        channels[channel].sort_unstable_by(|left, right| left.total_cmp(right));
        let trim = (channels[channel].len() / 10).max(1);
        let kept = &channels[channel][trim..channels[channel].len() - trim];
        result[channel] = kept.iter().sum::<f32>() / kept.len() as f32;
    }
    Ok(result)
}

#[tauri::command]
pub async fn sample_roll_density_reference(
    id: String,
    kind: String,
    x: f32,
    y: f32,
    state: State<'_, EngineState>,
) -> Result<DensityAnchor, String> {
    if !x.is_finite() || !y.is_finite() || !(0.0..=1.0).contains(&x) || !(0.0..=1.0).contains(&y) {
        return Err("Density sample coordinates must be finite and within the image.".into());
    }
    let (path, roll_id, input, provenance) = {
        let item = state.items.get(&id).ok_or("Image ID not found")?;
        let item = read_lock(item.value());
        let effective = item.effective_pipeline_state();
        if effective.contract == ProcessingContract::CaptureCorrectedV11 {
            let image = item
                .relative_transmission_proxy
                .clone()
                .ok_or_else(|| "Capture Corrected proxy is not prepared.".to_string())?;
            let provenance = item
                .runtime_density_provenance
                .clone()
                .ok_or_else(|| "Capture Corrected provenance is unavailable.".to_string())?;
            (
                item.file_path.clone(),
                item.roll_id.clone(),
                (image, item.relative_transmission_quality.clone()),
                provenance,
            )
        } else {
            let image = item.prophoto_estimate_proxy.clone().unwrap_or_default();
            (
                item.file_path.clone(),
                item.roll_id.clone(),
                (image, None),
                crate::app_state::DensityAnchorProvenance {
                    input_domain: crate::app_state::DataDomain::ProPhotoEstimate,
                    raw_decode_version: Some(RAW_DECODE_VERSION),
                    algorithm_version: crate::app_state::DENSITY_ANCHOR_ALGORITHM_VERSION
                        .to_string(),
                    legacy: false,
                    ..Default::default()
                },
            )
        }
    };
    let source = match kind.as_str() {
        "base" => DensityAnchorSource::SampledFilmBase,
        "full" => DensityAnchorSource::SampledFullExposure,
        _ => return Err("Unknown density reference kind.".to_string()),
    };
    tokio::task::spawn_blocking(move || {
        let (mut image, quality) = input;
        if image.width() == 0 || image.height() == 0 {
            image = decode_prophoto_estimate_image_buffer(&path, DecodeMode::DevelopProxy)?;
        }
        let density = averaged_reference_density(&image, quality.as_ref(), x, y)?;
        if density.iter().any(|value| !value.is_finite()) {
            return Err("Density sample produced a non-finite value.".into());
        }
        Ok(DensityAnchor {
            density,
            source,
            scope: DensityAnchorScope::Roll,
            confidence: DensityAnchorConfidence::UserSampled,
            reference_id: Some(format!("{roll_id}:{path}")),
            provenance,
        })
    })
    .await
    .map_err(|error| format!("Density sampling worker failed: {error}"))?
}

fn decode_export_source(path: &str) -> Result<ImageBuffer<Rgb<u16>, Vec<u16>>, String> {
    // Use the same direct source decoder as Develop for large scanner files.
    // This keeps preview/export colors aligned and avoids LibRaw's additional
    // full-resolution allocation for LinearRaw DNG.
    if is_dng_extension(path) {
        return decode_reduced_dng_for_working_space(path, u32::MAX)
            .or_else(|_| decode_image_buffer(path, DecodeMode::ExportFull));
    }
    if is_tiff_extension(path) || is_scanner_fff_tiff(path) {
        return decode_reduced_tiff_for_working_space(path, u32::MAX)
            .or_else(|_| decode_image_buffer(path, DecodeMode::ExportFull));
    }
    decode_image_buffer(path, DecodeMode::ExportFull)
}

fn persist_import_batch(
    connection: &mut rusqlite::Connection,
    items: &[FilmItem],
) -> Result<(), String> {
    let transaction = connection
        .transaction()
        .map_err(|error| format!("Failed to begin import transaction: {error}"))?;
    for item in items {
        let params_str = serde_json::to_string(&item.params)
            .map_err(|error| format!("Failed to serialize tuning state: {error}"))?;
        let geom_str = serde_json::to_string(&item.geom)
            .map_err(|error| format!("Failed to serialize geometry state: {error}"))?;
        let base_color_str = serde_json::to_string(&item.base_color)
            .map_err(|error| format!("Failed to serialize base color: {error}"))?;
        let pipeline_state_str = serde_json::to_string(&item.pipeline_state)
            .map_err(|error| format!("Failed to serialize pipeline state: {error}"))?;
        transaction
            .execute(
                "INSERT INTO image_states (
                     roll_id, file_path, thumbnail_base64, embedded_thumb_base64,
                     rendered_thumb_base64, params, geom, base_color, pipeline_state,
                     math_version, raw_decode_version, updated_at
                 )
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)
                 ON CONFLICT(roll_id, file_path) DO UPDATE SET
                 thumbnail_base64=excluded.thumbnail_base64,
                 embedded_thumb_base64=excluded.embedded_thumb_base64,
                 rendered_thumb_base64=COALESCE(excluded.rendered_thumb_base64, image_states.rendered_thumb_base64),
                 params=excluded.params,
                 geom=excluded.geom,
                 base_color=excluded.base_color,
                 pipeline_state=excluded.pipeline_state,
                 math_version=excluded.math_version,
                 raw_decode_version=excluded.raw_decode_version,
                 updated_at=excluded.updated_at",
                rusqlite::params![
                    item.roll_id,
                    item.file_path,
                    item.preferred_thumbnail(),
                    item.embedded_thumbnail_base64,
                    item.rendered_thumbnail_base64,
                    params_str,
                    geom_str,
                    base_color_str,
                    pipeline_state_str,
                    persistence::math_version_for_contract(item.pipeline_state.contract),
                    RAW_DECODE_VERSION,
                    persistence::now_timestamp(),
                ],
            )
            .map_err(|error| {
                format!("Failed to persist imported image {}: {error}", item.file_path)
            })?;
    }
    transaction
        .commit()
        .map_err(|error| format!("Failed to commit imported images: {error}"))
}

fn default_pipeline_state_for_import(
    loose: bool,
    target_roll: &str,
    rolls: &[Roll],
) -> PipelineState {
    default_pipeline_state_for_import_with_profiles(loose, target_roll, rolls, &[])
}

fn pipeline_image_kind(path: &str) -> PipelineImageKind {
    if !Path::new(path).is_file() {
        PipelineImageKind::Missing
    } else if is_raw_extension(path) {
        PipelineImageKind::RawBayer
    } else if is_direct_image_extension(path) || is_tiff_extension(path) {
        PipelineImageKind::DirectRgb
    } else {
        PipelineImageKind::Unsupported
    }
}

fn resolver_profile(view: &CalibrationProfileView) -> ResolverProfile {
    let has_reference = |kind| {
        view.profile
            .references
            .iter()
            .any(|reference| reference.kind == kind)
    };
    ResolverProfile {
        profile_id: view.profile.profile_id.clone(),
        payload_digest: view.profile.payload.payload_digest.clone(),
        available: view.availability == CalibrationProfileAvailability::Available,
        capture_validation_error: view
            .profile
            .payload
            .capture_validation_error(RAW_DECODE_VERSION)
            .map(str::to_string),
        fit_validation_error: view
            .profile
            .payload
            .fit_validation_error()
            .map(str::to_string),
        capture_separation_fitted: view.profile.payload.fit_model.is_some()
            && view.profile.payload.fit_validation_error().is_none(),
        has_dark: has_reference(CalibrationReferenceKind::DarkFrame),
        has_open_gate: has_reference(CalibrationReferenceKind::OpenGate),
        has_flat: view
            .profile
            .references
            .iter()
            .find(|reference| reference.kind == CalibrationReferenceKind::FlatField)
            .is_some_and(|reference| verified_reference_error(&view.profile, reference).is_none()),
    }
}

fn resolve_image_pipeline(
    persisted: &PipelineState,
    roll: Option<&Roll>,
    profiles: &[CalibrationProfileView],
    path: &str,
    runtime_failure: Option<String>,
) -> PipelineResolution {
    resolve_pipeline(&pipeline_resolver_input(
        persisted,
        roll,
        profiles,
        path,
        runtime_failure,
    ))
}

fn pipeline_resolver_input(
    persisted: &PipelineState,
    roll: Option<&Roll>,
    profiles: &[CalibrationProfileView],
    path: &str,
    runtime_failure: Option<String>,
) -> PipelineResolverInput {
    pipeline_resolver_input_for_kind(
        persisted,
        roll,
        profiles,
        pipeline_image_kind(path),
        runtime_failure,
    )
}

fn pipeline_resolver_input_for_kind(
    persisted: &PipelineState,
    roll: Option<&Roll>,
    profiles: &[CalibrationProfileView],
    image_kind: PipelineImageKind,
    runtime_failure: Option<String>,
) -> PipelineResolverInput {
    let roll_profile_id = roll.and_then(|roll| roll.calibration_profile_id.clone());
    let profile = roll_profile_id.as_deref().and_then(|profile_id| {
        profiles
            .iter()
            .find(|view| view.profile.profile_id == profile_id)
            .map(resolver_profile)
    });
    let mut anchors = persisted.density_anchors.clone();
    if let Some(roll) = roll {
        if roll.density_anchors.d_min_base.is_some() {
            anchors.d_min_base = roll.density_anchors.d_min_base.clone();
        }
        if roll.density_anchors.d_max_full_exposure.is_some() {
            anchors.d_max_full_exposure = roll.density_anchors.d_max_full_exposure.clone();
        }
    }
    PipelineResolverInput {
        persisted_contract: persisted.contract,
        roll_profile_id,
        profile,
        image_kind,
        density_anchors: anchors,
        raw_decode_version: RAW_DECODE_VERSION,
        runtime_failure,
    }
}

fn state_from_resolution(
    persisted: &PipelineState,
    resolution: &PipelineResolution,
) -> PipelineState {
    let mut state = persisted.clone();
    state.contract = resolution.resolved_path;
    state.density_anchors = resolution.usable_density_anchors.clone();
    let mut report = resolution.processing_report.clone();
    let expected_domain = match resolution.resolved_path {
        ProcessingContract::CaptureCorrectedV11 => "relative_transmission_rgb",
        ProcessingContract::LegacyV1 => "legacy_linear_srgb",
        _ => "linear_prophoto_estimate",
    };
    if report.base_source == "unresolved"
        && persisted.processing_report.analysis_data_domain == expected_domain
        && persisted.processing_report.base_source != "unresolved"
    {
        // Resolver stages describe capability; the persisted report carries
        // the completed frame analysis. Keep both pieces of provenance.
        report.base_source = persisted.processing_report.base_source.clone();
        report.base_confidence = persisted.processing_report.base_confidence.clone();
        report.excluded_open_light_pixels = persisted.processing_report.excluded_open_light_pixels;
        report.excluded_saturated_pixels = persisted.processing_report.excluded_saturated_pixels;
        report.excluded_invalid_pixels = persisted.processing_report.excluded_invalid_pixels;
        report.tone_mapping_mode = persisted.processing_report.tone_mapping_mode.clone();
        report.uses_physical_anchors = persisted.processing_report.uses_physical_anchors;
        report.analysis_data_domain = persisted.processing_report.analysis_data_domain.clone();
        report
            .fallback_reasons
            .extend(persisted.processing_report.fallback_reasons.iter().cloned());
    }
    state.processing_report = report;
    state
}

fn resolution_key(resolution: &PipelineResolution) -> String {
    serde_json::to_string(resolution).unwrap_or_else(|_| {
        format!(
            "{:?}|{:?}|{:?}",
            resolution.requested_path, resolution.resolved_path, resolution.resolved_profile_id
        )
    })
}

fn resolution_density_provenance(
    resolution: &PipelineResolution,
) -> crate::app_state::DensityAnchorProvenance {
    crate::app_state::DensityAnchorProvenance {
        input_domain: resolution.resolved_path.input_domain(),
        calibration_profile_id: resolution.resolved_profile_id.clone(),
        calibration_payload_digest: resolution.resolved_payload_digest.clone(),
        raw_decode_version: Some(RAW_DECODE_VERSION),
        algorithm_version: crate::app_state::DENSITY_ANCHOR_ALGORITHM_VERSION.to_string(),
        legacy: false,
    }
}

fn pipeline_state_for_roll_profile(
    roll: &Roll,
    _profiles: &[CalibrationProfileView],
) -> PipelineState {
    // Persist the requested path. Runtime capability resolution may fall back
    // for one invocation without rewriting this request or the Roll binding.
    if roll.calibration_profile_id.is_some() {
        PipelineState::capture_corrected(roll.density_anchors.clone(), false)
    } else if roll.scanner_profile_id.is_some() {
        PipelineState::smart_auto()
    } else {
        PipelineState::from_roll_anchors(roll.density_anchors.clone())
    }
}

fn default_pipeline_state_for_import_with_profiles(
    loose: bool,
    target_roll: &str,
    rolls: &[Roll],
    profiles: &[CalibrationProfileView],
) -> PipelineState {
    if loose {
        // Loose Import has no capture, film-stock, or roll-reference metadata.
        // The lowest v1.1 contract guarantees output without reintroducing Status M.
        return PipelineState::smart_auto();
    }
    rolls
        .iter()
        .find(|roll| roll.roll_id == target_roll)
        .map(|roll| pipeline_state_for_roll_profile(roll, profiles))
        .unwrap_or_else(PipelineState::smart_auto)
}

#[tauri::command]
pub async fn import_images(
    paths: Vec<String>,
    is_loose: Option<bool>,
    in_library: Option<bool>,
    roll_id: Option<String>,
    is_historical: Option<bool>,
    replace_library: Option<bool>,
    state: State<'_, EngineState>,
    app_handle: tauri::AppHandle,
) -> Result<(), String> {
    if paths.is_empty() {
        return Ok(());
    }

    let target_roll = roll_id
        .clone()
        .unwrap_or_else(|| "LOOSE_DEFAULT".to_string());
    let loose = is_loose.unwrap_or(false);
    let in_lib = in_library.unwrap_or(true);
    let calibration_profiles = load_calibration_profile_views().unwrap_or_default();
    let default_pipeline_state = {
        let rolls = read_lock(&state.rolls);
        default_pipeline_state_for_import_with_profiles(
            loose,
            &target_roll,
            &rolls,
            &calibration_profiles,
        )
    };
    let historical = is_historical.unwrap_or(false);
    if historical {
        return Err(
            "Historical rolls must be resumed from persisted state, not re-imported".into(),
        );
    }

    if replace_library.unwrap_or(true) {
        clear_library_membership(&state)?;
    }

    // ═══════════════════════════════════════════════════════════════════
    //  STEP 1: Create the MPSC channel — the SINGLE data pipe.
    // ═══════════════════════════════════════════════════════════════════
    enum ImportWork {
        Item(FilmItem),
        Failed { file_path: String, message: String },
    }
    let (tx, rx) = std::sync::mpsc::channel::<ImportWork>();

    // ═══════════════════════════════════════════════════════════════════
    //  STEP 2: Spawn the consumer thread — ABSOLUTE single-writer to SQLite.
    //  Adaptive Flush: first 5 items flush immediately (instant UI feedback),
    //  then batch every 15 items for SQLite throughput.
    // ═══════════════════════════════════════════════════════════════════
    let app_handle_consumer = app_handle.clone();
    let paths_consumer = paths.clone();
    let roll_id_consumer = roll_id.clone();
    let import_total = Arc::new(AtomicUsize::new(paths_consumer.len()));
    let import_total_consumer = import_total.clone();

    std::thread::spawn(move || {
        let mut conn = match persistence::open_connection() {
            Ok(c) => {
                c.busy_timeout(std::time::Duration::from_secs(5)).ok();
                c
            }
            Err(e) => {
                let message = format!("Failed to open import database: {e}");
                eprintln!("[Import Consumer] {message}");
                let _ = app_handle_consumer.emit(
                    "import_error",
                    serde_json::json!({
                        "message": message,
                        "file_paths": paths_consumer,
                        "roll_id": roll_id_consumer,
                        "processed": 0,
                        "total": import_total_consumer.load(Ordering::SeqCst),
                    }),
                );
                let _ = app_handle_consumer.emit(
                    "import_complete",
                    serde_json::json!({
                        "total": 0,
                        "failed": import_total_consumer.load(Ordering::SeqCst),
                    }),
                );
                return;
            }
        };

        let state = app_handle_consumer.state::<EngineState>();
        let mut buffer: Vec<FilmItem> = Vec::new();
        let mut total_processed: usize = 0;
        let mut total_failed: usize = 0;
        let mut failed_roll_paths = HashSet::new();
        let total_for_progress = import_total_consumer.clone();

        // ── Helper: flush a batch to SQLite + emit events ──
        let flush_batch = |batch: &mut Vec<FilmItem>,
                           conn: &mut rusqlite::Connection,
                           state: &EngineState,
                           app: &tauri::AppHandle,
                           processed: &mut usize,
                           failed: &mut usize,
                           failed_paths: &mut HashSet<String>,
                           total_for_progress: &Arc<AtomicUsize>| {
            if batch.is_empty() {
                return;
            }
            let items = std::mem::take(batch);
            let persistence_result = persist_import_batch(conn, &items);

            if let Err(message) = persistence_result {
                eprintln!("[Import Consumer] {message}");
                let failed_batch_paths: Vec<&str> =
                    items.iter().map(|item| item.file_path.as_str()).collect();
                for path in &failed_batch_paths {
                    failed_paths.insert(normalize_path(path));
                }
                *processed += items.len();
                *failed += items.len();
                let _ = app.emit(
                    "import_error",
                    serde_json::json!({
                        "message": message,
                        "file_paths": failed_batch_paths,
                        "roll_id": items.first().map(|item| item.roll_id.as_str()),
                        "processed": *processed,
                        "total": total_for_progress.load(Ordering::SeqCst),
                    }),
                );
                return;
            }

            // Emit events AFTER commit so frontend sees consistent state
            for item in items {
                let payload = serde_json::json!({
                    "id": item.id.clone(),
                    "roll_id": item.roll_id.clone(),
                    "thumbnail_base64": item.preferred_thumbnail(),
                    "embedded_thumbnail_base64": item.embedded_thumbnail_base64.clone(),
                    "rendered_thumbnail_base64": item.rendered_thumbnail_base64.clone(),
                    "thumbnail_kind": item.thumbnail_kind(),
                    "file_path": item.file_path.clone(),
                    "processed": *processed + 1,
                    "total": total_for_progress.load(Ordering::SeqCst),
                });
                state
                    .items
                    .insert(item.id.clone(), Arc::new(RwLock::new(item)));
                let _ = app.emit("import_progress", payload);
                *processed += 1;
            }
        };

        // ── Micro-batch recv loop: flush every 3 items for smooth UI ──
        // A roll of film typically has 3-6 frames per strip; flushing every 3
        // ensures the frontend grid updates like water flowing, eliminating 0% deadlock.
        while let Ok(work) = rx.recv() {
            match work {
                ImportWork::Item(item) => {
                    buffer.push(item);
                    // Commit each completed preview immediately so a slow file
                    // never delays successful neighbors.
                    flush_batch(
                        &mut buffer,
                        &mut conn,
                        &state,
                        &app_handle_consumer,
                        &mut total_processed,
                        &mut total_failed,
                        &mut failed_roll_paths,
                        &total_for_progress,
                    );
                }
                ImportWork::Failed { file_path, message } => {
                    failed_roll_paths.insert(normalize_path(&file_path));
                    total_processed += 1;
                    total_failed += 1;
                    let _ = app_handle_consumer.emit(
                        "import_error",
                        serde_json::json!({
                            "message": message,
                            "file_paths": [file_path],
                            "roll_id": roll_id_consumer,
                            "processed": total_processed,
                            "total": total_for_progress.load(Ordering::SeqCst),
                        }),
                    );
                }
            }
        }

        // ── Flush remaining items after channel closes ──
        flush_batch(
            &mut buffer,
            &mut conn,
            &state,
            &app_handle_consumer,
            &mut total_processed,
            &mut total_failed,
            &mut failed_roll_paths,
            &total_for_progress,
        );

        if let Some(roll_id) = roll_id_consumer.as_deref() {
            if !failed_roll_paths.is_empty() {
                let cleanup = (|| -> Result<Option<Vec<Roll>>, String> {
                    let _mutation = state.roll_mutation.blocking_lock();
                    let mut updated = read_lock(&state.rolls).clone();
                    if !remove_failed_roll_paths(&mut updated, roll_id, &failed_roll_paths) {
                        return Ok(None);
                    }
                    persist_roll_snapshot(&updated)?;
                    *write_lock(&state.rolls) = updated.clone();
                    Ok(Some(updated))
                })();
                match cleanup {
                    Ok(Some(updated)) => update_rolls_compatibility_mirror(&updated),
                    Ok(None) => {}
                    Err(error) => {
                        eprintln!("[Import Consumer] Failed to reconcile roll metadata: {error}");
                        let _ = app_handle_consumer.emit(
                            "import_error",
                            serde_json::json!({
                                "message": format!("Failed to reconcile roll metadata: {error}"),
                                "file_paths": [],
                                "roll_id": roll_id,
                                "processed": total_processed,
                                "total": total_for_progress.load(Ordering::SeqCst),
                            }),
                        );
                    }
                }
            }
        }

        // ── Update item_order for filmstrip ordering ──
        {
            if let Ok(mut order_guard) = state.item_order.write() {
                for path in paths_consumer {
                    let id_opt = {
                        let guard = state.items.clone();
                        let mut found = None;
                        let target = roll_id_consumer
                            .clone()
                            .unwrap_or_else(|| "LOOSE_DEFAULT".to_string());
                        for kv in guard.iter() {
                            let item = read_lock(kv.value());
                            let db_path = item.file_path.clone();
                            if item.roll_id == target
                                && (db_path == path
                                    || db_path.replace("\\", "/").to_lowercase()
                                        == path.replace("\\", "/").to_lowercase())
                            {
                                found = Some(kv.key().clone());
                                break;
                            }
                        }
                        found
                    };
                    if let Some(id) = id_opt {
                        if !order_guard.contains(&id) {
                            order_guard.push(id);
                        }
                    }
                }
            }
        }

        // ── Emit completion event ──
        let _ = app_handle_consumer.emit(
            "import_complete",
            serde_json::json!({ "total": total_processed, "failed": total_failed }),
        );
    });

    // ═══════════════════════════════════════════════════════════════════
    //  STEP 3: Spawn the producer thread.
    //  Dedup + DB cache + bounded Rayon pool (4 threads) for thumbnail extraction.
    //  ALL heavy I/O and computation is isolated here — main thread returns in µs.
    // ═══════════════════════════════════════════════════════════════════
    let app_handle_producer = app_handle.clone();
    let target_roll_producer = target_roll.clone();

    std::thread::spawn(move || {
        // ── Phase 1: Fast dedup — in-memory HashSet against DashMap (lock-free reads) ──
        let state = app_handle_producer.state::<EngineState>();
        let existing_items_by_path: std::collections::HashMap<String, String> = {
            let guard = state.items.clone();
            guard
                .iter()
                .filter_map(|kv| {
                    let item = read_lock(kv.value());
                    if item.roll_id == target_roll_producer {
                        Some((
                            item.file_path.replace("\\", "/").to_lowercase(),
                            kv.key().clone(),
                        ))
                    } else {
                        None
                    }
                })
                .collect()
        };

        let selected_paths = paths;
        let paths_to_process: Vec<String> = selected_paths
            .into_iter()
            .filter(|p| !existing_items_by_path.contains_key(&p.replace("\\", "/").to_lowercase()))
            .collect();

        let total = paths_to_process.len();
        import_total.store(total, Ordering::SeqCst);

        // ── Emit initial progress so frontend knows import started ──
        let _ = app_handle_producer.emit(
            "import_progress",
            serde_json::json!({
                "phase": "start",
                "total": total,
            }),
        );

        if total == 0 {
            // tx drops when this closure returns → consumer recv() returns Err →
            // consumer emits import_complete with total_processed=0
            return;
        }

        // ── Phase 2: Build DB cache for instant re-import of already-processed images ──
        let db_cache: std::collections::HashMap<
            String,
            (
                String,
                Option<String>,
                TuningParams,
                crate::app_state::GeometryState,
                BaseColor,
                PipelineState,
            ),
        > = {
            let mut cache = std::collections::HashMap::new();
            if let Ok(conn) = persistence::open_connection() {
                conn.busy_timeout(std::time::Duration::from_secs(5)).ok();
                if let Ok(mut stmt) = conn.prepare(
                    "SELECT file_path,
                            COALESCE(embedded_thumb_base64, thumbnail_base64),
                            rendered_thumb_base64,
                            params, geom, base_color, pipeline_state
                     FROM image_states WHERE roll_id = ?1",
                ) {
                    if let Ok(rows) =
                        stmt.query_map(rusqlite::params![&target_roll_producer], |row| {
                            Ok((
                                row.get::<_, String>(0)?,
                                row.get::<_, String>(1)?,
                                row.get::<_, Option<String>>(2)?,
                                row.get::<_, String>(3)?,
                                row.get::<_, String>(4)?,
                                row.get::<_, String>(5)?,
                                row.get::<_, String>(6)?,
                            ))
                        })
                    {
                        for row in rows.flatten() {
                            let (
                                fp,
                                embedded_thumb,
                                rendered_thumb,
                                params_str,
                                geom_str,
                                bc_str,
                                pipeline_state_str,
                            ) = row;
                            if let (Ok(params), Ok(geom), Ok(bc), Ok(pipeline_state)) = (
                                serde_json::from_str(&params_str),
                                serde_json::from_str(&geom_str),
                                serde_json::from_str(&bc_str),
                                serde_json::from_str(&pipeline_state_str),
                            ) {
                                cache.insert(
                                    fp.replace("\\", "/").to_lowercase(),
                                    (
                                        embedded_thumb,
                                        rendered_thumb,
                                        params,
                                        geom,
                                        bc,
                                        pipeline_state,
                                    ),
                                );
                            }
                        }
                    }
                }
            }
            cache
        };

        // ── Helper: process a single path into a FilmItem ──
        // ONLY uses libraw_unpack_thumb (lazy demosaicing) — never full unpack.
        // All captures are immutable references → safe for parallel invocation.
        let process_path: Arc<dyn Fn(&String) -> Result<FilmItem, String> + Send + Sync> =
            Arc::new(move |path: &String| -> Result<FilmItem, String> {
                // ── Fast path: hit the DB cache (no libraw decoding needed) ──
                if let Some((
                    embedded_thumb,
                    rendered_thumb,
                    params,
                    geom,
                    base_color,
                    pipeline_state,
                )) = db_cache.get(&path.replace("\\", "/").to_lowercase())
                {
                    let id = format!("img_{}", NEXT_ID.fetch_add(1, Ordering::SeqCst));
                    std::fs::File::open(path)
                        .map_err(|error| format!("Cannot read {path}: {error}"))?;
                    return Ok(FilmItem {
                        id,
                        roll_id: target_roll_producer.clone(),
                        file_path: path.clone(),
                        embedded_thumbnail_base64: embedded_thumb.clone(),
                        rendered_thumbnail_base64: rendered_thumb.clone(),
                        original_proxy: None,
                        proxy_image: None,
                        prophoto_estimate_proxy: None,
                        relative_transmission_proxy: None,
                        relative_transmission_quality: None,
                        pristine_proxy: None,
                        base_color: base_color.clone(),
                        runtime_pipeline_state: None,
                        runtime_density_provenance: None,
                        runtime_pipeline_key: None,
                        pipeline_state: pipeline_state.clone(),
                        params: params.clone(),
                        geom: normalize_persisted_geometry_for_rendered_image(
                            geom.clone(),
                            rendered_thumb
                                .as_deref()
                                .is_some_and(|thumbnail| !thumbnail.is_empty()),
                        ),
                        is_loose: loose,
                        in_library: in_lib,
                    });
                }

                let embedded_thumbnail_base64 =
                    decode_import_preview_result(path, IMPORT_PREVIEW_LONG_EDGE)?;
                let id = format!("img_{}", NEXT_ID.fetch_add(1, Ordering::SeqCst));
                let params = TuningParams::default();
                let geom = crate::app_state::GeometryState::default();

                Ok(FilmItem {
                    id,
                    roll_id: target_roll_producer.clone(),
                    file_path: path.clone(),
                    embedded_thumbnail_base64,
                    rendered_thumbnail_base64: None,
                    original_proxy: None,
                    proxy_image: None,
                    prophoto_estimate_proxy: None,
                    relative_transmission_proxy: None,
                    relative_transmission_quality: None,
                    pristine_proxy: None,
                    base_color: BaseColor::default(),
                    runtime_pipeline_state: None,
                    runtime_density_provenance: None,
                    runtime_pipeline_key: None,
                    pipeline_state: default_pipeline_state.clone(),
                    params,
                    geom,
                    is_loose: loose,
                    in_library: in_lib,
                })
            });

        let to_work = |path: &String| match process_path(path) {
            Ok(item) => ImportWork::Item(item),
            Err(message) => ImportWork::Failed {
                file_path: path.clone(),
                message,
            },
        };

        // Give the first selected frame exclusive I/O priority. Continue in
        // selection order so the filmstrip never appears as a staircase when
        // later files happen to finish before earlier ones.
        let Some((first_path, remaining_paths)) = paths_to_process.split_first() else {
            return;
        };
        if tx.send(to_work(first_path)).is_err() {
            return;
        }
        let (result_tx, result_rx) = std::sync::mpsc::channel::<(usize, ImportWork)>();
        let preview_pool = rayon::ThreadPoolBuilder::new()
            .num_threads(
                std::thread::available_parallelism()
                    .map(|n| n.get().clamp(2, 8))
                    .unwrap_or(4),
            )
            .thread_name(|index| format!("nexfilm-import-preview-{index}"))
            .build()
            .ok();
        let queued_paths = remaining_paths.to_vec();
        let process_path_for_workers = process_path.clone();
        let result_tx_for_workers = result_tx.clone();
        let run_workers = move || {
            queued_paths.par_iter().enumerate().for_each_with(
                result_tx_for_workers,
                |sender, (offset, path)| {
                    let work = match process_path_for_workers(path) {
                        Ok(item) => ImportWork::Item(item),
                        Err(message) => ImportWork::Failed {
                            file_path: path.clone(),
                            message,
                        },
                    };
                    let _ = sender.send((offset + 1, work));
                },
            );
        };
        // Keep the pool alive until the result channel closes. Dropping a
        // detached pool immediately can terminate workers before all previews
        // have been delivered.
        if let Some(pool) = preview_pool.as_ref() {
            pool.spawn(run_workers);
        } else {
            std::thread::spawn(run_workers);
        }
        drop(result_tx);
        // Parallel preview work may complete out of order. Buffer only the
        // completed gaps and publish in the caller's exact selection order so
        // filenames such as 1..36 never shuffle in the filmstrip or library.
        forward_indexed_results_in_order(result_rx, 1, |item| tx.send(item).is_ok());
        // tx drops here → consumer's rx.recv() returns Err →
        // consumer flushes remaining buffer and emits import_complete
    });

    // ═══════════════════════════════════════════════════════════════════
    //  STEP 4: RETURN IMMEDIATELY — unblock Tauri IPC in microseconds.
    //  This is the SINGLE most critical line for fixing "stuck at 0%" and
    //  OS-level stuttering. Both threads run independently from here on.
    // ═══════════════════════════════════════════════════════════════════
    Ok(())
}
fn filmstrip_item(item: &FilmItem) -> FilmstripItem {
    let file_missing = std::fs::File::open(&item.file_path).is_err();
    FilmstripItem {
        id: item.id.clone(),
        roll_id: item.roll_id.clone(),
        file_path: item.file_path.clone(),
        thumbnail_base64: item.preferred_thumbnail().to_string(),
        embedded_thumbnail_base64: item.embedded_thumbnail_base64.clone(),
        rendered_thumbnail_base64: item.rendered_thumbnail_base64.clone(),
        thumbnail_kind: item.thumbnail_kind().to_string(),
        base_analyzed: pipeline_has_base(item.effective_pipeline_state(), &item.base_color),
        state_available: true,
        file_missing,
    }
}

fn clear_library_membership(state: &EngineState) -> Result<(), String> {
    for entry in state.items.iter() {
        entry
            .value()
            .write()
            .map_err(|error| error.to_string())?
            .in_library = false;
    }
    *state.active_id.write().map_err(|error| error.to_string())? = None;
    Ok(())
}

fn activate_library_roll(state: &EngineState, roll: &Roll) -> Result<Vec<String>, String> {
    clear_library_membership(state)?;
    let roll_paths: HashSet<String> = roll
        .image_paths
        .iter()
        .map(|path| normalize_path(path))
        .collect();
    let is_loose_roll = roll.format == "Loose" || roll.roll_id == "LOOSE_DEFAULT";
    let mut activated_ids = Vec::new();
    for entry in state.items.iter() {
        let mut item = write_lock(entry.value());
        if item.roll_id == roll.roll_id && roll_paths.contains(&normalize_path(&item.file_path)) {
            item.in_library = true;
            item.is_loose = is_loose_roll;
            activated_ids.push(item.id.clone());
        }
    }
    Ok(activated_ids)
}

#[tauri::command]
pub async fn get_filmstrip(state: State<'_, EngineState>) -> Result<Vec<FilmstripItem>, String> {
    let item_order = state.item_order.read().map_err(|e| e.to_string())?;
    let mut strip = Vec::with_capacity(item_order.len());
    for id in item_order.iter() {
        if let Some(item_arc) = state.items.get(id) {
            let item = item_arc.read().map_err(|e| e.to_string())?;
            if item.in_library {
                strip.push(filmstrip_item(&item));
            }
        }
    }
    Ok(strip)
}

#[tauri::command]
pub async fn get_roll_filmstrip(
    roll_id: String,
    state: State<'_, EngineState>,
) -> Result<Vec<FilmstripItem>, String> {
    let roll = state
        .rolls
        .read()
        .map_err(|error| error.to_string())?
        .iter()
        .find(|roll| roll.roll_id == roll_id)
        .cloned()
        .ok_or_else(|| format!("Roll not found: {roll_id}"))?;
    let mut strip = Vec::with_capacity(roll.image_paths.len());
    let guard = state.items.clone();
    for path in &roll.image_paths {
        let mut found = false;
        for kv in guard.iter() {
            let item = read_lock(kv.value());
            let db_path = item.file_path.clone();
            if item.roll_id == roll_id
                && (db_path == *path
                    || db_path.replace("\\", "/").to_lowercase()
                        == path.replace("\\", "/").to_lowercase())
            {
                strip.push(filmstrip_item(&item));
                found = true;
                break;
            }
        }
        if !found {
            strip.push(FilmstripItem {
                id: format!("archive_missing_{}_{}", roll_id, strip.len()),
                roll_id: roll_id.clone(),
                file_path: path.clone(),
                thumbnail_base64: if std::fs::File::open(path).is_ok() {
                    FALLBACK_THUMB.to_string()
                } else {
                    "FILE_MISSING".to_string()
                },
                embedded_thumbnail_base64: FALLBACK_THUMB.to_string(),
                rendered_thumbnail_base64: None,
                thumbnail_kind: "embedded".to_string(),
                base_analyzed: false,
                state_available: false,
                file_missing: std::fs::File::open(path).is_err(),
            });
        }
    }
    Ok(strip)
}

#[derive(Serialize)]
pub struct LutData {
    pub size: u32,
    pub data: Vec<u8>,
    pub is_1d: bool,
}

#[derive(Clone, Debug)]
struct ParsedLut {
    size: usize,
    rgba: Vec<f32>,
    is_1d: bool,
}

impl ParsedLut {
    fn into_ipc(self) -> LutData {
        let data = unsafe {
            std::slice::from_raw_parts(
                self.rgba.as_ptr() as *const u8,
                self.rgba.len() * std::mem::size_of::<f32>(),
            )
        }
        .to_vec();
        LutData {
            size: self.size as u32,
            data,
            is_1d: self.is_1d,
        }
    }

    fn sample(&self, rgb: [f32; 3]) -> [f32; 3] {
        if self.size < 2 || self.rgba.len() < self.size * 4 {
            return rgb;
        }
        if self.is_1d {
            return [
                self.sample_1d(rgb[0], 0),
                self.sample_1d(rgb[1], 1),
                self.sample_1d(rgb[2], 2),
            ];
        }
        self.sample_3d(rgb)
    }

    fn sample_1d(&self, value: f32, channel: usize) -> f32 {
        let position = value.clamp(0.0, 1.0) * (self.size - 1) as f32;
        let low = position.floor() as usize;
        let high = (low + 1).min(self.size - 1);
        let fraction = position - low as f32;
        let a = self.rgba[low * 4 + channel];
        let b = self.rgba[high * 4 + channel];
        a + (b - a) * fraction
    }

    fn sample_3d(&self, rgb: [f32; 3]) -> [f32; 3] {
        let position = rgb.map(|value| value.clamp(0.0, 1.0) * (self.size - 1) as f32);
        let low = position.map(|value| value.floor() as usize);
        let high = low.map(|value| (value + 1).min(self.size - 1));
        let fraction = [
            position[0] - low[0] as f32,
            position[1] - low[1] as f32,
            position[2] - low[2] as f32,
        ];
        let mut output = [0.0; 3];
        for z in 0..=1 {
            for y in 0..=1 {
                for x in 0..=1 {
                    let coordinates = [
                        if x == 0 { low[0] } else { high[0] },
                        if y == 0 { low[1] } else { high[1] },
                        if z == 0 { low[2] } else { high[2] },
                    ];
                    let weight = if x == 0 {
                        1.0 - fraction[0]
                    } else {
                        fraction[0]
                    } * if y == 0 {
                        1.0 - fraction[1]
                    } else {
                        fraction[1]
                    } * if z == 0 {
                        1.0 - fraction[2]
                    } else {
                        fraction[2]
                    };
                    let index = ((coordinates[2] * self.size + coordinates[1]) * self.size
                        + coordinates[0])
                        * 4;
                    for channel in 0..3 {
                        output[channel] += self.rgba[index + channel] * weight;
                    }
                }
            }
        }
        output
    }
}

fn extract_points(v: &Value, channel: &str) -> Vec<[f32; 2]> {
    let mut points = Vec::new();
    let mut target = &Value::Null;
    let channel_upper = channel.to_uppercase();
    let channel_lower = channel.to_lowercase();

    macro_rules! find_channel {
        ($obj:expr) => {
            $obj.get(&channel_upper)
                .or_else(|| $obj.get(&channel_lower))
        };
    }

    if let Some(t1) = find_channel!(v) {
        target = t1;
    } else if let Some(curves) = v.get("curves") {
        if let Some(t2) = find_channel!(curves) {
            target = t2;
        }
    } else if let Some(points_obj) = v.get("points") {
        if let Some(t3) = find_channel!(points_obj) {
            target = t3;
        }
    } else if let Some(cc) = v.get("cc_params") {
        if let Some(dc) = cc.get("density_curve") {
            if let Some(pts) = dc.get("points") {
                if let Some(t4) = find_channel!(pts) {
                    target = t4;
                }
            }
        }
    }

    if let Some(arr) = target.as_array() {
        for item in arr {
            if let Some(pair) = item.as_array() {
                if pair.len() >= 2 {
                    if let (Some(x), Some(y)) = (pair[0].as_f64(), pair[1].as_f64()) {
                        points.push([x as f32, y as f32]);
                    }
                }
            }
        }
    }
    points
}

fn interpolate(x: f32, points: &[[f32; 2]]) -> f32 {
    if points.is_empty() {
        return x;
    }
    if x <= points[0][0] {
        return points[0][1];
    }
    if x >= points[points.len() - 1][0] {
        return points[points.len() - 1][1];
    }
    for i in 0..points.len() - 1 {
        let p0 = points[i];
        let p1 = points[i + 1];
        if x >= p0[0] && x <= p1[0] {
            let mut t = 0.0;
            if p1[0] - p0[0] > 1e-6 {
                t = (x - p0[0]) / (p1[0] - p0[0]);
            }
            return p0[1] + t * (p1[1] - p0[1]);
        }
    }
    x
}

fn parse_lut(path: &str) -> Result<ParsedLut, String> {
    let content = std::fs::read_to_string(path).map_err(|e| e.to_string())?;

    if path.to_lowercase().ends_with(".json") {
        let v: Value =
            serde_json::from_str(&content).map_err(|e| format!("Invalid JSON: {}", e))?;
        let mut r_points = extract_points(&v, "r");
        let mut g_points = extract_points(&v, "g");
        let mut b_points = extract_points(&v, "b");
        let rgb_points = extract_points(&v, "rgb");

        if r_points.is_empty() {
            r_points = rgb_points.clone();
        }
        if g_points.is_empty() {
            g_points = rgb_points.clone();
        }
        if b_points.is_empty() {
            b_points = rgb_points.clone();
        }

        if r_points.is_empty() {
            return Err("No valid curve points found in JSON".to_string());
        }

        r_points.sort_by(|a, b| a[0].total_cmp(&b[0]));
        g_points.sort_by(|a, b| a[0].total_cmp(&b[0]));
        b_points.sort_by(|a, b| a[0].total_cmp(&b[0]));

        let size = 1024;
        let mut data_floats: Vec<f32> = Vec::with_capacity(size * 4);
        for i in 0..size {
            let x = i as f32 / (size - 1) as f32;
            let r_val = interpolate(x, &r_points);
            let g_val = interpolate(x, &g_points);
            let b_val = interpolate(x, &b_points);
            data_floats.push(r_val);
            data_floats.push(g_val);
            data_floats.push(b_val);
            data_floats.push(1.0); // Alpha
        }

        return Ok(ParsedLut {
            size,
            rgba: data_floats,
            is_1d: true,
        });
    }

    let mut size_3d = 0;
    let mut size_1d = 0;
    let mut data_floats: Vec<f32> = Vec::new();

    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line.starts_with("LUT_3D_SIZE") {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() == 2 {
                size_3d = parts[1].parse().unwrap_or(0);
            }
        } else if line.starts_with("LUT_1D_SIZE") {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() == 2 {
                size_1d = parts[1].parse().unwrap_or(0);
            }
        } else if line.starts_with("DOMAIN_MIN")
            || line.starts_with("DOMAIN_MAX")
            || line.starts_with("TITLE")
        {
            continue;
        } else {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() == 3 {
                if let (Ok(r), Ok(g), Ok(b)) = (
                    parts[0].parse::<f32>(),
                    parts[1].parse::<f32>(),
                    parts[2].parse::<f32>(),
                ) {
                    data_floats.push(r);
                    data_floats.push(g);
                    data_floats.push(b);
                }
            }
        }
    }

    if (size_3d == 0 && size_1d == 0) || data_floats.is_empty() {
        return Err("Invalid LUT file".into());
    }

    let mut max_val: f32 = 0.0;
    for &v in &data_floats {
        if v > max_val {
            max_val = v;
        }
    }
    if max_val > 1.0 {
        for v in &mut data_floats {
            *v /= 1023.0;
        }
    }

    let mut final_size = size_3d;
    let mut is_1d = false;

    if size_1d > 0 && size_3d == 0 {
        final_size = size_1d;
        is_1d = true;
    }
    if final_size < 2 {
        return Err("LUT size must be at least 2".into());
    }

    // Force RGB data to RGBA (Alpha = 1.0)
    let mut rgba_floats = Vec::with_capacity((data_floats.len() / 3) * 4);
    for chunk in data_floats.chunks(3) {
        if chunk.len() == 3 {
            rgba_floats.push(chunk[0]);
            rgba_floats.push(chunk[1]);
            rgba_floats.push(chunk[2]);
            rgba_floats.push(1.0);
        }
    }

    let expected_values = if is_1d {
        final_size * 4
    } else {
        final_size * final_size * final_size * 4
    };
    if rgba_floats.len() < expected_values {
        return Err(format!(
            "LUT declares size {final_size} but contains only {} RGB entries",
            rgba_floats.len() / 4
        ));
    }

    Ok(ParsedLut {
        size: final_size,
        rgba: rgba_floats,
        is_1d,
    })
}

fn validate_export_color_space(color_space: &str) -> Result<&'static str, String> {
    canonical_output_space(color_space).ok_or_else(|| {
        format!(
            "Unsupported export color space '{color_space}'. Choose sRGB, Display P3, Adobe RGB (1998), Rec.2020, ProPhoto RGB, ACEScg, or ACES2065-1."
        )
    })
}

fn encode_export_buffer(
    image: ImageBuffer<Rgb<u16>, Vec<u16>>,
    output_space: ColorSpaceId,
) -> Result<ImageBuffer<Rgb<u16>, Vec<u16>>, String> {
    // The inversion/tone/LUT pipeline produces the legacy display-referred
    // sRGB signal used by the preview. Decode that transfer curve before the
    // final matrix conversion; applying an OETF directly to these values is
    // the washed-out regression fixed by MATH_VERSION 3.
    let matrix = linear_conversion_matrix(ColorSpaceId::SRgb, output_space);
    let mut encoded = ImageBuffer::<Rgb<u16>, Vec<u16>>::new(image.width(), image.height());
    encoded
        .as_mut()
        .par_chunks_exact_mut(3)
        .zip(image.as_raw().par_chunks_exact(3))
        .for_each(|(target, source_pixel)| {
            let linear = convert_encoded_to_linear_rgb_with_matrix(
                [
                    source_pixel[0] as f32 / 65535.0,
                    source_pixel[1] as f32 / 65535.0,
                    source_pixel[2] as f32 / 65535.0,
                ],
                ColorSpaceId::SRgb,
                matrix,
            );
            let encoded_rgb = crate::color_science::encode_linear_rgb(linear, output_space);
            for channel in 0..3 {
                target[channel] = (encoded_rgb[channel].clamp(0.0, 1.0) * 65535.0).round() as u16;
            }
        });
    Ok(encoded)
}

#[cfg(test)]
mod lut_tests {
    use super::ParsedLut;

    #[test]
    fn identity_1d_lut_preserves_rgb() {
        let lut = ParsedLut {
            size: 2,
            rgba: vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0, 1.0],
            is_1d: true,
        };

        let input = [0.2, 0.5, 0.8];
        let output = lut.sample(input);
        for channel in 0..3 {
            assert!((output[channel] - input[channel]).abs() < 1e-6);
        }
    }

    #[test]
    fn identity_3d_lut_uses_opengl_texture_order() {
        let mut rgba = Vec::with_capacity(2 * 2 * 2 * 4);
        for blue in 0..=1 {
            for green in 0..=1 {
                for red in 0..=1 {
                    rgba.extend_from_slice(&[red as f32, green as f32, blue as f32, 1.0]);
                }
            }
        }
        let lut = ParsedLut {
            size: 2,
            rgba,
            is_1d: false,
        };

        let input = [0.2, 0.5, 0.8];
        let output = lut.sample(input);
        for channel in 0..3 {
            assert!((output[channel] - input[channel]).abs() < 1e-6);
        }
    }

    #[test]
    fn frontend_lut_sampling_uses_texel_center_coordinates() {
        let frontend = include_str!("../ui/main.js");
        assert!(frontend.contains("(clamp(value, 0.0, 1.0) * (size - 1.0) + 0.5) / size"));

        for size in [2.0_f32, 17.0, 33.0, 65.0] {
            for value in [0.0_f32, 0.2, 0.5, 0.8, 1.0] {
                let texture_coordinate = (value * (size - 1.0) + 0.5) / size;
                let texture_grid_position = texture_coordinate * size - 0.5;
                let cpu_grid_position = value * (size - 1.0);
                assert!((texture_grid_position - cpu_grid_position).abs() < 1e-6);
            }
        }
    }
}

#[tauri::command]
pub async fn load_3d_lut(path: String) -> Result<LutData, String> {
    tokio::task::spawn_blocking(move || parse_lut(&path).map(ParsedLut::into_ipc))
        .await
        .map_err(|error| format!("LUT worker failed: {error}"))?
}

#[tauri::command]
pub async fn get_roll_previews(
    roll_id: String,
    state: State<'_, EngineState>,
) -> Result<Vec<String>, String> {
    let rolls = read_lock(&state.rolls);
    if let Some(roll) = rolls.iter().find(|r| r.roll_id == roll_id) {
        return Ok(collect_roll_previews(roll, &state, 8));
    }
    Ok(Vec::new())
}

fn collect_roll_previews(roll: &Roll, state: &EngineState, limit: usize) -> Vec<String> {
    let mut previews = Vec::with_capacity(limit.min(roll.image_paths.len()));
    for path in &roll.image_paths {
        for entry in state.items.iter() {
            let item = read_lock(entry.value());
            if item.roll_id == roll.roll_id
                && normalize_path(&item.file_path) == normalize_path(path)
            {
                let thumbnail = item.preferred_thumbnail();
                if !thumbnail.is_empty() {
                    previews.push(thumbnail.to_string());
                }
                break;
            }
        }
        if previews.len() == limit {
            break;
        }
    }
    previews
}

#[cfg(test)]
mod history_contract_tests {
    use super::*;
    use crate::app_state::GeometryState;

    fn insert_history_item(
        state: &EngineState,
        id: &str,
        roll_id: &str,
        path: &str,
        rendered_thumbnail: Option<&str>,
    ) {
        state.items.insert(
            id.to_string(),
            Arc::new(RwLock::new(FilmItem {
                id: id.to_string(),
                roll_id: roll_id.to_string(),
                file_path: path.to_string(),
                embedded_thumbnail_base64: format!("orange-{id}"),
                rendered_thumbnail_base64: rendered_thumbnail.map(str::to_string),
                original_proxy: None,
                proxy_image: None,
                prophoto_estimate_proxy: None,
                relative_transmission_proxy: None,
                relative_transmission_quality: None,
                pristine_proxy: None,
                base_color: BaseColor::default(),
                runtime_pipeline_state: None,
                runtime_density_provenance: None,
                runtime_pipeline_key: None,
                pipeline_state: PipelineState::default(),
                params: TuningParams::default(),
                geom: GeometryState::default(),
                is_loose: false,
                in_library: false,
            })),
        );
    }

    #[test]
    fn roll_card_uses_the_first_frames_in_roll_order() {
        let state = EngineState::new();
        let roll = Roll {
            roll_id: "roll-a".into(),
            date: String::new(),
            format: "135".into(),
            film_stock: String::new(),
            camera: String::new(),
            image_paths: vec!["first.dng".into(), "second.dng".into(), "third.dng".into()],
            density_anchors: Default::default(),
            calibration_profile_id: None,
            scanner_profile_id: None,
        };
        insert_history_item(&state, "first", "roll-a", "first.dng", None);
        insert_history_item(
            &state,
            "wrong-roll",
            "roll-b",
            "second.dng",
            Some("wrong-positive"),
        );
        insert_history_item(
            &state,
            "second",
            "roll-a",
            "second.dng",
            Some("positive-second"),
        );
        insert_history_item(
            &state,
            "third",
            "roll-a",
            "third.dng",
            Some("positive-third"),
        );

        assert_eq!(
            collect_roll_previews(&roll, &state, 3),
            vec!["orange-first", "positive-second", "positive-third"]
        );
    }

    #[test]
    fn failed_import_paths_are_removed_only_from_the_owning_roll() {
        let mut rolls = vec![
            Roll {
                roll_id: "roll-a".into(),
                date: String::new(),
                format: "135".into(),
                film_stock: String::new(),
                camera: String::new(),
                image_paths: vec!["A\\First.DNG".into(), "A\\Second.DNG".into()],
                density_anchors: Default::default(),
                calibration_profile_id: None,
                scanner_profile_id: None,
            },
            Roll {
                roll_id: "roll-b".into(),
                date: String::new(),
                format: "135".into(),
                film_stock: String::new(),
                camera: String::new(),
                image_paths: vec!["A\\First.DNG".into()],
                density_anchors: Default::default(),
                calibration_profile_id: None,
                scanner_profile_id: None,
            },
        ];

        assert!(remove_failed_roll_paths(
            &mut rolls,
            "roll-a",
            &HashSet::from(["a/first.dng".to_string()]),
        ));
        assert_eq!(rolls[0].image_paths, vec!["A\\Second.DNG"]);
        assert_eq!(rolls[1].image_paths, vec!["A\\First.DNG"]);

        assert!(remove_failed_roll_paths(
            &mut rolls,
            "roll-a",
            &HashSet::from(["a/second.dng".to_string()]),
        ));
        assert_eq!(rolls.len(), 1);
        assert_eq!(rolls[0].roll_id, "roll-b");
    }
}

#[tauri::command]
pub async fn get_raw_thumbnails(paths: Vec<String>) -> Result<Vec<String>, String> {
    tokio::task::spawn_blocking(move || {
        paths
            .into_iter()
            .map(|path| decode_import_preview_result(&path, IMPORT_PREVIEW_LONG_EDGE))
            .collect::<Result<Vec<_>, _>>()
    })
    .await
    .map_err(|error| format!("Thumbnail worker failed: {error}"))?
}

#[tauri::command]
pub async fn get_embedded_preview(
    id: String,
    state: State<'_, EngineState>,
) -> Result<String, String> {
    let file_path = {
        let item_arc = state.items.get(&id).ok_or("Image ID not found")?;
        let item = read_lock(&item_arc);
        if std::fs::File::open(&item.file_path).is_err() {
            return Err("FILE_MISSING".into());
        }
        item.file_path.clone()
    };

    tokio::task::spawn_blocking(move || {
        decode_develop_preview_base64(&file_path, 2560)
            .unwrap_or_else(|| FALLBACK_THUMB.to_string())
    })
    .await
    .map_err(|e| e.to_string())
}

/// Decode the selected source through the full-resolution RAW path for
/// calibration inspection. The result is resized only after demosaic so this
/// never falls back to a camera-embedded thumbnail or the half-size proxy.
#[tauri::command]
pub async fn get_density_calibration_preview(
    id: String,
    state: State<'_, EngineState>,
) -> Result<String, String> {
    let file_path = {
        let item_arc = state.items.get(&id).ok_or("Image ID not found")?;
        let item = read_lock(&item_arc);
        if std::fs::File::open(&item.file_path).is_err() {
            return Err("FILE_MISSING".into());
        }
        item.file_path.clone()
    };

    tokio::task::spawn_blocking(move || {
        let image = decode_image_buffer(&file_path, DecodeMode::ExportFull)?;
        encode_stretched_preview_jpeg_base64(
            image::DynamicImage::ImageRgb16(image),
            MAX_PREVIEW_PROXY_LONG_EDGE,
            95,
        )
        .ok_or_else(|| format!("Could not encode calibration preview for {file_path}"))
    })
    .await
    .map_err(|error| format!("Calibration preview worker failed: {error}"))?
}

#[derive(serde::Serialize)]
pub struct ActiveImageState {
    pub params: TuningParams,
    pub geom: crate::app_state::GeometryState,
    pub base_analyzed: bool,
    pub pipeline_state: PipelineState,
}

// ═══════════════════════════════════════════════════════════════════════════
//  LRU Proxy Cache — strict bounded-capacity enforcement
// ═══════════════════════════════════════════════════════════════════════════

/// Evict the oldest proxy data from memory if the LRU cache exceeds MAX_PROXY_CACHE.
/// Physically drops the ImageBuffer allocations (~72MB per evicted image).
fn evict_proxy_if_needed(state: &EngineState) {
    let active_id = read_lock(&state.active_id).clone();
    let mut order = write_lock(&state.proxy_loaded_order);
    while order.len() > crate::app_state::MAX_PROXY_CACHE {
        // Only the active image is protected. Protecting a navigation window
        // can make every entry non-evictable and violate the hard limit.
        let victim_pos = order
            .iter()
            .position(|id| active_id.as_deref() != Some(id.as_str()));
        let Some(victim_pos) = victim_pos else {
            break;
        };
        if let Some(oldest_id) = order.remove(victim_pos) {
            if let Some(item_arc) = state.items.get(&oldest_id) {
                let mut item = write_lock(&item_arc);
                item.original_proxy = None;
                item.proxy_image = None;
                item.prophoto_estimate_proxy = None;
                item.relative_transmission_proxy = None;
                item.relative_transmission_quality = None;
                item.pristine_proxy = None;
            }
        }
    }
}

/// Mark an image's proxy as loaded and move it to the back of the LRU order.
/// Triggers eviction if the cache exceeds capacity.
fn track_proxy_loaded(state: &EngineState, id: &str) {
    let mut order = write_lock(&state.proxy_loaded_order);
    order.retain(|x| x != id);
    order.push_back(id.to_string());
    drop(order);
    evict_proxy_if_needed(state);
}

#[tauri::command]
pub async fn switch_active_image(
    id: String,
    roll_id: String,
    generation: u64,
    state: State<'_, EngineState>,
    _app_handle: tauri::AppHandle,
) -> Result<ActiveImageState, String> {
    let epoch = claim_development_generation(&state, &id, generation)?;
    // State activation is deliberately ordered before proxy preparation. The
    // database is authoritative, including settings written by batch jobs for
    // frames that have never been rendered in this process.
    let item_arc = state.items.get(&id).ok_or("Image ID not found")?.clone();
    let file_path = {
        let item = item_arc.read().map_err(|error| error.to_string())?;
        if item.roll_id != roll_id {
            return Err("Image does not belong to the requested roll".into());
        }
        if std::fs::File::open(&item.file_path).is_err() {
            return Err("FILE_MISSING".into());
        }
        item.file_path.clone()
    };

    let persisted_roll_id = roll_id.clone();
    let persisted_file_path = file_path.clone();
    let persisted = tokio::task::spawn_blocking(move || {
        let connection = persistence::open_connection()
            .map_err(|error| format!("Failed to open state database: {error}"))?;
        load_image_state_from_connection(&connection, &persisted_roll_id, &persisted_file_path)?
            .ok_or_else(|| "Persisted image state is missing".to_string())
    })
    .await
    .map_err(|error| format!("State-loading worker failed: {error}"))??;

    let (_, params, geom, base_color, pipeline_state) = persisted;
    ensure_current_development_generation(&epoch, generation)?;
    let mut item = item_arc.write().map_err(|error| error.to_string())?;
    ensure_current_development_generation(&epoch, generation)?;
    if item.roll_id != roll_id || item.file_path != file_path {
        return Err("Image identity changed while loading persisted state".into());
    }
    item.params = params;
    item.geom = geom;
    item.base_color = base_color;
    item.pipeline_state = pipeline_state;

    // Return the current resolved capability, not the persisted request. This
    // keeps UI caches and every processing entry point on the resolver's
    // authoritative contract after a Profile or density-anchor change.
    let profiles = load_calibration_profile_views().unwrap_or_default();
    let roll = state
        .rolls
        .read()
        .ok()
        .and_then(|rolls| rolls.iter().find(|roll| roll.roll_id == roll_id).cloned());
    let resolution = resolve_image_pipeline(
        &item.pipeline_state,
        roll.as_ref(),
        &profiles,
        &item.file_path,
        None,
    );
    let resolved_state = state_from_resolution(&item.pipeline_state, &resolution);
    item.runtime_pipeline_state = Some(resolved_state.clone());

    *state.active_id.write().map_err(|e| e.to_string())? = Some(id.clone());
    Ok(ActiveImageState {
        params: item.params.clone(),
        geom: item.geom.clone(),
        base_analyzed: pipeline_has_base(&resolved_state, &item.base_color),
        pipeline_state: resolved_state,
    })
}

#[tauri::command]
pub async fn prepare_proxy(
    id: String,
    target_long_edge: Option<u32>,
    state: State<'_, EngineState>,
) -> Result<u32, String> {
    let target_long_edge = preview_proxy_target_long_edge(target_long_edge);
    let item_arc = state.items.get(&id).ok_or("Image ID not found")?.clone();
    let (file_path, roll_id, current_long_edge, persisted_state, cached_resolution_key) = {
        let item = read_lock(&item_arc);
        if std::fs::File::open(&item.file_path).is_err() {
            return Err("FILE_MISSING".into());
        }
        let current_long_edge = item
            .proxy_image
            .as_ref()
            .map(|image| image.width().max(image.height()))
            .unwrap_or(0);
        (
            item.file_path.clone(),
            item.roll_id.clone(),
            current_long_edge,
            item.pipeline_state.clone(),
            item.runtime_pipeline_key.clone(),
        )
    };

    let rolls = read_lock(&state.rolls).clone();
    let roll = rolls.iter().find(|roll| roll.roll_id == roll_id);
    let profiles = load_calibration_profile_views()?;
    let scanner_profiles = {
        let connection = persistence::open_connection()
            .map_err(|error| format!("Failed to open scanner database: {error}"))?;
        persistence::load_scanner_profiles(&connection)
            .map_err(|error| format!("Failed to load scanner profiles: {error}"))?
    };
    let scanner_profile = roll
        .and_then(|roll| roll.scanner_profile_id.as_deref())
        .and_then(|profile_id| {
            scanner_profiles
                .iter()
                .find(|record| record.profile.profile_id == profile_id)
        })
        .filter(|record| record_is_current(record))
        .map(|record| record.profile.clone());
    if roll.is_some_and(|roll| roll.scanner_profile_id.is_some()) && scanner_profile.is_none() {
        eprintln!("[Scanner Pipeline] Bound scanner profile is missing or stale; using source RGB unchanged");
    }
    let initial_resolution =
        resolve_image_pipeline(&persisted_state, roll, &profiles, &file_path, None);
    let initial_resolution_key = resolution_key(&initial_resolution);
    if current_long_edge >= target_long_edge
        && cached_resolution_key.as_deref() == Some(initial_resolution_key.as_str())
    {
        track_proxy_loaded(&state, &id);
        return Ok(current_long_edge);
    }

    let contract = initial_resolution.resolved_path;
    let capture_profile = if contract == ProcessingContract::CaptureCorrectedV11 {
        initial_resolution
            .resolved_profile_id
            .as_deref()
            .and_then(|profile_id| {
                profiles
                    .iter()
                    .find(|view| view.profile.profile_id == profile_id)
                    .map(|view| view.profile.clone())
            })
    } else {
        None
    };

    struct PreparedProxy {
        transport: ImageBuffer<Rgb<u16>, Vec<u16>>,
        prophoto_estimate: Option<ImageBuffer<Rgb<f32>, Vec<f32>>>,
        capture_corrected: Option<CaptureCorrectedProxyData>,
        fallback_reason: Option<String>,
    }

    let decode_path = file_path.clone();
    let prepared =
        tokio::task::spawn_blocking(move || -> Result<_, String> {
            let decode_mode = preview_proxy_decode_mode(target_long_edge);
            if contract == ProcessingContract::CaptureCorrectedV11 {
                let capture_attempt = capture_profile
                    .as_ref()
                    .ok_or_else(|| "capture_profile_unavailable".to_string())
                    .and_then(|profile| decode_capture_corrected_image_buffer(&decode_path, profile))
                    .map(|capture| resize_capture_corrected_proxy(capture, target_long_edge));
                match capture_attempt {
                    Ok(capture_corrected) => {
                        let transport = relative_transmission_to_transport_proxy(&capture_corrected);
                        return Ok(PreparedProxy {
                            transport,
                            prophoto_estimate: None,
                            capture_corrected: Some(capture_corrected),
                            fallback_reason: None,
                        });
                    }
                    Err(error) => {
                        eprintln!(
                            "[RAW Pipeline] Capture Corrected unavailable; falling back to Smart Auto / ProPhoto Estimate for this invocation: {error}"
                        );
                        let mut estimate = decode_scanner_profiled_estimate_image_buffer(
                            &decode_path,
                            decode_mode,
                            scanner_profile.as_ref(),
                            target_long_edge,
                        )?;
                        let (width, height) = estimate.dimensions();
                        let ratio =
                            (target_long_edge as f32 / width.max(height) as f32).min(1.0);
                        if ratio < 0.999 {
                            estimate = image::imageops::resize(
                                &estimate,
                                (width as f32 * ratio).max(1.0) as u32,
                                (height as f32 * ratio).max(1.0) as u32,
                                FilterType::Lanczos3,
                            );
                        }
                        return Ok(PreparedProxy {
                            transport: prophoto_estimate_to_transport_proxy(&estimate),
                            prophoto_estimate: Some(estimate),
                            capture_corrected: None,
                            fallback_reason: Some(error),
                        });
                    }
                }
            }
            if contract != ProcessingContract::LegacyV1 {
                let mut estimate = if scanner_profile.is_some()
                    && !is_raw_extension(&decode_path)
                    && !is_dng_extension(&decode_path)
                {
                    decode_scanner_profiled_estimate_image_buffer(
                        &decode_path,
                        decode_mode,
                        scanner_profile.as_ref(),
                        target_long_edge,
                    )?
                } else if is_dng_extension(&decode_path) {
                    let linear = decode_reduced_dng_for_working_space(&decode_path, target_long_edge)
                        .or_else(|_| decode_image_buffer(&decode_path, decode_mode))?;
                    linear_srgb_u16_to_prophoto_f32(&linear)
                } else if is_tiff_extension(&decode_path) || is_scanner_fff_tiff(&decode_path) {
                    let linear = decode_tiff_for_smart_auto(&decode_path, target_long_edge)?;
                    linear_srgb_u16_to_prophoto_f32(&linear)
                } else {
                    decode_scanner_profiled_estimate_image_buffer(
                        &decode_path,
                        decode_mode,
                        scanner_profile.as_ref(),
                        target_long_edge,
                    )?
                };
                let (width, height) = estimate.dimensions();
                let ratio = (target_long_edge as f32 / width.max(height) as f32).min(1.0);
                if ratio < 0.999 {
                    estimate = image::imageops::resize(
                        &estimate,
                        (width as f32 * ratio).max(1.0) as u32,
                        (height as f32 * ratio).max(1.0) as u32,
                        FilterType::Lanczos3,
                    );
                }
                let transport = prophoto_estimate_to_transport_proxy(&estimate);
                return Ok(PreparedProxy {
                    transport,
                    prophoto_estimate: Some(estimate),
                    capture_corrected: None,
                    fallback_reason: None,
                });
            }

            if scanner_profile.is_some() && !is_raw_extension(&decode_path) {
                let mut estimate = decode_scanner_profiled_estimate_image_buffer(
                    &decode_path,
                    decode_mode,
                    scanner_profile.as_ref(),
                    target_long_edge,
                )?;
                let (width, height) = estimate.dimensions();
                let ratio = (target_long_edge as f32 / width.max(height) as f32).min(1.0);
                if ratio < 0.999 {
                    estimate = image::imageops::resize(
                        &estimate,
                        (width as f32 * ratio).max(1.0) as u32,
                        (height as f32 * ratio).max(1.0) as u32,
                        FilterType::Lanczos3,
                    );
                }
                return Ok(PreparedProxy {
                    transport: prophoto_estimate_to_transport_proxy(&estimate),
                    prophoto_estimate: Some(estimate),
                    capture_corrected: None,
                    fallback_reason: None,
                });
            }
            let img_buffer = if is_dng_extension(&decode_path) {
                decode_reduced_dng_for_working_space(&decode_path, target_long_edge)
                    .or_else(|_| decode_image_buffer(&decode_path, decode_mode))?
            } else if is_tiff_extension(&decode_path) || is_scanner_fff_tiff(&decode_path) {
                decode_reduced_tiff_for_working_space(&decode_path, target_long_edge)
                    .or_else(|_| decode_image_buffer(&decode_path, decode_mode))?
            } else {
                decode_image_buffer(&decode_path, decode_mode)?
            };
            let (width, height) = img_buffer.dimensions();
            let ratio = (target_long_edge as f32 / width.max(height) as f32).min(1.0);
            let display = if ratio < 0.999 {
                image::imageops::resize(
                    &img_buffer,
                    (width as f32 * ratio).max(1.0) as u32,
                    (height as f32 * ratio).max(1.0) as u32,
                    FilterType::Lanczos3,
                )
            } else {
                img_buffer
            };
            Ok(PreparedProxy {
                transport: display,
                prophoto_estimate: None,
                capture_corrected: None,
                fallback_reason: None,
            })
        })
        .await
        .map_err(|e| e.to_string())??;

    let final_resolution = if let Some(reason) = prepared.fallback_reason.clone() {
        resolve_image_pipeline(&persisted_state, roll, &profiles, &file_path, Some(reason))
    } else {
        initial_resolution
    };
    let final_state = state_from_resolution(&persisted_state, &final_resolution);
    let final_resolution_key = resolution_key(&final_resolution);
    let density_provenance = resolution_density_provenance(&final_resolution);
    let loaded_long_edge = prepared.transport.width().max(prepared.transport.height());
    let retained_long_edge = {
        let mut item = write_lock(&item_arc);
        // Legacy keeps linear-sRGB u16. Smart Auto and Capture Corrected keep
        // domain-typed f32 sources plus a separate u16 GPU transport texture.
        let retained_long_edge = item
            .proxy_image
            .as_ref()
            .map(|image| image.width().max(image.height()))
            .unwrap_or(0);
        let resolution_changed =
            item.runtime_pipeline_key.as_deref() != Some(final_resolution_key.as_str());
        if loaded_long_edge > retained_long_edge || resolution_changed {
            item.original_proxy = None;
            item.proxy_image = Some(prepared.transport);
            item.prophoto_estimate_proxy = prepared.prophoto_estimate;
            item.relative_transmission_proxy = prepared
                .capture_corrected
                .as_ref()
                .map(|data| data.image.clone());
            item.relative_transmission_quality =
                prepared.capture_corrected.map(|data| data.quality);
            item.pristine_proxy = None;
        }
        item.runtime_pipeline_state = Some(final_state);
        item.runtime_density_provenance = Some(density_provenance);
        item.runtime_pipeline_key = Some(final_resolution_key);
        loaded_long_edge.max(retained_long_edge)
    };
    track_proxy_loaded(&state, &id);
    Ok(retained_long_edge)
}

#[tauri::command]
pub async fn analyze_proxy_base_color(
    id: String,
    generation: u64,
    state: State<'_, EngineState>,
) -> Result<(), String> {
    let epoch = claim_development_generation(&state, &id, generation)?;
    let item_arc = state.items.get(&id).ok_or("Image ID not found")?.clone();

    tokio::task::spawn_blocking(move || {
        ensure_current_development_generation(&epoch, generation)?;
        let (base_color, runtime_pipeline_state, persisted_pipeline_state) = {
            let item = read_lock(&item_arc);
            let effective = item.effective_pipeline_state().clone();
            if pipeline_has_base(&effective, &item.base_color) {
                return Ok(());
            }
            if effective.contract == ProcessingContract::LegacyV1 {
                let proxy = item
                    .proxy_image
                    .as_ref()
                    .ok_or_else(|| "PROXY_NOT_READY".to_string())?;
                (
                    compute_auto_base(proxy),
                    effective,
                    item.pipeline_state.clone(),
                )
            } else {
                let capture_corrected =
                    effective.contract == ProcessingContract::CaptureCorrectedV11;
                let input = if capture_corrected {
                    item.relative_transmission_proxy.as_ref()
                } else {
                    item.prophoto_estimate_proxy.as_ref()
                }
                .ok_or_else(|| "PROXY_NOT_READY".to_string())?;
                let quality = capture_corrected
                    .then_some(item.relative_transmission_quality.as_ref())
                    .flatten();
                let (density, estimated_confidence, estimated_source) =
                    if effective.density_anchors.has_roll_full_exposure() {
                        // A sampled leader fixes D-max. Its missing base endpoint
                        // must be inferred from the confirmed Film Area, not from
                        // unrelated border and sprocket pixels in the full scan.
                        (
                            compute_content_limits_f32(input, quality, &item.geom, [0.0; 3])?.d_min,
                            0.8,
                            "detected_film_base",
                        )
                    } else if let Some(quality) = quality {
                        (
                            compute_auto_base_capture_corrected(input, quality, &item.geom)?,
                            0.9,
                            "detected_film_base",
                        )
                    } else {
                        let (density, confidence) = compute_auto_base_f32(input, &item.geom)?;
                        (
                            density,
                            confidence,
                            if confidence > 0.0 {
                                "content_estimate"
                            } else {
                                "compatibility_fallback"
                            },
                        )
                    };
                let mut runtime = effective;
                let mut persisted = item.pipeline_state.clone();
                for report in [
                    &mut runtime.processing_report,
                    &mut persisted.processing_report,
                ] {
                    report.base_source = estimated_source.to_string();
                    report.base_confidence = format!("{estimated_confidence:.3}");
                    report.analysis_data_domain = if capture_corrected {
                        "relative_transmission_rgb".to_string()
                    } else {
                        "linear_prophoto_estimate".to_string()
                    };
                    report.uses_physical_anchors = runtime.density_anchors.has_base();
                    let (open, saturated, invalid) = smart_auto_exclusion_counts(input, &item.geom);
                    report.excluded_open_light_pixels = open;
                    report.excluded_saturated_pixels = saturated;
                    report.excluded_invalid_pixels = invalid;
                    if estimated_confidence <= 0.0
                        && !report
                            .fallback_reasons
                            .iter()
                            .any(|reason| reason == "smart_auto_no_trusted_film_base")
                    {
                        report
                            .fallback_reasons
                            .push("smart_auto_no_trusted_film_base".to_string());
                    }
                }
                (base_color_from_density(density), runtime, persisted)
            }
        };

        let mut item = write_lock(&item_arc);
        ensure_current_development_generation(&epoch, generation)?;
        if pipeline_has_base(item.effective_pipeline_state(), &item.base_color) {
            return Ok(());
        }
        persist_base_and_pipeline(
            &item.roll_id,
            &item.file_path,
            &base_color,
            &persisted_pipeline_state,
        )?;
        item.base_color = base_color;
        item.pipeline_state = persisted_pipeline_state;
        item.runtime_pipeline_state = Some(runtime_pipeline_state);
        item.pristine_proxy = None;
        Ok(())
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn analyze_proxy_density_limits(
    id: String,
    state: State<'_, EngineState>,
) -> Result<AutoColorLimits, String> {
    let item_arc = state.items.get(&id).ok_or("Image ID not found")?.clone();
    tokio::task::spawn_blocking(move || {
        let (
            legacy_proxy,
            prophoto_estimate,
            relative_transmission,
            relative_transmission_quality,
            geom,
            base_color,
            mode,
            linked_color_limits,
            mut pipeline_state,
        ) = {
            let item = read_lock(&item_arc);
            if !pipeline_has_base(item.effective_pipeline_state(), &item.base_color) {
                return Err("BASE_COLOR_NOT_ANALYZED".to_string());
            }
            (
                item.proxy_image.clone(),
                item.prophoto_estimate_proxy.clone(),
                item.relative_transmission_proxy.clone(),
                item.relative_transmission_quality.clone(),
                item.geom.clone(),
                item.base_color.clone(),
                item.params.film_mode.clone(),
                is_noritsu_rendered_image(&item.file_path),
                item.effective_pipeline_state().clone(),
            )
        };
        let mut limits = if pipeline_state.contract == ProcessingContract::LegacyV1 {
            compute_auto_color_limits(
                &legacy_proxy.ok_or_else(|| "PROXY_NOT_READY".to_string())?,
                &geom,
                &base_color,
                mode,
                linked_color_limits,
            )?
        } else {
            let (input, quality) =
                if pipeline_state.contract == ProcessingContract::CaptureCorrectedV11 {
                    (
                        relative_transmission
                            .as_ref()
                            .ok_or_else(|| "PROXY_NOT_READY".to_string())?,
                        relative_transmission_quality.as_ref(),
                    )
                } else {
                    (
                        prophoto_estimate
                            .as_ref()
                            .ok_or_else(|| "PROXY_NOT_READY".to_string())?,
                        None,
                    )
                };
            let base = pipeline_base_density(&pipeline_state, &base_color);
            if pipeline_state.density_anchors.is_fully_anchored() {
                let full_exposure = pipeline_state
                    .density_anchors
                    .d_max_full_exposure
                    .as_ref()
                    .expect("complete anchors include full exposure");
                AutoColorLimits {
                    d_min: [0.0; 3],
                    d_max: [
                        full_exposure.density[0] - base[0],
                        full_exposure.density[1] - base[1],
                        full_exposure.density[2] - base[2],
                    ],
                    pipeline_state: None,
                }
            } else {
                let mut estimated = compute_content_limits_f32(input, quality, &geom, base)?;
                if pipeline_state.contract != ProcessingContract::CaptureCorrectedV11
                    && !pipeline_state.density_anchors.has_roll_base()
                    && !pipeline_state.density_anchors.has_roll_full_exposure()
                {
                    // Smart Auto has no physical channel endpoints. Use one
                    // luma scale so the orange mask cannot turn into a green
                    // or cyan cast through three independent stretches.
                    let short_content = estimated.d_max[0] - estimated.d_min[0] < 0.8;
                    share_smart_auto_density_scale(&mut estimated);
                    preserve_smart_auto_content_span(&mut estimated);
                    if short_content {
                        pipeline_state.processing_report.tone_mapping_mode =
                            "preserve_tone_adaptive_midpoint".to_string();
                    }
                }
                apply_roll_density_anchor_limits(
                    &mut estimated,
                    &pipeline_state.density_anchors,
                    base,
                );
                estimated
            }
        };
        if pipeline_state.contract != ProcessingContract::LegacyV1 {
            let analysis_limits = limits.clone();
            if !pipeline_state.density_anchors.is_fully_anchored() {
                pipeline_state.content_range = Some(ContentRange {
                    low: analysis_limits.d_min,
                    high: analysis_limits.d_max,
                    source_scope: if geom.calibration_points.is_some() {
                        ContentRangeScope::FilmArea
                    } else {
                        ContentRangeScope::FullFrame
                    },
                    percentile_method: "co_sited_2pct_v1".to_string(),
                });
                preserve_tone_density_span(&mut limits, &pipeline_state.density_anchors);
            }
            pipeline_state.render_mapping.mode = RenderMode::PreserveTone;
            pipeline_state.render_mapping.density_low = limits.d_min;
            pipeline_state.render_mapping.density_high = limits.d_max;
            let mut item = write_lock(&item_arc);
            let mut persisted = item.pipeline_state.clone();
            persisted.processing_report = pipeline_state.processing_report.clone();
            persisted.content_range = pipeline_state.content_range.clone();
            persisted.render_mapping = pipeline_state.render_mapping.clone();
            persist_pipeline_state(&item.roll_id, &item.file_path, &persisted)?;
            item.pipeline_state = persisted;
            item.runtime_pipeline_state = Some(pipeline_state.clone());
            limits.pipeline_state = Some(pipeline_state);
        }
        Ok(limits)
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn reset_image_development(
    id: String,
    mut params: TuningParams,
    generation: u64,
    state: State<'_, EngineState>,
) -> Result<PipelineState, String> {
    let epoch = claim_development_generation(&state, &id, generation)?;
    params.raw_decode.working_colorspace = DENSITY_CAPTURE_WORKING_SPACE.to_string();
    let item_arc = state.items.get(&id).ok_or("Image ID not found")?.clone();

    tokio::task::spawn_blocking(move || {
        let mut item = write_lock(&item_arc);
        ensure_current_development_generation(&epoch, generation)?;
        let default_base = BaseColor::default();
        let mut reset_pipeline = if item.pipeline_state.contract == ProcessingContract::LegacyV1 {
            PipelineState::default()
        } else {
            let mut pipeline = item.pipeline_state.clone();
            let capture_corrected = pipeline.contract == ProcessingContract::CaptureCorrectedV11;
            pipeline.density_anchors.d_min_base = pipeline
                .density_anchors
                .d_min_base
                .filter(|anchor| anchor.scope == DensityAnchorScope::Roll);
            pipeline.density_anchors.d_max_full_exposure = pipeline
                .density_anchors
                .d_max_full_exposure
                .filter(|anchor| anchor.scope == DensityAnchorScope::Roll);
            pipeline.contract = if capture_corrected {
                ProcessingContract::CaptureCorrectedV11
            } else {
                pipeline.density_anchors.prophoto_contract()
            };
            pipeline.content_range = None;
            pipeline.render_mapping = Default::default();
            pipeline.processing_report = if capture_corrected {
                PipelineProcessingReport::capture_corrected(false)
            } else {
                let mut report = PipelineProcessingReport::smart_auto();
                if pipeline.density_anchors.has_roll_base() {
                    report.base_source = "verified_anchor".to_string();
                    report.base_confidence = "verified".to_string();
                    report.uses_physical_anchors = true;
                }
                report
            };
            pipeline
        };
        if reset_pipeline.density_anchors.d_min_base.is_none()
            && reset_pipeline.contract != ProcessingContract::CaptureCorrectedV11
        {
            reset_pipeline.contract = match reset_pipeline.contract {
                ProcessingContract::LegacyV1 => ProcessingContract::LegacyV1,
                _ => ProcessingContract::SmartAutoProPhotoV11,
            };
        }
        let params_json = serde_json::to_string(&params)
            .map_err(|error| format!("Failed to serialize reset parameters: {error}"))?;
        let base_json = serde_json::to_string(&default_base)
            .map_err(|error| format!("Failed to serialize reset base color: {error}"))?;
        let pipeline_json = serde_json::to_string(&reset_pipeline)
            .map_err(|error| format!("Failed to serialize reset pipeline: {error}"))?;
        let connection = persistence::open_connection()
            .map_err(|error| format!("Failed to open image database: {error}"))?;
        let changed = connection
            .execute(
                "UPDATE image_states
                 SET params = ?1, base_color = ?2, pipeline_state = ?3,
                     rendered_thumb_base64 = NULL, thumbnail_base64 = ?4, updated_at = ?5
                 WHERE roll_id = ?6 AND file_path = ?7",
                rusqlite::params![
                    params_json,
                    base_json,
                    pipeline_json,
                    item.embedded_thumbnail_base64,
                    persistence::now_timestamp(),
                    item.roll_id,
                    item.file_path,
                ],
            )
            .map_err(|error| format!("Failed to reset image state: {error}"))?;
        if changed != 1 {
            return Err(format!(
                "Persisted image state was not found: {}",
                item.file_path
            ));
        }

        item.params = params;
        item.base_color = default_base;
        item.pipeline_state = reset_pipeline;
        item.runtime_pipeline_state = None;
        item.runtime_density_provenance = None;
        item.runtime_pipeline_key = None;
        item.rendered_thumbnail_base64 = None;
        item.pristine_proxy = None;
        Ok(item.pipeline_state.clone())
    })
    .await
    .map_err(|error| format!("Reset worker failed: {error}"))?
}

#[tauri::command]
pub async fn sync_thumbnail_buffer(
    id: String,
    generation: u64,
    state: State<'_, EngineState>,
) -> Result<(), String> {
    let epoch = claim_development_generation(&state, &id, generation)?;
    let item_arc = state.items.get(&id).ok_or("Image ID not found")?.clone();
    tokio::task::spawn_blocking(move || {
        ensure_current_development_generation(&epoch, generation)?;
        {
            let mut item = write_lock(&item_arc);
            if item.pristine_proxy.is_none() {
                if let Some(proxy) = item.proxy_image.as_ref() {
                    item.pristine_proxy = Some(compute_pristine_proxy(
                        proxy,
                        item.prophoto_estimate_proxy.as_ref(),
                        item.relative_transmission_proxy.as_ref(),
                        item.relative_transmission_quality.as_ref(),
                        &item.base_color,
                        item.effective_pipeline_state(),
                        item.params.film_mode.clone(),
                    ));
                }
            }
        }
        let new_thumbnail = {
            let item = read_lock(&item_arc);
            generate_processed_thumbnail(&item)
                .ok_or_else(|| "Thumbnail source is not ready".to_string())?
        };
        if new_thumbnail.is_empty() {
            return Ok(());
        }
        let mut item = write_lock(&item_arc);
        ensure_current_development_generation(&epoch, generation)?;
        persist_rendered_thumbnail(&item.roll_id, &item.file_path, &new_thumbnail)?;
        item.rendered_thumbnail_base64 = Some(new_thumbnail);
        if item.effective_pipeline_state().contract != ProcessingContract::LegacyV1 {
            item.pristine_proxy = None;
        }
        Ok(())
    })
    .await
    .map_err(|error| format!("Thumbnail worker failed: {error}"))?
}

#[tauri::command]
pub async fn set_thumbnail_data(
    id: String,
    thumbnail: String,
    generation: u64,
    state: State<'_, EngineState>,
) -> Result<(), String> {
    let epoch = claim_development_generation(&state, &id, generation)?;
    let item_arc = state.items.get(&id).ok_or("Image ID not found")?.clone();
    tokio::task::spawn_blocking(move || {
        let mut item = write_lock(&item_arc);
        ensure_current_development_generation(&epoch, generation)?;
        persist_rendered_thumbnail(&item.roll_id, &item.file_path, &thumbnail)?;
        item.rendered_thumbnail_base64 = Some(thumbnail);
        Ok(())
    })
    .await
    .map_err(|error| format!("Thumbnail persistence worker failed: {error}"))?
}

#[tauri::command]
pub async fn clear_invalid_rendered_thumbnail(
    id: String,
    expected_thumbnail: String,
    state: State<'_, EngineState>,
) -> Result<(), String> {
    let item_arc = state.items.get(&id).ok_or("Image ID not found")?.clone();
    tokio::task::spawn_blocking(move || {
        let mut item = write_lock(&item_arc);
        if item.rendered_thumbnail_base64.as_deref() != Some(expected_thumbnail.as_str()) {
            return Ok(());
        }
        let connection = persistence::open_connection()
            .map_err(|error| format!("Failed to open image database: {error}"))?;
        let changed = connection
            .execute(
                "UPDATE image_states
                 SET rendered_thumb_base64 = NULL, thumbnail_base64 = embedded_thumb_base64,
                     updated_at = ?1
                 WHERE roll_id = ?2 AND file_path = ?3 AND rendered_thumb_base64 = ?4",
                rusqlite::params![
                    persistence::now_timestamp(),
                    item.roll_id,
                    item.file_path,
                    expected_thumbnail,
                ],
            )
            .map_err(|error| format!("Failed to clear invalid thumbnail: {error}"))?;
        if changed == 1 {
            item.rendered_thumbnail_base64 = None;
        }
        Ok(())
    })
    .await
    .map_err(|error| format!("Thumbnail cleanup worker failed: {error}"))?
}

#[tauri::command]
pub async fn update_geometry(
    id: String,
    geom: crate::app_state::GeometryState,
    generation: u64,
    state: State<'_, EngineState>,
) -> Result<(), String> {
    let epoch = claim_development_generation(&state, &id, generation)?;
    let item_arc = state.items.get(&id).ok_or("Image ID not found")?.clone();
    tokio::task::spawn_blocking(move || {
        let mut item = write_lock(&item_arc);
        ensure_current_development_generation(&epoch, generation)?;
        persist_geometry(&item.roll_id, &item.file_path, &geom)?;
        item.geom = geom;
        Ok(())
    })
    .await
    .map_err(|error| format!("Geometry persistence worker failed: {error}"))?
}

#[tauri::command(rename_all = "snake_case")]
pub async fn auto_detect_film_border(
    roll_id: String,
    file_path: String,
    state: State<'_, EngineState>,
) -> Result<crate::film_border::FilmBorderDetection, String> {
    let cache_key = format!("{}::{}", roll_id, normalize_path(&file_path));
    if let Some(cached) = state.film_border_cache.get(&cache_key) {
        return Ok(cached.clone());
    }

    let normalized_path = normalize_path(&file_path);
    let in_memory_thumbnail = state.items.iter().find_map(|entry| {
        let item = read_lock(entry.value());
        (item.roll_id == roll_id && normalize_path(&item.file_path) == normalized_path)
            .then(|| item.embedded_thumbnail_base64.clone())
    });

    let result = tokio::task::spawn_blocking(move || {
        let encoded = match in_memory_thumbnail {
            Some(encoded) => encoded,
            None => {
                let connection = persistence::open_connection()
                    .map_err(|error| format!("Failed to open state database: {error}"))?;
                connection
                    .query_row(
                        "SELECT COALESCE(embedded_thumb_base64, thumbnail_base64)
                         FROM image_states WHERE roll_id = ?1 AND file_path = ?2",
                        rusqlite::params![roll_id, file_path],
                        |row| row.get::<_, Option<String>>(0),
                    )
                    .optional()
                    .map_err(|error| format!("Failed to read cached thumbnail: {error}"))?
                    .flatten()
                    .ok_or_else(|| "CACHED_THUMBNAIL_NOT_FOUND".to_string())?
            }
        };
        detect_film_border_from_encoded(&encoded)
    })
    .await
    .map_err(|error| format!("Film-border worker failed: {error}"))??;

    state.film_border_cache.insert(cache_key, result.clone());
    Ok(result)
}

fn detect_film_border_from_encoded(
    encoded: &str,
) -> Result<crate::film_border::FilmBorderDetection, String> {
    let encoded = if encoded.starts_with("data:") {
        encoded
            .split_once(',')
            .map(|(_, payload)| payload)
            .ok_or_else(|| "Cached thumbnail data URL is invalid".to_string())?
    } else {
        encoded
    };
    let bytes = general_purpose::STANDARD
        .decode(encoded)
        .map_err(|error| format!("Cached thumbnail is not valid base64: {error}"))?;
    let thumbnail = image::load_from_memory(&bytes)
        .map_err(|error| format!("Cached thumbnail cannot be decoded: {error}"))?;
    Ok(crate::film_border::detect_film_border(&thumbnail))
}

fn normalize_persisted_geometry(mut geom: GeometryState) -> GeometryState {
    if !geom.calibration_confirmed {
        geom.calibration_points = None;
    }
    geom.lens_distortion = geom.lens_distortion.clamp(-100.0, 100.0);
    geom
}

fn normalize_persisted_geometry_for_rendered_image(
    mut geom: GeometryState,
    has_rendered_thumbnail: bool,
) -> GeometryState {
    // Older versions could persist a rendered positive without the newer
    // calibration_confirmed flag. Treat that state as a committed full-frame
    // calibration so reopening an already processed frame does not re-open the
    // film-area blocker.
    if has_rendered_thumbnail && !geom.calibration_confirmed {
        if geom.calibration_points.is_none() {
            geom.calibration_points = Some([[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]]);
        }
        geom.calibration_confirmed = true;
    }
    normalize_persisted_geometry(geom)
}

#[inline]
fn should_apply_sprocket_mask(crop_uv: [f32; 2], bounds: [f32; 4], sprocket_uv: [f32; 2]) -> bool {
    let [min_x, min_y, max_x, max_y] = bounds;
    let has_visible_border = min_x > 0.001 || min_y > 0.001 || max_x < 0.999 || max_y < 0.999;
    if has_visible_border {
        return crop_uv[0] < min_x
            || crop_uv[0] > max_x
            || crop_uv[1] < min_y
            || crop_uv[1] > max_y;
    }

    // A full-frame calibration has no explicit outside region. Restrict the
    // mask to the pair of edge bands nearest the sampled perforation instead
    // of allowing a matching midtone anywhere in the photograph to turn white.
    let horizontal_edge = sprocket_uv[0].min(1.0 - sprocket_uv[0]);
    let vertical_edge = sprocket_uv[1].min(1.0 - sprocket_uv[1]);
    if vertical_edge <= horizontal_edge {
        crop_uv[1].min(1.0 - crop_uv[1]) <= (vertical_edge * 1.75).clamp(0.08, 0.24)
    } else {
        crop_uv[0].min(1.0 - crop_uv[0]) <= (horizontal_edge * 1.75).clamp(0.08, 0.24)
    }
}

fn apply_batch_geometry_to_item(
    item_geometry: &mut GeometryState,
    geometry: &Value,
    modules: &[String],
) -> Result<(), String> {
    let copies_full_geometry = modules.iter().any(|module| module == "geometry");
    if copies_full_geometry {
        *item_geometry = serde_json::from_value(geometry.clone())
            .map_err(|error| format!("Invalid batch geometry payload: {error}"))?;
        return Ok(());
    }

    if modules.iter().any(|module| module == "film_area") {
        let points = geometry
            .get("calibration_points")
            .cloned()
            .ok_or_else(|| "Batch film-area payload has no calibration points".to_string())?;
        item_geometry.calibration_points = Some(
            serde_json::from_value(points)
                .map_err(|error| format!("Invalid batch film-area points: {error}"))?,
        );
        item_geometry.calibration_confirmed = geometry
            .get("calibration_confirmed")
            .and_then(Value::as_bool)
            .unwrap_or(false);
    }
    Ok(())
}

#[tauri::command]
pub async fn batch_copy_settings(
    source: ImageKey,
    targets: Vec<ImageKey>,
    modules: Vec<String>,
    state: State<'_, EngineState>,
    app_handle: tauri::AppHandle,
) -> Result<BatchCopyResult, String> {
    let commit = tokio::task::spawn_blocking(move || {
        let mut connection = persistence::open_connection()
            .map_err(|error| format!("Failed to open state database: {error}"))?;
        crate::batch_settings::copy_settings_transaction(
            &mut connection,
            &source,
            &targets,
            &modules,
            persistence::now_timestamp(),
        )
    })
    .await
    .map_err(|error| format!("Batch settings worker failed: {error}"))??;

    if let Some(geometry) = commit.geometry.as_ref() {
        let updated = commit
            .result
            .targets
            .iter()
            .cloned()
            .collect::<HashSet<_>>();
        for entry in state.items.iter() {
            let item_arc = entry.value().clone();
            let mut item = write_lock(&item_arc);
            let key = ImageKey {
                roll_id: item.roll_id.clone(),
                file_path: item.file_path.clone(),
            };
            if updated.contains(&key) {
                if let Err(error) =
                    apply_batch_geometry_to_item(&mut item.geom, geometry, &commit.result.modules)
                {
                    eprintln!("[Batch Settings] committed geometry cache refresh failed: {error}");
                }
            }
        }
    }

    if let Err(error) = app_handle.emit("settings_updated", &commit.result) {
        eprintln!("[Batch Settings] settings_updated broadcast failed: {error}");
    }
    Ok(commit.result)
}

/// Standalone geometry application — does NOT require a write lock on FilmItem.
fn compute_geometry_proxy(
    original_proxy: &ImageBuffer<Rgb<u16>, Vec<u16>>,
    geom: &crate::app_state::GeometryState,
) -> ImageBuffer<Rgb<u16>, Vec<u16>> {
    let mut current = original_proxy.clone();

    if geom.angle.abs() > 0.01 {
        let angle_rad = geom.angle.to_radians();
        let (w, h) = current.dimensions();

        let cos_a = angle_rad.cos();
        let sin_a = angle_rad.sin();

        let new_w = (w as f32 * cos_a.abs() + h as f32 * sin_a.abs()).ceil() as u32;
        let new_h = (w as f32 * sin_a.abs() + h as f32 * cos_a.abs()).ceil() as u32;

        let diag = ((w as f32).hypot(h as f32)).ceil() as u32;
        let mut expanded = ImageBuffer::from_pixel(diag, diag, image::Rgb([0, 0, 0]));
        let offset_x = (diag as i64 - w as i64) / 2;
        let offset_y = (diag as i64 - h as i64) / 2;
        image::imageops::overlay(&mut expanded, &current, offset_x, offset_y);

        let rotated = imageproc::geometric_transformations::rotate_about_center(
            &expanded,
            angle_rad,
            imageproc::geometric_transformations::Interpolation::Bicubic,
            image::Rgb([0, 0, 0]),
        );

        let crop_x = (diag.saturating_sub(new_w)) / 2;
        let crop_y = (diag.saturating_sub(new_h)) / 2;
        current = image::imageops::crop_imm(&rotated, crop_x, crop_y, new_w, new_h).to_image();
    }

    match geom.rotate_90_count.rem_euclid(4) {
        1 => current = image::imageops::rotate90(&current),
        2 => current = image::imageops::rotate180(&current),
        3 => current = image::imageops::rotate270(&current),
        _ => {}
    }

    if geom.flip_h {
        current = image::imageops::flip_horizontal(&current);
    }
    if geom.flip_v {
        current = image::imageops::flip_vertical(&current);
    }

    current
}

#[tauri::command]
pub async fn geometry_auto_align(
    id: String,
    state: State<'_, EngineState>,
) -> Result<crate::app_state::AutoAlignResult, String> {
    let item_arc = state.items.get(&id).ok_or("Image not found")?.clone();

    let (crop_rect, angle) = tokio::task::spawn_blocking(move || -> Result<_, String> {
        let original_proxy = {
            let item = read_lock(&item_arc);
            item.proxy_image
                .clone()
                .ok_or_else(|| "PROXY_NOT_READY".to_string())?
        };

        let first_result = crate::geometry::auto_crop_rect(&original_proxy)?;

        let proxy_image = {
            let item = read_lock(&item_arc);
            let mut geom = item.geom.clone();
            geom.angle = first_result.angle;
            compute_geometry_proxy(&original_proxy, &geom)
        };

        let second_result = crate::geometry::auto_crop_rect(&proxy_image)?;

        let mut item = write_lock(&item_arc);
        item.geom.angle = first_result.angle;
        item.geom.crop_rect = second_result.crop_rect.clone();

        Ok((item.geom.crop_rect.clone(), item.geom.angle))
    })
    .await
    .map_err(|e| e.to_string())??;

    Ok(crate::app_state::AutoAlignResult { crop_rect, angle })
}

pub fn get_proxy_response_buffer(state: &EngineState, id: &str) -> Result<Vec<u8>, String> {
    let out_buffer = {
        let item_arc = state.items.get(id).ok_or("Image ID not found")?;
        let item = read_lock(&item_arc);
        if let Some(proxy) = item.proxy_image.as_ref() {
            build_response_buffer_from_proxy_with_state(
                proxy,
                &item.base_color,
                item.effective_pipeline_state(),
                item.relative_transmission_quality.as_ref(),
                true,
            )
        } else {
            return Err("PROXY_NOT_READY".into());
        }
    };
    track_proxy_loaded(state, id);
    Ok(out_buffer)
}

#[tauri::command]
pub async fn update_tuning_parameters(
    id: String,
    mut params: TuningParams,
    roll_id: String,
    generation: u64,
    state: State<'_, EngineState>,
) -> Result<(), String> {
    let epoch = claim_development_generation(&state, &id, generation)?;
    params.raw_decode.working_colorspace = DENSITY_CAPTURE_WORKING_SPACE.to_string();
    let item_arc = state.items.get(&id).ok_or("Image ID not found")?.clone();
    tokio::task::spawn_blocking(move || {
        let mut item = write_lock(&item_arc);
        ensure_current_development_generation(&epoch, generation)?;
        if item.roll_id != roll_id {
            return Err("Image does not belong to the requested roll".into());
        }
        persist_tuning_parameters(&roll_id, &item.file_path, &params)?;
        if item.params.film_mode != params.film_mode {
            item.pristine_proxy = None;
        }
        item.params = params;
        Ok(())
    })
    .await
    .map_err(|error| format!("Tuning persistence worker failed: {error}"))?
}

fn gaussian_blur_rgb16_parallel(
    source: &ImageBuffer<Rgb<u16>, Vec<u16>>,
    sigma: f32,
) -> ImageBuffer<Rgb<u16>, Vec<u16>> {
    if !sigma.is_finite() || sigma <= 0.0 {
        return source.clone();
    }
    let (width, height) = source.dimensions();
    if width == 0 || height == 0 {
        return ImageBuffer::new(width, height);
    }

    let radius = (2.0 * sigma).ceil() as usize;
    let kernel_len = radius * 2 + 1;
    let sigma_squared = sigma.powi(2);
    let normalization = (2.0 * std::f32::consts::PI).sqrt() * sigma;
    let kernel = (0..kernel_len)
        .map(|index| {
            let distance = index as isize - radius as isize;
            (-(distance as f32).powi(2) / (2.0 * sigma_squared)).exp() / normalization
        })
        .collect::<Vec<_>>();
    let width = width as usize;
    let height = height as usize;
    let row_len = width * 3;
    let source = source.as_raw();

    let mut horizontal = vec![0u16; source.len()];
    horizontal
        .par_chunks_exact_mut(row_len)
        .enumerate()
        .for_each(|(row_index, output_row)| {
            let source_row = &source[row_index * row_len..(row_index + 1) * row_len];
            for x in 0..width {
                let interior = x >= radius && x + radius < width;
                let pixel_offset = x * 3;
                for channel in 0..3 {
                    let mut sum = 0.0;
                    for (kernel_index, weight) in kernel.iter().enumerate() {
                        let source_x = if interior {
                            x + kernel_index - radius
                        } else {
                            x.saturating_add(kernel_index)
                                .saturating_sub(radius)
                                .min(width - 1)
                        };
                        sum += source_row[source_x * 3 + channel] as f32 * weight;
                    }
                    output_row[pixel_offset + channel] = sum.clamp(0.0, 65535.0) as u16;
                }
            }
        });

    let mut vertical = vec![0u16; source.len()];
    vertical
        .par_chunks_exact_mut(row_len)
        .enumerate()
        .for_each(|(row_index, output_row)| {
            let interior = row_index >= radius && row_index + radius < height;
            for x in 0..width {
                let pixel_offset = x * 3;
                for channel in 0..3 {
                    let mut sum = 0.0;
                    for (kernel_index, weight) in kernel.iter().enumerate() {
                        let source_y = if interior {
                            row_index + kernel_index - radius
                        } else {
                            row_index
                                .saturating_add(kernel_index)
                                .saturating_sub(radius)
                                .min(height - 1)
                        };
                        sum +=
                            horizontal[source_y * row_len + pixel_offset + channel] as f32 * weight;
                    }
                    output_row[pixel_offset + channel] = sum.clamp(0.0, 65535.0) as u16;
                }
            }
        });

    ImageBuffer::from_raw(width as u32, height as u32, vertical)
        .unwrap_or_else(|| ImageBuffer::new(width as u32, height as u32))
}

fn apply_usm(buffer: &mut ImageBuffer<Rgb<u16>, Vec<u16>>, sigma: f32, amount: f32) {
    let blurred = gaussian_blur_rgb16_parallel(buffer, sigma);
    buffer
        .as_mut()
        .par_chunks_exact_mut(3)
        .zip(blurred.as_raw().par_chunks_exact(3))
        .for_each(|(pixel, blurred_pixel)| {
            for channel in 0..3 {
                let orig = pixel[channel] as f32;
                let blur = blurred_pixel[channel] as f32;
                pixel[channel] = (orig + (orig - blur) * amount).clamp(0.0, 65535.0) as u16;
            }
        });
}

#[inline]
fn sample_rgb16_nearest(image: &ImageBuffer<Rgb<u16>, Vec<u16>>, uv: [f32; 2]) -> Option<[u16; 3]> {
    if !uv[0].is_finite()
        || !uv[1].is_finite()
        || uv[0] < 0.0
        || uv[0] > 1.0
        || uv[1] < 0.0
        || uv[1] > 1.0
    {
        return None;
    }
    let (width, height) = image.dimensions();
    let x = (uv[0] * width as f32).floor().min((width - 1) as f32) as u32;
    let y = (uv[1] * height as f32).floor().min((height - 1) as f32) as u32;
    let pixel = image.get_pixel(x, y);
    Some([pixel[0], pixel[1], pixel[2]])
}

#[inline]
fn sample_rgb32_nearest(image: &ImageBuffer<Rgb<f32>, Vec<f32>>, uv: [f32; 2]) -> Option<[f32; 3]> {
    if !uv[0].is_finite()
        || !uv[1].is_finite()
        || uv[0] < 0.0
        || uv[0] > 1.0
        || uv[1] < 0.0
        || uv[1] > 1.0
    {
        return None;
    }
    let (width, height) = image.dimensions();
    let x = (uv[0] * width as f32).floor().min((width - 1) as f32) as u32;
    let y = (uv[1] * height as f32).floor().min((height - 1) as f32) as u32;
    let pixel = image.get_pixel(x, y);
    Some([pixel[0], pixel[1], pixel[2]])
}

#[inline]
fn sample_rgb32_nearest_checked(
    image: &ImageBuffer<Rgb<f32>, Vec<f32>>,
    quality: Option<&crate::raw_backend::QualityMask>,
    uv: [f32; 2],
) -> Option<[f32; 3]> {
    let pixel = sample_rgb32_nearest(image, uv)?;
    if let Some(quality) = quality {
        let (width, height) = image.dimensions();
        let x = (uv[0] * width as f32).floor().min((width - 1) as f32) as usize;
        let y = (uv[1] * height as f32).floor().min((height - 1) as f32) as usize;
        if !quality
            .valid
            .get(y * width as usize + x)
            .copied()
            .unwrap_or(false)
        {
            return None;
        }
    }
    Some(pixel)
}

/// Collect co-sited RGB samples through the same geometry map used by the
/// renderer. A film-area quadrilateral is a semantic region, not merely its
/// axis-aligned bounding box; this keeps lamp panels and sprocket regions out
/// of Smart Auto base estimation even when perspective correction is active.
fn collect_film_area_rgb32(
    proxy: &ImageBuffer<Rgb<f32>, Vec<f32>>,
    quality: Option<&crate::raw_backend::QualityMask>,
    geom: &GeometryState,
    reject_saturated: bool,
) -> Vec<[f32; 3]> {
    const SAMPLE_EDGE: u32 = 512;
    let (source_width, source_height) = proxy.dimensions();
    let longest = source_width.max(source_height).max(1);
    let sample_width = ((source_width as f64 / longest as f64) * SAMPLE_EDGE as f64)
        .round()
        .max(2.0) as u32;
    let sample_height = ((source_height as f64 / longest as f64) * SAMPLE_EDGE as f64)
        .round()
        .max(2.0) as u32;
    let points =
        geom.calibration_points
            .unwrap_or([[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]]);
    let min_x = points
        .iter()
        .map(|point| point[0])
        .fold(f32::INFINITY, f32::min);
    let max_x = points
        .iter()
        .map(|point| point[0])
        .fold(f32::NEG_INFINITY, f32::max);
    let min_y = points
        .iter()
        .map(|point| point[1])
        .fold(f32::INFINITY, f32::min);
    let max_y = points
        .iter()
        .map(|point| point[1])
        .fold(f32::NEG_INFINITY, f32::max);
    // Do not let nearest-neighbour samples exactly on the selected edge pick
    // up a one-pixel lamp-panel/sprocket fringe. The margin is sub-pixel on a
    // normal proxy and scales with the source resolution.
    let region_margin = 1.0 / source_width.max(source_height).max(1) as f32;
    let homography = shader_homography(points);
    let mut values = Vec::new();
    for y in 0..sample_height {
        for x in 0..sample_width {
            let base_uv = [
                x as f32 / (sample_width - 1) as f32,
                y as f32 / (sample_height - 1) as f32,
            ];
            let crop_uv = [
                geom.crop_rect.x + base_uv[0] * geom.crop_rect.width,
                geom.crop_rect.y + base_uv[1] * geom.crop_rect.height,
            ];
            if geom.calibration_points.is_some()
                && (crop_uv[0] < min_x + region_margin
                    || crop_uv[0] > max_x - region_margin
                    || crop_uv[1] < min_y + region_margin
                    || crop_uv[1] > max_y - region_margin)
            {
                continue;
            }
            let Some(perspective_uv) = apply_perspective_uv(
                crop_uv,
                geom.perspective_vertical,
                geom.perspective_horizontal,
                geom.perspective_aspect,
                geom.perspective_scale,
            ) else {
                continue;
            };
            let Some(oriented_uv) = apply_homography(&homography, perspective_uv) else {
                continue;
            };
            let Some(oriented_uv) = apply_lens_distortion_uv(oriented_uv, geom.lens_distortion)
            else {
                continue;
            };
            let source_uv =
                map_oriented_uv_to_source(oriented_uv, source_width, source_height, geom);
            let Some(pixel) = sample_rgb32_nearest_checked(proxy, quality, source_uv) else {
                continue;
            };
            if pixel.iter().any(|value| {
                !value.is_finite() || *value <= 0.0 || (reject_saturated && *value >= 0.995)
            }) {
                continue;
            }
            values.push(pixel);
        }
    }
    values
}

fn render_shader_equivalent_core(
    source_width: u32,
    source_height: u32,
    sample: impl Fn([f32; 2]) -> Option<[f32; 3]> + Sync,
    params: &TuningParams,
    geom: &crate::app_state::GeometryState,
    base_color: &BaseColor,
    pipeline_state: &PipelineState,
    lut: Option<&ParsedLut>,
) -> ImageBuffer<Rgb<u16>, Vec<u16>> {
    let crop = &geom.crop_rect;
    let output_width = (source_width as f32 * crop.width.clamp(0.0, 1.0))
        .round()
        .max(1.0) as u32;
    let output_height = (source_height as f32 * crop.height.clamp(0.0, 1.0))
        .round()
        .max(1.0) as u32;
    let points =
        geom.calibration_points
            .unwrap_or([[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]]);
    let homography = shader_homography(points);
    let min_x = points
        .iter()
        .map(|point| point[0])
        .fold(f32::INFINITY, f32::min);
    let max_x = points
        .iter()
        .map(|point| point[0])
        .fold(f32::NEG_INFINITY, f32::max);
    let min_y = points
        .iter()
        .map(|point| point[1])
        .fold(f32::INFINITY, f32::min);
    let max_y = points
        .iter()
        .map(|point| point[1])
        .fold(f32::NEG_INFINITY, f32::max);
    let sprocket_uv = params
        .sprocket
        .sprocket_uv
        .as_deref()
        .filter(|uv| uv.len() >= 2 && uv[0] >= 0.0)
        .map(|uv| [uv[0], uv[1]]);
    let sprocket_target = sprocket_uv.and_then(|uv| sample(uv));
    let tolerance = params.sprocket.sprocket_tolerance.unwrap_or(0.10);
    let feather = params.sprocket.sprocket_feather.unwrap_or(0.05);
    let lut_opacity = params.lut.lut_opacity.clamp(0.0, 1.0) * LUT_CONTROL_SCALE;
    let luma_coefficients = DENSITY_LUMA_COEFFICIENTS;
    let exposure_offsets = if params.film_mode == FilmMode::BW {
        [params.exposure.exposure; 3]
    } else {
        [
            params.exposure.exposure + params.exposure.exp_r * CHANNEL_CONTROL_SCALE,
            params.exposure.exposure + params.exposure.exp_g * CHANNEL_CONTROL_SCALE,
            params.exposure.exposure + params.exposure.exp_b * CHANNEL_CONTROL_SCALE,
        ]
    };
    let pipeline = FilmPipeline::from_state(
        pipeline_state,
        base_color,
        exposure_offsets,
        params.film_mode.clone(),
    );
    let positive_to_display = (pipeline_state.contract != ProcessingContract::LegacyV1
        && pipeline_state.contract != ProcessingContract::CaptureCorrectedV11
        && params.film_mode == FilmMode::Color)
        .then(|| linear_conversion_matrix(ColorSpaceId::ProPhotoRgb, ColorSpaceId::SRgb));
    let (bw_dmin, bw_dmax) = neutral_density_bounds(params.density.d_min, params.density.d_max);

    let mut output = ImageBuffer::<Rgb<u16>, Vec<u16>>::new(output_width, output_height);
    output
        .as_mut()
        .par_chunks_exact_mut(3)
        .enumerate()
        .for_each(|(index, out_pixel)| {
            let x = (index % output_width as usize) as u32;
            let y = (index / output_width as usize) as u32;
            let crop_uv = [
                crop.x + (x as f32 + 0.5) / output_width as f32 * crop.width,
                crop.y + (y as f32 + 0.5) / output_height as f32 * crop.height,
            ];
            let Some(perspective_uv) = apply_perspective_uv(
                crop_uv,
                geom.perspective_vertical,
                geom.perspective_horizontal,
                geom.perspective_aspect,
                geom.perspective_scale,
            ) else {
                return;
            };
            let Some(warped_uv) = apply_homography(&homography, perspective_uv) else {
                return;
            };
            let Some(warped_uv) = apply_lens_distortion_uv(warped_uv, geom.lens_distortion) else {
                return;
            };
            let Some(raw) = sample(warped_uv) else {
                return;
            };
            let density = if pipeline_state.contract == ProcessingContract::CaptureCorrectedV11 {
                let Some(true_density) = pipeline.compute_relative_density(&raw, true) else {
                    return;
                };
                pipeline.apply_exposure(&true_density)
            } else {
                pipeline.process_pixel(&raw)
            };
            let (d_min, d_max) = if params.film_mode == FilmMode::BW {
                ([bw_dmin; 3], [bw_dmax; 3])
            } else {
                (params.density.d_min, params.density.d_max)
            };
            let working_gamma = if positive_to_display.is_some() {
                1.0
            } else {
                params.density.gamma
            };
            let normalized_working = [
                normalize_density_channel(density[0], d_min[0], d_max[0], 0.0, 0.0, working_gamma),
                normalize_density_channel(density[1], d_min[1], d_max[1], 0.0, 0.0, working_gamma),
                normalize_density_channel(density[2], d_min[2], d_max[2], 0.0, 0.0, working_gamma),
            ];
            let normalized = positive_to_display
                .map(|matrix| {
                    apply_linear_matrix(normalized_working, matrix).map(|value| {
                        value
                            .clamp(0.0, 1.0)
                            .powf(1.0 / params.density.gamma.max(1e-6))
                    })
                })
                .unwrap_or(normalized_working);
            let (saturation, temperature, tint) = if params.film_mode == FilmMode::Color {
                (
                    params.tone.saturation,
                    params.tone.temperature,
                    params.tone.tint,
                )
            } else {
                (0.0, 0.0, 0.0)
            };
            let baseline = apply_post_gamma_adjustments_with_luma(
                normalized,
                params.tone.highlights,
                params.tone.shadows,
                saturation,
                temperature,
                tint,
                luma_coefficients,
            );
            let mut rendered = if let Some(lut) = lut {
                let mapped = lut.sample(baseline);
                [
                    baseline[0] + (mapped[0] - baseline[0]) * lut_opacity,
                    baseline[1] + (mapped[1] - baseline[1]) * lut_opacity,
                    baseline[2] + (mapped[2] - baseline[2]) * lut_opacity,
                ]
            } else {
                baseline
            };
            if params.film_mode == FilmMode::BW {
                let luma = rendered
                    .iter()
                    .zip(luma_coefficients)
                    .map(|(value, coefficient)| value * coefficient)
                    .sum();
                rendered = [luma; 3];
            }

            if let Some(target) = sprocket_target {
                let raw_luma = raw
                    .iter()
                    .zip(luma_coefficients)
                    .map(|(value, coefficient)| value * coefficient)
                    .sum::<f32>();
                let target_luma = target
                    .iter()
                    .zip(luma_coefficients)
                    .map(|(value, coefficient)| value * coefficient)
                    .sum::<f32>();
                if should_apply_sprocket_mask(
                    crop_uv,
                    [min_x, min_y, max_x, max_y],
                    sprocket_uv.expect("sprocket target requires a sample point"),
                ) {
                    let mask = sprocket_white_mask(raw_luma - target_luma, tolerance, feather);
                    for channel in &mut rendered {
                        *channel += (1.0 - *channel) * mask;
                    }
                }
            }

            for channel in 0..3 {
                out_pixel[channel] = (rendered[channel] * 65535.0).clamp(0.0, 65535.0) as u16;
            }
        });
    output
}

fn render_shader_equivalent(
    source: &ImageBuffer<Rgb<u16>, Vec<u16>>,
    params: &TuningParams,
    geom: &crate::app_state::GeometryState,
    base_color: &BaseColor,
    lut: Option<&ParsedLut>,
) -> ImageBuffer<Rgb<u16>, Vec<u16>> {
    let state = PipelineState::default();
    render_shader_equivalent_core(
        source.width(),
        source.height(),
        |uv| {
            sample_rgb16_nearest(source, uv).map(|pixel| {
                [
                    pixel[0] as f32 / 65535.0,
                    pixel[1] as f32 / 65535.0,
                    pixel[2] as f32 / 65535.0,
                ]
            })
        },
        params,
        geom,
        base_color,
        &state,
        lut,
    )
}

fn render_shader_equivalent_with_state(
    source: &ImageBuffer<Rgb<u16>, Vec<u16>>,
    params: &TuningParams,
    geom: &crate::app_state::GeometryState,
    base_color: &BaseColor,
    pipeline_state: &PipelineState,
    lut: Option<&ParsedLut>,
) -> ImageBuffer<Rgb<u16>, Vec<u16>> {
    render_shader_equivalent_core(
        source.width(),
        source.height(),
        |uv| {
            sample_rgb16_nearest(source, uv).map(|pixel| {
                [
                    pixel[0] as f32 / 65535.0,
                    pixel[1] as f32 / 65535.0,
                    pixel[2] as f32 / 65535.0,
                ]
            })
        },
        params,
        geom,
        base_color,
        pipeline_state,
        lut,
    )
}

fn render_f32_shader_equivalent(
    source: &ImageBuffer<Rgb<f32>, Vec<f32>>,
    quality: Option<&crate::raw_backend::QualityMask>,
    params: &TuningParams,
    geom: &crate::app_state::GeometryState,
    base_color: &BaseColor,
    pipeline_state: &PipelineState,
    lut: Option<&ParsedLut>,
) -> ImageBuffer<Rgb<u16>, Vec<u16>> {
    render_shader_equivalent_core(
        source.width(),
        source.height(),
        |uv| sample_rgb32_nearest_checked(source, quality, uv),
        params,
        geom,
        base_color,
        pipeline_state,
        lut,
    )
}

#[derive(Clone)]
struct ExportItemSnapshot {
    id: String,
    file_path: String,
    roll_id: String,
    params: TuningParams,
    geom: GeometryState,
    base_color: BaseColor,
    pipeline_state: PipelineState,
    resolver_input: PipelineResolverInput,
    capture_profile: Option<CalibrationConfigProfile>,
    scanner_profile: Option<crate::scanner_profile::ScannerInputProfile>,
    output_path: std::path::PathBuf,
    export_metadata: Option<ExportMetadata>,
}

#[derive(Clone, Debug)]
struct ExportMetadata {
    roll_id: String,
    film_stock: String,
    camera: String,
    date: String,
}

impl From<&Roll> for ExportMetadata {
    fn from(roll: &Roll) -> Self {
        Self {
            roll_id: roll.roll_id.clone(),
            film_stock: roll.film_stock.clone(),
            camera: roll.camera.clone(),
            date: roll.date.clone(),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ExportFormat {
    Jpeg,
    Png,
    Tiff8,
    Tiff16,
}

fn export_profile_for_output(format: ExportFormat, output_space: ColorSpaceId) -> Option<Vec<u8>> {
    // JPEG viewers universally treat an untagged JPEG as sRGB. Avoid attaching
    // a generated matrix profile to standard sRGB JPEGs so their appearance
    // matches the browser canvas and the operating system's native sRGB path.
    if format == ExportFormat::Jpeg && output_space == ColorSpaceId::SRgb {
        None
    } else {
        Some(crate::color_science::build_icc_profile(output_space))
    }
}

impl ExportFormat {
    fn parse(value: &str) -> Result<Self, String> {
        match value {
            "jpeg" | "jpeg100" => Ok(Self::Jpeg),
            "png" => Ok(Self::Png),
            "tiff8" => Ok(Self::Tiff8),
            "tiff16" | "tiff16_uncompressed" => Ok(Self::Tiff16),
            other => Err(format!("Unsupported export format: {other}")),
        }
    }

    fn extension(self) -> &'static str {
        match self {
            Self::Jpeg => "jpg",
            Self::Png => "png",
            Self::Tiff8 | Self::Tiff16 => "tiff",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ExportConflictPolicy {
    Unique,
    Overwrite,
    Skip,
}

impl ExportConflictPolicy {
    fn parse(value: &str) -> Result<Self, String> {
        match value {
            "unique" => Ok(Self::Unique),
            "overwrite" => Ok(Self::Overwrite),
            "skip" => Ok(Self::Skip),
            other => Err(format!("Unsupported file conflict policy: {other}")),
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BatchExportResult {
    exported: usize,
    skipped: usize,
    failed: usize,
    output_dir: String,
    errors: Vec<String>,
    warnings: Vec<String>,
}

fn export_sharpening(value: &str) -> Result<Option<(f32, f32)>, String> {
    match value {
        "none" => Ok(None),
        "low" => Ok(Some((0.8, 0.25))),
        "standard" => Ok(Some((1.0, 0.5))),
        "high" => Ok(Some((1.2, 0.8))),
        other => Err(format!("Unsupported output sharpening preset: {other}")),
    }
}

fn export_dimensions(
    width: u32,
    height: u32,
    resize_mode: &str,
    long_edge: u32,
    allow_upscale: bool,
) -> Result<(u32, u32), String> {
    match resize_mode {
        "original" => Ok((width, height)),
        "long_edge" => {
            if !(256..=32768).contains(&long_edge) {
                return Err("Long edge must be between 256 and 32768 pixels".to_string());
            }
            let source_edge = width.max(height);
            if source_edge == 0 || (!allow_upscale && source_edge <= long_edge) {
                return Ok((width, height));
            }
            let scale = long_edge as f64 / source_edge as f64;
            Ok((
                ((width as f64 * scale).round() as u32).max(1),
                ((height as f64 * scale).round() as u32).max(1),
            ))
        }
        other => Err(format!("Unsupported resize mode: {other}")),
    }
}

fn sanitize_export_file_stem(value: &str) -> String {
    let replaced: String = value
        .chars()
        .map(|character| {
            if character.is_control()
                || matches!(
                    character,
                    '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*'
                )
            {
                '_'
            } else {
                character
            }
        })
        .collect();
    let collapsed = replaced.split_whitespace().collect::<Vec<_>>().join(" ");
    let trimmed = collapsed.trim().trim_end_matches(['.', ' ']);
    let mut stem: String = if trimmed.is_empty() {
        "Export".to_string()
    } else {
        trimmed.chars().take(180).collect()
    };
    let base = stem
        .split('.')
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase();
    let reserved = matches!(base.as_str(), "con" | "prn" | "aux" | "nul")
        || (base.len() == 4
            && (base.starts_with("com") || base.starts_with("lpt"))
            && matches!(base.as_bytes()[3], b'1'..=b'9'));
    if reserved {
        stem.insert(0, '_');
    }
    stem
}

fn render_export_name(
    template: &str,
    snapshot: &ExportItemSnapshot,
    roll: Option<&Roll>,
    sequence: usize,
) -> Result<String, String> {
    if template.trim().is_empty() {
        return Err("Filename template cannot be empty".to_string());
    }
    let original = std::path::Path::new(&snapshot.file_path)
        .file_stem()
        .unwrap_or_default()
        .to_string_lossy();
    let mut rendered = template.to_string();
    let sequence = format!("{sequence:03}");
    for (token, value) in [
        (
            "{Roll}",
            roll.map(|value| value.roll_id.as_str()).unwrap_or("Roll"),
        ),
        (
            "{Camera}",
            roll.map(|value| value.camera.as_str()).unwrap_or("Camera"),
        ),
        (
            "{Film}",
            roll.map(|value| value.film_stock.as_str())
                .unwrap_or("Film"),
        ),
        (
            "{Date}",
            roll.map(|value| value.date.as_str()).unwrap_or("Undated"),
        ),
        ("{Original}", original.as_ref()),
        ("{Seq}", sequence.as_str()),
    ] {
        rendered = rendered.replace(token, value);
    }
    if rendered.contains('{') || rendered.contains('}') {
        return Err(format!(
            "Filename template contains an unknown token: {template}"
        ));
    }
    Ok(sanitize_export_file_stem(&rendered))
}

fn reserve_export_path(
    output_dir: &std::path::Path,
    stem: &str,
    extension: &str,
    policy: ExportConflictPolicy,
    reserved: &mut HashSet<String>,
) -> Option<std::path::PathBuf> {
    let base = output_dir.join(format!("{stem}.{extension}"));
    let base_key = normalize_path(base.to_string_lossy().as_ref());
    let batch_collision = reserved.contains(&base_key);
    if batch_collision && policy == ExportConflictPolicy::Skip {
        return None;
    }
    if !batch_collision {
        match policy {
            ExportConflictPolicy::Overwrite => {
                reserved.insert(base_key);
                return Some(base);
            }
            ExportConflictPolicy::Skip if base.exists() => return None,
            ExportConflictPolicy::Skip | ExportConflictPolicy::Unique if !base.exists() => {
                reserved.insert(base_key);
                return Some(base);
            }
            ExportConflictPolicy::Unique => {}
            ExportConflictPolicy::Skip => return None,
        }
    }

    for suffix in 2..=100_000 {
        let candidate = output_dir.join(format!("{stem} ({suffix}).{extension}"));
        let key = normalize_path(candidate.to_string_lossy().as_ref());
        if !candidate.exists() && !reserved.contains(&key) {
            reserved.insert(key);
            return Some(candidate);
        }
    }
    None
}

#[derive(Clone, Copy)]
enum TiffByteOrder {
    Little,
    Big,
}

impl TiffByteOrder {
    fn read_u16(self, bytes: &[u8]) -> u16 {
        match self {
            Self::Little => u16::from_le_bytes([bytes[0], bytes[1]]),
            Self::Big => u16::from_be_bytes([bytes[0], bytes[1]]),
        }
    }

    fn read_u32(self, bytes: &[u8]) -> u32 {
        match self {
            Self::Little => u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]),
            Self::Big => u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]),
        }
    }

    fn push_u16(self, output: &mut Vec<u8>, value: u16) {
        output.extend_from_slice(&match self {
            Self::Little => value.to_le_bytes(),
            Self::Big => value.to_be_bytes(),
        });
    }

    fn push_u32(self, output: &mut Vec<u8>, value: u32) {
        output.extend_from_slice(&match self {
            Self::Little => value.to_le_bytes(),
            Self::Big => value.to_be_bytes(),
        });
    }

    fn write_u32(self, output: &mut [u8], value: u32) {
        output.copy_from_slice(&match self {
            Self::Little => value.to_le_bytes(),
            Self::Big => value.to_be_bytes(),
        });
    }
}

enum ExportTiffEntry {
    Raw {
        tag: u16,
        bytes: [u8; 12],
    },
    Ascii {
        tag: u16,
        bytes: Vec<u8>,
    },
    Long {
        tag: u16,
        value: u32,
    },
    Binary {
        tag: u16,
        type_code: u16,
        bytes: Vec<u8>,
    },
}

impl ExportTiffEntry {
    fn tag(&self) -> u16 {
        match self {
            Self::Raw { tag, .. }
            | Self::Ascii { tag, .. }
            | Self::Long { tag, .. }
            | Self::Binary { tag, .. } => *tag,
        }
    }
}

fn exif_ascii(value: &str) -> Vec<u8> {
    let mut bytes = value.replace('\0', " ").into_bytes();
    bytes.push(0);
    bytes
}

fn exif_datetime(value: &str) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return None;
    }
    let normalized = trimmed.replace('-', ":");
    Some(if normalized.len() == 10 {
        format!("{normalized} 00:00:00")
    } else {
        normalized.chars().take(19).collect()
    })
}

fn build_metadata_ifd(
    order: TiffByteOrder,
    ifd_offset: u32,
    original_entries: Vec<[u8; 12]>,
    next_ifd: u32,
    metadata: &ExportMetadata,
    icc_profile: Option<&[u8]>,
) -> Result<Vec<u8>, String> {
    let mut entries = original_entries
        .into_iter()
        .map(|bytes| ExportTiffEntry::Raw {
            tag: order.read_u16(&bytes[0..2]),
            bytes,
        })
        .collect::<Vec<_>>();
    let description = [
        (!metadata.roll_id.trim().is_empty()).then(|| format!("Roll: {}", metadata.roll_id.trim())),
        (!metadata.film_stock.trim().is_empty())
            .then(|| format!("Film: {}", metadata.film_stock.trim())),
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>()
    .join("; ");
    if !description.is_empty() {
        entries.push(ExportTiffEntry::Ascii {
            tag: 0x010e,
            bytes: exif_ascii(&description),
        });
    }
    if !metadata.camera.trim().is_empty() {
        entries.push(ExportTiffEntry::Ascii {
            tag: 0x0110,
            bytes: exif_ascii(metadata.camera.trim()),
        });
    }
    entries.push(ExportTiffEntry::Ascii {
        tag: 0x0131,
        bytes: exif_ascii("NexFilm"),
    });
    let date_time = exif_datetime(&metadata.date);
    if let Some(value) = date_time.as_deref() {
        entries.push(ExportTiffEntry::Ascii {
            tag: 0x0132,
            bytes: exif_ascii(value),
        });
    }
    if let Some(profile) = icc_profile {
        entries.push(ExportTiffEntry::Binary {
            tag: 34675,
            type_code: 7,
            bytes: profile.to_vec(),
        });
    }

    let ifd_size = 2usize
        .checked_add(
            entries
                .len()
                .saturating_add(usize::from(date_time.is_some()))
                * 12,
        )
        .and_then(|size| size.checked_add(4))
        .ok_or_else(|| "EXIF metadata is too large".to_string())?;
    let exif_ifd_offset = ifd_offset
        .checked_add(u32::try_from(ifd_size).map_err(|_| "EXIF metadata is too large")?)
        .ok_or_else(|| "EXIF metadata offset overflow".to_string())?;
    if date_time.is_some() {
        entries.push(ExportTiffEntry::Long {
            tag: 0x8769,
            value: exif_ifd_offset,
        });
    }
    entries.sort_by_key(ExportTiffEntry::tag);

    let exif_ifd_size = if date_time.is_some() { 18u32 } else { 0 };
    let mut external_offset = exif_ifd_offset
        .checked_add(exif_ifd_size)
        .ok_or_else(|| "EXIF metadata offset overflow".to_string())?;
    let mut external_data = Vec::new();
    let mut output = Vec::new();
    order.push_u16(
        &mut output,
        u16::try_from(entries.len()).map_err(|_| "Too many TIFF entries")?,
    );
    for entry in entries {
        match entry {
            ExportTiffEntry::Raw { bytes, .. } => output.extend_from_slice(&bytes),
            ExportTiffEntry::Long { tag, value } => {
                order.push_u16(&mut output, tag);
                order.push_u16(&mut output, 4);
                order.push_u32(&mut output, 1);
                order.push_u32(&mut output, value);
            }
            ExportTiffEntry::Ascii { tag, bytes } => {
                order.push_u16(&mut output, tag);
                order.push_u16(&mut output, 2);
                order.push_u32(
                    &mut output,
                    u32::try_from(bytes.len()).map_err(|_| "EXIF text is too large")?,
                );
                if bytes.len() <= 4 {
                    output.extend_from_slice(&bytes);
                    output.resize(output.len() + 4 - bytes.len(), 0);
                } else {
                    order.push_u32(&mut output, external_offset);
                    external_data.extend_from_slice(&bytes);
                    if external_data.len() % 2 != 0 {
                        external_data.push(0);
                    }
                    external_offset = exif_ifd_offset
                        .checked_add(exif_ifd_size)
                        .and_then(|offset| {
                            offset.checked_add(u32::try_from(external_data.len()).ok()?)
                        })
                        .ok_or_else(|| "EXIF metadata offset overflow".to_string())?;
                }
            }
            ExportTiffEntry::Binary {
                tag,
                type_code,
                bytes,
            } => {
                order.push_u16(&mut output, tag);
                order.push_u16(&mut output, type_code);
                order.push_u32(
                    &mut output,
                    u32::try_from(bytes.len()).map_err(|_| "ICC profile is too large")?,
                );
                order.push_u32(&mut output, external_offset);
                external_data.extend_from_slice(&bytes);
                if external_data.len() % 2 != 0 {
                    external_data.push(0);
                }
                external_offset = exif_ifd_offset
                    .checked_add(exif_ifd_size)
                    .and_then(|offset| offset.checked_add(u32::try_from(external_data.len()).ok()?))
                    .ok_or_else(|| "ICC profile offset overflow".to_string())?;
            }
        }
    }
    order.push_u32(&mut output, next_ifd);

    if let Some(value) = date_time.as_deref() {
        let date_bytes = exif_ascii(value);
        order.push_u16(&mut output, 1);
        order.push_u16(&mut output, 0x9003);
        order.push_u16(&mut output, 2);
        order.push_u32(
            &mut output,
            u32::try_from(date_bytes.len()).map_err(|_| "EXIF date is too large")?,
        );
        order.push_u32(&mut output, external_offset);
        order.push_u32(&mut output, 0);
        output.extend_from_slice(&external_data);
        output.extend_from_slice(&date_bytes);
    } else {
        output.extend_from_slice(&external_data);
    }
    Ok(output)
}

fn build_exif_tiff(metadata: &ExportMetadata) -> Result<Vec<u8>, String> {
    let order = TiffByteOrder::Little;
    let mut output = b"II".to_vec();
    order.push_u16(&mut output, 42);
    order.push_u32(&mut output, 8);
    output.extend_from_slice(&build_metadata_ifd(
        order,
        8,
        Vec::new(),
        0,
        metadata,
        None,
    )?);
    Ok(output)
}

fn insert_jpeg_exif(encoded: &mut Vec<u8>, metadata: &ExportMetadata) -> Result<(), String> {
    if !encoded.starts_with(&[0xff, 0xd8]) {
        return Err("JPEG encoder returned invalid data".to_string());
    }
    let mut payload = b"Exif\0\0".to_vec();
    payload.extend_from_slice(&build_exif_tiff(metadata)?);
    let segment_length = payload
        .len()
        .checked_add(2)
        .and_then(|length| u16::try_from(length).ok())
        .ok_or_else(|| "EXIF metadata exceeds the JPEG segment limit".to_string())?;
    let mut segment = vec![0xff, 0xe1];
    segment.extend_from_slice(&segment_length.to_be_bytes());
    segment.extend_from_slice(&payload);
    encoded.splice(2..2, segment);
    Ok(())
}

fn insert_jpeg_icc(encoded: &mut Vec<u8>, profile: &[u8]) -> Result<(), String> {
    if !encoded.starts_with(&[0xff, 0xd8]) {
        return Err("JPEG encoder returned invalid data".to_string());
    }
    let mut payload = b"ICC_PROFILE\0".to_vec();
    payload.extend_from_slice(&[1, 1]);
    payload.extend_from_slice(profile);
    let segment_length = payload
        .len()
        .checked_add(2)
        .and_then(|length| u16::try_from(length).ok())
        .ok_or_else(|| "ICC profile exceeds the JPEG segment limit".to_string())?;
    let mut segment = vec![0xff, 0xe2];
    segment.extend_from_slice(&segment_length.to_be_bytes());
    segment.extend_from_slice(&payload);
    encoded.splice(2..2, segment);
    Ok(())
}

fn png_crc32(bytes: &[u8]) -> u32 {
    let mut crc = 0xffff_ffffu32;
    for byte in bytes {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            crc = (crc >> 1) ^ (0xedb8_8320u32 & (0u32.wrapping_sub(crc & 1)));
        }
    }
    !crc
}

fn insert_png_exif(encoded: &mut Vec<u8>, metadata: &ExportMetadata) -> Result<(), String> {
    if !encoded.starts_with(b"\x89PNG\r\n\x1a\n") {
        return Err("PNG encoder returned invalid data".to_string());
    }
    let mut offset = 8usize;
    while offset
        .checked_add(12)
        .is_some_and(|end| end <= encoded.len())
    {
        let length = u32::from_be_bytes(
            encoded[offset..offset + 4]
                .try_into()
                .map_err(|_| "Invalid PNG chunk")?,
        ) as usize;
        let chunk_end = offset
            .checked_add(12)
            .and_then(|value| value.checked_add(length))
            .ok_or_else(|| "Invalid PNG chunk length".to_string())?;
        if chunk_end > encoded.len() {
            break;
        }
        if &encoded[offset + 4..offset + 8] == b"IEND" {
            let payload = build_exif_tiff(metadata)?;
            let mut chunk = Vec::with_capacity(payload.len() + 12);
            chunk.extend_from_slice(
                &u32::try_from(payload.len())
                    .map_err(|_| "EXIF metadata is too large")?
                    .to_be_bytes(),
            );
            chunk.extend_from_slice(b"eXIf");
            chunk.extend_from_slice(&payload);
            let mut crc_input = b"eXIf".to_vec();
            crc_input.extend_from_slice(&payload);
            chunk.extend_from_slice(&png_crc32(&crc_input).to_be_bytes());
            encoded.splice(offset..offset, chunk);
            return Ok(());
        }
        offset = chunk_end;
    }
    Err("PNG output does not contain an IEND chunk".to_string())
}

fn insert_png_icc(encoded: &mut Vec<u8>, profile: &[u8]) -> Result<(), String> {
    if !encoded.starts_with(b"\x89PNG\r\n\x1a\n") {
        return Err("PNG encoder returned invalid data".to_string());
    }
    let mut compressed = ZlibEncoder::new(Vec::new(), Compression::default());
    compressed
        .write_all(profile)
        .map_err(|error| format!("ICC profile compression failed: {error}"))?;
    let compressed = compressed
        .finish()
        .map_err(|error| format!("ICC profile compression failed: {error}"))?;
    let mut payload = b"NexFilm\0".to_vec();
    payload.push(0);
    payload.extend_from_slice(&compressed);
    let mut offset = 8usize;
    while offset
        .checked_add(12)
        .is_some_and(|end| end <= encoded.len())
    {
        let length = u32::from_be_bytes(
            encoded[offset..offset + 4]
                .try_into()
                .map_err(|_| "Invalid PNG chunk")?,
        ) as usize;
        let chunk_end = offset
            .checked_add(12)
            .and_then(|value| value.checked_add(length))
            .ok_or_else(|| "Invalid PNG chunk length".to_string())?;
        if chunk_end > encoded.len() {
            break;
        }
        if &encoded[offset + 4..offset + 8] == b"IEND" {
            let mut chunk = Vec::with_capacity(payload.len() + 12);
            chunk.extend_from_slice(&(payload.len() as u32).to_be_bytes());
            chunk.extend_from_slice(b"iCCP");
            chunk.extend_from_slice(&payload);
            let mut crc_input = b"iCCP".to_vec();
            crc_input.extend_from_slice(&payload);
            chunk.extend_from_slice(&png_crc32(&crc_input).to_be_bytes());
            encoded.splice(offset..offset, chunk);
            return Ok(());
        }
        offset = chunk_end;
    }
    Err("PNG output does not contain an IEND chunk".to_string())
}

fn insert_tiff_exif(encoded: &mut Vec<u8>, metadata: &ExportMetadata) -> Result<(), String> {
    if encoded.len() < 8 {
        return Err("TIFF encoder returned invalid data".to_string());
    }
    let order = match &encoded[0..2] {
        b"II" => TiffByteOrder::Little,
        b"MM" => TiffByteOrder::Big,
        _ => return Err("TIFF encoder returned invalid byte order".to_string()),
    };
    if order.read_u16(&encoded[2..4]) != 42 {
        return Err("TIFF encoder returned an unsupported header".to_string());
    }
    let original_ifd = order.read_u32(&encoded[4..8]) as usize;
    if original_ifd
        .checked_add(2)
        .is_none_or(|end| end > encoded.len())
    {
        return Err("TIFF output has an invalid image directory".to_string());
    }
    let entry_count = order.read_u16(&encoded[original_ifd..original_ifd + 2]) as usize;
    let entries_start = original_ifd + 2;
    let entries_end = entries_start
        .checked_add(entry_count.saturating_mul(12))
        .ok_or_else(|| "TIFF directory is too large".to_string())?;
    if entries_end
        .checked_add(4)
        .is_none_or(|end| end > encoded.len())
    {
        return Err("TIFF output has a truncated image directory".to_string());
    }
    let replaced_tags = [0x010e, 0x0110, 0x0131, 0x0132, 0x8769];
    let mut original_entries = Vec::new();
    for entry in encoded[entries_start..entries_end].chunks_exact(12) {
        if !replaced_tags.contains(&order.read_u16(&entry[0..2])) {
            original_entries.push(entry.try_into().map_err(|_| "Invalid TIFF entry")?);
        }
    }
    let next_ifd = order.read_u32(&encoded[entries_end..entries_end + 4]);
    if encoded.len() % 2 != 0 {
        encoded.push(0);
    }
    let new_ifd = u32::try_from(encoded.len()).map_err(|_| "TIFF output is too large")?;
    let directory = build_metadata_ifd(order, new_ifd, original_entries, next_ifd, metadata, None)?;
    encoded.extend_from_slice(&directory);
    order.write_u32(&mut encoded[4..8], new_ifd);
    Ok(())
}

fn insert_tiff_icc(encoded: &mut Vec<u8>, profile: &[u8]) -> Result<(), String> {
    if encoded.len() < 8 {
        return Err("TIFF encoder returned invalid data".to_string());
    }
    let order = match &encoded[0..2] {
        b"II" => TiffByteOrder::Little,
        b"MM" => TiffByteOrder::Big,
        _ => return Err("TIFF encoder returned invalid byte order".to_string()),
    };
    if order.read_u16(&encoded[2..4]) != 42 {
        return Err("TIFF encoder returned an unsupported header".to_string());
    }
    let original_ifd = order.read_u32(&encoded[4..8]) as usize;
    if original_ifd
        .checked_add(2)
        .is_none_or(|end| end > encoded.len())
    {
        return Err("TIFF output has an invalid image directory".to_string());
    }
    let entry_count = order.read_u16(&encoded[original_ifd..original_ifd + 2]) as usize;
    let entries_start = original_ifd + 2;
    let entries_end = entries_start
        .checked_add(entry_count.saturating_mul(12))
        .ok_or_else(|| "TIFF directory is too large".to_string())?;
    if entries_end
        .checked_add(4)
        .is_none_or(|end| end > encoded.len())
    {
        return Err("TIFF output has a truncated image directory".to_string());
    }
    let mut original_entries = Vec::new();
    for entry in encoded[entries_start..entries_end].chunks_exact(12) {
        if order.read_u16(&entry[0..2]) != 34675 {
            original_entries.push(entry.try_into().map_err(|_| "Invalid TIFF entry")?);
        }
    }
    let next_ifd = order.read_u32(&encoded[entries_end..entries_end + 4]);
    if encoded.len() % 2 != 0 {
        encoded.push(0);
    }
    let new_ifd = u32::try_from(encoded.len()).map_err(|_| "TIFF output is too large")?;
    let empty_metadata = ExportMetadata {
        roll_id: String::new(),
        film_stock: String::new(),
        camera: String::new(),
        date: String::new(),
    };
    let directory = build_metadata_ifd(
        order,
        new_ifd,
        original_entries,
        next_ifd,
        &empty_metadata,
        Some(profile),
    )?;
    encoded.extend_from_slice(&directory);
    order.write_u32(&mut encoded[4..8], new_ifd);
    Ok(())
}

fn attach_export_profile(
    encoded: &mut Vec<u8>,
    format: ExportFormat,
    profile: &[u8],
) -> Result<(), String> {
    match format {
        ExportFormat::Jpeg => insert_jpeg_icc(encoded, profile),
        ExportFormat::Png => insert_png_icc(encoded, profile),
        ExportFormat::Tiff8 | ExportFormat::Tiff16 => insert_tiff_icc(encoded, profile),
    }
}

fn attach_export_metadata(
    encoded: &mut Vec<u8>,
    format: ExportFormat,
    metadata: &ExportMetadata,
) -> Result<(), String> {
    match format {
        ExportFormat::Jpeg => insert_jpeg_exif(encoded, metadata),
        ExportFormat::Png => insert_png_exif(encoded, metadata),
        ExportFormat::Tiff8 | ExportFormat::Tiff16 => insert_tiff_exif(encoded, metadata),
    }
}

#[allow(dead_code)]
fn write_export_image(
    buffer: ImageBuffer<Rgb<u16>, Vec<u16>>,
    path: &std::path::Path,
    format: ExportFormat,
    quality: u32,
    metadata: Option<&ExportMetadata>,
) -> Result<(), String> {
    write_export_image_with_profile(buffer, path, format, quality, metadata, None)
}

fn write_export_image_with_profile(
    buffer: ImageBuffer<Rgb<u16>, Vec<u16>>,
    path: &std::path::Path,
    format: ExportFormat,
    quality: u32,
    metadata: Option<&ExportMetadata>,
    profile: Option<&[u8]>,
) -> Result<(), String> {
    let dynamic = match format {
        ExportFormat::Jpeg | ExportFormat::Tiff8 => {
            let (width, height) = buffer.dimensions();
            let mut out8 = ImageBuffer::<Rgb<u8>, Vec<u8>>::new(width, height);
            out8.as_mut()
                .par_chunks_exact_mut(3)
                .zip(buffer.as_raw().par_chunks_exact(3))
                .for_each(|(target, source)| {
                    target[0] = (source[0] >> 8) as u8;
                    target[1] = (source[1] >> 8) as u8;
                    target[2] = (source[2] >> 8) as u8;
                });
            image::DynamicImage::ImageRgb8(out8)
        }
        ExportFormat::Png | ExportFormat::Tiff16 => image::DynamicImage::ImageRgb16(buffer),
    };
    let output_format = match format {
        ExportFormat::Jpeg => ImageOutputFormat::Jpeg(quality as u8),
        ExportFormat::Png => ImageOutputFormat::Png,
        ExportFormat::Tiff8 | ExportFormat::Tiff16 => ImageOutputFormat::Tiff,
    };
    let mut cursor = Cursor::new(Vec::new());
    dynamic
        .write_to(&mut cursor, output_format)
        .map_err(|error| format!("Image encoding failed: {error}"))?;
    let mut encoded = cursor.into_inner();
    if let Some(profile) = profile {
        attach_export_profile(&mut encoded, format, profile)?;
    }
    if let Some(metadata) = metadata {
        attach_export_metadata(&mut encoded, format, metadata)?;
    }
    write_bytes_atomically(path, &encoded)
}

fn write_bytes_atomically(path: &std::path::Path, bytes: &[u8]) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| format!("Output path has no parent: {}", path.display()))?;
    let file_name = path
        .file_name()
        .ok_or_else(|| format!("Output path has no file name: {}", path.display()))?
        .to_string_lossy();

    for _ in 0..128 {
        let sequence = EXPORT_TEMP_ID.fetch_add(1, Ordering::Relaxed);
        let temp_path = parent.join(format!(
            "{EXPORT_TEMP_PREFIX}{}-{sequence}-{file_name}",
            std::process::id()
        ));
        let mut file = match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp_path)
        {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => {
                return Err(format!(
                    "Could not create temporary export in {}: {error}",
                    parent.display()
                ))
            }
        };

        let staged = file.write_all(bytes).and_then(|_| file.sync_all());
        drop(file);
        if let Err(error) = staged {
            let _ = std::fs::remove_file(&temp_path);
            return Err(format!("Could not stage {}: {error}", path.display()));
        }

        if let Err(error) = replace_file_atomically(&temp_path, path) {
            let _ = std::fs::remove_file(&temp_path);
            return Err(format!("Could not finalize {}: {error}", path.display()));
        }
        return Ok(());
    }

    Err(format!(
        "Could not allocate a unique temporary export name in {}",
        parent.display()
    ))
}

#[cfg(target_os = "windows")]
fn replace_file_atomically(
    source: &std::path::Path,
    target: &std::path::Path,
) -> std::io::Result<()> {
    use std::os::windows::ffi::OsStrExt;

    #[link(name = "kernel32")]
    extern "system" {
        fn MoveFileExW(
            existing_file_name: *const u16,
            new_file_name: *const u16,
            flags: u32,
        ) -> i32;
    }

    const MOVEFILE_REPLACE_EXISTING: u32 = 0x1;
    const MOVEFILE_WRITE_THROUGH: u32 = 0x8;
    let source = source
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let target = target
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let result = unsafe {
        MoveFileExW(
            source.as_ptr(),
            target.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    };
    if result == 0 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(())
    }
}

#[cfg(not(target_os = "windows"))]
fn replace_file_atomically(
    source: &std::path::Path,
    target: &std::path::Path,
) -> std::io::Result<()> {
    std::fs::rename(source, target)
}

fn cleanup_stale_export_files(directory: &std::path::Path) -> Result<usize, String> {
    let entries = std::fs::read_dir(directory).map_err(|error| {
        format!(
            "Could not inspect export directory {}: {error}",
            directory.display()
        )
    })?;
    let mut removed = 0;
    for entry in entries {
        let entry = entry
            .map_err(|error| format!("Could not inspect an export directory entry: {error}"))?;
        let name = entry.file_name();
        if !name.to_string_lossy().starts_with(EXPORT_TEMP_PREFIX) {
            continue;
        }
        match std::fs::remove_file(entry.path()) {
            Ok(()) => removed += 1,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(format!(
                    "Could not clean stale export {}: {error}",
                    entry.path().display()
                ))
            }
        }
    }
    Ok(removed)
}

#[tauri::command]
pub async fn batch_export_images(
    export_ids: Vec<String>,
    output_dir: String,
    format: String,
    color_space: String,
    resize_mode: String,
    long_edge: u32,
    allow_upscale: bool,
    sharpening: String,
    naming_template: String,
    conflict_policy: String,
    quality: u32,
    write_exif: bool,
    state: State<'_, EngineState>,
    app_handle: tauri::AppHandle,
) -> Result<BatchExportResult, String> {
    let color_space = validate_export_color_space(&color_space)?.to_string();
    let output_space = parse_output_space(&color_space)
        .ok_or_else(|| format!("Unsupported export color space: {color_space}"))?;
    let output_path = std::path::PathBuf::from(&output_dir);
    if !output_path.is_dir() {
        return Err(format!("Export directory does not exist: {output_dir}"));
    }
    let export_format = ExportFormat::parse(&format)?;
    let conflict_policy = ExportConflictPolicy::parse(&conflict_policy)?;
    let sharpening = export_sharpening(&sharpening)?;
    export_dimensions(1, 1, &resize_mode, long_edge, allow_upscale)?;
    if export_format == ExportFormat::Jpeg && !(1..=100).contains(&quality) {
        return Err("JPEG quality must be between 1 and 100".to_string());
    }
    let count = export_ids.len();
    if count == 0 {
        return Ok(BatchExportResult {
            exported: 0,
            skipped: 0,
            failed: 0,
            output_dir,
            errors: Vec::new(),
            warnings: Vec::new(),
        });
    }
    if EXPORT_ACTIVE.swap(true, Ordering::SeqCst) {
        return Err("Another export is already running".to_string());
    }
    let _active_guard = ExportActiveGuard;
    let cleanup_directory = output_path.clone();
    tokio::task::spawn_blocking(move || cleanup_stale_export_files(&cleanup_directory))
        .await
        .map_err(|error| format!("Export cleanup worker failed: {error}"))??;

    let rolls = read_lock(&state.rolls).clone();
    let calibration_profiles = tokio::task::spawn_blocking(load_calibration_profile_views)
        .await
        .map_err(|error| format!("Calibration profile worker failed: {error}"))??;
    let progress_app = app_handle.clone();
    let identities = export_ids
        .iter()
        .map(|id| {
            let item_arc = state
                .items
                .get(id)
                .ok_or_else(|| format!("Image is no longer available for export: {id}"))?;
            let item = read_lock(&item_arc);
            Ok((id.clone(), item.file_path.clone(), item.roll_id.clone()))
        })
        .collect::<Result<Vec<_>, String>>()?;

    // Freeze every roll-backed edit in one SQLite read transaction. The user can
    // continue editing after this point without changing the running export.
    let mut export_snapshots = tokio::task::spawn_blocking(move || {
        let mut connection = persistence::open_connection()
            .map_err(|error| format!("Failed to open export database: {error}"))?;
        connection
            .busy_timeout(std::time::Duration::from_secs(5))
            .map_err(|error| format!("Failed to configure export database: {error}"))?;
        let transaction = connection
            .transaction()
            .map_err(|error| format!("Failed to start export snapshot: {error}"))?;
        let mut snapshots = Vec::with_capacity(identities.len());

        for (id, file_path, roll_id) in identities {
            let (params, geom, base_color, pipeline_state) =
                load_image_state_from_connection(&transaction, &roll_id, &file_path)?
                    .map(|(_, params, geom, base_color, pipeline_state)| {
                        (params, geom, base_color, pipeline_state)
                    })
                    .ok_or_else(|| {
                        format!(
                            "Persisted edit state is missing for {} image {}",
                            roll_id, file_path
                        )
                    })?;
            snapshots.push(ExportItemSnapshot {
                id,
                file_path,
                roll_id,
                params,
                geom,
                base_color,
                pipeline_state,
                resolver_input: PipelineResolverInput {
                    persisted_contract: ProcessingContract::LegacyV1,
                    roll_profile_id: None,
                    profile: None,
                    image_kind: PipelineImageKind::Unsupported,
                    density_anchors: DensityAnchors::default(),
                    raw_decode_version: RAW_DECODE_VERSION,
                    runtime_failure: None,
                },
                capture_profile: None,
                scanner_profile: None,
                output_path: std::path::PathBuf::new(),
                export_metadata: None,
            });
        }

        transaction
            .commit()
            .map_err(|error| format!("Failed to finalize export snapshot: {error}"))?;
        Ok::<_, String>(snapshots)
    })
    .await
    .map_err(|error| format!("Export snapshot worker failed: {error}"))??;

    // Resolve every output path before starting parallel work. This keeps
    // duplicate templates deterministic and prevents workers racing to write
    // the same file.
    let mut reserved_paths = HashSet::new();
    let mut skipped_count = 0;
    let mut resolution_warnings = Vec::new();
    for (index, snapshot) in export_snapshots.iter_mut().enumerate() {
        let roll = rolls.iter().find(|roll| roll.roll_id == snapshot.roll_id);
        snapshot.scanner_profile = roll
            .and_then(|roll| roll.scanner_profile_id.as_deref())
            .and_then(|profile_id| {
                persistence::open_connection()
                    .ok()
                    .and_then(|connection| persistence::load_scanner_profiles(&connection).ok())
                    .and_then(|records| {
                        records
                            .into_iter()
                            .find(|record| record.profile.profile_id == profile_id)
                    })
            })
            .filter(|record| record_is_current(record))
            .map(|record| record.profile);
        snapshot.resolver_input = pipeline_resolver_input(
            &snapshot.pipeline_state,
            roll,
            &calibration_profiles,
            &snapshot.file_path,
            None,
        );
        let resolution = resolve_pipeline(&snapshot.resolver_input);
        if resolution.requested_path != resolution.resolved_path {
            resolution_warnings.push(format!(
                "export_pipeline_fallback|{}|requested={:?}|resolved={:?}|{}",
                snapshot.file_path,
                resolution.requested_path,
                resolution.resolved_path,
                resolution.processing_report.fallback_reasons.join(",")
            ));
        }
        snapshot.pipeline_state = state_from_resolution(&snapshot.pipeline_state, &resolution);
        if snapshot.pipeline_state.contract == ProcessingContract::CaptureCorrectedV11 {
            let profile_id = resolution
                .resolved_profile_id
                .as_deref()
                .ok_or_else(|| "Resolver omitted the Capture Profile id".to_string())?;
            snapshot.capture_profile = calibration_profiles
                .iter()
                .find(|view| {
                    view.profile.profile_id == profile_id
                        && view.availability == CalibrationProfileAvailability::Available
                        && view.profile.payload.capture_is_verified(RAW_DECODE_VERSION)
                })
                .map(|view| view.profile.clone());
            debug_assert!(snapshot.capture_profile.is_some());
        }
        if write_exif {
            snapshot.export_metadata = roll.map(ExportMetadata::from);
        }
        let stem = render_export_name(&naming_template, snapshot, roll, index + 1)?;
        if let Some(path) = reserve_export_path(
            &output_path,
            &stem,
            export_format.extension(),
            conflict_policy,
            &mut reserved_paths,
        ) {
            snapshot.output_path = path;
        } else {
            skipped_count += 1;
        }
    }
    export_snapshots.retain(|snapshot| !snapshot.output_path.as_os_str().is_empty());

    // Parse every referenced LUT before starting any writes. A bad or missing
    // LUT must fail the export rather than silently changing the appearance.
    let lut_sources = export_snapshots
        .iter()
        .filter_map(|snapshot| {
            snapshot
                .params
                .lut
                .lut_path
                .as_ref()
                .map(|path| (path.clone(), snapshot.file_path.clone()))
        })
        .collect::<Vec<_>>();
    let parsed_luts = tokio::task::spawn_blocking(move || {
        let mut parsed = HashMap::new();
        for (path, file_path) in lut_sources {
            if parsed.contains_key(&path) {
                continue;
            }
            let lut = parse_lut(&path)
                .map_err(|error| format!("Cannot load LUT for {file_path}: {error}"))?;
            parsed.insert(path, lut);
        }
        Ok::<_, String>(parsed)
    })
    .await
    .map_err(|error| format!("Export LUT worker failed: {error}"))??;

    let result = tokio::task::spawn_blocking(move || {
        let success_count = std::sync::atomic::AtomicUsize::new(0);
        let failures = Mutex::new(Vec::<String>::new());
        let warnings = Mutex::new(resolution_warnings);
        let _ = progress_app.emit(
            "export_progress",
            serde_json::json!({ "processed": skipped_count, "total": count }),
        );
        let processed_count = std::sync::atomic::AtomicUsize::new(skipped_count);

        // Process one full-resolution image at a time. Each image still uses
        // Rayon internally, but the outer loop is deliberately sequential so
        // several decoded/rotated/graded 16-bit buffers cannot coexist.
        export_snapshots.iter().for_each(|snapshot| {
            let file_path = snapshot.file_path.clone();
            let params_owned = snapshot.params.clone();
            let geom_owned = snapshot.geom.clone();
            let base_color_owned = snapshot.base_color.clone();
            let decoded = if snapshot.pipeline_state.contract == ProcessingContract::LegacyV1 {
                decode_export_source(&file_path)
            } else {
                // The v1.1 branch decodes directly into ProPhoto f32 below.
                // Avoid allocating and retaining a second full-size legacy image.
                Ok(ImageBuffer::<Rgb<u16>, Vec<u16>>::new(1, 1))
            };
            match decoded {
                Ok(original) => {
                    let params = &params_owned;
                    let base_color = &base_color_owned;
                    if snapshot.pipeline_state.contract != ProcessingContract::LegacyV1 {
                        let mut render_pipeline_state = snapshot.pipeline_state.clone();
                        let decoded_f32 = if snapshot.pipeline_state.contract
                            == ProcessingContract::CaptureCorrectedV11
                        {
                            let capture_result = snapshot
                                .capture_profile
                                .as_ref()
                                .ok_or_else(|| "verified Capture Profile is missing".to_string())
                                .and_then(|profile| decode_capture_corrected_image_buffer(&file_path, profile));
                            match capture_result {
                                Ok(capture) => Ok((capture.image, Some(capture.quality))),
                                Err(error) => {
                                    let mut fallback_input = snapshot.resolver_input.clone();
                                    fallback_input.runtime_failure = Some(error.clone());
                                    let fallback = resolve_pipeline(&fallback_input);
                                    render_pipeline_state = state_from_resolution(
                                        &snapshot.pipeline_state,
                                        &fallback,
                                    );
                                    lock_mutex(&warnings).push(format!(
                                        "export_pipeline_runtime_fallback|{}|requested={:?}|resolved={:?}|{}",
                                        file_path,
                                        fallback.requested_path,
                                        fallback.resolved_path,
                                        fallback.processing_report.fallback_reasons.join(",")
                                    ));
                                    decode_scanner_profiled_estimate_image_buffer(
                                        &file_path,
                                        DecodeMode::ExportFull,
                                        snapshot.scanner_profile.as_ref(),
                                        u32::MAX,
                                    )
                                    .map(|estimate| (estimate, None))
                                }
                            }
                        } else {
                            decode_scanner_profiled_estimate_image_buffer(
                                &file_path,
                                DecodeMode::ExportFull,
                                snapshot.scanner_profile.as_ref(),
                                u32::MAX,
                            )
                            .map(|estimate| (estimate, None))
                        };
                        let (input, quality_mask) = match decoded_f32 {
                            Ok(decoded) => decoded,
                            Err(error) => {
                                lock_mutex(&failures).push(format!("Failed to decode {}: {error}", file_path));
                                return;
                            }
                        };
                        let rendered_display = render_f32_shader_equivalent(
                            &input,
                            quality_mask.as_ref(),
                            params,
                            &geom_owned,
                            base_color,
                            &render_pipeline_state,
                            params.lut.lut_path.as_deref().and_then(|path| parsed_luts.get(path)),
                        );
                        let mut out_buffer = rendered_display;
                        let (width, height) = out_buffer.dimensions();
                        let (target_width, target_height) = match export_dimensions(
                            width, height, &resize_mode, long_edge, allow_upscale,
                        ) {
                            Ok(dimensions) => dimensions,
                            Err(error) => {
                                lock_mutex(&failures).push(format!("Invalid export dimensions for {}: {error}", file_path));
                                return;
                            }
                        };
                        if (target_width, target_height) != (width, height) {
                            out_buffer = image::imageops::resize(&out_buffer, target_width, target_height, image::imageops::FilterType::Lanczos3);
                        }
                        if let Some((sigma, amount)) = sharpening {
                            apply_usm(&mut out_buffer, sigma, amount);
                        }
                        let out_buffer = match encode_export_buffer(out_buffer, output_space) {
                            Ok(buffer) => buffer,
                            Err(error) => {
                                lock_mutex(&failures).push(format!("Failed to convert {} to {}: {error}", file_path, color_space));
                                return;
                            }
                        };
                        let profile = export_profile_for_output(export_format, output_space);
                        match write_export_image_with_profile(
                            out_buffer, &snapshot.output_path, export_format, quality,
                            snapshot.export_metadata.as_ref(), profile.as_deref(),
                        ) {
                            Ok(()) => { success_count.fetch_add(1, std::sync::atomic::Ordering::SeqCst); }
                            Err(error) => lock_mutex(&failures).push(error),
                        }
                        return;
                    }

                    let mut transformed = original;

                    if geom_owned.angle.abs() > 0.01 {
                        let angle_rad = geom_owned.angle.to_radians();
                        let (w, h) = transformed.dimensions();

                        let cos_a = angle_rad.cos();
                        let sin_a = angle_rad.sin();

                        let new_w = (w as f32 * cos_a.abs() + h as f32 * sin_a.abs()).ceil() as u32;
                        let new_h = (w as f32 * sin_a.abs() + h as f32 * cos_a.abs()).ceil() as u32;

                        let diag = ((w as f32).hypot(h as f32)).ceil() as u32;
                        let mut expanded =
                            ImageBuffer::from_pixel(diag, diag, image::Rgb([0, 0, 0]));
                        let offset_x = (diag as i64 - w as i64) / 2;
                        let offset_y = (diag as i64 - h as i64) / 2;
                        image::imageops::overlay(&mut expanded, &transformed, offset_x, offset_y);

                        let rotated = imageproc::geometric_transformations::rotate_about_center(
                            &expanded,
                            angle_rad,
                            imageproc::geometric_transformations::Interpolation::Bicubic,
                            image::Rgb([0, 0, 0]),
                        );

                        let crop_x = (diag.saturating_sub(new_w)) / 2;
                        let crop_y = (diag.saturating_sub(new_h)) / 2;
                        transformed =
                            image::imageops::crop_imm(&rotated, crop_x, crop_y, new_w, new_h)
                                .to_image();
                    }

                    match geom_owned.rotate_90_count.rem_euclid(4) {
                        1 => transformed = image::imageops::rotate90(&transformed),
                        2 => transformed = image::imageops::rotate180(&transformed),
                        3 => transformed = image::imageops::rotate270(&transformed),
                        _ => {}
                    }

                    if geom_owned.flip_h {
                        transformed = image::imageops::flip_horizontal(&transformed);
                    }
                    if geom_owned.flip_v {
                        transformed = image::imageops::flip_vertical(&transformed);
                    }

                    let export_lut = params
                        .lut
                        .lut_path
                        .as_deref()
                        .and_then(|path| parsed_luts.get(path));
                    let rendered_display = render_shader_equivalent(
                        &transformed,
                        params,
                        &geom_owned,
                        base_color,
                        export_lut,
                    );
                    // Resize and output sharpening intentionally preserve the
                    // legacy display-referred grading contract.
                    let mut out_buffer = rendered_display;

                    let (width, height) = out_buffer.dimensions();
                    let (target_width, target_height) = match export_dimensions(
                        width,
                        height,
                        &resize_mode,
                        long_edge,
                        allow_upscale,
                    ) {
                        Ok(dimensions) => dimensions,
                        Err(error) => {
                            lock_mutex(&failures).push(format!(
                                "Invalid export dimensions for {}: {error}",
                                file_path
                            ));
                            let processed = processed_count.fetch_add(
                                1,
                                std::sync::atomic::Ordering::SeqCst,
                            ) + 1;
                            let _ = progress_app.emit(
                                "export_progress",
                                serde_json::json!({
                                    "processed": processed,
                                    "total": count,
                                    "id": snapshot.id
                                }),
                            );
                            return;
                        }
                    };
                    if (target_width, target_height) != (width, height) {
                        out_buffer = image::imageops::resize(
                            &out_buffer,
                            target_width,
                            target_height,
                            image::imageops::FilterType::Lanczos3,
                        );
                    }

                    if let Some((sigma, amount)) = sharpening {
                        apply_usm(&mut out_buffer, sigma, amount);
                    }

                    out_buffer = match encode_export_buffer(out_buffer, output_space) {
                        Ok(buffer) => buffer,
                        Err(error) => {
                            lock_mutex(&failures).push(format!(
                                "Failed to convert {} to {}: {error}",
                                file_path, color_space
                            ));
                            let processed = processed_count.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
                            let _ = progress_app.emit(
                                "export_progress",
                                serde_json::json!({ "processed": processed, "total": count, "id": snapshot.id }),
                            );
                            return;
                        }
                    };

                    let profile = export_profile_for_output(export_format, output_space);
                    match write_export_image_with_profile(
                        out_buffer,
                        &snapshot.output_path,
                        export_format,
                        quality,
                        snapshot.export_metadata.as_ref(),
                        profile.as_deref(),
                    ) {
                        Ok(()) => {
                            success_count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                        }
                        Err(error) => lock_mutex(&failures).push(error),
                    }
                }
                Err(error) => {
                    lock_mutex(&failures).push(format!("Failed to decode {}: {error}", file_path))
                }
            }
            let processed = processed_count.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
            let _ = progress_app.emit(
                "export_progress",
                serde_json::json!({ "processed": processed, "total": count, "id": snapshot.id }),
            );
        });

        let failures = failures
            .into_inner()
            .unwrap_or_else(|error| error.into_inner());
        BatchExportResult {
            exported: success_count.into_inner(),
            skipped: skipped_count,
            failed: failures.len(),
            output_dir,
            errors: failures,
            warnings: warnings
                .into_inner()
                .unwrap_or_else(|error| error.into_inner()),
        }
    })
    .await
    .map_err(|error| format!("Export worker failed: {error}"))?;

    Ok(result)
}

#[tauri::command]
pub async fn get_rolls(state: State<'_, EngineState>) -> Result<Vec<Roll>, String> {
    let rolls = read_lock(&state.rolls);
    Ok(rolls.clone())
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CalibrationProfileInput {
    pub profile_id: Option<String>,
    pub name: String,
    #[serde(default)]
    pub camera: String,
    #[serde(default)]
    pub light_source: String,
    #[serde(default)]
    pub lens: String,
    #[serde(default)]
    pub notes: String,
    #[serde(default)]
    pub references: Vec<CalibrationReference>,
    /// Optional target-patch measurements. Supplying this field runs the
    /// measurement-backed pre-log fit and stores its coefficients.
    #[serde(default)]
    pub fit_measurements: Option<CalibrationMeasurementSet>,
    /// UI-friendly target fit request. The backend decodes the three RAW
    /// frames and fills each patch's measured transmission before fitting.
    #[serde(default)]
    pub fit_spec: Option<CalibrationFitSpec>,
    #[serde(default)]
    pub clear_fit: bool,
    /// Optional fit configuration. Offsets are disabled by default and, when
    /// requested, are accepted only if the affine model stays positive over
    /// the normalized transmission cube.
    #[serde(default)]
    pub fit_options: Option<FitOptions>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CalibrationFitSpec {
    pub dark_frame: String,
    pub open_frame: String,
    #[serde(default)]
    pub flat_frame: Option<String>,
    pub target_frame: String,
    pub patches: Vec<CalibrationPatchSpec>,
    #[serde(default)]
    pub resolution: Option<u32>,
    #[serde(default)]
    pub crop_geometry: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CalibrationPatchSpec {
    pub patch_id: String,
    pub position: [f32; 2],
    pub reference_value: [f32; 3],
    pub reference_domain: ReferenceDomain,
    #[serde(default)]
    pub validation: bool,
    #[serde(default)]
    pub saturated: bool,
    #[serde(default)]
    pub bad_pixel: bool,
    #[serde(default)]
    pub outlier: bool,
}

fn sha256_bytes(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn sha256_file(path: &Path) -> Result<String, String> {
    let mut file = std::fs::File::open(path)
        .map_err(|error| format!("Failed to open {} for digest: {error}", path.display()))?;
    let mut digest = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let read = file
            .read(&mut buffer)
            .map_err(|error| format!("Failed to hash {}: {error}", path.display()))?;
        if read == 0 {
            break;
        }
        digest.update(&buffer[..read]);
    }
    Ok(format!("{:x}", digest.finalize()))
}

fn sample_capture_patch(
    image: &crate::raw_backend::RelativeTransmissionRgbF32,
    position: [f32; 2],
) -> Result<[f32; 3], String> {
    if !position
        .iter()
        .all(|value| value.is_finite() && (0.0..=1.0).contains(value))
    {
        return Err("fit_patch_position_invalid".into());
    }
    let center_x = (position[0] * image.width.saturating_sub(1) as f32).round() as i32;
    let center_y = (position[1] * image.height.saturating_sub(1) as f32).round() as i32;
    let mut sum = [0.0_f32; 3];
    let mut count = 0_u32;
    for y in (center_y - 1).max(0)..=(center_y + 1).min(image.height as i32 - 1) {
        for x in (center_x - 1).max(0)..=(center_x + 1).min(image.width as i32 - 1) {
            let index = y as usize * image.width as usize + x as usize;
            if !image.quality.valid.get(index).copied().unwrap_or(false) {
                continue;
            }
            let pixel = image
                .transmission
                .get(index * 3..index * 3 + 3)
                .ok_or_else(|| "fit_patch_sample_missing".to_string())?;
            if !pixel.iter().all(|value| value.is_finite() && *value > 0.0) {
                continue;
            }
            for channel in 0..3 {
                sum[channel] += pixel[channel];
            }
            count += 1;
        }
    }
    if count == 0 {
        return Err("fit_patch_sample_invalid".into());
    }
    Ok(sum.map(|value| value / count as f32))
}

fn build_measurement_set_from_spec(
    spec: &CalibrationFitSpec,
    profile_camera: &str,
    profile_light: &str,
) -> Result<CalibrationMeasurementSet, String> {
    if spec.patches.len() < 4 {
        return Err("fit_requires_at_least_four_patches".into());
    }
    let dark = crate::raw_backend::decode_raw_mosaic(&spec.dark_frame)?;
    let open = crate::raw_backend::decode_raw_mosaic(&spec.open_frame)?;
    let target = crate::raw_backend::decode_raw_mosaic(&spec.target_frame)?;
    for (label, mosaic) in [("dark", &dark), ("open", &open), ("target", &target)] {
        if !matches!(
            mosaic.metadata.cfa,
            crate::raw_backend::CfaPattern::Bayer { .. }
        ) {
            return Err(format!("fit_{label}_requires_bayer_raw"));
        }
        if mosaic.metadata.libraw_version != target.metadata.libraw_version {
            return Err("fit_libraw_version_mismatch".into());
        }
    }
    let geometry = crate::raw_backend::raw_geometry_fingerprint(&target)?;
    if crate::raw_backend::raw_geometry_fingerprint(&dark)? != geometry
        || crate::raw_backend::raw_geometry_fingerprint(&open)? != geometry
    {
        return Err("fit_reference_geometry_mismatch".into());
    }
    let corrected =
        crate::raw_backend::decode_capture_corrected_input(&target, Some(&dark), Some(&open), &[])?;
    let patches = spec
        .patches
        .iter()
        .map(|patch| {
            Ok(CalibrationPatch {
                patch_id: patch.patch_id.trim().to_string(),
                position: Some(patch.position),
                capture_transmission: sample_capture_patch(&corrected, patch.position)?,
                reference_value: patch.reference_value,
                reference_domain: patch.reference_domain,
                validation: patch.validation,
                saturated: patch.saturated,
                bad_pixel: patch.bad_pixel,
                outlier: patch.outlier,
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    let dark_digest = sha256_file(Path::new(&spec.dark_frame))?;
    let open_digest = sha256_file(Path::new(&spec.open_frame))?;
    let target_digest = sha256_file(Path::new(&spec.target_frame))?;
    let flat_digest = spec
        .flat_frame
        .as_deref()
        .map(|path| sha256_file(Path::new(path)))
        .transpose()?;
    let quality_bytes = serde_json::to_vec(&corrected.quality.valid)
        .map_err(|error| format!("fit_quality_digest_serialize_failed|{error}"))?;
    let quality_mask_digest = sha256_bytes(&quality_bytes);
    let input_digest = sha256_bytes(
        &serde_json::to_vec(&(
            &dark_digest,
            &open_digest,
            &target_digest,
            &flat_digest,
            &patches,
        ))
        .map_err(|error| format!("fit_input_digest_serialize_failed|{error}"))?,
    );
    let camera_model = if !profile_camera.trim().is_empty() {
        profile_camera.trim().to_string()
    } else {
        target.metadata.camera_id.clone()
    };
    Ok(CalibrationMeasurementSet {
        dark_frame: spec.dark_frame.clone(),
        open_frame: spec.open_frame.clone(),
        flat_frame: spec.flat_frame.clone(),
        target_frame: spec.target_frame.clone(),
        patches,
        camera_model,
        scanner_model: None,
        iso: target.metadata.iso,
        exposure: target.metadata.exposure_seconds,
        light_source: profile_light.trim().to_string(),
        resolution: spec.resolution.or(Some(target.width.max(target.height))),
        crop_geometry: spec.crop_geometry.clone().unwrap_or(geometry),
        raw_decode_version: RAW_DECODE_VERSION.to_string(),
        input_digest,
        quality_mask_digest,
    })
}

fn reference_summary(
    reference: &CalibrationReference,
) -> Result<CalibrationReferenceSummary, String> {
    let mosaic = crate::raw_backend::decode_raw_mosaic(&reference.file_path)?;
    Ok(CalibrationReferenceSummary {
        reference_id: reference.reference_id.clone(),
        kind: reference.kind,
        content_digest: sha256_file(Path::new(&reference.file_path))?,
        raw_metadata_digest: crate::raw_backend::normalized_raw_metadata_digest(&mosaic.metadata)?,
    })
}

fn verified_reference_error(
    profile: &CalibrationConfigProfile,
    reference: &CalibrationReference,
) -> Option<String> {
    let Some(expected) = profile.payload.reference_frames.iter().find(|summary| {
        summary.reference_id == reference.reference_id && summary.kind == reference.kind
    }) else {
        return Some(format!(
            "profile_reference_summary_missing|{}",
            reference.file_name
        ));
    };
    let actual = match reference_summary(reference) {
        Ok(actual) => actual,
        Err(error) => {
            return Some(format!(
                "profile_reference_revalidation_failed|{}|{}",
                reference.file_name, error
            ));
        }
    };
    if actual.content_digest != expected.content_digest {
        Some(format!(
            "profile_reference_content_digest_changed|{}",
            reference.file_name
        ))
    } else if actual.raw_metadata_digest != expected.raw_metadata_digest {
        Some(format!(
            "profile_reference_metadata_digest_changed|{}",
            reference.file_name
        ))
    } else {
        None
    }
}

fn capture_hardware_fingerprint(
    profile: &CalibrationConfigProfile,
    mosaic: &crate::raw_backend::RawMosaic,
) -> Result<String, String> {
    let geometry_fingerprint = crate::raw_backend::raw_geometry_fingerprint(mosaic)?;
    Ok(sha256_bytes(
        format!(
            "{}|{}|{}|{}|{:?}",
            profile.camera.trim(),
            profile.lens.trim(),
            mosaic.metadata.camera_id,
            geometry_fingerprint,
            mosaic.metadata.cfa,
        )
        .as_bytes(),
    ))
}

fn calibration_level_from_profile(payload: &CalibrationProfilePayload) -> CalibrationLevel {
    if !payload.capture_is_verified(RAW_DECODE_VERSION) {
        CalibrationLevel::SmartAuto
    } else if payload.fit_model.is_some() && payload.fit_validation_error().is_none() {
        CalibrationLevel::CaptureCharacterized
    } else {
        CalibrationLevel::CaptureCorrectedExperimental
    }
}

fn timestamp_from_system_time(value: std::time::SystemTime) -> Option<i64> {
    value
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .map(|duration| duration.as_secs() as i64)
}

fn calibration_id(prefix: &str) -> String {
    format!(
        "{prefix}-{}-{}",
        persistence::now_timestamp(),
        NEXT_CALIBRATION_ID.fetch_add(1, Ordering::Relaxed)
    )
}

fn calibration_profile_view(mut profile: CalibrationConfigProfile) -> CalibrationProfileView {
    // Roll density anchors are owned by Library/Roll calibration. Older beta
    // builds could store them as Profile references, so isolate them here
    // without making the rest of the Profile unusable.
    profile.references.retain(|reference| {
        !matches!(
            reference.kind,
            CalibrationReferenceKind::FilmBase | CalibrationReferenceKind::FullExposure
        )
    });
    profile.calibration_level = calibration_level_from_profile(&profile.payload);
    let mut warnings = Vec::new();
    let mut blocking_warning = false;
    let unsupported = profile.schema_version != CALIBRATION_PROFILE_SCHEMA_VERSION
        || profile.payload.payload_version != CALIBRATION_PROFILE_PAYLOAD_VERSION;
    if unsupported {
        warnings.push("profile_schema_unsupported".to_string());
        blocking_warning = true;
    }
    let capture_payload_present = !profile.payload.capabilities.is_empty()
        || profile.payload.capture_parameters.is_some()
        || profile.payload.validation_report.is_some()
        || profile.payload.fit_model.is_some()
        || profile.payload.fit_measurements.is_some();
    if !unsupported && capture_payload_present {
        if let Some(reason) = profile.payload.capture_validation_error(RAW_DECODE_VERSION) {
            warnings.push(format!("profile_capture_invalid|{reason}"));
            blocking_warning = true;
        }
        if let Some(reason) = profile.payload.fit_validation_error() {
            warnings.push(format!("profile_capture_fit_invalid|{reason}"));
            blocking_warning = true;
        }
    }
    for reference in &profile.references {
        if reference.kind == CalibrationReferenceKind::Unknown {
            warnings.push(format!(
                "profile_reference_kind_unsupported|{}",
                reference.file_name
            ));
            blocking_warning = true;
        }
        let path = Path::new(&reference.file_path);
        let Ok(metadata) = std::fs::metadata(path) else {
            warnings.push(format!("profile_reference_missing|{}", reference.file_name));
            blocking_warning |= reference.kind != CalibrationReferenceKind::FlatField;
            continue;
        };
        let current_modified = metadata
            .modified()
            .ok()
            .and_then(timestamp_from_system_time);
        if metadata.len() != reference.file_size || current_modified != reference.modified_at {
            warnings.push(format!("profile_reference_changed|{}", reference.file_name));
            blocking_warning |= reference.kind != CalibrationReferenceKind::FlatField;
        }
        if profile.payload.issuer == CalibrationPayloadIssuer::BackendCalibrationSession {
            if let Some(error) = verified_reference_error(&profile, reference) {
                warnings.push(error);
                blocking_warning |= reference.kind != CalibrationReferenceKind::FlatField;
            }
        }
    }
    let availability = if unsupported {
        CalibrationProfileAvailability::Unsupported
    } else if blocking_warning {
        CalibrationProfileAvailability::NeedsAttention
    } else {
        CalibrationProfileAvailability::Available
    };
    CalibrationProfileView {
        profile,
        availability,
        warnings,
    }
}

fn load_calibration_profile_views() -> Result<Vec<CalibrationProfileView>, String> {
    let connection = persistence::open_connection()
        .map_err(|error| format!("Failed to open calibration database: {error}"))?;
    let profiles = persistence::load_calibration_profiles(&connection)
        .map_err(|error| format!("Failed to load calibration profiles: {error}"))?;
    Ok(profiles
        .into_iter()
        .map(|profile| {
            let session_valid =
                profile
                    .payload
                    .calibration_session_id
                    .as_deref()
                    .map(|session_id| {
                        persistence::calibration_session_matches(
                            &connection,
                            session_id,
                            &profile.profile_id,
                            &profile.payload.payload_digest,
                        )
                        .unwrap_or(false)
                    });
            let mut view = calibration_profile_view(profile);
            if session_valid == Some(false) {
                view.warnings
                    .push("profile_calibration_session_missing_or_changed".to_string());
                view.availability = CalibrationProfileAvailability::NeedsAttention;
            }
            view
        })
        .collect())
}

#[tauri::command]
pub async fn get_calibration_profiles() -> Result<Vec<CalibrationProfileView>, String> {
    tokio::task::spawn_blocking(load_calibration_profile_views)
        .await
        .map_err(|error| format!("Calibration profile worker failed: {error}"))?
}

/// Import a local scanner input profile. Scanner provenance is persisted
/// independently from Capture Calibration and never upgrades a film-density
/// capability.
#[tauri::command]
pub async fn import_scanner_profile(path: String) -> Result<ScannerProfileRecord, String> {
    tokio::task::spawn_blocking(move || {
        let (mut profile, digest) = import_local_profile(&path)?;
        if profile.profile_id.trim().is_empty() {
            profile.profile_id = format!("scanner-{digest}");
        }
        profile.validate()?;
        let record = ScannerProfileRecord {
            profile,
            source_digest: digest,
            source_path: path,
        };
        let mut connection = persistence::open_connection()
            .map_err(|error| format!("Failed to open scanner profile database: {error}"))?;
        persistence::save_scanner_profile(&mut connection, &record)
            .map_err(|error| format!("Failed to save scanner profile: {error}"))?;
        Ok(record)
    })
    .await
    .map_err(|error| format!("Scanner profile worker failed: {error}"))?
}

#[tauri::command]
pub async fn get_scanner_profiles() -> Result<Vec<ScannerProfileRecord>, String> {
    tokio::task::spawn_blocking(|| {
        let connection = persistence::open_connection()
            .map_err(|error| format!("Failed to open scanner profile database: {error}"))?;
        let profiles = persistence::load_scanner_profiles(&connection)
            .map_err(|error| format!("Failed to load scanner profiles: {error}"))?;
        let mut valid = Vec::new();
        for record in profiles {
            if record_is_current(&record) {
                valid.push(record);
            }
        }
        Ok(valid)
    })
    .await
    .map_err(|error| format!("Scanner profile worker failed: {error}"))?
}

#[tauri::command]
pub async fn update_roll_scanner_profile(
    roll_id: String,
    profile_id: Option<String>,
    state: State<'_, EngineState>,
) -> Result<Vec<Roll>, String> {
    let profile_id = profile_id
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    let profiles = tokio::task::spawn_blocking(|| {
        let connection = persistence::open_connection()
            .map_err(|error| format!("Failed to open scanner database: {error}"))?;
        persistence::load_scanner_profiles(&connection)
            .map_err(|error| format!("Failed to load scanner profiles: {error}"))
    })
    .await
    .map_err(|error| format!("Scanner profile worker failed: {error}"))??;
    if let Some(requested) = &profile_id {
        let record = profiles
            .iter()
            .find(|record| record.profile.profile_id == *requested)
            .ok_or_else(|| "Scanner profile no longer exists.".to_string())?;
        if !record_is_current(record) {
            return Err("Scanner profile source has changed; re-import it before binding.".into());
        }
    }
    let _mutation = state.roll_mutation.lock().await;
    let mut rolls = read_lock(&state.rolls).clone();
    let selected_template = {
        let roll = rolls
            .iter_mut()
            .find(|roll| roll.roll_id == roll_id)
            .ok_or_else(|| format!("Roll not found: {roll_id}"))?;
        if roll_calibration_format(&roll.format) == RollCalibrationFormat::Loose
            && profile_id.is_some()
        {
            return Err("Loose Import cannot bind a Scanner Profile.".into());
        }
        roll.scanner_profile_id = profile_id;
        pipeline_state_for_roll_profile(roll, &[])
    };
    let persisted = rolls.clone();
    let pipeline_updates = state
        .items
        .iter()
        .filter_map(|entry| {
            let item = read_lock(entry.value());
            if item.roll_id != roll_id {
                return None;
            }
            let mut next = selected_template.clone();
            next.content_range = item.pipeline_state.content_range.clone();
            next.render_mapping = item.pipeline_state.render_mapping.clone();
            Some((item.file_path.clone(), next))
        })
        .collect::<Vec<_>>();
    let persisted_pipeline_states = pipeline_updates
        .iter()
        .map(|(file_path, pipeline_state)| {
            (roll_id.clone(), file_path.clone(), pipeline_state.clone())
        })
        .collect::<Vec<_>>();
    tokio::task::spawn_blocking(move || {
        let mut connection = persistence::open_connection()
            .map_err(|error| format!("Failed to open roll database: {error}"))?;
        persistence::save_rolls_and_pipeline_states(
            &mut connection,
            &persisted,
            &persisted_pipeline_states,
        )
        .map_err(|error| format!("Failed to save Scanner Profile binding: {error}"))?;
        update_rolls_compatibility_mirror(&persisted);
        Ok::<_, String>(())
    })
    .await
    .map_err(|error| format!("Roll persistence worker failed: {error}"))??;
    for entry in state.items.iter() {
        let item = entry.value();
        let Ok(mut item) = item.write() else {
            continue;
        };
        if item.roll_id != roll_id {
            continue;
        }
        if let Some((_, next)) = pipeline_updates
            .iter()
            .find(|(file_path, _)| file_path == &item.file_path)
        {
            item.pipeline_state = next.clone();
        }
        item.runtime_pipeline_state = None;
        item.runtime_pipeline_key = None;
        item.original_proxy = None;
        item.proxy_image = None;
        item.prophoto_estimate_proxy = None;
        item.relative_transmission_proxy = None;
        item.relative_transmission_quality = None;
        item.pristine_proxy = None;
    }
    *write_lock(&state.rolls) = rolls.clone();
    Ok(rolls)
}

#[tauri::command]
pub async fn apply_scanner_profile(
    profile_id: String,
    input: [f32; 3],
) -> Result<[f32; 3], String> {
    tokio::task::spawn_blocking(move || {
        let connection = persistence::open_connection()
            .map_err(|error| format!("Failed to open scanner profile database: {error}"))?;
        let profile = persistence::load_scanner_profiles(&connection)
            .map_err(|error| format!("Failed to load scanner profiles: {error}"))?
            .into_iter()
            .find(|record| record.profile.profile_id == profile_id)
            .ok_or_else(|| "Scanner profile not found.".to_string())?;
        if !record_is_current(&profile) {
            return Err("Scanner profile source digest changed; re-import is required.".into());
        }
        profile.profile.apply_linear_rgb(input)
    })
    .await
    .map_err(|error| format!("Scanner profile worker failed: {error}"))?
}

#[tauri::command]
pub async fn choose_calibration_reference(
    kind: CalibrationReferenceKind,
) -> Result<Option<CalibrationReference>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let path = FileDialog::new()
            .set_title("Choose Calibration Reference")
            .add_filter(
                "Calibration Reference",
                &[
                    "dng", "nef", "nrw", "cr2", "cr3", "arw", "raf", "rw2", "orf", "srw", "pef",
                    "3fr", "iiq", "raw", "tiff", "tif", "jpg", "jpeg", "png", "json", "csv",
                ],
            )
            .pick_file();
        let Some(path) = path else {
            return Ok(None);
        };
        let metadata = std::fs::metadata(&path)
            .map_err(|error| format!("Failed to read calibration reference: {error}"))?;
        let file_name = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("reference")
            .to_string();
        Ok(Some(CalibrationReference {
            reference_id: calibration_id("cal-ref"),
            kind,
            file_path: path.to_string_lossy().to_string(),
            file_name,
            file_size: metadata.len(),
            modified_at: metadata
                .modified()
                .ok()
                .and_then(timestamp_from_system_time),
            added_at: persistence::now_timestamp(),
        }))
    })
    .await
    .map_err(|error| format!("Calibration reference dialog failed: {error}"))?
}

#[tauri::command]
pub async fn choose_scanner_profile_file() -> Result<Option<String>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        Ok(FileDialog::new()
            .set_title("Choose Scanner Input Profile")
            .add_filter("Scanner Profile Configuration", &["json"])
            .pick_file()
            .map(|path| path.to_string_lossy().to_string()))
    })
    .await
    .map_err(|error| format!("Scanner profile dialog failed: {error}"))?
}

#[tauri::command]
pub async fn save_calibration_profile(
    input: CalibrationProfileInput,
) -> Result<CalibrationProfileView, String> {
    tokio::task::spawn_blocking(move || {
        let name = input.name.trim();
        if name.is_empty() {
            return Err("Calibration profile name is required.".to_string());
        }
        let mut connection = persistence::open_connection()
            .map_err(|error| format!("Failed to open calibration database: {error}"))?;
        let existing = persistence::load_calibration_profiles(&connection)
            .map_err(|error| format!("Failed to load calibration profiles: {error}"))?;
        let requested_id = input
            .profile_id
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty());
        let existing_profile = requested_id.and_then(|profile_id| {
            existing
                .iter()
                .find(|profile| profile.profile_id == profile_id)
        });
        if requested_id.is_some() && existing_profile.is_none() {
            return Err("Calibration profile no longer exists.".to_string());
        }
        let now = persistence::now_timestamp();
        let references = input
            .references
            .into_iter()
            .filter(|reference| {
                !matches!(
                    reference.kind,
                    CalibrationReferenceKind::FilmBase | CalibrationReferenceKind::FullExposure
                )
            })
            .collect::<Vec<_>>();
        let input_hardware = (
            input.camera.trim(),
            input.light_source.trim(),
            input.lens.trim(),
        );
        let existing_hardware = existing_profile.map(|profile| {
            (
                profile.camera.as_str(),
                profile.light_source.as_str(),
                profile.lens.as_str(),
            )
        });
        let references_unchanged =
            existing_profile.is_some_and(|profile| profile.references == references);
        let mut payload = if references_unchanged && existing_hardware == Some(input_hardware) {
            existing_profile
                .map(|profile| profile.payload.clone())
                .unwrap_or_default()
        } else {
            CalibrationProfilePayload::default()
        };
        let mut fit_measurements = input.fit_measurements;
        let fit_from_spec = input.fit_spec.is_some();
        if input.clear_fit {
            payload.fit_measurements = None;
            payload.fit_model = None;
        }
        if let Some(spec) = input.fit_spec.as_ref() {
            let reference_matches = |kind: CalibrationReferenceKind, path: &str| {
                references.iter().any(|reference| {
                    reference.kind == kind
                        && normalize_path(&reference.file_path) == normalize_path(path)
                })
            };
            if !reference_matches(CalibrationReferenceKind::DarkFrame, &spec.dark_frame)
                || !reference_matches(CalibrationReferenceKind::OpenGate, &spec.open_frame)
                || !reference_matches(
                    CalibrationReferenceKind::TransmissionTarget,
                    &spec.target_frame,
                )
                || (spec.flat_frame.as_deref().is_some_and(|path| {
                    !reference_matches(CalibrationReferenceKind::FlatField, path)
                }))
            {
                return Err(
                    "Calibration fit frames must be selected as matching Profile references."
                        .into(),
                );
            }
            let measurements = build_measurement_set_from_spec(
                spec,
                input.camera.as_str(),
                input.light_source.as_str(),
            )?;
            fit_measurements = Some(measurements);
        }
        if let Some(measurements) = fit_measurements {
            if fit_from_spec {
                let reference_matches = |kind: CalibrationReferenceKind, path: &str| {
                    references.iter().any(|reference| {
                        reference.kind == kind
                            && normalize_path(&reference.file_path) == normalize_path(path)
                    })
                };
                if !reference_matches(
                    CalibrationReferenceKind::DarkFrame,
                    &measurements.dark_frame,
                ) || !reference_matches(
                    CalibrationReferenceKind::OpenGate,
                    &measurements.open_frame,
                ) || !reference_matches(
                    CalibrationReferenceKind::TransmissionTarget,
                    &measurements.target_frame,
                ) || (measurements.flat_frame.as_deref().is_some_and(|path| {
                    !reference_matches(CalibrationReferenceKind::FlatField, path)
                })) {
                    return Err("Calibration measurements must match Profile references.".into());
                }
            }
            let model =
                fit_capture_separation(&measurements, input.fit_options.unwrap_or_default())
                    .map_err(|error| format!("Calibration fit failed: {error}"))?;
            payload.fit_measurements = Some(measurements);
            payload.fit_model = Some(model);
        }
        let calibration_level = calibration_level_from_profile(&payload);
        let profile = CalibrationConfigProfile {
            profile_id: requested_id
                .map(str::to_string)
                .unwrap_or_else(|| calibration_id("cal-profile")),
            schema_version: CALIBRATION_PROFILE_SCHEMA_VERSION,
            name: name.to_string(),
            created_at: existing_profile
                .map(|profile| profile.created_at)
                .unwrap_or(now),
            updated_at: now,
            camera: input.camera.trim().to_string(),
            light_source: input.light_source.trim().to_string(),
            lens: input.lens.trim().to_string(),
            calibration_level,
            notes: input.notes.trim().to_string(),
            references,
            payload,
        };
        let mut reference_ids = HashSet::new();
        for reference in &profile.references {
            if reference.file_path.trim().is_empty()
                || !reference_ids.insert(reference.reference_id.as_str())
            {
                return Err(
                    "Calibration references must be unique and have a file path.".to_string(),
                );
            }
        }
        persistence::save_calibration_profile(&mut connection, &profile)
            .map_err(|error| format!("Failed to save calibration profile: {error}"))?;
        Ok(calibration_profile_view(profile))
    })
    .await
    .map_err(|error| format!("Calibration profile worker failed: {error}"))?
}

fn unique_capture_reference(
    profile: &CalibrationConfigProfile,
    kind: CalibrationReferenceKind,
) -> Result<&CalibrationReference, String> {
    let mut references = profile
        .references
        .iter()
        .filter(|reference| reference.kind == kind);
    let reference = references
        .next()
        .ok_or_else(|| format!("Calibration session requires {kind:?}"))?;
    if references.next().is_some() {
        return Err(format!(
            "Calibration session reference is ambiguous: {kind:?}"
        ));
    }
    Ok(reference)
}

fn build_capture_calibration_session(
    profile: &CalibrationConfigProfile,
) -> Result<CalibrationProfilePayload, String> {
    if profile.camera.trim().is_empty() || profile.light_source.trim().is_empty() {
        return Err("Calibration session requires camera and light-source identity.".to_string());
    }
    let dark_reference = unique_capture_reference(profile, CalibrationReferenceKind::DarkFrame)?;
    let open_reference = unique_capture_reference(profile, CalibrationReferenceKind::OpenGate)?;
    let dark = crate::raw_backend::decode_raw_mosaic(&dark_reference.file_path)?;
    let open = crate::raw_backend::decode_raw_mosaic(&open_reference.file_path)?;
    let summaries = profile
        .references
        .iter()
        .filter(|reference| {
            matches!(
                reference.kind,
                CalibrationReferenceKind::DarkFrame
                    | CalibrationReferenceKind::OpenGate
                    | CalibrationReferenceKind::FlatField
                    | CalibrationReferenceKind::TransmissionTarget
            )
        })
        .map(reference_summary)
        .collect::<Result<Vec<_>, _>>()?;
    build_capture_calibration_payload(
        profile,
        &dark,
        &open,
        summaries,
        persistence::now_timestamp(),
        calibration_id("cal-session"),
    )
}

fn build_capture_calibration_payload(
    profile: &CalibrationConfigProfile,
    dark: &crate::raw_backend::RawMosaic,
    open: &crate::raw_backend::RawMosaic,
    summaries: Vec<CalibrationReferenceSummary>,
    checked_at: i64,
    session_id: String,
) -> Result<CalibrationProfilePayload, String> {
    if !matches!(
        open.metadata.cfa,
        crate::raw_backend::CfaPattern::Bayer { .. }
    ) {
        return Err(
            "Capture Corrected currently requires Bayer RAW; X-Trans falls back.".to_string(),
        );
    }
    let corrected = crate::raw_backend::correct_cfa_capture(&open, Some(&dark), Some(&open), &[])?;
    let quality = corrected.quality.summary();
    if quality.valid_samples == 0 {
        return Err("Calibration session produced no valid dark/open samples.".to_string());
    }
    let mut mask_bytes = vec![0u8; quality.total_samples.div_ceil(8)];
    let mut bad_pixel_indices = Vec::new();
    for (index, valid) in corrected.quality.valid.iter().enumerate() {
        if !valid {
            mask_bytes[index / 8] |= 1 << (index % 8);
            if index <= u32::MAX as usize {
                bad_pixel_indices.push(index as u32);
            }
        }
    }
    let mask_digest = sha256_bytes(&mask_bytes);
    let geometry_fingerprint = crate::raw_backend::raw_geometry_fingerprint(&open)?;
    let hardware_fingerprint = capture_hardware_fingerprint(profile, &open)?;
    if !summaries
        .iter()
        .any(|summary| summary.kind == CalibrationReferenceKind::DarkFrame)
        || !summaries
            .iter()
            .any(|summary| summary.kind == CalibrationReferenceKind::OpenGate)
    {
        return Err(
            "Calibration session summaries must cover dark and open-gate references.".to_string(),
        );
    }
    let has_flat = summaries
        .iter()
        .any(|summary| summary.kind == CalibrationReferenceKind::FlatField);
    let mut payload = CalibrationProfilePayload {
        issuer: CalibrationPayloadIssuer::BackendCalibrationSession,
        calibration_session_id: Some(session_id),
        hardware_fingerprint,
        raw_decode_version: Some(RAW_DECODE_VERSION),
        libraw_version: open.metadata.libraw_version.clone(),
        reference_frames: summaries,
        capture_parameters: Some(CaptureCalibrationParameters {
            correction_algorithm: crate::app_state::CAPTURE_CORRECTION_ALGORITHM_VERSION
                .to_string(),
            demosaic_algorithm: crate::app_state::CAPTURE_DEMOSAIC_ALGORITHM_VERSION.to_string(),
            epsilon: crate::raw_backend::DENSITY_EPSILON,
            light_source_id: profile.light_source.trim().to_string(),
            geometry_fingerprint,
        }),
        quality_mask: Some(CalibrationQualityMaskSummary {
            total_samples: quality.total_samples as u64,
            valid_samples: quality.valid_samples as u64,
            invalid_denominator: quality.invalid_denominator as u64,
            negative_samples: quality.negative_samples as u64,
            saturated_samples: quality.saturated_samples as u64,
            bad_pixels: quality.bad_pixels as u64,
            out_of_range: quality.out_of_range as u64,
            bad_pixel_indices,
            mask_artifact_digest: mask_digest,
        }),
        mask_artifact: Some(CalibrationQualityMaskArtifact {
            encoding: "invalid_bitset_le_v1".to_string(),
            sample_count: quality.total_samples as u64,
            data_base64: general_purpose::STANDARD.encode(mask_bytes),
        }),
        valid_range: Some(CalibrationValidRange {
            minimum_transmission: [crate::raw_backend::DENSITY_EPSILON; 3],
            maximum_transmission: [1.0 + crate::raw_backend::DENSITY_EPSILON; 3],
        }),
        capabilities: vec![CalibrationCapability::CaptureCorrected],
        validation_report: Some(CalibrationValidationReport {
            status: CalibrationValidationStatus::Passed,
            checked_at: Some(checked_at),
            checks: vec![
                "sha256_reference_content".to_string(),
                "normalized_raw_metadata".to_string(),
                "dark_open_homogeneous_path".to_string(),
                "mask_artifact_persisted".to_string(),
            ],
            warnings: if has_flat {
                vec!["flat_stored_not_applied_pending_independent_definition".to_string()]
            } else {
                vec!["optional_flat_not_available".to_string()]
            },
        }),
        ..CalibrationProfilePayload::default()
    };
    // Preserve an already fitted target model while refreshing capture-frame
    // provenance. The fit is part of the same payload digest and session.
    payload.fit_model = profile.payload.fit_model.clone();
    payload.fit_measurements = profile.payload.fit_measurements.clone();
    if let Some(measurements) = payload.fit_measurements.as_ref() {
        let matches_reference = |kind: CalibrationReferenceKind, path: &str| {
            profile.references.iter().any(|reference| {
                reference.kind == kind
                    && normalize_path(&reference.file_path) == normalize_path(path)
            })
        };
        if !matches_reference(
            CalibrationReferenceKind::DarkFrame,
            &measurements.dark_frame,
        ) || !matches_reference(CalibrationReferenceKind::OpenGate, &measurements.open_frame)
            || !matches_reference(
                CalibrationReferenceKind::TransmissionTarget,
                &measurements.target_frame,
            )
        {
            return Err("Stored fit measurements no longer match Profile references.".into());
        }
    }
    payload.payload_digest = payload.canonical_digest()?;
    Ok(payload)
}

#[tauri::command]
pub async fn run_capture_calibration_session(
    profile_id: String,
) -> Result<CalibrationProfileView, String> {
    tokio::task::spawn_blocking(move || {
        let mut connection = persistence::open_connection()
            .map_err(|error| format!("Failed to open calibration database: {error}"))?;
        let mut profile = persistence::load_calibration_profiles(&connection)
            .map_err(|error| format!("Failed to load calibration profiles: {error}"))?
            .into_iter()
            .find(|profile| profile.profile_id == profile_id)
            .ok_or_else(|| "Calibration profile no longer exists.".to_string())?;
        let payload = build_capture_calibration_session(&profile)?;
        let session_id = payload
            .calibration_session_id
            .clone()
            .ok_or_else(|| "Calibration session id was not generated.".to_string())?;
        let checked_at = payload
            .validation_report
            .as_ref()
            .and_then(|report| report.checked_at)
            .unwrap_or_else(persistence::now_timestamp);
        profile.payload = payload;
        profile.calibration_level = calibration_level_from_profile(&profile.payload);
        profile.updated_at = checked_at;
        persistence::save_calibration_profile_and_session(
            &mut connection,
            &profile,
            &session_id,
            checked_at,
            "passed",
        )
        .map_err(|error| {
            format!("Failed to save calibrated profile/session atomically: {error}")
        })?;
        Ok(calibration_profile_view(profile))
    })
    .await
    .map_err(|error| format!("Calibration session worker failed: {error}"))?
}

#[tauri::command]
pub async fn delete_calibration_profile(profile_id: String) -> Result<bool, String> {
    tokio::task::spawn_blocking(move || {
        let connection = persistence::open_connection()
            .map_err(|error| format!("Failed to open calibration database: {error}"))?;
        persistence::delete_calibration_profile(&connection, &profile_id)
            .map_err(|error| format!("Failed to delete calibration profile: {error}"))
    })
    .await
    .map_err(|error| format!("Calibration profile worker failed: {error}"))?
}

fn persist_roll_snapshot(rolls: &[Roll]) -> Result<(), String> {
    let mut connection = persistence::open_connection()
        .map_err(|error| format!("Failed to open roll database: {error}"))?;
    persistence::save_rolls(&mut connection, rolls)
        .map_err(|error| format!("Failed to save roll metadata: {error}"))
}

fn update_rolls_compatibility_mirror(rolls: &[Roll]) {
    if let Err(error) = persistence::write_rolls_compatibility_mirror(rolls) {
        eprintln!("[Roll Persistence] {error}");
    }
}

async fn persist_roll_snapshot_async(rolls: Vec<Roll>) -> Result<Vec<Roll>, String> {
    tokio::task::spawn_blocking(move || {
        persist_roll_snapshot(&rolls)?;
        update_rolls_compatibility_mirror(&rolls);
        Ok(rolls)
    })
    .await
    .map_err(|error| format!("Roll persistence worker failed: {error}"))?
}

fn remove_failed_roll_paths(
    rolls: &mut Vec<Roll>,
    roll_id: &str,
    failed_paths: &HashSet<String>,
) -> bool {
    let Some(index) = rolls.iter().position(|roll| roll.roll_id == roll_id) else {
        return false;
    };
    let original_len = rolls[index].image_paths.len();
    rolls[index]
        .image_paths
        .retain(|path| !failed_paths.contains(&normalize_path(path)));
    if rolls[index].image_paths.len() == original_len {
        return false;
    }
    if rolls[index].image_paths.is_empty() {
        rolls.remove(index);
    }
    true
}

#[tauri::command]
pub async fn import_roll(
    mut roll: Roll,
    paths: Vec<String>,
    state: State<'_, EngineState>,
    app_handle: tauri::AppHandle,
) -> Result<(), String> {
    let roll_id_clone = roll.roll_id.clone();
    let is_loose_roll = roll.format == "Loose" || roll.roll_id == "LOOSE_DEFAULT";
    {
        let _mutation = state.roll_mutation.lock().await;
        let mut updated = read_lock(&state.rolls).clone();
        if let Some(existing) = updated
            .iter_mut()
            .find(|existing| existing.roll_id == roll.roll_id)
        {
            // Import metadata is not a Profile-selection action. Preserve the
            // existing Roll binding even when an older client omits the field.
            roll.calibration_profile_id = existing.calibration_profile_id.clone();
            roll.scanner_profile_id = existing.scanner_profile_id.clone();
            *existing = roll;
        } else {
            // Capture Corrected is Experimental and opt-in per Roll. A newly
            // imported Roll never inherits a previous Profile implicitly.
            roll.calibration_profile_id = None;
            roll.scanner_profile_id = None;
            updated.push(roll);
        }
        let updated = persist_roll_snapshot_async(updated).await?;
        *write_lock(&state.rolls) = updated;
    }

    crate::commands::import_images(
        paths,
        Some(is_loose_roll),
        Some(true),
        Some(roll_id_clone),
        Some(false),
        Some(true),
        state,
        app_handle,
    )
    .await
}

fn selected_roll_item_state(
    state: &EngineState,
    roll_id: &str,
    image_id: Option<&str>,
) -> Option<(PipelineState, bool, PipelineImageKind)> {
    let requested = image_id
        .and_then(|id| state.items.get(id))
        .and_then(|entry| {
            let item = entry.value().read().ok()?;
            (item.roll_id == roll_id).then(|| {
                (
                    item.pipeline_state.clone(),
                    item.geom.calibration_points.is_some(),
                    pipeline_image_kind(&item.file_path),
                )
            })
        });
    requested.or_else(|| {
        state.items.iter().find_map(|entry| {
            let item = entry.value().read().ok()?;
            (item.roll_id == roll_id).then(|| {
                (
                    item.pipeline_state.clone(),
                    item.geom.calibration_points.is_some(),
                    pipeline_image_kind(&item.file_path),
                )
            })
        })
    })
}

fn roll_calibration_format(value: &str) -> RollCalibrationFormat {
    let normalized = value.trim().to_ascii_lowercase();
    if normalized == "135"
        || normalized.starts_with("135 ")
        || normalized == "35mm"
        || normalized.starts_with("35 mm")
    {
        RollCalibrationFormat::Film135
    } else if normalized == "120" || normalized.starts_with("120 ") || normalized == "medium format"
    {
        RollCalibrationFormat::Film120
    } else if normalized == "loose" || normalized == "loose import" {
        RollCalibrationFormat::Loose
    } else {
        RollCalibrationFormat::Other
    }
}

fn build_roll_calibration_status(
    roll: &Roll,
    profiles: &[CalibrationProfileView],
    selected_state: Option<(PipelineState, bool, PipelineImageKind)>,
) -> RollCalibrationStatus {
    let format = roll_calibration_format(&roll.format);
    let requested_profile_id = roll.calibration_profile_id.clone();
    let requested_profile = requested_profile_id.as_deref().and_then(|profile_id| {
        profiles
            .iter()
            .find(|view| view.profile.profile_id == profile_id)
    });
    let (pipeline, frame_set, image_kind) = selected_state.unwrap_or_else(|| {
        (
            pipeline_state_for_roll_profile(roll, profiles),
            false,
            PipelineImageKind::RawBayer,
        )
    });
    let resolution = resolve_pipeline(&pipeline_resolver_input_for_kind(
        &pipeline,
        Some(roll),
        profiles,
        image_kind,
        None,
    ));
    let resolved_profile = resolution
        .resolved_profile_id
        .as_deref()
        .and_then(|profile_id| {
            profiles
                .iter()
                .find(|view| view.profile.profile_id == profile_id)
        });
    let fallback_to_smart_auto = resolution.requested_path
        == ProcessingContract::CaptureCorrectedV11
        && resolution.resolved_path != ProcessingContract::CaptureCorrectedV11;
    let legacy = resolution.resolved_path == ProcessingContract::LegacyV1;
    let tone = if pipeline.render_mapping.mode == RenderMode::FullTone {
        RollToneStatus::FullTone
    } else {
        RollToneStatus::Preserve
    };
    let base = if resolution.usable_density_anchors.has_roll_base() {
        RollBaseStatus::Sampled
    } else {
        RollBaseStatus::Estimated
    };
    let dmax = if resolution.usable_density_anchors.has_roll_full_exposure() {
        RollDmaxStatus::FullExposure
    } else {
        RollDmaxStatus::Unknown
    };
    let calibration = if legacy {
        RollCalibrationMode::Legacy
    } else if resolution.resolved_path == ProcessingContract::CaptureCorrectedV11 {
        RollCalibrationMode::Configured
    } else {
        RollCalibrationMode::SmartAuto
    };

    let mut warnings = Vec::new();
    if !frame_set {
        warnings.push("film_frame_not_set".to_string());
    }
    if let Some(view) = requested_profile {
        match view.availability {
            CalibrationProfileAvailability::Unsupported => {
                warnings.push("profile_unsupported_fallback".to_string())
            }
            CalibrationProfileAvailability::NeedsAttention => {
                warnings.push("profile_needs_attention_fallback".to_string())
            }
            CalibrationProfileAvailability::Available => {}
        }
    } else if requested_profile_id.is_some() {
        warnings.push("profile_missing_fallback".to_string());
    }
    if fallback_to_smart_auto {
        warnings.push(format!(
            "profile_capture_fallback|{}",
            resolution.processing_report.fallback_reasons.join(",")
        ));
    }
    if calibration == RollCalibrationMode::Configured {
        warnings.push("profile_capture_corrected_density_unvalidated".to_string());
    }
    if legacy {
        warnings.push("legacy_contract_preserved".to_string());
    }
    match format {
        RollCalibrationFormat::Film135 => match (
            resolution.usable_density_anchors.has_roll_base(),
            resolution.usable_density_anchors.has_roll_full_exposure(),
        ) {
            (false, false) => warnings.push("film135_references_missing".to_string()),
            (false, true) => warnings.push("film_base_missing".to_string()),
            (true, false) => warnings.push("full_exposure_missing".to_string()),
            (true, true) => {}
        },
        RollCalibrationFormat::Film120 => match (
            resolution.usable_density_anchors.has_roll_base(),
            resolution.usable_density_anchors.has_roll_full_exposure(),
        ) {
            (false, false) => warnings.push("film120_external_reference_recommended".to_string()),
            (false, true) => warnings.push("film120_base_missing".to_string()),
            (true, false) => warnings.push("film120_dmax_unknown".to_string()),
            (true, true) => {}
        },
        RollCalibrationFormat::Loose => warnings.push("loose_smart_auto".to_string()),
        RollCalibrationFormat::Other => warnings.push("roll_format_unknown".to_string()),
    }

    RollCalibrationStatus {
        roll_id: roll.roll_id.clone(),
        format,
        requested_profile_id,
        resolved_profile_id: resolved_profile.map(|view| view.profile.profile_id.clone()),
        profile_name: resolved_profile.map(|view| view.profile.name.clone()),
        fallback_to_smart_auto,
        frame: if frame_set {
            RollFrameStatus::Set
        } else {
            RollFrameStatus::NotSet
        },
        base,
        dmax,
        calibration,
        tone,
        warnings,
    }
}

#[tauri::command]
pub async fn update_roll_calibration_profile(
    roll_id: String,
    profile_id: Option<String>,
    state: State<'_, EngineState>,
) -> Result<RollCalibrationStatus, String> {
    let profile_id = profile_id
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    let profiles = tokio::task::spawn_blocking(load_calibration_profile_views)
        .await
        .map_err(|error| format!("Calibration profile worker failed: {error}"))??;
    if let Some(requested) = &profile_id {
        if !profiles
            .iter()
            .any(|view| view.profile.profile_id == *requested)
        {
            return Err("Calibration profile no longer exists.".to_string());
        }
    }

    let _mutation = state.roll_mutation.lock().await;
    let mut updated_rolls = read_lock(&state.rolls).clone();
    let roll = updated_rolls
        .iter_mut()
        .find(|roll| roll.roll_id == roll_id)
        .ok_or_else(|| format!("Roll not found: {roll_id}"))?;
    if roll_calibration_format(&roll.format) == RollCalibrationFormat::Loose && profile_id.is_some()
    {
        return Err("Loose Import uses Smart Auto and cannot bind a Profile.".to_string());
    }
    roll.calibration_profile_id = profile_id.clone();
    let updated_roll = roll.clone();
    let selected_template = pipeline_state_for_roll_profile(&updated_roll, &profiles);
    let pipeline_updates = state
        .items
        .iter()
        .filter_map(|entry| {
            let item = read_lock(entry.value());
            if item.roll_id != roll_id
                || item.pipeline_state.contract == ProcessingContract::LegacyV1
            {
                return None;
            }
            let mut next = selected_template.clone();
            next.content_range = item.pipeline_state.content_range.clone();
            next.render_mapping = item.pipeline_state.render_mapping.clone();
            Some((entry.key().clone(), item.file_path.clone(), next))
        })
        .collect::<Vec<_>>();
    let persisted_rolls = updated_rolls.clone();
    let persisted_profile_id = profile_id.clone();
    let persisted_pipeline_states = pipeline_updates
        .iter()
        .map(|(_, file_path, pipeline_state)| {
            (roll_id.clone(), file_path.clone(), pipeline_state.clone())
        })
        .collect::<Vec<_>>();
    tokio::task::spawn_blocking(move || {
        let mut connection = persistence::open_connection()
            .map_err(|error| format!("Failed to open calibration database: {error}"))?;
        persistence::save_rolls_profile_selection_and_pipeline_states(
            &mut connection,
            &persisted_rolls,
            persisted_profile_id.as_deref(),
            &persisted_pipeline_states,
        )
        .map_err(|error| format!("Failed to save Roll Profile selection: {error}"))?;
        update_rolls_compatibility_mirror(&persisted_rolls);
        Ok::<_, String>(())
    })
    .await
    .map_err(|error| format!("Roll Profile worker failed: {error}"))??;
    *write_lock(&state.rolls) = updated_rolls;
    for (item_id, _, pipeline_state) in pipeline_updates {
        if let Some(entry) = state.items.get(&item_id) {
            let mut item = write_lock(entry.value());
            item.pipeline_state = pipeline_state;
            item.runtime_pipeline_state = None;
            item.runtime_density_provenance = None;
            item.runtime_pipeline_key = None;
            item.original_proxy = None;
            item.proxy_image = None;
            item.prophoto_estimate_proxy = None;
            item.relative_transmission_proxy = None;
            item.relative_transmission_quality = None;
            item.pristine_proxy = None;
        }
    }
    let selected_state = selected_roll_item_state(&state, &roll_id, None);
    Ok(build_roll_calibration_status(
        &updated_roll,
        &profiles,
        selected_state,
    ))
}

#[tauri::command]
pub async fn get_roll_calibration_status(
    roll_id: String,
    image_id: Option<String>,
    state: State<'_, EngineState>,
) -> Result<RollCalibrationStatus, String> {
    let roll = read_lock(&state.rolls)
        .iter()
        .find(|roll| roll.roll_id == roll_id)
        .cloned()
        .ok_or_else(|| format!("Roll not found: {roll_id}"))?;
    let profiles = tokio::task::spawn_blocking(load_calibration_profile_views)
        .await
        .map_err(|error| format!("Calibration profile worker failed: {error}"))??;
    let selected_state = selected_roll_item_state(&state, &roll_id, image_id.as_deref());
    Ok(build_roll_calibration_status(
        &roll,
        &profiles,
        selected_state,
    ))
}

#[cfg(test)]
mod calibration_profile_contract_tests {
    use super::{
        build_capture_calibration_payload, build_roll_calibration_status,
        calibration_level_from_profile, calibration_profile_view, pipeline_resolver_input,
        pipeline_state_for_roll_profile, roll_calibration_format, sha256_bytes,
        CalibrationProfileInput, PipelineImageKind,
    };
    use crate::app_state::{
        CalibrationCapability, CalibrationConfigProfile, CalibrationLevel,
        CalibrationPayloadIssuer, CalibrationProfileAvailability, CalibrationProfilePayload,
        CalibrationProfileView, CalibrationQualityMaskArtifact, CalibrationQualityMaskSummary,
        CalibrationReference, CalibrationReferenceKind, CalibrationReferenceSummary,
        CalibrationValidRange, CalibrationValidationReport, CalibrationValidationStatus,
        CaptureCalibrationParameters, DensityAnchors, PipelineState, ProcessingContract, Roll,
        RollBaseStatus, RollCalibrationFormat, RollCalibrationMode, RollDmaxStatus,
        CALIBRATION_PROFILE_SCHEMA_VERSION, CAPTURE_CORRECTION_ALGORITHM_VERSION,
        CAPTURE_DEMOSAIC_ALGORITHM_VERSION,
    };
    use crate::persistence::RAW_DECODE_VERSION;
    use crate::raw_backend::{CaptureConditions, CfaPattern, RawMetadata, RawMosaic};
    use base64::{engine::general_purpose, Engine as _};

    fn roll(format: &str, profile_id: Option<&str>) -> Roll {
        Roll {
            roll_id: "roll-a".to_string(),
            date: String::new(),
            format: format.to_string(),
            film_stock: String::new(),
            camera: String::new(),
            image_paths: Vec::new(),
            density_anchors: DensityAnchors::default(),
            calibration_profile_id: profile_id.map(str::to_string),
            scanner_profile_id: None,
        }
    }

    fn profile(availability: CalibrationProfileAvailability) -> CalibrationProfileView {
        CalibrationProfileView {
            profile: CalibrationConfigProfile {
                profile_id: "profile-a".to_string(),
                schema_version: CALIBRATION_PROFILE_SCHEMA_VERSION,
                name: "Profile A".to_string(),
                created_at: 1,
                updated_at: 1,
                camera: String::new(),
                light_source: String::new(),
                lens: String::new(),
                calibration_level: CalibrationLevel::SmartAuto,
                notes: String::new(),
                references: Vec::new(),
                payload: CalibrationProfilePayload::default(),
            },
            availability,
            warnings: Vec::new(),
        }
    }

    fn reference(kind: CalibrationReferenceKind) -> CalibrationReference {
        CalibrationReference {
            reference_id: format!("reference-{kind:?}"),
            kind,
            file_path: std::env::current_exe()
                .unwrap()
                .to_string_lossy()
                .to_string(),
            file_name: "reference.raw".to_string(),
            file_size: 0,
            modified_at: None,
            added_at: 1,
        }
    }

    fn verified_capture_profile() -> CalibrationProfileView {
        let mut view = profile(CalibrationProfileAvailability::Available);
        view.profile.references = [
            CalibrationReferenceKind::DarkFrame,
            CalibrationReferenceKind::OpenGate,
            CalibrationReferenceKind::FlatField,
        ]
        .into_iter()
        .map(reference)
        .collect();
        let mask_bytes = vec![1 << 5, 0];
        let mut payload = CalibrationProfilePayload {
            issuer: CalibrationPayloadIssuer::BackendCalibrationSession,
            calibration_session_id: Some("session-a".to_string()),
            hardware_fingerprint: "camera|lens|light".to_string(),
            raw_decode_version: Some(RAW_DECODE_VERSION),
            libraw_version: "0.22-test".to_string(),
            reference_frames: view
                .profile
                .references
                .iter()
                .map(|reference| CalibrationReferenceSummary {
                    reference_id: reference.reference_id.clone(),
                    kind: reference.kind,
                    content_digest: format!("content-{}", reference.reference_id),
                    raw_metadata_digest: format!("metadata-{}", reference.reference_id),
                })
                .collect(),
            capture_parameters: Some(CaptureCalibrationParameters {
                correction_algorithm: CAPTURE_CORRECTION_ALGORITHM_VERSION.to_string(),
                demosaic_algorithm: CAPTURE_DEMOSAIC_ALGORITHM_VERSION.to_string(),
                epsilon: 1.0e-6,
                light_source_id: "light-a".to_string(),
                geometry_fingerprint: "geometry-a".to_string(),
            }),
            quality_mask: Some(CalibrationQualityMaskSummary {
                total_samples: 16,
                valid_samples: 15,
                bad_pixels: 1,
                bad_pixel_indices: vec![5],
                mask_artifact_digest: sha256_bytes(&mask_bytes),
                ..CalibrationQualityMaskSummary::default()
            }),
            mask_artifact: Some(CalibrationQualityMaskArtifact {
                encoding: "invalid_bitset_le_v1".to_string(),
                sample_count: 16,
                data_base64: general_purpose::STANDARD.encode(mask_bytes),
            }),
            valid_range: Some(CalibrationValidRange {
                minimum_transmission: [0.01; 3],
                maximum_transmission: [1.0; 3],
            }),
            capabilities: vec![CalibrationCapability::CaptureCorrected],
            validation_report: Some(CalibrationValidationReport {
                status: CalibrationValidationStatus::Passed,
                checked_at: Some(1),
                checks: vec!["capture_reference_consistency".to_string()],
                warnings: Vec::new(),
            }),
            ..CalibrationProfilePayload::default()
        };
        payload.payload_digest = payload.canonical_digest().unwrap();
        view.profile.payload = payload;
        view.profile.calibration_level = CalibrationLevel::Calibrated;
        view
    }

    fn synthetic_mosaic(value: u16) -> RawMosaic {
        RawMosaic {
            width: 4,
            height: 4,
            samples: vec![value; 16],
            metadata: RawMetadata {
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
                camera_id: "maker|camera".to_string(),
                libraw_version: "0.22-test".to_string(),
                capture_conditions: CaptureConditions::default(),
            },
        }
    }

    #[test]
    fn reference_presence_alone_does_not_promote_profile_level() {
        assert_eq!(
            calibration_level_from_profile(&CalibrationProfilePayload::default()),
            CalibrationLevel::SmartAuto
        );
    }

    #[test]
    fn normal_profile_input_cannot_forge_a_verified_payload() {
        let value = serde_json::json!({
            "profileId": null,
            "name": "Forged",
            "camera": "Camera",
            "lightSource": "Light",
            "references": [],
            "payload": {
                "issuer": "backend_calibration_session",
                "validation_report": { "status": "passed" }
            }
        });
        assert!(serde_json::from_value::<CalibrationProfileInput>(value).is_err());
    }

    #[test]
    fn backend_session_builds_self_verifying_payload_from_synthetic_mosaics() {
        let mut view = profile(CalibrationProfileAvailability::Available);
        view.profile.camera = "Session Camera".to_string();
        view.profile.light_source = "Session Light".to_string();
        view.profile.lens = "Session Lens".to_string();
        let summaries = [
            CalibrationReferenceKind::DarkFrame,
            CalibrationReferenceKind::OpenGate,
        ]
        .into_iter()
        .map(|kind| CalibrationReferenceSummary {
            reference_id: format!("summary-{kind:?}"),
            kind,
            content_digest: format!("content-{kind:?}"),
            raw_metadata_digest: format!("metadata-{kind:?}"),
        })
        .collect();
        let payload = build_capture_calibration_payload(
            &view.profile,
            &synthetic_mosaic(100),
            &synthetic_mosaic(1000),
            summaries,
            42,
            "session-synthetic".to_string(),
        )
        .expect("synthetic calibration payload");

        assert!(payload.capture_is_verified(RAW_DECODE_VERSION));
        assert_eq!(
            payload.calibration_session_id.as_deref(),
            Some("session-synthetic")
        );
        assert!(payload.mask_artifact.is_some());
        assert_eq!(payload.canonical_digest().unwrap(), payload.payload_digest);

        let mut tampered = payload.clone();
        tampered.hardware_fingerprint.push_str("-changed");
        assert_eq!(
            tampered.capture_validation_error(RAW_DECODE_VERSION),
            Some("capture_payload_digest_mismatch")
        );
    }

    #[test]
    fn preview_analysis_thumbnail_auto_invert_and_export_share_one_resolution() {
        let verified = verified_capture_profile();
        let roll = roll("135", Some("profile-a"));
        let persisted = pipeline_state_for_roll_profile(&roll, std::slice::from_ref(&verified));
        let raw_path =
            std::env::temp_dir().join(format!("nexfilm-resolver-{}.dng", std::process::id()));
        std::fs::write(&raw_path, b"resolver-only").unwrap();
        let input = pipeline_resolver_input(
            &persisted,
            Some(&roll),
            std::slice::from_ref(&verified),
            raw_path.to_string_lossy().as_ref(),
            None,
        );
        let resolutions = (0..5)
            .map(|_| crate::capability_resolver::resolve_pipeline(&input))
            .collect::<Vec<_>>();
        std::fs::remove_file(raw_path).unwrap();

        for resolution in &resolutions[1..] {
            assert_eq!(resolution, &resolutions[0]);
        }
        assert_eq!(
            resolutions[0].resolved_path,
            ProcessingContract::CaptureCorrectedV11
        );
    }

    #[test]
    fn legacy_profile_roll_anchors_are_hidden_and_do_not_set_the_level() {
        let mut view = profile(CalibrationProfileAvailability::Available);
        view.profile.references = vec![
            reference(CalibrationReferenceKind::FilmBase),
            reference(CalibrationReferenceKind::FullExposure),
        ];
        let normalized = calibration_profile_view(view.profile);

        assert!(normalized.profile.references.is_empty());
        assert_eq!(
            normalized.profile.calibration_level,
            CalibrationLevel::SmartAuto
        );
    }

    #[test]
    fn recognizes_135_and_120_subformats() {
        assert_eq!(
            roll_calibration_format("135 Half-frame"),
            RollCalibrationFormat::Film135
        );
        assert_eq!(
            roll_calibration_format("120 (6x7)"),
            RollCalibrationFormat::Film120
        );
        assert_eq!(
            roll_calibration_format("Loose"),
            RollCalibrationFormat::Loose
        );
    }

    #[test]
    fn missing_profile_keeps_request_and_falls_back_to_smart_auto() {
        let status = build_roll_calibration_status(
            &roll("135", Some("missing-profile")),
            &[],
            Some((
                PipelineState::smart_auto(),
                true,
                PipelineImageKind::RawBayer,
            )),
        );

        assert_eq!(
            status.requested_profile_id.as_deref(),
            Some("missing-profile")
        );
        assert_eq!(status.resolved_profile_id, None);
        assert!(status.fallback_to_smart_auto);
        assert_eq!(status.calibration, RollCalibrationMode::SmartAuto);
        assert!(status
            .warnings
            .iter()
            .any(|warning| warning == "profile_missing_fallback"));
    }

    #[test]
    fn available_but_unverified_profile_keeps_binding_and_falls_back() {
        let status = build_roll_calibration_status(
            &roll("120 (6x6)", Some("profile-a")),
            &[profile(CalibrationProfileAvailability::Available)],
            Some((
                PipelineState::smart_auto(),
                true,
                PipelineImageKind::RawBayer,
            )),
        );

        assert_eq!(status.requested_profile_id.as_deref(), Some("profile-a"));
        assert_eq!(status.resolved_profile_id, None);
        assert!(status.fallback_to_smart_auto);
        assert_eq!(status.calibration, RollCalibrationMode::SmartAuto);
        assert_eq!(status.base, RollBaseStatus::Estimated);
        assert_eq!(status.dmax, RollDmaxStatus::Unknown);
        assert!(status
            .warnings
            .iter()
            .any(|warning| warning.starts_with("profile_capture_fallback|")));

        let state = pipeline_state_for_roll_profile(
            &roll("120 (6x6)", Some("profile-a")),
            &[profile(CalibrationProfileAvailability::Available)],
        );
        assert_eq!(state.contract, ProcessingContract::CaptureCorrectedV11);
        assert!(state.processing_report.fallback_reasons.is_empty());
    }

    #[test]
    fn verified_capture_profile_selects_relative_transmission_without_density_claim() {
        let verified = verified_capture_profile();
        let roll = roll("120 (6x6)", Some("profile-a"));
        let state = pipeline_state_for_roll_profile(&roll, &[verified.clone()]);
        assert_eq!(state.contract, ProcessingContract::CaptureCorrectedV11);
        assert_eq!(
            state.contract.input_domain(),
            crate::app_state::DataDomain::RelativeTransmissionRgb
        );

        let status = build_roll_calibration_status(
            &roll,
            &[verified],
            Some((state, true, PipelineImageKind::RawBayer)),
        );
        assert_eq!(status.resolved_profile_id.as_deref(), Some("profile-a"));
        assert!(!status.fallback_to_smart_auto);
        assert_eq!(status.calibration, RollCalibrationMode::Configured);
        assert!(status
            .warnings
            .iter()
            .any(|warning| { warning == "profile_capture_corrected_density_unvalidated" }));
    }

    #[test]
    fn unavailable_profile_falls_back_without_erasing_its_id() {
        let status = build_roll_calibration_status(
            &roll("135", Some("profile-a")),
            &[profile(CalibrationProfileAvailability::NeedsAttention)],
            Some((
                PipelineState::smart_auto(),
                true,
                PipelineImageKind::RawBayer,
            )),
        );

        assert_eq!(status.requested_profile_id.as_deref(), Some("profile-a"));
        assert_eq!(status.resolved_profile_id, None);
        assert!(status.fallback_to_smart_auto);
    }
}

#[tauri::command]
pub async fn save_contact_sheet(
    data_url: String,
    filename: Option<String>,
) -> Result<String, String> {
    let b64_data = if data_url.starts_with("data:image/") {
        if let Some(idx) = data_url.find("base64,") {
            &data_url[idx + 7..]
        } else {
            return Err("Invalid data URL".into());
        }
    } else {
        &data_url
    }
    .to_string();
    let default_name = filename.unwrap_or_else(|| "contact_sheet.jpg".to_string());

    let file_path = tauri::async_runtime::spawn_blocking(move || {
        FileDialog::new()
            .set_file_name(&default_name)
            .add_filter("JPEG Image", &["jpg", "jpeg"])
            .save_file()
    })
    .await
    .map_err(|e| format!("Dialog error: {:?}", e))?;

    if let Some(path) = file_path {
        tauri::async_runtime::spawn_blocking(move || {
            let image_data = general_purpose::STANDARD
                .decode(b64_data)
                .map_err(|error| format!("Base64 decode failed: {error}"))?;
            write_bytes_atomically(&path, &image_data)?;
            Ok(path.to_string_lossy().to_string())
        })
        .await
        .map_err(|error| format!("Contact sheet worker failed: {error}"))?
    } else {
        Err("Cancelled".into())
    }
}

#[derive(Default, Serialize)]
pub struct DeleteRollsResult {
    pub removed_rolls: usize,
    pub removed_images: usize,
    pub removed_records: usize,
    pub deleted_source_files: usize,
    pub missing_source_files: usize,
    pub protected_source_files: usize,
    pub failed_source_files: Vec<String>,
}

fn process_source_paths<F>(
    source_paths: HashMap<String, String>,
    protected_paths: &HashSet<String>,
    result: &mut DeleteRollsResult,
    mut move_to_trash: F,
) where
    F: FnMut(&str) -> Result<(), String>,
{
    for (normalized, path) in source_paths {
        if protected_paths.contains(&normalized) {
            result.protected_source_files += 1;
            continue;
        }
        if !std::path::Path::new(&path).exists() {
            result.missing_source_files += 1;
            continue;
        }
        match move_to_trash(&path) {
            Ok(()) => result.deleted_source_files += 1,
            Err(error) => result.failed_source_files.push(format!("{path}: {error}")),
        }
    }
}

fn trash_source_paths(
    source_paths: HashMap<String, String>,
    protected_paths: &HashSet<String>,
    result: &mut DeleteRollsResult,
) {
    process_source_paths(source_paths, protected_paths, result, |path| {
        trash::delete(path).map_err(|error| error.to_string())
    });
}

#[tauri::command]
pub async fn delete_rolls(
    roll_ids: Vec<String>,
    delete_source_files: Option<bool>,
    state: State<'_, EngineState>,
) -> Result<DeleteRollsResult, String> {
    if roll_ids.is_empty() {
        return Ok(DeleteRollsResult::default());
    }

    let deleted_roll_ids: HashSet<&str> = roll_ids.iter().map(String::as_str).collect();
    let (remaining_rolls, source_paths, removed_roll_count) = {
        let _mutation = state.roll_mutation.lock().await;
        let rolls = read_lock(&state.rolls).clone();
        let mut paths = HashMap::new();
        for roll in rolls
            .iter()
            .filter(|roll| deleted_roll_ids.contains(roll.roll_id.as_str()))
        {
            for path in &roll.image_paths {
                paths
                    .entry(normalize_path(path))
                    .or_insert_with(|| path.clone());
            }
        }
        let remaining: Vec<Roll> = rolls
            .iter()
            .filter(|roll| !deleted_roll_ids.contains(roll.roll_id.as_str()))
            .cloned()
            .collect();
        let removed_roll_count = rolls.len().saturating_sub(remaining.len());
        let persisted_rolls = remaining.clone();
        let persisted_ids = roll_ids.clone();
        tokio::task::spawn_blocking(move || {
            let mut connection = persistence::open_connection()
                .map_err(|error| format!("Failed to open roll database: {error}"))?;
            persistence::delete_rolls_and_states(&mut connection, &persisted_ids, &persisted_rolls)
                .map_err(|error| format!("Failed to delete rolls: {error}"))?;
            update_rolls_compatibility_mirror(&persisted_rolls);
            Ok::<_, String>(())
        })
        .await
        .map_err(|error| format!("Roll deletion worker failed: {error}"))??;
        *write_lock(&state.rolls) = remaining.clone();
        (remaining, paths, removed_roll_count)
    };

    let mut source_paths = source_paths;
    let ids_to_remove: Vec<String> = state
        .items
        .iter()
        .filter_map(|entry| {
            let item = entry.value().read().ok()?;
            if deleted_roll_ids.contains(item.roll_id.as_str()) {
                source_paths
                    .entry(normalize_path(&item.file_path))
                    .or_insert_with(|| item.file_path.clone());
                Some(entry.key().clone())
            } else {
                None
            }
        })
        .collect();
    let removed_record_count = ids_to_remove.len();
    let removed_ids: HashSet<String> = ids_to_remove.iter().cloned().collect();
    for id in ids_to_remove {
        state.items.remove(&id);
    }
    state
        .item_order
        .write()
        .map_err(|error| error.to_string())?
        .retain(|id| !removed_ids.contains(id));
    state
        .proxy_loaded_order
        .write()
        .map_err(|error| error.to_string())?
        .retain(|id| !removed_ids.contains(id));
    {
        let mut active_id = state.active_id.write().map_err(|error| error.to_string())?;
        if active_id
            .as_ref()
            .is_some_and(|id| removed_ids.contains(id))
        {
            *active_id = None;
        }
    }

    let mut result = DeleteRollsResult {
        removed_rolls: removed_roll_count,
        removed_images: 0,
        removed_records: removed_record_count,
        deleted_source_files: 0,
        missing_source_files: 0,
        protected_source_files: 0,
        failed_source_files: Vec::new(),
    };
    if delete_source_files.unwrap_or(false) {
        let protected_paths: HashSet<String> = remaining_rolls
            .iter()
            .flat_map(|roll| roll.image_paths.iter())
            .map(|path| normalize_path(path))
            .chain(state.items.iter().filter_map(|entry| {
                let item = entry.value().read().ok()?;
                Some(normalize_path(&item.file_path))
            }))
            .collect();
        result = tokio::task::spawn_blocking(move || {
            trash_source_paths(source_paths, &protected_paths, &mut result);
            result
        })
        .await
        .map_err(|error| format!("Source deletion worker failed: {error}"))?;
    }
    Ok(result)
}

#[tauri::command]
pub async fn delete_images(
    images: Vec<ImageKey>,
    delete_source_files: Option<bool>,
    state: State<'_, EngineState>,
) -> Result<DeleteRollsResult, String> {
    let mut seen = HashSet::new();
    let images = images
        .into_iter()
        .filter(|image| !image.roll_id.is_empty() && !image.file_path.is_empty())
        .filter(|image| seen.insert((image.roll_id.clone(), normalize_path(&image.file_path))))
        .collect::<Vec<_>>();
    if images.is_empty() {
        return Ok(DeleteRollsResult::default());
    }

    let deleted_keys = images
        .iter()
        .map(|image| (image.roll_id.clone(), normalize_path(&image.file_path)))
        .collect::<HashSet<_>>();
    // Only paths proven to exist in NexFilm state are eligible for physical deletion.
    let mut source_paths = HashMap::new();

    let (updated_rolls, removed_from_rolls, removed_state_count) = {
        let _mutation = state.roll_mutation.lock().await;
        let mut updated = read_lock(&state.rolls).clone();
        let mut removed_from_rolls = 0;
        for roll in &mut updated {
            let roll_id = roll.roll_id.clone();
            roll.image_paths.retain(|path| {
                let should_remove = deleted_keys.contains(&(roll_id.clone(), normalize_path(path)));
                if should_remove {
                    removed_from_rolls += 1;
                    source_paths
                        .entry(normalize_path(path))
                        .or_insert_with(|| path.clone());
                }
                !should_remove
            });
        }

        let image_keys = images
            .iter()
            .map(|image| (image.roll_id.clone(), image.file_path.clone()))
            .collect::<Vec<_>>();
        let persisted_rolls = updated.clone();
        let removed_state_count = tokio::task::spawn_blocking(move || {
            let mut connection = persistence::open_connection()
                .map_err(|error| format!("Failed to open image database: {error}"))?;
            let removed = persistence::delete_images_and_update_rolls(
                &mut connection,
                &image_keys,
                &persisted_rolls,
            )
            .map_err(|error| format!("Failed to delete images: {error}"))?;
            update_rolls_compatibility_mirror(&persisted_rolls);
            Ok::<_, String>(removed)
        })
        .await
        .map_err(|error| format!("Image deletion worker failed: {error}"))??;
        *write_lock(&state.rolls) = updated.clone();
        (updated, removed_from_rolls, removed_state_count)
    };

    let ids_to_remove = state
        .items
        .iter()
        .filter_map(|entry| {
            let item = entry.value().read().ok()?;
            if deleted_keys.contains(&(item.roll_id.clone(), normalize_path(&item.file_path))) {
                source_paths
                    .entry(normalize_path(&item.file_path))
                    .or_insert_with(|| item.file_path.clone());
                Some(entry.key().clone())
            } else {
                None
            }
        })
        .collect::<Vec<_>>();
    let removed_ids = ids_to_remove.iter().cloned().collect::<HashSet<_>>();
    for id in &ids_to_remove {
        state.items.remove(id);
    }
    state
        .item_order
        .write()
        .map_err(|error| error.to_string())?
        .retain(|id| !removed_ids.contains(id));
    state
        .proxy_loaded_order
        .write()
        .map_err(|error| error.to_string())?
        .retain(|id| !removed_ids.contains(id));
    {
        let mut active_id = state.active_id.write().map_err(|error| error.to_string())?;
        if active_id
            .as_ref()
            .is_some_and(|id| removed_ids.contains(id))
        {
            *active_id = None;
        }
    }

    let mut result = DeleteRollsResult {
        removed_rolls: 0,
        removed_images: removed_from_rolls
            .max(removed_state_count)
            .max(ids_to_remove.len()),
        removed_records: ids_to_remove.len(),
        deleted_source_files: 0,
        missing_source_files: 0,
        protected_source_files: 0,
        failed_source_files: Vec::new(),
    };
    if delete_source_files.unwrap_or(false) {
        let protected_paths = updated_rolls
            .iter()
            .flat_map(|roll| roll.image_paths.iter())
            .map(|path| normalize_path(path))
            .chain(state.items.iter().filter_map(|entry| {
                let item = entry.value().read().ok()?;
                Some(normalize_path(&item.file_path))
            }))
            .collect::<HashSet<_>>();
        result = tokio::task::spawn_blocking(move || {
            trash_source_paths(source_paths, &protected_paths, &mut result);
            result
        })
        .await
        .map_err(|error| format!("Source deletion worker failed: {error}"))?;
    }
    Ok(result)
}

#[tauri::command]
pub async fn update_roll_metadata(
    roll_id: String,
    date: String,
    format: String,
    film_stock: String,
    camera: String,
    state: State<'_, EngineState>,
) -> Result<Roll, String> {
    if film_stock.trim().is_empty() {
        return Err("Film stock is required".to_string());
    }
    let (updated_roll, updated_rolls) = {
        let _mutation = state.roll_mutation.lock().await;
        let mut updated = read_lock(&state.rolls).clone();
        let roll = updated
            .iter_mut()
            .find(|roll| roll.roll_id == roll_id)
            .ok_or_else(|| format!("Roll not found: {roll_id}"))?;
        roll.date = date;
        roll.format = format;
        roll.film_stock = film_stock;
        roll.camera = camera;
        let updated_roll = roll.clone();
        let updated = persist_roll_snapshot_async(updated).await?;
        *write_lock(&state.rolls) = updated.clone();
        (updated_roll, updated)
    };
    let _ = updated_rolls;
    Ok(updated_roll)
}

#[tauri::command]
pub async fn update_roll_density_anchors(
    roll_id: String,
    base: Option<DensityAnchor>,
    full_exposure: Option<DensityAnchor>,
    state: State<'_, EngineState>,
) -> Result<DensityAnchors, String> {
    if base.is_none() && full_exposure.is_none() {
        return Err("Sample a film-base or film-leader reference first.".to_string());
    }
    let profiles = tokio::task::spawn_blocking(load_calibration_profile_views)
        .await
        .map_err(|error| format!("Calibration profile worker failed: {error}"))??;
    let requested_profile_id = state
        .rolls
        .read()
        .map_err(|error| error.to_string())?
        .iter()
        .find(|roll| roll.roll_id == roll_id)
        .and_then(|roll| roll.calibration_profile_id.clone());
    let requested_profile = requested_profile_id
        .as_deref()
        .and_then(|id| profiles.iter().find(|view| view.profile.profile_id == id));
    if requested_profile_id.is_some() && requested_profile.is_none() {
        return Err("Cannot save density anchors: bound Capture Profile is missing.".into());
    }
    if let Some(view) = requested_profile {
        if view.availability != CalibrationProfileAvailability::Available
            || !view.profile.payload.capture_is_verified(RAW_DECODE_VERSION)
        {
            return Err(format!(
                "Cannot save density anchors: bound Capture Profile is unavailable ({})",
                view.warnings.join(",")
            ));
        }
    }
    let validate_anchor = |anchor: &DensityAnchor,
                           expected_source: DensityAnchorSource|
     -> Result<(), String> {
        if anchor.source != expected_source {
            return Err(format!(
                "density_anchor_source_mismatch|expected={expected_source:?}"
            ));
        }
        if anchor.scope != DensityAnchorScope::Roll {
            return Err("density_anchor_scope_must_be_roll".into());
        }
        if anchor.confidence == DensityAnchorConfidence::Estimated {
            return Err("density_anchor_confidence_must_be_user_sampled_or_verified".into());
        }
        if anchor.density.iter().any(|value| !value.is_finite()) {
            return Err("density_anchor_values_must_be_finite".into());
        }
        if anchor.provenance.algorithm_version != crate::app_state::DENSITY_ANCHOR_ALGORITHM_VERSION
            || anchor.provenance.legacy
            || anchor
                .reference_id
                .as_deref()
                .is_none_or(|id| !id.starts_with(&(roll_id.clone() + ":")))
        {
            return Err("density_anchor_provenance_invalid".into());
        }
        if let Some(view) = requested_profile {
            if anchor.provenance.input_domain
                != crate::app_state::DataDomain::RelativeTransmissionRgb
                || anchor.provenance.calibration_profile_id.as_deref()
                    != Some(view.profile.profile_id.as_str())
                || anchor.provenance.calibration_payload_digest.as_deref()
                    != Some(view.profile.payload.payload_digest.as_str())
                || anchor.provenance.raw_decode_version != Some(RAW_DECODE_VERSION)
            {
                return Err("density_anchor_capture_profile_provenance_mismatch".into());
            }
        } else if anchor.provenance.input_domain != crate::app_state::DataDomain::ProPhotoEstimate
            || anchor.provenance.calibration_profile_id.is_some()
            || anchor.provenance.calibration_payload_digest.is_some()
        {
            return Err("density_anchor_prophoto_provenance_invalid".into());
        }
        Ok(())
    };
    if let Some(anchor) = &base {
        validate_anchor(anchor, DensityAnchorSource::SampledFilmBase)?;
    }
    if let Some(anchor) = &full_exposure {
        validate_anchor(anchor, DensityAnchorSource::SampledFullExposure)?;
    }
    if let (Some(base), Some(full)) = (&base, &full_exposure) {
        if (0..3).any(|channel| full.density[channel] <= base.density[channel] + 1e-4) {
            return Err(
                "The film-leader sample must be denser than the film-base sample.".to_string(),
            );
        }
    }
    let _mutation = state.roll_mutation.lock().await;
    let mut updated_rolls = read_lock(&state.rolls).clone();
    let roll = updated_rolls
        .iter_mut()
        .find(|roll| roll.roll_id == roll_id)
        .ok_or_else(|| format!("Roll not found: {roll_id}"))?;
    let mut anchors = roll.density_anchors.clone();
    if let Some(base) = base {
        replace_base_anchor_preserving_history(&mut anchors, base);
    }
    if let Some(full_exposure) = full_exposure {
        if let Some(previous) = anchors.d_max_full_exposure.replace(full_exposure) {
            if !anchors.retained_records.contains(&previous) {
                anchors.retained_records.push(previous);
            }
        }
    }
    if let (Some(base), Some(full)) = (
        anchors.d_min_base.as_ref(),
        anchors.d_max_full_exposure.as_ref(),
    ) {
        if (0..3).any(|channel| {
            !base.density[channel].is_finite()
                || !full.density[channel].is_finite()
                || full.density[channel] <= base.density[channel]
        }) {
            return Err("density_anchor_dmax_must_exceed_dmin".into());
        }
    }
    roll.density_anchors = anchors.clone();
    let capture_requested = roll.calibration_profile_id.is_some();

    let affected = state
        .items
        .iter()
        .filter_map(|entry| {
            let item = entry.value().read().ok()?;
            (item.roll_id == roll_id).then(|| {
                let mut pipeline = item.pipeline_state.clone();
                pipeline.density_anchors = anchors.clone();
                pipeline.contract = if capture_requested {
                    ProcessingContract::CaptureCorrectedV11
                } else {
                    anchors.prophoto_contract()
                };
                (entry.key().clone(), item.file_path.clone(), pipeline)
            })
        })
        .collect::<Vec<_>>();
    let persisted_rolls = updated_rolls.clone();
    let persisted_states = affected
        .iter()
        .map(|(_, path, pipeline)| (roll_id.clone(), path.clone(), pipeline.clone()))
        .collect::<Vec<_>>();
    tokio::task::spawn_blocking(move || {
        let mut connection = persistence::open_connection()
            .map_err(|error| format!("Failed to open calibration database: {error}"))?;
        persistence::save_rolls_and_pipeline_states(
            &mut connection,
            &persisted_rolls,
            &persisted_states,
        )
        .map_err(|error| format!("Failed to save density references: {error}"))?;
        update_rolls_compatibility_mirror(&persisted_rolls);
        Ok::<_, String>(())
    })
    .await
    .map_err(|error| format!("Calibration persistence worker failed: {error}"))??;

    *write_lock(&state.rolls) = updated_rolls;
    for (id, _, pipeline) in affected {
        if let Some(item) = state.items.get(&id) {
            let mut item = write_lock(item.value());
            item.pipeline_state = pipeline;
            item.runtime_pipeline_state = None;
            item.runtime_density_provenance = None;
            item.runtime_pipeline_key = None;
        }
    }
    Ok(anchors)
}

#[tauri::command]
pub async fn promote_roll(roll_id: String, state: State<'_, EngineState>) -> Result<(), String> {
    let roll = state
        .rolls
        .read()
        .map_err(|error| error.to_string())?
        .iter()
        .find(|roll| roll.roll_id == roll_id)
        .cloned()
        .ok_or("Roll not found")?;
    let promoted_ids = activate_library_roll(&state, &roll)?;
    let mut order = state
        .item_order
        .write()
        .map_err(|error| error.to_string())?;
    for id in promoted_ids {
        if !order.contains(&id) {
            order.push(id);
        }
    }
    Ok(())
}

#[tauri::command]
pub async fn append_to_roll(
    roll_id: String,
    paths: Vec<String>,
    state: State<'_, EngineState>,
    app_handle: tauri::AppHandle,
) -> Result<(), String> {
    {
        let _mutation = state.roll_mutation.lock().await;
        let mut updated = read_lock(&state.rolls).clone();
        let roll = updated
            .iter_mut()
            .find(|roll| roll.roll_id == roll_id)
            .ok_or_else(|| format!("Roll not found: {roll_id}"))?;
        for path in &paths {
            if !roll
                .image_paths
                .iter()
                .any(|existing| normalize_path(existing) == normalize_path(path))
            {
                roll.image_paths.push(path.clone());
            }
        }
        let updated = persist_roll_snapshot_async(updated).await?;
        *write_lock(&state.rolls) = updated;
    }
    crate::commands::import_images(
        paths,
        Some(false),
        Some(true),
        Some(roll_id),
        Some(false),
        Some(false),
        state,
        app_handle,
    )
    .await
}

#[tauri::command]
pub async fn locate_missing_file(
    id: String,
    state: State<'_, EngineState>,
) -> Result<String, String> {
    let file_path = tauri::async_runtime::spawn_blocking(|| {
        FileDialog::new()
            .set_title("Locate Missing File")
            .pick_file()
    })
    .await
    .map_err(|e| format!("Dialog error: {:?}", e))?;

    if let Some(path) = file_path {
        let new_path = path.to_string_lossy().to_string();
        let item_arc = state
            .items
            .get(&id)
            .map(|entry| entry.value().clone())
            .ok_or_else(|| format!("Image not found: {id}"))?;
        let (old_path, roll_id, is_loose) = {
            let item = item_arc.read().map_err(|error| error.to_string())?;
            (item.file_path.clone(), item.roll_id.clone(), item.is_loose)
        };
        let _mutation = state.roll_mutation.lock().await;
        {
            let item = read_lock(&item_arc);
            if item.file_path != old_path || item.roll_id != roll_id {
                return Err("Image changed while it was being relocated".to_string());
            }
        }

        if is_loose {
            let db_roll_id = roll_id.clone();
            let db_old_path = old_path.clone();
            let db_new_path = new_path.clone();
            tokio::task::spawn_blocking(move || {
                let connection = persistence::open_connection()
                    .map_err(|error| format!("Failed to open state database: {error}"))?;
                let updated = persistence::relocate_image_state(
                    &connection,
                    &db_roll_id,
                    &db_old_path,
                    &db_new_path,
                )
                .map_err(|error| format!("Failed to relocate image state: {error}"))?;
                if updated != 1 {
                    return Err("Persisted loose image state was not found".to_string());
                }
                Ok::<_, String>(())
            })
            .await
            .map_err(|error| format!("File relocation worker failed: {error}"))??;
            write_lock(&item_arc).file_path = new_path.clone();
            return Ok(new_path);
        }

        let updated_rolls = {
            let mut updated = read_lock(&state.rolls).clone();
            let roll = updated
                .iter_mut()
                .find(|roll| roll.roll_id == roll_id)
                .ok_or_else(|| format!("Owning roll not found: {roll_id}"))?;
            let position = roll
                .image_paths
                .iter()
                .position(|path| normalize_path(path) == normalize_path(&old_path))
                .ok_or_else(|| format!("Image is not registered in roll {roll_id}"))?;
            roll.image_paths[position] = new_path.clone();

            let db_roll_id = roll_id.clone();
            let db_old_path = old_path.clone();
            let db_new_path = new_path.clone();
            let db_rolls = updated.clone();
            tokio::task::spawn_blocking(move || {
                let mut connection = persistence::open_connection()
                    .map_err(|error| format!("Failed to open state database: {error}"))?;
                persistence::relocate_roll_image(
                    &mut connection,
                    &db_roll_id,
                    &db_old_path,
                    &db_new_path,
                    &db_rolls,
                )
                .map_err(|error| format!("Failed to relocate image state: {error}"))?;
                update_rolls_compatibility_mirror(&db_rolls);
                Ok::<_, String>(())
            })
            .await
            .map_err(|error| format!("File relocation worker failed: {error}"))??;
            write_lock(&item_arc).file_path = new_path.clone();
            *write_lock(&state.rolls) = updated.clone();
            updated
        };
        let _ = updated_rolls;
        Ok(new_path)
    } else {
        Err("Cancelled".into())
    }
}

pub fn init_db() -> rusqlite::Result<()> {
    let conn = persistence::open_connection()?;
    persistence::init_schema(&conn)
}

pub fn save_image_state_to_db(item: &crate::app_state::FilmItem) -> Result<(), String> {
    let conn = persistence::open_connection().map_err(|e| e.to_string())?;
    conn.busy_timeout(std::time::Duration::from_secs(5)).ok();
    let params_str = serde_json::to_string(&item.params).map_err(|e| e.to_string())?;
    let geom_str = serde_json::to_string(&item.geom).map_err(|e| e.to_string())?;
    let base_color_str = serde_json::to_string(&item.base_color).map_err(|e| e.to_string())?;
    let pipeline_state_str =
        serde_json::to_string(&item.pipeline_state).map_err(|e| e.to_string())?;

    conn.execute(
        "INSERT INTO image_states (
             roll_id, file_path, thumbnail_base64, embedded_thumb_base64,
             rendered_thumb_base64, params, geom, base_color,
             pipeline_state, math_version, raw_decode_version, updated_at
         )
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)
         ON CONFLICT(roll_id, file_path) DO UPDATE SET 
         thumbnail_base64=excluded.thumbnail_base64,
         embedded_thumb_base64=excluded.embedded_thumb_base64,
         rendered_thumb_base64=excluded.rendered_thumb_base64,
         params=excluded.params,
         geom=excluded.geom,
         base_color=excluded.base_color,
         pipeline_state=excluded.pipeline_state,
         math_version=excluded.math_version,
         raw_decode_version=excluded.raw_decode_version,
         updated_at=excluded.updated_at",
        rusqlite::params![
            item.roll_id,
            item.file_path,
            item.preferred_thumbnail(),
            item.embedded_thumbnail_base64,
            item.rendered_thumbnail_base64,
            params_str,
            geom_str,
            base_color_str,
            pipeline_state_str,
            persistence::math_version_for_contract(item.pipeline_state.contract),
            RAW_DECODE_VERSION,
            persistence::now_timestamp(),
        ],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

fn persist_single_image_update(
    roll_id: &str,
    file_path: &str,
    sql: &str,
    value: impl rusqlite::ToSql,
) -> Result<(), String> {
    let connection = persistence::open_connection()
        .map_err(|error| format!("Failed to open image database: {error}"))?;
    let changed = connection
        .execute(
            sql,
            rusqlite::params![value, persistence::now_timestamp(), roll_id, file_path],
        )
        .map_err(|error| format!("Failed to update image state: {error}"))?;
    if changed == 1 {
        Ok(())
    } else {
        Err(format!("Persisted image state was not found: {file_path}"))
    }
}

fn persist_tuning_parameters(
    roll_id: &str,
    file_path: &str,
    params: &TuningParams,
) -> Result<(), String> {
    let serialized = serde_json::to_string(params)
        .map_err(|error| format!("Failed to serialize tuning parameters: {error}"))?;
    persist_single_image_update(
        roll_id,
        file_path,
        "UPDATE image_states SET params = ?1, updated_at = ?2 WHERE roll_id = ?3 AND file_path = ?4",
        serialized,
    )
}

fn persist_geometry(roll_id: &str, file_path: &str, geom: &GeometryState) -> Result<(), String> {
    let serialized = serde_json::to_string(geom)
        .map_err(|error| format!("Failed to serialize geometry: {error}"))?;
    persist_single_image_update(
        roll_id,
        file_path,
        "UPDATE image_states SET geom = ?1, updated_at = ?2 WHERE roll_id = ?3 AND file_path = ?4",
        serialized,
    )
}

fn persist_base_color(
    roll_id: &str,
    file_path: &str,
    base_color: &BaseColor,
) -> Result<(), String> {
    let serialized = serde_json::to_string(base_color)
        .map_err(|error| format!("Failed to serialize base color: {error}"))?;
    persist_single_image_update(
        roll_id,
        file_path,
        "UPDATE image_states SET base_color = ?1, updated_at = ?2 WHERE roll_id = ?3 AND file_path = ?4",
        serialized,
    )
}

fn persist_base_and_pipeline(
    roll_id: &str,
    file_path: &str,
    base_color: &BaseColor,
    pipeline_state: &PipelineState,
) -> Result<(), String> {
    let connection = persistence::open_connection()
        .map_err(|error| format!("Failed to open image database: {error}"))?;
    let base = serde_json::to_string(base_color)
        .map_err(|error| format!("Failed to serialize base color: {error}"))?;
    let pipeline = serde_json::to_string(pipeline_state)
        .map_err(|error| format!("Failed to serialize pipeline state: {error}"))?;
    let changed = connection
        .execute(
            "UPDATE image_states
             SET base_color = ?1, pipeline_state = ?2, math_version = ?3, updated_at = ?4
             WHERE roll_id = ?5 AND file_path = ?6",
            rusqlite::params![
                base,
                pipeline,
                persistence::math_version_for_contract(pipeline_state.contract),
                persistence::now_timestamp(),
                roll_id,
                file_path,
            ],
        )
        .map_err(|error| format!("Failed to persist pipeline analysis: {error}"))?;
    if changed == 1 {
        Ok(())
    } else {
        Err(format!("Persisted image state was not found: {file_path}"))
    }
}

fn persist_pipeline_state(
    roll_id: &str,
    file_path: &str,
    pipeline_state: &PipelineState,
) -> Result<(), String> {
    let serialized = serde_json::to_string(pipeline_state)
        .map_err(|error| format!("Failed to serialize pipeline state: {error}"))?;
    persist_single_image_update(
        roll_id,
        file_path,
        "UPDATE image_states SET pipeline_state = ?1, updated_at = ?2 WHERE roll_id = ?3 AND file_path = ?4",
        serialized,
    )
}

fn persist_rendered_thumbnail(
    roll_id: &str,
    file_path: &str,
    thumbnail: &str,
) -> Result<(), String> {
    persist_single_image_update(
        roll_id,
        file_path,
        "UPDATE image_states SET rendered_thumb_base64 = ?1, thumbnail_base64 = ?1, updated_at = ?2 WHERE roll_id = ?3 AND file_path = ?4",
        thumbnail,
    )
}

type PersistedImageState = (
    String,
    crate::app_state::TuningParams,
    crate::app_state::GeometryState,
    crate::app_state::BaseColor,
    PipelineState,
);

fn load_image_state_from_connection(
    connection: &rusqlite::Connection,
    roll_id: &str,
    file_path: &str,
) -> Result<Option<PersistedImageState>, String> {
    let mut stmt = connection
        .prepare(
            "SELECT COALESCE(rendered_thumb_base64, embedded_thumb_base64, thumbnail_base64),
                COALESCE(length(rendered_thumb_base64), 0) > 0, params, geom, base_color, pipeline_state
         FROM image_states WHERE roll_id = ?1 AND file_path = ?2",
        )
        .map_err(|error| format!("Failed to prepare image-state read: {error}"))?;

    let mut rows = stmt
        .query(rusqlite::params![roll_id, file_path])
        .map_err(|error| format!("Failed to query image state: {error}"))?;
    if let Some(row) = rows
        .next()
        .map_err(|error| format!("Failed to read image state: {error}"))?
    {
        let thumb: String = row.get(0).map_err(|error| error.to_string())?;
        let has_rendered_thumbnail: bool = row.get(1).map_err(|error| error.to_string())?;
        let params_str: String = row.get(2).map_err(|error| error.to_string())?;
        let geom_str: String = row.get(3).map_err(|error| error.to_string())?;
        let base_color_str: String = row.get(4).map_err(|error| error.to_string())?;
        let pipeline_state_str: String = row.get(5).map_err(|error| error.to_string())?;

        let params = serde_json::from_str(&params_str)
            .map_err(|error| format!("Invalid persisted tuning parameters: {error}"))?;
        let geom = serde_json::from_str(&geom_str)
            .map_err(|error| format!("Invalid persisted geometry: {error}"))?;
        let base_color = serde_json::from_str(&base_color_str)
            .map_err(|error| format!("Invalid persisted base color: {error}"))?;
        let pipeline_state = serde_json::from_str(&pipeline_state_str)
            .map_err(|error| format!("Invalid persisted pipeline state: {error}"))?;

        return Ok(Some((
            thumb,
            params,
            normalize_persisted_geometry_for_rendered_image(geom, has_rendered_thumbnail),
            base_color,
            pipeline_state,
        )));
    }
    Ok(None)
}

pub fn load_image_state_from_db(roll_id: &str, file_path: &str) -> Option<PersistedImageState> {
    let connection = persistence::open_connection().ok()?;
    connection
        .busy_timeout(std::time::Duration::from_secs(5))
        .ok()?;
    load_image_state_from_connection(&connection, roll_id, file_path)
        .ok()
        .flatten()
}

fn load_all_image_states_from_connection(
    state: &crate::app_state::EngineState,
    connection: &rusqlite::Connection,
) -> Result<(), String> {
    let mut statement = connection
        .prepare(
            "SELECT roll_id, file_path,
                    COALESCE(embedded_thumb_base64, thumbnail_base64),
                    rendered_thumb_base64,
                    params, geom, base_color, pipeline_state
             FROM image_states",
        )
        .map_err(|error| format!("Failed to prepare image-state restore: {error}"))?;
    let rows = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, Option<String>>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, String>(5)?,
                row.get::<_, String>(6)?,
                row.get::<_, String>(7)?,
            ))
        })
        .map_err(|error| format!("Failed to query image-state restore: {error}"))?;
    let mut restored = Vec::new();
    for row in rows {
        let row = row.map_err(|error| format!("Failed to read image-state row: {error}"))?;
        let (
            roll_id,
            file_path,
            embedded_thumb,
            rendered_thumb,
            params,
            geom,
            base_color,
            pipeline_state_str,
        ) = row;
        let img_id = format!("img_{}", NEXT_ID.fetch_add(1, Ordering::SeqCst));
        let params = serde_json::from_str(&params)
            .map_err(|error| format!("Invalid tuning parameters for {file_path}: {error}"))?;
        let geom = serde_json::from_str(&geom)
            .map(|geom| {
                normalize_persisted_geometry_for_rendered_image(
                    geom,
                    rendered_thumb
                        .as_deref()
                        .is_some_and(|thumbnail| !thumbnail.is_empty()),
                )
            })
            .map_err(|error| format!("Invalid geometry for {file_path}: {error}"))?;
        let base_color = serde_json::from_str(&base_color)
            .map_err(|error| format!("Invalid base color for {file_path}: {error}"))?;
        let pipeline_state = serde_json::from_str(&pipeline_state_str)
            .map_err(|error| format!("Invalid pipeline state for {file_path}: {error}"))?;
        let item = FilmItem {
            id: img_id.clone(),
            is_loose: roll_id == "LOOSE_DEFAULT",
            roll_id,
            file_path,
            embedded_thumbnail_base64: embedded_thumb,
            rendered_thumbnail_base64: rendered_thumb,
            original_proxy: None,
            proxy_image: None,
            prophoto_estimate_proxy: None,
            relative_transmission_proxy: None,
            relative_transmission_quality: None,
            pristine_proxy: None,
            base_color,
            runtime_pipeline_state: None,
            runtime_density_provenance: None,
            runtime_pipeline_key: None,
            pipeline_state,
            params,
            geom,
            // Restored records belong to Rolls. A working Library is created
            // only by a new import, Promote, or Continue Editing.
            in_library: false,
        };
        restored.push((img_id, item));
    }
    let mut order = state
        .item_order
        .write()
        .map_err(|error| error.to_string())?;
    for (img_id, item) in restored {
        state
            .items
            .insert(img_id.clone(), Arc::new(RwLock::new(item)));
        order.push(img_id);
    }
    Ok(())
}

pub fn load_all_image_states(state: &crate::app_state::EngineState) -> Result<(), String> {
    let connection = persistence::open_connection()
        .map_err(|error| format!("Failed to open image-state database: {error}"))?;
    load_all_image_states_from_connection(state, &connection)
}

fn migrate_legacy_loose_roll(
    connection: &mut rusqlite::Connection,
    rolls: &mut Vec<Roll>,
) -> Result<bool, String> {
    if rolls.iter().any(|roll| roll.roll_id == "LOOSE_DEFAULT") {
        return Ok(false);
    }
    let paths = {
        let mut statement = connection
            .prepare(
                "SELECT file_path FROM image_states
                 WHERE roll_id = 'LOOSE_DEFAULT' ORDER BY updated_at, file_path",
            )
            .map_err(|error| format!("Failed to inspect legacy loose imports: {error}"))?;
        let paths = statement
            .query_map([], |row| row.get::<_, String>(0))
            .map_err(|error| format!("Failed to read legacy loose imports: {error}"))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| format!("Failed to read legacy loose import path: {error}"))?;
        paths
    };
    if paths.is_empty() {
        return Ok(false);
    }
    rolls.push(Roll {
        roll_id: "LOOSE_DEFAULT".to_string(),
        date: String::new(),
        format: "Loose".to_string(),
        film_stock: "Loose Import".to_string(),
        camera: String::new(),
        image_paths: paths,
        density_anchors: Default::default(),
        calibration_profile_id: None,
        scanner_profile_id: None,
    });
    persistence::save_rolls(connection, rolls)
        .map_err(|error| format!("Failed to migrate legacy loose imports: {error}"))?;
    Ok(true)
}

pub fn load_all_rolls(state: &crate::app_state::EngineState) -> Result<(), String> {
    let mut connection = persistence::open_connection()
        .map_err(|error| format!("Failed to open roll database: {error}"))?;
    let legacy_path = persistence::data_file("rolls.json");
    let legacy_rolls = match std::fs::read_to_string(&legacy_path) {
        Ok(json) => serde_json::from_str::<Vec<Roll>>(&json)
            .map_err(|error| format!("Failed to parse {}: {error}", legacy_path.display()))?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Vec::new(),
        Err(error) => {
            return Err(format!(
                "Failed to read legacy roll metadata from {}: {error}",
                legacy_path.display()
            ))
        }
    };
    persistence::migrate_legacy_rolls_if_empty(&mut connection, &legacy_rolls)
        .map_err(|error| format!("Failed to migrate legacy roll metadata: {error}"))?;
    let mut rolls = persistence::load_rolls(&connection)
        .map_err(|error| format!("Failed to load roll metadata: {error}"))?;
    if migrate_legacy_loose_roll(&mut connection, &mut rolls)? {
        update_rolls_compatibility_mirror(&rolls);
    }
    *state.rolls.write().map_err(|error| error.to_string())? = rolls;
    Ok(())
}

#[tauri::command]
pub async fn get_user_cameras() -> Result<Vec<String>, String> {
    tokio::task::spawn_blocking(|| {
        let conn = persistence::open_connection().map_err(|e| e.to_string())?;
        let mut stmt = conn
            .prepare("SELECT name FROM user_cameras ORDER BY name")
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([], |row| row.get(0))
            .map_err(|e| e.to_string())?;
        let mut cameras = Vec::new();
        for name_result in rows {
            cameras.push(name_result.map_err(|e| e.to_string())?);
        }
        Ok(cameras)
    })
    .await
    .map_err(|error| format!("Camera database worker failed: {error}"))?
}

#[tauri::command]
pub async fn get_user_films() -> Result<Vec<String>, String> {
    tokio::task::spawn_blocking(|| {
        let conn = persistence::open_connection().map_err(|e| e.to_string())?;
        let mut stmt = conn
            .prepare("SELECT name FROM user_films ORDER BY name")
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([], |row| row.get(0))
            .map_err(|e| e.to_string())?;
        let mut films = Vec::new();
        for name_result in rows {
            films.push(name_result.map_err(|e| e.to_string())?);
        }
        Ok(films)
    })
    .await
    .map_err(|error| format!("Film database worker failed: {error}"))?
}

#[tauri::command]
pub async fn add_user_camera(camera: String) -> Result<(), String> {
    tokio::task::spawn_blocking(move || {
        let conn = persistence::open_connection().map_err(|e| e.to_string())?;
        conn.execute(
            "INSERT OR IGNORE INTO user_cameras (name) VALUES (?1)",
            rusqlite::params![camera],
        )
        .map_err(|e| e.to_string())?;
        Ok(())
    })
    .await
    .map_err(|error| format!("Camera database worker failed: {error}"))?
}

#[tauri::command]
pub async fn add_user_film(film: String) -> Result<(), String> {
    tokio::task::spawn_blocking(move || {
        let conn = persistence::open_connection().map_err(|e| e.to_string())?;
        conn.execute(
            "INSERT OR IGNORE INTO user_films (name) VALUES (?1)",
            rusqlite::params![film],
        )
        .map_err(|e| e.to_string())?;
        Ok(())
    })
    .await
    .map_err(|error| format!("Film database worker failed: {error}"))?
}

pub fn generate_processed_thumbnail(item: &FilmItem) -> Option<String> {
    if item.pristine_proxy.is_none() {
        return None;
    }
    let params = &item.params;
    let base_color = &item.base_color;
    let exposure_offsets = if params.film_mode == FilmMode::BW {
        [params.exposure.exposure; 3]
    } else {
        [
            params.exposure.exposure + params.exposure.exp_r * CHANNEL_CONTROL_SCALE,
            params.exposure.exposure + params.exposure.exp_g * CHANNEL_CONTROL_SCALE,
            params.exposure.exposure + params.exposure.exp_b * CHANNEL_CONTROL_SCALE,
        ]
    };
    let effective_pipeline = item.effective_pipeline_state();
    let pipeline = FilmPipeline::from_state(
        effective_pipeline,
        base_color,
        exposure_offsets,
        params.film_mode.clone(),
    );

    let pristine = item.pristine_proxy.as_ref()?;
    let (width, height) = pristine.dimensions();
    let mut thumb_8bit = RgbImage::new(width, height);

    let pristine_pixels: &[f32] = pristine.as_raw().as_slice();
    let out_pixels: &mut [u8] = thumb_8bit.as_mut();

    let d_min = params.density.d_min;
    let d_max = params.density.d_max;
    let gamma = params.density.gamma;
    let highlights = params.tone.highlights;
    let shadows = params.tone.shadows;
    let (bw_dmin, bw_dmax) = neutral_density_bounds(d_min, d_max);
    let (saturation, temperature, tint) = if params.film_mode == FilmMode::Color {
        (
            params.tone.saturation,
            params.tone.temperature,
            params.tone.tint,
        )
    } else {
        (0.0, 0.0, 0.0)
    };
    let luma_coefficients = DENSITY_LUMA_COEFFICIENTS;
    let prophoto_to_srgb = (effective_pipeline.contract != ProcessingContract::LegacyV1
        && effective_pipeline.contract != ProcessingContract::CaptureCorrectedV11
        && params.film_mode == FilmMode::Color)
        .then(|| linear_conversion_matrix(ColorSpaceId::ProPhotoRgb, ColorSpaceId::SRgb));

    pristine_pixels
        .par_chunks(3)
        .zip(out_pixels.par_chunks_mut(3))
        .for_each(|(in_px, out_px)| {
            let true_density = [in_px[0], in_px[1], in_px[2]];
            let density = pipeline.apply_exposure(&true_density);

            let (effective_dmin, effective_dmax) = if params.film_mode == FilmMode::BW {
                ([bw_dmin; 3], [bw_dmax; 3])
            } else {
                (d_min, d_max)
            };
            let working_gamma = if prophoto_to_srgb.is_some() {
                1.0
            } else {
                gamma
            };
            let normalized = [
                normalize_density_channel(
                    density[0],
                    effective_dmin[0],
                    effective_dmax[0],
                    highlights,
                    shadows,
                    working_gamma,
                ),
                normalize_density_channel(
                    density[1],
                    effective_dmin[1],
                    effective_dmax[1],
                    highlights,
                    shadows,
                    working_gamma,
                ),
                normalize_density_channel(
                    density[2],
                    effective_dmin[2],
                    effective_dmax[2],
                    highlights,
                    shadows,
                    working_gamma,
                ),
            ];
            let gamma_corrected = if let Some(matrix) = prophoto_to_srgb {
                apply_linear_matrix(normalized, matrix)
                    .map(|value| value.clamp(0.0, 1.0).powf(1.0 / gamma.max(1e-6)))
            } else {
                normalized
            };
            let mut final_rgb = apply_post_gamma_adjustments_with_luma(
                gamma_corrected,
                0.0,
                0.0,
                saturation,
                temperature,
                tint,
                luma_coefficients,
            );
            if params.film_mode == FilmMode::BW {
                let luma = final_rgb
                    .iter()
                    .zip(luma_coefficients)
                    .map(|(value, coefficient)| value * coefficient)
                    .sum();
                final_rgb = [luma; 3];
            }

            // `final_rgb` is already the display-referred sRGB grading signal.
            // Encoding it again would lift midtones and wash out the image.
            out_px[0] = (final_rgb[0].clamp(0.0, 1.0) * 255.0).round() as u8;
            out_px[1] = (final_rgb[1].clamp(0.0, 1.0) * 255.0).round() as u8;
            out_px[2] = (final_rgb[2].clamp(0.0, 1.0) * 255.0).round() as u8;
        });

    let (orig_width, orig_height) = (width, height);
    let cx = (item.geom.crop_rect.x * orig_width as f32)
        .max(0.0)
        .min(orig_width as f32) as u32;
    let cy = (item.geom.crop_rect.y * orig_height as f32)
        .max(0.0)
        .min(orig_height as f32) as u32;
    let cw = (item.geom.crop_rect.width * orig_width as f32)
        .max(1.0)
        .min((orig_width - cx) as f32) as u32;
    let ch = (item.geom.crop_rect.height * orig_height as f32)
        .max(1.0)
        .min((orig_height - cy) as f32) as u32;

    let mut cropped_thumb = thumb_8bit;
    if cw < orig_width || ch < orig_height {
        cropped_thumb = image::imageops::crop(&mut cropped_thumb, cx, cy, cw, ch).to_image();
    }

    let ratio_thumb = 1024.0 / (cw.max(ch) as f32);
    let thumb_width = (cw as f32 * ratio_thumb).max(1.0) as u32;
    let thumb_height = (ch as f32 * ratio_thumb).max(1.0) as u32;
    let thumb = image::imageops::resize(
        &cropped_thumb,
        thumb_width,
        thumb_height,
        FilterType::Nearest,
    );

    let mut cursor = std::io::Cursor::new(Vec::new());
    if let Ok(_) = thumb.write_to(&mut cursor, image::ImageOutputFormat::Jpeg(70)) {
        use base64::{engine::general_purpose, Engine as _};
        return Some(general_purpose::STANDARD.encode(cursor.into_inner()));
    }
    None
}

#[cfg(test)]
mod import_contract_tests {
    use super::{
        apply_roll_density_anchor_limits, compute_auto_base, compute_auto_base_f32,
        compute_auto_color_limits, compute_content_limits_f32, decode_image_buffer,
        decode_import_preview_base64,
        decode_reduced_dng_for_working_space, decode_reduced_tiff_for_working_space,
        decode_tiff_for_smart_auto, default_pipeline_state_for_import, is_better_preview_edge,
        is_lightweight_direct_preview, is_noritsu_rendered_image, is_raw_extension,
        is_scanner_fff_tiff, is_tiff_extension, libraw_decode_error_message, linearize_scanner_fff,
        persist_import_batch, pipeline_base_density, pipeline_has_base,
        preserve_smart_auto_content_span, preserve_tone_density_span,
        prophoto_estimate_to_transport_proxy, raw_decode_failure_hint, reference_density_extreme,
        render_f32_shader_equivalent, render_shader_equivalent, rgb16_image_from_bytes,
        share_smart_auto_density_scale, AutoColorLimits, DecodeMode, IMPORT_PREVIEW_LONG_EDGE,
        PROPHOTO_TRANSPORT_MAX, PROPHOTO_TRANSPORT_MIN,
    };
    use crate::app_state::{
        BaseColor, DensityAnchor, DensityAnchorConfidence, DensityAnchorScope, DensityAnchorSource,
        DensityAnchors, FilmItem, FilmMode, GeometryState, PipelineState, ProcessingContract, Roll,
        TuningParams,
    };
    use crate::color_science::{
        apply_linear_matrix, compress_linear_srgb_for_density, linear_conversion_matrix,
        ColorSpaceId, DENSITY_CAPTURE_PROFILE,
    };
    use base64::Engine as _;
    use image::{ImageBuffer, Rgb};
    use rayon::prelude::*;

    #[test]
    fn import_only_directly_decodes_small_encoded_images() {
        assert!(is_lightweight_direct_preview("frame.jpg"));
        assert!(is_lightweight_direct_preview("frame.PNG"));
        assert!(!is_lightweight_direct_preview("frame.tiff"));
        assert!(!is_lightweight_direct_preview("frame.dng"));
    }

    #[test]
    fn jpeg_and_png_inputs_decode_to_linear_pixels_and_use_geometry_transforms() {
        let root = std::env::temp_dir().join(format!(
            "nexfilm-direct-input-test-{}-{}",
            std::process::id(),
            super::NEXT_ID.fetch_add(1, std::sync::atomic::Ordering::SeqCst)
        ));
        std::fs::create_dir(&root).unwrap();
        let source = image::ImageBuffer::from_pixel(4, 2, image::Rgb([128, 64, 192]));
        for (extension, format) in [
            ("png", image::ImageOutputFormat::Png),
            ("jpg", image::ImageOutputFormat::Jpeg(100)),
        ] {
            let path = root.join(format!("source.{extension}"));
            let mut bytes = std::io::Cursor::new(Vec::new());
            image::DynamicImage::ImageRgb8(source.clone())
                .write_to(&mut bytes, format)
                .unwrap();
            std::fs::write(&path, bytes.into_inner()).unwrap();

            let decoded =
                decode_image_buffer(path.to_string_lossy().as_ref(), DecodeMode::DevelopProxy)
                    .unwrap();
            // 128/255 sRGB decodes to approximately 0.216 linear. JPEG's
            // quantization gets a wider tolerance than the lossless PNG.
            let red = decoded.get_pixel(0, 0)[0];
            assert!((11_000..=18_000).contains(&red), "{extension}: {red}");

            let mut geom = GeometryState::default();
            geom.crop_rect.x = 0.0;
            geom.crop_rect.y = 0.0;
            geom.crop_rect.width = 0.5;
            geom.crop_rect.height = 1.0;
            let transformed = render_shader_equivalent(
                &decoded,
                &TuningParams::default(),
                &geom,
                &BaseColor::default(),
                None,
            );
            assert_eq!(transformed.dimensions(), (2, 2), "{extension}");
            let mut tuned = TuningParams::default();
            tuned.exposure.exposure = 1.0;
            let tuned_output =
                render_shader_equivalent(&decoded, &tuned, &geom, &BaseColor::default(), None);
            assert_ne!(transformed.as_raw(), tuned_output.as_raw(), "{extension}");
        }
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn scanner_fff_linearization_removes_the_standard_1_8_transfer_curve() {
        let encoded = image::ImageBuffer::from_pixel(1, 1, image::Rgb([32768u16; 3]));
        let linear = linearize_scanner_fff(encoded, ColorSpaceId::SRgb);
        assert!((18_000..=20_000).contains(&linear.get_pixel(0, 0)[0]));
    }

    #[test]
    fn scanner_fff_classifier_requires_a_flextight_or_imacon_scanner_identity() {
        let root = std::env::temp_dir().join(format!(
            "nexfilm-fff-classifier-test-{}-{}",
            std::process::id(),
            super::NEXT_ID.fetch_add(1, std::sync::atomic::Ordering::SeqCst)
        ));
        std::fs::create_dir(&root).unwrap();
        let write_fixture = |name: &str, identity: &[u8]| {
            let path = root.join(name);
            let mut bytes = b"MM\0*\0\0\0\x08".to_vec();
            bytes.extend_from_slice(identity);
            std::fs::write(&path, bytes).unwrap();
            path
        };

        let flextight = write_fixture("flextight.fff", b"ColorModel: Flextight X5 & 949");
        let imacon = write_fixture("imacon.fff", b"Imacon film scanner");
        let camera = write_fixture("camera.fff", b"Hasselblad CFV 100C/907X");
        let generic = write_fixture("generic.fff", b"generic TIFF RGB image");
        let wrong_extension = write_fixture("flextight.tiff", b"Flextight X5");

        assert!(is_scanner_fff_tiff(flextight.to_string_lossy().as_ref()));
        assert!(is_scanner_fff_tiff(imacon.to_string_lossy().as_ref()));
        assert!(!is_scanner_fff_tiff(camera.to_string_lossy().as_ref()));
        assert!(!is_scanner_fff_tiff(generic.to_string_lossy().as_ref()));
        assert!(!is_scanner_fff_tiff(
            wrong_extension.to_string_lossy().as_ref()
        ));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn supplied_scanner_and_camera_fff_fixtures_are_routed_separately() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("test_picture");
        for path in [
            root.join("图像 001.fff"),
            root.join("哈苏fff").join("1 001-可以反相.fff"),
            root.join("哈苏fff").join("无法反相.fff"),
        ] {
            if path.exists() {
                assert!(
                    is_scanner_fff_tiff(path.to_string_lossy().as_ref()),
                    "{} must use the scanner FFF pipeline",
                    path.display()
                );
            }
        }
        for path in [
            root.join("哈苏fff").join("任务 _1233.fff"),
            root.join("哈苏fff").join("任务 _1343.fff"),
        ] {
            if path.exists() {
                assert!(
                    !is_scanner_fff_tiff(path.to_string_lossy().as_ref()),
                    "{} must use the camera RAW pipeline",
                    path.display()
                );
            }
        }
    }

    #[test]
    #[ignore = "large user-supplied CFV-100C fixtures; run explicitly for RAW pipeline validation"]
    fn hasselblad_cfv_100c_fff_fixtures_decode_through_the_camera_raw_pipeline() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("test_picture")
            .join("哈苏fff");
        for path in [root.join("任务 _1233.fff"), root.join("任务 _1343.fff")] {
            if !path.exists() {
                continue;
            }
            assert!(
                !is_scanner_fff_tiff(path.to_string_lossy().as_ref()),
                "{} must not use the scanner FFF pipeline",
                path.display()
            );
            let decoded =
                decode_image_buffer(path.to_string_lossy().as_ref(), DecodeMode::DevelopProxy)
                    .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
            assert!(
                decoded.width() > 4000 && decoded.height() > 3000,
                "{} returned an embedded preview instead of a RAW proxy: {:?}",
                path.display(),
                decoded.dimensions()
            );
            assert!(
                decoded.as_raw().iter().any(|value| *value > 0),
                "{} produced a black RAW proxy",
                path.display()
            );
        }
    }

    #[derive(Debug)]
    struct AbEndpointStats {
        zero: [u64; 3],
        maximum: [u64; 3],
        pixels: u64,
    }

    fn ab_endpoint_stats(image: &image::ImageBuffer<image::Rgb<u16>, Vec<u16>>) -> AbEndpointStats {
        let mut stats = AbEndpointStats {
            zero: [0; 3],
            maximum: [0; 3],
            pixels: (image.width() as u64) * (image.height() as u64),
        };
        for pixel in image.as_raw().chunks_exact(3) {
            for channel in 0..3 {
                stats.zero[channel] += u64::from(pixel[channel] == 0);
                stats.maximum[channel] += u64::from(pixel[channel] == u16::MAX);
            }
        }
        stats
    }

    fn ab_compress_to_positive_linear_srgb(rgb: [f32; 3]) -> [f32; 3] {
        const FLOOR: f32 = 1.0 / 65_535.0;
        const CEILING: f32 = 1.0 - FLOOR;
        let luma = crate::core_math::density_luma(rgb);
        if luma <= FLOOR {
            return [FLOOR; 3];
        }
        if luma >= CEILING {
            return [CEILING; 3];
        }

        // Move only along the chroma vector toward the neutral axis. This
        // preserves linear-sRGB luminance while fitting every channel into the
        // positive transmission domain required by the logarithmic film math.
        let mut scale = 1.0f32;
        for channel in rgb {
            if channel < FLOOR {
                scale = scale.min((luma - FLOOR) / (luma - channel));
            } else if channel > CEILING {
                scale = scale.min((CEILING - luma) / (channel - luma));
            }
        }
        rgb.map(|channel| (luma + (channel - luma) * scale).clamp(FLOOR, CEILING))
    }

    fn ab_compress_to_density_safe_linear_srgb(rgb: [f32; 3]) -> [f32; 3] {
        const ABSOLUTE_FLOOR: f32 = 1.0 / 65_535.0;
        const CHROMA_FLOOR_RATIO: f32 = 0.01;
        const CEILING: f32 = 1.0 - ABSOLUTE_FLOOR;
        let luma = crate::core_math::density_luma(rgb);
        let floor = ABSOLUTE_FLOOR.max(luma.max(0.0) * CHROMA_FLOOR_RATIO);
        if luma <= floor {
            return [floor; 3];
        }
        if luma >= CEILING {
            return [CEILING; 3];
        }

        // Keep the luminance axis fixed, but do not allow an out-of-gamut
        // channel to become an extreme density spike after -log10().
        let mut scale = 1.0f32;
        for channel in rgb {
            if channel < floor {
                scale = scale.min((luma - floor) / (luma - channel));
            } else if channel > CEILING {
                scale = scale.min((CEILING - luma) / (channel - luma));
            }
        }
        rgb.map(|channel| (luma + (channel - luma) * scale).clamp(floor, CEILING))
    }

    fn ab_render_variant(
        output_root: &std::path::Path,
        frame: &str,
        variant: &str,
        source: &image::ImageBuffer<image::Rgb<u16>, Vec<u16>>,
    ) -> ([u16; 3], [f32; 3], [f32; 3]) {
        let base = compute_auto_base(source);
        let limits = compute_auto_color_limits(
            source,
            &GeometryState::default(),
            &base,
            FilmMode::Color,
            false,
        )
        .unwrap_or_else(|error| panic!("{frame}/{variant}: {error}"));
        let mut params = TuningParams::default();
        params.density.d_min = limits.d_min;
        params.density.d_max = limits.d_max;
        let rendered =
            render_shader_equivalent(source, &params, &GeometryState::default(), &base, None);
        let frame_root = output_root.join(frame);
        std::fs::create_dir_all(&frame_root).unwrap();
        source
            .save(frame_root.join(format!("{variant}-negative.png")))
            .unwrap();
        rendered
            .save(frame_root.join(format!("{variant}-positive.png")))
            .unwrap();
        (
            [base.base_r, base.base_g, base.base_b],
            limits.d_min,
            limits.d_max,
        )
    }

    fn ab_decode_raw_transport(
        path: &std::path::Path,
        output_color: i32,
    ) -> image::ImageBuffer<image::Rgb<u16>, Vec<u16>> {
        let options = crate::raw_backend::DecodeOptions {
            half_size: true,
            demosaic_quality: 3,
            output_bps: 16,
            no_auto_bright: true,
            output_color,
            linear_gamma: true,
            use_camera_wb: true,
        };
        let decoded = crate::raw_backend::RawProcessor::extract_image_with_options(path, &options)
            .unwrap_or_else(|error| {
                panic!("{} (output_color={output_color}): {error}", path.display())
            });
        rgb16_image_from_bytes(
            decoded.width as u32,
            decoded.height as u32,
            decoded.colors as usize,
            decoded.bits,
            &decoded.data,
        )
        .unwrap_or_else(|error| panic!("{} (output_color={output_color}): {error}", path.display()))
    }

    fn ab_decode_camera_f32(
        path: &std::path::Path,
    ) -> image::ImageBuffer<image::Rgb<u16>, Vec<u16>> {
        let options = crate::raw_backend::DecodeOptions {
            half_size: true,
            demosaic_quality: 3,
            output_bps: 16,
            no_auto_bright: true,
            output_color: 0,
            linear_gamma: true,
            use_camera_wb: true,
        };
        let decoded = crate::raw_backend::extract_camera_rgb_with_options(path, &options)
            .unwrap_or_else(|error| panic!("{} (camera-rgb): {error}", path.display()));
        let camera = rgb16_image_from_bytes(
            decoded.width as u32,
            decoded.height as u32,
            decoded.colors as usize,
            decoded.bits,
            &decoded.data,
        )
        .unwrap_or_else(|error| panic!("{} (camera-rgb): {error}", path.display()));
        let mut output = image::ImageBuffer::new(camera.width(), camera.height());
        output
            .as_mut()
            .par_chunks_exact_mut(3)
            .zip(camera.as_raw().par_chunks_exact(3))
            .for_each(|(target, pixel)| {
                let signed = apply_linear_matrix(
                    [
                        pixel[0] as f32 / 65535.0,
                        pixel[1] as f32 / 65535.0,
                        pixel[2] as f32 / 65535.0,
                    ],
                    decoded.camera_to_srgb,
                );
                let rgb = compress_linear_srgb_for_density(signed);
                for channel in 0..3 {
                    target[channel] = (rgb[channel] * 65535.0).round() as u16;
                }
            });
        output
    }

    #[test]
    #[ignore = "manual CFV-100C gamut-transport A/B; writes diagnostic PNGs under target"]
    fn hasselblad_cfv_100c_prophoto_transport_ab() {
        const DIAGNOSTIC_EDGE: u32 = 1600;
        let fixture_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("test_picture")
            .join("哈苏fff");
        let output_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join("fff-gamut-ab");
        std::fs::create_dir_all(&output_root).unwrap();

        let transport_to_srgb =
            linear_conversion_matrix(ColorSpaceId::ProPhotoRgbD65, DENSITY_CAPTURE_PROFILE);
        for (frame, path) in [
            ("1233", fixture_root.join("任务 _1233.fff")),
            ("1343", fixture_root.join("任务 _1343.fff")),
        ] {
            if !path.exists() {
                continue;
            }
            let path_text = path.to_string_lossy();
            let current = decode_image_buffer(&path_text, DecodeMode::DevelopProxy)
                .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
            let current_stats = ab_endpoint_stats(&current);
            let current_small =
                image::imageops::resize(
                    &current,
                    DIAGNOSTIC_EDGE,
                    ((DIAGNOSTIC_EDGE as f64 * current.height() as f64 / current.width() as f64)
                        .round() as u32)
                        .max(1),
                    image::imageops::FilterType::Triangle,
                );
            drop(current);
            let current_auto =
                ab_render_variant(&output_root, frame, "a-current-srgb", &current_small);

            let transport = ab_decode_raw_transport(path.as_path(), 4);
            let transport_stats = ab_endpoint_stats(&transport);

            let mut signed_min = [f32::INFINITY; 3];
            let mut signed_max = [f32::NEG_INFINITY; 3];
            let mut signed_below_zero = [0u64; 3];
            let mut signed_above_one = [0u64; 3];
            for pixel in transport.as_raw().chunks_exact(3) {
                let converted = apply_linear_matrix(
                    [
                        pixel[0] as f32 / 65_535.0,
                        pixel[1] as f32 / 65_535.0,
                        pixel[2] as f32 / 65_535.0,
                    ],
                    transport_to_srgb,
                );
                for channel in 0..3 {
                    signed_min[channel] = signed_min[channel].min(converted[channel]);
                    signed_max[channel] = signed_max[channel].max(converted[channel]);
                    signed_below_zero[channel] += u64::from(converted[channel] < 0.0);
                    signed_above_one[channel] += u64::from(converted[channel] > 1.0);
                }
            }

            let transport_small = image::imageops::resize(
                &transport,
                DIAGNOSTIC_EDGE,
                ((DIAGNOSTIC_EDGE as f64 * transport.height() as f64 / transport.width() as f64)
                    .round() as u32)
                    .max(1),
                image::imageops::FilterType::Triangle,
            );
            drop(transport);
            let mut direct = image::ImageBuffer::<image::Rgb<u16>, Vec<u16>>::new(
                transport_small.width(),
                transport_small.height(),
            );
            let mut compressed = image::ImageBuffer::<image::Rgb<u16>, Vec<u16>>::new(
                transport_small.width(),
                transport_small.height(),
            );
            let mut density_safe = image::ImageBuffer::<image::Rgb<u16>, Vec<u16>>::new(
                transport_small.width(),
                transport_small.height(),
            );
            for ((source, direct_pixel), compressed_pixel) in transport_small
                .pixels()
                .zip(direct.pixels_mut())
                .zip(compressed.pixels_mut())
            {
                let signed = apply_linear_matrix(
                    [
                        source[0] as f32 / 65_535.0,
                        source[1] as f32 / 65_535.0,
                        source[2] as f32 / 65_535.0,
                    ],
                    transport_to_srgb,
                );
                *direct_pixel = image::Rgb(
                    signed.map(|value| (value.clamp(0.0, 1.0) * 65_535.0).round() as u16),
                );
                *compressed_pixel = image::Rgb(
                    ab_compress_to_positive_linear_srgb(signed)
                        .map(|value| (value * 65_535.0).round() as u16),
                );
            }
            for (source, density_safe_pixel) in
                transport_small.pixels().zip(density_safe.pixels_mut())
            {
                let signed = apply_linear_matrix(
                    [
                        source[0] as f32 / 65_535.0,
                        source[1] as f32 / 65_535.0,
                        source[2] as f32 / 65_535.0,
                    ],
                    transport_to_srgb,
                );
                *density_safe_pixel = image::Rgb(
                    ab_compress_to_density_safe_linear_srgb(signed)
                        .map(|value| (value * 65_535.0).round() as u16),
                );
            }
            let direct_stats = ab_endpoint_stats(&direct);
            let compressed_stats = ab_endpoint_stats(&compressed);
            let density_safe_stats = ab_endpoint_stats(&density_safe);
            let direct_auto =
                ab_render_variant(&output_root, frame, "b-transport-direct-clamp", &direct);
            let compressed_auto =
                ab_render_variant(&output_root, frame, "c-transport-compressed", &compressed);
            let density_safe_auto = ab_render_variant(
                &output_root,
                frame,
                "d-transport-density-safe",
                &density_safe,
            );

            println!("frame={frame} current endpoints={current_stats:?} auto={current_auto:?}");
            println!(
                "frame={frame} transport endpoints={transport_stats:?} signed_min={signed_min:?} signed_max={signed_max:?} below_zero={signed_below_zero:?} above_one={signed_above_one:?}"
            );
            println!("frame={frame} direct endpoints={direct_stats:?} auto={direct_auto:?}");
            println!(
                "frame={frame} compressed endpoints={compressed_stats:?} auto={compressed_auto:?}"
            );
            println!(
                "frame={frame} density-safe endpoints={density_safe_stats:?} auto={density_safe_auto:?}"
            );

            for (variant, output_color) in [("e-camera-rgb", 0), ("f-aces-ap0", 6)] {
                let raw = ab_decode_raw_transport(path.as_path(), output_color);
                let raw_stats = ab_endpoint_stats(&raw);
                let raw_small = image::imageops::resize(
                    &raw,
                    DIAGNOSTIC_EDGE,
                    ((DIAGNOSTIC_EDGE as f64 * raw.height() as f64 / raw.width() as f64).round()
                        as u32)
                        .max(1),
                    image::imageops::FilterType::Triangle,
                );
                let raw_auto = ab_render_variant(&output_root, frame, variant, &raw_small);
                println!("frame={frame} {variant} endpoints={raw_stats:?} auto={raw_auto:?}");
            }

            let camera_f32 = ab_decode_camera_f32(path.as_path());
            let camera_f32_stats = ab_endpoint_stats(&camera_f32);
            let camera_f32_small = image::imageops::resize(
                &camera_f32,
                DIAGNOSTIC_EDGE,
                ((DIAGNOSTIC_EDGE as f64 * camera_f32.height() as f64 / camera_f32.width() as f64)
                    .round() as u32)
                    .max(1),
                image::imageops::FilterType::Triangle,
            );
            let camera_f32_auto =
                ab_render_variant(&output_root, frame, "g-camera-f32", &camera_f32_small);
            println!(
                "frame={frame} camera-f32 endpoints={camera_f32_stats:?} auto={camera_f32_auto:?}"
            );

            assert_eq!(current_stats.pixels, transport_stats.pixels);
            assert!(
                signed_below_zero.iter().any(|count| *count > 0),
                "{frame} must expose signed linear-sRGB values for this A/B"
            );
            assert!(
                compressed_stats.zero.iter().all(|count| *count == 0),
                "{frame} compressed transport must remain positive before density math"
            );
            assert!(
                density_safe_stats.zero.iter().all(|count| *count == 0),
                "{frame} density-safe transport must remain positive before density math"
            );
        }
    }

    #[test]
    fn supported_large_formats_use_deferred_import_preview_paths() {
        for path in [
            "a.dng", "a.nef", "a.nrw", "a.cr3", "a.arw", "a.raf", "a.rw2", "a.orf", "a.srw",
            "a.pef", "a.3fr", "a.fff", "a.iiq", "a.x3f",
        ] {
            assert!(
                is_raw_extension(path),
                "{path} must be recognized as a deferred import format"
            );
        }
        assert!(is_tiff_extension("scan.tif"));
        assert!(is_tiff_extension("scan.TIFF"));
    }

    #[test]
    fn noritsu_tiff_metadata_enables_linked_auto_limits() {
        let root = std::env::temp_dir().join(format!(
            "nexfilm-noritsu-tiff-test-{}-{}",
            std::process::id(),
            super::NEXT_ID.fetch_add(1, std::sync::atomic::Ordering::SeqCst)
        ));
        std::fs::create_dir(&root).unwrap();

        let make = b"NORITSU KOKI\0";
        let value_offset = 8 + 2 + 12 + 4;
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"II");
        bytes.extend_from_slice(&42u16.to_le_bytes());
        bytes.extend_from_slice(&8u32.to_le_bytes());
        bytes.extend_from_slice(&1u16.to_le_bytes());
        bytes.extend_from_slice(&271u16.to_le_bytes());
        bytes.extend_from_slice(&2u16.to_le_bytes());
        bytes.extend_from_slice(&(make.len() as u32).to_le_bytes());
        bytes.extend_from_slice(&(value_offset as u32).to_le_bytes());
        bytes.extend_from_slice(&0u32.to_le_bytes());
        bytes.extend_from_slice(make);

        let noritsu_path = root.join("noritsu.tiff");
        std::fs::write(&noritsu_path, &bytes).unwrap();
        assert!(is_noritsu_rendered_image(
            noritsu_path.to_string_lossy().as_ref()
        ));

        let generic_path = root.join("generic.tiff");
        let generic = bytes
            .windows(make.len())
            .position(|window| window == make)
            .map(|offset| {
                let mut generic = bytes.clone();
                generic[offset..offset + make.len()].copy_from_slice(b"GENERIC TIFF\0");
                generic
            })
            .unwrap();
        std::fs::write(&generic_path, generic).unwrap();
        assert!(!is_noritsu_rendered_image(
            generic_path.to_string_lossy().as_ref()
        ));

        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn embedded_preview_selection_prefers_the_closest_useful_resolution() {
        assert!(is_better_preview_edge(1024, 256, 1024));
        assert!(is_better_preview_edge(900, 640, 1024));
        assert!(is_better_preview_edge(1280, 2048, 1024));
        assert!(!is_better_preview_edge(2048, 1280, 1024));
        assert!(!is_better_preview_edge(640, 1024, 1024));
    }

    #[test]
    fn raw_rgb16_conversion_preserves_full_range_and_channel_stride() {
        let samples = [0u16, 32768, u16::MAX, 111, u16::MAX, 1, 16383, 222];
        let mut bytes = Vec::with_capacity(samples.len() * std::mem::size_of::<u16>());
        for sample in samples {
            bytes.extend_from_slice(&sample.to_ne_bytes());
        }

        let image = rgb16_image_from_bytes(2, 1, 4, 16, &bytes).unwrap();
        assert_eq!(image.get_pixel(0, 0).0, [0, 32768, u16::MAX]);
        assert_eq!(image.get_pixel(1, 0).0, [u16::MAX, 1, 16383]);
    }

    #[test]
    fn prophoto_transport_preserves_signed_estimate_values_within_cache_range() {
        let source = ImageBuffer::from_raw(2, 1, vec![-1.0, 0.0, 1.0, 2.0, 2.5, 3.0]).unwrap();
        let transport = prophoto_estimate_to_transport_proxy(&source);
        let span = PROPHOTO_TRANSPORT_MAX - PROPHOTO_TRANSPORT_MIN;
        for (encoded, original) in transport.as_raw().iter().zip(source.as_raw().iter()) {
            let decoded = *encoded as f32 / 65535.0 * span + PROPHOTO_TRANSPORT_MIN;
            assert!((decoded - original).abs() <= span / 65535.0 + 1e-6);
        }
    }

    #[test]
    fn roll_reference_extremes_use_opposite_density_tails() {
        let image = ImageBuffer::from_raw(
            100,
            1,
            (0..100)
                .flat_map(|index| {
                    let transmission = 0.01 + index as f32 * 0.0099;
                    [transmission; 3]
                })
                .collect(),
        )
        .unwrap();
        let base = reference_density_extreme(&image, DensityAnchorSource::SampledFilmBase).unwrap();
        let full =
            reference_density_extreme(&image, DensityAnchorSource::SampledFullExposure).unwrap();
        assert!(base.iter().all(|value| value < &full[0]));
        assert!(full.iter().all(|value| *value > 1.0));
    }

    #[test]
    fn import_contract_defaults_follow_loose_and_roll_anchor_rules() {
        let anchors = DensityAnchors {
            d_min_base: Some(DensityAnchor {
                density: [0.1; 3],
                source: DensityAnchorSource::SampledFilmBase,
                scope: DensityAnchorScope::Roll,
                confidence: DensityAnchorConfidence::UserSampled,
                reference_id: Some("base.tif".into()),
                provenance: Default::default(),
            }),
            d_max_full_exposure: Some(DensityAnchor {
                density: [2.0; 3],
                source: DensityAnchorSource::SampledFullExposure,
                scope: DensityAnchorScope::Roll,
                confidence: DensityAnchorConfidence::UserSampled,
                reference_id: Some("full.tif".into()),
                provenance: Default::default(),
            }),
            retained_records: Vec::new(),
        };
        let rolls = vec![Roll {
            roll_id: "roll-a".into(),
            date: String::new(),
            format: "35mm".into(),
            film_stock: String::new(),
            camera: String::new(),
            image_paths: Vec::new(),
            density_anchors: anchors,
            calibration_profile_id: None,
            scanner_profile_id: None,
        }];
        assert_eq!(
            default_pipeline_state_for_import(true, "roll-a", &rolls).contract,
            ProcessingContract::SmartAutoProPhotoV11
        );
        assert_eq!(
            default_pipeline_state_for_import(false, "roll-a", &rolls).contract,
            ProcessingContract::RollAnchoredProPhotoV11
        );
    }

    #[test]
    fn only_two_roll_samples_form_complete_density_anchors() {
        let sampled = |source| DensityAnchor {
            density: [0.2; 3],
            source,
            scope: DensityAnchorScope::Roll,
            confidence: DensityAnchorConfidence::UserSampled,
            reference_id: None,
            provenance: Default::default(),
        };
        let estimated_base = DensityAnchor {
            density: [0.1; 3],
            source: DensityAnchorSource::EstimatedFromContent,
            scope: DensityAnchorScope::Frame,
            confidence: DensityAnchorConfidence::Estimated,
            reference_id: None,
            provenance: Default::default(),
        };
        let full_only = DensityAnchors {
            d_min_base: None,
            d_max_full_exposure: Some(sampled(DensityAnchorSource::SampledFullExposure)),
            retained_records: Vec::new(),
        };
        assert!(!full_only.is_fully_anchored());
        assert_eq!(
            full_only.prophoto_contract(),
            ProcessingContract::SmartAutoProPhotoV11
        );

        let estimated_and_full = DensityAnchors {
            d_min_base: Some(estimated_base),
            d_max_full_exposure: full_only.d_max_full_exposure.clone(),
            retained_records: Vec::new(),
        };
        assert!(!estimated_and_full.is_fully_anchored());
        assert_eq!(
            estimated_and_full.prophoto_contract(),
            ProcessingContract::SmartAutoProPhotoV11
        );

        let complete = DensityAnchors {
            d_min_base: Some(sampled(DensityAnchorSource::SampledFilmBase)),
            d_max_full_exposure: full_only.d_max_full_exposure,
            retained_records: Vec::new(),
        };
        assert!(complete.is_fully_anchored());
        assert_eq!(
            complete.prophoto_contract(),
            ProcessingContract::RollAnchoredProPhotoV11
        );
    }

    #[test]
    fn partial_roll_anchor_fixes_only_its_own_density_endpoint() {
        let base = DensityAnchor {
            density: [0.2, 0.3, 0.4],
            source: DensityAnchorSource::SampledFilmBase,
            scope: DensityAnchorScope::Roll,
            confidence: DensityAnchorConfidence::UserSampled,
            reference_id: None,
            provenance: Default::default(),
        };
        let full = DensityAnchor {
            density: [2.2, 2.4, 2.6],
            source: DensityAnchorSource::SampledFullExposure,
            scope: DensityAnchorScope::Roll,
            confidence: DensityAnchorConfidence::UserSampled,
            reference_id: None,
            provenance: Default::default(),
        };
        let mut base_only_limits = AutoColorLimits {
            d_min: [0.12, 0.13, 0.14],
            d_max: [1.7, 1.8, 1.9],
            pipeline_state: None,
        };
        apply_roll_density_anchor_limits(
            &mut base_only_limits,
            &DensityAnchors {
                d_min_base: Some(base.clone()),
                d_max_full_exposure: None,
                retained_records: Vec::new(),
            },
            base.density,
        );
        assert_eq!(base_only_limits.d_min, [0.0; 3]);
        assert_eq!(base_only_limits.d_max, [1.7, 1.8, 1.9]);

        let mut full_only_limits = AutoColorLimits {
            d_min: [0.12, 0.13, 0.14],
            d_max: [1.7, 1.8, 1.9],
            pipeline_state: None,
        };
        let estimated_base = [0.1, 0.2, 0.3];
        apply_roll_density_anchor_limits(
            &mut full_only_limits,
            &DensityAnchors {
                d_min_base: None,
                d_max_full_exposure: Some(full),
                retained_records: Vec::new(),
            },
            estimated_base,
        );
        assert_eq!(full_only_limits.d_min, [0.12, 0.13, 0.14]);
        for (actual, expected) in full_only_limits.d_max.iter().zip([2.1, 2.2, 2.3]) {
            assert!((actual - expected).abs() < 1e-6);
        }
    }

    #[test]
    fn film_area_content_never_moves_a_roll_dmin_anchor() {
        let base = DensityAnchor {
            density: [0.2, 0.3, 0.4],
            source: DensityAnchorSource::SampledFilmBase,
            scope: DensityAnchorScope::Roll,
            confidence: DensityAnchorConfidence::UserSampled,
            reference_id: None,
            provenance: Default::default(),
        };
        let anchors = DensityAnchors {
            d_min_base: Some(base.clone()),
            d_max_full_exposure: None,
            retained_records: Vec::new(),
        };
        for film_area_limits in [[0.05, 0.10, 0.15], [0.65, 0.70, 0.75]] {
            let mut limits = AutoColorLimits {
                d_min: film_area_limits,
                d_max: [1.4, 1.5, 1.6],
                pipeline_state: None,
            };
            apply_roll_density_anchor_limits(&mut limits, &anchors, base.density);
            assert_eq!(limits.d_min, [0.0; 3]);
        }
    }

    #[test]
    fn preserve_tone_does_not_full_stretch_a_short_tone_photo() {
        let anchors = DensityAnchors {
            d_min_base: Some(DensityAnchor {
                density: [0.2; 3],
                source: DensityAnchorSource::SampledFilmBase,
                scope: DensityAnchorScope::Roll,
                confidence: DensityAnchorConfidence::UserSampled,
                reference_id: None,
                provenance: Default::default(),
            }),
            d_max_full_exposure: None,
            retained_records: Vec::new(),
        };
        let mut limits = AutoColorLimits {
            d_min: [0.0; 3],
            d_max: [0.35, 0.45, 0.55],
            pipeline_state: None,
        };
        preserve_tone_density_span(&mut limits, &anchors);
        assert_eq!(limits.d_min, [0.0; 3]);
        for maximum in limits.d_max {
            assert!((maximum - super::PRESERVE_TONE_MIN_DENSITY_SPAN).abs() < 1.0e-6);
        }
    }

    #[test]
    fn smart_auto_base_ignores_outside_white_panel_and_invalid_values() {
        let mut image =
            ImageBuffer::<Rgb<f32>, Vec<f32>>::from_pixel(8, 8, Rgb([0.99, 0.99, 0.99]));
        for y in 2..6 {
            for x in 2..6 {
                image.put_pixel(x, y, Rgb([0.72, 0.68, 0.64]));
            }
        }
        image.put_pixel(3, 3, Rgb([-1.0, f32::NAN, 2.0]));
        let mut geom = GeometryState::default();
        geom.calibration_points = Some([[0.2, 0.2], [0.8, 0.2], [0.8, 0.8], [0.2, 0.8]]);
        let (base, confidence) = compute_auto_base_f32(&image, &geom).unwrap();
        assert!(confidence > 0.1);
        assert!(base.iter().all(|value| value.is_finite() && *value > 0.0));
        assert!(base[0] > 0.05 && base[0] < 0.2);
    }

    #[test]
    fn smart_auto_without_film_area_uses_content_fallback() {
        let image = ImageBuffer::from_pixel(8, 8, Rgb([0.8, 0.75, 0.7]));
        let (base, confidence) = compute_auto_base_f32(&image, &GeometryState::default()).unwrap();
        assert_eq!(base, [0.0; 3]);
        assert_eq!(confidence, 0.0);
    }

    #[test]
    fn selected_film_area_limits_ignore_outside_light_panel_changes() {
        let mut first = ImageBuffer::<Rgb<f32>, Vec<f32>>::from_pixel(16, 16, Rgb([0.98; 3]));
        let mut second = first.clone();
        for y in 4..12 {
            for x in 4..12 {
                let value = 0.35 + ((x + y) % 8) as f32 * 0.04;
                let pixel = Rgb([value, value * 0.95, value * 1.05]);
                first.put_pixel(x, y, pixel);
                second.put_pixel(x, y, pixel);
            }
        }
        for y in 0..4 {
            for x in 0..16 {
                second.put_pixel(x, y, Rgb([0.70; 3]));
            }
        }
        let mut geom = GeometryState::default();
        geom.calibration_points = Some([[0.25, 0.25], [0.75, 0.25], [0.75, 0.75], [0.25, 0.75]]);
        let a = compute_content_limits_f32(&first, None, &geom, [0.0; 3]).unwrap();
        let b = compute_content_limits_f32(&second, None, &geom, [0.0; 3]).unwrap();
        for channel in 0..3 {
            assert!((a.d_min[channel] - b.d_min[channel]).abs() < 1.0e-5);
            assert!((a.d_max[channel] - b.d_max[channel]).abs() < 1.0e-5);
        }
    }

    #[test]
    fn uncalibrated_smart_auto_renders_finite_visible_positive_fixture() {
        let image = ImageBuffer::from_fn(32, 24, |x, y| {
            let value = 0.25 + ((x + y) % 12) as f32 * 0.035;
            Rgb([value, value * 0.92, value * 1.08])
        });
        let mut geom = GeometryState::default();
        geom.calibration_points = Some([[0.1, 0.1], [0.9, 0.1], [0.9, 0.9], [0.1, 0.9]]);
        let mut state = PipelineState::smart_auto();
        let mut limits = compute_content_limits_f32(&image, None, &geom, [0.0; 3]).unwrap();
        share_smart_auto_density_scale(&mut limits);
        state.processing_report.base_source = "content_estimate".to_string();
        state.render_mapping.density_low = limits.d_min;
        state.render_mapping.density_high = limits.d_max;
        let rendered = render_f32_shader_equivalent(
            &image,
            None,
            &TuningParams::default(),
            &geom,
            &BaseColor::default(),
            &state,
            None,
        );
        assert!(rendered.as_raw().iter().all(|value| *value <= u16::MAX));
        assert!(rendered.as_raw().iter().any(|value| *value > 0));
        assert!(rendered.as_raw().iter().any(|value| *value < u16::MAX));
    }

    #[test]
    fn unprofiled_generic_tiff_is_rejected_from_smart_auto_domain() {
        let error = decode_tiff_for_smart_auto("missing-unprofiled.tiff", 256).unwrap_err();
        assert_eq!(error, "scanner_tiff_input_space_unknown");
    }

    #[test]
    fn smart_auto_short_tone_is_not_forced_to_fixed_density_span() {
        let anchors = DensityAnchors::default();
        let mut limits = AutoColorLimits {
            d_min: [0.1; 3],
            d_max: [0.4; 3],
            pipeline_state: None,
        };
        preserve_tone_density_span(&mut limits, &anchors);
        assert_eq!(limits.d_min, [0.1; 3]);
        assert_eq!(limits.d_max, [0.4; 3]);
    }

    #[test]
    fn smart_auto_uses_one_shared_density_scale_for_colour_channels() {
        let mut limits = AutoColorLimits {
            d_min: [0.10, 0.20, 0.30],
            d_max: [0.80, 1.00, 1.20],
            pipeline_state: None,
        };
        share_smart_auto_density_scale(&mut limits);
        assert_eq!(limits.d_min[0], limits.d_min[1]);
        assert_eq!(limits.d_min[1], limits.d_min[2]);
        assert_eq!(limits.d_max[0], limits.d_max[1]);
        assert_eq!(limits.d_max[1], limits.d_max[2]);
    }

    #[test]
    fn smart_auto_short_content_gets_an_adaptive_mid_tone_window() {
        let mut limits = AutoColorLimits {
            d_min: [0.30; 3],
            d_max: [0.50; 3],
            pipeline_state: None,
        };
        preserve_smart_auto_content_span(&mut limits);
        assert!((limits.d_min[0] - 0.0).abs() < 1.0e-6);
        assert!((limits.d_max[0] - 0.8).abs() < 1.0e-6);
        assert_eq!(limits.d_min, [limits.d_min[0]; 3]);
        assert_eq!(limits.d_max, [limits.d_max[0]; 3]);
    }

    #[test]
    fn estimated_smart_auto_base_is_analysis_state_not_physical_anchor() {
        let mut state = PipelineState::smart_auto();
        assert!(!pipeline_has_base(&state, &BaseColor::default()));
        state.processing_report.base_source = "content_estimate".to_string();
        assert!(pipeline_has_base(&state, &BaseColor::default()));
        assert_eq!(
            pipeline_base_density(&state, &BaseColor::default()),
            [0.0; 3]
        );
        assert!(state.density_anchors.d_min_base.is_none());
    }

    #[test]
    fn nikon_nef_decode_errors_include_high_efficiency_guidance() {
        assert!(raw_decode_failure_hint("frame.NEF")
            .unwrap()
            .contains("Nikon Z 8/Z 9 HE/HE* NEF"));
        assert!(raw_decode_failure_hint("frame.raf").is_none());

        let message = libraw_decode_error_message("frame.NEF", "unsupported format");
        assert!(message.contains("LibRaw"));
        assert!(message.contains("unsupported format"));
        assert!(message.contains("standard/lossless NEF"));
    }

    #[test]
    #[ignore]
    fn linked_libraw_supports_current_gfx_raf_generation() {
        let version = crate::raw_backend::RawProcessor::version();
        let mut numbers = version
            .split(['.', '-'])
            .filter_map(|part| part.parse::<u32>().ok());
        let major = numbers.next().unwrap_or_default();
        let minor = numbers.next().unwrap_or_default();
        assert!(
            (major, minor) >= (0, 22),
            "LibRaw {version} is too old for current Fujifilm GFX RAF files"
        );
    }

    #[test]
    #[ignore = "large user-supplied FFF fixtures; run explicitly when validating scanner FFF"]
    fn hasselblad_fff_fixtures_decode_visible_previews_and_linear_proxies() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("test_picture");
        let paths = [
            root.join("图像 001.fff"),
            root.join("哈苏fff").join("1 001-可以反相.fff"),
            root.join("哈苏fff").join("无法反相.fff"),
        ]
        .into_iter()
        .filter(|path| path.exists())
        .collect::<Vec<_>>();
        if paths.is_empty() {
            return;
        }

        for path in paths {
            let decode_started = std::time::Instant::now();
            let decoded =
                decode_reduced_tiff_for_working_space(path.to_string_lossy().as_ref(), 2560)
                    .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
            assert!(
                decoded.width() > 256 && decoded.height() > 256,
                "{}",
                path.display()
            );
            assert!(
                decoded.as_raw().iter().any(|value| *value > 0),
                "{} produced a black proxy",
                path.display()
            );
            let reduced =
                image::imageops::resize(&decoded, 160, 512, image::imageops::FilterType::Triangle);
            let mut geom = GeometryState::default();
            geom.crop_rect.width = 0.5;
            let transformed = render_shader_equivalent(
                &reduced,
                &TuningParams::default(),
                &geom,
                &BaseColor::default(),
                None,
            );
            assert_eq!(transformed.dimensions(), (80, 512));

            let preview_started = std::time::Instant::now();
            let preview = decode_import_preview_base64(
                path.to_string_lossy().as_ref(),
                IMPORT_PREVIEW_LONG_EDGE,
            )
            .unwrap_or_else(|| panic!("{} could not be decoded", path.display()));
            let preview_bytes = base64::engine::general_purpose::STANDARD
                .decode(preview)
                .expect("FFF preview must be base64");
            let preview_image = image::load_from_memory(&preview_bytes)
                .expect("FFF preview must contain a displayable image")
                .to_rgb8();
            assert!(preview_image.width() > 1 && preview_image.height() > 1);
            assert!(
                preview_image.as_raw().iter().any(|value| *value > 6),
                "{} produced a black import preview",
                path.display()
            );

            println!(
                "{}: FFF preview {:?} ({}x{}), proxy {:?} ({:?})",
                path.display(),
                preview_started.elapsed(),
                preview_image.width(),
                preview_image.height(),
                decode_started.elapsed(),
                decoded.dimensions()
            );
        }
    }

    #[test]
    #[ignore = "large user-supplied scanner TIFF fixtures; run explicitly for memory validation"]
    fn nikon_scanner_tiff_import_and_proxy_use_bounded_memory() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("test_picture")
            .join("尼康扫描仪tiff");
        let mut paths = std::fs::read_dir(root)
            .ok()
            .into_iter()
            .flatten()
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| {
                path.extension().is_some_and(|extension| {
                    extension.eq_ignore_ascii_case("tif") || extension.eq_ignore_ascii_case("tiff")
                })
            })
            .collect::<Vec<_>>();
        paths.sort();
        if paths.is_empty() {
            return;
        }
        for path in paths {
            let started = std::time::Instant::now();
            let preview = decode_import_preview_base64(
                path.to_string_lossy().as_ref(),
                IMPORT_PREVIEW_LONG_EDGE,
            )
            .unwrap_or_else(|| panic!("{} could not be decoded", path.display()));
            let preview_bytes = base64::engine::general_purpose::STANDARD
                .decode(preview)
                .expect("TIFF preview must be base64");
            let preview_image = image::load_from_memory(&preview_bytes)
                .expect("TIFF preview must be displayable")
                .to_rgb8();
            assert!(preview_image.width().max(preview_image.height()) <= IMPORT_PREVIEW_LONG_EDGE);
            assert!(
                preview_image.as_raw().iter().any(|value| *value > 6),
                "{} produced a black import preview",
                path.display()
            );

            let proxy =
                decode_reduced_tiff_for_working_space(path.to_string_lossy().as_ref(), 4096)
                    .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
            assert_eq!(proxy.width().max(proxy.height()), 4096);
            assert!(proxy.as_raw().iter().any(|value| *value > 0));
            println!(
                "{}: Nikon TIFF preview {:?}, proxy {:?} in {:?}",
                path.display(),
                (preview_image.width(), preview_image.height()),
                proxy.dimensions(),
                started.elapsed()
            );
        }
    }

    #[test]
    #[ignore = "1-1.6 GB user-supplied Epson LinearRaw DNG; run explicitly for memory validation"]
    fn epson_6400_dng_preview_is_visible_and_proxy_is_bounded() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("test_picture")
            .join("爱普森dng")
            .join("6400DPI");
        let mut paths = std::fs::read_dir(root)
            .ok()
            .into_iter()
            .flatten()
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| {
                path.extension()
                    .is_some_and(|extension| extension.eq_ignore_ascii_case("dng"))
            })
            .collect::<Vec<_>>();
        paths.sort();
        if paths.is_empty() {
            return;
        }

        for path in paths {
            let started = std::time::Instant::now();
            let preview = decode_import_preview_base64(
                path.to_string_lossy().as_ref(),
                IMPORT_PREVIEW_LONG_EDGE,
            )
            .unwrap_or_else(|| panic!("{} could not be decoded", path.display()));
            let bytes = base64::engine::general_purpose::STANDARD
                .decode(preview)
                .expect("DNG preview must be base64");
            let preview = image::load_from_memory(&bytes)
                .expect("DNG preview must be displayable")
                .to_rgb8();
            let maximum = preview.as_raw().iter().copied().max().unwrap_or(0);
            let visible = preview.as_raw().iter().filter(|value| **value > 6).count();
            assert!(maximum > 12, "{} preview is black", path.display());
            assert!(
                visible > preview.as_raw().len() / 100,
                "{} preview does not contain enough visible pixels",
                path.display()
            );

            let proxy = decode_reduced_dng_for_working_space(path.to_string_lossy().as_ref(), 4096)
                .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
            assert_eq!(proxy.width().max(proxy.height()), 4096);
            assert!(
                proxy.as_raw().iter().any(|value| *value > 16),
                "{} produced a black Develop proxy",
                path.display()
            );
            println!(
                "{}: Epson DNG preview {:?}, proxy {:?} in {:?}",
                path.display(),
                preview.dimensions(),
                proxy.dimensions(),
                started.elapsed()
            );
        }
    }

    #[test]
    fn supplied_jpeg_fixture_decodes_and_accepts_geometry_edits() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("test_picture")
            .join("000021940005.jpg");
        if !path.exists() {
            return;
        }
        assert!(is_noritsu_rendered_image(path.to_string_lossy().as_ref()));
        let decoded =
            decode_image_buffer(path.to_string_lossy().as_ref(), DecodeMode::DevelopProxy)
                .expect("JPEG fixture should decode to a linear proxy");
        let base = compute_auto_base(&decoded);
        let limits = compute_auto_color_limits(
            &decoded,
            &GeometryState::default(),
            &base,
            crate::app_state::FilmMode::Color,
            true,
        )
        .expect("Noritsu fixture should produce density limits");
        assert_ne!(limits.d_min[0], limits.d_min[2]);
        assert_ne!(limits.d_max[0], limits.d_max[2]);
        println!(
            "Noritsu base {:?}, D-Min {:?}, D-Max {:?}",
            [base.base_r, base.base_g, base.base_b],
            limits.d_min,
            limits.d_max
        );
        if std::env::var_os("NEXFILM_WRITE_NORITSU_DEBUG").is_some() {
            let mut params = TuningParams::default();
            params.density.d_min = limits.d_min;
            params.density.d_max = limits.d_max;
            let preview = image::imageops::resize(
                &decoded,
                800,
                (800.0 * decoded.height() as f32 / decoded.width() as f32) as u32,
                image::imageops::FilterType::Triangle,
            );
            let rendered =
                render_shader_equivalent(&preview, &params, &GeometryState::default(), &base, None);
            rendered
                .save(
                    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                        .join("target/noritsu-auto-invert-debug.png"),
                )
                .unwrap();
        }
        let mut geom = GeometryState::default();
        geom.crop_rect.x = 0.0;
        geom.crop_rect.y = 0.0;
        geom.crop_rect.width = 0.5;
        geom.crop_rect.height = 1.0;
        let transformed = render_shader_equivalent(
            &decoded,
            &TuningParams::default(),
            &geom,
            &BaseColor::default(),
            None,
        );
        assert_eq!(transformed.width(), decoded.width() / 2);
        assert_eq!(transformed.height(), decoded.height());
    }

    #[test]
    fn nikon_nef_uses_the_embedded_jpeg_without_libraw_thumbnail_unpack() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("test_picture")
            .join("_DSC7333.NEF");
        if !path.exists() {
            return;
        }
        let started = std::time::Instant::now();
        let preview =
            decode_import_preview_base64(path.to_string_lossy().as_ref(), IMPORT_PREVIEW_LONG_EDGE)
                .expect("NEF should expose an embedded JPEG preview");
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(preview)
            .expect("preview must be base64");
        let image = image::load_from_memory(&bytes).expect("preview must be a JPEG");
        assert!(
            image.width() <= IMPORT_PREVIEW_LONG_EDGE && image.height() <= IMPORT_PREVIEW_LONG_EDGE
        );
        assert!(image.width().max(image.height()) >= IMPORT_PREVIEW_LONG_EDGE / 2);
        assert!(image.width() > 1 && image.height() > 1);
        println!(
            "NEF embedded preview extraction: {:?}, {}x{}, {} KiB",
            started.elapsed(),
            image.width(),
            image.height(),
            bytes.len() / 1024
        );
    }

    #[test]
    fn nikon_nef_develop_decode_is_full_resolution() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("test_picture")
            .join("_DSC7333.NEF");
        if !path.exists() {
            return;
        }
        let started = std::time::Instant::now();
        let decoded =
            decode_image_buffer(path.to_string_lossy().as_ref(), DecodeMode::DevelopProxy)
                .expect("NEF develop proxy should decode");
        println!(
            "NEF develop proxy {:?}, {:?}",
            started.elapsed(),
            decoded.dimensions()
        );
        assert!(decoded.width() > 256);
    }

    #[test]
    #[ignore]
    fn nikon_nef_batch_import_previews_are_lightweight() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("test_picture");
        let paths: Vec<_> = std::fs::read_dir(&root)
            .expect("test_picture should be readable")
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| {
                path.extension()
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("nef"))
            })
            .collect();
        if paths.is_empty() {
            return;
        }
        let started = std::time::Instant::now();
        for path in &paths {
            let preview = decode_import_preview_base64(
                path.to_string_lossy().as_ref(),
                IMPORT_PREVIEW_LONG_EDGE,
            )
            .expect("each NEF should expose an embedded preview");
            let bytes = base64::engine::general_purpose::STANDARD
                .decode(preview)
                .expect("preview must be base64");
            let image = image::load_from_memory(&bytes).expect("preview must be a JPEG");
            assert!(image.width().max(image.height()) <= IMPORT_PREVIEW_LONG_EDGE);
        }
        println!(
            "{} NEF import previews extracted in {:?}",
            paths.len(),
            started.elapsed()
        );
    }

    #[test]
    fn loose_images_use_the_same_sqlite_contract_as_roll_images() {
        let mut connection = rusqlite::Connection::open_in_memory().unwrap();
        crate::persistence::init_schema(&connection).unwrap();
        let item = FilmItem {
            id: "loose-id".into(),
            roll_id: "LOOSE_DEFAULT".into(),
            file_path: "loose.dng".into(),
            embedded_thumbnail_base64: "orange".into(),
            rendered_thumbnail_base64: None,
            original_proxy: None,
            proxy_image: None,
            prophoto_estimate_proxy: None,
            relative_transmission_proxy: None,
            relative_transmission_quality: None,
            pristine_proxy: None,
            base_color: BaseColor::default(),
            runtime_pipeline_state: None,
            runtime_density_provenance: None,
            runtime_pipeline_key: None,
            pipeline_state: PipelineState::smart_auto(),
            params: TuningParams::default(),
            geom: GeometryState::default(),
            is_loose: true,
            in_library: true,
        };

        persist_import_batch(&mut connection, &[item]).unwrap();
        assert!(crate::persistence::row_exists(&connection, "LOOSE_DEFAULT", "loose.dng").unwrap());
    }
}

#[cfg(test)]
mod library_management_contract_tests {
    use super::{
        activate_library_roll, clear_library_membership, load_all_image_states_from_connection,
        migrate_legacy_loose_roll, normalize_path, persist_import_batch, process_source_paths,
        DeleteRollsResult,
    };
    use crate::app_state::{
        BaseColor, EngineState, FilmItem, GeometryState, PipelineState, Roll, TuningParams,
    };
    use std::sync::{Arc, RwLock};

    fn item(id: &str, roll_id: &str, path: &str, in_library: bool) -> FilmItem {
        FilmItem {
            id: id.to_string(),
            roll_id: roll_id.to_string(),
            file_path: path.to_string(),
            embedded_thumbnail_base64: "preview".to_string(),
            rendered_thumbnail_base64: None,
            original_proxy: None,
            proxy_image: None,
            prophoto_estimate_proxy: None,
            relative_transmission_proxy: None,
            relative_transmission_quality: None,
            pristine_proxy: None,
            base_color: BaseColor::default(),
            runtime_pipeline_state: None,
            runtime_density_provenance: None,
            runtime_pipeline_key: None,
            pipeline_state: PipelineState::default(),
            params: TuningParams::default(),
            geom: GeometryState::default(),
            is_loose: false,
            in_library,
        }
    }

    #[test]
    fn clearing_a_working_library_keeps_records_but_removes_membership_and_active_image() {
        let state = EngineState::new();
        state.items.insert(
            "old".to_string(),
            Arc::new(RwLock::new(item("old", "roll-old", "old.dng", true))),
        );
        *state.active_id.write().unwrap() = Some("old".to_string());

        clear_library_membership(&state).unwrap();

        assert!(!state.items.get("old").unwrap().read().unwrap().in_library);
        assert!(state.active_id.read().unwrap().is_none());
        assert!(state.items.contains_key("old"));
    }

    #[test]
    fn restored_records_do_not_reopen_in_the_working_library() {
        let mut connection = rusqlite::Connection::open_in_memory().unwrap();
        crate::persistence::init_schema(&connection).unwrap();
        persist_import_batch(
            &mut connection,
            &[item("persisted", "LOOSE_DEFAULT", "old-scan.dng", true)],
        )
        .unwrap();
        let restored = EngineState::new();

        load_all_image_states_from_connection(&restored, &connection).unwrap();

        assert_eq!(restored.items.len(), 1);
        let restored_item = restored.items.iter().next().unwrap();
        assert!(!restored_item.value().read().unwrap().in_library);
    }

    #[test]
    fn legacy_loose_records_become_a_manageable_archive_roll() {
        let mut connection = rusqlite::Connection::open_in_memory().unwrap();
        crate::persistence::init_schema(&connection).unwrap();
        persist_import_batch(
            &mut connection,
            &[
                item("a", "LOOSE_DEFAULT", "a.dng", true),
                item("b", "LOOSE_DEFAULT", "b.dng", true),
            ],
        )
        .unwrap();
        let mut rolls = Vec::new();

        assert!(migrate_legacy_loose_roll(&mut connection, &mut rolls).unwrap());
        assert_eq!(rolls.len(), 1);
        assert_eq!(rolls[0].format, "Loose");
        assert_eq!(rolls[0].image_paths, vec!["a.dng", "b.dng"]);
        assert!(!migrate_legacy_loose_roll(&mut connection, &mut rolls).unwrap());
        assert_eq!(crate::persistence::load_rolls(&connection).unwrap(), rolls);
    }

    #[test]
    fn activating_a_roll_replaces_the_previous_working_library() {
        let state = EngineState::new();
        for item in [
            item("old", "roll-old", "old.dng", true),
            item("target", "roll-new", "new.dng", false),
            item("not-in-roll", "roll-new", "extra.dng", true),
        ] {
            state
                .items
                .insert(item.id.clone(), Arc::new(RwLock::new(item)));
        }
        let roll = Roll {
            roll_id: "roll-new".to_string(),
            date: "2026-07-31".to_string(),
            format: "135".to_string(),
            film_stock: "Test Film".to_string(),
            camera: "Test Camera".to_string(),
            image_paths: vec!["NEW.DNG".to_string()],
            density_anchors: Default::default(),
            calibration_profile_id: None,
            scanner_profile_id: None,
        };

        let activated = activate_library_roll(&state, &roll).unwrap();

        assert_eq!(activated, vec!["target".to_string()]);
        assert!(!state.items.get("old").unwrap().read().unwrap().in_library);
        assert!(
            state
                .items
                .get("target")
                .unwrap()
                .read()
                .unwrap()
                .in_library
        );
        assert!(
            !state
                .items
                .get("not-in-roll")
                .unwrap()
                .read()
                .unwrap()
                .in_library
        );
    }

    #[test]
    fn activating_a_loose_batch_preserves_its_loose_identity() {
        let state = EngineState::new();
        state.items.insert(
            "loose".to_string(),
            Arc::new(RwLock::new(item("loose", "loose-1", "scan.tif", false))),
        );
        let roll = Roll {
            roll_id: "loose-1".to_string(),
            date: String::new(),
            format: "Loose".to_string(),
            film_stock: "Loose Import".to_string(),
            camera: String::new(),
            image_paths: vec!["scan.tif".to_string()],
            density_anchors: Default::default(),
            calibration_profile_id: None,
            scanner_profile_id: None,
        };

        activate_library_roll(&state, &roll).unwrap();

        let entry = state.items.get("loose").unwrap();
        let item = entry.read().unwrap();
        assert!(item.in_library);
        assert!(item.is_loose);
    }

    #[test]
    fn source_recycling_selects_only_unshared_existing_files() {
        let root = std::env::temp_dir().join(format!(
            "nexfilm-delete-test-{}-{}",
            std::process::id(),
            super::NEXT_ID.fetch_add(1, std::sync::atomic::Ordering::SeqCst)
        ));
        std::fs::create_dir(&root).unwrap();
        let removable = root.join("remove.dng");
        let protected = root.join("shared.dng");
        let missing = root.join("missing.dng");
        std::fs::write(&removable, b"remove").unwrap();
        std::fs::write(&protected, b"keep").unwrap();
        let mut paths = std::collections::HashMap::new();
        for path in [&removable, &protected, &missing] {
            let path = path.to_string_lossy().to_string();
            paths.insert(normalize_path(&path), path);
        }
        let protected_paths =
            std::collections::HashSet::from([normalize_path(protected.to_string_lossy().as_ref())]);
        let mut result = DeleteRollsResult::default();

        process_source_paths(paths, &protected_paths, &mut result, |path| {
            std::fs::remove_file(path).map_err(|error| error.to_string())
        });

        assert_eq!(result.deleted_source_files, 1);
        assert_eq!(result.protected_source_files, 1);
        assert_eq!(result.missing_source_files, 1);
        assert!(result.failed_source_files.is_empty());
        assert!(!removable.exists());
        assert!(protected.exists());
        std::fs::remove_file(protected).unwrap();
        std::fs::remove_dir(root).unwrap();
    }
}

#[cfg(test)]
mod export_contract_tests {
    use super::{
        build_response_buffer_from_proxy, build_response_buffer_from_proxy_with_state,
        co_sited_density_extremes, compute_auto_color_limits, density_histogram_extremes,
        embedded_input_profile, encode_export_buffer, export_dimensions, export_profile_for_output,
        gaussian_blur_rgb16_parallel, normalize_persisted_geometry_for_rendered_image,
        render_shader_equivalent, reserve_export_path, sanitize_export_file_stem,
        should_apply_sprocket_mask, validate_export_color_space, write_export_image,
        write_export_image_with_profile, ExportConflictPolicy, ExportFormat,
    };
    use crate::app_state::{
        BaseColor, DensityAnchors, FilmMode, GeometryState, PipelineState, TuningParams,
    };
    use crate::color_science::ColorSpaceId;
    use image::{ColorType, GenericImageView, ImageBuffer, Rgb};
    use std::collections::HashSet;

    fn neutral_params() -> TuningParams {
        let mut params = TuningParams::default();
        params.film_mode = FilmMode::BW;
        params.density.d_min = [0.0; 3];
        params.density.d_max = [2.0; 3];
        params.density.gamma = 1.0;
        params
    }

    fn white_base() -> BaseColor {
        BaseColor {
            base_r: u16::MAX,
            base_g: u16::MAX,
            base_b: u16::MAX,
        }
    }

    #[test]
    fn rendered_legacy_state_is_treated_as_confirmed_calibration() {
        let normalized =
            normalize_persisted_geometry_for_rendered_image(GeometryState::default(), true);
        assert!(normalized.calibration_confirmed);
        assert_eq!(
            normalized.calibration_points,
            Some([[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]])
        );

        let untouched =
            normalize_persisted_geometry_for_rendered_image(GeometryState::default(), false);
        assert!(!untouched.calibration_confirmed);
        assert!(untouched.calibration_points.is_none());
    }

    #[test]
    fn full_frame_sprocket_mask_is_restricted_to_sampled_edge_band() {
        let full_bounds = [0.0, 0.0, 1.0, 1.0];
        assert!(should_apply_sprocket_mask(
            [0.5, 0.04],
            full_bounds,
            [0.5, 0.05],
        ));
        assert!(!should_apply_sprocket_mask(
            [0.5, 0.5],
            full_bounds,
            [0.5, 0.05],
        ));
        assert!(should_apply_sprocket_mask(
            [0.1, 0.5],
            [0.2, 0.2, 0.8, 0.8],
            [0.1, 0.5],
        ));
    }

    #[test]
    fn parallel_gaussian_blur_matches_reference_filter() {
        let mut source = ImageBuffer::new(11, 7);
        for (index, pixel) in source.pixels_mut().enumerate() {
            let value = (index as u16).wrapping_mul(997);
            *pixel = Rgb([value, value.wrapping_mul(3), value.wrapping_mul(7)]);
        }
        for sigma in [0.8, 1.0, 1.7] {
            let optimized = gaussian_blur_rgb16_parallel(&source, sigma);
            let reference = imageproc::filter::gaussian_blur_f32(&source, sigma);
            assert_eq!(optimized.as_raw(), reference.as_raw(), "sigma={sigma}");
        }
    }

    #[test]
    fn shader_equivalent_export_applies_crop_without_using_quality_as_scale() {
        let source = ImageBuffer::from_pixel(4, 4, Rgb([6554, 6554, 6554]));
        let mut geom = GeometryState::default();
        geom.crop_rect.x = 0.25;
        geom.crop_rect.y = 0.25;
        geom.crop_rect.width = 0.5;
        geom.crop_rect.height = 0.5;

        let output =
            render_shader_equivalent(&source, &neutral_params(), &geom, &white_base(), None);
        assert_eq!(output.dimensions(), (2, 2));
        assert!((32000..=33500).contains(&output.get_pixel(0, 0)[0]));
    }

    #[test]
    fn shader_equivalent_export_applies_sprocket_mask_only_outside_calibration_bounds() {
        let source = ImageBuffer::from_pixel(4, 4, Rgb([6554, 6554, 6554]));
        let mut params = neutral_params();
        params.sprocket.sprocket_uv = Some(vec![0.5, 0.5]);
        params.sprocket.sprocket_tolerance = Some(0.1);
        params.sprocket.sprocket_feather = Some(0.05);
        let mut geom = GeometryState::default();
        geom.calibration_points = Some([[0.25, 0.25], [0.75, 0.25], [0.75, 0.75], [0.25, 0.75]]);

        let output = render_shader_equivalent(&source, &params, &geom, &white_base(), None);
        assert_eq!(output.get_pixel(0, 0)[0], u16::MAX);
        assert!(output.get_pixel(1, 1)[0] < 40000);
    }

    #[test]
    fn black_and_white_export_ignores_color_only_controls() {
        let source = ImageBuffer::from_pixel(2, 2, Rgb([4000, 12000, 30000]));
        let mut params = neutral_params();
        params.tone.saturation = 1.0;
        params.tone.temperature = 1.0;
        params.tone.tint = 1.0;

        let output = render_shader_equivalent(
            &source,
            &params,
            &GeometryState::default(),
            &white_base(),
            None,
        );
        let pixel = output.get_pixel(0, 0);
        assert_eq!(pixel[0], pixel[1]);
        assert_eq!(pixel[1], pixel[2]);
    }

    #[test]
    fn auto_color_histogram_retains_true_sixteen_bit_limits() {
        let mut histogram = vec![0u32; 65536];
        for count in histogram.iter_mut().take(20_001).skip(10_000) {
            *count = 1;
        }
        let (low, high) = density_histogram_extremes(&histogram, 10_001);
        assert_eq!(low, 10_100);
        assert_eq!(high, 19_900);
        assert_ne!(low % 257, 0, "limits must not be quantized to 8-bit steps");
    }

    #[test]
    fn monochrome_auto_color_returns_one_weighted_density_range() {
        let mut proxy = ImageBuffer::new(64, 64);
        for (index, pixel) in proxy.pixels_mut().enumerate() {
            let green = 2_000 + ((index % 64) as u16) * 900;
            *pixel = Rgb([40_000, green, 55_000]);
        }
        let limits = compute_auto_color_limits(
            &proxy,
            &GeometryState::default(),
            &white_base(),
            FilmMode::BW,
            false,
        )
        .unwrap();
        assert_eq!(limits.d_min[0], limits.d_min[1]);
        assert_eq!(limits.d_min[1], limits.d_min[2]);
        assert_eq!(limits.d_max[0], limits.d_max[1]);
        assert_eq!(limits.d_max[1], limits.d_max[2]);
        assert!(limits.d_min[0] < limits.d_max[0]);
    }

    #[test]
    fn linked_scanner_limits_preserve_co_sited_channel_bounds() {
        let mut proxy = ImageBuffer::new(64, 64);
        for (index, pixel) in proxy.pixels_mut().enumerate() {
            let x = (index % 64) as u16;
            *pixel = Rgb([5_000 + x * 500, 12_000 + x * 650, 25_000 + x * 400]);
        }
        let limits = compute_auto_color_limits(
            &proxy,
            &GeometryState::default(),
            &white_base(),
            FilmMode::Color,
            true,
        )
        .unwrap();
        assert_ne!(limits.d_min[0], limits.d_min[1]);
        assert_ne!(limits.d_max[1], limits.d_max[2]);
        for channel in 0..3 {
            assert!(limits.d_min[channel] < limits.d_max[channel]);
        }
    }

    #[test]
    fn co_sited_limits_keep_scanner_channel_slopes() {
        let samples = (0..100)
            .map(|index| {
                let value = index as f32 / 100.0;
                [0.1 + value, 0.2 + value * 2.0, 0.3 + value * 3.0]
            })
            .collect();
        let (low, high) = co_sited_density_extremes(samples).unwrap();
        let ranges = [high[0] - low[0], high[1] - low[1], high[2] - low[2]];
        assert!((ranges[1] / ranges[0] - 2.0).abs() < 1e-5);
        assert!((ranges[2] / ranges[0] - 3.0).abs() < 1e-5);
    }

    #[test]
    fn export_accepts_profiled_professional_gamuts() {
        assert_eq!(validate_export_color_space("srgb").unwrap(), "srgb");
        assert_eq!(validate_export_color_space("rec2020").unwrap(), "rec2020");
        assert_eq!(
            validate_export_color_space("prophoto").unwrap(),
            "prophoto-rgb"
        );
        assert_eq!(
            validate_export_color_space("display-p3").unwrap(),
            "display-p3"
        );
        assert_eq!(validate_export_color_space("aces").unwrap(), "aces");
        assert!(validate_export_color_space("not-a-colour-space").is_err());
    }

    #[test]
    fn professional_export_profiles_are_embedded_in_every_format() {
        let root = std::env::temp_dir().join(format!(
            "nexfilm-export-profile-test-{}-{}",
            std::process::id(),
            super::NEXT_ID.fetch_add(1, std::sync::atomic::Ordering::SeqCst)
        ));
        std::fs::create_dir(&root).unwrap();
        let source = ImageBuffer::from_pixel(2, 2, Rgb([32768, 16384, 49152]));
        let profile =
            crate::color_science::build_icc_profile(crate::color_science::ColorSpaceId::DisplayP3);
        for (format, name, marker) in [
            (ExportFormat::Jpeg, "frame.jpg", b"ICC_PROFILE\0".as_slice()),
            (ExportFormat::Png, "frame.png", b"iCCP".as_slice()),
            (ExportFormat::Tiff16, "frame.tiff", &[0x73, 0x87]),
        ] {
            let path = root.join(name);
            write_export_image_with_profile(
                source.clone(),
                &path,
                format,
                92,
                None,
                Some(&profile),
            )
            .unwrap();
            let encoded = std::fs::read(&path).unwrap();
            assert!(
                encoded.windows(marker.len()).any(|bytes| bytes == marker),
                "ICC marker missing from {name}"
            );
            assert_eq!(
                embedded_input_profile(path.to_string_lossy().as_ref()),
                Some(ColorSpaceId::DisplayP3),
                "embedded profile is not recognized for {name}"
            );
            assert_eq!(image::open(&path).unwrap().dimensions(), (2, 2));
        }
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn srgb_export_preserves_the_display_referred_positive_signal() {
        let source = ImageBuffer::from_pixel(1, 1, Rgb([11_796, 32_768, 48_168]));
        let encoded =
            encode_export_buffer(source.clone(), crate::color_science::ColorSpaceId::SRgb).unwrap();
        for channel in 0..3 {
            assert!(
                (encoded.get_pixel(0, 0)[channel] as i32 - source.get_pixel(0, 0)[channel] as i32)
                    .abs()
                    <= 1
            );
        }
    }

    #[test]
    fn srgb_jpeg_uses_the_standard_implicit_profile() {
        assert!(export_profile_for_output(ExportFormat::Jpeg, ColorSpaceId::SRgb).is_none());
        assert!(export_profile_for_output(ExportFormat::Png, ColorSpaceId::SRgb).is_some());
        assert!(export_profile_for_output(ExportFormat::Jpeg, ColorSpaceId::DisplayP3).is_some());

        let root = std::env::temp_dir().join(format!(
            "nexfilm-srgb-jpeg-test-{}-{}",
            std::process::id(),
            super::NEXT_ID.fetch_add(1, std::sync::atomic::Ordering::SeqCst)
        ));
        std::fs::create_dir(&root).unwrap();
        let path = root.join("frame.jpg");
        let source = ImageBuffer::from_pixel(2, 2, Rgb([11_796, 32_768, 48_168]));
        let profile = export_profile_for_output(ExportFormat::Jpeg, ColorSpaceId::SRgb);
        write_export_image_with_profile(
            source,
            &path,
            ExportFormat::Jpeg,
            100,
            None,
            profile.as_deref(),
        )
        .unwrap();
        let encoded = std::fs::read(&path).unwrap();
        assert!(
            !encoded.windows(12).any(|bytes| bytes == b"ICC_PROFILE\0"),
            "standard sRGB JPEG must not carry the generated matrix profile"
        );
        let decoded = image::open(&path).unwrap().to_rgb8();
        assert_eq!(decoded.dimensions(), (2, 2));
        let expected = [46u8, 128u8, 188u8];
        for (actual, expected) in decoded.get_pixel(0, 0).0.into_iter().zip(expected) {
            assert!(
                actual.abs_diff(expected) <= 2,
                "sRGB JPEG pixel changed from {expected} to {actual}"
            );
        }
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn proxy_response_keeps_analysis_state_separate_from_density_values() {
        let proxy = ImageBuffer::from_pixel(1, 1, Rgb([1234, 2345, 3456]));
        let analyzed = build_response_buffer_from_proxy(&proxy, &white_base(), true, true);
        let staged = build_response_buffer_from_proxy(&proxy, &BaseColor::default(), true, false);
        assert_eq!(u32::from_le_bytes(analyzed[24..28].try_into().unwrap()), 1);
        assert_eq!(u32::from_le_bytes(staged[24..28].try_into().unwrap()), 0);
        assert_eq!(
            u16::from_le_bytes(analyzed[28..30].try_into().unwrap()),
            1234
        );
    }

    #[test]
    fn capture_corrected_proxy_transports_quality_mask_in_alpha() {
        let proxy = ImageBuffer::from_fn(2, 1, |x, _| {
            if x == 0 {
                Rgb([10_000, 20_000, 30_000])
            } else {
                Rgb([40_000, 50_000, 60_000])
            }
        });
        let pipeline = PipelineState::capture_corrected(DensityAnchors::default(), false);
        let mut quality = crate::raw_backend::QualityMask::new(2);
        quality.invalidate(1, crate::raw_backend::QualityFlag::BadPixel);
        let response = build_response_buffer_from_proxy_with_state(
            &proxy,
            &white_base(),
            &pipeline,
            Some(&quality),
            true,
        );

        let flags = u32::from_le_bytes(response[24..28].try_into().unwrap());
        assert_ne!(flags & 4, 0, "Measured domain flag must be present");
        assert_eq!(
            u16::from_le_bytes(response[34..36].try_into().unwrap()),
            u16::MAX
        );
        assert_eq!(u16::from_le_bytes(response[42..44].try_into().unwrap()), 0);
    }

    #[test]
    fn export_dimensions_preserve_aspect_ratio_and_respect_upscale_policy() {
        assert_eq!(
            export_dimensions(4000, 3000, "long_edge", 2048, false).unwrap(),
            (2048, 1536)
        );
        assert_eq!(
            export_dimensions(400, 300, "long_edge", 2048, false).unwrap(),
            (400, 300)
        );
        assert_eq!(
            export_dimensions(400, 300, "long_edge", 2048, true).unwrap(),
            (2048, 1536)
        );
        assert!(export_dimensions(400, 300, "long_edge", 64, false).is_err());
    }

    #[test]
    fn export_names_are_safe_for_windows_and_reserved_device_names() {
        assert_eq!(
            sanitize_export_file_stem("  Roll:01 / frame*  "),
            "Roll_01 _ frame_"
        );
        assert_eq!(sanitize_export_file_stem("CON"), "_CON");
        assert_eq!(sanitize_export_file_stem("..."), "Export");
    }

    #[test]
    fn export_conflict_policy_allocates_unique_paths_without_races() {
        let root = std::env::temp_dir().join(format!(
            "nexfilm-export-path-test-{}-{}",
            std::process::id(),
            super::NEXT_ID.fetch_add(1, std::sync::atomic::Ordering::SeqCst)
        ));
        std::fs::create_dir(&root).unwrap();
        std::fs::write(root.join("frame.tiff"), b"existing").unwrap();
        let mut reserved = HashSet::new();

        let first = reserve_export_path(
            &root,
            "frame",
            "tiff",
            ExportConflictPolicy::Unique,
            &mut reserved,
        )
        .unwrap();
        let second = reserve_export_path(
            &root,
            "frame",
            "tiff",
            ExportConflictPolicy::Unique,
            &mut reserved,
        )
        .unwrap();
        assert_eq!(first.file_name().unwrap(), "frame (2).tiff");
        assert_eq!(second.file_name().unwrap(), "frame (3).tiff");
        assert!(reserve_export_path(
            &root,
            "frame",
            "tiff",
            ExportConflictPolicy::Skip,
            &mut reserved,
        )
        .is_none());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn export_encodes_all_supported_output_formats() {
        let root = std::env::temp_dir().join(format!(
            "nexfilm-export-format-test-{}-{}",
            std::process::id(),
            super::NEXT_ID.fetch_add(1, std::sync::atomic::Ordering::SeqCst)
        ));
        std::fs::create_dir(&root).unwrap();
        let source = ImageBuffer::from_pixel(3, 2, Rgb([12000, 30000, 50000]));
        let cases = [
            (ExportFormat::Jpeg, "frame.jpg", ColorType::Rgb8),
            (ExportFormat::Png, "frame.png", ColorType::Rgb16),
            (ExportFormat::Tiff8, "frame-8.tiff", ColorType::Rgb8),
            (ExportFormat::Tiff16, "frame-16.tiff", ColorType::Rgb16),
        ];
        for (format, name, color) in cases {
            let path = root.join(name);
            write_export_image(source.clone(), &path, format, 92, None).unwrap();
            let decoded = image::open(&path).unwrap();
            assert_eq!(decoded.dimensions(), (3, 2));
            assert_eq!(decoded.color(), color);
        }
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn export_writes_roll_metadata_to_supported_formats() {
        let root = std::env::temp_dir().join(format!(
            "nexfilm-export-exif-test-{}-{}",
            std::process::id(),
            super::NEXT_ID.fetch_add(1, std::sync::atomic::Ordering::SeqCst)
        ));
        std::fs::create_dir(&root).unwrap();
        let source = ImageBuffer::from_pixel(3, 2, Rgb([12000, 30000, 50000]));
        let metadata = super::ExportMetadata {
            roll_id: "ROLL-07".to_string(),
            film_stock: "Kodak Portra 400".to_string(),
            camera: "Nikon F3".to_string(),
            date: "2026-08-03".to_string(),
        };
        for (format, name) in [
            (ExportFormat::Jpeg, "frame.jpg"),
            (ExportFormat::Png, "frame.png"),
            (ExportFormat::Tiff8, "frame-8.tiff"),
            (ExportFormat::Tiff16, "frame-16.tiff"),
        ] {
            let path = root.join(name);
            write_export_image(source.clone(), &path, format, 92, Some(&metadata)).unwrap();
            assert_eq!(image::open(&path).unwrap().dimensions(), (3, 2));
            let encoded = std::fs::read(path).unwrap();
            assert!(encoded.windows(7).any(|bytes| bytes == b"ROLL-07"));
            assert!(encoded.windows(8).any(|bytes| bytes == b"Nikon F3"));
            assert!(encoded
                .windows(19)
                .any(|bytes| bytes == b"2026:08:03 00:00:00"));
        }
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    #[ignore = "manual performance benchmark"]
    fn benchmark_single_nef_jpeg_export_pipeline() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("test_picture")
            .join("_DSC7333.NEF");
        if !path.exists() {
            return;
        }
        let path = path.to_string_lossy().to_string();
        let decode_started = std::time::Instant::now();
        let decoded = super::decode_image_buffer(&path, super::DecodeMode::ExportFull)
            .expect("NEF full decode should succeed");
        let decode_elapsed = decode_started.elapsed();

        let render_started = std::time::Instant::now();
        let mut rendered = render_shader_equivalent(
            &decoded,
            &neutral_params(),
            &GeometryState::default(),
            &white_base(),
            None,
        );
        let render_elapsed = render_started.elapsed();

        let sharpen_started = std::time::Instant::now();
        super::apply_usm(&mut rendered, 1.0, 0.5);
        let sharpen_elapsed = sharpen_started.elapsed();

        let root = std::env::temp_dir().join(format!(
            "nexfilm-export-benchmark-{}",
            super::NEXT_ID.fetch_add(1, std::sync::atomic::Ordering::SeqCst)
        ));
        std::fs::create_dir(&root).unwrap();
        let encode_started = std::time::Instant::now();
        super::write_export_image(
            rendered,
            &root.join("benchmark.jpg"),
            super::ExportFormat::Jpeg,
            100,
            None,
        )
        .unwrap();
        let encode_elapsed = encode_started.elapsed();
        println!(
            "JPEG export benchmark: source={}x{}, decode={decode_elapsed:?}, render={render_elapsed:?}, sharpen={sharpen_elapsed:?}, encode={encode_elapsed:?}",
            decoded.width(),
            decoded.height()
        );
        std::fs::remove_dir_all(root).unwrap();
    }
}
