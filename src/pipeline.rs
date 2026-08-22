use crate::core_math::{density_luma, status_m_crosstalk_matrix};
use nalgebra::{Matrix3, Vector3};

use crate::app_state::{BaseColor, FilmMode, PipelineState, ProcessingContract};

/// 胶片物理处理管线状态对象。
/// 封装了色彩变换矩阵与下限阈值，片基密度 (D_min) 以及通道偏移补偿。
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

    /// v1.1 scientific path: ProPhoto RGB is the shared linear basis and no
    /// Status M matrix is applied at this lowest contract level. Density
    /// channel alignment is therefore the explicit base subtraction itself.
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
                [
                    density_from_u16(base_color.base_r),
                    density_from_u16(base_color.base_g),
                    density_from_u16(base_color.base_b),
                ]
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

#[cfg(test)]
mod tests {
    use super::FilmPipeline;
    use crate::app_state::{
        BaseColor, DensityAnchors, FilmMode, PipelineState, ProcessingContract,
    };

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
    fn state_uses_roll_base_anchor_for_prophoto_density() {
        let mut state = PipelineState::smart_auto();
        state.density_anchors = DensityAnchors {
            d_min_base: Some(crate::app_state::DensityAnchor {
                density: [0.11, 0.22, 0.33],
                source: crate::app_state::DensityAnchorSource::SampledFilmBase,
                scope: crate::app_state::DensityAnchorScope::Roll,
                confidence: crate::app_state::DensityAnchorConfidence::UserSampled,
                reference_id: None,
            }),
            d_max_full_exposure: None,
        };
        state.contract = ProcessingContract::RollBaseProPhotoV11;
        let pipeline =
            FilmPipeline::from_state(&state, &BaseColor::default(), [0.0; 3], FilmMode::Color);
        let density = pipeline.compute_true_density(&[0.5, 0.5, 0.5]);
        assert!((density[0] - (-0.5f32.log10() - 0.11)).abs() < 1e-6);
    }
}
