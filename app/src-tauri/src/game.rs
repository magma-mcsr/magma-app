//! Finding MCSR Ranked on this computer: its instances in the usual launcher folders, and the
//! Minecraft names that played on them.
//!
//! Only two things are read: whether a folder exists, and the first part of each instance's
//! Minecraft logs (`logs/latest.log`, then the few newest `logs/*.log.gz`), where Minecraft writes
//! "Setting user: <name>" every time it starts. The older logs matter because a game left running
//! past midnight starts a new latest.log without that line.
//! Launcher account files (which hold login tokens) are never opened.

use std::io::Read;
use std::path::{Path, PathBuf};

use tauri::{AppHandle, Manager};

/// "Setting user" comes within the first lines of a launch; no need to read further (the log of
/// a long session can be gigabytes).
const LOG_HEAD_BYTES: u64 = 256 * 1024;
/// How many logs per instance to look at, newest first.
const LOGS_PER_INSTANCE: usize = 6;

/// %APPDATA% and the home folder. Development builds can point both at a test folder with
/// MAGMA_GAME_ROOT, to try a computer with or without the game.
pub fn roots(app: &AppHandle) -> (Option<PathBuf>, Option<PathBuf>) {
    #[cfg(debug_assertions)]
    if let Some(root) = std::env::var_os("MAGMA_GAME_ROOT") {
        let root = PathBuf::from(root);
        return (Some(root.clone()), Some(root));
    }
    (app.path().data_dir().ok(), app.path().home_dir().ok())
}

/// Game folders (the `minecraft` or `.minecraft` of an instance) that have MCSR Ranked installed,
/// most recently played first.
pub fn instances(roaming: Option<&Path>, home: Option<&Path>) -> Vec<PathBuf> {
    let mut candidates: Vec<PathBuf> = Vec::new();
    fn add_instances(out: &mut Vec<PathBuf>, root: PathBuf, game_dirs: &[&str]) {
        if let Ok(entries) = std::fs::read_dir(&root) {
            for entry in entries.flatten() {
                for game_dir in game_dirs {
                    out.push(entry.path().join(game_dir));
                }
            }
        }
    }
    if let Some(r) = roaming {
        add_instances(&mut candidates, r.join("PrismLauncher").join("instances"), &["minecraft", ".minecraft"]);
        add_instances(&mut candidates, r.join("ModrinthApp").join("profiles"), &[""]);
        add_instances(&mut candidates, r.join("com.modrinth.theseus").join("profiles"), &[""]);
        candidates.push(r.join(".minecraft"));
    }
    if let Some(h) = home {
        add_instances(&mut candidates, h.join("curseforge").join("minecraft").join("Instances"), &[""]);
    }
    let last_played = |dir: &PathBuf| {
        std::fs::metadata(dir.join("logs").join("latest.log"))
            .or_else(|_| std::fs::metadata(dir.join("mcsrranked")))
            .and_then(|m| m.modified())
            .ok()
    };
    let mut found: Vec<PathBuf> = candidates.into_iter().filter(|d| d.join("mcsrranked").is_dir()).collect();
    found.sort_by_key(|d| std::cmp::Reverse(last_played(d)));
    found
}

/// Minecraft names that last launched these instances, most recent first, without repeats.
pub fn account_names(instances: &[PathBuf]) -> Vec<String> {
    let mut names: Vec<String> = Vec::new();
    for dir in instances {
        if let Some(name) = newest_logs(&dir.join("logs")).iter().find_map(|log| logged_user(log)) {
            if !names.iter().any(|n| n.eq_ignore_ascii_case(&name)) {
                names.push(name);
            }
        }
    }
    names
}

/// latest.log and the gzipped older logs of one instance, newest first.
fn newest_logs(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut logs: Vec<(std::time::SystemTime, PathBuf)> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.file_name().and_then(|n| n.to_str()).is_some_and(|n| n == "latest.log" || n.ends_with(".log.gz")))
        .filter_map(|p| Some((std::fs::metadata(&p).and_then(|m| m.modified()).ok()?, p)))
        .collect();
    logs.sort_by_key(|(t, _)| std::cmp::Reverse(*t));
    logs.into_iter().take(LOGS_PER_INSTANCE).map(|(_, p)| p).collect()
}

/// The name in "Setting user: <name>" near the top of a Minecraft log (plain or gzipped).
fn logged_user(log: &Path) -> Option<String> {
    let file = std::fs::File::open(log).ok()?;
    let mut head = Vec::new();
    if log.extension().is_some_and(|x| x == "gz") {
        // A partial read is fine: only the beginning is needed
        let _ = flate2::read::GzDecoder::new(file).take(LOG_HEAD_BYTES).read_to_end(&mut head);
    } else {
        file.take(LOG_HEAD_BYTES).read_to_end(&mut head).ok()?;
    }
    let text = String::from_utf8_lossy(&head);
    let at = text.find("Setting user: ")? + "Setting user: ".len();
    let name: String = text[at..].chars().take_while(|c| c.is_ascii_alphanumeric() || *c == '_').collect();
    (3..=16).contains(&name.len()).then_some(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fake_root(name: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("magma-game-test-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        root
    }

    fn instance(root: &Path, name: &str, log: Option<&str>) -> PathBuf {
        let dir = root.join("PrismLauncher").join("instances").join(name).join("minecraft");
        std::fs::create_dir_all(dir.join("mcsrranked")).unwrap();
        if let Some(text) = log {
            std::fs::create_dir_all(dir.join("logs")).unwrap();
            std::fs::write(dir.join("logs").join("latest.log"), text).unwrap();
        }
        dir
    }

    #[test]
    fn reads_the_name_from_the_log() {
        let root = fake_root("name");
        instance(&root, "MCSR Ranked", Some("[01:53:07] [main/INFO]: Loading\n[01:53:07] [main/INFO]: Setting user: Loupitwai\n[x] more"));
        // An instance without MCSR Ranked is ignored
        let plain = root.join("PrismLauncher").join("instances").join("Vanilla").join("minecraft").join("logs");
        std::fs::create_dir_all(&plain).unwrap();
        std::fs::write(plain.join("latest.log"), "Setting user: Someone").unwrap();
        let found = instances(Some(&root), None);
        assert_eq!(found.len(), 1);
        assert_eq!(account_names(&found), vec!["Loupitwai".to_string()]);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn reads_an_older_gzipped_log_after_midnight() {
        use std::io::Write;
        let root = fake_root("gz");
        // The game kept running past midnight: the new latest.log has no "Setting user"
        let dir = instance(&root, "MCSR Ranked", Some("[00:00:01] [main/INFO]: something else"));
        let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        gz.write_all(b"[01:53:07] [main/INFO]: Setting user: Loupitwai\n").unwrap();
        std::fs::write(dir.join("logs").join("2026-10-24-1.log.gz"), gz.finish().unwrap()).unwrap();
        assert_eq!(account_names(&[dir]), vec!["Loupitwai".to_string()]);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn nothing_without_the_game() {
        let root = fake_root("empty");
        assert!(instances(Some(&root), Some(&root)).is_empty());
        let with_no_log = vec![instance(&root, "MCSR Ranked", None)];
        assert!(account_names(&with_no_log).is_empty());
        let _ = std::fs::remove_dir_all(&root);
    }
}

