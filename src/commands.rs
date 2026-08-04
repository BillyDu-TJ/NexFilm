use crate::app_state::{
    BaseColor, EngineState, FilmItem, FilmMode, FilmstripItem, GeometryState, Roll, TuningParams,
};
use crate::batch_settings::{BatchCopyResult, ImageKey};
use crate::color_science::{
    apply_linear_matrix, canonical_output_space, convert_encoded_to_linear_rgb_with_matrix,
    identify_icc_profile, libraw_native_profile, linear_conversion_matrix, parse_output_space,
    ColorSpaceId, DENSITY_CAPTURE_PROFILE, DENSITY_CAPTURE_WORKING_SPACE,
};
use crate::core_math::{
    apply_homography, apply_perspective_uv, apply_post_gamma_adjustments_with_luma, density_luma,
    neutral_density_bounds, normalize_density_channel, shader_homography, sprocket_white_mask,
    DENSITY_LUMA_COEFFICIENTS,
};
use crate::persistence::{self, MATH_VERSION, RAW_DECODE_VERSION};
use crate::pipeline::FilmPipeline;
use serde::Serialize;

use base64::{engine::general_purpose, Engine as _};
use flate2::{read::ZlibDecoder, write::ZlibEncoder, Compression};
use image::{
    imageops::FilterType, GenericImageView, ImageBuffer, ImageOutputFormat, Rgb, RgbImage,
};
use rayon::prelude::*;
use rfd::FileDialog;
use rusqlite::OptionalExtension;
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::io::{BufReader, Cursor, Read, Seek, SeekFrom, Write};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock, RwLock};
use std::sync::{MutexGuard, RwLockReadGuard, RwLockWriteGuard};
use tauri::State;
use tauri::{Emitter, Manager};

static NEXT_ID: AtomicUsize = AtomicUsize::new(1);
static EXPORT_TEMP_ID: AtomicUsize = AtomicUsize::new(1);
static RAYON_INIT: OnceLock<()> = OnceLock::new();
static EXPORT_ACTIVE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
const EXPORT_TEMP_PREFIX: &str = ".nexfilm-part-";
const CHANNEL_CONTROL_SCALE: f32 = 0.5;
const LUT_CONTROL_SCALE: f32 = 0.5;

const FALLBACK_THUMB: &str = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mM8c+bMfwAIGwK9t856VAAAAABJRU5ErkJggg==";
const IMPORT_PREVIEW_LONG_EDGE: u32 = 1024;
const PROXY_LONG_EDGE: f32 = 2560.0;

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

fn is_scanner_fff_tiff(path: &str) -> bool {
    if !is_fff_extension(path) {
        return false;
    }
    let mut header = [0u8; 4];
    std::fs::File::open(path)
        .and_then(|mut file| file.read_exact(&mut header))
        .is_ok()
        && matches!(header, [b'I', b'I', 42, 0] | [b'M', b'M', 0, 42])
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

fn extract_tiff_icc_profile(bytes: &[u8]) -> Option<Vec<u8>> {
    let little = match bytes.get(0..2)? {
        b"II" => true,
        b"MM" => false,
        _ => return None,
    };
    if read_endian_u16(bytes, 2, little)? != 42 {
        return None;
    }
    let ifd_offset = usize::try_from(read_endian_u32(bytes, 4, little)?).ok()?;
    let entry_count = usize::from(read_endian_u16(bytes, ifd_offset, little)?);
    let entries_start = ifd_offset.checked_add(2)?;
    for index in 0..entry_count {
        let offset = entries_start.checked_add(index.checked_mul(12)?)?;
        let tag = read_endian_u16(bytes, offset, little)?;
        let type_code = read_endian_u16(bytes, offset + 2, little)?;
        let count = usize::try_from(read_endian_u32(bytes, offset + 4, little)?).ok()?;
        if tag != 34675 || type_code != 7 {
            continue;
        }
        if count <= 4 {
            return Some(bytes.get(offset + 8..offset + 8 + count)?.to_vec());
        }
        let value_offset = usize::try_from(read_endian_u32(bytes, offset + 8, little)?).ok()?;
        return Some(
            bytes
                .get(value_offset..value_offset.checked_add(count)?)?
                .to_vec(),
        );
    }
    None
}

fn embedded_input_profile(path: &str) -> Option<ColorSpaceId> {
    let bytes = std::fs::read(path).ok()?;
    let extension = std::path::Path::new(path)
        .extension()
        .and_then(|extension| extension.to_str())?
        .to_ascii_lowercase();
    let profile = match extension.as_str() {
        "jpg" | "jpeg" => extract_jpeg_icc_profile(&bytes),
        "png" => extract_png_icc_profile(&bytes),
        "tif" | "tiff" => extract_tiff_icc_profile(&bytes),
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
    if let Some(bytes) = extract_tiff_jpeg_preview(path, max_edge) {
        // Keep a display-ready JPEG untouched when it already fits the target.
        if bytes.len() <= 256 * 1024
            && jpeg_dimensions(&bytes).is_some_and(|(width, height)| width.max(height) <= max_edge)
        {
            return Some(general_purpose::STANDARD.encode(bytes));
        }
        if let Ok(image) = image::load_from_memory(&bytes) {
            return encode_preview_jpeg_base64(image, max_edge, 86);
        }
    }
    let mut processor = rawlib::RawProcessor::new().ok()?;
    processor.open_file(path).ok()?;
    processor.unpack_thumb().ok()?;
    let thumb = processor.get_thumbnail().ok()?;

    if thumb.format == rawlib::ImageFormat::Jpeg {
        return Some(encode_preview_bytes_base64(&thumb.data, max_edge, 86));
    }

    if thumb.format == rawlib::ImageFormat::Bitmap {
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

fn decode_fff_fallback_preview_base64(path: &str, max_edge: u32) -> Option<String> {
    let image = decode_image_buffer(path, DecodeMode::DevelopProxy).ok()?;
    encode_stretched_preview_jpeg_base64(image::DynamicImage::ImageRgb16(image), max_edge, 86)
}

fn decode_scanner_fff_preview_base64(path: &str, max_edge: u32) -> Option<String> {
    let preview = decode_scanner_fff_tiff_page(path, 1).ok()?;
    let preview_edge = preview.width().max(preview.height()).min(max_edge);
    let preview = image::DynamicImage::ImageRgb16(preview).to_rgb8();
    encode_preview_jpeg_base64(image::DynamicImage::ImageRgb8(preview), preview_edge, 86)
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
        return extract_embedded_preview_base64(path, max_edge)
            .or_else(|| decode_direct_image_preview_base64(path, max_edge));
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
    if is_direct_image_extension(path) {
        return decode_direct_image_preview_base64(path, max_edge);
    }
    if is_scanner_fff_tiff(path) {
        return decode_scanner_fff_preview_base64(path, max_edge)
            .or_else(|| decode_fff_fallback_preview_base64(path, max_edge));
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

fn compute_pristine_proxy(
    proxy: &ImageBuffer<Rgb<u16>, Vec<u16>>,
    base_color: &BaseColor,
    mode: FilmMode,
) -> ImageBuffer<Rgb<f32>, Vec<f32>> {
    let pipeline = FilmPipeline::new(
        [base_color.base_r, base_color.base_g, base_color.base_b],
        [0.0, 0.0, 0.0],
        mode,
    );
    let (width, height) = proxy.dimensions();
    let mut pristine = ImageBuffer::<Rgb<f32>, Vec<f32>>::new(width, height);

    let raw_pixels: &[u16] = proxy.as_raw().as_slice();
    let out_pixels: &mut [f32] = pristine.as_mut();

    raw_pixels
        .par_chunks(3)
        .zip(out_pixels.par_chunks_mut(3))
        .for_each(|(in_px, out_px)| {
            let linear_rgb = [
                (in_px[0] as f32) / 65535.0,
                (in_px[1] as f32) / 65535.0,
                (in_px[2] as f32) / 65535.0,
            ];
            let true_density = pipeline.compute_true_density(&linear_rgb);
            out_px[0] = true_density[0];
            out_px[1] = true_density[1];
            out_px[2] = true_density[2];
        });

    pristine
}

#[derive(Debug, Clone, Serialize)]
pub struct AutoColorLimits {
    pub d_min: [f32; 3],
    pub d_max: [f32; 3],
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
    Ok(AutoColorLimits { d_min, d_max })
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

fn libraw_output_color(colorspace: &str) -> Result<i32, String> {
    crate::color_science::libraw_output_color(colorspace)
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

    // RAW_DECODE_VERSION 5 contract: LibRaw 0.22.2 camera/RAF tables, unsigned
    // 16-bit output, camera white balance and matrix, linear gamma, fixed
    // brightness, and fixed linear-sRGB density-capture coordinates. Black and
    // white levels are applied by LibRaw before this buffer.
    let options = rawlib::DecodeOptions {
        half_size: mode == DecodeMode::DevelopProxy,
        demosaic_quality: 3,
        output_bps: 16,
        no_auto_bright: true,
        output_color: libraw_output_color(DENSITY_CAPTURE_WORKING_SPACE)?,
        linear_gamma: true,
        use_camera_wb: true,
    };
    let decoded =
        rawlib::RawProcessor::extract_image_with_options(path, &options).map_err(|error| {
            format!(
                "LibRaw {} cannot decode {path}: {error}",
                rawlib::RawProcessor::version()
            )
        })?;

    let decoded = rgb16_image_from_bytes(
        decoded.width as u32,
        decoded.height as u32,
        decoded.colors as usize,
        decoded.bits,
        &decoded.data,
    )?;
    let native_profile = libraw_native_profile(libraw_output_color(DENSITY_CAPTURE_WORKING_SPACE)?);
    Ok(convert_linear_image(
        decoded,
        native_profile,
        requested_profile,
    ))
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
        transaction
            .execute(
                "INSERT INTO image_states (
                     roll_id, file_path, thumbnail_base64, embedded_thumb_base64,
                     rendered_thumb_base64, params, geom, base_color,
                     math_version, raw_decode_version, updated_at
                 )
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
                 ON CONFLICT(roll_id, file_path) DO UPDATE SET
                 thumbnail_base64=excluded.thumbnail_base64,
                 embedded_thumb_base64=excluded.embedded_thumb_base64,
                 rendered_thumb_base64=COALESCE(excluded.rendered_thumb_base64, image_states.rendered_thumb_base64),
                 params=excluded.params,
                 geom=excluded.geom,
                 base_color=excluded.base_color,
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
                    MATH_VERSION,
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
            ),
        > = {
            let mut cache = std::collections::HashMap::new();
            if let Ok(conn) = persistence::open_connection() {
                conn.busy_timeout(std::time::Duration::from_secs(5)).ok();
                if let Ok(mut stmt) = conn.prepare(
                    "SELECT file_path,
                            COALESCE(embedded_thumb_base64, thumbnail_base64),
                            rendered_thumb_base64,
                            params, geom, base_color
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
                            ))
                        })
                    {
                        for row in rows.flatten() {
                            let (fp, embedded_thumb, rendered_thumb, params_str, geom_str, bc_str) =
                                row;
                            if let (Ok(params), Ok(geom), Ok(bc)) = (
                                serde_json::from_str(&params_str),
                                serde_json::from_str(&geom_str),
                                serde_json::from_str(&bc_str),
                            ) {
                                cache.insert(
                                    fp.replace("\\", "/").to_lowercase(),
                                    (embedded_thumb, rendered_thumb, params, geom, bc),
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
                if let Some((embedded_thumb, rendered_thumb, params, geom, base_color)) =
                    db_cache.get(&path.replace("\\", "/").to_lowercase())
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
                        pristine_proxy: None,
                        base_color: base_color.clone(),
                        params: params.clone(),
                        geom: normalize_persisted_geometry(geom.clone()),
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
                    pristine_proxy: None,
                    base_color: BaseColor::default(),
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
        let (result_tx, result_rx) = std::sync::mpsc::channel::<ImportWork>();
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
            queued_paths
                .par_iter()
                .for_each_with(result_tx_for_workers, |sender, path| {
                    let work = match process_path_for_workers(path) {
                        Ok(item) => ImportWork::Item(item),
                        Err(message) => ImportWork::Failed {
                            file_path: path.clone(),
                            message,
                        },
                    };
                    let _ = sender.send(work);
                });
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
        // Deliver previews as soon as each worker finishes.  The frontend
        // already owns the selection-order skeleton, so completion order does
        // not change layout while avoiding a slow first frame blocking every
        // later preview.
        while let Ok(item) = result_rx.recv() {
            if tx.send(item).is_err() {
                return;
            }
        }
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
                pristine_proxy: None,
                base_color: BaseColor::default(),
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
            },
            Roll {
                roll_id: "roll-b".into(),
                date: String::new(),
                format: "135".into(),
                film_stock: String::new(),
                camera: String::new(),
                image_paths: vec!["A\\First.DNG".into()],
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

#[derive(serde::Serialize)]
pub struct ActiveImageState {
    pub params: TuningParams,
    pub geom: crate::app_state::GeometryState,
    pub base_analyzed: bool,
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
    state: State<'_, EngineState>,
    _app_handle: tauri::AppHandle,
) -> Result<ActiveImageState, String> {
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

    let (_, params, geom, base_color) = persisted;
    let mut item = item_arc.write().map_err(|error| error.to_string())?;
    if item.roll_id != roll_id || item.file_path != file_path {
        return Err("Image identity changed while loading persisted state".into());
    }
    item.params = params;
    item.geom = geom;
    item.base_color = base_color;

    *state.active_id.write().map_err(|e| e.to_string())? = Some(id.clone());
    Ok(ActiveImageState {
        params: item.params.clone(),
        geom: item.geom.clone(),
        base_analyzed: item.base_color != BaseColor::default(),
    })
}

#[tauri::command]
pub async fn prepare_proxy(id: String, state: State<'_, EngineState>) -> Result<(), String> {
    let item_arc = state.items.get(&id).ok_or("Image ID not found")?.clone();
    let (file_path, has_proxy) = {
        let item = read_lock(&item_arc);
        if std::fs::File::open(&item.file_path).is_err() {
            return Err("FILE_MISSING".into());
        }
        (item.file_path.clone(), item.proxy_image.is_some())
    };

    if has_proxy {
        track_proxy_loaded(&state, &id);
        return Ok(());
    }

    let loaded = tokio::task::spawn_blocking(move || -> Result<_, String> {
        let img_buffer = decode_image_buffer(&file_path, DecodeMode::DevelopProxy)?;
        let (width, height) = img_buffer.dimensions();
        let ratio_proxy = (PROXY_LONG_EDGE / (width.max(height) as f32)).min(1.0);
        let proxy_width = (width as f32 * ratio_proxy).max(1.0) as u32;
        let proxy_height = (height as f32 * ratio_proxy).max(1.0) as u32;
        Ok(if ratio_proxy < 0.999 {
            image::imageops::resize(&img_buffer, proxy_width, proxy_height, FilterType::Triangle)
        } else {
            img_buffer
        })
    })
    .await
    .map_err(|e| e.to_string())??;

    {
        let mut item = write_lock(&item_arc);
        // The Develop proxy is always unmodified linear-sRGB capture data.
        // Geometry, inversion, and tone operations belong to the renderer.
        if item.proxy_image.is_none() {
            item.original_proxy = None;
            item.proxy_image = Some(loaded);
            item.pristine_proxy = None;
        }
    }
    track_proxy_loaded(&state, &id);
    Ok(())
}

#[tauri::command]
pub async fn analyze_proxy_base_color(
    id: String,
    state: State<'_, EngineState>,
) -> Result<(), String> {
    let item_arc = state.items.get(&id).ok_or("Image ID not found")?.clone();

    tokio::task::spawn_blocking(move || {
        let base_color = {
            let item = read_lock(&item_arc);
            if item.base_color != BaseColor::default() {
                return Ok(());
            }
            let proxy = item
                .proxy_image
                .as_ref()
                .ok_or_else(|| "PROXY_NOT_READY".to_string())?;
            compute_auto_base(proxy)
        };

        let (roll_id, file_path, previous_base_color) = {
            let mut item = write_lock(&item_arc);
            if item.base_color != BaseColor::default() {
                return Ok(());
            }
            let previous = std::mem::replace(&mut item.base_color, base_color.clone());
            (item.roll_id.clone(), item.file_path.clone(), previous)
        };
        if let Err(error) = persist_base_color(&roll_id, &file_path, &base_color) {
            let mut item = write_lock(&item_arc);
            if item.base_color == base_color {
                item.base_color = previous_base_color;
            }
            return Err(error);
        }
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
        let (proxy, geom, base_color, mode, linked_color_limits) = {
            let item = read_lock(&item_arc);
            if item.base_color == BaseColor::default() {
                return Err("BASE_COLOR_NOT_ANALYZED".to_string());
            }
            (
                item.proxy_image
                    .clone()
                    .ok_or_else(|| "PROXY_NOT_READY".to_string())?,
                item.geom.clone(),
                item.base_color.clone(),
                item.params.film_mode.clone(),
                is_noritsu_rendered_image(&item.file_path),
            )
        };
        compute_auto_color_limits(&proxy, &geom, &base_color, mode, linked_color_limits)
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn sync_thumbnail_buffer(
    id: String,
    state: State<'_, EngineState>,
) -> Result<(), String> {
    let item_arc = state.items.get(&id).ok_or("Image ID not found")?.clone();
    tokio::task::spawn_blocking(move || {
        {
            let mut item = write_lock(&item_arc);
            if item.pristine_proxy.is_none() {
                if let Some(proxy) = item.proxy_image.as_ref() {
                    item.pristine_proxy = Some(compute_pristine_proxy(
                        proxy,
                        &item.base_color,
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
        let (roll_id, file_path, previous_thumbnail) = {
            let mut item = write_lock(&item_arc);
            let previous = item
                .rendered_thumbnail_base64
                .replace(new_thumbnail.clone());
            (item.roll_id.clone(), item.file_path.clone(), previous)
        };
        if let Err(error) = persist_rendered_thumbnail(&roll_id, &file_path, &new_thumbnail) {
            let mut item = write_lock(&item_arc);
            if item.rendered_thumbnail_base64.as_deref() == Some(new_thumbnail.as_str()) {
                item.rendered_thumbnail_base64 = previous_thumbnail;
            }
            return Err(error);
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
    state: State<'_, EngineState>,
) -> Result<(), String> {
    let item_arc = state.items.get(&id).ok_or("Image ID not found")?.clone();
    tokio::task::spawn_blocking(move || {
        let (roll_id, file_path, previous_thumbnail) = {
            let mut item = write_lock(&item_arc);
            let previous = item.rendered_thumbnail_base64.replace(thumbnail.clone());
            (item.roll_id.clone(), item.file_path.clone(), previous)
        };
        if let Err(error) = persist_rendered_thumbnail(&roll_id, &file_path, &thumbnail) {
            let mut item = write_lock(&item_arc);
            if item.rendered_thumbnail_base64.as_deref() == Some(thumbnail.as_str()) {
                item.rendered_thumbnail_base64 = previous_thumbnail;
            }
            return Err(error);
        }
        Ok(())
    })
    .await
    .map_err(|error| format!("Thumbnail persistence worker failed: {error}"))?
}

#[tauri::command]
pub async fn update_geometry(
    id: String,
    geom: crate::app_state::GeometryState,
    state: State<'_, EngineState>,
) -> Result<(), String> {
    let item_arc = state.items.get(&id).ok_or("Image ID not found")?.clone();
    tokio::task::spawn_blocking(move || {
        let (roll_id, file_path, previous_geom) = {
            let mut item = write_lock(&item_arc);
            let previous = std::mem::replace(&mut item.geom, geom.clone());
            (item.roll_id.clone(), item.file_path.clone(), previous)
        };
        if let Err(error) = persist_geometry(&roll_id, &file_path, &geom) {
            let mut item = write_lock(&item_arc);
            if item.geom == geom {
                item.geom = previous_geom;
            }
            return Err(error);
        }
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
    geom
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
        match serde_json::from_value::<GeometryState>(geometry.clone()) {
            Ok(geometry) => {
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
                        item.geom = geometry.clone();
                    }
                }
            }
            Err(error) => {
                eprintln!("[Batch Settings] committed geometry cache refresh failed: {error}");
            }
        }
    }

    if let Err(error) = app_handle.emit("settings_updated", &commit.result) {
        eprintln!("[Batch Settings] settings_updated broadcast failed: {error}");
    }
    Ok(commit.result)
}

/// Standalone geometry application — does NOT require a write lock on FilmItem.
/// Returns (proxy_image, pristine_proxy) for the caller to assign under lock.
fn compute_geometry_and_pristine(
    original_proxy: &ImageBuffer<Rgb<u16>, Vec<u16>>,
    geom: &crate::app_state::GeometryState,
    base_color: &BaseColor,
    film_mode: FilmMode,
) -> (
    ImageBuffer<Rgb<u16>, Vec<u16>>,
    ImageBuffer<Rgb<f32>, Vec<f32>>,
) {
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

    let pristine = compute_pristine_proxy(&current, base_color, film_mode);
    (current, pristine)
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
            compute_geometry_and_pristine(
                &original_proxy,
                &geom,
                &item.base_color,
                item.params.film_mode.clone(),
            )
            .0
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
            build_response_buffer_from_proxy(
                proxy,
                &item.base_color,
                true,
                item.base_color != BaseColor::default(),
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
    state: State<'_, EngineState>,
) -> Result<(), String> {
    params.raw_decode.working_colorspace = DENSITY_CAPTURE_WORKING_SPACE.to_string();
    let item_arc = state.items.get(&id).ok_or("Image ID not found")?.clone();
    tokio::task::spawn_blocking(move || {
        let (file_path, previous_params, previous_pristine, film_mode_changed) = {
            let mut item = write_lock(&item_arc);
            if item.roll_id != roll_id {
                return Err("Image does not belong to the requested roll".into());
            }
            let previous_params = item.params.clone();
            let film_mode_changed = previous_params.film_mode != params.film_mode;
            let previous_pristine = if film_mode_changed {
                item.pristine_proxy.take()
            } else {
                None
            };
            item.params = params.clone();
            (
                item.file_path.clone(),
                previous_params,
                previous_pristine,
                film_mode_changed,
            )
        };
        if let Err(error) = persist_tuning_parameters(&roll_id, &file_path, &params) {
            let mut item = write_lock(&item_arc);
            if item.params == params {
                item.params = previous_params;
                if film_mode_changed {
                    item.pristine_proxy = previous_pristine;
                }
            }
            return Err(error);
        }
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

fn render_shader_equivalent(
    source: &ImageBuffer<Rgb<u16>, Vec<u16>>,
    params: &TuningParams,
    geom: &crate::app_state::GeometryState,
    base_color: &BaseColor,
    lut: Option<&ParsedLut>,
) -> ImageBuffer<Rgb<u16>, Vec<u16>> {
    let (source_width, source_height) = source.dimensions();
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
    let sprocket_target = sprocket_uv.and_then(|uv| sample_rgb16_nearest(source, uv));
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
    let pipeline = FilmPipeline::new(
        [base_color.base_r, base_color.base_g, base_color.base_b],
        exposure_offsets,
        params.film_mode.clone(),
    );
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
            let Some(raw) = sample_rgb16_nearest(source, warped_uv) else {
                return;
            };
            let linear_rgb = [
                raw[0] as f32 / 65535.0,
                raw[1] as f32 / 65535.0,
                raw[2] as f32 / 65535.0,
            ];
            let density = pipeline.process_pixel(&linear_rgb);
            let (d_min, d_max) = if params.film_mode == FilmMode::BW {
                ([bw_dmin; 3], [bw_dmax; 3])
            } else {
                (params.density.d_min, params.density.d_max)
            };
            let normalized = [
                normalize_density_channel(
                    density[0],
                    d_min[0],
                    d_max[0],
                    0.0,
                    0.0,
                    params.density.gamma,
                ),
                normalize_density_channel(
                    density[1],
                    d_min[1],
                    d_max[1],
                    0.0,
                    0.0,
                    params.density.gamma,
                ),
                normalize_density_channel(
                    density[2],
                    d_min[2],
                    d_max[2],
                    0.0,
                    0.0,
                    params.density.gamma,
                ),
            ];
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
                let raw_luma = linear_rgb
                    .iter()
                    .zip(luma_coefficients)
                    .map(|(value, coefficient)| value * coefficient)
                    .sum::<f32>();
                let target_luma = ([
                    target[0] as f32 / 65535.0,
                    target[1] as f32 / 65535.0,
                    target[2] as f32 / 65535.0,
                ])
                .iter()
                .zip(luma_coefficients)
                .map(|(value, coefficient)| value * coefficient)
                .sum::<f32>();
                let outside_calibration = crop_uv[0] < min_x
                    || crop_uv[0] > max_x
                    || crop_uv[1] < min_y
                    || crop_uv[1] > max_y;
                if outside_calibration {
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

#[derive(Clone)]
struct ExportItemSnapshot {
    id: String,
    file_path: String,
    roll_id: String,
    params: TuningParams,
    geom: GeometryState,
    base_color: BaseColor,
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
            let (params, geom, base_color) =
                load_image_state_from_connection(&transaction, &roll_id, &file_path)?
                    .map(|(_, params, geom, base_color)| (params, geom, base_color))
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
    for (index, snapshot) in export_snapshots.iter_mut().enumerate() {
        let roll = rolls.iter().find(|roll| roll.roll_id == snapshot.roll_id);
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
            match decode_image_buffer(&file_path, DecodeMode::ExportFull) {
                Ok(original) => {
                    let params = &params_owned;
                    let base_color = &base_color_owned;

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

                    let profile = crate::color_science::build_icc_profile(output_space);
                    match write_export_image_with_profile(
                        out_buffer,
                        &snapshot.output_path,
                        export_format,
                        quality,
                        snapshot.export_metadata.as_ref(),
                        Some(&profile),
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
    roll: Roll,
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
            *existing = roll;
        } else {
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

fn delete_source_paths(
    source_paths: HashMap<String, String>,
    protected_paths: &HashSet<String>,
    result: &mut DeleteRollsResult,
) {
    for (normalized, path) in source_paths {
        if protected_paths.contains(&normalized) {
            result.protected_source_files += 1;
            continue;
        }
        match std::fs::remove_file(&path) {
            Ok(()) => result.deleted_source_files += 1,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                result.missing_source_files += 1;
            }
            Err(error) => result.failed_source_files.push(format!("{path}: {error}")),
        }
    }
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
            delete_source_paths(source_paths, &protected_paths, &mut result);
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
            delete_source_paths(source_paths, &protected_paths, &mut result);
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

    conn.execute(
        "INSERT INTO image_states (
             roll_id, file_path, thumbnail_base64, embedded_thumb_base64,
             rendered_thumb_base64, params, geom, base_color,
             math_version, raw_decode_version, updated_at
         )
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
         ON CONFLICT(roll_id, file_path) DO UPDATE SET 
         thumbnail_base64=excluded.thumbnail_base64,
         embedded_thumb_base64=excluded.embedded_thumb_base64,
         rendered_thumb_base64=excluded.rendered_thumb_base64,
         params=excluded.params,
         geom=excluded.geom,
         base_color=excluded.base_color,
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
            MATH_VERSION,
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
);

fn load_image_state_from_connection(
    connection: &rusqlite::Connection,
    roll_id: &str,
    file_path: &str,
) -> Result<Option<PersistedImageState>, String> {
    let mut stmt = connection
        .prepare(
            "SELECT COALESCE(rendered_thumb_base64, embedded_thumb_base64, thumbnail_base64),
                params, geom, base_color
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
        let params_str: String = row.get(1).map_err(|error| error.to_string())?;
        let geom_str: String = row.get(2).map_err(|error| error.to_string())?;
        let base_color_str: String = row.get(3).map_err(|error| error.to_string())?;

        let params = serde_json::from_str(&params_str)
            .map_err(|error| format!("Invalid persisted tuning parameters: {error}"))?;
        let geom = serde_json::from_str(&geom_str)
            .map_err(|error| format!("Invalid persisted geometry: {error}"))?;
        let base_color = serde_json::from_str(&base_color_str)
            .map_err(|error| format!("Invalid persisted base color: {error}"))?;

        return Ok(Some((
            thumb,
            params,
            normalize_persisted_geometry(geom),
            base_color,
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
                    params, geom, base_color
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
            ))
        })
        .map_err(|error| format!("Failed to query image-state restore: {error}"))?;
    let mut restored = Vec::new();
    for row in rows {
        let row = row.map_err(|error| format!("Failed to read image-state row: {error}"))?;
        let (roll_id, file_path, embedded_thumb, rendered_thumb, params, geom, base_color) = row;
        let img_id = format!("img_{}", NEXT_ID.fetch_add(1, Ordering::SeqCst));
        let params = serde_json::from_str(&params)
            .map_err(|error| format!("Invalid tuning parameters for {file_path}: {error}"))?;
        let geom = serde_json::from_str(&geom)
            .map(normalize_persisted_geometry)
            .map_err(|error| format!("Invalid geometry for {file_path}: {error}"))?;
        let base_color = serde_json::from_str(&base_color)
            .map_err(|error| format!("Invalid base color for {file_path}: {error}"))?;
        let item = FilmItem {
            id: img_id.clone(),
            is_loose: roll_id == "LOOSE_DEFAULT",
            roll_id,
            file_path,
            embedded_thumbnail_base64: embedded_thumb,
            rendered_thumbnail_base64: rendered_thumb,
            original_proxy: None,
            proxy_image: None,
            pristine_proxy: None,
            base_color,
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
    let pipeline = FilmPipeline::new(
        [base_color.base_r, base_color.base_g, base_color.base_b],
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
            let gamma_corrected = [
                normalize_density_channel(
                    density[0],
                    effective_dmin[0],
                    effective_dmax[0],
                    highlights,
                    shadows,
                    gamma,
                ),
                normalize_density_channel(
                    density[1],
                    effective_dmin[1],
                    effective_dmax[1],
                    highlights,
                    shadows,
                    gamma,
                ),
                normalize_density_channel(
                    density[2],
                    effective_dmin[2],
                    effective_dmax[2],
                    highlights,
                    shadows,
                    gamma,
                ),
            ];
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
        compute_auto_base, compute_auto_color_limits, decode_image_buffer,
        decode_import_preview_base64, decode_scanner_fff_tiff_page, is_better_preview_edge,
        is_lightweight_direct_preview, is_noritsu_rendered_image, is_raw_extension,
        is_tiff_extension, linearize_scanner_fff, persist_import_batch, render_shader_equivalent,
        rgb16_image_from_bytes, DecodeMode, IMPORT_PREVIEW_LONG_EDGE,
    };
    use crate::app_state::{BaseColor, FilmItem, GeometryState, TuningParams};
    use crate::color_science::ColorSpaceId;
    use base64::Engine as _;

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
    fn linked_libraw_supports_current_gfx_raf_generation() {
        let version = rawlib::RawProcessor::version();
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
    #[ignore = "large user-supplied FFF fixture; run explicitly when validating scanner FFF"]
    fn hasselblad_fff_fixture_decodes_preview_and_linear_proxy() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("test_picture");
        let Some(path) = std::fs::read_dir(&root)
            .ok()
            .into_iter()
            .flatten()
            .flatten()
            .map(|entry| entry.path())
            .find(|path| {
                path.extension()
                    .is_some_and(|extension| extension.eq_ignore_ascii_case("fff"))
            })
        else {
            return;
        };

        let decode_started = std::time::Instant::now();
        let decoded =
            decode_image_buffer(path.to_string_lossy().as_ref(), DecodeMode::DevelopProxy)
                .expect("scanner FFF main image should decode as 16-bit TIFF");
        assert!(decoded.width() > 256 && decoded.height() > 256);
        assert!(decoded.as_raw().iter().any(|value| *value > 0));
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

        let reduced_page = decode_scanner_fff_tiff_page(path.to_string_lossy().as_ref(), 1)
            .expect("scanner FFF should contain a reduced TIFF page");
        assert!(reduced_page.width().max(reduced_page.height()) < IMPORT_PREVIEW_LONG_EDGE);

        let preview_started = std::time::Instant::now();
        let preview =
            decode_import_preview_base64(path.to_string_lossy().as_ref(), IMPORT_PREVIEW_LONG_EDGE)
                .expect("FFF should expose a TIFF-compatible preview");
        let preview_bytes = base64::engine::general_purpose::STANDARD
            .decode(preview)
            .expect("FFF preview must be base64");
        let preview_image = image::load_from_memory(&preview_bytes)
            .expect("FFF preview must contain a displayable image");
        assert!(preview_image.width() > 1 && preview_image.height() > 1);
        assert_eq!(
            (preview_image.width(), preview_image.height()),
            reduced_page.dimensions()
        );

        println!(
            "FFF preview {:?} ({}x{}), proxy {:?} ({:?})",
            preview_started.elapsed(),
            preview_image.width(),
            preview_image.height(),
            decode_started.elapsed(),
            decoded.dimensions()
        );
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
    fn nikon_nef_develop_decode_is_measured() {
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
            pristine_proxy: None,
            base_color: BaseColor::default(),
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
        activate_library_roll, clear_library_membership, delete_source_paths,
        load_all_image_states_from_connection, migrate_legacy_loose_roll, normalize_path,
        persist_import_batch, DeleteRollsResult,
    };
    use crate::app_state::{BaseColor, EngineState, FilmItem, GeometryState, Roll, TuningParams};
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
            pristine_proxy: None,
            base_color: BaseColor::default(),
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
        };

        activate_library_roll(&state, &roll).unwrap();

        let entry = state.items.get("loose").unwrap();
        let item = entry.read().unwrap();
        assert!(item.in_library);
        assert!(item.is_loose);
    }

    #[test]
    fn source_deletion_removes_only_unshared_existing_files() {
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

        delete_source_paths(paths, &protected_paths, &mut result);

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
        build_response_buffer_from_proxy, co_sited_density_extremes, compute_auto_color_limits,
        density_histogram_extremes, embedded_input_profile, encode_export_buffer,
        export_dimensions, gaussian_blur_rgb16_parallel, render_shader_equivalent,
        reserve_export_path, sanitize_export_file_stem, validate_export_color_space,
        write_export_image, write_export_image_with_profile, ExportConflictPolicy, ExportFormat,
    };
    use crate::app_state::{BaseColor, FilmMode, GeometryState, TuningParams};
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
