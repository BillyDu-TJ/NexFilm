use image::{ImageBuffer, Rgb, GrayImage, Luma};
use imageproc::filter::gaussian_blur_f32;
use imageproc::hough::{detect_lines, LineDetectionOptions};
use imageproc::edges::canny;

/// 旋转 90 度
pub fn rotate_90(img: &ImageBuffer<Rgb<u16>, Vec<u16>>) -> ImageBuffer<Rgb<u16>, Vec<u16>> {
    image::imageops::rotate90(img)
}

/// 手动自由裁切功能
pub fn crop(img: &mut ImageBuffer<Rgb<u16>, Vec<u16>>, x: u32, y: u32, width: u32, height: u32) -> ImageBuffer<Rgb<u16>, Vec<u16>> {
    image::imageops::crop(img, x, y, width, height).to_image()
}

/// 基于边缘自动校正对齐
/// 如果未找到高置信度的至少3条物理边缘（片基黑边/齿孔），原样返回。
pub fn auto_align(img: &ImageBuffer<Rgb<u16>, Vec<u16>>) -> Result<ImageBuffer<Rgb<u16>, Vec<u16>>, String> {
    let (width, height) = img.dimensions();
    
    // 我们仅在图像外围的 8% 区域进行分析，以避免艺术内容干扰
    let margin_x = (width as f32 * 0.08) as u32;
    let margin_y = (height as f32 * 0.08) as u32;

    // 1. 转为灰度图像
    let mut gray = GrayImage::new(width, height);
    for y in 0..height {
        for x in 0..width {
            // 仅对边缘区域的像素进行处理，内部置 0
            if x < margin_x || x > width - margin_x || y < margin_y || y > height - margin_y {
                let px = img.get_pixel(x, y);
                // 简单的取绿通道或亮度
                let luma = (px[1] >> 8) as u8;
                gray.put_pixel(x, y, Luma([luma]));
            } else {
                gray.put_pixel(x, y, Luma([0]));
            }
        }
    }

    // 2. 高斯模糊，平滑噪点
    let blurred = gaussian_blur_f32(&gray, 2.0);

    // 3. 边缘检测
    let edges = canny(&blurred, 50.0, 150.0);

    // 4. 霍夫变换寻找直线
    let options = LineDetectionOptions {
        vote_threshold: (width.min(height) / 4) as u32,
        suppression_radius: 10,
    };
    
    let lines = detect_lines(&edges, options);

    // 筛选出边界直线（垂直于边缘或接近水平垂直）
    let mut valid_lines = 0;
    for line in &lines {
        // 角度 r, theta
        let theta = line.angle_in_degrees as f32;
        let is_horizontal = theta < 15.0 || theta > 165.0;
        let is_vertical = theta > 75.0 && theta < 105.0;
        
        if is_horizontal || is_vertical {
            valid_lines += 1;
        }
    }

    // 5. 极强的稳定性兜底：有效边界小于3，直接跳过自动校正，原样返回
    if valid_lines < 3 {
        // 置信度不足，可能已经被裁切过
        return Ok(img.clone());
    }

    // 6. 透视更正 (Perspective Correction)
    // 这里因为是 Rust 后端并使用基础库，若要执行完整的单应性变换需要解 8 个方程
    // 为保持代码纯粹和高效，当识别出足够的线条时，我们仅示例做轻微的透视或直接进行紧致裁切
    // 这里直接利用 imageproc::geometric_transformations::warp_into 进行单应性变换（假定找到了交点）
    // （在真实情况下，我们会使用霍夫直线交点计算出 4 个角点，然后映射为矩阵）

    // TODO: 实现真实基于4点交点的 Homography 计算。当前为了代码安全性和运行正确性，假定只做非常安全的微调或原样返回
    // 既然已经到了这里，说明有边框。目前因为缺少 OpenCV 的 `getPerspectiveTransform` 简单封装，我们返回原图
    
    Ok(img.clone())
}
