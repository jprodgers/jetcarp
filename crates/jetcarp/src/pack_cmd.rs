//! `jetcarp pack`: packs (the book's *Pack format* reference). `key` makes or shows the
//! publisher's key, `rater` records a rater's consent, `publish` writes a new signed version of
//! a feed into a store, `export` lays one version out for a single download, `info` summarises
//! a version, `check` checks a store or export, and `import` brings one into the library. The
//! work is in the operations `pack.key`, `pack.rater`, `pack.publish`, `pack.export`,
//! `pack.info`, `pack.check` and `pack.import`.

use crate::show;
use anyhow::Result;
use jetcarp_api::op::{Op, Progress};
use jetcarp_api::ops::pack::{
    ExportIn, ImportIn, InfoIn, KeyIn, PackCheck, PackExport, PackImport, PackInfo, PackKey,
    PackPublish, PackRater, PublishIn, RaterIn,
};
use std::path::PathBuf;

#[derive(clap::Args)]
pub struct PackArgs {
    /// Catalog: a .sqlite file, or a folder holding library.sqlite (default: $JETCARP_LIBRARY, else
    /// <data>/library.sqlite).
    #[arg(long, global = true)]
    library: Option<PathBuf>,
    #[command(subcommand)]
    cmd: PackCmd,
}

/// What a selection publishes: the shared flags of `publish` and `export`.
#[derive(clap::Args, Clone, Default)]
struct PublishFlags {
    /// The feed: letters, digits and - (jetcarp-starter).
    #[arg(long)]
    feed: Option<String>,
    #[arg(long, default_value = "")]
    title: String,
    #[arg(long, default_value = "")]
    description: String,
    /// Free text for people (2026.11).
    #[arg(long, default_value = "")]
    version_label: String,
    /// An SPDX licence id (default CC0-1.0).
    #[arg(long)]
    licence: Option<String>,
    /// The version's time (RFC 3339 UTC, default now), for reproducible packs.
    #[arg(long)]
    created: Option<String>,
    /// Only these records (ids, prefixes or names; repeat).
    #[arg(long = "id")]
    ids: Vec<String>,
    /// Only the patterns a library query finds, as JSON ('{"class":"spaceship"}').
    #[arg(long)]
    query: Option<String>,
    /// Only these explore searches (uuids; repeat).
    #[arg(long = "search")]
    searches: Vec<String>,
    /// Leave out explore searches (their runs, scores and settings).
    #[arg(long)]
    no_searches: bool,
    /// Leave out media (posters, thumbnails, animations).
    #[arg(long)]
    no_media: bool,
    /// Leave out ratings (otherwise only consented raters' go in).
    #[arg(long)]
    no_ratings: bool,
    /// Leave out ant and turmite runs.
    #[arg(long)]
    no_ants: bool,
    /// Leave out Quick Start entries.
    #[arg(long)]
    no_quickstart: bool,
    /// Withdraw a record: pattern:<id>=reason (repeat).
    #[arg(long)]
    retract: Vec<String>,
    /// Merge another version: a store's folder, or <folder>@sha256:<manifest> (repeat).
    #[arg(long)]
    merge: Vec<String>,
    /// Write views.sql (DuckDB views over the store).
    #[arg(long)]
    views: bool,
    /// The signing key (an OpenSSH Ed25519 private key; default the settings folder's).
    #[arg(long)]
    key: Option<String>,
}

impl PublishFlags {
    fn input(&self, to: String) -> Result<PublishIn> {
        let query = self
            .query
            .as_deref()
            .map(serde_json::from_str)
            .transpose()
            .map_err(|e| anyhow::anyhow!("--query: not JSON: {e}"))?;
        Ok(PublishIn {
            feed: self
                .feed
                .clone()
                .ok_or_else(|| anyhow::anyhow!("--feed NAME is needed"))?,
            to,
            title: self.title.clone(),
            description: self.description.clone(),
            version_label: self.version_label.clone(),
            licence: self.licence.clone(),
            created: self.created.clone(),
            ids: self.ids.clone(),
            query,
            searches: (!self.searches.is_empty()).then(|| self.searches.clone()),
            no_searches: self.no_searches,
            no_media: self.no_media,
            no_ratings: self.no_ratings,
            no_ants: self.no_ants,
            no_quickstart: self.no_quickstart,
            retract: self.retract.clone(),
            merge: self.merge.clone(),
            views: self.views,
            key: self.key.clone(),
        })
    }
}

#[derive(clap::Args, Clone)]
struct ReadFlags {
    /// A store's or export's folder.
    path: String,
    /// The feed, when the store has several.
    #[arg(long)]
    feed: Option<String>,
    /// A version other than the feed's newest (sha256:...).
    #[arg(long)]
    manifest: Option<String>,
    /// Accept a version its publisher did not sign.
    #[arg(long)]
    allow_unsigned: bool,
}

#[derive(clap::Subcommand)]
enum PackCmd {
    /// Make or show the publisher's Ed25519 key (kept in the settings folder), and who the
    /// publisher is (named only if given).
    Key {
        /// The publisher's name, as packs show it (and the default rater of `jobs review`);
        /// "" for none.
        #[arg(long)]
        name: Option<String>,
        /// The publisher's ORCID iD (0000-0002-1825-0097); "" for none.
        #[arg(long)]
        orcid: Option<String>,
        /// The publisher's GitHub account name; "" for none.
        #[arg(long)]
        github: Option<String>,
        /// Use this OpenSSH Ed25519 private key file instead.
        #[arg(long)]
        key: Option<String>,
    },
    /// Record a rater's consent to permanent public release of their ratings (or withdraw it).
    Rater {
        /// The rater's name as the library's ratings write it.
        name: String,
        /// Withdraw consent: later packs leave their ratings out.
        #[arg(long)]
        withdraw: bool,
        /// Packs may show the name (else only a pseudonymous id).
        #[arg(long)]
        show_name: bool,
        #[arg(long, default_value = "")]
        orcid: String,
        /// Their notes may be published too.
        #[arg(long)]
        notes: bool,
    },
    /// Publish the library (or a selection) as a new signed version of a feed in a store.
    Publish {
        /// The store's folder.
        #[arg(long)]
        to: String,
        #[command(flatten)]
        sel: PublishFlags,
    },
    /// Lay one version out with readable names for a single download (from a store, or the
    /// library published first with the publish flags).
    Export {
        /// The export's folder (conventionally <feed>-<version label>).
        #[arg(long)]
        to: String,
        /// The store to export a version of (default: publish the library first).
        #[arg(long)]
        from: Option<String>,
        /// The version (default: the newest of the store's feed).
        #[arg(long)]
        manifest: Option<String>,
        #[command(flatten)]
        sel: PublishFlags,
    },
    /// A version's summary: title, publisher and signature, tables (rows, parts, bytes),
    /// searches (nothing is imported, and the data files are not read).
    Info {
        /// A store's or export's folder.
        path: String,
        /// The feed, when the store has several.
        #[arg(long)]
        feed: Option<String>,
        /// A version other than the feed's newest (sha256:...).
        #[arg(long)]
        manifest: Option<String>,
    },
    /// Check a store or export: format, signatures, every hash and id (nothing is imported).
    Check {
        #[command(flatten)]
        r: ReadFlags,
    },
    /// Import a store or export into the library: every hash, id and signature checked, records
    /// merged by id, a sample verified again.
    Import {
        #[command(flatten)]
        r: ReadFlags,
        /// Refuse two different rows for one record id.
        #[arg(long)]
        strict: bool,
        /// sample, full or none.
        #[arg(long, default_value = "sample")]
        verify: String,
        /// How many new patterns the sample verifies again.
        #[arg(long, default_value_t = 3000)]
        sample: u64,
        /// The sample's seed (default: from the manifest hash, so the same pack samples the
        /// same patterns).
        #[arg(long)]
        seed: Option<u64>,
        /// Most seconds verification may take.
        #[arg(long, default_value_t = 600.0)]
        verify_seconds: f64,
    },
}

/// Progress lines on standard error.
struct Log;
impl Progress for Log {
    fn log(&self, line: &str) {
        eprintln!("{line}");
    }
}

fn read_in(r: &ReadFlags) -> ImportIn {
    ImportIn {
        path: r.path.clone(),
        feed: r.feed.clone(),
        manifest: r.manifest.clone(),
        allow_unsigned: r.allow_unsigned,
        ..Default::default()
    }
}

pub fn run(a: PackArgs, json: bool) -> Result<()> {
    let cx = crate::platform::ctx()
        .with_library(a.library.clone().map(jetcarp_store::Library::file_for));
    match a.cmd {
        PackCmd::Key {
            name,
            orcid,
            github,
            key,
        } => show(
            &PackKey::run(
                &cx,
                KeyIn {
                    name,
                    orcid,
                    github,
                    key,
                },
                &Log,
            )?,
            json,
        ),
        PackCmd::Rater {
            name,
            withdraw,
            show_name,
            orcid,
            notes,
        } => show(
            &PackRater::run(
                &cx,
                RaterIn {
                    name,
                    consent: !withdraw,
                    show_name,
                    orcid,
                    notes,
                },
                &Log,
            )?,
            json,
        ),
        PackCmd::Publish { to, sel } => show(&PackPublish::run(&cx, sel.input(to)?, &Log)?, json),
        PackCmd::Export {
            to,
            from,
            manifest,
            sel,
        } => {
            let publish = match &from {
                Some(_) => None,
                None => Some(sel.input(String::new())?),
            };
            show(
                &PackExport::run(
                    &cx,
                    ExportIn {
                        to,
                        from,
                        manifest,
                        publish,
                    },
                    &Log,
                )?,
                json,
            )
        }
        PackCmd::Info {
            path,
            feed,
            manifest,
        } => show(
            &PackInfo::run(
                &cx,
                InfoIn {
                    path,
                    feed,
                    manifest,
                },
                &Log,
            )?,
            json,
        ),
        PackCmd::Check { r } => show(&PackCheck::run(&cx, read_in(&r), &Log)?, json),
        PackCmd::Import {
            r,
            strict,
            verify,
            sample,
            seed,
            verify_seconds,
        } => {
            let mut i = read_in(&r);
            i.strict = strict;
            i.verify = Some(verify);
            i.sample = Some(sample);
            i.seed = seed;
            i.verify_seconds = Some(verify_seconds);
            show(&PackImport::run(&cx, i, &Log)?, json)
        }
    }
}
