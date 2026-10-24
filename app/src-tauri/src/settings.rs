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
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            players: vec![],
            me: None,
            keep_copy: false,
            copy_folder: None,
            start_with_windows: false,
            run_in_tray: true,
            paused: false,
            network: None,
            last_run_version: None,
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
        Store { path, settings: Mutex::new(settings) }
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
