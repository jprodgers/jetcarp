//! `jetcarp evolve`: evolutionary glider search (`jetcarp_core::evolve`), after Edmond's method.
//! The search is a catalog job (`jetcarp_core::jobs::evolve`), checkpointed after every
//! generation, so `--resume JOB` continues one that was stopped (another front end's
//! `job_stop`, the app's Hunts window) or cut off.

use anyhow::{Result, anyhow};
use jetcarp_api::args;
use jetcarp_core::evolve::{EvolveSpec, GenStats};
use jetcarp_core::jobs::evolve::{self as evolve_job, EvolveJobSpec, GenHook};
use jetcarp_core::jobs::{RunOptions, typed_spec};
use jetcarp_core::library::{self, JobStatus, Library};
use jetcarp_core::topology::Nb;
use std::path::PathBuf;
use std::sync::Arc;

#[derive(clap::Args)]
pub struct EvolveArgs {
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
    /// Worker threads (default: all cores). Results do not depend on this.
    #[arg(long)]
    threads: Option<usize>,
    /// Output root (default: $JETCARP_OUT, else <Pictures>/JETCARP); the run gets its own folder.
    #[arg(long)]
    out: Option<PathBuf>,
    /// Catalog for verified movers: a .sqlite file or a folder (default: $JETCARP_LIBRARY, else
    /// <data>/catalog.sqlite).
    #[arg(long)]
    library: Option<PathBuf>,
    /// Do not add verified movers to the library.
    #[arg(long)]
    no_store: bool,
    /// Generations the mover tracker may run on aperiodic patches.
    #[arg(long, default_value_t = 20000)]
    track_gens: u64,
    /// Print only the summary.
    #[arg(long)]
    quiet: bool,
    /// Continue evolve job JOB from its last finished generation (its search, folder, and
    /// storing are the job's own; the other flags are ignored).
    #[arg(long)]
    resume: Option<i64>,
}

/// `--nb` for evolve and track.
pub fn parse_nb(s: &str) -> Result<Nb> {
    args::nb_word(s).ok_or_else(|| {
        anyhow!(
            "neighbourhood '{}' is not touch or edge",
            s.trim().to_ascii_lowercase()
        )
    })
}

pub fn run(a: EvolveArgs) -> Result<()> {
    let lib = match &a.library {
        Some(p) => Library::open(p),
        None => Library::open_default(),
    };
    let (job, tiling_name) = match a.resume {
        Some(j) => {
            let js: EvolveJobSpec =
                typed_spec(&lib, j, evolve_job::KIND_NAME).map_err(|e| anyhow!(e))?;
            if let Some(d) = &js.folder {
                eprintln!("output: {}", d.display());
            }
            (j, library::tiling_key(&js.search.tiling))
        }
        None => {
            let tiling = library::tiling_from_key(&a.tiling).map_err(|e| anyhow!(e))?;
            let factors: Vec<u32> = a
                .verify
                .split([',', ' '])
                .filter(|s| !s.is_empty())
                .map(|s| s.trim().parse::<u32>())
                .collect::<Result<_, _>>()
                .map_err(|_| anyhow!("--verify takes multiples like 1,2,4"))?;
            let spec = EvolveSpec {
                tiling,
                nb: parse_nb(&a.nb)?,
                states: a.states,
                pop: a.pop,
                gens: a.gens,
                seed: a.seed,
                radius: a.radius,
                horizon: a.horizon,
                pop_cap: a.cap,
                champions: a.champions,
                verify_factors: factors,
                stop_on_glider: a.stop_on_glider,
                threads: a.threads,
            };
            let label = format!("evolve-{}", a.tiling.replace(':', "-"));
            let root = jetcarp_api::out::out_root(a.out.as_deref());
            let dir = jetcarp_api::out::make_out_dir(&root, &label).map_err(|e| anyhow!(e))?;
            eprintln!("output: {}", dir.display());
            let js = EvolveJobSpec {
                search: spec,
                store: !a.no_store,
                track_gens: a.track_gens,
                folder: Some(dir),
            };
            let j = evolve_job::start(&lib, &js).map_err(|e| anyhow!(e))?;
            (j, a.tiling.clone())
        }
    };
    eprintln!("evolve job {job} in {}", lib.path().display());
    let quiet = a.quiet;
    let hook: GenHook = Arc::new(move |g: &GenStats| {
        if !quiet {
            eprintln!(
                "gen {:>4}  best {:>6.2}  median {:>6.2}  best ever {:>6.2}  at ceiling {:>3}  distinct {:>3}  evals {:>7}  {:>7.1}s",
                g.generation,
                g.best,
                g.median,
                g.best_ever,
                g.at_ceiling,
                g.distinct,
                g.unique_evaluations,
                g.elapsed_s
            );
        }
    });
    let (out, res) =
        evolve_job::run(&lib, job, Some(hook), &RunOptions::default()).map_err(|e| anyhow!(e))?;
    let Some(res) = res else {
        if out.status == JobStatus::Done {
            println!("evolve job {job} is done (its files are in its folder)");
        } else {
            eprintln!(
                "evolve job {job} paused after {} generation(s); `jetcarp evolve --resume {job}` continues",
                out.progress["generation"]
                    .as_u64()
                    .map(|g| g + 1)
                    .unwrap_or(0)
            );
        }
        return Ok(());
    };
    let (report, found, files) = (res.report, res.found, res.files);
    println!(
        "evolve on {} ({} tiles, margin {}, boundary ring {}): {} generations, {:.1}s",
        tiling_name,
        report.arena_tiles,
        report.margin,
        report.boundary,
        report.generations.len(),
        report.elapsed_s
    );
    if let Some(g) = report.generations.last() {
        println!(
            "best ever {:.2}, {} genome evaluations ({} distinct)",
            g.best_ever, g.evaluations, g.unique_evaluations
        );
    }
    for c in &report.champions {
        let v = c.verify.as_ref();
        println!(
            "  {:>6.2}  gen {:>4}  {}  [{}]",
            c.fitness,
            c.first_generation,
            c.rule,
            v.map(|v| if v.passed {
                "GLIDER".to_string()
            } else {
                v.reason.clone()
            })
            .unwrap_or_default()
        );
    }
    for f in &found {
        println!("  mover: {}", f.summary);
    }
    for f in files {
        println!("wrote {}", f.display());
    }
    Ok(())
}
