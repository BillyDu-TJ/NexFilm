use nalgebra::{Matrix3, Vector3};
use crate::core_math::status_m_crosstalk_matrix;

use crate::app_state::FilmMode;

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

        let base_density = Vector3::new(
            -t_r.log10(),
            -t_g.log10(),
            -t_b.log10(),
        );

        Self {
            crosstalk_matrix: status_m_crosstalk_matrix(),
            epsilon,
            base_density,
            exposure_offset: Vector3::new(exp_offset[0], exp_offset[1], exp_offset[2]),
            mode,
        }
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

        let d_raw = Vector3::new(
            -t_r.log10(),
            -t_g.log10(),
            -t_b.log10(),
        );

        let delta_d = Vector3::new(
            (d_raw.x - self.base_density.x).max(0.0),
            (d_raw.y - self.base_density.y).max(0.0),
            (d_raw.z - self.base_density.z).max(0.0),
        );

        match self.mode {
            FilmMode::Color => {
                let true_density_vec = self.crosstalk_matrix * delta_d;
                [true_density_vec.x, true_density_vec.y, true_density_vec.z]
            }
            FilmMode::BW => {
                // 黑白管线：不应用串扰矩阵，输出灰度密度
                let gray_density = (delta_d.x + delta_d.y + delta_d.z) / 3.0;
                [gray_density, gray_density, gray_density]
            }
        }
    }

    /// 应用曝光偏移并防止负密度
    #[inline]
    pub fn apply_exposure(&self, true_density: &[f32; 3]) -> [f32; 3] {
        match self.mode {
            FilmMode::Color => {
                let final_r = (true_density[0] + self.exposure_offset.x).max(0.0);
                let final_g = (true_density[1] + self.exposure_offset.y).max(0.0);
                let final_b = (true_density[2] + self.exposure_offset.z).max(0.0);
                [final_r, final_g, final_b]
            }
            FilmMode::BW => {
                // 黑白模式下旁路偏色设置，仅应用基础曝光补偿（取第一通道偏移量或者忽略偏色）
                let final_gray = (true_density[0] + self.exposure_offset.x).max(0.0);
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
