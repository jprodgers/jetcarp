//! HashLife: Gosper's hashed quadtree (Gosper 1984, "Exploiting regularities in large cellular
//! spaces"), written for JETCARP from the published algorithm.
//!
//! - **Macrocells.** A node of level `k` is a `2^k × 2^k` block of lattice sites made of four
//!   level `k-1` children. Nodes are canonicalised in a hash table, so equal blocks anywhere in
//!   space or time are one node. Level 0 nodes are single sites.
//! - **RESULT.** For a node of level `k ≥ 2`, `next(node, j)` is its centre (level `k-1`) after
//!   `2^min(j, k-2)` generations, memoised on the node. With `j ≥ k-2` both halves of the
//!   recursion advance (Gosper's full speed); with a smaller `j` only the first half does, which
//!   gives any power-of-two step and so any generation count (one jump per set bit).
//! - **Lattices.** A site is one unit cell of a periodic tiling: the square grid (Moore or von
//!   Neumann), the hexagon grid as a sheared lattice, and every multi-tile-type tiling, whose `k`
//!   tiles per unit cell are packed into the site state (`state_0 + n·state_1 + n²·state_2…`).
//!   Every JETCARP lattice has neighbour offsets within one unit cell, which is what the light-speed
//!   argument needs.
//! - **Rules.** Anything whose next state depends only on the cell's state and the number of
//!   neighbours in state 1: Life-like B/S and Generations rules (B0 included), WireWorld, and
//!   Greenberg–Hastings. Live and dead edges are not supported (not translation invariant).
//! - **B0 and step sequences.** A field's generation is a step sequence (each enabled CA on it,
//!   in list order, for its steps per generation). Its *phases* follow the far background, which
//!   B0 rules change uniformly every step (alternating without S_max, full for good with it, per
//!   tile type on multi-type tilings): stored states are real ones with 0 and the background
//!   swapped, so empty space stays empty in every phase, and the memo is keyed by phase too
//!   ([`LatticeRule`]). A world's Life-like B0 field is already kept that way by the stepper's
//!   emulation, so it is read and written as stored states at the CA's step count.
//! - **Several CAs.** A CA only reads and writes its own field, so each field is its own
//!   universe: separate fields step independently in lockstep, CAs sharing a field are one step
//!   sequence ([`LatticeRule::for_world_fields`]). Rules HashLife cannot run are refused by name.
//! - **Bit-parallel leaf.** Nodes up to [`Options::leaf_level`] (default 6, 64 × 64 sites) of
//!   B/S and Generations rules without B0 are stepped with the hunter's bit-sliced stepper
//!   instead of recursing: on a fresh 1024² soup that is 2.6 to 3.7 times faster and needs 14
//!   times fewer nodes.
//! - **Two modes.** [`Plane`]: the unbounded plane, the natural HashLife setting; import cells
//!   from a world, a scene, a library record, or RLE, jump any number of generations, export a
//!   region into a JETCARP world or render a PNG with a density renderer for huge extents.
//!   [`jump_world`] (and [`Command::Jump`](crate::world::Command::Jump)): a wrap-edge world whose
//!   sides make a power-of-two torus, advanced in place, equal to stepping it (states only).
//! - **Ages are not tracked.** After a jump every nonzero cell has age 1 (as `Field::set` gives a
//!   painted cell; Generations' dying states and WireWorld cells too), so compare results with
//!   [`World::states_hash`], not `state_hash`. Exact ages would need every cell's history through
//!   the jump, which the memoised quadtree does not keep (its results are shared between all
//!   places and times with the same contents).
//! - **Memory.** The node store has a cap ([`Options::node_cap`]). When it is reached, a mark and
//!   sweep collection keeps the nodes reachable from the current computation (an explicit stack
//!   of pinned nodes), with the memoised results of those nodes when that frees at least half the
//!   cap (otherwise the memo is cleared). Collection never changes results.

use crate::render::{Image, Palette, Rgba};
use crate::rules::{FieldKind, Rule};
use crate::topology::{Boundary, LatticeGrid, Nb, SizeSpec, Storage, TilingSpec, Topology};
use crate::world::{CaSpec, Command, Scene, SeedSpec, World};
use std::collections::HashMap;

/// Default node cap: about 2 GB of nodes and table.
pub const DEFAULT_NODE_CAP: usize = 48_000_000;
/// Largest number of distinct site states (states ^ tile types).
pub const MAX_SITE_STATES: u64 = 1 << 16;
/// Largest jump (and plane extent stays far inside i128).
pub const MAX_GENS: u64 = 1 << 62;
/// Default [`Options::leaf_level`]: nodes up to 2^LEAF × 2^LEAF sites are stepped bit-parallel.
pub const DEFAULT_LEAF_LEVEL: u8 = 6;
/// Largest leaf level (a row of sites fits in a 64-bit word).
pub const MAX_LEAF_LEVEL: u8 = 6;

#[derive(Clone, Copy, Debug)]
pub struct Options {
    /// Live nodes before a garbage collection.
    pub node_cap: usize,
    /// Largest node level computed by the bit-parallel leaf (B/S and Generations rules without
    /// B0); 0 turns it off. Results never depend on it.
    pub leaf_level: u8,
}
impl Default for Options {
    fn default() -> Self {
        Options {
            node_cap: DEFAULT_NODE_CAP,
            leaf_level: DEFAULT_LEAF_LEVEL,
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Rules on a lattice

/// One rule applied for one step: neighbour offsets per tile type and the table from (state,
/// neighbours in state 1) to the next state.
#[derive(Clone, Debug)]
struct StepRule {
    /// Per tile type: neighbour offsets `(di, dj, k)` in unit cells (each within -1..=1).
    offsets: Vec<Vec<(i32, i32, u32)>>,
    /// `table[s * (maxn + 1) + c]`.
    table: Vec<u8>,
    maxn: usize,
    /// `(birth, survive, states)` for B/S and Generations rules without B0 (the bit-parallel
    /// leaf).
    bs: Option<(u32, u32, u8)>,
    /// A Life-like B0 rule: the world's stepper stores it relative to its background (see
    /// `step_b0`), so a world's field holds stored states, not real ones.
    b0_life: bool,
}

impl StepRule {
    fn new(rule: &Rule, nb: Nb, g: &LatticeGrid) -> Result<(StepRule, u8), String> {
        let offsets: Vec<Vec<(i32, i32, u32)>> = g.offsets[nb as usize]
            .iter()
            .map(|t| t.iter().map(|o| (o.di, o.dj, o.k)).collect())
            .collect();
        if offsets
            .iter()
            .flatten()
            .any(|o| o.0.abs() > 1 || o.1.abs() > 1)
        {
            return Err("HashLife needs neighbours within one unit cell".into());
        }
        let maxn = offsets.iter().map(|t| t.len()).max().unwrap_or(0);
        let b0_life = matches!(rule, Rule::LifeLike { birth, .. } if birth & 1 == 1);
        if b0_life && offsets.iter().any(|t| t.len() != maxn) {
            return Err(
                "HashLife runs B0 Life-like rules only where every tile has the same number of \
                 neighbours (the stepper's alternating-background emulation assumes one degree)"
                    .into(),
            );
        }
        let states: u8 = match rule {
            Rule::LifeLike { .. } => 2,
            Rule::Generations { states, .. } | Rule::GreenbergHastings { states, .. } => *states,
            Rule::WireWorld => 4,
            _ => {
                return Err(format!(
                    "HashLife runs Life-like, Generations, WireWorld, and Greenberg-Hastings \
                     rules, not {}",
                    rule.notation()
                ));
            }
        };
        let mut table = vec![0u8; states as usize * (maxn + 1)];
        for s in 0..states {
            for c in 0..=maxn as u32 {
                table[s as usize * (maxn + 1) + c as usize] = match rule {
                    Rule::LifeLike { birth, survive }
                    | Rule::Generations { birth, survive, .. } => {
                        // As `step_bs`.
                        if s == 0 {
                            ((birth >> c) & 1) as u8
                        } else if s == 1 {
                            if (survive >> c) & 1 == 1 {
                                1
                            } else if states > 2 {
                                2
                            } else {
                                0
                            }
                        } else if s + 1 < states {
                            s + 1
                        } else {
                            0
                        }
                    }
                    // As `step_wireworld`.
                    Rule::WireWorld => match s {
                        1 => 2,
                        2 => 3,
                        3 => {
                            if c == 1 || c == 2 {
                                1
                            } else {
                                3
                            }
                        }
                        _ => 0,
                    },
                    // As `step_gh`.
                    Rule::GreenbergHastings { threshold, .. } => {
                        if s == 0 {
                            (c >= *threshold as u32) as u8
                        } else if s + 1 >= states {
                            0
                        } else {
                            s + 1
                        }
                    }
                    _ => unreachable!(),
                };
            }
        }
        let bs = match *rule {
            Rule::LifeLike { birth, survive } if birth & 1 == 0 => Some((birth, survive, 2)),
            Rule::Generations {
                birth,
                survive,
                states,
            } if birth & 1 == 0 => Some((birth, survive, states)),
            _ => None,
        };
        Ok((
            StepRule {
                offsets,
                table,
                maxn,
                bs,
                b0_life,
            },
            states,
        ))
    }

    #[inline]
    fn apply(&self, s: u8, c: usize) -> u8 {
        self.table[s as usize * (self.maxn + 1) + c]
    }
}

/// A phase of the step sequence: which rule runs, and how stored states relate to real ones.
///
/// HashLife needs empty space to stay empty. With B0 rules it does not: the far background
/// changes, uniformly, every step. So each phase records the real background state of every
/// tile type, `background[k]`, and the stored state of a tile is its real state with 0 and the
/// background swapped (`σ_b`, its own inverse). Far away the stored plane is then always empty,
/// and each phase's stored rule (`tables`) maps an empty neighbourhood to empty. A real state-1
/// neighbour of type `k` is a stored `σ_b(1)` (`counted[k]`), so the stored rule is still a
/// table of (state, count). This is the standard alternating-background trick, generalised to
/// any background cycle (B0 without S_max alternates; with S_max the background stays full).
#[derive(Clone, Debug)]
struct Phase {
    rule: usize,
    counted: Vec<u8>,
    /// Per tile type: `tables[k][s * (maxn + 1) + c]`, stored to stored.
    tables: Vec<Vec<u8>>,
    maxn: usize,
    background: Vec<u8>,
    next: u16,
}

/// `σ_b`: swap 0 and `b`.
#[inline]
fn swap0(b: u8, x: u8) -> u8 {
    if x == 0 {
        b
    } else if x == b {
        0
    } else {
        x
    }
}

/// Most phases a step sequence may have (and raw sequence states explored to find them).
const MAX_PHASES: usize = 4096;
const MAX_RAW_PHASES: usize = 1 << 21;

/// One field of a world as HashLife runs it ([`LatticeRule::for_world_fields`]).
#[derive(Clone, Debug)]
pub struct FieldProgram {
    pub field: usize,
    pub rule: LatticeRule,
    /// The phase the field's states are in now.
    pub start: u16,
    /// Whether the world keeps this field as stored states (a Life-like B0 rule's emulation)
    /// rather than real ones.
    pub stored: bool,
    /// The index of the Life-like B0 CA, whose step counter (`ticks`) a jump advances.
    pub b0_ca: Option<usize>,
}

/// What HashLife runs on one field of a lattice: the step sequence of one generation (each
/// enabled CA on the field, in list order, for its steps per generation), its phases, and the
/// site packing (a unit cell's `k` tiles in one site state, `state_0 + n·state_1 + …`).
#[derive(Clone, Debug)]
pub struct LatticeRule {
    pub ntypes: u32,
    pub states: u8,
    /// Number of site states, `states ^ ntypes`.
    pub site_states: u32,
    /// Steps in one generation (0 for a field no enabled CA steps).
    pub steps_per_gen: u64,
    steps: Vec<StepRule>,
    phases: Vec<Phase>,
    /// Phases before the cycle (the cycle is `tail..phases.len()`).
    tail: usize,
    /// `after2[e][p]`: the phase `2^e` steps after phase `p`.
    after2: Vec<Vec<u16>>,
    /// Site state → per-type states.
    digits: Vec<u8>,
    pow: Vec<u32>,
    /// The bit-parallel leaf (one phase, a B/S or Generations rule without B0).
    leaf: Option<std::sync::Arc<crate::hunt::bits::Plan>>,
}

impl LatticeRule {
    /// The HashLife form of `rule` with neighbourhood `nb` on lattice `g` (one step per
    /// generation).
    pub fn new(rule: &Rule, nb: Nb, g: &LatticeGrid) -> Result<LatticeRule, String> {
        LatticeRule::program(g, &[(rule.clone(), nb, 1)])
    }

    /// The HashLife form of a step sequence: per generation, each `(rule, nb, steps)` in order.
    /// Every rule must have the same number of states (they share a field).
    pub fn program(g: &LatticeGrid, seq: &[(Rule, Nb, u32)]) -> Result<LatticeRule, String> {
        LatticeRule::build(g, seq, None)
    }

    /// As [`LatticeRule::program`]; an empty sequence is a field that never changes, with
    /// `static_states` states.
    fn build(
        g: &LatticeGrid,
        seq: &[(Rule, Nb, u32)],
        static_states: Option<u8>,
    ) -> Result<LatticeRule, String> {
        let mut steps = vec![];
        let mut states = static_states;
        for (rule, nb, _) in seq {
            let (r, s) = StepRule::new(rule, *nb, g)?;
            if let Some(s0) = states
                && s0 != s
            {
                return Err(format!(
                    "CAs sharing a field need the same number of states ({s0} and {s})"
                ));
            }
            states = Some(s);
            steps.push(r);
        }
        let states = states.ok_or("no CA to run")?;
        let nt = g.ntypes as usize;
        let n = (states as u64).checked_pow(g.ntypes).unwrap_or(u64::MAX);
        if n > MAX_SITE_STATES {
            return Err(format!(
                "{} states on {} tile types per unit cell is {} site states; HashLife allows {}",
                states, g.ntypes, n, MAX_SITE_STATES
            ));
        }
        let pow: Vec<u32> = (0..g.ntypes).map(|k| (states as u32).pow(k)).collect();
        let mut digits = vec![0u8; n as usize * nt];
        for v in 0..n as u32 {
            for k in 0..g.ntypes {
                digits[(v * g.ntypes + k) as usize] = ((v / pow[k as usize]) % states as u32) as u8;
            }
        }
        // The per-generation sequence of rule indices.
        let mut order: Vec<usize> = vec![];
        for (i, (_, _, reps)) in seq.iter().enumerate() {
            if order.len() + *reps as usize > MAX_RAW_PHASES {
                return Err("too many steps per generation for HashLife".into());
            }
            order.extend(std::iter::repeat_n(i, *reps as usize));
        }
        let steps_per_gen = order.len() as u64;
        let (phases, tail) = if order.is_empty() {
            (
                vec![Phase {
                    rule: usize::MAX,
                    counted: vec![1; nt],
                    tables: vec![],
                    maxn: 0,
                    background: vec![0; nt],
                    next: 0,
                }],
                0,
            )
        } else {
            phases_of(&steps, &order, nt, states)?
        };
        let np = phases.len();
        let mut after2 = vec![phases.iter().map(|p| p.next).collect::<Vec<u16>>()];
        for e in 1..64 {
            let prev: &Vec<u16> = &after2[e - 1];
            let v: Vec<u16> = (0..np).map(|p| prev[prev[p] as usize]).collect();
            after2.push(v);
        }
        let leaf = match (np, steps.as_slice()) {
            (1, [r]) => r.bs.and_then(|(b, s, c)| {
                crate::hunt::bits::Plan::from_offsets(r.offsets.clone(), b, s, c)
                    .ok()
                    .map(std::sync::Arc::new)
            }),
            _ => None,
        };
        Ok(LatticeRule {
            ntypes: g.ntypes,
            states,
            site_states: n as u32,
            steps_per_gen,
            steps,
            phases,
            tail,
            after2,
            digits,
            pow,
            leaf,
        })
    }

    /// The HashLife programs of a world: one per field its CAs use, in the order of each
    /// field's first CA. A field whose CAs are all switched off gets an empty program (it never
    /// changes). Several CAs are well defined here because a CA only ever reads and writes its
    /// own field: CAs on separate fields step independently in lockstep, and CAs sharing a field
    /// take turns in list order, which is a periodic step sequence. Every CA, switched off or
    /// not, must have a rule HashLife runs. A Life-like B0 rule needs its field to itself: the
    /// stepper keeps that field relative to the rule's own background, which another CA on the
    /// field would read as real states.
    pub fn for_world_fields(w: &World) -> Result<Vec<FieldProgram>, String> {
        let g = w.topo.lattice().ok_or(
            "HashLife needs a periodic lattice tiling, not an aperiodic patch".to_string(),
        )?;
        if w.cas.is_empty() {
            return Err("the world has no CA".into());
        }
        let mut fields: Vec<usize> = vec![];
        for ca in &w.cas {
            if !fields.contains(&ca.field) {
                fields.push(ca.field);
            }
        }
        let many = w.cas.len() > 1;
        let mut out = vec![];
        for fi in fields {
            let mut seq = vec![];
            let mut states = None;
            let mut b0 = None;
            let users = w.cas.iter().filter(|c| c.field == fi).count();
            for (ci, ca) in w.cas.iter().enumerate().filter(|c| c.1.field == fi) {
                let who = if many {
                    format!("CA {} ({}): ", ca.id, ca.name)
                } else {
                    String::new()
                };
                if ca.rule.field_kind() != w.fields[ca.field].kind {
                    return Err(format!("{who}the CA's field does not match its rule"));
                }
                let (r, s) = StepRule::new(&ca.rule, ca.nb, g).map_err(|e| format!("{who}{e}"))?;
                states = Some(s);
                if r.b0_life {
                    if users > 1 {
                        return Err(format!(
                            "{who}a B0 Life-like rule needs its field to itself (make it \
                             private): the stepper keeps that field relative to the rule's \
                             alternating background"
                        ));
                    }
                    b0 = Some((ci, ca.ticks));
                }
                if ca.enabled {
                    seq.push((ca.rule.clone(), ca.nb, ca.steps_per_gen));
                }
            }
            let prog = LatticeRule::build(g, &seq, states)
                .map_err(|e| if many { format!("field {fi}: {e}") } else { e })?;
            // The stepper's B0 emulation stores this field relative to the background after
            // `ticks` steps; every other field holds real states (phase 0 relabels nothing).
            let (start, stored) = match b0 {
                Some((_, ticks)) => (prog.raw_phase(ticks), true),
                None => (0, false),
            };
            out.push(FieldProgram {
                field: fi,
                rule: prog,
                start,
                stored,
                b0_ca: b0.map(|x| x.0),
            });
        }
        Ok(out)
    }

    #[inline]
    fn digit(&self, site: u32, k: u32) -> u8 {
        self.digits[(site * self.ntypes + k) as usize]
    }

    /// Pack per-type states into a site state.
    pub fn pack(&self, states: &[u8]) -> u32 {
        states
            .iter()
            .zip(&self.pow)
            .map(|(&s, &p)| s as u32 * p)
            .sum()
    }
    /// The state of tile `k` in site state `site`.
    pub fn unpack(&self, site: u32, k: u32) -> u8 {
        self.digit(site, k)
    }

    /// Number of phases of the step sequence (1 unless B0 rules or several CAs share a field).
    pub fn phase_count(&self) -> usize {
        self.phases.len()
    }
    /// The real background state of each tile type at a phase.
    pub fn background(&self, phase: u16) -> &[u8] {
        &self.phases[phase as usize].background
    }
    /// The phase `t` steps after an empty background (phase 0).
    pub fn raw_phase(&self, t: u64) -> u16 {
        let (mu, n) = (self.tail as u64, self.phases.len() as u64);
        if t < mu {
            t as u16
        } else {
            (mu + (t - mu) % (n - mu)) as u16
        }
    }
    /// The phase `steps` steps after `phase`.
    pub fn after(&self, mut phase: u16, steps: u64) -> u16 {
        for (e, a) in self.after2.iter().enumerate() {
            if (steps >> e) & 1 == 1 {
                phase = a[phase as usize];
            }
        }
        phase
    }
    /// Real tile state from stored (and back: the map is its own inverse).
    #[inline]
    pub fn real(&self, phase: u16, k: u32, stored: u8) -> u8 {
        swap0(self.phases[phase as usize].background[k as usize], stored)
    }
    /// A site state with every tile mapped between real and stored at `phase`.
    pub fn real_site(&self, phase: u16, site: u32) -> u32 {
        let b = &self.phases[phase as usize].background;
        if b.iter().all(|&x| x == 0) {
            return site;
        }
        (0..self.ntypes)
            .map(|k| swap0(b[k as usize], self.digit(site, k)) as u32 * self.pow[k as usize])
            .sum()
    }
    /// Whether the background is empty at every phase (no B0 rule in the sequence).
    pub fn empty_background(&self) -> bool {
        self.phases
            .iter()
            .all(|p| p.background.iter().all(|&b| b == 0))
    }
    /// Whether the bit-parallel leaf runs this program.
    pub fn has_bit_leaf(&self) -> bool {
        self.leaf.is_some()
    }

    /// The site at `(x, y)` of a 4×4 block after one step of phase `ph` (`1 ≤ x, y ≤ 2`).
    #[inline]
    fn next_site(&self, ph: &Phase, g: &[[u32; 4]; 4], x: usize, y: usize) -> u32 {
        let mut out = 0;
        let r = &self.steps[ph.rule];
        for k in 0..self.ntypes {
            let s = self.digit(g[y][x], k);
            let mut c = 0usize;
            for &(di, dj, kk) in &r.offsets[k as usize] {
                let v = g[(y as i32 + dj) as usize][(x as i32 + di) as usize];
                c += (self.digit(v, kk) == ph.counted[kk as usize]) as usize;
            }
            out +=
                ph.tables[k as usize][s as usize * (ph.maxn + 1) + c] as u32 * self.pow[k as usize];
        }
        out
    }
}

/// The phases of a step sequence: follow `(position in the sequence, real background)` from an
/// empty background until it repeats, label each step by `(rule, background)`, and keep the
/// shortest tail and cycle that give the same labels (so a single rule without B0 has one phase
/// however many steps per generation it runs).
fn phases_of(
    steps: &[StepRule],
    order: &[usize],
    nt: usize,
    states: u8,
) -> Result<(Vec<Phase>, usize), String> {
    let s_len = order.len();
    let mut seen: HashMap<(usize, Vec<u8>), usize> = HashMap::new();
    let mut labels: Vec<(usize, Vec<u8>)> = vec![];
    let (mut q, mut b) = (0usize, vec![0u8; nt]);
    let (mu0, lam0) = loop {
        if let Some(&t) = seen.get(&(q, b.clone())) {
            break (t, labels.len() - t);
        }
        if labels.len() >= MAX_RAW_PHASES {
            return Err("the background of this rule sequence takes too long to repeat".into());
        }
        seen.insert((q, b.clone()), labels.len());
        let r = &steps[order[q]];
        labels.push((order[q], b.clone()));
        let nb: Vec<u8> = (0..nt)
            .map(|k| {
                let c = r.offsets[k].iter().filter(|o| b[o.2 as usize] == 1).count();
                r.apply(b[k], c)
            })
            .collect();
        b = nb;
        q = (q + 1) % s_len;
    };
    let total = mu0 + lam0;
    let ext = |t: usize| -> &(usize, Vec<u8>) {
        if t < total {
            &labels[t]
        } else {
            &labels[mu0 + (t - mu0) % lam0]
        }
    };
    let lam = (1..=lam0)
        .filter(|d| lam0 % d == 0)
        .find(|&d| (mu0..total).all(|t| ext(t) == ext(t + d)))
        .unwrap_or(lam0);
    let mut mu = mu0;
    while mu > 0 && labels[mu - 1] == *ext(mu - 1 + lam) {
        mu -= 1;
    }
    let count = mu + lam;
    if count > MAX_PHASES {
        return Err(format!(
            "this rule sequence has {count} phases; HashLife allows {MAX_PHASES}"
        ));
    }
    let next = |p: usize| if p + 1 < count { p + 1 } else { mu };
    let mut out = vec![];
    for p in 0..count {
        let (ri, b) = &labels[p];
        let b2 = &labels[next(p)].1;
        let r = &steps[*ri];
        let counted: Vec<u8> = b.iter().map(|&x| swap0(x, 1)).collect();
        let tables: Vec<Vec<u8>> = (0..nt)
            .map(|k| {
                let mut t = vec![0u8; states as usize * (r.maxn + 1)];
                for s in 0..states {
                    for c in 0..=r.maxn {
                        t[s as usize * (r.maxn + 1) + c] = swap0(b2[k], r.apply(swap0(b[k], s), c));
                    }
                }
                // An empty stored neighbourhood stays empty (the point of the relabelling).
                let ce = r.offsets[k]
                    .iter()
                    .filter(|o| counted[o.2 as usize] == 0)
                    .count();
                debug_assert_eq!(t[ce], 0);
                t
            })
            .collect();
        out.push(Phase {
            rule: *ri,
            counted,
            tables,
            maxn: r.maxn,
            background: b.clone(),
            next: next(p) as u16,
        });
    }
    Ok((out, mu))
}

// ---------------------------------------------------------------------------------------------
// The node store

const NONE: u32 = u32::MAX;
const FREE: u8 = 255;
const NO_RES: u8 = 255;

#[derive(Clone, Copy, Debug)]
struct Node {
    c: [u32; 4],
    res: u32,
    /// Nonzero sites (saturating).
    pop: u64,
    level: u8,
    /// `min(j, level - 2)` of the memoised result, or `NO_RES`.
    res_j: u8,
    /// The phase the memoised result started from (see [`LatticeRule`]; 0 for one phase).
    res_ph: u16,
}

/// Counters for reports.
#[derive(Clone, Copy, Debug, Default, serde::Serialize)]
pub struct Stats {
    pub nodes: usize,
    pub peak_nodes: usize,
    pub collections: u32,
    pub node_cap: usize,
}

/// The canonical node store and RESULT memo.
pub struct Universe {
    pub rule: LatticeRule,
    nleaf: u32,
    nodes: Vec<Node>,
    free: Vec<u32>,
    table: Vec<u32>,
    count: usize,
    empty: Vec<u32>,
    cap: usize,
    /// Nodes held by computations in progress (roots for collection).
    pinned: Vec<u32>,
    gc_wanted: bool,
    stats: Stats,
    /// Levels `3..=leaf_level` use the bit-parallel leaf (0: off).
    leaf_level: u8,
    /// Scratch planes for the leaf.
    leaf_buf: [Vec<u64>; 2],
    leaf_grid: Vec<u32>,
}

#[inline]
fn hash4(c: &[u32; 4]) -> u64 {
    let a = (c[0] as u64) | (c[1] as u64) << 32;
    let b = (c[2] as u64) | (c[3] as u64) << 32;
    let mut x = a.wrapping_mul(0x9E37_79B9_7F4A_7C15)
        ^ b.wrapping_mul(0xC2B2_AE3D_27D4_EB4F).rotate_left(31);
    x ^= x >> 29;
    x = x.wrapping_mul(0xBF58_476D_1CE4_E5B9);
    x ^ (x >> 32)
}

impl Universe {
    pub fn new(rule: LatticeRule, opts: &Options) -> Universe {
        let nleaf = rule.site_states;
        let nodes: Vec<Node> = (0..nleaf)
            .map(|v| Node {
                c: [NONE; 4],
                res: NONE,
                pop: (v != 0) as u64,
                level: 0,
                res_j: NO_RES,
                res_ph: 0,
            })
            .collect();
        let mut u = Universe {
            rule,
            nleaf,
            nodes,
            free: vec![],
            table: vec![NONE; 1 << 12],
            count: 0,
            empty: vec![0],
            cap: opts.node_cap.max(1 << 12),
            pinned: vec![],
            gc_wanted: false,
            stats: Stats::default(),
            leaf_level: 0,
            leaf_buf: [vec![], vec![]],
            leaf_grid: vec![],
        };
        if u.rule.leaf.is_some() {
            u.leaf_level = opts.leaf_level.min(MAX_LEAF_LEVEL);
        }
        u.stats.node_cap = u.cap;
        u
    }

    pub fn stats(&self) -> Stats {
        Stats {
            nodes: self.count,
            ..self.stats
        }
    }

    pub fn level(&self, n: u32) -> u8 {
        self.nodes[n as usize].level
    }
    /// Nonzero sites under `n` (saturating at `u64::MAX`).
    pub fn pop(&self, n: u32) -> u64 {
        self.nodes[n as usize].pop
    }
    /// Children `[nw, ne, sw, se]` of a node of level ≥ 1.
    pub fn children(&self, n: u32) -> [u32; 4] {
        self.nodes[n as usize].c
    }
    /// The leaf node for a site state.
    pub fn leaf(&self, site: u32) -> u32 {
        debug_assert!(site < self.nleaf);
        site
    }

    /// The canonical node with these children (all of one level).
    pub fn join(&mut self, c: [u32; 4]) -> u32 {
        let mask = self.table.len() - 1;
        let mut i = hash4(&c) as usize & mask;
        loop {
            let id = self.table[i];
            if id == NONE {
                break;
            }
            if self.nodes[id as usize].c == c {
                return id;
            }
            i = (i + 1) & mask;
        }
        let level = self.nodes[c[0] as usize].level + 1;
        debug_assert!(c.iter().all(|&x| self.nodes[x as usize].level + 1 == level));
        let pop = c
            .iter()
            .fold(0u64, |a, &x| a.saturating_add(self.nodes[x as usize].pop));
        let node = Node {
            c,
            res: NONE,
            pop,
            level,
            res_j: NO_RES,
            res_ph: 0,
        };
        let id = match self.free.pop() {
            Some(id) => {
                self.nodes[id as usize] = node;
                id
            }
            None => {
                if self.nodes.len() >= NONE as usize - 1 {
                    panic!("HashLife node store full (4 billion nodes)");
                }
                self.nodes.push(node);
                (self.nodes.len() - 1) as u32
            }
        };
        self.table[i] = id;
        self.count += 1;
        self.stats.peak_nodes = self.stats.peak_nodes.max(self.count);
        if self.count * 2 > self.table.len() {
            self.rehash(self.table.len() * 2);
        }
        if self.count >= self.cap {
            self.gc_wanted = true;
        }
        id
    }

    fn rehash(&mut self, size: usize) {
        let size = size.next_power_of_two().max(1 << 12);
        self.table = vec![NONE; size];
        let mask = size - 1;
        for id in self.nleaf as usize..self.nodes.len() {
            let n = &self.nodes[id];
            if n.level == FREE {
                continue;
            }
            let mut i = hash4(&n.c) as usize & mask;
            while self.table[i] != NONE {
                i = (i + 1) & mask;
            }
            self.table[i] = id as u32;
        }
    }

    /// The empty node of a level.
    pub fn empty(&mut self, level: u8) -> u32 {
        while self.empty.len() <= level as usize {
            let e = *self.empty.last().unwrap();
            let n = self.join([e; 4]);
            self.empty.push(n);
        }
        self.empty[level as usize]
    }

    /// Mark and sweep from the pinned nodes, the empty nodes, and `extra` (and, when that frees
    /// at least half the cap, from the memoised results of kept nodes; otherwise the memo is
    /// cleared). Never moves a node, so ids held by callers stay valid when pinned.
    pub fn collect(&mut self, extra: &[u32]) {
        // First try keeping the memo: results of kept nodes are kept too. If that frees less
        // than half the cap, mark again without results (clearing the memo).
        let mut keep_memo = true;
        let mut mark;
        loop {
            mark = vec![false; self.nodes.len()];
            let mut stack: Vec<u32> = self
                .pinned
                .iter()
                .chain(self.empty.iter())
                .chain(extra.iter())
                .copied()
                .collect();
            let mut marked = 0usize;
            while let Some(id) = stack.pop() {
                let i = id as usize;
                if id < self.nleaf || mark[i] {
                    continue;
                }
                mark[i] = true;
                marked += 1;
                stack.extend_from_slice(&self.nodes[i].c);
                if keep_memo && self.nodes[i].res_j != NO_RES {
                    stack.push(self.nodes[i].res);
                }
            }
            if !keep_memo || marked * 2 <= self.cap {
                break;
            }
            keep_memo = false;
        }
        self.free.clear();
        let mut live = 0;
        let skip = self.nleaf as usize;
        for (i, (n, &kept)) in self.nodes.iter_mut().zip(&mark).enumerate().skip(skip) {
            if kept {
                if !keep_memo {
                    n.res = NONE;
                    n.res_j = NO_RES;
                }
                live += 1;
            } else {
                n.level = FREE;
                n.res = NONE;
                n.res_j = NO_RES;
                self.free.push(i as u32);
            }
        }
        // Reuse low ids first (better locality).
        self.free.reverse();
        self.count = live;
        self.rehash((live * 2).max(1 << 12));
        self.gc_wanted = false;
        self.stats.collections += 1;
        if live * 4 > self.cap * 3 {
            // Almost everything is in use: raise the cap rather than collect constantly.
            self.cap = live * 2;
            self.stats.node_cap = self.cap;
        }
    }

    /// Centre of four level `k` nodes arranged 2×2 (a level `k` node).
    #[inline]
    fn center4(&mut self, a: u32, b: u32, c: u32, d: u32) -> u32 {
        let (a, b, c, d) = (
            self.nodes[a as usize].c[3],
            self.nodes[b as usize].c[2],
            self.nodes[c as usize].c[1],
            self.nodes[d as usize].c[0],
        );
        self.join([a, b, c, d])
    }

    /// Centre of a node (one level down).
    pub fn center(&mut self, n: u32) -> u32 {
        let [a, b, c, d] = self.nodes[n as usize].c;
        self.center4(a, b, c, d)
    }

    fn base(&mut self, n: u32, ph: u16) -> u32 {
        let c = self.nodes[n as usize].c;
        let mut g = [[0u32; 4]; 4];
        for (q, &ch) in c.iter().enumerate() {
            let (ox, oy) = ((q & 1) * 2, (q >> 1) * 2);
            let l = self.nodes[ch as usize].c;
            g[oy][ox] = l[0];
            g[oy][ox + 1] = l[1];
            g[oy + 1][ox] = l[2];
            g[oy + 1][ox + 1] = l[3];
        }
        let r = &self.rule;
        let p = &r.phases[ph as usize];
        let out = [
            r.next_site(p, &g, 1, 1),
            r.next_site(p, &g, 2, 1),
            r.next_site(p, &g, 1, 2),
            r.next_site(p, &g, 2, 2),
        ];
        self.join(out)
    }

    /// RESULT: the centre of `n` (level `k ≥ 2`) after `2^min(j, k-2)` steps, starting at phase
    /// `ph` of the rule's step sequence (0 when it has one phase).
    pub fn next(&mut self, n: u32, j: u8, ph: u16) -> u32 {
        let nd = self.nodes[n as usize];
        let k = nd.level;
        assert!(
            (2..FREE).contains(&k),
            "next() needs a node of level 2 or more"
        );
        let jj = j.min(k - 2);
        if nd.res_j == jj && nd.res_ph == ph {
            return nd.res;
        }
        let r = if nd.pop == 0 {
            // Every phase keeps (stored) empty space empty.
            self.empty(k - 1)
        } else if k == 2 {
            self.base(n, ph)
        } else if k <= self.leaf_level {
            self.leaf_next(n, jj)
        } else {
            if self.gc_wanted {
                self.pinned.push(n);
                self.collect(&[]);
                self.pinned.pop();
            }
            let mark = self.pinned.len();
            self.pinned.push(n);
            let [nw, ne, sw, se] = nd.c;
            let (a, b, c, d) = (
                self.nodes[nw as usize].c,
                self.nodes[ne as usize].c,
                self.nodes[sw as usize].c,
                self.nodes[se as usize].c,
            );
            let s = [
                nw,
                self.join([a[1], b[0], a[3], b[2]]),
                ne,
                self.join([a[2], a[3], c[0], c[1]]),
                self.join([a[3], b[2], c[1], d[0]]),
                self.join([b[2], b[3], d[0], d[1]]),
                sw,
                self.join([c[1], d[0], c[3], d[2]]),
                se,
            ];
            self.pinned.extend_from_slice(&s);
            let mut r = [0u32; 9];
            for (ri, &si) in r.iter_mut().zip(&s) {
                *ri = self.next(si, j, ph);
                self.pinned.push(*ri);
            }
            let res = if jj == k - 2 {
                // The second half starts 2^(k-3) steps later.
                let ph2 = self.rule.after2[k as usize - 3][ph as usize];
                let q = [
                    self.join([r[0], r[1], r[3], r[4]]),
                    self.join([r[1], r[2], r[4], r[5]]),
                    self.join([r[3], r[4], r[6], r[7]]),
                    self.join([r[4], r[5], r[7], r[8]]),
                ];
                self.pinned.extend_from_slice(&q);
                let mut o = [0u32; 4];
                for (oi, &qi) in o.iter_mut().zip(&q) {
                    *oi = self.next(qi, j, ph2);
                    self.pinned.push(*oi);
                }
                self.join(o)
            } else {
                let o = [
                    self.center4(r[0], r[1], r[3], r[4]),
                    self.center4(r[1], r[2], r[4], r[5]),
                    self.center4(r[3], r[4], r[6], r[7]),
                    self.center4(r[4], r[5], r[7], r[8]),
                ];
                self.join(o)
            };
            self.pinned.truncate(mark);
            res
        };
        let m = &mut self.nodes[n as usize];
        m.res = r;
        m.res_j = jj;
        m.res_ph = ph;
        r
    }

    /// A node of level `lg` from a row-major `2^lg × 2^lg` grid of site states.
    pub fn from_grid(&mut self, lg: u8, grid: &[u32]) -> u32 {
        let mut side = 1usize << lg;
        assert_eq!(grid.len(), side * side);
        let mut cur: Vec<u32> = grid.iter().map(|&v| self.leaf(v)).collect();
        while side > 1 {
            let h = side / 2;
            let mut nxt = Vec::with_capacity(h * h);
            for y in 0..h {
                for x in 0..h {
                    let at = |xx: usize, yy: usize| cur[yy * side + xx];
                    let c = [
                        at(2 * x, 2 * y),
                        at(2 * x + 1, 2 * y),
                        at(2 * x, 2 * y + 1),
                        at(2 * x + 1, 2 * y + 1),
                    ];
                    nxt.push(self.join(c));
                }
            }
            cur = nxt;
            side = h;
        }
        cur[0]
    }

    /// Write the node's sites into a row-major grid of side `2^level` (zero first).
    pub fn to_grid(&self, n: u32) -> Vec<u32> {
        let side = 1usize << self.level(n);
        let mut g = vec![0u32; side * side];
        self.fill_grid(n, 0, 0, side, &mut g);
        g
    }
    fn fill_grid(&self, n: u32, x: usize, y: usize, side: usize, g: &mut [u32]) {
        let nd = &self.nodes[n as usize];
        if nd.pop == 0 {
            return;
        }
        if nd.level == 0 {
            g[y * side + x] = n;
            return;
        }
        let h = 1usize << (nd.level - 1);
        let c = nd.c;
        self.fill_grid(c[0], x, y, side, g);
        self.fill_grid(c[1], x + h, y, side, g);
        self.fill_grid(c[2], x, y + h, side, g);
        self.fill_grid(c[3], x + h, y + h, side, g);
    }

    /// Call `f(x, y, site)` for every nonzero site of `n` (top-left at `(x0, y0)`) inside the
    /// half-open box `[bx0, bx1) × [by0, by1)`.
    pub fn for_each_site(
        &self,
        n: u32,
        x0: i128,
        y0: i128,
        bx: (i128, i128),
        by: (i128, i128),
        f: &mut impl FnMut(i128, i128, u32),
    ) {
        let nd = &self.nodes[n as usize];
        if nd.pop == 0 {
            return;
        }
        let side = 1i128 << nd.level;
        if x0 >= bx.1 || y0 >= by.1 || x0 + side <= bx.0 || y0 + side <= by.0 {
            return;
        }
        if nd.level == 0 {
            f(x0, y0, n);
            return;
        }
        let h = side / 2;
        let c = nd.c;
        self.for_each_site(c[0], x0, y0, bx, by, f);
        self.for_each_site(c[1], x0 + h, y0, bx, by, f);
        self.for_each_site(c[2], x0, y0 + h, bx, by, f);
        self.for_each_site(c[3], x0 + h, y0 + h, bx, by, f);
    }

    /// Nonzero sites under `n` inside a box (exact, u128).
    pub fn pop_in(&self, n: u32, x0: i128, y0: i128, bx: (i128, i128), by: (i128, i128)) -> u128 {
        let nd = &self.nodes[n as usize];
        if nd.pop == 0 {
            return 0;
        }
        let side = 1i128 << nd.level;
        if x0 >= bx.1 || y0 >= by.1 || x0 + side <= bx.0 || y0 + side <= by.0 {
            return 0;
        }
        if x0 >= bx.0 && y0 >= by.0 && x0 + side <= bx.1 && y0 + side <= by.1 && nd.pop < u64::MAX {
            return nd.pop as u128;
        }
        if nd.level == 0 {
            return 1;
        }
        let h = side / 2;
        let c = nd.c;
        self.pop_in(c[0], x0, y0, bx, by)
            + self.pop_in(c[1], x0 + h, y0, bx, by)
            + self.pop_in(c[2], x0, y0 + h, bx, by)
            + self.pop_in(c[3], x0 + h, y0 + h, bx, by)
    }

    /// Offset of the first nonzero site along an axis from the node's near edge: `dir` 0 = min
    /// x, 1 = min y, 2 = max x (from the left edge), 3 = max y. Memoised in `memo`.
    fn extent(&self, n: u32, dir: u8, memo: &mut HashMap<(u32, u8), Option<u128>>) -> Option<u128> {
        let nd = &self.nodes[n as usize];
        if nd.pop == 0 {
            return None;
        }
        if nd.level == 0 {
            return Some(0);
        }
        if let Some(v) = memo.get(&(n, dir)) {
            return *v;
        }
        let h = 1u128 << (nd.level - 1);
        let [nw, ne, sw, se] = nd.c;
        // (near pair, far pair) for this direction.
        let (near, far) = match dir {
            0 => ([nw, sw], [ne, se]),
            1 => ([nw, ne], [sw, se]),
            2 => ([ne, se], [nw, sw]),
            _ => ([sw, se], [nw, ne]),
        };
        let pick = |a: Option<u128>, b: Option<u128>, max: bool| match (a, b) {
            (Some(x), Some(y)) => Some(if max { x.max(y) } else { x.min(y) }),
            (x, None) | (None, x) => x,
        };
        let is_max = dir >= 2;
        let near_v = pick(
            self.extent(near[0], dir, memo),
            self.extent(near[1], dir, memo),
            is_max,
        );
        let v = match near_v {
            Some(v) => Some(if is_max { v + h } else { v }),
            None => pick(
                self.extent(far[0], dir, memo),
                self.extent(far[1], dir, memo),
                is_max,
            )
            .map(|v| if is_max { v } else { v + h }),
        };
        memo.insert((n, dir), v);
        v
    }

    /// Bounding box `[x0, y0, x1, y1]` (inclusive) of the nonzero sites, relative to the node.
    pub fn bounds(&self, n: u32) -> Option<[u128; 4]> {
        let mut memo = HashMap::new();
        Some([
            self.extent(n, 0, &mut memo)?,
            self.extent(n, 1, &mut memo)?,
            self.extent(n, 2, &mut memo)?,
            self.extent(n, 3, &mut memo)?,
        ])
    }

    /// The sub-node of level `level` whose top-left corner is at `(x, y)` relative to `n`
    /// (multiples of `2^level`).
    fn descend(&self, mut n: u32, level: u8, mut x: u128, mut y: u128) -> u32 {
        while self.level(n) > level {
            let h = 1u128 << (self.level(n) - 1);
            let q = (x >= h) as usize + 2 * (y >= h) as usize;
            if x >= h {
                x -= h;
            }
            if y >= h {
                y -= h;
            }
            n = self.nodes[n as usize].c[q];
        }
        n
    }
}

// ---------------------------------------------------------------------------------------------
// The bit-parallel leaf

/// The hunter's bit-parallel stepper (`hunt::bits`) as HashLife's base case: a node of level
/// `k ≤ leaf_level` is copied into bit planes (one 64-bit word per row of sites, per tile type
/// and plane), stepped `2^jj` times with the hunter's bit-sliced counter and rule terms, and its
/// centre read back. This replaces the recursion below level `leaf_level`, which on a fresh soup
/// creates and hashes many short-lived nodes.
impl Universe {
    fn leaf_next(&mut self, n: u32, jj: u8) -> u32 {
        let plan = self.rule.leaf.clone().expect("leaf plan");
        let k = self.level(n);
        let side = 1usize << k;
        let nt = plan.ntypes;
        let np = plan.planes();
        let words = nt * np * side;
        let mut cur = std::mem::take(&mut self.leaf_buf[0]);
        let mut nxt = std::mem::take(&mut self.leaf_buf[1]);
        cur.clear();
        cur.resize(words, 0);
        nxt.clear();
        nxt.resize(words, 0);
        self.leaf_fill(n, 0, 0, side, np, &mut cur);
        let gens = 1usize << jj;
        for g in 1..=gens {
            match plan.cbits {
                1 => leaf_step::<1>(&plan, &cur, &mut nxt, side, g),
                2 => leaf_step::<2>(&plan, &cur, &mut nxt, side, g),
                3 => leaf_step::<3>(&plan, &cur, &mut nxt, side, g),
                4 => leaf_step::<4>(&plan, &cur, &mut nxt, side, g),
                _ => leaf_step::<5>(&plan, &cur, &mut nxt, side, g),
            }
            std::mem::swap(&mut cur, &mut nxt);
        }
        // The centre, as site states.
        let h = side / 2;
        let o = side / 4;
        let mut grid = std::mem::take(&mut self.leaf_grid);
        grid.clear();
        grid.resize(h * h, 0);
        let mut st = vec![0u8; nt];
        for y in 0..h {
            for x in 0..h {
                let b = o + x;
                for (t, v) in st.iter_mut().enumerate() {
                    let row = |p: usize| cur[(t * np + p) * side + o + y] >> b & 1;
                    *v = if row(0) == 1 {
                        1
                    } else {
                        (1..np).fold(0u8, |a, p| a | ((row(p) as u8) << (p - 1)))
                    };
                }
                grid[y * h + x] = self.rule.pack(&st);
            }
        }
        let r = self.from_grid(k - 1, &grid);
        self.leaf_grid = grid;
        self.leaf_buf = [cur, nxt];
        r
    }

    /// Copy the sites of `n` (top-left at `(x, y)`) into the planes.
    fn leaf_fill(&self, n: u32, x: usize, y: usize, side: usize, np: usize, buf: &mut [u64]) {
        let nd = &self.nodes[n as usize];
        if nd.pop == 0 {
            return;
        }
        if nd.level == 0 {
            for t in 0..self.rule.ntypes as usize {
                let s = self.rule.digit(n, t as u32);
                if s == 0 {
                    continue;
                }
                if s == 1 {
                    buf[(t * np) * side + y] |= 1 << x;
                } else {
                    for p in 1..np {
                        if (s >> (p - 1)) & 1 == 1 {
                            buf[(t * np + p) * side + y] |= 1 << x;
                        }
                    }
                }
            }
            return;
        }
        let h = 1usize << (nd.level - 1);
        let c = nd.c;
        self.leaf_fill(c[0], x, y, side, np, buf);
        self.leaf_fill(c[1], x + h, y, side, np, buf);
        self.leaf_fill(c[2], x, y + h, side, np, buf);
        self.leaf_fill(c[3], x + h, y + h, side, np, buf);
    }
}

/// One generation of the leaf's planes: rows `g .. side - g` (the part still exact after `g`
/// generations), as `hunt::bits` steps a universe row. Bits shifted in from outside the block
/// are zero; they only reach cells outside the exact part.
fn leaf_step<const CB: usize>(
    plan: &crate::hunt::bits::Plan,
    cur: &[u64],
    nxt: &mut [u64],
    side: usize,
    g: usize,
) {
    use crate::hunt::bits::{MAX_DEGREE, count};
    let np = plan.planes();
    let mask = if side == 64 {
        !0u64
    } else {
        (1u64 << side) - 1
    };
    let gens = plan.states > 2;
    let dbits = plan.dbits;
    let states = plan.states as u32;
    for y in g..side - g {
        for (t, offs) in plan.offs.iter().enumerate() {
            let mut xs = [0u64; MAX_DEGREE];
            let mut any_in = 0u64;
            for (o, &(di, dj, k2)) in offs.iter().enumerate() {
                let w = cur[(k2 as usize * np) * side + (y as i32 + dj) as usize];
                let v = if di > 0 {
                    w >> di
                } else if di < 0 {
                    w << -di
                } else {
                    w
                };
                xs[o] = v;
                any_in |= v;
            }
            let ia = (t * np) * side + y;
            let alive = cur[ia];
            if !gens {
                if any_in == 0 && alive == 0 {
                    nxt[ia] = 0;
                    continue;
                }
                let full = count(&xs, offs.len());
                let mut c = [0u64; CB];
                c.copy_from_slice(&full[..CB]);
                let bm = plan.birth_t.eval(&c);
                let sm = plan.survive_t.eval(&c);
                nxt[ia] = ((!alive & bm) | (alive & sm)) & mask;
                continue;
            }
            let full = count(&xs, offs.len());
            let mut c = [0u64; CB];
            c.copy_from_slice(&full[..CB]);
            let bm = plan.birth_t.eval(&c);
            let sm = plan.survive_t.eval(&c);
            let mut v = [0u64; 8];
            let mut nz = 0u64;
            for (p, x) in v.iter_mut().enumerate().take(dbits) {
                *x = cur[(t * np + 1 + p) * side + y];
                nz |= *x;
            }
            let empty = !(alive | nz);
            let na = (empty & bm) | (alive & sm);
            let dying = alive & !sm;
            // Decaying cells count up; reaching `states` means empty.
            let mut carry = nz;
            for x in v.iter_mut().take(dbits) {
                let t2 = *x & carry;
                *x ^= carry;
                carry = t2;
            }
            let mut eq = !0u64;
            for (p, x) in v.iter().enumerate().take(dbits) {
                eq &= if states >> p & 1 == 1 { *x } else { !*x };
            }
            if states >> dbits != 0 {
                eq = 0;
            }
            for x in v.iter_mut().take(dbits) {
                *x &= !eq;
            }
            v[1] |= dying;
            nxt[ia] = na & mask;
            for (p, &x) in v.iter().enumerate().take(dbits) {
                nxt[(t * np + 1 + p) * side + y] = x & mask;
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Bits of a generation count

/// Parse a generation count: `1000`, `2^40`, `1e6`, `1_000_000`.
pub fn parse_gens(s: &str) -> Result<u64, String> {
    let t = s.trim().replace('_', "");
    let v = if let Some((a, b)) = t.split_once('^') {
        let a: u64 = a
            .trim()
            .parse()
            .map_err(|_| format!("bad generation count '{s}'"))?;
        let b: u32 = b
            .trim()
            .parse()
            .map_err(|_| format!("bad generation count '{s}'"))?;
        a.checked_pow(b).ok_or(format!("{s} is too large"))?
    } else if let Some((a, b)) = t.to_ascii_lowercase().split_once('e') {
        let a: u64 = a
            .trim()
            .parse()
            .map_err(|_| format!("bad generation count '{s}'"))?;
        let b: u32 = b
            .trim()
            .parse()
            .map_err(|_| format!("bad generation count '{s}'"))?;
        a.checked_mul(10u64.checked_pow(b).ok_or(format!("{s} is too large"))?)
            .ok_or(format!("{s} is too large"))?
    } else {
        t.parse()
            .map_err(|_| format!("bad generation count '{s}'"))?
    };
    if v > MAX_GENS {
        return Err(format!(
            "{s} generations is more than HashLife allows (2^62)"
        ));
    }
    Ok(v)
}

// ---------------------------------------------------------------------------------------------
// Torus mode: a wrap-edge world, advanced in place

/// Why a world cannot jump on its torus (`None` when it can).
pub fn torus_problem(w: &World) -> Option<String> {
    torus_shape(w).err()
}

type FieldPrograms = Vec<FieldProgram>;

/// The world's field programs and the axis-aligned period `(W, H)` of a wrap-edge lattice
/// world in lattice coordinates, both powers of two. The world's own identification is
/// `(i + w, j) ~ (i, j)` and `(i + wrap_shift, j + h) ~ (i, j)`; repeating the twisted rows `m`
/// times (the smallest `m` with `m · wrap_shift ≡ 0 mod w`) gives an untwisted torus `w × m h`.
fn torus_shape(w: &World) -> Result<(FieldPrograms, u64, u64), String> {
    let progs = LatticeRule::for_world_fields(w)?;
    let g = w.topo.lattice().unwrap();
    if w.topo.spec.boundary != Boundary::Wrap || !g.wrap {
        return Err(
            "HashLife jumps in place only on wrap edges (dead and live edges are not \
                    translation invariant); use the unbounded plane instead"
                .into(),
        );
    }
    if w.script.is_some() {
        return Err("remove the script first (HashLife does not evaluate script triggers)".into());
    }
    let (gw, gh) = (g.w as u64, g.h as u64);
    if !gw.is_power_of_two() || !gh.is_power_of_two() {
        return Err(format!(
            "HashLife needs a torus whose sides are powers of two (this one is {gw} × {gh} unit cells)"
        ));
    }
    let shift = (g.wrap_shift as i64).rem_euclid(gw as i64) as u64;
    let m = if shift == 0 { 1 } else { gw / gcd(gw, shift) };
    Ok((progs, gw, gh * m))
}

fn gcd(a: u64, b: u64) -> u64 {
    if b == 0 { a } else { gcd(b, a % b) }
}

thread_local! {
    static COMMAND_OPTIONS: std::cell::Cell<Options> = std::cell::Cell::new(Options::default());
    static LAST_REPORT: std::cell::Cell<Option<JumpReport>> = const { std::cell::Cell::new(None) };
}

/// Options used by [`Command::Jump`] on this thread (results never depend on them).
pub fn command_options() -> Options {
    COMMAND_OPTIONS.with(|c| c.get())
}
/// Run `f` with these options for [`Command::Jump`] on this thread.
pub fn with_options<T>(opts: Options, f: impl FnOnce() -> T) -> T {
    let old = COMMAND_OPTIONS.with(|c| c.replace(opts));
    let r = f();
    COMMAND_OPTIONS.with(|c| c.set(old));
    r
}
/// The report of the last jump on this thread.
pub fn last_report() -> Option<JumpReport> {
    LAST_REPORT.with(|c| c.get())
}

/// Result of a jump.
#[derive(Clone, Copy, Debug, Default, serde::Serialize)]
pub struct JumpReport {
    pub generations: u64,
    pub seconds: f64,
    /// Summed over the fields (one universe each).
    pub stats: Stats,
}

impl Stats {
    fn add(&mut self, o: Stats) {
        self.nodes += o.nodes;
        self.peak_nodes += o.peak_nodes;
        self.collections += o.collections;
        self.node_cap = self.node_cap.max(o.node_cap);
    }
}

/// Advance a wrap-edge world whose torus has power-of-two sides by `gens` generations (each
/// running every enabled CA's steps per generation, in list order). Exact on states; nonzero
/// cells get age 1. Each field runs in its own universe: CAs on separate fields are independent,
/// and CAs sharing a field are one step sequence ([`LatticeRule::for_world_fields`]). This is
/// what [`Command::Jump`] runs; call that instead to keep the command log.
pub fn jump_world(w: &mut World, gens: u64, opts: &Options) -> Result<JumpReport, String> {
    let t0 = std::time::Instant::now();
    let (progs, tw, th) = torus_shape(w)?;
    if gens > MAX_GENS {
        return Err("too many generations (at most 2^62)".into());
    }
    let side = tw.max(th);
    if side * side > 1 << 32 {
        return Err("torus too large".into());
    }
    let mut all = Stats::default();
    let g = w.topo.lattice().unwrap().clone();
    for fp in progs {
        let steps = gens
            .checked_mul(fp.rule.steps_per_gen)
            .filter(|&s| s <= MAX_GENS)
            .ok_or("too many generations (at most 2^62 steps)")?;
        if steps == 0 {
            continue;
        }
        if let Some(ci) = fp.b0_ca {
            w.cas[ci].ticks += steps;
        }
        let fi = fp.field;
        let st = jump_field(&g, &mut w.fields[fi], fp, side, steps, opts);
        all.add(st);
    }
    w.generation += gens;
    let rep = JumpReport {
        generations: gens,
        seconds: t0.elapsed().as_secs_f64(),
        stats: all,
    };
    LAST_REPORT.with(|c| c.set(Some(rep)));
    Ok(rep)
}

/// One field of a torus jump: `steps` steps of `rule` on the square torus of side `side`.
fn jump_field(
    g: &LatticeGrid,
    f: &mut crate::world::Field,
    fp: FieldProgram,
    side: u64,
    steps: u64,
    opts: &Options,
) -> Stats {
    let FieldProgram {
        rule,
        start,
        stored,
        ..
    } = fp;
    let lg = side.trailing_zeros() as u8;
    let ntypes = rule.ntypes;
    let mut u = Universe::new(rule, opts);
    // The square torus, side = max(W, H), from the world (wrap resolves every coordinate).
    let s = side as usize;
    let mut grid = vec![0u32; s * s];
    {
        let mut st = vec![0u8; ntypes as usize];
        for y in 0..s {
            for x in 0..s {
                for (k, v) in st.iter_mut().enumerate() {
                    let idx = g.resolve(x as i32, y as i32, k as u32).unwrap();
                    *v = f.state[idx];
                }
                grid[y * s + x] = u.rule.pack(&st);
            }
        }
    }
    let mut t = u.from_grid(lg, &grid);
    drop(grid);
    // The world's states are stored states at `start` (phase 0 relabels nothing).
    let mut ph = start;
    for j in 0..63u8 {
        if (steps >> j) & 1 == 0 {
            continue;
        }
        // A block of 2^m × 2^m copies, level lg + m, advanced 2^j with j ≤ lg + m - 2.
        let m = (j as i32 + 2 - lg as i32).max(1) as u8;
        let mut b = t;
        for _ in 0..m {
            b = u.join([b; 4]);
        }
        let r = u.next(b, j, ph);
        ph = u.rule.after2[j as usize][ph as usize];
        // The result's corner is at 2^(lg + m - 2): half a torus when m = 1.
        t = if m == 1 {
            let [nw, ne, sw, se] = u.children(r);
            u.join([se, sw, ne, nw])
        } else {
            u.descend(r, lg, 0, 0)
        };
    }
    let out = u.to_grid(t);
    for idx in 0..f.state.len() {
        let (uu, j, k) = g.decompose(idx);
        let i = (uu + g.row_start[j as usize]).rem_euclid(side as i32) as usize;
        let sv = u.rule.unpack(out[j as usize * s + i], k);
        let v = if stored { sv } else { u.rule.real(ph, k, sv) };
        f.state[idx] = v;
        f.age[idx] = (v != 0) as u16;
    }
    u.stats()
}

// ---------------------------------------------------------------------------------------------
// Plane mode

/// One field of a plane: its universe, root, origin, and the phase of its step sequence.
pub struct Layer {
    pub uni: Universe,
    root: u32,
    /// Lattice coordinates of the root's top-left site.
    x0: i128,
    y0: i128,
    /// Phase of the next step ([`LatticeRule`]); the stored states differ from the real ones
    /// by the phase's background.
    pub phase: u16,
}

impl Layer {
    fn new(rule: LatticeRule, opts: &Options) -> Layer {
        let mut uni = Universe::new(rule, opts);
        let root = uni.empty(3);
        Layer {
            uni,
            root,
            x0: -4,
            y0: -4,
            phase: 0,
        }
    }

    /// The real background state of each tile type now (all 0 unless a B0 rule runs).
    pub fn background(&self) -> &[u8] {
        self.uni.rule.background(self.phase)
    }

    fn set_cells(&mut self, cells: &[SiteCell]) -> Result<(), String> {
        let nt = self.uni.rule.ntypes;
        let states = self.uni.rule.states;
        self.phase = 0;
        if cells.is_empty() {
            self.root = self.uni.empty(3);
            self.x0 = -4;
            self.y0 = -4;
            return Ok(());
        }
        let (mut x0, mut y0, mut x1, mut y1) = (i64::MAX, i64::MAX, i64::MIN, i64::MIN);
        for &(i, j, k, s) in cells {
            if k >= nt {
                return Err(format!(
                    "tile type {k} out of range (this lattice has {nt})"
                ));
            }
            if s >= states {
                return Err(format!("state {s} out of range (this rule has {states})"));
            }
            x0 = x0.min(i);
            y0 = y0.min(j);
            x1 = x1.max(i);
            y1 = y1.max(j);
        }
        let span = (x1 - x0 + 1).max(y1 - y0 + 1).max(8) as u64;
        let lg = span.next_power_of_two().trailing_zeros() as u8;
        let side = 1usize << lg;
        if side * side > 1 << 31 {
            return Err(format!(
                "pattern too spread out to import ({span} cells across)"
            ));
        }
        let mut digits = vec![0u8; side * side * nt as usize];
        for &(i, j, k, s) in cells {
            let at = ((j - y0) as usize * side + (i - x0) as usize) * nt as usize + k as usize;
            digits[at] = s;
        }
        let grid: Vec<u32> = digits
            .chunks(nt as usize)
            .map(|d| self.uni.rule.pack(d))
            .collect();
        drop(digits);
        self.root = self.uni.from_grid(lg, &grid);
        self.x0 = x0 as i128;
        self.y0 = y0 as i128;
        Ok(())
    }

    fn bounds(&self) -> Option<[i128; 4]> {
        let b = self.uni.bounds(self.root)?;
        Some([
            self.x0 + b[0] as i128,
            self.y0 + b[1] as i128,
            self.x0 + b[2] as i128,
            self.y0 + b[3] as i128,
        ])
    }

    fn expand(&mut self) {
        let k = self.uni.level(self.root);
        let e = self.uni.empty(k - 1);
        let [nw, ne, sw, se] = self.uni.children(self.root);
        let a = self.uni.join([e, e, e, nw]);
        let b = self.uni.join([e, e, ne, e]);
        let c = self.uni.join([e, sw, e, e]);
        let d = self.uni.join([se, e, e, e]);
        self.root = self.uni.join([a, b, c, d]);
        let h = 1i128 << (k - 1);
        self.x0 -= h;
        self.y0 -= h;
    }

    /// The outer ring (all but the four central grandchildren) is empty.
    fn centered(&self) -> bool {
        let [nw, ne, sw, se] = self.uni.children(self.root);
        let p = |n: u32, q: [usize; 3]| {
            q.iter()
                .all(|&i| self.uni.pop(self.uni.children(n)[i]) == 0)
        };
        p(nw, [0, 1, 2]) && p(ne, [0, 1, 3]) && p(sw, [0, 2, 3]) && p(se, [1, 2, 3])
    }

    fn shrink(&mut self) {
        while self.uni.level(self.root) > 3 && self.centered() {
            let k = self.uni.level(self.root);
            self.root = self.uni.center(self.root);
            let q = 1i128 << (k - 2);
            self.x0 += q;
            self.y0 += q;
        }
    }

    /// One jump of `2^j` steps.
    fn jump_pow(&mut self, j: u8) -> Result<(), String> {
        while self.uni.level(self.root) < j + 2 || !self.centered() {
            self.expand();
        }
        if self.uni.level(self.root) >= 120 {
            return Err("pattern grew too large for HashLife coordinates".into());
        }
        self.expand();
        // The result covers the root before this last expansion: its corner moves back.
        let k = self.uni.level(self.root);
        self.root = self.uni.next(self.root, j, self.phase);
        self.phase = self.uni.rule.after2[j as usize][self.phase as usize];
        let h = 1i128 << (k - 2);
        self.x0 += h;
        self.y0 += h;
        self.shrink();
        Ok(())
    }

    fn step(&mut self, steps: u64) -> Result<(), String> {
        let mut steps = steps;
        // With several phases, a jump that starts off the cycle's first phase recomputes the
        // memo; a single step first puts big jumps back on it.
        if steps & 1 == 1 && self.uni.rule.phase_count() > 1 && self.phase != 0 {
            self.jump_pow(0)?;
            steps -= 1;
        }
        for j in (0..63u8).rev() {
            if (steps >> j) & 1 == 1 {
                self.jump_pow(j)?;
            }
        }
        Ok(())
    }

    /// Real states of the tiles in a box: the tiles that differ from the background, or, when
    /// the background is not empty, every nonzero tile in the box.
    fn cells_in(&self, i0: i64, j0: i64, w: u64, h: u64, out: &mut Vec<SiteCell>) {
        let rule = &self.uni.rule;
        let nt = rule.ntypes;
        let bg = self.background();
        let bx = (i0 as i128, i0 as i128 + w as i128);
        let by = (j0 as i128, j0 as i128 + h as i128);
        if bg.iter().all(|&b| b == 0) {
            self.uni
                .for_each_site(self.root, self.x0, self.y0, bx, by, &mut |x, y, s| {
                    for k in 0..nt {
                        let v = rule.unpack(s, k);
                        if v != 0 {
                            out.push((x as i64, y as i64, k, v));
                        }
                    }
                });
            return;
        }
        let mut stored: HashMap<(i64, i64), u32> = HashMap::new();
        self.uni
            .for_each_site(self.root, self.x0, self.y0, bx, by, &mut |x, y, s| {
                stored.insert((x as i64, y as i64), s);
            });
        for j in j0..j0 + h as i64 {
            for i in i0..i0 + w as i64 {
                let s = stored.get(&(i, j)).copied().unwrap_or(0);
                for k in 0..nt {
                    let v = rule.real(self.phase, k, rule.unpack(s, k));
                    if v != 0 {
                        out.push((i, j, k, v));
                    }
                }
            }
        }
    }
}

/// A pattern on the unbounded plane. Coordinates are lattice coordinates `(i, j)` of unit cells
/// (unsheared), as in pattern-library records. Each field of the source world is a [`Layer`]
/// (one for a single CA); all layers advance in lockstep, a generation at a time.
pub struct Plane {
    pub layers: Vec<Layer>,
    pub generation: u64,
    /// The source tiling (identity).
    pub tiling: TilingSpec,
    /// The source CAs in list order (rule notation, neighbourhood, palette…), each with its
    /// layer.
    pub cas: Vec<(CaSpec, usize)>,
}

/// One live tile: lattice column and row, tile type, state.
pub type SiteCell = (i64, i64, u32, u8);
/// A parsed pattern: the rule from its header (if any) and live cells `(x, y, state)`.
pub type ParsedPattern = (Option<String>, Vec<(i64, i64, u8)>);

fn plane_lattice(tiling: &TilingSpec) -> Result<LatticeGrid, String> {
    let mut t = tiling.clone();
    t.size = Some(SizeSpec::Rect { w: 8, h: 8 });
    t.boundary = Boundary::Wrap;
    let topo = Topology::build(&t)?;
    let Storage::Lattice(g) = &topo.storage else {
        return Err("HashLife needs a periodic lattice tiling, not an aperiodic patch".into());
    };
    Ok(g.clone())
}

impl Plane {
    /// An empty plane for a rule on a tiling (any size; only its lattice is used).
    pub fn new(tiling: &TilingSpec, ca: CaSpec, opts: &Options) -> Result<Plane, String> {
        let g = plane_lattice(tiling)?;
        let (rule, nb) = Rule::parse(&ca.rule)?;
        let nb = ca.neighborhood.or(nb).unwrap_or_else(|| rule.default_nb());
        let lr = LatticeRule::program(&g, &[(rule, nb, ca.steps_per_gen.max(1))])?;
        let mut ca = ca;
        ca.neighborhood = Some(nb);
        ca.seed = Some(SeedSpec::Empty);
        ca.id = None;
        Ok(Plane {
            layers: vec![Layer::new(lr, opts)],
            generation: 0,
            tiling: crate::pattern::tiling_identity(tiling),
            cas: vec![(ca, 0)],
        })
    }

    /// The live cells of a world, placed at their lattice coordinates, one layer per field its
    /// CAs use. Edges are ignored: the pattern continues on the empty plane.
    pub fn from_world(w: &World, opts: &Options) -> Result<Plane, String> {
        let progs = LatticeRule::for_world_fields(w)?;
        let g = w.topo.lattice().unwrap();
        let mut layers = vec![];
        for fp in &progs {
            let mut l = Layer::new(fp.rule.clone(), opts);
            let f = &w.fields[fp.field];
            let cells: Vec<SiteCell> = f
                .state
                .iter()
                .enumerate()
                .filter(|(_, s)| **s != 0)
                .map(|(idx, &s)| {
                    let (u, j, k) = g.decompose(idx);
                    ((u + g.row_start[j as usize]) as i64, j as i64, k, s)
                })
                .collect();
            l.set_cells(&cells)?;
            // A Life-like B0 field continues on the plane with the world's current background.
            if fp.stored {
                l.phase = fp.start;
            }
            layers.push(l);
        }
        let cas = w
            .cas
            .iter()
            .map(|c| {
                let mut spec = c.spec();
                spec.seed = Some(SeedSpec::Empty);
                spec.id = None;
                let li = progs.iter().position(|p| p.field == c.field).unwrap();
                (spec, li)
            })
            .collect();
        Ok(Plane {
            layers,
            generation: w.generation,
            tiling: crate::pattern::tiling_identity(&w.topo.spec),
            cas,
        })
    }

    /// The first CA (the only one for a plane made from a pattern).
    pub fn ca(&self) -> &CaSpec {
        &self.cas[0].0
    }
    /// Tile types per unit cell.
    pub fn ntypes(&self) -> u32 {
        self.layers[0].uni.rule.ntypes
    }

    /// Replace the first layer's pattern with these cells (real states at the current
    /// generation, on an empty background).
    pub fn set_cells(&mut self, cells: &[SiteCell]) -> Result<(), String> {
        self.layers[0].set_cells(cells)
    }

    /// Sites (unit cells) that differ from the background, over all layers.
    pub fn population(&self) -> u64 {
        self.layers
            .iter()
            .fold(0u64, |a, l| a.saturating_add(l.uni.pop(l.root)))
    }

    /// Node counts, summed over the layers.
    pub fn stats(&self) -> Stats {
        let mut s = Stats::default();
        for l in &self.layers {
            s.add(l.uni.stats());
        }
        s
    }

    /// The real background state of each tile type of the first layer (all 0 without B0).
    pub fn background(&self) -> &[u8] {
        self.layers[0].background()
    }

    /// Tiles that differ from the background, over all layers.
    pub fn tile_population(&self) -> u128 {
        let mut n = 0u128;
        for l in &self.layers {
            let nt = l.uni.rule.ntypes;
            if nt == 1 {
                n += l.uni.pop(l.root) as u128;
                continue;
            }
            let side = 1i128 << l.uni.level(l.root);
            l.uni.for_each_site(
                l.root,
                l.x0,
                l.y0,
                (l.x0, l.x0 + side),
                (l.y0, l.y0 + side),
                &mut |_, _, s| {
                    n += (0..nt).filter(|&k| l.uni.rule.unpack(s, k) != 0).count() as u128
                },
            );
        }
        n
    }

    /// Bounding box `[i0, j0, i1, j1]` (inclusive) of the sites that differ from the
    /// background, over all layers.
    pub fn bounds(&self) -> Option<[i128; 4]> {
        self.layers
            .iter()
            .filter_map(|l| l.bounds())
            .reduce(|a, b| {
                [
                    a[0].min(b[0]),
                    a[1].min(b[1]),
                    a[2].max(b[2]),
                    a[3].max(b[3]),
                ]
            })
    }

    /// Advance `gens` generations (each running every layer's step sequence once).
    pub fn step(&mut self, gens: u64) -> Result<(), String> {
        for l in &self.layers {
            gens.checked_mul(l.uni.rule.steps_per_gen)
                .filter(|&s| s <= MAX_GENS)
                .ok_or("too many generations (at most 2^62 steps)")?;
        }
        for l in &mut self.layers {
            let steps = gens * l.uni.rule.steps_per_gen;
            l.step(steps)?;
        }
        self.generation += gens;
        Ok(())
    }

    /// Live tiles (real states) of the first layer inside the box `[i0, i0 + w) × [j0, j0 + h)`
    /// of unit cells.
    pub fn cells_in(&self, i0: i64, j0: i64, w: u64, h: u64) -> Vec<SiteCell> {
        self.layer_cells_in(0, i0, j0, w, h)
    }

    /// As [`Plane::cells_in`], for one layer.
    pub fn layer_cells_in(&self, layer: usize, i0: i64, j0: i64, w: u64, h: u64) -> Vec<SiteCell> {
        let mut out = vec![];
        self.layers[layer].cells_in(i0, j0, w, h, &mut out);
        out
    }

    /// The state of one tile (first layer).
    pub fn get(&self, i: i64, j: i64, k: u32) -> u8 {
        let c = self.cells_in(i, j, 1, 1);
        c.iter().find(|c| c.2 == k).map(|c| c.3).unwrap_or(0)
    }

    /// The default export region: the bounding box plus a margin, `(i0, j0, w, h)`.
    pub fn default_region(&self, margin: u64) -> Option<(i64, i64, u64, u64)> {
        let b = self.bounds()?;
        let m = margin as i128;
        let (i0, j0) = (b[0] - m, b[1] - m);
        let (w, h) = (b[2] - b[0] + 1 + 2 * m, b[3] - b[1] + 1 + 2 * m);
        if i0 < i64::MIN as i128 / 2
            || j0 < i64::MIN as i128 / 2
            || b[2] > i64::MAX as i128 / 2
            || b[3] > i64::MAX as i128 / 2
        {
            return None;
        }
        Some((i0 as i64, j0 as i64, w as u64, h as u64))
    }

    /// A JETCARP scene of the region `[i0, i0 + w) × [j0, j0 + h)`: the source tiling at that size
    /// with `boundary` edges, the source CAs with no seed, and each layer's cells (real states)
    /// painted in with one `set_cells` command on its first CA. World cell `(u, j, k)` takes
    /// plane tile `(i0 + i, j0 + j, k)` where `i = u + row_start[j]` is the cell's lattice
    /// column, so a world imported with [`Plane::from_world`] exports back onto itself with
    /// region `(0, 0, w, h)`.
    pub fn to_scene(
        &self,
        region: (i64, i64, u64, u64),
        boundary: Boundary,
    ) -> Result<Scene, String> {
        let (i0, j0, w, h) = region;
        if w == 0 || h == 0 || w > u32::MAX as u64 || h > u32::MAX as u64 {
            return Err("empty or oversized region".into());
        }
        let cells_total = w * h * self.ntypes() as u64;
        if cells_total > crate::topology::MAX_CELLS {
            return Err(format!(
                "a {w} × {h} region is {cells_total} cells, more than a JETCARP world allows; \
                 pick a smaller --region or render a PNG"
            ));
        }
        let mut tiling = self.tiling.clone();
        tiling.size = Some(SizeSpec::Rect {
            w: w as u32,
            h: h as u32,
        });
        tiling.boundary = boundary;
        let topo = Topology::build(&tiling)?;
        let g = topo
            .lattice()
            .ok_or("export needs a lattice tiling")?
            .clone();
        // Lattice columns span row_start .. row_start + w; collect a little wider.
        let rs_min = *g.row_start.iter().min().unwrap_or(&0) as i64;
        let rs_max = *g.row_start.iter().max().unwrap_or(&0) as i64;
        let mut sc = Scene::new(tiling);
        sc.cas = self
            .cas
            .iter()
            .map(|(c, _)| {
                let mut c = c.clone();
                c.seed = Some(SeedSpec::Empty);
                c
            })
            .collect();
        for li in 0..self.layers.len() {
            let cells = self.layer_cells_in(li, i0 + rs_min, j0, w + (rs_max - rs_min) as u64, h);
            let mut set: Vec<[u32; 2]> = vec![];
            for (i, j, k, s) in cells {
                let jj = j - j0;
                if jj < 0 || jj >= h as i64 {
                    continue;
                }
                let uu = i - i0 - g.row_start[jj as usize] as i64;
                if uu < 0 || uu >= w as i64 {
                    continue;
                }
                set.push([g.index(uu as i32, jj as i32, k) as u32, s as u32]);
            }
            set.sort_unstable();
            // CAs without ids get 1, 2, … in list order.
            let first = self.cas.iter().position(|c| c.1 == li).unwrap() as u32 + 1;
            if !set.is_empty() {
                sc.commands.push(Command::SetCells {
                    id: first,
                    cells: set,
                });
            }
        }
        sc.name = "HashLife export".into();
        sc.description = format!(
            "HashLife result at generation {} (region i {i0}.., j {j0}.., {w} × {h} unit cells). \
             Ages are not tracked by HashLife: every live cell starts at age 1. Replaying this \
             scene gives generation 0 with these cells.",
            self.generation
        );
        Ok(sc)
    }

    /// Visit the nodes of level `p` (blocks of `2^p × 2^p` unit cells) that hold any site
    /// differing from the background, inside the box `[i0, i0 + w) × [j0, j0 + h)`, over all
    /// layers: `put(i, j, sites)` with the block's lattice corner and its count of such sites.
    /// Work is proportional to the blocks visited, however large the pattern, so this is what
    /// a view at any zoom draws from.
    pub fn blocks_in(
        &self,
        region: (i64, i64, u64, u64),
        p: u8,
        put: &mut impl FnMut(i128, i128, u64),
    ) {
        for l in &self.layers {
            blocks(&l.uni, l.root, l.x0, l.y0, p, &region, put);
        }
    }

    /// The real states of the sites in a box as one value per site (the largest tile state of
    /// the unit cell over all layers), row-major `w × h`; for views at one site per pixel or
    /// closer. Background sites take the background's largest state.
    pub fn site_grid(&self, region: (i64, i64, u64, u64)) -> Vec<u8> {
        let (i0, j0, w, h) = region;
        let mut out = vec![0u8; (w * h) as usize];
        for l in &self.layers {
            let rule = &l.uni.rule;
            let nt = rule.ntypes;
            let bgmax = l.background().iter().copied().max().unwrap_or(0);
            if bgmax != 0 {
                for v in out.iter_mut() {
                    *v = (*v).max(bgmax);
                }
            }
            let bx = (i0 as i128, i0 as i128 + w as i128);
            let by = (j0 as i128, j0 as i128 + h as i128);
            let mut put = |x: i128, y: i128, s: u32| {
                let at = ((y - j0 as i128) as u64 * w + (x - i0 as i128) as u64) as usize;
                let v = (0..nt)
                    .map(|k| rule.real(l.phase, k, rule.unpack(s, k)))
                    .max()
                    .unwrap_or(0);
                if bgmax != 0 {
                    out[at] = v;
                } else {
                    out[at] = out[at].max(v);
                }
            };
            l.uni.for_each_site(l.root, l.x0, l.y0, bx, by, &mut put);
        }
        out
    }

    /// A density picture of a region in lattice coordinates (one pixel per `2^p × 2^p` block of
    /// unit cells, `p` the smallest that fits `max_side`): background where empty, the palette's
    /// colour scaled by the live fraction of each block. At one unit cell per pixel, states are
    /// coloured individually. Sheared lattices (hexagons) are drawn in lattice coordinates.
    pub fn render_density(&self, region: (i64, i64, u64, u64), max_side: u32) -> Image {
        self.render_density_in(region, max_side, &HlColors::default())
    }

    /// [`render_density`](Self::render_density) in other colours ([`HlColors`]: a palette's
    /// background where empty, its stops for states and densities).
    pub fn render_density_in(
        &self,
        region: (i64, i64, u64, u64),
        max_side: u32,
        pal: &HlColors,
    ) -> Image {
        let (i0, j0, w, h) = region;
        let mut p = 0u32;
        while (w >> p) > max_side as u64 || (h >> p) > max_side as u64 {
            p += 1;
        }
        let iw = (w.div_ceil(1 << p)).max(1) as u32;
        let ih = (h.div_ceil(1 << p)).max(1) as u32;
        let mut img = Image {
            width: iw,
            height: ih,
            rgba: pal.background.repeat((iw * ih) as usize),
        };
        let states = self.layers[0].uni.rule.states;
        let wire = self.ca().rule.trim().eq_ignore_ascii_case("wireworld");
        if p == 0 {
            let first = self.site_grid(region);
            for (i, &v) in first.iter().enumerate() {
                if v != 0 {
                    let c = pal.state(v, states, wire);
                    img.rgba[i * 4..i * 4 + 4].copy_from_slice(&c);
                }
            }
            return img;
        }
        let block = 1i128 << p;
        let mut acc = vec![0u64; (iw * ih) as usize];
        self.blocks_in(region, p as u8, &mut |x, y, n| {
            let px = (x - i0 as i128).div_euclid(block);
            let py = (y - j0 as i128).div_euclid(block);
            if px >= 0 && py >= 0 && px < iw as i128 && py < ih as i128 {
                acc[(py * iw as i128 + px) as usize] += n;
            }
        });
        let sites_per_block = (block * block) as f64;
        // Where the background is live, a block's live fraction is what does not differ.
        let live_bg = self.background().iter().any(|&b| b != 0);
        for (i, &n) in acc.iter().enumerate() {
            let f = (n as f64 / sites_per_block).clamp(0.0, 1.0);
            let f = if live_bg { 1.0 - f } else { f };
            if f == 0.0 {
                continue;
            }
            let c = pal.density(f);
            img.rgba[i * 4..i * 4 + 4].copy_from_slice(&c);
        }
        img
    }
}

/// Colours for HashLife pictures from one palette: its stops for states and densities, its
/// background for empty sites (WireWorld keeps its own colours).
#[derive(Clone, Debug, PartialEq)]
pub struct HlColors {
    stops: Vec<u32>,
    pub background: Rgba,
}

impl HlColors {
    pub fn new(p: &Palette) -> HlColors {
        HlColors {
            stops: p.packed(),
            background: p.background_rgba(),
        }
    }
    /// A tile state's colour.
    pub fn state(&self, v: u8, states: u8, wire: bool) -> Rgba {
        if wire {
            return wire_color(v);
        }
        crate::render::ramp_stops(&self.stops, state_f(v, states))
    }
    /// The density map's colour for a live fraction `f` in `(0, 1]`.
    pub fn density(&self, f: f64) -> Rgba {
        let c = crate::render::ramp_stops(&self.stops, 0.15 + 0.8 * f.sqrt());
        density_mix(self.background, c, f)
    }
}

impl Default for HlColors {
    fn default() -> Self {
        HlColors::new(&Palette::builtin("Ember").unwrap())
    }
}

fn density_mix(bg: Rgba, c: Rgba, f: f64) -> Rgba {
    let t = 0.55 + 0.45 * f.sqrt();
    let mix = |a: u8, b: u8| (a as f64 * (1.0 - t) + b as f64 * t).round() as u8;
    [mix(bg[0], c[0]), mix(bg[1], c[1]), mix(bg[2], c[2]), 255]
}

/// The density map's colour for a live fraction `f` in `(0, 1]` (square-root scale so sparse
/// blocks stay visible), in Ember ([`HlColors::density`] for another palette).
pub fn density_color(f: f64) -> Rgba {
    let c = crate::render::ramp("Ember", 0.15 + 0.8 * f.sqrt());
    density_mix(crate::render::BACKGROUND, c, f)
}

/// Visit nodes of level `p` (blocks) inside the region, reporting nonzero sites per block.
fn blocks(
    u: &Universe,
    n: u32,
    x: i128,
    y: i128,
    p: u8,
    r: &(i64, i64, u64, u64),
    put: &mut impl FnMut(i128, i128, u64),
) {
    let pop = u.pop(n);
    if pop == 0 {
        return;
    }
    let side = 1i128 << u.level(n);
    let (i0, j0) = (r.0 as i128, r.1 as i128);
    if x >= i0 + r.2 as i128 || y >= j0 + r.3 as i128 || x + side <= i0 || y + side <= j0 {
        return;
    }
    let lv = u.level(n);
    if lv <= p {
        // Blocks straddle node boundaries when the region is not aligned; good enough for a
        // density picture (the node's whole count goes to the pixel of its corner).
        put(x, y, pop);
        return;
    }
    let h = side / 2;
    let c = u.children(n);
    blocks(u, c[0], x, y, p, r, put);
    blocks(u, c[1], x + h, y, p, r, put);
    blocks(u, c[2], x, y + h, p, r, put);
    blocks(u, c[3], x + h, y + h, p, r, put);
}

/// The colour of a tile state in HashLife pictures (the Ember ramp; WireWorld's own colours;
/// [`HlColors::state`] for another palette).
pub fn state_color(v: u8, states: u8, wire: bool) -> Rgba {
    if wire {
        return wire_color(v);
    }
    crate::render::ramp("Ember", state_f(v, states))
}

fn wire_color(v: u8) -> Rgba {
    match v {
        1 => [0x6c, 0xd0, 0xff, 0xff],
        2 => [0xff, 0x5c, 0x5c, 0xff],
        _ => [0xc9, 0x93, 0x1e, 0xff],
    }
}

/// Where a state sits on the ramp.
fn state_f(v: u8, states: u8) -> f64 {
    if states <= 2 {
        0.8
    } else {
        0.9 - 0.6 * (v - 1) as f64 / (states - 1).max(1) as f64
    }
}

// ---------------------------------------------------------------------------------------------
// Importing patterns

/// Parse RLE (`x = .., y = .., rule = ..` header, `b`/`o` or `.`/`A`..`X` cells, `$` rows, `!`
/// end). Returns the rule from the header (if any) and the live cells as `(x, y, state)`.
pub fn parse_rle(text: &str) -> Result<ParsedPattern, String> {
    let mut rule = None;
    let mut body = String::new();
    for line in text.lines() {
        let l = line.trim();
        if l.starts_with('#') || l.is_empty() {
            continue;
        }
        if l.starts_with('x') && l.contains('=') && body.is_empty() {
            for part in l.split(',') {
                if let Some((k, v)) = part.split_once('=')
                    && k.trim() == "rule"
                {
                    rule = Some(v.trim().to_string());
                }
            }
            continue;
        }
        body.push_str(l);
    }
    let mut cells = vec![];
    let (mut x, mut y) = (0i64, 0i64);
    let mut num = String::new();
    let mut prefix: Option<char> = None;
    for ch in body.chars() {
        if ch.is_ascii_digit() {
            num.push(ch);
            continue;
        }
        let n: i64 = if num.is_empty() {
            1
        } else {
            num.parse().map_err(|_| "bad RLE count")?
        };
        num.clear();
        let state = match ch {
            'b' | '.' => Some(0u8),
            'o' => Some(1),
            'A'..='X' => {
                let base = match prefix.take() {
                    None => 0,
                    Some(p) => (p as u8 - b'p' + 1) as u32 * 24,
                };
                Some((base + (ch as u8 - b'A') as u32 + 1).min(255) as u8)
            }
            'p'..='y' => {
                prefix = Some(ch);
                if !num.is_empty() {
                    return Err("bad RLE".into());
                }
                // Keep the count for the next letter.
                num = n.to_string();
                if n == 1 {
                    num.clear();
                }
                continue;
            }
            '$' => {
                y += n;
                x = 0;
                None
            }
            '!' => break,
            c if c.is_whitespace() => None,
            c => return Err(format!("unexpected '{c}' in RLE")),
        };
        if let Some(s) = state {
            if s != 0 {
                for d in 0..n {
                    cells.push((x + d, y, s));
                }
            }
            x += n;
        }
    }
    Ok((rule, cells))
}

/// Parse plaintext `.cells` (`!` comments, `O`/`*` live, `.` dead).
pub fn parse_plaintext(text: &str) -> Vec<(i64, i64, u8)> {
    let mut cells = vec![];
    let mut y = 0;
    for line in text.lines() {
        if line.starts_with('!') {
            continue;
        }
        for (x, ch) in line.chars().enumerate() {
            if ch == 'O' || ch == '*' || ch == 'o' {
                cells.push((x as i64, y, 1));
            }
        }
        y += 1;
    }
    cells
}

/// A plane from a file's text: a scene (`"tiling"`), a pattern-library record (`"phases"`), RLE,
/// or plaintext. `rule` overrides the rule (RLE and plaintext default to the RLE header, else
/// Life). Scenes are replayed and their only CA's cells imported.
pub fn plane_from_text(text: &str, rule: Option<&str>, opts: &Options) -> Result<Plane, String> {
    let t = text.trim_start();
    if t.starts_with('{') {
        let v: serde_json::Value = serde_json::from_str(t).map_err(|e| format!("JSON: {e}"))?;
        if v.get("phases").is_some() {
            let rec = crate::pattern::PatternRecord::from_json(t)?;
            if rec.is_aperiodic() {
                return Err("aperiodic library patterns cannot run in HashLife".into());
            }
            let mut ca = CaSpec::new(rule.unwrap_or(&rec.rule));
            ca.neighborhood = Some(rec.neighborhood);
            ca.steps_per_gen = rec.steps_per_gen;
            let mut p = Plane::new(&rec.tiling, ca, opts)?;
            let cells: Vec<SiteCell> = rec
                .phase(0)?
                .iter()
                .map(|c| (c.i as i64, c.j as i64, c.k, c.s))
                .collect();
            p.set_cells(&cells)?;
            return Ok(p);
        }
        let mut sc = Scene::from_json(t)?;
        if let Some(r) = rule {
            for c in &mut sc.cas {
                c.rule = r.to_string();
            }
        }
        let w = World::from_scene(&sc)?;
        return Plane::from_world(&w, opts);
    }
    let (hdr, cells) =
        if t.starts_with('!') || (!t.contains('$') && !t.contains("x =") && !t.contains("x=")) {
            (None, parse_plaintext(t))
        } else {
            parse_rle(t)?
        };
    let r = rule
        .map(String::from)
        .or(hdr)
        .unwrap_or_else(|| "B3/S23".into());
    let r = normalize_rle_rule(&r);
    let mut p = Plane::new(&TilingSpec::new("square"), CaSpec::new(&r), opts)?;
    let cells: Vec<SiteCell> = cells.into_iter().map(|(x, y, s)| (x, y, 0, s)).collect();
    p.set_cells(&cells)?;
    Ok(p)
}

/// RLE headers write rules as `B3/S23`, `b3/s23`, `23/3` (S/B), or `23/3/3` (Generations S/B/C).
fn normalize_rle_rule(r: &str) -> String {
    let t = r.trim();
    let parts: Vec<&str> = t.split('/').collect();
    if !parts.iter().all(|p| p.chars().all(|c| c.is_ascii_digit())) {
        // B/S forms in any case.
        return if Rule::parse(t).is_ok() {
            t.to_string()
        } else {
            t.to_ascii_uppercase()
        };
    }
    match parts.as_slice() {
        [s, b] if s.chars().chain(b.chars()).all(|c| c.is_ascii_digit()) => format!("B{b}/S{s}"),
        [s, b, c]
            if s.chars()
                .chain(b.chars())
                .chain(c.chars())
                .all(|c| c.is_ascii_digit()) =>
        {
            format!("B{b}/S{s}/C{c}")
        }
        _ => t.to_string(),
    }
}

/// Field kind check used by callers that want to know whether a world qualifies at all
/// (for either mode).
pub fn plane_problem(w: &World) -> Option<String> {
    LatticeRule::for_world_fields(w).err()
}

/// Whether a field kind can be represented (binary or multi-state).
pub fn field_ok(k: FieldKind) -> bool {
    matches!(k, FieldKind::Binary | FieldKind::Multi(_))
}
