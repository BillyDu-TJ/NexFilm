use image::{ImageBuffer, Rgb};
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::sync::RwLock;

/// v1.1 keeps a compact display proxy plus an f32 scientific proxy for the
/// active working set, so the cache is deliberately smaller than v1.0.
/// Exceeding this triggers physical drop of the oldest proxy data.
pub const MAX_PROXY_CACHE: usize = 2;
pub const CALIBRATION_PROFILE_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CalibrationLevel {
    SmartAuto,
    Calibrated,
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
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
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CalibrationProfileAvailability {
    Available,
    NeedsAttention,
    Unsupported,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
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
    /// references. Per-frame density analysis is unnecessary in this mode.
    RollAnchoredProPhotoV11,
    /// Reserved for a later measured workflow. Alpha never selects this
    /// contract automatically.
    MeasuredV11,
}

impl Default for ProcessingContract {
    fn default() -> Self {
        Self::LegacyV1
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DensityAnchor {
    /// Raw channel density before film-base subtraction.
    pub density: [f32; 3],
    pub source: DensityAnchorSource,
    pub scope: DensityAnchorScope,
    pub confidence: DensityAnchorConfidence,
    #[serde(default)]
    pub reference_id: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct DensityAnchors {
    #[serde(default)]
    pub d_min_base: Option<DensityAnchor>,
    #[serde(default)]
    pub d_max_full_exposure: Option<DensityAnchor>,
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
}

impl Default for PipelineState {
    fn default() -> Self {
        Self {
            contract: ProcessingContract::LegacyV1,
            density_anchors: DensityAnchors::default(),
            content_range: None,
            render_mapping: RenderMapping::default(),
        }
    }
}

impl PipelineState {
    pub fn smart_auto() -> Self {
        Self {
            contract: ProcessingContract::SmartAutoProPhotoV11,
            ..Self::default()
        }
    }

    pub fn from_roll_anchors(anchors: DensityAnchors) -> Self {
        Self {
            contract: anchors.prophoto_contract(),
            density_anchors: anchors,
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
    pub gamma: f32,
}

impl Default for DensityParams {
    fn default() -> Self {
        Self {
            d_min: [0.1, 0.1, 0.1],
            d_max: [2.0, 2.0, 2.0],
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
}

impl Default for SprocketParams {
    fn default() -> Self {
        Self {
            sprocket_uv: None,
            sprocket_tolerance: None,
            sprocket_feather: None,
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
        object.remove("saturation");
        object.remove("temperature");
        object.remove("tint");

        let params: TuningParams = serde_json::from_value(legacy).unwrap();
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
    /// Linear ProPhoto RGB transmission retained as f32 for v1.1 contracts.
    /// LegacyV1 leaves this empty and continues to use the u16 proxy above.
    pub scientific_proxy: Option<ImageBuffer<Rgb<f32>, Vec<f32>>>,
    pub pristine_proxy: Option<ImageBuffer<Rgb<f32>, Vec<f32>>>,
    pub base_color: BaseColor,
    pub pipeline_state: PipelineState,
    pub params: TuningParams,
    pub geom: GeometryState,
    pub is_loose: bool,
    /// Ephemeral membership in the current Library/Develop working session.
    /// Persisted archive records always restore with this set to false.
    pub in_library: bool,
}

impl FilmItem {
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
    pub image_paths: Vec<String>,
    #[serde(default)]
    pub density_anchors: DensityAnchors,
    /// `None` is the explicit Smart Auto choice. Existing rolls deserialize
    /// to it without being changed by the application's last-used selection.
    #[serde(default)]
    pub calibration_profile_id: Option<String>,
}

pub struct EngineState {
    pub items: dashmap::DashMap<String, std::sync::Arc<std::sync::RwLock<FilmItem>>>,
    /// Monotonic per-image edit epochs used to reject stale asynchronous writes.
    pub development_generations:
        dashmap::DashMap<String, std::sync::Arc<std::sync::atomic::AtomicU64>>,
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
            film_border_cache: dashmap::DashMap::new(),
            item_order: RwLock::new(Vec::new()),
            active_id: RwLock::new(None),
            rolls: RwLock::new(Vec::new()),
            roll_mutation: tokio::sync::Mutex::new(()),
            proxy_loaded_order: RwLock::new(VecDeque::new()),
        }
    }
}
