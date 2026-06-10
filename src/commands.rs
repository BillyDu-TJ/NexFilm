use crate::app_state::{BaseColor, EngineState, FilmItem, FilmstripItem, TuningParams};
use crate::pipeline::FilmPipeline;
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
) -> ImageBuffer<Rgb<f32>, Vec<f32>> {
    let pipeline = FilmPipeline::new(
        [base_color.base_r, base_color.base_g, base_color.base_b],
        [0.0, 0.0, 0.0],
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
    let file_paths = FileDialog::new()
        .add_filter("Images", &["tif", "tiff", "png", "jpg", "jpeg", "raw"])
        .pick_files();
    
    if let Some(paths) = file_paths {
        Ok(paths.into_iter().map(|p| p.to_string_lossy().to_string()).collect())
    } else {
        Ok(Vec::new())
    }
}

#[tauri::command]
pub async fn select_export_dir() -> Result<Option<String>, String> {
    let dir_path = FileDialog::new().pick_folder();
    Ok(dir_path.map(|p| p.to_string_lossy().to_string()))
}

#[tauri::command]
pub async fn import_images(paths: Vec<String>, state: State<'_, EngineState>) -> Result<(), String> {
    if paths.is_empty() {
        return Ok(());
    }

    let mut new_items: Vec<FilmItem> = paths.into_par_iter().filter_map(|path| {
        let img = image::open(&path).ok()?;
        let img_buffer = img.into_rgb16();

        let (width, height) = img_buffer.dimensions();
        
        // Proxy 800px
        let ratio_proxy = 800.0 / (width.max(height) as f32);
        let proxy_width = (width as f32 * ratio_proxy) as u32;
        let proxy_height = (height as f32 * ratio_proxy) as u32;
        let proxy = image::imageops::resize(&img_buffer, proxy_width, proxy_height, FilterType::Triangle);

        // Thumbnail 120px
        let ratio_thumb = 120.0 / (width.max(height) as f32);
        let thumb_width = (width as f32 * ratio_thumb) as u32;
        let thumb_height = (height as f32 * ratio_thumb) as u32;
        let thumb = image::imageops::resize(&img_buffer, thumb_width, thumb_height, FilterType::Triangle);

        // Thumbnail to base64
        let mut cursor = Cursor::new(Vec::new());
        // Convert thumbnail to 8-bit for JPEG export
        let mut thumb_8bit = RgbImage::new(thumb_width, thumb_height);
        for (in_px, out_px) in thumb.pixels().zip(thumb_8bit.pixels_mut()) {
            out_px[0] = (in_px[0] >> 8) as u8;
            out_px[1] = (in_px[1] >> 8) as u8;
            out_px[2] = (in_px[2] >> 8) as u8;
        }
        thumb_8bit.write_to(&mut cursor, ImageOutputFormat::Jpeg(70)).ok()?;
        let thumbnail_base64 = general_purpose::STANDARD.encode(cursor.into_inner());

        let base_color = compute_auto_base(&proxy);
        let pristine_proxy = compute_pristine_proxy(&proxy, &base_color);

        let id = format!("img_{}", NEXT_ID.fetch_add(1, Ordering::SeqCst));

        Some(FilmItem {
            id,
            file_path: path,
            thumbnail_base64,
            proxy_image: proxy,
            pristine_proxy,
            base_color,
            params: TuningParams::default(),
        })
    }).collect();

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

#[tauri::command]
pub async fn switch_active_image(id: String, state: State<'_, EngineState>) -> Result<TuningParams, String> {
    let items = state.items.read().map_err(|e| e.to_string())?;
    if let Some(item) = items.iter().find(|i| i.id == id) {
        *state.active_id.write().map_err(|e| e.to_string())? = Some(id);
        Ok(item.params.clone())
    } else {
        Err("Image ID not found".into())
    }
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

    items.par_iter().for_each(|item| {
        let img_result = image::open(&item.file_path);
        if let Ok(img) = img_result {
            let original = img.into_rgb16();
            let params = &item.params;
            let base_color = &item.base_color;

            let pipeline = FilmPipeline::new(
                [base_color.base_r, base_color.base_g, base_color.base_b],
                [
                    params.exposure + params.exp_r,
                    params.exposure + params.exp_g,
                    params.exposure + params.exp_b,
                ],
            );

            let (width, height) = original.dimensions();
            let mut out_buffer = ImageBuffer::<Rgb<u16>, Vec<u16>>::new(width, height);

            let raw_pixels: &[u16] = original.as_raw().as_slice();
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

            // Make safe filename
            let file_name = std::path::Path::new(&item.file_path)
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string();
            let out_path = std::path::Path::new(&output_dir).join(format!("nexfilm_{}", file_name));
            let _ = out_buffer.save(out_path);
        }
    });

    Ok(count)
}
