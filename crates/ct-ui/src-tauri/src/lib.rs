mod commands;

#[tauri::command]
fn is_installed_build() -> bool {
    #[cfg(target_os = "windows")]
    {
        let Ok(executable) = std::env::current_exe() else {
            return false;
        };
        let Some(install_directory) = executable.parent() else {
            return false;
        };
        install_directory.join("uninstall.exe").is_file()
    }

    #[cfg(not(target_os = "windows"))]
    false
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        // Must be the first plugin registered: it is the plugin that decides
        // whether this process should keep running at all, so every other
        // plugin's setup would otherwise happen in a process we are about to
        // hand off to and exit.
        .plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            use tauri::Manager;
            if let Some(window) = app.webview_windows().values().next() {
                let _ = window.set_focus();
            }
        }))
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .manage(commands::AppState::new())
        .manage(commands::notifications::NotificationState::new())
        .setup(|app| {
            commands::notifications::start_monitor(app.handle().clone());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            is_installed_build,
            commands::get_startup,
            commands::search_sessions,
            commands::list_projects,
            commands::search_memory,
            commands::inspect_session,
            commands::get_context,
            commands::get_corpus,
            commands::get_corpus_cached,
            commands::get_transcript,
            commands::get_transcript_entry,
            commands::run_doctor,
            commands::get_lifecycle,
            commands::get_compaction_diff,
            commands::get_turn_diff,
            commands::get_instruction_files,
            commands::get_cost,
            commands::get_temporal_ghost,
            commands::get_residual,
            commands::archived_sessions,
            commands::archive_session,
            commands::verify_archived,
            commands::export_session,
            commands::notifications::get_notification_settings,
            commands::notifications::update_notification_settings,
            commands::notifications::get_notification_status,
            commands::notifications::send_test_notification,
            commands::notifications::list_notifications,
            commands::notifications::mark_notifications_read,
            commands::notifications::dismiss_notification,
            commands::notifications::clear_notification_history
        ])
        .run(tauri::generate_context!())
        .expect("failed to run ContextTrace desktop");
}
