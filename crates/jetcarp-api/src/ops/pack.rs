//! `pack.*`: packs (format 1.0, the book's *Pack format* reference): the publisher's key
//! (`pack.key`), raters' consent (`pack.rater`), publishing the library into a store
//! (`pack.publish`), exporting a version for a single download (`pack.export`), checking a store
//! or export without importing it (`pack.check`), a version's summary (`pack.info`) and
//! importing one (`pack.import`).
//!
//! The publisher's key and settings live in the settings folder (`<config>/pack/`): the Ed25519
//! key in OpenSSH format (`publisher_ed25519`), who the publisher is (`publisher.json`, names
//! only if chosen), and the rater book (`raters.json`: consent and pseudonymous ids). JETCARP
//! never reads `~/.ssh` unless given a key's path.

use super::library::catalog_err;
use crate::ctx::Ctx;
use crate::error::ApiError;
use crate::op::{Op, Progress, Report};
use jetcarp_pack::import::{self as pimport, VerifyMode};
use jetcarp_pack::publish::{self as ppub, Publisher, RaterBook, Selection};
use jetcarp_pack::read::Source;
use jetcarp_pack::sign;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

/// Who the publisher is, as `publisher.json` keeps it (empty: not named).
#[derive(Clone, Debug, Default, Serialize, Deserialize, JsonSchema)]
pub struct PublisherInfo {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub orcid: String,
    #[serde(default)]
    pub github: String,
}

fn publisher_file() -> Result<PathBuf, ApiError> {
    Ok(sign::key_dir()
        .map_err(ApiError::io)?
        .join("publisher.json"))
}

fn load_info() -> Result<PublisherInfo, ApiError> {
    let p = publisher_file()?;
    if !p.exists() {
        let env = |v: &str| {
            std::env::var(v)
                .map(|x| x.trim().to_string())
                .unwrap_or_default()
        };
        return Ok(PublisherInfo {
            name: env("JETCARP_FINDER"),
            orcid: env("JETCARP_ORCID"),
            github: String::new(),
        });
    }
    let b = std::fs::read(&p).map_err(|e| ApiError::io(format!("{}: {e}", p.display())))?;
    serde_json::from_slice(&b).map_err(|e| ApiError::invalid(format!("{}: {e}", p.display())))
}

/// The publisher's name, when one is set (`pack.key` with a name, else `JETCARP_FINDER`).
pub fn publisher_name() -> Option<String> {
    load_info()
        .ok()
        .map(|i| i.name.trim().to_string())
        .filter(|n| !n.is_empty())
}

fn save_info(i: &PublisherInfo) -> Result<(), ApiError> {
    let p = publisher_file()?;
    let text = serde_json::to_string_pretty(i).map_err(|e| ApiError::io(e.to_string()))?;
    jetcarp_pack::store::write_atomic(&p, format!("{text}\n").as_bytes()).map_err(ApiError::io)
}

/// The publisher: the key (made on first use) and who they are.
fn publisher(key: Option<&str>, cx: &Ctx) -> Result<(Publisher, bool), ApiError> {
    let path = key.map(|k| cx.fs.readable(k)).transpose()?;
    let (signer, made) = sign::load_or_create(path.as_deref(), true).map_err(ApiError::io)?;
    let info = load_info()?;
    Ok((
        Publisher {
            signer,
            name: info.name,
            orcid: info.orcid,
            github: info.github,
        },
        made,
    ))
}

/// The publisher's key file in the settings folder, and whether it has been made (`pack.key`
/// makes it; `pack.publish` makes it on first use). Front ends that must not make one (the MCP
/// server) check this first.
pub fn publisher_key() -> Result<(PathBuf, bool), ApiError> {
    let p = sign::key_file().map_err(ApiError::io)?;
    let made = p.is_file();
    Ok((p, made))
}

fn book_path() -> Result<PathBuf, ApiError> {
    RaterBook::path().map_err(ApiError::io)
}

// ---------------------------------------------------------------------------------------------
// pack.key

/// Make or show the publisher's key, and say who the publisher is.
#[derive(Clone, Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct KeyIn {
    /// The publisher's name, shown in key statements and manifests (empty clears it).
    #[serde(default)]
    pub name: Option<String>,
    /// The publisher's ORCID iD.
    #[serde(default)]
    pub orcid: Option<String>,
    /// The publisher's GitHub user (where others can check the key).
    #[serde(default)]
    pub github: Option<String>,
    /// An OpenSSH Ed25519 private key file to use instead of the one in the settings folder.
    #[serde(default)]
    pub key: Option<String>,
}

/// The publisher's key.
#[derive(Clone, Debug, Serialize, JsonSchema)]
pub struct KeyOut {
    /// `SHA256:...`, as `ssh-keygen -l` prints it.
    pub fingerprint: String,
    /// The public key, `ssh-ed25519 AAAA...`.
    pub public_key: String,
    /// The key file.
    pub path: String,
    /// It was made now.
    pub made: bool,
    pub publisher: PublisherInfo,
}

impl Report for KeyOut {
    fn text(&self) -> String {
        let mut s = format!(
            "{}publisher key {}\n  {}\n  file {}\n",
            if self.made { "made " } else { "" },
            self.fingerprint,
            self.public_key,
            self.path
        );
        let p = &self.publisher;
        if p.name.is_empty() {
            s.push_str("  not named (packs show the key only; --name to be named)\n");
        } else {
            let _ = writeln!(
                s,
                "  named {}{}{}",
                p.name,
                if p.orcid.is_empty() {
                    String::new()
                } else {
                    format!(", ORCID {}", p.orcid)
                },
                if p.github.is_empty() {
                    String::new()
                } else {
                    format!(", GitHub {}", p.github)
                }
            );
        }
        s
    }
}

/// `pack.key`.
pub struct PackKey;
impl Op for PackKey {
    const NAME: &'static str = "pack.key";
    const SUMMARY: &'static str =
        "Make or show the publisher's Ed25519 signing key, and say who the publisher is.";
    const READ_ONLY: bool = false;
    type In = KeyIn;
    type Out = KeyOut;
    fn run(cx: &Ctx, a: KeyIn, _: &dyn Progress) -> Result<KeyOut, ApiError> {
        let (p, made) = publisher(a.key.as_deref(), cx)?;
        let mut info = load_info()?;
        let mut changed = !publisher_file()?.exists();
        for (field, v) in [
            (&mut info.name, a.name),
            (&mut info.orcid, a.orcid),
            (&mut info.github, a.github),
        ] {
            if let Some(v) = v {
                *field = v.trim().to_string();
                changed = true;
            }
        }
        if changed {
            save_info(&info)?;
        }
        let path = match &a.key {
            Some(k) => cx.fs.readable(k)?,
            None => sign::key_file().map_err(ApiError::io)?,
        };
        Ok(KeyOut {
            fingerprint: p.signer.fingerprint().to_string(),
            public_key: p.signer.public_openssh(),
            path: path.display().to_string(),
            made,
            publisher: info,
        })
    }
}

// ---------------------------------------------------------------------------------------------
// pack.rater

/// Record a rater's consent to permanent public release of their ratings.
#[derive(Clone, Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RaterIn {
    /// The rater's name as the library's ratings write it (`Emily Teich`).
    pub name: String,
    /// They consent (false withdraws consent: their ratings are left out of later packs).
    #[serde(default = "yes")]
    pub consent: bool,
    /// Packs may show their name (else only a pseudonymous id).
    #[serde(default)]
    pub show_name: bool,
    /// Their ORCID iD, shown with their name.
    #[serde(default)]
    pub orcid: String,
    /// Their notes may be published with their ratings.
    #[serde(default)]
    pub notes: bool,
}

fn yes() -> bool {
    true
}

/// The rater book after the change.
#[derive(Clone, Debug, Serialize, JsonSchema)]
pub struct RaterOut {
    pub raters: Vec<RaterLine>,
}

#[derive(Clone, Debug, Serialize, JsonSchema)]
pub struct RaterLine {
    pub name: String,
    pub id: String,
    /// Empty: no consent.
    pub consent_date: String,
    pub show_name: bool,
    pub notes: bool,
}

impl Report for RaterOut {
    fn text(&self) -> String {
        let mut s = String::new();
        for r in &self.raters {
            let _ = writeln!(
                s,
                "{:<40} {}  {}{}{}",
                r.name,
                r.id,
                if r.consent_date.is_empty() {
                    "no consent".to_string()
                } else {
                    format!("consented {}", r.consent_date)
                },
                if r.show_name { ", named" } else { "" },
                if r.notes { ", notes" } else { "" }
            );
        }
        s
    }
}

/// `pack.rater`.
pub struct PackRater;
impl Op for PackRater {
    const NAME: &'static str = "pack.rater";
    const SUMMARY: &'static str =
        "Record whether a rater consents to their ratings being published in packs, for good.";
    const READ_ONLY: bool = false;
    type In = RaterIn;
    type Out = RaterOut;
    fn run(_: &Ctx, a: RaterIn, _: &dyn Progress) -> Result<RaterOut, ApiError> {
        let name = a.name.trim();
        if name.is_empty() || name.contains(" & ") || name == "user" || name.starts_with("model:") {
            return Err(ApiError::invalid(
                "a rater is one person's name (a group's consent is its members')",
            ));
        }
        let p = book_path()?;
        let mut b = RaterBook::load(&p).map_err(ApiError::io)?;
        let date = if a.consent {
            jetcarp_core::pattern::now_utc()
        } else {
            String::new()
        };
        b.consent(name, &date, a.show_name, a.orcid.trim(), a.notes);
        b.save(&p).map_err(ApiError::io)?;
        Ok(RaterOut {
            raters: b
                .raters
                .iter()
                .map(|r| RaterLine {
                    name: r.name.clone(),
                    id: r.id.clone(),
                    consent_date: r.consent_date.clone(),
                    show_name: r.show_name,
                    notes: r.notes,
                })
                .collect(),
        })
    }
}

// ---------------------------------------------------------------------------------------------
// pack.publish

/// What to publish, and where.
#[derive(Clone, Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PublishIn {
    /// The feed: letters, digits and `-` (`jetcarp-starter`).
    pub feed: String,
    /// The store's folder (made if missing; a new version of the feed if it has one).
    pub to: String,
    /// The pack's title, for people.
    #[serde(default)]
    pub title: String,
    /// What the pack holds, for people.
    #[serde(default)]
    pub description: String,
    /// Free text for people (`2026.11`).
    #[serde(default)]
    pub version_label: String,
    /// An SPDX identifier (default CC0-1.0; JETCARP's own feeds take only CC0-1.0).
    #[serde(default)]
    pub licence: Option<String>,
    /// The version's time, RFC 3339 UTC (default: now), for reproducible packs.
    #[serde(default)]
    pub created: Option<String>,
    /// Only these records (pattern, ant and Quick Start ids or prefixes, or names).
    #[serde(default)]
    pub ids: Vec<String>,
    /// Only the patterns a library query finds (the `library.query` filters, as JSON).
    #[serde(default)]
    pub query: Option<Value>,
    /// Only these explore searches (uuids). Default: every one with a folder, or none when records
    /// are chosen with `ids` or `query`.
    #[serde(default)]
    pub searches: Option<Vec<String>>,
    /// Leave out search runs.
    #[serde(default)]
    pub no_searches: bool,
    /// Leave out pictures and animations.
    #[serde(default)]
    pub no_media: bool,
    /// Leave out ratings (otherwise only those of raters who consented go in).
    #[serde(default)]
    pub no_ratings: bool,
    /// Leave out ant and turmite runs.
    #[serde(default)]
    pub no_ants: bool,
    /// Leave out Quick Start entries.
    #[serde(default)]
    pub no_quickstart: bool,
    /// Withdraw records: `pattern:<id>=reason` (also `ant:` and `quickstart:`).
    #[serde(default)]
    pub retract: Vec<String>,
    /// Merge other versions: a store's folder, or `<folder>@sha256:<manifest>`.
    #[serde(default)]
    pub merge: Vec<String>,
    /// Write views.sql (DuckDB views over the store).
    #[serde(default)]
    pub views: bool,
    /// The signing key file (default: the settings folder's, made on first use).
    #[serde(default)]
    pub key: Option<String>,
}

/// A published version.
#[derive(Clone, Debug, Serialize, JsonSchema)]
pub struct PublishOut {
    pub store: String,
    pub feed: String,
    pub manifest: String,
    pub seq: u64,
    pub publisher: String,
    /// The whole report (tables with rows added and parts listed, searches, notes).
    pub report: Value,
    pub notes: Vec<String>,
}

impl Report for PublishOut {
    fn text(&self) -> String {
        let mut s = format!(
            "published {} version {} into {}\n  manifest {}\n  publisher {}\n",
            self.feed, self.seq, self.store, self.manifest, self.publisher
        );
        if let Some(t) = self.report["tables"].as_object() {
            for (k, v) in t {
                let (rows, parts) = (v[0].as_u64().unwrap_or(0), v[1].as_u64().unwrap_or(0));
                let _ = writeln!(
                    s,
                    "  {k:<14} {:>9} added, {}",
                    crate::out::count(rows, "row", "rows"),
                    crate::out::count(parts, "part", "parts")
                );
            }
        }
        if let Some(b) = self.report["bytes"].as_u64() {
            let _ = writeln!(s, "  {b} bytes in all");
        }
        s
    }
    fn notes(&self) -> String {
        self.notes.iter().map(|n| format!("{n}\n")).collect()
    }
}

/// A Quick Start entry (not a built-in one, which packs never carry) by the start of its handle
/// or id, as patterns and ant runs are found by the start of theirs.
fn quickstart_by_prefix(lib: &jetcarp_store::Library, k: &str) -> Result<String, ApiError> {
    let rows = lib.quickstart(&[]).map_err(catalog_err)?;
    let found: Vec<_> = rows
        .iter()
        .filter(|r| r.source != jetcarp_store::QsSource::Builtin)
        .filter(|r| r.handle.starts_with(k) || r.id.starts_with(k))
        .collect();
    match found.as_slice() {
        [one] => Ok(one.id.clone()),
        [] => Err(ApiError::not_found(format!("no record '{k}'"))),
        many => Err(ApiError::invalid(format!(
            "'{k}' starts {} Quick Start entries ({}, ...): give more of it",
            many.len(),
            many.iter()
                .take(3)
                .map(|r| r.handle.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ))),
    }
}

fn selection(cx: &Ctx, a: &PublishIn) -> Result<Selection, ApiError> {
    let lib = cx.library();
    let mut ids: Option<std::collections::HashSet<String>> = None;
    if let Some(q) = &a.query {
        let q: jetcarp_store::Query = serde_json::from_value(q.clone())
            .map_err(|e| ApiError::invalid(format!("query: {e}")))?;
        let found = lib.query(&q).map_err(catalog_err)?;
        ids.get_or_insert_with(Default::default)
            .extend(found.into_iter().map(|r| r.id));
    }
    for k in &a.ids {
        let id = match lib
            .get(k)
            .map(|r| r.id)
            .or_else(|_| lib.ant(k).map(|r| r.id))
            .or_else(|_| lib.get_quickstart(k).map(|r| r.id))
        {
            Ok(id) => id,
            Err(_) => quickstart_by_prefix(&lib, k)?,
        };
        ids.get_or_insert_with(Default::default).insert(id);
    }
    // A selection of records (`--id`, `--query`) takes searches only when they are named with
    // `--search`: sharing a few finds should not ship every explore search in the library.
    let selective = ids.is_some();
    Ok(Selection {
        ids,
        searches: if a.no_searches {
            Some(vec![])
        } else if a.searches.is_some() {
            a.searches.clone()
        } else if selective {
            Some(vec![])
        } else {
            None
        },
        ants: !a.no_ants,
        quickstart: !a.no_quickstart,
        media: !a.no_media,
        ratings: !a.no_ratings,
    })
}

fn options(cx: &Ctx, a: &PublishIn, to: PathBuf) -> Result<ppub::Options, ApiError> {
    let mut retract = vec![];
    for r in &a.retract {
        let (t, why) = r.split_once('=').unwrap_or((r.as_str(), ""));
        retract.push((t.trim().to_string(), why.trim().to_string()));
    }
    let mut merge = vec![];
    for m in &a.merge {
        let (p, h) = match m.split_once("@sha256:") {
            Some((p, h)) => (p, Some(format!("sha256:{h}"))),
            None => (m.as_str(), None),
        };
        merge.push((cx.fs.readable(p)?, h));
    }
    Ok(ppub::Options {
        feed: a.feed.clone(),
        to,
        title: a.title.clone(),
        description: a.description.clone(),
        version_label: a.version_label.clone(),
        licence: a.licence.clone().unwrap_or_else(|| "CC0-1.0".into()),
        created: a.created.clone(),
        selection: selection(cx, a)?,
        retract,
        merge,
        views: a.views,
        ..Default::default()
    })
}

fn do_publish(
    cx: &Ctx,
    a: &PublishIn,
    to: PathBuf,
    p: &dyn Progress,
) -> Result<ppub::Published, ApiError> {
    let (pubr, made) = publisher(a.key.as_deref(), cx)?;
    if made {
        p.log(&format!(
            "made a publisher key, {}",
            pubr.signer.fingerprint()
        ));
    }
    let bp = book_path()?;
    let mut book = RaterBook::load(&bp).map_err(ApiError::io)?;
    let before = book.clone();
    let o = options(cx, a, to)?;
    let out = ppub::publish(&cx.library(), &pubr, &mut book, &o, &|l| p.log(l))
        .map_err(ApiError::invalid)?;
    if book != before {
        book.save(&bp).map_err(ApiError::io)?;
    }
    Ok(out)
}

/// `pack.publish`.
pub struct PackPublish;
impl Op for PackPublish {
    const NAME: &'static str = "pack.publish";
    const SUMMARY: &'static str =
        "Publish the library (or a selection) as a new signed version of a feed in a pack store.";
    const READ_ONLY: bool = false;
    type In = PublishIn;
    type Out = PublishOut;
    fn run(cx: &Ctx, a: PublishIn, p: &dyn Progress) -> Result<PublishOut, ApiError> {
        let to = cx.fs.writable_dir(&a.to)?;
        let out = do_publish(cx, &a, to, p)?;
        let (pubr, _) = publisher(a.key.as_deref(), cx)?;
        Ok(PublishOut {
            store: out.store.clone(),
            feed: out.feed.clone(),
            manifest: out.manifest.clone(),
            seq: out.seq,
            publisher: pubr.signer.fingerprint().to_string(),
            notes: out.notes.clone(),
            report: serde_json::to_value(&out).unwrap_or_default(),
        })
    }
}

// ---------------------------------------------------------------------------------------------
// pack.export

/// Export one version with readable file names (for a single download).
#[derive(Clone, Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ExportIn {
    /// The export's folder (conventionally `<feed>-<version label>`).
    pub to: String,
    /// Export a version of this store (default: publish the library first, as `pack.publish`
    /// would, into a scratch store beside the export).
    #[serde(default)]
    pub from: Option<String>,
    /// The version (default: the newest of the store's feed).
    #[serde(default)]
    pub manifest: Option<String>,
    /// Without `from`: what to publish (the `pack.publish` options; its `to` is ignored).
    #[serde(default)]
    pub publish: Option<PublishIn>,
}

/// An export.
#[derive(Clone, Debug, Serialize, JsonSchema)]
pub struct ExportOut {
    pub to: String,
    pub manifest: String,
    pub files: u64,
    pub bytes: u64,
    /// Exported straight from the library: the store it was published into first, kept beside
    /// the export (`.<name>.store`), into which later versions are published.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub store: Option<String>,
}

impl Report for ExportOut {
    fn text(&self) -> String {
        let mut s = format!(
            "exported {} into {}: {}, {}\n",
            self.manifest,
            self.to,
            crate::out::count(self.files, "file", "files"),
            crate::out::count(self.bytes, "byte", "bytes")
        );
        if let Some(st) = &self.store {
            let _ = writeln!(
                s,
                "the version was published first into {st}, a hidden store kept beside the export: \
                 the same export again publishes the feed's next version into it, as `jetcarp pack \
                 publish --to {st}` does (then `jetcarp pack export --from {st}` lays it out)"
            );
        }
        s
    }
}

/// `pack.export`.
pub struct PackExport;
impl Op for PackExport {
    const NAME: &'static str = "pack.export";
    const SUMMARY: &'static str =
        "Export one version of a pack with readable file names, for a single download or a tar.";
    const READ_ONLY: bool = false;
    type In = ExportIn;
    type Out = ExportOut;
    fn run(cx: &Ctx, a: ExportIn, p: &dyn Progress) -> Result<ExportOut, ApiError> {
        let to = cx.fs.writable_dir(&a.to)?;
        let (src_dir, scratch) = match &a.from {
            Some(f) => (cx.fs.readable(f)?, None),
            None => {
                let pi = a.publish.clone().ok_or_else(|| {
                    ApiError::invalid("give a store to export from, or what to publish")
                })?;
                let name = to
                    .file_name()
                    .map(|s| s.to_string_lossy().to_string())
                    .unwrap_or_default();
                // The store the library is published into first, kept (hidden, beside the
                // export) so the feed's later versions are published into it.
                let scratch = to.with_file_name(format!(".{name}.store"));
                do_publish(cx, &pi, scratch.clone(), p)?;
                (scratch.clone(), Some(scratch))
            }
        };
        let r = (|| -> Result<ExportOut, ApiError> {
            let src = Source::open(&src_dir).map_err(ApiError::invalid)?;
            let h = match &a.manifest {
                Some(h) => h.clone(),
                None => src.default_manifest(None).map_err(ApiError::invalid)?,
            };
            let v = src.version(&h).map_err(ApiError::invalid)?;
            let e = jetcarp_pack::export::export(&src, &v, &to).map_err(ApiError::io)?;
            Ok(ExportOut {
                to: e.to,
                manifest: e.manifest,
                files: e.files,
                bytes: e.bytes,
                store: scratch.as_ref().map(|s| s.display().to_string()),
            })
        })();
        r
    }
}

// ---------------------------------------------------------------------------------------------
// pack.import and pack.check

/// A store or an export to import (or check).
#[derive(Clone, Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ImportIn {
    /// The store's or export's folder.
    pub path: String,
    /// The feed, when the store has several.
    #[serde(default)]
    pub feed: Option<String>,
    /// A version other than the feed's newest (`sha256:...`).
    #[serde(default)]
    pub manifest: Option<String>,
    /// Import a version that its publisher did not sign.
    #[serde(default)]
    pub allow_unsigned: bool,
    /// Refuse two different rows for one record id.
    #[serde(default)]
    pub strict: bool,
    /// `sample` (default), `full` or `none`.
    #[serde(default)]
    pub verify: Option<String>,
    /// How many new patterns to verify again with `sample` (default 3000).
    #[serde(default)]
    pub sample: Option<u64>,
    /// The sample's seed (default: from the manifest hash).
    #[serde(
        default,
        serialize_with = "jetcarp_core::json_int::uint_opt::serialize",
        deserialize_with = "jetcarp_core::json_int::uint_opt::deserialize"
    )]
    #[schemars(schema_with = "crate::op::uint_opt_schema")]
    pub seed: Option<u64>,
    /// Most seconds verification may take (default 600).
    #[serde(default)]
    pub verify_seconds: Option<f64>,
}

/// What an import (or a check) found.
#[derive(Clone, Debug, Serialize, JsonSchema)]
pub struct ImportOut {
    pub manifest: String,
    pub feed: String,
    pub publisher: String,
    pub publisher_name: String,
    /// `publisher` (signed by its publisher) or `unsigned`.
    pub signed: String,
    pub dry_run: bool,
    /// The whole report (counts, the verification sample, notes).
    pub report: Value,
    pub notes: Vec<String>,
}

impl Report for ImportOut {
    fn text(&self) -> String {
        let r = &self.report;
        let mut s = format!(
            "{} {} (feed {}, {}{}, {})\n",
            if self.dry_run { "checked" } else { "imported" },
            self.manifest,
            self.feed,
            self.publisher,
            if self.publisher_name.is_empty() {
                String::new()
            } else {
                format!(" {}", self.publisher_name)
            },
            if self.signed == "publisher" {
                "signed by its publisher"
            } else {
                "not signed"
            }
        );
        let _ = writeln!(
            s,
            "  {} objects checked ({} bytes)",
            r["objects_checked"], r["bytes_checked"]
        );
        if !self.dry_run {
            let _ = writeln!(
                s,
                "  patterns: {} added, {} merged, {} flagged needs_reverify",
                r["patterns_added"], r["patterns_merged"], r["flagged"]
            );
            let _ = writeln!(
                s,
                "  sightings {}, ratings {}, ants {}, Quick Start {}, media {}, searches {}, retracted {}",
                r["sightings"],
                r["ratings"],
                r["ants"],
                r["quickstart"],
                r["media"],
                r["searches"].as_array().map(|a| a.len()).unwrap_or(0),
                r["retracted"]
            );
            if let Some(g) = r["superseded"].as_u64().filter(|g| *g > 0) {
                let _ = writeln!(
                    s,
                    "  {} from earlier versions of this pack removed from the Quick Start",
                    crate::out::count(g, "entry", "entries")
                );
            }
            let smp = &r["sample"];
            let _ = writeln!(
                s,
                "  verified again ({}): {} of {} passed, {} skipped, {} failed",
                smp["mode"].as_str().unwrap_or(""),
                smp["passed"],
                smp["checked"],
                smp["skipped"],
                smp["failures"].as_array().map(|a| a.len()).unwrap_or(0)
            );
        }
        s
    }
    fn notes(&self) -> String {
        self.notes.iter().map(|n| format!("{n}\n")).collect()
    }
    fn check(&self) -> Result<(), ApiError> {
        let f = self.report["sample"]["failures"]
            .as_array()
            .map(|a| a.len())
            .unwrap_or(0);
        if f > 0 {
            return Err(ApiError::engine(format!(
                "{f} records of the sample failed to verify again"
            )));
        }
        Ok(())
    }
}

fn import_opts(cx: &Ctx, a: &ImportIn, dry_run: bool) -> Result<pimport::Options, ApiError> {
    let verify = match a.verify.as_deref().unwrap_or("sample") {
        "sample" => VerifyMode::Sample(a.sample.unwrap_or(3000)),
        "full" => VerifyMode::Full,
        "none" => VerifyMode::None,
        x => {
            return Err(ApiError::invalid(format!(
                "verify '{x}': sample, full or none"
            )));
        }
    };
    Ok(pimport::Options {
        path: cx.fs.readable(&a.path)?,
        feed: a.feed.clone(),
        manifest: a.manifest.clone(),
        allow_unsigned: a.allow_unsigned,
        strict: a.strict,
        verify,
        seed: a.seed,
        verify_seconds: a.verify_seconds.unwrap_or(600.0),
        dry_run,
    })
}

fn import_out(r: pimport::Imported) -> ImportOut {
    ImportOut {
        manifest: r.manifest.clone(),
        feed: r.feed.clone(),
        publisher: r.publisher.clone(),
        publisher_name: r.publisher_name.clone(),
        signed: r.signed.clone(),
        dry_run: r.dry_run,
        notes: r.notes.clone(),
        report: serde_json::to_value(&r).unwrap_or_default(),
    }
}

fn exists(p: &Path) -> Result<(), ApiError> {
    if p.exists() {
        Ok(())
    } else {
        Err(ApiError::not_found(format!(
            "{} does not exist",
            p.display()
        )))
    }
}

/// `pack.import`.
pub struct PackImport;
impl Op for PackImport {
    const NAME: &'static str = "pack.import";
    const SUMMARY: &'static str =
        "Import a pack store or export into the library, checking every hash, id and signature.";
    const READ_ONLY: bool = false;
    type In = ImportIn;
    type Out = ImportOut;
    fn run(cx: &Ctx, a: ImportIn, p: &dyn Progress) -> Result<ImportOut, ApiError> {
        let o = import_opts(cx, &a, false)?;
        exists(&o.path)?;
        let r = pimport::import(&cx.library(), &o, &|l| p.log(l)).map_err(ApiError::invalid)?;
        Ok(import_out(r))
    }
}

// ---------------------------------------------------------------------------------------------
// pack.info

/// A store or an export to describe.
#[derive(Clone, Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct InfoIn {
    /// The store's or export's folder.
    pub path: String,
    /// The feed, when the store has several.
    #[serde(default)]
    pub feed: Option<String>,
    /// A version other than the feed's newest (`sha256:...`).
    #[serde(default)]
    pub manifest: Option<String>,
}

/// One table of a version.
#[derive(Clone, Debug, Serialize, JsonSchema)]
pub struct TableInfo {
    pub rows: u64,
    pub parts: usize,
    pub bytes: u64,
}

/// A version's manifest, summarised.
#[derive(Clone, Debug, Serialize, JsonSchema)]
pub struct InfoOut {
    /// `store` or `export`.
    pub kind: String,
    /// A store's feeds (an export has none).
    pub feeds: Vec<String>,
    pub manifest: String,
    pub feed: String,
    pub title: String,
    pub description: String,
    pub version_label: String,
    pub created: String,
    pub licence: String,
    /// The publisher's id (the first key of its chain of statements).
    pub publisher: String,
    /// Who the publisher's key statement names, if anyone.
    pub publisher_name: String,
    /// A signature by the publisher's key verifies.
    pub signed: bool,
    /// Signatures that did not verify, and why.
    pub bad_signatures: Vec<String>,
    /// Rows, parts and bytes of each table.
    pub tables: std::collections::BTreeMap<String, TableInfo>,
    /// The explore searches it carries (uuids).
    pub searches: Vec<String>,
    /// Records it withdraws.
    pub retracts: usize,
    /// The versions it follows.
    pub parents: Vec<String>,
    /// The manifest's own counts.
    pub stats: std::collections::BTreeMap<String, u64>,
}

impl Report for InfoOut {
    fn text(&self) -> String {
        let mut s = format!(
            "{} {} (feed {}{})\n",
            self.kind,
            self.manifest,
            self.feed,
            if self.version_label.is_empty() {
                String::new()
            } else {
                format!(", version {}", self.version_label)
            }
        );
        if !self.title.is_empty() {
            let _ = writeln!(s, "  {}", self.title);
        }
        if !self.description.is_empty() {
            let _ = writeln!(s, "  {}", self.description);
        }
        let _ = writeln!(
            s,
            "  publisher {}{}, {}",
            self.publisher,
            if self.publisher_name.is_empty() {
                String::new()
            } else {
                format!(" ({})", self.publisher_name)
            },
            if self.signed {
                "signed by its publisher"
            } else {
                "not signed by its publisher"
            }
        );
        let _ = writeln!(s, "  created {}, licence {}", self.created, self.licence);
        for (k, t) in &self.tables {
            let _ = writeln!(
                s,
                "  {k:<14} {:>14} in {}, {} bytes",
                crate::out::count(t.rows, "row", "rows"),
                crate::out::count(t.parts as u64, "part", "parts"),
                t.bytes
            );
        }
        if !self.searches.is_empty() {
            let _ = writeln!(s, "  searches: {}", self.searches.join(", "));
        }
        if self.retracts > 0 {
            let _ = writeln!(
                s,
                "  withdraws {}",
                crate::out::count(self.retracts as u64, "record", "records")
            );
        }
        if self.feeds.len() > 1 {
            let _ = writeln!(s, "  the store's feeds: {}", self.feeds.join(", "));
        }
        s
    }
}

/// `pack.info`.
pub struct PackInfo;
impl Op for PackInfo {
    const NAME: &'static str = "pack.info";
    const SUMMARY: &'static str = "Summarise a pack store's or export's version (title, publisher, signature, tables, searches) without importing or checking every file.";
    const READ_ONLY: bool = true;
    type In = InfoIn;
    type Out = InfoOut;
    fn run(cx: &Ctx, a: InfoIn, _: &dyn Progress) -> Result<InfoOut, ApiError> {
        let path = cx.fs.readable(&a.path)?;
        exists(&path)?;
        let src = Source::open(&path).map_err(ApiError::invalid)?;
        let h = match &a.manifest {
            Some(h) => h.clone(),
            None => src
                .default_manifest(a.feed.as_deref())
                .map_err(ApiError::invalid)?,
        };
        let v = src.version(&h).map_err(ApiError::invalid)?;
        let m = &v.manifest;
        let tables = v
            .tables
            .iter()
            .map(|(k, t)| {
                (
                    k.clone(),
                    TableInfo {
                        rows: t.parts.iter().map(|(p, _)| p.rows).sum(),
                        parts: t.parts.len(),
                        bytes: t.parts.iter().map(|(p, _)| p.bytes).sum(),
                    },
                )
            })
            .collect();
        Ok(InfoOut {
            kind: match src {
                Source::Store(_) => "store",
                Source::Export(_) => "export",
            }
            .into(),
            feeds: src.feeds(),
            manifest: v.hash.clone(),
            feed: m.feed.clone(),
            title: m.title.clone(),
            description: m.description.clone(),
            version_label: m.version_label.clone(),
            created: m.created.clone(),
            licence: m.licence.clone(),
            publisher: m.publisher.clone(),
            publisher_name: v.signed.name.clone(),
            signed: v.signed.by_publisher,
            bad_signatures: v.signed.bad.clone(),
            tables,
            searches: m.searches.keys().cloned().collect(),
            retracts: m.retracts.len(),
            parents: m.parents.clone(),
            stats: m.stats.clone(),
        })
    }
}

/// `pack.check`.
pub struct PackCheck;
impl Op for PackCheck {
    const NAME: &'static str = "pack.check";
    const SUMMARY: &'static str = "Check a pack store or export (format, signatures, every hash and id) without importing it.";
    const READ_ONLY: bool = true;
    type In = ImportIn;
    type Out = ImportOut;
    fn run(cx: &Ctx, a: ImportIn, p: &dyn Progress) -> Result<ImportOut, ApiError> {
        let o = import_opts(cx, &a, true)?;
        exists(&o.path)?;
        let r = pimport::import(&cx.library(), &o, &|l| p.log(l)).map_err(ApiError::invalid)?;
        Ok(import_out(r))
    }
}
