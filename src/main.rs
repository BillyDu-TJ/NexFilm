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
            nexfilm_engine::commands::apply_tuning_parameters,
            nexfilm_engine::commands::batch_export_images
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
