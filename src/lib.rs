pub mod core_math;
pub mod pipeline;
pub mod io_processor;
pub mod app_state;
pub mod commands;

#[cfg(test)]
mod tests {
    use super::pipeline::FilmPipeline;

    #[test]
    fn test_film_pipeline_basic_run() {
        // 初始化管线
        let pipeline = FilmPipeline::default();
        
        // 假设的 RGB 线性透射率 T (如提示词中提供的 0.1, 0.2, 0.05)
        let input_linear_rgb = [0.1, 0.2, 0.05];
        
        // 执行 Step A-C 的线性处理流
        let output_density = pipeline.process_pixel(&input_linear_rgb);
        
        println!("======================================");
        println!("==   NexFilm Engine Phase 1 Test    ==");
        println!("======================================");
        println!("> Input Linear RGB (T)   : {:?}", input_linear_rgb);
        println!("> Output True Density    : {:?}", output_density);
        println!("======================================");
        
        // 基础验证，确保矩阵数学运算不产生 NaN
        assert!(!output_density[0].is_nan(), "R density is NaN");
        assert!(!output_density[1].is_nan(), "G density is NaN");
        assert!(!output_density[2].is_nan(), "B density is NaN");
    }
}
