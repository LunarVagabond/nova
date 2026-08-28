mod commands;
mod mock_server;
mod session_store;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .manage(nova_engine::GitStatusCache::new())
        .manage(session_store::SessionStore::new())
        .manage(mock_server::MockServerState::new())
        .invoke_handler(tauri::generate_handler![
            commands::open_project,
            commands::init_project,
            commands::validate_project,
            commands::git_status,
            commands::send_request,
            commands::get_history,
            commands::reopen_history_entry,
            commands::read_request,
            commands::save_request,
            commands::parse_multipart_body,
            commands::serialize_multipart_body,
            commands::save_manifest,
            commands::create_request,
            commands::delete_request,
            commands::rename_request,
            commands::duplicate_request,
            commands::parse_curl_command,
            commands::create_collection,
            commands::rename_collection,
            commands::delete_collection,
            commands::create_environment,
            commands::save_environment,
            commands::delete_environment,
            commands::run_tests,
            commands::import_project,
            commands::export_project,
            commands::mock_server_status,
            commands::start_mock_server,
            commands::stop_mock_server,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
