//! Feasibility probe for learning typing habits (1.1, stage 0a).
//!
//! After a paste we watch the focused field of the app we pasted into for up
//! to 20 s and record four facts only: could the field be read, did it show
//! what we pasted, was it then edited, and was it sent (the field emptied).
//! The field's text is compared in memory and dropped; nothing the user typed
//! or said is written anywhere. Per app, this tells us whether learning from
//! the user's own edits is possible at all (設計文件/自動學詞_設計.md).
//!
//! It runs only where `yuyin_field_probe.on` exists in the app data folder.
//! There is no setting for it, and nothing in the app creates that file.

use std::io::Write;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use log::{debug, warn};
use serde::Serialize;
use tauri::{AppHandle, Manager};

use super::context::FrontApp;
use super::session;

const MARKER: &str = "yuyin_field_probe.on";
const LOG_FILE: &str = "yuyin_field_probe.jsonl";
const FIRST_LOOK: Duration = Duration::from_millis(1200);
const EVERY: Duration = Duration::from_secs(1);
const WATCH: Duration = Duration::from_secs(20);
/// Focus is gone after this many unreadable looks in a row.
const MAX_MISSES: u32 = 3;

#[derive(Serialize, Default)]
struct Record {
    at: u128,
    /// Bundle id (macOS) or executable name (Windows).
    app: String,
    name: String,
    role: Option<String>,
    readable: bool,
    shows_paste: bool,
    edited: bool,
    sent: bool,
    looks: u32,
}

/// Fields reflow and trim, so compare without whitespace.
pub(crate) fn squash(s: &str) -> String {
    s.chars().filter(|c| !c.is_whitespace()).collect()
}

/// Whether the field shows the pasted text. Long pastes are matched by their
/// first and last 20 characters, since apps may reformat the middle.
pub(crate) fn shows(field: &str, pasted: &str) -> bool {
    let f = squash(field);
    let p: Vec<char> = squash(pasted).chars().collect();
    if p.is_empty() {
        return false;
    }
    if p.len() <= 40 {
        return f.contains(&p.iter().collect::<String>());
    }
    let head: String = p[..20].iter().collect();
    let tail: String = p[p.len() - 20..].iter().collect();
    f.contains(&head) && f.contains(&tail)
}

/// Call right after a successful paste.
pub fn after_paste(app: &AppHandle, front: Option<FrontApp>, pasted: String) {
    let Some(front) = front else { return };
    let Ok(dir) = app.path().app_data_dir() else {
        return;
    };
    if !dir.join(MARKER).exists() {
        return;
    }
    let generation = session::generation();
    std::thread::spawn(move || {
        std::thread::sleep(FIRST_LOOK);
        let mut rec = Record {
            at: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_millis())
                .unwrap_or(0),
            app: front.bundle_id.clone(),
            name: front.name.clone(),
            ..Default::default()
        };
        // What the field looked like when it first showed our paste, kept
        // in memory only to notice later changes.
        let mut baseline: Option<String> = None;
        let mut misses = 0;
        let started = Instant::now();
        loop {
            // A new dictation takes over the field; stop watching.
            if session::generation() != generation {
                break;
            }
            rec.looks += 1;
            match session::focused_field(front.pid) {
                Some((role, value)) => {
                    misses = 0;
                    rec.role.get_or_insert(role);
                    if let Some(value) = value {
                        rec.readable = true;
                        let now = squash(&value);
                        match &baseline {
                            None if shows(&value, &pasted) => {
                                rec.shows_paste = true;
                                baseline = Some(now);
                            }
                            None => {}
                            Some(_) if now.is_empty() => {
                                rec.sent = true;
                                break;
                            }
                            Some(before) => rec.edited |= &now != before,
                        }
                    }
                }
                None => {
                    misses += 1;
                    if misses >= MAX_MISSES {
                        break;
                    }
                }
            }
            if started.elapsed() >= WATCH {
                break;
            }
            std::thread::sleep(EVERY);
        }
        debug!(
            "field probe: app={} role={:?} readable={} shows={} edited={} sent={}",
            rec.app, rec.role, rec.readable, rec.shows_paste, rec.edited, rec.sent
        );
        let Ok(line) = serde_json::to_string(&rec) else {
            return;
        };
        let result = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(dir.join(LOG_FILE))
            .and_then(|mut f| writeln!(f, "{line}"));
        if let Err(e) = result {
            warn!("Failed to write field probe record: {e}");
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_paste_must_appear_whole() {
        assert!(shows("回覆 Claude：好啊 沒問題", "好啊 沒問題"));
        assert!(!shows("好啊", "好啊，沒問題"));
        assert!(!shows("anything", ""));
    }

    #[test]
    fn long_paste_matches_by_its_ends() {
        let pasted = "我想要做一個 Tauri 的 app，前端用 React，後端用 Rust。先幫我列出 MVP 需要的功能，順便估一下要幾天。";
        let reflowed = pasted.replace("，", "，\n").replace("Rust。", "Rust。  ");
        assert!(shows(&reflowed, pasted));
        let edited_end = pasted.replace("要幾天", "要多久");
        assert!(!shows(&edited_end, pasted));
    }
}
