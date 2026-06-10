use nalgebra::Matrix3;

/// 返回预定义的核心去串扰矩阵 (Status M to Print Density)。
/// 这个 3x3 矩阵用于消除胶片各染料层之间的光谱串扰。
/// 
/// Matrix layout (Row-major initialization):
/// R:  1.0197,  0.0317,  0.0091
/// G: -0.0052,  0.8933,  0.0521
/// B:  0.0131, -0.0011,  0.9712
#[inline(always)]
pub fn status_m_crosstalk_matrix() -> Matrix3<f32> {
    Matrix3::new(
        1.0197,  0.0317,  0.0091,
       -0.0052,  0.8933,  0.0521,
        0.0131, -0.0011,  0.9712,
    )
}
