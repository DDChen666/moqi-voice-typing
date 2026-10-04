//! Which kind of writing the user is doing, decided locally from the
//! frontmost app and its window title. Only the resulting label ever leaves
//! the machine (as a style instruction to the LLM); the app name and title
//! never do.

use serde::{Deserialize, Serialize};
use specta::Type;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum Context {
    /// LINE, Messenger and other chat apps.
    Chat,
    /// Instructions to an AI: Claude Code, ChatGPT, terminals, editors.
    ToAi,
    /// Note taking: Google Keep, Apple Notes, Obsidian.
    Notes,
    Other,
}

/// The app that had focus when the user pressed the shortcut.
#[derive(Clone, Debug, Default)]
pub struct FrontApp {
    /// macOS: the bundle id (`jp.naver.line.mac`). Windows: the executable's
    /// file name in lower case (`line.exe`). The lists below hold both.
    pub bundle_id: String,
    pub pid: i32,
    pub window_title: String,
    /// The app's name as the user sees it (e.g. "LINE", "終端機").
    pub name: String,
}

const CHAT_APPS: &[&str] = &[
    "jp.naver.line.mac",
    "com.facebook.archon",
    "com.facebook.archon.developerID",
    "com.apple.MobileSMS",
    "ru.keepcoder.Telegram",
    "net.whatsapp.WhatsApp",
    "com.hnc.Discord",
    "com.tinyspeck.slackmacgap",
    // Windows
    "line.exe",
    "messenger.exe",
    "telegram.exe",
    "whatsapp.exe",
    "whatsapp.root.exe", // the 2024+ WhatsApp app
    "discord.exe",
    "discordptb.exe",
    "discordcanary.exe",
    "slack.exe",
    "ms-teams.exe", // Teams sends on Enter like the others
    "teams.exe",    // classic Teams
];

const AI_APPS: &[&str] = &[
    "com.anthropic.claudefordesktop",
    "com.openai.chat",
    // Claude Code runs in terminals and editors.
    "com.apple.Terminal",
    "com.googlecode.iterm2",
    "dev.warp.Warp-Stable",
    "com.mitchellh.ghostty",
    "com.microsoft.VSCode",
    "com.todesktop.230313mzl4w4u92", // Cursor
    // Windows
    "claude.exe",
    "chatgpt.exe",
    "windowsterminal.exe",
    "openconsole.exe",
    "conhost.exe",
    "cmd.exe",
    "powershell.exe",
    "pwsh.exe",
    "wezterm-gui.exe",
    "alacritty.exe",
    "mintty.exe", // Git Bash, MSYS2, Cygwin
    "warp.exe",
    "tabby.exe",
    "hyper.exe",
    "code.exe",
    "code - insiders.exe",
    "cursor.exe",
];

const NOTES_APPS: &[&str] = &[
    "com.apple.Notes",
    "md.obsidian",
    // Windows
    "obsidian.exe",
    "onenote.exe",
    "onenoteim.exe",       // OneNote for Windows 10 (Store)
    "microsoft.notes.exe", // Sticky Notes
];

const BROWSERS: &[&str] = &[
    "com.google.Chrome",
    "com.apple.Safari",
    "company.thebrowser.Browser", // Arc
    "org.mozilla.firefox",
    "com.microsoft.edgemac",
    "com.brave.Browser",
    // Windows
    "chrome.exe",
    "msedge.exe",
    "firefox.exe",
    "brave.exe",
    "arc.exe",
    "vivaldi.exe",
    "opera.exe",
];

/// Web apps are identified by the tab title that browsers put in the window
/// title. Checked in order; the first match wins.
const WEB_TITLES: &[(&str, Context, &str)] = &[
    ("messenger", Context::Chat, "Messenger"),
    ("facebook", Context::Chat, "Facebook"),
    ("line", Context::Chat, "LINE"),
    ("google keep", Context::Notes, "Keep"),
    ("keep", Context::Notes, "Keep"),
    ("chatgpt", Context::ToAi, "ChatGPT"),
    ("claude", Context::ToAi, "Claude"),
    ("gemini", Context::ToAi, "Gemini"),
    ("deepseek", Context::ToAi, "DeepSeek"),
];

fn web_app(app: &FrontApp) -> Option<(Context, &'static str)> {
    if !BROWSERS.contains(&app.bundle_id.as_str()) {
        return None;
    }
    let title = app.window_title.to_lowercase();
    WEB_TITLES
        .iter()
        .find(|(needle, _, _)| contains_word(&title, needle))
        .map(|(_, context, name)| (*context, *name))
}

/// What the capsule shows for a moment on key press: the web app inside a
/// browser ("Keep"), otherwise the app's own name ("LINE").
pub fn display_name(app: &FrontApp) -> String {
    match web_app(app) {
        Some((_, name)) => name.to_string(),
        None => app.name.clone(),
    }
}

/// What a per-app style is saved under (apps.rs): the app, or for a web app
/// the browser and the site ("com.google.Chrome#Facebook"). The settings list
/// names that row "Facebook"; keyed by the browser alone, a choice made for it
/// changed every other tab too.
pub fn style_key(app: &FrontApp) -> String {
    match web_app(app) {
        Some((_, site)) => format!("{}#{site}", app.bundle_id),
        None => app.bundle_id.clone(),
    }
}

/// Password and permission prompts the system puts in front (the Keychain
/// password sheet, UAC): not somewhere to dictate, so not an app to list.
pub fn is_system_prompt(app: &FrontApp) -> bool {
    const PROMPTS: &[&str] = &[
        "com.apple.SecurityAgent",
        "com.apple.loginwindow",
        "com.apple.coreservices.uiagent",
        "com.apple.UserNotificationCenter",
        // Windows
        "consent.exe",
        "credentialuibroker.exe",
        "logonui.exe",
        "lockapp.exe",
    ];
    PROMPTS.contains(&app.bundle_id.as_str())
}

pub fn classify(app: &FrontApp) -> Context {
    let id = app.bundle_id.as_str();
    if CHAT_APPS.contains(&id) {
        return Context::Chat;
    }
    if AI_APPS.contains(&id) {
        return Context::ToAi;
    }
    if NOTES_APPS.contains(&id) {
        return Context::Notes;
    }
    web_app(app)
        .map(|(context, _)| context)
        .unwrap_or(Context::Other)
}

/// `needle` appears in `haystack` as a whole word, so "line" matches
/// "LINE - Google Chrome" but not "Timeline" or "online".
fn contains_word(haystack: &str, needle: &str) -> bool {
    haystack.match_indices(needle).any(|(start, _)| {
        let before = haystack[..start].chars().next_back();
        let after = haystack[start + needle.len()..].chars().next();
        !before.is_some_and(char::is_alphanumeric) && !after.is_some_and(char::is_alphanumeric)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app(bundle_id: &str, title: &str) -> FrontApp {
        FrontApp {
            bundle_id: bundle_id.into(),
            pid: 1,
            window_title: title.into(),
            name: "App".into(),
        }
    }

    #[test]
    fn display_names() {
        assert_eq!(
            display_name(&app("com.google.Chrome", "Google Keep - Google Chrome")),
            "Keep"
        );
        assert_eq!(display_name(&app("jp.naver.line.mac", "")), "App");
    }

    #[test]
    fn a_site_gets_its_own_style_not_the_whole_browser() {
        let facebook = app("com.google.Chrome", "Facebook - Google Chrome");
        let other = app("com.google.Chrome", "Hacker News - Google Chrome");
        assert_eq!(style_key(&facebook), "com.google.Chrome#Facebook");
        assert_eq!(style_key(&other), "com.google.Chrome");
        assert_eq!(
            style_key(&app("jp.naver.line.mac", "")),
            "jp.naver.line.mac"
        );
    }

    #[test]
    fn system_prompts_are_not_apps() {
        assert!(is_system_prompt(&app("com.apple.SecurityAgent", "")));
        assert!(is_system_prompt(&app("consent.exe", "")));
        assert!(!is_system_prompt(&app("com.apple.Notes", "")));
    }

    #[test]
    fn native_apps() {
        assert_eq!(classify(&app("jp.naver.line.mac", "")), Context::Chat);
        assert_eq!(classify(&app("com.openai.chat", "")), Context::ToAi);
        assert_eq!(
            classify(&app("com.apple.Terminal", "claude")),
            Context::ToAi
        );
        assert_eq!(classify(&app("com.apple.Notes", "")), Context::Notes);
        assert_eq!(classify(&app("com.apple.finder", "")), Context::Other);
    }

    #[test]
    fn web_apps_by_title() {
        let chrome = "com.google.Chrome";
        assert_eq!(
            classify(&app(chrome, "(2) Messenger - Google Chrome")),
            Context::Chat
        );
        assert_eq!(
            classify(&app(chrome, "Google Keep - Google Chrome")),
            Context::Notes
        );
        assert_eq!(classify(&app(chrome, "ChatGPT")), Context::ToAi);
        assert_eq!(classify(&app("com.apple.Safari", "Claude")), Context::ToAi);
        assert_eq!(classify(&app(chrome, "YouTube")), Context::Other);
    }

    #[test]
    fn windows_executables() {
        assert_eq!(classify(&app("line.exe", "")), Context::Chat);
        assert_eq!(classify(&app("windowsterminal.exe", "")), Context::ToAi);
        assert_eq!(classify(&app("obsidian.exe", "")), Context::Notes);
        assert_eq!(
            classify(&app("msedge.exe", "ChatGPT - Microsoft Edge")),
            Context::ToAi
        );
        assert_eq!(classify(&app("notepad.exe", "")), Context::Other);
    }

    #[test]
    fn windows_apps_and_titles() {
        for chat in ["whatsapp.root.exe", "discord.exe", "ms-teams.exe"] {
            assert_eq!(classify(&app(chat, "")), Context::Chat, "{chat}");
        }
        for ai in [
            "mintty.exe",
            "pwsh.exe",
            "code.exe",
            "cursor.exe",
            "claude.exe",
        ] {
            assert_eq!(classify(&app(ai, "")), Context::ToAi, "{ai}");
        }
        assert_eq!(classify(&app("onenoteim.exe", "")), Context::Notes);
        // Edge puts the profile and the other tabs in the title.
        assert_eq!(
            classify(&app(
                "msedge.exe",
                "Messenger 和另外 3 個頁面 - 個人 - Microsoft​ Edge"
            )),
            Context::Chat
        );
        assert_eq!(
            classify(&app("chrome.exe", "Google Keep - Google Chrome")),
            Context::Notes
        );
        assert_eq!(
            display_name(&app("chrome.exe", "Claude - Google Chrome")),
            "Claude"
        );
    }

    #[test]
    fn titles_only_count_in_browsers() {
        // A document named "Messenger notes" in a text editor is not a chat.
        assert_eq!(
            classify(&app("com.apple.TextEdit", "Messenger notes")),
            Context::Other
        );
    }

    #[test]
    fn whole_words_only() {
        let chrome = "com.google.Chrome";
        assert_eq!(
            classify(&app(chrome, "Timeline - Google Chrome")),
            Context::Other
        );
        assert_eq!(classify(&app(chrome, "Shop online")), Context::Other);
        assert_eq!(classify(&app(chrome, "Keeper Security")), Context::Other);
        assert_eq!(classify(&app(chrome, "LINE")), Context::Chat);
    }
}
