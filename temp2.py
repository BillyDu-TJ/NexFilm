import sys
import re

with open('src/commands.rs', 'r', encoding='utf-8') as f:
    content = f.read()

# 1. Update update_tuning_parameters
update_tuning_old = '''pub async fn update_tuning_parameters(
    id: String,
    params: TuningParams,
    state: State<'_, EngineState>,
) -> Result<(), String> {
    if let Some(item_arc) = state.items.get(&id) {
        let mut item = item_arc.write().map_err(|e| e.to_string())?;
        item.params = params;
    }
    Ok(())
}'''

update_tuning_new = '''pub async fn update_tuning_parameters(
    id: String,
    params: TuningParams,
    state: State<'_, EngineState>,
) -> Result<(), String> {
    if let Some(item_arc) = state.items.get(&id) {
        let mut item = item_arc.write().map_err(|e| e.to_string())?;
        item.params = params;
        let _ = save_image_state_to_db(&item);
    }
    Ok(())
}'''
content = content.replace(update_tuning_old, update_tuning_new)

# 2. Update geometry updates to also save
crop_rot_old = '''item.geom.rotate_90_count = rotate_count;
            item.geom.flip_h = flip_h;
            item.geom.flip_v = flip_v;'''
crop_rot_new = '''item.geom.rotate_90_count = rotate_count;
            item.geom.flip_h = flip_h;
            item.geom.flip_v = flip_v;
            let _ = save_image_state_to_db(&item);'''
content = content.replace(crop_rot_old, crop_rot_new)

auto_cal_old = '''item.geom.calibration_points = Some(pts);
        }
    }
    Ok(())'''
auto_cal_new = '''item.geom.calibration_points = Some(pts);
            let _ = save_image_state_to_db(&item);
        }
    }
    Ok(())'''
content = content.replace(auto_cal_old, auto_cal_new)

# 3. Update switch_active_image to lazy load proxies
switch_old = '''pub async fn switch_active_image(id: String, state: State<'_, EngineState>) -> Result<ActiveImageState, String> {
    if let Some(item_arc) = state.items.get(&id) {
        let item = item_arc.read().map_err(|e| e.to_string())?;
        if std::fs::File::open(&item.file_path).is_err() {
            return Err("FILE_MISSING".into());
        }
        *state.active_id.write().map_err(|e| e.to_string())? = Some(id.clone());
        Ok(ActiveImageState {
            params: item.params.clone(),
            geom: item.geom.clone(),
        })
    } else {
        Err("Image ID not found".into())
    }
}'''

switch_new = '''pub async fn switch_active_image(id: String, state: State<'_, EngineState>) -> Result<ActiveImageState, String> {
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
}'''
content = content.replace(switch_old, switch_new)

# 4. Replace remaining option unwraps in commands.rs for proxies
# `.proxy_image` -> `.proxy_image.as_ref().unwrap()` if we are just reading, but wait:
# let proxy = &item.proxy_image; => let proxy = item.proxy_image.as_ref().unwrap();
# item.proxy_image = current; => item.proxy_image = Some(current);
# item.original_proxy.clone() => item.original_proxy.clone().unwrap()
# etc.

content = content.replace('let proxy = &item.proxy_image;', 'let proxy = item.proxy_image.as_ref().unwrap();')
content = content.replace('item.original_proxy = proxy.clone();', 'item.original_proxy = Some(proxy.clone());')
content = content.replace('item.proxy_image = proxy;', 'item.proxy_image = Some(proxy);')
content = content.replace('item.pristine_proxy = pristine;', 'item.pristine_proxy = Some(pristine);')
content = content.replace('let pristine = &item.pristine_proxy;', 'let pristine = item.pristine_proxy.as_ref().unwrap();')
content = content.replace('let mut current = item.original_proxy.clone();', 'let mut current = item.original_proxy.clone().unwrap();')
content = content.replace('item.proxy_image = current;', 'item.proxy_image = Some(current);')
content = content.replace('compute_pristine_proxy(&item.proxy_image,', 'compute_pristine_proxy(item.proxy_image.as_ref().unwrap(),')
content = content.replace('item.original_proxy.clone()', 'item.original_proxy.clone().unwrap()')
content = content.replace('item.proxy_image.clone()', 'item.proxy_image.clone().unwrap()')


with open('src/commands.rs', 'w', encoding='utf-8') as f:
    f.write(content)

print("Done")
