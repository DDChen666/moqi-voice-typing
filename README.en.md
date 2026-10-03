<picture>
  <source media="(prefers-color-scheme: dark)" srcset="docs/media/banner-dark-en.png">
  <img src="docs/media/banner-light-en.png" width="100%" alt="默契 Moqi: dictation that gets you. Mandarin, English, and everything in between. Your voice is recognized on your Mac and never uploaded.">
</picture>

<br>

<p align="center">
  <a href="https://github.com/DDChen666/moqi-voice-typing/releases/latest"><img src="docs/media/download-en.png" width="300" alt="Download for macOS"></a>
</p>

<p align="center">
  <a href="docs/安裝教學-Mac.md"><b>Install guide</b></a> &nbsp;·&nbsp;
  <a href="docs/使用指南.md"><b>Usage</b></a> &nbsp;·&nbsp;
  <a href="docs/隱私.md"><b>Privacy</b></a> &nbsp;·&nbsp;
  <a href="docs/評測.md"><b>Benchmarks</b></a> &nbsp;·&nbsp;
  <a href="CHANGELOG.md"><b>Changelog</b></a> &nbsp;·&nbsp;
  <a href="README.md"><b>繁體中文</b></a>
</p>

<p align="center">
  <img alt="Version 1.0.0" src="https://img.shields.io/badge/version-1.0.0-0a84ff?style=flat-square">
  <img alt="macOS 15+" src="https://img.shields.io/badge/macOS-15%2B-1d1d1f?style=flat-square&logo=apple&logoColor=white">
  <img alt="Apple silicon" src="https://img.shields.io/badge/Apple-silicon-1d1d1f?style=flat-square">
  <img alt="Voice stays on device" src="https://img.shields.io/badge/voice-on--device-34c759?style=flat-square">
  <img alt="MIT" src="https://img.shields.io/badge/license-MIT-8e8e93?style=flat-square">
</p>

<br>

https://github.com/user-attachments/assets/d8b08c03-3a8e-45ee-86c3-d20c30ad6946

<br>

Hold the right Option key, speak, let go — the text appears at your cursor. Moqi is built for people who **switch between Mandarin and English mid-sentence**. Speech is recognized entirely on your Mac; sending the text to an AI for clean-up is up to you.

<br>

<img src="docs/media/feature-mixed.png" width="100%" alt="Code-switching that just works: fillers removed, PR, description and breaking changes kept in English">

**English stays English, fillers disappear.** 「幫我把這個 PR 的 description 改短一點」 is neither translated into Chinese nor flipped into English. The speech model was chosen by testing 8 on-device models on 30 real mixed-language recordings.

<br>

<img src="docs/media/feature-context.png" width="100%" alt="It knows where you're typing: chat, instructions to AI, and notes each get their own format">

**Adapts to the app.** Casual in chat apps — no trailing period, no accidental send. Every command and file name intact in Claude Code and terminals. Spoken lists become bullet points in Notes. The app is identified on your Mac; only a label such as "chat" is sent.

<br>

<img src="docs/media/feature-speed.png" width="100%" alt="1.80 seconds after release; 4.3 seconds without transcribing while you talk">

**Transcribes while you talk.** Every pause lets Moqi recognize what you've said so far. After 30+ seconds of dictation, cleaned-up text lands a median 1.8 s after you let go (real-world usage, AI clean-up included).

<br>

<img src="docs/media/feature-privacy.png" width="100%" alt="Recognition stays on your Mac; the privacy card shows 0 seconds of audio uploaded">

**Your voice never leaves your computer.** No account, no analytics. Updates ask before installing, and the check can be turned off. The API key lives in the macOS Keychain. In Raw mode, not even text is sent.

<br>

<img src="docs/media/feature-history.png" width="100%" alt="Every request is on record: history shows what was sent and what wasn't">

**See exactly what was sent, every time.** History keeps the raw transcript, the recording and a list of what left your Mac. Paste again with one click; switch windows mid-sentence and Moqi copies instead of pasting in the wrong place.

<br>

## Get started

1. [Download](https://github.com/DDChen666/moqi-voice-typing/releases/latest) `Moqi_1.0.0_aarch64.dmg` and drag Moqi into Applications.
2. The first launch is blocked because Moqi isn't signed with a paid Apple Developer ID. Open **System Settings → Privacy & Security** and click **Open Anyway**. If macOS says the app "is damaged", run `xattr -dr com.apple.quarantine /Applications/Moqi.app` in Terminal.
3. Follow the three setup steps: allow permissions, choose Raw or Tidy, and try a sentence.

Requires an M1 or later Mac with macOS 15 or later. The interface is available in Traditional Chinese and English.

|                        | What it does                                                    | Needs                                                                                     |
| ---------------------- | --------------------------------------------------------------- | ----------------------------------------------------------------------------------------- |
| **Raw**                | Punctuation, Traditional Chinese, your dictionary               | Nothing — fully on-device                                                                 |
| **Tidy** (recommended) | Removes fillers and false starts, turns spoken lists into lists | An API key for the clean-up service (DeepSeek by default; any OpenAI-compatible endpoint) |
| **Polish**             | Rewrites into written prose                                     | Same                                                                                      |

Hold right ⌥ Option to dictate, double-tap for hands-free, Esc to cancel.

## Roadmap

- [ ] **Learns your words** — fix a word once and Moqi gets it right next time. Recognition-side vocabulary is done on the `v1.1` branch (English terms correct 71% → 87%).
- [ ] **Windows**
- [ ] **On-device clean-up** with a small local language model, so not even text leaves your computer.
- [ ] **Learns your typing habits** (spaces vs. line breaks, punctuation), off by default.

## Contributing

Bug reports, "what I said vs. what Moqi wrote" examples and pull requests are welcome, in English or Chinese. See [CONTRIBUTING.md](CONTRIBUTING.md), [BUILD.md](BUILD.md) and the [architecture notes](docs/架構.md).

## Credits

Moqi is built on [Handy](https://github.com/cjpais/Handy) (hotkeys, overlay, recording and transcription — see [FORK.md](FORK.md)), [Qwen3-ASR](https://github.com/QwenLM/Qwen3-ASR), [transcribe.cpp](https://github.com/handy-computer/transcribe.cpp), [Silero VAD](https://github.com/snakers4/silero-vad), [OpenCC](https://github.com/BYVoid/OpenCC) and [Tauri](https://tauri.app/).

<br>

<p align="center">
  <img src="yuyin/brand/app-icon-1024.png" width="64" alt=""><br>
  <sub>默契 Moqi · <a href="LICENSE">MIT License</a></sub>
</p>
