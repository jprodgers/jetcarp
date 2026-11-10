//! `jetcarp stage1`: the stage 1 settings search (`jetcarp_core::stage1`). Every B/S rule of small
//! spaces and a sample of large ones, scored for visual interest, refined, expanded to rule-space
//! neighbours, stored as settings, and the best put in the Quick Start, as a resumable job.

use anyhow::{Context, Result, bail};
use jetcarp_core::library::{
    JobStatus, Library, SettingRow, SettingStatus, SettingsQuery, tiling_list,
};
use jetcarp_core::stage1::job::{
    self, Base, Neighbours, Refine, Space, Stage1Options, Stage1Progress, Stage1Spec,
};
use jetcarp_core::stage1::score::{FEATURES, features};
use jetcarp_core::stage1::sheet::{SheetOptions, write_sheet};
use jetcarp_core::*;
use std::io::{IsTerminal, Write};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::time::Instant;

#[derive(clap::Args)]
pub struct Stage1Args {
    /// Catalog: a .sqlite file, or a folder holding catalog.sqlite (default: $JETCARP_LIBRARY, else
    /// <data>/catalog.sqlite).
    #[arg(long, global = true)]
    library: Option<PathBuf>,
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(clap::Subcommand)]
enum Cmd {
    /// Start a search and run it (Ctrl-C pauses; `jetcarp stage1 resume JOB` continues).
    Run(Box<RunArgs>),
    /// Continue a paused or interrupted search.
    Resume {
        job: i64,
        #[command(flatten)]
        how: How,
    },
    /// A search's progress, or every search when no job is given.
    Status {
        job: Option<i64>,
        #[arg(long)]
        json: bool,
    },
    /// Ask a running search (in any process) to pause at its next batch.
    Stop { job: i64 },
    /// List settings, best first.
    Top(Box<ListArgs>),
    /// The approval queue: candidates, best first.
    Queue(Box<ListArgs>),
    /// One setting: its descriptors, features, and score terms.
    Show {
        id: String,
        #[arg(long)]
        json: bool,
    },
    /// Approve candidates (each counts as a user rating).
    Approve {
        ids: Vec<String>,
        /// The rating the approval counts as (1 to 5).
        #[arg(long, default_value_t = 4.0)]
        rating: f64,
        #[arg(long, default_value = "")]
        note: String,
    },
    /// Reject candidates (they leave the queue; rated only with --rating).
    Reject {
        ids: Vec<String>,
        #[arg(long)]
        rating: Option<f64>,
        #[arg(long, default_value = "")]
        note: String,
    },
    /// Rate a setting 1 to 5 (as the user, or `--rater model:<name>`).
    Rate {
        id: String,
        rating: f64,
        #[arg(long, default_value = "user")]
        rater: String,
        #[arg(long, default_value = "")]
        note: String,
    },
    /// Refit the score's weights from the ratings and rescore every setting.
    Refit {
        /// Ridge strength (pull towards the current weights).
        #[arg(long, default_value_t = 2.0)]
        lambda: f64,
        #[arg(long)]
        json: bool,
    },
    /// Show the thresholds, or set them (a new score version) and rescore.
    Thresholds {
        #[arg(long)]
        auto: Option<f64>,
        #[arg(long)]
        candidate: Option<f64>,
    },
    /// Score versions, oldest first (the active one is marked).
    Models {
        #[arg(long)]
        json: bool,
    },
    /// Score every setting again with the active model.
    Rescore,
    /// Thin the Quick Start's search picks and the queue: per space and rule family, best
    /// first, skip look-alikes (novelty distance below --min-distance), keep --auto-per-space
    /// auto and --candidate-per-space candidates, store the rest (approved and rejected kept).
    Thin {
        #[arg(long, default_value_t = 6)]
        auto_per_space: usize,
        #[arg(long, default_value_t = 3)]
        candidate_per_space: usize,
        /// Caps on squares and hexagons.
        #[arg(long, default_value_t = 24)]
        auto_main: usize,
        #[arg(long, default_value_t = 12)]
        candidate_main: usize,
        #[arg(long, default_value_t = 0.06)]
        min_distance: f64,
        /// Report only.
        #[arg(long)]
        dry_run: bool,
    },
    /// Contact sheet of settings (the top ones, or --ids) with a JSON sidecar.
    Sheet(Box<SheetArgs>),
}

#[derive(clap::Args, Clone)]
struct How {
    /// Worker threads (0: all cores). Results do not depend on this.
    #[arg(long, default_value_t = 0)]
    threads: usize,
    /// Pause after this many settings in this session.
    #[arg(long)]
    limit: Option<u64>,
    /// Settings per batch (0: adapt). Results do not depend on this.
    #[arg(long, default_value_t = 0)]
    batch: u64,
    /// Print the final progress as JSON.
    #[arg(long)]
    json: bool,
}

#[derive(clap::Args)]
struct RunArgs {
    /// Tilings: family ids, catalog:<id>, or sets (archimedean, all-periodic, ...).
    #[arg(long, default_value = "hex")]
    tilings: String,
    /// Neighbourhoods: touch, edge, or both.
    #[arg(long, default_value = "touch")]
    nb: String,
    /// Generations state counts (2 = B/S), comma separated, e.g. 2,3,4.
    #[arg(long, default_value = "2")]
    states: String,
    /// Birth digits, like 1-6 (default: 1 to the tiling's degree).
    #[arg(long)]
    birth: Option<String>,
    /// Survival digits, like 0-6 (default: 0 to the tiling's degree).
    #[arg(long)]
    survive: Option<String>,
    /// Explicit rules instead of a digit space (spaces or semicolons).
    #[arg(long)]
    rules: Option<String>,
    /// Every rule of a space up to this many; above, a seeded sample.
    #[arg(long, default_value_t = 131_072)]
    all_max: u64,
    /// Sample size for large spaces.
    #[arg(long, default_value_t = 20_000)]
    sample: usize,
    #[arg(long, default_value_t = 0.35)]
    density: f64,
    /// Soup size: 1 fills the grid; less is a centred disc of that radius.
    #[arg(long, default_value_t = 1.0)]
    soup: f64,
    /// Grid size in unit cells.
    #[arg(long, default_value = "64x64")]
    grid: String,
    /// wrap or dead.
    #[arg(long, default_value = "wrap")]
    boundary: String,
    #[arg(long, default_value_t = 1000)]
    gens: u64,
    #[arg(long, default_value_t = 4)]
    soups: u32,
    #[arg(long, default_value_t = 1)]
    seed: u64,
    /// Settings refined per space (0: no refinement).
    #[arg(long, default_value_t = 24)]
    refine_top: usize,
    #[arg(long, default_value = "0.15,0.25,0.5")]
    refine_densities: String,
    #[arg(long, default_value = "0.4")]
    refine_soups: String,
    #[arg(long, default_value = "128x128")]
    refine_grids: String,
    #[arg(long, default_value = "3000")]
    refine_gens: String,
    /// Rules expanded to their neighbours per space and round (0: none).
    #[arg(long, default_value_t = 24)]
    neighbours_top: usize,
    #[arg(long, default_value_t = 1)]
    rounds: u32,
    #[command(flatten)]
    how: How,
}

#[derive(clap::Args)]
struct ListArgs {
    /// Statuses (stored, candidate, auto, approved, rejected), comma separated.
    #[arg(long)]
    status: Option<String>,
    #[arg(long)]
    job: Option<i64>,
    /// Tiling family id or name.
    #[arg(long)]
    tiling: Option<String>,
    #[arg(long)]
    rule: Option<String>,
    /// lifelike or generations.
    #[arg(long)]
    family: Option<String>,
    #[arg(long)]
    behaviour: Option<String>,
    #[arg(long)]
    min_score: Option<f64>,
    #[arg(long, default_value_t = 20)]
    limit: usize,
    #[arg(long)]
    json: bool,
}

#[derive(clap::Args)]
struct SheetArgs {
    #[command(flatten)]
    list: ListArgs,
    /// Settings to draw (ids, comma separated) instead of the list's top ones.
    #[arg(long, value_delimiter = ',')]
    ids: Vec<String>,
    /// A label per setting, comma separated (default: #id).
    #[arg(long, value_delimiter = ',')]
    labels: Vec<String>,
    /// Output PNG; the sidecar is the same name with .json.
    #[arg(long)]
    png: PathBuf,
    #[arg(long, default_value_t = 240)]
    thumb: u32,
    #[arg(long, default_value_t = 6)]
    cols: u32,
    /// Generations shown side by side, as fractions of the setting's generations.
    #[arg(long, default_value = "0.1,1")]
    at: String,
    #[arg(long, default_value_t = 2)]
    label_scale: u32,
    /// Draw every setting in this palette instead of its own (`jetcarp palettes` lists them).
    #[arg(long)]
    palette: Option<String>,
}

fn msg<T>(r: Result<T, String>) -> Result<T> {
    r.map_err(anyhow::Error::msg)
}

fn size(s: &str) -> Result<[u32; 2]> {
    let (w, h) = s
        .split_once(['x', 'X'])
        .map(|(a, b)| (a.trim(), b.trim()))
        .unwrap_or((s.trim(), s.trim()));
    Ok([w.parse().context("width")?, h.parse().context("height")?])
}

fn list<T: std::str::FromStr>(s: &str) -> Result<Vec<T>> {
    s.split([',', ' '])
        .map(str::trim)
        .filter(|x| !x.is_empty())
        .map(|x| {
            x.parse::<T>()
                .map_err(|_| anyhow::anyhow!("bad value '{x}'"))
        })
        .collect()
}

fn boundary(s: &str) -> Result<Boundary> {
    Ok(match s {
        "wrap" => Boundary::Wrap,
        "dead" => Boundary::Dead,
        x => bail!("--boundary is wrap or dead, not {x}"),
    })
}

fn query(a: &ListArgs, default_status: &[SettingStatus]) -> Result<SettingsQuery> {
    let status = match &a.status {
        Some(s) => s
            .split(',')
            .filter(|x| !x.trim().is_empty())
            .map(|x| msg(SettingStatus::parse(x)))
            .collect::<Result<Vec<_>>>()?,
        None => default_status.to_vec(),
    };
    Ok(SettingsQuery {
        status,
        job: a.job,
        tiling: a.tiling.clone(),
        rule: a.rule.clone(),
        family: a.family.clone(),
        behaviour: a.behaviour.clone(),
        min_score: a.min_score,
        limit: a.limit,
        ..Default::default()
    })
}

fn print_rows(rows: &[SettingRow], json: bool) {
    if json {
        println!("{}", serde_json::to_string_pretty(rows).unwrap());
        return;
    }
    println!(
        "{:>7} {:>6} {:9} {:22} {:12} {:9} {:>5} {:>5}  behaviour",
        "id", "score", "status", "rule", "tiling", "grid", "dens", "gens"
    );
    for r in rows {
        let s = &r.setting;
        println!(
            "{:>7} {:>6.1} {:9} {:22} {:12} {:9} {:>5} {:>5}  {}{}",
            r.id,
            r.score.unwrap_or(0.0),
            r.status.name(),
            r.notation,
            r.tiling_name,
            format!(
                "{}x{}{}",
                s.grid[0],
                s.grid[1],
                if s.boundary == Boundary::Dead {
                    "d"
                } else {
                    ""
                }
            ),
            s.density,
            s.gens,
            r.descriptors.tags.join(", "),
            if s.soup < 1.0 {
                format!(" (soup {})", s.soup)
            } else {
                String::new()
            }
        );
    }
}

fn show_progress(p: &Stage1Progress, json: bool) {
    if json {
        println!("{}", serde_json::to_string_pretty(p).unwrap());
        return;
    }
    println!("{}", p.line());
    println!(
        "  status {}, passes {}, {:.0} s run, statuses {:?}",
        p.status,
        p.passes.join(" > "),
        p.elapsed_s,
        p.statuses
    );
    for s in &p.spaces {
        println!(
            "  {}: {} rules, {} evaluated, {} auto, {} candidates, best {:.1} {}",
            s.space, s.rules, s.evaluated, s.auto, s.candidate, s.best_score, s.best_rule
        );
    }
    for b in p.best.iter().take(10) {
        println!(
            "    {:>6.1} #{} {} on {} ({}) [{}]",
            b.score, b.id, b.rule, b.tiling, b.behaviour, b.status
        );
    }
}

fn run_job(lib: &Library, jid: i64, how: &How) -> Result<()> {
    eprintln!("stage 1 job {jid} in {}", lib.path().display());
    let stop = Arc::new(AtomicBool::new(false));
    let presses = Arc::new(AtomicU32::new(0));
    {
        let (stop, presses) = (stop.clone(), presses.clone());
        let _ = ctrlc::set_handler(move || {
            if presses.fetch_add(1, Ordering::SeqCst) == 0 {
                stop.store(true, Ordering::SeqCst);
                eprintln!(
                    "\nCtrl-C: pausing at the next batch (press again to quit now; `jetcarp stage1 resume {jid}` continues)"
                );
            } else {
                std::process::exit(130);
            }
        });
    }
    let tty = std::io::stderr().is_terminal();
    let last = std::sync::Mutex::new(Instant::now());
    let on_progress = Arc::new(move |p: &Stage1Progress| {
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
    let opts = Stage1Options {
        threads: how.threads,
        stop,
        limit: how.limit,
        batch: how.batch,
        on_progress: Some(on_progress),
        ..Default::default()
    };
    let p = msg(job::run(lib, jid, &opts))?;
    if tty {
        eprintln!();
    }
    show_progress(&p, how.json);
    if p.status == "paused" {
        eprintln!("stage 1 job {jid} paused; `jetcarp stage1 resume {jid}` continues");
    }
    Ok(())
}

pub fn run(a: Stage1Args) -> Result<()> {
    let lib = match &a.library {
        Some(p) => Library::open(p.clone()),
        None => Library::open_default(),
    };
    match a.cmd {
        Cmd::Run(r) => {
            let r = *r;
            let tilings = msg(tiling_list(&r.tilings))?;
            let nbs: Vec<Nb> = match r.nb.as_str() {
                "touch" | "t" => vec![Nb::Touch],
                "edge" | "e" => vec![Nb::Edge],
                "both" => vec![Nb::Touch, Nb::Edge],
                x => bail!("--nb is touch, edge, or both, not {x}"),
            };
            let states: Vec<u8> = list(&r.states)?;
            let digits = |s: &Option<String>| -> Result<Option<Vec<u32>>> {
                s.as_deref()
                    .map(|d| msg(census::parse_digits(d)))
                    .transpose()
            };
            let rules: Vec<String> = r
                .rules
                .as_deref()
                .map(census::parse_rule_list)
                .unwrap_or_default()
                .into_iter()
                .flat_map(|l| l.split_whitespace().map(String::from).collect::<Vec<_>>())
                .collect();
            let mut spaces = Vec::new();
            for t in &tilings {
                for &nb in &nbs {
                    for &st in &states {
                        let mut sp = Space::new(t.clone(), nb);
                        sp.states = st;
                        sp.birth = digits(&r.birth)?;
                        sp.survive = digits(&r.survive)?;
                        sp.rules = rules.clone();
                        sp.all_max = Some(r.all_max);
                        sp.sample = Some(r.sample);
                        spaces.push(sp);
                    }
                }
            }
            let spec = Stage1Spec {
                spaces,
                base: Base {
                    density: r.density,
                    soup: r.soup,
                    grid: size(&r.grid)?,
                    boundary: boundary(&r.boundary)?,
                    gens: r.gens,
                    soups: r.soups,
                    seed: r.seed,
                },
                refine: Refine {
                    top: r.refine_top,
                    densities: list(&r.refine_densities)?,
                    soups: list(&r.refine_soups)?,
                    grids: r
                        .refine_grids
                        .split([',', ' '])
                        .filter(|x| !x.trim().is_empty())
                        .map(size)
                        .collect::<Result<_>>()?,
                    gens: list(&r.refine_gens)?,
                },
                neighbours: Neighbours {
                    top: r.neighbours_top,
                    rounds: r.rounds,
                },
            };
            let jid = msg(job::start(&lib, &spec))?;
            run_job(&lib, jid, &r.how)
        }
        Cmd::Resume { job: j, how } => {
            let row = msg(lib.job(j))?;
            if row.status == JobStatus::Done {
                bail!("stage 1 job {j} is already done");
            }
            run_job(&lib, j, &how)
        }
        Cmd::Status { job: Some(j), json } => {
            let (_, p) = msg(job::status(&lib, j))?;
            show_progress(&p, json);
            Ok(())
        }
        Cmd::Status { job: None, json } => {
            let jobs: Vec<_> = msg(lib.jobs())?
                .into_iter()
                .filter(|j| j.kind == "stage1")
                .collect();
            if json {
                let ps: Vec<Stage1Progress> = jobs
                    .iter()
                    .filter_map(|j| job::status(&lib, j.id).ok().map(|x| x.1))
                    .collect();
                println!("{}", serde_json::to_string_pretty(&ps).unwrap());
            } else if jobs.is_empty() {
                println!("no stage 1 searches in {}", lib.path().display());
            } else {
                for j in jobs {
                    if let Ok((_, p)) = job::status(&lib, j.id) {
                        println!("{}  [{}; created {}]", p.line(), p.status, j.created);
                    }
                }
            }
            Ok(())
        }
        Cmd::Stop { job: j } => {
            msg(job::request_stop(&lib, j))?;
            eprintln!("asked stage 1 job {j} to pause at its next batch");
            Ok(())
        }
        Cmd::Top(l) => {
            let rows = msg(lib.settings(&query(&l, &[])?))?;
            print_rows(&rows, l.json);
            Ok(())
        }
        Cmd::Queue(l) => {
            let rows = msg(lib.settings(&query(&l, &[SettingStatus::Candidate])?))?;
            print_rows(&rows, l.json);
            Ok(())
        }
        Cmd::Show { id, json } => {
            let r = msg(lib.setting(&id))?;
            let m = msg(lib.score_model())?;
            let f = features(&r.descriptors, r.setting.soup < 1.0);
            if json {
                let v = serde_json::json!({
                    "setting": r,
                    "features": FEATURES.iter().zip(f).map(|(n, x)| (n.to_string(), x)).collect::<std::collections::BTreeMap<_, _>>(),
                    "terms": m.terms(&f),
                    "score_now": m.score(&f),
                    "scene": r.setting.scene(0).ok(),
                });
                println!("{}", serde_json::to_string_pretty(&v).unwrap());
                return Ok(());
            }
            print_rows(std::slice::from_ref(&r), false);
            println!("  {}", r.setting.label());
            println!("  pass {}, job {:?}, model {:?}", r.pass, r.job, r.model_id);
            let d = &r.descriptors;
            println!(
                "  lives: died {:.2}, filled {:.2}, froze early {:.2}, settled {:.2}, active {:.2}, transient {:.2}, median settle {:?}, longest run period {}",
                d.died,
                d.filled,
                d.froze_early,
                d.settled,
                d.active,
                d.transient,
                d.median_settle,
                d.longest_run_period
            );
            println!(
                "  looks: activity mean {:.4} end {:.4}, final density {:.3}, compress {:.2} (mid {:.2}), entropy {:.2} (mid {:.2}), growth {:.2}",
                d.activity_mean,
                d.activity_end,
                d.final_density,
                d.compress,
                d.compress_mid,
                d.entropy,
                d.entropy_mid,
                d.growth
            );
            println!(
                "  structure: {:.1} objects per soup, {} distinct ({} ship kinds, {} oscillator kinds, {} still lifes), longest period {}, {} symmetric, {} soups censused",
                d.objects,
                d.distinct,
                d.ship_kinds,
                d.oscillator_kinds,
                d.still_kinds,
                d.longest_period,
                d.symmetric,
                d.censused
            );
            println!(
                "  holds up: spread {:.3}, density spread {:.3}; novelty {:?}",
                d.spread, d.density_spread, d.novelty
            );
            println!("  score now {:.1} (model {}):", m.score(&f), m.id);
            for (n, t) in m.terms(&f) {
                println!("    {n:15} {t:6.2}");
            }
            Ok(())
        }
        Cmd::Approve { ids, rating, note } => {
            for id in ids {
                let r = msg(lib.review_setting(&id, SettingStatus::Approved, Some(rating), &note))?;
                println!("approved {} ({}): {}", r.id, r.entry_id(), r.notation);
            }
            Ok(())
        }
        Cmd::Reject { ids, rating, note } => {
            for id in ids {
                let r = msg(lib.review_setting(&id, SettingStatus::Rejected, rating, &note))?;
                println!("rejected {}: {}", r.id, r.notation);
            }
            Ok(())
        }
        Cmd::Rate {
            id,
            rating,
            rater,
            note,
        } => {
            let n = msg(lib.rate("setting", &id, &rater, rating, &note))?;
            println!("rating {n}: setting {id} = {rating} by {rater}");
            Ok(())
        }
        Cmd::Refit { lambda, json } => {
            let (m, rep) = msg(lib.refit_score(lambda))?;
            if json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&serde_json::json!({"model": m, "rescore": rep}))
                        .unwrap()
                );
            } else {
                println!(
                    "score model {} from {} ratings of {} settings (lambda {}): rmse {:?} -> {:?}",
                    m.id, m.ratings, m.rated_settings, m.lambda, m.rmse_before, m.rmse_after
                );
                for (k, v) in &m.weights {
                    println!("  {k:15} {v:8.3}");
                }
                println!(
                    "  bias {:.3}; rescored {} settings: {:?}",
                    m.bias, rep.settings, rep.statuses
                );
            }
            Ok(())
        }
        Cmd::Thresholds { auto, candidate } => {
            let m = msg(lib.score_model())?;
            if auto.is_none() && candidate.is_none() {
                println!(
                    "auto {} candidate {} (score model {}, {})",
                    m.auto, m.candidate, m.id, m.source
                );
                return Ok(());
            }
            let (m, rep) =
                msg(lib.set_thresholds(auto.unwrap_or(m.auto), candidate.unwrap_or(m.candidate)))?;
            println!(
                "score model {}: auto {} candidate {}; rescored {} settings: {:?}",
                m.id, m.auto, m.candidate, rep.settings, rep.statuses
            );
            Ok(())
        }
        Cmd::Models { json } => {
            let ms = msg(lib.score_models())?;
            if json {
                println!("{}", serde_json::to_string_pretty(&ms).unwrap());
            } else if ms.is_empty() {
                println!("no stored score models (the hand-set one is in use)");
            } else {
                for (m, active) in ms {
                    println!(
                        "{} {:2} {:10} auto {} candidate {} ratings {} rmse {:?} -> {:?} {}",
                        if active { "*" } else { " " },
                        m.id,
                        m.source,
                        m.auto,
                        m.candidate,
                        m.ratings,
                        m.rmse_before,
                        m.rmse_after,
                        m.created
                    );
                }
            }
            Ok(())
        }
        Cmd::Thin {
            auto_per_space,
            candidate_per_space,
            auto_main,
            candidate_main,
            min_distance,
            dry_run,
        } => {
            let r = msg(lib.thin_quickstart(&jetcarp_core::library::ThinOptions {
                auto_per_space,
                candidate_per_space,
                auto_main,
                candidate_main,
                min_distance,
                dry_run,
            }))?;
            println!(
                "{}{} spaces: before {:?}, after {:?}; {} look-alikes, {} changed",
                if dry_run { "(dry run) " } else { "" },
                r.spaces,
                r.before,
                r.after,
                r.look_alikes,
                r.changed
            );
            Ok(())
        }
        Cmd::Rescore => {
            let m = msg(lib.score_model())?;
            let rep = msg(lib.rescore(&m))?;
            println!(
                "rescored {} settings with model {}: {:?}",
                rep.settings, m.id, rep.statuses
            );
            Ok(())
        }
        Cmd::Sheet(s) => {
            let s = *s;
            let rows: Vec<SettingRow> = if s.ids.is_empty() {
                msg(lib.settings(&query(&s.list, &[])?))?
            } else {
                s.ids
                    .iter()
                    .map(|i| msg(lib.setting(i)))
                    .collect::<Result<_>>()?
            };
            let o = SheetOptions {
                thumb: s.thumb,
                cols: s.cols,
                labels: (!s.labels.is_empty()).then(|| s.labels.clone()),
                at: list(&s.at)?,
                label_scale: s.label_scale,
                palette: s.palette.clone(),
            };
            let js = msg(write_sheet(&rows, &o, &s.png))?;
            println!("wrote {} and {}", s.png.display(), js.display());
            Ok(())
        }
    }
}
