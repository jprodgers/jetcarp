//! Collisions: two library patterns placed at every relative offset (and phase) in a range, each
//! run until it settles, with the outcome classified and any new objects cataloged.
//!
//! Placement: pattern A (phase 0, `orientation_a`) has its bounding box's corner at lattice
//! (0, 0); pattern B (each phase asked for, `orientation_b`) has its corner at (dx, dy).
//! Placements where B starts within two neighbour steps of A are skipped (`overlap`): that is a
//! different starting pattern, not a collision. Each run uses a dead-edged frame with `margin`
//! empty cells around the pair.
//!
//! Settling: every `check_every` generations the objects are found (`objects::find_objects`).
//! The run has settled when every object is a still life, oscillator, or spaceship and no
//! spaceship's path (from where it is, for the generations left) comes within reach of another
//! object. A run that reaches the frame edge is classified as it stands then; a run whose
//! population passes `explode_cells` is `explodes`; one that never settles is `unsettled`.
//!
//! Outcomes: `both_survive` (A and B come out as they went in, possibly moved or turned),
//! `both_die` (nothing left), `a_survives` / `b_survives` (only that one is left), `new_objects`
//! (anything else; the products are listed and cataloged).
//!
//! Every placement's run is a pure function of its inputs and results are gathered in placement
//! order ([`Collected`]), so they are identical at any thread count and in any batches:
//! [`collide`] runs them all at once; a collide job (`crate::jobs::collide`) runs them a batch per
//! step, its checkpoint the gathered state.

use super::verify::{Frame, lattice_info, rule_world};
use super::{
    DiscoveredBy, PatternRecord, Provenance, default_periods, now_utc, paste::oriented, verify,
};
use crate::objects::{LCell, ObjClass, ObjectOptions, Phase, find_objects};
use crate::render::{Image, RenderOptions, render_with_colors};
use crate::world::Command;
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CollideSpec {
    /// B's corner offsets, inclusive ranges.
    pub dx: [i32; 2],
    pub dy: [i32; 2],
    /// B's phases to try (empty = all).
    #[serde(default)]
    pub phases: Vec<u32>,
    #[serde(default)]
    pub orientation_a: u32,
    #[serde(default)]
    pub orientation_b: u32,
    /// Generation limit per run.
    pub gens: u64,
    /// Empty cells around the pair.
    pub margin: i32,
    pub check_every: u64,
    /// Population that counts as an explosion.
    pub explode_cells: u64,
    /// The rule to run (both patterns must work in it). Default: A's rule if B works in it,
    /// else B's rule if A works in it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rule: Option<String>,
}

impl Default for CollideSpec {
    fn default() -> Self {
        CollideSpec {
            dx: [-6, 6],
            dy: [-6, 6],
            phases: vec![],
            orientation_a: 0,
            orientation_b: 0,
            gens: 1024,
            margin: 40,
            check_every: 16,
            explode_cells: 2000,
            rule: None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    BothSurvive,
    BothDie,
    ASurvives,
    BSurvives,
    NewObjects,
    Unsettled,
    Explodes,
    Overlap,
}

impl Outcome {
    pub fn name(self) -> &'static str {
        match self {
            Outcome::BothSurvive => "both_survive",
            Outcome::BothDie => "both_die",
            Outcome::ASurvives => "a_survives",
            Outcome::BSurvives => "b_survives",
            Outcome::NewObjects => "new_objects",
            Outcome::Unsettled => "unsettled",
            Outcome::Explodes => "explodes",
            Outcome::Overlap => "overlap",
        }
    }
    pub fn color(self) -> [u8; 4] {
        match self {
            Outcome::BothSurvive => [0x8c, 0x8c, 0x8c, 0xff],
            Outcome::BothDie => [0x4d, 0x9d, 0xff, 0xff],
            Outcome::ASurvives | Outcome::BSurvives => [0x3d, 0xdc, 0x6d, 0xff],
            Outcome::NewObjects => [0xff, 0x4d, 0xc4, 0xff],
            Outcome::Unsettled => [0xff, 0xb0, 0x2e, 0xff],
            Outcome::Explodes => [0xff, 0x4d, 0x4d, 0xff],
            Outcome::Overlap => [0x55, 0x55, 0x55, 0xff],
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CollideRun {
    pub dx: i32,
    pub dy: i32,
    pub phase: u32,
    pub outcome: Outcome,
    /// Generations run (0 for overlaps).
    pub gens: u64,
    /// Objects at the end (labels), e.g. `block x 2, glider (c/4)`.
    pub result: String,
    /// Library ids of the products that are neither A nor B.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub products: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CollideReport {
    /// Records written by CART, JETCARP's predecessor, call this `cart_version`.
    #[serde(alias = "cart_version")]
    pub jetcarp_version: String,
    pub a: String,
    pub b: String,
    pub a_label: String,
    pub b_label: String,
    pub rule: String,
    pub tiling: String,
    pub spec: CollideSpec,
    /// Runs per outcome.
    pub counts: BTreeMap<String, usize>,
    pub runs: Vec<CollideRun>,
}

pub struct CollideOutput {
    pub report: CollideReport,
    /// Verified records of the products (new objects), `seen` = runs producing them.
    pub records: Vec<PatternRecord>,
    /// Final state of each run that was not skipped, objects coloured by class (same order as
    /// the non-overlap runs).
    pub thumbs: Vec<(String, Outcome, Image)>,
}

fn shifted(v: &[LCell], dx: i32, dy: i32) -> Phase {
    v.iter()
        .map(|c| LCell {
            i: c.i + dx,
            j: c.j + dy,
            ..*c
        })
        .collect()
}

/// Collide `b` into `a` at every offset and phase in `spec`. `thumb_px` > 0 renders thumbnails.
pub fn collide(
    a: &PatternRecord,
    b: &PatternRecord,
    spec: &CollideSpec,
    thumb_px: u32,
    progress: Option<&(dyn Fn(usize, usize) + Sync)>,
) -> Result<CollideOutput, String> {
    let c = Collision::new(a, b, spec, now_utc())?;
    let mut got = Collected::new(&c);
    let outs = c
        .run(0..c.placements(), thumb_px, progress, None)
        .unwrap_or_default();
    got.add(outs, true)?;
    got.verify(got.products());
    Ok(got.output(&c))
}

/// One placement's run, its products' records, and its thumbnail.
pub type RunOut = (CollideRun, Vec<PatternRecord>, Option<Image>);

/// A collision ready to run: both patterns in the rule they run in, A's cells, B's phases, and
/// the placements (phases outer, then dy, then dx), numbered. Placement `k` is a pure function
/// of the patterns, the spec, `k`, and the collision's date (in the products' provenance).
pub struct Collision {
    a: PatternRecord,
    b: PatternRecord,
    spec: CollideSpec,
    date: String,
    info: super::verify::LatticeInfo,
    cells_a: Phase,
    /// Cells within two steps of A (unwrapped lattice coordinates).
    near: HashSet<(i32, i32, u32)>,
    phases: Vec<u32>,
    b_phases: Vec<Phase>,
    jobs: Vec<(i32, i32, u32)>,
}

impl Collision {
    pub fn new(
        a: &PatternRecord,
        b: &PatternRecord,
        spec: &CollideSpec,
        date: String,
    ) -> Result<Collision, String> {
        if a.site.is_some() || b.site.is_some() {
            return Err("collisions need lattice tilings (not aperiodic patches)".into());
        }
        if a.neighborhood != b.neighborhood || a.steps_per_gen != b.steps_per_gen {
            return Err("the patterns use different neighbourhoods or steps per generation".into());
        }
        // Patterns are rule-free: run both in one rule they both work in.
        let rule = match &spec.rule {
            Some(r) => r.clone(),
            None if b.works_in(&a.rule) => a.rule.clone(),
            None if a.works_in(&b.rule) => b.rule.clone(),
            None => {
                return Err(format!(
                    "no common rule: {} works in {}, {} in {} (give a rule both work in)",
                    a.label(),
                    a.range
                        .as_ref()
                        .map(|r| r.describe())
                        .unwrap_or_else(|| a.rule.clone()),
                    b.label(),
                    b.range
                        .as_ref()
                        .map(|r| r.describe())
                        .unwrap_or_else(|| b.rule.clone())
                ));
            }
        };
        let (a, b) = (a.in_rule(&rule)?, b.in_rule(&rule)?);
        if super::tiling_key(&a.tiling) != super::tiling_key(&b.tiling) {
            return Err(format!(
                "the patterns are on different tilings ({} and {})",
                a.tiling_name, b.tiling_name
            ));
        }
        if spec.dx[0] > spec.dx[1] || spec.dy[0] > spec.dy[1] {
            return Err("offset ranges must be low..high".into());
        }
        let n_off = (spec.dx[1] - spec.dx[0] + 1) as u64 * (spec.dy[1] - spec.dy[0] + 1) as u64;
        let phases: Vec<u32> = if spec.phases.is_empty() {
            (0..b.period as u32).collect()
        } else {
            spec.phases.clone()
        };
        if n_off * phases.len() as u64 > 100_000 {
            return Err("more than 100,000 placements; narrow the offset ranges".into());
        }
        let info = lattice_info(&a.tiling, a.neighborhood)?;
        // A small frame to get the tiling's offsets and symmetries.
        let probe = Frame::around(&a.tiling, &info, &a.phase(0)?, 2)?;
        let topo = probe.topo.clone();
        let g = topo.lattice().unwrap().clone();
        let cells_a = oriented(&topo, a.neighborhood, &a.phase(0)?, spec.orientation_a)?;
        let mut near: HashSet<(i32, i32, u32)> = cells_a.iter().map(|c| (c.i, c.j, c.k)).collect();
        for _ in 0..2 {
            let cur: Vec<(i32, i32, u32)> = near.iter().copied().collect();
            for (i, j, k) in cur {
                for o in &g.offsets[a.neighborhood as usize][k as usize] {
                    near.insert((i + o.di, j + o.dj, o.k));
                }
            }
        }
        let mut jobs = Vec::new();
        for &ph in &phases {
            for dy in spec.dy[0]..=spec.dy[1] {
                for dx in spec.dx[0]..=spec.dx[1] {
                    jobs.push((dx, dy, ph));
                }
            }
        }
        let b_phases: Vec<Phase> = phases
            .iter()
            .map(|&ph| {
                oriented(
                    &topo,
                    b.neighborhood,
                    &b.phase(ph as u64)?,
                    spec.orientation_b,
                )
            })
            .collect::<Result<_, _>>()?;
        Ok(Collision {
            a,
            b,
            spec: spec.clone(),
            date,
            info,
            cells_a,
            near,
            phases,
            b_phases,
            jobs,
        })
    }

    /// Placements in all.
    pub fn placements(&self) -> usize {
        self.jobs.len()
    }

    /// The date in the products' provenance.
    pub fn date(&self) -> &str {
        &self.date
    }

    /// Run placements `range` in parallel on the current rayon pool, in order (`thumb_px` > 0
    /// renders thumbnails). `progress(done, total)` is called as they finish. `None` when
    /// `cancel` was set part way (the rest skipped).
    pub fn run(
        &self,
        range: std::ops::Range<usize>,
        thumb_px: u32,
        progress: Option<&(dyn Fn(usize, usize) + Sync)>,
        cancel: Option<&std::sync::atomic::AtomicBool>,
    ) -> Option<Vec<Result<RunOut, String>>> {
        use std::sync::atomic::Ordering;
        let total = self.jobs.len();
        let done = std::sync::atomic::AtomicUsize::new(range.start);
        let outs: Vec<Option<Result<RunOut, String>>> = range
            .collect::<Vec<usize>>()
            .par_iter()
            .map(|&k| {
                if cancel.is_some_and(|c| c.load(Ordering::Relaxed)) {
                    return None;
                }
                let r = self.run_one(k, thumb_px);
                let d = done.fetch_add(1, Ordering::Relaxed) + 1;
                if let Some(p) = progress {
                    p(d, total);
                }
                Some(r)
            })
            .collect();
        outs.into_iter().collect()
    }

    /// Run placement `k` (on this thread; `thumb_px` > 0 renders its thumbnail).
    pub fn run_one(&self, k: usize, thumb_px: u32) -> Result<RunOut, String> {
        let (a, b, spec) = (&self.a, &self.b, &self.spec);
        let (dx, dy, ph) = self.jobs[k];
        let bi = self.phases.iter().position(|&p| p == ph).unwrap();
        let cells_b = shifted(&self.b_phases[bi], dx, dy);
        if cells_b.iter().any(|c| self.near.contains(&(c.i, c.j, c.k))) {
            return Ok((
                CollideRun {
                    dx,
                    dy,
                    phase: ph,
                    outcome: Outcome::Overlap,
                    gens: 0,
                    result: String::new(),
                    products: vec![],
                },
                vec![],
                None,
            ));
        }
        run_one(
            a,
            b,
            spec,
            &self.info,
            &self.cells_a,
            &cells_b,
            (dx, dy, ph),
            (&a.id, &b.id),
            &self.date,
            thumb_px,
        )
    }
}

/// A collision's results so far, gathered placement by placement in order, whatever the
/// batches (a collide job's checkpoint): the runs per outcome and the products (each id once,
/// from its first run, `seen` = the runs producing it). Once every placement is in, the products
/// are verified (in any batches) and [`output`](Self::output) gives exactly what one whole run
/// gives. The runs themselves (and thumbnails) are kept only when asked for; kept runs are in
/// the checkpoint too (thumbnails are not).
#[derive(Clone, Default, Serialize, Deserialize)]
pub struct Collected {
    /// When the collision started (its products' provenance date).
    pub date: String,
    /// Placements gathered.
    pub next: usize,
    /// Runs per outcome.
    pub counts: BTreeMap<String, usize>,
    products: Vec<PatternRecord>,
    /// Products verified (once every placement is in), and those that failed.
    verified: usize,
    failed: Vec<usize>,
    #[serde(skip)]
    at: HashMap<String, usize>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    runs: Vec<CollideRun>,
    #[serde(skip)]
    thumbs: Vec<(String, Outcome, Image)>,
}

impl Collected {
    pub fn new(c: &Collision) -> Collected {
        Collected {
            date: c.date.clone(),
            ..Default::default()
        }
    }

    /// One read back from a checkpoint (its index rebuilt); it must be of this collision.
    pub fn restored(mut self, c: &Collision) -> Result<Collected, String> {
        if self.next > c.placements() || self.date != c.date {
            return Err("the checkpoint does not match the collision".into());
        }
        self.at = self
            .products
            .iter()
            .enumerate()
            .map(|(k, r)| (r.id.clone(), k))
            .collect();
        Ok(self)
    }

    /// Distinct products so far.
    pub fn products(&self) -> usize {
        self.products.len()
    }
    /// Products verified so far.
    pub fn verified(&self) -> usize {
        self.verified
    }

    /// Gather the next placements' runs (of placements `next..`, in order), keeping the runs
    /// and thumbnails when `keep`. A run that failed fails the collision.
    pub fn add(&mut self, outs: Vec<Result<RunOut, String>>, keep: bool) -> Result<(), String> {
        for o in outs {
            let (run, recs, img) = o?;
            self.next += 1;
            *self.counts.entry(run.outcome.name().into()).or_default() += 1;
            for r in recs {
                match self.at.get(&r.id) {
                    Some(&k) => self.products[k].seen += 1,
                    None => {
                        self.at.insert(r.id.clone(), self.products.len());
                        self.products.push(r);
                    }
                }
            }
            if keep {
                if let Some(img) = img {
                    self.thumbs.push((
                        format!("{},{} p{}", run.dx, run.dy, run.phase),
                        run.outcome,
                        img,
                    ));
                }
                self.runs.push(run);
            }
        }
        Ok(())
    }

    /// Verify the next `n` products (in parallel on the current rayon pool); every placement
    /// must be in.
    pub fn verify(&mut self, n: usize) {
        let (a, b) = (self.verified, (self.verified + n).min(self.products.len()));
        let checked: Vec<Result<(), String>> = self.products[a..b]
            .par_iter_mut()
            .map(|r| {
                r.rescore();
                r.verified = verify(r, default_periods(r.period))?;
                Ok(())
            })
            .collect();
        for (u, r) in (a..b).zip(checked) {
            if r.is_err() {
                self.failed.push(u);
            }
        }
        self.verified = b;
    }

    /// The verified products' records (`seen` = runs producing them), once every placement is
    /// in and every product verified.
    pub fn records(&self) -> Vec<PatternRecord> {
        self.products
            .iter()
            .enumerate()
            .filter(|(u, _)| !self.failed.contains(u))
            .map(|(_, r)| r.clone())
            .collect()
    }

    /// The collision's report (with the runs kept), verified records, and thumbnails.
    pub fn output(self, c: &Collision) -> CollideOutput {
        let records = self.records();
        CollideOutput {
            report: CollideReport {
                jetcarp_version: crate::VERSION.into(),
                a: c.a.id.clone(),
                b: c.b.id.clone(),
                a_label: c.a.label(),
                b_label: c.b.label(),
                rule: c.a.rule.clone(),
                tiling: c.a.tiling_name.clone(),
                spec: c.spec.clone(),
                counts: self.counts,
                runs: self.runs,
            },
            records,
            thumbs: self.thumbs,
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn run_one(
    a: &PatternRecord,
    b: &PatternRecord,
    spec: &CollideSpec,
    info: &super::verify::LatticeInfo,
    cells_a: &[LCell],
    cells_b: &[LCell],
    (dx, dy, ph): (i32, i32, u32),
    (key_a, key_b): (&str, &str),
    date: &str,
    thumb_px: u32,
) -> Result<(CollideRun, Vec<PatternRecord>, Option<Image>), String> {
    let mut both: Phase = cells_a.iter().chain(cells_b).copied().collect();
    both.sort_unstable();
    let frame = Frame::around(&a.tiling, info, &both, spec.margin.max(info.reach + 2))?;
    let mut w = rule_world(a, frame.topo.clone())?;
    let id = w.cas[0].id;
    let fi = w.cas[0].field;
    w.apply(Command::SetCells {
        id,
        cells: frame.cells(&both)?,
    })?;
    let opts = ObjectOptions::default();
    let every = spec.check_every.max(1);
    let mut outcome = Outcome::Unsettled;
    let mut last_rep = None;
    let reach = info.reach;
    while w.generation < spec.gens {
        let n = every.min(spec.gens - w.generation);
        w.apply(Command::Step { gens: n })?;
        let (live, edge) = frame.live(&w.fields[fi].state, a.neighborhood);
        if live.is_empty() {
            outcome = Outcome::BothDie;
            last_rep = None;
            break;
        }
        if live.len() as u64 > spec.explode_cells {
            outcome = Outcome::Explodes;
            break;
        }
        let rep = find_objects(&w, &opts)?;
        let periodic = rep
            .objects
            .iter()
            .all(|o| !matches!(o.class, ObjClass::Other { .. }));
        let settled = periodic && {
            let left = spec.gens.saturating_sub(w.generation) as i32;
            let boxes: Vec<[i32; 4]> = rep
                .objects
                .iter()
                .map(|o| {
                    let b = o.lattice_bbox.unwrap_or([0, 0, 0, 0]);
                    [b[0] - reach, b[1] - reach, b[2] + reach, b[3] + reach]
                })
                .collect();
            rep.objects.iter().enumerate().all(|(k, o)| {
                let ObjClass::Spaceship { period, dx, dy } = o.class else {
                    return true;
                };
                let n = left / period.max(1) as i32 + 1;
                let b = boxes[k];
                let sweep = [
                    b[0].min(b[0] + dx * n),
                    b[1].min(b[1] + dy * n),
                    b[2].max(b[2] + dx * n),
                    b[3].max(b[3] + dy * n),
                ];
                boxes.iter().enumerate().all(|(m, q)| {
                    m == k
                        || sweep[2] < q[0]
                        || q[2] < sweep[0]
                        || sweep[3] < q[1]
                        || q[3] < sweep[1]
                })
            })
        };
        let stop = settled || edge;
        last_rep = Some(rep);
        if stop {
            outcome = if periodic {
                Outcome::NewObjects
            } else {
                Outcome::Unsettled
            };
            break;
        }
    }
    let mut products = Vec::new();
    let mut result = String::new();
    if let Some(rep) = &last_rep {
        result = rep.summary_line();
        if outcome == Outcome::NewObjects {
            // Compare by library id (canonical over phases and the tiling's symmetries).
            let prov = Provenance {
                source: format!("collide {} + {} at {dx},{dy} phase {ph}", a.id, b.id),
                tiling: Some(frame.topo.spec.clone()),
                generation: Some(w.generation),
                jetcarp_version: crate::VERSION.into(),
                date: date.to_string(),
                ..Default::default()
            };
            let recs: Vec<Option<PatternRecord>> = rep
                .objects
                .iter()
                .flat_map(|o| {
                    match super::split::records_from_object(
                        &w,
                        id,
                        o,
                        prov.clone(),
                        DiscoveredBy::Search,
                    ) {
                        Ok(v) => v.into_iter().map(Some).collect(),
                        Err(_) => vec![None],
                    }
                })
                .collect();
            let mut keys: Vec<&str> = recs
                .iter()
                .map(|r| r.as_ref().map(|r| r.id.as_str()).unwrap_or("?"))
                .collect();
            keys.sort_unstable();
            let mut ab = vec![key_a, key_b];
            ab.sort_unstable();
            outcome = if keys == ab {
                Outcome::BothSurvive
            } else if keys == [key_a] {
                Outcome::ASurvives
            } else if keys == [key_b] {
                Outcome::BSurvives
            } else {
                Outcome::NewObjects
            };
            if outcome == Outcome::NewObjects {
                let mut seen = HashSet::new();
                for r in recs.into_iter().flatten() {
                    if r.id != key_a && r.id != key_b && seen.insert(r.id.clone()) {
                        products.push(r);
                    }
                }
            }
        }
    }
    let ids = products.iter().map(|r| r.id.clone()).collect();
    let img = (thumb_px > 0).then(|| {
        let colors = match &last_rep {
            Some(r) => r.colors(w.topo.len()),
            None => vec![crate::render::BACKGROUND; w.topo.len()],
        };
        render_with_colors(
            &w,
            &colors,
            &RenderOptions {
                max_side: thumb_px,
                outline_min_px: 0.0,
                ..Default::default()
            },
        )
    });
    Ok((
        CollideRun {
            dx,
            dy,
            phase: ph,
            outcome,
            gens: w.generation,
            result,
            products: ids,
        },
        products,
        img,
    ))
}
