//! Measurement-backed Capture Separation fitting.
//!
//! This module intentionally models only the pre-log transmission correction.
//! It does not implement a film H-D curve, a digital mask, or Status M.

use nalgebra::{DMatrix, Matrix3, SVD};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

const MIN_PATCHES: usize = 4;
const MAX_CONDITION_NUMBER: f32 = 1.0e5;
const MAX_VALIDATION_RMSE: f32 = 0.08;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ReferenceDomain {
    CameraNativeTransmissionRgb,
    TransmissionRgb,
    DensityRgb,
    PcsXyzIccRgb,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FitModelType {
    CaptureSeparation3x3,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CalibrationPatch {
    #[serde(alias = "patch_id")]
    pub patch_id: String,
    pub position: Option<[f32; 2]>,
    #[serde(alias = "capture_transmission")]
    pub capture_transmission: [f32; 3],
    #[serde(alias = "reference_value")]
    pub reference_value: [f32; 3],
    #[serde(alias = "reference_domain")]
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CalibrationMeasurementSet {
    #[serde(alias = "dark_frame")]
    pub dark_frame: String,
    #[serde(alias = "open_frame")]
    pub open_frame: String,
    #[serde(default)]
    #[serde(alias = "flat_frame")]
    pub flat_frame: Option<String>,
    #[serde(alias = "target_frame")]
    pub target_frame: String,
    pub patches: Vec<CalibrationPatch>,
    #[serde(alias = "camera_model")]
    pub camera_model: String,
    #[serde(alias = "scanner_model")]
    pub scanner_model: Option<String>,
    pub iso: Option<f32>,
    pub exposure: Option<f32>,
    #[serde(alias = "light_source")]
    pub light_source: String,
    pub resolution: Option<u32>,
    #[serde(alias = "crop_geometry")]
    pub crop_geometry: String,
    #[serde(alias = "raw_decode_version")]
    pub raw_decode_version: String,
    #[serde(alias = "input_digest")]
    pub input_digest: String,
    #[serde(default)]
    #[serde(alias = "quality_mask_digest")]
    pub quality_mask_digest: String,
}

impl CalibrationMeasurementSet {
    pub fn validate(&self) -> Result<(), String> {
        for (name, value) in [
            ("dark_frame", self.dark_frame.as_str()),
            ("open_frame", self.open_frame.as_str()),
            ("target_frame", self.target_frame.as_str()),
            ("camera_model", self.camera_model.as_str()),
            ("light_source", self.light_source.as_str()),
            ("crop_geometry", self.crop_geometry.as_str()),
            ("raw_decode_version", self.raw_decode_version.as_str()),
            ("input_digest", self.input_digest.as_str()),
        ] {
            if value.trim().is_empty() {
                return Err(format!("measurement_{name}_missing"));
            }
        }
        if self
            .flat_frame
            .as_deref()
            .is_some_and(|value| value.trim().is_empty())
        {
            return Err("measurement_flat_frame_invalid".into());
        }
        if self.quality_mask_digest.trim().is_empty() {
            return Err("measurement_quality_mask_digest_missing".into());
        }
        if self.patches.is_empty() {
            return Err("measurement_patches_missing".into());
        }
        if self
            .patches
            .iter()
            .any(|patch| patch.patch_id.trim().is_empty())
        {
            return Err("measurement_patch_id_missing".into());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FitDiagnostics {
    pub rank: usize,
    pub condition_number: f32,
    pub training_patch_count: usize,
    pub validation_patch_count: usize,
    pub rejected_patch_count: usize,
    pub channel_rmse: [f32; 3],
    pub training_rmse: f32,
    pub validation_rmse: f32,
    pub max_abs_error: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CalibrationFitModel {
    pub model_type: FitModelType,
    pub source_domain: ReferenceDomain,
    pub target_domain: ReferenceDomain,
    pub pipeline_order: String,
    pub matrix: [[f32; 3]; 3],
    pub offset: [f32; 3],
    pub diagnostics: FitDiagnostics,
    pub measurement_digest: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct FitOptions {
    #[serde(default = "default_max_condition_number")]
    #[serde(alias = "max_condition_number")]
    pub max_condition_number: f32,
    #[serde(default = "default_max_validation_rmse")]
    #[serde(alias = "max_validation_rmse")]
    pub max_validation_rmse: f32,
    #[serde(default)]
    #[serde(alias = "fit_offset")]
    pub fit_offset: bool,
}

fn default_max_condition_number() -> f32 {
    MAX_CONDITION_NUMBER
}
fn default_max_validation_rmse() -> f32 {
    MAX_VALIDATION_RMSE
}

impl Default for FitOptions {
    fn default() -> Self {
        Self {
            max_condition_number: MAX_CONDITION_NUMBER,
            max_validation_rmse: MAX_VALIDATION_RMSE,
            fit_offset: false,
        }
    }
}

fn target_transmission(patch: &CalibrationPatch) -> Result<[f32; 3], String> {
    match patch.reference_domain {
        ReferenceDomain::TransmissionRgb => Ok(patch.reference_value),
        ReferenceDomain::DensityRgb => Ok(patch.reference_value.map(|d| 10.0_f32.powf(-d))),
        ReferenceDomain::CameraNativeTransmissionRgb => {
            Err("camera_native_reference_is_not_target_domain".into())
        }
        ReferenceDomain::PcsXyzIccRgb => Err("pcs_reference_is_not_density_calibration".into()),
    }
}

fn valid_patch(patch: &CalibrationPatch) -> bool {
    !patch.saturated
        && !patch.bad_pixel
        && !patch.outlier
        && patch
            .capture_transmission
            .iter()
            .all(|v| v.is_finite() && *v > 0.0)
        && patch.reference_value.iter().all(|v| v.is_finite())
        && patch
            .position
            .is_none_or(|p| p.iter().all(|v| v.is_finite()))
}

fn matrix_to_array(matrix: &Matrix3<f32>) -> [[f32; 3]; 3] {
    [
        [matrix[(0, 0)], matrix[(0, 1)], matrix[(0, 2)]],
        [matrix[(1, 0)], matrix[(1, 1)], matrix[(1, 2)]],
        [matrix[(2, 0)], matrix[(2, 1)], matrix[(2, 2)]],
    ]
}

pub fn measurement_digest(measurements: &CalibrationMeasurementSet) -> Result<String, String> {
    let bytes = serde_json::to_vec(measurements)
        .map_err(|error| format!("fit_measurement_digest_serialize_failed|{error}"))?;
    Ok(format!("{:x}", Sha256::digest(bytes)))
}

fn apply_raw(matrix: [[f32; 3]; 3], offset: [f32; 3], input: [f32; 3]) -> [f32; 3] {
    [
        matrix[0][0] * input[0] + matrix[0][1] * input[1] + matrix[0][2] * input[2] + offset[0],
        matrix[1][0] * input[0] + matrix[1][1] * input[1] + matrix[1][2] * input[2] + offset[1],
        matrix[2][0] * input[0] + matrix[2][1] * input[1] + matrix[2][2] * input[2] + offset[2],
    ]
}

pub fn apply_capture_separation(model: &CalibrationFitModel, input: [f32; 3]) -> Option<[f32; 3]> {
    if input.iter().any(|v| !v.is_finite() || *v <= 0.0) {
        return None;
    }
    let output = apply_raw(model.matrix, model.offset, input);
    output
        .iter()
        .all(|v| v.is_finite() && *v > 0.0)
        .then_some(output)
}

/// Conservative positivity check for an affine fit over the normalized
/// transmission cube. This prevents an optional offset from producing a
/// negative transmission at any cube corner before `log10`.
pub fn preserves_positive_unit_transmission(model: &CalibrationFitModel) -> bool {
    (0..8).all(|bits| {
        let input = [
            if bits & 1 == 0 { 1.0e-6 } else { 1.0 },
            if bits & 2 == 0 { 1.0e-6 } else { 1.0 },
            if bits & 4 == 0 { 1.0e-6 } else { 1.0 },
        ];
        apply_raw(model.matrix, model.offset, input)
            .iter()
            .all(|value| value.is_finite() && *value > 0.0)
    })
}

pub fn fit_capture_separation(
    measurements: &CalibrationMeasurementSet,
    options: FitOptions,
) -> Result<CalibrationFitModel, String> {
    measurements.validate()?;
    if !options.max_condition_number.is_finite() || options.max_condition_number <= 1.0 {
        return Err("fit_condition_limit_invalid".into());
    }
    if !options.max_validation_rmse.is_finite() || options.max_validation_rmse <= 0.0 {
        return Err("fit_validation_limit_invalid".into());
    }
    let mut domains = measurements
        .patches
        .iter()
        .filter(|p| valid_patch(p))
        .map(|p| p.reference_domain);
    let Some(domain) = domains.clone().next() else {
        return Err("fit_no_valid_patches".into());
    };
    if domains.any(|candidate| candidate != domain) {
        return Err("fit_reference_domains_mixed".into());
    }
    let mut train = Vec::new();
    let mut validation = Vec::new();
    let mut rejected = 0usize;
    for patch in &measurements.patches {
        if !valid_patch(patch) {
            rejected += 1;
            continue;
        }
        let target = target_transmission(patch)?;
        if target.iter().any(|v| !v.is_finite() || *v <= 0.0) {
            rejected += 1;
            continue;
        }
        if patch.validation {
            validation.push((patch.capture_transmission, target));
        } else {
            train.push((patch.capture_transmission, target));
        }
    }
    let parameter_count = if options.fit_offset { 4 } else { 3 };
    let minimum_training_patches = MIN_PATCHES.max(parameter_count + 1);
    if train.len() < minimum_training_patches {
        return Err(format!(
            "fit_insufficient_training_patches|need={minimum_training_patches}|actual={}",
            train.len()
        ));
    }
    if validation.is_empty() {
        return Err("fit_validation_patches_missing".into());
    }
    let x = DMatrix::from_fn(train.len(), parameter_count, |r, c| {
        if c < 3 {
            train[r].0[c]
        } else {
            1.0
        }
    });
    let y = DMatrix::from_fn(train.len(), 3, |r, c| train[r].1[c]);
    let svd = SVD::new(x.clone(), true, true);
    let singular = svd.singular_values.as_slice();
    let rank = singular.iter().filter(|v| **v > 1.0e-6).count();
    let condition_number = singular.first().copied().unwrap_or(f32::INFINITY)
        / singular.last().copied().unwrap_or(0.0).max(1.0e-12);
    if rank < parameter_count {
        return Err("fit_matrix_rank_deficient".into());
    }
    if !condition_number.is_finite() || condition_number > options.max_condition_number {
        return Err(format!("fit_condition_number_too_high|{condition_number}"));
    }
    let coefficients = svd
        .solve(&y, 1.0e-6)
        .map_err(|_| "fit_least_squares_failed".to_string())?;
    let matrix = Matrix3::new(
        coefficients[(0, 0)],
        coefficients[(1, 0)],
        coefficients[(2, 0)],
        coefficients[(0, 1)],
        coefficients[(1, 1)],
        coefficients[(2, 1)],
        coefficients[(0, 2)],
        coefficients[(1, 2)],
        coefficients[(2, 2)],
    );
    let matrix = matrix_to_array(&matrix);
    let offset = if options.fit_offset {
        [
            coefficients[(3, 0)],
            coefficients[(3, 1)],
            coefficients[(3, 2)],
        ]
    } else {
        [0.0; 3]
    };
    let errors = |rows: &Vec<([f32; 3], [f32; 3])>| {
        let mut channel_sum = [0.0_f32; 3];
        let mut count = 0usize;
        let mut max_error = 0.0_f32;
        for (input, target) in rows {
            let output = apply_raw(matrix, offset, *input);
            for c in 0..3 {
                let error = output[c] - target[c];
                channel_sum[c] += error * error;
                max_error = max_error.max(error.abs());
            }
            count += 1;
        }
        let channel_rmse = if count == 0 {
            [f32::INFINITY; 3]
        } else {
            channel_sum.map(|v| (v / count as f32).sqrt())
        };
        let rmse = (channel_rmse.iter().map(|v| v * v).sum::<f32>() / 3.0).sqrt();
        (channel_rmse, rmse, max_error)
    };
    let (_train_channels, training_rmse, train_max) = errors(&train);
    let (validation_channels, validation_rmse, validation_max) = errors(&validation);
    if !validation_rmse.is_finite() || validation_rmse > options.max_validation_rmse {
        return Err(format!("fit_validation_error_too_high|{validation_rmse}"));
    }
    let model_for_positivity = CalibrationFitModel {
        model_type: FitModelType::CaptureSeparation3x3,
        source_domain: ReferenceDomain::CameraNativeTransmissionRgb,
        target_domain: ReferenceDomain::TransmissionRgb,
        pipeline_order: "capture_correction->capture_separation->log10".into(),
        matrix,
        offset,
        diagnostics: FitDiagnostics {
            rank,
            condition_number,
            training_patch_count: train.len(),
            validation_patch_count: validation.len(),
            rejected_patch_count: rejected,
            channel_rmse: validation_channels,
            training_rmse,
            validation_rmse,
            max_abs_error: train_max.max(validation_max),
        },
        measurement_digest: measurement_digest(measurements)?,
    };
    if options.fit_offset && !preserves_positive_unit_transmission(&model_for_positivity) {
        return Err("fit_offset_breaks_positive_transmission".into());
    }
    let channel_rmse = validation_channels;
    let mut model = model_for_positivity;
    model.diagnostics.channel_rmse = channel_rmse;
    Ok(model)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(matrix: [[f32; 3]; 3], changed: bool) -> CalibrationMeasurementSet {
        let inputs = [
            [0.18, 0.31, 0.62],
            [0.42, 0.21, 0.71],
            [0.73, 0.52, 0.27],
            [0.34, 0.81, 0.46],
            [0.67, 0.38, 0.59],
            [0.25, 0.64, 0.33],
            [0.55, 0.29, 0.76],
            [0.81, 0.44, 0.19],
        ];
        let patches = inputs
            .into_iter()
            .enumerate()
            .map(|(i, input)| {
                let mut reference = apply_raw(matrix, [0.0; 3], input);
                if changed && i == 0 {
                    reference[0] += 0.02;
                }
                CalibrationPatch {
                    patch_id: format!("p{i}"),
                    position: Some([i as f32, 0.0]),
                    capture_transmission: input,
                    reference_value: reference,
                    reference_domain: ReferenceDomain::TransmissionRgb,
                    validation: i >= 6,
                    saturated: false,
                    bad_pixel: false,
                    outlier: false,
                }
            })
            .collect();
        CalibrationMeasurementSet {
            dark_frame: "dark.raw".into(),
            open_frame: "open.raw".into(),
            flat_frame: None,
            target_frame: "target.raw".into(),
            patches,
            camera_model: "fixture-camera".into(),
            scanner_model: None,
            iso: Some(100.0),
            exposure: Some(1.0),
            light_source: "fixture-light".into(),
            resolution: Some(1000),
            crop_geometry: "fixture-geometry".into(),
            raw_decode_version: "raw-v1".into(),
            input_digest: if changed { "changed" } else { "original" }.into(),
            quality_mask_digest: "mask".into(),
        }
    }

    #[test]
    fn recovers_known_matrix_and_applies_pixels() {
        let expected = [[1.05, 0.02, 0.01], [0.01, 0.96, 0.03], [0.02, 0.01, 1.02]];
        let model =
            fit_capture_separation(&fixture(expected, false), FitOptions::default()).unwrap();
        for r in 0..3 {
            for c in 0..3 {
                assert!((model.matrix[r][c] - expected[r][c]).abs() < 1.0e-4);
            }
        }
        let output = apply_capture_separation(&model, [0.31, 0.48, 0.72]).unwrap();
        let expected_output = apply_raw(expected, [0.0; 3], [0.31, 0.48, 0.72]);
        assert!(output
            .iter()
            .zip(expected_output)
            .all(|(a, b)| (*a - b).abs() < 1.0e-4));
    }

    #[test]
    fn changed_reference_changes_fit_and_digest() {
        let matrix = [[1.0, 0.02, 0.0], [0.0, 1.0, 0.01], [0.01, 0.0, 1.0]];
        let a = fit_capture_separation(&fixture(matrix, false), FitOptions::default()).unwrap();
        let b = fit_capture_separation(&fixture(matrix, true), FitOptions::default()).unwrap();
        assert_ne!(a.measurement_digest, b.measurement_digest);
        assert_ne!(a.matrix, b.matrix);
    }

    #[test]
    fn changing_reference_value_changes_digest_even_when_input_digest_is_unchanged() {
        let matrix = [[1.0, 0.02, 0.0], [0.0, 1.0, 0.01], [0.01, 0.0, 1.0]];
        let mut changed = fixture(matrix, false);
        changed.patches[0].reference_value[1] += 0.01;
        let original =
            fit_capture_separation(&fixture(matrix, false), FitOptions::default()).unwrap();
        let modified = fit_capture_separation(&changed, FitOptions::default()).unwrap();
        assert_ne!(original.measurement_digest, modified.measurement_digest);
        assert_ne!(original.matrix, modified.matrix);
    }

    #[test]
    fn optional_offset_is_recovered_only_when_enabled() {
        let matrix = [[1.0, 0.02, 0.0], [0.0, 1.0, 0.01], [0.01, 0.0, 1.0]];
        let offset = [0.01, 0.015, 0.02];
        let mut set = fixture(matrix, false);
        for patch in &mut set.patches {
            patch.reference_value = apply_raw(matrix, offset, patch.capture_transmission);
        }
        let model = fit_capture_separation(
            &set,
            FitOptions {
                fit_offset: true,
                ..FitOptions::default()
            },
        )
        .unwrap();
        assert!(model
            .offset
            .iter()
            .zip(offset)
            .all(|(actual, expected)| (*actual - expected).abs() < 1.0e-4));
    }

    #[test]
    fn measurement_json_accepts_camel_case_and_legacy_snake_case_fields() {
        let value = serde_json::json!({
            "darkFrame": "dark.raw",
            "open_frame": "open.raw",
            "targetFrame": "target.raw",
            "patches": [{
                "patchId": "p0",
                "capture_transmission": [0.2, 0.3, 0.4],
                "referenceValue": [0.2, 0.3, 0.4],
                "reference_domain": "transmission_rgb"
            }],
            "cameraModel": "camera",
            "lightSource": "light",
            "cropGeometry": "crop",
            "rawDecodeVersion": "raw",
            "inputDigest": "digest",
            "qualityMaskDigest": "mask"
        });
        let set: CalibrationMeasurementSet = serde_json::from_value(value).unwrap();
        assert_eq!(set.dark_frame, "dark.raw");
        assert_eq!(set.open_frame, "open.raw");
        assert_eq!(set.patches[0].patch_id, "p0");
    }

    #[test]
    fn rejects_degenerate_missing_and_pcs_data() {
        let mut set = fixture([[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]], false);
        for patch in &mut set.patches {
            patch.capture_transmission = [0.4; 3];
        }
        assert!(fit_capture_separation(&set, FitOptions::default()).is_err());
        let mut set = fixture([[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]], false);
        set.patches.retain(|p| !p.validation);
        assert!(fit_capture_separation(&set, FitOptions::default()).is_err());
        let mut set = fixture([[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]], false);
        set.patches[0].reference_domain = ReferenceDomain::PcsXyzIccRgb;
        assert!(fit_capture_separation(&set, FitOptions::default()).is_err());
    }
}
