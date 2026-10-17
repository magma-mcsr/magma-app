//! Self-update from this repository's GitHub Releases.
//!
//! Every release carries a `latest.json` and an installer signed with the Magma update
//! key. The app only installs an update whose signature matches the public key built
//! into it (`plugins.updater.pubkey` in `tauri.conf.json`).
//!
//! - Shortly after launch: if a newer version is out, it is installed right away.
//! - While running: check every few hours and download in the background. The title bar
//!   then offers "Update"; if the user ignores it, the next launch installs it.
//! - When the server says this version is no longer supported (HTTP 426): uploads pause
//!   with the queued files kept, and the update is installed as soon as it is found.

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

pub struct Updates {
    ready: Mutex<Option<(Update, Vec<u8>)>>,
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
        Updates { ready: Mutex::new(None), wake: Notify::new() }
    }

    pub fn status(&self) -> UpdateStatus {
        UpdateStatus { ready: self.ready.lock().expect("update lock").as_ref().map(|(u, _)| u.version.clone()) }
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

pub async fn run(app: AppHandle) {
    // Development builds are never replaced by a release.
    if cfg!(debug_assertions) {
        return;
    }
    tokio::time::sleep(FIRST_CHECK_DELAY).await;
    let mut first = true;
    loop {
        let updates = app.state::<Updates>();
        let required = || app.state::<Engine>().live().update_required;

        // Already downloaded and now required: no need to download again.
        if required() {
            if let Err(e) = install_ready(&app) {
                if e != "no update ready" {
                    eprintln!("[update] install failed: {e}");
                }
            }
        }

        let has_ready = updates.ready.lock().expect("update lock").is_some();
        if !has_ready {
            match check_and_download(&app).await {
                Ok(Some((update, bytes))) if first || required() => {
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
        first = false;

        let wait = if required() { REQUIRED_RETRY } else { CHECK_INTERVAL };
        let _ = tokio::time::timeout(wait, updates.wake.notified()).await;
    }
}
