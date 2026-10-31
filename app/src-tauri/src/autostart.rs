//! "Start with Windows": a value under HKCU\Software\Microsoft\Windows\CurrentVersion\Run.
//!
//! Written here rather than with a plugin so the program path is quoted. The install
//! folder ("Magma MCSR") contains a space, and an unquoted path makes Windows try other
//! programs first (C:\...\Local\Magma.exe), a known way to hijack startup entries.
//!
//! The Microsoft Store package cannot use that entry, which Windows ignores for packaged apps.
//! It declares a startup task in its manifest instead, and Windows keeps the on/off state. The
//! user can also change that state in Windows Settings or Task Manager, and then only they can
//! turn it back on.

use windows_sys::Win32::Foundation::ERROR_SUCCESS;
use windows_sys::Win32::System::Registry::{RegDeleteKeyValueW, RegSetKeyValueW, HKEY_CURRENT_USER, REG_SZ};

const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const VALUE_NAME: &str = "Magma MCSR";

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Id of the startup task in the package manifest.
const STARTUP_TASK: &str = "MagmaStartup";

fn startup_task() -> Result<windows::ApplicationModel::StartupTask, String> {
    windows::ApplicationModel::StartupTask::GetAsync(&windows::core::HSTRING::from(STARTUP_TASK))
        .and_then(|task| task.join())
        .map_err(|e| e.to_string())
}

fn enable_packaged() -> Result<(), String> {
    use windows::ApplicationModel::StartupTaskState;
    let state = startup_task()?.RequestEnableAsync().and_then(|s| s.join()).map_err(|e| e.to_string())?;
    match state {
        StartupTaskState::Enabled | StartupTaskState::EnabledByPolicy => Ok(()),
        _ => Err("Windows has this turned off. Turn on Magma MCSR in Settings > Apps > Startup.".into()),
    }
}

/// Start the app at login, hidden in the tray (`--background`).
pub fn enable() -> Result<(), String> {
    if crate::packaged::is_packaged() {
        return enable_packaged();
    }
    // A development build shares its settings with the installed app; never point the
    // startup entry at it.
    if cfg!(debug_assertions) {
        eprintln!("[autostart] development build: startup entry left unchanged");
        return Ok(());
    }
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let command = wide(&format!("\"{}\" --background", exe.display()));
    let status = unsafe {
        RegSetKeyValueW(
            HKEY_CURRENT_USER,
            wide(RUN_KEY).as_ptr(),
            wide(VALUE_NAME).as_ptr(),
            REG_SZ,
            command.as_ptr().cast(),
            (command.len() * 2) as u32,
        )
    };
    if status == ERROR_SUCCESS { Ok(()) } else { Err(format!("could not write the startup entry (error {status})")) }
}

pub fn disable() -> Result<(), String> {
    if crate::packaged::is_packaged() {
        return startup_task()?.Disable().map_err(|e| e.to_string());
    }
    if cfg!(debug_assertions) {
        eprintln!("[autostart] development build: startup entry left unchanged");
        return Ok(());
    }
    let status = unsafe { RegDeleteKeyValueW(HKEY_CURRENT_USER, wide(RUN_KEY).as_ptr(), wide(VALUE_NAME).as_ptr()) };
    // Already gone is fine.
    if status == ERROR_SUCCESS || status == 2 { Ok(()) } else { Err(format!("could not remove the startup entry (error {status})")) }
}
