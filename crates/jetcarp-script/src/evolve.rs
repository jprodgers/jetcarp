//! Rhai builtins for the evolutionary glider search and the mover tracker:
//! `evolve(#{...})` and `track_mover(#{...})`.

use crate::api::{from_json, to_json};
use crate::{Ctx, Progress};
use jetcarp_api::args::{check_keys, get_bool, get_f64, get_str, get_u64};
use jetcarp_core::evolve::catalog::catalog_gliders;
use jetcarp_core::evolve::{EvolveSpec, evolve, write_outputs};
use jetcarp_core::library::movers::{TrackSpec, mover_record, track};
use jetcarp_core::library::{DiscoveredBy, Library, Provenance, now_utc, tiling_from_key};
use jetcarp_core::topology::{Nb, SizeSpec};
use rhai::{Dynamic, Engine, EvalAltResult, Map};
use serde_json::{Value, json};
use std::sync::Arc;

type R<T> = Result<T, Box<EvalAltResult>>;

fn nb_of(v: &Value) -> Result<Nb, String> {
    match get_str(v, "nb")? {
        None | Some("touch") | Some("vertex") => Ok(Nb::Touch),
        Some("edge") => Ok(Nb::Edge),
        Some(o) => Err(format!("nb '{o}' is not touch or edge")),
    }
}

fn library_of(c: &Ctx) -> Library {
    match &c.opts.library {
        Some(p) => Library::open(p.clone()),
        None => Library::open_default(),
    }
}

/// The search: spec keys as `jetcarp evolve`; returns a summary map.
fn run_evolve(c: &Arc<Ctx>, v: &Value) -> Result<Value, String> {
    check_keys(
        v,
        &[
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
        ],
        "evolve",
    )?;
    let d = EvolveSpec::default();
    let tiling_key = get_str(v, "tiling")?.unwrap_or("spectre");
    let factors = match v.get("verify") {
        None | Some(Value::Null) => d.verify_factors.clone(),
        Some(Value::Array(a)) => a
            .iter()
            .map(|x| x.as_u64().map(|x| x as u32).ok_or("verify: whole numbers"))
            .collect::<Result<_, _>>()?,
        Some(_) => return Err("verify is a list of radius multiples, like [1, 2, 4]".into()),
    };
    let spec = EvolveSpec {
        tiling: tiling_from_key(tiling_key)?,
        nb: nb_of(v)?,
        states: get_u64(v, "states")?.unwrap_or(d.states as u64) as u8,
        pop: get_u64(v, "pop")?.unwrap_or(d.pop as u64) as usize,
        gens: get_u64(v, "gens")?.unwrap_or(d.gens as u64) as usize,
        seed: get_u64(v, "seed")?.unwrap_or(d.seed),
        radius: get_u64(v, "radius")?.unwrap_or(d.radius as u64) as u32,
        horizon: get_u64(v, "horizon")?.unwrap_or(d.horizon),
        pop_cap: get_u64(v, "cap")?.unwrap_or(d.pop_cap as u64) as usize,
        champions: get_u64(v, "champions")?.unwrap_or(d.champions as u64) as usize,
        verify_factors: factors,
        stop_on_glider: get_bool(v, "stop_on_glider")?.unwrap_or(false),
        threads: get_u64(v, "threads")?.map(|t| t as usize),
    };
    let label = get_str(v, "label")?
        .map(String::from)
        .unwrap_or_else(|| format!("evolve-{}", tiling_key.replace(':', "-")));
    let dir = c.out_dir(&label)?;
    let gens = spec.gens as u64;
    let t0 = std::time::Instant::now();
    let report = evolve(&spec, &|g| {
        c.progress(Progress {
            label: format!("evolve {tiling_key}"),
            done: g.generation as u64 + 1,
            total: gens,
            item: format!("best {:.2}, at ceiling {}", g.best_ever, g.at_ceiling),
            elapsed: t0.elapsed().as_secs_f64(),
            eta: None,
        });
        !c.stopped()
    })?;
    let lib = get_bool(v, "store")?.unwrap_or(true).then(|| library_of(c));
    let found = catalog_gliders(
        &report,
        lib.as_ref(),
        get_u64(v, "track_gens")?.unwrap_or(20_000),
    )?;
    write_outputs(&report, &dir)?;
    std::fs::write(
        dir.join("movers.json"),
        serde_json::to_string_pretty(&found).unwrap() + "\n",
    )
    .map_err(|e| e.to_string())?;
    let last = report.generations.last();
    Ok(json!({
        "dir": dir.display().to_string(),
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

fn run_track(c: &Arc<Ctx>, v: &Value) -> Result<Value, String> {
    check_keys(
        v,
        &[
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
        ],
        "track_mover",
    )?;
    let family = get_str(v, "tiling")?.unwrap_or("spectre");
    let level = get_u64(v, "level")?.unwrap_or(4) as u32;
    let cells: Vec<(u32, u8)> = match v.get("cells") {
        Some(Value::Array(a)) => a
            .iter()
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
            .collect::<Result<_, &str>>()?,
        _ => return Err("track_mover needs cells: [[cell, state], ...]".into()),
    };
    let patch = jetcarp_core::topology::TilingSpec::new(family).with_size(SizeSpec::Level(level));
    let mut spec = TrackSpec::new(
        get_str(v, "rule")?.unwrap_or("SpectreGlider"),
        nb_of(v)?,
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
    let window =
        get_bool(v, "window")?.unwrap_or(false) || v.get("rings").is_some_and(|x| !x.is_null());
    let rep = if window {
        let d = jetcarp_core::window::track::WindowOpts::default();
        let opts = jetcarp_core::window::track::WindowOpts {
            margin: get_f64(v, "margin")?.unwrap_or(d.margin),
            fixed_levels: get_u64(v, "fixed_levels")?
                .map(|x| x as u32)
                .unwrap_or(d.fixed_levels),
            target_rings: get_u64(v, "rings")?.map(|x| x.min(u32::MAX as u64) as u32),
        };
        if get_u64(v, "gens")?.is_none() && opts.target_rings.is_some() {
            spec.max_gens = opts.target_rings.unwrap_or(0) as u64 * 100 + 20_000;
        }
        jetcarp_core::window::track::track_window(&spec, &opts)?
    } else {
        track(&spec)?
    };
    let mut out = serde_json::to_value(&rep).map_err(|e| e.to_string())?;
    if get_bool(v, "store")?.unwrap_or(false) && rep.verified {
        let prov = Provenance {
            source: match &c.opts.script_path {
                Some(p) => format!("script {}", p.display()),
                None => "script".into(),
            },
            tiling: Some(patch),
            jetcarp_version: jetcarp_core::VERSION.into(),
            date: now_utc(),
            ..Default::default()
        };
        let rec = mover_record(&spec, &rep, prov, DiscoveredBy::Script)?;
        let id = rec.id.clone();
        library_of(c).add(rec)?;
        out["id"] = json!(id);
    }
    Ok(out)
}

pub(crate) fn register(e: &mut Engine, ctx: &Arc<Ctx>) {
    let c = ctx.clone();
    e.register_fn("evolve", move |m: Map| -> R<Map> {
        let v = to_json(&Dynamic::from_map(m))?;
        let r = run_evolve(&c, &v)?;
        Ok(from_json(&r).cast::<Map>())
    });
    let c = ctx.clone();
    e.register_fn("track_mover", move |m: Map| -> R<Map> {
        let v = to_json(&Dynamic::from_map(m))?;
        let r = run_track(&c, &v)?;
        Ok(from_json(&r).cast::<Map>())
    });
}
