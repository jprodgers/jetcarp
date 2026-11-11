//! Searches from scripts: `hunt`, `census`, `ants`, `evolve`, `track_mover` (on a sliding
//! window), `sandpile_pile`, `lib.search`, and `lib.collide` start (or carry on) a catalog job
//! through the operation `search.start` and run it on the script's thread until it ends, its
//! `limit` has run, or the script is cancelled; `*_status`, `*_stop`, `hunts`, and `ant_jobs` are
//! `jobs.status`, `jobs.stop`, and `jobs.list`. Each keeps the keys and the result it had (see
//! `help.md`). A `label` on `hunt`, `census`, and `lib.search` names the job, and a call with the
//! same label carries that job on (or, when it is done, gives it as it is), so a script of
//! labelled jobs resumes after a reboot.

use crate::Ctx;
use crate::api::{LibHandle, from_json, to_json};
use jetcarp_api::args::{check_keys, get_bool, get_f64, get_str, get_u64, nb_word};
use jetcarp_api::op::{Op, Progress, Quiet};
use jetcarp_api::ops::jobs::{JobIn, JobsList, JobsStatus, JobsStop, ListIn, StatusIn, StatusOut};
use jetcarp_api::ops::search::{SearchStart, SearchStartIn, SearchStarted};
use jetcarp_search::ants::AntProgress;
use jetcarp_search::census::CensusProgress;
use jetcarp_search::hunt::HuntProgress;
use jetcarp_store::Library;
use rhai::{Array, Dynamic, Engine, EvalAltResult, INT, Map};
use serde_json::{Value, json};
use std::sync::Arc;
use std::time::Instant;

type R<T> = Result<T, Box<EvalAltResult>>;

fn e(s: impl Into<String>) -> Box<EvalAltResult> {
    s.into().into()
}

fn map_json(m: &Map) -> R<Value> {
    to_json(&Dynamic::from_map(m.clone()))
}

/// A script's progress for a job: its progress lines, and cancelled when the script is.
struct ScriptProgress<'a> {
    ctx: &'a Ctx,
    label: String,
    t0: Instant,
}

impl Progress for ScriptProgress<'_> {
    fn report(&self, done: u64, total: Option<u64>, msg: &str) {
        let el = self.t0.elapsed().as_secs_f64();
        let total = total.unwrap_or(0);
        self.ctx.progress(crate::Progress {
            label: self.label.clone(),
            done,
            total,
            item: msg.to_string(),
            elapsed: el,
            eta: (done > 0 && total > done).then(|| el / done as f64 * (total - done) as f64),
        });
    }
    fn cancelled(&self) -> bool {
        self.ctx.stopped()
    }
}

/// The catalog a global function uses: the script's `library` option, else the default.
fn default_lib(c: &Ctx) -> Library {
    match &c.opts.library {
        Some(p) => Library::open(p.clone()),
        None => Library::open_default(),
    }
}

/// Start (or carry on) a job of `strategy` on the script's thread. `v` is the script's map: its
/// `label`, `resume`, `limit`, and `threads` steer the run, the rest is the strategy's spec.
fn start(
    c: &Ctx,
    lib: &Library,
    strategy: &str,
    v: &Value,
    reuse_label: bool,
) -> Result<SearchStarted, String> {
    let mut spec = v.clone();
    if let Some(o) = spec.as_object_mut() {
        for k in ["label", "resume", "limit", "threads"] {
            o.remove(k);
        }
    }
    let label = get_str(v, "label")?.map(String::from);
    let limit = get_u64(v, "limit")?;
    let a = SearchStartIn {
        strategy: Some(strategy.into()),
        spec,
        reuse_label: reuse_label && label.is_some(),
        label,
        resume: get_u64(v, "resume")?.map(|j| j as i64),
        budget: limit.map(|n| jetcarp_api::ops::search::BudgetIn {
            items: Some(n),
            ..Default::default()
        }),
        threads: get_u64(v, "threads")?.map(|t| t as usize),
        ..Default::default()
    };
    let p = ScriptProgress {
        ctx: c,
        label: strategy.into(),
        t0: Instant::now(),
    };
    SearchStart::run(&c.api(Some(lib.clone())), a, &p).map_err(|e| e.to_string())
}

/// A job's progress as its kind's own type, back as JSON (every field present).
fn typed<T: serde::de::DeserializeOwned + Default + serde::Serialize>(v: &Value) -> Value {
    let t: T = serde_json::from_value(v.clone()).unwrap_or_default();
    serde_json::to_value(t).unwrap_or_default()
}

// ---------------------------------------------------------------------------------------------
// Hunts, censuses, ant searches

const HUNT_KEYS: &[&str] = &[
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
    "pop_cap",
    "margin",
    "threads",
    "resume",
    "limit",
    "label",
    "store",
];

/// `hunt(#{ ... })` / `lib.hunt(#{ ... })`: the hunt's progress (`HuntProgress`).
fn hunt(c: &Ctx, lib: &Library, v: &Value) -> Result<Value, String> {
    check_keys(v, HUNT_KEYS, "hunt")?;
    let mut v = v.clone();
    if v.get("soups").is_none() && v.get("resume").is_none() {
        v["soups"] = json!(10_000);
    }
    let t0 = Instant::now();
    let out = start(c, lib, "hunt", &v, true)?;
    let p: HuntProgress = serde_json::from_value(out.progress.clone()).unwrap_or_default();
    c.log(format!("{} ({:.1}s)", p.line(), t0.elapsed().as_secs_f64()));
    serde_json::to_value(&p).map_err(|e| e.to_string())
}

const CENSUS_KEYS: &[&str] = &[
    "tiling", "level", "cells", "nb", "rules", "birth", "survive", "states", "all_max", "sample",
    "soups", "density", "gens", "seed", "explode", "label", "resume", "limit", "threads",
];

/// `census(#{ ... })` / `lib.census(#{ ... })`: the census's progress (`#{ job, status,
/// rules_done, rules_total, per_s, elapsed_s }`).
fn census(c: &Ctx, lib: &Library, v: &Value) -> Result<Value, String> {
    check_keys(v, CENSUS_KEYS, "census")?;
    let t0 = Instant::now();
    let out = start(c, lib, "census", v, true)?;
    let p: CensusProgress = serde_json::from_value(out.progress.clone()).unwrap_or_default();
    c.log(format!("{} ({:.1}s)", p.line(), t0.elapsed().as_secs_f64()));
    serde_json::to_value(&p).map_err(|e| e.to_string())
}

const ANT_KEYS: &[&str] = &[
    "tilings",
    "rules",
    "strings",
    "tables",
    "alphabet",
    "all_max",
    "sample",
    "soups",
    "size",
    "density",
    "seed",
    "steps",
    "max_period",
    "symmetry",
    "threads",
    "resume",
    "limit",
];

/// `ants(#{ ... })` / `lib.ants(#{ ... })`: the search's progress (`AntProgress`).
fn ants(c: &Ctx, lib: &Library, v: &Value) -> Result<Value, String> {
    check_keys(v, ANT_KEYS, "ants")?;
    let mut v = v.clone();
    // Rules as text are separated by commas too, as scripts always wrote them.
    if let Some(Value::String(s)) = v.get("rules") {
        let r: Vec<String> = s
            .split([';', ',', ' '])
            .map(str::trim)
            .filter(|x| !x.is_empty())
            .map(String::from)
            .collect();
        v["rules"] = json!(r);
    }
    if let Some(Value::Bool(false)) = v.get("strings") {
        v.as_object_mut().map(|o| o.remove("strings"));
    }
    let t0 = Instant::now();
    let out = start(c, lib, "ants", &v, false)?;
    let p: AntProgress = serde_json::from_value(out.progress.clone()).unwrap_or_default();
    c.log(format!("{} ({:.1}s)", p.line(), t0.elapsed().as_secs_f64()));
    serde_json::to_value(&p).map_err(|e| e.to_string())
}

/// A job's progress with its spec (`lib.hunt_status(job)`, `lib.ant_status(job)`).
fn status_of<T>(c: &Ctx, lib: &Library, job: i64) -> Result<Value, String>
where
    T: serde::de::DeserializeOwned + Default + serde::Serialize,
{
    let a = StatusIn {
        job: Some(job),
        ..Default::default()
    };
    let StatusOut::One(view) =
        JobsStatus::run(&c.api(Some(lib.clone())), a, &Quiet).map_err(|e| e.to_string())?
    else {
        return Err(format!("no job {job}"));
    };
    let mut v = typed::<T>(&view.progress);
    v["spec"] = view.spec.clone();
    Ok(v)
}

/// The statuses of every job of a kind, oldest first (`lib.hunts()`, `lib.ant_jobs()`).
fn statuses<T>(c: &Ctx, lib: &Library, kind: &str) -> Result<Array, String>
where
    T: serde::de::DeserializeOwned + Default + serde::Serialize,
{
    let a = ListIn {
        kind: Some(kind.into()),
        ..Default::default()
    };
    let l = JobsList::run(&c.api(Some(lib.clone())), a, &Quiet).map_err(|e| e.to_string())?;
    let mut out = Array::new();
    for j in l.jobs.iter().rev() {
        out.push(from_json(&status_of::<T>(c, lib, j.job)?));
    }
    Ok(out)
}

fn stop(c: &Ctx, lib: &Library, job: i64) -> Result<(), String> {
    JobsStop::run(&c.api(Some(lib.clone())), JobIn { job }, &Quiet)
        .map(|_| ())
        .map_err(|e| e.to_string())
}

// ---------------------------------------------------------------------------------------------
// Bounded searches and collisions

const SEARCH_KEYS: &[&str] = &[
    "rules",
    "tilings",
    "soups",
    "size",
    "boundary",
    "gens",
    "density",
    "seed",
    "max_period",
    "soup_radius",
    "store",
    "label",
];

/// `lib.search(#{ ... })`: a bounded soup search whose verified finds are added to `lib` (a job
/// of kind `search`). Returns `#{ groups: [#{ rule, tiling, soups, objects, distinct, patterns:
/// [#{ id, label, class, period, speed, cells, count, new }] }], new_patterns, known_patterns,
/// new_rules, job, ids, stored, seconds }`.
fn search(c: &Ctx, lib: &Library, v: &Value) -> Result<Value, String> {
    check_keys(v, SEARCH_KEYS, "search")?;
    if v.get("rules").is_none_or(|r| r.is_null()) {
        return Err("give `rules`, e.g. [\"B3/S23\"]".into());
    }
    let mut v = v.clone();
    if let Some(Value::String(s)) = v.get("tilings") {
        v["tilings"] = json!(s.replace(';', ","));
    }
    if let Some(Value::String(s)) = v.get("rules") {
        v["rules"] = json!(s.replace(',', ";"));
    }
    if v.get("soups").is_none() {
        v["soups"] = json!(16);
    }
    let t0 = Instant::now();
    let out = start(c, lib, "search", &v, true)?;
    if out.mode == "none" && out.result.is_none() {
        c.log(format!(
            "search job {} ({}) already done",
            out.job,
            out.label.as_deref().unwrap_or("")
        ));
        return Ok(
            json!({ "job": out.job, "skipped": true, "groups": [], "new_patterns": 0,
            "known_patterns": 0, "new_rules": 0, "ids": [], "stored": true, "seconds": 0.0 }),
        );
    }
    let r: jetcarp_search::jobs::search::SearchResult = out
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
                "objects": g.objects,
                "distinct": g.found.len(),
                "patterns": g.found.iter().map(|f| json!({
                    "id": f.id, "label": f.label, "class": f.class, "period": f.period,
                    "speed": f.speed, "cells": f.cells, "count": f.count, "new": f.new,
                })).collect::<Vec<_>>(),
            })
        })
        .collect();
    let rules: Vec<&str> = r.report.spec.rules.iter().map(String::as_str).collect();
    c.log(format!(
        "search {}: {} distinct patterns, {} new in the library, {:.1}s",
        rules.join(" "),
        r.ids.len(),
        r.added.len(),
        t0.elapsed().as_secs_f64()
    ));
    Ok(json!({
        "groups": groups,
        "new_patterns": r.added.len(),
        "known_patterns": r.merged.len(),
        "new_rules": r.new_rules.len(),
        "job": r.stored.then_some(out.job),
        "ids": r.ids,
        "stored": r.stored,
        "seconds": t0.elapsed().as_secs_f64(),
    }))
}

const COLLIDE_KEYS: &[&str] = &[
    "a",
    "b",
    "offsets",
    "dx",
    "dy",
    "phases",
    "orientation_a",
    "orientation_b",
    "gens",
    "margin",
    "rule",
    "store",
    "label",
];

/// `lib.collide(#{ a, b, ... })`: pattern `b` into pattern `a` at every offset and phase (a job of
/// kind `collide`). Returns `#{ job, a, b, rule, tiling, outcomes, products: [#{ id, label, runs,
/// new }], runs: [#{ dx, dy, phase, outcome, gens, result, products }] }`.
fn collide(c: &Ctx, lib: &Library, v: &Value) -> Result<Value, String> {
    check_keys(v, COLLIDE_KEYS, "collide")?;
    let out = start(c, lib, "collide", v, false)?;
    let r = out.result.ok_or_else(|| {
        format!(
            "collision {} stopped before its end ({})",
            out.job, out.status
        )
    })?;
    let rep = &r["report"];
    Ok(json!({
        "job": out.job,
        "a": rep["a_label"], "b": rep["b_label"], "rule": rep["rule"], "tiling": rep["tiling"],
        "outcomes": rep["counts"],
        "products": r["products"],
        "runs": rep["runs"],
    }))
}

// ---------------------------------------------------------------------------------------------
// Evolve searches and movers

const EVOLVE_KEYS: &[&str] = &[
    "tiling",
    "nb",
    "states",
    "pop",
    "gens",
    "seed",
    "radius",
    "horizon",
    "cap",
    "champions",
    "verify",
    "stop_on_glider",
    "store",
    "track_gens",
    "label",
    "threads",
];

/// `evolve(#{ ... })`: the evolutionary glider search (a job of kind `evolve`) in a new output
/// folder (`label`, default `evolve-<tiling>`). Returns a summary map.
fn evolve(c: &Ctx, v: &Value) -> Result<Value, String> {
    check_keys(v, EVOLVE_KEYS, "evolve")?;
    let tiling_key = get_str(v, "tiling")?.unwrap_or("spectre").to_string();
    let label = get_str(v, "label")?
        .map(String::from)
        .unwrap_or_else(|| format!("evolve-{}", tiling_key.replace(':', "-")));
    let dir = c.out_dir(&label)?;
    let mut spec = v.clone();
    if let Some(o) = spec.as_object_mut() {
        o.remove("label");
        o.insert("folder".into(), json!(dir.display().to_string()));
        // The search's own threads; the job runs on them too.
    }
    let lib = default_lib(c);
    let out = start(c, &lib, "evolve", &spec, false)?;
    let Some(r) = out.result else {
        return Ok(json!({
            "dir": dir.display().to_string(), "tiling": tiling_key, "job": out.job,
            "generations": out.progress["generation"], "stopped_early": true,
            "status": out.status,
        }));
    };
    let report: jetcarp_core::evolve::EvolveReport =
        serde_json::from_value(r["report"].clone()).map_err(|e| e.to_string())?;
    let found: Vec<jetcarp_search::evolve::FoundMover> =
        serde_json::from_value(r["found"].clone()).unwrap_or_default();
    let last = report.generations.last();
    Ok(json!({
        "dir": dir.display().to_string(),
        "job": out.job,
        "tiling": tiling_key,
        "generations": report.generations.len(),
        "best_ever": last.map(|g| g.best_ever).unwrap_or(0.0),
        "evaluations": last.map(|g| g.evaluations).unwrap_or(0),
        "unique_evaluations": last.map(|g| g.unique_evaluations).unwrap_or(0),
        "arena_tiles": report.arena_tiles,
        "boundary": report.boundary,
        "elapsed": report.elapsed_s,
        "stopped_early": report.stopped_early,
        "champions": report.champions.iter().map(|ch| json!({
            "rule": ch.rule,
            "fitness": ch.fitness,
            "first_generation": ch.first_generation,
            "glider": ch.verify.as_ref().is_some_and(|v| v.passed),
            "check": ch.verify.as_ref().map(|v| v.reason.clone()).unwrap_or_default(),
        })).collect::<Vec<_>>(),
        "gliders": report.gliders.iter().map(|g| g.rule.clone()).collect::<Vec<_>>(),
        "movers": found.iter().map(|f| json!({
            "rule": f.rule,
            "kind": f.kind,
            "ids": f.ids,
            "added": f.added,
            "summary": f.summary,
        })).collect::<Vec<_>>(),
        "fitness": report.generations.iter().map(|g| json!({
            "generation": g.generation, "best": g.best, "median": g.median, "best_ever": g.best_ever,
        })).collect::<Vec<_>>(),
    }))
}

const TRACK_KEYS: &[&str] = &[
    "rule",
    "tiling",
    "level",
    "cells",
    "nb",
    "max_level",
    "gens",
    "max_pop",
    "store",
    "window",
    "rings",
    "margin",
    "fixed_levels",
];

/// `track_mover(#{ ... })`: follow a pattern over growing patch levels (here), or on a sliding
/// window (`window: true` or `rings`: a job of kind `mover`). Returns the tracker's report, with
/// the stored record's `id`.
fn track_mover(c: &Ctx, v: &Value) -> Result<Value, String> {
    use jetcarp_core::pattern::movers::{TrackSpec, mover_record, track};
    use jetcarp_core::pattern::{DiscoveredBy, Provenance, now_utc};
    use jetcarp_core::topology::{Nb, SizeSpec, TilingSpec};
    check_keys(v, TRACK_KEYS, "track_mover")?;
    if !v.get("cells").is_some_and(|x| x.is_array()) {
        return Err("track_mover needs cells: [[cell, state], ...]".into());
    }
    let source = match &c.opts.script_path {
        Some(p) => format!("script {}", p.display()),
        None => "script".into(),
    };
    let window =
        get_bool(v, "window")?.unwrap_or(false) || v.get("rings").is_some_and(|x| !x.is_null());
    if window {
        let mut spec = v.clone();
        if let Some(o) = spec.as_object_mut() {
            o.remove("window");
            o.remove("max_level");
            o.insert("source".into(), json!(source));
            if let Some(p) = o.get("max_pop").and_then(|x| x.as_f64()) {
                o.insert("max_pop".into(), json!(p as u64));
            }
        }
        let lib = default_lib(c);
        let out = start(c, &lib, "mover", &spec, false)?;
        let mut rep =
            out.progress.get("report").cloned().ok_or_else(|| {
                format!("mover {} stopped before its end ({})", out.job, out.status)
            })?;
        if let Some(id) = out.progress.get("record").and_then(|x| x.as_str()) {
            rep["id"] = json!(id);
        }
        return Ok(rep);
    }
    let family = get_str(v, "tiling")?.unwrap_or("spectre");
    let level = get_u64(v, "level")?.unwrap_or(4) as u32;
    let cells: Vec<(u32, u8)> = v["cells"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|p| {
            let p = p
                .as_array()
                .filter(|p| p.len() == 2)
                .ok_or("cells: [[cell, state], ...]")?;
            Ok((
                p[0].as_u64().ok_or("cells: [[cell, state], ...]")? as u32,
                p[1].as_u64().ok_or("cells: [[cell, state], ...]")? as u8,
            ))
        })
        .collect::<Result<_, &str>>()?;
    let nb = match get_str(v, "nb")? {
        None => Nb::Touch,
        Some(o) => nb_word(o).ok_or_else(|| format!("nb '{o}' is not touch or edge"))?,
    };
    let patch = TilingSpec::new(family).with_size(SizeSpec::Level(level));
    let mut spec = TrackSpec::new(
        get_str(v, "rule")?.unwrap_or("SpectreGlider"),
        nb,
        patch.clone(),
        cells,
    );
    spec.max_level = get_u64(v, "max_level")?.map(|x| x as u32);
    if let Some(g) = get_u64(v, "gens")? {
        spec.max_gens = g;
    }
    if let Some(p) = get_f64(v, "max_pop")? {
        spec.max_pop = p as usize;
    }
    let rep = track(&spec)?;
    let mut out = serde_json::to_value(&rep).map_err(|e| e.to_string())?;
    if get_bool(v, "store")?.unwrap_or(false) && rep.verified {
        let prov = Provenance {
            source,
            tiling: Some(patch),
            jetcarp_version: jetcarp_core::VERSION.into(),
            date: now_utc(),
            ..Default::default()
        };
        let rec = mover_record(&spec, &rep, prov, DiscoveredBy::Script)?;
        let id = rec.id.clone();
        default_lib(c).add(rec)?;
        out["id"] = json!(id);
    }
    Ok(out)
}

// ---------------------------------------------------------------------------------------------
// Piles

/// Keys `sandpile_pile` accepts.
pub(crate) const PILE_KEYS: &[&str] = &[
    "tiling",
    "grains",
    "neighborhood",
    "nb",
    "centre",
    "center",
    "level",
    "png",
    "px",
    "margin",
    "palette",
    "reverse",
    "preview",
    "folder",
    "gpu",
    "steps",
    "label",
];

/// `sandpile_pile(#{ ... })`: one pile (a job of kind `pile`) in its own folder under
/// `<out>/piles/` (or `parent`), or carried on in `folder`. Returns the summary map.
pub(crate) fn sandpile_pile(c: &Arc<Ctx>, m: &Map, parent: Option<&std::path::Path>) -> R<Map> {
    for k in m.keys() {
        if !PILE_KEYS.contains(&k.as_str()) {
            return Err(e(format!(
                "sandpile_pile: unknown key '{k}' (keys: {})",
                PILE_KEYS.join(", ")
            )));
        }
    }
    let v = map_json(m)?;
    let text = |keys: &[&str]| keys.iter().find_map(|k| v.get(*k)).cloned();
    let tiling_id = get_str(&v, "tiling")
        .map_err(e)?
        .unwrap_or("square")
        .to_string();
    let mut spec = json!({ "tiling": tiling_id });
    for (to, from) in [
        ("grains", &["grains"][..]),
        ("nb", &["neighborhood", "nb"][..]),
        ("centre", &["centre", "center"][..]),
        ("level", &["level"][..]),
        ("png", &["png"][..]),
        ("px", &["px"][..]),
        ("margin", &["margin"][..]),
        ("palette", &["palette"][..]),
        ("reverse", &["reverse"][..]),
        ("preview", &["preview"][..]),
        ("folder", &["folder"][..]),
        ("gpu", &["gpu"][..]),
        ("name", &["label"][..]),
    ] {
        if let Some(x) = text(from).filter(|x| !x.is_null()) {
            spec[to] = x;
        }
    }
    if spec.get("grains").is_none() {
        spec["grains"] = json!(10_000);
    }
    if let Some(n) = spec.get("level").and_then(|x| x.as_f64()) {
        spec["level"] = json!(n as u64);
    }
    if let Some(n) = spec.get("preview").and_then(|x| x.as_f64()) {
        spec["preview"] = json!(n.max(0.0) as u64);
    }
    if let Some(p) = parent {
        spec["out"] = json!(p.display().to_string());
    }
    if c.stopped() {
        return Err(e("stopped"));
    }
    let lib = default_lib(c);
    let out = start(c, &lib, "pile", &spec, false).map_err(|x| e(format!("sandpile_pile: {x}")))?;
    let p: jetcarp_search::pile::PileProgress =
        serde_json::from_value(out.progress.clone()).unwrap_or_default();
    if !out.created {
        c.log(format!("resuming {} from step {}", p.folder, p.step));
    }
    if out.status != "done" {
        return Err(e(format!(
            "sandpile_pile stopped in step {}; resume with folder: \"{}\"",
            p.step + 1,
            p.folder
        )));
    }
    let dir = std::path::PathBuf::from(&p.folder);
    if parent.is_none() {
        c.add_out_dir(&dir);
    }
    if p.png.is_some() || p.preview.is_some() {
        c.files_written.fetch_add(
            1 + p.png.is_some() as u64,
            std::sync::atomic::Ordering::Relaxed,
        );
    }
    let meta = out.result.unwrap_or_default();
    let mut o = Map::new();
    let put = |o: &mut Map, k: &str, v: Dynamic| {
        o.insert(k.into(), v);
    };
    let path_or_unit = |s: &Option<String>| {
        s.as_ref()
            .map(|p| Dynamic::from(p.clone()))
            .unwrap_or(Dynamic::UNIT)
    };
    put(&mut o, "folder", p.folder.clone().into());
    put(&mut o, "png", path_or_unit(&p.png));
    put(&mut o, "preview", path_or_unit(&p.preview));
    put(&mut o, "tiling", tiling_id.into());
    put(&mut o, "grains", (p.grains as INT).into());
    put(&mut o, "steps", (p.steps as INT).into());
    put(
        &mut o,
        "topplings",
        i64::try_from(p.topplings).map_or(Dynamic::from(p.topplings as f64), Dynamic::from),
    );
    put(&mut o, "sweeps", (p.sweeps as INT).into());
    put(&mut o, "seconds", p.seconds.into());
    let num = |x: &Value| x.as_f64().unwrap_or(0.0);
    put(&mut o, "width", num(&meta["pile_width"]).into());
    put(&mut o, "height", num(&meta["pile_height"]).into());
    if let (Some(w), Some(h)) = (
        meta["image"]["width"].as_u64(),
        meta["image"]["height"].as_u64(),
    ) {
        put(&mut o, "image_width", (w as INT).into());
        put(&mut o, "image_height", (h as INT).into());
    }
    let backend = p
        .backend
        .clone()
        .or_else(|| {
            meta["records"]
                .as_array()
                .and_then(|r| r.last())
                .and_then(|r| r["backend"].as_str())
                .map(String::from)
        })
        .unwrap_or_default();
    put(&mut o, "backend", backend.into());
    put(&mut o, "job", (out.job as INT).into());
    Ok(o)
}

// ---------------------------------------------------------------------------------------------
// Registration

pub(crate) fn register(eng: &mut Engine, ctx: &Arc<Ctx>) {
    // Each search as a global function (the script's catalog) and, where it always was one, a
    // method of a library handle.
    type F = fn(&Ctx, &Library, &Value) -> Result<Value, String>;
    for (name, f, global) in [
        ("hunt", hunt as F, true),
        ("census", census as F, true),
        ("ants", ants as F, true),
        ("search", search as F, false),
        ("collide", collide as F, false),
    ] {
        let c = ctx.clone();
        eng.register_fn(name, move |l: &mut LibHandle, m: Map| -> R<Map> {
            let v = map_json(&m)?;
            Ok(from_json(&f(&c, &l.0, &v).map_err(e)?).cast::<Map>())
        });
        if global {
            let c = ctx.clone();
            eng.register_fn(name, move |m: Map| -> R<Map> {
                let v = map_json(&m)?;
                Ok(from_json(&f(&c, &default_lib(&c), &v).map_err(e)?).cast::<Map>())
            });
        }
    }
    let c = ctx.clone();
    eng.register_fn("evolve", move |m: Map| -> R<Map> {
        let v = map_json(&m)?;
        Ok(from_json(&evolve(&c, &v).map_err(e)?).cast::<Map>())
    });
    let c = ctx.clone();
    eng.register_fn("track_mover", move |m: Map| -> R<Map> {
        let v = map_json(&m)?;
        Ok(from_json(&track_mover(&c, &v).map_err(e)?).cast::<Map>())
    });
    // Status and stop of hunts and ant searches, and their lists.
    let c = ctx.clone();
    eng.register_fn(
        "hunt_status",
        move |l: &mut LibHandle, job: INT| -> R<Map> {
            Ok(from_json(&status_of::<HuntProgress>(&c, &l.0, job).map_err(e)?).cast::<Map>())
        },
    );
    let c = ctx.clone();
    eng.register_fn("ant_status", move |l: &mut LibHandle, job: INT| -> R<Map> {
        Ok(from_json(&status_of::<AntProgress>(&c, &l.0, job).map_err(e)?).cast::<Map>())
    });
    for name in ["hunt_stop", "ant_stop"] {
        let c = ctx.clone();
        eng.register_fn(name, move |l: &mut LibHandle, job: INT| -> R<()> {
            stop(&c, &l.0, job).map_err(e)
        });
    }
    let c = ctx.clone();
    eng.register_fn("hunts", move |l: &mut LibHandle| -> R<Array> {
        statuses::<HuntProgress>(&c, &l.0, "hunt").map_err(e)
    });
    let c = ctx.clone();
    eng.register_fn("ant_jobs", move |l: &mut LibHandle| -> R<Array> {
        statuses::<AntProgress>(&c, &l.0, "ants").map_err(e)
    });
}
