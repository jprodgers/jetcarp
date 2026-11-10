//! The adaptive stage 1 search as a resumable catalog job (`jobs.kind = 'stage1'`).
//!
//! A search covers one or more **spaces**: a tiling and neighbourhood with a rule family (B/S, or
//! Generations with a state count) and its digits. Its passes, in order:
//!
//! 1. **coarse**: every rule of a space where it has at most `all_max` rules (squares with
//!    touching neighbours and no B0: 131,072; edge neighbours: 512; hexagons: 8,192), otherwise
//!    a seeded sample of `sample` rules (triangles with touching neighbours: 2^25), each at the
//!    base setting;
//! 2. **refine**: the `top` settings of each space (by score) again with each other density,
//!    soup size, grid size, and generation count of the refinement lists, one axis at a time;
//! 3. **neighbours** (`rounds` times): the `top` rules of each space not yet expanded, and every
//!    rule one birth or survival count away from them (B0 excluded), each at its parent's best
//!    setting.
//!
//! Tasks run in batches of consecutive indices on a worker pool; a batch's results are merged in
//! task order: novelty against the reference set (the built-in Quick Start rules of the space,
//! evaluated at the base setting, every `auto` or `approved` setting of other jobs on the tiling,
//! and every earlier setting of this job that scored at least the candidate threshold), the score
//! with the job's model (kept in the checkpoint, so a refit elsewhere does not change a running
//! search), the status, and one flush of the settings, Quick Start rows, progress, and
//! checkpoint. The top settings and rules for later passes are ranked by that model's score from
//! the stored descriptors, ties by the setting's label. So results do not depend on threads or
//! batch sizes, and a resumed search equals an uninterrupted one (`tests/stage1.rs`).

use super::eval::{Descriptors, GridKit, Setting, evaluate};
use super::score::{ScoreModel, features, novelty_vector};
use crate::census::{all_bs_rules, max_degree, sample_bs_rules};
use crate::library::{
    JobStatus, Library, NewSetting, QsStatus, QuickStartRow, SettingStatus, SettingsQuery, now_utc,
    tiling_display_name, tiling_key,
};
use crate::rules::Rule;
use crate::topology::{Boundary, Nb, TilingSpec};
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

/// A rule space on one grid.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Space {
    pub tiling: TilingSpec,
    #[serde(default)]
    pub nb: Nb,
    /// 2 for B/S rules, more for Generations.
    #[serde(default = "two")]
    pub states: u8,
    /// Birth digits (default 1 to the largest degree; B0 is never searched).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub birth: Option<Vec<u32>>,
    /// Survival digits (default 0 to the largest degree).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub survive: Option<Vec<u32>>,
    /// Explicit rules instead of the digit space.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub rules: Vec<String>,
    /// Every rule when the space has at most this many (default 131,072).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub all_max: Option<u64>,
    /// Otherwise this many sampled rules (default 20,000; seeded by the search's seed).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sample: Option<usize>,
}

fn two() -> u8 {
    2
}

impl Space {
    pub fn new(tiling: TilingSpec, nb: Nb) -> Space {
        Space {
            tiling,
            nb,
            states: 2,
            birth: None,
            survive: None,
            rules: vec![],
            all_max: None,
            sample: None,
        }
    }
    /// `squares (touch)`, `hexagons (touch) C3`.
    pub fn label(&self) -> String {
        format!(
            "{} ({}){}",
            tiling_display_name(&self.tiling),
            match self.nb {
                Nb::Touch => "touch",
                Nb::Edge => "edge",
            },
            if self.states > 2 {
                format!(" C{}", self.states)
            } else {
                String::new()
            }
        )
    }
}

/// The base setting of every coarse task (rule and tiling come from the space).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Base {
    pub density: f64,
    pub soup: f64,
    pub grid: [u32; 2],
    pub boundary: Boundary,
    pub gens: u64,
    pub soups: u32,
    pub seed: u64,
}

impl Default for Base {
    fn default() -> Self {
        let s = Setting::default();
        Base {
            density: s.density,
            soup: s.soup,
            grid: s.grid,
            boundary: s.boundary,
            gens: s.gens,
            soups: s.soups,
            seed: s.seed,
        }
    }
}

/// The refinement pass.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Refine {
    /// Settings refined per space (0 skips the pass).
    pub top: usize,
    pub densities: Vec<f64>,
    pub soups: Vec<f64>,
    pub grids: Vec<[u32; 2]>,
    pub gens: Vec<u64>,
}

impl Default for Refine {
    fn default() -> Self {
        Refine {
            top: 24,
            densities: vec![0.15, 0.25, 0.5],
            soups: vec![0.4],
            grids: vec![[128, 128]],
            gens: vec![3000],
        }
    }
}

/// The rule-space neighbour pass.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Neighbours {
    /// Rules expanded per space and round (0 skips the pass).
    pub top: usize,
    pub rounds: u32,
}

impl Default for Neighbours {
    fn default() -> Self {
        Neighbours { top: 24, rounds: 1 }
    }
}

/// Everything that defines a search's results (threads and batching do not).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Stage1Spec {
    pub spaces: Vec<Space>,
    #[serde(default)]
    pub base: Base,
    #[serde(default)]
    pub refine: Refine,
    #[serde(default)]
    pub neighbours: Neighbours,
}

impl Default for Stage1Spec {
    fn default() -> Self {
        Stage1Spec {
            spaces: vec![Space::new(TilingSpec::new("hex"), Nb::Touch)],
            base: Base::default(),
            refine: Refine::default(),
            neighbours: Neighbours::default(),
        }
    }
}

impl Stage1Spec {
    pub fn check(&self) -> Result<(), String> {
        if self.spaces.is_empty() {
            return Err("a stage 1 search needs at least one space".into());
        }
        for sp in &self.spaces {
            if sp.states < 2 {
                return Err("states must be at least 2".into());
            }
        }
        self.base_setting(0, "B3/S23").check()
    }

    /// The base setting of a space for a rule.
    pub fn base_setting(&self, space: usize, rule: &str) -> Setting {
        let sp = &self.spaces[space.min(self.spaces.len().saturating_sub(1))];
        let b = &self.base;
        Setting {
            rule: rule.to_string(),
            tiling: sp.tiling.clone(),
            nb: Some(sp.nb),
            density: b.density,
            soup: b.soup,
            grid: b.grid,
            boundary: b.boundary,
            gens: b.gens,
            soups: b.soups,
            seed: b.seed,
        }
    }

    /// Pass names in order.
    pub fn passes(&self) -> Vec<String> {
        let mut v = vec!["coarse".to_string()];
        if self.refine.top > 0 {
            v.push("refine".into());
        }
        if self.neighbours.top > 0 {
            for r in 1..=self.neighbours.rounds {
                v.push(format!("neighbours-{r}"));
            }
        }
        v
    }

    /// The coarse rules of a space (every rule, or the seeded sample).
    pub fn space_rules(&self, space: usize) -> Result<Vec<String>, String> {
        let sp = &self.spaces[space];
        if !sp.rules.is_empty() {
            return Ok(sp.rules.clone());
        }
        let (birth, survive) = digits(sp)?;
        let states = (sp.states > 2).then_some(sp.states);
        let bits = (birth.len() + survive.len()) as u32;
        let all_max = sp.all_max.unwrap_or(1 << 17);
        if bits < 63 && (1u64 << bits) <= all_max {
            all_bs_rules(&birth, &survive, states)
        } else {
            Ok(sample_bs_rules(
                &birth,
                &survive,
                states,
                sp.sample.unwrap_or(20_000),
                self.base.seed ^ (space as u64).wrapping_mul(0x9e37_79b9_7f4a_7c15),
            ))
        }
    }
}

/// A space's digits (birth without 0, survival from 0), up to the tiling's largest degree.
pub fn digits(sp: &Space) -> Result<(Vec<u32>, Vec<u32>), String> {
    let mut t = sp.tiling.clone();
    t.size = Some(crate::topology::SizeSpec::Rect { w: 8, h: 8 });
    let topo = crate::topology::Topology::build(&t)?;
    let deg = max_degree(&topo, sp.nb) as u32;
    let birth: Vec<u32> = sp
        .birth
        .clone()
        .unwrap_or_else(|| (1..=deg).collect())
        .into_iter()
        .filter(|&d| d >= 1 && d <= deg)
        .collect();
    let survive: Vec<u32> = sp
        .survive
        .clone()
        .unwrap_or_else(|| (0..=deg).collect())
        .into_iter()
        .filter(|&d| d <= deg)
        .collect();
    Ok((birth, survive))
}

pub use crate::rules::rule_neighbours;

// ---------------------------------------------------------------------------------------------
// Quick Start entries

/// A setting's title: `Spaceships: B2/S34 on hexagons`.
pub fn title(s: &Setting, d: &Descriptors) -> String {
    let mut b = d.behaviour.clone();
    if let Some(c) = b.get(0..1) {
        b = c.to_uppercase() + &b[1..];
    }
    let t = tiling_display_name(&s.tiling);
    let t = t.split(" (").next().unwrap_or(&t).to_lowercase();
    format!(
        "{b}: {} on {t}",
        s.notation().unwrap_or_else(|_| s.rule.clone())
    )
}

/// The Quick Start entry for a stored setting: a scene that reproduces soup 0 exactly, the grid,
/// rule family, behaviour, a title, and a description (status `auto`; the caller sets it).
pub fn quickstart_entry(
    id: i64,
    s: &Setting,
    d: &Descriptors,
    score: f64,
) -> Result<QuickStartRow, String> {
    let mut scene = s.scene(0)?;
    let t = title(s, d);
    scene.name = t.clone();
    let desc = format!(
        "Stage 1 setting {id}: {}x{} {} grid, density {}, {}, {} generations; score {:.1}; {}{}",
        s.grid[0],
        s.grid[1],
        super::eval::boundary_name(s.boundary),
        s.density,
        if s.soup >= 1.0 {
            "soup fills the grid".to_string()
        } else {
            format!("soup disc of radius {}", s.soup)
        },
        s.gens,
        score,
        d.tags.join(", "),
        if d.distinct > 0 {
            format!(
                "; {} distinct objects ({} ship kinds, {} oscillator kinds, longest period {})",
                d.distinct, d.ship_kinds, d.oscillator_kinds, d.longest_period
            )
        } else {
            String::new()
        }
    );
    scene.description = desc.clone();
    let mut q = QuickStartRow::for_scene(&format!("setting-{id}"), "setting", scene);
    q.source_id = Some(id.to_string());
    q.title = t;
    q.description = desc;
    q.behaviour = d.behaviour.clone();
    q.score = Some(score);
    q.status = QsStatus::Auto;
    Ok(q)
}

// ---------------------------------------------------------------------------------------------
// Progress and checkpoint

/// A setting in progress lists.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct TopSetting {
    pub id: i64,
    pub rule: String,
    pub tiling: String,
    pub score: f64,
    pub behaviour: String,
    pub status: String,
    pub label: String,
}

/// Counts for one space.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct SpaceTotals {
    pub space: String,
    /// Coarse rules (every rule, or the sample).
    pub rules: u64,
    pub evaluated: u64,
    pub auto: u64,
    pub candidate: u64,
    pub failed: u64,
    pub best_score: f64,
    pub best_rule: String,
    /// Settings by behaviour.
    pub behaviour: BTreeMap<String, u64>,
}

/// Progress of a search, written to the job's `progress` at every flush and kept live in
/// memory for a dashboard ([`Stage1Handle::progress`]).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Stage1Progress {
    pub job: i64,
    /// `running`, `paused`, `done`, `failed`.
    pub status: String,
    pub passes: Vec<String>,
    /// Index into `passes`.
    pub pass: usize,
    pub pass_done: u64,
    pub pass_total: u64,
    /// Settings evaluated, over every pass and session.
    pub evaluated: u64,
    /// Settings per second in this session.
    pub per_s: f64,
    /// Seconds run over every session.
    pub elapsed_s: f64,
    /// Settings by status when they were stored (`auto`, `candidate`, `stored`, ...).
    pub statuses: BTreeMap<String, u64>,
    /// Score histogram, ten bins of 10 points.
    pub histogram: [u64; 10],
    /// The best settings so far (score, then label).
    pub best: Vec<TopSetting>,
    /// The newest `auto` settings, newest first.
    pub newest_auto: Vec<TopSetting>,
    pub spaces: Vec<SpaceTotals>,
    pub model_id: i64,
    pub threads: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    pub updated: String,
}

impl Stage1Progress {
    /// `stage1 4: coarse 8,192/8,192, 1,520/s, 41 auto, 230 candidates | best 81.2 B2/S34 ...`
    pub fn line(&self) -> String {
        let pass = self.passes.get(self.pass).cloned().unwrap_or_default();
        format!(
            "stage1 {}: {} {}/{}, {} settings, {:.0}/s, {} auto, {} candidates{}",
            self.job,
            pass,
            digits3(self.pass_done),
            digits3(self.pass_total),
            digits3(self.evaluated),
            self.per_s,
            self.statuses.get("auto").copied().unwrap_or(0),
            self.statuses.get("candidate").copied().unwrap_or(0),
            self.best
                .first()
                .map(|b| format!(" | best {:.1} {} ({})", b.score, b.rule, b.behaviour))
                .unwrap_or_default()
        )
    }
}

fn digits3(n: u64) -> String {
    let s = n.to_string();
    let mut out = String::new();
    for (i, ch) in s.chars().enumerate() {
        if i > 0 && (s.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(ch);
    }
    out
}

/// A task: a setting of a space, made by a pass.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Task {
    pub space: usize,
    pub setting: Setting,
}

/// What survives a crash: written with every flush.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Checkpoint {
    pub pass: usize,
    pub next: u64,
    /// Tasks of the current pass (empty for the coarse pass, whose tasks follow from the spec).
    pub tasks: Vec<Task>,
    /// Rules expanded by the neighbour passes, per space.
    pub expanded: Vec<BTreeSet<String>>,
    /// The job's score model.
    pub model: Option<ScoreModel>,
    pub progress: Stage1Progress,
}

/// Called with the progress after every batch.
pub type ProgressHook = Arc<dyn Fn(&Stage1Progress) + Send + Sync>;

/// How to run a search (none of this changes its results).
#[derive(Clone, Default)]
pub struct Stage1Options {
    /// Worker threads (0: all cores).
    pub threads: usize,
    pub stop: Arc<AtomicBool>,
    /// Stop (paused) after this many settings in this session.
    pub limit: Option<u64>,
    pub live: Option<Arc<Mutex<Stage1Progress>>>,
    pub on_progress: Option<ProgressHook>,
    /// Settings per batch (0: adapt to about two seconds).
    pub batch: u64,
}

/// Create a search job (status `paused` until [`run`] starts it). Returns the job id.
pub fn start(lib: &Library, spec: &Stage1Spec) -> Result<i64, String> {
    spec.check()?;
    let v = serde_json::to_value(spec).map_err(|e| e.to_string())?;
    let job = lib.create_job("stage1", &v)?;
    lib.update_job(job, Some(JobStatus::Paused), None, None)?;
    Ok(job)
}

fn job_spec(
    lib: &Library,
    job: i64,
) -> Result<(crate::library::store::JobRow, Stage1Spec), String> {
    let j = lib.job(job)?;
    if j.kind != "stage1" {
        return Err(format!(
            "job {job} is a {} job, not a stage 1 search",
            j.kind
        ));
    }
    let spec: Stage1Spec =
        serde_json::from_value(j.spec.clone()).map_err(|e| format!("job {job} spec: {e}"))?;
    Ok((j, spec))
}

/// Ask a running search (in any process) to stop at its next batch.
pub fn request_stop(lib: &Library, job: i64) -> Result<(), String> {
    let (j, _) = job_spec(lib, job)?;
    if j.status == JobStatus::Running {
        lib.update_job(job, Some(JobStatus::Paused), None, None)?;
    }
    Ok(())
}

/// A search's spec and last saved progress.
pub fn status(lib: &Library, job: i64) -> Result<(Stage1Spec, Stage1Progress), String> {
    let (j, spec) = job_spec(lib, job)?;
    let mut p: Stage1Progress = serde_json::from_value(j.progress.clone()).unwrap_or_default();
    p.job = job;
    p.status = match j.status {
        JobStatus::Running => "running",
        JobStatus::Paused => "paused",
        JobStatus::Done => "done",
        JobStatus::Failed => "failed",
    }
    .into();
    if p.passes.is_empty() {
        p.passes = spec.passes();
    }
    Ok((spec, p))
}

/// Unique key of a setting (as the catalog's unique index).
fn key(s: &Setting) -> String {
    format!(
        "{}|{}|{}|{}|{}x{}|{:?}|{}",
        s.notation().unwrap_or_else(|_| s.rule.clone()),
        tiling_key(&s.tiling),
        s.density,
        s.soup,
        s.grid[0],
        s.grid[1],
        s.boundary,
        s.gens
    )
}

/// Reference vectors for novelty, per space.
struct Refs {
    v: Vec<Vec<[f64; 16]>>,
}

impl Refs {
    fn novelty(&self, space: usize, d: &Descriptors, extra: &[[f64; 16]]) -> f64 {
        let x = novelty_vector(d);
        self.v[space]
            .iter()
            .chain(extra)
            .map(|r| super::score::distance(&x, r))
            .fold(f64::INFINITY, f64::min)
            .min(1.0)
    }
}

/// The built-in Quick Start rules (B/S and Generations without B0) shown on a space's tiling
/// with its neighbourhood.
fn quickstart_rules(sp: &Space) -> Vec<String> {
    let fam = &sp.tiling.family;
    let mut out: Vec<String> = Vec::new();
    for e in crate::quickstart::entries() {
        if e.scene.tiling.family != *fam || e.scene.tiling.params != sp.tiling.params {
            continue;
        }
        let Some(ca) = e.scene.cas.first() else {
            continue;
        };
        let Ok((r, hint)) = Rule::parse(&ca.rule) else {
            continue;
        };
        let nb = ca.neighborhood.or(hint).unwrap_or_default();
        if nb != sp.nb || crate::hunt::bits::bs_rule(&r).is_err() {
            continue;
        }
        let states = match r {
            Rule::Generations { states, .. } => states,
            _ => 2,
        };
        if states != sp.states {
            continue;
        }
        let n = r.notation();
        if !out.contains(&n) {
            out.push(n);
        }
    }
    out
}

/// What the workers need, built once per grid.
struct Kits(Mutex<HashMap<String, Arc<GridKit>>>);

impl Kits {
    fn get(&self, s: &Setting) -> Result<Arc<GridKit>, String> {
        let k = format!(
            "{}|{}x{}|{:?}",
            tiling_key(&s.tiling),
            s.grid[0],
            s.grid[1],
            s.boundary
        );
        if let Some(x) = self.0.lock().unwrap().get(&k) {
            return Ok(x.clone());
        }
        let kit = Arc::new(GridKit::new(s)?);
        self.0.lock().unwrap().insert(k, kit.clone());
        Ok(kit)
    }
}

/// Run (or resume) a search job until it is done or stopped. Returns its final progress. On an
/// error the job is marked `failed`; it can still be resumed.
pub fn run(lib: &Library, job: i64, opts: &Stage1Options) -> Result<Stage1Progress, String> {
    let r = run_inner(lib, job, opts);
    if r.is_err() && lib.job(job).is_ok_and(|j| j.kind == "stage1") {
        let _ = lib.update_job(job, Some(JobStatus::Failed), None, None);
    }
    r
}

fn run_inner(lib: &Library, job: i64, opts: &Stage1Options) -> Result<Stage1Progress, String> {
    let (j, spec) = job_spec(lib, job)?;
    if j.status == JobStatus::Done {
        return status(lib, job).map(|x| x.1);
    }
    spec.check()?;
    let mut cp: Checkpoint = match lib.job_checkpoint(job)? {
        Some(b) => serde_json::from_slice(&b).map_err(|e| format!("job {job} checkpoint: {e}"))?,
        None => Checkpoint::default(),
    };
    let model = match &cp.model {
        Some(m) => m.clone(),
        None => {
            let m = lib.ensure_score_model()?;
            cp.model = Some(m.clone());
            m
        }
    };
    let passes = spec.passes();
    let nspaces = spec.spaces.len();
    if cp.expanded.len() != nspaces {
        cp.expanded = vec![BTreeSet::new(); nspaces];
    }
    let threads = if opts.threads == 0 {
        std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(1)
    } else {
        opts.threads
    };
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(threads)
        .build()
        .map_err(|e| e.to_string())?;
    let kits = Kits(Mutex::new(HashMap::new()));
    // Coarse rules per space.
    let coarse: Vec<Vec<String>> = (0..nspaces)
        .map(|i| spec.space_rules(i))
        .collect::<Result<_, _>>()?;
    let mut p = std::mem::take(&mut cp.progress);
    p.job = job;
    p.passes = passes.clone();
    p.model_id = model.id;
    p.threads = threads;
    if p.spaces.len() != nspaces {
        p.spaces = spec
            .spaces
            .iter()
            .zip(&coarse)
            .map(|(sp, r)| SpaceTotals {
                space: sp.label(),
                rules: r.len() as u64,
                ..Default::default()
            })
            .collect();
    }
    // Reference vectors: built-in Quick Start rules at the base setting, other jobs' shown
    // settings, and this job's settings above the candidate threshold.
    let mut refs = Refs {
        v: vec![vec![]; nspaces],
    };
    for (si, sp) in spec.spaces.iter().enumerate() {
        let rules = quickstart_rules(sp);
        let base = spec.base_setting(si, "B3/S23");
        let kit = kits.get(&base)?;
        let vs: Vec<[f64; 16]> = pool.install(|| {
            rules
                .par_iter()
                .filter_map(|r| {
                    let s = Setting {
                        rule: r.clone(),
                        ..base.clone()
                    };
                    evaluate(&s, &kit).ok().map(|(d, _)| novelty_vector(&d))
                })
                .collect()
        });
        refs.v[si] = vs;
    }
    let space_of = |s: &Setting| -> Option<usize> {
        let tk = tiling_key(&s.tiling);
        let (r, nb) = s.rule().ok()?;
        let states = match r {
            Rule::Generations { states, .. } => states,
            _ => 2,
        };
        spec.spaces
            .iter()
            .position(|sp| tiling_key(&sp.tiling) == tk && sp.nb == nb && sp.states == states)
    };
    for row in lib.settings(&SettingsQuery {
        status: vec![SettingStatus::Auto, SettingStatus::Approved],
        order: Some("id".into()),
        ..Default::default()
    })? {
        if row.job == Some(job) {
            continue;
        }
        if let Some(si) = space_of(&row.setting) {
            refs.v[si].push(novelty_vector(&row.descriptors));
        }
    }
    // This job's settings so far (resume): evaluated keys and references.
    let mut done_keys: HashSet<String> = HashSet::new();
    let mine = lib.settings(&SettingsQuery {
        job: Some(job),
        order: Some("id".into()),
        ..Default::default()
    })?;
    for row in &mine {
        done_keys.insert(key(&row.setting));
        let f = features(&row.descriptors, row.setting.soup < 1.0);
        if model.score(&f) >= model.candidate
            && let Some(si) = space_of(&row.setting)
        {
            refs.v[si].push(novelty_vector(&row.descriptors));
        }
    }
    drop(mine);
    lib.update_job(job, Some(JobStatus::Running), None, None)?;
    let session = Instant::now();
    let base_elapsed = p.elapsed_s;
    let mut session_done = 0u64;
    let mut batch = if opts.batch > 0 { opts.batch } else { 64 };
    let finish = |lib: &Library, p: &mut Stage1Progress, st: JobStatus| -> Result<(), String> {
        p.status = match st {
            JobStatus::Done => "done",
            JobStatus::Paused => "paused",
            JobStatus::Failed => "failed",
            JobStatus::Running => "running",
        }
        .into();
        let v = serde_json::to_value(&*p).map_err(|e| e.to_string())?;
        lib.update_job(job, Some(st), Some(&v), None)
    };
    loop {
        if cp.pass >= passes.len() {
            finish(lib, &mut p, JobStatus::Done)?;
            publish(opts, &p);
            return Ok(p);
        }
        let pass_name = passes[cp.pass].clone();
        // Tasks of this pass.
        let total: u64 = if cp.pass == 0 {
            coarse.iter().map(|v| v.len() as u64).sum()
        } else {
            if cp.next == 0 && cp.tasks.is_empty() {
                cp.tasks = plan_pass(lib, job, &spec, &model, &pass_name, &mut cp.expanded)?
                    .into_iter()
                    .filter(|t| !done_keys.contains(&key(&t.setting)))
                    .collect();
                // Unique within the pass, first wins.
                let mut seen = HashSet::new();
                cp.tasks.retain(|t| seen.insert(key(&t.setting)));
            }
            cp.tasks.len() as u64
        };
        p.pass = cp.pass;
        p.pass_total = total;
        p.pass_done = cp.next;
        if cp.next >= total {
            cp.pass += 1;
            cp.next = 0;
            cp.tasks.clear();
            continue;
        }
        // Stop requests: this process, another process (job marked paused), a session limit.
        let other_stop = lib
            .job(job)
            .map(|j| j.status == JobStatus::Paused)
            .unwrap_or(false);
        let limit_hit = opts.limit.is_some_and(|l| session_done >= l);
        if opts.stop.load(Ordering::SeqCst) || other_stop || limit_hit {
            finish(lib, &mut p, JobStatus::Paused)?;
            publish(opts, &p);
            return Ok(p);
        }
        let mut n = batch.min(total - cp.next);
        if let Some(l) = opts.limit {
            n = n.min(l - session_done).max(1);
        }
        let tasks: Vec<Task> = (cp.next..cp.next + n)
            .map(|i| {
                if cp.pass == 0 {
                    let mut i = i as usize;
                    let mut si = 0;
                    while i >= coarse[si].len() {
                        i -= coarse[si].len();
                        si += 1;
                    }
                    Task {
                        space: si,
                        setting: spec.base_setting(si, &coarse[si][i]),
                    }
                } else {
                    cp.tasks[i as usize].clone()
                }
            })
            .collect();
        // Grids first (sequentially), then the evaluations in parallel.
        for t in &tasks {
            kits.get(&t.setting)?;
        }
        let t0 = Instant::now();
        let results: Vec<Result<Descriptors, String>> = pool.install(|| {
            tasks
                .par_iter()
                .map(|t| {
                    let kit = kits.get(&t.setting)?;
                    evaluate(&t.setting, &kit).map(|x| x.0)
                })
                .collect()
        });
        // Novelty against the references before this batch, in parallel.
        let pre: Vec<Option<f64>> = pool.install(|| {
            results
                .par_iter()
                .zip(&tasks)
                .map(|(r, t)| r.as_ref().ok().map(|d| refs.novelty(t.space, d, &[])))
                .collect()
        });
        let mut rows: Vec<NewSetting> = Vec::new();
        let mut added: Vec<Vec<[f64; 16]>> = vec![vec![]; nspaces];
        for ((r, t), nov) in results.into_iter().zip(&tasks).zip(pre) {
            let tot = &mut p.spaces[t.space];
            match r {
                Ok(mut d) => {
                    let x = novelty_vector(&d);
                    let within = added[t.space]
                        .iter()
                        .map(|r| super::score::distance(&x, r))
                        .fold(f64::INFINITY, f64::min);
                    d.novelty = Some(nov.unwrap_or(1.0).min(within).min(1.0));
                    let f = features(&d, t.setting.soup < 1.0);
                    let score = model.score(&f);
                    let status = SettingStatus::for_score(&model, score);
                    if score >= model.candidate {
                        added[t.space].push(novelty_vector(&d));
                    }
                    tot.evaluated += 1;
                    match status {
                        SettingStatus::Auto => tot.auto += 1,
                        SettingStatus::Candidate => tot.candidate += 1,
                        _ => {}
                    }
                    *tot.behaviour.entry(d.behaviour.clone()).or_default() += 1;
                    if score > tot.best_score || tot.best_rule.is_empty() {
                        tot.best_score = score;
                        tot.best_rule = t.setting.notation().unwrap_or_default();
                    }
                    rows.push(NewSetting {
                        setting: t.setting.clone(),
                        descriptors: d,
                        score,
                        status,
                        model_id: (model.id != 0).then_some(model.id),
                        pass: pass_name.clone(),
                    });
                }
                Err(_) => tot.failed += 1,
            }
            done_keys.insert(key(&t.setting));
        }
        for (si, v) in added.into_iter().enumerate() {
            refs.v[si].extend(v);
        }
        cp.next += n;
        session_done += n;
        p.pass_done = cp.next;
        p.evaluated += rows.len() as u64;
        let elapsed = session.elapsed().as_secs_f64();
        p.elapsed_s = base_elapsed + elapsed;
        p.per_s = session_done as f64 / elapsed.max(1e-9);
        p.updated = now_utc();
        p.status = "running".into();
        // The rows, then (knowing their ids and final statuses) the progress and checkpoint,
        // in one transaction.
        let cell = Mutex::new(p.clone());
        {
            let rows = &rows;
            let cp_ref = &cp;
            let cell = &cell;
            let fin = move |fl: &crate::library::Stage1Flush| -> Result<(serde_json::Value, Vec<u8>), String> {
                let mut q = cell.lock().unwrap().clone();
                note_flush(&mut q, rows, fl);
                let mut c = cp_ref.clone();
                c.progress = q.clone();
                let blob = serde_json::to_vec(&c).map_err(|e| e.to_string())?;
                let v = serde_json::to_value(&q).map_err(|e| e.to_string())?;
                *cell.lock().unwrap() = q;
                Ok((v, blob))
            };
            lib.stage1_flush(Some(job), rows, Some(&fin))?;
        }
        p = cell.into_inner().unwrap();
        publish(opts, &p);
        if opts.batch == 0 {
            let dt = t0.elapsed().as_secs_f64();
            let want = (batch as f64 * 2.0 / dt.max(1e-3)).clamp(16.0, 8192.0) as u64;
            batch = ((batch + want) / 2).max(16);
        }
    }
}

/// Count a flush in the progress: statuses, the histogram, the best and newest settings.
fn note_flush(p: &mut Stage1Progress, rows: &[NewSetting], fl: &crate::library::Stage1Flush) {
    for ((r, id), st) in rows.iter().zip(&fl.ids).zip(&fl.statuses) {
        *p.statuses.entry(st.name()).or_default() += 1;
        let bin = ((r.score / 10.0).floor() as usize).min(9);
        p.histogram[bin] += 1;
        let top = TopSetting {
            id: *id,
            rule: r.setting.notation().unwrap_or_default(),
            tiling: tiling_display_name(&r.setting.tiling),
            score: r.score,
            behaviour: r.descriptors.behaviour.clone(),
            status: st.name(),
            label: r.setting.label(),
        };
        if *st == SettingStatus::Auto {
            p.newest_auto.insert(0, top.clone());
            p.newest_auto.truncate(16);
        }
        p.best.push(top);
    }
    p.best.sort_by(|a, b| {
        b.score
            .total_cmp(&a.score)
            .then_with(|| a.label.cmp(&b.label))
    });
    p.best.dedup_by(|a, b| a.id == b.id);
    p.best.truncate(24);
}

fn publish(opts: &Stage1Options, p: &Stage1Progress) {
    if let Some(l) = &opts.live {
        *l.lock().unwrap() = p.clone();
    }
    if let Some(h) = &opts.on_progress {
        h(p);
    }
}

/// The tasks of a refinement or neighbour pass, from this job's settings so far.
fn plan_pass(
    lib: &Library,
    job: i64,
    spec: &Stage1Spec,
    model: &ScoreModel,
    pass: &str,
    expanded: &mut [BTreeSet<String>],
) -> Result<Vec<Task>, String> {
    let rows = lib.settings(&SettingsQuery {
        job: Some(job),
        order: Some("id".into()),
        ..Default::default()
    })?;
    // Ranked per space by this job's model, ties by label.
    let mut per: Vec<Vec<(f64, String, Setting)>> = vec![vec![]; spec.spaces.len()];
    for r in rows {
        let tk = tiling_key(&r.setting.tiling);
        let Ok((rule, nb)) = r.setting.rule() else {
            continue;
        };
        let states = match rule {
            Rule::Generations { states, .. } => states,
            _ => 2,
        };
        let Some(si) = spec
            .spaces
            .iter()
            .position(|sp| tiling_key(&sp.tiling) == tk && sp.nb == nb && sp.states == states)
        else {
            continue;
        };
        let f = features(&r.descriptors, r.setting.soup < 1.0);
        per[si].push((model.score(&f), r.setting.label(), r.setting));
    }
    for v in &mut per {
        v.sort_by(|a, b| b.0.total_cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
    }
    let mut out = Vec::new();
    if pass == "refine" {
        let rf = &spec.refine;
        for (si, v) in per.iter().enumerate() {
            for (_, _, s) in v.iter().take(rf.top) {
                for &d in &rf.densities {
                    if d != s.density {
                        out.push(Task {
                            space: si,
                            setting: Setting {
                                density: d,
                                ..s.clone()
                            },
                        });
                    }
                }
                for &x in &rf.soups {
                    if x != s.soup {
                        out.push(Task {
                            space: si,
                            setting: Setting {
                                soup: x,
                                ..s.clone()
                            },
                        });
                    }
                }
                for &g in &rf.grids {
                    if g != s.grid {
                        out.push(Task {
                            space: si,
                            setting: Setting {
                                grid: g,
                                ..s.clone()
                            },
                        });
                    }
                }
                for &g in &rf.gens {
                    if g != s.gens {
                        out.push(Task {
                            space: si,
                            setting: Setting {
                                gens: g,
                                ..s.clone()
                            },
                        });
                    }
                }
            }
        }
    } else if pass.starts_with("neighbours") {
        for (si, v) in per.iter().enumerate() {
            let (birth, survive) = digits(&spec.spaces[si])?;
            // The best setting of each rule, best rules first.
            let mut best: Vec<&Setting> = Vec::new();
            let mut seen: HashSet<String> = HashSet::new();
            for (_, _, s) in v {
                let n = s.notation().unwrap_or_default();
                if seen.insert(n.clone()) && !expanded[si].contains(&n) {
                    best.push(s);
                }
                if best.len() >= spec.neighbours.top {
                    break;
                }
            }
            for s in best {
                let n = s.notation().unwrap_or_default();
                expanded[si].insert(n);
                let bare = Rule::parse(&s.rule).map(|x| x.0.notation())?;
                for r in rule_neighbours(&bare, &birth, &survive)? {
                    out.push(Task {
                        space: si,
                        setting: Setting {
                            rule: r,
                            ..s.clone()
                        },
                    });
                }
            }
        }
    }
    Ok(out)
}

/// A search running on a background thread.
pub struct Stage1Handle {
    pub job: i64,
    pub stop: Arc<AtomicBool>,
    pub live: Arc<Mutex<Stage1Progress>>,
    thread: Option<std::thread::JoinHandle<Result<Stage1Progress, String>>>,
}

impl Stage1Handle {
    /// Run (or resume) a search job on a new thread.
    pub fn spawn(lib: Library, job: i64, mut opts: Stage1Options) -> Stage1Handle {
        let stop = opts.stop.clone();
        let live = opts
            .live
            .get_or_insert_with(|| Arc::new(Mutex::new(Stage1Progress::default())))
            .clone();
        {
            let mut l = live.lock().unwrap();
            if let Ok((_, p)) = status(&lib, job) {
                *l = p;
            }
            l.job = job;
            l.status = "running".into();
        }
        let thread = std::thread::Builder::new()
            .name(format!("stage1-{job}"))
            .spawn(move || {
                let r = run(&lib, job, &opts);
                if let Err(e) = &r
                    && let Some(live) = &opts.live
                {
                    let mut l = live.lock().unwrap();
                    l.status = "failed".into();
                    l.error = Some(e.clone());
                }
                r
            })
            .ok();
        Stage1Handle {
            job,
            stop,
            live,
            thread,
        }
    }
    pub fn progress(&self) -> Stage1Progress {
        self.live.lock().unwrap().clone()
    }
    pub fn is_finished(&self) -> bool {
        self.thread.as_ref().is_none_or(|t| t.is_finished())
    }
    pub fn stop(&self) {
        self.stop.store(true, Ordering::SeqCst);
    }
    pub fn join(mut self) -> Result<Stage1Progress, String> {
        match self.thread.take() {
            Some(t) => t
                .join()
                .map_err(|_| "the stage 1 thread panicked".to_string())?,
            None => Err("the stage 1 thread could not start".into()),
        }
    }
}
