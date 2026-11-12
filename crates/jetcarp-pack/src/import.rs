//! Importing a version of a pack (a store or an export) into a library.
//!
//! 1. The version is read and checked ([`crate::read`]): format, version, `requires`, the
//!    signatures and the publisher's key statement, every index, and then every object's hash
//!    (every part, segment, media file and reference patch) before anything is stored. A pack
//!    not signed by its publisher is refused unless `allow_unsigned`; one with a signature that
//!    does not verify is always refused.
//! 2. Every content id is checked against its identity and pin, and the identity's phases
//!    against the typed `phases`; a lattice tiling's pin against its `tilings` columns, and an
//!    aperiodic one's against its reference patch.
//! 3. Records are merged by content id: a pattern the library has keeps its own facts (with
//!    `strict`, a pack row that differs from another for the same id is a refusal); events are
//!    added: the further rules a pattern was verified in, sightings (per search: the larger
//!    count, so importing again changes nothing), ratings, aliases. A new record keeps its
//!    finder and find date (its provenance row 0 and history from the pack, then an
//!    `imported` row naming the manifest; [`jetcarp_store::library::Origin::with_history`]).
//! 4. A record whose pin, or whose rule family's semantics version, differs from this engine's
//!    is stored as it is, flagged `needs_reverify` (`jetcarp library reverify` checks it); every
//!    other new record is put in canonical form first, which must give its id.
//! 5. Retractions are recorded as flags (`pack_retractions`); nothing is deleted.
//! 6. A sample of the new patterns (3,000 by default) is verified again, within a time budget;
//!    the result is recorded in the library (`pack_imports`), not written to the pack.
//! 7. Nothing from the pack is run: scenes' scripts stay off.

use crate::docs::Retract;
use crate::read::{self, Source, Version};
use crate::rows;
use crate::tables::*;
use jetcarp_core::pattern::verify::{default_periods, verify};
use jetcarp_core::pattern::{CurationState, DiscoveredBy, PatternRecord, Provenance};
use jetcarp_store::Library;
use jetcarp_store::library::pack::{ImportSighting, PackImportRow, PackMedia, RetractionRow};
use jetcarp_store::library::{AliasRow, History as LibHistory, Origin, ProvenanceRow as LibProv};
use rayon::prelude::*;
use serde::Serialize;
use serde_json::{Value, json};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::PathBuf;
use std::time::Instant;

/// How much of a pack to verify again on import.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub enum VerifyMode {
    Sample(u64),
    Full,
    None,
}

#[derive(Clone, Debug)]
pub struct Options {
    pub path: PathBuf,
    pub feed: Option<String>,
    pub manifest: Option<String>,
    pub allow_unsigned: bool,
    pub strict: bool,
    pub verify: VerifyMode,
    /// The sample's seed (default: from the manifest hash, so the same pack samples the same).
    pub seed: Option<u64>,
    /// Most seconds verification may take in all.
    pub verify_seconds: f64,
    /// Check the pack only; store nothing.
    pub dry_run: bool,
}

impl Default for Options {
    fn default() -> Options {
        Options {
            path: PathBuf::new(),
            feed: None,
            manifest: None,
            allow_unsigned: false,
            strict: false,
            verify: VerifyMode::Sample(3000),
            seed: None,
            verify_seconds: 600.0,
            dry_run: false,
        }
    }
}

/// The verification sample.
#[derive(Clone, Debug, Default, Serialize)]
pub struct Sample {
    pub mode: String,
    pub n: u64,
    pub seed: String,
    pub checked: u64,
    pub passed: u64,
    /// Over the time budget, or too large for one record's budget.
    pub skipped: u64,
    pub failures: Vec<String>,
    pub seconds: f64,
}

/// What an import did.
#[derive(Clone, Debug, Default, Serialize)]
pub struct Imported {
    pub manifest: String,
    pub feed: String,
    pub publisher: String,
    pub publisher_name: String,
    /// `publisher` (signed by its publisher's key), `unsigned`.
    pub signed: String,
    pub patterns_added: u64,
    pub patterns_merged: u64,
    pub flagged: u64,
    pub ants: u64,
    pub quickstart: u64,
    pub sightings: u64,
    pub ratings: u64,
    pub aliases: u64,
    pub media: u64,
    pub searches: Vec<String>,
    /// The searches whose working databases were rebuilt.
    pub rebuilt: Vec<jetcarp_search::jobs::explore::Rebuilt>,
    pub retracted: u64,
    pub objects_checked: u64,
    pub bytes_checked: u64,
    pub sample: Sample,
    pub notes: Vec<String>,
    pub dry_run: bool,
}

fn de<T: serde::de::DeserializeOwned>(v: Value, what: &str) -> Result<T, String> {
    serde_json::from_value(v).map_err(|e| format!("{what}: {e}"))
}

fn split_target(t: &str) -> Result<(&str, &str), String> {
    t.split_once(':')
        .filter(|(k, id)| {
            matches!(*k, "pattern" | "ant" | "quickstart" | "search_run") && !id.is_empty()
        })
        .ok_or_else(|| format!("target '{t}' is not kind:id"))
}

/// Check every object a version names against its hash (parts as they are read later; here
/// the search segments, score sidecars, media files and reference patches).
fn check_objects(src: &Source, v: &Version, out: &mut Imported) -> Result<(), String> {
    for tp in v.tables.values() {
        for (p, path) in &tp.parts {
            let (hex, n) = crate::hash::file_hex(path, crate::store::MAX_OBJECT)?;
            if format!("sha256:{hex}") != p.object || n != p.bytes {
                return Err(format!(
                    "part {} ({}) is altered or truncated",
                    p.object,
                    src.within(path)
                ));
            }
            out.objects_checked += 1;
            out.bytes_checked += n;
        }
    }
    for sp in v.searches.values() {
        let mut check = |obj: &str, bytes: u64, f: &PathBuf| -> Result<(), String> {
            let (hex, n) = crate::hash::file_hex(f, crate::store::MAX_OBJECT)?;
            if format!("sha256:{hex}") != obj || n != bytes {
                return Err(format!(
                    "search object {obj} ({}) is altered or truncated",
                    src.within(f)
                ));
            }
            out.objects_checked += 1;
            out.bytes_checked += n;
            Ok(())
        };
        for (p, f) in sp.index.parts.iter().zip(&sp.run_files) {
            check(&p.object, p.bytes, f)?;
        }
        for (s, fs) in sp.index.scores.iter().zip(&sp.score_files) {
            for (p, f) in s.parts.iter().zip(fs) {
                check(&p.object, p.bytes, f)?;
            }
        }
    }
    Ok(())
}

/// Read a whole table's rows.
fn rows_of<T: serde::de::DeserializeOwned>(
    src: &Source,
    v: &Version,
    table: &str,
) -> Result<Vec<(usize, T)>, String> {
    let mut out = vec![];
    src.table_rows(v, table, |part, rows| {
        for r in rows {
            out.push((part, de(r, table)?));
        }
        Ok(())
    })?;
    Ok(out)
}

/// The pin this engine gives a pin key (none when it cannot build the tiling).
fn engine_pin(key: &str) -> Option<String> {
    let spec = rows::tiling_of_key(key).ok()?;
    jetcarp_core::pin::geometry_pin(&spec).ok().map(|p| p.pin)
}

/// Import a version into a library.
pub fn import(lib: &Library, opts: &Options, log: &dyn Fn(&str)) -> Result<Imported, String> {
    let t0 = Instant::now();
    let src = Source::open(&opts.path)?;
    let h = match &opts.manifest {
        Some(h) => h.clone(),
        None => src.default_manifest(opts.feed.as_deref())?,
    };
    let v = src.version(&h)?;
    let m = &v.manifest;
    let mut out = Imported {
        manifest: h.clone(),
        feed: m.feed.clone(),
        publisher: m.publisher.clone(),
        publisher_name: v.signed.name.clone(),
        dry_run: opts.dry_run,
        ..Default::default()
    };
    // A key revoked once stays revoked, whatever this store's statements say now.
    let revoked = lib.revoked_keys()?;
    if !opts.dry_run {
        lib.record_revoked(&v.signed.revoked)?;
    }
    let by_publisher = v.signed.by_publisher
        && !v
            .signed
            .publisher_signers
            .iter()
            .all(|k| revoked.contains(k));
    if let Some(b) = v.signed.bad.first() {
        return Err(format!("a signature on this version does not verify: {b}"));
    }
    out.signed = if by_publisher {
        "publisher".into()
    } else if opts.allow_unsigned {
        "unsigned".into()
    } else {
        return Err(format!(
            "this version is not signed by its publisher {} (allow unsigned packs to import it anyway)",
            m.publisher
        ));
    };
    log(&format!(
        "{h}: feed {}, by {}{}, checking every object",
        m.feed,
        m.publisher,
        if v.signed.name.is_empty() {
            String::new()
        } else {
            format!(" ({})", v.signed.name)
        }
    ));
    check_objects(&src, &v, &mut out)?;

    // Tilings and rules: pins and semantics against this engine's.
    let tilings: Vec<(usize, TilingRow)> = rows_of(&src, &v, "tilings")?;
    let mut stale_pins: HashSet<String> = HashSet::new();
    let mut ref_patches: Vec<(String, String)> = vec![];
    for (_, t) in &tilings {
        let patch = match &t.reference {
            Some(rh) => {
                let rel = read::export_reference_path(&t.family).ok_or("unsafe family name")?;
                let b = src.object(rh, "parquet", &rel, crate::store::MAX_OBJECT)?;
                out.objects_checked += 1;
                out.bytes_checked += b.len() as u64;
                let rows = crate::parquet_io::read_all(b.into(), &reference_patch().cols)?;
                let tiles: Vec<RefTileRow> = rows
                    .into_iter()
                    .map(|r| de(r, "reference patch"))
                    .collect::<Result<_, _>>()?;
                ref_patches.push((t.family.clone(), rh.clone()));
                Some(tiles)
            }
            None => None,
        };
        if t.kind == "lattice" || patch.is_some() {
            let (pin, ph, nh) = rows::pin_of_rows(t, patch.as_deref())?;
            if pin != t.pin || ph != t.polygon_hash || nh != t.neighbour_hash {
                return Err(format!(
                    "tiling {}: its geometry columns give pin {pin}, not its pin {}",
                    t.pin_key, t.pin
                ));
            }
        }
        if engine_pin(&t.pin_key).as_deref() != Some(t.pin.as_str()) {
            stale_pins.insert(t.pin_key.clone());
        }
    }
    let rules: Vec<(usize, RuleRow)> = rows_of(&src, &v, "rules")?;
    let mut stale_families: HashSet<String> = HashSet::new();
    for (_, r) in &rules {
        if jetcarp_core::semantics::version(&r.family) != Some(r.semantics.as_str()) {
            stale_families.insert(r.family.clone());
        }
    }
    if !stale_pins.is_empty() {
        out.notes.push(format!(
            "{} tilings' geometry differs from this engine's: their records are flagged needs_reverify",
            stale_pins.len()
        ));
    }

    // Events, keyed by record.
    log("reading verifications, sightings, provenance and annotations");
    let mut verifs: HashMap<String, Vec<VerificationRow>> = HashMap::new();
    for (_, r) in rows_of::<VerificationRow>(&src, &v, "verifications")? {
        verifs.entry(r.pattern_id.clone()).or_default().push(r);
    }
    let mut prov: HashMap<String, Vec<ProvenanceRow>> = HashMap::new();
    for (_, r) in rows_of::<ProvenanceRow>(&src, &v, "provenance")? {
        split_target(&r.target)?;
        prov.entry(r.target.clone()).or_default().push(r);
    }
    // (target, publisher, field) -> the row a reader shows: the highest rev, then date, value.
    let mut ann: HashMap<(String, String, String), Value> = HashMap::new();
    src.table_rows(&v, "annotations", |_, rows| {
        for r in rows {
            let k = (
                r["target"].as_str().unwrap_or("").to_string(),
                r["publisher"].as_str().unwrap_or("").to_string(),
                r["field"].as_str().unwrap_or("").to_string(),
            );
            if ann
                .get(&k)
                .is_none_or(|old| read::later_annotation(old, &r))
            {
                ann.insert(k, r);
            }
        }
        Ok(())
    })?;
    // The values a record gets: its publisher's first, then others' for fields still empty.
    let mut fields: HashMap<String, BTreeMap<String, Value>> = HashMap::new();
    let mut keys: Vec<&(String, String, String)> = ann.keys().collect();
    keys.sort_by(|a, b| {
        ((a.1 != m.publisher), &a.1, &a.0, &a.2).cmp(&((b.1 != m.publisher), &b.1, &b.0, &b.2))
    });
    for k in keys {
        let val: Value =
            serde_json::from_str(ann[k]["value"].as_str().unwrap_or("null")).unwrap_or(Value::Null);
        fields
            .entry(k.0.clone())
            .or_default()
            .entry(k.2.clone())
            .or_insert(val);
    }

    // Patterns.
    log(if opts.dry_run {
        "checking ids and patterns (a check stores nothing)"
    } else {
        "checking ids and storing patterns"
    });
    let mut seen_ids: HashMap<String, String> = HashMap::new();
    // Every pattern stored, and whether it is new.
    let mut kept: Vec<(String, bool)> = vec![];
    let mut flagged_ids: Vec<String> = vec![];
    let mut other_rules: Vec<(
        String,
        String,
        jetcarp_core::pattern::verify::Verification,
        String,
    )> = vec![];
    let retracted: HashSet<&str> = m.retracts.iter().map(|r| r.target.as_str()).collect();
    let mut batch: Vec<(PatternRecord, Origin, bool)> = vec![];
    let now = jetcarp_core::pattern::now_utc();
    let flush = |batch: &mut Vec<(PatternRecord, Origin, bool)>,
                 out: &mut Imported,
                 kept: &mut Vec<(String, bool)>|
     -> Result<(), String> {
        if batch.is_empty() || opts.dry_run {
            batch.clear();
            return Ok(());
        }
        // Canonical form first (in parallel): it must give the pack's id.
        let ready: Vec<Result<(PatternRecord, Origin), String>> = std::mem::take(batch)
            .into_par_iter()
            .map(|(rec, o, prepare)| {
                if !prepare {
                    return Ok((rec, o));
                }
                let id = rec.id.clone();
                let r = jetcarp_store::prepare(rec)?;
                if r.id != id {
                    return Err(format!(
                        "pattern {id}: its canonical form has id {} (the pack's record is not canonical)",
                        r.id
                    ));
                }
                Ok((r, o))
            })
            .collect();
        let mut ok = vec![];
        for r in ready {
            match r {
                Ok(x) => ok.push(x),
                Err(e) => return Err(e),
            }
        }
        let ids: Vec<String> = ok.iter().map(|x| x.0.id.clone()).collect();
        let res = lib.add_many_each(ok, 0, false)?;
        for (id, r) in ids.into_iter().zip(res) {
            match r? {
                jetcarp_store::AddOutcome::Added => {
                    out.patterns_added += 1;
                    kept.push((id, true));
                }
                jetcarp_store::AddOutcome::Merged { .. } => {
                    out.patterns_merged += 1;
                    kept.push((id, false));
                }
            }
        }
        Ok(())
    };
    let mut pattern_err: Option<String> = None;
    src.table_rows(&v, "patterns", |_, rows| {
        for r in rows {
            let p: PatternRow = de(r, "patterns")?;
            let text = if opts.strict {
                jetcarp_core::jcs::to_string_safe(&serde_json::to_value(&p).unwrap_or_default())
            } else {
                String::new()
            };
            if let Some(prev) = seen_ids.get(&p.id) {
                if opts.strict && *prev != crate::hash::hex(text.as_bytes()) {
                    return Err(format!("pattern {}: two different rows for one id", p.id));
                }
                continue;
            }
            seen_ids.insert(p.id.clone(), crate::hash::hex(text.as_bytes()));
            check_pattern_row(&p)?;
            if retracted.contains(format!("pattern:{}", p.id).as_str()) {
                continue;
            }
            let vs = verifs.get(&p.id).map(Vec::as_slice).unwrap_or(&[]);
            let main = vs
                .iter()
                .filter(|x| x.main)
                .max_by_key(|x| ((x.publisher == m.publisher), x.periods, x.date.clone()))
                .or_else(|| vs.iter().max_by_key(|x| (x.periods, x.date.clone())));
            let tg = format!("pattern:{}", p.id);
            let (origin, run) = origin_of(prov.get(&tg), &m.publisher, &h)?;
            let mut rec = match rows::record_of(&p, main, run) {
                Ok(r) => r,
                Err(e) => {
                    pattern_err.get_or_insert(e);
                    continue;
                }
            };
            apply_fields(&mut rec, fields.get(&tg));
            rec.created = now.clone();
            rec.updated = now.clone();
            let family = main
                .and_then(|x| jetcarp_core::semantics::of_notation(&x.rule))
                .map(|x| x.0)
                .unwrap_or("");
            let stale = stale_pins.contains(&p.pin_key)
                || stale_families.contains(family)
                || main.is_some_and(|x| {
                    jetcarp_core::semantics::version(family) != Some(x.semantics.as_str())
                });
            if stale {
                rec.needs_reverify = true;
                flagged_ids.push(rec.id.clone());
            }
            for x in vs {
                if Some(x) != main {
                    other_rules.push((
                        p.id.clone(),
                        x.rule.clone(),
                        rows::verification_of(x),
                        x.date.clone(),
                    ));
                }
            }
            batch.push((rec, origin, !stale));
            if batch.len() >= 4096 {
                flush(&mut batch, &mut out, &mut kept)?;
            }
        }
        Ok(())
    })?;
    flush(&mut batch, &mut out, &mut kept)?;
    if let Some(e) = pattern_err {
        return Err(e);
    }
    out.flagged = flagged_ids.len() as u64;
    let new_ids: Vec<String> = kept.iter().filter(|x| x.1).map(|x| x.0.clone()).collect();
    let stored: HashSet<String> = kept.iter().map(|x| x.0.clone()).collect();
    if !opts.dry_run && !other_rules.is_empty() {
        other_rules.retain(|x| stored.contains(&x.0));
        lib.add_rule_verifications(&other_rules)?;
    }

    // Search runs, before the sightings (which name searches by uuid): copied into the
    // library's searches folder, each a paused job with its own settings and its working
    // database rebuilt from its runs.
    for (uuid, sp) in &v.searches {
        if !opts.dry_run {
            log(&format!(
                "search {uuid}: copying its segments and rebuilding its state"
            ));
            if let Some(r) = import_search(lib, uuid, sp, &h)? {
                out.notes.push(format!(
                    "search {uuid}: {} runs, {} elites in {} niches, rebuilt in {} s",
                    r.runs, r.elites, r.niches, r.seconds
                ));
                out.rebuilt.push(r);
            }
        }
        out.searches.push(uuid.clone());
    }

    // Sightings: per pattern, rule and search, the larger of the publishers' totals.
    log(if opts.dry_run {
        "checking sightings, ants, Quick Start entries, media and ratings"
    } else {
        "merging sightings"
    });
    let mut totals: BTreeMap<
        (String, String, String),
        (String, BTreeMap<String, (u64, u64, String, String)>),
    > = BTreeMap::new();
    src.table_rows(&v, "sightings", |_, rows| {
        for r in rows {
            let s: SightingRow = de(r, "sightings")?;
            if s.count < 0 || s.soups < 0 {
                return Err(format!("sightings of {}: a negative count", s.pattern_id));
            }
            let e = totals
                .entry((s.pattern_id.clone(), s.rule.clone(), s.source.clone()))
                .or_insert((s.source_kind.clone().unwrap_or_default(), BTreeMap::new()));
            let p =
                e.1.entry(s.publisher.clone())
                    .or_insert((0, 0, String::new(), String::new()));
            p.0 += s.count as u64;
            p.1 += s.soups as u64;
            if p.2.is_empty() || s.first_seen < p.2 {
                p.2 = s.first_seen.clone();
            }
            if s.last_seen > p.3 {
                p.3 = s.last_seen.clone();
            }
        }
        Ok(())
    })?;
    let mut jobs: HashMap<String, i64> = HashMap::new();
    let mut merged = vec![];
    let mut extra: BTreeMap<String, u64> = BTreeMap::new();
    let new_set: HashSet<&String> = new_ids.iter().collect();
    for ((pid, rule, source), (kind, per)) in totals {
        if !stored.contains(&pid) {
            continue;
        }
        let best = per
            .values()
            .max_by_key(|x| x.0)
            .cloned()
            .unwrap_or_default();
        if source == "unattributed" {
            if new_set.contains(&pid) {
                *extra.entry(pid).or_default() += best.0;
            }
            continue;
        }
        if !read::safe_uuid(&source) {
            return Err(format!(
                "sightings of {pid}: source '{source}' is not a search uuid"
            ));
        }
        if opts.dry_run {
            continue;
        }
        let job = match jobs.get(&source) {
            Some(j) => *j,
            None => {
                let j = lib.job_for_uuid(&source, &kind, &h)?;
                jobs.insert(source.clone(), j);
                j
            }
        };
        merged.push(ImportSighting {
            pattern_id: pid,
            rule,
            job,
            count: best.0,
            soups: best.1,
            first_seen: best.2,
            last_seen: best.3,
        });
    }
    out.sightings = merged.len() as u64;
    if !opts.dry_run {
        let extra: Vec<(String, u64)> = extra.into_iter().collect();
        lib.merge_sightings(&merged, &extra)?;
    }

    // A pack names a Quick Start entry by its pack id (`qs-...`), the library by its own name:
    // an entry the library has already is that entry (its pack id computed as a publish would),
    // and a new one is kept under its pack id.
    let mut qs_lib: HashMap<String, String> = HashMap::new();
    for q in lib.pack_quickstart_with(&[])? {
        if let Ok((p, _)) = rows::quickstart_pack_id(&q) {
            qs_lib.entry(p).or_insert(q.row.id);
        }
    }
    let local_qs = |id: &str| qs_lib.get(id).cloned().unwrap_or_else(|| id.to_string());
    let local = |(k, id): (&str, &str)| -> (String, String) {
        let id = if k == "quickstart" {
            local_qs(id)
        } else {
            id.to_string()
        };
        (k.to_string(), id)
    };

    // Ant runs and Quick Start entries. An ant whose id does not hash refuses the pack, as a
    // pattern's does; one whose start, outcome or run cannot be read is left out on its own,
    // with a note naming it.
    let mut ant_rows = vec![];
    for (_, a) in rows_of::<AntPackRow>(&src, &v, "ants")? {
        let pin_ok = crate::hash::hex(format!("{}\n{}", a.identity, a.pin).as_bytes());
        if format!("ant-{}", &pin_ok[..32]) != a.id {
            return Err(format!("ant {}: its id does not match its identity", a.id));
        }
        if retracted.contains(format!("ant:{}", a.id).as_str()) {
            continue;
        }
        let mut row = match rows::ant_of(&a) {
            Ok(r) => r,
            Err(e) => {
                out.notes.push(format!("left out: {e}"));
                continue;
            }
        };
        let tg = format!("ant:{}", a.id);
        if let Some(f) = fields.get(&tg) {
            row.name = f.get("name").and_then(Value::as_str).map(String::from);
            row.notes = f
                .get("notes")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            row.rating = f.get("rating").and_then(Value::as_i64);
        }
        row.needs_reverify = stale_pins.contains(&a.pin_key)
            || jetcarp_core::semantics::version(&a.family) != Some(a.semantics.as_str());
        let (origin, _) = origin_of(prov.get(&tg), &m.publisher, &h)?;
        ant_rows.push((row, origin));
    }
    if !opts.dry_run && !ant_rows.is_empty() {
        out.ants = lib.import_ants(&ant_rows)?;
    }
    let mut qs = vec![];
    for (_, q) in rows_of::<QuickstartPackRow>(&src, &v, "quickstart")? {
        if retracted.contains(format!("quickstart:{}", q.id).as_str()) {
            continue;
        }
        if q.id != rows::quickstart_id(&q.scene, &q.pin) {
            return Err(format!(
                "Quick Start entry {}: its id is not qs- and the hash of its scene and pin",
                q.id
            ));
        }
        let mut row = rows::quickstart_of(&q)?;
        row.id = local_qs(&q.id);
        row.source_ref = h.clone();
        let tg = format!("quickstart:{}", q.id);
        let (origin, _) = origin_of(prov.get(&tg), &m.publisher, &h)?;
        qs.push((row, origin, q.pin.clone()));
    }
    if !opts.dry_run && !qs.is_empty() {
        out.quickstart = lib.import_quickstart(&qs)?;
    }

    // Aliases (none for a retracted record).
    let mut aliases = vec![];
    for (_, a) in rows_of::<AliasPackRow>(&src, &v, "aliases")? {
        if retracted.contains(a.target.as_str()) {
            continue;
        }
        let (k, id) = split_target(&a.target)?;
        aliases.push(AliasRow {
            alias: a.old_id.clone(),
            target_kind: k.into(),
            id: id.into(),
            reason: a.reason.clone(),
            created: a.date.clone(),
        });
    }
    if !opts.dry_run && !aliases.is_empty() {
        out.aliases = lib.import_aliases(&aliases)?;
    }

    // Media files.
    let mut media = vec![];
    for (_, mr) in rows_of::<MediaPackRow>(&src, &v, "media")? {
        let ext = crate::publish::media_ext(&mr.media_type)?;
        let rel = read::export_media_path(&mr.target, &mr.kind, mr.from_seed, ext)
            .ok_or_else(|| format!("media row for '{}'", mr.target))?;
        let b = src.object(&mr.object, ext, &rel, 256 << 20)?;
        out.objects_checked += 1;
        out.bytes_checked += b.len() as u64;
        // Nothing is added for a retracted record (its file is checked all the same).
        if retracted.contains(mr.target.as_str()) {
            continue;
        }
        let (tk, tid) = local(split_target(&mr.target)?);
        if !opts.dry_run {
            jetcarp_store::media::put_file(&lib.folder(), &b, ext)?;
        }
        media.push(PackMedia {
            target_kind: tk,
            target_id: tid,
            kind: mr.kind.clone(),
            from_seed: mr.from_seed,
            hash: crate::hash::hex_of(&mr.object)?.into(),
            ext: ext.into(),
            render: mr.render.clone(),
            width: mr.width as i64,
            height: mr.height as i64,
            frames: mr.frames as i64,
            bytes: mr.bytes,
            key: mr.render_key.clone(),
            created: now.clone(),
        });
    }
    if !opts.dry_run && !media.is_empty() {
        out.media = lib.import_media(&media)?;
    }

    // Ratings, under their raters' names when they chose to be named. A rater's rows are events
    // (consent recorded again adds one): the latest stands, by consent_date, then by the row's
    // RFC 8785 text.
    let mut raters: HashMap<String, (String, RaterRow)> = HashMap::new();
    for (_, r) in rows_of::<RaterRow>(&src, &v, "raters")? {
        let text = jetcarp_core::jcs::to_string_safe(&serde_json::to_value(&r).unwrap_or_default());
        let key = (r.consent_date.clone(), text);
        match raters.get(&r.rater) {
            Some((t, old)) if (old.consent_date.clone(), t.clone()) >= key => {}
            _ => {
                raters.insert(r.rater.clone(), (key.1, r));
            }
        }
    }
    let raters: HashMap<String, RaterRow> = raters.into_iter().map(|(k, v)| (k, v.1)).collect();
    let name_of = |id: &str| -> String {
        raters
            .get(id)
            .and_then(|r| r.display_name.clone())
            .unwrap_or_else(|| id.to_string())
    };
    let mut ratings = vec![];
    for (_, r) in rows_of::<RatingPackRow>(&src, &v, "ratings")? {
        let Some(info) = raters.get(&r.rater) else {
            return Err(format!("ratings by {} who has no raters row", r.rater));
        };
        if retracted.contains(r.target.as_str()) {
            continue;
        }
        let rater = match &info.members {
            Some(ms) if !ms.is_empty() => ms
                .iter()
                .map(|x| name_of(x))
                .collect::<Vec<_>>()
                .join(" & "),
            _ => name_of(&r.rater),
        };
        let (tk, tid) = local(split_target(&r.target)?);
        let (rating, verdict) = match r.scale.as_str() {
            "keep-maybe-drop 1" => (
                jetcarp_store::library::verdict_rating(&r.verdict)
                    .ok_or_else(|| format!("verdict '{}'", r.verdict))?,
                Some(r.verdict.clone()),
            ),
            "stars-1-5 1" => (
                r.verdict
                    .parse::<f64>()
                    .ok()
                    .filter(|x| (1.0..=5.0).contains(x))
                    .ok_or_else(|| format!("stars '{}'", r.verdict))?,
                None,
            ),
            other => {
                out.notes
                    .push(format!("ratings on scale '{other}' are not kept"));
                continue;
            }
        };
        ratings.push(jetcarp_store::RatingRow {
            id: 0,
            target_kind: tk,
            target_id: tid,
            rater,
            rating,
            verdict,
            // A note is kept only while the rater's latest consent shares notes.
            note: r.note.clone().filter(|_| info.notes).unwrap_or_default(),
            source: r.round.clone().unwrap_or_default(),
            created: r.date.clone(),
        });
    }
    if !opts.dry_run && !ratings.is_empty() {
        out.ratings = lib.import_ratings(&ratings)?.0;
    }

    // Retractions, as flags.
    let rr: Vec<RetractionRow> = m
        .retracts
        .iter()
        .filter_map(|r: &Retract| {
            let (k, id) = local(split_target(&r.target).ok()?);
            Some(RetractionRow {
                target_kind: k,
                target_id: id,
                publisher: if r.publisher.is_empty() {
                    m.publisher.clone()
                } else {
                    r.publisher.clone()
                },
                reason: r.reason.clone(),
                date: r.date.clone(),
                manifest: h.clone(),
            })
        })
        .collect();
    if !opts.dry_run && !rr.is_empty() {
        out.retracted = lib.record_retractions(&rr)?;
    }

    // The sample.
    let seed = opts.seed.unwrap_or_else(|| {
        u64::from_str_radix(&crate::hash::hex_of(&h).unwrap_or("0")[..16], 16).unwrap_or(0)
    });
    out.sample = verify_sample(
        lib,
        &new_ids,
        &flagged_ids,
        &opts.verify,
        seed,
        opts.verify_seconds,
        opts.dry_run,
    );
    if !opts.dry_run {
        lib.record_pack_import(&PackImportRow {
            manifest: h.clone(),
            feed: m.feed.clone(),
            publisher: m.publisher.clone(),
            signed: out.signed.clone(),
            source: opts
                .path
                .file_name()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_default(),
            imported: now.clone(),
            engine_version: jetcarp_core::VERSION.into(),
            report: json!({
                "publisher_name": out.publisher_name,
                "patterns_added": out.patterns_added,
                "patterns_merged": out.patterns_merged,
                "flagged": out.flagged,
                "ants": out.ants,
                "quickstart": out.quickstart,
                "sightings": out.sightings,
                "ratings": out.ratings,
                "media": out.media,
                "searches": out.searches,
                "retracted": out.retracted,
                "objects_checked": out.objects_checked,
                "sample": out.sample,
                "seconds": t0.elapsed().as_secs(),
            }),
        })?;
    }
    let _ = ref_patches;
    Ok(out)
}

/// A pattern row's own consistency: its id is its identity's hash on its pin, its typed phases
/// are its identity's, and it is within the format's limits.
fn check_pattern_row(p: &PatternRow) -> Result<(), String> {
    if jetcarp_core::pattern::range::content_id(&p.identity, &p.pin) != p.id {
        return Err(format!(
            "pattern {}: its id is not the hash of its identity and pin",
            p.id
        ));
    }
    if !p.identity.starts_with("jetcarp-pattern-v3\n") {
        return Err(format!(
            "pattern {}: identity scheme is not jetcarp-pattern-v3",
            p.id
        ));
    }
    if p.period < 1 || p.period > 1 << 32 {
        return Err(format!(
            "pattern {}: period {} beyond the limit of 2^32",
            p.id, p.period
        ));
    }
    if p.phases.iter().any(|c| c.len() > 1_000_000) {
        return Err(format!(
            "pattern {}: a phase with more than 1,000,000 cells",
            p.id
        ));
    }
    let typed: Vec<String> = p
        .phases
        .iter()
        .map(|c| rows::phase_text(c))
        .collect::<Result<_, _>>()?;
    let line = p
        .identity
        .lines()
        .find_map(|l| l.strip_prefix("phases="))
        .unwrap_or("");
    if typed.join(";") != line {
        return Err(format!(
            "pattern {}: its typed phases are not its identity's",
            p.id
        ));
    }
    for (k, want) in [
        ("canonical=", p.canonical.as_str()),
        ("class=", p.class.as_str()),
        ("nb=", p.neighbourhood.as_str()),
    ] {
        if p.identity.lines().find_map(|l| l.strip_prefix(k)) != Some(want) {
            return Err(format!(
                "pattern {}: its {} is not its identity's",
                p.id,
                &k[..k.len() - 1]
            ));
        }
    }
    Ok(())
}

/// A record's origin from a pack's provenance rows: the history of the publisher whose row 0 is
/// earliest (the manifest's publisher's on a tie), with row 0's `run` as the record's own
/// provenance.
fn origin_of(
    rows: Option<&Vec<ProvenanceRow>>,
    publisher: &str,
    manifest: &str,
) -> Result<(Origin, Option<Provenance>), String> {
    let source = format!("pack {manifest}");
    let Some(rows) = rows.filter(|r| !r.is_empty()) else {
        let mut o = Origin::imported(&source);
        o.pack_hash = manifest.to_string();
        return Ok((o, None));
    };
    let mut by: BTreeMap<&str, Vec<&ProvenanceRow>> = BTreeMap::new();
    for r in rows {
        by.entry(r.publisher.as_str()).or_default().push(r);
    }
    let (_, mut hist) = by
        .into_iter()
        .filter(|(_, v)| v.iter().any(|r| r.seq == 0))
        .min_by_key(|(p, v)| {
            let d0 = v
                .iter()
                .find(|r| r.seq == 0)
                .map(|r| r.date.clone())
                .unwrap_or_default();
            (d0, *p != publisher, p.to_string())
        })
        .ok_or("a record's provenance has no row 0")?;
    hist.sort_by_key(|r| r.seq);
    let lib = |r: &ProvenanceRow| LibProv {
        target_kind: String::new(),
        target_id: String::new(),
        seq: r.seq as i64,
        kind: r.kind.clone(),
        finder: r.finder.clone().unwrap_or_default(),
        orcid: r.orcid.clone().unwrap_or_default(),
        search_kind: r.search_kind.clone().unwrap_or_default(),
        job_uuid: r.search.clone().unwrap_or_default(),
        pack_hash: r.pack.clone().unwrap_or_default(),
        machine: r.machine.clone().unwrap_or_default(),
        engine_version: r.engine_version.clone(),
        geometry_pin: r.pin.clone(),
        source: r.source.clone().unwrap_or_default(),
        date: r.date.clone(),
    };
    let row0 = hist[0];
    if !matches!(row0.kind.as_str(), "found" | "imported") {
        return Err(format!(
            "{}: provenance row 0 is '{}'",
            row0.target, row0.kind
        ));
    }
    let run: Option<Provenance> = row0
        .run
        .as_deref()
        .and_then(|r| serde_json::from_str(r).ok())
        .or_else(|| {
            Some(Provenance {
                source: row0.source.clone().unwrap_or_default(),
                jetcarp_version: row0.engine_version.clone(),
                date: row0.date.clone(),
                ..Default::default()
            })
        });
    let o = Origin::found_elsewhere(
        row0.finder.as_deref().unwrap_or(""),
        row0.orcid.as_deref().unwrap_or(""),
        &source,
    )
    .with_history(
        LibHistory {
            row0: lib(row0),
            rows: hist[1..].iter().map(|r| lib(r)).collect(),
        },
        manifest,
    );
    Ok((o, run))
}

fn apply_fields(rec: &mut PatternRecord, f: Option<&BTreeMap<String, Value>>) {
    let Some(f) = f else { return };
    if let Some(Value::String(s)) = f.get("name") {
        rec.name = Some(s.clone());
    }
    if let Some(Value::Array(a)) = f.get("tags") {
        rec.tags = a
            .iter()
            .filter_map(|x| x.as_str().map(String::from))
            .collect();
    }
    if let Some(Value::String(s)) = f.get("notes") {
        rec.notes = s.clone();
    }
    if let Some(Value::String(s)) = f.get("state")
        && let Ok(st) = CurationState::parse(s)
    {
        rec.state = st;
    }
    if let Some(r) = f
        .get("rating")
        .and_then(Value::as_u64)
        .filter(|r| (1..=5).contains(r))
    {
        rec.rating = Some(r as u8);
    }
    if let Some(Value::String(s)) = f.get("discovered_by")
        && let Ok(d) = DiscoveredBy::parse(s)
    {
        rec.discovered_by = d;
    }
}

/// Copy a search's segments and sidecars into the library's `searches/<uuid>/` folder (each
/// checked against its hash on the way) and write its manifest; a job is made for it.
fn import_search(
    lib: &Library,
    uuid: &str,
    sp: &read::SearchParts,
    manifest: &str,
) -> Result<Option<jetcarp_search::jobs::explore::Rebuilt>, String> {
    use jetcarp_search::explore::runs::{Manifest, ScoreFile, ScoreSet, SegmentEntry};
    let dir = lib.folder().join("searches").join(uuid);
    let mf = dir.join("manifest.json");
    if mf.exists() {
        // The library has this search already (its own folder): leave it.
        return Ok(None);
    }
    let ix = &sp.index;
    let copy = |from: &PathBuf, obj: &str, rel: &str| -> Result<(String, u64), String> {
        let to = dir.join(rel);
        if let Some(d) = to.parent() {
            std::fs::create_dir_all(d).map_err(|e| format!("{}: {e}", d.display()))?;
        }
        let tmp = to.with_extension("parquet.tmp");
        std::fs::copy(from, &tmp).map_err(|e| format!("{}: {e}", tmp.display()))?;
        let (hex, n) = crate::hash::file_hex(&tmp, crate::store::MAX_OBJECT)?;
        if format!("sha256:{hex}") != obj {
            let _ = std::fs::remove_file(&tmp);
            return Err(format!("search object {obj} is altered"));
        }
        std::fs::rename(&tmp, &to).map_err(|e| format!("{}: {e}", to.display()))?;
        Ok((hex, n))
    };
    let mut segments = vec![];
    for (p, f) in ix.parts.iter().zip(&sp.run_files) {
        let rel = format!("runs/{}.parquet", p.segment);
        let (hex, n) = copy(f, &p.object, &rel)?;
        segments.push(SegmentEntry {
            file: rel,
            sha256: hex,
            bytes: n,
            rows: p.rows,
            first_id: p.first.parse().map_err(|_| "a segment's first id")?,
            last_id: p.last.parse().map_err(|_| "a segment's last id")?,
            sealed: p.sealed.clone(),
            engine_version: p.engine_version.clone(),
        });
    }
    let mut scores = vec![];
    for (s, fs) in ix.scores.iter().zip(&sp.score_files) {
        let mut files = vec![];
        for (p, f) in s.parts.iter().zip(fs) {
            let rel = format!("scores-{}/{}.parquet", s.version, p.segment);
            let (hex, n) = copy(f, &p.object, &rel)?;
            files.push(ScoreFile {
                segment: format!("runs/{}.parquet", p.segment),
                file: rel,
                sha256: hex,
                bytes: n,
                rows: p.rows,
            });
        }
        scores.push(ScoreSet {
            version: s.version as u32,
            created: s.created.clone(),
            engine_version: s.engine_version.clone(),
            files,
        });
    }
    let m = Manifest {
        format: "jetcarp-search-manifest 1".into(),
        search: uuid.into(),
        kind: ix.kind.clone(),
        engine_version: ix.engine_version.clone(),
        run_schema: ix.run_schema.clone(),
        score_schema: ix.score_schema.clone(),
        created: ix.created.clone(),
        updated: ix.updated.clone(),
        spec: ix.spec.clone(),
        segments,
        scores,
        remeasure: ix
            .remeasure
            .clone()
            .map(serde_json::from_value)
            .transpose()
            .map_err(|e| format!("search {uuid}: its remeasure record: {e}"))?,
    };
    let text = serde_json::to_string_pretty(&m).map_err(|e| e.to_string())?;
    crate::store::write_atomic(&mf, text.as_bytes())?;
    let explore = ix.kind == jetcarp_search::jobs::explore::KIND_NAME;
    let mut spec = ix.spec.clone();
    if explore {
        // The settings' numbers as numbers again (a pack's documents write decimals as
        // strings), every other member kept as it came (its label among them).
        let typed: jetcarp_search::jobs::explore::ExploreSpec =
            serde_json::from_value(spec.clone())
                .map_err(|e| format!("search {uuid}: its settings: {e}"))?;
        if let (Value::Object(m), Ok(Value::Object(t))) = (&mut spec, serde_json::to_value(&typed))
        {
            for (k, v) in t {
                m.insert(k, v);
            }
        }
    }
    let job = lib.job_for_uuid_with(uuid, &ix.kind, manifest, Some(&spec))?;
    if !explore {
        return Ok(None);
    }
    jetcarp_search::jobs::explore::rebuild(lib, job).map(Some)
}

/// Verify a sample of the new patterns again (the flagged ones are left to `library reverify`).
fn verify_sample(
    lib: &Library,
    new_ids: &[String],
    flagged: &[String],
    mode: &VerifyMode,
    seed: u64,
    budget: f64,
    dry_run: bool,
) -> Sample {
    let t0 = Instant::now();
    let flagged: HashSet<&String> = flagged.iter().collect();
    let pool: Vec<&String> = new_ids.iter().filter(|i| !flagged.contains(i)).collect();
    let mut s = Sample {
        seed: format!("{seed:016x}"),
        ..Default::default()
    };
    let n = match mode {
        VerifyMode::None => {
            s.mode = "none".into();
            return s;
        }
        VerifyMode::Full => {
            s.mode = "full".into();
            pool.len() as u64
        }
        VerifyMode::Sample(n) => {
            s.mode = "sample".into();
            (*n).min(pool.len() as u64)
        }
    };
    s.n = n;
    if dry_run || n == 0 {
        return s;
    }
    // A partial Fisher-Yates shuffle with SplitMix64: the same pack and seed, the same sample.
    let mut idx: Vec<usize> = (0..pool.len()).collect();
    let mut rng = jetcarp_core::rng::Rng::new(seed);
    for i in 0..n as usize {
        let j = i + rng.below((idx.len() - i) as u64) as usize;
        idx.swap(i, j);
    }
    let chosen: Vec<&String> = idx[..n as usize].iter().map(|&i| pool[i]).collect();
    let results: Vec<Result<bool, String>> = chosen
        .par_iter()
        .map(|id| {
            if t0.elapsed().as_secs_f64() > budget {
                return Ok(false);
            }
            let rec = lib.get(id)?;
            // One record's budget: a period of at most 4096 and 10,000 cells.
            if rec.period > 4096 || rec.max_cells > 10_000 {
                return Ok(false);
            }
            verify(&rec, default_periods(rec.period)).map(|_| true)
        })
        .collect();
    for (id, r) in chosen.iter().zip(results) {
        match r {
            Ok(true) => {
                s.checked += 1;
                s.passed += 1;
            }
            Ok(false) => s.skipped += 1,
            Err(e) => {
                s.checked += 1;
                s.failures.push(format!("{id}: {e}"));
            }
        }
    }
    s.seconds = (t0.elapsed().as_secs_f64() * 10.0).round() / 10.0;
    s
}
