//! Jobs and rule spaces for scripts: `lib.jobs(#{ ... })` (the catalog's jobs as rows),
//! `lib.census_rows(job)`, and `rule_neighbours(rule, #{ tiling, nb })` (see `help.md`). The
//! searches themselves (`hunt`, `census`, `ants`, ...) are `crate::search`, through the operation
//! `search.start`.

use jetcarp_api::args::{self, check_keys, get_str};
use jetcarp_core::Nb;
use jetcarp_core::census;
use jetcarp_core::pattern::tiling_list;
use jetcarp_core::rules::Rule;
use jetcarp_core::topology::{SizeSpec, TilingSpec, Topology};
use jetcarp_store::{JobStatus, Library};
use serde_json::{Value, json};

fn status_name(s: JobStatus) -> &'static str {
    match s {
        JobStatus::Running => "running",
        JobStatus::Paused => "paused",
        JobStatus::Done => "done",
        JobStatus::Failed => "failed",
    }
}

/// `lib.jobs(#{ kind, label, prefix, status })`: jobs (oldest first) as `#{ id, kind, status,
/// label, spec, progress, created, updated, finished }`.
pub fn jobs(lib: &Library, v: &Value) -> Result<Value, String> {
    check_keys(v, &["kind", "label", "prefix", "status"], "jobs")?;
    let kind = get_str(v, "kind")?;
    let label = get_str(v, "label")?;
    let prefix = get_str(v, "prefix")?;
    let status = get_str(v, "status")?;
    let mut out = Vec::new();
    for j in lib.jobs()? {
        if j.id == 0
            || kind.is_some_and(|k| j.kind != k)
            || label.is_some_and(|l| j.label() != Some(l))
            || prefix.is_some_and(|p| !j.label().is_some_and(|l| l.starts_with(p)))
            || status.is_some_and(|s| status_name(j.status) != s)
        {
            continue;
        }
        out.push(json!({
            "id": j.id,
            "kind": j.kind,
            "status": status_name(j.status),
            "label": j.label(),
            "spec": j.spec,
            "progress": j.progress,
            "created": j.created,
            "updated": j.updated,
            "finished": j.finished,
        }));
    }
    Ok(Value::Array(out))
}

/// A tiling from text (`"spectre"`, `"k2-01"`) or a tiling map (as settings and hunts store
/// them).
pub fn one_tiling(v: &Value) -> Result<TilingSpec, String> {
    match v {
        Value::String(s) => tiling_list(s)?
            .into_iter()
            .next()
            .ok_or_else(|| format!("no tiling in {s:?}")),
        Value::Object(_) => serde_json::from_value(v.clone()).map_err(|e| format!("tiling: {e}")),
        x => Err(format!("a tiling is text or a map, not {x}")),
    }
}

fn nb_of(v: &Value) -> Result<Option<Nb>, String> {
    let Some(x) = get_str(v, "nb")? else {
        return Ok(None);
    };
    args::nb_word(x)
        .map(Some)
        .ok_or_else(|| format!("`nb` is \"touch\" or \"edge\", not {x}"))
}

/// `lib.census_rows(job)`: a census job's rule summaries (without the per-soup results), in
/// rule order.
pub fn census_rows(lib: &Library, job: i64) -> Result<Value, String> {
    let rows = lib.census_rows(job)?;
    Ok(Value::Array(
        rows.into_iter()
            .map(|mut r| {
                if let Some(o) = r.as_object_mut() {
                    o.remove("results");
                }
                r
            })
            .collect(),
    ))
}

/// `rule_neighbours(rule, #{ tiling, nb })`: every rule one birth or survival count away (B0
/// left out), with digits up to the tiling's largest neighbour count. Edge-neighbour rules keep
/// their `V`.
pub fn rule_neighbours(rule: &str, v: &Value) -> Result<Value, String> {
    check_keys(v, &["tiling", "nb"], "rule_neighbours")?;
    let mut tiling = match v.get("tiling") {
        None | Some(Value::Null) => TilingSpec::new("square"),
        Some(t) => one_tiling(t)?,
    };
    let (r, hint) = Rule::parse(rule)?;
    let nb = nb_of(v)?.or(hint).unwrap_or_default();
    if tiling.size.is_none() && !args::is_aperiodic(&tiling) {
        tiling.size = Some(SizeSpec::Rect { w: 8, h: 8 });
    }
    let topo = Topology::build(&tiling)?;
    let max = census::max_degree(&topo, nb) as u32;
    let birth: Vec<u32> = (1..=max).collect();
    let survive: Vec<u32> = (0..=max).collect();
    let out = jetcarp_core::rules::rule_neighbours(&r.notation(), &birth, &survive)?;
    Ok(Value::Array(
        out.into_iter()
            .map(|n| {
                Value::String(if nb == Nb::Edge && !n.ends_with('V') {
                    format!("{n}V")
                } else {
                    n
                })
            })
            .collect(),
    ))
}
