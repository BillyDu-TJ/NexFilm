use crate::pipeline::FilmPipeline;
use image::ImageResult;
use rayon::prelude::*;

/// 读取 16-bit 图像，结合指定的物理管线实例进行并发对数转换和去串扰。
pub fn process_image_file(input_path: &str, output_path: &str, pipeline: &FilmPipeline, d_min: f32, d_max: f32, gamma: f32) -> ImageResult<()> {
    // 1. 读取并转换为 16-bit RGB
    let img = image::open(input_path)?;
    let mut img_buffer = img.into_rgb16();

    // 2. 提取底层 buffer 并行处理
    // pipeline 对象被不可变借用，在 Rayon 多线程间安全共享！
    let raw_buffer: &mut [u16] = img_buffer.as_mut();

    raw_buffer.par_chunks_mut(3).for_each(|pixel| {
        // [A] 数据映射：16-bit (0-65535) -> 线性透射率浮点数 (0.0-1.0)
        let linear_rgb = [
            (pixel[0] as f32) / 65535.0,
            (pixel[1] as f32) / 65535.0,
            (pixel[2] as f32) / 65535.0,
        ];

        // [B] 物理管线运算：原始密度转换 -> 扣除色罩 -> 去除串扰
        let density = pipeline.process_pixel(&linear_rgb);

        // [C] Log 映射 (The Math)：使用 d_min 和 d_max 进行区间归一化，并引入 gamma 曲线进行反差增强
        let norm_r = ((density[0] - d_min) / (d_max - d_min)).clamp(0.0, 1.0);
        let norm_g = ((density[1] - d_min) / (d_max - d_min)).clamp(0.0, 1.0);
        let norm_b = ((density[2] - d_min) / (d_max - d_min)).clamp(0.0, 1.0);

        pixel[0] = (norm_r.powf(1.0 / gamma) * 65535.0) as u16;
        pixel[1] = (norm_g.powf(1.0 / gamma) * 65535.0) as u16;
        pixel[2] = (norm_b.powf(1.0 / gamma) * 65535.0) as u16;
    });

    // 3. 将处理后的结果存回 16-bit 格式
    img_buffer.save(output_path)?;

    Ok(())
}
