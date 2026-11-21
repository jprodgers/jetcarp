//! Life-like rules with birth on 0 (B0), stored relative to their background.
//!
//! Under a B0 rule a dead cell with no live neighbours is born, so empty space does not stay
//! empty: the whole field turns on, and unless the rule survives on every count the tiles have,
//! it keeps flashing. JETCARP stores each cell relative to the **background**, the rule run from
//! its start: `stored = real xor background`. Empty space then stays empty, and a step decodes
//! the real states, applies the plain rule and stores the result against the next background,
//! which is exact on any neighbour graph.
//!
//! **The start.** The background starts from the empty world, except when it turns live for good
//! at the first step: when every tile, live with every neighbour slot live, survives (the rule
//! survives on every neighbour count the tiles have; `S8` on the square grid). Such a rule starts
//! on the live background, so a pattern is read against it from generation 0, as Golly reads it
//! (it runs the rule's inverse), and the two agree generation for generation (#238). Before 0.4.3
//! these rules also started from the empty world, one step behind Golly.
//!
//! Where every tile has the same number of neighbours the background is uniform: all dead and all
//! live in turn, or live for good when the rule survives on that number. The stored states are
//! then bit for bit Golly's emulation of B0 rules (the alternating one credited to David Eppstein,
//! and the inverse rule with survival on every count), which JETCARP used before 0.4.3 on every
//! tiling; where the tiles' neighbour counts differ and the survival list holds some of them but
//! not all, that emulation was wrong from the third step (#171).
//!
//! - **Lattices.** The empty world of a periodic tiling is periodic too, so the background is one
//!   state per tile type ([`Types`]), worked out on the unit cell: a short tail of phases and then
//!   a cycle. A neighbour slot past a dead edge is in the background (stored 0: as if the tiling
//!   went on, empty); past a live edge it is live, as for B/S rules without B0.
//! - **Patches.** An aperiodic patch's background differs from tile to tile (its open boundary's
//!   tiles have fewer neighbours, and inner tiles differ too) and need not settle, so it is kept
//!   per cell ([`Cells`]) and stepped with the CA.

use crate::topology::{LatticeGrid, Nb, Storage, Topology};
use rayon::prelude::*;
use std::collections::HashMap;

/// Most phases (tail and cycle) a lattice's background may take.
pub const MAX_PHASES: usize = 1 << 16;

/// A cell's next state under the two-state B/S rule `(birth, survive)`: live (`alive != 0`) or
/// dead, with `count` live neighbours.
#[inline]
pub fn next_state(alive: u8, count: u32, birth: u32, survive: u32) -> u8 {
    let m = if alive != 0 { survive } else { birth };
    (count < 32 && (m >> count) & 1 == 1) as u8
}

/// A lattice's background: each tile type's state from the start (see the module notes), step by
/// step, as `tail` phases and then a cycle of the rest.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Types {
    pub ntypes: usize,
    /// Phases before the cycle.
    pub tail: usize,
    /// `states[p * ntypes + k]`: tile type `k`'s state at phase `p`.
    pub states: Vec<u8>,
    /// `live[p * ntypes + k]`: live background cells in a type-`k` tile's neighbour slots at
    /// phase `p`, slots past a dead edge included.
    pub live: Vec<u32>,
}

impl Types {
    /// The background of `(birth, survive)` with neighbourhood `nb` on lattice `g`.
    pub fn new(g: &LatticeGrid, nb: Nb, birth: u32, survive: u32) -> Result<Types, String> {
        let nt = g.ntypes as usize;
        let offsets = &g.offsets[nb as usize];
        let live =
            |b: &[u8], k: usize| -> u32 { offsets[k].iter().map(|o| b[o.k as usize] as u32).sum() };
        let mut seen: HashMap<Vec<u8>, usize> = HashMap::new();
        let mut phases: Vec<Vec<u8>> = Vec::new();
        // The live background, when it is the first step's for good; else the empty world.
        let ones = vec![1u8; nt];
        let lives =
            birth & 1 == 1 && (0..nt).all(|k| next_state(1, live(&ones, k), birth, survive) == 1);
        let mut b = if lives { ones } else { vec![0u8; nt] };
        let tail = loop {
            if let Some(&p) = seen.get(&b) {
                break p;
            }
            if phases.len() >= MAX_PHASES {
                return Err(format!(
                    "the empty background of this B0 rule takes more than {MAX_PHASES} \
                     generations to repeat on this tiling"
                ));
            }
            seen.insert(b.clone(), phases.len());
            let next = (0..nt)
                .map(|k| next_state(b[k], live(&b, k), birth, survive))
                .collect();
            phases.push(std::mem::replace(&mut b, next));
        };
        Ok(Types {
            ntypes: nt,
            tail,
            live: phases
                .iter()
                .flat_map(|b| (0..nt).map(|k| live(b, k)).collect::<Vec<_>>())
                .collect(),
            states: phases.concat(),
        })
    }

    /// Phases in all, tail and cycle.
    pub fn phases(&self) -> usize {
        self.states.len() / self.ntypes.max(1)
    }

    /// The phase `t` steps after the empty world.
    pub fn phase(&self, t: u64) -> usize {
        let (mu, n) = (self.tail as u64, self.phases() as u64);
        if t < mu {
            t as usize
        } else {
            (mu + (t - mu) % (n - mu)) as usize
        }
    }

    /// Tile type `k`'s state at phase `p`.
    #[inline]
    pub fn state(&self, p: usize, k: usize) -> u8 {
        self.states[p * self.ntypes + k]
    }

    /// The state of every tile type at phase `p`, when they all have the same one.
    pub fn uniform(&self, p: usize) -> Option<u8> {
        let row = &self.states[p * self.ntypes..(p + 1) * self.ntypes];
        row.iter().all(|&s| s == row[0]).then_some(row[0])
    }
}

/// A patch's background: every cell's state `tick` steps after the start (see the module notes).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Cells {
    pub tick: u64,
    pub states: Vec<u8>,
}

/// Most cell updates spent looking for a patch's background cycle ([`Background::new`]), and most
/// steps: a background that has not repeated by then is treated as never repeating.
const CYCLE_WORK: usize = 1 << 26;
const CYCLE_STEPS: usize = 4096;

/// One step of the plain rule on every cell of a patch.
fn cells_next(topo: &Topology, nb: Nb, birth: u32, survive: u32, b: &[u8]) -> Vec<u8> {
    (0..b.len())
        .into_par_iter()
        .with_min_len(crate::step::MIN_PAR)
        .map(|i| {
            let mut c = 0u32;
            topo.for_each_neighbor(i, nb, |j| c += b[j] as u32);
            next_state(b[i], c, birth, survive)
        })
        .collect()
}

/// Whether the background of `(birth, survive)` on a patch starts live (see the module notes):
/// every cell, live with its neighbours live, survives. For steppers that keep a patch's
/// background themselves.
pub fn patch_starts_live(topo: &Topology, nb: Nb, birth: u32, survive: u32) -> bool {
    cells_start(topo, nb, birth, survive).first() == Some(&1)
}

/// A patch's background at the start: every cell live when the first step's live background
/// lasts (every cell, live with its neighbours live, survives), else the empty world.
fn cells_start(topo: &Topology, nb: Nb, birth: u32, survive: u32) -> Vec<u8> {
    let ones = vec![1u8; topo.len()];
    if birth & 1 == 1 && cells_next(topo, nb, birth, survive, &ones) == ones {
        ones
    } else {
        vec![0; topo.len()]
    }
}

/// A patch's background as `(tail, period)`, when it repeats within the budget (Brent's cycle
/// finding, from `x0`, the start).
fn cells_cycle(
    topo: &Topology,
    nb: Nb,
    birth: u32,
    survive: u32,
    x0: Vec<u8>,
) -> Option<(u64, u64)> {
    let n = topo.len();
    let budget = (CYCLE_WORK / n.max(1)).min(CYCLE_STEPS);
    let step = |b: &[u8]| cells_next(topo, nb, birth, survive, b);
    let (mut power, mut lam) = (1usize, 1usize);
    let (mut tortoise, mut hare) = (x0.clone(), step(&x0));
    let mut steps = 1;
    while tortoise != hare {
        if power == lam {
            tortoise = hare.clone();
            power *= 2;
            lam = 0;
        }
        hare = step(&hare);
        lam += 1;
        steps += 1;
        if steps > budget {
            return None;
        }
    }
    let (mut tortoise, mut hare) = (x0.clone(), x0);
    for _ in 0..lam {
        hare = step(&hare);
    }
    let mut mu = 0usize;
    while tortoise != hare {
        tortoise = step(&tortoise);
        hare = step(&hare);
        mu += 1;
        if mu > budget {
            return None;
        }
    }
    Some((mu as u64, lam as u64))
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Kind {
    Types(Types),
    Cells(Cells),
}

/// A B0 CA's background, for the rule and neighbourhood it was made for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Background {
    pub nb: Nb,
    pub birth: u32,
    pub survive: u32,
    pub kind: Kind,
    /// The background's `(tail, period)` in steps from the start, when known: always on a
    /// lattice; on a patch when it repeats soon enough to be found ([`Background::new`]).
    pub cycle: Option<(u64, u64)>,
    /// Whether it starts live (see the module notes); else from the empty world.
    pub starts_live: bool,
}

impl Background {
    /// The background of `(birth, survive)` with neighbourhood `nb` on `topo`, at its start (see
    /// the module notes). On a patch this looks for the background's cycle, for at most 2^26
    /// cell updates and 4,096 steps.
    pub fn new(topo: &Topology, nb: Nb, birth: u32, survive: u32) -> Result<Background, String> {
        let (kind, cycle, starts_live) = match &topo.storage {
            Storage::Lattice(g) => {
                let t = Types::new(g, nb, birth, survive)?;
                let cycle = (t.tail as u64, (t.phases() - t.tail) as u64);
                let live = t.ntypes > 0 && t.uniform(0) == Some(1);
                (Kind::Types(t), Some(cycle), live)
            }
            Storage::Explicit(_) => {
                let start = cells_start(topo, nb, birth, survive);
                let live = start.first() == Some(&1);
                let cycle = cells_cycle(topo, nb, birth, survive, start.clone());
                (
                    Kind::Cells(Cells {
                        tick: 0,
                        states: start,
                    }),
                    cycle,
                    live,
                )
            }
        };
        Ok(Background {
            nb,
            birth,
            survive,
            kind,
            cycle,
            starts_live,
        })
    }

    /// This patch background after `tick` steps, as `states`.
    pub fn with_cells(&self, tick: u64, states: Vec<u8>) -> Background {
        Background {
            nb: self.nb,
            birth: self.birth,
            survive: self.survive,
            kind: Kind::Cells(Cells { tick, states }),
            cycle: self.cycle,
            starts_live: self.starts_live,
        }
    }

    /// Whether this is the background of `(birth, survive)` with neighbourhood `nb`.
    pub fn is_for(&self, nb: Nb, birth: u32, survive: u32) -> bool {
        (self.nb, self.birth, self.survive) == (nb, birth, survive)
    }

    fn fits(&self, nb: Nb, birth: u32, survive: u32, tick: u64) -> bool {
        self.is_for(nb, birth, survive)
            && match &self.kind {
                Kind::Types(_) => true,
                Kind::Cells(c) => c.tick == tick,
            }
    }

    /// The phase `t` steps after the start: equal for two step counts exactly when the
    /// background is the same at both, and so goes on the same. A background whose cycle is not
    /// known gives every step count a value of its own (from 2^63), which matches no other.
    pub fn phase_key(&self, t: u64) -> u64 {
        match self.cycle {
            Some((mu, _)) if t < mu => t,
            Some((mu, lam)) => mu + (t - mu) % lam,
            None => (1 << 63) | t,
        }
    }
}

/// The background of a B0 CA after `tick` steps, made (or stepped from its start) in `slot` when
/// `slot` holds none for this rule, neighbourhood and step count. `None` when a lattice's
/// background takes too long to repeat ([`MAX_PHASES`]), which adding the CA refuses.
pub fn ready(
    slot: &mut Option<std::sync::Arc<Background>>,
    topo: &Topology,
    nb: Nb,
    (birth, survive): (u32, u32),
    tick: u64,
) -> Option<std::sync::Arc<Background>> {
    if !slot
        .as_deref()
        .is_some_and(|b| b.fits(nb, birth, survive, tick))
    {
        // A patch's background at another step count keeps its cycle; its states are stepped
        // from the start, only as far as that step count's phase when the cycle is known.
        let mut b = match slot.as_deref() {
            Some(old) if old.is_for(nb, birth, survive) => {
                old.with_cells(0, vec![old.starts_live as u8; topo.len()])
            }
            _ => Background::new(topo, nb, birth, survive).ok()?,
        };
        let steps = match b.phase_key(tick) {
            p if p >> 63 == 1 => tick,
            p => p,
        };
        if let Kind::Cells(c) = &mut b.kind {
            for _ in 0..steps {
                c.states = cells_next(topo, nb, birth, survive, &c.states);
            }
            c.tick = tick;
        }
        *slot = Some(std::sync::Arc::new(b));
    }
    slot.clone()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::topology::{Boundary, TilingSpec};

    fn types(family: &str, nb: Nb, rule: &str) -> Types {
        let t = Topology::build(&TilingSpec::rect(family, 8, 8, Boundary::Wrap)).unwrap();
        let (r, _) = crate::rules::Rule::parse(rule).unwrap();
        let crate::rules::Rule::LifeLike { birth, survive } = r else {
            panic!()
        };
        Types::new(t.lattice().unwrap(), nb, birth, survive).unwrap()
    }

    // One tile type: dead and live in turn without survival on the full count; with it, live
    // from the start (#238), where it was dead for the first generation before.
    #[test]
    fn uniform_backgrounds_alternate_or_stay_live() {
        let a = types("square", Nb::Touch, "B014/S2");
        assert_eq!((a.tail, a.states.clone()), (0, vec![0, 1]));
        assert_eq!((a.phase(0), a.phase(1), a.phase(7)), (0, 1, 1));
        let b = types("square", Nb::Touch, "B0123478/S01234678");
        assert_eq!((b.tail, b.states.clone()), (0, vec![1]));
        assert_eq!((b.phase(0), b.phase(1), b.phase(1000)), (0, 0, 0));
        // On hexagons the full count is 6, on 4.8.8 the squares' 4 and the octagons' 8.
        assert_eq!(types("hex", Nb::Touch, "B0/S6").states, vec![1]);
        assert_eq!(types("hex", Nb::Touch, "B0/S8").states, vec![0, 1]);
        assert_eq!(types("oct", Nb::Touch, "B0/S48").uniform(0), Some(1));
        assert_eq!(types("oct", Nb::Touch, "B0/S8").uniform(0), Some(0));
    }

    // A patch whose every tile survives on its own neighbour count starts live; one that does
    // not starts from the empty world.
    #[test]
    fn patch_backgrounds_start_live_with_full_survival() {
        let t =
            Topology::build(&TilingSpec::new("hat").with_size(crate::SizeSpec::Level(2))).unwrap();
        let all: u32 = (0..=31).fold(0, |m, c| m | 1 << c);
        let full = Background::new(&t, Nb::Touch, 1, all).unwrap();
        assert!(full.starts_live);
        assert_eq!(full.cycle, Some((0, 1)));
        let Kind::Cells(c) = &full.kind else { panic!() };
        assert!(c.states.iter().all(|&s| s == 1));
        let flashing = Background::new(&t, Nb::Touch, 1, 1 << 8).unwrap();
        assert!(!flashing.starts_live);
    }

    // 4.8.8 with B0/S8: octagons (8 neighbours) survive the first full step, squares (4) do not;
    // then every tile dies, and the cycle starts again: period 3.
    #[test]
    fn mixed_degrees_give_a_background_per_type() {
        let t = Topology::build(&TilingSpec::rect("oct", 8, 8, Boundary::Wrap)).unwrap();
        let g = t.lattice().unwrap();
        let bg = types("oct", Nb::Touch, "B0/S8");
        assert_eq!((bg.tail, bg.phases()), (0, 3));
        let octagon = (0..g.ntypes as usize)
            .find(|&k| g.offsets[Nb::Touch as usize][k].len() == 8)
            .unwrap();
        for k in 0..bg.ntypes {
            let want = [0, 1, (k == octagon) as u8];
            assert_eq!([bg.state(0, k), bg.state(1, k), bg.state(2, k)], want);
        }
        assert_eq!(bg.uniform(2), None);
    }

    // A patch's background cycle, as stepping it shows, and the phase keys it gives: B0/S8 on a
    // hat patch (no tile has 8 neighbours) alternates from the start; B0/S6 settles later.
    #[test]
    fn patch_backgrounds_know_their_cycle() {
        let t =
            Topology::build(&TilingSpec::new("hat").with_size(crate::SizeSpec::Level(3))).unwrap();
        for (rule, want) in [("B0/S8", (0, 2)), ("B0/S6", (36, 6))] {
            let (r, _) = crate::rules::Rule::parse(rule).unwrap();
            let crate::rules::Rule::LifeLike { birth, survive } = r else {
                panic!()
            };
            let bg = Background::new(&t, Nb::Touch, birth, survive).unwrap();
            assert_eq!(bg.cycle, Some(want), "{rule}");
            let (mu, lam) = want;
            let mut states = vec![vec![0u8; t.len()]];
            for _ in 0..mu + 2 * lam {
                let next = cells_next(&t, Nb::Touch, birth, survive, states.last().unwrap());
                states.push(next);
            }
            for a in 0..states.len() as u64 {
                for b in 0..states.len() as u64 {
                    let same = states[a as usize] == states[b as usize];
                    assert_eq!(bg.phase_key(a) == bg.phase_key(b), same, "{rule} {a} {b}");
                }
            }
        }
        // A background not known to repeat matches no other step count.
        let none = Background {
            cycle: None,
            ..Background::new(&t, Nb::Touch, 1, 1 << 8).unwrap()
        };
        assert_ne!(none.phase_key(3), none.phase_key(5));
        assert_eq!(none.phase_key(3) >> 63, 1);
    }
}
