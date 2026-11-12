//! Background jobs over MCP: `hunt_start`, `ants_start`, `pile_start`, `evolve_start`, and
//! `mover_start` start (or carry on) a hunt, an ant search, a large sandpile, an evolutionary
//! glider search, or a mover verification in the background, and `job_status` / `job_stop` watch
//! and stop any catalog job (those, and soup searches, collisions, and censuses), wherever it was
//! started. One status and one stop tool for every kind keeps the list short for small models;
//! `kind` filters the listing.
//!
//! Each tool reads its arguments as it always has and calls the operations `search.start`
//! (`background`: the job runs on a thread of this process, in the context's registry, with its
//! lease and checkpoints), `jobs.status`, `jobs.list`, and `jobs.stop`.

use crate::tools::{Res, ToolOutput, check_unknown, ctx, err, int, string, tool, uint};
use jetcarp_api::args;
use jetcarp_api::op::{self, Op, Progress, Quiet};
use jetcarp_api::ops::jobs::{
    JobIn, JobView, JobsList, JobsStatus, JobsStop, ListIn, StatusIn, StatusOut,
};
use jetcarp_api::ops::search::{
    AntsIn, EvolveIn, HuntIn, MoverIn, PileIn, SearchStart, SearchStartIn, SearchStarted,
};
use jetcarp_search::ants::AntProgress;
use jetcarp_search::hunt::HuntProgress;
use jetcarp_search::pile::PileProgress;
use jetcarp_search::window::MoverProgress;
use serde_json::{Value, json};
use std::sync::atomic::Ordering;

pub const NAMES: &[&str] = &[
    "hunt_start",
    "ants_start",
    "pile_start",
    "evolve_start",
    "mover_start",
    "job_status",
    "job_stop",
];

pub fn handles(name: &str) -> bool {
    NAMES.contains(&name)
}

pub fn definitions() -> Vec<Value> {
    vec![
        tool(
            "hunt_start",
            "Start a soup hunt",
            false,
            "Starts a fast soup hunt in the background and returns at once with its job id (poll job_status, stop with job_stop). Small random soups (default 16x16 unit cells at density 0.5) run on an unbounded plane, bit-parallel, until they settle; ships that fly away are cut out and counted; every object is classified, and new patterns are verified and stored in the catalog (library_list shows them; class gun for stationary guns). Thousands of soups per second for Life on squares. Results do not depend on threads. The hunt is a resumable catalog job: {\"resume\": 3} continues job 3. Rules: B/S and Generations rules without B0 (a V suffix means edge neighbours); tilings: any periodic tiling (family id, catalog id, or a set such as \"archimedean\"). Examples: {\"rules\": [\"B3/S23\"], \"soups\": 100000}, {\"rules\": [\"B2/S34\", \"B24/S35\"], \"tilings\": \"hex\", \"soups\": 0} (0 = until stopped).",
            json!({"type": "object", "properties": {
                    "rules": {"type": ["array", "string"], "items": {"type": "string"}, "description": "Rules (default [\"B3/S23\"])."},
                    "tilings": {"type": ["array", "string"], "items": {"type": "string"}, "description": "Tilings (default \"square\")."},
                    "nb": {"type": "string", "enum": ["touch", "edge"], "description": "Neighbourhood for every rule (default: the notation decides)."},
                    "soups": {"type": "integer", "minimum": 0, "description": "Soups per rule and tiling (default 10000; 0 = until stopped)."},
                    "size": {"type": "string", "description": "Soup size WxH in unit cells (default \"16x16\")."},
                    "density": {"type": "number", "minimum": 0, "maximum": 1, "description": "Default 0.5."},
                    "shape": {"type": "string", "enum": ["rect", "disc"], "description": "Soup shape (default rect)."},
                    "seed": {"type": ["integer", "string"], "minimum": 0, "description": "Default 1; soup i uses an RNG state from (seed, i). Any whole number to 2^64 - 1; beyond 2^53 as a string of digits (\"17705299566207515059\")."},
                    "gens": {"type": "integer", "minimum": 1, "description": "Generations before an unsettled soup is censused as it is (default 20000)."},
                    "max_period": {"type": "integer", "minimum": 1, "maximum": 1024, "description": "Longest period looked for (default 64)."},
                    "threads": {"type": "integer", "minimum": 0, "description": "Worker threads (default 0 = all cores)."},
                    "resume": {"type": "integer", "minimum": 1, "description": "Continue this hunt job instead of starting a new one."},
                }, "additionalProperties": false}),
        ),
        tool(
            "ants_start",
            "Start an ant search",
            false,
            "Starts an ant and turmite search in the background and returns its job id at once (poll job_status, stop with job_stop). Turn strings (Langton's ant is RL; L/R turn one edge, N none, U back, L2/R2 two edges on hexagons) and turmite tables (several states) run on an unbounded periodic tiling from an empty field or seeded soups; each run is classed as highway (proved exactly: period, displacement, onset), periodic, symmetric growth, bounded, or chaotic, and stored (library_list {\"kind\": \"ant\"}). Mirror images and repeats are skipped. Results do not depend on threads; {\"resume\": 7} continues job 7. Examples: {\"tilings\": \"square\", \"strings\": [2, 6]}, {\"tilings\": \"hex\", \"strings\": 4}, {\"rules\": [\"RL\", \"LLRR\"], \"soups\": 50}, {\"tilings\": \"square\", \"tables\": [2, 2], \"steps\": 200000}.",
            json!({"type": "object", "properties": {
                "tilings": {"type": ["array", "string"], "items": {"type": "string"}, "description": "Periodic tilings (default \"square\"): \"hex\", \"triangle\", \"4.8.8\", \"archimedean\", ..."},
                "strings": {"type": ["array", "integer"], "items": {"type": "integer"}, "description": "Turn string lengths: [min, max] or one length."},
                "tables": {"type": "array", "items": {"type": "integer"}, "description": "Turmite tables [states, colours], e.g. [2, 2]."},
                "alphabet": {"type": "string", "description": "Turn letters to use, e.g. \"LR\" (default: the tiling's)."},
                "rules": {"type": ["array", "string"], "items": {"type": "string"}, "description": "Explicit rules: \"RL\", \"LLRR\", \"RL2N\", \"1R1,1L0/1R1,0N0\"."},
                "soups": {"type": "integer", "minimum": 1, "description": "Seeded soups per rule (default: an empty field)."},
                "size": {"type": "integer", "minimum": 1, "maximum": 64, "description": "Soup side in unit cells (default 8)."},
                "density": {"type": "number", "minimum": 0, "maximum": 1, "description": "Soup density (default 0.5)."},
                "seed": {"type": ["integer", "string"], "minimum": 0, "description": "Default 1. Any whole number to 2^64 - 1; beyond 2^53 as a string of digits (\"17705299566207515059\")."},
                "steps": {"type": "integer", "minimum": 1, "description": "Steps before a run is classed as it is (default 1000000)."},
                "max_period": {"type": "integer", "minimum": 1, "maximum": 1000000, "description": "Longest highway period looked for (default 20000)."},
                "threads": {"type": "integer", "minimum": 0, "description": "Worker threads (default 0 = all cores)."},
                "resume": {"type": "integer", "minimum": 1, "description": "Continue this ant search instead of starting one."},
            }, "additionalProperties": false}),
        ),
        tool(
            "pile_start",
            "Start a large sandpile",
            false,
            "Starts a large sandpile in the background and returns its job id at once (poll job_status, stop with job_stop): N grains dropped on one cell of an unbounded grid, built by doubling (N's binary digits), every finished step saved, and a step stopped part way kept as a checkpoint, so a stopped pile resumes where it stopped ({\"resume\": job}), then a cropped pile.png, a preview.png, growth.csv, and pile.json in its folder under <Pictures>/JETCARP/piles (files_list shows them). A million grains takes seconds; 10^8 hours. Example: {\"tiling\": \"hex\", \"grains\": \"1e6\"}.",
            json!({"type": "object", "properties": {
                "tiling": {"type": "string", "description": "Tiling id (default \"square\"); aperiodic patches up to level 8."},
                "grains": {"type": ["integer", "string"], "description": "Grains: 1000000, \"1e7\", \"2^30\"."},
                "nb": {"type": "string", "enum": ["edge", "touch"], "description": "Neighbourhood (default edge)."},
                "centre": {"type": "string", "enum": ["cell", "vertex", "edge"], "description": "Where the grains go (default cell)."},
                "level": {"type": "integer", "minimum": 1, "maximum": 8, "description": "Aperiodic patch level (default: big enough)."},
                "px": {"type": "number", "minimum": 0.05, "maximum": 64, "description": "Pixels per cell in pile.png (default 2)."},
                "palette": {"type": "string", "description": "Palette for the heights, with its background and stage: Ember (default) or any built-in (Tide, Prism, Ink, Mono, Aurora, Synthwave, Moss, Candy, Lava, Glacier, Phosphor, Koi, Desert, Thermal, Deep Sea, Paper, Blueprint, Neon, Autumn)."},
                "reverse": {"type": "boolean", "description": "Run the palette the other way (height 1 at its last stop)."},
                "gpu": {"type": "boolean", "description": "Topple on the GPU when there is one (periodic tilings)."},
                "resume": {"type": "integer", "minimum": 1, "description": "Continue this pile job."},
            }, "additionalProperties": false}),
        ),
        tool(
            "evolve_start",
            "Start an evolutionary glider search",
            false,
            "Starts Edmond's evolutionary glider search in the background and returns its job id at once (poll job_status, stop with job_stop): a population of multi-state priority rules evolves on a tiling (default the aperiodic Spectre) towards rules whose seeds travel far; the best are verified (exactly on lattices, to growing distances on aperiodic patches) and the movers stored in the catalog (library_list {\"class\": \"mover\"}). Deterministic for a seed; checkpointed every generation, so a stopped search resumes from its last generation ({\"resume\": job}) and ends as an uninterrupted one. Its files (fitness.csv, champions.json, gliders.json, run.json, movers.json) go to its folder under <Pictures>/JETCARP. The published settings (population 256, 800 generations, radius 48) take a long time; small ones (pop 32, gens 40, radius 16) take a minute. Example: {\"tiling\": \"spectre\", \"pop\": 32, \"gens\": 40, \"radius\": 16}.",
            json!({"type": "object", "properties": {
                "tiling": {"type": "string", "description": "Tiling id (default \"spectre\")."},
                "nb": {"type": "string", "enum": ["touch", "edge"], "description": "Default touch."},
                "states": {"type": "integer", "minimum": 2, "maximum": 8, "description": "Default 4."},
                "pop": {"type": "integer", "minimum": 4, "maximum": 4096, "description": "Population (default 256)."},
                "gens": {"type": "integer", "minimum": 1, "maximum": 100000, "description": "Generations (default 800)."},
                "radius": {"type": "integer", "minimum": 4, "maximum": 256, "description": "Arena radius (default 48)."},
                "horizon": {"type": "integer", "minimum": 10, "description": "Steps per evaluation (default 2500)."},
                "seed": {"type": ["integer", "string"], "minimum": 0, "description": "Default 1. Any whole number to 2^64 - 1; beyond 2^53 as a string of digits (\"17705299566207515059\")."},
                "stop_on_glider": {"type": "boolean", "description": "Stop at the first verified glider."},
                "threads": {"type": "integer", "minimum": 0, "description": "Worker threads (default all)."},
                "resume": {"type": "integer", "minimum": 1, "description": "Continue this evolve job from its last generation."},
            }, "additionalProperties": false}),
        ),
        tool(
            "mover_start",
            "Verify a mover far on a sliding window",
            false,
            "Starts a long mover verification in the background and returns its job id at once (poll job_status, stop with job_stop, resume with {\"resume\": job}): a pattern on an aperiodic tiling (Spectre, hat, Penrose) runs on a sliding window that generates supertiles around it with exact coordinates, dropping tiles behind, so it can be followed far past the fixed level-8 patches (tens of thousands of edge rings a minute for the Spectre glider). The run is exact on one infinite tiling; it ends at `rings` (verified) or `gens`, or when the pattern dies, grows, or repeats. A verified mover goes to the catalog (library_list {\"class\": \"mover\"}) with its distance and a sliding-window flag. Defaults: Edmond's Spectre glider pair (rule SpectreGlider, states 1 and 3 near the deepest tile of level 4). Examples: {\"rings\": 100000}, {\"tiling\": \"hat\", \"rule\": \"Priority 4: 3|n2>=1->2; 0|n3>=2->0; 0|n2>=1->3; 0|n1>=1->2\", \"cells\": [[0, 1], [3, 2]], \"rings\": 20000}.",
            json!({"type": "object", "properties": {
                "tiling": {"type": "string", "enum": ["spectre", "hat", "penrose-kd", "penrose-rhomb"], "description": "Default spectre."},
                "rule": {"type": "string", "description": "Rule notation (default SpectreGlider)."},
                "level": {"type": "integer", "minimum": 1, "maximum": 8, "description": "Patch level the cells are given on (default 4)."},
                "cells": {"type": "array", "items": {"type": "array", "items": {"type": "integer"}}, "description": "Starting cells [[tile, state], ...] on that level."},
                "states": {"type": "array", "items": {"type": "integer"}, "description": "Instead of cells: states on a tile and an edge neighbour (default [1, 3]), tried near the deepest tile (or at `at`)."},
                "at": {"type": "integer", "minimum": 0, "description": "The tile for `states`."},
                "nb": {"type": "string", "enum": ["touch", "edge"], "description": "Default touch."},
                "rings": {"type": "integer", "minimum": 1, "description": "Stop, verified, at this many edge rings (default 10000)."},
                "gens": {"type": "integer", "minimum": 1, "description": "Generation limit (default 100 per ring plus 20000)."},
                "max_pop": {"type": "integer", "minimum": 1, "description": "Stop as grown above this many cells (default 2000)."},
                "margin": {"type": "number", "minimum": 8, "description": "World units of tiles kept around the live cells (default 24)."},
                "store": {"type": "boolean", "description": "Store a verified mover in the catalog (default true)."},
                "resume": {"type": "integer", "minimum": 1, "description": "Continue this mover job."},
            }, "additionalProperties": false}),
        ),
        tool(
            "job_status",
            "Job status",
            true,
            "Reports a catalog job's progress: hunts (soups, objects by class, newest finds), ant searches (runs, outcomes, newest highways), piles (doubling step, grains, topplings, the pictures when done), mover verifications (generations, rings, speed, the summary and stored record when done), and finished soup searches, collisions, and census jobs. With `wait_s`, waits up to that many seconds (at most 600) for a running job to finish before answering, so one call replaces many polls. Without `job`, lists every job (newest first), optionally of one `kind`. Example: {\"job\": 3, \"wait_s\": 300}, {\"kind\": \"ants\"}.",
            json!({"type": "object", "properties": {
                "job": {"type": "integer", "minimum": 1, "description": "The job id (default: list jobs)."},
                "wait_s": {"type": "integer", "minimum": 0, "maximum": 600, "description": "Wait up to this many seconds for the job to stop running (default 0: answer now)."},
                "kind": {"type": "string", "enum": ["hunt", "ants", "pile", "evolve", "mover", "search", "collide", "census", "explore"], "description": "List only this kind."},
                "limit": {"type": "integer", "minimum": 1, "maximum": 200, "description": "Most jobs listed (default 30)."},
            }, "additionalProperties": false}),
        ),
        tool(
            "job_stop",
            "Stop a job",
            false,
            "Stops a running job at its next checkpoint (a few seconds; a pile saves the step in flight as it stands, and carries it on when resumed): a hunt, ant search, pile, evolve search, mover verification, soup search, collision, census, or explore search. It stays resumable: hunt_start, ants_start, pile_start, evolve_start, mover_start, or explore_start with {\"resume\": job}. Works for jobs started here, by the CLI, by a script, or by the app. Example: {\"job\": 3}.",
            json!({"type": "object", "properties": {
                "job": {"type": "integer", "minimum": 1, "description": "The job id."},
            }, "required": ["job"], "additionalProperties": false}),
        ),
    ]
}

pub fn call(name: &str, args: &Value) -> Res {
    match name {
        "hunt_start" => hunt_start(args),
        "ants_start" => ants_start(args),
        "pile_start" => pile_start(args),
        "evolve_start" => evolve_start(args),
        "mover_start" => mover_start(args),
        "job_status" => status_wait(args, None),
        "job_stop" => stop(args),
        _ => Err(format!("unknown tool {name}")),
    }
}

/// Text items: a list, or text split at any of `seps`.
fn list(args: &Value, key: &str, seps: &[char]) -> Result<Vec<String>, String> {
    Ok(match args.get(key) {
        None | Some(Value::Null) => vec![],
        Some(Value::String(s)) => s
            .split(seps)
            .map(str::trim)
            .filter(|x| !x.is_empty())
            .map(String::from)
            .collect(),
        Some(Value::Array(a)) => a
            .iter()
            .map(|x| {
                x.as_str()
                    .map(String::from)
                    .ok_or(format!("`{key}` must be text"))
            })
            .collect::<Result<_, _>>()?,
        Some(v) => return Err(format!("`{key}` must be a list of text, got {v}")),
    })
}

fn pair(args: &Value, key: &str) -> Result<Option<(u64, u64)>, String> {
    Ok(match args.get(key) {
        None | Some(Value::Null) => None,
        Some(Value::Number(n)) => {
            let n = n
                .as_u64()
                .ok_or(format!("`{key}` must be a whole number"))?;
            Some((n, n))
        }
        Some(Value::Array(a)) if a.len() == 2 => Some((
            a[0].as_u64().ok_or(format!("`{key}` is [a, b]"))?,
            a[1].as_u64().ok_or(format!("`{key}` is [a, b]"))?,
        )),
        Some(v) => return Err(format!("`{key}` is [a, b] or a number, got {v}")),
    })
}

fn num(args: &Value, key: &str) -> Option<f64> {
    args.get(key).and_then(|v| v.as_f64())
}

/// Start (or carry on) a job of this strategy in the background, through `search.start`.
fn start_in_background(
    strategy: &str,
    spec: Value,
    resume: Option<i64>,
    threads: usize,
) -> Result<SearchStarted, String> {
    let a = SearchStartIn {
        strategy: Some(strategy.into()),
        spec,
        resume,
        threads: Some(threads),
        background: true,
        ..Default::default()
    };
    SearchStart::run(ctx(), a, &Quiet).map_err(err)
}

fn with_line(v: Value, line: String) -> Value {
    let mut v = v;
    if v.is_object() {
        v["line"] = json!(line);
    }
    v
}

fn hunt_start(args: &Value) -> Res {
    check_unknown(
        args,
        &[
            "rules",
            "tilings",
            "nb",
            "soups",
            "size",
            "density",
            "shape",
            "seed",
            "gens",
            "max_period",
            "threads",
            "resume",
        ],
    )?;
    let rules = list(args, "rules", &[',', ';'])?;
    let spec = HuntIn {
        rules: (!rules.is_empty()).then(|| json!(rules)),
        tilings: Some(json!(list(args, "tilings", &[',', ';'])?.join(","))),
        nb: string(args, "nb")?.map(String::from),
        soups: Some(int(args, "soups")?.unwrap_or(10_000).max(0) as u64),
        size: string(args, "size")?
            .map(|s| {
                args::size_pair(s)
                    .map(|(w, h)| json!([w, h]))
                    .map_err(|_| format!("bad size '{s}'"))
            })
            .transpose()?,
        density: num(args, "density"),
        shape: string(args, "shape")?.map(String::from),
        seed: uint(args, "seed")?,
        gens: int(args, "gens")?.map(|x| x.max(1) as u64),
        max_period: int(args, "max_period")?.map(|x| x.clamp(1, 1024) as u64),
        ..Default::default()
    };
    let threads = int(args, "threads")?.unwrap_or(0).max(0) as usize;
    let out = start_in_background("hunt", op::to_value(&spec), int(args, "resume")?, threads)?;
    let p: HuntProgress = serde_json::from_value(out.progress).unwrap_or_default();
    Ok(ToolOutput::json(&json!({
        "job": out.job,
        "status": "running",
        "catalog": out.catalog,
        "progress": with_line(serde_json::to_value(&p).unwrap_or_default(), p.line()),
        "note": "The hunt runs in the background: poll job_status {\"job\": ...}; job_stop stops it (resumable with hunt_start {\"resume\": job}).",
    })))
}

fn ants_start(args: &Value) -> Res {
    check_unknown(
        args,
        &[
            "tilings",
            "strings",
            "tables",
            "alphabet",
            "rules",
            "soups",
            "size",
            "density",
            "seed",
            "steps",
            "max_period",
            "threads",
            "resume",
        ],
    )?;
    let rules = list(args, "rules", &[',', ';', ' '])?;
    let spec = AntsIn {
        tilings: Some(json!(list(args, "tilings", &[',', ';', ' '])?.join(","))),
        strings: pair(args, "strings")?.map(|(a, b)| json!([a, b])),
        tables: pair(args, "tables")?.map(|(a, b)| json!([a, b])),
        alphabet: string(args, "alphabet")?.map(String::from),
        rules: (!rules.is_empty()).then(|| json!(rules)),
        soups: int(args, "soups")?.map(|n| n.max(0) as u64),
        size: Some(json!(int(args, "size")?.unwrap_or(8).clamp(1, 64))),
        density: num(args, "density"),
        seed: uint(args, "seed")?,
        steps: int(args, "steps")?.map(|x| x.max(1) as u64),
        max_period: int(args, "max_period")?.map(|x| x.max(1) as u64),
        ..Default::default()
    };
    let threads = int(args, "threads")?.unwrap_or(0).max(0) as usize;
    let out = start_in_background("ants", op::to_value(&spec), int(args, "resume")?, threads)?;
    let p: AntProgress = serde_json::from_value(out.progress).unwrap_or_default();
    Ok(ToolOutput::json(&json!({
        "job": out.job,
        "status": "running",
        "catalog": out.catalog,
        "progress": with_line(serde_json::to_value(&p).unwrap_or_default(), p.line()),
        "note": "The search runs in the background: poll job_status {\"job\": ...}; job_stop stops it (resumable with ants_start {\"resume\": job}). Results: library_list {\"kind\": \"ant\"}.",
    })))
}

fn pile_start(args: &Value) -> Res {
    check_unknown(
        args,
        &[
            "tiling", "grains", "nb", "centre", "level", "px", "palette", "reverse", "gpu",
            "resume",
        ],
    )?;
    let resume = int(args, "resume")?;
    let spec = if resume.is_some() {
        Value::Null
    } else {
        let grains = match args.get("grains") {
            Some(g @ (Value::Number(_) | Value::String(_))) => g.clone(),
            _ => return Err("give `grains`, e.g. 1000000 or \"1e7\"".into()),
        };
        op::to_value(&PileIn {
            tiling: Some(string(args, "tiling")?.unwrap_or("square").to_string()),
            grains: Some(grains),
            nb: string(args, "nb")?.map(String::from),
            centre: string(args, "centre")?.map(String::from),
            level: int(args, "level")?.map(|l| l.clamp(1, 8) as u32),
            px: num(args, "px"),
            palette: string(args, "palette")?.map(String::from),
            reverse: args.get("reverse").and_then(|v| v.as_bool()),
            gpu: args.get("gpu").and_then(|v| v.as_bool()),
            ..Default::default()
        })
    };
    // Topples on the GPU when the job asks for it and the host registered a GPU toppler.
    let out = start_in_background("pile", spec, resume, 0)?;
    let p: PileProgress = serde_json::from_value(out.progress).unwrap_or_default();
    Ok(ToolOutput::json(&json!({
        "job": out.job,
        "status": "running",
        "folder": p.folder,
        "line": p.line(),
        "progress": p,
        "note": "The pile builds in the background: poll job_status {\"job\": ...}; job_stop stops it (resumable with pile_start {\"resume\": job}).",
    })))
}

fn evolve_start(args: &Value) -> Res {
    check_unknown(
        args,
        &[
            "tiling",
            "nb",
            "states",
            "pop",
            "gens",
            "radius",
            "horizon",
            "seed",
            "stop_on_glider",
            "threads",
            "resume",
        ],
    )?;
    let spec = EvolveIn {
        tiling: Some(string(args, "tiling")?.unwrap_or("spectre").to_string()),
        nb: Some(
            match string(args, "nb")? {
                Some("edge") => "edge",
                _ => "touch",
            }
            .into(),
        ),
        states: int(args, "states")?.map(|x| x as u8),
        pop: int(args, "pop")?.map(|x| x.max(4) as usize),
        gens: int(args, "gens")?.map(|x| x.max(1) as usize),
        seed: uint(args, "seed")?,
        radius: int(args, "radius")?.map(|x| x.max(4) as u32),
        horizon: int(args, "horizon")?.map(|x| x.max(10) as u64),
        stop_on_glider: args
            .get("stop_on_glider")
            .and_then(|v| v.as_bool())
            .unwrap_or(false),
        threads: int(args, "threads")?.filter(|&t| t > 0).map(|t| t as usize),
        ..Default::default()
    };
    let threads = int(args, "threads")?.unwrap_or(0).max(0) as usize;
    let out = start_in_background("evolve", op::to_value(&spec), int(args, "resume")?, threads)?;
    Ok(ToolOutput::json(&json!({
        "job": out.job,
        "status": "running",
        "note": "The search runs in the background: poll job_status {\"job\": ...}; job_stop stops it (resumable with evolve_start {\"resume\": job}). Verified movers go to the catalog.",
    })))
}

fn mover_start(args: &Value) -> Res {
    check_unknown(
        args,
        &[
            "tiling", "rule", "level", "cells", "states", "at", "nb", "rings", "gens", "max_pop",
            "margin", "store", "resume",
        ],
    )?;
    let spec = MoverIn {
        tiling: string(args, "tiling")?.map(String::from),
        rule: string(args, "rule")?.map(String::from),
        level: Some(int(args, "level")?.unwrap_or(4).clamp(1, 8) as u32),
        nb: Some(
            match string(args, "nb")? {
                Some("edge") => "edge",
                _ => "touch",
            }
            .into(),
        ),
        cells: args.get("cells").filter(|c| c.is_array()).cloned(),
        states: args.get("states").filter(|c| c.is_array()).cloned(),
        at: int(args, "at")?.map(|x| x.max(0) as u32),
        rings: Some(int(args, "rings")?.unwrap_or(10_000).max(1) as u32),
        gens: int(args, "gens")?.map(|x| x.max(1) as u64),
        max_pop: int(args, "max_pop")?.map(|p| p.max(1) as usize),
        margin: num(args, "margin"),
        store: Some(args.get("store").and_then(|v| v.as_bool()).unwrap_or(true)),
        source: Some("MCP mover_start".into()),
        ..Default::default()
    };
    let out = start_in_background("mover", op::to_value(&spec), int(args, "resume")?, 0)?;
    let p: MoverProgress = serde_json::from_value(out.progress).unwrap_or_default();
    let start = ctx()
        .library()
        .job(out.job)
        .ok()
        .and_then(|j| j.spec.get("track").and_then(|t| t.get("cells")).cloned())
        .unwrap_or(Value::Null);
    Ok(ToolOutput::json(&json!({
        "job": out.job,
        "status": "running",
        "start": start,
        "line": p.line(),
        "progress": p,
        "note": "The run continues in the background: poll job_status {\"job\": ...}; job_stop stops it (resumable with mover_start {\"resume\": job}). A verified mover goes to the catalog.",
    })))
}

/// One job's progress as `job_status` has always shown it: the kind's progress (live when it
/// runs here) with its line, or for kinds without a start tool the job's spec and progress.
fn job_json(v: &JobView) -> Value {
    let mut out = match v.kind.as_str() {
        "hunt" => {
            let p: HuntProgress = serde_json::from_value(v.progress.clone()).unwrap_or_default();
            with_line(serde_json::to_value(&p).unwrap_or_default(), p.line())
        }
        "ants" => {
            let p: AntProgress = serde_json::from_value(v.progress.clone()).unwrap_or_default();
            with_line(serde_json::to_value(&p).unwrap_or_default(), p.line())
        }
        "mover" => {
            let p: MoverProgress = serde_json::from_value(v.progress.clone()).unwrap_or_default();
            with_line(serde_json::to_value(&p).unwrap_or_default(), p.line())
        }
        "pile" => {
            let p: PileProgress = serde_json::from_value(v.progress.clone()).unwrap_or_default();
            with_line(serde_json::to_value(&p).unwrap_or_default(), p.line())
        }
        "evolve" => v.progress.clone(),
        _ => json!({"job": v.job, "kind": v.kind, "spec": v.spec, "progress": v.progress}),
    };
    if !out.is_object() {
        out = json!({});
    }
    out["kind"] = json!(v.kind);
    out["job"] = json!(v.job);
    out["status"] = json!(v.status);
    out
}

/// `job_status`, with `wait_s` waiting (polling the catalog) until the job is no longer running,
/// the wait runs out, or the client cancels the call.
pub fn status_wait(args: &Value, cancel: Option<&std::sync::atomic::AtomicBool>) -> Res {
    check_unknown(args, &["job", "kind", "limit", "wait_s"])?;
    let wait = int(args, "wait_s")?.unwrap_or(0).clamp(0, 600) as u64;
    struct Cancel<'a>(Option<&'a std::sync::atomic::AtomicBool>);
    impl Progress for Cancel<'_> {
        fn cancelled(&self) -> bool {
            self.0.is_some_and(|c| c.load(Ordering::Relaxed))
        }
    }
    if let Some(job) = int(args, "job")? {
        let a = StatusIn {
            job: Some(job),
            wait_s: (wait > 0).then_some(wait as f64),
            ..Default::default()
        };
        let StatusOut::One(v) = JobsStatus::run(ctx(), a, &Cancel(cancel)).map_err(err)? else {
            return Err(format!("no job {job}"));
        };
        let mut out = job_json(&v);
        if let Some(w) = v.waited_s {
            out["waited_s"] = json!(w as u64);
        }
        return Ok(ToolOutput::json(&out));
    }
    let a = ListIn {
        kind: string(args, "kind")?.map(String::from),
        limit: Some(int(args, "limit")?.unwrap_or(30).clamp(1, 200) as usize),
        ..Default::default()
    };
    let l = JobsList::run(ctx(), a, &Quiet).map_err(err)?;
    let jobs: Vec<Value> = l
        .jobs
        .iter()
        .map(|j| {
            json!({
                "job": j.job, "kind": j.kind, "status": j.status, "line": j.line,
                "created": j.created, "updated": j.updated,
            })
        })
        .collect();
    Ok(ToolOutput::json(&json!({
        "catalog": l.catalog,
        "jobs": jobs,
    })))
}

fn stop(args: &Value) -> Res {
    check_unknown(args, &["job"])?;
    let job = int(args, "job")?.ok_or("give `job`")?;
    let s = JobsStop::run(ctx(), JobIn { job }, &Quiet).map_err(err)?;
    let resume = match s.kind.as_str() {
        "hunt" => format!("hunt_start {{\"resume\": {job}}}"),
        "ants" => format!("ants_start {{\"resume\": {job}}}"),
        "evolve" => format!("evolve_start {{\"resume\": {job}}}"),
        "explore" => format!("explore_start {{\"resume\": {job}}} (it continues from its archive)"),
        "mover" => format!("mover_start {{\"resume\": {job}}}"),
        "pile" => format!("pile_start {{\"resume\": {job}}}"),
        _ => format!("`jetcarp jobs resume {job}`"),
    };
    Ok(ToolOutput::json(&json!({
        "job": job,
        "kind": s.kind,
        "stopping": s.stopping,
        "note": format!("It stops at its next checkpoint and stays resumable with {resume}."),
    })))
}
