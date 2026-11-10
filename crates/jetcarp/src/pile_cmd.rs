//! `jetcarp pile`: a large sandpile on any tiling, built by doubling on an unbounded grid, with a
//! cropped PNG, a preview, metadata, and a pile file per doubling step. Each pile is a catalog
//! job (kind `pile`, `jetcarp_core::pile::job`), so it shows in `job_status` and the dashboard and
//! resumes by job id (or by its folder).

use anyhow::{Result, bail};
use jetcarp_core::library::Library;
use jetcarp_core::pile::job::{self as pilejob, PileJobSpec};
use jetcarp_core::pile::{self, Centre, PileSpec, Toppler};
use jetcarp_core::{Nb, SizeSpec};
use std::io::{IsTerminal, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

#[derive(clap::Args)]
pub struct PileArgs {
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
    /// Continue a pile: its job id, or its run folder (from the last saved step).
    #[arg(long)]
    resume: Option<String>,
    /// The catalog folder or file (default: the standard catalog).
    #[arg(long)]
    library: Option<PathBuf>,
    /// Pixels across a cell in pile.png (below 1.5, each pixel averages the cells centred in it).
    #[arg(long, default_value_t = 2.0)]
    px: f64,
    /// Empty cells around the pile in the image.
    #[arg(long, default_value_t = 4.0)]
    margin: f64,
    /// Palette for the heights, with its background and stage (`jetcarp palettes` lists them).
    /// Default: Ember, or the folder's own when resuming one.
    #[arg(long)]
    palette: Option<String>,
    /// Run the palette the other way (height 1 at its last stop).
    #[arg(long)]
    reverse: bool,
    /// Longest side of preview.png (0 = none).
    #[arg(long, default_value_t = 1024)]
    preview: u32,
    /// Skip the full-size pile.png.
    #[arg(long)]
    no_png: bool,
    /// Topple on the GPU (periodic tilings).
    #[arg(long)]
    gpu: bool,
    /// Worker threads (default: all cores).
    #[arg(long)]
    threads: Option<usize>,
}

pub fn run(a: PileArgs) -> Result<()> {
    if let Some(p) = &a.palette {
        jetcarp_core::render::builtin_palette(p).map_err(anyhow::Error::msg)?;
    }
    if let Some(t) = a.threads {
        rayon::ThreadPoolBuilder::new()
            .num_threads(t)
            .build_global()
            .ok();
    }
    let lib = match &a.library {
        Some(p) => Library::open(Library::file_for(p.clone())),
        None => Library::open_default(),
    };
    let msg = anyhow::Error::msg;
    // Every pile is a catalog job (kind `pile`): `job_status`, the dashboard, and `--resume`
    // by job id see it.
    let job = match &a.resume {
        Some(r) => match r.parse::<i64>() {
            Ok(id) => {
                pilejob::status(&lib, id).map_err(msg)?;
                id
            }
            Err(_) => job_for_folder(&lib, &PathBuf::from(r), &a)?,
        },
        None => {
            let mut tiling = jetcarp_script::spec::tiling_by_id(&a.tiling).map_err(msg)?;
            if let Some(l) = a.level {
                tiling.size = Some(SizeSpec::Level(l));
            }
            let spec = PileSpec {
                tiling,
                nb: pile::parse_nb(&a.nb).map_err(msg)?,
                grains: pile::parse_grains(&a.grains).map_err(msg)?,
                centre: Centre::parse(&a.centre).map_err(msg)?,
            };
            let root = a
                .out
                .clone()
                .unwrap_or_else(|| jetcarp_api::out::out_root(None).join("piles"));
            std::fs::create_dir_all(&root)?;
            let label = format!("{}-{}", a.tiling, spec.grains);
            let dir = jetcarp_api::out::make_out_dir(&root, &label).map_err(msg)?;
            pilejob::start(&lib, &job_spec(spec, dir, &a)).map_err(msg)?
        }
    };
    let (spec, p0) = pilejob::status(&lib, job).map_err(msg)?;
    let nb = match spec.pile.nb {
        Nb::Edge => "edge",
        Nb::Touch => "touch",
    };
    eprintln!(
        "pile job {job}: {} grains on {} ({nb}, centre {}): {} doubling steps, {} done; folder {}",
        spec.pile.grains,
        p0.tiling,
        spec.pile.centre.name(),
        p0.steps,
        p0.step,
        spec.folder.display()
    );
    let gpu: Option<Box<dyn Toppler>> = if spec.gpu {
        if pile::is_patch(&spec.pile.tiling) {
            bail!("--gpu works on periodic tilings; aperiodic patches run on the CPU");
        }
        let g = jetcarp_gpu::pile::PileGpu::headless().map_err(msg)?;
        eprintln!("GPU: {}", g.name());
        Some(Box::new(g))
    } else {
        None
    };

    let stop = Arc::new(AtomicBool::new(false));
    let presses = Arc::new(AtomicU32::new(0));
    {
        let (stop, presses) = (stop.clone(), presses.clone());
        let _ = ctrlc::set_handler(move || {
            if presses.fetch_add(1, Ordering::SeqCst) == 0 {
                stop.store(true, Ordering::SeqCst);
                eprintln!(
                    "\nCtrl-C: stopping (the last finished step is saved; press again to quit now)"
                );
            } else {
                std::process::exit(130);
            }
        });
    }
    // Print progress from the job's live record while it runs.
    let live = Arc::new(Mutex::new(p0.clone()));
    let done = Arc::new(AtomicBool::new(false));
    let printer = {
        let (live, done) = (live.clone(), done.clone());
        std::thread::spawn(move || {
            let tty = std::io::stderr().is_terminal();
            let every = if tty { 0.5 } else { 30.0 };
            let mut step = live.lock().unwrap().step;
            let mut last = Instant::now();
            while !done.load(Ordering::SeqCst) {
                std::thread::sleep(std::time::Duration::from_millis(100));
                let p = live.lock().unwrap().clone();
                if p.step > step {
                    step = p.step;
                    if tty {
                        eprint!("\r\x1b[2K");
                    }
                    eprintln!(
                        "step {:>2}/{}: {:>20} grains  {:>22} topplings  {:>10} so far",
                        p.step,
                        p.steps,
                        p.grains_done,
                        p.topplings,
                        pile::fmt_seconds(p.seconds)
                    );
                } else if last.elapsed().as_secs_f64() >= every && p.in_step[2] > 0.0 {
                    last = Instant::now();
                    let line = format!(
                        "step {}/{}: {:.0} sweeps, {:.3e} topplings, {}",
                        p.step + 1,
                        p.steps,
                        p.in_step[0],
                        p.in_step[1],
                        pile::fmt_seconds(p.in_step[2])
                    );
                    if tty {
                        eprint!("\r\x1b[2K{line}");
                        let _ = std::io::stderr().flush();
                    } else {
                        eprintln!("{line}");
                    }
                }
            }
        })
    };
    let r = pilejob::run_job(&lib, job, stop.clone(), gpu, Some(live.clone()));
    done.store(true, Ordering::SeqCst);
    let _ = printer.join();
    let p = r.map_err(msg)?;
    if p.status == "paused" {
        eprintln!(
            "\nstopped after step {}/{}; resume with: jetcarp pile --resume {job}",
            p.step, p.steps
        );
        std::process::exit(130);
    }
    eprintln!(
        "done: {} grains, {} topplings, {} sweeps, {} of toppling",
        p.grains,
        p.topplings,
        p.sweeps,
        pile::fmt_seconds(p.seconds)
    );
    if let Some(png) = &p.png {
        eprintln!("image:   {png}");
    }
    if let Some(pv) = &p.preview {
        eprintln!("preview: {pv}");
    }
    eprintln!("folder:  {}", spec.folder.display());
    eprintln!("job:     {job}");
    Ok(())
}

fn job_spec(pile: PileSpec, folder: PathBuf, a: &PileArgs) -> PileJobSpec {
    // A folder's own colouring (look.json) unless the flags give one.
    let kept = pile::export::read_look_file(&folder);
    PileJobSpec {
        pile,
        px_per_cell: a.px,
        margin: a.margin,
        palette: a
            .palette
            .clone()
            .or_else(|| kept.as_ref().map(|k| k.0.clone()))
            .unwrap_or_else(|| "Ember".into()),
        reverse: a.reverse || (a.palette.is_none() && kept.is_some_and(|k| k.1)),
        folder,
        preview_side: a.preview,
        png: !a.no_png,
        gpu: a.gpu,
        extra: serde_json::json!({
            "tiling_id": a.tiling,
            "threads": rayon::current_num_threads(),
            "host": jetcarp_api::out::host_name(),
            "started": jetcarp_api::out::now_rfc3339(),
        }),
    }
}

/// The pile job of a run folder: the job whose folder it is, or a new job for a folder made
/// before piles were jobs (from its last step file).
fn job_for_folder(lib: &Library, dir: &std::path::Path, a: &PileArgs) -> Result<i64> {
    let want = std::fs::canonicalize(dir).unwrap_or_else(|_| dir.to_path_buf());
    for j in lib.jobs().map_err(anyhow::Error::msg)? {
        if j.kind != pilejob::KIND {
            continue;
        }
        if let Ok(s) = serde_json::from_value::<PileJobSpec>(j.spec.clone())
            && std::fs::canonicalize(&s.folder).unwrap_or(s.folder.clone()) == want
        {
            return Ok(j.id);
        }
    }
    let f = pile::latest_step_file(dir)
        .ok_or_else(|| anyhow::anyhow!("no step files in {}/steps", dir.display()))?;
    let p = pile::file::load(&f).map_err(anyhow::Error::msg)?;
    eprintln!("resuming {} from {}", dir.display(), f.display());
    pilejob::start(lib, &job_spec(p.spec.clone(), dir.to_path_buf(), a)).map_err(anyhow::Error::msg)
}
