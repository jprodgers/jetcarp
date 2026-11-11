//! One tiling model: a neighbour graph with cell polygons and a cyclic neighbour order.
//!
//! Three storage modes behind one interface:
//! - **Dense**: single-tile-type lattices (square, hex, brick, parallelogram). Neighbours come from a
//!   fixed stencil, computed on the fly.
//! - **Periodic**: lattices with several tile types per unit cell. Each tile type has an offset table
//!   `(dtype, di, dj)`; no per-cell adjacency is stored.
//! - **Explicit**: aperiodic patches (Spectre) store a CSR adjacency.
//!
//! Dense is the one-type case of the periodic offset table, so both share `LatticeGrid`.
//! Neighbour slots are in clockwise order on a y-down screen, so turmite turns are consistent.

use crate::aperiodic;
use crate::edges::{self, EdgeShape, GlueKind};
use crate::families::{self, Family, FamilyKind};
use crate::geom::*;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::OnceLock;

/// Largest lattice grid. A binary field costs about 6 bytes per cell (state, age, back buffers).
pub const MAX_CELLS: u64 = 256_000_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Nb {
    /// Cells sharing an edge or a corner (Moore on squares).
    #[default]
    Touch,
    /// Cells sharing a stretch of edge (von Neumann on squares).
    Edge,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Boundary {
    /// Cells past the edge are missing (count as dead; grains fall off).
    #[default]
    Dead,
    /// The grid is a torus.
    Wrap,
    /// Cells past the edge count as live (state 1) for counting rules.
    Live,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SizeSpec {
    /// About this many cells, in a roughly square region.
    Cells(u32),
    /// Exactly `w` by `h` lattice unit cells.
    Rect { w: u32, h: u32 },
    /// Substitution level (aperiodic tilings).
    Level(u32),
}

/// Everything needed to rebuild a tiling. Accepts the prototype's tile JSON
/// (`{family, symmetry, params, edges}`) directly.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TilingSpec {
    pub family: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub params: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub edges: Option<Vec<EdgeShape>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<SizeSpec>,
    #[serde(default)]
    pub boundary: Boundary,
}

impl TilingSpec {
    pub fn new(family: &str) -> TilingSpec {
        TilingSpec {
            family: family.into(),
            params: None,
            edges: None,
            size: None,
            boundary: Boundary::Dead,
        }
    }
    pub fn with_size(mut self, size: SizeSpec) -> Self {
        self.size = Some(size);
        self
    }
    pub fn with_boundary(mut self, b: Boundary) -> Self {
        self.boundary = b;
        self
    }
    pub fn rect(family: &str, w: u32, h: u32, b: Boundary) -> Self {
        TilingSpec::new(family)
            .with_size(SizeSpec::Rect { w, h })
            .with_boundary(b)
    }
}

#[derive(Clone, Debug, Default)]
pub struct Csr {
    pub off: Vec<u32>,
    pub nb: Vec<u32>,
}
impl Csr {
    pub fn from_lists(lists: &[Vec<u32>]) -> Csr {
        let mut off = Vec::with_capacity(lists.len() + 1);
        off.push(0);
        let mut nb = Vec::new();
        for l in lists {
            nb.extend_from_slice(l);
            off.push(nb.len() as u32);
        }
        Csr { off, nb }
    }
    #[inline]
    pub fn row(&self, i: usize) -> &[u32] {
        &self.nb[self.off[i] as usize..self.off[i + 1] as usize]
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Offset {
    pub di: i32,
    pub dj: i32,
    pub k: u32,
}

/// A lattice region of `w × h` unit cells with `ntypes` tiles each.
/// Rows are sheared so the region fills a screen-aligned rectangle; wrap uses the matching torus.
#[derive(Clone, Debug)]
pub struct LatticeGrid {
    pub w: u32,
    pub h: u32,
    pub ntypes: u32,
    pub wrap: bool,
    pub a1: P,
    pub a2: P,
    /// Per tile type: transform from shape coordinates and the shape index.
    pub types: Vec<(Affine, usize)>,
    /// Lattice `i` of the first cell in each row.
    pub row_start: Vec<i32>,
    /// Shift in `i` when wrapping from row `h` back to row 0.
    pub wrap_shift: i32,
    /// `offsets[nb][type]` = neighbour slots in cyclic order.
    pub offsets: [Vec<Vec<Offset>>; 2],
}

impl LatticeGrid {
    #[inline]
    pub fn decompose(&self, idx: usize) -> (i32, i32, u32) {
        let nt = self.ntypes as usize;
        let k = (idx % nt) as u32;
        let c = idx / nt;
        (
            (c % self.w as usize) as i32,
            (c / self.w as usize) as i32,
            k,
        )
    }
    #[inline]
    pub fn index(&self, u: i32, j: i32, k: u32) -> usize {
        ((j as usize * self.w as usize) + u as usize) * self.ntypes as usize + k as usize
    }
    /// Resolve lattice coordinates `(i, j)` (unsheared) to a cell, honouring wrap.
    #[inline]
    pub fn resolve(&self, i: i32, j: i32, k: u32) -> Option<usize> {
        let (w, h) = (self.w as i32, self.h as i32);
        let (mut i, mut j) = (i, j);
        if self.wrap {
            let q = j.div_euclid(h);
            j -= q * h;
            i -= q * self.wrap_shift;
            let u = (i - self.row_start[j as usize]).rem_euclid(w);
            Some(self.index(u, j, k))
        } else {
            if j < 0 || j >= h {
                return None;
            }
            let u = i - self.row_start[j as usize];
            if u < 0 || u >= w {
                return None;
            }
            Some(self.index(u, j, k))
        }
    }
    #[inline]
    pub fn neighbor(&self, idx: usize, nb: Nb, slot: usize) -> Option<usize> {
        let (u, j, k) = self.decompose(idx);
        let o = self.offsets[nb as usize][k as usize][slot];
        self.resolve(u + self.row_start[j as usize] + o.di, j + o.dj, o.k)
    }
    pub fn transform(&self, idx: usize) -> Affine {
        let (u, j, k) = self.decompose(idx);
        let i = (u + self.row_start[j as usize]) as f64;
        let j = j as f64;
        let t = [
            i * self.a1[0] + j * self.a2[0],
            i * self.a1[1] + j * self.a2[1],
        ];
        Affine::trans(t[0], t[1]).mul(&self.types[k as usize].0)
    }
}

/// Where each tile of an explicit patch sits.
#[derive(Clone, Debug)]
pub enum Placements {
    /// Float transforms (the hat and Penrose size classes up to level 6).
    Float(Vec<Affine>),
    /// Exact rigid motions in Z[ζ_N] (N = 10 or 12), 20 bytes per tile. A tile's float
    /// transform is `post ∘ place ∘ pre`: `pre` takes the float prototile to exact coordinates
    /// and `post` takes exact coordinates to the drawing (fixed turns or translations of the
    /// whole patch; identity for the Spectre).
    Exact {
        ring: u8,
        pre: Affine,
        post: Affine,
        places: Vec<crate::exact::Place>,
    },
}

impl Placements {
    pub fn len(&self) -> usize {
        match self {
            Placements::Float(v) => v.len(),
            Placements::Exact { places, .. } => places.len(),
        }
    }
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
    #[inline]
    pub fn get(&self, i: usize) -> Affine {
        match self {
            Placements::Float(v) => v[i],
            Placements::Exact {
                ring,
                pre,
                post,
                places,
            } => {
                let iso = places[i].iso();
                let mut m = if *ring == 10 {
                    iso.to_affine::<crate::exact::Z10>()
                } else {
                    iso.to_affine::<crate::exact::Z12>()
                };
                if *pre != Affine::ID {
                    m = m.mul(pre);
                }
                if *post != Affine::ID {
                    m = post.mul(&m);
                }
                m
            }
        }
    }
    /// The exact placements, if any.
    pub fn exact(&self) -> Option<&[crate::exact::Place]> {
        match self {
            Placements::Exact { places, .. } => Some(places),
            _ => None,
        }
    }
}

#[derive(Clone, Debug)]
pub struct ExplicitGraph {
    pub transforms: Placements,
    /// Shape index per tile.
    pub shape: Vec<u16>,
    pub csr: [Csr; 2],
    /// Cells on the open boundary of the patch.
    pub open: Vec<bool>,
}

#[derive(Clone, Debug)]
pub enum Storage {
    Lattice(LatticeGrid),
    Explicit(ExplicitGraph),
}

/// A built tiling: geometry plus neighbour graph.
#[derive(Debug)]
pub struct Topology {
    pub spec: TilingSpec,
    pub family: &'static Family,
    /// Straight corner polygons per shape; contact is measured from these.
    pub shapes: Vec<Vec<P>>,
    /// Drawn outlines per shape (curved edges flattened).
    pub outlines: Vec<Vec<P>>,
    pub storage: Storage,
    pub bounds: [f64; 4],
    pub warnings: Vec<String>,
    n: usize,
    center: usize,
    locator: OnceLock<Locator>,
    /// Explicit patches: cells bucketed by centroid, for [`Topology::for_cells_in_box`].
    centroids: OnceLock<CentroidIndex>,
}

impl Topology {
    /// A topology on an explicit aperiodic patch built elsewhere (a sliding window's tiles,
    /// [`crate::window`]). `spec.family` must be an aperiodic family. `frame` gives the bounds
    /// and centre cell when the caller knows them (else they are computed from the tiles).
    pub fn from_patch(
        spec: &TilingSpec,
        patch: aperiodic::BuiltPatch,
        frame: Option<([f64; 4], usize)>,
    ) -> Result<Topology, String> {
        let fam = families::family(&spec.family)
            .ok_or_else(|| format!("unknown tiling family '{}'", spec.family))?;
        if fam.kind != FamilyKind::Aperiodic {
            return Err(format!("'{}' is not an aperiodic family", spec.family));
        }
        let edge_shapes = if fam.classes.is_empty() {
            None
        } else {
            Some(
                spec.edges
                    .clone()
                    .unwrap_or_else(|| families::fresh_edges(fam)),
            )
        };
        Ok(Topology::from_patch_parts(
            spec,
            fam,
            patch,
            edge_shapes.as_ref(),
            5,
            Vec::new(),
            frame,
        ))
    }

    fn from_patch_parts(
        spec: &TilingSpec,
        fam: &'static Family,
        patch: aperiodic::BuiltPatch,
        edge_shapes: Option<&Vec<EdgeShape>>,
        per_span: usize,
        mut warnings: Vec<String>,
        frame: Option<([f64; 4], usize)>,
    ) -> Topology {
        let outlines: Vec<Vec<P>> = match edge_shapes {
            Some(e) if !fam.classes.is_empty() => {
                let o = edges::tile_outline(fam.classes, e, &patch.shapes[0], per_span);
                if edges::self_intersects(&o) {
                    warnings.push("Edges cross: tiles would overlap.".into());
                }
                vec![o]
            }
            _ => patch.shapes.clone(),
        };
        let (bounds, center) = frame.unwrap_or_else(|| patch.bounds_and_center());
        let n = patch.len();
        Topology {
            spec: spec.clone(),
            family: fam,
            shapes: patch.shapes.clone(),
            outlines,
            storage: Storage::Explicit(ExplicitGraph {
                transforms: patch.placements,
                shape: patch.shape,
                csr: [patch.touch, patch.edge],
                open: patch.open,
            }),
            bounds,
            warnings,
            n,
            center,
            locator: OnceLock::new(),
            centroids: OnceLock::new(),
        }
    }

    /// The geometry pin of a tiling: a hash of its unit cell's polygons and offset table
    /// (periodic tilings), or of the patch its spec names (aperiodic ones), whatever grid size
    /// the spec gives. Records carry it so a library can flag them when the engine's geometry
    /// changes; see [`crate::pin`] for exactly what is hashed.
    pub fn geometry_pin(spec: &TilingSpec) -> Result<crate::pin::GeometryPin, String> {
        crate::pin::geometry_pin(spec)
    }

    pub fn build(spec: &TilingSpec) -> Result<Topology, String> {
        let fam = families::family(&spec.family)
            .ok_or_else(|| format!("unknown tiling family '{}'", spec.family))?;
        let mut warnings = Vec::new();
        let params = spec
            .params
            .clone()
            .unwrap_or_else(|| families::init_params(fam.id));
        let edge_shapes = match (&spec.edges, fam.kind) {
            (_, FamilyKind::Static) => None,
            (Some(e), _) => {
                if e.len() != fam.classes.len() {
                    return Err(format!(
                        "family '{}' has {} edge classes but the tile JSON gives {}",
                        fam.id,
                        fam.classes.len(),
                        e.len()
                    ));
                }
                Some(e.clone())
            }
            (None, _) => Some(families::fresh_edges(fam)),
        };
        let per_span = if fam.kind == FamilyKind::Aperiodic {
            5
        } else {
            10
        };
        match fam.kind {
            FamilyKind::Aperiodic => {
                let level = match spec.size {
                    Some(SizeSpec::Level(l)) => l,
                    _ => params
                        .get("level")
                        .and_then(|v| v.as_u64())
                        .map(|l| l as u32)
                        .unwrap_or_else(|| aperiodic::default_level(fam.id)),
                };
                if spec.boundary != Boundary::Dead {
                    warnings.push(
                        "Aperiodic patches have no wrap or live edges; using dead edges.".into(),
                    );
                }
                let patch = aperiodic::build(fam.id, level)?;
                Ok(Topology::from_patch_parts(
                    spec,
                    fam,
                    patch,
                    edge_shapes.as_ref(),
                    per_span,
                    warnings,
                    None,
                ))
            }
            _ => {
                let def = lattice_def(fam, &params)?;
                let outlines: Vec<Vec<P>> = match fam.kind {
                    FamilyKind::Static => def.shapes.clone(),
                    _ => {
                        let o = edges::tile_outline(
                            fam.classes,
                            edge_shapes.as_ref().unwrap(),
                            &def.shapes[0],
                            per_span,
                        );
                        if edges::self_intersects(&o) {
                            warnings.push("Edges cross: tiles would overlap.".into());
                        }
                        vec![o]
                    }
                };
                let (w, h) = match spec.size {
                    Some(SizeSpec::Rect { w, h }) => (w, h),
                    Some(SizeSpec::Level(_)) => {
                        return Err("Level sizes apply only to aperiodic tilings".into());
                    }
                    Some(SizeSpec::Cells(c)) => def.dims_for(c),
                    None => def.dims_for(900),
                };
                if w < 8 || h < 8 {
                    return Err(format!(
                        "grid must be at least 8 by 8 unit cells, got {w} by {h}"
                    ));
                }
                if (w as u64) * (h as u64) * def.types.len() as u64 > MAX_CELLS {
                    return Err(format!(
                        "grid is larger than {} million cells",
                        MAX_CELLS / 1_000_000
                    ));
                }
                let grid = def.grid(w, h, spec.boundary == Boundary::Wrap);
                let n = (w * h) as usize * def.types.len();
                // Bounds from the four corner unit cells.
                let mut bounds = [
                    f64::INFINITY,
                    f64::INFINITY,
                    f64::NEG_INFINITY,
                    f64::NEG_INFINITY,
                ];
                for j in [0, h as i32 - 1] {
                    for u in 0..w as i32 {
                        for k in 0..grid.ntypes {
                            add_bounds(&mut bounds, &grid, &def.shapes, grid.index(u, j, k));
                        }
                    }
                }
                for j in 0..h as i32 {
                    for u in [0, w as i32 - 1] {
                        for k in 0..grid.ntypes {
                            add_bounds(&mut bounds, &grid, &def.shapes, grid.index(u, j, k));
                        }
                    }
                }
                let center = grid.index(w as i32 / 2, h as i32 / 2, 0);
                Ok(Topology {
                    spec: spec.clone(),
                    family: fam,
                    shapes: def.shapes,
                    outlines,
                    storage: Storage::Lattice(grid),
                    bounds,
                    warnings,
                    n,
                    center,
                    locator: OnceLock::new(),
                    centroids: OnceLock::new(),
                })
            }
        }
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.n
    }
    pub fn is_empty(&self) -> bool {
        self.n == 0
    }
    pub fn storage_name(&self) -> &'static str {
        match &self.storage {
            Storage::Lattice(g) if g.ntypes == 1 => "dense",
            Storage::Lattice(_) => "periodic",
            Storage::Explicit(_) => "explicit",
        }
    }
    /// Cells past a dead lattice edge count as live (state 1).
    pub fn live_edges(&self) -> bool {
        self.spec.boundary == Boundary::Live && matches!(self.storage, Storage::Lattice(_))
    }
    /// Neighbour slots that fall past the edge.
    #[inline]
    pub fn missing(&self, i: usize, nb: Nb) -> u32 {
        match &self.storage {
            Storage::Lattice(g) if !g.wrap => {
                let mut present = 0;
                self.for_each_neighbor(i, nb, |_| present += 1);
                self.degree(i, nb) as u32 - present
            }
            _ => 0,
        }
    }
    pub fn wraps(&self) -> bool {
        matches!(&self.storage, Storage::Lattice(g) if g.wrap)
    }
    pub fn lattice(&self) -> Option<&LatticeGrid> {
        match &self.storage {
            Storage::Lattice(g) => Some(g),
            _ => None,
        }
    }
    /// Row structure for elementary (Wolfram) rules: only the square grid in Phase 0.
    pub fn rows(&self) -> Option<&LatticeGrid> {
        match &self.storage {
            Storage::Lattice(g) if self.family.id == "square" && g.ntypes == 1 => Some(g),
            _ => None,
        }
    }
    /// Row structure for hexagonal elementary rules: the hexagon family's dense lattice.
    pub fn hex_rows(&self) -> Option<&LatticeGrid> {
        match &self.storage {
            Storage::Lattice(g) if self.family.id == "hex" && g.ntypes == 1 => Some(g),
            _ => None,
        }
    }
    /// Column structure for the three-parent hexagon rules: the flat-top hexagon grid, two
    /// hexagons per unit cell (type 0 and, half a row lower, type 1).
    pub fn flat_hex_rows(&self) -> Option<&LatticeGrid> {
        match &self.storage {
            Storage::Lattice(g) if self.family.id == "hex_flat" && g.ntypes == 2 => Some(g),
            _ => None,
        }
    }
    pub fn center_cell(&self) -> usize {
        self.center
    }
    pub fn transform(&self, i: usize) -> Affine {
        match &self.storage {
            Storage::Lattice(g) => g.transform(i),
            Storage::Explicit(e) => e.transforms.get(i),
        }
    }
    pub fn shape_of(&self, i: usize) -> usize {
        match &self.storage {
            Storage::Lattice(g) => g.types[i % g.ntypes as usize].1,
            Storage::Explicit(e) => e.shape[i] as usize,
        }
    }
    pub fn polygon(&self, i: usize) -> Vec<P> {
        let m = self.transform(i);
        self.shapes[self.shape_of(i)]
            .iter()
            .map(|&p| m.apply(p))
            .collect()
    }
    pub fn centroid(&self, i: usize) -> P {
        centroid(&self.polygon(i))
    }
    /// Number of neighbour slots, including slots that fall off a dead edge.
    #[inline]
    pub fn degree(&self, i: usize, nb: Nb) -> usize {
        match &self.storage {
            Storage::Lattice(g) => g.offsets[nb as usize][i % g.ntypes as usize].len(),
            Storage::Explicit(e) => {
                let c = &e.csr[nb as usize];
                (c.off[i + 1] - c.off[i]) as usize
            }
        }
    }
    /// The neighbour in a slot, or `None` past a dead edge.
    #[inline]
    pub fn slot(&self, i: usize, nb: Nb, k: usize) -> Option<usize> {
        match &self.storage {
            Storage::Lattice(g) => g.neighbor(i, nb, k),
            Storage::Explicit(e) => Some(e.csr[nb as usize].row(i)[k] as usize),
        }
    }
    #[inline]
    pub fn for_each_neighbor(&self, i: usize, nb: Nb, mut f: impl FnMut(usize)) {
        match &self.storage {
            Storage::Lattice(g) => {
                let (u, j, k) = g.decompose(i);
                let base = u + g.row_start[j as usize];
                for o in &g.offsets[nb as usize][k as usize] {
                    if let Some(x) = g.resolve(base + o.di, j + o.dj, o.k) {
                        f(x);
                    }
                }
            }
            Storage::Explicit(e) => {
                for &x in e.csr[nb as usize].row(i) {
                    f(x as usize);
                }
            }
        }
    }
    /// Cells on an aperiodic patch's open boundary. Their neighbour lists are incomplete.
    #[inline]
    pub fn is_open(&self, i: usize) -> bool {
        match &self.storage {
            Storage::Explicit(e) => e.open[i],
            _ => false,
        }
    }
    /// Is any cell missing neighbours (dead lattice edge or open patch boundary)?
    pub fn has_boundary(&self) -> bool {
        !self.wraps()
    }
    /// Neighbour counts present in the tiling, for reports: (edge degrees, touch degrees), interior cells only.
    pub fn degree_sets(&self) -> (Vec<usize>, Vec<usize>) {
        // Lattices: every interior cell of a type has that type's full offset list.
        if let Storage::Lattice(g) = &self.storage {
            let set = |nb: Nb| -> Vec<usize> {
                let s: std::collections::BTreeSet<usize> =
                    g.offsets[nb as usize].iter().map(|o| o.len()).collect();
                s.into_iter().collect()
            };
            return (set(Nb::Edge), set(Nb::Touch));
        }
        let mut e = std::collections::BTreeSet::new();
        let mut t = std::collections::BTreeSet::new();
        for i in 0..self.n {
            if self.is_interior(i) {
                e.insert(self.degree(i, Nb::Edge));
                t.insert(self.degree(i, Nb::Touch));
            }
        }
        (e.into_iter().collect(), t.into_iter().collect())
    }
    /// Not on a boundary, and no touching neighbour on one either.
    pub fn is_interior(&self, i: usize) -> bool {
        match &self.storage {
            Storage::Explicit(e) => {
                !e.open[i] && !e.csr[0].row(i).iter().any(|&j| e.open[j as usize])
            }
            Storage::Lattice(g) => {
                g.wrap
                    || (0..self.degree(i, Nb::Touch)).all(|k| g.neighbor(i, Nb::Touch, k).is_some())
            }
        }
    }
    /// Cell containing a world point, if any. Lattices: constant time, from lattice coordinates.
    pub fn locate(&self, p: P) -> Option<usize> {
        if let Storage::Lattice(g) = &self.storage {
            let (a1, a2) = (g.a1, g.a2);
            let fj = p[1] / a2[1];
            let fi = (p[0] - fj * a2[0]) / a1[0];
            let (i0, j0) = (fi.floor() as i32, fj.floor() as i32);
            for dj in -2..=2 {
                let j = j0 + dj;
                if j < 0 || j >= g.h as i32 {
                    continue;
                }
                for di in -2..=2 {
                    let u = i0 + di - g.row_start[j as usize];
                    if u < 0 || u >= g.w as i32 {
                        continue;
                    }
                    for k in 0..g.ntypes {
                        let c = g.index(u, j, k);
                        if point_in_poly(p, &self.polygon(c)) {
                            return Some(c);
                        }
                    }
                }
            }
            return None;
        }
        self.locator
            .get_or_init(|| Locator::new(self))
            .find(self, p)
    }
    /// Call `f(cell, centroid)` for every cell whose centroid lies in the world box
    /// `[x0, y0, x1, y1]`, in a fixed order. Lattices visit only the lattice points near the
    /// box; with wrap edges the box may run past the grid's edges, and each cell is reported
    /// at the position of its torus image inside the box (a box wider than the torus reports a
    /// cell once per image). Explicit patches use a centroid bucket index built on first use.
    pub fn for_cells_in_box(&self, bx: [f64; 4], mut f: impl FnMut(usize, P)) {
        match &self.storage {
            Storage::Lattice(g) => {
                // Lattice point (i, j) of type k has its centroid at i a1 + j a2 + c_k.
                let ck: Vec<P> = g
                    .types
                    .iter()
                    .map(|(m, sh)| m.apply(centroid(&self.shapes[*sh])))
                    .collect();
                let (mut cx0, mut cx1, mut cy0, mut cy1) = (f64::MAX, f64::MIN, f64::MAX, f64::MIN);
                for c in &ck {
                    cx0 = cx0.min(c[0]);
                    cx1 = cx1.max(c[0]);
                    cy0 = cy0.min(c[1]);
                    cy1 = cy1.max(c[1]);
                }
                let (a1, a2) = (g.a1, g.a2);
                if a2[1].abs() < 1e-12 || a1[0].abs() < 1e-12 {
                    return;
                }
                let jr = |y: f64, c: f64| (y - c) / a2[1];
                let (ja, jb) = {
                    let (p, q) = (jr(bx[1], cy1), jr(bx[3], cy0));
                    (p.min(q).floor() as i64 - 1, p.max(q).ceil() as i64 + 1)
                };
                let (ja, jb) = if g.wrap {
                    (ja, jb)
                } else {
                    (ja.max(0), jb.min(g.h as i64 - 1))
                };
                for j in ja..=jb {
                    let base = j as f64 * a2[0];
                    let ir = |x: f64, c: f64| (x - base - c) / a1[0];
                    let (p, q) = (ir(bx[0], cx1), ir(bx[2], cx0));
                    let (ia, ib) = (p.min(q).floor() as i64 - 1, p.max(q).ceil() as i64 + 1);
                    let (ia, ib) = if g.wrap || j < 0 || j >= g.h as i64 {
                        (ia, ib)
                    } else {
                        let rs = g.row_start[j as usize] as i64;
                        (ia.max(rs), ib.min(rs + g.w as i64 - 1))
                    };
                    for i in ia..=ib {
                        let t = [i as f64 * a1[0] + base, i as f64 * a1[1] + j as f64 * a2[1]];
                        for (k, c) in ck.iter().enumerate() {
                            let p = [t[0] + c[0], t[1] + c[1]];
                            if p[0] < bx[0] || p[0] > bx[2] || p[1] < bx[1] || p[1] > bx[3] {
                                continue;
                            }
                            if let Some(cell) = g.resolve(i as i32, j as i32, k as u32) {
                                f(cell, p);
                            }
                        }
                    }
                }
            }
            Storage::Explicit(_) => {
                let ix = self.centroids.get_or_init(|| CentroidIndex::new(self));
                let col = |x: f64| ((x - ix.origin[0]) / ix.cell).floor();
                let row = |y: f64| ((y - ix.origin[1]) / ix.cell).floor();
                let c0 = col(bx[0]).max(0.0) as usize;
                let r0 = row(bx[1]).max(0.0) as usize;
                let (c1, r1) = (col(bx[2]), row(bx[3]));
                if c1 < 0.0 || r1 < 0.0 {
                    return;
                }
                let c1 = (c1 as usize).min(ix.cols - 1);
                let r1 = (r1 as usize).min(ix.rows - 1);
                for r in r0..=r1 {
                    for c in c0..=c1 {
                        let k = r * ix.cols + c;
                        for &id in &ix.ids[ix.off[k] as usize..ix.off[k + 1] as usize] {
                            let p = ix.pts[id as usize];
                            if p[0] >= bx[0] && p[0] <= bx[2] && p[1] >= bx[1] && p[1] <= bx[3] {
                                f(id as usize, p);
                            }
                        }
                    }
                }
            }
        }
    }

    pub fn describe(&self) -> String {
        let dims = match &self.storage {
            Storage::Lattice(g) => format!("{}x{} unit cells, {} tile type(s)", g.w, g.h, g.ntypes),
            Storage::Explicit(_) => match self.spec.size {
                Some(SizeSpec::Level(l)) => format!("level {l}"),
                _ => "patch".into(),
            },
        };
        format!(
            "{} ({}): {} cells, {}, storage {}, edges {}",
            self.family.name,
            self.family.id,
            self.n,
            dims,
            self.storage_name(),
            match self.spec.boundary {
                _ if self.wraps() => "wrap",
                Boundary::Live if self.live_edges() => "live",
                _ => "dead",
            }
        )
    }
}

fn add_bounds(b: &mut [f64; 4], g: &LatticeGrid, shapes: &[Vec<P>], idx: usize) {
    let m = g.transform(idx);
    for &p in &shapes[g.types[idx % g.ntypes as usize].1] {
        let q = m.apply(p);
        b[0] = b[0].min(q[0]);
        b[1] = b[1].min(q[1]);
        b[2] = b[2].max(q[0]);
        b[3] = b[3].max(q[1]);
    }
}

/// Uniform-grid point locator over cell bounding boxes.
#[derive(Debug)]
struct Locator {
    origin: P,
    cell: f64,
    cols: usize,
    rows: usize,
    /// Tiles whose bounding box meets each bucket, as CSR (offsets per bucket, then ids).
    off: Vec<u32>,
    ids: Vec<u32>,
}
impl Locator {
    fn new(t: &Topology) -> Locator {
        use rayon::prelude::*;
        let b = t.bounds;
        let area = (b[2] - b[0]).max(1e-9) * (b[3] - b[1]).max(1e-9);
        let cell = (area / t.len().max(1) as f64).sqrt() * 1.5;
        let cols = (((b[2] - b[0]) / cell).ceil() as usize).max(1);
        let rows = (((b[3] - b[1]) / cell).ceil() as usize).max(1);
        // Bucket range of each tile: (c0, c1, r0, r1).
        let span: Vec<[u32; 4]> = (0..t.len())
            .into_par_iter()
            .map(|i| {
                let bb = bbox(&t.polygon(i));
                let c =
                    |x: f64| (((x - b[0]) / cell).floor().max(0.0) as usize).min(cols - 1) as u32;
                let r =
                    |y: f64| (((y - b[1]) / cell).floor().max(0.0) as usize).min(rows - 1) as u32;
                [c(bb[0]), c(bb[2]), r(bb[1]), r(bb[3])]
            })
            .collect();
        let mut off = vec![0u32; cols * rows + 1];
        for s in &span {
            for r in s[2]..=s[3] {
                for c in s[0]..=s[1] {
                    off[r as usize * cols + c as usize + 1] += 1;
                }
            }
        }
        for k in 1..off.len() {
            off[k] += off[k - 1];
        }
        let mut next = off.clone();
        let mut ids = vec![0u32; *off.last().unwrap() as usize];
        for (i, s) in span.iter().enumerate() {
            for r in s[2]..=s[3] {
                for c in s[0]..=s[1] {
                    let k = &mut next[r as usize * cols + c as usize];
                    ids[*k as usize] = i as u32;
                    *k += 1;
                }
            }
        }
        Locator {
            origin: [b[0], b[1]],
            cell,
            cols,
            rows,
            off,
            ids,
        }
    }
    fn find(&self, t: &Topology, p: P) -> Option<usize> {
        let c = ((p[0] - self.origin[0]) / self.cell).floor();
        let r = ((p[1] - self.origin[1]) / self.cell).floor();
        if c < 0.0 || r < 0.0 || c as usize >= self.cols || r as usize >= self.rows {
            return None;
        }
        let k = r as usize * self.cols + c as usize;
        self.ids[self.off[k] as usize..self.off[k + 1] as usize]
            .iter()
            .map(|&i| i as usize)
            .find(|&i| point_in_poly(p, &t.polygon(i)))
    }
}

/// Explicit patches' cells bucketed by centroid: every cell in exactly one bucket.
#[derive(Debug)]
struct CentroidIndex {
    origin: P,
    cell: f64,
    cols: usize,
    rows: usize,
    off: Vec<u32>,
    ids: Vec<u32>,
    pts: Vec<P>,
}
impl CentroidIndex {
    fn new(t: &Topology) -> CentroidIndex {
        use rayon::prelude::*;
        let b = t.bounds;
        let area = (b[2] - b[0]).max(1e-9) * (b[3] - b[1]).max(1e-9);
        let cell = (area / t.len().max(1) as f64).sqrt() * 2.0;
        let cols = (((b[2] - b[0]) / cell).ceil() as usize).max(1);
        let rows = (((b[3] - b[1]) / cell).ceil() as usize).max(1);
        let pts: Vec<P> = (0..t.len())
            .into_par_iter()
            .map(|i| t.centroid(i))
            .collect();
        let bucket = |p: P| {
            let c = (((p[0] - b[0]) / cell).floor().max(0.0) as usize).min(cols - 1);
            let r = (((p[1] - b[1]) / cell).floor().max(0.0) as usize).min(rows - 1);
            r * cols + c
        };
        let keys: Vec<u32> = pts.par_iter().map(|&p| bucket(p) as u32).collect();
        let mut off = vec![0u32; cols * rows + 1];
        for &k in &keys {
            off[k as usize + 1] += 1;
        }
        for k in 1..off.len() {
            off[k] += off[k - 1];
        }
        let mut next = off.clone();
        let mut ids = vec![0u32; t.len()];
        for (i, &k) in keys.iter().enumerate() {
            let at = &mut next[k as usize];
            ids[*at as usize] = i as u32;
            *at += 1;
        }
        CentroidIndex {
            origin: [b[0], b[1]],
            cell,
            cols,
            rows,
            off,
            ids,
            pts,
        }
    }
}

/// Lattice description before choosing a region size.
struct LatticeDef {
    a1: P,
    a2: P,
    types: Vec<(Affine, usize)>,
    shapes: Vec<Vec<P>>,
    offsets: [Vec<Vec<Offset>>; 2],
}

impl LatticeDef {
    fn dims_for(&self, cells: u32) -> (u32, u32) {
        let units = (cells as f64 / self.types.len() as f64).max(64.0);
        let cell_area = cross(self.a1, self.a2).abs();
        let side = (units * cell_area).sqrt();
        let w = (side / self.a1[0]).round().max(8.0) as u32;
        let h = (side / self.a2[1]).round().max(8.0) as u32;
        (w, h)
    }
    fn grid(&self, w: u32, h: u32, wrap: bool) -> LatticeGrid {
        let s = self.a2[0] / self.a1[0];
        let row_start: Vec<i32> = (0..h)
            .map(|j| -((j as f64 * s + 1e-9).floor() as i32))
            .collect();
        let wrap_shift = -((h as f64 * s + 1e-9).floor() as i32);
        LatticeGrid {
            w,
            h,
            ntypes: self.types.len() as u32,
            wrap,
            a1: self.a1,
            a2: self.a2,
            types: self.types.clone(),
            row_start,
            wrap_shift,
            offsets: self.offsets.clone(),
        }
    }
}

/// Find the translation lattice and tile types, rotate so `a1` is horizontal, and measure the
/// neighbour offset table from contact between corner polygons.
fn lattice_def(fam: &Family, params: &Value) -> Result<LatticeDef, String> {
    // (types as transforms + shape index, all translation vectors found, shapes)
    type Types = Vec<(Affine, usize)>;
    let (raw_types, translations, shapes): (Types, Vec<P>, Vec<Vec<P>>) = match fam.kind {
        FamilyKind::Static => {
            let (a1, a2, polys) = match fam.id {
                "periodic" => crate::catalog::from_params(params)?,
                _ => families::static_cell(fam.id, params).ok_or("missing static cell")?,
            };
            let mut tr = Vec::new();
            for i in -3..=3 {
                for j in -3..=3 {
                    if i != 0 || j != 0 {
                        tr.push(add(scale(a1, i as f64), scale(a2, j as f64)));
                    }
                }
            }
            (
                (0..polys.len()).map(|k| (Affine::ID, k)).collect(),
                tr,
                polys,
            )
        }
        // Isohedral types: tactile gives the aspects and translations directly.
        _ if crate::isohedral::index_of(fam.id).is_some() => {
            let (asp, t1, t2, corners) = crate::isohedral::cell(fam.id, params).unwrap()?;
            if corners
                .iter()
                .chain(&[t1, t2])
                .any(|p| !p[0].is_finite() || !p[1].is_finite())
            {
                return Err("tile corners are not finite".into());
            }
            let mut tr = Vec::new();
            for i in -3..=3 {
                for j in -3..=3 {
                    if i != 0 || j != 0 {
                        tr.push(add(scale(t1, i as f64), scale(t2, j as f64)));
                    }
                }
            }
            (asp.into_iter().map(|m| (m, 0)).collect(), tr, vec![corners])
        }
        _ => {
            let corners = families::corners(fam.id, params)?;
            if corners
                .iter()
                .any(|p| !p[0].is_finite() || !p[1].is_finite())
            {
                return Err("tile corners are not finite".into());
            }
            let t = edges::glue_maps(fam.classes, &corners);
            // BFS over the glue group; classify by linear part.
            let key = |m: &Affine| m.0.map(|v| (v * 1e5).round() as i64);
            let lin_key = |m: &Affine| m.linear().map(|v| (v * 1e5).round() as i64);
            let mut seen = HashMap::new();
            let mut queue = std::collections::VecDeque::new();
            seen.insert(key(&Affine::ID), ());
            queue.push_back(Affine::ID);
            let mut types: Vec<(Affine, usize)> = Vec::new();
            let mut type_keys: Vec<[i64; 4]> = Vec::new();
            let mut tr = Vec::new();
            let diam = corners
                .iter()
                .map(|&p| dist(p, corners[0]))
                .fold(0.0, f64::max)
                .max(1e-3);
            let mut count = 0;
            while let Some(m) = queue.pop_front() {
                count += 1;
                if count > 600 {
                    break;
                }
                let lk = lin_key(&m);
                if !type_keys.contains(&lk) {
                    type_keys.push(lk);
                    types.push((m, 0));
                }
                if lk == lin_key(&Affine::ID) && key(&m) != key(&Affine::ID) {
                    tr.push(m.translation());
                }
                for g in &t {
                    let nm = m.mul(g);
                    let k = key(&nm);
                    if seen.contains_key(&k) {
                        continue;
                    }
                    seen.insert(k, ());
                    if len(nm.translation()) < diam * 12.0 {
                        queue.push_back(nm);
                    }
                }
            }
            if types.len() > 12 {
                return Err("tile parameters do not give a periodic tiling".into());
            }
            (types, tr, vec![corners])
        }
    };
    if translations.len() < 2 {
        return Err("could not find the translation lattice".into());
    }
    // Shortest vector, preferring the most horizontal among ties. The flat-top hexagon grid keeps
    // its given rectangular basis instead, so its rows are horizontal and its columns vertical.
    let fixed_basis = (fam.id == "hex_flat")
        .then(|| families::static_cell(fam.id, params).map(|c| (c.0, c.1)))
        .flatten();
    let lmin = translations
        .iter()
        .map(|&v| len(v))
        .fold(f64::INFINITY, f64::min);
    let v1 = if let Some((b1, _)) = fixed_basis {
        b1
    } else {
        *translations
            .iter()
            .filter(|&&v| len(v) <= lmin * (1.0 + 1e-6))
            .min_by(|&&a, &&b| {
                let ka = (angle_of(a).abs() * 1e6).round() as i64;
                let kb = (angle_of(b).abs() * 1e6).round() as i64;
                ka.cmp(&kb)
                    .then(angle_of(a).partial_cmp(&angle_of(b)).unwrap())
            })
            .unwrap()
    };
    let v2 = if let Some((_, b2)) = fixed_basis {
        b2
    } else {
        *translations
            .iter()
            .filter(|&&v| cross(v1, v).abs() > 1e-6 * lmin * lmin)
            .min_by(|&&a, &&b| {
                // Quantised keys, so the choice never rests on the last bit of a float.
                let q = |x: f64| (x * 1e6).round() as i64;
                q(len(a))
                    .cmp(&q(len(b)))
                    .then(q(dot(a, v1).abs()).cmp(&q(dot(b, v1).abs())))
                    .then(q(angle_of(a)).cmp(&q(angle_of(b))))
            })
            .ok_or("lattice is degenerate")?
    };
    let v2 = if cross(v1, v2) < 0.0 {
        scale(v2, -1.0)
    } else {
        v2
    };
    let rot = Affine::rot(-angle_of(v1));
    let a1 = rot.apply_vec(v1);
    let a2 = rot.apply_vec(v2);
    let a1 = [a1[0], 0.0];
    // Rotate the type transforms; shift each representative into the base unit cell.
    let inv_basis = |p: P| -> (f64, f64) {
        let det = a1[0] * a2[1] - a2[0] * a1[1];
        (
            (p[0] * a2[1] - a2[0] * p[1]) / det,
            (a1[0] * p[1] - p[0] * a1[1]) / det,
        )
    };
    let types: Vec<(Affine, usize)> = raw_types
        .iter()
        .map(|(m, s)| {
            let r = rot.mul(m);
            let c = r.apply(centroid(&shapes[*s]));
            let (fi, fj) = inv_basis(c);
            let (di, dj) = (fi.floor(), fj.floor());
            let shift = add(scale(a1, -di), scale(a2, -dj));
            (Affine::trans(shift[0], shift[1]).mul(&r), *s)
        })
        .collect();
    // Offset table from measured contact.
    let world = |k: usize, di: i32, dj: i32| -> Vec<P> {
        let (m, s) = &types[k];
        let t = add(scale(a1, di as f64), scale(a2, dj as f64));
        let mm = Affine::trans(t[0], t[1]).mul(m);
        shapes[*s].iter().map(|&p| mm.apply(p)).collect()
    };
    let eps = 1e-4;
    let mut offsets: [Vec<Vec<Offset>>; 2] = [Vec::new(), Vec::new()];
    for k in 0..types.len() {
        let pk = world(k, 0, 0);
        let bk = bbox(&pk);
        let flip = signed_area2(&pk) < 0.0;
        let ck = centroid(&pk);
        let mut touch: Vec<((i64, i64), Offset)> = Vec::new();
        let mut edge: Vec<((i64, i64), Offset)> = Vec::new();
        for k2 in 0..types.len() {
            for di in -4..=4 {
                for dj in -4..=4 {
                    if k2 == k && di == 0 && dj == 0 {
                        continue;
                    }
                    let q = world(k2, di, dj);
                    let bq = bbox(&q);
                    if bk[0] > bq[2] + eps
                        || bq[0] > bk[2] + eps
                        || bk[1] > bq[3] + eps
                        || bq[1] > bk[3] + eps
                    {
                        continue;
                    }
                    let (c, pos) = contact(&pk, &q, eps);
                    let o = Offset {
                        di,
                        dj,
                        k: k2 as u32,
                    };
                    // The cyclic order key: the position along P's boundary, quantised (and
                    // wrapped) so float noise never reorders it; several tiles touching at one
                    // corner share a position and go by the angle of their centres about P's.
                    let m = pk.len() as i64 * 1_000_000;
                    let qpos = ((pos * 1e6).round() as i64).rem_euclid(m);
                    let cq = centroid(&q);
                    let ang = angle_of(sub(cq, ck));
                    let key = (qpos, (ang * 1e6).round() as i64);
                    if c >= 1 {
                        touch.push((key, o));
                    }
                    if c == 2 {
                        edge.push((key, o));
                    }
                }
            }
        }
        for (list, slot) in [(&mut touch, 0usize), (&mut edge, 1usize)] {
            list.sort_by_key(|x| (x.0, x.1.k, x.1.di, x.1.dj));
            if flip {
                list.reverse();
            }
            offsets[slot].push(list.iter().map(|x| x.1).collect());
        }
    }
    if offsets[1].iter().any(|l| l.is_empty()) {
        return Err("tiles do not meet edge to edge; check the parameters".into());
    }
    Ok(LatticeDef {
        a1,
        a2,
        types,
        shapes,
        offsets,
    })
}

/// Contact between polygons: 0 none, 1 touch at a point, 2 share a stretch of boundary.
/// Also returns a position along P's boundary (edge index + fraction) for cyclic ordering.
pub fn contact(p: &[P], q: &[P], eps: f64) -> (u8, f64) {
    let mut touched: Option<f64> = None;
    let m = p.len();
    for i in 0..m {
        let a = p[i];
        let b = p[(i + 1) % m];
        let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
        let l = crate::detmath::hypot(dx, dy);
        if l < eps {
            continue;
        }
        for j in 0..q.len() {
            let c = q[j];
            let e = q[(j + 1) % q.len()];
            let d1 = ((c[0] - a[0]) * dy - (c[1] - a[1]) * dx) / l;
            let d2 = ((e[0] - a[0]) * dy - (e[1] - a[1]) * dx) / l;
            let t1 = ((c[0] - a[0]) * dx + (c[1] - a[1]) * dy) / l;
            let t2 = ((e[0] - a[0]) * dx + (e[1] - a[1]) * dy) / l;
            if d1.abs() < eps && d2.abs() < eps {
                let lo = t1.min(t2).max(0.0);
                let hi = t1.max(t2).min(l);
                if hi - lo > eps * 5.0 {
                    return (2, i as f64 + (lo + hi) / 2.0 / l);
                }
                if hi - lo > -eps && touched.is_none() {
                    touched = Some(i as f64 + lo.clamp(0.0, l) / l);
                }
            } else {
                if d1.abs() < eps && t1 > -eps && t1 < l + eps && touched.is_none() {
                    touched = Some(i as f64 + t1.clamp(0.0, l) / l);
                }
                if d2.abs() < eps && t2 > -eps && t2 < l + eps && touched.is_none() {
                    touched = Some(i as f64 + t2.clamp(0.0, l) / l);
                }
            }
        }
    }
    if touched.is_none() {
        // A corner of P lying on an edge of Q.
        'outer: for (vi, &v) in p.iter().enumerate() {
            for j in 0..q.len() {
                let c = q[j];
                let e = q[(j + 1) % q.len()];
                let (dx, dy) = (e[0] - c[0], e[1] - c[1]);
                let l = crate::detmath::hypot(dx, dy);
                if l < eps {
                    continue;
                }
                let d = ((v[0] - c[0]) * dy - (v[1] - c[1]) * dx) / l;
                let t = ((v[0] - c[0]) * dx + (v[1] - c[1]) * dy) / l;
                if d.abs() < eps && t > -eps && t < l + eps {
                    touched = Some(vi as f64);
                    break 'outer;
                }
            }
        }
    }
    match touched {
        Some(pos) => (1, pos % m as f64),
        None => (0, 0.0),
    }
}

/// Kinds of glue for UI display.
pub fn glue_kind_name(k: GlueKind) -> &'static str {
    match k {
        GlueKind::Direct => "slide/turn",
        GlueKind::Flip => "glide",
        GlueKind::SelfHalfTurn => "half turn",
        GlueKind::Shared => "shared",
        GlueKind::Mirror => "mirror",
        GlueKind::Straight => "straight",
    }
}

// ---------------------------------------------------------------------------------------------
// Tile editor support

/// Transforms of the tiles around the prototile (identity excluded), for the editor's preview.
/// Reshapeable families: up to `depth` glue steps. Aperiodic: the touching neighbours of a central
/// tile of the first shape in a small patch. Static families: neighbouring copies of the unit cell.
pub fn ghost_transforms(spec: &TilingSpec, depth: usize) -> Result<Vec<(Affine, usize)>, String> {
    let fam = families::family(&spec.family).ok_or("unknown family")?;
    let params = spec
        .params
        .clone()
        .unwrap_or_else(|| families::init_params(fam.id));
    if let Some(g) = crate::isohedral::ghosts(fam.id, &params) {
        return Ok(g?.into_iter().map(|m| (m, 0)).collect());
    }
    match fam.kind {
        FamilyKind::Reshapeable => {
            let corners = families::corners(fam.id, &params)?;
            let t = edges::glue_maps(fam.classes, &corners);
            let key = |m: &Affine| m.0.map(|v| (v * 4e3).round() as i64);
            let mut seen = std::collections::HashSet::new();
            seen.insert(key(&Affine::ID));
            let mut out = Vec::new();
            let mut frontier = vec![Affine::ID];
            for _ in 0..depth {
                let mut next = Vec::new();
                for m in &frontier {
                    for g in &t {
                        let nm = m.mul(g);
                        if seen.insert(key(&nm)) {
                            out.push((nm, 0));
                            next.push(nm);
                        }
                    }
                }
                frontier = next;
            }
            Ok(out)
        }
        FamilyKind::Aperiodic => {
            let small = TilingSpec {
                size: Some(SizeSpec::Level(aperiodic::preview_level(fam.id))),
                boundary: Boundary::Dead,
                ..spec.clone()
            };
            let t = Topology::build(&small)?;
            let cs: Vec<P> = (0..t.len()).map(|i| t.centroid(i)).collect();
            let c0 = centroid(&cs);
            let best = (0..cs.len())
                .filter(|&i| !t.is_open(i) && t.shape_of(i) == 0)
                .min_by(|&a, &b| dist(cs[a], c0).partial_cmp(&dist(cs[b], c0)).unwrap())
                .ok_or("patch too small")?;
            let mi = t.transform(best).inv();
            let mut out = Vec::new();
            t.for_each_neighbor(best, Nb::Touch, |j| {
                out.push((mi.mul(&t.transform(j)), t.shape_of(j)))
            });
            Ok(out)
        }
        FamilyKind::Static => {
            let (a1, a2, polys) = families::static_cell(fam.id, &params).ok_or("no cell")?;
            let mut out = Vec::new();
            for i in -1..=1 {
                for j in -1..=1 {
                    let t = add(scale(a1, i as f64), scale(a2, j as f64));
                    for k in 0..polys.len() {
                        if i != 0 || j != 0 || k != 0 {
                            out.push((Affine::trans(t[0], t[1]), k));
                        }
                    }
                }
            }
            Ok(out)
        }
    }
}

/// Do curved edges change who shares an edge with whom? Builds a small patch and compares the
/// straight-corner neighbour graph (used by the CAs) with contact measured on the curved outlines.
/// Returns (cells whose edge neighbours differ, cells checked).
pub fn curve_check(spec: &TilingSpec) -> Result<(usize, usize), String> {
    let mut small = spec.clone();
    small.boundary = Boundary::Dead;
    small.size = Some(match families::family(&spec.family).map(|f| f.kind) {
        Some(FamilyKind::Aperiodic) => SizeSpec::Level(aperiodic::preview_level(&spec.family)),
        _ => SizeSpec::Cells(110),
    });
    let t = Topology::build(&small)?;
    if t.family.kind == FamilyKind::Static {
        return Ok((0, 0));
    }
    let outline = |i: usize| -> Vec<P> {
        let m = t.transform(i);
        t.outlines[t.shape_of(i)]
            .iter()
            .map(|&p| m.apply(p))
            .collect()
    };
    let (mut changed, mut checked) = (0, 0);
    for i in 0..t.len() {
        if !t.is_interior(i) {
            continue;
        }
        checked += 1;
        let oi = outline(i);
        let mut straight = Vec::new();
        t.for_each_neighbor(i, Nb::Edge, |j| straight.push(j));
        let mut curved = Vec::new();
        t.for_each_neighbor(i, Nb::Touch, |j| {
            if contact(&oi, &outline(j), 1e-4).0 == 2 {
                curved.push(j);
            }
        });
        straight.sort_unstable();
        curved.sort_unstable();
        if straight != curved {
            changed += 1;
        }
    }
    Ok((changed, checked))
}

/// Lattice dimensions that fill a `view_w` x `view_h` pixel view with tiles about `cell_px`
/// pixels across (the square root of the mean tile area). This is the app's *Fill the view* size
/// and the scripts' `size: "screen"`. Aperiodic tilings and tilings that fail to build get
/// `Cells(900)`; each side is kept to 8..=4000 unit cells.
pub fn fill_size(spec: &TilingSpec, view_w: f64, view_h: f64, cell_px: f64) -> SizeSpec {
    let probe = TilingSpec {
        size: Some(SizeSpec::Cells(400)),
        boundary: Boundary::Dead,
        ..spec.clone()
    };
    let Ok(t) = Topology::build(&probe) else {
        return SizeSpec::Cells(900);
    };
    let Some(g) = t.lattice() else {
        return SizeSpec::Cells(900);
    };
    let tile_area = cross(g.a1, g.a2).abs() / g.ntypes as f64;
    let zoom = cell_px.max(1e-3) / tile_area.sqrt();
    let w = (view_w / (zoom * g.a1[0])).floor().clamp(8.0, 4000.0) as u32;
    let h = (view_h / (zoom * g.a2[1])).floor().clamp(8.0, 4000.0) as u32;
    SizeSpec::Rect { w, h }
}
