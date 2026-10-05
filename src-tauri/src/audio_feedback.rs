use crate::settings::SoundTheme;
use crate::settings::{self, AppSettings};
use cpal::traits::{DeviceTrait, HostTrait};
use log::{debug, error, warn};
use rodio::OutputStreamBuilder;
use std::collections::HashMap;
use std::fs::File;
use std::io::BufReader;
use std::path::{Path, PathBuf};
use std::sync::{mpsc, Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use once_cell::sync::Lazy;
use tauri::{AppHandle, Manager};

pub enum SoundType {
    Start,
    Stop,
}

fn resolve_sound_path(
    app: &AppHandle,
    settings: &AppSettings,
    sound_type: SoundType,
) -> Option<PathBuf> {
    let sound_file = get_sound_path(settings, sound_type);
    let base_dir = get_sound_base_dir(settings);
    match base_dir {
        tauri::path::BaseDirectory::AppData => {
            crate::portable::resolve_app_data(app, &sound_file).ok()
        }
        _ => app.path().resolve(&sound_file, base_dir).ok(),
    }
}

fn get_sound_path(settings: &AppSettings, sound_type: SoundType) -> String {
    match (settings.sound_theme, sound_type) {
        (SoundTheme::Custom, SoundType::Start) => "custom_start.wav".to_string(),
        (SoundTheme::Custom, SoundType::Stop) => "custom_stop.wav".to_string(),
        (_, SoundType::Start) => settings.sound_theme.to_start_path(),
        (_, SoundType::Stop) => settings.sound_theme.to_stop_path(),
    }
}

fn get_sound_base_dir(settings: &AppSettings) -> tauri::path::BaseDirectory {
    match settings.sound_theme {
        SoundTheme::Custom => tauri::path::BaseDirectory::AppData,
        _ => tauri::path::BaseDirectory::Resource,
    }
}

pub fn play_feedback_sound(app: &AppHandle, sound_type: SoundType) {
    let settings = settings::get_settings(app);
    if !settings.audio_feedback {
        return;
    }
    if let Some(path) = resolve_sound_path(app, &settings, sound_type) {
        play_sound_async(app, path);
    }
}

pub fn play_feedback_sound_blocking(app: &AppHandle, sound_type: SoundType) {
    let settings = settings::get_settings(app);
    if !settings.audio_feedback {
        return;
    }
    if let Some(path) = resolve_sound_path(app, &settings, sound_type) {
        play_sound_blocking(app, &path);
    }
}

pub fn play_test_sound(app: &AppHandle, sound_type: SoundType) {
    let settings = settings::get_settings(app);
    if let Some(path) = resolve_sound_path(app, &settings, sound_type) {
        play_sound_blocking(app, &path);
    }
}

fn play_sound_async(app: &AppHandle, path: PathBuf) {
    let app_handle = app.clone();
    thread::spawn(move || {
        if let Err(e) = play_sound_at_path(&app_handle, path.as_path()) {
            error!("Failed to play sound '{}': {}", path.display(), e);
        }
    });
}

fn play_sound_blocking(app: &AppHandle, path: &Path) {
    if let Err(e) = play_sound_at_path(app, path) {
        error!("Failed to play sound '{}': {}", path.display(), e);
    }
}

fn play_sound_at_path(app: &AppHandle, path: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let settings = settings::get_settings(app);
    let volume = settings.audio_feedback_volume;
    let selected_device = settings.selected_output_device.clone();
    play_audio_file(path, selected_device, volume)
}

// Yuyin fork: one player thread keeps the output stream open while the user
// is dictating, with the sounds decoded once into memory.
//
// Handy opened a new output stream for every sound and decoded the WAV from
// disk inside the audio callback. The stop sound plays the moment the user
// releases the key, which is when recognition starts and, after a rest, when
// macOS pages the speech model back in from swap: a callback waiting on the
// disk misses its deadline and the "dun" crackles, now and then and mostly
// the first time after a rest. Now the stream opened for the start sound is
// still open for the stop sound, nothing is read from disk while it plays,
// and the stream closes 30 s after the last sound.

/// How long the output stream stays open after the last sound.
const STREAM_KEEP_ALIVE: Duration = Duration::from_secs(30);
/// How much longer than its length a sound may take before the stream is
/// taken for dead.
const PLAY_GRACE: Duration = Duration::from_secs(1);

struct PlayRequest {
    path: PathBuf,
    device: Option<String>,
    volume: f32,
    /// Answered when the sound has finished (blocking plays).
    done: Option<mpsc::Sender<Result<(), String>>>,
}

/// A sound decoded into memory.
#[derive(Clone)]
struct Decoded {
    channels: u16,
    rate: u32,
    samples: Arc<Vec<f32>>,
}

/// The output stream kept open between sounds.
struct OpenStream {
    /// The device asked for (None or "Default": the system's default output).
    device: Option<String>,
    /// For the default output, the device that was when the stream opened.
    default_name: Option<String>,
    output: rodio::OutputStream,
}

fn follows_default(device: &Option<String>) -> bool {
    matches!(device.as_deref(), None | Some("Default"))
}

/// The system's default output device, as rodio's default stream opens it.
fn default_output_name() -> Option<String> {
    cpal::default_host().default_output_device()?.name().ok()
}

static PLAYER: Lazy<Mutex<Option<mpsc::Sender<PlayRequest>>>> = Lazy::new(|| Mutex::new(None));

fn player() -> Option<mpsc::Sender<PlayRequest>> {
    let mut slot = PLAYER.lock().unwrap_or_else(|e| e.into_inner());
    if slot.is_none() {
        let (tx, rx) = mpsc::channel::<PlayRequest>();
        let spawned = thread::Builder::new()
            .name("feedback-sound".into())
            .spawn(move || run_player(rx));
        if let Err(e) = spawned {
            error!("Failed to start the feedback sound player: {}", e);
            return None;
        }
        *slot = Some(tx);
    }
    slot.clone()
}

fn run_player(rx: mpsc::Receiver<PlayRequest>) {
    let mut stream: Option<OpenStream> = None;
    let mut decoded: HashMap<PathBuf, Decoded> = HashMap::new();
    loop {
        let request = match rx.recv_timeout(STREAM_KEEP_ALIVE) {
            Ok(request) => request,
            Err(mpsc::RecvTimeoutError::Timeout) => {
                if stream.take().is_some() {
                    debug!("Closed the feedback sound stream after a rest");
                }
                continue;
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => return,
        };
        let result = play_request(&request, &mut stream, &mut decoded);
        if let Err(e) = &result {
            error!("Failed to play sound '{}': {}", request.path.display(), e);
            // A stream that failed may be stale (device unplugged): reopen next time.
            stream = None;
        }
        if let Some(done) = request.done {
            let _ = done.send(result.map(|_| ()));
        }
    }
}

fn play_request(
    request: &PlayRequest,
    stream: &mut Option<OpenStream>,
    decoded: &mut HashMap<PathBuf, Decoded>,
) -> Result<(), String> {
    // The system's default output may have changed since the stream was
    // opened (headphones plugged in): sounds go where the system's do.
    let default_name = if follows_default(&request.device) {
        default_output_name()
    } else {
        None
    };
    let current = stream
        .as_ref()
        .is_some_and(|s| s.device == request.device && s.default_name == default_name);
    if !current {
        *stream = None;
        let builder = output_stream_builder(request.device.clone()).map_err(|e| e.to_string())?;
        let opened = builder.open_stream().map_err(|e| e.to_string())?;
        *stream = Some(OpenStream {
            device: request.device.clone(),
            default_name,
            output: opened,
        });
    }
    let Some(OpenStream { output, .. }) = stream.as_ref() else {
        return Err("no output stream".into());
    };
    let sound = match decoded.get(&request.path) {
        Some(sound) => sound.clone(),
        None => {
            let sound = decode(&request.path)?;
            decoded.insert(request.path.clone(), sound.clone());
            sound
        }
    };
    let sink = rodio::Sink::connect_new(output.mixer());
    sink.set_volume(request.volume);
    sink.append(rodio::buffer::SamplesBuffer::new(
        sound.channels,
        sound.rate,
        sound.samples.as_ref().clone(),
    ));
    if request.done.is_some() {
        // Never wait longer than the sound lasts: a stream whose device went
        // away (headphones unplugged) stops taking samples, and waiting for
        // the sink to drain would hang this thread, silencing every sound
        // after it until Moqi restarts. Failing here reopens the stream.
        let frames = sound.samples.len() as f64 / f64::from(sound.channels.max(1));
        let lasts = Duration::from_secs_f64(frames / f64::from(sound.rate.max(1)));
        let deadline = Instant::now() + lasts + PLAY_GRACE;
        while !sink.empty() {
            if Instant::now() >= deadline {
                sink.stop();
                return Err("the output stream stopped playing".into());
            }
            thread::sleep(Duration::from_millis(10));
        }
    } else {
        sink.detach();
    }
    Ok(())
}

fn decode(path: &Path) -> Result<Decoded, String> {
    use rodio::Source;
    let file = File::open(path).map_err(|e| e.to_string())?;
    let decoder = rodio::Decoder::new(BufReader::new(file)).map_err(|e| e.to_string())?;
    let channels = decoder.channels();
    let rate = decoder.sample_rate();
    let samples: Vec<f32> = decoder.collect();
    Ok(Decoded {
        channels,
        rate,
        samples: Arc::new(samples),
    })
}

fn output_stream_builder(
    selected_device: Option<String>,
) -> Result<OutputStreamBuilder, Box<dyn std::error::Error>> {
    Ok(if let Some(device_name) = selected_device {
        if device_name == "Default" {
            debug!("Using default device");
            OutputStreamBuilder::from_default_device()?
        } else {
            let host = crate::audio_toolkit::get_cpal_host();
            let devices = host.output_devices()?;

            let mut found_device = None;
            for device in devices {
                if device.name()? == device_name {
                    found_device = Some(device);
                    break;
                }
            }

            match found_device {
                Some(device) => OutputStreamBuilder::from_device(device)?,
                None => {
                    warn!("Device '{}' not found, using default device", device_name);
                    OutputStreamBuilder::from_default_device()?
                }
            }
        }
    } else {
        debug!("Using default device");
        OutputStreamBuilder::from_default_device()?
    })
}

fn play_audio_file(
    path: &std::path::Path,
    selected_device: Option<String>,
    volume: f32,
) -> Result<(), Box<dyn std::error::Error>> {
    // Every play here is synchronous for its caller (the async ones already
    // run on their own thread), so the start sound still ends before the
    // microphone opens.
    let Some(player) = player() else {
        return Err("feedback sound player unavailable".into());
    };
    let (done, finished) = mpsc::channel();
    player
        .send(PlayRequest {
            path: path.to_path_buf(),
            device: selected_device,
            volume,
            done: Some(done),
        })
        .map_err(|e| e.to_string())?;
    finished
        .recv_timeout(Duration::from_secs(10))
        .map_err(|e| e.to_string())?
        .map_err(|e| e.into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_bundled_sounds_decode_into_memory() {
        for name in ["marimba_start.wav", "marimba_stop.wav"] {
            let path = Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("resources")
                .join(name);
            let sound = decode(&path).expect(name);
            assert_eq!((sound.channels, sound.rate), (2, 44_100), "{name}");
            // About half a second or more of stereo audio.
            assert!(sound.samples.len() > 40_000, "{name}");
        }
    }
}
