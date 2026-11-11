//! Pattern graphs: canonical forms and dedup for patterns on aperiodic patches.
//!
//! Under an outer-totalistic rule (B/S, Generations, WireWorld, `SpectreGlider`, priority tables)
//! a pattern on any tiling evolves according to only a little of its surroundings. Let `U` be the
//! tiles the pattern uses in any phase. A tile of `U` changes according to the states of its
//! neighbours, which are tiles of `U` or quiet tiles next to `U` (and, for rules that count
//! quiescent neighbours, its neighbour count). So the pattern's whole evolution is fixed by the
//! **pattern graph**: the tiles of `U` with their shapes (and handedness on tilings with a chiral
//! prototile, like the hat), their states in every phase, their neighbour counts when the rule
//! counts quiescent neighbours, and which of them touch. The tiles around only have to stay
//! quiet, which the rule decides from how many pattern tiles (in which states) each one touches;
//! tiles farther away never see a live neighbour.
//!
//! So a record's canonical form is the pattern graph's canonical labelling, found by colour
//! refinement and individualization (the method of graph-isomorphism tools such as nauty, here
//! without automorphism pruning since patterns are small), minimised over the starting phase and,
//! on chiral tilings, over a global mirror image. It is the same up to the patch's rotations and
//! reflections, and the same at every site where the pattern does the same thing, however the
//! tiles farther out are arranged. Pasting finds an embedding of the graph at the target tile
//! (backtracking over neighbours) and checks with the rule that every tile next to it stays
//! quiet in every phase; then the pattern runs there exactly as stored.
//!
//! Movers keep their starting neighbourhood in the key too (which pattern tiles each tile around
//! touches, and its neighbour count when the rule counts quiescent neighbours), since their
//! future leaves the pattern's tiles at once.

use super::site::{Site, SiteTile, interior_point, patch_topology, r6, tile_in_frame};
use crate::geom::P;
use crate::objects::{LCell, Phase};
use crate::rules::Rule;
use crate::topology::{Nb, Topology};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet};

/// A pattern's tiles as a labelled graph (see the module notes).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SiteGraph {
    /// The rule: the tiles around a pasted pattern are checked to stay quiet under it.
    pub rule: String,
    /// Neighbour counts are part of the graph (the rule counts quiescent neighbours).
    pub degrees: bool,
    /// Handedness is part of the graph (a chiral prototile, like the hat).
    pub chirality: bool,
    /// The pattern's tiles in canonical order.
    pub cells: Vec<GraphCell>,
    /// Movers: the tiles around the start, as the sorted pattern tiles each touches (and its
    /// neighbour count when `degrees`), part of the key.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub around: Vec<(Vec<u32>, u32)>,
}

/// One tile of a [`SiteGraph`].
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GraphCell {
    pub shape: u16,
    /// Mirrored relative to the graph's reference handedness (chiral prototiles only).
    #[serde(default, skip_serializing_if = "is_false")]
    pub flip: bool,
    /// Neighbour count (only when the rule counts quiescent neighbours; else 0).
    #[serde(default, skip_serializing_if = "is_zero")]
    pub degree: u32,
    /// State in each phase of one period, phase 0 first.
    pub states: Vec<u8>,
    /// Neighbouring pattern tiles (graph labels, ascending).
    pub nbrs: Vec<u32>,
}

fn is_false(b: &bool) -> bool {
    !*b
}
fn is_zero(x: &u32) -> bool {
    *x == 0
}

/// Does the rule's next state depend on how many quiescent neighbours a cell has?
pub fn counts_quiescent(rule: &Rule) -> bool {
    match rule {
        Rule::SpectreGlider => true,
        Rule::Priority(p) => p.rows.iter().any(|r| r.conds.iter().any(|c| c.0 == 0)),
        _ => false,
    }
}

/// Is the rule outer-totalistic (only the multiset of neighbour states matters) with a quiet
/// background?
fn graph_rule(rule: &Rule) -> bool {
    match rule {
        Rule::LifeLike { birth, .. } | Rule::Generations { birth, .. } => birth & 1 == 0,
        Rule::WireWorld | Rule::SpectreGlider => true,
        Rule::Priority(p) => !(0..=24).any(|d| p.ignites(d)),
        _ => false,
    }
}

/// Does a quiescent cell stay quiescent with these neighbour state counts (`c[0]` = quiescent
/// neighbours that exist)?
fn stays_quiet(rule: &Rule, c: &[u16; 8]) -> bool {
    match rule {
        Rule::LifeLike { birth, .. } | Rule::Generations { birth, .. } => (birth >> c[1]) & 1 == 0,
        Rule::WireWorld => true,
        Rule::SpectreGlider => !(c[0] >= 3 && c[1] >= 1),
        Rule::Priority(p) => p.next(0, c) == 0,
        _ => false,
    }
}

/// Is a prototile chiral (its mirror image is not a rotation of it)?
pub fn chiral(shape: &[P]) -> bool {
    let n = shape.len();
    if n < 3 {
        return false;
    }
    // (edge length, turn) around the polygon; the mirror image walks it backwards.
    let seq = |pts: &[P]| -> Vec<(i64, i64)> {
        (0..n)
            .map(|i| {
                let a = pts[i];
                let b = pts[(i + 1) % n];
                let c = pts[(i + 2) % n];
                let l = (crate::detmath::hypot(b[0] - a[0], b[1] - a[1]) * 1e5).round() as i64;
                let u = [b[0] - a[0], b[1] - a[1]];
                let v = [c[0] - b[0], c[1] - b[1]];
                let t = crate::detmath::atan2(u[0] * v[1] - u[1] * v[0], u[0] * v[0] + u[1] * v[1]);
                (l, (t * 1e5).round() as i64)
            })
            .collect()
    };
    let fwd = seq(shape);
    let mirrored: Vec<P> = shape.iter().rev().map(|p| [-p[0], p[1]]).collect();
    let back = seq(&mirrored);
    !(0..n).any(|k| (0..n).all(|i| fwd[i] == back[(i + k) % n]))
}

fn flipped(topo: &Topology, x: usize) -> bool {
    topo.transform(x).det() < 0.0
}

// ---------------------------------------------------------------------------------------------
// Canonical labelling

/// The graph being labelled: per cell a static label and its states (phase 0 first), adjacency.
struct G {
    label: Vec<[u32; 3]>,
    states: Vec<Vec<u8>>,
    adj: Vec<Vec<usize>>,
    /// Movers: per tile around, the pattern cells it touches and its label.
    around: Vec<(Vec<usize>, u32)>,
}

/// Leaves explored before giving up (very symmetric patterns keep the geometric site).
const MAX_LEAVES: usize = 20_000;

impl G {
    fn n(&self) -> usize {
        self.label.len()
    }

    /// The labels with phase rotation `t0` and global mirror `gflip`.
    fn key(&self, v: usize, t0: usize, gflip: bool) -> Vec<u32> {
        let l = self.label[v];
        let p = self.states[v].len();
        let mut k = vec![l[0], l[1] ^ gflip as u32, l[2]];
        k.extend((0..p).map(|u| self.states[v][(t0 + u) % p] as u32));
        k
    }

    fn refine(&self, colors: &mut [u32]) {
        let n = self.n();
        let mut k_old = distinct(colors);
        loop {
            let sigs: Vec<(u32, Vec<u32>, Vec<Vec<u32>>)> = (0..n)
                .map(|v| {
                    let mut nb: Vec<u32> = self.adj[v].iter().map(|&w| colors[w]).collect();
                    nb.sort_unstable();
                    let mut ar: Vec<Vec<u32>> = self
                        .around
                        .iter()
                        .filter(|(t, _)| t.contains(&v))
                        .map(|(t, l)| {
                            let mut c: Vec<u32> = t.iter().map(|&w| colors[w]).collect();
                            c.sort_unstable();
                            c.push(u32::MAX - l);
                            c
                        })
                        .collect();
                    ar.sort();
                    (colors[v], nb, ar)
                })
                .collect();
            let mut uniq = sigs.clone();
            uniq.sort();
            uniq.dedup();
            for v in 0..n {
                colors[v] = uniq.binary_search(&sigs[v]).unwrap() as u32;
            }
            if uniq.len() == k_old {
                break;
            }
            k_old = uniq.len();
        }
    }

    /// Code of an ordering (position -> cell).
    fn code(&self, order: &[usize], t0: usize, gflip: bool) -> Vec<u32> {
        let n = self.n();
        let mut pos = vec![0u32; n];
        for (i, &v) in order.iter().enumerate() {
            pos[v] = i as u32;
        }
        let mut c = vec![n as u32];
        for &v in order {
            c.extend(self.key(v, t0, gflip));
            let mut nb: Vec<u32> = self.adj[v].iter().map(|&w| pos[w]).collect();
            nb.sort_unstable();
            c.push(u32::MAX);
            c.extend(nb);
        }
        let mut ar: Vec<Vec<u32>> = self
            .around
            .iter()
            .map(|(t, l)| {
                let mut v: Vec<u32> = t.iter().map(|&w| pos[w]).collect();
                v.sort_unstable();
                v.push(u32::MAX - l);
                v
            })
            .collect();
        ar.sort();
        for a in ar {
            c.push(u32::MAX - 1);
            c.extend(a);
        }
        c
    }

    fn search(
        &self,
        mut colors: Vec<u32>,
        t0: usize,
        gflip: bool,
        best: &mut Option<(Vec<u32>, Vec<usize>, usize, bool)>,
        leaves: &mut usize,
    ) {
        if *leaves > MAX_LEAVES {
            return;
        }
        self.refine(&mut colors);
        let n = self.n();
        if distinct(&colors) == n {
            *leaves += 1;
            let mut order: Vec<usize> = (0..n).collect();
            order.sort_by_key(|&v| colors[v]);
            let code = self.code(&order, t0, gflip);
            if best.as_ref().is_none_or(|b| code < b.0) {
                *best = Some((code, order, t0, gflip));
            }
            return;
        }
        // The first colour class with more than one cell.
        let mut count: BTreeMap<u32, usize> = BTreeMap::new();
        for &c in &colors {
            *count.entry(c).or_default() += 1;
        }
        let target = *count.iter().find(|(_, k)| **k > 1).unwrap().0;
        for v in 0..n {
            if colors[v] != target {
                continue;
            }
            let mut c2: Vec<u32> = colors.iter().map(|&c| 2 * c + 1).collect();
            c2[v] -= 1;
            self.search(c2, t0, gflip, best, leaves);
        }
    }

    /// The canonical (code, order, starting phase, mirror), or `None` if too symmetric.
    fn canonical(&self, chirality: bool) -> Option<(Vec<u32>, Vec<usize>, usize, bool)> {
        let p = self.states.first().map(|s| s.len()).unwrap_or(1).max(1);
        let mut best = None;
        let mut leaves = 0;
        for t0 in 0..p {
            for gflip in [false, true] {
                if gflip && !chirality {
                    continue;
                }
                let keys: Vec<Vec<u32>> = (0..self.n()).map(|v| self.key(v, t0, gflip)).collect();
                let mut uniq = keys.clone();
                uniq.sort();
                uniq.dedup();
                let colors: Vec<u32> = keys
                    .iter()
                    .map(|k| uniq.binary_search(k).unwrap() as u32)
                    .collect();
                self.search(colors, t0, gflip, &mut best, &mut leaves);
            }
        }
        (leaves <= MAX_LEAVES).then_some(best).flatten()
    }
}

fn distinct(c: &[u32]) -> usize {
    let mut v = c.to_vec();
    v.sort_unstable();
    v.dedup();
    v.len()
}

/// Tiles next to two or more separate pieces of a pattern (pieces by adjacency among `cells`).
fn bridges(topo: &Topology, nb: Nb, cells: &[usize]) -> Vec<usize> {
    let index: HashMap<usize, usize> = cells.iter().enumerate().map(|(k, &x)| (x, k)).collect();
    let n = cells.len();
    let mut comp = vec![usize::MAX; n];
    let mut k = 0;
    for s in 0..n {
        if comp[s] != usize::MAX {
            continue;
        }
        comp[s] = k;
        let mut stack = vec![s];
        while let Some(v) = stack.pop() {
            topo.for_each_neighbor(cells[v], nb, |y| {
                if let Some(&w) = index.get(&y)
                    && comp[w] == usize::MAX
                {
                    comp[w] = k;
                    stack.push(w);
                }
            });
        }
        k += 1;
    }
    if k < 2 {
        return vec![];
    }
    let mut touch: BTreeMap<usize, HashSet<usize>> = BTreeMap::new();
    for (v, &x) in cells.iter().enumerate() {
        topo.for_each_neighbor(x, nb, |y| {
            if !index.contains_key(&y) {
                touch.entry(y).or_default().insert(comp[v]);
            }
        });
    }
    touch
        .into_iter()
        .filter(|(_, c)| c.len() >= 2)
        .map(|(y, _)| y)
        .collect()
}

fn connected(adj: &[Vec<usize>]) -> bool {
    let n = adj.len();
    if n == 0 {
        return false;
    }
    let mut seen = vec![false; n];
    let mut stack = vec![0];
    seen[0] = true;
    while let Some(v) = stack.pop() {
        for &w in &adj[v] {
            if !seen[w] {
                seen[w] = true;
                stack.push(w);
            }
        }
    }
    seen.into_iter().all(|s| s)
}

/// Build the site of a pattern on an explicit patch for `rule` (phases: one period, cell ids in
/// `i`). Outer-totalistic rules with a quiet background get a pattern graph; rule tables,
/// patterns whose tiles are not connected, and very large or very symmetric patterns keep the
/// geometric site ([`super::site::build_site`]). `mover`: keep the starting neighbourhood in the
/// key. Returns the site, the phases as site-tile indices (canonical start), and the canonical
/// text.
pub fn build_site_for(
    topo: &Topology,
    nb: Nb,
    rule: &Rule,
    phases: &[Phase],
) -> Result<(Site, Vec<Phase>, String), String> {
    build_graph_site(topo, nb, rule, phases, false)
}

/// [`build_site_for`] for a mover's starting configuration.
pub fn build_mover_site(
    topo: &Topology,
    nb: Nb,
    rule: &Rule,
    start: &Phase,
) -> Result<(Site, Vec<Phase>, String), String> {
    build_graph_site(topo, nb, rule, std::slice::from_ref(start), true)
}

fn build_graph_site(
    topo: &Topology,
    nb: Nb,
    rule: &Rule,
    phases: &[Phase],
    mover: bool,
) -> Result<(Site, Vec<Phase>, String), String> {
    if topo.lattice().is_some() {
        return Err("sites are for aperiodic patches".into());
    }
    let mut cells: Vec<usize> = phases.iter().flatten().map(|c| c.i as usize).collect();
    cells.sort_unstable();
    cells.dedup();
    if !graph_rule(rule) || cells.is_empty() || cells.len() > 200 {
        return super::site::build_site(topo, nb, phases);
    }
    // Pieces that hold each other in place through a quiet tile they both touch: that tile
    // joins the graph (a quiet "bridge", state 0 in every phase).
    cells.extend(bridges(topo, nb, &cells));
    let index: HashMap<usize, usize> = cells.iter().enumerate().map(|(k, &x)| (x, k)).collect();
    let degrees = counts_quiescent(rule);
    let chirality = topo.shapes.iter().any(|s| chiral(s));
    let p = phases.len();
    let mut states = vec![vec![0u8; p]; cells.len()];
    for (t, ph) in phases.iter().enumerate() {
        for c in ph {
            states[index[&(c.i as usize)]][t] = c.s;
        }
    }
    let mut adj = vec![vec![]; cells.len()];
    let mut ring: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
    for (k, &x) in cells.iter().enumerate() {
        if topo.is_open(x) {
            return Err("the pattern is too close to the patch's open boundary".into());
        }
        topo.for_each_neighbor(x, nb, |y| match index.get(&y) {
            Some(&j) => adj[k].push(j),
            None => ring.entry(y).or_default().push(k),
        });
    }
    if ring.keys().any(|&y| topo.is_open(y)) {
        return Err("the pattern is too close to the patch's open boundary".into());
    }
    if !connected(&adj) {
        return super::site::build_site(topo, nb, phases);
    }
    let around: Vec<(Vec<usize>, u32)> = if mover {
        ring.iter()
            .map(|(&y, t)| {
                (
                    t.clone(),
                    if degrees {
                        topo.degree(y, nb) as u32
                    } else {
                        0
                    },
                )
            })
            .collect()
    } else {
        vec![]
    };
    let g = G {
        label: cells
            .iter()
            .map(|&x| {
                [
                    topo.shape_of(x) as u32,
                    (chirality && flipped(topo, x)) as u32,
                    if degrees {
                        topo.degree(x, nb) as u32
                    } else {
                        0
                    },
                ]
            })
            .collect(),
        states,
        adj,
        around,
    };
    let Some((code, order, t0, gflip)) = g.canonical(chirality) else {
        return super::site::build_site(topo, nb, phases);
    };
    let mut pos = vec![0u32; cells.len()];
    for (i, &v) in order.iter().enumerate() {
        pos[v] = i as u32;
    }
    let gcells: Vec<GraphCell> = order
        .iter()
        .map(|&v| {
            let mut nbrs: Vec<u32> = g.adj[v].iter().map(|&w| pos[w]).collect();
            nbrs.sort_unstable();
            GraphCell {
                shape: g.label[v][0] as u16,
                flip: (g.label[v][1] == 1) ^ gflip,
                degree: g.label[v][2],
                states: (0..p).map(|u| g.states[v][(t0 + u) % p]).collect(),
                nbrs,
            }
        })
        .collect();
    let mut around: Vec<(Vec<u32>, u32)> = g
        .around
        .iter()
        .map(|(t, l)| {
            let mut v: Vec<u32> = t.iter().map(|&w| pos[w]).collect();
            v.sort_unstable();
            (v, *l)
        })
        .collect();
    around.sort();
    let out: Vec<Phase> = (0..p)
        .map(|u| {
            let mut v: Phase = phases[(t0 + u) % p]
                .iter()
                .map(|c| LCell {
                    i: pos[index[&(c.i as usize)]] as i32,
                    j: 0,
                    k: 0,
                    s: c.s,
                })
                .collect();
            v.sort_unstable();
            v
        })
        .collect();
    // Geometry for drawing: the pattern's tiles in label order, then the tiles around.
    let anchor = cells[order[0]];
    let inv = topo.transform(anchor).inv();
    let mut tiles: Vec<SiteTile> = order
        .iter()
        .map(|&v| {
            let (corners, point) = tile_in_frame(topo, cells[v], &inv);
            SiteTile {
                shape: topo.shape_of(cells[v]) as u16,
                dist: 0,
                point,
                corners,
            }
        })
        .collect();
    let mut rest: Vec<SiteTile> = ring
        .keys()
        .map(|&y| {
            let (corners, point) = tile_in_frame(topo, y, &inv);
            SiteTile {
                shape: topo.shape_of(y) as u16,
                dist: 1,
                point,
                corners,
            }
        })
        .collect();
    rest.sort_by(|a, b| a.point.partial_cmp(&b.point).unwrap());
    tiles.extend(rest);
    let canonical = graph_canonical_text(&code, mover, degrees, chirality);
    let site = Site {
        patch: topo.spec.clone(),
        anchor: anchor as u32,
        anchor_shape: topo.shape_of(anchor) as u16,
        anchor_point: r6(interior_point(&topo.polygon(anchor))),
        radius: 1,
        tiles,
        graph: Some(Box::new(SiteGraph {
            rule: rule.notation(),
            degrees,
            chirality,
            cells: gcells,
            around,
        })),
        nb: Some(nb),
    };
    Ok((site, out, canonical))
}

/// A pattern graph's canonical form: `graph`, the flags (` start` for a mover's starting
/// configuration, ` degrees`, ` chiral`), and 32 hex digits of SHA-256 of its canonical code
/// (the code's numbers as little-endian `u32`s).
pub(crate) fn graph_canonical_text(
    code: &[u32],
    mover: bool,
    degrees: bool,
    chirality: bool,
) -> String {
    let mut h = crate::sha256::Sha256::new();
    for x in code {
        h.update(&x.to_le_bytes());
    }
    format!(
        "graph{}{}{} {}",
        if mover { " start" } else { "" },
        if degrees { " degrees" } else { "" },
        if chirality { " chiral" } else { "" },
        &h.finish_hex()[..32]
    )
}

/// The canonical labelling of a stored pattern graph, from the graph alone (no patch): the
/// canonical code, the order (canonical position -> stored cell), the starting phase, and
/// whether the global mirror was taken. The same as building the graph from the patch gives,
/// since the stored graph is the patch's graph relabelled, its phases rotated, and (on chiral
/// tilings) perhaps mirrored. `None` when the graph is too symmetric to label (such patterns
/// keep a geometric site, so a stored graph never is).
pub(crate) fn canonical_of(g: &SiteGraph) -> Option<(Vec<u32>, Vec<usize>, usize, bool)> {
    let p = g.cells.first().map(|c| c.states.len()).unwrap_or(1).max(1);
    if g.cells.iter().any(|c| c.states.len() != p) {
        return None;
    }
    let gr = G {
        label: g
            .cells
            .iter()
            .map(|c| [c.shape as u32, c.flip as u32, c.degree])
            .collect(),
        states: g.cells.iter().map(|c| c.states.clone()).collect(),
        adj: g
            .cells
            .iter()
            .map(|c| c.nbrs.iter().map(|&w| w as usize).collect())
            .collect(),
        around: g
            .around
            .iter()
            .map(|(t, l)| (t.iter().map(|&w| w as usize).collect(), *l))
            .collect(),
    };
    if gr.adj.iter().flatten().any(|&w| w >= gr.n())
        || gr
            .around
            .iter()
            .any(|(t, _)| t.iter().any(|&w| w >= gr.n()))
    {
        return None;
    }
    gr.canonical(g.chirality)
}

// ---------------------------------------------------------------------------------------------
// Matching

/// Map a pattern graph onto `topo` with its first cell on `target`; every tile next to it must
/// stay quiet under the rule in every phase.
pub(crate) fn match_graph(
    topo: &Topology,
    site: &Site,
    g: &SiteGraph,
    target: usize,
) -> Result<Vec<u32>, String> {
    if topo.lattice().is_some() {
        return Err(
            "this pattern was found on an aperiodic patch; it can only be pasted on one".into(),
        );
    }
    if target >= topo.len() {
        return Err(format!(
            "no cell {target} (the patch has {} cells)",
            topo.len()
        ));
    }
    if g.cells.is_empty() {
        return Err("empty pattern graph".into());
    }
    if topo.shape_of(target) as u16 != g.cells[0].shape {
        return Err(format!(
            "cell {target} has tile shape {}, the pattern's anchor tile has shape {}",
            topo.shape_of(target),
            g.cells[0].shape
        ));
    }
    let rule = Rule::parse(&g.rule)?.0;
    let nb = site.nb.unwrap_or(Nb::Touch);
    let fail = |why: &str| {
        format!(
            "the tiles around cell {target} do not match the pattern's surroundings ({why}). Paste it where the same pattern graph fits, such as its original site (cell {} on {} level {:?})",
            site.anchor, site.patch.family, site.patch.size
        )
    };
    let m = Matcher::new(topo, nb, g, &rule);
    let mut why = String::from("no embedding");
    for gflip in [false, true] {
        if gflip && !g.chirality {
            continue;
        }
        match m.run(target, gflip) {
            Ok(map) => return Ok(map.into_iter().map(|x| x as u32).collect()),
            Err(e) => why = e,
        }
    }
    Err(fail(&why))
}

struct Matcher<'a> {
    topo: &'a Topology,
    nb: Nb,
    g: &'a SiteGraph,
    rule: &'a Rule,
    /// Matching order (BFS over the graph from cell 0) and each cell's parent in it.
    order: Vec<usize>,
    parent: Vec<usize>,
}

impl<'a> Matcher<'a> {
    fn new(topo: &'a Topology, nb: Nb, g: &'a SiteGraph, rule: &'a Rule) -> Matcher<'a> {
        let n = g.cells.len();
        let mut order = vec![0];
        let mut parent = vec![usize::MAX; n];
        let mut seen = vec![false; n];
        seen[0] = true;
        let mut k = 0;
        while k < order.len() {
            let v = order[k];
            for &w in &g.cells[v].nbrs {
                let w = w as usize;
                if w < n && !seen[w] {
                    seen[w] = true;
                    parent[w] = v;
                    order.push(w);
                }
            }
            k += 1;
        }
        Matcher {
            topo,
            nb,
            g,
            rule,
            order,
            parent,
        }
    }

    fn fits(&self, v: usize, x: usize, gflip: bool) -> bool {
        let c = &self.g.cells[v];
        self.topo.shape_of(x) as u16 == c.shape
            && !self.topo.is_open(x)
            && (!self.g.chirality || (flipped(self.topo, x) ^ gflip) == c.flip)
            && (!self.g.degrees || self.topo.degree(x, self.nb) as u32 == c.degree)
    }

    fn run(&self, target: usize, gflip: bool) -> Result<Vec<usize>, String> {
        let n = self.g.cells.len();
        if self.order.len() != n {
            return Err("the stored graph is not connected".into());
        }
        if !self.fits(0, target, gflip) {
            return Err("the anchor tile differs".into());
        }
        let mut map = vec![usize::MAX; n];
        let mut used: HashSet<usize> = HashSet::new();
        map[0] = target;
        used.insert(target);
        let mut last = String::from("no embedding");
        if self.extend(1, gflip, &mut map, &mut used, &mut last) {
            Ok(map)
        } else {
            Err(last)
        }
    }

    fn adjacent(&self, x: usize, y: usize) -> bool {
        let mut a = false;
        self.topo.for_each_neighbor(x, self.nb, |z| a |= z == y);
        a
    }

    fn extend(
        &self,
        k: usize,
        gflip: bool,
        map: &mut Vec<usize>,
        used: &mut HashSet<usize>,
        last: &mut String,
    ) -> bool {
        if k == self.order.len() {
            return match self.quiet_around(map) {
                Ok(()) => true,
                Err(e) => {
                    *last = e;
                    false
                }
            };
        }
        let v = self.order[k];
        let px = map[self.parent[v]];
        let mut cands = Vec::new();
        self.topo.for_each_neighbor(px, self.nb, |y| cands.push(y));
        cands.sort_unstable();
        'next: for y in cands {
            if used.contains(&y) || !self.fits(v, y, gflip) {
                continue;
            }
            // Adjacency to every cell placed so far must match the graph.
            for &u in &self.order[..k] {
                let should = self.g.cells[v].nbrs.contains(&(u as u32));
                if should != self.adjacent(y, map[u]) {
                    continue 'next;
                }
            }
            map[v] = y;
            used.insert(y);
            if self.extend(k + 1, gflip, map, used, last) {
                return true;
            }
            used.remove(&y);
            map[v] = usize::MAX;
        }
        if last.as_str() == "no embedding" {
            *last = "the pattern's tiles do not fit around this tile".into();
        }
        false
    }

    /// Does quiet tile `y`, touching these graph cells, stay quiet in every phase?
    fn quiet(&self, y: usize, touches: &[usize], p: usize) -> bool {
        let deg = self.topo.degree(y, self.nb) as u16;
        (0..p).all(|t| {
            let mut c = [0u16; 8];
            for &v in touches {
                c[self.g.cells[v].states[t].min(7) as usize] += 1;
            }
            c[0] += deg.saturating_sub(touches.len() as u16);
            stays_quiet(self.rule, &c)
        })
    }

    /// Every tile next to the placed pattern stays quiet in every phase; with `around`, the
    /// tiles around are exactly the stored ones.
    fn quiet_around(&self, map: &[usize]) -> Result<(), String> {
        let topo = self.topo;
        let at: HashMap<usize, usize> = map.iter().enumerate().map(|(v, &x)| (x, v)).collect();
        // Every pattern tile's neighbours outside the pattern, with the pattern tiles they touch.
        let mut ring: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
        for (v, &x) in map.iter().enumerate() {
            topo.for_each_neighbor(x, self.nb, |y| {
                if !at.contains_key(&y) {
                    ring.entry(y).or_default().push(v);
                }
            });
        }
        let p = self.g.cells[0].states.len().max(1);
        // A mover's start is not periodic: its neighbourhood must match instead.
        let mover = !self.g.around.is_empty();
        // Quiet tiles of the graph (bridges) must stay quiet too.
        if !mover {
            for (v, &x) in map.iter().enumerate() {
                if self.g.cells[v].states.iter().any(|&s| s != 0) {
                    continue;
                }
                let mut touches = Vec::new();
                topo.for_each_neighbor(x, self.nb, |y| {
                    if let Some(&w) = at.get(&y) {
                        touches.push(w);
                    }
                });
                if !self.quiet(x, &touches, p) {
                    return Err("a quiet tile between the pattern's pieces would come alive".into());
                }
            }
        }
        for (&y, touches) in &ring {
            if topo.is_open(y) {
                return Err("a tile next to the pattern lies on the patch's open boundary".into());
            }
            if mover {
                continue;
            }
            if !self.quiet(y, touches, p) {
                return Err("a tile next to the pattern would come alive".into());
            }
        }
        if mover {
            let mut a: Vec<(Vec<u32>, u32)> = ring
                .iter()
                .map(|(&y, t)| {
                    let mut v: Vec<u32> = t.iter().map(|&w| w as u32).collect();
                    v.sort_unstable();
                    (
                        v,
                        if self.g.degrees {
                            topo.degree(y, self.nb) as u32
                        } else {
                            0
                        },
                    )
                })
                .collect();
            a.sort();
            if a != self.g.around {
                return Err("the tiles around the start differ".into());
            }
        }
        Ok(())
    }
}

/// Up to `limit` other tiles of `topo` (lowest ids first; sharing no tile with the original site
/// or each other) where a graph site matches, with their tile maps.
pub fn other_sites(topo: &Topology, site: &Site, limit: usize) -> Vec<(usize, Vec<u32>)> {
    let mut out = Vec::new();
    if limit == 0 || site.graph.is_none() {
        return out;
    }
    let mut used: HashSet<u32> = super::site::match_site(topo, site, site.anchor as usize)
        .map(|m| m.into_iter().collect())
        .unwrap_or_default();
    for x in 0..topo.len() {
        if out.len() >= limit {
            break;
        }
        if used.contains(&(x as u32)) || topo.shape_of(x) as u16 != site.anchor_shape {
            continue;
        }
        if let Ok(m) = super::site::match_site(topo, site, x)
            && !m.iter().any(|t| used.contains(t))
        {
            used.extend(m.iter().copied());
            out.push((x, m));
        }
    }
    out
}

/// Every tile of a patch where a graph site matches (for counting and tests).
pub fn all_sites(spec: &crate::topology::TilingSpec, site: &Site) -> Result<Vec<usize>, String> {
    let topo = patch_topology(spec)?;
    Ok((0..topo.len())
        .filter(|&x| super::site::match_site(&topo, site, x).is_ok())
        .collect())
}
