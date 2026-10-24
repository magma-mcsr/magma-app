//! Background replay sync.
//!
//! Each round: ask the API for the listed players' recent matches, queue the replays that
//! are new and not yet expired (higher players first, then the ones closest to expiring),
//! download them one at a time within the CDN rate limit, optionally keep a copy, and
//! upload them to Magma.
//!
//! The list can be longer than one connection can keep up with. Downloads then follow the
//! list order: a round downloads for at most ROUND_DOWNLOAD_BUDGET, then checks for new
//! matches and sorts again, so the top of the list is never stuck behind a backlog, and
//! players further down get whatever capacity is left.
//!
//! The first player in the list (usually the user) is also checked on their own every
//! OWN_CHECK_INTERVAL, in between everything else, so a match they just played is uploaded
//! within seconds instead of waiting for the next round. The settings panel can ask for that
//! check right away.
//!
//! The server never tells the app what to download. The only thing the app asks it is
//! "do you already have this match?", which can only skip downloads.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};
use tokio::sync::Notify;

use crate::mcsr::{Client, MatchSummary};
use crate::settings::{Store, TrackedPlayer};
use crate::update::Updates;
use crate::upload::{Lookup, Outcome, Uploader};

/// Official replays are deleted after 7 days; leave an hour of margin.
const RETENTION_SECS: i64 = 7 * 24 * 3600 - 3600;
/// Roughly how long one download takes, including the rate-limit spacing.
const SECS_PER_DOWNLOAD: i64 = 21;
/// Wait between rounds once everything found has been downloaded.
const ROUND_INTERVAL: Duration = Duration::from_secs(10 * 60);
/// A round downloads for at most this long, then checks for new matches and sorts again.
const ROUND_DOWNLOAD_BUDGET: Duration = Duration::from_secs(30 * 60);
/// Players at the top of the list whose matches are checked every round.
const ALWAYS_CHECK: usize = 200;
/// Further down the list, this many players are checked per round, taking turns, so a long
/// list does not make every round spend minutes on API requests.
const CHECK_IN_TURN: usize = 100;
/// Forget processed matches after this long (they are expired by then anyway).
const JOURNAL_KEEP_SECS: i64 = 14 * 24 * 3600;
/// The server accepts 10 uploads per minute per IP. A backlog (after an outage) is sent
/// no faster than this so it never hits that limit.
const UPLOAD_SPACING: Duration = Duration::from_secs(7);
/// How often the first player's newest matches are checked, in between everything else.
const OWN_CHECK_INTERVAL: Duration = Duration::from_secs(15);
/// How many of their newest matches that check asks for.
const OWN_CHECK_COUNT: u32 = 5;
/// Only matches that ended this recently are taken out of the normal order.
const OWN_RECENT_SECS: i64 = 3600;
/// While the server refuses this app version, ask again this often.
const UPDATE_RECHECK: Duration = Duration::from_secs(15 * 60);

#[derive(Clone, Copy, PartialEq, Default)]
pub enum Phase {
    #[default]
    Idle,
    Checking,
    Downloading,
}

/// What the engine is doing right now (drives the App tab's status in the title bar).
#[derive(Clone, Default)]
pub struct Live {
    pub phase: Phase,
    pub done: usize,
    pub total: usize,
    pub expiring: usize,
    pub pending_uploads: usize,
    /// The server no longer accepts this app version; nothing syncs until it updates.
    pub update_required: bool,
}

pub struct Engine {
    wake: Notify,
    live: std::sync::Mutex<Live>,
    /// Check the first player's newest matches at the next chance, without waiting.
    own_now: AtomicBool,
}

impl Engine {
    pub fn new() -> Self {
        Engine { wake: Notify::new(), live: std::sync::Mutex::new(Live::default()), own_now: AtomicBool::new(false) }
    }

    /// Check the first player's newest matches now (the "Check now" button).
    pub fn check_own_now(&self) {
        self.own_now.store(true, Ordering::SeqCst);
        self.wake.notify_one();
    }

    /// Start a round now (for example after a player was added or sync was resumed).
    pub fn wake(&self) {
        self.wake.notify_one();
    }

    pub fn live(&self) -> Live {
        self.live.lock().expect("live lock").clone()
    }

    pub fn set(&self, app: &AppHandle, f: impl FnOnce(&mut Live)) {
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

/// Matches already handled (downloaded and handed to the uploader, or skipped as stored).
pub fn processed_ids(dir: &Path) -> std::collections::HashSet<i64> {
    Journal::load(dir).processed.into_keys().collect()
}

/// Matches downloaded and waiting to be uploaded.
pub fn pending_ids(dir: &Path) -> std::collections::HashSet<i64> {
    std::fs::read_dir(dir.join("pending-uploads"))
        .map(|it| it.flatten().filter_map(|e| e.path().file_stem()?.to_str()?.parse::<i64>().ok()).collect())
        .unwrap_or_default()
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
    let Ok(data_dir) = crate::settings::app_dir(&app) else { return };
    let pending_dir = data_dir.join("pending-uploads");
    let _ = std::fs::create_dir_all(&pending_dir);
    let mut journal = Journal::load(&data_dir);
    let uploader = Uploader::new();
    let mut upload_blocked_until: Option<tokio::time::Instant> = None;
    // Matches found but not downloaded yet, kept across rounds (players further down the
    // list are not checked every round).
    let mut known: HashMap<i64, MatchSummary> = HashMap::new();
    // Where the next turn starts among the players after ALWAYS_CHECK.
    let mut turn: usize = 0;
    // When the first player's newest matches were last checked.
    let mut own_checked = tokio::time::Instant::now() - OWN_CHECK_INTERVAL;

    loop {
        let engine = app.state::<Engine>();
        let settings = app.state::<Store>().get();
        engine.set(&app, |l| l.pending_uploads = count_pending(&pending_dir));

        // Waiting for an update the server requires. Ask again now and then, in case the
        // server lowers its minimum version (for example after a mistake).
        if engine.live().update_required {
            engine.set(&app, |l| l.phase = Phase::Idle);
            let _ = tokio::time::timeout(UPDATE_RECHECK, engine.wake.notified()).await;
            if !matches!(uploader.has(0).await, Lookup::UpdateRequired) {
                engine.set(&app, |l| l.update_required = false);
            }
            continue;
        }

        // Paused or nothing to sync.
        if settings.paused || settings.players.is_empty() {
            engine.set(&app, |l| l.phase = Phase::Idle);
            let _ = tokio::time::timeout(Duration::from_secs(60), engine.wake.notified()).await;
            continue;
        }

        // Send through the chosen network connection, if any (looked up every round).
        let client = app.state::<Client>();
        let address = crate::network::local_address(settings.network.as_deref());
        client.bind(address);
        uploader.bind(address);

        flush_uploads(&app, &uploader, &pending_dir, &mut upload_blocked_until).await;

        // 1. Find new matches: the top of the list every round, the rest in turn.
        engine.set(&app, |l| l.phase = Phase::Checking);
        let players = &settings.players;
        let mut to_check: Vec<&TrackedPlayer> = players.iter().take(ALWAYS_CHECK).collect();
        let rest = &players[players.len().min(ALWAYS_CHECK)..];
        if !rest.is_empty() {
            let n = CHECK_IN_TURN.min(rest.len());
            let first = turn % rest.len();
            to_check.extend((0..n).map(|i| &rest[(first + i) % rest.len()]));
            turn = (first + n) % rest.len();
        }
        for player in to_check {
            check_own(&app, &client, &uploader, &mut journal, &data_dir, &pending_dir, &mut upload_blocked_until, &mut own_checked).await;
            match client.recent_matches(&player.uuid).await {
                Ok(matches) => {
                    for m in matches.into_iter().filter(|m| m.replay_exists > 0) {
                        known.entry(m.id).or_insert(m);
                    }
                }
                Err(e) => eprintln!("[sync] matches for {}: {e}", player.nickname),
            }
        }
        // A match belongs to its highest listed player; matches of removed players are dropped.
        let rank_of = |m: &MatchSummary| m.players.iter().filter_map(|p| players.iter().position(|q| q.uuid == p.uuid)).min();
        known.retain(|id, m| m.date + RETENTION_SECS > now() && !journal.processed.contains_key(id) && rank_of(m).is_some());
        // Higher players first; within a player, the replay closest to expiring first.
        let mut queue: Vec<(usize, MatchSummary)> = known.values().map(|m| (rank_of(m).unwrap_or(usize::MAX), m.clone())).collect();
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

        // 2. Download one at a time, for at most ROUND_DOWNLOAD_BUDGET.
        let started = tokio::time::Instant::now();
        let mut more_waiting = false;
        for (k, (_, m)) in queue.iter().enumerate() {
            check_own(&app, &client, &uploader, &mut journal, &data_dir, &pending_dir, &mut upload_blocked_until, &mut own_checked).await;
            let settings = app.state::<Store>().get();
            if settings.paused || settings.players.is_empty() || engine.live().update_required {
                break;
            }
            if started.elapsed() > ROUND_DOWNLOAD_BUDGET {
                more_waiting = true;
                break;
            }
            // Already handled by the check of the first player's newest matches.
            if journal.processed.contains_key(&m.id) {
                continue;
            }
            engine.set(&app, |l| {
                l.phase = Phase::Downloading;
                l.done = k;
            });
            if !fetch(&app, &client, &uploader, m, &mut journal, &data_dir, &pending_dir, &mut upload_blocked_until).await {
                break;
            }
        }
        known.retain(|id, _| !journal.processed.contains_key(id));

        engine.set(&app, |l| {
            l.phase = Phase::Idle;
            l.expiring = 0;
            l.pending_uploads = count_pending(&pending_dir);
        });
        // Still a backlog after the time budget: check again and continue right away.
        if more_waiting {
            continue;
        }
        // Wait for the next round, still checking the first player's newest matches. A wake
        // from anything but the "Check now" button starts the next round right away.
        let idle = tokio::time::Instant::now();
        while idle.elapsed() < ROUND_INTERVAL {
            let woke = tokio::time::timeout(OWN_CHECK_INTERVAL, engine.wake.notified()).await.is_ok();
            if woke && !engine.own_now.load(Ordering::SeqCst) {
                break;
            }
            check_own(&app, &client, &uploader, &mut journal, &data_dir, &pending_dir, &mut upload_blocked_until, &mut own_checked).await;
            engine.set(&app, |l| l.phase = Phase::Idle);
        }
    }
}

/// Download one replay, keep a copy if wanted, and upload it. Returns false when sync has to
/// stop because the server requires a newer app.
#[allow(clippy::too_many_arguments)]
async fn fetch(
    app: &AppHandle,
    client: &Client,
    uploader: &Uploader,
    m: &MatchSummary,
    journal: &mut Journal,
    data_dir: &Path,
    pending_dir: &Path,
    upload_blocked_until: &mut Option<tokio::time::Instant>,
) -> bool {
    let settings = app.state::<Store>().get();
    // Look the connection up again before each download: a phone or mobile connection
    // that reconnects comes back with a new address, and this follows it right away.
    let address = crate::network::local_address(settings.network.as_deref());
    client.bind(address);
    uploader.bind(address);
    // Magma already has it and no local copy is wanted: nothing to do.
    if !settings.keep_copy {
        match uploader.has(m.id).await {
            Lookup::Stored => {
                journal.mark(data_dir, m.id);
                return true;
            }
            Lookup::UpdateRequired => {
                app.state::<Updates>().require(app);
                return false;
            }
            Lookup::Missing => {}
        }
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
            journal.mark(data_dir, m.id);
            flush_uploads(app, uploader, pending_dir, upload_blocked_until).await;
        }
        // Gone from the CDN: nothing more can be done for this one.
        Err(e) if e.contains("no longer available") || e.contains("no replay") => journal.mark(data_dir, m.id),
        Err(e) => eprintln!("[sync] download {}: {e}", m.id),
    }
    true
}

/// Every OWN_CHECK_INTERVAL (or right away after "Check now"), ask for the first player's
/// newest matches and handle the ones that ended within the last hour, newest first.
#[allow(clippy::too_many_arguments)]
async fn check_own(
    app: &AppHandle,
    client: &Client,
    uploader: &Uploader,
    journal: &mut Journal,
    data_dir: &Path,
    pending_dir: &Path,
    upload_blocked_until: &mut Option<tokio::time::Instant>,
    checked: &mut tokio::time::Instant,
) {
    let engine = app.state::<Engine>();
    let asked = engine.own_now.swap(false, Ordering::SeqCst);
    if !asked && checked.elapsed() < OWN_CHECK_INTERVAL {
        return;
    }
    *checked = tokio::time::Instant::now();
    let settings = app.state::<Store>().get();
    if settings.paused || engine.live().update_required {
        return;
    }
    let Some(first) = settings.players.first() else { return };
    let matches = match client.newest_matches(&first.uuid, OWN_CHECK_COUNT).await {
        Ok(m) => m,
        Err(e) => {
            eprintln!("[sync] newest matches for {}: {e}", first.nickname);
            return;
        }
    };
    let mut fresh: Vec<MatchSummary> = matches
        .into_iter()
        .filter(|m| m.replay_exists > 0 && m.date + OWN_RECENT_SECS > now() && !journal.processed.contains_key(&m.id))
        .collect();
    fresh.sort_by_key(|m| std::cmp::Reverse(m.date));
    for m in &fresh {
        engine.set(app, |l| l.phase = Phase::Downloading);
        if !fetch(app, client, uploader, m, journal, data_dir, pending_dir, upload_blocked_until).await {
            break;
        }
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
            Outcome::UpdateRequired => {
                app.state::<Updates>().require(app);
                break;
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
