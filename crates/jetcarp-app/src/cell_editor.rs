//! Unit-cell editor for the `periodic` family: lattice vectors plus any polygons in one cell, so
//! mixed-tile and non-edge-to-edge tilings can be drawn by hand. The checks (overlaps, gaps,
//! self-intersections, measured neighbour counts) live in `jetcarp_core::cellcheck`; this window only
//! edits the cell and draws the report.

use crate::editor::EditorAction;
use eframe::egui::{self, Color32, Pos2, Rect, RichText, Sense, Stroke, Vec2};
use jetcarp_core::TilingSpec;
use jetcarp_core::catalog;
use jetcarp_core::cellcheck::{self, CellReport, Problem, UnitCell};
use jetcarp_core::families::{self, FAMILIES, FamilyKind};
use jetcarp_core::geom::{self, P, add, cross, dist, dot, len, scale, sub};

/// This window's key in `crate::popout` and the settings file.
pub const KEY: &str = "cell-editor";

const TILE_COLORS: [Color32; 10] = [
    Color32::from_rgb(0x4c, 0x78, 0xa8),
    Color32::from_rgb(0xd0, 0x7a, 0x3a),
    Color32::from_rgb(0x5a, 0x9e, 0x6f),
    Color32::from_rgb(0xa6, 0x5f, 0xa8),
    Color32::from_rgb(0xb8, 0xa2, 0x3c),
    Color32::from_rgb(0x3f, 0x9f, 0xa8),
    Color32::from_rgb(0xb5, 0x56, 0x5e),
    Color32::from_rgb(0x7f, 0x7f, 0xc9),
    Color32::from_rgb(0x8c, 0x6d, 0x4f),
    Color32::from_rgb(0x6f, 0x8f, 0x3a),
];
const BG: Color32 = Color32::from_rgb(0x15, 0x17, 0x1d);
const RED: Color32 = Color32::from_rgb(0xff, 0x4d, 0x4d);
const ORANGE: Color32 = Color32::from_rgb(0xff, 0xa0, 0x30);
const TEAL: Color32 = Color32::from_rgb(0x30, 0xe0, 0xc8);
const YELLOW: Color32 = Color32::from_rgb(0xf0, 0xe0, 0x50);
const A1_COL: Color32 = Color32::from_rgb(0xff, 0x70, 0x70);
const A2_COL: Color32 = Color32::from_rgb(0x70, 0xd0, 0xff);

fn tile_color(k: usize) -> Color32 {
    TILE_COLORS[k % TILE_COLORS.len()]
}
fn dim(c: Color32, f: f32) -> Color32 {
    let m = |v: u8, b: u8| (b as f32 + (v as f32 - b as f32) * f) as u8;
    Color32::from_rgb(m(c.r(), 0x15), m(c.g(), 0x17), m(c.b(), 0x1d))
}

#[derive(Clone, Copy, PartialEq)]
enum Tool {
    Select,
    Draw,
    Stamp,
}

#[derive(Clone, Copy, PartialEq)]
enum Grid {
    Off,
    Square,
    Tri,
}

#[derive(Clone, Copy, PartialEq)]
enum SnapKind {
    Vertex,
    Edge,
    Lattice,
    Angle,
    Grid,
}

#[derive(Clone)]
enum Drag {
    /// Base-cell corners that move together, with the lattice shift of the copy that was grabbed.
    Vertices(Vec<(usize, usize, [i32; 2])>),
    Lattice(u8),
    Poly {
        k: usize,
        from: P,
        orig: Vec<P>,
    },
    Pan,
}

/// Screen mapping: `screen = min + off + world · s` (y down, as in the main view).
#[derive(Clone, Copy)]
struct View {
    s: f32,
    off: Vec2,
    min: Pos2,
}
impl View {
    fn to_screen(self, p: P) -> Pos2 {
        Pos2::new(
            self.min.x + self.off.x + p[0] as f32 * self.s,
            self.min.y + self.off.y + p[1] as f32 * self.s,
        )
    }
    fn to_world(self, q: Pos2) -> P {
        [
            ((q.x - self.min.x - self.off.x) / self.s) as f64,
            ((q.y - self.min.y - self.off.y) / self.s) as f64,
        ]
    }
}

/// Corner `(tile, corner, lattice shift of the copy)`.
type CornerHit = (usize, usize, [i32; 2]);
/// Edge `(tile, edge, lattice shift of the copy, nearest point)`.
type EdgeHit = (usize, usize, [i32; 2], P);

/// Where the editor can start from.
struct Source {
    label: String,
    group: &'static str,
    spec: Option<TilingSpec>,
    blank: Option<UnitCell>,
}

pub struct CellEditor {
    pub open: bool,
    cell: UnitCell,
    report: CellReport,
    undo: Vec<UnitCell>,
    redo: Vec<UnitCell>,
    tool: Tool,
    sel: Option<usize>,
    sel_vertex: Option<(usize, usize)>,
    drawing: Vec<P>,
    drag: Option<Drag>,
    snap_vertices: bool,
    snap_edges: bool,
    snap_angles: bool,
    angle_step: f64,
    grid: Grid,
    grid_step: f64,
    weld: bool,
    ngon: usize,
    ngon_match: bool,
    ngon_side: f64,
    ngon_flip: bool,
    ngon_centre: bool,
    ngon_rot: f64,
    rings: i32,
    view: Option<(f32, Vec2)>,
    json: String,
    live: bool,
    source: String,
    snap_mark: Option<(P, SnapKind)>,
    hover_problem: Option<usize>,
    sources: Vec<Source>,
    /// Canvas rectangle from the last frame.
    canvas: Rect,
}

impl CellEditor {
    pub fn new() -> CellEditor {
        let cell = UnitCell {
            a1: [1.0, 0.0],
            a2: [0.0, 1.0],
            polys: vec![vec![[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]]],
        };
        let report = cellcheck::check(&cell, true);
        CellEditor {
            open: false,
            cell,
            report,
            undo: vec![],
            redo: vec![],
            tool: Tool::Select,
            sel: None,
            sel_vertex: None,
            drawing: vec![],
            drag: None,
            snap_vertices: true,
            snap_edges: true,
            snap_angles: false,
            angle_step: 15.0,
            grid: Grid::Off,
            grid_step: 0.25,
            weld: true,
            ngon: 4,
            ngon_match: true,
            ngon_side: 1.0,
            ngon_flip: false,
            ngon_centre: false,
            ngon_rot: 0.0,
            rings: 1,
            view: None,
            json: String::new(),
            live: false,
            source: "Square".into(),
            snap_mark: None,
            hover_problem: None,
            sources: sources(),
            canvas: Rect::NOTHING,
        }
    }

    /// Start editing the unit cell of a lattice tiling.
    pub fn load_spec(&mut self, spec: &TilingSpec) -> Result<(), String> {
        let cell = UnitCell::from_spec(spec)?;
        self.source = families::family(&spec.family)
            .map(|f| f.name.to_string())
            .unwrap_or_default();
        self.set_fresh(cell);
        Ok(())
    }

    /// Debug aid for `JETCARP_CELL_EDITOR`: a family id, a catalog id, a cell JSON file, or anything
    /// else for the current tiling.
    pub fn open_env(&mut self, what: &str, current: &TilingSpec) {
        self.open = true;
        if let Ok(text) = std::fs::read_to_string(what)
            && let Ok(c) = parse_cell(&text)
        {
            self.source = "File".into();
            self.set_fresh(c);
            return;
        }
        let spec = if families::family(what).is_some() {
            TilingSpec::new(what)
        } else if catalog::get(what).is_some() {
            TilingSpec {
                params: Some(serde_json::json!({"catalog": what})),
                ..TilingSpec::new("periodic")
            }
        } else {
            current.clone()
        };
        let _ = self.load_spec(&spec);
    }

    fn set_fresh(&mut self, cell: UnitCell) {
        self.cell = cell;
        self.undo.clear();
        self.redo.clear();
        self.sel = None;
        self.sel_vertex = None;
        self.drawing.clear();
        self.drag = None;
        self.view = None;
        self.recheck(true);
    }

    fn recheck(&mut self, build: bool) {
        self.report = cellcheck::check(&self.cell, build);
        if self.sel.is_some_and(|k| k >= self.cell.polys.len()) {
            self.sel = None;
        }
        if self
            .sel_vertex
            .is_some_and(|(k, v)| k >= self.cell.polys.len() || v >= self.cell.polys[k].len())
        {
            self.sel_vertex = None;
        }
    }

    fn push_undo(&mut self) {
        self.undo.push(self.cell.clone());
        if self.undo.len() > 200 {
            self.undo.remove(0);
        }
        self.redo.clear();
    }

    fn undo(&mut self) -> bool {
        match self.undo.pop() {
            Some(c) => {
                self.redo.push(std::mem::replace(&mut self.cell, c));
                self.recheck(true);
                true
            }
            None => false,
        }
    }

    fn redo(&mut self) -> bool {
        match self.redo.pop() {
            Some(c) => {
                self.undo.push(std::mem::replace(&mut self.cell, c));
                self.recheck(true);
                true
            }
            None => false,
        }
    }

    fn shifts(&self, rings: i32) -> Vec<[i32; 2]> {
        let mut v = Vec::new();
        for i in -rings..=rings {
            for j in -rings..=rings {
                v.push([i, j]);
            }
        }
        v.sort_by_key(|s| s[0].abs() + s[1].abs());
        v
    }

    fn tidy_unit(&self) -> f64 {
        (self.cell.area() / self.cell.polys.len().max(1) as f64)
            .sqrt()
            .max(1e-6)
    }

    // ------------------------------------------------------------------ snapping

    fn grid_round(&self, p: P) -> P {
        let g = self.grid_step.max(1e-6);
        match self.grid {
            Grid::Off => p,
            Grid::Square => [(p[0] / g).round() * g, (p[1] / g).round() * g],
            Grid::Tri => {
                let h = g * geom::S3 / 2.0;
                let j = (p[1] / h).round();
                let o = if (j as i64) & 1 == 1 { g / 2.0 } else { 0.0 };
                [((p[0] - o) / g).round() * g + o, j * h]
            }
        }
    }

    /// Snap a world point. `skip` corners (base indices, at every shift) are not targets;
    /// `base_only` restricts targets to the base cell (used while a lattice vector moves).
    fn snap(
        &self,
        raw: P,
        anchor: Option<P>,
        skip: &[(usize, usize)],
        base_only: bool,
        px: f32,
    ) -> (P, Option<SnapKind>) {
        let r = 10.0 / px as f64;
        let rings = if base_only { 0 } else { self.rings + 1 };
        let shifts = self.shifts(rings);
        if self.snap_vertices {
            let mut best: Option<(f64, P, SnapKind)> = None;
            let mut consider = |q: P, kind: SnapKind| {
                let d = dist(q, raw);
                if d < r && best.is_none_or(|b| d < b.0) {
                    best = Some((d, q, kind));
                }
            };
            for &s in &shifts {
                let t = self.cell.shift(s);
                for (k, poly) in self.cell.polys.iter().enumerate() {
                    for (v, &p) in poly.iter().enumerate() {
                        if !skip.contains(&(k, v)) {
                            consider(add(p, t), SnapKind::Vertex);
                        }
                    }
                }
                if !base_only {
                    consider(t, SnapKind::Lattice);
                }
            }
            for &p in &self.drawing {
                consider(p, SnapKind::Vertex);
            }
            if base_only {
                consider([0.0, 0.0], SnapKind::Lattice);
            }
            if let Some((_, q, k)) = best {
                return (q, Some(k));
            }
        }
        let angled = anchor.filter(|_| self.snap_angles).map(|a| {
            let v = sub(raw, a);
            let step = self.angle_step.max(1.0).to_radians();
            let ang = (v[1].atan2(v[0]) / step).round() * step;
            let dir = [ang.cos(), ang.sin()];
            (a, dir)
        });
        if self.snap_edges {
            let mut best: Option<(f64, P)> = None;
            for &s in &shifts {
                let t = self.cell.shift(s);
                for (k, poly) in self.cell.polys.iter().enumerate() {
                    let m = poly.len();
                    for e in 0..m {
                        if skip.contains(&(k, e)) || skip.contains(&(k, (e + 1) % m)) {
                            continue;
                        }
                        let (a, b) = (add(poly[e], t), add(poly[(e + 1) % m], t));
                        let mut q = nearest_on_seg(raw, a, b);
                        if self.grid != Grid::Off {
                            // Along the edge in grid steps from its start.
                            let l = dist(a, b);
                            let step = self.grid_step.max(1e-6);
                            let tt = ((dist(a, q) / step).round() * step).min(l);
                            q = add(a, scale(sub(b, a), tt / l.max(1e-300)));
                        }
                        if let Some((o, dir)) = angled
                            && let Some(x) = ray_hits_seg(o, dir, a, b)
                            && dist(x, raw) < r
                        {
                            q = x;
                        }
                        let d = dist(q, raw);
                        if d < r && best.is_none_or(|bb| d < bb.0) {
                            best = Some((d, q));
                        }
                    }
                }
            }
            if let Some((_, q)) = best {
                return (q, Some(SnapKind::Edge));
            }
        }
        if let Some((o, dir)) = angled {
            let mut l = dot(sub(raw, o), dir).max(0.0);
            if self.grid != Grid::Off {
                l = (l / self.grid_step).round() * self.grid_step;
            }
            return (add(o, scale(dir, l)), Some(SnapKind::Angle));
        }
        if self.grid != Grid::Off {
            return (self.grid_round(raw), Some(SnapKind::Grid));
        }
        (raw, None)
    }

    // ------------------------------------------------------------------ hit tests (screen space)

    fn hit_vertex(&self, v: &View, q: Pos2) -> Option<CornerHit> {
        let mut best: Option<(f32, CornerHit)> = None;
        for s in self.shifts(self.rings) {
            let t = self.cell.shift(s);
            for (k, poly) in self.cell.polys.iter().enumerate() {
                for (i, &p) in poly.iter().enumerate() {
                    let d = v.to_screen(add(p, t)).distance(q);
                    // Base cell wins ties.
                    let d = if s == [0, 0] { d - 0.5 } else { d };
                    if d < 9.0 && best.is_none_or(|b| d < b.0) {
                        best = Some((d, (k, i, s)));
                    }
                }
            }
        }
        best.map(|b| b.1)
    }

    fn hit_edge(&self, v: &View, q: Pos2) -> Option<EdgeHit> {
        let w = v.to_world(q);
        let mut best: Option<(f32, EdgeHit)> = None;
        for s in self.shifts(self.rings) {
            let t = self.cell.shift(s);
            for (k, poly) in self.cell.polys.iter().enumerate() {
                let m = poly.len();
                for e in 0..m {
                    let (a, b) = (add(poly[e], t), add(poly[(e + 1) % m], t));
                    let n = nearest_on_seg(w, a, b);
                    let d = v.to_screen(n).distance(q);
                    if d < 8.0 && best.is_none_or(|bb| d < bb.0) {
                        best = Some((d, (k, e, s, n)));
                    }
                }
            }
        }
        best.map(|b| b.1)
    }

    fn hit_poly(&self, v: &View, q: Pos2) -> Option<(usize, [i32; 2])> {
        let w = v.to_world(q);
        for s in self.shifts(self.rings) {
            let t = self.cell.shift(s);
            for (k, poly) in self.cell.polys.iter().enumerate().rev() {
                if poly.len() >= 3 && geom::point_in_poly(sub(w, t), poly) {
                    return Some((k, s));
                }
            }
        }
        None
    }

    fn hit_lattice(&self, v: &View, q: Pos2) -> Option<u8> {
        if v.to_screen(self.cell.a1).distance(q) < 10.0 {
            Some(1)
        } else if v.to_screen(self.cell.a2).distance(q) < 10.0 {
            Some(2)
        } else {
            None
        }
    }

    /// Every base-cell corner that sits on the grabbed corner, with the shift that puts it there.
    fn welded(&self, k: usize, i: usize, s: [i32; 2]) -> Vec<(usize, usize, [i32; 2])> {
        let g = add(self.cell.polys[k][i], self.cell.shift(s));
        if !self.weld {
            return vec![(k, i, s)];
        }
        let tol = 1e-6 * self.tidy_unit();
        let mut out = Vec::new();
        for (k2, poly) in self.cell.polys.iter().enumerate() {
            for (i2, &p) in poly.iter().enumerate() {
                let f = self.cell.to_lattice(sub(g, p));
                let sh = [f[0].round() as i32, f[1].round() as i32];
                if dist(add(p, self.cell.shift(sh)), g) < tol {
                    out.push((k2, i2, sh));
                }
            }
        }
        out
    }

    // ------------------------------------------------------------------ edits

    fn close_drawing(&mut self) {
        if self.drawing.len() >= 3 {
            let pts = std::mem::take(&mut self.drawing);
            self.push_undo();
            self.cell.polys.push(pts);
            self.sel = Some(self.cell.polys.len() - 1);
            self.sel_vertex = None;
            self.recheck(true);
        }
    }

    /// The polygon the stamp tool would place for a pointer at `q`.
    fn stamp_preview(&self, v: &View, q: Pos2) -> Vec<P> {
        let n = self.ngon.clamp(3, 24);
        if let Some((k, e, s, _)) = self.hit_edge(v, q) {
            let poly = &self.cell.polys[k];
            let t = self.cell.shift(s);
            let (a, b) = (add(poly[e], t), add(poly[(e + 1) % poly.len()], t));
            // New tile on the far side of the edge from its owner.
            let (mut p, mut r) = if geom::signed_area2(poly) > 0.0 {
                (b, a)
            } else {
                (a, b)
            };
            if self.ngon_flip {
                std::mem::swap(&mut p, &mut r);
            }
            let el = dist(p, r);
            let side = if self.ngon_match { el } else { self.ngon_side };
            let dir = scale(sub(r, p), 1.0 / el.max(1e-12));
            let start = if self.ngon_centre {
                add(p, scale(dir, (el - side) / 2.0))
            } else {
                p
            };
            return cellcheck::regular_polygon(start, add(start, dir), n, side);
        }
        let c = self.snap(v.to_world(q), None, &[], false, v.s).0;
        cellcheck::regular_polygon_at(c, n, self.ngon_side, self.ngon_rot - 90.0)
    }

    fn delete_selected(&mut self) -> Option<String> {
        if let Some((k, i)) = self.sel_vertex {
            if self.cell.polys[k].len() <= 3 {
                return Some("A tile needs at least 3 corners; delete the tile instead.".into());
            }
            self.push_undo();
            self.cell.polys[k].remove(i);
            self.sel_vertex = None;
            self.recheck(true);
        } else if let Some(k) = self.sel {
            self.push_undo();
            self.cell.polys.remove(k);
            self.sel = None;
            self.recheck(true);
        }
        None
    }

    fn fit(&mut self, rect: Rect) {
        let mut pts: Vec<P> = vec![
            [0.0, 0.0],
            self.cell.a1,
            self.cell.a2,
            add(self.cell.a1, self.cell.a2),
        ];
        for p in &self.cell.polys {
            pts.extend_from_slice(p);
        }
        let b = geom::bbox(&pts);
        let pad = 1.0 + 1.2 * self.rings as f64;
        let s = (rect.width() as f64 / ((b[2] - b[0]) * pad + 1e-3))
            .min(rect.height() as f64 / ((b[3] - b[1]) * pad + 1e-3))
            .clamp(2.0, 2000.0);
        let off = Vec2::new(
            (rect.width() as f64 / 2.0 - s * (b[0] + b[2]) / 2.0) as f32,
            (rect.height() as f64 / 2.0 - s * (b[1] + b[3]) / 2.0) as f32,
        );
        self.view = Some((s as f32, off));
    }

    // ------------------------------------------------------------------ window

    /// Draw the editor window. Returns actions for the app.
    pub fn show(&mut self, ctx: &egui::Context) -> Vec<EditorAction> {
        let mut actions = Vec::new();
        if !self.open {
            return actions;
        }
        let mut open = self.open;
        crate::popout::tool_window(
            ctx,
            crate::popout::Spec::new(KEY, "Unit cell editor")
                .size(1080.0, 640.0)
                .configure(|w| w.default_size([1080.0, 640.0]).resizable(true)),
            &mut open,
            |ui| {
                ui.horizontal_top(|ui| {
                    let size = egui::vec2(
                        (ui.available_width() - 330.0).max(360.0),
                        (ui.available_height()).clamp(480.0, 900.0),
                    );
                    let edited = self.canvas(ui, size, &mut actions);
                    ui.vertical(|ui| {
                        ui.set_width(320.0);
                        egui::ScrollArea::vertical()
                            .id_salt("cell-controls")
                            .show(ui, |ui| {
                                let changed = self.controls(ui, &mut actions);
                                if (edited || changed) && self.live && self.report.ok() {
                                    actions.push(EditorAction::Apply(self.cell.spec()));
                                }
                            });
                    });
                });
            },
        );
        self.open = open;
        actions
    }

    fn controls(&mut self, ui: &mut egui::Ui, actions: &mut Vec<EditorAction>) -> bool {
        let mut changed = false;
        // Start from.
        ui.horizontal(|ui| {
            ui.label("Start from");
            let mut pick: Option<usize> = None;
            egui::ComboBox::from_id_salt("cell-source")
                .selected_text(self.source.clone())
                .width(210.0)
                .show_ui(ui, |ui| {
                    let mut group = "";
                    for (i, s) in self.sources.iter().enumerate() {
                        if s.group != group {
                            group = s.group;
                            ui.label(RichText::new(group).weak().small());
                        }
                        if ui.selectable_label(false, &s.label).clicked() {
                            pick = Some(i);
                        }
                    }
                });
            if let Some(i) = pick {
                let label = self.sources[i].label.clone();
                let res = match (&self.sources[i].spec, &self.sources[i].blank) {
                    (_, Some(c)) => Ok(c.clone()),
                    (Some(spec), _) => UnitCell::from_spec(spec),
                    _ => Err("nothing to load".into()),
                };
                match res {
                    Ok(c) => {
                        self.push_undo();
                        let undo = std::mem::take(&mut self.undo);
                        self.set_fresh(c);
                        self.undo = undo;
                        self.source = label;
                        changed = true;
                    }
                    Err(e) => actions.push(EditorAction::Message(e, true)),
                }
            }
        });
        ui.label(
            RichText::new(
                "Ghost copies show the neighbouring cells. Scroll to zoom; drag empty space or use the right button to pan.",
            )
            .small()
            .weak(),
        );
        ui.separator();
        // Tools.
        ui.horizontal(|ui| {
            for (t, label, tip) in [
                (Tool::Select, "Select", "Drag corners, tiles, and the lattice arrows. Double-click an edge to add a corner."),
                (Tool::Draw, "Draw tile", "Click corners; click the first corner, double-click, or press Enter to close. Right-click removes the last corner; Esc cancels."),
                (Tool::Stamp, "Stamp n-gon", "Click an edge to stamp a regular polygon on it, or click empty space."),
            ] {
                if ui.add(egui::Button::new(label).selected(self.tool == t)).on_hover_text(tip).clicked() {
                    self.tool = t;
                    self.drawing.clear();
                }
            }
        });
        if self.tool == Tool::Stamp {
            ui.horizontal(|ui| {
                ui.label("Sides");
                ui.add(egui::DragValue::new(&mut self.ngon).range(3..=24));
                for n in [3, 4, 6, 8, 12] {
                    if ui.small_button(n.to_string()).clicked() {
                        self.ngon = n;
                    }
                }
            });
            ui.horizontal(|ui| {
                ui.checkbox(&mut self.ngon_match, "Side = edge length");
                ui.add_enabled(
                    !self.ngon_match,
                    egui::DragValue::new(&mut self.ngon_side)
                        .speed(0.01)
                        .range(0.01..=100.0),
                );
            });
            ui.horizontal(|ui| {
                ui.checkbox(&mut self.ngon_flip, "Other side");
                ui.checkbox(&mut self.ngon_centre, "Centre on edge");
            });
            ui.horizontal(|ui| {
                ui.label("In empty space: side");
                ui.add(
                    egui::DragValue::new(&mut self.ngon_side)
                        .speed(0.01)
                        .range(0.01..=100.0),
                );
                ui.label("turn");
                ui.add(
                    egui::DragValue::new(&mut self.ngon_rot)
                        .speed(1.0)
                        .suffix("°"),
                );
            });
        }
        ui.horizontal_wrapped(|ui| {
            ui.label("Snap");
            ui.checkbox(&mut self.snap_vertices, "Corners");
            ui.checkbox(&mut self.snap_edges, "Edges");
            ui.checkbox(&mut self.snap_angles, "Angles");
            ui.add_enabled(
                self.snap_angles,
                egui::DragValue::new(&mut self.angle_step)
                    .range(1.0..=90.0)
                    .suffix("°"),
            );
        });
        ui.horizontal(|ui| {
            ui.label("Grid");
            ui.radio_value(&mut self.grid, Grid::Off, "Off");
            ui.radio_value(&mut self.grid, Grid::Square, "Square");
            ui.radio_value(&mut self.grid, Grid::Tri, "Triangle");
            ui.add(
                egui::DragValue::new(&mut self.grid_step)
                    .speed(0.01)
                    .range(0.01..=10.0),
            );
        });
        ui.horizontal(|ui| {
            ui.checkbox(&mut self.weld, "Move shared corners together")
                .on_hover_text(
                    "Dragging a corner also drags every corner (in any tile or copy) sitting on it",
                );
        });
        ui.horizontal(|ui| {
            ui.label("Ghost rings");
            ui.add(egui::Slider::new(&mut self.rings, 1..=3));
        });
        // Lattice.
        ui.separator();
        ui.horizontal(|ui| {
            ui.colored_label(A1_COL, "a1");
            let before = self.cell.clone();
            let mut c = false;
            c |= ui
                .add(
                    egui::DragValue::new(&mut self.cell.a1[0])
                        .speed(0.01)
                        .max_decimals(6),
                )
                .changed();
            c |= ui
                .add(
                    egui::DragValue::new(&mut self.cell.a1[1])
                        .speed(0.01)
                        .max_decimals(6),
                )
                .changed();
            ui.colored_label(A2_COL, "a2");
            c |= ui
                .add(
                    egui::DragValue::new(&mut self.cell.a2[0])
                        .speed(0.01)
                        .max_decimals(6),
                )
                .changed();
            c |= ui
                .add(
                    egui::DragValue::new(&mut self.cell.a2[1])
                        .speed(0.01)
                        .max_decimals(6),
                )
                .changed();
            if c {
                self.undo.push(before);
                self.redo.clear();
                changed = true;
            }
        });
        // Selection.
        ui.separator();
        if let Some(k) = self.sel {
            let col = tile_color(k);
            ui.horizontal(|ui| {
                let (r, _) = ui.allocate_exact_size(egui::vec2(12.0, 12.0), Sense::hover());
                ui.painter().rect_filled(r, 2.0, col);
                let t = &self.report.tiles[k];
                ui.label(
                    RichText::new(format!(
                        "Tile {}: {} corners, area {:.4}",
                        k + 1,
                        t.corners,
                        t.area
                    ))
                    .strong(),
                );
            });
            let t = &self.report.tiles[k];
            ui.label(
                RichText::new(format!(
                    "{} edge neighbours (teal), {} touching (edge + yellow)",
                    t.edge.len(),
                    t.touch.len()
                ))
                .small(),
            );
            ui.horizontal(|ui| {
                if ui.button("Delete tile").clicked() {
                    self.sel_vertex = None;
                    self.delete_selected();
                    changed = true;
                }
                if ui
                    .button("Duplicate")
                    .on_hover_text("A copy nudged aside, to drag into place")
                    .clicked()
                {
                    self.push_undo();
                    let d = self.tidy_unit() * 0.3;
                    let copy: Vec<P> = self.cell.polys[k].iter().map(|&p| add(p, [d, d])).collect();
                    self.cell.polys.push(copy);
                    self.sel = Some(self.cell.polys.len() - 1);
                    self.sel_vertex = None;
                    changed = true;
                }
                if ui
                    .button("Move into cell")
                    .on_hover_text(
                        "Shift by a lattice vector so its centre lies in the parallelogram",
                    )
                    .clicked()
                {
                    let f = self.cell.to_lattice(geom::centroid(&self.cell.polys[k]));
                    let s = [-(f[0].floor() as i32), -(f[1].floor() as i32)];
                    if s != [0, 0] {
                        self.push_undo();
                        let t = self.cell.shift(s);
                        for p in &mut self.cell.polys[k] {
                            *p = add(*p, t);
                        }
                        changed = true;
                    }
                }
            });
            if let Some((vk, vi)) = self.sel_vertex.filter(|v| v.0 == k) {
                ui.horizontal(|ui| {
                    ui.label(format!("Corner {}", vi + 1));
                    let before = self.cell.clone();
                    let p = &mut self.cell.polys[vk][vi];
                    let mut c = ui
                        .add(egui::DragValue::new(&mut p[0]).speed(0.01).max_decimals(6))
                        .changed();
                    c |= ui
                        .add(egui::DragValue::new(&mut p[1]).speed(0.01).max_decimals(6))
                        .changed();
                    if c {
                        self.undo.push(before);
                        self.redo.clear();
                        changed = true;
                    }
                });
                ui.horizontal(|ui| {
                    if ui
                        .button("Insert corner after")
                        .on_hover_text("Adds a corner halfway along the next edge")
                        .clicked()
                    {
                        self.push_undo();
                        let poly = &mut self.cell.polys[vk];
                        let m = poly.len();
                        let mid = scale(add(poly[vi], poly[(vi + 1) % m]), 0.5);
                        poly.insert(vi + 1, mid);
                        self.sel_vertex = Some((vk, vi + 1));
                        changed = true;
                    }
                    if ui.button("Delete corner").clicked() {
                        if let Some(m) = self.delete_selected() {
                            actions.push(EditorAction::Message(m, true));
                        }
                        changed = true;
                    }
                });
            }
        } else {
            ui.label(RichText::new("Click a tile to select it.").weak());
        }
        ui.horizontal(|ui| {
            if ui
                .add_enabled(!self.undo.is_empty(), egui::Button::new("Undo"))
                .clicked()
            {
                changed |= self.undo();
            }
            if ui
                .add_enabled(!self.redo.is_empty(), egui::Button::new("Redo"))
                .clicked()
            {
                changed |= self.redo();
            }
            if ui.button("Fit view").clicked() {
                self.view = None;
            }
        });
        if changed {
            self.recheck(true);
        }
        // Validation.
        ui.separator();
        self.report_ui(ui);
        // Apply.
        ui.separator();
        ui.horizontal(|ui| {
            let ok = self.report.ok();
            if ui
                .add_enabled(
                    ok,
                    egui::Button::new(RichText::new("Use this cell").strong()),
                )
                .on_hover_text("Rebuild the simulation on this tiling")
                .on_disabled_hover_text("Fix the errors above first")
                .clicked()
            {
                actions.push(EditorAction::Apply(self.cell.spec()));
            }
            ui.checkbox(&mut self.live, "Live")
                .on_hover_text("Rebuild the simulation after every valid edit");
        });
        ui.horizontal(|ui| {
            if ui.button("Copy cell JSON").clicked() {
                let text = serde_json::to_string_pretty(&self.cell.spec()).unwrap_or_default();
                ui.ctx().copy_text(text.clone());
                self.json = text;
                actions.push(EditorAction::Message(
                    "Unit cell JSON copied to the clipboard.".into(),
                    false,
                ));
            }
            if ui.button("Load JSON below").clicked() {
                match parse_cell(&self.json) {
                    Ok(c) => {
                        self.push_undo();
                        let undo = std::mem::take(&mut self.undo);
                        self.set_fresh(c);
                        self.undo = undo;
                        self.source = "Pasted JSON".into();
                        changed = true;
                    }
                    Err(e) => actions.push(EditorAction::Message(format!("Cell JSON: {e}"), true)),
                }
            }
        });
        ui.add(
            egui::TextEdit::multiline(&mut self.json)
                .desired_rows(4)
                .code_editor()
                .desired_width(f32::INFINITY)
                .hint_text("Paste a periodic tiling ({\"family\": \"periodic\", \"params\": {...}}), bare {a1, a2, polys}, or any lattice tiling's JSON"),
        );
        changed
    }

    fn report_ui(&mut self, ui: &mut egui::Ui) {
        let r = &self.report;
        let pct = if r.cell_area > 0.0 {
            100.0 * r.tile_area / r.cell_area
        } else {
            0.0
        };
        ui.label(
            RichText::new(format!(
                "Cell area {:.4} · tiles {:.4} ({pct:.2}%)",
                r.cell_area, r.tile_area
            ))
            .small(),
        );
        let errors = r.problems.iter().filter(|p| p.is_error()).count();
        if r.problems.is_empty() {
            ui.colored_label(Color32::LIGHT_GREEN, "Valid tiling");
        } else if errors == 0 {
            ui.colored_label(Color32::LIGHT_GREEN, "Valid tiling (with notes)");
        } else {
            ui.colored_label(RED, format!("{errors} problem(s)"));
        }
        let mut hovered = None;
        let mut clicked = None;
        egui::ScrollArea::vertical()
            .id_salt("cell-problems")
            .max_height(130.0)
            .show(ui, |ui| {
                // Long lists of open edges say the same thing; show a few.
                let mut open_shown = 0;
                for (i, p) in r.problems.iter().enumerate() {
                    if matches!(p, Problem::Unmatched { .. }) {
                        open_shown += 1;
                        if open_shown > 4 {
                            continue;
                        }
                    }
                    let col = if p.is_error() { RED } else { ORANGE };
                    let resp = ui.add(
                        egui::Label::new(
                            RichText::new(format!("• {}", p.message()))
                                .color(col)
                                .small(),
                        )
                        .sense(Sense::click()),
                    );
                    if resp.hovered() {
                        hovered = Some(i);
                    }
                    if resp.clicked() {
                        clicked = Some(i);
                    }
                }
                if open_shown > 4 {
                    ui.label(
                        RichText::new(format!(
                            "… and {} more open edges (orange on the canvas)",
                            open_shown - 4
                        ))
                        .color(ORANGE)
                        .small(),
                    );
                }
            });
        self.hover_problem = hovered;
        if let Some(i) = clicked
            && let Some(&k) = self.report.problems[i].polys().first()
        {
            self.sel = Some(k);
            self.sel_vertex = None;
        }
        let r = &self.report;
        let (e, t) = r.degree_sets();
        ui.label(
            RichText::new(format!("Neighbours: {e:?} by edge, {t:?} touching"))
                .strong()
                .small(),
        );
        let mut pick = None;
        egui::ScrollArea::vertical()
            .id_salt("cell-tiles")
            .max_height(150.0)
            .show(ui, |ui| {
                egui::Grid::new("cell-tile-table")
                    .striped(true)
                    .num_columns(5)
                    .show(ui, |ui| {
                        for h in ["Tile", "Corners", "Area", "Edge", "Touch"] {
                            ui.label(RichText::new(h).small().weak());
                        }
                        ui.end_row();
                        for (k, t) in r.tiles.iter().enumerate() {
                            let sel = self.sel == Some(k);
                            ui.horizontal(|ui| {
                                let (r, _) =
                                    ui.allocate_exact_size(egui::vec2(10.0, 10.0), Sense::hover());
                                ui.painter().rect_filled(r, 2.0, tile_color(k));
                                if ui.selectable_label(sel, (k + 1).to_string()).clicked() {
                                    pick = Some(k);
                                }
                            });
                            ui.label(t.corners.to_string());
                            ui.label(format!("{:.3}", t.area));
                            ui.label(t.edge.len().to_string());
                            ui.label(t.touch.len().to_string());
                            ui.end_row();
                        }
                    });
            });
        if let Some(k) = pick {
            self.sel = Some(k);
            self.sel_vertex = None;
        }
    }

    // ------------------------------------------------------------------ canvas

    /// Draw the cell and handle editing. Returns true when an edit finished.
    fn canvas(&mut self, ui: &mut egui::Ui, size: Vec2, actions: &mut Vec<EditorAction>) -> bool {
        let (rect, resp) = ui.allocate_exact_size(size, Sense::click_and_drag());
        self.canvas = rect;
        if self.view.is_none() {
            self.fit(rect);
        }
        // Zoom about the pointer, pan with the right or middle button.
        if resp.hovered() {
            let (scroll, zoom) = ui.input(|i| (i.smooth_scroll_delta.y, i.zoom_delta()));
            let f = (scroll * 0.002).exp() * zoom;
            if (f - 1.0).abs() > 1e-4
                && let (Some((s, off)), Some(hp)) = (self.view, resp.hover_pos())
            {
                let ns = (s * f).clamp(2.0, 4000.0);
                let anchor = hp - rect.min - off;
                self.view = Some((ns, hp - rect.min - anchor * (ns / s)));
            }
        }
        if (resp.dragged_by(egui::PointerButton::Secondary)
            || resp.dragged_by(egui::PointerButton::Middle))
            && let Some((s, off)) = self.view
        {
            self.view = Some((s, off + resp.drag_delta()));
        }
        let (s, off) = self.view.unwrap();
        let v = View {
            s,
            off,
            min: rect.min,
        };
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 4.0, BG);
        self.draw_grid(&painter, &v, rect);
        let hover = resp.hover_pos();
        let edited = self.interact(ui, &resp, &v, actions);
        self.draw(&painter, &v, hover);
        // Keys while the pointer is over the canvas.
        if resp.hovered() && !ui.ctx().egui_wants_keyboard_input() {
            let (del, undo, redo, enter, esc) = ui.input(|i| {
                let cmd = i.modifiers.command;
                (
                    i.key_pressed(egui::Key::Delete) || i.key_pressed(egui::Key::Backspace),
                    cmd && !i.modifiers.shift && i.key_pressed(egui::Key::Z),
                    cmd && ((i.modifiers.shift && i.key_pressed(egui::Key::Z))
                        || i.key_pressed(egui::Key::Y)),
                    i.key_pressed(egui::Key::Enter),
                    i.key_pressed(egui::Key::Escape),
                )
            });
            if del && self.drawing.is_empty() {
                if let Some(m) = self.delete_selected() {
                    actions.push(EditorAction::Message(m, true));
                }
                return true;
            }
            if del {
                self.drawing.pop();
            }
            if undo {
                return self.undo();
            }
            if redo {
                return self.redo();
            }
            if enter && self.tool == Tool::Draw {
                self.close_drawing();
                return true;
            }
            if esc {
                self.drawing.clear();
                self.drag = None;
            }
        }
        edited
    }

    fn interact(
        &mut self,
        ui: &egui::Ui,
        resp: &egui::Response,
        v: &View,
        _actions: &mut Vec<EditorAction>,
    ) -> bool {
        let mut edited = false;
        self.snap_mark = None;
        let press = ui.input(|i| i.pointer.press_origin());
        match self.tool {
            Tool::Select => {
                if resp.double_clicked()
                    && let Some(q) = resp.interact_pointer_pos()
                    && let Some((k, e, s, _)) = self.hit_edge(v, q)
                {
                    // Insert a corner on the edge.
                    let t = self.cell.shift(s);
                    let poly = &self.cell.polys[k];
                    let (a, b) = (add(poly[e], t), add(poly[(e + 1) % poly.len()], t));
                    let (p, _) = self.snap(v.to_world(q), None, &[], false, v.s);
                    let p = nearest_on_seg(p, a, b);
                    self.push_undo();
                    self.cell.polys[k].insert(e + 1, sub(p, t));
                    self.sel = Some(k);
                    self.sel_vertex = Some((k, e + 1));
                    self.recheck(true);
                    return true;
                }
                if resp.drag_started_by(egui::PointerButton::Primary)
                    && let Some(q) = press
                {
                    if let Some(l) = self.hit_lattice(v, q) {
                        self.push_undo();
                        self.drag = Some(Drag::Lattice(l));
                    } else if let Some((k, i, s)) = self.hit_vertex(v, q) {
                        self.push_undo();
                        self.sel = Some(k);
                        self.sel_vertex = Some((k, i));
                        self.drag = Some(Drag::Vertices(self.welded(k, i, s)));
                    } else if let Some((k, _)) = self.hit_poly(v, q) {
                        self.push_undo();
                        self.sel = Some(k);
                        self.sel_vertex = None;
                        self.drag = Some(Drag::Poly {
                            k,
                            from: v.to_world(q),
                            orig: self.cell.polys[k].clone(),
                        });
                    } else {
                        self.drag = Some(Drag::Pan);
                    }
                } else if resp.clicked()
                    && let Some(q) = resp.interact_pointer_pos()
                {
                    if let Some((k, i, _)) = self.hit_vertex(v, q) {
                        self.sel = Some(k);
                        self.sel_vertex = Some((k, i));
                    } else if let Some((k, _)) = self.hit_poly(v, q) {
                        self.sel = Some(k);
                        self.sel_vertex = None;
                    } else {
                        self.sel = None;
                        self.sel_vertex = None;
                    }
                }
            }
            Tool::Draw => {
                if resp.double_clicked() {
                    self.close_drawing();
                    return true;
                }
                if resp.secondary_clicked() {
                    self.drawing.pop();
                } else if resp.clicked()
                    && let Some(q) = resp.interact_pointer_pos()
                {
                    if self.drawing.len() >= 3 && v.to_screen(self.drawing[0]).distance(q) < 10.0 {
                        self.close_drawing();
                        return true;
                    }
                    let (p, _) =
                        self.snap(v.to_world(q), self.drawing.last().copied(), &[], false, v.s);
                    if self.drawing.last().is_none_or(|l| dist(*l, p) > 1e-9) {
                        self.drawing.push(p);
                    }
                } else if resp.drag_started_by(egui::PointerButton::Primary) {
                    self.drag = Some(Drag::Pan);
                }
            }
            Tool::Stamp => {
                if resp.clicked()
                    && let Some(q) = resp.interact_pointer_pos()
                {
                    let poly = self.stamp_preview(v, q);
                    self.push_undo();
                    self.cell.polys.push(poly);
                    self.sel = Some(self.cell.polys.len() - 1);
                    self.sel_vertex = None;
                    self.recheck(true);
                    return true;
                } else if resp.drag_started_by(egui::PointerButton::Primary) {
                    self.drag = Some(Drag::Pan);
                }
            }
        }
        // Dragging.
        if let Some(d) = self.drag.clone()
            && resp.dragged_by(egui::PointerButton::Primary)
            && let Some(q) = resp.interact_pointer_pos()
        {
            let w = v.to_world(q);
            match d {
                Drag::Pan => {
                    if let Some((s, off)) = self.view {
                        self.view = Some((s, off + resp.drag_delta()));
                    }
                }
                Drag::Lattice(l) => {
                    let (p, kind) = self.snap(w, Some([0.0, 0.0]), &[], true, v.s);
                    self.snap_mark = kind.map(|k| (p, k));
                    if l == 1 {
                        self.cell.a1 = p;
                    } else {
                        self.cell.a2 = p;
                    }
                    self.recheck(false);
                }
                Drag::Vertices(list) => {
                    let (k, i, s) = list
                        .iter()
                        .copied()
                        .find(|x| Some((x.0, x.1)) == self.sel_vertex)
                        .unwrap_or(list[0]);
                    let poly = &self.cell.polys[k];
                    let prev = add(poly[(i + poly.len() - 1) % poly.len()], self.cell.shift(s));
                    let skip: Vec<(usize, usize)> = list.iter().map(|x| (x.0, x.1)).collect();
                    let (p, kind) = self.snap(w, Some(prev), &skip, false, v.s);
                    self.snap_mark = kind.map(|k| (p, k));
                    for &(k2, i2, s2) in &list {
                        self.cell.polys[k2][i2] = sub(p, self.cell.shift(s2));
                    }
                    self.recheck(false);
                }
                Drag::Poly { k, from, orig } => {
                    let mut delta = sub(w, from);
                    // Snap the nearest corner of the moved tile onto another tile's corner.
                    let mut best: Option<(f64, P)> = None;
                    if self.snap_vertices {
                        let r = 10.0 / v.s as f64;
                        let mut targets: Vec<P> = Vec::new();
                        for sh in self.shifts(self.rings + 1) {
                            let t = self.cell.shift(sh);
                            targets.push(t);
                            for (k2, poly) in self.cell.polys.iter().enumerate() {
                                if k2 != k {
                                    targets.extend(poly.iter().map(|&p2| add(p2, t)));
                                }
                            }
                        }
                        for target in targets {
                            for &p in &orig {
                                let d = dist(add(p, delta), target);
                                if d < r && best.is_none_or(|b| d < b.0) {
                                    best = Some((d, sub(target, p)));
                                }
                            }
                        }
                    }
                    if let Some((_, dd)) = best {
                        delta = dd;
                        self.snap_mark = Some((add(orig[0], delta), SnapKind::Vertex));
                    } else if self.grid != Grid::Off {
                        let g = self.grid_round(add(orig[0], delta));
                        delta = sub(g, orig[0]);
                    }
                    self.cell.polys[k] = orig.iter().map(|&p| add(p, delta)).collect();
                    self.recheck(false);
                }
            }
        }
        if resp.drag_stopped()
            && let Some(d) = self.drag.take()
            && !matches!(d, Drag::Pan)
        {
            // Drop no-op drags from the undo stack.
            if self.undo.last() == Some(&self.cell) {
                self.undo.pop();
            }
            self.recheck(true);
            edited = true;
        }
        // Hover feedback: what a click would snap to.
        if self.drag.is_none()
            && let Some(q) = resp.hover_pos()
            && self.tool == Tool::Draw
        {
            let (p, kind) = self.snap(v.to_world(q), self.drawing.last().copied(), &[], false, v.s);
            self.snap_mark = kind.map(|k| (p, k));
        }
        edited
    }

    fn draw_grid(&self, painter: &egui::Painter, v: &View, rect: Rect) {
        if self.grid == Grid::Off {
            return;
        }
        let g = self.grid_step.max(1e-6);
        if g * (v.s as f64) < 6.0 {
            return;
        }
        let tl = v.to_world(rect.min);
        let br = v.to_world(rect.max);
        let row_h = if self.grid == Grid::Tri {
            g * geom::S3 / 2.0
        } else {
            g
        };
        let mut j = (tl[1] / row_h).floor();
        while j * row_h <= br[1] {
            let o = if self.grid == Grid::Tri && (j as i64) & 1 == 1 {
                g / 2.0
            } else {
                0.0
            };
            let mut i = (tl[0] / g).floor() - 1.0;
            while i * g <= br[0] + g {
                painter.circle_filled(
                    v.to_screen([i * g + o, j * row_h]),
                    1.0,
                    Color32::from_gray(70),
                );
                i += 1.0;
            }
            j += 1.0;
        }
    }

    fn draw(&self, painter: &egui::Painter, v: &View, hover: Option<Pos2>) {
        let fill = |poly: &[P], col: Color32, t: P| {
            if poly.len() < 3 {
                return;
            }
            let pts: Vec<P> = poly.iter().map(|&p| add(p, t)).collect();
            let mut mesh = egui::Mesh::default();
            for p in &pts {
                mesh.colored_vertex(v.to_screen(*p), col);
            }
            for tr in geom::triangulate(&pts) {
                mesh.add_triangle(tr[0], tr[1], tr[2]);
            }
            painter.add(egui::Shape::mesh(mesh));
        };
        let outline = |poly: &[P], t: P, stroke: Stroke| {
            if poly.len() < 2 {
                return;
            }
            let pts: Vec<Pos2> = poly.iter().map(|&p| v.to_screen(add(p, t))).collect();
            painter.add(egui::Shape::closed_line(pts, stroke));
        };
        let bad: Vec<usize> = self
            .report
            .problems
            .iter()
            .filter(|p| p.is_error() && !matches!(p, Problem::Overlap { .. } | Problem::Gap { .. }))
            .flat_map(|p| p.polys())
            .collect();
        let shifts = self.shifts(self.rings);
        // Ghost copies.
        for &s in &shifts {
            if s == [0, 0] {
                continue;
            }
            let t = self.cell.shift(s);
            for (k, poly) in self.cell.polys.iter().enumerate() {
                fill(poly, dim(tile_color(k), 0.35), t);
                outline(poly, t, Stroke::new(1.0, Color32::from_gray(80)));
            }
        }
        // Base cell.
        for (k, poly) in self.cell.polys.iter().enumerate() {
            fill(poly, dim(tile_color(k), 0.8), [0.0, 0.0]);
            outline(poly, [0.0, 0.0], Stroke::new(1.5, Color32::from_gray(200)));
        }
        // Lattice parallelogram.
        let (a1, a2) = (self.cell.a1, self.cell.a2);
        let para: Vec<Pos2> = [[0.0, 0.0], a1, add(a1, a2), a2]
            .iter()
            .map(|&p| v.to_screen(p))
            .collect();
        let mut closed = para.clone();
        closed.push(para[0]);
        painter.extend(egui::Shape::dashed_line(
            &closed,
            Stroke::new(1.5, Color32::from_gray(230)),
            6.0,
            4.0,
        ));
        for &s in &self.shifts(self.rings + 1) {
            painter.circle_filled(
                v.to_screen(self.cell.shift(s)),
                2.0,
                Color32::from_gray(160),
            );
        }
        // Problems.
        for &s in &shifts {
            let t = self.cell.shift(s);
            for p in &self.report.problems {
                match p {
                    Problem::Overlap { pieces, .. } => {
                        for piece in pieces {
                            fill(
                                piece,
                                Color32::from_rgba_unmultiplied(0xff, 0x30, 0x30, 150),
                                t,
                            );
                            outline(piece, t, Stroke::new(1.0, RED));
                        }
                    }
                    Problem::Gap { spots, .. } => {
                        for &sp in spots {
                            let c = v.to_screen(add(sp, t));
                            painter.circle_stroke(c, 6.0, Stroke::new(2.0, ORANGE));
                            painter.circle_filled(c, 2.0, ORANGE);
                        }
                    }
                    Problem::Unmatched { from, to, .. } => {
                        painter.line_segment(
                            [v.to_screen(add(*from, t)), v.to_screen(add(*to, t))],
                            Stroke::new(3.0, ORANGE),
                        );
                    }
                    Problem::SelfIntersect { at, .. } | Problem::ZeroEdge { at, .. } => {
                        painter.circle_stroke(v.to_screen(add(*at, t)), 7.0, Stroke::new(2.5, RED));
                    }
                    _ => {}
                }
            }
        }
        for &k in &bad {
            if k < self.cell.polys.len() {
                outline(&self.cell.polys[k], [0.0, 0.0], Stroke::new(3.0, RED));
            }
        }
        if let Some(i) = self.hover_problem
            && let Some(p) = self.report.problems.get(i)
        {
            if let Some(at) = p.at() {
                painter.circle_stroke(v.to_screen(at), 16.0, Stroke::new(3.0, Color32::WHITE));
            }
            for k in p.polys() {
                if k < self.cell.polys.len() {
                    outline(
                        &self.cell.polys[k],
                        [0.0, 0.0],
                        Stroke::new(3.0, Color32::WHITE),
                    );
                }
            }
        }
        // Selection and its measured neighbours.
        if let Some(k) = self.sel
            && let Some(info) = self.report.tiles.get(k)
        {
            for c in &info.touch {
                if !info.edge.contains(c) {
                    outline(
                        &self.cell.polys[c.poly],
                        self.cell.shift(c.shift),
                        Stroke::new(2.0, YELLOW),
                    );
                }
            }
            for c in &info.edge {
                outline(
                    &self.cell.polys[c.poly],
                    self.cell.shift(c.shift),
                    Stroke::new(2.5, TEAL),
                );
            }
            outline(
                &self.cell.polys[k],
                [0.0, 0.0],
                Stroke::new(3.0, Color32::WHITE),
            );
        }
        // Corner handles and tile numbers.
        for (k, poly) in self.cell.polys.iter().enumerate() {
            for (i, &p) in poly.iter().enumerate() {
                let r = Rect::from_center_size(v.to_screen(p), egui::vec2(7.0, 7.0));
                let on = self.sel_vertex == Some((k, i));
                painter.rect_filled(r, 1.0, if on { Color32::WHITE } else { BG });
                painter.rect_stroke(
                    r,
                    1.0,
                    Stroke::new(1.2, Color32::from_gray(220)),
                    egui::StrokeKind::Middle,
                );
            }
            if poly.len() >= 3 {
                painter.text(
                    v.to_screen(geom::centroid(poly)),
                    egui::Align2::CENTER_CENTER,
                    (k + 1).to_string(),
                    egui::FontId::proportional(13.0),
                    Color32::WHITE,
                );
            }
        }
        // Lattice vectors.
        for (a, col, name) in [(a1, A1_COL, "a1"), (a2, A2_COL, "a2")] {
            let (o, tip) = (v.to_screen([0.0, 0.0]), v.to_screen(a));
            painter.line_segment([o, tip], Stroke::new(2.5, col));
            let d = (tip - o).normalized();
            let n = egui::vec2(-d.y, d.x);
            for side in [-1.0, 1.0] {
                painter.line_segment(
                    [tip, tip - d * 11.0 + n * (5.0 * side)],
                    Stroke::new(2.5, col),
                );
            }
            painter.circle(tip, 6.0, BG, Stroke::new(2.0, col));
            painter.text(
                tip + egui::vec2(9.0, -9.0),
                egui::Align2::LEFT_BOTTOM,
                name,
                egui::FontId::proportional(13.0),
                col,
            );
        }
        // Tool previews.
        if self.tool == Tool::Draw && !self.drawing.is_empty() {
            let mut pts: Vec<Pos2> = self.drawing.iter().map(|&p| v.to_screen(p)).collect();
            if let Some((p, _)) = self.snap_mark {
                pts.push(v.to_screen(p));
            } else if let Some(h) = hover {
                pts.push(h);
            }
            painter.add(egui::Shape::line(pts, Stroke::new(2.0, TEAL)));
            for &p in &self.drawing {
                painter.circle_filled(v.to_screen(p), 4.0, TEAL);
            }
            if self.drawing.len() >= 3 {
                painter.circle_stroke(v.to_screen(self.drawing[0]), 9.0, Stroke::new(1.5, TEAL));
            }
        }
        if self.tool == Tool::Stamp
            && let Some(h) = hover
        {
            let poly = self.stamp_preview(v, h);
            fill(
                &poly,
                Color32::from_rgba_unmultiplied(0x30, 0xe0, 0xc8, 60),
                [0.0, 0.0],
            );
            outline(&poly, [0.0, 0.0], Stroke::new(2.0, TEAL));
        }
        if let Some((p, kind)) = self.snap_mark {
            let c = v.to_screen(p);
            painter.circle_stroke(c, 8.0, Stroke::new(1.5, YELLOW));
            let label = match kind {
                SnapKind::Vertex => "corner",
                SnapKind::Edge => "edge",
                SnapKind::Lattice => "lattice point",
                SnapKind::Angle => "angle",
                SnapKind::Grid => "grid",
            };
            painter.text(
                c + egui::vec2(10.0, 10.0),
                egui::Align2::LEFT_TOP,
                label,
                egui::FontId::proportional(11.0),
                YELLOW,
            );
        }
    }
}

fn nearest_on_seg(p: P, a: P, b: P) -> P {
    let ab = sub(b, a);
    let l2 = dot(ab, ab);
    if l2 < 1e-300 {
        return a;
    }
    let t = (dot(sub(p, a), ab) / l2).clamp(0.0, 1.0);
    add(a, scale(ab, t))
}

/// Where the ray `o + t·dir` (t ≥ 0) crosses segment `ab`.
fn ray_hits_seg(o: P, dir: P, a: P, b: P) -> Option<P> {
    let e = sub(b, a);
    let den = cross(dir, e);
    if den.abs() < 1e-12 * len(e).max(1e-300) {
        return None;
    }
    let w = sub(a, o);
    let t = cross(w, e) / den;
    let u = cross(w, dir) / den;
    (t >= 0.0 && (0.0..=1.0).contains(&u)).then(|| add(o, scale(dir, t)))
}

/// A cell from pasted JSON: a tiling spec of any lattice family, or bare periodic parameters.
fn parse_cell(text: &str) -> Result<UnitCell, String> {
    let v: serde_json::Value = serde_json::from_str(text).map_err(|e| e.to_string())?;
    if v.get("family").is_some() {
        let spec: TilingSpec = serde_json::from_value(v).map_err(|e| e.to_string())?;
        return UnitCell::from_spec(&spec);
    }
    UnitCell::from_params(&v)
}

fn sources() -> Vec<Source> {
    let mut out = Vec::new();
    let blank = |label: &str, a1: P, a2: P| Source {
        label: label.into(),
        group: "Empty cells",
        spec: None,
        blank: Some(UnitCell {
            a1,
            a2,
            polys: vec![],
        }),
    };
    out.push(blank("Empty square lattice", [1.0, 0.0], [0.0, 1.0]));
    out.push(blank(
        "Empty hexagonal lattice",
        [1.0, 0.0],
        [0.5, geom::S3 / 2.0],
    ));
    for f in FAMILIES {
        if f.kind == FamilyKind::Static && f.id != "periodic" {
            out.push(Source {
                label: f.name.into(),
                group: "Fixed tilings",
                spec: Some(TilingSpec::new(f.id)),
                blank: None,
            });
        }
    }
    for t in catalog::all() {
        if families::family(&t.id).is_none() {
            out.push(Source {
                label: t.name.clone(),
                group: if t.group.is_empty() {
                    "Catalog"
                } else {
                    t.group.as_str()
                },
                spec: Some(TilingSpec {
                    params: Some(serde_json::json!({"catalog": t.id})),
                    ..TilingSpec::new("periodic")
                }),
                blank: None,
            });
        }
    }
    for f in FAMILIES {
        if f.kind == FamilyKind::Reshapeable {
            out.push(Source {
                label: format!("{} (straight edges)", f.name),
                group: "Reshapeable tiles",
                spec: Some(TilingSpec::new(f.id)),
                blank: None,
            });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    //! Drive the editor headlessly with egui input events.
    use super::*;
    use egui::{Event, Key, Modifiers, PointerButton, RawInput};

    struct Harness {
        ctx: egui::Context,
        ed: CellEditor,
        t: f64,
        actions: Vec<EditorAction>,
    }

    impl Harness {
        fn new(cell: UnitCell) -> Harness {
            let mut ed = CellEditor::new();
            ed.set_fresh(cell);
            ed.open = true;
            let mut h = Harness {
                ctx: egui::Context::default(),
                ed,
                t: 0.0,
                actions: vec![],
            };
            h.frame(vec![]);
            h.frame(vec![]);
            h
        }
        fn frame(&mut self, events: Vec<Event>) {
            self.frame_mod(events, Modifiers::NONE);
        }
        fn frame_mod(&mut self, mut events: Vec<Event>, modifiers: Modifiers) {
            self.t += 1.0 / 60.0;
            events.insert(0, Event::ModifiersChanged(modifiers));
            let input = RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(1400.0, 900.0))),
                time: Some(self.t),
                events,
                ..Default::default()
            };
            let ed = &mut self.ed;
            let acts = &mut self.actions;
            let mut out = self.ctx.run_pass(input, |ctx| acts.extend(ed.show(ctx)));
            // No renderer here: drop the font atlas upload (epaint asserts on unapplied deltas).
            out.textures_delta.clear();
        }
        fn screen(&self, p: P) -> Pos2 {
            let (s, off) = self.ed.view.unwrap();
            View {
                s,
                off,
                min: self.ed.canvas.min,
            }
            .to_screen(p)
        }
        fn button(&mut self, at: Pos2, pressed: bool, button: PointerButton) {
            self.frame(vec![Event::PointerButton {
                pos: at,
                button,
                pressed,
                modifiers: Modifiers::NONE,
            }]);
        }
        fn click(&mut self, p: P) {
            let a = self.screen(p);
            self.frame(vec![Event::PointerMoved(a)]);
            self.button(a, true, PointerButton::Primary);
            self.button(a, false, PointerButton::Primary);
            // Let the double-click window pass.
            self.t += 1.0;
            self.frame(vec![]);
        }
        fn double_click(&mut self, p: P) {
            let a = self.screen(p);
            self.frame(vec![Event::PointerMoved(a)]);
            for _ in 0..2 {
                self.button(a, true, PointerButton::Primary);
                self.button(a, false, PointerButton::Primary);
            }
            self.t += 1.0;
            self.frame(vec![]);
        }
        fn drag(&mut self, from: P, to: P) {
            let (a, b) = (self.screen(from), self.screen(to));
            self.frame(vec![Event::PointerMoved(a)]);
            self.button(a, true, PointerButton::Primary);
            for i in 1..=6 {
                self.frame(vec![Event::PointerMoved(a + (b - a) * (i as f32 / 6.0))]);
            }
            self.button(b, false, PointerButton::Primary);
            self.t += 1.0;
            self.frame(vec![]);
        }
        fn key(&mut self, key: Key, modifiers: Modifiers) {
            self.frame_mod(
                vec![Event::Key {
                    key,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers,
                }],
                modifiers,
            );
            self.frame(vec![]);
        }
        fn degrees(&self) -> Vec<(usize, usize)> {
            self.ed
                .report
                .tiles
                .iter()
                .map(|t| (t.edge.len(), t.touch.len()))
                .collect()
        }
    }

    fn unit_square() -> UnitCell {
        UnitCell {
            a1: [1.0, 0.0],
            a2: [0.0, 1.0],
            polys: vec![vec![[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]]],
        }
    }

    #[test]
    fn stamp_square_then_triangles_to_build_elongated_triangular() {
        let mut h = Harness::new(unit_square());
        h.ed.tool = Tool::Stamp;
        h.ed.ngon = 3;
        // Triangle on the square's y = 1 edge, outside the square.
        h.click([0.5, 1.0]);
        assert_eq!(h.ed.cell.polys.len(), 2);
        let tri = h.ed.cell.polys[1].clone();
        let apex = [0.5, 1.0 + geom::S3 / 2.0];
        assert!(tri.iter().any(|&p| dist(p, apex) < 1e-9), "{tri:?}");
        // Drag a2 onto the apex: snaps to the corner exactly.
        h.ed.tool = Tool::Select;
        h.drag([0.0, 1.0], [0.52, 1.0 + geom::S3 / 2.0 + 0.01]);
        assert!(dist(h.ed.cell.a2, apex) < 1e-12, "{:?}", h.ed.cell.a2);
        // Second triangle on the first one's right edge fills the gap.
        h.ed.tool = Tool::Stamp;
        h.click(scale(add([1.0, 1.0], apex), 0.5));
        assert_eq!(h.ed.cell.polys.len(), 3);
        assert!(
            h.ed.report.problems.is_empty(),
            "{:?}",
            h.ed.report
                .problems
                .iter()
                .map(|p| p.message())
                .collect::<Vec<_>>()
        );
        // Squares: 4 edge (2 squares, 2 triangles), triangles 3 edge.
        assert_eq!(
            h.degrees().iter().map(|d| d.0).collect::<Vec<_>>(),
            vec![4, 3, 3]
        );
        // Undo twice: back to the square with the stamped triangle on the original lattice.
        h.frame(vec![Event::PointerMoved(h.screen([0.5, 0.5]))]);
        h.key(Key::Z, Modifiers::COMMAND);
        h.key(Key::Z, Modifiers::COMMAND);
        assert_eq!(h.ed.cell.polys.len(), 2);
        assert_eq!(h.ed.cell.a2, [0.0, 1.0]);
        h.key(Key::Y, Modifiers::COMMAND);
        assert!(dist(h.ed.cell.a2, apex) < 1e-12);
    }

    #[test]
    fn lattice_drag_snaps_to_edge_and_offsets_bricks() {
        let brick = UnitCell {
            a1: [3.0, 0.0],
            a2: [0.0, 1.0],
            polys: vec![vec![[0.0, 0.0], [3.0, 0.0], [3.0, 1.0], [0.0, 1.0]]],
        };
        let mut h = Harness::new(brick);
        assert_eq!(h.degrees(), vec![(4, 8)]);
        h.ed.snap_vertices = false;
        h.ed.grid = Grid::Square;
        h.drag([0.0, 1.0], [1.04, 1.003]);
        assert_eq!(h.ed.cell.a2, [1.0, 1.0]);
        assert!(h.ed.report.ok());
        assert_eq!(h.degrees(), vec![(6, 6)]);
        assert!(
            !h.actions
                .iter()
                .any(|a| matches!(a, EditorAction::Apply(_)))
        );
    }

    #[test]
    fn welded_and_free_corner_drags() {
        // Welded: every corner of the square is one lattice orbit, so the square moves rigidly.
        let mut h = Harness::new(unit_square());
        h.drag([1.0, 1.0], [1.2, 1.1]);
        let p = &h.ed.cell.polys[0];
        assert!(
            dist(p[2], [1.2, 1.1]) < 1e-5 && dist(sub(p[2], p[0]), [1.0, 1.0]) < 1e-12,
            "{p:?}"
        );
        assert!(h.ed.report.ok());
        // Free: only the grabbed corner moves, leaving an overlap and a gap.
        let mut h = Harness::new(unit_square());
        h.ed.weld = false;
        h.drag([1.0, 1.0], [1.2, 1.1]);
        let p = &h.ed.cell.polys[0];
        assert!(dist(p[2], [1.2, 1.1]) < 1e-5 && p[0] == [0.0, 0.0], "{p:?}");
        assert!(
            h.ed.report
                .problems
                .iter()
                .any(|q| matches!(q, Problem::Overlap { .. }))
        );
    }

    #[test]
    fn draw_tile_insert_and_delete_corner() {
        let mut h = Harness::new(UnitCell {
            a1: [1.0, 0.0],
            a2: [0.0, 1.0],
            polys: vec![],
        });
        h.ed.tool = Tool::Draw;
        for p in [[0.01, -0.01], [1.0, 0.01], [0.99, 1.0], [0.0, 1.01]] {
            h.click(p);
        }
        h.click([0.0, 0.0]); // back on the first corner closes the tile
        assert!(h.ed.drawing.is_empty());
        assert_eq!(h.ed.cell.polys, unit_square().polys);
        assert!(h.ed.report.problems.is_empty());
        assert_eq!(h.degrees(), vec![(4, 8)]);
        // Insert a corner mid-edge: still a valid tiling, same contact.
        h.ed.tool = Tool::Select;
        h.double_click([0.5, 0.0]);
        assert_eq!(h.ed.cell.polys[0].len(), 5);
        assert!(h.ed.report.ok());
        assert_eq!(h.ed.sel_vertex, Some((0, 1)));
        // Delete it again.
        h.key(Key::Delete, Modifiers::NONE);
        assert_eq!(h.ed.cell.polys, unit_square().polys);
    }
}
