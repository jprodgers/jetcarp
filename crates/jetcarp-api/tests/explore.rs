//! Explore searches through the operations: `search.start` (here, in the background, carried on),
//! `jobs.status`, `jobs.stop`, `jobs.resume`, `jobs.report`, and `jobs.remove` on them; and their
//! run records: `search.runs`, `search.sheet`, `search.run_scene`, `search.try`,
//! `search.review`, `search.rescore`, `search.clips`, and `quickstart.publish`.

mod common;

use common::{fails, scratch};
use jetcarp_api::{Ctx, ErrorKind, Quiet, call, call_json};
use serde_json::{Value, json};
use std::time::{Duration, Instant};

fn ctx(dir: &std::path::Path) -> Ctx {
    Ctx::new()
        .with_library(Some(dir.join("lib")))
        .with_out_root(Some(dir.join("out")))
}

/// A small, quick search.
fn small() -> Value {
    json!({
        "tilings": "square, hex", "families": ["life_like", "generations", "cyclic"],
        "screen_cells": 400, "screen_gens": 48,
        "confirm_cells": 900, "confirm_gens": 64, "confirm_seeds": 2, "seed": 3,
    })
}

fn screened(v: &Value) -> u64 {
    v["progress"]["screened"].as_u64().unwrap()
}

#[test]
fn explore_searches_start_stop_and_continue_through_the_ops() {
    let dir = scratch("explore-jobs");
    let cx = ctx(&dir);
    let r = call_json(
        &cx,
        "search.start",
        json!({"strategy": "explore", "spec": small(), "budget": {"items": 150}, "threads": 2,
               "label": "t · explore", "gpu": true}),
    )
    .unwrap();
    assert_eq!(
        (&r["kind"], &r["status"], &r["stopped"], &r["created"]),
        (
            &json!("explore"),
            &json!("paused"),
            &json!("budget"),
            &json!(true)
        ),
        "{r}"
    );
    let job = r["job"].as_i64().unwrap();
    let n1 = screened(&r);
    assert!(n1 >= 150, "{r}");
    // Asked for, but this context has no GPU: it ran on the CPU and says why.
    let backend = r["progress"]["backend"].as_str().unwrap();
    assert!(backend.starts_with("cpu only ("), "{backend}");
    assert!(
        r["line"]
            .as_str()
            .unwrap()
            .starts_with(&format!("explore {job}: "))
    );
    // The status says it continues rather than resumes exactly.
    let st = call(&cx, "jobs.status", json!({"job": job}), &Quiet).unwrap();
    assert_eq!(
        (
            &st.value["deterministic"],
            &st.value["resumable"],
            &st.value["unit"]
        ),
        (&json!(false), &json!(true), &json!("runs"))
    );
    assert!(
        st.text.contains("continues it from its archive"),
        "{}",
        st.text
    );
    assert!(st.text.contains("life_like"), "{}", st.text);
    let hunt_like = call_json(
        &cx,
        "search.start",
        json!({"strategy": "hunt", "spec": {"rules": "B3/S23", "soups": 5}}),
    )
    .unwrap();
    let st = call(
        &cx,
        "jobs.status",
        json!({"job": hunt_like["job"].as_i64().unwrap()}),
        &Quiet,
    )
    .unwrap();
    assert_eq!(st.value["deterministic"], true);
    // In the background, stopped from "another front end" by jobs.stop.
    let b = call_json(
        &cx,
        "jobs.resume",
        json!({"job": job, "background": true, "threads": 2}),
    )
    .unwrap();
    assert_eq!(b["mode"], "background");
    let t0 = Instant::now();
    loop {
        let v = call_json(&cx, "jobs.status", json!({"job": job})).unwrap();
        if v["progress"]["screened"].as_u64().unwrap_or(0) > n1 + 50 {
            break;
        }
        assert!(t0.elapsed() < Duration::from_secs(120), "{v}");
        std::thread::sleep(Duration::from_millis(50));
    }
    let s = call_json(&cx, "jobs.stop", json!({"job": job})).unwrap();
    assert_eq!((&s["stopping"], &s["here"]), (&json!(true), &json!(true)));
    let v = call_json(&cx, "jobs.status", json!({"job": job, "wait_s": 60})).unwrap();
    assert_eq!(v["status"], "paused", "{v}");
    let n2 = screened(&v);
    let elites = v["progress"]["elites"].as_u64().unwrap();
    // Carried on here: it loads the saved archive and goes on counting.
    let c = call_json(
        &cx,
        "jobs.resume",
        json!({"job": job, "budget": {"items": 40}, "threads": 2}),
    )
    .unwrap();
    assert_eq!(
        (&c["status"], &c["stopped"]),
        (&json!("paused"), &json!("budget"))
    );
    assert!(screened(&c) >= n2 + 40);
    assert_eq!(c["progress"]["loaded"]["elites"].as_u64().unwrap(), elites);
    // The list shows it with its unit; jobs.report has a section for it.
    let l = call_json(&cx, "jobs.list", json!({"kind": "explore"})).unwrap();
    assert_eq!(l["jobs"].as_array().unwrap().len(), 1);
    assert_eq!(l["jobs"][0]["unit"], "runs");
    let rep = call_json(&cx, "jobs.report", json!({"job": job})).unwrap();
    assert!(
        rep["markdown"]
            .as_str()
            .unwrap()
            .contains("## Explore searches"),
        "{rep}"
    );
    // Bad specs.
    for (spec, want) in [
        (json!({"families": "nonesuch"}), "no rule family 'nonesuch'"),
        (json!({"tilings": "nowhere"}), "no tiling 'nowhere'"),
        (json!({"import": "search.sqlite"}), "unknown field `import`"),
        (json!({"bogus": 1}), "unknown field `bogus`"),
    ] {
        let e = call_json(
            &cx,
            "search.start",
            json!({"strategy": "explore", "spec": spec}),
        )
        .unwrap_err();
        assert_eq!(e.kind, ErrorKind::Invalid, "{e}");
        assert!(e.to_string().contains(want), "{e}");
    }
    // Removed with its results: its folder goes too.
    let folder = std::path::PathBuf::from(c["progress"]["folder"].as_str().unwrap());
    assert!(folder.join("search.sqlite").is_file());
    let rm = call_json(
        &cx,
        "jobs.remove",
        json!({"job": job, "delete_results": true}),
    )
    .unwrap();
    assert_eq!(rm["folder_deleted"], true, "{rm}");
    assert!(!folder.exists());
}

#[test]
fn an_explore_jobs_runs_are_listed_drawn_reviewed_clipped_and_published() {
    let dir = scratch("explore-runs");
    let cx = ctx(&dir);
    // No explore job yet.
    fails(
        call_json(&cx, "search.runs", json!({})),
        ErrorKind::NotFound,
    );
    let r = call_json(
        &cx,
        "search.start",
        json!({"strategy": "explore", "spec": small(), "budget": {"items": 400}, "threads": 2}),
    )
    .unwrap();
    let job = r["job"].as_i64().unwrap();
    // search.runs: best first; the newest explore job by default.
    let v = call_json(&cx, "search.runs", json!({"limit": 10})).unwrap();
    assert_eq!(v["job"], job);
    let runs = v["runs"].as_array().unwrap();
    assert_eq!(runs.len(), 10);
    let scores: Vec<f64> = runs.iter().map(|x| x["score"].as_f64().unwrap()).collect();
    assert!(scores.windows(2).all(|w| w[0] >= w[1]), "{scores:?}");
    assert!(runs.iter().all(|x| x["stage"] == 1));
    let best = runs[0]["id"].as_i64().unwrap();
    let v = call_json(
        &cx,
        "search.runs",
        json!({"job": job, "family": "cyclic", "tiling": "hex", "limit": 5}),
    )
    .unwrap();
    for x in v["runs"].as_array().unwrap() {
        assert_eq!(
            (&x["family"], &x["tiling"]),
            (&json!("cyclic"), &json!("hex"))
        );
    }
    let v = call_json(
        &cx,
        "search.runs",
        json!({"job": job, "stage": 2, "limit": 3}),
    )
    .unwrap();
    assert!(
        v["runs"]
            .as_array()
            .unwrap()
            .iter()
            .all(|x| x["stage"] == 2 && x["parent"].is_i64()),
        "{v}"
    );
    let v = call_json(
        &cx,
        "search.runs",
        json!({"job": job, "ids": [best, -3, best]}),
    )
    .unwrap();
    assert_eq!(v["runs"].as_array().unwrap().len(), 1);
    // SQL over the runs is gone (they are in Parquet segments: DuckDB reads those).
    fails(
        call_json(
            &cx,
            "search.runs",
            json!({"job": job, "sql": "SELECT id FROM runs"}),
        ),
        ErrorKind::Invalid,
    );
    fails(
        call_json(&cx, "search.runs", json!({"job": job, "fate": "exploded"})),
        ErrorKind::Invalid,
    );
    // A sheet, written.
    let png = dir.join("sheet.png");
    let s = call_json(
        &cx,
        "search.sheet",
        json!({"job": job, "ids": [best, best - 1], "px": 64, "cols": 4,
               "png": png.display().to_string()}),
    )
    .unwrap();
    assert!(png.is_file() && s["width"].as_u64().unwrap() > 64, "{s}");
    fails(
        call_json(&cx, "search.sheet", json!({"job": job})),
        ErrorKind::Invalid,
    );
    // The run as a scene.
    let sc = call_json(&cx, "search.run_scene", json!({"job": job, "run": best})).unwrap();
    assert_eq!(sc["scene"]["cas"][0]["rule"], runs[0]["rule"], "{sc}");
    fails(
        call_json(&cx, "search.run_scene", json!({"job": job, "run": -1})),
        ErrorKind::NotFound,
    );
    // A configuration of one's own.
    let t = call(
        &cx,
        "search.try",
        json!({"tiling": "square", "rule": "B3/S23", "cells": 400, "gens": 60, "px": 64}),
        &Quiet,
    )
    .unwrap();
    assert_eq!(t.value["family"], "life_like");
    assert!(t.text.contains("life_like: "), "{}", t.text);
    fails(
        call_json(
            &cx,
            "search.try",
            json!({"tiling": "square", "rule": "nonsense"}),
        ),
        ErrorKind::Invalid,
    );
    // Reviews: one run, then a folder of rating files.
    let rv = call_json(
        &cx,
        "search.review",
        json!({"job": job, "run": best, "verdict": "keep", "note": "busy and smooth"}),
    )
    .unwrap();
    assert_eq!(
        (&rv["recorded"], &rv["counts"]),
        (&json!(1), &json!([["keep", 1]]))
    );
    fails(
        call_json(
            &cx,
            "search.review",
            json!({"job": job, "run": best, "verdict": "great"}),
        ),
        ErrorKind::Invalid,
    );
    fails(
        call_json(
            &cx,
            "search.review",
            json!({"job": job, "run": -1, "verdict": "drop"}),
        ),
        ErrorKind::NotFound,
    );
    let rd = dir.join("ratings");
    std::fs::create_dir_all(&rd).unwrap();
    for (id, v) in [(best - 1, "drop"), (best - 2, "maybe")] {
        std::fs::write(
            rd.join(format!("{id}.json")),
            format!(r#"{{"ts": 1794168214909, "verdict": "{v}"}}"#),
        )
        .unwrap();
    }
    let rv = call_json(
        &cx,
        "search.review",
        json!({"job": job, "from": rd.display().to_string(), "rater": "taste round"}),
    )
    .unwrap();
    assert_eq!(
        (&rv["recorded"], &rv["files"]),
        (&json!(2), &json!(2)),
        "{rv}"
    );
    let v = call_json(&cx, "search.runs", json!({"job": job, "reviewed": "keep"})).unwrap();
    assert_eq!(v["runs"][0]["id"], best);
    // Clips for rating.
    let out = dir.join("clips");
    let c = call_json(
        &cx,
        "search.clips",
        json!({"job": job, "out": out.display().to_string(), "sample": 4, "px": 32,
               "frames": 3}),
    )
    .unwrap();
    // At least one per category, spread over families.
    let clips = c["clips"].as_array().unwrap();
    assert!(!clips.is_empty(), "{c}");
    for x in clips {
        assert!(out.join(x["file"].as_str().unwrap()).is_file());
    }
    assert!(out.join("manifest.json").is_file());
    // Rescored: the same scores (the score has not changed) and the same archive.
    let before = call_json(&cx, "jobs.status", json!({"job": job})).unwrap();
    let rs = call_json(&cx, "search.rescore", json!({"job": job})).unwrap();
    assert_eq!(rs["elites"], before["progress"]["elites"], "{rs}");
    assert!(rs["runs"].as_u64().unwrap() >= screened(&before));
    let v2 = call_json(&cx, "search.runs", json!({"job": job, "limit": 10})).unwrap();
    assert_eq!(v2["runs"][0]["id"], best);
    // Published as an atlas: rows of the Quick Start table, under the search's uuid.
    let p = call_json(
        &cx,
        "quickstart.publish",
        json!({"job": job, "min_score": 0.0, "per_niche": 3}),
    )
    .unwrap();
    let n = p["entries"].as_u64().unwrap();
    assert!(n > 0, "{p}");
    assert_eq!(p["added"].as_u64(), Some(n), "{p}");
    let rows = cx
        .library()
        .quickstart(&[jetcarp_store::QsSource::Atlas])
        .unwrap();
    assert_eq!(rows.len() as u64, n);
    assert!(
        rows.iter()
            .all(|q| q.source_ref == p["search"].as_str().unwrap() && q.preview.is_some()),
        "{p}"
    );
    // Published again: the same entries, written again.
    let again = call_json(
        &cx,
        "quickstart.publish",
        json!({"job": job, "min_score": 0.0, "per_niche": 3}),
    )
    .unwrap();
    assert_eq!(
        (
            again["added"].as_u64(),
            again["updated"].as_u64(),
            again["removed"].as_u64()
        ),
        (Some(0), Some(n), Some(0)),
        "{again}"
    );
    // Sealed on request: every run in Parquet, the same runs listed.
    let sealed = call_json(&cx, "search.seal", json!({"job": job})).unwrap();
    assert_eq!(
        sealed["segments"],
        json!(["runs/part-000001.parquet"]),
        "{sealed}"
    );
    let again = call_json(&cx, "search.seal", json!({"job": job})).unwrap();
    assert_eq!(again["segments"], json!([]));
    let v3 = call_json(&cx, "search.runs", json!({"job": job, "limit": 10})).unwrap();
    assert_eq!(v3["runs"], v2["runs"]);
    let v = call_json(&cx, "search.runs", json!({"job": job, "reviewed": "any"})).unwrap();
    assert_eq!(v["runs"].as_array().unwrap().len(), 3);
    // Deleting the job with its folder keeps the reviews: they are in the library.
    let folder = std::path::PathBuf::from(sealed["folder"].as_str().unwrap());
    let uuid = folder.file_name().unwrap().to_string_lossy().into_owned();
    let gone = call_json(
        &cx,
        "jobs.remove",
        json!({"job": job, "delete_results": true}),
    )
    .unwrap();
    assert_eq!(gone["folder_deleted"], true, "{gone}");
    assert!(!folder.exists());
    let left = cx.library().run_review_counts(&uuid).unwrap();
    assert_eq!(
        left,
        vec![("drop".into(), 1), ("keep".into(), 1), ("maybe".into(), 1)]
    );
}

#[test]
fn a_clips_rating_page_saves_ratings_that_reviews_read_back() {
    let dir = scratch("explore-rating-page");
    let cx = ctx(&dir);
    let r = call_json(
        &cx,
        "search.start",
        json!({"strategy": "explore", "spec": small(), "budget": {"items": 200}, "threads": 2}),
    )
    .unwrap();
    let job = r["job"].as_i64().unwrap();
    let out = dir.join("round-1");
    let c = call_json(
        &cx,
        "search.clips",
        json!({"job": job, "out": out.display().to_string(), "sample": 4, "px": 32,
               "frames": 3, "page": true}),
    )
    .unwrap();
    // The page: offline, with the manifest's clips and the search's uuid embedded.
    let page = std::fs::read_to_string(out.join("index.html")).unwrap();
    assert_eq!(c["page"], out.join("index.html").display().to_string());
    assert!(!page.contains("http://") && !page.contains("https://"));
    assert!(!page.contains("__"), "a slot was left unfilled");
    let clips = c["clips"].as_array().unwrap();
    let uuid = cx.library().job(job).unwrap().uuid;
    assert!(page.contains(&format!("\"search\":\"{uuid}\"")));
    assert!(page.contains("\"round\":\"round-1\""));
    for x in clips {
        assert!(page.contains(&format!("\"file\":\"{}\"", x["file"].as_str().unwrap())));
    }
    // A ratings file as the page saves it: verdicts, a star alone, and a run of another search.
    let ids: Vec<i64> = clips.iter().map(|x| x["id"].as_i64().unwrap()).collect();
    let file = dir.join("ratings-round-1.json");
    let ratings = json!({
        "format": "jetcarp-ratings 1", "job": job, "search": uuid, "round": "round-1",
        "rater": "Ada", "note": "fronts are best",
        "ratings": [
            {"run": ids[0], "verdict": "keep", "star": true, "ts": 1794662868264_i64},
            {"run": ids[1], "verdict": "drop", "star": false, "ts": 1794662868300_i64},
            {"run": ids[2], "verdict": null, "star": true, "ts": 1794662868400_i64},
            {"run": 999_999, "verdict": "maybe", "star": false, "ts": 1794662868500_i64},
        ],
    });
    std::fs::write(&file, ratings.to_string()).unwrap();
    let from = file.display().to_string();
    let rv = call_json(&cx, "search.review", json!({"job": job, "from": from})).unwrap();
    assert_eq!(
        (
            &rv["recorded"],
            &rv["stars"],
            &rv["unknown_runs"],
            &rv["rater"],
            &rv["note"]
        ),
        (
            &json!(2),
            &json!(2),
            &json!(1),
            &json!("Ada"),
            &json!("fronts are best")
        ),
        "{rv}"
    );
    assert_eq!(rv["counts"], json!([["drop", 1], ["keep", 1]]), "{rv}");
    assert_eq!(rv["starred"], 2, "{rv}");
    // Stars are not verdicts: the reviewed runs are the two with one.
    let v = call_json(&cx, "search.runs", json!({"job": job, "reviewed": "any"})).unwrap();
    assert_eq!(v["runs"].as_array().unwrap().len(), 2, "{v}");
    let stars = cx.library().run_stars(&uuid).unwrap();
    assert_eq!(stars.len(), 2);
    assert!(
        stars
            .iter()
            .all(|s| s.rater == "Ada" && s.source == "ratings:round-1")
    );
    // Read again: nothing new. With another rater: recorded for that rater.
    let rv = call_json(&cx, "search.review", json!({"job": job, "from": from})).unwrap();
    assert_eq!(
        (&rv["recorded"], &rv["already"]),
        (&json!(0), &json!(4)),
        "{rv}"
    );
    let rv = call_json(
        &cx,
        "search.review",
        json!({"job": job, "from": from, "rater": "Bob"}),
    )
    .unwrap();
    assert_eq!(
        (&rv["recorded"], &rv["stars"], &rv["rater"]),
        (&json!(2), &json!(2), &json!("Bob"))
    );
    // Another search's ratings, or another kind of file, are refused.
    let mut other = ratings.clone();
    other["search"] = json!("00000000-0000-0000-0000-000000000000");
    std::fs::write(&file, other.to_string()).unwrap();
    fails(
        call_json(&cx, "search.review", json!({"job": job, "from": from})),
        ErrorKind::Invalid,
    );
    std::fs::write(&file, r#"{"format": "something else"}"#).unwrap();
    fails(
        call_json(&cx, "search.review", json!({"job": job, "from": from})),
        ErrorKind::Invalid,
    );
}

/// A JACA search's runs carry their sign measures; every measure comes out on request, in a
/// list or a CSV file, every run or the runs after an id; reviews fit a score model, which a
/// rescore applies (the job then keeps it) and a new search takes.
#[test]
fn measures_out_a_refit_and_its_score() {
    let dir = scratch("explore-refit");
    let cx = ctx(&dir);
    let spec = json!({
        "tilings": "square, hex", "families": ["jaca"], "screen_cells": 600,
        "screen_gens": 96, "confirm_cells": 900, "confirm_gens": 96, "confirm_seeds": 1,
        "seed": 7,
    });
    let r = call_json(
        &cx,
        "search.start",
        json!({"strategy": "explore", "spec": spec, "budget": {"items": 160}, "threads": 2}),
    )
    .unwrap();
    let job = r["job"].as_i64().unwrap();
    let v = call_json(
        &cx,
        "search.runs",
        json!({"job": job, "limit": 0, "measures": "all", "order": "id"}),
    )
    .unwrap();
    let runs = v["runs"].as_array().unwrap().clone();
    assert!(runs.len() >= 100, "{} runs", runs.len());
    assert_eq!(v["measures"].as_array().unwrap().len(), 26);
    for x in &runs {
        let m = &x["measures"];
        for k in [
            "sign_pos",
            "sign_balance",
            "sign_sorted",
            "sign_trade",
            "sign_lost",
        ] {
            assert!(m[k].is_number(), "{k}: {x}");
        }
        let (pos, bal) = (
            m["sign_pos"].as_f64().unwrap(),
            m["sign_balance"].as_f64().unwrap(),
        );
        assert!(
            (0.0..=1.0).contains(&pos) && (0.0..=0.5).contains(&bal),
            "{m}"
        );
    }
    fails(
        call_json(
            &cx,
            "search.runs",
            json!({"job": job, "measures": "colour"}),
        ),
        ErrorKind::Invalid,
    );
    // A CSV file: every run after an id, with the measures asked for.
    let after = runs[9]["id"].as_i64().unwrap();
    let c = call_json(
        &cx,
        "search.runs",
        json!({"job": job, "limit": 0, "since": after,
               "csv": dir.join("after.csv").display().to_string(),
               "measures": "sign_lost,density_late"}),
    )
    .unwrap();
    assert_eq!(c["written"].as_u64(), Some(runs.len() as u64 - 10), "{c}");
    let text = std::fs::read_to_string(c["csv"].as_str().unwrap()).unwrap();
    let mut lines = text.lines();
    assert!(
        lines
            .next()
            .unwrap()
            .ends_with(",score,sign_lost,density_late")
    );
    let first = lines.next().unwrap();
    assert!(
        first.starts_with(&format!("{},", runs[10]["id"])),
        "{first}"
    );
    assert_eq!(lines.count() + 1, runs.len() - 10);
    // Reviews: runs that kept both signs and sorted them well kept, lost ones dropped.
    let measure = |x: &Value, k: &str| x["measures"][k].as_f64().unwrap();
    let mut picks: Vec<&Value> = runs.iter().take(90).collect();
    picks.sort_by(|a, b| {
        let key = |x: &Value| measure(x, "sign_lost") + measure(x, "sign_sorted");
        key(b).total_cmp(&key(a))
    });
    for (i, x) in picks.iter().enumerate() {
        let verdict = ["keep", "maybe", "drop"][i * 3 / picks.len()];
        call_json(
            &cx,
            "search.review",
            json!({"job": job, "run": x["id"], "verdict": verdict, "rater": "tester"}),
        )
        .unwrap();
    }
    let f = call_json(
        &cx,
        "search.refit",
        json!({"jobs": [job], "family": "jaca", "rater": "tester",
               "out": dir.join("taste.json").display().to_string()}),
    )
    .unwrap();
    assert_eq!(
        (f["keep"].as_u64(), f["drop"].as_u64()),
        (Some(30), Some(30)),
        "{f}"
    );
    assert_eq!(f["replayed"], 0, "new runs have their sign measures: {f}");
    let auc = f["auc"].as_f64().unwrap();
    assert!(auc > 0.6 && auc <= 1.0, "{f}");
    assert!(!f["strongest"].as_array().unwrap().is_empty());
    let model_path = f["saved"].as_str().unwrap().to_string();
    let model: Value =
        serde_json::from_str(&std::fs::read_to_string(&model_path).unwrap()).unwrap();
    assert_eq!(model["format"], "jetcarp-score-model 1");
    assert_eq!(model["family"], "jaca");
    assert_eq!(model["fitted_on"]["jobs"], json!([job]));
    assert_eq!(model["fitted_on"]["raters"], json!(["tester"]));
    // Too few reviews of another rater: refused.
    fails(
        call_json(
            &cx,
            "search.refit",
            json!({"jobs": [job], "rater": "nobody"}),
        ),
        ErrorKind::Invalid,
    );
    // Rescored by the model: the job keeps it; back to the built-in score drops it.
    let rs = call_json(
        &cx,
        "search.rescore",
        json!({"job": job, "score": model_path}),
    )
    .unwrap();
    assert!(rs["score"].as_str().unwrap().contains("tester"), "{rs}");
    assert!(rs["changed"].as_u64().unwrap() > 0, "{rs}");
    assert_eq!(cx.library().job(job).unwrap().spec["score"], model);
    let top = call_json(&cx, "search.runs", json!({"job": job, "limit": 1})).unwrap();
    assert!(top["runs"][0]["score"].as_f64().unwrap() <= 3.0);
    let again = call_json(&cx, "search.rescore", json!({"job": job})).unwrap();
    assert_eq!(again["changed"], 0, "the job's own model: {again}");
    let back = call_json(
        &cx,
        "search.rescore",
        json!({"job": job, "score": "builtin"}),
    )
    .unwrap();
    assert_eq!(back["score"], "the built-in score");
    assert!(cx.library().job(job).unwrap().spec.get("score").is_none());
    // A new search with the model: its spec holds it.
    let mut spec2 = spec.clone();
    spec2["score"] = json!(model_path);
    let r2 = call_json(
        &cx,
        "search.start",
        json!({"strategy": "explore", "spec": spec2, "budget": {"items": 20}, "threads": 2}),
    )
    .unwrap();
    let j2 = r2["job"].as_i64().unwrap();
    assert_eq!(cx.library().job(j2).unwrap().spec["score"], model);
    spec2["score"] = json!({"format": "jetcarp-score-model 9"});
    fails(
        call_json(
            &cx,
            "search.start",
            json!({"strategy": "explore", "spec": spec2, "budget": {"items": 20}}),
        ),
        ErrorKind::Invalid,
    );
    // Parents from the first job: its keeps and maybes by default, or the verdicts asked for.
    let parents = |extra: Value| -> Vec<Value> {
        let mut sp = spec.clone();
        sp["parents_from"] = json!(job);
        for (k, v) in extra.as_object().unwrap() {
            sp[k] = v.clone();
        }
        let r = call_json(
            &cx,
            "search.start",
            json!({"strategy": "explore", "spec": sp, "budget": {"items": 10}, "threads": 2}),
        )
        .unwrap();
        let j = r["job"].as_i64().unwrap();
        let spec = cx.library().job(j).unwrap().spec;
        spec["parents"].as_array().cloned().unwrap_or_default()
    };
    let both = parents(json!({}));
    assert_eq!(both.len(), 60);
    assert!(both[..30].iter().all(|p| p["why"] == "keep" && p["job"] == job));
    let keeps = parents(json!({"parents_reviewed": "keep"}));
    assert_eq!(keeps.len(), 30);
    let drops = parents(json!({"parents_reviewed": "drop"}));
    assert!(drops.len() == 30 && drops.iter().all(|p| p["why"] == "drop"));
    fails(
        call_json(
            &cx,
            "search.start",
            json!({"strategy": "explore",
                   "spec": {"families": ["jaca"], "parents_from": job, "parents_reviewed": "fine"}}),
        ),
        ErrorKind::Invalid,
    );
}
