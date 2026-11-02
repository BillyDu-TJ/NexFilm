//! The auxiliary RAW source: rawler (the dnglab decoder library), pinned to
//! 0.8.0. Per `docs/raw-backend-dual-source-plan.md` this module is the only
//! place rawler is touched, so a future rawler upgrade edits one file.
//!
//! Three capabilities, each on an explicit trigger line:
//!
//! * M1 — `camera_to_srgb_fallback`: files LibRaw decodes with an identity
//!   colour matrix (every camera the bundled LibRaw predates, e.g. Hasselblad
//!   X2D or Nikon ZR) borrow their matrix from rawler's camera database. Only
//!   metadata is read; LibRaw's pixels stay authoritative.
//! * M2 — `decode_mosaic` / `decode_camera_rgb` / `read_container_source`:
//!   files LibRaw cannot unpack at all are decoded from rawler pixels through
//!   the shared fixed demosaic chain. Bayer only: X-Trans keeps going through
//!   LibRaw, which handles every shipping X-Trans body.
//! * M-HE — `nikon_high_efficiency_label`: Nikon's High Efficiency NEF
//!   compression is patented (intoPIX TicoRAW) and undecodable by every
//!   open-source backend, LibRaw and rawler alike. A failed Nikon decode is
//!   therefore answered with recognition and conversion guidance instead of a
//!   generic error.

use crate::raw_backend::{
    bayer_raw_channel, camera_matrix_plausible, camera_to_srgb_from_xyz_to_cam,
    corrected_capture, demosaic_bayer_fixed, libraw_flip_from_tiff_orientation,
    normalized_as_shot_gains, normalized_as_shot_neutral, orient_camera_native,
    xyz_to_cam_3x3_from_flat, CameraMatrixSource, CameraRgbData, CfaPattern, CorrectedCfaF32,
    DECODER_SOURCE_RAWLER, DENSITY_EPSILON, QualityFlag, QualityMask, RawContainerSource,
    RawMetadata, RawMosaic,
};
use rawler::imgop::xyz::Illuminant;
use rawler::rawimage::RawPhotometricInterpretation;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

/// Keep in sync with the exact pin in Cargo.toml; rawler 0.x changes its API
/// freely between minor versions.
pub(crate) const RAWLER_VERSION: &str = "0.8.0";

/// NEF makernote tag holding the NEFCompression enum on classic bodies.
const NIKON_MAKERNOTE_NEF_COMPRESSION: u16 = 0x0093;
/// Newer Z bodies moved the same enum to this tag, with the u16 at data offset
/// 10 (rawler's NEF decoder reads it the same way).
const NIKON_MAKERNOTE_NEF_COMPRESSION_NEW: u16 = 0x0051;

/// NEFCompression values that identify the patented High Efficiency family
/// (ExifTool Nikon.pm and rawler's `NefCompression` agree).
const NEF_COMPRESSION_HIGH_EFFICIENCY: u16 = 13;
const NEF_COMPRESSION_HIGH_EFFICIENCY_STAR: u16 = 14;

// ---------------------------------------------------------------------------
// M1: the colour-matrix probe (metadata only, pixels untouched)
// ---------------------------------------------------------------------------

/// Camera→sRGB matrix for a file LibRaw decoded with the identity matrix.
///
/// rawler is asked for a dummy decode (metadata parsed, pixels skipped), and
/// the preferred per-illuminant table is converted exactly the way LibRaw
/// converts its own `cam_xyz`. The returned string names the illuminant for
/// diagnostics; `Ok(None)` means rawler has no characterisation either and the
/// decode stays sensor-native.
pub(crate) fn camera_to_srgb_fallback<P: AsRef<Path>>(
    path: P,
) -> Result<Option<([f32; 9], String)>, String> {
    let image = decode_metadata_only(path)?;
    Ok(rawler_camera_to_srgb(&image))
}

/// rawler's dummy decode: the container, camera model and colour tables are
/// parsed, but pixel decompression is skipped. This is what keeps the M1
/// matrix backfill a cold-path metadata probe instead of a second full decode.
fn decode_metadata_only<P: AsRef<Path>>(path: P) -> Result<rawler::RawImage, String> {
    let source = rawler::rawsource::RawSource::new(path.as_ref())
        .map_err(|error| format!("rawler could not open {}: {error}", path.as_ref().display()))?;
    // `decode_dummy` is the documented fuzzing entry point and the only
    // public skip-decompression switch in rawler 0.8.
    rawler::decode_dummy(&source)
        .map_err(|error| format!("rawler metadata probe failed: {error}"))
}

/// Convert a decoded rawler image's colour table into the camera→sRGB matrix
/// LibRaw would have used. Prefers D65; falls back through the common
/// daylight-family illuminants and finally any table rawler carries, with the
/// chosen illuminant recorded.
fn rawler_camera_to_srgb(image: &rawler::RawImage) -> Option<([f32; 9], String)> {
    let preferred = [
        Illuminant::D65,
        Illuminant::D50,
        Illuminant::Daylight,
        Illuminant::FineWeather,
        Illuminant::CloudyWeather,
        Illuminant::Shade,
        Illuminant::A,
    ];
    let (illuminant, matrix) = image
        .color_matrix_find_first(preferred)
        // Unstable HashMap order, but a non-standard illuminant recorded in
        // the label beats no characterisation at all.
        .or_else(|| image.color_matrix.iter().next().map(|(key, value)| (*key, value.clone())))?;
    let flat: Vec<f32> = matrix;
    let xyz_to_cam = xyz_to_cam_3x3_from_flat(&flat)?;
    let camera_to_srgb = camera_to_srgb_from_xyz_to_cam(&xyz_to_cam)?;
    camera_matrix_plausible(camera_to_srgb)
        .then_some((camera_to_srgb, format!("{illuminant:?}")))
}

// ---------------------------------------------------------------------------
// M2: pixel fallback through the shared demosaic chain
// ---------------------------------------------------------------------------

/// Raw mosaic for files LibRaw cannot unpack. Bayer CFA only, matching the
/// shared `demosaic_bayer_fixed` contract.
pub(crate) fn decode_mosaic<P: AsRef<Path>>(path: P) -> Result<RawMosaic, String> {
    let path = path.as_ref();
    let image = rawler::decode_file(path).map_err(|error| format!("rawler decode failed: {error}"))?;
    build_mosaic(path, &image)
}

/// Camera RGB for files LibRaw cannot unpack: rawler pixels, black/white
/// normalisation, normalized-as-shot white balance, the shared fixed demosaic
/// and orientation — the same contract LibRaw's `dcraw_process` fulfils on the
/// primary path, minus tone options the fallback never applies (its output is
/// linear camera-space RGB; the caller applies the matrix in f32).
pub(crate) fn decode_camera_rgb<P: AsRef<Path>>(
    path: P,
    half_size: bool,
) -> Result<CameraRgbData, String> {
    let path = path.as_ref();
    let image =
        rawler::decode_file(path).map_err(|error| format!("rawler decode failed: {error}"))?;
    let mosaic = build_mosaic(path, &image)?;

    // Multipliers like LibRaw's cam_mul; normalised so the strongest channel
    // is unity. Without usable coefficients this is a true no-balance decode.
    let gains = normalized_as_shot_gains(image.wb_coeffs).unwrap_or([1.0; 4]);
    let corrected = normalize_camera_native(&mosaic, gains);
    let demosaiced = demosaic_bayer_fixed(&corrected)?;
    let oriented = orient_camera_native(demosaiced, mosaic.metadata.orientation);
    let (width, height, pixels) = if half_size {
        downsample_half(&oriented)
    } else {
        (
            oriented.width,
            oriented.height,
            oriented
                .pixels
                .iter()
                .map(|value| (*value).clamp(0.0, 1.0))
                .collect::<Vec<f32>>(),
        )
    };
    if width > u16::MAX as u32 || height > u16::MAX as u32 {
        return Err(format!("rawler mosaic {width}x{height} exceeds the 16-bit transport"));
    }
    let mut data = Vec::with_capacity(pixels.len() * 2);
    for value in &pixels {
        data.extend_from_slice(&((*value * 65535.0).round() as u16).to_ne_bytes());
    }

    let (camera_to_srgb, source) = rawler_camera_to_srgb(&image)
        .map(|(matrix, illuminant)| (matrix, CameraMatrixSource::RawlerFallback(illuminant)))
        .unwrap_or((
            [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0],
            CameraMatrixSource::Unresolved,
        ));
    Ok(CameraRgbData {
        width: width as u16,
        height: height as u16,
        colors: 3,
        bits: 16,
        data,
        camera_to_srgb,
        camera_to_srgb_source: source,
    })
}

/// The mosaic plus the camera colour metadata a DNG re-wrap needs, from rawler
/// pixels. rawler's per-illuminant table is already in DNG `ColorMatrix1`
/// orientation (XYZ→camera), and its white-balance multipliers follow the same
/// cam_mul convention as LibRaw's, so both tags reuse the existing converters.
pub(crate) fn read_container_source<P: AsRef<Path>>(
    path: P,
) -> Result<RawContainerSource, String> {
    let path = path.as_ref();
    let image =
        rawler::decode_file(path).map_err(|error| format!("rawler decode failed: {error}"))?;
    let mosaic = build_mosaic(path, &image)?;
    let (make, model) =
        (image.clean_make.trim().to_string(), image.clean_model.trim().to_string());
    Ok(RawContainerSource {
        xyz_to_camera: rawler_color_matrix_xyz_to_cam(&image),
        as_shot_neutral: normalized_as_shot_neutral(image.wb_coeffs),
        make,
        model,
        mosaic,
    })
}

/// rawler's colour table in DNG ColorMatrix1 form (XYZ→camera rows).
fn rawler_color_matrix_xyz_to_cam(image: &rawler::RawImage) -> Option<[[f32; 3]; 3]> {
    let (illuminant, matrix) = image
        .color_matrix_find_first([
            Illuminant::D65,
            Illuminant::D50,
            Illuminant::Daylight,
            Illuminant::A,
        ])?;
    let _ = illuminant;
    xyz_to_cam_3x3_from_flat(&matrix)
}

/// Adapt a decoded rawler image into the application's RawMosaic contract.
fn build_mosaic(path: &Path, image: &rawler::RawImage) -> Result<RawMosaic, String> {
    if image.cpp != 1 {
        return Err(format!(
            "rawler image carries {} samples per pixel; only single-plane CFA mosaics are supported",
            image.cpp
        ));
    }
    let samples = match &image.data {
        rawler::RawImageData::Integer(samples) => samples.clone(),
        rawler::RawImageData::Float(_) => {
            return Err("rawler produced floating-point pixels instead of a u16 CFA mosaic".to_string())
        }
    };
    let (width, height) = (image.width as u32, image.height as u32);
    let expected = (width as usize).saturating_mul(height as usize);
    if samples.len() != expected {
        return Err(format!(
            "rawler mosaic carries {} samples for {width}x{height}",
            samples.len()
        ));
    }

    // The CFA phase is anchored at the full raw origin (the demosaic applies
    // active-area offsets on top), so the uncropped pattern from the
    // photometric tag is the right one — `cropped_cfa()` would describe the
    // shifted pattern of the cropped view.
    let cfa = match &image.photometric {
        RawPhotometricInterpretation::Cfa(config) => &config.cfa,
        _ => {
            return Err("rawler image is not a CFA mosaic".to_string());
        }
    };
    // Bayer-only boundary: X-Trans bodies are old enough that LibRaw handles
    // them, so a 6x6 pattern on the fallback path is rejected rather than
    // silently mis-demosaiced.
    if cfa.width != 2 || cfa.height != 2 {
        return Err(format!(
            "rawler CFA pattern {:?} ({}x{}) is outside the Bayer-only fallback",
            cfa.name, cfa.width, cfa.height
        ));
    }
    let filters = bayer_filters_from_cfa(cfa);

    let black_level = per_channel_levels(
        image
            .blacklevel
            .levels
            .iter()
            .map(|rational| {
                if rational.d == 0 {
                    0.0
                } else {
                    rational.n as f32 / rational.d as f32
                }
            })
            .collect::<Vec<f32>>()
            .as_slice(),
    );
    let white_level = per_channel_levels(
        image.whitelevel.0.iter().map(|level| *level as f32).collect::<Vec<f32>>().as_slice(),
    );

    // rawler records optical-black rectangles as sensor-area rects, matching
    // the [top, left, bottom, right] contract.
    let masked_areas = image
        .blackareas
        .iter()
        .map(|area| {
            [
                area.p.y as u32,
                area.p.x as u32,
                (area.p.y + area.d.h) as u32,
                (area.p.x + area.d.w) as u32,
            ]
        })
        .collect();
    let [left, top, active_width, active_height] = image
        .active_area
        .map(|area| {
            [
                area.p.x as u32,
                area.p.y as u32,
                area.d.w as u32,
                area.d.h as u32,
            ]
        })
        .unwrap_or([0, 0, width, height]);

    // rawler 0.8 hardcodes RawImage::orientation to Normal ("TODO fixme"), so
    // the orientation comes straight from the TIFF IFD0 tag, mapped through
    // LibRaw's own table to keep both backends bit-identical.
    let metadata = RawMetadata {
        cfa: CfaPattern::Bayer { filters },
        active_area: [left, top, active_width, active_height],
        raw_pitch_bytes: width * 2,
        black_level,
        white_level,
        masked_areas,
        masked_pixels: Vec::new(),
        orientation: libraw_flip_from_tiff_orientation(tiff_ifd0_orientation(path)),
        // rawler's decoded RawImage does not expose EXIF exposure metadata.
        iso: None,
        exposure_seconds: None,
        camera_id: format!(
            "{}|{}",
            image.clean_make.trim(),
            image.clean_model.trim()
        ),
        decoder_source: DECODER_SOURCE_RAWLER.to_string(),
        decoder_version: RAWLER_VERSION.to_string(),
        capture_conditions: Default::default(),
    };

    Ok(RawMosaic {
        width,
        height,
        samples,
        metadata,
    })
}

/// Normalise raw CFA samples into camera-native [0,1] transmission with the
/// same flag discipline as `correct_cfa_capture`: below-black is negative,
/// at-or-above-white is saturated, an unusable denominator is invalid — and
/// every flagged sample is masked instead of epsilon-repaired. White-balance
/// gains are applied here (as on the LibRaw path) so the demosaic sees
/// balanced channels.
fn normalize_camera_native(mosaic: &RawMosaic, gains: [f32; 4]) -> CorrectedCfaF32 {
    let expected = (mosaic.width as usize) * (mosaic.height as usize);
    let mut samples = vec![f32::NAN; expected];
    let mut quality = QualityMask::new(expected);
    let CfaPattern::Bayer { filters } = mosaic.metadata.cfa else {
        // Callers only reach this through build_mosaic, which rejects non-Bayer.
        return corrected_capture(mosaic, samples, quality);
    };
    for (index, sample) in mosaic.samples.iter().enumerate() {
        let y = (index / mosaic.width as usize) as i32;
        let x = (index % mosaic.width as usize) as i32;
        let channel = bayer_raw_channel(filters, y, x);
        let black = mosaic.metadata.black_level[channel.min(3)];
        let white = mosaic.metadata.white_level[channel.min(3)];
        let denominator = white - black;
        let value = if !denominator.is_finite() || denominator <= DENSITY_EPSILON {
            quality.invalidate(index, QualityFlag::InvalidDenominator);
            f32::NAN
        } else {
            let transmission = (*sample as f32 - black) / denominator * gains[channel.min(3)];
            if (*sample as f32) < black {
                quality.invalidate(index, QualityFlag::NegativeSample);
            }
            if (*sample as f32) >= white {
                quality.invalidate(index, QualityFlag::SaturatedSample);
            }
            if !transmission.is_finite() || transmission <= 0.0 {
                quality.invalidate(index, QualityFlag::NegativeSample);
            } else if transmission > 1.0 + DENSITY_EPSILON {
                quality.invalidate(index, QualityFlag::OutOfRange);
            }
            transmission
        };
        if quality.valid[index] {
            samples[index] = value;
        }
    }
    corrected_capture(mosaic, samples, quality)
}

/// Even-numbered 2×2 box reduction for the Develop proxy. A target sample is
/// valid only when all four sources are.
fn downsample_half(source: &crate::raw_backend::CameraNativeTransmissionRgbF32) -> (u32, u32, Vec<f32>) {
    let width = source.width / 2;
    let height = source.height / 2;
    let mut pixels = vec![f32::NAN; (width as usize) * (height as usize) * 3];
    let mut quality = QualityMask::new((width as usize) * (height as usize));
    for target_y in 0..height as usize {
        for target_x in 0..width as usize {
            let mut sums = [0.0f32; 3];
            let mut count = 0usize;
            for dy in 0..2 {
                for dx in 0..2 {
                    let source_index = (target_y * 2 + dy) * source.width as usize + target_x * 2 + dx;
                    if !source.quality.valid[source_index] {
                        continue;
                    }
                    for channel in 0..3 {
                        sums[channel] += source.pixels[source_index * 3 + channel];
                    }
                    count += 1;
                }
            }
            let target_index = target_y * width as usize + target_x;
            if count == 4 {
                for channel in 0..3 {
                    pixels[target_index * 3 + channel] = sums[channel] / 4.0;
                }
            } else {
                quality.invalidate(target_index, QualityFlag::DemosaicInsufficientNeighbors);
            }
        }
    }
    (width, height, pixels)
}

/// Collapse rawler's per-site level list (one entry, or one per CFA site) into
/// the four-channel contract.
fn per_channel_levels(levels: &[f32]) -> [f32; 4] {
    if levels.is_empty() {
        return [0.0; 4];
    }
    [
        levels[0],
        levels[1 % levels.len()],
        levels[2 % levels.len()],
        levels[3 % levels.len()],
    ]
}

/// Build LibRaw's packed `filters` word from a 2×2 rawler CFA. Bit placement
/// mirrors the read-side formula `bayer_raw_channel` uses, so a round trip
/// through LibRaw constants is lossless (RGGB→0x94949494 etc.).
fn bayer_filters_from_cfa(cfa: &rawler::CFA) -> u32 {
    let mut pattern = 0u32;
    for row in 0..2usize {
        for col in 0..2usize {
            let code = cfa.color_at(row, col).min(3) as u32;
            pattern |= code << ((((row << 1) & 14) | (col & 1)) << 1);
        }
    }
    // LibRaw repeats the 2x2 byte across all four positions so rows 4..7 read
    // the same phase as rows 0..3.
    pattern | (pattern << 8) | (pattern << 16) | (pattern << 24)
}

// ---------------------------------------------------------------------------
// M-HE: Nikon High Efficiency recognition
// ---------------------------------------------------------------------------

/// `Some("HE")` / `Some("HE*")` when the file carries Nikon's patented High
/// Efficiency NEF compression. Best-effort by design: every parse failure
/// returns `None` and the caller keeps its original error. The check reads
/// only headers and makernotes, never pixel data.
pub(crate) fn nikon_high_efficiency_label<P: AsRef<Path>>(path: P) -> Option<&'static str> {
    let mut file = File::open(path.as_ref()).ok()?;
    let header = read_exact_at(&mut file, 0, 8).ok()?;
    let (little_endian, ifd0_offset) = parse_tiff_header(&header)?;
    let ifd = parse_ifd(&mut file, ifd0_offset, little_endian)?;

    // The label only makes sense for Nikon files.
    let make = ifd
        .iter()
        .find(|entry| entry.tag == 0x010F)
        .and_then(|entry| entry.value(&mut file, little_endian, 0, 256).ok())
        .map(|bytes| ascii_string(&bytes))?;
    if !make.to_ascii_lowercase().contains("nikon") {
        return None;
    }

    let makernote = ifd.iter().find(|entry| entry.tag == 0x927C)?;
    // The makernote value never fits the inline slot; its pointer is relative
    // to the file TIFF header (base 0).
    let makernote_offset = makernote.pointer(&mut file, little_endian).ok()?;
    // Nikon's modern makernote: "Nikon\0\x02\x10\0" followed by an embedded
    // TIFF header whose offsets are relative to that header.
    let signature = read_exact_at(&mut file, makernote_offset as u64, 18).ok()?;
    if !signature.starts_with(b"Nikon\0\x02\x10\0") {
        return None;
    }
    let (maker_endian, maker_ifd_offset) = parse_tiff_header(&signature[10..18])?;
    let maker_ifd_base = makernote_offset + 10;
    let maker_entries = parse_ifd(&mut file, maker_ifd_base + maker_ifd_offset, maker_endian)?;

    // Classic bodies: NEFCompression (0x0093) as an inline SHORT.
    if let Some(entry) = maker_entries
        .iter()
        .find(|entry| entry.tag == NIKON_MAKERNOTE_NEF_COMPRESSION)
    {
        if let Some(value) = entry.scalar_u16(&mut file, maker_ifd_base, maker_endian) {
            return high_efficiency_label(value);
        }
    }
    // Newer Z bodies: the same enum inside tag 0x0051, u16 at byte offset 10.
    let entry = maker_entries
        .iter()
        .find(|entry| entry.tag == NIKON_MAKERNOTE_NEF_COMPRESSION_NEW)?;
    let data = entry
        .value(&mut file, maker_endian, maker_ifd_base, 64 * 1024)
        .ok()?;
    if data.len() < 12 {
        return None;
    }
    let value = if maker_endian {
        u16::from_le_bytes([data[10], data[11]])
    } else {
        u16::from_be_bytes([data[10], data[11]])
    };
    high_efficiency_label(value)
}

fn high_efficiency_label(compression: u16) -> Option<&'static str> {
    match compression {
        NEF_COMPRESSION_HIGH_EFFICIENCY => Some("HE"),
        NEF_COMPRESSION_HIGH_EFFICIENCY_STAR => Some("HE*"),
        _ => None,
    }
}

/// The user-facing answer for an undecodable HE/HE* file: name the cause and
/// give the two working routes forward.
pub(crate) fn high_efficiency_error_message(label: &str, libraw_error: &str) -> String {
    format!(
        "This file uses Nikon {label} (High Efficiency) RAW compression and cannot be decoded: \
HE/HE* is a patented intoPIX TicoRAW format that no open-source RAW library (LibRaw, rawler/dnglab, darktable) \
supports — this is not a NexFilm-specific limitation. Two ways forward: set the camera's NEF compression to \
\"Lossless compression\" and re-shoot, or convert the file to DNG with Adobe DNG Converter or Nikon NX Studio \
and import the DNG, which NexFilm fully supports. If you believe this file is actually lossless-compressed, \
please report it with a sample. (Backend detail: {libraw_error})"
    )
}

// ---------------------------------------------------------------------------
// Minimal TIFF reading (IFD0 tag scan + Nikon makernote). Only what the HE
// check and the orientation lookup need; no pixel data is touched.
// ---------------------------------------------------------------------------

struct TiffEntry {
    tag: u16,
    count: u32,
    /// Absolute file offset of the 4-byte inline slot: either the value bytes
    /// themselves or a pointer whose target is relative to `base`.
    value_offset: u32,
    type_size: u32,
}

impl TiffEntry {
    fn total_bytes(&self) -> u32 {
        self.count.saturating_mul(self.type_size)
    }

    fn inline(&self) -> bool {
        self.total_bytes() <= 4
    }

    /// Read the entry's value bytes, resolving an offset pointer when the
    /// value does not fit the inline slot. `base` is the offset the entry's
    /// pointers are relative to (0 for the file TIFF, the makernote header
    /// start for maker IFDs).
    fn value(
        &self,
        file: &mut File,
        little_endian: bool,
        base: u32,
        cap: u32,
    ) -> std::io::Result<Vec<u8>> {
        let total = self.total_bytes().min(cap);
        let offset = if self.inline() {
            self.value_offset
        } else {
            base.checked_add(self.pointer(file, little_endian)?).ok_or_else(|| {
                std::io::Error::new(std::io::ErrorKind::InvalidData, "TIFF pointer overflow")
            })?
        };
        read_exact_at(file, offset as u64, total as usize)
    }

    fn pointer(&self, file: &mut File, little_endian: bool) -> std::io::Result<u32> {
        let bytes = read_exact_at(file, self.value_offset as u64, 4)?;
        u32_at(&bytes, 0, little_endian)
            .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::UnexpectedEof, "short pointer"))
    }

    /// The entry's value as a single u16 (a SHORT or LONG inline slot).
    fn scalar_u16(&self, file: &mut File, base: u32, little_endian: bool) -> Option<u16> {
        let bytes = if self.inline() {
            read_exact_at(file, self.value_offset as u64, 2).ok()?
        } else {
            let pointer = self.pointer(file, little_endian).ok()?;
            read_exact_at(file, (base.checked_add(pointer)?) as u64, 2).ok()?
        };
        u16_at(&bytes, 0, little_endian)
    }
}

fn read_exact_at(file: &mut File, offset: u64, len: usize) -> std::io::Result<Vec<u8>> {
    file.seek(SeekFrom::Start(offset))?;
    let mut buffer = vec![0u8; len];
    file.read_exact(&mut buffer)?;
    Ok(buffer)
}

/// The u16 sitting at `offset` in `bytes`, honouring the byte order.
fn u16_at(bytes: &[u8], offset: usize, little_endian: bool) -> Option<u16> {
    let high = *bytes.get(offset)?;
    let low = *bytes.get(offset + 1)?;
    Some(if little_endian {
        u16::from_le_bytes([high, low])
    } else {
        u16::from_be_bytes([high, low])
    })
}

fn u32_at(bytes: &[u8], offset: usize, little_endian: bool) -> Option<u32> {
    let mut raw = [0u8; 4];
    for (slot, index) in raw.iter_mut().zip(0..4) {
        *slot = *bytes.get(offset + index)?;
    }
    Some(if little_endian {
        u32::from_le_bytes(raw)
    } else {
        u32::from_be_bytes(raw)
    })
}

fn parse_tiff_header(header: &[u8]) -> Option<(bool, u32)> {
    let little_endian = match header.get(..2)? {
        b"II" => true,
        b"MM" => false,
        _ => return None,
    };
    if u16_at(header, 2, little_endian)? != 42 {
        return None;
    }
    Some((little_endian, u32_at(header, 4, little_endian)?))
}

fn parse_ifd(file: &mut File, offset: u32, little_endian: bool) -> Option<Vec<TiffEntry>> {
    let count_bytes = read_exact_at(file, offset as u64, 2).ok()?;
    let count = u16_at(&count_bytes, 0, little_endian)? as usize;
    // 12 bytes per entry plus the next-IFD pointer.
    let table = read_exact_at(file, offset as u64 + 2, count.saturating_mul(12)).ok()?;
    let mut entries = Vec::with_capacity(count);
    for index in 0..count {
        let entry = &table[index * 12..index * 12 + 12];
        let tag = u16_at(entry, 0, little_endian)?;
        let kind = u16_at(entry, 2, little_endian)?;
        let count = u32_at(entry, 4, little_endian)?;
        // The inline slot starts here; for out-of-line values it holds the
        // pointer, which is relative to the TIFF header this IFD belongs to.
        let value_offset = offset.saturating_add(2 + (index as u32) * 12 + 8);
        let type_size = match kind {
            1 | 2 | 6 | 7 => 1, // BYTE, ASCII, SBYTE, UNDEFINED
            3 | 8 => 2,         // SHORT
            4 | 9 => 4,         // LONG
            5 | 10 => 8,        // RATIONAL
            _ => 1,
        };
        entries.push(TiffEntry {
            tag,
            count,
            value_offset,
            type_size,
        });
    }
    Some(entries)
}

/// IFD0 Orientation (0x0112) as the raw TIFF value; 0 when absent, which the
/// flip table treats as "no rotation".
fn tiff_ifd0_orientation<P: AsRef<Path>>(path: P) -> u16 {
    tiff_ifd0_u16(path, 0x0112).unwrap_or(0)
}

fn tiff_ifd0_u16<P: AsRef<Path>>(path: P, tag: u16) -> Option<u16> {
    let mut file = File::open(path.as_ref()).ok()?;
    let header = read_exact_at(&mut file, 0, 8).ok()?;
    let (little_endian, ifd0_offset) = parse_tiff_header(&header)?;
    let ifd = parse_ifd(&mut file, ifd0_offset, little_endian)?;
    ifd.iter()
        .find(|entry| entry.tag == tag)?
        .scalar_u16(&mut file, 0, little_endian)
}

fn ascii_string(bytes: &[u8]) -> String {
    let end = bytes.iter().position(|byte| *byte == 0).unwrap_or(bytes.len());
    String::from_utf8_lossy(&bytes[..end]).trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    // -- synthetic TIFF construction ------------------------------------

    fn push_u16(buffer: &mut Vec<u8>, value: u16, little_endian: bool) {
        let raw = if little_endian {
            value.to_le_bytes()
        } else {
            value.to_be_bytes()
        };
        buffer.extend_from_slice(&raw);
    }

    fn push_u32(buffer: &mut Vec<u8>, value: u32, little_endian: bool) {
        let raw = if little_endian {
            value.to_le_bytes()
        } else {
            value.to_be_bytes()
        };
        buffer.extend_from_slice(&raw);
    }

    /// Minimal NEF-shaped TIFF: IFD0 with Make, Model and a Nikon makernote
    /// ("Nikon\0\x02\x10\0" + embedded TIFF), carrying either the classic
    /// 0x0093 NEFCompression SHORT or the new-Z 0x0051 blob form.
    fn nikon_tiff(
        little_endian: bool,
        make: &str,
        compression: Option<u16>,
        use_new_tag: bool,
        orientation: Option<u16>,
    ) -> Vec<u8> {
        // Makernote payload first so its size is known.
        // The real header is 10 bytes ("Nikon\0\x02\x10\0" + one pad byte)
        // before the embedded TIFF header starts, matching rawler's skip of 10.
        let mut makernote = b"Nikon\0\x02\x10\0\0".to_vec();
        push_u16(&mut makernote, if little_endian { 0x4949 } else { 0x4D4D }, true);
        push_u16(&mut makernote, 42, little_endian);
        push_u32(&mut makernote, 8, little_endian); // mini IFD follows the header
        let mini_ifd_offset = makernote.len() as u32;
        match compression {
            Some(value) if !use_new_tag => {
                push_u16(&mut makernote, 1, little_endian); // entry count
                push_u16(&mut makernote, NIKON_MAKERNOTE_NEF_COMPRESSION, little_endian);
                push_u16(&mut makernote, 3, little_endian); // SHORT
                push_u32(&mut makernote, 1, little_endian); // count
                push_u16(&mut makernote, value, little_endian); // inline value
                push_u16(&mut makernote, 0, little_endian); // slot padding
                push_u32(&mut makernote, 0, little_endian); // next IFD
            }
            Some(value) => {
                push_u16(&mut makernote, 1, little_endian);
                push_u16(&mut makernote, NIKON_MAKERNOTE_NEF_COMPRESSION_NEW, little_endian);
                push_u16(&mut makernote, 7, little_endian); // UNDEFINED
                push_u32(&mut makernote, 12, little_endian); // count
                // Absolute within the makernote; stored relative to the
                // embedded TIFF header (which starts 10 bytes in).
                let data_offset = (mini_ifd_offset + 2 + 12 + 4 - 10) as u32;
                push_u32(&mut makernote, data_offset, little_endian); // pointer
                push_u32(&mut makernote, 0, little_endian); // next IFD
                // The 0x0051 blob carries the compression u16 at byte 10.
                // The pointer above is relative to the mini header (base 10).
                let mut blob = vec![0u8; 12];
                let raw = if little_endian {
                    value.to_le_bytes()
                } else {
                    value.to_be_bytes()
                };
                blob[10..12].copy_from_slice(&raw);
                makernote.extend_from_slice(&blob);
            }
            None => {
                push_u16(&mut makernote, 0, little_endian);
                push_u32(&mut makernote, 0, little_endian);
            }
        }

        // File-level TIFF.
        let mut tiff = Vec::new();
        tiff.extend_from_slice(if little_endian { b"II" } else { b"MM" });
        push_u16(&mut tiff, 42, little_endian);
        push_u32(&mut tiff, 8, little_endian); // IFD0 at 8

        let make_bytes = make.as_bytes();
        let model_bytes = b"NIKON Z 8\0";
        let entry_count = 3 + usize::from(orientation.is_some());
        let make_offset: u32 = (8 + 2 + entry_count * 12 + 4) as u32;
        let model_offset = make_offset + make_bytes.len() as u32;
        let makernote_offset = model_offset + model_bytes.len() as u32;

        push_u16(&mut tiff, entry_count as u16, little_endian);
        // Make (0x010F), ASCII, out of line.
        push_u16(&mut tiff, 0x010F, little_endian);
        push_u16(&mut tiff, 2, little_endian);
        push_u32(&mut tiff, make_bytes.len() as u32, little_endian);
        push_u32(&mut tiff, make_offset, little_endian);
        // Model (0x0110), ASCII, out of line.
        push_u16(&mut tiff, 0x0110, little_endian);
        push_u16(&mut tiff, 2, little_endian);
        push_u32(&mut tiff, model_bytes.len() as u32, little_endian);
        push_u32(&mut tiff, model_offset, little_endian);
        // Orientation (0x0112), SHORT inline, when requested.
        if let Some(value) = orientation {
            push_u16(&mut tiff, 0x0112, little_endian);
            push_u16(&mut tiff, 3, little_endian);
            push_u32(&mut tiff, 1, little_endian);
            push_u16(&mut tiff, value, little_endian);
            push_u16(&mut tiff, 0, little_endian);
        }
        // MakerNote (0x927C), UNDEFINED, out of line.
        push_u16(&mut tiff, 0x927C, little_endian);
        push_u16(&mut tiff, 7, little_endian);
        push_u32(&mut tiff, makernote.len() as u32, little_endian);
        push_u32(&mut tiff, makernote_offset, little_endian);
        push_u32(&mut tiff, 0, little_endian); // next IFD

        tiff.extend_from_slice(make_bytes);
        tiff.extend_from_slice(model_bytes);
        tiff.extend_from_slice(&makernote);
        tiff
    }

    fn temp_file(name: &str, bytes: &[u8]) -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!(
            "nexfilm-rawler-backend-{name}-{}",
            std::process::id()
        ));
        std::fs::write(&path, bytes).expect("write temp file");
        path
    }

    // -- M-HE ------------------------------------------------------------

    #[test]
    fn classic_makernote_labels_high_efficiency() {
        for (value, expected) in [(13u16, Some("HE")), (14, Some("HE*")), (3, None), (10, None)] {
            let path = temp_file("classic", &nikon_tiff(true, "NIKON CORPORATION", Some(value), false, None));
            assert_eq!(nikon_high_efficiency_label(&path), expected, "compression {value}");
            let _ = std::fs::remove_file(&path);
        }
    }

    #[test]
    fn new_z_makernote_labels_high_efficiency_and_big_endian_parses() {
        for little_endian in [true, false] {
            let path = temp_file(
                "newz",
                &nikon_tiff(little_endian, "NIKON CORPORATION", Some(13), true, None),
            );
            assert_eq!(nikon_high_efficiency_label(&path), Some("HE"));
            let _ = std::fs::remove_file(&path);
            let path = temp_file(
                "newz-star",
                &nikon_tiff(little_endian, "NIKON CORPORATION", Some(14), true, None),
            );
            assert_eq!(nikon_high_efficiency_label(&path), Some("HE*"));
            let _ = std::fs::remove_file(&path);
            // A lossless file must never be mislabelled.
            let path = temp_file(
                "newz-lossless",
                &nikon_tiff(little_endian, "NIKON CORPORATION", Some(3), true, None),
            );
            assert_eq!(nikon_high_efficiency_label(&path), None);
            let _ = std::fs::remove_file(&path);
        }
    }

    #[test]
    fn non_nikon_files_are_never_labelled() {
        let path = temp_file(
            "canon",
            &nikon_tiff(true, "Canon", Some(13), false, None),
        );
        assert_eq!(nikon_high_efficiency_label(&path), None);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn truncated_files_are_never_labelled() {
        let path = temp_file("empty", &[]);
        assert_eq!(nikon_high_efficiency_label(&path), None);
        let _ = std::fs::remove_file(&path);
        let path = temp_file("garbage", &[0x49, 0x49, 0x2A, 0x00, 0xFF, 0xFF]);
        assert_eq!(nikon_high_efficiency_label(&path), None);
        let _ = std::fs::remove_file(&path);
        assert_eq!(nikon_high_efficiency_label("/no/such/file.nef"), None);
    }

    #[test]
    fn orientation_is_read_from_ifd0() {
        let path = temp_file(
            "orientation",
            &nikon_tiff(true, "NIKON CORPORATION", None, false, Some(6)),
        );
        assert_eq!(tiff_ifd0_orientation(&path), 6);
        let _ = std::fs::remove_file(&path);
        let path = temp_file(
            "no-orientation",
            &nikon_tiff(true, "NIKON CORPORATION", None, false, None),
        );
        assert_eq!(tiff_ifd0_orientation(&path), 0);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn high_efficiency_error_message_names_the_routes_forward() {
        let message = high_efficiency_error_message("HE*", "LibRaw error 3: Unsupported file format");
        assert!(message.contains("HE*"));
        assert!(message.contains("TicoRAW"));
        assert!(message.contains("Lossless compression"));
        assert!(message.contains("Adobe DNG Converter"));
        assert!(message.contains("NX Studio"));
    }

    // -- M2 helpers -------------------------------------------------------

    #[test]
    fn bayer_filters_roundtrip_libraw_constants() {
        let cases = [
            ("RGGB", 0x94949494u32),
            ("GRBG", 0x61616161),
            ("GBRG", 0x49494949),
            ("BGGR", 0x16161616),
        ];
        for (name, expected) in cases {
            let cfa = rawler::CFA::new(name);
            assert_eq!(bayer_filters_from_cfa(&cfa), expected, "{name}");
        }
    }

    #[test]
    fn per_channel_levels_broadcast_a_single_entry() {
        assert_eq!(per_channel_levels(&[512.0]), [512.0; 4]);
        assert_eq!(per_channel_levels(&[1.0, 2.0]), [1.0, 2.0, 1.0, 2.0]);
        assert_eq!(per_channel_levels(&[1.0, 2.0, 3.0, 4.0]), [1.0, 2.0, 3.0, 4.0]);
        assert_eq!(per_channel_levels(&[]), [0.0; 4]);
    }

    fn bayer_mosaic(samples: Vec<u16>, black: f32, white: f32) -> RawMosaic {
        RawMosaic {
            width: 2,
            height: 2,
            samples,
            metadata: RawMetadata {
                cfa: CfaPattern::Bayer { filters: 0x94949494 },
                active_area: [0, 0, 2, 2],
                black_level: [black; 4],
                white_level: [white; 4],
                orientation: 0,
                camera_id: "Nikon|ZR".to_string(),
                decoder_source: DECODER_SOURCE_RAWLER.to_string(),
                decoder_version: RAWLER_VERSION.to_string(),
                ..RawMetadata::default()
            },
        }
    }

    #[test]
    fn camera_native_normalization_matches_capture_flag_discipline() {
        // Samples: a mid value, a saturated one, an exactly-black one and one
        // below black. RGGB puts channel 0 at (0,0) and channel 2 at (1,1).
        let mosaic = bayer_mosaic(vec![550, 1000, 100, 50], 100.0, 1000.0);
        let corrected = normalize_camera_native(&mosaic, [1.0; 4]);
        assert!((corrected.samples[0] - 0.5).abs() < 1.0e-6);
        assert!(!corrected.quality.valid[1]);
        assert!(corrected.quality.flags[1].contains(&QualityFlag::SaturatedSample));
        assert!(!corrected.quality.valid[2]);
        assert!(corrected.quality.flags[2].contains(&QualityFlag::NegativeSample));
        assert!(!corrected.quality.valid[3]);
        assert!(corrected.quality.flags[3].contains(&QualityFlag::NegativeSample));

        // White-balance gains scale after normalisation and only attenuate.
        let corrected = normalize_camera_native(&mosaic, [0.5, 1.0, 1.0, 1.0]);
        assert!((corrected.samples[0] - 0.25).abs() < 1.0e-6);

        // A flat black==white table cannot normalise anything.
        let corrected = normalize_camera_native(&bayer_mosaic(vec![550; 4], 100.0, 100.0), [1.0; 4]);
        assert!(corrected
            .quality
            .flags
            .iter()
            .all(|flags| flags.contains(&QualityFlag::InvalidDenominator)));
        assert_eq!(corrected.diagnostics.quality.valid_samples, 0);
    }
}

/// Sample-backed validation, gated on environment variables so the default
/// test run stays hermetic. These tests are the dual-source cross-validation
/// the plan mandates (risk table #1): rawler must agree with LibRaw on a file
/// both backends can decode before either fallback is trusted for the files
/// only one of them can.
/// - `NEXFILM_RAW_SAMPLE_ARW`: any Sony ARW (matrix cross-check against
///   LibRaw's resolved camera→sRGB).
/// - `NEXFILM_RAW_SAMPLE_NEF`: any lossless-compressed Nikon NEF (mosaic
///   metadata + pixel agreement, HE negative check, orientation parity).
#[cfg(test)]
mod sample_tests {
    use super::*;

    fn sample_path(var: &str) -> Option<std::path::PathBuf> {
        std::env::var(var).ok().filter(|value| !value.is_empty()).map(std::path::PathBuf::from)
    }

    #[test]
    fn rawler_matrix_agrees_with_libraw_on_a_shared_camera() {
        let Some(path) = sample_path("NEXFILM_RAW_SAMPLE_ARW") else {
            eprintln!("skipping: set NEXFILM_RAW_SAMPLE_ARW to run the matrix cross-check");
            return;
        };
        let Some((matrix, illuminant)) = camera_to_srgb_fallback(&path).expect("rawler probe") else {
            panic!("rawler offers no colour matrix for {path:?}");
        };
        eprintln!("rawler matrix ({illuminant}): {matrix:?}");
        // Structural invariant of the cam_xyz_coeff conversion: every output
        // row sums to one (a camera-neutral renders as equal RGB).
        for row in 0..3 {
            let sum: f32 = matrix[row * 3..row * 3 + 3].iter().sum();
            assert!((sum - 1.0).abs() < 1.0e-3, "row {row} sums to {sum}");
        }
        assert!(crate::raw_backend::camera_matrix_plausible(matrix));
        // Compare against LibRaw's own resolution through the primary path.
        let options = crate::raw_backend::DecodeOptions {
            half_size: true,
            demosaic_quality: 0,
            output_bps: 16,
            no_auto_bright: true,
            output_color: 0,
            linear_gamma: true,
            use_camera_wb: false,
        };
        let libraw = crate::raw_backend::libraw_extract_camera_rgb_with_policy(
            &path,
            &options,
            crate::raw_backend::WhiteBalancePolicy::NormalizedAsShot,
        )
        .expect("libraw decode");
        eprintln!("libraw matrix: {:?}", libraw.camera_to_srgb);
        let deviation: f32 = matrix
            .iter()
            .zip(libraw.camera_to_srgb.iter())
            .map(|(a, b)| (a - b).abs())
            .fold(0.0, f32::max);
        eprintln!("max element deviation: {deviation}");
        // This comparison can only flag gross conversion errors (a flipped
        // direction or transpose lands far beyond 1.0). A moderate deviation
        // is expected on bodies where LibRaw itself only carries a generic
        // family-fallback table instead of per-camera data: the two matrices
        // then genuinely describe different colour characterisations, which
        // is exactly the case M1 exists to repair.
        assert!(deviation < 1.0, "matrix deviation {deviation} too large");
    }

    /// The M1 acceptance file: an X2D 3FR/FFF that LibRaw decodes with the
    /// identity matrix on the bundled 0.21.3 — the reported yellow-export
    /// repro. `NEXFILM_RAW_SAMPLE_3FR` points at the raw capture (the
    /// exported TIF alone cannot exercise the decode stage; ask the reporter
    /// for the original 3FR the TIF was exported from).
    #[test]
    fn x2d_capture_gets_the_matrix_backfilled() {
        let Some(path) = sample_path("NEXFILM_RAW_SAMPLE_3FR") else {
            eprintln!(
                "skipping: set NEXFILM_RAW_SAMPLE_3FR to the X2D 3FR behind the reported 0218-.tif export"
            );
            return;
        };
        let options = crate::raw_backend::DecodeOptions {
            half_size: true,
            demosaic_quality: 3,
            output_bps: 16,
            no_auto_bright: true,
            output_color: 0,
            linear_gamma: true,
            use_camera_wb: false,
        };
        // Trigger condition: LibRaw decodes the pixels but without a camera
        // matrix — the exact identity-matrix case M1 exists for.
        let libraw = crate::raw_backend::libraw_extract_camera_rgb_with_policy(
            &path,
            &options,
            crate::raw_backend::WhiteBalancePolicy::NormalizedAsShot,
        )
        .expect("libraw must decode X2D pixels");
        eprintln!("libraw camera matrix: {:?}", libraw.camera_to_srgb);
        assert!(crate::raw_backend::camera_matrix_missing(libraw.camera_to_srgb));

        // The backfill: rawler's per-camera D65 table, converted to the exact
        // LibRaw convention.
        let Some((matrix, illuminant)) = camera_to_srgb_fallback(&path).expect("rawler probe")
        else {
            panic!("rawler offers no colour matrix for {path:?}");
        };
        eprintln!("rawler matrix ({illuminant}): {matrix:?}");
        for row in 0..3 {
            let sum: f32 = matrix[row * 3..row * 3 + 3].iter().sum();
            assert!((sum - 1.0).abs() < 1.0e-3, "row {row} sums to {sum}");
        }
        assert!(crate::raw_backend::camera_matrix_plausible(matrix));

        // Through the dispatch wrapper the matrix arrives replaced, with its
        // provenance recorded.
        let decoded = crate::raw_backend::extract_camera_rgb_with_policy(
            &path,
            &options,
            crate::raw_backend::WhiteBalancePolicy::NormalizedAsShot,
        )
        .expect("wrapper decode");
        assert_eq!(
            decoded.camera_to_srgb_source,
            crate::raw_backend::CameraMatrixSource::RawlerFallback(illuminant.clone())
        );
        assert_eq!(decoded.camera_to_srgb, matrix);

        // Scene-independent sanity: the backfilled decode must differ from the
        // sensor-native one (otherwise the repair would be a no-op), and the
        // replacement matrix must differ from identity beyond the missing-
        // matrix tolerance.
        let identity_delta: f32 = matrix
            .iter()
            .zip([
                1.0f32, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0,
            ])
            .map(|(a, b)| (a - b).abs())
            .fold(0.0, f32::max);
        assert!(identity_delta > 0.05, "backfilled matrix is near-identity");
        eprintln!("matrix departs from identity by {identity_delta}");
    }

    #[test]
    fn rawler_mosaic_agrees_with_libraw_on_a_lossless_nef() {
        let Some(path) = sample_path("NEXFILM_RAW_SAMPLE_NEF") else {
            eprintln!("skipping: set NEXFILM_RAW_SAMPLE_NEF to run the mosaic cross-check");
            return;
        };
        let libraw = crate::raw_backend::libraw_decode_raw_mosaic(&path).expect("libraw mosaic");
        let rawler = decode_mosaic(&path).expect("rawler mosaic");

        assert_eq!((libraw.width, libraw.height), (rawler.width, rawler.height));
        assert_eq!(
            libraw.metadata.active_area,
            rawler.metadata.active_area,
            "active area"
        );
        // CFA phase must match per-site as RGB. The packed words may differ:
        // LibRaw labels some bodies four-colour (second green = code 3), e.g.
        // a D810 arrives as 0xB4B4B4B4 against rawler's 0x94949494, with an
        // identical physical phase.
        let phase = |cfa: crate::raw_backend::CfaPattern| match cfa {
            crate::raw_backend::CfaPattern::Bayer { filters } => [
                crate::raw_backend::bayer_rgb_channel(filters, 0, 0),
                crate::raw_backend::bayer_rgb_channel(filters, 0, 1),
                crate::raw_backend::bayer_rgb_channel(filters, 1, 0),
                crate::raw_backend::bayer_rgb_channel(filters, 1, 1),
            ],
            _ => [9; 4],
        };
        assert_eq!(
            phase(libraw.metadata.cfa),
            phase(rawler.metadata.cfa),
            "CFA phase must match LibRaw exactly"
        );
        // Lossless NEF: rawler's white level is the sensor limit; LibRaw may
        // resolve a tighter linear_max, so compare black levels and allow the
        // white levels to be within the sensor range.
        for channel in 0..4 {
            assert!(
                (libraw.metadata.black_level[channel] - rawler.metadata.black_level[channel]).abs()
                    <= 2.0,
                "black level channel {channel}: {} vs {}",
                libraw.metadata.black_level[channel],
                rawler.metadata.black_level[channel]
            );
        }
        eprintln!(
            "white libraw={:?} rawler={:?}",
            libraw.metadata.white_level, rawler.metadata.white_level
        );
        eprintln!(
            "black libraw={:?} rawler={:?}",
            libraw.metadata.black_level, rawler.metadata.black_level
        );
        eprintln!(
            "iso/exposure libraw={:?}/{:?} rawler={:?}/{:?} camera {} vs {} orientation libraw={} rawler={}",
            libraw.metadata.iso, libraw.metadata.exposure_seconds,
            rawler.metadata.iso, rawler.metadata.exposure_seconds,
            libraw.metadata.camera_id, rawler.metadata.camera_id,
            libraw.metadata.orientation, rawler.metadata.orientation
        );
        // The HE check must be negative for a lossless file.
        assert_eq!(nikon_high_efficiency_label(&path), None);
        // Orientation parity with the LibRaw path.
        assert_eq!(
            libraw.metadata.orientation, rawler.metadata.orientation,
            "orientation must match the LibRaw flip code"
        );
        // Conversion equivalence: on a body LibRaw knows, its rgb_cam is the
        // cam_xyz_coeff product of a real per-camera table, so the rawler
        // table converted through the same recipe must land within the
        // between-databases noise of the two sources.
        let converted = camera_to_srgb_fallback(&path)
            .expect("rawler probe")
            .expect("rawler matrix for a supported camera")
            .0;
        let options = crate::raw_backend::DecodeOptions {
            half_size: true,
            demosaic_quality: 0,
            output_bps: 16,
            no_auto_bright: true,
            output_color: 0,
            linear_gamma: true,
            use_camera_wb: false,
        };
        let libraw_rgb = crate::raw_backend::libraw_extract_camera_rgb_with_policy(
            &path,
            &options,
            crate::raw_backend::WhiteBalancePolicy::NormalizedAsShot,
        )
        .expect("libraw decode");
        let deviation: f32 = converted
            .iter()
            .zip(libraw_rgb.camera_to_srgb.iter())
            .map(|(a, b)| (a - b).abs())
            .fold(0.0, f32::max);
        eprintln!("matrix conversion deviation vs libraw: {deviation}");
        eprintln!("rawler-converted: {converted:?}");
        eprintln!("libraw rgb_cam:   {:?}", libraw_rgb.camera_to_srgb);
        assert!(
            deviation < 0.2,
            "the xyz_to_cam conversion diverges from LibRaw's own product: {deviation}"
        );
        // Pixels: lossless NEF decompression agrees between the backends up
        // to 1 LSB of rounding (measured on a D810 sample: every deviation is
        // exactly one quantisation step on a 14-bit sensor).
        let max_abs_diff = libraw
            .samples
            .iter()
            .zip(rawler.samples.iter())
            .map(|(a, b)| (*a as i32 - *b as i32).abs())
            .fold(0, i32::max);
        assert!(
            max_abs_diff <= 2,
            "rawler pixels diverge from LibRaw by {max_abs_diff} LSB"
        );
    }
}

