//! Ant and turmite bindings (`jetcarp_core::ants`): `ant_run(rule, #{...})` runs one rule, and
//! `lib.ant(id)`, `lib.ant_list(#{...})`, and `lib.ant_census()` read the catalog. Ant searches
//! (`ants(#{...})`, `lib.ant_status(job)`, `lib.ant_stop(job)`, `lib.ant_jobs()`) are jobs:
//! `crate::search`.

use crate::Ctx;
use crate::api::{LibHandle, from_json, to_json};
use jetcarp_api::args::{check_keys, get_bool, get_f64, get_pair, get_str, get_u64};
use jetcarp_core::ants::run::{Limits, Outcome, Start};
use jetcarp_core::ants::search::parse_rule;
use jetcarp_core::library::{AntQuery, Library};
use jetcarp_search::ants::run_one;
use rhai::{Array, Dynamic, Engine, EvalAltResult, Map};
use serde_json::Value;
use std::sync::Arc;

type R<T> = Result<T, Box<EvalAltResult>>;

fn e(s: String) -> Box<EvalAltResult> {
    s.into()
}

fn map_json(m: &Map) -> R<Value> {
    to_json(&Dynamic::from_map(m.clone()))
}

/// A pair of whole numbers (`n`, `"AxB"`, or `[a, b]`) under `key`.
fn size(v: &Value, key: &str, d: (u32, u32)) -> Result<(u32, u32), String> {
    get_pair(v, key, d, ("a", "b"))
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
    let t = jetcarp_core::pattern::tiling_list(get_str(v, "tiling")?.unwrap_or("square"))?;
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
    let row = run_one(&m, &t[0], &start, &lim)?;
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
}
