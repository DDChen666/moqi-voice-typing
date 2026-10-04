//! Tauri commands for the Yuyin settings panel (`src/yuyin/` in the frontend).

use tauri::AppHandle;

use super::config::{self, YuyinConfig};
use super::{polish, secrets};

#[tauri::command]
#[specta::specta]
pub fn yuyin_get_config(app: AppHandle) -> YuyinConfig {
    config::get(&app)
}

#[tauri::command]
#[specta::specta]
pub fn yuyin_set_config(app: AppHandle, config: YuyinConfig) -> Result<(), String> {
    config::set(&app, config)?;
    super::sync::changed();
    Ok(())
}

/// The host whose key the settings mean: `base_url` when the page names one
/// (the service being edited), else the saved configuration's service.
fn key_host(app: &AppHandle, base_url: Option<String>) -> String {
    match base_url {
        Some(url) => polish::host(&url),
        None => polish::key_host(&config::get(app)).unwrap_or_default(),
    }
}

/// Whether a key is stored for that service. The key itself is never sent to
/// the frontend.
#[tauri::command]
#[specta::specta]
pub fn yuyin_has_api_key(app: AppHandle, base_url: Option<String>) -> bool {
    secrets::has_api_key(&key_host(&app, base_url))
}

/// Store the key for that service in the keychain; an empty string removes it.
#[tauri::command]
#[specta::specta]
pub fn yuyin_set_api_key(
    app: AppHandle,
    key: String,
    base_url: Option<String>,
) -> Result<(), String> {
    secrets::set_api_key(&key_host(&app, base_url), &key)
}

/// The corrections learned from the user's edits (dictionary page).
#[tauri::command]
#[specta::specta]
pub fn yuyin_learned(app: AppHandle) -> Vec<super::learn::Rule> {
    super::learn::rules(&app)
}

/// Apply a learned correction from now on (`active`), or forget it for good.
#[tauri::command]
#[specta::specta]
pub fn yuyin_set_learned(app: AppHandle, from: String, to: String, active: bool) {
    super::learn::set_rule(&app, &from, &to, active);
}

/// Where this computer syncs, when it last did, and with which computers.
#[tauri::command]
#[specta::specta]
pub fn yuyin_sync_status(app: AppHandle) -> super::sync::Status {
    super::sync::status(&app)
}

/// Sync through `folder` (a cloud drive folder), or stop with `None`.
#[tauri::command]
#[specta::specta]
pub fn yuyin_set_sync_folder(app: AppHandle, folder: Option<String>) -> Result<(), String> {
    super::sync::set_folder(&app, folder)
}

/// Sync right away (the settings page's button).
#[tauri::command]
#[specta::specta]
pub async fn yuyin_sync_now(app: AppHandle) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || super::sync::sync_now(&app))
        .await
        .map_err(|e| e.to_string())?
}

/// The apps the user recently dictated into, for per-app styles.
#[tauri::command]
#[specta::specta]
pub fn yuyin_recent_apps(app: AppHandle) -> Vec<super::apps::RecentApp> {
    super::apps::recent(&app)
}

/// The models of a server on this computer (Ollama, LM Studio).
#[tauri::command]
#[specta::specta]
pub async fn yuyin_local_models(base_url: String) -> Result<Vec<String>, String> {
    polish::local_models(&base_url).await
}

/// OpenRouter's public model catalog for the model picker.
#[tauri::command]
#[specta::specta]
pub async fn yuyin_openrouter_models() -> Result<Vec<polish::ModelInfo>, String> {
    polish::openrouter_models().await
}

/// The home page's numbers (all computed locally from the timings log).
#[tauri::command]
#[specta::specta]
pub fn yuyin_stats(app: AppHandle) -> super::stats::Stats {
    super::stats::stats(&app)
}

/// Per-entry details for the history page, keyed by the recording's file name.
#[tauri::command]
#[specta::specta]
pub fn yuyin_history_meta(
    app: AppHandle,
) -> std::collections::HashMap<String, super::stats::EntryMeta> {
    super::stats::history_meta(&app)
}

/// History's 重貼: go back to the app the user was in and paste `text` there.
/// Hiding Moqi hands focus back to the previous app on macOS; Windows leaves
/// the hidden window in the foreground, so there we hand focus back
/// ourselves. Then the paste follows the normal path (clipboard restored
/// afterwards).
#[tauri::command]
#[specta::specta]
pub async fn yuyin_repaste(app: AppHandle, text: String) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    app.hide().map_err(|e| e.to_string())?;
    #[cfg(not(target_os = "macos"))]
    if let Some(window) = tauri::Manager::get_webview_window(&app, "main") {
        window.hide().map_err(|e| e.to_string())?;
    }
    tauri::async_runtime::spawn_blocking(move || {
        // Let the window server finish switching the frontmost app.
        std::thread::sleep(std::time::Duration::from_millis(250));
        #[cfg(target_os = "windows")]
        super::focus_return::restore();
        let (tx, rx) = std::sync::mpsc::channel();
        let app_for_paste = app.clone();
        app.run_on_main_thread(move || {
            let _ = tx.send(crate::utils::paste(text, app_for_paste));
        })
        .map_err(|e| e.to_string())?;
        rx.recv().map_err(|e| e.to_string())?
    })
    .await
    .map_err(|e| e.to_string())?
}

/// A frontend crash, written to the app log (a blank window says nothing).
#[tauri::command]
#[specta::specta]
pub fn yuyin_report_error(message: String) {
    log::error!("frontend: {message}");
}

/// Run the clean-up on sample text with the current settings and report
/// errors (bad key, timeout) instead of falling back.
#[tauri::command]
#[specta::specta]
pub async fn yuyin_test_polish(app: AppHandle, text: String) -> Result<String, String> {
    polish::test(&app, &text).await
}
