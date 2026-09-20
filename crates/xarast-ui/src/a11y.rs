//! Accessibility: what the interface tells AccessKit.
//!
//! egui publishes an AccessKit tree of its own for standard widgets, and
//! the shell carries that tree to AT-SPI through `accesskit_winit`. Two
//! things are left to this crate, and both are done here rather than
//! sprinkled through the panels:
//!
//! * **Custom-painted regions must still publish a node.** The canvas is a
//!   painted rectangle with no widget underneath it; without an explicit
//!   description it is a hole in the tree. Criterion 12 of the phase
//!   document forbids that.
//! * **Every control needs a name a screen reader can read aloud.** An
//!   icon-only toggle whose label is a drawn glyph is anonymous to AT-SPI,
//!   so the toggles in this crate carry text labels — `Hide layer`, not a
//!   drawn eye alone.
//!
//! Keyboard operability is the other half of the same constraint and lives
//! with the widgets themselves: every control this crate creates is a
//! focusable egui widget, reachable with `Tab`, and the canvas pans and
//! zooms from the keyboard (see [`crate::canvas`]).

use crate::model::DocumentView;

/// Describes the canvas region to AccessKit.
///
/// The label carries the document title and the zoom, because a screen
/// reader user landing on the canvas needs to know *which* document they
/// are in and how far it is magnified before anything else.
pub fn describe_canvas(response: &egui::Response, doc: &DocumentView) {
    let label = canvas_label(doc);
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Other, response.enabled(), label.clone())
    });
}

/// The canvas's accessible name.
pub fn canvas_label(doc: &DocumentView) -> String {
    format!(
        "Document canvas: {}, {:.0} % zoom",
        doc.title,
        doc.view.zoom_percent()
    )
}

/// Publishes a role on an existing egui widget's AccessKit node.
///
/// egui derives a role from its own widget type, and its widget types do
/// not include a tree, a list or a row: the spike's P8 census found the
/// layer rows exposed as a flat run of buttons, which a screen reader
/// reads as a toolbar rather than a stack of layers. This is the supported
/// way to correct that without leaving egui — the node is egui's, only the
/// role and the position within the list are ours.
///
/// A no-op when AccessKit is not enabled on the context, so it costs
/// nothing in a normal run.
pub fn set_role(ctx: &egui::Context, id: egui::Id, role: egui::accesskit::Role) {
    ctx.accesskit_node_builder(id, |node| node.set_role(role));
}

/// Sets the accessible name of an existing egui widget.
///
/// An icon-sized control has no room for visible text, and egui takes a
/// widget's accessible name from the text it was given — so a checkbox
/// built with an empty label is anonymous to AT-SPI. Every such control
/// in this crate gets its name here instead.
pub fn set_label(ctx: &egui::Context, id: egui::Id, label: impl Into<String>) {
    let label = label.into();
    ctx.accesskit_node_builder(id, |node| node.set_label(label));
}

/// Marks a widget as item `index` of `count` in a list, with a role.
pub fn set_list_item(ctx: &egui::Context, id: egui::Id, index: usize, count: usize) {
    ctx.accesskit_node_builder(id, |node| {
        node.set_role(egui::accesskit::Role::ListItem);
        node.set_position_in_set(index + 1);
        node.set_size_of_set(count);
    });
}

/// The accessible name of a layer row.
///
/// Spelled out rather than encoded in icons: "Sky, visible, unlocked" is
/// what a screen reader should say, and it is also the tooltip a sighted
/// user gets.
pub fn layer_label(name: &str, visible: bool, locked: bool) -> String {
    format!(
        "{name}, {}, {}",
        if visible { "visible" } else { "hidden" },
        if locked { "locked" } else { "unlocked" }
    )
}

/// The accessible name of a palette swatch.
pub fn swatch_label(name: &str, named: bool) -> String {
    if named {
        format!("{name} (document colour)")
    } else {
        name.to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_canvas_name_says_which_document_and_what_zoom() {
        let doc = DocumentView {
            title: "poster.xar".to_owned(),
            view: crate::model::ViewTransform {
                zoom: 1.5,
                ..Default::default()
            },
            ..Default::default()
        };
        let label = canvas_label(&doc);
        assert!(label.contains("poster.xar"), "{label}");
        assert!(label.contains("150"), "{label}");
    }

    #[test]
    fn layer_names_spell_out_their_state() {
        assert_eq!(layer_label("Sky", true, false), "Sky, visible, unlocked");
        assert_eq!(layer_label("Sky", false, true), "Sky, hidden, locked");
    }

    #[test]
    fn a_document_colour_says_so() {
        assert_eq!(
            swatch_label("Brand red", true),
            "Brand red (document colour)"
        );
        assert_eq!(swatch_label("Red", false), "Red");
    }
}
