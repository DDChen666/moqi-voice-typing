//! Dictionary words for the recognizer itself.
//!
//! Qwen3-ASR takes a vocabulary list in its system turn and leans toward
//! those spellings (transcribe-cpp 0.3's `RunOptions::vocabulary`). That fixes
//! a name at the source, however the recognizer would have misheard it,
//! where a learned correction only catches the one misspelling it saw. On the
//! user's 30 M0 clips (1.1 branch, a patched 0.2.4 engine) the dictionary
//! took English terms from 71 % to 87 % right with no change in the wait;
//! 200 words made recognition worse and ~2 s slower, so at most
//! [`MAX_WORDS`] go in: the ones used most recently (taught, added, or heard
//! in a dictation), so the words the user needs now are always among them.
//!
//! The recognizer writes Simplified Chinese (Moqi converts afterwards), so
//! Chinese words are given to it in Simplified too.
//!
//! For evaluations: `YUYIN_ASR_VOCAB=0` turns it off; `YUYIN_ASR_VOCAB=<file>`
//! uses that file's words instead of the dictionary (one per line);
//! `YUYIN_ASR_VOCAB_AS_TYPED=1` skips the conversion to Simplified.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::RwLock;
use std::time::{SystemTime, UNIX_EPOCH};

use ferrous_opencc::{config::BuiltinConfig, OpenCC};
use log::warn;
use once_cell::sync::Lazy;
use tauri::{AppHandle, Manager};

use super::config;

pub const MAX_WORDS: usize = 50;
const FILE: &str = "yuyin_vocab_used.json";

/// When each dictionary word was last used, Unix milliseconds. Local to this
/// computer (not synced): it only orders what the recognizer is given.
static USED: Lazy<RwLock<Option<HashMap<String, f64>>>> = Lazy::new(|| RwLock::new(None));

static TO_SIMPLIFIED: Lazy<Option<OpenCC>> = Lazy::new(|| {
    OpenCC::from_config(BuiltinConfig::Tw2sp)
        .map_err(|e| warn!("No Simplified converter for the recognizer's words: {e}"))
        .ok()
});

/// The words to give the recognizer, most recently used first.
pub fn words(app: &AppHandle) -> Vec<String> {
    match std::env::var("YUYIN_ASR_VOCAB") {
        Ok(v) if v == "0" => Vec::new(),
        Ok(path) if !path.is_empty() => {
            let text = std::fs::read_to_string(&path).unwrap_or_default();
            prepare(
                text.lines()
                    .filter(|l| !l.starts_with('#'))
                    .map(str::to_string),
            )
        }
        _ => {
            let vocab = config::get(app).vocab;
            let used = with_used(app, |u| (u.clone(), false));
            prepare(ranked(&vocab, &used))
        }
    }
}

/// `vocab` most recently used first; never-used words keep their dictionary
/// order, newest added first.
fn ranked(vocab: &[String], used: &HashMap<String, f64>) -> Vec<String> {
    let mut order: Vec<(usize, &String)> = vocab.iter().enumerate().collect();
    order.sort_by(|(i, a), (j, b)| {
        let (ta, tb) = (used.get(*a).copied(), used.get(*b).copied());
        tb.unwrap_or(f64::MIN)
            .total_cmp(&ta.unwrap_or(f64::MIN))
            .then(j.cmp(i))
    });
    order.into_iter().map(|(_, w)| w.clone()).collect()
}

/// Trimmed, deduplicated, at most [`MAX_WORDS`], Chinese in Simplified.
fn prepare(words: impl IntoIterator<Item = String>) -> Vec<String> {
    let as_typed = std::env::var_os("YUYIN_ASR_VOCAB_AS_TYPED").is_some();
    let mut out: Vec<String> = Vec::new();
    for word in words {
        let word = if as_typed {
            word.trim().to_string()
        } else {
            simplified(word.trim())
        };
        if !word.is_empty() && listenable(&word) && !out.contains(&word) {
            out.push(word);
        }
        if out.len() == MAX_WORDS {
            break;
        }
    }
    out
}

/// Worth listening for. A lone letter ("x") makes the recognizer write it
/// for any short syllable, and a word half Chinese, half English ("的go",
/// learned from one changed character) is no term at all. Both stay in the
/// dictionary for the clean-up; only the recognizer doesn't get them.
fn listenable(word: &str) -> bool {
    let latin = word.chars().filter(|c| c.is_ascii_alphabetic()).count();
    let han = word.chars().any(is_han);
    let lone_letter = latin == 1 && word.chars().count() == 1;
    !lone_letter && !(han && latin > 0)
}

fn is_han(c: char) -> bool {
    ('\u{4e00}'..='\u{9fff}').contains(&c) || ('\u{3400}'..='\u{4dbf}').contains(&c)
}

/// Dictionary words in a row (nothing but spaces or punctuation between
/// them) that mean the recognizer recited its list instead of transcribing.
const RECITED_RUN: usize = 6;

/// Whether the recognizer recited `vocabulary` (the words it was given)
/// instead of the speech. Seen on 2026-10-08: 8 s of speech came back as
/// "首先 Grokbot bot grok x 视窗 Claude Claude Code LINE …", all 50 words in
/// the order given, and was pasted like that. People name a few terms in a
/// row ("React、Rust、Python"), but not six with nothing else between them.
pub fn recited(text: &str, vocabulary: &[String]) -> bool {
    longest_run(text, vocabulary) >= RECITED_RUN
}

/// The longest run of dictionary words in `text`, matching whole words
/// (not "bot" inside "robot") and the longest word at each place
/// ("Claude Code" over "Claude"), ignoring case.
fn longest_run(text: &str, vocabulary: &[String]) -> usize {
    let lower = |s: &str| -> Vec<char> { s.chars().flat_map(char::to_lowercase).collect() };
    let text = lower(text);
    let words: Vec<Vec<char>> = vocabulary
        .iter()
        .map(|w| lower(w.trim()))
        .filter(|w| !w.is_empty())
        .collect();
    let alnum = |c: Option<&char>| c.is_some_and(|c| c.is_ascii_alphanumeric());
    let fits = |at: usize, word: &[char]| {
        text.get(at..at + word.len()) == Some(word)
            && !(word[0].is_ascii_alphanumeric() && at > 0 && alnum(text.get(at - 1)))
            && !(word[word.len() - 1].is_ascii_alphanumeric() && alnum(text.get(at + word.len())))
    };
    let (mut best, mut run, mut i) = (0, 0, 0);
    while i < text.len() {
        let c = text[i];
        if c.is_whitespace() || (!c.is_alphanumeric() && !is_han(c)) {
            i += 1;
            continue;
        }
        match words.iter().filter(|w| fits(i, w)).map(Vec::len).max() {
            Some(len) => {
                run += 1;
                best = best.max(run);
                i += len;
            }
            None => {
                run = 0;
                i += 1;
            }
        }
    }
    best
}

fn simplified(word: &str) -> String {
    let han = word.chars().any(|c| ('\u{4e00}'..='\u{9fff}').contains(&c));
    match TO_SIMPLIFIED.as_ref() {
        Some(cc) if han => cc.convert(word),
        _ => word.to_string(),
    }
}

/// Mark `words` as used now: just taught or added, so they reach the
/// recognizer from the next dictation on.
pub fn touch(app: &AppHandle, words: &[String]) {
    if words.is_empty() {
        return;
    }
    let now = now_ms();
    with_used(app, |used| {
        for w in words {
            used.insert(w.trim().to_string(), now);
        }
        ((), true)
    });
}

/// After a dictation: dictionary words that appear in `text` were used.
pub fn note_output(app: &AppHandle, text: &str) {
    let vocab = config::get(app).vocab;
    let lower = text.to_lowercase();
    let heard: Vec<String> = vocab
        .into_iter()
        .filter(|w| !w.trim().is_empty() && lower.contains(&w.trim().to_lowercase()))
        .collect();
    touch(app, &heard);
}

fn store_path(app: &AppHandle) -> Option<PathBuf> {
    app.path().app_data_dir().ok().map(|d| d.join(FILE))
}

fn with_used<T>(app: &AppHandle, f: impl FnOnce(&mut HashMap<String, f64>) -> (T, bool)) -> T {
    let mut slot = USED.write().unwrap_or_else(|e| e.into_inner());
    let used = slot.get_or_insert_with(|| {
        store_path(app)
            .and_then(|p| std::fs::read_to_string(p).ok())
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    });
    let (out, save) = f(used);
    if save {
        if let Some(path) = store_path(app) {
            let json = serde_json::to_string(used).unwrap_or_default();
            if let Err(e) = std::fs::write(&path, json) {
                warn!("Failed to save {}: {e}", path.display());
            }
        }
    }
    out
}

fn now_ms() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as f64)
        .unwrap_or(0.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(words: &[&str]) -> Vec<String> {
        words.iter().map(|w| w.to_string()).collect()
    }

    #[test]
    fn recently_used_words_come_first() {
        let vocab = s(&["Claude", "GitHub", "Supabase", "默契"]);
        let used = HashMap::from([("Supabase".to_string(), 20.0), ("Claude".to_string(), 10.0)]);
        // Used ones by recency, then the rest newest added first.
        assert_eq!(
            ranked(&vocab, &used),
            s(&["Supabase", "Claude", "默契", "GitHub"])
        );
    }

    #[test]
    fn at_most_fifty_trimmed_unique_words() {
        let many: Vec<String> = (0..80).map(|i| format!(" w{i} ")).collect();
        let words = prepare(many);
        assert_eq!(words.len(), MAX_WORDS);
        assert_eq!(words[0], "w0");
        assert_eq!(prepare(s(&["", "  ", "API", "API"])), s(&["API"]));
    }

    #[test]
    fn lone_letters_and_half_chinese_words_are_not_listened_for() {
        assert_eq!(
            prepare(s(&["x", "X", "的go", "AI", "grok", "C++", "視窗", "3D"])),
            s(&["AI", "grok", "C++", "视窗", "3D"])
        );
    }

    #[test]
    fn a_recited_dictionary_is_caught() {
        // The 2026-10-08 dictation, exactly as the recognizer returned it.
        let vocab = s(&[
            "Grokbot",
            "bot",
            "grok",
            "x",
            "视窗",
            "Claude",
            "Claude Code",
            "LINE",
            "patreon",
            "YouTube",
            "Patreon",
            "Discord",
            "pixiv",
            "sol",
            "Gemini",
            "ChatGPT",
        ]);
        let out = "首先 Grokbot bot grok x 视窗 Claude Claude Code LINE patreon YouTube \
                   Patreon Discord pixiv sol Gemini ChatGPT";
        assert!(recited(out, &vocab));
        // After some real speech too.
        assert!(recited(&format!("我跟你确认一下，{out}"), &vocab));
        // The same afternoon, in another order. The words given to the
        // recognizer by then had no lone letters or half-Chinese words, so
        // "x" and "的 go" break the run here; what follows still counts.
        let vocab = s(&[
            "Grokbot",
            "bot",
            "Grok",
            "Claude",
            "sol",
            "mac",
            "Graphtreon",
            "patreon",
            "pixiv",
            "danbooru",
            "Bilibili",
            "YouTube",
            "Notion",
            "Keep",
            "Messenger",
            "LINE",
        ]);
        let out = "那关于 Grokbot bot Grok Claude x sol 的 go mac Graphtreon patreon pixiv \
                   danbooru Bilibili YouTube Notion Keep Messenger LINE";
        assert!(recited(out, &vocab));
    }

    #[test]
    fn naming_a_few_terms_is_not_reciting() {
        let vocab = s(&[
            "React",
            "Rust",
            "Python",
            "Tauri",
            "Claude",
            "Claude Code",
            "bot",
        ]);
        assert!(!recited("用 React、Rust、Python、Tauri 写一个 App", &vocab));
        assert!(!recited(
            "我要让 Claude Code 跟 Grokbot 那边建立流程",
            &vocab
        ));
        // Whole words only: "bot" in "robot" is not the dictionary's "bot".
        assert_eq!(
            longest_run("robot robot robot robot robot robot", &vocab),
            0
        );
        assert_eq!(longest_run("Claude Claude Code bot", &vocab), 3);
    }

    #[test]
    fn chinese_words_go_in_simplified() {
        assert_eq!(
            prepare(s(&["語音輸入法", "GitHub"])),
            s(&["语音输入法", "GitHub"])
        );
    }
}
