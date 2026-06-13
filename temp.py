import sys
import re

with open('src/commands.rs', 'r', encoding='utf-8') as f:
    content = f.read()

# 1. Update init_db
old_init = '''pub fn init_db() -> rusqlite::Result<()> {
    let conn = rusqlite::Connection::open(get_db_path())?;
    conn.execute("CREATE TABLE IF NOT EXISTS user_cameras (id INTEGER PRIMARY KEY, name TEXT NOT NULL UNIQUE)", [])?;
    conn.execute("CREATE TABLE IF NOT EXISTS user_films (id INTEGER PRIMARY KEY, name TEXT NOT NULL UNIQUE)", [])?;
    Ok(())
}'''

new_init = '''pub fn init_db() -> rusqlite::Result<()> {
    let conn = rusqlite::Connection::open(get_db_path())?;
    conn.execute("CREATE TABLE IF NOT EXISTS user_cameras (id INTEGER PRIMARY KEY, name TEXT NOT NULL UNIQUE)", [])?;
    conn.execute("CREATE TABLE IF NOT EXISTS user_films (id INTEGER PRIMARY KEY, name TEXT NOT NULL UNIQUE)", [])?;
    conn.execute("CREATE TABLE IF NOT EXISTS image_states (
        file_path TEXT PRIMARY KEY,
        thumbnail_base64 TEXT,
        params TEXT,
        geom TEXT,
        base_color TEXT
    )", [])?;
    Ok(())
}

pub fn save_image_state_to_db(item: &crate::app_state::FilmItem) -> Result<(), String> {
    let conn = rusqlite::Connection::open(get_db_path()).map_err(|e| e.to_string())?;
    let params_str = serde_json::to_string(&item.params).map_err(|e| e.to_string())?;
    let geom_str = serde_json::to_string(&item.geom).map_err(|e| e.to_string())?;
    let base_color_str = serde_json::to_string(&item.base_color).map_err(|e| e.to_string())?;
    
    conn.execute(
        "INSERT INTO image_states (file_path, thumbnail_base64, params, geom, base_color) 
         VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT(file_path) DO UPDATE SET 
         params=excluded.params, 
         geom=excluded.geom, 
         base_color=excluded.base_color",
        rusqlite::params![item.file_path, item.thumbnail_base64, params_str, geom_str, base_color_str]
    ).map_err(|e| e.to_string())?;
    Ok(())
}

pub fn load_image_state_from_db(file_path: &str) -> Option<(String, crate::app_state::TuningParams, crate::app_state::GeometryState, crate::app_state::BaseColor)> {
    let conn = rusqlite::Connection::open(get_db_path()).ok()?;
    let mut stmt = conn.prepare("SELECT thumbnail_base64, params, geom, base_color FROM image_states WHERE file_path = ?1").ok()?;
    
    let mut rows = stmt.query(rusqlite::params![file_path]).ok()?;
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
'''
content = content.replace(old_init, new_init)

import_logic = '''
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
'''

content = re.sub(r'let mut new_items = Vec::new();\s*for chunk in paths.chunks\(4\).*?new_items.extend\(chunk_items_result\?\);\s*\}', import_logic, content, flags=re.DOTALL)

with open('src/commands.rs', 'w', encoding='utf-8') as f:
    f.write(content)
