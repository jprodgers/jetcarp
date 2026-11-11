//! Soup search: run seeded random soups (the census's soups), take the objects at the end with
//! `objects::find_objects`, verify each distinct one, and return library records.
//!
//! Every (rule, tiling, soup) run is a pure function of its inputs and results are gathered in
//! run order ([`Gathered`]), so the report and the records are identical at any thread count and
//! in any batches: [`search`] runs them all at once; a search job (`crate::jobs::search`) runs
//! them a batch per step, its checkpoint the gathered state.

use jetcarp_core::census::{CensusSpec, soup_world};
use jetcarp_core::objects::{ObjClass, ObjectOptions, find_objects};
use jetcarp_core::pattern::split::records_from_object;
use jetcarp_core::pattern::{
    DiscoveredBy, PatternClass, PatternRecord, Provenance, default_periods, now_utc,
    record_from_object, verify,
};
use jetcarp_core::topology::{Boundary, Nb, SizeSpec, TilingSpec, Topology};
use jetcarp_core::world::{Command, Status};
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

/// A search over rules x tilings x soups.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SearchSpec {
    pub rules: Vec<String>,
    /// Tiling identities (family, params); size and boundary come from `size`/`boundary`.
    pub tilings: Vec<TilingSpec>,
    pub size: SizeSpec,
    pub boundary: Boundary,
    /// Neighbourhood for every rule; `None` lets the notation decide (`V` = edge).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub neighborhood: Option<Nb>,
    pub soups: u32,
    pub density: f64,
    /// Generations per soup (a soup that settles earlier stops there).
    pub gens: u64,
    /// Soup `s` uses RNG state `seed + s`.
    pub seed: u64,
    /// Longest period looked for.
    pub max_period: u64,
    /// A soup whose population passes this fraction of the cells is abandoned as exploded.
    pub explode_fraction: f64,
    /// Fill only a disc at the centre (radius as a fraction of half the shorter side, like the
    /// `blob` seed) instead of the whole grid. With dead edges, spaceships that escape the soup
    /// are caught near the edge, cataloged, and removed (as soup searches on the infinite plane
    /// do); without this, wrapped grids keep ships until they crash into debris.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub soup_radius: Option<f64>,
}

impl Default for SearchSpec {
    fn default() -> Self {
        SearchSpec {
            rules: vec!["B3/S23".into()],
            tilings: vec![TilingSpec::new("square")],
            size: SizeSpec::Rect { w: 64, h: 64 },
            boundary: Boundary::Wrap,
            neighborhood: None,
            soups: 16,
            density: 0.37,
            gens: 2000,
            seed: 1,
            max_period: 64,
            explode_fraction: 0.5,
            soup_radius: None,
        }
    }
}

/// One distinct pattern found in a (rule, tiling) group.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Found {
    pub id: String,
    pub label: String,
    pub class: PatternClass,
    pub period: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub speed: Option<String>,
    pub cells: usize,
    /// Copies found over all soups.
    pub count: u64,
    /// Soups it appeared in.
    pub soups: u32,
    pub verified: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// Set by the caller after storing: true if it was not in the library before.
    #[serde(default)]
    pub new: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GroupReport {
    pub rule: String,
    pub tiling: String,
    pub tiling_spec: TilingSpec,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    pub soups: u32,
    /// Soups by how they ended: settled (empty or exactly periodic), active, exploded.
    pub settled: u32,
    pub active: u32,
    pub exploded: u32,
    /// Objects by class over all soups (`still life`, `oscillator`, `spaceship`, `other (dies)`,
    /// ...).
    pub objects: BTreeMap<String, u64>,
    /// Distinct patterns, most common first.
    pub found: Vec<Found>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SearchReport {
    pub jetcarp_version: String,
    pub spec: SearchSpec,
    pub groups: Vec<GroupReport>,
}

pub struct SearchOutput {
    pub report: SearchReport,
    /// Verified records, one per distinct pattern, with `seen` = copies found.
    pub records: Vec<PatternRecord>,
    /// Soups each record appeared in (parallel to `records`).
    pub soups: Vec<u32>,
}

/// What one soup run left.
pub struct SoupOut {
    ended: u8, // 0 settled, 1 active, 2 exploded
    objects: Vec<(String, Option<PatternRecord>)>,
    error: Option<String>,
}

/// Run the search on the current rayon pool. `progress(done, total)` is called as soups finish.
pub fn search(
    spec: &SearchSpec,
    progress: Option<&(dyn Fn(usize, usize) + Sync)>,
) -> Result<SearchOutput, String> {
    search_cancellable(spec, progress, None)
}

/// [`search`] that gives up (an error, nothing stored) soon after `cancel` is set.
pub fn search_cancellable(
    spec: &SearchSpec,
    progress: Option<&(dyn Fn(usize, usize) + Sync)>,
    cancel: Option<&std::sync::atomic::AtomicBool>,
) -> Result<SearchOutput, String> {
    let s = SoupSearch::new(spec)?;
    let mut g = Gathered::new(&s, now_utc());
    let outs = s.run(&g.date, 0..s.runs(), progress, cancel);
    if cancel.is_some_and(|c| c.load(Ordering::Relaxed)) {
        return Err("search cancelled".into());
    }
    g.add(&s, outs);
    g.verify(g.distinct());
    Ok(g.output(spec))
}

/// A bounded search ready to run: its spec checked, a topology per tiling, and its soup runs,
/// numbered (tiling, rule) group by group (tilings outer), soups inner. Run `k` is a pure
/// function of the spec, `k`, and the search's date (in the records' provenance).
pub struct SoupSearch {
    spec: SearchSpec,
    topos: Vec<Result<Arc<Topology>, String>>,
    /// (tiling, rule) per group.
    groups: Vec<(usize, usize)>,
}

impl SoupSearch {
    pub fn new(spec: &SearchSpec) -> Result<SoupSearch, String> {
        if spec.soups == 0 || spec.gens == 0 {
            return Err("a search needs at least one soup and one generation".into());
        }
        if spec.rules.is_empty() || spec.tilings.is_empty() {
            return Err("a search needs at least one rule and one tiling".into());
        }
        if !(0.0..=1.0).contains(&spec.density) {
            return Err("density must be between 0 and 1".into());
        }
        // Topologies, one per tiling.
        let topos: Vec<Result<Arc<Topology>, String>> = spec
            .tilings
            .par_iter()
            .map(|t| {
                let mut s = t.clone();
                s.size = Some(spec.size);
                s.boundary = spec.boundary;
                if jetcarp_core::families::family(&s.family)
                    .is_some_and(|f| f.kind == jetcarp_core::families::FamilyKind::Aperiodic)
                {
                    // Aperiodic patches: a size given as a level is used; otherwise their default.
                    s.size = Some(match spec.size {
                        SizeSpec::Level(l) => SizeSpec::Level(l),
                        _ => SizeSpec::Level(jetcarp_core::aperiodic::default_level(&s.family) + 1),
                    });
                    s.boundary = Boundary::Dead;
                }
                Topology::build(&s).map(Arc::new)
            })
            .collect();
        let groups: Vec<(usize, usize)> = (0..spec.tilings.len())
            .flat_map(|t| (0..spec.rules.len()).map(move |r| (t, r)))
            .collect();
        Ok(SoupSearch {
            spec: spec.clone(),
            topos,
            groups,
        })
    }

    /// Soup runs in all (groups x soups).
    pub fn runs(&self) -> usize {
        self.groups.len() * self.spec.soups as usize
    }

    /// Runs `range`, in parallel on the current rayon pool, in order. `progress(done, total)`
    /// is called as they finish; once `cancel` is set the rest are skipped (and the results
    /// are not to be used).
    pub fn run(
        &self,
        date: &str,
        range: std::ops::Range<usize>,
        progress: Option<&(dyn Fn(usize, usize) + Sync)>,
        cancel: Option<&std::sync::atomic::AtomicBool>,
    ) -> Vec<SoupOut> {
        let total = self.runs();
        let done = AtomicUsize::new(range.start);
        range
            .collect::<Vec<usize>>()
            .par_iter()
            .map(|&k| {
                if cancel.is_some_and(|c| c.load(Ordering::Relaxed)) {
                    return SoupOut {
                        ended: 0,
                        objects: vec![],
                        error: Some("cancelled".into()),
                    };
                }
                let out = self.run_one(date, k);
                let d = done.fetch_add(1, Ordering::Relaxed) + 1;
                if let Some(p) = progress {
                    p(d, total);
                }
                out
            })
            .collect()
    }

    /// Run `k` (on this thread).
    pub fn run_one(&self, date: &str, k: usize) -> SoupOut {
        let spec = &self.spec;
        let (gi, soup) = (k / spec.soups as usize, (k % spec.soups as usize) as u32);
        let (t, r) = self.groups[gi];
        match &self.topos[t] {
            Ok(topo) => run_soup(spec, topo, &spec.rules[r], soup, date),
            Err(e) => SoupOut {
                ended: 0,
                objects: vec![],
                error: Some(e.clone()),
            },
        }
    }
}

/// What a search has found so far, gathered run by run in run order, whatever the batches
/// (a search job's checkpoint): per group its report, and each distinct pattern's record from
/// the first soup that found it. Once every run is in, the distinct patterns are verified (in
/// any batches) and [`output`](Self::output) gives exactly what one whole run gives.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Gathered {
    /// When the search started (its records' provenance date).
    pub date: String,
    /// Runs gathered.
    pub next: usize,
    reports: Vec<GroupReport>,
    /// The first find of each distinct pattern of a group, and where its report lists it
    /// (group, found index).
    unique: Vec<PatternRecord>,
    at: Vec<(usize, usize)>,
    /// Distinct patterns verified (once every run is in), and those that failed.
    verified: usize,
    failed: Vec<(usize, String)>,
    /// Per group: a pattern id's found index.
    #[serde(skip)]
    index: Vec<HashMap<String, usize>>,
}

impl Gathered {
    pub fn new(s: &SoupSearch, date: String) -> Gathered {
        let spec = &s.spec;
        let reports: Vec<GroupReport> = s
            .groups
            .iter()
            .map(|&(t, r)| GroupReport {
                rule: spec.rules[r].clone(),
                tiling: jetcarp_core::pattern::tiling_display_name(&spec.tilings[t]),
                tiling_spec: spec.tilings[t].clone(),
                error: None,
                soups: spec.soups,
                settled: 0,
                active: 0,
                exploded: 0,
                objects: BTreeMap::new(),
                found: vec![],
            })
            .collect();
        Gathered {
            date,
            index: vec![HashMap::new(); reports.len()],
            reports,
            ..Default::default()
        }
    }

    /// One read back from a checkpoint (its index rebuilt); it must be of this search.
    pub fn restored(mut self, s: &SoupSearch) -> Result<Gathered, String> {
        if self.reports.len() != s.groups.len() || self.next > s.runs() {
            return Err("the checkpoint does not match the search".into());
        }
        self.index = self
            .reports
            .iter()
            .map(|g| {
                g.found
                    .iter()
                    .enumerate()
                    .map(|(k, f)| (f.id.clone(), k))
                    .collect()
            })
            .collect();
        Ok(self)
    }

    /// Distinct patterns (over all groups).
    pub fn distinct(&self) -> usize {
        self.unique.len()
    }
    /// Distinct patterns verified so far.
    pub fn verified(&self) -> usize {
        self.verified
    }

    /// Gather the next runs (the outputs of runs `next..`, in order). When the last run is in,
    /// each distinct pattern's record gets its copies as `seen`.
    pub fn add(&mut self, s: &SoupSearch, outs: Vec<SoupOut>) {
        let soups = s.spec.soups as usize;
        for o in outs {
            let gi = self.next / soups;
            self.next += 1;
            let g = &mut self.reports[gi];
            if let Some(e) = o.error {
                g.error.get_or_insert(e);
                continue;
            }
            match o.ended {
                0 => g.settled += 1,
                1 => g.active += 1,
                _ => g.exploded += 1,
            }
            let idx = &mut self.index[gi];
            let mut in_soup: Vec<String> = Vec::new();
            for (cls, rec) in o.objects {
                *g.objects.entry(cls).or_default() += 1;
                let Some(rec) = rec else { continue };
                let k = *idx.entry(rec.id.clone()).or_insert_with(|| {
                    g.found.push(Found {
                        id: rec.id.clone(),
                        label: rec.label(),
                        class: rec.class,
                        period: rec.period,
                        speed: rec.speed.clone(),
                        cells: rec.cells,
                        count: 0,
                        soups: 0,
                        verified: false,
                        error: None,
                        new: false,
                    });
                    self.unique.push(rec.clone());
                    self.at.push((gi, g.found.len() - 1));
                    g.found.len() - 1
                });
                g.found[k].count += 1;
                if !in_soup.contains(&rec.id) {
                    in_soup.push(rec.id.clone());
                    g.found[k].soups += 1;
                }
            }
        }
        if self.next == s.runs() {
            // Seen counts, before verification.
            for (u, &(gi, k)) in self.at.iter().enumerate() {
                self.unique[u].seen = self.reports[gi].found[k].count;
                self.unique[u].rescore();
            }
        }
    }

    /// Verify the next `n` distinct patterns (in parallel on the current rayon pool); every
    /// run must be in.
    pub fn verify(&mut self, n: usize) {
        let (a, b) = (self.verified, (self.verified + n).min(self.unique.len()));
        let checked: Vec<Result<jetcarp_core::pattern::Verification, String>> = self.unique[a..b]
            .par_iter()
            .map(|rec| verify(rec, default_periods(rec.period)))
            .collect();
        for (u, r) in (a..b).zip(checked) {
            match r {
                Ok(v) => self.unique[u].verified = v,
                Err(e) => self.failed.push((u, e)),
            }
        }
        self.verified = b;
    }

    /// The search's report and verified records, once every run is in and every distinct
    /// pattern verified.
    pub fn output(mut self, spec: &SearchSpec) -> SearchOutput {
        let failed: HashMap<usize, String> = std::mem::take(&mut self.failed).into_iter().collect();
        let mut records = Vec::new();
        let mut soups = Vec::new();
        for (u, rec) in self.unique.into_iter().enumerate() {
            let (gi, k) = self.at[u];
            let f = &mut self.reports[gi].found[k];
            match failed.get(&u) {
                None => {
                    f.verified = true;
                    soups.push(f.soups);
                    records.push(rec);
                }
                Some(e) => f.error = Some(e.clone()),
            }
        }
        for g in &mut self.reports {
            g.found
                .sort_by(|a, b| b.count.cmp(&a.count).then(a.label.cmp(&b.label)));
        }
        SearchOutput {
            report: SearchReport {
                jetcarp_version: jetcarp_core::VERSION.into(),
                spec: spec.clone(),
                groups: self.reports,
            },
            records,
            soups,
        }
    }
}

fn class_key(c: &ObjClass) -> String {
    match c {
        ObjClass::Other { reason } => format!("other ({reason})"),
        c => c.kind().to_string(),
    }
}

fn run_soup(spec: &SearchSpec, topo: &Arc<Topology>, rule: &str, soup: u32, date: &str) -> SoupOut {
    let cs = CensusSpec {
        tiling: topo.spec.clone(),
        neighborhood: spec.neighborhood,
        rules: vec![rule.to_string()],
        soups: spec.soups,
        density: spec.density,
        gens: spec.gens,
        seed: spec.seed,
        explode_fraction: spec.explode_fraction,
        activity_window: 10,
    };
    let fail = |e: String| SoupOut {
        ended: 0,
        objects: vec![],
        error: Some(e),
    };
    let made = match spec.soup_radius {
        None => soup_world(topo, &cs, rule, soup),
        Some(radius) => blob_world(topo, &cs, rule, soup, radius),
    };
    let mut w = match made {
        Ok(w) => w,
        Err(e) => return fail(e),
    };
    if let Err(e) = jetcarp_core::objects::supported(&w.cas[0].rule) {
        return fail(e);
    }
    let prov = Provenance {
        source: "search".into(),
        tiling: Some(topo.spec.clone()),
        rng_state: Some(spec.seed.wrapping_add(soup as u64)),
        soup: Some(soup as u64),
        density: Some(spec.density),
        generation: None,
        jetcarp_version: jetcarp_core::VERSION.into(),
        date: date.to_string(),
    };
    let n = topo.len() as f64;
    let fi = w.cas[0].field;
    let mut ended = 1;
    // Dead edges: catch spaceships in a band along the edge before they hit it.
    let band = match (spec.boundary, topo.lattice()) {
        (Boundary::Dead, Some(_)) => {
            jetcarp_core::pattern::verify::lattice_info(&topo.spec, w.cas[0].nb)
                .map(|i| 4 * i.reach + 8)
                .ok()
        }
        _ => None,
    };
    let mut objects: Vec<(String, Option<PatternRecord>)> = Vec::new();
    for _ in 0..spec.gens {
        if let Err(e) = w.apply(Command::Step { gens: 1 }) {
            return fail(e);
        }
        if let Some(b) = band
            && w.generation % 8 == 0
            && let Err(e) = catch_escapes(&mut w, b, spec.max_period, &prov, &mut objects)
        {
            return fail(e);
        }
        match w.status() {
            Status::Repeats { .. } | Status::Empty => {
                ended = 0;
                break;
            }
            Status::Running => {}
        }
        if w.fields[fi].population() as f64 > spec.explode_fraction * n {
            ended = 2;
            break;
        }
    }
    if ended == 2 {
        return SoupOut {
            ended,
            objects,
            error: None,
        };
    }
    let opts = ObjectOptions {
        max_period: spec.max_period,
        ..Default::default()
    };
    let rep = match find_objects(&w, &opts) {
        Ok(r) => r,
        Err(e) => return fail(e),
    };
    let ca_id = w.cas[0].id;
    let prov = Provenance {
        generation: Some(w.generation),
        ..prov
    };
    for o in &rep.objects {
        match o.class {
            ObjClass::Other { .. } => objects.push((class_key(&o.class), None)),
            _ => match records_from_object(&w, ca_id, o, prov.clone(), DiscoveredBy::Search) {
                Ok(parts) => {
                    for r in parts {
                        objects.push((r.class.name().to_string(), Some(r)));
                    }
                }
                Err(_) => objects.push((class_key(&o.class), None)),
            },
        }
    }
    SoupOut {
        ended,
        objects,
        error: None,
    }
}

/// A soup filling only a disc at the centre (the `blob` seed), RNG state `seed + soup`.
fn blob_world(
    topo: &Arc<Topology>,
    cs: &CensusSpec,
    rule: &str,
    soup: u32,
    radius: f64,
) -> Result<jetcarp_core::world::World, String> {
    let mut w =
        jetcarp_core::world::World::on_topology(topo.clone(), cs.seed.wrapping_add(soup as u64));
    let mut ca = jetcarp_core::world::CaSpec::new(rule).seed(jetcarp_core::world::SeedSpec::Blob {
        radius,
        density: cs.density,
    });
    ca.neighborhood = cs.neighborhood;
    w.apply(Command::AddCa { ca })?;
    Ok(w)
}

/// Spaceships in the band of width `band` along a dead edge are cataloged and erased, so they
/// neither crash into the edge nor leave debris there. Only spaceships are taken.
fn catch_escapes(
    w: &mut jetcarp_core::world::World,
    band: i32,
    max_period: u64,
    prov: &Provenance,
    out: &mut Vec<(String, Option<PatternRecord>)>,
) -> Result<(), String> {
    let topo = w.topo.clone();
    let g = topo.lattice().unwrap();
    let fi = w.cas[0].field;
    let (gw, gh) = (g.w as i32, g.h as i32);
    let edge: Vec<[u32; 2]> = w.fields[fi]
        .state
        .iter()
        .enumerate()
        .filter(|(_, s)| **s != 0)
        .filter(|(x, _)| {
            let (u, j, _) = g.decompose(*x);
            u < band || u >= gw - band || j < band || j >= gh - band
        })
        .map(|(x, &s)| [x as u32, s as u32])
        .collect();
    // A soup that has spread to the edge is not a few escaping ships: leave it alone.
    if edge.is_empty() || edge.len() > 200 {
        return Ok(());
    }
    let ca = &w.cas[0];
    let mut spec = jetcarp_core::world::CaSpec::new(&ca.rule.notation())
        .seed(jetcarp_core::world::SeedSpec::Empty);
    spec.neighborhood = Some(ca.nb);
    spec.steps_per_gen = ca.steps_per_gen;
    let mut w2 = jetcarp_core::world::World::on_topology(topo.clone(), 1);
    w2.apply(Command::AddCa { ca: spec })?;
    let id2 = w2.cas[0].id;
    w2.apply(Command::SetCells {
        id: id2,
        cells: edge,
    })?;
    let rep = find_objects(
        &w2,
        &ObjectOptions {
            max_period,
            ..Default::default()
        },
    )?;
    let prov = Provenance {
        source: "search (escaped)".into(),
        generation: Some(w.generation),
        ..prov.clone()
    };
    let mut erase = Vec::new();
    for o in &rep.objects {
        if let ObjClass::Spaceship { .. } = o.class {
            let rec = record_from_object(&w2, id2, o, prov.clone(), DiscoveredBy::Search).ok();
            out.push((class_key(&o.class), rec));
            erase.extend(o.cells.iter().map(|&c| [c, 0]));
        }
    }
    if !erase.is_empty() {
        let id = w.cas[0].id;
        w.apply(Command::SetCells { id, cells: erase })?;
    }
    Ok(())
}
