//! Splitting compound records into the independent objects they are made of.
//!
//! A record is a compound when it is several objects that never influence each other: two
//! gliders flying side by side, a pulsar with blinkers beside it, two blocks a cell apart. The
//! test is [`crate::objects::compound`]'s: each part runs alone as an object of the record's
//! period and velocity, and the record's own phases are, at every generation, exactly the union
//! of the parts' solo runs. It runs in the record's own rule, on its tiling (the unbounded
//! lattice, with the bit-parallel stepper for B/S and Generations rules and the world's stepper
//! on dead-edged frames otherwise) or, for a record found on an aperiodic patch, at its site on
//! that patch. Puffers, guns and movers are left as they are.
//!
//! The object finder ([`crate::objects::find_objects`]) and the hunter already report parts;
//! this module splits records made before that (`jetcarp library compounds`) and is a last check
//! when an object becomes a record ([`records_from_object`]).

use super::site::{match_site, patch_topology};
use super::verify::rule_world;
use super::{
    DiscoveredBy, PatternClass, PatternRecord, Provenance, RecordParts, build_record,
    record_from_object,
};
use crate::hunt::bits::Plan;
use crate::hunt::separate::BitRunner;
use crate::objects::compound::{self, Runner};
use crate::objects::{LCell, Object, Phase};
use crate::topology::Topology;
use crate::world::World;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

/// One part of a compound pattern: its class, period, displacement per period (zero unless a
/// spaceship), and one period of phases (in the record's coordinates; site indices on aperiodic
/// patches).
#[derive(Clone, Debug, PartialEq)]
pub struct Part {
    pub class: PatternClass,
    pub period: u64,
    pub d: [i32; 2],
    pub phases: Vec<Phase>,
}

fn class_of(period: u64, d: [i32; 2]) -> PatternClass {
    crate::hunt::separate::class_of(period, d)
}

/// The parts of a compound still life, oscillator, or spaceship, or `None` when it is one object
/// (or not a class that is split).
pub fn split_compound(rec: &PatternRecord) -> Result<Option<Vec<Part>>, String> {
    if !matches!(
        rec.class,
        PatternClass::StillLife | PatternClass::Oscillator | PatternClass::Spaceship
    ) {
        return Ok(None);
    }
    let phases = rec.all_phases()?;
    if phases.len() as u64 != rec.period.max(1) || phases.iter().any(|p| p.is_empty()) {
        return Ok(None);
    }
    let d = match rec.class {
        PatternClass::Spaceship => rec.displacement.unwrap_or([0, 0]),
        _ => [0, 0],
    };
    // One touching component in every phase: nothing to split (and nothing to build a runner
    // for, which most records need not).
    if phases.iter().all(|p| p.len() < 2) {
        return Ok(None);
    }
    let r = record_runner(rec)?;
    if phases.iter().all(|p| compound::atoms(&*r, p).len() < 2) {
        return Ok(None);
    }
    Ok(compound::split(&*r, &phases, d).map(|ps| {
        ps.into_iter()
            .map(|cp| Part {
                class: class_of(cp.period, cp.d),
                period: cp.period,
                d: cp.d,
                phases: cp.phases,
            })
            .collect()
    }))
}

/// A small dead-edged grid of a record's tiling (cached by tiling).
fn small_topology(rec: &PatternRecord) -> Result<Arc<Topology>, String> {
    static CACHE: OnceLock<Mutex<HashMap<String, Arc<Topology>>>> = OnceLock::new();
    let key = super::tiling_key(&rec.tiling);
    let cache = CACHE.get_or_init(Default::default);
    if let Some(t) = cache.lock().unwrap().get(&key) {
        return Ok(t.clone());
    }
    let t = Arc::new(crate::hunt::separate::small_topology(&rec.tiling)?);
    let mut c = cache.lock().unwrap();
    if c.len() > 512 {
        c.clear();
    }
    c.insert(key, t.clone());
    Ok(t)
}

/// The bit-parallel plan for a record's rule on its lattice, when the stepper runs the rule
/// (cached).
fn plan_for(rec: &PatternRecord, topo: &Topology) -> Option<Arc<Plan>> {
    static CACHE: OnceLock<Mutex<HashMap<String, Option<Arc<Plan>>>>> = OnceLock::new();
    if rec.steps_per_gen != 1 {
        return None;
    }
    let key = format!(
        "{}|{:?}|{}",
        super::tiling_key(&rec.tiling),
        rec.neighborhood,
        rec.rule
    );
    let cache = CACHE.get_or_init(Default::default);
    if let Some(p) = cache.lock().unwrap().get(&key) {
        return p.clone();
    }
    let p = crate::rules::Rule::parse(&rec.rule)
        .ok()
        .and_then(|(rule, _)| Plan::new(&rule, topo.lattice()?, rec.neighborhood).ok())
        .map(Arc::new);
    let mut c = cache.lock().unwrap();
    if c.len() > 4096 {
        c.clear();
    }
    c.insert(key, p.clone());
    p
}

/// Runs a record's cells alone in its own rule: on the unbounded lattice, or at its site on its
/// aperiodic patch (cells are site indices there).
pub fn record_runner(rec: &PatternRecord) -> Result<Box<dyn Runner + Send + Sync>, String> {
    if let Some(site) = &rec.site {
        let topo = patch_topology(&site.patch)?;
        let to = match_site(&topo, site, site.anchor as usize)?;
        let from: HashMap<u32, i32> = to.iter().enumerate().map(|(k, &x)| (x, k as i32)).collect();
        let w = rule_world(rec, topo)?;
        let inner = crate::objects::runner_for(&w, Some(w.cas[0].id))?;
        return Ok(Box::new(SiteRunner { inner, to, from }));
    }
    let topo = small_topology(rec)?;
    if let Some(plan) = plan_for(rec, &topo) {
        return Ok(Box::new(BitRunner(plan)));
    }
    let w = rule_world(rec, topo)?;
    crate::objects::runner_for(&w, Some(w.cas[0].id))
}

/// A record's site on its patch: cells are site indices, run as patch cells.
struct SiteRunner {
    inner: Box<dyn Runner + Send + Sync>,
    /// Patch cell of each site tile.
    to: Vec<u32>,
    from: HashMap<u32, i32>,
}

impl SiteRunner {
    fn out(&self, v: &[LCell]) -> Option<Phase> {
        v.iter()
            .map(|c| {
                self.to.get(c.i as usize).map(|&x| LCell {
                    i: x as i32,
                    j: 0,
                    k: 0,
                    s: c.s,
                })
            })
            .collect()
    }
    fn back(&self, v: &[LCell]) -> Option<Phase> {
        let mut p: Phase = v
            .iter()
            .map(|c| {
                self.from.get(&(c.i as u32)).map(|&k| LCell {
                    i: k,
                    j: 0,
                    k: 0,
                    s: c.s,
                })
            })
            .collect::<Option<_>>()?;
        p.sort_unstable();
        Some(p)
    }
}

impl Runner for SiteRunner {
    fn run(&self, cells: &[LCell], gens: u64) -> Option<Vec<Phase>> {
        let run = self.inner.run(&self.out(cells)?, gens)?;
        run.iter().map(|ph| self.back(ph)).collect()
    }
    fn touching(&self, c: &LCell, out: &mut Vec<(i32, i32, u32)>) {
        let Some(&x) = self.to.get(c.i as usize) else {
            return;
        };
        let mut buf = Vec::new();
        self.inner.touching(
            &LCell {
                i: x as i32,
                j: 0,
                k: 0,
                s: c.s,
            },
            &mut buf,
        );
        out.extend(
            buf.iter()
                .filter_map(|&(y, _, _)| self.from.get(&(y as u32)).map(|&k| (k, 0, 0))),
        );
    }
    fn close(&self, a: &[LCell], b: &[LCell]) -> bool {
        match (self.out(a), self.out(b)) {
            (Some(a), Some(b)) => self.inner.close(&a, &b),
            _ => true,
        }
    }
    fn distance(&self, a: &[LCell], b: &[LCell]) -> f64 {
        match (self.out(a), self.out(b)) {
            (Some(a), Some(b)) => self.inner.distance(&a, &b),
            _ => 1e9,
        }
    }
}

/// Records (canonical form, id, rule range; not verified) for the parts of a compound record,
/// in its rule, on its tiling (at its site on an aperiodic patch). Parts get no name.
pub fn part_records(
    rec: &PatternRecord,
    parts: &[Part],
    provenance: Provenance,
    discovered_by: DiscoveredBy,
) -> Result<Vec<PatternRecord>, String> {
    let (topo, map): (Arc<Topology>, Option<Vec<u32>>) = match &rec.site {
        None => (small_topology(rec)?, None),
        Some(site) => {
            let t = patch_topology(&site.patch)?;
            let m = match_site(&t, site, site.anchor as usize)?;
            (t, Some(m))
        }
    };
    let isotropic = super::canon::isotropic(&rec.rule);
    parts
        .iter()
        .map(|p| {
            let phases: Vec<Phase> = match &map {
                None => p.phases.clone(),
                Some(m) => p
                    .phases
                    .iter()
                    .map(|ph| {
                        let mut v: Phase = ph
                            .iter()
                            .map(|c| LCell {
                                i: m[c.i as usize] as i32,
                                j: 0,
                                k: 0,
                                s: c.s,
                            })
                            .collect();
                        v.sort_unstable();
                        v
                    })
                    .collect(),
            };
            let what = RecordParts {
                rule: rec.rule.clone(),
                nb: rec.neighborhood,
                steps_per_gen: rec.steps_per_gen,
                isotropic,
                class: p.class,
                period: p.period,
                d: p.d,
                name: None,
            };
            build_record(&topo, &what, &phases, provenance.clone(), discovered_by)
        })
        .collect()
}

/// [`record_from_object`], with compound objects split into their independent parts (see the
/// module notes; the object finder has normally split them already). Parts get no name; a part
/// already in the library keeps its stored name when merged. Equal parts (two blocks) each
/// count.
pub fn records_from_object(
    world: &World,
    ca_id: u32,
    obj: &Object,
    provenance: Provenance,
    discovered_by: DiscoveredBy,
) -> Result<Vec<PatternRecord>, String> {
    let rec = record_from_object(world, ca_id, obj, provenance.clone(), discovered_by)?;
    // Objects on aperiodic patches were split by the object finder, on the world itself.
    if obj.known_name || rec.site.is_some() {
        return Ok(vec![rec]);
    }
    let Some(parts) = split_compound(&rec)? else {
        return Ok(vec![rec]);
    };
    part_records(&rec, &parts, provenance, discovered_by)
}
