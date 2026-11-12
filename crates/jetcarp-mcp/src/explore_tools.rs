//! MCP tools for the exploratory search, so a model can run a search, look at what it finds, try
//! ideas of its own, and record what it thinks. Each calls an operation: an explore search is a
//! catalog job (`search.start` with strategy `explore`, `jobs.status`, `jobs.stop`), and its runs
//! are read through `search.runs`, `search.sheet`, `search.run_scene`, `search.try`, and
//! `search.review`. A search is named by its `job` (default: the newest explore job).
//!
//! `explore_start` runs the search in a process of its own (`jetcarp jobs resume JOB`) when this
//! server is the `jetcarp` program, so it outlives the client and uses the GPU; elsewhere (inside
//! the app, in tests) on a thread of this process. The tool names are as before (step 11 of the
//! architecture plan regroups them).

use crate::tools::{Res, ToolOutput, check_unknown, ctx, err, int, string, tool, uint};
use crate::util;
use jetcarp_api::op::{self, Op, Quiet};
use jetcarp_api::ops::explore::{
    ReviewIn, RunIn, RunsIn, SearchReview, SearchRunScene, SearchRuns, SearchSheet, SearchTry,
    SheetIn, TryIn,
};
use jetcarp_api::ops::jobs::{JobIn, JobsList, JobsStatus, JobsStop, ListIn, StatusIn, StatusOut};
use jetcarp_api::ops::search::{BudgetIn, ExploreIn, SearchStart, SearchStartIn};
use serde_json::{Value, json};

pub const NAMES: &[&str] = &[
    "explore_start",
    "explore_status",
    "explore_stop",
    "explore_query",
    "explore_sheet",
    "explore_try",
    "explore_scene",
    "explore_review",
];

pub fn handles(name: &str) -> bool {
    NAMES.contains(&name)
}

const MAX_ROWS: i64 = 200;
const MAX_SHEET: usize = 36;

const JOB: &str = "The explore search's job id (default: the newest explore job).";

pub fn definitions() -> Vec<Value> {
    vec![
        tool(
            "explore_start",
            "Start a search",
            false,
            "Starts (or with `resume` carries on) the broad search over tilings, rule families, rules, and seeds, and returns its job id at once (explore_status follows it, explore_stop stops it). It runs for `hours` (or `minutes`), uses the GPU when there is one (`gpu`, default true), and stores every run in the job's own database. The search keeps the best runs of each kind of behaviour on each rule family and tiling group, breeds from those, and re-checks the best on bigger grids with more seeds. It is not deterministic: a stopped search continues from its archive. Example: {\"hours\": 2}; small: {\"minutes\": 2, \"tilings\": \"square,hex\", \"families\": \"life_like,cyclic\", \"screen_cells\": 4000}.",
            json!({"type": "object", "properties": {
                "hours": {"type": "number", "description": "Hours to run (default 1)."},
                "minutes": {"type": "number", "description": "Minutes to run, instead of hours."},
                "gpu": {"type": "boolean", "description": "Use the GPU when there is one (default true)."},
                "cpu_threads": {"type": "integer", "minimum": 1, "description": "Worker threads (default: all cores)."},
                "tilings": {"type": "string", "description": "Only these tiling keys, comma separated (default all: 1,359 today; the progress counts them)."},
                "families": {"type": "string", "description": "Only these rule families, comma separated (default all)."},
                "screen_cells": {"type": "integer", "minimum": 64, "description": "Cells of a screening run (default 16000)."},
                "screen_gens": {"type": "integer", "minimum": 1, "description": "Generations of a screening run (default 1024)."},
                "confirm_cells": {"type": "integer", "minimum": 64, "description": "Cells of a confirmation run, the best runs again on a bigger grid (default 64000)."},
                "confirm_gens": {"type": "integer", "minimum": 1, "description": "Generations of a confirmation run (default 4096)."},
                "label": {"type": "string", "description": "A name to find the job by."},
                "resume": {"type": "integer", "minimum": 1, "description": "Carry on this explore job instead of starting one."},
                "detach": {"type": "boolean", "description": "Run in a process of its own, outliving this server (default true where the server can)."}
            }, "additionalProperties": false}),
        ),
        tool(
            "explore_status",
            "Search progress",
            true,
            "Progress of an explore search: runs measured, runs per second, how many elites and behaviour niches it holds, its status, and a table of its runs by rule family (count, mean and best score). Without `job` and with `list`: every explore job.",
            json!({"type": "object", "properties": {
                "job": {"type": "integer", "description": JOB},
                "list": {"type": "boolean", "description": "List every explore job instead."}
            }, "additionalProperties": false}),
        ),
        tool(
            "explore_stop",
            "Stop a search",
            false,
            "Asks a running explore search to stop at its next checkpoint (a few seconds), wherever it runs. Everything it found stays in its database, and explore_start {\"resume\": job} continues it.",
            json!({"type": "object", "properties": {"job": {"type": "integer", "description": JOB}}, "additionalProperties": false}),
        ),
        tool(
            "explore_query",
            "Find runs",
            true,
            "Lists an explore search's stored runs, best first, with their measures. Filter with `family` (life_like, generations, ltl, jaca, cyclic, rps, greenberg_hastings, hodgepodge, priority, wireworld, turmite, layered, wolfram, hex_wolfram, hex_tri, margolus, lenia, sandpile, forest_fire, table, von_neumann, spectre_glider), `tiling`, `fate` (died, froze, cycle, running), `min_score`, `min_movers`, `stage` (1 screened, 2 confirmed on bigger grids; default 1), `reviewed` (any, keep, maybe, drop). Scores run about 0 to 3; higher is more interesting. For other questions, the search folder's sealed runs are Parquet files (runs/part-*.parquet beside search.sqlite) that DuckDB, Polars or pyarrow read directly.",
            json!({"type": "object", "properties": {
                "job": {"type": "integer", "description": JOB},
                "family": {"type": "string", "description": "Only this rule family."},
                "tiling": {"type": "string", "description": "Only this tiling key."},
                "fate": {"type": "string", "enum": ["died", "froze", "cycle", "running"], "description": "Only runs that ended this way."},
                "min_score": {"type": "number", "description": "Only runs with at least this score."},
                "min_movers": {"type": "integer", "description": "Only runs with at least this many moving objects."},
                "stage": {"type": "integer", "enum": [1, 2], "description": "1 screened (default), 2 confirmed on bigger grids."},
                "reviewed": {"type": "string", "enum": ["any", "keep", "maybe", "drop"], "description": "Only reviewed runs."},
                "limit": {"type": "integer", "minimum": 1, "maximum": MAX_ROWS, "description": "Most runs to return (default 40)."}
            }, "additionalProperties": false}),
        ),
        tool(
            "explore_sheet",
            "Contact sheet of runs",
            true,
            "Returns a picture of an explore search's runs at several moments, labelled with id, tiling, score, fate, rule, and measures, so they can be judged by eye. Give `ids` (at most 36 runs; explore_query finds them).",
            json!({"type": "object", "properties": {
                "job": {"type": "integer", "description": JOB},
                "ids": {"type": "array", "items": {"type": "integer"}, "description": "Run ids to show."},
                "px": {"type": "integer", "minimum": 64, "maximum": 400, "description": "Thumbnail size (default 200)."},
                "cols": {"type": "integer", "minimum": 1, "maximum": 20, "description": "Thumbnails per row (default 8)."},
                "fracs": {"type": "array", "items": {"type": "number"}, "description": "Moments, as fractions of the run (default [0.25, 1])."},
                "palette": {"type": "string", "description": "Draw every run in this palette."}
            }, "additionalProperties": false}),
        ),
        tool(
            "explore_try",
            "Try one configuration",
            false,
            "Runs one configuration of your own right now on the CPU and returns its fate, measures, score, and a picture. Good for testing an idea from explore_query, such as a rule on another tiling or a different seed. `seed` takes the seed shorthands (\"random:0.3\", \"pattern:blobs:0.4:by_region\", \"pattern:symmetric:0.5\").",
            json!({"type": "object", "properties": {
                "tiling": {"type": "string", "description": "Tiling key (square, hex, trihex, spectre, catalog:k2-001, ih28, ...)."},
                "rule": {"type": "string", "description": "Rule notation."},
                "seed": {"description": "Seed shorthand or object (default the rule's own)."},
                "nb": {"type": "string", "enum": ["touch", "edge"], "description": "Neighbourhood (default touch, or what the notation says)."},
                "cells": {"type": "integer", "minimum": 100, "maximum": 100000, "description": "About this many cells (default 16000)."},
                "gens": {"type": "integer", "minimum": 1, "maximum": 8000, "description": "Generations (default 1024)."},
                "rng": {"type": ["integer", "string"], "description": "Random seed (default 1). Any whole number to 2^64 - 1; beyond 2^53 as a string of digits (\"17705299566207515059\")."},
                "px": {"type": "integer", "minimum": 64, "maximum": 800, "description": "Picture size (default 420)."}
            }, "required": ["tiling", "rule"], "additionalProperties": false}),
        ),
        tool(
            "explore_scene",
            "Scene of a run",
            true,
            "Returns an explore search's stored run as scene JSON, ready for load_scene, to open and explore it in detail.",
            json!({"type": "object", "properties": {
                "job": {"type": "integer", "description": JOB},
                "id": {"type": "integer", "description": "Run id, from explore_query."}
            }, "required": ["id"], "additionalProperties": false}),
        ),
        tool(
            "explore_review",
            "Record a judgement",
            false,
            "Records what you think of an explore search's run: verdict `keep`, `maybe`, or `drop`, and a note. Reviews are used when choosing what is published and to tune the score.",
            json!({"type": "object", "properties": {
                "job": {"type": "integer", "description": JOB},
                "id": {"type": "integer", "description": "Run id, from explore_query."},
                "verdict": {"type": "string", "enum": ["keep", "maybe", "drop"], "description": "Your judgement."},
                "note": {"type": "string", "description": "What you saw, in a sentence."}
            }, "required": ["id", "verdict"], "additionalProperties": false}),
        ),
    ]
}

fn num(args: &Value, key: &str) -> Option<f64> {
    args.get(key).and_then(Value::as_f64)
}

fn start(args: &Value) -> Res {
    check_unknown(
        args,
        &[
            "hours",
            "minutes",
            "gpu",
            "cpu_threads",
            "tilings",
            "families",
            "screen_cells",
            "screen_gens",
            "confirm_cells",
            "confirm_gens",
            "label",
            "resume",
            "detach",
        ],
    )?;
    let minutes = match (num(args, "minutes"), num(args, "hours")) {
        (Some(m), _) => m,
        (None, Some(h)) => h * 60.0,
        (None, None) => 60.0,
    };
    let gpu = args.get("gpu").and_then(Value::as_bool).unwrap_or(true);
    let spec = ExploreIn {
        tilings: string(args, "tilings")?.map(|s| json!(s)),
        families: string(args, "families")?.map(|s| json!(s)),
        screen_cells: int(args, "screen_cells")?.map(|x| x.max(64) as u32),
        screen_gens: int(args, "screen_gens")?.map(|x| x.max(1) as u32),
        confirm_cells: int(args, "confirm_cells")?.map(|x| x.max(64) as u32),
        confirm_gens: int(args, "confirm_gens")?.map(|x| x.max(1) as u32),
        gpu: Some(gpu),
        ..Default::default()
    };
    let cx = ctx();
    let detach =
        args.get("detach").and_then(Value::as_bool).unwrap_or(true) && cx.platform.can_detach();
    let a = SearchStartIn {
        strategy: Some("explore".into()),
        spec: op::to_value(&spec),
        label: string(args, "label")?.map(String::from),
        resume: int(args, "resume")?,
        budget: Some(BudgetIn {
            minutes: Some(minutes),
            ..Default::default()
        }),
        threads: int(args, "cpu_threads")?.map(|t| t.max(0) as usize),
        gpu,
        background: !detach,
        detach,
        ..Default::default()
    };
    let out = SearchStart::run(cx, a, &Quiet).map_err(err)?;
    Ok(ToolOutput::json(&json!({
        "job": out.job,
        "started": out.created,
        "mode": out.mode,
        "minutes": minutes,
        "folder": out.progress.get("folder"),
        "note": format!(
            "The search runs {}: poll explore_status {{\"job\": {}}}; explore_stop stops it, and explore_start {{\"resume\": {}}} continues it. Results accumulate in its database as it runs.",
            if out.mode == "detached" { "in a process of its own" } else { "in the background" },
            out.job,
            out.job
        ),
    })))
}

/// The `job` argument (`None`: the operation takes the newest explore job).
fn job_arg(args: &Value) -> Result<Option<i64>, String> {
    int(args, "job")
}

fn status(args: &Value) -> Res {
    check_unknown(args, &["job", "list"])?;
    if args.get("list").and_then(Value::as_bool) == Some(true) {
        let l = JobsList::run(
            ctx(),
            ListIn {
                kind: Some("explore".into()),
                ..Default::default()
            },
            &Quiet,
        )
        .map_err(err)?;
        let jobs: Vec<Value> = l
            .jobs
            .iter()
            .map(
                |j| json!({"job": j.job, "status": j.status, "line": j.line, "updated": j.updated}),
            )
            .collect();
        return Ok(ToolOutput::json(&json!({"jobs": jobs})));
    }
    let job = match job_arg(args)? {
        Some(j) => j,
        None => {
            jetcarp_api::ops::explore::explore_job(ctx(), None)
                .map_err(err)?
                .1
                .id
        }
    };
    let StatusOut::One(v) = JobsStatus::run(
        ctx(),
        StatusIn {
            job: Some(job),
            ..Default::default()
        },
        &Quiet,
    )
    .map_err(err)?
    else {
        return Err(format!("no job {job}"));
    };
    if v.kind != "explore" {
        return Err(format!("job {job} is a {} job, not explore", v.kind));
    }
    let p = &v.progress;
    Ok(ToolOutput::json(&json!({
        "job": v.job,
        "status": v.status,
        "line": v.line,
        "screened": p.get("screened"),
        "confirmed": p.get("confirmed"),
        "per_sec": p.get("per_s"),
        "elites": p.get("elites"),
        "niches": p.get("niches"),
        "confirmed_elites": p.get("confirmed_elites"),
        "elapsed_s": p.get("elapsed_s"),
        "backend": p.get("backend"),
        "families": p.get("families"),
        "continues": "resuming continues the search from its archive (it is not deterministic)",
    })))
}

fn stop(args: &Value) -> Res {
    check_unknown(args, &["job"])?;
    let job = match job_arg(args)? {
        Some(j) => j,
        None => {
            jetcarp_api::ops::explore::explore_job(ctx(), None)
                .map_err(err)?
                .1
                .id
        }
    };
    let s = JobsStop::run(ctx(), JobIn { job }, &Quiet).map_err(err)?;
    Ok(ToolOutput::json(&json!({
        "job": job,
        "stopping": s.stopping,
        "note": format!("It stops at its next checkpoint; explore_start {{\"resume\": {job}}} continues it."),
    })))
}

fn query(args: &Value) -> Res {
    check_unknown(
        args,
        &[
            "job",
            "family",
            "tiling",
            "fate",
            "min_score",
            "min_movers",
            "stage",
            "reviewed",
            "limit",
        ],
    )?;
    let a = RunsIn {
        job: job_arg(args)?,
        family: string(args, "family")?.map(String::from),
        tiling: string(args, "tiling")?.map(String::from),
        fate: string(args, "fate")?.map(String::from),
        min_score: num(args, "min_score"),
        min_movers: int(args, "min_movers")?.map(|m| m.max(0) as u32),
        stage: int(args, "stage")?.map(|s| s.clamp(1, 2) as u8),
        reviewed: string(args, "reviewed")?.map(String::from),
        limit: Some(int(args, "limit")?.unwrap_or(40).clamp(1, MAX_ROWS) as usize),
        ..Default::default()
    };
    let r = SearchRuns::run(ctx(), a, &Quiet).map_err(err)?;
    Ok(ToolOutput::json(
        &json!({"job": r.job, "runs": op::to_value(&r.runs)}),
    ))
}

fn sheet_tool(args: &Value) -> Res {
    check_unknown(args, &["job", "ids", "px", "cols", "fracs", "palette"])?;
    let ids = args.get("ids").and_then(Value::as_array).map(|a| {
        a.iter()
            .filter_map(Value::as_i64)
            .take(MAX_SHEET)
            .collect::<Vec<i64>>()
    });
    let fracs = args
        .get("fracs")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(Value::as_f64).collect());
    let a = SheetIn {
        job: job_arg(args)?,
        ids,
        px: int(args, "px")?.unwrap_or(200).clamp(64, 400) as u32,
        cols: int(args, "cols")?.unwrap_or(8).clamp(1, 20) as u32,
        fracs,
        palette: string(args, "palette")?.map(String::from),
        max: Some(MAX_SHEET),
        png: None,
    };
    let s = SearchSheet::run(ctx(), a, &Quiet).map_err(err)?;
    Ok(ToolOutput {
        content: vec![
            json!({"type": "text", "text": format!("{} runs", s.runs.len())}),
            json!({"type": "image", "data": util::base64(&s.bytes), "mimeType": "image/png"}),
        ],
        is_error: false,
    })
}

fn try_tool(args: &Value) -> Res {
    check_unknown(
        args,
        &["tiling", "rule", "seed", "nb", "cells", "gens", "rng", "px"],
    )?;
    let a = TryIn {
        tiling: string(args, "tiling")?.ok_or("tiling is required")?.into(),
        rule: string(args, "rule")?.ok_or("rule is required")?.into(),
        seed: args.get("seed").cloned().filter(|v| !v.is_null()),
        nb: string(args, "nb")?.map(String::from),
        cells: Some(int(args, "cells")?.unwrap_or(16000).clamp(100, 100_000) as u32),
        gens: Some(int(args, "gens")?.unwrap_or(1024).clamp(1, 8000) as u32),
        rng: uint(args, "rng")?,
        px: Some(int(args, "px")?.unwrap_or(420).clamp(64, 800) as u32),
        png: None,
    };
    let t = SearchTry::run(ctx(), a, &Quiet).map_err(err)?;
    let text = json!({
        "family": t.family, "fate": t.fate, "score": t.score, "category": t.category,
        "settle": t.settle, "period": t.period, "density": t.density, "activity": t.activity,
        "movers": t.movers, "mover_speed": t.mover_speed, "smooth": t.smooth, "entropy": t.entropy,
    });
    Ok(ToolOutput {
        content: vec![
            json!({"type": "text", "text": text.to_string()}),
            json!({"type": "image", "data": util::base64(&t.png), "mimeType": "image/png"}),
        ],
        is_error: false,
    })
}

fn scene(args: &Value) -> Res {
    check_unknown(args, &["job", "id"])?;
    let a = RunIn {
        job: job_arg(args)?,
        run: int(args, "id")?.ok_or("id is required")?,
        save: None,
    };
    let r = SearchRunScene::run(ctx(), a, &Quiet).map_err(err)?;
    Ok(ToolOutput::json(&r.scene))
}

fn review(args: &Value) -> Res {
    check_unknown(args, &["job", "id", "verdict", "note"])?;
    let a = ReviewIn {
        job: job_arg(args)?,
        run: Some(int(args, "id")?.ok_or("id is required")?),
        verdict: Some(
            string(args, "verdict")?
                .ok_or("verdict is required")?
                .into(),
        ),
        note: string(args, "note")?.map(String::from),
        rater: Some("model".into()),
        from: None,
    };
    let r = SearchReview::run(ctx(), a, &Quiet).map_err(err)?;
    Ok(ToolOutput::text(format!(
        "recorded {} for run {} of job {}",
        args["verdict"].as_str().unwrap_or(""),
        args["id"],
        r.job
    )))
}

pub fn call(name: &str, args: &Value) -> Res {
    match name {
        "explore_start" => start(args),
        "explore_status" => status(args),
        "explore_stop" => stop(args),
        "explore_query" => query(args),
        "explore_sheet" => sheet_tool(args),
        "explore_try" => try_tool(args),
        "explore_scene" => scene(args),
        "explore_review" => review(args),
        _ => Err(format!("unknown tool {name}")),
    }
}
