use image::{ImageBuffer, Rgb};
use serde::{Deserialize, Serialize};
use std::sync::RwLock;

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

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TuningParams {
    pub film_mode: FilmMode,
    pub d_min: f32,
    pub d_max: f32,
    pub exposure: f32,
    pub gamma: f32,
    pub exp_r: f32,
    pub exp_g: f32,
    pub exp_b: f32,
}

impl Default for TuningParams {
    fn default() -> Self {
        Self {
            film_mode: FilmMode::Color,
            d_min: 0.1,
            d_max: 2.0,
            exposure: 0.0,
            gamma: 1.0,
            exp_r: 0.0,
            exp_g: 0.0,
            exp_b: 0.0,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct BaseColor {
    pub base_r: u16,
    pub base_g: u16,
    pub base_b: u16,
}

pub struct FilmItem {
    pub id: String,
    pub file_path: String,
    pub thumbnail_base64: String,
    pub original_proxy: ImageBuffer<Rgb<u16>, Vec<u16>>,
    pub proxy_image: ImageBuffer<Rgb<u16>, Vec<u16>>,
    pub pristine_proxy: ImageBuffer<Rgb<f32>, Vec<f32>>,
    pub base_color: BaseColor,
    pub params: TuningParams,
    pub geom: GeometryState,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeometryState {
    pub crop_rect: CropRect,
    pub angle: f32,
    pub flip_h: bool,
    pub flip_v: bool,
    pub rotate_90_count: i32,
}

impl Default for GeometryState {
    fn default() -> Self {
        GeometryState {
            crop_rect: CropRect::default(),
            angle: 0.0,
            flip_h: false,
            flip_v: false,
            rotate_90_count: 0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CropRect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl Default for CropRect {
    fn default() -> Self {
        CropRect { x: 0.0, y: 0.0, width: 1.0, height: 1.0 }
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
    pub file_path: String,
    pub thumbnail_base64: String,
}

pub struct EngineState {
    pub items: dashmap::DashMap<String, std::sync::Arc<std::sync::RwLock<FilmItem>>>,
    pub item_order: RwLock<Vec<String>>,
    pub active_id: RwLock<Option<String>>,
}

impl EngineState {
    pub fn new() -> Self {
        EngineState {
            items: dashmap::DashMap::new(),
            item_order: RwLock::new(Vec::new()),
            active_id: RwLock::new(None),
        }
    }
}
