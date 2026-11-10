//! MCP tools for the exploratory search (`jetcarp_core::explore`), so a model can run a search,
//! look at what it finds, try ideas of its own, and record what it thinks.
//!
//! A database is `db`: a path, or a bare name inside `<Documents>/jetcarp-out/explore`
//! (default `search`).

use crate::tools::{Res, ToolOutput, check_unknown, int, string, tool};
use crate::util;
use jetcarp_core::explore::cand::Cand;
use jetcarp_core::explore::job::{feat_from_blob, try_one};
use jetcarp_core::explore::sheet::{self, RunRow};
use jetcarp_core::explore::space::{Fam, sized_spec};
use jetcarp_core::rules::Rule;
use jetcarp_core::topology::Nb;
use rusqlite::{Connection, OpenFlags};
use serde_json::{Value, json};
use std::path::PathBuf;

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

fn db_path(args: &Value) -> Result<PathBuf, String> {
    let name = string(args, "db")?.unwrap_or("search");
    if name.contains('/') || name.ends_with(".sqlite") {
        return Ok(PathBuf::from(name));
    }
    let base = dirs::document_dir()
        .ok_or("no Documents folder")?
        .join("jetcarp-out/explore");
    Ok(base.join(format!("{name}.sqlite")))
}

fn read(args: &Value) -> Result<Connection, String> {
    let p = db_path(args)?;
    Connection::open_with_flags(&p, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map_err(|e| format!("{}: {e}", p.display()))
}

const DB: &str = "Database: a path, or a name inside Documents/jetcarp-out/explore (default \"search\").";

pub fn definitions() -> Vec<Value> {
    vec![
        tool(
            "explore_start",
            "Start a search",
            false,
            "Starts the broad search over tilings, rules, seeds in the background, and returns at once (explore_status watches, explore_stop stops). It runs for `hours`, resumes from the database if it already exists, uses the GPU when one is available, and stores every run in SQLite. The search keeps the best run of each kind of behaviour on each rule family and tiling group, breeds from those, and re-checks the best on bigger grids with more seeds.",
            json!({"type": "object", "properties": {
                "db": {"type": "string", "description": DB},
                "hours": {"type": "number", "description": "Hours to run (default 1)."},
                "gpu": {"type": "boolean", "description": "Use the GPU (default true)."},
                "cpu_threads": {"type": "integer", "minimum": 1, "description": "CPU threads for rules the GPU cannot run (default 12)."}
            }, "additionalProperties": false}),
        ),
        tool(
            "explore_status",
            "Search progress",
            true,
            "Progress of a search: runs measured, runs per second, how many elites and behaviour niches it holds, and a table of the runs by rule family (count, mean and best score).",
            json!({"type": "object", "properties": {"db": {"type": "string", "description": DB}}, "additionalProperties": false}),
        ),
        tool(
            "explore_stop",
            "Stop a search",
            false,
            "Asks a running search to stop at its next checkpoint. Everything it found stays in the database, and explore_start resumes from there.",
            json!({"type": "object", "properties": {"db": {"type": "string", "description": DB}}, "additionalProperties": false}),
        ),
        tool(
            "explore_query",
            "Find runs",
            true,
            "Lists stored runs, best first, with their measures. Filter with `family` (life_like, generations, ltl, jaca, cyclic, rps, greenberg_hastings, hodgepodge, priority, wireworld, turmite, layered, wolfram, hex_wolfram, hex_tri, margolus, lenia, sandpile, forest_fire, table, von_neumann, spectre_glider), `tiling`, `fate` (died, froze, cycle, running), `min_score`, `min_movers`, `stage` (1 screened, 2 confirmed on bigger grids; default 1); or give `sql` that returns run ids (`SELECT id FROM runs WHERE ...`, read only: columns id, stage, parent, tiling, nb, fam, rule, seed, seed_kind, cells, gens, steps, rng, origin, fate, settle, period, score, bin, ts). Scores run about 0 to 4; higher is more interesting.",
            json!({"type": "object", "properties": {
                "db": {"type": "string", "description": DB},
                "family": {"type": "string", "description": "Only this rule family."},
                "tiling": {"type": "string", "description": "Only this tiling key."},
                "fate": {"type": "string", "enum": ["died", "froze", "cycle", "running"], "description": "Only runs that ended this way."},
                "min_score": {"type": "number", "description": "Only runs with at least this score."},
                "min_movers": {"type": "integer", "description": "Only runs with at least this many moving objects."},
                "stage": {"type": "integer", "enum": [1, 2], "description": "1 screened (default), 2 confirmed on bigger grids."},
                "sql": {"type": "string", "description": "A single SELECT returning run ids, instead of the filters."},
                "limit": {"type": "integer", "minimum": 1, "maximum": MAX_ROWS, "description": "Most runs to return (default 40)."}
            }, "additionalProperties": false}),
        ),
        tool(
            "explore_sheet",
            "Contact sheet of runs",
            true,
            "Returns a picture of runs at several moments, labelled with id, tiling, score, fate, rule, and measures, so they can be judged by eye. Give `ids` or `sql` (at most 36 runs).",
            json!({"type": "object", "properties": {
                "db": {"type": "string", "description": DB},
                "ids": {"type": "array", "items": {"type": "integer"}, "description": "Run ids to show."},
                "sql": {"type": "string", "description": "A single SELECT returning run ids, instead of ids."},
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
                "rng": {"type": "integer", "description": "Random seed (default 1)."},
                "px": {"type": "integer", "minimum": 64, "maximum": 800, "description": "Picture size (default 420)."}
            }, "required": ["tiling", "rule"], "additionalProperties": false}),
        ),
        tool(
            "explore_scene",
            "Scene of a run",
            true,
            "Returns a stored run as scene JSON, ready for load_scene, to open and explore it in detail.",
            json!({"type": "object", "properties": {
                "db": {"type": "string", "description": DB},
                "id": {"type": "integer", "description": "Run id, from explore_query."}
            }, "required": ["id"], "additionalProperties": false}),
        ),
        tool(
            "explore_review",
            "Record a judgement",
            false,
            "Records what you think of a run: verdict `keep`, `maybe`, or `drop`, and a note. Reviews are used when choosing what is published and to tune the score.",
            json!({"type": "object", "properties": {
                "db": {"type": "string", "description": DB},
                "id": {"type": "integer", "description": "Run id, from explore_query."},
                "verdict": {"type": "string", "enum": ["keep", "maybe", "drop"], "description": "Your judgement."},
                "note": {"type": "string", "description": "What you saw, in a sentence."}
            }, "required": ["id", "verdict"], "additionalProperties": false}),
        ),
    ]
}

fn row_json(r: &RunRow) -> Value {
    let f = feat_from_blob(&r.feats);
    json!({
        "id": r.id, "tiling": r.cand.tiling, "family": r.cand.fam.name(), "rule": r.cand.rule,
        "nb": if r.cand.nb == Nb::Edge {"edge"} else {"touch"},
        "seed": r.cand.seed.describe(), "cells": r.cand.cells, "gens": r.cand.gens,
        "fate": r.fate, "score": (r.score * 100.0).round() / 100.0,
        "settle": f.settle, "period": f.period, "density": f.density_late,
        "activity": f.activity_late, "movers": f.movers, "smooth": f.smooth,
    })
}

fn find_jetcarp() -> Result<PathBuf, String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    if exe.file_name().is_some_and(|n| n == "jetcarp") {
        return Ok(exe);
    }
    let sib = exe.with_file_name("jetcarp");
    if sib.exists() {
        return Ok(sib);
    }
    Ok(PathBuf::from("jetcarp"))
}

fn start(args: &Value) -> Res {
    check_unknown(args, &["db", "hours", "gpu", "cpu_threads"])?;
    let db = db_path(args)?;
    if let Some(p) = db.parent() {
        std::fs::create_dir_all(p).map_err(|e| e.to_string())?;
    }
    let hours = args.get("hours").and_then(Value::as_f64).unwrap_or(1.0);
    let mut c = std::process::Command::new(find_jetcarp()?);
    c.args(["explore", "run", "--db"]).arg(&db);
    c.args(["--hours", &hours.to_string()]);
    if let Some(t) = int(args, "cpu_threads")? {
        c.args(["--cpu-threads", &t.to_string()]);
    }
    if args.get("gpu").and_then(Value::as_bool) == Some(false) {
        c.arg("--no-gpu");
    }
    let log = std::fs::File::create(db.with_extension("log")).map_err(|e| e.to_string())?;
    c.stdout(log.try_clone().map_err(|e| e.to_string())?).stderr(log);
    let child = c.spawn().map_err(|e| e.to_string())?;
    Ok(ToolOutput::json(&json!({
        "started": true, "pid": child.id(), "db": db, "hours": hours,
        "note": "poll explore_status; results accumulate in the database as it runs"
    })))
}

fn status(args: &Value) -> Res {
    check_unknown(args, &["db"])?;
    let db = db_path(args)?;
    let live = std::fs::read_to_string(db.with_extension("status.json"))
        .ok()
        .and_then(|t| serde_json::from_str::<Value>(&t).ok());
    let conn = read(args)?;
    let e = |x: rusqlite::Error| x.to_string();
    let mut st = conn
        .prepare(
            "SELECT fam, COUNT(*), ROUND(AVG(score),2), ROUND(MAX(score),2) FROM runs
             WHERE stage=1 GROUP BY fam ORDER BY COUNT(*) DESC",
        )
        .map_err(e)?;
    let fams: Vec<Value> = st
        .query_map([], |r| {
            Ok(json!({
                "family": r.get::<_, String>(0)?, "runs": r.get::<_, i64>(1)?,
                "mean_score": r.get::<_, f64>(2)?, "best_score": r.get::<_, f64>(3)?,
            }))
        })
        .map_err(e)?
        .flatten()
        .collect();
    Ok(ToolOutput::json(&json!({"live": live, "families": fams})))
}

fn stop(args: &Value) -> Res {
    check_unknown(args, &["db"])?;
    let db = db_path(args)?;
    std::fs::write(db.with_extension("stop"), "stop").map_err(|e| e.to_string())?;
    Ok(ToolOutput::text("asked the search to stop"))
}

fn safe_sql(sql: &str) -> Result<String, String> {
    let t = sql.trim().trim_end_matches(';');
    if !t.to_ascii_lowercase().starts_with("select") || t.contains(';') {
        return Err("sql must be a single SELECT that returns run ids".into());
    }
    Ok(t.to_string())
}

fn query(args: &Value) -> Res {
    check_unknown(
        args,
        &[
            "db",
            "family",
            "tiling",
            "fate",
            "min_score",
            "min_movers",
            "stage",
            "sql",
            "limit",
        ],
    )?;
    let conn = read(args)?;
    let limit = int(args, "limit")?.unwrap_or(40).clamp(1, MAX_ROWS);
    let sql = if let Some(s) = string(args, "sql")? {
        format!("SELECT * FROM ({}) LIMIT {limit}", safe_sql(s)?)
    } else {
        let mut w = vec![format!("stage={}", int(args, "stage")?.unwrap_or(1))];
        let quote = |s: &str| s.replace('\'', "''");
        if let Some(f) = string(args, "family")? {
            w.push(format!("fam='{}'", quote(f)));
        }
        if let Some(f) = string(args, "tiling")? {
            w.push(format!("tiling='{}'", quote(f)));
        }
        if let Some(f) = string(args, "fate")? {
            w.push(format!("fate='{}'", quote(f)));
        }
        if let Some(m) = args.get("min_score").and_then(Value::as_f64) {
            w.push(format!("score>={m}"));
        }
        format!(
            "SELECT id FROM runs WHERE {} ORDER BY score DESC LIMIT {limit}",
            w.join(" AND ")
        )
    };
    let mut rows = sheet::query_runs(&conn, &sql)?;
    if let Some(m) = int(args, "min_movers")? {
        rows.retain(|r| feat_from_blob(&r.feats).movers as i64 >= m);
    }
    let v: Vec<Value> = rows.iter().map(row_json).collect();
    Ok(ToolOutput::json(&json!({"runs": v})))
}

fn sheet_tool(args: &Value) -> Res {
    check_unknown(args, &["db", "ids", "sql", "px", "cols", "fracs", "palette"])?;
    let conn = read(args)?;
    let sql = if let Some(s) = string(args, "sql")? {
        safe_sql(s)?
    } else if let Some(a) = args.get("ids").and_then(Value::as_array) {
        let ids: Vec<String> = a.iter().filter_map(Value::as_i64).map(|i| i.to_string()).collect();
        format!("SELECT id FROM runs WHERE id IN ({})", ids.join(","))
    } else {
        return Err("give ids or sql".into());
    };
    let mut rows = sheet::query_runs(&conn, &sql)?;
    rows.truncate(MAX_SHEET);
    let fracs: Vec<f64> = args
        .get("fracs")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(Value::as_f64).collect())
        .unwrap_or_else(|| vec![0.25, 1.0]);
    let px = int(args, "px")?.unwrap_or(200).clamp(64, 400) as u32;
    let cols = int(args, "cols")?.unwrap_or(8).clamp(1, 20) as u32;
    let img = sheet::sheet(&rows, &fracs, px, cols, string(args, "palette")?);
    let bytes = util::png_bytes(&img, &[])?;
    Ok(ToolOutput {
        content: vec![
            json!({"type": "text", "text": format!("{} runs", rows.len())}),
            json!({"type": "image", "data": util::base64(&bytes), "mimeType": "image/png"}),
        ],
        is_error: false,
    })
}

fn try_tool(args: &Value) -> Res {
    check_unknown(
        args,
        &["tiling", "rule", "seed", "nb", "cells", "gens", "rng", "px"],
    )?;
    let tiling = string(args, "tiling")?.ok_or("tiling is required")?;
    let text = string(args, "rule")?.ok_or("rule is required")?;
    let (rule, nb0) = Rule::parse(text)?;
    let fam = Fam::of(&rule);
    let nb = match string(args, "nb")? {
        Some("edge") => Nb::Edge,
        Some("touch") => Nb::Touch,
        Some(o) => return Err(format!("nb must be touch or edge, got {o}")),
        None => nb0.unwrap_or(Nb::Touch),
    };
    let cells = int(args, "cells")?.unwrap_or(16000).clamp(100, 100_000) as u32;
    let gens = int(args, "gens")?.unwrap_or(1024).clamp(1, 8000) as u32;
    let rng = int(args, "rng")?.unwrap_or(1) as u64;
    let (spec, topo) = sized_spec(tiling, cells)?;
    let seed = match args.get("seed") {
        Some(v) if !v.is_null() => jetcarp_script::spec::parse_seed(v)?,
        _ => jetcarp_core::world::default_seed(&rule, topo.len()),
    };
    let cand = Cand {
        tiling: tiling.to_string(),
        nb,
        fam,
        rule: rule.notation(),
        seed,
        cells,
        gens,
        steps: 1,
        rng,
        origin: "try".into(),
    };
    let ev = try_one(cand.clone())?;
    let _ = spec;
    let px = int(args, "px")?.unwrap_or(420).clamp(64, 800) as u32;
    let img = sheet::thumb(&cand, 1.0, px, None).ok_or("could not render")?;
    let bytes = util::png_bytes(&img, &[])?;
    let f = &ev.feat;
    let text = json!({
        "fate": format!("{:?}", ev.fate).to_lowercase(), "score": ev.score, "settle": f.settle,
        "period": f.period, "density": f.density_late, "activity": f.activity_late,
        "movers": f.movers, "mover_speed": f.mover_speed, "smooth": f.smooth, "entropy": f.entropy,
    });
    Ok(ToolOutput {
        content: vec![
            json!({"type": "text", "text": text.to_string()}),
            json!({"type": "image", "data": util::base64(&bytes), "mimeType": "image/png"}),
        ],
        is_error: false,
    })
}

fn scene(args: &Value) -> Res {
    check_unknown(args, &["db", "id"])?;
    let conn = read(args)?;
    let id = int(args, "id")?.ok_or("id is required")?;
    let rows = sheet::query_runs(&conn, &format!("SELECT id FROM runs WHERE id={id}"))?;
    let r = rows.first().ok_or_else(|| format!("no run {id}"))?;
    let (spec, _) = sized_spec(&r.cand.tiling, r.cand.cells)?;
    Ok(ToolOutput::json(
        &serde_json::to_value(r.cand.scene(&spec)).map_err(|e| e.to_string())?,
    ))
}

fn review(args: &Value) -> Res {
    check_unknown(args, &["db", "id", "verdict", "note"])?;
    let db = db_path(args)?;
    let id = int(args, "id")?.ok_or("id is required")?;
    let verdict = string(args, "verdict")?.ok_or("verdict is required")?;
    if !matches!(verdict, "keep" | "maybe" | "drop") {
        return Err("verdict must be keep, maybe, or drop".into());
    }
    let note = string(args, "note")?.unwrap_or("");
    let c = Connection::open(&db).map_err(|e| e.to_string())?;
    c.busy_timeout(std::time::Duration::from_secs(10))
        .map_err(|e| e.to_string())?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    c.execute(
        "INSERT INTO reviews(run_id, verdict, note, ts) VALUES(?,?,?,?)",
        rusqlite::params![id, verdict, note, now],
    )
    .map_err(|e| e.to_string())?;
    Ok(ToolOutput::text(format!("recorded {verdict} for run {id}")))
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
