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
    /// The cells of the last default layout, each a list of panel ids:
    /// where a panel missing from the tree is put back.
    homes: Vec<Vec<String>>,
    /// The pane whose tab takes the keyboard focus on the next frame.
    focus: Option<String>,
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
            homes: Vec::new(),
            focus: None,
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
        self.homes = order.iter().map(|id| vec![id.0.to_owned()]).collect();
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
        self.homes = groups
            .iter()
            .map(|g| g.iter().map(|id| id.0.to_owned()).collect())
            .collect();
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
        // A layout saved before a panel existed (the photo panel, say)
        // must still show it: each registered panel it lacks goes back
        // to its default cell.
        let missing: Vec<String> = self
            .panels
            .keys()
            .filter(|id| self.tile_of(id).is_none())
            .cloned()
            .collect();
        for id in missing {
            self.dock(&id);
        }
        true
    }

    /// The tile holding the panel `id`, if the tree has it.
    fn tile_of(&self, id: &str) -> Option<egui_tiles::TileId> {
        self.tree.tiles.find_pane(&Pane(id.to_owned()))
    }

    /// Puts a registered panel missing from the tree back into it: as a
    /// tab beside a panel of its default cell when one is docked in a tab
    /// group, otherwise as a new cell at the end of the root container.
    fn dock(&mut self, id: &str) -> egui_tiles::TileId {
        let beside = self
            .homes
            .iter()
            .find(|cell| cell.iter().any(|p| p == id))
            .into_iter()
            .flatten()
            .filter(|p| *p != id)
            .find_map(|p| self.tile_of(p))
            .and_then(|t| self.tree.tiles.parent_of(t))
            .filter(|parent| {
                matches!(
                    self.tree.tiles.get(*parent),
                    Some(egui_tiles::Tile::Container(egui_tiles::Container::Tabs(_)))
                )
            });
        let tile = self.tree.tiles.insert_pane(Pane(id.to_owned()));
        let parent = match beside {
            Some(tabs) => Some(tabs),
            // A column or a grid takes it as one more cell; a tab group or
            // a lone pane at the root is put in a column with it, so the
            // panel does not hide behind a tab.
            None => match self.tree.root {
                Some(root)
                    if matches!(
                        self.tree.tiles.get_container(root),
                        Some(egui_tiles::Container::Linear(_) | egui_tiles::Container::Grid(_))
                    ) =>
                {
                    Some(root)
                }
                Some(root) => {
                    let column = self.tree.tiles.insert_vertical_tile(vec![root]);
                    self.tree.root = Some(column);
                    Some(column)
                }
                None => {
                    self.tree.root = Some(tile);
                    None
                }
            },
        };
        if let Some(parent) = parent
            && let Some(egui_tiles::Tile::Container(c)) = self.tree.tiles.get_mut(parent)
        {
            c.add_child(tile);
        }
        tile
    }

    /// Shows the panel `id` and brings it to the front: docked again if
    /// the layout lacks it, made visible, its tab made the active one in
    /// every tab group above it, and its tab given the keyboard focus on
    /// the next frame. Returns `false` for a panel that is not registered.
    pub fn show(&mut self, id: PanelId) -> bool {
        if !self.panels.contains_key(id.0) {
            return false;
        }
        let mut child = match self.tile_of(id.0) {
            Some(tile) => tile,
            None => self.dock(id.0),
        };
        self.tree.tiles.set_visible(child, true);
        while let Some(parent) = self.tree.tiles.parent_of(child) {
            self.tree.tiles.set_visible(parent, true);
            if let Some(egui_tiles::Tile::Container(egui_tiles::Container::Tabs(tabs))) =
                self.tree.tiles.get_mut(parent)
            {
                tabs.set_active(child);
            }
            child = parent;
        }
        self.focus = Some(id.0.to_owned());
        true
    }

    /// Whether the panel `id` is in the tree at all, showing or not.
    pub fn is_docked(&self, id: PanelId) -> bool {
        self.tile_of(id.0).is_some()
    }

    /// Whether the panel `id` is in the tree, visible, and the active tab
    /// of every tab group above it: what a user sees as "showing".
    pub fn is_showing(&self, id: PanelId) -> bool {
        let Some(mut child) = self.tile_of(id.0) else {
            return false;
        };
        loop {
            if !self.tree.tiles.is_visible(child) {
                return false;
            }
            let Some(parent) = self.tree.tiles.parent_of(child) else {
                return self.tree.root == Some(child);
            };
            if let Some(egui_tiles::Tile::Container(egui_tiles::Container::Tabs(tabs))) =
                self.tree.tiles.get(parent)
                && !tabs.is_active(child)
            {
                return false;
            }
            child = parent;
        }
    }

    /// Runs one frame of the docked panels inside `ui`.
    pub fn run(&mut self, ui: &mut egui::Ui, ctx: &mut PanelCtx<'_>) -> UiOutput {
        let mut behavior = HostBehavior {
            panels: &mut self.panels,
            ctx,
            output: UiOutput::default(),
            focus: self.focus.take(),
        };
        self.tree.ui(&mut behavior, ui);
        behavior.output
    }
}

struct HostBehavior<'a, 'c> {
    panels: &'a mut BTreeMap<String, Box<dyn Panel>>,
    ctx: &'a mut PanelCtx<'c>,
    output: UiOutput,
    /// The pane whose tab takes the keyboard focus this frame.
    focus: Option<String>,
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
        // A pane just shown (F9, the Window menu) takes the keyboard on
        // its tab, so Tab walks straight into it.
        if let Some(pane) = tiles.get_pane(&tile_id)
            && self.focus.as_ref() == Some(&pane.0)
        {
            button_response.request_focus();
            self.focus = None;
        }
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

    fn grouped(ids: &[PanelId], groups: &[&[PanelId]]) -> UiHost {
        let counter = std::rc::Rc::new(std::cell::Cell::new(0));
        let mut host = UiHost::new();
        for id in ids {
            host.register(Box::new(Probe {
                id: *id,
                ran: counter.clone(),
            }));
        }
        host.set_default_layout_grouped(groups);
        host
    }

    const LAYERS: PanelId = PanelId("layers");
    const GALLERY: PanelId = PanelId("gallery");
    const PHOTO: PanelId = PanelId("photo");

    #[test]
    fn showing_a_panel_makes_its_tab_the_active_one() {
        let mut host = grouped(&[LAYERS, GALLERY, PHOTO], &[&[LAYERS], &[GALLERY, PHOTO]]);
        run_once(&mut host);
        assert!(host.is_showing(LAYERS));
        assert!(host.is_showing(GALLERY));
        assert!(!host.is_showing(PHOTO), "a second tab starts hidden");
        assert!(host.show(PHOTO));
        assert!(host.is_showing(PHOTO));
        assert!(!host.is_showing(GALLERY));
        run_once(&mut host);
        assert!(host.is_showing(PHOTO), "and stays in front");
        assert!(host.show(GALLERY));
        assert!(host.is_showing(GALLERY));
        assert!(!host.show(PanelId("never-registered")));
    }

    #[test]
    fn showing_a_hidden_panel_makes_it_visible() {
        let mut host = grouped(&[LAYERS, GALLERY], &[&[LAYERS], &[GALLERY]]);
        run_once(&mut host);
        let tile = host.tile_of(GALLERY.0).unwrap();
        host.tree.tiles.set_visible(tile, false);
        assert!(!host.is_showing(GALLERY));
        assert!(host.show(GALLERY));
        assert!(host.is_showing(GALLERY));
    }

    #[test]
    fn a_layout_from_before_a_panel_existed_still_shows_it() {
        // Saved when only the layers and the gallery existed; the frame
        // run first wraps each pane in a tab group, as a saved one is.
        let mut old = grouped(&[LAYERS, GALLERY], &[&[LAYERS], &[GALLERY]]);
        run_once(&mut old);
        let saved = old.save_layout();

        let mut now = grouped(&[LAYERS, GALLERY, PHOTO], &[&[LAYERS], &[GALLERY, PHOTO]]);
        assert!(now.load_layout(&saved));
        assert!(now.is_docked(PHOTO));
        // Beside the gallery, as a tab of its group, not showing yet.
        let parent = |h: &UiHost, id: PanelId| h.tree.tiles.parent_of(h.tile_of(id.0).unwrap());
        assert_eq!(parent(&now, PHOTO), parent(&now, GALLERY));
        assert!(now.is_showing(GALLERY));
        assert!(now.show(PHOTO));
        let out = run_once(&mut now);
        assert!(out.closed.is_empty());
        assert!(now.is_showing(PHOTO));
    }

    #[test]
    fn a_panel_with_no_docked_neighbour_goes_to_the_end_of_the_column() {
        let mut old = grouped(&[LAYERS], &[&[LAYERS]]);
        run_once(&mut old);
        let saved = old.save_layout();
        let mut now = grouped(&[LAYERS, GALLERY], &[&[LAYERS], &[GALLERY]]);
        assert!(now.load_layout(&saved));
        assert!(now.is_docked(GALLERY));
        run_once(&mut now);
        assert!(now.is_showing(LAYERS));
        assert!(now.is_showing(GALLERY));
    }

    #[test]
    fn panel_ids_display_as_their_string() {
        assert_eq!(PanelId("layers").to_string(), "layers");
        assert_eq!(PanelId("layers").as_str(), "layers");
    }
}
