//! User settings, stored as JSON in the app's config directory and saved on every change.

use std::path::PathBuf;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TrackedPlayer {
    pub uuid: String,
    pub nickname: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    /// Players to sync, highest priority first. Empty until first-run setup is done.
    #[serde(alias = "followed")]
    pub players: Vec<TrackedPlayer>,
    /// Older versions stored the user's own account separately. It is read once and
    /// moved to the front of `players`.
    #[serde(skip_serializing)]
    me: Option<TrackedPlayer>,
    /// Keep a copy of every downloaded replay on disk.
    pub keep_copy: bool,
    pub copy_folder: Option<String>,
    pub start_with_windows: bool,
    /// Closing the window hides it to the tray instead of quitting.
    pub run_in_tray: bool,
    pub paused: bool,
    /// Network connection to sync through, by name (for example "Wi-Fi"). None: the system
    /// decides. See network.rs.
    pub network: Option<String>,
    /// App version that last ran, to say "Updated to ..." once after an update.
    pub last_run_version: Option<String>,
    /// "Not now" on the first-run welcome: the app no longer opens on it.
    pub welcome_dismissed: bool,
    /// The name found on this computer was tracked automatically at first run. Done once: a
    /// player removed or renamed later is never added back.
    pub auto_setup_done: bool,
    /// The Replays tab still shows what that automatic setup did, until "Got it".
    pub auto_tracked_notice: bool,
    /// Windows notification when a match the first player just finished is analyzed.
    pub notify_analyzed: bool,
    /// "Share sync statistics": the status report every 6 hours (see report.rs).
    pub share_stats: bool,
    /// Random ID for this install, made on first run, sent only in that report.
    pub install_id: Option<String>,
    /// This build said once that the Microsoft Store build is installed too (see lib.rs).
    pub store_build_notice_shown: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            players: vec![],
            me: None,
            keep_copy: false,
            copy_folder: None,
            // On: replays are only kept for 7 days, so an app nobody remembers to open misses matches.
            // The first-run notice and welcome say so, and point to the setting.
            start_with_windows: true,
            run_in_tray: true,
            paused: false,
            network: None,
            last_run_version: None,
            welcome_dismissed: false,
            auto_setup_done: false,
            auto_tracked_notice: false,
            notify_analyzed: true,
            share_stats: true,
            install_id: None,
            store_build_notice_shown: false,
        }
    }
}

pub struct Store {
    path: PathBuf,
    settings: Mutex<Settings>,
}

impl Store {
    pub fn load(dir: PathBuf) -> Self {
        let path = dir.join("settings.json");
        let mut settings: Settings = std::fs::read(&path)
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default();
        if let Some(me) = settings.me.take() {
            if !settings.players.iter().any(|p| p.uuid == me.uuid) {
                settings.players.insert(0, me);
            }
        }
        let store = Store { path, settings: Mutex::new(settings) };
        if store.get().install_id.is_none() {
            if let Some(id) = crate::report::new_install_id() {
                let _ = store.update(|s| s.install_id = Some(id));
            }
        }
        store
    }

    pub fn get(&self) -> Settings {
        self.settings.lock().expect("settings lock").clone()
    }

    /// Apply a change and write it to disk right away.
    pub fn update<R>(&self, f: impl FnOnce(&mut Settings) -> R) -> Result<(R, Settings), String> {
        let mut s = self.settings.lock().expect("settings lock");
        let result = f(&mut s);
        if let Some(dir) = self.path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        let tmp = self.path.with_extension("json.tmp");
        std::fs::write(&tmp, serde_json::to_vec_pretty(&*s).expect("settings serialize")).map_err(|e| e.to_string())?;
        std::fs::rename(&tmp, &self.path).map_err(|e| e.to_string())?;
        Ok((result, s.clone()))
    }
}

/// Where settings, the sync journal and pending uploads live. Development builds can use another
/// folder with MAGMA_APP_DATA, to try a first run without touching the installed app's data.
pub fn app_dir(app: &tauri::AppHandle) -> tauri::Result<PathBuf> {
    #[cfg(debug_assertions)]
    if let Some(dir) = std::env::var_os("MAGMA_APP_DATA") {
        let dir = PathBuf::from(dir);
        let _ = std::fs::create_dir_all(&dir);
        return Ok(dir);
    }
    use tauri::Manager;
    app.path().app_config_dir()
}
