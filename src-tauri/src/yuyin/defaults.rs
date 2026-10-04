//! Yuyin's defaults for Handy's own settings, applied before shortcuts are
//! registered. Changing upstream's defaults in `settings.rs` would conflict
//! with every upstream settings change, so we write the user's store instead.
//!
//! - First launch: [`apply_first_run`].
//! - A later release that changes a default existing installs should get too
//!   bumps [`DEFAULTS_VERSION`] and adds a step to [`apply_upgrades`].

use log::info;
use tauri::AppHandle;

use super::config;
use crate::settings::{self, ModelUnloadTimeout, ShortcutActivation};

/// Hold right Option to talk (product definition: Fn or right Option on Mac;
/// Fn only works reliably on Apple keyboards).
#[cfg(not(target_os = "windows"))]
const TALK_KEY: &str = "option_right";
/// The same key on a PC keyboard: right Alt. handy-keys reads it from its
/// own key code, and on layouts where it is AltGr it drops the Left Ctrl
/// that Windows adds, so it works on Chinese (Taiwan), US and AltGr layouts.
#[cfg(target_os = "windows")]
const TALK_KEY: &str = "alt_right";

/// Qwen3-ASR 1.7B at Handy's default quant: the engine M0 chose. When the file
/// is already in the models dir (pre-installed), we select it and skip the
/// model step of onboarding, so first launch only asks for permissions
/// (criterion 10). Otherwise Handy's normal onboarding lets the user pick.
const PREINSTALLED_MODEL_FILE: &str = "Qwen3-ASR-1.7B-Q5_K_M.gguf";
const PREINSTALLED_MODEL_ID: &str = "handy-computer/Qwen3-ASR-1.7B-gguf/Qwen3-ASR-1.7B-Q5_K_M.gguf";

/// Bump when a release adds a step to [`apply_upgrades`].
const DEFAULTS_VERSION: u32 = 2;
const DEFAULTS_VERSION_FILE: &str = "yuyin_defaults_version";

/// Everything, in order. Called once per launch.
pub fn apply(app: &AppHandle) {
    apply_first_run(app);
    apply_upgrades(app);
}

fn apply_first_run(app: &AppHandle) {
    if !config::is_first_run(app) {
        return;
    }
    let mut s = settings::get_settings(app);
    // Traditional Chinese output; also sends `zh` to Qwen3-ASR, which would
    // otherwise translate English-heavy sentences into English (M0).
    s.selected_language = "zh-Hant".to_string();
    // Hold to talk, release to paste. Handy's default HoldOrToggle turns a
    // short tap into "keep recording", the opposite of criterion 3.
    s.shortcut_activation = ShortcutActivation::PushToTalk;
    if let Some(binding) = s.bindings.get_mut("transcribe") {
        binding.default_binding = TALK_KEY.to_string();
        binding.current_binding = TALK_KEY.to_string();
    }
    // Criterion 1: a start cue so the user waits for it before speaking.
    s.audio_feedback = true;
    // Criterion 2: reloading the 1.7B model on a press adds seconds.
    s.model_unload_timeout = ModelUnloadTimeout::Never;
    let preinstalled = crate::portable::app_data_dir(app)
        .map(|dir| dir.join("models").join(PREINSTALLED_MODEL_FILE).exists())
        .unwrap_or(false);
    if preinstalled {
        s.selected_model = PREINSTALLED_MODEL_ID.to_string();
        s.onboarding_completed = true;
    }
    settings::write_settings(app, s);

    // Writing yuyin.json marks the first run as done.
    if let Err(e) = config::set(app, config::get(app)) {
        log::error!("Failed to write initial yuyin.json: {e}");
    }
    info!("Applied Yuyin first-run defaults");
}

fn version_file(app: &AppHandle) -> Option<std::path::PathBuf> {
    crate::portable::app_data_dir(app)
        .ok()
        .map(|dir| dir.join(DEFAULTS_VERSION_FILE))
}

/// Defaults added after 0.1, applied once to installs that predate them
/// (and right after the first-run defaults on a new install).
fn apply_upgrades(app: &AppHandle) {
    let Some(path) = version_file(app) else {
        return;
    };
    let applied: u32 = std::fs::read_to_string(&path)
        .ok()
        .and_then(|s| s.trim().parse().ok())
        .unwrap_or(0);
    if applied >= DEFAULTS_VERSION {
        return;
    }
    let mut s = settings::get_settings(app);
    if applied < 1 {
        // Criterion 6: restore the clipboard only after the target app has
        // read the transcript, and mark it transient so clipboard managers
        // (Maccy, Paste, Windows clipboard history) skip it.
        s.reliable_paste = true;
    }
    if applied < 2 {
        // History is where a failed paste is recovered (criterion 4) and
        // where the user checks what was sent; Handy keeps only 5.
        s.history_limit = s.history_limit.max(100);
    }
    settings::write_settings(app, s);
    if let Err(e) = std::fs::write(&path, DEFAULTS_VERSION.to_string()) {
        log::error!("Failed to record Yuyin defaults version: {e}");
    }
    info!("Applied Yuyin defaults {applied} -> {DEFAULTS_VERSION}");
}
