//! Aperiodic tilings as explicit patches, one generator per family.
//!
//! A generator returns a [`Patch`]: the prototile shapes and, for each tile, its shape index and
//! the transform placing it. `topology.rs` turns a patch into an explicit neighbour graph.
//!
//! The Spectre's level is its substitution level. For the hat and the Penrose tilings a level is a
//! size class matching the Spectre's growth: the [`SIZE_CLASSES`] tiles nearest a fixed centre
//! of a substitution patch grown until a disc around that centre is completely covered. That gives
//! round patches of predictable size from substitutions that grow by φ⁴ per step, and each level
//! contains the smaller ones.

use crate::geom::*;
use crate::penrose::{self, Penrose};
use crate::topology::{Csr, Placements, Topology};
use crate::{hat, spectre};

/// Tile counts of size-class levels 1 to 8 (hat and Penrose tilings). Levels 1 to 6 are cut
/// from float substitution patches ([`patch`]); 7 and 8 are built exactly
/// ([`crate::sizeclass`]).
pub const SIZE_CLASSES: [usize; 8] = [10, 70, 500, 4000, 35000, 280000, 2_240_000, 17_920_000];
/// Size classes up to this level are cut from float patches, as they always were.
const FLOAT_CLASSES: u32 = 6;
const CLASS_LEVELS: &[(u32, usize)] = &[
    (3, 500),
    (4, 4000),
    (5, 35000),
    (6, 280_000),
    (7, 2_240_000),
    (8, 17_920_000),
];

fn size_class(id: &str) -> bool {
    matches!(id, "hat" | "penrose-kd" | "penrose-rhomb")
}

pub struct Patch {
    /// Prototile corner polygons (any orientation; reflections are allowed in the transforms).
    pub shapes: Vec<Vec<P>>,
    /// Per tile: (shape index, transform from shape coordinates).
    pub cells: Vec<(u16, Affine)>,
}

impl Patch {
    pub fn polygons(&self) -> Vec<Vec<P>> {
        self.cells
            .iter()
            .map(|(s, m)| {
                self.shapes[*s as usize]
                    .iter()
                    .map(|&p| m.apply(p))
                    .collect()
            })
            .collect()
    }
}

/// The prototiles of a family, without generating a patch.
pub fn prototiles(id: &str) -> Vec<Vec<P>> {
    match id {
        "spectre" => vec![spectre::SPECTRE.to_vec()],
        "hat" => vec![hat::hat_outline()],
        "penrose-kd" => penrose::shapes(Penrose::P2),
        "penrose-rhomb" => penrose::shapes(Penrose::P3),
        _ => vec![],
    }
}

/// Size choices for the UI: (level, tile count).
pub fn levels(id: &str) -> &'static [(u32, usize)] {
    match id {
        "spectre" => &[
            (3, 559),
            (4, 4401),
            (5, 34649),
            (6, 272_791),
            (7, 2_147_679),
            (8, 16_908_641),
        ],
        _ if size_class(id) => CLASS_LEVELS,
        _ => &[],
    }
}

/// Valid levels for a family.
pub fn level_range(id: &str) -> (u32, u32) {
    match id {
        "spectre" => (1, 8),
        _ if size_class(id) => (1, 8),
        _ => (1, 1),
    }
}

/// The largest level used without asking (mover tracking): level 8 (17 to 18 million tiles,
/// 1.3 to 1.6 GB with the graph) only on request.
pub fn auto_level_max(id: &str) -> u32 {
    level_range(id).1.min(7)
}

/// The largest level whose deepest tile can root a glider-search arena: 6, as before the exact
/// patches, so searches reproduce. The larger radii of the cross-radius check grow the same
/// ball onto levels 7 and 8 (the Spectre's level-6 deepest tile has 126 rings of room on level
/// 7, which holds level 6 at its edge, and 441 on level 8).
pub fn arena_root_max(id: &str) -> u32 {
    level_range(id).1.min(6)
}

/// The level used when none is given.
pub fn default_level(id: &str) -> u32 {
    levels(id).first().map(|l| l.0).unwrap_or(1)
}

/// A small level for editor previews and quick checks (a few dozen tiles or more).
pub fn preview_level(id: &str) -> u32 {
    match id {
        "spectre" => 2,
        _ if size_class(id) => 2,
        _ => level_range(id).0,
    }
}

/// Tile count for a level, if known.
pub fn tile_count(id: &str, level: u32) -> Option<usize> {
    if size_class(id) {
        return SIZE_CLASSES.get(level.checked_sub(1)? as usize).copied();
    }
    if id == "spectre" && (1..=level_range(id).1).contains(&level) {
        return Some(spectre::tile_count(level) as usize);
    }
    levels(id).iter().find(|l| l.0 == level).map(|l| l.1)
}

pub fn patch(id: &str, level: u32) -> Result<Patch, String> {
    let (lo, hi) = level_range(id);
    if !(lo..=hi).contains(&level) {
        return Err(format!("{id} level must be {lo} to {hi}, got {level}"));
    }
    if size_class(id) && level > FLOAT_CLASSES {
        return Err(format!(
            "{id} level {level} is built only exactly (aperiodic::build)"
        ));
    }
    match id {
        "spectre" => Ok(Patch {
            shapes: vec![spectre::SPECTRE.to_vec()],
            cells: spectre::spectre_transforms(level)
                .into_iter()
                .map(|m| (0, m))
                .collect(),
        }),
        "hat" => Ok(nearest(
            vec![hat::hat_outline()],
            SIZE_CLASSES[level as usize - 1],
            |k| {
                let (ms, centre) = hat::generate(k);
                (ms.into_iter().map(|m| (0, m)).collect(), centre)
            },
        )),
        "penrose-kd" | "penrose-rhomb" => {
            let kind = if id == "penrose-kd" {
                Penrose::P2
            } else {
                Penrose::P3
            };
            Ok(nearest(
                penrose::shapes(kind),
                SIZE_CLASSES[level as usize - 1],
                |k| (penrose::generate(kind, k), [0.0, 0.0]),
            ))
        }
        _ => Err(format!("no aperiodic generator for '{id}'")),
    }
}

/// A patch with its neighbour graph, ready for a [`crate::topology::Topology`].
pub struct BuiltPatch {
    pub shapes: Vec<Vec<P>>,
    /// Shape index per tile.
    pub shape: Vec<u16>,
    pub placements: Placements,
    /// Touching neighbours and edge neighbours (clockwise on a y-down screen).
    pub touch: Csr,
    pub edge: Csr,
    /// Tiles on the open boundary.
    pub open: Vec<bool>,
}

impl BuiltPatch {
    pub fn len(&self) -> usize {
        self.shape.len()
    }
    pub fn is_empty(&self) -> bool {
        self.shape.is_empty()
    }

    /// The patch's bounding box, and the tile whose vertex average is nearest the average of all
    /// tiles' vertex averages (the first such tile on a tie).
    pub fn bounds_and_center(&self) -> ([f64; 4], usize) {
        use rayon::prelude::*;
        let n = self.len();
        if n == 0 {
            return ([0.0; 4], 0);
        }
        let poly = |i: usize| -> Vec<P> {
            let m = self.placements.get(i);
            self.shapes[self.shape[i] as usize]
                .iter()
                .map(|&p| m.apply(p))
                .collect()
        };
        let bounds = (0..n).into_par_iter().map(|i| bbox(&poly(i))).reduce(
            || {
                [
                    f64::INFINITY,
                    f64::INFINITY,
                    f64::NEG_INFINITY,
                    f64::NEG_INFINITY,
                ]
            },
            |a, b| {
                [
                    a[0].min(b[0]),
                    a[1].min(b[1]),
                    a[2].max(b[2]),
                    a[3].max(b[3]),
                ]
            },
        );
        // The average of the vertex averages: summed in order below a million tiles (as the
        // float patches always were), in parallel above.
        let c0 = if n < 1 << 20 {
            let cs: Vec<P> = (0..n).map(|i| centroid(&poly(i))).collect();
            centroid(&cs)
        } else {
            let s = (0..n)
                .into_par_iter()
                .map(|i| centroid(&poly(i)))
                .fold(|| [0.0f64; 2], |a, c| [a[0] + c[0], a[1] + c[1]])
                .reduce(|| [0.0; 2], |a, b| [a[0] + b[0], a[1] + b[1]]);
            [s[0] / n as f64, s[1] / n as f64]
        };
        let center = (0..n)
            .into_par_iter()
            .map(|i| ((dist(centroid(&poly(i)), c0) * 1e6).round(), i))
            .reduce(
                || (f64::INFINITY, usize::MAX),
                |a, b| match a.0.partial_cmp(&b.0) {
                    Some(std::cmp::Ordering::Less) => a,
                    Some(std::cmp::Ordering::Greater) => b,
                    _ if a.1 <= b.1 => a,
                    _ => b,
                },
            )
            .1;
        (bounds, center)
    }
}

/// Build a patch and its neighbour graph. The Spectre is built exactly at every level
/// ([`spectre::spectre_places`], [`spectre::spectre_graph_exact`]); the hat and Penrose size
/// classes by float geometry up to level 6 ([`patch`], [`crate::patch::build_graph`], equal to
/// the exact graph as tested) and exactly above ([`crate::sizeclass`]).
pub fn build(id: &str, level: u32) -> Result<BuiltPatch, String> {
    let (lo, hi) = level_range(id);
    if !(lo..=hi).contains(&level) {
        return Err(format!("{id} level must be {lo} to {hi}, got {level}"));
    }
    if id == "spectre" {
        let places = spectre::spectre_places(level)?;
        let g = spectre::spectre_graph_exact(&places)?;
        return Ok(BuiltPatch {
            shapes: vec![spectre::SPECTRE.to_vec()],
            shape: vec![0; places.len()],
            placements: Placements::Exact {
                ring: 12,
                pre: Affine::ID,
                post: Affine::ID,
                places,
            },
            touch: Csr {
                off: g.touch.0,
                nb: g.touch.1,
            },
            edge: Csr {
                off: g.edge.0,
                nb: g.edge.1,
            },
            open: g.open,
        });
    }
    if size_class(id) && level > FLOAT_CLASSES {
        return crate::sizeclass::build(id, SIZE_CLASSES[level as usize - 1]);
    }
    let p = patch(id, level)?;
    let polys = p.polygons();
    let g = crate::patch::build_graph(&polys);
    Ok(BuiltPatch {
        shapes: p.shapes,
        shape: p.cells.iter().map(|c| c.0).collect(),
        placements: Placements::Float(p.cells.iter().map(|c| c.1).collect()),
        touch: Csr::from_lists(&g.touch),
        edge: Csr::from_lists(&g.edge),
        open: g.open,
    })
}

/// The rigid motion taking a level-`from` patch of `id` onto the same tiles of its level-`to`
/// patch (`from <= to`). The hat and Penrose size classes nest in place (identity); each Spectre
/// level holds the previous one as a mirrored Delta supertile.
pub fn embedding(id: &str, from: u32, to: u32) -> Affine {
    let mut m = Affine::ID;
    if id == "spectre" {
        for l in from..to {
            m = spectre::level_embedding(l).mul(&m);
        }
    }
    m
}

/// The tile of the level-`to` patch that is tile `i` of the level-`from` patch, exactly, when the
/// family numbers its levels so (the Spectre: each level is the Δ child of the next, after the
/// Ξ child's tiles), checked by comparing exact placements. `None` for families without exact
/// numbering (use [`embedding`] and a point lookup).
pub fn map_tile(
    id: &str,
    from: u32,
    to: u32,
    i: usize,
    small: &Topology,
    big: &Topology,
) -> Option<Result<usize, String>> {
    if id != "spectre" || from > to {
        return None;
    }
    let (Some(a), Some(b)) = (small_exact(small), small_exact(big)) else {
        return None;
    };
    let mut j = i;
    let mut m = crate::exact::Iso::ID;
    for l in from..to {
        j += spectre::level_offset(l);
        m = spectre::level_embedding_exact(l).mul::<crate::exact::Z12>(&m);
    }
    Some(match (a.get(i), b.get(j)) {
        (Some(p), Some(q)) if m.mul::<crate::exact::Z12>(&p.iso()) == q.iso() => Ok(j),
        (Some(_), Some(_)) => Err(format!(
            "tile {i} of level {from} is not tile {j} of level {to}: the patches do not nest"
        )),
        _ => Err(format!("tile {i} is outside the level-{to} patch")),
    })
}

fn small_exact(t: &Topology) -> Option<&[crate::exact::Place]> {
    match &t.storage {
        crate::topology::Storage::Explicit(e) => e.transforms.exact(),
        _ => None,
    }
}

/// The `n` tiles nearest a generator's centre, translated so that centre is the origin.
///
/// `grow(k)` returns the tiles of the `k`-th generation and its centre; generations must agree
/// around the centre, so every size is cut from the same tiling and smaller patches nest inside
/// larger ones. The first generation whose fully covered disc about the centre holds `n` tiles is
/// used. Coverage is measured on a grid of cells twice the tile diameter: the tile covering a
/// covered cell's middle has its vertex average inside that cell, so a cell with no tile centre
/// lies (at least partly) outside the patch.
fn nearest(shapes: Vec<Vec<P>>, n: usize, grow: impl Fn(u32) -> (Vec<(u16, Affine)>, P)) -> Patch {
    let mids: Vec<P> = shapes.iter().map(|s| centroid(s)).collect();
    let diam = shapes
        .iter()
        .map(|s| {
            let c = centroid(s);
            2.0 * s.iter().map(|&p| dist(p, c)).fold(0.0, f64::max)
        })
        .fold(0.0, f64::max);
    let cell = 2.0 * diam;
    for k in 1.. {
        let (cells, centre) = grow(k);
        if cells.len() < n {
            continue;
        }
        let pos: Vec<P> = cells
            .iter()
            .map(|(s, m)| sub(m.apply(mids[*s as usize]), centre))
            .collect();
        let r2: Vec<f64> = pos.iter().map(|c| c[0] * c[0] + c[1] * c[1]).collect();
        let far = r2.iter().copied().fold(0.0, f64::max).sqrt();
        let g = (far / cell).ceil() as i64 + 1;
        let w = (2 * g + 1) as usize;
        let mut hit = vec![false; w * w];
        for c in &pos {
            let (x, y) = ((c[0] / cell).floor() as i64, (c[1] / cell).floor() as i64);
            if x.abs() <= g && y.abs() <= g {
                hit[(y + g) as usize * w + (x + g) as usize] = true;
            }
        }
        // Distance from the centre to the nearest empty cell, less a margin of two cells.
        let mut gap = f64::INFINITY;
        for y in -g..=g {
            for x in -g..=g {
                if !hit[(y + g) as usize * w + (x + g) as usize] {
                    let dx = (x as f64 + 0.5).abs() - 0.5;
                    let dy = (y as f64 + 0.5).abs() - 0.5;
                    gap = gap.min(crate::detmath::hypot(dx.max(0.0), dy.max(0.0)) * cell);
                }
            }
        }
        let safe = gap - 2.0 * cell;
        // Guard against a centre that drifts to the patch edge (each step costs ~7x memory).
        assert!(
            cells.len() < 8_000_000,
            "patch of {n} tiles: coverage did not grow"
        );
        if safe <= 0.0 || r2.iter().filter(|&&d| d <= safe * safe).count() < n {
            continue;
        }
        let mut order: Vec<usize> = (0..cells.len()).collect();
        // Quantised, so tiles at equal distances keep their generation order on every platform.
        let q = |d: f64| (d * 1e6).round() as i64;
        order.sort_by(|&a, &b| q(r2[a]).cmp(&q(r2[b])).then(a.cmp(&b)));
        let back = Affine::trans(-centre[0], -centre[1]);
        return Patch {
            shapes,
            cells: order[..n]
                .iter()
                .map(|&i| (cells[i].0, back.mul(&cells[i].1)))
                .collect(),
        };
    }
    unreachable!()
}

/// A patch of `id` from exact placements (any set of tiles of the family's tiling, such as a
/// sliding window's, [`crate::window`]) with its exact neighbour graph, in the frame and
/// neighbour order of the exact level patches.
pub fn from_places(
    id: &str,
    shape: Vec<u16>,
    places: Vec<crate::exact::Place>,
) -> Result<BuiltPatch, String> {
    if id == "spectre" {
        let g = spectre::spectre_graph_exact(&places)?;
        return Ok(BuiltPatch {
            shapes: vec![spectre::SPECTRE.to_vec()],
            shape,
            placements: Placements::Exact {
                ring: 12,
                pre: Affine::ID,
                post: Affine::ID,
                places,
            },
            touch: Csr {
                off: g.touch.0,
                nb: g.touch.1,
            },
            edge: Csr {
                off: g.edge.0,
                nb: g.edge.1,
            },
            open: g.open,
        });
    }
    crate::sizeclass::from_places(id, shape, places)
}
