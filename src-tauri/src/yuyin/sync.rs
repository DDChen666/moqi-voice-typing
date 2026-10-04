//! Sync between the user's computers through a cloud folder they already
//! have (OneDrive, Dropbox, Google Drive, iCloud Drive): no account, no
//! server. Each computer writes only its own file, `<folder>/Moqi/<id>.json`,
//! so the cloud client never sees two computers edit one file; each reads
//! the others' files and merges them, newest change first.
//!
//! What syncs: the dictionary, the voice snippets, the learned corrections,
//! and the clean-up settings (level, service, address, model). What doesn't: API keys (each
//! computer's keychain), recordings, history, statistics, and the consent to
//! learn from edits (asked on each computer).

use std::collections::{BTreeMap, HashMap};
use std::hash::{BuildHasher, Hasher};
use std::path::{Path, PathBuf};
use std::sync::{Condvar, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use log::{debug, warn};
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::{AppHandle, Manager};

use super::config::{self, Level, Service, YuyinConfig};
use super::learn::{self, Rule};
use super::snippets::Snippet;

const STATE_FILE: &str = "yuyin_sync.json";
const SUBFOLDER: &str = "Moqi";
/// Look for the other computers' changes this often.
const EVERY: Duration = Duration::from_secs(60);
/// After a change here, wait this long so a burst of edits syncs once.
const SETTLE: Duration = Duration::from_secs(3);

/// A dictionary word's latest state anywhere.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq)]
struct Mark {
    present: bool,
    at: f64,
}

/// A voice snippet's latest state anywhere, by trigger.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
struct SnippetMark {
    text: String,
    present: bool,
    at: f64,
}

/// The clean-up settings that follow the user from computer to computer.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
struct SharedSettings {
    level: Level,
    service: Service,
    base_url: String,
    model: String,
    #[serde(default)]
    translate_to: Option<String>,
}

impl SharedSettings {
    fn of(cfg: &YuyinConfig) -> Self {
        Self {
            level: cfg.level,
            service: cfg.service,
            base_url: cfg.base_url.clone(),
            model: cfg.model.clone(),
            translate_to: cfg.translate_to.clone(),
        }
    }
    fn apply_to(&self, cfg: &mut YuyinConfig) {
        cfg.level = self.level;
        cfg.service = self.service;
        cfg.base_url = self.base_url.clone();
        cfg.model = self.model.clone();
        cfg.translate_to = self.translate_to.clone();
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct Stamped<T> {
    at: f64,
    value: T,
}

/// One computer's file in the sync folder.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
struct DeviceFile {
    #[serde(default)]
    id: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    os: String,
    #[serde(default)]
    updated_at: f64,
    #[serde(default)]
    vocab: BTreeMap<String, Mark>,
    #[serde(default)]
    learned: Vec<Rule>,
    #[serde(default)]
    settings: Option<Stamped<SharedSettings>>,
    #[serde(default)]
    snippets: BTreeMap<String, SnippetMark>,
}

/// This computer's side of the sync, in the app data folder.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
struct State {
    #[serde(default)]
    device_id: String,
    /// The folder the user picked (the `Moqi` subfolder goes inside it).
    #[serde(default)]
    folder: Option<String>,
    #[serde(default)]
    last_sync: f64,
    /// Every dictionary word this computer knows of, with its latest state.
    #[serde(default)]
    vocab: BTreeMap<String, Mark>,
    /// The shared settings as of the last sync, to notice local changes.
    #[serde(default)]
    settings: Option<Stamped<SharedSettings>>,
    /// Every voice snippet this computer knows of, with its latest state.
    #[serde(default)]
    snippets: BTreeMap<String, SnippetMark>,
    #[serde(default)]
    last_error: Option<String>,
}

/// What the settings page shows.
#[derive(Clone, Debug, Serialize, Type)]
pub struct Status {
    pub folder: Option<String>,
    /// Unix ms; 0 before the first sync.
    pub last_sync: f64,
    /// The other computers in the folder: name and last change (Unix ms).
    pub devices: Vec<(String, f64)>,
    pub error: Option<String>,
}

static LOCK: Mutex<()> = Mutex::new(());
static WAKE: Lazy<(Mutex<bool>, Condvar)> = Lazy::new(|| (Mutex::new(false), Condvar::new()));

fn now_ms() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as f64)
        .unwrap_or(0.0)
}

fn new_device_id() -> String {
    let random = || {
        std::collections::hash_map::RandomState::new()
            .build_hasher()
            .finish()
    };
    format!("{:016x}{:016x}", random(), random())
}

fn state_path(app: &AppHandle) -> Option<PathBuf> {
    app.path().app_data_dir().ok().map(|d| d.join(STATE_FILE))
}

fn load_state(app: &AppHandle) -> State {
    let mut state: State = state_path(app)
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default();
    if state.device_id.is_empty() {
        state.device_id = new_device_id();
        save_state(app, &state);
    }
    state
}

fn save_state(app: &AppHandle, state: &State) {
    let Some(path) = state_path(app) else { return };
    match serde_json::to_string_pretty(state) {
        Ok(json) => {
            if let Err(e) = std::fs::write(path, json) {
                warn!("Failed to save sync state: {e}");
            }
        }
        Err(e) => warn!("Failed to encode sync state: {e}"),
    }
}

fn sync_dir(folder: &str) -> PathBuf {
    Path::new(folder).join(SUBFOLDER)
}

// ---------------------------------------------------------------- merging

/// Record local dictionary changes in `clock`: words added since the last
/// sync become present, words removed become absent, both stamped `now`.
/// On the first sync the words already here are stamped as old, so a word
/// another computer removed stays removed, while words only this computer
/// has are still shared.
fn note_local_vocab(clock: &mut BTreeMap<String, Mark>, vocab: &[String], now: f64) {
    let added_at = if clock.is_empty() { 0.0 } else { now };
    for word in vocab {
        let present = clock.get(word).is_some_and(|m| m.present);
        if !present {
            clock.insert(
                word.clone(),
                Mark {
                    present: true,
                    at: added_at,
                },
            );
        }
    }
    for (word, mark) in clock.iter_mut() {
        if mark.present && !vocab.contains(word) {
            *mark = Mark {
                present: false,
                at: now,
            };
        }
    }
}

/// Record local snippet changes in `clock`, as `note_local_vocab` does for
/// words: a new or edited snippet is stamped `now`, a removed one becomes
/// absent; on the first sync the snippets already here count as old.
fn note_local_snippets(clock: &mut BTreeMap<String, SnippetMark>, snippets: &[Snippet], now: f64) {
    let added_at = if clock.is_empty() { 0.0 } else { now };
    for s in snippets {
        let same = clock
            .get(&s.trigger)
            .is_some_and(|m| m.present && m.text == s.text);
        if !same {
            clock.insert(
                s.trigger.clone(),
                SnippetMark {
                    text: s.text.clone(),
                    present: true,
                    at: added_at,
                },
            );
        }
    }
    for (trigger, mark) in clock.iter_mut() {
        if mark.present && !snippets.iter().any(|s| &s.trigger == trigger) {
            mark.present = false;
            mark.at = now;
        }
    }
}

fn merge_snippets<'a>(
    clocks: impl Iterator<Item = &'a BTreeMap<String, SnippetMark>>,
) -> BTreeMap<String, SnippetMark> {
    let mut merged: BTreeMap<String, SnippetMark> = BTreeMap::new();
    for clock in clocks {
        for (trigger, mark) in clock {
            match merged.get(trigger) {
                Some(have) if have.at >= mark.at => {}
                _ => {
                    merged.insert(trigger.clone(), mark.clone());
                }
            }
        }
    }
    merged
}

/// The snippets after merging: local order kept, new ones appended.
fn apply_snippets(local: &[Snippet], merged: &BTreeMap<String, SnippetMark>) -> Vec<Snippet> {
    let mut out: Vec<Snippet> = local
        .iter()
        .filter_map(|s| match merged.get(&s.trigger) {
            Some(m) if !m.present => None,
            Some(m) => Some(Snippet {
                trigger: s.trigger.clone(),
                text: m.text.clone(),
            }),
            None => Some(s.clone()),
        })
        .collect();
    for (trigger, mark) in merged {
        if mark.present && !out.iter().any(|s| &s.trigger == trigger) {
            out.push(Snippet {
                trigger: trigger.clone(),
                text: mark.text.clone(),
            });
        }
    }
    out
}

/// Newest state of every word across `clocks`.
fn merge_vocab<'a>(
    clocks: impl Iterator<Item = &'a BTreeMap<String, Mark>>,
) -> BTreeMap<String, Mark> {
    let mut merged: BTreeMap<String, Mark> = BTreeMap::new();
    for clock in clocks {
        for (word, mark) in clock {
            match merged.get(word) {
                Some(have) if have.at >= mark.at => {}
                _ => {
                    merged.insert(word.clone(), *mark);
                }
            }
        }
    }
    merged
}

/// The dictionary after merging: the local order kept, new words appended.
fn apply_vocab(local: &[String], merged: &BTreeMap<String, Mark>) -> Vec<String> {
    let present = |w: &String| merged.get(w).is_none_or(|m| m.present);
    let mut out: Vec<String> = local.iter().filter(|w| present(w)).cloned().collect();
    for (word, mark) in merged {
        if mark.present && !out.contains(word) {
            out.push(word.clone());
        }
    }
    out
}

/// Newest version of every learned correction across `lists`.
fn merge_rules<'a>(lists: impl Iterator<Item = &'a Vec<Rule>>) -> Vec<Rule> {
    let mut merged: HashMap<(String, String), Rule> = HashMap::new();
    for list in lists {
        for rule in list {
            let key = (rule.from.clone(), rule.to.clone());
            match merged.get(&key) {
                Some(have) if have.changed >= rule.changed => {}
                _ => {
                    merged.insert(key, rule.clone());
                }
            }
        }
    }
    let mut rules: Vec<Rule> = merged.into_values().collect();
    rules.sort_by(|a, b| a.from.cmp(&b.from).then(a.to.cmp(&b.to)));
    rules
}

fn newest_settings<'a>(
    all: impl Iterator<Item = &'a Option<Stamped<SharedSettings>>>,
) -> Option<Stamped<SharedSettings>> {
    all.flatten().max_by(|a, b| a.at.total_cmp(&b.at)).cloned()
}

// ---------------------------------------------------------------- syncing

fn read_others(dir: &Path, own_id: &str) -> Vec<DeviceFile> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "json"))
        .filter(|p| p.file_stem().is_some_and(|s| s != own_id))
        .filter_map(|p| std::fs::read_to_string(&p).ok())
        .filter_map(|s| serde_json::from_str::<DeviceFile>(&s).ok())
        .filter(|f| !f.id.is_empty() && f.id != own_id)
        .collect()
}

/// One round: note local changes, merge the other computers' files, apply
/// the result here, and publish this computer's file.
pub fn sync_now(app: &AppHandle) -> Result<(), String> {
    let _guard = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut state = load_state(app);
    let Some(folder) = state.folder.clone() else {
        return Ok(());
    };
    let result = run(app, &mut state, &folder);
    state.last_error = result.as_ref().err().cloned();
    if result.is_ok() {
        state.last_sync = now_ms();
    }
    save_state(app, &state);
    result
}

fn run(app: &AppHandle, state: &mut State, folder: &str) -> Result<(), String> {
    let dir = sync_dir(folder);
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let now = now_ms();
    let mut cfg = config::get(app);

    let others = read_others(&dir, &state.device_id);

    // What changed here since the last round. A computer joining adopts the
    // settings already in the folder; only later changes here count as new.
    note_local_vocab(&mut state.vocab, &cfg.vocab, now);
    note_local_snippets(&mut state.snippets, &cfg.snippets, now);
    let shared = SharedSettings::of(&cfg);
    let changed_here = match &state.settings {
        Some(last) => last.value != shared,
        None => others.iter().all(|o| o.settings.is_none()),
    };
    if changed_here {
        state.settings = Some(Stamped {
            at: now,
            value: shared,
        });
    }
    let local_rules = learn::all_rules(app);

    // Everyone's latest.
    let vocab = merge_vocab(std::iter::once(&state.vocab).chain(others.iter().map(|o| &o.vocab)));
    let rules = merge_rules(std::iter::once(&local_rules).chain(others.iter().map(|o| &o.learned)));
    let settings =
        newest_settings(std::iter::once(&state.settings).chain(others.iter().map(|o| &o.settings)));

    let snippets =
        merge_snippets(std::iter::once(&state.snippets).chain(others.iter().map(|o| &o.snippets)));

    // Apply here.
    let mut next = cfg.clone();
    next.vocab = apply_vocab(&cfg.vocab, &vocab);
    next.snippets = apply_snippets(&cfg.snippets, &snippets);
    if let Some(s) = &settings {
        s.value.apply_to(&mut next);
    }
    if next != cfg {
        config::set(app, next.clone())?;
        cfg = next;
    }
    if rules != local_rules {
        learn::replace_rules(app, rules.clone());
    }
    state.vocab = vocab;
    state.snippets = snippets;
    state.settings = settings.or_else(|| {
        Some(Stamped {
            at: now,
            value: SharedSettings::of(&cfg),
        })
    });

    // Publish this computer's view.
    let file = DeviceFile {
        id: state.device_id.clone(),
        name: tauri_plugin_os::hostname(),
        os: std::env::consts::OS.to_string(),
        updated_at: now,
        vocab: state.vocab.clone(),
        learned: rules,
        settings: state.settings.clone(),
        snippets: state.snippets.clone(),
    };
    let json = serde_json::to_string_pretty(&file).map_err(|e| e.to_string())?;
    // Write then rename, so the cloud client never uploads half a file.
    let target = dir.join(format!("{}.json", state.device_id));
    let temp = dir.join(format!(".{}.tmp", state.device_id));
    std::fs::write(&temp, json).map_err(|e| format!("{}: {e}", temp.display()))?;
    std::fs::rename(&temp, &target).map_err(|e| format!("{}: {e}", target.display()))?;
    debug!("sync: merged {} other computer(s)", others.len());
    Ok(())
}

/// Something here changed (dictionary, settings, a learned correction):
/// sync soon.
pub fn changed() {
    let (lock, wake) = &*WAKE;
    if let Ok(mut pending) = lock.lock() {
        *pending = true;
        wake.notify_all();
    }
}

/// Sync at launch, after changes here, and every minute for the others'.
pub fn start(app: &AppHandle) {
    let app = app.clone();
    std::thread::spawn(move || loop {
        if let Err(e) = sync_now(&app) {
            warn!("Sync failed: {e}");
        }
        let (lock, wake) = &*WAKE;
        let Ok(pending) = lock.lock() else { return };
        let (mut pending, _) = wake
            .wait_timeout_while(pending, EVERY, |p| !*p)
            .unwrap_or_else(|e| e.into_inner());
        let was_change = *pending;
        *pending = false;
        drop(pending);
        if was_change {
            std::thread::sleep(SETTLE);
        }
    });
}

pub fn status(app: &AppHandle) -> Status {
    let state = load_state(app);
    let devices = state
        .folder
        .as_deref()
        .map(|f| read_others(&sync_dir(f), &state.device_id))
        .unwrap_or_default()
        .into_iter()
        .map(|d| (if d.name.is_empty() { d.id } else { d.name }, d.updated_at))
        .collect();
    Status {
        folder: state.folder,
        last_sync: state.last_sync,
        devices,
        error: state.last_error,
    }
}

/// Start syncing with `folder`, or stop (`None`). This computer's file stays
/// in the folder when stopping, so the others keep what it shared.
pub fn set_folder(app: &AppHandle, folder: Option<String>) -> Result<(), String> {
    {
        let _guard = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let mut state = load_state(app);
        if let Some(f) = &folder {
            if !Path::new(f).is_dir() {
                return Err(format!("{f} is not a folder"));
            }
        }
        state.folder = folder;
        state.last_error = None;
        save_state(app, &state);
    }
    changed();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mark(present: bool, at: f64) -> Mark {
        Mark { present, at }
    }

    #[test]
    fn local_dictionary_changes_are_stamped() {
        let mut clock = BTreeMap::new();
        clock.insert("Claude".to_string(), mark(true, 1.0));
        clock.insert("Notion".to_string(), mark(true, 1.0));
        note_local_vocab(&mut clock, &["Claude".into(), "Supabase".into()], 5.0);
        assert_eq!(clock["Claude"], mark(true, 1.0));
        assert_eq!(clock["Notion"], mark(false, 5.0));
        assert_eq!(clock["Supabase"], mark(true, 5.0));
    }

    #[test]
    fn a_joining_computer_does_not_undo_removals() {
        let mut here = BTreeMap::new();
        note_local_vocab(&mut here, &["Notion".into(), "Supabase".into()], 9.0);
        let mut mac = BTreeMap::new();
        mac.insert("Notion".to_string(), mark(false, 5.0));
        let merged = merge_vocab([&here, &mac].into_iter());
        let local = vec!["Notion".to_string(), "Supabase".to_string()];
        assert_eq!(apply_vocab(&local, &merged), vec!["Supabase".to_string()]);
    }

    #[test]
    fn newest_change_of_a_word_wins() {
        let mut mac = BTreeMap::new();
        mac.insert("Notion".to_string(), mark(false, 9.0));
        mac.insert("Gumroad".to_string(), mark(true, 3.0));
        let mut win = BTreeMap::new();
        win.insert("Notion".to_string(), mark(true, 1.0));
        win.insert("Supabase".to_string(), mark(true, 4.0));
        let merged = merge_vocab([&win, &mac].into_iter());
        let local = vec![
            "Notion".to_string(),
            "Supabase".to_string(),
            "Claude".to_string(),
        ];
        // Notion removed on the Mac later; Gumroad added there; Claude was
        // never synced and stays.
        assert_eq!(
            apply_vocab(&local, &merged),
            vec![
                "Supabase".to_string(),
                "Claude".to_string(),
                "Gumroad".to_string()
            ]
        );
    }

    fn rule(from: &str, to: &str, active: bool, changed: f64) -> Rule {
        Rule {
            from: from.into(),
            to: to.into(),
            count: 2,
            active,
            dismissed: !active,
            last_seen: changed,
            changed,
        }
    }

    #[test]
    fn newest_rule_state_wins() {
        let here = vec![rule("這周", "這週", true, 1.0)];
        let there = vec![
            rule("這周", "這週", false, 2.0),
            rule("表格", "Excel", true, 1.5),
        ];
        let merged = merge_rules([&here, &there].into_iter());
        assert_eq!(merged.len(), 2);
        assert!(merged.iter().any(|r| r.from == "這周" && !r.active));
        assert!(merged.iter().any(|r| r.from == "表格" && r.active));
    }

    #[test]
    fn snippets_merge_by_trigger() {
        let mine = vec![Snippet {
            trigger: "我的地址".into(),
            text: "舊地址".into(),
        }];
        let mut here = BTreeMap::new();
        note_local_snippets(&mut here, &mine, 1.0);
        let mut mac = BTreeMap::new();
        mac.insert(
            "我的地址".to_string(),
            SnippetMark {
                text: "新地址".into(),
                present: true,
                at: 5.0,
            },
        );
        mac.insert(
            "我的信箱".to_string(),
            SnippetMark {
                text: "me@example.com".into(),
                present: true,
                at: 5.0,
            },
        );
        let merged = merge_snippets([&here, &mac].into_iter());
        let out = apply_snippets(&mine, &merged);
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].text, "新地址");
        assert_eq!(out[1].trigger, "我的信箱");
    }

    #[test]
    fn device_files_tolerate_missing_fields() {
        let f: DeviceFile = serde_json::from_str(r#"{"id": "abc"}"#).unwrap();
        assert_eq!(f.id, "abc");
        assert!(f.vocab.is_empty() && f.settings.is_none());
        assert_ne!(new_device_id(), new_device_id());
    }
}
