//! The recognizer's spacing, and the one question mark that is never in
//! doubt, fixed before anything else sees the text (history's 原話 too) and
//! again after the clean-up.
//!
//! - Spelled-out capitals are joined. Given a dictionary, Qwen3-ASR writes
//!   some acronyms letter by letter: "S E O", "V S Code", "Grok C L I" (6 of
//!   the user's 93 dictations in `results/hist_vocab_v1`). Only capitals
//!   that stand alone, two or more in a row; "S Premium" and "A 四 B 三" stay.
//! - No space next to full-width punctuation ("如你所知， Super Grok").
//! - 嗎 is only ever the question particle (嘛 is another character), so a
//!   sentence ending 嗎。 is a question: 嗎？. Other questions (呢, 是不是…)
//!   can be statements too and are left to the clean-up.

/// Full-width punctuation that takes no space after it, and before it.
const NO_SPACE_AFTER: &str = "，。？！：；、）」』】";
const NO_SPACE_BEFORE: &str = "，。？！：；、（「『【";

pub fn tidy(text: &str) -> String {
    question_marks(&no_space_by_full_width(&join_spelled(text)))
}

/// 嗎。 → 嗎？ (and Simplified 吗。, which the recognizer writes).
pub fn question_marks(text: &str) -> String {
    text.replace("嗎。", "嗎？").replace("吗。", "吗？")
}

fn join_spelled(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let alone = |i: usize| {
        chars.get(i).is_some_and(char::is_ascii_uppercase)
            && (i == 0 || !chars[i - 1].is_ascii_alphanumeric())
            && !chars.get(i + 1).is_some_and(char::is_ascii_alphanumeric)
    };
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while i < chars.len() {
        out.push(chars[i]);
        if alone(i) && chars.get(i + 1) == Some(&' ') && alone(i + 2) {
            i += 2; // drop the space; the next letter goes in next
        } else {
            i += 1;
        }
    }
    out
}

fn no_space_by_full_width(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    for (i, &c) in chars.iter().enumerate() {
        if c == ' ' {
            let after_mark = out
                .chars()
                .next_back()
                .is_some_and(|p| NO_SPACE_AFTER.contains(p));
            let next = chars[i + 1..].iter().find(|&&n| n != ' ');
            let before_mark = next.is_some_and(|&n| NO_SPACE_BEFORE.contains(n));
            if after_mark || before_mark {
                continue;
            }
        }
        out.push(c);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spelled_out_capitals_are_joined() {
        assert_eq!(tidy("对于我们的研究 S E O 来说"), "对于我们的研究 SEO 来说");
        assert_eq!(tidy("比如 V S Code。"), "比如 VS Code。");
        assert_eq!(tidy("使用 Grok C L I 来生成"), "使用 Grok CLI 来生成");
        assert_eq!(tidy("A I 很好用"), "AI 很好用");
    }

    #[test]
    fn letters_that_are_words_stay_apart() {
        assert_eq!(tidy("我是 S Premium。"), "我是 S Premium。");
        assert_eq!(tidy("我的感觉 A 四 B 三"), "我的感觉 A 四 B 三");
        assert_eq!(tidy("I am OK, I think"), "I am OK, I think");
        assert_eq!(tidy("X Premium 跟 Grok Bot"), "X Premium 跟 Grok Bot");
    }

    #[test]
    fn no_space_next_to_full_width_punctuation() {
        assert_eq!(
            tidy("如你所知， Super Grok 支援 Grok Bot"),
            "如你所知，Super Grok 支援 Grok Bot"
        );
        assert_eq!(tidy("写在哪？ GitHub 吗？"), "写在哪？GitHub 吗？");
        assert_eq!(tidy("用 Rust ，然后"), "用 Rust，然后");
        // Ordinary spaces stay.
        assert_eq!(
            tidy("我要讓 Claude Code 跟 Grokbot 那邊"),
            "我要讓 Claude Code 跟 Grokbot 那邊"
        );
    }

    #[test]
    fn a_sentence_ending_in_ma_is_a_question() {
        assert_eq!(tidy("你可以幫我看一下嗎。好"), "你可以幫我看一下嗎？好");
        assert_eq!(tidy("可以传到你那边吗。"), "可以传到你那边吗？");
        // A comma after 嗎 is often "你知道嗎，…": left alone.
        assert_eq!(tidy("你知道嗎，他昨天來了。"), "你知道嗎，他昨天來了。");
        assert_eq!(tidy("好嘛。"), "好嘛。");
    }
}
