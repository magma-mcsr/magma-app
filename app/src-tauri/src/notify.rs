//! A Windows notification when a match the first listed player just finished is analyzed.
//! Only those matches: with many players listed, anything else would notify all the time.
//! Clicking it opens the app on that match.

use tauri::AppHandle;

/// Show "Analysis ready" for a match; a click shows the window on the match page.
pub fn analysis_ready(app: &AppHandle, nickname: &str, match_id: i64, season: Option<i64>, summary: &str) {
    #[cfg(windows)]
    {
        use tauri_winrt_notification::Toast;
        // An installed app has its own identity (the installer's shortcut); a development build
        // has none, so Windows only shows notifications that borrow PowerShell's.
        let app_id = if cfg!(debug_assertions) { Toast::POWERSHELL_APP_ID } else { "com.magmamcsr.app" };
        let handle = app.clone();
        let nickname = nickname.to_owned();
        let shown = Toast::new(app_id)
            .title("Analysis ready")
            .text1(summary)
            .text2("Click to open it on Magma")
            .on_activated(move |_| {
                let app = handle.clone();
                let nickname = nickname.clone();
                let _ = handle.run_on_main_thread(move || crate::shell::show_match(&app, &nickname, match_id, season));
                Ok(())
            })
            .show();
        if let Err(e) = shown {
            eprintln!("[notify] {e}");
        }
    }
    #[cfg(not(windows))]
    let _ = (app, nickname, match_id, season, summary);
}
