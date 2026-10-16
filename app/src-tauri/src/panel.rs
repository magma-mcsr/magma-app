//! Commands for the settings panel. Every command rejects calls that do not come from
//! the panel webview.

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State, Webview};
use tauri_plugin_autostart::ManagerExt as _;
use tauri_plugin_dialog::DialogExt as _;
use tauri_plugin_opener::OpenerExt as _;

use crate::mcsr::{Client, Player};
use crate::settings::{Settings, Store, TrackedPlayer};
use crate::shell::{self, SyncStatus, PANEL_LABEL};

fn guard(webview: &Webview) -> Result<(), String> {
    if webview.label() == PANEL_LABEL {
        Ok(())
    } else {
        Err("not allowed".into())
    }
}

/// Save a change, then tell the panel and the title bar.
fn apply(app: &AppHandle, store: &Store, f: impl FnOnce(&mut Settings)) -> Result<Settings, String> {
    let ((), settings) = store.update(f)?;
    let _ = app.emit_to(PANEL_LABEL, "settings-changed", settings.clone());
    shell::broadcast_status(app, &settings);
    // Players added, sync resumed, or copy turned on: start a round right away.
    app.state::<crate::sync::Engine>().wake();
    Ok(settings)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PanelInit {
    settings: Settings,
    status: SyncStatus,
    version: &'static str,
    default_folder: Option<String>,
}

#[tauri::command]
pub fn panel_init(webview: Webview, store: State<'_, Store>) -> Result<PanelInit, String> {
    guard(&webview)?;
    let settings = store.get();
    Ok(PanelInit {
        status: shell::status_for(&settings, &webview.app_handle().state::<crate::sync::Engine>().live()),
        settings,
        version: env!("CARGO_PKG_VERSION"),
        default_folder: default_folder(webview.app_handle()),
    })
}

fn default_folder(app: &AppHandle) -> Option<String> {
    app.path().document_dir().ok().map(|d| d.join("MCSR Replays").to_string_lossy().into_owned())
}

#[tauri::command]
pub async fn lookup_player(webview: Webview, client: State<'_, Client>, name: String) -> Result<Player, String> {
    guard(&webview)?;
    client.player(name.trim()).await
}

#[tauri::command]
pub fn add_player(webview: Webview, store: State<'_, Store>, player: TrackedPlayer) -> Result<Settings, String> {
    guard(&webview)?;
    apply(webview.app_handle(), &store, |s| {
        if !s.players.iter().any(|p| p.uuid == player.uuid) {
            s.players.push(player);
        }
    })
}

#[tauri::command]
pub fn remove_player(webview: Webview, store: State<'_, Store>, uuid: String) -> Result<Settings, String> {
    guard(&webview)?;
    apply(webview.app_handle(), &store, |s| s.players.retain(|p| p.uuid != uuid))
}

/// New order of players, given as UUIDs. Unknown UUIDs are ignored.
#[tauri::command]
pub fn reorder_players(webview: Webview, store: State<'_, Store>, uuids: Vec<String>) -> Result<Settings, String> {
    guard(&webview)?;
    apply(webview.app_handle(), &store, |s| {
        let mut ordered: Vec<TrackedPlayer> = uuids.iter().filter_map(|u| s.players.iter().find(|p| &p.uuid == u).cloned()).collect();
        for p in &s.players {
            if !ordered.contains(p) {
                ordered.push(p.clone());
            }
        }
        s.players = ordered;
    })
}

#[tauri::command]
pub fn set_keep_copy(webview: Webview, store: State<'_, Store>, enabled: bool) -> Result<Settings, String> {
    guard(&webview)?;
    let fallback = default_folder(webview.app_handle());
    apply(webview.app_handle(), &store, |s| {
        s.keep_copy = enabled;
        if enabled && s.copy_folder.is_none() {
            s.copy_folder = fallback;
        }
    })
}

#[tauri::command]
pub async fn choose_folder(webview: Webview, store: State<'_, Store>) -> Result<Settings, String> {
    guard(&webview)?;
    let app = webview.app_handle().clone();
    let (tx, rx) = tokio::sync::oneshot::channel();
    let mut dialog = app.dialog().file().set_title("Choose a folder for replays");
    if let Some(current) = store.get().copy_folder {
        dialog = dialog.set_directory(current);
    }
    dialog.pick_folder(move |picked| {
        let _ = tx.send(picked);
    });
    let picked = rx.await.map_err(|e| e.to_string())?;
    match picked.and_then(|p| p.into_path().ok()) {
        Some(path) => apply(&app, &store, |s| s.copy_folder = Some(path.to_string_lossy().into_owned())),
        None => Ok(store.get()),
    }
}

#[tauri::command]
pub fn set_start_with_windows(webview: Webview, store: State<'_, Store>, enabled: bool) -> Result<Settings, String> {
    guard(&webview)?;
    let app = webview.app_handle();
    let launcher = app.autolaunch();
    if enabled { launcher.enable() } else { launcher.disable() }.map_err(|e| e.to_string())?;
    apply(app, &store, |s| s.start_with_windows = enabled)
}

#[tauri::command]
pub fn set_run_in_tray(webview: Webview, store: State<'_, Store>, enabled: bool) -> Result<Settings, String> {
    guard(&webview)?;
    apply(webview.app_handle(), &store, |s| s.run_in_tray = enabled)
}

#[tauri::command]
pub fn set_paused(webview: Webview, store: State<'_, Store>, paused: bool) -> Result<Settings, String> {
    guard(&webview)?;
    apply(webview.app_handle(), &store, |s| s.paused = paused)
}

#[tauri::command]
pub fn close_panel(webview: Webview) -> Result<(), String> {
    guard(&webview)?;
    shell::set_panel(webview.app_handle(), false);
    Ok(())
}

/// Open a link in the system browser. Only Magma and the project's own pages are allowed.
#[tauri::command]
pub fn open_link(webview: Webview, url: String) -> Result<(), String> {
    guard(&webview)?;
    let allowed = ["https://magmamcsr.com/", "https://github.com/"];
    if !allowed.iter().any(|p| url.starts_with(p)) {
        return Err("link not allowed".into());
    }
    webview.app_handle().opener().open_url(url, None::<&str>).map_err(|e| e.to_string())
}

