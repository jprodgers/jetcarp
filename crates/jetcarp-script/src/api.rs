//! The Rhai bindings: global functions and `World` methods. See `help.md` for the reference.

use crate::describe;
use crate::out;
use crate::series::{self, Item};
use crate::sim::{self, Shared};
use crate::spec::{self, Screen};
use crate::sweep::{self, Verdict};
use crate::{Ctx, Progress};
use jetcarp_core::objects::{self, ObjClass, ObjectOptions};
use jetcarp_core::world::CaChange;
use jetcarp_core::*;
use rhai::{Array, Dynamic, Engine, EvalAltResult, FnPtr, INT, Map, NativeCallContext};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::sync::Arc;

type R<T> = Result<T, Box<EvalAltResult>>;

fn err<T>(e: impl Into<String>) -> R<T> {
    Err(e.into().into())
}

trait OrErr<T> {
    fn or_err(self) -> R<T>;
}
impl<T> OrErr<T> for Result<T, String> {
    fn or_err(self) -> R<T> {
        self.map_err(|e| e.into())
    }
}

/// Rhai value → JSON.
pub fn to_json(d: &Dynamic) -> R<Value> {
    if d.is_unit() {
        return Ok(Value::Null);
    }
    rhai::serde::from_dynamic::<Value>(d)
}

/// JSON → Rhai value.
pub fn from_json(v: &Value) -> Dynamic {
    rhai::serde::to_dynamic(v).unwrap_or(Dynamic::UNIT)
}

fn map_json(m: &Map) -> R<Value> {
    to_json(&Dynamic::from_map(m.clone()))
}

/// `--set` values: integers, floats, true/false, else text.
pub fn param_value(s: &str) -> Value {
    let t = s.trim();
    if let Ok(i) = t.parse::<i64>() {
        json!(i)
    } else if let Ok(f) = t.parse::<f64>() {
        json!(f)
    } else if t == "true" || t == "false" {
        json!(t == "true")
    } else {
        json!(s)
    }
}

pub fn params_map(p: &BTreeMap<String, String>) -> Dynamic {
    let v: serde_json::Map<String, Value> =
        p.iter().map(|(k, v)| (k.clone(), param_value(v))).collect();
    from_json(&Value::Object(v))
}

/// An engine with no JETCARP functions (syntax checks).
pub fn bare_engine() -> Engine {
    let mut e = Engine::new();
    e.set_max_expr_depths(256, 256);
    e
}

fn ca_index(w: &World, id: Option<INT>) -> R<usize> {
    if w.cas.is_empty() {
        return err("this world has no CA yet: call w.add_ca(\"rule\") first");
    }
    match id {
        None => Ok(0),
        Some(id) => w.cas.iter().position(|c| c.id as INT == id).ok_or_else(|| {
            format!(
                "no CA with id {id}; ids: {:?}",
                w.cas.iter().map(|c| c.id).collect::<Vec<_>>()
            )
            .into()
        }),
    }
}

fn lattice_cell(w: &World, x: INT, y: INT) -> R<usize> {
    let g = w
        .topo
        .lattice()
        .ok_or("this tiling has no rows and columns (aperiodic patch): use get_cell/set_cell with an index")?;
    let per = g.ntypes as INT;
    let width = g.w as INT * per;
    if x < 0 || y < 0 || x >= width || y >= g.h as INT {
        return err(format!(
            "cell ({x}, {y}) is outside the grid ({width} x {})",
            g.h
        ));
    }
    Ok(g.index((x / per) as i32, y as i32, (x % per) as u32))
}

fn grid_size(w: &World) -> (INT, INT) {
    match w.topo.lattice() {
        Some(g) => ((g.w * g.ntypes) as INT, g.h as INT),
        None => (w.topo.len() as INT, 1),
    }
}

fn status_map(w: &World) -> Map {
    let (gw, gh) = grid_size(w);
    let pops: Array = w
        .fields
        .iter()
        .map(|f| Dynamic::from(f.population() as INT))
        .collect();
    let period = match w.status() {
        Status::Repeats { period, .. } => Dynamic::from(period as INT),
        _ => Dynamic::UNIT,
    };
    let mut m = Map::new();
    m.insert("generation".into(), (w.generation as INT).into());
    m.insert("hash".into(), w.hash_hex().into());
    m.insert("status".into(), w.status().to_string().into());
    m.insert("done".into(), w.done().into());
    m.insert(
        "done_reason".into(),
        w.done_reason().map(Dynamic::from).unwrap_or(Dynamic::UNIT),
    );
    m.insert("period".into(), period);
    m.insert("population".into(), (total_population(w) as INT).into());
    m.insert("populations".into(), pops.into());
    m.insert("cells".into(), (w.topo.len() as INT).into());
    m.insert("width".into(), gw.into());
    m.insert("height".into(), gh.into());
    m.insert("tiling".into(), w.topo.describe().into());
    m.insert("cas".into(), cas_array(w).into());
    m
}

fn cas_array(w: &World) -> Array {
    w.cas
        .iter()
        .map(|c| {
            let mut m = Map::new();
            m.insert("id".into(), (c.id as INT).into());
            m.insert("name".into(), c.name.clone().into());
            m.insert("rule".into(), c.notation().into());
            m.insert("enabled".into(), c.enabled.into());
            m.insert("field".into(), (c.field as INT).into());
            m.insert("palette".into(), c.palette.clone().into());
            m.insert("steps_per_gen".into(), (c.steps_per_gen as INT).into());
            m.insert(
                "population".into(),
                (w.fields[c.field].population() as INT).into(),
            );
            Dynamic::from_map(m)
        })
        .collect()
}

fn total_population(w: &World) -> u64 {
    let mut f: Vec<usize> = w.cas.iter().map(|c| c.field).collect();
    f.sort_unstable();
    f.dedup();
    f.iter().map(|&i| w.fields[i].population()).sum()
}

fn objects_map(r: &objects::ObjectReport) -> Map {
    let kind = |c: &ObjClass| c.kind().replace(' ', "_");
    let t = r.class_totals();
    let mut totals = Map::new();
    for (k, n) in ["still_life", "oscillator", "spaceship", "other"]
        .iter()
        .zip(t)
    {
        totals.insert((*k).into(), (n as INT).into());
    }
    let counts: Array = r
        .counts
        .iter()
        .map(|c| {
            let mut m = Map::new();
            m.insert("name".into(), c.name.clone().into());
            m.insert("label".into(), c.label().into());
            m.insert("kind".into(), kind(&c.class).into());
            m.insert("count".into(), (c.count as INT).into());
            m.insert(
                "period".into(),
                c.class
                    .period()
                    .map(|p| Dynamic::from(p as INT))
                    .unwrap_or(Dynamic::UNIT),
            );
            m.insert(
                "speed".into(),
                c.class.speed().map(Dynamic::from).unwrap_or(Dynamic::UNIT),
            );
            m.insert("key".into(), c.key.clone().into());
            Dynamic::from_map(m)
        })
        .collect();
    let objs: Array = r
        .objects
        .iter()
        .map(|o| {
            let mut m = Map::new();
            m.insert("id".into(), (o.id as INT).into());
            m.insert("name".into(), o.name.clone().into());
            m.insert("kind".into(), kind(&o.class).into());
            m.insert(
                "period".into(),
                o.class
                    .period()
                    .map(|p| Dynamic::from(p as INT))
                    .unwrap_or(Dynamic::UNIT),
            );
            m.insert(
                "speed".into(),
                o.class.speed().map(Dynamic::from).unwrap_or(Dynamic::UNIT),
            );
            if let ObjClass::Spaceship { dx, dy, .. } = o.class {
                m.insert("dx".into(), (dx as INT).into());
                m.insert("dy".into(), (dy as INT).into());
            }
            if let ObjClass::Other { reason } = &o.class {
                m.insert("reason".into(), reason.clone().into());
            }
            m.insert("size".into(), (o.size as INT).into());
            m.insert("x".into(), o.centroid[0].into());
            m.insert("y".into(), o.centroid[1].into());
            m.insert("key".into(), o.key.clone().into());
            m.insert(
                "cells".into(),
                o.cells
                    .iter()
                    .map(|&c| Dynamic::from(c as INT))
                    .collect::<Array>()
                    .into(),
            );
            Dynamic::from_map(m)
        })
        .collect();
    let mut m = Map::new();
    m.insert("summary".into(), r.summary_line().into());
    m.insert("live_cells".into(), (r.live_cells as INT).into());
    m.insert("totals".into(), totals.into());
    m.insert("counts".into(), counts.into());
    m.insert("objects".into(), objs.into());
    m
}

/// Census-style classification of a continued run.
fn classify(w: &mut World, v: &Value, stop: &dyn Fn() -> bool) -> Result<Map, String> {
    spec::check_keys(v, &["max_gens", "explode", "window"], "classify")?;
    let max = spec::get_u64(v, "max_gens")?.unwrap_or(2000);
    let explode = spec::get_f64(v, "explode")?.unwrap_or(0.5);
    let window = spec::get_u64(v, "window")?
        .unwrap_or(10)
        .clamp(1, max.max(1));
    let n = w.topo.len() as f64;
    let was = w.track_changes();
    w.set_track_changes(true);
    let start = w.generation;
    let mut peak = total_population(w);
    let mut changed = 0u64;
    let mut class = "active";
    let mut period = None;
    for g in 1..=max {
        if g % 64 == 0 && stop() {
            w.set_track_changes(was);
            return Err("cancelled".into());
        }
        w.apply(Command::Step { gens: 1 })?;
        let pop = total_population(w);
        peak = peak.max(pop);
        if g + window > max {
            changed += w.last_changed().unwrap_or(0);
        }
        if pop == 0 && w.cas.iter().all(|c| c.agents.is_empty()) {
            class = "died";
            break;
        }
        if let Status::Repeats { period: p, .. } = w.status() {
            class = if p == 1 { "frozen" } else { "oscillating" };
            period = Some(p);
            break;
        }
        if pop as f64 > explode * n {
            class = "exploded";
            break;
        }
    }
    w.set_track_changes(was);
    let mut m = Map::new();
    m.insert("class".into(), class.into());
    m.insert(
        "period".into(),
        period
            .map(|p| Dynamic::from(p as INT))
            .unwrap_or(Dynamic::UNIT),
    );
    m.insert("generations".into(), ((w.generation - start) as INT).into());
    m.insert("population".into(), (total_population(w) as INT).into());
    m.insert("peak".into(), (peak as INT).into());
    if class == "active" {
        m.insert(
            "activity".into(),
            (changed as f64 / (window as f64 * n.max(1.0))).into(),
        );
    }
    Ok(m)
}

fn outcome_map(o: &sim::RunOutcome, w: &World) -> Map {
    let mut m = Map::new();
    m.insert("generations".into(), (o.generations as INT).into());
    m.insert("generation".into(), (w.generation as INT).into());
    m.insert("stopped".into(), o.stopped.into());
    m.insert("done".into(), o.done.into());
    m.insert(
        "done_reason".into(),
        o.done_reason
            .clone()
            .map(Dynamic::from)
            .unwrap_or(Dynamic::UNIT),
    );
    m
}

fn new_world(ctx: &Ctx, v: &Value) -> R<Shared> {
    let mut keys: Vec<&str> = spec::TILING_KEYS.to_vec();
    keys.extend(["rng_seed", "screen", "cell_px", "cas"]);
    spec::check_keys(v, &keys, "world").or_err()?;
    let mut screen = ctx.opts.screen;
    if let Some(s) = spec::get_str(v, "screen").or_err()? {
        let (w, h) = Screen::parse_size(s).or_err()?;
        (screen.width, screen.height) = (w, h);
    }
    if let Some(px) = spec::get_f64(v, "cell_px").or_err()? {
        screen.cell_px = px.clamp(0.5, 4096.0);
    }
    let tiling = spec::tiling_spec(v, &screen).or_err()?;
    ctx.check_tiling(&tiling).or_err()?;
    let rng = spec::get_u64(v, "rng_seed").or_err()?.unwrap_or(1);
    let mut w = World::new(tiling, rng).or_err()?;
    ctx.check_cells(w.topo.len()).or_err()?;
    if let Some(cas) = spec::get(v, "cas") {
        for c in cas
            .as_array()
            .ok_or("`cas` should be a list of rules or [rule, options] pairs")?
        {
            let spec = match c {
                Value::String(r) => spec::ca_spec(r, &Value::Null),
                Value::Object(o) => {
                    let rule = o
                        .get("rule")
                        .and_then(|r| r.as_str())
                        .ok_or("a CA map needs `rule`")?;
                    let mut rest = o.clone();
                    rest.remove("rule");
                    spec::check_keys(&Value::Object(rest.clone()), spec::CA_KEYS, "CA").or_err()?;
                    spec::ca_spec(rule, &Value::Object(rest))
                }
                x => Err(format!("bad CA {x}")),
            }
            .or_err()?;
            w.apply(Command::AddCa { ca: spec }).or_err()?;
        }
    }
    Ok(Shared::new(w, screen))
}

fn image_args(ctx: &Ctx, s: &Shared, path: &str, opts: &Value) -> R<Map> {
    let p = ctx.writable(path).or_err()?;
    let mut g = s.lock();
    let screen = g.screen;
    let spec = sim::image_spec(opts, &screen).or_err()?;
    sim::apply_palette(&mut g.world, &spec).or_err()?;
    let (w, h, bytes) = sim::save_png(&g.world, &spec, &p).or_err()?;
    ctx.files_written
        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let mut m = Map::new();
    m.insert("path".into(), p.display().to_string().into());
    m.insert("width".into(), (w as INT).into());
    m.insert("height".into(), (h as INT).into());
    m.insert("bytes".into(), (bytes as INT).into());
    Ok(m)
}

/// The full engine for a run.
pub fn engine(ctx: Arc<Ctx>) -> Engine {
    let mut e = bare_engine();
    if let Some(n) = ctx.opts.limits.max_operations {
        e.set_max_operations(n);
    }
    {
        let c = ctx.clone();
        e.on_print(move |s| c.log(s.to_string()));
        let c = ctx.clone();
        e.on_debug(move |s, src, pos| {
            let at = pos
                .line()
                .map(|l| format!(" (line {l})"))
                .unwrap_or_default();
            let _ = src;
            c.log(format!("[debug]{at} {s}"));
        });
        let c = ctx.clone();
        e.on_progress(move |_| c.stopped().then(|| Dynamic::from("stopped")));
    }
    e.register_type_with_name::<Shared>("World");
    register_globals(&mut e, &ctx);
    register_world(&mut e, &ctx);
    register_library(&mut e, &ctx);
    crate::evolve::register(&mut e, &ctx);
    crate::hl::register(&mut e, &ctx);
    crate::pile::register(&mut e, &ctx);
    crate::ants::register(&mut e, &ctx);
    crate::media::register(&mut e, &ctx);
    e
}

fn register_globals(e: &mut Engine, ctx: &Arc<Ctx>) {
    // Worlds.
    let c = ctx.clone();
    e.register_fn("world", move || new_world(&c, &Value::Null));
    let c = ctx.clone();
    e.register_fn("world", move |m: Map| new_world(&c, &map_json(&m)?));
    let c = ctx.clone();
    e.register_fn("load_scene", move |path: &str| -> R<Shared> {
        let p = c.readable(path).or_err()?;
        let text = std::fs::read_to_string(&p).map_err(|e| format!("{}: {e}", p.display()))?;
        let sc = Scene::from_json(&text).or_err()?;
        let w = World::from_scene(&sc).or_err()?;
        c.check_cells(w.topo.len()).or_err()?;
        Ok(Shared::new(w, c.opts.screen))
    });
    let c = ctx.clone();
    e.register_fn("scene_world", move |json_text: &str| -> R<Shared> {
        let sc = Scene::from_json(json_text).or_err()?;
        let w = World::from_scene(&sc).or_err()?;
        c.check_cells(w.topo.len()).or_err()?;
        Ok(Shared::new(w, c.opts.screen))
    });

    // Output.
    let c = ctx.clone();
    e.register_fn("out_dir", move |label: &str| -> R<String> {
        Ok(c.out_dir(label).or_err()?.display().to_string())
    });
    let c = ctx.clone();
    e.register_fn("out_dir", move || -> R<String> {
        Ok(c.out_dir("script").or_err()?.display().to_string())
    });
    let c = ctx.clone();
    e.register_fn("out_root", move || c.root.display().to_string());
    let c = ctx.clone();
    e.register_fn("write_text", move |path: &str, text: &str| -> R<String> {
        let p = c.writable(path).or_err()?;
        out::write_bytes(&p, text.as_bytes()).or_err()?;
        c.files_written
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        Ok(p.display().to_string())
    });
    let c = ctx.clone();
    e.register_fn("append_text", move |path: &str, text: &str| -> R<String> {
        use std::io::Write;
        let p = c.writable(path).or_err()?;
        let mut f = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&p)
            .map_err(|e| format!("{}: {e}", p.display()))?;
        f.write_all(text.as_bytes()).map_err(|e| e.to_string())?;
        Ok(p.display().to_string())
    });
    let c = ctx.clone();
    e.register_fn("write_json", move |path: &str, v: Dynamic| -> R<String> {
        let p = c.writable(path).or_err()?;
        let text = serde_json::to_string_pretty(&to_json(&v)?).map_err(|e| e.to_string())?;
        out::write_bytes(&p, text.as_bytes()).or_err()?;
        c.files_written
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        Ok(p.display().to_string())
    });
    let c = ctx.clone();
    e.register_fn("write_csv", move |path: &str, rows: Array| -> R<String> {
        let p = c.writable(path).or_err()?;
        let rows: Vec<Value> = rows.iter().map(to_json).collect::<R<_>>()?;
        if let Some(bad) = rows.iter().find(|r| !r.is_object()) {
            return err(format!(
                "write_csv needs a list of maps (one per row), got {bad}"
            ));
        }
        out::write_bytes(&p, out::to_csv(&rows, &[]).as_bytes()).or_err()?;
        c.files_written
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        Ok(p.display().to_string())
    });
    let c = ctx.clone();
    e.register_fn("read_text", move |path: &str| -> R<String> {
        let p = c.readable(path).or_err()?;
        std::fs::read_to_string(&p).map_err(|e| format!("{}: {e}", p.display()).into())
    });
    let c = ctx.clone();
    // A sweep folder's manifest rows (finished or still running), sorted by index.
    e.register_fn("manifest", move |folder: &str| -> R<Array> {
        let p = c.readable(folder).or_err()?;
        let m = crate::results::Manifest::load(&p).or_err()?;
        Ok(m.rows
            .iter()
            .map(|r| from_json(&Value::Object(r.clone())))
            .collect())
    });
    let c = ctx.clone();
    e.register_fn("read_json", move |path: &str| -> R<Dynamic> {
        let p = c.readable(path).or_err()?;
        let t = std::fs::read_to_string(&p).map_err(|e| format!("{}: {e}", p.display()))?;
        let v: Value = serde_json::from_str(&t).map_err(|e| format!("{}: {e}", p.display()))?;
        Ok(from_json(&v))
    });
    let c = ctx.clone();
    e.register_fn("exists", move |path: &str| {
        c.readable(path).map(|p| p.exists()).unwrap_or(false)
    });
    e.register_fn("join", |a: &str, b: &str| {
        std::path::Path::new(a).join(b).display().to_string()
    });
    e.register_fn("to_json", |v: Dynamic| -> R<String> {
        serde_json::to_string(&to_json(&v)?).map_err(|e| e.to_string().into())
    });
    e.register_fn("parse_json", |t: &str| -> R<Dynamic> {
        let v: Value = serde_json::from_str(t).map_err(|e| format!("parse_json: {e}"))?;
        Ok(from_json(&v))
    });

    // Logging, progress, time.
    let c = ctx.clone();
    e.register_fn("log", move |v: Dynamic| c.log(v.to_string()));
    let c = ctx.clone();
    e.register_fn("progress", move |done: INT, total: INT| {
        c.progress(Progress {
            label: "script".into(),
            done: done.max(0) as u64,
            total: total.max(0) as u64,
            item: String::new(),
            elapsed: c.elapsed(),
            eta: None,
        })
    });
    let c = ctx.clone();
    e.register_fn("progress", move |done: INT, total: INT, label: &str| {
        let el = c.elapsed();
        c.progress(Progress {
            label: "script".into(),
            done: done.max(0) as u64,
            total: total.max(0) as u64,
            item: label.into(),
            elapsed: el,
            eta: (done > 0).then(|| el / done as f64 * (total - done).max(0) as f64),
        })
    });
    e.register_fn("now", out::stamp);
    e.register_fn("timestamp", crate::unix_time);
    let c = ctx.clone();
    e.register_fn("elapsed", move || c.elapsed());
    let c = ctx.clone();
    e.register_fn("stopped", move || c.stopped());
    let c = ctx.clone();
    e.register_fn("screen", move || -> Map {
        let s = c.opts.screen;
        let mut m = Map::new();
        m.insert("width".into(), (s.width as INT).into());
        m.insert("height".into(), (s.height as INT).into());
        m.insert("cell_px".into(), s.cell_px.into());
        m
    });
    e.register_fn("jetcarp_version", || jetcarp_core::VERSION.to_string());

    // Catalogs.
    e.register_fn("tilings", || -> Array {
        families::FAMILIES
            .iter()
            .map(|f| Dynamic::from(f.id.to_string()))
            .collect()
    });
    e.register_fn("catalog_tilings", || -> Array {
        catalog::all()
            .iter()
            .map(|t| Dynamic::from(t.id.clone()))
            .collect()
    });
    e.register_fn("catalog_tilings", |k: INT| -> Array {
        catalog::all()
            .iter()
            .filter(|t| t.k as INT == k)
            .map(|t| Dynamic::from(t.id.clone()))
            .collect()
    });
    e.register_fn("isohedral_tilings", || -> Array {
        jetcarp_core::isohedral::IH_TYPES
            .iter()
            .map(|t| Dynamic::from(format!("ih{:02}", t.num)))
            .collect()
    });
    e.register_fn("named_rules", || -> Array {
        rules::catalog()
            .iter()
            .map(|r| {
                let mut m = Map::new();
                m.insert("name".into(), r.name.into());
                m.insert("rule".into(), r.notation.into());
                m.insert("family".into(), r.family.into());
                Dynamic::from_map(m)
            })
            .collect()
    });
    e.register_fn("series", |rules: Dynamic| -> R<Array> {
        series_array(&json!({"rules": to_json(&rules)?}))
    });
    e.register_fn("series", |rules: &str, from: INT, to: INT| -> R<Array> {
        series_array(&json!({"rules": rules, "from": from.max(0), "to": to.max(0)}))
    });

    // Sweeps.
    let c = ctx.clone();
    e.register_fn("sweep", move |nc: NativeCallContext, m: Map| -> R<Map> {
        let mut m = m;
        let keep = match m.remove("keep") {
            None => None,
            Some(k) if k.is_fnptr() => Some(k.cast::<FnPtr>()),
            Some(k) if k.is_unit() => None,
            Some(k) => {
                return err(format!(
                    "sweep `keep` should be a function like |w, n| w.population() > 0, got {}",
                    k.type_name()
                ));
            }
        };
        let mut v = map_json(&m)?;
        if keep.is_some() {
            v["keep"] = json!(true);
        }
        let ast = c
            .ast
            .lock()
            .unwrap()
            .clone()
            .ok_or("no script is running")?;
        let engine = nc.engine();
        let call = |w: &Shared, item: &Item| -> Result<Verdict, String> {
            let fp = keep.as_ref().unwrap();
            let n = Dynamic::from(item.n.unwrap_or(item.index as u64) as INT);
            let r = match fp.call::<Dynamic>(engine, &ast, (w.clone(), n)) {
                Err(e) if matches!(*e, EvalAltResult::ErrorFunctionNotFound(..)) => {
                    fp.call::<Dynamic>(engine, &ast, (w.clone(),))
                }
                r => r,
            }
            .map_err(|e| format!("in keep: {e}"))?;
            verdict(r)
        };
        let cb: Option<&sweep::Callback> = if keep.is_some() { Some(&call) } else { None };
        let summary = sweep::run_sweep(&c, &v, cb).or_err()?;
        Ok(from_json(&summary).cast::<Map>())
    });
    // The host's live view.
    let c = ctx.clone();
    let show = move |s: &Shared, label: &str| -> R<()> {
        let g = s.lock();
        let what = format!(
            "{}, generation {}, hash {}",
            if label.is_empty() {
                g.world.topo.describe()
            } else {
                label.to_string()
            },
            g.world.generation,
            &g.world.hash_hex()[..8]
        );
        if !c.opts.live_view {
            c.log(format!("show: {what} (no live view here)"));
            return Ok(());
        }
        let w = World::from_scene(&g.world.to_scene()).or_err()?;
        c.log(format!("show: {what}"));
        c.show(crate::Shown::new(label.to_string(), w));
        Ok(())
    };
    let s1 = show.clone();
    e.register_fn("show", move |s: Shared| s1(&s, ""));
    e.register_fn("show", move |s: Shared, label: &str| show(&s, label));
    e.register_fn("sweep_script", |m: Map| -> R<String> {
        Ok(sweep::sweep_to_script(&map_json(&m)?))
    });
}

fn verdict(r: Dynamic) -> Result<Verdict, String> {
    if r.is_unit() {
        return Ok(Verdict {
            keep: true,
            extra: Default::default(),
        });
    }
    if let Some(b) = r.clone().try_cast::<bool>() {
        return Ok(Verdict {
            keep: b,
            extra: Default::default(),
        });
    }
    if r.is_map() {
        let v = to_json(&r).map_err(|e| e.to_string())?;
        let mut extra = v.as_object().cloned().unwrap_or_default();
        let keep = match extra.remove("keep") {
            None | Some(Value::Null) => true,
            Some(Value::Bool(b)) => b,
            Some(x) => return Err(format!("keep: should be true or false, got {x}")),
        };
        return Ok(Verdict { keep, extra });
    }
    Err(format!(
        "the keep function should return true/false or a map of columns, got {}",
        r.type_name()
    ))
}

fn series_array(v: &Value) -> R<Array> {
    Ok(series::expand(v)
        .or_err()?
        .into_iter()
        .map(|i| Dynamic::from(i.rule))
        .collect())
}

fn register_world(e: &mut Engine, ctx: &Arc<Ctx>) {
    e.register_fn("to_string", |s: &mut Shared| -> String {
        let g = s.lock();
        format!(
            "World({}, generation {})",
            g.world.topo.describe(),
            g.world.generation
        )
    });
    e.register_fn("to_debug", |s: &mut Shared| -> String {
        let g = s.lock();
        format!(
            "World({}, generation {})",
            g.world.topo.describe(),
            g.world.generation
        )
    });

    // CAs.
    let add = |s: &mut Shared, rule: &str, opts: &Value| -> R<INT> {
        spec::check_keys(opts, spec::CA_KEYS, "add_ca").or_err()?;
        let spec = spec::ca_spec(rule, opts).or_err()?;
        let mut g = s.lock();
        let a = g.world.apply(Command::AddCa { ca: spec }).or_err()?;
        Ok(a.new_id.unwrap_or(0) as INT)
    };
    e.register_fn("add_ca", move |s: &mut Shared, rule: &str| {
        add(s, rule, &Value::Null)
    });
    e.register_fn("add_ca", move |s: &mut Shared, rule: &str, m: Map| {
        add(s, rule, &map_json(&m)?)
    });
    e.register_fn("update_ca", |s: &mut Shared, id: INT, m: Map| -> R<()> {
        let v = map_json(&m)?;
        let mut keys = spec::CA_KEYS.to_vec();
        keys.push("rule");
        spec::check_keys(&v, &keys, "update_ca").or_err()?;
        let mut g = s.lock();
        let i = ca_index(&g.world, Some(id))?;
        let tmp =
            spec::ca_spec(spec::get_str(&v, "rule").or_err()?.unwrap_or("B3/S23"), &v).or_err()?;
        let change = CaChange {
            name: tmp.name,
            rule: spec::get_str(&v, "rule").or_err()?.map(spec::resolve_rule),
            neighborhood: tmp.neighborhood,
            enabled: spec::get_bool(&v, "enabled").or_err()?,
            affects_others: tmp.affects_others,
            steps_per_gen: spec::get(&v, "steps_per_gen").map(|_| tmp.steps_per_gen),
            palette: tmp.palette,
            drop_per_gen: spec::get(&v, "drop_per_gen").map(|_| tmp.drop_per_gen),
            seed: tmp.seed,
            color_by: spec::get(&v, "color_by").map(|_| tmp.color_by),
            agent_edge: spec::get(&v, "agent_edge").map(|_| tmp.agent_edge),
            palette_neg: tmp.palette_neg,
            glow: spec::get(&v, "glow").map(|_| tmp.glow),
        };
        let id = g.world.cas[i].id;
        g.world.apply(Command::UpdateCa { id, change }).or_err()?;
        Ok(())
    });
    e.register_fn("remove_ca", |s: &mut Shared, id: INT| -> R<()> {
        let mut g = s.lock();
        let i = ca_index(&g.world, Some(id))?;
        let id = g.world.cas[i].id;
        g.world.apply(Command::RemoveCa { id }).or_err().map(|_| ())
    });
    let seed = |s: &mut Shared, id: Option<INT>, seed: Dynamic| -> R<()> {
        let spec = spec::parse_seed(&to_json(&seed)?).or_err()?;
        let mut g = s.lock();
        let i = ca_index(&g.world, id)?;
        let id = g.world.cas[i].id;
        g.world
            .apply(Command::Seed { id, seed: spec })
            .or_err()
            .map(|_| ())
    };
    e.register_fn("seed", move |s: &mut Shared, v: Dynamic| seed(s, None, v));
    e.register_fn("seed", move |s: &mut Shared, id: INT, v: Dynamic| {
        seed(s, Some(id), v)
    });
    let clear = |s: &mut Shared, id: Option<INT>| -> R<()> {
        let mut g = s.lock();
        let i = ca_index(&g.world, id)?;
        let id = g.world.cas[i].id;
        g.world.apply(Command::Clear { id }).or_err().map(|_| ())
    };
    e.register_fn("clear", move |s: &mut Shared| clear(s, None));
    e.register_fn("clear", move |s: &mut Shared, id: INT| clear(s, Some(id)));
    e.register_fn("reset", |s: &mut Shared| -> R<()> {
        s.lock().world.apply(Command::Reset).or_err().map(|_| ())
    });
    e.register_fn("apply", |s: &mut Shared, cmd: Dynamic| -> R<()> {
        let v = match cmd.clone().try_cast::<String>() {
            Some(t) => serde_json::from_str(&t).map_err(|e| format!("apply: {e}"))?,
            None => to_json(&cmd)?,
        };
        let cmd: Command =
            serde_json::from_value(v).map_err(|e| format!("apply: bad command: {e}"))?;
        s.lock().world.apply(cmd).or_err().map(|_| ())
    });
    // Reseeding (see README "Reseeding"): now, automatic policies, and their status.
    let reseed = |s: &mut Shared, mode: Dynamic, ca: Option<INT>| -> R<()> {
        let v = match mode.clone().try_cast::<String>() {
            Some(t) => jetcarp_core::reseed::mode_json(&t),
            None => to_json(&mode)?,
        };
        let mode: jetcarp_core::reseed::ReseedMode =
            serde_json::from_value(v).map_err(|e| format!("reseed: bad mode: {e}"))?;
        let cmd = Command::Reseed {
            ca: ca.map(|c| c as u32),
            mode,
        };
        s.lock().world.apply(cmd).or_err().map(|_| ())
    };
    e.register_fn("reseed", move |s: &mut Shared, mode: Dynamic| {
        reseed(s, mode, None)
    });
    e.register_fn("reseed", move |s: &mut Shared, id: INT, mode: Dynamic| {
        reseed(s, mode, Some(id))
    });
    e.register_fn("set_reseed", |s: &mut Shared, list: Array| -> R<()> {
        let v = to_json(&Dynamic::from_array(list))?;
        let policies: Vec<jetcarp_core::reseed::AutoReseed> =
            serde_json::from_value(v).map_err(|e| format!("set_reseed: bad policy: {e}"))?;
        s.lock()
            .world
            .apply(Command::SetReseed { policies })
            .or_err()
            .map(|_| ())
    });
    e.register_fn("reseed_status", |s: &mut Shared| -> Dynamic {
        let g = s.lock();
        let w = &g.world;
        from_json(&serde_json::json!({
            "policies": w.reseed_status(),
            "activity": w.last_activity(),
            "history": w.activity_history().iter().map(|a| a.fraction as f64).collect::<Vec<_>>(),
        }))
    });

    // Running.
    let c = ctx.clone();
    e.register_fn("step", move |s: &mut Shared| -> R<()> {
        if c.stopped() {
            return err("stopped");
        }
        s.lock()
            .world
            .apply(Command::Step { gens: 1 })
            .or_err()
            .map(|_| ())
    });
    let c = ctx.clone();
    e.register_fn("step", move |s: &mut Shared, n: INT| -> R<()> {
        let u = spec::Until {
            trigger: jetcarp_core::script::Trigger::Generations { n: n.max(0) as u64 },
            max_gens: n.max(0) as u64,
        };
        let o = sim::run_until(&mut s.lock().world, &u, &|| c.stopped());
        if o.stopped == "cancelled" {
            err("stopped")
        } else {
            Ok(())
        }
    });
    let c = ctx.clone();
    e.register_fn("run", move |s: &mut Shared, m: Map| -> R<Map> {
        let v = map_json(&m)?;
        spec::check_keys(&v, spec::UNTIL_KEYS, "run").or_err()?;
        let u = spec::parse_until(&v, 10_000).or_err()?;
        let mut g = s.lock();
        let o = sim::run_until(&mut g.world, &u, &|| c.stopped());
        if o.stopped == "cancelled" {
            return err("stopped");
        }
        Ok(outcome_map(&o, &g.world))
    });
    e.register_fn("track_changes", |s: &mut Shared, on: bool| {
        s.lock().world.set_track_changes(on)
    });

    // Reading.
    e.register_fn("generation", |s: &mut Shared| {
        s.lock().world.generation as INT
    });
    e.register_fn("hash", |s: &mut Shared| s.lock().world.hash_hex());
    e.register_fn("population", |s: &mut Shared| {
        total_population(&s.lock().world) as INT
    });
    e.register_fn("population", |s: &mut Shared, id: INT| -> R<INT> {
        let g = s.lock();
        let i = ca_index(&g.world, Some(id))?;
        Ok(g.world.fields[g.world.cas[i].field].population() as INT)
    });
    e.register_fn("done", |s: &mut Shared| s.lock().world.done());
    e.register_fn("done_reason", |s: &mut Shared| -> Dynamic {
        s.lock()
            .world
            .done_reason()
            .map(Dynamic::from)
            .unwrap_or(Dynamic::UNIT)
    });
    e.register_fn("status", |s: &mut Shared| status_map(&s.lock().world));
    e.register_fn("period", |s: &mut Shared| -> Dynamic {
        match s.lock().world.status() {
            Status::Repeats { period, .. } => Dynamic::from(period as INT),
            _ => Dynamic::UNIT,
        }
    });
    e.register_fn("cells", |s: &mut Shared| s.lock().world.topo.len() as INT);
    e.register_fn("width", |s: &mut Shared| grid_size(&s.lock().world).0);
    e.register_fn("height", |s: &mut Shared| grid_size(&s.lock().world).1);
    e.register_fn("tiling", |s: &mut Shared| s.lock().world.topo.describe());
    e.register_fn("rule", |s: &mut Shared| -> Dynamic {
        s.lock()
            .world
            .cas
            .first()
            .map(|c| Dynamic::from(c.notation()))
            .unwrap_or(Dynamic::UNIT)
    });
    e.register_fn("cas", |s: &mut Shared| cas_array(&s.lock().world));
    let get = |s: &mut Shared, x: INT, y: INT, id: Option<INT>| -> R<INT> {
        let g = s.lock();
        let i = ca_index(&g.world, id)?;
        let c = lattice_cell(&g.world, x, y)?;
        let f = &g.world.fields[g.world.cas[i].field];
        Ok(if f.height.is_empty() {
            f.state[c] as INT
        } else {
            f.height[c] as INT
        })
    };
    e.register_fn("get", move |s: &mut Shared, x: INT, y: INT| {
        get(s, x, y, None)
    });
    e.register_fn("get", move |s: &mut Shared, x: INT, y: INT, id: INT| {
        get(s, x, y, Some(id))
    });
    let set_cells = |s: &mut Shared, cells: Vec<[u32; 2]>, id: Option<INT>| -> R<()> {
        let mut g = s.lock();
        let i = ca_index(&g.world, id)?;
        let id = g.world.cas[i].id;
        g.world
            .apply(Command::SetCells { id, cells })
            .or_err()
            .map(|_| ())
    };
    e.register_fn(
        "set",
        move |s: &mut Shared, x: INT, y: INT, v: INT| -> R<()> {
            let c = lattice_cell(&s.lock().world, x, y)?;
            set_cells(s, vec![[c as u32, v.max(0) as u32]], None)
        },
    );
    e.register_fn(
        "set",
        move |s: &mut Shared, x: INT, y: INT, v: INT, id: INT| -> R<()> {
            let c = lattice_cell(&s.lock().world, x, y)?;
            set_cells(s, vec![[c as u32, v.max(0) as u32]], Some(id))
        },
    );
    e.register_fn("get_cell", |s: &mut Shared, i: INT| -> R<INT> {
        let g = s.lock();
        let ci = ca_index(&g.world, None)?;
        let f = &g.world.fields[g.world.cas[ci].field];
        let i = usize::try_from(i)
            .ok()
            .filter(|&i| i < g.world.topo.len())
            .ok_or("cell index out of range")?;
        Ok(if f.height.is_empty() {
            f.state[i] as INT
        } else {
            f.height[i] as INT
        })
    });
    e.register_fn("set_cell", move |s: &mut Shared, i: INT, v: INT| -> R<()> {
        set_cells(s, vec![[i.max(0) as u32, v.max(0) as u32]], None)
    });
    e.register_fn("set_cells", move |s: &mut Shared, list: Array| -> R<()> {
        let mut cells = Vec::with_capacity(list.len());
        for item in &list {
            let a = item
                .clone()
                .try_cast::<Array>()
                .ok_or("set_cells needs [[x, y, state], ...] or [[x, y], ...]")?;
            let n: Vec<INT> = a
                .iter()
                .map(|d| d.as_int().map_err(|_| "cell coordinates must be integers"))
                .collect::<Result<_, _>>()?;
            let (x, y, v) = match n.as_slice() {
                [x, y] => (*x, *y, 1),
                [x, y, v] => (*x, *y, *v),
                _ => return err("set_cells items are [x, y] or [x, y, state]"),
            };
            let c = lattice_cell(&s.lock().world, x, y)?;
            cells.push([c as u32, v.max(0) as u32]);
        }
        set_cells(s, cells, None)
    });
    e.register_fn(
        "pattern",
        move |s: &mut Shared, text: &str, x: INT, y: INT| -> R<()> {
            let mut cells = vec![];
            for (dy, line) in text.lines().enumerate() {
                for (dx, ch) in line.chars().enumerate() {
                    let v = match ch {
                        'O' | 'o' | '*' | '#' | 'X' => 1,
                        d @ '1'..='9' => d as u32 - '0' as u32,
                        _ => continue,
                    };
                    let c = lattice_cell(&s.lock().world, x + dx as INT, y + dy as INT)?;
                    cells.push([c as u32, v]);
                }
            }
            set_cells(s, cells, None)
        },
    );
    e.register_fn("row", |s: &mut Shared, y: INT| -> R<Array> {
        let g = s.lock();
        let (gw, _) = grid_size(&g.world);
        let ci = ca_index(&g.world, None)?;
        let f = &g.world.fields[g.world.cas[ci].field];
        (0..gw)
            .map(|x| {
                let c = lattice_cell(&g.world, x, y)?;
                Ok(Dynamic::from(if f.height.is_empty() {
                    f.state[c] as INT
                } else {
                    f.height[c] as INT
                }))
            })
            .collect()
    });

    // Analysis.
    let objs = |s: &mut Shared, v: &Value| -> R<Map> {
        spec::check_keys(
            v,
            &[
                "ca",
                "max_period",
                "merge_distance",
                "separate",
                "piece_distance",
            ],
            "objects",
        )
        .or_err()?;
        let g = s.lock();
        let d = ObjectOptions::default();
        let o = ObjectOptions {
            ca: spec::get_u64(v, "ca").or_err()?.map(|c| c as u32),
            max_period: spec::get_u64(v, "max_period")
                .or_err()?
                .unwrap_or(d.max_period),
            merge_distance: spec::get_u64(v, "merge_distance")
                .or_err()?
                .map_or(d.merge_distance, |m| m as u32),
            separate: spec::get_bool(v, "separate")
                .or_err()?
                .unwrap_or(d.separate),
            piece_distance: spec::get_u64(v, "piece_distance")
                .or_err()?
                .map_or(d.piece_distance, |m| m as u32),
        };
        let r = objects::find_objects(&g.world, &o).or_err()?;
        Ok(objects_map(&r))
    };
    e.register_fn("objects", move |s: &mut Shared| objs(s, &Value::Null));
    e.register_fn("objects", move |s: &mut Shared, m: Map| {
        objs(s, &map_json(&m)?)
    });
    let c = ctx.clone();
    e.register_fn("classify", move |s: &mut Shared| -> R<Map> {
        classify(&mut s.lock().world, &Value::Null, &|| c.stopped()).or_err()
    });
    let c = ctx.clone();
    e.register_fn("classify", move |s: &mut Shared, m: Map| -> R<Map> {
        let v = map_json(&m)?;
        classify(&mut s.lock().world, &v, &|| c.stopped()).or_err()
    });
    e.register_fn("describe", |s: &mut Shared| -> Map {
        let v = Value::Object(describe::describe(&s.lock().world, None));
        from_json(&v).cast::<Map>()
    });

    // Saving.
    let c = ctx.clone();
    e.register_fn("save_png", move |s: &mut Shared, path: &str| {
        image_args(&c, s, path, &Value::Null)
    });
    let c = ctx.clone();
    e.register_fn("save_png", move |s: &mut Shared, path: &str, m: Map| {
        image_args(&c, s, path, &map_json(&m)?)
    });
    let c = ctx.clone();
    e.register_fn(
        "save_scene",
        move |s: &mut Shared, path: &str| -> R<String> {
            let p = c.writable(path).or_err()?;
            out::write_bytes(&p, s.lock().world.to_scene().to_json().as_bytes()).or_err()?;
            c.files_written
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            Ok(p.display().to_string())
        },
    );
    e.register_fn("scene", |s: &mut Shared| {
        s.lock().world.to_scene().to_json()
    });
    e.register_fn("copy", |s: &mut Shared| -> R<Shared> {
        let g = s.lock();
        let mut w = World::from_scene(&g.world.to_scene()).or_err()?;
        w.set_track_changes(g.world.track_changes());
        Ok(Shared::new(w, g.screen))
    });
}

// ---------------------------------------------------------------------------------------------
// The pattern library

/// A pattern catalog (SQLite file), as scripts see it.
#[derive(Clone)]
pub struct LibHandle(pub jetcarp_core::library::Library);

fn lib_records(v: Vec<jetcarp_core::library::PatternRecord>) -> Array {
    v.iter()
        .map(|r| from_json(&crate::library::record_json(r)))
        .collect()
}

fn register_library(e: &mut Engine, ctx: &Arc<Ctx>) {
    use jetcarp_core::library::{DiscoveredBy, Library, Query, Update, promote::promote};
    e.register_type_with_name::<LibHandle>("Library");
    let c = ctx.clone();
    e.register_fn("library", move || {
        LibHandle(match &c.opts.library {
            Some(p) => Library::open(p.clone()),
            None => Library::open_default(),
        })
    });
    let c = ctx.clone();
    e.register_fn("library", move |path: &str| -> R<LibHandle> {
        // Another catalog (a folder holding catalog.sqlite, or a .sqlite file): it must be
        // somewhere the script may write to.
        let file = Library::file_for(std::path::PathBuf::from(path));
        let dir = file
            .parent()
            .map(|d| d.display().to_string())
            .filter(|d| !d.is_empty())
            .unwrap_or_else(|| ".".into());
        let d = c.writable_dir(&dir).or_err()?;
        Ok(LibHandle(Library::open(
            d.join(file.file_name().unwrap_or_default()),
        )))
    });
    e.register_fn("to_string", |l: &mut LibHandle| {
        format!("Library({})", l.0.path().display())
    });
    e.register_fn("path", |l: &mut LibHandle| l.0.path().display().to_string());
    e.register_fn("info", |l: &mut LibHandle| -> R<Map> {
        let i = l.0.info().or_err()?;
        Ok(from_json(&serde_json::to_value(i).unwrap_or_default()).cast::<Map>())
    });
    e.register_fn("works_in", |l: &mut LibHandle, rule: &str| -> R<Array> {
        let q = Query {
            works_in: Some(rule.to_string()),
            ..Default::default()
        };
        Ok(lib_records(l.0.query(&q).or_err()?))
    });
    e.register_fn("sightings", |l: &mut LibHandle, key: &str| -> R<Array> {
        let id = l.0.get(key).or_err()?.id;
        let v = l.0.sightings(&id).or_err()?;
        Ok(v.iter()
            .map(|s| from_json(&serde_json::to_value(s).unwrap_or_default()))
            .collect())
    });
    // Quick Start entries ("My finds"): all, or with one status.
    let qs = |l: &LibHandle, status: Option<&str>| -> R<Array> {
        let st = match status {
            Some(s) => vec![jetcarp_core::library::QsStatus::parse(s).or_err()?],
            None => vec![],
        };
        Ok(l.0
            .quickstart(&st)
            .or_err()?
            .iter()
            .map(|q| {
                from_json(&serde_json::json!({
                    "id": format!("find:{}", q.id), "title": q.title,
                    "description": q.description, "grid": q.grid, "rule": q.rule,
                    "rule_family": q.rule_family, "behaviour": q.behaviour,
                    "source_kind": q.source_kind, "source_id": q.source_id, "score": q.score,
                    "status": q.status, "created": q.created,
                }))
            })
            .collect())
    };
    e.register_fn("quickstart", move |l: &mut LibHandle| qs(l, None));
    e.register_fn("quickstart", move |l: &mut LibHandle, status: &str| {
        qs(l, Some(status))
    });
    e.register_fn("query", |l: &mut LibHandle, m: Map| -> R<Array> {
        let q: Query =
            serde_json::from_value(map_json(&m)?).map_err(|e| format!("bad library query: {e}"))?;
        Ok(lib_records(l.0.query(&q).or_err()?))
    });
    e.register_fn("list", |l: &mut LibHandle| -> R<Array> {
        Ok(lib_records(l.0.list().or_err()?))
    });
    e.register_fn("count", |l: &mut LibHandle| -> R<INT> {
        Ok(l.0.count(&Query::default()).or_err()? as INT)
    });
    e.register_fn("count", |l: &mut LibHandle, m: Map| -> R<INT> {
        let q: Query =
            serde_json::from_value(map_json(&m)?).map_err(|e| format!("bad library query: {e}"))?;
        Ok(l.0.count(&q).or_err()? as INT)
    });
    e.register_fn("get", |l: &mut LibHandle, key: &str| -> R<Map> {
        let r = l.0.get(key).or_err()?;
        Ok(from_json(&crate::library::record_json(&r)).cast::<Map>())
    });
    let c = ctx.clone();
    let catalog = move |l: &LibHandle, s: &Shared, v: &Value| -> R<Map> {
        spec::check_keys(v, &["ca", "max_period", "source"], "catalog").or_err()?;
        let source = spec::get_str(v, "source")
            .or_err()?
            .map(String::from)
            .unwrap_or_else(|| match &c.opts.script_path {
                Some(p) => format!("script {}", p.display()),
                None => "script".into(),
            });
        let g = s.lock();
        let rep = crate::library::catalog_world(
            &l.0,
            &g.world,
            spec::get_u64(v, "ca").or_err()?.map(|x| x as u32),
            spec::get_u64(v, "max_period").or_err()?,
            &source,
            DiscoveredBy::Script,
        )
        .or_err()?;
        Ok(from_json(&rep.to_json()).cast::<Map>())
    };
    for name in ["catalog", "add_from"] {
        let f = catalog.clone();
        e.register_fn(name, move |l: &mut LibHandle, s: Shared| {
            f(l, &s, &Value::Null)
        });
        let f = catalog.clone();
        e.register_fn(name, move |l: &mut LibHandle, s: Shared, m: Map| {
            f(l, &s, &map_json(&m)?)
        });
    }
    let prom = |l: &LibHandle, key: &str, title: Option<&str>| -> R<Map> {
        let (sc, id) = promote(&l.0, key, title, None).or_err()?;
        let mut m = Map::new();
        m.insert("entry".into(), format!("find:{id}").into());
        m.insert("id".into(), id.into());
        m.insert("title".into(), sc.name.into());
        Ok(m)
    };
    e.register_fn("promote", move |l: &mut LibHandle, key: &str| {
        prom(l, key, None)
    });
    e.register_fn(
        "promote",
        move |l: &mut LibHandle, key: &str, title: &str| prom(l, key, Some(title)),
    );
    // Soup search into this library: lib.search(#{ rules, tilings, soups, size, ... }).
    let c = ctx.clone();
    e.register_fn("search", move |l: &mut LibHandle, m: Map| -> R<Map> {
        let v = map_json(&m)?;
        let rep = crate::library::search_into(&l.0, &v, &c).or_err()?;
        Ok(from_json(&rep).cast::<Map>())
    });
    // Fast hunter: lib.hunt(#{ rules, tilings, soups, size, density, ... }) runs a resumable hunt
    // job here; hunt(#{ ... }) uses the default catalog.
    let c = ctx.clone();
    e.register_fn("hunt", move |l: &mut LibHandle, m: Map| -> R<Map> {
        let v = map_json(&m)?;
        let p = crate::library::hunt_into(&l.0, &v, &c).or_err()?;
        Ok(from_json(&p).cast::<Map>())
    });
    let c = ctx.clone();
    e.register_fn("hunt", move |m: Map| -> R<Map> {
        let lib = match &c.opts.library {
            Some(p) => Library::open(p.clone()),
            None => Library::open_default(),
        };
        let v = map_json(&m)?;
        let p = crate::library::hunt_into(&lib, &v, &c).or_err()?;
        Ok(from_json(&p).cast::<Map>())
    });
    // Labelled jobs, censuses as jobs, rule-space neighbours (crate::jobs).
    e.register_fn("jobs", |l: &mut LibHandle| -> R<Array> {
        Ok(from_json(&crate::jobs::jobs(&l.0, &Value::Null).or_err()?).cast::<Array>())
    });
    e.register_fn("jobs", |l: &mut LibHandle, m: Map| -> R<Array> {
        Ok(from_json(&crate::jobs::jobs(&l.0, &map_json(&m)?).or_err()?).cast::<Array>())
    });
    let c = ctx.clone();
    e.register_fn("census", move |l: &mut LibHandle, m: Map| -> R<Map> {
        let v = map_json(&m)?;
        let p = crate::jobs::census_into(&l.0, &v, &c).or_err()?;
        Ok(from_json(&p).cast::<Map>())
    });
    let c = ctx.clone();
    e.register_fn("census", move |m: Map| -> R<Map> {
        let lib = match &c.opts.library {
            Some(p) => Library::open(p.clone()),
            None => Library::open_default(),
        };
        let v = map_json(&m)?;
        let p = crate::jobs::census_into(&lib, &v, &c).or_err()?;
        Ok(from_json(&p).cast::<Map>())
    });
    e.register_fn("census_rows", |l: &mut LibHandle, job: INT| -> R<Array> {
        Ok(from_json(&crate::jobs::census_rows(&l.0, job).or_err()?).cast::<Array>())
    });
    e.register_fn("rule_neighbours", |rule: &str| -> R<Array> {
        Ok(from_json(&crate::jobs::rule_neighbours(rule, &Value::Null).or_err()?).cast::<Array>())
    });
    e.register_fn("rule_neighbours", |rule: &str, m: Map| -> R<Array> {
        Ok(
            from_json(&crate::jobs::rule_neighbours(rule, &map_json(&m)?).or_err()?)
                .cast::<Array>(),
        )
    });
    e.register_fn("hunt_status", |l: &mut LibHandle, job: INT| -> R<Map> {
        let p = crate::library::hunt_status(&l.0, job).or_err()?;
        Ok(from_json(&p).cast::<Map>())
    });
    e.register_fn("hunt_stop", |l: &mut LibHandle, job: INT| -> R<()> {
        jetcarp_core::hunt::job::request_stop(&l.0, job).or_err()
    });
    e.register_fn("hunts", |l: &mut LibHandle| -> R<Array> {
        let mut out = Array::new();
        for j in
            l.0.jobs()
                .or_err()?
                .into_iter()
                .filter(|j| j.kind == "hunt")
        {
            out.push(from_json(
                &crate::library::hunt_status(&l.0, j.id).or_err()?,
            ));
        }
        Ok(out)
    });
    // A sweep run to the Quick Start: lib.promote_run(folder, "HexT3 sum 588" | "#12" | "n=588")
    // and lib.promote_run(folder, run, #{ title, note }).
    let c = ctx.clone();
    let prom_run = move |l: &LibHandle, folder: &str, run: &str, v: &Value| -> R<Map> {
        spec::check_keys(v, &["title", "note"], "promote_run").or_err()?;
        let dir = c.readable(folder).or_err()?;
        let p = crate::results::promote_folder_run(
            &dir,
            run,
            &l.0,
            spec::get_str(v, "title").or_err()?,
            spec::get_str(v, "note").or_err()?,
        )
        .or_err()?;
        let mut m = Map::new();
        m.insert("rule".into(), p.rule.into());
        m.insert("index".into(), (p.index as INT).into());
        m.insert("title".into(), p.title.into());
        m.insert("entry".into(), format!("find:{}", p.id).into());
        Ok(m)
    };
    let f = prom_run.clone();
    e.register_fn(
        "promote_run",
        move |l: &mut LibHandle, folder: &str, run: &str| f(l, folder, run, &Value::Null),
    );
    e.register_fn(
        "promote_run",
        move |l: &mut LibHandle, folder: &str, run: &str, m: Map| {
            prom_run(l, folder, run, &map_json(&m)?)
        },
    );
    e.register_fn("update", |l: &mut LibHandle, key: &str, m: Map| -> R<Map> {
        let u: Update = serde_json::from_value(map_json(&m)?)
            .map_err(|e| format!("bad library update: {e}"))?;
        let r = l.0.update(key, &u).or_err()?;
        Ok(from_json(&crate::library::record_json(&r)).cast::<Map>())
    });
    // `lib.rate(kind, id, rating)`, `lib.rate(kind, id, rating, #{ rater, note })`.
    let rate = |l: &LibHandle, kind: &str, id: &str, rating: f64, v: &Value| -> R<INT> {
        spec::check_keys(v, &["rater", "note"], "rate").or_err()?;
        let rater = spec::get_str(v, "rater").or_err()?.unwrap_or("user");
        let note = spec::get_str(v, "note").or_err()?.unwrap_or("");
        l.0.rate(kind, id, rater, rating, note).or_err()
    };
    e.register_fn(
        "rate",
        move |l: &mut LibHandle, kind: &str, id: Dynamic, rating: Dynamic| -> R<INT> {
            let r = rating
                .as_float()
                .or_else(|_| rating.as_int().map(|i| i as f64))?;
            rate(l, kind, &id.to_string(), r, &json!({}))
        },
    );
    e.register_fn(
        "rate",
        move |l: &mut LibHandle, kind: &str, id: Dynamic, rating: Dynamic, m: Map| -> R<INT> {
            let r = rating
                .as_float()
                .or_else(|_| rating.as_int().map(|i| i as f64))?;
            rate(l, kind, &id.to_string(), r, &map_json(&m)?)
        },
    );
}
