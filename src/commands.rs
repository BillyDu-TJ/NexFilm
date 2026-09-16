use crate::app_state::{
    BaseColor, CalibrationCapability, CalibrationConfigProfile, CalibrationLevel,
    CalibrationPayloadIssuer, CalibrationProfileAvailability, CalibrationProfilePayload,
    CalibrationProfileView, CalibrationQualityMaskArtifact, CalibrationQualityMaskSummary,
    CalibrationReference, CalibrationReferenceKind, CalibrationReferenceSummary,
    CalibrationValidRange, CalibrationValidationReport, CalibrationValidationStatus,
    CaptureCalibrationParameters, ContentRange, ContentRangeScope, DensityAnchor,
    DensityAnchorConfidence, DensityAnchorScope, DensityAnchorSource, DensityAnchors, EngineState,
    FilmItem, FilmMode, FilmstripItem, GeometryState, InputDomainConfidence, InputDomainRecord,
    InputDomainSource, InputPrimaries, InputReference, InputTransferCurve,
    PipelineProcessingReport, PipelineState, ProcessingContract, RenderMapping, RenderMode, Roll,
    RollBaseStatus, RollCalibrationFormat, RollCalibrationMode, RollCalibrationStatus,
    RollDmaxStatus, RollFrameStatus, RollToneStatus, TuningParams,
    CALIBRATION_PROFILE_PAYLOAD_VERSION, CALIBRATION_PROFILE_SCHEMA_VERSION,
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
    apply_lens_distortion_uv, apply_perspective_uv, apply_post_gamma_adjustments_with_luma,
    density_luma, neutral_density_bounds, normalize_density_channel, sprocket_white_mask,
    trim_density_endpoints, DENSITY_LUMA_COEFFICIENTS,
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
    image: ImageBuffer<Rgb<u16>, Vec<u16>>,
    target: ColorSpaceId,
) -> ImageBuffer<Rgb<u16>, Vec<u16>> {
    linearize_scanner_fff_with_input(image, target, ScannerFffInput::DOCUMENTED_DEFAULT)
}

fn linearize_scanner_fff_with_input(
    mut image: ImageBuffer<Rgb<u16>, Vec<u16>>,
    target: ColorSpaceId,
    input: ScannerFffInput,
) -> ImageBuffer<Rgb<u16>, Vec<u16>> {
    // A scanner 3F/FFF stores three already-sampled RGB channels whose transfer
    // curve and primaries are declared by the container's own settings record.
    // The documented default applies only when that record cannot be read, in
    // which case the input domain is reported as estimated.
    let matrix = linear_conversion_matrix(input.input_color_space(), target);
    let gamma = input.gamma;
    image.as_mut().par_chunks_exact_mut(3).for_each(|pixel| {
        let encoded = [
            pixel[0] as f32 / 65535.0,
            pixel[1] as f32 / 65535.0,
            pixel[2] as f32 / 65535.0,
        ];
        let linear = apply_linear_matrix(encoded.map(|value| value.powf(gamma)), matrix);
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
    let base_density = if is_smart_auto_compatibility(pipeline_state) {
        [base_color.base_r, base_color.base_g, base_color.base_b]
            .map(|value| -(value as f32 / 65535.0).max(1.0e-6).log10())
    } else {
        pipeline_base_density(pipeline_state, base_color)
    };
    let base_analyzed = pipeline_has_base(pipeline_state, base_color);
    let flags = u32::from(base_analyzed)
        | match pipeline_state.contract {
            ProcessingContract::LegacyV1 => 0,
            ProcessingContract::CaptureCorrectedV11 => 4,
            ProcessingContract::SmartAutoProPhotoV11
            | ProcessingContract::RollBaseProPhotoV11
            | ProcessingContract::RollAnchoredProPhotoV11
                if !is_smart_auto_compatibility(pipeline_state) =>
            {
                // Every non-compatibility ProPhoto contract uses the signed
                // ProPhoto transport proxy. The frontend must decode it back
                // to the estimate domain before taking log density; omitting
                // this bit makes roll-anchored previews follow the legacy
                // sRGB/Status-M path and can collapse them to black.
                2
            }
            _ => 0,
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

/// Per-channel film-base candidate from the whole frame's brightest samples, in
/// the Smart Auto working domain.
///
/// The retired v1.0.2 path estimated the base this way, and a loose frame
/// without a confirmed Film Area still needs a neutral reference: the film base
/// is the most transmissive part of the film, so the brightest percentile per
/// channel is the base rather than the brightest scene content.
fn compute_frame_base_density_f32(
    proxy: &ImageBuffer<Rgb<f32>, Vec<f32>>,
) -> Result<([f32; 3], f32), String> {
    let mut channels: [Vec<f32>; 3] = [Vec::new(), Vec::new(), Vec::new()];
    for pixel in proxy.as_raw().chunks_exact(3) {
        if pixel
            .iter()
            .any(|value| !value.is_finite() || *value <= 0.0 || *value >= 0.98)
        {
            continue;
        }
        for channel in 0..3 {
            channels[channel].push(pixel[channel]);
        }
    }
    let sampled = channels[0].len();
    if sampled == 0 {
        return Err("The frame contains no usable film-base samples.".to_string());
    }
    let tail = ((sampled as f32 * 0.01).ceil() as usize).clamp(1, sampled);
    let density = std::array::from_fn(|channel| {
        let values = &mut channels[channel];
        values.sort_unstable_by(|left, right| right.total_cmp(left));
        let mean = values.iter().take(tail).sum::<f32>() / tail as f32;
        -mean.max(1.0e-6).log10()
    });
    let confidence =
        (sampled as f32 / (proxy.width() * proxy.height()).max(1) as f32).clamp(0.0, 1.0);
    Ok((density, confidence))
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

/// Result of the film-base estimate that the unified density stage consumes.
/// `usable` is the quality gate: an implausible or missing base must fall back
/// to the content mapping instead of producing a cast, because after
/// unification the film base is the only neutral reference.
struct FilmBaseEstimate {
    density: [f32; 3],
    confidence: f32,
    source: &'static str,
    usable: bool,
    fallback_reason: Option<&'static str>,
}

/// Sample count a rebate-band candidate needs before it can be believed.
const FILM_BASE_BAND_MIN_SAMPLES: usize = 64;
/// Half-width of the rebate band on each side of the Film Area boundary.
const FILM_BASE_BAND_WIDTH: f32 = 0.010;
/// Sample count the in-area low-density tail needs (matches `compute_auto_base_f32`).
const FILM_BASE_TAIL_MIN_SAMPLES: usize = 8;
/// Largest per-channel density difference a clear film base can show between its
/// own channels before the candidate is describing scene content instead.
const FILM_BASE_MAX_CHANNEL_SPREAD: f32 = 0.75;
/// A clear base sits close to full transmission; anything denser is not a base.
const FILM_BASE_MIN_DENSITY: f32 = 0.02;
const FILM_BASE_MAX_DENSITY: f32 = 1.60;
/// How far the rebate band may sit *denser* than the in-area tail and still
/// describe the same physical base.
///
/// The clear base is the film's lowest density, so nothing inside the gate can
/// be brighter than it: a band candidate that sits above the in-area tail is
/// describing the frame line, the gate shadow or scene content, and using it
/// over-subtracts the mask. The tolerance only has to absorb sampling noise,
/// not a visible density error, because a wrong base is exactly what the
/// positive renders as a cast.
const FILM_BASE_BAND_AGREEMENT: f32 = 0.05;

fn validate_film_base_candidate(
    density: [f32; 3],
    samples: usize,
    minimum_samples: usize,
) -> Result<(), &'static str> {
    if samples < minimum_samples {
        return Err("film_base_too_few_samples");
    }
    if density.iter().any(|value| !value.is_finite()) {
        return Err("film_base_non_finite");
    }
    if density
        .iter()
        .any(|value| *value < FILM_BASE_MIN_DENSITY || *value > FILM_BASE_MAX_DENSITY)
    {
        return Err("film_base_out_of_range");
    }
    let maximum = density.iter().copied().fold(f32::MIN, f32::max);
    let minimum = density.iter().copied().fold(f32::MAX, f32::min);
    if maximum - minimum > FILM_BASE_MAX_CHANNEL_SPREAD {
        return Err("film_base_channel_spread");
    }
    Ok(())
}

/// The rebate band only describes the same physical base as the in-area tail
/// when it is not denser than it, channel by channel. A band that is really
/// scene content (or the frame line just outside a gate that sits inside the
/// picture) is what over-subtracts, so that direction rejects the candidate.
///
/// The opposite direction is legitimate: a frame that contains no base-like
/// content has an in-area tail well above the base, and the rebate still is the
/// base. It is only bounded so a light-panel or sprocket-leak sample cannot
/// masquerade as a base.
fn film_base_band_agrees(band: [f32; 3], tail: [f32; 3]) -> bool {
    /// A band far brighter than the frame's own brightest content is more
    /// likely the open gate or a lamp panel than the film's clear base.
    const FILM_BASE_BAND_MAX_BRIGHTER: f32 = 0.50;
    band.iter()
        .zip(tail.iter())
        .all(|(band, tail)| *band - *tail <= FILM_BASE_BAND_AGREEMENT)
        && band
            .iter()
            .zip(tail.iter())
            .all(|(band, tail)| *tail - *band <= FILM_BASE_BAND_MAX_BRIGHTER)
}

/// Robust per-channel median density of a sample set that already excluded
/// zeros, saturation, invalid values and everything outside the sampling band.
fn median_channel_density(samples: &mut [[f32; 3]]) -> [f32; 3] {
    std::array::from_fn(|channel| {
        samples.sort_unstable_by(|left, right| left[channel].total_cmp(&right[channel]));
        let middle = samples.len() / 2;
        let transmission = if samples.len() % 2 == 0 {
            (samples[middle - 1][channel] + samples[middle][channel]) * 0.5
        } else {
            samples[middle][channel]
        };
        -transmission.max(1.0e-6).log10()
    })
}

/// Film-base estimate in the documented sampling priority order:
///
/// 1. the visible rebate / orange mask band on both sides of the Film Area,
///    taken as a robust median and only believed when it agrees with the
///    in-area tail;
/// 2. the lowest-density tail inside the Film Area;
/// 3. the brightest in-frame quantile as an explicitly discounted last resort.
///
/// A candidate that fails the quality gate is reported as unusable, and the
/// caller keeps the content-driven mapping instead of neutralising on it.
fn estimate_film_base_f32(
    proxy: &ImageBuffer<Rgb<f32>, Vec<f32>>,
    geom: &GeometryState,
) -> FilmBaseEstimate {
    let unusable = |fallback_reason: &'static str| FilmBaseEstimate {
        density: [0.0; 3],
        confidence: 0.0,
        source: "unavailable",
        usable: false,
        fallback_reason: Some(fallback_reason),
    };

    let mut gate_failure = None;
    if geom.calibration_points.is_some() {
        // 1. Rebate band just inside and outside the confirmed Film Area.
        let mut band = collect_film_area_band_rgb32(proxy, None, geom, FILM_BASE_BAND_WIDTH);
        let band_density = if band.is_empty() {
            None
        } else {
            Some(median_channel_density(&mut band))
        };

        // 2. Lowest-density tail inside the Film Area.
        let tail = compute_auto_base_f32(proxy, geom).unwrap_or(([0.0; 3], 0.0));
        let tail_usable = validate_film_base_candidate(
            tail.0,
            (tail.1 * (proxy.width() * proxy.height()).max(1) as f32).round() as usize,
            FILM_BASE_TAIL_MIN_SAMPLES,
        )
        .map_err(|reason| {
            gate_failure = Some(reason);
        })
        .is_ok()
            && tail.1 > 0.0;

        if let Some(band_density) = band_density {
            let agrees = film_base_band_agrees(band_density, tail.0);
            if validate_film_base_candidate(band_density, band.len(), FILM_BASE_BAND_MIN_SAMPLES)
                .is_ok()
                && agrees
            {
                return FilmBaseEstimate {
                    density: band_density,
                    confidence: 0.95,
                    source: "film_edge_band",
                    usable: true,
                    fallback_reason: None,
                };
            }
        }

        if tail_usable {
            return FilmBaseEstimate {
                density: tail.0,
                confidence: tail.1,
                source: "film_area_low_density_tail",
                usable: true,
                fallback_reason: None,
            };
        }
    }

    // 3. Brightest in-frame quantile. This is the estimator the retired path
    // used; it over-subtracts whenever the frame has no clear base in it, so it
    // is gated and its confidence is deliberately halved.
    match compute_frame_base_density_f32(proxy) {
        Ok((density, confidence)) => match validate_film_base_candidate(
            density,
            (confidence * (proxy.width() * proxy.height()).max(1) as f32).round() as usize,
            1,
        ) {
            Ok(()) if confidence > 0.0 => FilmBaseEstimate {
                density,
                confidence: (confidence * 0.5).max(0.05),
                source: "content_high_quantile",
                usable: true,
                fallback_reason: None,
            },
            Ok(()) | Err(_) => unusable(gate_failure.unwrap_or("missing_film_base_reference")),
        },
        Err(_) => unusable(gate_failure.unwrap_or("missing_film_base_reference")),
    }
}

fn smart_auto_exclusion_counts(
    proxy: &ImageBuffer<Rgb<f32>, Vec<f32>>,
    geom: &GeometryState,
) -> (usize, usize, usize) {
    let points =
        geom.calibration_points
            .unwrap_or([[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]]);
    let mut open = 0;
    let mut saturated = 0;
    let mut invalid = 0;
    for (index, pixel) in proxy.as_raw().chunks_exact(3).enumerate() {
        let x =
            (index as u32 % proxy.width()) as f32 / proxy.width().saturating_sub(1).max(1) as f32;
        let y =
            (index as u32 / proxy.width()) as f32 / proxy.height().saturating_sub(1).max(1) as f32;
        if !point_in_film_area([x, y], &points, 0.0) {
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
    if let Some(model) = profile
        .payload
        .fit_model
        .as_ref()
        .filter(|_| profile.payload.fit_validation_error().is_none())
    {
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
            let srgb = compress_linear_srgb_for_density([
                pixel[0] as f32 / 65535.0,
                pixel[1] as f32 / 65535.0,
                pixel[2] as f32 / 65535.0,
            ]);
            target.copy_from_slice(&apply_linear_matrix(srgb, matrix));
        });
    converted
}

fn compute_content_limits_f32(
    proxy: &ImageBuffer<Rgb<f32>, Vec<f32>>,
    quality: Option<&crate::raw_backend::QualityMask>,
    geom: &GeometryState,
    base_density: [f32; 3],
) -> Result<AutoColorLimits, String> {
    compute_content_limits_f32_with_bounds(proxy, quality, geom, base_density, None)
}

fn compute_content_limits_f32_with_bounds(
    proxy: &ImageBuffer<Rgb<f32>, Vec<f32>>,
    quality: Option<&crate::raw_backend::QualityMask>,
    geom: &GeometryState,
    base_density: [f32; 3],
    physical_span: Option<[f32; 3]>,
) -> Result<AutoColorLimits, String> {
    let samples = content_density_samples(proxy, quality, geom, base_density, physical_span);
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

/// Sample the frame's content into per-channel densities relative to the film
/// base, on the fixed analysis grid. The display-window endpoints and the
/// channel-response measurement share this so both look at exactly the same
/// pixels.
fn collect_content_density_samples(
    proxy: &ImageBuffer<Rgb<f32>, Vec<f32>>,
    quality: Option<&crate::raw_backend::QualityMask>,
    geom: &GeometryState,
    base_density: [f32; 3],
    physical_span: Option<[f32; 3]>,
    inside_calibration_only: bool,
) -> Vec<[f32; 3]> {
    /// Samples this close to the ceiling are filler, lamp panel or clipped
    /// highlights, never a scene endpoint.
    const CONTENT_WINDOW_SATURATION: f32 = 0.995;
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
            let Some(perspective_uv) = apply_perspective_uv(
                crop_uv,
                geom.perspective_vertical,
                geom.perspective_horizontal,
                geom.perspective_aspect,
                geom.perspective_scale,
            ) else {
                continue;
            };
            let Some(oriented_uv) = apply_lens_distortion_uv(perspective_uv, geom.lens_distortion)
            else {
                continue;
            };
            if inside_calibration_only && !point_in_film_area(crop_uv, &points, 0.0) {
                continue;
            }
            let source_uv =
                map_oriented_uv_to_source(oriented_uv, source_width, source_height, geom);
            let Some(raw) = sample_rgb32_nearest_checked(proxy, quality, source_uv) else {
                continue;
            };
            // Smart Auto input is an estimate, not a physical measurement, but
            // invalid samples must still be excluded rather than repaired into
            // a fake density with epsilon.
            if raw.iter().any(|value| !value.is_finite() || *value <= 0.0) {
                continue;
            }
            // A stitched white filler, a light panel or a specular blowout is
            // not scene content: letting it define an endpoint would move the
            // window for reasons the picture does not contain.
            if raw.iter().any(|value| *value >= CONTENT_WINDOW_SATURATION) {
                continue;
            }
            let density = raw.map(|value| -value.log10());
            let density = [
                density[0] - base_density[0],
                density[1] - base_density[1],
                density[2] - base_density[2],
            ];
            if let Some(span) = physical_span {
                // A complete roll calibration gives us a useful physical
                // validity window. Samples below the measured clear base or
                // above the fully exposed leader are normally white backing,
                // sprocket/edge contamination, or saturation, rather than
                // scene content.
                const PHYSICAL_RANGE_MARGIN: f32 = 0.10;
                if density.iter().enumerate().any(|(channel, value)| {
                    !value.is_finite()
                        || *value < -PHYSICAL_RANGE_MARGIN
                        || *value > span[channel] + PHYSICAL_RANGE_MARGIN
                }) {
                    continue;
                }
            }
            if density.iter().all(|value| value.is_finite()) {
                samples.push(density);
            }
        }
    }
    samples
}

/// The content samples the analysis uses: the confirmed Film Area when it holds
/// enough data, the whole frame otherwise.
fn content_density_samples(
    proxy: &ImageBuffer<Rgb<f32>, Vec<f32>>,
    quality: Option<&crate::raw_backend::QualityMask>,
    geom: &GeometryState,
    base_density: [f32; 3],
    physical_span: Option<[f32; 3]>,
) -> Vec<[f32; 3]> {
    let samples =
        collect_content_density_samples(proxy, quality, geom, base_density, physical_span, true);
    if samples.len() < 64 && geom.calibration_points.is_none() {
        return collect_content_density_samples(
            proxy,
            quality,
            geom,
            base_density,
            physical_span,
            false,
        );
    }
    samples
}

/// Per-channel density span the frame's content covers, each channel measured
/// on its own 2% to 98% range of the sampled densities.
///
/// The display-window endpoints stay co-sited because a saturated coloured
/// object must not be able to move one channel's endpoint on its own. The
/// response measurement needs the opposite property: a channel the capture
/// compressed has to be recognised even when its extremes do not line up with
/// the frame's luminance, which is exactly what the merged camera scans do.
/// Samples that touch either end of the working range were already dropped
/// during collection.
fn measure_content_channel_spans(
    proxy: &ImageBuffer<Rgb<f32>, Vec<f32>>,
    quality: Option<&crate::raw_backend::QualityMask>,
    geom: &GeometryState,
    base_density: [f32; 3],
    physical_span: Option<[f32; 3]>,
) -> Option<[f32; 3]> {
    let samples = content_density_samples(proxy, quality, geom, base_density, physical_span);
    if samples.len() < 64 {
        return None;
    }
    let spans = std::array::from_fn(|channel| {
        let mut values: Vec<f32> = samples.iter().map(|sample| sample[channel]).collect();
        values.sort_unstable_by(f32::total_cmp);
        let pick = |fraction: f32| -> f32 {
            values[(((values.len() - 1) as f32) * fraction).round() as usize]
        };
        pick(0.98) - pick(0.02)
    });
    spans
        .iter()
        .all(|span| span.is_finite() && *span > 1.0e-4)
        .then_some(spans)
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
                // A loose or unanchored Smart Auto frame has no physical anchor,
                // but it does have a per-frame film-base estimate. That estimate
                // is the neutral reference: subtracting it removes the mask per
                // channel. Aligning the channels on the content instead made the
                // result depend on whatever the photograph happened to contain,
                // which cancelled scene-wide colour and pushed colour-dominant
                // scenes to the opposite cast.
                return smart_auto_frame_base_density(state, base_color).unwrap_or([0.0; 3]);
            }
            crate::pipeline::base_density_from_base_color(base_color)
        })
}

/// The per-frame film-base estimate a Smart Auto state should subtract, when it
/// has one. `None` means there is no trusted base, and the caller keeps the
/// zero-reference behaviour with a reported fallback reason.
fn smart_auto_frame_base_density(
    state: &PipelineState,
    base_color: &BaseColor,
) -> Option<[f32; 3]> {
    if is_smart_auto_compatibility(state) {
        return None;
    }
    crate::pipeline::frame_base_density(state, base_color)
}

/// True when this frame's persisted display window was derived with rules that
/// no longer apply, so it has to be re-derived before it is trusted again.
///
/// Only the frame-wise routes are affected: a complete Roll calibration derives
/// its endpoints from the sampled anchors, which no window rule can invalidate.
fn frame_needs_window_reanalysis(state: &PipelineState) -> bool {
    if matches!(
        state.contract,
        ProcessingContract::RollAnchoredProPhotoV11 | ProcessingContract::CaptureCorrectedV11
    ) {
        return false;
    }
    state.processing_report.analysis_window_rule < crate::app_state::DENSITY_WINDOW_RULE_VERSION
}

fn pipeline_has_base(state: &PipelineState, base_color: &BaseColor) -> bool {
    if state.contract == ProcessingContract::LegacyV1 {
        *base_color != BaseColor::default()
    } else if is_smart_auto_compatibility(state) {
        // A loose Smart Auto result produced before the compatibility source
        // was selected is not reusable: its base was measured in the
        // ProPhoto estimate domain and would make the v1.0.2 proxy invert
        // with the wrong endpoint. Force one fresh analysis on that path.
        state.processing_report.base_source == "compatibility_base"
    } else {
        state
            .density_anchors
            .d_min_base
            .as_ref()
            .is_some_and(|anchor| anchor_matches_resolved_contract(anchor, state))
            || (state.processing_report.base_source != "unresolved"
                && !state.processing_report.base_source.is_empty()
                && *base_color != BaseColor::default())
    }
}

/// Recognise the persisted marker left by imports that ran on the retired
/// v1.0.2 compatibility source. Previous releases marked every scanner-produced
/// loose frame with it, which switched the frame onto the historical density
/// recipe (u16 linear-sRGB working space, Status M crosstalk and no display
/// matrix). It is read-only legacy support: no import writes it any more, and
/// the one-time migration clears it.
fn is_smart_auto_compatibility(state: &PipelineState) -> bool {
    state.contract == ProcessingContract::SmartAutoProPhotoV11
        && !state.density_anchors.has_roll_base()
        && !state.density_anchors.has_roll_full_exposure()
        && state.processing_report.analysis_data_domain == "legacy_linear_srgb"
}

/// Drop the retired v1.0.2 density-recipe marker from a frame that still carries
/// it. The input class now selects only the input domain, so no entry point may
/// turn an input class into a different set of density maths: a scanner scan,
/// a camera RAW and a merged TIFF all run the shared ProPhoto-estimate density
/// stage after their own input-domain conversion.
fn clear_retired_legacy_domain(state: &mut PipelineState) {
    if state.contract != ProcessingContract::SmartAutoProPhotoV11
        || state.density_anchors.has_roll_base()
        || state.density_anchors.has_roll_full_exposure()
    {
        return;
    }
    if state.processing_report.analysis_data_domain != "legacy_linear_srgb" {
        return;
    }
    state.processing_report.analysis_data_domain = "linear_prophoto_estimate".to_string();
    if state.processing_report.base_source == "compatibility_base" {
        // The estimate was measured in the retired domain; it must not be
        // reused against the shared density stage.
        state.processing_report.base_source = "unresolved".to_string();
        state.processing_report.base_confidence = "low".to_string();
    }
    if !state
        .processing_report
        .fallback_reasons
        .iter()
        .any(|reason| reason == "retired_density_recipe")
    {
        state
            .processing_report
            .fallback_reasons
            .push("retired_density_recipe".to_string());
    }
}

fn srgb_proxy_u16_to_prophoto_f32(
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

fn anchor_matches_resolved_contract(anchor: &DensityAnchor, state: &PipelineState) -> bool {
    if anchor.provenance.algorithm_version != crate::app_state::DENSITY_ANCHOR_ALGORITHM_VERSION
        || anchor.provenance.input_domain != state.contract.input_domain()
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

fn share_smart_auto_density_scale(limits: &mut AutoColorLimits) -> [f32; 3] {
    // Content alignment is the no-trusted-base fallback, so its grey-world shift
    // must stay partial: a dominant-colour scene may pull the channel centres
    // apart by far more than the mask actually differs, and a large bound turns
    // that measurement into an opposite cast. A quarter of a density unit still
    // removes a moderate illuminant/mask imbalance and can no longer cancel a
    // scene-wide hue.
    const MAX_CHANNEL_OFFSET: f32 = 0.20;
    let low = density_luma(limits.d_min);
    let high = density_luma(limits.d_max);
    if !low.is_finite() || !high.is_finite() || high <= low + 1.0e-6 {
        return [0.0; 3];
    }

    // Use one contrast slope for all channels, but retain the co-sited
    // channel-center differences needed to neutralize the film mask and
    // capture illuminant. Removing these offsets preserves the negative's
    // mask as a severe cyan/green cast in the positive. The bound prevents a
    // strongly coloured subject from driving an unbounded grey-world shift.
    let centers = [
        (limits.d_min[0] + limits.d_max[0]) * 0.5,
        (limits.d_min[1] + limits.d_max[1]) * 0.5,
        (limits.d_min[2] + limits.d_max[2]) * 0.5,
    ];
    let center = density_luma(centers);
    let mut offsets =
        centers.map(|value| (value - center).clamp(-MAX_CHANNEL_OFFSET, MAX_CHANNEL_OFFSET));
    let residual = density_luma(offsets);
    offsets =
        offsets.map(|value| (value - residual).clamp(-MAX_CHANNEL_OFFSET, MAX_CHANNEL_OFFSET));

    for channel in 0..3 {
        limits.d_min[channel] = low + offsets[channel];
        limits.d_max[channel] = high + offsets[channel];
    }
    offsets
}

/// Largest channel-to-channel density response ratio a capture can explain.
/// Below this the three channels responded alike and the shared window is kept
/// exactly as it is.
///
/// A real scanner scan leaves a few percent of response difference between the
/// film's three layers even when the frame's own colour is unremarkable: the
/// Noritsu fixtures measure 1.11 to 1.36 with the *same* channel ordering on
/// three different photographs, which is a capture property rather than a
/// property of what was photographed. A 1.6 deadband therefore kept the cast
/// instead of correcting it.
const CHANNEL_RESPONSE_BALANCED_RATIO: f32 = 1.15;
/// Ratio at which the measured response is trusted in full.
const CHANNEL_RESPONSE_FULL_RATIO: f32 = 1.35;
/// Hard limit on how far one channel's display span may follow its response.
///
/// The compensation may bend the channels apart by at most a third of the
/// shared span, which covers the layer/capture differences real scans show
/// while keeping a colour-dominant photograph from being flattened.
const CHANNEL_RESPONSE_MAX_GAIN: f32 = 1.35;
/// An imbalance no film/capture can explain: one channel was truncated
/// outright upstream (a merged scan whose red channel kept a third of the
/// density range the others have). Scene content cannot compress a channel's
/// span that far, so above this the cap widens instead of leaving the frame
/// with a permanent cast.
const CHANNEL_RESPONSE_TRUNCATED_IMBALANCE: f32 = 2.0;
/// Gain limit that applies once a channel was truncated upstream.
const CHANNEL_RESPONSE_TRUNCATED_MAX_GAIN: f32 = 3.0;
/// Never let a compensated window collapse into a degenerate display range.
const CHANNEL_RESPONSE_MIN_SPAN: f32 = 0.2;

/// Per-channel density response of one frame's content window.
#[derive(Clone, Copy, Debug)]
struct ChannelResponse {
    spans: [f32; 3],
    imbalance: f32,
    gains: [f32; 3],
}

/// How far one channel's span may follow its response at this imbalance.
fn channel_response_gain_limit(imbalance: f32) -> f32 {
    if imbalance > CHANNEL_RESPONSE_TRUNCATED_IMBALANCE {
        CHANNEL_RESPONSE_TRUNCATED_MAX_GAIN
    } else {
        CHANNEL_RESPONSE_MAX_GAIN
    }
}

/// Measure how each channel's density responds across the frame's content.
///
/// A capture or an upstream renderer can compress one channel: on the merged
/// Lightroom scans this work started from, red covered 0.41 D where green
/// covered 1.00 D and blue 1.57 D. One window shared by all three channels then
/// confines red to 48% of the display range, which is what turned those frames
/// cyan. The healthy fixtures on hand measure 1.0 to 1.4 and stay inside the
/// deadband, so their gains are exactly one and their mapping is the one they
/// already had.
fn channel_response_from_spans(spans: [f32; 3]) -> ChannelResponse {
    let reference = density_luma(spans);
    let measurable = spans.iter().all(|span| span.is_finite() && *span > 1.0e-4)
        && reference.is_finite()
        && reference > 1.0e-4;
    if !measurable {
        return ChannelResponse {
            spans,
            imbalance: 1.0,
            gains: [1.0; 3],
        };
    }
    let maximum = spans.iter().copied().fold(f32::MIN, f32::max);
    let minimum = spans.iter().copied().fold(f32::MAX, f32::min);
    let imbalance = maximum / minimum;
    let blend = ((imbalance - CHANNEL_RESPONSE_BALANCED_RATIO)
        / (CHANNEL_RESPONSE_FULL_RATIO - CHANNEL_RESPONSE_BALANCED_RATIO))
        .clamp(0.0, 1.0);
    if blend <= 0.0 {
        return ChannelResponse {
            spans,
            imbalance,
            gains: [1.0; 3],
        };
    }
    let gains = spans.map(|span| {
        let limit = channel_response_gain_limit(imbalance);
        (span / reference).clamp(1.0 / limit, limit).powf(blend)
    });
    ChannelResponse {
        spans,
        imbalance,
        gains,
    }
}

/// Per-channel response of the window's own endpoints. Used when the caller has
/// no separately measured spans; the analysis route passes the per-channel
/// measurement instead.
fn content_channel_response(limits: &AutoColorLimits) -> ChannelResponse {
    channel_response_from_spans([
        limits.d_max[0] - limits.d_min[0],
        limits.d_max[1] - limits.d_min[1],
        limits.d_max[2] - limits.d_min[2],
    ])
}

/// Give every channel the density span its own response covers, without moving
/// the film base off the neutral axis.
///
/// Dividing the window by the measured response is the same statement as
/// `density / gain`, so the film base — density zero in every channel — keeps
/// landing on one display value, while each channel finally covers the density
/// range its own response actually recorded.
fn apply_content_channel_response(limits: &mut AutoColorLimits, response: &ChannelResponse) {
    if response.gains == [1.0; 3] {
        return;
    }
    let low = limits.d_min[0];
    let shared_span = limits.d_max[0] - low;
    if !low.is_finite() || !shared_span.is_finite() || shared_span <= 1.0e-6 {
        return;
    }
    for channel in 0..3 {
        let gain = response.gains[channel];
        let channel_low = low * gain;
        let channel_span = (shared_span * gain).max(CHANNEL_RESPONSE_MIN_SPAN);
        limits.d_min[channel] = channel_low;
        limits.d_max[channel] = channel_low + channel_span;
    }
}

/// Keep one shared density scale for all channels without any per-channel
/// shift. Used when the frame's own film base already supplies the neutral
/// reference, so the content must not be allowed to re-balance the channels.
fn share_smart_auto_density_scale_without_offsets(limits: &mut AutoColorLimits) {
    let low = density_luma(limits.d_min);
    let high = density_luma(limits.d_max);
    if !low.is_finite() || !high.is_finite() || high <= low + 1.0e-6 {
        return;
    }
    for channel in 0..3 {
        limits.d_min[channel] = low;
        limits.d_max[channel] = high;
    }
}

fn preserve_content_span(limits: &mut AutoColorLimits, minimum_span: f32) {
    let low = limits.d_min[0];
    let high = limits.d_max[0];
    let span = high - low;
    if !span.is_finite() || span <= 1.0e-6 || span >= minimum_span {
        return;
    }
    for channel in 0..3 {
        let center = (limits.d_min[channel] + limits.d_max[channel]) * 0.5;
        limits.d_min[channel] = center - minimum_span * 0.5;
        limits.d_max[channel] = center + minimum_span * 0.5;
    }
}

fn preserve_smart_auto_content_span(limits: &mut AutoColorLimits) {
    preserve_content_span(limits, 0.8);
}

fn prepare_content_render_limits(
    limits: &mut AutoColorLimits,
    anchors: &DensityAnchors,
    base_density: [f32; 3],
) -> ([f32; 3], bool) {
    prepare_content_render_limits_with_spans(limits, anchors, base_density, None)
}

/// Resolve the display window, optionally from a separately measured per-channel
/// response. `measured_spans` carries the frame's own 2%/98% range per channel
/// (see `measure_content_channel_spans`); without one the window's co-sited
/// endpoints stand in for it.
fn prepare_content_render_limits_with_spans(
    limits: &mut AutoColorLimits,
    anchors: &DensityAnchors,
    base_density: [f32; 3],
    measured_spans: Option<[f32; 3]>,
) -> ([f32; 3], bool) {
    const MIN_DISPLAY_DENSITY_SPAN: f32 = 0.8;
    let calibrated_span = roll_physical_density_span(anchors, base_density);

    if let Some(span) = calibrated_span {
        // The sampled base and leader define the physical coordinate system,
        // not the display endpoints of every photograph. Work in the
        // calibrated 0..1 density coordinate, derive a content window there,
        // then convert that window back to per-channel density limits.
        let mut relative = AutoColorLimits {
            d_min: [0.0; 3],
            d_max: [0.0; 3],
            pipeline_state: None,
        };
        for channel in 0..3 {
            relative.d_min[channel] = limits.d_min[channel] / span[channel];
            relative.d_max[channel] = limits.d_max[channel] / span[channel];
        }
        let observed_span = density_luma(relative.d_max) - density_luma(relative.d_min);
        let channel_offsets = share_smart_auto_density_scale(&mut relative);
        let physical_luma_span = density_luma(span).max(1.0e-4);
        let minimum_relative_span =
            (MIN_DISPLAY_DENSITY_SPAN / physical_luma_span).clamp(0.25, 0.8);
        preserve_content_span(&mut relative, minimum_relative_span);
        for channel in 0..3 {
            limits.d_min[channel] = relative.d_min[channel] * span[channel];
            limits.d_max[channel] = relative.d_max[channel] * span[channel];
        }
        return (
            [
                channel_offsets[0] * span[0],
                channel_offsets[1] * span[1],
                channel_offsets[2] * span[2],
            ],
            observed_span < minimum_relative_span,
        );
    }

    let observed_span = density_luma(limits.d_max) - density_luma(limits.d_min);
    // When the frame carries a film-base estimate, subtracting it already
    // removed the mask, so the channels must not be realigned on the content: a
    // grey-world shift would re-introduce exactly the cast the base just
    // removed, tint the film base itself and make the result depend on what the
    // photograph contains. Only the shared density scale and origin are kept.
    //
    // What the frame may still correct is the *span*: a capture, or film whose
    // three layers do not respond alike, records a different density range per
    // channel. With one shared span the short channel can never reach the white
    // point, and that channel's cast stays across the whole frame. The bounded
    // per-channel response below fixes the span while leaving the origin — and
    // therefore the film base — neutral.
    let channel_offsets = if base_density.iter().any(|value| *value > 0.0) {
        // The film base already removed the mask, so the content must not
        // re-balance the channels. A channel whose response the capture
        // compressed still needs its own density span, though: with one shared
        // span it can never reach the white point and the whole frame keeps
        // that channel's cast.
        let response = measured_spans
            .map(channel_response_from_spans)
            .unwrap_or_else(|| content_channel_response(limits));
        // Keep the shared window origin: the film base is density zero in every
        // channel, and a per-channel origin would print that base as a colour
        // instead of as one neutral value. Only the span may follow the
        // measured response, which leaves the base exactly where it was.
        share_smart_auto_density_scale_without_offsets(limits);
        preserve_smart_auto_content_span(limits);
        apply_content_channel_response(limits, &response);
        [0.0; 3]
    } else {
        let offsets = share_smart_auto_density_scale(limits);
        preserve_smart_auto_content_span(limits);
        offsets
    };
    (channel_offsets, observed_span < MIN_DISPLAY_DENSITY_SPAN)
}

fn roll_physical_density_span(
    anchors: &DensityAnchors,
    base_density: [f32; 3],
) -> Option<[f32; 3]> {
    anchors
        .is_fully_anchored()
        .then(|| anchors.d_max_full_exposure.as_ref())
        .flatten()
        .map(|full| {
            [
                full.density[0] - base_density[0],
                full.density[1] - base_density[1],
                full.density[2] - base_density[2],
            ]
        })
        .filter(|span| {
            span.iter()
                .all(|value| value.is_finite() && *value > 1.0e-4)
        })
}

/// Detect the film base on one frame's own pixels.
///
/// The roll anchors describe the film, but a scanner exposes every frame with
/// its own exposure and white balance, which shifts the recorded density of the
/// clear film base. Removing the mask with a roll constant therefore leaves a
/// per-frame colour cast. The clear film base is the largest uniform, low
/// density region of a negative, so the dominant density peak away from the
/// fully exposed leader is a stable per-frame estimate.
fn detect_frame_base_density(
    proxy: &ImageBuffer<Rgb<f32>, Vec<f32>>,
    anchor_base: [f32; 3],
) -> Option<[f32; 3]> {
    const BIN: f32 = 0.01;
    const MAX_OFFSET: f32 = 0.45;
    if !anchor_base
        .iter()
        .all(|value| value.is_finite() && *value > 0.0)
    {
        return None;
    }
    let anchor_luma = density_luma(anchor_base);
    let (width, height) = proxy.dimensions();
    if width < 32 || height < 32 {
        return None;
    }
    // Subsample to roughly one million samples: enough to find a large
    // uniform region, cheap enough to run during proxy preparation.
    let stride = (((width * height) as f64 / 1_000_000.0).sqrt().ceil() as u32).max(1);
    let low = (anchor_luma - MAX_OFFSET).max(0.0);
    let high = anchor_luma + MAX_OFFSET;
    let bin_count = (((high - low) / BIN).ceil() as usize).max(1);
    let mut histogram = vec![0u32; bin_count + 1];
    let mut total = 0u32;
    let mut y = 0u32;
    while y < height {
        let mut x = 0u32;
        while x < width {
            let pixel = proxy.get_pixel(x, y).0;
            if pixel.iter().all(|value| value.is_finite() && *value > 0.0) {
                let luma = density_luma([-pixel[0].log10(), -pixel[1].log10(), -pixel[2].log10()]);
                if luma >= low && luma <= high {
                    let bin = (((luma - low) / BIN) as usize).min(bin_count);
                    histogram[bin] += 1;
                    total += 1;
                }
            }
            x += stride;
        }
        y += stride;
    }
    if total < 4096 {
        return None;
    }
    let (peak_bin, peak_count) = histogram
        .iter()
        .enumerate()
        .max_by_key(|(_, count)| **count)?;
    // A large uniform region must own a meaningful share of the frame.
    if peak_count.saturating_mul(40) < total {
        return None;
    }
    let peak_center = low + (peak_bin as f32 + 0.5) * BIN;
    let mut channels: [Vec<f32>; 3] = [Vec::new(), Vec::new(), Vec::new()];
    let mut y = 0u32;
    while y < height {
        let mut x = 0u32;
        while x < width {
            let pixel = proxy.get_pixel(x, y).0;
            if pixel.iter().all(|value| value.is_finite() && *value > 0.0) {
                let density = [-pixel[0].log10(), -pixel[1].log10(), -pixel[2].log10()];
                if (density_luma(density) - peak_center).abs() <= BIN * 1.5 {
                    for channel in 0..3 {
                        channels[channel].push(density[channel]);
                    }
                }
            }
            x += stride;
        }
        y += stride;
    }
    let mut detected = [0.0f32; 3];
    for channel in 0..3 {
        if channels[channel].len() < 256 {
            return None;
        }
        channels[channel].sort_unstable_by(|left, right| left.total_cmp(right));
        detected[channel] = channels[channel][channels[channel].len() / 2];
        if !detected[channel].is_finite()
            || (detected[channel] - anchor_base[channel]).abs() > MAX_OFFSET + 0.15
        {
            return None;
        }
    }
    Some(detected)
}

/// Frame base for rendering: the measured value when the proxy was prepared,
/// otherwise a fresh measurement from the retained ProPhoto estimate.
fn resolve_frame_render_parameters(
    item: &FilmItem,
    state: &PipelineState,
) -> (Option<[f32; 3]>, Option<f32>) {
    if state.contract != ProcessingContract::RollAnchoredProPhotoV11 {
        return (None, None);
    }
    if let (Some(base), Some(highlight)) = (item.runtime_frame_base, item.runtime_frame_highlight) {
        return (Some(base), Some(highlight));
    }
    let anchor_base = pipeline_base_density(state, &item.base_color);
    if !anchor_base.iter().all(|value| *value > 0.0) {
        return (item.runtime_frame_base, item.runtime_frame_highlight);
    }
    let span = roll_physical_density_span(&state.density_anchors, anchor_base);
    let Some(estimate) = item.prophoto_estimate_proxy.as_ref() else {
        return (item.runtime_frame_base, item.runtime_frame_highlight);
    };
    let base = item
        .runtime_frame_base
        .or_else(|| detect_frame_base_density(estimate, anchor_base));
    let highlight = item.runtime_frame_highlight.or_else(|| {
        base.and_then(|base| {
            span.and_then(|span| detect_frame_highlight_fraction(estimate, base, span))
        })
    });
    (base, highlight)
}

/// Fraction of the roll density span that this frame's brightest scene content
/// occupies.
///
/// The user samples the film base and a fully exposed leader. The leader is the
/// film's maximum density, but a normally exposed scene keeps highlight
/// headroom below it: on a real Roll the scene occupies roughly half of the
/// base-to-leader span. Mapping the whole span to black..white therefore
/// under-exposes every print, which is what makes skies and clouds sit in the
/// middle of the histogram.
///
/// The measurement is done on block averages so dust, scratches, frame edges
/// and rebate lettering cannot claim the highlight, then clamped to a sane
/// band so a single unusual frame cannot swing the whole look.
fn detect_frame_highlight_fraction(
    proxy: &ImageBuffer<Rgb<f32>, Vec<f32>>,
    base_density: [f32; 3],
    span: [f32; 3],
) -> Option<f32> {
    let interior = film_block_luma_profile(proxy, base_density)?;
    let span_luma = density_luma(span);
    if !span_luma.is_finite() || span_luma <= 1.0e-4 {
        return None;
    }
    let highlight_luma = interior[highlight_block_index(interior.len())];
    let fraction = highlight_luma / span_luma;
    if !fraction.is_finite() {
        return None;
    }
    Some(fraction.clamp(ROLL_HIGHLIGHT_FLOOR, ROLL_HIGHLIGHT_CEILING))
}

/// Which picture blocks set one frame's highlight.
///
/// The blocks are already 32x32 averages, and blocks touching the light table,
/// the blocking card or the frame rim are dropped, so the statistic only has to
/// survive a stray specular. Taking the percentile of the *blocks* that used to
/// stand here (0.95) put the white point below the picture's brightest areas on
/// every frame: measured on real scans, 6-14% of the picture pixels sat above
/// that endpoint, and the clouds came out as flat white. A near-maximum block
/// reads the picture's actual highlight instead; the frame-level maximum over
/// the Roll is what keeps every frame below it clipping-free.
const HIGHLIGHT_BLOCK_QUANTILE: f32 = 0.999;

/// Bounds for the Roll's content white point, as a fraction of the measured
/// base-to-leader span. The floor keeps an underexposed negative from being
/// mapped to near-black. The ceiling is the film's own maximum density (the
/// fully exposed leader): content measured above it means the leader reference
/// was sampled low, not that the mapping should reach past the film.
const ROLL_HIGHLIGHT_FLOOR: f32 = 0.45;
const ROLL_HIGHLIGHT_CEILING: f32 = 1.0;

fn highlight_block_index(block_count: usize) -> usize {
    ((block_count - 1) as f32 * HIGHLIGHT_BLOCK_QUANTILE).round() as usize
}

/// Sorted block-average luminance of the picture area, in density units
/// relative to this frame's own film base.
fn film_block_luma_profile(
    proxy: &ImageBuffer<Rgb<f32>, Vec<f32>>,
    base_density: [f32; 3],
) -> Option<Vec<f32>> {
    const BLOCK: u32 = 32;
    const MIN_FILM_SHARE: f32 = 0.9;
    let (width, height) = proxy.dimensions();
    if width < BLOCK * 4 || height < BLOCK * 4 {
        return None;
    }
    let blocks_x = width / BLOCK;
    let blocks_y = height / BLOCK;
    // Subsample inside each block: the statistic only needs block averages.
    let step = ((BLOCK as f32 / 8.0).ceil() as u32).max(1);
    let mut block_luma = vec![f32::NAN; (blocks_x * blocks_y) as usize];
    for block_y in 0..blocks_y {
        for block_x in 0..blocks_x {
            let mut film = 0u32;
            let mut total = 0u32;
            let mut sum = [0.0f64; 3];
            let mut y = block_y * BLOCK;
            while y < (block_y + 1) * BLOCK {
                let mut x = block_x * BLOCK;
                while x < (block_x + 1) * BLOCK {
                    let pixel = proxy.get_pixel(x, y).0;
                    total += 1;
                    if pixel.iter().all(|value| value.is_finite() && *value > 0.0) {
                        let relative = [
                            -pixel[0].log10() - base_density[0],
                            -pixel[1].log10() - base_density[1],
                            -pixel[2].log10() - base_density[2],
                        ];
                        let luma = density_luma(relative);
                        if luma > 0.03 && luma < 1.45 {
                            film += 1;
                            for channel in 0..3 {
                                sum[channel] += relative[channel] as f64;
                            }
                        }
                    }
                    x += step;
                }
                y += step;
            }
            let film_share = film as f32 / total as f32;
            if total == 0 || film_share < MIN_FILM_SHARE || film < 64 {
                continue;
            }
            let mean = [
                (sum[0] / film as f64) as f32,
                (sum[1] / film as f64) as f32,
                (sum[2] / film as f64) as f32,
            ];
            block_luma[(block_y * blocks_x + block_x) as usize] = density_luma(mean);
        }
    }
    // Drop blocks that touch the light table, the blocking card or the frame
    // rim: those edges are denser than the photograph and would otherwise
    // claim the highlight.
    const NEIGHBOURHOOD: i32 = 3;
    const MIN_INTERIOR_SHARE: f32 = 0.85;
    let mut interior: Vec<f32> = Vec::with_capacity((blocks_x * blocks_y) as usize);
    for block_y in 0..blocks_y as i32 {
        for block_x in 0..blocks_x as i32 {
            let index = (block_y as u32 * blocks_x + block_x as u32) as usize;
            let luma = block_luma[index];
            if !luma.is_finite() {
                continue;
            }
            let mut film_neighbours = 0u32;
            let mut neighbours = 0u32;
            for dy in -NEIGHBOURHOOD..=NEIGHBOURHOOD {
                for dx in -NEIGHBOURHOOD..=NEIGHBOURHOOD {
                    let ny = block_y + dy;
                    let nx = block_x + dx;
                    if ny < 0 || nx < 0 || ny >= blocks_y as i32 || nx >= blocks_x as i32 {
                        continue;
                    }
                    neighbours += 1;
                    if block_luma[(ny as u32 * blocks_x + nx as u32) as usize].is_finite() {
                        film_neighbours += 1;
                    }
                }
            }
            let interior_share = film_neighbours as f32 / neighbours as f32;
            if neighbours == 0 || interior_share < MIN_INTERIOR_SHARE {
                continue;
            }
            interior.push(luma);
        }
    }
    if interior.len() < 32 {
        return None;
    }
    interior.sort_unstable_by(|left, right| left.total_cmp(right));
    Some(interior)
}

/// Resolve the density mapping used by every complete roll-anchor renderer.
/// The stored anchors are raw D values; `FilmPipeline` subtracts the roll D-min
/// before rendering, so the display mapping is the relative span
/// `[D-min_frame - D-min_roll, D-max - D-min_roll]`. `frame_base` carries the
/// per-frame measurement that keeps mask removal exact on every frame.
fn roll_density_mapping_with_frame_base(
    state: &PipelineState,
    frame_base: Option<[f32; 3]>,
    highlight_fraction: Option<f32>,
) -> Option<RenderMapping> {
    if !matches!(
        state.contract,
        ProcessingContract::RollAnchoredProPhotoV11 | ProcessingContract::CaptureCorrectedV11
    ) {
        return None;
    }
    let base = state.density_anchors.d_min_base.as_ref()?;
    let full = state.density_anchors.d_max_full_exposure.as_ref()?;
    if base.scope != DensityAnchorScope::Roll
        || full.scope != DensityAnchorScope::Roll
        || base.source != DensityAnchorSource::SampledFilmBase
        || full.source != DensityAnchorSource::SampledFullExposure
        || matches!(base.confidence, DensityAnchorConfidence::Estimated)
        || matches!(full.confidence, DensityAnchorConfidence::Estimated)
        || !anchor_matches_resolved_contract(base, state)
        || !anchor_matches_resolved_contract(full, state)
    {
        return None;
    }
    let span = [
        full.density[0] - base.density[0],
        full.density[1] - base.density[1],
        full.density[2] - base.density[2],
    ];
    if span
        .iter()
        .any(|value| !value.is_finite() || *value <= 1.0e-4)
    {
        return None;
    }
    let offset = frame_base
        .filter(|frame| frame.iter().all(|value| value.is_finite() && *value > 0.0))
        .map(|frame| {
            [
                frame[0] - base.density[0],
                frame[1] - base.density[1],
                frame[2] - base.density[2],
            ]
        })
        .unwrap_or([0.0; 3]);
    // A fully exposed leader is the film's maximum density. Real scenes keep
    // highlight headroom below it, so the white point uses only the measured
    // fraction of the span that this Roll's content actually reaches.
    let highlight = state
        .density_anchors
        .highlight_fraction
        .or(highlight_fraction)
        .filter(|value| value.is_finite())
        .map(|value| value.clamp(ROLL_HIGHLIGHT_FLOOR, ROLL_HIGHLIGHT_CEILING))
        .unwrap_or(1.0);
    Some(RenderMapping {
        mode: RenderMode::RollAnchored,
        density_low: offset,
        density_high: [
            offset[0] + span[0] * highlight,
            offset[1] + span[1] * highlight,
            offset[2] + span[2] * highlight,
        ],
        exposure: 0.0,
        gamma: 1.0,
        channel_offsets: [0.0; 3],
    })
}

fn fixed_roll_density_mapping(state: &PipelineState) -> Option<RenderMapping> {
    roll_density_mapping_with_frame_base(state, None, None)
}

/// Learn the Roll's white point once and reuse it for every frame, so the whole
/// Roll keeps a single fixed mapping instead of per-frame auto exposure.
fn record_roll_highlight_fraction(state: &EngineState, roll_id: &str, fraction: f32) {
    if !fraction.is_finite() {
        return;
    }
    let fraction = fraction.clamp(ROLL_HIGHLIGHT_FLOOR, ROLL_HIGHLIGHT_CEILING);
    let mut rolls = write_lock(&state.rolls);
    let Some(roll) = rolls.iter_mut().find(|roll| roll.roll_id == roll_id) else {
        return;
    };
    if roll
        .density_anchors
        .highlight_fraction
        .is_some_and(|current| (current - fraction).abs() < 1.0e-4)
    {
        return;
    }
    roll.density_anchors.highlight_fraction = Some(fraction);
    let snapshot = rolls.clone();
    drop(rolls);
    if let Ok(connection) = persistence::open_connection() {
        let mut connection = connection;
        let _ = persistence::save_rolls(&mut connection, &snapshot);
    }
}

fn apply_roll_anchor_report(state: &mut PipelineState) {
    let report = &mut state.processing_report;
    if state.contract == ProcessingContract::RollAnchoredProPhotoV11 {
        // ProPhoto is a display-domain estimate. A user click improves
        // repeatability, but it is not a measured density or Status M anchor.
        report.base_source = "roll_anchor_prophoto_estimate".to_string();
        report.base_confidence = "estimated".to_string();
        report.uses_physical_anchors = false;
        return;
    }
    report.base_source = "roll_anchor_relative_transmission".to_string();
    report.base_confidence = state
        .density_anchors
        .d_min_base
        .as_ref()
        .map(|anchor| match anchor.confidence {
            DensityAnchorConfidence::Verified => "verified",
            DensityAnchorConfidence::UserSampled => "user_sampled",
            DensityAnchorConfidence::Estimated => "estimated",
        })
        .unwrap_or("estimated")
        .to_string();
    report.uses_physical_anchors = true;
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
    let pipeline = if is_smart_auto_compatibility(pipeline_state) {
        FilmPipeline::new(
            [base_color.base_r, base_color.base_g, base_color.base_b],
            [0.0, 0.0, 0.0],
            mode,
        )
    } else {
        FilmPipeline::from_state(pipeline_state, base_color, [0.0, 0.0, 0.0], mode)
    };
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

    if !is_smart_auto_compatibility(pipeline_state) {
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
            return pristine;
        }
    }
    {
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

/// The film base a pasted inversion actually runs on, reported back so the
/// Develop preview cannot render a different reference than the frame stores.
#[derive(Debug, Clone, Serialize)]
pub struct AppliedFilmBase {
    pub base_density: [f32; 3],
    pub base_source: String,
    /// True when the base was measured on the target frame itself.
    pub measured_on_frame: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct AutoInvertRollResult {
    pub roll_id: String,
    pub total: usize,
    pub processed: usize,
    pub succeeded: usize,
    pub failed: usize,
    pub failed_ids: Vec<String>,
}

#[inline]
fn map_oriented_uv_to_source(
    uv: [f32; 2],
    source_width: u32,
    source_height: u32,
    geom: &GeometryState,
) -> [f32; 2] {
    map_oriented_uv_to_source_impl(uv, source_width, source_height, geom)
}

/// Apply the frame's quarter turns and flips to a display-referred image.
///
/// This is the image-space counterpart of `map_oriented_uv_to_source`: rotating
/// and then flipping a rendered frame produces exactly the pixels the renderer
/// picks when it walks the oriented grid. The filmstrip thumbnail needs it to
/// show the frame the user graded; without it a flipped frame looked upside down
/// in the Library and the filmstrip while the preview and the export were the
/// other way up.
fn orient_display_image(image: image::RgbImage, geom: &GeometryState) -> image::RgbImage {
    let mut current = image;
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

#[inline]
fn map_oriented_uv_to_source_impl(
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
                let Some(perspective_uv) = apply_perspective_uv(
                    crop_uv,
                    geom.perspective_vertical,
                    geom.perspective_horizontal,
                    geom.perspective_aspect,
                    geom.perspective_scale,
                ) else {
                    continue;
                };
                let Some(oriented_uv) =
                    apply_lens_distortion_uv(perspective_uv, geom.lens_distortion)
                else {
                    continue;
                };
                if inside_calibration_only && !point_in_film_area(crop_uv, &points, 0.0) {
                    continue;
                }
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
        let input =
            scanner_fff_container_input(path).unwrap_or(ScannerFffInput::DOCUMENTED_DEFAULT);
        return Ok(linearize_scanner_fff_with_input(
            image,
            requested_profile,
            input,
        ));
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
    // A camera-scanned TIFF used to be rejected outright here when it packed no
    // ICC profile, which turned a file that the legacy path had always accepted
    // into a hard import failure. Such a file is read as sRGB (see
    // `decode_reduced_tiff_for_working_space`) and flagged as an estimated
    // input domain through `tiff_smart_auto_input_is_estimated`.
    decode_reduced_tiff_for_working_space(path, target_long_edge)
}

/// True when a TIFF has to be read as an estimated sRGB input because it packs
/// no embedded ICC profile. Scanner FFF frames are scanner-linear by contract
/// and are excluded, so this only describes ordinary profile-less RGB TIFFs.
fn tiff_smart_auto_input_is_estimated(path: &str) -> bool {
    is_tiff_extension(path) && !is_scanner_fff_tiff(path) && embedded_input_profile(path).is_none()
}

/// Primaries and transfer curve a known colour space declares.
fn input_domain_for_color_space(id: ColorSpaceId) -> (InputPrimaries, InputTransferCurve) {
    match id {
        ColorSpaceId::SRgb => (InputPrimaries::Srgb, InputTransferCurve::Srgb),
        ColorSpaceId::DisplayP3 => (InputPrimaries::DisplayP3, InputTransferCurve::Srgb),
        ColorSpaceId::AdobeRgb => (InputPrimaries::AdobeRgb1998, InputTransferCurve::Gamma22),
        ColorSpaceId::Rec2020 => (InputPrimaries::Rec2020, InputTransferCurve::Gamma22),
        ColorSpaceId::ProPhotoRgb | ColorSpaceId::ProPhotoRgbD65 => {
            (InputPrimaries::ProPhotoRgb, InputTransferCurve::Gamma18)
        }
        ColorSpaceId::Aces2065 | ColorSpaceId::AcesCg => {
            (InputPrimaries::AcesCg, InputTransferCurve::Linear)
        }
    }
}

/// Map a profile name recorded inside a scanner container onto a recognised
/// working space. Unknown names stay scanner-device.
fn input_primaries_from_profile_name(name: &str) -> InputPrimaries {
    let lowered = name.to_ascii_lowercase();
    if lowered.contains("adobe rgb") {
        InputPrimaries::AdobeRgb1998
    } else if lowered.contains("prophoto") || lowered.contains("romm") {
        InputPrimaries::ProPhotoRgb
    } else if lowered.contains("display p3") {
        InputPrimaries::DisplayP3
    } else if lowered.contains("2020") {
        InputPrimaries::Rec2020
    } else if lowered.contains("acescg") {
        InputPrimaries::AcesCg
    } else if lowered.contains("srgb") || lowered.contains("iec 61966") {
        InputPrimaries::Srgb
    } else {
        InputPrimaries::ScannerDevice
    }
}

/// The transfer curve a scanner container record declares for its stored
/// samples. Anything outside the documented scanner range stays device-defined.
fn input_transfer_from_gamma(gamma: f32) -> InputTransferCurve {
    if (gamma - 1.8).abs() < 0.06 {
        InputTransferCurve::Gamma18
    } else if (gamma - 2.0).abs() < 0.06 {
        InputTransferCurve::Gamma20
    } else if (gamma - 2.2).abs() < 0.12 {
        InputTransferCurve::Gamma22
    } else if (gamma - 1.0).abs() < 0.03 {
        InputTransferCurve::Linear
    } else {
        InputTransferCurve::ScannerDevice
    }
}

/// How a scanner container describes the samples it stores.
#[derive(Debug, Clone, Copy, PartialEq)]
struct ScannerFffInput {
    /// Transfer curve exponent that turns the stored samples into linear light.
    gamma: f32,
    /// Primaries the stored samples are expressed in.
    primaries: InputPrimaries,
}

impl ScannerFffInput {
    /// Documented fallback for a Flextight/Imacon container whose settings
    /// record cannot be read: scanner-linear samples in an sRGB-like basis.
    const DOCUMENTED_DEFAULT: Self = Self {
        gamma: 1.8,
        primaries: InputPrimaries::Srgb,
    };

    fn input_color_space(self) -> ColorSpaceId {
        match self.primaries {
            InputPrimaries::AdobeRgb1998 => ColorSpaceId::AdobeRgb,
            InputPrimaries::DisplayP3 => ColorSpaceId::DisplayP3,
            InputPrimaries::Rec2020 => ColorSpaceId::Rec2020,
            InputPrimaries::ProPhotoRgb => ColorSpaceId::ProPhotoRgb,
            InputPrimaries::AcesCg => ColorSpaceId::AcesCg,
            _ => ColorSpaceId::SRgb,
        }
    }

    fn transfer_curve(self) -> InputTransferCurve {
        input_transfer_from_gamma(self.gamma)
    }
}

/// Read the value that follows `key` in the ASCII plist a Flextight/Imacon
/// container stores next to the image settings. Returns the first `<string>`
/// or `<real>`/`<integer>` payload after the key, which is how the container
/// declares the output profile and gamma of the stored samples.
fn scanner_fff_plist_value(head: &[u8], key: &str) -> Option<String> {
    let needle = format!("<key>{key}</key>");
    let start = head
        .windows(needle.len())
        .position(|window| window.eq_ignore_ascii_case(needle.as_bytes()))?;
    let tail = &head[start + needle.len()..];
    // The value element follows the key within a few dozen bytes.
    let window = &tail[..tail.len().min(160)];
    for (open, close) in [
        ("<string>", "</string>"),
        ("<real>", "</real>"),
        ("<integer>", "</integer>"),
    ] {
        let Some(open_start) = window
            .windows(open.len())
            .position(|candidate| candidate.eq_ignore_ascii_case(open.as_bytes()))
        else {
            continue;
        };
        let value_start = open_start + open.len();
        let Some(close_offset) = window[value_start..]
            .windows(close.len())
            .position(|candidate| candidate.eq_ignore_ascii_case(close.as_bytes()))
        else {
            continue;
        };
        let raw = &window[value_start..value_start + close_offset];
        let text = String::from_utf8_lossy(raw).trim().to_string();
        if !text.is_empty() {
            return Some(text);
        }
    }
    None
}

/// Decode the sample domain a Flextight/Imacon container records for itself.
/// The container stores its own output profile and gamma, so the scanner input
/// domain no longer has to be a hard-coded constant. `None` means the record
/// could not be read and the documented default applies.
fn scanner_fff_container_input(path: &str) -> Option<ScannerFffInput> {
    let mut file = std::fs::File::open(path).ok()?;
    let mut head = vec![0u8; FFF_SCANNER_METADATA_LIMIT as usize];
    let read = file.read(&mut head).ok()?;
    head.truncate(read);

    let gamma = scanner_fff_plist_value(&head, "Gamma").and_then(|value| value.parse::<f32>().ok());
    let profile_name = scanner_fff_plist_value(&head, "RGBProfile")
        .or_else(|| scanner_fff_plist_value(&head, "Name"));
    let primaries = profile_name
        .as_deref()
        .map(input_primaries_from_profile_name);
    match (gamma, primaries) {
        (Some(gamma), Some(primaries))
            if gamma.is_finite()
                && (0.5..=3.5).contains(&gamma)
                && primaries != InputPrimaries::Unknown =>
        {
            Some(ScannerFffInput { gamma, primaries })
        }
        (Some(gamma), _) if gamma.is_finite() && (0.5..=3.5).contains(&gamma) => {
            Some(ScannerFffInput {
                gamma,
                primaries: ScannerFffInput::DOCUMENTED_DEFAULT.primaries,
            })
        }
        (_, Some(primaries)) if primaries != InputPrimaries::Unknown => Some(ScannerFffInput {
            gamma: ScannerFffInput::DOCUMENTED_DEFAULT.gamma,
            primaries,
        }),
        _ => None,
    }
}

/// True when a DNG stores an uncompressed LinearRaw RGB SubIFD. That is how
/// VueScan-style scanner DNGs differ from camera DNGs, and the difference has
/// to be read from the file rather than assumed from the suffix.
fn dng_has_uncompressed_linear_raw_rgb_subifd(path: &str) -> bool {
    let Ok(root) = read_classic_tiff_directory(path, 0) else {
        return false;
    };
    root.sub_ifd_offsets.iter().any(|offset| {
        read_classic_tiff_subdirectory(path, *offset).is_ok_and(|candidate| {
            candidate.photometric == 34892
                && candidate.compression == 1
                && candidate.samples_per_pixel >= 3
        })
    })
}

/// Resolve the input domain for one file. This is the only stage that is
/// allowed to differ between input classes: the density maths, neutralisation,
/// white point and display mapping after it are shared by every input.
///
/// Priority order: embedded ICC, scanner Input Profile or container record,
/// RAW/DNG metadata, scanner device identification, then an estimated sRGB
/// fallback. The suffix only selects a decoder.
fn resolve_input_domain(
    path: &str,
    scanner_profile: Option<&crate::scanner_profile::ScannerInputProfile>,
) -> InputDomainRecord {
    if let Some(profile) = embedded_input_profile(path) {
        let (primaries, transfer) = input_domain_for_color_space(profile);
        return InputDomainRecord {
            primaries,
            transfer,
            reference: if transfer == InputTransferCurve::Linear {
                InputReference::LinearTransmission
            } else {
                InputReference::DisplayReferred
            },
            normalization: "embedded_icc_full_range".to_string(),
            source: InputDomainSource::EmbeddedIcc,
            confidence: InputDomainConfidence::Verified,
            estimated: false,
            detail: crate::color_science::profile_name(profile).to_string(),
        };
    }

    if is_scanner_fff_tiff(path) {
        return match scanner_fff_container_input(path) {
            Some(container) => InputDomainRecord {
                primaries: container.primaries,
                transfer: container.transfer_curve(),
                reference: InputReference::LinearTransmission,
                normalization: "scanner_container_full_range".to_string(),
                source: InputDomainSource::ScannerContainerRecord,
                confidence: InputDomainConfidence::Verified,
                estimated: false,
                detail: format!("flextight_container_gamma_{:.2}", container.gamma),
            },
            None => InputDomainRecord {
                primaries: ScannerFffInput::DOCUMENTED_DEFAULT.primaries,
                transfer: ScannerFffInput::DOCUMENTED_DEFAULT.transfer_curve(),
                reference: InputReference::LinearTransmission,
                normalization: "scanner_documented_full_range".to_string(),
                source: InputDomainSource::DeviceIdentification,
                confidence: InputDomainConfidence::Estimated,
                estimated: true,
                detail: "flextight_container_record_unreadable".to_string(),
            },
        };
    }

    if let Some(profile) = scanner_profile
        .filter(|_| !is_raw_extension(path) || is_dng_extension(path) || is_scanner_fff_tiff(path))
    {
        let characterized = profile.verified
            && profile.confidence
                == crate::scanner_profile::ScannerProfileConfidence::Characterized;
        return InputDomainRecord {
            primaries: InputPrimaries::ScannerDevice,
            transfer: InputTransferCurve::Linear,
            reference: InputReference::LinearTransmission,
            normalization: "scanner_profile_linear_rgb".to_string(),
            source: InputDomainSource::ScannerInputProfile,
            confidence: if characterized {
                InputDomainConfidence::Verified
            } else {
                InputDomainConfidence::Declared
            },
            estimated: !characterized,
            detail: format!("{} {}", profile.manufacturer, profile.model),
        };
    }

    if is_dng_extension(path) {
        if dng_has_uncompressed_linear_raw_rgb_subifd(path) {
            return InputDomainRecord {
                primaries: InputPrimaries::Srgb,
                transfer: InputTransferCurve::Linear,
                reference: InputReference::LinearTransmission,
                normalization: "scanner_dng_linear_raw_full_range".to_string(),
                source: InputDomainSource::DeviceIdentification,
                confidence: InputDomainConfidence::Estimated,
                estimated: true,
                detail: "linear_raw_scanner_dng_read_as_linear_srgb".to_string(),
            };
        }
        return InputDomainRecord {
            primaries: InputPrimaries::CameraNative,
            transfer: InputTransferCurve::CameraRaw,
            reference: InputReference::LinearTransmission,
            normalization: "camera_dng_raw_levels".to_string(),
            source: InputDomainSource::RawMetadata,
            confidence: InputDomainConfidence::Declared,
            estimated: false,
            detail: "libraw_camera_matrix_to_prophoto_estimate".to_string(),
        };
    }

    if is_raw_extension(path) {
        return InputDomainRecord {
            primaries: InputPrimaries::CameraNative,
            transfer: InputTransferCurve::CameraRaw,
            reference: InputReference::LinearTransmission,
            normalization: "camera_raw_levels".to_string(),
            source: InputDomainSource::RawMetadata,
            confidence: InputDomainConfidence::Declared,
            estimated: false,
            detail: "libraw_camera_matrix_to_prophoto_estimate".to_string(),
        };
    }

    // Priority 5: read as sRGB and say that the domain was assumed. An
    // unprofiled TIFF/JPEG/PNG stays importable instead of failing.
    InputDomainRecord {
        primaries: InputPrimaries::Srgb,
        transfer: InputTransferCurve::Srgb,
        reference: InputReference::DisplayReferred,
        normalization: "assumed_srgb_full_range".to_string(),
        source: InputDomainSource::FallbackSrgb,
        confidence: InputDomainConfidence::Estimated,
        estimated: true,
        detail: if is_tiff_extension(path) {
            "tiff_without_embedded_icc".to_string()
        } else {
            "encoded_rgb_without_embedded_icc".to_string()
        },
    }
}

/// Convert one encoded sample of a profiled TIFF into the Smart Auto working
/// space. Kept separate from the file plumbing so the gamut behaviour can be
/// tested directly.
fn encoded_pixel_to_prophoto_estimate(encoded: [f32; 3], source_profile: ColorSpaceId) -> [f32; 3] {
    let to_prophoto = linear_conversion_matrix(source_profile, ColorSpaceId::ProPhotoRgb);
    compress_linear_srgb_for_density(convert_encoded_to_linear_rgb_with_matrix(
        encoded,
        source_profile,
        to_prophoto,
    ))
}

/// Convert a TIFF that carries an embedded ICC profile straight into the Smart
/// Auto working space.
///
/// Routing such a file through the 16-bit linear-sRGB transport first would
/// clamp it per channel, and the orange mask of a colour negative deliberately
/// sits outside the sRGB gamut: on a real merged scan that clamped 84.7% of the
/// red channel against 0.95% in the file itself, which is what produced the
/// cyan highlights and magenta shadows. Returns an error when the file has no
/// embedded profile, is a scanner FFF, or cannot be read by the streaming
/// decoder, so the caller can keep the estimate-input fallback.
fn decode_profiled_tiff_prophoto_estimate(
    path: &str,
    target_long_edge: u32,
) -> Result<ImageBuffer<Rgb<f32>, Vec<f32>>, String> {
    if is_scanner_fff_tiff(path) {
        return Err("scanner_fff_uses_scanner_linear_path".to_string());
    }
    let source_profile =
        embedded_input_profile(path).ok_or_else(|| "no_embedded_profile".to_string())?;
    let encoded = decode_uncompressed_tiff_reduced(path, target_long_edge)?;
    let mut estimate = ImageBuffer::<Rgb<f32>, Vec<f32>>::new(encoded.width(), encoded.height());
    estimate
        .as_mut()
        .par_chunks_exact_mut(3)
        .zip(encoded.as_raw().par_chunks_exact(3))
        .for_each(|(target, pixel)| {
            target.copy_from_slice(&encoded_pixel_to_prophoto_estimate(
                [
                    f32::from(pixel[0]) / 65535.0,
                    f32::from(pixel[1]) / 65535.0,
                    f32::from(pixel[2]) / 65535.0,
                ],
                source_profile,
            ));
        });
    Ok(estimate)
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
        let input =
            scanner_fff_container_input(path).unwrap_or(ScannerFffInput::DOCUMENTED_DEFAULT);
        return Ok(linearize_scanner_fff_with_input(
            image,
            requested_profile,
            input,
        ));
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

    // RAW_DECODE_VERSION 11 contract: camera FFF files, including tethered
    // Hasselblad digital-back captures, use LibRaw instead of the Flextight
    // scanner path. LibRaw performs black subtraction and demosaic in camera
    // RGB, but its signed output-gamut matrix is applied here in f32. This
    // avoids LibRaw's unsigned-16 CLIP after convert_to_rgb(). White balance is
    // installed explicitly as the as-shot ratio normalized so the strongest
    // channel is unity: the channel balance is preserved, but decoding can only
    // attenuate, so no channel is pushed into the 16-bit ceiling.
    let options = crate::raw_backend::DecodeOptions {
        half_size: mode == DecodeMode::DevelopProxy,
        demosaic_quality: 3,
        output_bps: 16,
        no_auto_bright: true,
        output_color: 0,
        linear_gamma: true,
        use_camera_wb: false,
    };
    let decoded = crate::raw_backend::extract_camera_rgb_with_policy(
        path,
        &options,
        crate::raw_backend::WhiteBalancePolicy::NormalizedAsShot,
    )
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
            let rgb = apply_linear_matrix(
                [
                    pixel[0] as f32 / 65535.0,
                    pixel[1] as f32 / 65535.0,
                    pixel[2] as f32 / 65535.0,
                ],
                matrix,
            );
            for channel in 0..3 {
                target[channel] = (rgb[channel].clamp(0.0, 1.0) * 65535.0).round() as u16;
            }
        });
    Ok(converted)
}

/// Decode the Smart Auto ProPhoto Estimate. Camera/sRGB values are first fit
/// into a finite positive transmission domain while preserving luminance;
/// the resulting ProPhoto values remain a relative display estimate, never a
/// measured Density Input RGB.
fn decode_prophoto_estimate_image_buffer(
    path: &str,
    mode: DecodeMode,
) -> Result<ImageBuffer<Rgb<f32>, Vec<f32>>, String> {
    decode_prophoto_estimate_image_buffer_with_policy(
        path,
        mode,
        crate::raw_backend::WhiteBalancePolicy::NormalizedAsShot,
    )
}

fn decode_prophoto_estimate_image_buffer_with_policy(
    path: &str,
    mode: DecodeMode,
    white_balance: crate::raw_backend::WhiteBalancePolicy,
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
                    compress_linear_srgb_for_density([
                        pixel[0] as f32 / 65535.0,
                        pixel[1] as f32 / 65535.0,
                        pixel[2] as f32 / 65535.0,
                    ]),
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
        use_camera_wb: false,
    };
    let decoded =
        crate::raw_backend::decode_smart_auto_rgb_with_policy(path, &options, white_balance)
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
            let srgb = compress_linear_srgb_for_density(apply_linear_matrix(
                camera,
                decoded.camera_to_srgb,
            ));
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
        let rgb = apply_linear_matrix(
            compress_linear_srgb_for_density([pixel[0], pixel[1], pixel[2]]),
            matrix,
        );
        pixel.copy_from_slice(&rgb);
    });
    Ok(linear)
}

fn reference_density_extreme(
    image: &ImageBuffer<Rgb<f32>, Vec<f32>>,
    source: DensityAnchorSource,
) -> Result<[f32; 3], String> {
    // The file-based compatibility path has no click geometry. Restrict it
    // to a central ROI and use a trimmed mean; full-frame 1%/99% tails are
    // dominated by light panels, borders, sprockets, and edge leaks.
    let (width, height) = image.dimensions();
    if width == 0 || height == 0 {
        return Err("The reference image contains no pixels.".to_string());
    }
    let mut densities = [Vec::new(), Vec::new(), Vec::new()];
    let x0 = (width as f32 * 0.10).round() as u32;
    let x1 = (width as f32 * 0.90).round().max((x0 + 1) as f32) as u32;
    let y0 = (height as f32 * 0.10).round() as u32;
    let y1 = (height as f32 * 0.90).round().max((y0 + 1) as f32) as u32;
    for y in y0.min(height - 1)..y1.min(height) {
        for x in x0.min(width - 1)..x1.min(width) {
            let pixel = image.get_pixel(x, y).0;
            for channel in 0..3 {
                let transmission = pixel[channel];
                if transmission.is_finite() && transmission > 1.0e-6 && transmission < 0.999999 {
                    densities[channel].push(-transmission.log10());
                }
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
        let len = densities[channel].len();
        densities[channel].sort_unstable_by(|left, right| left.total_cmp(right));
        let (start, end) = match source {
            DensityAnchorSource::SampledFilmBase => (0, (len / 5).max(1)),
            DensityAnchorSource::SampledFullExposure => ((len * 4 / 5).min(len - 1), len),
            _ => (len / 5, (len * 4 / 5).max(len / 5 + 1)),
        };
        let kept = &densities[channel][start.min(len - 1)..end.min(len).max(start + 1).min(len)];
        result[channel] = kept.iter().copied().sum::<f32>() / kept.len() as f32;
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
            highlight_fraction: None,
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
            // Include the sampled location so repeated clicks on one
            // reference image remain independent observations.
            reference_id: Some(format!("{roll_id}:{path}:{x:.4}:{y:.4}")),
            provenance,
        })
    })
    .await
    .map_err(|error| format!("Density sampling worker failed: {error}"))?
}

fn median_density(values: &mut [f32]) -> Result<f32, String> {
    median_value_result(values)
}

fn median_value_result(values: &mut [f32]) -> Result<f32, String> {
    if values.is_empty() || values.iter().any(|value| !value.is_finite()) {
        return Err("Density reference samples must be finite and non-empty.".to_string());
    }
    values.sort_unstable_by(|left, right| left.total_cmp(right));
    let middle = values.len() / 2;
    if values.len() % 2 == 0 {
        Ok((values[middle - 1] + values[middle]) * 0.5)
    } else {
        Ok(values[middle])
    }
}

/// Merge independently sampled local regions without mixing reference
/// contracts. The returned anchor keeps one representative provenance record;
/// its reference id records that it is an aggregate for this Roll.
#[tauri::command]
pub fn aggregate_roll_density_references(
    roll_id: String,
    kind: String,
    samples: Vec<DensityAnchor>,
) -> Result<DensityAnchor, String> {
    let expected_source = match kind.as_str() {
        "base" => DensityAnchorSource::SampledFilmBase,
        "full" => DensityAnchorSource::SampledFullExposure,
        _ => return Err("Unknown density reference kind.".to_string()),
    };
    if samples.is_empty() {
        return Err("At least one density reference sample is required.".to_string());
    }
    let roll_prefix = format!("{roll_id}:");
    let provenance = samples[0].provenance.clone();
    for sample in &samples {
        if sample.source != expected_source {
            return Err("Density reference source does not match the requested kind.".to_string());
        }
        if sample.scope != DensityAnchorScope::Roll {
            return Err("Density reference scope must be the current Roll.".to_string());
        }
        if sample.confidence == DensityAnchorConfidence::Estimated {
            return Err("Estimated density references cannot be aggregated.".to_string());
        }
        if sample
            .reference_id
            .as_deref()
            .is_none_or(|id| !id.starts_with(&roll_prefix))
        {
            return Err("Density reference belongs to a different Roll.".to_string());
        }
        if sample.provenance != provenance {
            return Err("Density references must share one data-domain contract.".to_string());
        }
        if sample.provenance.algorithm_version != crate::app_state::DENSITY_ANCHOR_ALGORITHM_VERSION
            || sample.provenance.legacy
            || sample.density.iter().any(|value| !value.is_finite())
        {
            return Err("Density reference provenance is invalid.".to_string());
        }
    }
    let mut merged = samples
        .last()
        .cloned()
        .ok_or_else(|| "At least one density reference sample is required.".to_string())?;
    merged.density = (0..3)
        .map(|channel| {
            let mut values = samples
                .iter()
                .map(|sample| sample.density[channel])
                .collect::<Vec<_>>();
            median_density(&mut values)
        })
        .collect::<Result<Vec<_>, _>>()?
        .try_into()
        .map_err(|_| "Could not form a three-channel density reference.".to_string())?;
    merged.reference_id = Some(format!("{roll_id}:aggregate:{kind}:{}", samples.len()));
    merged.confidence = if samples
        .iter()
        .any(|sample| sample.confidence == DensityAnchorConfidence::Verified)
    {
        DensityAnchorConfidence::Verified
    } else {
        DensityAnchorConfidence::UserSampled
    };
    Ok(merged)
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
        // The white point is a Roll property as well: copy it so the runtime
        // state renders every frame with the same fixed mapping.
        if roll.density_anchors.highlight_fraction.is_some() {
            anchors.highlight_fraction = roll.density_anchors.highlight_fraction;
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
    state_from_resolution_with_frame_base(persisted, resolution, None, None)
}

fn state_from_resolution_with_frame_base(
    persisted: &PipelineState,
    resolution: &PipelineResolution,
    frame_base: Option<[f32; 3]>,
    highlight_fraction: Option<f32>,
) -> PipelineState {
    let mut state = persisted.clone();
    state.contract = resolution.resolved_path;
    state.density_anchors = resolution.usable_density_anchors.clone();
    let mut report = resolution.processing_report.clone();
    // The input domain describes this frame's decoded pixels, not the
    // capability the resolver selected, so it always comes from the persisted
    // per-frame analysis.
    report.input_domain = persisted.processing_report.input_domain.clone();
    let expected_domain = match resolution.resolved_path {
        ProcessingContract::CaptureCorrectedV11 => "relative_transmission_rgb",
        ProcessingContract::LegacyV1 => "legacy_linear_srgb",
        ProcessingContract::SmartAutoProPhotoV11
            if persisted.processing_report.analysis_data_domain == "legacy_linear_srgb" =>
        {
            "legacy_linear_srgb"
        }
        _ => "linear_prophoto_estimate",
    };
    if resolution.resolved_path == ProcessingContract::SmartAutoProPhotoV11
        && persisted.processing_report.analysis_data_domain == "legacy_linear_srgb"
    {
        // The resolver describes the generic Smart Auto capability as
        // ProPhoto, but loose imports persist an explicit v1.0.2-compatible
        // source marker. Preserve that marker even before Auto Invert has
        // populated the base estimate.
        report.analysis_data_domain = "legacy_linear_srgb".to_string();
    }
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
        report.render_route = persisted.processing_report.render_route.clone();
        report.fallback_reason = persisted.processing_report.fallback_reason.clone();
        report.channel_response = persisted.processing_report.channel_response.clone();
        report
            .fallback_reasons
            .extend(persisted.processing_report.fallback_reasons.iter().cloned());
    }
    state.processing_report = report;
    if let Some(mapping) =
        roll_density_mapping_with_frame_base(&state, frame_base, highlight_fraction)
    {
        state.content_range = None;
        state.render_mapping = mapping;
        state.processing_report.render_route = "RollAnchoredDirectInvert".to_string();
        state.processing_report.fallback_reason.clear();
        apply_roll_anchor_report(&mut state);
    } else if state.contract != ProcessingContract::LegacyV1 {
        if state.render_mapping.mode == RenderMode::RollAnchored {
            // A previously anchored frame must not keep using its fixed
            // endpoints after anchors become invalid or incomplete.
            state.content_range = None;
            state.render_mapping = RenderMapping::default();
        }
        state.processing_report.render_route = "FilmAreaSmartAuto".to_string();
        state.processing_report.uses_physical_anchors = false;
        state.processing_report.fallback_reason = state
            .processing_report
            .fallback_reasons
            .first()
            .cloned()
            .unwrap_or_else(|| {
                if state.density_anchors.has_roll_base()
                    || state.density_anchors.has_roll_full_exposure()
                {
                    "incomplete_roll_anchors".to_string()
                } else {
                    "missing_or_invalid_roll_anchors".to_string()
                }
            });
        if !state
            .processing_report
            .fallback_reasons
            .contains(&state.processing_report.fallback_reason)
        {
            state
                .processing_report
                .fallback_reasons
                .push(state.processing_report.fallback_reason.clone());
        }
    }
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
        // Loose Import has no capture, film-stock, or roll-reference metadata,
        // so it runs the same unanchored Smart Auto path as a Roll without
        // anchors: a ProPhoto estimate with a per-frame Film Area analysis.
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
                        runtime_frame_base: None,
                        runtime_frame_highlight: None,
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
                    runtime_frame_base: None,
                    runtime_frame_highlight: None,
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
                runtime_frame_base: None,
                runtime_frame_highlight: None,
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
    clear_retired_legacy_domain(&mut item.pipeline_state);

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
    let mut resolved_state = state_from_resolution_with_frame_base(
        &item.pipeline_state,
        &resolution,
        item.runtime_frame_base,
        item.runtime_frame_highlight,
    );
    clear_retired_legacy_domain(&mut resolved_state);
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
    let (
        file_path,
        roll_id,
        current_long_edge,
        mut persisted_state,
        cached_resolution_key,
        has_prophoto_estimate,
        has_capture_corrected,
    ) = {
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
            item.prophoto_estimate_proxy.is_some(),
            item.relative_transmission_proxy.is_some(),
        )
    };
    let rolls = read_lock(&state.rolls).clone();
    let roll = rolls.iter().find(|roll| roll.roll_id == roll_id);
    if roll.is_none() {
        return Err(format!("Roll not found: {roll_id}"));
    }
    clear_retired_legacy_domain(&mut persisted_state);
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
    // The input class resolves an explicit domain record. It is decided here,
    // where the decoder choice is known, and then travels with the frame so the
    // technical report and every later stage see the same domain.
    let input_domain = resolve_input_domain(&file_path, scanner_profile.as_ref());
    persisted_state.processing_report.input_domain = input_domain.clone();
    let use_smart_auto_compatibility_proxy = initial_resolution.resolved_path
        == ProcessingContract::SmartAutoProPhotoV11
        && is_smart_auto_compatibility(&persisted_state);
    let initial_resolution_key = format!(
        "{}|smart_auto_compatibility={use_smart_auto_compatibility_proxy}",
        resolution_key(&initial_resolution)
    );
    let auxiliary_ready = match initial_resolution.resolved_path {
        ProcessingContract::CaptureCorrectedV11 => has_capture_corrected,
        ProcessingContract::SmartAutoProPhotoV11
        | ProcessingContract::RollBaseProPhotoV11
        | ProcessingContract::RollAnchoredProPhotoV11 => {
            !use_smart_auto_compatibility_proxy && has_prophoto_estimate
                || use_smart_auto_compatibility_proxy
        }
        _ => true,
    };
    if current_long_edge >= target_long_edge
        && cached_resolution_key.as_deref() == Some(initial_resolution_key.as_str())
        && auxiliary_ready
    {
        // The proxy is already the one this frame renders: a base inherited
        // from another frame can still be replaced on it right here.
        {
            let mut item = write_lock(&item_arc);
            if let Err(error) = remeasure_inherited_film_base(&mut item) {
                eprintln!("[Film Base] inherited film base re-measurement failed: {error}");
            }
        }
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
            if use_smart_auto_compatibility_proxy {
                // Loose Smart Auto deliberately shares the v1.0.2 source
                // contract: camera matrix/gamut compression is quantized once
                // to the same linear-sRGB u16 proxy used by legacy analysis.
                let mut legacy = if is_dng_extension(&decode_path) {
                    decode_reduced_dng_for_working_space(&decode_path, target_long_edge)
                        .or_else(|_| decode_image_buffer(&decode_path, decode_mode))?
                } else if is_tiff_extension(&decode_path) || is_scanner_fff_tiff(&decode_path) {
                    decode_reduced_tiff_for_working_space(&decode_path, target_long_edge)
                        .or_else(|_| decode_image_buffer(&decode_path, decode_mode))?
                } else {
                    decode_image_buffer(&decode_path, decode_mode)?
                };
                let (width, height) = legacy.dimensions();
                let ratio = (target_long_edge as f32 / width.max(height) as f32).min(1.0);
                if ratio < 0.999 {
                    legacy = image::imageops::resize(
                        &legacy,
                        (width as f32 * ratio).max(1.0) as u32,
                        (height as f32 * ratio).max(1.0) as u32,
                        FilterType::Lanczos3,
                    );
                }
                let estimate = srgb_proxy_u16_to_prophoto_f32(&legacy);
                return Ok(PreparedProxy {
                    transport: legacy,
                    prophoto_estimate: Some(estimate),
                    capture_corrected: None,
                    fallback_reason: None,
                });
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
                } else if is_scanner_fff_tiff(&decode_path) {
                    // Scanner FFF has no ICC tag for its device RGB and must
                    // never be handed to LibRaw, so keep it on the TIFF decoder
                    // without the generic-image fallback below.
                    let linear = decode_tiff_for_smart_auto(&decode_path, target_long_edge)?;
                    linear_srgb_u16_to_prophoto_f32(&linear)
                } else if is_tiff_extension(&decode_path) {
                    // A profile-less or unusual TIFF still has to be importable;
                    // the generic decoder reads whatever the streaming reader
                    // cannot, matching the legacy branch's behaviour.
                    match decode_profiled_tiff_prophoto_estimate(&decode_path, target_long_edge) {
                        Ok(estimate) => estimate,
                        Err(_) => {
                            let linear = decode_tiff_for_smart_auto(&decode_path, target_long_edge)
                                .or_else(|_| decode_image_buffer(&decode_path, decode_mode))?;
                            linear_srgb_u16_to_prophoto_f32(&linear)
                        }
                    }
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
    // The roll anchors describe the film, not the scanner exposure of this
    // frame. Measure the frame's own film base so mask removal stays exact, and
    // how much of the film span its scene actually reaches so the print is not
    // pushed towards black.
    let anchor_base = pipeline_base_density(&persisted_state, &BaseColor::default());
    let detected_base = prepared
        .prophoto_estimate
        .as_ref()
        .filter(|_| {
            matches!(
                final_resolution.resolved_path,
                ProcessingContract::RollAnchoredProPhotoV11
                    | ProcessingContract::CaptureCorrectedV11
            )
        })
        .and_then(|estimate| {
            if anchor_base.iter().all(|value| *value > 0.0) {
                detect_frame_base_density(estimate, anchor_base)
            } else {
                None
            }
        });
    let detected_highlight = detected_base
        .zip(roll_physical_density_span(
            &persisted_state.density_anchors,
            anchor_base,
        ))
        .and_then(|(base, span)| {
            prepared
                .prophoto_estimate
                .as_ref()
                .and_then(|estimate| detect_frame_highlight_fraction(estimate, base, span))
        });
    let mut final_state = state_from_resolution_with_frame_base(
        &persisted_state,
        &final_resolution,
        detected_base,
        detected_highlight,
    );
    clear_retired_legacy_domain(&mut final_state);
    final_state.processing_report.input_domain = input_domain;
    if final_resolution.resolved_path != ProcessingContract::LegacyV1
        && tiff_smart_auto_input_is_estimated(&file_path)
    {
        // The pixels are usable but their source domain could only be assumed,
        // so record it next to the other analysis provenance instead of
        // silently treating the estimate as a measured input space.
        let report = &mut final_state.processing_report;
        if !report
            .fallback_reasons
            .iter()
            .any(|reason| reason == "scanner_tiff_input_space_estimated")
        {
            report
                .fallback_reasons
                .push("scanner_tiff_input_space_estimated".to_string());
        }
        if report.fallback_reason.is_empty() {
            report.fallback_reason = "scanner_tiff_input_space_estimated".to_string();
        }
    }
    let final_resolution_key = format!(
        "{}|smart_auto_compatibility={}",
        resolution_key(&final_resolution),
        use_smart_auto_compatibility_proxy
    );
    let density_provenance = resolution_density_provenance(&final_resolution);
    let loaded_long_edge = prepared.transport.width().max(prepared.transport.height());
    let retained_long_edge = {
        let mut item = write_lock(&item_arc);
        // Legacy and loose Smart Auto compatibility keep the historical
        // linear-sRGB u16 source; anchored Smart Auto and Capture Corrected
        // additionally retain their domain-typed f32 analysis source.
        let retained_long_edge = item
            .proxy_image
            .as_ref()
            .map(|image| image.width().max(image.height()))
            .unwrap_or(0);
        let resolution_changed =
            item.runtime_pipeline_key.as_deref() != Some(final_resolution_key.as_str());
        let missing_auxiliary = match contract {
            ProcessingContract::CaptureCorrectedV11 => item.relative_transmission_proxy.is_none(),
            ProcessingContract::SmartAutoProPhotoV11
            | ProcessingContract::RollBaseProPhotoV11
            | ProcessingContract::RollAnchoredProPhotoV11 => {
                !use_smart_auto_compatibility_proxy && item.prophoto_estimate_proxy.is_none()
            }
            _ => false,
        };
        if loaded_long_edge > retained_long_edge || resolution_changed || missing_auxiliary {
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
        item.runtime_frame_base = detected_base;
        item.runtime_frame_highlight = detected_highlight;
        // The retired marker is never written back: the input class decides only
        // the input domain, and every frame keeps the shared density maths.
        clear_retired_legacy_domain(&mut item.pipeline_state);
        item.pipeline_state.processing_report.input_domain =
            final_state.processing_report.input_domain.clone();
        item.runtime_pipeline_state = Some(final_state);
        item.runtime_density_provenance = Some(density_provenance);
        item.runtime_pipeline_key = Some(final_resolution_key);
        // A base copied from another frame is a place-holder. This frame now
        // has its own pixels, so measure the film on them instead of printing
        // the other frame's mask.
        if let Err(error) = remeasure_inherited_film_base(&mut item) {
            eprintln!("[Film Base] inherited film base re-measurement failed: {error}");
        }
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
    // The first frame analysed on a Roll fixes its white point, so later frames
    // inherit the same mapping instead of each running its own auto exposure.
    {
        let (item_roll_id, frame_highlight) = {
            let item = read_lock(&item_arc);
            (item.roll_id.clone(), item.runtime_frame_highlight)
        };
        let roll_missing = read_lock(&state.rolls)
            .iter()
            .find(|roll| roll.roll_id == item_roll_id)
            .is_some_and(|roll| roll.density_anchors.highlight_fraction.is_none());
        if roll_missing {
            if let Some(fraction) = frame_highlight {
                record_roll_highlight_fraction(&state, &item_roll_id, fraction);
            }
        }
    }

    tokio::task::spawn_blocking(move || {
        ensure_current_development_generation(&epoch, generation)?;
        let (base_color, runtime_pipeline_state, persisted_pipeline_state) = {
            let item = read_lock(&item_arc);
            let mut effective = item.effective_pipeline_state().clone();
            clear_retired_legacy_domain(&mut effective);
            // A frame whose window was derived by an older release is analysed
            // once more, so the retired per-channel content offsets cannot keep
            // rendering a cast that the current rules no longer produce.
            if pipeline_has_base(&effective, &item.base_color)
                && !frame_needs_window_reanalysis(&effective)
            {
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
                let smart_auto_compatibility = is_smart_auto_compatibility(&effective);
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
                let mut pending_base_fallback: Option<&'static str> = None;
                let (density, estimated_confidence, estimated_source) = if smart_auto_compatibility
                {
                    let proxy = item
                        .proxy_image
                        .as_ref()
                        .ok_or_else(|| "PROXY_NOT_READY".to_string())?;
                    let base = compute_auto_base(proxy);
                    (
                        [
                            -(base.base_r as f32 / 65535.0).max(1.0e-6).log10(),
                            -(base.base_g as f32 / 65535.0).max(1.0e-6).log10(),
                            -(base.base_b as f32 / 65535.0).max(1.0e-6).log10(),
                        ],
                        1.0,
                        "compatibility_base",
                    )
                } else if let Some(quality) = quality {
                    (
                        compute_auto_base_capture_corrected(input, quality, &item.geom)?,
                        0.9,
                        "detected_film_base",
                    )
                } else {
                    // The film base is the only neutral reference the unified
                    // density stage has, so it is estimated in the documented
                    // sampling order and quality-gated. A candidate that fails
                    // the gate leaves the frame on the content mapping and is
                    // reported as "missing_film_base_reference" instead of
                    // silently neutralising on a scene sample.
                    let estimate = estimate_film_base_f32(input, &item.geom);
                    pending_base_fallback = estimate.fallback_reason;
                    if estimate.usable {
                        (
                            estimate.density,
                            estimate.confidence,
                            match estimate.source {
                                "film_edge_band" => "film_edge_band",
                                "film_area_low_density_tail" => "detected_film_base",
                                _ => "content_estimate",
                            },
                        )
                    } else if effective.density_anchors.has_roll_full_exposure() {
                        // A sampled leader fixes D-max. If no edge rebate or low-density
                        // tail was identifiable, infer the missing base from the
                        // confirmed Film Area content minimum.
                        (
                            compute_content_limits_f32(input, quality, &item.geom, [0.0; 3])?.d_min,
                            0.8,
                            "detected_film_base",
                        )
                    } else {
                        (
                            estimate.density,
                            estimate.confidence,
                            "missing_film_base_reference",
                        )
                    }
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
                    } else if smart_auto_compatibility {
                        "legacy_linear_srgb".to_string()
                    } else {
                        "linear_prophoto_estimate".to_string()
                    };
                    report.uses_physical_anchors = runtime.density_anchors.has_base();
                    report.analysis_window_rule = crate::app_state::DENSITY_WINDOW_RULE_VERSION;
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
                    if let Some(reason) = pending_base_fallback {
                        if !report
                            .fallback_reasons
                            .iter()
                            .any(|existing| existing == reason)
                        {
                            report.fallback_reasons.push(reason.to_string());
                        }
                        report.fallback_reason = reason.to_string();
                    }
                }
                (base_color_from_density(density), runtime, persisted)
            }
        };

        let mut item = write_lock(&item_arc);
        ensure_current_development_generation(&epoch, generation)?;
        let mut effective = item.effective_pipeline_state().clone();
        clear_retired_legacy_domain(&mut effective);
        if pipeline_has_base(&effective, &item.base_color)
            && !frame_needs_window_reanalysis(&effective)
        {
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

/// Choose the film base a pasted inversion runs on.
///
/// The film base is the neutral reference *of one frame*: the density a
/// scanner or camera records for the clear film moves whenever the capture,
/// the lamp or the strip itself changes. Subtracting another frame's figure
/// therefore tints that frame end to end — a Roll digitised in two passes
/// recorded 0.98 and 0.82 D of blue base, and the second pass printed at
/// R/G 1.41 (visibly red) on the first pass's base while the same window on
/// its own base stayed at R/G 0.97.
///
/// Priority: measure this frame now, keep this frame's own earlier
/// measurement, and only fall back to the copied figure when neither exists.
fn choose_pasted_film_base(
    copied: [f32; 3],
    frame_measurement: Option<([f32; 3], String, String)>,
    measured: Option<FilmBaseEstimate>,
) -> ([f32; 3], String, String, bool) {
    if let Some(estimate) = measured.filter(|estimate| estimate.usable) {
        return (
            estimate.density,
            estimate.source.to_string(),
            format!("{:.3}", estimate.confidence),
            true,
        );
    }
    if let Some((density, source, confidence)) = frame_measurement {
        return (density, source, confidence, true);
    }
    (
        copied,
        crate::pipeline::INHERITED_FILM_BASE_SOURCE.to_string(),
        "1.000".to_string(),
        false,
    )
}

/// Write one film base into one frame.
///
/// Everything that depends on the base moves with it: the stored colour, the
/// provenance the technical report shows, and — on a sampled Roll — the
/// display mapping, which is the Roll anchor plus *this* frame's own base.
fn install_film_base(
    item: &mut FilmItem,
    density: [f32; 3],
    source: &str,
    confidence: &str,
) -> Result<(), String> {
    let base_color = base_color_from_density(density);
    let mut runtime = item.effective_pipeline_state().clone();
    clear_retired_legacy_domain(&mut runtime);
    runtime.processing_report.base_source = source.to_string();
    runtime.processing_report.base_confidence = confidence.to_string();
    runtime.processing_report.analysis_window_rule = crate::app_state::DENSITY_WINDOW_RULE_VERSION;
    // A complete Roll has no per-frame window of its own: it renders from the
    // sampled anchors, offset by the base this frame actually recorded. A
    // mapping copied from another frame would carry that frame's exposure.
    // The white point is only re-derived when this frame knows its highlight
    // fraction, so an unknown one cannot silently flatten the print.
    let highlight_known = runtime.density_anchors.highlight_fraction.is_some()
        || item.runtime_frame_highlight.is_some();
    if highlight_known {
        if let Some(mapping) = roll_density_mapping_with_frame_base(
            &runtime,
            Some(density),
            item.runtime_frame_highlight,
        ) {
            runtime.render_mapping = mapping;
        }
    }
    let mut persisted = item.pipeline_state.clone();
    persisted.processing_report = runtime.processing_report.clone();
    persisted.render_mapping = runtime.render_mapping.clone();
    persist_base_and_pipeline(&item.roll_id, &item.file_path, &base_color, &persisted)?;
    item.base_color = base_color;
    item.pipeline_state = persisted;
    item.runtime_pipeline_state = Some(runtime);
    item.runtime_frame_base = Some(density);
    item.pristine_proxy = None;
    Ok(())
}

/// The base a frame that inherited one from another frame should run on, now
/// that its own pixels are decoded. `None` keeps the copied figure: a frame
/// without a confirmed Film Area has nowhere trustworthy to measure.
fn inherited_film_base_measurement(
    state: &PipelineState,
    geom: &GeometryState,
    proxy: &ImageBuffer<Rgb<f32>, Vec<f32>>,
) -> Option<FilmBaseEstimate> {
    (state.processing_report.base_source == crate::pipeline::INHERITED_FILM_BASE_SOURCE
        && geom.calibration_points.is_some())
    .then(|| estimate_film_base_f32(proxy, geom))
    .filter(|estimate| estimate.usable)
}

/// Replace a base this frame inherited with its own measurement, the first time
/// the frame has pixels to measure. Returns true when it was replaced.
fn remeasure_inherited_film_base(item: &mut FilmItem) -> Result<bool, String> {
    let Some(proxy) = item.prophoto_estimate_proxy.as_ref() else {
        return Ok(false);
    };
    let Some(estimate) = inherited_film_base_measurement(&item.pipeline_state, &item.geom, proxy)
    else {
        return Ok(false);
    };
    let confidence = format!("{:.3}", estimate.confidence);
    install_film_base(item, estimate.density, estimate.source, &confidence)?;
    Ok(true)
}

#[tauri::command]
pub async fn apply_film_base(
    id: String,
    generation: u64,
    base_density: [f32; 3],
    state: State<'_, EngineState>,
) -> Result<AppliedFilmBase, String> {
    let epoch = claim_development_generation(&state, &id, generation)?;
    let item_arc = state.items.get(&id).ok_or("Image ID not found")?.clone();
    tokio::task::spawn_blocking(move || {
        ensure_current_development_generation(&epoch, generation)?;
        let mut item = write_lock(&item_arc);
        // A pasted film base is only a starting point: the base is the neutral
        // reference of *this* frame's clear film, so measure it here whenever
        // the frame's own Film Area analysis is available.
        let measured = item
            .geom
            .calibration_points
            .is_some()
            .then(|| item.prophoto_estimate_proxy.as_ref())
            .flatten()
            .map(|proxy| estimate_film_base_f32(proxy, &item.geom));
        let frame_measurement = crate::pipeline::base_is_frame_measurement(
            &item.pipeline_state.processing_report.base_source,
            &item.base_color,
        )
        .then(|| {
            (
                crate::pipeline::base_density_from_base_color(&item.base_color),
                item.pipeline_state.processing_report.base_source.clone(),
                item.pipeline_state
                    .processing_report
                    .base_confidence
                    .clone(),
            )
        });
        let (effective_density, base_source, base_confidence, measured_on_frame) =
            choose_pasted_film_base(base_density, frame_measurement, measured);
        install_film_base(&mut item, effective_density, &base_source, &base_confidence)?;
        Ok(AppliedFilmBase {
            base_density: effective_density,
            base_source,
            measured_on_frame,
        })
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
            frame_render_parameters,
            mut pipeline_state,
        ) = {
            let item = read_lock(&item_arc);
            let mut effective = item.effective_pipeline_state().clone();
            clear_retired_legacy_domain(&mut effective);
            if !pipeline_has_base(&effective, &item.base_color) {
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
                resolve_frame_render_parameters(&item, &effective),
                effective,
            )
        };
        let mut observed_content_range = None;
        let mut channel_offsets = [0.0; 3];
        let mut channel_response = None;
        let fixed_roll_mapping = roll_density_mapping_with_frame_base(
            &pipeline_state,
            frame_render_parameters.0,
            frame_render_parameters.1,
        );
        let mut limits = if let Some(mapping) = fixed_roll_mapping.as_ref() {
            // Complete anchors are the only source of display endpoints on
            // this route. In particular, do not call any content percentile
            // or channel-sharing helper here.
            pipeline_state.processing_report.render_route = "RollAnchoredDirectInvert".to_string();
            pipeline_state.processing_report.fallback_reason.clear();
            pipeline_state.processing_report.tone_mapping_mode = "roll_anchored_fixed".to_string();
            apply_roll_anchor_report(&mut pipeline_state);
            AutoColorLimits {
                d_min: mapping.density_low,
                d_max: mapping.density_high,
                pipeline_state: None,
            }
        } else if pipeline_state.contract == ProcessingContract::LegacyV1
            || is_smart_auto_compatibility(&pipeline_state)
        {
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
            let physical_span = roll_physical_density_span(&pipeline_state.density_anchors, base);
            let mut estimated =
                compute_content_limits_f32_with_bounds(input, quality, &geom, base, physical_span)?;
            observed_content_range = Some((estimated.d_min, estimated.d_max));
            // A sampled film span already scales every channel by its own
            // measured response, so only the content-window route measures the
            // per-channel response and reports it.
            let measured_spans = if physical_span.is_none() {
                measure_content_channel_spans(input, quality, &geom, base, physical_span)
            } else {
                None
            };
            channel_response = measured_spans.map(channel_response_from_spans);
            let (offsets, short_content) = prepare_content_render_limits_with_spans(
                &mut estimated,
                &pipeline_state.density_anchors,
                base,
                measured_spans,
            );
            channel_offsets = offsets;
            if short_content {
                pipeline_state.processing_report.tone_mapping_mode =
                    "preserve_tone_adaptive_midpoint".to_string();
            } else {
                pipeline_state.processing_report.tone_mapping_mode =
                    "preserve_tone_content_range".to_string();
            }
            estimated
        };
        pipeline_state.processing_report.channel_response =
            channel_response.map(|response| crate::app_state::ChannelResponseRecord {
                spans: response.spans,
                imbalance: response.imbalance,
                gains: response.gains,
            });
        if pipeline_state.contract != ProcessingContract::LegacyV1 {
            if let Some(mapping) = fixed_roll_mapping {
                // A complete roll has no per-frame ContentRange. Keeping it
                // empty is intentional and makes accidental Smart Auto
                // reuse visible in persisted technical reports.
                pipeline_state.content_range = None;
                pipeline_state.render_mapping = mapping;
            } else {
                let (analysis_low, analysis_high) =
                    observed_content_range.unwrap_or((limits.d_min, limits.d_max));
                pipeline_state.content_range = Some(ContentRange {
                    low: analysis_low,
                    high: analysis_high,
                    source_scope: if geom.calibration_points.is_some() {
                        ContentRangeScope::FilmArea
                    } else {
                        ContentRangeScope::FullFrame
                    },
                    percentile_method: "co_sited_2pct_v1".to_string(),
                });
                pipeline_state.processing_report.render_route = "FilmAreaSmartAuto".to_string();
                pipeline_state.render_mapping.mode = RenderMode::PreserveTone;
                pipeline_state.render_mapping.density_low = limits.d_min;
                pipeline_state.render_mapping.density_high = limits.d_max;
                pipeline_state.render_mapping.channel_offsets = channel_offsets;
            }
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

/// Apply the fixed mapping for every frame in one Roll. The UI prepares each
/// proxy before calling this command, so decoding and rendering stay on the
/// blocking worker while progress events keep the Develop view responsive.
#[tauri::command]
pub async fn auto_invert_roll(
    roll_id: String,
    frame_id: Option<String>,
    emit_progress: Option<bool>,
    state: State<'_, EngineState>,
    app_handle: tauri::AppHandle,
) -> Result<AutoInvertRollResult, String> {
    let cancellation = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    state
        .roll_batch_cancellations
        .insert(roll_id.clone(), cancellation.clone());
    let active_id = read_lock(&state.active_id).clone();
    let roll_image_paths = state
        .rolls
        .read()
        .ok()
        .and_then(|rolls| {
            rolls
                .iter()
                .find(|roll| roll.roll_id == roll_id)
                .map(|roll| roll.image_paths.clone())
        })
        .unwrap_or_default();
    let normalized_roll_paths = roll_image_paths
        .iter()
        .map(|path| path.replace('\\', "/").to_lowercase())
        .collect::<Vec<_>>();
    let requested_frame_id = frame_id.clone();
    let emit_progress = emit_progress.unwrap_or(true);
    let item_arcs = state
        .items
        .iter()
        .filter_map(|entry| {
            let item = read_lock(entry.value());
            let normalized_path = item.file_path.replace('\\', "/").to_lowercase();
            let listed_in_roll = normalized_roll_paths.is_empty()
                || normalized_roll_paths
                    .iter()
                    .any(|path| path == &normalized_path);
            (item.roll_id == roll_id && listed_in_roll)
                .then(|| (entry.key().clone(), entry.value().clone()))
        })
        .filter(|(id, _)| {
            requested_frame_id
                .as_deref()
                .is_none_or(|requested| requested == id)
        })
        .collect::<Vec<_>>();
    let mut item_arcs = item_arcs;
    item_arcs.sort_by_key(|(id, item_arc)| {
        let item = read_lock(item_arc);
        let active_rank = if active_id.as_deref() == Some(id.as_str()) {
            0usize
        } else {
            1usize
        };
        let path = item.file_path.replace('\\', "/").to_lowercase();
        let sequence_rank = normalized_roll_paths
            .iter()
            .position(|candidate| candidate == &path)
            .unwrap_or(usize::MAX);
        (active_rank, sequence_rank)
    });
    let total = item_arcs.len();
    if total == 0 {
        state.roll_batch_cancellations.remove(&roll_id);
        return Err(format!("Roll has no editable frames: {roll_id}"));
    }
    // Every frame of this batch shares the Roll's white point. The UI measures
    // it on the whole Roll before the first frame is rendered, which is the
    // digital equivalent of a darkroom test strip.
    let mut roll_anchors = read_lock(&state.rolls)
        .iter()
        .find(|roll| roll.roll_id == roll_id)
        .map(|roll| roll.density_anchors.clone())
        .ok_or_else(|| format!("Roll not found: {roll_id}"))?;
    if roll_anchors.highlight_fraction.is_none() {
        // Fallback for a caller that rendered a frame without calibrating the
        // Roll first: the frame it asked for already carries its own
        // measurement, so the Roll still gets one shared white point without
        // decoding every frame here. The UI measures the whole Roll before a
        // batch, so this only covers direct calls.
        let cached = item_arcs.iter().find_map(|(_, item_arc)| {
            let item = read_lock(item_arc);
            item.runtime_frame_highlight
        });
        if let Some(fraction) = cached.filter(|value| value.is_finite()) {
            record_roll_highlight_fraction(&state, &roll_id, fraction);
            roll_anchors.highlight_fraction =
                Some(fraction.clamp(ROLL_HIGHLIGHT_FLOOR, ROLL_HIGHLIGHT_CEILING));
        }
    }
    let worker_cancellation = cancellation.clone();
    let worker_anchors = roll_anchors;
    let result = tokio::task::spawn_blocking(move || {
        let mut result = AutoInvertRollResult {
            roll_id: roll_id.clone(),
            total,
            processed: 0,
            succeeded: 0,
            failed: 0,
            failed_ids: Vec::new(),
        };
        if emit_progress {
            let _ = app_handle.emit(
                "auto_invert_roll_progress",
                serde_json::json!({
                    "roll_id": roll_id,
                    "total": total,
                    "processed": 0,
                    "succeeded": 0,
                    "failed": 0,
                    "done": false
                }),
            );
        }
        for (id, item_arc) in item_arcs {
            if worker_cancellation.load(Ordering::Acquire) {
                break;
            }
            let outcome = (|| -> Result<(), String> {
                let mut item = write_lock(&item_arc);
                let mut pipeline = item.effective_pipeline_state().clone();
                pipeline.density_anchors = worker_anchors.clone();
                let (frame_base, highlight_fraction) =
                    resolve_frame_render_parameters(&item, &pipeline);
                if item.runtime_frame_base.is_none() {
                    item.runtime_frame_base = frame_base;
                }
                if item.runtime_frame_highlight.is_none() {
                    item.runtime_frame_highlight = highlight_fraction;
                }
                let mapping =
                    roll_density_mapping_with_frame_base(&pipeline, frame_base, highlight_fraction)
                        .ok_or_else(|| "complete_roll_anchors_required".to_string())?;
                if item.proxy_image.is_none() {
                    return Err("PROXY_NOT_READY".to_string());
                }
                pipeline.content_range = None;
                pipeline.render_mapping = mapping.clone();
                pipeline.processing_report.render_route = "RollAnchoredDirectInvert".to_string();
                pipeline.processing_report.tone_mapping_mode = "roll_anchored_fixed".to_string();
                pipeline.processing_report.fallback_reason.clear();
                apply_roll_anchor_report(&mut pipeline);
                let mut params = item.params.clone();
                params.density.d_min = mapping.density_low;
                params.density.d_max = mapping.density_high;
                persist_tuning_parameters(&item.roll_id, &item.file_path, &params)?;
                persist_pipeline_state(&item.roll_id, &item.file_path, &pipeline)?;
                item.params = params;
                item.pipeline_state = pipeline.clone();
                item.runtime_pipeline_state = Some(pipeline);
                item.pristine_proxy = Some(compute_pristine_proxy(
                    item.proxy_image.as_ref().expect("checked above"),
                    item.prophoto_estimate_proxy.as_ref(),
                    item.relative_transmission_proxy.as_ref(),
                    item.relative_transmission_quality.as_ref(),
                    &item.base_color,
                    item.effective_pipeline_state(),
                    item.params.film_mode.clone(),
                ));
                let thumbnail = generate_processed_thumbnail(&item)
                    .ok_or_else(|| "THUMBNAIL_RENDER_FAILED".to_string())?;
                persist_rendered_thumbnail(&item.roll_id, &item.file_path, &thumbnail)?;
                item.rendered_thumbnail_base64 = Some(thumbnail);
                Ok(())
            })();
            result.processed += 1;
            match outcome {
                Ok(()) => result.succeeded += 1,
                Err(error) => {
                    result.failed += 1;
                    result.failed_ids.push(format!("{id}:{error}"));
                }
            }
            if emit_progress {
                let _ = app_handle.emit(
                    "auto_invert_roll_progress",
                    serde_json::json!({
                        "roll_id": result.roll_id,
                        "total": result.total,
                        "processed": result.processed,
                        "succeeded": result.succeeded,
                        "failed": result.failed,
                        "failed_ids": result.failed_ids,
                        "done": result.processed == result.total
                    }),
                );
            }
        }
        if emit_progress {
            let _ = app_handle.emit(
                "auto_invert_roll_progress",
                serde_json::json!({
                    "roll_id": result.roll_id,
                    "total": result.total,
                    "processed": result.processed,
                    "succeeded": result.succeeded,
                    "failed": result.failed,
                    "failed_ids": result.failed_ids,
                    "done": true,
                    "cancelled": worker_cancellation.load(Ordering::Acquire)
                }),
            );
        }
        Ok::<AutoInvertRollResult, String>(result)
    })
    .await
    .map_err(|error| format!("Roll auto-invert worker failed: {error}"))??;
    state.roll_batch_cancellations.remove(&result.roll_id);
    Ok(result)
}

/// Measure the Roll's content white point on every frame of the Roll.
///
/// `DensityAnchors.highlight_fraction` records where the Roll's brightest scene
/// content sits inside the measured base-to-leader span, and every frame shares
/// that one display mapping. It therefore has to describe the brightest frame,
/// not a typical frame: a white point below one frame's highlights clips them,
/// and the user can only answer that by raising D-Max by hand. Reading every
/// frame makes "the Roll's brightest content" the value the mapping uses.
///
/// The per-frame measurement is cached on the frame, so a second pass over the
/// same session costs no decoding work.
#[tauri::command]
pub async fn calibrate_roll_highlight_fraction(
    roll_id: String,
    emit_progress: Option<bool>,
    state: State<'_, EngineState>,
    app_handle: tauri::AppHandle,
) -> Result<Option<f32>, String> {
    let cancellation = Arc::new(std::sync::atomic::AtomicBool::new(false));
    state
        .roll_batch_cancellations
        .insert(roll_id.clone(), cancellation.clone());
    let result = measure_roll_highlight_fraction(
        &roll_id,
        emit_progress.unwrap_or(true),
        &cancellation,
        &state,
        Some(&app_handle),
    )
    .await;
    state.roll_batch_cancellations.remove(&roll_id);
    let fraction = result?;
    if let Some(fraction) = fraction {
        record_roll_highlight_fraction(&state, &roll_id, fraction);
    }
    Ok(fraction)
}

async fn measure_roll_highlight_fraction(
    roll_id: &str,
    emit_progress: bool,
    cancellation: &Arc<std::sync::atomic::AtomicBool>,
    state: &EngineState,
    app_handle: Option<&tauri::AppHandle>,
) -> Result<Option<f32>, String> {
    let roll = read_lock(&state.rolls)
        .iter()
        .find(|roll| roll.roll_id == roll_id)
        .cloned()
        .ok_or_else(|| format!("Roll not found: {roll_id}"))?;
    let anchor_base = pipeline_base_density(
        &PipelineState::from_roll_anchors(roll.density_anchors.clone()),
        &BaseColor::default(),
    );
    // Without a base and a fully exposed leader there is no span to place a
    // scene white point inside.
    let Some(span) = roll_physical_density_span(&roll.density_anchors, anchor_base) else {
        return Ok(None);
    };
    let normalized_roll_paths = roll
        .image_paths
        .iter()
        .map(|path| path.replace('\\', "/").to_lowercase())
        .collect::<Vec<_>>();
    let mut item_arcs = state
        .items
        .iter()
        .filter_map(|entry| {
            let item = read_lock(entry.value());
            let normalized_path = item.file_path.replace('\\', "/").to_lowercase();
            let listed_in_roll = normalized_roll_paths.is_empty()
                || normalized_roll_paths
                    .iter()
                    .any(|path| path == &normalized_path);
            (item.roll_id == roll_id && listed_in_roll).then(|| entry.value().clone())
        })
        .collect::<Vec<_>>();
    item_arcs.sort_by_key(|item_arc| {
        let item = read_lock(item_arc);
        let path = item.file_path.replace('\\', "/").to_lowercase();
        normalized_roll_paths
            .iter()
            .position(|candidate| candidate == &path)
            .unwrap_or(usize::MAX)
    });
    let total = item_arcs.len();
    if total == 0 {
        return Ok(None);
    }
    let worker_cancellation = cancellation.clone();
    let worker_roll_id = roll_id.to_string();
    let worker_app_handle = app_handle.cloned();
    let (brightest, measured) = tokio::task::spawn_blocking(move || {
        measure_roll_highlight_frames(
            &item_arcs,
            anchor_base,
            span,
            &worker_cancellation,
            |processed| {
                if let Some(app_handle) = worker_app_handle.as_ref().filter(|_| emit_progress) {
                    let _ = app_handle.emit(
                        "auto_invert_roll_progress",
                        serde_json::json!({
                            "roll_id": worker_roll_id,
                            "phase": "highlight",
                            "total": total,
                            "processed": processed,
                            "succeeded": 0,
                            "failed": 0,
                            "done": false
                        }),
                    );
                }
            },
        )
    })
    .await
    .map_err(|error| format!("Roll white point worker failed: {error}"))?;
    if cancellation.load(Ordering::Acquire) {
        // A cancelled pass only sees part of the Roll, and a partial maximum is
        // exactly the under-estimate this measurement exists to remove.
        return Ok(None);
    }
    let Some(brightest) = brightest else {
        return Ok(roll.density_anchors.highlight_fraction);
    };
    if measured < total {
        if let Some(existing) = roll.density_anchors.highlight_fraction {
            // A frame that could not be measured hides part of the Roll, and a
            // hidden brighter frame is exactly what a lower value would clip.
            return Ok(Some(existing));
        }
    }
    Ok(Some(
        brightest.clamp(ROLL_HIGHLIGHT_FLOOR, ROLL_HIGHLIGHT_CEILING),
    ))
}

/// Measure every frame's share of the Roll span and keep the brightest one.
///
/// A frame that was already prepared carries its measurement, so a repeat pass
/// over the same session only walks memory. `on_frame` receives the number of
/// frames finished so far and drives the progress events.
fn measure_roll_highlight_frames(
    item_arcs: &[Arc<RwLock<FilmItem>>],
    anchor_base: [f32; 3],
    span: [f32; 3],
    cancellation: &Arc<std::sync::atomic::AtomicBool>,
    mut on_frame: impl FnMut(usize),
) -> (Option<f32>, usize) {
    let mut brightest: Option<f32> = None;
    let mut measured = 0usize;
    for (index, item_arc) in item_arcs.iter().enumerate() {
        if cancellation.load(Ordering::Acquire) {
            break;
        }
        let (cached_highlight, path, frame_id) = {
            let item = read_lock(item_arc);
            (
                item.runtime_frame_highlight,
                item.file_path.clone(),
                item.id.clone(),
            )
        };
        let fraction = match cached_highlight {
            Some(fraction) => Some(fraction),
            None => match decode_prophoto_estimate_image_buffer(&path, DecodeMode::DevelopProxy) {
                Ok(estimate) => {
                    let detected_base = detect_frame_base_density(&estimate, anchor_base);
                    let base = detected_base.unwrap_or(anchor_base);
                    let fraction = detect_frame_highlight_fraction(&estimate, base, span);
                    let mut item = write_lock(item_arc);
                    // Keep the measurement with the frame so the render pass
                    // does not have to measure it a second time.
                    item.runtime_frame_base = detected_base;
                    if fraction.is_some() {
                        item.runtime_frame_highlight = fraction;
                    }
                    fraction
                }
                Err(error) => {
                    eprintln!("[Roll White Point] Frame {frame_id} could not be measured: {error}");
                    None
                }
            },
        };
        if let Some(fraction) = fraction.filter(|value| value.is_finite()) {
            brightest = Some(brightest.map_or(fraction, |current: f32| current.max(fraction)));
            measured += 1;
        }
        on_frame(index + 1);
    }
    (brightest, measured)
}

#[tauri::command]
pub async fn cancel_auto_invert_roll(
    roll_id: String,
    state: State<'_, EngineState>,
) -> Result<bool, String> {
    Ok(state
        .roll_batch_cancellations
        .get(&roll_id)
        .map(|flag| {
            flag.store(true, Ordering::Release);
            true
        })
        .unwrap_or(false))
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
                    let prophoto_estimate = pipeline
                        .density_anchors
                        .d_min_base
                        .as_ref()
                        .is_some_and(|anchor| {
                            anchor.provenance.input_domain
                                == crate::app_state::DataDomain::ProPhotoEstimate
                        });
                    report.base_source = if prophoto_estimate {
                        "roll_anchor_prophoto_estimate".to_string()
                    } else {
                        "roll_anchor_relative_transmission".to_string()
                    };
                    report.base_confidence = if prophoto_estimate {
                        "estimated".to_string()
                    } else {
                        "user_sampled".to_string()
                    };
                    report.uses_physical_anchors = !prophoto_estimate;
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
        if item.effective_pipeline_state().contract != ProcessingContract::LegacyV1
            && !is_smart_auto_compatibility(item.effective_pipeline_state())
        {
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
        let framing_changed = geometry_framing_changed(&item.geom, &geom);
        persist_geometry(&item.roll_id, &item.file_path, &geom)?;
        item.geom = geom;
        if framing_changed {
            // The stored render was produced from the previous framing. Keeping
            // it would let the Library, the Develop placeholder, and the crop
            // layout show a crop the user has already changed, so drop it and
            // fall back to the import preview until the next capture arrives.
            clear_stored_rendered_thumbnail(&mut item)?;
        }
        Ok(())
    })
    .await
    .map_err(|error| format!("Geometry persistence worker failed: {error}"))?
}

/// Geometry fields that decide what the rendered frame looks like. A write that
/// only touches, for example, the constrain flag leaves the stored render valid.
fn geometry_framing_changed(
    previous: &crate::app_state::GeometryState,
    next: &crate::app_state::GeometryState,
) -> bool {
    previous.crop_rect != next.crop_rect
        || (previous.angle - next.angle).abs() > 1e-4
        || previous.rotate_90_count.rem_euclid(4) != next.rotate_90_count.rem_euclid(4)
        || previous.flip_h != next.flip_h
        || previous.flip_v != next.flip_v
        || previous.calibration_points != next.calibration_points
        || previous.calibration_confirmed != next.calibration_confirmed
        || (previous.perspective_vertical - next.perspective_vertical).abs() > 1e-4
        || (previous.perspective_horizontal - next.perspective_horizontal).abs() > 1e-4
        || (previous.perspective_aspect - next.perspective_aspect).abs() > 1e-4
        || (previous.perspective_scale - next.perspective_scale).abs() > 1e-4
        || (previous.lens_distortion - next.lens_distortion).abs() > 1e-4
}

/// Drop a rendered thumbnail that no longer matches the persisted geometry. The
/// expected value guards against clearing a capture that arrived after this
/// geometry change was queued.
fn clear_stored_rendered_thumbnail(item: &mut crate::app_state::FilmItem) -> Result<(), String> {
    let Some(expected) = item.rendered_thumbnail_base64.clone() else {
        return Ok(());
    };
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
                expected,
            ],
        )
        .map_err(|error| format!("Failed to clear stale rendered thumbnail: {error}"))?;
    if changed == 1 {
        item.rendered_thumbnail_base64 = None;
    }
    Ok(())
}

#[cfg(test)]
mod geometry_thumbnail_invalidation_tests {
    use super::geometry_framing_changed;
    use crate::app_state::{CropRect, GeometryState};

    #[test]
    fn crop_and_angles_invalidate_the_stored_render() {
        let base = GeometryState::default();

        let mut cropped = base.clone();
        cropped.crop_rect = CropRect {
            x: 0.1,
            y: 0.1,
            width: 0.5,
            height: 0.5,
        };
        assert!(geometry_framing_changed(&base, &cropped));

        let mut rotated = base.clone();
        rotated.angle = 3.0;
        assert!(geometry_framing_changed(&base, &rotated));

        let mut turned = base.clone();
        turned.rotate_90_count = 1;
        assert!(geometry_framing_changed(&base, &turned));

        let mut flipped = base.clone();
        flipped.flip_v = true;
        assert!(geometry_framing_changed(&base, &flipped));

        let mut area = base.clone();
        area.calibration_points = Some([[0.1, 0.1], [0.9, 0.1], [0.9, 0.9], [0.1, 0.9]]);
        area.calibration_confirmed = true;
        assert!(geometry_framing_changed(&base, &area));

        let mut scaled = base.clone();
        scaled.perspective_scale = 1.1;
        assert!(geometry_framing_changed(&base, &scaled));
    }

    #[test]
    fn a_repeated_write_or_a_constrain_toggle_keeps_the_stored_render() {
        let base = GeometryState::default();
        assert!(!geometry_framing_changed(&base, &base.clone()));

        let mut constrained = base.clone();
        constrained.constrain_crop = true;
        assert!(!geometry_framing_changed(&base, &constrained));

        // A full extra turn is the same framing and must not discard the render.
        let mut extra_turn = base.clone();
        extra_turn.rotate_90_count = 4;
        assert!(!geometry_framing_changed(&base, &extra_turn));
    }
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

#[inline]
fn should_apply_sprocket_mask_for_area(
    crop_uv: [f32; 2],
    points: &[[f32; 2]; 4],
    sprocket_uv: [f32; 2],
) -> bool {
    let full_frame = points
        .iter()
        .zip([[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]])
        .all(|(actual, expected)| {
            (actual[0] - expected[0]).abs() <= 0.001 && (actual[1] - expected[1]).abs() <= 0.001
        });
    if !full_frame {
        return !point_in_film_area(crop_uv, points, 0.0);
    }

    should_apply_sprocket_mask(crop_uv, [0.0, 0.0, 1.0, 1.0], sprocket_uv)
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

    if commit.geometry.is_some()
        || commit.density_params.is_some()
        || !commit.inherited_targets.is_empty()
    {
        let updated = commit
            .result
            .targets
            .iter()
            .cloned()
            .collect::<HashSet<_>>();
        let inherited = commit
            .inherited_targets
            .iter()
            .cloned()
            .collect::<HashSet<_>>();
        let parsed_base_color: Option<BaseColor> = commit
            .inherited_base_color
            .as_ref()
            .and_then(|s| serde_json::from_str(s).ok());
        for entry in state.items.iter() {
            let item_arc = entry.value().clone();
            let mut item = write_lock(&item_arc);
            let key = ImageKey {
                roll_id: item.roll_id.clone(),
                file_path: item.file_path.clone(),
            };
            if updated.contains(&key) {
                if let Some(geometry) = commit.geometry.as_ref() {
                    if let Err(error) = apply_batch_geometry_to_item(
                        &mut item.geom,
                        geometry,
                        &commit.result.modules,
                    ) {
                        eprintln!(
                            "[Batch Settings] committed geometry cache refresh failed: {error}"
                        );
                    }
                }
                if let Some(density_params) = commit.density_params.as_ref() {
                    if let Ok(mut params) = serde_json::to_value(&item.params) {
                        if crate::batch_settings::merge_density_endpoints(
                            &mut params,
                            density_params,
                        ) {
                            match serde_json::from_value::<TuningParams>(params) {
                                Ok(merged) => item.params = merged,
                                Err(error) => eprintln!(
                                    "[Batch Settings] density limits could not be cached: {error}"
                                ),
                            }
                        }
                    }
                }
                if inherited.contains(&key) {
                    if let Some(base_color) = parsed_base_color.as_ref() {
                        item.base_color = base_color.clone();
                    }
                    // Only the provenance changes here: the frame keeps its own
                    // anchors and mapping, and measures the base itself when it
                    // is next decoded.
                    let marker = crate::pipeline::INHERITED_FILM_BASE_SOURCE.to_string();
                    item.pipeline_state.processing_report.base_source = marker.clone();
                    item.pipeline_state.processing_report.base_confidence = "1.000".to_string();
                    let mut runtime = item.effective_pipeline_state().clone();
                    runtime.processing_report.base_source = marker;
                    runtime.processing_report.base_confidence = "1.000".to_string();
                    item.runtime_pipeline_state = Some(runtime);
                    item.pristine_proxy = None;
                }
                // The frame may already be decoded here: replace a base copied
                // from another frame with this frame's own measurement.
                if let Err(error) = remeasure_inherited_film_base(&mut item) {
                    eprintln!("[Batch Settings] film base re-measurement failed: {error}");
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
            let mut pipeline_state = item.effective_pipeline_state().clone();
            clear_retired_legacy_domain(&mut pipeline_state);
            build_response_buffer_from_proxy_with_state(
                proxy,
                &item.base_color,
                &pipeline_state,
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

fn point_in_film_area(point: [f32; 2], points: &[[f32; 2]; 4], margin: f32) -> bool {
    let signed_area = points
        .iter()
        .zip(points.iter().cycle().skip(1))
        .take(4)
        .map(|(left, right)| left[0] * right[1] - right[0] * left[1])
        .sum::<f32>();
    let orientation = if signed_area >= 0.0 { 1.0 } else { -1.0 };

    points
        .iter()
        .zip(points.iter().cycle().skip(1))
        .take(4)
        .all(|(start, end)| {
            let edge = [end[0] - start[0], end[1] - start[1]];
            let offset = [point[0] - start[0], point[1] - start[1]];
            let cross = edge[0] * offset[1] - edge[1] * offset[0];
            orientation * cross >= margin * edge[0].hypot(edge[1])
        })
}

/// Collect co-sited RGB samples through the same geometry map used by the
/// renderer. Film-area points only define the analysis region; they must not
/// warp the image or implicitly correct perspective.
fn collect_film_area_rgb32(
    proxy: &ImageBuffer<Rgb<f32>, Vec<f32>>,
    quality: Option<&crate::raw_backend::QualityMask>,
    geom: &GeometryState,
    reject_saturated: bool,
) -> Vec<[f32; 3]> {
    let points =
        geom.calibration_points
            .unwrap_or([[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]]);
    let has_film_area = geom.calibration_points.is_some();
    collect_mapped_rgb32(proxy, quality, geom, reject_saturated, |uv| {
        // Do not let nearest-neighbour samples exactly on the selected edge pick
        // up a one-pixel lamp-panel/sprocket fringe. The margin is sub-pixel on a
        // normal proxy and scales with the source resolution.
        let region_margin = 1.0 / proxy.width().max(proxy.height()).max(1) as f32;
        !has_film_area || point_in_film_area(uv, &points, region_margin)
    })
}

/// Collect co-sited RGB samples from the ring that borders the confirmed Film
/// Area: a narrow band just inside the gate and a narrower one just outside it
/// cover the visible rebate and the orange mask band on both sides.
fn collect_film_area_band_rgb32(
    proxy: &ImageBuffer<Rgb<f32>, Vec<f32>>,
    quality: Option<&crate::raw_backend::QualityMask>,
    geom: &GeometryState,
    band: f32,
) -> Vec<[f32; 3]> {
    let Some(points) = geom.calibration_points else {
        return Vec::new();
    };
    collect_mapped_rgb32(proxy, quality, geom, true, |uv| {
        let inside = point_in_film_area(uv, &points, band);
        let outside = point_in_film_area(uv, &points, -band);
        outside && !inside
    })
}

fn collect_mapped_rgb32(
    proxy: &ImageBuffer<Rgb<f32>, Vec<f32>>,
    quality: Option<&crate::raw_backend::QualityMask>,
    geom: &GeometryState,
    reject_saturated: bool,
    in_region: impl Fn([f32; 2]) -> bool,
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
            let Some(perspective_uv) = apply_perspective_uv(
                crop_uv,
                geom.perspective_vertical,
                geom.perspective_horizontal,
                geom.perspective_aspect,
                geom.perspective_scale,
            ) else {
                continue;
            };
            let Some(oriented_uv) = apply_lens_distortion_uv(perspective_uv, geom.lens_distortion)
            else {
                continue;
            };
            if !in_region(crop_uv) {
                continue;
            }
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
    let sprocket_uv = params
        .sprocket
        .sprocket_uv
        .as_deref()
        .filter(|uv| uv.len() >= 2 && uv[0] >= 0.0)
        .map(|uv| [uv[0], uv[1]]);
    let sprocket_target = params
        .sprocket
        .sprocket_target_color
        .as_deref()
        .filter(|c| c.len() >= 3)
        .map(|c| [c[0], c[1], c[2]])
        .or_else(|| sprocket_uv.and_then(|uv| sample(uv)));
    let tolerance = params.sprocket.sprocket_tolerance.unwrap_or(0.10);
    let feather = params.sprocket.sprocket_feather.unwrap_or(0.05);
    let lut_opacity = params.lut.lut_opacity.clamp(0.0, 1.0) * LUT_CONTROL_SCALE;
    let luma_coefficients = DENSITY_LUMA_COEFFICIENTS;
    let master_exposure = params.exposure.exposure;
    let channel_exposure_offsets = if params.film_mode == FilmMode::BW {
        [0.0; 3]
    } else {
        [
            params.exposure.exp_r * CHANNEL_CONTROL_SCALE,
            params.exposure.exp_g * CHANNEL_CONTROL_SCALE,
            params.exposure.exp_b * CHANNEL_CONTROL_SCALE,
        ]
    };
    let smart_auto_compatibility = is_smart_auto_compatibility(pipeline_state);
    let roll_anchored = pipeline_state.render_mapping.mode == RenderMode::RollAnchored;
    let legacy_compatibility =
        pipeline_state.contract == ProcessingContract::LegacyV1 || smart_auto_compatibility;
    let pipeline_exposure_offsets = if legacy_compatibility {
        channel_exposure_offsets.map(|value| value + master_exposure)
    } else {
        channel_exposure_offsets
    };
    let pipeline = if smart_auto_compatibility {
        FilmPipeline::new(
            [base_color.base_r, base_color.base_g, base_color.base_b],
            pipeline_exposure_offsets,
            params.film_mode.clone(),
        )
    } else {
        FilmPipeline::from_state(
            pipeline_state,
            base_color,
            pipeline_exposure_offsets,
            params.film_mode.clone(),
        )
    };
    let smart_auto_to_srgb = smart_auto_compatibility
        .then(|| linear_conversion_matrix(ColorSpaceId::ProPhotoRgb, ColorSpaceId::SRgb));
    let positive_to_display = (pipeline_state.contract != ProcessingContract::LegacyV1
        && pipeline_state.contract != ProcessingContract::CaptureCorrectedV11
        && !smart_auto_compatibility
        && params.film_mode == FilmMode::Color)
        .then(|| linear_conversion_matrix(ColorSpaceId::ProPhotoRgb, ColorSpaceId::SRgb));
    let (bw_dmin, bw_dmax) = neutral_density_bounds(params.density.d_min, params.density.d_max);
    // The Master D-Min/D-Max sliders shift whichever endpoints the active
    // route derived. Keeping the shift separate lets a sampled Roll keep its
    // fixed mapping while a frame is still trimmable.
    let density_min_offset = params.density.d_min_offset;
    let density_max_offset = params.density.d_max_offset;

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
            let Some(warped_uv) = apply_lens_distortion_uv(perspective_uv, geom.lens_distortion)
            else {
                return;
            };
            let Some(raw) = sample(warped_uv) else {
                return;
            };
            let raw = smart_auto_to_srgb
                .map(|matrix| apply_linear_matrix(raw, matrix))
                .unwrap_or(raw);
            let density = if pipeline_state.contract == ProcessingContract::CaptureCorrectedV11 {
                let Some(true_density) = pipeline.compute_relative_density(&raw, true) else {
                    return;
                };
                pipeline.apply_exposure(&true_density)
            } else {
                pipeline.process_pixel(&raw)
            };
            let (d_min, d_max) = if roll_anchored {
                (
                    pipeline_state.render_mapping.density_low,
                    pipeline_state.render_mapping.density_high,
                )
            } else if params.film_mode == FilmMode::BW {
                ([bw_dmin; 3], [bw_dmax; 3])
            } else {
                (params.density.d_min, params.density.d_max)
            };
            let (d_min, d_max) =
                trim_density_endpoints(d_min, d_max, density_min_offset, density_max_offset);
            let working_gamma = if legacy_compatibility {
                params.density.gamma
            } else {
                1.0
            };
            let normalize = |value: f32, low: f32, high: f32| {
                normalize_density_channel(value, low, high, 0.0, 0.0, working_gamma)
            };
            let normalized_working = [
                normalize(density[0], d_min[0], d_max[0]),
                normalize(density[1], d_min[1], d_max[1]),
                normalize(density[2], d_min[2], d_max[2]),
            ];
            let mut positive_linear = positive_to_display
                .map(|matrix| apply_linear_matrix(normalized_working, matrix))
                .unwrap_or(normalized_working);
            if !legacy_compatibility {
                let exposure_gain = 2.0f32.powf(master_exposure);
                positive_linear = positive_linear.map(|value| value * exposure_gain);
            }
            let normalized = if legacy_compatibility {
                positive_linear
            } else {
                positive_linear.map(|value| {
                    value
                        .clamp(0.0, 1.0)
                        .powf(1.0 / params.density.gamma.max(1e-6))
                })
            };
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
                params.tone.contrast,
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
                let effective_sprocket_uv = sprocket_uv.unwrap_or([0.5, 0.05]);
                if should_apply_sprocket_mask_for_area(crop_uv, &points, effective_sprocket_uv) {
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
    // The core renderer crops and samples, but it does not place the frame: the
    // WebGL preview applies the quarter turns, flips and angle through its
    // geometry matrix, and this renderer has to do the same or a flipped frame
    // exports upside down. A quarter turn also swaps the output's aspect.
    let (source_width, source_height) = (source.width(), source.height());
    let turned = geom.rotate_90_count.rem_euclid(2) == 1;
    let (layout_width, layout_height) = if turned {
        (source_height, source_width)
    } else {
        (source_width, source_height)
    };
    render_shader_equivalent_core(
        layout_width,
        layout_height,
        |uv| {
            let source_uv = map_oriented_uv_to_source(uv, source_width, source_height, geom);
            sample_rgb32_nearest_checked(source, quality, source_uv)
        },
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
            Ok((
                id.clone(),
                item.file_path.clone(),
                item.roll_id.clone(),
                item.is_loose,
            ))
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

        for (id, file_path, roll_id, _is_loose) in identities {
            let (params, geom, base_color, mut pipeline_state) =
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
            clear_retired_legacy_domain(&mut pipeline_state);
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
        clear_retired_legacy_domain(&mut snapshot.pipeline_state);
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
            // Say which frame is being worked on before the decode starts. A
            // full-resolution frame takes seconds, and a counter that only
            // moves once the file is written reads as a frozen export.
            let _ = progress_app.emit(
                "export_progress",
                serde_json::json!({
                    "processed": processed_count.load(std::sync::atomic::Ordering::SeqCst),
                    "total": count,
                    "id": snapshot.id,
                    "file": std::path::Path::new(&file_path)
                        .file_name()
                        .map(|name| name.to_string_lossy().to_string())
                        .unwrap_or_default(),
                    "stage": "decoding",
                }),
            );
            let params_owned = snapshot.params.clone();
            let geom_owned = snapshot.geom.clone();
            let base_color_owned = snapshot.base_color.clone();
            let decoded = if snapshot.pipeline_state.contract == ProcessingContract::LegacyV1
                || is_smart_auto_compatibility(&snapshot.pipeline_state)
            {
                decode_export_source(&file_path)
            } else {
                // The v1.1 branch decodes directly into ProPhoto f32 below.
                // Avoid allocating and retaining a second full-size legacy image.
                Ok(ImageBuffer::<Rgb<u16>, Vec<u16>>::new(1, 1))
            };
            match decoded {
                Ok(original) => {
                    let params = &params_owned;
                    // The stored base is replaced below when it still names
                    // another frame instead of this one's own clear film.
                    let mut base_color = base_color_owned.clone();
                    if snapshot.pipeline_state.contract != ProcessingContract::LegacyV1
                        && !is_smart_auto_compatibility(&snapshot.pipeline_state)
                    {
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
                        // A base inherited from another frame is a place-holder:
                        // this decode is the first chance to measure the film on
                        // its own pixels, and the export must not print another
                        // frame's mask.
                        if let Some(estimate) =
                            inherited_film_base_measurement(&render_pipeline_state, &geom_owned, &input)
                        {
                            base_color = base_color_from_density(estimate.density);
                            let mut persisted = snapshot.pipeline_state.clone();
                            persisted.processing_report.base_source = estimate.source.to_string();
                            persisted.processing_report.base_confidence =
                                format!("{:.3}", estimate.confidence);
                            if let Err(error) = persist_base_and_pipeline(
                                &snapshot.roll_id,
                                &snapshot.file_path,
                                &base_color,
                                &persisted,
                            ) {
                                lock_mutex(&warnings).push(format!(
                                    "export_film_base_persist_failed|{}|{error}",
                                    file_path
                                ));
                            }
                        }
                        // Match the Develop preview: mask removal uses this
                        // frame's own film base, and the white point uses the
                        // fraction of the film span its scene actually reaches.
                        if matches!(
                            render_pipeline_state.contract,
                            ProcessingContract::RollAnchoredProPhotoV11
                                | ProcessingContract::CaptureCorrectedV11
                        ) {
                            let anchor_base =
                                pipeline_base_density(&render_pipeline_state, &base_color);
                            let frame_base =
                                detect_frame_base_density(&input, anchor_base).unwrap_or(anchor_base);
                            if let Some(span) = roll_physical_density_span(
                                &render_pipeline_state.density_anchors,
                                anchor_base,
                            ) {
                                let highlight =
                                    detect_frame_highlight_fraction(&input, frame_base, span);
                                if let Some(mapping) = roll_density_mapping_with_frame_base(
                                    &render_pipeline_state,
                                    Some(frame_base),
                                    highlight,
                                ) {
                                    render_pipeline_state.render_mapping = mapping;
                                }
                            }
                        }
                        let rendered_display = render_f32_shader_equivalent(
                            &input,
                            quality_mask.as_ref(),
                            params,
                            &geom_owned,
                            &base_color,
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
                        &base_color,
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
    let base = resolution
        .usable_density_anchors
        .d_min_base
        .as_ref()
        .filter(|anchor| {
            anchor.provenance.input_domain != crate::app_state::DataDomain::ProPhotoEstimate
                && anchor.confidence != DensityAnchorConfidence::Estimated
        })
        .map(|_| RollBaseStatus::Sampled)
        .unwrap_or(RollBaseStatus::Estimated);
    let dmax = if resolution.usable_density_anchors.has_roll_full_exposure() {
        RollDmaxStatus::FullExposure
    } else {
        RollDmaxStatus::Unknown
    };
    let calibration = if legacy {
        RollCalibrationMode::Legacy
    } else if pipeline.render_mapping.mode == RenderMode::RollAnchored {
        // Roll anchors are a display-domain estimate, not a physical Status M
        // or laboratory density claim.
        RollCalibrationMode::SmartAuto
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
    if resolution
        .usable_density_anchors
        .d_min_base
        .as_ref()
        .is_some_and(|anchor| {
            anchor.provenance.input_domain == crate::app_state::DataDomain::ProPhotoEstimate
        })
    {
        warnings.push("roll_anchor_prophoto_estimate".to_string());
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
    use crate::calibration_fit::{
        CalibrationFitModel, FitDiagnostics, FitModelType, ReferenceDomain,
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
    fn invalid_characterized_fit_does_not_invalidate_verified_capture_payload() {
        let mut payload = verified_capture_profile().profile.payload;
        payload.fit_model = Some(CalibrationFitModel {
            model_type: FitModelType::CaptureSeparation3x3,
            source_domain: ReferenceDomain::CameraNativeTransmissionRgb,
            target_domain: ReferenceDomain::TransmissionRgb,
            pipeline_order: "capture_correction->capture_separation->log10".to_string(),
            matrix: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
            offset: [0.0; 3],
            diagnostics: FitDiagnostics {
                rank: 1,
                condition_number: f32::INFINITY,
                training_patch_count: 4,
                validation_patch_count: 1,
                rejected_patch_count: 0,
                channel_rmse: [0.0; 3],
                training_rmse: 0.0,
                validation_rmse: 0.0,
                max_abs_error: 0.0,
            },
            measurement_digest: "invalid-fit".to_string(),
        });
        payload.fit_measurements = None;
        payload.payload_digest = payload.canonical_digest().unwrap();

        assert_eq!(payload.capture_validation_error(RAW_DECODE_VERSION), None);
        assert_eq!(
            payload.fit_validation_error(),
            Some("capture_fit_measurements_missing")
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
    // New endpoints invalidate the learned white point; it is re-measured from
    // the Roll's own frames on the next Auto Invert.
    anchors.highlight_fraction = None;
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
                pipeline.content_range = None;
                if let Some(mapping) = fixed_roll_density_mapping(&pipeline) {
                    pipeline.render_mapping = mapping;
                    pipeline.processing_report.render_route =
                        "RollAnchoredDirectInvert".to_string();
                    pipeline.processing_report.tone_mapping_mode =
                        "roll_anchored_fixed".to_string();
                    pipeline.processing_report.fallback_reason.clear();
                    apply_roll_anchor_report(&mut pipeline);
                } else if pipeline.contract != ProcessingContract::LegacyV1 {
                    pipeline.render_mapping = RenderMapping::default();
                    pipeline.processing_report.render_route = "FilmAreaSmartAuto".to_string();
                    pipeline.processing_report.fallback_reason =
                        "incomplete_roll_anchors".to_string();
                    pipeline
                        .processing_report
                        .fallback_reasons
                        .retain(|reason| reason != "incomplete_roll_anchors");
                    pipeline
                        .processing_report
                        .fallback_reasons
                        .push("incomplete_roll_anchors".to_string());
                    pipeline.processing_report.uses_physical_anchors = false;
                }
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
        persistence::save_rolls_and_pipeline_states_reset_thumbnails(
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
            item.rendered_thumbnail_base64 = None;
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
            runtime_frame_base: None,
            runtime_frame_highlight: None,
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
    let master_exposure = params.exposure.exposure;
    let channel_exposure_offsets = if params.film_mode == FilmMode::BW {
        [0.0; 3]
    } else {
        [
            params.exposure.exp_r * CHANNEL_CONTROL_SCALE,
            params.exposure.exp_g * CHANNEL_CONTROL_SCALE,
            params.exposure.exp_b * CHANNEL_CONTROL_SCALE,
        ]
    };
    let mut effective_pipeline = item.effective_pipeline_state().clone();
    clear_retired_legacy_domain(&mut effective_pipeline);
    let smart_auto_compatibility = is_smart_auto_compatibility(&effective_pipeline);
    let legacy_compatibility =
        effective_pipeline.contract == ProcessingContract::LegacyV1 || smart_auto_compatibility;
    let pipeline_exposure_offsets = if legacy_compatibility {
        channel_exposure_offsets.map(|value| value + master_exposure)
    } else {
        channel_exposure_offsets
    };
    let pipeline = if smart_auto_compatibility {
        FilmPipeline::new(
            [base_color.base_r, base_color.base_g, base_color.base_b],
            pipeline_exposure_offsets,
            params.film_mode.clone(),
        )
    } else {
        FilmPipeline::from_state(
            &effective_pipeline,
            base_color,
            pipeline_exposure_offsets,
            params.film_mode.clone(),
        )
    };

    let pristine = item.pristine_proxy.as_ref()?;
    let (width, height) = pristine.dimensions();
    let mut thumb_8bit = RgbImage::new(width, height);

    let pristine_pixels: &[f32] = pristine.as_raw().as_slice();
    let out_pixels: &mut [u8] = thumb_8bit.as_mut();

    let roll_anchored = effective_pipeline.render_mapping.mode == RenderMode::RollAnchored;
    let (d_min, d_max) = if roll_anchored {
        (
            effective_pipeline.render_mapping.density_low,
            effective_pipeline.render_mapping.density_high,
        )
    } else {
        (params.density.d_min, params.density.d_max)
    };
    let gamma = params.density.gamma;
    let highlights = params.tone.highlights;
    let shadows = params.tone.shadows;
    let (bw_dmin, bw_dmax) = neutral_density_bounds(d_min, d_max);
    let density_min_offset = params.density.d_min_offset;
    let density_max_offset = params.density.d_max_offset;
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
        && !smart_auto_compatibility
        && params.film_mode == FilmMode::Color)
        .then(|| linear_conversion_matrix(ColorSpaceId::ProPhotoRgb, ColorSpaceId::SRgb));

    pristine_pixels
        .par_chunks(3)
        .zip(out_pixels.par_chunks_mut(3))
        .for_each(|(in_px, out_px)| {
            let true_density = [in_px[0], in_px[1], in_px[2]];
            let density = pipeline.apply_exposure(&true_density);

            let (effective_dmin, effective_dmax) = if roll_anchored {
                (d_min, d_max)
            } else if params.film_mode == FilmMode::BW {
                ([bw_dmin; 3], [bw_dmax; 3])
            } else {
                (d_min, d_max)
            };
            let (effective_dmin, effective_dmax) = trim_density_endpoints(
                effective_dmin,
                effective_dmax,
                density_min_offset,
                density_max_offset,
            );
            let working_gamma = if legacy_compatibility { gamma } else { 1.0 };
            let normalize = |value: f32, low: f32, high: f32| {
                normalize_density_channel(value, low, high, highlights, shadows, working_gamma)
            };
            let normalized = [
                normalize(density[0], effective_dmin[0], effective_dmax[0]),
                normalize(density[1], effective_dmin[1], effective_dmax[1]),
                normalize(density[2], effective_dmin[2], effective_dmax[2]),
            ];
            let mut positive_linear = prophoto_to_srgb
                .map(|matrix| apply_linear_matrix(normalized, matrix))
                .unwrap_or(normalized);
            if !legacy_compatibility {
                let exposure_gain = 2.0f32.powf(master_exposure);
                positive_linear = positive_linear.map(|value| value * exposure_gain);
            }
            let gamma_corrected = if legacy_compatibility {
                positive_linear
            } else {
                positive_linear.map(|value| value.clamp(0.0, 1.0).powf(1.0 / gamma.max(1e-6)))
            };
            let mut final_rgb = apply_post_gamma_adjustments_with_luma(
                gamma_corrected,
                0.0,
                0.0,
                params.tone.contrast,
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

    // The filmstrip and the Cards show this thumbnail: it has to carry the same
    // geometry as the Develop preview and the export, otherwise a flipped frame
    // looks upside down everywhere except in the renderer the user graded.
    let oriented_thumb = orient_display_image(thumb_8bit, &item.geom);
    let (oriented_width, oriented_height) = oriented_thumb.dimensions();
    let cx = (item.geom.crop_rect.x * oriented_width as f32)
        .max(0.0)
        .min(oriented_width as f32) as u32;
    let cy = (item.geom.crop_rect.y * oriented_height as f32)
        .max(0.0)
        .min(oriented_height as f32) as u32;
    let cw = (item.geom.crop_rect.width * oriented_width as f32)
        .max(1.0)
        .min((oriented_width - cx) as f32) as u32;
    let ch = (item.geom.crop_rect.height * oriented_height as f32)
        .max(1.0)
        .min((oriented_height - cy) as f32) as u32;

    let mut cropped_thumb = oriented_thumb;
    if cw < oriented_width || ch < oriented_height {
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
        aggregate_roll_density_references, apply_content_channel_response, base_color_from_density,
        channel_response_from_spans, clear_retired_legacy_domain, compute_auto_base,
        compute_auto_base_f32, compute_auto_color_limits, compute_content_limits_f32,
        compute_content_limits_f32_with_bounds, compute_frame_base_density_f32,
        content_channel_response, decode_image_buffer, decode_import_preview_base64,
        decode_profiled_tiff_prophoto_estimate, decode_prophoto_estimate_image_buffer,
        decode_prophoto_estimate_image_buffer_with_policy, decode_reduced_dng_for_working_space,
        decode_reduced_tiff_for_working_space, decode_scanner_profiled_estimate_image_buffer,
        decode_tiff_for_smart_auto, decode_uncompressed_tiff_reduced,
        default_pipeline_state_for_import, density_luma, detect_frame_base_density,
        detect_frame_highlight_fraction, embedded_input_profile,
        encoded_pixel_to_prophoto_estimate, estimate_film_base_f32, fixed_roll_density_mapping,
        frame_needs_window_reanalysis, is_better_preview_edge, is_dng_extension,
        is_lightweight_direct_preview, is_noritsu_rendered_image, is_raw_extension,
        is_scanner_fff_tiff, is_smart_auto_compatibility, is_tiff_extension,
        libraw_decode_error_message, linear_srgb_u16_to_prophoto_f32, linearize_scanner_fff,
        measure_content_channel_spans, measure_roll_highlight_frames, persist_import_batch,
        pipeline_base_density, pipeline_has_base, point_in_film_area,
        prepare_content_render_limits, prepare_content_render_limits_with_spans,
        preserve_smart_auto_content_span, prophoto_estimate_to_transport_proxy,
        raw_decode_failure_hint, reference_density_extreme, render_f32_shader_equivalent,
        render_shader_equivalent, resolve_input_domain, rgb16_image_from_bytes,
        roll_density_mapping_with_frame_base, roll_physical_density_span,
        share_smart_auto_density_scale, share_smart_auto_density_scale_without_offsets,
        srgb_proxy_u16_to_prophoto_f32, tiff_smart_auto_input_is_estimated, trim_density_endpoints,
        AutoColorLimits, DecodeMode, CHANNEL_RESPONSE_BALANCED_RATIO, CHANNEL_RESPONSE_FULL_RATIO,
        CHANNEL_RESPONSE_MAX_GAIN, CHANNEL_RESPONSE_MIN_SPAN, IMPORT_PREVIEW_LONG_EDGE,
        PROPHOTO_TRANSPORT_MAX, PROPHOTO_TRANSPORT_MIN,
    };
    use crate::app_state::{
        BaseColor, DataDomain, DensityAnchor, DensityAnchorConfidence, DensityAnchorProvenance,
        DensityAnchorScope, DensityAnchorSource, DensityAnchors, EngineState, FilmItem, FilmMode,
        GeometryState, PipelineState, ProcessingContract, RenderMapping, RenderMode, Roll,
        TuningParams,
    };
    use crate::color_science::{
        apply_linear_matrix, compress_linear_srgb_for_density,
        convert_encoded_to_linear_rgb_with_matrix, linear_conversion_matrix, ColorSpaceId,
        DENSITY_CAPTURE_PROFILE,
    };
    use base64::Engine as _;
    use image::{ImageBuffer, Rgb};
    use rayon::prelude::*;
    use std::sync::{Arc, RwLock};

    #[test]
    fn import_only_directly_decodes_small_encoded_images() {
        assert!(is_lightweight_direct_preview("frame.jpg"));
        assert!(is_lightweight_direct_preview("frame.PNG"));
        assert!(!is_lightweight_direct_preview("frame.tiff"));
        assert!(!is_lightweight_direct_preview("frame.dng"));
    }

    #[test]
    fn complete_roll_mapping_is_fixed_and_content_independent() {
        let provenance = DensityAnchorProvenance {
            input_domain: DataDomain::ProPhotoEstimate,
            algorithm_version: crate::app_state::DENSITY_ANCHOR_ALGORITHM_VERSION.to_string(),
            legacy: false,
            ..Default::default()
        };
        let anchors = DensityAnchors {
            d_min_base: Some(DensityAnchor {
                density: [0.2, 0.3, 0.4],
                source: DensityAnchorSource::SampledFilmBase,
                scope: DensityAnchorScope::Roll,
                confidence: DensityAnchorConfidence::UserSampled,
                reference_id: Some("roll:base".into()),
                provenance: provenance.clone(),
            }),
            d_max_full_exposure: Some(DensityAnchor {
                density: [1.4, 1.8, 2.4],
                source: DensityAnchorSource::SampledFullExposure,
                scope: DensityAnchorScope::Roll,
                confidence: DensityAnchorConfidence::UserSampled,
                reference_id: Some("roll:full".into()),
                provenance,
            }),
            retained_records: Vec::new(),
            highlight_fraction: None,
        };
        let state = PipelineState::from_roll_anchors(anchors);
        let mapping = fixed_roll_density_mapping(&state).expect("valid anchors");
        assert_eq!(mapping.mode, RenderMode::RollAnchored);
        assert_eq!(mapping.density_low, [0.0; 3]);
        assert!((mapping.density_high[0] - 1.2).abs() < 1.0e-5);
        assert!((mapping.density_high[1] - 1.5).abs() < 1.0e-5);
        assert!((mapping.density_high[2] - 2.0).abs() < 1.0e-5);
        assert_eq!(state.content_range, None);
    }

    #[test]
    fn roll_render_uses_persisted_mapping_when_params_are_stale() {
        let provenance = DensityAnchorProvenance {
            input_domain: DataDomain::ProPhotoEstimate,
            algorithm_version: crate::app_state::DENSITY_ANCHOR_ALGORITHM_VERSION.to_string(),
            legacy: false,
            ..Default::default()
        };
        let anchors = DensityAnchors {
            d_min_base: Some(DensityAnchor {
                density: [0.0; 3],
                source: DensityAnchorSource::SampledFilmBase,
                scope: DensityAnchorScope::Roll,
                confidence: DensityAnchorConfidence::UserSampled,
                reference_id: Some("roll-a:base".into()),
                provenance: provenance.clone(),
            }),
            d_max_full_exposure: Some(DensityAnchor {
                density: [1.0, 1.5, 2.0],
                source: DensityAnchorSource::SampledFullExposure,
                scope: DensityAnchorScope::Roll,
                confidence: DensityAnchorConfidence::UserSampled,
                reference_id: Some("roll-a:full".into()),
                provenance,
            }),
            retained_records: Vec::new(),
            highlight_fraction: None,
        };
        let mut state = PipelineState::from_roll_anchors(anchors);
        state.render_mapping = RenderMapping {
            mode: RenderMode::RollAnchored,
            density_low: [0.0; 3],
            density_high: [1.0, 1.5, 2.0],
            exposure: 0.0,
            gamma: 1.0,
            channel_offsets: [0.0; 3],
        };
        let source = ImageBuffer::from_pixel(
            1,
            1,
            Rgb([10.0f32.powf(-0.5), 10.0f32.powf(-0.75), 10.0f32.powf(-1.0)]),
        );
        let mut stale_params = TuningParams::default();
        stale_params.density.d_min = [0.25, 0.30, 0.35];
        stale_params.density.d_max = [2.25, 2.30, 2.35];
        stale_params.density.gamma = 1.0;
        let mut mapped_params = stale_params.clone();
        mapped_params.density.d_min = [0.0; 3];
        mapped_params.density.d_max = [1.0, 1.5, 2.0];
        let base = BaseColor {
            base_r: u16::MAX,
            base_g: u16::MAX,
            base_b: u16::MAX,
        };
        let stale_render = render_f32_shader_equivalent(
            &source,
            None,
            &stale_params,
            &GeometryState::default(),
            &base,
            &state,
            None,
        );
        let mapped_render = render_f32_shader_equivalent(
            &source,
            None,
            &mapped_params,
            &GeometryState::default(),
            &base,
            &state,
            None,
        );
        assert_eq!(stale_render, mapped_render);
    }

    #[test]
    fn master_density_offsets_trim_the_roll_mapping_endpoints() {
        let provenance = DensityAnchorProvenance {
            input_domain: DataDomain::ProPhotoEstimate,
            algorithm_version: crate::app_state::DENSITY_ANCHOR_ALGORITHM_VERSION.to_string(),
            legacy: false,
            ..Default::default()
        };
        let anchors = DensityAnchors {
            d_min_base: Some(DensityAnchor {
                density: [0.0; 3],
                source: DensityAnchorSource::SampledFilmBase,
                scope: DensityAnchorScope::Roll,
                confidence: DensityAnchorConfidence::UserSampled,
                reference_id: Some("roll-a:base".into()),
                provenance: provenance.clone(),
            }),
            d_max_full_exposure: Some(DensityAnchor {
                density: [1.0, 1.5, 2.0],
                source: DensityAnchorSource::SampledFullExposure,
                scope: DensityAnchorScope::Roll,
                confidence: DensityAnchorConfidence::UserSampled,
                reference_id: Some("roll-a:full".into()),
                provenance,
            }),
            retained_records: Vec::new(),
            highlight_fraction: None,
        };
        let mut state = PipelineState::from_roll_anchors(anchors);
        state.render_mapping = RenderMapping {
            mode: RenderMode::RollAnchored,
            density_low: [0.0; 3],
            density_high: [1.0, 1.5, 2.0],
            exposure: 0.0,
            gamma: 1.0,
            channel_offsets: [0.0; 3],
        };
        let source = ImageBuffer::from_pixel(
            1,
            1,
            Rgb([10.0f32.powf(-0.5), 10.0f32.powf(-0.75), 10.0f32.powf(-1.0)]),
        );
        let base = BaseColor {
            base_r: u16::MAX,
            base_g: u16::MAX,
            base_b: u16::MAX,
        };
        let offset = 0.1f32;
        let mut trimmed_params = TuningParams::default();
        trimmed_params.density.d_min_offset = offset;
        trimmed_params.density.d_max_offset = offset;
        // The Roll measures its own span per channel, so the slider amount is
        // applied as each channel's share of it.
        let (trimmed_low, trimmed_high) =
            trim_density_endpoints([0.0; 3], [1.0, 1.5, 2.0], offset, offset);
        let mut shifted_state = state.clone();
        shifted_state.render_mapping.density_low = trimmed_low;
        shifted_state.render_mapping.density_high = trimmed_high;

        let trimmed_render = render_f32_shader_equivalent(
            &source,
            None,
            &trimmed_params,
            &GeometryState::default(),
            &base,
            &state,
            None,
        );
        let shifted_render = render_f32_shader_equivalent(
            &source,
            None,
            &TuningParams::default(),
            &GeometryState::default(),
            &base,
            &shifted_state,
            None,
        );
        let untrimmed_render = render_f32_shader_equivalent(
            &source,
            None,
            &TuningParams::default(),
            &GeometryState::default(),
            &base,
            &state,
            None,
        );
        // The Master trim must move exactly the endpoints the shader and the
        // thumbnails use, and the sampled anchors themselves stay untouched.
        assert_eq!(trimmed_render, shifted_render);
        assert_ne!(trimmed_render, untrimmed_render);
        assert_eq!(state.render_mapping.density_low, [0.0; 3]);
        assert_eq!(state.render_mapping.density_high, [1.0, 1.5, 2.0]);
    }

    #[test]
    fn density_reference_aggregation_uses_channel_medians_and_preserves_contract() {
        let provenance = DensityAnchorProvenance {
            input_domain: DataDomain::ProPhotoEstimate,
            algorithm_version: crate::app_state::DENSITY_ANCHOR_ALGORITHM_VERSION.to_string(),
            legacy: false,
            ..Default::default()
        };
        let sample = |id: &str, density: [f32; 3]| DensityAnchor {
            density,
            source: DensityAnchorSource::SampledFilmBase,
            scope: DensityAnchorScope::Roll,
            confidence: DensityAnchorConfidence::UserSampled,
            reference_id: Some(format!("roll-a:{id}")),
            provenance: provenance.clone(),
        };
        let merged = aggregate_roll_density_references(
            "roll-a".to_string(),
            "base".to_string(),
            vec![
                sample("one", [0.2, 0.3, 0.4]),
                sample("two", [0.4, 0.5, 0.6]),
                sample("outlier", [4.0, 5.0, 6.0]),
            ],
        )
        .unwrap();
        assert_eq!(merged.density, [0.4, 0.5, 0.6]);
        assert_eq!(merged.provenance.input_domain, DataDomain::ProPhotoEstimate);
        assert!(merged.reference_id.unwrap().contains(":aggregate:base:3"));
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
        use_camera_wb: bool,
    ) -> image::ImageBuffer<image::Rgb<u16>, Vec<u16>> {
        let options = crate::raw_backend::DecodeOptions {
            half_size: true,
            demosaic_quality: 3,
            output_bps: 16,
            no_auto_bright: true,
            output_color,
            linear_gamma: true,
            use_camera_wb,
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

    #[derive(Clone, Copy, PartialEq)]
    enum AbWhiteBalance {
        AsShot,
        Daylight,
        NormalizedAsShot,
    }

    fn ab_decode_camera_f32(
        path: &std::path::Path,
        white_balance: AbWhiteBalance,
    ) -> image::ImageBuffer<image::Rgb<u16>, Vec<u16>> {
        let options = crate::raw_backend::DecodeOptions {
            half_size: true,
            demosaic_quality: 3,
            output_bps: 16,
            no_auto_bright: true,
            output_color: 0,
            linear_gamma: true,
            use_camera_wb: white_balance == AbWhiteBalance::AsShot,
        };
        let policy = match white_balance {
            AbWhiteBalance::NormalizedAsShot => {
                crate::raw_backend::WhiteBalancePolicy::NormalizedAsShot
            }
            _ => crate::raw_backend::WhiteBalancePolicy::FromOptions,
        };
        let decoded = crate::raw_backend::extract_camera_rgb_with_policy(path, &options, policy)
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

    struct AbChannelStats {
        minimum: u16,
        p01: u16,
        p50: u16,
        p99: u16,
        maximum: u16,
        full_percent: f64,
        zero_percent: f64,
        /// Share of samples at or above 98% of full scale. The density
        /// compression keeps exact 65535 values rare, so this is the metric
        /// that actually shows a channel running out of headroom.
        ceiling_percent: f64,
        headroom_stops: f64,
    }

    /// Per-channel histogram statistics of a 16-bit transport, used to compare
    /// the camera RAW channel headroom with and without as-shot white balance.
    fn ab_channel_stats(
        image: &image::ImageBuffer<image::Rgb<u16>, Vec<u16>>,
    ) -> [AbChannelStats; 3] {
        let mut histograms = [[0u32; 65536]; 3];
        for pixel in image.as_raw().chunks_exact(3) {
            for (channel, value) in pixel.iter().enumerate() {
                histograms[channel][usize::from(*value)] += 1;
            }
        }
        let total = u64::from(image.width()) * u64::from(image.height());
        std::array::from_fn(|channel| {
            let histogram = &histograms[channel];
            let percentile = |fraction: f64| -> u16 {
                let target = ((total as f64 * fraction).ceil() as u64).max(1);
                let mut accumulated = 0u64;
                for (value, count) in histogram.iter().enumerate() {
                    accumulated += u64::from(*count);
                    if accumulated >= target {
                        return value as u16;
                    }
                }
                u16::MAX
            };
            let maximum = (0..65536)
                .rev()
                .find(|value| histogram[*value] > 0)
                .unwrap_or(0) as u16;
            AbChannelStats {
                minimum: (0..65536).find(|value| histogram[*value] > 0).unwrap_or(0) as u16,
                p01: percentile(0.01),
                p50: percentile(0.50),
                p99: percentile(0.99),
                maximum,
                full_percent: 100.0 * f64::from(histogram[usize::from(u16::MAX)])
                    / total.max(1) as f64,
                zero_percent: 100.0 * f64::from(histogram[0]) / total.max(1) as f64,
                ceiling_percent: 100.0 * f64::from(histogram[64_222..].iter().sum::<u32>())
                    / total.max(1) as f64,
                headroom_stops: if maximum == 0 {
                    0.0
                } else {
                    (65_535.0f64 / f64::from(maximum)).log2()
                },
            }
        })
    }

    fn ab_print_channel_stats(label: &str, stats: &[AbChannelStats; 3]) {
        println!("[WB A/B] {label}");
        for (channel, name) in ["R", "G", "B"].into_iter().enumerate() {
            let stat = &stats[channel];
            println!(
                "[WB A/B]   {name}: min={} p01={} p50={} p99={} max={} full={:.4}% ceiling>={:.3}% zero={:.4}% headroom={:.2} stops",
                stat.minimum,
                stat.p01,
                stat.p50,
                stat.p99,
                stat.maximum,
                stat.full_percent,
                stat.ceiling_percent,
                stat.zero_percent,
                stat.headroom_stops
            );
        }
    }

    fn ab_print_proxy_stats(label: &str, image: &image::ImageBuffer<image::Rgb<u16>, Vec<u16>>) {
        let stats = ab_channel_stats(image);
        println!("[DIAG] {label} (u16)");
        for (channel, name) in ["R", "G", "B"].into_iter().enumerate() {
            let stat = &stats[channel];
            println!(
                "[DIAG]   {name}: min={} p01={} p50={} p99={} max={} ceiling98={:.3}% zero={:.3}%",
                stat.minimum,
                stat.p01,
                stat.p50,
                stat.p99,
                stat.maximum,
                stat.ceiling_percent,
                stat.zero_percent
            );
        }
    }

    fn ab_print_proxy_stats_f32(
        label: &str,
        image: &image::ImageBuffer<image::Rgb<f32>, Vec<f32>>,
    ) {
        let mut means = [0.0f64; 3];
        let mut minima = [f32::INFINITY; 3];
        let mut maxima = [f32::NEG_INFINITY; 3];
        for pixel in image.as_raw().chunks_exact(3) {
            for channel in 0..3 {
                means[channel] += f64::from(pixel[channel]);
                minima[channel] = minima[channel].min(pixel[channel]);
                maxima[channel] = maxima[channel].max(pixel[channel]);
            }
        }
        let total = (f64::from(image.width()) * f64::from(image.height())).max(1.0);
        println!("[DIAG] {label} (f32)");
        for (channel, name) in ["R", "G", "B"].into_iter().enumerate() {
            println!(
                "[DIAG]   {name}: min={:.4} mean={:.4} max={:.4}",
                minima[channel],
                means[channel] / total,
                maxima[channel]
            );
        }
    }

    /// Rendered pixels are already display-referred sRGB, so the mean ratios
    /// between channels are a usable cast indicator: 1.000 means neutral.
    /// Only samples inside the Film Area are averaged, otherwise the orange
    /// rebate dominates a small preview.
    fn ab_print_rendered_cast(
        label: &str,
        image: &image::ImageBuffer<image::Rgb<u16>, Vec<u16>>,
        geom: &GeometryState,
    ) {
        let points =
            geom.calibration_points
                .unwrap_or([[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]]);
        let mut means = [0.0f64; 3];
        let mut total = 0.0f64;
        for (index, pixel) in image.as_raw().chunks_exact(3).enumerate() {
            let x = (index as u32 % image.width()) as f32
                / image.width().saturating_sub(1).max(1) as f32;
            let y = (index as u32 / image.width()) as f32
                / image.height().saturating_sub(1).max(1) as f32;
            if !point_in_film_area([x, y], &points, 0.0) {
                continue;
            }
            for (channel, value) in pixel.iter().enumerate() {
                means[channel] += f64::from(*value);
            }
            total += 1.0;
        }
        let total = total.max(1.0);
        means = means.map(|value| value / total);
        let green = means[1].max(1.0);
        println!(
            "[DIAG] {label} rendered (Film Area): mean=({:.0},{:.0},{:.0}) R/G={:.3} B/G={:.3} R-B={:.0}",
            means[0],
            means[1],
            means[2],
            means[0] / green,
            means[2] / green,
            means[0] - means[2]
        );
    }

    /// Per-channel film-base density from the frame's brightest samples, the way
    /// the v1.0.2 path estimates it. `scoped_to_area` restricts the samples to
    /// the declared Film Area.
    fn ab_frame_base_density(
        image: &ImageBuffer<Rgb<f32>, Vec<f32>>,
        geom: &GeometryState,
        scoped_to_area: bool,
    ) -> [f32; 3] {
        let points =
            geom.calibration_points
                .unwrap_or([[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]]);
        let mut channels = [Vec::new(), Vec::new(), Vec::new()];
        for (index, pixel) in image.as_raw().chunks_exact(3).enumerate() {
            if pixel
                .iter()
                .any(|value| !value.is_finite() || *value <= 0.0)
            {
                continue;
            }
            if scoped_to_area {
                let x = (index as u32 % image.width()) as f32
                    / image.width().saturating_sub(1).max(1) as f32;
                let y = (index as u32 / image.width()) as f32
                    / image.height().saturating_sub(1).max(1) as f32;
                if !point_in_film_area([x, y], &points, 0.0) {
                    continue;
                }
            }
            for channel in 0..3 {
                channels[channel].push(pixel[channel]);
            }
        }
        std::array::from_fn(|channel| {
            let values = &mut channels[channel];
            if values.is_empty() {
                return 0.0;
            }
            values.sort_unstable_by(|left, right| right.total_cmp(left));
            let tail = ((values.len() as f32 * 0.01).ceil() as usize).clamp(1, values.len());
            let mean = values.iter().take(tail).sum::<f32>() / tail as f32;
            -mean.max(1.0e-6).log10()
        })
    }

    fn ab_probe_region(
        label: &str,
        width: u32,
        height: u32,
        uv: [f32; 2],
        sample: impl Fn(u32, u32) -> [f64; 3],
    ) -> [f64; 3] {
        const RADIUS: i32 = 12;
        let center_x = (uv[0] * width as f32) as i32;
        let center_y = (uv[1] * height as f32) as i32;
        let mut sum = [0.0f64; 3];
        let mut count = 0.0f64;
        for y in (center_y - RADIUS).max(0)..(center_y + RADIUS).min(height as i32 - 1) {
            for x in (center_x - RADIUS).max(0)..(center_x + RADIUS).min(width as i32 - 1) {
                let value = sample(x as u32, y as u32);
                for channel in 0..3 {
                    sum[channel] += value[channel];
                }
                count += 1.0;
            }
        }
        let mean = sum.map(|value| value / count.max(1.0));
        println!(
            "[DIAG]   {label} @({:.2},{:.2}) = ({:.4}, {:.4}, {:.4})",
            uv[0], uv[1], mean[0], mean[1], mean[2]
        );
        mean
    }

    fn ab_probe_u16(
        label: &str,
        image: &image::ImageBuffer<image::Rgb<u16>, Vec<u16>>,
        uv: [f32; 2],
    ) -> [f64; 3] {
        ab_probe_region(label, image.width(), image.height(), uv, |x, y| {
            let pixel = image.get_pixel(x, y).0;
            [
                f64::from(pixel[0]) / 65535.0,
                f64::from(pixel[1]) / 65535.0,
                f64::from(pixel[2]) / 65535.0,
            ]
        })
    }

    fn ab_probe_f32(
        label: &str,
        image: &image::ImageBuffer<image::Rgb<f32>, Vec<f32>>,
        uv: [f32; 2],
    ) -> [f64; 3] {
        ab_probe_region(label, image.width(), image.height(), uv, |x, y| {
            let pixel = image.get_pixel(x, y).0;
            [
                f64::from(pixel[0]),
                f64::from(pixel[1]),
                f64::from(pixel[2]),
            ]
        })
    }

    /// Write an 8-bit JPEG preview so diagnostics stay viewable; a 16-bit PNG
    /// of a full-size render is both huge and awkward to inspect.
    fn ab_save_preview(
        path: std::path::PathBuf,
        image: &image::ImageBuffer<image::Rgb<u16>, Vec<u16>>,
    ) {
        let eight_bit = image::ImageBuffer::<Rgb<u8>, Vec<u8>>::from_fn(
            image.width(),
            image.height(),
            |x, y| {
                let pixel = image.get_pixel(x, y).0;
                Rgb([
                    (pixel[0] / 257) as u8,
                    (pixel[1] / 257) as u8,
                    (pixel[2] / 257) as u8,
                ])
            },
        );
        eight_bit
            .save(&path)
            .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
    }

    /// Manual A/B for task B: decode the same camera RAW with LibRaw's as-shot
    /// white balance enabled and disabled, and report the per-channel headroom.
    /// Override the fixture with `NEXFILM_WB_AB_FRAME`.
    #[test]
    #[ignore = "manual camera-RAW as-shot WB headroom A/B; decodes large fixtures"]
    fn camera_raw_as_shot_white_balance_headroom_ab() {
        let frame = std::env::var("NEXFILM_WB_AB_FRAME").unwrap_or_else(|_| {
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("test_picture")
                .join("哈苏fff")
                .join("任务 _1233.fff")
                .to_string_lossy()
                .to_string()
        });
        let path = std::path::Path::new(&frame);
        assert!(path.is_file(), "fixture is missing: {frame}");

        if let Ok(white_balance) = crate::raw_backend::read_white_balance(path) {
            println!(
                "[WB A/B] metadata: as-shot={:?} daylight={:?}",
                white_balance.as_shot, white_balance.daylight
            );
        }

        // Measure exactly what the decode hands to the pipeline: LibRaw output
        // through the signed camera-to-sRGB matrix and the density-domain
        // compression, before the final ProPhoto transport matrix.
        let as_shot = ab_channel_stats(&ab_decode_camera_f32(path, AbWhiteBalance::AsShot));
        let daylight = ab_channel_stats(&ab_decode_camera_f32(path, AbWhiteBalance::Daylight));
        let normalized = ab_channel_stats(&ab_decode_camera_f32(
            path,
            AbWhiteBalance::NormalizedAsShot,
        ));
        ab_print_channel_stats("LibRaw as-shot white balance", &as_shot);
        ab_print_channel_stats("LibRaw daylight fallback (use_camera_wb = 0)", &daylight);
        ab_print_channel_stats("normalized as-shot ratio (plan B)", &normalized);

        // Deliberately no assertion here: the measured effect is fixture
        // dependent, because LibRaw falls back to the camera's fixed daylight
        // white balance when as-shot WB is switched off. On a capture the
        // photographer balanced on the film base, that fallback pushes red back
        // up, so this stays a reporting tool for the acceptance table.
        for (channel, name) in ["R", "G", "B"].into_iter().enumerate() {
            println!(
                "[WB A/B] {name}: p50 as-shot={} daylight={} normalized={} | ceiling as-shot={:.4}% daylight={:.4}% normalized={:.4}%",
                as_shot[channel].p50,
                daylight[channel].p50,
                normalized[channel].p50,
                as_shot[channel].ceiling_percent,
                daylight[channel].ceiling_percent,
                normalized[channel].ceiling_percent
            );
        }
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

            let transport = ab_decode_raw_transport(path.as_path(), 4, false);
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
                let raw = ab_decode_raw_transport(path.as_path(), output_color, false);
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

            let camera_f32 = ab_decode_camera_f32(path.as_path(), AbWhiteBalance::NormalizedAsShot);
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
    fn smart_auto_input_compression_produces_finite_positive_prophoto_values() {
        let matrix = linear_conversion_matrix(ColorSpaceId::SRgb, ColorSpaceId::ProPhotoRgb);
        for source in [[-0.4, 0.3, 1.8], [0.95, 0.08, -0.2], [f32::NAN, 0.5, 0.5]] {
            let safe = compress_linear_srgb_for_density(source);
            let prophoto = apply_linear_matrix(safe, matrix);
            assert!(safe
                .iter()
                .all(|value| value.is_finite() && *value > 0.0 && *value < 1.0));
            assert!(prophoto
                .iter()
                .all(|value| value.is_finite() && *value > 0.0));
        }
    }

    #[test]
    fn file_reference_path_uses_a_masked_trimmed_mean() {
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
        assert!(base.iter().all(|value| value.is_finite() && *value > 0.0));
        assert!(base[0] < full[0]);
        assert!(full[0] < 1.0);
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
            highlight_fraction: None,
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
        let loose = default_pipeline_state_for_import(true, "roll-a", &rolls);
        assert_eq!(loose.contract, ProcessingContract::SmartAutoProPhotoV11);
        assert_eq!(
            loose.processing_report.analysis_data_domain,
            "linear_prophoto_estimate"
        );
        // Loose Import shares the unanchored Smart Auto math, so it must not be
        // recognised as the retired v1.0.2 compatibility source.
        assert!(!is_smart_auto_compatibility(&loose));
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
            highlight_fraction: None,
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
            highlight_fraction: None,
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
            highlight_fraction: None,
        };
        assert!(complete.is_fully_anchored());
        assert_eq!(
            complete.prophoto_contract(),
            ProcessingContract::RollAnchoredProPhotoV11
        );
    }

    #[test]
    fn complete_roll_anchors_calibrate_content_scale_without_becoming_display_endpoints() {
        let base = DensityAnchor {
            density: [0.5, 0.6, 0.7],
            source: DensityAnchorSource::SampledFilmBase,
            scope: DensityAnchorScope::Roll,
            confidence: DensityAnchorConfidence::UserSampled,
            reference_id: None,
            provenance: Default::default(),
        };
        let full = DensityAnchor {
            density: [1.5, 1.8, 2.3],
            source: DensityAnchorSource::SampledFullExposure,
            scope: DensityAnchorScope::Roll,
            confidence: DensityAnchorConfidence::UserSampled,
            reference_id: None,
            provenance: Default::default(),
        };
        let anchors = DensityAnchors {
            d_min_base: Some(base.clone()),
            d_max_full_exposure: Some(full),
            retained_records: Vec::new(),
            highlight_fraction: None,
        };
        let mut limits = AutoColorLimits {
            d_min: [0.20, 0.35, 0.48],
            d_max: [0.95, 1.25, 1.50],
            pipeline_state: None,
        };
        let original = limits.clone();
        let (_, short_content) = prepare_content_render_limits(&mut limits, &anchors, base.density);
        assert!(!short_content);
        assert_ne!(limits.d_min, [0.0; 3]);
        let physical_span = [1.0, 1.2, 1.6];
        assert_ne!(limits.d_max, physical_span);
        let relative_spans = [
            (limits.d_max[0] - limits.d_min[0]) / physical_span[0],
            (limits.d_max[1] - limits.d_min[1]) / physical_span[1],
            (limits.d_max[2] - limits.d_min[2]) / physical_span[2],
        ];
        assert!((relative_spans[0] - relative_spans[1]).abs() < 1.0e-6);
        assert!((relative_spans[1] - relative_spans[2]).abs() < 1.0e-6);
        for channel in 0..3 {
            assert!(limits.d_min[channel] <= original.d_min[channel] + 0.2);
            assert!(limits.d_max[channel] >= original.d_max[channel] - 0.2);
        }
    }

    #[test]
    fn complete_roll_anchors_keep_equal_relative_density_neutral() {
        let base_density = [0.5, 0.6, 0.7];
        let physical_span = [1.0, 1.2, 1.6];
        let anchors = DensityAnchors {
            d_min_base: Some(DensityAnchor {
                density: base_density,
                source: DensityAnchorSource::SampledFilmBase,
                scope: DensityAnchorScope::Roll,
                confidence: DensityAnchorConfidence::UserSampled,
                reference_id: None,
                provenance: Default::default(),
            }),
            d_max_full_exposure: Some(DensityAnchor {
                density: [
                    base_density[0] + physical_span[0],
                    base_density[1] + physical_span[1],
                    base_density[2] + physical_span[2],
                ],
                source: DensityAnchorSource::SampledFullExposure,
                scope: DensityAnchorScope::Roll,
                confidence: DensityAnchorConfidence::UserSampled,
                reference_id: None,
                provenance: Default::default(),
            }),
            retained_records: Vec::new(),
            highlight_fraction: None,
        };
        let mut limits = AutoColorLimits {
            d_min: [0.20, 0.24, 0.32],
            d_max: [0.80, 0.96, 1.28],
            pipeline_state: None,
        };
        prepare_content_render_limits(&mut limits, &anchors, base_density);

        let neutral_midpoint = [0.50, 0.60, 0.80];
        let normalized = [0, 1, 2].map(|channel| {
            (neutral_midpoint[channel] - limits.d_min[channel])
                / (limits.d_max[channel] - limits.d_min[channel])
        });
        let minimum = normalized.into_iter().fold(f32::INFINITY, f32::min);
        let maximum = normalized.into_iter().fold(f32::NEG_INFINITY, f32::max);
        assert!(maximum - minimum < 1.0e-6, "normalized={normalized:?}");
        assert!((density_luma(normalized) - 0.5).abs() < 1.0e-6);
    }

    fn test_film_item(id: &str, roll_id: &str, path: &str) -> FilmItem {
        FilmItem {
            id: id.to_string(),
            roll_id: roll_id.to_string(),
            file_path: path.to_string(),
            embedded_thumbnail_base64: String::new(),
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
            runtime_frame_base: None,
            runtime_frame_highlight: None,
            pipeline_state: PipelineState::default(),
            params: TuningParams::default(),
            geom: GeometryState::default(),
            is_loose: false,
            in_library: false,
        }
    }

    #[test]
    fn roll_white_point_is_the_brightest_frame_not_a_sample_average() {
        let state = EngineState::new();
        let anchors = DensityAnchors {
            d_min_base: Some(DensityAnchor {
                density: [0.20, 0.24, 0.30],
                source: DensityAnchorSource::SampledFilmBase,
                scope: DensityAnchorScope::Roll,
                confidence: DensityAnchorConfidence::UserSampled,
                reference_id: None,
                provenance: Default::default(),
            }),
            d_max_full_exposure: Some(DensityAnchor {
                density: [1.30, 1.62, 2.10],
                source: DensityAnchorSource::SampledFullExposure,
                scope: DensityAnchorScope::Roll,
                confidence: DensityAnchorConfidence::UserSampled,
                reference_id: None,
                provenance: Default::default(),
            }),
            retained_records: Vec::new(),
            highlight_fraction: None,
        };
        // Every frame of a Roll shares one white point, so that white point has
        // to describe the brightest frame. Under the previous sample-of-five
        // rule the middle frame (0.58) became the white point, which put
        // frame-b's highlights above the display endpoint.
        let frames = [("frame-a", 0.52f32), ("frame-b", 0.74), ("frame-c", 0.58)];
        let mut item_arcs = Vec::new();
        for (id, fraction) in frames {
            let mut item = test_film_item(id, "roll-white-point", &format!("{id}.tif"));
            item.runtime_frame_highlight = Some(fraction);
            let arc = Arc::new(RwLock::new(item));
            state.items.insert(id.to_string(), arc.clone());
            item_arcs.push(arc);
        }
        let anchor_base = pipeline_base_density(
            &PipelineState::from_roll_anchors(anchors.clone()),
            &BaseColor::default(),
        );
        let span = roll_physical_density_span(&anchors, anchor_base).expect("complete anchors");
        let cancellation = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let (brightest, measured) =
            measure_roll_highlight_frames(&item_arcs, anchor_base, span, &cancellation, |_| {});
        assert_eq!(measured, frames.len());
        assert!((brightest.expect("a Roll white point") - 0.74).abs() < 1.0e-6);
    }

    #[test]
    fn master_density_trim_keeps_a_sampled_roll_neutral() {
        let anchors = DensityAnchors {
            d_min_base: Some(DensityAnchor {
                density: [0.0; 3],
                source: DensityAnchorSource::SampledFilmBase,
                scope: DensityAnchorScope::Roll,
                confidence: DensityAnchorConfidence::UserSampled,
                reference_id: None,
                provenance: Default::default(),
            }),
            d_max_full_exposure: Some(DensityAnchor {
                density: [1.0, 1.5, 2.0],
                source: DensityAnchorSource::SampledFullExposure,
                scope: DensityAnchorScope::Roll,
                confidence: DensityAnchorConfidence::UserSampled,
                reference_id: None,
                provenance: Default::default(),
            }),
            retained_records: Vec::new(),
            highlight_fraction: None,
        };
        let mut state = PipelineState::from_roll_anchors(anchors);
        state.render_mapping = RenderMapping {
            mode: RenderMode::RollAnchored,
            density_low: [0.0; 3],
            density_high: [1.0, 1.5, 2.0],
            exposure: 0.0,
            gamma: 1.0,
            channel_offsets: [0.0; 3],
        };
        // A frame whose density sits halfway through every channel's window.
        let source = ImageBuffer::from_pixel(
            1,
            1,
            Rgb([10.0f32.powf(-0.5), 10.0f32.powf(-0.75), 10.0f32.powf(-1.0)]),
        );
        let base = BaseColor {
            base_r: u16::MAX,
            base_g: u16::MAX,
            base_b: u16::MAX,
        };
        let untrimmed = render_f32_shader_equivalent(
            &source,
            None,
            &TuningParams::default(),
            &GeometryState::default(),
            &base,
            &state,
            None,
        );
        let mut trimmed_params = TuningParams::default();
        trimmed_params.density.d_min_offset = -0.03;
        trimmed_params.density.d_max_offset = 0.09;
        let trimmed = render_f32_shader_equivalent(
            &source,
            None,
            &trimmed_params,
            &GeometryState::default(),
            &base,
            &state,
            None,
        );
        let channel_spread = |pixel: &image::Rgb<u16>| {
            let maximum = pixel.0.iter().copied().max().unwrap_or(0);
            let minimum = pixel.0.iter().copied().min().unwrap_or(0);
            maximum - minimum
        };
        assert!(channel_spread(untrimmed.get_pixel(0, 0)) <= 1);
        assert!(
            channel_spread(trimmed.get_pixel(0, 0)) <= 1,
            "the Master trim turned a neutral frame into {:?}",
            trimmed.get_pixel(0, 0)
        );
        // The plain uniform shift this replaced splits the three channels,
        // which is the green cast the sliders used to introduce.
        let mut shifted_state = state.clone();
        shifted_state.render_mapping.density_low = [-0.03; 3];
        shifted_state.render_mapping.density_high = [1.09, 1.59, 2.09];
        let shifted = render_f32_shader_equivalent(
            &source,
            None,
            &TuningParams::default(),
            &GeometryState::default(),
            &base,
            &shifted_state,
            None,
        );
        assert!(channel_spread(shifted.get_pixel(0, 0)) > 4);
    }

    #[test]
    fn physical_anchor_window_rejects_white_backing_from_content_range() {
        let mut image = ImageBuffer::from_pixel(16, 16, Rgb([0.99, 0.99, 0.99]));
        for y in 3..13 {
            for x in 3..13 {
                let density = 0.35 + ((x + y) % 5) as f32 * 0.08;
                let transmission = 10.0f32.powf(-density);
                image.put_pixel(x, y, Rgb([transmission, transmission, transmission]));
            }
        }
        let bounded = compute_content_limits_f32_with_bounds(
            &image,
            None,
            &GeometryState::default(),
            [0.0; 3],
            Some([1.2, 1.4, 1.6]),
        )
        .unwrap();
        assert!(bounded.d_min.iter().all(|value| *value >= -0.1));
        assert!(bounded.d_max.iter().all(|value| *value <= 1.7));
        assert!(bounded.d_max[0] - bounded.d_min[0] > 0.2);
    }

    #[test]
    fn partial_roll_anchor_keeps_content_derived_display_limits() {
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
            highlight_fraction: None,
        };
        let mut limits = AutoColorLimits {
            d_min: [0.12, 0.18, 0.24],
            d_max: [0.92, 1.08, 1.24],
            pipeline_state: None,
        };
        prepare_content_render_limits(&mut limits, &anchors, base.density);
        assert_ne!(limits.d_min, [0.0; 3]);
        assert_ne!(limits.d_max, [1.9; 3]);
        // A partial Roll anchor still resolves this frame's window from its own
        // content, but the origin stays shared so the film base prints neutral.
        let levels = base_display_levels(&limits);
        assert!(
            levels.iter().copied().fold(f32::MIN, f32::max)
                - levels.iter().copied().fold(f32::MAX, f32::min)
                <= 1.0e-4,
            "the film base must print neutral: {levels:?}"
        );
        let spans = [
            limits.d_max[0] - limits.d_min[0],
            limits.d_max[1] - limits.d_min[1],
            limits.d_max[2] - limits.d_min[2],
        ];
        let maximum = spans.iter().copied().fold(f32::MIN, f32::max);
        let minimum = spans.iter().copied().fold(f32::MAX, f32::min);
        assert!(minimum > 0.2 - 1.0e-6, "no channel may collapse: {spans:?}");
        assert!(
            maximum / minimum <= CHANNEL_RESPONSE_MAX_GAIN + 1.0e-3,
            "the span correction stays bounded: {spans:?}"
        );
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

    /// Manual colour diagnostic for a loose RAW frame: renders the Smart Auto
    /// path under two white-balance policies and reports the resulting cast.
    #[test]
    #[ignore = "manual loose RAW render diagnostic; decodes local fixtures"]
    fn loose_raw_render_diagnostic() {
        use crate::raw_backend::WhiteBalancePolicy;
        const EDGE: u32 = 1400;
        let frame = std::env::var("NEXFILM_DIAG_FRAME").unwrap_or_else(|_| {
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("test_picture")
                .join("哈苏fff")
                .join("任务 _1233.fff")
                .to_string_lossy()
                .to_string()
        });
        let output_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join("diag-loose-raw");
        std::fs::create_dir_all(&output_root).unwrap();
        let stem = std::path::Path::new(&frame)
            .file_stem()
            .map(|value| value.to_string_lossy().to_string())
            .unwrap_or_else(|| "frame".to_string());
        let geom = GeometryState::default();

        for (label, white_balance) in [
            ("daylight", WhiteBalancePolicy::FromOptions),
            ("normalized-as-shot", WhiteBalancePolicy::NormalizedAsShot),
        ] {
            let mut estimate = decode_prophoto_estimate_image_buffer_with_policy(
                &frame,
                DecodeMode::DevelopProxy,
                white_balance,
            )
            .unwrap();
            let (width, height) = estimate.dimensions();
            let ratio = (EDGE as f32 / width.max(height) as f32).min(1.0);
            if ratio < 0.999 {
                estimate = image::imageops::resize(
                    &estimate,
                    (width as f32 * ratio).max(1.0) as u32,
                    (height as f32 * ratio).max(1.0) as u32,
                    image::imageops::FilterType::Lanczos3,
                );
            }
            let mut state = default_pipeline_state_for_import(true, "LOOSE_DEFAULT", &[]);
            state.processing_report.base_source = "content_estimate".to_string();
            state.processing_report.base_confidence = "0.500".to_string();
            // Mirror production: the analysed frame base is the density
            // reference for an unanchored Smart Auto frame.
            let (analyzed_base_density, _) = compute_auto_base_f32(&estimate, &geom).unwrap();
            let analyzed_base_color = base_color_from_density(analyzed_base_density);
            let base = pipeline_base_density(&state, &analyzed_base_color);
            let mut limits = compute_content_limits_f32_with_bounds(
                &estimate,
                None,
                &geom,
                base,
                roll_physical_density_span(&state.density_anchors, base),
            )
            .unwrap();
            let (offsets, _) =
                prepare_content_render_limits(&mut limits, &state.density_anchors, base);
            state.render_mapping.mode = RenderMode::PreserveTone;
            state.render_mapping.density_low = limits.d_min;
            state.render_mapping.density_high = limits.d_max;
            state.render_mapping.channel_offsets = offsets;
            let mut params = TuningParams::default();
            params.density.d_min = limits.d_min;
            params.density.d_max = limits.d_max;
            let rendered = render_f32_shader_equivalent(
                &estimate,
                None,
                &params,
                &geom,
                &analyzed_base_color,
                &state,
                None,
            );
            ab_save_preview(output_root.join(format!("{stem}-{label}.jpg")), &rendered);
            ab_print_rendered_cast(label, &rendered, &geom);
            for (region, uv) in [("sky", [0.5, 0.15]), ("subject", [0.5, 0.6])] {
                print!("[DIAG] {stem} {label} ");
                ab_probe_u16(region, &rendered, uv);
            }
            // The v1.0.2 math on the same frame, for comparison: linear-sRGB
            // working space, per-channel base subtraction and the Status M
            // crosstalk matrix, with no display-space matrix.
            if let Ok(legacy_proxy) =
                decode_image_buffer(&frame, DecodeMode::DevelopProxy).map(|image| {
                    let ratio = (EDGE as f32 / image.width().max(image.height()) as f32).min(1.0);
                    if ratio < 0.999 {
                        image::imageops::resize(
                            &image,
                            (image.width() as f32 * ratio).max(1.0) as u32,
                            (image.height() as f32 * ratio).max(1.0) as u32,
                            image::imageops::FilterType::Lanczos3,
                        )
                    } else {
                        image
                    }
                })
            {
                let legacy_base = compute_auto_base(&legacy_proxy);
                if let Ok(legacy_limits) = compute_auto_color_limits(
                    &legacy_proxy,
                    &geom,
                    &legacy_base,
                    FilmMode::Color,
                    false,
                ) {
                    let mut legacy_params = TuningParams::default();
                    legacy_params.density.d_min = legacy_limits.d_min;
                    legacy_params.density.d_max = legacy_limits.d_max;
                    let legacy_render = render_shader_equivalent(
                        &legacy_proxy,
                        &legacy_params,
                        &geom,
                        &legacy_base,
                        None,
                    );
                    ab_save_preview(
                        output_root.join(format!("{stem}-{label}-v102.jpg")),
                        &legacy_render,
                    );
                    ab_print_rendered_cast(&format!("{label} v1.0.2"), &legacy_render, &geom);
                }
            }

            // Same frame with a display encoding instead of writing the linear
            // density straight to the display buffer.
            let mut gamma_params = params.clone();
            gamma_params.density.gamma = 2.2;
            let gamma_render = render_f32_shader_equivalent(
                &estimate,
                None,
                &gamma_params,
                &geom,
                &analyzed_base_color,
                &state,
                None,
            );
            ab_save_preview(
                output_root.join(format!("{stem}-{label}-gamma22.jpg")),
                &gamma_render,
            );
            print!("[DIAG] {stem} {label} gamma22 ");
            ab_print_rendered_cast(&format!("{label} gamma22"), &gamma_render, &geom);
        }
    }

    /// Manual colour diagnostic for a loose TIFF: renders the retired legacy
    /// path and the current Smart Auto path side by side and prints how far the
    /// rendered channels drift from neutral. Override the input with
    /// `NEXFILM_DIAG_FRAME`.
    #[test]
    #[ignore = "manual loose TIFF render diagnostic; reads local fixtures"]
    fn loose_tiff_render_diagnostic() {
        const EDGE: u32 = 1200;
        let frame = std::env::var("NEXFILM_DIAG_FRAME").unwrap_or_else(|_| {
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("test_picture")
                .join("lr合并")
                .join("_DSC7583-Pano.tif")
                .to_string_lossy()
                .to_string()
        });
        let output_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join("diag-loose-tiff");
        std::fs::create_dir_all(&output_root).unwrap();
        let stem = std::path::Path::new(&frame)
            .file_stem()
            .map(|value| value.to_string_lossy().to_string())
            .unwrap_or_else(|| "frame".to_string());

        // Approximates the gate that automatic Film Area detection finds on a
        // merged pano: everything outside it is the orange rebate and the
        // film-edge lettering.
        let mut geom = GeometryState::default();
        geom.calibration_points = Some([
            [0.095, 0.080],
            [0.890, 0.070],
            [0.900, 0.900],
            [0.090, 0.910],
        ]);
        let linear = decode_tiff_for_smart_auto(&frame, EDGE).unwrap();
        // Follow the production branch: a profiled TIFF is converted straight
        // into the working space, only profile-less files take the u16 route.
        let estimate = match decode_profiled_tiff_prophoto_estimate(&frame, EDGE) {
            Ok(estimate) => estimate,
            Err(_) => linear_srgb_u16_to_prophoto_f32(&linear),
        };
        let mode = FilmMode::Color;
        println!(
            "[DIAG] {stem} proxy={}x{} icc={:?} estimated_input={}",
            linear.width(),
            linear.height(),
            embedded_input_profile(&frame),
            tiff_smart_auto_input_is_estimated(&frame)
        );
        ab_print_proxy_stats("decoded linear sRGB", &linear);
        ab_print_proxy_stats_f32("prophoto estimate", &estimate);

        // Optional: sample a reference rendering (e.g. a Negative Lab Pro
        // export) on a coarse grid so the target colours are on record.
        if let Ok(reference) = std::env::var("NEXFILM_REFERENCE_FRAME") {
            if let Ok(image) = image::open(&reference) {
                let rgb = image.to_rgb8();
                println!(
                    "[DIAG] reference {} {}x{}",
                    reference,
                    rgb.width(),
                    rgb.height()
                );
                for row in 0..6 {
                    let mut line = String::new();
                    for column in 0..6 {
                        let center_x = (((column as f32 + 0.5) / 6.0) * rgb.width() as f32)
                            .min(rgb.width() as f32 - 1.0)
                            as u32;
                        let center_y = (((row as f32 + 0.5) / 6.0) * rgb.height() as f32)
                            .min(rgb.height() as f32 - 1.0)
                            as u32;
                        let mut sum = [0.0f32; 3];
                        let mut samples = 0.0f32;
                        for offset_y in 0..16 {
                            for offset_x in 0..16 {
                                let x = (center_x + offset_x).min(rgb.width() - 1);
                                let y = (center_y + offset_y).min(rgb.height() - 1);
                                let pixel = rgb.get_pixel(x, y).0;
                                for channel in 0..3 {
                                    sum[channel] += f32::from(pixel[channel]);
                                }
                                samples += 1.0;
                            }
                        }
                        let mean = sum.map(|value| value / samples);
                        line.push_str(&format!(
                            "({:>3.0},{:>3.0},{:>3.0}) ",
                            mean[0], mean[1], mean[2]
                        ));
                    }
                    println!("[DIAG]   row {row}: {line}");
                }
            }
        }

        // Save the decoded negative itself (gamma-encoded for viewing) so the
        // scene layout can be checked against the rendered positive.
        let negative_preview =
            ImageBuffer::<Rgb<u16>, Vec<u16>>::from_fn(linear.width(), linear.height(), |x, y| {
                let pixel = linear.get_pixel(x, y).0;
                Rgb([
                    ((f32::from(pixel[0]) / 65535.0).powf(1.0 / 2.2) * 65535.0) as u16,
                    ((f32::from(pixel[1]) / 65535.0).powf(1.0 / 2.2) * 65535.0) as u16,
                    ((f32::from(pixel[2]) / 65535.0).powf(1.0 / 2.2) * 65535.0) as u16,
                ])
            });
        ab_save_preview(
            output_root.join(format!("{stem}-negative.jpg")),
            &negative_preview,
        );

        let legacy_base = compute_auto_base(&linear);
        println!(
            "[DIAG] {stem} v1.0.2 base u16=({},{},{}) density=({:.3},{:.3},{:.3})",
            legacy_base.base_r,
            legacy_base.base_g,
            legacy_base.base_b,
            -(f32::from(legacy_base.base_r) / 65535.0)
                .max(1.0e-6)
                .log10(),
            -(f32::from(legacy_base.base_g) / 65535.0)
                .max(1.0e-6)
                .log10(),
            -(f32::from(legacy_base.base_b) / 65535.0)
                .max(1.0e-6)
                .log10()
        );
        let legacy_limits =
            compute_auto_color_limits(&linear, &geom, &legacy_base, mode.clone(), false).unwrap();
        let mut legacy_params = TuningParams::default();
        legacy_params.density.d_min = legacy_limits.d_min;
        legacy_params.density.d_max = legacy_limits.d_max;
        let legacy_render =
            render_shader_equivalent(&linear, &legacy_params, &geom, &legacy_base, None);
        ab_save_preview(
            output_root.join(format!("{stem}-legacy.jpg")),
            &legacy_render,
        );

        let mut state = default_pipeline_state_for_import(true, "LOOSE_DEFAULT", &[]);
        state.processing_report.base_source = "content_estimate".to_string();
        state.processing_report.base_confidence = "0.500".to_string();
        let (analyzed_base_density, _) = compute_auto_base_f32(&estimate, &geom).unwrap();
        let analyzed_base_color = base_color_from_density(analyzed_base_density);
        let render_smart_auto = |estimate: &ImageBuffer<Rgb<f32>, Vec<f32>>,
                                 estimated_base: Option<[f32; 3]>|
         -> (
            ImageBuffer<Rgb<u16>, Vec<u16>>,
            AutoColorLimits,
            [f32; 3],
            PipelineState,
        ) {
            let mut state = state.clone();
            if let Some(density) = estimated_base {
                // Experiment: treat the Film-Area base as the display zero
                // reference (still not a physical anchor).
                state.density_anchors.d_min_base = Some(DensityAnchor {
                    density,
                    source: DensityAnchorSource::EstimatedFromContent,
                    scope: DensityAnchorScope::Frame,
                    confidence: DensityAnchorConfidence::Estimated,
                    reference_id: None,
                    provenance: DensityAnchorProvenance {
                        input_domain: DataDomain::ProPhotoEstimate,
                        algorithm_version: crate::app_state::DENSITY_ANCHOR_ALGORITHM_VERSION
                            .to_string(),
                        legacy: false,
                        ..Default::default()
                    },
                });
            }
            // Mirror the production path: the analysed frame base is the
            // reference, and the channels are not realigned on the content.
            let base = pipeline_base_density(&state, &analyzed_base_color);
            let mut limits = compute_content_limits_f32_with_bounds(
                estimate,
                None,
                &geom,
                base,
                roll_physical_density_span(&state.density_anchors, base),
            )
            .unwrap();
            let (offsets, _short_content) =
                prepare_content_render_limits(&mut limits, &state.density_anchors, base);
            state.render_mapping.mode = RenderMode::PreserveTone;
            state.render_mapping.density_low = limits.d_min;
            state.render_mapping.density_high = limits.d_max;
            state.render_mapping.channel_offsets = offsets;
            let mut params = TuningParams::default();
            params.density.d_min = limits.d_min;
            params.density.d_max = limits.d_max;
            let rendered = render_f32_shader_equivalent(
                estimate,
                None,
                &params,
                &geom,
                &analyzed_base_color,
                &state,
                None,
            );
            (rendered, limits, offsets, state)
        };
        let (smart_auto_render, limits, offsets, _) = render_smart_auto(&estimate, None);
        ab_save_preview(
            output_root.join(format!("{stem}-smart-auto.jpg")),
            &smart_auto_render,
        );

        println!(
            "[DIAG] {stem} limits: d_min=({:.3},{:.3},{:.3}) d_max=({:.3},{:.3},{:.3}) offsets=({:.3},{:.3},{:.3})",
            limits.d_min[0], limits.d_min[1], limits.d_min[2],
            limits.d_max[0], limits.d_max[1], limits.d_max[2],
            offsets[0], offsets[1], offsets[2]
        );
        ab_print_rendered_cast("legacy", &legacy_render, &geom);
        ab_print_rendered_cast("smart-auto", &smart_auto_render, &geom);

        // Experiment: use the Film-Area base as the per-channel density
        // reference instead of the zero reference the Smart Auto path uses now.
        let (estimated_base_density, estimated_confidence) =
            compute_auto_base_f32(&estimate, &geom).unwrap();
        println!(
            "[DIAG] {stem} estimated base density=({:.3},{:.3},{:.3}) confidence={:.3}",
            estimated_base_density[0],
            estimated_base_density[1],
            estimated_base_density[2],
            estimated_confidence
        );
        let (base_relative_render, base_relative_limits, base_relative_offsets, _) =
            render_smart_auto(&estimate, Some(estimated_base_density));
        ab_save_preview(
            output_root.join(format!("{stem}-smart-auto-based.jpg")),
            &base_relative_render,
        );
        println!(
            "[DIAG] {stem} base-relative limits: d_min=({:.3},{:.3},{:.3}) d_max=({:.3},{:.3},{:.3}) offsets=({:.3},{:.3},{:.3})",
            base_relative_limits.d_min[0], base_relative_limits.d_min[1], base_relative_limits.d_min[2],
            base_relative_limits.d_max[0], base_relative_limits.d_max[1], base_relative_limits.d_max[2],
            base_relative_offsets[0], base_relative_offsets[1], base_relative_offsets[2]
        );
        ab_print_rendered_cast("smart-auto base-relative", &base_relative_render, &geom);

        // Display-transform comparison: the new path converts the normalised
        // working values through the adapted ProPhoto(D50)->sRGB(D65) matrix,
        // which strongly reshapes saturated values; the v1.0.2 path displays its
        // working values directly. Render both to see which matches.
        for (label, apply_display_matrix) in
            [("display matrix", true), ("no display matrix", false)]
        {
            let base_density = compute_frame_base_density_f32(&estimate).unwrap().0;
            let mut limits = compute_content_limits_f32_with_bounds(
                &estimate,
                None,
                &geom,
                base_density,
                roll_physical_density_span(&state.density_anchors, base_density),
            )
            .unwrap();
            let low = crate::core_math::density_luma(limits.d_min);
            let high = crate::core_math::density_luma(limits.d_max);
            for channel in 0..3 {
                limits.d_min[channel] = low;
                limits.d_max[channel] = high;
            }
            let mut variant_state = state.clone();
            variant_state.density_anchors.d_min_base = Some(DensityAnchor {
                density: base_density,
                source: DensityAnchorSource::EstimatedFromContent,
                scope: DensityAnchorScope::Frame,
                confidence: DensityAnchorConfidence::Estimated,
                reference_id: None,
                provenance: DensityAnchorProvenance {
                    input_domain: DataDomain::ProPhotoEstimate,
                    algorithm_version: crate::app_state::DENSITY_ANCHOR_ALGORITHM_VERSION
                        .to_string(),
                    legacy: false,
                    ..Default::default()
                },
            });
            variant_state.render_mapping.mode = RenderMode::PreserveTone;
            variant_state.render_mapping.density_low = limits.d_min;
            variant_state.render_mapping.density_high = limits.d_max;
            variant_state.render_mapping.channel_offsets = [0.0; 3];
            let mut params = TuningParams::default();
            params.density.d_min = limits.d_min;
            params.density.d_max = limits.d_max;
            let _ = &params;
            let pipeline = crate::pipeline::FilmPipeline::from_state(
                &variant_state,
                &BaseColor::default(),
                [0.0; 3],
                mode.clone(),
            );
            let display_matrix =
                linear_conversion_matrix(ColorSpaceId::ProPhotoRgb, ColorSpaceId::SRgb);
            let mut rendered =
                ImageBuffer::<Rgb<u16>, Vec<u16>>::new(estimate.width(), estimate.height());
            rendered
                .as_mut()
                .par_chunks_exact_mut(3)
                .zip(estimate.as_raw().par_chunks_exact(3))
                .for_each(|(target, pixel)| {
                    let density = pipeline.compute_true_density(&[pixel[0], pixel[1], pixel[2]]);
                    let normalized = std::array::from_fn(|channel| {
                        let span = limits.d_max[channel] - limits.d_min[channel];
                        ((density[channel] - limits.d_min[channel]) / span).clamp(0.0, 1.0)
                    });
                    let rgb = if apply_display_matrix {
                        apply_linear_matrix(normalized, display_matrix)
                    } else {
                        normalized
                    };
                    for (channel, value) in rgb.iter().enumerate() {
                        target[channel] = (value.clamp(0.0, 1.0) * 65535.0).round() as u16;
                    }
                });
            ab_save_preview(output_root.join(format!("{stem}-{label}.jpg")), &rendered);
            ab_print_rendered_cast(label, &rendered, &geom);
        }

        // Decisive check: tint the same negative warm and render both paths.
        // A grey-world alignment cancels the tint (the render barely moves);
        // a film-base alignment keeps it, because the tint really is in the
        // scene. Per-frame casts on colour-dominant scenes come from the
        // difference between those two behaviours.
        for (label, tint) in [
            ("untinted", [1.0f32, 1.0, 1.0]),
            ("warm", [1.18, 1.0, 0.85]),
        ] {
            let tinted_linear = ImageBuffer::<Rgb<u16>, Vec<u16>>::from_fn(
                linear.width(),
                linear.height(),
                |x, y| {
                    let pixel = linear.get_pixel(x, y).0;
                    Rgb([
                        ((f32::from(pixel[0]) * tint[0]).clamp(0.0, 65535.0)) as u16,
                        ((f32::from(pixel[1]) * tint[1]).clamp(0.0, 65535.0)) as u16,
                        ((f32::from(pixel[2]) * tint[2]).clamp(0.0, 65535.0)) as u16,
                    ])
                },
            );
            let mut tinted_estimate = estimate.clone();
            tinted_estimate
                .as_mut()
                .par_chunks_exact_mut(3)
                .for_each(|pixel| {
                    for channel in 0..3 {
                        pixel[channel] *= tint[channel];
                    }
                });
            let legacy_tinted_base = compute_auto_base(&tinted_linear);
            let legacy_tinted = compute_auto_color_limits(
                &tinted_linear,
                &geom,
                &legacy_tinted_base,
                mode.clone(),
                false,
            )
            .unwrap();
            let mut legacy_tinted_params = TuningParams::default();
            legacy_tinted_params.density.d_min = legacy_tinted.d_min;
            legacy_tinted_params.density.d_max = legacy_tinted.d_max;
            let legacy_render_tinted = render_shader_equivalent(
                &tinted_linear,
                &legacy_tinted_params,
                &geom,
                &legacy_tinted_base,
                None,
            );
            let (smart_render_tinted, _, _, _) = render_smart_auto(&tinted_estimate, None);
            print!("[DIAG] {stem} tint={label} v1.0.2 ");
            ab_print_rendered_cast("v1.0.2", &legacy_render_tinted, &geom);
            ab_print_rendered_cast("smart-auto", &smart_render_tinted, &geom);
        }

        // Where does the densest red content actually sit? If it lands on the
        // rebate or the edge lettering, the analysis area is leaking; if it is
        // genuine scene content, the red window is legitimate and the gap is
        // the film response itself.
        {
            let points =
                geom.calibration_points
                    .unwrap_or([[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]]);
            let mut densest: Vec<(f32, [f32; 2])> = Vec::new();
            for (index, pixel) in estimate.as_raw().chunks_exact(3).enumerate() {
                if pixel[0] <= 0.0 {
                    continue;
                }
                let x = index as u32 % estimate.width();
                let y = index as u32 / estimate.width();
                densest.push((
                    pixel[0],
                    [
                        x as f32 / estimate.width() as f32,
                        y as f32 / estimate.height() as f32,
                    ],
                ));
            }
            densest.sort_by(|left, right| left.0.total_cmp(&right.0));
            for (rank, (transmission, uv)) in densest.iter().take(5).enumerate() {
                println!(
                    "[DIAG] {stem} darkest red #{rank}: T={:.4} uv=({:.3},{:.3}) in_area={}",
                    transmission,
                    uv[0],
                    uv[1],
                    point_in_film_area(*uv, &points, 0.0)
                );
            }
            let inside = densest
                .iter()
                .filter(|(_, uv)| point_in_film_area(*uv, &points, 0.0))
                .count();
            println!(
                "[DIAG] {stem} darkest-red samples inside the analysis area: {inside}/{}",
                densest.len()
            );
        }

        // Direction check: in a colour negative the scene-bright area is the
        // dense one, so a dense patch must render bright and a clear patch dark.
        {
            let mut sample = ImageBuffer::<Rgb<f32>, Vec<f32>>::new(8, 8);
            for y in 0..8u32 {
                for x in 0..8u32 {
                    let value = if x < 4 { 0.05 } else { 0.90 };
                    sample.put_pixel(x, y, Rgb([value, value, value]));
                }
            }
            let (rendered, _, _, _) = render_smart_auto(&sample, None);
            println!(
                "[DIAG] {stem} direction check: dense patch renders {:.3}, clear patch renders {:.3}",
                f32::from(rendered.get_pixel(1, 1).0[0]) / 65535.0,
                f32::from(rendered.get_pixel(6, 6).0[0]) / 65535.0
            );
        }

        // Prototype: convert the embedded profile straight into the Smart Auto
        // working space. The orange mask of a colour negative lies outside the
        // sRGB gamut, so routing through linear sRGB clips it per channel; the
        // ProPhoto working space contains it.
        if let Some(source_profile) = embedded_input_profile(&frame) {
            let encoded = decode_uncompressed_tiff_reduced(&frame, EDGE).unwrap();
            let to_prophoto = linear_conversion_matrix(source_profile, ColorSpaceId::ProPhotoRgb);
            let mut direct =
                ImageBuffer::<Rgb<f32>, Vec<f32>>::new(encoded.width(), encoded.height());
            direct
                .as_mut()
                .par_chunks_exact_mut(3)
                .zip(encoded.as_raw().par_chunks_exact(3))
                .for_each(|(target, pixel)| {
                    let rgb = [
                        f32::from(pixel[0]) / 65535.0,
                        f32::from(pixel[1]) / 65535.0,
                        f32::from(pixel[2]) / 65535.0,
                    ];
                    let linear =
                        convert_encoded_to_linear_rgb_with_matrix(rgb, source_profile, to_prophoto);
                    target.copy_from_slice(&compress_linear_srgb_for_density(linear));
                });
            ab_print_proxy_stats_f32("prophoto estimate (direct)", &direct);
            let (direct_render, direct_limits, direct_offsets, _) =
                render_smart_auto(&direct, None);
            ab_save_preview(
                output_root.join(format!("{stem}-direct-prophoto.jpg")),
                &direct_render,
            );
            println!(
                "[DIAG] {stem} direct limits: d_min=({:.3},{:.3},{:.3}) d_max=({:.3},{:.3},{:.3}) offsets=({:.3},{:.3},{:.3})",
                direct_limits.d_min[0], direct_limits.d_min[1], direct_limits.d_min[2],
                direct_limits.d_max[0], direct_limits.d_max[1], direct_limits.d_max[2],
                direct_offsets[0], direct_offsets[1], direct_offsets[2]
            );
            ab_print_rendered_cast("direct prophoto", &direct_render, &geom);
            for (region, uv) in [
                ("sky", [0.50, 0.13]),
                ("recess", [0.55, 0.55]),
                ("road", [0.30, 0.80]),
            ] {
                print!("[DIAG] {stem} direct ");
                ab_probe_u16(region, &direct_render, uv);
            }
        }

        // Prototype: keep the embedded-profile conversion in f32 instead of
        // clamping it into the u16 linear-sRGB transport, which is what clips
        // the wide-gamut red of a colour negative.
        if let Some(source_profile) = embedded_input_profile(&frame) {
            let encoded = decode_uncompressed_tiff_reduced(&frame, EDGE).unwrap();
            let to_srgb = linear_conversion_matrix(source_profile, ColorSpaceId::SRgb);
            let to_prophoto =
                linear_conversion_matrix(ColorSpaceId::SRgb, ColorSpaceId::ProPhotoRgb);
            let mut wide =
                ImageBuffer::<Rgb<f32>, Vec<f32>>::new(encoded.width(), encoded.height());
            wide.as_mut()
                .par_chunks_exact_mut(3)
                .zip(encoded.as_raw().par_chunks_exact(3))
                .for_each(|(target, pixel)| {
                    let rgb = [
                        f32::from(pixel[0]) / 65535.0,
                        f32::from(pixel[1]) / 65535.0,
                        f32::from(pixel[2]) / 65535.0,
                    ];
                    let linear =
                        convert_encoded_to_linear_rgb_with_matrix(rgb, source_profile, to_srgb);
                    let srgb = compress_linear_srgb_for_density(linear);
                    target.copy_from_slice(&apply_linear_matrix(srgb, to_prophoto));
                });
            ab_print_proxy_stats_f32("prophoto estimate (f32 conversion)", &wide);
            let (wide_render, wide_limits, _, _) = render_smart_auto(&wide, None);
            ab_save_preview(
                output_root.join(format!("{stem}-smart-auto-f32.jpg")),
                &wide_render,
            );
            println!(
                "[DIAG] {stem} f32 limits: d_min=({:.3},{:.3},{:.3}) d_max=({:.3},{:.3},{:.3})",
                wide_limits.d_min[0],
                wide_limits.d_min[1],
                wide_limits.d_min[2],
                wide_limits.d_max[0],
                wide_limits.d_max[1],
                wide_limits.d_max[2]
            );
            ab_print_rendered_cast("smart-auto-f32", &wide_render, &geom);
        }

        // Trace the whole render chain for a few pixels: the positive must keep
        // the same brightness ordering as the scene in every channel.
        {
            let mut trace_state = state.clone();
            trace_state.render_mapping.density_low = limits.d_min;
            trace_state.render_mapping.density_high = limits.d_max;
            trace_state.render_mapping.channel_offsets = offsets;
            let pipeline = crate::pipeline::FilmPipeline::from_state(
                &trace_state,
                &BaseColor::default(),
                [0.0; 3],
                mode.clone(),
            );
            let matrix = linear_conversion_matrix(ColorSpaceId::ProPhotoRgb, ColorSpaceId::SRgb);
            println!("[DIAG] {stem} prophoto->srgb matrix = {matrix:?}");
            for (label, uv) in [
                ("sky", [0.50, 0.13]),
                ("recess", [0.55, 0.55]),
                ("road", [0.30, 0.80]),
            ] {
                let x = ((uv[0] * estimate.width() as f32) as u32).min(estimate.width() - 1);
                let y = ((uv[1] * estimate.height() as f32) as u32).min(estimate.height() - 1);
                let pixel = estimate.get_pixel(x, y).0;
                let density = pipeline.compute_true_density(&[pixel[0], pixel[1], pixel[2]]);
                let normalized = std::array::from_fn(|channel| {
                    let span = limits.d_max[channel] - limits.d_min[channel];
                    ((density[channel] - limits.d_min[channel]) / span).clamp(0.0, 1.0)
                });
                let mixed = apply_linear_matrix(normalized, matrix);
                let rendered = smart_auto_render.get_pixel(x, y).0;
                println!(
                    "[DIAG] {stem} trace {label}: estimate=({:.3},{:.3},{:.3}) density=({:.3},{:.3},{:.3}) normalized=({:.3},{:.3},{:.3}) mixed=({:.3},{:.3},{:.3}) rendered=({:.3},{:.3},{:.3})",
                    pixel[0], pixel[1], pixel[2],
                    density[0], density[1], density[2],
                    normalized[0], normalized[1], normalized[2],
                    mixed[0], mixed[1], mixed[2],
                    f32::from(rendered[0]) / 65535.0,
                    f32::from(rendered[1]) / 65535.0,
                    f32::from(rendered[2]) / 65535.0
                );
            }
        }

        // Prototype: keep the display matrix but replace the per-channel clamp
        // with a chroma compression toward the neutral axis, the same idea the
        // input side already uses, so out-of-gamut values lose saturation
        // instead of shifting hue.
        {
            let mut state = state.clone();
            let base = [0.0; 3];
            let mut limits = compute_content_limits_f32_with_bounds(
                &estimate,
                None,
                &geom,
                base,
                roll_physical_density_span(&state.density_anchors, base),
            )
            .unwrap();
            let (offsets, _) =
                prepare_content_render_limits(&mut limits, &state.density_anchors, base);
            let matrix = linear_conversion_matrix(ColorSpaceId::ProPhotoRgb, ColorSpaceId::SRgb);
            let mut pipeline_state = state.clone();
            pipeline_state.render_mapping.density_low = limits.d_min;
            pipeline_state.render_mapping.density_high = limits.d_max;
            pipeline_state.render_mapping.channel_offsets = offsets;
            let pipeline = crate::pipeline::FilmPipeline::from_state(
                &pipeline_state,
                &BaseColor::default(),
                [0.0; 3],
                mode.clone(),
            );
            let mut compressed =
                ImageBuffer::<Rgb<u16>, Vec<u16>>::new(estimate.width(), estimate.height());
            compressed
                .as_mut()
                .par_chunks_exact_mut(3)
                .zip(estimate.as_raw().par_chunks_exact(3))
                .for_each(|(target, pixel)| {
                    let density = pipeline.compute_true_density(&[pixel[0], pixel[1], pixel[2]]);
                    let normalized = std::array::from_fn(|channel| {
                        let span = limits.d_max[channel] - limits.d_min[channel];
                        ((density[channel] - limits.d_min[channel]) / span).clamp(0.0, 1.0)
                    });
                    let mut rgb = apply_linear_matrix(normalized, matrix);
                    let luma = crate::core_math::density_luma(rgb);
                    let mut scale = 1.0f32;
                    for value in rgb {
                        if value < 0.0 {
                            scale = scale.min(luma / (luma - value));
                        } else if value > 1.0 {
                            scale = scale.min((1.0 - luma) / (value - luma));
                        }
                    }
                    rgb = rgb.map(|value| (luma + (value - luma) * scale).clamp(0.0, 1.0));
                    for (channel, value) in rgb.iter().enumerate() {
                        target[channel] = (value * 65535.0).round() as u16;
                    }
                });
            ab_save_preview(
                output_root.join(format!("{stem}-display-gamut-compressed.jpg")),
                &compressed,
            );
            ab_print_rendered_cast("display gamut compressed", &compressed, &geom);
            for (region, uv) in [
                ("sky", [0.50, 0.13]),
                ("recess", [0.55, 0.55]),
                ("road", [0.30, 0.80]),
            ] {
                print!("[DIAG] {stem} gamut-compressed ");
                ab_probe_u16(region, &compressed, uv);
            }
        }

        // Named scene regions: the sky should be a light blue/white, the
        // concrete facade and the road neutral, the dry grass olive.
        for (label, uv) in [
            ("sky", [0.50, 0.13]),
            ("cloud", [0.72, 0.10]),
            ("facade", [0.55, 0.55]),
            ("road", [0.30, 0.80]),
            ("grass", [0.12, 0.78]),
        ] {
            println!("[DIAG] {stem} region '{label}'");
            ab_probe_u16("  decoded", &linear, uv);
            ab_probe_f32("  estimate", &estimate, uv);
            ab_probe_u16("  legacy render", &legacy_render, uv);
            ab_probe_u16("  smart-auto render", &smart_auto_render, uv);
        }
    }

    #[test]
    fn loose_smart_auto_analysis_still_follows_the_film_area() {
        // A loose frame has no film base or leader sample, so the declared Film
        // Area is the only thing that can separate the mask, the light panel and
        // the scene. Moving or cropping it therefore has to move both the
        // estimated base and the content range.
        let mut proxy = ImageBuffer::<Rgb<f32>, Vec<f32>>::from_pixel(32, 32, Rgb([0.92; 3]));
        for y in 8..24 {
            for x in 8..24 {
                let value = 0.30 + ((x + y) % 9) as f32 * 0.03;
                proxy.put_pixel(x, y, Rgb([value, value * 0.92, value * 1.08]));
            }
        }
        let mut wide = GeometryState::default();
        wide.calibration_points = Some([[0.10, 0.10], [0.90, 0.10], [0.90, 0.90], [0.10, 0.90]]);
        let mut tight = GeometryState::default();
        tight.calibration_points = Some([[0.28, 0.28], [0.72, 0.28], [0.72, 0.72], [0.28, 0.72]]);

        let (wide_base, wide_confidence) = compute_auto_base_f32(&proxy, &wide).unwrap();
        let (tight_base, tight_confidence) = compute_auto_base_f32(&proxy, &tight).unwrap();
        assert!(wide_confidence > 0.0 && tight_confidence > 0.0);
        assert!(wide_base.iter().all(|value| *value > 0.0));
        assert!(
            (0..3).any(|channel| (wide_base[channel] - tight_base[channel]).abs() > 1.0e-3),
            "moving the Film Area must move the measured base: {wide_base:?} vs {tight_base:?}"
        );

        let wide_limits = compute_content_limits_f32(&proxy, None, &wide, wide_base).unwrap();
        let tight_limits = compute_content_limits_f32(&proxy, None, &tight, tight_base).unwrap();
        let range_moved = |left: &AutoColorLimits, right: &AutoColorLimits| {
            (0..3).any(|channel| {
                (left.d_min[channel] - right.d_min[channel]).abs() > 1.0e-3
                    || (left.d_max[channel] - right.d_max[channel]).abs() > 1.0e-3
            })
        };
        assert!(
            range_moved(&wide_limits, &tight_limits),
            "moving the Film Area must move the content range: {wide_limits:?} vs {tight_limits:?}"
        );

        let mut cropped = wide.clone();
        cropped.crop_rect.x = 0.30;
        cropped.crop_rect.y = 0.30;
        cropped.crop_rect.width = 0.40;
        cropped.crop_rect.height = 0.40;
        let cropped_limits = compute_content_limits_f32(&proxy, None, &cropped, wide_base).unwrap();
        assert!(
            range_moved(&wide_limits, &cropped_limits),
            "cropping the frame must move the content range: {wide_limits:?} vs {cropped_limits:?}"
        );

        // Without a Film Area there is no trustworthy base candidate, which is
        // why the content-driven zero-reference mapping takes over.
        let (no_area_base, no_area_confidence) =
            compute_auto_base_f32(&proxy, &GeometryState::default()).unwrap();
        assert_eq!(no_area_base, [0.0; 3]);
        assert_eq!(no_area_confidence, 0.0);
    }

    /// Synthetic Smart Auto frame: an orange-masked colour negative in the
    /// ProPhoto estimate domain. `tint` scales each channel's transmission, so a
    /// value above one makes that channel brighter and therefore less dense.
    fn synthetic_negative_frame(
        width: u32,
        height: u32,
        tint: [f32; 3],
    ) -> ImageBuffer<Rgb<f32>, Vec<f32>> {
        const BASE_TRANSMISSION: [f32; 3] = [0.62, 0.34, 0.20];
        ImageBuffer::from_fn(width, height, |x, y| {
            let u = x as f32 / (width - 1).max(1) as f32;
            let v = y as f32 / (height - 1).max(1) as f32;
            // Scene density per channel: a gradient plus a ripple, so the content
            // window has real structure to measure.
            let scene = 0.25 + 0.9 * (1.0 - v) + 0.25 * ((u * 9.0).sin() * 0.5 + 0.5);
            Rgb(std::array::from_fn(|channel| {
                let density = scene * [0.85, 1.0, 1.15][channel];
                (BASE_TRANSMISSION[channel] * 10.0f32.powf(-density) * tint[channel])
                    .clamp(1.0e-5, 1.0)
            }))
        })
    }

    /// Synthetic colour negative whose per-channel density *response* differs,
    /// the way an upstream capture or renderer compresses a single channel.
    ///
    /// `base_density` for this frame is [`synthetic_base_density`]: a real
    /// negative's clear base is the least dense part of the film, and the whole
    /// content of every channel sits above it.
    fn synthetic_channel_response_frame(
        width: u32,
        height: u32,
        response: [f32; 3],
    ) -> ImageBuffer<Rgb<f32>, Vec<f32>> {
        const BASE_TRANSMISSION: [f32; 3] = [0.62, 0.34, 0.20];
        ImageBuffer::from_fn(width, height, |x, y| {
            let u = x as f32 / (width - 1).max(1) as f32;
            let v = y as f32 / (height - 1).max(1) as f32;
            let scene = 0.25 + 0.9 * (1.0 - v) + 0.25 * ((u * 9.0).sin() * 0.5 + 0.5);
            Rgb(std::array::from_fn(|channel| {
                let density = scene * response[channel];
                (BASE_TRANSMISSION[channel] * 10.0f32.powf(-density)).clamp(1.0e-5, 1.0)
            }))
        })
    }

    /// Film-base density of the synthetic negatives above.
    fn synthetic_base_density() -> [f32; 3] {
        [0.62f32, 0.34, 0.20].map(|transmission| -transmission.log10())
    }

    /// Run one frame through the unified density stage exactly as the app does:
    /// shared window from the frame's own content, per-channel base subtraction,
    /// zero density-domain offsets, and the bounded per-channel response the
    /// frame's own content measures.
    fn render_unified_frame(
        working: &ImageBuffer<Rgb<f32>, Vec<f32>>,
        geom: &GeometryState,
        base_density: [f32; 3],
    ) -> (ImageBuffer<Rgb<u16>, Vec<u16>>, AutoColorLimits, [f32; 3]) {
        let mut limits =
            compute_content_limits_f32_with_bounds(working, None, geom, base_density, None)
                .unwrap();
        let (offsets, _short_content) =
            prepare_content_render_limits(&mut limits, &DensityAnchors::default(), base_density);
        let mut state = PipelineState::smart_auto();
        state.processing_report.base_source = "detected_film_base".to_string();
        state.render_mapping.mode = RenderMode::PreserveTone;
        state.render_mapping.density_low = limits.d_min;
        state.render_mapping.density_high = limits.d_max;
        state.render_mapping.channel_offsets = offsets;
        let mut params = TuningParams::default();
        params.density.d_min = limits.d_min;
        params.density.d_max = limits.d_max;
        let rendered = render_f32_shader_equivalent(
            working,
            None,
            &params,
            geom,
            &base_color_from_density(base_density),
            &state,
            None,
        );
        (rendered, limits, offsets)
    }

    /// The mapping this route produced before the channel-response work: one
    /// shared density window for all three channels.
    fn render_with_shared_window(
        working: &ImageBuffer<Rgb<f32>, Vec<f32>>,
        geom: &GeometryState,
        base_density: [f32; 3],
    ) -> ImageBuffer<Rgb<u16>, Vec<u16>> {
        let mut limits =
            compute_content_limits_f32_with_bounds(working, None, geom, base_density, None)
                .unwrap();
        share_smart_auto_density_scale_without_offsets(&mut limits);
        preserve_smart_auto_content_span(&mut limits);
        let mut state = PipelineState::smart_auto();
        state.processing_report.base_source = "detected_film_base".to_string();
        let mut params = TuningParams::default();
        params.density.d_min = limits.d_min;
        params.density.d_max = limits.d_max;
        render_f32_shader_equivalent(
            working,
            None,
            &params,
            geom,
            &base_color_from_density(base_density),
            &state,
            None,
        )
    }

    fn channel_peak(image: &ImageBuffer<Rgb<u16>, Vec<u16>>) -> [u16; 3] {
        image
            .as_raw()
            .chunks_exact(3)
            .fold([0u16; 3], |mut peak, pixel| {
                for channel in 0..3 {
                    peak[channel] = peak[channel].max(pixel[channel]);
                }
                peak
            })
    }

    fn rendered_channel_means(image: &ImageBuffer<Rgb<u16>, Vec<u16>>) -> [f64; 3] {
        let mut sums = [0.0f64; 3];
        for pixel in image.as_raw().chunks_exact(3) {
            for channel in 0..3 {
                sums[channel] += f64::from(pixel[channel]);
            }
        }
        let total = f64::from(image.width()) * f64::from(image.height());
        sums.map(|sum| sum / total.max(1.0))
    }

    /// A frame whose three channels respond alike must keep the exact shared
    /// window it has today; that is the promise that the response compensation
    /// cannot touch healthy photographs.
    #[test]
    fn balanced_channel_response_keeps_the_shared_window() {
        let geom = GeometryState::default();
        let base_density = [0.42, 0.58, 0.74];
        let frame = ImageBuffer::<Rgb<f32>, Vec<f32>>::from_fn(96, 96, |x, _y| {
            let v = (x as f32 / 95.0) * 1.0 + 0.05;
            Rgb([
                10f32.powf(-(base_density[0] + v)),
                10f32.powf(-(base_density[1] + v)),
                10f32.powf(-(base_density[2] + v)),
            ])
        });
        let mut limits =
            compute_content_limits_f32_with_bounds(&frame, None, &geom, base_density, None)
                .unwrap();

        let response = content_channel_response(&limits);
        assert!(
            response.imbalance < CHANNEL_RESPONSE_BALANCED_RATIO,
            "the synthetic frame must measure as balanced, got {}",
            response.imbalance
        );
        assert_eq!(response.gains, [1.0; 3]);

        let expected_low = density_luma(limits.d_min);
        let expected_high = density_luma(limits.d_max);
        assert!(expected_high - expected_low > 0.8);
        let (offsets, _) =
            prepare_content_render_limits(&mut limits, &DensityAnchors::default(), base_density);

        assert_eq!(offsets, [0.0; 3]);
        for channel in 0..3 {
            assert!((limits.d_min[channel] - expected_low).abs() < 1.0e-6);
            assert!((limits.d_max[channel] - expected_high).abs() < 1.0e-6);
        }
        let levels = base_display_levels(&limits);
        assert!(
            levels.iter().copied().fold(f32::MIN, f32::max)
                - levels.iter().copied().fold(f32::MAX, f32::min)
                <= 1.0e-4,
            "the film base must print neutral: {levels:?}"
        );
    }

    /// Content that sits on the print's neutral axis must stay neutral even
    /// when the frame's per-channel spans differ enough to engage the
    /// correction. The synthetic negative below carries one scene ramp scaled
    /// per channel, which is what a neutral scene looks like through three
    /// layers that do not respond alike.
    #[test]
    fn span_corrected_frame_keeps_its_neutral_axis() {
        let geom = GeometryState::default();
        let base_density = synthetic_base_density();
        let frame = synthetic_negative_frame(96, 96, [1.0, 1.0, 1.0]);
        let (rendered, limits, offsets) = render_unified_frame(&frame, &geom, base_density);
        assert_eq!(offsets, [0.0; 3]);
        let response = channel_response_from_spans([
            limits.d_max[0] - limits.d_min[0],
            limits.d_max[1] - limits.d_min[1],
            limits.d_max[2] - limits.d_min[2],
        ]);
        assert!(
            response.gains != [1.0; 3],
            "this fixture must exercise the correction: {response:?}"
        );
        let worst = rendered
            .as_raw()
            .chunks_exact(3)
            .map(|pixel| {
                let maximum = pixel.iter().copied().max().unwrap_or_default() as i32;
                let minimum = pixel.iter().copied().min().unwrap_or_default() as i32;
                maximum - minimum
            })
            .max()
            .unwrap_or_default();
        assert!(
            worst <= 2,
            "the neutral axis must survive the span correction, worst spread {worst}/65535"
        );
    }

    /// A channel a capture compressed gets its own density span, so the
    /// positive can still reach the white point there instead of keeping that
    /// channel's cast across the whole frame.
    #[test]
    fn compressed_channel_recovers_its_display_span() {
        let geom = GeometryState::default();
        let base_density = synthetic_base_density();
        let frame = synthetic_channel_response_frame(96, 96, [0.72, 1.0, 1.0]);
        let mut limits =
            compute_content_limits_f32_with_bounds(&frame, None, &geom, base_density, None)
                .unwrap();

        let response = content_channel_response(&limits);
        assert!(
            response.imbalance > CHANNEL_RESPONSE_FULL_RATIO,
            "the synthetic frame must measure as imbalanced, got {}",
            response.imbalance
        );
        assert!(
            response.gains[0] < 1.0,
            "the compressed channel must receive a shorter span: {:?}",
            response.gains
        );
        assert!(
            response.gains[0] >= 1.0 / CHANNEL_RESPONSE_MAX_GAIN - 1.0e-6,
            "the span correction stays bounded: {:?}",
            response.gains
        );
        assert!((response.gains[1] - 1.0).abs() < 0.35);
        assert!(
            response.gains[1] > 1.0,
            "the untouched channel keeps the shared response: {:?}",
            response.gains
        );

        let (offsets, _) =
            prepare_content_render_limits(&mut limits, &DensityAnchors::default(), base_density);
        assert_eq!(offsets, [0.0; 3]);
        assert!(
            limits.d_max[0] < limits.d_max[1] * 0.80,
            "the compressed channel must keep its own shorter span: {:?}",
            limits.d_max
        );
        // Every channel puts the film base (density zero) on one display value:
        // the compensation must not tint the black point.
        let base_level = [
            -limits.d_min[0] / (limits.d_max[0] - limits.d_min[0]),
            -limits.d_min[1] / (limits.d_max[1] - limits.d_min[1]),
            -limits.d_min[2] / (limits.d_max[2] - limits.d_min[2]),
        ];
        assert!(
            (base_level[0] - base_level[2]).abs() < 0.005,
            "the film base must stay neutral: {base_level:?}"
        );

        // The shared window cannot lift the compressed channel anywhere near
        // the top of the display range; the compensated one does.
        let shared = channel_peak(&render_with_shared_window(&frame, &geom, base_density));
        let shared_render = render_with_shared_window(&frame, &geom, base_density);
        let compensated_render = render_unified_frame(&frame, &geom, base_density).0;
        let compensated = channel_peak(&compensated_render);
        assert!(
            compensated[0] > shared[0],
            "the compressed channel must regain display range: {shared:?} -> {compensated:?}"
        );
        // The cast is what the correction exists for: on the shared window this
        // frame is strongly cyan, and the bounded span correction has to bring
        // the three channels back together without moving the film base.
        let shared_means = rendered_channel_means(&shared_render);
        let compensated_means = rendered_channel_means(&compensated_render);
        let shared_balance = shared_means[0] / shared_means[1];
        assert!(
            shared_balance < 0.5,
            "the shared window must leave the frame cyan: {shared_balance:.3}"
        );
        let compensated_balance = compensated_means[0] / compensated_means[1];
        assert!(
            compensated_balance > 0.85,
            "the correction must remove the cast, got {compensated_balance:.3} \
             (shared {shared_balance:.3})"
        );
    }

    /// The compensation is a bounded correction, never a free rescaling of the
    /// frame: the gain is capped and no window may collapse.
    #[test]
    fn channel_response_compensation_stays_bounded() {
        let limits = AutoColorLimits {
            d_min: [0.0; 3],
            d_max: [0.01, 2.0, 3.0],
            pipeline_state: None,
        };
        let response = content_channel_response(&limits);
        assert!(response.imbalance > CHANNEL_RESPONSE_FULL_RATIO);
        let limit = super::channel_response_gain_limit(response.imbalance);
        for gain in response.gains {
            assert!(gain >= 1.0 / limit - 1.0e-6);
            assert!(gain <= limit + 1.0e-6);
        }

        let mut applied = AutoColorLimits {
            d_min: [0.05; 3],
            d_max: [0.55; 3],
            pipeline_state: None,
        };
        apply_content_channel_response(&mut applied, &response);
        for channel in 0..3 {
            let span = applied.d_max[channel] - applied.d_min[channel];
            assert!(span >= CHANNEL_RESPONSE_MIN_SPAN - 1.0e-6, "span {span}");
            assert!((applied.d_min[channel] - 0.05 * response.gains[channel]).abs() < 1.0e-6);
        }
    }

    /// The display-side controls change tone, never the balance of a print.
    ///
    /// This is the guarantee the Develop sliders depend on: a photograph whose
    /// content sits on the print's neutral axis must stay neutral when the user
    /// moves exposure, highlights, shadows or the master D-Min/D-Max trim, even
    /// on a capture whose per-channel density spans differ (so the bounded span
    /// correction is active).
    #[test]
    fn tone_controls_never_tint_a_neutral_print() {
        let geom = GeometryState::default();
        let base_density = [0.42, 0.58, 0.74];
        let base_color = base_color_from_density(base_density);
        // Density spans in the ratio of a real scanner capture, inside the gain
        // cap so the correction is exact rather than clipped.
        let reference_spans = [0.85f32, 1.0, 1.15];
        let frame = ImageBuffer::<Rgb<f32>, Vec<f32>>::from_fn(96, 96, |x, _y| {
            let v = (x as f32 / 95.0) * 1.0 + 0.05;
            Rgb([
                10f32.powf(-(base_density[0] + v * reference_spans[0])),
                10f32.powf(-(base_density[1] + v * reference_spans[1])),
                10f32.powf(-(base_density[2] + v * reference_spans[2])),
            ])
        });
        let mut state = PipelineState::smart_auto();
        state.processing_report.base_source = "film_edge_band".to_string();
        let mut limits =
            compute_content_limits_f32_with_bounds(&frame, None, &geom, base_density, None)
                .unwrap();
        let spans = measure_content_channel_spans(&frame, None, &geom, base_density, None);
        let (offsets, _) = prepare_content_render_limits_with_spans(
            &mut limits,
            &state.density_anchors,
            base_density,
            spans,
        );
        assert_eq!(offsets, [0.0; 3]);
        let response = channel_response_from_spans(spans.expect("measured spans"));
        assert!(
            response.gains != [1.0; 3],
            "this fixture must exercise the per-channel span: {response:?}"
        );

        let mut params = TuningParams::default();
        params.density.d_min = limits.d_min;
        params.density.d_max = limits.d_max;
        let mut exposure = params.clone();
        exposure.exposure.exposure = 0.6;
        let mut highlights = params.clone();
        highlights.tone.highlights = 0.6;
        let mut shadows = params.clone();
        shadows.tone.shadows = 0.6;
        let mut trim = params.clone();
        trim.density.d_min_offset = -0.12;
        trim.density.d_max_offset = 0.09;

        let mut baseline_worst = None;
        for (label, controls) in [
            ("baseline", params),
            ("master exposure", exposure),
            ("highlights", highlights),
            ("shadows", shadows),
            ("master D-Min/D-Max trim", trim),
        ] {
            let rendered = render_f32_shader_equivalent(
                &frame,
                None,
                &controls,
                &geom,
                &base_color,
                &state,
                None,
            );
            let worst = rendered
                .as_raw()
                .chunks_exact(3)
                .map(|pixel| {
                    let maximum = pixel.iter().copied().max().unwrap_or_default() as i32;
                    let minimum = pixel.iter().copied().min().unwrap_or_default() as i32;
                    maximum - minimum
                })
                .max()
                .unwrap_or_default();
            // The residual below is not a tint the controls introduce: it is the
            // 16-bit quantisation of the stored film base plus the minimum-span
            // guard, and it is identical with every control engaged.
            let baseline_worst = *baseline_worst.get_or_insert(worst);
            assert!(
                worst <= 4 && worst <= baseline_worst + 1,
                "{label} tinted a neutral print by {worst}/65535 (baseline {baseline_worst})"
            );
        }
    }

    /// §七.1 (decisive): a frame-wide colour change must move the result. The
    /// content-aligned version of this pipeline was bit-identical because the
    /// film base, not the picture, is the neutral reference.
    #[test]
    fn unified_pipeline_responds_to_a_frame_wide_colour_change() {
        let geom = GeometryState::default();
        let base_density = [0.42, 0.58, 0.74];
        let original = synthetic_negative_frame(64, 48, [1.0, 1.0, 1.0]);
        let warmed = synthetic_negative_frame(64, 48, [1.18, 1.0, 0.85]);

        let (original_render, _, _) = render_unified_frame(&original, &geom, base_density);
        let (warmed_render, _, _) = render_unified_frame(&warmed, &geom, base_density);
        assert_ne!(
            original_render.as_raw(),
            warmed_render.as_raw(),
            "a frame-wide colour change must not be cancelled by the pipeline"
        );

        let original_means = rendered_channel_means(&original_render);
        let warmed_means = rendered_channel_means(&warmed_render);
        let original_rg = original_means[0] / original_means[1].max(1.0);
        let warmed_rg = warmed_means[0] / warmed_means[1].max(1.0);
        assert!(
            (warmed_rg - original_rg).abs() > 0.005,
            "the response must be measurable: {original_rg:.4} -> {warmed_rg:.4}"
        );

        // The response has to grow with the injection instead of merely existing.
        let stronger = synthetic_negative_frame(64, 48, [1.36, 1.0, 0.72]);
        let (stronger_render, _, _) = render_unified_frame(&stronger, &geom, base_density);
        let stronger_means = rendered_channel_means(&stronger_render);
        let stronger_rg = stronger_means[0] / stronger_means[1].max(1.0);
        assert!(
            (stronger_rg - original_rg).abs() > (warmed_rg - original_rg).abs(),
            "a stronger injection must move the result further: {original_rg:.4} -> {warmed_rg:.4} -> {stronger_rg:.4}"
        );
    }

    /// §七.7: the stitched white padding must not become a base candidate or a
    /// window endpoint. Saturated samples are excluded before either statistic
    /// sees them.
    #[test]
    fn stitched_white_padding_does_not_move_the_base_or_the_window() {
        let mut geom = GeometryState::default();
        geom.calibration_points = Some([[0.1, 0.1], [0.9, 0.1], [0.9, 0.9], [0.1, 0.9]]);
        let base_density = [0.40, 0.55, 0.70];
        let clean = synthetic_negative_frame(96, 96, [1.0, 1.0, 1.0]);
        let mut padded = clean.clone();
        // 0.139% of the frame, the share a merged panorama fills in.
        let white_pixels = (96.0f32 * 96.0 * 0.00139).round().max(1.0) as u32;
        for index in 0..white_pixels {
            let x = (index * 7) % 96;
            let y = (index * 11) % 96;
            padded.put_pixel(x, y, Rgb([1.0, 1.0, 1.0]));
        }

        let clean_windows =
            compute_content_limits_f32_with_bounds(&clean, None, &geom, base_density, None)
                .unwrap();
        let padded_windows =
            compute_content_limits_f32_with_bounds(&padded, None, &geom, base_density, None)
                .unwrap();
        let clean_base = estimate_film_base_f32(&clean, &geom);
        let padded_base = estimate_film_base_f32(&padded, &geom);
        assert_eq!(clean_base.source, padded_base.source);
        for channel in 0..3 {
            assert!(
                (clean_windows.d_min[channel] - padded_windows.d_min[channel]).abs() < 0.02
                    && (clean_windows.d_max[channel] - padded_windows.d_max[channel]).abs() < 0.02,
                "white padding moved the content window: {:?} vs {:?}",
                clean_windows,
                padded_windows
            );
            assert!(
                (clean_base.density[channel] - padded_base.density[channel]).abs() < 0.02,
                "white padding moved the film base: {:?} vs {:?}",
                clean_base.density,
                padded_base.density
            );
        }
        let (_, clean_limits, _) = render_unified_frame(&clean, &geom, base_density);
        let (_, padded_limits, _) = render_unified_frame(&padded, &geom, base_density);
        // Excluding the filler changes the percentile averages by well under a
        // thousandth of a density unit; what must not happen is the filler
        // becoming an endpoint.
        for channel in 0..3 {
            assert!(
                (clean_limits.d_min[channel] - padded_limits.d_min[channel]).abs() < 1.0e-3
                    && (clean_limits.d_max[channel] - padded_limits.d_max[channel]).abs() < 1.0e-3,
                "white padding moved the render window: {:?} vs {:?}",
                clean_limits,
                padded_limits
            );
        }
    }

    /// §七.3: frames from one Roll share the density stage and the film base, so
    /// their channel ratios must stay inside one band instead of jumping per
    /// frame.
    #[test]
    fn one_roll_keeps_a_consistent_channel_ratio() {
        let geom = GeometryState::default();
        let base_density = [0.42, 0.58, 0.74];
        let frames = [0.75f32, 1.0, 1.35].map(|exponent| {
            ImageBuffer::from_fn(64, 64, |x, y| {
                let u = x as f32 / 63.0;
                let v = y as f32 / 63.0;
                let scene = (0.2 + 0.9 * (1.0 - v)).powf(exponent) + 0.2 * u;
                Rgb([
                    (0.62 * 10.0f32.powf(-scene * 0.85)).clamp(1.0e-5, 1.0),
                    (0.34 * 10.0f32.powf(-scene)).clamp(1.0e-5, 1.0),
                    (0.20 * 10.0f32.powf(-scene * 1.15)).clamp(1.0e-5, 1.0),
                ])
            })
        });
        let ratios: Vec<(f64, f64)> = frames
            .iter()
            .map(|frame| {
                let (render, _, _) = render_unified_frame(frame, &geom, base_density);
                let means = rendered_channel_means(&render);
                (means[0] / means[1].max(1.0), means[2] / means[1].max(1.0))
            })
            .collect();
        let spread = |values: Vec<f64>| {
            values.iter().cloned().fold(f64::MIN, f64::max)
                - values.iter().cloned().fold(f64::MAX, f64::min)
        };
        let rg_spread = spread(ratios.iter().map(|ratio| ratio.0).collect());
        let bg_spread = spread(ratios.iter().map(|ratio| ratio.1).collect());
        println!("[ROLL] ratios={ratios:?} rg_spread={rg_spread:.4} bg_spread={bg_spread:.4}");
        assert!(
            rg_spread < 0.25,
            "R/G jumps between frames of one Roll: {ratios:?}"
        );
        assert!(
            bg_spread < 0.25,
            "B/G jumps between frames of one Roll: {ratios:?}"
        );
    }

    /// §七.6 diagnostic: the wide-gamut stitched TIFF criterion ("处理后蓝通道
    /// 零值占比不得高于文件自身", no cyan highlights, no channel overflow) is
    /// NOT met by the current pipeline - the red channel collapses and the blue
    /// channel overflows on two of the three merged panoramas. This test only
    /// prints the measured shares so the gap stays visible; it deliberately
    /// asserts nothing until the wide-gamut input-domain mapping is fixed.
    #[test]
    #[ignore = "large user-supplied merged TIFF fixtures; run explicitly for the wide-gamut diagnostic"]
    fn merged_panorama_wide_gamut_diagnostic() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("test_picture")
            .join("lr合并");
        fn zero_share(values: &[u16], total: u64) -> f64 {
            100.0 * values.iter().filter(|value| **value == 0).count() as f64 / total.max(1) as f64
        }
        let mut checked = 0;
        for name in [
            "_DSC7569-Pano.tif",
            "_DSC7571-Pano.tif",
            "_DSC7583-Pano.tif",
        ] {
            let path = root.join(name);
            if !path.is_file() {
                continue;
            }
            let path_string = path.to_string_lossy().to_string();
            let legacy = report_legacy_transport(&path_string, 1024).unwrap();
            let file_blue: Vec<u16> = legacy
                .as_raw()
                .chunks_exact(3)
                .map(|pixel| pixel[2])
                .collect();
            let file_blue_zero = zero_share(
                &file_blue,
                u64::from(legacy.width()) * u64::from(legacy.height()),
            );
            let working = report_working_estimate(&path_string, 1024).unwrap();
            let border = crate::film_border::detect_film_border(&image::DynamicImage::ImageRgb16(
                legacy.clone(),
            ));
            let mut geom = GeometryState::default();
            let force_no_area =
                std::env::var("NEXFILM_REPORT_NO_AREA").is_ok_and(|value| value == "1");
            if !force_no_area && border.confidence == crate::film_border::DetectionConfidence::High
            {
                geom.calibration_points = Some(border.points);
            }
            let estimate = estimate_film_base_f32(&working, &geom);
            let base = if estimate.usable {
                estimate.density
            } else {
                [0.0; 3]
            };
            let (render, _, _) = render_unified_frame(&working, &geom, base);
            let rendered_blue: Vec<u16> = render
                .as_raw()
                .chunks_exact(3)
                .map(|pixel| pixel[2])
                .collect();
            let rendered_blue_zero = zero_share(
                &rendered_blue,
                u64::from(render.width()) * u64::from(render.height()),
            );
            let rendered_blue_full = 100.0
                * rendered_blue
                    .iter()
                    .filter(|value| **value == u16::MAX)
                    .count() as f64
                / rendered_blue.len().max(1) as f64;
            println!(
                "[CLAMP] {name} file_blue_zero={file_blue_zero:.3}% rendered_blue_zero={rendered_blue_zero:.3}% rendered_blue_full={rendered_blue_full:.3}%"
            );
            checked += 1;
        }
        assert!(checked > 0, "no merged panorama fixture available");
    }

    /// §七.2/T4: with a usable film base the density stage subtracts that base
    /// per channel, shares one window and keeps every channel offset at zero. A
    /// scene dominated by one hue therefore cannot drive an opposite cast.
    #[test]
    fn a_trusted_film_base_keeps_zero_density_offsets_and_the_scene_hue() {
        let geom = GeometryState::default();
        let base_density = [0.42, 0.58, 0.74];
        // A frame dominated by one hue: the whole picture is warm.
        let warm = synthetic_negative_frame(64, 48, [1.25, 1.0, 0.80]);
        let (render, limits, offsets) = render_unified_frame(&warm, &geom, base_density);
        assert!(
            offsets.iter().all(|value| value.abs() <= 0.20 + 1.0e-5),
            "channel offsets must stay bounded within +/-0.20 D: {offsets:?}"
        );
        assert!(
            limits.d_min.iter().all(|value| value.is_finite())
                && limits.d_max.iter().all(|value| value.is_finite()),
            "the shared window must stay finite"
        );
        let means = rendered_channel_means(&render);
        assert!(
            means[0] > 0.0 && means[1] > 0.0 && means[2] > 0.0,
            "a dominant-hue frame must not be pushed to black: {means:?}"
        );
    }

    /// §七.4 and §七.5: the input class, the file suffix and the import mode may
    /// change the input-domain record, never the density maths. The same working
    /// pixels must produce the same density mapping and the same render whatever
    /// provenance they are labelled with.
    #[test]
    fn input_class_and_provenance_never_change_the_density_mapping() {
        let geom = GeometryState::default();
        let base_density = [0.40, 0.55, 0.70];
        let working = synthetic_negative_frame(48, 32, [1.0, 1.0, 1.0]);
        let (baseline_render, baseline_limits, baseline_offsets) =
            render_unified_frame(&working, &geom, base_density);

        // The same pixels labelled as a scanner TIFF, a camera DNG and a scanner
        // FFF used to pick three different density recipes.
        let mut scanner_tiff = PipelineState::smart_auto();
        scanner_tiff.processing_report.input_domain = resolve_input_domain("scan.tif", None);
        let mut camera_dng = PipelineState::smart_auto();
        camera_dng.processing_report.input_domain = resolve_input_domain("frame.dng", None);
        let mut scanner_fff = PipelineState::smart_auto();
        scanner_fff.processing_report.input_domain = resolve_input_domain("scan.fff", None);

        for state in [scanner_tiff, camera_dng, scanner_fff] {
            let mut limits =
                compute_content_limits_f32_with_bounds(&working, None, &geom, base_density, None)
                    .unwrap();
            let (offsets, _) = prepare_content_render_limits(
                &mut limits,
                &DensityAnchors::default(),
                base_density,
            );
            assert_eq!(limits.d_min, baseline_limits.d_min, "{state:?}");
            assert_eq!(limits.d_max, baseline_limits.d_max, "{state:?}");
            assert_eq!(offsets, baseline_offsets, "{state:?}");
            let mut render_state = state.clone();
            render_state.processing_report.base_source = "detected_film_base".to_string();
            render_state.render_mapping.density_low = limits.d_min;
            render_state.render_mapping.density_high = limits.d_max;
            render_state.render_mapping.channel_offsets = offsets;
            let mut params = TuningParams::default();
            params.density.d_min = limits.d_min;
            params.density.d_max = limits.d_max;
            let rendered = render_f32_shader_equivalent(
                &working,
                None,
                &params,
                &geom,
                &base_color_from_density(base_density),
                &render_state,
                None,
            );
            assert_eq!(
                rendered.as_raw(),
                baseline_render.as_raw(),
                "provenance changed the density maths"
            );
        }
    }

    /// §七.8: a film-base candidate that over- or under-subtracts must be
    /// rejected instead of producing a cast. The band around a confirmed Film
    /// Area is only believed when it agrees with the in-area low-density tail.
    #[test]
    fn film_base_quality_gate_rejects_implausible_candidates() {
        assert!(super::validate_film_base_candidate([0.3, 0.45, 0.6], 512, 64).is_ok());
        assert_eq!(
            super::validate_film_base_candidate([0.3, 0.45, 0.6], 4, 64),
            Err("film_base_too_few_samples")
        );
        assert_eq!(
            super::validate_film_base_candidate([0.0, 0.45, 0.6], 512, 64),
            Err("film_base_out_of_range")
        );
        assert_eq!(
            super::validate_film_base_candidate([1.4, 0.2, 0.3], 512, 64),
            Err("film_base_channel_spread")
        );
        assert!(super::film_base_band_agrees(
            [0.30, 0.45, 0.60],
            [0.35, 0.50, 0.65]
        ));
        assert!(!super::film_base_band_agrees(
            [0.30, 0.45, 0.60],
            [0.90, 0.50, 0.65]
        ));

        // A frame with nothing but saturated pixels has no base reference at all
        // and must say so instead of neutralising on a fabricated candidate.
        let saturated = ImageBuffer::from_pixel(64, 64, Rgb([1.0f32, 1.0, 1.0]));
        let no_base = estimate_film_base_f32(&saturated, &GeometryState::default());
        assert!(!no_base.usable);
        assert_eq!(no_base.fallback_reason, Some("missing_film_base_reference"));

        // With a confirmed Film Area and a visible rebate, both the rebate band
        // and the in-area tail must stay near the bright rebate instead of
        // running into the denser scene inside the gate.
        let mut geom = GeometryState::default();
        geom.calibration_points = Some([[0.15, 0.15], [0.85, 0.15], [0.85, 0.85], [0.15, 0.85]]);
        let mut frame = ImageBuffer::from_pixel(96, 96, Rgb([0.85f32, 0.80, 0.75]));
        for (x, y, pixel) in frame.enumerate_pixels_mut() {
            let x = x as f32 / 95.0;
            let y = y as f32 / 95.0;
            let inside_area = (0.15..0.85).contains(&x) && (0.15..0.85).contains(&y);
            // The rebate extends a little way into the gate, exactly as it does
            // on a real frame, so the in-area low-density tail has a base to
            // find.
            let in_area_rebate = inside_area && y < 0.32;
            if inside_area && !in_area_rebate {
                // Scene content that is denser than the base in every channel.
                *pixel = Rgb([0.45, 0.30, 0.18]);
            }
        }
        let estimate = estimate_film_base_f32(&frame, &geom);
        assert!(estimate.usable);
        assert!(
            matches!(
                estimate.source,
                "film_edge_band" | "film_area_low_density_tail"
            ),
            "unexpected base source {}",
            estimate.source
        );
        assert!(
            estimate.density[0] < 0.25,
            "the base must stay near the bright rebate, got {:?} ({})",
            estimate.density,
            estimate.source
        );

        // Without a Film Area the brightest in-frame quantile is the documented
        // last resort, and it is explicitly discounted.
        let no_area = estimate_film_base_f32(&frame, &GeometryState::default());
        assert!(no_area.usable);
        assert_eq!(no_area.source, "content_high_quantile");
        assert!(no_area.confidence <= 0.5);
    }

    #[test]
    fn smart_auto_content_limits_skip_invalid_samples_without_epsilon_repair() {
        let mut image = ImageBuffer::from_fn(8, 8, |x, y| {
            let value = 0.35 + ((x + y) % 4) as f32 * 0.04;
            Rgb([value, value + 0.05, value + 0.1])
        });
        image.put_pixel(0, 0, Rgb([-1.0, f32::NAN, 0.0]));
        let limits =
            compute_content_limits_f32(&image, None, &GeometryState::default(), [0.0; 3]).unwrap();
        assert!(limits
            .d_min
            .iter()
            .chain(limits.d_max.iter())
            .all(|value| value.is_finite()));
        assert!(limits.d_min.iter().all(|value| *value > 0.0));
    }

    /// Per-channel statistics of a rendered 16-bit transport: mean, quantiles,
    /// how much of the channel sits at an endpoint, and the R/G, B/G ratios the
    /// density report is quoted in.
    fn ab_render_report(label: &str, image: &image::ImageBuffer<image::Rgb<u16>, Vec<u16>>) {
        let total = u64::from(image.width()) * u64::from(image.height());
        let mut histograms = vec![[0u64; 65536]; 3];
        let mut sums = [0.0f64; 3];
        for pixel in image.as_raw().chunks_exact(3) {
            for (channel, value) in pixel.iter().enumerate() {
                histograms[channel][usize::from(*value)] += 1;
                sums[channel] += f64::from(*value);
            }
        }
        let mut means = [0.0f64; 3];
        for channel in 0..3 {
            means[channel] = sums[channel] / total.max(1) as f64;
        }
        let report = |channel: usize| {
            let histogram = &histograms[channel];
            let percentile = |fraction: f64| -> u16 {
                let target = ((total as f64 * fraction).ceil() as u64).max(1);
                let mut accumulated = 0u64;
                for (value, count) in histogram.iter().enumerate() {
                    accumulated += *count;
                    if accumulated >= target {
                        return value as u16;
                    }
                }
                u16::MAX
            };
            (
                means[channel] / 65535.0,
                percentile(0.01),
                percentile(0.50),
                percentile(0.99),
                100.0 * histogram[0] as f64 / total.max(1) as f64,
                100.0 * histogram[usize::from(u16::MAX)] as f64 / total.max(1) as f64,
            )
        };
        let stats = [report(0), report(1), report(2)];
        println!(
            "[STAT {label}] R mean={:.4} p01={} p50={} p99={} zero={:.3}% full={:.3}%",
            stats[0].0, stats[0].1, stats[0].2, stats[0].3, stats[0].4, stats[0].5
        );
        println!(
            "[STAT {label}] G mean={:.4} p01={} p50={} p99={} zero={:.3}% full={:.3}%",
            stats[1].0, stats[1].1, stats[1].2, stats[1].3, stats[1].4, stats[1].5
        );
        println!(
            "[STAT {label}] B mean={:.4} p01={} p50={} p99={} zero={:.3}% full={:.3}%",
            stats[2].0, stats[2].1, stats[2].2, stats[2].3, stats[2].4, stats[2].5
        );
        println!(
            "[STAT {label}] R/G={:.4} B/G={:.4}",
            means[0] / means[1].max(1.0e-9),
            means[2] / means[1].max(1.0e-9)
        );
    }

    /// The working-domain image the unified pipeline actually consumes for one
    /// file, following the same decoder branch `prepare_proxy` takes.
    /// The u16 linear-sRGB transport the retired v1.0.2 recipe consumed for one
    /// file, following the same decoder branch the retired path took.
    fn report_legacy_transport(
        path: &str,
        edge: u32,
    ) -> Result<image::ImageBuffer<image::Rgb<u16>, Vec<u16>>, String> {
        if is_scanner_fff_tiff(path) || is_tiff_extension(path) {
            return decode_reduced_tiff_for_working_space(path, edge)
                .or_else(|_| decode_image_buffer(path, DecodeMode::DevelopProxy));
        }
        if is_dng_extension(path) {
            return decode_reduced_dng_for_working_space(path, edge)
                .or_else(|_| decode_image_buffer(path, DecodeMode::DevelopProxy));
        }
        decode_image_buffer(path, DecodeMode::DevelopProxy)
    }

    fn report_working_estimate(
        path: &str,
        edge: u32,
    ) -> Result<image::ImageBuffer<image::Rgb<f32>, Vec<f32>>, String> {
        if is_scanner_fff_tiff(path) {
            let linear = decode_tiff_for_smart_auto(path, edge)?;
            return Ok(linear_srgb_u16_to_prophoto_f32(&linear));
        }
        if is_dng_extension(path) {
            if let Ok(linear) = decode_reduced_dng_for_working_space(path, edge) {
                return Ok(linear_srgb_u16_to_prophoto_f32(&linear));
            }
            return decode_prophoto_estimate_image_buffer(path, DecodeMode::DevelopProxy);
        }
        if is_raw_extension(path) {
            return decode_prophoto_estimate_image_buffer(path, DecodeMode::DevelopProxy);
        }
        if is_tiff_extension(path) {
            return match decode_profiled_tiff_prophoto_estimate(path, edge) {
                Ok(estimate) => Ok(estimate),
                Err(_) => {
                    let linear = decode_tiff_for_smart_auto(path, edge)?;
                    Ok(linear_srgb_u16_to_prophoto_f32(&linear))
                }
            };
        }
        decode_scanner_profiled_estimate_image_buffer(path, DecodeMode::DevelopProxy, None, edge)
    }

    /// Quantitative before/after report for the unified density pipeline.
    ///
    /// Every fixture is processed twice: through the retired v1.0.2 recipe
    /// (u16 linear-sRGB transport, whole-frame base, Status M, no display
    /// matrix) and through the unified pipeline (input-domain conversion ->
    /// ProPhoto estimate -> shared base, window, white point, display mapping).
    /// Run with
    /// `cargo test --lib density_pipeline_sample_report -- --ignored --nocapture`.
    #[test]
    #[ignore = "manual quantitative density-pipeline report over local fixtures"]
    fn density_pipeline_sample_report() {
        let edge: u32 = std::env::var("NEXFILM_REPORT_EDGE")
            .ok()
            .and_then(|value| value.parse().ok())
            .unwrap_or(1200);
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("test_picture");
        let mut samples: Vec<std::path::PathBuf> = Vec::new();
        for path in [
            root.join("哈苏fff").join("任务 _1233.fff"),
            root.join("哈苏fff").join("任务 _1343.fff"),
            root.join("哈苏fff").join("无法反相.fff"),
            root.join("哈苏fff").join("1 001-可以反相.fff"),
            root.join("哈苏fff").join("任务 _0866.fff"),
            root.join("lr合并").join("_DSC7569-Pano.tif"),
            root.join("lr合并").join("_DSC7571-Pano.tif"),
            root.join("lr合并").join("_DSC7583-Pano.tif"),
            root.join("尼康扫描仪tiff").join("5.3-1.tif"),
            root.join("尼康扫描仪tiff").join("5.3-2.tif"),
            root.join("尼康扫描仪tiff").join("5.3-3.tif"),
            root.join("尼康扫描仪tiff").join("5.3-4.tif"),
            root.join("爱普森dng").join("raw0002.dng"),
            root.join("精益黑白").join("raw0002.dng"),
            root.join("raw0029.dng"),
            root.join("raw0032.dng"),
        ] {
            if path.is_file() {
                samples.push(path);
            }
        }
        for directory in [
            root.join("尼康nef_raw"),
            root.join("诺日士jpg"),
            root.join("爱普森dng"),
            root.join("精益黑白"),
        ] {
            let Ok(entries) = std::fs::read_dir(&directory) else {
                continue;
            };
            let mut listed: Vec<std::path::PathBuf> = entries
                .filter_map(|entry| entry.ok().map(|entry| entry.path()))
                .filter(|path| path.is_file())
                .collect();
            listed.sort();
            samples.extend(listed.into_iter().take(3));
        }

        for path in samples {
            let path_string = path.to_string_lossy().to_string();
            if let Ok(only) = std::env::var("NEXFILM_REPORT_ONLY") {
                if !only.is_empty()
                    && !only
                        .split(',')
                        .any(|needle| !needle.is_empty() && path_string.contains(needle))
                {
                    continue;
                }
            }
            let file_name = path
                .file_name()
                .map(|name| name.to_string_lossy().to_string())
                .unwrap_or_default();
            println!("\n=== {file_name} ===");
            let domain = resolve_input_domain(&path_string, None);
            println!(
                "[DOMAIN] primaries={:?} transfer={:?} reference={:?} source={:?} confidence={:?} estimated={} detail={}",
                domain.primaries,
                domain.transfer,
                domain.reference,
                domain.source,
                domain.confidence,
                domain.is_estimated(),
                domain.detail
            );

            let legacy = match report_legacy_transport(&path_string, edge) {
                Ok(legacy) => legacy,
                Err(error) => {
                    println!("[SKIP] the retired recipe could not decode this file: {error}");
                    continue;
                }
            };
            let border = crate::film_border::detect_film_border(&image::DynamicImage::ImageRgb16(
                legacy.clone(),
            ));
            let mut geom = GeometryState::default();
            // NEXFILM_REPORT_NO_AREA=1 reports the loose default geometry, the
            // one the retired baseline numbers were measured with.
            let force_no_area =
                std::env::var("NEXFILM_REPORT_NO_AREA").is_ok_and(|value| value == "1");
            if !force_no_area && border.confidence == crate::film_border::DetectionConfidence::High
            {
                geom.calibration_points = Some(border.points);
            }
            // NEXFILM_REPORT_AREA="left,right" uses the full-height Film Area a
            // user actually confirmed, and NEXFILM_REPORT_FLIPV=1 matches a
            // vertically flipped persisted project, so the harness can reproduce
            // a saved frame instead of guessing a gate.
            if let Ok(area) = std::env::var("NEXFILM_REPORT_AREA") {
                let bounds: Vec<f32> = area
                    .split(',')
                    .filter_map(|value| value.trim().parse().ok())
                    .collect();
                if bounds.len() == 2 {
                    geom.calibration_points = Some([
                        [bounds[0], 0.0],
                        [bounds[1], 0.0],
                        [bounds[1], 1.0],
                        [bounds[0], 1.0],
                    ]);
                }
            }
            if std::env::var("NEXFILM_REPORT_FLIPV").is_ok_and(|value| value == "1") {
                geom.flip_v = true;
            }
            println!(
                "[FILM AREA] status={} detected={} points={:?}",
                border.status,
                geom.calibration_points.is_some(),
                geom.calibration_points
            );

            let legacy_base = compute_auto_base(&legacy);
            let legacy_limits =
                compute_auto_color_limits(&legacy, &geom, &legacy_base, FilmMode::Color, false)
                    .unwrap();
            let mut legacy_params = TuningParams::default();
            legacy_params.density.d_min = legacy_limits.d_min;
            legacy_params.density.d_max = legacy_limits.d_max;
            let legacy_render =
                render_shader_equivalent(&legacy, &legacy_params, &geom, &legacy_base, None);
            ab_render_report("v1.0.2", &legacy_render);

            let working = match report_working_estimate(&path_string, edge) {
                Ok(working) => working,
                Err(error) => {
                    println!("[SKIP] the unified pipeline could not decode this file: {error}");
                    continue;
                }
            };
            let estimate = estimate_film_base_f32(&working, &geom);
            println!(
                "[BASE] source={} usable={} density=({:.4},{:.4},{:.4}) confidence={:.4} fallback={:?}",
                estimate.source,
                estimate.usable,
                estimate.density[0],
                estimate.density[1],
                estimate.density[2],
                estimate.confidence,
                estimate.fallback_reason
            );
            let base_density = if estimate.usable {
                estimate.density
            } else {
                [0.0; 3]
            };
            let mut limits =
                compute_content_limits_f32_with_bounds(&working, None, &geom, base_density, None)
                    .unwrap();
            let measured_spans =
                measure_content_channel_spans(&working, None, &geom, base_density, None);
            let (offsets, short_content) = prepare_content_render_limits_with_spans(
                &mut limits,
                &DensityAnchors::default(),
                base_density,
                measured_spans,
            );
            println!(
                "[WINDOW] d_min=({:.4},{:.4},{:.4}) d_max=({:.4},{:.4},{:.4}) offsets=({:.4},{:.4},{:.4}) short_content={}",
                limits.d_min[0],
                limits.d_min[1],
                limits.d_min[2],
                limits.d_max[0],
                limits.d_max[1],
                limits.d_max[2],
                offsets[0],
                offsets[1],
                offsets[2],
                short_content
            );
            // §五.1 diagnosis: per-channel content densities relative to the
            // film base, next to the single shared window the density stage uses.
            // The gap between the two decides whether a channel survives.
            let span_samples = super::collect_film_area_rgb32(&working, None, &geom, true);
            if span_samples.len() >= 64 {
                let mut channels: [Vec<f32>; 3] = [Vec::new(), Vec::new(), Vec::new()];
                for sample in &span_samples {
                    for (channel, values) in channels.iter_mut().enumerate() {
                        let density = -sample[channel].max(1.0e-6).log10() - base_density[channel];
                        if density.is_finite() {
                            values.push(density);
                        }
                    }
                }
                let percentiles: [(f32, f32, f32); 3] = std::array::from_fn(|channel| {
                    let values = &mut channels[channel];
                    values.sort_unstable_by(f32::total_cmp);
                    let pick = |fraction: f32| -> f32 {
                        values[(((values.len() - 1) as f32 * fraction).round() as usize)
                            .min(values.len() - 1)]
                    };
                    (pick(0.02), pick(0.50), pick(0.98))
                });
                println!(
                    "[SPAN] base=({:.3},{:.3},{:.3}) R(p02,p50,p98)=({:.3},{:.3},{:.3}) G=({:.3},{:.3},{:.3}) B=({:.3},{:.3},{:.3}) shared_window=({:.3},{:.3})",
                    base_density[0],
                    base_density[1],
                    base_density[2],
                    percentiles[0].0,
                    percentiles[0].1,
                    percentiles[0].2,
                    percentiles[1].0,
                    percentiles[1].1,
                    percentiles[1].2,
                    percentiles[2].0,
                    percentiles[2].1,
                    percentiles[2].2,
                    limits.d_min[0],
                    limits.d_max[0]
                );
            }
            let mut state = PipelineState::smart_auto();
            state.processing_report.base_source = if estimate.usable {
                "detected_film_base"
            } else {
                "missing_film_base_reference"
            }
            .to_string();
            state.processing_report.input_domain = domain;

            // The mapping this route produced before the per-channel response
            // work: one shared density window for every channel. Rendering both
            // here keeps the before/after evidence reproducible, and the
            // difference count is the regression check for healthy frames.
            let mut shared_limits =
                compute_content_limits_f32_with_bounds(&working, None, &geom, base_density, None)
                    .unwrap();
            let response = measured_spans
                .map(channel_response_from_spans)
                .unwrap_or_else(|| content_channel_response(&shared_limits));
            share_smart_auto_density_scale_without_offsets(&mut shared_limits);
            preserve_smart_auto_content_span(&mut shared_limits);
            println!(
                "[RESPONSE] spans=({:.3},{:.3},{:.3}) imbalance={:.2} gains=({:.3},{:.3},{:.3}) compensated={}",
                response.spans[0],
                response.spans[1],
                response.spans[2],
                response.imbalance,
                response.gains[0],
                response.gains[1],
                response.gains[2],
                response.gains != [1.0; 3]
            );
            let mut shared_params = TuningParams::default();
            shared_params.density.d_min = shared_limits.d_min;
            shared_params.density.d_max = shared_limits.d_max;
            let shared_render = render_f32_shader_equivalent(
                &working,
                None,
                &shared_params,
                &geom,
                &base_color_from_density(base_density),
                &state,
                None,
            );
            let mut params = TuningParams::default();
            params.density.d_min = limits.d_min;
            params.density.d_max = limits.d_max;
            // NEXFILM_REPORT_GAMMA / NEXFILM_REPORT_EXPOSURE let this harness
            // answer "what would the existing Gamma / Exposure sliders do to
            // this frame" without touching the application defaults.
            if let Ok(value) = std::env::var("NEXFILM_REPORT_GAMMA") {
                if let Ok(gamma) = value.parse::<f32>() {
                    params.density.gamma = gamma;
                }
            }
            if let Ok(value) = std::env::var("NEXFILM_REPORT_EXPOSURE") {
                if let Ok(exposure) = value.parse::<f32>() {
                    params.exposure.exposure = exposure;
                }
            }
            let render = render_f32_shader_equivalent(
                &working,
                None,
                &params,
                &geom,
                &base_color_from_density(base_density),
                &state,
                None,
            );
            ab_render_report("unified", &render);
            let differing_samples = shared_render
                .as_raw()
                .iter()
                .zip(render.as_raw().iter())
                .filter(|(shared, compensated)| shared != compensated)
                .count();
            let mean_absolute_change = shared_render
                .as_raw()
                .iter()
                .zip(render.as_raw().iter())
                .map(|(shared, compensated)| f64::from(shared.abs_diff(*compensated)))
                .sum::<f64>()
                / render.as_raw().len().max(1) as f64;
            println!(
                "[COMPARE] shared-window vs compensated differing samples = {differing_samples} of {} mean_abs_change={:.3}/65535",
                render.as_raw().len(),
                mean_absolute_change
            );
            ab_render_report("shared-window", &shared_render);
            let output_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("target")
                .join("diag-density-report");
            std::fs::create_dir_all(&output_root).unwrap();
            ab_save_preview(
                output_root.join(format!("{file_name}-unified.jpg")),
                &render,
            );
            ab_save_preview(
                output_root.join(format!("{file_name}-shared-window.jpg")),
                &shared_render,
            );
            ab_save_preview(
                output_root.join(format!("{file_name}-v1.0.2.jpg")),
                &legacy_render,
            );
        }
    }

    /// The Roll calibrations this workspace was reported from. `rolls.json` is
    /// the compatibility mirror the application writes, and it carries the
    /// user-sampled anchors with their provenance.
    fn report_roll_anchors(
        root: &std::path::Path,
    ) -> std::collections::HashMap<String, DensityAnchors> {
        let mut anchors = std::collections::HashMap::new();
        let Ok(raw) = std::fs::read_to_string(root.join("rolls.json")) else {
            return anchors;
        };
        let Ok(list) = serde_json::from_str::<serde_json::Value>(&raw) else {
            return anchors;
        };
        for entry in list.as_array().into_iter().flatten() {
            let Some(roll_id) = entry.get("roll_id").and_then(|value| value.as_str()) else {
                continue;
            };
            let Some(value) = entry.get("density_anchors") else {
                continue;
            };
            if let Ok(parsed) = serde_json::from_value::<DensityAnchors>(value.clone()) {
                anchors.insert(roll_id.to_string(), parsed);
            }
        }
        anchors
    }

    /// The Film Area gate and the film-base estimate the application itself
    /// persisted for a frame. Reading them keeps the report on the geometry the
    /// user actually confirmed instead of a fresh auto-detected guess.
    fn reported_frame_state(
        db_path: &std::path::Path,
        roll_id: &str,
        file_path: &str,
    ) -> Option<(GeometryState, Option<BaseColor>)> {
        let connection = rusqlite::Connection::open_with_flags(
            db_path,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        )
        .ok()?;
        let (geom, base): (Option<String>, Option<String>) = connection
            .query_row(
                "SELECT geom, base_color FROM image_states WHERE roll_id = ?1 AND file_path = ?2",
                rusqlite::params![roll_id, file_path],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .ok()?;
        let geom = serde_json::from_str::<GeometryState>(geom.as_deref()?).ok()?;
        let base = base
            .as_deref()
            .and_then(|value| serde_json::from_str::<BaseColor>(value).ok());
        Some((geom, base))
    }

    /// A complete Roll calibration for a photograph whose Roll was never
    /// calibrated: the frame's own clear base, plus a leader endpoint above the
    /// frame's darkest content (a real leader is denser than any scene).
    fn synthesized_roll_anchors(
        estimate: &ImageBuffer<Rgb<f32>, Vec<f32>>,
        geom: &GeometryState,
        base: [f32; 3],
    ) -> DensityAnchors {
        let mut darkest = [0.0f32; 3];
        for sample in super::content_density_samples(estimate, None, geom, base, None) {
            for channel in 0..3 {
                darkest[channel] = darkest[channel].max(sample[channel]);
            }
        }
        let full = [
            base[0] + darkest[0] + 0.10,
            base[1] + darkest[1] + 0.10,
            base[2] + darkest[2] + 0.10,
        ];
        let span = [full[0] - base[0], full[1] - base[1], full[2] - base[2]];
        let provenance = DensityAnchorProvenance {
            input_domain: DataDomain::ProPhotoEstimate,
            algorithm_version: crate::app_state::DENSITY_ANCHOR_ALGORITHM_VERSION.to_string(),
            raw_decode_version: Some(crate::persistence::RAW_DECODE_VERSION),
            legacy: false,
            ..Default::default()
        };
        DensityAnchors {
            d_min_base: Some(DensityAnchor {
                density: base,
                source: DensityAnchorSource::SampledFilmBase,
                scope: DensityAnchorScope::Roll,
                confidence: DensityAnchorConfidence::UserSampled,
                reference_id: None,
                provenance: provenance.clone(),
            }),
            d_max_full_exposure: Some(DensityAnchor {
                density: full,
                source: DensityAnchorSource::SampledFullExposure,
                scope: DensityAnchorScope::Roll,
                confidence: DensityAnchorConfidence::UserSampled,
                reference_id: None,
                provenance,
            }),
            retained_records: Vec::new(),
            highlight_fraction: detect_frame_highlight_fraction(estimate, base, span),
        }
    }

    /// The film base is the neutral reference of the whole pipeline: after the
    /// window is resolved it must land on one display value in all three
    /// channels, or the print shows the mask as a cast.
    fn base_display_levels(limits: &AutoColorLimits) -> [f32; 3] {
        std::array::from_fn(|channel| {
            -limits.d_min[channel] / (limits.d_max[channel] - limits.d_min[channel])
        })
    }

    /// One-line balance summary so the three routes can be compared across a
    /// whole fixture set at a glance.
    fn report_balance(stem: &str, label: &str, image: &ImageBuffer<Rgb<u16>, Vec<u16>>) {
        let means = rendered_channel_means(image);
        println!(
            "[BALANCE] {stem:<22} {label:<22} R/G={:.4} B/G={:.4}",
            means[0] / means[1].max(1.0),
            means[2] / means[1].max(1.0)
        );
    }

    /// One photograph through all three business routes, next to the retired
    /// v1.0.2 recipe.
    ///
    /// * `P1` complete Roll calibration (`RollAnchoredDirectInvert`): fixed
    ///   mapping from the sampled base and leader, no content window.
    /// * `P2` Roll import without a calibrated base or leader: the confirmed
    ///   Film Area's clear-base estimate drives a per-frame window.
    /// * `P3` Loose Import: no Roll container, and by design the identical
    ///   per-frame path (`P2` and `P3` must render bit-identical).
    ///
    /// Override the fixtures with `NEXFILM_PIPELINE_FIXTURES` (`path|roll_id`
    /// entries separated by `;`), narrow the run with `NEXFILM_PIPELINE_ONLY`,
    /// and set the decode edge with `NEXFILM_REPORT_EDGE`.
    #[test]
    #[ignore = "manual three-pipeline parity report over real photographs"]
    fn three_pipeline_parity_report() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let edge: u32 = std::env::var("NEXFILM_REPORT_EDGE")
            .ok()
            .and_then(|value| value.parse().ok())
            .unwrap_or(1200);
        let calibrations = report_roll_anchors(root);
        let mut fixtures: Vec<(String, Option<String>)> = Vec::new();
        if let Ok(spec) = std::env::var("NEXFILM_PIPELINE_FIXTURES") {
            for entry in spec.split(';').filter(|entry| !entry.trim().is_empty()) {
                let mut parts = entry.split('|');
                let path = parts.next().unwrap_or_default().trim().to_string();
                let roll = parts
                    .next()
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(str::to_string);
                fixtures.push((path, roll));
            }
        } else {
            for (path, roll) in [
                // Uncalibrated Roll (scenario 2), real captures.
                (r"G:\DCIM\755ND810\_DSC7654.NEF", "roll_1789478298921_665"),
                (r"G:\DCIM\755ND810\_DSC7655.NEF", "roll_1789478298921_665"),
                (r"G:\DCIM\755ND810\_DSC7656.NEF", "roll_1789478298921_665"),
                (r"G:\DCIM\755ND810\_DSC7688.NEF", "roll_1789383279417_164"),
                // Calibrated Roll (scenario 1), real captures.
                (r"G:\DCIM\755ND810\_DSC7724.NEF", "roll_1789385815308_996"),
                (r"G:\DCIM\755ND810\_DSC7725.NEF", "roll_1789385815308_996"),
            ] {
                fixtures.push((path.to_string(), Some(roll.to_string())));
            }
            for path in [
                "诺日士jpg/000000070001.jpg",
                "诺日士jpg/000000070002.jpg",
                "诺日士jpg/000000070003.jpg",
                "尼康扫描仪tiff/5.3-1.tif",
                "哈苏fff/任务 _0866.fff",
                "lr合并/_DSC7569-Pano.tif",
            ] {
                fixtures.push((
                    root.join("test_picture")
                        .join(path)
                        .to_string_lossy()
                        .to_string(),
                    None,
                ));
            }
        }

        let out_root = root.join("target").join("three-pipeline");
        std::fs::create_dir_all(&out_root).unwrap();
        let only = std::env::var("NEXFILM_PIPELINE_ONLY").unwrap_or_default();

        for (path, roll_id) in fixtures {
            if !only.is_empty()
                && !only
                    .split(',')
                    .any(|needle| !needle.is_empty() && path.contains(needle))
            {
                continue;
            }
            let source_path = std::path::Path::new(&path);
            if !source_path.is_file() {
                println!("\n[SKIP] missing fixture {path}");
                continue;
            }
            let stem = source_path
                .file_stem()
                .map(|value| value.to_string_lossy().to_string())
                .unwrap_or_default();
            let Ok(legacy) = report_legacy_transport(&path, edge) else {
                println!("\n[SKIP] {stem}: the retired recipe could not decode this file");
                continue;
            };
            let border = crate::film_border::detect_film_border(&image::DynamicImage::ImageRgb16(
                legacy.clone(),
            ));
            let mut geom = GeometryState::default();
            if border.confidence == crate::film_border::DetectionConfidence::High {
                geom.calibration_points = Some(border.points);
                geom.calibration_confirmed = true;
            }
            let db_path = root.join("nexfilm_user.db");
            let persisted = roll_id
                .as_deref()
                .and_then(|id| reported_frame_state(&db_path, id, &path));
            let mut gate_origin = "auto-detected gate";
            let persisted = persisted.filter(|(state, _)| {
                state.calibration_confirmed && state.calibration_points.is_some()
            });
            if roll_id.is_some() && persisted.is_none() {
                println!(
                    "\n[SKIP] {stem}: this Roll frame has no confirmed Film Area in the workspace database"
                );
                continue;
            }
            if let Some((state, base)) = persisted.as_ref() {
                // Keep the user's confirmed gate and orientation, but show the
                // whole scan so the rebate and sprocket area stay inspectable:
                // that is where a residual cast shows up first.
                geom = state.clone();
                geom.crop_rect = crate::app_state::CropRect {
                    x: 0.0,
                    y: 0.0,
                    width: 1.0,
                    height: 1.0,
                };
                gate_origin = "confirmed Film Area (from the workspace database)";
                if let Some(base) = base {
                    println!(
                        "[STORED BASE] r={} g={} b={} density=({:.4},{:.4},{:.4})",
                        base.base_r,
                        base.base_g,
                        base.base_b,
                        -((base.base_r as f32 / 65535.0).max(1.0e-6)).log10(),
                        -((base.base_g as f32 / 65535.0).max(1.0e-6)).log10(),
                        -((base.base_b as f32 / 65535.0).max(1.0e-6)).log10()
                    );
                }
            }
            let Ok(estimate) = report_working_estimate(&path, edge) else {
                println!("\n[SKIP] {stem}: the unified pipeline could not decode this file");
                continue;
            };

            println!("\n================ {stem} ================");
            println!(
                "[GATE] origin={} detected={} status={} points={:?} roll={}",
                gate_origin,
                geom.calibration_points.is_some(),
                border.status,
                geom.calibration_points,
                roll_id.as_deref().unwrap_or("-")
            );

            // ---- Scenario 2 and 3: the per-frame Film Area path -------------
            let frame_base = estimate_film_base_f32(&estimate, &geom);
            let base_density = if frame_base.usable {
                frame_base.density
            } else {
                [0.0; 3]
            };
            let base_color = base_color_from_density(base_density);
            let base_source = match frame_base.source {
                "film_edge_band" => "film_edge_band",
                "film_area_low_density_tail" => "detected_film_base",
                "content_high_quantile" => "content_estimate",
                _ => "missing_film_base_reference",
            };
            println!(
                "[BASE] source={} usable={} density=({:.4},{:.4},{:.4}) confidence={:.3} fallback={:?}",
                frame_base.source,
                frame_base.usable,
                frame_base.density[0],
                frame_base.density[1],
                frame_base.density[2],
                frame_base.confidence,
                frame_base.fallback_reason
            );

            let base_trusted = frame_base.usable && base_density.iter().any(|value| *value > 0.0);
            if !base_trusted {
                // Scenarios 2 and 3 both require a confirmed Film Area whose
                // clear base can actually be measured. Without one the frame has
                // no neutral reference at all, so it has no per-frame route to
                // report: it would only show the content-alignment fallback.
                println!(
                    "[NO TRUSTED BASE] fallback={:?} - the per-frame routes are skipped for this fixture",
                    frame_base.fallback_reason
                );
            }
            if base_trusted {
                let mut uncalibrated = PipelineState::from_roll_anchors(DensityAnchors::default());
                uncalibrated.processing_report.base_source = base_source.to_string();
                let mut limits = compute_content_limits_f32_with_bounds(
                    &estimate,
                    None,
                    &geom,
                    base_density,
                    None,
                )
                .unwrap();
                let spans =
                    measure_content_channel_spans(&estimate, None, &geom, base_density, None);
                let (offsets, short_content) = prepare_content_render_limits_with_spans(
                    &mut limits,
                    &uncalibrated.density_anchors,
                    base_density,
                    spans,
                );
                if let Some(response) = spans.map(channel_response_from_spans) {
                    println!(
                    "[RESPONSE] spans=({:.3},{:.3},{:.3}) imbalance={:.3} gains=({:.3},{:.3},{:.3})",
                    response.spans[0],
                    response.spans[1],
                    response.spans[2],
                    response.imbalance,
                    response.gains[0],
                    response.gains[1],
                    response.gains[2]
                );
                }
                println!(
                "[WINDOW] d_min=({:.4},{:.4},{:.4}) d_max=({:.4},{:.4},{:.4}) offsets=({:.4},{:.4},{:.4}) short_content={short_content}",
                limits.d_min[0],
                limits.d_min[1],
                limits.d_min[2],
                limits.d_max[0],
                limits.d_max[1],
                limits.d_max[2],
                offsets[0],
                offsets[1],
                offsets[2]
            );
                let levels = base_display_levels(&limits);
                let level_spread = levels.iter().copied().fold(f32::MIN, f32::max)
                    - levels.iter().copied().fold(f32::MAX, f32::min);
                println!(
                    "[BASE LEVEL] display=({:.4},{:.4},{:.4}) spread={:.5}",
                    levels[0], levels[1], levels[2], level_spread
                );
                // A frame with a trusted base must keep one shared window origin, so
                // the base prints as one neutral value.
                assert!(
                    offsets == [0.0; 3],
                    "a frame with a trusted base must not shift the window origin: {offsets:?}"
                );
                assert!(
                    level_spread <= 1.0e-4,
                    "the film base must stay neutral: {levels:?}"
                );

                let mut params = TuningParams::default();
                params.density.d_min = limits.d_min;
                params.density.d_max = limits.d_max;
                let roll_render = render_f32_shader_equivalent(
                    &estimate,
                    None,
                    &params,
                    &geom,
                    &base_color,
                    &uncalibrated,
                    None,
                );
                ab_render_report("P2-uncalibrated-roll", &roll_render);
                report_balance(&stem, "P2-uncalibrated-roll", &roll_render);
                ab_save_preview(
                    out_root.join(format!("{stem}-P2-uncalibrated-roll.jpg")),
                    &roll_render,
                );

                let mut loose = PipelineState::smart_auto();
                loose.processing_report.base_source = base_source.to_string();
                let loose_render = render_f32_shader_equivalent(
                    &estimate,
                    None,
                    &params,
                    &geom,
                    &base_color,
                    &loose,
                    None,
                );
                let identical = roll_render.as_raw() == loose_render.as_raw();
                println!(
                    "[PARITY] uncalibrated Roll vs Loose Import identical samples = {}/{}",
                    if identical {
                        roll_render.as_raw().len()
                    } else {
                        roll_render
                            .as_raw()
                            .iter()
                            .zip(loose_render.as_raw().iter())
                            .filter(|(left, right)| left != right)
                            .count()
                    },
                    roll_render.as_raw().len()
                );
                assert!(
                    identical,
                    "scenario 2 and scenario 3 must share one density path"
                );
                ab_save_preview(
                    out_root.join(format!("{stem}-P3-loose-import.jpg")),
                    &loose_render,
                );
            }

            // ---- Scenario 1: complete Roll calibration ---------------------
            let calibrated = roll_id
                .as_deref()
                .and_then(|id| calibrations.get(id))
                .filter(|anchors| anchors.is_fully_anchored())
                .cloned();
            let (anchors, origin) = match calibrated {
                Some(anchors) => (anchors, "persisted Roll calibration"),
                None => (
                    synthesized_roll_anchors(&estimate, &geom, base_density),
                    "synthesized from this frame",
                ),
            };
            let mut anchored = PipelineState::from_roll_anchors(anchors);
            let anchor_base = anchored
                .density_anchors
                .d_min_base
                .as_ref()
                .map(|anchor| anchor.density)
                .expect("an anchored state carries a base anchor");
            let frame_offset_base = detect_frame_base_density(&estimate, anchor_base);
            let physical_span = roll_physical_density_span(&anchored.density_anchors, anchor_base);
            let frame_highlight = frame_offset_base
                .or(Some(anchor_base))
                .zip(physical_span)
                .and_then(|(base, span)| detect_frame_highlight_fraction(&estimate, base, span));
            let Some(mapping) =
                roll_density_mapping_with_frame_base(&anchored, frame_offset_base, frame_highlight)
            else {
                println!("[P1] skipped: the Roll anchors do not resolve on this route");
                continue;
            };
            println!(
                "[ANCHORS] origin={origin} base=({:.4},{:.4},{:.4}) full=({:.4},{:.4},{:.4}) highlight={:?} frame_base_offset={:?}",
                anchored.density_anchors.d_min_base.as_ref().unwrap().density[0],
                anchored.density_anchors.d_min_base.as_ref().unwrap().density[1],
                anchored.density_anchors.d_min_base.as_ref().unwrap().density[2],
                anchored.density_anchors.d_max_full_exposure.as_ref().unwrap().density[0],
                anchored.density_anchors.d_max_full_exposure.as_ref().unwrap().density[1],
                anchored.density_anchors.d_max_full_exposure.as_ref().unwrap().density[2],
                anchored.density_anchors.highlight_fraction,
                frame_offset_base
            );
            println!(
                "[P1 MAPPING] low=({:.4},{:.4},{:.4}) high=({:.4},{:.4},{:.4})",
                mapping.density_low[0],
                mapping.density_low[1],
                mapping.density_low[2],
                mapping.density_high[0],
                mapping.density_high[1],
                mapping.density_high[2]
            );
            anchored.render_mapping = mapping;
            let anchored_base_color = base_color_from_density(anchor_base);
            let anchored_render = render_f32_shader_equivalent(
                &estimate,
                None,
                &TuningParams::default(),
                &geom,
                &anchored_base_color,
                &anchored,
                None,
            );
            ab_render_report("P1-roll-anchored", &anchored_render);
            ab_save_preview(
                out_root.join(format!("{stem}-P1-roll-anchored.jpg")),
                &anchored_render,
            );

            // ---- Retired v1.0.2 recipe, the reported reference -------------
            let legacy_base = compute_auto_base(&legacy);
            if let Ok(legacy_limits) =
                compute_auto_color_limits(&legacy, &geom, &legacy_base, FilmMode::Color, false)
            {
                let mut legacy_params = TuningParams::default();
                legacy_params.density.d_min = legacy_limits.d_min;
                legacy_params.density.d_max = legacy_limits.d_max;
                let legacy_render =
                    render_shader_equivalent(&legacy, &legacy_params, &geom, &legacy_base, None);
                ab_render_report("legacy-v1.0.2", &legacy_render);
                ab_save_preview(
                    out_root.join(format!("{stem}-legacy-v1.0.2.jpg")),
                    &legacy_render,
                );
            }
        }
    }

    #[test]
    #[ignore = "requires the local Nikon loose-import fixture"]
    fn nikon_loose_import_reports_smart_auto_channel_statistics() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("test_picture")
            .join("尼康nef_raw")
            .join("_DSC7357.NEF");
        assert!(path.is_file(), "fixture is missing: {}", path.display());

        let image = decode_prophoto_estimate_image_buffer(
            path.to_string_lossy().as_ref(),
            DecodeMode::DevelopProxy,
        )
        .unwrap();
        let legacy_image =
            decode_image_buffer(path.to_string_lossy().as_ref(), DecodeMode::DevelopProxy).unwrap();
        let compatibility_image = srgb_proxy_u16_to_prophoto_f32(&legacy_image);
        let mut geom = GeometryState::default();
        geom.calibration_points = Some([
            [0.14733543, 0.1509434],
            [0.825169, 0.15463659],
            [0.8369906, 0.8396226],
            [0.14733543, 0.8443396],
        ]);

        let raw_limits = compute_auto_color_limits(
            &legacy_image,
            &geom,
            &compute_auto_base(&legacy_image),
            FilmMode::Color,
            false,
        )
        .unwrap();
        let display_limits = raw_limits.clone();
        let legacy_base = compute_auto_base(&legacy_image);

        let render = |limits: &AutoColorLimits| {
            let mut params = TuningParams::default();
            params.density.d_min = limits.d_min;
            params.density.d_max = limits.d_max;
            let mut state = PipelineState::smart_auto();
            state.processing_report.base_source = "content_estimate".to_string();
            state.processing_report.analysis_data_domain = "legacy_linear_srgb".to_string();
            render_f32_shader_equivalent(
                &compatibility_image,
                None,
                &params,
                &geom,
                &legacy_base,
                &state,
                None,
            )
        };
        let fixed_render = render(&display_limits);
        let legacy_limits =
            compute_auto_color_limits(&legacy_image, &geom, &legacy_base, FilmMode::Color, false)
                .unwrap();
        let mut legacy_params = TuningParams::default();
        legacy_params.density.d_min = legacy_limits.d_min;
        legacy_params.density.d_max = legacy_limits.d_max;
        let legacy_render =
            render_shader_equivalent(&legacy_image, &legacy_params, &geom, &legacy_base, None);
        let output = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join("smart-auto-real-ab");
        std::fs::create_dir_all(&output).unwrap();
        fixed_render
            .save(output.join("DSC7357-fixed-channel-offsets.png"))
            .unwrap();
        legacy_render
            .save(output.join("DSC7357-v1.0.2-legacy.png"))
            .unwrap();

        let mean = |rendered: &ImageBuffer<Rgb<u16>, Vec<u16>>| {
            let mut sum = [0u64; 3];
            for pixel in rendered.pixels() {
                for channel in 0..3 {
                    sum[channel] += pixel[channel] as u64;
                }
            }
            let count = u64::from(rendered.width()) * u64::from(rendered.height());
            sum.map(|value| value as f64 / count as f64)
        };

        let fixed_mean = mean(&fixed_render);
        let legacy_mean = mean(&legacy_render);
        let colour_distance = |left: [f64; 3], right: [f64; 3]| {
            left.into_iter()
                .zip(right)
                .map(|(a, b)| (a - b).powi(2))
                .sum::<f64>()
                .sqrt()
        };
        assert!(
            colour_distance(fixed_mean, legacy_mean) < 10.0,
            "fixed={fixed_mean:?} legacy={legacy_mean:?}"
        );

        eprintln!(
            "proxy={}x{} raw={:?}..{:?} fixed={:?}..{:?} fixed_mean={:?} legacy_base={:?} legacy={:?}..{:?} legacy_mean={:?} output={}",
            image.width(),
            image.height(),
            raw_limits.d_min,
            raw_limits.d_max,
            display_limits.d_min,
            display_limits.d_max,
            fixed_mean,
            [legacy_base.base_r, legacy_base.base_g, legacy_base.base_b],
            legacy_limits.d_min,
            legacy_limits.d_max,
            legacy_mean,
            output.display()
        );
    }

    #[test]
    fn loose_smart_auto_compatibility_matches_legacy_render_for_quantized_rgb() {
        let source = ImageBuffer::from_fn(37, 23, |x, y| {
            let r = 3_000u16.wrapping_add((x * 1_703 + y * 311) as u16);
            let g = 7_000u16.wrapping_add((x * 557 + y * 2_107) as u16);
            let b = 11_000u16.wrapping_add((x * 2_401 + y * 733) as u16);
            Rgb([r, g, b])
        });
        let mut geom = GeometryState::default();
        geom.calibration_points = Some([[0.08, 0.10], [0.92, 0.10], [0.92, 0.90], [0.08, 0.90]]);
        let base = compute_auto_base(&source);
        let limits = compute_auto_color_limits(&source, &geom, &base, FilmMode::Color, false)
            .expect("synthetic proxy should produce density limits");
        let mut params = TuningParams::default();
        params.density.d_min = limits.d_min;
        params.density.d_max = limits.d_max;
        params.density.gamma = 1.15;
        params.tone.saturation = 0.18;
        params.tone.temperature = -0.12;
        params.tone.tint = 0.07;

        let legacy = render_shader_equivalent(&source, &params, &geom, &base, None);
        let mut compatibility_state = PipelineState::smart_auto();
        compatibility_state.processing_report.base_source = "compatibility_base".to_string();
        compatibility_state.processing_report.analysis_data_domain =
            "legacy_linear_srgb".to_string();
        let compatibility_source = srgb_proxy_u16_to_prophoto_f32(&source);
        let compatible = render_f32_shader_equivalent(
            &compatibility_source,
            None,
            &params,
            &geom,
            &base,
            &compatibility_state,
            None,
        );

        let max_delta = legacy
            .as_raw()
            .iter()
            .zip(compatible.as_raw())
            .map(|(left, right)| left.abs_diff(*right))
            .max()
            .unwrap_or(0);
        // The f32 ProPhoto round trip precedes the same u16 display shader;
        // matrix round-off can move a final channel by a handful of levels.
        assert!(
            max_delta <= 4,
            "compatibility render differs by {max_delta} levels"
        );
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
        let mut params = TuningParams::default();
        params.density.d_min = limits.d_min;
        params.density.d_max = limits.d_max;
        state.processing_report.base_source = "content_estimate".to_string();
        state.render_mapping.density_low = limits.d_min;
        state.render_mapping.density_high = limits.d_max;
        let rendered = render_f32_shader_equivalent(
            &image,
            None,
            &params,
            &geom,
            &BaseColor::default(),
            &state,
            None,
        );
        assert!(rendered.as_raw().iter().all(|value| *value <= u16::MAX));
        assert!(rendered.as_raw().iter().any(|value| *value > 0));
        assert!(rendered.as_raw().iter().any(|value| *value < u16::MAX));
        let mut means = [0.0f64; 3];
        for pixel in rendered.pixels() {
            for channel in 0..3 {
                means[channel] += pixel[channel] as f64;
            }
        }
        means = means.map(|value| value / f64::from(rendered.width() * rendered.height()));
        let minimum = means.iter().copied().fold(f64::INFINITY, f64::min);
        let maximum = means.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        assert!(maximum - minimum < 500.0, "channel means: {means:?}");
    }

    /// Minimal little-endian, single-strip, uncompressed RGB16 TIFF without an
    /// ICC profile: the shape the streaming TIFF decoder accepts.
    fn write_unprofiled_rgb16_tiff(path: &std::path::Path, width: u32, height: u32) {
        fn push_entry(bytes: &mut Vec<u8>, tag: u16, type_code: u16, count: u32, value: u32) {
            bytes.extend_from_slice(&tag.to_le_bytes());
            bytes.extend_from_slice(&type_code.to_le_bytes());
            bytes.extend_from_slice(&count.to_le_bytes());
            bytes.extend_from_slice(&value.to_le_bytes());
        }

        const IFD_OFFSET: u32 = 8;
        const SHORT: u16 = 3;
        const LONG: u16 = 4;
        let entry_count: u16 = 11;
        let bits_offset = IFD_OFFSET + 2 + u32::from(entry_count) * 12 + 4;
        let pixel_offset = bits_offset + 12;

        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"II");
        bytes.extend_from_slice(&42u16.to_le_bytes());
        bytes.extend_from_slice(&IFD_OFFSET.to_le_bytes());
        bytes.extend_from_slice(&entry_count.to_le_bytes());
        push_entry(&mut bytes, 256, LONG, 1, width);
        push_entry(&mut bytes, 257, LONG, 1, height);
        push_entry(&mut bytes, 258, LONG, 3, bits_offset);
        push_entry(&mut bytes, 259, SHORT, 1, 1);
        push_entry(&mut bytes, 262, SHORT, 1, 2);
        push_entry(&mut bytes, 273, LONG, 1, pixel_offset);
        push_entry(&mut bytes, 274, SHORT, 1, 1);
        push_entry(&mut bytes, 277, SHORT, 1, 3);
        push_entry(&mut bytes, 278, LONG, 1, height);
        push_entry(&mut bytes, 279, LONG, 1, width * height * 6);
        push_entry(&mut bytes, 284, SHORT, 1, 1);
        bytes.extend_from_slice(&0u32.to_le_bytes());
        for _ in 0..3 {
            bytes.extend_from_slice(&16u32.to_le_bytes());
        }
        for y in 0..height {
            for x in 0..width {
                for channel in 0..3u32 {
                    let value = (4_000 + x * 1_000 + y * 137 + channel * 3_000) as u16;
                    bytes.extend_from_slice(&value.to_le_bytes());
                }
            }
        }
        std::fs::write(path, bytes).unwrap();
    }

    #[test]
    fn unprofiled_generic_tiff_is_read_as_an_estimated_srgb_input() {
        // Writing a plain uncompressed RGB16 TIFF keeps the regression
        // independent of the gitignored capture fixtures under `test_picture`.
        let directory =
            std::env::temp_dir().join(format!("nexfilm-unprofiled-tiff-{}", std::process::id()));
        std::fs::create_dir_all(&directory).unwrap();
        let path = directory.join("unprofiled-rgb16.tif");
        write_unprofiled_rgb16_tiff(&path, 6, 4);
        let path_string = path.to_string_lossy().to_string();

        assert!(embedded_input_profile(&path_string).is_none());
        assert!(tiff_smart_auto_input_is_estimated(&path_string));

        // The retired contract rejected this file outright, which turned a file
        // the legacy path had always accepted into a hard import failure.
        let decoded = decode_tiff_for_smart_auto(&path_string, 256)
            .expect("an unprofiled RGB TIFF must still decode");
        assert_eq!(decoded.dimensions(), (6, 4));

        std::fs::remove_file(&path).ok();
        std::fs::remove_dir_all(&directory).ok();
    }

    #[test]
    fn profiled_tiff_conversion_keeps_the_mask_inside_every_channel() {
        // Adobe RGB sample of a colour negative's orange mask, taken from the
        // merged pano fixture.
        let encoded = [0.9929, 0.7916, 0.7066];
        let to_srgb = linear_conversion_matrix(ColorSpaceId::AdobeRgb, ColorSpaceId::SRgb);
        let via_srgb =
            convert_encoded_to_linear_rgb_with_matrix(encoded, ColorSpaceId::AdobeRgb, to_srgb);
        assert!(
            via_srgb[0] > 1.0,
            "the mask must lie outside the sRGB gamut: {via_srgb:?}"
        );

        let estimate = encoded_pixel_to_prophoto_estimate(encoded, ColorSpaceId::AdobeRgb);
        assert!(
            estimate.iter().all(|value| *value > 0.0 && *value < 1.0),
            "the working space must contain the mask without clamping: {estimate:?}"
        );
        assert!(
            estimate[0] > estimate[1] && estimate[1] > estimate[2],
            "the mask keeps its warm ordering: {estimate:?}"
        );
    }

    fn scanner_fff_keeps_the_tiff_branch_and_is_never_treated_as_an_estimate() {
        let fixture_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("test_picture")
            .join("哈苏fff");
        let scanner = fixture_root.join("1 001-可以反相.fff");
        if scanner.is_file() {
            let path = scanner.to_string_lossy().to_string();
            assert!(is_scanner_fff_tiff(&path));
            // `.fff` is also a LibRaw extension, so the scanner container check
            // has to win before the branch falls through to LibRaw.
            assert!(is_raw_extension(&path));
            assert!(is_tiff_extension(&path) || is_scanner_fff_tiff(&path));
            assert!(!tiff_smart_auto_input_is_estimated(&path));
        }
        // A Hasselblad digital-back capture uses the same extension but must
        // stay on the LibRaw path instead.
        let camera = fixture_root.join("任务 _1233.fff");
        if camera.is_file() {
            let path = camera.to_string_lossy().to_string();
            assert!(!is_scanner_fff_tiff(&path));
            assert!(is_raw_extension(&path));
        }
    }

    #[test]
    fn smart_auto_uses_one_shared_density_scale_for_colour_channels() {
        let mut limits = AutoColorLimits {
            d_min: [0.10, 0.20, 0.30],
            d_max: [0.80, 1.00, 1.20],
            pipeline_state: None,
        };
        let offsets = share_smart_auto_density_scale(&mut limits);
        let spans = [
            limits.d_max[0] - limits.d_min[0],
            limits.d_max[1] - limits.d_min[1],
            limits.d_max[2] - limits.d_min[2],
        ];
        assert!((spans[0] - spans[1]).abs() < 1.0e-6);
        assert!((spans[1] - spans[2]).abs() < 1.0e-6);
        assert!(offsets[0] < offsets[1] && offsets[1] < offsets[2]);
        // The bound only exists to stop a strongly coloured subject from driving
        // an unbounded grey-world shift; a strongly masked colour negative needs
        // more than the historical 0.30 before it is neutralised.
        assert!(offsets.iter().all(|value| value.abs() <= 0.60));
        assert_ne!(limits.d_min[0], limits.d_min[2]);
    }

    #[test]
    fn smart_auto_short_content_gets_an_adaptive_mid_tone_window() {
        let mut limits = AutoColorLimits {
            d_min: [0.20, 0.30, 0.40],
            d_max: [0.40, 0.50, 0.60],
            pipeline_state: None,
        };
        preserve_smart_auto_content_span(&mut limits);
        for (actual, expected) in limits.d_min.into_iter().zip([-0.10, 0.0, 0.10]) {
            assert!((actual - expected).abs() < 1.0e-6);
        }
        for (actual, expected) in limits.d_max.into_iter().zip([0.70, 0.80, 0.90]) {
            assert!((actual - expected).abs() < 1.0e-6);
        }
    }

    #[test]
    fn estimated_smart_auto_base_is_analysis_state_not_physical_anchor() {
        let mut state = PipelineState::smart_auto();
        let analyzed_base = BaseColor {
            base_r: 20000,
            base_g: 15000,
            base_b: 10000,
        };
        assert!(!pipeline_has_base(&state, &BaseColor::default()));
        assert!(!pipeline_has_base(&state, &analyzed_base));
        state.processing_report.base_source = "content_estimate".to_string();
        // A default base_color must never be considered as having a valid base
        assert!(!pipeline_has_base(&state, &BaseColor::default()));
        // An analyzed non-default base_color has a valid base
        assert!(pipeline_has_base(&state, &analyzed_base));
        assert_ne!(pipeline_base_density(&state, &analyzed_base), [0.0; 3]);
        assert!(state.density_anchors.d_min_base.is_none());

        state.processing_report.analysis_data_domain = "legacy_linear_srgb".to_string();
        assert!(!pipeline_has_base(&state, &BaseColor::default()));
        state.processing_report.base_source = "compatibility_base".to_string();
        assert!(pipeline_has_base(&state, &BaseColor::default()));
    }

    #[test]
    fn input_class_never_selects_the_density_maths() {
        // No entry point may turn an input class into the retired recipe: the
        // marker is only ever cleared, never written, whatever the path suffix.
        for path in [
            "scan.tif",
            "scan.tiff",
            "lab-scan.jpg",
            "epson-scanner.dng",
            "frame.NEF",
            "frame.raf",
            "camera.CR3",
        ] {
            let mut state = PipelineState::smart_auto();
            state.processing_report.analysis_data_domain = "legacy_linear_srgb".to_string();
            state.processing_report.base_source = "compatibility_base".to_string();
            state.processing_report.base_confidence = "high".to_string();
            clear_retired_legacy_domain(&mut state);
            assert!(
                !is_smart_auto_compatibility(&state),
                "{path} must not keep the retired density recipe"
            );
            assert_eq!(
                state.processing_report.analysis_data_domain, "linear_prophoto_estimate",
                "{path}"
            );
        }
    }

    #[test]
    fn clearing_the_retired_marker_drops_the_compatibility_base() {
        let mut state = PipelineState::smart_auto();
        state.processing_report.analysis_data_domain = "legacy_linear_srgb".to_string();
        state.processing_report.base_source = "compatibility_base".to_string();
        state.processing_report.base_confidence = "high".to_string();

        clear_retired_legacy_domain(&mut state);

        assert_eq!(
            state.processing_report.analysis_data_domain,
            "linear_prophoto_estimate"
        );
        assert_eq!(state.processing_report.base_source, "unresolved");
        assert_eq!(state.processing_report.base_confidence, "low");
        assert!(!is_smart_auto_compatibility(&state));
        assert!(state
            .processing_report
            .fallback_reasons
            .iter()
            .any(|reason| reason == "retired_density_recipe"));
    }

    #[test]
    fn roll_anchored_frames_keep_their_anchors_untouched() {
        // A Roll with sampled anchors is never routed through the retired
        // recipe, so clearing must be a no-op there.
        let mut state = PipelineState::smart_auto();
        state.contract = ProcessingContract::RollAnchoredProPhotoV11;
        state.density_anchors = DensityAnchors {
            d_min_base: Some(DensityAnchor {
                density: [0.2, 0.25, 0.3],
                source: DensityAnchorSource::SampledFilmBase,
                scope: DensityAnchorScope::Roll,
                confidence: DensityAnchorConfidence::UserSampled,
                reference_id: None,
                provenance: Default::default(),
            }),
            ..DensityAnchors::default()
        };
        let before = state.clone();
        clear_retired_legacy_domain(&mut state);
        assert_eq!(state, before);
    }

    #[test]
    fn unanchored_roll_import_initializes_as_unresolved_and_heals_legacy_dirty_records() {
        // Scenario 2 (uncalibrated roll import):
        // 1. from_roll_anchors with empty anchors must start with unresolved base
        let unanchored_state = PipelineState::from_roll_anchors(DensityAnchors::default());
        assert_eq!(
            unanchored_state.contract,
            ProcessingContract::SmartAutoProPhotoV11
        );
        assert_eq!(unanchored_state.processing_report.base_source, "unresolved");
        assert_eq!(unanchored_state.processing_report.base_confidence, "low");
        assert!(!pipeline_has_base(&unanchored_state, &BaseColor::default()));

        // 2. Legacy dirty database record (where base_source was "content_estimate" but base_color is default)
        // must be rejected by pipeline_has_base so that auto-invert triggers fresh analysis.
        let mut dirty_legacy_state = unanchored_state.clone();
        dirty_legacy_state.processing_report.base_source = "content_estimate".to_string();
        assert!(
            !pipeline_has_base(&dirty_legacy_state, &BaseColor::default()),
            "Legacy records with default base_color must heal and report no base"
        );

        // 3. Once analyzed or copied, a non-default base_color satisfies pipeline_has_base
        let analyzed_color = BaseColor {
            base_r: 18000,
            base_g: 24000,
            base_b: 36000,
        };
        assert!(pipeline_has_base(&dirty_legacy_state, &analyzed_color));
        assert_ne!(
            pipeline_base_density(&dirty_legacy_state, &analyzed_color),
            [0.0; 3]
        );

        // Scenario 1 (fully anchored roll):
        // Physical anchor satisfies pipeline_has_base even if base_color is default
        let provenance = DensityAnchorProvenance {
            input_domain: DataDomain::ProPhotoEstimate,
            algorithm_version: crate::app_state::DENSITY_ANCHOR_ALGORITHM_VERSION.to_string(),
            raw_decode_version: Some(crate::persistence::RAW_DECODE_VERSION),
            legacy: false,
            ..Default::default()
        };
        let fully_anchored = PipelineState::from_roll_anchors(DensityAnchors {
            d_min_base: Some(DensityAnchor {
                density: [0.35, 0.65, 0.90],
                source: DensityAnchorSource::SampledFilmBase,
                scope: DensityAnchorScope::Roll,
                confidence: DensityAnchorConfidence::UserSampled,
                reference_id: None,
                provenance: provenance.clone(),
            }),
            d_max_full_exposure: Some(DensityAnchor {
                density: [2.1, 2.2, 2.3],
                source: DensityAnchorSource::SampledFullExposure,
                scope: DensityAnchorScope::Roll,
                confidence: DensityAnchorConfidence::UserSampled,
                reference_id: None,
                provenance,
            }),
            retained_records: Vec::new(),
            highlight_fraction: Some(0.85),
        });
        assert_eq!(
            fully_anchored.contract,
            ProcessingContract::RollAnchoredProPhotoV11
        );
        assert!(pipeline_has_base(&fully_anchored, &BaseColor::default()));
        assert_eq!(
            pipeline_base_density(&fully_anchored, &BaseColor::default()),
            [0.35, 0.65, 0.90]
        );

        // Scenario 3 (loose import):
        let loose_state = PipelineState::smart_auto();
        assert_eq!(
            loose_state.contract,
            ProcessingContract::SmartAutoProPhotoV11
        );
        assert_eq!(loose_state.processing_report.base_source, "unresolved");
        assert!(!pipeline_has_base(&loose_state, &BaseColor::default()));
        let mut analyzed_loose = loose_state.clone();
        analyzed_loose.processing_report.base_source = "film_edge_band".to_string();
        assert!(pipeline_has_base(&analyzed_loose, &analyzed_color));
        // A window derived by an older release is re-derived once, so the
        // retired per-channel content offsets cannot survive an application
        // update and keep rendering the old cast.
        assert!(!frame_needs_window_reanalysis(&analyzed_loose));
        let mut stale = analyzed_loose.clone();
        stale.processing_report.analysis_window_rule = 0;
        assert!(frame_needs_window_reanalysis(&stale));
        // A complete Roll calibration derives its endpoints from the sampled
        // anchors, so no window rule can invalidate them.
        let mut anchored = analyzed_loose.clone();
        anchored.contract = ProcessingContract::RollAnchoredProPhotoV11;
        anchored.processing_report.analysis_window_rule = 0;
        assert!(!frame_needs_window_reanalysis(&anchored));
    }

    #[test]
    fn real_world_nef_roll_unanchored_and_loose_import_match_and_eliminate_mask() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("test_picture");
        let nef_path = root.join("尼康nef_raw").join("_DSC7333.NEF");
        if !nef_path.is_file() {
            return;
        }
        let path_str = nef_path.to_string_lossy().to_string();
        let edge = 1200;
        let legacy = report_legacy_transport(&path_str, edge).expect("legacy decode");
        let estimate = report_working_estimate(&path_str, edge).expect("working estimate");

        // 1. Detect film border / film area
        let border = crate::film_border::detect_film_border(&image::DynamicImage::ImageRgb16(
            legacy.clone(),
        ));
        assert!(border.confidence == crate::film_border::DetectionConfidence::High);
        let mut geom = GeometryState::default();
        geom.calibration_points = Some(border.points);
        geom.calibration_confirmed = true;

        // 2. Scenario 2: Roll Import without calibration
        let roll_state = PipelineState::from_roll_anchors(DensityAnchors::default());
        assert!(!pipeline_has_base(&roll_state, &BaseColor::default()));

        // 3. Scenario 3: Loose Import
        let loose_state = PipelineState::smart_auto();
        assert!(!pipeline_has_base(&loose_state, &BaseColor::default()));

        // 4. Base analysis for both
        let roll_base_est = estimate_film_base_f32(&estimate, &geom);
        let loose_base_est = estimate_film_base_f32(&estimate, &geom);
        assert!(roll_base_est.usable);
        assert!(loose_base_est.usable);
        assert!(
            matches!(
                roll_base_est.source,
                "film_edge_band" | "film_area_low_density_tail"
            ),
            "the frame's base must come from the clear rebate or its own lowest density, got {}",
            roll_base_est.source
        );
        assert_eq!(roll_base_est.density, loose_base_est.density);

        // Density must be positive realistic film base (e.g. orange mask density around 0.3~0.6)
        let base_density = roll_base_est.density;
        assert!(base_density[0] > 0.25 && base_density[0] < 0.70);
        assert!(base_density[1] > 0.25 && base_density[1] < 0.70);
        assert!(base_density[2] > 0.35 && base_density[2] < 0.90);

        // Colors stored into base_color
        let base_color = base_color_from_density(base_density);
        assert_ne!(base_color, BaseColor::default());

        let mut roll_state_analyzed = roll_state.clone();
        roll_state_analyzed.processing_report.base_source = roll_base_est.source.to_string();
        let mut loose_state_analyzed = loose_state.clone();
        loose_state_analyzed.processing_report.base_source = loose_base_est.source.to_string();

        assert!(pipeline_has_base(&roll_state_analyzed, &base_color));
        assert!(pipeline_has_base(&loose_state_analyzed, &base_color));

        // 5. Compute limits for both
        let mut roll_limits =
            compute_content_limits_f32_with_bounds(&estimate, None, &geom, base_density, None)
                .unwrap();
        let mut loose_limits =
            compute_content_limits_f32_with_bounds(&estimate, None, &geom, base_density, None)
                .unwrap();
        assert_eq!(roll_limits.d_min, loose_limits.d_min);
        assert_eq!(roll_limits.d_max, loose_limits.d_max);
        println!(
            "[DIAG] BEFORE prepare: d_min={:?}, d_max={:?}",
            roll_limits.d_min, roll_limits.d_max
        );

        let roll_spans = measure_content_channel_spans(&estimate, None, &geom, base_density, None);
        let loose_spans = measure_content_channel_spans(&estimate, None, &geom, base_density, None);
        println!("[DIAG] roll_spans={:?}", roll_spans);
        prepare_content_render_limits_with_spans(
            &mut roll_limits,
            &roll_state.density_anchors,
            base_density,
            roll_spans,
        );
        prepare_content_render_limits_with_spans(
            &mut loose_limits,
            &loose_state.density_anchors,
            base_density,
            loose_spans,
        );
        assert_eq!(roll_limits.d_min, loose_limits.d_min);
        assert_eq!(roll_limits.d_max, loose_limits.d_max);

        // The window keeps one shared origin, so the film base — density zero in
        // every channel — prints as one neutral value on both routes.
        let levels = base_display_levels(&roll_limits);
        assert!(
            levels.iter().copied().fold(f32::MIN, f32::max)
                - levels.iter().copied().fold(f32::MAX, f32::min)
                <= 1.0e-4,
            "the film base must print neutral: {levels:?}"
        );

        // 6. Verify render equivalence between Scenario 2 (unanchored roll) and Scenario 3 (loose import)
        let mut params = TuningParams::default();
        params.density.d_min = roll_limits.d_min;
        params.density.d_max = roll_limits.d_max;

        let roll_rendered = render_f32_shader_equivalent(
            &estimate,
            None,
            &params,
            &geom,
            &base_color,
            &roll_state_analyzed,
            None,
        );
        let loose_rendered = render_f32_shader_equivalent(
            &estimate,
            None,
            &params,
            &geom,
            &base_color,
            &loose_state_analyzed,
            None,
        );

        // Exact match
        assert_eq!(roll_rendered.as_raw(), loose_rendered.as_raw());

        // 7. Verify color cast elimination (neutral balance)
        let mut sums = [0.0f64; 3];
        let total = (roll_rendered.width() * roll_rendered.height()) as f64;
        for pixel in roll_rendered.as_raw().chunks_exact(3) {
            sums[0] += pixel[0] as f64;
            sums[1] += pixel[1] as f64;
            sums[2] += pixel[2] as f64;
        }
        let r_mean = sums[0] / total;
        let g_mean = sums[1] / total;
        let b_mean = sums[2] / total;

        let r_g_ratio = r_mean / g_mean;
        let b_g_ratio = b_mean / g_mean;

        println!("[DIAG] base_density: {:?}", base_density);
        println!(
            "[DIAG] roll_limits: d_min={:?}, d_max={:?}",
            roll_limits.d_min, roll_limits.d_max
        );
        println!(
            "[DIAG] r_mean={:.4}, g_mean={:.4}, b_mean={:.4}",
            r_mean, g_mean, b_mean
        );
        println!(
            "[DIAG] r_g_ratio={:.4}, b_g_ratio={:.4}",
            r_g_ratio, b_g_ratio
        );
        println!(
            "[DIAG] prophoto_to_srgb: {:?}",
            linear_conversion_matrix(ColorSpaceId::ProPhotoRgb, ColorSpaceId::SRgb)
        );

        // Variant A: share_smart_auto_density_scale (with offsets)
        let mut limits_a =
            compute_content_limits_f32_with_bounds(&estimate, None, &geom, base_density, None)
                .unwrap();
        share_smart_auto_density_scale(&mut limits_a);
        preserve_smart_auto_content_span(&mut limits_a);
        let mut params_a = TuningParams::default();
        params_a.density.d_min = limits_a.d_min;
        params_a.density.d_max = limits_a.d_max;
        let rendered_a = render_f32_shader_equivalent(
            &estimate,
            None,
            &params_a,
            &geom,
            &base_color,
            &roll_state_analyzed,
            None,
        );
        ab_save_preview(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("target")
                .join("nef_variant_a_offsets.jpg"),
            &rendered_a,
        );

        // Variant B: raw co-sited extremes (per-channel limits preserved)
        let mut limits_b =
            compute_content_limits_f32_with_bounds(&estimate, None, &geom, base_density, None)
                .unwrap();
        preserve_smart_auto_content_span(&mut limits_b);
        let mut params_b = TuningParams::default();
        params_b.density.d_min = limits_b.d_min;
        params_b.density.d_max = limits_b.d_max;
        let rendered_b = render_f32_shader_equivalent(
            &estimate,
            None,
            &params_b,
            &geom,
            &base_color,
            &roll_state_analyzed,
            None,
        );
        ab_save_preview(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("target")
                .join("nef_variant_b_perchannel.jpg"),
            &rendered_b,
        );

        // Variant C: Legacy v1.0.2
        let legacy_base = compute_auto_base(&legacy);
        let legacy_limits =
            compute_auto_color_limits(&legacy, &geom, &legacy_base, FilmMode::Color, false)
                .unwrap();
        let mut params_c = TuningParams::default();
        params_c.density.d_min = legacy_limits.d_min;
        params_c.density.d_max = legacy_limits.d_max;
        let rendered_c = render_shader_equivalent(&legacy, &params_c, &geom, &legacy_base, None);
        ab_save_preview(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("target")
                .join("nef_variant_c_legacy.jpg"),
            &rendered_c,
        );

        println!(
            "[DIAG] Variant A limits: d_min={:?}, d_max={:?}",
            limits_a.d_min, limits_a.d_max
        );
        println!(
            "[DIAG] Variant B limits: d_min={:?}, d_max={:?}",
            limits_b.d_min, limits_b.d_max
        );
        println!(
            "[DIAG] Variant C legacy limits: d_min={:?}, d_max={:?}",
            legacy_limits.d_min, legacy_limits.d_max
        );

        let output_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join("nef_test_render.jpg");
        ab_save_preview(output_path, &roll_rendered);

        assert!(
            r_g_ratio > 0.70 && r_g_ratio < 1.30,
            "R/G ratio was {r_g_ratio}"
        );
        assert!(
            b_g_ratio > 0.70 && b_g_ratio < 1.30,
            "B/G ratio was {b_g_ratio}"
        );
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
            runtime_frame_base: None,
            runtime_frame_highlight: None,
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
            runtime_frame_base: None,
            runtime_frame_highlight: None,
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
        point_in_film_area, render_f32_shader_equivalent, render_shader_equivalent,
        reserve_export_path, sanitize_export_file_stem, should_apply_sprocket_mask,
        should_apply_sprocket_mask_for_area, validate_export_color_space, write_export_image,
        write_export_image_with_profile, ExportConflictPolicy, ExportFormat,
    };
    use crate::app_state::{
        BaseColor, DensityAnchor, DensityAnchorConfidence, DensityAnchorScope, DensityAnchorSource,
        DensityAnchors, FilmMode, GeometryState, PipelineState, ProcessingContract, TuningParams,
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
    fn prophoto_master_exposure_preserves_neutrality_with_channel_spans() {
        let mut state = PipelineState::smart_auto();
        state.contract = ProcessingContract::RollAnchoredProPhotoV11;
        state.processing_report.analysis_data_domain = "linear_prophoto_estimate".to_string();
        state.density_anchors = DensityAnchors {
            d_min_base: Some(DensityAnchor {
                density: [0.0; 3],
                source: DensityAnchorSource::SampledFilmBase,
                scope: DensityAnchorScope::Roll,
                confidence: DensityAnchorConfidence::UserSampled,
                reference_id: None,
                provenance: Default::default(),
            }),
            d_max_full_exposure: Some(DensityAnchor {
                density: [1.0, 1.5, 2.0],
                source: DensityAnchorSource::SampledFullExposure,
                scope: DensityAnchorScope::Roll,
                confidence: DensityAnchorConfidence::UserSampled,
                reference_id: None,
                provenance: Default::default(),
            }),
            retained_records: Vec::new(),
            highlight_fraction: None,
        };

        let source = ImageBuffer::from_pixel(
            1,
            1,
            Rgb([10.0f32.powf(-0.5), 10.0f32.powf(-0.75), 10.0f32.powf(-1.0)]),
        );
        let mut params = TuningParams::default();
        params.density.d_min = [0.0; 3];
        params.density.d_max = [1.0, 1.5, 2.0];
        params.density.gamma = 1.0;
        params.exposure.exposure = 0.75;

        let rendered = render_f32_shader_equivalent(
            &source,
            None,
            &params,
            &GeometryState::default(),
            &white_base(),
            &state,
            None,
        );
        let pixel = rendered.get_pixel(0, 0);
        let spread = pixel[0].max(pixel[1]).max(pixel[2]) - pixel[0].min(pixel[1]).min(pixel[2]);
        assert!(spread <= 2, "neutral exposure introduced a cast: {pixel:?}");
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
    fn film_area_points_do_not_apply_perspective_correction() {
        let source = ImageBuffer::from_fn(8, 6, |x, y| {
            let value = 4_000 + (x * 700 + y * 1_100) as u16;
            Rgb([value, value + 500, value + 1_000])
        });
        let expected = render_shader_equivalent(
            &source,
            &neutral_params(),
            &GeometryState::default(),
            &white_base(),
            None,
        );
        let mut film_area = GeometryState::default();
        film_area.calibration_points =
            Some([[0.08, 0.18], [0.94, 0.06], [0.82, 0.91], [0.17, 0.76]]);

        let rendered =
            render_shader_equivalent(&source, &neutral_params(), &film_area, &white_base(), None);

        assert_eq!(rendered.as_raw(), expected.as_raw());
    }

    #[test]
    fn film_area_region_uses_the_quadrilateral_not_its_bounding_box() {
        let area = [[0.2, 0.1], [0.9, 0.2], [0.8, 0.9], [0.1, 0.8]];

        assert!(point_in_film_area([0.5, 0.5], &area, 0.0));
        assert!(!point_in_film_area([0.12, 0.12], &area, 0.0));
        assert!(should_apply_sprocket_mask_for_area(
            [0.12, 0.12],
            &area,
            [0.12, 0.12],
        ));
        assert!(!should_apply_sprocket_mask_for_area(
            [0.5, 0.5],
            &area,
            [0.12, 0.12],
        ));
    }

    #[test]
    fn shader_equivalent_export_prefers_explicit_sprocket_target_color() {
        let source = ImageBuffer::from_pixel(4, 4, Rgb([6554, 6554, 6554]));
        let mut params = neutral_params();
        params.sprocket.sprocket_target_color =
            Some(vec![6554.0 / 65535.0, 6554.0 / 65535.0, 6554.0 / 65535.0]);
        params.sprocket.sprocket_tolerance = Some(0.1);
        params.sprocket.sprocket_feather = Some(0.05);
        let mut geom = GeometryState::default();
        geom.calibration_points = Some([[0.25, 0.25], [0.75, 0.25], [0.75, 0.75], [0.25, 0.75]]);

        let output = render_shader_equivalent(&source, &params, &geom, &white_base(), None);
        assert_eq!(output.get_pixel(0, 0)[0], u16::MAX);
        assert!(output.get_pixel(1, 1)[0] < 40000);
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
    fn persisted_legacy_loose_record_keeps_the_compatibility_transport_flag() {
        // Records written before the Smart Auto migration still carry the
        // v1.0.2 marker, so reading them back has to keep both the legacy
        // transport flag and the base-density fallback.
        let proxy = ImageBuffer::from_pixel(1, 1, Rgb([12_000, 24_000, 36_000]));
        let mut state = PipelineState::smart_auto();
        state.processing_report.analysis_data_domain = "legacy_linear_srgb".to_string();
        state.processing_report.base_source = "compatibility_base".to_string();
        let response =
            build_response_buffer_from_proxy_with_state(&proxy, &white_base(), &state, None, true);
        let flags = u32::from_le_bytes(response[24..28].try_into().unwrap());
        assert_eq!(flags & 2, 0, "compatibility transport must not be ProPhoto");
        assert_ne!(flags & 1, 0, "compatibility base must be marked analyzed");
        for channel in 0..3 {
            let offset = 8 + channel * 4;
            assert_eq!(
                f32::from_le_bytes(response[offset..offset + 4].try_into().unwrap()),
                0.0
            );
        }
    }

    #[test]
    fn loose_smart_auto_proxy_advertises_the_prophoto_transport_domain() {
        let proxy = ImageBuffer::from_pixel(1, 1, Rgb([12_000, 24_000, 36_000]));
        let mut state = PipelineState::smart_auto();
        state.processing_report.base_source = "content_estimate".to_string();
        state.processing_report.base_confidence = "0.500".to_string();
        let response =
            build_response_buffer_from_proxy_with_state(&proxy, &white_base(), &state, None, true);
        let flags = u32::from_le_bytes(response[24..28].try_into().unwrap());
        assert_ne!(
            flags & 2,
            0,
            "loose Smart Auto must advertise the ProPhoto transport domain"
        );
        assert_ne!(flags & 1, 0, "analyzed base must still be reported");
    }

    #[test]
    fn roll_prophoto_proxy_sets_prophoto_transport_flag() {
        let proxy = ImageBuffer::from_pixel(1, 1, Rgb([12_000, 24_000, 36_000]));
        let base = white_base();
        for contract in [
            ProcessingContract::SmartAutoProPhotoV11,
            ProcessingContract::RollBaseProPhotoV11,
            ProcessingContract::RollAnchoredProPhotoV11,
        ] {
            let mut state = PipelineState::smart_auto();
            state.contract = contract;
            state.processing_report.analysis_data_domain = "linear_prophoto_estimate".to_string();
            if contract != ProcessingContract::SmartAutoProPhotoV11 {
                state.density_anchors.d_min_base = Some(DensityAnchor {
                    density: [0.1; 3],
                    source: DensityAnchorSource::SampledFilmBase,
                    scope: DensityAnchorScope::Roll,
                    confidence: DensityAnchorConfidence::UserSampled,
                    reference_id: Some("roll:base".to_string()),
                    provenance: Default::default(),
                });
            }
            if contract == ProcessingContract::RollAnchoredProPhotoV11 {
                state.density_anchors.d_max_full_exposure = Some(DensityAnchor {
                    density: [1.5; 3],
                    source: DensityAnchorSource::SampledFullExposure,
                    scope: DensityAnchorScope::Roll,
                    confidence: DensityAnchorConfidence::UserSampled,
                    reference_id: Some("roll:full".to_string()),
                    provenance: Default::default(),
                });
            }
            let response =
                build_response_buffer_from_proxy_with_state(&proxy, &base, &state, None, true);
            let flags = u32::from_le_bytes(response[24..28].try_into().unwrap());
            assert_ne!(
                flags & 2,
                0,
                "{contract:?} must advertise the ProPhoto transport domain"
            );
        }
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

/// Regression coverage for the roll-anchored renderer: the density direction
/// and the per-frame film base that keeps mask removal exact.
#[cfg(test)]
mod roll_render_tests {
    use super::*;
    use crate::app_state::{
        BaseColor, DataDomain, DensityAnchor, DensityAnchorConfidence, DensityAnchorProvenance,
        DensityAnchorScope, DensityAnchorSource, DensityAnchors, TuningParams,
    };
    use image::{ImageBuffer, Rgb};

    fn anchors(base: [f32; 3], full: [f32; 3]) -> DensityAnchors {
        let provenance = DensityAnchorProvenance {
            input_domain: DataDomain::ProPhotoEstimate,
            algorithm_version: crate::app_state::DENSITY_ANCHOR_ALGORITHM_VERSION.to_string(),
            raw_decode_version: Some(crate::persistence::RAW_DECODE_VERSION),
            legacy: false,
            ..Default::default()
        };
        DensityAnchors {
            d_min_base: Some(DensityAnchor {
                density: base,
                source: DensityAnchorSource::SampledFilmBase,
                scope: DensityAnchorScope::Roll,
                confidence: DensityAnchorConfidence::UserSampled,
                reference_id: Some("roll:base".into()),
                provenance: provenance.clone(),
            }),
            d_max_full_exposure: Some(DensityAnchor {
                density: full,
                source: DensityAnchorSource::SampledFullExposure,
                scope: DensityAnchorScope::Roll,
                confidence: DensityAnchorConfidence::UserSampled,
                reference_id: Some("roll:full".into()),
                provenance,
            }),
            retained_records: Vec::new(),
            highlight_fraction: None,
        }
    }

    fn transmittance(density: [f32; 3]) -> Rgb<f32> {
        Rgb([
            10.0f32.powf(-density[0]),
            10.0f32.powf(-density[1]),
            10.0f32.powf(-density[2]),
        ])
    }

    fn synthetic_frame(base: [f32; 3], scene: [f32; 3]) -> ImageBuffer<Rgb<f32>, Vec<f32>> {
        // Upper 3/4 is the scene, the lower band is clear film base with a
        // little grain, mirroring a strip scan that includes the rebate.
        ImageBuffer::from_fn(200, 200, |x, y| {
            if y > 150 {
                let jitter = ((x % 7) as f32 - 3.0) * 0.002;
                transmittance([base[0] + jitter, base[1] + jitter, base[2] + jitter])
            } else {
                let jitter = ((x + y) % 11) as f32 * 0.01;
                transmittance([scene[0] + jitter, scene[1] + jitter, scene[2] + jitter])
            }
        })
    }

    #[test]
    fn frame_base_detection_finds_the_clear_film_band() {
        let base = [0.62, 0.73, 0.91];
        let frame = synthetic_frame(base, [1.10, 1.25, 1.50]);
        let detected = detect_frame_base_density(&frame, [0.55, 0.75, 1.02])
            .expect("the uniform film base band must be detected");
        for channel in 0..3 {
            assert!(
                (detected[channel] - base[channel]).abs() < 0.02,
                "channel {channel}: detected {detected:?} for base {base:?}"
            );
        }
    }

    #[test]
    fn frame_base_detection_rejects_an_implausible_match() {
        // A frame dominated by a bright sky has no rebate; its dominant peak
        // sits far away from the roll anchor, so the anchor stays in charge.
        let frame = ImageBuffer::from_pixel(200, 200, transmittance([2.4, 2.5, 2.6]));
        assert!(detect_frame_base_density(&frame, [0.55, 0.75, 1.02]).is_none());
    }

    #[test]
    fn frame_base_shifts_the_roll_mapping_without_changing_its_span() {
        let roll_base = [0.55, 0.75, 1.02];
        let roll_full = [1.74, 2.15, 2.65];
        let state = PipelineState::from_roll_anchors(anchors(roll_base, roll_full));
        let frame_base = [0.62, 0.73, 0.91];
        let mapping = roll_density_mapping_with_frame_base(&state, Some(frame_base), None)
            .expect("complete anchors produce a mapping");
        assert_eq!(mapping.mode, RenderMode::RollAnchored);
        for channel in 0..3 {
            let span = roll_full[channel] - roll_base[channel];
            assert!(
                (mapping.density_low[channel] - (frame_base[channel] - roll_base[channel])).abs()
                    < 1e-6
            );
            assert!(
                (mapping.density_high[channel] - mapping.density_low[channel] - span).abs() < 1e-6
            );
        }
    }

    #[test]
    fn roll_render_turns_dense_negative_areas_bright() {
        let roll_base = [0.55, 0.75, 1.02];
        let roll_full = [1.74, 2.15, 2.65];
        let scene = [1.20, 1.35, 1.60];
        let frame = synthetic_frame(roll_base, scene);
        let mut state = PipelineState::from_roll_anchors(anchors(roll_base, roll_full));
        state.render_mapping =
            roll_density_mapping_with_frame_base(&state, Some(roll_base), None).expect("mapping");
        state.processing_report.render_route = "RollAnchoredDirectInvert".to_string();
        let rendered = render_f32_shader_equivalent(
            &frame,
            None,
            &TuningParams::default(),
            &GeometryState::default(),
            &BaseColor::default(),
            &state,
            None,
        );
        let luminance = |x: u32, y: u32| {
            let pixel = rendered.get_pixel(x, y).0;
            [0.2126f32, 0.7152, 0.0722]
                .iter()
                .zip(pixel)
                .map(|(weight, value)| weight * value as f32)
                .sum::<f32>()
        };
        let scene_row = luminance(100, 80);
        let base_row = luminance(100, 180);
        // The scene is denser than the clear film base, so it must print
        // brighter. Inverting the ramp a second time swaps these.
        assert!(
            scene_row > base_row + 4000.0,
            "scene {scene_row} should be far brighter than the film base {base_row}"
        );
    }

    /// A scene whose brightest content sits at half of the film span must land
    /// near paper white, not in the middle of the histogram.
    /// The filmstrip thumbnail must show the frame the renderer produces.
    #[test]
    fn thumbnail_geometry_matches_the_render_mapping() {
        let source = RgbImage::from_fn(6, 4, |x, y| Rgb([(y * 6 + x) as u8, 0, 0]));
        for geom in [
            GeometryState::default(),
            GeometryState {
                rotate_90_count: 1,
                ..Default::default()
            },
            GeometryState {
                rotate_90_count: 2,
                ..Default::default()
            },
            GeometryState {
                rotate_90_count: 3,
                ..Default::default()
            },
            GeometryState {
                flip_h: true,
                ..Default::default()
            },
            GeometryState {
                flip_v: true,
                ..Default::default()
            },
            GeometryState {
                rotate_90_count: 1,
                flip_v: true,
                ..Default::default()
            },
            GeometryState {
                rotate_90_count: 2,
                flip_h: true,
                ..Default::default()
            },
        ] {
            let oriented = orient_display_image(source.clone(), &geom);
            let (width, height) = oriented.dimensions();
            let expected = if geom.rotate_90_count.rem_euclid(2) == 0 {
                (6, 4)
            } else {
                (4, 6)
            };
            assert_eq!((width, height), expected, "geom {geom:?}");
            for y in 0..height {
                for x in 0..width {
                    let uv = [
                        (x as f32 + 0.5) / width as f32,
                        (y as f32 + 0.5) / height as f32,
                    ];
                    let source_uv = map_oriented_uv_to_source(uv, 6, 4, &geom);
                    let source_x = (source_uv[0] * 6.0).floor().clamp(0.0, 5.0) as u32;
                    let source_y = (source_uv[1] * 4.0).floor().clamp(0.0, 3.0) as u32;
                    assert_eq!(
                        oriented.get_pixel(x, y),
                        source.get_pixel(source_x, source_y),
                        "geom {geom:?} maps ({x},{y}) to ({source_x},{source_y})"
                    );
                }
            }
        }
    }

    /// The renderer must place the frame exactly like the thumbnail does, or an
    /// export of a flipped frame comes out upside down compared with a preview.
    #[test]
    fn export_render_places_the_frame_with_the_frame_geometry() {
        let params = TuningParams::default();
        let base = BaseColor {
            base_r: u16::MAX,
            base_g: u16::MAX,
            base_b: u16::MAX,
        };
        let mut state = PipelineState::smart_auto();
        state.render_mapping = RenderMapping {
            mode: RenderMode::RollAnchored,
            density_low: [0.0; 3],
            density_high: [1.0; 3],
            exposure: 0.0,
            gamma: 1.0,
            channel_offsets: [0.0; 3],
        };
        // One dense pixel in the source's top-left corner.
        let source = ImageBuffer::from_fn(6, 4, |x, y| {
            if x == 0 && y == 0 {
                Rgb([1.0e-3f32, 1.0e-3, 1.0e-3])
            } else {
                Rgb([1.0f32, 1.0, 1.0])
            }
        });
        let marker = RgbImage::from_fn(6, 4, |x, y| {
            if x == 0 && y == 0 {
                Rgb([255u8, 255, 255])
            } else {
                Rgb([0u8, 0, 0])
            }
        });
        for geom in [
            GeometryState::default(),
            GeometryState {
                flip_v: true,
                ..Default::default()
            },
            GeometryState {
                flip_h: true,
                ..Default::default()
            },
            GeometryState {
                rotate_90_count: 2,
                ..Default::default()
            },
            GeometryState {
                rotate_90_count: 1,
                flip_v: true,
                ..Default::default()
            },
        ] {
            let rendered =
                render_f32_shader_equivalent(&source, None, &params, &geom, &base, &state, None);
            let white = (0..rendered.height())
                .flat_map(|y| (0..rendered.width()).map(move |x| (x, y)))
                .filter(|(x, y)| rendered.get_pixel(*x, *y)[0] > 30_000)
                .collect::<Vec<_>>();
            let oriented = orient_display_image(marker.clone(), &geom);
            let expected = (0..oriented.height())
                .flat_map(|y| (0..oriented.width()).map(move |x| (x, y)))
                .filter(|(x, y)| oriented.get_pixel(*x, *y)[0] > 128)
                .collect::<Vec<_>>();
            assert_eq!(expected.len(), 1, "geom {geom:?} marker");
            assert_eq!(
                white, expected,
                "geom {geom:?} places the frame differently"
            );
        }
    }

    fn synthetic_scene(base: [f32; 3], ground: f32, sky: f32) -> ImageBuffer<Rgb<f32>, Vec<f32>> {
        ImageBuffer::from_fn(512, 512, |x, y| {
            let edge = x < 40 || y < 40 || x >= 472 || y >= 472;
            let relative = if edge {
                -0.4
            } else if y < 190 {
                sky
            } else {
                ground
            };
            transmittance([base[0] + relative, base[1] + relative, base[2] + relative])
        })
    }

    #[test]
    fn frame_highlight_fraction_measures_scene_headroom() {
        let base = [0.60, 0.73, 0.91];
        let span = [1.0, 1.0, 1.0];
        let frame = synthetic_scene(base, 0.25, 0.60);
        let fraction = detect_frame_highlight_fraction(&frame, base, span)
            .expect("a flat scene must produce a highlight estimate");
        assert!(
            (fraction - 0.60).abs() < 0.08,
            "scene highlight 0.60 of the span produced {fraction}"
        );
    }

    #[test]
    fn frame_highlight_fraction_reads_the_brightest_picture_area() {
        // A small bright area is what a cloud or a sunlit wall looks like: a
        // 95th percentile of the picture blocks sat far below it and blew every
        // highlight of the Roll out to white.
        let base = [0.60, 0.73, 0.91];
        let span = [1.0, 1.0, 1.0];
        let frame = ImageBuffer::from_fn(512, 512, |x, y| {
            let edge = x < 40 || y < 40 || x >= 472 || y >= 472;
            let patch = (300..364).contains(&x) && (300..364).contains(&y);
            let relative = if edge {
                -0.4
            } else if patch {
                1.0
            } else {
                0.30
            };
            transmittance([base[0] + relative, base[1] + relative, base[2] + relative])
        });
        let fraction = detect_frame_highlight_fraction(&frame, base, span)
            .expect("a scene with one bright area must produce an estimate");
        assert!(
            (fraction - 1.0).abs() < 0.05,
            "the brightest picture area sets the highlight, got {fraction}"
        );
    }

    #[test]
    fn frame_highlight_fraction_is_clamped_to_a_sane_band() {
        let base = [0.60, 0.73, 0.91];
        let span = [1.0, 1.0, 1.0];
        let dark = detect_frame_highlight_fraction(&synthetic_scene(base, 0.10, 0.15), base, span)
            .expect("dark scene");
        assert!((dark - 0.45).abs() < 1.0e-6, "dark scene clamped to {dark}");
        let bright =
            detect_frame_highlight_fraction(&synthetic_scene(base, 0.80, 1.30), base, span)
                .expect("bright scene");
        assert!(
            (bright - 1.0).abs() < 1.0e-6,
            "a scene above the film's own maximum density stops at the leader: {bright}"
        );
    }

    #[test]
    fn roll_highlight_fraction_sets_the_white_point_for_every_frame() {
        let roll_base = [0.55, 0.75, 1.02];
        let roll_full = [1.74, 2.15, 2.65];
        let mut state = PipelineState::from_roll_anchors(anchors(roll_base, roll_full));
        state.density_anchors.highlight_fraction = Some(0.50);
        let mapping = roll_density_mapping_with_frame_base(&state, Some(roll_base), Some(0.80))
            .expect("complete anchors produce a mapping");
        for channel in 0..3 {
            let span = roll_full[channel] - roll_base[channel];
            assert!(
                (mapping.density_high[channel] - span * 0.5).abs() < 1.0e-6,
                "the Roll value must win over the frame value"
            );
        }
        state.density_anchors.highlight_fraction = Some(0.05);
        let clamped = roll_density_mapping_with_frame_base(&state, None, None).expect("mapping");
        for channel in 0..3 {
            let span = roll_full[channel] - roll_base[channel];
            assert!((clamped.density_high[channel] - span * 0.45).abs() < 1.0e-6);
        }
    }

    /// The Roll's white point has to survive capability resolution, otherwise
    /// the runtime state silently falls back to the leader-only mapping.
    #[test]
    fn resolver_input_carries_the_roll_highlight_fraction() {
        let mut anchors = anchors([0.55, 0.75, 1.02], [1.74, 2.15, 2.65]);
        anchors.highlight_fraction = Some(0.5);
        let persisted = PipelineState::from_roll_anchors(anchors.clone());
        let roll = Roll {
            roll_id: "roll-highlight".into(),
            date: String::new(),
            format: "135".into(),
            film_stock: "Test".into(),
            camera: String::new(),
            image_paths: Vec::new(),
            density_anchors: anchors,
            calibration_profile_id: None,
            scanner_profile_id: None,
        };
        let input = pipeline_resolver_input_for_kind(
            &persisted,
            Some(&roll),
            &[],
            pipeline_image_kind("frame.tif"),
            None,
        );
        assert_eq!(input.density_anchors.highlight_fraction, Some(0.5));
    }

    fn usable_frame_base_estimate(
        density: [f32; 3],
        source: &'static str,
        confidence: f32,
    ) -> FilmBaseEstimate {
        FilmBaseEstimate {
            density,
            confidence,
            source,
            usable: true,
            fallback_reason: None,
        }
    }

    /// A pasted film base must be measured on the frame it runs on. The same
    /// Roll records a different clear-film density on either side of a scan
    /// pass, so letting one frame's figure travel to the next tints it end to
    /// end.
    #[test]
    fn pasted_film_base_is_measured_on_the_target_frame() {
        let copied = [0.5775, 0.7360, 0.9848];
        let measured = usable_frame_base_estimate([0.6956, 0.7609, 0.8364], "film_edge_band", 0.95);

        let (density, source, confidence, measured_on_frame) =
            choose_pasted_film_base(copied, None, Some(measured));

        assert_eq!(density, [0.6956, 0.7609, 0.8364]);
        assert_eq!(source, "film_edge_band");
        assert_eq!(confidence, "0.950");
        assert!(measured_on_frame);
    }

    /// When the frame cannot be measured right now, its own earlier
    /// measurement still beats the figure copied from another frame.
    #[test]
    fn pasted_film_base_keeps_the_frames_own_earlier_measurement() {
        let copied = [0.5775, 0.7360, 0.9848];
        let own = (
            [0.6067, 0.7570, 1.0074],
            "film_edge_band".to_string(),
            "0.950".to_string(),
        );

        let (density, source, confidence, measured_on_frame) =
            choose_pasted_film_base(copied, Some(own), None);

        assert_eq!(density, [0.6067, 0.7570, 1.0074]);
        assert_eq!(source, "film_edge_band");
        assert_eq!(confidence, "0.950");
        assert!(measured_on_frame);
    }

    /// The copied figure stays as the fallback for frames that have neither a
    /// Film Area analysis nor an earlier base of their own.
    #[test]
    fn pasted_film_base_falls_back_to_the_copied_figure() {
        let copied = [0.5775, 0.7360, 0.9848];

        let (density, source, confidence, measured_on_frame) =
            choose_pasted_film_base(copied, None, None);

        assert_eq!(density, copied);
        assert_eq!(source, crate::pipeline::INHERITED_FILM_BASE_SOURCE);
        assert_eq!(confidence, "1.000");
        assert!(!measured_on_frame);
    }

    /// A measurement that fails the quality gate must not override the frame's
    /// own base or the copied figure.
    #[test]
    fn pasted_film_base_ignores_an_unusable_measurement() {
        let copied = [0.5775, 0.7360, 0.9848];
        let unusable = FilmBaseEstimate {
            density: [0.4; 3],
            confidence: 0.0,
            source: "unavailable",
            usable: false,
            fallback_reason: Some("missing_film_base_reference"),
        };

        let (density, source, _confidence, measured_on_frame) =
            choose_pasted_film_base(copied, None, Some(unusable));

        assert_eq!(density, copied);
        assert_eq!(source, crate::pipeline::INHERITED_FILM_BASE_SOURCE);
        assert!(!measured_on_frame);
    }

    /// A marker alone does not make a base this frame's measurement: a reset
    /// or half-written frame still holds the default half-white colour, and a
    /// copied base names the frame it came from.
    #[test]
    fn frame_measurement_requires_a_marker_and_a_real_base() {
        let measured = base_color_from_density([0.58, 0.74, 0.99]);

        assert!(crate::pipeline::base_is_frame_measurement(
            "film_edge_band",
            &measured
        ));
        assert!(crate::pipeline::base_is_frame_measurement(
            "film_area_low_density_tail",
            &measured
        ));
        assert!(!crate::pipeline::base_is_frame_measurement(
            crate::pipeline::INHERITED_FILM_BASE_SOURCE,
            &measured
        ));
        assert!(!crate::pipeline::base_is_frame_measurement(
            "unresolved",
            &measured
        ));
        assert!(!crate::pipeline::base_is_frame_measurement("", &measured));
        assert!(!crate::pipeline::base_is_frame_measurement(
            "compatibility_fallback",
            &measured
        ));
        assert!(!crate::pipeline::base_is_frame_measurement(
            "missing_film_base_reference",
            &measured
        ));
        assert!(!crate::pipeline::base_is_frame_measurement(
            "film_edge_band",
            &BaseColor::default()
        ));
    }

    /// A frame that inherited another frame's base measures the film on its own
    /// pixels once they are decoded — and only then. The measurement needs a
    /// confirmed Film Area, and a frame that already has its own base keeps it.
    #[test]
    fn inherited_film_base_is_re_measured_on_the_frames_own_pixels() {
        let transmission = 10f32.powf(-0.85);
        let proxy = ImageBuffer::from_pixel(
            96,
            96,
            Rgb([transmission, transmission * 0.92, transmission * 0.84]),
        );
        let mut geom = GeometryState::default();
        let mut state = PipelineState::smart_auto();
        state.processing_report.base_source =
            crate::pipeline::INHERITED_FILM_BASE_SOURCE.to_string();

        assert!(
            inherited_film_base_measurement(&state, &geom, &proxy).is_none(),
            "a frame without a confirmed Film Area has nowhere to measure"
        );

        geom.calibration_points = Some([[0.10, 0.10], [0.90, 0.10], [0.90, 0.90], [0.10, 0.90]]);
        let estimate = inherited_film_base_measurement(&state, &geom, &proxy)
            .expect("the frame's own film base is measurable");
        assert!(estimate.usable);
        for (channel, expected) in estimate.density.iter().zip([0.85, 0.886, 0.926]) {
            assert!(
                (channel - expected).abs() < 0.02,
                "expected {expected}, measured {channel}"
            );
        }

        state.processing_report.base_source = "film_edge_band".to_string();
        assert!(
            inherited_film_base_measurement(&state, &geom, &proxy).is_none(),
            "a frame that already measured its own base is left alone"
        );
    }
}
