mod mcsr;
mod panel;
mod settings;
mod shell;
mod sync;
mod update;
mod upload;

use tauri::Manager;

use mcsr::Client;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        // No injected link handler: the website has no IPC access, and shell.rs decides
        // where new-window links open.
        .plugin(tauri_plugin_opener::Builder::new().open_js_links_on_click(false).build())
        .plugin(tauri_plugin_updater::Builder::new().build())
        // Launched at login with --background: start hidden in the tray.
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            Some(vec!["--background"]),
        ))
        .manage(Client::new())
        .manage(shell::PanelState::default())
        .manage(shell::WindowMemory::default())
        .manage(shell::LastDownload::default())
        .manage(sync::Engine::new())
        .manage(update::Updates::new())
        .setup(|app| {
            let dir = app.path().app_config_dir()?;
            app.manage(settings::Store::load(dir));
            shell::setup(app)?;
            tauri::async_runtime::spawn(sync::run(app.handle().clone()));
            tauri::async_runtime::spawn(update::run(app.handle().clone()));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            shell::titlebar,
            shell::sync_status,
            shell::update_status,
            shell::download_fly,
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
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app, event| {
            // Closing the window does not quit when the app runs in the tray: sync keeps
            // going in the background. Quit from the tray menu passes an exit code.
            if let tauri::RunEvent::ExitRequested { code: None, api, .. } = event {
                if app.state::<settings::Store>().get().run_in_tray {
                    api.prevent_exit();
                }
            }
        });
}
