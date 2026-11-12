//! `jetcarp search <strategy>`: start (or carry on) a search as a catalog job, through the
//! operation `search.start`. Each strategy keeps the flags of the command it replaces (`hunt`,
//! `library search`, `ants search`, `census`, `movers track --window --job`, `pile`, `library
//! collide`, `evolve`, `explore run`); `jetcarp pile` is the same as `jetcarp search pile`. The
//! job runs here until it ends, Ctrl-C stops it at its next checkpoint (resumable with `jetcarp
//! jobs resume JOB`), or its budget (`--minutes`, `--until`, `--limit`) runs out; `--detach` runs
//! it in a process of its own. `jetcarp search try` runs one configuration now, measured as the
//! explore search measures its runs (`search.try`; not a job).

use crate::show;
use anyhow::{Context, Result, bail};
use jetcarp_api::op::{Op, Progress};
use jetcarp_api::ops::explore::{SearchTry, TryIn};
use jetcarp_api::ops::search::{
    AntsIn, BoundedIn, BudgetIn, CensusIn, CollideIn, EvolveIn, ExploreIn, FilesIn, HuntIn,
    MoverIn, PileIn, SearchStart, SearchStartIn, SearchStarted,
};
use serde::Serialize;
use serde_json::{Value, json};
use std::io::{IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

#[derive(clap::Args)]
pub struct SearchArgs {
    #[command(subcommand)]
    strategy: Strategy,
}

#[derive(clap::Subcommand)]
enum Strategy {
    /// Fast soup hunter: small soups on an unbounded plane, bit-parallel, until they settle;
    /// every object classed, new patterns verified and cataloged.
    #[command(visible_alias = "soup")]
    Hunt(Box<HuntArgs>),
    /// Bounded soup search: soups on a finite grid of any tiling (aperiodic patches too); every
    /// verified still life, oscillator, and spaceship cataloged.
    Bounded(Box<BoundedArgs>),
    /// Ant and turmite search: turn strings and turmite tables on unbounded periodic tilings,
    /// highways proved exactly, every run classed and cataloged.
    Ants(Box<AntsArgs>),
    /// Rule census: rules run from seeded soups, each run classed (CSV, JSON, contact sheet).
    Census(Box<CensusArgs>),
    /// A mover on an aperiodic tiling, followed on a sliding window of supertiles as far as
    /// --rings or --gens allow, verified to a distance (`jetcarp library movers track`: fixed
    /// patch levels).
    #[command(visible_alias = "movers")]
    Mover(Box<MoverArgs>),
    /// Large sandpile: N grains on one spot of an unbounded grid, built by doubling, with a
    /// cropped PNG, a preview, metadata, and a resumable pile file per step.
    Pile(Box<PileArgs>),
    /// Collide pattern B into pattern A at every offset and phase in a range.
    Collide(Box<CollideArgs>),
    /// Evolutionary glider search (Edmond's method) on any tiling; verified movers go to the
    /// library.
    Evolve(Box<EvolveArgs>),
    /// The broad search over tilings, rule families, rules, and seeds (MAP-Elites): runs until
    /// stopped or out of budget, every run kept in the job's own folder (`searches/<uuid>/` beside
    /// the library: Parquet segments of a million runs, and a working database). Not
    /// deterministic: a stopped search continues from its archive.
    /// `jetcarp jobs runs|sheet|scene|review|clips|rescore|seal` read it.
    Explore(Box<ExploreArgs>),
    /// Run one configuration now on the CPU, measured as the explore search measures its runs:
    /// fate, measures, score, and a picture (not a job).
    Try(Box<TryArgs>),
}

/// What every search takes.
#[derive(clap::Args)]
pub struct JobFlags {
    /// Catalog: a .sqlite file, or a folder holding library.sqlite (default: $JETCARP_LIBRARY,
    /// else <data>/library.sqlite).
    #[arg(long)]
    library: Option<PathBuf>,
    /// A name to find the job by (`jetcarp jobs list --label`).
    #[arg(long)]
    label: Option<String>,
    /// Carry on this job instead of starting one (its spec is kept; the other options are
    /// ignored). Piles: a job id or the pile's run folder.
    #[arg(long, value_name = "JOB")]
    resume: Option<String>,
    /// Stop at the next checkpoint after this many minutes; the job stays resumable.
    #[arg(long)]
    minutes: Option<f64>,
    /// Stop at the next checkpoint after this Unix time (seconds since 1970): the same end
    /// however often the job is resumed.
    #[arg(long)]
    until: Option<u64>,
    /// Stop after this many units in this session (soups per rule and tiling, runs, rules,
    /// generations); the job stays resumable.
    #[arg(long)]
    limit: Option<u64>,
    /// Worker threads (default: all cores). Results do not depend on this.
    #[arg(long)]
    threads: Option<usize>,
    /// Run in a process of its own and return at once (`jetcarp jobs status JOB` follows it).
    #[arg(long)]
    detach: bool,
    /// No progress on stderr.
    #[arg(long)]
    quiet: bool,
}

#[derive(clap::Args)]
pub struct HuntArgs {
    #[command(flatten)]
    job: JobFlags,
    /// Rules, separated by spaces or semicolons (B/S and Generations rules without B0; a `V`
    /// suffix means edge neighbours).
    #[arg(long, default_value = "B3/S23")]
    rules: String,
    /// More rules, one per line.
    #[arg(long)]
    rules_file: Option<PathBuf>,
    /// Tilings: family ids, catalog:<id>, or the sets all-periodic, archimedean, kuniform:<k>,
    /// isohedral.
    #[arg(long, default_value = "square")]
    tilings: String,
    /// Neighbourhood for every rule: touch or edge (default: the notation decides).
    #[arg(long)]
    nb: Option<String>,
    /// Soups per rule and tiling.
    #[arg(long, default_value_t = 100_000)]
    soups: u64,
    /// Run until stopped (Ctrl-C, `jetcarp jobs stop JOB`, or the MCP tool job_stop).
    #[arg(long)]
    forever: bool,
    /// Soup size in unit cells, WxH.
    #[arg(long, default_value = "16x16")]
    size: String,
    #[arg(long, default_value_t = 0.5)]
    density: f64,
    /// rect or disc.
    #[arg(long, default_value = "rect")]
    shape: String,
    #[arg(long, default_value_t = 1)]
    seed: u64,
    /// Generations before a soup that has not settled is censused as it is.
    #[arg(long, default_value_t = 20_000)]
    gens: u64,
    /// Longest period looked for.
    #[arg(long, default_value_t = 64)]
    max_period: u64,
    /// Live cells above which a soup counts as exploded (0: automatic).
    #[arg(long, default_value_t = 0)]
    pop_cap: u64,
    /// Empty margin around the soup before escaping ships are cut (0: automatic).
    #[arg(long, default_value_t = 0)]
    margin: u32,
    /// Storage policy: still lifes and oscillators with more cells than this are kept as
    /// unstored rows (no full record) until seen --store-min-seen times.
    #[arg(long, default_value_t = 64)]
    store_max_cells: u64,
    /// ... or with period x cells above this.
    #[arg(long, default_value_t = 4000)]
    store_max_volume: u64,
    /// Copies in this hunt after which such an object is stored anyway (0: never).
    #[arg(long, default_value_t = 2)]
    store_min_seen: u64,
    /// Store every new object as a full record (the policy before wave 4).
    #[arg(long)]
    store_all: bool,
    /// Run the start of each soup on the GPU (many soups per dispatch; results are identical).
    #[arg(long)]
    gpu: bool,
}

#[derive(clap::Args)]
pub struct BoundedArgs {
    #[command(flatten)]
    job: JobFlags,
    /// Rules separated by spaces or semicolons ("B3/S23 B36/S23").
    #[arg(long, alias = "rule")]
    rules: Option<String>,
    /// File with one rule per line (# comments).
    #[arg(long)]
    rules_file: Option<PathBuf>,
    /// Tilings: family ids, catalog:<id>, or the sets all-periodic, archimedean, kuniform:<k>,
    /// isohedral; comma separated.
    #[arg(long, alias = "tiling", default_value = "square")]
    tilings: String,
    /// WxH unit cells (64x64), about N cells (4000), or L<level> for aperiodic patches.
    #[arg(long, default_value = "64x64")]
    size: String,
    /// wrap or dead.
    #[arg(long, default_value = "wrap")]
    boundary: String,
    /// Neighbourhood for every rule: touch or edge (default: per rule; V suffix = edge).
    #[arg(long)]
    nb: Option<String>,
    #[arg(long, default_value_t = 16)]
    soups: u32,
    #[arg(long, default_value_t = 0.37)]
    density: f64,
    #[arg(long, default_value_t = 2000)]
    gens: u64,
    #[arg(long, default_value_t = 1)]
    seed: u64,
    #[arg(long, default_value_t = 64)]
    max_period: u64,
    /// Soup only a disc at the centre: radius as a fraction of half the shorter side (e.g. 0.15).
    /// With --boundary dead, escaping spaceships are caught at the edge and cataloged.
    #[arg(long)]
    soup_radius: Option<f64>,
    /// Write the full report as JSON.
    #[arg(long)]
    json_out: Option<PathBuf>,
    /// Report only; do not add to the library.
    #[arg(long)]
    no_store: bool,
    /// Rows per group in the printed report.
    #[arg(long, default_value_t = 12)]
    top: usize,
}

#[derive(clap::Args)]
pub struct AntsArgs {
    #[command(flatten)]
    job: JobFlags,
    /// Tilings: family ids, catalog ids, vertex configurations, or sets (archimedean,
    /// kuniform:2, isohedral, all-periodic).
    #[arg(long, default_value = "square")]
    tilings: String,
    /// Turn string lengths, `2-6` or `4`.
    #[arg(long)]
    strings: Option<String>,
    /// Turmite tables STATESxCOLORS, e.g. `2x2`.
    #[arg(long)]
    tables: Option<String>,
    /// Turn letters to search (default: the tiling's: L R N U on squares, L R N U L2 R2 on
    /// hexagons, L R N on triangles).
    #[arg(long)]
    alphabet: Option<String>,
    /// Tables: enumerate every table up to this many, else sample --sample of them.
    #[arg(long, default_value_t = 1_000_000)]
    all_max: u64,
    #[arg(long, default_value_t = 10_000)]
    sample: u64,
    /// Explicit rules, separated by spaces or semicolons (`RL`, `LLRR`, `RL2N`,
    /// `1R1,1L0/1R1,0N0`).
    #[arg(long)]
    rules: Option<String>,
    /// Soups per rule (default: an empty field).
    #[arg(long)]
    soups: Option<u64>,
    /// Soup size in unit cells, WxH.
    #[arg(long, default_value = "8x8")]
    size: String,
    #[arg(long, default_value_t = 0.5)]
    density: f64,
    #[arg(long, default_value_t = 1)]
    seed: u64,
    /// Steps before a run is classed as it is.
    #[arg(long, default_value_t = 1_000_000)]
    steps: u64,
    /// Longest highway or orbit period looked for.
    #[arg(long, default_value_t = 20_000)]
    max_period: u64,
    /// Skip the mirror-symmetry test.
    #[arg(long)]
    no_symmetry: bool,
}

#[derive(clap::Args)]
pub struct CensusArgs {
    #[command(flatten)]
    job: JobFlags,
    /// Tiling family (see `jetcarp tilings`).
    #[arg(long, default_value = "square")]
    family: String,
    /// WxH unit cells (64x64), about N cells (4000), or L<level> for aperiodic tilings (L4).
    #[arg(long, default_value = "64x64")]
    size: String,
    /// wrap, dead, or live.
    #[arg(long, default_value = "wrap")]
    boundary: String,
    /// Take the tiling from a scene or tile JSON file instead of --family/--size/--boundary.
    #[arg(long)]
    tiling: Option<PathBuf>,
    /// Neighbourhood for every rule: touch or edge (default: per rule; a V suffix means edge).
    #[arg(long)]
    nb: Option<String>,
    /// Rules separated by spaces or semicolons, e.g. "B3/S23 B36/S23 B2/S/C3".
    #[arg(long)]
    rules: Option<String>,
    /// File with one rule per line (# comments).
    #[arg(long)]
    rules_file: Option<PathBuf>,
    /// Every B/S rule whose digits come from --birth and --survive.
    #[arg(long)]
    all: bool,
    /// A deterministic random sample of N distinct B/S rules over --birth and --survive.
    #[arg(long)]
    sample: Option<usize>,
    #[arg(long, default_value_t = 1)]
    sample_seed: u64,
    /// Birth digits for --all/--sample, like 1-8 or 1,2,3,6 ("max" = the tiling's largest degree).
    #[arg(long, default_value = "1-max")]
    birth: String,
    /// Survival digits for --all/--sample.
    #[arg(long, default_value = "0-max")]
    survive: String,
    /// Generations rules with this many states (for --all/--sample).
    #[arg(long)]
    states: Option<u8>,
    /// Soups per rule (soup s uses RNG state seed + s).
    #[arg(long, default_value_t = 8)]
    soups: u32,
    #[arg(long, default_value_t = 0.37)]
    density: f64,
    /// Generation limit per run.
    #[arg(long, default_value_t = 2000)]
    gens: u64,
    #[arg(long, default_value_t = 1)]
    seed: u64,
    /// Stop a run and call it exploded once its population exceeds this fraction of the cells.
    #[arg(long, default_value_t = 0.5)]
    explode: f64,
    /// Write per-rule summaries as CSV.
    #[arg(long)]
    csv: Option<PathBuf>,
    /// Write the full report (spec, seeds, every run) as JSON.
    #[arg(long)]
    json_out: Option<PathBuf>,
    /// Labelled contact sheet of each rule's first soup at the end of its run.
    #[arg(long)]
    sheet: Option<PathBuf>,
    #[arg(long, default_value_t = 8)]
    sheet_cols: u32,
    /// Thumbnail size in pixels for --sheet.
    #[arg(long, default_value_t = 160)]
    thumb: u32,
    /// Rerun the census recorded in a JSON report (its spec replaces the tiling and rule flags).
    #[arg(long)]
    rerun: Option<PathBuf>,
}

#[derive(clap::Args)]
pub struct MoverArgs {
    #[command(flatten)]
    job: JobFlags,
    /// Rule notation (SpectreGlider, "Priority 4: ...", B/S).
    #[arg(long, default_value = "SpectreGlider")]
    rule: String,
    /// Aperiodic tiling: spectre, hat, penrose-kd, penrose-rhomb.
    #[arg(long, default_value = "spectre")]
    tiling: String,
    /// Patch level the cells are given on.
    #[arg(long, default_value_t = 4)]
    level: u32,
    #[arg(long, default_value = "touch")]
    nb: String,
    /// Starting cells as CELL:STATE pairs ("1203:1,1204:3").
    #[arg(long)]
    cells: Option<String>,
    /// Instead of --cells: put these states (comma separated, "1,3") on the tile --at and its
    /// edge neighbours in order; with no --at, try pairs around the patch's deepest tile until
    /// one travels.
    #[arg(long)]
    states: Option<String>,
    /// The tile for --states.
    #[arg(long)]
    at: Option<u32>,
    /// Generation limit (default 20000; with --rings, 100 per ring more).
    #[arg(long)]
    gens: Option<u64>,
    /// Stop as grown above this many live cells.
    #[arg(long, default_value_t = 2000)]
    max_pop: usize,
    /// Add the mover to the library when verified.
    #[arg(long)]
    store: bool,
    /// Write the tracker's full report as JSON.
    #[arg(long)]
    json_out: Option<PathBuf>,
    /// Stop, verified, at this many edge rings.
    #[arg(long)]
    rings: Option<u32>,
    /// World units of tiles kept around the live cells.
    #[arg(long, default_value_t = 24.0)]
    margin: f64,
    /// Spectre: follow the fixed patches' Delta chain up to this level.
    #[arg(long, default_value_t = 8)]
    fixed_levels: u32,
}

#[derive(clap::Args)]
pub struct PileArgs {
    #[command(flatten)]
    job: JobFlags,
    /// Tiling id: square, hex, triangle, snubsq, cairo, k3-012, ih41, spectre, hat, ...
    #[arg(long, default_value = "square")]
    tiling: String,
    /// Grains: 1000000, 1e7, 2^30 (up to 2^64 − 1).
    #[arg(long, default_value = "100000")]
    grains: String,
    /// edge (von Neumann on squares) or touch (Moore).
    #[arg(long, default_value = "edge")]
    nb: String,
    /// cell, vertex, or edge (grains split among the cells meeting at the point).
    #[arg(long, default_value = "cell")]
    centre: String,
    /// Aperiodic tilings: patch level (default: the smallest that should hold the pile, up to 8).
    #[arg(long)]
    level: Option<u32>,
    /// Parent folder for the run folder (default `<Pictures>/JETCARP/piles`).
    #[arg(long)]
    out: Option<PathBuf>,
    /// Pixels across a cell in pile.png (default 2; below 1.5, each pixel averages the cells
    /// centred in it). With --resume on a finished pile, the picture options export it again.
    #[arg(long)]
    px: Option<f64>,
    /// Empty cells around the pile in the image (default 4).
    #[arg(long)]
    margin: Option<f64>,
    /// Palette for the heights, with its background and stage (`jetcarp palettes` lists them).
    /// Default: Ember, or the folder's own when resuming one.
    #[arg(long)]
    palette: Option<String>,
    /// Run the palette the other way (height 1 at its last stop).
    #[arg(long)]
    reverse: bool,
    /// Longest side of preview.png (default 1024; 0 = none).
    #[arg(long)]
    preview: Option<u32>,
    /// Skip the full-size pile.png.
    #[arg(long)]
    no_png: bool,
    /// Topple on the GPU (periodic tilings).
    #[arg(long)]
    gpu: bool,
}

#[derive(clap::Args)]
pub struct CollideArgs {
    #[command(flatten)]
    job: JobFlags,
    /// Pattern A (stays at the origin).
    a: Option<String>,
    /// Pattern B (placed at each offset).
    b: Option<String>,
    /// Offset range for both axes, like -6..6 (overridden by --dx/--dy).
    #[arg(long, default_value = "-6..6", allow_hyphen_values = true)]
    offsets: String,
    #[arg(long, allow_hyphen_values = true)]
    dx: Option<String>,
    #[arg(long, allow_hyphen_values = true)]
    dy: Option<String>,
    /// B's phases: all, or a list like 0,1.
    #[arg(long, default_value = "all")]
    phases: String,
    #[arg(long, default_value_t = 0)]
    orientation_a: u32,
    /// B's orientation (index into the tiling's symmetries; `jetcarp library show` lists the
    /// direction each one moves a spaceship).
    #[arg(long, default_value_t = 0)]
    orientation_b: u32,
    #[arg(long, default_value_t = 1024)]
    gens: u64,
    #[arg(long, default_value_t = 40)]
    margin: i32,
    /// Rule to run (both patterns must work in it; default: A's rule if B works in it, else
    /// B's).
    #[arg(long)]
    rule: Option<String>,
    #[arg(long)]
    csv: Option<PathBuf>,
    #[arg(long)]
    json_out: Option<PathBuf>,
    /// Labelled contact sheet of every run's final state, objects coloured by class.
    #[arg(long)]
    sheet: Option<PathBuf>,
    #[arg(long, default_value_t = 96)]
    thumb: u32,
    #[arg(long, default_value_t = 13)]
    cols: u32,
    /// Do not catalog the products.
    #[arg(long)]
    no_store: bool,
}

#[derive(clap::Args)]
pub struct EvolveArgs {
    #[command(flatten)]
    job: JobFlags,
    /// Tiling: a family id (spectre, hat, penrose-kd, penrose-rhomb, square, hex, trihex, ...) or
    /// catalog:<id> (k-uniform and Archimedean tilings).
    #[arg(long, default_value = "spectre")]
    tiling: String,
    /// Neighbourhood: touch (the paper's vertex adjacency) or edge.
    #[arg(long, default_value = "touch")]
    nb: String,
    /// States per cell (2-8).
    #[arg(long, default_value_t = 4)]
    states: u8,
    /// Population size.
    #[arg(long, default_value_t = 256)]
    pop: usize,
    /// GA generations.
    #[arg(long, default_value_t = 800)]
    gens: usize,
    /// RNG seed (the run is a pure function of it).
    #[arg(long, default_value_t = 1)]
    seed: u64,
    /// Arena radius in edge rings.
    #[arg(long, default_value_t = 48)]
    radius: u32,
    /// Generations per seed run.
    #[arg(long, default_value_t = 2500)]
    horizon: u64,
    /// Population cap during the GA (explosive rules score 0).
    #[arg(long, default_value_t = 64)]
    cap: usize,
    /// Distinct top genomes verified at the end.
    #[arg(long, default_value_t = 16)]
    champions: usize,
    /// Verification radii as multiples of --radius.
    #[arg(long, default_value = "1,2,4")]
    verify: String,
    /// Stop at the first genome that passes verification.
    #[arg(long)]
    stop_on_glider: bool,
    /// Output root (default: $JETCARP_OUT, else <Pictures>/JETCARP); the run gets its own folder.
    #[arg(long)]
    out: Option<PathBuf>,
    /// Do not add verified movers to the library.
    #[arg(long)]
    no_store: bool,
    /// Generations the mover tracker may run on aperiodic patches.
    #[arg(long, default_value_t = 20000)]
    track_gens: u64,
}

#[derive(clap::Args)]
pub struct ExploreArgs {
    #[command(flatten)]
    job: JobFlags,
    /// Hours to run (the same as --minutes 60 x HOURS).
    #[arg(long)]
    hours: Option<f64>,
    /// Only these tilings: keys, comma separated (square,hex,trihex,ih28,catalog:k2-001; default:
    /// every tiling the search knows, 1,359 today; the progress line counts them).
    #[arg(long)]
    tilings: Option<String>,
    /// Only these rule families, comma separated (life_like, generations, ltl, jaca, cyclic, rps,
    /// greenberg_hastings, hodgepodge, priority, wireworld, turmite, layered, wolfram,
    /// hex_wolfram, hex_tri, margolus, lenia, sandpile, forest_fire, table, von_neumann,
    /// spectre_glider; default: all).
    #[arg(long)]
    families: Option<String>,
    /// About this many cells in a screening run's grid.
    #[arg(long, default_value_t = 16000)]
    screen_cells: u32,
    #[arg(long, default_value_t = 1024)]
    screen_gens: u32,
    /// Cells of a confirmation run (the best runs again, on a bigger grid).
    #[arg(long, default_value_t = 64000)]
    confirm_cells: u32,
    #[arg(long, default_value_t = 4096)]
    confirm_gens: u32,
    /// Seeds an elite is confirmed with.
    #[arg(long, default_value_t = 3)]
    confirm_seeds: u32,
    /// Most worlds in a GPU batch.
    #[arg(long, default_value_t = 1024)]
    batch: usize,
    #[arg(long, default_value_t = 0)]
    seed: u64,
    /// Run the rules a GPU can run on the GPU, when there is one (in this and later runs of the
    /// job); without one, or without this, the CPU runs everything.
    #[arg(long)]
    gpu: bool,
    /// Stop (paused, resumable) if the process uses more memory than this many GB.
    #[arg(long)]
    max_rss_gb: Option<f64>,
}

#[derive(clap::Args)]
pub struct TryArgs {
    /// Tiling key (square, hex, trihex, spectre, catalog:k2-001, ih28, ...).
    #[arg(long, default_value = "square")]
    tiling: String,
    /// Rule notation.
    #[arg(long)]
    rule: String,
    /// Starting pattern: a seed shorthand ("random:0.3", "pattern:blobs:0.4:by_region") or JSON
    /// (default: the rule's own).
    #[arg(long)]
    seed: Option<String>,
    /// touch or edge (default: touch, or what the notation says).
    #[arg(long)]
    nb: Option<String>,
    /// About this many cells.
    #[arg(long, default_value_t = 16000)]
    cells: u32,
    #[arg(long, default_value_t = 1024)]
    gens: u32,
    /// Random seed.
    #[arg(long, default_value_t = 1)]
    rng: u64,
    /// Picture size in pixels.
    #[arg(long, default_value_t = 420)]
    px: u32,
    /// Write a picture of the run's end here.
    #[arg(long)]
    png: Option<PathBuf>,
    /// Save the scene that replays the run here.
    #[arg(long)]
    save: Option<PathBuf>,
}

fn path(p: &Path) -> String {
    p.display().to_string()
}

fn spec<T: Serialize>(t: &T) -> Value {
    serde_json::to_value(t).unwrap_or_default()
}

/// Progress on standard error: one line rewritten in place on a terminal, else a line every
/// five seconds; the person's own lines (`hunt job 3 in ...`) on lines of their own; Ctrl-C
/// stops the job at its next checkpoint.
pub(crate) struct CliProgress {
    tty: bool,
    quiet: bool,
    stop: Arc<AtomicBool>,
    last: Mutex<(Option<Instant>, bool)>,
}

impl CliProgress {
    pub(crate) fn new(quiet: bool) -> CliProgress {
        let stop = Arc::new(AtomicBool::new(false));
        let presses = Arc::new(AtomicU32::new(0));
        {
            let (stop, presses) = (stop.clone(), presses.clone());
            let _ = ctrlc::set_handler(move || {
                if presses.fetch_add(1, Ordering::SeqCst) == 0 {
                    stop.store(true, Ordering::SeqCst);
                    eprintln!(
                        "\nCtrl-C: stopping at the next checkpoint (press Ctrl-C again to quit now; `jetcarp jobs resume JOB` continues)"
                    );
                } else {
                    std::process::exit(130);
                }
            });
        }
        CliProgress {
            tty: std::io::stderr().is_terminal(),
            quiet,
            stop,
            last: Mutex::new((Some(Instant::now()), false)),
        }
    }

    /// End the line rewritten in place, if one is showing.
    pub(crate) fn finish(&self) {
        let mut l = self.last.lock().unwrap();
        if l.1 {
            eprintln!();
            l.1 = false;
        }
    }
}

impl Progress for CliProgress {
    fn report(&self, _done: u64, _total: Option<u64>, msg: &str) {
        if self.quiet {
            return;
        }
        let mut l = self.last.lock().unwrap();
        if self.tty {
            eprint!("\r\x1b[2K{msg}");
            let _ = std::io::stderr().flush();
            l.1 = true;
        } else if l.0.is_none_or(|t| t.elapsed().as_secs_f64() >= 5.0) {
            eprintln!("{msg}");
            l.0 = Some(Instant::now());
        }
    }
    fn cancelled(&self) -> bool {
        self.stop.load(Ordering::SeqCst)
    }
    fn log(&self, line: &str) {
        let mut l = self.last.lock().unwrap();
        if l.1 {
            eprint!("\r\x1b[2K");
            l.1 = false;
        }
        eprintln!("{line}");
    }
}

/// A run's budget from its flags (`None`: none given).
pub(crate) fn budget(
    minutes: Option<f64>,
    until: Option<u64>,
    items: Option<u64>,
) -> Option<BudgetIn> {
    (minutes.is_some() || until.is_some() || items.is_some()).then_some(BudgetIn {
        minutes,
        until,
        items,
    })
}

/// The input every strategy shares.
fn start_in(strategy: &str, spec: Value, j: &JobFlags) -> Result<SearchStartIn> {
    let resume = match &j.resume {
        None => None,
        Some(r) => Some(
            r.parse::<i64>()
                .with_context(|| format!("--resume takes a job id, not {r}"))?,
        ),
    };
    Ok(SearchStartIn {
        strategy: Some(strategy.into()),
        spec,
        label: j.label.clone(),
        resume,
        budget: budget(j.minutes, j.until, j.limit),
        threads: j.threads,
        detach: j.detach,
        ..Default::default()
    })
}

fn ctx(j: &JobFlags) -> jetcarp_api::Ctx {
    let lib = j.library.clone().map(jetcarp_store::Library::file_for);
    crate::platform::ctx().with_library(lib)
}

/// Run the search and print what it did.
fn start(j: &JobFlags, a: SearchStartIn, json: bool) -> Result<SearchStarted> {
    let cx = ctx(j);
    let p = CliProgress::new(j.quiet);
    let r = SearchStart::run(&cx, a, &p);
    p.finish();
    let out = r?;
    show(&out, json)?;
    Ok(out)
}

pub fn run(a: SearchArgs, json: bool) -> Result<()> {
    match a.strategy {
        Strategy::Hunt(h) => hunt(*h, json),
        Strategy::Bounded(b) => bounded(*b, json),
        Strategy::Ants(x) => ants(*x, json),
        Strategy::Census(c) => census(*c, json),
        Strategy::Mover(m) => mover(*m, json),
        Strategy::Pile(p) => pile(*p, json),
        Strategy::Collide(c) => collide(*c, json),
        Strategy::Evolve(e) => evolve(*e, json),
        Strategy::Explore(e) => explore(*e, json),
        Strategy::Try(t) => try_one(*t, json),
    }
}

fn explore(mut a: ExploreArgs, json: bool) -> Result<()> {
    if let Some(h) = a.hours {
        if a.job.minutes.is_some() {
            bail!("give --hours or --minutes, not both");
        }
        a.job.minutes = Some(h * 60.0);
    }
    let s = ExploreIn {
        tilings: a.tilings.map(Value::String),
        families: a.families.map(Value::String),
        screen_cells: Some(a.screen_cells),
        screen_gens: Some(a.screen_gens),
        confirm_cells: Some(a.confirm_cells),
        confirm_gens: Some(a.confirm_gens),
        confirm_seeds: Some(a.confirm_seeds),
        batch: Some(a.batch),
        seed: Some(a.seed),
        gpu: Some(a.gpu),
        max_rss_gb: a.max_rss_gb,
    };
    let mut i = start_in("explore", spec(&s), &a.job)?;
    i.gpu = a.gpu;
    start(&a.job, i, json).map(|_| ())
}

fn try_one(a: TryArgs, json: bool) -> Result<()> {
    let seed = a
        .seed
        .map(|s| serde_json::from_str::<Value>(&s).unwrap_or(Value::String(s)));
    let t = TryIn {
        tiling: a.tiling,
        rule: a.rule,
        seed,
        nb: a.nb,
        cells: Some(a.cells),
        gens: Some(a.gens),
        rng: Some(a.rng),
        px: Some(a.px),
        png: a.png.as_deref().map(path),
    };
    let cx = crate::platform::ctx();
    let out = SearchTry::run(&cx, t, &jetcarp_api::Quiet)?;
    if let Some(p) = &a.save {
        let text = serde_json::to_string_pretty(&out.scene)? + "\n";
        std::fs::write(p, text).with_context(|| format!("writing {}", p.display()))?;
        eprintln!("scene: {}", p.display());
    }
    show(&out, json)
}

fn hunt(a: HuntArgs, json: bool) -> Result<()> {
    let store = if a.store_all {
        json!("all")
    } else {
        json!({"max_cells": a.store_max_cells, "max_volume": a.store_max_volume,
               "min_seen": a.store_min_seen})
    };
    let s = HuntIn {
        rules: Some(json!(a.rules)),
        rules_file: a.rules_file.as_deref().map(path),
        tilings: Some(json!(a.tilings)),
        nb: a.nb,
        soups: Some(if a.forever { 0 } else { a.soups }),
        size: Some(json!(a.size)),
        density: Some(a.density),
        shape: Some(a.shape),
        seed: Some(a.seed),
        gens: Some(a.gens),
        max_period: Some(a.max_period),
        pop_cap: Some(a.pop_cap),
        margin: Some(a.margin),
        store: Some(store),
    };
    let mut i = start_in("hunt", spec(&s), &a.job)?;
    i.gpu = a.gpu;
    start(&a.job, i, json).map(|_| ())
}

fn bounded(a: BoundedArgs, json: bool) -> Result<()> {
    if a.rules.is_none() && a.rules_file.is_none() && a.job.resume.is_none() {
        bail!("no rules: use --rules or --rules-file");
    }
    let s = BoundedIn {
        rules: a.rules.map(Value::String),
        rules_file: a.rules_file.as_deref().map(path),
        tilings: Some(json!(a.tilings)),
        size: Some(json!(a.size)),
        boundary: Some(a.boundary),
        nb: a.nb,
        soups: Some(a.soups),
        density: Some(a.density),
        gens: Some(a.gens),
        seed: Some(a.seed),
        max_period: Some(a.max_period),
        soup_radius: a.soup_radius,
        store: Some(!a.no_store),
        ..Default::default()
    };
    let mut i = start_in("search", spec(&s), &a.job)?;
    i.files = FilesIn {
        json: a.json_out.as_deref().map(path),
        ..Default::default()
    };
    let cx = ctx(&a.job);
    let p = CliProgress::new(a.job.quiet);
    let r = SearchStart::run(&cx, i, &p);
    p.finish();
    let out = r?;
    if json || a.top == 12 {
        return show(&out, json);
    }
    print!("{}", out.text_with_top(a.top));
    eprint!("{}", jetcarp_api::Report::notes(&out));
    Ok(())
}

fn ants(a: AntsArgs, json: bool) -> Result<()> {
    if a.strings.is_none() && a.tables.is_none() && a.rules.is_none() && a.job.resume.is_none() {
        bail!("give --strings 2-6, --tables 2x2, or --rules \"RL LLRR\"");
    }
    let s = AntsIn {
        tilings: Some(json!(a.tilings)),
        strings: a.strings.map(Value::String),
        tables: a.tables.map(Value::String),
        alphabet: a.alphabet,
        all_max: Some(a.all_max),
        sample: Some(a.sample),
        rules: a.rules.map(Value::String),
        soups: a.soups,
        size: Some(json!(a.size)),
        density: Some(a.density),
        seed: Some(a.seed),
        steps: Some(a.steps),
        max_period: Some(a.max_period),
        symmetry: Some(!a.no_symmetry),
    };
    let i = start_in("ants", spec(&s), &a.job)?;
    start(&a.job, i, json).map(|_| ())
}

fn census(a: CensusArgs, json: bool) -> Result<()> {
    let census = match &a.rerun {
        Some(p) => {
            let text =
                std::fs::read_to_string(p).with_context(|| format!("reading {}", p.display()))?;
            let v: Value = serde_json::from_str(&text)?;
            Some(
                v.get("spec")
                    .cloned()
                    .context("no \"spec\" in that report")?,
            )
        }
        None => None,
    };
    let tiling = match &a.tiling {
        Some(p) => {
            let text =
                std::fs::read_to_string(p).with_context(|| format!("reading {}", p.display()))?;
            let v: Value = serde_json::from_str(&text)?;
            Some(v.get("tiling").cloned().unwrap_or(v))
        }
        None => None,
    };
    if census.is_none()
        && a.rules.is_none()
        && a.rules_file.is_none()
        && !a.all
        && a.sample.is_none()
        && a.job.resume.is_none()
    {
        bail!("no rules: use --rules, --rules-file, --all, or --sample");
    }
    let from_family = tiling.is_none();
    let s = CensusIn {
        tiling: Some(tiling.unwrap_or_else(|| json!(a.family))),
        size: from_family.then_some(a.size),
        boundary: from_family.then_some(a.boundary),
        nb: a.nb,
        rules: a.rules.map(Value::String),
        rules_file: a.rules_file.as_deref().map(path),
        all: a.all,
        sample: a.sample,
        sample_seed: Some(a.sample_seed),
        all_max: a.sample.map(|_| 0),
        birth: Some(json!(a.birth)),
        survive: Some(json!(a.survive)),
        states: a.states,
        soups: Some(a.soups),
        density: Some(a.density),
        gens: Some(a.gens),
        seed: Some(a.seed),
        explode: Some(a.explode),
        census,
        ..Default::default()
    };
    let mut i = start_in("census", spec(&s), &a.job)?;
    i.files = FilesIn {
        json: a.json_out.as_deref().map(path),
        csv: a.csv.as_deref().map(path),
        sheet: a.sheet.as_deref().map(path),
        thumb: Some(a.thumb),
        cols: Some(a.sheet_cols),
    };
    start(&a.job, i, json).map(|_| ())
}

fn mover(a: MoverArgs, json: bool) -> Result<()> {
    let s = MoverIn {
        rule: Some(a.rule),
        tiling: Some(a.tiling),
        level: Some(a.level),
        nb: Some(a.nb),
        cells: a.cells.map(Value::String),
        states: a.states.map(Value::String),
        at: a.at,
        rings: a.rings,
        gens: a.gens,
        max_pop: Some(a.max_pop),
        margin: Some(a.margin),
        fixed_levels: Some(a.fixed_levels),
        store: Some(a.store),
        source: Some("jetcarp search mover".into()),
    };
    let mut i = start_in("mover", spec(&s), &a.job)?;
    i.files = FilesIn {
        json: a.json_out.as_deref().map(path),
        ..Default::default()
    };
    start(&a.job, i, json).map(|_| ())
}

/// `jetcarp search pile` and `jetcarp pile`.
pub fn pile(a: PileArgs, json: bool) -> Result<()> {
    // `--resume` takes a job id or a run folder.
    let folder = a
        .job
        .resume
        .as_ref()
        .filter(|r| r.parse::<i64>().is_err())
        .cloned();
    let mut s = PileIn {
        out: a.out.as_deref().map(path),
        px: a.px,
        margin: a.margin,
        palette: a.palette,
        reverse: a.reverse.then_some(true),
        preview: a.preview,
        png: a.no_png.then_some(false),
        gpu: Some(a.gpu),
        ..Default::default()
    };
    match folder {
        Some(f) => s.folder = Some(f),
        None => {
            s.tiling = Some(a.tiling);
            s.grains = Some(json!(a.grains));
            s.nb = Some(a.nb);
            s.centre = Some(a.centre);
            s.level = a.level;
        }
    }
    let mut i = SearchStartIn {
        strategy: Some("pile".into()),
        spec: spec(&s),
        label: a.job.label.clone(),
        resume: a.job.resume.as_ref().and_then(|r| r.parse::<i64>().ok()),
        budget: budget(a.job.minutes, a.job.until, a.job.limit),
        threads: a.job.threads,
        detach: a.job.detach,
        ..Default::default()
    };
    i.gpu = a.gpu;
    start(&a.job, i, json).map(|_| ())
}

fn collide(a: CollideArgs, json: bool) -> Result<()> {
    let (pa, pb) = match (&a.a, &a.b, &a.job.resume) {
        (Some(x), Some(y), _) => (x.clone(), y.clone()),
        (_, _, Some(_)) => (String::new(), String::new()),
        _ => bail!("give patterns A and B"),
    };
    let s = CollideIn {
        a: pa,
        b: pb,
        offsets: Some(json!(a.offsets)),
        dx: a.dx.map(Value::String),
        dy: a.dy.map(Value::String),
        phases: Some(json!(a.phases)),
        orientation_a: Some(a.orientation_a),
        orientation_b: Some(a.orientation_b),
        gens: Some(a.gens),
        margin: Some(a.margin),
        rule: a.rule,
        store: Some(!a.no_store),
    };
    let mut i = start_in("collide", spec(&s), &a.job)?;
    i.files = FilesIn {
        json: a.json_out.as_deref().map(path),
        csv: a.csv.as_deref().map(path),
        sheet: a.sheet.as_deref().map(path),
        thumb: Some(a.thumb),
        cols: Some(a.cols),
    };
    start(&a.job, i, json).map(|_| ())
}

fn evolve(a: EvolveArgs, json: bool) -> Result<()> {
    let s = EvolveIn {
        tiling: Some(a.tiling),
        nb: Some(a.nb),
        states: Some(a.states),
        pop: Some(a.pop),
        gens: Some(a.gens),
        seed: Some(a.seed),
        radius: Some(a.radius),
        horizon: Some(a.horizon),
        cap: Some(a.cap),
        champions: Some(a.champions),
        verify: Some(json!(a.verify)),
        stop_on_glider: a.stop_on_glider,
        threads: a.job.threads,
        store: Some(!a.no_store),
        track_gens: Some(a.track_gens),
        out: a.out.as_deref().map(path),
        ..Default::default()
    };
    let i = start_in("evolve", spec(&s), &a.job)?;
    start(&a.job, i, json).map(|_| ())
}

#[cfg(test)]
mod tests {
    /// The --tilings help names how many tilings the search knows; keep it true.
    #[test]
    fn the_tilings_help_gives_the_real_count() {
        use clap::CommandFactory;
        let n = jetcarp_core::explore::space::all_tilings().len();
        let count = format!("{},{:03}", n / 1000, n % 1000);
        #[derive(clap::Parser)]
        struct Wrap {
            #[command(flatten)]
            a: super::ExploreArgs,
        }
        let help = Wrap::command().render_long_help().to_string();
        assert!(
            help.contains(&format!("knows, {count} today")),
            "{count}: {help}"
        );
    }
}
