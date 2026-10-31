mod autostart;
mod game;
mod mcsr;
mod network;
mod notify;
mod packaged;
mod panel;
mod report;
mod placement;
mod settings;
mod shell;
mod sync;
mod update;
mod upload;

use tauri::Manager;

use mcsr::Client;

/// The app's windows need the WebView2 Runtime. Windows 11 and most Windows 10 PCs have it, and the
/// installer build installs it, but a Store install cannot. Without it, say so and offer the
/// download page rather than fail with an error nobody can act on.
fn webview_runtime_ready() -> bool {
    if tauri::webview_version().is_ok() {
        return true;
    }
    #[cfg(windows)]
    if !packaged::started_at_login() {
        use windows_sys::Win32::UI::WindowsAndMessaging::{MessageBoxW, IDYES, MB_ICONINFORMATION, MB_SETFOREGROUND, MB_YESNO};
        let wide = |s: &str| s.encode_utf16().chain(std::iter::once(0)).collect::<Vec<u16>>();
        let text = wide("Magma MCSR needs the Microsoft WebView2 Runtime, and this computer doesn't have it.\n\nOpen Microsoft's download page? After installing it, open Magma MCSR again.");
        let title = wide("Magma MCSR");
        let answer = unsafe { MessageBoxW(std::ptr::null_mut(), text.as_ptr(), title.as_ptr(), MB_YESNO | MB_ICONINFORMATION | MB_SETFOREGROUND) };
        if answer == IDYES {
            let _ = tauri_plugin_opener::open_url("https://developer.microsoft.com/microsoft-edge/webview2/", None::<&str>);
        }
    }
    false
}

/// Someone who moved to the Store build and kept this one would otherwise run both after every
/// restart, downloading everything twice. The Store build cannot reach this startup entry, so this
/// build removes it, and says once that it can be uninstalled.
fn step_aside_for_store_build(app: &tauri::AppHandle) {
    if cfg!(debug_assertions) || !packaged::store_build_installed() {
        return;
    }
    if let Err(e) = autostart::disable() {
        eprintln!("[autostart] {e}");
    }
    let store = app.state::<settings::Store>();
    if store.get().store_build_notice_shown {
        return;
    }
    let _ = store.update(|s| s.store_build_notice_shown = true);
    use tauri_plugin_dialog::DialogExt;
    app.dialog()
        .message("The Microsoft Store version of Magma MCSR is on this computer too, so this version no longer starts with Windows. You can uninstall it in Windows Settings > Apps.")
        .title("Magma MCSR")
        .show(|_| {});
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    if !webview_runtime_ready() {
        return;
    }
    let builder = tauri::Builder::default();
    // One app at a time (registered first, as the plugin asks). A second launch hands over to the
    // running one and exits: opened by hand, it shows the window; started with Windows
    // (--background), nothing. Two would download and upload everything twice (0.2.0 after a
    // restart: one from the startup entry, one reopened by Windows). Not in development builds,
    // which would otherwise hand over to an installed app with the same identifier.
    #[cfg(not(debug_assertions))]
    let builder = builder.plugin(tauri_plugin_single_instance::init(|app, args, _cwd| {
        if !args.iter().any(|a| a == "--background") {
            shell::show_main(app);
        }
    }));
    builder
        .plugin(tauri_plugin_dialog::init())
        // No injected link handler: the website has no IPC access, and shell.rs decides
        // where new-window links open.
        .plugin(tauri_plugin_opener::Builder::new().open_js_links_on_click(false).build())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .manage(Client::new())
        .manage(shell::PanelState::default())
        .manage(shell::PanelPage::default())
        .manage(shell::WindowMemory::default())
        .manage(shell::LastDownload::default())
        .manage(shell::NoticeState::default())
        .manage(sync::Engine::new())
        .manage(panel::SetupPending(std::sync::atomic::AtomicBool::new(true)))
        .manage(update::Updates::new())
        .setup(|app| {
            let dir = settings::app_dir(app.handle())?;
            app.manage(settings::Store::load(dir));
            app.state::<update::Updates>().note_version(&app.state::<settings::Store>());
            std::thread::spawn(update::clean_old_installers);
            // Rewrite the startup entry on every launch: it then always points at this
            // install, quoted (versions before 0.1.2 wrote it without quotes). A packaged app
            // has no entry to rewrite: Windows keeps that setting, and the user may have turned
            // it off there.
            if !packaged::is_packaged() && !packaged::store_build_installed() && app.state::<settings::Store>().get().start_with_windows {
                if let Err(e) = autostart::enable() {
                    eprintln!("[autostart] {e}");
                }
            }
            step_aside_for_store_build(app.handle());
            shell::setup(app)?;
            tauri::async_runtime::spawn(panel::auto_setup(app.handle().clone()));
            tauri::async_runtime::spawn(sync::run(app.handle().clone()));
            tauri::async_runtime::spawn(update::run(app.handle().clone()));
            tauri::async_runtime::spawn(report::run(app.handle().clone()));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            shell::titlebar,
            shell::sync_status,
            shell::app_view_open,
            shell::update_status,
            shell::download_fly,
            shell::notice_mode,
            shell::notice_state,
            shell::notice_action,
            panel::show_whats_new,
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
            panel::set_notify_analyzed,
            panel::set_share_stats,
            panel::check_now,
            panel::detect_accounts,
            panel::show_player_matches,
            panel::dismiss_welcome,
            panel::recent_own,
            panel::ack_setup_notice,
            panel::undo_auto_track,
            panel::replace_first_player,
            panel::network_options,
            panel::set_network,
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
