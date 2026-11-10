//! Stage 1 settings search from scripts: `stage1(#{ ... })`, `lib.stage1(#{ ... })`, and the
//! `lib` helpers for settings, the approval queue, ratings, and refits (see `help.md`).

use crate::spec::{check_keys, get_f64, get_str, get_u64};
use jetcarp_core::library::{Library, SettingStatus, SettingsQuery, tiling_list};
use jetcarp_core::stage1::job::{
    self, Base, Neighbours, Refine, Space, Stage1Options, Stage1Progress, Stage1Spec,
};
use jetcarp_core::{Boundary, Nb};
use serde_json::{Value, json};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

fn texts(v: &Value, key: &str) -> Result<Vec<String>, String> {
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
}

fn numbers(v: &Value, key: &str) -> Result<Option<Vec<f64>>, String> {
    Ok(match v.get(key) {
        None | Some(Value::Null) => None,
        Some(Value::Number(n)) => Some(vec![n.as_f64().unwrap_or(0.0)]),
        Some(Value::Array(a)) => Some(
            a.iter()
                .map(|x| x.as_f64().ok_or(format!("`{key}` must be numbers")))
                .collect::<Result<Vec<_>, _>>()?,
        ),
        Some(x) => return Err(format!("`{key}` must be a number or a list, got {x}")),
    })
}

fn size(v: &Value, what: &str) -> Result<[u32; 2], String> {
    match v {
        Value::Number(n) => {
            let n = n
                .as_u64()
                .ok_or(format!("`{what}` must be a whole number"))? as u32;
            Ok([n, n])
        }
        Value::String(s) => crate::Screen::parse_size(s).map(|(w, h)| [w, h]),
        Value::Array(a) if a.len() == 2 => Ok([
            a[0].as_u64().ok_or(format!("`{what}` must be [w, h]"))? as u32,
            a[1].as_u64().ok_or(format!("`{what}` must be [w, h]"))? as u32,
        ]),
        x => Err(format!(
            "`{what}` must be [w, h], \"WxH\", or a number, got {x}"
        )),
    }
}

fn digits(v: &Value, key: &str) -> Result<Option<Vec<u32>>, String> {
    Ok(match v.get(key) {
        None | Some(Value::Null) => None,
        Some(Value::String(s)) => Some(jetcarp_core::census::parse_digits(s)?),
        Some(Value::Array(a)) => Some(
            a.iter()
                .map(|x| {
                    x.as_u64()
                        .map(|d| d as u32)
                        .ok_or(format!("`{key}` must be digits"))
                })
                .collect::<Result<Vec<_>, _>>()?,
        ),
        Some(x) => {
            return Err(format!(
                "`{key}` must be digits (\"1-6\" or [1, 2]), got {x}"
            ));
        }
    })
}

/// The search spec from a script map (see `help.md`).
pub fn spec_from(v: &Value) -> Result<Stage1Spec, String> {
    let d = Stage1Spec::default();
    let t = texts(v, "tilings")?.join(",");
    let tilings = tiling_list(if t.trim().is_empty() { "hex" } else { &t })?;
    let nbs = match get_str(v, "nb")? {
        None | Some("touch") => vec![Nb::Touch],
        Some("edge") => vec![Nb::Edge],
        Some("both") => vec![Nb::Touch, Nb::Edge],
        Some(x) => return Err(format!("`nb` is \"touch\", \"edge\", or \"both\", not {x}")),
    };
    let states: Vec<u8> = numbers(v, "states")?
        .unwrap_or(vec![2.0])
        .into_iter()
        .map(|x| x as u8)
        .collect();
    let rules: Vec<String> = texts(v, "rules")?
        .into_iter()
        .flat_map(|l| l.split_whitespace().map(String::from).collect::<Vec<_>>())
        .collect();
    let mut spaces = Vec::new();
    for t in &tilings {
        for &nb in &nbs {
            for &st in &states {
                let mut sp = Space::new(t.clone(), nb);
                sp.states = st;
                sp.birth = digits(v, "birth")?;
                sp.survive = digits(v, "survive")?;
                sp.rules = rules.clone();
                sp.all_max = get_u64(v, "all_max")?;
                sp.sample = get_u64(v, "sample")?.map(|x| x as usize);
                spaces.push(sp);
            }
        }
    }
    let base = Base {
        density: get_f64(v, "density")?.unwrap_or(d.base.density),
        soup: get_f64(v, "soup")?.unwrap_or(d.base.soup),
        grid: match v.get("grid") {
            None | Some(Value::Null) => d.base.grid,
            Some(g) => size(g, "grid")?,
        },
        boundary: match get_str(v, "boundary")? {
            None | Some("wrap") => Boundary::Wrap,
            Some("dead") => Boundary::Dead,
            Some(x) => return Err(format!("`boundary` is \"wrap\" or \"dead\", not {x}")),
        },
        gens: get_u64(v, "gens")?.unwrap_or(d.base.gens),
        soups: get_u64(v, "soups")?.unwrap_or(d.base.soups as u64) as u32,
        seed: get_u64(v, "seed")?.unwrap_or(d.base.seed),
    };
    let refine = match v.get("refine") {
        None | Some(Value::Null) => d.refine.clone(),
        Some(Value::Bool(false)) => Refine {
            top: 0,
            ..d.refine.clone()
        },
        Some(r) => {
            check_keys(r, &["top", "densities", "soups", "grids", "gens"], "refine")?;
            Refine {
                top: get_u64(r, "top")?.map_or(d.refine.top, |x| x as usize),
                densities: numbers(r, "densities")?.unwrap_or(d.refine.densities.clone()),
                soups: numbers(r, "soups")?.unwrap_or(d.refine.soups.clone()),
                grids: match r.get("grids") {
                    None | Some(Value::Null) => d.refine.grids.clone(),
                    Some(Value::Array(a)) => a
                        .iter()
                        .map(|g| size(g, "refine.grids"))
                        .collect::<Result<_, _>>()?,
                    Some(g) => vec![size(g, "refine.grids")?],
                },
                gens: numbers(r, "gens")?
                    .map(|v| v.into_iter().map(|x| x as u64).collect())
                    .unwrap_or(d.refine.gens.clone()),
            }
        }
    };
    let neighbours = match v.get("neighbours") {
        None | Some(Value::Null) => d.neighbours.clone(),
        Some(Value::Bool(false)) => Neighbours { top: 0, rounds: 0 },
        Some(n) => {
            check_keys(n, &["top", "rounds"], "neighbours")?;
            Neighbours {
                top: get_u64(n, "top")?.map_or(d.neighbours.top, |x| x as usize),
                rounds: get_u64(n, "rounds")?.map_or(d.neighbours.rounds, |x| x as u32),
            }
        }
    };
    Ok(Stage1Spec {
        spaces,
        base,
        refine,
        neighbours,
    })
}

/// `stage1(#{ ... })` / `lib.stage1(#{ ... })`: run (or resume) a search in this catalog on the
/// script's thread, until it is done, `limit` settings have run, or the script is cancelled.
pub(crate) fn stage1_into(
    lib: &Library,
    v: &Value,
    ctx: &Arc<crate::Ctx>,
) -> Result<Value, String> {
    check_keys(
        v,
        &[
            "tilings",
            "nb",
            "states",
            "birth",
            "survive",
            "rules",
            "all_max",
            "sample",
            "density",
            "soup",
            "grid",
            "boundary",
            "gens",
            "soups",
            "seed",
            "refine",
            "neighbours",
            "threads",
            "resume",
            "limit",
            "label",
        ],
        "stage1",
    )?;
    let jid = match get_u64(v, "resume")? {
        Some(j) => j as i64,
        None => match crate::jobs::labelled(lib, "stage1", v)? {
            Some((j, _)) => j,
            None => {
                let j = job::start(lib, &spec_from(v)?)?;
                crate::jobs::set_label(lib, j, v)?;
                j
            }
        },
    };
    let stop = Arc::new(AtomicBool::new(false));
    let done = Arc::new(AtomicBool::new(false));
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
    let opts = Stage1Options {
        threads: get_u64(v, "threads")?.unwrap_or(0) as usize,
        stop,
        limit: get_u64(v, "limit")?,
        on_progress: Some(Arc::new(move |p: &Stage1Progress| {
            c.progress(crate::Progress {
                label: format!(
                    "stage1 {} {}",
                    p.job,
                    p.passes.get(p.pass).cloned().unwrap_or_default()
                ),
                done: p.pass_done,
                total: p.pass_total,
                item: p
                    .best
                    .first()
                    .map(|b| format!("best {:.1} {}", b.score, b.rule))
                    .unwrap_or_default(),
                elapsed: t0.elapsed().as_secs_f64(),
                eta: (p.per_s > 0.0 && p.pass_total > p.pass_done)
                    .then(|| (p.pass_total - p.pass_done) as f64 / p.per_s),
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

/// A search's saved progress with its spec (`lib.stage1_status(job)`).
pub fn status(lib: &Library, job: i64) -> Result<Value, String> {
    let (spec, p) = job::status(lib, job)?;
    let mut v = serde_json::to_value(&p).map_err(|e| e.to_string())?;
    v["spec"] = serde_json::to_value(&spec).map_err(|e| e.to_string())?;
    Ok(v)
}

/// The settings query from a script map.
pub fn query_from(v: &Value) -> Result<SettingsQuery, String> {
    check_keys(
        v,
        &[
            "status",
            "job",
            "tiling",
            "rule",
            "family",
            "behaviour",
            "min_score",
            "order",
            "limit",
            "offset",
        ],
        "settings",
    )?;
    Ok(SettingsQuery {
        status: texts(v, "status")?
            .iter()
            .map(|s| SettingStatus::parse(s))
            .collect::<Result<_, _>>()?,
        job: get_u64(v, "job")?.map(|j| j as i64),
        tiling: get_str(v, "tiling")?.map(String::from),
        rule: get_str(v, "rule")?.map(String::from),
        family: get_str(v, "family")?.map(String::from),
        behaviour: get_str(v, "behaviour")?.map(String::from),
        min_score: get_f64(v, "min_score")?,
        order: get_str(v, "order")?.map(String::from),
        limit: get_u64(v, "limit")?.unwrap_or(100) as usize,
        offset: get_u64(v, "offset")?.unwrap_or(0) as usize,
    })
}

/// Settings as maps (`lib.settings(#{ ... })`, `lib.queue()`).
pub fn settings(lib: &Library, q: &SettingsQuery) -> Result<Value, String> {
    let rows = lib.settings(q)?;
    Ok(Value::Array(
        rows.iter()
            .map(|r| {
                json!({
                    "id": r.id,
                    "entry_id": r.entry_id(),
                    "rule": r.notation,
                    "family": r.family,
                    "tiling": r.tiling_name,
                    "score": r.score,
                    "status": r.status.name(),
                    "behaviour": r.behaviour,
                    "tags": r.descriptors.tags,
                    "pass": r.pass,
                    "job": r.job,
                    "setting": r.setting,
                    "descriptors": r.descriptors,
                })
            })
            .collect(),
    ))
}

/// Settings as small maps (`lib.settings(#{ brief: true, ... })`): the descriptors are reduced
/// to the object counts.
pub fn settings_brief(lib: &Library, q: &SettingsQuery) -> Result<Value, String> {
    let rows = lib.settings(q)?;
    Ok(Value::Array(
        rows.iter()
            .map(|r| {
                let d = &r.descriptors;
                json!({
                    "id": r.id,
                    "rule": r.notation,
                    "tiling": r.tiling_name,
                    "score": r.score,
                    "status": r.status.name(),
                    "behaviour": r.behaviour,
                    "job": r.job,
                    "setting": { "tiling": r.setting.tiling, "nb": r.setting.nb },
                    "descriptors": {
                        "ship_kinds": d.ship_kinds,
                        "oscillator_kinds": d.oscillator_kinds,
                        "distinct": d.distinct,
                        "longest_period": d.longest_period,
                    },
                })
            })
            .collect(),
    ))
}
