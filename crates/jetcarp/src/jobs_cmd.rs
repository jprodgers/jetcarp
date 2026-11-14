//! `jetcarp jobs`: the catalog's jobs, whatever started them: `list`, `status`, `wait`, `stop`,
//! `resume`, `report`, `remove` (the operations `jobs.*`); and an explore job's run records:
//! `runs`, `sheet`, `scene`, `review`, `clips`, `refit`, `rescore`, `seal` (`search.runs`,
//! `search.sheet`, `search.run_scene`, `search.review`, `search.clips`, `search.refit`,
//! `search.rescore`, `search.seal`).

use crate::search_cmd::CliProgress;
use crate::show;
use anyhow::{Context, Result};
use jetcarp_api::op::{Op, Quiet};
use jetcarp_api::ops::explore::{
    ClipsIn, RefitIn, RescoreIn, ReviewIn, RunIn, RunsIn, SealIn, SearchClips, SearchRefit,
    SearchRescore, SearchReview, SearchRunScene, SearchRuns, SearchSeal, SearchSheet, SheetIn,
};
use jetcarp_api::ops::jobs::{
    JobIn, JobsList, JobsRemove, JobsReport, JobsResume, JobsStatus, JobsStop, JobsWait, ListIn,
    RemoveIn, ReportIn, ResumeIn, StatusIn, WaitIn,
};
use jetcarp_api::ops::search::FilesIn;
use std::path::PathBuf;

#[derive(clap::Args)]
pub struct JobsArgs {
    /// Catalog: a .sqlite file, or a folder holding library.sqlite (default: $JETCARP_LIBRARY,
    /// else <data>/library.sqlite).
    #[arg(long, global = true)]
    library: Option<PathBuf>,
    #[command(subcommand)]
    cmd: JobsCmd,
}

#[derive(clap::Args)]
struct Filters {
    /// Only jobs of this kind: hunt, search, ants, census, mover, pile, collide, evolve, explore.
    #[arg(long)]
    kind: Option<String>,
    /// Only jobs with this status: running, paused, done, failed.
    #[arg(long)]
    status: Option<String>,
    /// Only jobs with this label.
    #[arg(long)]
    label: Option<String>,
    /// Only jobs whose label starts with this.
    #[arg(long)]
    prefix: Option<String>,
    /// At most this many (newest first).
    #[arg(long)]
    limit: Option<usize>,
}

#[derive(clap::Subcommand)]
enum JobsCmd {
    /// The jobs, newest first: id, kind, status, started, progress.
    List(Filters),
    /// One job's progress (no JOB: the list), live when it runs in this process.
    Status {
        job: Option<i64>,
        /// Wait up to this many seconds for a running job to stop running.
        #[arg(long)]
        wait: Option<f64>,
        /// Show this many of its newest and rarest finds.
        #[arg(long)]
        finds: Option<usize>,
        #[command(flatten)]
        filters: Filters,
    },
    /// Wait until a job stops running, in any process. The exit status says how it ended: 0
    /// done, 3 paused (stopped, out of budget, or past its memory cap), 4 failed, 5 still
    /// running at --timeout, 6 gone (marked running, but its process crashed or was killed).
    Wait {
        job: i64,
        /// Give up after this many seconds.
        #[arg(long)]
        timeout: Option<f64>,
        /// No progress on stderr.
        #[arg(long)]
        quiet: bool,
    },
    /// Ask a running job (in any process) to stop at its next checkpoint; it stays resumable.
    Stop { job: i64 },
    /// Carry on a stopped, paused, or cut-off job from its last checkpoint.
    Resume {
        job: i64,
        /// Worker threads (default: all cores; an explore search leaves two free).
        #[arg(long)]
        threads: Option<usize>,
        #[command(flatten)]
        gpu: crate::platform::GpuFlags,
        /// Stop at the next checkpoint after this many minutes.
        #[arg(long)]
        minutes: Option<f64>,
        /// Stop at the next checkpoint after this Unix time (seconds since 1970).
        #[arg(long)]
        until: Option<u64>,
        /// Stop after this many units in this session.
        #[arg(long)]
        limit: Option<u64>,
        /// Run in a process of its own and return at once.
        #[arg(long)]
        detach: bool,
        /// When it ends: the full report as JSON.
        #[arg(long)]
        json_out: Option<PathBuf>,
        /// When it ends: a census's or a collision's table as CSV.
        #[arg(long)]
        csv: Option<PathBuf>,
        /// When it ends: a census's or a collision's contact sheet.
        #[arg(long)]
        sheet: Option<PathBuf>,
        /// No progress on stderr.
        #[arg(long)]
        quiet: bool,
    },
    /// A Markdown report of what a job (or every job with a label prefix) searched, for how
    /// long, and what it found.
    Report {
        job: Option<i64>,
        /// Every job whose label starts with this, instead of one job.
        #[arg(long)]
        prefix: Option<String>,
        /// Write the Markdown here instead of printing it.
        #[arg(long)]
        md: Option<PathBuf>,
        /// Rules or finds listed per row.
        #[arg(long, default_value_t = 5)]
        top: usize,
    },
    /// Delete a job that is not running, with its sightings (and with --delete-results, the
    /// patterns only it found, its ant runs, and its working folder: an explore search's runs).
    Remove {
        job: i64,
        #[arg(long)]
        delete_results: bool,
    },
    /// An explore job's stored runs, best first (JOB default: the newest explore job).
    Runs(Box<RunsArgs>),
    /// A contact sheet of an explore job's runs at several moments, labelled.
    Sheet(Box<SheetArgs>),
    /// An explore job's run as a scene that replays it (printed, or saved with --save).
    Scene {
        job: i64,
        run: i64,
        #[arg(long)]
        save: Option<PathBuf>,
    },
    /// Record a verdict on an explore job's run (keep, maybe, drop), or with --from the ratings
    /// that the clip-rating page saved (`jobs clips --page`).
    Review {
        /// The explore job.
        job: i64,
        /// The run reviewed (its id in the job; `jetcarp jobs runs` lists them).
        run: Option<i64>,
        /// keep, maybe, or drop.
        verdict: Option<String>,
        /// What was seen, in a sentence.
        #[arg(long)]
        note: Option<String>,
        /// Who judged; with --from, instead of the name the ratings file gives (default: the
        /// file's name, else the publisher's name when `jetcarp pack key --name` set one, else
        /// user).
        #[arg(long)]
        rater: Option<String>,
        /// Instead of one run: the ratings file the clip-rating page saved (its verdicts become
        /// reviews, its stars stars), or a folder of rating files (`<run id>.json`); ratings
        /// already recorded from it are skipped.
        #[arg(long)]
        from: Option<PathBuf>,
    },
    /// Animated clips of a stratified sample of an explore job's runs (or chosen runs), with a
    /// manifest, for rating by eye.
    Clips(Box<ClipsArgs>),
    /// Fit a score model to the keep, maybe and drop reviews of explore jobs' runs (JOB...
    /// default: the newest explore job), and report how well it ranks runs held out of the fit.
    Refit(Box<RefitArgs>),
    /// Recompute every stored score of a stopped explore job from its stored measures (after
    /// the score changes, or by a fitted model), and rebuild its archive. Sealed segments are not
    /// rewritten: their new scores go in a sidecar per segment (`scores-<n>/` in the search
    /// folder).
    Rescore {
        job: Option<i64>,
        /// Score with this fitted model (`jetcarp jobs refit --out` writes one); the job keeps
        /// it, so the search goes on with it. `builtin` goes back to the built-in score.
        #[arg(long)]
        score: Option<String>,
        /// First replay the runs that lack measures the score reads (JACA runs stored before
        /// sign measures), caching what they measure. Slow for a large search.
        #[arg(long)]
        replay: bool,
        #[command(flatten)]
        gpu: crate::platform::GpuFlags,
    },
    /// Seal a stopped explore job's newest runs (those not yet in a Parquet segment), so its
    /// folder holds every run in Parquet; a search seals every million runs by itself.
    Seal { job: Option<i64> },
}

#[derive(clap::Args)]
struct RunsArgs {
    job: Option<i64>,
    /// Only this rule family.
    #[arg(long)]
    family: Option<String>,
    /// Only this tiling key.
    #[arg(long)]
    tiling: Option<String>,
    /// died, froze, cycle, or running.
    #[arg(long)]
    fate: Option<String>,
    #[arg(long)]
    min_score: Option<f64>,
    #[arg(long)]
    min_movers: Option<u32>,
    /// 1 screened (default), 2 confirmation runs.
    #[arg(long)]
    stage: Option<u8>,
    /// Only reviewed runs: any, keep, maybe, or drop.
    #[arg(long)]
    reviewed: Option<String>,
    /// These runs (comma separated) instead of the filters.
    #[arg(long, value_delimiter = ',')]
    ids: Option<Vec<i64>>,
    /// score (default), newest, or id (the default with --since).
    #[arg(long)]
    order: Option<String>,
    /// Most runs (0: every run).
    #[arg(long, default_value_t = 40)]
    limit: usize,
    /// Also show these measures: all, or names, comma separated (settle, density_late,
    /// activity_late, smooth, sign_balance, sign_sorted, sign_trade, sign_lost, ...).
    #[arg(long)]
    measures: Option<String>,
    /// text (default), csv, or json.
    #[arg(long, default_value = "text")]
    format: String,
    /// Only runs with a larger id than this, in id order (to export a search in parts).
    #[arg(long)]
    since: Option<i64>,
    /// Write the runs here as CSV instead of printing them.
    #[arg(long)]
    out: Option<PathBuf>,
}

#[derive(clap::Args)]
struct RefitArgs {
    /// The explore jobs whose runs' reviews to learn (default: the newest explore job).
    jobs: Vec<i64>,
    /// Only this rater's reviews (default: every rater's).
    #[arg(long)]
    rater: Option<String>,
    /// Fit to this family's runs only; the model then scores only that family.
    #[arg(long)]
    family: Option<String>,
    /// Write the model here (JSON), for `search explore --score` and `jobs rescore --score`.
    #[arg(long)]
    out: Option<PathBuf>,
    /// Do not replay reviewed runs that lack measures (JACA runs stored before sign measures);
    /// they count at the mean.
    #[arg(long)]
    no_replay: bool,
    #[command(flatten)]
    gpu: crate::platform::GpuFlags,
}

#[derive(clap::Args)]
struct SheetArgs {
    job: Option<i64>,
    /// Where to write the PNG.
    #[arg(long, alias = "out")]
    png: PathBuf,
    /// Runs to show, comma separated (`jetcarp jobs runs` finds them).
    #[arg(long, value_delimiter = ',')]
    ids: Option<Vec<i64>>,
    /// Pixels per thumbnail.
    #[arg(long, default_value_t = 220)]
    px: u32,
    /// Thumbnails per row.
    #[arg(long, default_value_t = 9)]
    cols: u32,
    /// Fractions of each run to show, comma separated.
    #[arg(long, default_value = "0.125,0.5,1", value_delimiter = ',')]
    fracs: Vec<f64>,
    /// A palette; several (comma separated) compare them side by side, one row per run.
    #[arg(long)]
    palette: Option<String>,
    /// Most runs on the sheet.
    #[arg(long, default_value_t = 36)]
    max: usize,
}

#[derive(clap::Args)]
struct ClipsArgs {
    job: Option<i64>,
    /// The folder for the clips and manifest.json.
    #[arg(long)]
    out: PathBuf,
    #[arg(long, default_value_t = 96)]
    sample: usize,
    #[arg(long, default_value_t = 1)]
    seed: u64,
    #[arg(long, default_value_t = 150)]
    px: u32,
    #[arg(long, default_value_t = 28)]
    frames: u32,
    /// Render these runs instead of a sample: a file of one `id[ bucket]` per line.
    #[arg(long)]
    ids: Option<PathBuf>,
    /// Also write index.html, a page for rating the clips by eye: Keep, Maybe, Drop and a star
    /// per clip, with the keyboard too. It works offline; its Save ratings button downloads a
    /// file for `jetcarp jobs review JOB --from FILE`.
    #[arg(long)]
    page: bool,
}

/// `jetcarp jobs wait`'s exit status for how the job ended (1 is any error, 2 a usage error).
pub fn wait_status(ended: &str) -> i32 {
    match ended {
        "done" => 0,
        "paused" => 3,
        "failed" => 4,
        "timeout" => 5,
        _ => 6,
    }
}

fn list_in(f: Filters) -> ListIn {
    ListIn {
        kind: f.kind,
        status: f.status,
        label: f.label,
        prefix: f.prefix,
        limit: f.limit,
    }
}

pub fn run(a: JobsArgs, json: bool) -> Result<()> {
    let lib = a.library.map(jetcarp_store::Library::file_for);
    let cx = crate::platform::ctx().with_library(lib);
    match a.cmd {
        JobsCmd::List(f) => show(&JobsList::run(&cx, list_in(f), &Quiet)?, json),
        JobsCmd::Status {
            job,
            wait,
            finds,
            filters,
        } => {
            let l = list_in(filters);
            let s = StatusIn {
                job,
                wait_s: wait,
                finds,
                kind: l.kind,
                status: l.status,
                label: l.label,
                prefix: l.prefix,
                limit: l.limit,
            };
            let p = CliProgress::new(false);
            let r = JobsStatus::run(&cx, s, &p);
            p.finish();
            show(&r?, json)
        }
        JobsCmd::Wait {
            job,
            timeout,
            quiet,
        } => {
            let p = CliProgress::new(quiet);
            let r = JobsWait::run(
                &cx,
                WaitIn {
                    job,
                    timeout_s: timeout,
                },
                &p,
            );
            p.finish();
            let r = r?;
            show(&r, json)?;
            match wait_status(&r.ended) {
                0 => Ok(()),
                code => {
                    use std::io::Write as _;
                    let _ = std::io::stdout().flush();
                    std::process::exit(code)
                }
            }
        }
        JobsCmd::Stop { job } => show(&JobsStop::run(&cx, JobIn { job }, &Quiet)?, json),
        JobsCmd::Resume {
            job,
            threads,
            gpu,
            minutes,
            until,
            limit,
            detach,
            json_out,
            csv,
            sheet,
            quiet,
        } => {
            let path = |p: Option<PathBuf>| p.map(|p| p.display().to_string());
            let r = ResumeIn {
                job,
                budget: crate::search_cmd::budget(minutes, until, limit),
                threads,
                gpu: gpu.asked(),
                background: false,
                detach,
                files: FilesIn {
                    json: path(json_out),
                    csv: path(csv),
                    sheet: path(sheet),
                    ..Default::default()
                },
            };
            let p = CliProgress::new(quiet);
            let out = JobsResume::run(&cx, r, &p);
            p.finish();
            show(&out?, json)
        }
        JobsCmd::Report {
            job,
            prefix,
            md,
            top,
        } => {
            let r = JobsReport::run(
                &cx,
                ReportIn {
                    job,
                    prefix,
                    top: Some(top),
                },
                &Quiet,
            )?;
            match md {
                Some(p) if !json => {
                    std::fs::write(&p, &r.markdown)
                        .with_context(|| format!("writing {}", p.display()))?;
                    println!("report: {}", p.display());
                    Ok(())
                }
                _ => show(&r, json),
            }
        }
        JobsCmd::Runs(r) => {
            let r = *r;
            let a = RunsIn {
                job: r.job,
                family: r.family,
                tiling: r.tiling,
                fate: r.fate,
                min_score: r.min_score,
                min_movers: r.min_movers,
                stage: r.stage,
                reviewed: r.reviewed,
                ids: r.ids,
                order: r.order,
                limit: Some(r.limit),
                measures: r.measures.map(serde_json::Value::String),
                since: r.since,
                csv: r.out.map(|p| p.display().to_string()),
            };
            match r.format.as_str() {
                "csv" if a.csv.is_none() => {
                    let out = std::io::stdout();
                    let mut w = std::io::BufWriter::new(out.lock());
                    jetcarp_api::ops::explore::runs_csv(&cx, &a, &mut w)?;
                    Ok(())
                }
                "text" | "csv" => show(&SearchRuns::run(&cx, a, &Quiet)?, json),
                "json" => show(&SearchRuns::run(&cx, a, &Quiet)?, true),
                f => anyhow::bail!("--format is text, csv, or json, not {f}"),
            }
        }
        JobsCmd::Sheet(a) => {
            let a = *a;
            let i = SheetIn {
                job: a.job,
                ids: a.ids,
                px: a.px,
                cols: a.cols,
                fracs: Some(a.fracs),
                palette: a.palette,
                max: Some(a.max),
                png: Some(a.png.display().to_string()),
            };
            show(&SearchSheet::run(&cx, i, &Quiet)?, json)
        }
        JobsCmd::Scene { job, run, save } => {
            let i = RunIn {
                job: Some(job),
                run,
                save: save.map(|p| p.display().to_string()),
            };
            show(&SearchRunScene::run(&cx, i, &Quiet)?, json)
        }
        JobsCmd::Review {
            job,
            run,
            verdict,
            note,
            rater,
            from,
        } => {
            let i = ReviewIn {
                job: Some(job),
                run,
                verdict,
                note,
                rater,
                from: from.map(|p| p.display().to_string()),
            };
            show(&SearchReview::run(&cx, i, &Quiet)?, json)
        }
        JobsCmd::Clips(a) => {
            let a = *a;
            let i = ClipsIn {
                job: a.job,
                out: a.out.display().to_string(),
                sample: a.sample,
                seed: a.seed,
                ids: None,
                ids_file: a.ids.map(|p| p.display().to_string()),
                px: a.px,
                frames: a.frames,
                page: a.page,
            };
            let p = CliProgress::new(false);
            let r = SearchClips::run(&cx, i, &p);
            p.finish();
            show(&r?, json)
        }
        JobsCmd::Refit(a) => {
            let a = *a;
            let i = RefitIn {
                jobs: (!a.jobs.is_empty()).then_some(a.jobs),
                rater: a.rater,
                family: a.family,
                out: a.out.map(|p| p.display().to_string()),
                replay: Some(!a.no_replay),
                gpu: a.gpu.asked(),
            };
            let p = CliProgress::new(false);
            let r = SearchRefit::run(&cx, i, &p);
            p.finish();
            show(&r?, json)
        }
        JobsCmd::Rescore {
            job,
            score,
            replay,
            gpu,
        } => {
            let p = CliProgress::new(false);
            let i = RescoreIn {
                job,
                score: score.map(serde_json::Value::String),
                replay: Some(replay),
                gpu: gpu.asked(),
            };
            let r = SearchRescore::run(&cx, i, &p);
            p.finish();
            show(&r?, json)
        }
        JobsCmd::Seal { job } => show(&SearchSeal::run(&cx, SealIn { job }, &Quiet)?, json),
        JobsCmd::Remove {
            job,
            delete_results,
        } => show(
            &JobsRemove::run(
                &cx,
                RemoveIn {
                    job,
                    delete_results,
                },
                &Quiet,
            )?,
            json,
        ),
    }
}
