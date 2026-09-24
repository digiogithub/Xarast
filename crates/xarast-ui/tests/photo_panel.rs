//! The photo panel (phase 10, T10.6.8), headless through `egui_kittest`:
//! every control is named and reachable from the keyboard, a pointer drag
//! of a slider is previews then one commit, `Esc` cancels it, and a chain
//! with an unknown operation is shown read-only.

use std::cell::RefCell;

use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};
use xarast_app::photo_panel::{PhotoOp, PhotoOps, PhotoOrient, PhotoPanelOp, PhotoPanelView};
use xarast_ui::model::{CommandSink, UiCommand, UiModel};
use xarast_ui::panel::{Panel, PanelCtx};
use xarast_ui::panels::PhotoPanel;
use xarast_ui::theme::{ResolvedTheme, ThemeTokens};

fn view(ops: PhotoOps) -> PhotoPanelView {
    PhotoPanelView {
        node: Some(xarast_doc::NodeId::default()),
        selected: 1,
        name: "Harbour".to_owned(),
        master: Some((600, 400)),
        editable: ops.is_editable(),
        ops,
        dragging: false,
    }
}

fn model(v: PhotoPanelView) -> UiModel {
    UiModel {
        photo_panel: Some(v),
        ..UiModel::default()
    }
}

fn harness<'a>(m: &'a UiModel, commands: &'a RefCell<Vec<UiCommand>>) -> Harness<'a> {
    let tokens = ThemeTokens::of(ResolvedTheme::Dark);
    let mut panel = PhotoPanel::new();
    let mut h = Harness::builder()
        .with_size(egui::vec2(360.0, 900.0))
        .build_ui(move |ui| {
            let mut sink = CommandSink::new();
            let mut ctx = PanelCtx {
                model: m,
                tokens: &tokens,
                out: &mut sink,
            };
            panel.ui(ui, &mut ctx);
            commands.borrow_mut().extend(sink.drain());
        });
    h.run();
    h
}

fn ops_of(commands: &RefCell<Vec<UiCommand>>) -> Vec<PhotoPanelOp> {
    commands
        .borrow()
        .iter()
        .filter_map(|c| match c {
            UiCommand::PhotoPanel(op) => Some(op.clone()),
            _ => None,
        })
        .collect()
}

fn collect(node: egui_kittest::Node<'_>, out: &mut Vec<(String, String)>) {
    let ak = node.accesskit_node();
    out.push((
        format!("{:?}", ak.role()),
        ak.label().unwrap_or_default().to_owned(),
    ));
    for child in node.children() {
        collect(child, out);
    }
}

fn some_chain() -> PhotoOps {
    PhotoOps {
        ops: vec![PhotoOp::Brightness(0.2), PhotoOp::Greyscale],
    }
}

#[test]
fn every_control_has_an_accessible_name() {
    let m = model(view(some_chain()));
    let commands = RefCell::new(Vec::new());
    let h = harness(&m, &commands);
    use egui::accesskit::Role;
    for name in ["Brightness", "Contrast", "Saturation", "Gamma"] {
        h.get_by_role_and_label(Role::Slider, name);
    }
    for name in ["Input black", "Input white", "Output black", "Output white"] {
        h.get_by_role_and_label(Role::Slider, &format!("{name} (all channels)"));
    }
    for name in ["Crop left", "Crop top", "Crop width", "Crop height"] {
        h.get_by_label(name);
    }
    for name in [
        "Rotate left",
        "Rotate right",
        "Flip horizontal",
        "Flip vertical",
        "Remove crop",
        "Reset all",
    ] {
        h.get_by_label(name);
    }
    h.get_by_role_and_label(Role::CheckBox, "Greyscale");
    // The chain is a list whose items say what each operation does.
    let item = h.get_by_label("Brightness +20 %");
    assert_eq!(item.accesskit_node().role(), Role::ListItem);
    h.get_by_role_and_label(Role::ListItem, "Greyscale");

    let mut all = Vec::new();
    for c in h.root().children() {
        collect(c, &mut all);
    }
    let anonymous: Vec<_> = all
        .iter()
        .filter(|(role, label)| {
            matches!(
                role.as_str(),
                "Button"
                    | "CheckBox"
                    | "Slider"
                    | "TextInput"
                    | "SpinButton"
                    | "Switch"
                    | "ComboBox"
            ) && label.is_empty()
        })
        .collect();
    assert!(anonymous.is_empty(), "anonymous controls: {anonymous:?}");
}

#[test]
fn the_panel_is_operable_from_the_keyboard_alone() {
    let m = model(view(PhotoOps::new()));
    let commands = RefCell::new(Vec::new());
    let mut h = harness(&m, &commands);
    // Tab to the brightness slider and step it: one Set per key.
    let mut reached = Vec::new();
    for _ in 0..40 {
        h.key_press(egui::Key::Tab);
        h.run();
        let f = focused(&h);
        if let Some(f) = &f {
            reached.push(f.clone());
        }
        if f.as_deref() == Some("Brightness") {
            break;
        }
    }
    assert_eq!(
        reached.last().map(String::as_str),
        Some("Brightness"),
        "{reached:?}"
    );
    commands.borrow_mut().clear();
    h.key_press(egui::Key::ArrowRight);
    h.run();
    let ops = ops_of(&commands);
    assert_eq!(ops.len(), 1, "{ops:?}");
    let PhotoPanelOp::Set(chain) = &ops[0] else {
        panic!("{ops:?}")
    };
    assert!(
        matches!(chain.ops[..], [PhotoOp::Brightness(b)] if b > 0.0),
        "{chain:?}"
    );

    // On to "Rotate right", then Enter: one Set turning the picture.
    for _ in 0..60 {
        h.key_press(egui::Key::Tab);
        h.run();
        let f = focused(&h);
        if let Some(f) = &f {
            reached.push(f.clone());
        }
        if f.as_deref() == Some("Rotate right") {
            break;
        }
    }
    assert_eq!(
        reached.last().map(String::as_str),
        Some("Rotate right"),
        "{reached:?}"
    );
    commands.borrow_mut().clear();
    h.key_press(egui::Key::Enter);
    h.run();
    assert_eq!(
        ops_of(&commands),
        [PhotoPanelOp::Set(PhotoOps {
            ops: vec![PhotoOp::Orient(PhotoOrient::CW)]
        })]
    );
    // Every focus stop on the way had a name.
    assert!(reached.iter().all(|n| !n.is_empty()), "{reached:?}");
}

fn focused(h: &Harness<'_>) -> Option<String> {
    fn find(node: egui_kittest::Node<'_>) -> Option<String> {
        let ak = node.accesskit_node();
        if ak.is_focused() {
            return Some(ak.label().unwrap_or_default().to_owned());
        }
        node.children().find_map(find)
    }
    h.root().children().find_map(find)
}

fn drag_slider(h: &mut Harness<'_>, name: &str, escape_midway: bool) {
    let rect = h
        .get_by_role_and_label(egui::accesskit::Role::Slider, name)
        .rect();
    let start = egui::pos2(rect.center().x, rect.center().y);
    h.hover_at(start);
    h.run();
    h.drag_at(start);
    h.run();
    for step in 1..=6 {
        if escape_midway && step == 4 {
            h.key_press(egui::Key::Escape);
            h.run();
        }
        h.hover_at(start + egui::vec2(8.0 * step as f32, 0.0));
        h.run();
    }
    h.drop_at(start + egui::vec2(48.0, 0.0));
    h.run();
}

#[test]
fn a_slider_drag_previews_and_commits_once() {
    let m = model(view(PhotoOps::new()));
    let commands = RefCell::new(Vec::new());
    let mut h = harness(&m, &commands);
    drag_slider(&mut h, "Contrast", false);
    let ops = ops_of(&commands);
    let previews = ops
        .iter()
        .filter(|o| matches!(o, PhotoPanelOp::Preview(_)))
        .count();
    assert!(previews >= 3, "{ops:?}");
    assert_eq!(ops.last(), Some(&PhotoPanelOp::Commit), "{ops:?}");
    assert_eq!(
        ops.iter().filter(|o| **o == PhotoPanelOp::Commit).count(),
        1
    );
    assert!(
        !ops.iter().any(|o| matches!(o, PhotoPanelOp::Set(_))),
        "a drag sets nothing: {ops:?}"
    );
    // Every preview is a contrast change and nothing else.
    for o in &ops {
        if let PhotoPanelOp::Preview(chain) = o {
            assert!(matches!(chain.ops[..], [PhotoOp::Contrast(_)]), "{chain:?}");
        }
    }
}

#[test]
fn escape_during_a_slider_drag_cancels_it() {
    let m = model(view(PhotoOps::new()));
    let commands = RefCell::new(Vec::new());
    let mut h = harness(&m, &commands);
    drag_slider(&mut h, "Saturation", true);
    let ops = ops_of(&commands);
    let cancel = ops
        .iter()
        .position(|o| *o == PhotoPanelOp::Cancel)
        .unwrap_or_else(|| panic!("no cancel: {ops:?}"));
    assert!(
        ops[cancel + 1..].is_empty(),
        "the rest of the drag is ignored: {ops:?}"
    );
    assert!(!ops.contains(&PhotoPanelOp::Commit));
}

#[test]
fn an_unknown_operation_makes_the_chain_read_only() {
    let chain = PhotoOps {
        ops: vec![
            PhotoOp::Brightness(0.1),
            PhotoOp::Unknown {
                kind: "curves".into(),
                raw: "<xarast:photo-op xarast:kind=\"curves\"/>".into(),
            },
        ],
    };
    let m = model(view(chain));
    let commands = RefCell::new(Vec::new());
    let h = harness(&m, &commands);
    h.get_by_label_contains("Non-editable: unknown operation \u{2018}curves\u{2019}");
    h.get_by_label("Unknown operation \u{2018}curves\u{2019}");
    assert!(h.query_by_label("Brightness").is_none(), "no sliders");
    assert!(h.query_by_label("Reset all").is_none());
}

#[test]
fn with_nothing_to_adjust_the_panel_says_why() {
    let mut v = view(PhotoOps::new());
    v.node = None;
    v.selected = 3;
    let m = model(v);
    let commands = RefCell::new(Vec::new());
    let h = harness(&m, &commands);
    h.get_by_label_contains("3 objects selected");
    let m = UiModel::default();
    let h = harness(&m, &commands);
    h.get_by_label_contains("Open a document");
}

#[test]
fn the_workspace_docks_the_panel_as_a_tab_beside_the_bitmap_gallery() {
    use xarast_ui::model::DocumentView;
    use xarast_ui::scale::Scale;
    use xarast_ui::workspace::Workspace;
    let m = UiModel {
        document: Some(DocumentView {
            title: "photo.xar".to_owned(),
            ..Default::default()
        }),
        photo_panel: Some(view(some_chain())),
        ..UiModel::default()
    };
    let mut workspace = Workspace::new();
    let mut h = Harness::builder()
        .with_size(egui::vec2(1400.0, 1000.0))
        .build(move |ctx| {
            let _ = workspace.ui(ctx, &m, Scale::new(1.0), &[]);
        });
    h.run();
    // The gallery's tab shows first; the photo tab is one click away.
    let slider = egui::accesskit::Role::Slider;
    assert!(h.query_by_role_and_label(slider, "Brightness").is_none());
    h.get_by_label("Photo").click();
    h.run();
    h.run();
    h.get_by_role_and_label(slider, "Brightness");
}
