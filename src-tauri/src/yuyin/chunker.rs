//! Transcribe while the user is still speaking (M1 step 2b).
//!
//! Qwen3-ASR has no streaming mode, so a 25-second instruction used to be
//! transcribed only after the key was released: about 2 s of waiting on top
//! of the clean-up (criterion 2). Instead, each time the VAD reports a real
//! pause after enough speech, the speech so far is transcribed on a
//! background thread. On release only the tail after the last pause is left.
//!
//! - Pieces end in silence, so no word is cut in half. Pauses are found in
//!   the VAD's frame-by-frame decision, before its 450 ms hangover.
//! - A piece ends at the next pause between sentences (0.75 s). Past 8 s it
//!   ends at the next breath (0.25 s) instead, so the last piece, the one
//!   release waits for, stays short. Cutting at every breath was faster
//!   still, but split clauses and cost words ("人類社會" → "人類周圍");
//!   see `M0_引擎盲測/results/chunk_v4*` (`tools/chunk_eval.py`).
//! - The recorder still hands back the whole recording on release, for
//!   history and the WAV file. We only use it to find the tail.
//! - Anything unexpected (a piece failed, the lengths disagree, a piece takes
//!   too long) falls back to transcribing the whole recording in one go, as
//!   before, and that gets a second try if it fails. Nothing is ever dropped.

use std::sync::mpsc;
use std::sync::{Arc, Condvar, Mutex, OnceLock};
use std::time::{Duration, Instant};

use anyhow::Result;
use log::{debug, info, warn};
use once_cell::sync::Lazy;
use tauri::{AppHandle, Manager};

use crate::managers::transcription::TranscriptionManager;

const SAMPLE_RATE: usize = 16_000;
/// A tail this short holds no word (the VAD adds 450 ms of pre-roll to any
/// speech), only the resampler flush at release.
const MIN_TAIL_SAMPLES: usize = SAMPLE_RATE / 5;

/// When to cut a piece, in samples. `tools/chunk_eval.py` tries other values
/// through `YUYIN_CHUNK_*_MS` environment variables, without a rebuild.
struct Tuning {
    /// Unvoiced audio that counts as a pause.
    pause: usize,
    /// Shorter pieces lose too much context for the model; wait for more.
    min_piece: usize,
    /// Past this, a breath will do: people who speak without long pauses
    /// would otherwise leave everything to the release.
    long_piece: usize,
    long_pause: usize,
}

static TUNING: Lazy<Tuning> = Lazy::new(|| {
    let ms = |name: &str, default: usize| {
        std::env::var(name)
            .ok()
            .and_then(|v| v.parse::<usize>().ok())
            .unwrap_or(default)
            * SAMPLE_RATE
            / 1000
    };
    Tuning {
        pause: ms("YUYIN_CHUNK_PAUSE_MS", 750),
        min_piece: ms("YUYIN_CHUNK_MIN_PIECE_MS", 4_000),
        long_piece: ms("YUYIN_CHUNK_LONG_PIECE_MS", 8_000),
        long_pause: ms("YUYIN_CHUNK_LONG_PAUSE_MS", 250),
    }
});
/// How long release waits for background pieces before transcribing the
/// whole recording instead.
const WAIT_LIMIT: Duration = Duration::from_secs(20);

#[derive(Default)]
struct State {
    /// Bumped on every recording; background results for an older one are
    /// thrown away.
    generation: u64,
    /// Taking frames: between `begin` and release.
    active: bool,
    /// What the recorder kept so far (speech plus VAD padding).
    samples: Vec<f32>,
    /// `samples[..committed]` has been handed to the background thread.
    committed: usize,
    /// Unvoiced samples since the last voiced frame.
    quiet: usize,
    /// A voiced frame came after the last cut: the tail holds speech, and
    /// the next pause may cut again.
    voiced_since_cut: bool,
    /// One slot per piece, filled in by the background thread.
    pieces: Vec<Option<std::result::Result<String, String>>>,
    /// How long each piece took, for the replay check.
    piece_ms: Vec<Option<u128>>,
}

static STATE: Lazy<Mutex<State>> = Lazy::new(|| Mutex::new(State::default()));
static PIECE_DONE: Condvar = Condvar::new();

struct Job {
    generation: u64,
    index: usize,
    audio: Vec<f32>,
}

static WORKER: OnceLock<Mutex<mpsc::Sender<Job>>> = OnceLock::new();

fn worker(app: &AppHandle) -> mpsc::Sender<Job> {
    WORKER
        .get_or_init(|| {
            let (tx, rx) = mpsc::channel::<Job>();
            let app = app.clone();
            std::thread::Builder::new()
                .name("yuyin-chunker".into())
                .spawn(move || run_worker(app, rx))
                .expect("spawn chunker thread");
            Mutex::new(tx)
        })
        .lock()
        .expect("chunker sender")
        .clone()
}

fn run_worker(app: AppHandle, rx: mpsc::Receiver<Job>) {
    for job in rx {
        if STATE.lock().map(|s| s.generation).unwrap_or(0) != job.generation {
            continue; // cancelled or superseded before we got to it
        }
        let started = Instant::now();
        let seconds = job.audio.len() as f64 / SAMPLE_RATE as f64;
        let tm = app.state::<Arc<TranscriptionManager>>();
        let result = tm.transcribe(job.audio).map_err(|e| e.to_string());
        debug!(
            "chunker: piece {} ({:.1}s of audio) done in {:?}",
            job.index,
            seconds,
            started.elapsed()
        );
        let elapsed = started.elapsed().as_millis();
        if let Ok(mut s) = STATE.lock() {
            if s.generation == job.generation {
                if let Some(slot) = s.pieces.get_mut(job.index) {
                    *slot = Some(result);
                }
                if let Some(slot) = s.piece_ms.get_mut(job.index) {
                    *slot = Some(elapsed);
                }
            }
        }
        PIECE_DONE.notify_all();
    }
}

/// Before capture starts. `enabled` is false for models with their own
/// streaming, without VAD (no pauses to find), or when the model is unloaded
/// after every transcription.
pub fn begin(app: &AppHandle, enabled: bool) {
    if enabled {
        let _ = worker(app); // start the thread outside the audio path
    }
    if let Ok(mut s) = STATE.lock() {
        let generation = s.generation + 1;
        *s = State {
            generation,
            active: enabled,
            ..State::default()
        };
    }
}

/// Audio the recorder keeps (audio thread: keep it cheap).
pub fn on_speech(frame: &[f32]) {
    let Ok(mut s) = STATE.lock() else { return };
    if !s.active {
        return;
    }
    s.samples.extend_from_slice(frame);
}

/// Every frame's VAD decision before smoothing, after the recorder has kept
/// or dropped it (audio thread: keep it cheap).
pub fn on_vad(voiced: bool, len: usize) {
    let job = {
        let Ok(mut s) = STATE.lock() else { return };
        if !s.active {
            return;
        }
        if voiced {
            s.quiet = 0;
            s.voiced_since_cut = true;
            return;
        }
        s.quiet += len;
        let pending = s.samples.len() - s.committed;
        let t = &*TUNING;
        let needed = if pending >= t.long_piece {
            t.long_pause
        } else {
            t.pause
        };
        if !s.voiced_since_cut || pending < t.min_piece || s.quiet < needed {
            return;
        }
        let (start, end) = (s.committed, s.samples.len());
        s.committed = end;
        s.voiced_since_cut = false;
        s.pieces.push(None);
        s.piece_ms.push(None);
        Job {
            generation: s.generation,
            index: s.pieces.len() - 1,
            audio: s.samples[start..end].to_vec(),
        }
    };
    debug!(
        "chunker: pause after {:.1}s of speech, transcribing piece {}",
        job.audio.len() as f64 / SAMPLE_RATE as f64,
        job.index
    );
    if let Some(tx) = WORKER.get() {
        if let Ok(tx) = tx.lock() {
            let _ = tx.send(job);
        }
    }
}

/// Stop taking frames and forget this recording (tap, cancel, empty audio).
pub fn abandon() {
    if let Ok(mut s) = STATE.lock() {
        s.generation += 1;
        s.active = false;
        s.samples = Vec::new();
        s.pieces.clear();
    }
    PIECE_DONE.notify_all();
}

/// How the text for one recording was put together, for the timings log.
#[derive(Clone, Copy, Debug, Default)]
pub struct Stats {
    /// Pieces transcribed while the user was speaking.
    pub pieces: usize,
    /// Audio left to transcribe after release.
    pub tail_samples: usize,
}

/// On release: the text of the whole recording. Waits for the background
/// pieces, transcribes the tail and joins them; otherwise transcribes
/// `samples` in one go.
pub fn transcribe(tm: &TranscriptionManager, samples: Vec<f32>) -> (Result<String>, Stats) {
    let whole = |samples: Vec<f32>| {
        let tail_samples = samples.len();
        (
            transcribe_retrying(tm, samples),
            Stats {
                pieces: 0,
                tail_samples,
            },
        )
    };

    let (generation, committed, count, tail_has_voice) = {
        let Ok(mut s) = STATE.lock() else {
            return whole(samples);
        };
        s.active = false;
        if s.pieces.is_empty() {
            return whole(samples);
        }
        if s.samples.len() != samples.len() {
            warn!(
                "chunker: kept {} samples but the recorder returned {}; transcribing all",
                s.samples.len(),
                samples.len()
            );
            return whole(samples);
        }
        (
            s.generation,
            s.committed,
            s.pieces.len(),
            s.voiced_since_cut,
        )
    };

    let texts = match wait_for_pieces(generation) {
        Some(texts) => texts,
        None => return whole(samples),
    };

    let tail = &samples[committed..];
    // A pause just before release leaves only silence after the last cut;
    // the model would make up a word ("嗯。") for it.
    let tail_text = if tail_has_voice && tail.len() >= MIN_TAIL_SAMPLES {
        match tm.transcribe(tail.to_vec()) {
            Ok(text) => text,
            Err(e) => {
                warn!("chunker: tail failed ({e}); transcribing all");
                return whole(samples);
            }
        }
    } else {
        String::new()
    };

    let stats = Stats {
        pieces: count,
        tail_samples: tail.len(),
    };
    info!(
        "chunker: {} pieces while speaking, {:.1}s tail after release",
        count,
        tail.len() as f64 / SAMPLE_RATE as f64
    );
    let mut parts = texts;
    parts.push(tail_text);
    (Ok(join(&parts)), stats)
}

/// The whole recording, given a second try: one failed run must not lose
/// the dictation. If the engine was dropped (it panicked), it is loaded
/// again first; `transcribe` waits for the load.
fn transcribe_retrying(tm: &TranscriptionManager, samples: Vec<f32>) -> Result<String> {
    match tm.transcribe(samples.clone()) {
        Ok(text) => Ok(text),
        Err(e) => {
            warn!("chunker: transcription failed ({e}); trying once more");
            if !tm.is_model_loaded() {
                tm.initiate_model_load();
            }
            tm.transcribe(samples)
        }
    }
}

/// Pieces cut so far in this recording (the replay check).
pub fn piece_count() -> usize {
    STATE.lock().map(|s| s.pieces.len()).unwrap_or(0)
}

/// How long each piece took to transcribe, once done (the replay check).
pub fn piece_times_ms() -> Vec<Option<u128>> {
    STATE.lock().map(|s| s.piece_ms.clone()).unwrap_or_default()
}

/// Block until every piece handed to the background thread is done (or
/// [`WAIT_LIMIT`] passes). Only the replay check needs this.
pub fn wait_for_background() {
    let deadline = Instant::now() + WAIT_LIMIT;
    let Ok(mut s) = STATE.lock() else { return };
    while !s.pieces.iter().all(Option::is_some) {
        let now = Instant::now();
        if now >= deadline {
            return;
        }
        match PIECE_DONE.wait_timeout(s, deadline - now) {
            Ok((guard, _)) => s = guard,
            Err(_) => return,
        }
    }
}

/// The text of every piece, in order, once all are done. `None` if one
/// failed, the recording was abandoned, or they took longer than
/// [`WAIT_LIMIT`].
fn wait_for_pieces(generation: u64) -> Option<Vec<String>> {
    let deadline = Instant::now() + WAIT_LIMIT;
    let mut s = STATE.lock().ok()?;
    loop {
        if s.generation != generation {
            return None;
        }
        if s.pieces.iter().all(Option::is_some) {
            let texts: std::result::Result<Vec<String>, String> =
                s.pieces.drain(..).flatten().collect();
            s.samples = Vec::new();
            return match texts {
                Ok(texts) => Some(texts),
                Err(e) => {
                    warn!("chunker: a piece failed ({e}); transcribing all");
                    None
                }
            };
        }
        let now = Instant::now();
        if now >= deadline {
            warn!("chunker: pieces took over {WAIT_LIMIT:?}; transcribing all");
            return None;
        }
        s = PIECE_DONE.wait_timeout(s, deadline - now).ok()?.0;
    }
}

/// Join piece texts. Chinese needs no separator; two Latin words (or a
/// sentence end and a Latin word) need a space.
///
/// The model ends every piece with 。 because the audio ends there, but a
/// pause is often mid-sentence ("其實在我看來都。沒有辦法"). Between pieces
/// it becomes ，; a real sentence end reads fine either way (M0 replay,
/// `tools/chunk_eval.py`).
fn join(parts: &[String]) -> String {
    let mut out = String::new();
    // A piece that ends 吗。 ends a question: keep it one (吗？), not ，.
    let parts: Vec<String> = parts
        .iter()
        .map(|p| super::spacing::question_marks(p.trim()))
        .collect();
    for part in parts.iter().map(String::as_str).filter(|p| !p.is_empty()) {
        if out.ends_with('。') {
            out.pop();
            out.push('，');
        }
        if let (Some(a), Some(b)) = (out.chars().last(), part.chars().next()) {
            let latin_end =
                a.is_ascii_alphanumeric() || matches!(a, '.' | ',' | '!' | '?' | ';' | ':');
            if latin_end && b.is_ascii_alphanumeric() {
                out.push(' ');
            }
        }
        out.push_str(part);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(parts: &[&str]) -> Vec<String> {
        parts.iter().map(|p| p.to_string()).collect()
    }

    #[test]
    fn joins_chinese_with_commas_at_the_cuts() {
        assert_eq!(
            join(&s(&["我先講第一段。", "然後第二段。"])),
            "我先講第一段，然後第二段。"
        );
        // Questions keep their mark.
        assert_eq!(
            join(&s(&["你有空嗎？", "我們去吃拉麵。"])),
            "你有空嗎？我們去吃拉麵。"
        );
    }

    #[test]
    fn joins_latin_words_with_a_space() {
        assert_eq!(
            join(&s(&["幫我開 GitHub", "repo 的設定"])),
            "幫我開 GitHub repo 的設定"
        );
        assert_eq!(
            join(&s(&["Run the tests.", "Then commit."])),
            "Run the tests. Then commit."
        );
        assert_eq!(join(&s(&["用 React", "寫"])), "用 React寫");
    }

    #[test]
    fn a_question_at_a_cut_keeps_its_question_mark() {
        assert_eq!(
            join(&s(&["可以传到你那边吗。", "我想确认"])),
            "可以传到你那边吗？我想确认"
        );
    }

    #[test]
    fn skips_empty_parts() {
        assert_eq!(join(&s(&["第一段。", "  ", ""])), "第一段。");
        assert_eq!(join(&s(&["", "第二段。"])), "第二段。");
        assert_eq!(join(&s(&[])), "");
    }

    /// Feed the hooks the way the recorder does and check where pieces are
    /// cut, with the default tuning. Uses the shared state, so everything
    /// runs in one test.
    #[test]
    fn cuts_only_at_pauses_after_enough_speech() {
        let frame = 480; // Silero: 30 ms
        let second = vec![0.1f32; SAMPLE_RATE];
        let speak = |seconds: usize| {
            for _ in 0..seconds {
                on_speech(&second);
                on_vad(true, frame);
            }
        };
        // Unvoiced frames the recorder dropped.
        let pause = |ms: usize| {
            for _ in 0..(SAMPLE_RATE * ms / 1000 / frame) {
                on_vad(false, frame);
            }
        };
        let quiet_frame = vec![0.0f32; frame];
        // Unvoiced frames the VAD hangover still kept.
        let hangover = |ms: usize| {
            for _ in 0..(SAMPLE_RATE * ms / 1000 / frame) {
                on_speech(&quiet_frame);
                on_vad(false, frame);
            }
        };
        let reset = || {
            let mut st = STATE.lock().unwrap();
            let generation = st.generation + 1;
            *st = State {
                generation,
                active: true,
                ..State::default()
            };
        };

        // 2 s of speech, then a long pause: too short to cut.
        reset();
        speak(2);
        pause(600);
        assert_eq!(STATE.lock().unwrap().pieces.len(), 0);

        // 3 more seconds (5 s total), then a breath: still one piece of speech.
        speak(3);
        pause(300);
        assert_eq!(STATE.lock().unwrap().pieces.len(), 0);

        // The gap goes on into a pause between sentences: all 5 s are cut, once.
        pause(480);
        {
            let st = STATE.lock().unwrap();
            assert_eq!(st.pieces.len(), 1);
            assert_eq!(st.committed, 5 * SAMPLE_RATE);
            assert!(!st.voiced_since_cut, "the tail after it is silence");
        }
        pause(2000);
        assert_eq!(STATE.lock().unwrap().pieces.len(), 1);

        // Past 8 s, a breath will do; a shorter gap still won't.
        speak(9);
        pause(150);
        assert_eq!(STATE.lock().unwrap().pieces.len(), 1);
        pause(150);
        {
            let st = STATE.lock().unwrap();
            assert_eq!(st.pieces.len(), 2);
            assert_eq!(st.committed, 14 * SAMPLE_RATE);
        }

        // A breath the hangover keeps as speech still counts: the cut lands
        // on the ninth quiet frame (270 ms).
        speak(9);
        hangover(400);
        {
            let st = STATE.lock().unwrap();
            assert_eq!(st.pieces.len(), 3);
            assert_eq!(st.committed, 23 * SAMPLE_RATE + 9 * frame);
        }

        // After abandon, frames are ignored.
        abandon();
        on_speech(&second);
        assert!(STATE.lock().unwrap().samples.is_empty());
    }
}
