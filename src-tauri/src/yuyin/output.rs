//! Output when pasting isn't safe: the user switched to another window
//! between pressing and releasing the key (acceptance criterion 7). We copy
//! the text and show a short notice instead of pasting into the wrong place.

use std::time::Duration;

use tauri::AppHandle;
use tauri_plugin_clipboard_manager::ClipboardExt;

use super::session::{self, PolishOutcome};
use crate::settings::PasteMethod;

const NOTICE_DURATION: Duration = Duration::from_millis(1800);
/// Long enough for the check mark to draw (0.4 s) and be seen.
const DONE_DURATION: Duration = Duration::from_millis(650);
/// Nothing left the computer: the pill also says "all on this Mac", which
/// needs time to be read.
const LOCAL_DONE_DURATION: Duration = Duration::from_millis(1400);
const FALLBACK_DURATION: Duration = Duration::from_millis(1600);

/// After a successful paste: a check mark, or a short notice that the raw
/// transcript was pasted because the clean-up failed. Then the pill leaves.
pub fn show_result(app: &AppHandle, outcome: Option<PolishOutcome>) {
    let (state, duration) = match outcome {
        Some(PolishOutcome::Failed) => ("fallback", FALLBACK_DURATION),
        Some(PolishOutcome::Skipped) => ("done", LOCAL_DONE_DURATION),
        _ => ("done", DONE_DURATION),
    };
    crate::overlay::show_result_overlay(app, state);
    hide_later(app, duration);
}

fn hide_later(app: &AppHandle, after: Duration) {
    let generation = session::generation();
    let app = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(after);
        // Don't hide the pill of a dictation that started in the meantime.
        if session::generation() == generation {
            crate::utils::hide_recording_overlay(&app);
        }
    });
}

/// Terminals that don't paste on Ctrl+V (acceptance criterion 8): mintty
/// (Git Bash, MSYS2) sends Ctrl+V to the shell and pastes on Shift+Insert;
/// Alacritty, WezTerm, Tabby and Hyper paste on Ctrl+Shift+V. Windows
/// Terminal and the classic console paste on Ctrl+V. On macOS every
/// terminal takes ⌘V, so nothing here applies.
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
fn terminal_paste(executable: &str) -> Option<PasteMethod> {
    match executable {
        "mintty.exe" => Some(PasteMethod::ShiftInsert),
        "alacritty.exe" | "wezterm-gui.exe" | "tabby.exe" | "hyper.exe" => {
            Some(PasteMethod::CtrlShiftV)
        }
        _ => None,
    }
}

/// The keys to paste with: the user's setting, except that the default
/// Ctrl+V becomes the foreground terminal's own paste keys.
pub fn paste_method(configured: PasteMethod) -> PasteMethod {
    if configured != PasteMethod::CtrlV {
        return configured;
    }
    #[cfg(target_os = "windows")]
    if let Some(method) = session::frontmost_id().and_then(|id| terminal_paste(&id)) {
        return method;
    }
    configured
}

pub fn copy_instead(app: &AppHandle, text: &str) -> Result<(), String> {
    app.clipboard()
        .write_text(text)
        .map_err(|e| format!("failed to copy to clipboard: {e}"))?;
    crate::overlay::show_result_overlay(app, "copied");
    hide_later(app, NOTICE_DURATION);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terminals_get_their_own_paste_keys() {
        assert_eq!(terminal_paste("mintty.exe"), Some(PasteMethod::ShiftInsert));
        assert_eq!(
            terminal_paste("wezterm-gui.exe"),
            Some(PasteMethod::CtrlShiftV)
        );
        // Windows Terminal and chat apps paste on Ctrl+V.
        assert_eq!(terminal_paste("windowsterminal.exe"), None);
        assert_eq!(terminal_paste("line.exe"), None);
    }

    #[test]
    fn a_chosen_paste_method_is_kept() {
        for method in [
            PasteMethod::Direct,
            PasteMethod::ShiftInsert,
            PasteMethod::None,
        ] {
            assert_eq!(paste_method(method), method);
        }
    }
}
