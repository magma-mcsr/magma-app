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
//!   with the queued files kept, the title bar says so, and the update is installed.
//! - After an update, the title bar shows "Updated to x.y.z" once, linking to the notes.

use std::sync::Mutex;
use std::time::Duration;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_updater::{Update, UpdaterExt};
use tokio::sync::Notify;

use crate::shell::TITLEBAR_LABEL;
use crate::sync::Engine;

const FIRST_CHECK_DELAY: Duration = Duration::from_secs(5);
const CHECK_INTERVAL: Duration = Duration::from_secs(6 * 3600);
/// How often to look again while the server refuses this version.
const REQUIRED_RETRY: Duration = Duration::from_secs(15 * 60);

/// How long the title bar shows "Updating" before a required update closes the app.
const REQUIRED_NOTICE: Duration = Duration::from_secs(3);

pub struct Updates {
    ready: Mutex<Option<(Update, Vec<u8>)>>,
    /// Set once after this version was installed as an update; taken by the title bar.
    updated: Mutex<Option<String>>,
    wake: Notify,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct UpdateStatus {
    /// Version downloaded and ready to install, if any.
    pub ready: Option<String>,
    /// This version was just installed as an update (shown once).
    pub updated: Option<String>,
}

impl Updates {
    pub fn new() -> Self {
        Updates { ready: Mutex::new(None), updated: Mutex::new(None), wake: Notify::new() }
    }

    pub fn status(&self) -> UpdateStatus {
        UpdateStatus {
            ready: self.ready.lock().expect("update lock").as_ref().map(|(u, _)| u.version.clone()),
            updated: None,
        }
    }

    /// Status for the title bar when it loads; the "Updated to" notice is handed out once.
    pub fn status_for_titlebar(&self) -> UpdateStatus {
        UpdateStatus { updated: self.updated.lock().expect("update lock").take(), ..self.status() }
    }

    /// Remember the running version; if it changed since the last run, this run follows
    /// an update. A first install says nothing.
    pub fn note_version(&self, store: &crate::settings::Store) {
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
            eprintln!("[update] server requires a newer version");
            engine.set(app, |l| l.update_required = true);
        }
        self.wake.notify_one();
    }
}

/// Install the downloaded update. The installer replaces the app and starts it again.
pub fn install_ready(app: &AppHandle) -> Result<(), String> {
    let ready = app.state::<Updates>().ready.lock().expect("update lock").take();
    let Some((update, bytes)) = ready else { return Err("no update ready".into()) };
    install(&update, &bytes)
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
    eprintln!("[update] installing {}", update.version);
    update.install(bytes).map_err(|e| e.to_string())
}

async fn check_and_download(app: &AppHandle) -> Result<Option<(Update, Vec<u8>)>, String> {
    let Some(update) = app.updater().map_err(|e| e.to_string())?.check().await.map_err(|e| e.to_string())? else {
        return Ok(None);
    };
    eprintln!("[update] downloading {}", update.version);
    let bytes = update.download(|_, _| {}, || {}).await.map_err(|e| e.to_string())?;
    Ok(Some((update, bytes)))
}

/// Tell the title bar the app is about to close for a required update, and give the
/// user a moment to read it.
async fn announce_required_install(app: &AppHandle, version: &str) {
    let _ = app.emit_to(TITLEBAR_LABEL, "update-installing", version.to_owned());
    tokio::time::sleep(REQUIRED_NOTICE).await;
}

pub async fn run(app: AppHandle) {
    // Development builds are never replaced by a release.
    if cfg!(debug_assertions) {
        return;
    }
    tokio::time::sleep(FIRST_CHECK_DELAY).await;
    // Started at login, nobody has opened the window yet: updating now interrupts no one.
    let mut quiet = std::env::args().any(|a| a == "--background");
    loop {
        let updates = app.state::<Updates>();
        let required = || app.state::<Engine>().live().update_required;
        // Once the user opens the window, the app no longer closes on its own.
        quiet = quiet && app.get_window("main").is_none();

        // Already downloaded and now required: no need to download again.
        if required() {
            let ready = updates.ready.lock().expect("update lock").take();
            if let Some((update, bytes)) = ready {
                announce_required_install(&app, &update.version).await;
                if let Err(e) = install(&update, &bytes) {
                    eprintln!("[update] install failed: {e}");
                }
            }
        }

        let has_ready = updates.ready.lock().expect("update lock").is_some();
        if !has_ready {
            match check_and_download(&app).await {
                Ok(Some((update, bytes))) if quiet || required() => {
                    if required() {
                        announce_required_install(&app, &update.version).await;
                    }
                    if let Err(e) = install(&update, &bytes) {
                        eprintln!("[update] install failed: {e}");
                    }
                }
                Ok(Some(ready)) => {
                    *updates.ready.lock().expect("update lock") = Some(ready);
                    let _ = app.emit_to(TITLEBAR_LABEL, "update-status", updates.status());
                }
                Ok(None) => {}
                Err(e) => eprintln!("[update] check failed: {e}"),
            }
        }
        quiet = false;

        let wait = if required() { REQUIRED_RETRY } else { CHECK_INTERVAL };
        let _ = tokio::time::timeout(wait, updates.wake.notified()).await;
    }
}
