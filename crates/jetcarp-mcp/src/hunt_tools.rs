//! MCP tool for the fast soup hunter (`jetcarp_core::hunt`): `hunt_start` runs a hunt in the
//! background, on the job runner (`jetcarp_core::jobs`; its handle is in the process's
//! registry). Hunts are catalog jobs, so one started here, by `jetcarp hunt`, or by a script is
//! watched and stopped with `job_status` and `job_stop` (`job_tools.rs`) from any of them
//! (stopping one in another process takes effect at its next checkpoint).

use crate::tools::{Res, ToolOutput, check_unknown, int, string, tool};
use jetcarp_api::args;
use jetcarp_core::hunt::job::{self, HuntProgress, HuntSpec};
use jetcarp_core::hunt::soup::{SoupShape, SoupSpec};
use jetcarp_core::jobs::{RunOptions, registry};
use jetcarp_core::library::Library;
use serde_json::{Value, json};

pub const NAMES: &[&str] = &["hunt_start"];

pub fn handles(name: &str) -> bool {
    NAMES.contains(&name)
}

fn lib() -> Library {
    Library::open_default()
}

pub fn definitions() -> Vec<Value> {
    vec![tool(
        "hunt_start",
        "Start a soup hunt",
        false,
        "Starts a fast soup hunt in the background and returns at once with its job id (poll job_status, stop with job_stop). Small random soups (default 16x16 unit cells at density 0.5) run on an unbounded plane, bit-parallel, until they settle; ships that fly away are cut out and counted; every object is classified, and new patterns are verified and stored in the catalog (library_list shows them; class gun for stationary guns). Thousands of soups per second for Life on squares. Results do not depend on threads. The hunt is a resumable catalog job: {\"resume\": 3} continues job 3. Rules: B/S and Generations rules without B0 (a V suffix means edge neighbours); tilings: any periodic tiling (family id, catalog id, or a set such as \"archimedean\"). Examples: {\"rules\": [\"B3/S23\"], \"soups\": 100000}, {\"rules\": [\"B2/S34\", \"B24/S35\"], \"tilings\": \"hex\", \"soups\": 0} (0 = until stopped).",
        json!({"type": "object", "properties": {
                "rules": {"type": ["array", "string"], "items": {"type": "string"}, "description": "Rules (default [\"B3/S23\"])."},
                "tilings": {"type": ["array", "string"], "items": {"type": "string"}, "description": "Tilings (default \"square\")."},
                "nb": {"type": "string", "enum": ["touch", "edge"], "description": "Neighbourhood for every rule (default: the notation decides)."},
                "soups": {"type": "integer", "minimum": 0, "description": "Soups per rule and tiling (default 10000; 0 = until stopped)."},
                "size": {"type": "string", "description": "Soup size WxH in unit cells (default \"16x16\")."},
                "density": {"type": "number", "minimum": 0, "maximum": 1, "description": "Default 0.5."},
                "shape": {"type": "string", "enum": ["rect", "disc"], "description": "Soup shape (default rect)."},
                "seed": {"type": "integer", "minimum": 0, "description": "Default 1; soup i uses an RNG state from (seed, i)."},
                "gens": {"type": "integer", "minimum": 1, "description": "Generations before an unsettled soup is censused as it is (default 20000)."},
                "max_period": {"type": "integer", "minimum": 1, "maximum": 1024, "description": "Longest period looked for (default 64)."},
                "threads": {"type": "integer", "minimum": 0, "description": "Worker threads (default 0 = all cores)."},
                "resume": {"type": "integer", "minimum": 1, "description": "Continue this hunt job instead of starting a new one."},
            }, "additionalProperties": false}),
    )]
}

pub fn call(name: &str, args: &Value) -> Res {
    match name {
        "hunt_start" => start(args),
        _ => Err(format!("unknown tool {name}")),
    }
}

fn list(args: &Value, key: &str) -> Result<Vec<String>, String> {
    Ok(match args.get(key) {
        None | Some(Value::Null) => vec![],
        Some(Value::String(s)) => s
            .split([',', ';'])
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
            .collect::<Result<_, _>>()?,
        Some(v) => return Err(format!("`{key}` must be a list of text, got {v}")),
    })
}

fn summary(p: &HuntProgress) -> Value {
    let mut v = serde_json::to_value(p).unwrap_or_default();
    v["line"] = json!(p.line());
    v
}

fn start(args: &Value) -> Res {
    check_unknown(
        args,
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
            "threads",
            "resume",
        ],
    )?;
    let l = lib();
    let jid = match int(args, "resume")? {
        Some(j) => {
            if registry().is_running(&l, j) {
                return Err(format!("hunt {j} is already running here"));
            }
            job::status(&l, j)?;
            j
        }
        None => {
            let d = HuntSpec::default();
            let mut rules = list(args, "rules")?;
            if rules.is_empty() {
                rules = d.rules.clone();
            }
            let t = list(args, "tilings")?.join(",");
            let (w, h) = match string(args, "size")? {
                None => (d.soup.w, d.soup.h),
                Some(s) => args::size_pair(s).map_err(|_| format!("bad size '{s}'"))?,
            };
            let soups = int(args, "soups")?.unwrap_or(10_000).max(0) as u64;
            let spec = HuntSpec {
                rules,
                tilings: args::tilings_or_square(&t)?,
                neighborhood: match string(args, "nb")? {
                    None => None,
                    Some(x) => Some(
                        args::nb_word(x)
                            .ok_or_else(|| format!("`nb` is touch or edge, not {x}"))?,
                    ),
                },
                soups: (soups > 0).then_some(soups),
                soup: SoupSpec {
                    w,
                    h,
                    density: args
                        .get("density")
                        .and_then(|v| v.as_f64())
                        .unwrap_or(d.soup.density),
                    shape: match string(args, "shape")? {
                        Some(s) => SoupShape::parse(s)?,
                        None => SoupShape::Rect,
                    },
                },
                seed: int(args, "seed")?
                    .map(|x| x.max(0) as u64)
                    .unwrap_or(d.seed),
                max_gens: int(args, "gens")?
                    .map(|x| x.max(1) as u64)
                    .unwrap_or(d.max_gens),
                max_period: int(args, "max_period")?
                    .map(|x| x.clamp(1, 1024) as u64)
                    .unwrap_or(d.max_period),
                pop_cap: 0,
                margin: 0,
                store: d.store,
            };
            job::start(&l, &spec)?
        }
    };
    let opts = RunOptions {
        threads: int(args, "threads")?.unwrap_or(0).max(0) as usize,
        ..Default::default()
    };
    registry().start(l.clone(), jid, opts)?;
    let p: HuntProgress = registry()
        .with(&l, jid, |h| h.progress_as())
        .unwrap_or_default();
    Ok(ToolOutput::json(&json!({
        "job": jid,
        "status": "running",
        "catalog": l.path().display().to_string(),
        "progress": summary(&p),
        "note": "The hunt runs in the background: poll job_status {\"job\": ...}; job_stop stops it (resumable with hunt_start {\"resume\": job}).",
    })))
}

/// Stop a hunt running in this process; false when it is not running here.
pub(crate) fn stop_here(jid: i64) -> bool {
    registry().stop(&lib(), jid)
}

/// The progress as `job_status` shows it.
pub(crate) fn status_json(l: &Library, jid: i64) -> Result<Value, String> {
    Ok(summary(&progress_of(l, jid)?))
}

fn progress_of(l: &Library, jid: i64) -> Result<HuntProgress, String> {
    if let Some((finished, p)) = registry().with(l, jid, |h| {
        (h.is_finished(), h.progress_as::<HuntProgress>())
    }) && (!finished || p.status != "running")
    {
        return Ok(p);
    }
    job::status(l, jid).map(|x| x.1)
}
