//! Native tile editor, ported from the editor in JETCARP's JavaScript prototype (the CART tiling editor, 2026): drag corner handles
//! and edge points, choose free / mirror / half-turn / straight edges, snap, curves, undo.
//! The editing rules live in jetcarp-core (`edges.rs`, `families.rs`) and are tested there.

use eframe::egui::{self, Color32, Pos2, RichText, Sense, Stroke, Vec2};
use jetcarp_core::TilingSpec;
use jetcarp_core::edges::{self, EdgeMode, EdgeShape, GlueKind, Node};
use jetcarp_core::families::{self, FAMILIES, Family, FamilyKind};
use jetcarp_core::geom::{self, Affine, P};
use jetcarp_core::topology::{curve_check, ghost_transforms};
use serde_json::Value;

/// This window's key in `crate::popout` and the settings file.
pub const KEY: &str = "tile-editor";

const CLASS_COLORS: [Color32; 6] = [
    Color32::from_rgb(0xe4, 0x57, 0x2e),
    Color32::from_rgb(0x17, 0xa3, 0x98),
    Color32::from_rgb(0x9b, 0x7e, 0xd7),
    Color32::from_rgb(0xd9, 0x9a, 0x00),
    Color32::from_rgb(0x2e, 0x86, 0xde),
    Color32::from_rgb(0xc2, 0x33, 0x7b),
];

#[derive(Clone, Copy, PartialEq)]
enum Snap {
    None,
    Square,
    Tri,
}

#[derive(Clone, Copy)]
enum Drag {
    Corner(usize),
    Node {
        e: usize,
        ci: usize,
        i: usize,
        count: usize,
    },
}

pub enum EditorAction {
    /// Use this tile for the simulation.
    Apply(TilingSpec),
    Message(String, bool),
}

pub struct TileEditor {
    pub open: bool,
    family: &'static Family,
    params: Value,
    edges: Vec<EdgeShape>,
    sel: usize,
    sel_node: Option<usize>,
    snap: Snap,
    undo: Vec<(Value, Vec<EdgeShape>)>,
    drag: Option<Drag>,
    ghosts: Vec<(Affine, usize)>,
    ghost_shapes: Vec<Vec<P>>,
    check: Option<(usize, usize)>,
    json: String,
    live: bool,
    view: Option<(f32, Vec2)>,
    rng: u64,
}

impl TileEditor {
    pub fn new() -> TileEditor {
        let mut e = TileEditor {
            open: false,
            family: families::family("lizard").unwrap(),
            params: Value::Null,
            edges: vec![],
            sel: 0,
            sel_node: None,
            snap: Snap::None,
            undo: vec![],
            drag: None,
            ghosts: vec![],
            ghost_shapes: vec![],
            check: None,
            json: String::new(),
            live: false,
            view: None,
            rng: 0x1234_5678,
        };
        e.load_family("lizard");
        e
    }

    /// Start editing the tiling currently in use.
    pub fn load_spec(&mut self, spec: &TilingSpec) {
        let Some(f) = families::family(&spec.family) else {
            return;
        };
        self.family = f;
        self.params = spec
            .params
            .clone()
            .unwrap_or_else(|| families::init_params(f.id));
        self.edges = match (&spec.edges, f.kind) {
            (_, FamilyKind::Static) => vec![],
            (Some(e), _) if e.len() == f.classes.len() => e.clone(),
            _ => families::fresh_edges(f),
        };
        self.after_load();
    }

    fn load_family(&mut self, id: &str) {
        let f = families::family(id).unwrap();
        self.family = f;
        self.params = families::init_params(id);
        self.edges = if f.kind == FamilyKind::Static {
            vec![]
        } else {
            families::fresh_edges(f)
        };
        self.after_load();
    }

    fn after_load(&mut self) {
        self.sel = 0;
        self.sel_node = None;
        self.undo.clear();
        self.view = None;
        self.refresh_ghosts();
        self.run_check();
    }

    fn spec(&self) -> TilingSpec {
        TilingSpec {
            params: Some(self.params.clone()),
            edges: if self.family.kind == FamilyKind::Static {
                None
            } else {
                Some(self.edges.clone())
            },
            ..TilingSpec::new(self.family.id)
        }
    }

    /// No edge classes to edit: catalog tilings and aperiodic tilings with fixed prototiles.
    fn fixed(&self) -> bool {
        self.family.classes.is_empty()
    }

    fn corners(&self) -> Vec<P> {
        match self.family.kind {
            FamilyKind::Static => families::static_cell(self.family.id, &self.params)
                .map(|c| c.2[0].clone())
                .unwrap_or_default(),
            FamilyKind::Aperiodic if self.fixed() => {
                jetcarp_core::aperiodic::prototiles(self.family.id)
                    .into_iter()
                    .next()
                    .unwrap_or_default()
            }
            _ => families::corners(self.family.id, &self.params).unwrap_or_default(),
        }
    }

    fn outline(&self) -> Vec<P> {
        let c = self.corners();
        if self.fixed() || c.is_empty() {
            return c;
        }
        edges::tile_outline(self.family.classes, &self.edges, &c, 10)
    }

    fn refresh_ghosts(&mut self) {
        self.ghosts = ghost_transforms(&self.spec(), 3).unwrap_or_default();
        self.ghost_shapes = match self.family.kind {
            FamilyKind::Static => families::static_cell(self.family.id, &self.params)
                .map(|c| c.2)
                .unwrap_or_default(),
            _ if self.fixed() => jetcarp_core::aperiodic::prototiles(self.family.id),
            _ => vec![self.outline()],
        };
    }

    fn run_check(&mut self) {
        self.check = curve_check(&self.spec()).ok();
    }

    fn push_undo(&mut self) {
        self.undo.push((self.params.clone(), self.edges.clone()));
        if self.undo.len() > 60 {
            self.undo.remove(0);
        }
    }

    fn snap_pt(&self, p: P) -> P {
        let g = 0.125;
        match self.snap {
            Snap::None => p,
            Snap::Square => [(p[0] / g).round() * g, (p[1] / g).round() * g],
            Snap::Tri => {
                let h = g * geom::S3 / 2.0;
                let j = (p[1] / h).round();
                let o = if (j as i64) & 1 == 1 { g / 2.0 } else { 0.0 };
                [((p[0] - o) / g).round() * g + o, j * h]
            }
        }
    }

    fn rand(&mut self) -> f64 {
        let mut r = jetcarp_core::rng::Rng::new(self.rng);
        let v = r.next_f64();
        self.rng = r.state;
        v
    }

    fn randomize(&mut self) {
        if self.fixed() {
            return;
        }
        self.push_undo();
        let amp = if self.family.kind == FamilyKind::Aperiodic {
            0.16
        } else {
            0.26
        };
        for _ in 0..30 {
            for ci in 0..self.edges.len() {
                let kind = self.family.classes[ci].kind;
                let mode = if edges::allowed_modes(kind).contains(&EdgeMode::J) {
                    if self.edges[ci].mode == EdgeMode::I {
                        EdgeMode::J
                    } else {
                        self.edges[ci].mode
                    }
                } else {
                    edges::allowed_modes(kind)[0]
                };
                let mut nodes = Vec::new();
                if mode == EdgeMode::J {
                    let k = 1 + (self.rand() * 3.0) as usize;
                    for i in 0..k {
                        let t = (i as f64 + 0.6 + self.rand() * 0.4) / (k as f64 + 0.8);
                        let d = (self.rand() - 0.5) * 2.0 * amp;
                        nodes.push(Node { t, d });
                    }
                } else if mode != EdgeMode::I {
                    let t = 0.2 + self.rand() * 0.2;
                    let d = (self.rand() - 0.5) * 2.0 * amp;
                    nodes = if mode == EdgeMode::U {
                        vec![Node { t, d }, Node { t: 1.0 - t, d }]
                    } else {
                        vec![Node { t, d }, Node { t: 1.0 - t, d: -d }]
                    };
                }
                let curved = self.rand() > 0.2;
                self.edges[ci] = EdgeShape {
                    mode,
                    curved,
                    nodes,
                };
            }
            if !edges::self_intersects(&self.outline()) {
                break;
            }
        }
    }

    /// Draw the editor window. Returns actions for the app.
    pub fn show(&mut self, ctx: &egui::Context) -> Vec<EditorAction> {
        let mut actions = Vec::new();
        if !self.open {
            return actions;
        }
        let mut open = self.open;
        crate::popout::tool_window(
            ctx,
            crate::popout::Spec::new(KEY, "Tile editor")
                .size(900.0, 560.0)
                .configure(|w| w.default_size([900.0, 560.0]).resizable(true)),
            &mut open,
            |ui| {
                ui.horizontal_top(|ui| {
                    let canvas = egui::vec2((ui.available_width() - 300.0).max(320.0), 480.0);
                    let final_edit = self.canvas(ui, canvas);
                    ui.vertical(|ui| {
                        ui.set_width(290.0);
                        let changed = self.controls(ui, &mut actions);
                        if (final_edit || changed) && self.live {
                            actions.push(EditorAction::Apply(self.spec()));
                        }
                    });
                });
            },
        );
        self.open = open;
        actions
    }

    fn controls(&mut self, ui: &mut egui::Ui, actions: &mut Vec<EditorAction>) -> bool {
        let mut changed = false;
        egui::ComboBox::from_id_salt("ed-family")
            .selected_text(self.family.name)
            .width(270.0)
            .show_ui(ui, |ui| {
                let mut group = "";
                for f in FAMILIES {
                    if f.group != group {
                        group = f.group;
                        ui.label(RichText::new(group).weak().small());
                    }
                    if ui
                        .selectable_label(self.family.id == f.id, f.name)
                        .clicked()
                        && self.family.id != f.id
                    {
                        self.load_family(f.id);
                        changed = true;
                    }
                }
                if let Some(id) = crate::isohedral_ui::combo_entry(ui, self.family.id) {
                    self.load_family(id);
                    changed = true;
                }
            });
        match crate::isohedral_ui::controls(ui, self.family.id, Some(&self.params)) {
            Some(crate::isohedral_ui::IhChange::Type(id)) => {
                self.load_family(id);
                changed = true;
            }
            Some(crate::isohedral_ui::IhChange::Params(ev)) => {
                if ev.started {
                    self.push_undo();
                }
                self.params = ev.params;
                if ev.finished {
                    changed = true;
                } else {
                    self.refresh_ghosts();
                }
            }
            None => {}
        }
        ui.label(RichText::new(self.family.note).small());
        ui.label(
            RichText::new(format!("Symmetry {}", self.family.sym))
                .small()
                .weak(),
        );
        ui.separator();
        if self.family.kind == FamilyKind::Static {
            ui.label("Catalog tilings have fixed shapes. You can still use them.");
        } else if self.fixed() {
            ui.label("This aperiodic tiling has fixed shapes. You can still use it.");
        } else if self.sel < self.edges.len() {
            let kind = self.family.classes[self.sel].kind;
            let col = CLASS_COLORS[self.sel % CLASS_COLORS.len()];
            ui.horizontal(|ui| {
                let (r, _) = ui.allocate_exact_size(egui::vec2(12.0, 12.0), Sense::hover());
                ui.painter().rect_filled(r, 2.0, col);
                ui.label(
                    RichText::new(format!(
                        "Edge class {} of {}",
                        self.sel + 1,
                        self.edges.len()
                    ))
                    .strong(),
                );
            });
            ui.label(
                RichText::new(
                    "Click an edge to select its class. Double-click an edge to add a point.",
                )
                .small()
                .weak(),
            );
            ui.horizontal(|ui| {
                for (mode, label, tip) in [
                    (
                        EdgeMode::J,
                        "Free",
                        "Any shape. The partner edge copies it.",
                    ),
                    (EdgeMode::U, "Mirror", "Mirrors about its midpoint."),
                    (
                        EdgeMode::S,
                        "Half-turn",
                        "Looks the same after a half turn.",
                    ),
                    (EdgeMode::I, "Straight", "A straight edge; points are kept."),
                ] {
                    let allowed = edges::allowed_modes(kind).contains(&mode);
                    let on = self.edges[self.sel].mode == mode;
                    if ui
                        .add_enabled(allowed, egui::Button::new(label).selected(on))
                        .on_hover_text(tip)
                        .clicked()
                        && !on
                    {
                        self.push_undo();
                        self.edges[self.sel].mode = mode;
                        edges::resymmetrize(&mut self.edges[self.sel]);
                        changed = true;
                    }
                }
            });
            let must = match kind {
                GlueKind::SelfHalfTurn | GlueKind::Shared => Some("half-turn symmetric"),
                GlueKind::Mirror => Some("mirror symmetric"),
                GlueKind::Straight => Some("straight"),
                _ => None,
            };
            if let Some(must) = must {
                ui.label(
                    RichText::new(format!("This edge must stay {must} to fit its neighbour."))
                        .small()
                        .weak(),
                );
            }
            ui.horizontal(|ui| {
                let curved = self.edges[self.sel].curved;
                if ui
                    .add(egui::Button::new("Lines").selected(!curved))
                    .clicked()
                    && curved
                {
                    self.push_undo();
                    self.edges[self.sel].curved = false;
                    changed = true;
                }
                if ui
                    .add(egui::Button::new("Curves").selected(curved))
                    .clicked()
                    && !curved
                {
                    self.push_undo();
                    self.edges[self.sel].curved = true;
                    changed = true;
                }
                if ui.button("Add point").clicked() {
                    self.push_undo();
                    let st = &self.edges[self.sel];
                    let mut ts: Vec<f64> = vec![0.0];
                    let mut mid: Vec<f64> = st.nodes.iter().map(|q| q.t).collect();
                    mid.sort_by(|a, b| a.partial_cmp(b).unwrap());
                    ts.extend(mid);
                    ts.push(1.0);
                    let (mut gap, mut at) = (0.0, 0.3);
                    for w in ts.windows(2) {
                        if w[1] - w[0] > gap {
                            gap = w[1] - w[0];
                            at = (w[0] + w[1]) / 2.0;
                        }
                    }
                    let t = if st.mode == EdgeMode::J {
                        at
                    } else {
                        at.min(0.3)
                    };
                    edges::insert_node(&mut self.edges[self.sel], kind, Node { t, d: 0.14 });
                    changed = true;
                }
                let st = &self.edges[self.sel];
                if ui
                    .add_enabled(
                        st.mode != EdgeMode::I && !st.nodes.is_empty(),
                        egui::Button::new("Delete point"),
                    )
                    .clicked()
                {
                    self.push_undo();
                    let m = self.edges[self.sel].nodes.len();
                    let i = self.sel_node.filter(|&i| i < m).unwrap_or(m - 1);
                    edges::delete_node(&mut self.edges[self.sel], i);
                    self.sel_node = None;
                    changed = true;
                }
            });
        }
        ui.separator();
        ui.horizontal(|ui| {
            ui.label("Snap");
            ui.radio_value(&mut self.snap, Snap::None, "Off");
            ui.radio_value(&mut self.snap, Snap::Square, "Square");
            ui.radio_value(&mut self.snap, Snap::Tri, "Triangle");
        });
        ui.horizontal(|ui| {
            if ui
                .add_enabled(!self.undo.is_empty(), egui::Button::new("Undo"))
                .clicked()
                && let Some((p, e)) = self.undo.pop()
            {
                self.params = p;
                self.edges = e;
                changed = true;
            }
            if ui
                .add_enabled(!self.fixed(), egui::Button::new("Randomize"))
                .clicked()
            {
                self.randomize();
                changed = true;
            }
            if ui.button("Reset").clicked() {
                self.load_family(self.family.id);
                changed = true;
            }
        });
        if changed {
            self.refresh_ghosts();
            self.run_check();
        }
        ui.separator();
        let bad = edges::self_intersects(&self.outline());
        if bad {
            ui.colored_label(Color32::LIGHT_RED, "Edges cross: tiles would overlap");
        } else {
            ui.colored_label(Color32::LIGHT_GREEN, "Valid tile");
        }
        if let Some((d, n)) = self.check
            && n > 0
        {
            let txt = if d == 0 {
                "Curves keep the same edge neighbours as straight edges.".to_string()
            } else {
                format!(
                    "Curves change edge contact for {d} of {n} tiles. CAs use the straight-edge graph."
                )
            };
            ui.label(RichText::new(txt).small());
        }
        ui.separator();
        ui.horizontal(|ui| {
            if ui
                .button(RichText::new("Use this tile").strong())
                .on_hover_text("Rebuild the simulation on this tile")
                .clicked()
            {
                actions.push(EditorAction::Apply(self.spec()));
            }
            ui.checkbox(&mut self.live, "Live")
                .on_hover_text("Rebuild the simulation after every edit");
        });
        ui.horizontal(|ui| {
            if ui.button("Copy tile JSON").clicked() {
                let j = families::tile_json(
                    self.family,
                    &self.params,
                    (self.family.kind != FamilyKind::Static).then_some(&self.edges[..]),
                );
                let text = serde_json::to_string_pretty(&j).unwrap_or_default();
                ui.ctx().copy_text(text.clone());
                self.json = text;
                actions.push(EditorAction::Message(
                    "Tile JSON copied to the clipboard.".into(),
                    false,
                ));
            }
            if ui.button("Load JSON below").clicked() {
                match serde_json::from_str::<TilingSpec>(&self.json) {
                    Ok(spec) if families::family(&spec.family).is_some() => {
                        self.load_spec(&spec);
                        changed = true;
                    }
                    Ok(spec) => actions.push(EditorAction::Message(
                        format!("Unknown family '{}'", spec.family),
                        true,
                    )),
                    Err(e) => actions.push(EditorAction::Message(format!("Tile JSON: {e}"), true)),
                }
            }
        });
        ui.add(
            egui::TextEdit::multiline(&mut self.json)
                .desired_rows(6)
                .code_editor()
                .hint_text("Paste tile JSON here"),
        );
        changed
    }

    /// Draw the tile and handle dragging. Returns true when an edit finished (mouse released).
    fn canvas(&mut self, ui: &mut egui::Ui, size: Vec2) -> bool {
        let (rect, resp) = ui.allocate_exact_size(size, Sense::click_and_drag());
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 4.0, Color32::from_rgb(0x15, 0x17, 0x1d));
        let corners = self.corners();
        if corners.is_empty() {
            return false;
        }
        // Fit the view to the tile once.
        if self.view.is_none() {
            let b = geom::bbox(&corners);
            let pad = if self.family.kind == FamilyKind::Aperiodic {
                1.25
            } else {
                1.9
            };
            let s = (rect.width() as f64 / ((b[2] - b[0]) * pad + 0.01))
                .min(rect.height() as f64 / ((b[3] - b[1]) * pad + 0.01));
            let off = Vec2::new(
                (rect.width() as f64 / 2.0 - s * (b[0] + b[2]) / 2.0) as f32,
                (rect.height() as f64 / 2.0 - s * (b[1] + b[3]) / 2.0) as f32,
            );
            self.view = Some((s as f32, off));
        }
        let (s, off) = self.view.unwrap();
        let to_screen = |p: P| {
            Pos2::new(
                rect.min.x + off.x + p[0] as f32 * s,
                rect.min.y + off.y + p[1] as f32 * s,
            )
        };
        let to_world = |q: Pos2| -> P {
            [
                ((q.x - rect.min.x - off.x) / s) as f64,
                ((q.y - rect.min.y - off.y) / s) as f64,
            ]
        };
        // Snap grid.
        if self.snap != Snap::None {
            let g = 0.125;
            let tl = to_world(rect.min);
            let br = to_world(rect.max);
            let row_h = if self.snap == Snap::Tri {
                g * geom::S3 / 2.0
            } else {
                g
            };
            let mut j = (tl[1] / row_h).floor();
            while j * row_h <= br[1] {
                let o = if self.snap == Snap::Tri && (j as i64) & 1 == 1 {
                    g / 2.0
                } else {
                    0.0
                };
                let mut i = (tl[0] / g).floor() - 1.0;
                while i * g <= br[0] + g {
                    painter.circle_filled(
                        to_screen([i * g + o, j * row_h]),
                        1.0,
                        Color32::from_gray(70),
                    );
                    i += 1.0;
                }
                j += 1.0;
            }
        }
        // Ghost neighbours and the tile.
        let fill = |poly: &[P], col: Color32| {
            let mut mesh = egui::Mesh::default();
            for p in poly {
                mesh.colored_vertex(to_screen(*p), col);
            }
            for t in geom::triangulate(poly) {
                mesh.add_triangle(t[0], t[1], t[2]);
            }
            painter.add(egui::Shape::mesh(mesh));
            let mut pts: Vec<Pos2> = poly.iter().map(|p| to_screen(*p)).collect();
            pts.push(pts[0]);
            painter.add(egui::Shape::line(
                pts,
                Stroke::new(1.0, Color32::from_gray(85)),
            ));
        };
        for (m, k) in &self.ghosts {
            if let Some(shape) = self.ghost_shapes.get(*k) {
                let poly: Vec<P> = shape.iter().map(|&p| m.apply(p)).collect();
                fill(&poly, Color32::from_rgb(0x2c, 0x31, 0x3d));
            }
        }
        let outline = self.outline();
        fill(&outline, Color32::from_rgb(0x46, 0x4f, 0x63));
        let fam = self.family;
        if self.fixed() {
            return false;
        }
        // Edges by class, nodes, handles.
        let m = corners.len();
        for e in 0..m {
            let (ci, _) = edges::class_of_edge(fam.classes, e);
            let col = CLASS_COLORS[ci % CLASS_COLORS.len()];
            let mut pts = edges::flatten_edge(fam.classes, &self.edges, &corners, e, 10);
            pts.push(corners[(e + 1) % m]);
            let w = if ci == self.sel { 3.5 } else { 2.0 };
            painter.add(egui::Shape::line(
                pts.iter().map(|p| to_screen(*p)).collect(),
                Stroke::new(w, col),
            ));
            let shared_first =
                fam.classes[ci].kind != GlueKind::Shared || fam.classes[ci].edges[0] == e;
            if shared_first {
                let (r, s2) = (corners[e], corners[(e + 1) % m]);
                for q in edges::nodes_for_edge(fam.classes, &self.edges, e) {
                    let p = to_screen(edges::edge_world(r, s2, q));
                    painter.circle(
                        p,
                        5.0,
                        Color32::from_rgb(0x15, 0x17, 0x1d),
                        Stroke::new(2.0, col),
                    );
                }
            }
        }
        let handles = families::handles(fam.id);
        for (i, c) in corners.iter().enumerate() {
            let p = to_screen(*c);
            let h = handles.contains(&i);
            let r = egui::Rect::from_center_size(p, egui::vec2(9.0, 9.0));
            painter.rect_filled(
                r,
                1.0,
                if h {
                    Color32::from_gray(230)
                } else {
                    Color32::from_rgb(0x15, 0x17, 0x1d)
                },
            );
            painter.rect_stroke(
                r,
                1.0,
                Stroke::new(1.5, Color32::from_gray(230)),
                egui::StrokeKind::Middle,
            );
        }
        // Interaction.
        let hit = |q: Pos2, me: &TileEditor| -> Option<Drag> {
            for &c in handles {
                if to_screen(corners[c]).distance(q) < 11.0 {
                    return Some(Drag::Corner(c));
                }
            }
            for e in 0..m {
                let (ci, _) = edges::class_of_edge(fam.classes, e);
                if fam.classes[ci].kind == GlueKind::Shared && e != 0 {
                    continue;
                }
                let nodes = edges::nodes_for_edge(fam.classes, &me.edges, e);
                let (r, s2) = (corners[e], corners[(e + 1) % m]);
                for (i, q2) in nodes.iter().enumerate() {
                    if to_screen(edges::edge_world(r, s2, *q2)).distance(q) < 10.0 {
                        return Some(Drag::Node {
                            e,
                            ci,
                            i,
                            count: nodes.len(),
                        });
                    }
                }
            }
            None
        };
        let edge_near = |q: Pos2, me: &TileEditor| -> Option<(usize, usize, bool)> {
            let mut best = None;
            let mut bd = 12.0;
            for e in 0..m {
                let mut pts = edges::flatten_edge(fam.classes, &me.edges, &corners, e, 10);
                pts.push(corners[(e + 1) % m]);
                for w in pts.windows(2) {
                    let (a, b) = (to_screen(w[0]), to_screen(w[1]));
                    let ab = b - a;
                    let u = ((q - a).dot(ab) / ab.length_sq().max(1e-6)).clamp(0.0, 1.0);
                    let d = (a + ab * u).distance(q);
                    if d < bd {
                        bd = d;
                        let (ci, slave) = edges::class_of_edge(fam.classes, e);
                        best = Some((e, ci, slave));
                    }
                }
            }
            best
        };
        let mut final_edit = false;
        if resp.double_clicked()
            && let Some(q) = resp.interact_pointer_pos()
            && let Some((e, ci, _)) = edge_near(q, self)
        {
            self.push_undo();
            let (r, s2) = (corners[e], corners[(e + 1) % m]);
            let node = edges::world_to_edge(r, s2, self.snap_pt(to_world(q)));
            edges::insert_node(
                &mut self.edges[ci],
                fam.classes[ci].kind,
                edges::to_class_frame(fam.classes, e, node),
            );
            self.sel = ci;
            final_edit = true;
        } else if (resp.drag_started() || resp.clicked())
            && let Some(q) = resp.interact_pointer_pos()
        {
            match hit(q, self) {
                Some(d) => {
                    if let Drag::Node {
                        e, ci, i, count, ..
                    } = d
                    {
                        self.sel = ci;
                        self.sel_node = Some(edges::class_node_index(fam.classes, e, i, count));
                    }
                    if resp.drag_started() {
                        self.push_undo();
                        self.drag = Some(d);
                    }
                }
                None => {
                    if let Some((_, ci, _)) = edge_near(q, self) {
                        self.sel = ci;
                        self.sel_node = None;
                    }
                }
            }
        }
        if let (Some(d), Some(q)) = (self.drag, resp.interact_pointer_pos())
            && resp.dragged()
        {
            let p = self.snap_pt(to_world(q));
            match d {
                Drag::Corner(c) => {
                    families::set_handle(fam.id, &mut self.params, c, p);
                    self.refresh_ghosts();
                }
                Drag::Node {
                    e, ci, i, count, ..
                } => {
                    let (r, s2) = (corners[e], corners[(e + 1) % m]);
                    let node =
                        edges::to_class_frame(fam.classes, e, edges::world_to_edge(r, s2, p));
                    edges::set_node(
                        &mut self.edges[ci],
                        edges::class_node_index(fam.classes, e, i, count),
                        node,
                    );
                    self.ghost_shapes = vec![self.outline()];
                }
            }
        }
        if resp.drag_stopped() && self.drag.take().is_some() {
            self.refresh_ghosts();
            self.run_check();
            final_edit = true;
        }
        final_edit
    }
}
