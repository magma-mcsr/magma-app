//! Background replay sync.
//!
//! Each round: ask the API for every listed player's recent matches, queue the replays
//! that are new and not yet expired (higher players first, then the ones closest to
//! expiring), download them one at a time within the CDN rate limit, optionally keep a
//! copy, and upload them to Magma.
//!
//! The server never tells the app what to download. The only thing the app asks it is
//! "do you already have this match?", which can only skip downloads.

use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};
use tokio::sync::Notify;

use crate::mcsr::{Client, MatchSummary};
use crate::settings::{Store, TrackedPlayer};
use crate::upload::{Outcome, Uploader};

/// Official replays are deleted after 7 days; leave an hour of margin.
const RETENTION_SECS: i64 = 7 * 24 * 3600 - 3600;
/// Roughly how long one download takes, including the rate-limit spacing.
const SECS_PER_DOWNLOAD: i64 = 26;
const ROUND_INTERVAL: Duration = Duration::from_secs(10 * 60);
/// Forget processed matches after this long (they are expired by then anyway).
const JOURNAL_KEEP_SECS: i64 = 14 * 24 * 3600;
/// The server accepts 10 uploads per minute per IP. A backlog (after an outage) is sent
/// no faster than this so it never hits that limit.
const UPLOAD_SPACING: Duration = Duration::from_secs(7);

#[derive(Clone, Copy, PartialEq, Default)]
pub enum Phase {
    #[default]
    Idle,
    Checking,
    Downloading,
}

/// What the engine is doing right now (drives the title bar pill).
#[derive(Clone, Default)]
pub struct Live {
    pub phase: Phase,
    pub done: usize,
    pub total: usize,
    pub expiring: usize,
    pub pending_uploads: usize,
}

pub struct Engine {
    wake: Notify,
    live: std::sync::Mutex<Live>,
}

impl Engine {
    pub fn new() -> Self {
        Engine { wake: Notify::new(), live: std::sync::Mutex::new(Live::default()) }
    }

    /// Start a round now (for example after a player was added or sync was resumed).
    pub fn wake(&self) {
        self.wake.notify_one();
    }

    pub fn live(&self) -> Live {
        self.live.lock().expect("live lock").clone()
    }

    fn set(&self, app: &AppHandle, f: impl FnOnce(&mut Live)) {
        f(&mut self.live.lock().expect("live lock"));
        crate::shell::broadcast_status(app, &app.state::<Store>().get());
    }
}

// ---------- journal: which matches are already handled ----------

#[derive(Default, Serialize, Deserialize)]
struct Journal {
    /// match id -> unix seconds when it was handled
    processed: BTreeMap<i64, i64>,
}

impl Journal {
    fn path(dir: &Path) -> PathBuf {
        dir.join("sync-journal.json")
    }

    fn load(dir: &Path) -> Self {
        let mut j: Journal = std::fs::read(Self::path(dir)).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
        let cutoff = now() - JOURNAL_KEEP_SECS;
        j.processed.retain(|_, at| *at > cutoff);
        j
    }

    fn mark(&mut self, dir: &Path, match_id: i64) {
        self.processed.insert(match_id, now());
        let tmp = Self::path(dir).with_extension("json.tmp");
        if std::fs::write(&tmp, serde_json::to_vec(self).expect("journal serializes")).is_ok() {
            let _ = std::fs::rename(tmp, Self::path(dir));
        }
    }
}

fn now() -> i64 {
    chrono::Utc::now().timestamp()
}

// ---------- file names ----------

fn safe(name: &str) -> String {
    name.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '_').collect()
}

/// `2026-10-16_14201972_Feinberg_vs_subdas.rrf`: local date, match id, then every player,
/// listed players first so the file sorts and reads naturally.
fn file_name(m: &MatchSummary, listed: &[TrackedPlayer]) -> String {
    let date = chrono::DateTime::from_timestamp(m.date, 0)
        .map(|d| d.with_timezone(&chrono::Local).format("%Y-%m-%d").to_string())
        .unwrap_or_else(|| "unknown-date".into());
    let rank = |uuid: &str| listed.iter().position(|p| p.uuid == uuid).unwrap_or(usize::MAX);
    let mut players: Vec<_> = m.players.iter().collect();
    players.sort_by_key(|p| rank(&p.uuid));
    let mut names: Vec<String> = players.iter().map(|p| safe(&p.nickname)).filter(|n| !n.is_empty()).take(3).collect();
    if players.len() > 3 {
        names.push("and_more".into());
    }
    if names.is_empty() {
        format!("{date}_{}.rrf", m.id)
    } else {
        format!("{date}_{}_{}.rrf", m.id, names.join("_vs_"))
    }
}

// ---------- the loop ----------

pub async fn run(app: AppHandle) {
    let Ok(data_dir) = app.path().app_data_dir() else { return };
    let pending_dir = data_dir.join("pending-uploads");
    let _ = std::fs::create_dir_all(&pending_dir);
    let mut journal = Journal::load(&data_dir);
    let uploader = Uploader::new();
    let mut upload_blocked_until: Option<tokio::time::Instant> = None;

    loop {
        let engine = app.state::<Engine>();
        let settings = app.state::<Store>().get();
        engine.set(&app, |l| l.pending_uploads = count_pending(&pending_dir));

        if settings.paused || settings.players.is_empty() {
            engine.set(&app, |l| l.phase = Phase::Idle);
            let _ = tokio::time::timeout(Duration::from_secs(60), engine.wake.notified()).await;
            continue;
        }

        flush_uploads(&app, &uploader, &pending_dir, &mut upload_blocked_until).await;

        // 1. Find new matches, highest-priority players first.
        engine.set(&app, |l| l.phase = Phase::Checking);
        let client = app.state::<Client>();
        let mut seen = HashSet::new();
        let mut queue: Vec<(usize, MatchSummary)> = Vec::new();
        for (rank, player) in settings.players.iter().enumerate() {
            match client.recent_matches(&player.uuid).await {
                Ok(matches) => {
                    for m in matches {
                        let fresh = m.date + RETENTION_SECS > now();
                        if m.replay_exists > 0 && fresh && !journal.processed.contains_key(&m.id) && seen.insert(m.id) {
                            queue.push((rank, m));
                        }
                    }
                }
                Err(e) => eprintln!("[sync] matches for {}: {e}", player.nickname),
            }
        }
        // Higher players first; within a player, the replay closest to expiring first.
        queue.sort_by_key(|(rank, m)| (*rank, m.date));
        let total = queue.len();
        let expiring = queue
            .iter()
            .enumerate()
            .filter(|(k, (_, m))| m.date + RETENTION_SECS < now() + (*k as i64 + 1) * SECS_PER_DOWNLOAD)
            .count();
        engine.set(&app, |l| {
            l.total = total;
            l.done = 0;
            l.expiring = expiring;
        });

        // 2. Download one at a time.
        for (k, (_, m)) in queue.iter().enumerate() {
            let settings = app.state::<Store>().get();
            if settings.paused || settings.players.is_empty() {
                break;
            }
            engine.set(&app, |l| {
                l.phase = Phase::Downloading;
                l.done = k;
            });
            // Magma already has it and no local copy is wanted: nothing to do.
            if !settings.keep_copy && uploader.has(m.id).await {
                journal.mark(&data_dir, m.id);
                continue;
            }
            match client.download_replay(m.id, m.replay_exists).await {
                Ok(bytes) => {
                    if settings.keep_copy {
                        if let Some(folder) = &settings.copy_folder {
                            let path = Path::new(folder).join(file_name(m, &settings.players));
                            if std::fs::create_dir_all(folder).is_ok() && !path.exists() {
                                if let Err(e) = std::fs::write(&path, &bytes) {
                                    eprintln!("[sync] could not save {}: {e}", path.display());
                                }
                            }
                        }
                    }
                    // Held until the upload succeeds, so a closed app or a server outage loses nothing.
                    let _ = std::fs::write(pending_dir.join(format!("{}.rrf", m.id)), &bytes);
                    journal.mark(&data_dir, m.id);
                    flush_uploads(&app, &uploader, &pending_dir, &mut upload_blocked_until).await;
                }
                // Gone from the CDN: nothing more can be done for this one.
                Err(e) if e.contains("no longer available") || e.contains("no replay") => journal.mark(&data_dir, m.id),
                Err(e) => eprintln!("[sync] download {}: {e}", m.id),
            }
        }

        engine.set(&app, |l| {
            l.phase = Phase::Idle;
            l.expiring = 0;
            l.pending_uploads = count_pending(&pending_dir);
        });
        let _ = tokio::time::timeout(ROUND_INTERVAL, engine.wake.notified()).await;
    }
}

fn count_pending(dir: &Path) -> usize {
    std::fs::read_dir(dir).map(|it| it.flatten().filter(|e| e.path().extension().is_some_and(|x| x == "rrf")).count()).unwrap_or(0)
}

/// Upload everything waiting in `pending-uploads/`, oldest first.
async fn flush_uploads(app: &AppHandle, uploader: &Uploader, dir: &Path, blocked_until: &mut Option<tokio::time::Instant>) {
    if blocked_until.is_some_and(|t| tokio::time::Instant::now() < t) {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    let mut files: Vec<PathBuf> = entries.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "rrf")).collect();
    files.sort();
    for (i, path) in files.into_iter().enumerate() {
        if i > 0 {
            tokio::time::sleep(UPLOAD_SPACING).await;
        }
        let Some(match_id) = path.file_stem().and_then(|s| s.to_str()).and_then(|s| s.parse::<i64>().ok()) else { continue };
        let Ok(bytes) = std::fs::read(&path) else { continue };
        match uploader.upload(match_id, bytes).await {
            Outcome::Accepted => {
                let _ = std::fs::remove_file(&path);
            }
            Outcome::Rejected(why) => {
                eprintln!("[sync] upload {match_id} rejected: {why}");
                let _ = std::fs::remove_file(&path);
            }
            Outcome::Retry { after, reason } => {
                eprintln!("[sync] upload paused for {}s: {reason}", after.as_secs());
                *blocked_until = Some(tokio::time::Instant::now() + after);
                break;
            }
        }
    }
    app.state::<Engine>().set(app, |l| l.pending_uploads = count_pending(dir));
}
