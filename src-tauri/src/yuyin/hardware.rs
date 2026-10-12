//! Which speech model this computer can dictate with.
//!
//! Qwen3-ASR 1.7B, the engine M0 chose, needs a GPU. On the CPU of an Intel
//! i5-12400 it took about 6 s to recognise a 6–10 s sentence and 14 s for
//! 30 s (an RTX 4070: under 0.5 s), and laptop CPUs are slower still.
//! SenseVoice Small (Q4_K_M, 146 MB) recognised the same recordings on that
//! CPU in 0.4–0.7 s (30 s: 1.7 s). It is less accurate (M0: 14.4 % of
//! characters wrong against 12.0 %, English terms more often split), but it
//! keeps up with speech on any PC. So a Windows PC without a discrete GPU
//! with room for the 1.7B model gets SenseVoice, on the CPU. Integrated GPUs
//! are not given the 1.7B model: untested, and the CPU is the one device
//! every PC has. Measurements: docs/評測.md.
//!
//! Apple silicon always has Metal: macOS keeps the 1.7B model.

use log::info;
#[cfg(target_os = "windows")]
use log::warn;
use tauri::AppHandle;

use crate::settings::{self, TranscribeAcceleratorSetting};

/// The engine M0 chose, for a computer with a GPU it can use.
pub const FULL_MODEL: &str = "handy-computer/Qwen3-ASR-1.7B-gguf/Qwen3-ASR-1.7B-Q5_K_M.gguf";
/// The engine for a PC without one: fast on the CPU.
pub const LIGHT_MODEL: &str = "handy-computer/SenseVoiceSmall-gguf/SenseVoiceSmall-Q4_K_M.gguf";

/// GPU memory the 1.7B model runs in with room to spare (its weights are
/// 1.5 GB, plus working buffers).
#[cfg(any(target_os = "windows", test))]
const MIN_GPU_MEMORY: u64 = 4 * 1024 * 1024 * 1024;

/// Some discrete GPU has room for the 1.7B model. One that does not report
/// its memory gets the benefit of the doubt, as before this check.
#[cfg(any(target_os = "windows", test))]
fn has_capable_gpu(discrete_gpu_memory: &[u64]) -> bool {
    discrete_gpu_memory
        .iter()
        .any(|&bytes| bytes == 0 || bytes >= MIN_GPU_MEMORY)
}

/// The model this computer should dictate with. Lists the compute devices,
/// so call it after the transcription backend is initialised.
pub fn recommended_model() -> &'static str {
    #[cfg(target_os = "windows")]
    if !has_capable_gpu(&crate::managers::transcription::discrete_gpu_memory()) {
        return LIGHT_MODEL;
    }
    FULL_MODEL
}

/// The light model runs on the CPU: an integrated GPU would first spend
/// seconds building its kernels, and its drivers vary.
fn use_cpu(app: &AppHandle) {
    let mut s = settings::get_settings(app);
    if s.transcribe_accelerator == TranscribeAcceleratorSetting::Auto {
        s.transcribe_accelerator = TranscribeAcceleratorSetting::Cpu;
        settings::write_settings(app, s);
    }
}

/// For onboarding, which downloads and selects the returned model.
pub fn model_for_onboarding(app: &AppHandle) -> &'static str {
    let model = recommended_model();
    if model == LIGHT_MODEL {
        info!("no GPU the 1.7B speech model can use; onboarding gets {LIGHT_MODEL} on the CPU");
        use_cpu(app);
    }
    model
}

/// At launch, before the model is preloaded. An install from before this
/// check, on a PC without a GPU the 1.7B model can use, has that model and
/// waits seconds for every sentence. When the light model is already here,
/// select it now so the preload loads it; otherwise download it in the
/// background and switch once it is ready (the 1.7B model keeps working,
/// slowly, until then).
#[cfg(target_os = "windows")]
pub fn prepare_at_launch(app: &AppHandle) {
    use std::sync::Arc;
    use tauri::Manager;

    use crate::managers::model::ModelManager;

    let current = settings::get_settings(app);
    if !current.onboarding_completed || current.selected_model != FULL_MODEL {
        return;
    }
    if recommended_model() != LIGHT_MODEL {
        return;
    }
    let Some(models) = app
        .try_state::<Arc<ModelManager>>()
        .map(|m| m.inner().clone())
    else {
        return;
    };
    let downloaded = |models: &ModelManager| {
        models
            .get_model_info(LIGHT_MODEL)
            .is_some_and(|m| m.is_downloaded)
    };
    if downloaded(&models) {
        info!("no GPU the 1.7B speech model can use; switching to {LIGHT_MODEL}");
        use_cpu(app);
        let mut s = settings::get_settings(app);
        s.selected_model = LIGHT_MODEL.to_string();
        settings::write_settings(app, s);
        return;
    }
    info!("no GPU the 1.7B speech model can use; downloading {LIGHT_MODEL} to switch to it");
    let app = app.clone();
    std::thread::spawn(move || {
        if let Err(e) = tauri::async_runtime::block_on(models.download_model(LIGHT_MODEL)) {
            warn!("could not download the light speech model: {e}");
            return;
        }
        use_cpu(&app);
        // The launch preload may still be loading the 1.7B model.
        for _ in 0..120 {
            match crate::commands::models::switch_active_model(&app, LIGHT_MODEL) {
                Ok(()) => {
                    info!("switched to the light speech model");
                    return;
                }
                Err(e) if e.contains("already in progress") => {
                    std::thread::sleep(std::time::Duration::from_secs(1));
                }
                Err(e) => {
                    warn!("could not switch to the light speech model: {e}");
                    return;
                }
            }
        }
        warn!("could not switch to the light speech model: a model load kept running");
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    const GB: u64 = 1024 * 1024 * 1024;

    #[test]
    fn a_discrete_gpu_with_room_keeps_the_full_model() {
        assert!(has_capable_gpu(&[12 * GB]));
        assert!(has_capable_gpu(&[4 * GB]));
        // Memory not reported: as before this check.
        assert!(has_capable_gpu(&[0]));
        // Any one of several will do.
        assert!(has_capable_gpu(&[2 * GB, 8 * GB]));
    }

    #[test]
    fn no_gpu_or_a_small_one_gets_the_light_model() {
        // The CPU only, or an integrated GPU (not listed).
        assert!(!has_capable_gpu(&[]));
        // A 2 GB laptop GPU (MX-class) has no room for the 1.7B model.
        assert!(!has_capable_gpu(&[2 * GB]));
    }
}
