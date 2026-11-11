//! Sliding-window aperiodic patches: only the tiles of an infinite aperiodic tiling that lie
//! near a region of interest (a set of discs around the live cells of a moving pattern), so a
//! glider can be followed indefinitely with memory bounded by the window, not by the distance.
//!
//! **Addressing.** A tile's address is its path through the supertile hierarchy: a top
//! supertile and the child index taken at every level down to the tile. The window keeps only
//! the top of the hierarchy (its [`Frame`]) and expands, depth first, only the branches whose
//! bounding disc meets a window disc. When the window nears the top supertile's edge, the
//! hierarchy grows by a level: the Spectre gains an ancestor (a parent label and the child index
//! the old top takes in it, chosen so the window lies deep in the new top; up to
//! [`Hier::fixed_levels`] the Δ chain of the fixed level patches, so the window holds exactly
//! the tiles of `aperiodic::build(\"spectre\", 8)` there), and the hat and Penrose tilings, whose
//! size classes grow round one centre, a generation step. The depth grows with the distance
//! travelled (logarithmically), the memory stays that of the window.
//!
//! **Exactness.** Every placement is an exact rigid motion in Z[ζ₁₂] or Z[ζ₁₀]
//! ([`crate::exact`]): the Spectre's supertile transforms are composed exactly from the top down,
//! Penrose half-tiles are deflated exactly, and hats are snapped to their lattice. A tile is
//! identified by its exact placement, so tiles from separately expanded branches or from two
//! windows match by equality, and neighbours are found by the exact edge matcher on the window's
//! tiles ([`crate::aperiodic::from_places`], the builder of the exact level patches, in their
//! neighbour order). Floats only decide which branches to expand and which tiles to keep (a tile
//! is in the window when its vertex average lies in a disc); they never decide adjacency.
//!
//! **Coverage.** After a window is built, an open tile (one with an unmatched edge) whose centre
//! lies deeper than two tile radii inside a window disc means the top supertile ends inside the
//! window, and the hierarchy grows until no such tile remains. Whatever the window holds, a tile
//! whose edges are all matched has its true edge neighbours, and a tile two neighbour steps from
//! every open tile has its true touching neighbours, so the tracker ([`track`]) keeps live cells
//! at least the rule's edge margin away from open tiles and is exact without relying on floats.

use crate::exact::{Cyclo, FxMap, Iso, Place, Z4, Z10, Z12, zadd, zscale};
use crate::geom::{Affine, P, dist};
use crate::penrose::Penrose;
use crate::spectre::{L, LABELS, rules};
use crate::topology::{SizeSpec, TilingSpec, Topology};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

pub mod track;

/// A disc of the window, in exact-coordinate floats (the frame of the exact placements).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Disc {
    pub c: P,
    pub r: f64,
}

/// How far a point lies inside the union of discs (negative outside).
pub fn depth(discs: &[Disc], p: P) -> f64 {
    discs
        .iter()
        .map(|d| d.r - dist(p, d.c))
        .fold(f64::NEG_INFINITY, f64::max)
}

/// The top of the supertile hierarchy above a window.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Frame {
    /// Spectre: a level-`base` Δ supertile at the identity (the start patch's frame) and its
    /// ancestors, one per level above `base`: `[parent label, child index]`, the label an index
    /// into Γ, Δ, Θ, Λ, Ξ, Π, Σ, Φ, Ψ and the child index the supertile below takes in it.
    Spectre { base: u32, chain: Vec<[u8; 2]> },
    /// Hat and Penrose: the generation depth `k` of the size classes' construction about their
    /// fixed centre (`hat::generate_window`, `penrose::generate_window`).
    Centred { k: u32 },
}

impl Frame {
    /// The top supertile's level (Spectre) or generation depth.
    pub fn level(&self) -> u32 {
        match self {
            Frame::Spectre { base, chain } => base + chain.len() as u32,
            Frame::Centred { k } => *k,
        }
    }
}

/// Exact Spectre substitution tables, grown on demand.
struct SpectreTables {
    /// `levels[n][i]`: child `i` (a level-`n` supertile) inside a level-`n + 1` parent.
    levels: Vec<[Iso; 8]>,
    quad: [Z4; 4],
    /// Bounding disc per label (in the order of `LABELS`) at each level, in the label's frame.
    discs: Vec<[Disc; 9]>,
    gamma2: Iso,
}

impl SpectreTables {
    fn new() -> SpectreTables {
        let c = crate::spectre::spectre_exact_corners();
        let gamma2 = Iso {
            rot: 1,
            refl: false,
            t: c[8],
        };
        let pts = |isos: &[Iso]| -> Vec<P> {
            isos.iter()
                .flat_map(|m| c.iter().map(move |&z| Z12::to_f64(m.apply::<Z12>(z))))
                .collect()
        };
        let disc_of = |p: &[P]| -> Disc {
            let n = p.len() as f64;
            let m = p
                .iter()
                .fold([0.0, 0.0], |a, q| [a[0] + q[0] / n, a[1] + q[1] / n]);
            let r = p.iter().map(|&q| dist(q, m)).fold(0.0, f64::max);
            Disc {
                c: m,
                r: r * (1.0 + 1e-12) + 1e-9,
            }
        };
        let one = disc_of(&pts(&[Iso::ID]));
        let two = disc_of(&pts(&[Iso::ID, gamma2]));
        let mut d0 = [one; 9];
        d0[0] = two;
        SpectreTables {
            levels: Vec::new(),
            quad: [c[3], c[5], c[7], c[11]],
            discs: vec![d0],
            gamma2,
        }
    }

    /// Make the tables reach level `n` (discs up to `n`, transforms up to `n - 1`).
    fn ensure(&mut self, n: u32) {
        while self.discs.len() <= n as usize {
            let (tr, q) = crate::spectre::super_step_exact(&self.quad);
            let below = *self.discs.last().unwrap();
            let mut next = below;
            for (pi, &p) in LABELS.iter().enumerate() {
                let kids: Vec<(P, f64)> = rules(p)
                    .iter()
                    .enumerate()
                    .filter_map(|(i, c)| {
                        c.map(|c| {
                            let d = below[label_index(c)];
                            (tr[i].to_affine::<Z12>().apply(d.c), d.r)
                        })
                    })
                    .collect();
                let n = kids.len() as f64;
                let m = kids
                    .iter()
                    .fold([0.0, 0.0], |a, k| [a[0] + k.0[0] / n, a[1] + k.0[1] / n]);
                let r = kids.iter().map(|k| dist(k.0, m) + k.1).fold(0.0, f64::max);
                next[pi] = Disc {
                    c: m,
                    r: r * (1.0 + 1e-12) + 1e-9,
                };
            }
            self.levels.push(tr);
            self.quad = q;
            self.discs.push(next);
        }
    }
}

fn label_index(l: L) -> usize {
    LABELS.iter().position(|&x| x == l).unwrap()
}

/// The supertile hierarchy above a window, and the family's exact prototiles.
pub struct Hier {
    pub family: String,
    pub frame: Frame,
    /// Spectre: follow the Δ chain of the fixed level patches up to this level, then choose
    /// ancestors that keep the window deep.
    pub fixed_levels: u32,
    ring: u8,
    /// Per shape: the sum of its exact corners and their number (vertex averages).
    sums: Vec<(Z4, i64)>,
    /// Largest distance from a tile's vertex average to its corners.
    pub rho: f64,
    /// Drawing frame (world = `post` ∘ exact ∘ `pre`).
    pub pre: Affine,
    pub post: Affine,
    spectre: Option<SpectreTables>,
    /// Hat: the largest distance of a float hat position from the lattice point it was snapped
    /// to (the float hierarchy's error; the snap allows [`HAT_SNAP`]).
    pub snap: f64,
}

/// Largest distance a hat's float position may be from its lattice point.
pub const HAT_SNAP: f64 = 0.05;

/// Largest hierarchy level (Spectre) or generation depth: far beyond any reachable distance
/// (a level-40 Spectre supertile is about 10¹⁸ units across).
pub const MAX_LEVEL: u32 = 34;

impl Hier {
    /// The hierarchy for a window whose start cells are given on a level-`base` patch (the
    /// Spectre's frame is that patch's; the hat and Penrose patches share one frame).
    pub fn new(family: &str, base: u32, fixed_levels: u32) -> Result<Hier, String> {
        let frame = match family {
            "spectre" => Frame::Spectre {
                base,
                chain: vec![],
            },
            "hat" => Frame::Centred { k: 4 },
            "penrose-kd" | "penrose-rhomb" => Frame::Centred { k: 3 },
            _ => {
                return Err(format!(
                    "sliding windows are for spectre, hat, penrose-kd, and penrose-rhomb, not {family}"
                ));
            }
        };
        Hier::with_frame(family, frame, fixed_levels)
    }

    /// A hierarchy at a saved frame (a checkpoint).
    pub fn with_frame(family: &str, frame: Frame, fixed_levels: u32) -> Result<Hier, String> {
        let (ring, corners, pre, post): (u8, Vec<Vec<Z4>>, Affine, Affine) = match family {
            "spectre" => (
                12,
                vec![crate::spectre::spectre_exact_corners().to_vec()],
                Affine::ID,
                Affine::ID,
            ),
            "hat" | "penrose-kd" | "penrose-rhomb" => {
                let (shapes, pre, post) = crate::sizeclass::exact_frame(family)?;
                let ring = if family == "hat" { 12 } else { 10 };
                (
                    ring,
                    shapes.into_iter().map(|s| s.corners).collect(),
                    pre,
                    post,
                )
            }
            _ => return Err(format!("no sliding window for '{family}'")),
        };
        let to_f = |z: Z4| -> P {
            if ring == 12 {
                Z12::to_f64(z)
            } else {
                Z10::to_f64(z)
            }
        };
        let sums: Vec<(Z4, i64)> = corners
            .iter()
            .map(|c| (c.iter().fold([0; 4], |a, &b| zadd(a, b)), c.len() as i64))
            .collect();
        let mut rho = 0.0f64;
        for (c, &(s, m)) in corners.iter().zip(&sums) {
            let mid = to_f(s);
            let mid = [mid[0] / m as f64, mid[1] / m as f64];
            for &z in c {
                rho = rho.max(dist(to_f(z), mid));
            }
        }
        if let (Frame::Spectre { .. }, false) = (&frame, family == "spectre") {
            return Err("a Spectre frame on another tiling".into());
        }
        let spectre = (family == "spectre").then(SpectreTables::new);
        Ok(Hier {
            family: family.into(),
            frame,
            fixed_levels,
            ring,
            sums,
            rho,
            pre,
            post,
            spectre,
            snap: 0.0,
        })
    }

    pub fn level(&self) -> u32 {
        self.frame.level()
    }

    /// Exact-coordinate float of a tile's vertex average.
    pub fn centre(&self, shape: u16, p: &Place) -> P {
        let (s, m) = self.sums[shape as usize];
        let iso = p.iso();
        let z = if self.ring == 12 {
            iso.apply_vec::<Z12>(s)
        } else {
            iso.apply_vec::<Z10>(s)
        };
        let z = zadd(z, zscale(iso.t, m));
        let f = if self.ring == 12 {
            Z12::to_f64(z)
        } else {
            Z10::to_f64(z)
        };
        [f[0] / m as f64, f[1] / m as f64]
    }

    /// Exact-coordinate float of a world point (the inverse of the drawing frame's `post`).
    pub fn from_world(&self, p: P) -> P {
        self.post.inv().apply(p)
    }

    /// The Spectre's top supertile: (label, level, placement).
    fn spectre_top(&mut self) -> (L, u32, Iso) {
        let Frame::Spectre { base, chain } = &self.frame else {
            unreachable!()
        };
        let (base, chain) = (*base, chain.clone());
        let t = self.spectre.as_mut().unwrap();
        t.ensure(base + chain.len() as u32 + 2);
        let mut m = Iso::ID;
        let mut label = L::Delta;
        for (j, &[p, i]) in chain.iter().enumerate() {
            let n = base as usize + j;
            m = m.mul::<Z12>(&t.levels[n][i as usize].inv::<Z12>());
            label = LABELS[p as usize];
        }
        (label, base + chain.len() as u32, m)
    }

    /// The tiles whose vertex average lies in a disc, in depth-first hierarchy order (the
    /// numbering order of the fixed patches).
    pub fn tiles_in(&mut self, discs: &[Disc]) -> Result<Vec<(u16, Place)>, String> {
        let mut out: Vec<(u16, Place)> = Vec::new();
        let fit = |i: &Iso| {
            i.compact()
                .ok_or_else(|| "tile coordinates overflow 32 bits".to_string())
        };
        match self.frame.clone() {
            Frame::Spectre { .. } => {
                let (label, level, m) = self.spectre_top();
                let t = self.spectre.as_ref().unwrap();
                let mut stack = vec![(label, level, m)];
                // Depth first in child order: push children in reverse.
                let mut leaves: Vec<Iso> = Vec::new();
                while let Some((l, n, m)) = stack.pop() {
                    let d = t.discs[n as usize][label_index(l)];
                    let c = m.to_affine::<Z12>().apply(d.c);
                    let slack = 1e-6 * (1.0 + c[0].abs() + c[1].abs());
                    if !discs.iter().any(|w| dist(c, w.c) <= w.r + d.r + slack) {
                        continue;
                    }
                    if n == 0 {
                        leaves.push(m);
                        if l == L::Gamma {
                            leaves.push(m.mul::<Z12>(&t.gamma2));
                        }
                        // Leaves are emitted in pop order, which is depth-first child order.
                        for iso in leaves.drain(..) {
                            let p = fit(&iso)?;
                            if depth(discs, self.centre(0, &p)) >= 0.0 {
                                out.push((0, p));
                            }
                        }
                        continue;
                    }
                    let kids = rules(l);
                    for i in (0..8).rev() {
                        if let Some(c) = kids[i] {
                            stack.push((c, n - 1, m.mul::<Z12>(&t.levels[n as usize - 1][i])));
                        }
                    }
                }
            }
            Frame::Centred { k } => {
                let slack = 2.0 * self.rho + 1e-6;
                let mut err = None;
                let mut snap = 0.0f64;
                let mut got: Vec<(u16, Place)> = Vec::new();
                if self.family == "hat" {
                    let keep =
                        |c: P, r: f64| discs.iter().any(|w| dist(c, w.c) <= w.r + r + slack + 8.0);
                    crate::hat::generate_window(k, &keep, &mut |m| {
                        if err.is_some() {
                            return;
                        }
                        match crate::sizeclass::snap_hat_place(&m, HAT_SNAP)
                            .and_then(|(i, off)| Ok((fit(&i)?, off)))
                        {
                            Ok((p, off)) => {
                                snap = snap.max(off);
                                got.push((0, p));
                            }
                            Err(e) => err = Some(e),
                        }
                    });
                } else {
                    let kind = if self.family == "penrose-kd" {
                        Penrose::P2
                    } else {
                        Penrose::P3
                    };
                    let keep = |t: [P; 3]| {
                        let m = [
                            (t[0][0] + t[1][0] + t[2][0]) / 3.0,
                            (t[0][1] + t[1][1] + t[2][1]) / 3.0,
                        ];
                        let r = t.iter().map(|&q| dist(q, m)).fold(0.0, f64::max);
                        discs.iter().any(|w| dist(m, w.c) <= w.r + r + slack + 1e-6)
                    };
                    crate::penrose::generate_window(kind, k, &keep, &mut |s, i| {
                        got.push((s, fit(&i)?));
                        Ok(())
                    })?;
                }
                if let Some(e) = err {
                    return Err(e);
                }
                self.snap = self.snap.max(snap);
                got.retain(|(s, p)| depth(discs, self.centre(*s, p)) >= 0.0);
                // The size classes' order (nearest the centre first), so neighbour lists break
                // ties as the fixed patches do.
                let order = crate::sizeclass::size_class_order(&self.family, &got)?;
                out = order.into_iter().map(|i| got[i as usize]).collect();
            }
        }
        Ok(out)
    }

    /// Grow the hierarchy by one level.
    fn grow(&mut self, discs: &[Disc]) -> Result<(), String> {
        if self.level() >= MAX_LEVEL {
            return Err(format!(
                "the supertile hierarchy reached level {MAX_LEVEL} without covering the window"
            ));
        }
        match &mut self.frame {
            Frame::Centred { k } => {
                *k += 1;
                Ok(())
            }
            Frame::Spectre { .. } => {
                let (label, n, m) = self.spectre_top();
                let entry = if n < self.fixed_levels && label == L::Delta {
                    [1u8, 1u8]
                } else {
                    let t = self.spectre.as_ref().unwrap();
                    // Score of a candidate supertile: how far out the window reaches, as a
                    // fraction of its bounding radius.
                    let score = |l: usize, lv: usize, g: &Iso| -> f64 {
                        let d = t.discs[lv][l];
                        let c = g.to_affine::<Z12>().apply(d.c);
                        discs
                            .iter()
                            .map(|w| (dist(c, w.c) + w.r) / d.r)
                            .fold(0.0, f64::max)
                    };
                    let parents = |l: L| -> Vec<(usize, usize)> {
                        let mut v = Vec::new();
                        for (pi, &p) in LABELS.iter().enumerate() {
                            for (i, c) in rules(p).iter().enumerate() {
                                if *c == Some(l) {
                                    v.push((pi, i));
                                }
                            }
                        }
                        v
                    };
                    let n = n as usize;
                    let mut best: Option<(f64, [u8; 2])> = None;
                    for (p, i) in parents(label) {
                        let mp = m.mul::<Z12>(&t.levels[n][i].inv::<Z12>());
                        // Look one level further: the best grandparent for this parent.
                        let s = parents(LABELS[p])
                            .into_iter()
                            .map(|(q, j)| {
                                let g = mp.mul::<Z12>(&t.levels[n + 1][j].inv::<Z12>());
                                score(q, n + 2, &g)
                            })
                            .fold(f64::INFINITY, f64::min);
                        if best.is_none_or(|b| s < b.0) {
                            best = Some((s, [p as u8, i as u8]));
                        }
                    }
                    best.ok_or("no parent supertile")?.1
                };
                if let Frame::Spectre { chain, .. } = &mut self.frame {
                    chain.push(entry);
                }
                Ok(())
            }
        }
    }

    /// The window of tiles in the discs, growing the hierarchy until it covers them.
    pub fn build(&mut self, discs: &[Disc]) -> Result<WindowPatch, String> {
        loop {
            let tiles = self.tiles_in(discs)?;
            let centres: Vec<P> = tiles.iter().map(|(s, p)| self.centre(*s, p)).collect();
            let (shape, places): (Vec<u16>, Vec<Place>) = tiles.iter().copied().unzip();
            let built = crate::aperiodic::from_places(&self.family, shape, places)?;
            let lim = 2.0 * self.rho + 1e-6;
            let short = built
                .open
                .iter()
                .zip(&centres)
                .any(|(&o, &c)| o && depth(discs, c) > lim);
            if short || tiles.is_empty() {
                self.grow(discs)?;
                continue;
            }
            let spec =
                TilingSpec::new(&self.family).with_size(SizeSpec::Level(self.level().min(255)));
            // Bounds from the vertex averages and the tile radius; the centre cell nearest
            // the first disc's centre.
            let mut bounds = [
                f64::INFINITY,
                f64::INFINITY,
                f64::NEG_INFINITY,
                f64::NEG_INFINITY,
            ];
            let mut centre = (f64::INFINITY, 0);
            for (i, &c) in centres.iter().enumerate() {
                let w = self.post.apply(c);
                bounds = [
                    bounds[0].min(w[0] - self.rho),
                    bounds[1].min(w[1] - self.rho),
                    bounds[2].max(w[0] + self.rho),
                    bounds[3].max(w[1] + self.rho),
                ];
                let d = dist(c, discs[0].c);
                if d < centre.0 {
                    centre = (d, i);
                }
            }
            let topo = Arc::new(Topology::from_patch(
                &spec,
                built,
                Some((bounds, centre.1)),
            )?);
            let index = tiles
                .iter()
                .enumerate()
                .map(|(i, &k)| (k, i as u32))
                .collect();
            return Ok(WindowPatch {
                topo,
                keys: tiles,
                index,
                centres,
                discs: discs.to_vec(),
                level: self.level(),
            });
        }
    }
}

/// The tiles of one window, with their neighbour graph.
pub struct WindowPatch {
    pub topo: Arc<Topology>,
    /// Per tile: shape and exact placement.
    pub keys: Vec<(u16, Place)>,
    pub index: FxMap<(u16, Place), u32>,
    /// Exact-coordinate floats of the vertex averages.
    pub centres: Vec<P>,
    pub discs: Vec<Disc>,
    /// The hierarchy's level when it was built.
    pub level: u32,
}

impl WindowPatch {
    pub fn len(&self) -> usize {
        self.keys.len()
    }
    pub fn is_empty(&self) -> bool {
        self.keys.is_empty()
    }
    pub fn find(&self, key: &(u16, Place)) -> Option<u32> {
        self.index.get(key).copied()
    }
}

/// A Place as plain numbers, for checkpoints.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Key(pub u16, pub [i32; 4], pub u8, pub bool);

impl Key {
    pub fn of(k: &(u16, Place)) -> Key {
        Key(k.0, k.1.t, k.1.rot, k.1.refl)
    }
    pub fn key(&self) -> (u16, Place) {
        (
            self.0,
            Place {
                t: self.1,
                rot: self.2,
                refl: self.3,
            },
        )
    }
}
