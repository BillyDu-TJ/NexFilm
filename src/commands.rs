use crate::app_state::{BaseColor, EngineState, FilmItem, FilmstripItem, TuningParams, FilmMode};
use serde::{Serialize, Deserialize};
use crate::pipeline::FilmPipeline;
use crate::geometry;
use base64::{engine::general_purpose, Engine as _};
use image::{imageops::FilterType, ImageBuffer, ImageOutputFormat, Rgb, RgbImage};
use rayon::prelude::*;
use rfd::FileDialog;
use std::io::Cursor;
use tauri::State;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, RwLock};

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
    let file_paths = tauri::async_runtime::spawn_blocking(|| {
        FileDialog::new()
            .add_filter("RAW Images", &["dng", "nef", "cr2", "cr3", "arw", "raf", "tiff", "tif"])
            .pick_files()
    }).await.map_err(|e| format!("Dialog error: {:?}", e))?;
    
    if let Some(paths) = file_paths {
        Ok(paths.into_iter().map(|p| p.to_string_lossy().to_string()).collect())
    } else {
        Ok(Vec::new())
    }
}

#[tauri::command]
pub async fn select_export_dir() -> Result<Option<String>, String> {
    let dir_path = tauri::async_runtime::spawn_blocking(|| {
        FileDialog::new().pick_folder()
    }).await.map_err(|e| format!("Dialog error: {:?}", e))?;
    
    Ok(dir_path.map(|p| p.to_string_lossy().to_string()))
}

#[tauri::command]
pub async fn open_dcp_dialog() -> Result<Option<String>, String> {
    let file_path = tauri::async_runtime::spawn_blocking(|| {
        FileDialog::new()
            .add_filter("DCP Profile / JSON Config", &["dcp", "json"])
            .pick_file()
    }).await.map_err(|e| format!("Dialog error: {:?}", e))?;
    
    Ok(file_path.map(|p| p.to_string_lossy().to_string()))
}

#[tauri::command]
pub async fn open_lut_dialog() -> Result<Option<String>, String> {
    let file_path = tauri::async_runtime::spawn_blocking(|| {
        FileDialog::new()
            .add_filter("3D LUT / JSON Config", &["cube", "json", "3dl"])
            .pick_file()
    }).await.map_err(|e| format!("Dialog error: {:?}", e))?;
    
    Ok(file_path.map(|p| p.to_string_lossy().to_string()))
}

#[tauri::command]
pub async fn get_builtin_luts() -> Result<Vec<String>, String> {
    let mut luts = Vec::new();
    if let Ok(entries) = std::fs::read_dir("assets/luts") {
        for entry in entries.filter_map(Result::ok) {
            if let Ok(file_type) = entry.file_type() {
                if file_type.is_file() {
                    let path = entry.path();
                    if path.extension().and_then(|e| e.to_str()) == Some("cube") {
                        if let Some(path_str) = path.to_str() {
                            luts.push(path_str.to_string());
                        }
                    }
                }
            }
        }
    }
    Ok(luts)
}

#[tauri::command]
pub async fn get_builtin_dcps() -> Result<Vec<String>, String> {
    let mut dcps = Vec::new();
    if let Ok(entries) = std::fs::read_dir("assets/dcps") {
        for entry in entries.filter_map(Result::ok) {
            if let Ok(file_type) = entry.file_type() {
                if file_type.is_file() {
                    let path = entry.path();
                    if path.extension().and_then(|e| e.to_str()) == Some("dcp") {
                        if let Some(path_str) = path.to_str() {
                            dcps.push(path_str.to_string());
                        }
                    }
                }
            }
        }
    }
    Ok(dcps)
}

pub fn load_image_buffer(path: &str, use_half_size: bool, dcp_profile: Option<&str>, colorspace: &str) -> Result<ImageBuffer<Rgb<u16>, Vec<u16>>, String> {
    if path.to_lowercase().ends_with(".tif") || path.to_lowercase().ends_with(".tiff") {
        let img = image::open(path).map(|i| i.into_rgb16()).map_err(|e| format!("TIFF读取失败: {:?}", e))?;
        if use_half_size {
            let (w, h) = img.dimensions();
            Ok(image::imageops::resize(&img, w / 2, h / 2, FilterType::Triangle))
        } else {
            Ok(img)
        }
    } else {
        let buf = std::fs::read(path).map_err(|e| format!("RAW文件读取失败: {:?}", e))?;
        unsafe {
            let data = libraw_sys::libraw_init(0);
            if data.is_null() {
                return Err("Failed to init libraw".to_string());
            }

            (*data).params.use_camera_wb = 1;
            (*data).params.use_camera_matrix = 1;
            (*data).params.output_color = if colorspace == "aces" { 6 } else { 1 }; // sRGB
            (*data).params.gamm[0] = 1.0;
            (*data).params.gamm[1] = 1.0;
            
            let mut _c_dcp: Option<std::ffi::CString> = None;
            if let Some(dcp) = dcp_profile {
                if let Ok(c_str) = std::ffi::CString::new(dcp) {
                    (*data).params.camera_profile = c_str.as_ptr() as *mut std::os::raw::c_char;
                    _c_dcp = Some(c_str);
                }
            }
            
            if use_half_size {
                (*data).params.half_size = 1;
            }

            if libraw_sys::libraw_open_buffer(data, buf.as_ptr() as *const _, buf.len()) != 0 {
                libraw_sys::libraw_close(data);
                return Err("Failed to open RAW buffer".to_string());
            }
            if libraw_sys::libraw_unpack(data) != 0 {
                libraw_sys::libraw_close(data);
                return Err("Failed to unpack RAW".to_string());
            }

            (*data).params.output_bps = 16;
            
            if libraw_sys::libraw_dcraw_process(data) != 0 {
                libraw_sys::libraw_close(data);
                return Err("Failed to process RAW".to_string());
            }

            let mut err = 0;
            let mem_image = libraw_sys::libraw_dcraw_make_mem_image(data, &mut err);
            if mem_image.is_null() {
                libraw_sys::libraw_close(data);
                return Err("Failed to create mem image".to_string());
            }

            let width = (*mem_image).width as u32;
            let height = (*mem_image).height as u32;
            let colors = (*mem_image).colors as usize;
            let data_len = (*mem_image).data_size as usize;
            
            let slice = std::slice::from_raw_parts((*mem_image).data.as_ptr() as *const u16, data_len / 2);
            
            let mut img_buffer = ImageBuffer::<Rgb<u16>, Vec<u16>>::new(width, height);
            for (i, pixel) in img_buffer.pixels_mut().enumerate() {
                let idx = i * colors;
                pixel[0] = slice.get(idx).copied().unwrap_or(0);
                pixel[1] = slice.get(idx + 1).copied().unwrap_or(0);
                pixel[2] = slice.get(idx + 2).copied().unwrap_or(0);
            }
            
            libraw_sys::libraw_dcraw_clear_mem(mem_image as *mut _);
            libraw_sys::libraw_close(data);

            Ok(img_buffer)
        }
    }
}

#[tauri::command]
pub async fn import_images(paths: Vec<String>, state: State<'_, EngineState>) -> Result<(), String> {
    if paths.is_empty() {
        return Ok(());
    }

    let dcp_profile = state.dcp_profile.read().unwrap().clone();
    let colorspace = state.working_colorspace.read().unwrap().clone();

    let new_items_result: Result<Vec<FilmItem>, String> = paths.into_par_iter().map(|path| {
        let img_buffer = load_image_buffer(&path, true, dcp_profile.as_deref(), &colorspace)?;

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
            geom: crate::app_state::GeometryState::default(),
        })
    }).collect();

    let new_items = new_items_result?;
    
    let mut order_guard = state.item_order.write().map_err(|e| e.to_string())?;
    for item in new_items {
        let id = item.id.clone();
        state.items.insert(id.clone(), Arc::new(RwLock::new(item)));
        order_guard.push(id);
    }

    if state.active_id.read().map_err(|e| e.to_string())?.is_none() {
        if let Some(first_id) = order_guard.first() {
            *state.active_id.write().map_err(|e| e.to_string())? = Some(first_id.clone());
        }
    }

    Ok(())
}

#[tauri::command]
pub async fn get_filmstrip(state: State<'_, EngineState>) -> Result<Vec<FilmstripItem>, String> {
    let item_order = state.item_order.read().map_err(|e| e.to_string())?;
    let mut strip = Vec::with_capacity(item_order.len());
    for id in item_order.iter() {
        if let Some(item_arc) = state.items.get(id) {
            let item = item_arc.read().map_err(|e| e.to_string())?;
            strip.push(FilmstripItem {
                id: item.id.clone(),
                file_path: item.file_path.clone(),
                thumbnail_base64: item.thumbnail_base64.clone(),
            });
        }
    }
    Ok(strip)
}

#[derive(Serialize)]
pub struct LutData {
    pub size: u32,
    pub data: Vec<u8>,
    pub is_1d: bool,
}

#[tauri::command]
pub async fn load_3d_lut(path: String) -> Result<LutData, String> {
    let content = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
    let mut size_3d = 0;
    let mut size_1d = 0;
    let mut data_floats: Vec<f32> = Vec::new();
    
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line.starts_with("LUT_3D_SIZE") {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() == 2 { size_3d = parts[1].parse().unwrap_or(0); }
        } else if line.starts_with("LUT_1D_SIZE") {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() == 2 { size_1d = parts[1].parse().unwrap_or(0); }
        } else if line.starts_with("DOMAIN_MIN") || line.starts_with("DOMAIN_MAX") || line.starts_with("TITLE") {
            continue;
        } else {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() == 3 {
                if let (Ok(r), Ok(g), Ok(b)) = (parts[0].parse::<f32>(), parts[1].parse::<f32>(), parts[2].parse::<f32>()) {
                    data_floats.push(r);
                    data_floats.push(g);
                    data_floats.push(b);
                }
            }
        }
    }
    
    if (size_3d == 0 && size_1d == 0) || data_floats.is_empty() {
        return Err("Invalid LUT file".into());
    }

    let mut max_val: f32 = 0.0;
    for &v in &data_floats {
        if v > max_val {
            max_val = v;
        }
    }
    if max_val > 1.0 {
        for v in &mut data_floats {
            *v /= 1023.0;
        }
    }
    
    let mut final_size = size_3d;
    let mut is_1d = false;

    if size_1d > 0 && size_3d == 0 {
        final_size = size_1d;
        is_1d = true;
    }
    
    // Force RGB data to RGBA (Alpha = 1.0)
    let mut rgba_floats = Vec::with_capacity((data_floats.len() / 3) * 4);
    for chunk in data_floats.chunks(3) {
        if chunk.len() == 3 {
            rgba_floats.push(chunk[0]);
            rgba_floats.push(chunk[1]);
            rgba_floats.push(chunk[2]);
            rgba_floats.push(1.0);
        }
    }
    
    let data_bytes = unsafe {
        std::slice::from_raw_parts(
            rgba_floats.as_ptr() as *const u8,
            rgba_floats.len() * std::mem::size_of::<f32>()
        )
    }.to_vec();
    
    Ok(LutData {
        size: final_size as u32,
        data: data_bytes,
        is_1d,
    })
}

#[tauri::command]
pub async fn load_dcp_profile(path: String, state: State<'_, EngineState>) -> Result<(), String> {
    *state.dcp_profile.write().unwrap() = Some(path.clone());
    if let Some(active_id) = state.active_id.read().unwrap().clone() {
        if let Some(item_arc) = state.items.get(&active_id) {
            let mut item = item_arc.write().unwrap();
            let colorspace = state.working_colorspace.read().unwrap().clone();
            if let Ok(img_buffer) = load_image_buffer(&item.file_path, true, Some(&path), &colorspace) {
                let (width, height) = img_buffer.dimensions();
                let ratio_proxy = 800.0 / (width.max(height) as f32);
                let proxy_width = (width as f32 * ratio_proxy).max(1.0) as u32;
                let proxy_height = (height as f32 * ratio_proxy).max(1.0) as u32;
                let proxy = image::imageops::resize(&img_buffer, proxy_width, proxy_height, FilterType::Triangle);
                
                item.base_color = compute_auto_base(&proxy);
                item.pristine_proxy = compute_pristine_proxy(&proxy, &item.base_color, item.params.film_mode.clone());
                item.original_proxy = proxy.clone();
                item.proxy_image = proxy;
            }
        }
    }
    Ok(())
}

#[tauri::command]
pub async fn set_working_colorspace(colorspace: String, state: State<'_, EngineState>) -> Result<(), String> {
    *state.working_colorspace.write().unwrap() = colorspace.clone();
    if let Some(active_id) = state.active_id.read().unwrap().clone() {
        if let Some(item_arc) = state.items.get(&active_id) {
            let mut item = item_arc.write().unwrap();
            let dcp = state.dcp_profile.read().unwrap().clone();
            if let Ok(img_buffer) = load_image_buffer(&item.file_path, true, dcp.as_deref(), &colorspace) {
                let (width, height) = img_buffer.dimensions();
                let ratio_proxy = 800.0 / (width.max(height) as f32);
                let proxy_width = (width as f32 * ratio_proxy).max(1.0) as u32;
                let proxy_height = (height as f32 * ratio_proxy).max(1.0) as u32;
                let proxy = image::imageops::resize(&img_buffer, proxy_width, proxy_height, FilterType::Triangle);
                
                item.base_color = compute_auto_base(&proxy);
                item.pristine_proxy = compute_pristine_proxy(&proxy, &item.base_color, item.params.film_mode.clone());
                item.original_proxy = proxy.clone();
                item.proxy_image = proxy;
            }
        }
    }
    Ok(())
}

#[derive(serde::Serialize)]
pub struct ActiveImageState {
    pub params: TuningParams,
    pub geom: crate::app_state::GeometryState,
}

#[tauri::command]
pub async fn switch_active_image(id: String, state: State<'_, EngineState>) -> Result<ActiveImageState, String> {
    if let Some(item_arc) = state.items.get(&id) {
        *state.active_id.write().map_err(|e| e.to_string())? = Some(id.clone());
        let item = item_arc.read().map_err(|e| e.to_string())?;
        Ok(ActiveImageState {
            params: item.params.clone(),
            geom: item.geom.clone(),
        })
    } else {
        Err("Image ID not found".into())
    }
}

#[tauri::command]
pub async fn set_film_mode(id: String, mode: String, state: State<'_, EngineState>) -> Result<(), String> {
    if let Some(item_arc) = state.items.get(&id) {
        let mut item = item_arc.write().map_err(|e| e.to_string())?;
        let new_mode = if mode == "B&W" { FilmMode::BW } else { FilmMode::Color };
        if item.params.film_mode != new_mode {
            item.params.film_mode = new_mode.clone();
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
    if let Some(item_arc) = state.items.get(&id) {
        {
            let item = item_arc.read().map_err(|e| e.to_string())?;
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
            
            let (orig_width, orig_height) = (width, height);
            let cx = (item.geom.crop_rect.x * orig_width as f32).max(0.0).min(orig_width as f32) as u32;
            let cy = (item.geom.crop_rect.y * orig_height as f32).max(0.0).min(orig_height as f32) as u32;
            let cw = (item.geom.crop_rect.width * orig_width as f32).max(1.0).min((orig_width - cx) as f32) as u32;
            let ch = (item.geom.crop_rect.height * orig_height as f32).max(1.0).min((orig_height - cy) as f32) as u32;
            
            let mut cropped_thumb = thumb_8bit;
            if cw < orig_width || ch < orig_height {
                cropped_thumb = image::imageops::crop(&mut cropped_thumb, cx, cy, cw, ch).to_image();
            }

            let ratio_thumb = 120.0 / (cw.max(ch) as f32);
            let thumb_width = (cw as f32 * ratio_thumb).max(1.0) as u32;
            let thumb_height = (ch as f32 * ratio_thumb).max(1.0) as u32;
            let thumb = image::imageops::resize(&cropped_thumb, thumb_width, thumb_height, FilterType::Triangle);
            
            let mut cursor = Cursor::new(Vec::new());
            thumb.write_to(&mut cursor, ImageOutputFormat::Jpeg(70)).map_err(|e| e.to_string())?;
            new_thumbnail = general_purpose::STANDARD.encode(cursor.into_inner());
        }
        if !new_thumbnail.is_empty() {
            let mut item = item_arc.write().map_err(|e| e.to_string())?;
            item.thumbnail_base64 = new_thumbnail;
        }
    }
    Ok(())
}

#[tauri::command]
pub async fn update_geometry(id: String, geom: crate::app_state::GeometryState, state: State<'_, EngineState>) -> Result<(), String> {
    if let Some(item_arc) = state.items.get(&id) {
        let mut item = item_arc.write().map_err(|e| e.to_string())?;
        item.geom = geom;
        reapply_geometry(&mut item);
    }
    Ok(())
}

fn reapply_geometry(item: &mut FilmItem) {
    let mut current = item.original_proxy.clone();
    
    if item.geom.angle.abs() > 0.01 {
        let angle_rad = item.geom.angle.to_radians();
        let (w, h) = current.dimensions();
        
        let cos_a = angle_rad.cos();
        let sin_a = angle_rad.sin();
        
        let new_w = (w as f32 * cos_a.abs() + h as f32 * sin_a.abs()).ceil() as u32;
        let new_h = (w as f32 * sin_a.abs() + h as f32 * cos_a.abs()).ceil() as u32;
        
        let diag = ((w as f32).hypot(h as f32)).ceil() as u32;
        let mut expanded = ImageBuffer::from_pixel(diag, diag, image::Rgb([0, 0, 0]));
        let offset_x = (diag as i64 - w as i64) / 2;
        let offset_y = (diag as i64 - h as i64) / 2;
        image::imageops::overlay(&mut expanded, &current, offset_x, offset_y);
        
        let rotated = imageproc::geometric_transformations::rotate_about_center(
            &expanded,
            angle_rad,
            imageproc::geometric_transformations::Interpolation::Bicubic,
            image::Rgb([0, 0, 0]),
        );
        
        let crop_x = (diag.saturating_sub(new_w)) / 2;
        let crop_y = (diag.saturating_sub(new_h)) / 2;
        current = image::imageops::crop_imm(&rotated, crop_x, crop_y, new_w, new_h).to_image();
    }
    
    match item.geom.rotate_90_count.rem_euclid(4) {
        1 => current = image::imageops::rotate90(&current),
        2 => current = image::imageops::rotate180(&current),
        3 => current = image::imageops::rotate270(&current),
        _ => {}
    }
    
    if item.geom.flip_h {
        current = image::imageops::flip_horizontal(&current);
    }
    if item.geom.flip_v {
        current = image::imageops::flip_vertical(&current);
    }
    
    item.proxy_image = current;
    item.pristine_proxy = compute_pristine_proxy(&item.proxy_image, &item.base_color, item.params.film_mode.clone());
}

#[tauri::command]
pub async fn geometry_auto_align(id: String, state: State<'_, EngineState>) -> Result<crate::app_state::AutoAlignResult, String> {
    if let Some(item_arc) = state.items.get(&id) {
        let mut item = item_arc.write().map_err(|e| e.to_string())?;
        let first_result = geometry::auto_crop_rect(&item.original_proxy)?;
        item.geom.angle = first_result.angle;
        
        reapply_geometry(&mut item);
        
        let second_result = geometry::auto_crop_rect(&item.proxy_image)?;
        item.geom.crop_rect = second_result.crop_rect.clone();
        
        return Ok(crate::app_state::AutoAlignResult {
            crop_rect: item.geom.crop_rect.clone(),
            angle: item.geom.angle,
        });
    }
    Err("Image not found".to_string())
}

#[tauri::command]
pub async fn get_proxy_image_data(
    id: String,
    state: State<'_, EngineState>,
) -> Result<tauri::ipc::Response, String> {
    let item_arc = state.items.get(&id).ok_or("Image ID not found")?;
    let item = item_arc.read().map_err(|e| e.to_string())?;
    
    let proxy = &item.proxy_image;
    let (width, height) = proxy.dimensions();
    let base_color = &item.base_color;
    
    // Calculate base_density
    let epsilon = 1e-6_f32;
    let t_r = (base_color.base_r as f32 / 65535.0).max(epsilon);
    let t_g = (base_color.base_g as f32 / 65535.0).max(epsilon);
    let t_b = (base_color.base_b as f32 / 65535.0).max(epsilon);
    let bd_r: f32 = -t_r.log10();
    let bd_g: f32 = -t_g.log10();
    let bd_b: f32 = -t_b.log10();

    // Header: width(u32), height(u32), bd_r(f32), bd_g(f32), bd_b(f32) => 20 bytes
    let mut out_buffer = vec![0u8; (width * height * 8) as usize + 20];
    out_buffer[0..4].copy_from_slice(&width.to_le_bytes());
    out_buffer[4..8].copy_from_slice(&height.to_le_bytes());
    out_buffer[8..12].copy_from_slice(&bd_r.to_le_bytes());
    out_buffer[12..16].copy_from_slice(&bd_g.to_le_bytes());
    out_buffer[16..20].copy_from_slice(&bd_b.to_le_bytes());
    
    let raw_pixels: &[u16] = proxy.as_raw().as_slice();
    let out_slice = &mut out_buffer[20..];
    
    raw_pixels.par_chunks(3).zip(out_slice.par_chunks_mut(8)).for_each(|(chunk, out_chunk)| {
        out_chunk[0..2].copy_from_slice(&chunk[0].to_le_bytes());
        out_chunk[2..4].copy_from_slice(&chunk[1].to_le_bytes());
        out_chunk[4..6].copy_from_slice(&chunk[2].to_le_bytes());
        out_chunk[6..8].copy_from_slice(&65535u16.to_le_bytes()); // Alpha
    });

    Ok(tauri::ipc::Response::new(out_buffer))
}

#[tauri::command]
pub async fn update_tuning_parameters(
    id: String,
    params: TuningParams,
    state: State<'_, EngineState>,
) -> Result<(), String> {
    if let Some(item_arc) = state.items.get(&id) {
        let mut item = item_arc.write().map_err(|e| e.to_string())?;
        item.params = params;
    }
    Ok(())
}

#[tauri::command]
pub async fn batch_export_images(
    output_dir: String,
    format: String,
    color_space: String,
    state: State<'_, EngineState>,
) -> Result<usize, String> {
    let item_order = state.item_order.read().map_err(|e| e.to_string())?;
    let count = item_order.len();
    if count == 0 {
        return Ok(0);
    }

    let success_count = std::sync::atomic::AtomicUsize::new(0);

    let dcp_profile = state.dcp_profile.read().unwrap().clone();
    let working_colorspace = state.working_colorspace.read().unwrap().clone();

    item_order.par_iter().for_each(|id| {
        if let Some(item_arc) = state.items.get(id) {
            let item = item_arc.read().unwrap();
            if let Ok(original) = load_image_buffer(&item.file_path, false, dcp_profile.as_deref(), &working_colorspace) {
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
                
                if item.geom.angle.abs() > 0.01 {
                    let angle_rad = item.geom.angle.to_radians();
                    let (w, h) = transformed.dimensions();
                    
                    let cos_a = angle_rad.cos();
                    let sin_a = angle_rad.sin();
                    
                    let new_w = (w as f32 * cos_a.abs() + h as f32 * sin_a.abs()).ceil() as u32;
                    let new_h = (w as f32 * sin_a.abs() + h as f32 * cos_a.abs()).ceil() as u32;
                    
                    let diag = ((w as f32).hypot(h as f32)).ceil() as u32;
                    let mut expanded = ImageBuffer::from_pixel(diag, diag, image::Rgb([0, 0, 0]));
                    let offset_x = (diag as i64 - w as i64) / 2;
                    let offset_y = (diag as i64 - h as i64) / 2;
                    image::imageops::overlay(&mut expanded, &transformed, offset_x, offset_y);
                    
                    let rotated = imageproc::geometric_transformations::rotate_about_center(
                        &expanded,
                        angle_rad,
                        imageproc::geometric_transformations::Interpolation::Bicubic,
                        image::Rgb([0, 0, 0]),
                    );
                    
                    let crop_x = (diag.saturating_sub(new_w)) / 2;
                    let crop_y = (diag.saturating_sub(new_h)) / 2;
                    transformed = image::imageops::crop_imm(&rotated, crop_x, crop_y, new_w, new_h).to_image();
                }

                match item.geom.rotate_90_count.rem_euclid(4) {
                    1 => transformed = image::imageops::rotate90(&transformed),
                    2 => transformed = image::imageops::rotate180(&transformed),
                    3 => transformed = image::imageops::rotate270(&transformed),
                    _ => {}
                }
                
                if item.geom.flip_h {
                    transformed = image::imageops::flip_horizontal(&transformed);
                }
                if item.geom.flip_v {
                    transformed = image::imageops::flip_vertical(&transformed);
                }

                let (orig_width, orig_height) = transformed.dimensions();
                let cx = (item.geom.crop_rect.x * orig_width as f32).max(0.0).min(orig_width as f32) as u32;
                let cy = (item.geom.crop_rect.y * orig_height as f32).max(0.0).min(orig_height as f32) as u32;
                let cw = (item.geom.crop_rect.width * orig_width as f32).max(1.0).min((orig_width - cx) as f32) as u32;
                let ch = (item.geom.crop_rect.height * orig_height as f32).max(1.0).min((orig_height - cy) as f32) as u32;
                
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

                let file_stem = std::path::Path::new(&item.file_path)
                    .file_stem()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_string();
                
                // Color space info is ignored by basic `image` crate save unless embedding ICC,
                // but we name the file appropriately to acknowledge it
                let cs_suffix = match color_space.as_str() {
                    "adobergb" => "AdobeRGB",
                    "rec2020" => "Rec2020",
                    "prophoto" => "ProPhoto",
                    "aces" => "ACES-AP1",
                    _ => "sRGB"
                };

                let out_path = match format.as_str() {
                    "jpeg100" => {
                        // JPEG is 8-bit, we must convert
                        let mut out8 = ImageBuffer::<Rgb<u8>, Vec<u8>>::new(width, height);
                        for (in_p, out_p) in out_buffer.pixels().zip(out8.pixels_mut()) {
                            out_p[0] = (in_p[0] >> 8) as u8;
                            out_p[1] = (in_p[1] >> 8) as u8;
                            out_p[2] = (in_p[2] >> 8) as u8;
                        }
                        let path = std::path::Path::new(&output_dir).join(format!("nexfilm_{}_{}.jpg", file_stem, cs_suffix));
                        out8.save(&path).map(|_| path)
                    },
                    "png" => {
                        let path = std::path::Path::new(&output_dir).join(format!("nexfilm_{}_{}.png", file_stem, cs_suffix));
                        out_buffer.save(&path).map(|_| path)
                    },
                    "tiff8" => {
                        let mut out8 = ImageBuffer::<Rgb<u8>, Vec<u8>>::new(width, height);
                        for (in_p, out_p) in out_buffer.pixels().zip(out8.pixels_mut()) {
                            out_p[0] = (in_p[0] >> 8) as u8;
                            out_p[1] = (in_p[1] >> 8) as u8;
                            out_p[2] = (in_p[2] >> 8) as u8;
                        }
                        let path = std::path::Path::new(&output_dir).join(format!("nexfilm_{}_{}_8bit.tiff", file_stem, cs_suffix));
                        out8.save(&path).map(|_| path)
                    },
                    _ => {
                        // tiff16_uncompressed or tiff16_lzw
                        let path = std::path::Path::new(&output_dir).join(format!("nexfilm_{}_{}_16bit.tiff", file_stem, cs_suffix));
                        out_buffer.save(&path).map(|_| path)
                    }
                };
                
                if out_path.is_ok() {
                    success_count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                }
            }
        }
    });

    Ok(success_count.into_inner())
}
