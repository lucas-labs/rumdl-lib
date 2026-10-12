use regex::Regex;
use std::sync::LazyLock;

/// Whether `c` is a letter of a Chinese, Japanese or Korean script.
///
/// Letters only: Han ideographs (`中`, the iteration mark `々`, `〇`), kana
/// including half-width forms and the prolonged sound mark (`ー`), and Hangul
/// syllables and jamo. CJK punctuation (`。`, `「`, `・`), full-width Latin
/// letters and digits (`Ｔ`, `１`) and bopomofo are not CJK letters. Neither are
/// combining marks such as the kana voiced sound mark (`\u{3099}`): a mark
/// renders as part of the letter it follows, so it is that letter's business.
pub(crate) fn is_cjk_letter(c: char) -> bool {
    let cp = c as u32;
    // Below U+1100 only U+0305 COMBINING OVERLINE and U+0323 COMBINING DOT BELOW match
    // the class, carrying Katakana script extensions. These combining marks attach to
    // whatever precedes them, so returning false here is deliberate to avoid splitting
    // grapheme clusters.
    if cp < 0x1100 {
        return false;
    }
    // The three dense blocks contain nothing but letters; answer without the regex.
    if matches!(cp, 0x4E00..=0x9FFF | 0x3400..=0x4DBF | 0xAC00..=0xD7A3) {
        return true;
    }
    // Script extensions catch characters shared across CJK scripts (`々`, `ー`);
    // subtracting punctuation, symbols and combining marks leaves letters and
    // numerals.
    static CJK_LETTER: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"^[\p{scx=Han}\p{scx=Hiragana}\p{scx=Katakana}\p{scx=Hangul}--\p{P}--\p{S}--\p{Mn}--\p{Me}]$")
            .expect("CJK letter class is a valid regex")
    });
    let mut buf = [0u8; 4];
    CJK_LETTER.is_match(c.encode_utf8(&mut buf))
}

/// Whether `c` belongs to text that CJK-aware renderers join without a space
/// across a soft line break: a Han or kana letter, or CJK punctuation.
///
/// Hangul is excluded because Korean separates words with spaces. The
/// punctuation is the kind only CJK text uses (`。`, `，`, `「`, `・`):
/// ideographic, katakana, and full-width or half-width forms. Marks Western text shares,
/// such as `“`, `…` and `—`, are left out, so a break next to one keeps its
/// space. So is the ideographic space (`\u{3000}`), which is whitespace the
/// author wrote.
pub(crate) fn joins_cjk_soft_break(c: char) -> bool {
    if is_cjk_letter(c) {
        static HANGUL: LazyLock<Regex> =
            LazyLock::new(|| Regex::new(r"^\p{sc=Hangul}$").expect("Hangul class is a valid regex"));
        let mut buf = [0u8; 4];
        return !HANGUL.is_match(c.encode_utf8(&mut buf));
    }
    let cp = c as u32;
    // CJK Symbols and Punctuation, the katakana middle dot and double hyphen, the
    // vertical and compatibility forms, and the full-width and half-width forms,
    // each limited to its punctuation.
    if !matches!(
        cp,
        0x3001..=0x303F | 0x30A0 | 0x30FB | 0xFE10..=0xFE1F | 0xFE30..=0xFE4F | 0xFF01..=0xFF65
    ) {
        return false;
    }
    static PUNCTUATION: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"^\p{P}$").expect("punctuation class is a valid regex"));
    let mut buf = [0u8; 4];
    PUNCTUATION.is_match(c.encode_utf8(&mut buf))
}
