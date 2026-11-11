//! Reviews of an explore search's runs: the verdicts (keep, maybe, drop) people give runs, kept
//! as ratings in the library ([`jetcarp_store::library::RunReview`]; target kind `search_run`,
//! target id `<search uuid>:<run id>`), so they outlive the search's folder. One at a time
//! ([`add_review`]), from a folder of rating files ([`import_ratings`]), or moved from a working
//! database of format 1 ([`move_old`]); counted per verdict ([`review_counts`]); and turned into a
//! run filter ([`resolve`]).

use crate::explore::runs::{RunFilter, Runs};
use jetcarp_store::Library;
use jetcarp_store::library::{Dedupe, RunReview};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub use jetcarp_store::library::VERDICTS;

fn utc(ts: i64) -> String {
    jetcarp_core::pattern::utc_of(ts.max(0) as u64)
}

/// Record a verdict on a run (`keep`, `maybe`, `drop`). The run must exist.
pub fn add_review(
    lib: &Library,
    runs: &Runs,
    run: i64,
    verdict: &str,
    note: &str,
    rater: &str,
    source: &str,
) -> Result<(), String> {
    if !VERDICTS.contains(&verdict) {
        return Err(format!(
            "a verdict is keep, maybe, or drop, not '{verdict}'"
        ));
    }
    if !runs.has(run)? {
        return Err(format!("no run {run}"));
    }
    lib.add_run_reviews(
        &[RunReview {
            search: runs.uuid(),
            run,
            verdict: verdict.into(),
            note: note.into(),
            rater: rater.into(),
            source: source.into(),
            created: jetcarp_core::pattern::now_utc(),
        }],
        Dedupe::None,
    )?;
    Ok(())
}

/// What a ratings folder held.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct RatingsRead {
    pub files: u64,
    pub recorded: u64,
    /// Files whose run is not in this search.
    pub unknown_runs: u64,
    /// Files that did not read as a rating.
    pub unreadable: u64,
    /// Ratings already recorded from this folder, skipped.
    pub already: u64,
}

/// Record the ratings in a folder of rating files, as the clip-rating page saves them:
/// `<run id>.json` holding `{"verdict": "keep", "ts": <ms>}` (or that inside `data`; without a
/// time, the file's), with an optional `note`. Their source is `ratings:<folder name>` (the name
/// only, never where the folder is on this disk). A rating recorded already (the same run,
/// verdict, note, rater, time, and source) is skipped, so reading a folder again records only
/// what is new in it.
pub fn import_ratings(
    lib: &Library,
    runs: &Runs,
    dir: &Path,
    rater: &str,
) -> Result<RatingsRead, String> {
    let rd = std::fs::read_dir(dir).map_err(|x| format!("{}: {x}", dir.display()))?;
    let source = format!("ratings:{}", crate::explore::store::file_name(dir));
    let mut out = RatingsRead::default();
    let mut files: Vec<PathBuf> = rd
        .flatten()
        .map(|d| d.path())
        .filter(|p| p.extension().is_some_and(|x| x == "json"))
        .collect();
    files.sort();
    let mut read: Vec<(i64, String, String, i64)> = Vec::new();
    for p in files {
        let Some(run) = p
            .file_stem()
            .and_then(|s| s.to_str())
            .and_then(|s| s.parse::<i64>().ok())
        else {
            continue;
        };
        out.files += 1;
        let v: Option<serde_json::Value> = std::fs::read_to_string(&p)
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok());
        let Some(v) = v else {
            out.unreadable += 1;
            continue;
        };
        let v = v.get("data").cloned().unwrap_or(v);
        let Some(verdict) = v
            .get("verdict")
            .and_then(|x| x.as_str())
            .filter(|x| VERDICTS.contains(x))
        else {
            out.unreadable += 1;
            continue;
        };
        let note = v.get("note").and_then(|x| x.as_str()).unwrap_or("");
        // Rating pages write milliseconds.
        let ts = v
            .get("ts")
            .and_then(|x| x.as_i64())
            .map(|t| if t > 100_000_000_000 { t / 1000 } else { t })
            .or_else(|| {
                let m = std::fs::metadata(&p).and_then(|m| m.modified()).ok()?;
                let s = m.duration_since(std::time::UNIX_EPOCH).ok()?;
                Some(s.as_secs() as i64)
            })
            .unwrap_or_else(crate::explore::store::now);
        read.push((run, verdict.to_string(), note.to_string(), ts));
    }
    let ids: Vec<i64> = read.iter().map(|r| r.0).collect();
    let known = runs.fetch(&ids, 1 << crate::explore::segments::col::ID)?;
    let mut list = Vec::new();
    for (run, verdict, note, ts) in read {
        if !known.contains_key(&run) {
            out.unknown_runs += 1;
            continue;
        }
        list.push(RunReview {
            search: runs.uuid(),
            run,
            verdict,
            note,
            rater: rater.to_string(),
            source: source.clone(),
            created: utc(ts),
        });
    }
    let (done, skipped) = lib.add_run_reviews(&list, Dedupe::Exact)?;
    out.recorded = done;
    out.already = skipped;
    Ok(out)
}

/// Move a search's working-database reviews (format 1, from before reviews moved to the library)
/// into the library, with source `review` for those that name none, then drop the table. Reviews
/// recorded already (every field the same) are skipped, so a move cut short and done again records
/// each once. Returns how many were recorded.
pub fn move_old(lib: &Library, runs: &Runs) -> Result<u64, String> {
    if !crate::explore::store::has_table(&runs.db, "reviews")? {
        return Ok(0);
    }
    let search = runs.uuid();
    let list: Vec<RunReview> = crate::explore::store::old_reviews(&runs.db)?
        .into_iter()
        .filter(|r| VERDICTS.contains(&r.1.as_str()))
        .map(|(run, verdict, note, ts, rater, source)| RunReview {
            search: search.clone(),
            run,
            verdict,
            note,
            rater,
            source: if source.is_empty() {
                "review".into()
            } else {
                source
            },
            created: utc(ts),
        })
        .collect();
    let n = lib.add_run_reviews(&list, Dedupe::Exact)?.0;
    crate::explore::store::drop_old_reviews(&runs.db)?;
    Ok(n)
}

/// A search's reviews per verdict.
pub fn review_counts(lib: &Library, search: &str) -> Result<Vec<(String, u64)>, String> {
    lib.run_review_counts(search)
}

/// Turn a filter's `reviewed` (any, or a verdict) into the runs it names (`within`, with any
/// `within` already there).
pub fn resolve(lib: &Library, search: &str, f: &mut RunFilter) -> Result<(), String> {
    let Some(v) = f.reviewed.clone() else {
        return Ok(());
    };
    let verdict = match v.as_str() {
        "any" => None,
        x if VERDICTS.contains(&x) => Some(x),
        x => return Err(format!("reviewed is any, keep, maybe, or drop, not {x}")),
    };
    let mut ids = lib.reviewed_runs(search, verdict)?;
    if let Some(w) = &f.within {
        let w: std::collections::HashSet<i64> = w.iter().copied().collect();
        ids.retain(|i| w.contains(i));
    }
    f.within = Some(ids);
    Ok(())
}
