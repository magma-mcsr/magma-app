//! Client for the Magma replay upload API (api.magmamcsr.com).
//!
//! Endpoints:
//! - `HEAD /api/mcsr-ranked/replays/{matchId}`: 200 when Magma already has the match
//! - `GET /api/mcsr-ranked/replays/analysis?matchIds=...`: which of those matches are analyzed
//!   (the Replays tab shows it next to your recent matches)
//! - `POST /api/mcsr-ranked/replays`: the original .rrf bytes, unchanged

use std::time::Duration;

use sha2::{Digest, Sha256};

/// Production by default. Set `MAGMA_UPLOAD_URL` (e.g. `http://127.0.0.1:3050`)
/// to test against a local server.
pub fn base_url() -> String {
    if let Ok(url) = std::env::var("MAGMA_UPLOAD_URL") {
        return url.trim_end_matches('/').to_owned();
    }
    "https://api.magmamcsr.com".into()
}

pub enum Outcome {
    /// Stored or queued by the server; the local copy can be deleted.
    Accepted,
    /// The server will never accept this file (bad request, too large). Drop it.
    Rejected(String),
    /// Try again later.
    Retry { after: Duration, reason: String },
    /// The server no longer accepts this app version (HTTP 426). Keep the file and update.
    UpdateRequired,
}

pub enum Lookup {
    Stored,
    Missing,
    UpdateRequired,
}

pub struct Uploader {
    http: crate::network::BoundClient,
    base: String,
}

impl Uploader {
    pub fn new() -> Self {
        Uploader { http: crate::network::BoundClient::new(Duration::from_secs(60)), base: base_url() }
    }

    /// Send from this local address (None: the system decides). See network.rs.
    pub fn bind(&self, address: Option<std::net::IpAddr>) {
        self.http.bind(address);
    }

    /// Whether Magma already has this match. Any error counts as "missing", so the
    /// download goes ahead as usual.
    /// Which of these matches Magma has analyzed (not just received). One request for up to
    /// 100 ids; None when the answer could not be read. The query is made unique so no cache
    /// on the way returns an older answer.
    pub async fn analyzed(&self, ids: &[i64]) -> Option<std::collections::HashSet<i64>> {
        #[derive(serde::Deserialize)]
        struct Row {
            #[serde(rename = "matchId")]
            match_id: i64,
        }
        #[derive(serde::Deserialize)]
        struct Reply {
            analyzed: Vec<Row>,
        }
        if ids.is_empty() {
            return Some(Default::default());
        }
        let list: Vec<String> = ids.iter().map(|id| id.to_string()).collect();
        let bust = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis());
        let url = format!("{}/api/mcsr-ranked/replays/analysis?matchIds={}&_={bust}", self.base, list.join(","));
        let reply: Reply = self.http.get().get(url).send().await.ok()?.error_for_status().ok()?.json().await.ok()?;
        Some(reply.analyzed.into_iter().map(|r| r.match_id).collect())
    }

    pub async fn has(&self, match_id: i64) -> Lookup {
        match self.http.get().head(format!("{}/api/mcsr-ranked/replays/{match_id}", self.base)).send().await {
            Ok(res) if res.status() == reqwest::StatusCode::OK => Lookup::Stored,
            Ok(res) if res.status() == reqwest::StatusCode::UPGRADE_REQUIRED => Lookup::UpdateRequired,
            _ => Lookup::Missing,
        }
    }

    pub async fn upload(&self, match_id: i64, bytes: Vec<u8>) -> Outcome {
        let sha = hex::encode(Sha256::digest(&bytes));
        let res = self
            .http
            .get()
            .post(format!("{}/api/mcsr-ranked/replays", self.base))
            .header("Content-Type", "application/octet-stream")
            .header("X-Match-Id", match_id.to_string())
            .header("X-Content-SHA256", sha)
            .body(bytes)
            .send()
            .await;
        let res = match res {
            Ok(r) => r,
            Err(e) => return Outcome::Retry { after: Duration::from_secs(300), reason: format!("network: {e}") },
        };
        let status = res.status();
        if status.is_success() {
            return Outcome::Accepted;
        }
        if status == reqwest::StatusCode::UPGRADE_REQUIRED {
            return Outcome::UpdateRequired;
        }
        if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
            let after = res
                .headers()
                .get(reqwest::header::RETRY_AFTER)
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.parse().ok())
                .unwrap_or(60);
            return Outcome::Retry { after: Duration::from_secs(after), reason: "upload rate limit".into() };
        }
        if status.is_client_error() {
            let body = res.text().await.unwrap_or_default();
            return Outcome::Rejected(format!("HTTP {}: {}", status.as_u16(), body.chars().take(200).collect::<String>()));
        }
        Outcome::Retry { after: Duration::from_secs(300), reason: format!("server error HTTP {}", status.as_u16()) }
    }
}
