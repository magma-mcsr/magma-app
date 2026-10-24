//! Finding MCSR Ranked on this computer: its instances in the usual launcher folders, and the
//! Minecraft names that played on them.
//!
//! MCSR Ranked's own guide recommends Prism Launcher or MultiMC. MultiMC and portable Prism can be
//! unpacked anywhere, so besides the installers' fixed folders a few usual places are looked at
//! (home, Desktop, Downloads, Documents, Games, drive roots), one level deep, for a folder with an
//! `instances` folder and the launcher's program or config file. Only folder names and whether
//! those files exist are checked there.
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

/// Files that mark a portable launcher folder.
const LAUNCHER_MARKS: [&str; 4] = ["MultiMC.exe", "multimc.cfg", "prismlauncher.exe", "prismlauncher.cfg"];

/// Where to look: %APPDATA%, the home folder, and the drive roots. Development builds can point all
/// of it at one test folder with MAGMA_GAME_ROOT (no drive roots then), to try a computer with or
/// without the game.
pub struct Roots {
    pub roaming: Option<PathBuf>,
    pub home: Option<PathBuf>,
    pub drives: Vec<PathBuf>,
}

pub fn roots(app: &AppHandle) -> Roots {
    #[cfg(debug_assertions)]
    if let Some(root) = std::env::var_os("MAGMA_GAME_ROOT") {
        let root = PathBuf::from(root);
        return Roots { roaming: Some(root.clone()), home: Some(root), drives: Vec::new() };
    }
    let drives = ('C'..='H').map(|d| PathBuf::from(format!("{d}:\\"))).filter(|p| p.is_dir()).collect();
    Roots { roaming: app.path().data_dir().ok(), home: app.path().home_dir().ok(), drives }
}

/// Portable launcher folders (MultiMC, portable Prism) in the usual places, one level deep.
fn portable_launchers(home: Option<&Path>, drives: &[PathBuf]) -> Vec<PathBuf> {
    let mut places: Vec<PathBuf> = drives.to_vec();
    if let Some(h) = home {
        places.push(h.to_path_buf());
        for sub in ["Desktop", "Downloads", "Documents", "Games", "OneDrive/Desktop", "OneDrive/Documents"] {
            places.push(h.join(sub));
        }
        // Installed with scoop
        places.push(h.join("scoop").join("persist"));
    }
    let is_launcher = |d: &Path| d.join("instances").is_dir() && LAUNCHER_MARKS.iter().any(|m| d.join(m).is_file());
    let mut found = Vec::new();
    for place in places {
        if is_launcher(&place) {
            found.push(place.clone());
        }
        if let Ok(entries) = std::fs::read_dir(&place) {
            for entry in entries.flatten() {
                let d = entry.path();
                if d.is_dir() && is_launcher(&d) {
                    found.push(d);
                }
            }
        }
    }
    found
}

/// Game folders (the `minecraft` or `.minecraft` of an instance) that have MCSR Ranked installed,
/// most recently played first.
pub fn instances(roaming: Option<&Path>, home: Option<&Path>, drives: &[PathBuf]) -> Vec<PathBuf> {
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
    for launcher in portable_launchers(home, drives) {
        add_instances(&mut candidates, launcher.join("instances"), &["minecraft", ".minecraft"]);
    }
    candidates.sort();
    candidates.dedup();
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
        let found = instances(Some(&root), None, &[]);
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
    fn finds_portable_launchers() {
        for (name, rel, mark) in [
            ("multimc-desktop", "Desktop/MultiMC", "MultiMC.exe"),
            ("prism-portable", "Downloads/PrismLauncher-Windows", "prismlauncher.exe"),
            ("multimc-home", "MultiMC", "multimc.cfg"),
        ] {
            let root = fake_root(name);
            let launcher = root.join(rel);
            std::fs::create_dir_all(launcher.join("instances").join("Ranked").join(".minecraft").join("mcsrranked")).unwrap();
            std::fs::write(launcher.join(mark), b"").unwrap();
            let found = instances(None, Some(&root), &[]);
            assert_eq!(found.len(), 1, "{name}");
            assert!(found[0].ends_with("Ranked/.minecraft"), "{name}: {:?}", found[0]);
            let _ = std::fs::remove_dir_all(&root);
        }
        // A drive root counts too
        let drive = fake_root("drive");
        std::fs::create_dir_all(drive.join("MultiMC").join("instances").join("R").join("minecraft").join("mcsrranked")).unwrap();
        std::fs::write(drive.join("MultiMC").join("MultiMC.exe"), b"").unwrap();
        assert_eq!(instances(None, None, &[drive.clone()]).len(), 1);
        // A folder that merely has "instances" is not a launcher
        let other = fake_root("other");
        std::fs::create_dir_all(other.join("Desktop").join("stuff").join("instances").join("x").join("minecraft").join("mcsrranked")).unwrap();
        assert!(instances(None, Some(&other), &[]).is_empty());
        let _ = std::fs::remove_dir_all(&drive);
        let _ = std::fs::remove_dir_all(&other);
    }

    #[test]
    fn nothing_without_the_game() {
        let root = fake_root("empty");
        assert!(instances(Some(&root), Some(&root), &[]).is_empty());
        let with_no_log = vec![instance(&root, "MCSR Ranked", None)];
        assert!(account_names(&with_no_log).is_empty());
        let _ = std::fs::remove_dir_all(&root);
    }
}

