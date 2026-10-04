//! Learning from the user's own corrections (自動學詞, roadmap 1.1).
//!
//! After Moqi pastes, the field it typed into is read now and then (the same
//! `session::focused_field` as the field probe) until the user is done with
//! it. Words the user corrected in what Moqi typed — 蘇帕貝斯 → Supabase,
//! 這周 → 這週 — are counted; a correction seen twice is applied to later
//! dictations, before and after the clean-up, and a corrected English term
//! also joins the dictionary so the clean-up spells it the same way.
//!
//! Privacy (docs/隱私.md): off until the user turns it on
//! (`YuyinConfig::learn_from_edits`); the field's text is compared in memory
//! and dropped; only the corrected words are saved (`yuyin_learned.json`);
//! nothing is sent anywhere; password boxes are never read.

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
const EVERY: Duration = Duration::from_secs(1);
/// The paste must show up in the field this soon, or there is nothing to watch.
const SHOWS_WITHIN: Duration = Duration::from_secs(5);
/// Done once the text has stopped changing for this long after an edit.
const SETTLED: Duration = Duration::from_secs(6);
/// Nothing changed this long after the paste: nothing to learn.
const UNTOUCHED: Duration = Duration::from_secs(45);
const LONGEST: Duration = Duration::from_secs(180);
/// Focus is gone after this many unreadable looks in a row.
const MAX_MISSES: u32 = 3;
/// A correction made this many times is applied from then on.
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
    apply_rules(text, &rules)
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
            out.push_str(to);
            last = end;
        }
        search = end;
    }
    out.push_str(&text[last..]);
    out
}

// ---------------------------------------------------------------- watching

/// Call right after a successful paste of `pasted` into `front`.
pub fn after_paste(app: &AppHandle, front: Option<FrontApp>, pasted: String) {
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
        let found = corrections(&pasted, &edited);
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
    // Rewritten rather than corrected: more than 40 % of it removed.
    let removed = ops.iter().filter(|o| matches!(o, Op::Del(_))).count();
    if removed * 10 > a.len() * 4 {
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
    let terms = with_store(app, |store| {
        let terms = learn_into(store, found, now_ms());
        (terms, true)
    });
    super::sync::changed();
    if terms.is_empty() {
        return;
    }
    // A learned English term also goes into the dictionary, so the clean-up
    // keeps the same spelling.
    let mut cfg = config::get(app);
    let mut added = false;
    for term in terms {
        if !cfg.vocab.iter().any(|w| w.eq_ignore_ascii_case(&term)) {
            cfg.vocab.push(term);
            added = true;
        }
    }
    if added {
        if let Err(e) = config::set(app, cfg) {
            warn!("Failed to add a learned term to the dictionary: {e}");
        }
    }
}

/// Count `found` into the store; returns the English terms that just became
/// active (for the dictionary).
fn learn_into(store: &mut Store, found: &[(String, String)], now: f64) -> Vec<String> {
    let mut terms = Vec::new();
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
            continue;
        }
        rule.count += 1;
        rule.last_seen = now;
        rule.changed = now;
        if rule.count >= TIMES_TO_LEARN && !rule.active {
            rule.active = true;
            if to.chars().any(|c| c.is_ascii_alphabetic()) {
                terms.push(to.clone());
            }
            // One correction per word: the newest wins.
            for other in store.rules.iter_mut() {
                if other.from == *from && other.to != *to && other.active {
                    other.active = false;
                    other.changed = now;
                }
            }
        }
    }
    terms
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
    fn learns_after_two_and_stops_when_undone() {
        let mut store = Store::default();
        let found = vec![pair("蘇帕貝斯", "Supabase")];
        assert!(learn_into(&mut store, &found, 1.0).is_empty());
        assert!(!store.rules[0].active);
        assert_eq!(
            learn_into(&mut store, &found, 2.0),
            vec!["Supabase".to_string()]
        );
        assert!(store.rules[0].active);
        // The user changed it back: no longer applied, never relearned.
        learn_into(&mut store, &[pair("Supabase", "蘇帕貝斯")], 3.0);
        assert!(!store.rules[0].active && store.rules[0].dismissed);
        learn_into(&mut store, &found, 4.0);
        learn_into(&mut store, &found, 5.0);
        assert!(!store.rules[0].active);
    }

    #[test]
    fn the_newest_correction_of_a_word_wins() {
        let mut store = Store::default();
        for _ in 0..2 {
            learn_into(&mut store, &[pair("時辰", "時程")], 1.0);
        }
        for _ in 0..2 {
            learn_into(&mut store, &[pair("時辰", "時間")], 2.0);
        }
        let active: Vec<_> = store.rules.iter().filter(|r| r.active).collect();
        assert_eq!(active.len(), 1);
        assert_eq!(active[0].to, "時間");
    }
}
