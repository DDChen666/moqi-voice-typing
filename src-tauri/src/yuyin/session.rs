//! State of the current dictation, from key press to paste.
//!
//! - On press we remember the frontmost app, its focused window and the
//!   writing context, so the context is judged where the user started typing.
//! - Before pasting we check that the same window still has focus
//!   (acceptance criterion 7).
//! - Each stage is timed and appended to `yuyin_timings.jsonl` in the app data
//!   directory: numbers, the context label, the app's display name and where
//!   text was sent — never what the user said. It is the evidence for
//!   acceptance criteria 1 and 2, and feeds the home page and history
//!   ([`super::stats`]). It never leaves the machine.

use std::io::Write;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use log::{debug, warn};
use once_cell::sync::Lazy;
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

use super::config::Level;
use super::context::{classify, display_name, Context, FrontApp};

const TIMINGS_FILE: &str = "yuyin_timings.jsonl";
/// Held while appending to or rewriting the timings log, so deleting a
/// history entry can't drop a record written at the same moment.
pub static TIMINGS_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Presses shorter than this are accidental taps and produce no text
/// (acceptance criterion 3).
const MIN_HOLD: Duration = Duration::from_millis(300);

/// What happened to the LLM clean-up step.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PolishOutcome {
    /// Raw level, or nothing to clean up.
    Skipped,
    Ok,
    /// Timed out, network or API error, missing key, or the output failed the
    /// sanity check. The raw transcript was used instead.
    Failed,
}

struct Session {
    pressed: Instant,
    front: Option<FrontApp>,
    context: Context,
    window: Option<platform::Window>,
    mic_ready: Option<Instant>,
    released: Option<Instant>,
    transcribed: Option<Instant>,
    polished: Option<Instant>,
    polish: PolishOutcome,
    level: Option<Level>,
    chars_in: usize,
    chars_out: usize,
    focus_changed: bool,
    audio_samples: Option<usize>,
    chunks: super::chunker::Stats,
    /// Shown in history ("LINE"); stays on this machine.
    app_name: String,
    /// Characters of transcript sent to the clean-up service, and its host.
    sent_chars: usize,
    sent_to: Option<String>,
    /// The history entry's recording, to join history with this record.
    file_name: Option<String>,
    /// This app's extra instruction and output language (apps.rs).
    note: String,
    translate_to: Option<String>,
    /// Text the user had selected when they pressed the key, to edit by
    /// voice (only with `YuyinConfig::edit_selection` on).
    selection: Option<String>,
}

/// What the clean-up needs beyond the transcript and the style.
#[derive(Clone, Debug, Default)]
pub struct Extras {
    pub note: String,
    pub translate_to: Option<String>,
    pub selection: Option<String>,
}

static CURRENT: Lazy<Mutex<Option<Session>>> = Lazy::new(|| Mutex::new(None));

/// Bumped on every key press, so delayed UI work (like hiding the "copied"
/// notice) can tell whether a new dictation has started since.
static GENERATION: AtomicU64 = AtomicU64::new(0);

pub fn generation() -> u64 {
    GENERATION.load(Ordering::SeqCst)
}

fn with_session(f: impl FnOnce(&mut Session)) {
    if let Ok(mut guard) = CURRENT.lock() {
        if let Some(session) = guard.as_mut() {
            f(session);
        }
    }
}

/// Set by the coordinator just before a recording starts: the key no longer
/// ends it (double-tap hands-free); the next press does.
static HANDS_FREE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub fn set_hands_free(on: bool) {
    HANDS_FREE.store(on, Ordering::SeqCst);
}

/// Shown in the capsule on key press.
#[derive(Clone, Serialize)]
struct ContextEvent {
    context: Context,
    app: String,
    hands_free: bool,
    /// Where this dictation's text will go ("DeepSeek"), or none: all local.
    sends_to: Option<String>,
    /// What the user says will edit the text they selected.
    editing: bool,
    /// The text will be written in another language.
    translating: bool,
}

/// Key press (at `pressed`): remember where the user is typing.
pub fn begin(app: &AppHandle, pressed: Instant) {
    GENERATION.fetch_add(1, Ordering::SeqCst);
    let (front, window) = match platform::frontmost() {
        Some((app, window)) => (Some(app), window),
        None => (None, None),
    };
    let auto = front.as_ref().map(classify).unwrap_or(Context::Other);
    let app_name = front.as_ref().map(display_name).unwrap_or_default();
    // The user's choices for this app (apps.rs) override the automatic style.
    let cfg = super::config::get(app);
    let style = front
        .as_ref()
        .and_then(|f| super::apps::style_for(&cfg, &super::context::style_key(f)));
    let context = style.and_then(|s| s.context).unwrap_or(auto);
    let note = style.map(|s| s.note.clone()).unwrap_or_default();
    let translate_to = super::apps::translate_to(&cfg, style);
    if let Some(f) = &front {
        super::apps::note_used(app, f, &app_name, auto);
    }
    let extras = Extras {
        note: note.clone(),
        translate_to: translate_to.clone(),
        selection: None,
    };
    let event = ContextEvent {
        context,
        app: app_name.clone(),
        hands_free: HANDS_FREE.load(Ordering::SeqCst),
        sends_to: super::polish::destination(&super::polish::effective(&cfg, &extras)),
        editing: false,
        translating: translate_to.is_some(),
    };
    let _ = app.emit_to("recording_overlay", "yuyin-context", event.clone());
    debug!(
        "yuyin session: context={:?} app={:?} window_known={}",
        context,
        front.as_ref().map(|a| a.bundle_id.as_str()),
        window.is_some()
    );
    if let Ok(mut guard) = CURRENT.lock() {
        *guard = Some(Session {
            pressed,
            front,
            context,
            window,
            mic_ready: None,
            released: None,
            transcribed: None,
            polished: None,
            polish: PolishOutcome::Skipped,
            level: None,
            chars_in: 0,
            chars_out: 0,
            focus_changed: false,
            audio_samples: None,
            chunks: Default::default(),
            app_name,
            sent_chars: 0,
            sent_to: None,
            file_name: None,
            note,
            translate_to,
            selection: None,
        });
    }
    read_selection(app, &cfg, event);
}

/// With "edit the selection by voice" on, read the selected text of the app
/// the user is in, off the key-press path; the capsule then says it will
/// edit the selection. Not read when no clean-up service can do the edit.
fn read_selection(app: &AppHandle, cfg: &super::config::YuyinConfig, event: ContextEvent) {
    if !cfg.edit_selection {
        return;
    }
    let editing = Extras {
        selection: Some(String::new()),
        ..extras()
    };
    if !super::polish::will_run(&super::polish::effective(cfg, &editing)) {
        debug!("edit by voice: no clean-up service to do it");
        return;
    }
    let Some(pid) = front_app().map(|f| f.pid) else {
        return;
    };
    let generation = generation();
    let app = app.clone();
    std::thread::spawn(move || {
        let Some(text) = platform::selected_text(pid).filter(|t| !t.trim().is_empty()) else {
            debug!("edit by voice: nothing selected");
            return;
        };
        debug!(
            "edit by voice: {} characters selected",
            text.chars().count()
        );
        if self::generation() != generation {
            return;
        }
        with_session(|s| s.selection = Some(text));
        let _ = app.emit_to(
            "recording_overlay",
            "yuyin-context",
            ContextEvent {
                editing: true,
                ..event
            },
        );
    });
}

/// This dictation's extra instruction, output language and selection.
pub fn extras() -> Extras {
    CURRENT
        .lock()
        .ok()
        .and_then(|g| {
            g.as_ref().map(|s| Extras {
                note: s.note.clone(),
                translate_to: s.translate_to.clone(),
                selection: s.selection.clone(),
            })
        })
        .unwrap_or_default()
}

/// The app the user started dictating in, for work that happens after the
/// session ends (the field probe).
pub fn front_app() -> Option<FrontApp> {
    CURRENT
        .lock()
        .ok()
        .and_then(|g| g.as_ref().and_then(|s| s.front.clone()))
}

/// The focused UI element of `pid`: its role and, when it holds plain text,
/// that text. None without Accessibility or when nothing is focused.
pub fn focused_field(pid: i32) -> Option<(String, Option<String>)> {
    platform::focused_field(pid)
}

/// The executable in the foreground right now, for per-app paste keys.
#[cfg(target_os = "windows")]
pub fn frontmost_id() -> Option<String> {
    platform::frontmost_id()
}

/// Top-left of the screen the user is typing on (physical pixels), so the
/// capsule opens there (acceptance criterion 9).
#[cfg(target_os = "windows")]
pub fn typing_monitor_origin() -> Option<(i32, i32)> {
    platform::foreground_monitor_origin()
}

pub fn context() -> Context {
    CURRENT
        .lock()
        .ok()
        .and_then(|g| g.as_ref().map(|s| s.context))
        .unwrap_or(Context::Other)
}

pub fn mark_mic_ready() {
    with_session(|s| s.mic_ready = s.mic_ready.or(Some(Instant::now())));
}

pub fn mark_released() {
    with_session(|s| s.released = Some(Instant::now()));
}

/// Whether the key was released within [`MIN_HOLD`] of being pressed.
pub fn was_tap() -> bool {
    CURRENT
        .lock()
        .ok()
        .and_then(|g| g.as_ref().and_then(|s| s.released.map(|r| r - s.pressed)))
        .is_some_and(|held| held < MIN_HOLD)
}

pub fn mark_transcribed(chars: usize) {
    with_session(|s| {
        s.transcribed = Some(Instant::now());
        s.chars_in = chars;
    });
}

/// How much audio there was and how much was left after release (step 2b).
pub fn mark_chunks(audio_samples: usize, stats: super::chunker::Stats) {
    with_session(|s| {
        s.audio_samples = Some(audio_samples);
        s.chunks = stats;
    });
}

/// Just before the transcript goes to the clean-up service.
pub fn mark_sent(chars: usize, host: String) {
    with_session(|s| {
        s.sent_chars = chars;
        s.sent_to = Some(host);
    });
}

/// The history entry for this dictation was saved.
pub fn mark_saved(file_name: &str) {
    with_session(|s| s.file_name = Some(file_name.to_string()));
}

pub fn mark_polished(level: Level, outcome: PolishOutcome, chars_out: usize) {
    with_session(|s| {
        s.polished = Some(Instant::now());
        s.level = Some(level);
        s.polish = outcome;
        s.chars_out = chars_out;
    });
}

/// Before paste: has the user moved to another app or window since pressing
/// the key? When we can't tell (no Accessibility, no session), assume not, so
/// a missing permission never blocks pasting.
pub fn focus_changed() -> bool {
    let Ok(mut guard) = CURRENT.lock() else {
        return false;
    };
    let Some(session) = guard.as_mut() else {
        return false;
    };
    let Some(before) = session.front.as_ref() else {
        return false;
    };
    let changed = match platform::frontmost() {
        None => false,
        Some((now, _)) if now.pid != before.pid => true,
        Some((_, window_now)) => match (&session.window, window_now) {
            (Some(a), Some(b)) => !a.same_as(&b),
            _ => false,
        },
    };
    session.focus_changed = changed;
    changed
}

#[derive(Serialize)]
struct TimingRecord {
    /// Unix time in milliseconds.
    at: u128,
    context: Context,
    level: Option<Level>,
    polish: PolishOutcome,
    /// Criterion 1: key press → first microphone samples.
    press_to_mic_ms: Option<u128>,
    /// How long the user held the key (≈ audio length).
    press_to_release_ms: Option<u128>,
    release_to_transcribed_ms: Option<u128>,
    transcribed_to_polished_ms: Option<u128>,
    /// Criterion 2: key release → text pasted (or copied).
    release_to_output_ms: Option<u128>,
    focus_changed: bool,
    chars_in: usize,
    chars_out: usize,
    /// Audio the recorder kept (speech plus VAD padding).
    audio_ms: Option<usize>,
    /// Pieces transcribed while the user was still speaking.
    pieces: usize,
    /// Audio left to transcribe after release.
    tail_ms: Option<usize>,
    app: String,
    sent_chars: usize,
    sent_to: Option<String>,
    file_name: Option<String>,
}

fn between(a: Option<Instant>, b: Option<Instant>) -> Option<u128> {
    match (a, b) {
        (Some(a), Some(b)) if b >= a => Some((b - a).as_millis()),
        _ => None,
    }
}

pub fn timings_path(app: &AppHandle) -> Option<std::path::PathBuf> {
    app.path()
        .app_data_dir()
        .ok()
        .map(|dir| dir.join(TIMINGS_FILE))
}

fn samples_to_ms(samples: usize) -> usize {
    samples / 16
}

/// Text was pasted (or copied): write the timing record, clear the session
/// and return what happened to the clean-up step.
pub fn finish(app: &AppHandle) -> Option<PolishOutcome> {
    let output = Instant::now();
    let s = CURRENT.lock().ok().and_then(|mut g| g.take())?;
    let outcome = s.polish;
    let record = TimingRecord {
        at: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0),
        context: s.context,
        level: s.level,
        polish: s.polish,
        press_to_mic_ms: between(Some(s.pressed), s.mic_ready),
        press_to_release_ms: between(Some(s.pressed), s.released),
        release_to_transcribed_ms: between(s.released, s.transcribed),
        transcribed_to_polished_ms: between(s.transcribed, s.polished),
        release_to_output_ms: between(s.released, Some(output)),
        focus_changed: s.focus_changed,
        chars_in: s.chars_in,
        chars_out: s.chars_out,
        audio_ms: s.audio_samples.map(samples_to_ms),
        pieces: s.chunks.pieces,
        tail_ms: s
            .audio_samples
            .map(|_| samples_to_ms(s.chunks.tail_samples)),
        app: s.app_name,
        sent_chars: s.sent_chars,
        sent_to: s.sent_to,
        file_name: s.file_name,
    };
    let Some(path) = timings_path(app) else {
        return Some(outcome);
    };
    let Ok(line) = serde_json::to_string(&record) else {
        return Some(outcome);
    };
    let _guard = TIMINGS_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let result = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .and_then(|mut f| writeln!(f, "{line}"));
    if let Err(e) = result {
        warn!("Failed to write yuyin timing record: {e}");
    }
    Some(outcome)
}

#[cfg(target_os = "macos")]
mod platform {
    //! Frontmost app via NSWorkspace; focused window and its title via the
    //! Accessibility API (the same permission paste already needs).

    use std::ffi::c_void;

    use objc2_app_kit::NSWorkspace;

    use super::FrontApp;

    type CFTypeRef = *const c_void;
    const UTF8: u32 = 0x0800_0100;

    #[link(name = "ApplicationServices", kind = "framework")]
    extern "C" {
        fn AXUIElementCreateApplication(pid: i32) -> CFTypeRef;
        fn AXUIElementCopyAttributeValue(
            element: CFTypeRef,
            attribute: CFTypeRef,
            value: *mut CFTypeRef,
        ) -> i32;
        fn AXUIElementSetMessagingTimeout(element: CFTypeRef, timeout: f32) -> i32;
    }

    #[link(name = "CoreFoundation", kind = "framework")]
    extern "C" {
        fn CFRelease(cf: CFTypeRef);
        fn CFEqual(a: CFTypeRef, b: CFTypeRef) -> u8;
        fn CFGetTypeID(cf: CFTypeRef) -> usize;
        fn CFStringGetTypeID() -> usize;
        fn CFStringCreateWithBytes(
            alloc: CFTypeRef,
            bytes: *const u8,
            len: isize,
            encoding: u32,
            external: u8,
        ) -> CFTypeRef;
        fn CFStringGetLength(s: CFTypeRef) -> isize;
        fn CFStringGetMaximumSizeForEncoding(len: isize, encoding: u32) -> isize;
        fn CFStringGetCString(s: CFTypeRef, buf: *mut u8, size: isize, encoding: u32) -> u8;
    }

    /// An owned Core Foundation reference, released on drop.
    struct Owned(CFTypeRef);

    impl Drop for Owned {
        fn drop(&mut self) {
            // CFRelease(NULL) aborts the process; never hand it one.
            if !self.0.is_null() {
                // SAFETY: we only wrap references we own (Create/Copy rule).
                unsafe { CFRelease(self.0) }
            }
        }
    }

    // SAFETY: AXUIElement and CFString are immutable CF objects; retaining,
    // comparing and releasing them from any thread is allowed.
    unsafe impl Send for Owned {}

    fn cf_string(s: &str) -> Option<Owned> {
        // SAFETY: bytes/len describe a valid UTF-8 buffer for the call's duration.
        let r = unsafe {
            CFStringCreateWithBytes(std::ptr::null(), s.as_ptr(), s.len() as isize, UTF8, 0)
        };
        // Not `then_some(Owned(r))`: that builds (and drops) the Owned even
        // when the check fails.
        if r.is_null() {
            None
        } else {
            Some(Owned(r))
        }
    }

    fn to_string(cf: &Owned) -> Option<String> {
        // SAFETY: cf is a live CF object; we check it is a CFString first.
        unsafe {
            if CFGetTypeID(cf.0) != CFStringGetTypeID() {
                return None;
            }
            let len = CFStringGetLength(cf.0);
            let size = CFStringGetMaximumSizeForEncoding(len, UTF8) + 1;
            let mut buf = vec![0u8; size.max(1) as usize];
            if CFStringGetCString(cf.0, buf.as_mut_ptr(), size, UTF8) == 0 {
                return None;
            }
            let end = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
            String::from_utf8(buf[..end].to_vec()).ok()
        }
    }

    fn copy_attribute(element: &Owned, name: &str) -> Option<Owned> {
        let attribute = cf_string(name)?;
        let mut value: CFTypeRef = std::ptr::null();
        // SAFETY: element and attribute are live; value receives a +1 reference.
        let err = unsafe { AXUIElementCopyAttributeValue(element.0, attribute.0, &mut value) };
        // A failed copy leaves `value` NULL. Build the Owned only on success:
        // `then_some(Owned(value))` built it anyway and released NULL, which
        // crashed the app once the field probe read attributes that many
        // elements don't have (2026-09-29).
        if err == 0 && !value.is_null() {
            Some(Owned(value))
        } else {
            None
        }
    }

    /// Test hook: one attribute of an app element, as `copy_attribute` sees it.
    #[cfg(test)]
    pub fn attribute_of_app(pid: i32, name: &str) -> Option<()> {
        // SAFETY: AXUIElementCreateApplication returns a +1 reference.
        let app = unsafe { AXUIElementCreateApplication(pid) };
        if app.is_null() {
            return None;
        }
        let app = Owned(app);
        copy_attribute(&app, name).map(|_| ())
    }

    /// The focused window of an app, kept alive so it can be compared later.
    pub struct Window(Owned);

    impl Window {
        pub fn same_as(&self, other: &Window) -> bool {
            // SAFETY: both are live AXUIElements.
            unsafe { CFEqual(self.0 .0, other.0 .0) != 0 }
        }
    }

    fn focused_window(pid: i32) -> Option<Window> {
        // SAFETY: AXUIElementCreateApplication returns a +1 reference.
        let app = unsafe { AXUIElementCreateApplication(pid) };
        if app.is_null() {
            return None;
        }
        let app = Owned(app);
        copy_attribute(&app, "AXFocusedWindow").map(Window)
    }

    fn window_title(window: &Window) -> String {
        copy_attribute(&window.0, "AXTitle")
            .and_then(|t| to_string(&t))
            .unwrap_or_default()
    }

    /// Text longer than this (a terminal's whole scrollback) is not read.
    const MAX_FIELD_CHARS: isize = 200_000;

    /// The focused element of `pid` and its role.
    fn focused_element(pid: i32) -> Option<(Owned, String)> {
        // SAFETY: AXUIElementCreateApplication returns a +1 reference.
        let app = unsafe { AXUIElementCreateApplication(pid) };
        if app.is_null() {
            return None;
        }
        let app = Owned(app);
        // A busy app must not stall us: answer within half a second or give up.
        // SAFETY: app is a live AXUIElement.
        unsafe { AXUIElementSetMessagingTimeout(app.0, 0.5) };
        let element = copy_attribute(&app, "AXFocusedUIElement")?;
        // SAFETY: element is a live AXUIElement.
        unsafe { AXUIElementSetMessagingTimeout(element.0, 0.5) };
        let role = copy_attribute(&element, "AXRole")
            .and_then(|r| to_string(&r))
            .unwrap_or_default();
        Some((element, role))
    }

    /// The text selected in `pid`'s focused element, to edit by voice. None
    /// for password fields, nothing selected, or an app that doesn't expose
    /// its selection to Accessibility.
    pub fn selected_text(pid: i32) -> Option<String> {
        let (element, role) = focused_element(pid)?;
        if role == "AXSecureTextField" {
            return None;
        }
        let selected = copy_attribute(&element, "AXSelectedText").filter(|v| {
            // SAFETY: v is a live CF object; the length is only read for strings.
            unsafe {
                CFGetTypeID(v.0) == CFStringGetTypeID() && CFStringGetLength(v.0) <= MAX_FIELD_CHARS
            }
        })?;
        let found = to_string(&selected).filter(|t| !t.is_empty());
        log::debug!(
            "selected text: {}",
            found.as_ref().map_or("none".to_string(), |t| format!(
                "{} chars",
                t.chars().count()
            ))
        );
        found
    }

    pub fn focused_field(pid: i32) -> Option<(String, Option<String>)> {
        let (element, role) = focused_element(pid)?;
        if role == "AXSecureTextField" {
            return Some((role, None));
        }
        let value = copy_attribute(&element, "AXValue").filter(|v| {
            // SAFETY: v is a live CF object; the length is only read for strings.
            unsafe {
                CFGetTypeID(v.0) == CFStringGetTypeID() && CFStringGetLength(v.0) <= MAX_FIELD_CHARS
            }
        });
        Some((role, value.and_then(|v| to_string(&v))))
    }

    /// The frontmost app and its focused window (None without Accessibility).
    pub fn frontmost() -> Option<(FrontApp, Option<Window>)> {
        let workspace = NSWorkspace::sharedWorkspace();
        let app = workspace.frontmostApplication()?;
        let pid = app.processIdentifier();
        let bundle_id = app
            .bundleIdentifier()
            .map(|s| s.to_string())
            .unwrap_or_default();
        let window = focused_window(pid);
        let window_title = window.as_ref().map(window_title).unwrap_or_default();
        let name = app
            .localizedName()
            .map(|s| s.to_string())
            .unwrap_or_default();
        Some((
            FrontApp {
                bundle_id,
                pid,
                window_title,
                name,
            },
            window,
        ))
    }
}

#[cfg(all(test, target_os = "macos"))]
mod platform_tests {
    use super::platform::*;

    /// A failed attribute copy leaves its out-pointer NULL; releasing that
    /// crashed the app (2026-09-29, CFRelease on a background thread).
    #[test]
    fn missing_attributes_are_none_not_a_crash() {
        let pid = std::process::id() as i32;
        for _ in 0..50 {
            assert!(attribute_of_app(pid, "AXThisAttributeDoesNotExist").is_none());
        }
        // Our own test process has no focused text field; this must simply
        // come back empty, however often it is asked.
        for _ in 0..50 {
            let _ = focused_field(pid);
            assert!(selected_text(pid).is_none());
        }
    }
}

#[cfg(target_os = "windows")]
mod platform {
    //! Foreground window through user32; Windows needs no permission for it.
    //! The app is the window's executable, lower-cased (`line.exe`) to match
    //! the lists in `context`; its display name is the executable's version
    //! description ("LINE"), else the file name without `.exe`.

    use std::collections::HashMap;
    use std::ffi::c_void;
    use std::path::Path;
    use std::sync::Mutex;

    use once_cell::sync::Lazy;
    use windows::core::{BOOL, PCWSTR, PWSTR};
    use windows::Win32::Foundation::{CloseHandle, HWND, LPARAM};
    use windows::Win32::Storage::FileSystem::{
        GetFileVersionInfoSizeW, GetFileVersionInfoW, VerQueryValueW,
    };
    use windows::Win32::System::Threading::{
        OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32,
        PROCESS_QUERY_LIMITED_INFORMATION,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        EnumChildWindows, GetForegroundWindow, GetWindowTextW, GetWindowThreadProcessId,
    };

    use super::FrontApp;

    /// The foreground top-level window, kept as its handle value: an HWND is
    /// a plain identifier, safe to hold and compare from any thread.
    pub struct Window(isize);

    impl Window {
        pub fn same_as(&self, other: &Window) -> bool {
            self.0 == other.0
        }
    }

    fn pid_of(hwnd: HWND) -> u32 {
        let mut pid = 0u32;
        // SAFETY: a stale hwnd makes the call fail and leaves pid at 0.
        unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
        pid
    }

    /// Full path of a process's executable. Limited-information access also
    /// works for elevated processes (an admin terminal).
    fn image_path(pid: u32) -> Option<String> {
        // SAFETY: the handle is closed before returning; the buffer outlives
        // the query and `len` is its capacity in UTF-16 units.
        unsafe {
            let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
            let mut buf = [0u16; 1024];
            let mut len = buf.len() as u32;
            let result = QueryFullProcessImageNameW(
                process,
                PROCESS_NAME_WIN32,
                PWSTR(buf.as_mut_ptr()),
                &mut len,
            );
            let _ = CloseHandle(process);
            result.ok()?;
            Some(String::from_utf16_lossy(&buf[..len as usize]))
        }
    }

    fn window_title(hwnd: HWND) -> String {
        let mut buf = [0u16; 512];
        // SAFETY: the buffer outlives the call; a stale hwnd returns 0.
        let len = unsafe { GetWindowTextW(hwnd, &mut buf) };
        String::from_utf16_lossy(&buf[..len.max(0) as usize])
    }

    /// Store apps draw inside ApplicationFrameHost's frame, while a child
    /// window belongs to the app's own process. Report that process, so the
    /// Store version of an app is recognised like the desktop one.
    fn hosted_app_pid(frame: HWND, host_pid: u32) -> Option<u32> {
        struct Search {
            host: u32,
            found: u32,
        }
        unsafe extern "system" fn visit(child: HWND, lparam: LPARAM) -> BOOL {
            // SAFETY: lparam points at the `Search` below, alive for the
            // whole enumeration.
            let search = unsafe { &mut *(lparam.0 as *mut Search) };
            let pid = pid_of(child);
            if pid != 0 && pid != search.host {
                search.found = pid;
                return BOOL(0); // stop
            }
            BOOL(1)
        }
        let mut search = Search {
            host: host_pid,
            found: 0,
        };
        // SAFETY: the callback only uses `search`, which outlives the call.
        unsafe {
            let _ = EnumChildWindows(
                Some(frame),
                Some(visit),
                LPARAM(&mut search as *mut Search as isize),
            );
        }
        (search.found != 0).then_some(search.found)
    }

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    /// The FileDescription of an executable's version resource ("LINE",
    /// "Visual Studio Code"), in the first language the file lists.
    pub(super) fn file_description(path: &str) -> Option<String> {
        let path_w = wide(path);
        // SAFETY: `data` is sized by the size query and outlives every
        // VerQueryValueW call, whose out-pointers point into it with `len`
        // counted in UTF-16 units (strings) or bytes (the translation table).
        unsafe {
            let size = GetFileVersionInfoSizeW(PCWSTR(path_w.as_ptr()), None);
            if size == 0 {
                return None;
            }
            let mut data = vec![0u8; size as usize];
            GetFileVersionInfoW(
                PCWSTR(path_w.as_ptr()),
                None,
                size,
                data.as_mut_ptr() as *mut c_void,
            )
            .ok()?;
            let block = data.as_ptr() as *const c_void;
            let mut ptr: *mut c_void = std::ptr::null_mut();
            let mut len = 0u32;
            let mut languages = Vec::new();
            let translation = wide("\\VarFileInfo\\Translation");
            if VerQueryValueW(block, PCWSTR(translation.as_ptr()), &mut ptr, &mut len).as_bool()
                && !ptr.is_null()
            {
                let pairs = std::slice::from_raw_parts(ptr as *const u16, (len / 2) as usize);
                for pair in pairs.chunks_exact(2) {
                    languages.push(format!("{:04x}{:04x}", pair[0], pair[1]));
                }
            }
            // Common tables for files without a translation list.
            languages.extend(["040904b0", "040904e4", "000004b0"].map(String::from));
            for language in languages {
                let key = wide(&format!("\\StringFileInfo\\{language}\\FileDescription"));
                if VerQueryValueW(block, PCWSTR(key.as_ptr()), &mut ptr, &mut len).as_bool()
                    && !ptr.is_null()
                    && len > 0
                {
                    let chars = std::slice::from_raw_parts(ptr as *const u16, len as usize);
                    let end = chars.iter().position(|&c| c == 0).unwrap_or(chars.len());
                    let text = String::from_utf16_lossy(&chars[..end]).trim().to_string();
                    if !text.is_empty() {
                        return Some(text);
                    }
                }
            }
            None
        }
    }

    static NAMES: Lazy<Mutex<HashMap<String, String>>> = Lazy::new(Default::default);

    /// Shown in the capsule and history; cached per executable.
    pub(super) fn display_name(path: &str) -> String {
        if let Some(name) = NAMES.lock().ok().and_then(|m| m.get(path).cloned()) {
            return name;
        }
        // Some apps describe themselves by file name (the Store Notepad says
        // "Notepad.exe"); show "Notepad" like the fallback would.
        let described = file_description(path).map(|d| {
            // `get` is None off a character boundary ("Windows 檔案總管").
            let cut = d.len().saturating_sub(4);
            match d.get(cut..) {
                Some(tail) if tail.eq_ignore_ascii_case(".exe") => d[..cut].to_string(),
                _ => d,
            }
        });
        let name = described.filter(|d| !d.is_empty()).unwrap_or_else(|| {
            Path::new(path)
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default()
        });
        if let Ok(mut names) = NAMES.lock() {
            names.insert(path.to_string(), name.clone());
        }
        name
    }

    fn file_name(path: &str) -> String {
        Path::new(path)
            .file_name()
            .map(|s| s.to_string_lossy().to_lowercase())
            .unwrap_or_default()
    }

    /// The foreground window's process and executable path.
    fn foreground() -> Option<(HWND, u32, String)> {
        // SAFETY: no arguments; returns NULL while no window is foreground.
        let hwnd = unsafe { GetForegroundWindow() };
        if hwnd.is_invalid() {
            return None;
        }
        let pid = pid_of(hwnd);
        if pid == 0 {
            return None;
        }
        let path = image_path(pid)?;
        if file_name(&path) == "applicationframehost.exe" {
            if let Some(app_pid) = hosted_app_pid(hwnd, pid) {
                if let Some(app_path) = image_path(app_pid) {
                    return Some((hwnd, app_pid, app_path));
                }
            }
        }
        Some((hwnd, pid, path))
    }

    /// Text longer than this (a terminal's whole scrollback) is not read.
    const MAX_FIELD_CHARS: i32 = 200_000;

    thread_local! {
        /// One UI Automation client per watching thread (COM objects stay on
        /// the thread that made them).
        static UIA: Option<windows::Win32::UI::Accessibility::IUIAutomation> = {
            use windows::Win32::System::Com::{
                CoCreateInstance, CoInitializeEx, CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED,
            };
            use windows::Win32::UI::Accessibility::CUIAutomation;
            // SAFETY: joining the MTA is per thread and may repeat; the client
            // is created and used only on this thread.
            unsafe {
                let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
                CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER).ok()
            }
        };
    }

    /// The text selected in `pid`'s focused control: through UI
    /// Automation's TextPattern (documents, rich edits, browsers), else by
    /// asking a classic edit control directly (WinForms and Win32 text boxes
    /// expose no TextPattern). None for password boxes, nothing selected, or
    /// another process in focus.
    pub fn selected_text(pid: i32) -> Option<String> {
        let found = uia_selection(pid).or_else(|| edit_control_selection(pid));
        log::debug!(
            "selected text: {}",
            found.as_ref().map_or("none".to_string(), |t| format!(
                "{} chars",
                t.chars().count()
            ))
        );
        found
    }

    fn uia_selection(pid: i32) -> Option<String> {
        use windows::Win32::UI::Accessibility::{IUIAutomationTextPattern, UIA_TextPatternId};
        UIA.with(|uia| {
            let uia = uia.as_ref()?;
            // SAFETY: COM calls on this thread's client; failures are errors.
            unsafe {
                let element = uia.GetFocusedElement().ok()?;
                if element.CurrentProcessId().ok()? != pid
                    || element.CurrentIsPassword().ok()?.as_bool()
                {
                    return None;
                }
                let ranges = element
                    .GetCurrentPatternAs::<IUIAutomationTextPattern>(UIA_TextPatternId)
                    .ok()?
                    .GetSelection()
                    .ok()?;
                let mut text = String::new();
                for i in 0..ranges.Length().ok()? {
                    let piece = ranges.GetElement(i).ok()?.GetText(MAX_FIELD_CHARS).ok()?;
                    text.push_str(&piece.to_string());
                }
                Some(text).filter(|t| !t.is_empty())
            }
        })
    }

    /// A classic edit control (class name with "Edit": Win32, WinForms,
    /// RichEdit) answers EM_GETSEL and WM_GETTEXT itself.
    fn edit_control_selection(pid: i32) -> Option<String> {
        use windows::Win32::Foundation::{LPARAM, WPARAM};
        use windows::Win32::UI::WindowsAndMessaging::{
            GetClassNameW, GetGUIThreadInfo, GetWindowLongW, SendMessageTimeoutW, GUITHREADINFO,
            GWL_STYLE, SMTO_ABORTIFHUNG, WM_GETTEXT, WM_GETTEXTLENGTH,
        };
        const EM_GETSEL: u32 = 0x00B0;
        const ES_PASSWORD: i32 = 0x0020;
        let send = |hwnd: HWND, msg: u32, w: usize, l: isize| -> Option<usize> {
            let mut result = 0usize;
            // SAFETY: standard messages to a window of another process; the
            // system marshals WM_GETTEXT's buffer. Times out if the app hangs.
            let ok = unsafe {
                SendMessageTimeoutW(
                    hwnd,
                    msg,
                    WPARAM(w),
                    LPARAM(l),
                    SMTO_ABORTIFHUNG,
                    500,
                    Some(&mut result),
                )
            };
            (ok.0 != 0).then_some(result)
        };
        // SAFETY: plain user32 queries; a stale handle makes them fail.
        unsafe {
            let fg = GetForegroundWindow();
            let mut owner = 0u32;
            let thread = GetWindowThreadProcessId(fg, Some(&mut owner));
            if owner as i32 != pid {
                return None;
            }
            let mut info = GUITHREADINFO {
                cbSize: std::mem::size_of::<GUITHREADINFO>() as u32,
                ..Default::default()
            };
            GetGUIThreadInfo(thread, &mut info).ok()?;
            let focus = info.hwndFocus;
            if focus.is_invalid() {
                return None;
            }
            let mut class = [0u16; 128];
            let n = GetClassNameW(focus, &mut class);
            let class = String::from_utf16_lossy(&class[..n.max(0) as usize]).to_lowercase();
            if !class.contains("edit") || GetWindowLongW(focus, GWL_STYLE) & ES_PASSWORD != 0 {
                return None;
            }
            let packed = send(focus, EM_GETSEL, 0, 0)? as u32;
            let (start, end) = ((packed & 0xFFFF) as usize, (packed >> 16) as usize);
            if start >= end {
                return None;
            }
            let len = send(focus, WM_GETTEXTLENGTH, 0, 0)?;
            if len > MAX_FIELD_CHARS as usize {
                return None;
            }
            let mut buf = vec![0u16; len + 1];
            let got = send(focus, WM_GETTEXT, buf.len(), buf.as_mut_ptr() as isize)?;
            let text = &buf[..got.min(len)];
            text.get(start..end.min(text.len()))
                .map(String::from_utf16_lossy)
                .filter(|t| !t.is_empty())
        }
    }

    /// The focused UI element of `pid` through UI Automation: its control
    /// type and, unless it is a password box, its text (TextPattern for
    /// documents and rich edits, ValuePattern for single fields). None when
    /// another process has the focus or nothing is focused.
    pub fn focused_field(pid: i32) -> Option<(String, Option<String>)> {
        use windows::Win32::UI::Accessibility::{
            IUIAutomationTextPattern, IUIAutomationValuePattern, UIA_DocumentControlTypeId,
            UIA_EditControlTypeId, UIA_TextPatternId, UIA_ValuePatternId,
        };
        UIA.with(|uia| {
            let uia = uia.as_ref()?;
            // SAFETY: COM calls on this thread's client; failures are errors.
            unsafe {
                let element = uia.GetFocusedElement().ok()?;
                if element.CurrentProcessId().ok()? != pid {
                    return None;
                }
                let control = element.CurrentControlType().ok()?;
                let role = if control == UIA_EditControlTypeId {
                    "Edit".to_string()
                } else if control == UIA_DocumentControlTypeId {
                    "Document".to_string()
                } else {
                    format!("ControlType{}", control.0)
                };
                if element.CurrentIsPassword().ok()?.as_bool() {
                    return Some((role, None));
                }
                let text = element
                    .GetCurrentPatternAs::<IUIAutomationTextPattern>(UIA_TextPatternId)
                    .and_then(|p| p.DocumentRange())
                    .and_then(|r| r.GetText(MAX_FIELD_CHARS + 1))
                    .map(|b| b.to_string())
                    .ok()
                    .or_else(|| {
                        element
                            .GetCurrentPatternAs::<IUIAutomationValuePattern>(UIA_ValuePatternId)
                            .and_then(|p| p.CurrentValue())
                            .map(|b| b.to_string())
                            .ok()
                    })
                    .filter(|t| t.encode_utf16().count() <= MAX_FIELD_CHARS as usize);
                Some((role, text))
            }
        })
    }

    pub fn frontmost() -> Option<(FrontApp, Option<Window>)> {
        let (hwnd, pid, path) = foreground()?;
        Some((
            FrontApp {
                bundle_id: file_name(&path),
                pid: pid as i32,
                window_title: window_title(hwnd),
                name: display_name(&path),
            },
            Some(Window(hwnd.0 as isize)),
        ))
    }

    /// Just the foreground executable (`mintty.exe`), for choosing paste keys.
    pub fn frontmost_id() -> Option<String> {
        foreground().map(|(_, _, path)| file_name(&path))
    }

    /// Top-left corner, in physical pixels, of the screen showing the
    /// foreground window: where the user is typing.
    pub fn foreground_monitor_origin() -> Option<(i32, i32)> {
        use windows::Win32::Graphics::Gdi::{
            GetMonitorInfoW, MonitorFromWindow, MONITORINFO, MONITOR_DEFAULTTONULL,
        };
        // SAFETY: plain queries; `info` is sized as the API requires.
        unsafe {
            let hwnd = GetForegroundWindow();
            if hwnd.is_invalid() {
                return None;
            }
            let monitor = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONULL);
            if monitor.is_invalid() {
                return None;
            }
            let mut info = MONITORINFO {
                cbSize: std::mem::size_of::<MONITORINFO>() as u32,
                ..Default::default()
            };
            GetMonitorInfoW(monitor, &mut info)
                .as_bool()
                .then_some((info.rcMonitor.left, info.rcMonitor.top))
        }
    }
}

#[cfg(all(test, target_os = "windows"))]
mod platform_tests {
    use super::platform::*;

    #[test]
    fn describes_executables_by_their_version_info() {
        let explorer =
            std::env::var("SystemRoot").unwrap_or_else(|_| "C:\\Windows".into()) + "\\explorer.exe";
        let name = display_name(&explorer);
        assert!(!name.is_empty());
        assert_ne!(name, "explorer", "version info should give a description");
    }

    #[test]
    fn falls_back_to_the_file_name() {
        assert!(file_description("C:\\no\\such\\Tool.exe").is_none());
        assert_eq!(display_name("C:\\no\\such\\Tool.exe"), "Tool");
    }

    #[test]
    fn foreground_query_never_panics() {
        // A test runner may have no foreground window at all.
        if let Some((app, window)) = frontmost() {
            assert!(app.bundle_id.ends_with(".exe"));
            let window = window.expect("a foreground app has a window");
            assert!(window.same_as(&window));
        }
        let _ = frontmost_id();
    }
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
mod platform {
    // Linux is not supported; context is always "other" and focus unchecked.
    use super::FrontApp;

    pub struct Window;

    impl Window {
        pub fn same_as(&self, _other: &Window) -> bool {
            true
        }
    }

    pub fn frontmost() -> Option<(FrontApp, Option<Window>)> {
        None
    }

    pub fn focused_field(_pid: i32) -> Option<(String, Option<String>)> {
        None
    }

    pub fn selected_text(_pid: i32) -> Option<String> {
        None
    }
}
