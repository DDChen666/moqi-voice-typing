//! Yuyin's own settings, kept in `yuyin.json` next to Handy's settings store
//! so upstream's busy `settings.rs` stays untouched. The API key is NOT here:
//! it lives in the macOS Keychain (see [`super::secrets`]).

use std::path::PathBuf;
use std::sync::RwLock;

use log::{error, warn};
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::{AppHandle, Manager};

const CONFIG_FILE: &str = "yuyin.json";

/// How much the LLM may change the transcript (product definition, section B).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum Level {
    /// 原話: punctuation, Traditional Chinese, dictionary. Never calls the LLM.
    Raw,
    /// 整理 (default): drop fillers, keep the last version of a correction,
    /// digits, spoken lists become lists. Never swaps the user's words.
    Tidy,
    /// 潤飾: rewrite into written style.
    Polish,
}

/// Where the clean-up runs (settings: 潤稿服務).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum Service {
    /// DeepSeek flash with reasoning off, the M0 pick.
    Deepseek,
    /// OpenRouter: one key for many providers' models, picked in settings.
    Openrouter,
    /// Any OpenAI-compatible endpoint at `base_url` / `model`.
    Custom,
    /// A model on this computer (Ollama, LM Studio): no key, nothing leaves.
    Local,
    /// Nothing leaves the machine: every level behaves like 原話.
    None,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Type)]
#[serde(default)]
pub struct YuyinConfig {
    pub level: Level,
    pub service: Service,
    /// Personal dictionary: correct spellings of names and terms the user says.
    pub vocab: Vec<String>,
    /// Any OpenAI-compatible endpoint; DeepSeek by default.
    pub base_url: String,
    pub model: String,
    /// After this, paste the raw transcript instead of waiting.
    pub timeout_ms: u64,
    /// Learn from the user's corrections after a paste (learn.rs). `None`:
    /// not asked yet; off until the user says yes (docs/隱私.md).
    pub learn_from_edits: Option<bool>,
    /// Voice snippets: say the trigger, get the text (snippets.rs).
    pub snippets: Vec<super::snippets::Snippet>,
    /// Per-app styles (apps.rs).
    pub app_styles: Vec<super::apps::AppStyle>,
    /// Write every dictation in this language ("en", "ja"…); `None`: as spoken.
    pub translate_to: Option<String>,
    /// With text selected, what the user says edits it (polish.rs). Off by
    /// default: the selected text is then sent to the clean-up service too.
    pub edit_selection: bool,
}

impl Default for YuyinConfig {
    fn default() -> Self {
        Self {
            level: Level::Tidy,
            service: Service::Deepseek,
            vocab: DEFAULT_VOCAB.iter().map(|s| s.to_string()).collect(),
            base_url: "https://api.deepseek.com".into(),
            // M0: flash with reasoning off is 0.8 s median; v4-pro is slower and worse.
            model: "deepseek-flash".into(),
            timeout_ms: 5_000,
            learn_from_edits: None,
            snippets: Vec::new(),
            app_styles: Vec::new(),
            translate_to: None,
            edit_selection: false,
        }
    }
}

/// A new install's dictionary: names Taiwanese speakers commonly mix into
/// Chinese. The user edits it on the dictionary page.
const DEFAULT_VOCAB: &[&str] = &[
    "Claude",
    "Claude Code",
    "ChatGPT",
    "Gemini",
    "DeepSeek",
    "GitHub",
    "API",
    "API key",
    "repo",
    "prompt",
    "MVP",
    "PR",
    "Python",
    "JavaScript",
    "React",
    "Notion",
    "Slack",
    "Google Meet",
    "LINE",
    "YouTube",
    "Excel",
    "PowerPoint",
    "PDF",
];

static CONFIG: Lazy<RwLock<Option<YuyinConfig>>> = Lazy::new(|| RwLock::new(None));

fn config_path(app: &AppHandle) -> Option<PathBuf> {
    app.path()
        .app_data_dir()
        .ok()
        .map(|dir| dir.join(CONFIG_FILE))
}

/// Whether this is the first launch of Yuyin (no config written yet).
pub fn is_first_run(app: &AppHandle) -> bool {
    config_path(app).is_some_and(|p| !p.exists())
}

pub fn get(app: &AppHandle) -> YuyinConfig {
    if let Some(cfg) = CONFIG.read().ok().and_then(|c| c.clone()) {
        return cfg;
    }
    let cfg = config_path(app)
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|s| match serde_json::from_str::<YuyinConfig>(&s) {
            Ok(cfg) => Some(cfg),
            Err(e) => {
                warn!("yuyin.json is invalid, using defaults: {e}");
                None
            }
        })
        .unwrap_or_default();
    if let Ok(mut slot) = CONFIG.write() {
        *slot = Some(cfg.clone());
    }
    cfg
}

pub fn set(app: &AppHandle, cfg: YuyinConfig) -> Result<(), String> {
    let path = config_path(app).ok_or("no app data directory")?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let json = serde_json::to_string_pretty(&cfg).map_err(|e| e.to_string())?;
    std::fs::write(&path, json).map_err(|e| {
        error!("Failed to write yuyin.json: {e}");
        e.to_string()
    })?;
    if let Ok(mut slot) = CONFIG.write() {
        *slot = Some(cfg);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn partial_json_fills_defaults() {
        let cfg: YuyinConfig = serde_json::from_str(r#"{"level":"raw"}"#).unwrap();
        assert_eq!(cfg.level, Level::Raw);
        assert_eq!(cfg.model, "deepseek-flash");
        assert!(cfg.vocab.contains(&"Claude Code".to_string()));
        assert_eq!(cfg.service, Service::Deepseek);
    }

    #[test]
    fn default_level_is_tidy() {
        assert_eq!(YuyinConfig::default().level, Level::Tidy);
    }
}
