use base64::{engine::general_purpose, Engine as _};
use image::{ImageBuffer, Rgb};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::VecDeque;
use std::sync::RwLock;

use crate::raw_backend::QualityMask;

/// v1.1 keeps a compact display proxy plus an f32 ProPhoto Estimate proxy for the
/// active working set, so the cache is deliberately smaller than v1.0.
/// Exceeding this triggers physical drop of the oldest proxy data.
pub const MAX_PROXY_CACHE: usize = 2;
pub const CALIBRATION_PROFILE_SCHEMA_VERSION: u32 = 2;
pub const CALIBRATION_PROFILE_PAYLOAD_VERSION: u32 = 2;
pub const CAPTURE_CORRECTION_ALGORITHM_VERSION: &str = "cfa_dark_open_v2";
pub const CAPTURE_DEMOSAIC_ALGORITHM_VERSION: &str = "fixed_bilinear_bayer_oriented_v2";
pub const DENSITY_ANCHOR_ALGORITHM_VERSION: &str = "density_anchor_v2";

/// Rules the frame-wise display window is derived with. Bumped when a change
/// makes previously persisted endpoints wrong, so frames analysed by an older
/// release are re-derived once instead of rendering the retired result.
///
/// 3: the content window keeps each channel's own co-sited endpoints again
/// instead of collapsing them onto one shared scale, so endpoints derived by
/// rule 2 (shared origin and span, plus a per-channel span response) have to be
/// re-derived.
pub const DENSITY_WINDOW_RULE_VERSION: u32 = 3;

fn default_calibration_profile_payload_version() -> u32 {
    CALIBRATION_PROFILE_PAYLOAD_VERSION
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DataDomain {
    LegacyLinearSrgb,
    ProPhotoEstimate,
    RawMosaic,
    CorrectedCfa,
    #[serde(alias = "camera_native_rgb")]
    CameraNativeTransmissionRgb,
    #[serde(alias = "density_input_rgb")]
    RelativeTransmissionRgb,
    Density,
    PositiveProPhotoRgb,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CalibrationLevel {
    SmartAuto,
    CaptureCorrectedExperimental,
    CaptureCharacterized,
    DensityCalibrated,
    ScannerInputEstimate,
    ScannerInputCharacterized,
    #[serde(alias = "calibrated")]
    Calibrated,
    #[serde(alias = "spectral")]
    Spectral,
}

impl Default for CalibrationLevel {
    fn default() -> Self {
        Self::SmartAuto
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CalibrationReferenceKind {
    /// Retained only so a damaged/future reference row cannot prevent other
    /// Profiles from loading. The UI never creates this value.
    Unknown,
    DarkFrame,
    OpenGate,
    FlatField,
    TransmissionTarget,
    FilmBase,
    FullExposure,
    SpectralCapture,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CalibrationReference {
    pub reference_id: String,
    pub kind: CalibrationReferenceKind,
    pub file_path: String,
    pub file_name: String,
    #[serde(default)]
    pub file_size: u64,
    #[serde(default)]
    pub modified_at: Option<i64>,
    pub added_at: i64,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CalibrationCapability {
    CaptureCorrected,
    MeasuredDensity,
    SpectralCapture,
    FilmReconstruction,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CalibrationValidationStatus {
    NotValidated,
    Passed,
    Failed,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CalibrationPayloadIssuer {
    LegacyUnverified,
    BackendCalibrationSession,
}

impl Default for CalibrationPayloadIssuer {
    fn default() -> Self {
        Self::LegacyUnverified
    }
}

impl Default for CalibrationValidationStatus {
    fn default() -> Self {
        Self::NotValidated
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CalibrationReferenceSummary {
    pub reference_id: String,
    pub kind: CalibrationReferenceKind,
    /// Content digest produced by the calibration session. A path alone is
    /// never sufficient provenance for a verified Capture payload.
    pub content_digest: String,
    pub raw_metadata_digest: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CaptureCalibrationParameters {
    pub correction_algorithm: String,
    pub demosaic_algorithm: String,
    pub epsilon: f32,
    pub light_source_id: String,
    pub geometry_fingerprint: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct CalibrationQualityMaskSummary {
    pub total_samples: u64,
    pub valid_samples: u64,
    pub invalid_denominator: u64,
    pub negative_samples: u64,
    pub saturated_samples: u64,
    pub bad_pixels: u64,
    pub out_of_range: u64,
    /// CFA sample indices excluded by the verified Capture calibration.
    #[serde(default)]
    pub bad_pixel_indices: Vec<u32>,
    /// Digest of `mask_artifact`. Legacy payloads that contain only the old
    /// digest are deliberately not considered verified.
    #[serde(default, alias = "mask_digest")]
    pub mask_artifact_digest: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CalibrationQualityMaskArtifact {
    pub encoding: String,
    pub sample_count: u64,
    pub data_base64: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CalibrationValidRange {
    pub minimum_transmission: [f32; 3],
    pub maximum_transmission: [f32; 3],
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct CalibrationValidationReport {
    #[serde(default)]
    pub status: CalibrationValidationStatus,
    pub checked_at: Option<i64>,
    #[serde(default)]
    pub checks: Vec<String>,
    #[serde(default)]
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CalibrationProfilePayload {
    #[serde(default = "default_calibration_profile_payload_version")]
    pub payload_version: u32,
    #[serde(default)]
    pub issuer: CalibrationPayloadIssuer,
    #[serde(default)]
    pub calibration_session_id: Option<String>,
    #[serde(default)]
    pub payload_digest: String,
    #[serde(default)]
    pub hardware_fingerprint: String,
    #[serde(default)]
    pub raw_decode_version: Option<i64>,
    #[serde(default)]
    pub libraw_version: String,
    #[serde(default)]
    pub reference_frames: Vec<CalibrationReferenceSummary>,
    #[serde(default)]
    pub capture_parameters: Option<CaptureCalibrationParameters>,
    #[serde(default)]
    pub quality_mask: Option<CalibrationQualityMaskSummary>,
    #[serde(default)]
    pub mask_artifact: Option<CalibrationQualityMaskArtifact>,
    #[serde(default)]
    pub valid_range: Option<CalibrationValidRange>,
    #[serde(default)]
    pub capabilities: Vec<CalibrationCapability>,
    #[serde(default)]
    pub validation_report: Option<CalibrationValidationReport>,
    /// Optional user-measurement-backed pre-log Capture Separation fit.
    /// Absence means the profile only provides capture normalization.
    #[serde(default)]
    pub fit_model: Option<crate::calibration_fit::CalibrationFitModel>,
    /// Normalized source measurements used to produce `fit_model`. Keeping the
    /// artifact with the coefficients makes the fit auditable and lets the
    /// loader reject a model whose measurement digest no longer matches.
    #[serde(default)]
    pub fit_measurements: Option<crate::calibration_fit::CalibrationMeasurementSet>,
}

impl Default for CalibrationProfilePayload {
    fn default() -> Self {
        Self {
            payload_version: CALIBRATION_PROFILE_PAYLOAD_VERSION,
            issuer: CalibrationPayloadIssuer::LegacyUnverified,
            calibration_session_id: None,
            payload_digest: String::new(),
            hardware_fingerprint: String::new(),
            raw_decode_version: None,
            libraw_version: String::new(),
            reference_frames: Vec::new(),
            capture_parameters: None,
            quality_mask: None,
            mask_artifact: None,
            valid_range: None,
            capabilities: Vec::new(),
            validation_report: None,
            fit_model: None,
            fit_measurements: None,
        }
    }
}

impl CalibrationProfilePayload {
    pub fn canonical_digest(&self) -> Result<String, String> {
        let mut canonical = self.clone();
        canonical.payload_digest.clear();
        let bytes = serde_json::to_vec(&canonical)
            .map_err(|error| format!("Failed to serialize calibration payload: {error}"))?;
        Ok(format!("{:x}", Sha256::digest(bytes)))
    }

    /// Validate the independently usable dark/open Capture payload. A
    /// Characterized fit is checked separately so a bad fit can downgrade to
    /// Capture Corrected without discarding the verified capture correction.
    pub fn capture_validation_error(
        &self,
        current_raw_decode_version: i64,
    ) -> Option<&'static str> {
        if self.payload_version != CALIBRATION_PROFILE_PAYLOAD_VERSION {
            return Some("capture_payload_version_unsupported");
        }
        if self.issuer != CalibrationPayloadIssuer::BackendCalibrationSession
            || self
                .calibration_session_id
                .as_deref()
                .is_none_or(str::is_empty)
            || self.payload_digest.trim().is_empty()
        {
            return Some("capture_payload_not_backend_issued");
        }
        if self.canonical_digest().ok().as_deref() != Some(self.payload_digest.as_str()) {
            return Some("capture_payload_digest_mismatch");
        }
        if !self
            .capabilities
            .contains(&CalibrationCapability::CaptureCorrected)
        {
            return Some("capture_capability_missing");
        }
        if self.validation_report.as_ref().map(|report| report.status)
            != Some(CalibrationValidationStatus::Passed)
        {
            return Some("capture_validation_not_passed");
        }
        if self.hardware_fingerprint.trim().is_empty() {
            return Some("capture_hardware_fingerprint_missing");
        }
        if self.libraw_version.trim().is_empty() {
            return Some("capture_libraw_version_missing");
        }
        if self.raw_decode_version != Some(current_raw_decode_version) {
            return Some("capture_raw_decode_version_mismatch");
        }
        let Some(parameters) = &self.capture_parameters else {
            return Some("capture_parameters_missing");
        };
        if parameters.correction_algorithm != CAPTURE_CORRECTION_ALGORITHM_VERSION
            || parameters.demosaic_algorithm != CAPTURE_DEMOSAIC_ALGORITHM_VERSION
            || !parameters.epsilon.is_finite()
            || (parameters.epsilon - 1.0e-6).abs() > f32::EPSILON
            || parameters.light_source_id.trim().is_empty()
            || parameters.geometry_fingerprint.trim().is_empty()
        {
            return Some("capture_parameters_invalid");
        }
        let required_reference = |kind| {
            self.reference_frames.iter().any(|reference| {
                reference.kind == kind
                    && !reference.content_digest.trim().is_empty()
                    && !reference.raw_metadata_digest.trim().is_empty()
            })
        };
        if !required_reference(CalibrationReferenceKind::DarkFrame)
            || !required_reference(CalibrationReferenceKind::OpenGate)
        {
            return Some("capture_reference_summary_incomplete");
        }
        let Some(quality) = &self.quality_mask else {
            return Some("capture_quality_mask_missing");
        };
        if quality.total_samples == 0
            || quality.valid_samples == 0
            || quality.valid_samples > quality.total_samples
            || quality.mask_artifact_digest.trim().is_empty()
            || quality
                .bad_pixel_indices
                .iter()
                .any(|index| u64::from(*index) >= quality.total_samples)
        {
            return Some("capture_quality_mask_invalid");
        }
        let Some(mask) = &self.mask_artifact else {
            return Some("capture_quality_mask_artifact_missing");
        };
        if mask.encoding != "invalid_bitset_le_v1"
            || mask.sample_count != quality.total_samples
            || mask.data_base64.trim().is_empty()
        {
            return Some("capture_quality_mask_artifact_invalid");
        }
        let Ok(mask_bytes) = general_purpose::STANDARD.decode(&mask.data_base64) else {
            return Some("capture_quality_mask_artifact_invalid");
        };
        if mask_bytes.len() != quality.total_samples.div_ceil(8) as usize
            || format!("{:x}", Sha256::digest(&mask_bytes)) != quality.mask_artifact_digest
        {
            return Some("capture_quality_mask_digest_mismatch");
        }
        let Some(range) = &self.valid_range else {
            return Some("capture_valid_range_missing");
        };
        if range
            .minimum_transmission
            .iter()
            .zip(range.maximum_transmission)
            .any(|(minimum, maximum)| {
                !minimum.is_finite()
                    || !maximum.is_finite()
                    || *minimum <= 0.0
                    || maximum <= *minimum
            })
        {
            return Some("capture_valid_range_invalid");
        }
        None
    }

    pub fn capture_is_verified(&self, current_raw_decode_version: i64) -> bool {
        self.capture_validation_error(current_raw_decode_version)
            .is_none()
    }

    pub fn fit_validation_error(&self) -> Option<&'static str> {
        let Some(model) = &self.fit_model else {
            return None;
        };
        let Some(measurements) = &self.fit_measurements else {
            return Some("capture_fit_measurements_missing");
        };
        if measurements.validate().is_err()
            || crate::calibration_fit::measurement_digest(measurements)
                .ok()
                .as_deref()
                != Some(model.measurement_digest.as_str())
        {
            return Some("capture_fit_measurements_invalid");
        }
        if model.source_domain
            != crate::calibration_fit::ReferenceDomain::CameraNativeTransmissionRgb
            || model.target_domain != crate::calibration_fit::ReferenceDomain::TransmissionRgb
            || model.pipeline_order != "capture_correction->capture_separation->log10"
            || model.measurement_digest.trim().is_empty()
            || model.matrix.iter().flatten().any(|v| !v.is_finite())
            || model.offset.iter().any(|v| !v.is_finite())
            || model.diagnostics.rank < 3
            || !model.diagnostics.condition_number.is_finite()
            || model.diagnostics.condition_number > 1.0e5
            || model.diagnostics.training_patch_count < 4
            || model.diagnostics.validation_patch_count == 0
            || !model.diagnostics.training_rmse.is_finite()
            || !model.diagnostics.validation_rmse.is_finite()
            || model.diagnostics.validation_rmse > 0.08
            || model
                .diagnostics
                .channel_rmse
                .iter()
                .any(|v| !v.is_finite())
            || !crate::calibration_fit::preserves_positive_unit_transmission(model)
        {
            return Some("capture_fit_model_invalid");
        }
        None
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CalibrationConfigProfile {
    pub profile_id: String,
    pub schema_version: u32,
    pub name: String,
    pub created_at: i64,
    pub updated_at: i64,
    #[serde(default)]
    pub camera: String,
    #[serde(default)]
    pub light_source: String,
    #[serde(default)]
    pub lens: String,
    #[serde(default)]
    pub calibration_level: CalibrationLevel,
    #[serde(default)]
    pub notes: String,
    #[serde(default)]
    pub references: Vec<CalibrationReference>,
    #[serde(default)]
    pub payload: CalibrationProfilePayload,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CalibrationProfileAvailability {
    Available,
    NeedsAttention,
    Unsupported,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CalibrationProfileView {
    pub profile: CalibrationConfigProfile,
    pub availability: CalibrationProfileAvailability,
    #[serde(default)]
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RollCalibrationFormat {
    Film135,
    Film120,
    Loose,
    Other,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RollFrameStatus {
    Set,
    NotSet,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RollBaseStatus {
    Sampled,
    Estimated,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RollDmaxStatus {
    FullExposure,
    FilmProfile,
    Unknown,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RollCalibrationMode {
    Configured,
    SmartAuto,
    Legacy,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RollToneStatus {
    Preserve,
    FullTone,
    Mixed,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RollCalibrationStatus {
    pub roll_id: String,
    pub format: RollCalibrationFormat,
    pub requested_profile_id: Option<String>,
    pub resolved_profile_id: Option<String>,
    pub profile_name: Option<String>,
    pub fallback_to_smart_auto: bool,
    pub frame: RollFrameStatus,
    pub base: RollBaseStatus,
    pub dmax: RollDmaxStatus,
    pub calibration: RollCalibrationMode,
    pub tone: RollToneStatus,
    #[serde(default)]
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ProcessingContract {
    /// Exact v1.0.2 compatibility path: linear-sRGB transport, gamut
    /// compression, u16 proxy, and the historical density matrix.
    LegacyV1,
    /// v1.1 display estimate in linear ProPhoto RGB. This path deliberately
    /// does not claim Status M or measured density calibration.
    SmartAutoProPhotoV11,
    /// v1.1 ProPhoto estimate with a roll-level film-base reference.
    RollBaseProPhotoV11,
    /// v1.1 ProPhoto estimate with roll-level film-base and full-exposure
    /// references. These anchors define the fixed roll density coordinate and
    /// display mapping; frame content never changes the endpoint mapping.
    RollAnchoredProPhotoV11,
    /// Capture-domain dark/open correction followed by fixed demosaic and an
    /// identity transform to relative transmission. This is not measured
    /// density and does not imply Status M validation.
    #[serde(alias = "measured_v11")]
    CaptureCorrectedV11,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PipelineStageStatus {
    Used,
    Default,
    Unavailable,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PipelineStageRecord {
    pub stage: String,
    pub input_domain: DataDomain,
    pub output_domain: DataDomain,
    pub status: PipelineStageStatus,
    pub detail: String,
}

/// Working primaries the decoded pixels are expressed in. The input class may
/// choose this value; it never chooses the density maths that consumes it.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum InputPrimaries {
    Srgb,
    DisplayP3,
    AdobeRgb1998,
    Rec2020,
    ProPhotoRgb,
    AcesCg,
    /// Three-channel camera data that has not been mapped to a known space.
    CameraNative,
    /// Scanner RGB described by the device identification or a bound Profile.
    ScannerDevice,
    Unknown,
}

/// Encoding curve of the decoded samples, before they are linearised.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum InputTransferCurve {
    Linear,
    Srgb,
    Gamma18,
    Gamma20,
    Gamma22,
    /// Curve recorded by the scanner container or Input Profile.
    ScannerDevice,
    /// Raw sensor samples; the RAW decoder owns the transfer function.
    CameraRaw,
    Unknown,
}

/// Whether the samples are proportional to film transmission or already
/// rendered for display by the capture device.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum InputReference {
    LinearTransmission,
    DisplayReferred,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum InputDomainSource {
    EmbeddedIcc,
    ScannerInputProfile,
    ScannerContainerRecord,
    RawMetadata,
    DeviceIdentification,
    /// No declaration was available; the file was read as sRGB.
    FallbackSrgb,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum InputDomainConfidence {
    /// The file states the domain in a form we can act on (ICC, container record).
    Verified,
    /// The domain follows from documented device behaviour rather than the file.
    Declared,
    /// The domain had to be assumed.
    Estimated,
}

/// Explicit, per-frame record of the input domain. Every input class resolves
/// one of these before the shared density stage, so a suffix, an import mode or
/// a wrong provenance guess can only change this record and the conversion that
/// produced it - never the density maths, neutralisation or display mapping.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct InputDomainRecord {
    pub primaries: InputPrimaries,
    pub transfer: InputTransferCurve,
    pub reference: InputReference,
    /// Black/white normalisation contract the samples were decoded with.
    pub normalization: String,
    pub source: InputDomainSource,
    pub confidence: InputDomainConfidence,
    /// True when the domain had to be assumed rather than read from the file.
    pub estimated: bool,
    #[serde(default)]
    pub detail: String,
}

impl Default for InputDomainRecord {
    fn default() -> Self {
        // Priority 5 of the input-domain order: fall back to sRGB and say so.
        Self {
            primaries: InputPrimaries::Srgb,
            transfer: InputTransferCurve::Srgb,
            reference: InputReference::DisplayReferred,
            normalization: "full_range_unit".to_string(),
            source: InputDomainSource::FallbackSrgb,
            confidence: InputDomainConfidence::Estimated,
            estimated: true,
            detail: String::new(),
        }
    }
}

impl InputDomainRecord {
    pub fn label(&self) -> String {
        format!(
            "{:?}/{:?}/{:?}/{:?}",
            self.primaries, self.transfer, self.reference, self.source
        )
    }

    pub fn is_estimated(&self) -> bool {
        self.estimated || self.confidence == InputDomainConfidence::Estimated
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct PipelineProcessingReport {
    #[serde(default)]
    pub stages: Vec<PipelineStageRecord>,
    #[serde(default)]
    pub fallback_reasons: Vec<String>,
    #[serde(default)]
    pub base_source: String,
    #[serde(default)]
    pub base_confidence: String,
    #[serde(default)]
    pub excluded_open_light_pixels: usize,
    #[serde(default)]
    pub excluded_saturated_pixels: usize,
    #[serde(default)]
    pub excluded_invalid_pixels: usize,
    #[serde(default)]
    pub tone_mapping_mode: String,
    #[serde(default)]
    pub uses_physical_anchors: bool,
    #[serde(default)]
    pub analysis_data_domain: String,
    #[serde(default)]
    pub render_route: String,
    #[serde(default)]
    pub fallback_reason: String,
    /// Input-domain resolution for this frame's pixels. Persisted with the
    /// frame and surfaced in the technical report.
    #[serde(default)]
    pub input_domain: InputDomainRecord,
    /// Version of the display-window rules this frame's endpoints were derived
    /// with. A frame analysed by an older release holds a window that was built
    /// with the retired rules (per-channel content offsets), so its endpoints
    /// have to be derived once more instead of rendering the old cast.
    /// Zero means "older than any recorded rule", which is what a frame
    /// persisted before this field existed deserializes to.
    #[serde(default)]
    pub analysis_window_rule: u32,
}

impl PipelineProcessingReport {
    pub fn smart_auto() -> Self {
        Self {
            stages: vec![PipelineStageRecord {
                stage: "raw_decode".to_string(),
                input_domain: DataDomain::RawMosaic,
                output_domain: DataDomain::ProPhotoEstimate,
                status: PipelineStageStatus::Used,
                detail: "libraw_dcraw_process_camera_wb_to_prophoto_estimate".to_string(),
            }],
            fallback_reasons: Vec::new(),
            base_source: "unresolved".to_string(),
            base_confidence: "low".to_string(),
            excluded_open_light_pixels: 0,
            excluded_saturated_pixels: 0,
            excluded_invalid_pixels: 0,
            tone_mapping_mode: "preserve_tone".to_string(),
            uses_physical_anchors: false,
            analysis_data_domain: "linear_prophoto_estimate".to_string(),
            render_route: "FilmAreaSmartAuto".to_string(),
            fallback_reason: String::new(),
            input_domain: InputDomainRecord::default(),
            analysis_window_rule: DENSITY_WINDOW_RULE_VERSION,
        }
    }

    pub fn capture_corrected(flat_used: bool) -> Self {
        Self {
            stages: vec![
                PipelineStageRecord {
                    stage: "raw_unpack".to_string(),
                    input_domain: DataDomain::RawMosaic,
                    output_domain: DataDomain::RawMosaic,
                    status: PipelineStageStatus::Used,
                    detail: "libraw_unpack_without_dcraw_process".to_string(),
                },
                PipelineStageRecord {
                    stage: "capture_correction".to_string(),
                    input_domain: DataDomain::RawMosaic,
                    output_domain: DataDomain::CorrectedCfa,
                    status: PipelineStageStatus::Used,
                    detail: CAPTURE_CORRECTION_ALGORITHM_VERSION.to_string(),
                },
                PipelineStageRecord {
                    stage: "fixed_demosaic".to_string(),
                    input_domain: DataDomain::CorrectedCfa,
                    output_domain: DataDomain::CameraNativeTransmissionRgb,
                    status: PipelineStageStatus::Used,
                    detail: CAPTURE_DEMOSAIC_ALGORITHM_VERSION.to_string(),
                },
                PipelineStageRecord {
                    stage: "capture_separation".to_string(),
                    input_domain: DataDomain::CameraNativeTransmissionRgb,
                    output_domain: DataDomain::RelativeTransmissionRgb,
                    status: PipelineStageStatus::Default,
                    detail: "identity_relative_transmission_no_density_claim".to_string(),
                },
                PipelineStageRecord {
                    stage: "flat_field".to_string(),
                    input_domain: DataDomain::CorrectedCfa,
                    output_domain: DataDomain::CorrectedCfa,
                    status: if flat_used {
                        PipelineStageStatus::Default
                    } else {
                        PipelineStageStatus::Unavailable
                    },
                    detail: if flat_used {
                        "stored_not_applied_pending_independent_definition".to_string()
                    } else {
                        "optional_flat_not_available".to_string()
                    },
                },
            ],
            ..Self::smart_auto()
        }
    }

    pub fn smart_auto_fallback(reason: impl Into<String>) -> Self {
        let mut report = Self::smart_auto();
        report.fallback_reasons.push(reason.into());
        report
    }
}

impl Default for ProcessingContract {
    fn default() -> Self {
        Self::LegacyV1
    }
}

impl ProcessingContract {
    pub fn input_domain(self) -> DataDomain {
        match self {
            Self::LegacyV1 => DataDomain::LegacyLinearSrgb,
            Self::CaptureCorrectedV11 => DataDomain::RelativeTransmissionRgb,
            Self::SmartAutoProPhotoV11
            | Self::RollBaseProPhotoV11
            | Self::RollAnchoredProPhotoV11 => DataDomain::ProPhotoEstimate,
        }
    }

    pub fn backend_label(self) -> &'static str {
        match self {
            Self::LegacyV1 => "LegacyV1",
            Self::CaptureCorrectedV11 => {
                "Capture Corrected / Relative Transmission RGB (experimental)"
            }
            Self::SmartAutoProPhotoV11
            | Self::RollBaseProPhotoV11
            | Self::RollAnchoredProPhotoV11 => "Smart Auto / ProPhoto Estimate",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DensityAnchorSource {
    SampledFilmBase,
    SampledFullExposure,
    EstimatedFromContent,
    LegacyEstimate,
    MeasuredProfile,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DensityAnchorScope {
    Frame,
    Roll,
    Profile,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DensityAnchorConfidence {
    Estimated,
    UserSampled,
    Verified,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DensityAnchorProvenance {
    pub input_domain: DataDomain,
    #[serde(default)]
    pub calibration_profile_id: Option<String>,
    #[serde(default)]
    pub calibration_payload_digest: Option<String>,
    #[serde(default)]
    pub raw_decode_version: Option<i64>,
    pub algorithm_version: String,
    #[serde(default)]
    pub legacy: bool,
}

impl Default for DensityAnchorProvenance {
    fn default() -> Self {
        Self {
            input_domain: DataDomain::ProPhotoEstimate,
            calibration_profile_id: None,
            calibration_payload_digest: None,
            raw_decode_version: None,
            algorithm_version: "legacy_unknown".to_string(),
            legacy: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DensityAnchor {
    /// Raw channel density before film-base subtraction.
    pub density: [f32; 3],
    pub source: DensityAnchorSource,
    pub scope: DensityAnchorScope,
    pub confidence: DensityAnchorConfidence,
    #[serde(default)]
    pub reference_id: Option<String>,
    /// Missing provenance in legacy JSON deserializes to an explicitly
    /// unverified ProPhoto Estimate record. It is retained for history but can
    /// never be promoted by deserialization.
    #[serde(default)]
    pub provenance: DensityAnchorProvenance,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct DensityAnchors {
    #[serde(default)]
    pub d_min_base: Option<DensityAnchor>,
    #[serde(default)]
    pub d_max_full_exposure: Option<DensityAnchor>,
    /// Replaced or domain-incompatible records retained for provenance and
    /// audit. Resolution never promotes these back into active endpoints.
    #[serde(default)]
    pub retained_records: Vec<DensityAnchor>,
    /// Fraction of the `base -> fully exposed leader` span that this Roll's own
    /// photographs actually reach. A leader is the film's maximum density and
    /// always sits above the scene highlights, so the display white point has
    /// to be placed from content rather than from the leader alone. One value
    /// per Roll keeps every frame on the same fixed mapping.
    #[serde(default)]
    pub highlight_fraction: Option<f32>,
}

impl DensityAnchors {
    pub fn has_base(&self) -> bool {
        self.d_min_base.is_some()
    }

    pub fn has_roll_base(&self) -> bool {
        self.d_min_base
            .as_ref()
            .is_some_and(|anchor| anchor.scope == DensityAnchorScope::Roll)
    }

    pub fn has_roll_full_exposure(&self) -> bool {
        self.d_max_full_exposure
            .as_ref()
            .is_some_and(|anchor| anchor.scope == DensityAnchorScope::Roll)
    }

    pub fn is_fully_anchored(&self) -> bool {
        self.has_roll_base() && self.has_roll_full_exposure()
    }

    pub fn prophoto_contract(&self) -> ProcessingContract {
        if self.is_fully_anchored() {
            ProcessingContract::RollAnchoredProPhotoV11
        } else if self.has_roll_base() {
            ProcessingContract::RollBaseProPhotoV11
        } else {
            ProcessingContract::SmartAutoProPhotoV11
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ContentRangeScope {
    FilmArea,
    FullFrame,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ContentRange {
    pub low: [f32; 3],
    pub high: [f32; 3],
    pub source_scope: ContentRangeScope,
    /// A stable algorithm identifier, not a user-facing label.
    pub percentile_method: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RenderMode {
    PreserveTone,
    FullTone,
    /// Fixed roll-level mapping derived only from valid D-min/D-max anchors.
    RollAnchored,
}

impl Default for RenderMode {
    fn default() -> Self {
        Self::PreserveTone
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RenderMapping {
    pub mode: RenderMode,
    pub density_low: [f32; 3],
    pub density_high: [f32; 3],
    pub exposure: f32,
    pub gamma: f32,
    pub channel_offsets: [f32; 3],
}

impl Default for RenderMapping {
    fn default() -> Self {
        Self {
            mode: RenderMode::PreserveTone,
            density_low: [0.1; 3],
            density_high: [2.0; 3],
            exposure: 0.0,
            gamma: 1.0,
            channel_offsets: [0.0; 3],
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PipelineState {
    #[serde(default)]
    pub contract: ProcessingContract,
    #[serde(default)]
    pub density_anchors: DensityAnchors,
    #[serde(default)]
    pub content_range: Option<ContentRange>,
    #[serde(default)]
    pub render_mapping: RenderMapping,
    #[serde(default)]
    pub processing_report: PipelineProcessingReport,
}

impl Default for PipelineState {
    fn default() -> Self {
        Self {
            contract: ProcessingContract::LegacyV1,
            density_anchors: DensityAnchors::default(),
            content_range: None,
            render_mapping: RenderMapping::default(),
            processing_report: PipelineProcessingReport::default(),
        }
    }
}

impl PipelineState {
    pub fn smart_auto() -> Self {
        Self {
            contract: ProcessingContract::SmartAutoProPhotoV11,
            processing_report: PipelineProcessingReport::smart_auto(),
            ..Self::default()
        }
    }

    pub fn from_roll_anchors(anchors: DensityAnchors) -> Self {
        let mut report = PipelineProcessingReport::smart_auto();
        let prophoto_estimate = anchors
            .d_min_base
            .as_ref()
            .is_some_and(|anchor| anchor.provenance.input_domain == DataDomain::ProPhotoEstimate);
        report.base_source = if anchors.has_roll_base() {
            if prophoto_estimate {
                "roll_anchor_prophoto_estimate".to_string()
            } else {
                "roll_anchor_relative_transmission".to_string()
            }
        } else {
            "unresolved".to_string()
        };
        report.base_confidence = if anchors.has_roll_base() {
            if prophoto_estimate {
                "estimated".to_string()
            } else {
                "user_sampled".to_string()
            }
        } else {
            "low".to_string()
        };
        report.uses_physical_anchors = anchors.has_roll_base() && !prophoto_estimate;
        Self {
            contract: anchors.prophoto_contract(),
            density_anchors: anchors,
            processing_report: report,
            ..Self::default()
        }
    }

    pub fn capture_corrected(anchors: DensityAnchors, flat_used: bool) -> Self {
        Self {
            contract: ProcessingContract::CaptureCorrectedV11,
            density_anchors: anchors,
            processing_report: PipelineProcessingReport::capture_corrected(flat_used),
            ..Self::default()
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum FilmMode {
    Color,
    BW,
}

impl Default for FilmMode {
    fn default() -> Self {
        FilmMode::Color
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct DensityParams {
    pub d_min: [f32; 3],
    pub d_max: [f32; 3],
    /// Manual Master D-Min/D-Max trim from the Develop panel. The value is a
    /// scalar shift applied to the endpoints the active route derived, so a
    /// frame can be adjusted without rewriting the Roll's density anchors.
    #[serde(default)]
    pub d_min_offset: f32,
    #[serde(default)]
    pub d_max_offset: f32,
    pub gamma: f32,
}

impl Default for DensityParams {
    fn default() -> Self {
        Self {
            d_min: [0.1, 0.1, 0.1],
            d_max: [2.0, 2.0, 2.0],
            d_min_offset: 0.0,
            d_max_offset: 0.0,
            gamma: 1.0,
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct ExposureParams {
    pub exposure: f32,
    pub exp_r: f32,
    pub exp_g: f32,
    pub exp_b: f32,
}

impl Default for ExposureParams {
    fn default() -> Self {
        Self {
            exposure: 0.0,
            exp_r: 0.0,
            exp_g: 0.0,
            exp_b: 0.0,
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct ToneParams {
    pub highlights: f32,
    pub shadows: f32,
    #[serde(default)]
    pub contrast: f32,
    #[serde(default)]
    pub saturation: f32,
    #[serde(default)]
    pub temperature: f32,
    #[serde(default)]
    #[serde(alias = "hue")]
    pub tint: f32,
}

impl Default for ToneParams {
    fn default() -> Self {
        Self {
            highlights: 0.0,
            shadows: 0.0,
            contrast: 0.0,
            saturation: 0.0,
            temperature: 0.0,
            tint: 0.0,
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct SprocketParams {
    pub sprocket_uv: Option<Vec<f32>>,
    pub sprocket_tolerance: Option<f32>,
    pub sprocket_feather: Option<f32>,
    #[serde(default)]
    pub sprocket_target_color: Option<Vec<f32>>,
}

impl Default for SprocketParams {
    fn default() -> Self {
        Self {
            sprocket_uv: None,
            sprocket_tolerance: None,
            sprocket_feather: None,
            sprocket_target_color: None,
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct LutParams {
    #[serde(default)]
    pub lut_path: Option<String>,
    #[serde(default = "default_lut_opacity")]
    pub lut_opacity: f32,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct RawDecodeParams {
    #[serde(default = "default_working_colorspace")]
    pub working_colorspace: String,
}

fn default_working_colorspace() -> String {
    "linear-srgb".to_string()
}

impl Default for RawDecodeParams {
    fn default() -> Self {
        Self {
            working_colorspace: default_working_colorspace(),
        }
    }
}

fn default_lut_opacity() -> f32 {
    1.0
}

impl Default for LutParams {
    fn default() -> Self {
        Self {
            lut_path: None,
            lut_opacity: 1.0,
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct TuningParams {
    pub film_mode: FilmMode,
    #[serde(flatten)]
    pub density: DensityParams,
    #[serde(flatten)]
    pub exposure: ExposureParams,
    #[serde(flatten)]
    pub tone: ToneParams,
    #[serde(flatten)]
    pub sprocket: SprocketParams,
    #[serde(flatten)]
    pub lut: LutParams,
    #[serde(flatten)]
    pub raw_decode: RawDecodeParams,
}

impl Default for TuningParams {
    fn default() -> Self {
        Self {
            film_mode: FilmMode::Color,
            density: DensityParams::default(),
            exposure: ExposureParams::default(),
            tone: ToneParams::default(),
            sprocket: SprocketParams::default(),
            lut: LutParams::default(),
            raw_decode: RawDecodeParams::default(),
        }
    }
}

#[cfg(test)]
mod tuning_params_tests {
    use super::TuningParams;

    #[test]
    fn legacy_tuning_json_defaults_new_post_gamma_controls() {
        let mut legacy = serde_json::to_value(TuningParams::default()).unwrap();
        let object = legacy.as_object_mut().unwrap();
        object.remove("contrast");
        object.remove("saturation");
        object.remove("temperature");
        object.remove("tint");

        let params: TuningParams = serde_json::from_value(legacy).unwrap();
        assert_eq!(params.tone.contrast, 0.0);
        assert_eq!(params.tone.saturation, 0.0);
        assert_eq!(params.tone.temperature, 0.0);
        assert_eq!(params.tone.tint, 0.0);
    }

    #[test]
    fn legacy_hue_field_loads_as_tint() {
        let mut legacy = serde_json::to_value(TuningParams::default()).unwrap();
        let object = legacy.as_object_mut().unwrap();
        object.remove("tint");
        object.insert("hue".to_string(), serde_json::json!(0.25));

        let params: TuningParams = serde_json::from_value(legacy).unwrap();
        assert_eq!(params.tone.tint, 0.25);
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BaseColor {
    pub base_r: u16,
    pub base_g: u16,
    pub base_b: u16,
}

impl Default for BaseColor {
    fn default() -> Self {
        Self {
            base_r: 32768,
            base_g: 32768,
            base_b: 32768,
        }
    }
}

pub struct FilmItem {
    pub id: String,
    pub roll_id: String,
    pub file_path: String,
    /// Import-stage preview. Develop rendering must never overwrite it.
    pub embedded_thumbnail_base64: String,
    /// Last rendered positive preview, if this frame has been developed.
    pub rendered_thumbnail_base64: Option<String>,
    pub original_proxy: Option<ImageBuffer<Rgb<u16>, Vec<u16>>>,
    pub proxy_image: Option<ImageBuffer<Rgb<u16>, Vec<u16>>>,
    /// Linear ProPhoto RGB estimate retained as f32 for Smart Auto contracts.
    /// It is not Camera Native Transmission RGB or measured density. LegacyV1
    /// leaves this empty and continues to use the u16 proxy above.
    pub prophoto_estimate_proxy: Option<ImageBuffer<Rgb<f32>, Vec<f32>>>,
    /// Capture-corrected relative transmission, distinct from ProPhoto
    /// Estimate. It is present only while the active contract is
    /// CaptureCorrectedV11.
    pub(crate) relative_transmission_proxy: Option<ImageBuffer<Rgb<f32>, Vec<f32>>>,
    /// Quality state for relative transmission. Invalid samples are never
    /// promoted into density math by epsilon substitution.
    pub(crate) relative_transmission_quality: Option<QualityMask>,
    pub pristine_proxy: Option<ImageBuffer<Rgb<f32>, Vec<f32>>>,
    pub base_color: BaseColor,
    /// Ephemeral result of the last capability resolution. The persisted
    /// `pipeline_state` keeps the user's request and all anchor records.
    pub runtime_pipeline_state: Option<PipelineState>,
    /// Provenance attached to frame anchors computed from the current input.
    pub runtime_density_provenance: Option<DensityAnchorProvenance>,
    /// Resolver cache key. Capability recovery invalidates fallback proxies.
    pub runtime_pipeline_key: Option<String>,
    /// Film base measured on this frame's own pixels. Scanner exposure and
    /// white balance drift from frame to frame, so roll anchors alone leave a
    /// colour cast after mask removal. `None` falls back to the roll anchor.
    pub runtime_frame_base: Option<[f32; 3]>,
    /// Fraction of the roll density span that this frame's brightest scene
    /// content occupies. A fully exposed leader is the film's maximum density,
    /// which sits above the scene highlights, so using the whole span pushes
    /// every photograph towards black.
    pub runtime_frame_highlight: Option<f32>,
    pub pipeline_state: PipelineState,
    pub params: TuningParams,
    pub geom: GeometryState,
    pub is_loose: bool,
    /// Ephemeral membership in the current Library/Develop working session.
    /// Persisted archive records always restore with this set to false.
    pub in_library: bool,
}

impl FilmItem {
    pub fn effective_pipeline_state(&self) -> &PipelineState {
        self.runtime_pipeline_state
            .as_ref()
            .unwrap_or(&self.pipeline_state)
    }

    pub fn preferred_thumbnail(&self) -> &str {
        self.rendered_thumbnail_base64
            .as_deref()
            .filter(|thumbnail| !thumbnail.is_empty())
            .unwrap_or(&self.embedded_thumbnail_base64)
    }

    pub fn thumbnail_kind(&self) -> &'static str {
        if self
            .rendered_thumbnail_base64
            .as_deref()
            .is_some_and(|thumbnail| !thumbnail.is_empty())
        {
            "rendered"
        } else {
            "embedded"
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GeometryState {
    pub crop_rect: CropRect,
    pub angle: f32,
    #[serde(default)]
    pub perspective_vertical: f32,
    #[serde(default)]
    pub perspective_horizontal: f32,
    #[serde(default)]
    pub perspective_aspect: f32,
    #[serde(default)]
    pub lens_distortion: f32,
    #[serde(default = "default_perspective_scale")]
    pub perspective_scale: f32,
    #[serde(default)]
    pub constrain_crop: bool,
    pub flip_h: bool,
    pub flip_v: bool,
    pub rotate_90_count: i32,
    #[serde(default)]
    pub calibration_points: Option<[[f32; 2]; 4]>,
    #[serde(default)]
    pub calibration_confirmed: bool,
}

impl Default for GeometryState {
    fn default() -> Self {
        GeometryState {
            crop_rect: CropRect::default(),
            angle: 0.0,
            perspective_vertical: 0.0,
            perspective_horizontal: 0.0,
            perspective_aspect: 0.0,
            lens_distortion: 0.0,
            perspective_scale: default_perspective_scale(),
            constrain_crop: false,
            flip_h: false,
            flip_v: false,
            rotate_90_count: 0,
            calibration_points: None,
            calibration_confirmed: false,
        }
    }
}

fn default_perspective_scale() -> f32 {
    1.0
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CropRect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl Default for CropRect {
    fn default() -> Self {
        CropRect {
            x: 0.0,
            y: 0.0,
            width: 1.0,
            height: 1.0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutoAlignResult {
    pub crop_rect: CropRect,
    pub angle: f32,
}

#[derive(Debug, Clone, Serialize)]
pub struct FilmstripItem {
    pub id: String,
    pub roll_id: String,
    pub file_path: String,
    /// Preferred thumbnail retained for compatibility with the current UI.
    pub thumbnail_base64: String,
    pub embedded_thumbnail_base64: String,
    pub rendered_thumbnail_base64: Option<String>,
    pub thumbnail_kind: String,
    pub base_analyzed: bool,
    pub state_available: bool,
    pub file_missing: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Roll {
    pub roll_id: String,
    pub date: String,
    pub format: String, // "135" or "120"
    pub film_stock: String,
    pub camera: String,
    /// Free-form note supplied at import time. Same film stock may be shot on
    /// several rolls, so this is the place to tell them apart.
    #[serde(default)]
    pub notes: String,
    pub image_paths: Vec<String>,
    #[serde(default)]
    pub density_anchors: DensityAnchors,
    /// `None` is the explicit Smart Auto choice. Existing rolls deserialize
    /// to it without being changed by the application's last-used selection.
    #[serde(default)]
    pub calibration_profile_id: Option<String>,
    /// Optional scanner input profile applied to scanner-originated RGB input.
    #[serde(default)]
    pub scanner_profile_id: Option<String>,
}

pub struct EngineState {
    pub items: dashmap::DashMap<String, std::sync::Arc<std::sync::RwLock<FilmItem>>>,
    /// Monotonic per-image edit epochs used to reject stale asynchronous writes.
    pub development_generations:
        dashmap::DashMap<String, std::sync::Arc<std::sync::atomic::AtomicU64>>,
    /// Cooperative cancellation flags for roll-level Auto Invert workers.
    pub roll_batch_cancellations:
        dashmap::DashMap<String, std::sync::Arc<std::sync::atomic::AtomicBool>>,
    pub film_border_cache: dashmap::DashMap<String, crate::film_border::FilmBorderDetection>,
    pub item_order: RwLock<Vec<String>>,
    pub active_id: RwLock<Option<String>>,
    pub rolls: RwLock<Vec<Roll>>,
    /// Serializes roll snapshot mutations across async commands and the import
    /// reconciliation worker without holding the synchronous roll RwLock over I/O.
    pub roll_mutation: tokio::sync::Mutex<()>,
    /// LRU order of images whose high-res proxy data is loaded in memory.
    /// Front = oldest, back = newest. Capacity enforced at MAX_PROXY_CACHE.
    pub proxy_loaded_order: RwLock<VecDeque<String>>,
}

impl EngineState {
    pub fn new() -> Self {
        EngineState {
            items: dashmap::DashMap::new(),
            development_generations: dashmap::DashMap::new(),
            roll_batch_cancellations: dashmap::DashMap::new(),
            film_border_cache: dashmap::DashMap::new(),
            item_order: RwLock::new(Vec::new()),
            active_id: RwLock::new(None),
            rolls: RwLock::new(Vec::new()),
            roll_mutation: tokio::sync::Mutex::new(()),
            proxy_loaded_order: RwLock::new(VecDeque::new()),
        }
    }
}
