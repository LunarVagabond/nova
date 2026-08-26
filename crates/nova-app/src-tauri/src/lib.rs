mod commands;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![
            commands::open_project,
            commands::validate_project,
            commands::send_request,
            commands::read_request,
            commands::save_request,
            commands::create_request,
            commands::parse_curl_command,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
