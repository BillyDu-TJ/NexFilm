#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use nexfilm_engine::app_state::EngineState;

fn main() {
    nexfilm_engine::commands::init_db().expect("failed to initialize NexFilm database");
    nexfilm_engine::commands::init_background_limits();

    let state = EngineState::new();
    nexfilm_engine::commands::load_all_image_states(&state);
    nexfilm_engine::commands::load_all_rolls(&state).expect("failed to load NexFilm roll metadata");

    tauri::Builder::default()
        .manage(state)
        .invoke_handler(tauri::generate_handler![
            nexfilm_engine::commands::open_file_dialog,
            nexfilm_engine::commands::select_export_dir,
            nexfilm_engine::commands::import_images,
            nexfilm_engine::commands::get_filmstrip,
            nexfilm_engine::commands::get_roll_filmstrip,
            nexfilm_engine::commands::get_raw_thumbnails,
            nexfilm_engine::commands::get_embedded_preview,
            nexfilm_engine::commands::promote_roll,
            nexfilm_engine::commands::switch_active_image,
            nexfilm_engine::commands::prepare_proxy,
            nexfilm_engine::commands::analyze_proxy_base_color,
            nexfilm_engine::commands::get_proxy_image_data,
            nexfilm_engine::commands::update_tuning_parameters,
            nexfilm_engine::commands::batch_export_images,
            nexfilm_engine::commands::sync_thumbnail_buffer,
            nexfilm_engine::commands::set_thumbnail_data,
            nexfilm_engine::commands::update_geometry,
            nexfilm_engine::commands::auto_detect_film_border,
            nexfilm_engine::commands::batch_copy_settings,
            nexfilm_engine::commands::geometry_auto_align,
            nexfilm_engine::commands::load_3d_lut,
            nexfilm_engine::commands::open_lut_dialog,
            nexfilm_engine::commands::get_builtin_luts,
            nexfilm_engine::commands::get_rolls,
            nexfilm_engine::commands::import_roll,
            nexfilm_engine::commands::delete_rolls,
            nexfilm_engine::commands::update_roll_metadata,
            nexfilm_engine::commands::save_contact_sheet,
            nexfilm_engine::commands::append_to_roll,
            nexfilm_engine::commands::locate_missing_file,
            nexfilm_engine::commands::get_user_cameras,
            nexfilm_engine::commands::get_user_films,
            nexfilm_engine::commands::add_user_camera,
            nexfilm_engine::commands::add_user_film,
            nexfilm_engine::commands::get_roll_previews
        ])
        .plugin(tauri_plugin_dialog::init())
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
