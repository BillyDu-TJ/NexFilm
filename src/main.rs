#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use nexfilm_engine::app_state::EngineState;
use tauri::http::{
    header::{ACCESS_CONTROL_ALLOW_ORIGIN, CACHE_CONTROL, CONTENT_TYPE},
    HeaderValue, Response, StatusCode,
};
use tauri::Manager;

fn proxy_protocol_response(
    status: StatusCode,
    body: Vec<u8>,
    content_type: &'static str,
) -> Response<Vec<u8>> {
    let mut response = Response::new(body);
    *response.status_mut() = status;
    response
        .headers_mut()
        .insert(CONTENT_TYPE, HeaderValue::from_static(content_type));
    response
        .headers_mut()
        .insert(CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
        .headers_mut()
        .insert(ACCESS_CONTROL_ALLOW_ORIGIN, HeaderValue::from_static("*"));
    response
}

fn show_fatal_error(message: &str) {
    let _ = rfd::MessageDialog::new()
        .set_title("NexFilm could not start")
        .set_description(message)
        .set_level(rfd::MessageLevel::Error)
        .show();
}

fn main() {
    if let Err(error) = nexfilm_engine::commands::init_db() {
        show_fatal_error(&format!(
            "Failed to initialize the NexFilm database:\n\n{error}"
        ));
        return;
    }
    nexfilm_engine::commands::init_background_limits();

    let state = EngineState::new();
    if let Err(error) = nexfilm_engine::commands::load_all_image_states(&state) {
        show_fatal_error(&format!("Failed to restore saved image state:\n\n{error}"));
        return;
    }
    if let Err(error) = nexfilm_engine::commands::load_all_rolls(&state) {
        show_fatal_error(&error);
        return;
    }

    let app_result = tauri::Builder::default()
        .manage(state)
        .register_asynchronous_uri_scheme_protocol(
            "nexfilm-proxy",
            |context, request, responder| {
                let app_handle = context.app_handle().clone();
                let id = request.uri().path().trim_start_matches('/').to_string();
                std::thread::spawn(move || {
                    let state = app_handle.state::<EngineState>();
                    let response =
                        match nexfilm_engine::commands::get_proxy_response_buffer(&state, &id) {
                            Ok(buffer) => proxy_protocol_response(
                                StatusCode::OK,
                                buffer,
                                "application/octet-stream",
                            ),
                            Err(error) => proxy_protocol_response(
                                if error == "PROXY_NOT_READY" {
                                    StatusCode::CONFLICT
                                } else {
                                    StatusCode::NOT_FOUND
                                },
                                error.into_bytes(),
                                "text/plain; charset=utf-8",
                            ),
                        };
                    responder.respond(response);
                });
            },
        )
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
            nexfilm_engine::commands::analyze_proxy_density_limits,
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
            nexfilm_engine::commands::delete_images,
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
        .run(tauri::generate_context!());

    if let Err(error) = app_result {
        show_fatal_error(&format!(
            "NexFilm stopped because of an application error:\n\n{error}"
        ));
    }
}
