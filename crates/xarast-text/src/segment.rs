//! Word segmentation for caret movement and double-click selection
//! (phase 9, T9.4.4, T9.4.5).
//!
//! UAX #29 word boundaries from ICU4X, with the dictionary segmenter for
//! scripts written without spaces when the `complex-scripts` feature is on
//! (the same data line breaking uses). Nothing here names an ICU type.

use std::ops::Range;

/// One segment between two word boundaries.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct WordSegment {
    /// Byte range in the text.
    pub range: Range<usize>,
    /// Letters, digits or ideographs: a word a caret jumps to. Spaces and
    /// punctuation are not.
    pub word_like: bool,
}

/// Splits `text` at its word boundaries. The segments cover the text
/// without gaps, in order; an empty text has none.
#[must_use]
pub fn word_segments(text: &str) -> Vec<WordSegment> {
    #[cfg(feature = "complex-scripts")]
    let seg = icu_segmenter::WordSegmenter::new_dictionary(
        icu_segmenter::options::WordBreakInvariantOptions::default(),
    );
    #[cfg(not(feature = "complex-scripts"))]
    let seg = icu_segmenter::WordSegmenter::new_for_non_complex_scripts(
        icu_segmenter::options::WordBreakInvariantOptions::default(),
    );
    let mut out = Vec::new();
    let mut prev = 0usize;
    for (at, kind) in seg.segment_str(text).iter_with_word_type() {
        if at > prev {
            out.push(WordSegment {
                range: prev..at,
                word_like: kind.is_word_like(),
            });
        }
        prev = at;
    }
    out
}

/// The grapheme cluster boundary before `at`: where Backspace deletes back
/// to (UAX #29 extended grapheme clusters, so a base with its marks, an
/// emoji sequence or a CRLF goes as one). 0 at the start of the text.
#[must_use]
pub fn prev_grapheme(text: &str, at: usize) -> usize {
    let at = at.min(text.len());
    icu_segmenter::GraphemeClusterSegmenter::new()
        .segment_str(text)
        .take_while(|&b| b < at)
        .last()
        .unwrap_or(0)
}

/// The grapheme cluster boundary after `at`: where Delete deletes up to.
/// The text's length at its end.
#[must_use]
pub fn next_grapheme(text: &str, at: usize) -> usize {
    icu_segmenter::GraphemeClusterSegmenter::new()
        .segment_str(text)
        .find(|&b| b > at)
        .unwrap_or(text.len())
        .min(text.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn words_spaces_and_punctuation_are_told_apart() {
        let s = word_segments("Hello, big world");
        let words: Vec<&str> = s
            .iter()
            .filter(|w| w.word_like)
            .map(|w| &"Hello, big world"[w.range.clone()])
            .collect();
        assert_eq!(words, ["Hello", "big", "world"]);
        assert_eq!(s.first().unwrap().range.start, 0);
        assert_eq!(s.last().unwrap().range.end, 16);
        assert!(s.windows(2).all(|w| w[0].range.end == w[1].range.start));
        assert!(word_segments("").is_empty());
    }

    #[test]
    fn hebrew_words_are_words() {
        let t = "שלום עולם";
        let n = word_segments(t).iter().filter(|w| w.word_like).count();
        assert_eq!(n, 2);
    }

    #[test]
    fn graphemes_step_over_marks_and_emoji_sequences() {
        // "e" + combining acute, a family emoji (a ZWJ sequence), "x".
        let t = "e\u{301}\u{1F468}\u{200D}\u{1F469}\u{200D}\u{1F467}x";
        let family = 3 * 4 + 2 * 3;
        assert_eq!(next_grapheme(t, 0), 3);
        assert_eq!(next_grapheme(t, 3), 3 + family);
        assert_eq!(prev_grapheme(t, 3 + family), 3);
        assert_eq!(prev_grapheme(t, 3), 0);
        assert_eq!(prev_grapheme(t, 0), 0);
        assert_eq!(next_grapheme(t, t.len()), t.len());
        assert_eq!(next_grapheme("a\r\nb", 1), 3);
    }
}
