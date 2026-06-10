use image::{ImageBuffer, Rgb};
use serde::{Deserialize, Serialize};
use std::sync::RwLock;

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TuningParams {
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
    pub proxy_image: ImageBuffer<Rgb<u16>, Vec<u16>>,
    pub pristine_proxy: ImageBuffer<Rgb<f32>, Vec<f32>>,
    pub base_color: BaseColor,
    pub params: TuningParams,
}

#[derive(Debug, Clone, Serialize)]
pub struct FilmstripItem {
    pub id: String,
    pub file_path: String,
    pub thumbnail_base64: String,
}

pub struct EngineState {
    pub items: RwLock<Vec<FilmItem>>,
    pub active_id: RwLock<Option<String>>,
}

impl EngineState {
    pub fn new() -> Self {
        EngineState {
            items: RwLock::new(Vec::new()),
            active_id: RwLock::new(None),
        }
    }
}
