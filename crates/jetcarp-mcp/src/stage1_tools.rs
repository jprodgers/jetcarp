//! MCP tool for the stage 1 settings search (`jetcarp_core::stage1`): `stage1_start` runs a search
//! in the background. Searches are catalog jobs, watched and stopped with `job_status` and
//! `job_stop` (`job_tools.rs`) wherever they were started; the settings are listed by
//! `library_list {kind: setting}` and rated, approved, or rejected by `library_update`, which call
//! the functions here.

use crate::tools::{Res, ToolOutput, check_unknown, int, string, tool};
use jetcarp_core::library::{Library, SettingStatus, SettingsQuery};
use jetcarp_core::stage1::job::{self, Stage1Handle, Stage1Options, Stage1Progress};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

pub const NAMES: &[&str] = &["stage1_start"];

pub fn handles(name: &str) -> bool {
    NAMES.contains(&name)
}

/// Searches running in this server process.
fn running() -> &'static Mutex<HashMap<i64, Stage1Handle>> {
    static R: OnceLock<Mutex<HashMap<i64, Stage1Handle>>> = OnceLock::new();
    R.get_or_init(Default::default)
}

fn lib() -> Library {
    Library::open_default()
}

const SPEC_KEYS: &[&str] = &[
    "tilings",
    "nb",
    "states",
    "birth",
    "survive",
    "rules",
    "all_max",
    "sample",
    "density",
    "soup",
    "grid",
    "boundary",
    "gens",
    "soups",
    "seed",
    "refine",
    "neighbours",
];

pub fn definitions() -> Vec<Value> {
    vec![tool(
        "stage1_start",
        "Start a settings search",
        false,
        "Starts a stage 1 settings search in the background and returns at once with its job id (poll job_status, stop with job_stop). Every B/S rule of each tiling (squares with touching neighbours: 131,072 rules; hexagons: 8,192; larger spaces such as triangles are sampled) runs from seeded random soups on a wrapped grid and is scored 0-100 for visual interest (lives, looks neither noise nor uniform, objects such as spaceships and oscillators, holds up across soups and densities, new compared with the Quick Start). The best settings are then refined (density, soup size, grid size, generations) and their rule-space neighbours searched. Settings are stored in the catalog, and the Quick Start shows the atlas of `jetcarp explore` instead of them; candidates wait for approval (library_list {\"kind\": \"setting\"}, then library_update with a rating or a decision). Results do not depend on threads; {\"resume\": 4} continues job 4. Examples: {\"tilings\": \"hex\"}, {\"tilings\": \"square\", \"sample\": 20000, \"all_max\": 1000}, {\"tilings\": \"square\", \"states\": [3, 4], \"birth\": \"2-4\", \"survive\": \"1-5\", \"refine\": false}.",
        json!({"type": "object", "properties": {
                "tilings": {"type": ["array", "string"], "items": {"type": "string"}, "description": "Tilings: family ids, catalog ids, or sets such as \"archimedean\" (default \"hex\")."},
                "nb": {"type": "string", "enum": ["touch", "edge", "both"], "description": "Neighbourhood (default touch)."},
                "states": {"type": ["array", "integer"], "items": {"type": "integer"}, "description": "2 = B/S; more = Generations (default [2])."},
                "birth": {"type": "string", "description": "Birth digits, like \"1-6\" (default all but 0)."},
                "survive": {"type": "string", "description": "Survival digits, like \"0-6\" (default all)."},
                "rules": {"type": ["array", "string"], "items": {"type": "string"}, "description": "Explicit rules instead of the digit space."},
                "all_max": {"type": "integer", "minimum": 1, "description": "Every rule of a space up to this many (default 131072)."},
                "sample": {"type": "integer", "minimum": 1, "description": "Rules sampled from larger spaces (default 20000)."},
                "density": {"type": "number", "minimum": 0, "maximum": 1, "description": "Soup density (default 0.35)."},
                "soup": {"type": "number", "minimum": 0, "description": "Soup size: 1 fills the grid, less is a centred disc of that radius (default 1)."},
                "grid": {"type": "string", "description": "Grid size WxH in unit cells (default \"64x64\")."},
                "boundary": {"type": "string", "enum": ["wrap", "dead"], "description": "Grid edges (default wrap)."},
                "gens": {"type": "integer", "minimum": 1, "description": "Generations per soup (default 1000)."},
                "soups": {"type": "integer", "minimum": 1, "description": "Soups per setting (default 4, plus two density probes)."},
                "seed": {"type": "integer", "minimum": 0, "description": "Soup seed (default 1); soup i uses an RNG state from (seed, i)."},
                "refine": {"description": "false, or {top, densities, soups, grids, gens}."},
                "neighbours": {"description": "false, or {top, rounds}."},
                "threads": {"type": "integer", "minimum": 0, "description": "Worker threads (default 0 = all cores)."},
                "resume": {"type": "integer", "minimum": 1, "description": "Continue this search job instead of starting a new one."},
            }, "additionalProperties": false}),
    )]
}

pub fn call(name: &str, args: &Value) -> Res {
    match name {
        "stage1_start" => start(args),
        _ => Err(format!("unknown tool {name}")),
    }
}

fn summary(p: &Stage1Progress) -> Value {
    let mut v = serde_json::to_value(p).unwrap_or_default();
    v["line"] = json!(p.line());
    v
}

fn id_of(args: &Value) -> Result<String, String> {
    match args.get("id") {
        Some(Value::Number(n)) => Ok(n.to_string()),
        Some(Value::String(s)) => Ok(s.clone()),
        _ => Err("give `id`".into()),
    }
}

fn start(args: &Value) -> Res {
    let mut keys = SPEC_KEYS.to_vec();
    keys.extend(["threads", "resume"]);
    check_unknown(args, &keys)?;
    let l = lib();
    let jid = match int(args, "resume")? {
        Some(j) => {
            if running()
                .lock()
                .unwrap()
                .get(&j)
                .is_some_and(|h| !h.is_finished())
            {
                return Err(format!("stage 1 job {j} is already running here"));
            }
            job::status(&l, j)?;
            j
        }
        None => {
            let mut spec_args = serde_json::Map::new();
            for k in SPEC_KEYS {
                if let Some(v) = args.get(*k) {
                    spec_args.insert(k.to_string(), v.clone());
                }
            }
            let spec = jetcarp_script::stage1::spec_from(&Value::Object(spec_args))?;
            job::start(&l, &spec)?
        }
    };
    let opts = Stage1Options {
        threads: int(args, "threads")?.unwrap_or(0).max(0) as usize,
        ..Default::default()
    };
    let h = Stage1Handle::spawn(l.clone(), jid, opts);
    let p = h.progress();
    running().lock().unwrap().insert(jid, h);
    Ok(ToolOutput::json(&json!({
        "job": jid,
        "status": "running",
        "catalog": l.path().display().to_string(),
        "progress": summary(&p),
        "note": "The search runs in the background: poll job_status {\"job\": ...}; job_stop pauses it (resumable with stage1_start {\"resume\": job}).",
    })))
}

/// Stop a search running in this process; false when it is not running here.
pub(crate) fn stop_here(jid: i64) -> bool {
    match running().lock().unwrap().get(&jid) {
        Some(h) => {
            h.stop();
            true
        }
        None => false,
    }
}

/// The progress as `job_status` shows it.
pub(crate) fn status_json(l: &Library, jid: i64) -> Result<Value, String> {
    Ok(summary(&progress_of(l, jid)?))
}

fn progress_of(l: &Library, jid: i64) -> Result<Stage1Progress, String> {
    if let Some(h) = running().lock().unwrap().get(&jid) {
        let p = h.progress();
        if !h.is_finished() || p.status != "running" {
            return Ok(p);
        }
    }
    job::status(l, jid).map(|x| x.1)
}

pub(crate) fn candidates(args: &Value) -> Res {
    check_unknown(
        args,
        &[
            "status",
            "job",
            "tiling",
            "rule",
            "behaviour",
            "min_score",
            "limit",
        ],
    )?;
    let status: Vec<SettingStatus> = match args.get("status") {
        None | Some(Value::Null) => vec![SettingStatus::Candidate],
        Some(Value::String(s)) => vec![SettingStatus::parse(s)?],
        Some(Value::Array(a)) => a
            .iter()
            .map(|x| SettingStatus::parse(x.as_str().unwrap_or("")))
            .collect::<Result<_, _>>()?,
        Some(x) => return Err(format!("`status` must be text or a list, got {x}")),
    };
    let q = SettingsQuery {
        status,
        job: int(args, "job")?,
        tiling: string(args, "tiling")?.map(String::from),
        rule: string(args, "rule")?.map(String::from),
        behaviour: string(args, "behaviour")?.map(String::from),
        min_score: args.get("min_score").and_then(|v| v.as_f64()),
        limit: int(args, "limit")?.unwrap_or(20).clamp(1, 500) as usize,
        ..Default::default()
    };
    let l = lib();
    let rows = l.settings(&q)?;
    let counts = l.settings_counts(&SettingsQuery::default())?;
    let out: Vec<Value> = rows
        .iter()
        .map(|r| {
            let d = &r.descriptors;
            json!({
                "id": r.id, "entry_id": r.entry_id(), "rule": r.notation, "tiling": r.tiling_name,
                "score": r.score, "status": r.status.name(), "behaviour": r.behaviour,
                "tags": d.tags, "setting": r.setting,
                "facts": {
                    "died": d.died, "settled": d.settled, "active": d.active,
                    "activity_end": d.activity_end, "final_density": d.final_density,
                    "compress": d.compress, "entropy": d.entropy, "ship_kinds": d.ship_kinds,
                    "oscillator_kinds": d.oscillator_kinds, "longest_period": d.longest_period,
                    "distinct": d.distinct, "novelty": d.novelty,
                },
            })
        })
        .collect();
    Ok(ToolOutput::json(&json!({
        "catalog": l.path().display().to_string(),
        "counts": counts,
        "settings": out,
    })))
}

pub(crate) fn rate(args: &Value) -> Res {
    check_unknown(args, &["id", "rating", "rater", "note"])?;
    let id = id_of(args)?;
    let rating = args
        .get("rating")
        .and_then(|v| v.as_f64())
        .ok_or("give `rating` (1 to 5)")?;
    let rater = string(args, "rater")?.unwrap_or("model:mcp");
    let note = string(args, "note")?.unwrap_or("");
    let n = lib().rate("setting", &id, rater, rating, note)?;
    Ok(ToolOutput::json(
        &json!({"rating_id": n, "setting": id, "rating": rating, "rater": rater}),
    ))
}

pub(crate) fn review(args: &Value) -> Res {
    check_unknown(args, &["id", "decision", "rating", "note"])?;
    let id = id_of(args)?;
    let st = match string(args, "decision")? {
        Some("approve") => SettingStatus::Approved,
        Some("reject") => SettingStatus::Rejected,
        _ => return Err("`decision` is approve or reject".into()),
    };
    let rating = args.get("rating").and_then(|v| v.as_f64());
    let note = string(args, "note")?.unwrap_or("");
    let r = lib().review_setting(&id, st, rating, note)?;
    Ok(ToolOutput::json(&json!({
        "id": r.id, "status": r.status.name(), "entry_id": r.entry_id(),
        "rule": r.notation, "score": r.score,
    })))
}
