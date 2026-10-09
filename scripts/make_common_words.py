"""Regenerate src-tauri/src/yuyin/data/common_words.txt.

Needs jieba's dict.txt (https://github.com/fxsjy/jieba/blob/master/jieba/dict.txt,
MIT License) and the opencc Python package:

    python3 scripts/make_common_words.py path/to/dict.txt src-tauri/src/yuyin/data/common_words.txt
"""

import re
import sys

import opencc

MIN = 2000
HAN = re.compile(r"^[一-鿿]{2,}$")


def main(src: str, out: str) -> None:
    words = []
    for line in open(src, encoding="utf-8"):
        parts = line.split()
        if len(parts) >= 2 and HAN.match(parts[0]) and int(parts[1]) >= MIN:
            words.append(parts[0])
    s2t, s2twp = opencc.OpenCC("s2t"), opencc.OpenCC("s2twp")
    forms = set()
    for word in words:
        forms.update({word, s2t.convert(word), s2twp.convert(word)})
    forms = sorted(f for f in forms if HAN.match(f))
    header = f"""# Common Chinese words (learn.rs `common_word`): a same-sound correction of one
# of these is a rewording more often than a mishearing, so it is learned only
# once the user has made it twice.
# Every word of two or more characters with a frequency of {MIN} or more in
# jieba's dict.txt ({len(words)} words), in Simplified, Traditional (s2t) and
# Taiwan (s2twp) forms. Regenerate with scripts/make_common_words.py.
# jieba: https://github.com/fxsjy/jieba, MIT License, Copyright (c) 2013 Sun Junyi.
"""
    with open(out, "w", encoding="utf-8") as f:
        f.write(header + "\n".join(forms) + "\n")
    print(len(words), "words,", len(forms), "forms")


if __name__ == "__main__":
    main(sys.argv[1], sys.argv[2])
