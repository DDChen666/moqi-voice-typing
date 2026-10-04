//! Where "paste last" goes on Windows.
//!
//! On macOS a menu bar menu leaves the frontmost app in front, so the tray's
//! "貼上上一次" pastes where the user was typing. On Windows, opening the
//! notification-area menu (often from the "^" overflow panel) makes the
//! shell and then Moqi the foreground, and nothing gives focus back when the
//! menu closes: the paste went nowhere. So we follow foreground changes and
//! remember the last ordinary window the user was in, skipping the taskbar,
//! the tray panels, the desktop and Moqi's own windows, and hand focus back
//! to it before pasting (acceptance criterion 4).

use std::sync::atomic::{AtomicIsize, Ordering};
use std::time::Duration;

use log::warn;
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::Accessibility::{SetWinEventHook, HWINEVENTHOOK};
use windows::Win32::UI::WindowsAndMessaging::{
    GetAncestor, GetClassNameW, GetWindowThreadProcessId, IsWindow, IsWindowVisible,
    SetForegroundWindow, EVENT_SYSTEM_FOREGROUND, GA_ROOT, WINEVENT_OUTOFCONTEXT,
};

static LAST_WINDOW: AtomicIsize = AtomicIsize::new(0);

/// Shell surfaces that take the foreground on the way to the tray menu.
const SHELL_CLASSES: &[&str] = &[
    "Shell_TrayWnd",
    "Shell_SecondaryTrayWnd",
    "NotifyIconOverflowWindow",
    "TopLevelWindowForOverflowXamlIsland",
    "XamlExplorerHostIslandWindow",
    "Progman",
    "WorkerW",
];

fn class_name(hwnd: HWND) -> String {
    let mut buf = [0u16; 128];
    // SAFETY: the buffer outlives the call; a stale hwnd returns 0.
    let len = unsafe { GetClassNameW(hwnd, &mut buf) };
    String::from_utf16_lossy(&buf[..len.max(0) as usize])
}

fn is_target(hwnd: HWND) -> bool {
    if hwnd.is_invalid() {
        return false;
    }
    let mut pid = 0u32;
    // SAFETY: plain query; a stale hwnd leaves pid at 0.
    unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
    pid != 0 && pid != std::process::id() && !SHELL_CLASSES.contains(&class_name(hwnd).as_str())
}

unsafe extern "system" fn on_foreground(
    _hook: HWINEVENTHOOK,
    _event: u32,
    hwnd: HWND,
    _object: i32,
    _child: i32,
    _thread: u32,
    _time: u32,
) {
    // SAFETY: GetAncestor tolerates any handle value.
    let root = unsafe { GetAncestor(hwnd, GA_ROOT) };
    let window = if root.is_invalid() { hwnd } else { root };
    if is_target(window) {
        LAST_WINDOW.store(window.0 as isize, Ordering::SeqCst);
    }
}

/// Start following foreground changes. Must run on a thread with a message
/// loop (Tauri's main thread); the hook lives as long as the app.
pub fn install() {
    // SAFETY: an out-of-context hook with a plain function callback; the
    // callback only reads window properties and stores an integer.
    let hook = unsafe {
        SetWinEventHook(
            EVENT_SYSTEM_FOREGROUND,
            EVENT_SYSTEM_FOREGROUND,
            None,
            Some(on_foreground),
            0,
            0,
            WINEVENT_OUTOFCONTEXT,
        )
    };
    if hook.is_invalid() {
        warn!("could not follow foreground changes; paste-last pastes into Moqi's focus");
    }
}

/// Give the foreground back to the window the user was last in, and wait a
/// moment for it to take focus. False when that window is gone.
pub fn restore() -> bool {
    let hwnd = HWND(LAST_WINDOW.load(Ordering::SeqCst) as *mut _);
    // SAFETY: IsWindow/IsWindowVisible accept any handle value; our process
    // owns the foreground right after the menu click, so it may hand it on.
    let restored = unsafe {
        IsWindow(Some(hwnd)).as_bool()
            && IsWindowVisible(hwnd).as_bool()
            && SetForegroundWindow(hwnd).as_bool()
    };
    if restored {
        std::thread::sleep(Duration::from_millis(120));
    }
    restored
}
