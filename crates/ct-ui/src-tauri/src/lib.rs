mod commands;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .manage(commands::AppState::new())
        .invoke_handler(tauri::generate_handler![
            commands::get_startup,
            commands::list_sessions,
            commands::search_sessions,
            commands::inspect_session,
            commands::get_context,
            commands::run_doctor,
            commands::get_lifecycle
        ])
        .run(tauri::generate_context!())
        .expect("failed to run ContextTrace desktop");
}
