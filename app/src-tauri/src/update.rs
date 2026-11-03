//! Self-update from this repository's GitHub Releases.
//!
//! Every release carries a `latest.json` and an installer signed with the Magma update
//! key. The app only installs an update whose signature matches the public key built
//! into it (`plugins.updater.pubkey` in `tauri.conf.json`).
//!
//! - Started at login (in the tray, nobody looking): a newer version is installed right away.
//! - Otherwise (opened by the user, or found while running): it is downloaded in the
//!   background and the title bar offers "Update to x.y.z". Nothing closes on its own;
//!   if the user ignores it, the next start at login installs it.
//! - When the server says this version is no longer supported (HTTP 426): uploads pause
//!   with the queued files kept. If the window is open, a notice covers the website and
//!   the user chooses to update or quit; with no window open, the update is installed.
//! - After an update, "What's new" (from CHANGELOG.md) is shown once over the website.
//! - The Microsoft Store build (see packaged.rs) gets its updates from the Store instead, the same
//!   way: it asks the Store for a newer version and has the Store install it. The Store alone
//!   would wait until the app is not running, which for an app in the tray may be never. When
//!   the app quits from the tray with an update found, the update is installed then. If the
//!   Store cannot install it silently, a required update opens the app's Store page.

use std::sync::Mutex;
use std::time::Duration;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_opener::OpenerExt;
use tauri_plugin_updater::{Update, UpdaterExt};
use tokio::sync::Notify;

use crate::shell::TITLEBAR_LABEL;
use crate::sync::Engine;

const FIRST_CHECK_DELAY: Duration = Duration::from_secs(5);
const CHECK_INTERVAL: Duration = Duration::from_secs(6 * 3600);
/// How often to look again while the server refuses this version.
const REQUIRED_RETRY: Duration = Duration::from_secs(15 * 60);

pub struct Updates {
    ready: Mutex<Option<(Update, Vec<u8>)>>,
    /// Store build: version of an update the Store has, not installed yet.
    store_ready: Mutex<Option<String>>,
    /// Set once after this version was installed as an update; taken by the title bar.
    updated: Mutex<Option<String>>,
    /// The user pressed "Update now" on the required-update notice.
    install_requested: std::sync::atomic::AtomicBool,
    wake: Notify,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct UpdateStatus {
    /// Version downloaded and ready to install, if any.
    pub ready: Option<String>,
}

impl Updates {
    pub fn new() -> Self {
        Updates {
            ready: Mutex::new(None),
            store_ready: Mutex::new(None),
            updated: Mutex::new(None),
            install_requested: Default::default(),
            wake: Notify::new(),
        }
    }

    pub fn status(&self) -> UpdateStatus {
        let ready = self.ready.lock().expect("update lock").as_ref().map(|(u, _)| u.version.clone());
        UpdateStatus { ready: ready.or_else(|| self.store_ready.lock().expect("update lock").clone()) }
    }

    /// Whether this run follows an update and "What's new" has not been shown yet.
    /// Handed out once.
    pub fn take_updated(&self) -> bool {
        self.updated.lock().expect("update lock").take().is_some()
    }

    /// Remember the running version; if it changed since the last run, this run follows
    /// an update. A first install says nothing.
    pub fn note_version(&self, store: &crate::settings::Store) {
        // A development build shares its settings with the installed app; recording its
        // version would hide "What's new" from the installed app's next update.
        if cfg!(debug_assertions) {
            return;
        }
        let current = env!("CARGO_PKG_VERSION");
        let settings = store.get();
        if settings.last_run_version.as_deref() == Some(current) {
            return;
        }
        // Versions before 0.1.2 did not record this; an existing player list means the
        // app was in use before, so this run follows an update.
        let updated = match &settings.last_run_version {
            Some(_) => true,
            None => !settings.players.is_empty(),
        };
        if updated {
            *self.updated.lock().expect("update lock") = Some(current.to_owned());
        }
        let _ = store.update(|s| s.last_run_version = Some(current.to_owned()));
    }

    /// The server no longer accepts this version: pause uploads and update now.
    pub fn require(&self, app: &AppHandle) {
        let engine = app.state::<Engine>();
        if !engine.live().update_required {
            log!("[update] server requires a newer version");
            engine.set(app, |l| l.update_required = true);
            crate::shell::show_notice(app, crate::shell::Notice::Required);
        }
        self.wake.notify_one();
    }
}

/// Install the downloaded update. The installer replaces the app and starts it again.
pub fn install_ready(app: &AppHandle) -> Result<(), String> {
    if crate::packaged::is_packaged() {
        let found = app.state::<Updates>().store_ready.lock().expect("update lock").take();
        if found.is_none() {
            return Err("no update ready".into());
        }
        install_from_store(app, Restart::Window);
        return Ok(());
    }
    let ready = app.state::<Updates>().ready.lock().expect("update lock").take();
    let Some((update, bytes)) = ready else { return Err("no update ready".into()) };
    install(&update, &bytes)
}

/// "Update now" on the required-update notice: install as soon as the update is here.
pub fn install_now(app: &AppHandle) -> Result<(), String> {
    if crate::packaged::is_packaged() {
        install_from_store(app, Restart::Window);
        return Ok(());
    }
    let updates = app.state::<Updates>();
    updates.install_requested.store(true, std::sync::atomic::Ordering::Relaxed);
    let ready = updates.ready.lock().expect("update lock").take();
    match ready {
        Some((update, bytes)) => install(&update, &bytes),
        None => {
            // Still downloading (or not found yet): the update loop installs it next.
            updates.wake.notify_one();
            Ok(())
        }
    }
}

/// The installer from earlier updates is left in the temp folder; remove it.
pub fn clean_old_installers() {
    let Ok(entries) = std::fs::read_dir(std::env::temp_dir()) else { return };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with("Magma MCSR-") && name.contains("-updater-") && entry.path().is_dir() {
            let _ = std::fs::remove_dir_all(entry.path());
        }
    }
}

fn install(update: &Update, bytes: &[u8]) -> Result<(), String> {
    log!("[update] installing {}", update.version);
    update.install(bytes).map_err(|e| e.to_string())
}

async fn check_and_download(app: &AppHandle) -> Result<Option<(Update, Vec<u8>)>, String> {
    let Some(update) = app.updater().map_err(|e| e.to_string())?.check().await.map_err(|e| e.to_string())? else {
        return Ok(None);
    };
    log!("[update] downloading {}", update.version);
    let bytes = update.download(|_, _| {}, || {}).await.map_err(|e| e.to_string())?;
    Ok(Some((update, bytes)))
}

pub async fn run(app: AppHandle) {
    // Development builds are never replaced by a release.
    if cfg!(debug_assertions) {
        return;
    }
    if crate::packaged::is_packaged() {
        return run_store(app).await;
    }
    tokio::time::sleep(FIRST_CHECK_DELAY).await;
    // Started at login, nobody has opened the window yet: updating now interrupts no one.
    let mut quiet = crate::packaged::started_at_login();
    loop {
        let updates = app.state::<Updates>();
        let required = || app.state::<Engine>().live().update_required;
        // Once the user opens the window, the app no longer closes on its own.
        quiet = quiet && app.get_window("main").is_none();

        // A required update may install without asking only if nobody is looking, or once
        // the user pressed "Update now"; otherwise the notice waits for the user.
        let may_install_required = || {
            app.get_window("main").is_none() || updates.install_requested.load(std::sync::atomic::Ordering::Relaxed)
        };

        // Already downloaded and now required: no need to download again.
        if required() && may_install_required() {
            let ready = updates.ready.lock().expect("update lock").take();
            if let Some((update, bytes)) = ready {
                if let Err(e) = install(&update, &bytes) {
                    log!("[update] install failed: {e}");
                }
            }
        }

        let has_ready = updates.ready.lock().expect("update lock").is_some();
        if !has_ready {
            match check_and_download(&app).await {
                Ok(Some((update, bytes))) if quiet || (required() && may_install_required()) => {
                    if let Err(e) = install(&update, &bytes) {
                        log!("[update] install failed: {e}");
                    }
                }
                Ok(Some(ready)) => {
                    *updates.ready.lock().expect("update lock") = Some(ready);
                    let _ = app.emit_to(TITLEBAR_LABEL, "update-status", updates.status());
                }
                Ok(None) => {}
                Err(e) => log!("[update] check failed: {e}"),
            }
        }
        quiet = false;

        let wait = if required() { REQUIRED_RETRY } else { CHECK_INTERVAL };
        let _ = tokio::time::timeout(wait, updates.wake.notified()).await;
    }
}

/// How the app comes back after a Store update closes it.
#[derive(Clone, Copy)]
enum Restart {
    /// In the tray, as at login.
    Tray,
    /// With its window open.
    Window,
    /// Not at all: the user quit.
    No,
}

/// Quit from the tray. With a Store update found, it is installed first; the Store closes the app
/// for that. Without one, or if that fails, the app just quits.
pub fn quit(app: &AppHandle) {
    let found = app.state::<Updates>().store_ready.lock().expect("update lock").take();
    if found.is_none() {
        app.exit(0);
        return;
    }
    for window in app.webview_windows().into_values() {
        let _ = window.hide();
    }
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        install_store_update(Restart::No).await;
        app.exit(0);
    });
}

/// Have the Store install its update. On success the Store closes the app, and Windows starts it
/// again as `restart` says. If it fails, a required update opens the Store page instead.
fn install_from_store(app: &AppHandle, restart: Restart) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        if !install_store_update(restart).await && app.state::<Engine>().live().update_required {
            if let Some(url) = crate::packaged::store_url() {
                let _ = app.opener().open_url(url, None::<&str>);
            }
        }
    });
}

async fn install_store_update(restart: Restart) -> bool {
    log!("[update] installing the Store update");
    let result = tauri::async_runtime::spawn_blocking(move || store::install(restart)).await;
    match result.map_err(|e| e.to_string()).and_then(|r| r) {
        Ok(true) => true,
        Ok(false) => {
            log!("[update] the Store did not install the update");
            false
        }
        Err(e) => {
            log!("[update] Store install failed: {e}");
            false
        }
    }
}

/// The Store build's update loop, like `run`: installed right away when started at login or when
/// required with nobody looking, otherwise offered in the title bar.
async fn run_store(app: AppHandle) {
    tokio::time::sleep(FIRST_CHECK_DELAY).await;
    let mut quiet = crate::packaged::started_at_login();
    loop {
        let updates = app.state::<Updates>();
        let required = app.state::<Engine>().live().update_required;
        quiet = quiet && app.get_window("main").is_none();
        let may_install_required =
            app.get_window("main").is_none() || updates.install_requested.load(std::sync::atomic::Ordering::Relaxed);

        let found = tauri::async_runtime::spawn_blocking(store::check).await;
        match found.map_err(|e| e.to_string()).and_then(|r| r) {
            Ok(Some(_)) if quiet || (required && may_install_required) => {
                install_store_update(if quiet { Restart::Tray } else { Restart::Window }).await;
            }
            Ok(Some(version)) => {
                let before = updates.store_ready.lock().expect("update lock").replace(version.clone());
                if before.as_deref() != Some(version.as_str()) {
                    let _ = app.emit_to(TITLEBAR_LABEL, "update-status", updates.status());
                }
            }
            Ok(None) => {}
            Err(e) => log!("[update] Store check failed: {e}"),
        }
        quiet = false;

        let wait = if required { REQUIRED_RETRY } else { CHECK_INTERVAL };
        let _ = tokio::time::timeout(wait, updates.wake.notified()).await;
    }
}

#[cfg(windows)]
mod store {
    use super::Restart;
    use windows::Services::Store::{StoreContext, StorePackageUpdateState};

    /// The version of a newer package the Store has for this app, if any. The Store numbers app
    /// version 0.x.y as 1.x.y.0 (see app/msix/build.ps1); this turns it back.
    pub fn check() -> Result<Option<String>, String> {
        let updates = StoreContext::GetDefault()
            .and_then(|c| c.GetAppAndOptionalStorePackageUpdatesAsync())
            .and_then(|op| op.join())
            .map_err(|e| e.to_string())?;
        let Ok(update) = updates.GetAt(0) else { return Ok(None) };
        let v = update.Package().and_then(|p| p.Id()).and_then(|id| id.Version()).map_err(|e| e.to_string())?;
        Ok(Some(format!("{}.{}.{}", v.Major.saturating_sub(1), v.Minor, v.Build)))
    }

    /// Download and install the update without asking. Windows closes the app to replace it and,
    /// unless `restart` is `No`, starts it again afterwards. Returns whether it was installed
    /// (when this returns at all: usually the app is closed first).
    pub fn install(restart: Restart) -> Result<bool, String> {
        let context = StoreContext::GetDefault().map_err(|e| e.to_string())?;
        let updates = context.GetAppAndOptionalStorePackageUpdatesAsync().and_then(|op| op.join()).map_err(|e| e.to_string())?;
        if updates.Size().unwrap_or(0) == 0 || !context.CanSilentlyDownloadStorePackageUpdates().unwrap_or(false) {
            return Ok(false);
        }
        register_restart(restart);
        let state = context
            .TrySilentDownloadAndInstallStorePackageUpdatesAsync(&updates)
            .and_then(|op| op.join())
            .and_then(|r| r.OverallState())
            .map_err(|e| e.to_string())?;
        Ok(state == StorePackageUpdateState::Completed)
    }

    /// Ask Windows to start the app again after the update closes it.
    fn register_restart(restart: Restart) {
        use windows_sys::Win32::System::Recovery::RegisterApplicationRestart;
        let args = match restart {
            Restart::Tray => "--background",
            Restart::Window => "",
            Restart::No => return,
        };
        let wide: Vec<u16> = args.encode_utf16().chain([0]).collect();
        unsafe { RegisterApplicationRestart(wide.as_ptr(), 0) };
    }
}

#[cfg(not(windows))]
mod store {
    use super::Restart;

    pub fn check() -> Result<Option<String>, String> {
        Ok(None)
    }

    pub fn install(_: Restart) -> Result<bool, String> {
        Ok(false)
    }
}
