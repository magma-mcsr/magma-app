//! Commands for the settings panel. Every command rejects calls that do not come from
//! the panel webview.

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State, Webview};
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
    /// The first-run setup is still looking for this computer's Minecraft name.
    setup_pending: bool,
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
        setup_pending: webview.app_handle().state::<SetupPending>().0.load(std::sync::atomic::Ordering::SeqCst),
        status: shell::status_for(&settings, &webview.app_handle().state::<crate::sync::Engine>().live()),
        settings,
        version: env!("CARGO_PKG_VERSION"),
        default_folder: default_folder(webview.app_handle()),
    })
}

/// Where copies go unless the user picks a folder: the game's own replay folder, so the
/// replays show up under "My Replays" in MCSR Ranked. Falls back to Documents\MCSR Replays.
fn default_folder(app: &AppHandle) -> Option<String> {
    game_replay_folder(app)
        .or_else(|| app.path().document_dir().ok().map(|d| d.join("MCSR Replays")))
        .map(|p| p.to_string_lossy().into_owned())
}

/// The `mcsrranked\replay` folder of an MCSR Ranked install, if one exists. Looks in the
/// usual places of the common launchers; with several, takes the most recently used.
fn game_replay_folder(app: &AppHandle) -> Option<std::path::PathBuf> {
    let roots = crate::game::roots(app);
    find_game_replay_folder(roots.roaming.as_deref(), roots.home.as_deref(), &roots.drives)
}

/// `roaming` is %APPDATA%, `home` the user's home folder (separate so tests can use fake ones).
/// The launchers searched are the ones in game.rs; the most recently played instance wins.
fn find_game_replay_folder(roaming: Option<&std::path::Path>, home: Option<&std::path::Path>, drives: &[std::path::PathBuf]) -> Option<std::path::PathBuf> {
    crate::game::instances(roaming, home, drives)
        .into_iter()
        .map(|d| d.join("mcsrranked").join("replay"))
        .find(|p| p.is_dir())
}

#[cfg(test)]
mod tests {
    use super::find_game_replay_folder;
    use std::path::{Path, PathBuf};

    /// A fresh empty folder standing in for %APPDATA% and the home folder.
    fn fake_root(name: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("magma-app-test-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        root
    }

    fn make(root: &Path, rel: &str) -> PathBuf {
        let p = root.join(rel);
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    #[test]
    fn finds_each_launcher() {
        for (name, rel, in_home) in [
            ("prism", "PrismLauncher/instances/MCSR Ranked/minecraft/mcsrranked/replay", false),
            ("prism-dot", "PrismLauncher/instances/Ranked/.minecraft/mcsrranked/replay", false),
            ("modrinth", "ModrinthApp/profiles/MCSR/mcsrranked/replay", false),
            ("theseus", "com.modrinth.theseus/profiles/MCSR/mcsrranked/replay", false),
            ("vanilla", ".minecraft/mcsrranked/replay", false),
            ("curseforge", "curseforge/minecraft/Instances/MCSR/mcsrranked/replay", true),
        ] {
            let root = fake_root(name);
            let expected = make(&root, rel);
            let found = if in_home { find_game_replay_folder(None, Some(&root), &[]) } else { find_game_replay_folder(Some(&root), None, &[]) };
            assert_eq!(found.map(|p| p.components().collect::<PathBuf>()), Some(expected.components().collect::<PathBuf>()), "{name}");
            let _ = std::fs::remove_dir_all(&root);
        }
    }

    #[test]
    fn nothing_found_without_the_mod_folder() {
        let root = fake_root("empty");
        make(&root, "PrismLauncher/instances/Vanilla 1.21/minecraft/saves");
        assert_eq!(find_game_replay_folder(Some(&root), Some(&root), &[]), None);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn several_installs_pick_the_most_recent() {
        let root = fake_root("several");
        make(&root, "PrismLauncher/instances/Old/minecraft/mcsrranked/replay");
        std::thread::sleep(std::time::Duration::from_millis(1100));
        let newer = make(&root, "PrismLauncher/instances/New/minecraft/mcsrranked/replay");
        let found = find_game_replay_folder(Some(&root), None, &[]).unwrap();
        assert_eq!(found.components().collect::<PathBuf>(), newer.components().collect::<PathBuf>());
        let _ = std::fs::remove_dir_all(&root);
    }
}

/// Minecraft names that played MCSR Ranked on this computer and have a Ranked profile, for the
/// first-run suggestion (see game.rs for what is read). At most three are looked up.
#[tauri::command]
pub async fn detect_accounts(webview: Webview, client: State<'_, Client>) -> Result<Vec<Player>, String> {
    guard(&webview)?;
    let roots = crate::game::roots(webview.app_handle());
    let names = crate::game::account_names(&crate::game::instances(roots.roaming.as_deref(), roots.home.as_deref(), &roots.drives));
    let mut found = Vec::new();
    for name in names.into_iter().take(3) {
        if let Ok(p) = client.player(&name).await {
            found.push(p);
        }
    }
    Ok(found)
}

/// Load a player's match list (or one match) on the website. With `show`, switch to the website.
#[tauri::command]
pub fn show_player_matches(webview: Webview, nickname: String, match_id: Option<i64>, show: Option<bool>) -> Result<(), String> {
    guard(&webview)?;
    if nickname.is_empty() || nickname.len() > 16 || !nickname.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
        return Err("invalid name".into());
    }
    let path = match match_id {
        Some(id) => format!("/ranked/player/{nickname}/matches/{id}"),
        None => format!("/ranked/player/{nickname}/matches"),
    };
    let url = format!("{}{path}", crate::shell::SITE_URL).parse().map_err(|e| format!("{e}"))?;
    let app = webview.app_handle();
    let site = app.get_webview(crate::shell::SITE_LABEL).ok_or("no website view")?;
    site.navigate(url).map_err(|e| e.to_string())?;
    if show == Some(true) {
        shell::set_panel(app, false);
    }
    Ok(())
}

/// True while the first-run setup is still looking for this computer's Minecraft name.
#[derive(Default)]
pub struct SetupPending(pub std::sync::atomic::AtomicBool);

/// First run, before anything is set: track the Minecraft name found on this computer right away,
/// and tell the user in the App tab what that does, with "Change name" and "Turn off" (see the
/// README). Runs once: removing or renaming the player later is respected. Without a name found,
/// nothing is added and the App tab asks for one.
pub async fn auto_setup(app: AppHandle) {
    use std::sync::atomic::Ordering;
    let pending = &app.state::<SetupPending>().0;
    let store = app.state::<Store>();
    let s = store.get();
    if s.auto_setup_done || s.welcome_dismissed || !s.players.is_empty() {
        pending.store(false, Ordering::SeqCst);
        let _ = app.emit("setup-done", ());
        return;
    }
    let roots = crate::game::roots(&app);
    let names = crate::game::account_names(&crate::game::instances(roots.roaming.as_deref(), roots.home.as_deref(), &roots.drives));
    let client = app.state::<Client>();
    let mut found = None;
    for name in names.into_iter().take(3) {
        if let Ok(p) = client.player(&name).await {
            found = Some(p);
            break;
        }
    }
    if let Some(p) = found {
        let added = apply(&app, &store, |s| {
            if s.players.is_empty() {
                s.players.push(TrackedPlayer { uuid: p.uuid.clone(), nickname: p.nickname.clone() });
                s.auto_tracked_notice = true;
            }
            s.auto_setup_done = true;
        });
        if added.is_ok() {
            app.state::<crate::sync::Engine>().wake();
            // Ready on the website for when they switch to it
            if let (Some(site), Ok(url)) = (
                app.get_webview(crate::shell::SITE_LABEL),
                format!("{}/ranked/player/{}/matches", crate::shell::SITE_URL, p.nickname).parse(),
            ) {
                let _ = site.navigate(url);
            }
        }
    }
    pending.store(false, Ordering::SeqCst);
    let _ = app.emit("setup-done", ());
}

/// "Got it" on the notice about the automatic setup.
#[tauri::command]
pub fn ack_setup_notice(webview: Webview, store: State<'_, Store>) -> Result<Settings, String> {
    guard(&webview)?;
    apply(webview.app_handle(), &store, |s| s.auto_tracked_notice = false)
}

/// "Turn off" on that notice: stop tracking the name that was added automatically.
#[tauri::command]
pub fn undo_auto_track(webview: Webview, store: State<'_, Store>, uuid: String) -> Result<Settings, String> {
    guard(&webview)?;
    apply(webview.app_handle(), &store, |s| {
        s.players.retain(|p| p.uuid != uuid);
        s.auto_tracked_notice = false;
    })
}

/// "Change name": put another player first in place of `replace`.
#[tauri::command]
pub fn replace_first_player(webview: Webview, store: State<'_, Store>, replace: String, player: TrackedPlayer) -> Result<Settings, String> {
    guard(&webview)?;
    apply(webview.app_handle(), &store, |s| {
        s.players.retain(|p| p.uuid != replace && p.uuid != player.uuid);
        s.players.insert(0, player);
        s.auto_tracked_notice = false;
    })
}

/// "Not now" on the first-run welcome: remember it and show the website.
#[tauri::command]
pub fn dismiss_welcome(webview: Webview, store: State<'_, Store>) -> Result<Settings, String> {
    guard(&webview)?;
    let settings = apply(webview.app_handle(), &store, |s| s.welcome_dismissed = true)?;
    shell::set_panel(webview.app_handle(), false);
    Ok(settings)
}

/// One of the first player's newest matches, and how far the app got with it.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OwnMatch {
    id: i64,
    /// Unix seconds when it ended.
    date: i64,
    opponent: Option<String>,
    /// "won", "lost" or "draw" for the first player.
    outcome: &'static str,
    forfeited: bool,
    /// "analyzed" (the analysis is ready), "analyzing" (Magma has the replay, the analysis is not
    /// ready yet), "uploading", "waiting" (not downloaded yet), "no_replay".
    state: &'static str,
}

/// The first player's newest matches with their sync state, for the app view.
#[tauri::command]
pub async fn recent_own(webview: Webview, client: State<'_, Client>, store: State<'_, Store>) -> Result<Vec<OwnMatch>, String> {
    guard(&webview)?;
    let Some(me) = store.get().players.first().cloned() else { return Ok(Vec::new()) };
    let matches = client.newest_matches(&me.uuid, 8).await?;
    let dir = crate::settings::app_dir(webview.app_handle()).map_err(|e| e.to_string())?;
    let (processed, pending) = (crate::sync::processed_ids(&dir), crate::sync::pending_ids(&dir));
    let uploader = crate::upload::Uploader::new();
    let ids: Vec<i64> = matches.iter().map(|m| m.id).collect();
    let analyzed = uploader.analyzed(&ids).await.unwrap_or_default();
    let mut out = Vec::new();
    for m in matches {
        let state = if analyzed.contains(&m.id) {
            "analyzed"
        } else if m.replay_exists == 0 {
            "no_replay"
        } else if pending.contains(&m.id) {
            "uploading"
        } else if matches!(uploader.has(m.id).await, crate::upload::Lookup::Stored) {
            "analyzing"
        } else if processed.contains(&m.id) {
            "uploading"
        } else {
            "waiting"
        };
        let outcome = match m.result.uuid.as_deref() {
            None => "draw",
            Some(w) if w == me.uuid => "won",
            Some(_) => "lost",
        };
        out.push(OwnMatch {
            id: m.id,
            date: m.date,
            opponent: m.players.iter().find(|p| p.uuid != me.uuid).map(|p| p.nickname.clone()),
            outcome,
            forfeited: m.forfeited,
            state,
        });
    }
    Ok(out)
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
    if enabled { crate::autostart::enable() } else { crate::autostart::disable() }?;
    apply(webview.app_handle(), &store, |s| s.start_with_windows = enabled)
}

#[tauri::command]
pub fn set_run_in_tray(webview: Webview, store: State<'_, Store>, enabled: bool) -> Result<Settings, String> {
    guard(&webview)?;
    apply(webview.app_handle(), &store, |s| s.run_in_tray = enabled)
}

#[tauri::command]
pub fn set_notify_analyzed(webview: Webview, store: State<'_, Store>, enabled: bool) -> Result<Settings, String> {
    guard(&webview)?;
    apply(webview.app_handle(), &store, |s| s.notify_analyzed = enabled)
}

#[tauri::command]
pub fn set_paused(webview: Webview, store: State<'_, Store>, paused: bool) -> Result<Settings, String> {
    guard(&webview)?;
    apply(webview.app_handle(), &store, |s| s.paused = paused)
}

/// "Check now": look for the first player's newest matches right away.
#[tauri::command]
pub fn check_now(webview: Webview) -> Result<(), String> {
    guard(&webview)?;
    webview.app_handle().state::<crate::sync::Engine>().check_own_now();
    Ok(())
}

/// Network connections the user can sync through (see network.rs).
#[tauri::command]
pub fn network_options(webview: Webview) -> Result<Vec<crate::network::Connection>, String> {
    guard(&webview)?;
    Ok(crate::network::connections())
}

/// Sync through this connection, by name; None lets the system decide.
#[tauri::command]
pub fn set_network(webview: Webview, store: State<'_, Store>, name: Option<String>) -> Result<Settings, String> {
    guard(&webview)?;
    apply(webview.app_handle(), &store, |s| s.network = name)
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

/// "What's new" in the settings footer: show this version's changes over the website.
/// Async: creating a webview from a synchronous command deadlocks on Windows.
#[tauri::command]
pub async fn show_whats_new(webview: Webview) -> Result<(), String> {
    guard(&webview)?;
    crate::shell::set_panel(webview.app_handle(), false);
    crate::shell::show_notice(webview.app_handle(), crate::shell::Notice::WhatsNew);
    Ok(())
}
