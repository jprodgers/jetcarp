//! MCP tools for the pattern catalog (`jetcarp_core::library`): list, get, a bounded soup search,
//! paste into the world, curation updates, promotion to the Quick Start, and the Quick Start
//! entries ("My finds").
//!
//! The catalog is `JETCARP_LIBRARY` (a folder or a `.sqlite` file), else
//! `<data>/catalog.sqlite`, shared with the app, scripts, and `jetcarp library`.

use crate::tools::{
    Res, ToolOutput, boolean, ca_index, check_unknown, int, short_status, string, tool,
};
use jetcarp_api::world::Host;
use jetcarp_core::library::search::SearchSpec;
use jetcarp_core::library::{
    self, CurationState, DiscoveredBy, Library, PasteAt, PatternClass, PatternRecord, QsStatus,
    Query, Update,
};
use jetcarp_core::{Boundary, Command, SizeSpec};
use serde_json::{Value, json};

pub const NAMES: &[&str] = &[
    "library_list",
    "library_get",
    "library_search",
    "paste_pattern",
    "library_update",
    "library_promote",
    "library_collide",
];

/// Most soups one library_search call may run (over all rules and tilings).
pub const MAX_SEARCH_SOUPS: u64 = 256;
/// Most cell updates one library_search call may run.
pub const MAX_SEARCH_WORK: u64 = 4_000_000_000;

pub fn handles(name: &str) -> bool {
    NAMES.contains(&name)
}

fn lib() -> Library {
    Library::open_default()
}

pub fn definitions() -> Vec<Value> {
    vec![
        tool(
            "library_list",
            "List catalog records",
            true,
            "Lists records of the catalog, best first. `kind` picks what: \"pattern\" (default: verified still lifes, oscillators, spaceships, guns; rule-free, with every rule verified in and the rule range), \"ant\" (ant and turmite runs: outcome highway, periodic, symmetric, bounded, or chaotic, with period, displacement, onset, and the outcome census per tiling), or \"quickstart\" (Quick Start entries, \"My finds\"). Ids work with library_get, library_update, library_promote, media_render. Filters are optional and per kind; at most 100 per call. Examples: {\"class\": \"spaceship\", \"tiling\": \"hex\"}, {\"works_in\": \"B36/S23\"}, {\"kind\": \"ant\", \"outcome\": \"highway\", \"tiling\": \"hex\", \"sort\": \"onset\"}, {\"kind\": \"quickstart\"}.",
            json!({"type": "object", "properties": {
                "kind": {"type": "string", "enum": ["pattern", "ant", "quickstart"], "description": "What to list (default pattern)."},
                "rule": {"type": "string", "description": "pattern: verified in a rule whose notation contains this; ant: this turn string or table (\"RL\", \"LLRR\")."},
                "works_in": {"type": "string", "description": "pattern: runs in this rule (inside its rule range or verified there), e.g. \"B3678/S0235678\"."},
                "order": {"type": "string", "enum": ["score", "seen", "rare", "newest", "updated", "cells", "period", "rating"], "description": "pattern: sort order (default score)."},
                "sort": {"type": "string", "enum": ["score", "onset", "period", "speed", "rule"], "description": "ant: sort order (default score)."},
                "offset": {"type": "integer", "minimum": 0, "description": "pattern: skip this many (paging)."},
                "tiling": {"type": "string", "description": "Tiling family id (\"square\", \"hex\"), catalog id (\"k2-001\"), or name."},
                "class": {"type": "string", "enum": ["still_life", "oscillator", "spaceship", "puffer", "gun", "mover"], "description": "pattern: only this class."},
                "outcome": {"type": "string", "enum": ["highway", "periodic", "symmetric", "bounded", "chaotic"], "description": "ant: only this outcome."},
                "state": {"type": "string", "enum": ["new", "reviewed", "promoted", "rejected"], "description": "pattern: only this curation state."},
                "status": {"type": ["array", "string"], "items": {"type": "string"}, "description": "quickstart: auto, candidate, approved, or rejected (default: all)."},
                "job": {"type": "integer", "description": "ant: only records of this job."},
                "min_period": {"type": "integer", "minimum": 1, "description": "pattern: periods of at least this."},
                "min_score": {"type": "number", "description": "pattern: scores of at least this (0-100)."},
                "tag": {"type": "string", "description": "pattern: only entries with this tag."},
                "text": {"type": "string", "description": "pattern: part of the name or label."},
                "limit": {"type": "integer", "minimum": 1, "maximum": 100, "description": "Default 20."},
            }, "additionalProperties": false}),
        ),
        tool(
            "library_get",
            "Get one catalog record",
            true,
            "Returns one record by id: a pattern (the default), an ant run (ids ant-...: rule, tiling, outcome, period, displacement, onset, speed, the exact proof), or a Quick Start entry (find:<id>). A pattern record has: rule (the one it runs in by default), rules (every rule it was verified in), range (B/S and Generations: minimal and maximal rule it runs identically in, with bitmasks), sightings (copies seen per rule and search job), tiling, class, period, displacement (spaceships: lattice vector per period) and speed, cells, bounding box, the cells of every phase as `i,j,k,s` tuples (lattice column, row, tile type, state), verification (periods checked exactly), provenance (where and when it was found), seen count, score, rating, name, tags, notes, state. On square and hexagon grids it also draws the phases as text ('O' live, '.' empty; hexagon rows are offset by half a cell). `id` may be the full id, a unique prefix, or the pattern's name (\"glider\").",
            json!({"type": "object", "properties": {
                "id": {"type": "string", "description": "Pattern id, unique id prefix, or name; ant-..., or find:<id>."},
                "kind": {"type": "string", "enum": ["pattern", "ant", "quickstart"], "description": "What the id names (default: from the id's form)."},
                "phases": {"type": "integer", "minimum": 1, "maximum": 16, "description": "How many phases to draw (default 1)."},
                "record": {"type": "boolean", "description": "Include the full record JSON (default true)."},
            }, "required": ["id"], "additionalProperties": false}),
        ),
        tool(
            "library_search",
            "Search soups for patterns",
            false,
            "Runs random soups with the given rules on the given tilings, finds the still lifes, oscillators, and spaceships left at the end, verifies each exactly, and adds them to the catalog as one search job (store: false only reports). Bounded: at most 4 rules x 4 tilings, 256 soups in total, grids up to 128 x 128 unit cells, 4,000 generations, 4 billion cell updates; takes seconds to a minute. Returns per rule and tiling the soup outcomes, object counts by class, the distinct patterns (id, label, count, new or already known). Example: {\"rules\": [\"B2/S34\"], \"tilings\": [\"hex\"], \"soups\": 16}.",
            json!({"type": "object", "properties": {
                "rules": {"type": ["array", "string"], "items": {"type": "string"}, "description": "Rule notations, e.g. [\"B3/S23\", \"B2/S34\"]. Life-like and Generations rules without B0."},
                "tilings": {"type": ["array", "string"], "items": {"type": "string"}, "description": "Tiling ids: \"square\", \"hex\", \"triangle\", \"oct\", \"trihex\", catalog ids like \"k2-001\" (default [\"square\"])."},
                "soups": {"type": "integer", "minimum": 1, "maximum": 64, "description": "Soups per rule and tiling (default 16)."},
                "width": {"type": "integer", "minimum": 16, "maximum": 128, "description": "Grid width in unit cells (default 48)."},
                "height": {"type": "integer", "minimum": 16, "maximum": 128, "description": "Grid height (default 48)."},
                "gens": {"type": "integer", "minimum": 10, "maximum": 4000, "description": "Generations per soup (default 1000)."},
                "density": {"type": "number", "minimum": 0.0, "maximum": 1.0, "description": "Soup density (default 0.37)."},
                "seed": {"type": "integer", "minimum": 0, "description": "Soup s uses RNG state seed + s (default 1)."},
                "store": {"type": "boolean", "description": "Add what is found to the library (default true)."},
            }, "additionalProperties": false}),
        ),
        tool(
            "paste_pattern",
            "Paste a library pattern",
            false,
            "Pastes a library pattern into a CA's field of the current world (through the PastePattern command, so it is logged and replayable). The world must use the pattern's tiling; the CA usually runs the pattern's rule. Place it with `x`, `y` (the pattern's top-left corner, same coordinates as set_cells), or `index` (a cell index), or at: \"center\" (default); on aperiodic patches use `index` or at: \"original\" and the paste is refused unless the tiles around that cell match the pattern's surroundings. `orientation` turns it (0 = as stored; 0-7 on squares, 0-11 on hexagons; library_get shows which way a spaceship then moves) and `phase` picks the phase. Example: {\"id\": \"glider\", \"x\": 10, \"y\": 10, \"orientation\": 1}.",
            json!({"type": "object", "properties": {
                "id": {"type": "string", "description": "Pattern id, unique prefix, or name."},
                "ca": {"description": "Which CA's field (id or name; default the first CA).", "type": ["integer", "string"]},
                "x": {"type": "integer", "description": "Column of the pattern's top-left corner (give with y)."},
                "y": {"type": "integer", "description": "Row of the pattern's top-left corner."},
                "index": {"type": "integer", "minimum": 0, "description": "Cell index for the corner (lattice) or the anchor tile (aperiodic patches)."},
                "at": {"type": "string", "enum": ["center", "original"], "description": "\"center\" (default) or \"original\" (aperiodic: where it was found)."},
                "orientation": {"type": "integer", "minimum": 0, "maximum": 11, "description": "Index into the tiling's symmetries (0 = as stored)."},
                "phase": {"type": "integer", "minimum": 0, "description": "Which phase to paste (default 0)."},
            }, "required": ["id"], "additionalProperties": false}),
        ),
        tool(
            "library_update",
            "Curate a catalog record",
            false,
            "Curates a record. Patterns (default): name, tags (replace with `tags`, or add_tags / remove_tags), notes or append_notes, rating 1-5 (0 clears), state (new, reviewed, promoted, rejected), discovered_by. Ants (ids ant-...): name, notes, rating 1-5, state. Quick Start entries (find:<id>): `decision` approve (shown in the menu) or reject (hidden). Mark LLM-written text as such in the notes. Examples: {\"id\": \"glider\", \"rating\": 4}, {\"id\": \"find:sweep-1a2b\", \"decision\": \"reject\"}.",
            json!({"type": "object", "properties": {
                "id": {"type": ["string", "integer"], "description": "Pattern id, prefix, or name; ant-...; find:<id>."},
                "kind": {"type": "string", "enum": ["pattern", "ant", "quickstart"], "description": "What the id names (default: from the id's form)."},
                "name": {"type": "string", "description": "New name (empty clears it)."},
                "tags": {"type": "array", "items": {"type": "string"}, "description": "pattern: replace all tags."},
                "add_tags": {"type": "array", "items": {"type": "string"}, "description": "pattern: tags to add."},
                "remove_tags": {"type": "array", "items": {"type": "string"}, "description": "pattern: tags to remove."},
                "notes": {"type": "string", "description": "Replace the notes."},
                "append_notes": {"type": "string", "description": "pattern: add a paragraph to the notes."},
                "rating": {"type": "number", "minimum": 0, "maximum": 5, "description": "1 (dull) to 5 (remarkable); 0 clears (patterns)."},
                "state": {"type": "string", "description": "pattern: new, reviewed, promoted, rejected; ant: free text."},
                "discovered_by": {"type": "string", "enum": ["search", "script", "llm", "user"], "description": "pattern: who found it."},
                "decision": {"type": "string", "enum": ["approve", "reject"], "description": "quickstart: approve (shown) or reject (hidden)."},
            }, "required": ["id"], "additionalProperties": false}),
        ),
        tool(
            "library_promote",
            "Promote a pattern to Quick Start",
            false,
            "Adds an entry to the app's Quick Start under \"My finds\" (stored in the catalog; library_list {\"kind\": \"quickstart\"} lists them). An ant run (`id` ant-...): its build-up scene from step 0. A library pattern (`id`): a scene with the pattern centred on a suitable wrapped grid (spaceships get room to fly and wrap around); sets the pattern's state to promoted. Or one run of a sweep (`folder` and `run`, from a run_script sweep's output folder and its manifest): the run's tiling, rule, and start, so it replays as in the sweep; the run is also starred in the folder's favourites.json. Optional title and description (sweep runs: `description` is put before the generated one). Examples: {\"id\": \"3368e316\", \"title\": \"Trihex glider\"} or {\"folder\": \"/home/me/Pictures/JETCARP/2026-11-05_17-18-41_HexT3-sum\", \"run\": \"HexT3 sum 588\"}.",
            json!({"type": "object", "properties": {
                "id": {"type": "string", "description": "Pattern id, unique id prefix, or name (give this, or folder and run)."},
                "folder": {"type": "string", "description": "A sweep output folder (the 'output folder' line of run_script)."},
                "run": {"type": "string", "description": "With folder: the run's rule text (\"HexT3 sum 588\"), manifest index (\"#12\"), or series number (\"n=588\")."},
                "title": {"type": "string", "description": "Quick Start title (default: the name, label, or rule)."},
                "description": {"type": "string", "description": "Quick Start description (default: generated)."},
            }, "additionalProperties": false}),
        ),
        tool(
            "library_collide",
            "Collide two patterns",
            false,
            "Collides pattern `b` into pattern `a` at every offset in a range and every phase of b, in a rule both work in, and classes each run (both survive, both die, one survives, new objects, unsettled, explodes, overlap). New objects are verified and stored in the catalog (store: false only reports). Bounded: at most 5,000 placements and 4,096 generations. Example: {\"a\": \"block\", \"b\": \"glider\", \"offsets\": 4}.",
            json!({"type": "object", "properties": {
                "a": {"type": "string", "description": "Pattern id, prefix, or name (the target)."},
                "b": {"type": "string", "description": "Pattern id, prefix, or name (moved over the offsets)."},
                "offsets": {"type": "integer", "minimum": 0, "maximum": 30, "description": "B's corner offsets in -n..n on both axes (default 6)."},
                "dx": {"type": "array", "items": {"type": "integer"}, "description": "[lo, hi] instead of offsets for x."},
                "dy": {"type": "array", "items": {"type": "integer"}, "description": "[lo, hi] instead of offsets for y."},
                "phases": {"type": "array", "items": {"type": "integer"}, "description": "B's phases to try (default all)."},
                "orientation_a": {"type": "integer", "minimum": 0, "maximum": 11, "description": "A's orientation (index into the tiling's symmetries)."},
                "orientation_b": {"type": "integer", "minimum": 0, "maximum": 11, "description": "B's orientation."},
                "gens": {"type": "integer", "minimum": 16, "maximum": 4096, "description": "Generations per run (default 1024)."},
                "rule": {"type": "string", "description": "The rule to run (both must work in it; default: A's or B's)."},
                "store": {"type": "boolean", "description": "Store new objects (default true)."},
            }, "required": ["a", "b"], "additionalProperties": false}),
        ),
    ]
}

pub fn call(
    host: &mut dyn Host,
    name: &str,
    args: &Value,
    progress: Option<crate::tools::ProgressFn>,
    cancel: Option<std::sync::Arc<std::sync::atomic::AtomicBool>>,
) -> Res {
    match name {
        "library_list" => list(args),
        "library_get" => get(args),
        "library_search" => search_tool(args, progress, cancel),
        "library_collide" => collide_tool(args, progress),
        "paste_pattern" => paste(host, args),
        "library_update" => update(args),
        "library_promote" => promote(args),
        _ => Err(format!("unknown tool {name}")),
    }
}

fn summary(r: &PatternRecord) -> Value {
    json!({
        "id": r.id, "label": r.label(), "name": r.name, "rule": r.rule, "rules": r.rules,
        "range": r.range.as_ref().map(|x| x.describe()), "tiling": r.tiling_name,
        "class": r.class, "period": r.period, "speed": r.speed, "displacement": r.displacement,
        "cells": r.cells, "seen": r.seen, "score": r.score, "rating": r.rating, "state": r.state,
        "tags": r.tags,
    })
}

fn strings(v: Option<&Value>, key: &str) -> Result<Vec<String>, String> {
    match v {
        None | Some(Value::Null) => Ok(vec![]),
        Some(Value::String(s)) => Ok(s
            .split([',', ' ', ';'])
            .map(str::trim)
            .filter(|x| !x.is_empty())
            .map(String::from)
            .collect()),
        Some(Value::Array(a)) => a
            .iter()
            .map(|x| {
                x.as_str()
                    .map(|s| s.trim().to_string())
                    .ok_or_else(|| format!("`{key}` must be strings"))
            })
            .collect(),
        Some(v) => Err(format!("`{key}` must be a list of strings, got {v}")),
    }
}

/// The kind of record an id names: `kind` when given, else from the id's form.
fn kind_of<'a>(args: &'a Value, id: &str) -> Result<&'a str, String> {
    Ok(match string(args, "kind")? {
        Some(k) => k,
        None if id.starts_with("ant-") => "ant",
        None if id.starts_with("find:") => "quickstart",
        None => "pattern",
    })
}

fn without(args: &Value, keys: &[&str]) -> Value {
    let mut v = args.clone();
    if let Some(o) = v.as_object_mut() {
        for k in keys {
            o.remove(*k);
        }
    }
    v
}

fn list(args: &Value) -> Res {
    let rest = without(args, &["kind"]);
    match string(args, "kind")?.unwrap_or("pattern") {
        "pattern" => list_patterns(&rest),
        "ant" => list_ants(&rest),
        "quickstart" => quickstart_list(&rest),
        k => Err(format!("`kind` is pattern, ant, or quickstart, not {k}")),
    }
}

pub(crate) fn ant_json(r: &library::AntRow) -> Value {
    json!({
        "id": r.id, "rule": r.rule, "tiling": r.tiling_name, "start": r.start.label(),
        "outcome": r.outcome.name(), "period": r.period, "d": r.d, "speed": r.speed,
        "onset": r.onset, "steps": r.steps, "cells": r.cells, "score": r.score,
        "name": r.name, "rating": r.rating, "state": r.state, "line": r.line(),
    })
}

fn list_ants(args: &Value) -> Res {
    check_unknown(args, &["outcome", "tiling", "rule", "job", "sort", "limit"])?;
    let q = library::AntQuery {
        outcome: string(args, "outcome")?
            .map(jetcarp_core::ants::run::Outcome::parse)
            .transpose()?,
        tiling: string(args, "tiling")?.map(String::from),
        rule: string(args, "rule")?.map(String::from),
        job: int(args, "job")?,
        sort: string(args, "sort")?.map(String::from),
        limit: Some(int(args, "limit")?.unwrap_or(20).clamp(1, 100) as usize),
    };
    let l = lib();
    let rows = l.ants(&q)?;
    Ok(ToolOutput::json(&json!({
        "catalog": l.path().display().to_string(),
        "census": l.ant_census(q.job)?,
        "shown": rows.len(),
        "ants": rows.iter().map(ant_json).collect::<Vec<_>>(),
    })))
}

fn promote_ant(args: &Value, id: &str) -> Res {
    let l = lib();
    let r = l.ant(id)?;
    let mut sc = jetcarp_core::ants::media::build_scene(&r)?;
    if let Some(t) = string(args, "title")? {
        sc.name = t.to_string();
    }
    if let Some(d) = string(args, "description")? {
        sc.description = d.to_string();
    }
    let mut q = library::QuickStartRow::for_scene(&r.id, "scene", sc.clone());
    q.behaviour = r.outcome.name().into();
    q.score = Some(r.score);
    l.put_quickstart(&q)?;
    l.update_ant(
        &r.id,
        &library::AntUpdate {
            state: Some("promoted".into()),
            ..Default::default()
        },
    )?;
    Ok(ToolOutput::json(&json!({
        "promoted": sc.name,
        "description": sc.description,
        "entry": format!("find:{}", r.id),
        "note": "It appears in the JETCARP app's Quick Start under \"My finds\": the run from step 0.",
    })))
}

fn list_patterns(args: &Value) -> Res {
    check_unknown(
        args,
        &[
            "rule",
            "works_in",
            "tiling",
            "class",
            "state",
            "min_period",
            "min_score",
            "tag",
            "text",
            "order",
            "offset",
            "limit",
        ],
    )?;
    let limit = int(args, "limit")?.unwrap_or(20).clamp(1, 100) as usize;
    let mut q = Query {
        rule: string(args, "rule")?.map(String::from),
        works_in: string(args, "works_in")?.map(String::from),
        order: string(args, "order")?.map(String::from),
        offset: int(args, "offset")?.map(|x| x.max(0) as usize),
        tiling: string(args, "tiling")?.map(String::from),
        class: string(args, "class")?
            .map(PatternClass::parse)
            .transpose()?,
        state: string(args, "state")?
            .map(CurationState::parse)
            .transpose()?,
        min_period: int(args, "min_period")?.map(|x| x.max(1) as u64),
        min_score: args.get("min_score").and_then(|v| v.as_f64()),
        tag: string(args, "tag")?.map(String::from),
        text: string(args, "text")?.map(String::from),
        limit: None,
        ..Default::default()
    };
    let l = lib();
    let matching = l.count(&q)?;
    q.limit = Some(limit);
    let shown: Vec<Value> = l.query(&q)?.iter().map(summary).collect();
    Ok(ToolOutput::json(&json!({
        "catalog": l.path().display().to_string(),
        "matching": matching,
        "shown": shown.len(),
        "patterns": shown,
    })))
}

fn get(args: &Value) -> Res {
    check_unknown(args, &["id", "kind", "phases", "record"])?;
    let id = string(args, "id")?.ok_or("give the record's `id`")?;
    let l = lib();
    match kind_of(args, id)? {
        "pattern" => {}
        "ant" => {
            let r = l.ant(id)?;
            let mut out = ToolOutput::text(r.line());
            out.content.push(json!({"type": "text", "text": serde_json::to_string_pretty(&r).unwrap_or_default()}));
            return Ok(out);
        }
        "quickstart" => {
            let q = l.get_quickstart(id.trim_start_matches("find:"))?;
            return Ok(ToolOutput::json(
                &serde_json::to_value(&q).unwrap_or_default(),
            ));
        }
        k => return Err(format!("`kind` is pattern, ant, or quickstart, not {k}")),
    }

    let r = l.get(id)?;
    let n = int(args, "phases")?.unwrap_or(1).clamp(1, 16) as u64;
    let mut text = format!("{} ({})\n", r.label(), r.id);
    text.push_str(&format!(
        "verified in: {}\n",
        if r.rules.is_empty() {
            r.rule.clone()
        } else {
            r.rules.join(", ")
        }
    ));
    if let Some(x) = &r.range {
        text.push_str(&format!("works in: {}\n", x.describe()));
    }
    let sightings = l.sightings(&r.id)?;
    if !sightings.is_empty() {
        text.push_str(&format!(
            "sightings: {}\n",
            sightings
                .iter()
                .map(|s| format!("{} x{} (job {})", s.rule, s.count, s.job))
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    let moves = library::orientation_moves(&r)?;
    if !moves.is_empty() {
        text.push_str(&format!(
            "moves per period by paste orientation: {}\n",
            moves
                .iter()
                .enumerate()
                .map(|(k, v)| format!("{k}: ({}, {})", v[0], v[1]))
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    for t in 0..n.min(r.period.max(1)) {
        match library::ascii(&r, t)? {
            Some(s) => text.push_str(&format!("phase {t}:\n{s}")),
            None => text.push_str(&format!(
                "phase {t}: {}\n",
                r.phases[(t % r.period.max(1)) as usize]
            )),
        }
    }
    let mut out = ToolOutput::text(text);
    if args.get("record").and_then(|v| v.as_bool()).unwrap_or(true) {
        out.content
            .push(json!({"type": "text", "text": r.to_json()}));
    }
    Ok(out)
}

fn collide_tool(args: &Value, progress: Option<crate::tools::ProgressFn>) -> Res {
    use jetcarp_core::library::collide::{CollideSpec, collide};
    check_unknown(
        args,
        &[
            "a",
            "b",
            "offsets",
            "dx",
            "dy",
            "phases",
            "orientation_a",
            "orientation_b",
            "gens",
            "rule",
            "store",
        ],
    )?;
    let l = lib();
    let a = l.get(string(args, "a")?.ok_or("give pattern `a`")?)?;
    let b = l.get(string(args, "b")?.ok_or("give pattern `b`")?)?;
    let n = int(args, "offsets")?.unwrap_or(6).clamp(0, 30) as i32;
    let range = |k: &str| -> Result<[i32; 2], String> {
        match args.get(k) {
            None | Some(Value::Null) => Ok([-n, n]),
            Some(Value::Array(v)) if v.len() == 2 => Ok([
                v[0].as_i64().ok_or(format!("`{k}` is [lo, hi]"))? as i32,
                v[1].as_i64().ok_or(format!("`{k}` is [lo, hi]"))? as i32,
            ]),
            Some(_) => Err(format!("`{k}` is [lo, hi]")),
        }
    };
    let phases: Vec<u32> = match args.get("phases") {
        Some(Value::Array(v)) => v
            .iter()
            .filter_map(|x| x.as_u64())
            .map(|x| x as u32)
            .collect(),
        _ => vec![],
    };
    let spec = CollideSpec {
        dx: range("dx")?,
        dy: range("dy")?,
        phases,
        orientation_a: int(args, "orientation_a")?.unwrap_or(0).max(0) as u32,
        orientation_b: int(args, "orientation_b")?.unwrap_or(0).max(0) as u32,
        gens: int(args, "gens")?.unwrap_or(1024).clamp(16, 4096) as u64,
        rule: string(args, "rule")?.map(String::from),
        ..Default::default()
    };
    let places = (spec.dx[1] - spec.dx[0] + 1).max(0) as u64
        * (spec.dy[1] - spec.dy[0] + 1).max(0) as u64
        * if spec.phases.is_empty() {
            b.period.max(1)
        } else {
            spec.phases.len() as u64
        };
    if places > 5000 {
        return Err(format!(
            "{places} placements is more than 5,000: use smaller offsets or fewer phases"
        ));
    }
    let report = |d: usize, t: usize| {
        if let Some(f) = &progress
            && (d == t || d.is_multiple_of((t / 20).max(1)))
        {
            f(d as f64, Some(t as f64), format!("{d} of {t} placements"));
        }
    };
    let out = collide(&a, &b, &spec, 0, Some(&report))?;
    let store = args.get("store").and_then(|v| v.as_bool()).unwrap_or(true);
    let mut new_ids = Vec::new();
    if store && !out.records.is_empty() {
        let job = l.create_job(
            "collide",
            &serde_json::to_value(&out.report.spec).unwrap_or_default(),
        )?;
        let res = l.add_many(out.records.clone(), Some(job))?;
        for (rec, o) in out.records.iter().zip(res) {
            if o? == library::AddOutcome::Added {
                new_ids.push(rec.id.clone());
            }
        }
        l.update_job(job, Some(library::JobStatus::Done), None, None)?;
    }
    let r = &out.report;
    Ok(ToolOutput::json(&json!({
        "a": r.a_label, "b": r.b_label, "rule": r.rule, "tiling": r.tiling,
        "outcomes": r.counts,
        "products": out.records.iter().map(|x| json!({
            "id": x.id, "label": x.label(), "runs": x.seen, "new": new_ids.contains(&x.id),
        })).collect::<Vec<_>>(),
        "new_object_runs": r.runs.iter().filter(|x| x.outcome.name() == "new_objects").take(40).map(|x| json!({
            "dx": x.dx, "dy": x.dy, "phase": x.phase, "result": x.result,
        })).collect::<Vec<_>>(),
    })))
}

fn search_tool(
    args: &Value,
    progress: Option<crate::tools::ProgressFn>,
    cancel: Option<std::sync::Arc<std::sync::atomic::AtomicBool>>,
) -> Res {
    check_unknown(
        args,
        &[
            "rules", "tilings", "soups", "width", "height", "gens", "density", "seed", "store",
        ],
    )?;
    let rules = strings(args.get("rules"), "rules")?;
    if rules.is_empty() || rules.len() > 4 {
        return Err("give 1 to 4 `rules`, e.g. [\"B3/S23\"]".into());
    }
    let mut tl = strings(args.get("tilings"), "tilings")?;
    if tl.is_empty() {
        tl.push("square".into());
    }
    let tilings = library::tiling_list(&tl.join(","))?;
    if tilings.len() > 4 {
        return Err("at most 4 tilings per call".into());
    }
    let soups = int(args, "soups")?.unwrap_or(16).clamp(1, 64) as u32;
    let w = int(args, "width")?.unwrap_or(48).clamp(16, 128) as u32;
    let h = int(args, "height")?.unwrap_or(48).clamp(16, 128) as u32;
    let gens = int(args, "gens")?.unwrap_or(1000).clamp(10, 4000) as u64;
    let total_soups = soups as u64 * rules.len() as u64 * tilings.len() as u64;
    if total_soups > MAX_SEARCH_SOUPS {
        return Err(format!(
            "{total_soups} soups is more than {MAX_SEARCH_SOUPS}; use fewer soups, rules, or tilings"
        ));
    }
    // Multi-type tilings have several cells per unit cell; allow for up to 12.
    let work = total_soups * (w * h) as u64 * 12 * gens;
    if work > MAX_SEARCH_WORK * 3 {
        return Err(
            "that search is too large for one call; use smaller grids or fewer generations".into(),
        );
    }
    let spec = SearchSpec {
        rules,
        tilings,
        size: SizeSpec::Rect { w, h },
        boundary: Boundary::Wrap,
        soups,
        gens,
        density: args
            .get("density")
            .and_then(|v| v.as_f64())
            .unwrap_or(0.37)
            .clamp(0.0, 1.0),
        seed: int(args, "seed")?.unwrap_or(1).max(0) as u64,
        ..Default::default()
    };
    let store = args
        .get("store")
        .map(|_| boolean(args, "store"))
        .transpose()?
        .unwrap_or(true);
    // Progress (soups done) at most once a second, and at the end.
    let last = std::sync::Mutex::new(None::<std::time::Instant>);
    let report = |done: usize, total: usize| {
        if let Some(f) = &progress {
            let mut l = last.lock().unwrap();
            if done >= total || l.is_none_or(|t| t.elapsed().as_secs_f64() >= 1.0) {
                *l = Some(std::time::Instant::now());
                f(
                    done as f64,
                    Some(total as f64),
                    format!("{done} of {total} soups"),
                );
            }
        }
    };
    let out =
        jetcarp_core::library::search::search_cancellable(&spec, Some(&report), cancel.as_deref())?;
    let l = lib();
    let (mut new_ids, mut new_rules, mut job) = (Vec::new(), Vec::new(), None);
    if store {
        let st = l.store_search(&out)?;
        new_ids = st.added;
        new_rules = st.new_rules;
        job = Some(st.job);
    }
    let groups: Vec<Value> = out
        .report
        .groups
        .iter()
        .map(|g| {
            json!({
                "rule": g.rule, "tiling": g.tiling, "error": g.error,
                "soups": {"settled": g.settled, "active": g.active, "exploded": g.exploded},
                "objects_by_class": g.objects,
                "distinct": g.found.len(),
                "patterns": g.found.iter().take(25).map(|f| json!({
                    "id": f.id, "label": f.label, "class": f.class, "period": f.period, "speed": f.speed,
                    "cells": f.cells, "count": f.count, "verified": f.verified,
                    "new": new_ids.contains(&f.id),
                })).collect::<Vec<_>>(),
            })
        })
        .collect();
    Ok(ToolOutput::json(&json!({
        "stored": store,
        "catalog": l.path().display().to_string(),
        "job": job,
        "new_patterns": new_ids.len(),
        "known_patterns_in_new_rules": new_rules.iter().map(|(id, r)| json!({"id": id, "rule": r})).collect::<Vec<_>>(),
        "groups": groups,
        "next": "library_get an id to see it; paste_pattern to put it in a world; library_update to curate.",
    })))
}

fn paste(host: &mut dyn Host, args: &Value) -> Res {
    check_unknown(
        args,
        &["id", "ca", "x", "y", "index", "at", "orientation", "phase"],
    )?;
    let id = string(args, "id")?.ok_or("give the pattern `id`")?;
    let rec = lib().get(id)?;
    let w = host.world();
    let ci = ca_index(w, args.get("ca"))?;
    let ca_id = w.cas[ci].id;
    let at = match (
        int(args, "x")?,
        int(args, "y")?,
        int(args, "index")?,
        string(args, "at")?,
    ) {
        (Some(x), Some(y), None, None) => PasteAt::Cell {
            x: x as i32,
            y: y as i32,
        },
        (None, None, Some(i), None) => PasteAt::Index {
            cell: i.max(0) as u32,
        },
        (None, None, None, Some("original")) => PasteAt::Original,
        (None, None, None, Some("center")) => PasteAt::Center,
        (None, None, None, None) => {
            if rec.site.is_some() {
                PasteAt::Original
            } else {
                PasteAt::Center
            }
        }
        _ => {
            return Err(
                "give x and y, or index, or at (\"center\"/\"original\"), not several".into(),
            );
        }
    };
    let phase = int(args, "phase")?.unwrap_or(0).max(0) as u64;
    let orientation = int(args, "orientation")?.unwrap_or(0).max(0) as u32;
    let mut warn = String::new();
    let ca_rule = w.cas[ci].notation();
    if w.cas[ci].nb != rec.neighborhood || !rec.works_in(&ca_rule) {
        warn = format!(
            "\nNote: the pattern runs as stored in {}{}; this CA runs {ca_rule}, so it may behave differently.",
            rec.rule,
            rec.range
                .as_ref()
                .map(|x| format!(" ({})", x.describe()))
                .unwrap_or_default(),
        );
    }
    host.exec(Command::PastePattern {
        id: ca_id,
        pattern: library::paste::source_from_record(&rec, phase),
        at,
        orientation,
        phase: 0,
    })?;
    Ok(ToolOutput::text(format!(
        "Pasted {} ({}, {} cells) into CA {ca_id}.{warn}\n{}",
        rec.label(),
        rec.id,
        rec.phase(phase)?.len(),
        short_status(host.world())
    )))
}

fn update(args: &Value) -> Res {
    let id = match args.get("id") {
        Some(Value::Number(n)) => n.to_string(),
        Some(Value::String(s)) => s.clone(),
        _ => return Err("give the record's `id`".into()),
    };
    let rest = without(args, &["kind"]);
    match kind_of(args, &id)? {
        "pattern" => update_pattern(&rest),
        "ant" => {
            check_unknown(&rest, &["id", "name", "notes", "rating", "state"])?;
            let r = lib().update_ant(
                &id,
                &library::AntUpdate {
                    name: string(&rest, "name")?.map(String::from),
                    notes: string(&rest, "notes")?.map(String::from),
                    rating: rest
                        .get("rating")
                        .and_then(|v| v.as_f64())
                        .map(|x| x.round() as i64),
                    state: string(&rest, "state")?.map(String::from),
                },
            )?;
            Ok(ToolOutput::json(&ant_json(&r)))
        }
        "quickstart" => {
            check_unknown(&rest, &["id", "decision"])?;
            let st = match string(&rest, "decision")? {
                Some("approve") => QsStatus::Approved,
                Some("reject") => QsStatus::Rejected,
                _ => return Err("give `decision`: approve or reject".into()),
            };
            let qid = id.trim_start_matches("find:");
            lib().set_quickstart_status(qid, st)?;
            Ok(ToolOutput::json(
                &json!({"id": format!("find:{qid}"), "status": st}),
            ))
        }
        k => Err(format!("`kind` is pattern, ant, or quickstart, not {k}")),
    }
}

fn update_pattern(args: &Value) -> Res {
    check_unknown(
        args,
        &[
            "id",
            "name",
            "tags",
            "add_tags",
            "remove_tags",
            "notes",
            "append_notes",
            "rating",
            "state",
            "discovered_by",
        ],
    )?;
    let id = string(args, "id")?.ok_or("give the pattern `id`")?;
    let u = Update {
        name: string(args, "name")?.map(String::from),
        tags: args
            .get("tags")
            .map(|v| strings(Some(v), "tags"))
            .transpose()?,
        add_tags: strings(args.get("add_tags"), "add_tags")?,
        remove_tags: strings(args.get("remove_tags"), "remove_tags")?,
        notes: string(args, "notes")?.map(String::from),
        append_notes: string(args, "append_notes")?.map(String::from),
        rating: int(args, "rating")?.map(|x| x.clamp(0, 255) as u8),
        state: string(args, "state")?
            .map(CurationState::parse)
            .transpose()?,
        discovered_by: string(args, "discovered_by")?
            .map(DiscoveredBy::parse)
            .transpose()?,
    };
    let r = lib().update(id, &u)?;
    Ok(ToolOutput::json(&summary(&r)))
}

fn promote(args: &Value) -> Res {
    check_unknown(args, &["id", "folder", "run", "title", "description"])?;
    if let Some(folder) = string(args, "folder")? {
        if string(args, "id")?.is_some() {
            return Err(
                "give `id` (a library pattern) or `folder` and `run` (a sweep run), not both"
                    .into(),
            );
        }
        let run = string(args, "run")?
            .ok_or("give `run`: the rule text, \"#index\", or \"n=number\" of a row in the folder's manifest")?;
        let p = jetcarp_script::results::promote_folder_run(
            std::path::Path::new(folder),
            run,
            &lib(),
            string(args, "title")?,
            string(args, "description")?,
        )?;
        return Ok(ToolOutput::json(&json!({
            "promoted": p.title,
            "rule": p.rule,
            "run": p.index,
            "entry": format!("find:{}", p.id),
            "note": "It appears in the JETCARP app's Quick Start under \"My finds\" (the app refreshes when you return to the Quick Start).",
        })));
    }
    let id = string(args, "id")?
        .ok_or("give the pattern `id` (or `folder` and `run` for a sweep run)")?;
    if id.starts_with("ant-") {
        return promote_ant(args, id);
    }
    let (sc, entry) = library::promote::promote(
        &lib(),
        id,
        string(args, "title")?,
        string(args, "description")?,
    )?;
    Ok(ToolOutput::json(&json!({
        "promoted": sc.name,
        "description": sc.description,
        "entry": format!("find:{entry}"),
        "note": "It appears in the JETCARP app's Quick Start under \"My finds\" (the app refreshes when you return to the Quick Start).",
    })))
}

fn quickstart_list(args: &Value) -> Res {
    check_unknown(args, &["status", "limit"])?;
    let st = match string(args, "status")? {
        Some(s) => vec![QsStatus::parse(s)?],
        None => vec![],
    };
    let limit = int(args, "limit")?.unwrap_or(50).clamp(1, 200) as usize;
    let l = lib();
    let all = l.quickstart(&st)?;
    let shown: Vec<Value> = all
        .iter()
        .rev()
        .take(limit)
        .map(|q| {
            json!({
                "id": format!("find:{}", q.id), "title": q.title, "description": q.description,
                "grid": q.grid, "rule": q.rule, "rule_family": q.rule_family,
                "behaviour": q.behaviour, "source_kind": q.source_kind, "source_id": q.source_id,
                "score": q.score, "status": q.status, "created": q.created,
            })
        })
        .collect();
    Ok(ToolOutput::json(&json!({
        "catalog": l.path().display().to_string(),
        "entries": all.len(),
        "shown": shown.len(),
        "newest_first": shown,
    })))
}
