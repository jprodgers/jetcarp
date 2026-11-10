//! Cataloging a world's objects into the pattern library (`jetcarp_core::library`), shared by the
//! Rhai `library()` handle and the app (sweep favourites).

use jetcarp_api::args;
use jetcarp_core::World;
use jetcarp_core::library::split::records_from_object;
use jetcarp_core::library::{AddOutcome, DiscoveredBy, Library, PatternRecord, Provenance, now_utc};
use jetcarp_core::objects::{ObjClass, ObjectOptions, find_objects};
use serde_json::{Value, json};
use std::collections::BTreeMap;

/// What [`catalog_world`] did.
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize)]
pub struct CatalogReport {
    /// Objects found (every copy).
    pub objects: usize,
    /// Distinct patterns stored for the first time.
    pub added: Vec<String>,
    /// Distinct patterns that were already in the library (their seen count grew).
    pub merged: Vec<String>,
    /// Objects that are not still lifes, oscillators, or spaceships (or could not be recorded).
    pub skipped: usize,
    /// Patterns that failed verification, with the reason.
    pub failed: Vec<(String, String)>,
}

impl CatalogReport {
    pub fn to_json(&self) -> Value {
        json!({
            "objects": self.objects,
            "added": self.added,
            "merged": self.merged,
            "ids": self.added.iter().chain(&self.merged).collect::<Vec<_>>(),
            "skipped": self.skipped,
            "failed": self.failed.iter().map(|(id, e)| json!({"id": id, "error": e})).collect::<Vec<_>>(),
        })
    }
}

/// Find the objects of CA `ca` (default: the first CA the detector supports) in `world`, verify
/// each distinct still life, oscillator, and spaceship, and add it to `lib` (copies add to its
/// seen count). `source` goes into the provenance.
pub fn catalog_world(
    lib: &Library,
    world: &World,
    ca: Option<u32>,
    max_period: Option<u64>,
    source: &str,
    by: DiscoveredBy,
) -> Result<CatalogReport, String> {
    let d = ObjectOptions::default();
    let opts = ObjectOptions {
        ca,
        max_period: max_period.unwrap_or(d.max_period),
        ..d
    };
    let rep = find_objects(world, &opts)?;
    let ca_id = rep.ca_id;
    let tiling = world.topo.spec.clone();
    let prov = Provenance {
        source: source.to_string(),
        tiling: Some(tiling),
        rng_state: Some(world.to_scene().rng_state),
        generation: Some(world.generation),
        jetcarp_version: jetcarp_core::VERSION.into(),
        date: now_utc(),
        ..Default::default()
    };
    let mut out = CatalogReport {
        objects: rep.objects.len(),
        ..Default::default()
    };
    let mut distinct: BTreeMap<String, PatternRecord> = BTreeMap::new();
    for o in &rep.objects {
        if matches!(o.class, ObjClass::Other { .. }) {
            out.skipped += 1;
            continue;
        }
        match records_from_object(world, ca_id, o, prov.clone(), by) {
            Ok(parts) => {
                for r in parts {
                    match distinct.get_mut(&r.id) {
                        Some(x) => x.seen += 1,
                        None => {
                            distinct.insert(r.id.clone(), r);
                        }
                    }
                }
            }
            Err(_) => out.skipped += 1,
        }
    }
    let ids: Vec<String> = distinct.keys().cloned().collect();
    let res = lib.add_many(distinct.into_values().collect(), None)?;
    for (id, r) in ids.into_iter().zip(res) {
        match r {
            Ok(AddOutcome::Added) => out.added.push(id),
            Ok(AddOutcome::Merged { .. }) => out.merged.push(id),
            Err(e) => out.failed.push((id, e)),
        }
    }
    Ok(out)
}

/// A record as JSON for scripts (the stored fields).
pub fn record_json(r: &PatternRecord) -> Value {
    let mut v = serde_json::to_value(r).unwrap_or(Value::Null);
    v["label"] = json!(r.label());
    v
}

/// `lib.search(#{...})`: a soup search (`jetcarp_core::library::search`, as `jetcarp library search`)
/// whose verified finds are added to `lib`. Keys: `rules` (list or text), `tilings` (list or
/// text: ids, vertex configurations, `archimedean`, `kuniform:2`, ...), `soups` (per rule and
/// tiling, default 16), `size` (`[w, h]`, `"WxH"`, or n for n x n unit cells; default 64 x 64),
/// `boundary` (`wrap` default, or `dead` with `soup_radius` to catch escaping spaceships),
/// `gens` (default 2000), `density` (0.37), `seed` (1), `max_period` (64), `soup_radius`,
/// `store` (default true). Returns `#{ groups: [#{ rule, tiling, soups, objects, distinct,
/// patterns: [#{ id, label, class, period, speed, cells, count, new }] }], new_patterns,
/// known_patterns, new_rules, job, ids, seconds }`. Stored searches are a catalog job (`job`).
pub(crate) fn search_into(lib: &Library, v: &Value, ctx: &crate::Ctx) -> Result<Value, String> {
    use jetcarp_core::library::search::{SearchSpec, search};
    args::check_keys(
        v,
        &[
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
        ],
        "search",
    )?;
    // A search with this label already in the catalog is not run again (searches do not
    // resume; their results were stored when the job was written).
    if let Some((job, _)) = crate::jobs::labelled(lib, "search", v)? {
        ctx.log(format!(
            "search job {job} ({}) already done",
            args::get_str(v, "label")?.unwrap_or("")
        ));
        return Ok(
            json!({ "job": job, "skipped": true, "groups": [], "new_patterns": 0,
            "known_patterns": 0, "new_rules": 0, "ids": [], "stored": true, "seconds": 0.0 }),
        );
    }
    let list = |key: &str| -> Result<String, String> {
        Ok(match v.get(key) {
            None | Some(Value::Null) => String::new(),
            Some(Value::String(s)) => s.clone(),
            Some(Value::Array(a)) => a
                .iter()
                .map(|x| {
                    x.as_str()
                        .map(String::from)
                        .ok_or(format!("`{key}` must be text"))
                })
                .collect::<Result<Vec<_>, _>>()?
                .join(";"),
            Some(x) => return Err(format!("`{key}` must be a list of text, got {x}")),
        })
    };
    let rules: Vec<String> = list("rules")?
        .split([';', ','])
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(String::from)
        .collect();
    if rules.is_empty() {
        return Err("give `rules`, e.g. [\"B3/S23\"]".into());
    }
    let t = list("tilings")?.replace(';', ",");
    let tilings = jetcarp_core::library::tiling_list(if t.trim().is_empty() { "square" } else { &t })?;
    let (w, h) = match v.get("size") {
        None | Some(Value::Null) => (64, 64),
        Some(Value::Number(n)) => {
            let n = n.as_u64().ok_or("`size` must be a whole number")? as u32;
            (n, n)
        }
        Some(Value::String(s)) => crate::Screen::parse_size(s)?,
        Some(Value::Array(a)) if a.len() == 2 => (
            a[0].as_u64().ok_or("`size` must be [w, h]")? as u32,
            a[1].as_u64().ok_or("`size` must be [w, h]")? as u32,
        ),
        Some(x) => {
            return Err(format!(
                "`size` must be [w, h], \"WxH\", or a number, got {x}"
            ));
        }
    };
    ctx.check_cells(w as usize * h as usize)?;
    let d = SearchSpec::default();
    let spec = SearchSpec {
        rules,
        tilings,
        size: jetcarp_core::SizeSpec::Rect { w, h },
        boundary: match args::get_str(v, "boundary")? {
            Some(b) => args::parse_boundary(b)?,
            None => d.boundary,
        },
        soups: args::get_u64(v, "soups")?.unwrap_or(16) as u32,
        gens: args::get_u64(v, "gens")?.unwrap_or(d.gens),
        density: args::get_f64(v, "density")?.unwrap_or(d.density),
        seed: args::get_u64(v, "seed")?.unwrap_or(d.seed),
        max_period: args::get_u64(v, "max_period")?.unwrap_or(d.max_period),
        soup_radius: args::get_f64(v, "soup_radius")?,
        ..d
    };
    let store = args::get_bool(v, "store")?.unwrap_or(true);
    let t0 = std::time::Instant::now();
    let label = format!("search {}", spec.rules.join(" "));
    let report = |done: usize, total: usize| {
        let el = t0.elapsed().as_secs_f64();
        ctx.progress(crate::Progress {
            label: label.clone(),
            done: done as u64,
            total: total as u64,
            item: String::new(),
            elapsed: el,
            eta: (done > 0).then(|| el / done as f64 * total.saturating_sub(done) as f64),
        });
    };
    let out = search(&spec, Some(&report))?;
    let (mut new_ids, mut known, mut new_rules, mut job) = (vec![], 0, 0, None);
    if store {
        let st = lib.store_search(&out)?;
        new_ids = st.added;
        known = st.merged.len();
        new_rules = st.new_rules.len();
        job = Some(st.job);
        crate::jobs::set_label(lib, st.job, v)?;
    }
    let groups: Vec<Value> = out
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
                    "speed": f.speed, "cells": f.cells, "count": f.count,
                    "new": new_ids.contains(&f.id),
                })).collect::<Vec<_>>(),
            })
        })
        .collect();
    ctx.log(format!(
        "{label}: {} distinct patterns, {} new in the library, {:.1}s",
        out.records.len(),
        new_ids.len(),
        t0.elapsed().as_secs_f64()
    ));
    Ok(json!({
        "groups": groups,
        "new_patterns": new_ids.len(),
        "known_patterns": known,
        "new_rules": new_rules,
        "job": job,
        "ids": out.records.iter().map(|r| r.id.clone()).collect::<Vec<_>>(),
        "stored": store,
        "seconds": t0.elapsed().as_secs_f64(),
    }))
}

/// `hunt(#{ ... })` / `lib.hunt(#{ ... })`: run (or resume) a fast-hunter job in this catalog on
/// the script's thread, until it is done, `limit` soups have run, or the script is cancelled.
/// Returns the hunt's progress (see `jetcarp_core::hunt::job::HuntProgress`).
pub(crate) fn hunt_into(
    lib: &Library,
    v: &Value,
    ctx: &std::sync::Arc<crate::Ctx>,
) -> Result<Value, String> {
    use jetcarp_core::hunt::job::{self, HuntOptions, HuntSpec};
    use jetcarp_core::hunt::soup::{SoupShape, SoupSpec};
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    args::check_keys(
        v,
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
            "pop_cap",
            "margin",
            "threads",
            "resume",
            "limit",
            "label",
            "store",
        ],
        "hunt",
    )?;
    let list = |key: &str| -> Result<Vec<String>, String> {
        Ok(match v.get(key) {
            None | Some(Value::Null) => vec![],
            Some(Value::String(s)) => s
                .split([';', ','])
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
                .collect::<Result<Vec<_>, _>>()?,
            Some(x) => return Err(format!("`{key}` must be a list of text, got {x}")),
        })
    };
    let found = crate::jobs::labelled(lib, "hunt", v)?.map(|x| x.0);
    let jid = match args::get_u64(v, "resume")?.map(|j| j as i64).or(found) {
        Some(j) => j,
        None => {
            let d = HuntSpec::default();
            let mut rules = list("rules")?;
            if rules.is_empty() {
                rules = d.rules.clone();
            }
            // Tilings as text, or as tiling maps (as settings store them).
            let tilings = match v.get("tilings") {
                Some(Value::Array(a)) if a.iter().any(|x| x.is_object()) => a
                    .iter()
                    .map(crate::jobs::one_tiling)
                    .collect::<Result<Vec<_>, _>>()?,
                _ => {
                    let t = list("tilings")?.join(",");
                    jetcarp_core::library::tiling_list(if t.trim().is_empty() {
                        "square"
                    } else {
                        &t
                    })?
                }
            };
            let (w, h) = match v.get("size") {
                None | Some(Value::Null) => (d.soup.w, d.soup.h),
                Some(Value::Number(n)) => {
                    let n = n.as_u64().ok_or("`size` must be a whole number")? as u32;
                    (n, n)
                }
                Some(Value::String(s)) => crate::Screen::parse_size(s)?,
                Some(Value::Array(a)) if a.len() == 2 => (
                    a[0].as_u64().ok_or("`size` must be [w, h]")? as u32,
                    a[1].as_u64().ok_or("`size` must be [w, h]")? as u32,
                ),
                Some(x) => {
                    return Err(format!(
                        "`size` must be [w, h], \"WxH\", or a number, got {x}"
                    ));
                }
            };
            let soups = args::get_u64(v, "soups")?.unwrap_or(10_000);
            let spec = HuntSpec {
                rules,
                tilings,
                neighborhood: match args::get_str(v, "nb")? {
                    None => None,
                    Some("touch") => Some(jetcarp_core::Nb::Touch),
                    Some("edge") => Some(jetcarp_core::Nb::Edge),
                    Some(x) => return Err(format!("`nb` is \"touch\" or \"edge\", not {x}")),
                },
                soups: (soups > 0).then_some(soups),
                soup: SoupSpec {
                    w,
                    h,
                    density: args::get_f64(v, "density")?.unwrap_or(d.soup.density),
                    shape: match args::get_str(v, "shape")? {
                        Some(s) => SoupShape::parse(s)?,
                        None => SoupShape::Rect,
                    },
                },
                seed: args::get_u64(v, "seed")?.unwrap_or(d.seed),
                max_gens: args::get_u64(v, "gens")?.unwrap_or(d.max_gens),
                max_period: args::get_u64(v, "max_period")?.unwrap_or(d.max_period),
                pop_cap: args::get_u64(v, "pop_cap")?.unwrap_or(0),
                margin: args::get_u64(v, "margin")?.unwrap_or(0) as u32,
                store: store_policy(v)?,
            };
            let j = job::start(lib, &spec)?;
            crate::jobs::set_label(lib, j, v)?;
            j
        }
    };
    let stop = Arc::new(AtomicBool::new(false));
    let done = Arc::new(AtomicBool::new(false));
    // The script's cancel flag and time limit stop the hunt at its next batch.
    let watcher = {
        let (stop, done, c) = (stop.clone(), done.clone(), ctx.clone());
        std::thread::spawn(move || {
            while !done.load(Ordering::Relaxed) {
                if c.stopped() {
                    stop.store(true, Ordering::SeqCst);
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
        })
    };
    let c = ctx.clone();
    let t0 = std::time::Instant::now();
    let opts = HuntOptions {
        threads: args::get_u64(v, "threads")?.unwrap_or(0) as usize,
        stop,
        limit: args::get_u64(v, "limit")?,
        on_progress: Some(Arc::new(move |p: &job::HuntProgress| {
            let total = p.soups_total.unwrap_or(0);
            let el = t0.elapsed().as_secs_f64();
            c.progress(crate::Progress {
                label: format!("hunt {}", p.job),
                done: p.soups_done,
                total,
                item: p
                    .newest
                    .first()
                    .map(|f| format!("{} ({})", f.label, f.rule))
                    .unwrap_or_default(),
                elapsed: el,
                eta: (p.soups_per_s > 0.0 && total > p.soups_done).then(|| {
                    (total - p.soups_done) as f64 * p.groups.len().max(1) as f64 / p.soups_per_s
                }),
            });
        })),
        ..Default::default()
    };
    let res = job::run(lib, jid, &opts);
    done.store(true, Ordering::Relaxed);
    let _ = watcher.join();
    let p = res?;
    ctx.log(format!("{} ({:.1}s)", p.line(), t0.elapsed().as_secs_f64()));
    serde_json::to_value(&p).map_err(|e| e.to_string())
}

/// A hunt's saved progress (`lib.hunt_status(job)`).
pub(crate) fn hunt_status(lib: &Library, job: i64) -> Result<Value, String> {
    let (spec, p) = jetcarp_core::hunt::job::status(lib, job)?;
    let mut v = serde_json::to_value(&p).map_err(|e| e.to_string())?;
    v["spec"] = serde_json::to_value(&spec).map_err(|e| e.to_string())?;
    Ok(v)
}

/// A hunt's storage policy from `store`: `"all"`, or `#{ max_cells, max_volume, min_seen }`
/// (missing keys keep the defaults: 64 cells, period x cells 4,000, seen twice).
fn store_policy(v: &Value) -> Result<jetcarp_core::hunt::job::StorePolicy, String> {
    use jetcarp_core::hunt::job::StorePolicy;
    let d = StorePolicy::default();
    Ok(match v.get("store") {
        None | Some(Value::Null) => d,
        Some(Value::String(s)) if s == "all" => StorePolicy::all(),
        Some(m @ Value::Object(_)) => {
            args::check_keys(m, &["max_cells", "max_volume", "min_seen"], "store")?;
            StorePolicy {
                max_cells: args::get_u64(m, "max_cells")?.unwrap_or(d.max_cells),
                max_volume: args::get_u64(m, "max_volume")?.unwrap_or(d.max_volume),
                min_seen: args::get_u64(m, "min_seen")?.unwrap_or(d.min_seen),
            }
        }
        Some(x) => return Err(format!("`store` is \"all\" or a map, not {x}")),
    })
}
