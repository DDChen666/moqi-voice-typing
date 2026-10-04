//! Yuyin fork: the product layer we add on top of Handy.
//!
//! Everything in this module is new code. Upstream Handy files only get small
//! hooks marked `Yuyin fork:` so monthly cherry-picks from upstream stay easy.
//!
//! Flow of one dictation:
//! 1. key press   → [`session::begin`] records the frontmost app, its focused
//!    window and the writing context (chat / to-AI / notes / other).
//! 2. speaking    → [`chunker`] transcribes the speech so far at each pause,
//!    in the background; release  → only the tail after the last pause is
//!    left to transcribe.
//! 3. transcript  → [`polish::polish`] tidies it with the M0-validated prompt,
//!    unless the level is Raw. Any failure falls back to the raw transcript.
//! 4. before paste → [`session::focus_changed`] decides whether pasting is
//!    still safe; if the user switched windows we copy instead.

pub mod app_menu;
pub mod apps;
pub mod chunker;
pub mod commands;
pub mod config;
pub mod context;
pub mod defaults;
pub mod field_probe;
#[cfg(target_os = "windows")]
pub mod focus_return;
pub mod learn;
pub mod level_boost;
pub mod output;
pub mod polish;
pub mod prompt;
pub mod replay;
pub mod secrets;
pub mod session;
pub mod snippets;
pub mod stats;
pub mod sync;
pub mod warmup;
#[cfg(target_os = "windows")]
pub mod window_look;
pub mod wording;
