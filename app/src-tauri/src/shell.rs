//! The main window: a custom title bar (local page) above the website, plus a settings
//! panel (local page) that slides over the website's right edge.
//!
//! The website is loaded as a separate webview with no IPC access. Only the local pages
//! (title bar and panel) can call native commands.

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use serde::Serialize;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::webview::WebviewBuilder;
use tauri::window::WindowBuilder;
use tauri::{App, AppHandle, Emitter, LogicalPosition, LogicalSize, Manager, PhysicalPosition, PhysicalSize, Webview, WebviewUrl, Window, WindowEvent};

use crate::settings::{Settings, Store};
use crate::sync::{Engine, Live, Phase};

pub const SITE_URL: &str = "https://magmamcsr.com";
/// Title bar height and panel width in logical pixels.
const TITLEBAR_HEIGHT: f64 = 36.0;
const PANEL_WIDTH: f64 = 380.0;
pub const TITLEBAR_LABEL: &str = "titlebar";
pub const PANEL_LABEL: &str = "panel";
const SITE_LABEL: &str = "site";

/// Whether the settings panel is showing.
#[derive(Default)]
pub struct PanelState(AtomicBool);

pub fn build(app: &mut App) -> tauri::Result<()> {
    let window = WindowBuilder::new(app, "main")
        .title("Magma MCSR")
        .inner_size(1280.0, 800.0)
        .min_inner_size(480.0, 500.0)
        .decorations(false)
        // Started at login: stay in the tray until the user opens the window.
        .visible(!std::env::args().any(|a| a == "--background"))
        .build()?;

    let size = window.inner_size()?.to_logical::<f64>(window.scale_factor()?);
    let content_h = (size.height - TITLEBAR_HEIGHT).max(0.0);
    window.add_child(
        WebviewBuilder::new(TITLEBAR_LABEL, WebviewUrl::App("titlebar.html".into())),
        LogicalPosition::new(0.0, 0.0),
        LogicalSize::new(size.width, TITLEBAR_HEIGHT),
    )?;
    let site = window.add_child(
        WebviewBuilder::new(SITE_LABEL, WebviewUrl::External(SITE_URL.parse().expect("valid URL"))),
        LogicalPosition::new(0.0, TITLEBAR_HEIGHT),
        LogicalSize::new(size.width, content_h),
    )?;
    // Added last so it sits on top of the website.
    let panel = window.add_child(
        // Tauri intercepts drag events for file drops on Windows, which breaks HTML
        // drag-and-drop (used to reorder players). The panel never takes file drops.
        WebviewBuilder::new(PANEL_LABEL, WebviewUrl::App("panel.html".into())).disable_drag_drop_handler(),
        LogicalPosition::new((size.width - PANEL_WIDTH).max(0.0), TITLEBAR_HEIGHT),
        LogicalSize::new(PANEL_WIDTH.min(size.width), content_h),
    )?;
    panel.hide()?;

    let handle = app.handle().clone();
    window.on_window_event(move |event| match event {
        WindowEvent::Resized(_) | WindowEvent::ScaleFactorChanged { .. } => {
            if let Some(w) = handle.get_window("main") {
                layout(&handle, &w);
                let _ = handle.emit_to(TITLEBAR_LABEL, "window-maximized", w.is_maximized().unwrap_or(false));
            }
        }
        WindowEvent::CloseRequested { api, .. } => {
            if handle.state::<Store>().get().run_in_tray {
                api.prevent_close();
                if let Some(w) = handle.get_window("main") {
                    let _ = w.hide();
                }
            }
        }
        _ => {}
    });

    build_tray(app)?;
    watch_location(app.handle().clone(), site);

    // First run: open the panel so the user can set up sync.
    if app.state::<Store>().get().players.is_empty() {
        set_panel(app.handle(), true);
    }
    Ok(())
}

/// Title bar on top, website below, panel over the website's right edge.
fn layout(app: &AppHandle, window: &Window) {
    let (Ok(size), Ok(scale)) = (window.inner_size(), window.scale_factor()) else { return };
    let bar_h = ((TITLEBAR_HEIGHT * scale).round() as u32).min(size.height);
    let content_h = size.height - bar_h;
    if let Some(bar) = app.get_webview(TITLEBAR_LABEL) {
        let _ = bar.set_position(PhysicalPosition::new(0, 0));
        let _ = bar.set_size(PhysicalSize::new(size.width, bar_h));
    }
    if let Some(site) = app.get_webview(SITE_LABEL) {
        let _ = site.set_position(PhysicalPosition::new(0, bar_h as i32));
        let _ = site.set_size(PhysicalSize::new(size.width, content_h));
    }
    if let Some(panel) = app.get_webview(PANEL_LABEL) {
        let panel_w = ((PANEL_WIDTH * scale).round() as u32).min(size.width);
        let _ = panel.set_position(PhysicalPosition::new((size.width - panel_w) as i32, bar_h as i32));
        let _ = panel.set_size(PhysicalSize::new(panel_w, content_h));
    }
}

pub fn set_panel(app: &AppHandle, open: bool) {
    let Some(panel) = app.get_webview(PANEL_LABEL) else { return };
    app.state::<PanelState>().0.store(open, Ordering::Relaxed);
    if open {
        if let Some(w) = app.get_window("main") {
            layout(app, &w);
        }
        let _ = panel.show();
        let _ = panel.set_focus();
    } else {
        let _ = panel.hide();
    }
    let _ = app.emit_to(TITLEBAR_LABEL, "panel-open", open);
}

fn show_main(app: &AppHandle) {
    if let Some(w) = app.get_window("main") {
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
    }
}

fn build_tray(app: &App) -> tauri::Result<()> {
    let open = MenuItem::with_id(app, "open", "Open Magma MCSR", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&open, &quit])?;
    let mut tray = TrayIconBuilder::with_id("main").tooltip("Magma MCSR").menu(&menu).show_menu_on_left_click(false);
    if let Some(icon) = app.default_window_icon() {
        tray = tray.icon(icon.clone());
    }
    tray.on_menu_event(|app, event| match event.id.as_ref() {
        "open" => show_main(app),
        "quit" => app.exit(0),
        _ => {}
    })
    .on_tray_icon_event(|tray, event| {
        if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = event {
            show_main(tray.app_handle());
        }
    })
    .build(app)?;
    Ok(())
}

// ---------- sync status (shown in the title bar pill and the panel) ----------

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SyncStatus {
    /// "setup" | "paused" | "busy" | "warning" | "ready"
    pub state: &'static str,
    pub label: String,
}

pub fn status_for(s: &Settings, live: &Live) -> SyncStatus {
    let status = |state, label: String| SyncStatus { state, label };
    if s.players.is_empty() {
        return status("setup", "Set up replay sync".into());
    }
    if live.update_required {
        return status("warning", "Update required".into());
    }
    if s.paused {
        return status("paused", "Paused".into());
    }
    match live.phase {
        Phase::Checking => status("busy", "Checking for new matches".into()),
        Phase::Downloading if live.expiring > 0 => {
            status("warning", format!("Downloading {} of {} · {} expiring", live.done + 1, live.total, live.expiring))
        }
        Phase::Downloading => status("busy", format!("Downloading {} of {}", live.done + 1, live.total)),
        Phase::Idle if live.pending_uploads > 0 => status("ready", format!("Synced · {} to upload", live.pending_uploads)),
        Phase::Idle => status("ready", "Synced".into()),
    }
}

pub fn broadcast_status(app: &AppHandle, s: &Settings) {
    let status = status_for(s, &app.state::<Engine>().live());
    let _ = app.emit_to(TITLEBAR_LABEL, "sync-status", status.clone());
    let _ = app.emit_to(PANEL_LABEL, "sync-status", status);
}

// ---------- website location ----------

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct SiteLocation {
    path: String,
    host: String,
    external: bool,
}

/// The website is a single-page app, so route changes do not fire navigation events.
/// Poll its URL and tell the title bar when it changes.
fn watch_location(app: AppHandle, site: Webview) {
    std::thread::spawn(move || {
        let mut last = String::new();
        loop {
            std::thread::sleep(Duration::from_millis(400));
            let Ok(url) = site.url() else { break };
            let current = url.as_str().to_owned();
            if current == last {
                continue;
            }
            last = current;
            let host = url.host_str().unwrap_or_default().to_owned();
            let location = SiteLocation {
                path: url.path().to_owned(),
                external: !host.ends_with("magmamcsr.com"),
                host,
            };
            let _ = app.emit_to(TITLEBAR_LABEL, "site-location", location);
        }
    });
}

// ---------- title bar commands ----------

/// Title bar buttons. Rejected unless the call comes from the title bar webview.
#[tauri::command]
pub fn titlebar(webview: Webview, action: &str) -> Result<(), String> {
    if webview.label() != TITLEBAR_LABEL {
        return Err("not allowed".into());
    }
    let app = webview.app_handle();
    let window = webview.window();
    let site = || app.get_webview(SITE_LABEL).ok_or_else(|| "site webview missing".to_string());
    let result = match action {
        "back" => site()?.eval("history.back()"),
        "forward" => site()?.eval("history.forward()"),
        "reload" => site()?.eval("location.reload()"),
        "panel" => {
            let open = !app.state::<PanelState>().0.load(Ordering::Relaxed);
            set_panel(app, open);
            Ok(())
        }
        "minimize" => window.minimize(),
        "maximize" => match window.is_maximized() {
            Ok(true) => window.unmaximize(),
            _ => window.maximize(),
        },
        "close" => window.close(),
        "drag" => window.start_dragging(),
        "update" => return crate::update::install_ready(app),
        _ => return Err(format!("unknown action {action}")),
    };
    result.map_err(|e| e.to_string())
}

/// Downloaded update waiting to be installed, for the title bar when it first loads.
#[tauri::command]
pub fn update_status(webview: Webview) -> Result<crate::update::UpdateStatus, String> {
    if webview.label() != TITLEBAR_LABEL {
        return Err("not allowed".into());
    }
    Ok(webview.app_handle().state::<crate::update::Updates>().status())
}

/// Current sync status, for the title bar and panel when they first load.
#[tauri::command]
pub fn sync_status(webview: Webview, store: tauri::State<'_, Store>) -> Result<SyncStatus, String> {
    if webview.label() != TITLEBAR_LABEL && webview.label() != PANEL_LABEL {
        return Err("not allowed".into());
    }
    Ok(status_for(&store.get(), &webview.app_handle().state::<Engine>().live()))
}
