//! The main window on Windows: its size and its look.
//!
//! - Size: WebView2 zooms the page by Windows' "Text size" setting, so at
//!   111 % a 960 px window lays out only 865 CSS px, less than the 900 the
//!   design needs, and the home page's activity grid spilled out of its card.
//!   The window grows with the text size instead, keeping the page's room.
//! - Look: macOS gets its System Settings–style window in `lib.rs`; the
//!   Windows 11 counterpart is Mica behind a transparent web view. The page
//!   (App.css) keeps the content pane opaque and lets the sidebar show Mica.
//!   Windows 10 has no Mica and keeps an ordinary opaque window with a tinted
//!   sidebar.

use tauri::utils::config::WindowEffectsConfig;
use tauri::window::Effect;
use tauri::{AppHandle, Manager, Runtime, WebviewWindowBuilder};

/// The main window's designed size and minimum, in CSS pixels (as set in
/// `lib.rs` for every platform).
const SIZE: (f64, f64) = (960.0, 640.0);
const MIN_SIZE: (f64, f64) = (900.0, 600.0);

/// Windows 11 is build 22000 and later; Mica needs it.
const FIRST_MICA_BUILD: u32 = 22000;

fn windows_build() -> Option<u32> {
    let key = winreg::RegKey::predef(winreg::enums::HKEY_LOCAL_MACHINE)
        .open_subkey("SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion")
        .ok()?;
    let build: String = key.get_value("CurrentBuildNumber").ok()?;
    build.trim().parse().ok()
}

fn supports_mica(build: Option<u32>) -> bool {
    build.is_some_and(|b| b >= FIRST_MICA_BUILD)
}

/// Settings → Accessibility → Text size, 100–225 %, as a factor.
fn text_scale() -> f64 {
    winreg::RegKey::predef(winreg::enums::HKEY_CURRENT_USER)
        .open_subkey("Software\\Microsoft\\Accessibility")
        .and_then(|key| key.get_value::<u32, _>("TextScaleFactor"))
        .map(|percent| f64::from(percent) / 100.0)
        .unwrap_or(1.0)
        .clamp(1.0, 2.25)
}

/// Window size and minimum (logical pixels) that give the page its designed
/// room at `scale`, kept within `work_area` (the screen minus the taskbar).
fn window_sizes(scale: f64, work_area: Option<(f64, f64)>) -> ((f64, f64), (f64, f64)) {
    let (mut width, mut height) = (SIZE.0 * scale, SIZE.1 * scale);
    if let Some((area_width, area_height)) = work_area {
        // Leave room for the title bar and a margin around the window.
        width = width.min(area_width - 40.0).max(SIZE.0);
        height = height.min(area_height - 80.0).max(SIZE.1);
    }
    let min = (
        (MIN_SIZE.0 * scale).min(width),
        (MIN_SIZE.1 * scale).min(height),
    );
    ((width, height), min)
}

/// Size the main window for the text size, and give it Mica where Windows
/// supports it. The page learns about Mica from `window.__MOQI_MICA__`
/// (read in main.tsx), set before any script.
pub fn apply<'a, R: Runtime, M: Manager<R>>(
    app: &AppHandle<R>,
    builder: WebviewWindowBuilder<'a, R, M>,
) -> WebviewWindowBuilder<'a, R, M> {
    let work_area = app.primary_monitor().ok().flatten().map(|monitor| {
        let scale = monitor.scale_factor();
        let area = monitor.work_area();
        (
            f64::from(area.size.width) / scale,
            f64::from(area.size.height) / scale,
        )
    });
    let ((width, height), (min_width, min_height)) = window_sizes(text_scale(), work_area);
    let builder = builder
        .inner_size(width, height)
        .min_inner_size(min_width, min_height);

    if !supports_mica(windows_build()) {
        return builder;
    }
    builder
        .transparent(true)
        .effects(WindowEffectsConfig {
            // Plain Mica follows the window's light/dark theme.
            effects: vec![Effect::Mica],
            state: None,
            radius: None,
            color: None,
        })
        .initialization_script("window.__MOQI_MICA__ = true;")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mica_only_from_windows_11() {
        assert!(!supports_mica(Some(19045))); // Windows 10 22H2
        assert!(supports_mica(Some(22000)));
        assert!(supports_mica(Some(26200)));
        assert!(!supports_mica(None));
    }

    #[test]
    fn reads_this_machines_build() {
        assert!(windows_build().is_some_and(|b| b >= 10240));
    }

    #[test]
    fn window_grows_with_the_text_size() {
        let big_screen = Some((2560.0, 1392.0));
        assert_eq!(
            window_sizes(1.0, big_screen),
            ((960.0, 640.0), (900.0, 600.0))
        );
        let ((w, h), (mw, mh)) = window_sizes(1.11, big_screen);
        assert!((w - 1065.6).abs() < 0.01 && (h - 710.4).abs() < 0.01);
        assert!((mw - 999.0).abs() < 0.01 && (mh - 666.0).abs() < 0.01);
    }

    #[test]
    fn window_stays_on_a_small_screen() {
        // 1366x768 laptop at 150 % text: the window fills the work area
        // instead of running off it, and the minimum never exceeds it.
        let ((w, h), (mw, mh)) = window_sizes(1.5, Some((1366.0, 728.0)));
        assert_eq!((w, h), (1326.0, 648.0));
        assert!(mw <= w && mh <= h);
    }

    #[test]
    fn text_scale_is_a_sane_factor() {
        let scale = text_scale();
        assert!((1.0..=2.25).contains(&scale));
    }
}
