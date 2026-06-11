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

use crate::app_state::{CropRect, AutoAlignResult};

/// 基于边缘内容自动算出内部有效区域的边界
pub fn auto_crop_rect(img: &ImageBuffer<Rgb<u16>, Vec<u16>>) -> Result<AutoAlignResult, String> {
    let (width, height) = img.dimensions();
    
    // 1. 转为灰度图像
    let mut gray = GrayImage::new(width, height);
    for y in 0..height {
        for x in 0..width {
            let px = img.get_pixel(x, y);
            // 简单的亮度转换
            let luma = (px[0] as u32 + px[1] as u32 + px[2] as u32) / (3 * 256);
            gray.put_pixel(x, y, Luma([luma as u8]));
        }
    }

    // 2. 高斯模糊，过滤噪点
    let blurred = gaussian_blur_f32(&gray, 3.0);

    // 3. 边缘检测
    let edges = canny(&blurred, 40.0, 100.0);

    // 4. 霍夫直线检测
    let options = LineDetectionOptions {
        vote_threshold: (width.min(height) / 4) as u32,
        suppression_radius: 10,
    };
    let lines = detect_lines(&edges, options);

    let margin_w = width as f32 * 0.25;
    let margin_h = height as f32 * 0.25;

    let mut best_top = 0.0_f32;
    let mut best_bottom = height as f32;
    let mut best_left = 0.0_f32;
    let mut best_right = width as f32;

    let mut best_top_deg = 90;
    let mut best_bottom_deg = 90;
    let mut best_left_deg = 0;
    let mut best_right_deg = 0;

    for line in lines {
        let deg = line.angle_in_degrees;
        let r = line.r;
        let rad = (deg as f32).to_radians();
        let cos_a = rad.cos();
        let sin_a = rad.sin();

        // 水平线
        if deg > 75 && deg < 105 {
            let y_int = r / sin_a;
            if y_int < margin_h && y_int > best_top {
                best_top = y_int; // 取最靠里的顶线
                best_top_deg = deg;
            } else if y_int > height as f32 - margin_h && y_int < best_bottom {
                best_bottom = y_int; // 取最靠里的底线
                best_bottom_deg = deg;
            }
        }
        // 垂直线
        else if deg < 15 || deg > 165 {
            let x_int = r / cos_a;
            let x_int_abs = x_int.abs(); // r 可能是负的，如果 deg > 165, cos 是负数，r也是负数，结果是正的
            if x_int_abs < margin_w && x_int_abs > best_left {
                best_left = x_int_abs;
                best_left_deg = deg;
            } else if x_int_abs > width as f32 - margin_w && x_int_abs < best_right {
                best_right = x_int_abs;
                best_right_deg = deg;
            }
        }
    }

    // 如果未检测到，使用安全的保底裁切
    let final_x = if best_left > 0.0 { best_left / width as f32 } else { 0.05 };
    let final_y = if best_top > 0.0 { best_top / height as f32 } else { 0.05 };
    let final_r = if best_right < width as f32 { best_right / width as f32 } else { 0.95 };
    let final_b = if best_bottom < height as f32 { best_bottom / height as f32 } else { 0.95 };

    let mut angles = Vec::new();
    if best_top > 0.0 { angles.push(best_top_deg as i32 - 90); }
    if best_bottom < height as f32 { angles.push(best_bottom_deg as i32 - 90); }
    if best_left > 0.0 { 
        let d = if best_left_deg > 90 { best_left_deg as i32 - 180 } else { best_left_deg as i32 };
        angles.push(d); 
    }
    if best_right < width as f32 { 
        let d = if best_right_deg > 90 { best_right_deg as i32 - 180 } else { best_right_deg as i32 };
        angles.push(d); 
    }

    let avg_angle = if !angles.is_empty() {
        let sum: i32 = angles.iter().sum();
        sum as f32 / angles.len() as f32
    } else {
        0.0
    };

    let rect = CropRect {
        x: final_x,
        y: final_y,
        width: (final_r - final_x).max(0.1),
        height: (final_b - final_y).max(0.1),
    };

    Ok(AutoAlignResult {
        crop_rect: rect,
        angle: avg_angle,
    })
}
