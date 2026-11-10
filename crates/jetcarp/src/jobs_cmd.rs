//! `jetcarp jobs`: the catalog's jobs, whatever started them: `list`, `status`, `stop`,
//! `resume`, `report`, `remove` (the operations `jobs.*`).

use crate::search_cmd::CliProgress;
use crate::show;
use anyhow::{Context, Result};
use jetcarp_api::op::{Op, Quiet};
use jetcarp_api::ops::jobs::{
    JobIn, JobsList, JobsRemove, JobsReport, JobsResume, JobsStatus, JobsStop, ListIn, RemoveIn,
    ReportIn, ResumeIn, StatusIn,
};
use jetcarp_api::ops::search::{BudgetIn, FilesIn};
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
    /// Only jobs of this kind: hunt, search, ants, census, mover, pile, collide, evolve.
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
        /// Hunts: the start of each soup on the GPU; piles: topple on the GPU.
        #[arg(long)]
        gpu: bool,
        /// Stop at the next checkpoint after this many minutes.
        #[arg(long)]
        minutes: Option<f64>,
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
    /// patterns only it found and its ant runs).
    Remove {
        job: i64,
        #[arg(long)]
        delete_results: bool,
    },
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
                budget: (minutes.is_some() || limit.is_some()).then_some(BudgetIn {
                    minutes,
                    items: limit,
                }),
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
