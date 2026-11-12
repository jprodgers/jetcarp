//! Searches as library jobs: `search_start` starts (or carries on) a search of any strategy in
//! the background, `job_status` follows one or lists them, and `job_stop` stops one at its next
//! checkpoint, wherever it was started (here, the command line, a script, the app). They are the
//! operations `search.start`, `jobs.status`, `jobs.list`, `jobs.report` and `jobs.stop`.

use crate::schema::{self, object};
use crate::tools::{Call, Res, Tool, ToolOutput, ctx, err};
use jetcarp_api::op::{self, Op, Quiet, Report, input_schema};
use jetcarp_api::ops::jobs::{
    JobIn, JobView, JobsReport, JobsStatus, JobsStop, ReportIn, StatusIn, StatusOut,
};
use jetcarp_api::ops::search::{
    AntsIn, BoundedIn, BudgetIn, CensusIn, CollideIn, EvolveIn, ExploreIn, HuntIn, MoverIn, PileIn,
    SearchStart, SearchStartIn,
};
use serde_json::{Map, Value, json};

/// Longest wait one call makes for a job, in seconds.
pub const MAX_WAIT_S: f64 = 600.0;

/// The strategies, in the order the schema lists them, with their one-line descriptions.
const STRATEGIES: &[(&str, &str)] = &[
    (
        "hunt",
        "A soup hunt: small random soups on an unbounded plane (B/S and Generations rules without B0, any periodic tiling), each run until it settles; ships that fly away are cut out; every object is classed and new patterns are verified and stored. Thousands of soups a second for Life.",
    ),
    (
        "search",
        "Soups on a bounded grid of any tiling (aperiodic patches too), each run for `gens`; the objects left are verified and stored.",
    ),
    (
        "ants",
        "Ant and turmite rules (turn strings such as RL or LLRR, or turmite tables) on a periodic tiling, from an empty field or soups; each run classed highway (proved exactly), periodic, symmetric, bounded or chaotic, and stored.",
    ),
    (
        "census",
        "Rules classed by what their soups do (die, freeze, oscillate, grow, ...), with a table and a contact sheet.",
    ),
    (
        "mover",
        "A pattern on an aperiodic tiling (Spectre, hat, Penrose) followed far on a sliding window of generated supertiles; a verified mover is stored with its distance.",
    ),
    (
        "pile",
        "One large sandpile: N grains dropped on one cell of an unbounded grid, built by doubling, with pictures in its folder under the output folder.",
    ),
    (
        "collide",
        "Pattern `b` shot into pattern `a` at every offset and phase; each run classed, new products verified and stored.",
    ),
    (
        "evolve",
        "The evolutionary glider search: multi-state priority rules evolve towards rules whose seeds travel far; verified movers are stored.",
    ),
    (
        "explore",
        "The broad search over tilings, rule families, rules and seeds, keeping the best runs of each kind of behaviour and breeding from them; every run is stored in the job's own database (search_runs reads them). Not deterministic: a resumed search continues from its archive.",
    ),
];

fn strategy_schema(name: &str) -> Value {
    match name {
        "hunt" => input_schema::<HuntIn>(),
        "search" => input_schema::<BoundedIn>(),
        "ants" => input_schema::<AntsIn>(),
        "census" => input_schema::<CensusIn>(),
        "mover" => input_schema::<MoverIn>(),
        "pile" => input_schema::<PileIn>(),
        "collide" => input_schema::<CollideIn>(),
        "evolve" => input_schema::<EvolveIn>(),
        _ => input_schema::<ExploreIn>(),
    }
}

pub(crate) fn tools() -> Vec<Tool> {
    vec![
        Tool {
            name: "search_start",
            title: "Start or resume a search",
            read_only: false,
            destructive: false,
            description: || {
                format!(
                    "Starts a search as a library job and returns its job id at once; job_status follows it, job_stop stops it. Give one strategy as a key holding its settings (every setting is optional): `hunt` (soups on an unbounded plane), `search` (soups on a bounded grid of any tiling), `ants`, `census`, `mover`, `pile`, `collide`, `evolve`, or `explore` (help {{\"topic\": \"searches\"}} explains each). Or {{\"resume\": job}} carries on a stopped job. The job runs on a thread of this server, so it stops when the server exits (it stays resumable); {{\"detach\": true}} runs it in a process of its own that outlives the server (explore searches do this by default where they can). `wait_s` (at most {MAX_WAIT_S:.0}) waits for the job to end before answering, with its status. `budget` ends this run at a checkpoint after so many minutes or items. Results go to the library (library_query), an explore search's runs to search_runs. Examples: {{\"hunt\": {{\"rules\": [\"B3/S23\"], \"soups\": 20000}}, \"wait_s\": 120}}, {{\"ants\": {{\"tilings\": \"hex\", \"strings\": [2, 4]}}}}, {{\"pile\": {{\"tiling\": \"hex\", \"grains\": \"1e6\"}}}}, {{\"collide\": {{\"a\": \"block\", \"b\": \"glider\"}}, \"wait_s\": 60}}, {{\"explore\": {{\"tilings\": \"square,hex\", \"families\": \"life_like,cyclic\"}}, \"budget\": {{\"minutes\": 5}}}}."
                )
            },
            schema: || {
                let start = schema::props(&input_schema::<SearchStartIn>());
                let mut p = Map::new();
                for (name, what) in STRATEGIES {
                    p.insert(
                        name.to_string(),
                        schema::op(strategy_schema(name), Some(what)),
                    );
                }
                for k in ["resume", "label", "budget", "threads", "detach"] {
                    p.insert(k.into(), start[k].clone());
                }
                let mut gpu = start["gpu"].clone();
                gpu["description"] = json!(format!(
                    "{} Default false; explore true.",
                    gpu["description"].as_str().unwrap_or("")
                ));
                p.insert("gpu".into(), gpu);
                p.insert(
                    "wait_s".into(),
                    json!({"type": "number", "minimum": 0, "maximum": MAX_WAIT_S, "description": "Wait up to this many seconds for the job to end, then answer with its status (default 0: answer at once)."}),
                );
                object(p, &[])
            },
            run: search_start,
        },
        Tool {
            name: "job_status",
            title: "Job status, finds and report",
            read_only: true,
            destructive: false,
            description: || {
                format!(
                    "Reports a library job's progress, whatever started it: hunts (soups, objects by class), ant searches (runs, outcomes), piles (doubling step, grains, the pictures when done), movers, bounded searches, collisions, censuses, evolve and explore searches. With `wait_s` (at most {MAX_WAIT_S:.0}) it waits for a running job to end before answering, so one call replaces many polls. `finds` adds the job's newest and rarest finds (pattern ids for library_query); `report` gives a Markdown report of what it searched and found instead. Without `job`, lists the jobs, newest first, optionally of one kind or status. Examples: {{\"job\": 3, \"wait_s\": 300}}, {{\"job\": 3, \"finds\": 10}}, {{\"job\": 3, \"report\": true}}, {{\"kind\": \"ants\"}}."
                )
            },
            schema: || {
                let mut p = schema::props(&input_schema::<StatusIn>());
                let mut w = p["wait_s"].clone();
                w["maximum"] = json!(MAX_WAIT_S);
                p.insert("wait_s".into(), w);
                p.insert(
                    "report".into(),
                    json!({"type": "boolean", "description": "With `job`: a Markdown report of what it searched, for how long, and what it found."}),
                );
                object(p, &[])
            },
            run: job_status,
        },
        Tool {
            name: "job_stop",
            title: "Stop a job",
            read_only: false,
            destructive: false,
            description: || {
                "Stops a running job at its next checkpoint (a few seconds), in whatever process runs it: here, the command line, a script, or the app. It stays resumable: search_start {\"resume\": job} carries it on (a hunt, ant search, pile, evolve or mover job then ends exactly as an uninterrupted run would; an explore search continues from its archive). Example: {\"job\": 3}.".into()
            },
            schema: || object(schema::props(&input_schema::<JobIn>()), &["job"]),
            run: job_stop,
        },
    ]
}

/// The CLI's resume hint, as these tools say it.
fn mcp_words(text: &str, job: i64) -> String {
    text.replace(
        &format!("`jetcarp jobs resume {job}`"),
        &format!("search_start {{\"resume\": {job}}}"),
    )
}

/// One job, as `job_status` and `search_start` with a wait report it: its summary (with its
/// finds, by id), then the facts as one line of JSON.
pub(crate) fn job_output(v: &JobView) -> ToolOutput {
    let mut facts = json!({
        "job": v.job, "kind": v.kind, "status": if v.stale { "stale" } else { v.status.as_str() },
        "done": v.done, "total": v.total, "unit": v.unit,
        "elapsed_s": (v.elapsed_s * 10.0).round() / 10.0, "eta_s": v.eta_s.map(|e| e.round()),
        "resumable": v.resumable,
    });
    if let Some(l) = &v.label {
        facts["label"] = json!(l);
    }
    if let Some(e) = &v.error {
        facts["error"] = json!(e);
    }
    if let Some(f) = v.progress.get("folder").filter(|f| !f.is_null()) {
        facts["folder"] = f.clone();
    }
    if let Some(w) = v.waited_s {
        facts["waited_s"] = json!((w * 10.0).round() / 10.0);
    }
    let mut o = ToolOutput::text(mcp_words(&v.text(), v.job));
    o.content
        .push(json!({"type": "text", "text": facts.to_string()}));
    o
}

/// Wait for a job (with the client's cancel flag), and report it.
fn wait_for(c: &Call, job: i64, wait_s: f64, finds: Option<usize>) -> Res {
    let a = StatusIn {
        job: Some(job),
        wait_s: (wait_s > 0.0).then_some(wait_s.min(MAX_WAIT_S)),
        finds,
        ..Default::default()
    };
    let StatusOut::One(v) = JobsStatus::run(ctx(), a, &c.callbacks()).map_err(err)? else {
        return Err(format!("no job {job}"));
    };
    Ok(job_output(&v))
}

fn number(a: &Value, k: &str) -> Result<Option<f64>, String> {
    match a.get(k) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => v
            .as_f64()
            .or_else(|| v.as_str().and_then(|s| s.trim().parse().ok()))
            .map(Some)
            .ok_or_else(|| format!("`{k}` is a number, not {v}")),
    }
}

fn search_start(c: &mut Call) -> Res {
    let a = c.args;
    let names: Vec<&str> = STRATEGIES.iter().map(|s| s.0).collect();
    let which = schema::one_of("search_start", a, &names)?;
    let resume = a.get("resume").filter(|v| !v.is_null());
    if which.is_none() && resume.is_none() {
        return Err(format!(
            "search_start: give a strategy ({}) with its settings, or `resume` with a job id",
            names.join(", ")
        ));
    }
    let start_schema = input_schema::<SearchStartIn>();
    let mut rest = schema::pick(
        a,
        &["resume", "label", "budget", "threads", "gpu", "detach"],
    );
    schema::coerce(&start_schema, &mut rest);
    let (strategy, spec) = match which {
        Some(name) => {
            let s = strategy_schema(name);
            let mut spec = a[name].clone();
            if let Value::String(t) = &spec
                && let Ok(v) = serde_json::from_str::<Value>(t)
            {
                spec = v;
            }
            if !spec.is_object() {
                return Err(format!(
                    "search_start: `{name}` holds an object of settings"
                ));
            }
            schema::coerce(&s, &mut spec);
            // Read here so a bad setting is refused with the strategy's own words.
            let spec = match name {
                "hunt" => check::<HuntIn>(name, spec)?,
                "search" => check::<BoundedIn>(name, spec)?,
                "ants" => check::<AntsIn>(name, spec)?,
                "census" => check::<CensusIn>(name, spec)?,
                "mover" => check::<MoverIn>(name, spec)?,
                "pile" => check::<PileIn>(name, spec)?,
                "collide" => check::<CollideIn>(name, spec)?,
                "evolve" => check::<EvolveIn>(name, spec)?,
                _ => check::<ExploreIn>(name, spec)?,
            };
            (Some(name.to_string()), spec)
        }
        None => (None, Value::Null),
    };
    let explore = which == Some("explore");
    let gpu = rest.get("gpu").and_then(Value::as_bool).unwrap_or(explore);
    let cx = ctx();
    let detach = rest
        .get("detach")
        .and_then(Value::as_bool)
        .unwrap_or(explore && cx.platform.can_detach());
    if detach && !cx.platform.can_detach() {
        return Err(
            "search_start: this server cannot start a process of its own (it runs inside the app or a test); leave out `detach`"
                .into(),
        );
    }
    let budget: Option<BudgetIn> = match rest.get("budget") {
        None => None,
        Some(b) => Some(
            serde_json::from_value(b.clone()).map_err(|e| format!("search_start budget: {e}"))?,
        ),
    };
    let input = SearchStartIn {
        strategy,
        spec,
        label: rest.get("label").and_then(Value::as_str).map(String::from),
        resume: match rest.get("resume") {
            None => None,
            Some(v) => Some(
                v.as_i64()
                    .ok_or_else(|| format!("search_start: `resume` is a job id, not {v}"))?,
            ),
        },
        budget,
        threads: rest
            .get("threads")
            .and_then(Value::as_u64)
            .map(|t| t as usize),
        gpu,
        background: !detach,
        detach,
        ..Default::default()
    };
    let out = SearchStart::run(cx, input, &Quiet).map_err(err)?;
    let wait = number(a, "wait_s")?.unwrap_or(0.0).min(MAX_WAIT_S);
    if wait > 0.0 {
        let t0 = std::time::Instant::now();
        if out.mode == "detached" {
            // The new process takes the job up in a moment; until then it reads as paused.
            started(c, out.job, wait.min(20.0))?;
        }
        let left = (wait - t0.elapsed().as_secs_f64()).max(0.1);
        let mut o = wait_for(c, out.job, left, None)?;
        o.content.insert(
            0,
            json!({"type": "text", "text": format!(
                "{} job {} {} ({}).",
                out.kind_label,
                out.job,
                if out.created { "started" } else { "resumed" },
                out.mode
            )}),
        );
        return Ok(o);
    }
    Ok(ToolOutput::json(&json!({
        "job": out.job,
        "kind": out.kind,
        "started": out.created,
        "mode": out.mode,
        "status": out.status,
        "line": out.line,
        "folder": out.progress.get("folder"),
        "note": format!(
            "The job runs {}: job_status {{\"job\": {j}, \"wait_s\": 300}} waits for it, job_stop {{\"job\": {j}}} stops it, search_start {{\"resume\": {j}}} carries it on.",
            if out.mode == "detached" { "in a process of its own" } else { "in the background" },
            j = out.job
        ),
    })))
}

/// Wait (up to `max_s`, or until the client cancels) for a job handed to another process to be
/// taken up: running, or with time run.
fn started(c: &Call, job: i64, max_s: f64) -> Result<(), String> {
    let t0 = std::time::Instant::now();
    let p = c.callbacks();
    loop {
        let a = StatusIn {
            job: Some(job),
            ..Default::default()
        };
        if let StatusOut::One(v) = JobsStatus::run(ctx(), a, &Quiet).map_err(err)?
            && (v.status != "paused" || v.elapsed_s > 0.0)
        {
            return Ok(());
        }
        if jetcarp_api::Progress::cancelled(&p) || t0.elapsed().as_secs_f64() >= max_s {
            return Ok(());
        }
        std::thread::sleep(std::time::Duration::from_millis(200));
    }
}

/// A strategy's settings read into its input type (so a bad key is refused here), as JSON.
fn check<T: serde::de::DeserializeOwned + serde::Serialize>(
    name: &str,
    v: Value,
) -> Result<Value, String> {
    let t: T = serde_json::from_value(v).map_err(|e| format!("search_start {name}: {e}"))?;
    Ok(op::to_value(&t))
}

fn job_status(c: &mut Call) -> Res {
    let a = c.args;
    let report = match a.get("report") {
        None | Some(Value::Null) => false,
        Some(Value::Bool(b)) => *b,
        Some(Value::String(s)) if s == "true" || s == "false" => s == "true",
        Some(v) => return Err(format!("job_status: `report` is true or false, not {v}")),
    };
    let mut rest = a.clone();
    if let Some(o) = rest.as_object_mut() {
        o.remove("report");
    }
    let mut input: StatusIn = schema::read(&input_schema::<StatusIn>(), "job_status", rest)?;
    if report {
        let job = input.job.ok_or("job_status: `report` needs the `job`")?;
        let r = JobsReport::run(
            ctx(),
            ReportIn {
                job: Some(job),
                ..Default::default()
            },
            &Quiet,
        )
        .map_err(err)?;
        return Ok(ToolOutput::text(r.text()));
    }
    if let Some(job) = input.job {
        return wait_for(c, job, input.wait_s.unwrap_or(0.0), input.finds);
    }
    input.limit = Some(input.limit.unwrap_or(30).min(200));
    let StatusOut::List(l) = JobsStatus::run(ctx(), input, &Quiet).map_err(err)? else {
        return Err("job_status: no list".into());
    };
    let jobs: Vec<Value> = l
        .jobs
        .iter()
        .map(|j| {
            json!({
                "job": j.job, "kind": j.kind, "status": j.status, "label": j.label,
                "line": j.line, "created": j.created, "updated": j.updated,
            })
        })
        .collect();
    Ok(ToolOutput::json(&json!({
        "catalog": l.catalog,
        "jobs": jobs,
    })))
}

fn job_stop(c: &mut Call) -> Res {
    let input: JobIn = schema::read(&input_schema::<JobIn>(), "job_stop", c.args.clone())?;
    let job = input.job;
    let s = JobsStop::run(ctx(), input, &Quiet).map_err(err)?;
    Ok(ToolOutput::json(&json!({
        "job": job,
        "kind": s.kind,
        "stopping": s.stopping,
        "status": s.status,
        "note": if s.stopping {
            format!("It stops at its next checkpoint and stays resumable with search_start {{\"resume\": {job}}}.")
        } else {
            format!("It was not running ({}).", s.status)
        },
    })))
}

/// The strategies' names (for tests and the docs).
pub fn strategies() -> Vec<&'static str> {
    STRATEGIES.iter().map(|s| s.0).collect()
}
