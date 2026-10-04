//! Learning from the user's own corrections (自動學詞).
//!
//! Two ways in, one store of corrections (`yuyin_learned.json`):
//!
//! - **Watched** ([`after_paste`]): after Moqi pastes, the field it typed
//!   into is read now and then (the same `session::focused_field` as the
//!   field probe) until the user is done with it. Apps that hide their
//!   fields until asked (VS Code, Slack…) are asked at the key press
//!   (`session::wake_accessibility`).
//! - **Taught** ([`teach`]): the user corrects a dictation in Moqi's history.
//!   It works the same in every app on every platform, because nothing is
//!   read from other apps.
//!
//! Either way, a correction is learned at once when it fixes a misheard
//! word: one turned into an English term (蘇帕貝斯 → Supabase), or a Chinese
//! word that sounds alike (實做 → 實作, 時辰 → 時程; [`sounds_alike`]). A
//! correction that changes the sound (但是 → 可是) is a rewording, not a
//! misheard word: applied everywhere it would replace every 但是 the user
//! ever says, so it is learned only once the user has made it twice.
//!
//! A learned correction is applied to later dictations, before and after the
//! clean-up. Its corrected word also joins the dictionary, which the
//! clean-up spells by and the recognizer listens for (`asr_vocab`), so the
//! word comes out right however it would have been misheard next time.
//!
//! Privacy (docs/隱私.md): watching is off until the user turns it on
//! (`YuyinConfig::learn_from_edits`); the field's text is compared in memory
//! and dropped; only the corrected words are saved; nothing is sent
//! anywhere; password boxes are never read.

use std::path::PathBuf;
use std::sync::RwLock;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use log::{debug, warn};
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::{AppHandle, Manager};

use super::config;
use super::context::FrontApp;
use super::session;

const FILE: &str = "yuyin_learned.json";
const FIRST_LOOK: Duration = Duration::from_millis(1200);
/// How often the field is read. A chat box empties when the user sends, and
/// the last reading before that is the corrected text: at 1 s a fix made
/// just before pressing Enter was often missed.
const EVERY: Duration = Duration::from_millis(500);
/// The paste must show up in the field this soon, or there is nothing to watch.
const SHOWS_WITHIN: Duration = Duration::from_secs(5);
/// Done once the text has stopped changing for this long after an edit.
const SETTLED: Duration = Duration::from_secs(6);
/// Nothing changed this long after the paste: nothing to learn.
const UNTOUCHED: Duration = Duration::from_secs(45);
const LONGEST: Duration = Duration::from_secs(180);
/// Focus is gone after this many unreadable looks in a row.
const MAX_MISSES: u32 = 6;
/// A rewording made this many times is applied from then on (a misheard
/// word needs one; see the module docs).
pub const TIMES_TO_LEARN: u32 = 2;

/// One correction the user made: `from` (what Moqi typed) → `to`.
#[derive(Clone, Debug, Serialize, Deserialize, Type, PartialEq)]
pub struct Rule {
    pub from: String,
    pub to: String,
    /// How many times the user made this correction.
    pub count: u32,
    /// Applied to new dictations.
    pub active: bool,
    /// Removed or undone by the user: kept so it is never learned again.
    pub dismissed: bool,
    /// When it was last seen, Unix milliseconds.
    pub last_seen: f64,
    /// When it last changed in any way (sync keeps the newest), Unix ms.
    #[serde(default)]
    pub changed: f64,
}

#[derive(Default, Serialize, Deserialize)]
struct Store {
    #[serde(default)]
    rules: Vec<Rule>,
}

static STORE: Lazy<RwLock<Option<Store>>> = Lazy::new(|| RwLock::new(None));

fn store_path(app: &AppHandle) -> Option<PathBuf> {
    app.path().app_data_dir().ok().map(|d| d.join(FILE))
}

/// Run `f` on the store (loaded on first use), saving it if `f` says so.
fn with_store<T>(app: &AppHandle, f: impl FnOnce(&mut Store) -> (T, bool)) -> T {
    let mut slot = STORE.write().unwrap_or_else(|e| e.into_inner());
    let store = slot.get_or_insert_with(|| {
        store_path(app)
            .and_then(|p| std::fs::read_to_string(p).ok())
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    });
    let (result, changed) = f(store);
    if changed {
        if let Some(path) = store_path(app) {
            let written = serde_json::to_string_pretty(&*store)
                .map_err(|e| e.to_string())
                .and_then(|json| std::fs::write(&path, json).map_err(|e| e.to_string()));
            if let Err(e) = written {
                warn!("Failed to save learned corrections: {e}");
            }
        }
    }
    result
}

fn now_ms() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as f64)
        .unwrap_or(0.0)
}

// ---------------------------------------------------------------- applying

/// `text` with every learned correction applied.
pub fn apply(app: &AppHandle, text: &str) -> String {
    let mut rules: Vec<(String, String)> = with_store(app, |s| {
        let active = s
            .rules
            .iter()
            .filter(|r| r.active && !r.dismissed)
            .map(|r| (r.from.clone(), r.to.clone()))
            .collect();
        (active, false)
    });
    if rules.is_empty() {
        return text.to_string();
    }
    // Longer first, so 蘇帕貝斯 wins over 貝斯.
    rules.sort_by_key(|(from, _)| std::cmp::Reverse(from.chars().count()));
    apply_by_sound(&apply_rules(text, &rules), &rules)
}

/// The recognizer spells an English name it doesn't know with whatever
/// characters sound like it, and not the same ones each time: taught
/// 深拓 -> Zentro, it wrote 申拓 the next time. So a correction to an English
/// term also replaces any Chinese spelling that sounds like the one taught
/// (two characters or more). Corrections between Chinese words stay exact:
/// a word that sounds like 時辰 may well be meant.
fn apply_by_sound(text: &str, rules: &[(String, String)]) -> String {
    let by_sound: Vec<(Vec<Vec<String>>, &str)> = rules
        .iter()
        .filter(|(from, to)| {
            to.chars().any(|c| c.is_ascii_alphabetic())
                && from.chars().count() >= 2
                && from.chars().all(is_han)
        })
        .filter_map(|(from, to)| syllables(from).map(|s| (s, to.as_str())))
        .collect();
    if by_sound.is_empty() {
        return text.to_string();
    }
    use pinyin::ToPinyinMulti;
    let chars: Vec<char> = text.chars().collect();
    let readings: Vec<Option<Vec<String>>> = chars
        .iter()
        .map(|c| {
            c.to_pinyin_multi()
                .map(|r| r.into_iter().map(|p| fuzzy(p.plain())).collect())
        })
        .collect();
    let sounds_like = |at: usize, syllables: &[Vec<String>]| {
        at + syllables.len() <= chars.len()
            && syllables.iter().enumerate().all(|(k, want)| {
                readings[at + k]
                    .as_ref()
                    .is_some_and(|have| have.iter().any(|r| want.contains(r)))
            })
    };
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while i < chars.len() {
        match by_sound.iter().find(|(syl, _)| sounds_like(i, syl)) {
            Some((syl, to)) => {
                out.push_str(&spaced(
                    out.chars().next_back(),
                    to,
                    chars.get(i + syl.len()).copied(),
                ));
                i += syl.len();
            }
            None => {
                out.push(chars[i]);
                i += 1;
            }
        }
    }
    out
}

/// `to` with a space on a side where English meets Chinese, the way Moqi
/// writes mixed text (交給 Kalopp 團隊, not 交給Kalopp團隊).
fn spaced(before: Option<char>, to: &str, after: Option<char>) -> String {
    let latin = |c: Option<char>| c.is_some_and(|c| c.is_ascii_alphanumeric());
    let lead = before.is_some_and(is_han) && latin(to.chars().next());
    let trail = after.is_some_and(is_han) && latin(to.chars().next_back());
    format!(
        "{}{to}{}",
        if lead { " " } else { "" },
        if trail { " " } else { "" }
    )
}

fn is_han(c: char) -> bool {
    ('\u{4e00}'..='\u{9fff}').contains(&c) || ('\u{3400}'..='\u{4dbf}').contains(&c)
}

fn apply_rules(text: &str, rules: &[(String, String)]) -> String {
    rules.iter().fold(text.to_string(), |text, (from, to)| {
        replace_whole(&text, from, to)
    })
}

/// Replace `from` with `to`, but not inside a longer Latin word ("cloud" in
/// "cloudy"), and not where `to` is already written out (applying
/// Claude → Claude Code twice must not give "Claude Code Code").
fn replace_whole(text: &str, from: &str, to: &str) -> String {
    if from.is_empty() {
        return text.to_string();
    }
    let inner = to.find(from);
    let glued = |a: Option<char>, b: Option<char>| matches!((a, b), (Some(a), Some(b)) if a.is_ascii_alphanumeric() && b.is_ascii_alphanumeric());
    let mut out = String::with_capacity(text.len());
    let mut last = 0;
    let mut search = 0;
    while let Some(pos) = text[search..].find(from) {
        let at = search + pos;
        let end = at + from.len();
        let written_out =
            inner.is_some_and(|k| at >= k && text.get(at - k..).is_some_and(|t| t.starts_with(to)));
        let inside_word = glued(text[..at].chars().next_back(), from.chars().next())
            || glued(from.chars().next_back(), text[end..].chars().next());
        if !written_out && !inside_word {
            out.push_str(&text[last..at]);
            out.push_str(&spaced(
                text[..at].chars().next_back(),
                to,
                text[end..].chars().next(),
            ));
            last = end;
        }
        search = end;
    }
    out.push_str(&text[last..]);
    out
}

// ---------------------------------------------------------------- watching

/// Call right after a successful paste of `pasted` into `front`; `heard` is
/// the recognizer's text before the clean-up.
pub fn after_paste(app: &AppHandle, front: Option<FrontApp>, pasted: String, heard: String) {
    if config::get(app).learn_from_edits != Some(true) {
        return;
    }
    let Some(front) = front else { return };
    let pasted = normalize(&pasted);
    if pasted.trim().is_empty() {
        return;
    }
    let generation = session::generation();
    let app = app.clone();
    std::thread::spawn(move || {
        let Some(edited) = watch(front.pid, &pasted, generation) else {
            return;
        };
        let dictionary = config::get(&app).vocab;
        let found = learnable(&pasted, &normalize(&heard), &edited, &dictionary);
        debug!(
            "learn: {} correction(s) in {}",
            found.len(),
            front.bundle_id
        );
        if !found.is_empty() {
            record(&app, &found);
        }
    });
}

/// Fields differ in line breaks (RichEdit uses \r) and non-breaking spaces.
fn normalize(s: &str) -> String {
    s.replace("\r\n", "\n")
        .replace('\r', "\n")
        .replace('\u{a0}', " ")
}

/// Read the field until the user is done with it; what became of the paste.
fn watch(pid: i32, pasted: &str, generation: u64) -> Option<String> {
    std::thread::sleep(FIRST_LOOK);
    let started = Instant::now();
    // The field when it first showed the paste, and its latest reading
    // that still has the text around the paste untouched.
    let mut baseline: Option<String> = None;
    let mut latest: Option<String> = None;
    let mut changed_at: Option<Instant> = None;
    let mut misses = 0;
    loop {
        // A new dictation takes over the field.
        if session::generation() != generation {
            break;
        }
        let reading = session::focused_field(pid)
            .and_then(|(_, v)| v)
            .map(|v| normalize(&v));
        match (reading, &baseline) {
            (Some(value), None) => {
                misses = 0;
                if value.contains(pasted) {
                    latest = Some(value.clone());
                    baseline = Some(value);
                } else if started.elapsed() >= SHOWS_WITHIN {
                    return None;
                }
            }
            // Sent (a chat box empties): the last reading is the final text.
            (Some(value), Some(_)) if value.trim().is_empty() => break,
            (Some(value), Some(base)) if edited_span(base, pasted, &value).is_some() => {
                misses = 0;
                if latest.as_deref() != Some(value.as_str()) {
                    changed_at = Some(Instant::now());
                    latest = Some(value);
                }
            }
            // Unreadable, another field, or text outside the paste changed.
            _ => {
                misses += 1;
                if misses >= MAX_MISSES {
                    break;
                }
            }
        }
        let waited = started.elapsed();
        match changed_at {
            Some(at) if at.elapsed() >= SETTLED => break,
            None if waited >= UNTOUCHED => return None,
            _ => {}
        }
        if waited >= LONGEST {
            break;
        }
        std::thread::sleep(EVERY);
    }
    changed_at?;
    edited_span(baseline.as_deref()?, pasted, latest.as_deref()?)
}

/// What became of the paste: the part of `now` between the text that came
/// before and after it in `baseline`. None if that surrounding text changed.
fn edited_span(baseline: &str, pasted: &str, now: &str) -> Option<String> {
    let at = baseline.rfind(pasted)?;
    let (prefix, suffix) = (&baseline[..at], &baseline[at + pasted.len()..]);
    if now.len() < prefix.len() + suffix.len() || !now.starts_with(prefix) || !now.ends_with(suffix)
    {
        return None;
    }
    Some(now[prefix.len()..now.len() - suffix.len()].to_string())
}

// ---------------------------------------------------------------- comparing

#[derive(Clone, Copy, Debug, PartialEq)]
enum Kind {
    /// A run of ASCII letters and digits ("GitHub", "v2").
    Latin,
    /// One letter of any other script (Chinese characters one by one).
    Char,
    /// Punctuation and symbols.
    Mark,
}

#[derive(Debug)]
struct Tok {
    start: usize,
    end: usize,
    kind: Kind,
}

fn tokenize(s: &str) -> Vec<Tok> {
    let mut out = Vec::new();
    let mut chars = s.char_indices().peekable();
    while let Some((i, c)) = chars.next() {
        if c.is_whitespace() {
            continue;
        }
        if c.is_ascii_alphanumeric() {
            let mut end = i + c.len_utf8();
            while let Some(&(j, d)) = chars.peek() {
                if !d.is_ascii_alphanumeric() {
                    break;
                }
                end = j + d.len_utf8();
                chars.next();
            }
            out.push(Tok {
                start: i,
                end,
                kind: Kind::Latin,
            });
        } else {
            let kind = if c.is_alphanumeric() {
                Kind::Char
            } else {
                Kind::Mark
            };
            out.push(Tok {
                start: i,
                end: i + c.len_utf8(),
                kind,
            });
        }
    }
    out
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Op {
    Same(usize, usize),
    Del(usize),
    Ins(usize),
}

/// Longest-common-subsequence diff of two token lists.
fn diff(a: &[&str], b: &[&str]) -> Vec<Op> {
    let (n, m) = (a.len(), b.len());
    let mut lcs = vec![0u32; (n + 1) * (m + 1)];
    let at = |i: usize, j: usize| i * (m + 1) + j;
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            lcs[at(i, j)] = if a[i] == b[j] {
                lcs[at(i + 1, j + 1)] + 1
            } else {
                lcs[at(i + 1, j)].max(lcs[at(i, j + 1)])
            };
        }
    }
    let (mut i, mut j) = (0, 0);
    let mut ops = Vec::with_capacity(n + m);
    while i < n || j < m {
        if i < n && j < m && a[i] == b[j] {
            ops.push(Op::Same(i, j));
            i += 1;
            j += 1;
        } else if j < m && (i == n || lcs[at(i, j + 1)] >= lcs[at(i + 1, j)]) {
            ops.push(Op::Ins(j));
            j += 1;
        } else {
            ops.push(Op::Del(i));
            i += 1;
        }
    }
    ops
}

/// Diffs bigger than this many token pairs are not worth comparing.
const MAX_CELLS: usize = 4_000_000;
/// Tokens on either side of one correction.
const MAX_TOKENS: usize = 6;

/// The word-level corrections from `pasted` to `edited`: (what Moqi typed,
/// what the user made of it). Rewrites, additions, deletions and changes to
/// punctuation alone are not corrections.
fn corrections(pasted: &str, edited: &str) -> Vec<(String, String)> {
    let (a, b) = (tokenize(pasted), tokenize(edited));
    if a.is_empty() || b.is_empty() || a.len() * b.len() > MAX_CELLS {
        return Vec::new();
    }
    let at: Vec<&str> = a.iter().map(|t| &pasted[t.start..t.end]).collect();
    let bt: Vec<&str> = b.iter().map(|t| &edited[t.start..t.end]).collect();
    let ops = diff(&at, &bt);
    // Rewritten rather than corrected: more than 40 % of it removed. Not for
    // a short dictation, where fixing one term is already that much
    // (打開蘇帕貝斯 -> 打開 Supabase); MAX_TOKENS still bounds each change.
    let removed = ops.iter().filter(|o| matches!(o, Op::Del(_))).count();
    if a.len() >= 10 && removed * 10 > a.len() * 4 {
        return Vec::new();
    }

    let mut found = Vec::new();
    let mut k = 0;
    while k < ops.len() {
        if matches!(ops[k], Op::Same(..)) {
            k += 1;
            continue;
        }
        let first = k;
        while k < ops.len() && !matches!(ops[k], Op::Same(..)) {
            k += 1;
        }
        let dels: Vec<usize> = ops[first..k]
            .iter()
            .filter_map(|o| if let Op::Del(i) = o { Some(*i) } else { None })
            .collect();
        let inss: Vec<usize> = ops[first..k]
            .iter()
            .filter_map(|o| if let Op::Ins(j) = o { Some(*j) } else { None })
            .collect();
        let before = first.checked_sub(1).map(|p| ops[p]);
        let after = ops.get(k).copied();
        if let Some(pair) = correction(pasted, edited, &a, &b, &dels, &inss, before, after) {
            if !found.contains(&pair) {
                found.push(pair);
            }
        }
    }
    found
}

#[allow(clippy::too_many_arguments)]
fn correction(
    pasted: &str,
    edited: &str,
    a: &[Tok],
    b: &[Tok],
    dels: &[usize],
    inss: &[usize],
    before: Option<Op>,
    after: Option<Op>,
) -> Option<(String, String)> {
    if dels.is_empty() || inss.is_empty() || dels.len() > MAX_TOKENS || inss.len() > MAX_TOKENS {
        return None;
    }
    let all_marks = |toks: &[Tok], idx: &[usize]| idx.iter().all(|&i| toks[i].kind == Kind::Mark);
    if all_marks(a, dels) || all_marks(b, inss) {
        return None;
    }
    let (mut from_start, mut from_end) = (a[dels[0]].start, a[*dels.last()?].end);
    let (mut to_start, mut to_end) = (b[inss[0]].start, b[*inss.last()?].end);
    // One changed Chinese character is too small to apply on its own
    // (辰 → 程 would hit every 辰): take its neighbour too (時辰 → 時程).
    let one_char = |toks: &[Tok], idx: &[usize]| idx.len() == 1 && toks[idx[0]].kind == Kind::Char;
    if one_char(a, dels) || one_char(b, inss) {
        match (before, after) {
            (Some(Op::Same(i, j)), _) if a[i].kind == Kind::Char => {
                from_start = a[i].start;
                to_start = b[j].start;
            }
            (_, Some(Op::Same(i, j))) if a[i].kind == Kind::Char => {
                from_end = a[i].end;
                to_end = b[j].end;
            }
            _ => return None,
        }
    }
    let from = pasted[from_start..from_end].trim();
    let to = edited[to_start..to_end].trim();
    if from == to || from.chars().count() > 16 || to.chars().count() > 32 {
        return None;
    }
    Some((from.to_string(), to.to_string()))
}

// ---------------------------------------------------------------- remembering

fn record(app: &AppHandle, found: &[(String, String)]) {
    let learned = with_store(app, |store| {
        let learned = learn_into(store, found, now_ms(), false);
        (learned, true)
    });
    super::sync::changed();
    add_to_dictionary(app, &dictionary_words(&learned));
}

/// The corrected words worth a dictionary entry: misheard terms only, so a
/// rewording (但是 → 可是) never crowds the recognizer's list.
fn dictionary_words(learned: &[(String, String)]) -> Vec<String> {
    learned
        .iter()
        .filter(|(from, to)| is_term(to) && misheard(from, to))
        .map(|(_, to)| to.clone())
        .collect()
}

/// What one taught correction learned, for the history page to show.
#[derive(Clone, Debug, Default, Serialize, Type, PartialEq)]
pub struct Taught {
    /// Corrections now applied to new dictations: (what Moqi typed, the fix).
    pub corrections: Vec<(String, String)>,
    /// Rewordings noted but not applied until the user makes them again.
    pub noted: Vec<(String, String)>,
    /// Words now in the dictionary (and listened for by the recognizer).
    pub words: Vec<String>,
}

/// The user corrected `before` (what Moqi typed) into `after` in Moqi's
/// history: learn the misheard words at once, note the rewordings.
pub fn teach(app: &AppHandle, before: &str, heard: &str, after: &str) -> Taught {
    let dictionary = config::get(app).vocab;
    let found = learnable(
        &normalize(before),
        &normalize(heard),
        &normalize(after),
        &dictionary,
    );
    if found.is_empty() {
        return Taught::default();
    }
    let active = with_store(app, |store| {
        learn_into(store, &found, now_ms(), true);
        let active: Vec<(String, String)> = found
            .iter()
            .filter(|(from, to)| {
                store
                    .rules
                    .iter()
                    .any(|r| r.active && r.from == *from && r.to == *to)
            })
            .cloned()
            .collect();
        (active, true)
    });
    super::sync::changed();
    let words = dictionary_words(&active);
    add_to_dictionary(app, &words);
    debug!(
        "learn: taught {} correction(s), {} applied now",
        found.len(),
        active.len()
    );
    Taught {
        noted: found.into_iter().filter(|f| !active.contains(f)).collect(),
        corrections: active,
        words,
    }
}

/// The corrections to learn from the user turning `typed` into `edited`,
/// given what the recognizer `heard` before the clean-up.
///
/// A dictionary word is never learned as a mistake: the clean-up sometimes
/// "corrects" a name it doesn't know into one it does (卡洛普 came out as the
/// user's Claude), and learning Claude -> Kalopp from the user's fix would
/// replace every Claude after it. The recognizer's own spelling of that spot
/// is what was misheard, so when `heard` differs, its corrections are
/// learned too, but only those that end in a word the user actually wrote
/// in their fix (so the clean-up's own rewording is never learned).
fn learnable(
    typed: &str,
    heard: &str,
    edited: &str,
    dictionary: &[String],
) -> Vec<(String, String)> {
    let in_dictionary = |word: &str| {
        dictionary
            .iter()
            .any(|d| d.trim().eq_ignore_ascii_case(word.trim()))
    };
    let fixes = corrections(typed, edited);
    let mut found: Vec<(String, String)> = fixes
        .iter()
        .filter(|(from, _)| !in_dictionary(from))
        .cloned()
        .collect();
    if heard.trim() != typed.trim() {
        for (from, to) in corrections(heard, edited) {
            let the_users_fix = fixes.iter().any(|(_, fixed)| *fixed == to);
            if the_users_fix
                && !in_dictionary(&from)
                && !found.contains(&(from.clone(), to.clone()))
            {
                found.push((from, to));
            }
        }
    }
    found
}

/// Worth a dictionary entry: an English term, or two or more Chinese
/// characters (a name or term of art, not a lone particle).
fn is_term(word: &str) -> bool {
    word.chars().any(|c| c.is_ascii_alphabetic())
        || word.chars().filter(|c| c.is_alphabetic()).count() >= 2
}

/// Put `words` in the dictionary (once each) and in front of the
/// recognizer's list.
fn add_to_dictionary(app: &AppHandle, words: &[String]) {
    if words.is_empty() {
        return;
    }
    let mut cfg = config::get(app);
    let mut added = false;
    for word in words {
        if !cfg.vocab.iter().any(|w| w.eq_ignore_ascii_case(word)) {
            cfg.vocab.push(word.clone());
            added = true;
        }
    }
    if added {
        if let Err(e) = config::set(app, cfg) {
            warn!("Failed to add a learned word to the dictionary: {e}");
        }
    }
    super::asr_vocab::touch(app, words);
}

/// A misheard word rather than a reworded one (see the module docs).
fn misheard(from: &str, to: &str) -> bool {
    to.chars().any(|c| c.is_ascii_alphabetic()) || sounds_alike(from, to)
}

/// How many times a correction must be seen before it is applied.
fn times_needed(from: &str, to: &str) -> u32 {
    if misheard(from, to) {
        1
    } else {
        TIMES_TO_LEARN
    }
}

/// Whether two Chinese spellings sound alike in Mandarin: the same number
/// of syllables, each sharing a reading (any reading of a 破音字), counting
/// the pairs a Taiwanese accent and the recognizer mix up as one (zh/z,
/// ch/c, sh/s, n/l, -ng/-n), the way input methods' 模糊音 do.
fn sounds_alike(a: &str, b: &str) -> bool {
    let (x, y) = (syllables(a), syllables(b));
    match (x, y) {
        (Some(x), Some(y)) => {
            !x.is_empty()
                && x.len() == y.len()
                && x.iter()
                    .zip(&y)
                    .all(|(p, q)| p.iter().any(|r| q.contains(r)))
        }
        _ => false,
    }
}

/// Each Chinese character's readings (fuzzy, toneless); punctuation and
/// spaces are skipped. None if `s` has anything else (Latin letters).
fn syllables(s: &str) -> Option<Vec<Vec<String>>> {
    use pinyin::ToPinyinMulti;
    let mut out = Vec::new();
    for c in s.chars() {
        if let Some(readings) = c.to_pinyin_multi() {
            out.push(readings.into_iter().map(|p| fuzzy(p.plain())).collect());
        } else if c.is_alphanumeric() {
            return None;
        }
    }
    Some(out)
}

fn fuzzy(syllable: &str) -> String {
    let mut s = syllable.to_string();
    for (from, to) in [("zh", "z"), ("ch", "c"), ("sh", "s"), ("l", "n")] {
        if let Some(rest) = s.strip_prefix(from) {
            s = format!("{to}{rest}");
            break;
        }
    }
    for (from, to) in [("ang", "an"), ("eng", "en"), ("ing", "in")] {
        if let Some(stem) = s.strip_suffix(from) {
            s = format!("{stem}{to}");
            break;
        }
    }
    s
}

/// Count `found` into the store (`taught`: corrected in Moqi itself); returns
/// the corrections that just became active.
fn learn_into(
    store: &mut Store,
    found: &[(String, String)],
    now: f64,
    taught: bool,
) -> Vec<(String, String)> {
    let mut learned = Vec::new();
    for (from, to) in found {
        // The user changed one of our corrections back: stop applying it.
        if let Some(rule) = store
            .rules
            .iter_mut()
            .find(|r| r.active && r.from == *to && r.to == *from)
        {
            rule.active = false;
            rule.dismissed = true;
            rule.changed = now;
            continue;
        }
        let index = match store
            .rules
            .iter()
            .position(|r| r.from == *from && r.to == *to)
        {
            Some(i) => i,
            None => {
                store.rules.push(Rule {
                    from: from.clone(),
                    to: to.clone(),
                    count: 0,
                    active: false,
                    dismissed: false,
                    last_seen: now,
                    changed: now,
                });
                store.rules.len() - 1
            }
        };
        let rule = &mut store.rules[index];
        if rule.dismissed {
            // Removed once, but the user just taught it again on purpose.
            if !taught {
                continue;
            }
            rule.dismissed = false;
        }
        rule.count += 1;
        rule.last_seen = now;
        rule.changed = now;
        if rule.count >= times_needed(from, to) && !rule.active {
            rule.active = true;
            learned.push((from.clone(), to.clone()));
            // One correction per word: the newest wins.
            for other in store.rules.iter_mut() {
                if other.from == *from && other.to != *to && other.active {
                    other.active = false;
                    other.changed = now;
                }
            }
        }
    }
    learned
}

/// Everything learned, for the dictionary page: in use first, newest first.
pub fn rules(app: &AppHandle) -> Vec<Rule> {
    let mut rules: Vec<Rule> = with_store(app, |s| {
        (
            s.rules.iter().filter(|r| !r.dismissed).cloned().collect(),
            false,
        )
    });
    rules.sort_by(|x, y| {
        y.active
            .cmp(&x.active)
            .then(y.last_seen.total_cmp(&x.last_seen))
    });
    rules
}

/// Start applying a correction now, or forget it for good.
pub fn set_rule(app: &AppHandle, from: &str, to: &str, active: bool) {
    let now = now_ms();
    with_store(app, |store| {
        let mut changed = false;
        for rule in store.rules.iter_mut().filter(|r| r.from == from) {
            if rule.to == to {
                rule.active = active;
                rule.dismissed = !active;
                rule.changed = now;
                changed = true;
            } else if active && rule.active {
                rule.active = false;
                rule.changed = now;
            }
        }
        ((), changed)
    });
    super::sync::changed();
}

/// Every rule, removed ones included (sync needs them so a removal spreads).
pub fn all_rules(app: &AppHandle) -> Vec<Rule> {
    with_store(app, |s| (s.rules.clone(), false))
}

/// Replace every rule with the merged set from sync.
pub fn replace_rules(app: &AppHandle, rules: Vec<Rule>) {
    with_store(app, |s| {
        s.rules = rules;
        ((), true)
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pairs(a: &str, b: &str) -> Vec<(String, String)> {
        corrections(a, b)
    }
    fn pair(a: &str, b: &str) -> (String, String) {
        (a.to_string(), b.to_string())
    }

    #[test]
    fn finds_word_corrections() {
        assert_eq!(
            pairs("我們用蘇帕貝斯當資料庫", "我們用 Supabase 當資料庫"),
            vec![pair("蘇帕貝斯", "Supabase")]
        );
        assert_eq!(
            pairs("請用github登入", "請用 GitHub 登入"),
            vec![pair("github", "GitHub")]
        );
        assert_eq!(
            pairs("我想用 cloud code 寫", "我想用 Claude Code 寫"),
            vec![pair("cloud code", "Claude Code")]
        );
    }

    #[test]
    fn a_short_dictation_can_teach_a_term() {
        assert_eq!(
            pairs("打開蘇帕貝斯", "打開 Supabase"),
            vec![pair("蘇帕貝斯", "Supabase")]
        );
    }

    #[test]
    fn one_changed_character_keeps_its_neighbour() {
        assert_eq!(
            pairs("這周要交報告", "這週要交報告"),
            vec![pair("這周", "這週")]
        );
        assert_eq!(
            pairs("時辰很趕。", "時程很趕。"),
            vec![pair("時辰", "時程")]
        );
        // At the very start there is only the character after it.
        assert_eq!(pairs("周末見", "週末見"), vec![pair("周末", "週末")]);
    }

    #[test]
    fn ignores_what_is_not_a_correction() {
        // Punctuation only.
        assert!(pairs("好啊，沒問題。", "好啊，沒問題").is_empty());
        // Added or removed words.
        assert!(pairs("明天開會", "明天下午開會").is_empty());
        assert!(pairs("嗯明天開會", "明天開會").is_empty());
        // Rewritten.
        assert!(pairs("明天下午三點開會", "改到禮拜五再說吧").is_empty());
        // Unchanged.
        assert!(pairs("一樣的句子", "一樣的句子").is_empty());
    }

    #[test]
    fn edited_span_needs_the_surroundings_untouched() {
        let base = "Re: 好\n明天開會\n謝謝";
        assert_eq!(
            edited_span(base, "明天開會", "Re: 好\n明天下午開會\n謝謝").as_deref(),
            Some("明天下午開會")
        );
        assert_eq!(
            edited_span(base, "明天開會", "Re: 不好\n明天開會\n謝謝"),
            None
        );
        assert_eq!(
            edited_span("明天開會", "明天開會", "明天開").as_deref(),
            Some("明天開")
        );
        assert_eq!(normalize("a\r\nb\rc\u{a0}d"), "a\nb\nc d");
    }

    #[test]
    fn an_english_term_gets_spaces_next_to_chinese() {
        let rules = vec![pair("卡洛普", "Kalopp")];
        assert_eq!(
            apply_rules("請把報告交給卡洛普團隊。", &rules),
            "請把報告交給 Kalopp 團隊。"
        );
        // Already spaced, or next to punctuation: nothing added.
        assert_eq!(apply_rules("交給 卡洛普。", &rules), "交給 Kalopp。");
    }

    #[test]
    fn applies_without_breaking_words() {
        let rules = vec![pair("cloud", "Claude"), pair("這周", "這週")];
        assert_eq!(
            apply_rules("cloud 說這周會 cloudy", &rules),
            "Claude 說這週會 cloudy"
        );
        let grow = vec![pair("Claude", "Claude Code")];
        let once = apply_rules("用 Claude 寫", &grow);
        assert_eq!(once, "用 Claude Code 寫");
        assert_eq!(apply_rules(&once, &grow), once);
    }

    #[test]
    fn a_rewording_needs_two_and_stops_when_undone() {
        let mut store = Store::default();
        let found = vec![pair("明天", "後天")];
        assert!(learn_into(&mut store, &found, 1.0, false).is_empty());
        assert!(!store.rules[0].active);
        assert_eq!(learn_into(&mut store, &found, 2.0, false), found);
        assert!(store.rules[0].active);
        // The user changed it back: no longer applied, never relearned.
        learn_into(&mut store, &[pair("後天", "明天")], 3.0, false);
        assert!(!store.rules[0].active && store.rules[0].dismissed);
        learn_into(&mut store, &found, 4.0, false);
        learn_into(&mut store, &found, 5.0, false);
        assert!(!store.rules[0].active);
    }

    #[test]
    fn an_english_term_is_learned_the_first_time() {
        let mut store = Store::default();
        let found = vec![pair("蘇帕貝斯", "Supabase")];
        assert_eq!(learn_into(&mut store, &found, 1.0, false), found);
        assert!(store.rules[0].active);
    }

    #[test]
    fn a_misheard_chinese_word_is_learned_at_once_a_rewording_twice() {
        let mut store = Store::default();
        let misheard = vec![pair("實做", "實作")];
        assert_eq!(learn_into(&mut store, &misheard, 1.0, false), misheard);
        let reworded = vec![pair("但是", "可是")];
        assert!(learn_into(&mut store, &reworded, 2.0, true).is_empty());
        assert_eq!(learn_into(&mut store, &reworded, 3.0, true), reworded);
    }

    #[test]
    fn misheard_words_sound_alike_rewordings_do_not() {
        for (a, b) in [
            ("實做", "實作"),
            ("連接", "連結"),
            ("先定", "先訂"),
            ("太滑", "太花"),
            ("這周", "這週"),
            ("時辰", "時程"), // -n / -ng
            ("果他", "果它"),
            ("流覽", "瀏覽"),
        ] {
            assert!(sounds_alike(a, b), "{a} / {b}");
        }
        for (a, b) in [
            ("但是", "可是"),
            ("這樣", "真的"),
            ("出來", "出現得"),
            ("明天", "後天"),
            ("那個你", "欸你"),
            ("Cloud", "Claude"),
        ] {
            assert!(!sounds_alike(a, b), "{a} / {b}");
        }
    }

    #[test]
    fn only_misheard_terms_join_the_dictionary() {
        let learned = vec![
            pair("蘇帕貝斯", "Supabase"),
            pair("時辰", "時程"),
            pair("但是", "可是"),
            pair("周", "週"),
        ];
        assert_eq!(
            dictionary_words(&learned),
            vec!["Supabase".to_string(), "時程".to_string()]
        );
    }

    #[test]
    fn a_taught_correction_is_learned_again_even_if_removed_before() {
        let mut store = Store::default();
        let found = vec![pair("時辰", "時程")];
        assert_eq!(learn_into(&mut store, &found, 1.0, true), found);
        assert!(store.rules[0].active);
        // Removed on the dictionary page, then taught again on purpose.
        store.rules[0].active = false;
        store.rules[0].dismissed = true;
        assert_eq!(learn_into(&mut store, &found, 2.0, true), found);
        assert!(store.rules[0].active && !store.rules[0].dismissed);
    }

    #[test]
    fn an_english_term_replaces_any_spelling_that_sounds_like_the_taught_one() {
        let rules = vec![pair("深拓", "Zentro"), pair("時辰", "時程")];
        // A new spelling of the same sound.
        assert_eq!(
            apply_by_sound("請用申拓系統處理", &rules),
            "請用 Zentro 系統處理"
        );
        // Chinese-to-Chinese corrections stay exact: 實誠 is a real word.
        assert_eq!(apply_by_sound("他很實誠", &rules), "他很實誠");
        // Nothing that sounds different is touched.
        assert_eq!(apply_by_sound("請用深度系統", &rules), "請用深度系統");
    }

    #[test]
    fn a_dictionary_word_the_clean_up_put_in_is_not_learned_away() {
        let dictionary = vec!["Claude".to_string()];
        // Heard 卡洛普, the clean-up made it Claude, the user wrote Kalopp.
        assert_eq!(
            learnable(
                "請把報告交給 Claude 團隊。",
                "請把報告交給卡洛普團隊。",
                "請把報告交給 Kalopp 團隊。",
                &dictionary
            ),
            vec![pair("卡洛普", "Kalopp")]
        );
        // The clean-up's own changes (a removed filler, 然後 -> 接著) are not
        // the user's fix and are not learned from what was heard.
        assert_eq!(
            learnable(
                "接著我們用蘇帕貝斯當後端的資料庫。",
                "嗯然後我們用蘇帕貝斯當後端的資料庫。",
                "接著我們用 Supabase 當後端的資料庫。",
                &[]
            ),
            vec![pair("蘇帕貝斯", "Supabase")]
        );
    }

    #[test]
    fn terms_worth_a_dictionary_entry() {
        assert!(is_term("Supabase"));
        assert!(is_term("時程"));
        assert!(is_term("默契"));
        assert!(!is_term("週"));
        assert!(!is_term("，"));
    }

    #[test]
    fn the_newest_correction_of_a_word_wins() {
        let mut store = Store::default();
        for _ in 0..2 {
            learn_into(&mut store, &[pair("時辰", "時程")], 1.0, false);
        }
        for _ in 0..2 {
            learn_into(&mut store, &[pair("時辰", "時間")], 2.0, false);
        }
        let active: Vec<_> = store.rules.iter().filter(|r| r.active).collect();
        assert_eq!(active.len(), 1);
        assert_eq!(active[0].to, "時間");
    }
}
