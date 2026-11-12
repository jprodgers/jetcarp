//! Large sandpiles: N grains dropped on one spot of an unbounded grid, settled, and exported.
//!
//! **Doubling.** The pile is built from N's binary digits, most significant first:
//! `s ← stab(2s + bit · δ)`. By the abelian property `stab(2 · stab(x)) = stab(2x)`, so after the
//! last digit `s = stab(N · δ)`. Stable heights are below the degree `d`, so `2s + 1 ≤ 2d − 1`,
//! and a synchronous sweep keeps every height at or below `2d − 1` (a cell keeps at most `d − 1`
//! and receives at most one grain from each of its `d` neighbours). With `d ≤ 120` every height
//! fits in one byte, for any N up to `2^64 − 1`. Each step's pile is saved (see [`file`]), which
//! makes runs resumable and gives the pile at every power of two along the way.
//!
//! **Stopping in a step.** A step stopped part way (a cancel, or a job's time budget) keeps what
//! it has done: the board as its sweeps so far left it, with the grains already added, and the
//! step's counts so far ([`InStep`]). Saved, that is a checkpoint (`steps/step-KK.partial.pile`,
//! [`checkpoint_path`]), and the next [`Pile::next_step`] carries on toppling that board to
//! stability without adding grains again. By the abelian property the stable pile and the
//! toppling count are those of an uninterrupted step, and the synchronous sweeps carry on from
//! the same board, so the sweep count is the same too.
//!
//! **Boards.** Periodic tilings use [`lattice::LatticeBoard`], a box of lattice unit cells that
//! grows by a margin whenever an unstable cell nears its edge; nothing is ever lost. Aperiodic
//! patches use [`patch::PatchBoard`] on the exact patch up to level 8, with an error if a grain
//! reaches the patch's open edge.
//!
//! **Centre.** Grains go on one cell (default), or are split among the cells meeting at a vertex
//! or an edge midpoint of that cell: each of the `m` cells gets `⌊N/m⌋`, and the first `N mod m`
//! cells, in order of the angle of their centre around the point (atan2 from −180° up, ties by
//! cell coordinates), get one more. Doubling then runs on each cell's own count.
//!
//! **Toppling** is synchronous (Jacobi) sweeps on the CPU or, through [`Toppler`], on the GPU
//! (`jetcarp-gpu`). The final pile and the toppling count do not depend on the order (abelian
//! property); the sweep count is the same on every backend because every backend runs the same
//! synchronous sweeps.

pub mod export;
pub mod file;
pub mod lattice;
pub mod patch;

use crate::geom::*;
use crate::topology::{Nb, SizeSpec, Storage, TilingSpec, Topology};
use lattice::{Lattice, LatticeBoard};
use patch::PatchBoard;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

/// Where the grains go.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Centre {
    /// One cell (lattice cell (0, 0) of tile type 0, or the patch's centre tile).
    #[default]
    Cell,
    /// Split among the cells meeting at the first corner of that cell.
    Vertex,
    /// Split among the cells meeting at the midpoint of that cell's first edge.
    Edge,
}

impl Centre {
    pub fn parse(s: &str) -> Result<Centre, String> {
        match s.trim().to_ascii_lowercase().as_str() {
            "cell" | "tile" | "" => Ok(Centre::Cell),
            "vertex" | "corner" | "point" => Ok(Centre::Vertex),
            "edge" | "midpoint" | "edge_midpoint" | "edge-midpoint" => Ok(Centre::Edge),
            _ => Err(format!("centre must be cell, vertex, or edge, got '{s}'")),
        }
    }
    pub fn name(&self) -> &'static str {
        match self {
            Centre::Cell => "cell",
            Centre::Vertex => "vertex",
            Centre::Edge => "edge",
        }
    }
}

/// What to build.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PileSpec {
    /// The tiling (its size is ignored on periodic tilings; on aperiodic ones `Level(l)` picks
    /// the patch, else the smallest level that should hold the pile).
    pub tiling: TilingSpec,
    pub nb: Nb,
    #[serde(with = "crate::json_int::uint")]
    pub grains: u64,
    #[serde(default)]
    pub centre: Centre,
}

/// A cell of a board.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Site {
    Lattice { i: i64, j: i64, k: u32 },
    Patch { cell: u64 },
}

/// Grains destined for one cell.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Source {
    pub site: Site,
    #[serde(with = "crate::json_int::uint")]
    pub grains: u64,
}

/// One finished doubling step.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct StepRecord {
    /// 1-based: after step `k` the pile holds the first `k` binary digits of N.
    pub step: u32,
    #[serde(with = "crate::json_int::uint")]
    pub grains: u64,
    /// Beyond 2^53 on a pile of a billion grains or so: a decimal string in JSON then.
    #[serde(with = "crate::json_int::uint")]
    pub topplings: u64,
    pub sweeps: u64,
    pub seconds: f64,
    /// Width and height of the pile (nonzero cells) in lattice unit cells or world units on patches.
    pub width: f64,
    pub height: f64,
    /// Cells in the board (box or patch).
    pub board_cells: u64,
    pub backend: String,
}

/// Counts from one stabilisation.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct StabStats {
    pub topplings: u64,
    pub sweeps: u64,
    pub grows: u64,
    /// Stopped (cancelled) before the board was stable: the board is as the sweeps counted here
    /// left it, a valid start for carrying on.
    pub cancelled: bool,
}

/// A doubling step stopped part way: which step, and its counts so far. The board holds that
/// step's grains, part-toppled.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct InStep {
    /// The step (1-based), `done + 1`.
    pub step: u32,
    #[serde(with = "crate::json_int::uint")]
    pub topplings: u64,
    pub sweeps: u64,
    pub seconds: f64,
}

/// Progress inside a step, passed to [`Ctl::on_tick`].
#[derive(Clone, Copy, Debug, Default)]
pub struct Tick {
    pub sweeps: u64,
    pub topplings: u64,
    /// Seconds since the step's stabilisation started.
    pub seconds: f64,
}

/// Step context the host sees with each tick.
#[derive(Clone, Copy, Debug, Default)]
pub struct StepInfo {
    /// The step running (1-based) and the total.
    pub step: u32,
    pub steps: u32,
    pub grains: u64,
    /// What the step had done before this run of it (carried on from a checkpoint): the ticks
    /// count from there.
    pub carried: InStep,
}

/// A board borrowed for drawing.
#[derive(Clone, Copy)]
pub enum BoardRef<'a> {
    Lattice(&'a LatticeBoard),
    Patch(&'a PatchBoard),
}

impl<'a> BoardRef<'a> {
    /// The board as a job's live state (`jetcarp_search::jobs::Viewer`): the pile job shows its
    /// board this way each second and after each step.
    pub fn as_any(&self) -> &'a dyn std::any::Any {
        match *self {
            BoardRef::Lattice(b) => b,
            BoardRef::Patch(b) => b,
        }
    }
    /// A board shown as a job's live state, if that is what `state` is.
    pub fn from_any(state: &'a dyn std::any::Any) -> Option<BoardRef<'a>> {
        state
            .downcast_ref::<LatticeBoard>()
            .map(BoardRef::Lattice)
            .or_else(|| state.downcast_ref::<PatchBoard>().map(BoardRef::Patch))
    }
}

/// The progress callback: step, progress in the step, and the board (for drawing).
pub type OnTick<'a> = Box<dyn FnMut(&StepInfo, &Tick, BoardRef) + 'a>;

/// Cancel flag and progress callback for a run.
pub struct Ctl<'a> {
    pub cancel: Option<&'a AtomicBool>,
    /// Seconds between ticks.
    pub tick_every: f64,
    pub on_tick: Option<OnTick<'a>>,
    /// The tick draws the board: the GPU copies it back first (otherwise ticks may show the
    /// board as it was when the step's GPU run began).
    pub needs_board: bool,
    pub info: StepInfo,
}

impl Default for Ctl<'_> {
    fn default() -> Self {
        Ctl {
            cancel: None,
            tick_every: 0.5,
            on_tick: None,
            needs_board: false,
            info: StepInfo::default(),
        }
    }
}

impl<'a> Ctl<'a> {
    pub fn cancelled(&self) -> bool {
        self.cancel.is_some_and(|c| c.load(Ordering::Relaxed))
    }
    pub fn tick(&mut self, t: &Tick, b: BoardRef) {
        let info = self.info;
        let t = Tick {
            sweeps: t.sweeps + info.carried.sweeps,
            topplings: t.topplings + info.carried.topplings,
            seconds: t.seconds + info.carried.seconds,
        };
        if let Some(f) = self.on_tick.as_mut() {
            f(&info, &t, b);
        }
    }
}

/// Another way to topple a lattice board (the GPU). It must give the same pile, toppling count,
/// and sweep count as [`LatticeBoard::stabilize`], and on a cancel return as it does: the counts
/// so far with `cancelled` set, and the board (on the CPU) as those sweeps left it.
pub trait Toppler {
    fn name(&self) -> String;
    fn stabilize(&mut self, board: &mut LatticeBoard, ctl: &mut Ctl) -> Result<StabStats, String>;
}

/// Makes a GPU toppler. Hosts with a GPU (the CLI, the app) register one at start-up with
/// [`set_gpu_factory`], so scripts can ask for `gpu: true` without depending on `jetcarp-gpu`.
pub type TopplerFactory = fn() -> Result<Box<dyn Toppler>, String>;

static GPU_FACTORY: std::sync::OnceLock<TopplerFactory> = std::sync::OnceLock::new();

/// Register the host's GPU toppler (first call wins).
pub fn set_gpu_factory(f: TopplerFactory) {
    let _ = GPU_FACTORY.set(f);
}

/// A GPU toppler from the registered factory: `None` when the host registered none.
pub fn gpu_toppler() -> Option<Result<Box<dyn Toppler>, String>> {
    GPU_FACTORY.get().map(|f| f())
}

pub enum Board {
    Lattice(LatticeBoard),
    Patch(PatchBoard),
}

impl Board {
    pub fn as_ref(&self) -> BoardRef<'_> {
        match self {
            Board::Lattice(b) => BoardRef::Lattice(b),
            Board::Patch(b) => BoardRef::Patch(b),
        }
    }
    pub fn grains(&self) -> u64 {
        match self {
            Board::Lattice(b) => b.grains(),
            Board::Patch(b) => b.grains(),
        }
    }
    pub fn cells(&self) -> u64 {
        match self {
            Board::Lattice(b) => b.cells(),
            Board::Patch(b) => b.cells(),
        }
    }
    /// Bytes of working memory (heights and the back buffer).
    pub fn bytes(&self) -> u64 {
        match self {
            Board::Lattice(b) => b.bytes(),
            Board::Patch(b) => b.bytes(),
        }
    }
    fn add(&mut self, s: Site, n: u8) {
        match (self, s) {
            (Board::Lattice(b), Site::Lattice { i, j, k }) => b.add(i, j, k as usize, n),
            (Board::Patch(b), Site::Patch { cell }) => b.add(cell as usize, n),
            _ => unreachable!("site kind matches board kind"),
        }
    }
}

/// A pile being built.
pub struct Pile {
    pub spec: PileSpec,
    pub board: Board,
    pub sources: Vec<Source>,
    /// Doubling steps in all (bits of the largest source count).
    pub steps: u32,
    /// Steps finished.
    pub done: u32,
    pub records: Vec<StepRecord>,
    /// The step stopped part way, if one was: the board holds its grains, part-toppled.
    pub in_step: Option<InStep>,
}

/// A short name for a tiling: the family id, or the catalog id of a catalog tiling.
pub fn tiling_label(t: &TilingSpec) -> String {
    t.params
        .as_ref()
        .and_then(|p| p.get("catalog"))
        .and_then(|c| c.as_str())
        .map(str::to_string)
        .unwrap_or_else(|| t.family.clone())
}

/// Is the tiling aperiodic (built as an explicit patch)?
pub fn is_patch(spec: &TilingSpec) -> bool {
    crate::families::family(&spec.family)
        .is_some_and(|f| f.kind == crate::families::FamilyKind::Aperiodic)
}

/// The smallest patch level that should hold `grains` grains (or the largest level).
pub fn auto_level(family: &str, grains: u64) -> u32 {
    let (lo, hi) = crate::aperiodic::level_range(family);
    let need = grains as f64 * 2.5 + 2000.0;
    (lo.max(2)..=hi)
        .find(|&l| crate::aperiodic::tile_count(family, l).is_some_and(|n| n as f64 >= need))
        .unwrap_or(hi)
}

fn seg_dist(p: P, a: P, b: P) -> f64 {
    let ab = sub(b, a);
    let t = (dot(sub(p, a), ab) / dot(ab, ab).max(1e-300)).clamp(0.0, 1.0);
    dist(p, add(a, scale(ab, t)))
}

fn on_boundary(p: P, poly: &[P], eps: f64) -> bool {
    (0..poly.len()).any(|k| seg_dist(p, poly[k], poly[(k + 1) % poly.len()]) < eps)
}

/// Split `n` grains among cells meeting at `p`: sorted by the angle of their centre around `p`.
fn split<T: Copy>(p: P, mut cells: Vec<(T, P, (i64, i64, i64))>, n: u64) -> Vec<(T, u64)> {
    cells.sort_by(|a, b| {
        let aa = crate::detmath::atan2(a.1[1] - p[1], a.1[0] - p[0]);
        let ab = crate::detmath::atan2(b.1[1] - p[1], b.1[0] - p[0]);
        aa.partial_cmp(&ab).unwrap().then(a.2.cmp(&b.2))
    });
    let m = cells.len() as u64;
    cells
        .iter()
        .enumerate()
        .map(|(r, c)| (c.0, n / m + ((r as u64) < n % m) as u64))
        .collect()
}

impl Pile {
    pub fn new(spec: PileSpec) -> Result<Pile, String> {
        let (board, sources) = if is_patch(&spec.tiling) {
            let level = match spec.tiling.size {
                Some(SizeSpec::Level(l)) => l,
                _ => auto_level(&spec.tiling.family, spec.grains),
            };
            let mut ts = spec.tiling.clone();
            ts.size = Some(SizeSpec::Level(level));
            let topo = Arc::new(Topology::build(&ts)?);
            let sources = patch_sources(&topo, spec.nb, spec.centre, spec.grains)?;
            (
                Board::Patch(PatchBoard::new(topo, spec.nb, level)?),
                sources,
            )
        } else {
            let lat = Arc::new(Lattice::new(&spec.tiling, spec.nb)?);
            let sources = lattice_sources(&lat, spec.centre, spec.grains);
            (Board::Lattice(LatticeBoard::new(lat)), sources)
        };
        let steps = sources
            .iter()
            .map(|s| 64 - s.grains.leading_zeros())
            .max()
            .unwrap_or(0);
        Ok(Pile {
            spec,
            board,
            sources,
            steps,
            done: 0,
            records: Vec::new(),
            in_step: None,
        })
    }

    /// Grains in the pile after `k` steps.
    pub fn grains_after(&self, k: u32) -> u64 {
        let shift = self.steps - k.min(self.steps);
        self.sources
            .iter()
            .map(|s| s.grains.checked_shr(shift).unwrap_or(0))
            .sum()
    }
    pub fn finished(&self) -> bool {
        self.done >= self.steps
    }
    /// Total toppling count, sweeps, and seconds over the finished steps.
    pub fn totals(&self) -> (u64, u64, f64) {
        self.records.iter().fold((0, 0, 0.0), |a, r| {
            (a.0 + r.topplings, a.1 + r.sweeps, a.2 + r.seconds)
        })
    }

    /// Run the next doubling step, or carry on the one stopped part way ([`Pile::in_step`]). A
    /// cancelled step returns the error `cancelled` and leaves the board part-toppled, with
    /// `in_step` holding its counts so far (`done` unchanged): saved ([`file::save`]), that is a
    /// checkpoint, and the next call carries on from it.
    pub fn next_step(
        &mut self,
        toppler: Option<&mut dyn Toppler>,
        ctl: &mut Ctl,
    ) -> Result<StepRecord, String> {
        if self.finished() {
            return Err("the pile is finished".into());
        }
        let t0 = Instant::now();
        let carried = match self.in_step.take() {
            // The board holds this step's grains already.
            Some(x) if x.step == self.done + 1 => x,
            Some(x) => {
                return Err(format!(
                    "the pile stopped in step {}, but {} steps are done",
                    x.step, self.done
                ));
            }
            None => {
                let shift = self.steps - self.done - 1;
                match &mut self.board {
                    Board::Lattice(b) => b.double(),
                    Board::Patch(b) => b.double(),
                }
                for s in self.sources.clone() {
                    if (s.grains >> shift) & 1 == 1 {
                        self.board.add(s.site, 1);
                    }
                }
                InStep {
                    step: self.done + 1,
                    ..Default::default()
                }
            }
        };
        let grains = self.grains_after(self.done + 1);
        ctl.info = StepInfo {
            step: self.done + 1,
            steps: self.steps,
            grains,
            carried,
        };
        let (st, backend) = match &mut self.board {
            Board::Lattice(b) => {
                b.sync();
                match toppler {
                    Some(t) => (t.stabilize(b, ctl)?, t.name()),
                    None => (b.stabilize(ctl)?, "cpu".to_string()),
                }
            }
            Board::Patch(b) => {
                b.sync();
                (b.stabilize(ctl)?, "cpu".to_string())
            }
        };
        let have = self.board.grains();
        if have != grains {
            return Err(format!(
                "grain count check failed after step {}: {have} on the board, expected {grains}",
                self.done + 1
            ));
        }
        let (topplings, sweeps, seconds) = (
            carried.topplings + st.topplings,
            carried.sweeps + st.sweeps,
            carried.seconds + t0.elapsed().as_secs_f64(),
        );
        if st.cancelled {
            self.in_step = Some(InStep {
                step: self.done + 1,
                topplings,
                sweeps,
                seconds,
            });
            return Err("cancelled".into());
        }
        let (width, height) = self.extent();
        let rec = StepRecord {
            step: self.done + 1,
            grains,
            topplings,
            sweeps,
            seconds,
            width,
            height,
            board_cells: self.board.cells(),
            backend,
        };
        self.done += 1;
        self.records.push(rec.clone());
        Ok(rec)
    }

    /// Width and height of the nonzero cells' centres' bounding box, in world units.
    pub fn extent(&self) -> (f64, f64) {
        match export::world_bbox(&self.board.as_ref(), 0.0) {
            Some(b) => (b[2] - b[0], b[3] - b[1]),
            None => (0.0, 0.0),
        }
    }
}

/// Sources on a lattice: the origin cell, or the cells meeting at its first corner or edge midpoint.
pub fn lattice_sources(lat: &Lattice, centre: Centre, n: u64) -> Vec<Source> {
    let site = |i, j, k| Site::Lattice { i, j, k };
    let poly = lat.polygon(0, 0, 0);
    let p = match centre {
        Centre::Cell => {
            return vec![Source {
                site: site(0, 0, 0),
                grains: n,
            }];
        }
        Centre::Vertex => poly[0],
        Centre::Edge => scale(add(poly[0], poly[1]), 0.5),
    };
    let eps = lat.cell_area().sqrt() * 1e-6;
    let mut cells = Vec::new();
    let r = lat.reach as i64 + 1;
    for j in -r..=r {
        for i in -r..=r {
            for k in 0..lat.nt {
                let q = lat.polygon(i, j, k);
                if on_boundary(p, &q, eps) {
                    cells.push(((i, j, k as u32), centroid(&q), (j, i, k as i64)));
                }
            }
        }
    }
    split(p, cells, n)
        .into_iter()
        .map(|((i, j, k), g)| Source {
            site: site(i, j, k),
            grains: g,
        })
        .collect()
}

/// Sources on a patch: the centre tile, or the tiles meeting at its first corner or edge midpoint.
pub fn patch_sources(
    topo: &Topology,
    nb: Nb,
    centre: Centre,
    n: u64,
) -> Result<Vec<Source>, String> {
    let _ = nb;
    let c = topo.center_cell();
    let poly = topo.polygon(c);
    let p = match centre {
        Centre::Cell => {
            return Ok(vec![Source {
                site: Site::Patch { cell: c as u64 },
                grains: n,
            }]);
        }
        Centre::Vertex => poly[0],
        Centre::Edge => scale(add(poly[0], poly[1]), 0.5),
    };
    let eps = area(&poly).sqrt() * 1e-6;
    let mut cand = vec![c];
    topo.for_each_neighbor(c, Nb::Touch, |j| cand.push(j));
    let mut cells = Vec::new();
    for &i in &cand {
        let q = topo.polygon(i);
        if on_boundary(p, &q, eps) {
            cells.push((i, centroid(&q), (i as i64, 0, 0)));
        }
    }
    if cells.is_empty() {
        return Err("no tiles meet at the chosen point".into());
    }
    if let Storage::Explicit(e) = &topo.storage
        && cells.iter().any(|c| e.open[c.0])
    {
        return Err("the centre lies on the patch's open edge".into());
    }
    Ok(split(p, cells, n)
        .into_iter()
        .map(|(i, g)| Source {
            site: Site::Patch { cell: i as u64 },
            grains: g,
        })
        .collect())
}

/// Options for [`run_to_dir`].
#[derive(Clone, Debug)]
pub struct RunOptions {
    pub export: export::ExportOptions,
    /// Save each step's pile to `steps/`.
    pub save_steps: bool,
    /// Extra metadata merged into `pile.json` (host, date, command line).
    pub extra: serde_json::Value,
}

impl Default for RunOptions {
    fn default() -> Self {
        RunOptions {
            export: export::ExportOptions::default(),
            save_steps: true,
            extra: serde_json::Value::Null,
        }
    }
}

/// What a finished run wrote.
#[derive(Clone, Debug, Serialize)]
pub struct RunSummary {
    pub dir: PathBuf,
    pub png: Option<PathBuf>,
    pub preview: Option<PathBuf>,
    pub image_size: Option<(u32, u32)>,
    pub metadata: serde_json::Value,
}

/// A finished step's file: `dir/steps/step-KK.pile`.
pub fn step_path(dir: &Path, step: u32) -> PathBuf {
    dir.join(format!("steps/step-{step:02}.pile"))
}

/// The checkpoint of a step stopped part way: `dir/steps/step-KK.partial.pile`. It sorts before
/// the step's finished file and after the step before's, so [`latest_step_file`] finds whichever
/// is newer.
pub fn checkpoint_path(dir: &Path, step: u32) -> PathBuf {
    dir.join(format!("steps/step-{step:02}.partial.pile"))
}

/// Save the pile into its folder: a finished step's file (the step's checkpoint, if any, then
/// removed), or the checkpoint of the step stopped part way. Returns the file written.
pub fn save_to_dir(pile: &Pile, dir: &Path) -> Result<PathBuf, String> {
    std::fs::create_dir_all(dir.join("steps"))
        .map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
    match pile.in_step {
        Some(x) => {
            let p = checkpoint_path(dir, x.step);
            file::save(pile, &p)?;
            Ok(p)
        }
        None => {
            let p = step_path(dir, pile.done);
            file::save(pile, &p)?;
            let _ = std::fs::remove_file(checkpoint_path(dir, pile.done));
            Ok(p)
        }
    }
}

/// The latest step file in `dir/steps` (a step's checkpoint among them), if any.
pub fn latest_step_file(dir: &Path) -> Option<PathBuf> {
    let mut v: Vec<PathBuf> = std::fs::read_dir(dir.join("steps"))
        .ok()?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "pile"))
        .collect();
    v.sort();
    v.pop()
}

/// Run the remaining steps, saving each step to `dir/steps/step-KK.pile` and the growth table to
/// `dir/growth.csv`, then export `pile.png`, `preview.png`, and `pile.json`.
pub fn run_to_dir(
    pile: &mut Pile,
    dir: &Path,
    mut toppler: Option<&mut dyn Toppler>,
    ctl: &mut Ctl,
    opts: &RunOptions,
) -> Result<RunSummary, String> {
    std::fs::create_dir_all(dir.join("steps"))
        .map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
    while !pile.finished() {
        let r = match toppler.as_mut() {
            Some(t) => pile.next_step(Some(&mut **t), ctl),
            None => pile.next_step(None, ctl),
        };
        if let Err(e) = r {
            // A step stopped part way keeps its work in a checkpoint.
            if pile.in_step.is_some() {
                save_to_dir(pile, dir)?;
            }
            return Err(e);
        }
        if opts.save_steps {
            save_to_dir(pile, dir)?;
        }
        write_growth(pile, dir)?;
        let meta = export::metadata(pile, opts, None);
        write_json(&dir.join("pile.json"), &meta)?;
    }
    let last = step_path(dir, pile.done);
    if !opts.save_steps && pile.done > 0 && !last.exists() {
        save_to_dir(pile, dir)?;
    }
    write_growth(pile, dir)?;
    export::finish(pile, dir, opts)
}

/// Write JSON, an integer beyond 2^53 − 1 in it (a pile's topplings) as a decimal string.
pub(crate) fn write_json(p: &Path, v: &serde_json::Value) -> Result<(), String> {
    let v = crate::json_int::safe(v.clone());
    std::fs::write(p, serde_json::to_string_pretty(&v).unwrap())
        .map_err(|e| format!("{}: {e}", p.display()))
}

/// Write `dir/growth.csv`: one row per finished step.
pub fn write_growth(pile: &Pile, dir: &Path) -> Result<(), String> {
    let mut s =
        String::from("step,grains,topplings,sweeps,seconds,width,height,board_cells,backend\n");
    for r in &pile.records {
        s += &format!(
            "{},{},{},{},{:.3},{:.3},{:.3},{},{}\n",
            r.step,
            r.grains,
            r.topplings,
            r.sweeps,
            r.seconds,
            r.width,
            r.height,
            r.board_cells,
            r.backend
        );
    }
    let p = dir.join("growth.csv");
    std::fs::write(&p, s).map_err(|e| format!("{}: {e}", p.display()))
}

/// Grain counts as people write them: `10000000`, `1e7`, `2.5e6`, `2^30`, `1_000_000`.
pub fn parse_grains(s: &str) -> Result<u64, String> {
    let t: String = s.trim().chars().filter(|&c| c != '_' && c != ',').collect();
    let bad = || format!("'{s}' is not a grain count (try 1000000, 1e7, or 2^30)");
    if let Ok(n) = t.parse::<u64>() {
        return Ok(n);
    }
    if let Some((b, e)) = t.split_once('^') {
        let b: u64 = b.parse().map_err(|_| bad())?;
        let e: u32 = e.parse().map_err(|_| bad())?;
        return b
            .checked_pow(e)
            .ok_or_else(|| format!("{s} is larger than 2^64 − 1"));
    }
    let low = t.to_ascii_lowercase();
    if let Some((m, e)) = low.split_once('e') {
        // Exact for integer results: the mantissa's digits times a power of ten.
        let e: i32 = e.parse().map_err(|_| bad())?;
        let (int, frac) = m.split_once('.').unwrap_or((m, ""));
        let digits: u64 = format!("{int}{frac}").parse().map_err(|_| bad())?;
        let p = e - frac.len() as i32;
        if p < 0 {
            let d = 10u64.checked_pow((-p) as u32).ok_or_else(bad)?;
            if !digits.is_multiple_of(d) {
                return Err(bad());
            }
            return Ok(digits / d);
        }
        return 10u64
            .checked_pow(p as u32)
            .and_then(|x| x.checked_mul(digits))
            .ok_or_else(|| format!("{s} is larger than 2^64 − 1"));
    }
    Err(bad())
}

/// Seconds left: the running step is predicted from the previous step's time times the ratio of
/// the last two step times (clamped to 1–8), and each later step the same ratio again.
/// `elapsed` is the time spent in the running step so far.
pub fn eta_seconds(records: &[StepRecord], steps: u32, elapsed: f64) -> Option<f64> {
    let n = records.len();
    if n < 2 {
        return None;
    }
    let (a, b) = (records[n - 2].seconds, records[n - 1].seconds);
    let ratio = if a > 0.05 {
        (b / a).clamp(1.0, 8.0)
    } else {
        4.0
    };
    let left = steps.saturating_sub(n as u32);
    let mut t = b;
    let mut total = 0.0;
    for k in 0..left {
        t *= ratio;
        total += if k == 0 { (t - elapsed).max(0.0) } else { t };
    }
    Some(total)
}

/// A duration as `42.0 s`, `3.5 min`, `2.1 h`, or `1.5 days`.
pub fn fmt_seconds(s: f64) -> String {
    if s < 90.0 {
        format!("{s:.1} s")
    } else if s < 5400.0 {
        format!("{:.1} min", s / 60.0)
    } else if s < 172_800.0 {
        format!("{:.1} h", s / 3600.0)
    } else {
        format!("{:.1} days", s / 86400.0)
    }
}
