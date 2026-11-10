//! Labelled jobs, censuses as jobs, and rule-space helpers for scripts: `lib.jobs(#{ ... })`,
//! `census(#{ ... })` / `lib.census(#{ ... })`, `lib.census_rows(job)`, and
//! `rule_neighbours(rule, #{ tiling, nb })` (see `help.md`).
//!
//! A `label` on `hunt`, `census`, and `lib.search` names the job; called again with the same
//! label, the job already in the catalog is resumed (or, when done, returned as it is) instead of
//! a new one being started. That is what makes a script of labelled jobs resumable after a
//! reboot.

use jetcarp_api::args::{self, check_keys, get_f64, get_str, get_u64};
use jetcarp_core::Nb;
use jetcarp_core::census::{self, CensusJobOptions, CensusProgress, CensusSpec};
use jetcarp_core::library::{JobStatus, Library, tiling_list};
use jetcarp_core::rules::Rule;
use jetcarp_core::topology::{SizeSpec, TilingSpec, Topology};
use serde_json::{Value, json};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

/// The job of this kind with the label in `v` (`label`), if one exists: `(id, done)`.
pub fn labelled(lib: &Library, kind: &str, v: &Value) -> Result<Option<(i64, bool)>, String> {
    let Some(label) = get_str(v, "label")? else {
        return Ok(None);
    };
    Ok(lib
        .jobs_labelled(label)?
        .into_iter()
        .rev()
        .find(|j| j.kind == kind)
        .map(|j| (j.id, j.status == JobStatus::Done)))
}

/// Give a new job the label in `v`, if any.
pub fn set_label(lib: &Library, job: i64, v: &Value) -> Result<(), String> {
    if let Some(l) = get_str(v, "label")? {
        lib.set_job_label(job, l)?;
    }
    Ok(())
}

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

fn texts(v: &Value, key: &str) -> Vec<String> {
    match v.get(key) {
        Some(Value::String(s)) => s
            .split([';', ',', ' '])
            .map(str::trim)
            .filter(|x| !x.is_empty())
            .map(String::from)
            .collect(),
        Some(Value::Array(a)) => a
            .iter()
            .filter_map(|x| x.as_str().map(String::from))
            .collect(),
        _ => vec![],
    }
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
    Ok(match get_str(v, "nb")? {
        None => None,
        Some("touch") => Some(Nb::Touch),
        Some("edge") => Some(Nb::Edge),
        Some(x) => return Err(format!("`nb` is \"touch\" or \"edge\", not {x}")),
    })
}

fn digit_list(v: &Value, key: &str, max: u32, from: u32) -> Result<Vec<u32>, String> {
    let d = match v.get(key) {
        None | Some(Value::Null) => (from..=max).collect(),
        Some(Value::String(s)) => census::parse_digits(s)?,
        Some(Value::Array(a)) => a
            .iter()
            .filter_map(|x| x.as_u64().map(|d| d as u32))
            .collect(),
        Some(x) => return Err(format!("`{key}` must be digits, got {x}")),
    };
    Ok(d.into_iter().filter(|&x| x >= from && x <= max).collect())
}

/// The census spec from a script map: a tiling (with `level` on aperiodic patches, or `size`
/// in cells), the rules (`rules`, or every B/S rule of the digits up to `all_max`, else a
/// seeded `sample`), and the soups.
pub fn census_spec(v: &Value) -> Result<CensusSpec, String> {
    let d = CensusSpec::default();
    let mut tiling = one_tiling(v.get("tiling").unwrap_or(&Value::Null))?;
    if let Some(l) = get_u64(v, "level")? {
        tiling.size = Some(SizeSpec::Level(l as u32));
    } else if let Some(n) = get_u64(v, "cells")? {
        tiling.size = Some(SizeSpec::Cells(n as u32));
    } else if tiling.size.is_none() && args::is_aperiodic(&tiling) {
        tiling.size = Some(SizeSpec::Level(jetcarp_core::aperiodic::default_level(
            &tiling.family,
        )));
    }
    let nb = nb_of(v)?;
    let mut rules = texts(v, "rules");
    if rules.is_empty() {
        let topo = Topology::build(&tiling)?;
        let max = census::max_degree(&topo, nb.unwrap_or(Nb::Touch)) as u32;
        let birth = digit_list(v, "birth", max, 1)?;
        let survive = digit_list(v, "survive", max, 0)?;
        let states = get_u64(v, "states")?.map(|s| s as u8).filter(|&s| s > 2);
        let bits = (birth.len() + survive.len()) as u32;
        let all_max = get_u64(v, "all_max")?.unwrap_or(1 << 15);
        let seed = get_u64(v, "seed")?.unwrap_or(d.seed);
        rules = if bits < 63 && (1u64 << bits) <= all_max {
            census::all_bs_rules(&birth, &survive, states)?
        } else {
            census::sample_bs_rules(
                &birth,
                &survive,
                states,
                get_u64(v, "sample")?.unwrap_or(2000) as usize,
                seed ^ 0x5eed_ce05,
            )
        };
        if nb == Some(Nb::Edge) {
            for r in &mut rules {
                r.push('V');
            }
        }
    }
    Ok(CensusSpec {
        tiling,
        neighborhood: nb,
        rules,
        soups: get_u64(v, "soups")?.unwrap_or(d.soups as u64) as u32,
        density: get_f64(v, "density")?.unwrap_or(d.density),
        gens: get_u64(v, "gens")?.unwrap_or(d.gens),
        seed: get_u64(v, "seed")?.unwrap_or(d.seed),
        explode_fraction: get_f64(v, "explode")?.unwrap_or(d.explode_fraction),
        activity_window: d.activity_window,
    })
}

/// `census(#{ ... })` / `lib.census(#{ ... })`: run (or resume) a census job in this catalog,
/// on the script's thread, until it is done, `limit` rules have run, or the script is cancelled.
/// Returns the progress (`#{ job, status, rules_done, rules_total, per_s, elapsed_s }`).
pub(crate) fn census_into(
    lib: &Library,
    v: &Value,
    ctx: &Arc<crate::Ctx>,
) -> Result<Value, String> {
    check_keys(
        v,
        &[
            "tiling", "level", "cells", "nb", "rules", "birth", "survive", "states", "all_max",
            "sample", "soups", "density", "gens", "seed", "explode", "label", "resume", "limit",
            "threads",
        ],
        "census",
    )?;
    let jid = match get_u64(v, "resume")? {
        Some(j) => j as i64,
        None => match labelled(lib, "census", v)? {
            Some((j, _)) => j,
            None => {
                let j = census::start_job(lib, &census_spec(v)?)?;
                set_label(lib, j, v)?;
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
    let opts = CensusJobOptions {
        threads: get_u64(v, "threads")?.unwrap_or(0) as usize,
        stop,
        limit: get_u64(v, "limit")?,
        on_progress: Some(Arc::new(move |p: &CensusProgress| {
            c.progress(crate::Progress {
                label: format!("census {}", p.job),
                done: p.rules_done,
                total: p.rules_total,
                item: String::new(),
                elapsed: t0.elapsed().as_secs_f64(),
                eta: (p.per_s > 0.0 && p.rules_total > p.rules_done)
                    .then(|| (p.rules_total - p.rules_done) as f64 / p.per_s),
            });
        })),
        ..Default::default()
    };
    let res = census::run_job(lib, jid, &opts);
    done.store(true, Ordering::Relaxed);
    let _ = watcher.join();
    let p = res?;
    ctx.log(format!("{} ({:.1}s)", p.line(), t0.elapsed().as_secs_f64()));
    serde_json::to_value(&p).map_err(|e| e.to_string())
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
