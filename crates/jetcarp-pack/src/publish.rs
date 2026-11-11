//! Publishing a library into a store: one new version of a feed.
//!
//! **What goes in.** The selected patterns (every one, or a list of ids; records flagged
//! `needs_reverify` are left out), with their verifications, sightings, provenance and
//! curation; the ant runs and Quick Start entries selected; the tilings and rules they use
//! (with each aperiodic family's reference patch); aliases; media files (never frame lists);
//! the selected explore searches' sealed segments and score sidecars; ratings whose raters
//! consented ([`RaterBook`]). Library jobs (which hold local paths) never go in, and every
//! string that could hold a path is stripped of it.
//!
//! **A new version of a feed.** The store's ref for the feed names the previous version, which
//! becomes the first parent (with `merge`, other stores' versions are copied in and become
//! parents too). Each table keeps every part its parents list and adds parts holding only rows
//! the parents do not have: facts by key, events by the whole row; sightings are increments
//! (the total of a key is the sum of its rows), so a new version adds the difference.
//!
//! **Determinism.** The same library, selection, parents, key and `created` give byte-identical
//! objects, signatures and refs.

use crate::docs::{
    Creator, Engine, Index, KeyStatement, Manifest, Part, Ref, Related, Retract, ScorePart,
    ScoreVersion, SearchIndex, SearchPart, VERSION,
};
use crate::json;
use crate::parquet_io::{self, PART_BYTES};
use crate::read::{self, Source, Version};
use crate::rows::{self, jcs};
use crate::sign::{self, Signer};
use crate::store::Store;
use crate::tables::*;
use jetcarp_core::rules::Rule;
use jetcarp_store::Library;
use jetcarp_store::library::pack::PackPattern;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::path::{Path, PathBuf};

/// Who publishes: the signing key and, if the publisher chose to be named, a name, ORCID iD
/// and GitHub user.
#[derive(Clone, Debug)]
pub struct Publisher {
    pub signer: Signer,
    pub name: String,
    pub orcid: String,
    pub github: String,
}

/// What a publish takes from the library.
#[derive(Clone, Debug)]
pub struct Selection {
    /// Patterns, ant runs and Quick Start entries by id (`None`: every one); a Quick Start entry
    /// by its library id or its pack id (`qs-...`).
    pub ids: Option<HashSet<String>>,
    /// Explore searches by uuid (`None`: every one with a sealed folder).
    pub searches: Option<Vec<String>>,
    pub ants: bool,
    pub quickstart: bool,
    pub media: bool,
    pub ratings: bool,
}

impl Default for Selection {
    fn default() -> Selection {
        Selection {
            ids: None,
            searches: None,
            ants: true,
            quickstart: true,
            media: true,
            ratings: true,
        }
    }
}

/// A publish.
#[derive(Clone, Debug)]
pub struct Options {
    pub feed: String,
    pub to: PathBuf,
    pub title: String,
    pub description: String,
    pub version_label: String,
    pub licence: String,
    pub doi: String,
    pub related: Vec<Related>,
    /// The version's time (default: now); fixed for reproducible packs.
    pub created: Option<String>,
    pub selection: Selection,
    /// Records to withdraw: `pattern:<id>`, `ant:<id>`, `quickstart:<id>`, with a reason (a
    /// Quick Start entry by either id; the manifest names its pack id).
    pub retract: Vec<(String, String)>,
    /// Other versions to merge (a store, or a store and manifest hash): each becomes a parent.
    pub merge: Vec<(PathBuf, Option<String>)>,
    /// Write `views.sql` for DuckDB.
    pub views: bool,
    /// Close parts at this many bytes (tests).
    pub part_bytes: u64,
}

impl Default for Options {
    fn default() -> Options {
        Options {
            feed: String::new(),
            to: PathBuf::new(),
            title: String::new(),
            description: String::new(),
            version_label: String::new(),
            licence: "CC0-1.0".into(),
            doi: String::new(),
            related: vec![],
            created: None,
            selection: Selection::default(),
            retract: vec![],
            merge: vec![],
            views: false,
            part_bytes: PART_BYTES,
        }
    }
}

/// What a publish did.
#[derive(Clone, Debug, Default, Serialize)]
pub struct Published {
    pub store: String,
    pub feed: String,
    pub manifest: String,
    pub seq: u64,
    pub parents: Vec<String>,
    /// Per table: rows added in this version, and parts it lists in all.
    pub tables: BTreeMap<String, (u64, usize)>,
    pub searches: Vec<String>,
    pub media: u64,
    pub retracts: usize,
    /// Bytes of every object the version names.
    pub bytes: u64,
    /// Records left out, and why (flagged, not canonical, a scene not self-contained...).
    pub notes: Vec<String>,
}

// ---------------------------------------------------------------------------------------------
// Raters and their consent

/// One rater as the publisher's settings keep them (`<config>/pack/raters.json`).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct RaterEntry {
    /// The name ratings use in the library (`Jimmie Rodgers`; a group is its members joined by
    /// ` & `).
    pub name: String,
    /// The pseudonymous id packs use: random, made once, kept for good.
    pub id: String,
    /// When the rater consented to permanent public release (empty: no consent).
    #[serde(default)]
    pub consent_date: String,
    /// Packs show the name (else only the id).
    #[serde(default)]
    pub show_name: bool,
    #[serde(default)]
    pub orcid: String,
    /// Notes may be published too.
    #[serde(default)]
    pub notes: bool,
    /// A group's members (names); a group's ratings go out only when every member consented.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub members: Vec<String>,
}

/// The raters a publisher knows, with their consent.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct RaterBook {
    pub raters: Vec<RaterEntry>,
}

/// A new pseudonymous rater id: `rater-` and 16 random hex digits.
pub fn new_rater_id() -> String {
    let s = Signer::generate("rater")
        .map(|s| s.fingerprint().to_string())
        .unwrap_or_default();
    format!("rater-{}", &crate::hash::hex(s.as_bytes())[..16])
}

impl RaterBook {
    pub fn path() -> Result<PathBuf, String> {
        Ok(sign::key_dir()?.join("raters.json"))
    }

    pub fn load(path: &Path) -> Result<RaterBook, String> {
        if !path.exists() {
            return Ok(RaterBook::default());
        }
        let b = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
        serde_json::from_slice(&b).map_err(|e| format!("{}: {e}", path.display()))
    }

    pub fn save(&self, path: &Path) -> Result<(), String> {
        let text = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        crate::store::write_atomic(path, format!("{text}\n").as_bytes())
    }

    pub fn get(&self, name: &str) -> Option<&RaterEntry> {
        self.raters.iter().find(|r| r.name == name)
    }

    /// Record a rater's consent (making their id if they have none).
    pub fn consent(&mut self, name: &str, date: &str, show_name: bool, orcid: &str, notes: bool) {
        match self.raters.iter_mut().find(|r| r.name == name) {
            Some(r) => {
                r.consent_date = date.to_string();
                r.show_name = show_name;
                r.orcid = orcid.to_string();
                r.notes = notes;
            }
            None => self.raters.push(RaterEntry {
                name: name.to_string(),
                id: new_rater_id(),
                consent_date: date.to_string(),
                show_name,
                orcid: orcid.to_string(),
                notes,
                members: vec![],
            }),
        }
    }

    /// The members of a rater name (` & ` joins a group's).
    pub fn members_of(name: &str) -> Vec<String> {
        name.split(" & ").map(|s| s.trim().to_string()).collect()
    }

    /// The rater a library rating's rater stands for, if every person in it consented: its
    /// entry (a group's is made here, with a new id, the first time).
    fn resolve(&mut self, name: &str, publisher: &str) -> Option<RaterEntry> {
        let name = if name == "user" { publisher } else { name };
        if name.is_empty() || name.starts_with("model:") {
            return None;
        }
        let members = RaterBook::members_of(name);
        if members.len() == 1 {
            return self
                .get(name)
                .filter(|r| consent_time(&r.consent_date).is_some())
                .cloned();
        }
        let ok: Vec<&RaterEntry> = members.iter().filter_map(|m| self.get(m)).collect();
        if ok.len() != members.len() || ok.iter().any(|r| consent_time(&r.consent_date).is_none()) {
            return None;
        }
        let latest = ok
            .iter()
            .filter_map(|r| consent_time(&r.consent_date))
            .max()
            .unwrap_or_default();
        let notes = ok.iter().all(|r| r.notes);
        if let Some(g) = self.raters.iter_mut().find(|r| r.name == name) {
            g.consent_date = latest;
            g.notes = notes;
            g.members = members;
            return Some(g.clone());
        }
        let g = RaterEntry {
            name: name.to_string(),
            id: new_rater_id(),
            consent_date: latest,
            show_name: false,
            orcid: String::new(),
            notes,
            members,
        };
        self.raters.push(g.clone());
        Some(g)
    }
}

/// A rater's consent date as a pack writes it: an RFC 3339 UTC time with whole seconds, as
/// every time in a pack is (a book that holds a bare date, `2026-11-01`, gives its midnight,
/// `2026-11-01T00:00:00Z`). None when the rater has not consented, or the date is not a time.
pub fn consent_time(d: &str) -> Option<String> {
    if crate::docs::is_time(d) {
        return Some(d.to_string());
    }
    let t = format!("{d}T00:00:00Z");
    crate::docs::is_time(&t).then_some(t)
}

// ---------------------------------------------------------------------------------------------
// The publisher's key statement

/// The publisher's key statement, signed.
pub fn key_statement(p: &Publisher, created: &str) -> Result<(KeyStatement, Vec<u8>), String> {
    let mut k = KeyStatement {
        format: "jetcarp-key".into(),
        version: VERSION.into(),
        key: p.signer.public_openssh(),
        fingerprint: p.signer.fingerprint().into(),
        name: p.name.clone(),
        orcid: p.orcid.clone(),
        github: p.github.clone(),
        created: created.into(),
        supersedes: String::new(),
        revoked: false,
        authorised: vec![],
        signature: String::new(),
    };
    let unsigned = unsigned_value(&k)?;
    k.signature = p.signer.sign(sign::NS_KEY, &json::canonical(&unsigned)?)?;
    let bytes = json::canonical_of(&k)?;
    Ok((k, bytes))
}

fn unsigned_value<T: Serialize>(t: &T) -> Result<Value, String> {
    let mut v = serde_json::to_value(t).map_err(|e| e.to_string())?;
    if let Value::Object(m) = &mut v {
        m.remove("signature");
    }
    Ok(v)
}

// ---------------------------------------------------------------------------------------------
// Rows the parents have

/// What a version's parents already hold.
#[derive(Default)]
struct Inherited {
    /// Per table: the parts, in parent order, each once.
    parts: BTreeMap<String, Vec<Part>>,
    /// Fact keys (the first sort column's values) per fact table.
    keys: HashMap<String, HashSet<String>>,
    /// Every row's canonical text per event table, rows of this publisher only.
    rows: HashMap<String, HashSet<String>>,
    /// Sightings totals of this publisher: (pattern, rule, source) -> (count, soups, first, last).
    sightings: HashMap<(String, String, String), (i64, i64, String, String)>,
    /// This publisher's newest annotation values: (target, field) -> value.
    annotations: HashMap<(String, String), String>,
    retracts: Vec<Retract>,
    retain: Vec<String>,
}

fn inherit(src: &Source, parents: &[Version], me: &str) -> Result<Inherited, String> {
    let mut inh = Inherited::default();
    for v in parents {
        for (name, tp) in &v.tables {
            let list = inh.parts.entry(name.clone()).or_default();
            for (p, _) in &tp.parts {
                if !list.iter().any(|x| x.object == p.object) {
                    list.push(p.clone());
                }
            }
        }
        for r in &v.manifest.retracts {
            if !inh.retracts.contains(r) {
                inh.retracts.push(r.clone());
            }
        }
        for r in &v.manifest.retain {
            if !inh.retain.contains(r) {
                inh.retain.push(r.clone());
            }
        }
    }
    if parents.is_empty() {
        return Ok(inh);
    }
    // The rows: read through a version made of the union (the parents' tables merged).
    let mut annotations: HashMap<(String, String), (String, Value)> = HashMap::new();
    for (name, parts) in &inh.parts {
        let Some(def) = table(name) else { continue };
        for p in parts {
            let path = match src {
                Source::Store(s) => s.object_path(&p.object, "parquet")?,
                Source::Export(_) => return Err("merge from a store, not an export".into()),
            };
            let b = src.part_bytes(p, &path)?;
            parquet_io::read_rows(b, &def.cols, |rows| {
                for r in rows {
                    if def.facts {
                        inh.keys
                            .entry(name.clone())
                            .or_default()
                            .insert(parquet_io::first_key(def, &r));
                        continue;
                    }
                    let mine = r.get("publisher").and_then(Value::as_str) == Some(me)
                        || !def.cols.iter().any(|c| c.name == "publisher");
                    if !mine {
                        continue;
                    }
                    match name.as_str() {
                        "sightings" => {
                            let k = (
                                r["pattern_id"].as_str().unwrap_or("").to_string(),
                                r["rule"].as_str().unwrap_or("").to_string(),
                                r["source"].as_str().unwrap_or("").to_string(),
                            );
                            let e = inh.sightings.entry(k).or_insert((
                                0,
                                0,
                                String::new(),
                                String::new(),
                            ));
                            e.0 += r["count"].as_i64().unwrap_or(0);
                            e.1 += r["soups"].as_i64().unwrap_or(0);
                            let f = r["first_seen"].as_str().unwrap_or("").to_string();
                            if e.2.is_empty() || f < e.2 {
                                e.2 = f;
                            }
                            let l = r["last_seen"].as_str().unwrap_or("").to_string();
                            if l > e.3 {
                                e.3 = l;
                            }
                        }
                        "annotations" => {
                            let k = (
                                r["target"].as_str().unwrap_or("").to_string(),
                                r["field"].as_str().unwrap_or("").to_string(),
                            );
                            let newer = match annotations.get(&k) {
                                None => true,
                                Some((_, ov)) => read::later_annotation(ov, &r),
                            };
                            if newer {
                                annotations.insert(k, (p.object.clone(), r.clone()));
                            }
                        }
                        _ => {
                            inh.rows
                                .entry(name.clone())
                                .or_default()
                                .insert(jetcarp_core::jcs::to_string(&r));
                        }
                    }
                }
                Ok(())
            })?;
        }
    }
    inh.annotations = annotations
        .into_iter()
        .map(|(k, (_, v))| (k, v["value"].as_str().unwrap_or("").to_string()))
        .collect();
    Ok(inh)
}

// ---------------------------------------------------------------------------------------------
// The publish

fn now_or(created: &Option<String>) -> String {
    created
        .clone()
        .unwrap_or_else(jetcarp_core::pattern::now_utc)
}

fn target(kind: &str, id: &str) -> String {
    format!("{kind}:{id}")
}

fn opt(s: &str) -> Option<String> {
    (!s.is_empty()).then(|| s.to_string())
}

/// Collects one table's new rows (not in the parents).
struct NewRows {
    name: &'static str,
    /// Rows as JSON text (parsed only when written).
    rows: Vec<String>,
}

impl NewRows {
    fn new(name: &'static str) -> NewRows {
        NewRows { name, rows: vec![] }
    }
    fn push<T: Serialize>(&mut self, inh: &Inherited, row: &T) -> Result<(), String> {
        let def = table(self.name).unwrap();
        let facts = def.facts && inh.keys.contains_key(self.name);
        let events = !def.facts && inh.rows.contains_key(self.name);
        if facts || events {
            let v = serde_json::to_value(row).map_err(|e| e.to_string())?;
            if facts && inh.keys[self.name].contains(&parquet_io::first_key(def, &v)) {
                return Ok(());
            }
            if events && inh.rows[self.name].contains(&jetcarp_core::jcs::to_string(&v)) {
                return Ok(());
            }
        }
        self.rows
            .push(serde_json::to_string(row).map_err(|e| e.to_string())?);
        Ok(())
    }
}

/// Publish a version of a feed from a library into the store at `opts.to`.
pub fn publish(
    lib: &Library,
    publisher: &Publisher,
    book: &mut RaterBook,
    opts: &Options,
    log: &dyn Fn(&str),
) -> Result<Published, String> {
    if !crate::docs::valid_feed(&opts.feed) {
        return Err(format!(
            "feed name '{}': letters, digits and - only",
            opts.feed
        ));
    }
    let created = now_or(&opts.created);
    if !crate::docs::is_time(&created) {
        return Err(format!(
            "created '{created}': an RFC 3339 UTC time with whole seconds (2026-11-11T09:02:38Z)"
        ));
    }
    let store = Store::create(&opts.to)?;
    let src = Source::Store(store.clone());
    let me = publisher.signer.fingerprint().to_string();
    let mut out = Published {
        store: opts.to.display().to_string(),
        feed: opts.feed.clone(),
        ..Default::default()
    };

    // The previous version of this feed, and versions to merge.
    let mut parents: Vec<Version> = vec![];
    let mut seq = 1;
    if store.ref_path(&opts.feed)?.exists() {
        let r = src.read_ref(&opts.feed)?;
        if r.publisher != me {
            return Err(format!(
                "feed '{}' in this store is published by {}, not by this key ({me})",
                opts.feed, r.publisher
            ));
        }
        seq = r.seq + 1;
        parents.push(src.version(&r.manifest)?);
    }
    for (path, which) in &opts.merge {
        let other = Source::open(path)?;
        let h = match which {
            Some(h) => h.clone(),
            None => other.default_manifest(None)?,
        };
        let v = other.version(&h)?;
        copy_version(&other, &v, &store)?;
        if !parents.iter().any(|p| p.hash == v.hash) {
            parents.push(src.version(&v.hash)?);
        }
    }
    log(&format!(
        "publishing feed {} (version {seq}) into {}",
        opts.feed,
        opts.to.display()
    ));
    let inh = inherit(&src, &parents, &me)?;
    let rev = seq as i64;
    out.parents = parents.iter().map(|p| p.hash.clone()).collect();

    let sel = &opts.selection;
    let ids = sel.ids.as_ref();
    let mut t: BTreeMap<&'static str, NewRows> = [
        "tilings",
        "rules",
        "patterns",
        "verifications",
        "sightings",
        "provenance",
        "annotations",
        "ants",
        "quickstart",
        "media",
        "aliases",
        "ratings",
        "raters",
    ]
    .into_iter()
    .map(|n| (n, NewRows::new(n)))
    .collect();
    let mut pin_keys: BTreeSet<String> = BTreeSet::new();
    let mut rules_used: BTreeSet<(String, String, u32)> = BTreeSet::new();
    let mut published: HashSet<String> = HashSet::new();
    let mut flagged = 0u64;
    let mut not_canonical = vec![];
    let retracted: HashSet<String> = inh
        .retracts
        .iter()
        .map(|r| r.target.clone())
        .chain(opts.retract.iter().map(|r| r.0.clone()))
        .collect();

    // Patterns, streamed in id order: their parts are written as they come.
    let mut pw = parquet_io::TableWriter::new(&store, table("patterns").unwrap())
        .with_part_bytes(opts.part_bytes);
    let mut pattern_rows = 0u64;
    let mut sightings_sum: HashMap<
        (String, String, String),
        (i64, i64, String, String, Option<String>),
    > = HashMap::new();
    let n = lib.pack_patterns(ids, |pp: PackPattern| {
        let rec = &pp.rec;
        if rec.needs_reverify {
            flagged += 1;
            return Ok(());
        }
        if retracted.contains(&target("pattern", &rec.id)) {
            return Ok(());
        }
        let row = match rows::pattern_row(rec) {
            Ok(r) => r,
            Err(e) => {
                not_canonical.push(e);
                return Ok(());
            }
        };
        published.insert(rec.id.clone());
        pin_keys.insert(row.pin_key.clone());
        let key = parquet_io::first_key(table("patterns").unwrap(), &json!({"id": rec.id}));
        if !inh.keys.get("patterns").is_some_and(|k| k.contains(&key)) {
            let part: BTreeMap<String, String> = [
                ("class_group".to_string(), row.class_group.clone()),
                ("tiling_group".to_string(), row.tiling_group.clone()),
            ]
            .into_iter()
            .collect();
            if !part.values().all(|v| valid_partition_value(v)) {
                return Err(format!("pattern {}: partition {part:?}", rec.id));
            }
            pw.push_text(
                serde_json::to_string(&row).map_err(|e| e.to_string())?,
                part,
            )?;
            pattern_rows += 1;
        }
        let main_rule = Rule::parse(&rec.rule)
            .map(|r| r.0.notation())
            .unwrap_or_default();
        let nb = row.neighbourhood.clone();
        for v in &pp.rules {
            rules_used.insert((v.notation.clone(), nb.clone(), rec.steps_per_gen));
            let main = v.notation == main_rule;
            let ver = if main { &rec.verified } else { &v.verified };
            t.get_mut("verifications").unwrap().push(
                &inh,
                &VerificationRow {
                    pattern_id: rec.id.clone(),
                    rule: v.notation.clone(),
                    publisher: me.clone(),
                    main,
                    periods: ver.periods as i64,
                    generations: ver.generations as i64,
                    field: ver.field.clone(),
                    sites: (ver.sites > 0).then_some(ver.sites as i32),
                    engine_version: if ver.jetcarp_version.is_empty() {
                        v.engine_version.clone()
                    } else {
                        ver.jetcarp_version.clone()
                    },
                    pin: rec.geometry_pin.clone(),
                    semantics: if rec.semantics.is_empty() {
                        jetcarp_core::semantics::FIRST.into()
                    } else {
                        rec.semantics.clone()
                    },
                    date: if ver.date.is_empty() {
                        v.first_seen.clone()
                    } else {
                        ver.date.clone()
                    },
                    range: if main {
                        rec.range.as_ref().map(jcs)
                    } else {
                        None
                    },
                    mover: if main {
                        rec.mover.as_ref().map(jcs)
                    } else {
                        None
                    },
                },
            )?;
        }
        if !pp.rules.iter().any(|v| v.notation == main_rule) {
            rules_used.insert((main_rule.clone(), nb.clone(), rec.steps_per_gen));
            t.get_mut("verifications").unwrap().push(
                &inh,
                &VerificationRow {
                    pattern_id: rec.id.clone(),
                    rule: main_rule.clone(),
                    publisher: me.clone(),
                    main: true,
                    periods: rec.verified.periods as i64,
                    generations: rec.verified.generations as i64,
                    field: rec.verified.field.clone(),
                    sites: (rec.verified.sites > 0).then_some(rec.verified.sites as i32),
                    engine_version: rec.verified.jetcarp_version.clone(),
                    pin: rec.geometry_pin.clone(),
                    semantics: rec.semantics.clone(),
                    date: rec.verified.date.clone(),
                    range: rec.range.as_ref().map(jcs),
                    mover: rec.mover.as_ref().map(jcs),
                },
            )?;
        }
        // Sightings: the library's totals per job; with the copies counted without a job.
        let mut total = 0u64;
        for s in &pp.sightings {
            rules_used.insert((s.rule.clone(), nb.clone(), rec.steps_per_gen));
            total += s.count;
            sightings_sum.insert(
                (rec.id.clone(), s.rule.clone(), s.job_uuid.clone()),
                (
                    s.count as i64,
                    s.soups as i64,
                    s.first_seen.clone(),
                    s.last_seen.clone(),
                    opt(&s.job_kind),
                ),
            );
        }
        if rec.seen > total {
            sightings_sum.insert(
                (rec.id.clone(), main_rule.clone(), "unattributed".into()),
                (
                    (rec.seen - total) as i64,
                    0,
                    rec.created.clone(),
                    rec.updated.clone(),
                    None,
                ),
            );
        }
        for p in &pp.provenance {
            t.get_mut("provenance").unwrap().push(
                &inh,
                &provenance_row(
                    "pattern",
                    &rec.id,
                    p,
                    &me,
                    (p.seq == 0).then_some(&rec.provenance),
                ),
            )?;
        }
        let mut ann = vec![];
        if let Some(nm) = &rec.name {
            ann.push(("name", jcs(nm)));
        }
        if !rec.tags.is_empty() {
            ann.push(("tags", jcs(&rec.tags)));
        }
        if !rec.notes.is_empty() {
            ann.push(("notes", jcs(&crate::paths::strip(&rec.notes))));
        }
        if rec.state != jetcarp_core::pattern::CurationState::New {
            ann.push(("state", jcs(&rec.state.name())));
        }
        if let Some(r) = rec.rating {
            ann.push(("rating", r.to_string()));
        }
        if rec.discovered_by != jetcarp_core::pattern::DiscoveredBy::Search {
            ann.push(("discovered_by", jcs(&rec.discovered_by)));
        }
        let tg = target("pattern", &rec.id);
        for (field, value) in ann {
            annotate(&mut t, &inh, &tg, &me, field, value, &rec.updated, rev)?;
        }
        Ok(())
    })?;
    let pattern_parts = pw.finish()?;
    log(&format!("{n} patterns read, {pattern_rows} new rows"));
    if flagged > 0 {
        out.notes.push(format!(
            "{flagged} patterns flagged needs_reverify were left out (run `jetcarp library reverify` first)"
        ));
    }
    for e in not_canonical.iter().take(5) {
        out.notes.push(format!("left out: {e}"));
    }
    if not_canonical.len() > 5 {
        out.notes
            .push(format!("... {} more left out", not_canonical.len() - 5));
    }

    // Sightings: what grew since the parents.
    let mut keys: Vec<_> = sightings_sum.into_iter().collect();
    keys.sort_by(|a, b| a.0.cmp(&b.0));
    for ((pid, rule, source), (count, soups, first, last, kind)) in keys {
        let (c0, s0, f0, l0) = inh
            .sightings
            .get(&(pid.clone(), rule.clone(), source.clone()))
            .cloned()
            .unwrap_or_default();
        let dc = count - c0;
        if dc <= 0 {
            continue;
        }
        t.get_mut("sightings").unwrap().push(
            &inh,
            &SightingRow {
                pattern_id: pid,
                rule,
                publisher: me.clone(),
                source,
                source_kind: kind,
                count: dc,
                soups: (soups - s0).max(0),
                first_seen: if f0.is_empty() { first } else { f0.min(first) },
                last_seen: if l0.is_empty() {
                    last.clone()
                } else {
                    l0.max(last)
                },
            },
        )?;
    }

    // Ant runs.
    if sel.ants {
        for a in lib.pack_ants()? {
            if ids.is_some_and(|s| !s.contains(&a.row.id))
                || a.row.needs_reverify
                || retracted.contains(&target("ant", &a.row.id))
            {
                continue;
            }
            match rows::ant_row(&a.row, &a.pin_key) {
                Ok(r) => {
                    pin_keys.insert(a.pin_key.clone());
                    published.insert(a.row.id.clone());
                    t.get_mut("ants").unwrap().push(&inh, &r)?;
                    for p in &a.provenance {
                        t.get_mut("provenance")
                            .unwrap()
                            .push(&inh, &provenance_row("ant", &a.row.id, p, &me, None))?;
                    }
                    let tg = target("ant", &a.row.id);
                    if let Some(nm) = &a.row.name {
                        annotate(&mut t, &inh, &tg, &me, "name", jcs(nm), &a.row.updated, rev)?;
                    }
                    if !a.row.notes.is_empty() {
                        annotate(
                            &mut t,
                            &inh,
                            &tg,
                            &me,
                            "notes",
                            jcs(&crate::paths::strip(&a.row.notes)),
                            &a.row.updated,
                            rev,
                        )?;
                    }
                    if let Some(r) = a.row.rating {
                        annotate(
                            &mut t,
                            &inh,
                            &tg,
                            &me,
                            "rating",
                            r.to_string(),
                            &a.row.updated,
                            rev,
                        )?;
                    }
                }
                Err(e) => out.notes.push(format!("left out: {e}")),
            }
        }
    }

    // Quick Start entries, under their pack ids (`qs-` and the hash of the scene and pin): the
    // library keys them by name, and every row about one names `quickstart:<pack id>`. A
    // selection or a retraction may name either.
    let mut qs_pack_ids: HashMap<String, String> = HashMap::new();
    let mut qs_published: HashMap<String, String> = HashMap::new();
    for q in lib.pack_quickstart()? {
        let pack_id = rows::quickstart_pack_id(&q).ok().map(|x| x.0);
        if let Some(p) = &pack_id {
            qs_pack_ids.insert(q.row.id.clone(), p.clone());
        }
        let mut names = std::iter::once(&q.row.id).chain(&pack_id);
        if !sel.quickstart
            || ids.is_some_and(|s| !names.clone().any(|i| s.contains(i)))
            || names.any(|i| retracted.contains(&target("quickstart", i)))
        {
            continue;
        }
        match rows::quickstart_row(&q) {
            Ok(r) => {
                pin_keys.insert(r.pin_key.clone());
                qs_published.insert(q.row.id.clone(), r.id.clone());
                t.get_mut("quickstart").unwrap().push(&inh, &r)?;
                for p in &q.provenance {
                    t.get_mut("provenance")
                        .unwrap()
                        .push(&inh, &provenance_row("quickstart", &r.id, p, &me, None))?;
                }
            }
            Err(e) => out.notes.push(format!("left out: {e}")),
        }
    }
    // The target a row about a published record names (none for a record left out).
    let pack_target = |kind: &str, id: &str| -> Option<String> {
        match kind {
            "quickstart" => qs_published.get(id).map(|p| target(kind, p)),
            _ => published.contains(id).then(|| target(kind, id)),
        }
    };

    // Tilings (each aperiodic family with its reference patch) and rules.
    for k in &pin_keys {
        let (mut row, patch) = rows::tiling_row(k)?;
        if let Some(tiles) = patch {
            let rows: Vec<Value> = tiles
                .iter()
                .map(|t| serde_json::to_value(t).unwrap())
                .collect();
            let parts = parquet_io::write_table(&store, reference_patch(), rows, u64::MAX)?;
            row.reference = parts.first().map(|p| p.object.clone());
        }
        t.get_mut("tilings").unwrap().push(&inh, &row)?;
    }
    for (notation, nb, steps) in &rules_used {
        match rows::rule_row(notation, nb, *steps) {
            Ok(r) => t.get_mut("rules").unwrap().push(&inh, &r)?,
            Err(e) => out.notes.push(format!("rule {notation}: {e}")),
        }
    }

    // Aliases of published records.
    for kind in ["pattern", "ant"] {
        for a in lib.aliases(kind, "")? {
            if published.contains(&a.id) {
                t.get_mut("aliases").unwrap().push(
                    &inh,
                    &AliasPackRow {
                        old_id: a.alias.clone(),
                        target: target(kind, &a.id),
                        reason: a.reason.clone(),
                        date: a.created.clone(),
                    },
                )?;
            }
        }
    }

    // Searches.
    let mut searches: BTreeMap<String, String> = parents
        .iter()
        .flat_map(|p| p.manifest.searches.clone())
        .collect();
    let mut search_uuids: HashSet<String> = HashSet::new();
    for job in lib.jobs()? {
        if job.kind != "explore" {
            continue;
        }
        let uuid = job
            .spec
            .get("uuid")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        if !read::safe_uuid(&uuid) {
            continue;
        }
        if let Some(list) = &sel.searches
            && !list.contains(&uuid)
        {
            continue;
        }
        let dir = lib.folder().join("searches").join(&uuid);
        if !dir.join("manifest.json").is_file() {
            continue;
        }
        log(&format!("search {uuid}: copying its segments"));
        let (h, notes) = search_index(&store, &dir, &uuid)?;
        out.notes.extend(notes);
        searches.insert(uuid.clone(), h);
        search_uuids.insert(uuid.clone());
        out.searches.push(uuid);
    }

    // Media files.
    let mut media_n = 0u64;
    if sel.media {
        for m in lib.pack_media()? {
            let Some(tg) = pack_target(&m.target_kind, &m.target_id) else {
                continue;
            };
            let src_file = lib
                .folder()
                .join(jetcarp_store::media::rel_path(&m.hash, &m.ext));
            let media_type = match m.ext.as_str() {
                "png" => "image/png",
                "gif" => "image/gif",
                "webp" => "image/webp",
                _ => continue,
            };
            let (obj, bytes) = match store.put_file(&src_file, &m.ext, Some(&m.hash)) {
                Ok(x) => x,
                Err(e) => {
                    out.notes.push(format!("media of {}: {e}", m.target_id));
                    continue;
                }
            };
            let render: Value = serde_json::from_str(&m.render).unwrap_or(Value::Null);
            t.get_mut("media").unwrap().push(
                &inh,
                &MediaPackRow {
                    target: tg,
                    kind: m.kind.clone(),
                    from_seed: m.from_seed,
                    object: obj,
                    media_type: media_type.into(),
                    width: m.width as i32,
                    height: m.height as i32,
                    frames: m.frames as i32,
                    bytes: bytes as i64,
                    render_key: m.key.clone(),
                    render: jetcarp_core::jcs::to_string(&crate::paths::strip_json(render)),
                },
            )?;
            media_n += 1;
        }
    }
    out.media = media_n;

    // Ratings with consent.
    if sel.ratings {
        if !publisher.name.is_empty() && book.get(&publisher.name).is_none() {
            book.consent(&publisher.name, &created, true, &publisher.orcid, true);
        }
        let mut used: BTreeMap<String, RaterEntry> = BTreeMap::new();
        let mut left_out: BTreeMap<String, u64> = BTreeMap::new();
        for r in lib.ratings(None)? {
            let tg = match r.target_kind.as_str() {
                "search_run" => r
                    .target_id
                    .split_once(':')
                    .is_some_and(|(u, _)| search_uuids.contains(u) || sel.searches.is_none())
                    .then(|| target(&r.target_kind, &r.target_id)),
                k => pack_target(k, &r.target_id),
            };
            let Some(tg) = tg else {
                continue;
            };
            let Some(entry) = book.resolve(&r.rater, &publisher.name) else {
                *left_out.entry(r.rater.clone()).or_default() += 1;
                continue;
            };
            let (scale, verdict) = match &r.verdict {
                Some(v) => ("keep-maybe-drop 1", v.clone()),
                None => ("stars-1-5 1", json::decimal(r.rating)),
            };
            t.get_mut("ratings").unwrap().push(
                &inh,
                &RatingPackRow {
                    rater: entry.id.clone(),
                    target: tg,
                    scale: scale.into(),
                    verdict,
                    note: (entry.notes && !r.note.is_empty()).then(|| crate::paths::strip(&r.note)),
                    round: opt(&r.source),
                    date: r.created.clone(),
                },
            )?;
            for m in RaterBook::members_of(&entry.name) {
                if entry.members.is_empty() {
                    break;
                }
                if let Some(x) = book.get(&m) {
                    used.insert(x.id.clone(), x.clone());
                }
            }
            used.insert(entry.id.clone(), entry);
        }
        for (rater, n) in &left_out {
            out.notes.push(format!(
                "{n} ratings by '{rater}' left out: not every rater in it consented"
            ));
        }
        for e in used.values() {
            let members = (!e.members.is_empty()).then(|| {
                let mut v: Vec<String> = e
                    .members
                    .iter()
                    .filter_map(|m| book.get(m).map(|x| x.id.clone()))
                    .collect();
                v.sort();
                v
            });
            t.get_mut("raters").unwrap().push(
                &inh,
                &RaterRow {
                    rater: e.id.clone(),
                    consent_date: consent_time(&e.consent_date).unwrap_or_default(),
                    consent_scope: "public-permanent".into(),
                    display_name: (e.show_name && e.members.is_empty()).then(|| e.name.clone()),
                    orcid: (e.show_name && !e.orcid.is_empty()).then(|| e.orcid.clone()),
                    members,
                    notes: e.notes,
                },
            )?;
        }
    }

    // Every table's index: the parents' parts and the new ones.
    let mut tables_out: BTreeMap<String, String> = BTreeMap::new();
    for def in all() {
        let mut parts: Vec<Part> = inh.parts.get(def.name).cloned().unwrap_or_default();
        let new_parts = if def.name == "patterns" {
            pattern_parts.clone()
        } else if let Some(nr) = t.remove(def.name) {
            parquet_io::write_table_text(&store, def, nr.rows, opts.part_bytes)?
        } else {
            vec![]
        };
        let added: u64 = new_parts.iter().map(|p| p.rows).sum();
        parts.extend(new_parts);
        if parts.is_empty() {
            continue;
        }
        let h = write_index(&store, def, parts.clone())?;
        out.tables
            .insert(def.name.to_string(), (added, parts.len()));
        tables_out.insert(def.name.to_string(), h);
    }

    // Retractions: the parents' and the new ones.
    let mut retracts = inh.retracts.clone();
    for (tg, reason) in &opts.retract {
        let (k, id) = tg
            .split_once(':')
            .ok_or_else(|| format!("retract '{tg}': kind:id"))?;
        if !matches!(k, "pattern" | "ant" | "quickstart") || id.is_empty() {
            return Err(format!(
                "retract '{tg}': pattern:<id>, ant:<id> or quickstart:<id>"
            ));
        }
        // A Quick Start entry is retracted by its pack id, whichever id names it here.
        let tg = match qs_pack_ids.get(id) {
            Some(p) if k == "quickstart" => target(k, p),
            _ => tg.clone(),
        };
        if !retracts.iter().any(|r| r.target == tg) {
            retracts.push(Retract {
                target: tg,
                publisher: me.clone(),
                reason: reason.clone(),
                date: created.clone(),
            });
        }
    }
    retracts.sort();
    out.retracts = retracts.len();

    // The manifest.
    let mut stats: BTreeMap<String, u64> = BTreeMap::new();
    let mut bytes = 0u64;
    let count_rows = |name: &str| -> Result<u64, String> {
        let Some(h) = tables_out.get(name) else {
            return Ok(0);
        };
        let ix: Index = json::parse_as(&src.doc(h)?, "index", true)?;
        Ok(ix.parts.iter().map(|p| p.rows).sum())
    };
    for k in ["patterns", "ants", "quickstart"] {
        stats.insert(k.into(), count_rows(k)?);
    }
    let v_tmp = |h: &str| -> Result<u64, String> {
        let ix: Index = json::parse_as(&src.doc(h)?, "index", true)?;
        Ok(ix.parts.iter().map(|p| p.bytes).sum())
    };
    for h in tables_out.values() {
        bytes += v_tmp(h)?;
    }
    for h in searches.values() {
        let ix: SearchIndex = json::parse_as(&src.doc(h)?, "search index", true)?;
        bytes += ix.parts.iter().map(|p| p.bytes).sum::<u64>();
        bytes += ix
            .scores
            .iter()
            .flat_map(|s| &s.parts)
            .map(|p| p.bytes)
            .sum::<u64>();
    }
    stats.insert("bytes".into(), bytes);
    out.bytes = bytes;
    let mut creators = vec![];
    if !publisher.name.is_empty() {
        creators.push(Creator {
            name: publisher.name.clone(),
            orcid: publisher.orcid.clone(),
            role: "creator".into(),
        });
    }
    let manifest = Manifest {
        format: crate::FORMAT.into(),
        version: VERSION.into(),
        requires: vec![],
        feed: opts.feed.clone(),
        title: opts.title.clone(),
        description: opts.description.clone(),
        version_label: opts.version_label.clone(),
        created: created.clone(),
        licence: opts.licence.clone(),
        creators,
        doi: opts.doi.clone(),
        related: opts.related.clone(),
        parents: out.parents.clone(),
        publisher: me.clone(),
        engine: engine(),
        tables: tables_out,
        searches,
        retracts,
        retain: inh.retain.clone(),
        stats,
        slice_of: None,
        filter: None,
    };
    let mbytes = json::canonical_of(&manifest)?;
    let mh = store.put(&mbytes, "json")?;
    out.manifest = mh.clone();

    // The key statement, the manifest's signature and the ref.
    let kp = store.key_path(&me)?;
    let (_, kbytes) = key_statement(publisher, &created)?;
    if !kp.exists() {
        crate::store::write_atomic(&kp, &kbytes)?;
    }
    let sig = publisher.signer.sign(sign::NS_MANIFEST, &mbytes)?;
    crate::store::write_atomic(
        &store
            .sig_dir(&mh)?
            .join(format!("{}.sig", sign::signer_name(&me))),
        sig.as_bytes(),
    )?;
    let mut r = Ref {
        format: "jetcarp-ref".into(),
        version: VERSION.into(),
        feed: opts.feed.clone(),
        seq,
        manifest: mh.clone(),
        updated: created.clone(),
        publisher: me.clone(),
        signature: String::new(),
    };
    r.signature = publisher
        .signer
        .sign(sign::NS_REF, &json::canonical(&unsigned_value(&r)?)?)?;
    crate::store::write_atomic(&store.ref_path(&opts.feed)?, &json::canonical_of(&r)?)?;
    out.seq = seq;
    if opts.views {
        let v = src.version(&mh)?;
        store.write_file(
            Path::new("views.sql"),
            crate::export::views_sql(&v, None).as_bytes(),
        )?;
    }
    store.clean_temp();
    log(&format!("version {mh} written"));
    Ok(out)
}

/// What made the data: this JETCARP, its id schemes and its rule-semantics table.
pub fn engine() -> Engine {
    Engine {
        name: "JETCARP".into(),
        version: jetcarp_core::VERSION.into(),
        schemes: [
            ("pattern", jetcarp_core::pattern::range::IDENTITY_VERSION),
            ("ant", "jetcarp-ant-v1"),
            ("geometry", jetcarp_core::pin::PIN_VERSION),
            ("tables", crate::TABLES_SCHEME),
        ]
        .into_iter()
        .map(|(a, b)| (a.to_string(), b.to_string()))
        .collect(),
        rule_semantics: jetcarp_core::semantics::RULE_SEMANTICS
            .iter()
            .map(|(a, b)| (a.to_string(), b.to_string()))
            .collect(),
    }
}

fn annotate(
    t: &mut BTreeMap<&'static str, NewRows>,
    inh: &Inherited,
    tg: &str,
    me: &str,
    field: &str,
    value: String,
    date: &str,
    rev: i64,
) -> Result<(), String> {
    if inh.annotations.get(&(tg.to_string(), field.to_string())) == Some(&value) {
        return Ok(());
    }
    t.get_mut("annotations").unwrap().push(
        inh,
        &AnnotationRow {
            target: tg.to_string(),
            publisher: me.to_string(),
            field: field.to_string(),
            value,
            score_model: None,
            date: date.to_string(),
            rev,
        },
    )
}

fn provenance_row(
    kind: &str,
    id: &str,
    p: &jetcarp_store::library::ProvenanceRow,
    me: &str,
    run: Option<&jetcarp_core::pattern::Provenance>,
) -> crate::tables::ProvenanceRow {
    let strip = |s: &str| opt(&crate::paths::strip(s));
    crate::tables::ProvenanceRow {
        target: target(kind, id),
        publisher: me.to_string(),
        seq: p.seq as i32,
        kind: p.kind.clone(),
        finder: opt(&p.finder),
        orcid: opt(&p.orcid),
        search_kind: opt(&p.search_kind),
        search: opt(&p.job_uuid),
        machine: opt(&p.machine),
        engine_version: p.engine_version.clone(),
        pin: p.geometry_pin.clone(),
        date: p.date.clone(),
        source: strip(&p.source),
        pack: opt(&p.pack_hash),
        run: run.map(|r| {
            jetcarp_core::jcs::to_string(&crate::paths::strip_json(
                serde_json::to_value(r).unwrap_or_default(),
            ))
        }),
    }
}

/// Write a table's index (child indexes when it has more than 10,000 parts).
pub fn write_index(store: &Store, def: &TableDef, parts: Vec<Part>) -> Result<String, String> {
    let mk = |parts: Vec<Part>, children: Vec<String>| Index {
        format: "jetcarp-index".into(),
        version: VERSION.into(),
        table: def.name.into(),
        schema: def.schema.into(),
        parts,
        children,
        compacts: vec![],
    };
    if parts.len() <= read::MAX_PARTS {
        return store.put(&json::canonical_of(&mk(parts, vec![]))?, "json");
    }
    let mut children = vec![];
    for chunk in parts.chunks(read::MAX_PARTS) {
        children.push(store.put(&json::canonical_of(&mk(chunk.to_vec(), vec![]))?, "json")?);
    }
    if children.len() > read::MAX_PARTS {
        return Err(format!("{}: too many parts", def.name));
    }
    store.put(&json::canonical_of(&mk(vec![], children))?, "json")
}

/// A search folder's sealed segments and score sidecars copied in, and its index.
fn search_index(store: &Store, dir: &Path, uuid: &str) -> Result<(String, Vec<String>), String> {
    let b =
        std::fs::read(dir.join("manifest.json")).map_err(|e| format!("{}: {e}", dir.display()))?;
    let m: jetcarp_search::explore::runs::Manifest =
        serde_json::from_slice(&b).map_err(|e| format!("search {uuid} manifest: {e}"))?;
    let mut notes = vec![];
    if m.search != uuid {
        return Err(format!("search folder {uuid} holds search {}", m.search));
    }
    let stem = |f: &str| -> String {
        Path::new(f)
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_default()
    };
    let mut parts = vec![];
    for s in &m.segments {
        let (object, bytes) = store.put_file(&dir.join(&s.file), "parquet", Some(&s.sha256))?;
        parts.push(SearchPart {
            object,
            rows: s.rows,
            bytes,
            segment: stem(&s.file),
            first: s.first_id.to_string(),
            last: s.last_id.to_string(),
            sealed: s.sealed.clone(),
            engine_version: s.engine_version.clone(),
        });
    }
    let mut scores = vec![];
    for v in &m.scores {
        let mut ps = vec![];
        for f in &v.files {
            let (object, bytes) = store.put_file(&dir.join(&f.file), "parquet", Some(&f.sha256))?;
            ps.push(ScorePart {
                segment: stem(&f.segment),
                object,
                rows: f.rows,
                bytes,
            });
        }
        scores.push(ScoreVersion {
            version: v.version as u64,
            created: v.created.clone(),
            engine_version: v.engine_version.clone(),
            parts: ps,
        });
    }
    if dir.join("search.sqlite").is_file() {
        notes.push(format!(
            "search {uuid}: only its sealed segments are published; any runs its working database still holds are not (`jetcarp jobs seal` first)"
        ));
    }
    let clean = |v: Value| json::sanitize(crate::paths::strip_json(v));
    let ix = SearchIndex {
        format: "jetcarp-index".into(),
        version: VERSION.into(),
        table: "search".into(),
        schema: "jetcarp-search 1".into(),
        search: uuid.into(),
        kind: m.kind.clone(),
        run_schema: m.run_schema.clone(),
        score_schema: m.score_schema.clone(),
        engine_version: m.engine_version.clone(),
        created: m.created.clone(),
        updated: m.updated.clone(),
        spec: clean(m.spec.clone()),
        remeasure: m
            .remeasure
            .as_ref()
            .map(|r| clean(serde_json::to_value(r).unwrap_or_default())),
        parts,
        scores,
        children: vec![],
    };
    if ix.parts.len() > read::MAX_PARTS {
        return Err(format!(
            "search {uuid}: more than {} segments",
            read::MAX_PARTS
        ));
    }
    Ok((store.put(&json::canonical_of(&ix)?, "json")?, notes))
}

/// Copy a version's objects (manifest, indexes, parts, search segments, media, reference
/// patches), its signatures and the key statements of its signers into a store.
pub fn copy_version(src: &Source, v: &Version, to: &Store) -> Result<(), String> {
    to.put(&v.bytes, "json")?;
    for (h, b) in &v.indexes {
        if crate::hash::of(b) != *h {
            return Err(format!("index {h} is altered"));
        }
        to.put(b, "json")?;
    }
    for tp in v.tables.values() {
        for (p, path) in &tp.parts {
            to.put_file(path, "parquet", Some(crate::hash::hex_of(&p.object)?))?;
        }
    }
    for sp in v.searches.values() {
        for (p, f) in sp.index.parts.iter().zip(&sp.run_files) {
            to.put_file(f, "parquet", Some(crate::hash::hex_of(&p.object)?))?;
        }
        for (s, files) in sp.index.scores.iter().zip(&sp.score_files) {
            for (p, f) in s.parts.iter().zip(files) {
                to.put_file(f, "parquet", Some(crate::hash::hex_of(&p.object)?))?;
            }
        }
    }
    // Media and reference patches, named by table rows.
    src.table_rows(v, "media", |_, rows| {
        for r in rows {
            let m: MediaPackRow = serde_json::from_value(r).map_err(|e| e.to_string())?;
            let ext = media_ext(&m.media_type)?;
            let rel = read::export_media_path(&m.target, &m.kind, m.from_seed, ext)
                .ok_or("a media row with an unsafe target")?;
            let f = src.object_file(&m.object, ext, &rel)?;
            to.put_file(&f, ext, Some(crate::hash::hex_of(&m.object)?))?;
        }
        Ok(())
    })?;
    src.table_rows(v, "tilings", |_, rows| {
        for r in rows {
            let t: TilingRow = serde_json::from_value(r).map_err(|e| e.to_string())?;
            if let Some(h) = &t.reference {
                let rel = read::export_reference_path(&t.family).ok_or("unsafe family name")?;
                let f = src.object_file(h, "parquet", &rel)?;
                to.put_file(&f, "parquet", Some(crate::hash::hex_of(h)?))?;
            }
        }
        Ok(())
    })?;
    let sigs: Vec<(String, String)> = match src {
        Source::Store(s) => s.signatures(&v.hash)?,
        Source::Export(_) => vec![],
    };
    for (stem, text) in sigs {
        crate::store::write_atomic(
            &to.sig_dir(&v.hash)?.join(format!("{stem}.sig")),
            text.as_bytes(),
        )?;
    }
    for fp in v.signed.signers.iter().chain([&v.manifest.publisher]) {
        if let (Source::Store(s), Ok(dst)) = (src, to.key_path(fp))
            && let Ok(p) = s.key_path(fp)
            && p.is_file()
            && !dst.exists()
        {
            let b = crate::store::read_limited(&p, json::MAX_DOC)?;
            crate::store::write_atomic(&dst, &b)?;
        }
    }
    Ok(())
}

/// The extension of a media type.
pub fn media_ext(t: &str) -> Result<&'static str, String> {
    Ok(match t {
        "image/png" => "png",
        "image/gif" => "gif",
        "image/webp" => "webp",
        x => {
            return Err(format!(
                "media type '{x}' (image/png, image/gif or image/webp)"
            ));
        }
    })
}
