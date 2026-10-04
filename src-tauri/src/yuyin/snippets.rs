//! Voice snippets (語音片語): say a short phrase, get a longer text. When a
//! whole dictation is one of the user's triggers ("我的地址"), Moqi pastes
//! the snippet's text as written instead of the transcript, and skips the
//! clean-up. Punctuation, spaces and letter case don't matter, since the
//! recognizer adds them ("我的地址。", "My email").

use serde::{Deserialize, Serialize};
use specta::Type;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Type)]
pub struct Snippet {
    /// What the user says.
    pub trigger: String,
    /// What gets pasted, kept exactly (line breaks included).
    pub text: String,
}

/// Letters and digits only, Latin lower-cased: how a trigger is compared.
fn key(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

/// The snippet `transcript` calls for, if the whole dictation is a trigger.
pub fn expand<'a>(snippets: &'a [Snippet], transcript: &str) -> Option<&'a str> {
    let said = key(transcript);
    if said.is_empty() {
        return None;
    }
    snippets
        .iter()
        .find(|s| !s.text.is_empty() && key(&s.trigger) == said)
        .map(|s| s.text.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(trigger: &str, text: &str) -> Snippet {
        Snippet {
            trigger: trigger.into(),
            text: text.into(),
        }
    }

    #[test]
    fn the_whole_dictation_must_be_the_trigger() {
        let list = vec![
            s("我的地址", "台北市大安區某某路 1 號"),
            s("my email", "me@example.com"),
        ];
        assert_eq!(expand(&list, "我的地址。"), Some("台北市大安區某某路 1 號"));
        assert_eq!(expand(&list, " My Email! "), Some("me@example.com"));
        assert_eq!(expand(&list, "我的地址改了"), None);
        assert_eq!(expand(&list, "。"), None);
    }

    #[test]
    fn empty_snippets_never_fire() {
        assert_eq!(expand(&[s("地址", "")], "地址"), None);
        assert_eq!(expand(&[s("", "x")], ""), None);
    }
}
