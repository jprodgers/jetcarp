//! `jetcarp jobs`: the catalog's jobs, whatever started them: `list`, `status`, `stop`,
//! `resume`, `report`, `remove` (the operations `jobs.*`); and an explore job's run records:
//! `runs`, `sheet`, `scene`, `review`, `clips`, `rescore` (`search.runs`, `search.sheet`,
//! `search.run_scene`, `search.review`, `search.clips`, `search.rescore`).

use crate::search_cmd::CliProgress;
use crate::show;
use anyhow::{Context, Result};
use jetcarp_api::op::{Op, Quiet};
use jetcarp_api::ops::explore::{
    ClipsIn, RescoreIn, ReviewIn, RunIn, RunsIn, SearchClips, SearchRescore, SearchReview,
    SearchRunScene, SearchRuns, SearchSheet, SheetIn,
};
use jetcarp_api::ops::jobs::{
    JobIn, JobsList, JobsRemove, JobsReport, JobsResume, JobsStatus, JobsStop, ListIn, RemoveIn,
    ReportIn, ResumeIn, StatusIn,
};
use jetcarp_api::ops::search::FilesIn;
use std::path::PathBuf;

#[derive(clap::Args)]
pub struct JobsArgs {
    /// Catalog: a .sqlite file, or a folder holding catalog.sqlite (default: $JETCARP_LIBRARY,
    /// else <data>/catalog.sqlite).
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
    /// Ask a running job (in any process) to stop at its next checkpoint; it stays resumable.
    Stop { job: i64 },
    /// Carry on a stopped, paused, or cut-off job from its last checkpoint.
    Resume {
        job: i64,
        /// Worker threads (default: all cores).
        #[arg(long)]
        threads: Option<usize>,
        /// Hunts: the start of each soup on the GPU; piles: topple on the GPU; explore: the GPU
        /// batch stepper, when there is one.
        #[arg(long)]
        gpu: bool,
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
    /// in a folder of rating files (`<run id>.json`, as the clip-rating page saves them).
    Review {
        job: i64,
        run: Option<i64>,
        verdict: Option<String>,
        #[arg(long)]
        note: Option<String>,
        /// Who judged (default: user).
        #[arg(long)]
        rater: Option<String>,
        #[arg(long)]
        from: Option<PathBuf>,
    },
    /// Animated clips of a stratified sample of an explore job's runs (or chosen runs), with a
    /// manifest, for rating by eye.
    Clips(Box<ClipsArgs>),
    /// Recompute every stored score of a stopped explore job from its stored measures (after
    /// the score changes), and rebuild its archive.
    Rescore { job: Option<i64> },
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
    /// One SELECT returning run ids instead of the filters (`SELECT id FROM runs WHERE ...`).
    #[arg(long)]
    sql: Option<String>,
    /// score (default), newest, or id.
    #[arg(long)]
    order: Option<String>,
    #[arg(long, default_value_t = 40)]
    limit: usize,
}

#[derive(clap::Args)]
struct SheetArgs {
    job: Option<i64>,
    /// Where to write the PNG.
    #[arg(long, alias = "out")]
    png: PathBuf,
    /// Runs to show, comma separated.
    #[arg(long, value_delimiter = ',')]
    ids: Option<Vec<i64>>,
    /// One SELECT returning run ids (`SELECT id FROM runs WHERE ...`).
    #[arg(long)]
    sql: Option<String>,
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
    let lib = a.library.map(jetcarp_core::library::Library::file_for);
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
                gpu,
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
                sql: r.sql,
                order: r.order,
                limit: Some(r.limit),
            };
            show(&SearchRuns::run(&cx, a, &Quiet)?, json)
        }
        JobsCmd::Sheet(a) => {
            let a = *a;
            let i = SheetIn {
                job: a.job,
                ids: a.ids,
                sql: a.sql,
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
            };
            let p = CliProgress::new(false);
            let r = SearchClips::run(&cx, i, &p);
            p.finish();
            show(&r?, json)
        }
        JobsCmd::Rescore { job } => {
            let p = CliProgress::new(false);
            let r = SearchRescore::run(&cx, RescoreIn { job }, &p);
            p.finish();
            show(&r?, json)
        }
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
