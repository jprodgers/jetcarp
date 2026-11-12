//! `library.*`: the pattern library: what it holds (`library.info`), its records (`library.query`,
//! `library.get`), curation (`library.update`, `library.remove`), pictures and text drawings of a
//! pattern's phases (`library.render`), scenes made of patterns (`library.paste`), checking
//! records again (`library.verify`; `library.reverify`, the job that clears the flags a geometry
//! change or a change in a rule's meaning leaves), splitting stored compounds into their parts
//! (`library.compounds`, a job), and making the file smaller (`library.prune`). Records travel
//! between libraries in packs (`pack.*`).
//!
//! Records are patterns (verified still lifes, oscillators, spaceships, guns, movers) and ant
//! runs (ids `ant-...`). The catalog is the context's ([`Ctx::library`]). Searches that add to it
//! are jobs ([`super::search`]); Quick Start entries are [`super::quickstart`].

use crate::args;
use crate::ctx::Ctx;
use crate::error::{ApiError, ErrorKind};
use crate::op::{NoInput, Op, Progress, Report};
use jetcarp_core::pattern::{
    self, CurationState, DiscoveredBy, PasteAt, PatternClass, PatternRecord,
};
use jetcarp_core::{Boundary, CaSpec, Command, Nb, Scene, SeedSpec, VERSION, World};
use jetcarp_store::library::CatalogInfo;
use jetcarp_store::{AntCensus, AntQuery, AntRow, AntUpdate, Query, SightingRow, Update};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use std::fmt::Write as _;

/// A catalog error with a kind: missing records are `not_found`, unknown words `invalid`, the
/// rest (SQLite, files) `io`.
pub(crate) fn catalog_err(e: String) -> ApiError {
    let kind = if e.starts_with("no ") {
        ErrorKind::NotFound
    } else if e.starts_with("unknown ") || e.starts_with("bad ") {
        ErrorKind::Invalid
    } else {
        ErrorKind::Io
    };
    ApiError::new(kind, e)
}

fn invalid(e: String) -> ApiError {
    ApiError::invalid(e)
}

/// `pattern` or `ant`: `kind` when given, else from the id's form (`ant-...`).
fn record_kind(kind: Option<&str>, id: &str) -> Result<&'static str, ApiError> {
    match kind {
        Some("pattern") => Ok("pattern"),
        Some("ant") => Ok("ant"),
        Some(k) => Err(ApiError::invalid(format!(
            "`kind` is pattern or ant, not {k}"
        ))),
        None if id.starts_with("ant-") => Ok("ant"),
        None => Ok("pattern"),
    }
}

/// A serde enum's name (`still_life`), as the parsers read it back.
fn word<T: Serialize>(t: &T) -> String {
    serde_json::to_value(t)
        .ok()
        .and_then(|v| v.as_str().map(String::from))
        .unwrap_or_default()
}

// ---------------------------------------------------------------------------------------------
// library.info

/// The catalog's file, schema version, and row counts.
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(transparent)]
pub struct LibraryInfo(#[schemars(with = "Value")] pub CatalogInfo);

impl Report for LibraryInfo {
    fn text(&self) -> String {
        let i = &self.0;
        let mut s = format!(
            "catalog:        {}\nschema version: {}\npatterns {}, rules {}, pattern rules {}, \
             sightings {} ({} copies), jobs {}, Quick Start {}, ratings {}, media {}, ants {}\n",
            i.path,
            i.schema_version,
            i.patterns,
            i.rules,
            i.pattern_rules,
            i.sightings,
            i.seen,
            i.jobs,
            i.quickstart,
            i.ratings,
            i.media,
            i.ants
        );
        if i.needs_reverify > 0 {
            s.push_str(&format!(
                "re-verify:      {} record{} flagged after a change of geometry or of a rule's meaning (jetcarp library reverify)\n",
                i.needs_reverify,
                if i.needs_reverify == 1 { "" } else { "s" }
            ));
        }
        s
    }
}

/// `library.info`.
pub struct LibraryInfoOp;
impl Op for LibraryInfoOp {
    const NAME: &'static str = "library.info";
    const SUMMARY: &'static str = "The catalog's file, schema version, and row counts.";
    const READ_ONLY: bool = true;
    type In = NoInput;
    type Out = LibraryInfo;
    fn run(cx: &Ctx, _: NoInput, _: &dyn Progress) -> Result<LibraryInfo, ApiError> {
        cx.library().info().map(LibraryInfo).map_err(catalog_err)
    }
}

// ---------------------------------------------------------------------------------------------
// library.query

/// Which records to list. Every filter given must match; pattern filters and ant filters are
/// marked.
#[derive(Clone, Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct QueryIn {
    /// `pattern` (default) or `ant`.
    #[serde(default)]
    pub kind: Option<String>,
    /// Patterns verified in a rule whose notation contains this; ants: this turn string or table
    /// (`RL`, `Turmite LLRR`; its mirror image matches too).
    #[serde(default)]
    pub rule: Option<String>,
    /// Patterns that run in this rule (inside their rule range, or verified there).
    #[serde(default)]
    pub works_in: Option<String>,
    /// Patterns with this neighbourhood: `touch` or `edge`.
    #[serde(default)]
    pub neighborhood: Option<String>,
    /// Tiling family id (`hex`), catalog id (`k2-001`), or part of the tiling name.
    #[serde(default)]
    pub tiling: Option<String>,
    /// Patterns of this class: still_life, oscillator, spaceship, puffer, gun, mover.
    #[serde(default)]
    pub class: Option<String>,
    /// Patterns with a period of at least this.
    #[serde(default)]
    pub min_period: Option<u64>,
    /// Patterns with a period of at most this.
    #[serde(default)]
    pub max_period: Option<u64>,
    /// Patterns scoring at least this (0-100).
    #[serde(default)]
    pub min_score: Option<f64>,
    /// Patterns in this curation state: new, reviewed, promoted, rejected.
    #[serde(default)]
    pub state: Option<String>,
    /// Patterns with this tag.
    #[serde(default)]
    pub tag: Option<String>,
    /// Patterns whose name or label contains this.
    #[serde(default)]
    pub text: Option<String>,
    /// Patterns found by: search, script, llm, user.
    #[serde(default)]
    pub discovered_by: Option<String>,
    /// Pattern order: score (default), seen, rare, newest, updated, cells, period, rating, id.
    #[serde(default)]
    pub order: Option<String>,
    /// Skip this many patterns first (paging).
    #[serde(default)]
    pub offset: Option<usize>,
    /// At most this many records.
    #[serde(default)]
    pub limit: Option<usize>,
    /// Ants with this outcome: highway, periodic, symmetric, bounded, chaotic.
    #[serde(default)]
    pub outcome: Option<String>,
    /// Ants of this search job.
    #[serde(default)]
    pub job: Option<i64>,
    /// Ant order: score (default), onset, period, speed, rule.
    #[serde(default)]
    pub sort: Option<String>,
    /// Only count the matching patterns (no records).
    #[serde(default)]
    pub count_only: bool,
}

impl QueryIn {
    /// The pattern filters of a catalog [`Query`] (what Rhai's `lib.query(#{...})` reads).
    pub fn from_query(q: &Query) -> QueryIn {
        QueryIn {
            kind: None,
            rule: q.rule.clone(),
            works_in: q.works_in.clone(),
            neighborhood: q.neighborhood.as_ref().map(word),
            tiling: q.tiling.clone(),
            class: q.class.as_ref().map(word),
            min_period: q.min_period,
            max_period: q.max_period,
            min_score: q.min_score,
            state: q.state.as_ref().map(word),
            tag: q.tag.clone(),
            text: q.text.clone(),
            discovered_by: q.discovered_by.as_ref().map(word),
            order: q.order.clone(),
            offset: q.offset,
            limit: q.limit,
            ..Default::default()
        }
    }

    /// The catalog query for patterns.
    pub fn pattern_query(&self) -> Result<Query, ApiError> {
        Ok(Query {
            rule: self.rule.clone(),
            works_in: self.works_in.clone(),
            neighborhood: self
                .neighborhood
                .as_deref()
                .map(|n| {
                    args::nb_word(n).ok_or_else(|| {
                        ApiError::invalid(format!("neighborhood is touch or edge, not {n}"))
                    })
                })
                .transpose()?,
            order: self.order.clone(),
            offset: self.offset,
            tiling: self.tiling.clone(),
            class: self
                .class
                .as_deref()
                .map(PatternClass::parse)
                .transpose()
                .map_err(invalid)?,
            min_period: self.min_period,
            max_period: self.max_period,
            min_score: self.min_score,
            state: self
                .state
                .as_deref()
                .map(CurationState::parse)
                .transpose()
                .map_err(invalid)?,
            tag: self.tag.clone(),
            text: self.text.clone(),
            discovered_by: self
                .discovered_by
                .as_deref()
                .map(DiscoveredBy::parse)
                .transpose()
                .map_err(invalid)?,
            limit: self.limit,
        })
    }

    fn ant_query(&self) -> Result<AntQuery, ApiError> {
        Ok(AntQuery {
            outcome: self
                .outcome
                .as_deref()
                .map(jetcarp_core::ants::run::Outcome::parse)
                .transpose()
                .map_err(invalid)?,
            tiling: self.tiling.clone(),
            rule: self.rule.clone(),
            job: self.job,
            sort: self.sort.clone(),
            limit: self.limit,
        })
    }
}

/// The records found.
#[derive(Clone, Debug, Serialize, JsonSchema)]
pub struct QueryOut {
    /// The catalog file.
    pub catalog: String,
    /// `pattern` or `ant`.
    pub kind: String,
    /// Patterns matching the filters, before `offset` and `limit`.
    pub matching: Option<u64>,
    /// The patterns, in order.
    #[schemars(with = "Vec<Value>")]
    pub patterns: Vec<PatternRecord>,
    /// The ant runs, in order.
    #[schemars(with = "Vec<Value>")]
    pub ants: Vec<AntRow>,
    /// Ants: outcome counts per tiling (of `job`, when given).
    #[schemars(with = "Vec<Value>")]
    pub census: Vec<AntCensus>,
    /// The records shown that a pack retracted (`library show` says by whom and why).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub retracted: Vec<String>,
}

impl Report for QueryOut {
    fn text(&self) -> String {
        let mut s = String::new();
        let mark = |id: &str| {
            if self.retracted.iter().any(|x| x == id) {
                "  (retracted)"
            } else {
                ""
            }
        };
        if self.kind == "ant" {
            for r in &self.ants {
                let _ = writeln!(s, "{}{}", r.line(), mark(&r.id));
            }
            return s;
        }
        if let Some(m) = self.matching {
            let _ = writeln!(
                s,
                "catalog: {} ({} shown of {m} matching)",
                self.catalog,
                self.patterns.len()
            );
        }
        for r in &self.patterns {
            let _ = writeln!(s, "{}{}", r.summary(), mark(&r.id));
        }
        s
    }
}

/// `library.query`.
pub struct LibraryQuery;
impl Op for LibraryQuery {
    const NAME: &'static str = "library.query";
    const SUMMARY: &'static str =
        "List catalog records (patterns, best first, or ant runs), filtered and paged.";
    const READ_ONLY: bool = true;
    type In = QueryIn;
    type Out = QueryOut;
    fn run(cx: &Ctx, a: QueryIn, _: &dyn Progress) -> Result<QueryOut, ApiError> {
        let lib = cx.library();
        let catalog = lib.path().display().to_string();
        match a.kind.as_deref().unwrap_or("pattern") {
            "pattern" => {
                let q = a.pattern_query()?;
                let matching = lib.count(&q).map_err(catalog_err)? as u64;
                let patterns = if a.count_only {
                    Vec::new()
                } else {
                    lib.query(&q).map_err(catalog_err)?
                };
                let by = lib.retractions_by_id("pattern").map_err(catalog_err)?;
                Ok(QueryOut {
                    catalog,
                    kind: "pattern".into(),
                    matching: Some(matching),
                    retracted: patterns
                        .iter()
                        .filter(|r| by.contains_key(&r.id))
                        .map(|r| r.id.clone())
                        .collect(),
                    patterns,
                    ants: Vec::new(),
                    census: Vec::new(),
                })
            }
            "ant" => {
                let q = a.ant_query()?;
                let ants = lib.ants(&q).map_err(catalog_err)?;
                let by = lib.retractions_by_id("ant").map_err(catalog_err)?;
                Ok(QueryOut {
                    catalog,
                    kind: "ant".into(),
                    matching: None,
                    patterns: Vec::new(),
                    census: lib.ant_census(q.job).map_err(catalog_err)?,
                    retracted: ants
                        .iter()
                        .filter(|r| by.contains_key(&r.id))
                        .map(|r| r.id.clone())
                        .collect(),
                    ants,
                })
            }
            k => Err(ApiError::invalid(format!(
                "`kind` is pattern or ant, not {k}"
            ))),
        }
    }
}

// ---------------------------------------------------------------------------------------------
// library.get

/// One phase drawn as text: rows of `.` and `O` on square and hexagon grids.
#[derive(Clone, Debug, Serialize, JsonSchema)]
pub struct PhaseText {
    pub phase: u64,
    /// The drawing (square and hexagon grids only).
    pub ascii: Option<String>,
    /// The phase's cells (`i,j,k,s` tuples), for tilings without a drawing.
    pub cells: String,
}

impl PhaseText {
    /// `phase 0:` and the drawing, or `phase 0: cells` on one line.
    pub fn text(&self) -> String {
        match &self.ascii {
            Some(a) => format!("phase {}:\n{a}", self.phase),
            None => format!("phase {}: {}\n", self.phase, self.cells),
        }
    }
}

/// Phases `0..n` (at least one) of a record as text.
pub fn phases_text(rec: &PatternRecord, n: u64) -> Result<Vec<PhaseText>, ApiError> {
    (0..n.max(1))
        .map(|t| {
            Ok(PhaseText {
                phase: t,
                ascii: pattern::ascii(rec, t).map_err(ApiError::engine)?,
                cells: rec.phases[(t % rec.period.max(1)) as usize].to_string(),
            })
        })
        .collect()
}

/// Which record to get.
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GetIn {
    /// Pattern id, unique id prefix, or name (`glider`); or an ant run id (`ant-...`).
    pub id: String,
    /// `pattern` or `ant` (default: from the id's form).
    #[serde(default)]
    pub kind: Option<String>,
    /// Phases to draw as text (default 1).
    #[serde(default)]
    pub phases: Option<u64>,
    /// Only the record: no sightings, orientations, or phases.
    #[serde(default)]
    pub bare: bool,
}

/// A record and what goes with it: a pattern's sightings, the way it moves in each paste
/// orientation, and its phases as text; or an ant run.
#[derive(Clone, Debug, Serialize, JsonSchema)]
pub struct RecordView {
    /// `pattern` or `ant`.
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(with = "Option<Value>")]
    pub pattern: Option<PatternRecord>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(with = "Option<Value>")]
    pub ant: Option<AntRow>,
    pub label: String,
    /// Copies seen, by rule and search job.
    #[schemars(with = "Vec<Value>")]
    pub sightings: Vec<SightingRow>,
    /// Spaceships: the lattice vector moved per period in each paste orientation.
    pub orientations: Vec<[i32; 2]>,
    pub phases: Vec<PhaseText>,
    /// Retractions a pack recorded for it: `retracted by <publisher> on <date>: <reason>`. A
    /// retracted record stays in the library, shown as retracted.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub retracted: Vec<String>,
}

impl Report for RecordView {
    fn text(&self) -> String {
        let (Some(r), "pattern") = (&self.pattern, self.kind.as_str()) else {
            let mut s = self
                .ant
                .as_ref()
                .map(|a| format!("{}\n", a.line()))
                .unwrap_or_default();
            for x in &self.retracted {
                let _ = writeln!(s, "{x}");
            }
            return s;
        };
        let mut s = String::new();
        let _ = writeln!(s, "id:          {}", r.id);
        let _ = writeln!(s, "label:       {}", r.label());
        for x in &self.retracted {
            let _ = writeln!(s, "retracted:   {}", x.trim_start_matches("retracted "));
        }
        let _ = writeln!(
            s,
            "rule:        {} ({:?} neighbours, {} step(s) per generation)",
            r.rule, r.neighborhood, r.steps_per_gen
        );
        let _ = writeln!(
            s,
            "tiling:      {} ({} tile type(s) per unit cell)",
            r.tiling_name, r.tile_types
        );
        let _ = writeln!(s, "class:       {}, period {}", r.class.name(), r.period);
        if r.rules.len() > 1 {
            let _ = writeln!(s, "verified in: {}", r.rules.join(", "));
        }
        if let Some(x) = &r.range {
            let _ = writeln!(s, "works in:    {}", x.describe());
        }
        if let (Some(d), Some(xy)) = (r.displacement, r.displacement_xy) {
            let _ = writeln!(
                s,
                "moves:       {} : lattice vector ({}, {}) per period = ({:.3}, {:.3}) world units",
                r.speed.as_deref().unwrap_or("?"),
                d[0],
                d[1],
                xy[0],
                xy[1]
            );
            // Direction in each orientation, for `collide --orientation-b` and pasting.
            let dirs: Vec<String> = self
                .orientations
                .iter()
                .enumerate()
                .map(|(k, v)| format!("{k}:({},{})", v[0], v[1]))
                .collect();
            let _ = writeln!(s, "orientations: {}", dirs.join(" "));
        }
        let _ = writeln!(
            s,
            "cells:       {} (phases {}..{}), bounding box {}x{}",
            r.cells,
            r.min_cells,
            r.max_cells,
            r.bbox[2] + 1,
            r.bbox[3] + 1
        );
        let _ = writeln!(s, "seen:        {}", r.seen);
        for x in &self.sightings {
            let _ = writeln!(
                s,
                "             {} x{} in {} soup(s), job {} ({} to {})",
                x.rule, x.count, x.soups, x.job, x.first_seen, x.last_seen
            );
        }
        let _ = writeln!(
            s,
            "score:       {}{}",
            r.score,
            r.rating
                .map(|x| format!("  rating {x}/5"))
                .unwrap_or_default()
        );
        let _ = writeln!(
            s,
            "state:       {}  (discovered by {:?})",
            r.state.name(),
            r.discovered_by
        );
        if !r.tags.is_empty() {
            let _ = writeln!(s, "tags:        {}", r.tags.join(", "));
        }
        let _ = writeln!(
            s,
            "verified:    {} periods ({} generations) on {}, JETCARP {} {}",
            r.verified.periods,
            r.verified.generations,
            r.verified.field,
            r.verified.jetcarp_version,
            r.verified.date
        );
        if r.needs_reverify {
            let (family, engine) =
                jetcarp_core::semantics::of_notation(&r.rule).unwrap_or(("?", "?"));
            let why = if r.semantics != engine {
                format!(
                    "under version {} of the {family} rules' meaning (this engine has {engine})",
                    r.semantics
                )
            } else {
                format!("on another geometry (pin {})", r.geometry_pin)
            };
            let _ = writeln!(
                s,
                "flagged:     needs re-verification: it was verified {why}; run jetcarp library reverify"
            );
        }
        let p = &r.provenance;
        let _ = writeln!(
            s,
            "found:       {} {}{}{} JETCARP {} {}",
            p.source,
            p.soup.map(|s| format!("soup {s} ")).unwrap_or_default(),
            p.rng_state.map(|s| format!("rng {s} ")).unwrap_or_default(),
            p.generation
                .map(|g| format!("generation {g}"))
                .unwrap_or_default(),
            p.jetcarp_version,
            p.date
        );
        if let Some(x) = &r.site {
            let _ = writeln!(
                s,
                "site:        {} tiles within {} steps, anchor cell {} on {} {:?}",
                x.tiles.len(),
                x.radius,
                x.anchor,
                x.patch.family,
                x.patch.size
            );
        }
        let _ = writeln!(s, "canonical:   {}", r.canonical);
        if !r.notes.is_empty() {
            let _ = writeln!(s, "notes:\n{}", r.notes);
        }
        s.push('\n');
        for p in &self.phases {
            s.push_str(&p.text());
        }
        s
    }
}

/// A record's retractions, described ([`jetcarp_store::library::pack::RetractionRow::describe`]).
fn retracted(lib: &jetcarp_store::Library, kind: &str, id: &str) -> Result<Vec<String>, ApiError> {
    let mut by = lib.retractions_by_id(kind).map_err(catalog_err)?;
    let Some(rows) = by.remove(id) else {
        return Ok(Vec::new());
    };
    let names = lib.publisher_names().map_err(catalog_err)?;
    Ok(rows.iter().map(|r| r.describe(&names)).collect())
}

/// `library.get`.
pub struct LibraryGet;
impl Op for LibraryGet {
    const NAME: &'static str = "library.get";
    const SUMMARY: &'static str = "One catalog record: a pattern with its sightings, orientations, and phases drawn as text, or an ant run.";
    const READ_ONLY: bool = true;
    type In = GetIn;
    type Out = RecordView;
    fn run(cx: &Ctx, a: GetIn, _: &dyn Progress) -> Result<RecordView, ApiError> {
        let lib = cx.library();
        if record_kind(a.kind.as_deref(), &a.id)? == "ant" {
            let r = lib.ant(&a.id).map_err(catalog_err)?;
            return Ok(RecordView {
                kind: "ant".into(),
                label: r.line(),
                retracted: retracted(&lib, "ant", &r.id)?,
                pattern: None,
                ant: Some(r),
                sightings: Vec::new(),
                orientations: Vec::new(),
                phases: Vec::new(),
            });
        }
        let r = lib.get(&a.id).map_err(catalog_err)?;
        let retracted = retracted(&lib, "pattern", &r.id)?;
        if a.bare {
            return Ok(RecordView {
                kind: "pattern".into(),
                label: r.label(),
                sightings: Vec::new(),
                orientations: Vec::new(),
                phases: Vec::new(),
                pattern: Some(r),
                ant: None,
                retracted,
            });
        }
        Ok(RecordView {
            kind: "pattern".into(),
            label: r.label(),
            sightings: lib.sightings(&r.id).map_err(catalog_err)?,
            orientations: pattern::orientation_moves(&r).map_err(ApiError::engine)?,
            phases: phases_text(&r, a.phases.unwrap_or(1))?,
            pattern: Some(r),
            ant: None,
            retracted,
        })
    }
}

// ---------------------------------------------------------------------------------------------
// library.update

/// Changes to a record; what is not given stays.
#[derive(Clone, Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct UpdateIn {
    /// Pattern id, unique prefix, or name; or an ant run id (`ant-...`).
    pub id: String,
    /// `pattern` or `ant` (default: from the id's form).
    #[serde(default)]
    pub kind: Option<String>,
    /// New name (empty clears it).
    #[serde(default)]
    pub name: Option<String>,
    /// Patterns: replace all tags.
    #[serde(default)]
    pub tags: Option<Vec<String>>,
    /// Patterns: tags to add.
    #[serde(default)]
    pub add_tags: Vec<String>,
    /// Patterns: tags to remove.
    #[serde(default)]
    pub remove_tags: Vec<String>,
    /// Replace the notes.
    #[serde(default)]
    pub notes: Option<String>,
    /// Patterns: add a paragraph to the notes.
    #[serde(default)]
    pub append_notes: Option<String>,
    /// 1 (dull) to 5 (remarkable); 0 clears it on patterns.
    #[serde(default)]
    pub rating: Option<f64>,
    /// Patterns: new, reviewed, promoted, rejected. Ants: any word.
    #[serde(default)]
    pub state: Option<String>,
    /// Patterns: who found it (search, script, llm, user).
    #[serde(default)]
    pub discovered_by: Option<String>,
}

impl UpdateIn {
    /// The changes of a catalog [`Update`] (what Rhai's `lib.update(id, #{...})` reads).
    pub fn from_update(id: &str, u: &Update) -> UpdateIn {
        UpdateIn {
            id: id.to_string(),
            kind: Some("pattern".into()),
            name: u.name.clone(),
            tags: u.tags.clone(),
            add_tags: u.add_tags.clone(),
            remove_tags: u.remove_tags.clone(),
            notes: u.notes.clone(),
            append_notes: u.append_notes.clone(),
            rating: u.rating.map(f64::from),
            state: u.state.as_ref().map(word),
            discovered_by: u.discovered_by.as_ref().map(word),
        }
    }
}

/// The record after the change.
#[derive(Clone, Debug, Serialize, JsonSchema)]
pub struct Updated {
    /// `pattern` or `ant`.
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(with = "Option<Value>")]
    pub pattern: Option<PatternRecord>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(with = "Option<Value>")]
    pub ant: Option<AntRow>,
}

impl Report for Updated {
    fn text(&self) -> String {
        match (&self.pattern, &self.ant) {
            (Some(r), _) => format!(
                "updated {} ({}), state {}\n",
                r.id,
                r.label(),
                r.state.name()
            ),
            (None, Some(a)) => format!("updated {}\n", a.line()),
            _ => String::new(),
        }
    }
}

/// `library.update`.
pub struct LibraryUpdate;
impl Op for LibraryUpdate {
    const NAME: &'static str = "library.update";
    const SUMMARY: &'static str =
        "Curate a record: name, tags, notes, rating, curation state, who found it.";
    const READ_ONLY: bool = false;
    type In = UpdateIn;
    type Out = Updated;
    fn run(cx: &Ctx, a: UpdateIn, _: &dyn Progress) -> Result<Updated, ApiError> {
        let lib = cx.library();
        if record_kind(a.kind.as_deref(), &a.id)? == "ant" {
            if a.tags.is_some()
                || !a.add_tags.is_empty()
                || !a.remove_tags.is_empty()
                || a.append_notes.is_some()
                || a.discovered_by.is_some()
            {
                return Err(ApiError::invalid(
                    "an ant run takes name, notes, rating, and state",
                ));
            }
            let r = lib
                .update_ant(
                    &a.id,
                    &AntUpdate {
                        name: a.name,
                        notes: a.notes,
                        rating: a.rating.map(|x| x.round() as i64),
                        state: a.state,
                    },
                )
                .map_err(catalog_err)?;
            return Ok(Updated {
                kind: "ant".into(),
                pattern: None,
                ant: Some(r),
            });
        }
        let u = Update {
            name: a.name,
            tags: a.tags,
            add_tags: a.add_tags,
            remove_tags: a.remove_tags,
            notes: a.notes,
            append_notes: a.append_notes,
            rating: a.rating.map(|x| x.clamp(0.0, 255.0) as u8),
            state: a
                .state
                .as_deref()
                .map(CurationState::parse)
                .transpose()
                .map_err(invalid)?,
            discovered_by: a
                .discovered_by
                .as_deref()
                .map(DiscoveredBy::parse)
                .transpose()
                .map_err(invalid)?,
        };
        let r = lib.update(&a.id, &u).map_err(catalog_err)?;
        Ok(Updated {
            kind: "pattern".into(),
            pattern: Some(r),
            ant: None,
        })
    }
}

// ---------------------------------------------------------------------------------------------
// library.remove

/// The record to delete.
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RemoveIn {
    /// Pattern id, unique prefix, or name; or an ant run id (`ant-...`).
    pub id: String,
    /// `pattern` or `ant` (default: from the id's form).
    #[serde(default)]
    pub kind: Option<String>,
}

/// The record deleted.
#[derive(Clone, Debug, Serialize, JsonSchema)]
pub struct Removed {
    pub kind: String,
    pub id: String,
    pub label: String,
}

impl Report for Removed {
    fn text(&self) -> String {
        if self.kind == "ant" {
            // An ant run's line starts with its id.
            return format!("removed {}\n", self.label);
        }
        format!("removed {} ({})\n", self.id, self.label)
    }
}

/// `library.remove`.
pub struct LibraryRemove;
impl Op for LibraryRemove {
    const NAME: &'static str = "library.remove";
    const SUMMARY: &'static str =
        "Delete a pattern (with its rules, sightings, and Quick Start entry) or an ant run.";
    const READ_ONLY: bool = false;
    type In = RemoveIn;
    type Out = Removed;
    fn run(cx: &Ctx, a: RemoveIn, _: &dyn Progress) -> Result<Removed, ApiError> {
        let lib = cx.library();
        if record_kind(a.kind.as_deref(), &a.id)? == "ant" {
            let r = lib.remove_ant(&a.id).map_err(catalog_err)?;
            return Ok(Removed {
                kind: "ant".into(),
                label: r.line(),
                id: r.id,
            });
        }
        let r = lib.remove(&a.id).map_err(catalog_err)?;
        Ok(Removed {
            kind: "pattern".into(),
            label: r.label(),
            id: r.id,
        })
    }
}

// ---------------------------------------------------------------------------------------------
// library.render

fn px_default() -> u32 {
    160
}
fn max_phases_default() -> u32 {
    16
}

/// How to draw a pattern.
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RenderIn {
    /// Pattern id, unique prefix, or name.
    pub id: String,
    /// Write the phases side by side to this PNG.
    #[serde(default)]
    pub png: Option<String>,
    /// Panel size in pixels (default 160).
    #[serde(default = "px_default")]
    pub px: u32,
    /// At most this many phases (default 16).
    #[serde(default = "max_phases_default")]
    pub max_phases: u32,
    /// Draw the phases as text too (always, without a PNG).
    #[serde(default)]
    pub ascii: bool,
    /// The PNG's palette (default: the rule's own colours).
    #[serde(default)]
    pub palette: Option<String>,
}

/// A pattern drawn.
#[derive(Clone, Debug, Serialize, JsonSchema)]
pub struct Rendered {
    pub id: String,
    pub label: String,
    /// The phases as text (when asked for, or when there is no PNG).
    pub phases: Vec<PhaseText>,
    /// The PNG written (as given).
    pub png: Option<String>,
    /// `[width, height]` of the PNG.
    pub png_size: Option<(u32, u32)>,
}

impl Report for Rendered {
    fn text(&self) -> String {
        let mut s: String = self.phases.iter().map(PhaseText::text).collect();
        if let (Some(p), Some((w, h))) = (&self.png, self.png_size) {
            let _ = writeln!(s, "png: {p} ({w}x{h})");
        }
        s
    }
}

/// `library.render`.
pub struct LibraryRender;
impl Op for LibraryRender {
    const NAME: &'static str = "library.render";
    const SUMMARY: &'static str =
        "Draw a pattern's phases side by side as a PNG, or print them as text.";
    const READ_ONLY: bool = false;
    type In = RenderIn;
    type Out = Rendered;
    fn run(cx: &Ctx, a: RenderIn, _: &dyn Progress) -> Result<Rendered, ApiError> {
        if let Some(p) = &a.palette {
            jetcarp_core::render::builtin_palette(p).map_err(ApiError::invalid)?;
        }
        let rec = cx.library().get(&a.id).map_err(catalog_err)?;
        let phases = if a.ascii || a.png.is_none() {
            phases_text(&rec, rec.period.min(a.max_phases as u64))?
        } else {
            Vec::new()
        };
        let mut png_size = None;
        if let Some(p) = &a.png {
            let img =
                pattern::promote::render_phases_in(&rec, a.px, a.max_phases, a.palette.as_deref())
                    .map_err(ApiError::engine)?;
            let meta = vec![
                ("jetcarp:version", VERSION.to_string()),
                ("jetcarp:pattern", rec.id.clone()),
                ("jetcarp:label", rec.label()),
            ];
            let file = cx.fs.writable(p)?;
            jetcarp_core::render::write_png(&file, &img, &meta).map_err(ApiError::io)?;
            png_size = Some((img.width, img.height));
        }
        Ok(Rendered {
            label: rec.label(),
            id: rec.id,
            phases,
            png: a.png,
            png_size,
        })
    }
}

// ---------------------------------------------------------------------------------------------
// library.paste

fn paste_size_default() -> String {
    "120x80".into()
}
fn wrap_default() -> String {
    "wrap".into()
}

/// A scene made of library patterns on one grid.
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PasteSceneIn {
    /// Placements `ID@X,Y[,oK][,pK]` (orientation K, phase K), `ID@#CELL`, or `ID` alone for the
    /// centre (aperiodic patches: where it was found).
    pub placements: Vec<String>,
    /// The scene file to write.
    pub out: String,
    /// Grid size in unit cells (default 120x80), or L<level> on aperiodic tilings.
    #[serde(default = "paste_size_default")]
    pub size: String,
    /// wrap (default) or dead.
    #[serde(default = "wrap_default")]
    pub boundary: String,
    /// Scene name (default "N patterns").
    #[serde(default)]
    pub name: Option<String>,
    /// Also write a PNG of the scene after `gens` generations.
    #[serde(default)]
    pub png: Option<String>,
    /// Generations to run before the PNG (default 0).
    #[serde(default)]
    pub gens: u64,
}

/// The scene written.
#[derive(Clone, Debug, Serialize, JsonSchema)]
pub struct PastedScene {
    /// The scene file, as given.
    pub out: String,
    pub patterns: usize,
    pub cas: usize,
    pub hash: String,
    pub png: Option<String>,
    #[serde(serialize_with = "jetcarp_core::json_int::uint::serialize")]
    #[schemars(schema_with = "crate::op::uint_schema")]
    pub generation: u64,
    #[schemars(with = "Value")]
    pub scene: Scene,
}

impl Report for PastedScene {
    fn text(&self) -> String {
        let mut s = format!(
            "scene: {} ({} patterns, {} CA(s), hash {})\n",
            self.out, self.patterns, self.cas, self.hash
        );
        if let Some(p) = &self.png {
            let _ = writeln!(s, "png:   {p} (generation {})", self.generation);
        }
        s
    }
}

/// `--boundary` for pastes: wrap or dead (no live edges).
fn wrap_or_dead(s: &str) -> Result<Boundary, ApiError> {
    match args::parse_boundary(s) {
        Ok(b @ (Boundary::Wrap | Boundary::Dead)) => Ok(b),
        _ => Err(ApiError::invalid("--boundary is wrap or dead")),
    }
}

/// `library.paste`.
pub struct LibraryPaste;
impl Op for LibraryPaste {
    const NAME: &'static str = "library.paste";
    const SUMMARY: &'static str = "Make a scene from several library patterns placed on one grid (and optionally a PNG of it).";
    const READ_ONLY: bool = false;
    type In = PasteSceneIn;
    type Out = PastedScene;
    fn run(cx: &Ctx, a: PasteSceneIn, _: &dyn Progress) -> Result<PastedScene, ApiError> {
        struct Place {
            rec: PatternRecord,
            at: PasteAt,
            orientation: u32,
            phase: u32,
        }
        let bad =
            |what: &str, e: std::num::ParseIntError| ApiError::invalid(format!("{what}: {e}"));
        if a.placements.is_empty() {
            return Err(ApiError::invalid("give at least one placement"));
        }
        let lib = cx.library();
        let mut places = Vec::new();
        for s in &a.placements {
            let (id, rest) = s.split_once('@').unwrap_or((s.as_str(), ""));
            let rec = lib.get(id).map_err(catalog_err)?;
            let mut parts = rest.split(',').map(str::trim).filter(|x| !x.is_empty());
            let mut at = if rec.site.is_some() {
                PasteAt::Original
            } else {
                PasteAt::Center
            };
            let (mut orientation, mut phase) = (0, 0);
            if let Some(x) = parts.next() {
                if let Some(cell) = x.strip_prefix('#') {
                    at = PasteAt::Index {
                        cell: cell.parse().map_err(|e| bad("cell index after #", e))?,
                    };
                } else {
                    let y = parts
                        .next()
                        .ok_or_else(|| ApiError::invalid("placement is ID@X,Y"))?;
                    at = PasteAt::Cell {
                        x: x.parse().map_err(|e| bad("x", e))?,
                        y: y.parse().map_err(|e| bad("y", e))?,
                    };
                }
            }
            for q in parts {
                if let Some(o) = q.strip_prefix('o') {
                    orientation = o.parse().map_err(|e| bad("orientation", e))?;
                } else if let Some(ph) = q.strip_prefix('p') {
                    phase = ph.parse().map_err(|e| bad("phase", e))?;
                } else {
                    return Err(ApiError::invalid(format!(
                        "unknown placement option '{q}' (oK = orientation, pK = phase)"
                    )));
                }
            }
            places.push(Place {
                rec,
                at,
                orientation,
                phase,
            });
        }
        let first = &places[0].rec;
        for pl in &places[1..] {
            if pattern::tiling_key(&pl.rec.tiling) != pattern::tiling_key(&first.tiling) {
                return Err(ApiError::invalid(format!(
                    "all patterns must be on one tiling ({} vs {})",
                    first.tiling_name, pl.rec.tiling_name
                )));
            }
        }
        let mut tiling = match &first.site {
            Some(s) => s.patch.clone(),
            None => pattern::tiling_identity(&first.tiling),
        };
        if first.site.is_none() {
            tiling.size = Some(args::size_spec(&a.size).map_err(ApiError::invalid)?);
            tiling.boundary = wrap_or_dead(&a.boundary)?;
        }
        let mut sc = Scene::new(tiling);
        sc.name = a
            .name
            .clone()
            .unwrap_or_else(|| format!("{} patterns", places.len()));
        // One CA per distinct rule, in order of first use.
        let mut rules: Vec<(String, Nb, u32)> = Vec::new();
        for pl in &places {
            let k = (
                pl.rec.rule.clone(),
                pl.rec.neighborhood,
                pl.rec.steps_per_gen,
            );
            if !rules.contains(&k) {
                rules.push(k);
            }
        }
        sc.cas = rules
            .iter()
            .map(|(r, nb, s)| CaSpec::new(r).nb(*nb).steps(*s).seed(SeedSpec::Empty))
            .collect();
        for pl in &places {
            let k = (
                pl.rec.rule.clone(),
                pl.rec.neighborhood,
                pl.rec.steps_per_gen,
            );
            let ca = rules.iter().position(|x| *x == k).unwrap_or(0) as u32 + 1;
            sc.commands.push(Command::PastePattern {
                id: ca,
                pattern: pattern::paste::source_from_record(&pl.rec, pl.phase as u64),
                at: pl.at.clone(),
                orientation: pl.orientation,
                phase: 0,
            });
        }
        let mut w = World::from_scene(&sc)?;
        let file = cx.fs.writable(&a.out)?;
        std::fs::write(&file, sc.to_json() + "\n")
            .map_err(|e| ApiError::io(format!("writing {}: {e}", a.out)))?;
        let hash = w.hash_hex();
        if let Some(png) = &a.png {
            if a.gens > 0 {
                w.apply(Command::Step { gens: a.gens })?;
            }
            let opts = jetcarp_core::render::RenderOptions::default();
            let file = cx.fs.writable(png)?;
            jetcarp_core::render::write_png_streaming(
                &file,
                &w,
                &opts,
                &jetcarp_core::render::png_metadata(&w),
            )?;
        }
        Ok(PastedScene {
            out: a.out,
            patterns: places.len(),
            cas: sc.cas.len(),
            hash,
            png: a.png,
            generation: w.generation,
            scene: sc,
        })
    }
}

// ---------------------------------------------------------------------------------------------
// library.verify

fn sample_default() -> usize {
    200
}

/// Which patterns to verify again.
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct VerifyIn {
    /// Every pattern (default: those off squares and hexagons, plus `sample` of those).
    #[serde(default)]
    pub all: bool,
    /// Square and hexagon patterns checked too, the first by id (default 200).
    #[serde(default = "sample_default")]
    pub sample: usize,
    /// Periods run (default: the record's own default).
    #[serde(default)]
    pub periods: Option<u64>,
    /// Delete the patterns that fail.
    #[serde(default)]
    pub drop: bool,
}

/// A pattern that failed.
#[derive(Clone, Debug, Serialize, JsonSchema)]
pub struct Failed {
    pub id: String,
    pub tiling: String,
    pub error: String,
}

/// Checked and failed, on one tiling.
#[derive(Clone, Debug, Serialize, JsonSchema)]
pub struct TilingCount {
    pub checked: usize,
    pub failed: usize,
}

/// What a verification found.
#[derive(Clone, Debug, Serialize, JsonSchema)]
pub struct VerifyReport {
    pub checked: usize,
    /// Of them, square and hexagon patterns.
    pub sampled: usize,
    pub failed: Vec<Failed>,
    pub by_tiling: BTreeMap<String, TilingCount>,
    pub seconds: f64,
    /// Patterns deleted (with `drop`).
    pub removed: usize,
}

impl Report for VerifyReport {
    fn text(&self) -> String {
        let mut s = String::new();
        for (t, c) in &self.by_tiling {
            if c.failed > 0 {
                let _ = writeln!(s, "  {t}: {} of {} failed", c.failed, c.checked);
            }
        }
        for f in self.failed.iter().take(5) {
            let _ = writeln!(s, "  e.g. {} on {}: {}", f.id, f.tiling, f.error);
        }
        let _ = writeln!(
            s,
            "{} patterns verified ({} of them on squares and hexagons) in {:.0} s: {} failed",
            self.checked,
            self.sampled,
            self.seconds,
            self.failed.len()
        );
        if self.removed > 0 {
            let _ = writeln!(s, "removed {} patterns", self.removed);
        }
        s
    }
}

/// `library.verify`.
pub struct LibraryVerify;
impl Op for LibraryVerify {
    const NAME: &'static str = "library.verify";
    const SUMMARY: &'static str = "Verify stored patterns again with this build (e.g. after a geometry change), in pages of 5,000; optionally delete those that fail.";
    const READ_ONLY: bool = false;
    type In = VerifyIn;
    type Out = VerifyReport;
    fn run(cx: &Ctx, a: VerifyIn, p: &dyn Progress) -> Result<VerifyReport, ApiError> {
        use rayon::prelude::*;
        let lib = cx.library();
        let t0 = std::time::Instant::now();
        let (mut checked, mut sampled) = (0usize, 0usize);
        let mut failed: Vec<Failed> = Vec::new();
        let mut by_tiling: BTreeMap<String, TilingCount> = BTreeMap::new();
        let mut offset = 0;
        loop {
            if p.cancelled() {
                break;
            }
            let page = lib
                .query(&Query {
                    order: Some("id".into()),
                    limit: Some(5000),
                    offset: Some(offset),
                    ..Default::default()
                })
                .map_err(catalog_err)?;
            if page.is_empty() {
                break;
            }
            offset += page.len();
            let pick: Vec<PatternRecord> = page
                .into_iter()
                .filter(|r| {
                    let main = matches!(r.tiling.family.as_str(), "square" | "hex");
                    if a.all || !main {
                        true
                    } else if sampled < a.sample {
                        sampled += 1;
                        true
                    } else {
                        false
                    }
                })
                .collect();
            let res: Vec<(String, String, Option<String>)> = pick
                .par_iter()
                .map(|r| {
                    let n = a
                        .periods
                        .unwrap_or_else(|| pattern::default_periods(r.period));
                    let e = pattern::verify(r, n).err();
                    (r.id.clone(), r.tiling_name.clone(), e)
                })
                .collect();
            for (id, t, e) in res {
                checked += 1;
                let x = by_tiling.entry(t.clone()).or_insert(TilingCount {
                    checked: 0,
                    failed: 0,
                });
                x.checked += 1;
                if let Some(e) = e {
                    x.failed += 1;
                    failed.push(Failed {
                        id,
                        tiling: t,
                        error: e,
                    });
                }
            }
            p.report(
                checked as u64,
                None,
                &format!("{checked} checked, {} failed", failed.len()),
            );
        }
        let seconds = t0.elapsed().as_secs_f64();
        let mut removed = 0;
        if a.drop {
            for f in &failed {
                lib.remove(&f.id).map_err(catalog_err)?;
                removed += 1;
            }
        }
        Ok(VerifyReport {
            checked,
            sampled,
            failed,
            by_tiling,
            seconds,
            removed,
        })
    }
}

// ---------------------------------------------------------------------------------------------
// library.reverify

/// Which records to check again, and how.
#[derive(Clone, Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ReverifyIn {
    /// Every record, not only those flagged because their geometry pin (or a pattern's
    /// rule-semantics version) is not this engine's.
    #[serde(default)]
    pub all: bool,
    /// Delete the records that fail (default: keep them flagged and report them).
    #[serde(default)]
    pub drop: bool,
    /// Periods each pattern is run (default: its own default, 4 to 16).
    #[serde(default)]
    pub periods: Option<u64>,
    /// Resume this re-verification job instead of starting one.
    #[serde(default)]
    pub job: Option<i64>,
    /// Worker threads (0 or absent: all cores).
    #[serde(default)]
    pub threads: usize,
    /// Stop after this many minutes, resumable with `job`.
    #[serde(default)]
    pub minutes: Option<f64>,
}

/// What a re-verification did.
#[derive(Clone, Debug, Serialize, JsonSchema)]
pub struct ReverifyReport {
    /// The library file.
    pub catalog: String,
    /// The `reverify` job.
    pub job: i64,
    /// `done`, or `paused` when stopped (resume with `job`).
    pub status: String,
    /// Checked, passed, failed (the first 200, with why), moved to new ids, removed.
    #[schemars(with = "Value")]
    pub progress: jetcarp_search::jobs::reverify::ReverifyProgress,
    /// Records still flagged afterwards, by kind.
    pub flagged: BTreeMap<String, u64>,
    pub seconds: f64,
}

impl Report for ReverifyReport {
    fn text(&self) -> String {
        let p = &self.progress;
        let mut s = String::new();
        for f in p.failures.iter().take(10) {
            let _ = writeln!(s, "  failed {} {}: {}", f.kind, f.id, f.error);
        }
        if p.failures.len() > 10 {
            let _ = writeln!(s, "  ... {} more failures", p.failed - 10);
        }
        for m in p.moved.iter().take(10) {
            let _ = writeln!(
                s,
                "  moved  {} {} -> {} (old id kept as an alias)",
                m.kind, m.old, m.new
            );
        }
        let _ = writeln!(s, "{} ({:.1} s)", p.line, self.seconds);
        let left: u64 = self.flagged.values().sum();
        if left > 0 {
            let _ = writeln!(
                s,
                "{left} records still flagged in {}{}",
                self.catalog,
                if self.status == "paused" {
                    format!(" (resume with --job {})", self.job)
                } else {
                    String::new()
                }
            );
        }
        s
    }
}

/// `library.reverify`.
pub struct LibraryReverify;
impl Op for LibraryReverify {
    const NAME: &'static str = "library.reverify";
    const SUMMARY: &'static str = "Check again the records flagged after a change of geometry or of a rule's meaning (or all), as a resumable job: those that pass get this engine's pin and rule-semantics version and a provenance row, those that fail stay flagged.";
    const READ_ONLY: bool = false;
    type In = ReverifyIn;
    type Out = ReverifyReport;
    fn run(cx: &Ctx, a: ReverifyIn, p: &dyn Progress) -> Result<ReverifyReport, ApiError> {
        use jetcarp_search::jobs::reverify::{ReverifyProgress, ReverifySpec};
        use jetcarp_search::jobs::{self, Budget, JobSpec};
        let lib = cx.library();
        let budget = Budget {
            minutes: a.minutes,
            ..Default::default()
        };
        let job = match a.job {
            Some(j) => {
                let row = lib.job(j).map_err(catalog_err)?;
                if row.kind != "reverify" {
                    return Err(ApiError::invalid(format!(
                        "job {j} is a {} job, not a re-verification",
                        row.kind
                    )));
                }
                j
            }
            None => {
                let spec = ReverifySpec {
                    all: a.all,
                    drop: a.drop,
                    periods: a.periods,
                    ..Default::default()
                };
                let js = JobSpec {
                    strategy: "reverify".into(),
                    spec: serde_json::to_value(spec)
                        .map_err(|e| ApiError::engine(e.to_string()))?,
                    label: None,
                    budget: Budget::default(),
                };
                jobs::create(&lib, &js).map_err(ApiError::invalid)?
            }
        };
        let how = super::search::How {
            threads: a.threads,
            gpu: false,
            budget,
            background: false,
            detach: false,
            files: Default::default(),
        };
        let out = super::search::run_job(cx, &lib, job, &how, p)?;
        let progress: ReverifyProgress =
            serde_json::from_value(out.progress.clone()).unwrap_or_default();
        Ok(ReverifyReport {
            catalog: lib.path().display().to_string(),
            job,
            status: out.status,
            progress,
            flagged: lib.flagged().map_err(catalog_err)?.into_iter().collect(),
            seconds: out.seconds,
        })
    }
}

// ---------------------------------------------------------------------------------------------
// library.compounds

/// Find the stored compounds (several independent objects stored as one pattern) and, with
/// `apply`, store their parts in their place.
#[derive(Clone, Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CompoundsIn {
    /// Change the library: store each compound's parts (credited with its seen count and
    /// sightings, and given its history), then remove the compound, or keep it tagged
    /// `compound` when someone curated it or a part does not run in all its rules. Without it
    /// (the default) nothing changes: the compounds are only listed.
    #[serde(default)]
    pub apply: bool,
    /// The report file, one JSON object per compound (default:
    /// `reports/compounds-<job>.jsonl` beside the library).
    #[serde(default)]
    pub report: Option<String>,
    /// Resume this compounds job instead of starting one.
    #[serde(default)]
    pub job: Option<i64>,
    /// Worker threads (0 or absent: all cores).
    #[serde(default)]
    pub threads: usize,
    /// Stop after this many minutes, resumable with `job`.
    #[serde(default)]
    pub minutes: Option<f64>,
}

/// What a compounds pass found and did.
#[derive(Clone, Debug, Serialize, JsonSchema)]
pub struct CompoundsReport {
    /// The library file.
    pub catalog: String,
    /// The `compounds` job.
    pub job: i64,
    /// `done`, or `paused` when stopped (resume with `job`).
    pub status: String,
    /// Examined, compounds found (by class and tiling), removed or kept, parts, the first 200
    /// compounds with their parts, and the report file.
    #[schemars(with = "Value")]
    pub progress: jetcarp_search::jobs::compounds::CompoundsProgress,
    pub seconds: f64,
}

impl Report for CompoundsReport {
    fn text(&self) -> String {
        let p = &self.progress;
        let mut s = String::new();
        for c in p.examples.iter().take(10) {
            let parts: Vec<String> = c
                .parts
                .iter()
                .map(|x| {
                    format!(
                        "{}{} {}",
                        if x.copies > 1 {
                            format!("{} x ", x.copies)
                        } else {
                            String::new()
                        },
                        &x.id[..8.min(x.id.len())],
                        x.label
                    )
                })
                .collect();
            let _ = writeln!(
                s,
                "  {:<12} {} {} ({}, {}): {}",
                c.action,
                &c.id[..8.min(c.id.len())],
                c.label,
                c.tiling,
                c.rule,
                if parts.is_empty() {
                    c.why.clone()
                } else {
                    parts.join(" + ")
                }
            );
        }
        if p.compounds > 10 {
            let _ = writeln!(s, "  ... {} more in the report", p.compounds - 10);
        }
        if !p.by_class.is_empty() {
            let _ = writeln!(
                s,
                "  by class: {}",
                p.by_class
                    .iter()
                    .map(|(k, v)| format!("{k} {v}"))
                    .collect::<Vec<_>>()
                    .join(", ")
            );
        }
        let _ = writeln!(s, "{} ({:.1} s)", p.line, self.seconds);
        let _ = writeln!(s, "report: {}", p.report);
        if self.status == "paused" {
            let _ = writeln!(s, "paused: resume with --job {}", self.job);
        } else if !p.apply && p.compounds > 0 {
            let _ = writeln!(s, "nothing changed (dry run); --apply splits them");
        }
        s
    }
}

/// `library.compounds`.
pub struct LibraryCompounds;
impl Op for LibraryCompounds {
    const NAME: &'static str = "library.compounds";
    const SUMMARY: &'static str = "Find the stored patterns that are several independent objects (two gliders flying together, an oscillator beside a still life) as a resumable job, writing a report of each with its parts; with apply, store the parts (credited with the compound's sightings) and remove the compound, or keep it tagged when curated.";
    const READ_ONLY: bool = false;
    type In = CompoundsIn;
    type Out = CompoundsReport;
    fn run(cx: &Ctx, a: CompoundsIn, p: &dyn Progress) -> Result<CompoundsReport, ApiError> {
        use jetcarp_search::jobs::compounds::{CompoundsProgress, CompoundsSpec};
        use jetcarp_search::jobs::{self, Budget, JobSpec};
        let lib = cx.library();
        let budget = Budget {
            minutes: a.minutes,
            ..Default::default()
        };
        let job = match a.job {
            Some(j) => {
                let row = lib.job(j).map_err(catalog_err)?;
                if row.kind != "compounds" {
                    return Err(ApiError::invalid(format!(
                        "job {j} is a {} job, not a compounds pass",
                        row.kind
                    )));
                }
                j
            }
            None => {
                let spec = CompoundsSpec {
                    apply: a.apply,
                    report: a.report.clone(),
                    ..Default::default()
                };
                let js = JobSpec {
                    strategy: "compounds".into(),
                    spec: serde_json::to_value(spec)
                        .map_err(|e| ApiError::engine(e.to_string()))?,
                    label: None,
                    budget: Budget::default(),
                };
                jobs::create(&lib, &js).map_err(ApiError::invalid)?
            }
        };
        let how = super::search::How {
            threads: a.threads,
            gpu: false,
            budget,
            background: false,
            detach: false,
            files: Default::default(),
        };
        let out = super::search::run_job(cx, &lib, job, &how, p)?;
        let progress: CompoundsProgress =
            serde_json::from_value(out.progress.clone()).unwrap_or_default();
        Ok(CompoundsReport {
            catalog: lib.path().display().to_string(),
            job,
            status: out.status,
            progress,
            seconds: out.seconds,
        })
    }
}

// ---------------------------------------------------------------------------------------------
// library.prune

fn max_cells_default() -> u64 {
    64
}
fn max_volume_default() -> u64 {
    4000
}
fn min_seen_default() -> u64 {
    2
}

/// The hunt storage policy to apply to what is stored.
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PruneIn {
    /// Delete still lifes and oscillators with more cells than this (default 64)...
    #[serde(default = "max_cells_default")]
    pub max_cells: u64,
    /// ... or period x cells above this (default 4000) ...
    #[serde(default = "max_volume_default")]
    pub max_volume: u64,
    /// ... unless a job saw them this many times (default 2). Curated patterns are kept.
    #[serde(default = "min_seen_default")]
    pub min_seen: u64,
    /// Copy the catalog here first (a consistent copy, made by SQLite).
    #[serde(default)]
    pub backup: Option<String>,
    /// Skip the VACUUM (the file keeps its size until one runs).
    #[serde(default)]
    pub no_vacuum: bool,
}

/// What a prune did.
#[derive(Clone, Debug, Serialize, JsonSchema)]
pub struct PruneReport {
    pub catalog: String,
    pub backup: Option<String>,
    pub backup_bytes: Option<u64>,
    pub backup_seconds: Option<f64>,
    pub deleted: u64,
    pub checked: u64,
    /// Deleted, by class.
    pub by_class: BTreeMap<String, u64>,
    pub unstored_rows: u64,
    pub media_rows: u64,
    pub prune_seconds: f64,
    pub compressed: u64,
    pub bytes_before_compression: u64,
    pub bytes_after_compression: u64,
    pub compress_seconds: f64,
    pub vacuum_seconds: Option<f64>,
    pub bytes_before: u64,
    pub bytes_after: u64,
}

impl Report for PruneReport {
    fn text(&self) -> String {
        let mut s = String::new();
        if let (Some(b), Some(n), Some(t)) = (&self.backup, self.backup_bytes, self.backup_seconds)
        {
            let _ = writeln!(s, "backup: {b} ({:.1} GB, {t:.0} s)", n as f64 / 1e9);
        }
        let _ = writeln!(
            s,
            "pruned {} of {} still lifes and oscillators ({:?}); {} unstored rows, {} media rows; {:.0} s",
            self.deleted,
            self.checked,
            self.by_class,
            self.unstored_rows,
            self.media_rows,
            self.prune_seconds
        );
        let _ = writeln!(
            s,
            "compressed {} bodies: {:.0} MB to {:.0} MB; {:.0} s",
            self.compressed,
            self.bytes_before_compression as f64 / 1e6,
            self.bytes_after_compression as f64 / 1e6,
            self.compress_seconds
        );
        if let Some(t) = self.vacuum_seconds {
            let _ = writeln!(s, "vacuum: {t:.0} s");
        }
        let _ = writeln!(
            s,
            "{}: {:.2} GB before, {:.2} GB after",
            self.catalog,
            self.bytes_before as f64 / 1e9,
            self.bytes_after as f64 / 1e9
        );
        s
    }
}

/// `library.prune`.
pub struct LibraryPrune;
impl Op for LibraryPrune {
    const NAME: &'static str = "library.prune";
    const SUMMARY: &'static str = "Delete the still lifes and oscillators a hunt would no longer store, compress old pattern bodies, and VACUUM.";
    const READ_ONLY: bool = false;
    type In = PruneIn;
    type Out = PruneReport;
    fn run(cx: &Ctx, a: PruneIn, _: &dyn Progress) -> Result<PruneReport, ApiError> {
        let lib = cx.library();
        let size = |p: &std::path::Path| std::fs::metadata(p).map(|m| m.len()).unwrap_or(0);
        let file = lib.path().to_path_buf();
        let bytes_before = size(&file);
        let (mut backup_bytes, mut backup_seconds) = (None, None);
        if let Some(b) = &a.backup {
            let t0 = std::time::Instant::now();
            let path = cx.fs.writable(b)?;
            lib.backup_to(&path).map_err(catalog_err)?;
            backup_bytes = Some(size(&path));
            backup_seconds = Some(t0.elapsed().as_secs_f64());
        }
        let t0 = std::time::Instant::now();
        let r = lib
            .prune_patterns(a.max_cells, a.max_volume, a.min_seen)
            .map_err(catalog_err)?;
        let prune_seconds = t0.elapsed().as_secs_f64();
        let t0 = std::time::Instant::now();
        let (n, before, after) = lib.compact_bodies().map_err(catalog_err)?;
        let compress_seconds = t0.elapsed().as_secs_f64();
        let mut vacuum_seconds = None;
        if !a.no_vacuum {
            let t0 = std::time::Instant::now();
            lib.vacuum().map_err(catalog_err)?;
            vacuum_seconds = Some(t0.elapsed().as_secs_f64());
        }
        Ok(PruneReport {
            catalog: file.display().to_string(),
            backup: a.backup,
            backup_bytes,
            backup_seconds,
            deleted: r.deleted,
            checked: r.checked,
            by_class: r.by_class,
            unstored_rows: r.unstored_rows,
            media_rows: r.media_rows,
            prune_seconds,
            compressed: n,
            bytes_before_compression: before,
            bytes_after_compression: after,
            compress_seconds,
            vacuum_seconds,
            bytes_before,
            bytes_after: size(&file),
        })
    }
}
