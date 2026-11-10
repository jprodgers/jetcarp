//! Parallel rule sweeps: one world per rule, run until a trigger, one PNG each, a manifest.
//!
//! Every run builds its own world on one shared tiling with the same RNG state, so results do
//! not depend on the thread count or the order runs finish in; the manifest is sorted by index.
//! Rows are appended to `manifest.partial.jsonl` as runs finish, so an interrupted sweep resumes
//! (pass the same `folder`) without redoing finished runs.

use crate::describe::describe;
use crate::series::{self, Item};
use crate::sim::{self, ImageSpec, Shared};
use crate::{Ctx, Progress};
use jetcarp_api::args::{self, Screen, Until, get, get_bool, get_str, get_u64};
use jetcarp_api::out;
use jetcarp_core::*;
use rayon::prelude::*;
use serde_json::{Map, Value, json};
use std::io::Write as _;
use std::path::Path;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

/// What a per-run callback says: keep the run (write its image) and extra manifest columns.
pub struct Verdict {
    pub keep: bool,
    pub extra: Map<String, Value>,
}

pub type Callback<'a> = dyn Fn(&Shared, &Item) -> Result<Verdict, String> + Sync + 'a;

/// Every sweep option (besides the callback `keep`).
pub const SWEEP_KEYS: &[&str] = &[
    // series
    "rules",
    "from",
    "to",
    "birth",
    "survive",
    "states",
    "sample",
    "sample_seed",
    // world
    "tiling",
    "size",
    "boundary",
    "level",
    "params",
    "edges",
    "rng_seed",
    "screen",
    "cell_px",
    // CA
    "seed",
    "neighborhood",
    "palette",
    "steps_per_gen",
    "affects_others",
    "color_by",
    "agent_edge",
    "drop_per_gen",
    // run
    "until",
    "max_gens",
    "gens",
    "n",
    "fraction",
    "for_gens",
    "max_period",
    // output
    "png",
    "out",
    "folder",
    "resume",
    "threads",
    "describe",
    "keep",
    "name",
    "skip",
    // a scene as the start of every run
    "scene",
    "ca",
];

/// Columns that come first in the manifest, in this order.
const COLUMNS: &[&str] = &[
    "index",
    "n",
    "rule",
    "file",
    "kept",
    "skipped",
    "generations",
    "stopped",
    "done",
    "done_reason",
    "hash",
    "population",
    "density",
    "mean_height",
    "frac_0",
    "frac_1",
    "frac_2",
    "frac_3",
    "period",
    "row_period",
    "row_shift",
    "periodic_from",
    "distinct_rows",
    "row_class",
    "row_entropy",
    "row_hint",
    "png_bytes",
    "error",
];

struct Plan {
    items: Vec<Item>,
    topo: Arc<Topology>,
    rng_seed: u64,
    base: Base,
    until: Until,
    image: Option<ImageSpec>,
    screen: Screen,
    describe: bool,
    skip: Vec<String>,
}

/// What every run starts from.
enum Base {
    /// An empty world with one CA (the run's rule with the sweep's CA options).
    Plain { ca: Value },
    /// A scene's start: its CAs, seeds, and the edits made before its first step (drawn cells),
    /// with one CA running the run's rule (plus the sweep's CA options).
    Scene {
        scene: Box<Scene>,
        /// The `ca` option: a position in the start world's CA list (from 0) or a name.
        select: Value,
        ca: Value,
        /// The varied CA, counted in creation order (the scene's CAs, then its `add_ca`
        /// commands); set by [`Base::resolve`].
        vary: usize,
    },
}

/// Results a sweep can leave out (no image; the manifest row stays, with `skipped`).
pub const SKIP_KINDS: &[&str] = &["empty", "full", "uniform"];

fn ca_options(v: &Value) -> Value {
    let mut ca = Map::new();
    for k in args::CA_KEYS {
        if let Some(x) = get(v, k) {
            ca.insert(k.to_string(), x.clone());
        }
    }
    Value::Object(ca)
}

/// The base of every run, and the RNG seed (the scene's unless `rng_seed` is given).
fn parse_base(v: &Value) -> Result<(Base, u64), String> {
    let ca = ca_options(v);
    let rng = get_u64(v, "rng_seed")?;
    let Some(sc) = get(v, "scene") else {
        if get(v, "ca").is_some() {
            return Err("`ca` (which CA to sweep) needs a `scene`".into());
        }
        return Ok((Base::Plain { ca }, rng.unwrap_or(1)));
    };
    let scene: Scene = match sc {
        Value::String(t) => Scene::from_json(t)?,
        x => serde_json::from_value(x.clone()).map_err(|e| format!("bad scene: {e}"))?,
    };
    let rng = rng.unwrap_or(scene.rng_state);
    Ok((
        Base::Scene {
            scene: Box::new(scene),
            select: get(v, "ca").cloned().unwrap_or(json!(0)),
            ca,
            vary: 0,
        },
        rng,
    ))
}

impl Base {
    /// Find the varied CA of a scene base (replays the scene's start once).
    fn resolve(&mut self, topo: &Arc<Topology>, rng: u64) -> Result<(), String> {
        let Base::Scene {
            scene,
            select,
            vary,
            ..
        } = self
        else {
            return Ok(());
        };
        let (w, created) = replay_start(topo, rng, scene, None)?;
        if w.cas.is_empty() {
            return Err("the sweep's scene has no CA".into());
        }
        let pos = match select {
            Value::String(name) => w
                .cas
                .iter()
                .position(|c| c.name.eq_ignore_ascii_case(name) || c.notation() == *name)
                .ok_or_else(|| format!("the scene has no CA named \"{name}\""))?,
            x => {
                let i = x
                    .as_u64()
                    .ok_or_else(|| format!("`ca` should be a number (from 0) or a name, got {x}"))?
                    as usize;
                if i >= w.cas.len() {
                    return Err(format!(
                        "`ca` is {i}, but the scene has {} CAs (counting from 0)",
                        w.cas.len()
                    ));
                }
                i
            }
        };
        let id = w.cas[pos].id;
        *vary = created
            .iter()
            .position(|&c| c == id)
            .ok_or("the scene's CA ids are not consistent")?;
        Ok(())
    }
}

/// A scene's start on `topo`: its CAs, then its commands up to the first step (scripted
/// sequences are left out). With `vary` = (k, rule, options), the k-th CA created runs `rule`
/// and later rule changes to it are ignored. Returns the world and the ids of the CAs created,
/// in order.
fn replay_start(
    topo: &Arc<Topology>,
    rng: u64,
    scene: &Scene,
    vary: Option<(usize, &str, &Value)>,
) -> Result<(World, Vec<u32>), String> {
    let mut w = World::on_topology(topo.clone(), rng);
    for t in &scene.tables {
        w.apply(Command::AddTable { table: t.clone() })?;
    }
    for p in &scene.palettes {
        w.apply(Command::SetPalette { palette: p.clone() })?;
    }
    let mut created = vec![];
    let fix = |k: usize, mut s: CaSpec| -> Result<CaSpec, String> {
        if let Some((kv, rule, opts)) = vary
            && kv == k
        {
            let r = args::resolve_rule(rule);
            if s.rule != r {
                // A name like "Rule 30" would be wrong for the new rule.
                s.name = None;
                s.rule = r;
            }
            args::apply_ca_options(&mut s, opts)?;
        }
        Ok(s)
    };
    for s in &scene.cas {
        let s = fix(created.len(), s.clone())?;
        created.push(w.apply(Command::AddCa { ca: s })?.new_id.unwrap_or(0));
    }
    for c in &scene.commands {
        let vary_id = vary.and_then(|(k, _, _)| created.get(k).copied());
        match c {
            Command::Step { .. } => break,
            Command::SetScript { .. } | Command::ScriptGoto { .. } => {}
            Command::AddCa { ca } => {
                let s = fix(created.len(), ca.clone())?;
                let id = w.apply(Command::AddCa { ca: s })?.new_id.unwrap_or(0);
                created.push(id);
            }
            Command::UpdateCa { id, change } if Some(*id) == vary_id && change.rule.is_some() => {
                let mut change = change.clone();
                change.rule = None;
                w.apply(Command::UpdateCa { id: *id, change })?;
            }
            c => {
                w.apply(c.clone())?;
            }
        }
    }
    Ok((w, created))
}

/// A run's world at generation 0, and the id of the CA whose rule the sweep varies.
fn start_world(
    topo: &Arc<Topology>,
    rng: u64,
    base: &Base,
    rule: &str,
) -> Result<(World, u32), String> {
    match base {
        Base::Plain { ca } => {
            let mut w = World::on_topology(topo.clone(), rng);
            // Through a command, so the world's scene (Save scene, Quick Start) includes the CA.
            let id = w
                .apply(Command::AddCa {
                    ca: args::ca_spec(rule, ca)?,
                })?
                .new_id
                .unwrap_or(0);
            Ok((w, id))
        }
        Base::Scene {
            scene, ca, vary, ..
        } => {
            let (w, created) = replay_start(topo, rng, scene, Some((*vary, rule, ca)))?;
            Ok((w, created[*vary]))
        }
    }
}

fn parse_skip(v: &Value) -> Result<Vec<String>, String> {
    let list: Vec<String> = match get(v, "skip") {
        None => vec![],
        Some(Value::String(s)) => s
            .split([',', ' '])
            .filter(|x| !x.is_empty())
            .map(|x| x.trim().to_ascii_lowercase())
            .collect(),
        Some(Value::Array(a)) => a
            .iter()
            .map(|x| {
                x.as_str()
                    .map(|s| s.trim().to_ascii_lowercase())
                    .ok_or(format!("skip items should be text, got {x}"))
            })
            .collect::<Result<_, _>>()?,
        Some(x) => {
            return Err(format!(
                "`skip` should be a list like [\"empty\", \"uniform\"], got {x}"
            ));
        }
    };
    for k in &list {
        if !SKIP_KINDS.contains(&k.as_str()) {
            return Err(format!(
                "unknown skip \"{k}\"; use {}",
                SKIP_KINDS.join(", ")
            ));
        }
    }
    Ok(list)
}

/// Why a result is skipped, from its descriptors: `empty` (no live cells), `full` (no cell in
/// state 0), `uniform` (at most two distinct rows; off lattices, every cell the same).
pub fn skip_reason(skip: &[String], d: &Map<String, Value>) -> Option<String> {
    let num = |k: &str| d.get(k).and_then(|x| x.as_f64());
    let empty = num("population") == Some(0.0);
    let full = num("frac_0") == Some(0.0);
    let uniform = match d.get("row_class").and_then(|c| c.as_str()) {
        Some(c) => c == "empty" || c == "uniform",
        None => {
            empty
                || d.iter()
                    .any(|(k, x)| k.starts_with("frac_") && x.as_f64() == Some(1.0))
        }
    };
    skip.iter()
        .find(|k| match k.as_str() {
            "empty" => empty,
            "full" => full,
            "uniform" => uniform,
            _ => false,
        })
        .cloned()
}

/// The screen a sweep fills: the host's, changed by `screen` and `cell_px`.
fn sweep_screen(v: &Value, mut screen: Screen) -> Result<Screen, String> {
    if let Some(s) = get_str(v, "screen")? {
        let (w, h) = Screen::parse_size(s)?;
        (screen.width, screen.height) = (w, h);
    }
    if let Some(px) = args::get_f64(v, "cell_px")? {
        screen.cell_px = px.clamp(0.5, 4096.0);
    }
    Ok(screen)
}

struct Prepared {
    plan: Plan,
    tiling: TilingSpec,
    /// The options with defaults filled in (`tiling: "hex"` for hexagon rules).
    v: Value,
}

fn prepare(
    v: &Value,
    host_screen: Screen,
    check: &dyn Fn(&TilingSpec) -> Result<(), String>,
) -> Result<Prepared, String> {
    args::check_keys(v, SWEEP_KEYS, "sweep")?;
    let screen = sweep_screen(v, host_screen)?;
    let items = series::expand(v)?;
    let (mut base, rng_seed) = parse_base(v)?;
    let mut v = v.clone();
    let tiling = match &base {
        Base::Scene { scene, .. } => {
            if let Some(k) = ["tiling", "size", "boundary", "level", "params", "edges"]
                .iter()
                .find(|k| get(&v, k).is_some())
            {
                return Err(format!(
                    "a sweep with a `scene` takes its tiling from the scene; leave out `{k}`"
                ));
            }
            scene.tiling.clone()
        }
        Base::Plain { .. } => {
            // Hexagon elementary rules need the hexagon grid: make it the default for them.
            // (flat-top hexagons for the three-parent rules).
            if get(&v, "tiling").is_none()
                && let Some(first) = items.first()
            {
                match jetcarp_core::rules::Rule::parse(&first.rule) {
                    Ok((jetcarp_core::rules::Rule::HexWolfram { .. }, _)) => {
                        v["tiling"] = json!("hex")
                    }
                    Ok((jetcarp_core::rules::Rule::HexTri { .. }, _)) => {
                        v["tiling"] = json!("hex_flat")
                    }
                    _ => {}
                }
            }
            args::tiling_spec(&v, &screen)?
        }
    };
    check(&tiling)?;
    let topo = Arc::new(Topology::build(&tiling)?);
    base.resolve(&topo, rng_seed)?;
    let until = args::parse_until(&v, 10_000)?;
    let image = match get(&v, "png") {
        Some(Value::Bool(false)) => None,
        None | Some(Value::Bool(true)) => Some(sim::image_spec(&Value::Null, &screen)?),
        Some(p) => Some(sim::image_spec(p, &screen)?),
    };
    let plan = Plan {
        items,
        topo,
        rng_seed,
        base,
        until,
        image,
        screen,
        describe: get_bool(&v, "describe")?.unwrap_or(true),
        skip: parse_skip(&v)?,
    };
    // Check the first rule now, so a typo fails at once rather than in every run.
    if let Some(first) = plan.items.first() {
        start_world(&plan.topo, plan.rng_seed, &plan.base, &first.rule)
            .map_err(|e| format!("rule \"{}\": {e}", first.rule))?;
    }
    Ok(Prepared { plan, tiling, v })
}

pub(crate) fn run_sweep(ctx: &Ctx, v: &Value, cb: Option<&Callback>) -> Result<Value, String> {
    let t0 = Instant::now();
    let started = out::now_rfc3339();
    let Prepared { plan, tiling, v } = prepare(v, ctx.opts.screen, &|t| ctx.check_tiling(t))?;
    ctx.check_cells(plan.topo.len())?;
    let screen = plan.screen;
    let v = &v;

    // The output folder.
    let label = get_str(v, "out")?
        .map(String::from)
        .or_else(|| get_str(v, "name").ok().flatten().map(String::from))
        .unwrap_or_else(|| default_label(&plan.items));
    let folder = match get_str(v, "folder")? {
        Some(f) => {
            let p = ctx.writable_dir(f)?;
            ctx.add_out_dir(&p);
            p
        }
        None => ctx.out_dir(&label)?,
    };
    let resume = get_bool(v, "resume")?.unwrap_or(true);
    let settings = settings_json(v, &plan, &tiling);
    let run_json = folder.join("run.json");
    if resume
        && let Ok(old) = std::fs::read_to_string(&run_json)
        && let Ok(old) = serde_json::from_str::<Value>(&old)
        && old.get("sweep").is_some_and(|s| s != &settings)
    {
        return Err(format!(
            "{} holds a different sweep (its run.json settings differ); use a new folder or resume: false",
            folder.display()
        ));
    }
    let mut done_rows: Map<String, Value> = Map::new();
    if resume {
        for row in previous_rows(&folder) {
            if let Some(i) = row.get("index").and_then(|i| i.as_u64()) {
                let file_ok = match row.get("file").and_then(|f| f.as_str()) {
                    Some(f) if !f.is_empty() => folder.join(f).exists(),
                    _ => true,
                };
                if file_ok && row.get("error").is_none_or(|e| e.is_null()) {
                    done_rows.insert(i.to_string(), row);
                }
            }
        }
    }
    let skipped = done_rows.len();
    let threads = get_u64(v, "threads")?
        .map(|t| t.clamp(1, 1024) as usize)
        .or(ctx.opts.threads);
    let record = |extra: Value| -> Value {
        let mut r = json!({
            "jetcarp_version": jetcarp_core::VERSION,
            "kind": "sweep",
            "started": started,
            "host": out::host_name(),
            "os": std::env::consts::OS,
            "threads": threads.unwrap_or_else(rayon::current_num_threads),
            "screen": screen,
            "script_path": ctx.opts.script_path.as_ref().map(|p| p.display().to_string()),
            "params": ctx.params_json(),
            "sweep": settings,
            "tiling": plan.topo.describe(),
            "cells": plan.topo.len(),
            "dead_boundary": plan.topo.has_boundary(),
            "runs": plan.items.len(),
            "script": ctx.source,
        });
        if let (Some(o), Some(e)) = (r.as_object_mut(), extra.as_object()) {
            for (k, x) in e {
                o.insert(k.clone(), x.clone());
            }
        }
        r
    };
    out::write_bytes(
        &run_json,
        serde_json::to_string_pretty(&record(json!({"finished": null})))
            .unwrap()
            .as_bytes(),
    )?;
    ctx.log(format!(
        "sweep: {} runs of {} on {} ({} cells), until {:?} (max {} generations) -> {}{}",
        plan.items.len(),
        plan.items.first().map(|i| i.rule.as_str()).unwrap_or("-"),
        plan.topo.describe(),
        plan.topo.len(),
        plan.until.trigger,
        plan.until.max_gens,
        folder.display(),
        if skipped > 0 {
            format!(" (resuming: {skipped} already done)")
        } else {
            String::new()
        }
    ));

    // Run.
    let partial = Mutex::new(
        std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(folder.join("manifest.partial.jsonl"))
            .map_err(|e| format!("manifest.partial.jsonl: {e}"))?,
    );
    let todo: Vec<&Item> = plan
        .items
        .iter()
        .filter(|it| !done_rows.contains_key(&it.index.to_string()))
        .collect();
    let total = todo.len();
    let finished = AtomicUsize::new(0);
    let written = AtomicU64::new(0);
    let last_report = Mutex::new(Instant::now());
    let rows: Mutex<Vec<Value>> = Mutex::new(Vec::with_capacity(total));
    let failure: Mutex<Option<String>> = Mutex::new(None);
    let stop = || ctx.stopped() || failure.lock().unwrap().is_some();
    let work = || {
        todo.par_iter().with_max_len(1).for_each(|item| {
            if stop() {
                return;
            }
            match run_one(&plan, item, &folder, cb, &stop) {
                Ok(Some(row)) => {
                    if row
                        .get("file")
                        .is_some_and(|f| f.as_str().is_some_and(|s| !s.is_empty()))
                    {
                        written.fetch_add(1, Ordering::Relaxed);
                    }
                    if let Ok(mut f) = partial.lock() {
                        let _ = writeln!(f, "{}", row);
                    }
                    rows.lock().unwrap().push(row);
                    let k = finished.fetch_add(1, Ordering::Relaxed) + 1;
                    let mut last = last_report.lock().unwrap();
                    if k == total || last.elapsed().as_secs_f64() >= 0.25 {
                        *last = Instant::now();
                        drop(last);
                        let el = t0.elapsed().as_secs_f64();
                        ctx.progress(Progress {
                            label: format!("sweep {label}"),
                            done: k as u64,
                            total: total as u64,
                            item: item.rule.clone(),
                            elapsed: el,
                            eta: Some(el / k as f64 * (total - k) as f64),
                        });
                    }
                }
                Ok(None) => {}
                Err(e) => {
                    let mut f = failure.lock().unwrap();
                    if f.is_none() {
                        *f = Some(format!("sweep run \"{}\": {e}", item.rule));
                    }
                }
            }
        });
    };
    match threads {
        Some(t) if t != rayon::current_num_threads() => rayon::ThreadPoolBuilder::new()
            .num_threads(t)
            .build()
            .map_err(|e| e.to_string())?
            .install(work),
        _ => work(),
    }

    // Manifest, sorted by index.
    let mut all: Vec<Value> = done_rows.into_iter().map(|(_, r)| r).collect();
    all.extend(rows.into_inner().unwrap());
    all.sort_by_key(|r| r.get("index").and_then(|i| i.as_u64()).unwrap_or(0));
    let columns: Vec<String> = COLUMNS
        .iter()
        .filter(|c| all.iter().any(|r| r.get(**c).is_some()))
        .map(|c| c.to_string())
        .collect();
    out::write_bytes(
        &folder.join("manifest.csv"),
        out::to_csv(&all, &columns).as_bytes(),
    )?;
    let mut cols = columns.clone();
    for r in &all {
        for k in r
            .as_object()
            .map(|o| o.keys().cloned().collect::<Vec<_>>())
            .unwrap_or_default()
        {
            if !cols.contains(&k) {
                cols.push(k);
            }
        }
    }
    out::write_bytes(
        &folder.join("manifest.json"),
        serde_json::to_string_pretty(&json!({"columns": cols, "rows": all}))
            .unwrap()
            .as_bytes(),
    )?;
    let failed = failure.into_inner().unwrap();
    let cancelled = ctx.stopped();
    let complete = all.len() == plan.items.len();
    if complete {
        let _ = std::fs::remove_file(folder.join("manifest.partial.jsonl"));
    }
    let secs = t0.elapsed().as_secs_f64();
    let kept = all
        .iter()
        .filter(|r| r.get("kept") == Some(&json!(true)))
        .count();
    let images = all
        .iter()
        .filter(|r| {
            r.get("file")
                .is_some_and(|f| f.as_str().is_some_and(|s| !s.is_empty()))
        })
        .count();
    let skipped_n = all.iter().filter(|r| r.get("skipped").is_some()).count();
    let errors = all
        .iter()
        .filter(|r| r.get("error").is_some_and(|e| !e.is_null()))
        .count();
    let summary = json!({
        "folder": folder.display().to_string(),
        "runs": plan.items.len(),
        "completed": all.len(),
        "ran_now": all.len().saturating_sub(skipped),
        "resumed": skipped,
        "kept": kept,
        "skipped": skipped_n,
        "images": images,
        "images_written_now": written.load(Ordering::Relaxed),
        "errors": errors,
        "seconds": (secs * 1000.0).round() / 1000.0,
        "cancelled": cancelled,
        "complete": complete,
        "manifest": folder.join("manifest.csv").display().to_string(),
    });
    out::write_bytes(
        &run_json,
        serde_json::to_string_pretty(&record(json!({
            "finished": out::now_rfc3339(),
            "seconds": summary["seconds"],
            "summary": summary,
        })))
        .unwrap()
        .as_bytes(),
    )?;
    ctx.files_written
        .fetch_add(written.load(Ordering::Relaxed) + 3, Ordering::Relaxed);
    ctx.log(format!(
        "sweep done: {} of {} runs, {images} images, {kept} kept, {errors} errors in {:.1}s{}",
        all.len(),
        plan.items.len(),
        secs,
        if cancelled { " (cancelled)" } else { "" }
    ));
    ctx.record_sweep(summary.clone());
    if cancelled {
        return Err(format!(
            "stopped after {} of {} runs; run again with folder: \"{}\" to resume",
            all.len(),
            plan.items.len(),
            folder.display()
        ));
    }
    if let Some(e) = failed {
        return Err(e);
    }
    Ok(summary)
}

fn run_one(
    plan: &Plan,
    item: &Item,
    folder: &Path,
    cb: Option<&Callback>,
    stop: &(dyn Fn() -> bool + Sync),
) -> Result<Option<Value>, String> {
    let mut row = Map::new();
    row.insert("index".into(), json!(item.index));
    row.insert("n".into(), item.n.map_or(Value::Null, |n| json!(n)));
    row.insert("rule".into(), json!(item.rule));
    let (mut w, vary) = match start_world(&plan.topo, plan.rng_seed, &plan.base, &item.rule) {
        Ok(x) => x,
        Err(e) => {
            row.insert("kept".into(), json!(false));
            row.insert("error".into(), json!(e));
            return Ok(Some(Value::Object(row)));
        }
    };
    let o = sim::run_until(&mut w, &plan.until, stop);
    if o.stopped == "cancelled" {
        return Ok(None);
    }
    let shared = Shared::new(w, plan.screen);
    let verdict = match cb {
        Some(f) => f(&shared, item)?,
        None => Verdict {
            keep: true,
            extra: Map::new(),
        },
    };
    let mut sim_ = shared.lock();
    let w = &mut sim_.world;
    let desc = (plan.describe || !plan.skip.is_empty()).then(|| describe(w, Some(vary)));
    let skipped = desc.as_ref().and_then(|d| skip_reason(&plan.skip, d));
    let keep = verdict.keep && skipped.is_none();
    row.insert("kept".into(), json!(keep));
    if let Some(r) = &skipped {
        row.insert("skipped".into(), json!(r));
    }
    row.insert("generations".into(), json!(o.generations));
    row.insert("stopped".into(), json!(o.stopped));
    row.insert("done".into(), json!(o.done));
    row.insert(
        "done_reason".into(),
        o.done_reason.map_or(Value::Null, |r| json!(r)),
    );
    row.insert("hash".into(), json!(w.hash_hex()));
    match desc {
        Some(d) if plan.describe => row.extend(d),
        _ => {
            row.insert(
                "population".into(),
                json!(w.ca(vary).map(|c| w.fields[c.field].population())),
            );
        }
    }
    match (&plan.image, keep) {
        (Some(spec), true) => {
            sim::apply_palette(w, spec)?;
            let name = format!("{}.png", item.stem);
            let (_, _, bytes) = sim::save_png(w, spec, &folder.join(&name))?;
            row.insert("file".into(), json!(name));
            row.insert("png_bytes".into(), json!(bytes));
        }
        _ => {
            row.insert("file".into(), json!(""));
        }
    }
    for (k, x) in verdict.extra {
        if k != "keep" {
            row.insert(k, x);
        }
    }
    Ok(Some(Value::Object(row)))
}

/// Rows from an earlier, possibly interrupted, run of the same sweep in this folder.
fn previous_rows(folder: &Path) -> Vec<Value> {
    let mut rows = Vec::new();
    if let Ok(t) = std::fs::read_to_string(folder.join("manifest.json"))
        && let Ok(v) = serde_json::from_str::<Value>(&t)
        && let Some(r) = v.get("rows").and_then(|r| r.as_array())
    {
        rows.extend(r.iter().cloned());
    }
    if let Ok(t) = std::fs::read_to_string(folder.join("manifest.partial.jsonl")) {
        rows.extend(
            t.lines()
                .filter_map(|l| serde_json::from_str::<Value>(l).ok()),
        );
    }
    rows
}

/// The settings that define a sweep's results (for run.json and resume checks).
fn settings_json(v: &Value, plan: &Plan, tiling: &TilingSpec) -> Value {
    let mut s = Map::new();
    if let Some(o) = v.as_object() {
        for (k, x) in o {
            if !matches!(
                k.as_str(),
                "keep" | "threads" | "resume" | "folder" | "out" | "name"
            ) {
                s.insert(k.clone(), x.clone());
            }
        }
    }
    s.insert(
        "resolved_tiling".into(),
        serde_json::to_value(tiling).unwrap_or(Value::Null),
    );
    s.insert(
        "until_resolved".into(),
        serde_json::to_value(&plan.until).unwrap_or(Value::Null),
    );
    s.insert(
        "image".into(),
        serde_json::to_value(&plan.image).unwrap_or(Value::Null),
    );
    s.insert(
        "screen".into(),
        serde_json::to_value(plan.screen).unwrap_or(Value::Null),
    );
    s.insert("callback".into(), json!(v.get("keep").is_some()));
    Value::Object(s)
}

fn default_label(items: &[Item]) -> String {
    match items.first() {
        Some(i) if i.n.is_some() => i
            .stem
            .trim_end_matches(|c: char| c.is_ascii_digit())
            .trim_end_matches('-')
            .to_string(),
        Some(_) => "sweep".into(),
        None => "sweep".into(),
    }
}

/// A quick timing of a sweep before it starts (see [`estimate`]).
#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub struct Estimate {
    pub runs: usize,
    pub cells: usize,
    pub tiling: String,
    /// Mean seconds for one run on one thread (running, descriptors, and the image).
    pub seconds_per_run: f64,
    /// The runs timed: rule, generations, seconds.
    pub samples: Vec<(String, u64, f64)>,
    /// Image size, if images are written.
    pub image: Option<(u32, u32)>,
    /// Mean PNG size in bytes, if images are written.
    pub bytes_per_image: Option<f64>,
}

impl Estimate {
    /// Rough wall time on `threads` worker threads.
    pub fn seconds(&self, threads: usize) -> f64 {
        let t = threads.max(1).min(self.runs.max(1));
        self.seconds_per_run * self.runs as f64 / t as f64
    }
}

/// Time `samples` runs of a sweep given as JSON (the first, then evenly spread), without writing
/// anything: for hosts that show the size and rough duration of a sweep before starting it.
pub fn estimate(
    v: &Value,
    screen: Screen,
    samples: usize,
    stop: &dyn Fn() -> bool,
) -> Result<Estimate, String> {
    let Prepared { plan, .. } = prepare(v, screen, &|_| Ok(()))?;
    let n = plan.items.len();
    let k = samples.clamp(1, n.max(1));
    let mut picked: Vec<usize> = (0..k).map(|i| i * n / k).collect();
    picked.dedup();
    let mut out = vec![];
    let mut image = None;
    let mut bytes = vec![];
    for &i in picked.iter().filter(|&&i| i < n) {
        if stop() {
            break;
        }
        let item = &plan.items[i];
        let t = Instant::now();
        let (mut w, vary) = start_world(&plan.topo, plan.rng_seed, &plan.base, &item.rule)?;
        let o = sim::run_until(&mut w, &plan.until, stop);
        if plan.describe || !plan.skip.is_empty() {
            let _ = describe(&w, Some(vary));
        }
        if let Some(spec) = &plan.image {
            sim::apply_palette(&mut w, spec)?;
            let img = sim::render_image(&w, spec);
            image = Some((img.width, img.height));
            let level = match spec.compression.as_str() {
                "fast" => png::Compression::Fast,
                "high" => png::Compression::High,
                _ => png::Compression::Balanced,
            };
            bytes.push(out::encode_png(&img, &[], level)?.len());
        }
        out.push((item.rule.clone(), o.generations, t.elapsed().as_secs_f64()));
    }
    let mean = if out.is_empty() {
        0.0
    } else {
        out.iter().map(|x| x.2).sum::<f64>() / out.len() as f64
    };
    Ok(Estimate {
        runs: n,
        cells: plan.topo.len(),
        tiling: plan.topo.describe(),
        seconds_per_run: mean,
        samples: out,
        image,
        bytes_per_image: (!bytes.is_empty())
            .then(|| bytes.iter().sum::<usize>() as f64 / bytes.len() as f64),
    })
}

/// One run of a finished sweep at generation 0, rebuilt from the folder's `run.json` and the
/// run's rule (a manifest row): for hosts that open a sweep result to watch it live. Running it
/// for the row's `generations` reproduces the row's hash.
pub fn run_world(run_json: &Value, rule: &str) -> Result<World, String> {
    let settings = run_json
        .get("sweep")
        .ok_or("run.json has no sweep settings (is this a sweep folder?)")?;
    let (mut base, rng) = parse_base(settings)?;
    let tiling: TilingSpec = match settings.get("resolved_tiling") {
        Some(t) if !t.is_null() => {
            serde_json::from_value(t.clone()).map_err(|e| format!("run.json tiling: {e}"))?
        }
        _ => {
            let screen = run_json
                .get("screen")
                .and_then(|s| serde_json::from_value::<Screen>(s.clone()).ok())
                .unwrap_or_default();
            args::tiling_spec(settings, &sweep_screen(settings, screen)?)?
        }
    };
    let topo = Arc::new(Topology::build(&tiling)?);
    base.resolve(&topo, rng)?;
    start_world(&topo, rng, &base, rule).map(|(w, _)| w)
}

/// A `sweep(#{...})` script for a sweep given as JSON (for hosts that build sweeps from a form:
/// the user can then read, edit, and save the script).
pub fn sweep_to_script(spec: &Value) -> String {
    format!(
        "// Made from the sweep form.\nsweep({});\n",
        rhai_literal(spec, 0)
    )
}

/// JSON as a Rhai literal: maps become `#{ key: value }`, null becomes `()`.
pub fn rhai_literal(v: &Value, indent: usize) -> String {
    let pad = "    ".repeat(indent + 1);
    match v {
        Value::Null => "()".into(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => {
            if n.is_f64() {
                let f = n.as_f64().unwrap();
                if f.fract() == 0.0 {
                    format!("{f:.1}")
                } else {
                    f.to_string()
                }
            } else {
                n.to_string()
            }
        }
        Value::String(s) => serde_json::to_string(s).unwrap(),
        Value::Array(a) => format!(
            "[{}]",
            a.iter()
                .map(|x| rhai_literal(x, indent))
                .collect::<Vec<_>>()
                .join(", ")
        ),
        Value::Object(o) => {
            if o.is_empty() {
                return "#{}".into();
            }
            let items: Vec<String> = o
                .iter()
                .map(|(k, x)| {
                    let key = if k.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
                        && !k.starts_with(|c: char| c.is_ascii_digit())
                    {
                        k.clone()
                    } else {
                        serde_json::to_string(k).unwrap()
                    };
                    format!("{pad}{key}: {}", rhai_literal(x, indent + 1))
                })
                .collect();
            format!("#{{\n{},\n{}}}", items.join(",\n"), "    ".repeat(indent))
        }
    }
}
