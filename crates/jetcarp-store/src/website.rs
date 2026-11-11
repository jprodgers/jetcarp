//! Incremental static export of the catalog for a website (`jetcarp site export --to DIR`).
//!
//! The export folder holds JSON data and media files for a custom, mostly static site:
//!
//! ```text
//! DIR/manifest.json                      the index the site's build reads
//! DIR/patterns/<id>.json                 one per public pattern
//! DIR/quickstart/<id>.json               one per public Quick Start entry
//! DIR/ants/<id>.json                     one per public ant or turmite run (highways first)
//! DIR/media/pattern/<id>/anim.gif|webp   animation (pattern alone)
//! DIR/media/pattern/<id>/poster.png      poster frame
//! DIR/media/pattern/<id>/thumb.png       thumbnail
//! DIR/media/pattern/<id>/frames/0000.png per-step frames (only with `frames`)
//! DIR/media/quickstart/<id>/...          the same for Quick Start entries (from the seed)
//! DIR/media/ant/<id>/...                 the same for ants (a highway's loop alone, other
//!                                        outcomes their build-up from step 0)
//! DIR/index.html                         a viewer page over the manifest ([`PAGE`], written by
//!                                        [`write_page`]; `jetcarp site export` does)
//! ```
//!
//! **Incremental.** The manifest records a content hash (SHA-256) for every item and media
//! file, and it is the export's state: an export reads the old manifest, writes only items
//! whose JSON changed and media files whose hash changed (or that are missing), deletes the
//! files of items that are no longer public, and rewrites the manifest only when something
//! changed (so its `exported` time is the time of the last change). A second export with no
//! changes writes nothing. Removed and rejected items are **deleted** from the export rather
//! than marked: the manifest lists exactly what is public, so a site build never needs to filter.
//!
//! **What is public** ([`ExportFilter`]): patterns whose curation state is `promoted` or
//! `reviewed` (never `rejected` or `new`), optionally above a score or rating and of some
//! classes; Quick Start entries with status `auto` or `approved`; ant and turmite runs whose
//! state is `promoted` or `reviewed` (optionally only some outcomes, such as highways).

use crate::media::{self, MediaRow, MediaTarget};
use crate::{Library, QsStatus};
use jetcarp_core::media::{MediaMode, MediaOptions, content_hash};
use jetcarp_core::pattern::{CurationState, PatternClass, PatternRecord, RuleRange};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

/// Version of the export layout and manifest.
pub const FORMAT: u32 = 1;

/// A plain HTML and JavaScript page over an export: it reads `manifest.json` and shows every
/// item's animation in a grid, with filters and a detail view (`data/site/index.html`).
pub const PAGE: &str = include_str!("../data/site/index.html");

/// Write [`PAGE`] as `DIR/index.html`, unless it is there already unchanged. Returns whether it
/// was written. [`export`] leaves the page alone: it is not part of the manifest.
pub fn write_page(dir: &Path) -> Result<bool, String> {
    let p = dir.join("index.html");
    if std::fs::read_to_string(&p).ok().as_deref() == Some(PAGE) {
        return Ok(false);
    }
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    std::fs::write(&p, PAGE).map_err(|e| format!("{}: {e}", p.display()))?;
    Ok(true)
}

/// Which items are public.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ExportFilter {
    /// Curation states exported (default `promoted` and `reviewed`).
    pub states: Vec<CurationState>,
    pub min_score: Option<f64>,
    pub min_rating: Option<u8>,
    /// Only these classes (empty: all).
    pub classes: Vec<PatternClass>,
    /// Leave out oscillators with a shorter period than this (blinkers and other period-2
    /// oscillators are common in most rules).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_osc_period: Option<u64>,
    /// Only these pattern ids (empty: no limit), e.g. a showcase that has media.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ids: Vec<String>,
    /// Export Quick Start entries with these statuses (default `auto` and `approved`; empty:
    /// none).
    pub quickstart: Vec<QsStatus>,
    /// Export ant and turmite runs in these curation states (default `promoted` and
    /// `reviewed`; empty: none).
    #[serde(default = "public_ant_states")]
    pub ant_states: Vec<String>,
    /// Only ants with these outcomes (`highway`, `symmetric`, …; empty: all).
    #[serde(default)]
    pub ant_outcomes: Vec<String>,
}

fn public_ant_states() -> Vec<String> {
    vec!["promoted".into(), "reviewed".into()]
}

impl Default for ExportFilter {
    fn default() -> Self {
        ExportFilter {
            states: vec![CurationState::Promoted, CurationState::Reviewed],
            min_score: None,
            min_rating: None,
            classes: Vec::new(),
            min_osc_period: None,
            ids: Vec::new(),
            quickstart: vec![QsStatus::Auto, QsStatus::Approved],
            ant_states: public_ant_states(),
            ant_outcomes: Vec::new(),
        }
    }
}

impl ExportFilter {
    pub fn admits(&self, r: &PatternRecord) -> bool {
        r.state != CurationState::Rejected
            && self.states.contains(&r.state)
            && self.min_score.is_none_or(|s| r.score >= s)
            && self
                .min_rating
                .is_none_or(|m| r.rating.is_some_and(|x| x >= m))
            && (self.classes.is_empty() || self.classes.contains(&r.class))
            && (r.class != PatternClass::Oscillator
                || self.min_osc_period.is_none_or(|p| r.period >= p))
            && (self.ids.is_empty() || self.ids.contains(&r.id))
    }
    /// Whether an ant run is public.
    pub fn admits_ant(&self, a: &crate::AntRow) -> bool {
        a.state != "rejected"
            && self.ant_states.contains(&a.state)
            && self
                .min_rating
                .is_none_or(|m| a.rating.is_some_and(|x| x >= m as i64))
            && (self.ant_outcomes.is_empty()
                || self.ant_outcomes.iter().any(|o| o == a.outcome.name()))
    }
}

/// How to export.
#[derive(Clone, Debug)]
pub struct ExportOptions {
    pub filter: ExportFilter,
    /// Copy the per-step PNG frames too (default false: animation, poster, thumbnail).
    pub frames: bool,
    /// Render missing or stale media first (default true); otherwise items without media are
    /// exported without media links.
    pub render: bool,
    pub media: MediaOptions,
}

impl Default for ExportOptions {
    fn default() -> Self {
        ExportOptions {
            filter: ExportFilter::default(),
            frames: false,
            render: true,
            media: MediaOptions::default(),
        }
    }
}

/// What an export did. Paths are relative to the export folder.
#[derive(Clone, Debug, Default, Serialize)]
pub struct ExportReport {
    pub dir: PathBuf,
    pub patterns: usize,
    pub quickstart: usize,
    pub ants: usize,
    /// Files written (items, media, and `manifest.json` last when it changed).
    pub written: Vec<String>,
    pub deleted: Vec<String>,
    /// Items whose JSON and media were already up to date.
    pub unchanged: usize,
    /// Media rendered in the catalog for this export.
    pub rendered: usize,
    /// (kind, id, error) for media that could not be rendered.
    pub media_failed: Vec<(String, String, String)>,
}

/// One media file in the export.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SiteMedia {
    pub path: String,
    /// MIME type (`image/gif`, `image/webp`, `image/png`), or `directory` for frames.
    #[serde(rename = "type")]
    pub mime: String,
    pub width: u32,
    pub height: u32,
    pub frames: u32,
    pub bytes: u64,
    pub hash: String,
    /// `alone` or `seed`.
    pub mode: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delay_ms: Option<u64>,
}

/// An item's links: `anim`, `poster`, `thumb`, and (with `frames`) `frames`.
pub type MediaLinks = BTreeMap<String, SiteMedia>;

/// One entry of `manifest.json`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ManifestItem {
    /// `pattern`, `quickstart`, or `ant`.
    pub kind: String,
    pub id: String,
    /// The item's JSON file.
    pub path: String,
    /// Content hash of the item's JSON (which holds its media hashes).
    pub hash: String,
    pub title: String,
    /// Patterns: the class; Quick Start entries: the behaviour; ants: the outcome.
    pub class: String,
    pub grid: String,
    pub rule: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub score: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rating: Option<u8>,
    pub media: MediaLinks,
}

/// `manifest.json`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Manifest {
    pub format: u32,
    pub generator: String,
    /// UTC time of the last export that changed something.
    pub exported: String,
    pub filter: ExportFilter,
    /// The media house style's version and hash.
    pub style: Value,
    pub counts: BTreeMap<String, usize>,
    pub items: Vec<ManifestItem>,
}

impl Manifest {
    /// Read `DIR/manifest.json` (`None` if missing, unreadable, or another format).
    pub fn read(dir: &Path) -> Option<Manifest> {
        let s = std::fs::read_to_string(dir.join("manifest.json")).ok()?;
        let m: Manifest = serde_json::from_str(&s).ok()?;
        (m.format == FORMAT).then_some(m)
    }
    /// Every file the export holds, with its hash (frame folders expanded to their files, with
    /// the folder's hash).
    fn files(&self) -> BTreeMap<String, String> {
        let mut out = BTreeMap::new();
        for it in &self.items {
            out.insert(it.path.clone(), it.hash.clone());
            for m in it.media.values() {
                if m.mime == "directory" {
                    for k in 0..m.frames {
                        out.insert(format!("{}/{k:04}.png", m.path), m.hash.clone());
                    }
                } else {
                    out.insert(m.path.clone(), m.hash.clone());
                }
            }
        }
        out
    }
}

fn mime(kind: &str) -> &'static str {
    match kind {
        "gif" => "image/gif",
        "webp" => "image/webp",
        "frames" => "directory",
        _ => "image/png",
    }
}

/// The export's media links for a target, and the catalog files to copy (export path, source).
fn links(
    rows: &[MediaRow],
    kind: &str,
    id: &str,
    prefer_seed: bool,
    frames: bool,
) -> (MediaLinks, Vec<(String, PathBuf, SiteMedia)>) {
    let mut out = MediaLinks::new();
    let mut copies = Vec::new();
    // One mode: the preferred one if it has a poster, else the other.
    let has = |seed: bool| {
        rows.iter()
            .any(|r| r.from_seed == seed && r.kind == "poster")
    };
    let seed = if has(prefer_seed) {
        prefer_seed
    } else if has(!prefer_seed) {
        !prefer_seed
    } else {
        return (out, copies);
    };
    let base = format!("media/{kind}/{}", safe(id));
    for r in rows.iter().filter(|r| r.from_seed == seed) {
        let (name, file) = match r.kind.as_str() {
            "gif" | "webp" => ("anim", format!("anim.{}", r.kind)),
            "poster" => ("poster", "poster.png".into()),
            "thumb" => ("thumb", "thumb.png".into()),
            "frames" if frames => ("frames", "frames".into()),
            _ => continue,
        };
        let m = SiteMedia {
            path: format!("{base}/{file}"),
            mime: mime(&r.kind).into(),
            width: r.width,
            height: r.height,
            frames: r.frames,
            bytes: r.bytes,
            hash: r.hash.clone(),
            mode: if seed { "seed" } else { "alone" }.into(),
            delay_ms: (name == "anim")
                .then(|| r.render["delay_ms"].as_u64())
                .flatten(),
        };
        copies.push((m.path.clone(), r.abs_path.clone(), m.clone()));
        out.insert(name.into(), m);
    }
    (out, copies)
}

fn safe(id: &str) -> String {
    id.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

/// A pattern as the website sees it: the rule-free record without curation notes or
/// timestamps that change without the pattern changing.
#[derive(Clone, Debug, Serialize)]
pub struct SitePattern<'a> {
    pub format: u32,
    pub kind: &'static str,
    pub id: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<&'a str>,
    pub label: String,
    pub class: PatternClass,
    pub period: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub speed: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub displacement: Option<[i32; 2]>,
    pub tiling: &'a jetcarp_core::topology::TilingSpec,
    pub tiling_name: &'a str,
    pub tile_types: u32,
    pub neighborhood: jetcarp_core::topology::Nb,
    pub steps_per_gen: u32,
    /// The rule it runs in by default, every rule it was verified in, and its rule range.
    pub rule: &'a str,
    pub rules: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub range: Option<&'a RuleRange>,
    pub cells: usize,
    pub min_cells: usize,
    pub max_cells: usize,
    pub bbox: [i32; 4],
    pub canonical: &'a str,
    /// One period of phases (`i,j,k,s` tuples), phase 0 first.
    pub phases: &'a [String],
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gun: Option<&'a jetcarp_core::pattern::GunInfo>,
    /// Copies found over all searches, and per rule.
    pub seen: u64,
    pub seen_by_rule: BTreeMap<String, u64>,
    pub score: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rating: Option<u8>,
    pub tags: &'a [String],
    pub state: CurationState,
    pub discovered_by: jetcarp_core::pattern::DiscoveredBy,
    /// Date of the first find (`YYYY-MM-DD`).
    pub discovered: &'a str,
    pub verified_periods: u64,
    pub media: &'a MediaLinks,
}

fn pattern_json(lib: &Library, r: &PatternRecord, media: &MediaLinks) -> Result<Vec<u8>, String> {
    let mut seen_by_rule = BTreeMap::new();
    for s in lib.sightings(&r.id)? {
        *seen_by_rule.entry(s.rule).or_insert(0) += s.count;
    }
    let mut rules = vec![r.rule.clone()];
    for x in &r.rules {
        if !rules.contains(x) {
            rules.push(x.clone());
        }
    }
    let p = SitePattern {
        format: FORMAT,
        kind: "pattern",
        id: &r.id,
        name: r.name.as_deref(),
        label: r.label(),
        class: r.class,
        period: r.period,
        speed: r.speed.as_deref(),
        displacement: r.displacement,
        tiling: &r.tiling,
        tiling_name: &r.tiling_name,
        tile_types: r.tile_types,
        neighborhood: r.neighborhood,
        steps_per_gen: r.steps_per_gen,
        rule: &r.rule,
        rules,
        range: r.range.as_ref(),
        cells: r.cells,
        min_cells: r.min_cells,
        max_cells: r.max_cells,
        bbox: r.bbox,
        canonical: &r.canonical,
        phases: &r.phases,
        gun: r.gun.as_ref(),
        seen: r.seen,
        seen_by_rule,
        score: (r.score * 1000.0).round() / 1000.0,
        rating: r.rating,
        tags: &r.tags,
        state: r.state,
        discovered_by: r.discovered_by,
        discovered: r.provenance.date.get(..10).unwrap_or(""),
        verified_periods: r.verified.periods,
        media,
    };
    serde_json::to_vec_pretty(&p).map_err(|e| e.to_string())
}

/// An ant or turmite run as the website sees it (no curation notes or timestamps).
fn ant_json(a: &crate::AntRow, media: &MediaLinks) -> Result<Vec<u8>, String> {
    let v = json!({
        "format": FORMAT,
        "kind": "ant",
        "id": a.id,
        "name": a.name,
        "label": a.line(),
        "rule": a.rule,
        "rule_kind": a.kind,
        "states": a.states,
        "colors": a.colors,
        "tiling": a.tiling,
        "tiling_name": a.tiling_name,
        "start": a.start,
        "outcome": a.outcome.name(),
        "period": a.period,
        "displacement": a.d,
        "speed": a.speed.map(|s| (s * 1e6).round() / 1e6),
        "onset": a.onset,
        "steps": a.steps,
        "cells": a.cells,
        "score": (a.score * 1000.0).round() / 1000.0,
        "rating": a.rating,
        "state": a.state,
        "media": media,
    });
    serde_json::to_vec_pretty(&v).map_err(|e| e.to_string())
}

fn quickstart_json(q: &crate::QuickStartRow, media: &MediaLinks) -> Result<Vec<u8>, String> {
    let mut scene = q.scene.clone();
    scene.jetcarp_version.clear();
    scene.record = None;
    scene.view = None;
    let v = json!({
        "format": FORMAT,
        "kind": "quickstart",
        "id": q.id,
        "title": q.title,
        "description": q.description,
        "source_kind": q.source_kind,
        "source_id": q.source_id,
        "grid": q.grid,
        "rule": q.rule,
        "rule_family": q.rule_family,
        "behaviour": q.behaviour,
        "score": q.score.map(|s| (s * 1000.0).round() / 1000.0),
        "status": q.status,
        "scene": scene,
        "media": media,
    });
    serde_json::to_vec_pretty(&v).map_err(|e| e.to_string())
}

struct Planned {
    item: ManifestItem,
    json: Vec<u8>,
    copies: Vec<(String, PathBuf, SiteMedia)>,
}

/// Export the public part of the catalog to `dir` (see the module documentation).
pub fn export(lib: &Library, dir: &Path, o: &ExportOptions) -> Result<ExportReport, String> {
    let f = &o.filter;
    // With an id list, read just those patterns (a large catalog does not fit in memory at
    // once); otherwise every pattern.
    let mut recs: Vec<PatternRecord> = if f.ids.is_empty() {
        lib.list()?
    } else {
        f.ids.iter().filter_map(|id| lib.get(id).ok()).collect()
    }
    .into_iter()
    .filter(|r| f.admits(r))
    .collect();
    recs.sort_by(|a, b| a.id.cmp(&b.id));
    let mut qs = if f.quickstart.is_empty() {
        Vec::new()
    } else {
        lib.quickstart(&f.quickstart)?
    };
    qs.retain(|q| q.status != QsStatus::Rejected);
    qs.sort_by(|a, b| a.id.cmp(&b.id));
    let mut ants = if f.ant_states.is_empty() {
        Vec::new()
    } else {
        lib.ants(&crate::AntQuery::default())?
    };
    ants.retain(|a| f.admits_ant(a));
    ants.sort_by(|a, b| a.id.cmp(&b.id));
    let mut report = ExportReport {
        dir: dir.to_path_buf(),
        patterns: recs.len(),
        quickstart: qs.len(),
        ants: ants.len(),
        ..Default::default()
    };
    if o.render {
        let mut targets: Vec<MediaTarget> = recs
            .iter()
            .map(|r| MediaTarget::Pattern(r.id.clone()))
            .collect();
        targets.extend(qs.iter().map(|q| MediaTarget::QuickStart(q.id.clone())));
        targets.extend(ants.iter().map(|a| MediaTarget::Ant(a.id.clone())));
        let mo = MediaOptions {
            mode: MediaMode::Auto,
            frames: o.media.frames || o.frames,
            ..o.media.clone()
        };
        let r = media::render_many(lib, &targets, &mo)?;
        report.rendered = r.written();
        report.media_failed = r.failed;
    }
    // Plan every item.
    let mut planned = Vec::new();
    for r in &recs {
        let rows = media::stored(lib, "pattern", &r.id)?;
        let (m, copies) = links(&rows, "pattern", &r.id, false, o.frames);
        let json = pattern_json(lib, r, &m)?;
        planned.push(Planned {
            item: ManifestItem {
                kind: "pattern".into(),
                id: r.id.clone(),
                path: format!("patterns/{}.json", safe(&r.id)),
                hash: content_hash(&json),
                title: r.label(),
                class: r.class.name().into(),
                grid: r.tiling_name.clone(),
                rule: r.rule.clone(),
                score: Some((r.score * 1000.0).round() / 1000.0),
                rating: r.rating,
                media: m,
            },
            json,
            copies,
        });
    }
    for q in &qs {
        let rows = media::stored(lib, "quickstart", &q.id)?;
        let (m, copies) = links(&rows, "quickstart", &q.id, true, o.frames);
        let json = quickstart_json(q, &m)?;
        planned.push(Planned {
            item: ManifestItem {
                kind: "quickstart".into(),
                id: q.id.clone(),
                path: format!("quickstart/{}.json", safe(&q.id)),
                hash: content_hash(&json),
                title: q.title.clone(),
                class: q.behaviour.clone(),
                grid: q.grid.clone(),
                rule: q.rule.clone(),
                score: q.score.map(|s| (s * 1000.0).round() / 1000.0),
                rating: None,
                media: m,
            },
            json,
            copies,
        });
    }
    for a in &ants {
        let rows = media::stored(lib, "ant", &a.id)?;
        let (m, copies) = links(&rows, "ant", &a.id, false, o.frames);
        let json = ant_json(a, &m)?;
        planned.push(Planned {
            item: ManifestItem {
                kind: "ant".into(),
                id: a.id.clone(),
                path: format!("ants/{}.json", safe(&a.id)),
                hash: content_hash(&json),
                title: a.name.clone().unwrap_or_else(|| a.line()),
                class: a.outcome.name().into(),
                grid: a.tiling_name.clone(),
                rule: a.rule.clone(),
                score: Some((a.score * 1000.0).round() / 1000.0),
                rating: a.rating.map(|r| r.clamp(0, 5) as u8),
                media: m,
            },
            json,
            copies,
        });
    }
    let old = Manifest::read(dir);
    let old_files = old.as_ref().map(|m| m.files()).unwrap_or_default();
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let write = |rel: &str, bytes: &[u8]| -> Result<(), String> {
        let p = dir.join(rel);
        if let Some(parent) = p.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
        }
        std::fs::write(&p, bytes).map_err(|e| format!("{}: {e}", p.display()))
    };
    let up_to_date = |rel: &str, hash: &str| {
        old_files.get(rel).is_some_and(|h| h == hash) && dir.join(rel).exists()
    };
    let mut new_files: BTreeSet<String> = BTreeSet::new();
    for p in &planned {
        let mut changed = false;
        new_files.insert(p.item.path.clone());
        if !up_to_date(&p.item.path, &p.item.hash) {
            write(&p.item.path, &p.json)?;
            report.written.push(p.item.path.clone());
            changed = true;
        }
        for (rel, src, m) in &p.copies {
            if m.mime == "directory" {
                let names: Vec<String> = (0..m.frames).map(|k| format!("{k:04}.png")).collect();
                let all_fresh = names
                    .iter()
                    .all(|n| up_to_date(&format!("{rel}/{n}"), &m.hash));
                for n in &names {
                    let r = format!("{rel}/{n}");
                    new_files.insert(r.clone());
                    if !all_fresh {
                        let b = std::fs::read(src.join(n))
                            .map_err(|e| format!("{}: {e}", src.join(n).display()))?;
                        write(&r, &b)?;
                        report.written.push(r);
                        changed = true;
                    }
                }
            } else {
                new_files.insert(rel.clone());
                if !up_to_date(rel, &m.hash) {
                    let b = std::fs::read(src).map_err(|e| format!("{}: {e}", src.display()))?;
                    write(rel, &b)?;
                    report.written.push(rel.clone());
                    changed = true;
                }
            }
        }
        if !changed {
            report.unchanged += 1;
        }
    }
    // Delete what is no longer public.
    for rel in old_files.keys() {
        if !new_files.contains(rel) {
            let p = dir.join(rel);
            if p.exists() {
                std::fs::remove_file(&p).map_err(|e| format!("{}: {e}", p.display()))?;
            }
            report.deleted.push(rel.clone());
            // Remove folders left empty, up to the export folder.
            let mut d = p.parent().map(Path::to_path_buf);
            while let Some(x) = d {
                if x == dir || std::fs::remove_dir(&x).is_err() {
                    break;
                }
                d = x.parent().map(Path::to_path_buf);
            }
        }
    }
    let items: Vec<ManifestItem> = planned.into_iter().map(|p| p.item).collect();
    let mut counts = BTreeMap::new();
    counts.insert("patterns".to_string(), report.patterns);
    counts.insert("quickstart".to_string(), report.quickstart);
    counts.insert("ants".to_string(), report.ants);
    let style = json!({"version": o.media.style.version, "hash": o.media.style.hash()});
    let same_index = old.as_ref().is_some_and(|m| {
        m.items == items && m.filter == *f && m.style == style && m.counts == counts
    });
    if !same_index || !report.written.is_empty() || !report.deleted.is_empty() {
        let m = Manifest {
            format: FORMAT,
            generator: format!("JETCARP {}", jetcarp_core::VERSION),
            exported: jetcarp_core::pattern::now_utc(),
            filter: f.clone(),
            style,
            counts,
            items,
        };
        let bytes = serde_json::to_vec_pretty(&m).map_err(|e| e.to_string())?;
        write("manifest.json", &bytes)?;
        report.written.push("manifest.json".into());
    }
    Ok(report)
}
