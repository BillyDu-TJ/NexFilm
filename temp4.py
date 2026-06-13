import sys
import re

with open('src/commands.rs', 'r', encoding='utf-8') as f:
    content = f.read()

# Replace the whole import_images function
old_fn = re.search(r'pub async fn import_images\(.*?\) -> Result<\(\), String> \{.*?let mut order_guard = state\.item_order\.write\(\)\.map_err\(\|e\| e\.to_string\(\)\)\?;.*?for item in new_items \{.*?\}\n\n    Ok\(\(\)\)\n\}', content, re.DOTALL)

if old_fn:
    new_fn = '''pub async fn import_images(paths: Vec<String>, state: State<'_, EngineState>) -> Result<(), String> {
    if paths.is_empty() {
        return Ok(());
    }

    let dcp_profile = state.dcp_profile.read().unwrap().clone();
    let colorspace = state.working_colorspace.read().unwrap().clone();

    let mut new_items = Vec::new();
    let existing_paths: std::collections::HashSet<String> = {
        let guard = state.items.clone();
        guard.iter().map(|kv| kv.value().read().unwrap().file_path.clone()).collect()
    };
    
    let paths_to_process: Vec<String> = paths.into_iter().filter(|p| !existing_paths.contains(p)).collect();

    for chunk in paths_to_process.chunks(4) {
        let chunk_items_result: Result<Vec<FilmItem>, String> = chunk.into_par_iter().map(|path| {
            if let Some((thumb, params, geom, base_color)) = load_image_state_from_db(path) {
                let id = format!("img_{}", NEXT_ID.fetch_add(1, Ordering::SeqCst));
                return Ok(FilmItem {
                    id,
                    file_path: path.clone(),
                    thumbnail_base64: thumb,
                    original_proxy: None,
                    proxy_image: None,
                    pristine_proxy: None,
                    base_color,
                    params,
                    geom,
                });
            }

            let img_buffer = load_image_buffer(path, true, dcp_profile.as_deref(), &colorspace)?;
            let (width, height) = img_buffer.dimensions();
            let ratio_proxy = 2048.0 / (width.max(height) as f32);
            let proxy_width = (width as f32 * ratio_proxy).max(1.0) as u32;
            let proxy_height = (height as f32 * ratio_proxy).max(1.0) as u32;
            let proxy = image::imageops::resize(&img_buffer, proxy_width, proxy_height, FilterType::Triangle);
            
            let ratio_thumb = 1024.0 / (width.max(height) as f32);
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

            let item = FilmItem {
                id,
                file_path: path.clone(),
                thumbnail_base64,
                original_proxy: Some(proxy.clone()),
                proxy_image: Some(proxy),
                pristine_proxy: Some(pristine_proxy),
                base_color,
                params: TuningParams::default(),
                geom: crate::app_state::GeometryState::default(),
            };
            
            let _ = save_image_state_to_db(&item);
            
            Ok(item)
        }).collect();
        new_items.extend(chunk_items_result?);
    }

    let mut order_guard = state.item_order.write().map_err(|e| e.to_string())?;
    for item in new_items {
        let id = item.id.clone();
        state.items.insert(id.clone(), Arc::new(RwLock::new(item)));
        order_guard.push(id);
    }

    Ok(())
}'''
    content = content[:old_fn.start()] + new_fn + content[old_fn.end():]
else:
    print("Could not find import_images")

with open('src/commands.rs', 'w', encoding='utf-8') as f:
    f.write(content)
