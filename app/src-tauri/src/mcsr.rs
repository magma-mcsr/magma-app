//! Access to the official MCSR Ranked API and replay CDN.
//!
//! All network traffic of the app goes through here, so the rules live in one place:
//! honest User-Agent, only the requested player's own matches, and the CDN rate limit.

use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;

const API: &str = "https://api.mcsrranked.com";
const CDN: &str = "https://replay.mcsrranked.com";

/// Minimum time between the starts of two replay downloads. The CDN blocks an IP for
/// 5 minutes after more than 3 downloads within a minute; 21 s allows at most 3 in any
/// 60 s, the most that never trips the limit.
const DOWNLOAD_SPACING: Duration = Duration::from_secs(21);
/// After being blocked anyway, something else shares this IP's limit (Ranked itself when
/// you watch replays in game, or another computer on the same network). Back off to this
/// spacing for a while.
const SHARED_SPACING: Duration = Duration::from_secs(30);
const SHARED_FOR: Duration = Duration::from_secs(60 * 60);
const MAX_REPLAY_BYTES: usize = 64 << 20;
/// Minimum spacing between any two API calls. The official limit is 500 requests per
/// 10 minutes per IP; 1.5 s caps the app at 400, however many players are followed.
const API_SPACING: Duration = Duration::from_millis(1500);
/// Wait after an API 429 when the response does not say how long.
const API_RETRY_DEFAULT: Duration = Duration::from_secs(60);
const MAX_RETRIES: u32 = 3;

pub struct Client {
    /// Identifies itself as `magma-app/<version>` (see network.rs).
    http: crate::network::BoundClient,
    last_download: Mutex<Option<Instant>>,
    /// Until when downloads use SHARED_SPACING.
    shared_until: std::sync::Mutex<Option<Instant>>,
    last_api_call: Mutex<Option<Instant>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Player {
    pub uuid: String,
    pub nickname: String,
    #[serde(default)]
    pub elo_rate: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MatchResult {
    pub uuid: Option<String>,
    pub time: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MatchSummary {
    pub id: i64,
    #[serde(rename = "type")]
    pub match_type: i64,
    /// Unix seconds.
    pub date: i64,
    #[serde(default)]
    pub replay_exists: i64,
    #[serde(default)]
    pub forfeited: bool,
    pub result: MatchResult,
    pub players: Vec<Player>,
}

#[derive(Deserialize)]
struct Envelope<T> {
    status: String,
    data: Option<T>,
}

/// Minecraft names are 1-16 characters of letters, digits and underscores.
pub fn is_valid_name(name: &str) -> bool {
    (1..=16).contains(&name.len()) && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
}

impl Client {
    pub fn new() -> Self {
        Client {
            http: crate::network::BoundClient::new(Duration::from_secs(60)),
            last_download: Mutex::new(None),
            shared_until: std::sync::Mutex::new(None),
            last_api_call: Mutex::new(None),
        }
    }

    /// Send from this local address (None: the system decides). See network.rs.
    pub fn bind(&self, address: Option<std::net::IpAddr>) {
        self.http.bind(address);
    }

    fn download_spacing(&self) -> Duration {
        match *self.shared_until.lock().expect("spacing lock") {
            Some(until) if Instant::now() < until => SHARED_SPACING,
            _ => DOWNLOAD_SPACING,
        }
    }

    /// Every API request goes through here, so the spacing holds for all of them.
    async fn api<T: for<'de> Deserialize<'de>>(&self, path: &str) -> Result<T, String> {
        let mut attempts = 0;
        let res = loop {
            {
                // Holding the lock serializes API calls app-wide.
                let mut last = self.last_api_call.lock().await;
                if let Some(wait) = last.and_then(|t| API_SPACING.checked_sub(t.elapsed())) {
                    tokio::time::sleep(wait).await;
                }
                *last = Some(Instant::now());
            }
            let res = self.http.get().get(format!("{API}{path}")).send().await.map_err(|e| format!("Network error: {e}"))?;
            if res.status() != reqwest::StatusCode::TOO_MANY_REQUESTS {
                break res;
            }
            // Should not happen at this spacing (another program on the same network could
            // share the limit). Wait as long as the API asks, then try again.
            attempts += 1;
            if attempts > MAX_RETRIES {
                return Err("The MCSR Ranked API is rate limiting requests. Try again in a few minutes.".into());
            }
            let wait = res
                .headers()
                .get(reqwest::header::RETRY_AFTER)
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.parse::<u64>().ok())
                .map_or(API_RETRY_DEFAULT, Duration::from_secs);
            tokio::time::sleep(wait).await;
        };
        let env: Envelope<T> = res.json().await.map_err(|e| format!("Unexpected API response: {e}"))?;
        match (env.status.as_str(), env.data) {
            ("success", Some(d)) => Ok(d),
            _ => Err("Player not found.".into()),
        }
    }

    /// Look up a player by name (used to confirm names typed into settings).
    pub async fn player(&self, name: &str) -> Result<Player, String> {
        if !is_valid_name(name) {
            return Err("That is not a valid Minecraft name.".into());
        }
        self.api(&format!("/users/{name}")).await
    }

    /// A player's recent matches by UUID. A unique query parameter bypasses the API's
    /// 4-hour CDN cache so newly finished matches show up.
    pub async fn recent_matches(&self, uuid: &str) -> Result<Vec<MatchSummary>, String> {
        if uuid.len() != 32 || !uuid.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err("invalid UUID".into());
        }
        let bust = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
        self.api(&format!("/users/{uuid}/matches?count=50&_={bust}")).await
    }

    /// Download one replay into memory, respecting the CDN rate limit.
    pub async fn download_replay(
        &self,
        match_id: i64,
        replay_exists: i64,
    ) -> Result<Vec<u8>, String> {
        let bucket = match replay_exists {
            1 => "matches",
            2 => "archives",
            _ => return Err("This match has no replay.".into()),
        };
        let url = format!("{CDN}/{bucket}/{match_id}.rrf");

        // Holding the lock serializes downloads, so the spacing applies app-wide.
        let mut last = self.last_download.lock().await;
        let mut attempts = 0;
        loop {
            if let Some(wait) = last.and_then(|t| self.download_spacing().checked_sub(t.elapsed())) {
                tokio::time::sleep(wait).await;
            }
            // Spacing counts from the start of each request, like the CDN's limit.
            *last = Some(Instant::now());
            let res = self.http.get().get(&url).send().await;
            let res = res.map_err(|e| format!("Network error: {e}"))?;

            if res.status() == reqwest::StatusCode::TOO_MANY_REQUESTS {
                *self.shared_until.lock().expect("spacing lock") = Some(Instant::now() + SHARED_FOR);
                attempts += 1;
                if attempts > MAX_RETRIES {
                    return Err("The replay server is rate limiting downloads. Try again later.".into());
                }
                let retry = res
                    .headers()
                    .get(reqwest::header::RETRY_AFTER)
                    .and_then(|v| v.to_str().ok())
                    .and_then(|v| v.parse::<u64>().ok())
                    .unwrap_or(300);
                tokio::time::sleep(Duration::from_secs(retry + 5)).await;
                continue;
            }
            if res.status() == reqwest::StatusCode::NOT_FOUND {
                return Err("The replay is no longer available (replays are kept for 7 days).".into());
            }
            if !res.status().is_success() {
                return Err(format!("The replay server returned HTTP {}.", res.status().as_u16()));
            }
            if res.content_length().is_some_and(|n| n as usize > MAX_REPLAY_BYTES) {
                return Err("The replay file is unexpectedly large.".into());
            }
            let bytes = res.bytes().await.map_err(|e| format!("Download failed: {e}"))?;
            if bytes.len() > MAX_REPLAY_BYTES {
                return Err("The replay file is unexpectedly large.".into());
            }
            return Ok(bytes.to_vec());
        }
    }
}
