//! The Microsoft Store build is an MSIX package of the same program. Windows gives a packaged
//! app an identity and manages a few things for it, so these differ from the installer build:
//! starting with Windows (see autostart.rs), the name notifications go out under (notify.rs)
//! and updates, which come from the Store (update.rs).

const STORE_PACKAGE_FAMILY: &str = "MagmaMCSR.MagmaMCSR_2rye98jc3yv92";

/// Reads a string from one of the `GetCurrent...` package functions, which fail when the
/// program does not run from a package.
#[cfg(windows)]
fn read(get: unsafe extern "system" fn(*mut u32, *mut u16) -> u32) -> Option<String> {
    const ERROR_INSUFFICIENT_BUFFER: u32 = 122;
    let mut len = 0u32;
    if unsafe { get(&mut len, std::ptr::null_mut()) } != ERROR_INSUFFICIENT_BUFFER {
        return None;
    }
    let mut buf = vec![0u16; len as usize];
    if unsafe { get(&mut len, buf.as_mut_ptr()) } != 0 {
        return None;
    }
    Some(String::from_utf16_lossy(&buf[..(len as usize).saturating_sub(1)]))
}

/// True when running from the MSIX package.
pub fn is_packaged() -> bool {
    #[cfg(windows)]
    {
        use windows_sys::Win32::Storage::Packaging::Appx::GetCurrentPackageFullName;
        return read(GetCurrentPackageFullName).is_some();
    }
    #[cfg(not(windows))]
    false
}

/// The name Windows knows this packaged app by, for notifications.
pub fn app_user_model_id() -> Option<String> {
    #[cfg(windows)]
    {
        use windows_sys::Win32::Storage::Packaging::Appx::GetCurrentApplicationUserModelId;
        return read(GetCurrentApplicationUserModelId);
    }
    #[cfg(not(windows))]
    None
}

/// The address of this app's page in the Microsoft Store app.
pub fn store_url() -> Option<String> {
    #[cfg(windows)]
    {
        use windows_sys::Win32::Storage::Packaging::Appx::GetCurrentPackageFamilyName;
        return read(GetCurrentPackageFamilyName).map(|name| format!("ms-windows-store://pdp/?PFN={name}"));
    }
    #[cfg(not(windows))]
    None
}

/// True when the installer build finds the Store build on this computer as well. Windows keeps a
/// data folder for every installed package, named after the package family.
pub fn store_build_installed() -> bool {
    !is_packaged()
        && std::env::var_os("LOCALAPPDATA")
            .is_some_and(|dir| std::path::Path::new(&dir).join("Packages").join(STORE_PACKAGE_FAMILY).is_dir())
}

/// True when this run was started at login, so it stays in the tray with no window. The installer
/// build gets `--background` from its startup entry; a packaged app cannot pass arguments there,
/// so Windows is asked how it was started.
pub fn started_at_login() -> bool {
    if std::env::args().any(|a| a == "--background") {
        return true;
    }
    #[cfg(windows)]
    if is_packaged() {
        use windows::ApplicationModel::Activation::ActivationKind;
        use windows::ApplicationModel::AppInstance;
        return AppInstance::GetActivatedEventArgs()
            .and_then(|args| args.Kind())
            .is_ok_and(|kind| kind == ActivationKind::StartupTask);
    }
    false
}
