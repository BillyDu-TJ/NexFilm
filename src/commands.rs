use crate::app_state::{BaseColor, EngineState, FilmItem, FilmstripItem, TuningParams, FilmMode, Roll};
use serde::Serialize;
use crate::pipeline::FilmPipeline;

use base64::{engine::general_purpose, Engine as _};
use image::{imageops::FilterType, ImageBuffer, ImageOutputFormat, Rgb, RgbImage, GenericImageView};
use rayon::prelude::*;
use tauri::{Emitter, Manager};
use rfd::FileDialog;
use std::io::Cursor;
use tauri::State;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock, RwLock};
use serde_json::Value;
use std::collections::{HashSet, VecDeque};
use std::sync::{MutexGuard, RwLockReadGuard, RwLockWriteGuard};

static NEXT_ID: AtomicUsize = AtomicUsize::new(1);
static PREFETCH_ACTIVE_WORKERS: AtomicUsize = AtomicUsize::new(0);
static PREFETCH_HIGH_PRIORITY_WORKERS: AtomicUsize = AtomicUsize::new(0);
static RAYON_INIT: OnceLock<()> = OnceLock::new();

const FALLBACK_THUMB: &str = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mM8c+bMfwAIGwK9t856VAAAAABJRU5ErkJggg==";
const PROXY_LONG_EDGE: f32 = 2560.0;

struct LibrawGuard(*mut libraw_sys::libraw_data_t);
impl Drop for LibrawGuard {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe { libraw_sys::libraw_close(self.0) };
        }
    }
}

struct LibrawMemGuard(*mut libraw_sys::libraw_processed_image_t);
impl Drop for LibrawMemGuard {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe { libraw_sys::libraw_dcraw_clear_mem(self.0 as *mut _) };
        }
    }
}

fn resize_preview_image(mut img: image::DynamicImage, max_edge: u32) -> image::DynamicImage {
    let (w, h) = img.dimensions();
    if w.max(h) > max_edge {
        let ratio = max_edge as f32 / w.max(h) as f32;
        let new_w = (w as f32 * ratio).max(1.0) as u32;
        let new_h = (h as f32 * ratio).max(1.0) as u32;
        img = img.resize_exact(new_w, new_h, image::imageops::FilterType::Triangle);
    }
    img
}

fn write_jpeg_base64(img: image::DynamicImage, quality: u8) -> Option<String> {
    let mut cursor = Cursor::new(Vec::new());
    img.write_to(&mut cursor, ImageOutputFormat::Jpeg(quality)).ok()?;
    Some(general_purpose::STANDARD.encode(cursor.into_inner()))
}

fn encode_preview_jpeg_base64(img: image::DynamicImage, max_edge: u32, quality: u8) -> Option<String> {
    write_jpeg_base64(resize_preview_image(img, max_edge), quality)
}

fn preview_image_needs_stretch(img: &image::DynamicImage) -> bool {
    matches!(
        img,
        image::DynamicImage::ImageLuma16(_)
            | image::DynamicImage::ImageLumaA16(_)
            | image::DynamicImage::ImageRgb16(_)
            | image::DynamicImage::ImageRgba16(_)
            | image::DynamicImage::ImageRgb32F(_)
            | image::DynamicImage::ImageRgba32F(_)
    )
}

fn percentile_from_histogram(hist: &[u32], rank: u64) -> u16 {
    let mut seen = 0u64;
    for (value, count) in hist.iter().enumerate() {
        seen += *count as u64;
        if seen > rank {
            return value as u16;
        }
    }
    u16::MAX
}

fn encode_stretched_preview_jpeg_base64(img: image::DynamicImage, max_edge: u32, quality: u8) -> Option<String> {
    let img = resize_preview_image(img, max_edge);
    let rgb16 = img.to_rgb16();
    let (width, height) = rgb16.dimensions();
    let raw = rgb16.as_raw();
    if raw.is_empty() {
        return None;
    }

    let mut hist = vec![0u32; 65536];
    for &value in raw {
        hist[value as usize] += 1;
    }

    let total = raw.len() as u64;
    let clip = (total / 200).min(total.saturating_sub(1) / 2);
    let low = percentile_from_histogram(&hist, clip);
    let high = percentile_from_histogram(&hist, total.saturating_sub(clip + 1));
    if high <= low {
        return write_jpeg_base64(image::DynamicImage::ImageRgb16(rgb16), quality);
    }

    let low_f = low as f32;
    let scale = 255.0 / (high as f32 - low_f);
    let mut out = RgbImage::new(width, height);
    out.as_mut()
        .par_chunks_exact_mut(3)
        .zip(raw.par_chunks_exact(3))
        .for_each(|(dst, src)| {
            dst[0] = ((src[0] as f32 - low_f) * scale).round().clamp(0.0, 255.0) as u8;
            dst[1] = ((src[1] as f32 - low_f) * scale).round().clamp(0.0, 255.0) as u8;
            dst[2] = ((src[2] as f32 - low_f) * scale).round().clamp(0.0, 255.0) as u8;
        });

    write_jpeg_base64(image::DynamicImage::ImageRgb8(out), quality)
}

fn encode_preview_bytes_base64(bytes: &[u8], max_edge: u32, quality: u8) -> String {
    if let Ok(img) = image::load_from_memory(bytes) {
        if let Some(encoded) = encode_preview_jpeg_base64(img, max_edge, quality) {
            return encoded;
        }
    }
    general_purpose::STANDARD.encode(bytes)
}

fn is_raw_extension(path: &str) -> bool {
    matches!(
        std::path::Path::new(path)
            .extension()
            .and_then(|ext| ext.to_str())
            .map(|ext| ext.to_ascii_lowercase())
            .as_deref(),
        Some(
            "dng" | "nef" | "cr2" | "cr3" | "arw" | "raf" | "rw2" | "orf" | "srw" | "raw"
                | "3fr" | "erf" | "kdc" | "iiq" | "mos" | "mrw" | "pef" | "x3f"
        )
    )
}

fn is_direct_image_extension(path: &str) -> bool {
    matches!(
        std::path::Path::new(path)
            .extension()
            .and_then(|ext| ext.to_str())
            .map(|ext| ext.to_ascii_lowercase())
            .as_deref(),
        Some("tif" | "tiff" | "jpg" | "jpeg" | "png")
    )
}

fn is_tiff_extension(path: &str) -> bool {
    matches!(
        std::path::Path::new(path)
            .extension()
            .and_then(|ext| ext.to_str())
            .map(|ext| ext.to_ascii_lowercase())
            .as_deref(),
        Some("tif" | "tiff")
    )
}

fn decode_direct_image_preview_base64(path: &str, max_edge: u32) -> Option<String> {
    let img = image::open(path).ok()?;
    if is_tiff_extension(path) || preview_image_needs_stretch(&img) {
        encode_stretched_preview_jpeg_base64(img, max_edge, 86)
    } else {
        encode_preview_jpeg_base64(img, max_edge, 86)
    }
}

fn extract_embedded_preview_base64(path: &str, max_edge: u32) -> Option<String> {
    unsafe {
        let data = libraw_sys::libraw_init(0);
        if data.is_null() {
            return None;
        }
        let _data_guard = LibrawGuard(data);
        let c_path = std::ffi::CString::new(path).ok()?;
        let mut opened = libraw_sys::libraw_open_file(data, c_path.as_ptr()) == 0;

        let mut _buf = Vec::new();
        if !opened {
            if let Ok(b) = std::fs::read(path) {
                _buf = b;
                opened = libraw_sys::libraw_open_buffer(
                    data,
                    _buf.as_ptr() as *const _,
                    _buf.len(),
                ) == 0;
            }
        }

        if opened && libraw_sys::libraw_unpack_thumb(data) == 0 {
            let mut err = 0;
            let thumb = libraw_sys::libraw_dcraw_make_mem_thumb(data, &mut err);
            if !thumb.is_null() {
                let _thumb_guard = LibrawMemGuard(thumb);
                let thumb_type = (*thumb).type_;
                let thumb_len = (*thumb).data_size as usize;
                let thumb_data = std::slice::from_raw_parts((*thumb).data.as_ptr(), thumb_len);

                if thumb_type == 1 {
                    return Some(encode_preview_bytes_base64(thumb_data, max_edge, 86));
                }

                if thumb_type == 2 {
                    let width = (*thumb).width as u32;
                    let height = (*thumb).height as u32;
                    let colors = (*thumb).colors;
                    let bits = (*thumb).bits;

                    if colors == 3 && bits == 8 {
                        if let Some(img) = image::ImageBuffer::<image::Rgb<u8>, _>::from_raw(
                            width,
                            height,
                            thumb_data.to_vec(),
                        ) {
                            return encode_preview_jpeg_base64(
                                image::DynamicImage::ImageRgb8(img),
                                max_edge,
                                86,
                            );
                        }
                    } else if colors == 3 && bits == 16 {
                        let slice = std::slice::from_raw_parts(
                            (*thumb).data.as_ptr() as *const u16,
                            thumb_len / 2,
                        );
                        let mut img = ImageBuffer::<Rgb<u16>, Vec<u16>>::new(width, height);
                        img.as_mut().par_chunks_exact_mut(3).enumerate().for_each(|(i, pixel)| {
                            let idx = i * 3;
                            pixel[0] = slice.get(idx).copied().unwrap_or(0);
                            pixel[1] = slice.get(idx + 1).copied().unwrap_or(0);
                            pixel[2] = slice.get(idx + 2).copied().unwrap_or(0);
                        });
                        return encode_stretched_preview_jpeg_base64(
                            image::DynamicImage::ImageRgb16(img),
                            max_edge,
                            86,
                        );
                    }
                }

                return Some(encode_preview_bytes_base64(thumb_data, max_edge, 86));
            }
        }
    }

    None
}

fn decode_preview_base64(path: &str, max_edge: u32) -> Option<String> {
    if is_direct_image_extension(path) {
        return decode_direct_image_preview_base64(path, max_edge);
    }

    if let Some(preview) = extract_embedded_preview_base64(path, max_edge) {
        return Some(preview);
    }

    if !is_raw_extension(path) {
        return decode_direct_image_preview_base64(path, max_edge);
    }

    None
}

fn decode_develop_preview_base64(path: &str, max_edge: u32) -> Option<String> {
    if is_direct_image_extension(path) {
        return decode_direct_image_preview_base64(path, max_edge);
    }
    decode_preview_base64(path, max_edge)
}

fn build_response_buffer(width: u32, height: u32, base_color: &BaseColor, pixels: &[u16], is_full_proxy: bool) -> Vec<u8> {
    let epsilon = 1e-6_f32;
    let t_r = (base_color.base_r as f32 / 65535.0).max(epsilon);
    let t_g = (base_color.base_g as f32 / 65535.0).max(epsilon);
    let t_b = (base_color.base_b as f32 / 65535.0).max(epsilon);
    let bd_r: f32 = -t_r.log10();
    let bd_g: f32 = -t_g.log10();
    let bd_b: f32 = -t_b.log10();

    let mut out_buffer = vec![0u8; (width * height * 8) as usize + 24];
    out_buffer[0..4].copy_from_slice(&width.to_le_bytes());
    out_buffer[4..8].copy_from_slice(&height.to_le_bytes());
    out_buffer[8..12].copy_from_slice(&bd_r.to_le_bytes());
    out_buffer[12..16].copy_from_slice(&bd_g.to_le_bytes());
    out_buffer[16..20].copy_from_slice(&bd_b.to_le_bytes());
    out_buffer[20..24].copy_from_slice(&(if is_full_proxy { 1u32 } else { 0u32 }).to_le_bytes());

    let out_slice = &mut out_buffer[24..];
    pixels.par_chunks(3).zip(out_slice.par_chunks_mut(8)).for_each(|(chunk, out_chunk)| {
        out_chunk[0..2].copy_from_slice(&chunk[0].to_le_bytes());
        out_chunk[2..4].copy_from_slice(&chunk[1].to_le_bytes());
        out_chunk[4..6].copy_from_slice(&chunk[2].to_le_bytes());
        out_chunk[6..8].copy_from_slice(&65535u16.to_le_bytes());
    });
    out_buffer
}

fn build_response_buffer_from_proxy(
    proxy: &ImageBuffer<Rgb<u16>, Vec<u16>>,
    base_color: &BaseColor,
    is_full_proxy: bool,
) -> Vec<u8> {
    let (width, height) = proxy.dimensions();
    build_response_buffer(width, height, base_color, proxy.as_raw().as_slice(), is_full_proxy)
}

fn lock_mutex<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|e| e.into_inner())
}

fn read_lock<T>(lock: &RwLock<T>) -> RwLockReadGuard<'_, T> {
    lock.read().unwrap_or_else(|e| e.into_inner())
}

fn write_lock<T>(lock: &RwLock<T>) -> RwLockWriteGuard<'_, T> {
    lock.write().unwrap_or_else(|e| e.into_inner())
}

pub fn init_background_limits() {
    RAYON_INIT.get_or_init(|| {
        let threads = std::thread::available_parallelism()
            .map(|n| n.get().saturating_sub(2).clamp(1, 4))
            .unwrap_or(2);
        let _ = rayon::ThreadPoolBuilder::new()
            .num_threads(threads)
            .thread_name(|i| format!("nexfilm-rayon-{}", i))
            .build_global();
    });
}

#[derive(Clone)]
struct ProxyPrefetchJob {
    id: String,
    high_priority: bool,
    attempts: u8,
}

static PREFETCH_QUEUE: OnceLock<Mutex<VecDeque<ProxyPrefetchJob>>> = OnceLock::new();
static PREFETCH_IN_FLIGHT: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();

fn proxy_worker_limit(has_high_priority: bool) -> usize {
    if !has_high_priority {
        return 1;
    }
    std::thread::available_parallelism()
        .map(|n| if n.get() >= 4 { 2 } else { 1 })
        .unwrap_or(1)
}

fn prefetch_queue() -> &'static Mutex<VecDeque<ProxyPrefetchJob>> {
    PREFETCH_QUEUE.get_or_init(|| Mutex::new(VecDeque::new()))
}

fn prefetch_in_flight() -> &'static Mutex<HashSet<String>> {
    PREFETCH_IN_FLIGHT.get_or_init(|| Mutex::new(HashSet::new()))
}

struct ProxyInFlightGuard {
    id: String,
}

impl Drop for ProxyInFlightGuard {
    fn drop(&mut self) {
        lock_mutex(prefetch_in_flight()).remove(&self.id);
    }
}

fn enqueue_proxy_job(app_handle: tauri::AppHandle, id: String, high_priority: bool) {
    {
        let mut queue = lock_mutex(prefetch_queue());
        if let Some(existing) = queue.iter_mut().find(|job| job.id == id) {
            if high_priority && !existing.high_priority {
                existing.high_priority = true;
                if let Some(pos) = queue.iter().position(|job| job.id == id) {
                    if let Some(job) = queue.remove(pos) {
                        queue.push_front(job);
                    }
                }
            }
            return;
        }
        let job = ProxyPrefetchJob { id, high_priority, attempts: 0 };
        if high_priority {
            queue.push_front(job);
        } else {
            queue.push_back(job);
        }
    }

    spawn_proxy_workers(app_handle);
}

fn spawn_proxy_workers(app_handle: tauri::AppHandle) {
    loop {
        let has_high_priority = {
            let queue = lock_mutex(prefetch_queue());
            queue.iter().any(|job| job.high_priority)
        };
        let active = PREFETCH_ACTIVE_WORKERS.load(Ordering::SeqCst);
        if active >= proxy_worker_limit(has_high_priority) {
            break;
        }

        if PREFETCH_ACTIVE_WORKERS
            .compare_exchange(active, active + 1, Ordering::SeqCst, Ordering::SeqCst)
            .is_ok()
        {
            let app_for_worker = app_handle.clone();
            std::thread::spawn(move || {
                run_proxy_prefetch_worker(app_for_worker.clone());
                PREFETCH_ACTIVE_WORKERS.fetch_sub(1, Ordering::SeqCst);
                if !lock_mutex(prefetch_queue()).is_empty() {
                    spawn_proxy_workers(app_for_worker);
                }
            });
        }
    }
}

fn run_proxy_prefetch_worker(app_handle: tauri::AppHandle) {
    loop {
        let job = {
            let mut queue = lock_mutex(prefetch_queue());
            queue.pop_front()
        };

        let Some(job) = job else { break; };

        let already_processing = {
            let mut in_flight = lock_mutex(prefetch_in_flight());
            !in_flight.insert(job.id.clone())
        };
        if already_processing {
            continue;
        }
        let _in_flight_guard = ProxyInFlightGuard { id: job.id.clone() };
        let _priority_guard = if job.high_priority {
            PREFETCH_HIGH_PRIORITY_WORKERS.fetch_add(1, Ordering::SeqCst);
            Some(HighPriorityWorkerGuard)
        } else {
            None
        };

        let state = app_handle.state::<EngineState>();
        let item_arc = match state.items.get(&job.id) {
            Some(item) => item,
            None => {
                if job.attempts < 5 {
                    std::thread::sleep(std::time::Duration::from_millis(20));
                    let mut queue = lock_mutex(prefetch_queue());
                    queue.push_back(ProxyPrefetchJob {
                        id: job.id,
                        high_priority: job.high_priority,
                        attempts: job.attempts + 1,
                    });
                }
                continue;
            }
        };

        let (file_path, needs_base_color, existing_base_color, film_mode, has_proxy, has_pristine, dcp_profile, colorspace) = {
            let item = match item_arc.read() {
                Ok(item) => item,
                Err(e) => e.into_inner(),
            };
            let needs = item.base_color.base_r == 32768
                && item.base_color.base_g == 32768
                && item.base_color.base_b == 32768;
            (
                item.file_path.clone(),
                needs,
                item.base_color.clone(),
                item.params.film_mode.clone(),
                item.proxy_image.is_some(),
                item.pristine_proxy.is_some(),
                read_lock(&state.dcp_profile).clone(),
                read_lock(&state.working_colorspace).clone(),
            )
        };

        let result = if has_proxy && has_pristine {
            Ok(None)
        } else {
            let loaded = load_image_buffer(&file_path, true, dcp_profile.as_deref(), &colorspace);
            match loaded {
                Ok(img_buffer) => {
                    let (width, height) = img_buffer.dimensions();
                    let ratio_proxy = PROXY_LONG_EDGE / (width.max(height) as f32);
                    let proxy_width = (width as f32 * ratio_proxy).max(1.0) as u32;
                    let proxy_height = (height as f32 * ratio_proxy).max(1.0) as u32;
                    let proxy = image::imageops::resize(&img_buffer, proxy_width, proxy_height, FilterType::Triangle);
                    let bc = if needs_base_color {
                        compute_auto_base(&proxy)
                    } else {
                        existing_base_color.clone()
                    };
                    let pristine = compute_pristine_proxy(&proxy, &bc, film_mode);
                    Ok(Some((proxy, bc, pristine)))
                }
                Err(e) => Err(e),
            }
        };

        match result {
            Ok(Some((proxy, bc, pristine))) => {
                if let Some(item_arc) = state.items.get(&job.id) {
                    let mut item = write_lock(&item_arc);
                    if item.proxy_image.is_none() {
                        item.original_proxy = Some(proxy.clone());
                        item.proxy_image = Some(proxy);
                        if needs_base_color {
                            item.base_color = bc;
                        }
                    }
                    if item.pristine_proxy.is_none() {
                        item.pristine_proxy = Some(pristine);
                    }
                    if !item.is_loose {
                        let _ = save_image_state_to_db(&item);
                    }
                }
                track_proxy_loaded(&state, &job.id);
                let _ = app_handle.emit("proxy_ready", serde_json::json!({
                    "id": job.id,
                    "priority": if job.high_priority { "high" } else { "low" }
                }));
            }
            Ok(None) => {
                let _ = app_handle.emit("proxy_ready", serde_json::json!({
                    "id": job.id,
                    "priority": if job.high_priority { "high" } else { "low" }
                }));
            }
            Err(e) => {
                eprintln!("[Prefetch] FAILED to load {}: {}", job.id, e);
            }
        }
    }
}

struct HighPriorityWorkerGuard;

impl Drop for HighPriorityWorkerGuard {
    fn drop(&mut self) {
        PREFETCH_HIGH_PRIORITY_WORKERS.fetch_sub(1, Ordering::SeqCst);
    }
}


fn compute_auto_base(proxy: &ImageBuffer<Rgb<u16>, Vec<u16>>) -> BaseColor {
    let raw = proxy.as_raw();
    let total_pixels = raw.len() / 3;

    // O(N) histogram: count occurrences of each 16-bit value per channel
    let mut hist_r = vec![0u32; 65536];
    let mut hist_g = vec![0u32; 65536];
    let mut hist_b = vec![0u32; 65536];

    for px in raw.chunks_exact(3) {
        hist_r[px[0] as usize] += 1;
        hist_g[px[1] as usize] += 1;
        hist_b[px[2] as usize] += 1;
    }

    // Threshold: top 1% of pixels
    let threshold = ((total_pixels as f64 * 0.01).ceil() as usize).max(1);

    // Find 99th percentile by scanning from high to low,
    // accumulating counts until we reach threshold
    let find_percentile = |hist: &[u32]| -> u16 {
        let mut accum = 0u32;
        for (val, &count) in hist.iter().enumerate().rev() {
            accum += count;
            if accum >= threshold as u32 {
                return val as u16;
            }
        }
        65535 // fallback (should never reach here)
    };

    BaseColor {
        base_r: find_percentile(&hist_r),
        base_g: find_percentile(&hist_g),
        base_b: find_percentile(&hist_b),
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
            img_buffer.as_mut().par_chunks_exact_mut(3).enumerate().for_each(|(i, pixel)| {
                let idx = i * colors;
                pixel[0] = slice.get(idx).copied().unwrap_or(0);
                pixel[1] = slice.get(idx + 1).copied().unwrap_or(0);
                pixel[2] = slice.get(idx + 2).copied().unwrap_or(0);
            });
            
            libraw_sys::libraw_dcraw_clear_mem(mem_image as *mut _);
            libraw_sys::libraw_close(data);

            Ok(img_buffer)
        }
    }
}

#[tauri::command]
pub async fn import_images(paths: Vec<String>, is_loose: Option<bool>, in_library: Option<bool>, roll_id: Option<String>, is_historical: Option<bool>, _state: State<'_, EngineState>, app_handle: tauri::AppHandle) -> Result<(), String> {
    if paths.is_empty() {
        return Ok(());
    }

    let target_roll = roll_id.clone().unwrap_or_else(|| "LOOSE_DEFAULT".to_string());
    let loose = is_loose.unwrap_or(false);
    let in_lib = in_library.unwrap_or(true);
    let historical = is_historical.unwrap_or(false);

    // ═══════════════════════════════════════════════════════════════════
    //  STEP 1: Create the MPSC channel — the SINGLE data pipe.
    // ═══════════════════════════════════════════════════════════════════
    let (tx, rx) = std::sync::mpsc::channel::<FilmItem>();

    // ═══════════════════════════════════════════════════════════════════
    //  STEP 2: Spawn the consumer thread — ABSOLUTE single-writer to SQLite.
    //  Adaptive Flush: first 5 items flush immediately (instant UI feedback),
    //  then batch every 15 items for SQLite throughput.
    // ═══════════════════════════════════════════════════════════════════
    let app_handle_consumer = app_handle.clone();
    let paths_consumer = paths.clone();
    let roll_id_consumer = roll_id.clone();
    let import_total = Arc::new(AtomicUsize::new(paths_consumer.len()));
    let import_total_consumer = import_total.clone();

    std::thread::spawn(move || {
        let mut conn = match rusqlite::Connection::open(get_db_path()) {
            Ok(c) => {
                c.busy_timeout(std::time::Duration::from_secs(5)).ok();
                c
            }
            Err(e) => {
                eprintln!("[Import Consumer] Failed to open DB: {}", e);
                return;
            }
        };

        let state = app_handle_consumer.state::<EngineState>();
        let mut buffer: Vec<FilmItem> = Vec::new();
        let mut total_processed: usize = 0;
        let total_for_progress = import_total_consumer.clone();

        // ── Helper: flush a batch to SQLite + emit events ──
        let flush_batch = |batch: &mut Vec<FilmItem>,
                            conn: &mut rusqlite::Connection,
                            state: &EngineState,
                            app: &tauri::AppHandle,
                            processed: &mut usize,
                            total_for_progress: &Arc<AtomicUsize>| {
            if batch.is_empty() {
                return;
            }
            let items = std::mem::take(batch);

            // Single transaction for the entire batch
            let tx = match conn.transaction() {
                Ok(t) => t,
                Err(e) => {
                    eprintln!("[Import Consumer] Transaction begin failed: {}", e);
                    // Still emit events so the frontend isn't starved
                    for item in items {
                        let payload = serde_json::json!({
                            "id": item.id.clone(),
                            "roll_id": item.roll_id.clone(),
                            "thumbnail_base64": item.thumbnail_base64.clone(),
                            "file_path": item.file_path.clone(),
                            "processed": *processed + 1,
                            "total": total_for_progress.load(Ordering::SeqCst),
                        });
                        state.items.insert(item.id.clone(), Arc::new(RwLock::new(item)));
                        let _ = app.emit("import_progress", payload);
                        *processed += 1;
                    }
                    return;
                }
            };

            for item in items.iter() {
                if item.is_loose {
                    continue;
                }
                let params_str = serde_json::to_string(&item.params).unwrap_or_default();
                let geom_str = serde_json::to_string(&item.geom).unwrap_or_default();
                let base_color_str = serde_json::to_string(&item.base_color).unwrap_or_default();
                if let Err(e) = tx.execute(
                    "INSERT INTO image_states (roll_id, file_path, thumbnail_base64, params, geom, base_color)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                     ON CONFLICT(roll_id, file_path) DO UPDATE SET
                     thumbnail_base64=excluded.thumbnail_base64,
                     params=excluded.params,
                     geom=excluded.geom,
                     base_color=excluded.base_color",
                    rusqlite::params![item.roll_id, item.file_path, item.thumbnail_base64, params_str, geom_str, base_color_str],
                ) {
                    eprintln!("[Import Consumer] SQLite insert failed: {}", e);
                }
            }

            if let Err(e) = tx.commit() {
                eprintln!("[Import Consumer] Transaction commit failed: {}", e);
            }

            // Emit events AFTER commit so frontend sees consistent state
            for item in items {
                let payload = serde_json::json!({
                    "id": item.id.clone(),
                    "roll_id": item.roll_id.clone(),
                    "thumbnail_base64": item.thumbnail_base64.clone(),
                    "file_path": item.file_path.clone(),
                    "processed": *processed + 1,
                    "total": total_for_progress.load(Ordering::SeqCst),
                });
                state.items.insert(item.id.clone(), Arc::new(RwLock::new(item)));
                let _ = app.emit("import_progress", payload);
                *processed += 1;
            }
        };

        // ── Micro-batch recv loop: flush every 3 items for smooth UI ──
        // A roll of film typically has 3-6 frames per strip; flushing every 3
        // ensures the frontend grid updates like water flowing, eliminating 0% deadlock.
        while let Ok(item) = rx.recv() {
            buffer.push(item);

            let flush_threshold = if total_processed <= 2 { 1 } else { 3 };
            if buffer.len() >= flush_threshold {
                flush_batch(
                    &mut buffer,
                    &mut conn,
                    &state,
                    &app_handle_consumer,
                    &mut total_processed,
                    &total_for_progress,
                );
            }
        }

        // ── Flush remaining items after channel closes ──
        flush_batch(
            &mut buffer,
            &mut conn,
            &state,
            &app_handle_consumer,
            &mut total_processed,
            &total_for_progress,
        );

        // ── Update item_order for filmstrip ordering ──
        {
            if let Ok(mut order_guard) = state.item_order.write() {
                for path in paths_consumer {
                    let id_opt = {
                        let guard = state.items.clone();
                        let mut found = None;
                        let target = roll_id_consumer.clone().unwrap_or_else(|| "LOOSE_DEFAULT".to_string());
                        for kv in guard.iter() {
                            let item = kv.value().read().unwrap();
                            let db_path = item.file_path.clone();
                            if item.roll_id == target
                                && (db_path == path
                                    || db_path.replace("\\", "/").to_lowercase()
                                        == path.replace("\\", "/").to_lowercase())
                            {
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
        }

        // ── Emit completion event ──
        let _ = app_handle_consumer.emit("import_complete", serde_json::json!({ "total": total_processed }));
    });

    // ═══════════════════════════════════════════════════════════════════
    //  STEP 3: Spawn the producer thread.
    //  Dedup + DB cache + bounded Rayon pool (4 threads) for thumbnail extraction.
    //  ALL heavy I/O and computation is isolated here — main thread returns in µs.
    // ═══════════════════════════════════════════════════════════════════
    let app_handle_producer = app_handle.clone();
    let target_roll_producer = target_roll.clone();

    std::thread::spawn(move || {
        // ── Phase 1: Fast dedup — in-memory HashSet against DashMap (lock-free reads) ──
        let state = app_handle_producer.state::<EngineState>();
        let existing_items_by_path: std::collections::HashMap<String, String> = {
            let guard = state.items.clone();
            guard.iter().filter_map(|kv| {
                let item = kv.value().read().unwrap();
                if item.roll_id == target_roll_producer {
                    Some((item.file_path.replace("\\", "/").to_lowercase(), kv.key().clone()))
                } else {
                    None
                }
            }).collect()
        };

        let selected_paths = paths;
        let paths_to_refresh: Vec<(String, String)> = selected_paths
            .iter()
            .filter_map(|path| {
                let normalized = path.replace("\\", "/").to_lowercase();
                if is_direct_image_extension(path) {
                    existing_items_by_path
                        .get(&normalized)
                        .map(|id| (path.clone(), id.clone()))
                } else {
                    None
                }
            })
            .collect();
        let paths_to_process: Vec<String> = selected_paths.into_iter()
            .filter(|p| !existing_items_by_path.contains_key(&p.replace("\\", "/").to_lowercase()))
            .collect();

        let total = paths_to_process.len();
        import_total.store(total, Ordering::SeqCst);

        // ── Emit initial progress so frontend knows import started ──
        let _ = app_handle_producer.emit("import_progress", serde_json::json!({
            "phase": "start",
            "total": total,
        }));

        for (path, id) in paths_to_refresh {
            if let Some(thumbnail) = decode_preview_base64(&path, 256) {
                if let Some(item_arc) = state.items.get(&id) {
                    let (roll_id, file_path, params_str, geom_str, base_color_str) = {
                        let mut item = write_lock(&item_arc);
                        item.thumbnail_base64 = thumbnail.clone();
                        (
                            item.roll_id.clone(),
                            item.file_path.clone(),
                            serde_json::to_string(&item.params).unwrap_or_default(),
                            serde_json::to_string(&item.geom).unwrap_or_default(),
                            serde_json::to_string(&item.base_color).unwrap_or_default(),
                        )
                    };

                    if !loose {
                        if let Ok(conn) = rusqlite::Connection::open(get_db_path()) {
                            conn.busy_timeout(std::time::Duration::from_secs(5)).ok();
                            let _ = conn.execute(
                                "INSERT INTO image_states (roll_id, file_path, thumbnail_base64, params, geom, base_color)
                                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                                 ON CONFLICT(roll_id, file_path) DO UPDATE SET
                                 thumbnail_base64=excluded.thumbnail_base64",
                                rusqlite::params![roll_id, file_path, thumbnail, params_str, geom_str, base_color_str],
                            );
                        }
                    }

                    let _ = app_handle_producer.emit("thumbnail_updated", serde_json::json!({
                        "id": id,
                        "thumbnail": thumbnail,
                    }));
                }
            }
        }

        if total == 0 {
            // tx drops when this closure returns → consumer recv() returns Err →
            // consumer emits import_complete with total_processed=0
            return;
        }

        // ── Phase 2: Build DB cache for instant re-import of already-processed images ──
        let db_cache: std::collections::HashMap<
            String,
            (String, TuningParams, crate::app_state::GeometryState, BaseColor),
        > = if !loose {
            let mut cache = std::collections::HashMap::new();
            if let Ok(conn) = rusqlite::Connection::open(get_db_path()) {
                conn.busy_timeout(std::time::Duration::from_secs(5)).ok();
                if let Ok(mut stmt) = conn.prepare(
                    "SELECT file_path, thumbnail_base64, params, geom, base_color FROM image_states WHERE roll_id = ?1",
                ) {
                    if let Ok(rows) = stmt.query_map(rusqlite::params![&target_roll_producer], |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, String>(2)?,
                            row.get::<_, String>(3)?,
                            row.get::<_, String>(4)?,
                        ))
                    }) {
                        for row in rows.flatten() {
                            let (fp, thumb, params_str, geom_str, bc_str) = row;
                            if let (Ok(params), Ok(geom), Ok(bc)) = (
                                serde_json::from_str(&params_str),
                                serde_json::from_str(&geom_str),
                                serde_json::from_str(&bc_str),
                            ) {
                                cache.insert(
                                    fp.replace("\\", "/").to_lowercase(),
                                    (thumb, params, geom, bc),
                                );
                            }
                        }
                    }
                }
            }
            cache
        } else {
            std::collections::HashMap::new()
        };

        // ── Helper: process a single path into a FilmItem ──
        // ONLY uses libraw_unpack_thumb (lazy demosaicing) — never full unpack.
        // All captures are immutable references → safe for parallel invocation.
        let process_path = |path: &String| -> FilmItem {
            // ── Fast path: hit the DB cache (no libraw decoding needed) ──
            if !loose {
                if let Some((thumb, params, geom, base_color)) =
                    db_cache.get(&path.replace("\\", "/").to_lowercase())
                {
                    let id = format!("img_{}", NEXT_ID.fetch_add(1, Ordering::SeqCst));
                    let thumbnail_base64 = if is_direct_image_extension(path) {
                        decode_preview_base64(path, 256).unwrap_or_else(|| thumb.clone())
                    } else {
                        thumb.clone()
                    };
                    return FilmItem {
                        id,
                        roll_id: target_roll_producer.clone(),
                        file_path: path.clone(),
                        thumbnail_base64,
                        original_proxy: None,
                        proxy_image: None,
                        pristine_proxy: None,
                        base_color: base_color.clone(),
                        params: params.clone(),
                        geom: geom.clone(),
                        is_loose: loose,
                        in_library: in_lib,
                    };
                }
                // Historical mode: skip items not already in DB
                if historical {
                    let id = format!("img_{}", NEXT_ID.fetch_add(1, Ordering::SeqCst));
                    return FilmItem {
                        id,
                        roll_id: target_roll_producer.clone(),
                        file_path: path.clone(),
                        thumbnail_base64: FALLBACK_THUMB.to_string(),
                        original_proxy: None,
                        proxy_image: None,
                        pristine_proxy: None,
                        base_color: BaseColor {
                            base_r: 32768,
                            base_g: 32768,
                            base_b: 32768,
                        },
                        params: TuningParams::default(),
                        geom: crate::app_state::GeometryState::default(),
                        is_loose: loose,
                        in_library: in_lib,
                    };
                }
            }

            let thumbnail_base64 = decode_preview_base64(path, 256)
                .unwrap_or_else(|| FALLBACK_THUMB.to_string());
            let id = format!("img_{}", NEXT_ID.fetch_add(1, Ordering::SeqCst));
            let params = TuningParams::default();

            FilmItem {
                id,
                roll_id: target_roll_producer.clone(),
                file_path: path.clone(),
                thumbnail_base64,
                original_proxy: None,
                proxy_image: None,
                pristine_proxy: None,
                base_color: BaseColor::default(),
                params,
                geom: crate::app_state::GeometryState::default(),
                is_loose: loose,
                in_library: in_lib,
            }
        };

        // ═══════════════════════════════════════════════════════
        //  Phase 3: SERIAL for loop — ultra-safe for OS.
        //  NO Rayon, NO parallel disk I/O. One file at a time.
        //  Each item is sent immediately via channel for micro-batch consumption.
        //  A roll of film is at most ~40 frames; serial processing takes
        //  seconds, not minutes, and keeps the OS completely responsive.
        // ═══════════════════════════════════════════════════════
        for path in &paths_to_process {
            let item = process_path(path);
            if tx.send(item).is_err() {
                break;
            }
        }
        // tx drops here → consumer's rx.recv() returns Err →
        // consumer flushes remaining buffer and emits import_complete
    });

    // ═══════════════════════════════════════════════════════════════════
    //  STEP 4: RETURN IMMEDIATELY — unblock Tauri IPC in microseconds.
    //  This is the SINGLE most critical line for fixing "stuck at 0%" and
    //  OS-level stuttering. Both threads run independently from here on.
    // ═══════════════════════════════════════════════════════════════════
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
                    roll_id: item.roll_id.clone(),
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
                if item.roll_id == roll_id && (db_path == *path || db_path.replace("\\", "/").to_lowercase() == path.replace("\\", "/").to_lowercase()) {
                    strip.push(FilmstripItem {
                        id: item.id.clone(),
                        roll_id: item.roll_id.clone(),
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
    let mut thumbs = Vec::with_capacity(paths.len());
    for path in paths {
        thumbs.push(
            decode_preview_base64(&path, 256)
                .unwrap_or_else(|| FALLBACK_THUMB.to_string()),
        );
    }
    Ok(thumbs)
}

#[tauri::command]
pub async fn get_embedded_preview(id: String, state: State<'_, EngineState>) -> Result<String, String> {
    let file_path = {
        let item_arc = state.items.get(&id).ok_or("Image ID not found")?;
        let item = read_lock(&item_arc);
        if std::fs::File::open(&item.file_path).is_err() {
            return Err("FILE_MISSING".into());
        }
        item.file_path.clone()
    };

    tokio::task::spawn_blocking(move || {
        decode_develop_preview_base64(&file_path, 2560)
            .unwrap_or_else(|| FALLBACK_THUMB.to_string())
    })
    .await
    .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn load_dcp_profile(path: String, state: State<'_, EngineState>) -> Result<(), String> {
    *write_lock(&state.dcp_profile) = Some(path.clone());
    if let Some(active_id) = read_lock(&state.active_id).clone() {
        if let Some(item_arc) = state.items.get(&active_id) {
            let mut item = write_lock(&item_arc);
            item.original_proxy = None;
            item.proxy_image = None;
            item.pristine_proxy = None;
        }
    }
    Ok(())
}

#[tauri::command]
pub async fn set_working_colorspace(colorspace: String, state: State<'_, EngineState>) -> Result<(), String> {
    *write_lock(&state.working_colorspace) = colorspace.clone();
    if let Some(active_id) = read_lock(&state.active_id).clone() {
        if let Some(item_arc) = state.items.get(&active_id) {
            let mut item = write_lock(&item_arc);
            item.original_proxy = None;
            item.proxy_image = None;
            item.pristine_proxy = None;
        }
    }
    Ok(())
}

#[derive(serde::Serialize)]
pub struct ActiveImageState {
    pub params: TuningParams,
    pub geom: crate::app_state::GeometryState,
}

// ═══════════════════════════════════════════════════════════════════════════
//  LRU Proxy Cache — strict capacity enforcement (max 3 proxies in memory)
// ═══════════════════════════════════════════════════════════════════════════

/// Evict the oldest proxy data from memory if the LRU cache exceeds MAX_PROXY_CACHE.
/// Physically drops the ImageBuffer allocations (~72MB per evicted image).
fn evict_proxy_if_needed(state: &EngineState) {
    let active_id = read_lock(&state.active_id).clone();
    let protected_ids: HashSet<String> = {
        let mut ids = HashSet::new();
        if let Some(active) = active_id.as_ref() {
            ids.insert(active.clone());
            let order = read_lock(&state.item_order);
            if let Some(pos) = order.iter().position(|id| id == active) {
                let start = pos.saturating_sub(2);
                let end = (pos + 3).min(order.len());
                for id in order[start..end].iter() {
                    ids.insert(id.clone());
                }
            }
        }
        ids
    };
    let mut order = write_lock(&state.proxy_loaded_order);
    while order.len() > crate::app_state::MAX_PROXY_CACHE {
        let victim_pos = order
            .iter()
            .position(|id| !protected_ids.contains(id));
        let Some(victim_pos) = victim_pos else { break; };
        if let Some(oldest_id) = order.remove(victim_pos) {
            if let Some(item_arc) = state.items.get(&oldest_id) {
                let mut item = write_lock(&item_arc);
                item.original_proxy = None;
                item.proxy_image = None;
                item.pristine_proxy = None;
            }
        }
    }
}

/// Mark an image's proxy as loaded and move it to the back of the LRU order.
/// Triggers eviction if the cache exceeds capacity.
fn track_proxy_loaded(state: &EngineState, id: &str) {
    let mut order = write_lock(&state.proxy_loaded_order);
    order.retain(|x| x != id);
    order.push_back(id.to_string());
    drop(order);
    evict_proxy_if_needed(state);
}

#[tauri::command]
pub async fn switch_active_image(id: String, roll_id: String, state: State<'_, EngineState>, _app_handle: tauri::AppHandle) -> Result<ActiveImageState, String> {
    let _ = roll_id; // Unused, but required for correct JSON payload mapping from JS

    // Pure state switch: never trigger RAW unpack/demosaic here.
    {
        let item_arc = state.items.get(&id).ok_or("Image ID not found")?;
        let item = item_arc.read().map_err(|e| e.to_string())?;
        if std::fs::File::open(&item.file_path).is_err() {
            return Err("FILE_MISSING".into());
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
pub async fn prepare_proxy(id: String, state: State<'_, EngineState>) -> Result<(), String> {
    let item_arc = state.items.get(&id).ok_or("Image ID not found")?.clone();

    let (file_path, needs_base_color, existing_base_color, has_proxy, dcp_profile, colorspace) = {
        let item = read_lock(&item_arc);
        if std::fs::File::open(&item.file_path).is_err() {
            return Err("FILE_MISSING".into());
        }
        let needs = item.base_color.base_r == 32768
            && item.base_color.base_g == 32768
            && item.base_color.base_b == 32768;
        (
            item.file_path.clone(),
            needs,
            item.base_color.clone(),
            item.proxy_image.is_some(),
            read_lock(&state.dcp_profile).clone(),
            read_lock(&state.working_colorspace).clone(),
        )
    };

    if has_proxy {
        track_proxy_loaded(&state, &id);
        return Ok(());
    }

    let loaded = tokio::task::spawn_blocking(move || -> Result<_, String> {
        let img_buffer = load_image_buffer(&file_path, true, dcp_profile.as_deref(), &colorspace)?;
        let (width, height) = img_buffer.dimensions();
        let ratio_proxy = (PROXY_LONG_EDGE / (width.max(height) as f32)).min(1.0);
        let proxy_width = (width as f32 * ratio_proxy).max(1.0) as u32;
        let proxy_height = (height as f32 * ratio_proxy).max(1.0) as u32;
        let proxy = if ratio_proxy < 0.999 {
            image::imageops::resize(&img_buffer, proxy_width, proxy_height, FilterType::Triangle)
        } else {
            img_buffer
        };
        let bc = if needs_base_color {
            compute_auto_base(&proxy)
        } else {
            existing_base_color
        };
        Ok((proxy, bc))
    })
    .await
    .map_err(|e| e.to_string())??;

    {
        let mut item = write_lock(&item_arc);
        let original_proxy = loaded.0;
        if needs_base_color {
            item.base_color = loaded.1;
        }
        let has_spatial_transform = item.geom.angle.abs() > 0.01
            || item.geom.rotate_90_count.rem_euclid(4) != 0
            || item.geom.flip_h
            || item.geom.flip_v;
        if has_spatial_transform {
            let (proxy, pristine) = compute_geometry_and_pristine(
                &original_proxy,
                &item.geom,
                &item.base_color,
                item.params.film_mode.clone(),
            );
            item.original_proxy = Some(original_proxy);
            item.proxy_image = Some(proxy);
            item.pristine_proxy = Some(pristine);
        } else {
            item.original_proxy = Some(original_proxy.clone());
            item.proxy_image = Some(original_proxy);
            item.pristine_proxy = None;
        }
        let _ = save_image_state_to_db(&item);
    }

    track_proxy_loaded(&state, &id);
    Ok(())
}

#[tauri::command]
pub async fn set_film_mode(id: String, mode: String, state: State<'_, EngineState>) -> Result<(), String> {
    if let Some(item_arc) = state.items.get(&id) {
        let mut item = write_lock(&item_arc);
        let new_mode = if mode == "B&W" { FilmMode::BW } else { FilmMode::Color };
        if item.params.film_mode != new_mode {
            item.params.film_mode = new_mode.clone();
            let pipeline = FilmPipeline::new(
                [item.base_color.base_r, item.base_color.base_g, item.base_color.base_b],
                [0.0, 0.0, 0.0],
                new_mode,
            );
            
            let Some(proxy) = item.proxy_image.as_ref() else {
                return Err("PROXY_NOT_READY".into());
            };
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
        {
            let mut item = item_arc.write().map_err(|e| e.to_string())?;
            if item.pristine_proxy.is_none() {
                if let Some(proxy) = item.proxy_image.as_ref() {
                    item.pristine_proxy = Some(compute_pristine_proxy(
                        proxy,
                        &item.base_color,
                        item.params.film_mode.clone(),
                    ));
                }
            }
        }
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
pub async fn set_thumbnail_data(id: String, thumbnail: String, state: State<'_, EngineState>) -> Result<(), String> {
    if let Some(item_arc) = state.items.get(&id) {
        let mut item = write_lock(&item_arc);
        item.thumbnail_base64 = thumbnail;
        let _ = save_image_state_to_db(&item);
    }
    Ok(())
}

#[tauri::command]
pub async fn update_geometry(id: String, geom: crate::app_state::GeometryState, state: State<'_, EngineState>) -> Result<(), String> {
    if let Some(item_arc) = state.items.get(&id) {
        let mut item = write_lock(&item_arc);
        item.geom = geom;
        if item.original_proxy.is_some() {
            reapply_geometry(&mut item)?;
        }
        let _ = save_image_state_to_db(&item);
    }
    Ok(())
}

fn reapply_geometry(item: &mut FilmItem) -> Result<(), String> {
    let Some(original_proxy) = item.original_proxy.as_ref() else {
        return Err("PROXY_NOT_READY".into());
    };
    let (proxy, pristine) = compute_geometry_and_pristine(
        original_proxy,
        &item.geom,
        &item.base_color,
        item.params.film_mode.clone(),
    );
    item.proxy_image = Some(proxy);
    item.pristine_proxy = Some(pristine);
    Ok(())
}

/// Standalone geometry application — does NOT require a write lock on FilmItem.
/// Returns (proxy_image, pristine_proxy) for the caller to assign under lock.
fn compute_geometry_and_pristine(
    original_proxy: &ImageBuffer<Rgb<u16>, Vec<u16>>,
    geom: &crate::app_state::GeometryState,
    base_color: &BaseColor,
    film_mode: FilmMode,
) -> (ImageBuffer<Rgb<u16>, Vec<u16>>, ImageBuffer<Rgb<f32>, Vec<f32>>) {
    let mut current = original_proxy.clone();

    if geom.angle.abs() > 0.01 {
        let angle_rad = geom.angle.to_radians();
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

    match geom.rotate_90_count.rem_euclid(4) {
        1 => current = image::imageops::rotate90(&current),
        2 => current = image::imageops::rotate180(&current),
        3 => current = image::imageops::rotate270(&current),
        _ => {}
    }

    if geom.flip_h {
        current = image::imageops::flip_horizontal(&current);
    }
    if geom.flip_v {
        current = image::imageops::flip_vertical(&current);
    }

    let pristine = compute_pristine_proxy(&current, base_color, film_mode);
    (current, pristine)
}

#[tauri::command]
pub async fn geometry_auto_align(id: String, state: State<'_, EngineState>) -> Result<crate::app_state::AutoAlignResult, String> {
    let item_arc = state.items.get(&id).ok_or("Image not found")?.clone();
    
    let (crop_rect, angle) = tokio::task::spawn_blocking(move || -> Result<_, String> {
        let original_proxy = {
            let item = read_lock(&item_arc);
            item.original_proxy.clone().ok_or_else(|| "PROXY_NOT_READY".to_string())?
        };
        
        let first_result = crate::geometry::auto_crop_rect(&original_proxy)?;
        
        let proxy_image = {
            let mut item = write_lock(&item_arc);
            item.geom.angle = first_result.angle;
            reapply_geometry(&mut item)?;
            item.proxy_image.clone().ok_or_else(|| "PROXY_NOT_READY".to_string())?
        };
        
        let second_result = crate::geometry::auto_crop_rect(&proxy_image)?;
        
        let mut item = write_lock(&item_arc);
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
    _app_handle: tauri::AppHandle,
) -> Result<tauri::ipc::Response, String> {
    let out_buffer: Option<Vec<u8>> = {
        let item_arc = state.items.get(&id).ok_or("Image ID not found")?;
        let item = read_lock(&item_arc);
        if let Some(proxy) = item.proxy_image.as_ref() {
            Some(build_response_buffer_from_proxy(proxy, &item.base_color, true))
        } else {
            None
        }
    }; // read lock released immediately

    let result = if let Some(out_buffer) = out_buffer {
        track_proxy_loaded(&state, &id);
        Ok(tauri::ipc::Response::new(out_buffer))
    } else {
        Err("PROXY_NOT_READY".into())
    };

    result
}

#[tauri::command]
pub async fn is_proxy_ready(id: String, state: State<'_, EngineState>) -> Result<bool, String> {
    let item_arc = state.items.get(&id).ok_or("Image ID not found")?;
    let item = read_lock(&item_arc);
    Ok(item.original_proxy.is_some() && item.proxy_image.is_some())
}

/// Front-end-driven pre-fetch command.
/// Called by the JS after it finishes rendering image N, with the IDs of N+1 and N-1.
/// This is the Lr-class strategy: the frontend knows exactly which image is "next"
/// in the user's navigation flow, and triggers pre-loading at the lowest priority.
#[tauri::command]
pub async fn prefetch_proxy(id: String, app_handle: tauri::AppHandle) -> Result<(), String> {
    eprintln!("[Prefetch] Frontend requested prefetch of {}", id);
    enqueue_proxy_job(app_handle, id, false);
    Ok(())
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
    state: State<'_, EngineState>,
    app_handle: tauri::AppHandle
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
    
    crate::commands::import_images(paths, Some(false), Some(true), Some(roll_id_clone), Some(false), state, app_handle).await
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
    state: State<'_, EngineState>,
    app_handle: tauri::AppHandle
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
    crate::commands::import_images(paths, Some(false), Some(true), Some(roll_id), Some(false), state, app_handle).await
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
    conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA busy_timeout=5000;")?;
    conn.execute("CREATE TABLE IF NOT EXISTS user_cameras (id INTEGER PRIMARY KEY, name TEXT NOT NULL UNIQUE)", [])?;
    conn.execute("CREATE TABLE IF NOT EXISTS user_films (id INTEGER PRIMARY KEY, name TEXT NOT NULL UNIQUE)", [])?;
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
    conn.busy_timeout(std::time::Duration::from_secs(5)).ok();
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
                let mut _order_guard = state.item_order.write().unwrap();
                for row_result in rows {
                    if let Ok((db_roll_id, db_path, thumb, params_str, geom_str, base_color_str)) = row_result {
                        let params = serde_json::from_str(&params_str).unwrap_or_default();
                        let geom = serde_json::from_str(&geom_str).unwrap_or_default();
                        let base_color = serde_json::from_str(&base_color_str).unwrap_or_default();
                        
                        let is_loose_item = db_roll_id == "LOOSE_DEFAULT";
                        
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
                            is_loose: is_loose_item,
                            in_library: is_loose_item, // Only loose items appear in Library; roll items are viewed via History Films
                        };
                        state.items.insert(img_id.clone(), std::sync::Arc::new(std::sync::RwLock::new(item)));
                        _order_guard.push(img_id);
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
    let highlights = params.tone.highlights;
    let shadows = params.tone.shadows;

    pristine_pixels.par_chunks(3).zip(out_pixels.par_chunks_mut(3)).for_each(|(in_px, out_px)| {
        let true_density = [in_px[0], in_px[1], in_px[2]];
        let density = pipeline.apply_exposure(&true_density);

        // Normalize with NaN protection (d_max == d_min → division by zero)
        let eps = 1e-6_f32;
        let norm_r = if (d_max[0] - d_min[0]).abs() > eps {
            ((density[0] - d_min[0]) / (d_max[0] - d_min[0])).clamp(0.0, 1.0)
        } else { 0.5 };
        let norm_g = if (d_max[1] - d_min[1]).abs() > eps {
            ((density[1] - d_min[1]) / (d_max[1] - d_min[1])).clamp(0.0, 1.0)
        } else { 0.5 };
        let norm_b = if (d_max[2] - d_min[2]).abs() > eps {
            ((density[2] - d_min[2]) / (d_max[2] - d_min[2])).clamp(0.0, 1.0)
        } else { 0.5 };

        // ── Apply highlights/shadows (same formula as WebGL shader) ──
        let n_r = norm_r.clamp(0.0, 1.0);
        let n_g = norm_g.clamp(0.0, 1.0);
        let n_b = norm_b.clamp(0.0, 1.0);
        let final_r = n_r + shadows * (1.0 - n_r).powi(2) * n_r + highlights * n_r.powi(2) * (1.0 - n_r);
        let final_g = n_g + shadows * (1.0 - n_g).powi(2) * n_g + highlights * n_g.powi(2) * (1.0 - n_g);
        let final_b = n_b + shadows * (1.0 - n_b).powi(2) * n_b + highlights * n_b.powi(2) * (1.0 - n_b);

        out_px[0] = (final_r.powf(1.0 / gamma) * 255.0) as u8;
        out_px[1] = (final_g.powf(1.0 / gamma) * 255.0) as u8;
        out_px[2] = (final_b.powf(1.0 / gamma) * 255.0) as u8;
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
    let thumb = image::imageops::resize(&cropped_thumb, thumb_width, thumb_height, FilterType::Nearest);
    
    let mut cursor = std::io::Cursor::new(Vec::new());
    if let Ok(_) = thumb.write_to(&mut cursor, image::ImageOutputFormat::Jpeg(70)) {
        use base64::{Engine as _, engine::general_purpose};
        return Some(general_purpose::STANDARD.encode(cursor.into_inner()));
    }
    None
}
