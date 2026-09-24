//! Panels and the docking host.
//!
//! [`Panel`] is the seam the spike's second fallback needs: a panel is
//! addressed only through this trait, so one panel can change its painting
//! technology — or leave egui entirely for a custom pass — without any
//! other part of the interface knowing. `egui_tiles` provides the tree:
//! nested tabs, linear and grid containers, drag-and-drop between groups,
//! and a serialisable layout (`research/05 §2.5`).

use std::collections::BTreeMap;

use crate::model::{CommandSink, UiModel};
use crate::theme::ThemeTokens;

/// Identifies a panel kind. Stable across releases: it is what a saved
/// layout stores.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PanelId(pub &'static str);

impl PanelId {
    /// The identifier as it is written into a saved layout.
    pub fn as_str(self) -> &'static str {
        self.0
    }
}

impl std::fmt::Display for PanelId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.0)
    }
}

/// Everything a panel is given while it runs.
///
/// A borrow of the read-only model and a place to put commands. A panel
/// that needs anything else needs a wider model, not a back door.
#[derive(Debug)]
pub struct PanelCtx<'a> {
    /// This frame's model.
    pub model: &'a UiModel,
    /// The resolved theme tokens.
    pub tokens: &'a ThemeTokens,
    /// Where commands go.
    pub out: &'a mut CommandSink,
}

/// A dockable panel.
pub trait Panel {
    /// The stable identifier, used by the saved layout.
    fn id(&self) -> PanelId;

    /// The title shown on the tab.
    fn title(&self) -> &str;

    /// Draws the panel.
    fn ui(&mut self, ui: &mut egui::Ui, ctx: &mut PanelCtx<'_>);

    /// The smallest useful size, in logical points.
    fn min_size(&self) -> egui::Vec2 {
        egui::vec2(180.0, 80.0)
    }
}

impl std::fmt::Debug for dyn Panel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Panel")
            .field("id", &self.id())
            .field("title", &self.title())
            .finish()
    }
}

/// A pane of the docking tree: the identifier of a registered panel.
///
/// The tree stores identifiers rather than panels so that a layout can be
/// saved, loaded and validated without the panels existing yet — which is
/// also what makes a layout from an older release survive a panel being
/// renamed or removed.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct Pane(pub String);

/// A saved docking layout.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct LayoutState {
    /// The layout format version, so an incompatible tree can be discarded
    /// rather than misread.
    pub version: u32,
    /// The tree itself.
    pub tree: egui_tiles::Tree<Pane>,
}

/// The layout format version this release writes.
pub const LAYOUT_VERSION: u32 = 1;

/// What a frame of the interface produced.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UiOutput {
    /// Panels the user closed this frame.
    pub closed: Vec<String>,
    /// True when the layout changed and should be persisted.
    pub layout_changed: bool,
}

/// The docking host: the tree plus the registered panels.
pub struct UiHost {
    tree: egui_tiles::Tree<Pane>,
    panels: BTreeMap<String, Box<dyn Panel>>,
}

impl std::fmt::Debug for UiHost {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("UiHost")
            .field("panes", &self.tree.tiles.len())
            .field("panels", &self.panels.keys().collect::<Vec<_>>())
            .finish()
    }
}

impl Default for UiHost {
    fn default() -> Self {
        UiHost::new()
    }
}

impl UiHost {
    /// An empty host with an empty tree.
    pub fn new() -> UiHost {
        UiHost {
            tree: egui_tiles::Tree::empty("xarast_dock"),
            panels: BTreeMap::new(),
        }
    }

    /// Registers a panel. Registering the same identifier twice replaces
    /// the panel and keeps its place in the tree.
    pub fn register(&mut self, panel: Box<dyn Panel>) {
        self.panels.insert(panel.id().0.to_owned(), panel);
    }

    /// True when a panel with this identifier is registered.
    pub fn has(&self, id: PanelId) -> bool {
        self.panels.contains_key(id.0)
    }

    /// The registered identifiers, in a stable order.
    pub fn registered(&self) -> Vec<&str> {
        self.panels.keys().map(String::as_str).collect()
    }

    /// Builds the default layout: the named panels in one right-hand
    /// column, in the order given.
    ///
    /// The canvas is *not* a pane: it is the central region the docking
    /// tree sits beside, because the canvas must never be dragged into a
    /// tab group and lose the shared surface.
    pub fn set_default_layout(&mut self, order: &[PanelId]) {
        let mut tiles = egui_tiles::Tiles::default();
        let children: Vec<_> = order
            .iter()
            .filter(|id| self.panels.contains_key(id.0))
            .map(|id| tiles.insert_pane(Pane(id.0.to_owned())))
            .collect();
        let root = tiles.insert_vertical_tile(children);
        self.tree = egui_tiles::Tree::new("xarast_dock", root, tiles);
    }

    /// Builds the default layout as one right-hand column of **tab
    /// groups**: each inner slice is one cell of the column, its panels
    /// tabs of that cell with the first one showing. A single panel is a
    /// plain pane, as [`UiHost::set_default_layout`] makes it.
    pub fn set_default_layout_grouped(&mut self, groups: &[&[PanelId]]) {
        let mut tiles = egui_tiles::Tiles::default();
        let mut cells = Vec::new();
        for group in groups {
            let panes: Vec<_> = group
                .iter()
                .filter(|id| self.panels.contains_key(id.0))
                .map(|id| tiles.insert_pane(Pane(id.0.to_owned())))
                .collect();
            match panes.len() {
                0 => {}
                1 => cells.push(panes[0]),
                _ => cells.push(tiles.insert_tab_tile(panes)),
            }
        }
        let root = tiles.insert_vertical_tile(cells);
        self.tree = egui_tiles::Tree::new("xarast_dock", root, tiles);
    }

    /// The saved form of the current layout.
    pub fn save_layout(&self) -> LayoutState {
        LayoutState {
            version: LAYOUT_VERSION,
            tree: self.tree.clone(),
        }
    }

    /// Restores a saved layout.
    ///
    /// Returns `false`, changing nothing, when the layout comes from an
    /// incompatible version or names no panel this release still has: a
    /// stale layout must degrade to the default, never to an empty window.
    pub fn load_layout(&mut self, state: &LayoutState) -> bool {
        if state.version != LAYOUT_VERSION {
            return false;
        }
        let known = state
            .tree
            .tiles
            .iter()
            .filter_map(|(_, tile)| match tile {
                egui_tiles::Tile::Pane(p) => Some(p),
                egui_tiles::Tile::Container(_) => None,
            })
            .any(|p| self.panels.contains_key(&p.0));
        if !known {
            return false;
        }
        self.tree = state.tree.clone();
        true
    }

    /// Runs one frame of the docked panels inside `ui`.
    pub fn run(&mut self, ui: &mut egui::Ui, ctx: &mut PanelCtx<'_>) -> UiOutput {
        let mut behavior = HostBehavior {
            panels: &mut self.panels,
            ctx,
            output: UiOutput::default(),
        };
        self.tree.ui(&mut behavior, ui);
        behavior.output
    }
}

struct HostBehavior<'a, 'c> {
    panels: &'a mut BTreeMap<String, Box<dyn Panel>>,
    ctx: &'a mut PanelCtx<'c>,
    output: UiOutput,
}

impl egui_tiles::Behavior<Pane> for HostBehavior<'_, '_> {
    fn pane_ui(
        &mut self,
        ui: &mut egui::Ui,
        _id: egui_tiles::TileId,
        pane: &mut Pane,
    ) -> egui_tiles::UiResponse {
        match self.panels.get_mut(&pane.0) {
            Some(panel) => panel.ui(ui, self.ctx),
            None => {
                // A layout naming a panel this release does not have: say
                // so plainly instead of showing an empty square.
                ui.label(format!("Panel `{}` is not available", pane.0));
                self.output.closed.push(pane.0.clone());
            }
        }
        egui_tiles::UiResponse::None
    }

    fn on_tab_button(
        &mut self,
        tiles: &egui_tiles::Tiles<Pane>,
        tile_id: egui_tiles::TileId,
        button_response: egui::Response,
    ) -> egui::Response {
        // egui_tiles paints a tab's title without telling AccessKit: name
        // the tab after its panel, so that a panel sharing a tab group
        // (the photo panel beside the bitmap gallery) can be found and
        // chosen without a pointer.
        let title = self.tab_title_for_tile(tiles, tile_id).text().to_owned();
        let ctx = button_response.ctx.clone();
        ctx.accesskit_node_builder(button_response.id, |node| {
            node.set_role(egui::accesskit::Role::Tab);
            node.set_label(title);
        });
        button_response
    }

    fn tab_title_for_pane(&mut self, pane: &Pane) -> egui::WidgetText {
        match self.panels.get(&pane.0) {
            Some(p) => p.title().to_owned().into(),
            None => pane.0.clone().into(),
        }
    }

    fn simplification_options(&self) -> egui_tiles::SimplificationOptions {
        egui_tiles::SimplificationOptions {
            // A panel dragged out on its own must stay a tab group, so
            // that another can be dropped next to it.
            all_panes_must_have_tabs: true,
            ..Default::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::UiModel;
    use crate::theme::{ResolvedTheme, ThemeTokens};

    fn panes_of(state: &LayoutState) -> Vec<String> {
        let mut v: Vec<String> = state
            .tree
            .tiles
            .iter()
            .filter_map(|(_, tile)| match tile {
                egui_tiles::Tile::Pane(p) => Some(p.0.clone()),
                egui_tiles::Tile::Container(_) => None,
            })
            .collect();
        v.sort();
        v
    }

    struct Probe {
        id: PanelId,
        ran: std::rc::Rc<std::cell::Cell<u32>>,
    }

    impl Panel for Probe {
        fn id(&self) -> PanelId {
            self.id
        }
        fn title(&self) -> &str {
            self.id.0
        }
        fn ui(&mut self, ui: &mut egui::Ui, _ctx: &mut PanelCtx<'_>) {
            self.ran.set(self.ran.get() + 1);
            ui.label(self.id.0);
        }
    }

    fn host_with(ids: &[PanelId]) -> (UiHost, std::rc::Rc<std::cell::Cell<u32>>) {
        let counter = std::rc::Rc::new(std::cell::Cell::new(0));
        let mut host = UiHost::new();
        for id in ids {
            host.register(Box::new(Probe {
                id: *id,
                ran: counter.clone(),
            }));
        }
        host.set_default_layout(ids);
        (host, counter)
    }

    fn run_once(host: &mut UiHost) -> UiOutput {
        let model = UiModel::default();
        let tokens = ThemeTokens::of(ResolvedTheme::Dark);
        let mut sink = CommandSink::new();
        let mut ctx = PanelCtx {
            model: &model,
            tokens: &tokens,
            out: &mut sink,
        };
        let egui_ctx = egui::Context::default();
        let mut out = UiOutput::default();
        let _ = egui_ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(400.0, 800.0),
                )),
                ..Default::default()
            },
            |c| {
                egui::CentralPanel::default().show(c, |ui| {
                    out = host.run(ui, &mut ctx);
                });
            },
        );
        out
    }

    #[test]
    fn registered_panels_are_run() {
        let (mut host, counter) = host_with(&[PanelId("layers"), PanelId("colour")]);
        assert!(host.has(PanelId("layers")));
        run_once(&mut host);
        assert_eq!(counter.get(), 2);
    }

    #[test]
    fn registering_twice_replaces_rather_than_duplicates() {
        let (mut host, _) = host_with(&[PanelId("layers")]);
        let counter = std::rc::Rc::new(std::cell::Cell::new(0));
        host.register(Box::new(Probe {
            id: PanelId("layers"),
            ran: counter.clone(),
        }));
        assert_eq!(host.registered(), vec!["layers"]);
        run_once(&mut host);
        assert_eq!(counter.get(), 1);
    }

    #[test]
    fn a_layout_round_trips_through_serde() {
        let (host, _) = host_with(&[PanelId("layers"), PanelId("colour"), PanelId("status")]);
        let saved = host.save_layout();
        let json = serde_json::to_string(&saved).expect("a layout serialises");
        let back: LayoutState = serde_json::from_str(&json).expect("and reads back");
        assert_eq!(back.version, LAYOUT_VERSION);

        let (mut other, _) = host_with(&[PanelId("layers"), PanelId("colour"), PanelId("status")]);
        assert!(other.load_layout(&back));
        assert_eq!(panes_of(&other.save_layout()), panes_of(&saved));
    }

    #[test]
    fn a_layout_from_another_version_is_refused_not_misread() {
        let (mut host, _) = host_with(&[PanelId("layers")]);
        let mut saved = host.save_layout();
        saved.version = LAYOUT_VERSION + 1;
        assert!(!host.load_layout(&saved));
    }

    #[test]
    fn a_layout_naming_only_unknown_panels_is_refused() {
        let (stale, _) = host_with(&[PanelId("gallery-of-1998")]);
        let saved = stale.save_layout();
        let (mut current, _) = host_with(&[PanelId("layers")]);
        assert!(!current.load_layout(&saved));
        assert!(current.has(PanelId("layers")));
    }

    #[test]
    fn a_layout_naming_one_missing_panel_still_loads_and_says_so() {
        let (mixed, _) = host_with(&[PanelId("layers"), PanelId("gone")]);
        let saved = mixed.save_layout();
        let (mut current, _) = host_with(&[PanelId("layers")]);
        assert!(current.load_layout(&saved));
        let out = run_once(&mut current);
        assert_eq!(out.closed, vec!["gone".to_owned()]);
    }

    #[test]
    fn panel_ids_display_as_their_string() {
        assert_eq!(PanelId("layers").to_string(), "layers");
        assert_eq!(PanelId("layers").as_str(), "layers");
    }
}
