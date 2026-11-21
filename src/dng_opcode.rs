//! DNG stage-2 opcode execution.
//!
//! Adobe's DNG writer may store JPEG XL or Lossy JPEG LinearRaw samples after
//! a per-channel tone compression. `OpcodeList2` carries the inverse mapping
//! that has to run on the normalised, full-resolution active area before any
//! rotation or downsampling.

const OPCODE_OPTIONAL: u32 = 1;
const OPCODE_MAP_TABLE: u32 = 7;
const OPCODE_MAP_POLYNOMIAL: u32 = 8;
const MAX_POLYNOMIAL_DEGREE: u32 = 8;
const MIN_TABLE_ENTRIES: u32 = 2;
const MAX_TABLE_ENTRIES: u32 = 65_536;
const PARAMETER_HEADER_BYTES: usize = 9 * 4;

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct OpcodeListSummary {
    pub(crate) opcode_count: usize,
    pub(crate) map_polynomial_count: usize,
    pub(crate) map_table_count: usize,
    pub(crate) skipped_optional_count: usize,
    pub(crate) pixel_writes: u64,
}

#[derive(Debug, Clone)]
struct OpcodeHeader {
    id: u32,
    flags: u32,
    params: Vec<u8>,
}

#[derive(Debug, Clone)]
enum RectangleOpcode {
    MapPolynomial {
        shape: OpcodeShape,
        coefficients: Vec<f64>,
    },
    MapTable {
        shape: OpcodeShape,
        table: Vec<u16>,
    },
}

#[derive(Debug, Clone, Copy)]
struct OpcodeShape {
    top: u32,
    left: u32,
    bottom: u32,
    right: u32,
    plane: u32,
    planes: u32,
    row_pitch: u32,
    col_pitch: u32,
}

impl OpcodeShape {
    fn parse(params: &[u8]) -> Result<Self, String> {
        if params.len() < PARAMETER_HEADER_BYTES {
            return Err(format!(
                "DNG opcode parameters are {} bytes, shorter than the {PARAMETER_HEADER_BYTES}-byte rectangle header",
                params.len()
            ));
        }
        let value = |index: usize| read_u32_be(params, index * 4);
        Ok(Self {
            top: value(0)?,
            left: value(1)?,
            bottom: value(2)?,
            right: value(3)?,
            plane: value(4)?,
            planes: value(5)?,
            row_pitch: value(6)?,
            col_pitch: value(7)?,
        })
    }

    fn rows(&self, active_top: u32, height: u32) -> (usize, usize, usize) {
        clipped_axis(self.top, self.bottom, active_top, height, self.row_pitch)
    }

    fn columns(&self, active_left: u32, width: u32) -> (usize, usize, usize) {
        clipped_axis(self.left, self.right, active_left, width, self.col_pitch)
    }

    fn channels(&self) -> std::ops::Range<usize> {
        let start = usize::try_from(self.plane).unwrap_or(usize::MAX);
        let count = usize::try_from(self.planes).unwrap_or(0);
        start.min(3)..start.saturating_add(count).min(3)
    }
}

fn clipped_axis(
    start: u32,
    end: u32,
    origin: u32,
    length: u32,
    pitch: u32,
) -> (usize, usize, usize) {
    let origin = u64::from(origin);
    let limit = u64::from(length);
    let first = (origin + u64::from(start)).min(limit) as usize;
    let last = (origin + u64::from(end)).min(limit) as usize;
    let pitch = usize::try_from(pitch.max(1)).unwrap_or(1);
    (first, last, pitch)
}

fn read_u32_be(data: &[u8], offset: usize) -> Result<u32, String> {
    let bytes = data
        .get(offset..offset + 4)
        .ok_or_else(|| "DNG opcode list is truncated".to_string())?;
    Ok(u32::from_be_bytes(
        bytes.try_into().expect("four-byte opcode field"),
    ))
}

fn read_f64_be(data: &[u8], offset: usize) -> Result<f64, String> {
    let bytes = data
        .get(offset..offset + 8)
        .ok_or_else(|| "DNG polynomial coefficients are truncated".to_string())?;
    Ok(f64::from_be_bytes(
        bytes.try_into().expect("eight-byte polynomial coefficient"),
    ))
}

fn read_u16_be(data: &[u8], offset: usize) -> Result<u16, String> {
    let bytes = data
        .get(offset..offset + 2)
        .ok_or_else(|| "DNG map table is truncated".to_string())?;
    Ok(u16::from_be_bytes(
        bytes.try_into().expect("two-byte map table entry"),
    ))
}

fn parse_opcode_list(data: &[u8]) -> Result<(Vec<RectangleOpcode>, usize), String> {
    let opcode_count = usize::try_from(read_u32_be(data, 0)?)
        .map_err(|_| "DNG opcode count exceeds the platform limit".to_string())?;
    // Every opcode needs at least its 16-byte header, so this bounds parsing
    // before any per-opcode allocation.
    if opcode_count > (data.len().saturating_sub(4)) / 16 {
        return Err(format!(
            "DNG opcode list declares {opcode_count} opcodes in only {} bytes",
            data.len()
        ));
    }

    let mut headers = Vec::with_capacity(opcode_count);
    let mut offset = 4usize;
    for _ in 0..opcode_count {
        let id = read_u32_be(data, offset)?;
        let _version = read_u32_be(data, offset + 4)?;
        let flags = read_u32_be(data, offset + 8)?;
        let param_bytes = usize::try_from(read_u32_be(data, offset + 12)?)
            .map_err(|_| "DNG opcode parameter length exceeds the platform limit".to_string())?;
        offset += 16;
        let end = offset
            .checked_add(param_bytes)
            .ok_or_else(|| "DNG opcode parameter length overflowed".to_string())?;
        let params = data
            .get(offset..end)
            .ok_or_else(|| "DNG opcode parameters run past the list".to_string())?
            .to_vec();
        headers.push(OpcodeHeader { id, flags, params });
        offset = end;
    }
    if offset != data.len() {
        return Err(format!(
            "DNG opcode list has {} trailing bytes",
            data.len() - offset
        ));
    }

    let mut opcodes = Vec::new();
    let mut skipped_optional = 0usize;
    for header in headers {
        match header.id {
            OPCODE_MAP_POLYNOMIAL => {
                opcodes.push(parse_map_polynomial(&header.params)?);
            }
            OPCODE_MAP_TABLE => {
                opcodes.push(parse_map_table(&header.params)?);
            }
            _ if header.flags & OPCODE_OPTIONAL != 0 => {
                skipped_optional += 1;
            }
            _ => {
                return Err(format!(
                    "DNG OpcodeList2 contains unsupported required opcode {} \
                     (flags 0x{:08x})",
                    header.id, header.flags
                ));
            }
        }
    }
    Ok((opcodes, skipped_optional))
}

fn parse_map_polynomial(params: &[u8]) -> Result<RectangleOpcode, String> {
    let shape = OpcodeShape::parse(params)?;
    let degree = usize::try_from(read_u32_be(params, 8 * 4)?)
        .map_err(|_| "DNG polynomial degree exceeds the platform limit".to_string())?;
    if degree > MAX_POLYNOMIAL_DEGREE as usize {
        return Err(format!(
            "DNG MapPolynomial degree {degree} exceeds the supported maximum {MAX_POLYNOMIAL_DEGREE}"
        ));
    }
    let coefficient_count = degree + 1;
    let expected = PARAMETER_HEADER_BYTES + coefficient_count * 8;
    if params.len() != expected {
        return Err(format!(
            "DNG MapPolynomial degree {degree} needs {expected} parameter bytes, found {}",
            params.len()
        ));
    }
    let coefficients = (0..coefficient_count)
        .map(|index| read_f64_be(params, PARAMETER_HEADER_BYTES + index * 8))
        .collect::<Result<Vec<_>, _>>()?;
    if coefficients.iter().any(|value| !value.is_finite()) {
        return Err("DNG MapPolynomial contains a non-finite coefficient".to_string());
    }
    Ok(RectangleOpcode::MapPolynomial {
        shape,
        coefficients,
    })
}

fn parse_map_table(params: &[u8]) -> Result<RectangleOpcode, String> {
    let shape = OpcodeShape::parse(params)?;
    let count = read_u32_be(params, 8 * 4)?;
    if !(MIN_TABLE_ENTRIES..=MAX_TABLE_ENTRIES).contains(&count) {
        return Err(format!(
            "DNG MapTable entry count {count} is outside the supported range \
             {MIN_TABLE_ENTRIES}..={MAX_TABLE_ENTRIES}"
        ));
    }
    let count_usize = count as usize;
    let expected = PARAMETER_HEADER_BYTES + count_usize * 2;
    if params.len() != expected {
        return Err(format!(
            "DNG MapTable with {count} entries needs {expected} parameter bytes, found {}",
            params.len()
        ));
    }
    let table = (0..count_usize)
        .map(|index| read_u16_be(params, PARAMETER_HEADER_BYTES + index * 2))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(RectangleOpcode::MapTable { shape, table })
}

/// Apply an `OpcodeList2` payload to normalised, interleaved RGB f32 samples.
///
/// `active_left`/`active_top` are the full-image origin of the DNG active area.
/// Opcode rectangles and pitch are expressed in active-area coordinates, so the
/// origin is added before clipping to the decoded image bounds.
pub(crate) fn apply_opcode_list2(
    data: &[u8],
    width: u32,
    height: u32,
    active_left: u32,
    active_top: u32,
    pixels: &mut [f32],
) -> Result<OpcodeListSummary, String> {
    let expected = usize::try_from(width)
        .ok()
        .and_then(|width| {
            usize::try_from(height)
                .ok()
                .and_then(|height| width.checked_mul(height))
        })
        .and_then(|count| count.checked_mul(3))
        .ok_or_else(|| "DNG image dimensions overflowed".to_string())?;
    if pixels.len() != expected {
        return Err(format!(
            "DNG opcode target carries {} samples for {width}x{height} RGB",
            pixels.len()
        ));
    }

    let (opcodes, skipped_optional_count) = parse_opcode_list(data)?;
    let mut summary = OpcodeListSummary {
        opcode_count: opcodes.len() + skipped_optional_count,
        map_polynomial_count: 0,
        map_table_count: 0,
        skipped_optional_count,
        pixel_writes: 0,
    };
    let width_usize = width as usize;

    for opcode in &opcodes {
        match opcode {
            RectangleOpcode::MapPolynomial {
                shape,
                coefficients,
            } => {
                summary.map_polynomial_count += 1;
                let (top, bottom, row_pitch) = shape.rows(active_top, height);
                let (left, right, col_pitch) = shape.columns(active_left, width);
                let channels = shape.channels();
                for y in (top..bottom).step_by(row_pitch) {
                    for x in (left..right).step_by(col_pitch) {
                        let pixel_base = (y * width_usize + x) * 3;
                        for channel in channels.clone() {
                            let value = pixels[pixel_base + channel];
                            let mapped = evaluate_polynomial(coefficients, f64::from(value));
                            pixels[pixel_base + channel] = safe_mapped_value(mapped);
                            summary.pixel_writes += 1;
                        }
                    }
                }
            }
            RectangleOpcode::MapTable { shape, table } => {
                summary.map_table_count += 1;
                let (top, bottom, row_pitch) = shape.rows(active_top, height);
                let (left, right, col_pitch) = shape.columns(active_left, width);
                let channels = shape.channels();
                let last_index = (table.len() - 1) as f64;
                for y in (top..bottom).step_by(row_pitch) {
                    for x in (left..right).step_by(col_pitch) {
                        let pixel_base = (y * width_usize + x) * 3;
                        for channel in channels.clone() {
                            let value = f64::from(pixels[pixel_base + channel]);
                            let index = (value.clamp(0.0, 1.0) * last_index).round() as usize;
                            pixels[pixel_base + channel] = f32::from(table[index]) / 65535.0;
                            summary.pixel_writes += 1;
                        }
                    }
                }
            }
        }
    }
    Ok(summary)
}

fn evaluate_polynomial(coefficients: &[f64], x: f64) -> f64 {
    let mut value = 0.0;
    for coefficient in coefficients.iter().rev() {
        value = value * x + coefficient;
    }
    value
}

fn safe_mapped_value(value: f64) -> f32 {
    if !value.is_finite() || value < 0.0 {
        0.0
    } else {
        value as f32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn push_opcode(output: &mut Vec<u8>, id: u32, flags: u32, params: &[u8]) {
        output.extend_from_slice(&id.to_be_bytes());
        output.extend_from_slice(&0x0001_0300u32.to_be_bytes());
        output.extend_from_slice(&flags.to_be_bytes());
        output.extend_from_slice(&(params.len() as u32).to_be_bytes());
        output.extend_from_slice(params);
    }

    fn push_shape(output: &mut Vec<u8>, shape: [u32; 8]) {
        for value in shape {
            output.extend_from_slice(&value.to_be_bytes());
        }
    }

    fn polynomial_params(shape: [u32; 8], coefficients: &[f64]) -> Vec<u8> {
        let mut params = Vec::new();
        push_shape(&mut params, shape);
        params.extend_from_slice(&((coefficients.len() - 1) as u32).to_be_bytes());
        for coefficient in coefficients {
            params.extend_from_slice(&coefficient.to_be_bytes());
        }
        params
    }

    fn table_params(shape: [u32; 8], table: &[u16]) -> Vec<u8> {
        let mut params = Vec::new();
        push_shape(&mut params, shape);
        params.extend_from_slice(&(table.len() as u32).to_be_bytes());
        for value in table {
            params.extend_from_slice(&value.to_be_bytes());
        }
        params
    }

    fn with_count(opcodes: Vec<u8>, count: u32) -> Vec<u8> {
        let mut data = Vec::new();
        data.extend_from_slice(&count.to_be_bytes());
        data.extend_from_slice(&opcodes);
        data
    }

    #[test]
    fn polynomial_table_planes_and_pitch_follow_big_endian_parameters() {
        let mut opcodes = Vec::new();
        push_opcode(
            &mut opcodes,
            OPCODE_MAP_POLYNOMIAL,
            0,
            &polynomial_params([0, 0, 3, 3, 0, 1, 1, 1], &[0.25, 0.5]),
        );
        push_opcode(
            &mut opcodes,
            OPCODE_MAP_POLYNOMIAL,
            0,
            &polynomial_params([0, 0, 3, 3, 1, 1, 1, 2], &[0.0, 0.0, 1.0]),
        );
        push_opcode(
            &mut opcodes,
            OPCODE_MAP_TABLE,
            0,
            &table_params([1, 1, 3, 3, 2, 1, 2, 1], &[65535, 0]),
        );
        let list = with_count(opcodes, 3);

        let mut pixels = vec![0.0f32; 3 * 3 * 3];
        for pixel in pixels.chunks_exact_mut(3) {
            pixel.copy_from_slice(&[0.0, 0.5, 0.5]);
        }
        let summary =
            apply_opcode_list2(&list, 3, 3, 0, 0, &mut pixels).expect("valid opcode list");
        assert_eq!(summary.map_polynomial_count, 2);
        assert_eq!(summary.map_table_count, 1);
        assert_eq!(summary.skipped_optional_count, 0);

        for y in 0..3 {
            for x in 0..3 {
                let pixel = &pixels[(y * 3 + x) * 3..(y * 3 + x) * 3 + 3];
                assert!((pixel[0] - 0.25).abs() < 1.0e-6, "R at {x},{y}: {pixel:?}");
                if x % 2 == 0 {
                    assert!((pixel[1] - 0.25).abs() < 1.0e-6, "G at {x},{y}: {pixel:?}");
                } else {
                    assert!((pixel[1] - 0.5).abs() < 1.0e-6, "G at {x},{y}: {pixel:?}");
                }
                if y == 1 && x >= 1 {
                    assert!(pixel[2].abs() < 1.0e-6, "B at {x},{y}: {pixel:?}");
                } else {
                    assert!((pixel[2] - 0.5).abs() < 1.0e-6, "B at {x},{y}: {pixel:?}");
                }
            }
        }
    }

    #[test]
    fn active_area_origin_offsets_opcode_coordinates() {
        let mut opcodes = Vec::new();
        push_opcode(
            &mut opcodes,
            OPCODE_MAP_POLYNOMIAL,
            0,
            &polynomial_params([0, 0, 1, 1, 0, 1, 1, 1], &[1.0]),
        );
        let list = with_count(opcodes, 1);
        let mut pixels = vec![0.0f32; 4 * 4 * 3];
        let summary =
            apply_opcode_list2(&list, 4, 4, 2, 1, &mut pixels).expect("valid opcode list");
        assert_eq!(summary.pixel_writes, 1);
        let red_at = |x: usize, y: usize| pixels[(y * 4 + x) * 3];
        assert_eq!(red_at(2, 1), 1.0);
        assert_eq!(red_at(0, 0), 0.0);
    }

    #[test]
    fn optional_unknown_opcode_is_skipped_and_required_unknown_is_reported() {
        let mut optional = Vec::new();
        push_opcode(&mut optional, 99, OPCODE_OPTIONAL, &[1, 2, 3, 4]);
        let list = with_count(optional, 1);
        let mut pixels = vec![0.5f32; 3];
        let summary = apply_opcode_list2(&list, 1, 1, 0, 0, &mut pixels).expect("optional opcode");
        assert_eq!(summary.skipped_optional_count, 1);
        assert_eq!(pixels, vec![0.5; 3]);

        let mut required = Vec::new();
        push_opcode(&mut required, 99, 0, &[1, 2, 3, 4]);
        let error = apply_opcode_list2(&with_count(required, 1), 1, 1, 0, 0, &mut pixels)
            .expect_err("required unknown opcode must fail clearly");
        assert!(error.contains("unsupported required opcode 99"), "{error}");
    }

    #[test]
    fn truncated_and_invalid_opcode_lists_return_errors() {
        let mut list = Vec::new();
        list.extend_from_slice(&1u32.to_be_bytes());
        list.extend_from_slice(&8u32.to_be_bytes());
        assert!(apply_opcode_list2(&list, 1, 1, 0, 0, &mut [0.0; 3]).is_err());

        let mut bad_degree = Vec::new();
        push_opcode(
            &mut bad_degree,
            OPCODE_MAP_POLYNOMIAL,
            0,
            &polynomial_params([0, 0, 1, 1, 0, 1, 1, 1], &[0.0; 10]),
        );
        let error = apply_opcode_list2(&with_count(bad_degree, 1), 1, 1, 0, 0, &mut [0.0; 3])
            .expect_err("degree above eight must fail");
        assert!(error.contains("degree 9"), "{error}");

        let mut short_params = Vec::new();
        let mut params = Vec::new();
        push_shape(&mut params, [0, 0, 1, 1, 0, 1, 1, 1]);
        params.extend_from_slice(&3u32.to_be_bytes());
        params.extend_from_slice(&0.0f64.to_be_bytes());
        push_opcode(&mut short_params, OPCODE_MAP_POLYNOMIAL, 0, &params);
        assert!(
            apply_opcode_list2(&with_count(short_params, 1), 1, 1, 0, 0, &mut [0.0; 3]).is_err()
        );
    }
}
