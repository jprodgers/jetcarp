//! The clip-rating page and its ratings file.
//!
//! **The page** (`search.clips` with `page`) is `index.html` in a clips folder: the clips of the
//! folder's manifest (embedded in it) in batches, each with Keep, Maybe and Drop and a ★, rated
//! with the mouse or the keyboard. It needs no network (its styles and script are inline, its
//! fonts the system's) and follows the system's light or dark setting. The browser keeps the
//! ratings as they are given (`localStorage`, keyed by a hash of the manifest, so two folders never
//! share them), and *Save ratings* downloads them as one ratings file.
//!
//! **The ratings file** (format `jetcarp-ratings 1`) is a JSON object: `format`, `job` (the
//! explore job's id), `search` (its uuid), `round` (the clips folder's name), `rater` (the page's
//! name box), `ratings` (`[{run, verdict, star, ts}]`: verdict `keep`, `maybe`, `drop` or `null`,
//! star `true` or `false`, ts in milliseconds since 1970), and `note`. `search.review` with
//! `from` reads it ([`import_file`]): each verdict becomes a review of the run, and each star a
//! star ([`jetcarp_store::library::RunStar`]), both with source `ratings:<round>`; reading the same
//! file again records only what is new in it.

use super::explore::Clip;
use crate::error::ApiError;
use jetcarp_search::explore::runs::Runs;
use jetcarp_search::explore::segments::col;
use jetcarp_store::Library;
use jetcarp_store::library::{Dedupe, RunReview, RunStar, VERDICTS};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::path::{Path, PathBuf};

/// The page, with `__NAME__` slots filled by [`write_page`].
const PAGE: &str = include_str!("rate_page.html");

/// The ratings file's format.
pub const FORMAT: &str = "jetcarp-ratings 1";

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// JSON for a `<script type="application/json">` block: every `<` written `<`, so nothing in
/// it can end the block.
fn script_json(v: &impl Serialize) -> String {
    serde_json::to_string(v)
        .unwrap_or_else(|_| "null".into())
        .replace('<', "\\u003c")
}

fn folder_name(dir: &Path) -> String {
    dir.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "clips".into())
}

/// Write the rating page, `index.html`, into the clips folder `dir`: the clips of explore job
/// `job` (search `uuid`, `label`), whose `manifest.json` text is `manifest`.
pub(crate) fn write_page(
    dir: &Path,
    job: i64,
    uuid: &str,
    label: Option<&str>,
    manifest: &str,
    clips: &[Clip],
) -> Result<PathBuf, ApiError> {
    let round = folder_name(dir);
    let key = jetcarp_core::sha256::sha256_hex(manifest.as_bytes())[..16].to_string();
    let meta = json!({"job": job, "search": uuid, "round": round, "key": key});
    let lede = format!(
        "{} clips from explore job {job}{}, in the folder {round}.",
        clips.len(),
        label.map(|l| format!(" ({l})")).unwrap_or_default()
    );
    let html = PAGE
        .replace("__VERSION__", jetcarp_core::VERSION)
        .replace(
            "__TITLE__",
            &html_escape(&format!("Rate clips · explore job {job}")),
        )
        .replace(
            "__HEADING__",
            &html_escape(&format!("Rate the clips of explore job {job}")),
        )
        .replace("__LEDE__", &html_escape(&lede))
        .replace(
            "__COMMAND__",
            &html_escape(&format!("jetcarp jobs review {job} --from FILE")),
        )
        .replace("__META__", &script_json(&meta))
        .replace("__CLIPS__", &script_json(&clips));
    let page = dir.join("index.html");
    std::fs::write(&page, html)
        .map_err(|e| ApiError::io(format!("writing {}: {e}", page.display())))?;
    Ok(page)
}

/// A ratings file, as the page saves it.
#[derive(Debug, Deserialize)]
struct RatingsFile {
    format: String,
    #[serde(default)]
    job: Option<i64>,
    search: String,
    #[serde(default)]
    round: Option<String>,
    #[serde(default)]
    rater: Option<String>,
    #[serde(default)]
    ratings: Vec<FileRating>,
    #[serde(default)]
    note: Option<String>,
}

#[derive(Debug, Deserialize)]
struct FileRating {
    run: i64,
    #[serde(default)]
    verdict: Option<String>,
    #[serde(default)]
    star: bool,
    #[serde(default)]
    ts: Option<i64>,
}

/// What a ratings file held, and what was recorded from it.
#[derive(Clone, Debug, Default)]
pub(crate) struct FileRead {
    /// Verdicts and stars recorded now.
    pub recorded: u64,
    pub stars: u64,
    /// Ratings for runs not in this search, and ratings already recorded from this file.
    pub unknown_runs: u64,
    pub already: u64,
    /// The rater recorded, and the file's note.
    pub rater: String,
    pub note: Option<String>,
}

/// Is `path` a ratings file (a file whose `format` is [`FORMAT`]), rather than a folder of
/// rating files? A file of another kind is an error.
pub(crate) fn is_ratings_file(path: &Path) -> Result<bool, ApiError> {
    if path.is_dir() {
        return Ok(false);
    }
    let text = std::fs::read_to_string(path)
        .map_err(|e| ApiError::io(format!("{}: {e}", path.display())))?;
    let v: serde_json::Value = serde_json::from_str(&text)
        .map_err(|e| ApiError::invalid(format!("{} is not JSON: {e}", path.display())))?;
    match v.get("format").and_then(|f| f.as_str()) {
        Some(FORMAT) => Ok(true),
        Some(f) => Err(ApiError::invalid(format!(
            "{} is a {f} file; this program reads {FORMAT}",
            path.display()
        ))),
        None => Err(ApiError::invalid(format!(
            "{} is not a ratings file ({FORMAT}, as the clip-rating page saves it) or a folder \
             of rating files",
            path.display()
        ))),
    }
}

/// Record a ratings file's verdicts and stars on the runs of `runs`' search. The rater is
/// `rater` when given, else the file's, else `default_rater`.
pub(crate) fn import_file(
    lib: &Library,
    runs: &Runs,
    path: &Path,
    rater: Option<&str>,
    default_rater: &str,
) -> Result<FileRead, ApiError> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| ApiError::io(format!("{}: {e}", path.display())))?;
    let f: RatingsFile = serde_json::from_str(&text)
        .map_err(|e| ApiError::invalid(format!("{}: {e}", path.display())))?;
    if f.format != FORMAT {
        return Err(ApiError::invalid(format!(
            "{} is a {} file; this program reads {FORMAT}",
            path.display(),
            f.format
        )));
    }
    let uuid = runs.uuid();
    if f.search != uuid {
        return Err(ApiError::invalid(format!(
            "{} rates the runs of search {}{}, not of this job's search {uuid}",
            path.display(),
            f.search,
            f.job.map(|j| format!(" (job {j})")).unwrap_or_default()
        )));
    }
    for r in &f.ratings {
        if let Some(v) = &r.verdict
            && !VERDICTS.contains(&v.as_str())
        {
            return Err(ApiError::invalid(format!(
                "{}: run {}: a verdict is keep, maybe, or drop, not '{v}'",
                path.display(),
                r.run
            )));
        }
    }
    let rater = rater
        .map(str::trim)
        .filter(|r| !r.is_empty())
        .or_else(|| f.rater.as_deref().map(str::trim).filter(|r| !r.is_empty()))
        .unwrap_or(default_rater)
        .to_string();
    let round = f
        .round
        .clone()
        .filter(|r| !r.trim().is_empty())
        .unwrap_or_else(|| {
            path.file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_else(|| "ratings".into())
        });
    let source = format!("ratings:{round}");
    let file_time = std::fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|m| m.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs() as i64);
    let ids: Vec<i64> = f.ratings.iter().map(|r| r.run).collect();
    let known = runs.fetch(&ids, 1 << col::ID).map_err(ApiError::engine)?;
    let mut out = FileRead {
        rater: rater.clone(),
        note: f.note.clone().filter(|n| !n.trim().is_empty()),
        ..Default::default()
    };
    let (mut reviews, mut stars) = (Vec::new(), Vec::new());
    for r in &f.ratings {
        if !known.contains_key(&r.run) {
            out.unknown_runs += 1;
            continue;
        }
        // The page writes milliseconds.
        let ts =
            r.ts.map(|t| if t > 100_000_000_000 { t / 1000 } else { t })
                .or(file_time)
                .unwrap_or_else(jetcarp_search::dashboard::now_seconds);
        let created = jetcarp_core::pattern::utc_of(ts.max(0) as u64);
        if let Some(v) = &r.verdict {
            reviews.push(RunReview {
                search: uuid.clone(),
                run: r.run,
                verdict: v.clone(),
                note: String::new(),
                rater: rater.clone(),
                source: source.clone(),
                created: created.clone(),
            });
        }
        if r.star {
            stars.push(RunStar {
                search: uuid.clone(),
                run: r.run,
                rater: rater.clone(),
                source: source.clone(),
                created,
            });
        }
    }
    let (done, skipped) = lib
        .add_run_reviews(&reviews, Dedupe::Exact)
        .map_err(ApiError::engine)?;
    let (starred, star_skipped) = lib.add_run_stars(&stars).map_err(ApiError::engine)?;
    out.recorded = done;
    out.stars = starred;
    out.already = skipped + star_skipped;
    Ok(out)
}
