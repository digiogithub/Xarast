//! OpenType feature settings on text (`AttrValue::FontFeatures`, the text
//! tool's feature panel, T9.4.9) survive a `.xarast` round trip as
//! `xarast:features`, and a run without them writes nothing.

use std::io::Cursor;
use std::sync::Arc;

use xarast_doc::{
    AttrSlot, AttrValue, BuildLimits, Document, FeatureSetting, NodeKind, StoryText, TextItem,
};
use xarast_format::svg::{SvgOptions, normal_form};
use xarast_format::{OpenOptions, SaveOptions, WriteOptions, open_reader, save_opened_to, save_to};

fn doc() -> Document {
    let mut b = xarast_doc::builder::skeleton(BuildLimits::default()).unwrap();
    b.node(NodeKind::TextStory(Box::default())).unwrap();
    b.push_scope().unwrap();
    b.node(NodeKind::TextLine(Box::default())).unwrap();
    b.push_scope().unwrap();
    for c in "ab".chars() {
        b.node(NodeKind::TextItem(TextItem::Char(c))).unwrap();
    }
    b.attribute(AttrValue::FontFeatures(FeatureSetting::normalised(&[
        FeatureSetting::new("smcp", 1).unwrap(),
        FeatureSetting::new("liga", 0).unwrap(),
    ])))
    .unwrap();
    for c in "cd".chars() {
        b.node(NodeKind::TextItem(TextItem::Char(c))).unwrap();
    }
    b.node(NodeKind::TextItem(TextItem::LineBreak(true)))
        .unwrap();
    b.pop_scope();
    b.pop_scope();
    b.finish().unwrap().0
}

fn save_opts() -> SaveOptions {
    SaveOptions {
        write: WriteOptions::deterministic(),
        svg: SvgOptions::default(),
        ..SaveOptions::default()
    }
}

fn features_by_char(doc: &Document) -> Vec<Vec<(String, u16)>> {
    let story = doc
        .tree
        .preorder(doc.tree.root())
        .find(|n| matches!(doc.tree.kind(*n), Some(NodeKind::TextStory(_))))
        .unwrap();
    let st = StoryText::collect_simple(&doc.tree, &doc.defaults, story).unwrap();
    st.text
        .char_indices()
        .map(|(i, _)| {
            match st.runs[st.run_at(i).unwrap()]
                .attrs
                .get(AttrSlot::TxtFeatures)
            {
                AttrValue::FontFeatures(f) => f
                    .iter()
                    .map(|s| (s.tag_str().to_owned(), s.value))
                    .collect(),
                other => panic!("{other:?}"),
            }
        })
        .collect()
}

#[test]
fn feature_settings_read_back_and_only_where_they_were_set() {
    let doc = doc();
    let mut first = Cursor::new(Vec::new());
    save_to(&doc, &mut first, &save_opts()).unwrap();
    let first = first.into_inner();
    let mut r = xarast_format::XarastReader::open(Cursor::new(first.clone())).unwrap();
    let svg = String::from_utf8(r.document_bytes().unwrap()).unwrap();
    assert_eq!(
        svg.matches("xarast:features=\"liga:0 smcp:1\"").count(),
        1,
        "{svg}"
    );

    let mut o = open_reader(Cursor::new(first.clone()), &OpenOptions::default()).unwrap();
    let want = features_by_char(&doc);
    assert!(want[0].is_empty() && want[2] == [("liga".into(), 0), ("smcp".into(), 1)]);
    assert_eq!(features_by_char(&o.document), want);
    assert_eq!(normal_form(&doc), normal_form(&o.document));

    let mut second = Cursor::new(Vec::new());
    save_opened_to(&o.document, &mut o.package, &mut second, &save_opts()).unwrap();
    assert_eq!(
        first,
        second.into_inner(),
        "the first re-save is a fixed point"
    );
}

#[test]
fn feature_tags_are_four_printable_characters_and_the_last_setting_wins() {
    assert!(FeatureSetting::new("smc", 1).is_none());
    assert!(FeatureSetting::new("sm p", 1).is_none());
    assert!(FeatureSetting::new("ss01", 3).is_some());
    let n = FeatureSetting::normalised(&[
        FeatureSetting::new("smcp", 1).unwrap(),
        FeatureSetting::new("dlig", 1).unwrap(),
        FeatureSetting::new("smcp", 0).unwrap(),
    ]);
    let v: Vec<(&str, u16)> = n.iter().map(|s| (s.tag_str(), s.value)).collect();
    assert_eq!(v, [("dlig", 1), ("smcp", 0)]);
    let _: Arc<[FeatureSetting]> = n;
}
