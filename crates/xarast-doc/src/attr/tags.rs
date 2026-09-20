//! The `.xar` attribute tag to [`AttrSlot`] reconciliation.
//!
//! `docs/phases/phase-02-document-model.md` requires that **every `.xar`
//! attribute tag map to exactly one slot, or be recorded as multi-applicable
//! or ignorable**, and that the mapping be a checked-in table rather than
//! knowledge in someone's head. This is that table; the same list is in
//! `docs/memory/document-model.md` in prose.
//!
//! The tag numbers and their meanings are facts taken from
//! `docs/research/01-xar-format.md` §8 and §4.12. Phase 3 reads this table;
//! it does not get to invent its own.
//!
//! The reconciliation did **not** change the slot count: it came out at the
//! 46 slots `research/02 §10.6` proposed.

use super::AttrSlot;

/// Where a `.xar` attribute tag lands in the model.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum TagMapping {
    /// It replaces the value in this slot.
    Slot(AttrSlot),
    /// It accumulates instead of replacing: it occupies no slot.
    Multi,
    /// We read it and drop it on purpose.
    Ignorable,
}

/// Every `.xar` attribute tag, and where it goes.
///
/// Entries are `(tag, mapping, name)`. The `name` is the format's own, kept so
/// that a reader of this table does not need the format document open.
pub const XAR_ATTRIBUTE_TAGS: &[(u16, TagMapping, &str)] = &[
    // ── §8.2 line attributes ────────────────────────────────────────────────
    (151, TagMapping::Slot(AttrSlot::StrokeColour), "LINECOLOUR"),
    (
        193,
        TagMapping::Slot(AttrSlot::StrokeColour),
        "LINECOLOUR_NONE",
    ),
    (
        194,
        TagMapping::Slot(AttrSlot::StrokeColour),
        "LINECOLOUR_BLACK",
    ),
    (
        195,
        TagMapping::Slot(AttrSlot::StrokeColour),
        "LINECOLOUR_WHITE",
    ),
    (152, TagMapping::Slot(AttrSlot::LineWidth), "LINEWIDTH"),
    (174, TagMapping::Slot(AttrSlot::StartCap), "STARTCAP"),
    // The format writes the caps as a pair; the model, like the original, has
    // one cap style, so the end cap lands in the same slot.
    (175, TagMapping::Slot(AttrSlot::StartCap), "ENDCAP"),
    (176, TagMapping::Slot(AttrSlot::JoinType), "JOINSTYLE"),
    (177, TagMapping::Slot(AttrSlot::MitreLimit), "MITRELIMIT"),
    (178, TagMapping::Slot(AttrSlot::WindingRule), "WINDINGRULE"),
    (
        173,
        TagMapping::Slot(AttrSlot::StrokeTransp),
        "LINETRANSPARENCY",
    ),
    (179, TagMapping::Slot(AttrSlot::Quality), "QUALITY"),
    // ── §8.3 colour fills ───────────────────────────────────────────────────
    (150, TagMapping::Slot(AttrSlot::FillGeometry), "FLATFILL"),
    (
        190,
        TagMapping::Slot(AttrSlot::FillGeometry),
        "FLATFILL_NONE",
    ),
    (
        191,
        TagMapping::Slot(AttrSlot::FillGeometry),
        "FLATFILL_BLACK",
    ),
    (
        192,
        TagMapping::Slot(AttrSlot::FillGeometry),
        "FLATFILL_WHITE",
    ),
    (153, TagMapping::Slot(AttrSlot::FillGeometry), "LINEARFILL"),
    (
        4121,
        TagMapping::Slot(AttrSlot::FillGeometry),
        "LINEARFILL3POINT",
    ),
    (
        154,
        TagMapping::Slot(AttrSlot::FillGeometry),
        "CIRCULARFILL",
    ),
    (
        155,
        TagMapping::Slot(AttrSlot::FillGeometry),
        "ELLIPTICALFILL",
    ),
    (156, TagMapping::Slot(AttrSlot::FillGeometry), "CONICALFILL"),
    (200, TagMapping::Slot(AttrSlot::FillGeometry), "SQUAREFILL"),
    (
        202,
        TagMapping::Slot(AttrSlot::FillGeometry),
        "THREECOLFILL",
    ),
    (204, TagMapping::Slot(AttrSlot::FillGeometry), "FOURCOLFILL"),
    (157, TagMapping::Slot(AttrSlot::FillGeometry), "BITMAPFILL"),
    (
        158,
        TagMapping::Slot(AttrSlot::FillGeometry),
        "CONTONEBITMAPFILL",
    ),
    (159, TagMapping::Slot(AttrSlot::FillGeometry), "FRACTALFILL"),
    (4010, TagMapping::Slot(AttrSlot::FillGeometry), "NOISEFILL"),
    (
        4075,
        TagMapping::Slot(AttrSlot::FillGeometry),
        "LINEARFILLMULTISTAGE",
    ),
    (
        4076,
        TagMapping::Slot(AttrSlot::FillGeometry),
        "CIRCULARFILLMULTISTAGE",
    ),
    (
        4077,
        TagMapping::Slot(AttrSlot::FillGeometry),
        "ELLIPTICALFILLMULTISTAGE",
    ),
    (
        4078,
        TagMapping::Slot(AttrSlot::FillGeometry),
        "CONICALFILLMULTISTAGE",
    ),
    (
        4088,
        TagMapping::Slot(AttrSlot::FillGeometry),
        "SQUAREFILLMULTISTAGE",
    ),
    (
        4122,
        TagMapping::Slot(AttrSlot::FillGeometry),
        "LINEARFILL3POINTMULTISTAGE",
    ),
    (
        163,
        TagMapping::Slot(AttrSlot::FillMapping),
        "FILL_REPEATING",
    ),
    (
        164,
        TagMapping::Slot(AttrSlot::FillMapping),
        "FILL_NONREPEATING",
    ),
    (
        165,
        TagMapping::Slot(AttrSlot::FillMapping),
        "FILL_REPEATINGINVERTED",
    ),
    (
        206,
        TagMapping::Slot(AttrSlot::FillMapping),
        "FILL_REPEATING_EXTRA",
    ),
    (
        160,
        TagMapping::Slot(AttrSlot::FillEffect),
        "FILLEFFECT_FADE",
    ),
    (
        161,
        TagMapping::Slot(AttrSlot::FillEffect),
        "FILLEFFECT_RAINBOW",
    ),
    (
        162,
        TagMapping::Slot(AttrSlot::FillEffect),
        "FILLEFFECT_ALTRAINBOW",
    ),
    // ── §8.4 transparencies ─────────────────────────────────────────────────
    (
        166,
        TagMapping::Slot(AttrSlot::TranspFillGeometry),
        "FLATTRANSPARENTFILL",
    ),
    (
        167,
        TagMapping::Slot(AttrSlot::TranspFillGeometry),
        "LINEARTRANSPARENTFILL",
    ),
    (
        4123,
        TagMapping::Slot(AttrSlot::TranspFillGeometry),
        "LINEARTRANSPARENTFILL3POINT",
    ),
    (
        168,
        TagMapping::Slot(AttrSlot::TranspFillGeometry),
        "CIRCULARTRANSPARENTFILL",
    ),
    (
        169,
        TagMapping::Slot(AttrSlot::TranspFillGeometry),
        "ELLIPTICALTRANSPARENTFILL",
    ),
    (
        170,
        TagMapping::Slot(AttrSlot::TranspFillGeometry),
        "CONICALTRANSPARENTFILL",
    ),
    (
        201,
        TagMapping::Slot(AttrSlot::TranspFillGeometry),
        "SQUARETRANSPARENTFILL",
    ),
    (
        203,
        TagMapping::Slot(AttrSlot::TranspFillGeometry),
        "THREECOLTRANSPARENTFILL",
    ),
    (
        205,
        TagMapping::Slot(AttrSlot::TranspFillGeometry),
        "FOURCOLTRANSPARENTFILL",
    ),
    (
        171,
        TagMapping::Slot(AttrSlot::TranspFillGeometry),
        "BITMAPTRANSPARENTFILL",
    ),
    (
        172,
        TagMapping::Slot(AttrSlot::TranspFillGeometry),
        "FRACTALTRANSPARENTFILL",
    ),
    (
        4011,
        TagMapping::Slot(AttrSlot::TranspFillGeometry),
        "NOISETRANSPARENTFILL",
    ),
    (
        180,
        TagMapping::Slot(AttrSlot::TranspFillMapping),
        "TRANSPFILL_REPEATING",
    ),
    (
        181,
        TagMapping::Slot(AttrSlot::TranspFillMapping),
        "TRANSPFILL_NONREPEATING",
    ),
    (
        182,
        TagMapping::Slot(AttrSlot::TranspFillMapping),
        "TRANSPFILL_REPEATINGINVERTED",
    ),
    (
        207,
        TagMapping::Slot(AttrSlot::TranspFillMapping),
        "TRANSPFILL_REPEATING_EXTRA",
    ),
    // ── §8.5 dashes, §8.6 arrows, §8.7 miscellany ───────────────────────────
    (183, TagMapping::Slot(AttrSlot::DashPattern), "DASHSTYLE"),
    (184, TagMapping::Slot(AttrSlot::DashPattern), "DEFINEDASH"),
    (
        188,
        TagMapping::Slot(AttrSlot::DashPattern),
        "DEFINEDASH_SCALED",
    ),
    (185, TagMapping::Slot(AttrSlot::StartArrow), "ARROWHEAD"),
    (186, TagMapping::Slot(AttrSlot::EndArrow), "ARROWTAIL"),
    // A user value is multi-applicable, *except* when its key is the
    // web-address resource string, which is the format's old-style hyperlink
    // and goes to the `WebAddress` slot. The importer looks at the key; the
    // tag itself maps to `Multi`.
    (189, TagMapping::Multi, "USERVALUE"),
    (4086, TagMapping::Slot(AttrSlot::Feather), "FEATHER"),
    // The 3500–3505 block is the imagesetting overprint attributes. The
    // research records the range and that the records are empty; which of the
    // six is the "on" and which the "off" of each pair is confirmed in
    // Phase 3, against the corpus.
    (
        3500,
        TagMapping::Slot(AttrSlot::OverprintLine),
        "OVERPRINTLINE_ON",
    ),
    (
        3501,
        TagMapping::Slot(AttrSlot::OverprintLine),
        "OVERPRINTLINE_OFF",
    ),
    (
        3502,
        TagMapping::Slot(AttrSlot::OverprintFill),
        "OVERPRINTFILL_ON",
    ),
    (
        3503,
        TagMapping::Slot(AttrSlot::OverprintFill),
        "OVERPRINTFILL_OFF",
    ),
    (
        3504,
        TagMapping::Slot(AttrSlot::PrintOnAllPlates),
        "PRINTONALLPLATES_ON",
    ),
    (
        3505,
        TagMapping::Slot(AttrSlot::PrintOnAllPlates),
        "PRINTONALLPLATES_OFF",
    ),
    // ── §4.12.4 text attributes ─────────────────────────────────────────────
    (
        2900,
        TagMapping::Slot(AttrSlot::TxtLineSpace),
        "TEXT_LINESPACE_RATIO",
    ),
    (
        2901,
        TagMapping::Slot(AttrSlot::TxtLineSpace),
        "TEXT_LINESPACE_ABSOLUTE",
    ),
    (
        2902,
        TagMapping::Slot(AttrSlot::TxtJustification),
        "TEXT_JUSTIFICATION_LEFT",
    ),
    (
        2903,
        TagMapping::Slot(AttrSlot::TxtJustification),
        "TEXT_JUSTIFICATION_CENTRE",
    ),
    (
        2904,
        TagMapping::Slot(AttrSlot::TxtJustification),
        "TEXT_JUSTIFICATION_RIGHT",
    ),
    (
        2905,
        TagMapping::Slot(AttrSlot::TxtJustification),
        "TEXT_JUSTIFICATION_FULL",
    ),
    (
        2906,
        TagMapping::Slot(AttrSlot::TxtFontSize),
        "TEXT_FONT_SIZE",
    ),
    (
        2907,
        TagMapping::Slot(AttrSlot::TxtFontTypeface),
        "TEXT_FONT_TYPEFACE",
    ),
    (2908, TagMapping::Slot(AttrSlot::TxtBold), "TEXT_BOLD_ON"),
    (2909, TagMapping::Slot(AttrSlot::TxtBold), "TEXT_BOLD_OFF"),
    (
        2910,
        TagMapping::Slot(AttrSlot::TxtItalic),
        "TEXT_ITALIC_ON",
    ),
    (
        2911,
        TagMapping::Slot(AttrSlot::TxtItalic),
        "TEXT_ITALIC_OFF",
    ),
    (
        2912,
        TagMapping::Slot(AttrSlot::TxtUnderline),
        "TEXT_UNDERLINE_ON",
    ),
    (
        2913,
        TagMapping::Slot(AttrSlot::TxtUnderline),
        "TEXT_UNDERLINE_OFF",
    ),
    (
        2914,
        TagMapping::Slot(AttrSlot::TxtScript),
        "TEXT_SCRIPT_ON",
    ),
    (
        2915,
        TagMapping::Slot(AttrSlot::TxtScript),
        "TEXT_SCRIPT_OFF",
    ),
    (
        2916,
        TagMapping::Slot(AttrSlot::TxtScript),
        "TEXT_SUPERSCRIPT_ON",
    ),
    (
        2917,
        TagMapping::Slot(AttrSlot::TxtScript),
        "TEXT_SUBSCRIPT_ON",
    ),
    (
        2918,
        TagMapping::Slot(AttrSlot::TxtTracking),
        "TEXT_TRACKING",
    ),
    (
        2919,
        TagMapping::Slot(AttrSlot::TxtAspectRatio),
        "TEXT_ASPECT_RATIO",
    ),
    (
        2920,
        TagMapping::Slot(AttrSlot::TxtBaseline),
        "TEXT_BASELINE",
    ),
    (
        4201,
        TagMapping::Slot(AttrSlot::TxtLeftMargin),
        "TEXT_LEFT_INDENT",
    ),
    (
        4202,
        TagMapping::Slot(AttrSlot::TxtFirstIndent),
        "TEXT_FIRST_INDENT",
    ),
    (
        4203,
        TagMapping::Slot(AttrSlot::TxtRightMargin),
        "TEXT_RIGHT_INDENT",
    ),
    (4204, TagMapping::Slot(AttrSlot::TxtRuler), "TEXT_RULER"),
];

/// The slots no tag in `research/01 §8` or `§4.12` reaches.
///
/// They are not orphans: each is fed by a record in a category the attribute
/// sections do not cover — §4.8 (containers and effects) and §4.10 (brushes
/// and strokes) — or, for `WebAddress`, by a `USERVALUE` with a particular
/// key. Phase 3 wires them up; recording them here is what makes "every slot
/// is accounted for" a checkable statement rather than a hope.
pub const SLOTS_WITHOUT_ATTRIBUTE_TAG: &[AttrSlot] = &[
    // Reached by TAG_USERVALUE (189) with the web-address key.
    AttrSlot::WebAddress,
    // §4.10 brushes and strokes.
    AttrSlot::StrokeType,
    AttrSlot::VariableWidth,
    AttrSlot::BrushType,
    // §4.8 containers and effects.
    AttrSlot::BevelIndent,
    AttrSlot::BevelType,
    AttrSlot::BevelContrast,
    AttrSlot::BevelLightAngle,
    AttrSlot::BevelLightTilt,
    AttrSlot::ClipRegion,
    AttrSlot::ClipView,
];

/// Where a tag goes, if we know.
#[must_use]
pub fn mapping_for(tag: u16) -> Option<TagMapping> {
    XAR_ATTRIBUTE_TAGS
        .iter()
        .find(|(t, _, _)| *t == tag)
        .map(|(_, m, _)| *m)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::attr::{ALL_ATTR_SLOTS, ATTR_SLOT_COUNT};

    #[test]
    fn the_table_maps_each_tag_once() {
        let mut tags: Vec<u16> = XAR_ATTRIBUTE_TAGS.iter().map(|(t, _, _)| *t).collect();
        let before = tags.len();
        tags.sort_unstable();
        tags.dedup();
        assert_eq!(before, tags.len(), "a tag appears twice in the table");
    }

    #[test]
    fn every_slot_is_accounted_for_exactly_once() {
        let mut covered = [false; ATTR_SLOT_COUNT];
        for (_, m, name) in XAR_ATTRIBUTE_TAGS {
            match m {
                TagMapping::Slot(s) => covered[*s as usize] = true,
                TagMapping::Multi | TagMapping::Ignorable => {
                    assert!(!name.is_empty());
                }
            }
        }
        for s in SLOTS_WITHOUT_ATTRIBUTE_TAG {
            assert!(
                !covered[*s as usize],
                "{s:?} is listed as having no tag, but the table maps one to it"
            );
            covered[*s as usize] = true;
        }
        let missing: Vec<AttrSlot> = ALL_ATTR_SLOTS
            .into_iter()
            .filter(|s| !covered[*s as usize])
            .collect();
        assert!(
            missing.is_empty(),
            "these slots are reached by no tag and are not listed as tagless: {missing:?}"
        );
    }

    #[test]
    fn the_reconciliation_left_the_slot_count_at_46() {
        assert_eq!(ATTR_SLOT_COUNT, 46);
    }

    #[test]
    fn a_few_spot_checks_against_the_format_document() {
        assert_eq!(
            mapping_for(152),
            Some(TagMapping::Slot(AttrSlot::LineWidth))
        );
        assert_eq!(mapping_for(189), Some(TagMapping::Multi));
        assert_eq!(
            mapping_for(175),
            Some(TagMapping::Slot(AttrSlot::StartCap)),
            "the format's separate end cap shares the original's one cap style"
        );
        assert_eq!(mapping_for(9_999), None);
    }
}
