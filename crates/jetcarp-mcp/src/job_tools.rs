//! Background jobs over MCP: `ants_start`, `pile_start`, and `evolve_start` start an ant search,
//! a large sandpile, or an evolutionary glider search, and `job_status` / `job_stop` watch and
//! stop any catalog job (hunts, ant searches, piles, evolve searches, mover verifications, and the
//! finished soup searches, collisions, and census jobs), wherever it was started. One status and one stop tool for every kind keeps the list short for
//! small models; `kind` filters the listing.

use crate::tools::{Res, ToolOutput, check_unknown, int, string, tool};
use jetcarp_core::ants::job::{self as ants, AntHandle, AntOptions, AntSpec, Soups};
use jetcarp_core::ants::search::{Strings, Tables};
use jetcarp_core::library::{self, JobStatus, Library};
use jetcarp_core::pile::job::{self as pilejob, PileJobSpec, PileProgress};
use jetcarp_core::window::job as moverjob;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Instant;

pub const NAMES: &[&str] = &[
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

fn lib() -> Library {
    Library::open_default()
}

fn ants_running() -> &'static Mutex<HashMap<i64, AntHandle>> {
    static R: OnceLock<Mutex<HashMap<i64, AntHandle>>> = OnceLock::new();
    R.get_or_init(Default::default)
}

struct PileRun {
    stop: Arc<AtomicBool>,
    live: Arc<Mutex<PileProgress>>,
    thread: std::thread::JoinHandle<()>,
}

fn piles_running() -> &'static Mutex<HashMap<i64, PileRun>> {
    static R: OnceLock<Mutex<HashMap<i64, PileRun>>> = OnceLock::new();
    R.get_or_init(Default::default)
}

/// Evolve jobs running here: their stop flags and live progress.
type EvolveRun = (Arc<AtomicBool>, Arc<Mutex<Value>>);

fn evolve_running() -> &'static Mutex<HashMap<i64, EvolveRun>> {
    static R: OnceLock<Mutex<HashMap<i64, EvolveRun>>> = OnceLock::new();
    R.get_or_init(Default::default)
}

fn movers_running() -> &'static Mutex<HashMap<i64, moverjob::MoverHandle>> {
    static R: OnceLock<Mutex<HashMap<i64, moverjob::MoverHandle>>> = OnceLock::new();
    R.get_or_init(Default::default)
}

pub fn definitions() -> Vec<Value> {
    vec![
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
                "seed": {"type": "integer", "minimum": 0, "description": "Default 1."},
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
            "Starts a large sandpile in the background and returns its job id at once (poll job_status, stop with job_stop): N grains dropped on one cell of an unbounded grid, built by doubling (N's binary digits), every finished step saved so a stopped pile resumes ({\"resume\": job}), then a cropped pile.png, a preview.png, growth.csv, and pile.json in its folder under <Pictures>/JETCARP/piles (files_list shows them). A million grains takes seconds; 10^8 hours. Example: {\"tiling\": \"hex\", \"grains\": \"1e6\"}.",
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
            "Starts Edmond's evolutionary glider search in the background and returns its job id at once (poll job_status, stop with job_stop): a population of multi-state priority rules evolves on a tiling (default the aperiodic Spectre) towards rules whose seeds travel far; the best are verified (exactly on lattices, to growing distances on aperiodic patches) and the movers stored in the catalog (library_list {\"class\": \"mover\"}). Deterministic for a seed; a stopped search starts again from generation 0 (resume repeats it). The published settings (population 256, 800 generations, radius 48) take a long time; small ones (pop 32, gens 40, radius 16) take a minute. Example: {\"tiling\": \"spectre\", \"pop\": 32, \"gens\": 40, \"radius\": 16}.",
            json!({"type": "object", "properties": {
                "tiling": {"type": "string", "description": "Tiling id (default \"spectre\")."},
                "nb": {"type": "string", "enum": ["touch", "edge"], "description": "Default touch."},
                "states": {"type": "integer", "minimum": 2, "maximum": 8, "description": "Default 4."},
                "pop": {"type": "integer", "minimum": 4, "maximum": 4096, "description": "Population (default 256)."},
                "gens": {"type": "integer", "minimum": 1, "maximum": 100000, "description": "Generations (default 800)."},
                "radius": {"type": "integer", "minimum": 4, "maximum": 256, "description": "Arena radius (default 48)."},
                "horizon": {"type": "integer", "minimum": 10, "description": "Steps per evaluation (default 2500)."},
                "seed": {"type": "integer", "minimum": 0, "description": "Default 1."},
                "stop_on_glider": {"type": "boolean", "description": "Stop at the first verified glider."},
                "threads": {"type": "integer", "minimum": 0, "description": "Worker threads (default all)."},
                "resume": {"type": "integer", "minimum": 1, "description": "Run this evolve job's spec again."},
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
                "kind": {"type": "string", "enum": ["hunt", "ants", "pile", "evolve", "mover", "search", "collide", "census"], "description": "List only this kind."},
                "limit": {"type": "integer", "minimum": 1, "maximum": 200, "description": "Most jobs listed (default 30)."},
            }, "additionalProperties": false}),
        ),
        tool(
            "job_stop",
            "Stop a job",
            false,
            "Stops a running hunt, ant search, pile, evolve search, or mover verification at its next checkpoint (a few seconds; a pile cancels the step in flight). It stays resumable: hunt_start, ants_start, pile_start, or mover_start with {\"resume\": job}. Works for jobs started here, by the CLI, by a script, or by the app. Example: {\"job\": 3}.",
            json!({"type": "object", "properties": {
                "job": {"type": "integer", "minimum": 1, "description": "The job id."},
            }, "required": ["job"], "additionalProperties": false}),
        ),
    ]
}

pub fn call(name: &str, args: &Value) -> Res {
    match name {
        "ants_start" => ants_start(args),
        "pile_start" => pile_start(args),
        "evolve_start" => evolve_start(args),
        "mover_start" => mover_start(args),
        "job_status" => status(args),
        "job_stop" => stop(args),
        _ => Err(format!("unknown tool {name}")),
    }
}

fn list(args: &Value, key: &str) -> Result<Vec<String>, String> {
    Ok(match args.get(key) {
        None | Some(Value::Null) => vec![],
        Some(Value::String(s)) => s
            .split([',', ';', ' '])
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

fn ant_summary(p: &ants::AntProgress) -> Value {
    let mut v = serde_json::to_value(p).unwrap_or_default();
    v["line"] = json!(p.line());
    v
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
    let l = lib();
    let jid = match int(args, "resume")? {
        Some(j) => {
            if ants_running()
                .lock()
                .unwrap()
                .get(&j)
                .is_some_and(|h| !h.is_finished())
            {
                return Err(format!("ant search {j} is already running here"));
            }
            ants::status(&l, j)?;
            j
        }
        None => {
            let alphabet = string(args, "alphabet")?.map(String::from);
            let strings = pair(args, "strings")?.map(|(a, b)| Strings {
                min: a as usize,
                max: b as usize,
                alphabet: alphabet.clone(),
            });
            let tables = pair(args, "tables")?.map(|(n, k)| Tables {
                states: n as u8,
                colors: k as u8,
                alphabet: alphabet.clone(),
                all_max: 1_000_000,
                sample: 10_000,
                seed: int(args, "seed").ok().flatten().unwrap_or(1).max(0) as u64,
            });
            let rules = list(args, "rules")?;
            let d = AntSpec::default();
            let (strings, tables) = if strings.is_none() && tables.is_none() && rules.is_empty() {
                (d.strings.clone(), None)
            } else {
                (strings, tables)
            };
            let t = list(args, "tilings")?.join(",");
            let soup = match int(args, "soups")? {
                None | Some(0) => None,
                Some(n) => {
                    let s = int(args, "size")?.unwrap_or(8).clamp(1, 64) as u32;
                    Some(Soups {
                        w: s,
                        h: s,
                        density: args.get("density").and_then(|v| v.as_f64()).unwrap_or(0.5),
                        soups: n.max(1) as u64,
                        seed: int(args, "seed")?.unwrap_or(1).max(0) as u64,
                    })
                }
            };
            let spec = AntSpec {
                tilings: library::tiling_list(if t.trim().is_empty() { "square" } else { &t })?,
                rules,
                strings,
                tables,
                soup,
                steps: int(args, "steps")?
                    .map(|x| x.max(1) as u64)
                    .unwrap_or(d.steps),
                max_period: int(args, "max_period")?
                    .map(|x| x.max(1) as u64)
                    .unwrap_or(d.max_period),
                symmetry: true,
            };
            ants::start(&l, &spec)?
        }
    };
    let h = AntHandle::spawn(
        l.clone(),
        jid,
        AntOptions {
            threads: int(args, "threads")?.unwrap_or(0).max(0) as usize,
            ..Default::default()
        },
    );
    let p = h.progress();
    ants_running().lock().unwrap().insert(jid, h);
    Ok(ToolOutput::json(&json!({
        "job": jid,
        "status": "running",
        "catalog": l.path().display().to_string(),
        "progress": ant_summary(&p),
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
    let l = lib();
    let jid = match int(args, "resume")? {
        Some(j) => {
            if piles_running()
                .lock()
                .unwrap()
                .get(&j)
                .is_some_and(|r| !r.thread.is_finished())
            {
                return Err(format!("pile {j} is already running here"));
            }
            pilejob::status(&l, j)?;
            j
        }
        None => {
            let tid = string(args, "tiling")?.unwrap_or("square");
            let mut tiling = jetcarp_api::args::tiling_by_id(tid)?;
            if let Some(lv) = int(args, "level")? {
                tiling.size = Some(jetcarp_core::SizeSpec::Level(lv.clamp(1, 8) as u32));
            }
            let grains = match args.get("grains") {
                Some(Value::Number(n)) => n.as_u64().ok_or("`grains` must be a whole number")?,
                Some(Value::String(s)) => jetcarp_core::pile::parse_grains(s)?,
                _ => return Err("give `grains`, e.g. 1000000 or \"1e7\"".into()),
            };
            let pile = jetcarp_core::pile::PileSpec {
                tiling,
                nb: jetcarp_core::pile::parse_nb(string(args, "nb")?.unwrap_or("edge"))?,
                grains,
                centre: jetcarp_core::pile::Centre::parse(string(args, "centre")?.unwrap_or("cell"))?,
            };
            let root = jetcarp_api::out::out_root(None).join("piles");
            std::fs::create_dir_all(&root).map_err(|e| format!("{}: {e}", root.display()))?;
            let folder = jetcarp_api::out::make_out_dir(&root, &format!("{tid}-{grains}"))?;
            let spec = PileJobSpec {
                pile,
                folder,
                px_per_cell: args.get("px").and_then(|v| v.as_f64()).unwrap_or(2.0),
                margin: 4.0,
                palette: {
                    let p = string(args, "palette")?.unwrap_or("Ember");
                    jetcarp_core::render::builtin_palette(p)?;
                    p.to_string()
                },
                reverse: args
                    .get("reverse")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false),
                preview_side: 1024,
                png: true,
                gpu: args.get("gpu").and_then(|v| v.as_bool()).unwrap_or(false),
                extra: serde_json::Value::Null,
            };
            pilejob::start(&l, &spec)?
        }
    };
    let (spec, p0) = pilejob::status(&l, jid)?;
    let stop = Arc::new(AtomicBool::new(false));
    let live = Arc::new(Mutex::new(p0));
    let thread = {
        let (l, stop, live) = (l.clone(), stop.clone(), live.clone());
        let gpu = spec.gpu && !jetcarp_core::pile::is_patch(&spec.pile.tiling);
        std::thread::Builder::new()
            .name(format!("pile-{jid}"))
            .spawn(move || {
                let toppler = if gpu {
                    jetcarp_core::pile::gpu_toppler().and_then(|r| r.ok())
                } else {
                    None
                };
                if let Err(e) = pilejob::run_job(&l, jid, stop, toppler, Some(live.clone())) {
                    let mut x = live.lock().unwrap();
                    x.status = "failed".into();
                    x.error = Some(e);
                }
            })
            .map_err(|e| e.to_string())?
    };
    piles_running().lock().unwrap().insert(
        jid,
        PileRun {
            stop,
            live: live.clone(),
            thread,
        },
    );
    let p = live.lock().unwrap().clone();
    Ok(ToolOutput::json(&json!({
        "job": jid,
        "status": "running",
        "folder": spec.folder.display().to_string(),
        "progress": p,
        "line": p.line(),
        "note": "The pile builds in the background: poll job_status {\"job\": ...}; job_stop stops it (resumable with pile_start {\"resume\": job}).",
    })))
}

fn evolve_start(args: &Value) -> Res {
    use jetcarp_core::evolve::{EvolveSpec, evolve};
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
    let l = lib();
    let (jid, spec) = match int(args, "resume")? {
        Some(j) => {
            let row = l.job(j)?;
            if row.kind != "evolve" {
                return Err(format!(
                    "job {j} is a {} job, not an evolve search",
                    row.kind
                ));
            }
            if evolve_running().lock().unwrap().contains_key(&j) {
                return Err(format!("evolve job {j} is already running here"));
            }
            let spec: EvolveSpec =
                serde_json::from_value(row.spec.clone()).map_err(|e| format!("job {j}: {e}"))?;
            (j, spec)
        }
        None => {
            let d = EvolveSpec::default();
            let spec = EvolveSpec {
                tiling: library::tiling_from_key(string(args, "tiling")?.unwrap_or("spectre"))?,
                nb: match string(args, "nb")? {
                    Some("edge") => jetcarp_core::Nb::Edge,
                    _ => jetcarp_core::Nb::Touch,
                },
                states: int(args, "states")?.map(|x| x as u8).unwrap_or(d.states),
                pop: int(args, "pop")?
                    .map(|x| x.max(4) as usize)
                    .unwrap_or(d.pop),
                gens: int(args, "gens")?
                    .map(|x| x.max(1) as usize)
                    .unwrap_or(d.gens),
                seed: int(args, "seed")?
                    .map(|x| x.max(0) as u64)
                    .unwrap_or(d.seed),
                radius: int(args, "radius")?
                    .map(|x| x.max(4) as u32)
                    .unwrap_or(d.radius),
                horizon: int(args, "horizon")?
                    .map(|x| x.max(10) as u64)
                    .unwrap_or(d.horizon),
                stop_on_glider: args
                    .get("stop_on_glider")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false),
                threads: int(args, "threads")?.filter(|&t| t > 0).map(|t| t as usize),
                ..d
            };
            let v = serde_json::to_value(&spec).map_err(|e| e.to_string())?;
            let j = l.create_job("evolve", &v)?;
            (j, spec)
        }
    };
    l.update_job(jid, Some(JobStatus::Running), None, None)?;
    let stop = Arc::new(AtomicBool::new(false));
    let live = Arc::new(Mutex::new(json!({"status": "running", "generation": 0})));
    evolve_running()
        .lock()
        .unwrap()
        .insert(jid, (stop.clone(), live.clone()));
    let total = spec.gens;
    std::thread::Builder::new()
        .name(format!("evolve-{jid}"))
        .spawn(move || {
            let l = lib();
            let last = Mutex::new(Instant::now());
            let r = evolve(&spec, &|g| {
                let v = json!({
                    "status": "running", "generation": g.generation, "generations": total,
                    "best": g.best, "best_ever": g.best_ever, "median": g.median,
                    "at_ceiling": g.at_ceiling, "distinct": g.distinct, "elapsed_s": g.elapsed_s,
                    "line": format!("evolve {jid}: generation {}/{total}, best {:.2}, best ever {:.2}, {:.0} s",
                        g.generation, g.best, g.best_ever, g.elapsed_s),
                });
                *live.lock().unwrap() = v.clone();
                let mut t = last.lock().unwrap();
                if t.elapsed().as_secs_f64() >= 2.0 {
                    *t = Instant::now();
                    let _ = l.update_job(jid, None, Some(&v), None);
                    if l.job(jid).map(|j| j.status) == Ok(JobStatus::Paused) {
                        stop.store(true, Ordering::SeqCst);
                    }
                }
                !stop.load(Ordering::SeqCst)
            });
            let (status, v) = match r {
                Ok(rep) => {
                    let found = jetcarp_core::evolve::catalog::catalog_gliders(&rep, Some(&l), 20_000);
                    let dir = jetcarp_api::out::make_out_dir(
                        &jetcarp_api::out::out_root(None),
                        &format!("evolve-{}", rep.spec.tiling.family),
                    );
                    if let Ok(d) = &dir {
                        let _ = std::fs::write(
                            d.join("report.json"),
                            serde_json::to_string_pretty(&rep).unwrap_or_default(),
                        );
                    }
                    let stopped = stop.load(Ordering::SeqCst) && rep.generations.len() < total;
                    (
                        if stopped { JobStatus::Paused } else { JobStatus::Done },
                        json!({
                            "status": if stopped { "paused" } else { "done" },
                            "generation": rep.generations.len(), "generations": total,
                            "best_ever": rep.generations.last().map(|g| g.best_ever),
                            "champions": rep.champions.iter().take(8).map(|c| json!({
                                "rule": c.rule, "fitness": c.fitness,
                            })).collect::<Vec<_>>(),
                            "gliders": rep.gliders.len(),
                            "movers": found.as_ref().map(|f| f.iter().map(|m| json!({
                                "kind": m.kind, "ids": m.ids, "added": m.added, "summary": m.summary,
                            })).collect::<Vec<_>>()).unwrap_or_default(),
                            "folder": dir.map(|d| d.display().to_string()).unwrap_or_default(),
                            "line": format!("evolve {jid}: {} generations, {} verified glider(s)",
                                rep.generations.len(), rep.gliders.len()),
                        }),
                    )
                }
                Err(e) => (
                    JobStatus::Failed,
                    json!({"status": "failed", "error": e, "line": format!("evolve {jid} failed")}),
                ),
            };
            let _ = l.update_job(jid, Some(status), Some(&v), None);
            *live.lock().unwrap() = v;
            evolve_running().lock().unwrap().remove(&jid);
        })
        .map_err(|e| e.to_string())?;
    Ok(ToolOutput::json(&json!({
        "job": jid,
        "status": "running",
        "note": "The search runs in the background: poll job_status {\"job\": ...}; job_stop stops it. Verified movers go to the catalog.",
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
    let l = lib();
    let jid = match int(args, "resume")? {
        Some(j) => {
            if movers_running()
                .lock()
                .unwrap()
                .get(&j)
                .is_some_and(|h| !h.is_finished())
            {
                return Err(format!("mover job {j} is already running here"));
            }
            moverjob::status(&l, j)?;
            j
        }
        None => {
            let family = string(args, "tiling")?.unwrap_or("spectre");
            let level = int(args, "level")?.unwrap_or(4).clamp(1, 8) as u32;
            let patch = jetcarp_core::topology::TilingSpec::new(family)
                .with_size(jetcarp_core::SizeSpec::Level(level));
            let rule = string(args, "rule")?.unwrap_or("SpectreGlider").to_string();
            let nb = match string(args, "nb")? {
                Some("edge") => jetcarp_core::Nb::Edge,
                _ => jetcarp_core::Nb::Touch,
            };
            let d = jetcarp_core::window::track::WindowOpts::default();
            let rings = int(args, "rings")?.unwrap_or(10_000).max(1) as u32;
            let opts = jetcarp_core::window::track::WindowOpts {
                margin: args
                    .get("margin")
                    .and_then(|v| v.as_f64())
                    .unwrap_or(d.margin),
                fixed_levels: d.fixed_levels,
                target_rings: Some(rings),
            };
            let num = |v: &Value| v.as_u64().ok_or("cells are [[tile, state], ...]");
            let cells: Vec<(u32, u8)> = match args.get("cells") {
                Some(Value::Array(a)) => a
                    .iter()
                    .map(|c| {
                        let c = c
                            .as_array()
                            .filter(|c| c.len() == 2)
                            .ok_or("cells are [[tile, state], ...]")?;
                        Ok((num(&c[0])? as u32, num(&c[1])? as u8))
                    })
                    .collect::<Result<_, &str>>()?,
                _ => {
                    let states: Vec<u8> = match args.get("states") {
                        Some(Value::Array(a)) => a
                            .iter()
                            .map(|x| x.as_u64().map(|x| x as u8).ok_or("states are numbers"))
                            .collect::<Result<_, _>>()?,
                        _ => vec![1, 3],
                    };
                    let at = int(args, "at")?.map(|x| x.max(0) as u32);
                    moverjob::travelling_start(&rule, nb, &patch, &states, at, &opts)?
                }
            };
            let mut track = jetcarp_core::library::movers::TrackSpec::new(&rule, nb, patch, cells);
            track.max_gens = int(args, "gens")?
                .map(|x| x.max(1) as u64)
                .unwrap_or(rings as u64 * 100 + 20_000);
            if let Some(p) = int(args, "max_pop")? {
                track.max_pop = p.max(1) as usize;
            }
            let spec = moverjob::MoverJobSpec {
                track,
                window: opts,
                store: args.get("store").and_then(|v| v.as_bool()).unwrap_or(true),
                source: "MCP mover_start".into(),
            };
            moverjob::start(&l, &spec)?
        }
    };
    let h = moverjob::MoverHandle::spawn(l.clone(), jid);
    let p = h.progress();
    movers_running().lock().unwrap().insert(jid, h);
    let (spec, _) = moverjob::status(&l, jid)?;
    Ok(ToolOutput::json(&json!({
        "job": jid,
        "status": "running",
        "start": spec.track.cells,
        "progress": p,
        "line": p.line(),
        "note": "The run continues in the background: poll job_status {\"job\": ...}; job_stop stops it (resumable with mover_start {\"resume\": job}). A verified mover goes to the catalog.",
    })))
}

/// One job's progress, live when it runs in this process.
fn job_json(l: &Library, jid: i64) -> Result<Value, String> {
    let j = l.job(jid)?;
    let v = match j.kind.as_str() {
        "hunt" => crate::hunt_tools::status_json(l, jid)?,
        ants::KIND => {
            let here = ants_running()
                .lock()
                .unwrap()
                .get(&jid)
                .filter(|h| !h.is_finished())
                .map(|h| h.progress());
            let p = match here {
                Some(p) => p,
                None => ants::status(l, jid)?.1,
            };
            ant_summary(&p)
        }
        "evolve" => {
            let here = evolve_running()
                .lock()
                .unwrap()
                .get(&jid)
                .map(|r| r.1.lock().unwrap().clone());
            here.unwrap_or_else(|| j.progress.clone())
        }
        moverjob::KIND => {
            let here = movers_running()
                .lock()
                .unwrap()
                .get(&jid)
                .filter(|h| !h.is_finished())
                .map(|h| h.progress());
            let p = match here {
                Some(p) => p,
                None => moverjob::status(l, jid)?.1,
            };
            let mut v = serde_json::to_value(&p).unwrap_or_default();
            v["line"] = json!(p.line());
            v
        }
        pilejob::KIND => {
            let here = piles_running()
                .lock()
                .unwrap()
                .get(&jid)
                .filter(|r| !r.thread.is_finished())
                .map(|r| r.live.lock().unwrap().clone());
            let p = match here {
                Some(p) => p,
                None => pilejob::status(l, jid)?.1,
            };
            let mut v = serde_json::to_value(&p).unwrap_or_default();
            v["line"] = json!(p.line());
            v
        }
        _ => json!({
            "job": jid, "kind": j.kind, "spec": j.spec, "progress": j.progress,
        }),
    };
    let mut v = v;
    v["kind"] = json!(j.kind);
    v["job"] = json!(jid);
    v["status"] = v
        .get("status")
        .cloned()
        .unwrap_or(json!(format!("{:?}", j.status).to_lowercase()));
    Ok(v)
}

/// `job_status` with `wait_s`: poll the catalog once a second until the job is no longer running,
/// the wait runs out, or the client cancels the call.
pub fn status_wait(args: &Value, cancel: Option<&std::sync::atomic::AtomicBool>) -> Res {
    let wait = int(args, "wait_s")?.unwrap_or(0).clamp(0, 600) as u64;
    let Some(j) = int(args, "job")?.filter(|_| wait > 0) else {
        return status(args);
    };
    let t0 = std::time::Instant::now();
    loop {
        let v = job_json(&lib(), j)?;
        let running = v["status"].as_str() == Some("running");
        let cancelled = cancel.is_some_and(|c| c.load(Ordering::Relaxed));
        if !running || cancelled || t0.elapsed().as_secs() >= wait {
            let mut v = v;
            v["waited_s"] = json!(t0.elapsed().as_secs());
            return Ok(ToolOutput::json(&v));
        }
        std::thread::sleep(std::time::Duration::from_millis(1000));
    }
}

fn status(args: &Value) -> Res {
    check_unknown(args, &["job", "kind", "limit", "wait_s"])?;
    let l = lib();
    if let Some(j) = int(args, "job")? {
        return Ok(ToolOutput::json(&job_json(&l, j)?));
    }
    let kind = string(args, "kind")?;
    let limit = int(args, "limit")?.unwrap_or(30).clamp(1, 200) as usize;
    let mut out = Vec::new();
    for j in l.jobs()?.into_iter().rev().filter(|j| j.id != 0) {
        if kind.is_some_and(|k| k != j.kind) {
            continue;
        }
        if out.len() >= limit {
            break;
        }
        let v = job_json(&l, j.id).unwrap_or_else(|e| json!({"job": j.id, "error": e}));
        out.push(json!({
            "job": j.id, "kind": j.kind, "status": v["status"],
            "line": v.get("line").cloned().unwrap_or(Value::Null),
            "created": j.created, "updated": j.updated,
        }));
    }
    Ok(ToolOutput::json(&json!({
        "catalog": l.path().display().to_string(),
        "jobs": out,
    })))
}

fn stop(args: &Value) -> Res {
    check_unknown(args, &["job"])?;
    let jid = int(args, "job")?.ok_or("give `job`")?;
    let l = lib();
    let j = l.job(jid)?;
    let here = match j.kind.as_str() {
        "hunt" => crate::hunt_tools::stop_here(jid),
        ants::KIND => match ants_running().lock().unwrap().get(&jid) {
            Some(h) => {
                h.stop();
                true
            }
            None => false,
        },
        pilejob::KIND => match piles_running().lock().unwrap().get(&jid) {
            Some(r) => {
                r.stop.store(true, Ordering::SeqCst);
                true
            }
            None => false,
        },
        "evolve" => match evolve_running().lock().unwrap().get(&jid) {
            Some(r) => {
                r.0.store(true, Ordering::SeqCst);
                true
            }
            None => false,
        },
        moverjob::KIND => match movers_running().lock().unwrap().get(&jid) {
            Some(h) => {
                h.stop();
                true
            }
            None => false,
        },
        k => return Err(format!("job {jid} is a {k} job, which runs to its end")),
    };
    if !here && j.status == JobStatus::Running {
        // Another process: it sees `paused` at its next checkpoint.
        l.update_job(jid, Some(JobStatus::Paused), None, None)?;
    }
    let resume = match j.kind.as_str() {
        "hunt" => "hunt_start",
        ants::KIND => "ants_start",
        "evolve" => "evolve_start",
        moverjob::KIND => "mover_start",
        _ => "pile_start",
    };
    Ok(ToolOutput::json(&json!({
        "job": jid,
        "kind": j.kind,
        "stopping": true,
        "note": format!("It stops at its next checkpoint and stays resumable with {resume} {{\"resume\": {jid}}}."),
    })))
}
