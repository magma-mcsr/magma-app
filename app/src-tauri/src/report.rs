//! "Share sync statistics" (on by default, in Settings): every 6 hours the app tells Magma it is
//! running, so Magma can see how many apps are syncing and which players are covered.
//!
//! The report is exactly this, and nothing else:
//! - `installId`: a random ID made on first run and kept in the settings file. It is not derived
//!   from anything on this computer and is not tied to any account.
//! - `appVersion`
//! - `syncOn`: whether replay sync is switched on
//! - `players`: the UUIDs of the players in the list
//!
//! Turned off, nothing is sent. A failed report is not retried until the next one.

use std::time::Duration;

use serde_json::json;
use tauri::{AppHandle, Manager};

use crate::settings::Store;

/// The first report a minute after start (sync settles first), then every 6 hours.
const FIRST_AFTER: Duration = Duration::from_secs(60);
const EVERY: Duration = Duration::from_secs(6 * 3600);

/// A new random install ID: 16 random bytes from the operating system, as 32 hex digits.
pub fn new_install_id() -> Option<String> {
    let mut bytes = [0u8; 16];
    getrandom::fill(&mut bytes).ok()?;
    Some(hex::encode(bytes))
}

pub async fn run(app: AppHandle) {
    tokio::time::sleep(FIRST_AFTER).await;
    loop {
        send(&app).await;
        tokio::time::sleep(EVERY).await;
    }
}

async fn send(app: &AppHandle) {
    let s = app.state::<Store>().get();
    if !s.share_stats {
        return;
    }
    let Some(install_id) = s.install_id.clone() else { return };
    let body = json!({
        "installId": install_id,
        "appVersion": env!("CARGO_PKG_VERSION"),
        "syncOn": !s.paused,
        "players": s.players.iter().map(|p| p.uuid.as_str()).collect::<Vec<_>>(),
    });
    // Through the connection chosen under Network, like the uploads
    let client = crate::network::BoundClient::new(Duration::from_secs(30));
    client.bind(crate::network::local_address(s.network.as_deref()));
    let url = format!("{}/api/mcsr-ranked/app-report", crate::upload::base_url());
    if let Err(e) = client.get().post(url).json(&body).send().await.and_then(|r| r.error_for_status()) {
        eprintln!("[report] {e}");
    }
}
