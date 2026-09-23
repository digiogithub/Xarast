//! Unicode scripts, named by their ISO 15924 four-letter code.

use icu_properties::props::Script;
use icu_properties::{CodePointMapData, PropertyNamesShort};

/// An ISO 15924 script code, for example `Arab`, `Hebr`, `Hani`, `Latn`.
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
pub struct ScriptTag(pub [u8; 4]);

impl ScriptTag {
    /// Latin.
    pub const LATIN: ScriptTag = ScriptTag(*b"Latn");
    /// Arabic.
    pub const ARABIC: ScriptTag = ScriptTag(*b"Arab");
    /// Hebrew.
    pub const HEBREW: ScriptTag = ScriptTag(*b"Hebr");
    /// Han ideographs.
    pub const HAN: ScriptTag = ScriptTag(*b"Hani");
    /// Hiragana.
    pub const HIRAGANA: ScriptTag = ScriptTag(*b"Hira");
    /// Katakana.
    pub const KATAKANA: ScriptTag = ScriptTag(*b"Kana");
    /// Common (punctuation, digits, spaces shared by every script).
    pub const COMMON: ScriptTag = ScriptTag(*b"Zyyy");
    /// Inherited (combining marks that take the script of their base).
    pub const INHERITED: ScriptTag = ScriptTag(*b"Zinh");

    /// The Unicode `Script` property of `c`.
    #[must_use]
    pub fn of(c: char) -> ScriptTag {
        let script = CodePointMapData::<Script>::new().get(c);
        PropertyNamesShort::<Script>::new()
            .get_locale_script(script)
            .map(|s| {
                let b = s.as_str().as_bytes();
                let mut out = *b"Zzzz";
                for (o, i) in out.iter_mut().zip(b) {
                    *o = *i;
                }
                ScriptTag(out)
            })
            .unwrap_or(ScriptTag(*b"Zzzz"))
    }

    pub(crate) fn to_fontique(self) -> fontique::Script {
        fontique::Script::from_bytes(self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scripts_of_sample_characters() {
        assert_eq!(ScriptTag::of('a'), ScriptTag::LATIN);
        assert_eq!(ScriptTag::of('ש'), ScriptTag::HEBREW);
        assert_eq!(ScriptTag::of('ب'), ScriptTag::ARABIC);
        assert_eq!(ScriptTag::of('漢'), ScriptTag::HAN);
        assert_eq!(ScriptTag::of('の'), ScriptTag::HIRAGANA);
        assert_eq!(ScriptTag::of(' '), ScriptTag::COMMON);
        assert_eq!(ScriptTag::of('\u{0301}'), ScriptTag::INHERITED);
    }
}
