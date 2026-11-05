//! Speech-model warm-up: at launch on Windows, after a rest on both.
//!
//! ggml's Vulkan backend builds its GPU pipelines the first time each kernel
//! runs, and nothing keeps them between launches: on an RTX 4070 the first
//! transcription after every start took 16.9 s, the next ones 0.14 s for the
//! same 5 s of audio. Without a warm-up the user's first dictation of the day
//! would miss acceptance criterion 2 by fifteen seconds. So once the model is
//! loaded we transcribe two short synthetic clips in the background; a real
//! dictation during the warm-up simply waits for the engine as it would for
//! a load. Metal on macOS builds its pipelines fast and needs none of this.
//!
//! Warm kernels also go cold: after two idle days (with sleeps) the first
//! dictation took 4 s for 3 s of audio, the next ones about 0.1 s. So a press
//! after a long rest warms them again while the user speaks
//! ([`rewarm_if_idle`]).
//!
//! macOS needs that rewarm too, for another reason: under memory pressure it
//! swaps the idle model out. On a 16 GB MacBook Air the whole 2.2 GB
//! footprint was in swap four minutes after a dictation, and the first one
//! after a rest took 5–10 s to transcribe 3–5 s of audio (0.6 s otherwise).
//! Warming at the press pages the weights back in while the user speaks.
//! It still cost the first dictation after a rest seconds (the user: "every
//! time, even the first, should be fast"), so on macOS the model is also
//! kept warm: every [`KEEP_WARM_EVERY`] of rest it transcribes the short
//! clip in the background (about 0.3 s of GPU), which keeps its pages in
//! use and out of swap ([`keep_warm`]). It is loaded at launch too.

use std::f32::consts::TAU;
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime};

use log::{info, warn};

use crate::managers::model::ModelManager;
use crate::managers::transcription::TranscriptionManager;
use std::sync::Arc;
use tauri::{AppHandle, Manager};

/// Load the speech model at launch, which also warms it (see [`start`]).
/// Handy loads it on the first key press instead; with Vulkan that first
/// dictation after every restart would wait for the load and the kernel
/// build. The model stays loaded anyway (Moqi never unloads it), so this
/// only moves the wait to before the user needs it. Skipped until the model
/// has been downloaded (first-run onboarding loads it itself). On macOS the
/// load itself took the first dictation after every restart several seconds.
pub fn preload_at_launch(app: &AppHandle) {
    let selected = crate::settings::get_settings(app).selected_model;
    let downloaded = app
        .try_state::<Arc<ModelManager>>()
        .map(|models| {
            models
                .get_available_models()
                .iter()
                .any(|m| m.id == selected && m.is_downloaded)
        })
        .unwrap_or(false);
    if !downloaded {
        return;
    }
    if let Some(manager) = app.try_state::<Arc<TranscriptionManager>>() {
        info!("preloading the speech model at launch");
        manager.initiate_model_load();
    }
}

const SAMPLE_RATE: usize = 16_000;

/// Clip lengths in seconds. Different lengths reach different kernel
/// variants (prefill tiles, attention sizes); these two cover a short
/// sentence and a pause-delimited piece from the chunker.
const CLIPS: [f32; 2] = [1.5, 6.0];

/// A quiet voice-like signal: a 140 Hz buzz with a few harmonics, its
/// loudness rising and falling like syllables, plus a little noise. Enough
/// for the encoder and decoder to run their full paths; the text is ignored.
fn clip(seconds: f32) -> Vec<f32> {
    let n = (seconds * SAMPLE_RATE as f32) as usize;
    let mut noise: u32 = 0x1234_5678;
    (0..n)
        .map(|i| {
            let t = i as f32 / SAMPLE_RATE as f32;
            let syllables = 0.5 + 0.5 * (TAU * 4.0 * t).sin();
            let voice: f32 = (1..=4)
                .map(|h| (TAU * 140.0 * h as f32 * t).sin() / h as f32)
                .sum();
            noise ^= noise << 13;
            noise ^= noise >> 17;
            noise ^= noise << 5;
            let hiss = (noise as f32 / u32::MAX as f32 - 0.5) * 0.02;
            0.08 * syllables * voice + hiss
        })
        .collect()
}

/// When the engine was last warmed or the talk key last pressed. Wall-clock
/// time, so a night's sleep counts as idle.
static LAST_USE: Mutex<Option<SystemTime>> = Mutex::new(None);

/// How long the engine may rest before the next press warms it again.
#[cfg(not(target_os = "macos"))]
const IDLE_BEFORE_REWARM: Duration = Duration::from_secs(10 * 60);
/// Shorter on macOS: swapping starts within minutes. In the user's timing log
/// (258 dictations) none after a rest under 3 minutes took over 3 s to
/// transcribe; 5 % after 3–10 minutes did, 21 % after 10–60.
#[cfg(target_os = "macos")]
const IDLE_BEFORE_REWARM: Duration = Duration::from_secs(3 * 60);

/// Notes a use and says whether the rest before it was long enough to
/// rewarm. Unknown or backwards clocks never count as a rest.
fn note_use(now: SystemTime) -> bool {
    let mut last = LAST_USE.lock().unwrap_or_else(|e| e.into_inner());
    let rested = last
        .and_then(|t| now.duration_since(t).ok())
        .is_some_and(|idle| idle >= IDLE_BEFORE_REWARM);
    *last = Some(now);
    rested
}

/// Called when the talk key goes down: after a long rest, transcribe the
/// short clip in the background so the kernels are warm again by the time
/// the user lets go. A release before it finishes waits for the engine,
/// which it would have done anyway, cold.
pub fn rewarm_if_idle(manager: &TranscriptionManager) {
    if note_use(SystemTime::now()) && manager.is_model_loaded() {
        info!("model rested; warming it again while the user speaks");
        run(manager, &CLIPS[..1]);
    }
}

/// The rest's warm-up now, whatever the rest (`--transcribe-file` with
/// `YUYIN_WARMUP_FIRST`, to check a dictation waits for it).
pub fn rewarm_now(manager: &TranscriptionManager) {
    run(manager, &CLIPS[..1]);
}

/// Run the warm-up in the background; returns at once.
#[cfg(target_os = "windows")]
pub fn start(manager: &TranscriptionManager) {
    note_use(SystemTime::now());
    run(manager, &CLIPS);
}

/// How long the model may rest before [`keep_warm`] runs it. Under the 3 min
/// after which the user's dictations started coming out slow.
#[cfg(target_os = "macos")]
const KEEP_WARM_EVERY: Duration = Duration::from_secs(2 * 60);

/// Whether a rest since `last` is long enough for a keep-warm run.
#[cfg(any(target_os = "macos", test))]
fn due(last: Option<SystemTime>, now: SystemTime, every: Duration) -> bool {
    match last {
        None => true,
        Some(t) => now.duration_since(t).is_ok_and(|idle| idle >= every),
    }
}

/// Keep the loaded model out of swap (macOS): a background thread that runs
/// the short clip after every [`KEEP_WARM_EVERY`] of rest. Started once,
/// when the model is first loaded; it skips while the model is unloaded.
#[cfg(target_os = "macos")]
pub fn keep_warm(manager: &TranscriptionManager) {
    use std::sync::atomic::{AtomicBool, Ordering};
    static STARTED: AtomicBool = AtomicBool::new(false);
    if STARTED.swap(true, Ordering::SeqCst) {
        return;
    }
    let manager = manager.clone();
    let spawned = std::thread::Builder::new()
        .name("model-keep-warm".into())
        .spawn(move || loop {
            std::thread::sleep(Duration::from_secs(20));
            let now = SystemTime::now();
            let last = *LAST_USE.lock().unwrap_or_else(|e| e.into_inner());
            if !due(last, now, KEEP_WARM_EVERY) || !manager.is_model_loaded() {
                continue;
            }
            *LAST_USE.lock().unwrap_or_else(|e| e.into_inner()) = Some(now);
            let started = Instant::now();
            match super::asr_vocab::without(|| manager.transcribe(clip(CLIPS[0]))) {
                Ok(_) => log::debug!("model kept warm in {} ms", started.elapsed().as_millis()),
                Err(e) => warn!("keeping the model warm failed: {e}"),
            }
        });
    if let Err(e) = spawned {
        STARTED.store(false, Ordering::SeqCst);
        warn!("could not start keeping the model warm: {e}");
    }
}

fn run(manager: &TranscriptionManager, clips: &[f32]) {
    let manager = manager.clone();
    let clips = clips.to_vec();
    let spawned = std::thread::Builder::new()
        .name("model-warmup".into())
        .spawn(move || {
            for seconds in clips {
                let started = Instant::now();
                match super::asr_vocab::without(|| manager.transcribe(clip(seconds))) {
                    Ok(_) => info!(
                        "model warm-up: {seconds}s clip took {} ms",
                        started.elapsed().as_millis()
                    ),
                    Err(e) => {
                        warn!("model warm-up failed: {e}");
                        return;
                    }
                }
            }
        });
    if let Err(e) = spawned {
        warn!("could not start the model warm-up: {e}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clips_are_quiet_and_the_right_length() {
        let audio = clip(1.5);
        assert_eq!(audio.len(), 24_000);
        let peak = audio.iter().fold(0f32, |m, s| m.max(s.abs()));
        assert!(peak > 0.01 && peak < 0.5, "peak {peak}");
    }

    #[test]
    fn only_a_long_rest_rewarms() {
        let t0 = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000);
        note_use(t0);
        assert!(!note_use(t0 + Duration::from_secs(60)));
        assert!(note_use(t0 + Duration::from_secs(60) + IDLE_BEFORE_REWARM));
        // A clock that went backwards is not a rest.
        assert!(!note_use(t0));
    }

    #[test]
    fn keeps_warm_only_after_a_rest() {
        let every = Duration::from_secs(120);
        let t0 = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000);
        assert!(due(None, t0, every));
        assert!(!due(Some(t0), t0 + Duration::from_secs(119), every));
        assert!(due(Some(t0), t0 + every, every));
        // A clock that went backwards is not a rest.
        assert!(!due(Some(t0 + every), t0, every));
    }
}
