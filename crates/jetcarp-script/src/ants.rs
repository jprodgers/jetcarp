//! Ant and turmite search bindings (`jetcarp_core::ants`): `ants(#{...})` / `lib.ants(#{...})` run a
//! resumable search job, `ant_run(rule, #{...})` runs one rule, and `lib.ant(id)`,
//! `lib.ant_list(#{...})`, `lib.ant_census()`, `lib.ant_status(job)`, `lib.ant_jobs()`, and
//! `lib.ant_stop(job)` read and steer the catalog.

use crate::Ctx;
use crate::api::{LibHandle, from_json, to_json};
use crate::spec::{check_keys, get_bool, get_f64, get_str, get_u64};
use jetcarp_core::ants::job::{self, AntOptions, AntSpec, Soups, parse_rule};
use jetcarp_core::ants::run::{Limits, Outcome, Start};
use jetcarp_core::ants::search::{Strings, Tables};
use jetcarp_core::library::{AntQuery, Library};
use rhai::{Array, Dynamic, Engine, EvalAltResult, INT, Map};
use serde_json::Value;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

type R<T> = Result<T, Box<EvalAltResult>>;

fn e(s: String) -> Box<EvalAltResult> {
    s.into()
}

fn map_json(m: &Map) -> R<Value> {
    to_json(&Dynamic::from_map(m.clone()))
}

const SEARCH_KEYS: &[&str] = &[
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

fn list(v: &Value, key: &str) -> Result<Vec<String>, String> {
    Ok(match v.get(key) {
        None | Some(Value::Null) => vec![],
        Some(Value::String(s)) => s
            .split([';', ',', ' '])
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

fn size(v: &Value, key: &str, d: (u32, u32)) -> Result<(u32, u32), String> {
    Ok(match v.get(key) {
        None | Some(Value::Null) => d,
        Some(Value::Number(n)) => {
            let n = n
                .as_u64()
                .ok_or(format!("`{key}` must be a whole number"))? as u32;
            (n, n)
        }
        Some(Value::String(s)) => crate::Screen::parse_size(s)?,
        Some(Value::Array(a)) if a.len() == 2 => (
            a[0].as_u64().ok_or(format!("`{key}` must be [a, b]"))? as u32,
            a[1].as_u64().ok_or(format!("`{key}` must be [a, b]"))? as u32,
        ),
        Some(x) => {
            return Err(format!(
                "`{key}` must be [a, b], \"AxB\", or a number, got {x}"
            ));
        }
    })
}

/// The search spec from a script map.
pub(crate) fn spec_of(v: &Value) -> Result<AntSpec, String> {
    let d = AntSpec::default();
    let t = list(v, "tilings")?.join(",");
    let alphabet = get_str(v, "alphabet")?.map(String::from);
    let strings = match v.get("strings") {
        None | Some(Value::Null) => None,
        Some(Value::Bool(false)) => None,
        Some(Value::Number(n)) => {
            let n = n.as_u64().ok_or("`strings` is a length or [min, max]")? as usize;
            Some((n, n))
        }
        Some(_) => Some(size(v, "strings", (2, 6)).map(|(a, b)| (a as usize, b as usize))?),
    }
    .map(|(min, max)| Strings {
        min,
        max,
        alphabet: alphabet.clone(),
    });
    let tables = match v.get("tables") {
        None | Some(Value::Null) => None,
        Some(_) => {
            let (n, k) = size(v, "tables", (2, 2))?;
            Some(Tables {
                states: n as u8,
                colors: k as u8,
                alphabet: alphabet.clone(),
                all_max: get_u64(v, "all_max")?.unwrap_or(1_000_000),
                sample: get_u64(v, "sample")?.unwrap_or(10_000),
                seed: get_u64(v, "seed")?.unwrap_or(1),
            })
        }
    };
    let rules = list(v, "rules")?;
    let (strings, tables) = if strings.is_none() && tables.is_none() && rules.is_empty() {
        (d.strings.clone(), None)
    } else {
        (strings, tables)
    };
    let soup = match get_u64(v, "soups")? {
        None | Some(0) => None,
        Some(n) => {
            let (w, h) = size(v, "size", (8, 8))?;
            Some(Soups {
                w,
                h,
                density: get_f64(v, "density")?.unwrap_or(0.5),
                soups: n,
                seed: get_u64(v, "seed")?.unwrap_or(1),
            })
        }
    };
    Ok(AntSpec {
        tilings: jetcarp_core::library::tiling_list(if t.trim().is_empty() { "square" } else { &t })?,
        rules,
        strings,
        tables,
        soup,
        steps: get_u64(v, "steps")?.unwrap_or(d.steps),
        max_period: get_u64(v, "max_period")?.unwrap_or(d.max_period),
        symmetry: get_bool(v, "symmetry")?.unwrap_or(true),
    })
}

/// `ants(#{ ... })` / `lib.ants(#{ ... })`: run (or resume) a search job on the script's thread
/// until it is done, `limit` runs have run, or the script is cancelled. Returns the progress.
pub(crate) fn ants_into(lib: &Library, v: &Value, ctx: &Arc<Ctx>) -> Result<Value, String> {
    check_keys(v, SEARCH_KEYS, "ants")?;
    let jid = match get_u64(v, "resume")? {
        Some(j) => j as i64,
        None => job::start(lib, &spec_of(v)?)?,
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
    let opts = AntOptions {
        threads: get_u64(v, "threads")?.unwrap_or(0) as usize,
        stop,
        limit: get_u64(v, "limit")?,
        on_progress: Some(Arc::new(move |p: &job::AntProgress| {
            let el = t0.elapsed().as_secs_f64();
            c.progress(crate::Progress {
                label: format!("ants {}", p.job),
                done: p.runs_done,
                total: p.runs_total,
                item: p
                    .newest
                    .first()
                    .map(|f| format!("{} {} {}", f.rule, f.tiling, f.outcome.name()))
                    .unwrap_or_default(),
                elapsed: el,
                eta: (p.runs_per_s > 0.0 && p.runs_total > p.runs_done)
                    .then(|| (p.runs_total - p.runs_done) as f64 / p.runs_per_s),
            });
        })),
        ..Default::default()
    };
    let res = job::run_job(lib, jid, &opts);
    done.store(true, Ordering::Relaxed);
    let _ = watcher.join();
    let p = res?;
    ctx.log(format!("{} ({:.1}s)", p.line(), t0.elapsed().as_secs_f64()));
    serde_json::to_value(&p).map_err(|e| e.to_string())
}

/// A search's saved progress with its spec.
pub(crate) fn ant_status(lib: &Library, job: i64) -> Result<Value, String> {
    let (spec, p) = job::status(lib, job)?;
    let mut v = serde_json::to_value(&p).map_err(|e| e.to_string())?;
    v["spec"] = serde_json::to_value(&spec).map_err(|e| e.to_string())?;
    Ok(v)
}

/// `ant_run(rule, #{ tiling, steps, max_period, soup: [w, h], density, seed, store })`.
fn ant_run(lib: &Library, rule: &str, v: &Value) -> Result<Value, String> {
    check_keys(
        v,
        &[
            "tiling",
            "steps",
            "max_period",
            "soup",
            "density",
            "seed",
            "store",
            "symmetry",
        ],
        "ant_run",
    )?;
    let m = parse_rule(rule)?;
    let t = jetcarp_core::library::tiling_list(get_str(v, "tiling")?.unwrap_or("square"))?;
    let start = match v.get("soup") {
        None | Some(Value::Null) => Start::Empty,
        Some(_) => {
            let (w, h) = size(v, "soup", (8, 8))?;
            Start::Soup {
                w,
                h,
                density: get_f64(v, "density")?.unwrap_or(0.5),
                state: jetcarp_core::hunt::soup::soup_state(get_u64(v, "seed")?.unwrap_or(1), 0),
            }
        }
    };
    let lim = Limits {
        steps: get_u64(v, "steps")?.unwrap_or(1_000_000),
        max_period: get_u64(v, "max_period")?.unwrap_or(20_000),
        symmetry: get_bool(v, "symmetry")?.unwrap_or(true),
    };
    let row = job::run_one(&m, &t[0], &start, &lim)?;
    if get_bool(v, "store")?.unwrap_or(false) {
        lib.ants_flush(0, std::slice::from_ref(&row), None, None)?;
    }
    serde_json::to_value(&row).map_err(|e| e.to_string())
}

fn query_of(v: &Value) -> Result<AntQuery, String> {
    check_keys(
        v,
        &["outcome", "tiling", "rule", "job", "sort", "limit"],
        "ant_list",
    )?;
    Ok(AntQuery {
        outcome: get_str(v, "outcome")?.map(Outcome::parse).transpose()?,
        tiling: get_str(v, "tiling")?.map(String::from),
        rule: get_str(v, "rule")?.map(String::from),
        job: get_u64(v, "job")?.map(|j| j as i64),
        sort: get_str(v, "sort")?.map(String::from),
        limit: Some(get_u64(v, "limit")?.unwrap_or(100) as usize),
    })
}

fn default_lib(c: &Ctx) -> Library {
    match &c.opts.library {
        Some(p) => Library::open(p.clone()),
        None => Library::open_default(),
    }
}

pub(crate) fn register(eng: &mut Engine, ctx: &Arc<Ctx>) {
    let c = ctx.clone();
    eng.register_fn("ants", move |l: &mut LibHandle, m: Map| -> R<Map> {
        let v = map_json(&m)?;
        Ok(from_json(&ants_into(&l.0, &v, &c).map_err(e)?).cast::<Map>())
    });
    let c = ctx.clone();
    eng.register_fn("ants", move |m: Map| -> R<Map> {
        let v = map_json(&m)?;
        Ok(from_json(&ants_into(&default_lib(&c), &v, &c).map_err(e)?).cast::<Map>())
    });
    let c = ctx.clone();
    eng.register_fn("ant_run", move |rule: &str| -> R<Map> {
        Ok(from_json(&ant_run(&default_lib(&c), rule, &Value::Null).map_err(e)?).cast::<Map>())
    });
    let c = ctx.clone();
    eng.register_fn("ant_run", move |rule: &str, m: Map| -> R<Map> {
        let v = map_json(&m)?;
        Ok(from_json(&ant_run(&default_lib(&c), rule, &v).map_err(e)?).cast::<Map>())
    });
    eng.register_fn("ant", |l: &mut LibHandle, id: &str| -> R<Map> {
        let r = l.0.ant(id).map_err(e)?;
        Ok(from_json(&serde_json::to_value(&r).unwrap_or_default()).cast::<Map>())
    });
    eng.register_fn("ant_list", |l: &mut LibHandle, m: Map| -> R<Array> {
        let q = query_of(&map_json(&m)?).map_err(e)?;
        let rows = l.0.ants(&q).map_err(e)?;
        Ok(rows
            .iter()
            .map(|r| from_json(&serde_json::to_value(r).unwrap_or_default()))
            .collect())
    });
    eng.register_fn("ant_census", |l: &mut LibHandle| -> R<Array> {
        let c = l.0.ant_census(None).map_err(e)?;
        Ok(c.iter()
            .map(|r| from_json(&serde_json::to_value(r).unwrap_or_default()))
            .collect())
    });
    eng.register_fn("ant_status", |l: &mut LibHandle, job: INT| -> R<Map> {
        Ok(from_json(&ant_status(&l.0, job).map_err(e)?).cast::<Map>())
    });
    eng.register_fn("ant_stop", |l: &mut LibHandle, job: INT| -> R<()> {
        job::request_stop(&l.0, job).map_err(e)
    });
    eng.register_fn("ant_jobs", |l: &mut LibHandle| -> R<Array> {
        let mut out = Array::new();
        for j in
            l.0.jobs()
                .map_err(e)?
                .into_iter()
                .filter(|j| j.kind == job::KIND)
        {
            out.push(from_json(&ant_status(&l.0, j.id).map_err(e)?));
        }
        Ok(out)
    });
}
