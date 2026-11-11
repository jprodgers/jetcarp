//! MCP tools for the pattern catalog (`jetcarp_store::library`): list, get, a bounded soup search,
//! paste into the world, curation updates, promotion to the Quick Start, and the Quick Start
//! entries ("My finds").
//!
//! The catalog is `JETCARP_LIBRARY` (a folder or a `.sqlite` file), else
//! `<data>/library.sqlite`, shared with the app, scripts, and `jetcarp library`; the server opens
//! it once. Listing, reading, curation, and promotion are the operations `library.*` and
//! `quickstart.*`; the soup search and collisions are jobs run in the call (`search.start`),
//! bounded by the server's limits (`Limits::INTERACTIVE`, which the operation checks).

use crate::tools::{Res, ToolOutput, boolean, check_unknown, ctx, err, given, int, string, tool};
use jetcarp_api::Limits;
use jetcarp_api::op::{self, Callbacks, Op, Quiet, Report, WorldOp};
use jetcarp_api::ops::library::{
    GetIn, LibraryGet, LibraryQuery, LibraryUpdate, QueryIn, UpdateIn,
};
use jetcarp_api::ops::quickstart::{
    AddIn, IdIn, ListIn, QuickstartAdd, QuickstartGet, QuickstartList, QuickstartUpdate, StatusIn,
};
use jetcarp_api::ops::search::{BoundedIn, CollideIn, SearchStart, SearchStartIn, SearchStarted};
use jetcarp_api::ops::world::{PasteIn, WorldPaste};
use jetcarp_api::world::Host;
use jetcarp_core::pattern::PatternRecord;
use jetcarp_search::dashboard::digits;
use jetcarp_search::jobs::search::SearchResult;
use jetcarp_store::Library;
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

pub fn handles(name: &str) -> bool {
    NAMES.contains(&name)
}

/// The catalog: the server's one connection to it.
fn lib() -> Library {
    ctx().library()
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
            &format!(
                "Runs random soups with the given rules on the given tilings, finds the still lifes, oscillators, and spaceships left at the end, verifies each exactly, and adds them to the catalog as one search job (store: false only reports). Bounded: at most {r} rules x {r} tilings, {s} soups in total, grids up to 128 x 128 unit cells, {g} generations, {w} cell updates; takes seconds to a minute. Returns per rule and tiling the soup outcomes, object counts by class, the distinct patterns (id, label, count, new or already known). Example: {{\"rules\": [\"B2/S34\"], \"tilings\": [\"hex\"], \"soups\": 16}}.",
                r = Limits::INTERACTIVE.max_search_rules,
                s = Limits::INTERACTIVE.max_search_soups,
                g = digits(Limits::INTERACTIVE.max_search_gens),
                w = billions(Limits::INTERACTIVE.max_search_work)
            ),
            json!({"type": "object", "properties": {
                "rules": {"type": ["array", "string"], "items": {"type": "string"}, "description": "Rule notations, e.g. [\"B3/S23\", \"B2/S34\"]. Life-like and Generations rules without B0."},
                "tilings": {"type": ["array", "string"], "items": {"type": "string"}, "description": "Tiling ids: \"square\", \"hex\", \"triangle\", \"oct\", \"trihex\", catalog ids like \"k2-001\" (default [\"square\"])."},
                "soups": {"type": "integer", "minimum": 1, "maximum": 64, "description": "Soups per rule and tiling (default 16)."},
                "width": {"type": "integer", "minimum": 16, "maximum": 128, "description": "Grid width in unit cells (default 48)."},
                "height": {"type": "integer", "minimum": 16, "maximum": 128, "description": "Grid height (default 48)."},
                "gens": {"type": "integer", "minimum": 10, "maximum": Limits::INTERACTIVE.max_search_gens, "description": "Generations per soup (default 1000)."},
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
            &format!(
                "Collides pattern `b` into pattern `a` at every offset in a range and every phase of b, in a rule both work in, and classes each run (both survive, both die, one survives, new objects, unsettled, explodes, overlap). New objects are verified and stored in the catalog (store: false only reports). Bounded: at most {p} placements and {g} generations. Example: {{\"a\": \"block\", \"b\": \"glider\", \"offsets\": 4}}.",
                p = digits(Limits::INTERACTIVE.max_collide_runs),
                g = digits(Limits::INTERACTIVE.max_collide_gens)
            ),
            json!({"type": "object", "properties": {
                "a": {"type": "string", "description": "Pattern id, prefix, or name (the target)."},
                "b": {"type": "string", "description": "Pattern id, prefix, or name (moved over the offsets)."},
                "offsets": {"type": "integer", "minimum": 0, "maximum": 30, "description": "B's corner offsets in -n..n on both axes (default 6)."},
                "dx": {"type": "array", "items": {"type": "integer"}, "description": "[lo, hi] instead of offsets for x."},
                "dy": {"type": "array", "items": {"type": "integer"}, "description": "[lo, hi] instead of offsets for y."},
                "phases": {"type": "array", "items": {"type": "integer"}, "description": "B's phases to try (default all)."},
                "orientation_a": {"type": "integer", "minimum": 0, "maximum": 11, "description": "A's orientation (index into the tiling's symmetries)."},
                "orientation_b": {"type": "integer", "minimum": 0, "maximum": 11, "description": "B's orientation."},
                "gens": {"type": "integer", "minimum": 16, "maximum": Limits::INTERACTIVE.max_collide_gens, "description": "Generations per run (default 1024)."},
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

pub(crate) fn ant_json(r: &jetcarp_store::AntRow) -> Value {
    json!({
        "id": r.id, "rule": r.rule, "tiling": r.tiling_name, "start": r.start.label(),
        "outcome": r.outcome.name(), "period": r.period, "d": r.d, "speed": r.speed,
        "onset": r.onset, "steps": r.steps, "cells": r.cells, "score": r.score,
        "name": r.name, "rating": r.rating, "state": r.state, "line": r.line(),
    })
}

fn text(args: &Value, key: &str) -> Result<Option<String>, String> {
    Ok(string(args, key)?.map(String::from))
}

fn list_ants(args: &Value) -> Res {
    check_unknown(args, &["outcome", "tiling", "rule", "job", "sort", "limit"])?;
    let q = QueryIn {
        kind: Some("ant".into()),
        outcome: text(args, "outcome")?,
        tiling: text(args, "tiling")?,
        rule: text(args, "rule")?,
        job: int(args, "job")?,
        sort: text(args, "sort")?,
        limit: Some(int(args, "limit")?.unwrap_or(20).clamp(1, 100) as usize),
        ..Default::default()
    };
    let out = LibraryQuery::run(ctx(), q, &Quiet).map_err(err)?;
    Ok(ToolOutput::json(&json!({
        "catalog": out.catalog,
        "census": out.census,
        "shown": out.ants.len(),
        "ants": out.ants.iter().map(ant_json).collect::<Vec<_>>(),
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
    let q = QueryIn {
        rule: text(args, "rule")?,
        works_in: text(args, "works_in")?,
        order: text(args, "order")?,
        offset: int(args, "offset")?.map(|x| x.max(0) as usize),
        tiling: text(args, "tiling")?,
        class: text(args, "class")?,
        state: text(args, "state")?,
        min_period: int(args, "min_period")?.map(|x| x.max(1) as u64),
        min_score: args.get("min_score").and_then(|v| v.as_f64()),
        tag: text(args, "tag")?,
        text: text(args, "text")?,
        limit: Some(limit),
        ..Default::default()
    };
    let out = LibraryQuery::run(ctx(), q, &Quiet).map_err(err)?;
    let shown: Vec<Value> = out.patterns.iter().map(summary).collect();
    Ok(ToolOutput::json(&json!({
        "catalog": out.catalog,
        "matching": out.matching,
        "shown": shown.len(),
        "patterns": shown,
    })))
}

fn get(args: &Value) -> Res {
    check_unknown(args, &["id", "kind", "phases", "record"])?;
    let id = string(args, "id")?.ok_or("give the record's `id`")?;
    let kind = kind_of(args, id)?;
    match kind {
        "pattern" | "ant" => {}
        "quickstart" => {
            let q = QuickstartGet::run(ctx(), IdIn { id: id.into() }, &Quiet).map_err(err)?;
            return Ok(ToolOutput::json(&op::to_value(&q)));
        }
        k => return Err(format!("`kind` is pattern, ant, or quickstart, not {k}")),
    }
    let n = if kind == "pattern" {
        int(args, "phases")?.unwrap_or(1).clamp(1, 16) as u64
    } else {
        1
    };
    let a = GetIn {
        id: id.into(),
        kind: Some(kind.into()),
        phases: Some(n),
        bare: false,
    };
    let v = LibraryGet::run(ctx(), a, &Quiet).map_err(err)?;
    if let Some(r) = &v.ant {
        let mut out = ToolOutput::text(r.line());
        out.content.push(
            json!({"type": "text", "text": serde_json::to_string_pretty(r).unwrap_or_default()}),
        );
        return Ok(out);
    }
    let Some(r) = &v.pattern else {
        return Err(format!("no record '{id}'"));
    };
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
    if !v.sightings.is_empty() {
        text.push_str(&format!(
            "sightings: {}\n",
            v.sightings
                .iter()
                .map(|s| format!("{} x{} (job {})", s.rule, s.count, s.job))
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    if !v.orientations.is_empty() {
        text.push_str(&format!(
            "moves per period by paste orientation: {}\n",
            v.orientations
                .iter()
                .enumerate()
                .map(|(k, v)| format!("{k}: ({}, {})", v[0], v[1]))
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    for p in v.phases.iter().take(n.min(r.period.max(1)) as usize) {
        text.push_str(&p.text());
    }
    let mut out = ToolOutput::text(text);
    if args.get("record").and_then(|v| v.as_bool()).unwrap_or(true) {
        out.content
            .push(json!({"type": "text", "text": r.to_json()}));
    }
    Ok(out)
}

/// Progress and cancellation of a call, for an operation.
fn callbacks(
    progress: Option<crate::tools::ProgressFn>,
    cancel: Option<std::sync::Arc<std::sync::atomic::AtomicBool>>,
) -> Callbacks {
    Callbacks {
        on_report: progress.map(|f| {
            std::sync::Arc::new(move |d: u64, t: Option<u64>, m: &str| {
                f(d as f64, t.map(|t| t as f64), m.to_string())
            }) as std::sync::Arc<dyn Fn(u64, Option<u64>, &str) + Send + Sync>
        }),
        cancel,
    }
}

/// Run a search or collision here, as a job (`search.start`), until it ends or is cancelled.
fn run_here(strategy: &str, spec: Value, p: &Callbacks) -> Result<SearchStarted, String> {
    let a = SearchStartIn {
        strategy: Some(strategy.into()),
        spec,
        ..Default::default()
    };
    SearchStart::run(ctx(), a, p).map_err(err)
}

fn collide_tool(args: &Value, progress: Option<crate::tools::ProgressFn>) -> Res {
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
    // The bounds on one call (placements, generations) are the context's limits, checked by
    // the operation.
    let max_gens = ctx().limits.max_collide_gens;
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
    let (dx, dy) = (range("dx")?, range("dy")?);
    let spec = CollideIn {
        a: a.id,
        b: b.id,
        dx: Some(json!(dx)),
        dy: Some(json!(dy)),
        phases: Some(json!(phases)),
        orientation_a: Some(int(args, "orientation_a")?.unwrap_or(0).max(0) as u32),
        orientation_b: Some(int(args, "orientation_b")?.unwrap_or(0).max(0) as u32),
        gens: Some((int(args, "gens")?.unwrap_or(1024).max(16) as u64).min(max_gens)),
        rule: string(args, "rule")?.map(String::from),
        store: Some(args.get("store").and_then(|v| v.as_bool()).unwrap_or(true)),
        ..Default::default()
    };
    let out = run_here("collide", op::to_value(&spec), &callbacks(progress, None))?;
    let r = out
        .result
        .ok_or_else(|| format!("collision {} did not finish ({})", out.job, out.status))?;
    let rep = &r["report"];
    let runs: Vec<Value> = rep["runs"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|x| x["outcome"] == "new_objects")
        .take(40)
        .map(|x| json!({"dx": x["dx"], "dy": x["dy"], "phase": x["phase"], "result": x["result"]}))
        .collect();
    Ok(ToolOutput::json(&json!({
        "a": rep["a_label"], "b": rep["b_label"], "rule": rep["rule"], "tiling": rep["tiling"],
        "outcomes": rep["counts"],
        "products": r["products"],
        "new_object_runs": runs,
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
    if rules.is_empty() {
        return Err("give the `rules`, e.g. [\"B3/S23\"]".into());
    }
    let mut tl = strings(args.get("tilings"), "tilings")?;
    if tl.is_empty() {
        tl.push("square".into());
    }
    // The bounds on one call (rules, tilings, soups in all, generations, cell updates) are the
    // context's limits, checked by the operation.
    let max_gens = ctx().limits.max_search_gens;
    let soups = int(args, "soups")?.unwrap_or(16).clamp(1, 64) as u32;
    let w = int(args, "width")?.unwrap_or(48).clamp(16, 128) as u32;
    let h = int(args, "height")?.unwrap_or(48).clamp(16, 128) as u32;
    let gens = (int(args, "gens")?.unwrap_or(1000).max(10) as u64).min(max_gens);
    let store = args
        .get("store")
        .map(|_| boolean(args, "store"))
        .transpose()?
        .unwrap_or(true);
    let spec = BoundedIn {
        rules: Some(json!(rules)),
        tilings: Some(json!(tl.join(","))),
        width: Some(w),
        height: Some(h),
        boundary: Some("wrap".into()),
        soups: Some(soups),
        gens: Some(gens),
        density: Some(
            args.get("density")
                .and_then(|v| v.as_f64())
                .unwrap_or(0.37)
                .clamp(0.0, 1.0),
        ),
        seed: Some(int(args, "seed")?.unwrap_or(1).max(0) as u64),
        store: Some(store),
        ..Default::default()
    };
    let out = run_here("search", op::to_value(&spec), &callbacks(progress, cancel))?;
    let r: SearchResult = out
        .result
        .and_then(|r| serde_json::from_value(r).ok())
        .ok_or_else(|| format!("search {} stopped before its end ({})", out.job, out.status))?;
    let groups: Vec<Value> = r
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
                    "new": f.new,
                })).collect::<Vec<_>>(),
            })
        })
        .collect();
    Ok(ToolOutput::json(&json!({
        "stored": store,
        "catalog": out.catalog,
        "job": out.job,
        "new_patterns": r.added.len(),
        "known_patterns_in_new_rules": r.new_rules.iter().map(|(id, r)| json!({"id": id, "rule": r})).collect::<Vec<_>>(),
        "groups": groups,
        "next": "library_get an id to see it; paste_pattern to put it in a world; library_update to curate.",
    })))
}

fn paste(host: &mut dyn Host, args: &Value) -> Res {
    check_unknown(
        args,
        &["id", "ca", "x", "y", "index", "at", "orientation", "phase"],
    )?;
    let a = PasteIn {
        id: string(args, "id")?
            .ok_or("give the pattern `id`")?
            .to_string(),
        ca: given(args, "ca"),
        x: int(args, "x")?,
        y: int(args, "y")?,
        index: int(args, "index")?,
        at: string(args, "at")?.map(String::from),
        orientation: int(args, "orientation")?,
        phase: int(args, "phase")?,
    };
    let out = WorldPaste::run(crate::tools::ctx(), host, a, &Quiet).map_err(err)?;
    Ok(ToolOutput::text(out.text()))
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
            let a = UpdateIn {
                id,
                kind: Some("ant".into()),
                name: text(&rest, "name")?,
                notes: text(&rest, "notes")?,
                rating: rest.get("rating").and_then(|v| v.as_f64()),
                state: text(&rest, "state")?,
                ..Default::default()
            };
            let r = LibraryUpdate::run(ctx(), a, &Quiet).map_err(err)?;
            Ok(ToolOutput::json(
                &r.ant.as_ref().map(ant_json).unwrap_or_default(),
            ))
        }
        "quickstart" => {
            check_unknown(&rest, &["id", "decision"])?;
            let status = match string(&rest, "decision")? {
                Some("approve") => "approved",
                Some("reject") => "rejected",
                _ => return Err("give `decision`: approve or reject".into()),
            };
            let a = StatusIn {
                id,
                status: status.into(),
            };
            let r = QuickstartUpdate::run(ctx(), a, &Quiet).map_err(err)?;
            Ok(ToolOutput::json(&json!({"id": r.id, "status": r.status})))
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
    let a = UpdateIn {
        id: id.into(),
        kind: Some("pattern".into()),
        name: text(args, "name")?,
        tags: args
            .get("tags")
            .map(|v| strings(Some(v), "tags"))
            .transpose()?,
        add_tags: strings(args.get("add_tags"), "add_tags")?,
        remove_tags: strings(args.get("remove_tags"), "remove_tags")?,
        notes: text(args, "notes")?,
        append_notes: text(args, "append_notes")?,
        rating: int(args, "rating")?.map(|x| x.clamp(0, 255) as f64),
        state: text(args, "state")?,
        discovered_by: text(args, "discovered_by")?,
    };
    let r = LibraryUpdate::run(ctx(), a, &Quiet).map_err(err)?;
    Ok(ToolOutput::json(
        &r.pattern.as_ref().map(summary).unwrap_or_default(),
    ))
}

/// Where a promoted entry shows up.
const IN_THE_APP: &str = "It appears in the JETCARP app's Quick Start under \"My finds\" (the app refreshes when you return to the Quick Start).";

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
        let a = AddIn {
            folder: Some(folder.into()),
            run: Some(run.into()),
            title: text(args, "title")?,
            note: text(args, "description")?,
            ..Default::default()
        };
        let p = QuickstartAdd::run(ctx(), a, &Quiet).map_err(err)?;
        return Ok(ToolOutput::json(&json!({
            "promoted": p.title,
            "rule": p.rule,
            "run": p.run,
            "entry": p.entry,
            "note": IN_THE_APP,
        })));
    }
    let id = string(args, "id")?
        .ok_or("give the pattern `id` (or `folder` and `run` for a sweep run)")?;
    let ant = id.starts_with("ant-");
    let a = AddIn {
        pattern: (!ant).then(|| id.to_string()),
        ant: ant.then(|| id.to_string()),
        title: text(args, "title")?,
        description: text(args, "description")?,
        ..Default::default()
    };
    let p = QuickstartAdd::run(ctx(), a, &Quiet).map_err(err)?;
    Ok(ToolOutput::json(&json!({
        "promoted": p.title,
        "description": p.description,
        "entry": p.entry,
        "note": if ant {
            "It appears in the JETCARP app's Quick Start under \"My finds\": the run from step 0."
        } else {
            IN_THE_APP
        },
    })))
}

fn quickstart_list(args: &Value) -> Res {
    check_unknown(args, &["status", "limit"])?;
    let status = text(args, "status")?;
    let limit = int(args, "limit")?.unwrap_or(50).clamp(1, 200) as usize;
    let out = QuickstartList::run(ctx(), ListIn { status }, &Quiet).map_err(err)?;
    let shown: Vec<Value> = out
        .entries
        .iter()
        .rev()
        .take(limit)
        .map(op::to_value)
        .collect();
    Ok(ToolOutput::json(&json!({
        "catalog": out.catalog,
        "entries": out.entries.len(),
        "shown": shown.len(),
        "newest_first": shown,
    })))
}

/// `4 billion`, `1.5 billion`.
fn billions(n: u64) -> String {
    let b = n as f64 / 1e9;
    if b.fract() == 0.0 {
        format!("{b:.0} billion")
    } else {
        format!("{b} billion")
    }
}
