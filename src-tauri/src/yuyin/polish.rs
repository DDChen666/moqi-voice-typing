//! LLM clean-up of a transcript, set up the way the M0 evaluation validated:
//! DeepSeek flash with reasoning off, temperature 0, prompt v3 as the system
//! message and the transcript fenced in the user message.
//!
//! Never loses a draft: any failure returns the raw transcript, and the caller
//! records the outcome.

use std::time::Duration;

use log::{debug, warn};
use once_cell::sync::Lazy;
use serde_json::json;
use tauri::AppHandle;

use super::config::{self, Level, YuyinConfig};
use super::context::Context;
use super::prompt;
use super::secrets;
use super::session::{self, PolishOutcome};

/// One client for the whole app so the TLS connection is reused; creating a
/// client per request (as upstream does) costs a handshake every dictation.
static CLIENT: Lazy<reqwest::Client> = Lazy::new(|| {
    reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(3))
        .pool_idle_timeout(Duration::from_secs(90))
        .user_agent(concat!("Yuyin/", env!("CARGO_PKG_VERSION")))
        .build()
        .unwrap_or_else(|_| reqwest::Client::new())
});

/// OpenRouter's API, whose requests take a few extra fields (see `run`).
pub const OPENROUTER_HOST: &str = "openrouter.ai";

fn is_openrouter(cfg: &YuyinConfig) -> bool {
    host(&cfg.base_url) == OPENROUTER_HOST
}

/// OpenRouter models that refused to run with reasoning turned off; they get
/// the lowest effort instead for the rest of the session.
static MUST_REASON: Lazy<std::sync::Mutex<std::collections::HashSet<String>>> =
    Lazy::new(Default::default);

/// A model on this computer (Ollama, LM Studio). Both take OpenAI's
/// `reasoning_effort`; without "none", Gemma 4 E2B on Ollama reasons before
/// it answers: 8.8 s for one sentence instead of 0.6 s (M3, 2026-10-01).
fn is_local(cfg: &YuyinConfig) -> bool {
    is_loopback(&host(&cfg.base_url))
}
const LOCAL_REASONING: &str = "reasoning_effort";

fn endpoint(cfg: &YuyinConfig, path: &str) -> String {
    let base = cfg.base_url.trim().trim_end_matches('/');
    // "openrouter.ai/api/v1" typed without a scheme: HTTPS, as the host shown
    // in the capsule and history already assumes.
    if base.contains("://") {
        format!("{base}{path}")
    } else {
        format!("https://{base}{path}")
    }
}

/// The host whose key this configuration uses and where its text would go,
/// or `None` when nothing is sent: no service, or a custom service with no
/// address yet. Does not look at the level.
pub fn key_host(cfg: &YuyinConfig) -> Option<String> {
    if cfg.service == config::Service::None {
        return None;
    }
    Some(host(&cfg.base_url)).filter(|h| !h.is_empty())
}

/// A model server on this computer: its text never leaves, and it needs no
/// key (Ollama, LM Studio).
pub fn is_loopback(host: &str) -> bool {
    let name = if let Some(rest) = host.strip_prefix('[') {
        rest.split(']').next().unwrap_or(rest)
    } else {
        host.rsplit_once(':').map_or(host, |(name, _)| name)
    };
    name.eq_ignore_ascii_case("localhost")
        || name
            .parse::<std::net::IpAddr>()
            .is_ok_and(|ip| ip.is_loopback())
}

/// The key for this configuration's service, if the clean-up will run: not
/// at 原話, and not for a custom service still missing its model name. A
/// model on this computer runs without one (an empty key sends no header).
fn usable_key(cfg: &YuyinConfig) -> Option<String> {
    if cfg.level == Level::Raw || cfg.model.trim().is_empty() {
        return None;
    }
    let host = key_host(cfg)?;
    secrets::api_key(&host).or_else(|| is_loopback(&host).then(String::new))
}

/// Open the connection while the user is still speaking, so the request after
/// release doesn't pay for DNS + TLS. Sends no transcript content, and nothing
/// at all when the clean-up won't run (原話, 全程本機, or no key).
pub fn warm_up(app: &AppHandle) {
    let cfg = config::get(app);
    let Some(key) = usable_key(&cfg) else {
        return;
    };
    if is_local(&cfg) {
        load_local_model(&cfg);
        return;
    }
    let url = endpoint(&cfg, "/models");
    tauri::async_runtime::spawn(async move {
        let mut request = CLIENT.get(url).timeout(Duration::from_secs(5));
        if !key.is_empty() {
            request = request.bearer_auth(key);
        }
        let result = request.send().await;
        if let Err(e) = result {
            debug!("LLM connection warm-up failed: {e}");
        }
    });
}

/// Ollama unloads a model after five idle minutes, and loading Gemma 4 E2B
/// again takes 6.6 s (0.55 s once loaded; M3, 2026-10-01). Asking for one
/// token when the talk key goes down loads it while the user speaks. Nothing
/// of the user's is in the request, and it never leaves this computer.
fn load_local_model(cfg: &YuyinConfig) {
    let url = endpoint(cfg, "/chat/completions");
    let body = json!({
        "model": cfg.model,
        "messages": [{"role": "user", "content": "ok"}],
        "max_tokens": 1,
        "stream": false,
        LOCAL_REASONING: "none",
    });
    tauri::async_runtime::spawn(async move {
        let result = CLIENT
            .post(url)
            .timeout(Duration::from_secs(30))
            .json(&body)
            .send()
            .await;
        if let Err(e) = result {
            debug!("local model warm-up failed: {e}");
        }
    });
}

/// Clean up `transcript` for the current session's context. Returns the text
/// to paste: the polished version, or the transcript itself when the level is
/// Raw or anything goes wrong.
pub async fn polish(app: &AppHandle, transcript: &str) -> String {
    let extras = session::extras();
    let cfg = effective(&config::get(app), &extras);
    let context = session::context();
    let (text, outcome) = match run(&cfg, context, &extras, transcript).await {
        Ok(Some(text)) => (taiwan_weeks(app, &extras, text), PolishOutcome::Ok),
        Ok(None) => (transcript.to_string(), PolishOutcome::Skipped),
        Err(e) => {
            warn!("Polish failed, pasting the raw transcript: {e}");
            (transcript.to_string(), PolishOutcome::Failed)
        }
    };
    session::mark_polished(cfg.level, outcome, text.chars().count());
    text
}

/// The model writes 這周 where Taiwan writes 這週 (wording.rs). Only for
/// Traditional Chinese output that isn't being translated: 周 is correct in
/// Simplified Chinese and in Japanese.
fn taiwan_weeks(app: &AppHandle, extras: &session::Extras, text: String) -> String {
    let traditional = crate::settings::get_settings(app).selected_language == "zh-Hant";
    if traditional && extras.translate_to.is_none() {
        super::wording::weeks(&text)
    } else {
        text
    }
}

/// For the settings panel's test button: same request as a real dictation
/// (context Other), but errors are reported instead of silently falling back.
/// Tests the service even while the level is 原話, which never calls it.
pub async fn test(app: &AppHandle, text: &str) -> Result<String, String> {
    let mut cfg = config::get(app);
    if cfg.level == Level::Raw {
        cfg.level = Level::Tidy;
    }
    Ok(run(&cfg, Context::Other, &session::Extras::default(), text)
        .await?
        .unwrap_or_else(|| text.to_string()))
}

/// The configuration a dictation really uses: translating, or editing a
/// selection, needs the clean-up service even at 原話.
pub fn effective(cfg: &YuyinConfig, extras: &session::Extras) -> YuyinConfig {
    let mut cfg = cfg.clone();
    if cfg.level == Level::Raw && (extras.translate_to.is_some() || extras.selection.is_some()) {
        cfg.level = Level::Tidy;
    }
    cfg
}

/// Whether a clean-up service will be asked (on this computer or not).
pub fn will_run(cfg: &YuyinConfig) -> bool {
    usable_key(cfg).is_some()
}

/// `Ok(None)`: nothing to do (Raw level, no service, blank transcript).
async fn run(
    cfg: &YuyinConfig,
    context: Context,
    extras: &session::Extras,
    transcript: &str,
) -> Result<Option<String>, String> {
    if transcript.trim().is_empty() {
        return Ok(None);
    }
    let translate_to = extras.translate_to.as_deref();
    // Editing a selection, or cleaning up what was said. A translation or an
    // edit may change the length a lot, so only plain clean-ups get the
    // length guard.
    let (system, user, guarded) = match extras.selection.as_deref() {
        Some(selected) => (
            prompt::edit_system(&cfg.vocab, &extras.note, translate_to),
            prompt::edit_message(selected, transcript),
            false,
        ),
        None => {
            let Some(system) = prompt::system_prompt_for(
                cfg.level,
                context,
                &cfg.vocab,
                &extras.note,
                translate_to,
            ) else {
                return Ok(None);
            };
            (
                system,
                prompt::user_message(transcript),
                translate_to.is_none(),
            )
        }
    };
    // No key for this service yet (setup skipped, or the service was just
    // switched): nothing to do, not a failure — the capsule would otherwise
    // say "clean-up failed" on every dictation. Another service's key is never
    // used.
    let Some(key) = usable_key(cfg) else {
        return Ok(None);
    };

    let mut body = json!({
        "model": cfg.model,
        "messages": [
            {"role": "system", "content": system},
            {"role": "user", "content": user},
        ],
        "temperature": 0,
        "stream": false,
    });
    if cfg.base_url.contains("deepseek.com") {
        // DeepSeek V4 reasons by default: up to 50 s on long input (M0).
        body["thinking"] = json!({"type": "disabled"});
    }
    let openrouter = is_openrouter(cfg);
    if openrouter {
        // Same reason, for whichever model was picked; and the dictation
        // waits on the answer, so take the provider that answers first.
        body["reasoning"] = reasoning_off(&cfg.model);
        body["provider"] = json!({"sort": "latency"});
    }
    let local = is_local(cfg);
    if local {
        body[LOCAL_REASONING] = json!("none");
    }

    // Transparency (history, privacy card): what leaves the computer, and
    // where. A model on this computer is not "sent" anywhere.
    let to = host(&cfg.base_url);
    if !is_loopback(&to) {
        let selected = extras.selection.as_deref().map_or(0, |s| s.chars().count());
        session::mark_sent(transcript.chars().count() + selected, to);
    }

    let started = std::time::Instant::now();
    let mut response = send(cfg, &key, &body, openrouter).await?;
    if openrouter && response.status() == reqwest::StatusCode::BAD_REQUEST {
        let message = error_message(response).await;
        if !message.to_lowercase().contains("reason") {
            return Err(format!("HTTP 400: {message}"));
        }
        // This model can't switch reasoning off: lowest effort, and remember.
        warn!("{} needs reasoning: {message}", cfg.model);
        lock_must_reason().insert(cfg.model.clone());
        body["reasoning"] = reasoning_off(&cfg.model);
        response = send(cfg, &key, &body, openrouter).await?;
    }
    if local && response.status() == reqwest::StatusCode::BAD_REQUEST {
        // A server or model that doesn't know the setting: ask without it.
        if let Some(fields) = body.as_object_mut() {
            fields.remove(LOCAL_REASONING);
        }
        response = send(cfg, &key, &body, openrouter).await?;
    }
    let status = response.status();
    if !status.is_success() {
        return Err(format!("HTTP {status}: {}", error_message(response).await));
    }
    let json: serde_json::Value = response
        .json()
        .await
        .map_err(|e| format!("bad response: {e}"))?;
    let content = json["choices"][0]["message"]["content"]
        .as_str()
        .ok_or("response has no content")?;
    debug!("Polish took {:?}", started.elapsed());

    let text = clean_output(content, context);
    if guarded {
        check_output(transcript, &text)?;
    } else if text.is_empty() {
        return Err("empty output".into());
    }
    Ok(Some(text))
}

fn lock_must_reason() -> std::sync::MutexGuard<'static, std::collections::HashSet<String>> {
    MUST_REASON.lock().unwrap_or_else(|e| e.into_inner())
}

/// OpenRouter's reasoning setting for `model`: off, or the lowest effort for
/// models that refused to turn it off.
fn reasoning_off(model: &str) -> serde_json::Value {
    if lock_must_reason().contains(model) {
        json!({"effort": "minimal", "exclude": true})
    } else {
        json!({"enabled": false})
    }
}

async fn send(
    cfg: &YuyinConfig,
    key: &str,
    body: &serde_json::Value,
    openrouter: bool,
) -> Result<reqwest::Response, String> {
    let mut request = CLIENT
        .post(endpoint(cfg, "/chat/completions"))
        .timeout(Duration::from_millis(local_timeout(cfg)))
        .json(body);
    if !key.is_empty() {
        request = request.bearer_auth(key);
    }
    if openrouter {
        // OpenRouter's app attribution: names Moqi, carries nothing of the user's.
        request = request
            .header(
                "HTTP-Referer",
                "https://github.com/DDChen666/moqi-voice-typing",
            )
            .header("X-Title", "Moqi");
    }
    request.send().await.map_err(|e| {
        if e.is_timeout() {
            format!("timed out after {} ms", local_timeout(cfg))
        } else {
            format!("request failed: {e}")
        }
    })
}

/// One model in OpenRouter's catalog, for the settings page's model picker.
#[derive(Clone, Debug, serde::Serialize, specta::Type)]
pub struct ModelInfo {
    pub id: String,
    pub name: String,
    /// US$ per million tokens.
    pub input_price: f64,
    pub output_price: f64,
}

/// OpenRouter's public model list (no key, nothing of the user's is sent):
/// models that read and write text, without the slow `:batch` variants.
pub async fn openrouter_models() -> Result<Vec<ModelInfo>, String> {
    let json: serde_json::Value = CLIENT
        .get(format!("https://{OPENROUTER_HOST}/api/v1/models"))
        .timeout(Duration::from_secs(15))
        .send()
        .await
        .map_err(|e| format!("request failed: {e}"))?
        .error_for_status()
        .map_err(|e| e.to_string())?
        .json()
        .await
        .map_err(|e| format!("bad response: {e}"))?;
    Ok(parse_models(&json))
}

fn parse_models(json: &serde_json::Value) -> Vec<ModelInfo> {
    let per_million = |v: &serde_json::Value| {
        v.as_str()
            .and_then(|s| s.parse::<f64>().ok())
            .map_or(0.0, |p| p * 1e6)
    };
    let has_text = |v: &serde_json::Value| {
        v.as_array()
            .is_some_and(|a| a.iter().any(|m| m.as_str() == Some("text")))
    };
    json["data"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|m| {
            has_text(&m["architecture"]["input_modalities"])
                && has_text(&m["architecture"]["output_modalities"])
        })
        .filter_map(|m| {
            let id = m["id"].as_str()?;
            if id.ends_with(":batch") {
                return None;
            }
            Some(ModelInfo {
                id: id.to_string(),
                name: m["name"].as_str().unwrap_or(id).to_string(),
                input_price: per_million(&m["pricing"]["prompt"]),
                output_price: per_million(&m["pricing"]["completion"]),
            })
        })
        .collect()
}

/// The service's own explanation of a failed request (OpenAI-style
/// `{"error": {"message": …}}`), shortened for the settings page.
async fn error_message(response: reqwest::Response) -> String {
    let text = response.text().await.unwrap_or_default();
    let message = serde_json::from_str::<serde_json::Value>(&text)
        .ok()
        .and_then(|v| v["error"]["message"].as_str().map(str::to_string))
        .unwrap_or(text);
    message.chars().take(200).collect()
}

/// Trim whitespace, and never end with a newline: in chat apps a trailing
/// newline sends the message (acceptance criterion 8).
fn clean_output(content: &str, context: Context) -> String {
    let text = content.trim();
    let text = if context == Context::Chat {
        text.trim_end_matches(['。', '\n'])
    } else {
        text
    };
    text.to_string()
}

/// Guard against the LLM answering the transcript instead of cleaning it, or
/// dropping most of it. Short inputs are exempt: removing fillers from a
/// three-word message legitimately halves it.
fn check_output(transcript: &str, output: &str) -> Result<(), String> {
    let n_in = transcript.chars().count();
    let n_out = output.chars().count();
    if n_out == 0 {
        return Err("empty output".into());
    }
    if n_in >= 12 && (n_out * 2 < n_in || n_out * 2 > n_in * 3) {
        return Err(format!("length changed too much ({n_in} → {n_out} chars)"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chat_output_has_no_trailing_period_or_newline() {
        assert_eq!(
            clean_output("好啊，沒問題。\n", Context::Chat),
            "好啊，沒問題"
        );
        assert_eq!(clean_output("收到。\n", Context::Other), "收到。");
    }

    #[test]
    fn guard_rejects_answers_and_truncation() {
        let q = "請你用 DeepSeek 跟 Claude 各跑一次，比較一下兩個結果";
        assert!(check_output(q, "請你用 DeepSeek 跟 Claude 各跑一次，比較一下兩個結果。").is_ok());
        let answer = q.repeat(3);
        assert!(check_output(q, &answer).is_err());
        assert!(check_output(q, "比較").is_err());
        assert!(check_output(q, "").is_err());
    }

    #[test]
    fn guard_allows_short_inputs_to_shrink() {
        assert!(check_output("呃，好啊好啊", "好啊").is_ok());
    }
}

/// `https://api.deepseek.com/v1` → `api.deepseek.com`
pub fn host(base_url: &str) -> String {
    let base_url = base_url.trim();
    let rest = base_url.split("://").nth(1).unwrap_or(base_url);
    rest.split(['/', '?', '#'])
        .next()
        .unwrap_or(rest)
        .to_string()
}

#[cfg(test)]
mod host_tests {
    use super::host;

    #[test]
    fn host_of_base_url() {
        assert_eq!(host("https://api.deepseek.com"), "api.deepseek.com");
        assert_eq!(host("https://openrouter.ai/api/v1"), "openrouter.ai");
        assert_eq!(host("http://localhost:11434/v1"), "localhost:11434");
        assert_eq!(host(" openrouter.ai/api/v1 "), "openrouter.ai");
        assert_eq!(host(""), "");
    }
}

#[cfg(test)]
mod openrouter_tests {
    use super::*;

    #[test]
    fn model_list_keeps_text_models_without_batch() {
        let json = json!({"data": [
            {"id": "~deepseek/deepseek-flash-latest", "name": "DeepSeek Flash Latest",
             "pricing": {"prompt": "0.00000003", "completion": "0.0000006"},
             "architecture": {"input_modalities": ["text"], "output_modalities": ["text"]}},
            {"id": "deepseek/deepseek-v4.1-flash:batch", "name": "batch",
             "pricing": {"prompt": "0.0000001", "completion": "0.0000003"},
             "architecture": {"input_modalities": ["text"], "output_modalities": ["text"]}},
            {"id": "google/some-image-model", "name": "image",
             "pricing": {"prompt": "0", "completion": "0"},
             "architecture": {"input_modalities": ["text"], "output_modalities": ["image"]}}
        ]});
        let models = parse_models(&json);
        assert_eq!(models.len(), 1);
        assert_eq!(models[0].id, "~deepseek/deepseek-flash-latest");
        assert!((models[0].input_price - 0.03).abs() < 1e-9);
        assert!((models[0].output_price - 0.6).abs() < 1e-9);
    }

    #[test]
    fn local_servers_are_recognized() {
        assert!(is_loopback("localhost:11434"));
        assert!(is_loopback("127.0.0.1:1234"));
        assert!(is_loopback("[::1]:8080"));
        assert!(is_loopback("LOCALHOST"));
        assert!(!is_loopback("openrouter.ai"));
        assert!(!is_loopback("localhost.example.com"));
        assert!(!is_loopback("192.168.1.5:11434"));
        assert!(!is_loopback("127.evil.example"));
    }

    #[test]
    fn translating_or_editing_uses_the_service_even_at_raw() {
        let raw = YuyinConfig {
            level: Level::Raw,
            ..YuyinConfig::default()
        };
        let plain = session::Extras::default();
        assert_eq!(effective(&raw, &plain).level, Level::Raw);
        let translate = session::Extras {
            translate_to: Some("en".into()),
            ..Default::default()
        };
        assert_eq!(effective(&raw, &translate).level, Level::Tidy);
        let edit = session::Extras {
            selection: Some("x".into()),
            ..Default::default()
        };
        assert_eq!(effective(&raw, &edit).level, Level::Tidy);
    }

    #[test]
    fn a_local_model_needs_no_key_and_sends_nothing_out() {
        let cfg = YuyinConfig {
            service: config::Service::Local,
            base_url: "http://localhost:11434/v1".into(),
            model: "qwen3:8b".into(),
            ..YuyinConfig::default()
        };
        assert_eq!(usable_key(&cfg).as_deref(), Some(""));
        assert_eq!(destination(&cfg), None);
        assert_eq!(local_timeout(&cfg), 15_000);
        assert!(is_local(&cfg));
        assert!(!is_local(&YuyinConfig::default()));
    }

    #[test]
    fn openrouter_is_named_in_the_capsule() {
        assert_eq!(display_name("openrouter.ai"), "OpenRouter");
        assert_eq!(display_name("api.deepseek.com"), "DeepSeek");
        assert_eq!(display_name("localhost:11434"), "localhost:11434");
    }

    #[test]
    fn reasoning_is_off_until_a_model_refuses() {
        assert_eq!(reasoning_off("x/model-a"), json!({"enabled": false}));
        lock_must_reason().insert("x/model-b".into());
        assert_eq!(reasoning_off("x/model-b")["effort"], "minimal");
    }
}

#[cfg(test)]
mod key_host_tests {
    use super::*;

    fn cfg(service: config::Service, base_url: &str) -> YuyinConfig {
        YuyinConfig {
            service,
            base_url: base_url.into(),
            ..YuyinConfig::default()
        }
    }

    #[test]
    fn nothing_is_sent_without_a_service_or_an_address() {
        use config::Service;
        let deepseek = "https://api.deepseek.com";
        assert_eq!(
            key_host(&cfg(Service::Deepseek, deepseek)).as_deref(),
            Some("api.deepseek.com")
        );
        assert_eq!(
            key_host(&cfg(Service::Custom, "https://openrouter.ai/api/v1")).as_deref(),
            Some("openrouter.ai")
        );
        assert_eq!(key_host(&cfg(Service::Custom, "  ")), None);
        assert_eq!(key_host(&cfg(Service::None, deepseek)), None);
        // No usable key either way, so no capsule destination and no warm-up.
        assert_eq!(destination(&cfg(Service::None, deepseek)), None);
        assert_eq!(destination(&cfg(Service::Custom, "")), None);
        let no_model = YuyinConfig {
            model: " ".into(),
            ..cfg(Service::Custom, "https://openrouter.ai/api/v1")
        };
        assert_eq!(destination(&no_model), None);
    }

    #[test]
    fn an_address_without_a_scheme_uses_https() {
        let c = cfg(config::Service::Custom, "openrouter.ai/api/v1/");
        assert_eq!(
            endpoint(&c, "/chat/completions"),
            "https://openrouter.ai/api/v1/chat/completions"
        );
        let c = cfg(config::Service::Custom, "http://localhost:11434/v1");
        assert_eq!(endpoint(&c, "/models"), "http://localhost:11434/v1/models");
    }
}

/// A model on this computer may still be loading into memory on the first
/// request: give it longer before falling back to the transcript.
fn local_timeout(cfg: &YuyinConfig) -> u64 {
    if is_loopback(&host(&cfg.base_url)) {
        cfg.timeout_ms.max(15_000)
    } else {
        cfg.timeout_ms
    }
}

/// The models a server on this computer offers (OpenAI-style `/models`),
/// for the settings page. Never asks anything off this computer.
pub async fn local_models(base_url: &str) -> Result<Vec<String>, String> {
    if !is_loopback(&host(base_url)) {
        return Err("not a local address".into());
    }
    let probe = YuyinConfig {
        base_url: base_url.to_string(),
        ..YuyinConfig::default()
    };
    let json: serde_json::Value = CLIENT
        .get(endpoint(&probe, "/models"))
        .timeout(Duration::from_secs(3))
        .send()
        .await
        .map_err(|e| format!("request failed: {e}"))?
        .error_for_status()
        .map_err(|e| e.to_string())?
        .json()
        .await
        .map_err(|e| format!("bad response: {e}"))?;
    Ok(json["data"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|m| m["id"].as_str().map(str::to_string))
        .collect())
}

/// What the capsule names as the destination ("文字 → DeepSeek"), or `None`
/// when this dictation sends nothing off this computer.
pub fn destination(cfg: &YuyinConfig) -> Option<String> {
    usable_key(cfg)?;
    let to = host(&cfg.base_url);
    (!is_loopback(&to)).then(|| display_name(&to))
}

/// How the capsule names a host: the service's name when we know it.
fn display_name(host: &str) -> String {
    if host.ends_with("deepseek.com") {
        "DeepSeek".into()
    } else if host == OPENROUTER_HOST {
        "OpenRouter".into()
    } else {
        host.to_string()
    }
}
