#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use nexfilm_engine::app_state::EngineState;


fn main() {
    tauri::Builder::default()
        .manage(EngineState::new())
        .invoke_handler(tauri::generate_handler![
            nexfilm_engine::commands::open_file_dialog,
            nexfilm_engine::commands::select_export_dir,
            nexfilm_engine::commands::import_images,
            nexfilm_engine::commands::get_filmstrip,
            nexfilm_engine::commands::switch_active_image,
            nexfilm_engine::commands::get_proxy_image_data,
            nexfilm_engine::commands::update_tuning_parameters,
            nexfilm_engine::commands::batch_export_images,
            nexfilm_engine::commands::set_film_mode,
            nexfilm_engine::commands::sync_thumbnail_buffer,
            nexfilm_engine::commands::update_geometry,
            nexfilm_engine::commands::geometry_auto_align,
            nexfilm_engine::commands::load_3d_lut,
            nexfilm_engine::commands::load_dcp_profile,
            nexfilm_engine::commands::set_working_colorspace,
            nexfilm_engine::commands::open_lut_dialog,
            nexfilm_engine::commands::open_dcp_dialog,
            nexfilm_engine::commands::get_builtin_luts,
            nexfilm_engine::commands::get_builtin_dcps
        ])
        .plugin(tauri_plugin_dialog::init())
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
