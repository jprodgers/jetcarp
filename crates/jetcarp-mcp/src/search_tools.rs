//! `search_runs`: an explore search's stored runs (`search.runs`), a contact sheet of them
//! (`search.sheet`), one run's scene (`search.run_scene`), one configuration of your own tried now
//! (`search.try`), and a verdict on a run (`search.review`). A search is named by its `job`
//! (default: the newest explore job).

use crate::schema::{self, object};
use crate::tools::{Call, Res, Tool, ToolOutput, ctx, err};
use jetcarp_api::op::{self, Op, Quiet, input_schema};
use jetcarp_api::ops::explore::{
    ReviewIn, RunIn, RunsIn, SearchReview, SearchRunScene, SearchRuns, SearchSheet, SearchTry,
    SheetIn, TryIn,
};
use serde_json::{Value, json};

const MAX_ROWS: usize = 200;
const MAX_SHEET: usize = 36;
const ACTIONS: &[&str] = &["sheet", "scene", "try", "review"];

pub(crate) fn tools() -> Vec<Tool> {
    vec![Tool {
        name: "search_runs",
        title: "An explore search's runs",
        read_only: false,
        destructive: false,
        description: || {
            "Reads an explore search's stored runs (search_start {\"explore\": ...} makes one; `job` picks it, default the newest). With no action: its runs, best first, with their measures (fate, settle time, period, density, activity, movers, score: about 0 to 3, higher is more interesting), filtered by family, tiling, fate, min_score, min_movers, stage or reviewed. Or one action: `sheet`, a labelled picture of runs at several moments, to judge by eye ({\"sheet\": {\"ids\": [12, 40, 7]}}); `scene`, one run's scene JSON for world_new ({\"scene\": {\"run\": 12}}); `try`, a configuration of your own run now, with its measures and a picture ({\"try\": {\"tiling\": \"hex\", \"rule\": \"B2/S34\", \"seed\": \"random:0.3\"}}); or `review`, your verdict on a run ({\"review\": {\"run\": 12, \"verdict\": \"keep\", \"note\": \"slow gliders\"}}), which feeds what is published and the score. The sealed runs are also Parquet files (runs/part-*.parquet in the search's folder) for DuckDB, Polars or pyarrow.".into()
        },
        schema: || {
            let mut p = schema::props(&input_schema::<RunsIn>());
            let mut limit = p["limit"].clone();
            limit["description"] = json!(format!(
                "Most runs to return (default 40, at most {MAX_ROWS})."
            ));
            p.insert("limit".into(), limit);
            p.insert(
                "sheet".into(),
                schema::without(
                    schema::op(
                        input_schema::<SheetIn>(),
                        Some(&format!("A contact sheet of runs, returned as a picture (at most {MAX_SHEET} runs).")),
                    ),
                    &["job", "max"],
                ),
            );
            p.insert(
                "scene".into(),
                schema::without(
                    schema::op(input_schema::<RunIn>(), Some("One run's scene JSON.")),
                    &["job"],
                ),
            );
            p.insert(
                "try".into(),
                schema::op(
                    input_schema::<TryIn>(),
                    Some("Try one configuration now (CPU), returning its measures and a picture."),
                ),
            );
            p.insert(
                "review".into(),
                schema::without(
                    schema::op(
                        input_schema::<ReviewIn>(),
                        Some("Record a verdict on a run."),
                    ),
                    &["job", "from", "rater"],
                ),
            );
            object(p, &[])
        },
        run: search_runs,
    }]
}

/// An action's input, with the tool's `job` put in.
fn part(a: &Value, key: &str, with_job: bool) -> Result<Value, String> {
    let mut v = match &a[key] {
        Value::Object(_) => a[key].clone(),
        Value::String(s) => serde_json::from_str(s)
            .map_err(|e| format!("search_runs {key}: not a JSON object ({e})"))?,
        v => return Err(format!("search_runs: `{key}` holds an object, not {v}")),
    };
    if with_job && let Some(j) = a.get("job").filter(|j| !j.is_null()) {
        v["job"] = j.clone();
    }
    Ok(v)
}

fn search_runs(c: &mut Call) -> Res {
    let a = c.args;
    let Some(which) = schema::one_of("search_runs", a, ACTIONS)? else {
        let mut q: RunsIn = schema::read(&input_schema::<RunsIn>(), "search_runs", a.clone())?;
        q.limit = Some(q.limit.unwrap_or(40).clamp(1, MAX_ROWS));
        let r = SearchRuns::run(ctx(), q, &Quiet).map_err(err)?;
        return Ok(ToolOutput::json(
            &json!({"job": r.job, "runs": op::to_value(&r.runs)}),
        ));
    };
    let filters: Vec<String> = schema::props(&input_schema::<RunsIn>())
        .keys()
        .filter(|k| *k != "job" && a.get(k.as_str()).is_some_and(|v| !v.is_null()))
        .cloned()
        .collect();
    if !filters.is_empty() {
        return Err(format!(
            "search_runs: the filters (`{}`) are for listing runs, not with `{which}`",
            filters.join("`, `")
        ));
    }
    match which {
        "sheet" => {
            let mut s: SheetIn = schema::read(
                &input_schema::<SheetIn>(),
                "search_runs sheet",
                part(a, "sheet", true)?,
            )?;
            s.max = Some(MAX_SHEET);
            if let Some(ids) = &mut s.ids {
                ids.truncate(MAX_SHEET);
            }
            s.px = s.px.clamp(64, 400);
            s.cols = s.cols.clamp(1, 20);
            let out = SearchSheet::run(ctx(), s, &Quiet).map_err(err)?;
            let mut text = format!(
                "{} runs of explore job {} ({} x {} px)",
                out.runs.len(),
                out.job,
                out.width,
                out.height
            );
            if let Some(p) = &out.saved {
                text.push_str(&format!("; saved to {p}"));
            }
            Ok(ToolOutput::with_png(text, &out.bytes))
        }
        "scene" => {
            let r: RunIn = schema::read(
                &input_schema::<RunIn>(),
                "search_runs scene",
                part(a, "scene", true)?,
            )?;
            let out = SearchRunScene::run(ctx(), r, &Quiet).map_err(err)?;
            let mut o = ToolOutput::json(&out.scene);
            if let Some(p) = &out.saved {
                o.content
                    .insert(0, json!({"type": "text", "text": format!("Saved to {p}.")}));
            }
            Ok(o)
        }
        "try" => {
            if a.get("job").is_some_and(|v| !v.is_null()) {
                return Err(
                    "search_runs: `try` runs a configuration of your own; leave out `job`".into(),
                );
            }
            let mut t: TryIn = schema::read(
                &input_schema::<TryIn>(),
                "search_runs try",
                part(a, "try", false)?,
            )?;
            t.cells = Some(t.cells.unwrap_or(16_000).clamp(100, 100_000));
            t.gens = Some(t.gens.unwrap_or(1024).clamp(1, 8000));
            t.px = Some(t.px.unwrap_or(420).clamp(64, 800));
            let out = SearchTry::run(ctx(), t, &Quiet).map_err(err)?;
            let facts = json!({
                "family": out.family, "fate": out.fate, "score": out.score, "category": out.category,
                "settle": out.settle, "period": out.period, "density": out.density,
                "activity": out.activity, "movers": out.movers, "mover_speed": out.mover_speed,
                "smooth": out.smooth, "entropy": out.entropy,
            });
            Ok(ToolOutput::with_png(
                serde_json::to_string_pretty(&facts).unwrap_or_default(),
                &out.png,
            ))
        }
        _ => {
            let mut v = part(a, "review", true)?;
            for k in ["from", "rater"] {
                if v.get(k).is_some() {
                    return Err(format!("search_runs review: no `{k}` here"));
                }
            }
            v["rater"] = json!("model");
            let r: ReviewIn = schema::read(&input_schema::<ReviewIn>(), "search_runs review", v)?;
            if r.run.is_none() || r.verdict.is_none() {
                return Err(
                    "search_runs review: give `run` and `verdict` (keep, maybe, or drop)".into(),
                );
            }
            let (run, verdict) = (r.run, r.verdict.clone());
            let out = SearchReview::run(ctx(), r, &Quiet).map_err(err)?;
            Ok(ToolOutput::text(format!(
                "recorded {} for run {} of explore job {}",
                verdict.unwrap_or_default(),
                run.unwrap_or_default(),
                out.job
            )))
        }
    }
}
