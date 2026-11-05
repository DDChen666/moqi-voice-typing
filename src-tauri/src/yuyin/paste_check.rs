//! Did the paste land? Asked of the field itself, not of the clipboard.
//!
//! The reliable paste (`paste_tx`) took any read of the clipboard as the
//! target's and restored the previous clipboard 0.2 s after it. On the
//! user's Mac that lost dictations in Chrome: two in three, on the HTML
//! pages they were working in. Every one was "read 110 ms after the chord"
//! in the log, yet the field stayed empty, and with the old clipboard back
//! (an image) the text was nowhere but the history.
//!
//! So after the paste chord we read the focused field (Accessibility) until
//! it shows the paste. If it never changes, the paste is sent once more; if
//! that doesn't land either, the text stays on the clipboard and the pill
//! says so. A field that can't be read keeps the old behaviour, but the
//! clipboard is restored no earlier than [`CHECK_FOR`] after the chord.

use std::time::Duration;

use super::field_probe::{shows, squash};
use super::session;

/// How long a field may take to show the paste. Chrome's Accessibility tree
/// follows the page within about 0.1 s; heavy pages need longer.
pub const CHECK_FOR: Duration = Duration::from_millis(1500);
/// Between two readings of the field.
pub const EVERY: Duration = Duration::from_millis(100);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Seen {
    /// The field shows the paste, or changed since before it (editors
    /// reformat what they get).
    Pasted,
    /// The field reads exactly as before the paste: nothing landed.
    Unchanged,
    /// Readable, without the paste, and there is no reading from before to
    /// compare with: probably not pasted, but pasting again could double it.
    Missing,
    /// The field can't be read (no Accessibility, nothing focused, too long).
    Unknown,
}

pub fn judge(before: Option<&str>, now: Option<&str>, pasted: &str) -> Seen {
    let Some(now) = now else {
        return Seen::Unknown;
    };
    if shows(now, pasted) {
        return Seen::Pasted;
    }
    match before {
        Some(before) if squash(before) == squash(now) => Seen::Unchanged,
        Some(_) => Seen::Pasted,
        None => Seen::Missing,
    }
}

/// One reading of the focused field of `pid`.
pub fn look(pid: i32, before: Option<&str>, pasted: &str) -> Seen {
    let now = session::focused_field(pid).and_then(|(_, v)| v);
    judge(before, now.as_deref(), pasted)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_field_showing_the_paste_is_pasted() {
        assert_eq!(
            judge(
                Some("你好"),
                Some("你好以及那個姿勢跟表情。"),
                "以及那個姿勢跟表情。"
            ),
            Seen::Pasted
        );
        // Whatever the field held before.
        assert_eq!(judge(None, Some("以及知識。"), "以及知識。"), Seen::Pasted);
    }

    #[test]
    fn an_untouched_field_is_unchanged() {
        assert_eq!(judge(Some(""), Some(""), "以及知識。"), Seen::Unchanged);
        // A chat box reads as its placeholder when empty.
        assert_eq!(
            judge(
                Some("Reply to Claude…"),
                Some("Reply to Claude…"),
                "以及知識。"
            ),
            Seen::Unchanged
        );
        // Reflowed whitespace is not a change.
        assert_eq!(
            judge(Some("a b\n"), Some("a  b"), "以及知識。"),
            Seen::Unchanged
        );
    }

    #[test]
    fn a_reformatted_paste_still_counts() {
        assert_eq!(
            judge(Some("draft "), Some("draft -- 以及知識"), "以及知識。"),
            Seen::Pasted
        );
    }

    #[test]
    fn without_a_reading_from_before_a_missing_paste_is_not_retried() {
        assert_eq!(judge(None, Some("別的字"), "以及知識。"), Seen::Missing);
    }

    #[test]
    fn an_unreadable_field_is_unknown() {
        assert_eq!(judge(Some(""), None, "以及知識。"), Seen::Unknown);
    }
}
