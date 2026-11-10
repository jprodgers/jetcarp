//! `jetcarp pipeline`: the catalog pipeline (Phase 4 deliverable 9) as one resumable job.
//!
//! Steps (see `jetcarp_core::pipeline`): stage 1 on every grid (`scripts/stage1-all.rhai`),
//! contact sheets and the local model's shortlist (`tools/curate_settings.py`), the score refit,
//! stage 2 hunts (`scripts/stage2-hunt.rhai`), media, and the website export. The pipeline is a
//! `jobs` row of kind `pipeline` recording each step's status, time, and child jobs; a rerun
//! (`jetcarp pipeline resume`) skips the steps that are done and resumes the one that was
//! interrupted (its jobs are found again by label). Pausing it from the app's Hunts window or
//! with `jetcarp pipeline stop` stops the running step at its next checkpoint.

use anyhow::{Context, Result, bail};
use jetcarp_core::library::{
    CurationState, JobStatus, Library, PatternClass, QsStatus, SettingsQuery,
};
use jetcarp_core::media::{self, MediaOptions, MediaTarget};
use jetcarp_core::pipeline::{self, PipelineProgress, PipelineSpec, STEPS, StepState, step_title};
use jetcarp_core::stage1::sheet::{SheetOptions, write_sheet};
use jetcarp_core::website::{self, ExportFilter, ExportOptions};
use serde_json::{Value, json};
use std::collections::{BTreeMap, HashMap};
use std::io::{IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

const STAGE1_SCRIPT: &str = include_str!("../../jetcarp-script/scripts/stage1-all.rhai");
const STAGE2_SCRIPT: &str = include_str!("../../jetcarp-script/scripts/stage2-hunt.rhai");
const SITE_INDEX: &str = include_str!("../../../tools/site-example/index.html");

#[derive(clap::Args)]
pub struct PipelineArgs {
    /// Catalog: a .sqlite file, or a folder holding catalog.sqlite (default: $JETCARP_LIBRARY, else
    /// <data>/catalog.sqlite).
    #[arg(long, global = true)]
    library: Option<PathBuf>,
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(clap::Subcommand)]
enum Cmd {
    /// Start a pipeline and run it (Ctrl-C pauses; `jetcarp pipeline resume` continues).
    Run(Box<RunArgs>),
    /// Continue the newest unfinished pipeline, or this one.
    Resume {
        job: Option<i64>,
        /// Run these steps again even if done (comma separated: stage1,curate,refit,stage2,
        /// media,site).
        #[arg(long, value_delimiter = ',')]
        redo: Vec<String>,
        /// Worker threads (0: all cores).
        #[arg(long)]
        threads: Option<u32>,
        /// Settings the model rates in this session's shortlist step.
        #[arg(long)]
        rate: Option<u32>,
        /// Script parameters (`--set soups=2000`), kept in the pipeline's spec: they change
        /// the jobs not started yet (a started job keeps its own spec).
        #[arg(long = "set", value_name = "KEY=VALUE")]
        set: Vec<String>,
        #[command(flatten)]
        tools: Tools,
    },
    /// A pipeline's steps, or every pipeline when no job is given.
    Status {
        job: Option<i64>,
        #[arg(long)]
        json: bool,
    },
    /// Ask a running pipeline (in any process) to pause at its next checkpoint.
    Stop { job: i64 },
    /// The science: per space and per hunted rule, from a pipeline's jobs (Markdown).
    Report {
        job: Option<i64>,
        /// Write the Markdown here instead of stdout.
        #[arg(long)]
        md: Option<PathBuf>,
        /// Rules listed per space.
        #[arg(long, default_value_t = 5)]
        top: usize,
    },
}

#[derive(clap::Args, Clone)]
struct Tools {
    /// The repository's tools folder (for tools/curate_settings.py; default: found from this
    /// program's path, or $JETCARP_TOOLS).
    #[arg(long)]
    tools: Option<PathBuf>,
    /// Python interpreter for the model shortlist.
    #[arg(long, default_value = "python3")]
    python: String,
}

#[derive(clap::Args)]
struct RunArgs {
    /// Budgets: test (seconds), quick (minutes), standard, or full (multi-day).
    #[arg(long, default_value = "quick")]
    mode: String,
    /// Script parameter for both stage scripts (repeatable): --set sample_triangle=50000
    #[arg(long = "set", value_name = "KEY=VALUE")]
    set: Vec<String>,
    /// Label prefix of the pipeline's jobs (default: "pipeline <id>").
    #[arg(long)]
    label: Option<String>,
    /// Local vision model for the shortlist.
    #[arg(long, default_value = "qwen/qwen3.8-27b")]
    model: String,
    /// Its OpenAI-compatible server.
    #[arg(long, default_value = "http://localhost:1234/v1")]
    base_url: String,
    /// No model: the shortlist's deterministic stand-in rates (ratings `model:stub`).
    #[arg(long)]
    stub: bool,
    /// Settings the model rates in this run (default by mode: 16, 48, 160, 400).
    #[arg(long)]
    rate: Option<u32>,
    /// Website export folder (default <catalog folder>/site).
    #[arg(long)]
    site: Option<PathBuf>,
    /// Patterns given media, best first (default by mode; 0: all).
    #[arg(long)]
    media_limit: Option<u32>,
    /// Worker threads (0: all cores).
    #[arg(long, default_value_t = 0)]
    threads: u32,
    /// Only these steps; the others are marked skipped (comma separated).
    #[arg(long, value_delimiter = ',')]
    steps: Vec<String>,
    #[command(flatten)]
    tools: Tools,
}

fn err(e: String) -> anyhow::Error {
    anyhow::anyhow!(e)
}

fn open(p: &Option<PathBuf>) -> Library {
    match p {
        Some(p) => Library::open(Library::file_for(p.clone())),
        None => Library::open_default(),
    }
}

pub fn pipeline(a: PipelineArgs) -> Result<()> {
    let lib = open(&a.library);
    match a.cmd {
        Cmd::Run(r) => {
            let mode = r.mode.as_str();
            if !["test", "quick", "standard", "full"].contains(&mode) {
                bail!("--mode is test, quick, standard, or full");
            }
            for s in &r.steps {
                if !STEPS.contains(&s.as_str()) {
                    bail!("unknown step {s}; steps are {}", STEPS.join(", "));
                }
            }
            let mut params = BTreeMap::new();
            for kv in &r.set {
                let (k, v) = kv
                    .split_once('=')
                    .with_context(|| format!("--set wants KEY=VALUE, got '{kv}'"))?;
                params.insert(k.trim().to_string(), v.to_string());
            }
            let pick = |t: u32, q: u32, s: u32, f: u32| match mode {
                "test" => t,
                "quick" => q,
                "standard" => s,
                _ => f,
            };
            let spec = PipelineSpec {
                mode: r.mode.clone(),
                label: r.label.clone().unwrap_or_default(),
                params,
                model: r.model.clone(),
                base_url: r.base_url.clone(),
                stub: r.stub,
                rate: r.rate.unwrap_or(pick(8, 48, 160, 400)),
                site: r
                    .site
                    .clone()
                    .unwrap_or_else(|| lib.folder().join("site"))
                    .display()
                    .to_string(),
                media_limit: r.media_limit.unwrap_or(pick(40, 300, 2000, 0)),
                threads: r.threads,
            };
            let job = pipeline::start(&lib, &spec).map_err(err)?;
            if !r.steps.is_empty() {
                let (_, mut p) = pipeline::load(&lib, job).map_err(err)?;
                for st in &mut p.steps {
                    if !r.steps.contains(&st.name) {
                        st.status = "skipped".into();
                        st.note = "not selected".into();
                    }
                }
                pipeline::save(&lib, job, &p, JobStatus::Paused).map_err(err)?;
            }
            println!(
                "pipeline job {job} ({} mode) in {}",
                spec.mode,
                lib.path().display()
            );
            run(&lib, job, &[], None, None, &[], &r.tools)
        }
        Cmd::Resume {
            job,
            redo,
            threads,
            rate,
            set,
            tools,
        } => {
            let job = match job {
                Some(j) => j,
                None => pipeline::latest_unfinished(&lib)
                    .map_err(err)?
                    .context("no unfinished pipeline in this catalog")?,
            };
            run(&lib, job, &redo, threads, rate, &set, &tools)
        }
        Cmd::Status { job, json } => {
            let ids: Vec<i64> = match job {
                Some(j) => vec![j],
                None => lib
                    .jobs()
                    .map_err(err)?
                    .into_iter()
                    .filter(|j| j.kind == "pipeline")
                    .map(|j| j.id)
                    .collect(),
            };
            let mut out = Vec::new();
            for id in ids {
                let (spec, p) = pipeline::load(&lib, id).map_err(err)?;
                if json {
                    out.push(json!({ "spec": spec, "progress": p }));
                } else {
                    print_status(&spec, &p);
                }
            }
            if json {
                println!("{}", serde_json::to_string_pretty(&out)?);
            }
            Ok(())
        }
        Cmd::Stop { job } => {
            pipeline::request_stop(&lib, job).map_err(err)?;
            println!("pipeline {job}: asked to pause at its next checkpoint");
            Ok(())
        }
        Cmd::Report { job, md, top } => {
            let job = match job {
                Some(j) => j,
                None => lib
                    .jobs()
                    .map_err(err)?
                    .into_iter()
                    .rev()
                    .find(|j| j.kind == "pipeline")
                    .map(|j| j.id)
                    .context("no pipeline in this catalog")?,
            };
            let text = report(&lib, job, top)?;
            match md {
                Some(p) => {
                    std::fs::write(&p, &text)?;
                    println!("report: {}", p.display());
                }
                None => print!("{text}"),
            }
            Ok(())
        }
    }
}

fn print_status(spec: &PipelineSpec, p: &PipelineProgress) {
    println!(
        "{} [{}]: {}",
        p.line(spec),
        p.status,
        jetcarp_core::dashboard::duration(p.elapsed_s)
    );
    for s in &p.steps {
        let jobs = if s.jobs.is_empty() {
            String::new()
        } else {
            format!(
                ", jobs {}",
                s.jobs
                    .iter()
                    .map(|j| j.to_string())
                    .collect::<Vec<_>>()
                    .join(" ")
            )
        };
        println!(
            "  {:<8} {:<8} {:>10}{jobs}{}",
            s.name,
            s.status,
            if s.seconds > 0.0 {
                jetcarp_core::dashboard::duration(s.seconds)
            } else {
                String::new()
            },
            if s.note.is_empty() {
                String::new()
            } else {
                format!("  ({})", s.note)
            }
        );
    }
}

/// What a step ended with.
struct Outcome {
    /// `done`, `skipped`, or `paused`.
    status: &'static str,
    note: String,
    jobs: Vec<i64>,
    summary: Value,
}

struct Ctx<'a> {
    lib: &'a Library,
    job: i64,
    spec: PipelineSpec,
    cancel: Arc<AtomicBool>,
    tools: Tools,
    dir: PathBuf,
}

fn run(
    lib: &Library,
    job: i64,
    redo: &[String],
    threads: Option<u32>,
    rate: Option<u32>,
    set: &[String],
    tools: &Tools,
) -> Result<()> {
    let (mut spec, mut p) = pipeline::load(lib, job).map_err(err)?;
    if let Some(t) = threads {
        spec.threads = t;
    }
    if let Some(r) = rate {
        spec.rate = r;
    }
    for kv in set {
        let (k, v) = kv
            .split_once('=')
            .with_context(|| format!("--set wants KEY=VALUE, got '{kv}'"))?;
        spec.params.insert(k.trim().to_string(), v.to_string());
    }
    if !set.is_empty() {
        // Kept for later resumes; jobs already started keep their own specs.
        lib.set_job_spec(job, &serde_json::to_value(&spec)?)
            .map_err(err)?;
    }
    if p.status == "running" {
        let j = lib.job(job).map_err(err)?;
        let age = jetcarp_core::dashboard::now_seconds()
            - jetcarp_core::dashboard::utc_seconds(&j.updated).unwrap_or(0);
        if age < jetcarp_core::dashboard::STALE_S {
            bail!(
                "pipeline {job} is running in another process (updated {age} s ago); `jetcarp pipeline stop {job}` pauses it"
            );
        }
    }
    for r in redo {
        if let Some(st) = p.steps.iter_mut().find(|s| &s.name == r) {
            st.status = "pending".into();
        } else {
            bail!("unknown step {r}; steps are {}", STEPS.join(", "));
        }
    }
    let dir = lib.folder().join("pipeline").join(job.to_string());
    std::fs::create_dir_all(&dir)?;
    // Ctrl-C and pause requests from other processes stop the running step at its checkpoint.
    let cancel = Arc::new(AtomicBool::new(false));
    let presses = Arc::new(AtomicU32::new(0));
    {
        let (cancel, presses) = (cancel.clone(), presses.clone());
        let _ = ctrlc::set_handler(move || {
            if presses.fetch_add(1, Ordering::SeqCst) == 0 {
                cancel.store(true, Ordering::SeqCst);
                eprintln!(
                    "\nCtrl-C: pausing at the next checkpoint (press again to quit at once; `jetcarp pipeline resume` continues)"
                );
            } else {
                std::process::exit(130);
            }
        });
    }
    let finished = Arc::new(AtomicBool::new(false));
    let watcher = {
        let (cancel, finished) = (cancel.clone(), finished.clone());
        let lib = Library::open(lib.path().to_path_buf());
        std::thread::spawn(move || {
            let mut beat = Instant::now();
            while !finished.load(Ordering::Relaxed) {
                std::thread::sleep(std::time::Duration::from_secs(2));
                if lib.job(job).is_ok_and(|j| j.status == JobStatus::Paused) {
                    cancel.store(true, Ordering::SeqCst);
                    break;
                }
                // A heartbeat, so a long step is not shown as stalled (no progress written
                // for 5 minutes) in the Hunts window.
                if beat.elapsed().as_secs() >= 60 {
                    let _ = lib.update_job(job, None, None, None);
                    beat = Instant::now();
                }
            }
        })
    };
    let session = Instant::now();
    let base_elapsed = p.elapsed_s;
    let cx = Ctx {
        lib,
        job,
        spec: spec.clone(),
        cancel: cancel.clone(),
        tools: tools.clone(),
        dir,
    };
    let result = (|| -> Result<&'static str> {
        for i in 0..p.steps.len() {
            if matches!(p.steps[i].status.as_str(), "done")
                || (p.steps[i].status == "skipped" && p.steps[i].note == "not selected")
            {
                continue;
            }
            if cancel.load(Ordering::SeqCst) {
                return Ok("paused");
            }
            p.step = i;
            let name = p.steps[i].name.clone();
            p.steps[i].status = "running".into();
            if p.steps[i].started.is_empty() {
                p.steps[i].started = jetcarp_core::library::now_utc();
            }
            p.elapsed_s = base_elapsed + session.elapsed().as_secs_f64();
            pipeline::save(lib, job, &p, JobStatus::Running).map_err(err)?;
            println!(
                "== step {} of {}: {} ==",
                i + 1,
                p.steps.len(),
                step_title(&name)
            );
            let t0 = Instant::now();
            let out = run_step(&cx, &name, &p.steps[i]);
            let st = &mut p.steps[i];
            st.seconds += t0.elapsed().as_secs_f64();
            p.elapsed_s = base_elapsed + session.elapsed().as_secs_f64();
            match out {
                Ok(o) => {
                    st.status = o.status.into();
                    st.note = o.note;
                    for j in o.jobs {
                        if !st.jobs.contains(&j) {
                            st.jobs.push(j);
                        }
                    }
                    if !o.summary.is_null() {
                        st.summary = o.summary;
                    }
                    if o.status != "paused" {
                        st.finished = jetcarp_core::library::now_utc();
                    }
                    println!(
                        "-- {}: {} in {}{}",
                        step_title(&name),
                        st.status,
                        jetcarp_core::dashboard::duration(t0.elapsed().as_secs_f64()),
                        if st.note.is_empty() {
                            String::new()
                        } else {
                            format!(" ({})", st.note)
                        }
                    );
                    if o.status == "paused" {
                        return Ok("paused");
                    }
                    pipeline::save(lib, job, &p, JobStatus::Running).map_err(err)?;
                }
                Err(e) => {
                    st.status = "failed".into();
                    st.note = format!("{e:#}");
                    return Err(e);
                }
            }
        }
        p.step = p.steps.len();
        Ok("done")
    })();
    finished.store(true, Ordering::Relaxed);
    let _ = watcher.join();
    p.elapsed_s = base_elapsed + session.elapsed().as_secs_f64();
    let st = match &result {
        Ok("done")
            if p.steps
                .iter()
                .all(|s| s.status == "done" || s.note == "not selected") =>
        {
            JobStatus::Done
        }
        Ok(_) => JobStatus::Paused,
        Err(_) => JobStatus::Failed,
    };
    // A pause requested elsewhere is kept as paused, not overwritten.
    pipeline::save(lib, job, &p, st).map_err(err)?;
    let (spec, p) = pipeline::load(lib, job).map_err(err)?;
    print_status(&spec, &p);
    match result {
        Ok("paused") => {
            println!("paused: `jetcarp pipeline resume {job}` continues");
            Ok(())
        }
        Ok(_) => {
            if st == JobStatus::Paused {
                println!("some steps were skipped: `jetcarp pipeline resume {job}` retries them");
            }
            Ok(())
        }
        Err(e) => Err(e),
    }
}

fn run_step(cx: &Ctx, name: &str, prev: &StepState) -> Result<Outcome> {
    match name {
        "stage1" => script_step(cx, STAGE1_SCRIPT, "stage1-all.rhai", "stage 1"),
        "curate" => curate(cx, prev),
        "refit" => refit(cx),
        "stage2" => script_step(cx, STAGE2_SCRIPT, "stage2-hunt.rhai", "stage 2"),
        "media" => media_step(cx),
        "site" => site_step(cx),
        _ => bail!("unknown step {name}"),
    }
}

/// Jobs of this pipeline whose label starts with `<label> · <part>`.
fn child_jobs(cx: &Ctx, part: &str) -> Result<Vec<jetcarp_core::library::store::JobRow>> {
    let prefix = format!("{} · {part}", cx.spec.label);
    Ok(cx
        .lib
        .jobs()
        .map_err(err)?
        .into_iter()
        .filter(|j| j.label().is_some_and(|l| l.starts_with(&prefix)))
        .collect())
}

fn script_step(cx: &Ctx, source: &str, file: &str, part: &str) -> Result<Outcome> {
    let mut opts = jetcarp_script::RunOptions {
        library: Some(cx.lib.path().to_path_buf()),
        out_root: Some(cx.dir.clone()),
        threads: (cx.spec.threads > 0).then_some(cx.spec.threads as usize),
        script_path: Some(PathBuf::from(format!("scripts/{file}"))),
        ..Default::default()
    };
    opts.params.insert("mode".into(), cx.spec.mode.clone());
    opts.params.insert("label".into(), cx.spec.label.clone());
    opts.params.insert("from".into(), cx.spec.label.clone());
    opts.params
        .insert("threads".into(), cx.spec.threads.to_string());
    for (k, v) in &cx.spec.params {
        opts.params.insert(k.clone(), v.clone());
    }
    let tty = std::io::stderr().is_terminal();
    let last = Mutex::new(Instant::now());
    let log_path = cx
        .dir
        .join(format!("{}.log", file.trim_end_matches(".rhai")));
    let log = Mutex::new(
        std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log_path)?,
    );
    let on_event = move |e: jetcarp_script::Event| match e {
        jetcarp_script::Event::Log(line) => {
            if tty {
                eprint!("\r\x1b[2K");
            }
            println!("{line}");
            let _ = writeln!(log.lock().unwrap(), "{line}");
        }
        jetcarp_script::Event::Show(_) => {}
        jetcarp_script::Event::Progress(p) => {
            if tty {
                eprint!("\r\x1b[2K{}", p.line());
                let _ = std::io::stderr().flush();
            } else {
                let mut l = last.lock().unwrap();
                if l.elapsed().as_secs_f64() >= 30.0 {
                    eprintln!("{}", p.line());
                    *l = Instant::now();
                }
            }
        }
    };
    let s = jetcarp_script::run(source, opts, on_event, cx.cancel.clone());
    if tty {
        eprintln!();
    }
    let jobs = child_jobs(cx, part)?;
    let ids: Vec<i64> = jobs.iter().map(|j| j.id).collect();
    let summary = json!({
        "jobs": jobs.iter().map(|j| json!({
            "id": j.id, "kind": j.kind, "label": j.label(),
            "status": format!("{:?}", j.status).to_lowercase(),
        })).collect::<Vec<_>>(),
        "seconds": s.seconds,
    });
    if s.cancelled || cx.cancel.load(Ordering::SeqCst) {
        return Ok(Outcome {
            status: "paused",
            note: "paused".into(),
            jobs: ids,
            summary,
        });
    }
    if let Some(e) = &s.error {
        bail!("{file}: {e}");
    }
    let open: Vec<i64> = jobs
        .iter()
        .filter(|j| j.status != JobStatus::Done)
        .map(|j| j.id)
        .collect();
    if !open.is_empty() {
        // A child job paused on its own (from the Hunts window, or `jetcarp hunt --stop`): the
        // pipeline pauses too, and a resume finishes it. A failed one fails the step.
        if jobs
            .iter()
            .any(|j| open.contains(&j.id) && j.status == JobStatus::Failed)
        {
            bail!("{file} ended with failed jobs among {open:?}");
        }
        return Ok(Outcome {
            status: "paused",
            note: format!("jobs {open:?} paused"),
            jobs: ids,
            summary,
        });
    }
    Ok(Outcome {
        status: "done",
        note: format!("{} jobs", ids.len()),
        jobs: ids,
        summary,
    })
}

/// A file-name slug.
fn slug(s: &str) -> String {
    let mut o = String::new();
    for c in s.chars() {
        if c.is_ascii_alphanumeric() {
            o.push(c.to_ascii_lowercase());
        } else if !o.ends_with('-') {
            o.push('-');
        }
    }
    o.trim_matches('-').to_string()
}

fn tools_dir(t: &Tools) -> Option<PathBuf> {
    if let Some(d) = &t.tools {
        return Some(d.clone());
    }
    if let Ok(d) = std::env::var("JETCARP_TOOLS") {
        return Some(PathBuf::from(d));
    }
    // <repo>/target/<profile>/jetcarp
    let exe = std::env::current_exe().ok()?;
    let repo = exe.parent()?.parent()?.parent()?;
    let d = repo.join("tools");
    d.join("curate_settings.py").exists().then_some(d)
}

/// Is the model served? (`GET <base>/models` lists it.)
fn model_up(base: &str, model: &str) -> Result<(), String> {
    let out = std::process::Command::new("curl")
        .args([
            "-s",
            "-m",
            "5",
            &format!("{}/models", base.trim_end_matches('/')),
        ])
        .output()
        .map_err(|e| format!("curl: {e}"))?;
    let text = String::from_utf8_lossy(&out.stdout);
    if !out.status.success() || text.trim().is_empty() {
        return Err(format!("no answer from {base}"));
    }
    if !text.contains(model) {
        return Err(format!("{base} does not list {model}"));
    }
    Ok(())
}

fn model_ratings(lib: &Library, rater: &str) -> Result<usize> {
    Ok(lib
        .ratings(Some("setting"))
        .map_err(err)?
        .iter()
        .filter(|r| r.rater == rater)
        .count())
}

/// Contact sheets of each space's best settings (by the pipeline's stage 1 jobs), then the
/// model's shortlist.
fn curate(cx: &Ctx, _prev: &StepState) -> Result<Outcome> {
    let sheets = cx.dir.join("sheets");
    std::fs::create_dir_all(&sheets)?;
    let mut written = Vec::new();
    let mut n = 0;
    for j in child_jobs(cx, "stage 1")? {
        if j.kind != "stage1" {
            continue;
        }
        let rows = cx
            .lib
            .settings(&SettingsQuery {
                job: Some(j.id),
                order: Some("score".into()),
                limit: 100_000,
                ..Default::default()
            })
            .map_err(err)?;
        // Per space (tiling and neighbourhood), the 24 best, one setting per rule.
        let mut per: BTreeMap<(String, String), Vec<jetcarp_core::library::SettingRow>> =
            BTreeMap::new();
        let mut order: Vec<(String, String)> = Vec::new();
        for r in rows {
            let k = (
                r.tiling_name.clone(),
                if r.notation.ends_with('V') {
                    "edge"
                } else {
                    "touch"
                }
                .to_string(),
            );
            let v = per.entry(k.clone()).or_default();
            if v.is_empty() {
                order.push(k);
            }
            if v.len() < 24 && !v.iter().any(|x| x.notation == r.notation) {
                v.push(r);
            }
        }
        for k in order {
            let v = &per[&k];
            n += 1;
            let png = sheets.join(format!("{n:02}-{}-{}.png", slug(&k.0), k.1));
            let labels: Vec<String> = v
                .iter()
                .map(|r| format!("{} {:.0}", r.notation, r.score.unwrap_or(0.0)))
                .collect();
            let o = SheetOptions {
                labels: Some(labels),
                ..Default::default()
            };
            match write_sheet(v, &o, &png) {
                Ok(_) => written.push(png.display().to_string()),
                Err(e) => eprintln!("sheet {}: {e}", png.display()),
            }
        }
    }
    println!("{} contact sheets in {}", written.len(), sheets.display());
    let rater = format!(
        "model:{}",
        if cx.spec.stub {
            "stub"
        } else {
            cx.spec.model.rsplit('/').next().unwrap_or(&cx.spec.model)
        }
    );
    let before = model_ratings(cx.lib, &rater)?;
    let mut summary = json!({ "sheets": written, "rater": rater, "ratings_before": before });
    if !cx.spec.stub
        && let Err(e) = model_up(&cx.spec.base_url, &cx.spec.model)
    {
        return Ok(Outcome {
            status: "skipped",
            note: format!("model not reachable: {e}"),
            jobs: vec![],
            summary,
        });
    }
    let Some(tools) = tools_dir(&cx.tools) else {
        return Ok(Outcome {
            status: "skipped",
            note: "tools/curate_settings.py not found (--tools DIR)".into(),
            jobs: vec![],
            summary,
        });
    };
    let exe = std::env::current_exe()?;
    let mut cmd = std::process::Command::new(&cx.tools.python);
    cmd.arg("-u")
        .arg(tools.join("curate_settings.py"))
        .arg("--jetcarp")
        .arg(&exe)
        .arg("--library")
        .arg(cx.lib.path())
        .arg("--model")
        .arg(&cx.spec.model)
        .arg("--base-url")
        .arg(&cx.spec.base_url)
        .arg("--candidates")
        .arg(cx.spec.rate.to_string())
        .arg("--batch")
        .arg("8")
        .arg("--seed")
        .arg(cx.job.to_string())
        .arg("--sheets")
        .arg(cx.dir.join("model-sheets"));
    if cx.spec.stub {
        cmd.arg("--stub");
    }
    let log_path = cx.dir.join("curate.log");
    let log = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)?;
    cmd.stdout(log.try_clone()?).stderr(log);
    println!(
        "model shortlist: {} settings for {rater} (log {})",
        cx.spec.rate,
        log_path.display()
    );
    let mut child = cmd.spawn().context("starting tools/curate_settings.py")?;
    let status = loop {
        if let Some(s) = child.try_wait()? {
            break Some(s);
        }
        if cx.cancel.load(Ordering::SeqCst) {
            let _ = child.kill();
            let _ = child.wait();
            break None;
        }
        std::thread::sleep(std::time::Duration::from_millis(500));
    };
    let after = model_ratings(cx.lib, &rater)?;
    summary["ratings_after"] = json!(after);
    println!("{} new ratings by {rater} ({after} in all)", after - before);
    match status {
        None => Ok(Outcome {
            status: "paused",
            note: format!("{} ratings so far", after - before),
            jobs: vec![],
            summary,
        }),
        Some(s) if !s.success() => {
            bail!(
                "curate_settings.py failed ({s}); see {}",
                log_path.display()
            )
        }
        Some(_) => Ok(Outcome {
            status: "done",
            note: format!(
                "{} sheets, {} ratings by {rater}",
                written.len(),
                after - before
            ),
            jobs: vec![],
            summary,
        }),
    }
}

fn refit(cx: &Ctx) -> Result<Outcome> {
    let (_, rated) = cx.lib.rating_samples().map_err(err)?;
    if rated < 20 {
        return Ok(Outcome {
            status: "skipped",
            note: format!("{rated} rated settings; a refit needs 20"),
            jobs: vec![],
            summary: json!({ "rated": rated }),
        });
    }
    let before = cx.lib.score_model().map_err(err)?;
    let counts = cx
        .lib
        .settings_counts(&SettingsQuery::default())
        .map_err(err)?;
    let n = |k: &str| counts.get(k).copied().unwrap_or(0) as usize;
    let (n_auto, n_cand) = (n("auto") + n("approved"), n("candidate"));
    let (m, rep) = cx.lib.refit_score(2.0).map_err(err)?;
    // The model rates on its own scale (qwen's mean is about 2.5 stars, a target of 37), so
    // the refit lowers every score and would empty the Quick Start. The refit decides the
    // order; the thresholds then move to the scores at the same ranks, so as many settings
    // stay `auto` and `candidate` as before.
    let score_at = |rank: usize| -> Result<Option<f64>> {
        if rank == 0 {
            return Ok(None);
        }
        Ok(cx
            .lib
            .settings(&SettingsQuery {
                order: Some("score".into()),
                limit: 1,
                offset: rank - 1,
                ..Default::default()
            })
            .map_err(err)?
            .first()
            .and_then(|r| r.score))
    };
    let mut thresholds = json!(null);
    let rep = match (score_at(n_auto)?, score_at(n_auto + n_cand)?) {
        (Some(a), c) if !cx.spec.params.contains_key("keep_thresholds") => {
            let c = c.unwrap_or(a).min(a);
            let (m2, rep2) = cx.lib.set_thresholds(a, c).map_err(err)?;
            println!(
                "  thresholds by rank: auto {:.1} -> {:.1}, candidate {:.1} -> {:.1} ({n_auto} auto, {n_cand} candidates before)",
                before.auto, m2.auto, before.candidate, m2.candidate
            );
            thresholds = json!({ "auto": [before.auto, m2.auto], "candidate": [before.candidate, m2.candidate] });
            rep2
        }
        _ => rep,
    };
    // Rank and diversity per space: a few distinct auto entries and candidates per grid.
    let num = |k: &str, d: f64| -> f64 {
        cx.spec
            .params
            .get(k)
            .and_then(|v| v.parse().ok())
            .unwrap_or(d)
    };
    let thin = cx
        .lib
        .thin_quickstart(&jetcarp_core::library::ThinOptions {
            auto_per_space: num("thin_auto", 6.0) as usize,
            candidate_per_space: num("thin_candidate", 3.0) as usize,
            auto_main: num("thin_auto_main", 24.0) as usize,
            candidate_main: num("thin_candidate_main", 12.0) as usize,
            min_distance: num("thin_distance", 0.06),
            dry_run: false,
        })
        .map_err(err)?;
    println!(
        "  thinned per space: before {:?}, after {:?} ({} look-alikes)",
        thin.before, thin.after, thin.look_alikes
    );
    let mut changes: Vec<(String, f64, f64)> = m
        .weights
        .iter()
        .map(|(k, w)| (k.clone(), before.weights.get(k).copied().unwrap_or(0.0), *w))
        .collect();
    changes.sort_by(|a, b| (b.2 - b.1).abs().total_cmp(&(a.2 - a.1).abs()));
    for (k, a, b) in changes.iter().take(8) {
        println!("  {k:<16} {a:>6.2} -> {b:>6.2}");
    }
    println!(
        "  bias {:.2} -> {:.2}; rmse {:.1} -> {:.1}; {} settings rescored, {:?} changed",
        before.bias,
        m.bias,
        m.rmse_before.unwrap_or(0.0),
        m.rmse_after.unwrap_or(0.0),
        rep.settings,
        rep.changed
    );
    Ok(Outcome {
        status: "done",
        note: format!(
            "model {} from {} ratings of {} settings",
            m.id, m.ratings, m.rated_settings
        ),
        jobs: vec![],
        summary: json!({
            "model": m.id, "ratings": m.ratings, "rated_settings": m.rated_settings,
            "bias": [before.bias, m.bias],
            "weights": changes.iter().map(|(k, a, b)| json!({ "feature": k, "before": a, "after": b })).collect::<Vec<_>>(),
            "rmse": [m.rmse_before, m.rmse_after],
            "thresholds": thresholds,
            "rescore": rep,
            "thin": thin,
        }),
    })
}

/// The patterns worth showing: ships, guns, puffers, movers, and oscillators of period 3 or
/// more, best first.
fn showcase(lib: &Library, limit: u32) -> Result<Vec<jetcarp_core::library::PatternRecord>> {
    // One query per class (the catalog may hold hundreds of thousands of patterns, so never
    // all of them at once), each the best `limit`, then merged best first.
    let mut recs = Vec::new();
    for (class, min_period) in [
        (PatternClass::Spaceship, None),
        (PatternClass::Gun, None),
        (PatternClass::Puffer, None),
        (PatternClass::Mover, None),
        (PatternClass::Oscillator, Some(3)),
    ] {
        recs.extend(
            lib.query(&jetcarp_core::library::Query {
                class: Some(class),
                min_period,
                order: Some("score".into()),
                limit: (limit > 0).then_some(limit as usize),
                ..Default::default()
            })
            .map_err(err)?
            .into_iter()
            .filter(|r| r.state != CurationState::Rejected),
        );
    }
    recs.sort_by(|a, b| b.score.total_cmp(&a.score).then(a.id.cmp(&b.id)));
    if limit > 0 {
        recs.truncate(limit as usize);
    }
    Ok(recs)
}

fn media_step(cx: &Ctx) -> Result<Outcome> {
    let mut targets: Vec<MediaTarget> = cx
        .lib
        .quickstart(&[QsStatus::Auto, QsStatus::Approved])
        .map_err(err)?
        .into_iter()
        .map(|q| MediaTarget::QuickStart(q.id))
        .collect();
    let nq = targets.len();
    let recs = showcase(cx.lib, cx.spec.media_limit)?;
    let np = recs.len();
    targets.extend(recs.into_iter().map(|r| MediaTarget::Pattern(r.id)));
    println!("media for {nq} Quick Start entries and {np} patterns");
    let threads = if cx.spec.threads > 0 {
        cx.spec.threads as usize
    } else {
        std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(4)
    };
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(threads)
        .build()?;
    let (mut written, mut skipped, mut failed) = (0, 0, Vec::new());
    // Chunks, so a pause takes effect between them.
    // Quick Start entries (whole grids from the seed) get an animation, a poster, and a
    // thumbnail; the per-step frames are for patterns (one image per step on the website).
    let seed_opts = MediaOptions {
        frames: false,
        ..Default::default()
    };
    let total = targets.len();
    let (mut k, mut done) = (0, 0);
    for chunk in targets[..nq].chunks(64).chain(targets[nq..].chunks(64)) {
        k += 1;
        done += chunk.len();
        let opts = if matches!(chunk[0], MediaTarget::QuickStart(_)) {
            seed_opts.clone()
        } else {
            MediaOptions::default()
        };
        if cx.cancel.load(Ordering::SeqCst) {
            return Ok(Outcome {
                status: "paused",
                note: format!("{written} rendered"),
                jobs: vec![],
                summary: Value::Null,
            });
        }
        let r = pool
            .install(|| media::render_many(cx.lib, chunk, &opts))
            .map_err(err)?;
        written += r.written();
        skipped += r.skipped();
        failed.extend(r.failed);
        let line = format!(
            "{done} of {total} targets: {written} rendered, {skipped} up to date, {} failed",
            failed.len()
        );
        if std::io::stderr().is_terminal() {
            eprint!("\r{line}");
        } else if k % 16 == 0 {
            eprintln!("{line}");
        }
    }
    if std::io::stderr().is_terminal() {
        eprintln!();
    }
    for (k, id, e) in failed.iter().take(10) {
        eprintln!("  {k} {id}: {e}");
    }
    Ok(Outcome {
        status: "done",
        note: format!(
            "{written} rendered, {skipped} up to date, {} failed",
            failed.len()
        ),
        jobs: vec![],
        summary: json!({
            "quickstart": nq, "patterns": np, "written": written, "skipped": skipped,
            "failed": failed.iter().map(|(k, i, e)| json!([k, i, e])).collect::<Vec<_>>(),
        }),
    })
}

fn site_step(cx: &Ctx) -> Result<Outcome> {
    let dir = PathBuf::from(&cx.spec.site);
    std::fs::create_dir_all(&dir)?;
    let o = ExportOptions {
        filter: ExportFilter {
            states: vec![
                CurationState::New,
                CurationState::Reviewed,
                CurationState::Promoted,
            ],
            classes: vec![
                PatternClass::Spaceship,
                PatternClass::Gun,
                PatternClass::Puffer,
                PatternClass::Mover,
                PatternClass::Oscillator,
            ],
            min_osc_period: Some(3),
            ids: showcase(cx.lib, cx.spec.media_limit)?
                .into_iter()
                .map(|r| r.id)
                .collect(),
            quickstart: vec![QsStatus::Auto, QsStatus::Approved],
            ..Default::default()
        },
        // Only what the media step rendered: patterns beyond its limit are exported without
        // media rather than rendered here.
        render: false,
        ..Default::default()
    };
    let r = website::export(cx.lib, &dir, &o).map_err(err)?;
    let index = dir.join("index.html");
    if std::fs::read_to_string(&index).ok().as_deref() != Some(SITE_INDEX) {
        std::fs::write(&index, SITE_INDEX)?;
    }
    let size = dir_size(&dir);
    println!(
        "site {}: {} patterns, {} Quick Start entries, {} files written, {} deleted, {} unchanged, {:.1} MB",
        dir.display(),
        r.patterns,
        r.quickstart,
        r.written.len(),
        r.deleted.len(),
        r.unchanged,
        size as f64 / 1e6
    );
    println!("view it: python3 -m http.server -d {}", dir.display());
    Ok(Outcome {
        status: "done",
        note: format!("{} patterns, {} entries", r.patterns, r.quickstart),
        jobs: vec![],
        summary: json!({
            "dir": dir, "patterns": r.patterns, "quickstart": r.quickstart,
            "written": r.written.len(), "deleted": r.deleted.len(), "unchanged": r.unchanged,
            "bytes": size,
        }),
    })
}

fn dir_size(p: &Path) -> u64 {
    let Ok(rd) = std::fs::read_dir(p) else {
        return 0;
    };
    rd.flatten()
        .map(|e| match e.file_type() {
            Ok(t) if t.is_dir() => dir_size(&e.path()),
            Ok(_) => e.metadata().map(|m| m.len()).unwrap_or(0),
            Err(_) => 0,
        })
        .sum()
}

// ---------------------------------------------------------------------------------------------
// Report

fn report(lib: &Library, job: i64, top: usize) -> Result<String> {
    use std::fmt::Write as _;
    let (spec, p) = pipeline::load(lib, job).map_err(err)?;
    let mut o = String::new();
    writeln!(
        o,
        "# {} ({} mode)\n\nCatalog `{}`; status {}, {} in all.\n",
        spec.label,
        spec.mode,
        lib.path().display(),
        p.status,
        jetcarp_core::dashboard::duration(p.elapsed_s)
    )?;
    writeln!(
        o,
        "## Steps\n\n| Step | Status | Time | Jobs | Note |\n| --- | --- | --- | --- | --- |"
    )?;
    for s in &p.steps {
        writeln!(
            o,
            "| {} | {} | {} | {} | {} |",
            step_title(&s.name),
            s.status,
            jetcarp_core::dashboard::duration(s.seconds),
            s.jobs
                .iter()
                .map(|j| j.to_string())
                .collect::<Vec<_>>()
                .join(" "),
            s.note
        )?;
    }
    let jobs = lib.jobs().map_err(err)?;
    let prefix = format!("{} · ", spec.label);
    let mine: Vec<_> = jobs
        .iter()
        .filter(|j| j.label().is_some_and(|l| l.starts_with(&prefix)))
        .collect();
    // Stage 1 per space.
    writeln!(
        o,
        "\n## Stage 1 per space\n\n| Space | Settings | Auto | Candidates | Best rules (score, behaviour) |\n| --- | --- | --- | --- | --- |"
    )?;
    for j in mine.iter().filter(|j| j.kind == "stage1") {
        let (_, sp) = jetcarp_core::stage1::job::status(lib, j.id).map_err(err)?;
        let rows = lib
            .settings(&SettingsQuery {
                job: Some(j.id),
                order: Some("score".into()),
                limit: 1_000_000,
                ..Default::default()
            })
            .map_err(err)?;
        for s in &sp.spaces {
            // Settings of this space: the tiling name and neighbourhood of the label.
            let edge = s.space.contains("(edge)");
            let name = s.space.split(" (").next().unwrap_or(&s.space).to_string();
            let mut best: Vec<String> = Vec::new();
            let mut seen = std::collections::HashSet::new();
            for r in rows.iter().filter(|r| {
                r.tiling_name.split(" (").next() == Some(name.as_str())
                    && r.notation.ends_with('V') == edge
            }) {
                if seen.insert(r.notation.clone()) {
                    best.push(format!(
                        "{} {:.0} {}",
                        r.notation,
                        r.score.unwrap_or(0.0),
                        r.descriptors.tags.join("/")
                    ));
                }
                if best.len() >= top {
                    break;
                }
            }
            writeln!(
                o,
                "| {} | {} | {} | {} | {} |",
                s.space,
                s.evaluated,
                s.auto,
                s.candidate,
                best.join("; ")
            )?;
        }
    }
    // Aperiodic censuses.
    let census: Vec<_> = mine.iter().filter(|j| j.kind == "census").collect();
    if !census.is_empty() {
        writeln!(
            o,
            "\n## Stage 1 on aperiodic patches (census on the World)\n\n| Patch | Rules | Died | Frozen | Oscillating | Active | Exploded | Most oscillating rules |\n| --- | --- | --- | --- | --- | --- | --- | --- |"
        )?;
        for j in census {
            let rows = lib.census_rows(j.id).map_err(err)?;
            let n = rows.len().max(1) as f64;
            let mean = |k: &str| rows.iter().filter_map(|r| r[k].as_f64()).sum::<f64>() / n;
            let mut osc: Vec<(f64, String, Option<f64>)> = rows
                .iter()
                .filter(|r| r["exploded"].as_f64() == Some(0.0))
                .map(|r| {
                    (
                        r["oscillating"].as_f64().unwrap_or(0.0),
                        r["notation"].as_str().unwrap_or("").to_string(),
                        r["median_period"].as_f64(),
                    )
                })
                .collect();
            osc.sort_by(|a, b| b.0.total_cmp(&a.0).then(a.1.cmp(&b.1)));
            writeln!(
                o,
                "| {} | {} | {:.0}% | {:.0}% | {:.0}% | {:.0}% | {:.0}% | {} |",
                j.spec["tiling"]["family"].as_str().unwrap_or("?"),
                rows.len(),
                100.0 * mean("died"),
                100.0 * mean("frozen"),
                100.0 * mean("oscillating"),
                100.0 * mean("active"),
                100.0 * mean("exploded"),
                osc.iter()
                    .take(top)
                    .map(|x| format!(
                        "{} ({:.0}%{})",
                        x.1,
                        100.0 * x.0,
                        x.2.map(|p| format!(", p{p}")).unwrap_or_default()
                    ))
                    .collect::<Vec<_>>()
                    .join("; ")
            )?;
        }
    }
    // Stage 2 per hunted rule.
    writeln!(
        o,
        "\n## Stage 2 per hunted rule\n\n| Space | Rule | Soups | Distinct (unstored) | Spaceships (speed, cells, period, copies) | Guns | Puffers | Longest stored oscillators |\n| --- | --- | --- | --- | --- | --- | --- | --- |"
    )?;
    let mut recs: HashMap<String, Option<jetcarp_core::library::PatternRecord>> = HashMap::new();
    for j in mine.iter().filter(|j| j.kind == "hunt") {
        let (_, hp) = jetcarp_core::hunt::job::status(lib, j.id).map_err(err)?;
        let sightings = lib.job_sightings(j.id).map_err(err)?;
        for g in &hp.groups {
            let bare = g.rule.trim_end_matches('V');
            let mut ships = Vec::new();
            let mut guns = Vec::new();
            let mut puffers = Vec::new();
            let mut osc: Vec<(u64, String)> = Vec::new();
            for (pid, rule, count, _) in &sightings {
                if rule != bare && rule != &g.rule {
                    continue;
                }
                let r = recs
                    .entry(pid.clone())
                    .or_insert_with(|| lib.get(pid).ok())
                    .clone();
                let Some(r) = r else { continue };
                match r.class {
                    PatternClass::Spaceship => ships.push(format!(
                        "{} {}c p{} x{count}",
                        r.speed.clone().unwrap_or_default(),
                        r.cells,
                        r.period
                    )),
                    PatternClass::Gun => guns.push(format!("p{} x{count}", r.period)),
                    PatternClass::Puffer => puffers.push(format!(
                        "{} p{} x{count}",
                        r.speed.clone().unwrap_or_default(),
                        r.period
                    )),
                    PatternClass::Oscillator => {
                        osc.push((r.period, format!("p{} {}c", r.period, r.cells)))
                    }
                    _ => {}
                }
            }
            osc.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
            osc.dedup_by(|a, b| a.0 == b.0);
            let cap = |v: Vec<String>, n: usize| -> String {
                let k = v.len();
                let mut s = v.into_iter().take(n).collect::<Vec<_>>().join("; ");
                if k > n {
                    s.push_str(&format!(" (+{})", k - n));
                }
                s
            };
            writeln!(
                o,
                "| {} | {} | {} | {} ({}) | {} | {} | {} | {} |",
                g.tiling,
                g.rule,
                g.soups,
                g.distinct,
                g.unstored_distinct,
                cap(ships, 4),
                cap(guns, 3),
                cap(puffers, 3),
                osc.iter()
                    .take(3)
                    .map(|x| x.1.clone())
                    .collect::<Vec<_>>()
                    .join("; ")
            )?;
        }
    }
    for j in mine.iter().filter(|j| j.kind == "search") {
        let n = lib.job_sightings(j.id).map_err(err)?.len();
        writeln!(
            o,
            "\nSoup search {} ({}): {n} pattern sightings.",
            j.id,
            j.label().unwrap_or("")
        )?;
    }
    // The model against the score.
    let ratings = lib.ratings(Some("setting")).map_err(err)?;
    let model: Vec<_> = ratings
        .iter()
        .filter(|r| r.rater.starts_with("model:"))
        .collect();
    if !model.is_empty() {
        let mut pairs = Vec::new();
        let mut liked = Vec::new();
        for r in &model {
            if let Ok(s) = lib.setting(&r.target_id) {
                pairs.push((s.score.unwrap_or(0.0), r.rating));
                if r.rating >= 4.0 {
                    liked.push(format!(
                        "{} on {} ({:.0}, rated {})",
                        s.notation,
                        s.tiling_name,
                        s.score.unwrap_or(0.0),
                        r.rating
                    ));
                }
            }
        }
        let n = pairs.len() as f64;
        let (mx, my) = (
            pairs.iter().map(|p| p.0).sum::<f64>() / n,
            pairs.iter().map(|p| p.1).sum::<f64>() / n,
        );
        let cov: f64 = pairs.iter().map(|p| (p.0 - mx) * (p.1 - my)).sum();
        let vx: f64 = pairs.iter().map(|p| (p.0 - mx).powi(2)).sum();
        let vy: f64 = pairs.iter().map(|p| (p.1 - my).powi(2)).sum();
        let mut hist = [0usize; 5];
        for p in &pairs {
            hist[(p.1.round() as usize).clamp(1, 5) - 1] += 1;
        }
        writeln!(
            o,
            "\n## Model ratings\n\n{} ratings by {}; ratings 1-5: {:?}; correlation with the score {:.2}.\n\nRated 4 or 5: {}",
            model.len(),
            model
                .iter()
                .map(|r| r.rater.clone())
                .collect::<std::collections::BTreeSet<_>>()
                .into_iter()
                .collect::<Vec<_>>()
                .join(", "),
            hist,
            cov / (vx * vy).sqrt().max(1e-9),
            liked.join("; ")
        )?;
    }
    Ok(o)
}
