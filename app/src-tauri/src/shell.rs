//! The main window: a custom title bar (local page) above the website, plus a settings
//! panel (local page) that slides over the website's right edge.
//!
//! The website is loaded as a separate webview with no IPC access. Only the local pages
//! (title bar and panel) can call native commands.
//!
//! Closing the window to the tray destroys it, website included, so an app that only
//! syncs in the background does not keep a browser engine in memory. Reopening rebuilds
//! the window on the last page, with the same size and position.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
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

/// The last file the website downloaded, shown in the title bar until it is opened.
#[derive(Default)]
pub struct LastDownload(Mutex<Option<std::path::PathBuf>>);

/// What to restore when the window is opened again after being closed to the tray.
#[derive(Default)]
pub struct WindowMemory(Mutex<Remembered>);

#[derive(Default, Clone)]
struct Remembered {
    /// Last page on the Magma website.
    url: Option<String>,
    position: Option<PhysicalPosition<i32>>,
    size: Option<PhysicalSize<u32>>,
    maximized: bool,
}

pub fn setup(app: &mut App) -> tauri::Result<()> {
    build_tray(app)?;
    // Started at login: stay in the tray. The window, and the website in it, is only
    // created when the user opens it.
    if !std::env::args().any(|a| a == "--background") {
        open_main(app.handle())?;
    }
    Ok(())
}

fn open_main(app: &AppHandle) -> tauri::Result<()> {
    let remembered = app.state::<WindowMemory>().0.lock().expect("window memory").clone();
    let site_url = remembered.url.as_deref().and_then(|u| u.parse().ok()).unwrap_or_else(|| SITE_URL.parse().expect("valid URL"));

    let window = WindowBuilder::new(app, "main")
        .title("Magma MCSR")
        .inner_size(1280.0, 800.0)
        .min_inner_size(480.0, 500.0)
        .decorations(false)
        .visible(false)
        .build()?;
    if let Some(size) = remembered.size {
        let _ = window.set_size(size);
    }
    if let Some(position) = remembered.position.filter(|p| on_a_monitor(app, *p)) {
        let _ = window.set_position(position);
    }
    if remembered.maximized {
        let _ = window.maximize();
    }

    let size = window.inner_size()?.to_logical::<f64>(window.scale_factor()?);
    let content_h = (size.height - TITLEBAR_HEIGHT).max(0.0);
    window.add_child(
        WebviewBuilder::new(TITLEBAR_LABEL, WebviewUrl::App("titlebar.html".into())),
        LogicalPosition::new(0.0, 0.0),
        LogicalSize::new(size.width, TITLEBAR_HEIGHT),
    )?;
    let site = window.add_child(
        // Let files dropped on the website reach the page itself (the Run Analyzer takes
        // dropped worlds), as in a browser. The app does nothing with dropped files.
        WebviewBuilder::new(SITE_LABEL, WebviewUrl::External(site_url))
            .disable_drag_drop_handler()
            .on_new_window(new_window_handler(app.clone()))
            .on_download(download_handler(app.clone()))
            .initialization_script(REMEMBER_POINTER_JS)
            .on_navigation(navigation_handler(app.clone())),
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

    let handle = app.clone();
    window.on_window_event(move |event| match event {
        WindowEvent::Resized(_) | WindowEvent::ScaleFactorChanged { .. } => {
            if let Some(w) = handle.get_window("main") {
                layout(&handle, &w);
                let _ = handle.emit_to(TITLEBAR_LABEL, "window-maximized", w.is_maximized().unwrap_or(false));
            }
            // Entering or leaving fullscreen resizes the window before the window reports
            // its new fullscreen state, so lay out once more after it settles.
            let later = handle.clone();
            std::thread::spawn(move || {
                std::thread::sleep(Duration::from_millis(150));
                let again = later.clone();
                let _ = later.run_on_main_thread(move || {
                    if let Some(w) = again.get_window("main") {
                        layout(&again, &w);
                    }
                });
            });
        }
        // The window is destroyed; sync keeps running (see the exit handling in lib.rs).
        WindowEvent::CloseRequested { .. } => {
            if let Some(w) = handle.get_window("main") {
                remember_geometry(&handle, &w);
            }
            handle.state::<PanelState>().0.store(false, Ordering::Relaxed);
        }
        _ => {}
    });

    watch_location(app.clone(), site);
    window.show()?;
    let _ = window.set_focus();

    // First run: open the panel so the user can set up sync.
    if app.state::<Store>().get().players.is_empty() {
        set_panel(app, true);
    }
    Ok(())
}

fn remember_geometry(app: &AppHandle, window: &Window) {
    let memory = app.state::<WindowMemory>();
    let mut m = memory.0.lock().expect("window memory");
    m.maximized = window.is_maximized().unwrap_or(false);
    // A maximized or minimized window reports the wrong size; keep the last normal one.
    if !m.maximized && !window.is_minimized().unwrap_or(false) {
        m.size = window.inner_size().ok();
        m.position = window.outer_position().ok();
    }
}

/// A saved position is only used if it is still on a connected monitor.
fn on_a_monitor(app: &AppHandle, p: PhysicalPosition<i32>) -> bool {
    app.available_monitors().unwrap_or_default().iter().any(|m| {
        let (pos, size) = (m.position(), m.size());
        p.x >= pos.x && p.y >= pos.y && p.x < pos.x + size.width as i32 && p.y < pos.y + size.height as i32
    })
}

/// Links that open a new window or tab on the website:
/// - other Magma pages open in this window, like a single-tab browser
/// - small popup windows from Magma itself (sized with window.open features) stay popups
/// - everything else opens in the user's default browser
fn new_window_handler(app: AppHandle) -> impl Fn(tauri::Url, tauri::webview::NewWindowFeatures) -> tauri::webview::NewWindowResponse<tauri::Wry> + Send + 'static {
    use tauri::webview::NewWindowResponse;
    move |url, features| {
        let magma = url.host_str().is_some_and(|h| h == "magmamcsr.com" || h.ends_with(".magmamcsr.com"));
        if magma && features.size().is_some() {
            return NewWindowResponse::Allow;
        }
        if magma {
            if let Some(site) = app.get_webview(SITE_LABEL) {
                let _ = site.navigate(url);
            }
        } else {
            open_externally(&app, &url);
        }
        NewWindowResponse::Deny
    }
}

/// Runs in the website: remembers only where the user last pressed, so a download can
/// fly from there into the title bar. Nothing is sent anywhere.
const REMEMBER_POINTER_JS: &str = r#"
addEventListener("pointerdown", (e) => { window.__magmaLastPointer = { x: e.clientX, y: e.clientY }; }, true);
"#;

/// Runs in the website: a small tag with the file name flies from the last press up to
/// the title bar, where the download notice is.
const DOWNLOAD_FLY_JS: &str = r##"
((targetX, name) => {
  const from = window.__magmaLastPointer || { x: innerWidth / 2, y: innerHeight / 2 };
  const tag = document.createElement("div");
  tag.textContent = name;
  Object.assign(tag.style, {
    position: "fixed", left: from.x + "px", top: from.y + "px", zIndex: "2147483647",
    pointerEvents: "none", padding: "4px 10px", borderRadius: "999px",
    background: "#1a1b1e", border: "1px solid #3a3b40", color: "#f2f2f4",
    font: "12px system-ui, sans-serif", whiteSpace: "nowrap", maxWidth: "200px",
    overflow: "hidden", textOverflow: "ellipsis", boxShadow: "0 4px 16px rgba(0,0,0,.45)",
  });
  document.documentElement.appendChild(tag);
  const dx = targetX - from.x, dy = -14 - from.y;
  const at = (x, y, s) => "translate(calc(-50% + " + x + "px), calc(-50% + " + y + "px)) scale(" + s + ")";
  tag.animate(
    [
      { transform: at(0, 0, 1), opacity: 1 },
      { transform: at(dx * 0.45, dy * 0.45 - 60, 0.95), opacity: 1, offset: 0.45 },
      { transform: at(dx, dy, 0.6), opacity: 0 },
    ],
    { duration: 550, easing: "cubic-bezier(.45,0,.25,1)" },
  ).onfinish = () => tag.remove();
})
"##;

/// Downloads from the website go to the usual Downloads folder. There is no browser
/// download bar, so the title bar shows the finished file instead.
fn download_handler(app: AppHandle) -> impl Fn(Webview, tauri::webview::DownloadEvent<'_>) -> bool + Send + Sync + 'static {
    use tauri::webview::DownloadEvent;
    let file_name = |p: &std::path::Path| p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    move |_, event| {
        match event {
            DownloadEvent::Requested { destination, .. } => {
                let _ = app.emit_to(TITLEBAR_LABEL, "download-started", file_name(destination));
            }
            DownloadEvent::Finished { path, success, .. } => {
                let name = path.as_deref().map(file_name).unwrap_or_default();
                if success {
                    *app.state::<LastDownload>().0.lock().expect("last download") = path;
                }
                let _ = app.emit_to(TITLEBAR_LABEL, "download-finished", serde_json::json!({ "name": name, "success": success }));
            }
            _ => {}
        }
        true
    }
}

/// Only these kinds of links are ever handed to the system. Anything else (file:, ms-*,
/// custom app schemes) is ignored, so a link on the website can never open local files
/// or launch programs.
fn open_externally(app: &AppHandle, url: &tauri::Url) {
    if !matches!(url.scheme(), "http" | "https" | "mailto") {
        eprintln!("[shell] ignored link with scheme {}", url.scheme());
        return;
    }
    use tauri_plugin_opener::OpenerExt;
    if let Err(e) = app.opener().open_url(url.as_str(), None::<&str>) {
        eprintln!("[shell] could not open {url}: {e}");
    }
}

/// Links in the same window: web pages load as usual, mailto: opens the mail app.
fn navigation_handler(app: AppHandle) -> impl Fn(&tauri::Url) -> bool + Send + 'static {
    move |url| {
        if matches!(url.scheme(), "http" | "https" | "about" | "data" | "blob") {
            return true;
        }
        open_externally(&app, url);
        false
    }
}

/// Title bar on top, website below, panel over the website's right edge.
/// While a video on the website is fullscreen, the website fills the whole window.
fn layout(app: &AppHandle, window: &Window) {
    let (Ok(size), Ok(scale)) = (window.inner_size(), window.scale_factor()) else { return };
    if window.is_fullscreen().unwrap_or(false) {
        for label in [TITLEBAR_LABEL, PANEL_LABEL] {
            if let Some(w) = app.get_webview(label) {
                let _ = w.hide();
            }
        }
        if let Some(site) = app.get_webview(SITE_LABEL) {
            let _ = site.set_position(PhysicalPosition::new(0, 0));
            let _ = site.set_size(size);
        }
        return;
    }
    if let Some(bar) = app.get_webview(TITLEBAR_LABEL) {
        let _ = bar.show();
    }
    if app.state::<PanelState>().0.load(Ordering::Relaxed) {
        if let Some(panel) = app.get_webview(PANEL_LABEL) {
            let _ = panel.show();
        }
    }
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
    } else if let Err(e) = open_main(app) {
        eprintln!("[shell] could not open the window: {e}");
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
        let mut was_fullscreen = false;
        // Placement of a window that was maximized when a video went fullscreen, put back
        // when it leaves fullscreen.
        let saved: std::sync::Arc<Mutex<Option<crate::placement::Placement>>> = Default::default();
        let mut tick: u32 = 0;
        loop {
            // Fullscreen is checked often so the switch looks immediate; the URL less often.
            std::thread::sleep(Duration::from_millis(50));
            tick = tick.wrapping_add(1);

            // A video on the website going fullscreen makes the window fullscreen, but a
            // maximized window stays clipped to the work area and reports no resize. Watch
            // the state here and fix the window up when it changes.
            let Some(w) = app.get_window("main") else { break };
            let fullscreen = w.is_fullscreen().unwrap_or(false);
            if fullscreen != was_fullscreen {
                was_fullscreen = fullscreen;
                let handle = app.clone();
                let saved = saved.clone();
                let _ = app.run_on_main_thread(move || {
                    let Some(w) = handle.get_window("main") else { return };
                    let mut saved = saved.lock().expect("placement");
                    if fullscreen && w.is_maximized().unwrap_or(false) {
                        // Un-maximize to a monitor-sized window first, so nothing visibly moves.
                        *saved = crate::placement::expand_normal(&w);
                        let _ = w.set_fullscreen(false);
                        let _ = w.unmaximize();
                        let _ = w.set_fullscreen(true);
                    } else if !fullscreen {
                        if let Some(p) = saved.take() {
                            crate::placement::restore(&w, &p);
                        }
                    }
                    layout(&handle, &w);
                });
            }

            if !tick.is_multiple_of(8) {
                continue;
            }
            let Ok(url) = site.url() else { break };
            let current = url.as_str().to_owned();
            if current == last {
                continue;
            }
            last = current;
            let host = url.host_str().unwrap_or_default().to_owned();
            if host.ends_with("magmamcsr.com") {
                app.state::<WindowMemory>().0.lock().expect("window memory").url = Some(last.clone());
            }
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
        "show-download" => {
            let Some(path) = app.state::<LastDownload>().0.lock().expect("last download").clone() else {
                return Ok(());
            };
            use tauri_plugin_opener::OpenerExt;
            return app.opener().reveal_item_in_dir(path).map_err(|e| e.to_string());
        }
        _ => return Err(format!("unknown action {action}")),
    };
    result.map_err(|e| e.to_string())
}

/// Fly a download tag across the website to the title bar notice at `x` (CSS pixels).
#[tauri::command]
pub fn download_fly(webview: Webview, x: f64, name: String) -> Result<(), String> {
    if webview.label() != TITLEBAR_LABEL {
        return Err("not allowed".into());
    }
    let site = webview.app_handle().get_webview(SITE_LABEL).ok_or("site webview missing")?;
    let name = serde_json::to_string(&name).map_err(|e| e.to_string())?;
    site.eval(format!("{DOWNLOAD_FLY_JS}({x}, {name})")).map_err(|e| e.to_string())
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
