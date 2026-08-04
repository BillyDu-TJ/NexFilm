use image::{ImageBuffer, Rgb};
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::sync::RwLock;

/// Hard limit: at most 4 high-res proxy images kept in memory.
/// Exceeding this triggers physical drop of the oldest proxy data.
pub const MAX_PROXY_CACHE: usize = 4;

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
    pub pristine_proxy: Option<ImageBuffer<Rgb<f32>, Vec<f32>>>,
    pub base_color: BaseColor,
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
    pub state_available: bool,
    pub file_missing: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Roll {
    pub roll_id: String,
    pub date: String,
    pub format: String, // "135" or "120"
    pub film_stock: String,
    pub camera: String,
    pub image_paths: Vec<String>,
}

pub struct EngineState {
    pub items: dashmap::DashMap<String, std::sync::Arc<std::sync::RwLock<FilmItem>>>,
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
            film_border_cache: dashmap::DashMap::new(),
            item_order: RwLock::new(Vec::new()),
            active_id: RwLock::new(None),
            rolls: RwLock::new(Vec::new()),
            roll_mutation: tokio::sync::Mutex::new(()),
            proxy_loaded_order: RwLock::new(VecDeque::new()),
        }
    }
}
