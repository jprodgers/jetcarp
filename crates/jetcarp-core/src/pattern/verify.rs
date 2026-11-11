//! Exact verification of library patterns, and the dead-edged frames used to run a pattern
//! alone (also used by rendering, collisions, and promotion).
//!
//! A lattice pattern is pasted on an empty dead-edged grid that holds its whole path for the
//! periods checked, plus a margin wider than one generation's reach. Every generation the live
//! cells are compared exactly (position, tile type, state) with the stored phase, shifted by the
//! displacement for spaceships. If a live cell ever sits next to the frame edge the check fails,
//! so a passing run is the same as the run on the infinite plane (the rules stored have no B0, so
//! empty space stays empty). Aperiodic patterns run on the patch they were found on, pasted at
//! their site, and must stay off the patch's open boundary.

use super::{PatternRecord, now_utc};
use crate::objects::{LCell, Phase};
use crate::topology::{Boundary, Nb, SizeSpec, TilingSpec, Topology};
use crate::world::{CaSpec, Command, SeedSpec, World};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

/// How a record was verified.
#[derive(Clone, Debug, PartialEq, Default, Serialize, Deserialize)]
pub struct Verification {
    /// Periods run with every generation matching exactly (0 = not verified).
    pub periods: u64,
    pub generations: u64,
    /// `dead-edged frame WxH` or `patch <family> level L`.
    pub field: String,
    /// Aperiodic patterns: the sites it was run at (its own and other matching ones).
    #[serde(default, skip_serializing_if = "is_zero")]
    pub sites: u32,
    /// Records written by CART, JETCARP's predecessor, call this `cart_version`.
    #[serde(alias = "cart_version")]
    pub jetcarp_version: String,
    pub date: String,
}

fn is_zero(x: &u32) -> bool {
    *x == 0
}

/// Periods checked by default: 16 for short periods, fewer for long ones (at least 4, and at
/// least 64 generations when the period allows).
pub fn default_periods(period: u64) -> u64 {
    (128 / period.max(1)).clamp(4, 16)
}

/// Facts about a lattice tiling used for placing patterns on frames.
#[derive(Clone, Debug)]
pub struct LatticeInfo {
    /// `a2.x / a1.x`: row `j` starts at lattice column `-floor(j * shear)`.
    pub shear: f64,
    /// How far one generation can reach, in rows or row positions (as in `objects`).
    pub reach: i32,
    pub ntypes: u32,
    pub a1: [f64; 2],
    pub a2: [f64; 2],
}

fn small_spec(t: &TilingSpec) -> TilingSpec {
    let mut s = super::tiling_identity(t);
    s.size = Some(SizeSpec::Rect { w: 8, h: 8 });
    s.boundary = Boundary::Dead;
    s
}

/// Lattice facts for a tiling identity and neighbourhood (cached).
pub fn lattice_info(tiling: &TilingSpec, nb: Nb) -> Result<LatticeInfo, String> {
    static CACHE: OnceLock<Mutex<HashMap<String, LatticeInfo>>> = OnceLock::new();
    let key = format!("{}|{nb:?}", super::tiling_key(tiling));
    let cache = CACHE.get_or_init(Default::default);
    if let Some(i) = cache.lock().unwrap().get(&key) {
        return Ok(i.clone());
    }
    let topo = Topology::build(&small_spec(tiling))?;
    let g = topo
        .lattice()
        .ok_or_else(|| format!("{} is not a lattice tiling", tiling.family))?;
    let maxoff = g.offsets[nb as usize]
        .iter()
        .flatten()
        .map(|o| o.di.abs().max(o.dj.abs()))
        .max()
        .unwrap_or(1)
        .max(1);
    let shear = g.a2[0] / g.a1[0];
    let info = LatticeInfo {
        shear,
        reach: maxoff + (maxoff as f64 * shear.abs()).ceil() as i32 + 1,
        ntypes: g.ntypes,
        a1: g.a1,
        a2: g.a2,
    };
    cache.lock().unwrap().insert(key, info.clone());
    Ok(info)
}

/// A dead-edged (or wrapped) lattice grid with lattice cells placed on it by a fixed translation.
pub struct Frame {
    pub topo: Arc<Topology>,
    /// Lattice translation added to pattern coordinates: frame cell = pattern cell + (di, dj).
    pub di: i32,
    pub dj: i32,
}

fn row_start(shear: f64, j: i64) -> i64 {
    -((j as f64 * shear + 1e-9).floor() as i64)
}

/// Topologies for frames, cached by tiling and size.
fn frame_topology(
    tiling: &TilingSpec,
    w: u32,
    h: u32,
    boundary: Boundary,
) -> Result<Arc<Topology>, String> {
    static CACHE: OnceLock<Mutex<HashMap<String, Arc<Topology>>>> = OnceLock::new();
    let cache = CACHE.get_or_init(Default::default);
    let key = format!("{}|{w}x{h}|{boundary:?}", super::tiling_key(tiling));
    if let Some(t) = cache.lock().unwrap().get(&key) {
        return Ok(t.clone());
    }
    let mut spec = super::tiling_identity(tiling);
    spec.size = Some(SizeSpec::Rect { w, h });
    spec.boundary = boundary;
    let t = Arc::new(Topology::build(&spec)?);
    let mut c = cache.lock().unwrap();
    if c.len() > 256 {
        c.clear();
    }
    c.insert(key, t.clone());
    Ok(t)
}

impl Frame {
    /// A frame holding every cell of `envelope` with `margin` empty rows and row positions
    /// around it. Sizes are rounded up to multiples of 8 so frames are shared.
    pub fn around(
        tiling: &TilingSpec,
        info: &LatticeInfo,
        envelope: &[LCell],
        margin: i32,
    ) -> Result<Frame, String> {
        if envelope.is_empty() {
            return Err("empty pattern".into());
        }
        let m = margin as i64;
        let jmin = envelope.iter().map(|c| c.j).min().unwrap() as i64;
        let jmax = envelope.iter().map(|c| c.j).max().unwrap() as i64;
        let dj = m - jmin;
        let h = ((jmax - jmin + 1 + 2 * m) as u32).div_ceil(8) * 8;
        let q: Vec<i64> = envelope
            .iter()
            .map(|c| c.i as i64 - row_start(info.shear, c.j as i64 + dj))
            .collect();
        let (qmin, qmax) = (*q.iter().min().unwrap(), *q.iter().max().unwrap());
        let di = m - qmin;
        let w = ((qmax - qmin + 1 + 2 * m) as u32).div_ceil(8) * 8;
        if (w as u64) * (h as u64) * info.ntypes as u64 > 64_000_000 {
            return Err(format!("frame of {w}x{h} unit cells is too large"));
        }
        let topo = frame_topology(tiling, w, h, Boundary::Dead)?;
        Ok(Frame {
            topo,
            di: di as i32,
            dj: dj as i32,
        })
    }

    /// Frame cell index of a pattern cell.
    pub fn index(&self, c: &LCell) -> Option<usize> {
        let g = self.topo.lattice()?;
        let j = c.j + self.dj;
        if j < 0 || j >= g.h as i32 {
            return None;
        }
        let u = c.i + self.di - g.row_start[j as usize];
        if u < 0 || u >= g.w as i32 || c.k >= g.ntypes {
            return None;
        }
        Some(g.index(u, j, c.k))
    }

    /// `[cell, state]` pairs for pasting.
    pub fn cells(&self, v: &[LCell]) -> Result<Vec<[u32; 2]>, String> {
        v.iter()
            .map(|c| {
                self.index(c)
                    .map(|x| [x as u32, c.s as u32])
                    .ok_or_else(|| "pattern cell outside the frame (internal error)".to_string())
            })
            .collect()
    }

    /// The live cells of a state field in pattern coordinates (sorted), and whether any of them
    /// is missing a neighbour (touches the frame edge).
    pub fn live(&self, state: &[u8], nb: Nb) -> (Phase, bool) {
        let g = self.topo.lattice().unwrap();
        let mut edge = false;
        let mut v = Vec::new();
        for (x, &s) in state.iter().enumerate() {
            if s != 0 {
                let (u, j, k) = g.decompose(x);
                edge |= self.topo.missing(x, nb) > 0;
                v.push(LCell {
                    i: u + g.row_start[j as usize] - self.di,
                    j: j - self.dj,
                    k,
                    s,
                });
            }
        }
        v.sort_unstable();
        (v, edge)
    }
}

/// A world with one CA running the record's rule (empty field) on `topo`.
pub fn rule_world(rec: &PatternRecord, topo: Arc<Topology>) -> Result<World, String> {
    let mut w = World::on_topology(topo, 1);
    let ca = CaSpec::new(&rec.rule)
        .nb(rec.neighborhood)
        .steps(rec.steps_per_gen)
        .seed(SeedSpec::Empty);
    w.apply(Command::AddCa { ca })?;
    Ok(w)
}

/// Every cell the pattern occupies over `gens` generations (phases shifted by the displacement).
pub fn envelope(rec: &PatternRecord, gens: u64) -> Result<Phase, String> {
    let p = rec.period.max(1);
    let mut v = Vec::new();
    let phases = rec.all_phases()?;
    let d = rec.displacement.unwrap_or([0, 0]);
    // Corners suffice: every phase at its first and last position.
    let last = gens / p;
    for n in [0, last] {
        for ph in &phases {
            for c in ph {
                v.push(LCell {
                    i: c.i + d[0] * n as i32,
                    j: c.j + d[1] * n as i32,
                    ..*c
                });
            }
        }
    }
    Ok(v)
}

/// A puffer: run its starting phase alone on the unbounded lattice and require the exact front
/// recurrence with the stored period and displacement (see `objects::puffer`).
pub fn verify_puffer(rec: &PatternRecord) -> Result<Verification, String> {
    use crate::objects::puffer::{CHECK_PERIODS, Sparse, detect};
    let rule = crate::rules::Rule::parse(&rec.rule)?.0;
    let topo = Topology::build(&small_spec(&rec.tiling))?;
    let g = topo.lattice().ok_or("puffers live on lattice tilings")?;
    let sp = Sparse::new(&rule, g, rec.neighborhood)
        .ok_or("the puffer test does not support this rule")?;
    let d = rec
        .displacement
        .ok_or("a puffer record needs its displacement")?;
    let start = rec.phase(0)?;
    let f = detect(&sp, &start, rec.period.max(1), 50_000)
        .ok_or("its front does not recur while the debris grows: not a puffer")?;
    if f.period != rec.period || f.d != d {
        return Err(format!(
            "the front recurs every {} generations shifted by {:?}, the record says {} and {:?}",
            f.period, f.d, rec.period, d
        ));
    }
    Ok(Verification {
        periods: CHECK_PERIODS,
        generations: f.generations,
        field: format!(
            "unbounded lattice (sparse run): a {}-cell front recurs every {} generations shifted by ({}, {}) over the last {} periods while the population grows ({} to {})",
            f.front_cells, f.period, d[0], d[1], CHECK_PERIODS, f.pop_from, f.pop_to
        ),
        sites: 0,
        jetcarp_version: crate::VERSION.into(),
        date: now_utc(),
    })
}

/// Run the pattern alone for `periods` periods and compare every generation exactly with the
/// stored phases. Returns the verification record, or why it failed.
pub fn verify(rec: &PatternRecord, periods: u64) -> Result<Verification, String> {
    if rec.class == super::PatternClass::Mover {
        return super::movers::verify_mover(rec);
    }
    if rec.class == super::PatternClass::Puffer {
        return verify_puffer(rec);
    }
    if rec.class == super::PatternClass::Gun {
        return crate::hunt::gun::verify_gun(rec);
    }
    let periods = periods.max(1);
    let p = rec.period.max(1);
    if rec.phases.len() as u64 != p {
        return Err(format!("{} phases stored for period {p}", rec.phases.len()));
    }
    let gens = periods * p;
    if gens > 100_000 {
        return Err("verification would run more than 100,000 generations".into());
    }
    if let Some(site) = &rec.site {
        return super::site::verify_on_patch(rec, site, periods);
    }
    let info = lattice_info(&rec.tiling, rec.neighborhood)?;
    if info.ntypes != rec.tile_types {
        return Err(format!(
            "the tiling has {} tile types per unit cell, the record says {}",
            info.ntypes, rec.tile_types
        ));
    }
    let env = envelope(rec, gens)?;
    let frame = Frame::around(&rec.tiling, &info, &env, info.reach + 1)?;
    let mut w = rule_world(rec, frame.topo.clone())?;
    let id = w.cas[0].id;
    let start = rec.phase(0)?;
    if start.is_empty() {
        return Err("phase 0 is empty".into());
    }
    w.apply(Command::SetCells {
        id,
        cells: frame.cells(&start)?,
    })?;
    let fi = w.cas[0].field;
    let nb = rec.neighborhood;
    for t in 1..=gens {
        w.apply(Command::Step { gens: 1 })?;
        let (live, edge) = frame.live(&w.fields[fi].state, nb);
        let want = rec.phase(t)?;
        if live != want {
            return Err(format!(
                "generation {t}: {} live cells, expected {} as stored for phase {} (not a {} of period {p})",
                live.len(),
                want.len(),
                t % p,
                super::PatternClass::name(rec.class)
            ));
        }
        if edge {
            return Err(format!(
                "generation {t}: the pattern reached the frame edge"
            ));
        }
    }
    let g = frame.topo.lattice().unwrap();
    Ok(Verification {
        periods,
        generations: gens,
        field: format!("dead-edged frame {}x{} unit cells", g.w, g.h),
        sites: 0,
        jetcarp_version: crate::VERSION.into(),
        date: now_utc(),
    })
}
