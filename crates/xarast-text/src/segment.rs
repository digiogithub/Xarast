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
}
