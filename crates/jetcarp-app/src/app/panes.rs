//! Control panes: the registry, where each pane is ([`Place`]), and the docked layout (an
//! `egui_tiles` tree holding the grid view and the docked panes).
//!
//! A pane is docked (a tab or split in the tree, rearranged by dragging its tab), floating (an
//! `egui::Window` in the main window), popped out (its own OS window, see `crate::popout`), or
//! hidden. The grid view is a tile too; it cannot be closed, floated, or popped out.
//!
//! Adding a pane is one line in [`PANES`] (an id, a title, and the `JetcarpApp` method that draws
//! it). Wave 2's Reseed and Display panes go there.

use super::*;
use crate::popout::{self, Chrome, WinState};
use egui_tiles::{Container, Tile, TileId, Tiles, Tree};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// One control pane.
pub(super) struct PaneDef {
    /// Stable id, saved in the settings file.
    pub id: &'static str,
    pub title: &'static str,
    pub ui: fn(&mut JetcarpApp, &mut egui::Ui),
    /// Tab group in the default layout: 0 is the upper group left of the grid, 1 the lower.
    pub group: u8,
}

/// Every control pane, in default tab order.
pub(super) const PANES: &[PaneDef] = &[
    PaneDef {
        id: "scene",
        title: "Scene",
        ui: JetcarpApp::scene_pane,
        group: 0,
    },
    PaneDef {
        id: "tiling",
        title: "Tiling",
        ui: JetcarpApp::tiling_ui,
        group: 0,
    },
    PaneDef {
        id: "draw",
        title: "Draw",
        ui: JetcarpApp::draw_ui,
        group: 0,
    },
    PaneDef {
        id: "objects",
        title: "Objects",
        ui: JetcarpApp::objects_ui,
        group: 0,
    },
    PaneDef {
        id: "cas",
        title: "CAs",
        ui: JetcarpApp::cas_ui,
        group: 1,
    },
    PaneDef {
        id: "add",
        title: "Add",
        ui: JetcarpApp::add_ui,
        group: 1,
    },
    PaneDef {
        id: "sequence",
        title: "Sequence",
        ui: JetcarpApp::sequence_ui,
        group: 1,
    },
    PaneDef {
        id: "log",
        title: "Command log",
        ui: JetcarpApp::log_pane,
        group: 1,
    },
    PaneDef {
        id: "display",
        title: "Display",
        ui: JetcarpApp::display_pane,
        group: 0,
    },
    PaneDef {
        id: "reseed",
        title: "Reseed",
        ui: JetcarpApp::reseed_pane,
        group: 1,
    },
    PaneDef {
        id: "screensaver",
        title: "Screensaver",
        ui: JetcarpApp::saver_pane,
        group: 0,
    },
];

/// The grid view's id in the tree.
pub(super) const GRID: &str = "grid";

pub(super) fn pane_def(id: &str) -> Option<&'static PaneDef> {
    PANES.iter().find(|p| p.id == id)
}

/// Where a pane is.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Place {
    Docked,
    Floating,
    Popped,
    Hidden,
}

impl Place {
    pub const ALL: [Place; 4] = [Place::Docked, Place::Floating, Place::Popped, Place::Hidden];
    pub fn label(self) -> &'static str {
        match self {
            Place::Docked => "Docked",
            Place::Floating => "Floating",
            Place::Popped => "Own window",
            Place::Hidden => "Hidden",
        }
    }
}

/// A tile's pane: a `PANES` id or [`GRID`].
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Hash, Debug)]
#[serde(transparent)]
pub(super) struct PaneKey(pub String);

impl PaneKey {
    fn of(id: &str) -> PaneKey {
        PaneKey(id.to_string())
    }
}

/// The docked layout and every pane's place. Saved in the app settings.
#[derive(Serialize, Deserialize, Clone)]
pub(super) struct Layout {
    pub tree: Tree<PaneKey>,
    /// Panes missing here are docked.
    #[serde(default)]
    pub places: BTreeMap<String, Place>,
    /// Popped-out window sizes.
    #[serde(default)]
    pub windows: BTreeMap<String, WinState>,
}

fn tree_id() -> egui::Id {
    egui::Id::unique("jetcarp-panes")
}

impl Default for Layout {
    /// Two tab groups stacked in a column left of the grid.
    fn default() -> Layout {
        let mut tiles = Tiles::default();
        let group = |tiles: &mut Tiles<PaneKey>, g: u8| {
            let kids: Vec<TileId> = PANES
                .iter()
                .filter(|p| p.group == g)
                .map(|p| tiles.insert_pane(PaneKey::of(p.id)))
                .collect();
            tiles.insert_tab_tile(kids)
        };
        let upper = group(&mut tiles, 0);
        let lower = group(&mut tiles, 1);
        let column = tiles.insert_container(egui_tiles::Linear::new_binary(
            egui_tiles::LinearDir::Vertical,
            [upper, lower],
            0.45,
        ));
        let grid = tiles.insert_pane(PaneKey::of(GRID));
        let root = tiles.insert_container(egui_tiles::Linear::new_binary(
            egui_tiles::LinearDir::Horizontal,
            [column, grid],
            0.24,
        ));
        Layout {
            tree: Tree::new(tree_id(), root, tiles),
            places: BTreeMap::new(),
            windows: BTreeMap::new(),
        }
    }
}

impl Layout {
    pub fn place(&self, id: &str) -> Place {
        self.places.get(id).copied().unwrap_or(Place::Docked)
    }

    fn tile_of(&self, id: &str) -> Option<TileId> {
        self.tree.tiles.find_pane(&PaneKey::of(id))
    }

    /// Move a pane. Docking puts it back in the tab group of another docked pane (or a new
    /// column left of the grid); leaving the tree removes its tile.
    pub fn set_place(&mut self, id: &str, place: Place) {
        if pane_def(id).is_none() {
            return;
        }
        self.places.insert(id.to_string(), place);
        let tile = self.tile_of(id);
        match (place, tile) {
            (Place::Docked, None) => self.dock(id),
            (Place::Docked, Some(t)) => self.activate(t),
            (_, Some(t)) => {
                self.tree.remove_recursively(t);
            }
            (_, None) => {}
        }
        let win = self.windows.entry(id.to_string()).or_default();
        win.popped = place == Place::Popped;
    }

    /// Show a pane wherever it is (a hidden pane is docked).
    pub fn show_pane(&mut self, id: &str) {
        match self.place(id) {
            Place::Hidden => self.set_place(id, Place::Docked),
            Place::Docked => {
                if let Some(t) = self.tile_of(id) {
                    self.activate(t);
                }
            }
            _ => {}
        }
    }

    fn activate(&mut self, tile: TileId) {
        if let Some(parent) = self.tree.tiles.parent_of(tile)
            && let Some(Tile::Container(Container::Tabs(tabs))) = self.tree.tiles.get_mut(parent)
        {
            tabs.set_active(tile);
        }
    }

    fn dock(&mut self, id: &str) {
        let new = self.tree.tiles.insert_pane(PaneKey::of(id));
        // A tab group that already holds a control pane.
        let group = self.tree.tiles.iter().find_map(|(tid, t)| match t {
            Tile::Container(Container::Tabs(tabs))
                if tabs.children.iter().any(
                    |c| matches!(self.tree.tiles.get(*c), Some(Tile::Pane(p)) if p.0 != GRID),
                ) =>
            {
                Some(*tid)
            }
            _ => None,
        });
        if let Some(g) = group
            && let Some(Tile::Container(Container::Tabs(tabs))) = self.tree.tiles.get_mut(g)
        {
            tabs.add_child(new);
            tabs.set_active(new);
            return;
        }
        let tabs = self.tree.tiles.insert_tab_tile(vec![new]);
        match self.tree.root {
            Some(root) => {
                let split = self
                    .tree
                    .tiles
                    .insert_container(egui_tiles::Linear::new_binary(
                        egui_tiles::LinearDir::Horizontal,
                        [tabs, root],
                        0.24,
                    ));
                self.tree.root = Some(split);
            }
            None => self.tree.root = Some(tabs),
        }
    }

    /// Make a loaded layout consistent: the grid is in the tree once, unknown and duplicate
    /// panes are dropped, docked panes are in the tree and others are not.
    pub fn sanitize(&mut self) {
        let mut seen = std::collections::HashSet::new();
        let mut drop = vec![];
        for (tid, t) in self.tree.tiles.iter() {
            if let Tile::Pane(p) = t {
                let known = p.0 == GRID || pane_def(&p.0).is_some();
                if !known || !seen.insert(p.0.clone()) {
                    drop.push(*tid);
                }
            }
        }
        for t in drop {
            self.tree.remove_recursively(t);
        }
        let root_ok = self
            .tree
            .root
            .is_some_and(|r| self.tree.tiles.get(r).is_some());
        if !root_ok || self.tile_of(GRID).is_none() {
            let places = std::mem::take(&mut self.places);
            let windows = std::mem::take(&mut self.windows);
            *self = Layout::default();
            self.places = places;
            self.windows = windows;
        }
        self.places.retain(|k, _| pane_def(k).is_some());
        for p in PANES {
            let place = self.place(p.id);
            let tile = self.tile_of(p.id);
            match (place, tile) {
                (Place::Docked, None) => self.dock(p.id),
                (Place::Docked, Some(_)) => {}
                (_, Some(t)) => {
                    self.tree.remove_recursively(t);
                }
                (_, None) => {}
            }
        }
    }
}

/// The tree's behaviour: draws panes through `JetcarpApp`, small tab buttons, and a tab context
/// menu. Place changes are collected and applied after the tree is drawn.
struct Shell<'a> {
    app: &'a mut JetcarpApp,
    moves: Vec<(String, Place)>,
}

fn pane_scroll(app: &mut JetcarpApp, ui: &mut egui::Ui, def: &PaneDef) {
    egui::ScrollArea::vertical()
        .id_salt(("pane", def.id))
        .auto_shrink([false, false])
        .show(ui, |ui| {
            egui::Frame::new()
                .inner_margin(egui::Margin::symmetric(6, 4))
                .show(ui, |ui| (def.ui)(app, ui));
        });
}

fn place_menu(ui: &mut egui::Ui, id: &str, moves: &mut Vec<(String, Place)>) {
    if ui.button("Float").clicked() {
        moves.push((id.into(), Place::Floating));
    }
    if ui.button("Pop out to its own window").clicked() {
        moves.push((id.into(), Place::Popped));
    }
    if ui.button("Hide").clicked() {
        moves.push((id.into(), Place::Hidden));
    }
}

impl egui_tiles::Behavior<PaneKey> for Shell<'_> {
    fn pane_ui(
        &mut self,
        ui: &mut egui::Ui,
        _tile: TileId,
        pane: &mut PaneKey,
    ) -> egui_tiles::UiResponse {
        if pane.0 == GRID {
            self.app.grid_view(ui);
        } else if let Some(def) = pane_def(&pane.0) {
            pane_scroll(self.app, ui, def);
        }
        egui_tiles::UiResponse::None
    }

    fn tab_title_for_pane(&mut self, pane: &PaneKey) -> egui::WidgetText {
        match pane_def(&pane.0) {
            Some(d) => d.title.into(),
            None => "Grid".into(),
        }
    }

    fn on_tab_button(
        &mut self,
        tiles: &mut Tiles<PaneKey>,
        tile_id: TileId,
        resp: egui::Response,
    ) -> egui::Response {
        if let Some(Tile::Pane(p)) = tiles.get(tile_id)
            && p.0 != GRID
        {
            let id = p.0.clone();
            resp.context_menu(|ui| place_menu(ui, &id, &mut self.moves));
        }
        resp
    }

    fn top_bar_right_ui(
        &mut self,
        tiles: &Tiles<PaneKey>,
        ui: &mut egui::Ui,
        _tile_id: TileId,
        tabs: &egui_tiles::Tabs,
        _scroll_offset: &mut f32,
    ) {
        let Some(Tile::Pane(p)) = tabs.active.and_then(|a| tiles.get(a)) else {
            return;
        };
        if p.0 == GRID {
            return;
        }
        let small = |t: &str| egui::Button::new(t).frame(false).small();
        if ui
            .add(small("×"))
            .on_hover_text("Hide this pane (Window menu shows it again)")
            .clicked()
        {
            self.moves.push((p.0.clone(), Place::Hidden));
        }
        if ui
            .add(small("↗"))
            .on_hover_text("Pop out to its own window")
            .clicked()
        {
            self.moves.push((p.0.clone(), Place::Popped));
        }
        if ui
            .add(small("◱"))
            .on_hover_text("Float in this window")
            .clicked()
        {
            self.moves.push((p.0.clone(), Place::Floating));
        }
    }

    fn tab_bar_height(&self, _style: &egui::Style) -> f32 {
        22.0
    }

    fn gap_width(&self, _style: &egui::Style) -> f32 {
        2.0
    }

    fn simplification_options(&self) -> egui_tiles::SimplificationOptions {
        egui_tiles::SimplificationOptions {
            all_panes_must_have_tabs: false,
            ..Default::default()
        }
    }
}

impl JetcarpApp {
    /// The docked layout (grid and docked panes), filling `ui`.
    pub(super) fn docked_ui(&mut self, ui: &mut egui::Ui) {
        let mut tree = std::mem::replace(
            &mut self.shell.layout.tree,
            Tree::empty(egui::Id::unique("jetcarp-panes-taken")),
        );
        let mut shell = Shell {
            app: self,
            moves: vec![],
        };
        tree.ui(&mut shell, ui);
        let moves = shell.moves;
        self.shell.layout.tree = tree;
        for (id, p) in moves {
            self.shell.layout.set_place(&id, p);
        }
    }

    /// Floating and popped-out panes (every frame, after the docked layout).
    pub(super) fn loose_panes(&mut self, ctx: &egui::Context) {
        for def in PANES {
            let place = self.shell.layout.place(def.id);
            let floating = place == Place::Floating && !self.shell.panes_hidden;
            if !(floating || place == Place::Popped) {
                continue;
            }
            let mut st = self
                .shell
                .layout
                .windows
                .get(def.id)
                .cloned()
                .unwrap_or_default();
            st.popped = place == Place::Popped;
            let mut open = true;
            let spec = popout::Spec::new(def.id, def.title)
                .size(340.0, 340.0)
                .dockable()
                .configure(|w| w.default_pos(ctx.content_rect().min + egui::vec2(40.0, 60.0)));
            let (_, events) = popout::show(ctx, spec, &mut st, &mut open, |ui| {
                pane_scroll(self, ui, def)
            });
            self.shell.layout.windows.insert(def.id.to_string(), st);
            for e in events {
                let to = match e {
                    // Closing a popped-out pane's window docks it again.
                    Chrome::Return | Chrome::Dock => Place::Docked,
                    Chrome::PopOut => Place::Popped,
                    Chrome::Close => Place::Hidden,
                };
                self.shell.layout.set_place(def.id, to);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn docked(l: &Layout) -> Vec<String> {
        let mut v: Vec<String> = l
            .tree
            .tiles
            .tiles()
            .filter_map(|t| match t {
                Tile::Pane(p) => Some(p.0.clone()),
                _ => None,
            })
            .collect();
        v.sort();
        v
    }

    #[test]
    fn default_layout_has_every_pane_and_the_grid() {
        let l = Layout::default();
        let d = docked(&l);
        assert!(d.contains(&GRID.to_string()));
        for p in PANES {
            assert!(d.contains(&p.id.to_string()), "{} docked", p.id);
            assert_eq!(l.place(p.id), Place::Docked);
        }
    }

    #[test]
    fn dock_float_pop_close_redocks() {
        let mut l = Layout::default();
        l.set_place("tiling", Place::Floating);
        assert!(!docked(&l).contains(&"tiling".into()));
        assert_eq!(l.place("tiling"), Place::Floating);
        l.set_place("tiling", Place::Popped);
        assert_eq!(l.place("tiling"), Place::Popped);
        assert!(l.windows["tiling"].popped);
        // Closing its OS window: what `loose_panes` does on `Chrome::Return`.
        l.set_place("tiling", Place::Docked);
        assert_eq!(l.place("tiling"), Place::Docked);
        assert!(docked(&l).contains(&"tiling".into()));
        assert!(!l.windows["tiling"].popped);
        // Hidden, then shown again from the Window menu.
        l.set_place("cas", Place::Hidden);
        assert!(!docked(&l).contains(&"cas".into()));
        l.show_pane("cas");
        assert_eq!(l.place("cas"), Place::Docked);
        assert!(docked(&l).contains(&"cas".into()));
    }

    #[test]
    fn docking_with_every_pane_gone_makes_a_column() {
        let mut l = Layout::default();
        for p in PANES {
            l.set_place(p.id, Place::Hidden);
        }
        assert_eq!(docked(&l), vec![GRID.to_string()]);
        l.set_place("add", Place::Docked);
        assert_eq!(docked(&l), vec!["add".to_string(), GRID.to_string()]);
    }

    #[test]
    fn sanitize_drops_unknown_and_restores_the_grid() {
        let mut l = Layout::default();
        let t = l.tree.tiles.insert_pane(PaneKey("no-such-pane".into()));
        if let Some(Tile::Container(c)) = l.tree.root.and_then(|r| l.tree.tiles.get_mut(r)) {
            c.add_child(t);
        }
        l.places.insert("gone".into(), Place::Floating);
        l.sanitize();
        assert!(!docked(&l).contains(&"no-such-pane".into()));
        assert!(!l.places.contains_key("gone"));
        // A tree without the grid is rebuilt; places survive.
        let mut l = Layout::default();
        l.set_place("draw", Place::Floating);
        let g = l.tile_of(GRID).unwrap();
        l.tree.remove_recursively(g);
        l.sanitize();
        assert!(docked(&l).contains(&GRID.to_string()));
        assert_eq!(l.place("draw"), Place::Floating);
        assert!(!docked(&l).contains(&"draw".into()));
    }

    #[test]
    fn layout_json_round_trip() {
        let mut l = Layout::default();
        l.set_place("objects", Place::Popped);
        l.windows.get_mut("objects").unwrap().size = Some([400.0, 300.0]);
        let json = serde_json::to_string(&l).unwrap();
        let mut back: Layout = serde_json::from_str(&json).unwrap();
        back.sanitize();
        assert_eq!(docked(&back), docked(&l));
        assert_eq!(back.place("objects"), Place::Popped);
        assert_eq!(back.windows["objects"].size, Some([400.0, 300.0]));
        // The same tree (its maps serialize in hash order, so compare values).
        assert_eq!(format!("{:?}", back.tree), format!("{:?}", l.tree));
        let v = |s: &str| serde_json::from_str::<serde_json::Value>(s).unwrap();
        assert_eq!(v(&serde_json::to_string(&back).unwrap()), v(&json));
    }
}
