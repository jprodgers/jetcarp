//! Guns: a stationary core that recurs exactly every `p` generations while emitting a stream of
//! identical spaceships.
//!
//! **Decomposition.** At any generation, split the live cells into groups (cells within two
//! neighbour steps, as the census does). A group that is a spaceship on its own (it recurs
//! translated within the longest period) and lies apart from everything else is a **ship**; the
//! rest is the **core**. A newly forming ship that still touches the core is part of the core.
//!
//! **Detection.** A group that grows is run alone on the unbounded lattice for
//! [`test_gens`] generations. It is a gun of period `p` if, over the last [`CHECK_PERIODS`]
//! periods, the core is exactly the same cells (positions, tile types, states) at `t` and
//! `t - p` for every generation `t`, the number of ships grows by the same `e >= 1` every period,
//! and every ship is the same spaceship moving in the same direction. Puffers are tested first,
//! and rakes and breeders (moving cores) are not guns.
//!
//! **Record.** One period of core phases (the core in its canonical orientation), the period,
//! and [`GunInfo`]: the emitted ship's rule-free catalog id, its displacement per ship period in
//! the gun's orientation, ships per period, and the ship's phases in that direction. Guns keep
//! only the exact rules they were verified in (no rule range), like puffers.
//!
//! **Verification** ([`verify_gun`]) is independent of the hunter's stepper: the core is pasted
//! alone on an empty dead-edged world big enough to hold the whole stream over the periods
//! checked, run with the world's stepper, and every generation the core part must equal the
//! stored phase exactly while every other group is exactly a stored ship phase; after `k`
//! periods there must be exactly `k e` ships, and nothing may reach the frame's edge.

use super::bits::Universe;
use super::separate::{Cache, Census, Found, Run, ShipEntry, normalize};
use crate::library::verify::{Frame, Verification, default_periods, lattice_info, rule_world};
use crate::library::{
    GunInfo, PatternClass, PatternRecord, canonical_choice, decode_cells, encode_cells, now_utc,
    orient,
};
use crate::objects::{LCell, ObjClass, Phase};
use crate::world::Command;
use std::sync::Arc;

/// Periods over which the core must recur and the stream grow.
pub const CHECK_PERIODS: u64 = 4;

/// Live cells above which the test gives up.
pub const TEST_CAP: usize = 20_000;

/// Generations run for a gun test with this longest period.
pub fn test_gens(max_period: u64) -> u64 {
    (CHECK_PERIODS + 3) * max_period.max(16) + 96
}

/// A gun found by [`detect`].
#[derive(Clone, Debug)]
pub struct GunFit {
    pub period: u64,
    /// The gun as a pattern (class `gun`, with [`GunInfo`]).
    pub found: Arc<Found>,
    /// The emitted spaceship.
    pub ship: Arc<Found>,
    /// Its displacement per ship period, as run.
    pub ship_d: [i32; 2],
    pub per_period: u32,
}

/// Is this group a spaceship on its own? (Cached by its cells at the origin.)
pub fn ship_of(c: &Census, cache: &mut Cache, cells: &[LCell]) -> Option<ShipEntry> {
    let (key, _) = normalize(cells);
    if let Some(e) = cache.ships.get(&key) {
        return e.clone();
    }
    let e = match c.run_alone(&key, c.max_period) {
        Run::Periodic { period, d, phases } if d != [0, 0] => c
            .found(cache, PatternClass::Spaceship, period, d, &phases)
            .map(|found| ShipEntry {
                found,
                period,
                d,
                phases,
            }),
        _ => None,
    };
    cache.trim();
    let held = e.as_ref().map_or(0, |x| {
        x.phases
            .iter()
            .map(|p| super::separate::key_bytes(p.len()))
            .sum()
    });
    cache.add_bytes(super::separate::key_bytes(key.len()) + held);
    cache.ships.insert(key, e.clone());
    e
}

/// Split live cells into the core (sorted) and the ships apart from it.
pub fn split(c: &Census, cache: &mut Cache, cells: &[LCell]) -> (Phase, Vec<ShipEntry>) {
    let mut core = Vec::new();
    let mut ships = Vec::new();
    for g in c.groups(cells) {
        match ship_of(c, cache, &g.cells) {
            Some(s) => ships.push(s),
            None => core.extend(g.cells),
        }
    }
    core.sort_unstable();
    (core, ships)
}

/// Run a growing group alone and fit a gun (see the module notes).
pub fn detect(c: &Census, cache: &mut Cache, cells: &[LCell]) -> Option<GunFit> {
    let pmax = c.max_period;
    let gens = test_gens(pmax);
    let window = (CHECK_PERIODS + 1) * pmax;
    let cap = TEST_CAP.min(4 * cells.len() + 4000);
    // Cheap first pass: the cells inside the group's own box (where a gun's core stays) must
    // recur with some period while the whole population grows.
    let bb = super::separate::bbox_of(cells);
    let (ri, rj) = (c.plan.reach_i, c.plan.reach_j);
    let inner = |v: &[LCell]| -> u64 {
        let mut h = 0u64;
        for x in v {
            if x.i >= bb[0] - ri && x.i <= bb[1] + ri && x.j >= bb[2] - rj && x.j <= bb[3] + rj {
                let mut f = super::separate::FastHasher::default();
                std::hash::Hash::hash(x, &mut f);
                h = h.wrapping_add(std::hash::Hasher::finish(&f) | 1);
            }
        }
        h
    };
    let mut u = Universe::with_cells(c.plan.clone(), cells);
    let mut hashes: Vec<u64> = Vec::new();
    let mut pops: Vec<u64> = Vec::new();
    for t in 1..=gens {
        u.step();
        if u.is_empty() || u.population() as usize > cap {
            return None;
        }
        if t + window >= gens {
            hashes.push(inner(&u.cells()));
            pops.push(u.population());
        }
    }
    let n = hashes.len();
    if pops[n - 1] <= pops[0] {
        return None;
    }
    let candidates: Vec<u64> = (1..=pmax)
        .filter(|&p| {
            let need = (CHECK_PERIODS * p) as usize;
            need + (p as usize) < n
                && (0..need).all(|x| hashes[n - 1 - x] == hashes[n - 1 - x - p as usize])
        })
        .take(3)
        .collect();
    for p in candidates {
        if let Some(g) = fit(c, cache, cells, gens, p) {
            return Some(g);
        }
    }
    None
}

/// The exact test for one period: rerun and split the last periods into core and ships.
fn fit(c: &Census, cache: &mut Cache, cells: &[LCell], gens: u64, p: u64) -> Option<GunFit> {
    let pmax = p;
    let window = (CHECK_PERIODS + 1) * p;
    let mut u = Universe::with_cells(c.plan.clone(), cells);
    let mut cores: Vec<Phase> = Vec::new();
    let mut counts: Vec<usize> = Vec::new();
    let mut last: Vec<ShipEntry> = Vec::new();
    for t in 1..=gens {
        u.step();
        if t + window >= gens {
            let (core, ships) = split(c, cache, &u.cells());
            cores.push(core);
            counts.push(ships.len());
            if t == gens {
                last = ships;
            }
        }
    }
    let first = last.first()?;
    if last
        .iter()
        .any(|s| s.found.id != first.found.id || s.d != first.d)
    {
        return None;
    }
    let n = cores.len();
    for p in p..=pmax {
        let need = (CHECK_PERIODS * p) as usize;
        let pu = p as usize;
        if need + pu >= n {
            break;
        }
        if cores[n - 1].is_empty() || !(0..need).all(|x| cores[n - 1 - x] == cores[n - 1 - x - pu])
        {
            continue;
        }
        let e = counts[n - 1] as i64 - counts[n - 1 - pu] as i64;
        if e < 1
            || !(0..CHECK_PERIODS as usize)
                .all(|k| counts[n - 1 - k * pu] as i64 - counts[n - 1 - (k + 1) * pu] as i64 == e)
        {
            continue;
        }
        let phases: Vec<Phase> = cores[n - pu..].to_vec();
        let mut rec = c.record(PatternClass::Gun, p, [0, 0], &phases).ok()?;
        let (mi, _) = canonical_choice(&phases, &c.syms)?;
        let m = &c.syms[mi];
        let speed = ObjClass::Spaceship {
            period: first.period,
            dx: first.found.record.displacement.map_or(0, |d| d[0]),
            dy: first.found.record.displacement.map_or(0, |d| d[1]),
        }
        .speed()
        .unwrap_or_default();
        rec.gun = Some(GunInfo {
            ship: first.found.id.clone(),
            ship_label: first.found.label.clone(),
            ship_period: first.period,
            ship_displacement: m.vector(first.d),
            ship_speed: speed,
            per_period: e as u32,
            ship_phases: first
                .phases
                .iter()
                .map(|ph| encode_cells(&orient(ph, m).0))
                .collect(),
        });
        rec.rescore();
        return Some(GunFit {
            period: p,
            found: c.intern(cache, rec),
            ship: first.found.clone(),
            ship_d: first.d,
            per_period: e as u32,
        });
    }
    None
}

/// Verify a gun record exactly on an empty dead-edged world (see the module notes).
pub fn verify_gun(rec: &PatternRecord) -> Result<Verification, String> {
    let info = rec.gun.as_ref().ok_or("a gun record needs its gun info")?;
    let p = rec.period.max(1);
    if rec.phases.len() as u64 != p {
        return Err(format!("{} phases stored for period {p}", rec.phases.len()));
    }
    let phases = rec.all_phases()?;
    let ship: Vec<Phase> = info
        .ship_phases
        .iter()
        .map(|s| decode_cells(s).map(|v| normalize(&v).0))
        .collect::<Result<_, _>>()?;
    if ship.is_empty() || info.per_period == 0 || info.ship_period == 0 {
        return Err("the gun info has no ship".into());
    }
    let periods = default_periods(p).max(CHECK_PERIODS);
    let gens = periods * p;
    if gens > 100_000 {
        return Err("verification would run more than 100,000 generations".into());
    }
    let census = Census::new(
        &rec.rule,
        &rec.tiling,
        Some(rec.neighborhood),
        info.ship_period.max(p),
    )?;
    let li = lattice_info(&rec.tiling, rec.neighborhood)?;
    // The frame holds the core and the whole stream: the core's cells, and the same cells moved
    // as far as a ship travels in the generations run.
    let d = info.ship_displacement;
    let hops = (gens / info.ship_period + 2) as i32;
    let mut env: Vec<LCell> = Vec::new();
    for ph in &phases {
        for c in ph {
            env.push(*c);
            env.push(LCell {
                i: c.i + d[0] * hops,
                j: c.j + d[1] * hops,
                ..*c
            });
        }
    }
    let frame = Frame::around(&rec.tiling, &li, &env, li.reach + 8)?;
    let mut w = rule_world(rec, frame.topo.clone())?;
    let id = w.cas[0].id;
    w.apply(Command::SetCells {
        id,
        cells: frame.cells(&phases[0])?,
    })?;
    let fi = w.cas[0].field;
    for t in 1..=gens {
        w.apply(Command::Step { gens: 1 })?;
        let (live, edge) = frame.live(&w.fields[fi].state, rec.neighborhood);
        if edge {
            return Err(format!("generation {t}: the stream reached the frame edge"));
        }
        let mut core = Vec::new();
        let mut ships = 0u64;
        for g in census.groups(&live) {
            let (key, _) = normalize(&g.cells);
            if ship.contains(&key) {
                ships += 1;
            } else {
                core.extend(g.cells);
            }
        }
        core.sort_unstable();
        if core != phases[(t % p) as usize] {
            return Err(format!(
                "generation {t}: the core is not phase {} as stored ({} cells, {} stored)",
                t % p,
                core.len(),
                phases[(t % p) as usize].len()
            ));
        }
        if t % p == 0 && ships != (t / p) * info.per_period as u64 {
            return Err(format!(
                "generation {t}: {ships} ships, expected {} after {} periods",
                (t / p) * info.per_period as u64,
                t / p
            ));
        }
    }
    let g = frame.topo.lattice().unwrap();
    Ok(Verification {
        periods,
        generations: gens,
        field: format!(
            "dead-edged frame {}x{} unit cells: the core recurs every {p} generations and emits {} ship(s) per period ({} in all)",
            g.w,
            g.h,
            info.per_period,
            periods * info.per_period as u64
        ),
        sites: 0,
        jetcarp_version: crate::VERSION.into(),
        date: now_utc(),
    })
}
