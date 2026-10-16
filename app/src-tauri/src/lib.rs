mod mcsr;
mod panel;
mod settings;
mod shell;
mod sync;
mod upload;

use tauri::Manager;

use mcsr::Client;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        // Launched at login with --background: start hidden in the tray.
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            Some(vec!["--background"]),
        ))
        .manage(Client::new())
        .manage(shell::PanelState::default())
        .manage(sync::Engine::new())
        .setup(|app| {
            let dir = app.path().app_config_dir()?;
            app.manage(settings::Store::load(dir));
            shell::build(app)?;
            tauri::async_runtime::spawn(sync::run(app.handle().clone()));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            shell::titlebar,
            shell::sync_status,
            panel::panel_init,
            panel::lookup_player,
            panel::add_player,
            panel::remove_player,
            panel::reorder_players,
            panel::set_keep_copy,
            panel::choose_folder,
            panel::set_start_with_windows,
            panel::set_run_in_tray,
            panel::set_paused,
            panel::close_panel,
            panel::open_link,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
