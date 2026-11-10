//! Per-rule stepping. Every step is a pure function of the previous field state, computed in
//! parallel over cells into a back buffer, so results do not depend on the thread count.

use crate::rules::{FieldKind, Rule};
use crate::topology::{Nb, Storage, Topology};
use crate::world::{Agent, AgentEdge, CaInstance, Field};
use rayon::prelude::*;

pub(crate) const MIN_PAR: usize = 4096;

pub(crate) fn step_ca(topo: &Topology, ca: &mut CaInstance, f: &mut Field, first_of_gen: bool) {
    match &ca.rule {
        Rule::LifeLike { birth, survive } if birth & 1 == 1 => {
            ca.ticks += 1;
            step_b0(topo, ca.nb, *birth, *survive, ca.ticks, f)
        }
        Rule::LifeLike { birth, survive } => step_bs(topo, ca.nb, *birth, *survive, 2, f),
        Rule::Generations {
            birth,
            survive,
            states,
        } => step_bs(topo, ca.nb, *birth, *survive, *states, f),
        Rule::Wolfram(r) => step_wolfram(topo, *r, &mut ca.cursor, f),
        Rule::Turmite(mite) => step_turmite(topo, mite, &mut ca.agents, ca.agent_edge, f),
        Rule::WireWorld => step_wireworld(topo, ca.nb, f),
        Rule::Sandpile => {
            if first_of_gen && ca.drop_per_gen > 0 {
                let c = topo.center_cell();
                f.height[c] = f.height[c].saturating_add(ca.drop_per_gen);
            }
            step_sandpile(topo, ca.nb, f);
        }
        Rule::SpectreGlider => step_glider(topo, ca.nb, f),
        Rule::Priority(p) => crate::evolve::priority::step_priority(topo, ca.nb, p, f),
        Rule::Cyclic { states, threshold } => step_cyclic(topo, ca.nb, *states, *threshold, f),
        // Rock-paper-scissors converts on strictly more than the threshold.
        Rule::Rps { states, threshold } => step_cyclic(topo, ca.nb, *states, *threshold + 1, f),
        Rule::GreenbergHastings { states, threshold } => {
            step_gh(topo, ca.nb, *states, *threshold, f)
        }
        Rule::ForestFire { p, f: lightning } => {
            ca.ticks += 1;
            step_forest(topo, ca.nb, *p, *lightning, ca.noise, ca.ticks, f)
        }
        Rule::Hodgepodge { n, k1, k2, g } => step_hodgepodge(topo, ca.nb, *n, *k1, *k2, *g, f),
        Rule::HexWolfram { states, number } => {
            step_hex_wolfram(topo, *states, *number, &mut ca.cursor, f)
        }
        Rule::HexTri { .. } => {
            if let Some(t) = ca.rule.hext_table() {
                step_hext(topo, ca.rule.field_kind(), &t, &mut ca.cursor, f)
            }
        }
        Rule::Lenia(l) => {
            if ca.kernel.is_none() {
                ca.kernel = crate::lenia::kernel(topo, l).ok().map(std::sync::Arc::new);
            }
            if let Some(k) = &ca.kernel {
                crate::lenia::step(topo, l, k, f)
            }
        }
        Rule::Margolus(t) => {
            // The partition offset is the parity of this CA's step count (hashed and replayed).
            let phase = (ca.ticks % 2) as i32;
            ca.ticks += 1;
            step_margolus(topo, t, phase, f)
        }
        Rule::Table(_) => {
            if let Some(t) = &ca.table {
                step_table(topo, ca.nb, t, f)
            }
        }
        Rule::Layered { digits, .. } => {
            step_layered(topo, ca.nb, digits, &mut ca.layers, &mut ca.cursor, f)
        }
        Rule::Ltl(l) => {
            if ca.reach.is_none() {
                ca.reach = crate::ltl::reach(topo, l.hood, l.range)
                    .ok()
                    .map(std::sync::Arc::new);
            }
            if let Some(r) = &ca.reach {
                crate::ltl::step_ltl(topo, l, r, f)
            }
        }
        Rule::VonNeumann => {
            crate::vonneumann::step(topo, f);
            commit_states(f);
        }
        Rule::Jaca(r) => {
            let (stats, zone) = crate::jaca::step(topo, ca.nb, r, f);
            let heat = std::sync::Arc::make_mut(ca.heat.get_or_insert_with(Default::default));
            crate::jaca::update_heat(heat, f.state.len(), stats, &zone);
        }
    }
}

/// B0 rules without emulation would flip the whole background every generation. As in other CA
/// programs, they run as a pair of rules on alternating generations so the background stays dead:
/// stored states are the true states on even generations and their complement on odd ones.
/// For a cell with `d` neighbours and rule (B, S):
/// - without S_d: even steps use (not B, not S); odd steps use B' = {d-c : c in S}, S' = {d-c : c in B};
/// - with S_d (the background would stay live): the stored state stays complemented after the
///   first step, which uses (not B, not S); later steps use B' = {d-c : c not in S},
///   S' = {d-c : c not in B}.
///
/// `tick` is the 1-based step count of this CA.
fn step_b0(topo: &Topology, nb: Nb, birth: u32, survive: u32, tick: u64, f: &mut Field) {
    let odd = tick.is_multiple_of(2);
    let rev = |m: u32, d: u32| -> u32 {
        (0..=d)
            .filter(|c| m >> c & 1 == 1)
            .fold(0, |a, c| a | 1 << (d - c))
    };
    // Per degree: (birth, survive) masks to use this step.
    let masks: Vec<(u32, u32)> = (0..32u32)
        .map(|d| {
            let full = if d >= 31 {
                u32::MAX
            } else {
                (1u32 << (d + 1)) - 1
            };
            if survive >> d & 1 == 1 {
                if tick == 1 {
                    (!birth & full, !survive & full)
                } else {
                    (rev(!survive & full, d), rev(!birth & full, d))
                }
            } else if !odd {
                (!birth & full, !survive & full)
            } else {
                (rev(survive & full, d), rev(birth & full, d))
            }
        })
        .collect();
    let a = &f.state;
    let age = &f.age;
    f.back
        .par_iter_mut()
        .zip(f.back_age.par_iter_mut())
        .enumerate()
        .with_min_len(MIN_PAR)
        .for_each(|(i, (o, oa))| {
            let s = a[i];
            let mut c = 0u32;
            topo.for_each_neighbor(i, nb, |j| c += (a[j] == 1) as u32);
            let d = (topo.degree(i, nb) as u32).min(31);
            let (b, sv) = masks[d as usize];
            let v = if s == 0 { (b >> c) & 1 } else { (sv >> c) & 1 } as u8;
            *o = v;
            *oa = if v == 1 {
                if s == 1 {
                    age[i].saturating_add(1).min(65000)
                } else {
                    1
                }
            } else {
                0
            };
        });
    std::mem::swap(&mut f.state, &mut f.back);
    std::mem::swap(&mut f.age, &mut f.back_age);
}

/// Write a new state array (ages: 1 while a cell is nonzero and unchanged-from-zero, else kept).
fn commit_states(f: &mut Field) {
    let a = &f.state;
    let b = &f.back;
    f.back_age
        .par_iter_mut()
        .enumerate()
        .with_min_len(MIN_PAR)
        .for_each(|(i, o)| {
            let (old, new) = (a[i], b[i]);
            *o = if new == 0 {
                0
            } else if new == old {
                f.age[i].saturating_add(1).min(65000)
            } else {
                1
            };
        });
    std::mem::swap(&mut f.state, &mut f.back);
    std::mem::swap(&mut f.age, &mut f.back_age);
}

/// Cyclic CA and rock-paper-scissors: s advances to s+1 (mod n) when at least `t` neighbours hold it.
fn step_cyclic(topo: &Topology, nb: Nb, n: u8, t: u8, f: &mut Field) {
    let a = &f.state;
    f.back
        .par_iter_mut()
        .enumerate()
        .with_min_len(MIN_PAR)
        .for_each(|(i, o)| {
            let s = a[i];
            let next = if s + 1 >= n { 0 } else { s + 1 };
            let mut c = 0u8;
            topo.for_each_neighbor(i, nb, |j| c += (a[j] == next) as u8);
            *o = if c >= t { next } else { s };
        });
    commit_states(f);
}

/// Greenberg-Hastings: 0 rest, 1 excited, 2..n-1 refractory.
fn step_gh(topo: &Topology, nb: Nb, n: u8, t: u8, f: &mut Field) {
    let a = &f.state;
    f.back
        .par_iter_mut()
        .enumerate()
        .with_min_len(MIN_PAR)
        .for_each(|(i, o)| {
            let s = a[i];
            *o = if s == 0 {
                let mut c = 0u8;
                topo.for_each_neighbor(i, nb, |j| c += (a[j] == 1) as u8);
                (c >= t) as u8
            } else if s + 1 >= n {
                0
            } else {
                s + 1
            };
        });
    commit_states(f);
}

/// Counter-based random number in [0, 1) for (seed, tick, cell): independent of thread count.
#[inline]
pub(crate) fn noise01(seed: u64, tick: u64, cell: usize) -> f64 {
    let mut z = seed
        ^ tick.wrapping_mul(0x9E37_79B9_7F4A_7C15)
        ^ (cell as u64).wrapping_mul(0xD1B5_4A32_D192_ED03);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^= z >> 31;
    (z >> 11) as f64 / (1u64 << 53) as f64
}

/// Drossel-Schwabl forest fire: 0 empty, 1 tree, 2 burning.
fn step_forest(
    topo: &Topology,
    nb: Nb,
    p: f64,
    lightning: f64,
    seed: u64,
    tick: u64,
    f: &mut Field,
) {
    let a = &f.state;
    f.back
        .par_iter_mut()
        .enumerate()
        .with_min_len(MIN_PAR)
        .for_each(|(i, o)| {
            *o = match a[i] {
                2 => 0,
                1 => {
                    let mut burning = false;
                    topo.for_each_neighbor(i, nb, |j| burning |= a[j] == 2);
                    if burning || noise01(seed, tick, i) < lightning {
                        2
                    } else {
                        1
                    }
                }
                _ => (noise01(seed, tick, i) < p) as u8,
            };
        });
    commit_states(f);
}

/// Hodgepodge machine (Gerhardt-Schuster, as popularised by Dewdney), states 0..=n:
/// healthy (0) becomes a/k1 + b/k2; infected (1..n-1) becomes S/(a+b+1) + g, where S is the sum of
/// the cell's and its neighbours' states; ill (n) becomes healthy. Results are capped at n.
/// Here a = infected neighbours and b = ill neighbours; divisions round down.
fn step_hodgepodge(topo: &Topology, nb: Nb, n: u8, k1: u32, k2: u32, g: u32, f: &mut Field) {
    let a = &f.state;
    f.back
        .par_iter_mut()
        .enumerate()
        .with_min_len(MIN_PAR)
        .for_each(|(i, o)| {
            let s = a[i];
            let (mut inf, mut ill, mut sum) = (0u32, 0u32, s as u32);
            topo.for_each_neighbor(i, nb, |j| {
                let v = a[j];
                sum += v as u32;
                if v == n {
                    ill += 1;
                } else if v > 0 {
                    inf += 1;
                }
            });
            let v = if s == n {
                0
            } else if s == 0 {
                inf / k1 + ill / k2
            } else {
                sum / (inf + ill + 1) + g
            };
            *o = v.min(n as u32) as u8;
        });
    commit_states(f);
}

/// Elementary rule on hexagon rows. Each cell in the next row reads the two cells that touch it in
/// the current row: index = left*k + right, with k = 2 or 3 states.
fn step_hex_wolfram(topo: &Topology, k: u8, number: u32, cursor: &mut u32, f: &mut Field) {
    let Some(g) = topo.hex_rows() else { return };
    let live = topo.live_edges() as u32;
    let r = *cursor;
    let next = if r + 1 >= g.h { return } else { r + 1 };
    // The two edge offsets pointing one row up, ordered left to right.
    let mut up: Vec<_> = g.offsets[Nb::Edge as usize][0]
        .iter()
        .filter(|o| o.dj == -1)
        .collect();
    up.sort_by_key(|o| o.di);
    if up.len() != 2 {
        return;
    }
    let k = k as u32;
    let digits: Vec<u8> = (0..k * k)
        .map(|idx| ((number / k.pow(idx)) % k) as u8)
        .collect();
    let mut row = Vec::with_capacity(g.w as usize);
    for u in 0..g.w as i32 {
        let i = u + g.row_start[next as usize];
        let read = |o: &crate::topology::Offset| -> u32 {
            match g.resolve(i + o.di, next as i32 + o.dj, 0) {
                Some(c) => (f.state[c] as u32).min(k - 1),
                None => live,
            }
        };
        row.push(digits[(read(up[0]) * k + read(up[1])) as usize]);
    }
    for (u, v) in row.into_iter().enumerate() {
        let c = g.index(u as i32, next as i32, 0);
        let was = f.state[c];
        f.state[c] = v;
        f.age[c] = if v == 0 {
            0
        } else if was == v {
            f.age[c].saturating_add(1)
        } else {
            1
        };
    }
    *cursor = next;
}

/// Number of half-row layers of the flat-top hexagon grid: layer `2j` is the type-0 hexagons of
/// unit-cell row `j`, layer `2j + 1` the type-1 hexagons half a row lower.
pub(crate) fn hext_layers(g: &crate::topology::LatticeGrid) -> u32 {
    2 * g.h
}

/// The parents of the hexagon in column `u` of layer `layer` (flat-top grid, see
/// [`hext_layers`]) as lattice positions `(i, j, type)`: NW, N, NE.
#[inline]
pub(crate) fn hext_parents(u: i32, layer: u32) -> [(i32, i32, u32); 3] {
    let j = (layer / 2) as i32;
    if layer % 2 == 1 {
        // A type-1 hexagon: NW and NE are the type-0 hexagons of its own unit-cell row.
        [(u, j, 0), (u, j - 1, 1), (u + 1, j, 0)]
    } else {
        // A type-0 hexagon: NW and NE are type-1 hexagons of the row above.
        [(u - 1, j - 1, 1), (u, j - 1, 0), (u, j - 1, 1)]
    }
}

/// Three-parent hexagon rule: write the next half-row layer from the parents above it (NW and
/// NE in the layer just written, N two layers up). Wrap edges wrap both ways.
fn step_hext(topo: &Topology, kind: FieldKind, table: &[u8], cursor: &mut u32, f: &mut Field) {
    let Some(g) = topo.flat_hex_rows() else {
        return;
    };
    let k: u32 = match kind {
        FieldKind::Multi(n) => n as u32,
        _ => 2,
    };
    let live = topo.live_edges() as u32;
    let layers = hext_layers(g);
    let next = if *cursor + 1 >= layers {
        return;
    } else {
        *cursor + 1
    };
    let read = |(i, j, t): (i32, i32, u32)| -> u32 {
        // Row starts are all 0 on this rectangular lattice, so i is the column.
        match g.resolve(i, j, t) {
            Some(c) => (f.state[c] as u32).min(k - 1),
            None => live.min(k - 1),
        }
    };
    let row: Vec<u8> = (0..g.w as i32)
        .map(|u| {
            let [nw, n, ne] = hext_parents(u, next);
            table[((read(nw) * k + read(n)) * k + read(ne)) as usize]
        })
        .collect();
    let (j, t) = ((next / 2) as i32, next % 2);
    for (u, v) in row.into_iter().enumerate() {
        let c = g.index(u as i32, j, t);
        let was = f.state[c];
        f.state[c] = v;
        f.age[c] = if v == 0 {
            0
        } else if was == v {
            f.age[c].saturating_add(1)
        } else {
            1
        };
    }
    *cursor = next;
}

/// Margolus block step on the square grid. Blocks start at even coordinates plus `phase` (0 or 1)
/// on both axes. Wrap edges wrap blocks across the seam on an axis of even length; on an odd
/// axis, and with dead or live edges, a block that would cross the edge is left as it is.
fn step_margolus(topo: &Topology, table: &[u8; 16], phase: i32, f: &mut Field) {
    let Some(g) = topo.rows() else { return };
    let (w, h) = (g.w as i32, g.h as i32);
    let (wrap_x, wrap_y) = (g.wrap && w % 2 == 0, g.wrap && h % 2 == 0);
    let a = &f.state;
    f.back
        .par_iter_mut()
        .enumerate()
        .with_min_len(MIN_PAR)
        .for_each(|(i, o)| {
            let (u, j) = ((i % w as usize) as i32, (i / w as usize) as i32);
            let (bx, by) = (
                phase + 2 * (u - phase).div_euclid(2),
                phase + 2 * (j - phase).div_euclid(2),
            );
            let fix = |x: i32, n: i32, wrap: bool| -> Option<i32> {
                if wrap {
                    Some(x.rem_euclid(n))
                } else if x < 0 || x >= n {
                    None
                } else {
                    Some(x)
                }
            };
            let cells = [(bx, by), (bx + 1, by), (bx, by + 1), (bx + 1, by + 1)];
            let mut idx = [0usize; 4];
            for (k, &(x, y)) in cells.iter().enumerate() {
                match (fix(x, w, wrap_x), fix(y, h, wrap_y)) {
                    (Some(x), Some(y)) => idx[k] = (y * w + x) as usize,
                    _ => {
                        *o = a[i];
                        return;
                    }
                }
            }
            let code = idx
                .iter()
                .enumerate()
                .fold(0u8, |c, (k, &c2)| c | ((a[c2] != 0) as u8) << k);
            let me = ((u - bx) + 2 * (j - by)) as u8;
            *o = (table[code as usize] >> me) & 1;
        });
    commit_states(f);
}

/// Rule table: neighbours in cyclic slot order; missing neighbours read as 0.
fn step_table(topo: &Topology, nb: Nb, t: &crate::table::Compiled, f: &mut Field) {
    let live = topo.live_edges() as u8;
    let a = &f.state;
    f.back
        .par_iter_mut()
        .enumerate()
        .with_min_len(MIN_PAR)
        .for_each(|(i, o)| {
            let d = topo.degree(i, nb);
            let mut buf = [0u8; 16];
            let n = d.min(16);
            for (k, slot) in buf.iter_mut().enumerate().take(n) {
                *slot = topo.slot(i, nb, k).map(|j| a[j]).unwrap_or(live);
            }
            *o = t.lookup(a[i], &buf[..n]).unwrap_or(a[i]);
        });
    commit_states(f);
}

/// Life-like (`states == 2`) and Generations rules. Only state-1 neighbours count.
fn step_bs(topo: &Topology, nb: Nb, birth: u32, survive: u32, states: u8, f: &mut Field) {
    let live = topo.live_edges();
    let a = &f.state;
    let age = &f.age;
    f.back
        .par_iter_mut()
        .zip(f.back_age.par_iter_mut())
        .enumerate()
        .with_min_len(MIN_PAR)
        .for_each(|(i, (o, oa))| {
            let s = a[i];
            let mut c = 0u32;
            topo.for_each_neighbor(i, nb, |j| c += (a[j] == 1) as u32);
            if live {
                c += topo.missing(i, nb);
            }
            let v = if s == 0 {
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
            };
            *o = v;
            *oa = if v == 1 {
                if s == 1 {
                    age[i].saturating_add(1).min(65000)
                } else {
                    1
                }
            } else if v != 0 {
                age[i]
            } else {
                0
            };
        });
    std::mem::swap(&mut f.state, &mut f.back);
    std::mem::swap(&mut f.age, &mut f.back_age);
}

/// Writes the next row from the cursor row, stopping at the bottom on any kind of edge.
fn step_wolfram(topo: &Topology, rule: u8, cursor: &mut u32, f: &mut Field) {
    let Some(g) = topo.rows() else { return };
    let (w, h) = (g.w as i32, g.h);
    let r = *cursor;
    let next = if r + 1 >= h { return } else { r + 1 };
    let live = topo.live_edges() as u32;
    let at = |u: i32| -> u32 {
        let u = if g.wrap { u.rem_euclid(w) } else { u };
        if u < 0 || u >= w {
            live
        } else {
            (f.state[g.index(u, r as i32, 0)] != 0) as u32
        }
    };
    let row: Vec<u8> = (0..w)
        .map(|u| (rule >> (at(u - 1) << 2 | at(u) << 1 | at(u + 1))) & 1)
        .collect();
    for (u, v) in row.into_iter().enumerate() {
        let k = g.index(u as i32, next as i32, 0);
        let was = f.state[k];
        f.state[k] = v;
        f.age[k] = if v == 0 {
            0
        } else if was != 0 {
            f.age[k].saturating_add(1)
        } else {
            1
        };
    }
    *cursor = next;
}

/// Turmites step in order. At an edge they bounce (turn around) or fall off (are removed).
/// On an aperiodic patch, falling off means stepping onto an open boundary tile.
fn step_turmite(
    topo: &Topology,
    mite: &crate::turmite::Mite,
    agents: &mut Vec<Agent>,
    edge: AgentEdge,
    f: &mut Field,
) {
    let n = mite.colors;
    let mut gone = false;
    for a in agents.iter_mut() {
        if a.heading == u16::MAX {
            continue;
        }
        let c = a.cell as usize;
        let deg = topo.degree(c, Nb::Edge);
        if deg == 0 {
            continue;
        }
        let s = f.state[c] % n;
        let e = mite.entry(a.state % mite.states, s);
        let h = (a.heading as usize % deg + e.turn.slots(deg)) % deg;
        let ns = e.write;
        a.state = e.next;
        f.state[c] = ns;
        f.age[c] = if ns != 0 { 1 } else { 0 };
        match topo.slot(c, Nb::Edge, h) {
            Some(nx) => {
                let d2 = topo.degree(nx, Nb::Edge);
                let back = (0..d2)
                    .find(|&k| topo.slot(nx, Nb::Edge, k) == Some(c))
                    .unwrap_or(0);
                a.cell = nx as u32;
                a.heading = ((back + d2 / 2) % d2) as u16;
                if edge == AgentEdge::FallOff && topo.is_open(nx) {
                    a.heading = u16::MAX;
                    gone = true;
                }
            }
            None if edge == AgentEdge::FallOff => {
                a.heading = u16::MAX;
                gone = true;
            }
            None => a.heading = ((h + deg / 2) % deg) as u16, // bounce off the edge
        }
    }
    if gone {
        agents.retain(|a| a.heading != u16::MAX);
    }
}

fn step_wireworld(topo: &Topology, nb: Nb, f: &mut Field) {
    let a = &f.state;
    f.back
        .par_iter_mut()
        .enumerate()
        .with_min_len(MIN_PAR)
        .for_each(|(i, o)| {
            *o = match a[i] {
                1 => 2,
                2 => 3,
                3 => {
                    let mut heads = 0;
                    topo.for_each_neighbor(i, nb, |j| heads += (a[j] == 1) as u32);
                    if heads == 1 || heads == 2 { 1 } else { 3 }
                }
                _ => 0,
            };
        });
    std::mem::swap(&mut f.state, &mut f.back);
}

/// Edmond's Spectre glider rule (arXiv:2609.22579, Table 7), as replayed by JETCARP's JavaScript prototype.
fn step_glider(topo: &Topology, nb: Nb, f: &mut Field) {
    let a = &f.state;
    f.back
        .par_iter_mut()
        .zip(f.back_age.par_iter_mut())
        .enumerate()
        .with_min_len(MIN_PAR)
        .for_each(|(i, (o, oa))| {
            let s = a[i];
            let mut c = [0u32; 4];
            topo.for_each_neighbor(i, nb, |j| c[(a[j] & 3) as usize] += 1);
            let v = if s == 0 && c[0] >= 3 && c[1] >= 1 {
                3
            } else if s == 3 && c[3] >= 3 {
                0
            } else if s == 3 && c[1] >= 1 {
                1
            } else {
                0
            };
            *o = v;
            *oa = (v != 0) as u16;
        });
    std::mem::swap(&mut f.state, &mut f.back);
    std::mem::swap(&mut f.age, &mut f.back_age);
}

/// Does a state-1/state-3 pair at (t, u) move at least 25 units within 150 generations,
/// staying small (a glider, not an explosion)?
pub(crate) fn glider_travels(
    topo: &Topology,
    nb: Nb,
    t: usize,
    u: usize,
    cs: &[crate::geom::P],
) -> bool {
    let mut f = Field::new(FieldKind::Multi(4), topo.len(), false);
    f.state[t] = 1;
    f.state[u] = 3;
    let mut far = 0.0f64;
    for _ in 0..150 {
        step_glider(topo, nb, &mut f);
        let mut pop = 0;
        for (i, &s) in f.state.iter().enumerate() {
            if s != 0 {
                pop += 1;
                far = far.max(crate::geom::dist(cs[i], cs[t]));
                if topo.is_open(i) {
                    return far > 25.0;
                }
            }
        }
        if pop == 0 || pop > 40 {
            return false;
        }
    }
    far > 25.0
}

/// One synchronous toppling sweep. Each unstable cell topples `h / degree` times at once.
/// Grains sent past a dead edge or into an open boundary cell are counted in `lost`.
/// Returns the number of cells that toppled.
pub(crate) fn step_sandpile(topo: &Topology, nb: Nb, f: &mut Field) -> u64 {
    let h = &f.height;
    let explicit = matches!(topo.storage, Storage::Explicit(_));
    let fires = |j: usize| -> u32 {
        if explicit && topo.is_open(j) {
            return 0;
        }
        let d = topo.degree(j, nb) as u32;
        h[j].checked_div(d).unwrap_or(0)
    };
    let (lost, toppled): (u64, u64) = f
        .back_h
        .par_iter_mut()
        .enumerate()
        .with_min_len(MIN_PAR)
        .map(|(i, o)| {
            if explicit && topo.is_open(i) {
                *o = 0;
                return (h[i] as u64, 0);
            }
            let d = topo.degree(i, nb) as u32;
            let t = fires(i);
            let mut inc = 0u32;
            let mut kept = 0u32;
            topo.for_each_neighbor(i, nb, |j| {
                inc += fires(j);
                kept += !(explicit && topo.is_open(j)) as u32;
            });
            *o = h[i] - t * d + inc;
            (t as u64 * (d - kept) as u64, (t > 0) as u64)
        })
        .reduce(|| (0, 0), |a, b| (a.0 + b.0, a.1 + b.1));
    f.lost += lost;
    std::mem::swap(&mut f.height, &mut f.back_h);
    toppled
}

/// Three clock loops of different periods feeding long wires, on the square grid.
pub(crate) fn wire_demo(g: &crate::topology::LatticeGrid, f: &mut Field) {
    let (w, h) = (g.w as i32, g.h as i32);
    let mut put = |u: i32, j: i32, v: u32| {
        if u >= 0 && j >= 0 && u < w && j < h {
            f.set(g.index(u, j, 0), v);
        }
    };
    let loops = [(3, 6, 4), (3, 4, 6), (3, 10, 4)];
    let mut y = h / 6;
    for (k, &(x0, lw, lh)) in loops.iter().enumerate() {
        // Rectangle loop of conductor.
        for u in x0..x0 + lw {
            put(u, y, 3);
            put(u, y + lh - 1, 3);
        }
        for j in y..y + lh {
            put(x0, j, 3);
            put(x0 + lw - 1, j, 3);
        }
        // An electron: head then tail, moving clockwise.
        put(x0 + 1, y, 1);
        put(x0, y, 2);
        // Output wire from the right side, running to the far edge.
        let out_y = y + lh / 2;
        for u in x0 + lw..w - 2 {
            put(u, out_y, 3);
        }
        // A diode-like spur every so often to make the flow visible.
        if k == 1 {
            for j in out_y + 1..out_y + 6 {
                put(w / 2, j, 3);
            }
        }
        y += lh + h / 6;
    }
}

/// Layered totalistic rule: write layer `cursor + 1` from layer `cursor`. Layers are BFS distance
/// (over the CA's neighbourhood) from the cells that were nonzero at the first step; with none,
/// from the centre cell, which is set to 1.
fn step_layered(
    topo: &Topology,
    nb: Nb,
    digits: &[u8],
    layers: &mut Option<std::sync::Arc<Vec<u32>>>,
    cursor: &mut u32,
    f: &mut Field,
) {
    if layers.is_none() {
        let n = topo.len();
        let mut dist = vec![u32::MAX; n];
        let mut q = std::collections::VecDeque::new();
        for (i, d) in dist.iter_mut().enumerate() {
            if f.state[i] != 0 {
                *d = 0;
                q.push_back(i);
            }
        }
        if q.is_empty() {
            let c = topo.center_cell();
            f.state[c] = 1;
            f.age[c] = 1;
            dist[c] = 0;
            q.push_back(c);
        }
        while let Some(i) = q.pop_front() {
            let d = dist[i];
            topo.for_each_neighbor(i, nb, |j| {
                if dist[j] == u32::MAX {
                    dist[j] = d + 1;
                    q.push_back(j);
                }
            });
        }
        *layers = Some(std::sync::Arc::new(dist));
        *cursor = 0;
    }
    let dist = layers.as_ref().unwrap().clone();
    let next = *cursor + 1;
    let a = &f.state;
    let m = digits.len();
    let updates: Vec<(usize, u8)> = (0..topo.len())
        .into_par_iter()
        .with_min_len(MIN_PAR)
        .filter(|&i| dist[i] == next)
        .map(|i| {
            let mut sum = 0usize;
            topo.for_each_neighbor(i, nb, |j| {
                if dist[j] == *cursor {
                    sum += a[j] as usize;
                }
            });
            (i, digits[sum % m])
        })
        .collect();
    if updates.is_empty() {
        return;
    }
    for (i, v) in updates {
        let was = f.state[i];
        f.state[i] = v;
        f.age[i] = if v == 0 {
            0
        } else if was == v {
            f.age[i].saturating_add(1)
        } else {
            1
        };
    }
    *cursor = next;
}
