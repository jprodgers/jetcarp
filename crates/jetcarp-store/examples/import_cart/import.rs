//! The CART catalog importer (see `main.rs`). A module of its own so that the integration test
//! `tests/import_cart.rs` runs it on a small CART catalog it builds.
//!
//! **Records.** Each CART record is decoded, put in canonical form (which gives its content id and
//! this engine's geometry pin) and verified again on this engine in its own rule and in every rule
//! CART's `pattern_rules` says it was verified in; its rule range is computed again. A record that
//! fails in every rule is not imported (`cart-failures.tsv` says why). A record that fails in some
//! rules comes in with the others only, and its sightings in the failed rules are dropped. CART
//! records with one content id (the same pattern stored in two orientations or phases) become one
//! record:
//!
//! - seen counts add up (less the copies sighted in rules the record failed in), and so do
//!   sightings per (rule, job): copies and soups summed, first seen the earliest, last seen the
//!   latest; its rules are all the rules any of them verified in, each first seen the earliest;
//! - the records are taken in the order they were found (the moment of the find: the earliest of
//!   a record's own provenance date, the find in its run, and CART's `created`, which can be up to
//!   an hour later; then id). The name is the first one's; another record's different name goes
//!   into the notes as "Also named in CART: ..."; notes are kept, the first first, and tags are
//!   joined; the rating is the highest; the curation state is the first that is not `new`;
//! - its provenance (the run it came from) is the first one's, dated by the earliest find, and
//!   its `created` is the earliest. The interest score is computed again from the seen count, and
//!   the label is made again from the new id (CART's labels were all generated, from the old id).
//!
//! **Provenance.** Row 0 is how the record was found: `found`, by Jimmie Rodgers (ORCID iD
//! 0009-0003-8775-6483), at the moment of the earliest find (the record's own provenance date),
//! from its source (`hunt`, `search`), by the job whose sighting of it came first, with the engine
//! version its records state (`CART <version>` for CART's `cart_version`, the version as written
//! for a JETCARP catalog's `jetcarp_version`) and the geometry pin it was verified with today.
//! Row 1 is `imported`: now, by this engine, from `CART catalog <file name>` (`JETCARP catalog
//! <file name>` for JETCARP's own catalogs from before content ids, which the tool reads too).
//!
//! **Jobs.** CART's jobs become finished jobs of the library (uuids made from their CART rows, so a
//! second run finds them), with their spec and progress (absolute paths cut to file names, CART
//! record ids in the progress changed to the new ids, finds that failed left out), census rows,
//! and their unstored objects as counts in the progress (`unstored`, `unstored_distinct`, as a
//! hunt keeps them): CART's unstored rows name objects by CART ids with no cells, so they cannot
//! be given content ids. Checkpoints are not kept (CART's format; a finished job is not resumed).
//! CART job 0 (adds by hand) is the library's job 0.
//!
//! **Not kept.** CART's ids (no aliases: `cart-to-new-ids.tsv` maps them once), media (made again
//! from the records), settings and score models (gone in the library's schema), and its empty
//! tables (ants, Quick Start, ratings).
//!
//! **Again and alongside.** Running it again adds nothing: records already stored are left as
//! they are (only rules they lack are added), and jobs, sightings and census rows are written
//! only where missing. It writes only its own jobs and the records, so another importer can write
//! searches and ratings into the same library before or after it.

// Shared by the example and its test, which each use part of it.
#![allow(dead_code)]

use jetcarp_core::pattern::range::rule_range;
use jetcarp_core::pattern::{
    CurationState, PatternClass, PatternRecord, RuleRange, Verification, default_periods, verify,
};
use jetcarp_core::rules::Rule;
use jetcarp_core::topology::Nb;
use jetcarp_store::Library;
use jetcarp_store::library::Origin;
use rayon::prelude::*;
use rusqlite::{Connection, OpenFlags, OptionalExtension, params};
use serde_json::{Value, json};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fmt::Write as _;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::Instant;

/// Who found CART's records: CART was Jimmie Rodgers's.
pub const FINDER: &str = "Jimmie Rodgers";
pub const ORCID: &str = "0009-0003-8775-6483";

/// Records per write transaction.
const CHUNK: usize = 4000;

#[derive(Clone, Debug, Default)]
pub struct Options {
    pub cart: PathBuf,
    pub library: PathBuf,
    /// Where the TSV files and the report go (default: the current folder).
    pub report: Option<PathBuf>,
    /// Worker threads (0: 16, or the cores if fewer).
    pub threads: usize,
}

/// One CART id and what became of it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MapRow {
    pub cart: String,
    /// The new content id (empty when it was not imported).
    pub new: String,
    pub tiling: String,
    pub class: String,
    pub period: u64,
    pub rule: String,
    /// `imported`, `merged` (one of several CART ids of one record), `failed` (in every rule; not
    /// imported), or `failed-merged` (failed in its own rules; its record came in from another
    /// CART id).
    pub status: String,
    /// Why it failed (`failed`, `failed-merged`).
    pub reason: String,
}

/// What an import did (the counts are of this run).
#[derive(Clone, Debug, Default)]
pub struct Report {
    pub records: u64,
    /// Content ids among the records that verified.
    pub ids: u64,
    /// Records stored by this run, and records already in the library.
    pub added: u64,
    pub present: u64,
    /// Rules added to records already in the library.
    pub rules_added_to_present: u64,
    /// CART records that failed in every rule: by "tiling class", and by reason.
    pub failed: u64,
    pub failed_by: BTreeMap<String, u64>,
    pub reasons: BTreeMap<String, u64>,
    /// CART records verified in some of their rules only, and the (record, rule) pairs dropped.
    pub partial: u64,
    pub rules_dropped: u64,
    /// The partial records by "tiling class", the dropped rules by reason, and one TSV row per
    /// dropped rule (CART id, tiling, class, period, rule, reason).
    pub partial_by: BTreeMap<String, u64>,
    pub partial_reasons: BTreeMap<String, u64>,
    pub partial_rows: Vec<String>,
    /// (record, rule) pairs verified.
    pub rules_verified: u64,
    pub merged_groups: u64,
    pub merged_ids: u64,
    pub sightings_written: u64,
    /// CART sightings dropped: in a rule the record failed in (records that came in), and of
    /// records that failed in every rule.
    pub sightings_dropped: u64,
    pub sightings_of_failed: u64,
    /// Records whose seen count lost the copies sighted in a failed rule.
    pub seen_reduced: u64,
    pub jobs_added: u64,
    pub jobs: u64,
    pub census_rows_written: u64,
    pub unstored_objects: u64,
    pub unstored_copies: u64,
    /// CART ids in job progress changed to new ids, and finds there left out (they failed).
    pub progress_ids: u64,
    pub progress_dropped: u64,
    /// Absolute paths in job specs and progress cut to file names.
    pub paths_cut: u64,
    /// Records whose rule range could not be computed (stored without one).
    pub range_errors: u64,
    /// CART's `created` differs from its record's own provenance date.
    pub dates_differ: u64,
    pub add_errors: Vec<String>,
    pub mapping: Vec<MapRow>,
    pub seconds: f64,
    /// The report as text.
    pub text: String,
}

/// Run the import.
pub fn run(o: &Options) -> Result<Report, String> {
    let threads = if o.threads > 0 {
        o.threads
    } else {
        std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(4)
            .min(16)
    };
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(threads)
        .build()
        .map_err(|e| e.to_string())?;
    pool.install(|| run_in(o))
}

// ---------------------------------------------------------------------------------------------
// Reading CART

struct CartJob {
    id: i64,
    kind: String,
    spec: String,
    status: String,
    progress: String,
    created: String,
    updated: String,
    finished: Option<String>,
}

#[derive(Clone)]
struct CartRule {
    notation: String,
    nb: Nb,
    spg: u32,
}

#[derive(Clone)]
struct PrRow {
    rule: i64,
    periods: u64,
    first_seen: String,
}

#[derive(Clone)]
struct SightRow {
    rule: i64,
    job: i64,
    count: u64,
    soups: u64,
    first: String,
    last: String,
}

struct Cart {
    id: String,
    tiling: String,
    class: String,
    period: u64,
    rule: String,
    created: String,
    /// The moment of the find: the earliest of its provenance date and `created`.
    found: String,
    /// The engine that found it, as the record states it ([`found_with`]).
    found_with: String,
    seen: u64,
    rec: PatternRecord,
}

/// The engine that found a record, as its body states it: `CART <version>` for CART's
/// `cart_version`, the version as written for a JETCARP catalog's `jetcarp_version`.
fn found_with(prov: &Value) -> String {
    match (prov.get("cart_version"), prov.get("jetcarp_version")) {
        (Some(v), _) => match v.as_str().unwrap_or("") {
            "" => "CART".to_string(),
            v => format!("CART {v}"),
        },
        (None, Some(v)) => v.as_str().unwrap_or("").to_string(),
        _ => String::new(),
    }
}

/// The earlier of two UTC dates (`YYYY-MM-DDTHH:MM:SSZ`, so they compare as text), either when
/// the other is empty.
fn earliest(a: &str, b: &str) -> String {
    match (a, b) {
        ("", b) => b.to_string(),
        (a, "") => a.to_string(),
        (a, b) => a.min(b).to_string(),
    }
}

fn sql(e: rusqlite::Error) -> String {
    e.to_string()
}

fn open_cart(p: &Path) -> Result<Connection, String> {
    if !p.is_file() {
        return Err(format!("no CART catalog at {}", p.display()));
    }
    let s = p
        .to_str()
        .ok_or_else(|| format!("{}: not a UTF-8 path", p.display()))?;
    let uri = format!(
        "file:{}?mode=ro&immutable=1",
        s.replace('%', "%25")
            .replace('?', "%3f")
            .replace('#', "%23")
    );
    let c = Connection::open_with_flags(
        &uri,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_URI,
    )
    .map_err(|e| format!("{}: {e}", p.display()))?;
    let app: i64 = c
        .query_row("PRAGMA application_id", [], |r| r.get(0))
        .map_err(sql)?;
    if app != 0 {
        return Err(format!(
            "{} is a JETCARP library, not a CART catalog",
            p.display()
        ));
    }
    for t in ["patterns", "pattern_rules", "rules", "sightings", "jobs"] {
        let n: i64 = c
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE type = 'table' AND name = ?1",
                [t],
                |r| r.get(0),
            )
            .map_err(sql)?;
        if n == 0 {
            return Err(format!(
                "{} is not a CART catalog (it has no table {t})",
                p.display()
            ));
        }
    }
    Ok(c)
}

/// Who made a catalog, from what its records state: (`CART`, `CART`) for CART's, (`JETCARP`,
/// `<version>`) for one of JETCARP's own from before content ids. The second is its jobs' engine
/// version.
fn made_by(c: &Connection) -> Result<(&'static str, String), String> {
    let mut st = c
        .prepare("SELECT body FROM patterns ORDER BY rowid LIMIT 200")
        .map_err(sql)?;
    let mut q = st.query([]).map_err(sql)?;
    let mut seen: BTreeMap<String, u64> = BTreeMap::new();
    while let Some(r) = q.next().map_err(sql)? {
        let Ok(text) = body_text(r.get_ref(0).map_err(sql)?) else {
            continue;
        };
        let body: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
        *seen.entry(found_with(&body["provenance"])).or_default() += 1;
    }
    let most = seen
        .into_iter()
        .max_by_key(|x| x.1)
        .map(|x| x.0)
        .unwrap_or_else(|| "CART".into());
    Ok(if most == "CART" || most.starts_with("CART ") {
        ("CART", "CART".to_string())
    } else {
        ("JETCARP", most)
    })
}

fn has_table(c: &Connection, t: &str) -> bool {
    c.query_row(
        "SELECT count(*) FROM sqlite_master WHERE type = 'table' AND name = ?1",
        [t],
        |r| r.get::<_, i64>(0),
    )
    .unwrap_or(0)
        > 0
}

/// A CART body: JSON text, or `Z1` and the deflated JSON.
fn body_text(v: rusqlite::types::ValueRef) -> Result<String, String> {
    match v {
        rusqlite::types::ValueRef::Text(t) => Ok(String::from_utf8_lossy(t).to_string()),
        rusqlite::types::ValueRef::Blob(b) if b.starts_with(b"Z1") => {
            let mut s = String::new();
            flate2::read::DeflateDecoder::new(&b[2..])
                .read_to_string(&mut s)
                .map_err(|e| format!("body: {e}"))?;
            Ok(s)
        }
        rusqlite::types::ValueRef::Blob(b) => Ok(String::from_utf8_lossy(b).to_string()),
        _ => Err("body is empty".into()),
    }
}

/// The CART records of one tiling. A record that cannot be decoded comes back as an error with
/// its id, tiling, class, period and rule.
#[allow(clippy::type_complexity)]
fn read_tiling(
    c: &Connection,
    tiling: &str,
) -> Result<Vec<Result<Cart, (MapRow, String)>>, String> {
    let mut st = c
        .prepare(
            "SELECT id, name, rule, neighborhood, steps_per_gen, tiling_name, tile_types, class,
                period, dx, dy, speed, cells, min_cells, max_cells, canonical, seen,
                discovered_by, tags, notes, score, rating, state, created, updated, body, tiling_id
             FROM patterns WHERE tiling_id = ?1 ORDER BY id",
        )
        .map_err(sql)?;
    let rows = st
        .query_map([tiling], |r| {
            let id: String = r.get(0)?;
            let rule: String = r.get(2)?;
            let class: String = r.get(7)?;
            let period: i64 = r.get(8)?;
            let created: String = r.get(23)?;
            let seen: i64 = r.get(16)?;
            let head = MapRow {
                cart: id.clone(),
                tiling: tiling.to_string(),
                class: class.clone(),
                period: period as u64,
                rule: rule.clone(),
                ..Default::default()
            };
            let body = match body_text(r.get_ref(25)?) {
                Ok(b) => b,
                Err(e) => return Ok(Err((head, e))),
            };
            let body: Value = match serde_json::from_str(&body) {
                Ok(b) => b,
                Err(e) => return Ok(Err((head, format!("body: {e}")))),
            };
            let dx: Option<i64> = r.get(9)?;
            let dy: Option<i64> = r.get(10)?;
            let tags: String = r.get(18)?;
            let v = json!({
                "format": body["format"],
                "id": id,
                "name": r.get::<_, Option<String>>(1)?,
                "rule": rule,
                "neighborhood": r.get::<_, String>(3)?,
                "steps_per_gen": r.get::<_, i64>(4)?,
                "tiling": body["tiling"],
                "tiling_name": r.get::<_, String>(5)?,
                "tile_types": r.get::<_, i64>(6)?,
                "class": class,
                "period": period,
                "displacement": dx.zip(dy).map(|(a, b)| json!([a, b])),
                "displacement_xy": body["displacement_xy"],
                "speed": r.get::<_, Option<String>>(11)?,
                "cells": r.get::<_, i64>(12)?,
                "min_cells": r.get::<_, i64>(13)?,
                "max_cells": r.get::<_, i64>(14)?,
                "bbox": body["bbox"],
                "canonical": r.get::<_, String>(15)?,
                "phases": body["phases"],
                "site": body["site"],
                "verified": body["verified"],
                "mover": body["mover"],
                "gun": body["gun"],
                "seen": seen,
                "provenance": body["provenance"],
                "discovered_by": r.get::<_, String>(17)?,
                "tags": serde_json::from_str::<Value>(&tags).unwrap_or(json!([])),
                "notes": r.get::<_, String>(19)?,
                "score": r.get::<_, f64>(20)?,
                "rating": r.get::<_, Option<i64>>(21)?,
                "state": r.get::<_, String>(22)?,
                "created": created,
                "updated": r.get::<_, String>(24)?,
            });
            let with = found_with(&body["provenance"]);
            Ok(match serde_json::from_value::<PatternRecord>(v) {
                Ok(rec) => Ok(Cart {
                    id,
                    tiling: tiling.to_string(),
                    class,
                    period: period as u64,
                    rule,
                    found: earliest(&rec.provenance.date, &created),
                    created,
                    found_with: with,
                    seen: seen.max(0) as u64,
                    rec,
                }),
                Err(e) => Err((head, format!("decode: {e}"))),
            })
        })
        .map_err(sql)?;
    rows.collect::<Result<Vec<_>, _>>().map_err(sql)
}

// ---------------------------------------------------------------------------------------------
// Checking one record

/// A rule as the library keys it: the rule's own notation, neighbourhood, steps per generation.
fn rule_key(notation: &str, nb: Nb, spg: u32) -> Result<String, String> {
    let r = Rule::parse(notation)?.0;
    Ok(format!("{}|{}|{spg}", r.notation(), nb_name(nb)))
}

fn nb_name(nb: Nb) -> &'static str {
    match nb {
        Nb::Touch => "touch",
        Nb::Edge => "edge",
    }
}

/// One rule of a record, as checked.
#[derive(Clone)]
struct Checked {
    key: String,
    /// The notation the record carries (B/S rules on edge neighbours with a `V`, as CART wrote).
    notation: String,
    /// CART's `pattern_rules` row (absent for a main rule CART did not list).
    cart_rule: Option<i64>,
    first_seen: String,
    result: Result<Verification, String>,
}

/// A CART record checked: in canonical form, each of its rules verified.
struct Done {
    cart: usize,
    rec: Result<PatternRecord, String>,
    rules: Vec<Checked>,
    range: Option<RuleRange>,
    range_error: bool,
}

fn check(
    i: usize,
    c: &Cart,
    prs: &[PrRow],
    rules: &HashMap<i64, CartRule>,
    keys: &HashMap<i64, String>,
) -> Done {
    let fail = |e: String| Done {
        cart: i,
        rec: Err(e),
        rules: vec![],
        range: None,
        range_error: false,
    };
    let mut rec = c.rec.clone();
    if let Err(e) = rec.canonicalize() {
        return fail(format!("canonical form: {e}"));
    }
    let nb = rec.neighborhood;
    let spg = rec.steps_per_gen;
    let main = match rule_key(&c.rule, nb, spg) {
        Ok(k) => k,
        Err(e) => return fail(format!("rule {}: {e}", c.rule)),
    };
    let cart_periods = c.rec.verified.periods;
    let mut list: Vec<(String, String, Option<i64>, u64, String)> = vec![(
        main.clone(),
        c.rule.clone(),
        None,
        cart_periods,
        c.created.clone(),
    )];
    let mut bad: Vec<Checked> = vec![];
    for pr in prs {
        let (Some(r), Some(k)) = (rules.get(&pr.rule), keys.get(&pr.rule)) else {
            bad.push(Checked {
                key: format!("cart rule {}", pr.rule),
                notation: String::new(),
                cart_rule: Some(pr.rule),
                first_seen: pr.first_seen.clone(),
                result: Err(format!("CART rule {} cannot be read", pr.rule)),
            });
            continue;
        };
        if r.nb != nb || r.spg != spg {
            bad.push(Checked {
                key: k.clone(),
                notation: r.notation.clone(),
                cart_rule: Some(pr.rule),
                first_seen: pr.first_seen.clone(),
                result: Err(format!(
                    "rule {} is for {} neighbours, the record for {}",
                    r.notation,
                    nb_name(r.nb),
                    nb_name(nb)
                )),
            });
            continue;
        }
        if let Some(x) = list.iter_mut().find(|x| &x.0 == k) {
            x.2 = Some(pr.rule);
            x.3 = x.3.max(pr.periods);
            if pr.first_seen < x.4 {
                x.4 = pr.first_seen.clone();
            }
            continue;
        }
        let mut n = r.notation.clone();
        let lifelike = Rule::parse(&n).is_ok_and(|(rule, own)| {
            own.is_none() && matches!(rule, Rule::LifeLike { .. } | Rule::Generations { .. })
        });
        if nb == Nb::Edge && lifelike {
            n.push('V');
        }
        list.push((
            k.clone(),
            n,
            Some(pr.rule),
            pr.periods,
            pr.first_seen.clone(),
        ));
    }
    let mut out: Vec<Checked> = list
        .into_iter()
        .map(|(key, notation, cart_rule, periods, first_seen)| {
            let mut r = rec.clone();
            r.rule = notation.clone();
            let result = verify(&r, default_periods(r.period).max(periods.min(64)));
            Checked {
                key,
                notation,
                cart_rule,
                first_seen,
                result,
            }
        })
        .collect();
    out.extend(bad);
    let mut range = None;
    let mut range_error = false;
    if let Some(ok) = out.iter().find(|x| x.result.is_ok()) {
        let mut r = rec.clone();
        r.rule = ok.notation.clone();
        match rule_range(&r) {
            Ok(x) => range = x,
            Err(_) => range_error = true,
        }
    }
    Done {
        cart: i,
        rec: Ok(rec),
        rules: out,
        range,
        range_error,
    }
}

/// A failure reason with its numbers taken out, for counting.
fn reason_key(e: &str) -> String {
    let mut s = String::new();
    let mut digit = false;
    for ch in e.chars() {
        if ch.is_ascii_digit() {
            if !digit {
                s.push('N');
            }
            digit = true;
        } else {
            digit = false;
            s.push(ch);
        }
    }
    s.chars().take(90).collect()
}

// ---------------------------------------------------------------------------------------------
// Jobs

/// A CART job's uuid in the library: made from its CART row, so a second import finds it.
fn job_uuid(j: &CartJob) -> String {
    let h = jetcarp_core::sha256::sha256_hex(
        format!("cart-job\n{}\n{}\n{}\n{}", j.id, j.kind, j.created, j.spec).as_bytes(),
    );
    let b: Vec<char> = h.chars().take(32).collect();
    let variant = ['8', '9', 'a', 'b'][(b[16].to_digit(16).unwrap_or(0) & 3) as usize];
    let s = |a: usize, z: usize| b[a..z].iter().collect::<String>();
    format!(
        "{}-{}-8{}-{variant}{}-{}",
        s(0, 8),
        s(8, 12),
        s(13, 16),
        s(17, 20),
        s(20, 32)
    )
}

/// Absolute paths in a job's JSON cut to their file names (a library may be shared).
fn cut_paths(v: &mut Value, n: &mut u64) {
    match v {
        Value::String(s) => {
            let abs = (s.starts_with('/') && s.len() > 1 && !s.starts_with("//"))
                || s.starts_with("~/")
                || (s.len() > 2 && s.as_bytes()[1] == b':' && s.as_bytes()[2] == b'\\');
            if abs && !s.contains('\n') {
                let name = Path::new(s.as_str())
                    .file_name()
                    .map(|x| x.to_string_lossy().to_string())
                    .unwrap_or_default();
                *s = name;
                *n += 1;
            }
        }
        Value::Array(a) => a.iter_mut().for_each(|x| cut_paths(x, n)),
        Value::Object(m) => m.values_mut().for_each(|x| cut_paths(x, n)),
        _ => {}
    }
}

/// CART record ids in a job's progress changed to the new ids (and a find's label with them);
/// finds that were not imported are left out of their lists. Job numbers (a job's own, a
/// pipeline's steps') become the library's.
fn remap_progress(
    v: &mut Value,
    ids: &HashMap<String, String>,
    cart_ids: &HashSet<String>,
    jobs: &HashMap<i64, i64>,
    counts: &mut (u64, u64),
) {
    match v {
        Value::Array(a) => {
            a.retain(|x| {
                let gone = x
                    .get("id")
                    .and_then(|i| i.as_str())
                    .is_some_and(|i| cart_ids.contains(i) && !ids.contains_key(i));
                if gone {
                    counts.1 += 1;
                }
                !gone
            });
            for x in a.iter_mut() {
                remap_progress(x, ids, cart_ids, jobs, counts);
            }
        }
        Value::Object(m) => {
            if let Some(old) = m.get("id").and_then(|i| i.as_str()).map(String::from)
                && let Some(new) = ids.get(&old)
            {
                m.insert("id".into(), Value::String(new.clone()));
                if let Some(Value::String(l)) = m.get_mut("label") {
                    let tag = format!("#{}", &old[..old.len().min(6)]);
                    if l.ends_with(&tag) {
                        let keep = l.len() - tag.len();
                        l.truncate(keep);
                        l.push('#');
                        l.push_str(&new[..6]);
                    }
                }
                counts.0 += 1;
            }
            // A job names itself by number, and a pipeline's steps name their jobs.
            if let Some(Value::Number(j)) = m.get("job")
                && let Some(x) = j.as_i64().and_then(|n| jobs.get(&n))
            {
                m.insert("job".into(), json!(x));
            }
            if let Some(Value::Array(js)) = m.get_mut("jobs") {
                for j in js.iter_mut() {
                    if let Some(n) = j.as_i64() {
                        if let Some(x) = jobs.get(&n) {
                            *j = json!(x);
                        }
                    } else if let Some(n) = j.get("id").and_then(|i| i.as_i64())
                        && let Some(x) = jobs.get(&n)
                    {
                        j["id"] = json!(x);
                    }
                }
            }
            for x in m.values_mut() {
                remap_progress(x, ids, cart_ids, jobs, counts);
            }
        }
        _ => {}
    }
}

// ---------------------------------------------------------------------------------------------
// The import

/// A record to store: its CART ids (earliest first), the rules it verified in, its sightings.
struct Group {
    id: String,
    members: Vec<usize>,
}

fn run_in(o: &Options) -> Result<Report, String> {
    let t0 = Instant::now();
    let mut rep = Report::default();
    let cart = open_cart(&o.cart)?;
    let file_name = o
        .cart
        .file_name()
        .map(|f| f.to_string_lossy().to_string())
        .unwrap_or_else(|| "catalog.sqlite".into());
    let made_by = made_by(&cart)?;
    let origin =
        Origin::found_elsewhere(FINDER, ORCID, &format!("{} catalog {file_name}", made_by.0));

    // The library, made if new; a second connection writes what the library's own API has no
    // call for (jobs with their dates, sightings with theirs, census rows).
    let lib = Library::open(&o.library);
    lib.add_sightings(0, &[])?;
    let db = Connection::open(lib.path()).map_err(sql)?;
    db.busy_timeout(std::time::Duration::from_secs(60))
        .map_err(sql)?;
    db.execute_batch("PRAGMA foreign_keys = ON;").map_err(sql)?;

    // CART's rules, as the records name them.
    let mut rules: HashMap<i64, CartRule> = HashMap::new();
    let mut keys: HashMap<i64, String> = HashMap::new();
    {
        let mut st = cart
            .prepare(
                "SELECT id, notation, neighborhood, steps_per_gen FROM rules WHERE id IN
                    (SELECT rule_id FROM pattern_rules UNION SELECT rule_id FROM sightings)",
            )
            .map_err(sql)?;
        let v = st
            .query_map([], |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, i64>(3)?,
                ))
            })
            .map_err(sql)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(sql)?;
        for (id, notation, nb, spg) in v {
            let nb = match nb.as_str() {
                "touch" => Nb::Touch,
                "edge" => Nb::Edge,
                _ => continue,
            };
            if let Ok(k) = rule_key(&notation, nb, spg as u32) {
                keys.insert(id, k);
            }
            rules.insert(
                id,
                CartRule {
                    notation,
                    nb,
                    spg: spg as u32,
                },
            );
        }
    }
    let mut prs: HashMap<String, Vec<PrRow>> = HashMap::new();
    {
        let mut st = cart
            .prepare(
                "SELECT pattern_id, rule_id, periods, first_seen FROM pattern_rules
                 ORDER BY pattern_id, first_seen, rule_id",
            )
            .map_err(sql)?;
        let mut q = st.query([]).map_err(sql)?;
        while let Some(r) = q.next().map_err(sql)? {
            prs.entry(r.get(0).map_err(sql)?).or_default().push(PrRow {
                rule: r.get(1).map_err(sql)?,
                periods: r.get::<_, i64>(2).map_err(sql)?.max(0) as u64,
                first_seen: r.get(3).map_err(sql)?,
            });
        }
    }
    let mut sights: HashMap<String, Vec<SightRow>> = HashMap::new();
    {
        let mut st = cart
            .prepare(
                "SELECT pattern_id, rule_id, job_id, count, soups, first_seen, last_seen
                 FROM sightings",
            )
            .map_err(sql)?;
        let mut q = st.query([]).map_err(sql)?;
        while let Some(r) = q.next().map_err(sql)? {
            sights
                .entry(r.get(0).map_err(sql)?)
                .or_default()
                .push(SightRow {
                    rule: r.get(1).map_err(sql)?,
                    job: r.get(2).map_err(sql)?,
                    count: r.get::<_, i64>(3).map_err(sql)?.max(0) as u64,
                    soups: r.get::<_, i64>(4).map_err(sql)?.max(0) as u64,
                    first: r.get(5).map_err(sql)?,
                    last: r.get(6).map_err(sql)?,
                });
        }
    }
    let t_read = t0.elapsed().as_secs_f64();

    // Jobs (before the records, whose provenance names them).
    let mut cjobs: Vec<CartJob> = {
        let mut st = cart
            .prepare(
                "SELECT id, kind, spec, status, progress, created, updated, finished FROM jobs
                 ORDER BY id",
            )
            .map_err(sql)?;
        st.query_map([], |r| {
            Ok(CartJob {
                id: r.get(0)?,
                kind: r.get(1)?,
                spec: r.get(2)?,
                status: r.get(3)?,
                progress: r.get(4)?,
                created: r.get(5)?,
                updated: r.get(6)?,
                finished: r.get(7)?,
            })
        })
        .map_err(sql)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(sql)?
    };
    let mut unstored: HashMap<i64, (BTreeMap<String, u64>, u64)> = HashMap::new();
    if has_table(&cart, "unstored") {
        let mut st = cart
            .prepare("SELECT job_id, class, count(*), sum(count) FROM unstored GROUP BY 1, 2")
            .map_err(sql)?;
        let v = st
            .query_map([], |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, i64>(2)?,
                    r.get::<_, i64>(3)?,
                ))
            })
            .map_err(sql)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(sql)?;
        for (job, class, objects, copies) in v {
            let name = PatternClass::parse(&class)
                .map(|c| c.name().to_string())
                .unwrap_or(class);
            let e = unstored.entry(job).or_default();
            *e.0.entry(name).or_default() += copies.max(0) as u64;
            e.1 += objects.max(0) as u64;
            rep.unstored_objects += objects.max(0) as u64;
            rep.unstored_copies += copies.max(0) as u64;
        }
    }
    let mut job_ids: HashMap<i64, i64> = HashMap::new();
    let mut job_progress: Vec<(String, i64, Value)> = vec![];
    {
        let tx = db.unchecked_transaction().map_err(sql)?;
        for j in &mut cjobs {
            if j.id == 0 {
                job_ids.insert(0, 0);
                continue;
            }
            let uuid = job_uuid(j);
            let mut spec: Value = serde_json::from_str(&j.spec).unwrap_or_else(|_| json!({}));
            cut_paths(&mut spec, &mut rep.paths_cut);
            let mut progress: Value =
                serde_json::from_str(&j.progress).unwrap_or_else(|_| json!({}));
            cut_paths(&mut progress, &mut rep.paths_cut);
            if let (Some(m), Some((by, n))) = (progress.as_object_mut(), unstored.get(&j.id))
                && !m.contains_key("unstored")
            {
                m.insert("unstored".into(), json!(by));
                m.insert("unstored_distinct".into(), json!(n));
            }
            let status = match j.status.as_str() {
                "done" | "failed" => j.status.clone(),
                _ => "failed".to_string(),
            };
            let have: Option<i64> = tx
                .query_row("SELECT id FROM jobs WHERE uuid = ?1", [&uuid], |r| r.get(0))
                .optional()
                .map_err(sql)?;
            let id = match have {
                Some(id) => id,
                None => {
                    tx.execute(
                        "INSERT INTO jobs (uuid, kind, spec, status, progress, engine_version,
                            created, updated, finished)
                         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                        params![
                            uuid,
                            j.kind,
                            spec.to_string(),
                            status,
                            progress.to_string(),
                            made_by.1,
                            j.created,
                            j.updated,
                            j.finished.clone().unwrap_or_else(|| j.updated.clone()),
                        ],
                    )
                    .map_err(sql)?;
                    rep.jobs_added += 1;
                    tx.last_insert_rowid()
                }
            };
            job_ids.insert(j.id, id);
            job_progress.push((uuid, j.id, progress));
        }
        // Census rows.
        if has_table(&cart, "census_rows") {
            let mut st = cart
                .prepare("SELECT job_id, idx, rule, summary FROM census_rows ORDER BY 1, 2")
                .map_err(sql)?;
            let v = st
                .query_map([], |r| {
                    Ok((
                        r.get::<_, i64>(0)?,
                        r.get::<_, i64>(1)?,
                        r.get::<_, String>(2)?,
                        r.get::<_, String>(3)?,
                    ))
                })
                .map_err(sql)?
                .collect::<Result<Vec<_>, _>>()
                .map_err(sql)?;
            let mut ins = tx
                .prepare(
                    "INSERT OR IGNORE INTO census_rows (job_id, idx, rule, summary)
                     VALUES (?1, ?2, ?3, ?4)",
                )
                .map_err(sql)?;
            for (job, idx, rule, summary) in v {
                if let Some(&j) = job_ids.get(&job) {
                    rep.census_rows_written +=
                        ins.execute(params![j, idx, rule, summary]).map_err(sql)? as u64;
                }
            }
        }
        tx.commit().map_err(sql)?;
    }
    rep.jobs = job_ids.len() as u64 - job_ids.contains_key(&0) as u64;

    // Records, one tiling at a time (records with one content id share a tiling).
    let tilings: Vec<String> = {
        let mut st = cart
            .prepare("SELECT DISTINCT tiling_id FROM patterns ORDER BY 1")
            .map_err(sql)?;
        st.query_map([], |r| r.get(0))
            .map_err(sql)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(sql)?
    };
    let mut cart_ids: HashSet<String> = HashSet::new();
    let mut new_of: HashMap<String, String> = HashMap::new();
    let mut lib_rule: HashMap<String, i64> = HashMap::new();
    let (mut t_check, mut t_write) = (0.0, 0.0);
    for tiling in &tilings {
        let t1 = Instant::now();
        let read = read_tiling(&cart, tiling)?;
        let mut carts: Vec<Cart> = Vec::with_capacity(read.len());
        for r in read {
            rep.records += 1;
            match r {
                Ok(c) => carts.push(c),
                Err((mut m, e)) => {
                    cart_ids.insert(m.cart.clone());
                    fail_row(&mut rep, &mut m, &e);
                    rep.mapping.push(m);
                }
            }
        }
        let empty = vec![];
        let done: Vec<Done> = carts
            .par_iter()
            .enumerate()
            .map(|(i, c)| check(i, c, prs.get(&c.id).unwrap_or(&empty), &rules, &keys))
            .collect();
        t_check += t1.elapsed().as_secs_f64();
        let t2 = Instant::now();

        // Group by content id; record what became of each CART id.
        let mut groups: Vec<Group> = vec![];
        let mut at: HashMap<String, usize> = HashMap::new();
        let mut rows: Vec<MapRow> = Vec::with_capacity(done.len());
        for d in &done {
            let c = &carts[d.cart];
            cart_ids.insert(c.id.clone());
            if c.rec.created != c.rec.provenance.date {
                rep.dates_differ += 1;
            }
            let mut m = MapRow {
                cart: c.id.clone(),
                tiling: c.tiling.clone(),
                class: c.class.clone(),
                period: c.period,
                rule: c.rule.clone(),
                ..Default::default()
            };
            let rec = match &d.rec {
                Ok(r) => r,
                Err(e) => {
                    fail_row(&mut rep, &mut m, e);
                    rep.sightings_of_failed += sights.get(&c.id).map_or(0, |v| v.len()) as u64;
                    rows.push(m);
                    continue;
                }
            };
            m.new = rec.id.clone();
            let ok = d.rules.iter().filter(|x| x.result.is_ok()).count();
            if ok == 0 {
                let why = d
                    .rules
                    .first()
                    .and_then(|x| x.result.as_ref().err().cloned())
                    .unwrap_or_else(|| "no rule".into());
                fail_row(&mut rep, &mut m, &format!("in {}: {why}", c.rule));
                rep.sightings_of_failed += sights.get(&c.id).map_or(0, |v| v.len()) as u64;
                rows.push(m);
                continue;
            }
            rep.rules_verified += ok as u64;
            if ok < d.rules.len() {
                rep.partial += 1;
                rep.rules_dropped += (d.rules.len() - ok) as u64;
                *rep.partial_by
                    .entry(format!("{} {}", c.tiling, c.class))
                    .or_default() += 1;
                for x in &d.rules {
                    if let Err(e) = &x.result {
                        *rep.partial_reasons.entry(reason_key(e)).or_default() += 1;
                        rep.partial_rows.push(format!(
                            "{}\t{}\t{}\t{}\t{}\t{}",
                            c.id,
                            c.tiling,
                            c.class,
                            c.period,
                            x.notation,
                            e.replace(['\t', '\n'], " ")
                        ));
                    }
                }
            }
            if d.range_error {
                rep.range_errors += 1;
            }
            m.status = "imported".into();
            let g = *at.entry(rec.id.clone()).or_insert_with(|| {
                groups.push(Group {
                    id: rec.id.clone(),
                    members: vec![],
                });
                groups.len() - 1
            });
            groups[g].members.push(d.cart);
            rows.push(m);
        }
        // Failed records whose content id came in from another CART id.
        for m in &mut rows {
            if m.status == "failed" && !m.new.is_empty() && at.contains_key(&m.new) {
                m.status = "failed-merged".into();
            } else if m.status == "failed" {
                m.new.clear();
            }
        }
        let by_cart: HashMap<usize, &Done> = done.iter().map(|d| (d.cart, d)).collect();
        for g in &mut groups {
            g.members.sort_by(|&a, &b| {
                (&carts[a].found, &carts[a].id).cmp(&(&carts[b].found, &carts[b].id))
            });
            if g.members.len() > 1 {
                rep.merged_groups += 1;
                rep.merged_ids += g.members.len() as u64;
                for &i in &g.members {
                    if let Some(m) = rows.iter_mut().find(|m| m.cart == carts[i].id) {
                        m.status = "merged".into();
                    }
                }
            }
            for &i in &g.members {
                new_of.insert(carts[i].id.clone(), g.id.clone());
            }
        }
        rep.ids += groups.len() as u64;

        // Build and store the records, credited to the job that saw each first.
        let mut by_job: BTreeMap<i64, Vec<PatternRecord>> = BTreeMap::new();
        let mut sight_rows: Vec<(String, String, i64, SightRow)> = vec![];
        let mut first_seen: Vec<(String, String, String)> = vec![];
        for g in &groups {
            let ds: Vec<&Done> = g.members.iter().map(|i| by_cart[i]).collect();
            let cs: Vec<&Cart> = g.members.iter().map(|&i| &carts[i]).collect();
            // The rules verified (the earliest record's first), each first seen the earliest.
            let mut vrules: Vec<Checked> = vec![];
            for d in &ds {
                for x in d.rules.iter().filter(|x| x.result.is_ok()) {
                    match vrules.iter_mut().find(|y| y.key == x.key) {
                        Some(y) => {
                            if x.first_seen < y.first_seen {
                                y.first_seen = x.first_seen.clone();
                            }
                        }
                        None => vrules.push(x.clone()),
                    }
                }
            }
            let vkeys: HashSet<&str> = vrules.iter().map(|x| x.key.as_str()).collect();
            // Sightings in those rules, summed per (rule, job); the rest dropped.
            let mut seen = 0u64;
            let mut per: BTreeMap<(String, i64), SightRow> = BTreeMap::new();
            let mut first: Option<(String, i64)> = None;
            let mut reduced = false;
            for c in &cs {
                let mut lost = 0u64;
                for s in sights.get(&c.id).into_iter().flatten() {
                    let k = keys.get(&s.rule).cloned().unwrap_or_default();
                    if first
                        .as_ref()
                        .is_none_or(|f| (&s.first, s.job) < (&f.0, f.1))
                    {
                        first = Some((s.first.clone(), s.job));
                    }
                    if !vkeys.contains(k.as_str()) {
                        rep.sightings_dropped += 1;
                        lost += s.count;
                        continue;
                    }
                    let e = per.entry((k, s.job)).or_insert_with(|| SightRow {
                        rule: s.rule,
                        job: s.job,
                        count: 0,
                        soups: 0,
                        first: s.first.clone(),
                        last: s.last.clone(),
                    });
                    e.count += s.count;
                    e.soups += s.soups;
                    if s.first < e.first {
                        e.first = s.first.clone();
                    }
                    if s.last > e.last {
                        e.last = s.last.clone();
                    }
                }
                if lost > 0 {
                    reduced = true;
                }
                seen += c.seen.saturating_sub(lost);
            }
            if reduced {
                rep.seen_reduced += 1;
            }
            for ((k, _), s) in per {
                sight_rows.push((g.id.clone(), k, s.job, s));
            }
            for x in &vrules {
                first_seen.push((g.id.clone(), x.key.clone(), x.first_seen.clone()));
            }
            // The record.
            let mut rec = ds[0].rec.clone().unwrap();
            let base = &cs[0].rec;
            let mut names: Vec<String> = vec![];
            let mut notes: Vec<String> = vec![];
            rec.tags.clear();
            rec.rating = None;
            rec.state = CurationState::New;
            for c in &cs {
                if let Some(n) = c.rec.name.as_ref().filter(|n| !n.trim().is_empty())
                    && !names.contains(n)
                {
                    names.push(n.clone());
                }
                let t = c.rec.notes.trim();
                if !t.is_empty() && !notes.iter().any(|x| x == t) {
                    notes.push(t.to_string());
                }
                for t in &c.rec.tags {
                    if !rec.tags.contains(t) {
                        rec.tags.push(t.clone());
                    }
                }
                rec.rating = rec.rating.max(c.rec.rating);
                if rec.state == CurationState::New {
                    rec.state = c.rec.state;
                }
            }
            rec.name = names.first().cloned();
            if names.len() > 1 {
                notes.push(format!("Also named in CART: {}", names[1..].join(", ")));
            }
            rec.notes = notes.join("\n\n");
            rec.created = cs
                .iter()
                .map(|c| c.created.as_str())
                .min()
                .unwrap()
                .to_string();
            rec.updated = jetcarp_core::pattern::now_utc();
            rec.provenance = base.provenance.clone();
            rec.provenance.date = cs[0].found.clone();
            rec.provenance.jetcarp_version = cs[0].found_with.clone();
            if rec.provenance.source.is_empty() {
                rec.provenance.source = serde_json::to_value(base.discovered_by)
                    .ok()
                    .and_then(|v| v.as_str().map(String::from))
                    .unwrap_or_default();
            }
            rec.discovered_by = base.discovered_by;
            rec.range = ds[0].range.clone();
            rec.rules = vec![];
            rec.engine_version = jetcarp_core::VERSION.into();
            rec.needs_reverify = false;
            // Already stored (an earlier run): only the rules it lacks are added.
            let have: Option<HashSet<String>> = stored_rules(&db, &g.id)?;
            let job = first
                .as_ref()
                .and_then(|f| job_ids.get(&f.1).copied())
                .unwrap_or(0);
            let list = by_job.entry(job).or_default();
            match &have {
                None => rep.added += 1,
                Some(_) => rep.present += 1,
            }
            let mut k = 0;
            for x in &vrules {
                if have.as_ref().is_some_and(|h| h.contains(&x.key)) {
                    continue;
                }
                let mut r = rec.clone();
                r.rule = x.notation.clone();
                r.verified = x.result.clone().unwrap();
                r.seen = if k == 0 && have.is_none() { seen } else { 0 };
                if have.is_some() {
                    rep.rules_added_to_present += 1;
                }
                list.push(r);
                k += 1;
            }
        }
        rows.sort_by(|a, b| a.cart.cmp(&b.cart));
        rep.mapping.extend(rows);
        for (job, recs) in by_job {
            let mut it = recs.into_iter().peekable();
            while it.peek().is_some() {
                let chunk: Vec<PatternRecord> = it.by_ref().take(CHUNK).collect();
                for r in lib.add_many_as(chunk, job, &origin)? {
                    if let Err(e) = r {
                        rep.add_errors.push(e);
                    }
                }
            }
        }
        // Sightings and first-seen dates, where missing or later.
        let tx = db.unchecked_transaction().map_err(sql)?;
        {
            let mut ins = tx
                .prepare_cached(
                    "INSERT INTO sightings (pattern_id, rule_id, job_id, count, soups,
                        first_seen, last_seen) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
                     ON CONFLICT (pattern_id, rule_id, job_id) DO NOTHING",
                )
                .map_err(sql)?;
            for (id, k, job, s) in &sight_rows {
                let (Some(rule), Some(&j)) =
                    (lib_rule_id(&tx, &mut lib_rule, k)?, job_ids.get(job))
                else {
                    continue;
                };
                rep.sightings_written += ins
                    .execute(params![
                        id,
                        rule,
                        j,
                        s.count as i64,
                        s.soups as i64,
                        s.first,
                        s.last
                    ])
                    .map_err(sql)? as u64;
            }
            let mut up = tx
                .prepare_cached(
                    "UPDATE pattern_rules SET first_seen = ?3
                     WHERE pattern_id = ?1 AND rule_id = ?2 AND first_seen > ?3",
                )
                .map_err(sql)?;
            for (id, k, date) in &first_seen {
                if let Some(rule) = lib_rule_id(&tx, &mut lib_rule, k)? {
                    up.execute(params![id, rule, date]).map_err(sql)?;
                }
            }
        }
        tx.commit().map_err(sql)?;
        t_write += t2.elapsed().as_secs_f64();
        eprintln!(
            "{tiling}: {} records, {} ids ({:.1} s checking, {:.1} s in all)",
            carts.len(),
            groups.len(),
            t1.elapsed().as_secs_f64() - t2.elapsed().as_secs_f64(),
            t1.elapsed().as_secs_f64()
        );
    }

    // The jobs' progress with the new ids.
    {
        let tx = db.unchecked_transaction().map_err(sql)?;
        let mut counts = (0u64, 0u64);
        for (uuid, _, p) in &mut job_progress {
            remap_progress(p, &new_of, &cart_ids, &job_ids, &mut counts);
            tx.execute(
                "UPDATE jobs SET progress = ?2 WHERE uuid = ?1 AND progress != ?2",
                params![uuid.as_str(), p.to_string()],
            )
            .map_err(sql)?;
        }
        tx.commit().map_err(sql)?;
        rep.progress_ids = counts.0;
        rep.progress_dropped = counts.1;
    }
    rep.seconds = t0.elapsed().as_secs_f64();
    rep.text = summary(&rep, t_read, t_check, t_write);
    let dir = o.report.clone().unwrap_or_else(|| PathBuf::from("."));
    write_files(&rep, &dir)?;
    Ok(rep)
}

fn fail_row(rep: &mut Report, m: &mut MapRow, e: &str) {
    rep.failed += 1;
    *rep.failed_by
        .entry(format!("{} {}", m.tiling, m.class))
        .or_default() += 1;
    *rep.reasons.entry(reason_key(e)).or_default() += 1;
    m.status = "failed".into();
    m.reason = e.replace(['\t', '\n'], " ");
}

/// The rules a stored record has (keys as [`rule_key`] makes them), or `None` if it is not
/// stored.
fn stored_rules(db: &Connection, id: &str) -> Result<Option<HashSet<String>>, String> {
    let have: bool = db
        .prepare_cached("SELECT 1 FROM patterns WHERE id = ?1")
        .map_err(sql)?
        .exists([id])
        .map_err(sql)?;
    if !have {
        return Ok(None);
    }
    let mut st = db
        .prepare_cached(
            "SELECT r.notation, r.neighborhood, r.steps_per_gen FROM pattern_rules pr
             JOIN rules r ON r.id = pr.rule_id WHERE pr.pattern_id = ?1",
        )
        .map_err(sql)?;
    let v = st
        .query_map([id], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, i64>(2)?,
            ))
        })
        .map_err(sql)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(sql)?;
    Ok(Some(
        v.into_iter()
            .map(|(n, nb, spg)| format!("{n}|{nb}|{spg}"))
            .collect(),
    ))
}

/// The library's id for a rule key (the rule is stored by the records that verified in it).
fn lib_rule_id(
    tx: &Connection,
    cache: &mut HashMap<String, i64>,
    key: &str,
) -> Result<Option<i64>, String> {
    if let Some(&id) = cache.get(key) {
        return Ok(Some(id));
    }
    let mut p = key.splitn(3, '|');
    let (Some(n), Some(nb), Some(spg)) = (p.next(), p.next(), p.next()) else {
        return Ok(None);
    };
    let id: Option<i64> = tx
        .prepare_cached(
            "SELECT id FROM rules WHERE notation = ?1 AND neighborhood = ?2 AND steps_per_gen = ?3",
        )
        .map_err(sql)?
        .query_row(params![n, nb, spg.parse::<i64>().unwrap_or(1)], |r| {
            r.get(0)
        })
        .optional()
        .map_err(sql)?;
    if let Some(id) = id {
        cache.insert(key.to_string(), id);
    }
    Ok(id)
}

// ---------------------------------------------------------------------------------------------
// Reports

fn summary(r: &Report, t_read: f64, t_check: f64, t_write: f64) -> String {
    let mut s = String::new();
    let _ = writeln!(
        s,
        "CART records read {}; content ids imported {}; records stored by this run {}, already \
         in the library {} (rules added to those: {})",
        r.records, r.ids, r.added, r.present, r.rules_added_to_present
    );
    let _ = writeln!(
        s,
        "failed in every rule {}; verified in some rules only {} ({} record-rule pairs dropped; \
         {} verified)",
        r.failed, r.partial, r.rules_dropped, r.rules_verified
    );
    let _ = writeln!(
        s,
        "merged: {} groups covering {} CART ids",
        r.merged_groups, r.merged_ids
    );
    let _ = writeln!(
        s,
        "sightings written {}; dropped {} (in failed rules) + {} (of failed records); seen \
         counts reduced {}",
        r.sightings_written, r.sightings_dropped, r.sightings_of_failed, r.seen_reduced
    );
    let _ = writeln!(
        s,
        "jobs {} ({} added); census rows written {}; unstored objects {} ({} copies) kept as \
         counts",
        r.jobs, r.jobs_added, r.census_rows_written, r.unstored_objects, r.unstored_copies
    );
    let _ = writeln!(
        s,
        "job progress: {} CART ids changed to new ids, {} failed finds left out; {} absolute \
         paths cut to file names",
        r.progress_ids, r.progress_dropped, r.paths_cut
    );
    let _ = writeln!(
        s,
        "rule ranges not computed {}; CART created differs from the record's date {}; store \
         errors {}",
        r.range_errors,
        r.dates_differ,
        r.add_errors.len()
    );
    for e in r.add_errors.iter().take(10) {
        let _ = writeln!(s, "  store error: {e}");
    }
    if !r.failed_by.is_empty() {
        let _ = writeln!(s, "failures by tiling and class:");
        for (k, n) in &r.failed_by {
            let _ = writeln!(s, "  {k}: {n}");
        }
        let _ = writeln!(s, "failures by reason:");
        let mut v: Vec<_> = r.reasons.iter().collect();
        v.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
        for (k, n) in v {
            let _ = writeln!(s, "  {n}: {k}");
        }
    }
    if !r.partial_by.is_empty() {
        let _ = writeln!(
            s,
            "records verified in some rules only, by tiling and class:"
        );
        for (k, n) in &r.partial_by {
            let _ = writeln!(s, "  {k}: {n}");
        }
        let _ = writeln!(s, "rules dropped, by reason:");
        let mut v: Vec<_> = r.partial_reasons.iter().collect();
        v.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
        for (k, n) in v {
            let _ = writeln!(s, "  {n}: {k}");
        }
    }
    let ships: Vec<&MapRow> = r
        .mapping
        .iter()
        .filter(|m| m.class == "spaceship")
        .collect();
    let ok = ships
        .iter()
        .filter(|m| !m.new.is_empty() && !m.status.starts_with("failed"))
        .count();
    let _ = writeln!(
        s,
        "spaceships: {} ({} off the square grid), {} verified",
        ships.len(),
        ships.iter().filter(|m| m.tiling != "square").count(),
        ok
    );
    for m in &ships {
        let _ = writeln!(
            s,
            "  {}  {:<14} p{:<3} {:<34} {:<8} {}{}",
            m.cart,
            m.tiling,
            m.period,
            m.rule,
            m.status,
            if m.new.is_empty() { "-" } else { &m.new },
            if m.reason.is_empty() {
                String::new()
            } else {
                format!("  ({})", m.reason)
            }
        );
    }
    let _ = writeln!(
        s,
        "time: {:.1} s in all (reading {t_read:.1} s, checking {t_check:.1} s, the rest \
         writing {t_write:.1} s){}",
        r.seconds,
        peak_memory()
            .map(|m| format!("; peak memory {m}"))
            .unwrap_or_default()
    );
    s
}

/// The process's peak resident memory, where the system says (Linux).
fn peak_memory() -> Option<String> {
    let s = std::fs::read_to_string("/proc/self/status").ok()?;
    let l = s.lines().find(|l| l.starts_with("VmHWM:"))?;
    Some(l["VmHWM:".len()..].trim().to_string())
}

fn write_files(r: &Report, dir: &Path) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let mut map = String::from("cart_id\tnew_id\ttiling\tclass\tstatus\n");
    let mut fails = String::from("cart_id\ttiling\tclass\tperiod\trule\treason\n");
    for m in &r.mapping {
        let _ = writeln!(
            map,
            "{}\t{}\t{}\t{}\t{}",
            m.cart,
            if m.new.is_empty() { "-" } else { &m.new },
            m.tiling,
            m.class,
            m.status
        );
        if m.status == "failed" || m.status == "failed-merged" {
            let _ = writeln!(
                fails,
                "{}\t{}\t{}\t{}\t{}\t{}",
                m.cart, m.tiling, m.class, m.period, m.rule, m.reason
            );
        }
    }
    let mut partial = String::from("cart_id\ttiling\tclass\tperiod\trule\treason\n");
    for l in &r.partial_rows {
        partial.push_str(l);
        partial.push('\n');
    }
    for (f, text) in [
        ("cart-to-new-ids.tsv", &map),
        ("cart-failures.tsv", &fails),
        ("cart-rules-dropped.tsv", &partial),
        ("cart-import-report.txt", &r.text),
    ] {
        let p = dir.join(f);
        std::fs::write(&p, text).map_err(|e| format!("{}: {e}", p.display()))?;
    }
    Ok(())
}
