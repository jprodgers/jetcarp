//! `jetcarp ants`: ant and turmite search (`jetcarp_core::ants`). Turn strings and turmite tables run
//! on unbounded periodic tilings; highways are proved exactly; every run is classed and stored
//! in the catalog's `ants` table by a resumable job.

use anyhow::{Context, Result, bail};
use jetcarp_core::ants::job::{self, AntOptions, AntProgress, AntSpec, Soups, parse_rule};
use jetcarp_core::ants::run::{Limits, Outcome, Start};
use jetcarp_core::ants::search::{Strings, Tables};
use jetcarp_core::library::{self, AntQuery, AntRow, AntUpdate, JobStatus, Library, QuickStartRow};
use std::io::{IsTerminal, Write};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::time::Instant;

#[derive(clap::Args)]
pub struct AntsArgs {
    /// Catalog: a .sqlite file, or a folder holding catalog.sqlite (default: $JETCARP_LIBRARY, else
    /// <Documents>/jetcarp-out/catalog.sqlite).
    #[arg(long, global = true)]
    library: Option<PathBuf>,
    /// Print JSON.
    #[arg(long, global = true)]
    json: bool,
    #[command(subcommand)]
    cmd: AntsCmd,
}

#[derive(clap::Subcommand)]
enum AntsCmd {
    /// Search turn strings and/or turmite tables on tilings, as a resumable catalog job.
    Search(Box<SearchArgs>),
    /// A search job's progress (no JOB: every ant search).
    Status { job: Option<i64> },
    /// Ask a running search (in any process) to stop at its next batch.
    Stop { job: i64 },
    /// Run one rule (no catalog unless --store).
    Run(RunArgs),
    /// Stored runs, best first, with filters.
    List(ListArgs),
    /// One stored run.
    Show { id: String },
    /// Outcome counts per tiling (of one job, or every run).
    Census {
        #[arg(long)]
        job: Option<i64>,
    },
    /// Name, rate, or annotate a stored run.
    Update {
        id: String,
        #[arg(long)]
        name: Option<String>,
        #[arg(long)]
        notes: Option<String>,
        /// 1 to 5.
        #[arg(long)]
        rating: Option<i64>,
        /// new, kept, promoted, rejected, ... (free text).
        #[arg(long)]
        state: Option<String>,
    },
    /// Delete a stored run (with its media rows and ratings).
    Remove { id: String },
    /// Write the build-up scene (the run from step 0) to a file the app opens.
    Scene {
        id: String,
        #[arg(long)]
        out: PathBuf,
        /// The scene's palette (default Prism; `jetcarp palettes` lists them).
        #[arg(long)]
        palette: Option<String>,
    },
    /// Add the build-up scene to the Quick Start ("My finds").
    Promote {
        id: String,
        /// The scene's palette (default Prism).
        #[arg(long)]
        palette: Option<String>,
    },
}

#[derive(clap::Args)]
pub struct SearchArgs {
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
    /// Worker threads (0: all cores).
    #[arg(long, default_value_t = 0)]
    threads: usize,
    /// Stop after this many runs in this session; the job stays resumable.
    #[arg(long)]
    limit: Option<u64>,
    /// Continue a stopped or interrupted search (its spec is kept; other options ignored).
    #[arg(long, value_name = "JOB")]
    resume: Option<i64>,
}

#[derive(clap::Args)]
pub struct RunArgs {
    rule: String,
    #[arg(long, default_value = "square")]
    tiling: String,
    #[arg(long, default_value_t = 1_000_000)]
    steps: u64,
    #[arg(long, default_value_t = 20_000)]
    max_period: u64,
    /// Start from a soup WxH:density:seed (e.g. 8x8:0.5:1) instead of an empty field.
    #[arg(long)]
    soup: Option<String>,
    /// Store the result in the catalog.
    #[arg(long)]
    store: bool,
}

#[derive(clap::Args)]
pub struct ListArgs {
    /// highway, periodic, symmetric, bounded, chaotic.
    #[arg(long)]
    outcome: Option<String>,
    #[arg(long)]
    tiling: Option<String>,
    #[arg(long)]
    rule: Option<String>,
    #[arg(long)]
    job: Option<i64>,
    /// score, onset, period, speed, rule.
    #[arg(long, default_value = "score")]
    sort: String,
    #[arg(long, default_value_t = 50)]
    limit: usize,
}

fn msg<T>(r: Result<T, String>) -> Result<T> {
    r.map_err(anyhow::Error::msg)
}

fn parse_size(s: &str) -> Result<(u32, u32)> {
    let (w, h) = s
        .split_once(['x', 'X'])
        .map(|(a, b)| (a.trim(), b.trim()))
        .unwrap_or((s.trim(), s.trim()));
    Ok((w.parse().context("width")?, h.parse().context("height")?))
}

fn parse_range(s: &str) -> Result<(usize, usize)> {
    match s.split_once('-') {
        Some((a, b)) => Ok((a.trim().parse()?, b.trim().parse()?)),
        None => {
            let n = s.trim().parse()?;
            Ok((n, n))
        }
    }
}

fn show_progress(p: &AntProgress, json: bool) {
    if json {
        println!("{}", serde_json::to_string_pretty(p).unwrap());
        return;
    }
    println!("{}", p.line());
    println!(
        "  status {}, {:.0} s run, {} ant steps",
        p.status, p.elapsed_s, p.steps
    );
    for g in &p.groups {
        let outs: Vec<String> = g
            .by_outcome
            .iter()
            .map(|(k, v)| format!("{k} {v}"))
            .collect();
        println!(
            "  {}: {}/{} runs, {}{}{}{}",
            g.tiling,
            g.runs,
            g.rules,
            outs.join(", "),
            g.latest_onset
                .as_ref()
                .map(|(r, o)| format!("; latest highway {r} from step {o}"))
                .unwrap_or_default(),
            g.longest_period
                .as_ref()
                .map(|(r, p)| format!("; longest period {r} p{p}"))
                .unwrap_or_default(),
            g.error
                .as_ref()
                .map(|e| format!(" [error: {e}]"))
                .unwrap_or_default()
        );
    }
    if !p.newest.is_empty() {
        println!("  newest:");
        for f in p.newest.iter().take(10) {
            println!(
                "    {}  {:<22} {:<10} {}{}{}",
                f.id,
                f.rule,
                f.tiling,
                f.outcome.name(),
                f.period.map(|x| format!(" p{x}")).unwrap_or_default(),
                f.onset
                    .map(|x| format!(" from step {x}"))
                    .unwrap_or_default()
            );
        }
    }
}

fn show_rows(rows: &[AntRow], json: bool) {
    if json {
        println!("{}", serde_json::to_string_pretty(rows).unwrap());
        return;
    }
    for r in rows {
        println!("{}", r.line());
    }
}

pub fn run(a: AntsArgs) -> Result<()> {
    let lib = match &a.library {
        Some(p) => Library::open(Library::file_for(p.clone())),
        None => Library::open_default(),
    };
    match a.cmd {
        AntsCmd::Search(s) => search(&lib, *s, a.json),
        AntsCmd::Status { job: Some(j) } => {
            let (_, p) = msg(job::status(&lib, j))?;
            show_progress(&p, a.json);
            Ok(())
        }
        AntsCmd::Status { job: None } => {
            for j in msg(lib.jobs())?.into_iter().filter(|j| j.kind == job::KIND) {
                let (_, p) = msg(job::status(&lib, j.id))?;
                println!("{}", p.line());
                println!(
                    "  status {}, created {}, updated {}",
                    p.status, j.created, j.updated
                );
            }
            Ok(())
        }
        AntsCmd::Stop { job: j } => {
            msg(job::request_stop(&lib, j))?;
            eprintln!("asked ant search {j} to stop at its next batch");
            Ok(())
        }
        AntsCmd::Run(r) => run_one(&lib, r, a.json),
        AntsCmd::List(l) => {
            let q = AntQuery {
                outcome: l
                    .outcome
                    .as_deref()
                    .map(Outcome::parse)
                    .transpose()
                    .map_err(anyhow::Error::msg)?,
                tiling: l.tiling,
                rule: l.rule,
                job: l.job,
                sort: Some(l.sort),
                limit: Some(l.limit),
            };
            show_rows(&msg(lib.ants(&q))?, a.json);
            Ok(())
        }
        AntsCmd::Show { id } => {
            let r = msg(lib.ant(&id))?;
            if a.json {
                println!("{}", serde_json::to_string_pretty(&r)?);
            } else {
                println!("{}", r.line());
                println!(
                    "  rule {} ({} states, {} colours), tiling {}, start {}",
                    r.rule,
                    r.states,
                    r.colors,
                    r.tiling_name,
                    r.start.label()
                );
                println!(
                    "  {} steps, {} coloured cells, score {}, state {}{}",
                    r.steps,
                    r.cells,
                    r.score,
                    r.state,
                    r.name
                        .as_ref()
                        .map(|n| format!(", name {n}"))
                        .unwrap_or_default()
                );
                if let Some(p) = &r.run.proof {
                    println!(
                        "  proved at step {}: period {}, d ({}, {}), {} cells compared",
                        p.at, p.period, p.d[0], p.d[1], p.cells
                    );
                }
                if let Some(s) = &r.run.symmetry {
                    println!(
                        "  {} symmetric fields (steps {} to {}), last {} cells, reflection {:?}",
                        s.count, s.first_step, s.last_step, s.last_cells, s.reflection
                    );
                }
            }
            Ok(())
        }
        AntsCmd::Census { job } => {
            let c = msg(lib.ant_census(job))?;
            if a.json {
                println!("{}", serde_json::to_string_pretty(&c)?);
            } else {
                for t in c {
                    let outs: Vec<String> = t
                        .by_outcome
                        .iter()
                        .map(|(k, v)| format!("{k} {v}"))
                        .collect();
                    println!("{}: {} runs, {}", t.tiling_name, t.runs, outs.join(", "));
                }
            }
            Ok(())
        }
        AntsCmd::Update {
            id,
            name,
            notes,
            rating,
            state,
        } => {
            let r = msg(lib.update_ant(
                &id,
                &AntUpdate {
                    name,
                    notes,
                    rating,
                    state,
                },
            ))?;
            show_rows(&[r], a.json);
            Ok(())
        }
        AntsCmd::Remove { id } => {
            let r = msg(lib.remove_ant(&id))?;
            println!("removed {}", r.line());
            Ok(())
        }
        AntsCmd::Scene { id, out, palette } => {
            let r = msg(lib.ant(&id))?;
            let sc = msg(jetcarp_core::ants::media::build_scene_in(
                &r,
                palette.as_deref(),
            ))?;
            if let Some(d) = out.parent() {
                std::fs::create_dir_all(d)?;
            }
            std::fs::write(&out, sc.to_json())?;
            println!(
                "{} ({} generations of {} steps show the build-up)",
                out.display(),
                jetcarp_core::ants::media::build_gens(&r),
                sc.cas[0].steps_per_gen
            );
            Ok(())
        }
        AntsCmd::Promote { id, palette } => {
            let r = msg(lib.ant(&id))?;
            let sc = msg(jetcarp_core::ants::media::build_scene_in(
                &r,
                palette.as_deref(),
            ))?;
            let mut q = QuickStartRow::for_scene(&r.id, "scene", sc);
            q.behaviour = r.outcome.name().into();
            q.score = Some(r.score);
            msg(lib.put_quickstart(&q))?;
            msg(lib.update_ant(
                &r.id,
                &AntUpdate {
                    state: Some("promoted".into()),
                    ..Default::default()
                },
            ))?;
            println!("Quick Start entry {} ({})", q.id, q.title);
            Ok(())
        }
    }
}

fn run_one(lib: &Library, r: RunArgs, json: bool) -> Result<()> {
    let m = msg(parse_rule(&r.rule))?;
    let t = msg(library::tiling_list(&r.tiling))?;
    let start = match &r.soup {
        None => Start::Empty,
        Some(s) => {
            let p: Vec<&str> = s.split(':').collect();
            if p.len() != 3 {
                bail!("--soup is WxH:density:seed, e.g. 8x8:0.5:1");
            }
            let (w, h) = parse_size(p[0])?;
            Start::Soup {
                w,
                h,
                density: p[1].parse()?,
                state: jetcarp_core::hunt::soup::soup_state(p[2].parse()?, 0),
            }
        }
    };
    let lim = Limits {
        steps: r.steps,
        max_period: r.max_period,
        symmetry: true,
    };
    let t0 = Instant::now();
    let row = msg(job::run_one(&m, &t[0], &start, &lim))?;
    if r.store {
        msg(lib.ants_flush(0, std::slice::from_ref(&row), None, None))?;
    }
    if json {
        println!("{}", serde_json::to_string_pretty(&row)?);
    } else {
        println!("{}", row.line());
        eprintln!(
            "  {} steps in {:.2} s{}",
            row.steps,
            t0.elapsed().as_secs_f64(),
            if r.store { ", stored" } else { "" }
        );
    }
    Ok(())
}

fn search(lib: &Library, a: SearchArgs, json: bool) -> Result<()> {
    let jid = match a.resume {
        Some(j) => {
            let row = msg(lib.job(j))?;
            if row.status == JobStatus::Done {
                bail!("ant search {j} is already done");
            }
            j
        }
        None => {
            let strings = match &a.strings {
                Some(s) => {
                    let (min, max) = parse_range(s)?;
                    Some(Strings {
                        min,
                        max,
                        alphabet: a.alphabet.clone(),
                    })
                }
                None => None,
            };
            let tables = match &a.tables {
                Some(s) => {
                    let (n, k) = parse_size(s)?;
                    Some(Tables {
                        states: n as u8,
                        colors: k as u8,
                        alphabet: a.alphabet.clone(),
                        all_max: a.all_max,
                        sample: a.sample,
                        seed: a.seed,
                    })
                }
                None => None,
            };
            let rules: Vec<String> = a
                .rules
                .as_deref()
                .unwrap_or("")
                .split([';', ' ', '\t', '\n'])
                .map(str::trim)
                .filter(|x| !x.is_empty())
                .map(String::from)
                .collect();
            if strings.is_none() && tables.is_none() && rules.is_empty() {
                bail!("give --strings 2-6, --tables 2x2, or --rules \"RL LLRR\"");
            }
            let soup = match a.soups {
                None => None,
                Some(n) => {
                    let (w, h) = parse_size(&a.size)?;
                    Some(Soups {
                        w,
                        h,
                        density: a.density,
                        soups: n,
                        seed: a.seed,
                    })
                }
            };
            let spec = AntSpec {
                tilings: msg(library::tiling_list(&a.tilings))?,
                rules,
                strings,
                tables,
                soup,
                steps: a.steps,
                max_period: a.max_period,
                symmetry: !a.no_symmetry,
            };
            msg(job::start(lib, &spec))?
        }
    };
    eprintln!("ant search job {jid} in {}", lib.path().display());
    let stop = Arc::new(AtomicBool::new(false));
    let presses = Arc::new(AtomicU32::new(0));
    {
        let (stop, presses) = (stop.clone(), presses.clone());
        let _ = ctrlc::set_handler(move || {
            if presses.fetch_add(1, Ordering::SeqCst) == 0 {
                stop.store(true, Ordering::SeqCst);
                eprintln!(
                    "\nCtrl-C: stopping at the next batch (again to quit now; `jetcarp ants search --resume {jid}` continues)"
                );
            } else {
                std::process::exit(130);
            }
        });
    }
    let tty = std::io::stderr().is_terminal();
    let last = std::sync::Mutex::new(Instant::now());
    let on_progress = Arc::new(move |p: &AntProgress| {
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
    let opts = AntOptions {
        threads: a.threads,
        stop,
        limit: a.limit,
        on_progress: Some(on_progress),
        ..Default::default()
    };
    let p = msg(job::run_job(lib, jid, &opts))?;
    if tty {
        eprintln!();
    }
    show_progress(&p, json);
    if p.status == "paused" {
        eprintln!(
            "ant search {jid} paused at run {}; `jetcarp ants search --resume {jid}` continues",
            p.runs_done
        );
    }
    Ok(())
}
