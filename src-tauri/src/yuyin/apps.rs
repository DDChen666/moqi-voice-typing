//! Per-app styles (每個 App 的風格): for an app the user picks, a writing
//! style other than the automatic one, an extra instruction for the clean-up
//! ("用敬語"), and a language to write in. The apps to choose from are the
//! ones the user recently dictated into (yuyin_apps.json, this computer only).

use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use log::warn;
use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::{AppHandle, Manager};

use super::config::YuyinConfig;
use super::context::{Context, FrontApp};

const FILE: &str = "yuyin_apps.json";
const KEEP: usize = 30;

/// The user's choices for one app.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Type)]
pub struct AppStyle {
    /// Bundle id (macOS) or executable (Windows); for a web app, with the
    /// site: `context::style_key`.
    pub app: String,
    /// The app's name as shown in settings.
    pub name: String,
    /// The writing style; `None`: decided automatically.
    pub context: Option<Context>,
    /// An extra instruction for the clean-up.
    pub note: String,
    /// Output language code ("en", "ja"…); `None`: follow the global setting;
    /// `Some("")`: never translate in this app.
    pub translate_to: Option<String>,
}

/// An app the user dictated into recently.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Type)]
pub struct RecentApp {
    pub app: String,
    pub name: String,
    /// The style Moqi picks for it automatically.
    pub context: Context,
    /// Unix milliseconds.
    pub last_used: f64,
}

static RECENT: Mutex<Option<Vec<RecentApp>>> = Mutex::new(None);

fn path(app: &AppHandle) -> Option<PathBuf> {
    app.path().app_data_dir().ok().map(|d| d.join(FILE))
}

fn with_recent<T>(app: &AppHandle, f: impl FnOnce(&mut Vec<RecentApp>) -> T) -> T {
    let mut slot = RECENT.lock().unwrap_or_else(|e| e.into_inner());
    let list = slot.get_or_insert_with(|| {
        path(app)
            .and_then(|p| std::fs::read_to_string(p).ok())
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    });
    f(list)
}

/// Remember that the user dictated into `front` (style picked: `auto`).
pub fn note_used(app: &AppHandle, front: &FrontApp, name: &str, auto: Context) {
    if front.bundle_id.is_empty() || super::context::is_system_prompt(front) {
        return;
    }
    let key = super::context::style_key(front);
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as f64)
        .unwrap_or(0.0);
    let json = with_recent(app, |list| {
        list.retain(|r| r.app != key);
        list.insert(
            0,
            RecentApp {
                app: key.clone(),
                name: name.to_string(),
                context: auto,
                last_used: now,
            },
        );
        list.truncate(KEEP);
        serde_json::to_string_pretty(list)
    });
    if let (Some(p), Ok(json)) = (path(app), json) {
        if let Err(e) = std::fs::write(p, json) {
            warn!("Failed to save recent apps: {e}");
        }
    }
}

pub fn recent(app: &AppHandle) -> Vec<RecentApp> {
    with_recent(app, |list| list.clone())
}

pub fn style_for<'a>(cfg: &'a YuyinConfig, app: &str) -> Option<&'a AppStyle> {
    cfg.app_styles.iter().find(|s| s.app == app)
}

/// The output language for `app`: its own choice, else the global one.
pub fn translate_to(cfg: &YuyinConfig, style: Option<&AppStyle>) -> Option<String> {
    match style.and_then(|s| s.translate_to.clone()) {
        Some(code) if code.is_empty() => None,
        Some(code) => Some(code),
        None => cfg.translate_to.clone().filter(|c| !c.is_empty()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn style(translate_to: Option<&str>) -> AppStyle {
        AppStyle {
            app: "slack.exe".into(),
            name: "Slack".into(),
            context: None,
            note: String::new(),
            translate_to: translate_to.map(str::to_string),
        }
    }

    #[test]
    fn an_apps_language_overrides_the_global_one() {
        let cfg = YuyinConfig {
            translate_to: Some("ja".into()),
            ..YuyinConfig::default()
        };
        assert_eq!(translate_to(&cfg, None).as_deref(), Some("ja"));
        assert_eq!(
            translate_to(&cfg, Some(&style(None))).as_deref(),
            Some("ja")
        );
        assert_eq!(
            translate_to(&cfg, Some(&style(Some("en")))).as_deref(),
            Some("en")
        );
        assert_eq!(translate_to(&cfg, Some(&style(Some("")))), None);
        assert_eq!(translate_to(&YuyinConfig::default(), None), None);
    }
}
