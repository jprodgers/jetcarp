//! `library_curate {"compounds": {...}}`: the library pass that finds stored compounds (several
//! independent objects stored as one pattern: two gliders side by side, a pulsar beside a
//! blinker) and, with `apply`, stores their parts instead (`library.compounds`).
//!
//! It is a maintenance pass over records already stored, so it sits with the other curation
//! rather than with the searches. It runs as a resumable `compounds` job in the call, for at most
//! [`MAX_MINUTES`] (default [`DEFAULT_MINUTES`]); `job` carries a paused pass on, and job_status
//! follows it like any job. Its report goes beside the library (`reports/compounds-<job>.jsonl`).

use crate::schema;
use crate::tools::{Call, Res, ToolOutput, ctx, err};
use jetcarp_api::op::{Op, Report, input_schema};
use jetcarp_api::ops::library::{CompoundsIn, LibraryCompounds};
use serde_json::{Value, json};

/// Minutes one call runs the pass when none are given.
pub const DEFAULT_MINUTES: f64 = 5.0;
/// Most minutes one call runs it.
pub const MAX_MINUTES: f64 = 10.0;

/// The `compounds` argument's schema: the operation's input without the report file and
/// threads (the report goes beside the library, inside the folder rules).
pub(crate) fn schema() -> Option<Value> {
    let mut s = schema::without(input_schema::<CompoundsIn>(), &["report", "threads"]);
    s = schema::op(
        s,
        Some(
            "Find stored patterns that are several independent objects, as a resumable job: {} lists them with their parts (nothing changes); {\"apply\": true} stores each compound's parts in its place (credited with its sightings) and removes it, or keeps it tagged `compound` when curated.",
        ),
    );
    s = schema::describe(
        s,
        "minutes",
        &format!(
            "Stop after this many minutes (default {DEFAULT_MINUTES}, at most {MAX_MINUTES}), resumable with `job`."
        ),
    );
    Some(s)
}

pub(crate) fn run(c: &mut Call, v: &Value) -> Res {
    let mut v = match v {
        Value::Object(_) => v.clone(),
        Value::String(s) => serde_json::from_str(s)
            .map_err(|e| format!("library_curate compounds: not a JSON object ({e})"))?,
        x => {
            return Err(format!(
                "library_curate: `compounds` holds an object, not {x}"
            ));
        }
    };
    for k in ["report", "threads"] {
        if v.get(k).is_some() {
            return Err(format!(
                "library_curate compounds: `{k}` is not taken here (the report goes beside the library)"
            ));
        }
    }
    let minutes = v
        .get("minutes")
        .and_then(|m| m.as_f64().or_else(|| m.as_str()?.trim().parse().ok()))
        .unwrap_or(DEFAULT_MINUTES)
        .clamp(0.01, MAX_MINUTES);
    v["minutes"] = json!(minutes);
    let input: CompoundsIn = schema::read(
        &input_schema::<CompoundsIn>(),
        "library_curate compounds",
        v,
    )?;
    let out = LibraryCompounds::run(ctx(), input, &c.callbacks()).map_err(err)?;
    let text = out
        .text()
        .replace(
            &format!("resume with --job {}", out.job),
            &format!("carry on with {{\"compounds\": {{\"job\": {}}}}}", out.job),
        )
        .replace("--apply splits them", "{\"apply\": true} splits them");
    Ok(ToolOutput::text(text))
}
