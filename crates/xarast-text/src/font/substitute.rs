//! The missing-font substitution ladder of `docs/phases/phase-09-text.md`
//! W9.1: pure string and classification rules, no font access.
//!
//! The alias table is **written by us from public metric-compatibility
//! facts** — the Liberation, Croscore and URW families were each designed and
//! published as metric-compatible replacements for the named Microsoft/Adobe
//! faces. It is not transcribed from Xara's PANOSE tables or from any other
//! program's configuration.

/// Collapses runs of whitespace and trims, so `" Times  New Roman "` and
/// `"Times New Roman"` name the same family. Case is left alone: fontique's
/// name lookup is already case-insensitive.
pub(crate) fn normalise(name: &str) -> String {
    name.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// What a stripped style suffix implied.
#[derive(Copy, Clone, PartialEq, Eq, Debug, Default)]
pub(crate) struct StyleHint {
    /// A weight named by the suffix, when it named one.
    pub weight: Option<u16>,
    /// Whether the suffix named an italic or oblique.
    pub italic: bool,
}

/// Style words that can trail a family name in an old document
/// (`"Arial Bold"`, `"Times New Roman Bold Italic"`), with the weight they
/// imply. Lower case. "Roman" is deliberately absent: it ends real family
/// names ("Times New Roman").
const STYLE_WORDS: &[(&str, Option<u16>, bool)] = &[
    ("thin", Some(100), false),
    ("hairline", Some(100), false),
    ("extralight", Some(200), false),
    ("ultralight", Some(200), false),
    ("light", Some(300), false),
    ("book", Some(400), false),
    ("regular", Some(400), false),
    ("normal", Some(400), false),
    ("plain", Some(400), false),
    ("medium", Some(500), false),
    ("semibold", Some(600), false),
    ("demibold", Some(600), false),
    ("demi", Some(600), false),
    ("bold", Some(700), false),
    ("extrabold", Some(800), false),
    ("ultrabold", Some(800), false),
    ("heavy", Some(900), false),
    ("black", Some(900), false),
    ("italic", None, true),
    ("oblique", None, true),
    ("slanted", None, true),
    ("inclined", None, true),
];

/// Strips trailing style words. Returns `None` when nothing was stripped or
/// when stripping would leave nothing (a family literally called "Bold").
pub(crate) fn strip_style_suffix(name: &str) -> Option<(String, StyleHint)> {
    let mut words: Vec<&str> = name.split_whitespace().collect();
    let mut hint = StyleHint::default();
    let mut stripped = false;
    while words.len() > 1 {
        let Some(last) = words.last() else { break };
        let lower = last.to_ascii_lowercase();
        let Some(&(_, weight, italic)) = STYLE_WORDS.iter().find(|(w, _, _)| *w == lower) else {
            break;
        };
        // Words are stripped from the end; the first weight word met wins.
        if hint.weight.is_none() {
            hint.weight = weight;
        }
        hint.italic |= italic;
        words.pop();
        stripped = true;
    }
    stripped.then(|| (words.join(" "), hint))
}

/// Groups of metric-compatible families. Every member of a group has the same
/// advance widths as every other for the characters the original covers, so
/// substituting within a group keeps line breaks where the document had them.
const METRIC_ALIASES: &[&[&str]] = &[
    &[
        "Arial",
        "Helvetica",
        "Liberation Sans",
        "Arimo",
        "Nimbus Sans",
        "Nimbus Sans L",
        "TeX Gyre Heros",
        "FreeSans",
    ],
    &[
        "Arial Narrow",
        "Liberation Sans Narrow",
        "Nimbus Sans Narrow",
    ],
    &[
        "Times New Roman",
        "Times",
        "Liberation Serif",
        "Tinos",
        "Nimbus Roman",
        "Nimbus Roman No9 L",
        "TeX Gyre Termes",
        "FreeSerif",
    ],
    &[
        "Courier New",
        "Courier",
        "Liberation Mono",
        "Cousine",
        "Nimbus Mono PS",
        "Nimbus Mono L",
        "TeX Gyre Cursor",
        "FreeMono",
    ],
    &["Calibri", "Carlito"],
    &["Cambria", "Caladea"],
    &["Georgia", "Gelasio"],
];

/// The other members of `family`'s metric-compatibility group, in preference
/// order. Empty when the family is in no group.
pub(crate) fn metric_aliases(family: &str) -> impl Iterator<Item = &'static str> + '_ {
    METRIC_ALIASES
        .iter()
        .find(|g| g.iter().any(|m| m.eq_ignore_ascii_case(family)))
        .into_iter()
        .flat_map(|g| g.iter().copied())
        .filter(move |m| !m.eq_ignore_ascii_case(family))
}

/// The generic family a missing face most likely belongs to.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub(crate) enum Generic {
    SansSerif,
    Serif,
    Monospace,
}

/// Classifies a missing face. PANOSE first, when the document carried it
/// (public PANOSE 1.0 digit meanings: digit 1 = family kind, 2 = Latin text;
/// digit 2 = serif style, 11-13 are the sans-serif styles; digit 4 =
/// proportion, 9 = monospaced), then the name.
pub(crate) fn classify(family: &str, panose: Option<[u8; 10]>) -> Generic {
    if let Some(p) = panose
        && p[0] == 2
    {
        if p[3] == 9 {
            return Generic::Monospace;
        }
        match p[1] {
            11..=13 => return Generic::SansSerif,
            2..=10 | 14..=15 => return Generic::Serif,
            _ => {}
        }
    }
    let lower = family.to_ascii_lowercase();
    let has = |s: &str| lower.contains(s);
    if has("mono") || has("courier") || has("console") || has("typewriter") || has("code") {
        Generic::Monospace
    } else if has("sans") || has("gothic") || has("grotesk") || has("arial") || has("helvet") {
        Generic::SansSerif
    } else if has("serif")
        || has("times")
        || has("roman")
        || has("garamond")
        || has("georgia")
        || has("book")
        || has("bodoni")
        || has("palatino")
        || has("century")
    {
        Generic::Serif
    } else {
        Generic::SansSerif
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalise_collapses_whitespace() {
        assert_eq!(normalise("  Times   New\tRoman "), "Times New Roman");
    }

    #[test]
    fn style_suffixes_are_stripped_with_their_meaning() {
        let (f, h) = strip_style_suffix("Arial Bold").unwrap();
        assert_eq!(f, "Arial");
        assert_eq!(h.weight, Some(700));
        assert!(!h.italic);
        let (f, h) = strip_style_suffix("Times New Roman Bold Italic").unwrap();
        assert_eq!(f, "Times New Roman");
        assert_eq!(h.weight, Some(700));
        assert!(h.italic);
        assert!(strip_style_suffix("Arial").is_none());
        assert!(
            strip_style_suffix("Bold").is_none(),
            "never strip to nothing"
        );
    }

    #[test]
    fn aliases_exclude_the_family_itself() {
        let a: Vec<_> = metric_aliases("arial").collect();
        assert!(a.contains(&"Liberation Sans"));
        assert!(!a.iter().any(|m| m.eq_ignore_ascii_case("arial")));
        assert_eq!(metric_aliases("Comic Sans MS").count(), 0);
    }

    #[test]
    fn classification_prefers_panose() {
        let sans = [2, 11, 6, 4, 2, 2, 2, 2, 2, 4];
        let serif = [2, 2, 6, 3, 5, 4, 5, 2, 3, 4];
        let mono = [2, 7, 3, 9, 2, 2, 5, 2, 4, 4];
        assert_eq!(classify("Mystery", Some(sans)), Generic::SansSerif);
        assert_eq!(classify("Mystery", Some(serif)), Generic::Serif);
        assert_eq!(classify("Mystery", Some(mono)), Generic::Monospace);
        assert_eq!(classify("Old Typewriter", None), Generic::Monospace);
        assert_eq!(classify("Garamond Premier", None), Generic::Serif);
        assert_eq!(classify("Frutiger", None), Generic::SansSerif);
    }
}
