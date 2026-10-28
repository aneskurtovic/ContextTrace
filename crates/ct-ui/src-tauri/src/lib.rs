mod commands;

fn focus_main_window(app: &tauri::AppHandle) {
    use tauri::Manager;
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

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

    #[cfg(all(target_os = "macos", feature = "macos-updater"))]
    {
        let Ok(executable) = std::env::current_exe() else {
            return false;
        };
        let home = std::env::var_os("HOME").map(std::path::PathBuf::from);
        is_installed_macos_executable(&executable, home.as_deref())
    }

    #[cfg(not(any(
        target_os = "windows",
        all(target_os = "macos", feature = "macos-updater")
    )))]
    false
}

// Restrict updates to copies installed in a conventional Applications folder.
// This excludes a mounted DMG and source-build bundles, where replacement may
// fail or update the wrong copy. The release feature is a separate requirement.
#[cfg(any(test, all(target_os = "macos", feature = "macos-updater")))]
fn is_installed_macos_executable(
    executable: &std::path::Path,
    home: Option<&std::path::Path>,
) -> bool {
    let Some(macos) = executable.parent() else {
        return false;
    };
    let Some(contents) = macos.parent() else {
        return false;
    };
    let Some(bundle) = contents.parent() else {
        return false;
    };
    let Some(parent) = bundle.parent() else {
        return false;
    };
    macos.file_name().is_some_and(|name| name == "MacOS")
        && contents.file_name().is_some_and(|name| name == "Contents")
        && bundle
            .file_name()
            .is_some_and(|name| name == "ContextTrace.app")
        && (parent == std::path::Path::new("/Applications")
            || home.is_some_and(|dir| parent == dir.join("Applications")))
}

#[tauri::command]
fn restart_after_update(app: tauri::AppHandle) {
    app.restart();
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        // Must be the first plugin registered: it is the plugin that decides
        // whether this process should keep running at all, so every other
        // plugin's setup would otherwise happen in a process we are about to
        // hand off to and exit.
        .plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            focus_main_window(app);
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
            restart_after_update,
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
        .build(tauri::generate_context!())
        .expect("failed to build ContextTrace desktop")
        .run(|_app, _event| {
            #[cfg(target_os = "macos")]
            if let tauri::RunEvent::Reopen { .. } = _event {
                focus_main_window(_app);
            }
        });
}

#[cfg(test)]
mod tests {
    use super::is_installed_macos_executable;
    use std::path::Path;

    #[test]
    fn mac_updates_require_an_app_in_an_applications_directory() {
        let home = Path::new("/Users/example");
        assert!(is_installed_macos_executable(
            Path::new("/Applications/ContextTrace.app/Contents/MacOS/context-trace"),
            Some(home)
        ));
        assert!(is_installed_macos_executable(
            Path::new("/Users/example/Applications/ContextTrace.app/Contents/MacOS/context-trace"),
            Some(home)
        ));
        assert!(!is_installed_macos_executable(
            Path::new("/Volumes/ContextTrace/ContextTrace.app/Contents/MacOS/context-trace"),
            Some(home)
        ));
        assert!(!is_installed_macos_executable(
            Path::new(
                "/work/target/release/bundle/macos/ContextTrace.app/Contents/MacOS/context-trace"
            ),
            Some(home)
        ));
        assert!(!is_installed_macos_executable(
            Path::new("/Applications/Other.app/Contents/MacOS/context-trace"),
            Some(home)
        ));
    }
}
