//! The home page's numbers and each history entry's details, read from the
//! local timings log (`yuyin_timings.jsonl`, written by [`super::session`]).
//! Everything is computed on this machine.

use std::collections::{BTreeMap, HashMap};

use chrono::{Datelike, Duration, Local, NaiveDate, TimeZone};
use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::AppHandle;

/// "Time saved" compares speaking with typing at this many characters a
/// minute (a typical Zhuyin typist), and says so on the home page.
pub const TYPING_CHARS_PER_MINUTE: u64 = 40;
/// The activity grid shows this many weeks.
const WEEKS: i64 = 26;

/// One line of the timings log; only the fields the UI needs. Older lines
/// lack the newer fields, so all of them default.
#[derive(Deserialize, Default)]
#[serde(default)]
struct Record {
    at: i64,
    context: String,
    level: Option<String>,
    polish: String,
    press_to_release_ms: Option<u64>,
    /// Audio kept after silence is trimmed; missing on the oldest lines.
    audio_ms: Option<u64>,
    release_to_output_ms: Option<u64>,
    chars_in: u64,
    chars_out: u64,
    app: String,
    sent_chars: Option<u64>,
    sent_to: Option<String>,
    file_name: Option<String>,
    /// What a recording transcribed again from history sent
    /// (`session::note_retry_sent`): counts as sent, not as a dictation.
    #[serde(default)]
    retry: bool,
}

impl Record {
    /// How much text went to which clean-up service. Lines written before
    /// `sent_chars` was logged fall back to the transcript length whenever a
    /// clean-up was attempted (DeepSeek was the only service then), so the
    /// privacy card never under-reports.
    fn sent(&self) -> (u64, Option<String>) {
        match self.sent_chars {
            Some(chars) => (chars, self.sent_to.clone()),
            None if self.polish == "ok" || self.polish == "failed" => {
                (self.chars_in, Some("api.deepseek.com".to_string()))
            }
            None => (0, None),
        }
    }
}

#[derive(Serialize, Type, Debug, PartialEq)]
pub struct DayCount {
    /// `YYYY-MM-DD`, local time.
    pub date: String,
    pub dictations: u32,
}

#[derive(Serialize, Type, Debug, PartialEq)]
pub struct Privacy {
    /// Always zero: recognition runs on this machine. Reported rather than
    /// hard-coded in the UI so the claim is visibly backed by the log.
    pub audio_uploaded_ms: u64,
    /// Always zero: only a style label ("chat") leaves the machine.
    pub app_names_sent: u64,
    /// Characters of transcript sent to the clean-up service.
    pub text_sent_chars: u64,
    /// Hosts text was sent to, e.g. `api.deepseek.com`.
    pub sent_to: Vec<String>,
}

#[derive(Serialize, Type, Debug, PartialEq)]
pub struct Stats {
    pub dictations: u32,
    /// Characters pasted.
    pub chars: u64,
    /// Time spent holding the key.
    pub speaking_ms: u64,
    /// Typing the same text at [`TYPING_CHARS_PER_MINUTE`], minus speaking.
    pub saved_ms: u64,
    pub chars_per_minute: u32,
    pub active_days: u32,
    pub current_streak: u32,
    pub longest_streak: u32,
    /// The last [`WEEKS`] weeks, oldest first, starting on a Sunday.
    pub days: Vec<DayCount>,
    pub privacy: Privacy,
}

/// What the history page shows under an entry.
#[derive(Serialize, Type, Debug, Clone, PartialEq)]
pub struct EntryMeta {
    pub app: String,
    pub context: String,
    pub level: Option<String>,
    pub polish: String,
    pub sent_chars: u64,
    pub sent_to: Option<String>,
    pub spoke_ms: Option<u64>,
    pub output_ms: Option<u64>,
}

fn read_records(app: &AppHandle) -> Vec<Record> {
    let Some(path) = super::session::timings_path(app) else {
        return Vec::new();
    };
    let Ok(text) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    parse(&text)
}

fn parse(text: &str) -> Vec<Record> {
    text.lines()
        .filter_map(|line| serde_json::from_str(line).ok())
        .collect()
}

fn local_date(at_ms: i64) -> Option<NaiveDate> {
    Local
        .timestamp_millis_opt(at_ms)
        .single()
        .map(|t| t.date_naive())
}

/// Drop a history entry the user deleted from the timings log, so the home
/// page stops counting it. Entries that age out of the 100-entry history keep
/// their line: they still happened.
pub fn forget(app: &AppHandle, file_name: &str) {
    let Some(path) = super::session::timings_path(app) else {
        return;
    };
    let _guard = super::session::TIMINGS_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let Ok(text) = std::fs::read_to_string(&path) else {
        return;
    };
    let kept = without_entry(&text, file_name);
    if kept.len() == text.len() {
        return;
    }
    let tmp = path.with_extension("jsonl.tmp");
    let result = std::fs::write(&tmp, kept).and_then(|_| std::fs::rename(&tmp, &path));
    if let Err(e) = result {
        log::warn!("Failed to drop a deleted entry from the timings log: {e}");
    }
}

/// The log without the lines for `file_name`. Lines that don't parse stay.
fn without_entry(text: &str, file_name: &str) -> String {
    text.lines()
        .filter(|line| {
            serde_json::from_str::<Record>(line)
                .map_or(true, |r| r.file_name.as_deref() != Some(file_name))
        })
        .fold(String::new(), |mut out, line| {
            out.push_str(line);
            out.push('\n');
            out
        })
}

pub fn stats(app: &AppHandle) -> Stats {
    compute(&read_records(app), Local::now().date_naive(), local_date)
}

pub fn history_meta(app: &AppHandle) -> HashMap<String, EntryMeta> {
    meta_by_file(read_records(app))
}

fn meta_by_file(records: Vec<Record>) -> HashMap<String, EntryMeta> {
    let (retries, dictations): (Vec<Record>, Vec<Record>) =
        records.into_iter().partition(|r| r.retry);
    let mut meta: HashMap<String, EntryMeta> = dictations
        .into_iter()
        .filter_map(|r| {
            let file_name = r.file_name.clone()?;
            let (sent_chars, sent_to) = r.sent();
            Some((
                file_name,
                EntryMeta {
                    app: r.app,
                    context: r.context,
                    level: r.level,
                    polish: r.polish,
                    sent_chars,
                    sent_to,
                    spoke_ms: r.press_to_release_ms,
                    output_ms: r.release_to_output_ms,
                },
            ))
        })
        .collect();
    // A recording transcribed again sent its text once more: add it to the
    // entry, which has no record of its own when the dictation had failed.
    for r in retries {
        let Some(file_name) = r.file_name.clone() else {
            continue;
        };
        let (chars, to) = r.sent();
        meta.entry(file_name)
            .and_modify(|m| {
                m.sent_chars += chars;
                if m.sent_to.is_none() {
                    m.sent_to = to.clone();
                }
            })
            .or_insert(EntryMeta {
                app: String::new(),
                context: r.context,
                level: r.level,
                polish: r.polish,
                sent_chars: chars,
                sent_to: to,
                spoke_ms: None,
                output_ms: None,
            });
    }
    meta
}

fn compute(
    records: &[Record],
    today: NaiveDate,
    date_of: impl Fn(i64) -> Option<NaiveDate>,
) -> Stats {
    // Everything that was sent counts toward privacy; only dictations count
    // as dictations.
    let sent: Vec<(u64, Option<String>)> = records.iter().map(Record::sent).collect();
    let dictations: Vec<&Record> = records.iter().filter(|r| !r.retry).collect();
    let records = dictations.as_slice();
    let chars: u64 = records.iter().map(|r| r.chars_out).sum();
    // Speech, not key-down time: a recording left running (a stuck key, a
    // hands-free session nobody ended) would otherwise drag the speed down.
    let speaking_ms: u64 = records
        .iter()
        .filter_map(|r| r.audio_ms.or(r.press_to_release_ms))
        .sum();
    let typing_ms = chars * 60_000 / TYPING_CHARS_PER_MINUTE;
    let chars_per_minute = if speaking_ms > 0 {
        (chars * 60_000 / speaking_ms) as u32
    } else {
        0
    };

    let mut per_day: BTreeMap<NaiveDate, u32> = BTreeMap::new();
    for r in records {
        if let Some(d) = date_of(r.at) {
            *per_day.entry(d).or_default() += 1;
        }
    }

    let mut longest = 0u32;
    let mut run = 0u32;
    let mut previous: Option<NaiveDate> = None;
    for d in per_day.keys() {
        run = match previous {
            Some(p) if *d - p == Duration::days(1) => run + 1,
            _ => 1,
        };
        longest = longest.max(run);
        previous = Some(*d);
    }
    // A streak still counts today before the first dictation of the day.
    let mut current = 0u32;
    let mut day = if per_day.contains_key(&today) {
        today
    } else {
        today - Duration::days(1)
    };
    while per_day.contains_key(&day) {
        current += 1;
        day -= Duration::days(1);
    }

    // Weeks run Sunday to Saturday; the grid ends with this week.
    let this_sunday = today - Duration::days(today.weekday().num_days_from_sunday() as i64);
    let first = this_sunday - Duration::weeks(WEEKS - 1);
    let days = (0..WEEKS * 7)
        .map(|i| first + Duration::days(i))
        .filter(|d| *d <= today)
        .map(|d| DayCount {
            date: d.format("%Y-%m-%d").to_string(),
            dictations: per_day.get(&d).copied().unwrap_or(0),
        })
        .collect();

    let mut sent_to: Vec<String> = sent.iter().filter_map(|(_, to)| to.clone()).collect();
    sent_to.sort();
    sent_to.dedup();

    Stats {
        dictations: records.len() as u32,
        chars,
        speaking_ms,
        saved_ms: typing_ms.saturating_sub(speaking_ms),
        chars_per_minute,
        active_days: per_day.len() as u32,
        current_streak: current,
        longest_streak: longest,
        days,
        privacy: Privacy {
            audio_uploaded_ms: 0,
            app_names_sent: 0,
            text_sent_chars: sent.iter().map(|(chars, _)| chars).sum(),
            sent_to,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn day(s: &str) -> NaiveDate {
        NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
    }

    /// Records whose `at` is a day number, so tests pick dates directly.
    fn rec(at_day: &str, chars: u64, spoke_ms: u64, sent: u64) -> Record {
        Record {
            at: day(at_day)
                .and_hms_opt(12, 0, 0)
                .unwrap()
                .and_utc()
                .timestamp_millis(),
            chars_out: chars,
            press_to_release_ms: Some(spoke_ms),
            sent_chars: Some(sent),
            sent_to: (sent > 0).then(|| "api.deepseek.com".to_string()),
            ..Default::default()
        }
    }

    fn date_of_utc(at: i64) -> Option<NaiveDate> {
        chrono::DateTime::from_timestamp_millis(at).map(|t| t.date_naive())
    }

    fn retry(file: &str, sent: u64) -> Record {
        Record {
            retry: true,
            context: "other".into(),
            polish: "retry".into(),
            file_name: Some(file.into()),
            ..rec("2026-09-28", 0, 0, sent)
        }
    }

    #[test]
    fn a_retry_counts_as_sent_not_as_a_dictation() {
        let records = vec![rec("2026-09-28", 100, 30_000, 100), retry("a.wav", 40)];
        let s = compute(&records, day("2026-09-28"), date_of_utc);
        assert_eq!(s.dictations, 1);
        assert_eq!(s.chars, 100);
        assert_eq!(s.privacy.text_sent_chars, 140);
    }

    #[test]
    fn a_retry_shows_under_its_entry() {
        let dictated = Record {
            file_name: Some("ok.wav".into()),
            app: "Code".into(),
            ..rec("2026-09-28", 20, 3_000, 20)
        };
        let meta = meta_by_file(vec![dictated, retry("ok.wav", 20), retry("failed.wav", 35)]);
        // Retried after a dictation: both sends, the dictation's details.
        assert_eq!(meta["ok.wav"].sent_chars, 40);
        assert_eq!(meta["ok.wav"].app, "Code");
        // A failed dictation has no record of its own: the retry is it.
        assert_eq!(meta["failed.wav"].sent_chars, 35);
        assert_eq!(
            meta["failed.wav"].sent_to.as_deref(),
            Some("api.deepseek.com")
        );
        assert_eq!(meta["failed.wav"].context, "other");
    }

    #[test]
    fn totals_speed_and_saved_time() {
        let records = vec![
            rec("2026-09-27", 120, 40_000, 120),
            rec("2026-09-28", 48, 20_000, 0),
        ];
        let s = compute(&records, day("2026-09-28"), date_of_utc);
        assert_eq!(s.chars, 168);
        assert_eq!(s.speaking_ms, 60_000);
        assert_eq!(s.chars_per_minute, 168);
        // Typing 168 characters at 40/min takes 252 s; speaking took 60 s.
        assert_eq!(s.saved_ms, 192_000);
        assert_eq!(s.privacy.text_sent_chars, 120);
        assert_eq!(s.privacy.sent_to, vec!["api.deepseek.com".to_string()]);
        assert_eq!(s.privacy.audio_uploaded_ms, 0);
    }

    #[test]
    fn streaks_and_grid() {
        let records = vec![
            rec("2026-09-20", 1, 1, 0),
            rec("2026-09-21", 1, 1, 0),
            rec("2026-09-22", 1, 1, 0),
            rec("2026-09-26", 1, 1, 0),
            rec("2026-09-27", 1, 1, 0),
            rec("2026-09-27", 1, 1, 0),
        ];
        // Monday 2026-09-28, nothing yet today: the streak ending yesterday counts.
        let s = compute(&records, day("2026-09-28"), date_of_utc);
        assert_eq!(s.active_days, 5);
        assert_eq!(s.longest_streak, 3);
        assert_eq!(s.current_streak, 2);
        // 25 full weeks plus this week's Sunday and Monday.
        assert_eq!(s.days.len(), 25 * 7 + 2);
        assert_eq!(s.days.first().unwrap().date, "2026-04-05");
        let last = s.days.last().unwrap();
        assert_eq!((last.date.as_str(), last.dictations), ("2026-09-28", 0));
        let sunday = &s.days[s.days.len() - 2];
        assert_eq!((sunday.date.as_str(), sunday.dictations), ("2026-09-27", 2));
    }

    #[test]
    fn old_lines_without_new_fields_still_count() {
        let text = r#"{"at":1790598013964,"context":"to_ai","level":"tidy","polish":"ok","press_to_release_ms":11475,"chars_out":68}
not json
{"at":1790598060223,"context":"to_ai","polish":"ok","chars_out":135}"#;
        let records = parse(text);
        assert_eq!(records.len(), 2);
        assert_eq!(records[0].chars_out, 68);
        assert!(records[1].file_name.is_none());
    }

    #[test]
    fn old_lines_count_the_text_they_sent() {
        let text = r#"{"at":1,"level":"tidy","polish":"ok","chars_in":68,"chars_out":66}
{"at":2,"level":"raw","polish":"skipped","chars_in":20,"chars_out":20}
{"at":3,"level":"tidy","polish":"skipped","chars_in":30,"chars_out":30,"sent_chars":0}
{"at":4,"level":"tidy","polish":"ok","chars_in":40,"chars_out":40,"sent_chars":40,"sent_to":"api.example.com"}"#;
        let s = compute(&parse(text), day("2026-09-28"), date_of_utc);
        assert_eq!(s.privacy.text_sent_chars, 108);
        assert_eq!(
            s.privacy.sent_to,
            vec![
                "api.deepseek.com".to_string(),
                "api.example.com".to_string()
            ]
        );
    }

    #[test]
    fn forgetting_an_entry_drops_only_its_line() {
        let log = concat!(
            r#"{"at":1,"chars_out":5,"file_name":"a.wav"}"#,
            "\n",
            r#"{"at":2,"chars_out":7,"file_name":"b.wav"}"#,
            "\n",
            "not json\n",
        );
        let kept = without_entry(log, "a.wav");
        assert!(!kept.contains("a.wav"));
        assert!(kept.contains("b.wav"));
        assert!(kept.contains("not json"));
        assert_eq!(without_entry(log, "missing.wav"), log);
    }

    #[test]
    fn a_long_silent_recording_does_not_drag_the_speed_down() {
        let mut stuck = rec("2026-09-29", 60, 600_000, 0);
        stuck.audio_ms = Some(20_000);
        let normal = rec("2026-09-29", 100, 30_000, 0);
        let stats = compute(&[stuck, normal], day("2026-09-29"), date_of_utc);
        // 160 chars over 50 s of speech, not over 630 s of key-down.
        assert_eq!(stats.chars_per_minute, 192);
    }
}
