//! `jetcarp hunt`: the fast soup hunter (`jetcarp_core::hunt`). Small soups on an unbounded plane,
//! run bit-parallel until they settle, split into objects, and counted in the catalog as a
//! resumable job.

use anyhow::{Context, Result, bail};
use jetcarp_core::hunt::job::{self, HuntOptions, HuntProgress, HuntSpec};
use jetcarp_core::hunt::soup::{SoupShape, SoupSpec};
use jetcarp_core::library::{self, JobStatus, Library};
use jetcarp_core::*;
use std::io::{IsTerminal, Write};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::time::Instant;

#[derive(clap::Args)]
pub struct HuntArgs {
    /// Catalog: a .sqlite file, or a folder holding catalog.sqlite (default: $JETCARP_LIBRARY, else
    /// <data>/catalog.sqlite).
    #[arg(long)]
    library: Option<PathBuf>,
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
    /// Run until stopped (Ctrl-C, `jetcarp hunt --stop JOB`, or the MCP tool hunt_stop).
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
    /// Worker threads (0: all cores).
    #[arg(long, default_value_t = 0)]
    threads: usize,
    /// Run the start of each soup on the GPU (many soups per dispatch; results are identical).
    #[arg(long)]
    gpu: bool,
    /// Continue a stopped or interrupted hunt job (its spec is kept; other options ignored).
    #[arg(long, value_name = "JOB")]
    resume: Option<i64>,
    /// Stop after this many soups (per group) in this session; the job stays resumable.
    #[arg(long)]
    limit: Option<u64>,
    /// Print a hunt's status and exit.
    #[arg(long, value_name = "JOB")]
    status: Option<i64>,
    /// Ask a running hunt (in any process) to stop at its next checkpoint, and exit.
    #[arg(long, value_name = "JOB")]
    stop: Option<i64>,
    /// List hunt jobs and exit.
    #[arg(long)]
    list: bool,
    /// Print the final progress as JSON.
    #[arg(long)]
    json: bool,
}

fn msg<T>(r: Result<T, String>) -> Result<T> {
    r.map_err(anyhow::Error::msg)
}

fn parse_size(s: &str) -> Result<(u32, u32)> {
    let (w, h) = s
        .split_once(['x', 'X'])
        .map(|(a, b)| (a.trim(), b.trim()))
        .unwrap_or((s.trim(), s.trim()));
    Ok((
        w.parse().context("soup width")?,
        h.parse().context("soup height")?,
    ))
}

fn show(p: &HuntProgress, json: bool) {
    if json {
        println!("{}", serde_json::to_string_pretty(p).unwrap());
        return;
    }
    println!("{}", p.line());
    println!(
        "  status {}, {:.0} s run, {} settled, {} died, {} exploded, {} unsettled{}",
        p.status,
        p.elapsed_s,
        p.settled,
        p.died,
        p.exploded,
        p.unsettled,
        p.gpu
            .as_ref()
            .map(|g| format!(", GPU {g}"))
            .unwrap_or_default()
    );
    for g in &p.groups {
        let classes: Vec<String> = g.by_class.iter().map(|(k, v)| format!("{k} {v}")).collect();
        println!(
            "  {} on {}: {} soups, {} objects ({}), {} distinct, {} new{}",
            g.rule,
            g.tiling,
            g.soups,
            g.objects(),
            classes.join(", "),
            g.distinct,
            g.new_patterns,
            g.error
                .as_ref()
                .map(|e| format!(" [error: {e}]"))
                .unwrap_or_default()
        );
    }
    if !p.newest.is_empty() {
        println!("  newest finds:");
        for f in p.newest.iter().take(10) {
            println!(
                "    {}  {:<34} {:<10} {} soup {}{}",
                f.id,
                f.label,
                f.rule,
                f.tiling,
                f.soup,
                if f.new { "  (new to the catalog)" } else { "" }
            );
        }
    }
}

pub fn run(a: HuntArgs) -> Result<()> {
    let lib = match &a.library {
        Some(p) => Library::open(Library::file_for(p.clone())),
        None => Library::open_default(),
    };
    if a.list {
        for j in msg(lib.jobs())?.into_iter().filter(|j| j.kind == "hunt") {
            let (_, p) = msg(job::status(&lib, j.id))?;
            println!("{}", p.line());
            println!(
                "  status {}, created {}, updated {}",
                p.status, j.created, j.updated
            );
        }
        return Ok(());
    }
    if let Some(jid) = a.status {
        let (_, p) = msg(job::status(&lib, jid))?;
        show(&p, a.json);
        return Ok(());
    }
    if let Some(jid) = a.stop {
        msg(job::request_stop(&lib, jid))?;
        eprintln!("asked hunt {jid} to stop at its next checkpoint");
        return Ok(());
    }
    let jid = match a.resume {
        Some(j) => {
            let row = msg(lib.job(j))?;
            if row.status == JobStatus::Done {
                bail!("hunt {j} is already done");
            }
            j
        }
        None => {
            let mut rules: Vec<String> = a
                .rules
                .split([';', ' ', '\t', '\n'])
                .map(str::trim)
                .filter(|x| !x.is_empty())
                .map(String::from)
                .collect();
            if let Some(p) = &a.rules_file {
                let text = std::fs::read_to_string(p)
                    .with_context(|| format!("reading {}", p.display()))?;
                rules.extend(census::parse_rule_list(&text));
            }
            let (w, h) = parse_size(&a.size)?;
            let spec = HuntSpec {
                rules,
                tilings: msg(library::tiling_list(&a.tilings))?,
                neighborhood: match a.nb.as_deref() {
                    None => None,
                    Some("touch") | Some("t") => Some(Nb::Touch),
                    Some("edge") | Some("e") => Some(Nb::Edge),
                    Some(x) => bail!("--nb is touch or edge, not {x}"),
                },
                soups: (!a.forever).then_some(a.soups),
                soup: SoupSpec {
                    w,
                    h,
                    density: a.density,
                    shape: msg(SoupShape::parse(&a.shape))?,
                },
                seed: a.seed,
                max_gens: a.gens,
                max_period: a.max_period,
                pop_cap: a.pop_cap,
                margin: a.margin,
                store: if a.store_all {
                    job::StorePolicy::all()
                } else {
                    job::StorePolicy {
                        max_cells: a.store_max_cells,
                        max_volume: a.store_max_volume,
                        min_seen: a.store_min_seen,
                    }
                },
            };
            msg(job::start(&lib, &spec))?
        }
    };
    eprintln!("hunt job {jid} in {}", lib.path().display());
    let stop = Arc::new(AtomicBool::new(false));
    let presses = Arc::new(AtomicU32::new(0));
    {
        let (stop, presses) = (stop.clone(), presses.clone());
        let _ = ctrlc::set_handler(move || {
            if presses.fetch_add(1, Ordering::SeqCst) == 0 {
                stop.store(true, Ordering::SeqCst);
                eprintln!(
                    "\nCtrl-C: stopping at the next checkpoint (press Ctrl-C again to quit now; `jetcarp hunt --resume {jid}` continues)"
                );
            } else {
                std::process::exit(130);
            }
        });
    }
    let gpu: Option<Arc<dyn job::SoupBatcher>> = if a.gpu {
        match jetcarp_gpu::hunt::GpuHunter::headless() {
            Ok(g) => {
                eprintln!("GPU: {}", g.name());
                Some(Arc::new(g))
            }
            Err(e) => bail!("--gpu: {e}"),
        }
    } else {
        None
    };
    let tty = std::io::stderr().is_terminal();
    let last = std::sync::Mutex::new(Instant::now());
    let on_progress = Arc::new(move |p: &HuntProgress| {
        if tty {
            eprint!("\r\x1b[2K{}", p.line());
            let _ = std::io::stderr().flush();
        } else {
            let mut l = last.lock().unwrap();
            if l.elapsed().as_secs_f64() >= 5.0 || p.status != "running" {
                eprintln!("{}", p.line());
                *l = Instant::now();
            }
        }
    });
    let opts = HuntOptions {
        threads: a.threads,
        stop,
        limit: a.limit,
        gpu,
        on_progress: Some(on_progress),
        ..Default::default()
    };
    let p = msg(job::run(&lib, jid, &opts))?;
    if tty {
        eprintln!();
    }
    show(&p, a.json);
    if p.status == "paused" {
        eprintln!(
            "hunt {jid} paused at soup {}; `jetcarp hunt --resume {jid}` continues",
            p.soups_done
        );
    }
    Ok(())
}
