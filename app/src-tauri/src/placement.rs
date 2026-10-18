//! Windows window placement, used so a maximized window can go fullscreen and come
//! back without visibly shrinking in between.

#[cfg(windows)]
mod imp {
    use windows_sys::Win32::Foundation::{HWND, POINT, RECT};
    use windows_sys::Win32::Graphics::Gdi::{
        GetMonitorInfoW, MonitorFromPoint, MonitorFromWindow, MONITORINFO, MONITOR_DEFAULTTONEAREST,
        MONITOR_DEFAULTTOPRIMARY,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{GetWindowPlacement, SetWindowPlacement, WINDOWPLACEMENT};

    /// A window's placement (maximized or not, and its normal size and position).
    #[derive(Clone, Copy)]
    pub struct Placement(WINDOWPLACEMENT);

    // Only touched on the main thread; the raw handle inside is just an id.
    unsafe impl Send for Placement {}

    fn hwnd(window: &tauri::Window) -> Option<HWND> {
        window.hwnd().ok().map(|h| h.0 as HWND)
    }

    fn monitor_info(monitor: windows_sys::Win32::Graphics::Gdi::HMONITOR) -> Option<MONITORINFO> {
        let mut info = MONITORINFO { cbSize: size_of::<MONITORINFO>() as u32, ..Default::default() };
        (unsafe { GetMonitorInfoW(monitor, &mut info) } != 0).then_some(info)
    }

    /// Remember the current placement, then make the window's normal (restored) size
    /// cover its whole monitor, so un-maximizing it does not visibly shrink it.
    pub fn expand_normal(window: &tauri::Window) -> Option<Placement> {
        let hwnd = hwnd(window)?;
        let mut original = WINDOWPLACEMENT { length: size_of::<WINDOWPLACEMENT>() as u32, ..Default::default() };
        if unsafe { GetWindowPlacement(hwnd, &mut original) } == 0 {
            return None;
        }
        let screen = monitor_info(unsafe { MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST) })?.rcMonitor;
        // The normal position is in workspace coordinates: offset by the primary
        // monitor's work area (non-zero when the taskbar is on the top or left).
        let primary = monitor_info(unsafe { MonitorFromPoint(POINT { x: 0, y: 0 }, MONITOR_DEFAULTTOPRIMARY) })?;
        let (dx, dy) = (primary.rcWork.left - primary.rcMonitor.left, primary.rcWork.top - primary.rcMonitor.top);
        let mut expanded = original;
        expanded.rcNormalPosition =
            RECT { left: screen.left - dx, top: screen.top - dy, right: screen.right - dx, bottom: screen.bottom - dy };
        unsafe { SetWindowPlacement(hwnd, &expanded) };
        Some(Placement(original))
    }

    /// Put back a placement saved by [`expand_normal`].
    pub fn restore(window: &tauri::Window, placement: &Placement) {
        if let Some(hwnd) = hwnd(window) {
            unsafe { SetWindowPlacement(hwnd, &placement.0) };
        }
    }
}

#[cfg(windows)]
pub use imp::*;
