//! `jetcarp library ants`: the catalog's ant and turmite runs (`jetcarp_core::ants`): list, show,
//! census, curate, remove, the build-up scene, promotion to the Quick Start, and one rule run by
//! itself. Ant searches are jobs: `jetcarp search ants`, followed with `jetcarp jobs`.

use anyhow::{Result, bail};
use jetcarp_api::Ctx;
use jetcarp_api::args;
use jetcarp_core::ants::job;
use jetcarp_core::ants::run::{Limits, Outcome, Start};
use jetcarp_core::ants::search::parse_rule;
use jetcarp_core::library::{AntQuery, AntRow, AntUpdate, Library};
use jetcarp_core::pattern;
use std::path::PathBuf;
use std::time::Instant;

#[derive(clap::Args)]
pub struct AntsArgs {
    #[command(subcommand)]
    cmd: AntsCmd,
}

#[derive(clap::Subcommand)]
enum AntsCmd {
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

struct Opts {
    cmd: AntsCmd,
    json: bool,
}

fn msg<T>(r: Result<T, String>) -> Result<T> {
    r.map_err(anyhow::Error::msg)
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

pub fn run(cx: &Ctx, a: AntsArgs, json: bool) -> Result<()> {
    let lib = cx.library();
    let a = Opts { cmd: a.cmd, json };
    match a.cmd {
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
            use jetcarp_api::op::{Op, Quiet};
            use jetcarp_api::ops::quickstart::{AddIn, QuickstartAdd};
            let add = AddIn {
                ant: Some(id),
                palette,
                ..Default::default()
            };
            crate::show(&QuickstartAdd::run(cx, add, &Quiet)?, a.json)
        }
    }
}

fn run_one(lib: &Library, r: RunArgs, json: bool) -> Result<()> {
    let m = msg(parse_rule(&r.rule))?;
    let t = msg(pattern::tiling_list(&r.tiling))?;
    let start = match &r.soup {
        None => Start::Empty,
        Some(s) => {
            let p: Vec<&str> = s.split(':').collect();
            if p.len() != 3 {
                bail!("--soup is WxH:density:seed, e.g. 8x8:0.5:1");
            }
            let (w, h) = msg(args::size_pair(p[0]))?;
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
