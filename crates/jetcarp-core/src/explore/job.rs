//! The search loop: a GPU loop for rules a batch can run, a CPU loop for the rest, a writer
//! thread, and periodic confirmation of the best runs on bigger grids.

use super::archive::{self, Archive, Eval};
use super::batch::{BatchJob, BatchWorld, WorldTrace, trace_world};
use super::cand::Cand;
use super::features::{Fate, Features, frames_for, measure_run};
use super::seeds::seed_for;
use super::space::{self, ALL_FAMS, Fam, Grid, TilingChoice, sample_ltl, sample_rule};
use crate::ltl::Hood;
use crate::rng::Rng;
use crate::rules::Rule;
use crate::topology::Nb;
use crate::world::World;
use rayon::prelude::*;
use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant};

/// A batch backend (the GPU stepper in `jetcarp-gpu`).
pub trait Backend: Send {
    fn name(&self) -> String;
    fn supports(&self, rule: &Rule) -> bool;
    fn max_cells(&self) -> usize;
    fn run(&mut self, job: &BatchJob) -> Result<Vec<WorldTrace>, String>;
    /// Start over after errors (a lost device). Returns false if that is impossible.
    fn reset(&mut self) -> bool {
        false
    }
}

/// Search settings.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Cfg {
    pub db: PathBuf,
    pub hours: f64,
    /// Stop at this Unix time instead (restarts keep the original deadline).
    pub until: Option<u64>,
    pub screen_cells: u32,
    pub screen_gens: u32,
    pub confirm_cells: u32,
    pub confirm_gens: u32,
    /// Worlds in a GPU batch.
    pub batch: usize,
    pub cpu_threads: usize,
    pub max_rss_gb: f64,
    pub confirm_seeds: u32,
    pub seed: u64,
}

impl Default for Cfg {
    fn default() -> Self {
        Cfg {
            db: PathBuf::from("explore.sqlite"),
            hours: 1.0,
            until: None,
            screen_cells: 16_000,
            screen_gens: 1024,
            confirm_cells: 64_000,
            confirm_gens: 4096,
            batch: 1024,
            cpu_threads: 12,
            max_rss_gb: 72.0,
            confirm_seeds: 3,
            seed: 0,
        }
    }
}

const WINDOW: usize = 17;
/// Neighbour reads one Larger than Life batch may do (a few seconds of GPU time).
const LTL_NEIGHBOUR_UPDATES: f64 = 1.5e12;

/// GPU sampling weights.
const GPU_FAMS: &[(Fam, f64)] = &[
    (Fam::LifeLike, 30.0),
    (Fam::Generations, 25.0),
    (Fam::Jaca, 10.0),
    (Fam::Cyclic, 4.0),
    (Fam::Rps, 4.0),
    (Fam::GreenbergHastings, 4.0),
    (Fam::Hodgepodge, 4.0),
    (Fam::Priority, 8.0),
    (Fam::WireWorld, 1.0),
    (Fam::SpectreGlider, 1.0),
];

/// CPU sampling weights.
const CPU_FAMS: &[(Fam, f64)] = &[
    (Fam::Turmite, 25.0),
    (Fam::Layered, 20.0),
    (Fam::Wolfram, 8.0),
    (Fam::HexWolfram, 8.0),
    (Fam::HexTri, 8.0),
    (Fam::Margolus, 20.0),
    (Fam::Lenia, 3.0),
    (Fam::ForestFire, 3.0),
    (Fam::Sandpile, 4.0),
    (Fam::Table, 1.0),
];

fn weighted(rng: &mut Rng, w: &[(Fam, f64)]) -> Fam {
    let t: f64 = w.iter().map(|x| x.1).sum();
    let mut r = rng.next_f64() * t;
    for (f, v) in w {
        r -= v;
        if r <= 0.0 {
            return *f;
        }
    }
    w[0].0
}

/// A row for the writer.
struct RowOut {
    id: i64,
    stage: u8,
    parent: Option<i64>,
    ev: Eval,
    seed_kind: String,
}

/// Search-wide state.
struct Shared {
    cfg: Cfg,
    tilings: Vec<TilingChoice>,
    tiling_total: f64,
    grids: Mutex<HashMap<(String, u32), Arc<Grid>>>,
    archive: Mutex<Archive>,
    seen: Mutex<HashSet<u64>>,
    coverage: Mutex<HashMap<(usize, Fam, String), u32>>,
    named: Mutex<Vec<(usize, String)>>,
    next_id: AtomicI64,
    screened: AtomicU64,
    confirmed: AtomicU64,
    stop: AtomicBool,
    deadline: Instant,
    t_start: Instant,
    tx: mpsc::SyncSender<Vec<RowOut>>,
}

impl Shared {
    fn grid(&self, key: &str, cells: u32) -> Result<Arc<Grid>, String> {
        if let Some(g) = self.grids.lock().unwrap().get(&(key.to_string(), cells)) {
            return Ok(g.clone());
        }
        let g = Arc::new(Grid::build(key, cells)?);
        self.grids
            .lock()
            .unwrap()
            .insert((key.to_string(), cells), g.clone());
        Ok(g)
    }

    fn pick_tiling(&self, rng: &mut Rng) -> usize {
        let mut r = rng.next_f64() * self.tiling_total;
        for (i, t) in self.tilings.iter().enumerate() {
            r -= t.weight;
            if r <= 0.0 {
                return i;
            }
        }
        0
    }

    /// A distinct tiling with the fewest screened runs of `fams`; ties broken at random.
    fn least_tried_tiling(&self, rng: &mut Rng, fams: &[(Fam, f64)]) -> usize {
        let mut per = vec![0u64; self.tilings.len()];
        for ((ti, fam, _), n) in self.coverage.lock().unwrap().iter() {
            if fams.iter().any(|(f, _)| f == fam) {
                per[*ti] += *n as u64;
            }
        }
        let len = self.tilings.len();
        let start = rng.below(len as u64) as usize;
        (0..len)
            .map(|i| (start + i) % len)
            .filter(|&i| self.tilings[i].twin_of.is_none())
            .min_by_key(|&i| per[i])
            .unwrap_or(0)
    }

    /// The family in `fams` with the fewest screened runs.
    fn least_tried_fam(&self, fams: &[Fam]) -> Fam {
        let mut per: HashMap<Fam, u64> = HashMap::new();
        for ((_, fam, _), n) in self.coverage.lock().unwrap().iter() {
            *per.entry(*fam).or_insert(0) += *n as u64;
        }
        *fams
            .iter()
            .min_by_key(|f| per.get(f).copied().unwrap_or(0))
            .expect("at least one family")
    }

    fn done(&self) -> bool {
        self.stop.load(Ordering::Relaxed)
            || Instant::now() >= self.deadline
            || stop_file(&self.cfg.db).exists()
    }

    fn group(&self, key: &str) -> u8 {
        self.tilings
            .iter()
            .position(|t| t.key == key)
            .and_then(|p| group_index(&self.tilings[p].group))
            .unwrap_or(0)
    }

    fn tiling_index(&self, key: &str) -> usize {
        self.tilings.iter().position(|t| t.key == key).unwrap_or(0)
    }
}

fn group_index(g: &str) -> Option<u8> {
    Some(match g {
        "square" => 0,
        "hexagon" => 1,
        "triangle" => 2,
        "pentagon" => 3,
        "archimedean" => 4,
        "k-uniform" => 5,
        "isohedral" => 6,
        "aperiodic" => 7,
        _ => return None,
    })
}

/// The tiling a grid-specific family is searched on. Weighted tiling picks starve these: some
/// are structural twins of another tiling and carry the twin discount.
fn native_tiling(f: Fam) -> Option<&'static str> {
    match f {
        Fam::Wolfram | Fam::Margolus | Fam::Table => Some("square"),
        Fam::HexWolfram => Some("hex"),
        Fam::HexTri => Some("hex_flat"),
        _ => None,
    }
}

fn stop_file(db: &Path) -> PathBuf {
    db.with_extension("stop")
}

/// Generations and steps for a family on a grid.
fn run_len(fam: Fam, grid: &Grid, gens: u32) -> (u32, u32) {
    let side = (grid.topo.len() as f64).sqrt() as u32;
    match fam {
        Fam::Wolfram | Fam::HexWolfram | Fam::HexTri | Fam::Layered => (side * 3, 1),
        Fam::Turmite => (gens.min(1024), 64),
        Fam::Sandpile => (gens.min(1024), 32),
        Fam::Lenia => (gens.min(400), 1),
        _ => (gens, 1),
    }
}

/// Make candidates for a grid. `ltl` fixes a Larger than Life group: only those are made.
fn emit(
    s: &Shared,
    grid: &Grid,
    nb: Nb,
    fams: &[(Fam, f64)],
    ltl: Option<(Hood, u8)>,
    n: usize,
    gens: u32,
    rng: &mut Rng,
    backend: Option<&dyn Fn(&Rule) -> bool>,
) -> Vec<(Cand, String)> {
    let mut out: Vec<(Cand, String)> = Vec::with_capacity(n);
    let ti = s.tiling_index(&grid.key);
    let mut tries = 0;
    while out.len() < n && tries < n * 12 {
        tries += 1;
        let fam = if ltl.is_some() {
            Fam::Ltl
        } else {
            weighted(rng, fams)
        };
        if !fam.fits(grid) {
            continue;
        }
        let roll = rng.next_f64();
        let (rule, kind, seed, origin): (Option<Rule>, String, crate::world::SeedSpec, &str) =
            if roll < 0.4 {
                let r = match ltl {
                    Some(g) => sample_ltl(grid, rng, Some(g)),
                    None => sample_rule(fam, grid, nb, rng),
                };
                let (sd, k) = seed_for(fam, grid, rng);
                (r, k.to_string(), sd, "random")
            } else if roll < 0.65 {
                let el = s
                    .archive
                    .lock()
                    .unwrap()
                    .sample(rng, Some(fam))
                    .map(|(_, e)| e.clone());
                let Some(el) = el else { continue };
                let Some(base) = el.cand.rule() else { continue };
                if let Some(g) = ltl
                    && !matches!(&base, Rule::Ltl(l) if (l.hood, l.range) == g)
                {
                    continue;
                }
                let r = space::mutate_rule(&base, grid, nb, rng);
                (r, "mutated".to_string(), el.cand.seed.clone(), "mutate")
            } else if roll < 0.8 {
                let el = s
                    .archive
                    .lock()
                    .unwrap()
                    .sample(rng, Some(fam))
                    .map(|(_, e)| e.clone());
                let Some(el) = el else { continue };
                let Some(base) = el.cand.rule() else { continue };
                if let Some(g) = ltl
                    && !matches!(&base, Rule::Ltl(l) if (l.hood, l.range) == g)
                {
                    continue;
                }
                let (sd, k) = seed_for(fam, grid, rng);
                (Some(base), k.to_string(), sd, "seed")
            } else if roll < 0.9 {
                // The least-tried seed kind of this family on this grid.
                let mut best: Option<(u32, crate::world::SeedSpec, &'static str)> = None;
                let cov = s.coverage.lock().unwrap();
                for _ in 0..6 {
                    let (sd, k) = seed_for(fam, grid, rng);
                    let c = cov.get(&(ti, fam, k.to_string())).copied().unwrap_or(0);
                    if best.as_ref().is_none_or(|b| c < b.0) {
                        best = Some((c, sd, k));
                    }
                }
                drop(cov);
                let (_, sd, k) = best.unwrap();
                let r = match ltl {
                    Some(g) => sample_ltl(grid, rng, Some(g)),
                    None => sample_rule(fam, grid, nb, rng),
                };
                (r, k.to_string(), sd, "cover")
            } else {
                // A named rule on this grid.
                let named = {
                    let mut q = s.named.lock().unwrap();
                    if q.is_empty() {
                        continue;
                    }
                    let i = rng.below(q.len() as u64) as usize;
                    q.swap_remove(i)
                };
                let Ok((r, _)) = Rule::parse(&named.1) else {
                    continue;
                };
                let f2 = Fam::of(&r);
                // A rule this loop cannot run goes back for the other loop.
                if ltl.is_some()
                    || backend.is_some_and(|b| !b(&r))
                    || !fams.iter().any(|(f, _)| *f == f2)
                {
                    s.named.lock().unwrap().push(named);
                    continue;
                }
                if !f2.fits(grid) {
                    continue;
                }
                let (sd, k) = seed_for(f2, grid, rng);
                (Some(r), k.to_string(), sd, "named")
            };
        let Some(rule) = rule else { continue };
        if backend.is_some_and(|b| !b(&rule)) {
            continue;
        }
        let fam = Fam::of(&rule);
        let (g2, steps) = run_len(fam, grid, gens);
        let cand = Cand {
            tiling: grid.key.clone(),
            nb: if fam.nb_matters() { nb } else { Nb::Touch },
            fam,
            rule: rule.notation(),
            seed,
            cells: grid.cells_target,
            gens: g2,
            steps,
            rng: rng.next_u64() >> 8,
            origin: origin.to_string(),
        };
        if !s.seen.lock().unwrap().insert(cand.id_hash()) {
            continue;
        }
        out.push((cand, kind));
    }
    out
}

/// A world seeded for a candidate.
fn prepare(grid: &Grid, c: &Cand) -> Result<World, String> {
    let mut w = World::on_topology(grid.topo.clone(), c.rng);
    w.add_ca(c.ca())?;
    Ok(w)
}

fn evaluate(grid: &Grid, c: &Cand, t: &WorldTrace) -> (Fate, Features, f32, u32) {
    let window = if c.fam.is_rows() { 1 } else { WINDOW };
    let (fate, f) = measure_run(grid, t, c.gens, window);
    let score = archive::score(fate, &f, c.gens, c.fam);
    let bin = archive::behaviour_bin(fate, &f, c.gens);
    (fate, f, score, bin)
}

fn publish(s: &Shared, rows: Vec<RowOut>, stage: u8) {
    if stage == 1 {
        s.screened.fetch_add(rows.len() as u64, Ordering::Relaxed);
        let mut a = s.archive.lock().unwrap();
        let mut cov = s.coverage.lock().unwrap();
        for r in &rows {
            *cov.entry((
                s.tiling_index(&r.ev.cand.tiling),
                r.ev.cand.fam,
                r.seed_kind.clone(),
            ))
            .or_insert(0) += 1;
            a.offer(r.id, r.ev.clone(), s.group(&r.ev.cand.tiling));
        }
    } else {
        s.confirmed.fetch_add(rows.len() as u64, Ordering::Relaxed);
    }
    let _ = s.tx.send(rows);
}

/// Run one GPU batch of candidates on a grid.
fn gpu_batch(
    s: &Shared,
    be: &mut dyn Backend,
    grid: &Grid,
    nb: Nb,
    cands: Vec<(Cand, String, Option<i64>)>,
    stage: u8,
) -> Result<(), String> {
    if cands.is_empty() {
        return Ok(());
    }
    let gens = cands[0].0.gens;
    let trace = std::env::var_os("JETCARP_EXPLORE_TRACE").is_some();
    let t0 = Instant::now();
    let inits: Vec<Option<BatchWorld>> = cands
        .par_iter()
        .map(|(c, ..)| {
            let world = prepare(grid, c).ok()?;
            let rule = c.rule()?;
            Some(BatchWorld {
                rule,
                init: super::batch::field_bytes(world.fields.first()?),
            })
        })
        .collect();
    let (mut good, mut worlds) = (Vec::new(), Vec::new());
    for (c, w) in cands.into_iter().zip(inits) {
        if let Some(w) = w {
            good.push(c);
            worlds.push(w);
        }
    }
    if worlds.is_empty() {
        return Ok(());
    }
    let job = BatchJob {
        topo: grid.topo.clone(),
        nb,
        worlds,
        gens,
        frames_at: frames_for(gens, WINDOW),
    };
    let t1 = Instant::now();
    let traces = be.run(&job)?;
    let t2 = Instant::now();
    let rows: Vec<RowOut> = good
        .into_par_iter()
        .zip(traces.into_par_iter())
        .map(|((c, kind, parent), t)| {
            let (fate, feat, score, bin) = evaluate(grid, &c, &t);
            RowOut {
                id: s.next_id.fetch_add(1, Ordering::Relaxed),
                stage,
                parent,
                ev: Eval {
                    cand: c,
                    fate,
                    feat,
                    score,
                    bin,
                },
                seed_kind: kind,
            }
        })
        .collect();
    let t3 = Instant::now();
    let n = rows.len();
    publish(s, rows, stage);
    if trace {
        eprintln!(
            "batch stage {stage} {} worlds={n} cells={} gens={gens}: seed {:.2}s gpu {:.2}s measure {:.2}s publish {:.3}s",
            grid.key,
            grid.topo.len(),
            (t1 - t0).as_secs_f64(),
            (t2 - t1).as_secs_f64(),
            (t3 - t2).as_secs_f64(),
            t3.elapsed().as_secs_f64(),
        );
    }
    Ok(())
}

/// One screening batch on the GPU.
fn gpu_round(s: &Shared, be: &mut dyn Backend, rng: &mut Rng) -> Result<(), String> {
    let ltl_round = rng.below(8) == 0;
    // Some rounds go to the least-tried tiling, so every tiling is reached by the deadline.
    let ti = if ltl_round && rng.below(3) == 0 {
        s.least_tried_tiling(rng, &[(Fam::Ltl, 1.0)])
    } else if !ltl_round && rng.below(8) == 0 {
        s.least_tried_tiling(rng, GPU_FAMS)
    } else {
        s.pick_tiling(rng)
    };
    let key = s.tilings[ti].key.clone();
    let tg = Instant::now();
    let grid = s.grid(&key, s.cfg.screen_cells)?;
    if std::env::var_os("JETCARP_EXPLORE_TRACE").is_some() {
        eprintln!(
            "grid {key} {:.2}s; t={:.1}",
            tg.elapsed().as_secs_f64(),
            s.t_start.elapsed().as_secs_f64()
        );
    }
    let nb = if grid.has_edge && rng.below(4) == 0 {
        Nb::Edge
    } else {
        Nb::Touch
    };
    let mut max_worlds = (be.max_cells() / grid.topo.len().next_multiple_of(256))
        .min(s.cfg.batch)
        .max(1);
    let ltl = if ltl_round {
        let hood = if rng.below(4) == 0 {
            Hood::VonNeumann
        } else {
            Hood::Moore
        };
        // The largest range whose reach stays affordable (aperiodic reaches run slowly).
        let limit = if grid.topo.lattice().is_some() {
            180
        } else {
            60
        };
        let mut r = (2 + rng.below(5)) as u8;
        while r > 1 && grid.reach_size(hood, r) > limit {
            r -= 1;
        }
        let reach = grid.reach_size(hood, r).max(1);
        // Hold a Larger than Life batch to about the same work as a plain batch.
        let work = (grid.topo.len() * s.cfg.screen_gens as usize).max(1) as f64 * reach as f64;
        let cap = (LTL_NEIGHBOUR_UPDATES / work) as usize;
        max_worlds = max_worlds.min(cap.max(16));
        Some((hood, r))
    } else {
        None
    };
    let filter = |r: &Rule| be.supports(r);
    let te = Instant::now();
    let cands = emit(
        s,
        &grid,
        nb,
        GPU_FAMS,
        ltl,
        max_worlds,
        s.cfg.screen_gens,
        rng,
        Some(&filter),
    );
    if std::env::var_os("JETCARP_EXPLORE_TRACE").is_some() {
        eprintln!(
            "emit {:.2}s for {}",
            te.elapsed().as_secs_f64(),
            cands.len()
        );
    }
    let tagged = cands.into_iter().map(|(c, k)| (c, k, None)).collect();
    gpu_batch(s, be, &grid, nb, tagged, 1)
}

/// Confirm the best runs of one grid/neighbourhood group on the GPU.
fn gpu_confirm(s: &Shared, be: &mut dyn Backend, limit: usize) -> Result<usize, String> {
    let pending = s.archive.lock().unwrap().unconfirmed(4096);
    let mut first: Option<(String, Nb, Option<(Hood, u8)>)> = None;
    let mut picked: Vec<(i64, Eval)> = Vec::new();
    let key_of = |e: &Eval| -> Option<(String, Nb, Option<(Hood, u8)>)> {
        let r = e.cand.rule()?;
        if !be.supports(&r) {
            return None;
        }
        let l = if let Rule::Ltl(l) = &r {
            Some((l.hood, l.range))
        } else {
            None
        };
        Some((e.cand.tiling.clone(), e.cand.nb, l))
    };
    for (id, e) in pending {
        let Some(k) = key_of(&e) else { continue };
        match &first {
            None => first = Some(k),
            Some(f) if *f != k => continue,
            _ => {}
        }
        picked.push((id, e));
        if picked.len() >= limit {
            break;
        }
    }
    let Some((key, nb, _)) = first else {
        return Ok(0);
    };
    let grid = s.grid(&key, s.cfg.confirm_cells)?;
    let n = picked.len();
    let mut cands = Vec::new();
    for (id, e) in &picked {
        for sd in 0..s.cfg.confirm_seeds {
            let mut c = e.cand.clone();
            let (g2, st) = run_len(c.fam, &grid, s.cfg.confirm_gens);
            c.cells = s.cfg.confirm_cells;
            c.gens = g2;
            c.steps = st;
            c.rng = e.cand.rng.wrapping_add(sd as u64 + 1);
            c.origin = "confirm".into();
            cands.push((c, "confirm".to_string(), Some(*id)));
        }
    }
    gpu_batch(s, be, &grid, nb, cands, 2)?;
    let mut a = s.archive.lock().unwrap();
    for (id, _) in &picked {
        a.confirmed.insert(*id);
    }
    Ok(n)
}

fn gpu_loop(s: Arc<Shared>, mut be: Box<dyn Backend>, seed: u64) {
    let mut rng = Rng::new(seed);
    let mut round = 0u64;
    let mut fails = 0u32;
    while !s.done() {
        round += 1;
        let r = if round.is_multiple_of(8) {
            gpu_confirm(&s, be.as_mut(), 96).map(|_| ())
        } else {
            gpu_round(&s, be.as_mut(), &mut rng)
        };
        match r {
            Ok(()) => fails = 0,
            Err(e) => {
                eprintln!("gpu round {round}: {e}");
                fails += 1;
                std::thread::sleep(Duration::from_millis(500));
                if fails == 3 && !be.reset() {
                    eprintln!("the GPU could not be restarted");
                }
                if fails >= 40 {
                    // A supervisor restarts the process, which resumes from the database.
                    eprintln!("too many GPU errors: exiting");
                    std::process::exit(3);
                }
            }
        }
    }
}

fn cpu_loop(s: Arc<Shared>, seed: u64, fams: Vec<(Fam, f64)>) {
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(s.cfg.cpu_threads)
        .build()
        .expect("thread pool");
    let mut rng = Rng::new(seed);
    let mut round = 0u64;
    // Families that run on any tiling, and the ones tied to a native grid.
    let everywhere: Vec<(Fam, f64)> = fams
        .iter()
        .filter(|(f, _)| native_tiling(*f).is_none())
        .map(|(f, _)| (*f, 1.0))
        .collect();
    let native: Vec<Fam> = fams
        .iter()
        .map(|(f, _)| *f)
        .filter(|f| native_tiling(*f).is_some())
        .collect();
    while !s.done() {
        round += 1;
        // CPU rounds are few, so weighted picks alone would miss tilings. A quarter go to the
        // least-tried native-grid family, half sweep the least-tried tiling.
        let (ti, round_fams): (usize, Vec<(Fam, f64)>) = match round % 4 {
            0 if !native.is_empty() => {
                let f = s.least_tried_fam(&native);
                let key = native_tiling(f).expect("native family");
                (s.tiling_index(key), vec![(f, 1.0)])
            }
            1 | 2 if !everywhere.is_empty() => (
                s.least_tried_tiling(&mut rng, &everywhere),
                everywhere.clone(),
            ),
            _ => (s.pick_tiling(&mut rng), fams.clone()),
        };
        let key = s.tilings[ti].key.clone();
        let Ok(grid) = s.grid(&key, s.cfg.screen_cells) else {
            continue;
        };
        let nb = if grid.has_edge && rng.below(4) == 0 {
            Nb::Edge
        } else {
            Nb::Touch
        };
        let cands = emit(
            &s,
            &grid,
            nb,
            &round_fams,
            None,
            s.cfg.cpu_threads * 8,
            s.cfg.screen_gens,
            &mut rng,
            None,
        );
        let rows: Vec<RowOut> = pool.install(|| {
            cands
                .into_par_iter()
                .filter_map(|(c, kind)| cpu_eval(&s, &grid, c, kind, None, 1))
                .collect()
        });
        publish(&s, rows, 1);
        if round.is_multiple_of(4) {
            cpu_confirm(&s, &pool, &fams);
        }
    }
}

fn cpu_eval(
    s: &Shared,
    grid: &Grid,
    c: Cand,
    kind: String,
    parent: Option<i64>,
    stage: u8,
) -> Option<RowOut> {
    let mut world = prepare(grid, &c).ok()?;
    let window = if c.fam.is_rows() { 1 } else { WINDOW };
    let t = trace_world(&mut world, c.gens, &frames_for(c.gens, window)).ok()?;
    let (fate, feat, score, bin) = evaluate(grid, &c, &t);
    Some(RowOut {
        id: s.next_id.fetch_add(1, Ordering::Relaxed),
        stage,
        parent,
        ev: Eval {
            cand: c,
            fate,
            feat,
            score,
            bin,
        },
        seed_kind: kind,
    })
}

fn cpu_confirm(s: &Shared, pool: &rayon::ThreadPool, fams: &[(Fam, f64)]) {
    let pending = s.archive.lock().unwrap().unconfirmed(4096);
    let picked: Vec<(i64, Eval)> = pending
        .into_iter()
        .filter(|(_, e)| fams.iter().any(|(f, _)| *f == e.cand.fam))
        .take(s.cfg.cpu_threads * 2)
        .collect();
    let rows: Vec<RowOut> = pool.install(|| {
        picked
            .par_iter()
            .flat_map(|(id, e)| {
                let grid = s.grid(&e.cand.tiling, s.cfg.confirm_cells).ok()?;
                let rows: Vec<RowOut> = (0..s.cfg.confirm_seeds)
                    .filter_map(|sd| {
                        let mut c = e.cand.clone();
                        let (g2, st) = run_len(c.fam, &grid, s.cfg.confirm_gens);
                        c.cells = s.cfg.confirm_cells;
                        c.gens = g2;
                        c.steps = st;
                        c.rng = e.cand.rng.wrapping_add(sd as u64 + 1);
                        c.origin = "confirm".into();
                        cpu_eval(s, &grid, c, "confirm".into(), Some(*id), 2)
                    })
                    .collect();
                Some(rows)
            })
            .flatten()
            .collect()
    });
    let mut a = s.archive.lock().unwrap();
    for (id, _) in &picked {
        a.confirmed.insert(*id);
    }
    drop(a);
    publish(s, rows, 2);
}

fn open_db(path: &Path) -> Result<Connection, String> {
    let c = Connection::open(path).map_err(|e| e.to_string())?;
    c.execute_batch(
        "PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL; PRAGMA temp_store=MEMORY;
         CREATE TABLE IF NOT EXISTS runs(
           id INTEGER PRIMARY KEY, stage INT NOT NULL, parent INT, tiling TEXT NOT NULL, nb INT,
           fam TEXT NOT NULL, rule TEXT NOT NULL, seed TEXT NOT NULL, seed_kind TEXT, cells INT,
           gens INT, steps INT, rng INT, origin TEXT, fate TEXT, settle INT, period INT,
           score REAL, bin INT, feats BLOB, ts INT);
         CREATE INDEX IF NOT EXISTS runs_score ON runs(score);
         CREATE INDEX IF NOT EXISTS runs_parent ON runs(parent);
         CREATE TABLE IF NOT EXISTS reviews(run_id INT, verdict TEXT, note TEXT, ts INT);
         CREATE TABLE IF NOT EXISTS meta(k TEXT PRIMARY KEY, v TEXT);",
    )
    .map_err(|e| e.to_string())?;
    Ok(c)
}

fn feat_blob(f: &Features) -> Vec<u8> {
    bincode_like(f)
}

fn bincode_like(f: &Features) -> Vec<u8> {
    let v = [
        f.settle as f32,
        f.period as f32,
        f.density,
        f.density_late,
        f.activity_late,
        f.activity_early,
        f.pop_cv,
        f.growth,
        f.peak_density,
        f.entropy,
        f.bits,
        f.same_nb,
        f.smooth,
        f.comps_per_live,
        f.largest,
        f.size_entropy,
        f.patchiness,
        f.travel,
        f.movers as f32,
        f.mover_speed,
    ];
    v.iter().flat_map(|x| x.to_le_bytes()).collect()
}

/// Inverse of the stored blob.
pub fn feat_from_blob(b: &[u8]) -> Features {
    let g = |i: usize| -> f32 {
        b.get(i * 4..i * 4 + 4)
            .map(|s| f32::from_le_bytes(s.try_into().unwrap()))
            .unwrap_or(0.0)
    };
    Features {
        settle: g(0) as u32,
        period: g(1) as u32,
        density: g(2),
        density_late: g(3),
        activity_late: g(4),
        activity_early: g(5),
        pop_cv: g(6),
        growth: g(7),
        peak_density: g(8),
        entropy: g(9),
        bits: g(10),
        same_nb: g(11),
        smooth: g(12),
        comps_per_live: g(13),
        largest: g(14),
        size_entropy: g(15),
        patchiness: g(16),
        travel: g(17),
        movers: g(18) as u32,
        mover_speed: g(19),
    }
}

/// Run one candidate on the CPU now and measure it.
pub fn try_one(c: Cand) -> Result<Eval, String> {
    let grid = Grid::build(&c.tiling, c.cells)?;
    let mut world = prepare(&grid, &c)?;
    let window = if c.fam.is_rows() { 1 } else { WINDOW };
    let t = trace_world(&mut world, c.gens, &frames_for(c.gens, window))?;
    let (fate, feat, score, bin) = evaluate(&grid, &c, &t);
    Ok(Eval {
        cand: c,
        fate,
        feat,
        score,
        bin,
    })
}

fn fate_name(f: Fate) -> &'static str {
    match f {
        Fate::Died => "died",
        Fate::Froze => "froze",
        Fate::Cycle => "cycle",
        Fate::Running => "running",
    }
}

fn fate_parse(s: &str) -> Fate {
    match s {
        "died" => Fate::Died,
        "froze" => Fate::Froze,
        "cycle" => Fate::Cycle,
        _ => Fate::Running,
    }
}

fn writer(path: PathBuf, rx: mpsc::Receiver<Vec<RowOut>>) {
    let mut c = match open_db(&path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("writer: {e}");
            return;
        }
    };
    let now = || {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0)
    };
    while let Ok(batch) = rx.recv() {
        let mut batches = vec![batch];
        while let Ok(b) = rx.try_recv() {
            batches.push(b);
        }
        let tx = match c.transaction() {
            Ok(t) => t,
            Err(e) => {
                eprintln!("writer: {e}");
                continue;
            }
        };
        {
            let mut st = tx
                .prepare_cached(
                    "INSERT INTO runs(id,stage,parent,tiling,nb,fam,rule,seed,seed_kind,cells,gens,
                     steps,rng,origin,fate,settle,period,score,bin,feats,ts)
                     VALUES(?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?)",
                )
                .expect("prepare");
            let t = now();
            for b in batches.iter().flatten() {
                let e = &b.ev;
                let c = &e.cand;
                let seed = serde_json::to_string(&c.seed).unwrap_or_default();
                if let Err(err) = st.execute(params![
                    b.id,
                    b.stage,
                    b.parent,
                    c.tiling,
                    c.nb as u8,
                    c.fam.name(),
                    c.rule,
                    seed,
                    b.seed_kind,
                    c.cells,
                    c.gens,
                    c.steps,
                    c.rng as i64,
                    c.origin,
                    fate_name(e.fate),
                    e.feat.settle,
                    e.feat.period,
                    e.score as f64,
                    e.bin,
                    feat_blob(&e.feat),
                    t
                ]) {
                    eprintln!("writer: {err}");
                }
            }
        }
        if let Err(e) = tx.commit() {
            eprintln!("writer: {e}");
        }
    }
}

/// Recompute every stored score from the stored features, after [`archive::score`] changes.
/// Returns the number of runs. Run it while no search has the database open.
pub fn rescore(db: &Path) -> Result<usize, String> {
    let e = |x: rusqlite::Error| x.to_string();
    let mut c = Connection::open(db).map_err(e)?;
    c.busy_timeout(std::time::Duration::from_secs(60))
        .map_err(e)?;
    let scores: Vec<(i64, f64)> = {
        let mut st = c
            .prepare("SELECT id,fam,fate,gens,feats FROM runs")
            .map_err(e)?;
        let rows = st
            .query_map([], |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, i64>(3)?,
                    r.get::<_, Vec<u8>>(4)?,
                ))
            })
            .map_err(e)?;
        let mut out = Vec::new();
        for row in rows.flatten() {
            let (id, fam, fate, gens, feats) = row;
            let Some(fam) = ALL_FAMS.iter().copied().find(|f| f.name() == fam) else {
                continue;
            };
            let s = archive::score(fate_parse(&fate), &feat_from_blob(&feats), gens as u32, fam);
            out.push((id, s as f64));
        }
        out
    };
    let tx = c.transaction().map_err(e)?;
    tx.execute_batch("DROP INDEX IF EXISTS runs_score")
        .map_err(e)?;
    {
        let mut st = tx
            .prepare("UPDATE runs SET score=? WHERE id=?")
            .map_err(e)?;
        for (id, s) in &scores {
            st.execute(rusqlite::params![s, id]).map_err(e)?;
        }
    }
    tx.execute_batch("CREATE INDEX runs_score ON runs(score)")
        .map_err(e)?;
    tx.commit().map_err(e)?;
    Ok(scores.len())
}

/// Reload the archive, the seen set and coverage from a previous run.
fn resume(s: &Shared, c: &Connection) -> Result<(), String> {
    let fam_of = |n: &str| ALL_FAMS.iter().copied().find(|f| f.name() == n);
    let max: i64 = c
        .query_row("SELECT COALESCE(MAX(id),0) FROM runs", [], |r| r.get(0))
        .map_err(|e| e.to_string())?;
    s.next_id.store(max + 1, Ordering::Relaxed);
    let mut st = c
        .prepare(
            "SELECT id,tiling,nb,fam,rule,seed,seed_kind,cells,gens,steps,rng,origin,fate,score,bin,feats,stage
             FROM runs ORDER BY id",
        )
        .map_err(|e| e.to_string())?;
    let rows = st
        .query_map([], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, i64>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, String>(4)?,
                r.get::<_, String>(5)?,
                r.get::<_, Option<String>>(6)?,
                r.get::<_, i64>(7)?,
                r.get::<_, i64>(8)?,
                r.get::<_, i64>(9)?,
                r.get::<_, i64>(10)?,
                r.get::<_, String>(11)?,
                r.get::<_, String>(12)?,
                r.get::<_, f64>(13)?,
                r.get::<_, i64>(14)?,
                r.get::<_, Vec<u8>>(15)?,
                r.get::<_, i64>(16)?,
            ))
        })
        .map_err(|e| e.to_string())?;
    let mut arch = s.archive.lock().unwrap();
    let mut seen = s.seen.lock().unwrap();
    let mut cov = s.coverage.lock().unwrap();
    for row in rows.flatten() {
        let (
            id,
            tiling,
            nb,
            fam,
            rule,
            seed,
            kind,
            cells,
            gens,
            steps,
            rng,
            origin,
            fate,
            score,
            bin,
            feats,
            stage,
        ) = row;
        let Some(fam) = fam_of(&fam) else { continue };
        let Ok(seed) = serde_json::from_str(&seed) else {
            continue;
        };
        let cand = Cand {
            tiling: tiling.clone(),
            nb: if nb == 0 { Nb::Touch } else { Nb::Edge },
            fam,
            rule,
            seed,
            cells: cells as u32,
            gens: gens as u32,
            steps: steps as u32,
            rng: rng as u64,
            origin,
        };
        seen.insert(cand.id_hash());
        if stage != 1 {
            continue;
        }
        let ti = s.tiling_index(&tiling);
        *cov.entry((ti, fam, kind.unwrap_or_default())).or_insert(0) += 1;
        let ev = Eval {
            cand,
            fate: fate_parse(&fate),
            feat: feat_from_blob(&feats),
            score: score as f32,
            bin: bin as u32,
        };
        arch.offer(id, ev, s.group(&tiling));
    }
    drop(arch);
    drop(seen);
    drop(cov);
    let mut st = c
        .prepare("SELECT DISTINCT parent FROM runs WHERE parent IS NOT NULL")
        .map_err(|e| e.to_string())?;
    let ps: Vec<i64> = st
        .query_map([], |r| r.get(0))
        .map_err(|e| e.to_string())?
        .flatten()
        .collect();
    s.archive.lock().unwrap().confirmed.extend(ps);
    Ok(())
}

fn rss_gb() -> f64 {
    std::fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|t| {
            t.lines().find(|l| l.starts_with("VmRSS:")).and_then(|l| {
                l.split_whitespace()
                    .nth(1)
                    .and_then(|v| v.parse::<f64>().ok())
            })
        })
        .unwrap_or(0.0)
        / 1_048_576.0
}

#[derive(Serialize)]
struct Status {
    started: u64,
    now: u64,
    hours: f64,
    screened: u64,
    confirmed: u64,
    per_sec: f64,
    elites: usize,
    niches: usize,
    rss_gb: f64,
    backend: String,
}

fn status_loop(s: Arc<Shared>, backend: String, started: u64, t0: Instant) {
    let path = s.cfg.db.with_extension("status.json");
    let mut last = (0u64, Instant::now());
    let mut last_written = Instant::now() - Duration::from_secs(120);
    while !s.done() {
        std::thread::sleep(Duration::from_secs(2));
        let rss = rss_gb();
        if rss > s.cfg.max_rss_gb {
            eprintln!("memory {rss:.1} GB is over the cap: stopping");
            s.stop.store(true, Ordering::Relaxed);
        }
        if last_written.elapsed() < Duration::from_secs(30) {
            continue;
        }
        last_written = Instant::now();
        let screened = s.screened.load(Ordering::Relaxed);
        let (elites, niches) = {
            let a = s.archive.lock().unwrap();
            (a.runs.len(), a.cells.len())
        };
        let per_sec = (screened - last.0) as f64 / last.1.elapsed().as_secs_f64().max(1.0);
        last = (screened, Instant::now());
        let st = Status {
            started,
            now: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0),
            hours: t0.elapsed().as_secs_f64() / 3600.0,
            screened,
            confirmed: s.confirmed.load(Ordering::Relaxed),
            per_sec,
            elites,
            niches,
            rss_gb: rss,
            backend: backend.clone(),
        };
        if let Ok(t) = serde_json::to_string_pretty(&st) {
            let _ = std::fs::write(&path, t);
        }
    }
}

/// Run the search until its deadline or a stop request. The database resumes where it stopped.
pub fn run(cfg: Cfg, backend: Option<Box<dyn Backend>>) -> Result<(), String> {
    if let Some(p) = cfg.db.parent() {
        std::fs::create_dir_all(p).map_err(|e| e.to_string())?;
    }
    let _ = std::fs::remove_file(stop_file(&cfg.db));
    // A panic in any worker ends the process; the supervisor restarts it from the database.
    let hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        hook(info);
        std::process::exit(101);
    }));
    let conn = open_db(&cfg.db)?;
    eprintln!("building the tiling list");
    let tl = Instant::now();
    let tilings = space::all_tilings();
    eprintln!("tilings took {:.1}s", tl.elapsed().as_secs_f64());
    let tiling_total: f64 = tilings.iter().map(|t| t.weight).sum();
    eprintln!(
        "{} tilings, {} with distinct structure",
        tilings.len(),
        tilings.iter().filter(|t| t.twin_of.is_none()).count()
    );
    let (tx, rx) = mpsc::sync_channel(64);
    let mut named = Vec::new();
    for (i, t) in tilings.iter().enumerate() {
        for e in crate::rules::catalog() {
            if t.twin_of.is_none() {
                named.push((i, e.notation.to_string()));
            }
        }
    }
    let t0 = Instant::now();
    let shared = Arc::new(Shared {
        deadline: match cfg.until {
            Some(u) => {
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs())
                    .unwrap_or(0);
                t0 + Duration::from_secs(u.saturating_sub(now))
            }
            None => t0 + Duration::from_secs_f64(cfg.hours * 3600.0),
        },
        t_start: t0,
        cfg: cfg.clone(),
        tilings,
        tiling_total,
        grids: Mutex::new(HashMap::new()),
        archive: Mutex::new(Archive::default()),
        seen: Mutex::new(HashSet::new()),
        coverage: Mutex::new(HashMap::new()),
        named: Mutex::new(named),
        next_id: AtomicI64::new(1),
        screened: AtomicU64::new(0),
        confirmed: AtomicU64::new(0),
        stop: AtomicBool::new(false),
        tx,
    });
    resume(&shared, &conn)?;
    drop(conn);
    let db_path = cfg.db.clone();
    let wr = std::thread::spawn(move || writer(db_path, rx));
    let started = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let be_name = backend
        .as_ref()
        .map_or("cpu only".to_string(), |b| b.name());
    let mut handles = Vec::new();
    {
        let s = shared.clone();
        let name = be_name.clone();
        handles.push(std::thread::spawn(move || {
            status_loop(s, name, started, t0)
        }));
    }
    let mut cpu_fams = CPU_FAMS.to_vec();
    match backend {
        Some(be) => {
            let s = shared.clone();
            let seed = cfg.seed ^ 0xABCD;
            handles.push(std::thread::spawn(move || gpu_loop(s, be, seed)));
        }
        None => cpu_fams.extend_from_slice(GPU_FAMS),
    }
    {
        let s = shared.clone();
        let seed = cfg.seed ^ 0x1234;
        handles.push(std::thread::spawn(move || cpu_loop(s, seed, cpu_fams)));
    }
    for h in handles {
        let _ = h.join();
    }
    drop(shared);
    let _ = wr.join();
    Ok(())
}
