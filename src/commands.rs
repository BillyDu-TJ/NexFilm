use crate::app_state::{BaseColor, EngineState, FilmItem, FilmstripItem, TuningParams, FilmMode, Roll};
use serde::Serialize;
use crate::pipeline::FilmPipeline;

use base64::{engine::general_purpose, Engine as _};
use image::{imageops::FilterType, ImageBuffer, ImageOutputFormat, Rgb, RgbImage, GenericImageView};
use rayon::prelude::*;
use tauri::Emitter;
use rfd::FileDialog;
use std::io::Cursor;
use tauri::State;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, RwLock};
use serde_json::Value;

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
                    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
                    if ext == "cube" || ext == "json" {
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
pub async fn import_images(paths: Vec<String>, is_loose: Option<bool>, in_library: Option<bool>, roll_id: Option<String>, is_historical: Option<bool>, state: State<'_, EngineState>) -> Result<(), String> {
    if paths.is_empty() {
        return Ok(());
    }

    let _dcp_profile = state.dcp_profile.read().unwrap().clone();
    let _colorspace = state.working_colorspace.read().unwrap().clone();

    let mut new_items = Vec::new();
    let existing_paths: std::collections::HashSet<String> = {
        let guard = state.items.clone();
        guard.iter().map(|kv| kv.value().read().unwrap().file_path.replace("\\", "/").to_lowercase()).collect()
    };
    
    let paths_to_process: Vec<String> = paths.clone().into_iter().filter(|p| !existing_paths.contains(&p.replace("\\", "/").to_lowercase())).collect();

    for chunk in paths_to_process.chunks(4) {
        let chunk_items_result: Result<Vec<FilmItem>, String> = chunk.into_par_iter().map(|path| {
            let loose = is_loose.unwrap_or(false);
            let active_roll = roll_id.clone().unwrap_or_else(|| "LOOSE_DEFAULT".to_string());
            if !loose {
                if let Some((thumb, params, geom, base_color)) = load_image_state_from_db(&active_roll, path) {
                    let id = format!("img_{}", NEXT_ID.fetch_add(1, Ordering::SeqCst));
                    return Ok(FilmItem {
                        id,
                        roll_id: active_roll,
                        file_path: path.clone(),
                        thumbnail_base64: thumb,
                        original_proxy: None,
                        proxy_image: None,
                        pristine_proxy: None,
                        base_color,
                        params,
                        geom,
                        is_loose: loose,
                        in_library: in_library.unwrap_or(true),
                    });
                }
                if is_historical.unwrap_or(false) {
                    return Err(format!("HISTORICAL_MISS_DB: No database state found for roll {}, path: {}", active_roll, path));
                }
            }

            let mut thumbnail_base64 = String::new();
            if path.to_lowercase().ends_with(".tif") || path.to_lowercase().ends_with(".tiff") {
                if let Ok(img) = image::open(path) {
                    let (w, h) = img.dimensions();
                    let ratio = 256.0 / (w.max(h) as f32);
                    let new_w = (w as f32 * ratio).max(1.0) as u32;
                    let new_h = (h as f32 * ratio).max(1.0) as u32;
                    let thumb = image::imageops::resize(&img, new_w, new_h, FilterType::Nearest);
                    let mut cursor = Cursor::new(Vec::new());
                    if thumb.write_to(&mut cursor, ImageOutputFormat::Jpeg(70)).is_ok() {
                        thumbnail_base64 = general_purpose::STANDARD.encode(cursor.into_inner());
                    }
                }
            } else {
                unsafe {
                    let data = libraw_sys::libraw_init(0);
                    if !data.is_null() {
                        let c_path = std::ffi::CString::new(path.as_str()).unwrap_or_default();
                        let mut opened = libraw_sys::libraw_open_file(data, c_path.as_ptr()) == 0;
                        
                        let mut _buf = Vec::new();
                        if !opened {
                            if let Ok(b) = std::fs::read(path) {
                                _buf = b;
                                opened = libraw_sys::libraw_open_buffer(data, _buf.as_ptr() as *const _, _buf.len()) == 0;
                            }
                        }

                        if opened {
                            if libraw_sys::libraw_unpack_thumb(data) == 0 {
                                let mut err = 0;
                                let thumb = libraw_sys::libraw_dcraw_make_mem_thumb(data, &mut err);
                                if !thumb.is_null() {
                                    let thumb_type = (*thumb).type_;
                                    let thumb_len = (*thumb).data_size as usize;
                                    let thumb_data = std::slice::from_raw_parts((*thumb).data.as_ptr(), thumb_len);
                                    if thumb_type == 1 { // JPEG
                                        thumbnail_base64 = general_purpose::STANDARD.encode(thumb_data);
                                    } else {
                                        if let Ok(img) = image::load_from_memory(thumb_data) {
                                            let mut cursor = Cursor::new(Vec::new());
                                            if img.write_to(&mut cursor, ImageOutputFormat::Jpeg(70)).is_ok() {
                                                thumbnail_base64 = general_purpose::STANDARD.encode(cursor.into_inner());
                                            }
                                        }
                                    }
                                    libraw_sys::libraw_dcraw_clear_mem(thumb as *mut _);
                                }
                            }
                        }
                        libraw_sys::libraw_close(data);
                    }
                }
            }

            if thumbnail_base64.is_empty() {
                thumbnail_base64 = "FILE_MISSING".to_string();
            }

            let base_color = BaseColor { base_r: 32768, base_g: 32768, base_b: 32768 };

            let id = format!("img_{}", NEXT_ID.fetch_add(1, Ordering::SeqCst));

            let item = FilmItem {
                id,
                roll_id: active_roll,
                file_path: path.clone(),
                thumbnail_base64,
                original_proxy: None,
                proxy_image: None,
                pristine_proxy: None,
                base_color,
                params: TuningParams::default(),
                geom: crate::app_state::GeometryState::default(),
                is_loose: loose,
                in_library: in_library.unwrap_or(true),
            };
            
            if !loose {
                let _ = save_image_state_to_db(&item);
            }
            
            Ok(item)
        }).collect();
        new_items.extend(chunk_items_result?);
    }

    for item in new_items {
        let id = item.id.clone();
        state.items.insert(id.clone(), Arc::new(RwLock::new(item)));
    }

    let loose = is_loose.unwrap_or(false);
    if !loose {
        let mut order_guard = state.item_order.write().map_err(|e| e.to_string())?;
        for path in paths {
            let id_opt = {
                let guard = state.items.clone();
                let mut found = None;
                for kv in guard.iter() {
                    let db_path = kv.value().read().unwrap().file_path.clone();
                    if db_path == *path || db_path.replace("\\", "/").to_lowercase() == path.replace("\\", "/").to_lowercase() {
                        found = Some(kv.key().clone());
                        break;
                    }
                }
                found
            };
            if let Some(id) = id_opt {
                if !order_guard.contains(&id) {
                    order_guard.push(id);
                }
            }
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
            if item.in_library {
                strip.push(FilmstripItem {
                    id: item.id.clone(),
                    file_path: item.file_path.clone(),
                    thumbnail_base64: if std::fs::File::open(&item.file_path).is_ok() { item.thumbnail_base64.clone() } else { "FILE_MISSING".to_string() },
                });
            }
        }
    }
    Ok(strip)
}

#[tauri::command]
pub async fn get_roll_filmstrip(roll_id: String, state: State<'_, EngineState>) -> Result<Vec<FilmstripItem>, String> {
    let rolls = state.rolls.read().unwrap();
    if let Some(roll) = rolls.iter().find(|r| r.roll_id == roll_id) {
        let mut strip = Vec::with_capacity(roll.image_paths.len());
        let guard = state.items.clone();
        for path in &roll.image_paths {
            for kv in guard.iter() {
                let item = kv.value().read().unwrap();
                let db_path = item.file_path.clone();
                if db_path == *path || db_path.replace("\\", "/").to_lowercase() == path.replace("\\", "/").to_lowercase() {
                    strip.push(FilmstripItem {
                        id: item.id.clone(),
                        file_path: item.file_path.clone(),
                        thumbnail_base64: if std::fs::File::open(&item.file_path).is_ok() { item.thumbnail_base64.clone() } else { "FILE_MISSING".to_string() },
                    });
                    break;
                }
            }
        }
        return Ok(strip);
    }
    Ok(Vec::new())
}

#[derive(Serialize)]
pub struct LutData {
    pub size: u32,
    pub data: Vec<u8>,
    pub is_1d: bool,
}

fn extract_points(v: &Value, channel: &str) -> Vec<[f32; 2]> {
    let mut points = Vec::new();
    let mut target = &Value::Null;
    let channel_upper = channel.to_uppercase();
    let channel_lower = channel.to_lowercase();

    macro_rules! find_channel {
        ($obj:expr) => {
            $obj.get(&channel_upper).or_else(|| $obj.get(&channel_lower))
        };
    }

    if let Some(t1) = find_channel!(v) {
        target = t1;
    } else if let Some(curves) = v.get("curves") {
        if let Some(t2) = find_channel!(curves) {
            target = t2;
        }
    } else if let Some(points_obj) = v.get("points") {
        if let Some(t3) = find_channel!(points_obj) {
            target = t3;
        }
    } else if let Some(cc) = v.get("cc_params") {
        if let Some(dc) = cc.get("density_curve") {
            if let Some(pts) = dc.get("points") {
                if let Some(t4) = find_channel!(pts) {
                    target = t4;
                }
            }
        }
    }

    if let Some(arr) = target.as_array() {
        for item in arr {
            if let Some(pair) = item.as_array() {
                if pair.len() >= 2 {
                    if let (Some(x), Some(y)) = (pair[0].as_f64(), pair[1].as_f64()) {
                        points.push([x as f32, y as f32]);
                    }
                }
            }
        }
    }
    points
}

fn interpolate(x: f32, points: &[[f32; 2]]) -> f32 {
    if points.is_empty() { return x; }
    if x <= points[0][0] { return points[0][1]; }
    if x >= points[points.len() - 1][0] { return points[points.len() - 1][1]; }
    for i in 0..points.len() - 1 {
        let p0 = points[i];
        let p1 = points[i + 1];
        if x >= p0[0] && x <= p1[0] {
            let mut t = 0.0;
            if p1[0] - p0[0] > 1e-6 {
                t = (x - p0[0]) / (p1[0] - p0[0]);
            }
            return p0[1] + t * (p1[1] - p0[1]);
        }
    }
    x
}

#[tauri::command]
pub async fn load_3d_lut(path: String) -> Result<LutData, String> {
    let content = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;

    if path.to_lowercase().ends_with(".json") {
        let v: Value = serde_json::from_str(&content).map_err(|e| format!("Invalid JSON: {}", e))?;
        let mut r_points = extract_points(&v, "r");
        let mut g_points = extract_points(&v, "g");
        let mut b_points = extract_points(&v, "b");
        let rgb_points = extract_points(&v, "rgb");
        
        if r_points.is_empty() { r_points = rgb_points.clone(); }
        if g_points.is_empty() { g_points = rgb_points.clone(); }
        if b_points.is_empty() { b_points = rgb_points.clone(); }

        if r_points.is_empty() {
            return Err("No valid curve points found in JSON".to_string());
        }

        r_points.sort_by(|a, b| a[0].partial_cmp(&b[0]).unwrap());
        g_points.sort_by(|a, b| a[0].partial_cmp(&b[0]).unwrap());
        b_points.sort_by(|a, b| a[0].partial_cmp(&b[0]).unwrap());

        let size = 1024;
        let mut data_floats: Vec<f32> = Vec::with_capacity(size * 4);
        for i in 0..size {
            let x = i as f32 / (size - 1) as f32;
            let r_val = interpolate(x, &r_points);
            let g_val = interpolate(x, &g_points);
            let b_val = interpolate(x, &b_points);
            data_floats.push(r_val);
            data_floats.push(g_val);
            data_floats.push(b_val);
            data_floats.push(1.0); // Alpha
        }

        let data_bytes = unsafe {
            std::slice::from_raw_parts(
                data_floats.as_ptr() as *const u8,
                data_floats.len() * std::mem::size_of::<f32>()
            )
        }.to_vec();

        return Ok(LutData {
            size: size as u32,
            data: data_bytes,
            is_1d: true,
        });
    }

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
pub async fn get_roll_previews(roll_id: String, state: State<'_, EngineState>) -> Result<Vec<String>, String> {
    let rolls = state.rolls.read().unwrap();
    if let Some(roll) = rolls.iter().find(|r| r.roll_id == roll_id) {
        let mut previews = Vec::new();
        let guard = state.items.clone();
        for path in roll.image_paths.iter().take(3) {
            for kv in guard.iter() {
                let db_path = kv.value().read().unwrap().file_path.clone();
                if db_path == *path || db_path.replace("\\", "/").to_lowercase() == path.replace("\\", "/").to_lowercase() {
                    let thumb = kv.value().read().unwrap().thumbnail_base64.clone();
                    if thumb != "FILE_MISSING" && !thumb.is_empty() {
                        previews.push(thumb);
                    }
                    break;
                }
            }
        }
        return Ok(previews);
    }
    Ok(Vec::new())
}

#[tauri::command]
pub async fn get_raw_thumbnails(paths: Vec<String>) -> Result<Vec<String>, String> {
    use base64::{Engine as _, engine::general_purpose};
    use std::io::Cursor;
    use image::ImageOutputFormat;
    
    let mut thumbs = Vec::with_capacity(paths.len());
    for path in paths {
        let mut thumbnail_base64 = String::from("FILE_MISSING");
        unsafe {
            let data = libraw_sys::libraw_init(0);
            if !data.is_null() {
                let c_path = std::ffi::CString::new(path.clone()).unwrap_or_default();
                let mut opened = libraw_sys::libraw_open_file(data, c_path.as_ptr()) == 0;
                
                let mut _buf = Vec::new();
                if !opened {
                    if let Ok(b) = std::fs::read(&path) {
                        _buf = b;
                        opened = libraw_sys::libraw_open_buffer(data, _buf.as_ptr() as *const _, _buf.len()) == 0;
                    }
                }

                if opened {
                    if libraw_sys::libraw_unpack_thumb(data) == 0 {
                        let mut err = 0;
                        let thumb = libraw_sys::libraw_dcraw_make_mem_thumb(data, &mut err);
                        if !thumb.is_null() {
                            let thumb_type = (*thumb).type_;
                            let thumb_len = (*thumb).data_size as usize;
                            let thumb_data = std::slice::from_raw_parts((*thumb).data.as_ptr(), thumb_len);
                            if thumb_type == 1 { // JPEG
                                thumbnail_base64 = general_purpose::STANDARD.encode(thumb_data);
                            } else {
                                if let Ok(img) = image::load_from_memory(thumb_data) {
                                    let mut cursor = Cursor::new(Vec::new());
                                    if img.write_to(&mut cursor, ImageOutputFormat::Jpeg(70)).is_ok() {
                                        thumbnail_base64 = general_purpose::STANDARD.encode(cursor.into_inner());
                                    }
                                }
                            }
                            libraw_sys::libraw_dcraw_clear_mem(thumb as *mut _);
                        }
                    }
                }
                libraw_sys::libraw_close(data);
            }
        }
        thumbs.push(thumbnail_base64);
    }
    Ok(thumbs)
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
                let ratio_proxy = 2048.0 / (width.max(height) as f32);
                let proxy_width = (width as f32 * ratio_proxy).max(1.0) as u32;
                let proxy_height = (height as f32 * ratio_proxy).max(1.0) as u32;
                let proxy = image::imageops::resize(&img_buffer, proxy_width, proxy_height, FilterType::Triangle);
                
                item.base_color = compute_auto_base(&proxy);
                item.original_proxy = Some(proxy.clone());
                item.proxy_image = Some(proxy);
                reapply_geometry(&mut item);
                item.pristine_proxy = Some(compute_pristine_proxy(item.proxy_image.as_ref().unwrap(), &item.base_color, item.params.film_mode.clone()));
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
                let ratio_proxy = 2048.0 / (width.max(height) as f32);
                let proxy_width = (width as f32 * ratio_proxy).max(1.0) as u32;
                let proxy_height = (height as f32 * ratio_proxy).max(1.0) as u32;
                let proxy = image::imageops::resize(&img_buffer, proxy_width, proxy_height, FilterType::Triangle);
                
                item.base_color = compute_auto_base(&proxy);
                item.original_proxy = Some(proxy.clone());
                item.proxy_image = Some(proxy);
                reapply_geometry(&mut item);
                item.pristine_proxy = Some(compute_pristine_proxy(item.proxy_image.as_ref().unwrap(), &item.base_color, item.params.film_mode.clone()));
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
pub async fn switch_active_image(id: String, roll_id: String, state: State<'_, EngineState>) -> Result<ActiveImageState, String> {
    let _ = roll_id; // Unused, but required for correct JSON payload mapping from JS
    let needs_load = {
        let item_arc = state.items.get(&id).ok_or("Image ID not found")?;
        let item = item_arc.read().map_err(|e| e.to_string())?;
        if std::fs::File::open(&item.file_path).is_err() {
            return Err("FILE_MISSING".into());
        }
        item.proxy_image.is_none()
    };
    
    if needs_load {
        let item_arc = state.items.get(&id).unwrap();
        let mut item = item_arc.write().map_err(|e| e.to_string())?;
        if item.proxy_image.is_none() {
            let dcp = state.dcp_profile.read().unwrap().clone();
            let colorspace = state.working_colorspace.read().unwrap().clone();
            
            let img_buffer = load_image_buffer(&item.file_path, true, dcp.as_deref(), &colorspace)?;
            let (width, height) = img_buffer.dimensions();
            let ratio_proxy = 2048.0 / (width.max(height) as f32);
            let proxy_width = (width as f32 * ratio_proxy).max(1.0) as u32;
            let proxy_height = (height as f32 * ratio_proxy).max(1.0) as u32;
            let proxy = image::imageops::resize(&img_buffer, proxy_width, proxy_height, FilterType::Triangle);
            
            if item.base_color.base_r == 32768 && item.base_color.base_g == 32768 && item.base_color.base_b == 32768 {
                item.base_color = compute_auto_base(&proxy);
            }
            item.original_proxy = Some(proxy.clone());
            item.proxy_image = Some(proxy.clone());
            let pristine = compute_pristine_proxy(&proxy, &item.base_color, item.params.film_mode.clone());
            item.pristine_proxy = Some(pristine);
        }
    }
    
    let item_arc = state.items.get(&id).unwrap();
    let item = item_arc.read().map_err(|e| e.to_string())?;
    *state.active_id.write().map_err(|e| e.to_string())? = Some(id.clone());
    Ok(ActiveImageState {
        params: item.params.clone(),
        geom: item.geom.clone(),
    })
}

#[tauri::command]
pub async fn start_precache(ids: Vec<String>, state: State<'_, EngineState>, app: tauri::AppHandle) -> Result<(), String> {
    let mut items_to_cache = Vec::new();
    for id in ids {
        if let Some(arc) = state.items.get(&id) {
            items_to_cache.push(arc.clone());
        }
    }
    
    let dcp = state.dcp_profile.read().unwrap().clone();
    let colorspace = state.working_colorspace.read().unwrap().clone();
    
    std::thread::spawn(move || {
        let total = items_to_cache.len();
        let _ = app.emit("precache_progress", serde_json::json!({ "total": total, "current": 0 }));
        for (i, item_arc) in items_to_cache.into_iter().enumerate() {
            let needs_load = {
                let item = item_arc.read().unwrap();
                item.proxy_image.is_none()
            };
            
            if needs_load {
                let file_path = { item_arc.read().unwrap().file_path.clone() };
                if let Ok(img_buffer) = load_image_buffer(&file_path, true, dcp.as_deref(), &colorspace) {
                    let (width, height) = img_buffer.dimensions();
                    let ratio_proxy = 2048.0 / (width.max(height) as f32);
                    let proxy_width = (width as f32 * ratio_proxy).max(1.0) as u32;
                    let proxy_height = (height as f32 * ratio_proxy).max(1.0) as u32;
                    let proxy = image::imageops::resize(&img_buffer, proxy_width, proxy_height, FilterType::Triangle);
                    
                    let mut item = item_arc.write().unwrap();
                    if item.base_color.base_r == 32768 && item.base_color.base_g == 32768 && item.base_color.base_b == 32768 {
                        item.base_color = compute_auto_base(&proxy);
                    }
                    item.original_proxy = Some(proxy.clone());
                    item.proxy_image = Some(proxy.clone());
                    reapply_geometry(&mut item);
                    let pristine = compute_pristine_proxy(item.proxy_image.as_ref().unwrap(), &item.base_color, item.params.film_mode.clone());
                    item.pristine_proxy = Some(pristine);
                    if let Some(new_thumb) = generate_processed_thumbnail(&item) {
                        item.thumbnail_base64 = new_thumb.clone();
                        let _ = app.emit("thumbnail_updated", serde_json::json!({ "id": item.id, "thumbnail": new_thumb }));
                    }
                    if !item.is_loose {
                        let _ = save_image_state_to_db(&item);
                    }
                }
            }
            let _ = app.emit("precache_progress", serde_json::json!({ "total": total, "current": i + 1 }));
        }
    });

    Ok(())
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
            
            let proxy = item.proxy_image.as_ref().unwrap();
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
            item.pristine_proxy = Some(pristine);
        }
    }
    Ok(())
}

#[tauri::command]
pub async fn sync_thumbnail_buffer(id: String, state: State<'_, EngineState>) -> Result<(), String> {
    // Removed new_thumbnail variable hoisting
    if let Some(item_arc) = state.items.get(&id) {
        let new_thumbnail = {
            let item = item_arc.read().map_err(|e| e.to_string())?;
            generate_processed_thumbnail(&item).unwrap_or_default()
        };
        
        if !new_thumbnail.is_empty() {
            let mut item = item_arc.write().map_err(|e| e.to_string())?;
            item.thumbnail_base64 = new_thumbnail;
            let _ = save_image_state_to_db(&item);
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
        let _ = save_image_state_to_db(&item);
    }
    Ok(())
}

fn reapply_geometry(item: &mut FilmItem) {
    let mut current = item.original_proxy.clone().unwrap();
    
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
    
    item.proxy_image = Some(current);
    item.pristine_proxy = Some(compute_pristine_proxy(item.proxy_image.as_ref().unwrap(), &item.base_color, item.params.film_mode.clone()));
}

#[tauri::command]
pub async fn geometry_auto_align(id: String, state: State<'_, EngineState>) -> Result<crate::app_state::AutoAlignResult, String> {
    let item_arc = state.items.get(&id).ok_or("Image not found")?.clone();
    
    let (crop_rect, angle) = tokio::task::spawn_blocking(move || -> Result<_, String> {
        let original_proxy = {
            let item = item_arc.read().map_err(|e| e.to_string())?;
            item.original_proxy.clone().unwrap()
        };
        
        let first_result = crate::geometry::auto_crop_rect(&original_proxy)?;
        
        let proxy_image = {
            let mut item = item_arc.write().map_err(|e| e.to_string())?;
            item.geom.angle = first_result.angle;
            reapply_geometry(&mut item);
            item.proxy_image.clone().unwrap()
        };
        
        let second_result = crate::geometry::auto_crop_rect(&proxy_image)?;
        
        let mut item = item_arc.write().map_err(|e| e.to_string())?;
        item.geom.crop_rect = second_result.crop_rect.clone();
        
        Ok((item.geom.crop_rect.clone(), item.geom.angle))
    }).await.map_err(|e| e.to_string())??;

    Ok(crate::app_state::AutoAlignResult {
        crop_rect,
        angle,
    })
}

#[tauri::command]
pub async fn get_proxy_image_data(
    id: String,
    state: State<'_, EngineState>,
) -> Result<tauri::ipc::Response, String> {
    let item_arc = state.items.get(&id).ok_or("Image ID not found")?;
    let item = item_arc.read().map_err(|e| e.to_string())?;
    
    let proxy = item.proxy_image.as_ref().unwrap();
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
    roll_id: String,
    state: State<'_, EngineState>,
) -> Result<(), String> {
    let _ = roll_id;
    if let Some(item_arc) = state.items.get(&id) {
        let mut item = item_arc.write().map_err(|e| e.to_string())?;
        item.params = params;
        let _ = save_image_state_to_db(&item);
    }
    Ok(())
}

fn apply_usm(buffer: &mut ImageBuffer<Rgb<u16>, Vec<u16>>, sigma: f32, amount: f32) {
    let blurred = imageproc::filter::gaussian_blur_f32(buffer, sigma);
    buffer.pixels_mut().zip(blurred.pixels()).for_each(|(p, b)| {
        for i in 0..3 {
            let orig = p[i] as f32;
            let blur = b[i] as f32;
            let usm = orig + (orig - blur) * amount;
            p[i] = usm.clamp(0.0, 65535.0) as u16;
        }
    });
}

#[tauri::command]
pub async fn batch_export_images(
    export_ids: Vec<String>,
    output_dir: String,
    format: String,
    color_space: String,
    resample_mode: String,
    apply_usm_flag: bool,
    naming_token: String,
    quality: u32,
    state: State<'_, EngineState>,
) -> Result<usize, String> {
    let _ = color_space;
    let count = export_ids.len();
    if count == 0 {
        return Ok(0);
    }

    let success_count = std::sync::atomic::AtomicUsize::new(0);

    let dcp_profile = state.dcp_profile.read().unwrap().clone();
    let working_colorspace = state.working_colorspace.read().unwrap().clone();
    let rolls = state.rolls.read().unwrap().clone();

    export_ids.par_iter().enumerate().for_each(|(seq_idx, id)| {
        if let Some(item_arc) = state.items.get(id) {
            let item = item_arc.read().unwrap();
            if let Ok(original) = load_image_buffer(&item.file_path, false, dcp_profile.as_deref(), &working_colorspace) {
                let params = &item.params;
                let base_color = &item.base_color;

                let pipeline = FilmPipeline::new(
                    [base_color.base_r, base_color.base_g, base_color.base_b],
                    [
                        params.exposure.exposure + params.exposure.exp_r,
                        params.exposure.exposure + params.exposure.exp_g,
                        params.exposure.exposure + params.exposure.exp_b,
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

                if resample_mode == "long_edge_2048" {
                    let (tw, th) = transformed.dimensions();
                    let long_edge = tw.max(th);
                    if long_edge > 2048 {
                        let scale = 2048.0 / long_edge as f32;
                        let nw = (tw as f32 * scale).ceil() as u32;
                        let nh = (th as f32 * scale).ceil() as u32;
                        transformed = image::imageops::resize(&transformed, nw, nh, image::imageops::FilterType::Lanczos3);
                    }
                }
                
                let scale = quality as f32 / 100.0;
                if scale < 0.99 {
                    let (tw, th) = transformed.dimensions();
                    let nw = (tw as f32 * scale).max(1.0) as u32;
                    let nh = (th as f32 * scale).max(1.0) as u32;
                    transformed = image::imageops::resize(&transformed, nw, nh, image::imageops::FilterType::Lanczos3);
                }

                let (width, height) = transformed.dimensions();
                let mut out_buffer = ImageBuffer::<Rgb<u16>, Vec<u16>>::new(width, height);
                let raw_pixels: &[u16] = transformed.as_raw().as_slice();
                let out_pixels: &mut [u16] = out_buffer.as_mut();

                let d_min = params.density.d_min;
                let d_max = params.density.d_max;
                let gamma = params.density.gamma;

                raw_pixels.par_chunks(3).zip(out_pixels.par_chunks_mut(3)).for_each(|(in_px, out_px)| {
                    let linear_rgb = [
                        (in_px[0] as f32) / 65535.0,
                        (in_px[1] as f32) / 65535.0,
                        (in_px[2] as f32) / 65535.0,
                    ];
                    let density = pipeline.process_pixel(&linear_rgb);
                    let norm_r = ((density[0] - d_min[0]) / (d_max[0] - d_min[0])).clamp(0.0, 1.0);
                    let norm_g = ((density[1] - d_min[1]) / (d_max[1] - d_min[1])).clamp(0.0, 1.0);
                    let norm_b = ((density[2] - d_min[2]) / (d_max[2] - d_min[2])).clamp(0.0, 1.0);
                    out_px[0] = (norm_r.powf(1.0 / gamma) * 65535.0) as u16;
                    out_px[1] = (norm_g.powf(1.0 / gamma) * 65535.0) as u16;
                    out_px[2] = (norm_b.powf(1.0 / gamma) * 65535.0) as u16;
                });

                if apply_usm_flag && !format.starts_with("tiff16") {
                    apply_usm(&mut out_buffer, 1.0, 0.5);
                }

                let file_stem = std::path::Path::new(&item.file_path)
                    .file_stem()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_string();
                    
                let roll = rolls.iter().find(|r| r.image_paths.contains(&item.file_path));
                let roll_name = roll.map(|r| r.roll_id.clone()).unwrap_or_else(|| "Roll".to_string());
                let camera_name = roll.map(|r| r.camera.clone()).unwrap_or_else(|| "Camera".to_string());
                let seq_str = format!("{:03}", seq_idx + 1);
                
                let mut final_name = naming_token.clone();
                final_name = final_name.replace("{Roll}", &roll_name);
                final_name = final_name.replace("{Camera}", &camera_name);
                final_name = final_name.replace("{Seq}", &seq_str);
                
                if final_name.trim().is_empty() {
                    final_name = file_stem;
                }
                let file_stem = final_name;
                
                let out_path = match format.as_str() {
                    "jpeg100" => {
                        // JPEG is 8-bit, we must convert
                        let mut out8 = ImageBuffer::<Rgb<u8>, Vec<u8>>::new(width, height);
                        for (in_p, out_p) in out_buffer.pixels().zip(out8.pixels_mut()) {
                            out_p[0] = (in_p[0] >> 8) as u8;
                            out_p[1] = (in_p[1] >> 8) as u8;
                            out_p[2] = (in_p[2] >> 8) as u8;
                        }
                        let path = std::path::Path::new(&output_dir).join(format!("{}.jpg", file_stem));
                        
                        let mut cursor = std::io::Cursor::new(Vec::new());
                        let jpeg_quality = quality.clamp(1, 100) as u8;
                        if image::DynamicImage::ImageRgb8(out8.clone()).write_to(&mut cursor, image::ImageOutputFormat::Jpeg(jpeg_quality)).is_ok() {
                            std::fs::write(&path, cursor.into_inner())
                                .map(|_| path)
                                .map_err(|e| image::ImageError::IoError(e))
                        } else {
                            Err(image::ImageError::IoError(std::io::Error::new(std::io::ErrorKind::Other, "JPEG Encoding Error")))
                        }
                    },
                    "png" => {
                        let path = std::path::Path::new(&output_dir).join(format!("{}.png", file_stem));
                        out_buffer.save(&path).map(|_| path)
                    },
                    "tiff8" => {
                        let mut out8 = ImageBuffer::<Rgb<u8>, Vec<u8>>::new(width, height);
                        for (in_p, out_p) in out_buffer.pixels().zip(out8.pixels_mut()) {
                            out_p[0] = (in_p[0] >> 8) as u8;
                            out_p[1] = (in_p[1] >> 8) as u8;
                            out_p[2] = (in_p[2] >> 8) as u8;
                        }
                        let path = std::path::Path::new(&output_dir).join(format!("{}_8bit.tiff", file_stem));
                        out8.save(&path).map(|_| path)
                    },
                    _ => {
                        // tiff16_uncompressed or tiff16_lzw
                        let path = std::path::Path::new(&output_dir).join(format!("{}_16bit.tiff", file_stem));
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

#[tauri::command]
pub async fn get_rolls(state: State<'_, EngineState>) -> Result<Vec<Roll>, String> {
    let rolls = state.rolls.read().unwrap();
    Ok(rolls.clone())
}

#[tauri::command]
pub async fn import_roll(
    roll: Roll,
    paths: Vec<String>,
    state: State<'_, EngineState>
) -> Result<(), String> {
    let roll_id_clone = roll.roll_id.clone();
    {
        let mut rolls = state.rolls.write().unwrap();
        rolls.push(roll);
    }
    
    {
        let rolls = state.rolls.read().unwrap();
        if let Ok(json) = serde_json::to_string_pretty(&*rolls) {
            let _ = std::fs::write("rolls.json", json);
        }
    }
    
    crate::commands::import_images(paths, Some(false), Some(true), Some(roll_id_clone), Some(false), state).await
}

#[tauri::command]
pub async fn save_contact_sheet(data_url: String, filename: Option<String>) -> Result<String, String> {
    let b64_data = if data_url.starts_with("data:image/") {
        if let Some(idx) = data_url.find("base64,") {
            &data_url[idx + 7..]
        } else {
            return Err("Invalid data URL".into());
        }
    } else {
        &data_url
    };
    
    let image_data = general_purpose::STANDARD.decode(b64_data).map_err(|e| format!("Base64 decode failed: {:?}", e))?;
    let default_name = filename.unwrap_or_else(|| "contact_sheet.jpg".to_string());
    
    let file_path = tauri::async_runtime::spawn_blocking(move || {
        FileDialog::new()
            .set_file_name(&default_name)
            .add_filter("JPEG Image", &["jpg", "jpeg"])
            .save_file()
    }).await.map_err(|e| format!("Dialog error: {:?}", e))?;
    
    if let Some(path) = file_path {
        std::fs::write(&path, image_data).map_err(|e| format!("Save error: {:?}", e))?;
        Ok(path.to_string_lossy().to_string())
    } else {
        Err("Cancelled".into())
    }
}

#[tauri::command]
pub async fn delete_rolls(
    roll_ids: Vec<String>,
    state: State<'_, EngineState>
) -> Result<(), String> {
    {
        let mut rolls = state.rolls.write().unwrap();
        
        let mut paths_to_delete = Vec::new();
        for r in rolls.iter() {
            if roll_ids.contains(&r.roll_id) {
                paths_to_delete.extend(r.image_paths.clone());
            }
        }
        
        rolls.retain(|r| !roll_ids.contains(&r.roll_id));
        if let Ok(json) = serde_json::to_string_pretty(&*rolls) {
            let _ = std::fs::write("rolls.json", json);
        }
        
        // Also delete from memory and DB to give it "no memory"
        if let Ok(conn) = rusqlite::Connection::open(get_db_path()) {
            for rid in &roll_ids {
                let _ = conn.execute("DELETE FROM image_states WHERE roll_id = ?1", [rid]);
            }
            for path in &paths_to_delete {
                
                // Remove from state.items
                let mut ids_to_remove = Vec::new();
                for kv in state.items.iter() {
                    let db_path = kv.value().read().unwrap().file_path.clone();
                    if db_path == *path || db_path.replace("\\", "/").to_lowercase() == path.replace("\\", "/").to_lowercase() {
                        ids_to_remove.push(kv.key().clone());
                    }
                }
                for id in ids_to_remove {
                    state.items.remove(&id);
                    if let Ok(mut order) = state.item_order.write() {
                        order.retain(|i| i != &id);
                    }
                }
            }
        }
    }
    Ok(())
}

#[tauri::command]
pub async fn promote_roll(
    roll_id: String,
    state: State<'_, EngineState>
) -> Result<(), String> {
    let mut order_guard = state.item_order.write().map_err(|e| e.to_string())?;
    let rolls = state.rolls.read().unwrap();
    if let Some(roll) = rolls.iter().find(|r| r.roll_id == roll_id) {
        let guard = state.items.clone();
        for path in &roll.image_paths {
            for kv in guard.iter() {
                let mut item = kv.value().write().unwrap();
                let db_path = item.file_path.clone();
                if db_path == *path || db_path.replace("\\", "/").to_lowercase() == path.replace("\\", "/").to_lowercase() {
                    item.in_library = true;
                    item.is_loose = false;
                    let _ = save_image_state_to_db(&item);
                    if !order_guard.contains(&item.id) {
                        order_guard.push(item.id.clone());
                    }
                    break;
                }
            }
        }
    }
    Ok(())
}

#[tauri::command]
pub async fn append_to_roll(
    roll_id: String,
    paths: Vec<String>,
    state: State<'_, EngineState>
) -> Result<(), String> {
    {
        let mut rolls = state.rolls.write().unwrap();
        if let Some(roll) = rolls.iter_mut().find(|r| r.roll_id == roll_id) {
            for p in &paths {
                if !roll.image_paths.iter().any(|existing| existing.replace("\\", "/").to_lowercase() == p.replace("\\", "/").to_lowercase()) {
                    roll.image_paths.push(p.clone());
                }
            }
        }
        if let Ok(json) = serde_json::to_string_pretty(&*rolls) {
            let _ = std::fs::write("rolls.json", json);
        }
    }
    crate::commands::import_images(paths, Some(false), Some(true), Some(roll_id), Some(false), state).await
}

#[tauri::command]
pub async fn locate_missing_file(id: String, state: State<'_, EngineState>) -> Result<String, String> {
    let file_path = tauri::async_runtime::spawn_blocking(|| {
        FileDialog::new()
            .set_title("Locate Missing File")
            .pick_file()
    }).await.map_err(|e| format!("Dialog error: {:?}", e))?;
    
    if let Some(path) = file_path {
        let new_path = path.to_string_lossy().to_string();
        
        // Update items
        if let Some(item_arc) = state.items.get(&id) {
            let mut item = item_arc.write().unwrap();
            let old_path = item.file_path.clone();
            item.file_path = new_path.clone();
            
            // Update rolls
            let mut rolls = state.rolls.write().unwrap();
            for roll in rolls.iter_mut() {
                if let Some(pos) = roll.image_paths.iter().position(|p| p.replace("\\", "/").to_lowercase() == old_path.replace("\\", "/").to_lowercase()) {
                    roll.image_paths[pos] = new_path.clone();
                }
            }
            if let Ok(json) = serde_json::to_string_pretty(&*rolls) {
                let _ = std::fs::write("rolls.json", json);
            }
        }
        Ok(new_path)
    } else {
        Err("Cancelled".into())
    }
}

// SQLite Metadata Decoupling
fn get_db_path() -> String {
    "nexfilm_user.db".to_string()
}

pub fn init_db() -> rusqlite::Result<()> {
    let conn = rusqlite::Connection::open(get_db_path())?;
    conn.execute("CREATE TABLE IF NOT EXISTS user_cameras (id INTEGER PRIMARY KEY, name TEXT NOT NULL UNIQUE)", [])?;
    conn.execute("CREATE TABLE IF NOT EXISTS user_films (id INTEGER PRIMARY KEY, name TEXT NOT NULL UNIQUE)", [])?;
    let _ = conn.execute("DROP TABLE IF EXISTS image_states", []);
    conn.execute("CREATE TABLE IF NOT EXISTS image_states (
        roll_id TEXT NOT NULL,
        file_path TEXT NOT NULL,
        thumbnail_base64 TEXT,
        params TEXT,
        geom TEXT,
        base_color TEXT,
        PRIMARY KEY (roll_id, file_path)
    )", [])?;
    Ok(())
}

pub fn save_image_state_to_db(item: &crate::app_state::FilmItem) -> Result<(), String> {
    if item.is_loose {
        return Ok(());
    }
    let conn = rusqlite::Connection::open(get_db_path()).map_err(|e| e.to_string())?;
    conn.busy_timeout(std::time::Duration::from_secs(5)).ok();
    let params_str = serde_json::to_string(&item.params).map_err(|e| e.to_string())?;
    let geom_str = serde_json::to_string(&item.geom).map_err(|e| e.to_string())?;
    let base_color_str = serde_json::to_string(&item.base_color).map_err(|e| e.to_string())?;
    
    conn.execute(
        "INSERT INTO image_states (roll_id, file_path, thumbnail_base64, params, geom, base_color) 
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)
         ON CONFLICT(roll_id, file_path) DO UPDATE SET 
         thumbnail_base64=excluded.thumbnail_base64,
         params=excluded.params, 
         geom=excluded.geom, 
         base_color=excluded.base_color",
        rusqlite::params![item.roll_id, item.file_path, item.thumbnail_base64, params_str, geom_str, base_color_str]
    ).map_err(|e| e.to_string())?;
    Ok(())
}

pub fn load_image_state_from_db(roll_id: &str, file_path: &str) -> Option<(String, crate::app_state::TuningParams, crate::app_state::GeometryState, crate::app_state::BaseColor)> {
    let conn = rusqlite::Connection::open(get_db_path()).ok()?;
    let mut stmt = conn.prepare("SELECT thumbnail_base64, params, geom, base_color FROM image_states WHERE roll_id = ?1 AND file_path = ?2").ok()?;
    
    let mut rows = stmt.query(rusqlite::params![roll_id, file_path]).ok()?;
    if let Some(row) = rows.next().ok()? {
        let thumb: String = row.get(0).ok()?;
        let params_str: String = row.get(1).ok()?;
        let geom_str: String = row.get(2).ok()?;
        let base_color_str: String = row.get(3).ok()?;
        
        let params = serde_json::from_str(&params_str).ok()?;
        let geom = serde_json::from_str(&geom_str).ok()?;
        let base_color = serde_json::from_str(&base_color_str).ok()?;
        
        return Some((thumb, params, geom, base_color));
    }
    None
}

pub fn load_all_image_states(state: &crate::app_state::EngineState) {
    if let Ok(conn) = rusqlite::Connection::open(get_db_path()) {
        if let Ok(mut stmt) = conn.prepare("SELECT roll_id, file_path, thumbnail_base64, params, geom, base_color FROM image_states") {
            if let Ok(rows) = stmt.query_map([], |row| {
                let roll_id: String = row.get(0)?;
                let file_path: String = row.get(1)?;
                let thumb: String = row.get(2)?;
                let params_str: String = row.get(3)?;
                let geom_str: String = row.get(4)?;
                let base_color_str: String = row.get(5)?;
                Ok((roll_id, file_path, thumb, params_str, geom_str, base_color_str))
            }) {
                let _order_guard = state.item_order.write().unwrap();
                for row_result in rows {
                    if let Ok((db_roll_id, db_path, thumb, params_str, geom_str, base_color_str)) = row_result {
                        let params = serde_json::from_str(&params_str).unwrap_or_default();
                        let geom = serde_json::from_str(&geom_str).unwrap_or_default();
                        let base_color = serde_json::from_str(&base_color_str).unwrap_or_default();
                        
                        let img_id = format!("img_{}", crate::commands::NEXT_ID.fetch_add(1, std::sync::atomic::Ordering::SeqCst));
                        let item = crate::app_state::FilmItem {
                            id: img_id.clone(),
                            roll_id: db_roll_id,
                            file_path: db_path.clone(),
                            thumbnail_base64: thumb,
                            original_proxy: None,
                            proxy_image: None,
                            pristine_proxy: None,
                            base_color,
                            params,
                            geom,
                            is_loose: false,
                            in_library: true,
                        };
                        state.items.insert(img_id.clone(), std::sync::Arc::new(std::sync::RwLock::new(item)));
                    }
                }
            }
        }
    }
}


#[tauri::command]
pub fn get_user_cameras() -> Result<Vec<String>, String> {
    let conn = rusqlite::Connection::open(get_db_path()).map_err(|e| e.to_string())?;
    let mut stmt = conn.prepare("SELECT name FROM user_cameras ORDER BY name").map_err(|e| e.to_string())?;
    let rows = stmt.query_map([], |row| row.get(0)).map_err(|e| e.to_string())?;
    let mut cameras = Vec::new();
    for name_result in rows {
        cameras.push(name_result.map_err(|e| e.to_string())?);
    }
    Ok(cameras)
}

#[tauri::command]
pub fn get_user_films() -> Result<Vec<String>, String> {
    let conn = rusqlite::Connection::open(get_db_path()).map_err(|e| e.to_string())?;
    let mut stmt = conn.prepare("SELECT name FROM user_films ORDER BY name").map_err(|e| e.to_string())?;
    let rows = stmt.query_map([], |row| row.get(0)).map_err(|e| e.to_string())?;
    let mut films = Vec::new();
    for name_result in rows {
        films.push(name_result.map_err(|e| e.to_string())?);
    }
    Ok(films)
}

#[tauri::command]
pub fn add_user_camera(camera: String) -> Result<(), String> {
    let conn = rusqlite::Connection::open(get_db_path()).map_err(|e| e.to_string())?;
    conn.execute("INSERT OR IGNORE INTO user_cameras (name) VALUES (?1)", rusqlite::params![camera]).map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub fn add_user_film(film: String) -> Result<(), String> {
    let conn = rusqlite::Connection::open(get_db_path()).map_err(|e| e.to_string())?;
    conn.execute("INSERT OR IGNORE INTO user_films (name) VALUES (?1)", rusqlite::params![film]).map_err(|e| e.to_string())?;
    Ok(())
}


pub fn generate_processed_thumbnail(item: &FilmItem) -> Option<String> {
    if item.pristine_proxy.is_none() { return None; }
    let params = &item.params;
    let base_color = &item.base_color;
    let pipeline = FilmPipeline::new(
        [base_color.base_r, base_color.base_g, base_color.base_b],
        [
            params.exposure.exposure + params.exposure.exp_r,
            params.exposure.exposure + params.exposure.exp_g,
            params.exposure.exposure + params.exposure.exp_b,
        ],
        params.film_mode.clone(),
    );

    let pristine = item.pristine_proxy.as_ref().unwrap();
    let (width, height) = pristine.dimensions();
    let mut thumb_8bit = RgbImage::new(width, height);
    
    let pristine_pixels: &[f32] = pristine.as_raw().as_slice();
    let out_pixels: &mut [u8] = thumb_8bit.as_mut();

    let d_min = params.density.d_min;
    let d_max = params.density.d_max;
    let gamma = params.density.gamma;

    pristine_pixels.par_chunks(3).zip(out_pixels.par_chunks_mut(3)).for_each(|(in_px, out_px)| {
        let true_density = [in_px[0], in_px[1], in_px[2]];
        let density = pipeline.apply_exposure(&true_density);

        let norm_r = ((density[0] - d_min[0]) / (d_max[0] - d_min[0])).clamp(0.0, 1.0);
        let norm_g = ((density[1] - d_min[1]) / (d_max[1] - d_min[1])).clamp(0.0, 1.0);
        let norm_b = ((density[2] - d_min[2]) / (d_max[2] - d_min[2])).clamp(0.0, 1.0);

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

    let ratio_thumb = 1024.0 / (cw.max(ch) as f32);
    let thumb_width = (cw as f32 * ratio_thumb).max(1.0) as u32;
    let thumb_height = (ch as f32 * ratio_thumb).max(1.0) as u32;
    let thumb = image::imageops::resize(&cropped_thumb, thumb_width, thumb_height, FilterType::Triangle);
    
    let mut cursor = std::io::Cursor::new(Vec::new());
    if let Ok(_) = thumb.write_to(&mut cursor, image::ImageOutputFormat::Jpeg(70)) {
        use base64::{Engine as _, engine::general_purpose};
        return Some(general_purpose::STANDARD.encode(cursor.into_inner()));
    }
    None
}
