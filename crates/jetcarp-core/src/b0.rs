//! Life-like rules with birth on 0 (B0), stored relative to their background.
//!
//! Under a B0 rule a dead cell with no live neighbours is born, so empty space does not stay
//! empty: the whole field turns on, and unless the rule survives on every count the tiles have,
//! it keeps flashing. JETCARP stores each cell relative to the **background**, the rule run from
//! the empty world: `stored = real xor background`. Empty space then stays empty, and a step
//! decodes the real states, applies the plain rule and stores the result against the next
//! background, which is exact on any neighbour graph.
//!
//! Where every tile has the same number of neighbours the background is uniform: all dead and all
//! live in turn, or live for good from the first step when the rule survives on that number. The
//! stored states are then bit for bit those of the usual alternating-rule emulation (Golly's,
//! credited to David Eppstein), which JETCARP used before 0.4.3 on every tiling; where the tiles'
//! neighbour counts differ and the survival list holds some of them but not all, that emulation
//! was wrong from the third step (#171).
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

/// A lattice's background: each tile type's state in the empty world, step by step, as `tail`
/// phases and then a cycle of the rest.
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
        let mut b = vec![0u8; nt];
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

/// A patch's background: every cell's state in the empty world after `tick` steps.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Cells {
    pub tick: u64,
    pub states: Vec<u8>,
}

impl Cells {
    /// One step of the plain rule.
    fn step(&mut self, topo: &Topology, nb: Nb, birth: u32, survive: u32) {
        let b = &self.states;
        self.states = (0..b.len())
            .into_par_iter()
            .with_min_len(crate::step::MIN_PAR)
            .map(|i| {
                let mut c = 0u32;
                topo.for_each_neighbor(i, nb, |j| c += b[j] as u32);
                next_state(b[i], c, birth, survive)
            })
            .collect();
        self.tick += 1;
    }
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
}

impl Background {
    /// The background of `(birth, survive)` with neighbourhood `nb` on `topo`, at the empty
    /// world.
    pub fn new(topo: &Topology, nb: Nb, birth: u32, survive: u32) -> Result<Background, String> {
        let kind = match &topo.storage {
            Storage::Lattice(g) => Kind::Types(Types::new(g, nb, birth, survive)?),
            Storage::Explicit(_) => Kind::Cells(Cells {
                tick: 0,
                states: vec![0; topo.len()],
            }),
        };
        Ok(Background {
            nb,
            birth,
            survive,
            kind,
        })
    }

    /// A patch's background after `tick` steps, as `states`.
    pub fn cells(nb: Nb, birth: u32, survive: u32, tick: u64, states: Vec<u8>) -> Background {
        Background {
            nb,
            birth,
            survive,
            kind: Kind::Cells(Cells { tick, states }),
        }
    }

    fn fits(&self, nb: Nb, birth: u32, survive: u32, tick: u64) -> bool {
        (self.nb, self.birth, self.survive) == (nb, birth, survive)
            && match &self.kind {
                Kind::Types(_) => true,
                Kind::Cells(c) => c.tick == tick,
            }
    }
}

/// The background of a B0 CA after `tick` steps, made (or stepped from the empty world) in `slot`
/// when `slot` holds none for this rule, neighbourhood and step count. `None` when a lattice's
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
        let mut b = Background::new(topo, nb, birth, survive).ok()?;
        if let Kind::Cells(c) = &mut b.kind {
            while c.tick < tick {
                c.step(topo, nb, birth, survive);
            }
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

    // One tile type: dead and live in turn without survival on the full count, live for good
    // with it.
    #[test]
    fn uniform_backgrounds_alternate_or_stay_live() {
        let a = types("square", Nb::Touch, "B014/S2");
        assert_eq!((a.tail, a.states.clone()), (0, vec![0, 1]));
        assert_eq!((a.phase(0), a.phase(1), a.phase(7)), (0, 1, 1));
        let b = types("square", Nb::Touch, "B0123478/S01234678");
        assert_eq!((b.tail, b.states.clone()), (1, vec![0, 1]));
        assert_eq!((b.phase(0), b.phase(1), b.phase(1000)), (0, 1, 1));
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
}
