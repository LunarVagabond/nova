mod commands;
mod mock_server;
mod session_store;
mod websocket_session;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .manage(nova_engine::GitStatusCache::new())
        .manage(session_store::SessionStore::new())
        .manage(mock_server::MockServerState::new())
        .manage(websocket_session::WebSocketSessionState::new())
        .invoke_handler(tauri::generate_handler![
            commands::open_project,
            commands::init_project,
            commands::validate_project,
            commands::git_status,
            commands::send_request,
            commands::export_request_as,
            commands::get_resolved_variables,
            commands::get_history,
            commands::reopen_history_entry,
            commands::get_cookies,
            commands::delete_cookie,
            commands::clear_cookies,
            commands::update_cookie,
            commands::diff_against_previous_run,
            commands::diff_against_example_response,
            commands::read_request,
            commands::save_request,
            commands::read_websocket_request,
            commands::save_websocket_request,
            commands::connect_websocket,
            commands::create_websocket_request,
            commands::connect_websocket_session,
            commands::send_websocket_session_message,
            commands::disconnect_websocket_session,
            commands::websocket_session_status,
            commands::parse_multipart_body,
            commands::serialize_multipart_body,
            commands::parse_graphql_body_text,
            commands::serialize_graphql_body,
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
