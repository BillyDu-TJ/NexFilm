use crate::app_state::{BaseColor, EngineState, FilmItem, FilmstripItem, TuningParams, FilmMode};
use crate::pipeline::FilmPipeline;
use crate::geometry;
use base64::{engine::general_purpose, Engine as _};
use image::{imageops::FilterType, ImageBuffer, ImageOutputFormat, Rgb, RgbImage};
use rayon::prelude::*;
use rfd::FileDialog;
use std::io::Cursor;
use tauri::State;
use std::sync::atomic::{AtomicUsize, Ordering};

static NEXT_ID: AtomicUsize = AtomicUsize::new(1);

fn compute_auto_base(proxy: &ImageBuffer<Rgb<u16>, Vec<u16>>) -> BaseColor {
    let mut r_vals: Vec<u16> = Vec::with_capacity((proxy.width() * proxy.height()) as usize);
    let mut g_vals: Vec<u16> = Vec::with_capacity((proxy.width() * proxy.height()) as usize);
    let mut b_vals: Vec<u16> = Vec::with_capacity((proxy.width() * proxy.height()) as usize);

    for pixel in proxy.pixels() {
        r_vals.push(pixel[0]);
        g_vals.push(pixel[1]);
        b_vals.push(pixel[2]);
    }

    r_vals.sort_unstable();
    g_vals.sort_unstable();
    b_vals.sort_unstable();

    let idx = (r_vals.len() as f32 * 0.99) as usize;
    BaseColor {
        base_r: r_vals[idx],
        base_g: g_vals[idx],
        base_b: b_vals[idx],
    }
}

fn compute_pristine_proxy(
    proxy: &ImageBuffer<Rgb<u16>, Vec<u16>>,
    base_color: &BaseColor,
    mode: FilmMode,
) -> ImageBuffer<Rgb<f32>, Vec<f32>> {
    let pipeline = FilmPipeline::new(
        [base_color.base_r, base_color.base_g, base_color.base_b],
        [0.0, 0.0, 0.0],
        mode,
    );
    let (width, height) = proxy.dimensions();
    let mut pristine = ImageBuffer::<Rgb<f32>, Vec<f32>>::new(width, height);
    
    let raw_pixels: &[u16] = proxy.as_raw().as_slice();
    let out_pixels: &mut [f32] = pristine.as_mut();
    
    raw_pixels.par_chunks(3).zip(out_pixels.par_chunks_mut(3)).for_each(|(in_px, out_px)| {
        let linear_rgb = [
            (in_px[0] as f32) / 65535.0,
            (in_px[1] as f32) / 65535.0,
            (in_px[2] as f32) / 65535.0,
        ];
        let true_density = pipeline.compute_true_density(&linear_rgb);
        out_px[0] = true_density[0];
        out_px[1] = true_density[1];
        out_px[2] = true_density[2];
    });
    
    pristine
}

#[tauri::command]
pub async fn open_file_dialog() -> Result<Vec<String>, String> {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let file_paths = FileDialog::new()
            .add_filter("RAW Images", &["dng", "nef", "cr2", "cr3", "arw", "raf", "tiff", "tif"])
            .pick_files();
        let _ = tx.send(file_paths);
    });
    
    let file_paths = rx.recv().unwrap_or(None);
    
    if let Some(paths) = file_paths {
        Ok(paths.into_iter().map(|p| p.to_string_lossy().to_string()).collect())
    } else {
        Ok(Vec::new())
    }
}

#[tauri::command]
pub async fn select_export_dir() -> Result<Option<String>, String> {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let dir_path = FileDialog::new().pick_folder();
        let _ = tx.send(dir_path);
    });
    
    let dir_path = rx.recv().unwrap_or(None);
    Ok(dir_path.map(|p| p.to_string_lossy().to_string()))
}

pub fn load_image_buffer(path: &str) -> Result<ImageBuffer<Rgb<u16>, Vec<u16>>, String> {
    if path.to_lowercase().ends_with(".tif") || path.to_lowercase().ends_with(".tiff") {
        image::open(path).map(|i| i.into_rgb16()).map_err(|e| format!("TIFF读取失败: {:?}", e))
    } else {
        let buf = std::fs::read(path).map_err(|e| format!("RAW文件读取失败: {:?}", e))?;
        let processor = libraw::Processor::new();
        let mem_image = processor.process_16bit(&buf).map_err(|e| format!("RAW图像处理失败: {:?}", e))?;
        
        let data: &[u16] = &mem_image;
        let width = mem_image.width() as u32;
        let height = mem_image.height() as u32;
        let colors = 3;
        
        let mut img_buffer = ImageBuffer::<Rgb<u16>, Vec<u16>>::new(width, height);
        for (i, pixel) in img_buffer.pixels_mut().enumerate() {
            let idx = i * colors;
            pixel[0] = data.get(idx).copied().unwrap_or(0);
            pixel[1] = data.get(idx + 1).copied().unwrap_or(0);
            pixel[2] = data.get(idx + 2).copied().unwrap_or(0);
        }
        Ok(img_buffer)
    }
}

#[tauri::command]
pub async fn import_images(paths: Vec<String>, state: State<'_, EngineState>) -> Result<(), String> {
    if paths.is_empty() {
        return Ok(());
    }

    let new_items_result: Result<Vec<FilmItem>, String> = paths.into_par_iter().map(|path| {
        let img_buffer = load_image_buffer(&path)?;

        let (width, height) = img_buffer.dimensions();
        
        let ratio_proxy = 800.0 / (width.max(height) as f32);
        let proxy_width = (width as f32 * ratio_proxy).max(1.0) as u32;
        let proxy_height = (height as f32 * ratio_proxy).max(1.0) as u32;
        let proxy = image::imageops::resize(&img_buffer, proxy_width, proxy_height, FilterType::Triangle);

        let ratio_thumb = 120.0 / (width.max(height) as f32);
        let thumb_width = (width as f32 * ratio_thumb).max(1.0) as u32;
        let thumb_height = (height as f32 * ratio_thumb).max(1.0) as u32;
        let thumb = image::imageops::resize(&img_buffer, thumb_width, thumb_height, FilterType::Triangle);

        let mut cursor = Cursor::new(Vec::new());
        let mut thumb_8bit = RgbImage::new(thumb_width, thumb_height);
        for (in_px, out_px) in thumb.pixels().zip(thumb_8bit.pixels_mut()) {
            out_px[0] = (in_px[0] >> 8) as u8;
            out_px[1] = (in_px[1] >> 8) as u8;
            out_px[2] = (in_px[2] >> 8) as u8;
        }
        thumb_8bit.write_to(&mut cursor, ImageOutputFormat::Jpeg(70)).map_err(|e| format!("缩略图生成失败: {:?}", e))?;
        let thumbnail_base64 = general_purpose::STANDARD.encode(cursor.into_inner());

        let base_color = compute_auto_base(&proxy);
        let pristine_proxy = compute_pristine_proxy(&proxy, &base_color, FilmMode::Color);

        let id = format!("img_{}", NEXT_ID.fetch_add(1, Ordering::SeqCst));

        Ok(FilmItem {
            id,
            file_path: path,
            thumbnail_base64,
            original_proxy: proxy.clone(),
            proxy_image: proxy,
            pristine_proxy,
            base_color,
            params: TuningParams::default(),
            rotation: 0,
            crop_rect: crate::app_state::CropRect::default(),
        })
    }).collect();

    let mut new_items = new_items_result?;
    let mut items = state.items.write().map_err(|e| e.to_string())?;

    // If no active item, set the first new one
    if state.active_id.read().map_err(|e| e.to_string())?.is_none() && !new_items.is_empty() {
        *state.active_id.write().map_err(|e| e.to_string())? = Some(new_items[0].id.clone());
    }
    
    items.append(&mut new_items);

    Ok(())
}

#[tauri::command]
pub async fn get_filmstrip(state: State<'_, EngineState>) -> Result<Vec<FilmstripItem>, String> {
    let items = state.items.read().map_err(|e| e.to_string())?;
    let strip = items.iter().map(|item| FilmstripItem {
        id: item.id.clone(),
        file_path: item.file_path.clone(),
        thumbnail_base64: item.thumbnail_base64.clone(),
    }).collect();
    Ok(strip)
}

#[derive(serde::Serialize)]
pub struct ActiveImageState {
    pub params: TuningParams,
    pub crop_rect: crate::app_state::CropRect,
}

#[tauri::command]
pub async fn switch_active_image(id: String, state: State<'_, EngineState>) -> Result<ActiveImageState, String> {
    let items = state.items.read().map_err(|e| e.to_string())?;
    if let Some(item) = items.iter().find(|i| i.id == id) {
        *state.active_id.write().map_err(|e| e.to_string())? = Some(id);
        Ok(ActiveImageState {
            params: item.params.clone(),
            crop_rect: item.crop_rect.clone(),
        })
    } else {
        Err("Image ID not found".into())
    }
}

#[tauri::command]
pub async fn set_film_mode(id: String, mode: String, state: State<'_, EngineState>) -> Result<(), String> {
    let mut items = state.items.write().map_err(|e| e.to_string())?;
    if let Some(item) = items.iter_mut().find(|i| i.id == id) {
        let new_mode = if mode == "B&W" { FilmMode::BW } else { FilmMode::Color };
        if item.params.film_mode != new_mode {
            item.params.film_mode = new_mode.clone();
            // Recompute pristine proxy because true density differs between Color and B&W
            let pipeline = FilmPipeline::new(
                [item.base_color.base_r, item.base_color.base_g, item.base_color.base_b],
                [0.0, 0.0, 0.0],
                new_mode,
            );
            
            let proxy = &item.proxy_image;
            let (width, height) = proxy.dimensions();
            let mut pristine = ImageBuffer::<Rgb<f32>, Vec<f32>>::new(width, height);
            
            let raw_pixels: &[u16] = proxy.as_raw().as_slice();
            let out_pixels: &mut [f32] = pristine.as_mut();
            
            raw_pixels.par_chunks(3).zip(out_pixels.par_chunks_mut(3)).for_each(|(in_px, out_px)| {
                let linear_rgb = [
                    (in_px[0] as f32) / 65535.0,
                    (in_px[1] as f32) / 65535.0,
                    (in_px[2] as f32) / 65535.0,
                ];
                let true_density = pipeline.compute_true_density(&linear_rgb);
                out_px[0] = true_density[0];
                out_px[1] = true_density[1];
                out_px[2] = true_density[2];
            });
            item.pristine_proxy = pristine;
        }
    }
    Ok(())
}

#[tauri::command]
pub async fn sync_thumbnail_buffer(id: String, state: State<'_, EngineState>) -> Result<(), String> {
    let mut new_thumbnail = String::new();
    {
        let items = state.items.read().map_err(|e| e.to_string())?;
        if let Some(item) = items.iter().find(|i| i.id == id) {
            let params = &item.params;
            let base_color = &item.base_color;
            let pipeline = FilmPipeline::new(
                [base_color.base_r, base_color.base_g, base_color.base_b],
                [
                    params.exposure + params.exp_r,
                    params.exposure + params.exp_g,
                    params.exposure + params.exp_b,
                ],
                params.film_mode.clone(),
            );

            let pristine = &item.pristine_proxy;
            let (width, height) = pristine.dimensions();
            let mut thumb_8bit = RgbImage::new(width, height);
            
            let pristine_pixels: &[f32] = pristine.as_raw().as_slice();
            let out_pixels: &mut [u8] = thumb_8bit.as_mut();

            let d_min = params.d_min;
            let d_max = params.d_max;
            let gamma = params.gamma;

            pristine_pixels.par_chunks(3).zip(out_pixels.par_chunks_mut(3)).for_each(|(in_px, out_px)| {
                let true_density = [in_px[0], in_px[1], in_px[2]];
                let density = pipeline.apply_exposure(&true_density);

                let norm_r = ((density[0] - d_min) / (d_max - d_min)).clamp(0.0, 1.0);
                let norm_g = ((density[1] - d_min) / (d_max - d_min)).clamp(0.0, 1.0);
                let norm_b = ((density[2] - d_min) / (d_max - d_min)).clamp(0.0, 1.0);

                out_px[0] = (norm_r.powf(1.0 / gamma) * 255.0) as u8;
                out_px[1] = (norm_g.powf(1.0 / gamma) * 255.0) as u8;
                out_px[2] = (norm_b.powf(1.0 / gamma) * 255.0) as u8;
            });
            
            let ratio_thumb = 120.0 / (width.max(height) as f32);
            let thumb_width = (width as f32 * ratio_thumb).max(1.0) as u32;
            let thumb_height = (height as f32 * ratio_thumb).max(1.0) as u32;
            let thumb = image::imageops::resize(&thumb_8bit, thumb_width, thumb_height, FilterType::Triangle);
            
            let mut cursor = Cursor::new(Vec::new());
            thumb.write_to(&mut cursor, ImageOutputFormat::Jpeg(70)).map_err(|e| e.to_string())?;
            new_thumbnail = general_purpose::STANDARD.encode(cursor.into_inner());
        }
    }
    
    if !new_thumbnail.is_empty() {
        let mut items = state.items.write().map_err(|e| e.to_string())?;
        if let Some(item) = items.iter_mut().find(|i| i.id == id) {
            item.thumbnail_base64 = new_thumbnail;
        }
    }
    Ok(())
}

#[tauri::command]
pub async fn geometry_rotate(id: String, direction: String, state: State<'_, EngineState>) -> Result<(), String> {
    let mut items = state.items.write().map_err(|e| e.to_string())?;
    if let Some(item) = items.iter_mut().find(|i| i.id == id) {
        if direction == "left" {
            item.rotation = (item.rotation - 90).rem_euclid(360);
        } else {
            item.rotation = (item.rotation + 90).rem_euclid(360);
        }
        reapply_geometry(item);
    }
    Ok(())
}

#[tauri::command]
pub async fn geometry_crop_normalized(id: String, rect: crate::app_state::CropRect, state: State<'_, EngineState>) -> Result<(), String> {
    let mut items = state.items.write().map_err(|e| e.to_string())?;
    if let Some(item) = items.iter_mut().find(|i| i.id == id) {
        item.crop_rect = rect;
        reapply_geometry(item);
    }
    Ok(())
}

fn reapply_geometry(item: &mut FilmItem) {
    let mut current = item.original_proxy.clone();
    
    // 1. Rotate
    match item.rotation {
        90 => current = image::imageops::rotate90(&current),
        180 => current = image::imageops::rotate180(&current),
        270 => current = image::imageops::rotate270(&current),
        _ => {}
    }
    
    // 2. Crop
    let (w, h) = current.dimensions();
    let cx = (item.crop_rect.x * w as f32).max(0.0).min(w as f32) as u32;
    let cy = (item.crop_rect.y * h as f32).max(0.0).min(h as f32) as u32;
    let cw = (item.crop_rect.width * w as f32).max(1.0).min((w - cx) as f32) as u32;
    let ch = (item.crop_rect.height * h as f32).max(1.0).min((h - cy) as f32) as u32;
    
    if cw < w || ch < h {
        current = image::imageops::crop(&mut current, cx, cy, cw, ch).to_image();
    }
    
    item.proxy_image = current;
    item.pristine_proxy = compute_pristine_proxy(&item.proxy_image, &item.base_color, item.params.film_mode.clone());
}

#[tauri::command]
pub async fn geometry_auto_align(id: String, state: State<'_, EngineState>) -> Result<(), String> {
    let mut items = state.items.write().map_err(|e| e.to_string())?;
    if let Some(item) = items.iter_mut().find(|i| i.id == id) {
        let aligned = geometry::auto_align(&item.proxy_image)?;
        item.proxy_image = aligned;
        item.pristine_proxy = compute_pristine_proxy(&item.proxy_image, &item.base_color, item.params.film_mode.clone());
    }
    Ok(())
}

#[tauri::command]
pub async fn apply_tuning_parameters(
    params: TuningParams,
    state: State<'_, EngineState>,
) -> Result<tauri::ipc::Response, String> {
    let active_id = {
        let id_guard = state.active_id.read().map_err(|e| e.to_string())?;
        id_guard.clone().ok_or("No active image")?
    };

    let mut items = state.items.write().map_err(|e| e.to_string())?;
    let item = items.iter_mut().find(|i| i.id == active_id).ok_or("Active image not found")?;
    
    item.params = params.clone();
    let pristine = &item.pristine_proxy;

    let pipeline = FilmPipeline::new(
        [65535, 65535, 65535], // dummy for base
        [
            params.exposure + params.exp_r,
            params.exposure + params.exp_g,
            params.exposure + params.exp_b,
        ],
        params.film_mode.clone(),
    );

    let (width, height) = pristine.dimensions();
    let mut out_buffer = vec![0u8; (width * height * 4) as usize + 8];
    out_buffer[0..4].copy_from_slice(&width.to_le_bytes());
    out_buffer[4..8].copy_from_slice(&height.to_le_bytes());

    let pristine_pixels: &[f32] = pristine.as_raw().as_slice();
    let out_pixels: &mut [u8] = &mut out_buffer[8..];

    let d_min = params.d_min;
    let d_max = params.d_max;
    let gamma = params.gamma;

    pristine_pixels.par_chunks(3).zip(out_pixels.par_chunks_mut(4)).for_each(|(in_px, out_px)| {
        let true_density = [in_px[0], in_px[1], in_px[2]];
        let density = pipeline.apply_exposure(&true_density);

        let norm_r = ((density[0] - d_min) / (d_max - d_min)).clamp(0.0, 1.0);
        let norm_g = ((density[1] - d_min) / (d_max - d_min)).clamp(0.0, 1.0);
        let norm_b = ((density[2] - d_min) / (d_max - d_min)).clamp(0.0, 1.0);

        out_px[0] = (norm_r.powf(1.0 / gamma) * 255.0) as u8;
        out_px[1] = (norm_g.powf(1.0 / gamma) * 255.0) as u8;
        out_px[2] = (norm_b.powf(1.0 / gamma) * 255.0) as u8;
        out_px[3] = 255;
    });

    Ok(tauri::ipc::Response::new(out_buffer))
}

#[tauri::command]
pub async fn batch_export_images(
    output_dir: String,
    state: State<'_, EngineState>,
) -> Result<usize, String> {
    let items = state.items.read().map_err(|e| e.to_string())?;
    let count = items.len();
    if count == 0 {
        return Ok(0);
    }

    let success_count = std::sync::atomic::AtomicUsize::new(0);

    items.par_iter().for_each(|item| {
        if let Ok(original) = load_image_buffer(&item.file_path) {
            let params = &item.params;
            let base_color = &item.base_color;

            let pipeline = FilmPipeline::new(
                [base_color.base_r, base_color.base_g, base_color.base_b],
                [
                    params.exposure + params.exp_r,
                    params.exposure + params.exp_g,
                    params.exposure + params.exp_b,
                ],
                params.film_mode.clone(),
            );

            let mut transformed = original;
            
            // 1. Rotate
            match item.rotation {
                90 => transformed = image::imageops::rotate90(&transformed),
                180 => transformed = image::imageops::rotate180(&transformed),
                270 => transformed = image::imageops::rotate270(&transformed),
                _ => {}
            }
            
            // 2. Crop
            let (orig_width, orig_height) = transformed.dimensions();
            let cx = (item.crop_rect.x * orig_width as f32).max(0.0).min(orig_width as f32) as u32;
            let cy = (item.crop_rect.y * orig_height as f32).max(0.0).min(orig_height as f32) as u32;
            let cw = (item.crop_rect.width * orig_width as f32).max(1.0).min((orig_width - cx) as f32) as u32;
            let ch = (item.crop_rect.height * orig_height as f32).max(1.0).min((orig_height - cy) as f32) as u32;
            
            if cw < orig_width || ch < orig_height {
                transformed = image::imageops::crop(&mut transformed, cx, cy, cw, ch).to_image();
            }

            let (width, height) = transformed.dimensions();
            let mut out_buffer = ImageBuffer::<Rgb<u16>, Vec<u16>>::new(width, height);

            let raw_pixels: &[u16] = transformed.as_raw().as_slice();
            let out_pixels: &mut [u16] = out_buffer.as_mut();

            let d_min = params.d_min;
            let d_max = params.d_max;
            let gamma = params.gamma;

            raw_pixels.par_chunks(3).zip(out_pixels.par_chunks_mut(3)).for_each(|(in_px, out_px)| {
                let linear_rgb = [
                    (in_px[0] as f32) / 65535.0,
                    (in_px[1] as f32) / 65535.0,
                    (in_px[2] as f32) / 65535.0,
                ];

                let density = pipeline.process_pixel(&linear_rgb);

                let norm_r = ((density[0] - d_min) / (d_max - d_min)).clamp(0.0, 1.0);
                let norm_g = ((density[1] - d_min) / (d_max - d_min)).clamp(0.0, 1.0);
                let norm_b = ((density[2] - d_min) / (d_max - d_min)).clamp(0.0, 1.0);

                out_px[0] = (norm_r.powf(1.0 / gamma) * 65535.0) as u16;
                out_px[1] = (norm_g.powf(1.0 / gamma) * 65535.0) as u16;
                out_px[2] = (norm_b.powf(1.0 / gamma) * 65535.0) as u16;
            });

            // Force TIFF extension for 16-bit export
            let file_stem = std::path::Path::new(&item.file_path)
                .file_stem()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string();
            let out_path = std::path::Path::new(&output_dir).join(format!("nexfilm_{}.tiff", file_stem));
            if out_buffer.save(out_path).is_ok() {
                success_count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            }
        }
    });

    Ok(success_count.into_inner())
}
