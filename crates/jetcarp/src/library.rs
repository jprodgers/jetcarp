//! `jetcarp library`: the pattern library (list, show, paste, promote, render, update, export,
//! import, remove, quickstart, info, verify, reverify, prune) and its ant runs (`library ants`)
//! and movers (`library movers`). Searches that add to it are jobs: `jetcarp search`,
//! `jetcarp jobs`.

use crate::show;
use anyhow::{Context, Result};
use jetcarp_api::Ctx;
use jetcarp_api::op::{self, NoInput, Op, Quiet};
use jetcarp_api::ops::library::{
    ExportIn, GetIn, ImportIn, LibraryExport, LibraryGet, LibraryImport, LibraryInfoOp,
    LibraryPaste, LibraryPrune, LibraryQuery, LibraryRemove, LibraryRender, LibraryReverify,
    LibraryUpdate, LibraryVerify, PasteSceneIn, PruneIn, QueryIn, RemoveIn, RenderIn, ReverifyIn,
    UpdateIn, VerifyIn,
};
use jetcarp_api::ops::quickstart::{
    AddIn, IdIn, ListIn, QuickstartAdd, QuickstartList, QuickstartRemove, QuickstartUpdate,
    StatusIn,
};
use std::path::PathBuf;

#[derive(clap::Args)]
pub struct LibraryArgs {
    /// Catalog: a .sqlite file, or a folder holding library.sqlite (default: $JETCARP_LIBRARY, else
    /// <data>/library.sqlite).
    #[arg(long, global = true)]
    library: Option<PathBuf>,
    #[command(subcommand)]
    cmd: LibCmd,
}

#[derive(clap::Subcommand)]
enum LibCmd {
    /// List patterns (highest interest score first), optionally filtered.
    List(ListArgs),
    /// Show one pattern: its record, and a text drawing on square and hexagon grids.
    Show {
        id: String,
        /// Print the record as JSON.
        #[arg(long)]
        json: bool,
        /// Draw this many phases (square and hex only).
        #[arg(long, default_value_t = 1)]
        phases: u64,
    },
    /// Make a scene from several library patterns placed on one grid.
    Paste(PasteArgs),
    /// Add a Quick Start entry for a pattern ("My finds") and mark it promoted.
    Promote {
        id: String,
        #[arg(long)]
        title: Option<String>,
        #[arg(long)]
        description: Option<String>,
    },
    /// Promote one run of a sweep folder to the Quick Start ("My finds"): its tiling, CAs, and
    /// start, so it replays as in the sweep. The run is also marked a favourite in the folder.
    PromoteRun {
        /// The sweep folder (with manifest.json and run.json).
        folder: PathBuf,
        /// The run: rule text ("HexT3 sum 588"), manifest index ("#12"), or series number ("n=588").
        run: String,
        /// Quick Start title (default: the rule).
        #[arg(long)]
        title: Option<String>,
        /// A note shown before the description (e.g. why it is interesting).
        #[arg(long)]
        note: Option<String>,
    },
    /// Draw a pattern's phases (PNG), or print them as text.
    Render {
        id: String,
        #[arg(long)]
        png: Option<PathBuf>,
        /// Panel size in pixels.
        #[arg(long, default_value_t = 160)]
        px: u32,
        #[arg(long, default_value_t = 16)]
        max_phases: u32,
        /// Print text drawings of the phases (square and hex grids).
        #[arg(long)]
        ascii: bool,
        /// The PNG's palette (`jetcarp palettes` lists them; default the rule's own colours).
        #[arg(long)]
        palette: Option<String>,
    },
    /// Change a pattern's name, tags, notes, rating, or curation state.
    Update(UpdateArgs),
    /// Write patterns as JSON records, one file each with its rules and rule range (all when
    /// no ids are given).
    Export {
        ids: Vec<String>,
        /// Folder for the files.
        #[arg(long)]
        to: PathBuf,
    },
    /// Merge patterns from JSON records (a file or folder; a Phase 3 library folder brings its
    /// Quick Start scenes too) or another catalog file. Each is verified again in every rule.
    Import { path: PathBuf },
    /// Delete a pattern (with its rules, sightings, and Quick Start entry).
    Remove { id: String },
    /// List the Quick Start entries ("My finds"), or change or remove one.
    Quickstart(QuickstartArgs),
    /// The catalog's ant and turmite runs: `list`, `show`, `census`, `update`, `remove`,
    /// `scene`, `promote`, and `run` (one rule by itself).
    Ants(crate::ants_cmd::AntsArgs),
    /// Movers on aperiodic tilings: `track` (over growing patch levels), `verify` (a library
    /// mover again), `list`.
    Movers(crate::movers_cmd::MoversArgs),
    /// The catalog's file, schema version, and row counts.
    Info {
        #[arg(long)]
        json: bool,
    },
    /// Verify stored patterns again with this build (e.g. after a geometry change): every
    /// pattern off squares and hexagons, or all with --all, plus --sample square and hexagon
    /// ones; --drop removes those that fail.
    Verify {
        #[arg(long)]
        all: bool,
        /// Square and hexagon patterns checked too (the first by id).
        #[arg(long, default_value_t = 200)]
        sample: usize,
        /// Periods run (default: the record's own default).
        #[arg(long)]
        periods: Option<u64>,
        #[arg(long)]
        drop: bool,
    },
    /// Check again the records flagged after a geometry change (their geometry pin is not this
    /// engine's), or every record with --all: those that pass get this engine's pin and a
    /// provenance row (and a new id if their geometry changed, the old id kept as an alias);
    /// those that fail stay flagged and are listed, or are deleted with --drop. Runs as a job,
    /// resumable with --job.
    Reverify {
        #[arg(long)]
        all: bool,
        #[arg(long)]
        drop: bool,
        /// Periods each pattern is run (default: its own default).
        #[arg(long)]
        periods: Option<u64>,
        /// Resume this re-verification job.
        #[arg(long)]
        job: Option<i64>,
        /// Worker threads (default: all cores).
        #[arg(long, default_value_t = 0)]
        threads: usize,
        /// Stop after this many minutes (resume with --job).
        #[arg(long)]
        minutes: Option<f64>,
    },
    /// Delete the still lifes and oscillators a hunt would no longer store (the hunt storage
    /// policy: more than --max-cells cells or period x cells above --max-volume, unless a job
    /// saw them --min-seen times; curated patterns are kept). Their sightings become unstored
    /// rows. Then compresses old pattern bodies and VACUUMs.
    Prune {
        #[arg(long, default_value_t = 64)]
        max_cells: u64,
        #[arg(long, default_value_t = 4000)]
        max_volume: u64,
        #[arg(long, default_value_t = 2)]
        min_seen: u64,
        /// Copy the catalog here first (a consistent copy, made by SQLite).
        #[arg(long)]
        backup: Option<PathBuf>,
        /// Skip the VACUUM (the file keeps its size until one runs).
        #[arg(long)]
        no_vacuum: bool,
    },
}

#[derive(clap::Args)]
struct QuickstartArgs {
    /// Only entries with this status: auto, candidate, approved, rejected.
    #[arg(long)]
    status: Option<String>,
    /// Set an entry's status: ID=STATUS (ID with or without `find:`).
    #[arg(long)]
    set: Option<String>,
    /// Delete an entry.
    #[arg(long)]
    remove: Option<String>,
    #[arg(long)]
    json: bool,
}

#[derive(clap::Args)]
struct ListArgs {
    /// Patterns verified in a rule whose notation contains this.
    #[arg(long)]
    rule: Option<String>,
    /// Patterns that run in this rule (inside their rule range, or verified there).
    #[arg(long)]
    works_in: Option<String>,
    /// score (default), seen, rare, newest, updated, cells, period, rating, id.
    #[arg(long)]
    order: Option<String>,
    #[arg(long)]
    offset: Option<usize>,
    /// Family id (hex), catalog id (k2-001), or part of the tiling name.
    #[arg(long)]
    tiling: Option<String>,
    /// still_life, oscillator, spaceship.
    #[arg(long)]
    class: Option<String>,
    #[arg(long)]
    min_period: Option<u64>,
    #[arg(long)]
    max_period: Option<u64>,
    #[arg(long)]
    min_score: Option<f64>,
    /// new, reviewed, promoted, rejected.
    #[arg(long)]
    state: Option<String>,
    #[arg(long)]
    tag: Option<String>,
    /// Part of the name or label.
    #[arg(long)]
    text: Option<String>,
    /// search, script, llm, user.
    #[arg(long)]
    by: Option<String>,
    #[arg(long)]
    limit: Option<usize>,
    /// Print the records as a JSON array.
    #[arg(long)]
    json: bool,
    /// Print only the ids.
    #[arg(long)]
    ids: bool,
}

#[derive(clap::Args)]
struct PasteArgs {
    /// Placements `ID@X,Y[,oK][,pK]` (orientation K, phase K), or `ID` alone for the centre.
    #[arg(required = true)]
    placements: Vec<String>,
    /// Scene file to write.
    #[arg(long)]
    out: PathBuf,
    /// Grid size in unit cells, or L<level> on aperiodic tilings.
    #[arg(long, default_value = "120x80")]
    size: String,
    #[arg(long, default_value = "wrap")]
    boundary: String,
    #[arg(long)]
    name: Option<String>,
    /// Also write a PNG of the scene after this many generations.
    #[arg(long)]
    png: Option<PathBuf>,
    #[arg(long, default_value_t = 0)]
    gens: u64,
}

#[derive(clap::Args)]
struct UpdateArgs {
    id: String,
    #[arg(long)]
    name: Option<String>,
    /// Replace the tags (comma separated).
    #[arg(long)]
    tags: Option<String>,
    #[arg(long)]
    add_tag: Vec<String>,
    #[arg(long)]
    remove_tag: Vec<String>,
    #[arg(long)]
    notes: Option<String>,
    #[arg(long)]
    append_notes: Option<String>,
    /// 1-5 (0 clears).
    #[arg(long)]
    rating: Option<u8>,
    /// new, reviewed, promoted, rejected.
    #[arg(long)]
    state: Option<String>,
    /// search, script, llm, user.
    #[arg(long)]
    discovered_by: Option<String>,
    /// Print the updated record as JSON.
    #[arg(long)]
    json: bool,
}

fn path(p: &std::path::Path) -> String {
    p.display().to_string()
}

pub fn run(a: LibraryArgs, json: bool) -> Result<()> {
    let cx = crate::platform::ctx().with_library(a.library.clone());
    match a.cmd {
        LibCmd::List(l) => list(&cx, l, json),
        LibCmd::Show {
            id,
            json: j,
            phases,
        } => {
            let v = LibraryGet::run(
                &cx,
                GetIn {
                    id,
                    kind: Some("pattern".into()),
                    phases: Some(phases),
                    bare: false,
                },
                &Quiet,
            )?;
            if json || j {
                // The record, as it has always printed.
                if let Some(r) = &v.pattern {
                    println!("{}", r.to_json());
                }
                return Ok(());
            }
            show(&v, false)
        }
        LibCmd::Paste(p) => {
            let a = PasteSceneIn {
                placements: p.placements,
                out: path(&p.out),
                size: p.size,
                boundary: p.boundary,
                name: p.name,
                png: p.png.as_deref().map(path),
                gens: p.gens,
            };
            show(&LibraryPaste::run(&cx, a, &Quiet)?, json)
        }
        LibCmd::Promote {
            id,
            title,
            description,
        } => {
            let a = AddIn {
                pattern: Some(id),
                title,
                description,
                ..Default::default()
            };
            show(&QuickstartAdd::run(&cx, a, &Quiet)?, json)
        }
        LibCmd::PromoteRun {
            folder,
            run,
            title,
            note,
        } => {
            let a = AddIn {
                folder: Some(path(&folder)),
                run: Some(run),
                title,
                note,
                favourite: true,
                ..Default::default()
            };
            show(&QuickstartAdd::run(&cx, a, &Quiet)?, json)
        }
        LibCmd::Render {
            id,
            png,
            px,
            max_phases,
            ascii,
            palette,
        } => {
            let a = RenderIn {
                id,
                png: png.as_deref().map(path),
                px,
                max_phases,
                ascii,
                palette,
            };
            show(&LibraryRender::run(&cx, a, &Quiet)?, json)
        }
        LibCmd::Update(u) => update_cmd(&cx, u, json),
        LibCmd::Export { ids, to } => {
            let a = ExportIn { ids, to: path(&to) };
            show(&LibraryExport::run(&cx, a, &Quiet)?, json)
        }
        LibCmd::Import { path: p } => {
            let a = ImportIn { path: path(&p) };
            show(&LibraryImport::run(&cx, a, &Quiet)?, json)
        }
        LibCmd::Remove { id } => {
            let a = RemoveIn {
                id,
                kind: Some("pattern".into()),
            };
            show(&LibraryRemove::run(&cx, a, &Quiet)?, json)
        }
        LibCmd::Quickstart(q) => quickstart_cmd(&cx, q, json),
        LibCmd::Ants(a) => crate::ants_cmd::run(&cx, a, json),
        LibCmd::Movers(a) => crate::movers_cmd::run(&cx, a, json),
        LibCmd::Verify {
            all,
            sample,
            periods,
            drop,
        } => {
            let a = VerifyIn {
                all,
                sample,
                periods,
                drop,
            };
            let out = LibraryVerify::run(&cx, a, &crate::ProgressLine)?;
            eprintln!();
            show(&out, json)
        }
        LibCmd::Reverify {
            all,
            drop,
            periods,
            job,
            threads,
            minutes,
        } => {
            let a = ReverifyIn {
                all,
                drop,
                periods,
                job,
                threads,
                minutes,
            };
            let out = LibraryReverify::run(&cx, a, &crate::ProgressLine)?;
            eprintln!();
            show(&out, json)
        }
        LibCmd::Prune {
            max_cells,
            max_volume,
            min_seen,
            backup,
            no_vacuum,
        } => {
            let a = PruneIn {
                max_cells,
                max_volume,
                min_seen,
                backup: backup.as_deref().map(path),
                no_vacuum,
            };
            show(&LibraryPrune::run(&cx, a, &Quiet)?, json)
        }
        LibCmd::Info { json: j } => {
            let i = LibraryInfoOp::run(&cx, NoInput {}, &Quiet)?;
            if json || j {
                // In the record's own key order, as it has always printed.
                println!("{}", serde_json::to_string_pretty(&i.0)?);
                return Ok(());
            }
            show(&i, false)
        }
    }
}

fn quickstart_cmd(cx: &Ctx, q: QuickstartArgs, json: bool) -> Result<()> {
    let json = json || q.json;
    if let Some(id) = q.remove {
        return show(&QuickstartRemove::run(cx, IdIn { id }, &Quiet)?, json);
    }
    if let Some(s) = &q.set {
        let (id, st) = s
            .rsplit_once('=')
            .context("--set wants ID=STATUS, e.g. find:1a2b...=approved")?;
        let a = StatusIn {
            id: id.to_string(),
            status: st.to_string(),
        };
        return show(&QuickstartUpdate::run(cx, a, &Quiet)?, json);
    }
    let v = QuickstartList::run(cx, ListIn { status: q.status }, &Quiet)?;
    if json {
        // The entries, as they have always printed.
        println!("{}", op::to_pretty(&v.entries));
        return Ok(());
    }
    show(&v, false)
}

fn list(cx: &Ctx, l: ListArgs, json: bool) -> Result<()> {
    let q = QueryIn {
        rule: l.rule,
        works_in: l.works_in,
        order: l.order,
        offset: l.offset,
        tiling: l.tiling,
        class: l.class,
        min_period: l.min_period,
        max_period: l.max_period,
        min_score: l.min_score,
        state: l.state,
        tag: l.tag,
        text: l.text,
        discovered_by: l.by,
        limit: l.limit,
        ..Default::default()
    };
    let v = LibraryQuery::run(cx, q, &Quiet)?;
    if json || l.json {
        // The records, as they have always printed.
        println!("{}", serde_json::to_string_pretty(&v.patterns)?);
        return Ok(());
    }
    if l.ids {
        for r in &v.patterns {
            println!("{}", r.id);
        }
        return Ok(());
    }
    show(&v, false)
}

fn update_cmd(cx: &Ctx, u: UpdateArgs, json: bool) -> Result<()> {
    let a = UpdateIn {
        id: u.id,
        kind: Some("pattern".into()),
        name: u.name,
        tags: u.tags.map(|t| {
            t.split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect()
        }),
        add_tags: u.add_tag,
        remove_tags: u.remove_tag,
        notes: u.notes,
        append_notes: u.append_notes,
        rating: u.rating.map(f64::from),
        state: u.state,
        discovered_by: u.discovered_by,
    };
    let r = LibraryUpdate::run(cx, a, &Quiet)?;
    if json || u.json {
        // The record, as it has always printed.
        if let Some(p) = &r.pattern {
            println!("{}", p.to_json());
        }
        return Ok(());
    }
    show(&r, false)
}
