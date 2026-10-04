//! Traditional Chinese the way people in Taiwan write it day to day. The
//! recognizer outputs Simplified Chinese; OpenCC's `s2twp` converts both the
//! characters and the vocabulary (界面 → 介面, 软件 → 軟體). It follows the
//! official 臺, while everyday writing (and every place name people type)
//! uses 台, so that one character is folded back. Likewise it writes 账 as
//! 賬 (賬號, 報賬), where Taiwan writes 帳 (帳號, 報帳).
//!
//! The week is 週 in Taiwan (這週, 週末). OpenCC leaves 周 alone, and so does
//! the clean-up model, which writes 這周 even when told to use Taiwan's
//! wording. `weeks` fixes the calendar words only: 周 also means "around"
//! (四周, 周圍, 繞場一周) and is a surname.

use ferrous_opencc::{config::BuiltinConfig, OpenCC};

pub fn taiwan(text: &str) -> Result<String, String> {
    let converter = OpenCC::from_config(BuiltinConfig::S2twp).map_err(|e| e.to_string())?;
    Ok(weeks(
        &converter
            .convert(text)
            .replace('臺', "台")
            .replace('賬', "帳"),
    ))
}

/// 周 → 週 where it means the week: after 這 上 下 本 每 隔 當 兩 幾, and before
/// 末 一 二 三 四 五 六 日 天 休 (週末, 週三, 週休). Everything else is left alone.
pub fn weeks(text: &str) -> String {
    const BEFORE: &str = "這上下本每隔當兩幾";
    const AFTER: &str = "末一二三四五六日天休";
    if !text.contains('周') {
        return text.to_string();
    }
    let chars: Vec<char> = text.chars().collect();
    chars
        .iter()
        .enumerate()
        .map(|(i, &c)| {
            let week = c == '周'
                && (i.checked_sub(1).is_some_and(|p| BEFORE.contains(chars[p]))
                    || chars.get(i + 1).is_some_and(|n| AFTER.contains(*n)));
            if week {
                '週'
            } else {
                c
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tw(s: &str) -> String {
        taiwan(s).unwrap()
    }

    #[test]
    fn taiwan_vocabulary_and_everyday_tai() {
        assert_eq!(
            tw("好啊，那我们明天晚上七点在台北车站见"),
            "好啊，那我們明天晚上七點在台北車站見"
        );
        assert_eq!(tw("然后界面的话"), "然後介面的話");
        assert_eq!(
            tw("这个软件的视频功能，默认的内存和网络设置"),
            "這個軟體的影片功能，預設的記憶體和網路設定"
        );
        assert_eq!(tw("台风天去台湾的平台"), "颱風天去台灣的平台");
        assert_eq!(
            tw("我的账号要先报账，再转账结账"),
            "我的帳號要先報帳，再轉帳結帳"
        );
    }

    #[test]
    fn the_week_is_written_the_taiwan_way() {
        assert_eq!(
            tw("这周跟下周都要开会，周末休息"),
            "這週跟下週都要開會，週末休息"
        );
        assert_eq!(
            weeks("這周三交，上上周說過，每周一次"),
            "這週三交，上上週說過，每週一次"
        );
        assert_eq!(
            weeks("周休二日，周六周日不上班"),
            "週休二日，週六週日不上班"
        );
        assert_eq!(weeks("我這兩周比較忙"), "我這兩週比較忙");
    }

    #[test]
    fn other_meanings_of_zhou_are_left_alone() {
        for s in [
            "周圍很安靜",
            "環顧四周",
            "周先生和周杰倫",
            "考慮得很周到",
            "繞場一周",
        ] {
            assert_eq!(weeks(s), s);
        }
        assert_eq!(weeks("no week here"), "no week here");
    }

    #[test]
    fn english_is_untouched() {
        assert_eq!(
            tw("我今天在测试 Moqi 的新版本，repo 在 GitHub"),
            "我今天在測試 Moqi 的新版本，repo 在 GitHub"
        );
    }
}
