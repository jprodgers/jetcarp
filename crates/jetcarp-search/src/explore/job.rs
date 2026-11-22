//! The search itself ([`Search`]): MAP-Elites over tiling, rule family, rule, and seed. A step
//! runs GPU rounds on a thread of its own (for the rules a [`Backend`] can run) beside CPU rounds
//! on the run's thread pool (the rest, or everything when there is no GPU), with periodic
//! confirmation of the best runs on bigger grids, until its deadline; then
//! [`take_delta`](Search::take_delta) hands over what it measured and changed, which the job
//! writes to the search's working database (`crate::explore::store`) in one transaction.
//!
//! A search starts from a saved state ([`store::Saved`]: its archive, confirmed set, coverage,
//! family totals, seen set, counters, and random states), so a stopped one carries on where it
//! was. With a GPU it is not deterministic: the GPU and CPU rounds share the archive as they run,
//! so two searches with the same settings try different candidates. On the processor alone, the
//! same seed, settings and number of threads give the same runs, ids and confirmations whatever
//! the steps' length and however the threads take up the runs (see [`Search::step`]'s CPU
//! rounds), as long as no run meets its time cap, which is wall time.

use crate::explore::archive::{self, Archive, Eval, ScoreModel};
use crate::explore::breed::{self, Parent};
use crate::explore::store::{Delta, FamStat, RunOut, Saved};
use jetcarp_core::explore::batch::{
    Backend, BatchJob, BatchWorld, GIVEN_UP, WorldTrace, field_bytes, trace_world,
    trace_world_keeping,
};
use jetcarp_core::explore::cand::Cand;
use jetcarp_core::explore::features::{
    Fate, Features, frames_for, frames_with_trade, measure_run, measure_signs, trade_gen,
};
use jetcarp_core::explore::seeds::seed_for;
use jetcarp_core::explore::space::{self, Fam, Grid, TilingChoice, sample_ltl, sample_rule};
use jetcarp_core::ltl::Hood;
use jetcarp_core::rng::Rng;
use jetcarp_core::rules::Rule;
use jetcarp_core::topology::{Nb, Storage};
use jetcarp_core::world::World;
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

pub use crate::explore::store::feat_from_blob;

fn d_screen_cells() -> u32 {
    16_000
}
fn d_screen_gens() -> u32 {
    1024
}
fn d_confirm_cells() -> u32 {
    64_000
}
fn d_confirm_gens() -> u32 {
    4096
}
fn d_confirm_seeds() -> u32 {
    3
}
fn d_batch() -> usize {
    1024
}
/// A spec without `sizing` is a search made before it: it keeps the sizing its runs were made
/// with.
fn d_old_sizing() -> u32 {
    1
}
/// A spec without `schedule` is a search made before it: it keeps its whole tiling list.
fn d_old_schedule() -> u32 {
    1
}

/// How a search made now fits its tilings to its rule families (a search's `schedule`, #239).
/// Schedule 1, what searches made before JETCARP 0.4.3 keep: every tiling chosen stays in the
/// list, even one none of the search's families can run on. Schedule 2: such tilings are left
/// out, a family chosen by name that can run on none of the tilings chosen is refused, and when
/// every family the processor runs is tied to one grid (the elementary ones, or isotropic rules
/// when there is no GPU), every one of its rounds goes to those grids. A search with every family
/// keeps today's runs under both.
pub const SCHEDULE: u32 = 2;

/// Families the explore search samples only when a search names them: they are left out of the
/// default schedule, so a search that chooses no families runs as it did before they came
/// (isotropic rules, #239).
pub const OPT_IN: [Fam; 1] = [Fam::Isotropic];

/// The families an explore search can sample, in the order the app lists them: the GPU's, the
/// processor's, Larger than Life, and the ones taken only when named ([`OPT_IN`]). Von Neumann's
/// 29-state rule is not among them.
pub fn sampled_families() -> Vec<Fam> {
    let mut v: Vec<Fam> = GPU_FAMS
        .iter()
        .map(|x| x.0)
        .filter(|f| !OPT_IN.contains(f))
        .collect();
    v.extend(CPU_FAMS.iter().map(|x| x.0).filter(|f| !OPT_IN.contains(f)));
    v.push(Fam::Ltl);
    v.extend(OPT_IN);
    v
}

/// Cells the search's cache of grids holds at most, in all. A grid costs about 87 bytes a cell
/// (the 1,359 tilings' grids counted by allocation: 1.91 GB for 21.8 million cells at 16,000,
/// 7.47 GB for 86.8 million at 64,000; 67 to 125 bytes a cell on periodic tilings, 200 to 320 on
/// the aperiodic patches, which have 35,000 cells at both sizes), and about 94 with the cache's
/// own share, so every tiling's grids at both default sizes, as an unbounded cache ends up
/// holding, come to about 109 million cells and 9.4 GB. This keeps the most recently used, about
/// 3 GB, which holds every screening grid with room for the confirming grids in use; a grid let
/// go is built again in about 25 ms (16,000 cells) or 0.1 to 0.2 s (64,000 cells).
pub const GRID_CACHE_CELLS: usize = 32_000_000;

/// The grids a search has built, by tiling and size, up to [`GRID_CACHE_CELLS`] cells; past
/// that the least recently used go (a run still using one keeps it alive until it ends).
#[derive(Default)]
struct GridCache {
    map: HashMap<(String, u32), (Arc<Grid>, u64)>,
    cells: usize,
    limit: usize,
    tick: u64,
    hits: u64,
    builds: u64,
}

impl GridCache {
    fn new(limit: usize) -> GridCache {
        GridCache {
            limit,
            ..Default::default()
        }
    }

    fn get(&mut self, key: &(String, u32)) -> Option<Arc<Grid>> {
        self.tick += 1;
        let (g, used) = self.map.get_mut(key)?;
        *used = self.tick;
        self.hits += 1;
        Some(g.clone())
    }

    /// Let the least recently used grids that no run is using go, until the cache holds at most
    /// `cells` cells.
    fn shed(&mut self, cells: usize) {
        let mut idle: Vec<((String, u32), u64)> = self
            .map
            .iter()
            .filter(|(_, (g, _))| Arc::strong_count(g) == 1)
            .map(|(k, (_, used))| (k.clone(), *used))
            .collect();
        idle.sort_by_key(|x| x.1);
        for (k, _) in idle {
            if self.cells <= cells {
                break;
            }
            if let Some((g, _)) = self.map.remove(&k) {
                self.cells -= g.topo.len();
            }
        }
    }

    /// Keeps `g` (unless another thread built the same grid meanwhile: then that one), letting the
    /// least recently used others go while the cache is over its limit.
    fn insert(&mut self, key: (String, u32), g: Arc<Grid>) -> Arc<Grid> {
        self.tick += 1;
        self.builds += 1;
        if let Some((have, used)) = self.map.get_mut(&key) {
            *used = self.tick;
            return have.clone();
        }
        self.cells += g.topo.len();
        self.map.insert(key.clone(), (g.clone(), self.tick));
        while self.cells > self.limit && self.map.len() > 1 {
            let oldest = self
                .map
                .iter()
                .filter(|(k, _)| **k != key)
                .min_by_key(|(_, (_, used))| *used)
                .map(|(k, _)| k.clone());
            let Some(k) = oldest else { break };
            if let Some((old, _)) = self.map.remove(&k) {
                self.cells -= old.topo.len();
            }
        }
        g
    }
}

/// How the grid cache has done: grids and cells held, look-ups answered from it, grids built.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct GridCacheStats {
    pub grids: usize,
    pub cells: usize,
    pub hits: u64,
    pub builds: u64,
}

/// A search's settings.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Settings {
    /// Cells of a screening run's grid, and its generations.
    #[serde(default = "d_screen_cells")]
    pub screen_cells: u32,
    #[serde(default = "d_screen_gens")]
    pub screen_gens: u32,
    /// Cells and generations of a confirmation run, and how many seeds confirm an elite.
    #[serde(default = "d_confirm_cells")]
    pub confirm_cells: u32,
    #[serde(default = "d_confirm_gens")]
    pub confirm_gens: u32,
    #[serde(default = "d_confirm_seeds")]
    pub confirm_seeds: u32,
    /// Most worlds in a GPU batch.
    #[serde(default = "d_batch")]
    pub batch: usize,
    /// Where the random draws start (a search carries its random states on from there).
    #[serde(default, with = "jetcarp_core::json_int::uint")]
    pub seed: u64,
    /// Only these tilings (keys: `square`, `ih28`, `catalog:k2-001`, ...); empty: all.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tilings: Vec<String>,
    /// Only the tilings of these groups ([`space::TILING_GROUPS`]: `square`, `hexagon`, ...,
    /// `aperiodic`); empty: every group.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tiling_groups: Vec<String>,
    /// Leave out these tilings.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub skip_tilings: Vec<String>,
    /// Leave out the tilings of these groups.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub skip_tiling_groups: Vec<String>,
    /// Only these rule families; empty: the default schedule, every family but the ones taken
    /// only when named ([`OPT_IN`]: isotropic rules).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub families: Vec<Fam>,
    /// Leave out these rule families.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub skip_families: Vec<Fam>,
    /// A fitted score model (`jetcarp jobs refit`) to score runs with, in place of the built-in
    /// score (for its family only, when it was fitted to one); none: the built-in score.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub score: Option<ScoreModel>,
    /// JACA rules another job found (`--parents-from`), transplanted and tuned early.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub parents: Vec<Parent>,
    /// How its grids are sized from the cells asked ([`space::SIZING`]): 2 for a search made
    /// now (an aperiodic patch is the level nearest the cells asked), 1 for one made before
    /// JETCARP 0.4.3, which keeps its sizing when it carries on.
    #[serde(default = "d_old_sizing")]
    pub sizing: u32,
    /// How it fits its tilings to its rule families ([`SCHEDULE`]): 2 for a search made now, 1
    /// for one made before JETCARP 0.4.3, which keeps its whole tiling list when it carries on.
    #[serde(default = "d_old_schedule")]
    pub schedule: u32,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            screen_cells: d_screen_cells(),
            screen_gens: d_screen_gens(),
            confirm_cells: d_confirm_cells(),
            confirm_gens: d_confirm_gens(),
            confirm_seeds: d_confirm_seeds(),
            batch: d_batch(),
            seed: 0,
            tilings: vec![],
            tiling_groups: vec![],
            skip_tilings: vec![],
            skip_tiling_groups: vec![],
            families: vec![],
            skip_families: vec![],
            score: None,
            parents: vec![],
            sizing: space::SIZING,
            schedule: SCHEDULE,
        }
    }
}

impl Settings {
    /// Can a search run with these?
    pub fn check(&self) -> Result<(), String> {
        self.check_in(&space::all_tilings())
    }

    /// [`check`](Self::check) against this tiling list.
    fn check_in(&self, all: &[TilingChoice]) -> Result<(), String> {
        if self.screen_cells < 64 || self.confirm_cells < 64 {
            return Err("grids need at least 64 cells".into());
        }
        if self.screen_gens == 0 || self.confirm_gens == 0 {
            return Err("runs need at least one generation".into());
        }
        if self.batch == 0 {
            return Err("a GPU batch needs at least one world".into());
        }
        if !(1..=space::SIZING).contains(&self.sizing) {
            return Err(format!(
                "sizing must be 1 to {}, not {}",
                space::SIZING,
                self.sizing
            ));
        }
        if !(1..=SCHEDULE).contains(&self.schedule) {
            return Err(format!(
                "schedule must be 1 to {SCHEDULE}, not {}",
                self.schedule
            ));
        }
        for t in self.tilings.iter().chain(&self.skip_tilings) {
            if !all.iter().any(|x| &x.key == t) {
                return Err(format!(
                    "no tiling '{t}' in the search's list (ids: square, hex, trihex, spectre, ih28, k2-001, ...)"
                ));
            }
        }
        for g in self.tiling_groups.iter().chain(&self.skip_tiling_groups) {
            if !space::TILING_GROUPS.contains(&g.as_str()) {
                return Err(format!(
                    "no tiling group '{g}' (groups: {})",
                    space::TILING_GROUPS.join(", ")
                ));
            }
        }
        if self.schedule >= 2 {
            let sampled = sampled_families();
            if let Some(f) = self.families.iter().find(|f| !sampled.contains(f)) {
                return Err(format!(
                    "the explore search does not sample {} rules (families: {})",
                    f.name(),
                    family_names(&sampled)
                ));
            }
        }
        let (g, c, ltl) = families(&self.families, &self.skip_families, true);
        if g.is_empty() && c.is_empty() && !ltl {
            return Err("no rule family to search".into());
        }
        let picked = self.picked_tilings(all.to_vec());
        if picked.is_empty() {
            return Err(
                "no tiling left to search: the tilings and groups chosen leave none".into(),
            );
        }
        if self.schedule >= 2 {
            for f in self.named_families() {
                if let Some(k) = f.only_on()
                    && !picked.iter().any(|t| t.key == k)
                {
                    return Err(format!(
                        "{} rules run only on the {k} tiling, which this search leaves out",
                        f.name()
                    ));
                }
            }
            if self.chosen_tilings(picked).is_empty() {
                return Err("none of the rule families chosen runs on the tilings chosen".into());
            }
        }
        if let Some(m) = &self.score {
            m.check()?;
        }
        Ok(())
    }

    /// The families named (`families`), less those left out (`skip_families`).
    fn named_families(&self) -> Vec<Fam> {
        self.families
            .iter()
            .copied()
            .filter(|f| !self.skip_families.contains(f))
            .collect()
    }

    /// Every family the search samples, on the GPU or the processor (and Larger than Life).
    pub fn chosen_families(&self) -> Vec<Fam> {
        let (_, c, ltl) = families(&self.families, &self.skip_families, false);
        let mut v: Vec<Fam> = c.into_iter().map(|x| x.0).collect();
        if ltl {
            v.push(Fam::Ltl);
        }
        v
    }

    /// The tilings of `all` chosen (`tilings`, `tiling_groups`), less those left out
    /// (`skip_tilings`, `skip_tiling_groups`), in `all`'s order.
    fn picked_tilings(&self, mut all: Vec<TilingChoice>) -> Vec<TilingChoice> {
        if !self.tilings.is_empty() {
            all.retain(|t| self.tilings.contains(&t.key));
        }
        if !self.tiling_groups.is_empty() {
            all.retain(|t| self.tiling_groups.contains(&t.group));
        }
        if !self.skip_tilings.is_empty() || !self.skip_tiling_groups.is_empty() {
            all.retain(|t| {
                !self.skip_tilings.contains(&t.key) && !self.skip_tiling_groups.contains(&t.group)
            });
        }
        all
    }

    /// The tilings the search draws from, out of `all` (the whole list, [`space::all_tilings`]):
    /// the ones chosen less the ones left out, and under schedule 2 ([`SCHEDULE`]) less those
    /// none of its families can run on.
    pub fn chosen_tilings(&self, all: Vec<TilingChoice>) -> Vec<TilingChoice> {
        let mut t = self.picked_tilings(all);
        if self.schedule >= 2 {
            let fams = self.chosen_families();
            t.retain(|x| fams.iter().any(|f| f.runs_on(&x.key)));
        }
        t
    }
}

/// Families' names, separated by commas.
fn family_names(fams: &[Fam]) -> String {
    fams.iter().map(|f| f.name()).collect::<Vec<_>>().join(", ")
}

pub const WINDOW: usize = 17;
/// Longest a screening run on the processor may take. A slower one (a sandpile, which takes tens
/// of seconds at the default size; Lenia on a slow machine) stops there and is kept as a run of
/// the generations it reached: its candidate's `gens`, below the usual run length, so it replays
/// exactly. A step then ends within seconds of its deadline, and no round waits on one run for a
/// minute. It is wall time: a run's world steps in parallel on the run's pool, and while its
/// thread waits for a step it may take up another run, so on a busy pool a run reaches fewer
/// generations than alone. (Each run on a thread of its own, stepping alone, kept the caps exact
/// but screened about a fifth fewer runs.)
pub const CPU_RUN_S: f64 = 8.0;
/// The same for a confirmation run on the processor (four times the cells, four times the
/// generations).
pub const CPU_CONFIRM_RUN_S: f64 = 4.0 * CPU_RUN_S;
/// Worlds in a session's first GPU batch when the step has an end: small, to time the GPU before
/// a batch is sized to the time left.
const FIRST_GPU_WORLDS: usize = 64;
/// Share of the time left a GPU batch is sized to fill, for the error in its estimate.
const GPU_FILL: f64 = 0.7;
/// Seconds of the GPU's time a batch of confirmation runs is sized to at most, by its pace so far
/// (a first one is small): its 288 runs on 64,000 cells took a minute on a laptop's GPU, and
/// screening on the GPU waited for them (#83). As long as a run on the processor may take.
pub const GPU_CONFIRM_S: f64 = CPU_RUN_S;
/// Elites the GPU confirms every eight of its rounds (in batches of [`GPU_CONFIRM_S`] at most,
/// every other round until they are done).
const GPU_CONFIRMS: u64 = 96;
/// Neighbour reads one Larger than Life batch may do (a few seconds of GPU time).
const LTL_NEIGHBOUR_UPDATES: f64 = 1.5e12;
/// Seen identities kept in a hash set before they are merged into the sorted list.
const FRESH_MAX: usize = 1 << 20;

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
    // Only when named ([`OPT_IN`]), and on the square grid only: the GPU gives them rounds of
    // their own there (see `gpu_plan`), as many as this weight's share (#245).
    (Fam::Isotropic, 8.0),
];

/// CPU sampling weights. A family both lists hold (isotropic rules) runs on the GPU when there
/// is one, and on the processor only when there is not.
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
    // Only when named ([`OPT_IN`]); here only without a GPU, where they keep the place and
    // weight they had when only the processor ran them (#239), so a search on the processor
    // alone draws as it did then (#245).
    (Fam::Isotropic, 8.0),
];

/// The GPU loop's families, the CPU loop's (with a GPU, or without one: then it also takes the
/// GPU's), and whether Larger than Life rounds run (on the GPU), within `only` (empty: every
/// family but [`OPT_IN`]'s) and less `skip`. A family in both lists goes to the GPU's when there
/// is a GPU, and keeps its place in the processor's when there is not.
fn families(only: &[Fam], skip: &[Fam], gpu: bool) -> (Vec<(Fam, f64)>, Vec<(Fam, f64)>, bool) {
    let keep = |f: &Fam| {
        !skip.contains(f)
            && if only.is_empty() {
                !OPT_IN.contains(f)
            } else {
                only.contains(f)
            }
    };
    let on_gpu = |f: &Fam| GPU_FAMS.iter().any(|x| x.0 == *f);
    let on_cpu = |f: &Fam| CPU_FAMS.iter().any(|x| x.0 == *f);
    let g: Vec<(Fam, f64)> = GPU_FAMS.iter().copied().filter(|x| keep(&x.0)).collect();
    let mut c: Vec<(Fam, f64)> = CPU_FAMS
        .iter()
        .copied()
        .filter(|x| keep(&x.0) && !(gpu && on_gpu(&x.0)))
        .collect();
    if !gpu {
        c.extend(g.iter().copied().filter(|x| !on_cpu(&x.0)));
    }
    (g, c, keep(&Fam::Ltl))
}

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

/// Where a run came from (#28): the run it was made from, and when that is another job's
/// (`--parents-from`), that job and its search's uuid (empty for a spec made before 0.4.3); none
/// for a fresh draw.
pub type Lineage = Option<(i64, Option<(i64, String)>)>;

/// A run's `parent`, `parent_job` and `parent_search` from where it came from.
fn lineage_cols(from: Lineage) -> (Option<i64>, Option<i64>, Option<String>) {
    match from {
        None => (None, None, None),
        Some((run, None)) => (Some(run), None, None),
        Some((run, Some((job, search)))) => {
            (Some(run), Some(job), (!search.is_empty()).then_some(search))
        }
    }
}

/// A candidate made: its rule (none when it could not be made), seed kind, seed, origin, and the
/// run it was made from.
type Made = (
    Option<Rule>,
    String,
    jetcarp_core::world::SeedSpec,
    &'static str,
    Lineage,
);

/// A parent for a transplant or a balance variant.
struct Pick {
    rule: String,
    tiling: String,
    nb: Nb,
    seed: jetcarp_core::world::SeedSpec,
    weaker: Option<breed::Sign>,
    /// Its run: an elite of this search, or a run of the job it came from (that job, and its
    /// search's uuid).
    from: (i64, Option<(i64, String)>),
}

/// How a candidate is made.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Way {
    /// A fresh draw.
    Random,
    /// An elite's rule, changed.
    Mutate,
    /// An elite's rule from a new starting pattern.
    Seed,
    /// The least-tried starting pattern, with a fresh rule.
    Cover,
    /// A catalogue rule.
    Named,
    /// JACA: a strong pair on a grid it has not run on.
    Transplant,
    /// JACA: a pair whose weaker sign was losing, changed to help it.
    Balance,
}

/// The way for a family and a roll in [0, 1). JACA's effort: random 30%, mutate 20%, transplant
/// 15%, balance 15%, seed 10%, cover 10%. The other families: random 40%, mutate 25%, seed 15%,
/// cover 10%, named 10%.
fn way(fam: Fam, roll: f64) -> Way {
    if fam == Fam::Jaca {
        match roll {
            r if r < 0.30 => Way::Random,
            r if r < 0.50 => Way::Mutate,
            r if r < 0.65 => Way::Transplant,
            r if r < 0.80 => Way::Balance,
            r if r < 0.90 => Way::Seed,
            _ => Way::Cover,
        }
    } else {
        match roll {
            r if r < 0.4 => Way::Random,
            r if r < 0.65 => Way::Mutate,
            r if r < 0.8 => Way::Seed,
            r if r < 0.9 => Way::Cover,
            _ => Way::Named,
        }
    }
}

/// The identities tried: a sorted list (loaded) and a set of the ones tried since.
#[derive(Default)]
struct Seen {
    base: Vec<u64>,
    fresh: HashSet<u64>,
    /// Tried since the last delta.
    new: Vec<u64>,
}

impl Seen {
    fn insert(&mut self, h: u64) -> bool {
        if self.base.binary_search(&h).is_ok() || !self.fresh.insert(h) {
            return false;
        }
        self.new.push(h);
        if self.fresh.len() > FRESH_MAX {
            let mut add: Vec<u64> = self.fresh.drain().collect();
            add.sort_unstable();
            let old = std::mem::take(&mut self.base);
            let mut out = Vec::with_capacity(old.len() + add.len());
            let (mut i, mut j) = (0, 0);
            while i < old.len() || j < add.len() {
                if j >= add.len() || (i < old.len() && old[i] < add[j]) {
                    out.push(old[i]);
                    i += 1;
                } else {
                    out.push(add[j]);
                    j += 1;
                }
            }
            self.base = out;
        }
        true
    }
    fn len(&self) -> usize {
        self.base.len() + self.fresh.len()
    }
}

/// A run as [`Search::take_in`] takes it: its id, stage, parent and seed kind, and its candidate
/// and measures (`None` when its family or seed does not read).
#[derive(Clone, Debug)]
pub struct TakenRun {
    pub id: i64,
    pub stage: i64,
    pub parent: Option<i64>,
    pub seed_kind: String,
    pub ev: Option<Eval>,
}

/// What a step has measured and changed, until [`Search::take_delta`].
#[derive(Default)]
struct Pending {
    rows: Vec<RunOut>,
    coverage: HashMap<(String, Fam, String), u32>,
    families: HashMap<Fam, FamStat>,
    confirmed: Vec<i64>,
}

/// One family's totals, for reports.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct FamilyLine {
    pub family: String,
    /// Screened runs, their mean and best score, and the elites of the family.
    pub runs: u64,
    pub mean: f64,
    pub best: f64,
    pub elites: u64,
}

/// When a step ends.
pub struct StepLimits<'a> {
    /// The step's deadline: no round starts after it (one under way runs to its end).
    pub until: Instant,
    /// The run's end (its time budget, or a Quick Start build's minutes): runs on the processor
    /// still going then are given up, unrecorded, and a GPU batch is sized to end before it (one
    /// still going is given up too, between two of its submissions).
    pub end: Option<Instant>,
    /// Set to stop: no round starts, and runs on the processor and a GPU batch still going are
    /// given up, unrecorded.
    pub stop: &'a AtomicBool,
    /// Stop once this many runs have been screened in all.
    pub max_screened: Option<u64>,
}

impl StepLimits<'_> {
    /// Runs under way are to be given up: a stop, or the run's end reached.
    fn cut(&self) -> bool {
        self.stop.load(Ordering::SeqCst) || self.end.is_some_and(|e| Instant::now() >= e)
    }
}

/// How a run on the processor went.
enum Ran {
    /// Measured: in full, or cut short by its time cap (then its `gens` are the generations it
    /// reached).
    Done(Box<RunOut>),
    /// It could not be run or measured (counted as a panic when it panicked).
    Failed,
    /// Given up at a stop or the run's end; nothing is recorded.
    GivenUp,
}

/// For tests: every generation of a run on the processor waits this long more (0: none), so a
/// test can hold slow runs under way at a step's or a build's end.
#[doc(hidden)]
pub static SLOW_GENERATION_MS: AtomicU64 = AtomicU64::new(0);

/// For tests: each of the next [`HOLD_RUNS`] runs on the processor holds this many MB of memory
/// of its own (written, so resident) while it runs (0: none), so a test can make the search's
/// memory rise and fall.
#[doc(hidden)]
pub static HOLD_MB: AtomicU64 = AtomicU64::new(0);
#[doc(hidden)]
pub static HOLD_RUNS: AtomicI64 = AtomicI64::new(0);

/// Work on the processor under way on this thread (a run, or an elite's confirmation), while it
/// lives. A thread waiting inside a run's parallel step may take up other work of the pool
/// meanwhile, another run or a confirmation, which holds the first until it ends; the time the
/// thread spends on work it took up so is not the first run's own, and its time cap does not
/// count it ([`Busy::own`]). A run's measures then never depend on what its thread took up.
struct Busy;

thread_local! {
    /// The work under way on this thread, innermost last: when each started, and the time spent
    /// meanwhile on work taken up inside it.
    static BUSY: std::cell::RefCell<Vec<(Instant, std::time::Duration)>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

impl Busy {
    fn enter() -> Busy {
        BUSY.with(|b| {
            b.borrow_mut()
                .push((Instant::now(), std::time::Duration::ZERO))
        });
        Busy
    }

    /// The innermost work's own time so far: since it started, less the time spent on work taken
    /// up inside it.
    fn own() -> std::time::Duration {
        BUSY.with(|b| {
            b.borrow()
                .last()
                .map_or(std::time::Duration::ZERO, |(t, nested)| {
                    t.elapsed().saturating_sub(*nested)
                })
        })
    }
}

impl Drop for Busy {
    fn drop(&mut self) {
        BUSY.with(|b| {
            let mut b = b.borrow_mut();
            if let Some((t, _)) = b.pop()
                && let Some(outer) = b.last_mut()
            {
                outer.1 += t.elapsed();
            }
        });
    }
}

/// A round's screened runs, published in the order they were made as soon as all before them
/// have ended: a run that ends is offered to the archive and counted within moments, while the
/// archive takes them in the same order whatever order the threads end them in.
struct Release {
    /// The next run to publish, and each run's outcome once it has ended (`Some(None)`: it was
    /// not measured).
    next: usize,
    ended: Vec<Option<Option<RunOut>>>,
}

/// What a GPU batch costs: seconds per world-cell-generation (times the reach, for Larger than
/// Life) of the last batches, plain rules and Larger than Life apart, and Larger than Life on
/// lattices and on aperiodic patches apart (whose reaches are read from lists, about forty
/// times slower a cell: a batch sized from the lattices' pace ran 105 s, not 10). Not kept
/// between sessions.
#[derive(Default)]
struct GpuPace {
    plain: Option<f64>,
    ltl: Option<f64>,
    ltl_aperiodic: Option<f64>,
}

impl GpuPace {
    fn slot(&mut self, ltl: bool, lattice: bool) -> &mut Option<f64> {
        match (ltl, lattice) {
            (false, _) => &mut self.plain,
            (true, true) => &mut self.ltl,
            (true, false) => &mut self.ltl_aperiodic,
        }
    }
    fn get(&mut self, ltl: bool, lattice: bool) -> Option<f64> {
        *self.slot(ltl, lattice)
    }
    fn note(&mut self, ltl: bool, lattice: bool, work: f64, secs: f64) {
        if work <= 0.0 || secs <= 0.0 {
            return;
        }
        let x = secs / work;
        let p = self.slot(ltl, lattice);
        // The newer batches count for more (the device warms up, grids differ).
        *p = Some(p.map_or(x, |old| 0.5 * old + 0.5 * x));
    }
}

/// Seconds until `end` (None: no end).
fn secs_left(end: Option<Instant>) -> Option<f64> {
    end.map(|e| e.saturating_duration_since(Instant::now()).as_secs_f64())
}

/// Worlds of `work` units each that fit in `left` seconds at `pace` (seconds a unit), at most
/// `most`: 0 when not even one does. Without a pace yet, a small first batch when the time left
/// is short, else `most`.
fn fit_worlds(pace: Option<f64>, work: f64, left: Option<f64>, most: usize) -> usize {
    let Some(left) = left else {
        return most;
    };
    match pace {
        Some(p) if p > 0.0 && work > 0.0 => {
            ((GPU_FILL * left / (p * work)).floor().max(0.0) as usize).min(most)
        }
        _ if left < 120.0 => most.min(FIRST_GPU_WORLDS),
        _ => most,
    }
}

/// A live search.
pub struct Search {
    set: Settings,
    tilings: Vec<TilingChoice>,
    tiling_total: f64,
    grids: Mutex<GridCache>,
    archive: Mutex<Archive>,
    seen: Mutex<Seen>,
    coverage: Mutex<HashMap<(usize, Fam, String), u32>>,
    family_totals: Mutex<HashMap<Fam, FamStat>>,
    named: Mutex<Vec<(usize, String)>>,
    next_id: AtomicI64,
    screened: AtomicU64,
    confirmed: AtomicU64,
    pending: Mutex<Pending>,
    rng_gpu: Mutex<Rng>,
    rng_cpu: Mutex<Rng>,
    gpu_rounds: AtomicU64,
    /// Elites the GPU owes a confirmation ([`GPU_CONFIRMS`] every eight rounds).
    gpu_due: AtomicU64,
    cpu_rounds: AtomicU64,
    panics: AtomicU64,
    gpu_pace: Mutex<GpuPace>,
    /// Longest a run on the processor may take: screening, confirmation.
    cpu_caps: (std::time::Duration, std::time::Duration),
    /// Seconds of the GPU's time a confirmation batch is sized to at most ([`GPU_CONFIRM_S`]).
    gpu_confirm_s: f64,
    /// Runs cut short by their time cap, and runs given up, in this session.
    capped: AtomicU64,
    given_up: AtomicU64,
    /// The tilings each JACA pair has been transplanted to in this session.
    transplanted: Mutex<HashMap<String, HashSet<String>>>,
    /// Mean and largest neighbour counts by tiling and neighbourhood.
    degrees: Mutex<HashMap<(String, u8), (f64, usize)>>,
    /// Where the next parent of `--parents-from` is taken from.
    parent_turn: AtomicU64,
}

impl Search {
    /// A search with these settings, carrying on from `saved` (a default one: from the start).
    pub fn new(set: Settings, mut saved: Saved) -> Result<Search, String> {
        let all = space::all_tilings();
        set.check_in(&all)?;
        let tilings = set.chosen_tilings(all);
        let tiling_total: f64 = tilings.iter().map(|t| t.weight).sum();
        // The catalogue's rules, less those of families taken only when named, unless named
        // (isotropic rules). A round passes over the rules of families it does not run.
        let chosen = set.chosen_families();
        let named_ok = |f: Fam| !OPT_IN.contains(&f) || chosen.contains(&f);
        let mut named = Vec::new();
        for (i, t) in tilings.iter().enumerate() {
            if t.twin_of.is_none() {
                for e in jetcarp_core::rules::catalog() {
                    if Rule::parse(e.notation).is_ok_and(|(r, _)| named_ok(Fam::of(&r))) {
                        named.push((i, e.notation.to_string()));
                    }
                }
            }
        }
        let index: HashMap<&str, usize> = tilings
            .iter()
            .enumerate()
            .map(|(i, t)| (t.key.as_str(), i))
            .collect();
        let mut coverage = HashMap::new();
        for (t, f, k, n) in std::mem::take(&mut saved.coverage) {
            if let Some(&i) = index.get(t.as_str()) {
                *coverage.entry((i, f, k)).or_insert(0) += n;
            }
        }
        // A parent from another job has run where it was found.
        let mut transplanted: HashMap<String, HashSet<String>> = HashMap::new();
        for p in &set.parents {
            transplanted
                .entry(p.rule.clone())
                .or_default()
                .insert(p.tiling.clone());
        }
        let rng = |k: &str, x: u64| {
            Rng::new(
                saved
                    .meta
                    .get(k)
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(set.seed ^ x),
            )
        };
        Ok(Search {
            rng_gpu: Mutex::new(rng("rng_gpu", 0xABCD)),
            rng_cpu: Mutex::new(rng("rng_cpu", 0x1234)),
            screened: AtomicU64::new(saved.meta_u64("screened")),
            confirmed: AtomicU64::new(saved.meta_u64("confirmed")),
            next_id: AtomicI64::new(saved.next_id.max(1)),
            set,
            tiling_total,
            tilings,
            grids: Mutex::new(GridCache::new(GRID_CACHE_CELLS)),
            archive: Mutex::new(saved.archive),
            seen: Mutex::new(Seen {
                base: saved.seen,
                ..Default::default()
            }),
            coverage: Mutex::new(coverage),
            family_totals: Mutex::new(saved.families),
            named: Mutex::new(named),
            pending: Mutex::new(Pending::default()),
            gpu_rounds: AtomicU64::new(0),
            gpu_due: AtomicU64::new(0),
            cpu_rounds: AtomicU64::new(0),
            panics: AtomicU64::new(0),
            gpu_pace: Mutex::new(GpuPace::default()),
            cpu_caps: (
                std::time::Duration::from_secs_f64(CPU_RUN_S),
                std::time::Duration::from_secs_f64(CPU_CONFIRM_RUN_S),
            ),
            gpu_confirm_s: GPU_CONFIRM_S,
            capped: AtomicU64::new(0),
            given_up: AtomicU64::new(0),
            transplanted: Mutex::new(transplanted),
            degrees: Mutex::new(HashMap::new()),
            parent_turn: AtomicU64::new(0),
        })
    }

    pub fn settings(&self) -> &Settings {
        &self.set
    }
    /// Is there anything to run, with a GPU or without one? (Larger than Life runs on the GPU
    /// only.)
    pub fn can_run(&self, gpu: bool) -> Result<(), String> {
        let (g, c, ltl) = families(&self.set.families, &self.set.skip_families, gpu);
        if c.is_empty() && !(gpu && (ltl || !g.is_empty())) {
            return Err(if gpu {
                "no rule family to search".into()
            } else {
                "these families run on the GPU only (Larger than Life): use the GPU".into()
            });
        }
        Ok(())
    }
    /// Runs screened, in all.
    pub fn screened(&self) -> u64 {
        self.screened.load(Ordering::Relaxed)
    }
    /// Confirmation runs, in all.
    pub fn confirmed(&self) -> u64 {
        self.confirmed.load(Ordering::Relaxed)
    }
    /// Elites, and the niches they are in.
    pub fn elites(&self) -> (u64, u64) {
        let a = self.archive.lock().unwrap();
        (a.runs.len() as u64, a.cells.len() as u64)
    }
    /// Elites confirmed on bigger grids.
    pub fn confirmed_elites(&self) -> u64 {
        let a = self.archive.lock().unwrap();
        a.runs.keys().filter(|id| a.confirmed.contains(id)).count() as u64
    }
    /// Identities tried.
    pub fn seen(&self) -> u64 {
        self.seen.lock().unwrap().len() as u64
    }
    /// Distinct tilings with screened runs (of the search's list).
    pub fn tilings_tried(&self) -> u64 {
        let c = self.coverage.lock().unwrap();
        c.keys().map(|k| k.0).collect::<HashSet<_>>().len() as u64
    }
    /// The tilings the search draws from.
    pub fn tiling_count(&self) -> usize {
        self.tilings.len()
    }
    /// Candidates whose run panicked (skipped).
    pub fn panics(&self) -> u64 {
        self.panics.load(Ordering::Relaxed)
    }

    /// Runs on the processor cut short by their time cap ([`CPU_RUN_S`], [`CPU_CONFIRM_RUN_S`])
    /// in this session, kept as runs of the generations they reached.
    pub fn capped(&self) -> u64 {
        self.capped.load(Ordering::Relaxed)
    }

    /// Runs on the processor given up at a stop or the run's end in this session (not recorded).
    pub fn given_up(&self) -> u64 {
        self.given_up.load(Ordering::Relaxed)
    }
    /// Per family: screened runs, mean and best score, elites; most runs first.
    pub fn families(&self) -> Vec<FamilyLine> {
        let mut elites: HashMap<Fam, u64> = HashMap::new();
        for e in self.archive.lock().unwrap().runs.values() {
            *elites.entry(e.cand.fam).or_insert(0) += 1;
        }
        let mut v: Vec<FamilyLine> = self
            .family_totals
            .lock()
            .unwrap()
            .iter()
            .map(|(f, s)| FamilyLine {
                family: f.name().into(),
                runs: s.runs,
                mean: s.mean(),
                best: s.best,
                elites: elites.get(f).copied().unwrap_or(0),
            })
            .collect();
        v.sort_by(|a, b| b.runs.cmp(&a.runs).then(a.family.cmp(&b.family)));
        v
    }

    /// What was measured and changed since the last call: the runs, the identities tried, the
    /// coverage and family totals added, the elites added and dropped, the elites confirmed, and
    /// the counters and random states (`meta`).
    ///
    /// It may be taken while a step runs (a job writes one every couple of seconds): the runs,
    /// the elites they changed, the elites confirmed and the counters are taken under the lock
    /// that publishing holds, so they always agree.
    pub fn take_delta(&self) -> Delta {
        let mut pending = self.pending.lock().unwrap();
        let p = std::mem::take(&mut *pending);
        let elites = self.archive.lock().unwrap().take_changes();
        let (screened, confirmed) = (self.screened(), self.confirmed());
        drop(pending);
        let seen = std::mem::take(&mut self.seen.lock().unwrap().new);
        let meta = vec![
            ("screened".to_string(), screened.to_string()),
            ("confirmed".to_string(), confirmed.to_string()),
            (
                "rng_gpu".to_string(),
                self.rng_gpu.lock().unwrap().state.to_string(),
            ),
            (
                "rng_cpu".to_string(),
                self.rng_cpu.lock().unwrap().state.to_string(),
            ),
        ];
        Delta {
            rows: p.rows,
            seen,
            coverage: p.coverage,
            families: p.families,
            elites,
            confirmed: p.confirmed,
            meta,
        }
    }

    /// Take in runs a search already made (rebuilding a working database from its sealed
    /// segments, as an imported search's): their identities, and for the screened ones coverage,
    /// family totals and the archive, offered in id order; the confirmation runs mark their
    /// elites confirmed. Nothing is run, and the runs are not written again.
    pub fn take_in(&self, rows: &[TakenRun]) {
        let mut a = self.archive.lock().unwrap();
        let mut seen = self.seen.lock().unwrap();
        let mut cov = self.coverage.lock().unwrap();
        let mut fams = self.family_totals.lock().unwrap();
        let mut p = self.pending.lock().unwrap();
        for r in rows {
            self.next_id.fetch_max(r.id + 1, Ordering::Relaxed);
            let Some(ev) = &r.ev else { continue };
            seen.insert(ev.cand.id_hash());
            if r.stage != 1 {
                self.confirmed.fetch_add(1, Ordering::Relaxed);
                if let Some(parent) = r.parent
                    && a.confirmed.insert(parent)
                {
                    p.confirmed.push(parent);
                }
                continue;
            }
            self.screened.fetch_add(1, Ordering::Relaxed);
            let c = &ev.cand;
            if let Some(i) = self.tiling_index(&c.tiling) {
                *cov.entry((i, c.fam, r.seed_kind.clone())).or_insert(0) += 1;
            }
            *p.coverage
                .entry((c.tiling.clone(), c.fam, r.seed_kind.clone()))
                .or_insert(0) += 1;
            fams.entry(c.fam).or_default().add(ev.score as f64);
            p.families.entry(c.fam).or_default().add(ev.score as f64);
            a.offer(r.id, ev.clone(), self.group(&c.tiling));
        }
    }

    /// The group (0-7) of a tiling, for the archive's niches.
    pub fn group(&self, key: &str) -> u8 {
        group_of(&self.tilings, key)
    }

    fn tiling_index(&self, key: &str) -> Option<usize> {
        self.tilings.iter().position(|t| t.key == key)
    }

    fn grid(&self, key: &str, cells: u32) -> Result<Arc<Grid>, String> {
        let k = (key.to_string(), cells);
        if let Some(g) = self.grids.lock().unwrap().get(&k) {
            return Ok(g);
        }
        // Built outside the lock, so other rounds go on meanwhile.
        let g = Arc::new(Grid::build(key, cells)?);
        Ok(self.grids.lock().unwrap().insert(k, g))
    }

    /// A screening grid of this tiling, sized by the search's sizing
    /// ([`space::search_cells`]).
    fn screen_grid(&self, key: &str) -> Result<Arc<Grid>, String> {
        self.grid(
            key,
            space::search_cells(key, self.set.screen_cells, self.set.sizing),
        )
    }

    /// A confirming grid of this tiling ([`space::confirm_cells`]).
    fn confirm_grid(&self, key: &str) -> Result<Arc<Grid>, String> {
        let s = &self.set;
        self.grid(
            key,
            space::confirm_cells(key, s.screen_cells, s.confirm_cells, s.sizing),
        )
    }

    /// Let go of the grids no run is using, down to half the grid cache's limit: what a search
    /// gives back when its memory is over its cap. A grid let go is built again when a run needs
    /// it (the same grid: nothing a run measures changes).
    pub fn shed_grids(&self) {
        let mut c = self.grids.lock().unwrap();
        let half = c.limit / 2;
        c.shed(half);
    }

    /// The grid cache's size and how it has done ([`GRID_CACHE_CELLS`]).
    pub fn grid_cache(&self) -> GridCacheStats {
        let c = self.grids.lock().unwrap();
        GridCacheStats {
            grids: c.map.len(),
            cells: c.cells,
            hits: c.hits,
            builds: c.builds,
        }
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

    fn next_id(&self) -> i64 {
        self.next_id.fetch_add(1, Ordering::Relaxed)
    }

    fn publish(&self, rows: Vec<RunOut>, stage: u8) {
        let mut p = self.pending.lock().unwrap();
        if stage == 1 {
            self.screened
                .fetch_add(rows.len() as u64, Ordering::Relaxed);
            let mut a = self.archive.lock().unwrap();
            let mut cov = self.coverage.lock().unwrap();
            let mut fams = self.family_totals.lock().unwrap();
            for r in &rows {
                let c = &r.ev.cand;
                if let Some(i) = self.tiling_index(&c.tiling) {
                    *cov.entry((i, c.fam, r.seed_kind.clone())).or_insert(0) += 1;
                }
                *p.coverage
                    .entry((c.tiling.clone(), c.fam, r.seed_kind.clone()))
                    .or_insert(0) += 1;
                fams.entry(c.fam).or_default().add(r.ev.score as f64);
                p.families.entry(c.fam).or_default().add(r.ev.score as f64);
                a.offer(r.id, r.ev.clone(), self.group(&c.tiling));
            }
        } else {
            self.confirmed
                .fetch_add(rows.len() as u64, Ordering::Relaxed);
        }
        p.rows.extend(rows);
    }

    /// Publish confirmation runs, ids given in order, and mark their elites confirmed, at once
    /// (a delta taken meanwhile has both or neither).
    fn publish_confirmed(&self, ids: &[i64], mut rows: Vec<RunOut>) {
        for r in &mut rows {
            r.id = self.next_id();
        }
        let mut p = self.pending.lock().unwrap();
        let mut a = self.archive.lock().unwrap();
        for id in ids {
            if a.confirmed.insert(*id) {
                p.confirmed.push(*id);
            }
        }
        drop(a);
        self.confirmed
            .fetch_add(rows.len() as u64, Ordering::Relaxed);
        p.rows.extend(rows);
    }

    /// Publish screened runs, given ids in their order.
    fn publish_screened(&self, mut rows: Vec<RunOut>) {
        for r in &mut rows {
            r.id = self.next_id();
        }
        self.publish(rows, 1);
    }

    /// Publish a screened run of a round once it has ended (`None`: it was not measured), with
    /// every run after it that has ended too, in the order they were made, each given its id.
    fn release(&self, r: &Mutex<Release>, i: usize, out: Option<RunOut>) {
        let mut r = r.lock().unwrap();
        r.ended[i] = Some(out);
        let mut batch = Vec::new();
        while r.next < r.ended.len() {
            let n = r.next;
            let Some(out) = r.ended[n].take() else { break };
            r.next += 1;
            if let Some(mut row) = out {
                row.id = self.next_id();
                batch.push(row);
            }
        }
        // Still under the release lock, so rounds' runs reach the archive in their order.
        if !batch.is_empty() {
            self.publish(batch, 1);
        }
    }

    /// One step: GPU rounds (with `gpu`: the backend, and the pool that seeds its worlds and
    /// measures their runs) on a thread of their own and CPU rounds on `pool`, until the
    /// deadline, a stop, or the screened total. The CPU stops handing out candidates then (the
    /// ones running finish, each within its time cap; an elite is confirmed with all its seeds or
    /// not at all), and the GPU goes on with batches while the CPU finishes, so it is never idle
    /// waiting for a slow CPU round; a batch under way runs to its end. At a stop or the run's
    /// end ([`StepLimits::end`]) the runs still going on the processor are given up, no GPU
    /// batch is started that would not end before it, and one under way is given up at its next
    /// submission (its pace misjudged). Returns the GPU's error, when a GPU round failed (the GPU
    /// stops for the rest of the step).
    ///
    /// The two pools are the step's only threads at work (with the GPU thread, which waits on
    /// them and on the device), so their sizes bound the processor time the search takes.
    pub fn step(
        &self,
        pool: &rayon::ThreadPool,
        gpu: Option<(&mut dyn Backend, &rayon::ThreadPool)>,
        lim: &StepLimits<'_>,
    ) -> Option<String> {
        let go = || {
            !lim.stop.load(Ordering::SeqCst)
                && Instant::now() < lim.until
                && lim.max_screened.is_none_or(|m| self.screened() < m)
        };
        let has_gpu = gpu.is_some();
        let (gpu_fams, cpu_fams, ltl) =
            families(&self.set.families, &self.set.skip_families, has_gpu);
        let cpu_busy = AtomicBool::new(false);
        let gpu_go = || {
            !lim.cut()
                && (Instant::now() < lim.until || cpu_busy.load(Ordering::SeqCst))
                && lim.max_screened.is_none_or(|m| self.screened() < m)
        };
        std::thread::scope(|sc| {
            let gpu_thread = gpu.map(|(be, gpu_pool)| {
                let gpu_fams = gpu_fams.clone();
                // The GPU's rounds run on its own pool, so preparing its worlds and measuring
                // their runs (the processor's share of a batch) stays on that pool's threads.
                sc.spawn(move || {
                    gpu_pool.install(move || {
                        if gpu_fams.is_empty() && !ltl {
                            return None;
                        }
                        let mut rng = *self.rng_gpu.lock().unwrap();
                        let keep = |rng: Rng| *self.rng_gpu.lock().unwrap() = rng;
                        while gpu_go() {
                            let round = self.gpu_rounds.fetch_add(1, Ordering::Relaxed) + 1;
                            // Every eighth round owes GPU_CONFIRMS elites a confirmation, paid in
                            // batches of a few seconds every other round, between screening
                            // batches, so the GPU's screening never waits long (#83).
                            // (Only the GPU's thread reads or writes it.)
                            if round.is_multiple_of(8) {
                                let d = self.gpu_due.load(Ordering::Relaxed);
                                self.gpu_due.store(
                                    (d + GPU_CONFIRMS).min(2 * GPU_CONFIRMS),
                                    Ordering::Relaxed,
                                );
                            }
                            let due = self.gpu_due.load(Ordering::Relaxed);
                            let r = catch_unwind(AssertUnwindSafe(|| {
                                if due > 0 && round.is_multiple_of(2) {
                                    self.gpu_confirm(be, due as usize, lim)
                                } else {
                                    self.gpu_round(be, &mut rng, &gpu_fams, ltl, lim)
                                }
                            }));
                            match r {
                                Ok(Ok(true)) => {}
                                // No batch ends before the run's end, or one was given up.
                                Ok(Ok(false)) => break,
                                Ok(Err(e)) => {
                                    keep(rng);
                                    return Some(e);
                                }
                                Err(_) => {
                                    keep(rng);
                                    self.panics.fetch_add(1, Ordering::Relaxed);
                                    return Some("a GPU round panicked".into());
                                }
                            }
                        }
                        keep(rng);
                        None
                    })
                })
            });
            if !cpu_fams.is_empty() {
                cpu_busy.store(true, Ordering::SeqCst);
                let mut rng = *self.rng_cpu.lock().unwrap();
                pool.install(|| {
                    while go() {
                        self.cpu_round(&mut rng, &cpu_fams, lim);
                    }
                });
                *self.rng_cpu.lock().unwrap() = rng;
                cpu_busy.store(false, Ordering::SeqCst);
            }
            gpu_thread.and_then(|t| t.join().unwrap_or(Some("the GPU thread panicked".into())))
        })
    }

    /// Make candidates for a grid. `ltl` fixes a Larger than Life group: only those are made.
    #[allow(clippy::too_many_arguments)]
    fn emit(
        &self,
        grid: &Grid,
        nb: Nb,
        fams: &[(Fam, f64)],
        ltl: Option<(Hood, u8)>,
        n: usize,
        gens: u32,
        rng: &mut Rng,
        backend: Option<&dyn Fn(&Rule) -> bool>,
    ) -> Vec<(Cand, String, Lineage)> {
        let mut out: Vec<(Cand, String, Lineage)> = Vec::with_capacity(n);
        let Some(ti) = self.tiling_index(&grid.key) else {
            return out;
        };
        let mut tries = 0;
        // A transplant's scaled variant, made with it and tried next.
        let mut extra: Option<Made> = None;
        while out.len() < n && tries < n * 12 {
            tries += 1;
            let (rule, kind, seed, origin, from): Made = if let Some(x) = extra.take() {
                x
            } else {
                let fam = if ltl.is_some() {
                    Fam::Ltl
                } else {
                    weighted(rng, fams)
                };
                if !fam.fits(grid) {
                    continue;
                }
                let roll = rng.next_f64();
                // Parents: in proportion to quality, and for JACA only runs whose signs both held.
                let parent_ok = |e: &Eval| fam != Fam::Jaca || breed::strong(e);
                match way(fam, roll) {
                    Way::Random => {
                        let r = match ltl {
                            Some(g) => sample_ltl(grid, rng, Some(g)),
                            None => sample_rule(fam, grid, nb, rng),
                        };
                        let (sd, k) = seed_for(fam, grid, rng);
                        (r, k.to_string(), sd, "random", None)
                    }
                    Way::Mutate => {
                        let el = self
                            .archive
                            .lock()
                            .unwrap()
                            .sample_parent(rng, Some(fam), &parent_ok)
                            .map(|(id, e)| (id, e.clone()));
                        let Some((id, el)) = el else { continue };
                        let Some(base) = el.cand.rule() else { continue };
                        if let Some(g) = ltl
                            && !matches!(&base, Rule::Ltl(l) if (l.hood, l.range) == g)
                        {
                            continue;
                        }
                        let r = space::mutate_rule(&base, grid, nb, rng);
                        let seed = el.cand.seed.clone();
                        (r, "mutated".to_string(), seed, "mutate", Some((id, None)))
                    }
                    Way::Seed => {
                        let el = self
                            .archive
                            .lock()
                            .unwrap()
                            .sample_parent(rng, Some(fam), &parent_ok)
                            .map(|(id, e)| (id, e.clone()));
                        let Some((id, el)) = el else { continue };
                        let Some(base) = el.cand.rule() else { continue };
                        if let Some(g) = ltl
                            && !matches!(&base, Rule::Ltl(l) if (l.hood, l.range) == g)
                        {
                            continue;
                        }
                        let (sd, k) = seed_for(fam, grid, rng);
                        (Some(base), k.to_string(), sd, "seed", Some((id, None)))
                    }
                    Way::Transplant => {
                        let Some((made, scaled)) = self.transplant(grid, nb, rng) else {
                            continue;
                        };
                        extra = scaled;
                        made
                    }
                    Way::Balance => {
                        let Some(made) = self.balance(grid, nb, rng) else {
                            continue;
                        };
                        made
                    }
                    Way::Cover => {
                        // The least-tried seed kind of this family on this grid.
                        let mut best: Option<(u32, jetcarp_core::world::SeedSpec, &'static str)> =
                            None;
                        let cov = self.coverage.lock().unwrap();
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
                        (r, k.to_string(), sd, "cover", None)
                    }
                    Way::Named => {
                        // A named rule on this grid.
                        let named = {
                            let mut q = self.named.lock().unwrap();
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
                            self.named.lock().unwrap().push(named);
                            continue;
                        }
                        if !f2.fits(grid) {
                            continue;
                        }
                        let (sd, k) = seed_for(f2, grid, rng);
                        (Some(r), k.to_string(), sd, "named", None)
                    }
                }
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
            if !self.seen.lock().unwrap().insert(cand.id_hash()) {
                continue;
            }
            out.push((cand, kind, from));
        }
        out
    }

    /// A grid's mean and largest neighbour counts for a neighbourhood (from the grid cache, or
    /// built and remembered).
    fn degree(&self, key: &str, nb: Nb) -> Option<(f64, usize)> {
        let k = (key.to_string(), nb as u8);
        if let Some(d) = self.degrees.lock().unwrap().get(&k) {
            return Some(*d);
        }
        let g = self.screen_grid(key).ok()?;
        let d = (g.mean_deg[nb as usize], g.max_deg[nb as usize]);
        self.degrees.lock().unwrap().insert(k, d);
        Some(d)
    }

    /// Has this JACA pair run on this tiling (as a transplant, or where it was found)?
    fn tried_on(&self, rule: &str, home: &str, key: &str) -> bool {
        home == key
            || self
                .transplanted
                .lock()
                .unwrap()
                .get(rule)
                .is_some_and(|t| t.contains(key))
    }

    /// Up to `n` parents for a transplant or a balance variant: rules another job found
    /// (`--parents-from`, taken in turn, three picks in four while there are any), else elites of
    /// the archive that `ok` accepts. Each is (rule, its tiling, its neighbourhood, its seed, its
    /// weaker sign when it was losing).
    fn picks(
        &self,
        rng: &mut Rng,
        n: usize,
        from_queue: &dyn Fn(&Parent) -> bool,
        ok: &dyn Fn(&Eval) -> bool,
    ) -> Vec<Pick> {
        let mut out = Vec::new();
        let queued: Vec<&Parent> = self.set.parents.iter().filter(|p| from_queue(p)).collect();
        if !queued.is_empty() && rng.below(4) != 0 {
            let start = self.parent_turn.fetch_add(n as u64, Ordering::Relaxed) as usize;
            for i in 0..n.min(queued.len()) {
                let p = queued[(start + i) % queued.len()];
                let Ok(seed) = serde_json::from_str(&p.seed) else {
                    continue;
                };
                out.push(Pick {
                    rule: p.rule.clone(),
                    tiling: p.tiling.clone(),
                    nb: if p.nb == 0 { Nb::Touch } else { Nb::Edge },
                    seed,
                    weaker: p.weaker,
                    from: (p.run, Some((p.job, p.search.clone()))),
                });
            }
            return out;
        }
        let a = self.archive.lock().unwrap();
        for _ in 0..n {
            if let Some((id, e)) = a.sample_parent(rng, Some(Fam::Jaca), ok) {
                out.push(Pick {
                    rule: e.cand.rule.clone(),
                    tiling: e.cand.tiling.clone(),
                    nb: e.cand.nb,
                    seed: e.cand.seed.clone(),
                    weaker: breed::losing(&e.feat),
                    from: (id, None),
                });
            }
        }
        out
    }

    /// A transplant: a strong JACA pair, unchanged, on this grid where it has not run yet,
    /// preferring pairs found on grids of a similar neighbour count. With it, when the counts
    /// differ, its variant with every count scaled to this grid's (tried next).
    fn transplant(&self, grid: &Grid, nb: Nb, rng: &mut Rng) -> Option<(Made, Option<Made>)> {
        let picks = self.picks(rng, 6, &|p| p.strong, &breed::strong);
        let (d_new, max_new) = (grid.mean_deg[nb as usize], grid.max_deg[nb as usize]);
        let mut best: Option<(f64, Pick, f64)> = None;
        for p in picks {
            if self.tried_on(&p.rule, &p.tiling, &grid.key) {
                continue;
            }
            let Some((d_old, _)) = self.degree(&p.tiling, p.nb) else {
                continue;
            };
            // Near degrees first; now and then one farther afield.
            let far = (d_old - d_new).abs() / d_new.max(1.0) + 0.3 * rng.next_f64();
            if best.as_ref().is_none_or(|b| far < b.0) {
                best = Some((far, p, d_old));
            }
        }
        let (_, p, d_old) = best?;
        {
            let mut t = self.transplanted.lock().unwrap();
            let tried = t.entry(p.rule.clone()).or_default();
            tried.insert(p.tiling.clone());
            tried.insert(grid.key.clone());
        }
        let Ok((rule, _)) = Rule::parse(&p.rule) else {
            return None;
        };
        let scaled = match &rule {
            Rule::Jaca(j) if (d_old - d_new).abs() >= 0.5 => {
                let s = breed::scaled(j, d_old, d_new, max_new);
                (s != **j).then(|| {
                    (
                        Some(Rule::Jaca(Box::new(s))),
                        "transplant".to_string(),
                        p.seed.clone(),
                        "transplant-scaled",
                        Some(p.from.clone()),
                    )
                })
            }
            _ => None,
        };
        Some((
            (
                Some(rule),
                "transplant".to_string(),
                p.seed,
                "transplant",
                Some(p.from),
            ),
            scaled,
        ))
    }

    /// A balance variant: a JACA pair whose weaker sign was losing, changed to help that sign
    /// ([`breed::balance_variant`]).
    fn balance(&self, grid: &Grid, nb: Nb, rng: &mut Rng) -> Option<Made> {
        let picks = self.picks(rng, 1, &|p| p.weaker.is_some(), &|e: &Eval| {
            breed::losing(&e.feat).is_some()
        });
        let p = picks.into_iter().next()?;
        let weaker = p.weaker?;
        let Ok((Rule::Jaca(j), _)) = Rule::parse(&p.rule) else {
            return None;
        };
        let (r, seed) = breed::balance_variant(&j, &p.seed, weaker, grid.degree(nb), rng)?;
        Some((
            Some(Rule::Jaca(Box::new(r))),
            "balance".to_string(),
            seed,
            "balance",
            Some(p.from),
        ))
    }

    /// Run one GPU batch of candidates on a grid and publish its runs, in the order they were
    /// made: screening runs, or with `confirms` the confirmation runs of these elites (marked
    /// confirmed with them). `reach` is a Larger than Life batch's reach (cells read per cell),
    /// for the GPU's pace. At a stop or the run's end the batch is given up between two of its
    /// submissions, its runs not recorded: then false.
    #[allow(clippy::too_many_arguments)]
    fn gpu_batch(
        &self,
        be: &mut dyn Backend,
        grid: &Grid,
        nb: Nb,
        cands: Vec<(Cand, String, Lineage)>,
        confirms: Option<&[i64]>,
        reach: Option<usize>,
        lim: &StepLimits<'_>,
    ) -> Result<bool, String> {
        let stage = if confirms.is_some() { 2 } else { 1 };
        if cands.is_empty() {
            return Ok(true);
        }
        let gens = cands[0].0.gens;
        let trace = std::env::var_os("JETCARP_EXPLORE_TRACE").is_some();
        let t0 = Instant::now();
        let inits: Vec<Option<BatchWorld>> = cands
            .par_iter()
            .map(|(c, ..)| {
                catch_unwind(AssertUnwindSafe(|| {
                    let world = prepare(grid, c).ok()?;
                    let rule = c.rule()?;
                    Some(BatchWorld {
                        rule,
                        init: jetcarp_core::explore::batch::field_bytes(world.fields.first()?),
                    })
                }))
                .unwrap_or_else(|_| {
                    self.panics.fetch_add(1, Ordering::Relaxed);
                    None
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
            return Ok(true);
        }
        // A JACA run also needs its frame at the three-quarter mark.
        let frames_at = if good.iter().any(|(c, ..)| c.fam == Fam::Jaca) {
            frames_with_trade(gens, WINDOW)
        } else {
            frames_for(gens, WINDOW)
        };
        let job = BatchJob {
            topo: grid.topo.clone(),
            nb,
            worlds,
            gens,
            frames_at,
        };
        let t1 = Instant::now();
        let traces = match be.run_until(&job, &|| lim.cut()) {
            Err(e) if e == GIVEN_UP => {
                self.given_up
                    .fetch_add(good.len() as u64, Ordering::Relaxed);
                if trace {
                    eprintln!(
                        "batch stage {stage} {} worlds={} cells={} gens={gens}: given up after {:.2}s on the GPU",
                        grid.key,
                        good.len(),
                        grid.topo.len(),
                        t1.elapsed().as_secs_f64()
                    );
                }
                return Ok(false);
            }
            r => r?,
        };
        if traces.len() != good.len() {
            return Err(format!(
                "the GPU returned {} traces for {} worlds",
                traces.len(),
                good.len()
            ));
        }
        let t2 = Instant::now();
        let rows: Vec<RunOut> = good
            .into_par_iter()
            .zip(traces.into_par_iter())
            .filter_map(|((c, kind, from), t)| {
                let (fate, feat, score, bin) =
                    catch_unwind(AssertUnwindSafe(|| self.evaluate(grid, &c, &t)))
                        .map_err(|_| self.panics.fetch_add(1, Ordering::Relaxed))
                        .ok()?;
                let (parent, parent_job, parent_search) = lineage_cols(from);
                Some(RunOut {
                    id: 0,
                    stage,
                    parent,
                    parent_job,
                    parent_search,
                    ev: Eval {
                        cand: c,
                        fate,
                        feat,
                        score,
                        bin,
                    },
                    seed_kind: kind,
                })
            })
            .collect();
        let t3 = Instant::now();
        let n = rows.len();
        let work = n as f64 * grid.topo.len() as f64 * gens as f64 * reach.unwrap_or(1) as f64;
        self.gpu_pace.lock().unwrap().note(
            reach.is_some(),
            grid.topo.lattice().is_some(),
            work,
            (t3 - t0).as_secs_f64(),
        );
        match confirms {
            Some(ids) => self.publish_confirmed(ids, rows),
            None => self.publish_screened(rows),
        }
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
        Ok(true)
    }

    /// A GPU round's kind (a Larger than Life round, or a round of `fams`), tiling and families.
    /// Some rounds go to the least-tried tiling, so every tiling is reached by the deadline. The
    /// families tied to one grid ([`Fam::only_on`]: isotropic rules, when that grid is in the
    /// list) have rounds of their own on it, the least-tried of them each time, as many as their
    /// weights' share of the families' (every round when there are no others), and the other
    /// rounds draw from the rest, so no draw is spent on a tiling they cannot run on (#245). A
    /// search without them draws exactly as before.
    fn gpu_plan(
        &self,
        rng: &mut Rng,
        fams: &[(Fam, f64)],
        ltl_ok: bool,
    ) -> (bool, usize, Vec<(Fam, f64)>) {
        let ltl_round = ltl_ok && (fams.is_empty() || rng.below(8) == 0);
        let (tied, rest): (Vec<_>, Vec<_>) = fams
            .iter()
            .copied()
            .partition(|(f, _)| f.only_on().is_some_and(|k| self.tiling_index(k).is_some()));
        if !ltl_round && !tied.is_empty() {
            let weight = |v: &[(Fam, f64)]| v.iter().map(|x| x.1).sum::<f64>();
            if rest.is_empty() || rng.next_f64() * weight(fams) < weight(&tied) {
                let native: Vec<Fam> = tied.iter().map(|x| x.0).collect();
                let (ti, f) = self.native_round(&native);
                return (false, ti, f);
            }
        }
        let ti = if ltl_round && rng.below(3) == 0 {
            self.least_tried_tiling(rng, &[(Fam::Ltl, 1.0)])
        } else if !ltl_round && rng.below(8) == 0 {
            self.least_tried_tiling(rng, &rest)
        } else {
            self.pick_tiling(rng)
        };
        (ltl_round, ti, rest)
    }

    /// One screening batch on the GPU, sized to end before the run's end. False when not even a
    /// small batch would, or the batch was given up (a stop, or the end reached).
    fn gpu_round(
        &self,
        be: &mut dyn Backend,
        rng: &mut Rng,
        fams: &[(Fam, f64)],
        ltl_ok: bool,
        lim: &StepLimits<'_>,
    ) -> Result<bool, String> {
        let (ltl_round, ti, fams) = self.gpu_plan(rng, fams, ltl_ok);
        let key = self.tilings[ti].key.clone();
        let grid = self.screen_grid(&key)?;
        let nb = if grid.has_edge && rng.below(4) == 0 {
            Nb::Edge
        } else {
            Nb::Touch
        };
        let mut max_worlds = (be.max_cells() / grid.topo.len().next_multiple_of(256))
            .min(self.set.batch)
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
            let work =
                (grid.topo.len() * self.set.screen_gens as usize).max(1) as f64 * reach as f64;
            let cap = (LTL_NEIGHBOUR_UPDATES / work) as usize;
            max_worlds = max_worlds.min(cap.max(16));
            Some((hood, r, reach))
        } else {
            None
        };
        let reach = ltl.map(|l| l.2);
        let ltl = ltl.map(|l| (l.0, l.1));
        let world_work =
            grid.topo.len() as f64 * self.set.screen_gens as f64 * reach.unwrap_or(1) as f64;
        let pace = self
            .gpu_pace
            .lock()
            .unwrap()
            .get(reach.is_some(), grid.topo.lattice().is_some());
        max_worlds = fit_worlds(pace, world_work, secs_left(lim.end), max_worlds);
        if max_worlds == 0 {
            return Ok(false);
        }
        let filter = |r: &Rule| be.supports(r);
        let cands = self.emit(
            &grid,
            nb,
            &fams,
            ltl,
            max_worlds,
            self.set.screen_gens,
            rng,
            Some(&filter),
        );
        self.gpu_batch(be, &grid, nb, cands, None, reach, lim)
    }

    /// Confirm the best runs of one grid/neighbourhood group on the GPU, up to `limit`, as many
    /// as end before the run's end and fit in about `gpu_confirm_s` of the GPU's time, and take
    /// them off what the GPU owes (all of it when none is waiting). False when not even one would
    /// end in time, or the batch was given up (its elites stay unconfirmed).
    fn gpu_confirm(
        &self,
        be: &mut dyn Backend,
        limit: usize,
        lim: &StepLimits<'_>,
    ) -> Result<bool, String> {
        let pending = self.archive.lock().unwrap().unconfirmed(4096);
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
        let Some((key, nb, ltl)) = first else {
            self.gpu_due.store(0, Ordering::Relaxed);
            return Ok(true);
        };
        let grid = self.confirm_grid(&key)?;
        let reach = ltl.map(|(hood, r)| grid.reach_size(hood, r).max(1));
        let seeds = self.set.confirm_seeds.max(1) as usize;
        let gens = picked.first().map_or(self.set.confirm_gens, |(_, e)| {
            run_len(e.cand.fam, &grid, self.set.confirm_gens).0
        });
        let world_work = grid.topo.len() as f64 * gens as f64 * reach.unwrap_or(1) as f64;
        let pace = self
            .gpu_pace
            .lock()
            .unwrap()
            .get(reach.is_some(), grid.topo.lattice().is_some());
        // As many as end before the run's end, and at most about `gpu_confirm_s` of the GPU's
        // time (an elite's seeds at least), so screening on the GPU never waits long (#83).
        let most = picked.len() * seeds;
        let by_end = fit_worlds(pace, world_work, secs_left(lim.end), most);
        // With no pace yet for this kind of batch (Larger than Life on a patch, say: forty times
        // slower a cell), one elite, to time it: twelve then (36 worlds) ran 23 s.
        let by_time = match pace {
            Some(_) => fit_worlds(pace, world_work, Some(self.gpu_confirm_s / GPU_FILL), most),
            None => seeds,
        };
        picked.truncate(by_end.min(by_time.max(seeds)) / seeds);
        if picked.is_empty() {
            return Ok(false);
        }
        let n = picked.len() as u64;
        let d = self.gpu_due.load(Ordering::Relaxed);
        self.gpu_due.store(d.saturating_sub(n), Ordering::Relaxed);
        let mut cands = Vec::new();
        for (id, e) in &picked {
            for c in self.confirmations(e, &grid) {
                cands.push((c, "confirm".to_string(), Some((*id, None))));
            }
        }
        let ids: Vec<i64> = picked.iter().map(|p| p.0).collect();
        self.gpu_batch(be, &grid, nb, cands, Some(&ids), reach, lim)
    }

    /// The confirmation runs of an elite on a bigger grid: its candidate with other seeds (each
    /// counted as tried).
    fn confirmations(&self, e: &Eval, grid: &Grid) -> Vec<Cand> {
        let cands: Vec<Cand> = (0..self.set.confirm_seeds)
            .map(|sd| {
                let mut c = e.cand.clone();
                let (g2, st) = run_len(c.fam, grid, self.set.confirm_gens);
                c.cells = grid.cells_target;
                c.gens = g2;
                c.steps = st;
                c.rng = e.cand.rng.wrapping_add(sd as u64 + 1);
                c.origin = "confirm".into();
                c
            })
            .collect();
        let mut seen = self.seen.lock().unwrap();
        for c in &cands {
            seen.insert(c.id_hash());
        }
        cands
    }

    /// A CPU round's tiling and families, from `fams` (the processor's). CPU rounds are few, so
    /// weighted picks alone would miss tilings: a quarter go to the least-tried family tied to
    /// one grid ([`Fam::only_on`], when that grid is in the list), half sweep the least-tried
    /// tiling with the families that run on any. Under schedule 2 ([`SCHEDULE`]), when every
    /// family here is tied to one grid (isotropic rules alone, with no GPU), every round goes to
    /// one of them, rather than three in four to a tiling they cannot run on.
    fn cpu_plan(&self, round: u64, rng: &mut Rng, fams: &[(Fam, f64)]) -> (usize, Vec<(Fam, f64)>) {
        let everywhere: Vec<(Fam, f64)> = fams
            .iter()
            .filter(|(f, _)| f.only_on().is_none())
            .map(|(f, _)| (*f, 1.0))
            .collect();
        let native: Vec<Fam> = fams
            .iter()
            .map(|(f, _)| *f)
            .filter(|f| f.only_on().is_some_and(|k| self.tiling_index(k).is_some()))
            .collect();
        let only_native = self.set.schedule >= 2 && everywhere.is_empty() && !native.is_empty();
        match round % 4 {
            0 if !native.is_empty() => self.native_round(&native),
            _ if only_native => self.native_round(&native),
            1 | 2 if !everywhere.is_empty() => (
                self.least_tried_tiling(rng, &everywhere),
                everywhere.clone(),
            ),
            _ => (self.pick_tiling(rng), fams.to_vec()),
        }
    }

    /// A round of the least-tried of these grid-specific families, on its grid.
    fn native_round(&self, native: &[Fam]) -> (usize, Vec<(Fam, f64)>) {
        let f = self.least_tried_fam(native);
        let key = f.only_on().expect("a grid-specific family");
        (self.tiling_index(key).unwrap_or(0), vec![(f, 1.0)])
    }

    /// One CPU round: a tiling and families chosen, candidates made, and run on the pool (the
    /// caller's) beside this round's share of elites confirmed (twice the threads every four
    /// rounds, spread over them). The confirmations start first, each elite's seeds side by side,
    /// so screening goes on beside them on the other threads and none waits for a whole batch of
    /// confirmations (#83). A round under way runs to its end, as [`StepLimits::until`] says: the
    /// step's deadline only keeps the next round from starting, and only a stop or the run's end
    /// ([`StepLimits::cut`]) gives up its candidates and its confirmations. (A round of runs on
    /// the processor nearly always outlasts the 2 s step, so checking the deadline here left most
    /// candidates unrun and never confirmed.)
    ///
    /// What a round makes does not depend on how its threads take up its runs: its screened runs
    /// reach the archive, with their ids, in the order they were made ([`Release`]), its
    /// confirmations at its end in the order their elites were picked, and every elite picked is
    /// confirmed (or, at a stop or the run's end, none).
    fn cpu_round(&self, rng: &mut Rng, fams: &[(Fam, f64)], lim: &StepLimits<'_>) {
        let round = self.cpu_rounds.fetch_add(1, Ordering::Relaxed) + 1;
        let threads = rayon::current_num_threads().max(1);
        let (ti, round_fams) = self.cpu_plan(round, rng, fams);
        let key = self.tilings[ti].key.clone();
        let Ok(grid) = self.screen_grid(&key) else {
            return;
        };
        let nb = if grid.has_edge && rng.below(4) == 0 {
            Nb::Edge
        } else {
            Nb::Touch
        };
        let cands = self.emit(
            &grid,
            nb,
            &round_fams,
            None,
            threads * 8,
            self.set.screen_gens,
            rng,
            None,
        );
        // This round's elites to confirm: twice the threads every four rounds, as many in each.
        let share = 2 * threads as u64;
        let n = (round * share / 4 - (round - 1) * share / 4) as usize;
        let picked: Vec<(i64, Eval)> = if n == 0 || lim.cut() {
            Vec::new()
        } else {
            self.archive
                .lock()
                .unwrap()
                .unconfirmed(4096)
                .into_iter()
                .filter(|(_, e)| fams.iter().any(|(f, _)| *f == e.cand.fam))
                .take(n)
                .collect()
        };
        let confirmed: Vec<Mutex<Option<Vec<RunOut>>>> =
            picked.iter().map(|_| Mutex::new(None)).collect();
        let release = Mutex::new(Release {
            next: 0,
            ended: (0..cands.len()).map(|_| None).collect(),
        });
        let grid = &grid;
        rayon::scope_fifo(|sc| {
            for ((id, e), out) in picked.iter().zip(&confirmed) {
                sc.spawn_fifo(move |_| *out.lock().unwrap() = self.confirm_elite(*id, e, lim));
            }
            for (i, (c, kind, from)) in cands.into_iter().enumerate() {
                let release = &release;
                sc.spawn_fifo(move |_| {
                    let out = if lim.cut() {
                        None
                    } else {
                        match self.cpu_eval(grid, c, kind, from, None, 1, lim) {
                            Ran::Done(r) => Some(*r),
                            Ran::Failed | Ran::GivenUp => None,
                        }
                    };
                    self.release(release, i, out);
                });
            }
        });
        for ((id, _), out) in picked.iter().zip(confirmed) {
            if let Some(rows) = out.into_inner().unwrap() {
                self.publish_confirmed(&[*id], rows);
            }
        }
    }

    /// Confirm an elite on the processor, on the confirming grid with every seed, the seeds side
    /// by side: its runs (none when one was given up at a stop or the run's end, so it is
    /// confirmed with all its seeds or not at all). A Lenia rule on an aperiodic patch builds its
    /// kernel once for all of them.
    fn confirm_elite(&self, id: i64, e: &Eval, lim: &StepLimits<'_>) -> Option<Vec<RunOut>> {
        let _busy = Busy::enter();
        if lim.cut() {
            return None;
        }
        let grid = self.confirm_grid(&e.cand.tiling).ok()?;
        let cands = self.confirmations(e, &grid);
        let kernel = match e.cand.rule() {
            Some(Rule::Lenia(l)) if !matches!(grid.topo.storage, Storage::Lattice(_)) => {
                match jetcarp_core::lenia::kernel_while(&grid.topo, &l, &|| !lim.cut()) {
                    None => return None,
                    Some(Ok(k)) => Some(Arc::new(k)),
                    // Each run finds the same error.
                    Some(Err(_)) => None,
                }
            }
            _ => None,
        };
        let ran: Vec<Ran> = cands
            .into_par_iter()
            .map(|c| {
                let k = kernel.clone();
                self.cpu_eval(&grid, c, "confirm".into(), Some((id, None)), k, 2, lim)
            })
            .collect();
        let mut rows = Vec::new();
        for r in ran {
            match r {
                Ran::Done(r) => rows.push(*r),
                Ran::Failed => {}
                Ran::GivenUp => return None,
            }
        }
        Some(rows)
    }

    /// Run one candidate on the processor and measure it (its id is given when it is
    /// published). A run longer than its stage's cap stops there and is measured as a run of the
    /// generations it reached (its candidate's `gens` become those); one still going at a stop or
    /// the run's end is given up. Its cap counts its own time, not runs its thread takes up while
    /// it waits inside it ([`Busy`]). `kernel`: a Lenia kernel built for it already.
    #[allow(clippy::too_many_arguments)]
    fn cpu_eval(
        &self,
        grid: &Grid,
        mut c: Cand,
        kind: String,
        from: Lineage,
        kernel: Option<Arc<jetcarp_core::lenia::Kernel>>,
        stage: u8,
        lim: &StepLimits<'_>,
    ) -> Ran {
        let t_eval = Instant::now();
        let cap = if stage == 1 {
            self.cpu_caps.0
        } else {
            self.cpu_caps.1
        };
        let fam_name = c.fam.name();
        let gens = c.gens;
        // Why the run stopped early: its cap, or a stop or the run's end.
        let mut cut: Option<&'static str> = None;
        let busy = Busy::enter();
        let slow = SLOW_GENERATION_MS.load(Ordering::Relaxed);
        let hold = HOLD_MB.load(Ordering::Relaxed);
        // In blocks of 4 MB, as a run's own memory comes (a thread's heap holds such blocks).
        let _held: Vec<Vec<u8>> = if hold > 0 && HOLD_RUNS.fetch_sub(1, Ordering::Relaxed) > 0 {
            (0..hold.div_ceil(4)).map(|_| vec![1; 4 << 20]).collect()
        } else {
            Vec::new()
        };
        let r = catch_unwind(AssertUnwindSafe(|| {
            let mut world = prepare(grid, &c).ok()?;
            // A Lenia kernel on an aperiodic patch takes seconds to build (14 s on one thread
            // for R = 17 on a 64,000-cell Penrose patch), in the run's first generation: built
            // here instead, where a stop or the run's end gives it up.
            if let Some(ca) = world.cas.first_mut()
                && let Rule::Lenia(l) = &ca.rule
                && ca.kernel.is_none()
                && !matches!(grid.topo.storage, Storage::Lattice(_))
            {
                if let Some(k) = kernel {
                    ca.kernel = Some(k);
                } else {
                    match jetcarp_core::lenia::kernel_while(&grid.topo, l, &|| !lim.cut()) {
                        None => {
                            cut = Some("given up");
                            return Some(None);
                        }
                        Some(Ok(k)) => ca.kernel = Some(Arc::new(k)),
                        // The first generation finds the same error.
                        Some(Err(_)) => {}
                    }
                }
            }
            let window = if c.fam.is_rows() { 1 } else { WINDOW };
            let keep = (c.fam == Fam::Jaca).then(|| trade_gen(c.gens));
            let (mut t, reached) =
                trace_world_keeping(&mut world, c.gens, window, keep, &mut |_| {
                    if slow > 0 {
                        std::thread::sleep(std::time::Duration::from_millis(slow));
                    }
                    if lim.cut() {
                        cut = Some("given up");
                        false
                    } else if Busy::own() >= cap {
                        cut = Some("capped");
                        false
                    } else {
                        true
                    }
                })
                .ok()?;
            if cut == Some("given up") {
                return Some(None);
            }
            if keep.is_some() && reached < c.gens {
                // Cut short: the frame at three quarters of the generations it reached, from a
                // replay of its start.
                let wnd = frames_for(reached, window).len();
                if t.frames.len() > wnd {
                    t.frames.remove(0);
                }
                if frames_with_trade(reached, window).len() > wnd {
                    match trade_frame(grid, &c, trade_gen(reached), lim) {
                        Some(f) => t.frames.insert(0, f),
                        None => {
                            cut = Some("given up");
                            return Some(None);
                        }
                    }
                }
            }
            c.gens = reached;
            Some(Some(self.evaluate(grid, &c, &t)))
        }));
        let own = Busy::own().as_secs_f64();
        drop(busy);
        let secs = t_eval.elapsed().as_secs_f64();
        // How long past the run's end this one went on (it is given up at its next generation).
        let late = lim.end.map_or(0.0, |e| {
            Instant::now().saturating_duration_since(e).as_secs_f64()
        });
        if std::env::var_os("JETCARP_EXPLORE_TRACE").is_some() && (secs > 2.0 || late > 0.5) {
            let how = match cut {
                Some("capped") => format!(" (capped at generation {})", c.gens),
                Some(w) => format!(" ({w})"),
                None => String::new(),
            };
            let late = if late > 0.5 {
                format!(", {late:.2}s past the end")
            } else {
                String::new()
            };
            let nested = if secs - own > 0.5 {
                format!(" ({:.2}s of it on other runs)", secs - own)
            } else {
                String::new()
            };
            eprintln!(
                "cpu run stage {stage} {} {fam_name} cells={} gens={gens}: {secs:.2}s{nested}{how}{late}",
                grid.key,
                grid.topo.len(),
            );
        }
        let (fate, feat, score, bin) = match r {
            Ok(Some(Some(x))) => x,
            Ok(Some(None)) => {
                self.given_up.fetch_add(1, Ordering::Relaxed);
                return Ran::GivenUp;
            }
            Ok(None) => return Ran::Failed,
            Err(_) => {
                self.panics.fetch_add(1, Ordering::Relaxed);
                return Ran::Failed;
            }
        };
        if cut.is_some() {
            self.capped.fetch_add(1, Ordering::Relaxed);
        }
        let (parent, parent_job, parent_search) = lineage_cols(from);
        Ran::Done(Box::new(RunOut {
            id: 0,
            stage,
            parent,
            parent_job,
            parent_search,
            ev: Eval {
                cand: c,
                fate,
                feat,
                score,
                bin,
            },
            seed_kind: kind,
        }))
    }
}

/// The group (0-7) of a tiling in `tilings`, for the archive's niches (0 when it is not there).
pub fn group_of(tilings: &[TilingChoice], key: &str) -> u8 {
    tilings
        .iter()
        .find(|t| t.key == key)
        .and_then(|t| group_index(&t.group))
        .unwrap_or(0)
}

fn group_index(g: &str) -> Option<u8> {
    space::TILING_GROUPS
        .iter()
        .position(|x| *x == g)
        .map(|i| i as u8)
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

/// A world seeded for a candidate.
fn prepare(grid: &Grid, c: &Cand) -> Result<World, String> {
    let mut w = World::on_topology(grid.topo.clone(), c.rng);
    w.add_ca(c.ca())?;
    Ok(w)
}

/// A run's frame at generation `at`, from a replay of its start (`None` when it is given up at a
/// stop or the run's end, or does not run).
fn trade_frame(grid: &Grid, c: &Cand, at: u32, lim: &StepLimits<'_>) -> Option<Vec<u8>> {
    let mut world = prepare(grid, c).ok()?;
    for _ in 0..at {
        if lim.cut() {
            return None;
        }
        world.step();
    }
    Some(field_bytes(world.fields.first()?))
}

/// Measure a run from its trace: its fate and measures (with JACA's sign measures), its score
/// (by `model` when one applies, else the built-in score), and its behaviour bin.
pub fn evaluate_with(
    grid: &Grid,
    c: &Cand,
    t: &WorldTrace,
    model: Option<&ScoreModel>,
) -> (Fate, Features, f32, u32) {
    let window = if c.fam.is_rows() { 1 } else { WINDOW };
    let (fate, mut f) = measure_run(grid, t, c.gens, window);
    if c.fam == Fam::Jaca {
        f.signs = measure_signs(grid, t, c.gens, window, c.nb);
    }
    let score = archive::score_with(model, fate, &f, c.gens, c.fam);
    let bin = archive::behaviour_bin(fate, &f, c.gens);
    (fate, f, score, bin)
}

impl Search {
    fn evaluate(&self, grid: &Grid, c: &Cand, t: &WorldTrace) -> (Fate, Features, f32, u32) {
        evaluate_with(grid, c, t, self.set.score.as_ref())
    }
}

/// The frames a run of this candidate asks for.
pub fn frames_of(c: &Cand) -> Vec<u32> {
    if c.fam.is_rows() {
        frames_for(c.gens, 1)
    } else if c.fam == Fam::Jaca {
        frames_with_trade(c.gens, WINDOW)
    } else {
        frames_for(c.gens, WINDOW)
    }
}

/// A candidate's world as a GPU batch takes it: its rule and starting states (`None` when it
/// does not seed or its rule does not read).
pub fn batch_world(grid: &Grid, c: &Cand) -> Option<BatchWorld> {
    let world = prepare(grid, c).ok()?;
    Some(BatchWorld {
        rule: c.rule()?,
        init: field_bytes(world.fields.first()?),
    })
}

/// Run a candidate again on its grid (built for its tiling and cells) and measure it, with the
/// built-in score: a stored run replays to its stored measures.
pub fn replay_on(grid: &Grid, c: &Cand) -> Result<Eval, String> {
    let mut world = prepare(grid, c)?;
    let t = trace_world(&mut world, c.gens, &frames_of(c))?;
    let (fate, feat, score, bin) = evaluate_with(grid, c, &t, None);
    Ok(Eval {
        cand: c.clone(),
        fate,
        feat,
        score,
        bin,
    })
}

/// Run one candidate on the CPU now and measure it (with the built-in score).
pub fn try_one(c: Cand) -> Result<Eval, String> {
    try_one_with(c, None)
}

/// [`try_one`], scored by `model` when one applies.
pub fn try_one_with(c: Cand, model: Option<&ScoreModel>) -> Result<Eval, String> {
    let grid = Grid::build(&c.tiling, c.cells)?;
    let mut world = prepare(&grid, &c)?;
    let t = trace_world(&mut world, c.gens, &frames_of(&c))?;
    let (fate, feat, score, bin) = evaluate_with(&grid, &c, &t, model);
    Ok(Eval {
        cand: c,
        fate,
        feat,
        score,
        bin,
    })
}

/// The tilings' groups, by key, for [`crate::explore::store::rescore`].
pub fn tiling_groups() -> HashMap<String, u8> {
    let all = space::all_tilings();
    all.iter()
        .map(|t| (t.key.clone(), group_of(&all, &t.key)))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn a_gpu_batch_is_sized_to_the_time_left() {
        // No end: as many as the batch takes.
        assert_eq!(fit_worlds(Some(1e-9), 1e6, None, 1024), 1024);
        // A pace: the worlds that fill most of the time left; none when not one fits.
        assert_eq!(fit_worlds(Some(1e-9), 1e6, Some(10.0), 1024), 1024);
        assert_eq!(fit_worlds(Some(1e-9), 1e7, Some(1.0), 1024), 70);
        assert_eq!(fit_worlds(Some(1e-6), 1e7, Some(1.0), 1024), 0);
        // No pace yet: a small first batch, unless the end is far off.
        assert_eq!(fit_worlds(None, 1e7, Some(30.0), 1024), FIRST_GPU_WORLDS);
        assert_eq!(fit_worlds(None, 1e7, Some(600.0), 1024), 1024);
        let mut p = GpuPace::default();
        p.note(false, true, 1e9, 2.0);
        p.note(false, false, 1e9, 4.0);
        assert!((p.get(false, true).unwrap() - 3e-9).abs() < 1e-15);
        assert!(p.get(true, true).is_none() && p.get(true, false).is_none());
        // Larger than Life on a lattice and on an aperiodic patch are paced apart.
        p.note(true, true, 1e9, 1.0);
        p.note(true, false, 1e9, 40.0);
        assert_eq!(
            (p.get(true, true), p.get(true, false)),
            (Some(1e-9), Some(4e-8))
        );
    }

    fn quick(gens: u32) -> Settings {
        Settings {
            tilings: vec!["square".into(), "hex".into()],
            families: vec![Fam::LifeLike, Fam::Generations],
            screen_cells: 900,
            screen_gens: gens,
            confirm_cells: 900,
            confirm_gens: gens,
            confirm_seeds: 2,
            seed: 3,
            ..Settings::default()
        }
    }

    /// A run on the processor past its cap is kept as the run of the generations it reached,
    /// and replays to the same measures from its candidate.
    #[test]
    fn a_capped_run_is_kept_as_the_run_it_was() {
        let mut s = Search::new(quick(200_000), Saved::default()).unwrap();
        s.cpu_caps = (Duration::from_millis(30), Duration::from_millis(30));
        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(2)
            .build()
            .unwrap();
        let stop = AtomicBool::new(false);
        let lim = StepLimits {
            until: Instant::now() + Duration::from_millis(200),
            end: None,
            stop: &stop,
            max_screened: None,
        };
        assert!(s.step(&pool, None, &lim).is_none());
        let d = s.take_delta();
        assert!(!d.rows.is_empty() && s.capped() > 0);
        assert_eq!(s.given_up(), 0);
        let capped: Vec<&RunOut> = d.rows.iter().filter(|r| r.ev.cand.gens < 200_000).collect();
        assert!(!capped.is_empty());
        for r in capped.iter().take(4) {
            let ev = try_one(r.ev.cand.clone()).unwrap();
            assert_eq!(
                (ev.fate, ev.feat, ev.score, ev.bin),
                (r.ev.fate, r.ev.feat, r.ev.score, r.ev.bin),
                "run {} ({} generations)",
                r.id,
                r.ev.cand.gens
            );
        }
    }

    /// JACA runs carry their sign measures, a capped one too (its three-quarter frame replayed),
    /// and replay to the same measures from their candidates.
    #[test]
    fn jaca_runs_have_sign_measures_and_replay_to_them() {
        let set = Settings {
            families: vec![Fam::Jaca],
            ..quick(400)
        };
        let mut s = Search::new(set, Saved::default()).unwrap();
        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(2)
            .build()
            .unwrap();
        let stop = AtomicBool::new(false);
        let lim = StepLimits {
            until: Instant::now() + Duration::from_millis(300),
            end: None,
            stop: &stop,
            max_screened: Some(40),
        };
        assert!(s.step(&pool, None, &lim).is_none());
        let full = s.take_delta().rows;
        assert!(!full.is_empty());
        assert!(full.iter().all(|r| r.ev.feat.signs.is_some()));
        s.cpu_caps = (Duration::from_millis(2), Duration::from_millis(2));
        s.set.screen_gens = 200_000;
        let lim = StepLimits {
            until: Instant::now() + Duration::from_millis(300),
            max_screened: None,
            ..lim
        };
        assert!(s.step(&pool, None, &lim).is_none());
        let capped: Vec<RunOut> = s
            .take_delta()
            .rows
            .into_iter()
            .filter(|r| r.ev.cand.gens < 200_000)
            .collect();
        assert!(!capped.is_empty());
        for r in full.iter().take(3).chain(capped.iter().take(3)) {
            let ev = try_one(r.ev.cand.clone()).unwrap();
            assert!(ev.feat.signs.is_some());
            assert_eq!(
                (ev.fate, ev.feat, ev.score, ev.bin),
                (r.ev.fate, r.ev.feat, r.ev.score, r.ev.bin),
                "run {} ({} generations)",
                r.id,
                r.ev.cand.gens
            );
        }
    }

    /// At the run's end the runs still going are given up, unrecorded, so a step ends with it.
    #[test]
    fn runs_still_going_at_the_end_are_given_up() {
        let s = Search::new(quick(5_000_000), Saved::default()).unwrap();
        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(2)
            .build()
            .unwrap();
        let stop = AtomicBool::new(false);
        let t0 = Instant::now();
        let end = t0 + Duration::from_millis(300);
        let lim = StepLimits {
            until: end,
            end: Some(end),
            stop: &stop,
            max_screened: None,
        };
        assert!(s.step(&pool, None, &lim).is_none());
        assert!(t0.elapsed() < Duration::from_secs(3), "{:?}", t0.elapsed());
        let d = s.take_delta();
        assert!(d.rows.is_empty(), "{} rows", d.rows.len());
        assert_eq!((s.screened(), s.capped()), (0, 0));
        assert!(s.given_up() > 0);
    }

    /// A search on the processor alone confirms its elites, and runs every candidate of a round,
    /// even when each round outlasts its step (as rounds of the 2 s steps of a job nearly always
    /// do). Issue #3: the step's deadline was checked inside the round, so most candidates were
    /// never run and the confirmation every fourth round never came.
    #[test]
    fn a_cpu_search_confirms_its_elites_when_rounds_outlast_their_steps() {
        // Runs of 3,000 generations take milliseconds each, so a round of them outlasts its 2 ms
        // step, as rounds outlast the driver's 2 s steps. A step on a slow machine may end before
        // a round starts at all, so it steps until a confirmation comes (or a minute passes).
        let s = Search::new(quick(3000), Saved::default()).unwrap();
        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(2)
            .build()
            .unwrap();
        let stop = AtomicBool::new(false);
        let t0 = Instant::now();
        while s.confirmed() == 0 && t0.elapsed() < Duration::from_secs(60) {
            let lim = StepLimits {
                until: Instant::now() + Duration::from_millis(2),
                end: None,
                stop: &stop,
                max_screened: None,
            };
            assert!(s.step(&pool, None, &lim).is_none());
        }
        assert!(s.confirmed() > 0, "no confirmation runs");
        assert!(s.confirmed_elites() > 0, "no elite confirmed");
        // Every candidate made was run: the screening runs and the confirmation runs account
        // for the identities tried (a run that fails to start is the only gap).
        let tried = s.seen();
        let ran = s.screened() + s.confirmed();
        assert!(ran * 100 >= tried * 95, "{ran} runs of {tried} candidates");
    }

    /// Screening goes on while elites are confirmed (#83): a confirmation runs on the round's
    /// threads beside the round's screening, an elite's seeds side by side, so it holds no thread
    /// for longer than one run and never all of them. Here a confirmation run takes a second (its
    /// cap) and a screening run milliseconds; the runs screened stood still for the whole of each
    /// fourth round's confirmations (two elites a thread, two seeds each, one after another:
    /// about four seconds).
    #[test]
    fn screening_goes_on_while_elites_are_confirmed() {
        let set = Settings {
            confirm_gens: 5_000_000,
            ..quick(300)
        };
        let mut s = Search::new(set, Saved::default()).unwrap();
        s.cpu_caps = (Duration::from_secs(8), Duration::from_millis(1000));
        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(2)
            .build()
            .unwrap();
        let stop = AtomicBool::new(false);
        let done = AtomicBool::new(false);
        let longest = std::thread::scope(|sc| {
            let watch = sc.spawn(|| {
                let (mut at, mut n, mut longest) = (Instant::now(), s.screened(), Duration::ZERO);
                while !done.load(Ordering::SeqCst) {
                    let now = s.screened();
                    if now != n {
                        longest = longest.max(at.elapsed());
                        (at, n) = (Instant::now(), now);
                    }
                    std::thread::sleep(Duration::from_millis(5));
                }
                longest
            });
            let t0 = Instant::now();
            while t0.elapsed() < Duration::from_secs(9) {
                let lim = StepLimits {
                    until: Instant::now() + Duration::from_secs(2),
                    end: None,
                    stop: &stop,
                    max_screened: None,
                };
                assert!(s.step(&pool, None, &lim).is_none());
            }
            done.store(true, Ordering::SeqCst);
            watch.join().unwrap()
        });
        assert!(s.confirmed() >= 8, "{} confirmation runs", s.confirmed());
        assert!(
            longest < Duration::from_millis(2500),
            "screening stood still for {longest:?}"
        );
    }

    /// A slow GPU: each batch takes 50 ns a world-cell-generation (and runs on the processor),
    /// and its batches are kept: (worlds, generations, seconds).
    struct SlowBatches(Arc<Mutex<Vec<(usize, u32, f64)>>>);

    impl Backend for SlowBatches {
        fn name(&self) -> String {
            "slow batches".into()
        }
        fn supports(&self, rule: &Rule) -> bool {
            jetcarp_core::explore::batch::CpuBatch.supports(rule)
        }
        fn max_cells(&self) -> usize {
            1 << 22
        }
        fn run(&mut self, job: &BatchJob) -> Result<Vec<WorldTrace>, String> {
            let t0 = Instant::now();
            let work = job.worlds.len() as f64 * job.topo.len() as f64 * job.gens as f64;
            std::thread::sleep(Duration::from_secs_f64(work * 5e-8));
            let out = jetcarp_core::explore::batch::run_cpu(job);
            let secs = t0.elapsed().as_secs_f64();
            self.0
                .lock()
                .unwrap()
                .push((job.worlds.len(), job.gens, secs));
            out
        }
    }

    /// A batch of confirmation runs on the GPU is sized to about `gpu_confirm_s` of its time by
    /// its pace so far, so the GPU's screening waits for none longer (#83): here a batch of every
    /// elite of a tiling waiting (two seeds each, ten times the generations of a screening run)
    /// took about two seconds of this GPU's.
    #[test]
    fn a_gpu_confirmation_batch_is_sized_to_a_few_seconds() {
        let set = Settings {
            tilings: vec!["square".into()],
            screen_cells: 400,
            screen_gens: 48,
            confirm_cells: 400,
            confirm_gens: 960,
            batch: 64,
            ..quick(48)
        };
        let mut s = Search::new(set, Saved::default()).unwrap();
        s.gpu_confirm_s = 0.3;
        let batches = Arc::new(Mutex::new(Vec::new()));
        let mut be = SlowBatches(batches.clone());
        let (pool, gpu_pool) = (
            rayon::ThreadPoolBuilder::new()
                .num_threads(1)
                .build()
                .unwrap(),
            rayon::ThreadPoolBuilder::new()
                .num_threads(2)
                .build()
                .unwrap(),
        );
        let stop = AtomicBool::new(false);
        let confirms = |b: &[(usize, u32, f64)]| b.iter().filter(|x| x.1 == 960).count();
        let t0 = Instant::now();
        while confirms(&batches.lock().unwrap()) < 3 && t0.elapsed() < Duration::from_secs(60) {
            let lim = StepLimits {
                until: Instant::now() + Duration::from_millis(500),
                end: None,
                stop: &stop,
                max_screened: None,
            };
            assert!(s.step(&pool, Some((&mut be, &gpu_pool)), &lim).is_none());
        }
        let b = batches.lock().unwrap().clone();
        let confirm: Vec<_> = b.iter().filter(|x| x.1 == 960).collect();
        assert!(confirm.len() >= 3, "{b:?}");
        assert!(s.confirmed_elites() > 0);
        for x in &confirm {
            assert!(x.2 < 0.9, "a confirmation batch took {:.2} s: {x:?}", x.2);
        }
    }

    /// The runs of a search on the processor: (id, stage, parent, candidate identity, score,
    /// bin), in id order, after `n` screened, its steps `step` long.
    fn runs_of(set: &Settings, threads: usize, step: Duration, n: u64) -> Vec<RunKey> {
        let s = Search::new(set.clone(), Saved::default()).unwrap();
        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(threads)
            .build()
            .unwrap();
        let stop = AtomicBool::new(false);
        let mut rows = Vec::new();
        while s.screened() < n {
            let lim = StepLimits {
                until: Instant::now() + step,
                end: None,
                stop: &stop,
                max_screened: Some(n),
            };
            assert!(s.step(&pool, None, &lim).is_none());
            rows.extend(s.take_delta().rows);
        }
        let mut v: Vec<RunKey> = rows
            .iter()
            .map(|r| {
                (
                    r.id,
                    r.stage,
                    r.parent,
                    r.ev.cand.id_hash(),
                    r.ev.score.to_bits(),
                    r.ev.bin,
                )
            })
            .collect();
        v.sort_unstable();
        v
    }

    type RunKey = (i64, u8, Option<i64>, u64, u32, u32);

    /// A search on the processor makes the same runs, with the same ids, measures and parents,
    /// and confirms the same elites, whatever its steps' length and however its threads take up
    /// its runs: scheduling decides only when a run is made, not what. (Its rounds are sized to
    /// its threads, so the thread count is one of its settings here; its runs are short, so none
    /// meets its time cap, which is wall time.)
    #[test]
    fn a_cpu_search_is_the_same_whatever_its_steps() {
        let set = Settings {
            families: vec![Fam::LifeLike, Fam::Generations, Fam::Cyclic],
            seed: 9,
            ..quick(200)
        };
        let long = runs_of(&set, 3, Duration::from_secs(30), 500);
        let short = runs_of(&set, 3, Duration::from_millis(1), 500);
        assert!(long.iter().any(|r| r.1 == 2), "no confirmation runs");
        assert_eq!(long.len(), short.len());
        assert!(long == short, "the runs differ");
        assert!(runs_of(&set, 3, Duration::from_millis(40), 500) == long);
    }

    /// What a run on the processor costs, by family, on one thread (as each runs in a CPU
    /// round): screening runs and confirmation runs of candidates made as the search makes them,
    /// capped at two minutes rather than at [`CPU_RUN_S`] and [`CPU_CONFIRM_RUN_S`]; the families
    /// run side by side, one thread each. Ignored; `N` candidates per family (default 12), `FAMS`
    /// a comma list (default every family, as on a computer without a GPU), `SEED`:
    /// `cargo test --release -p jetcarp-search cpu_cost_by_family -- --ignored --nocapture`.
    /// On a 16-core desktop (median and worst of 6, seconds; caps 8 and 32): sandpile screening
    /// 21 and 37, confirmation 76 and over 120; Lenia 7.4 and 14, 21 and 37; the GPU's families
    /// (on the processor without a GPU) screening at most 3.7, confirmation 6 to 23; the rest at
    /// most 0.6 and 6.5.
    #[test]
    #[ignore]
    fn cpu_cost_by_family() {
        let n: usize = std::env::var("N")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(12);
        let seed: u64 = std::env::var("SEED")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(1);
        let fams: Vec<Fam> = match std::env::var("FAMS") {
            Ok(s) => s
                .split(',')
                .map(|f| serde_json::from_value(serde_json::json!(f.trim())).unwrap())
                .collect(),
            Err(_) => families(&sampled_families(), &[], false)
                .1
                .into_iter()
                .map(|f| f.0)
                .collect(),
        };
        let lines = Mutex::new(Vec::new());
        std::thread::scope(|sc| {
            for (k, fam) in fams.iter().enumerate() {
                let lines = &lines;
                sc.spawn(move || {
                    let mut s = Search::new(
                        Settings {
                            families: vec![*fam],
                            seed: seed + k as u64,
                            ..Settings::default()
                        },
                        Saved::default(),
                    )
                    .unwrap();
                    s.cpu_caps = (Duration::from_secs(120), Duration::from_secs(120));
                    let pool = rayon::ThreadPoolBuilder::new()
                        .num_threads(1)
                        .build()
                        .unwrap();
                    let stop = AtomicBool::new(false);
                    let lim = StepLimits {
                        until: Instant::now() + Duration::from_secs(1 << 20),
                        end: None,
                        stop: &stop,
                        max_screened: None,
                    };
                    let mut rng = Rng::new(seed * 7919 + k as u64);
                    let (mut sc_t, mut cf_t) = (Vec::new(), Vec::new());
                    let mut worst = (0.0f64, String::new());
                    let native = fam.only_on();
                    let mut tries = 0;
                    while sc_t.len() < n && tries < n * 20 {
                        tries += 1;
                        let ti = match native {
                            Some(key) => s.tiling_index(key).unwrap(),
                            None => s.pick_tiling(&mut rng),
                        };
                        let key = s.tilings[ti].key.clone();
                        let Ok(grid) = s.screen_grid(&key) else {
                            continue;
                        };
                        let nb = if grid.has_edge && rng.below(4) == 0 {
                            Nb::Edge
                        } else {
                            Nb::Touch
                        };
                        let c = s.emit(
                            &grid,
                            nb,
                            &[(*fam, 1.0)],
                            None,
                            1,
                            s.set.screen_gens,
                            &mut rng,
                            None,
                        );
                        let Some((c, kind, from)) = c.into_iter().next() else {
                            continue;
                        };
                        let what = format!("{} {} {:?}", c.tiling, c.rule, c.seed);
                        let t = Instant::now();
                        let r = pool.install(|| s.cpu_eval(&grid, c, kind, from, None, 1, &lim));
                        let ts = t.elapsed().as_secs_f64();
                        let Ran::Done(r) = r else { continue };
                        sc_t.push(ts);
                        if ts > worst.0 {
                            worst = (ts, format!("screen {what}"));
                        }
                        let Ok(big) = s.confirm_grid(&key) else {
                            continue;
                        };
                        let Some(c2) = s.confirmations(&r.ev, &big).into_iter().next() else {
                            continue;
                        };
                        let t = Instant::now();
                        let r2 = pool.install(|| {
                            s.cpu_eval(
                                &big,
                                c2,
                                "confirm".into(),
                                Some((r.id, None)),
                                None,
                                2,
                                &lim,
                            )
                        });
                        let tc = t.elapsed().as_secs_f64();
                        if matches!(r2, Ran::Done(_)) {
                            cf_t.push(tc);
                            if tc > worst.0 {
                                worst = (tc, format!("confirm {what}"));
                            }
                        }
                    }
                    let stat = |v: &mut Vec<f64>| -> String {
                        if v.is_empty() {
                            return "-".into();
                        }
                        v.sort_by(|a, b| a.partial_cmp(b).unwrap());
                        let q = |p: f64| v[((v.len() - 1) as f64 * p).round() as usize];
                        format!("{:7.2} {:7.2} {:7.2}", q(0.5), q(0.9), q(1.0))
                    };
                    let line = format!(
                        "{:20} {:3} screen(med p90 max) {}  confirm {}  worst {:.1}s {}",
                        fam.name(),
                        sc_t.len(),
                        stat(&mut sc_t),
                        stat(&mut cf_t),
                        worst.0,
                        worst.1.chars().take(160).collect::<String>()
                    );
                    eprintln!("{line}");
                    lines.lock().unwrap().push(line);
                });
            }
        });
        let mut v = lines.into_inner().unwrap();
        v.sort();
        println!("{}", v.join("\n"));
    }

    /// JACA's effort: at least 30% fresh draws, with transplants and balance variants; the other
    /// families as before.
    #[test]
    fn jaca_splits_its_effort() {
        let mut n: HashMap<&str, usize> = HashMap::new();
        for i in 0..1000 {
            let w = way(Fam::Jaca, (i as f64 + 0.5) / 1000.0);
            *n.entry(format!("{w:?}").leak()).or_default() += 1;
        }
        assert_eq!(n["Random"], 300);
        assert_eq!(
            (n["Mutate"], n["Transplant"], n["Balance"]),
            (200, 150, 150)
        );
        assert_eq!((n["Seed"], n["Cover"]), (100, 100));
        assert!(!n.contains_key("Named"));
        assert_eq!(way(Fam::LifeLike, 0.39), Way::Random);
        assert_eq!(way(Fam::LifeLike, 0.95), Way::Named);
        assert_eq!(way(Fam::LifeLike, 0.7), Way::Seed);
    }

    /// A search given parents transplants their pairs, unchanged, to tilings they have not run
    /// on (once per pair and tiling), scales them when the neighbour counts differ, and tunes the
    /// losing ones for balance.
    #[test]
    fn parents_are_transplanted_and_tuned() {
        let base = Settings {
            tilings: vec!["square".into(), "hex".into(), "trihex".into()],
            families: vec![Fam::Jaca],
            screen_cells: 600,
            screen_gens: 120,
            confirm_cells: 600,
            confirm_gens: 120,
            confirm_seeds: 1,
            seed: 11,
            ..Settings::default()
        };
        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(2)
            .build()
            .unwrap();
        let stop = AtomicBool::new(false);
        let run = |s: &Search, n: u64| {
            let lim = StepLimits {
                until: Instant::now() + Duration::from_secs(30),
                end: None,
                stop: &stop,
                max_screened: Some(n),
            };
            assert!(s.step(&pool, None, &lim).is_none());
            s.take_delta().rows
        };
        // Parents: the first search's runs on the square grid, every one taken as strong, and
        // the odd ones as losing their positive sign.
        let first = Search::new(base.clone(), Saved::default()).unwrap();
        let parents: Vec<Parent> = run(&first, 60)
            .iter()
            .filter(|r| r.ev.cand.tiling == "square")
            .take(8)
            .enumerate()
            .map(|(i, r)| Parent {
                strong: true,
                weaker: (i % 2 == 1).then_some(breed::Sign::Pos),
                ..Parent::of(1, OTHER_SEARCH, r.id, "keep", &r.ev)
            })
            .collect();
        assert!(parents.len() >= 4, "{} parents", parents.len());
        let s = Search::new(
            Settings {
                parents: parents.clone(),
                seed: 12,
                ..base
            },
            Saved::default(),
        )
        .unwrap();
        let rows = run(&s, 300);
        let by = |o: &str| rows.iter().filter(|r| r.ev.cand.origin == o).count();
        assert!(by("random") > 0 && by("transplant") > 0 && by("balance") > 0);
        let rules: HashSet<&str> = parents.iter().map(|p| p.rule.as_str()).collect();
        let mut seen = HashSet::new();
        for r in rows.iter().filter(|r| r.ev.cand.origin == "transplant") {
            let c = &r.ev.cand;
            if rules.contains(c.rule.as_str()) {
                assert_ne!(c.tiling, "square", "a parent's own tiling");
                assert!(seen.insert((c.rule.clone(), c.tiling.clone())), "twice");
            }
        }
        // Scaled transplants are JACA pairs too, never a parent's pair unchanged.
        for r in rows
            .iter()
            .filter(|r| r.ev.cand.origin == "transplant-scaled")
        {
            assert!(r.ev.cand.rule.starts_with("JACA"), "{}", r.ev.cand.rule);
            assert!(!rules.contains(r.ev.cand.rule.as_str()) || r.ev.cand.tiling != "square");
        }
        // Balance variants come from the losing parents (or losing elites).
        assert!(
            rows.iter()
                .filter(|r| r.ev.cand.origin == "balance")
                .all(|r| r.ev.cand.fam == Fam::Jaca)
        );
    }

    /// The uuid of the search the tests' parents come from (`--parents-from`).
    const OTHER_SEARCH: &str = "00000000-0000-4000-8000-0000000000a1";

    /// Every run made from another names it as its `parent` (#28): a mutation or a new seed of an
    /// elite (a screened run of the search, before it), a transplant or a balance variant (an
    /// elite, or a run of the job its parents came from), and a confirmation run the run it
    /// confirms; fresh draws, cover runs and named rules have none.
    #[test]
    fn runs_record_their_lineage() {
        let base = Settings {
            tilings: vec!["square".into(), "hex".into(), "trihex".into()],
            families: vec![Fam::Jaca, Fam::LifeLike],
            screen_cells: 600,
            screen_gens: 120,
            confirm_cells: 600,
            confirm_gens: 120,
            confirm_seeds: 1,
            seed: 21,
            ..Settings::default()
        };
        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(2)
            .build()
            .unwrap();
        let stop = AtomicBool::new(false);
        let run = |s: &Search, n: u64| {
            let mut rows = Vec::new();
            while s.screened() < n {
                let lim = StepLimits {
                    until: Instant::now() + Duration::from_millis(20),
                    end: None,
                    stop: &stop,
                    max_screened: Some(n),
                };
                assert!(s.step(&pool, None, &lim).is_none());
                rows.extend(s.take_delta().rows);
            }
            rows
        };
        let first = Search::new(base.clone(), Saved::default()).unwrap();
        let parents: Vec<Parent> = run(&first, 80)
            .iter()
            .filter(|r| r.ev.cand.fam == Fam::Jaca && r.stage == 1)
            .take(8)
            .enumerate()
            .map(|(i, r)| Parent {
                strong: true,
                weaker: (i % 2 == 1).then_some(breed::Sign::Pos),
                ..Parent::of(1, OTHER_SEARCH, r.id, "keep", &r.ev)
            })
            .collect();
        assert!(parents.len() >= 4, "{} parents", parents.len());
        let s = Search::new(
            Settings {
                parents: parents.clone(),
                seed: 22,
                ..base
            },
            Saved::default(),
        )
        .unwrap();
        let rows = run(&s, 600);
        let by_id: HashMap<i64, &RunOut> = rows.iter().map(|r| (r.id, r)).collect();
        let mut seen: HashMap<&str, usize> = HashMap::new();
        for r in &rows {
            let o = r.ev.cand.origin.as_str();
            *seen.entry(o).or_default() += 1;
            let elite = r.parent.and_then(|p| by_id.get(&p));
            match o {
                "random" | "cover" | "named" => assert_eq!(r.parent, None, "run {} ({o})", r.id),
                "confirm" => {
                    let p = elite.expect("a confirmation run's parent");
                    assert_eq!((p.stage, &p.ev.cand.rule), (1, &r.ev.cand.rule));
                }
                "mutate" | "seed" => {
                    let p = elite.unwrap_or_else(|| panic!("run {} ({o}): {:?}", r.id, r.parent));
                    assert!(p.stage == 1 && p.id < r.id, "run {} ({o})", r.id);
                    if o == "seed" {
                        assert_eq!(p.ev.cand.rule, r.ev.cand.rule);
                    }
                }
                _ => {
                    // A transplant or a balance variant: an elite, or a parent of the other job.
                    let p = r
                        .parent
                        .unwrap_or_else(|| panic!("run {} ({o}) has none", r.id));
                    match r.parent_job {
                        Some(job) => {
                            assert_eq!(job, 1, "run {} ({o})", r.id);
                            assert_eq!(r.parent_search.as_deref(), Some(OTHER_SEARCH));
                            let q = parents.iter().find(|q| q.run == p).expect("its parent");
                            if o != "balance" {
                                // Transplanted unchanged, or scaled from it.
                                assert!(o == "transplant-scaled" || q.rule == r.ev.cand.rule);
                            }
                        }
                        None => assert!(
                            elite.is_some_and(|e| e.stage == 1 && e.id < r.id),
                            "run {} ({o}): parent {p}",
                            r.id
                        ),
                    }
                }
            }
            if r.parent_job.is_some() {
                assert!(matches!(o, "transplant" | "transplant-scaled" | "balance"));
            }
            // A run made from this search's own names no other search.
            assert_eq!(
                r.parent_search.is_some(),
                r.parent_job.is_some(),
                "run {}",
                r.id
            );
        }
        for o in [
            "random",
            "mutate",
            "seed",
            "transplant",
            "balance",
            "confirm",
        ] {
            assert!(seen.get(o).is_some_and(|&n| n > 0), "no {o} run: {seen:?}");
        }
        let from_job = rows.iter().filter(|r| r.parent_job.is_some()).count();
        let from_elites = rows
            .iter()
            .filter(|r| r.stage == 1 && r.parent.is_some() && r.parent_job.is_none())
            .count();
        assert!(from_job > 0 && from_elites > 0, "{from_job} {from_elites}");
    }

    /// Runs of a search on one tiling until a confirmation run comes (or a minute passes): its
    /// screened runs' and confirmation runs' grids, as their candidates rebuild them.
    fn grid_sizes(set: Settings) -> (HashSet<usize>, HashSet<usize>) {
        let s = Search::new(set, Saved::default()).unwrap();
        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(2)
            .build()
            .unwrap();
        let stop = AtomicBool::new(false);
        let (mut screened, mut confirmed) = (HashSet::new(), HashSet::new());
        let t0 = Instant::now();
        // Until the first confirmation: a few seconds here, but a 35,000-tile screening patch on
        // a busy 4-core CI runner, beside the other search tests, once took over a minute.
        while confirmed.is_empty() && t0.elapsed() < Duration::from_secs(300) {
            let lim = StepLimits {
                until: Instant::now() + Duration::from_millis(50),
                end: None,
                stop: &stop,
                max_screened: None,
            };
            assert!(s.step(&pool, None, &lim).is_none());
            for r in s.take_delta().rows {
                let c = &r.ev.cand;
                let n = Grid::build(&c.tiling, c.cells).unwrap().topo.len();
                if r.stage == 1 {
                    screened.insert(n);
                } else {
                    confirmed.insert(n);
                }
            }
        }
        (screened, confirmed)
    }

    /// An aperiodic patch comes in levels of about 500, 4,000, 35,000 and 280,000 tiles. A
    /// search's screening patch is the level nearest the cells asked (4,000 for the default
    /// 16,000) and its confirming patch is bigger (35,000 for 64,000), as periodic grids are;
    /// both were the 35,000-tile level (#55). A run records its patch's tiles, so it rebuilds
    /// that patch. A search made before keeps its sizing (a spec without `sizing`).
    #[test]
    fn aperiodic_patches_are_sized_to_the_nearest_level() {
        let set = Settings {
            tilings: vec!["penrose-rhomb".into()],
            families: vec![Fam::LifeLike],
            screen_gens: 48,
            confirm_gens: 48,
            confirm_seeds: 1,
            seed: 7,
            ..Settings::default()
        };
        let (screened, confirmed) = grid_sizes(set.clone());
        assert_eq!(screened, HashSet::from([4000]), "screening patches");
        assert_eq!(confirmed, HashSet::from([35000]), "confirming patches");
        let mut old = serde_json::to_value(&set).unwrap();
        old.as_object_mut().unwrap().remove("sizing");
        let old: Settings = serde_json::from_value(old).unwrap();
        let (screened, confirmed) = grid_sizes(old);
        assert_eq!(
            screened,
            HashSet::from([35000]),
            "an older search's screening"
        );
        assert_eq!(
            confirmed,
            HashSet::from([35000]),
            "an older search's confirming"
        );
    }

    /// Small, quick settings for the tests of what a search draws from (#239): every tiling and
    /// family unless `extra` (a spec's keys) says otherwise.
    fn choosing(extra: serde_json::Value) -> Settings {
        let mut v = serde_json::to_value(Settings {
            screen_cells: 600,
            screen_gens: 96,
            confirm_cells: 600,
            confirm_gens: 96,
            confirm_seeds: 1,
            // Small GPU batches, so the GPU's runs come in many rounds beside the processor's.
            batch: 64,
            ..Settings::default()
        })
        .unwrap();
        for (k, x) in extra.as_object().unwrap() {
            v[k] = x.clone();
        }
        serde_json::from_value(v).unwrap()
    }

    /// The runs of a search, screened and confirming, until `n` are screened, every family it
    /// names has a run, and (when the processor has families to run) eight CPU rounds have run,
    /// so its rounds for the families tied to one grid have come whatever the GPU did meanwhile
    /// (or two minutes pass); with a GPU (a stand-in that runs its batches on the processor,
    /// beside the CPU rounds) or on the processor alone.
    fn sampled(set: &Settings, gpu: bool, n: u64) -> Vec<RunOut> {
        let s = Search::new(set.clone(), Saved::default()).unwrap();
        let cpu = !families(&set.families, &set.skip_families, gpu)
            .1
            .is_empty();
        let rounds = || s.cpu_rounds.load(Ordering::Relaxed);
        let named = set.named_families();
        let seen = Mutex::new(HashSet::new());
        let enough = || {
            s.screened() >= n
                && (!cpu || rounds() >= 8)
                && named.iter().all(|f| seen.lock().unwrap().contains(f))
        };
        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(2)
            .build()
            .unwrap();
        let gpu_pool = rayon::ThreadPoolBuilder::new()
            .num_threads(2)
            .build()
            .unwrap();
        let mut be = jetcarp_core::explore::batch::CpuBatch;
        let stop = AtomicBool::new(false);
        let mut rows = Vec::new();
        let t0 = Instant::now();
        while !enough() && t0.elapsed() < Duration::from_secs(120) {
            let lim = StepLimits {
                until: Instant::now() + Duration::from_millis(200),
                end: None,
                stop: &stop,
                max_screened: None,
            };
            let backend = gpu.then_some((&mut be as &mut dyn Backend, &gpu_pool));
            assert!(s.step(&pool, backend, &lim).is_none());
            let d = s.take_delta().rows;
            seen.lock().unwrap().extend(d.iter().map(|r| r.ev.cand.fam));
            rows.extend(d);
        }
        assert!(
            enough(),
            "{} screened, {} CPU rounds, families run {:?} of {named:?} (gpu {gpu})",
            s.screened(),
            rounds(),
            seen.lock().unwrap()
        );
        rows
    }

    /// A search told which tilings and families to search draws only from them, on the GPU and
    /// on the processor, and reaches every family named (#239).
    #[test]
    fn a_search_samples_only_the_tilings_and_families_chosen() {
        let all = space::all_tilings();
        let group = |k: &str| all.iter().find(|t| t.key == k).unwrap().group.clone();
        let set = choosing(serde_json::json!({
            "tiling_groups": ["square", "hexagon"],
            "skip_tilings": ["brick"],
            "families": ["life_like", "turmite", "isotropic", "wolfram"],
            "seed": 31,
        }));
        for gpu in [false, true] {
            let rows = sampled(&set, gpu, 400);
            let fams: HashSet<Fam> = rows.iter().map(|r| r.ev.cand.fam).collect();
            let tilings: HashSet<&str> = rows.iter().map(|r| r.ev.cand.tiling.as_str()).collect();
            for r in &rows {
                let c = &r.ev.cand;
                assert!(set.families.contains(&c.fam), "{:?} (gpu {gpu})", c.fam);
                assert_eq!(c.fam, Fam::of(&c.rule().unwrap()), "{}", c.rule);
                let g = group(&c.tiling);
                assert!(g == "square" || g == "hexagon", "{} ({g})", c.tiling);
                assert_ne!(c.tiling, "brick");
                if matches!(c.fam, Fam::Isotropic | Fam::Wolfram) {
                    assert_eq!(c.tiling, "square", "{:?}", c.fam);
                }
            }
            for f in [Fam::LifeLike, Fam::Turmite, Fam::Isotropic, Fam::Wolfram] {
                assert!(fams.contains(&f), "no {f:?} run (gpu {gpu}): {fams:?}");
            }
            assert!(tilings.contains("hex"), "{tilings:?}");
        }
    }

    /// The tilings, groups and families a search leaves out never appear in its runs (#239).
    #[test]
    fn the_tilings_and_families_left_out_never_appear() {
        let all = space::all_tilings();
        let group = |k: &str| all.iter().find(|t| t.key == k).unwrap().group.clone();
        let skipped_fams = [
            Fam::LifeLike,
            Fam::Generations,
            Fam::Turmite,
            Fam::Lenia,
            Fam::Sandpile,
        ];
        let set = choosing(serde_json::json!({
            "skip_tiling_groups": ["aperiodic", "isohedral", "k-uniform"],
            "skip_tilings": ["square", "hex"],
            "skip_families": skipped_fams,
            "seed": 32,
        }));
        let s = Search::new(set.clone(), Saved::default()).unwrap();
        assert!(
            s.tilings
                .iter()
                .all(|t| t.key != "square" && t.key != "hex")
        );
        for gpu in [false, true] {
            let rows = sampled(&set, gpu, 400);
            let fams: HashSet<Fam> = rows.iter().map(|r| r.ev.cand.fam).collect();
            for r in &rows {
                let c = &r.ev.cand;
                assert!(!skipped_fams.contains(&c.fam), "{:?} (gpu {gpu})", c.fam);
                assert_ne!(c.fam, Fam::Isotropic, "never named");
                assert!(c.tiling != "square" && c.tiling != "hex", "{}", c.tiling);
                let g = group(&c.tiling);
                assert!(
                    !["aperiodic", "isohedral", "k-uniform"].contains(&g.as_str()),
                    "{g}"
                );
            }
            assert!(fams.len() >= 4, "{fams:?}");
        }
    }

    /// Isotropic rules are left out of the default schedule: a search that names no families
    /// never runs one, nor one of the catalogue's. Named, they are sampled beside the others, on
    /// the square grid only; named alone, every round goes to the square grid (#239).
    #[test]
    fn isotropic_rules_are_sampled_when_named_and_never_otherwise() {
        let unnamed = choosing(serde_json::json!({
            "tilings": ["square", "hex"],
            "skip_families": ["lenia", "sandpile", "forest_fire"],
            "seed": 33,
        }));
        let s = Search::new(unnamed.clone(), Saved::default()).unwrap();
        let iso = |n: &String| matches!(Rule::parse(n), Ok((Rule::Isotropic(_), _)));
        assert!(!s.named.lock().unwrap().iter().any(|(_, n)| iso(n)));
        // Named beside every family that runs on these two tilings, the catalogue's isotropic
        // rules come too.
        let every = Settings {
            families: sampled_families()
                .into_iter()
                .filter(|f| f.runs_on("square") || f.runs_on("hex"))
                .collect(),
            ..unnamed.clone()
        };
        let s = Search::new(every, Saved::default()).unwrap();
        assert!(s.named.lock().unwrap().iter().any(|(_, n)| iso(n)));
        // Named beside families of any tiling, they run on the square grid only: on the processor
        // in a quarter of its rounds, and with a GPU in rounds of the GPU's own there (#245).
        let named = Settings {
            families: vec![Fam::LifeLike, Fam::Generations, Fam::Cyclic, Fam::Isotropic],
            ..unnamed.clone()
        };
        let alone = choosing(serde_json::json!({"families": ["isotropic"], "seed": 34}));
        let s = Search::new(alone.clone(), Saved::default()).unwrap();
        assert_eq!(s.tiling_count(), 1, "only the square grid");
        for gpu in [false, true] {
            let rows = sampled(&unnamed, gpu, 300);
            for r in &rows {
                let c = &r.ev.cand;
                assert_ne!(c.fam, Fam::Isotropic, "{} (gpu {gpu})", c.rule);
                assert!(!iso(&c.rule), "{} (gpu {gpu})", c.rule);
            }
            let rows = sampled(&named, gpu, 300);
            let iso_runs: Vec<&RunOut> = rows
                .iter()
                .filter(|r| r.ev.cand.fam == Fam::Isotropic)
                .collect();
            assert!(!iso_runs.is_empty(), "no isotropic run (gpu {gpu})");
            for r in iso_runs {
                assert!(iso(&r.ev.cand.rule), "{}", r.ev.cand.rule);
                assert_eq!(r.ev.cand.tiling, "square");
            }
            let rows = sampled(&alone, gpu, 150);
            assert!(rows.iter().all(|r| r.ev.cand.fam == Fam::Isotropic
                && r.ev.cand.tiling == "square"
                && iso(&r.ev.cand.rule)));
        }
    }

    /// A stand-in GPU that runs its batches on the processor and takes the rules the GPU's batch
    /// stepper takes (`jetcarp_gpu::batch::BatchSim::supports`), counting the isotropic worlds
    /// it ran.
    struct CountingBatches(Arc<AtomicU64>);

    impl Backend for CountingBatches {
        fn name(&self) -> String {
            "counting batches".into()
        }
        fn supports(&self, rule: &Rule) -> bool {
            jetcarp_gpu::batch::BatchSim::supports(rule)
        }
        fn max_cells(&self) -> usize {
            1 << 22
        }
        fn run(&mut self, job: &BatchJob) -> Result<Vec<WorldTrace>, String> {
            let out = jetcarp_core::explore::batch::run_cpu(job)?;
            let iso = job
                .worlds
                .iter()
                .filter(|w| matches!(w.rule, Rule::Isotropic(_)))
                .count();
            self.0.fetch_add(iso as u64, Ordering::Relaxed);
            Ok(out)
        }
    }

    /// With a GPU, a search that names isotropic rules runs them in the GPU's batches, screening
    /// and confirming them there, and the processor runs none of them (#245): every isotropic run
    /// the search makes is one the GPU ran, while the processor goes on with its own families.
    /// Without a GPU the processor runs them, drawn as before (at 8, in their place).
    #[test]
    fn isotropic_rules_run_on_the_gpu_when_there_is_one() {
        let set = choosing(serde_json::json!({
            "families": ["life_like", "turmite", "isotropic"],
            "seed": 37,
        }));
        let s = Search::new(set.clone(), Saved::default()).unwrap();
        let ran = Arc::new(AtomicU64::new(0));
        let mut be = CountingBatches(ran.clone());
        let (pool, gpu_pool) = (
            rayon::ThreadPoolBuilder::new()
                .num_threads(2)
                .build()
                .unwrap(),
            rayon::ThreadPoolBuilder::new()
                .num_threads(2)
                .build()
                .unwrap(),
        );
        let stop = AtomicBool::new(false);
        let mut rows: Vec<RunOut> = Vec::new();
        let count = |rows: &[RunOut], fam: Fam, stage: u8| {
            rows.iter()
                .filter(|r| r.ev.cand.fam == fam && r.stage == stage)
                .count()
        };
        let enough = |rows: &[RunOut]| {
            count(rows, Fam::Isotropic, 1) >= 100
                && count(rows, Fam::Isotropic, 2) >= 1
                && count(rows, Fam::Turmite, 1) >= 1
        };
        let t0 = Instant::now();
        while !enough(&rows) && t0.elapsed() < Duration::from_secs(120) {
            let lim = StepLimits {
                until: Instant::now() + Duration::from_millis(200),
                end: None,
                stop: &stop,
                max_screened: None,
            };
            assert!(s.step(&pool, Some((&mut be, &gpu_pool)), &lim).is_none());
            rows.extend(s.take_delta().rows);
        }
        let iso_runs = rows
            .iter()
            .filter(|r| r.ev.cand.fam == Fam::Isotropic)
            .count() as u64;
        assert_eq!(
            iso_runs,
            ran.load(Ordering::Relaxed),
            "every isotropic run is one the GPU ran"
        );
        assert!(
            enough(&rows),
            "{} isotropic runs screened, {} confirming; {} turmite runs",
            count(&rows, Fam::Isotropic, 1),
            count(&rows, Fam::Isotropic, 2),
            count(&rows, Fam::Turmite, 1)
        );
        assert!(
            rows.iter()
                .all(|r| r.ev.cand.fam != Fam::Isotropic || r.ev.cand.tiling == "square")
        );
        // The loops' families: isotropic rules on the GPU, at 8, not on the processor.
        let (g, c, _) = families(&set.families, &set.skip_families, true);
        assert!(g.contains(&(Fam::Isotropic, 8.0)), "{g:?}");
        assert!(c.iter().all(|x| x.0 != Fam::Isotropic), "{c:?}");
        // With no GPU the processor's list is the one it was: isotropic rules after its own
        // families, at 8, then the GPU's.
        let (_, c, _) = families(&set.families, &set.skip_families, false);
        assert_eq!(
            c,
            vec![
                (Fam::Turmite, 25.0),
                (Fam::Isotropic, 8.0),
                (Fam::LifeLike, 30.0)
            ]
        );
    }

    /// A search records what it was told to draw from in its spec, and the spec read back makes
    /// the same runs: the same seed and choice give the same search (on the processor, with the
    /// same threads) whatever the length of its steps (#239).
    #[test]
    fn a_search_with_a_choice_replays_from_its_spec() {
        let set = choosing(serde_json::json!({
            "tiling_groups": ["square", "hexagon", "triangle"],
            "skip_tilings": ["brick"],
            "families": ["life_like", "isotropic", "cyclic"],
            "skip_families": ["cyclic"],
            "screen_gens": 160,
            "seed": 35,
        }));
        let v = serde_json::to_value(&set).unwrap();
        assert_eq!(
            v["tiling_groups"],
            serde_json::json!(["square", "hexagon", "triangle"])
        );
        assert_eq!(v["skip_tilings"], serde_json::json!(["brick"]));
        assert_eq!(
            v["families"],
            serde_json::json!(["life_like", "isotropic", "cyclic"])
        );
        assert_eq!(v["skip_families"], serde_json::json!(["cyclic"]));
        assert_eq!(v["schedule"], SCHEDULE);
        let back: Settings = serde_json::from_value(v).unwrap();
        assert_eq!(back, set);
        let a = runs_of(&set, 3, Duration::from_millis(5), 300);
        let b = runs_of(&back, 3, Duration::from_secs(30), 300);
        assert_eq!(a.len(), b.len());
        assert!(a == b, "the runs differ");
    }

    /// A choice that leaves nothing to search, or names a family that cannot run on the tilings
    /// chosen, is refused when the search is made; one a search made before 0.4.3 had is not.
    #[test]
    fn a_choice_that_leaves_nothing_to_search_is_refused() {
        for (extra, want) in [
            (
                serde_json::json!({"tiling_groups": ["nowhere"]}),
                "no tiling group 'nowhere'",
            ),
            (
                serde_json::json!({"skip_tilings": ["nowhere"]}),
                "no tiling 'nowhere'",
            ),
            (
                serde_json::json!({"tiling_groups": ["hexagon"], "families": ["isotropic", "life_like"]}),
                "isotropic rules run only on the square tiling",
            ),
            (
                serde_json::json!({"families": ["von_neumann"]}),
                "does not sample von_neumann rules",
            ),
            (
                serde_json::json!({"skip_tiling_groups": space::TILING_GROUPS}),
                "no tiling left to search",
            ),
            (
                serde_json::json!({"families": ["life_like"], "skip_families": ["life_like"]}),
                "no rule family to search",
            ),
            (
                serde_json::json!({"tilings": ["hex"], "families": ["wolfram"]}),
                "wolfram rules run only on the square tiling",
            ),
        ] {
            let e = choosing(extra.clone()).check().unwrap_err();
            assert!(e.contains(want), "{extra}: {e}");
        }
        // Not named, the families that cannot run on the tilings chosen are dropped quietly.
        let s = choosing(serde_json::json!({"tiling_groups": ["hexagon"]}));
        assert!(s.check().is_ok());
        assert!(!s.chosen_tilings(space::all_tilings()).is_empty());
        // A search made before kept every tiling, and still carries on.
        let mut old = serde_json::to_value(choosing(
            serde_json::json!({"tilings": ["hex", "square"], "families": ["wolfram"]}),
        ))
        .unwrap();
        old.as_object_mut().unwrap().remove("schedule");
        let old: Settings = serde_json::from_value(old).unwrap();
        assert_eq!(old.schedule, 1);
        let s = Search::new(old, Saved::default()).unwrap();
        assert_eq!(s.tiling_count(), 2);
    }

    /// A fingerprint of a search's schedule: its tiling list and named rules, then the
    /// candidates of its first `rounds` CPU rounds and as many GPU rounds (six each), made from
    /// an empty archive, so it depends only on the schedule and the random draws, not on what
    /// any run measures. A rule's notation is left out only for forest fire, whose numbers come
    /// from `powf`, which may differ in its last bit between systems.
    fn schedule_pin(set: &Settings, rounds: u64) -> u64 {
        schedule_pin_of(set, rounds, &[false, true])
    }

    /// [`schedule_pin`] with the rounds of these loops only: `false` the processor's (with no
    /// GPU, so it takes the GPU's families too), `true` the GPU's.
    fn schedule_pin_of(set: &Settings, rounds: u64, loops: &[bool]) -> u64 {
        let s = Search::new(set.clone(), Saved::default()).unwrap();
        let mut h = jetcarp_core::rng::Fnv::default();
        let text = |h: &mut jetcarp_core::rng::Fnv, t: &str| {
            h.bytes(t.as_bytes());
            h.bytes(&[0xff]);
        };
        h.u64(s.tilings.len() as u64);
        for t in &s.tilings {
            text(&mut h, &t.key);
        }
        h.u64(s.tiling_total.to_bits());
        h.u64(s.named.lock().unwrap().len() as u64);
        for &gpu in loops {
            let (g, c, ltl) = families(&set.families, &set.skip_families, gpu);
            let mut rng = Rng::new(set.seed ^ 0x5eed);
            // The loop runs only with families to run (as `step` has it).
            let some = if gpu {
                !g.is_empty() || ltl
            } else {
                !c.is_empty()
            };
            for round in (1..=rounds).filter(|_| some) {
                let (ti, fams, group) = if gpu {
                    let (l, ti, f) = s.gpu_plan(&mut rng, &g, ltl);
                    (ti, f, l.then_some((Hood::Moore, 2)))
                } else {
                    let (ti, f) = s.cpu_plan(round, &mut rng, &c);
                    (ti, f, None)
                };
                let grid = s.screen_grid(&s.tilings[ti].key).unwrap();
                let made = s.emit(
                    &grid,
                    Nb::Touch,
                    &fams,
                    group,
                    6,
                    set.screen_gens,
                    &mut rng,
                    None,
                );
                for (cand, kind, _) in made {
                    text(&mut h, &cand.tiling);
                    text(&mut h, cand.fam.name());
                    text(&mut h, &kind);
                    text(&mut h, &cand.origin);
                    h.u64(cand.rng);
                    h.u64(cand.gens as u64);
                    if cand.fam != Fam::ForestFire {
                        text(&mut h, &cand.rule);
                    }
                }
            }
        }
        h.0
    }

    /// A spec as JETCARP 0.4.2 wrote it (no `sizing`, no `schedule`, none of the new choices).
    fn old_spec(extra: serde_json::Value) -> Settings {
        let mut v = serde_json::json!({
            "screen_cells": 900, "screen_gens": 120, "confirm_cells": 900, "confirm_gens": 120,
            "confirm_seeds": 1, "batch": 1024, "seed": 36,
        });
        for (k, x) in extra.as_object().unwrap() {
            v[k] = x.clone();
        }
        let set: Settings = serde_json::from_value(v).unwrap();
        assert_eq!((set.sizing, set.schedule), (1, 1));
        set
    }

    /// An older search, and a search that chooses nothing, keep the schedule they had before
    /// tilings and families could be chosen (#239): the same tiling list, named rules and
    /// candidates, pinned here as JETCARP 0.4.2's code made them (with the plans of its CPU and
    /// GPU rounds copied out of it). A search made now that chooses nothing makes the same
    /// candidates as an older one; one that names only grid-specific families leaves out the
    /// tilings they cannot run on, which an older one keeps.
    #[test]
    fn an_old_spec_replays_as_before() {
        let cases = [
            (
                "every tiling and family",
                serde_json::json!({}),
                0x9bbe_8c05_0184_8d60u64,
            ),
            (
                "grid-specific families",
                serde_json::json!({"tilings": ["square", "hex", "cairo"], "families": ["wolfram", "margolus"]}),
                0x089d_b83f_1ff6_f10c,
            ),
            (
                "families that run anywhere",
                serde_json::json!({"families": ["life_like", "jaca", "turmite"]}),
                0xddf8_24f4_e637_1cb1,
            ),
        ];
        for (what, extra, want) in &cases {
            let old = old_spec(extra.clone());
            assert_eq!(schedule_pin(&old, 24), *want, "{what}");
            let now = Settings {
                schedule: SCHEDULE,
                ..old
            };
            let pin = schedule_pin(&now, 24);
            if what.starts_with("grid-specific") {
                assert_ne!(pin, *want, "{what}, made now");
                let s = Search::new(now, Saved::default()).unwrap();
                assert_eq!(s.tiling_count(), 1, "only the square grid, made now");
            } else {
                assert_eq!(pin, *want, "{what}, made now");
            }
        }
    }

    /// On the processor alone, a search that names isotropic rules draws as it did when only the
    /// processor ran them (#241), now that a GPU runs them when there is one (#245): its tiling
    /// list, named rules and the candidates of its first CPU rounds, pinned as that code made
    /// them. (With a GPU they move to the GPU's rounds; a search with a GPU never replayed, as
    /// its rounds share the archive as they end.)
    #[test]
    fn a_search_naming_isotropic_rules_draws_as_before_on_the_processor() {
        let cases = [
            (
                "isotropic rules alone",
                serde_json::json!({"families": ["isotropic"]}),
                0x37c2_b0e4_01c7_f2f9u64,
            ),
            (
                "beside families of the GPU and the processor",
                serde_json::json!({"families": ["life_like", "isotropic", "turmite", "wolfram"]}),
                0xf8ba_1fd0_93d3_bfd9,
            ),
            (
                "every family",
                serde_json::json!({"families": sampled_families()}),
                0x9cac_4967_9cc4_4b4a,
            ),
        ];
        let pins: Vec<u64> = cases
            .iter()
            .map(|(what, extra, _)| {
                let mut set = choosing(extra.clone());
                set.seed = 38;
                assert_eq!(set.schedule, SCHEDULE);
                let p = schedule_pin_of(&set, 24, &[false]);
                eprintln!("{what}: {p:#018x}");
                p
            })
            .collect();
        for ((what, _, want), pin) in cases.iter().zip(pins) {
            assert_eq!(pin, *want, "{what}");
        }
    }

    #[test]
    fn the_grid_cache_lets_the_least_recently_used_go() {
        let g = |key: &str| Arc::new(Grid::build(key, 100).unwrap());
        let (sq, hex, tri) = (g("square"), g("hex"), g("triangle"));
        let n = sq.topo.len() + hex.topo.len();
        let mut c = GridCache::new(n + tri.topo.len() - 1);
        let k = |s: &str| (s.to_string(), 100);
        c.insert(k("square"), sq.clone());
        c.insert(k("hex"), hex.clone());
        assert_eq!(c.cells, n);
        // The square grid is used again, so the hexagons are the oldest when a third comes.
        assert!(c.get(&k("square")).is_some());
        c.insert(k("triangle"), tri.clone());
        assert!(c.get(&k("hex")).is_none(), "the least recently used went");
        assert!(c.get(&k("square")).is_some() && c.get(&k("triangle")).is_some());
        assert_eq!(c.cells, sq.topo.len() + tri.topo.len());
        assert_eq!((c.hits, c.builds), (3, 3));
        // A grid built twice (two rounds at once) is kept once.
        let again = c.insert(k("square"), g("square"));
        assert!(Arc::ptr_eq(&again, &sq));
        assert_eq!(c.cells, sq.topo.len() + tri.topo.len());
        // One grid larger than the limit is still kept, alone.
        let mut small = GridCache::new(1);
        small.insert(k("square"), sq.clone());
        small.insert(k("hex"), hex);
        assert_eq!(small.map.len(), 1);
        assert!(small.get(&k("hex")).is_some());
    }
}
