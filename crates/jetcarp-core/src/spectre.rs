//! Spectre tiling by substitution (Smith, Myers, Kaplan, Goodman-Strauss 2023; Kaplan's construction).
//! Ported from `spectreMatrices` in `reference/core.js` and `reference/spectre.mjs`.

use crate::geom::*;
use std::collections::HashMap;

const H: f64 = S3 / 2.0;
pub const SPECTRE: [P; 14] = [
    [0.0, 0.0],
    [1.0, 0.0],
    [1.5, -H],
    [1.5 + H, 0.5 - H],
    [1.5 + H, 1.5 - H],
    [2.5 + H, 1.5 - H],
    [3.0 + H, 1.5],
    [3.0, 2.0],
    [3.0 - H, 1.5],
    [2.5 - H, 1.5 + H],
    [1.5 - H, 1.5 + H],
    [0.5 - H, 1.5 + H],
    [-H, 1.5],
    [0.0, 1.0],
];

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub(crate) enum L {
    Gamma,
    Delta,
    Theta,
    Lambda,
    Xi,
    Pi,
    Sigma,
    Phi,
    Psi,
}
use L::*;
pub(crate) const LABELS: [L; 9] = [Gamma, Delta, Theta, Lambda, Xi, Pi, Sigma, Phi, Psi];

pub(crate) fn rules(l: L) -> [Option<L>; 8] {
    match l {
        Gamma => [
            Some(Pi),
            Some(Delta),
            None,
            Some(Theta),
            Some(Sigma),
            Some(Xi),
            Some(Phi),
            Some(Gamma),
        ],
        Delta => [
            Some(Xi),
            Some(Delta),
            Some(Xi),
            Some(Phi),
            Some(Sigma),
            Some(Pi),
            Some(Phi),
            Some(Gamma),
        ],
        Theta => [
            Some(Psi),
            Some(Delta),
            Some(Pi),
            Some(Phi),
            Some(Sigma),
            Some(Pi),
            Some(Phi),
            Some(Gamma),
        ],
        Lambda => [
            Some(Psi),
            Some(Delta),
            Some(Xi),
            Some(Phi),
            Some(Sigma),
            Some(Pi),
            Some(Phi),
            Some(Gamma),
        ],
        Xi => [
            Some(Psi),
            Some(Delta),
            Some(Pi),
            Some(Phi),
            Some(Sigma),
            Some(Psi),
            Some(Phi),
            Some(Gamma),
        ],
        Pi => [
            Some(Psi),
            Some(Delta),
            Some(Xi),
            Some(Phi),
            Some(Sigma),
            Some(Psi),
            Some(Phi),
            Some(Gamma),
        ],
        Sigma => [
            Some(Xi),
            Some(Delta),
            Some(Xi),
            Some(Phi),
            Some(Sigma),
            Some(Pi),
            Some(Lambda),
            Some(Gamma),
        ],
        Phi => [
            Some(Psi),
            Some(Delta),
            Some(Psi),
            Some(Phi),
            Some(Sigma),
            Some(Pi),
            Some(Phi),
            Some(Gamma),
        ],
        Psi => [
            Some(Psi),
            Some(Delta),
            Some(Psi),
            Some(Phi),
            Some(Sigma),
            Some(Psi),
            Some(Phi),
            Some(Gamma),
        ],
    }
}

/// The transforms placing each Spectre tile in a patch of the given substitution level.
/// Levels 3, 4, 5 give 559, 4,401, and 34,649 tiles.
pub fn spectre_transforms(level: u32) -> Vec<Affine> {
    // Each level is a list of (child label, transform) per label; level 0 emits tiles directly.
    // Represent a level's system as, per label, the list of level-0 tile transforms it expands to,
    // built lazily by recursion over `kids`.
    let mut quad = [SPECTRE[3], SPECTRE[5], SPECTRE[7], SPECTRE[11]];
    // kids[n][label] = list of (child label, T) for level n+1.
    let mut levels: Vec<HashMap<L, Vec<(L, Affine)>>> = Vec::new();
    for _ in 0..level {
        let (tr, super_quad) = super_step(&quad);
        let mut next = HashMap::new();
        for l in LABELS {
            let kids: Vec<(L, Affine)> = rules(l)
                .iter()
                .enumerate()
                .filter_map(|(i, c)| c.map(|c| (c, tr[i])))
                .collect();
            next.insert(l, kids);
        }
        levels.push(next);
        quad = super_quad;
    }
    let mut out = Vec::new();
    emit(&levels, level as usize, Delta, Affine::ID, &mut out);
    out
}

/// One substitution step: the eight child transforms of a supertile and the next level's key
/// points, from this level's key points.
fn super_step(quad: &[P; 4]) -> ([Affine; 8], [P; 4]) {
    let refl = Affine([-1.0, 0.0, 0.0, 0.0, 1.0, 0.0]);
    let steps: [(f64, usize, usize); 7] = [
        (60.0, 3, 1),
        (0.0, 2, 0),
        (60.0, 3, 1),
        (60.0, 3, 1),
        (0.0, 2, 0),
        (60.0, 3, 1),
        (-120.0, 3, 3),
    ];
    let mut ts = vec![Affine::ID];
    let mut total = 0.0;
    let mut r = Affine::ID;
    let mut tq = *quad;
    for (ang, from, to) in steps {
        total += ang;
        if ang != 0.0 {
            r = Affine::rot(total * std::f64::consts::PI / 180.0);
            tq = quad.map(|q| r.apply(q));
        }
        let p = ts.last().unwrap().apply(quad[from]);
        ts.push(Affine::trans(p[0] - tq[to][0], p[1] - tq[to][1]).mul(&r));
    }
    let tr: Vec<Affine> = ts.iter().map(|t| refl.mul(t)).collect();
    let super_quad = [
        tr[6].apply(quad[2]),
        tr[5].apply(quad[1]),
        tr[3].apply(quad[2]),
        tr[0].apply(quad[1]),
    ];
    (tr.try_into().unwrap(), super_quad)
}

/// Where a level-`level` patch sits inside the level-`level + 1` patch: the level-`level + 1`
/// Delta supertile's second child is a Delta supertile, placed by this map (a reflection), so
/// tile `i` of the smaller patch is the image of it under this map on the larger one.
pub fn level_embedding(level: u32) -> Affine {
    let mut quad = [SPECTRE[3], SPECTRE[5], SPECTRE[7], SPECTRE[11]];
    for _ in 0..level {
        quad = super_step(&quad).1;
    }
    // Delta's children: Xi, Delta, ... (index 1 is the Delta).
    debug_assert_eq!(rules(Delta)[1], Some(Delta));
    super_step(&quad).0[1]
}

fn emit(levels: &[HashMap<L, Vec<(L, Affine)>>], n: usize, l: L, m: Affine, out: &mut Vec<Affine>) {
    if n == 0 {
        out.push(m);
        if l == Gamma {
            out.push(
                m.mul(
                    &Affine::trans(SPECTRE[8][0], SPECTRE[8][1])
                        .mul(&Affine::rot(std::f64::consts::PI / 6.0)),
                ),
            );
        }
        return;
    }
    for (c, t) in &levels[n - 1][&l] {
        emit(levels, n - 1, *c, m.mul(t), out);
    }
}

/// Explicit neighbour graph for a Spectre patch, by exact corner matching
/// (the prototype's fast path in `buildGraph`).
pub struct SpectreGraph {
    pub polys: Vec<Vec<P>>,
    /// Edge neighbours in cyclic boundary order.
    pub edge: Vec<Vec<u32>>,
    /// Touching neighbours (edge or corner) in cyclic boundary order.
    pub touch: Vec<Vec<u32>>,
    /// Tiles with an unmatched unit edge (on the patch boundary).
    pub open: Vec<bool>,
}

fn key(p: P) -> (i64, i64) {
    ((p[0] * 1e4).round() as i64, (p[1] * 1e4).round() as i64)
}

pub fn spectre_graph(transforms: &[Affine]) -> SpectreGraph {
    let polys: Vec<Vec<P>> = transforms
        .iter()
        .map(|m| SPECTRE.iter().map(|&p| m.apply(p)).collect())
        .collect();
    let n = polys.len();
    let mut vm: HashMap<(i64, i64), Vec<u32>> = HashMap::with_capacity(n * 8);
    type Key = (i64, i64);
    let mut em: HashMap<(Key, Key), Vec<u32>> = HashMap::with_capacity(n * 8);
    for (t, poly) in polys.iter().enumerate() {
        for i in 0..poly.len() {
            let a = key(poly[i]);
            let b = key(poly[(i + 1) % poly.len()]);
            vm.entry(a).or_default().push(t as u32);
            let e = if a < b { (a, b) } else { (b, a) };
            em.entry(e).or_default().push(t as u32);
        }
    }
    let mut open = vec![false; n];
    for ts in em.values() {
        if ts.len() != 2 {
            open[ts[0] as usize] = true;
        }
    }
    let mut edge = vec![Vec::new(); n];
    let mut touch = vec![Vec::new(); n];
    for (t, poly) in polys.iter().enumerate() {
        // Orientation: list neighbours clockwise on a y-down screen.
        let flip = signed_area2(poly) < 0.0;
        let m = poly.len();
        let order: Vec<usize> = if flip {
            (0..m).rev().collect()
        } else {
            (0..m).collect()
        };
        let push = |list: &mut Vec<u32>, x: u32| {
            if x as usize != t && !list.contains(&x) {
                list.push(x);
            }
        };
        for &i in &order {
            // Edge i runs from vertex i to i+1 (reversed walk uses edge i-1..i, same set).
            let (a, b) = if flip {
                (key(poly[i]), key(poly[(i + m - 1) % m]))
            } else {
                (key(poly[i]), key(poly[(i + 1) % m]))
            };
            // Corner neighbours at the walk's current vertex, then the edge neighbour.
            let mut at_v: Vec<u32> = vm[&a]
                .iter()
                .copied()
                .filter(|&x| x as usize != t)
                .collect();
            // Order corner-only tiles around the vertex by angle for a stable cyclic order.
            let c = poly[i];
            at_v.sort_by(|&x, &y| {
                let cx = centroid(&polys[x as usize]);
                let cy = centroid(&polys[y as usize]);
                let ax = angle_of(sub(cx, c));
                let ay = angle_of(sub(cy, c));
                ax.partial_cmp(&ay).unwrap()
            });
            for x in at_v {
                push(&mut touch[t], x);
            }
            let e = if a < b { (a, b) } else { (b, a) };
            let ts = &em[&e];
            if ts.len() == 2 {
                let other = if ts[0] as usize == t { ts[1] } else { ts[0] };
                push(&mut edge[t], other);
                push(&mut touch[t], other);
            }
        }
    }
    SpectreGraph {
        polys,
        edge,
        touch,
        open,
    }
}

// ---------------------------------------------------------------------------------------------
// Exact construction (Z[ζ₁₂]).
//
// Every Spectre edge is a unit vector at a multiple of 30°, and every substitution step rotates
// by a multiple of 60°, reflects, and translates by a difference of corners, so all corners lie
// in Z[ζ₁₂] and every transform is an exact [`Iso`]. The functions below repeat the float
// construction above step for step in integers: same supertile order, same tile numbering.

use crate::exact::{
    Cyclo, Iso, OPEN, Place, Z4, Z12, around_vertex, pair_edges, zadd, zscale, zsub,
};

/// Directions (multiples of 30°) of the Spectre's 14 unit edges, edge `i` from corner `i` to
/// corner `i + 1` of [`SPECTRE`].
const EDGE_DIRS: [i64; 14] = [0, 10, 1, 3, 0, 2, 5, 7, 4, 6, 6, 8, 11, 9];

/// The Spectre's corners in Z[ζ₁₂].
pub fn spectre_exact_corners() -> [Z4; 14] {
    let mut out = [[0i64; 4]; 14];
    let mut p = [0i64; 4];
    for i in 0..14 {
        out[i] = p;
        p = zadd(p, Z12::unit(EDGE_DIRS[i]));
    }
    debug_assert_eq!(p, [0; 4]);
    out
}

fn iso_x(i: &Iso, z: Z4) -> Z4 {
    i.apply::<Z12>(z)
}
fn mul_x(a: &Iso, b: &Iso) -> Iso {
    a.mul::<Z12>(b)
}

/// [`super_step`] in exact arithmetic.
pub(crate) fn super_step_exact(quad: &[Z4; 4]) -> ([Iso; 8], [Z4; 4]) {
    // x -> -x: z -> -conj(z) = ζ⁶ conj(z).
    let refl = Iso {
        rot: 6,
        refl: true,
        t: [0; 4],
    };
    // (turn in 30° units, from, to)
    let steps: [(i64, usize, usize); 7] = [
        (2, 3, 1),
        (0, 2, 0),
        (2, 3, 1),
        (2, 3, 1),
        (0, 2, 0),
        (2, 3, 1),
        (-4, 3, 3),
    ];
    let mut ts = vec![Iso::ID];
    let mut total = 0;
    let mut r = Iso::ID;
    let mut tq = *quad;
    for (ang, from, to) in steps {
        total += ang;
        if ang != 0 {
            r = Iso::rot(total, 12);
            tq = quad.map(|q| iso_x(&r, q));
        }
        let p = iso_x(ts.last().unwrap(), quad[from]);
        ts.push(mul_x(&Iso::trans(zsub(p, tq[to])), &r));
    }
    let tr: Vec<Iso> = ts.iter().map(|t| mul_x(&refl, t)).collect();
    let super_quad = [
        iso_x(&tr[6], quad[2]),
        iso_x(&tr[5], quad[1]),
        iso_x(&tr[3], quad[2]),
        iso_x(&tr[0], quad[1]),
    ];
    (tr.try_into().unwrap(), super_quad)
}

fn exact_levels(level: u32) -> Vec<[Iso; 8]> {
    let c = spectre_exact_corners();
    let mut quad = [c[3], c[5], c[7], c[11]];
    let mut out = Vec::new();
    for _ in 0..level {
        let (tr, q) = super_step_exact(&quad);
        out.push(tr);
        quad = q;
    }
    out
}

/// Tiles per supertile label at each level, from the substitution rules (the Γ cluster is two
/// tiles), in the order of `LABELS`.
fn label_counts(level: u32) -> [u128; 9] {
    let mut c = [1u128; 9];
    c[0] = 2; // Gamma
    for _ in 0..level {
        let mut next = [0u128; 9];
        for (i, l) in LABELS.iter().enumerate() {
            for k in rules(*l).iter().flatten() {
                let j = LABELS.iter().position(|x| x == k).unwrap();
                next[i] += c[j];
            }
        }
        c = next;
    }
    c
}

/// Exact tile count of a level-`level` patch (a Δ supertile), from the substitution matrix.
pub fn tile_count(level: u32) -> u128 {
    label_counts(level)[1]
}

/// Index of the first tile of the level-`level` patch inside the level-`level + 1` patch (the Δ
/// child follows the Ξ child), so tile `i` of level `level` is tile `offset + i` one level up.
pub fn level_offset(level: u32) -> usize {
    label_counts(level)[4] as usize // Xi
}

/// [`level_embedding`] in exact arithmetic.
pub fn level_embedding_exact(level: u32) -> Iso {
    let c = spectre_exact_corners();
    let mut quad = [c[3], c[5], c[7], c[11]];
    for _ in 0..level {
        quad = super_step_exact(&quad).1;
    }
    super_step_exact(&quad).0[1]
}

/// The exact placement of every tile of a level-`level` patch, numbered as
/// [`spectre_transforms`].
pub fn spectre_exact(level: u32) -> Vec<Iso> {
    let levels = exact_levels(level);
    let gamma2 = Iso {
        rot: 1,
        refl: false,
        t: spectre_exact_corners()[8],
    };
    let n = tile_count(level) as usize;
    let mut out = Vec::with_capacity(n);
    fn emit(levels: &[[Iso; 8]], n: usize, l: L, m: Iso, g2: &Iso, out: &mut Vec<Iso>) {
        if n == 0 {
            out.push(m);
            if l == Gamma {
                out.push(mul_x(&m, g2));
            }
            return;
        }
        for (i, c) in rules(l).iter().enumerate() {
            if let Some(c) = c {
                emit(levels, n - 1, *c, mul_x(&m, &levels[n - 1][i]), g2, out);
            }
        }
    }
    emit(&levels, level as usize, Delta, Iso::ID, &gamma2, &mut out);
    debug_assert_eq!(out.len(), n);
    out
}

/// [`spectre_exact`] in compact form, built in parallel over supertiles two levels down.
/// Fails if a coordinate does not fit in 32 bits (far beyond any patch that fits in memory).
pub fn spectre_places(level: u32) -> Result<Vec<Place>, String> {
    use rayon::prelude::*;
    let levels = exact_levels(level);
    let gamma2 = Iso {
        rot: 1,
        refl: false,
        t: spectre_exact_corners()[8],
    };
    fn emit(
        levels: &[[Iso; 8]],
        n: usize,
        l: L,
        m: Iso,
        g2: &Iso,
        out: &mut Vec<Place>,
    ) -> Result<(), String> {
        if n == 0 {
            let fit = || format!("Spectre coordinates overflow 32 bits at {:?}", m.t);
            out.push(m.compact().ok_or_else(fit)?);
            if l == Gamma {
                out.push(mul_x(&m, g2).compact().ok_or_else(fit)?);
            }
            return Ok(());
        }
        for (i, c) in rules(l).iter().enumerate() {
            if let Some(c) = c {
                emit(levels, n - 1, *c, mul_x(&m, &levels[n - 1][i]), g2, out)?;
            }
        }
        Ok(())
    }
    // Supertiles `split` levels below the top, in order.
    let split = (level as usize).min(2);
    let mut roots = vec![(Delta, Iso::ID)];
    for d in 0..split {
        let lv = level as usize - d;
        roots = roots
            .into_iter()
            .flat_map(|(l, m)| {
                let ls = &levels;
                rules(l)
                    .into_iter()
                    .enumerate()
                    .filter_map(move |(i, c)| c.map(|c| (c, mul_x(&m, &ls[lv - 1][i]))))
            })
            .collect();
    }
    let below = level as usize - split;
    let parts: Vec<Result<Vec<Place>, String>> = roots
        .par_iter()
        .map(|(l, m)| {
            let mut out = Vec::new();
            emit(&levels, below, *l, *m, &gamma2, &mut out)?;
            Ok(out)
        })
        .collect();
    let mut out = Vec::with_capacity(tile_count(level) as usize);
    for p in parts {
        out.extend(p?);
    }
    Ok(out)
}

/// Precomputed corners of the Spectre under each linear part (rotation, reflection).
pub struct SpectreCorners {
    /// `[refl * 12 + rot][corner]`.
    lin: Vec<[Z4; 14]>,
    /// Sum of the corners per linear part (14 × the vertex average).
    sum: Vec<Z4>,
}

impl SpectreCorners {
    pub fn new() -> SpectreCorners {
        let c = spectre_exact_corners();
        let mut lin = Vec::new();
        let mut sum = Vec::new();
        for refl in [false, true] {
            for rot in 0..12u8 {
                let m = Iso {
                    rot,
                    refl,
                    t: [0; 4],
                };
                let v = c.map(|z| m.apply_vec::<Z12>(z));
                sum.push(v.iter().fold([0; 4], |a, &b| zadd(a, b)));
                lin.push(v);
            }
        }
        SpectreCorners { lin, sum }
    }
    #[inline]
    fn idx(p: &Place) -> usize {
        p.refl as usize * 12 + p.rot as usize
    }
    #[inline]
    pub fn corner(&self, p: &Place, k: usize) -> Z4 {
        let v = self.lin[Self::idx(p)][k];
        [
            v[0] + p.t[0] as i64,
            v[1] + p.t[1] as i64,
            v[2] + p.t[2] as i64,
            v[3] + p.t[3] as i64,
        ]
    }
    /// 14 × the tile's vertex average.
    #[inline]
    pub fn sum(&self, p: &Place) -> Z4 {
        zadd(self.sum[Self::idx(p)], zscale(p.t.map(|x| x as i64), 14))
    }
}

impl Default for SpectreCorners {
    fn default() -> Self {
        Self::new()
    }
}

/// Is the prototile [`SPECTRE`] listed counterclockwise (y up)?
fn proto_ccw() -> bool {
    signed_area2(&SPECTRE) > 0.0
}

/// Neighbour graph of an exact Spectre patch, as CSR arrays.
pub struct ExactGraph {
    /// Touching neighbours (edge or corner): CSR offsets and lists, in the order of
    /// [`spectre_graph`].
    pub touch: (Vec<u32>, Vec<u32>),
    /// Edge neighbours, likewise.
    pub edge: (Vec<u32>, Vec<u32>),
    pub open: Vec<bool>,
}

/// Is the tile listed clockwise in prototile order?
fn flipped(p: &Place) -> bool {
    p.refl == proto_ccw()
}

/// Corner `k` of tile `t` in counterclockwise order (corner 0 first).
fn ccw_corner(sc: &SpectreCorners, places: &[Place], t: usize, k: usize) -> Z4 {
    let p = &places[t];
    if flipped(p) {
        sc.corner(p, (14 - k) % 14)
    } else {
        sc.corner(p, k)
    }
}

/// Match every edge of the patch exactly (see [`crate::exact::pair_edges`]); `partner` is in
/// counterclockwise slot numbering (slot k runs from ccw corner k to k + 1).
fn ccw_pairs(sc: &SpectreCorners, places: &[Place]) -> Result<Vec<u32>, String> {
    pair_edges(
        places.len(),
        14,
        |_| 14,
        |t, k| ccw_corner(sc, places, t, k),
    )
}

/// Exact neighbour graph: edges matched by integer corner coordinates, corners walked around
/// over matched edges. The same lists in the same order as [`spectre_graph`], without
/// tolerances: tiles touching a corner are ordered by the exact angle of their vertex average
/// seen from the corner, ties by tile number.
pub fn spectre_graph_exact(places: &[Place]) -> Result<ExactGraph, String> {
    use rayon::prelude::*;
    let n = places.len();
    let sc = SpectreCorners::new();
    let partner = ccw_pairs(&sc, places)?;
    let corner = |t: usize, k: usize| ccw_corner(&sc, places, t, k);
    let deg = |_t: usize| 14usize;
    let open: Vec<bool> = (0..n)
        .into_par_iter()
        .map(|t| (0..14).any(|k| partner[t * 14 + k] == OPEN))
        .collect();
    // Per chunk of tiles: touch lists and lengths, edge lists and lengths. Chunks are built in
    // parallel a batch at a time and appended to the CSR arrays, so only one batch is held twice.
    // (Interior Spectres average 6.4 touching and 5.5 edge neighbours.)
    let chunk = 1 << 14;
    let mut touch = (
        Vec::with_capacity(n + 1),
        Vec::with_capacity(n * 13 / 2 + 64),
    );
    let mut edge = (
        Vec::with_capacity(n + 1),
        Vec::with_capacity(n * 28 / 5 + 64),
    );
    touch.0.push(0u32);
    edge.0.push(0u32);
    let chunks: Vec<usize> = (0..n.div_ceil(chunk)).collect();
    for batch in chunks.chunks(rayon::current_num_threads().max(1) * 4) {
        type Part = (Vec<u32>, Vec<u8>, Vec<u32>, Vec<u8>);
        let parts: Vec<Part> = batch
            .par_iter()
            .map(|&c| {
                let (mut tl, mut tn, mut el, mut en) =
                    (Vec::new(), Vec::new(), Vec::new(), Vec::new());
                let mut around = Vec::new();
                for t in c * chunk..((c + 1) * chunk).min(n) {
                    let p = &places[t];
                    let flip = flipped(p);
                    let (t0, e0) = (tl.len(), el.len());
                    // Prototile corners in the old walk order: forwards, or backwards from 13.
                    for s in 0..14 {
                        let i = if flip { 13 - s } else { s };
                        // Corner i as a ccw slot, and the ccw slot of the edge walked next
                        // (prototile edge i, or i - 1 when walking backwards).
                        let k = if flip { (14 - i) % 14 } else { i };
                        let slot = if flip { k } else { i };
                        around_vertex(t, k, 14, &deg, &corner, &partner, &mut around);
                        let v14 = zscale(sc.corner(p, i), 14);
                        around.sort_by(|&x, &y| {
                            let dx = zsub(sc.sum(&places[x as usize]), v14);
                            let dy = zsub(sc.sum(&places[y as usize]), v14);
                            Z12::cmp_angle(dx, dy).then(x.cmp(&y))
                        });
                        for &x in &around {
                            if !tl[t0..].contains(&x) {
                                tl.push(x);
                            }
                        }
                        let q = partner[t * 14 + slot];
                        if q != OPEN {
                            let u = q / 14;
                            if !el[e0..].contains(&u) {
                                el.push(u);
                            }
                            if !tl[t0..].contains(&u) {
                                tl.push(u);
                            }
                        }
                    }
                    tn.push((tl.len() - t0) as u8);
                    en.push((el.len() - e0) as u8);
                }
                (tl, tn, el, en)
            })
            .collect();
        for (tl, tn, el, en) in parts {
            for (csr, list, lens) in [(&mut touch, tl, tn), (&mut edge, el, en)] {
                csr.1.extend_from_slice(&list);
                for k in lens {
                    let last = *csr.0.last().unwrap();
                    csr.0.push(last + k as u32);
                }
            }
        }
    }
    Ok(ExactGraph { touch, edge, open })
}

/// Verify an exact patch with [`crate::exact::verify_patch`]: a tiling of a disc with no gaps or
/// overlaps, exactly.
pub fn verify_exact(places: &[Place]) -> Result<crate::exact::PatchStats, String> {
    let sc = SpectreCorners::new();
    let partner = ccw_pairs(&sc, places)?;
    crate::exact::verify_patch::<Z12>(
        places.len(),
        14,
        |_| 14,
        |t, k| ccw_corner(&sc, places, t, k),
        &partner,
    )
}
