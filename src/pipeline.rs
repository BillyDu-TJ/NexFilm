use crate::core_math::{density_luma, status_m_crosstalk_matrix};
use nalgebra::{Matrix3, Vector3};

use crate::app_state::{BaseColor, FilmMode, PipelineState, ProcessingContract};

/// Film inversion pipeline. LegacyV1 retains the historical Status M estimate;
/// newer contracts operate in their declared input domain without claiming a
/// physical density calibration that the capture profile has not supplied.
/// 实现了无状态函数式调用，兼容 Rayon 跨线程高并发处理。
pub struct FilmPipeline {
    /// 核心去串扰矩阵 (Status M)
    crosstalk_matrix: Matrix3<f32>,
    /// 透射率 <= 0 时钳制为一个极小的正数 (Epsilon)
    epsilon: f32,
    /// 片基密度 (D_min) 的对数值，用于扣除橙色色罩
    base_density: Vector3<f32>,
    /// 密度域白平衡与曝光偏移补偿
    exposure_offset: Vector3<f32>,
    /// 色彩模式 (Color / B&W)
    mode: FilmMode,
    contract: ProcessingContract,
}

impl Default for FilmPipeline {
    fn default() -> Self {
        Self::new([65535, 65535, 65535], [0.0, 0.0, 0.0], FilmMode::Color)
    }
}

impl FilmPipeline {
    /// 完整构建 Pipeline
    pub fn new(base_rgb: [u16; 3], exp_offset: [f32; 3], mode: FilmMode) -> Self {
        let epsilon = 1e-6_f32;

        // 解析基础透射率 (T_base)
        let t_r = (base_rgb[0] as f32 / 65535.0).max(epsilon);
        let t_g = (base_rgb[1] as f32 / 65535.0).max(epsilon);
        let t_b = (base_rgb[2] as f32 / 65535.0).max(epsilon);

        let base_density = Vector3::new(-t_r.log10(), -t_g.log10(), -t_b.log10());

        Self {
            crosstalk_matrix: status_m_crosstalk_matrix(),
            epsilon,
            base_density,
            exposure_offset: Vector3::new(exp_offset[0], exp_offset[1], exp_offset[2]),
            mode,
            contract: ProcessingContract::LegacyV1,
        }
    }

    /// v1.1 non-Legacy path. Smart Auto receives a ProPhoto Estimate while
    /// Capture Corrected receives quality-checked relative transmission. Neither path adds
    /// the historical empirical Status M matrix.
    pub fn new_prophoto(
        base_density: [f32; 3],
        exp_offset: [f32; 3],
        mode: FilmMode,
        contract: ProcessingContract,
    ) -> Self {
        debug_assert!(contract != ProcessingContract::LegacyV1);
        Self {
            crosstalk_matrix: Matrix3::identity(),
            epsilon: 1e-6_f32,
            base_density: Vector3::new(base_density[0], base_density[1], base_density[2]),
            exposure_offset: Vector3::new(exp_offset[0], exp_offset[1], exp_offset[2]),
            mode,
            contract,
        }
    }

    pub fn from_state(
        state: &PipelineState,
        base_color: &BaseColor,
        exp_offset: [f32; 3],
        mode: FilmMode,
    ) -> Self {
        if state.contract == ProcessingContract::LegacyV1 {
            return Self::new(
                [base_color.base_r, base_color.base_g, base_color.base_b],
                exp_offset,
                mode,
            );
        }
        let base_density = state
            .density_anchors
            .d_min_base
            .as_ref()
            .map(|anchor| anchor.density)
            .unwrap_or_else(|| {
                // A loose or unanchored Smart Auto frame has no physical anchor
                // but does carry a per-frame film-base estimate, and that
                // estimate is its neutral reference. Anything without one keeps
                // the zero reference.
                frame_base_density(state, base_color).unwrap_or([0.0; 3])
            });
        Self::new_prophoto(base_density, exp_offset, mode, state.contract)
    }

    pub fn contract(&self) -> ProcessingContract {
        self.contract
    }

    /// 第一性原理线性处理管线 - Phase 3 白平衡与曝光偏移
    ///
    /// # Parameters
    /// * `linear_rgb` - 线性透射率数组 [R, G, B]
    ///
    /// 提取出物理染料浓度 (纯净密度图)
    #[inline]
    pub fn compute_true_density(&self, linear_rgb: &[f32; 3]) -> [f32; 3] {
        let t_r = linear_rgb[0].max(self.epsilon);
        let t_g = linear_rgb[1].max(self.epsilon);
        let t_b = linear_rgb[2].max(self.epsilon);

        let d_raw = Vector3::new(-t_r.log10(), -t_g.log10(), -t_b.log10());

        // Match the WebGL shader: base subtraction is allowed to produce
        // negative density and is normalized later by D-min/D-max.
        let delta_d = d_raw - self.base_density;

        match self.mode {
            FilmMode::Color => {
                if self.contract == ProcessingContract::LegacyV1 {
                    let true_density_vec = self.crosstalk_matrix * delta_d;
                    [true_density_vec.x, true_density_vec.y, true_density_vec.z]
                } else {
                    [delta_d.x, delta_d.y, delta_d.z]
                }
            }
            FilmMode::BW => {
                // Monochrome density is measured in the fixed linear-sRGB
                // capture domain with green-heavy luminance weighting.
                let gray_density = density_luma([delta_d.x, delta_d.y, delta_d.z]);
                [gray_density, gray_density, gray_density]
            }
        }
    }

    /// Consume a relative-transmission sample only when its capture-quality
    /// mask says it is valid. Unlike the compatibility method above, this
    /// entry point never turns zero, negative, NaN, or saturated/masked input
    /// into an apparently valid density using epsilon.
    #[inline]
    pub fn compute_relative_density(
        &self,
        relative_transmission_rgb: &[f32; 3],
        quality_valid: bool,
    ) -> Option<[f32; 3]> {
        if self.contract != ProcessingContract::CaptureCorrectedV11
            || !quality_valid
            || relative_transmission_rgb
                .iter()
                .any(|value| !value.is_finite() || *value <= 0.0)
        {
            return None;
        }
        Some(self.compute_true_density(relative_transmission_rgb))
    }

    /// 应用曝光偏移并防止负密度
    #[inline]
    pub fn apply_exposure(&self, true_density: &[f32; 3]) -> [f32; 3] {
        match self.mode {
            FilmMode::Color => {
                let final_r = true_density[0] + self.exposure_offset.x;
                let final_g = true_density[1] + self.exposure_offset.y;
                let final_b = true_density[2] + self.exposure_offset.z;
                [final_r, final_g, final_b]
            }
            FilmMode::BW => {
                // 黑白模式下旁路偏色设置，仅应用基础曝光补偿（取第一通道偏移量或者忽略偏色）
                let final_gray = true_density[0] + self.exposure_offset.x;
                [final_gray, final_gray, final_gray]
            }
        }
    }

    /// 一步执行完整管线
    #[inline]
    pub fn process_pixel(&self, linear_rgb: &[f32; 3]) -> [f32; 3] {
        let true_density = self.compute_true_density(linear_rgb);
        self.apply_exposure(&true_density)
    }
}

#[inline]
fn density_from_u16(value: u16) -> f32 {
    -(value as f32 / 65535.0).max(1e-6).log10()
}

/// Per-channel film-base density carried by a persisted `BaseColor` estimate.
pub(crate) fn base_density_from_base_color(base_color: &BaseColor) -> [f32; 3] {
    [
        density_from_u16(base_color.base_r),
        density_from_u16(base_color.base_g),
        density_from_u16(base_color.base_b),
    ]
}

/// The per-frame film-base density this state should subtract, when it has a
/// trusted one. Retired compatibility records keep their own Status M maths.
pub(crate) fn frame_base_density(
    state: &PipelineState,
    base_color: &BaseColor,
) -> Option<[f32; 3]> {
    if state.contract == ProcessingContract::LegacyV1
        || state.density_anchors.d_min_base.is_some()
        || state.processing_report.analysis_data_domain == "legacy_linear_srgb"
        || *base_color == BaseColor::default()
    {
        return None;
    }
    let source = state.processing_report.base_source.as_str();
    if source.is_empty() || source == "unresolved" || source == "compatibility_fallback" {
        return None;
    }
    Some(base_density_from_base_color(base_color))
}

/// Recorded when a frame carries a film base that was measured on another
/// frame of the same Roll instead of on its own pixels.
pub(crate) const INHERITED_FILM_BASE_SOURCE: &str = "inherited_film_base";

/// True when a persisted base belongs to the frame that stores it.
///
/// The marker has to be read together with the value: a frame that was reset,
/// or written before its analysis finished, can still hold the default
/// half-white colour, which is not a measurement of anything.
pub(crate) fn base_is_frame_measurement(source: &str, base_color: &BaseColor) -> bool {
    if *base_color == BaseColor::default() {
        return false;
    }
    !matches!(
        source,
        "" | "unresolved"
            | INHERITED_FILM_BASE_SOURCE
            | "compatibility_fallback"
            | "missing_film_base_reference"
    )
}

#[cfg(test)]
mod tests {
    use super::FilmPipeline;
    use crate::app_state::{
        BaseColor, DensityAnchor, DensityAnchorConfidence, DensityAnchorScope, DensityAnchorSource,
        DensityAnchors, FilmMode, PipelineState, ProcessingContract,
    };
    use crate::raw_backend::{
        correct_cfa_capture, demosaic_bayer_fixed, CaptureConditions, CfaPattern, RawMetadata,
        RawMosaic,
    };

    fn synthetic_raw(value: u16) -> RawMosaic {
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
                camera_id: "pipeline-test-camera".to_string(),
                libraw_version: "test".to_string(),
                capture_conditions: CaptureConditions::default(),
            },
        }
    }

    #[test]
    fn capture_samples_produce_relative_transmission_density_and_anchored_state() {
        let sample = synthetic_raw(550);
        let dark = synthetic_raw(100);
        let open = synthetic_raw(1000);
        let corrected = correct_cfa_capture(&sample, Some(&dark), Some(&open), &[]);
        let camera_native = demosaic_bayer_fixed(&corrected.expect("capture correction"))
            .expect("fixed Bayer demosaic");
        let transmission = [
            camera_native.pixels[0],
            camera_native.pixels[1],
            camera_native.pixels[2],
        ];
        assert!(transmission
            .iter()
            .all(|value| (*value - 0.5).abs() < 1.0e-6));

        let anchors = DensityAnchors {
            d_min_base: Some(DensityAnchor {
                density: [0.1; 3],
                source: DensityAnchorSource::SampledFilmBase,
                scope: DensityAnchorScope::Roll,
                confidence: DensityAnchorConfidence::UserSampled,
                reference_id: Some("roll-a:base".to_string()),
                provenance: Default::default(),
            }),
            d_max_full_exposure: Some(DensityAnchor {
                density: [1.2; 3],
                source: DensityAnchorSource::SampledFullExposure,
                scope: DensityAnchorScope::Roll,
                confidence: DensityAnchorConfidence::UserSampled,
                reference_id: Some("roll-a:leader".to_string()),
                provenance: Default::default(),
            }),
            retained_records: Vec::new(),
            highlight_fraction: None,
        };
        assert!(anchors.is_fully_anchored());
        let state = PipelineState::capture_corrected(anchors, false);
        let pipeline =
            FilmPipeline::from_state(&state, &BaseColor::default(), [0.0; 3], FilmMode::Color);
        let density = pipeline
            .compute_relative_density(&transmission, camera_native.quality.valid[0])
            .expect("valid relative transmission");
        let expected = -0.5f32.log10() - 0.1;
        assert!(density
            .iter()
            .all(|value| (*value - expected).abs() < 1.0e-6));
    }

    #[test]
    fn monochrome_density_uses_green_heavy_capture_luminance() {
        let pipeline = FilmPipeline::new([u16::MAX; 3], [0.0; 3], FilmMode::BW);
        let density = pipeline.compute_true_density(&[1.0, 0.1, 1.0]);
        assert!((density[0] - 0.7152).abs() < 1e-4);
        assert_eq!(density[0], density[1]);
        assert_eq!(density[1], density[2]);
    }

    #[test]
    fn prophoto_contract_does_not_apply_status_m() {
        let pipeline = FilmPipeline::new_prophoto(
            [0.1, 0.2, 0.3],
            [0.0; 3],
            FilmMode::Color,
            ProcessingContract::SmartAutoProPhotoV11,
        );
        let density = pipeline.compute_true_density(&[0.2, 0.2, 0.2]);
        let expected = -0.2f32.log10();
        assert!((density[0] - (expected - 0.1)).abs() < 1e-6);
        assert!((density[1] - (expected - 0.2)).abs() < 1e-6);
        assert!((density[2] - (expected - 0.3)).abs() < 1e-6);
    }

    #[test]
    fn legacy_contract_retains_the_historical_status_m_result() {
        let pipeline = FilmPipeline::new([u16::MAX; 3], [0.0; 3], FilmMode::Color);
        let input = [0.5f32, 0.25, 0.125];
        let raw_density =
            nalgebra::Vector3::new(-input[0].log10(), -input[1].log10(), -input[2].log10());
        let expected = crate::core_math::status_m_crosstalk_matrix() * raw_density;
        let actual = pipeline.compute_true_density(&input);
        assert!((actual[0] - expected.x).abs() < 1.0e-6);
        assert!((actual[1] - expected.y).abs() < 1.0e-6);
        assert!((actual[2] - expected.z).abs() < 1.0e-6);
    }

    #[test]
    fn state_uses_roll_base_anchor_for_prophoto_density() {
        let mut state = PipelineState::smart_auto();
        state.density_anchors = DensityAnchors {
            d_min_base: Some(crate::app_state::DensityAnchor {
                density: [0.11, 0.22, 0.33],
                source: crate::app_state::DensityAnchorSource::SampledFilmBase,
                scope: crate::app_state::DensityAnchorScope::Roll,
                confidence: crate::app_state::DensityAnchorConfidence::UserSampled,
                reference_id: None,
                provenance: Default::default(),
            }),
            d_max_full_exposure: None,
            retained_records: Vec::new(),
            highlight_fraction: None,
        };
        state.contract = ProcessingContract::RollBaseProPhotoV11;
        let pipeline =
            FilmPipeline::from_state(&state, &BaseColor::default(), [0.0; 3], FilmMode::Color);
        let density = pipeline.compute_true_density(&[0.5, 0.5, 0.5]);
        assert!((density[0] - (-0.5f32.log10() - 0.11)).abs() < 1e-6);
    }

    #[test]
    fn smart_auto_frame_estimate_is_the_render_base() {
        // A loose frame has no physical anchor, but the per-frame film-base
        // estimate is its neutral reference: subtracting it per channel is what
        // removes the mask. Aligning on the content instead cancelled
        // scene-wide colour and gave colour-dominant scenes an opposite cast.
        let mut state = PipelineState::smart_auto();
        state.processing_report.base_source = "content_estimate".to_string();
        let base_color = BaseColor {
            base_r: 10000,
            base_g: 20000,
            base_b: 30000,
        };
        let pipeline = FilmPipeline::from_state(&state, &base_color, [0.0; 3], FilmMode::Color);
        let density = pipeline.compute_true_density(&[0.5, 0.5, 0.5]);
        for (channel, value) in density.iter().enumerate() {
            let expected = -0.5f32.log10()
                - crate::pipeline::base_density_from_base_color(&base_color)[channel];
            assert!((*value - expected).abs() < 1e-6, "{channel}: {density:?}");
        }
        // The estimate still never becomes a persisted physical anchor.
        assert!(state.density_anchors.d_min_base.is_none());

        // Without a trusted estimate the render keeps the zero reference.
        state.processing_report.base_source = "unresolved".to_string();
        let pipeline = FilmPipeline::from_state(&state, &base_color, [0.0; 3], FilmMode::Color);
        let density = pipeline.compute_true_density(&[0.5, 0.5, 0.5]);
        let expected = -0.5f32.log10();
        assert!(density.iter().all(|value| (*value - expected).abs() < 1e-6));
    }

    #[test]
    fn relative_density_rejects_invalid_input_without_epsilon_repair() {
        let pipeline = FilmPipeline::new_prophoto(
            [0.0; 3],
            [0.0; 3],
            FilmMode::Color,
            ProcessingContract::CaptureCorrectedV11,
        );
        assert_eq!(
            pipeline.compute_relative_density(&[0.5, 0.25, 0.125], true),
            Some([0.30103, 0.60206, 0.90309])
        );
        assert_eq!(
            pipeline.compute_relative_density(&[0.5, 0.0, 0.125], true),
            None
        );
        assert_eq!(
            pipeline.compute_relative_density(&[0.5, 0.25, 0.125], false),
            None
        );
    }

    #[test]
    fn prophoto_estimate_clamps_invalid_transport_without_nan() {
        let pipeline = FilmPipeline::new_prophoto(
            [0.0; 3],
            [0.0; 3],
            FilmMode::Color,
            ProcessingContract::SmartAutoProPhotoV11,
        );
        let density = pipeline.compute_true_density(&[-1.0, 2.0, f32::NAN]);
        assert!(density.iter().all(|value| value.is_finite()));
    }
}
