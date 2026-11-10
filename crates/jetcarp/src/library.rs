//! `jetcarp library`: the pattern catalog (list, show, search, collide, paste, promote, render,
//! update, export, import, remove, quickstart, jobs, info). The work is in `jetcarp_core::library`.

use anyhow::{Context, Result, bail};
use jetcarp_core::library::collide::{CollideSpec, collide};
use jetcarp_core::library::search::{SearchSpec, search};
use jetcarp_core::library::{
    self, AddOutcome, CurationState, DiscoveredBy, JobStatus, Library, PasteAt, PatternClass,
    PatternRecord, QsStatus, Query, Update,
};
use jetcarp_core::render::write_png;
use jetcarp_core::*;
use std::path::PathBuf;

#[derive(clap::Args)]
pub struct LibraryArgs {
    /// Catalog: a .sqlite file, or a folder holding catalog.sqlite (default: $JETCARP_LIBRARY, else
    /// <data>/catalog.sqlite).
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
    /// Run soups, catalog every verified still life, oscillator, and spaceship.
    Search(Box<SearchArgs>),
    /// Collide pattern B into pattern A at every offset and phase in a range.
    Collide(Box<CollideArgs>),
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
    /// List search jobs.
    Jobs {
        #[arg(long)]
        json: bool,
    },
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
struct SearchArgs {
    /// Rules separated by spaces or semicolons ("B3/S23 B36/S23").
    #[arg(long, alias = "rule")]
    rules: Option<String>,
    /// File with one rule per line (# comments).
    #[arg(long)]
    rules_file: Option<PathBuf>,
    /// Tilings: family ids, catalog:<id>, or the sets all-periodic, archimedean, kuniform:<k>,
    /// isohedral; comma separated.
    #[arg(long, alias = "tiling", default_value = "square")]
    tilings: String,
    /// WxH unit cells (64x64), about N cells (4000), or L<level> for aperiodic patches.
    #[arg(long, default_value = "64x64")]
    size: String,
    /// wrap or dead.
    #[arg(long, default_value = "wrap")]
    boundary: String,
    /// Neighbourhood for every rule: touch or edge (default: per rule; V suffix = edge).
    #[arg(long)]
    nb: Option<String>,
    #[arg(long, default_value_t = 16)]
    soups: u32,
    #[arg(long, default_value_t = 0.37)]
    density: f64,
    #[arg(long, default_value_t = 2000)]
    gens: u64,
    #[arg(long, default_value_t = 1)]
    seed: u64,
    #[arg(long, default_value_t = 64)]
    max_period: u64,
    /// Soup only a disc at the centre: radius as a fraction of half the shorter side (e.g. 0.15).
    /// With --boundary dead, escaping spaceships are caught at the edge and cataloged.
    #[arg(long)]
    soup_radius: Option<f64>,
    /// Write the full report as JSON.
    #[arg(long)]
    json: Option<PathBuf>,
    /// Report only; do not add to the library.
    #[arg(long)]
    no_store: bool,
    /// Worker threads (results do not depend on this).
    #[arg(long)]
    threads: Option<usize>,
    #[arg(long)]
    quiet: bool,
    /// Rows per group in the printed report.
    #[arg(long, default_value_t = 12)]
    top: usize,
}

#[derive(clap::Args)]
struct CollideArgs {
    /// Pattern A (stays at the origin).
    a: String,
    /// Pattern B (placed at each offset).
    b: String,
    /// Offset range for both axes, like -6..6 (overridden by --dx/--dy).
    #[arg(long, default_value = "-6..6", allow_hyphen_values = true)]
    offsets: String,
    #[arg(long, allow_hyphen_values = true)]
    dx: Option<String>,
    #[arg(long, allow_hyphen_values = true)]
    dy: Option<String>,
    /// B's phases: all, or a list like 0,1.
    #[arg(long, default_value = "all")]
    phases: String,
    #[arg(long, default_value_t = 0)]
    orientation_a: u32,
    /// B's orientation (index into the tiling's symmetries; `jetcarp library show` lists the
    /// direction each one moves a spaceship).
    #[arg(long, default_value_t = 0)]
    orientation_b: u32,
    #[arg(long, default_value_t = 1024)]
    gens: u64,
    #[arg(long, default_value_t = 40)]
    margin: i32,
    /// Rule to run (both patterns must work in it; default: A's rule if B works in it, else
    /// B's).
    #[arg(long)]
    rule: Option<String>,
    #[arg(long)]
    csv: Option<PathBuf>,
    #[arg(long)]
    json: Option<PathBuf>,
    /// Labelled contact sheet of every run's final state, objects coloured by class.
    #[arg(long)]
    sheet: Option<PathBuf>,
    #[arg(long, default_value_t = 96)]
    thumb: u32,
    #[arg(long, default_value_t = 13)]
    cols: u32,
    /// Do not catalog the products.
    #[arg(long)]
    no_store: bool,
    #[arg(long)]
    threads: Option<usize>,
    #[arg(long)]
    quiet: bool,
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

fn msg<T>(r: Result<T, String>) -> Result<T> {
    r.map_err(anyhow::Error::msg)
}

fn set_threads(t: Option<usize>) -> Result<()> {
    if let Some(t) = t {
        rayon::ThreadPoolBuilder::new()
            .num_threads(t)
            .build_global()
            .context("thread pool")?;
    }
    Ok(())
}

fn parse_size(s: &str) -> Result<SizeSpec> {
    let l = s.to_ascii_lowercase();
    if let Some((w, h)) = l.split_once('x') {
        return Ok(SizeSpec::Rect {
            w: w.trim().parse().context("width")?,
            h: h.trim().parse().context("height")?,
        });
    }
    if let Some(n) = l.strip_prefix("level").or_else(|| l.strip_prefix('l')) {
        return Ok(SizeSpec::Level(n.trim().parse().context("level")?));
    }
    Ok(SizeSpec::Cells(
        l.trim()
            .parse()
            .context("size is WxH, a cell count, or L<level>")?,
    ))
}

fn parse_boundary(s: &str) -> Result<Boundary> {
    Ok(match s.to_ascii_lowercase().as_str() {
        "wrap" | "torus" => Boundary::Wrap,
        "dead" => Boundary::Dead,
        _ => bail!("--boundary is wrap or dead"),
    })
}

fn parse_range(s: &str) -> Result<[i32; 2]> {
    let (a, b) = s
        .split_once("..")
        .map(|(a, b)| (a.trim(), b.trim().trim_start_matches('=')))
        .unwrap_or((s.trim(), s.trim()));
    let a: i32 = a.parse().with_context(|| format!("range '{s}'"))?;
    let b: i32 = b.parse().with_context(|| format!("range '{s}'"))?;
    Ok([a.min(b), a.max(b)])
}

pub fn run(a: LibraryArgs) -> Result<()> {
    let lib = match &a.library {
        Some(p) => Library::open(p.clone()),
        None => Library::open_default(),
    };
    match a.cmd {
        LibCmd::List(l) => list(&lib, l),
        LibCmd::Show { id, json, phases } => show(&lib, &id, json, phases),
        LibCmd::Search(s) => search_cmd(&lib, *s),
        LibCmd::Collide(c) => collide_cmd(&lib, *c),
        LibCmd::Paste(p) => paste_cmd(&lib, p),
        LibCmd::Promote {
            id,
            title,
            description,
        } => {
            let (sc, entry) = msg(library::promote::promote(
                &lib,
                &id,
                title.as_deref(),
                description.as_deref(),
            ))?;
            println!("promoted: {}", sc.name);
            println!("entry:    find:{entry} in {}", lib.path().display());
            println!("It appears in the app's Quick Start under \"My finds\".");
            Ok(())
        }
        LibCmd::PromoteRun {
            folder,
            run,
            title,
            note,
        } => {
            let p = msg(jetcarp_script::results::promote_folder_run(
                &folder,
                &run,
                &lib,
                title.as_deref(),
                note.as_deref(),
            ))?;
            println!("promoted: {} as \"{}\" (run #{})", p.rule, p.title, p.index);
            println!("entry:    find:{} in {}", p.id, lib.path().display());
            println!("It appears in the app's Quick Start under \"My finds\".");
            Ok(())
        }
        LibCmd::Render {
            id,
            png,
            px,
            max_phases,
            ascii,
            palette,
        } => {
            if let Some(p) = &palette {
                msg(jetcarp_core::render::builtin_palette(p))?;
            }
            let rec = msg(lib.get(&id))?;
            if ascii || png.is_none() {
                print_ascii(&rec, rec.period.min(max_phases as u64))?;
            }
            if let Some(p) = png {
                let img = msg(library::promote::render_phases_in(
                    &rec,
                    px,
                    max_phases,
                    palette.as_deref(),
                ))?;
                let meta = vec![
                    ("jetcarp:version", VERSION.to_string()),
                    ("jetcarp:pattern", rec.id.clone()),
                    ("jetcarp:label", rec.label()),
                ];
                msg(write_png(&p, &img, &meta))?;
                println!("png: {} ({}x{})", p.display(), img.width, img.height);
            }
            Ok(())
        }
        LibCmd::Update(u) => update_cmd(&lib, u),
        LibCmd::Export { ids, to } => {
            let done = msg(lib.export(&ids, &to))?;
            println!("{} pattern(s) exported to {}", done.len(), to.display());
            Ok(())
        }
        LibCmd::Import { path } => {
            let done = msg(lib.import(&path))?;
            print_adds(&done);
            let distinct: std::collections::HashSet<&String> = done.iter().map(|d| &d.0).collect();
            println!(
                "{} pattern(s) ({} pattern and rule pairs) imported into {}",
                distinct.len(),
                done.len(),
                lib.path().display()
            );
            Ok(())
        }
        LibCmd::Remove { id } => {
            let r = msg(lib.remove(&id))?;
            println!("removed {} ({})", r.id, r.label());
            Ok(())
        }
        LibCmd::Quickstart(q) => quickstart_cmd(&lib, q),
        LibCmd::Jobs { json } => {
            let v = msg(lib.jobs())?;
            if json {
                println!("{}", serde_json::to_string_pretty(&v)?);
                return Ok(());
            }
            for j in &v {
                println!(
                    "{:>5}  {:<8} {:<8} {}  {}{}",
                    j.id,
                    j.kind,
                    format!("{:?}", j.status).to_lowercase(),
                    j.created,
                    j.progress,
                    if j.has_checkpoint {
                        "  (checkpoint)"
                    } else {
                        ""
                    }
                );
            }
            println!(
                "{} job(s); {} running",
                v.len(),
                v.iter()
                    .filter(|j| j.id != 0 && j.status == JobStatus::Running)
                    .count()
            );
            Ok(())
        }
        LibCmd::Verify {
            all,
            sample,
            periods,
            drop,
        } => verify_cmd(&lib, all, sample, periods, drop),
        LibCmd::Prune {
            max_cells,
            max_volume,
            min_seen,
            backup,
            no_vacuum,
        } => {
            let size = |p: &std::path::Path| std::fs::metadata(p).map(|m| m.len()).unwrap_or(0);
            let file = lib.path().to_path_buf();
            let before = size(&file);
            if let Some(b) = &backup {
                let t0 = std::time::Instant::now();
                msg(lib.backup_to(b))?;
                println!(
                    "backup: {} ({:.1} GB, {:.0} s)",
                    b.display(),
                    size(b) as f64 / 1e9,
                    t0.elapsed().as_secs_f64()
                );
            }
            let t0 = std::time::Instant::now();
            let r = msg(lib.prune_patterns(max_cells, max_volume, min_seen))?;
            println!(
                "pruned {} of {} still lifes and oscillators ({:?}); {} unstored rows, {} media rows; {:.0} s",
                r.deleted,
                r.checked,
                r.by_class,
                r.unstored_rows,
                r.media_rows,
                t0.elapsed().as_secs_f64()
            );
            let t0 = std::time::Instant::now();
            let (n, a, b) = msg(lib.compact_bodies())?;
            println!(
                "compressed {n} bodies: {:.0} MB to {:.0} MB; {:.0} s",
                a as f64 / 1e6,
                b as f64 / 1e6,
                t0.elapsed().as_secs_f64()
            );
            if !no_vacuum {
                let t0 = std::time::Instant::now();
                msg(lib.vacuum())?;
                println!("vacuum: {:.0} s", t0.elapsed().as_secs_f64());
            }
            println!(
                "{}: {:.2} GB before, {:.2} GB after",
                file.display(),
                before as f64 / 1e9,
                size(&file) as f64 / 1e9
            );
            Ok(())
        }
        LibCmd::Info { json } => {
            let i = msg(lib.info())?;
            if json {
                println!("{}", serde_json::to_string_pretty(&i)?);
            } else {
                println!("catalog:        {}", i.path);
                println!("schema version: {}", i.schema_version);
                println!(
                    "patterns {}, rules {}, pattern rules {}, sightings {} ({} copies), jobs {}, \
                     Quick Start {}, ratings {}, media {}, ants {}",
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
            }
            Ok(())
        }
    }
}

fn quickstart_cmd(lib: &Library, q: QuickstartArgs) -> Result<()> {
    if let Some(id) = &q.remove {
        msg(lib.remove_quickstart(id))?;
        println!("removed {id}");
        return Ok(());
    }
    if let Some(s) = &q.set {
        let (id, st) = s
            .rsplit_once('=')
            .context("--set wants ID=STATUS, e.g. find:1a2b...=approved")?;
        msg(lib.set_quickstart_status(id, msg(QsStatus::parse(st))?))?;
        println!("{id}: {st}");
        return Ok(());
    }
    let st = match &q.status {
        Some(s) => vec![msg(QsStatus::parse(s))?],
        None => vec![],
    };
    let v = msg(lib.quickstart(&st))?;
    if q.json {
        let rows: Vec<serde_json::Value> = v
            .iter()
            .map(|r| {
                serde_json::json!({
                    "id": format!("find:{}", r.id), "title": r.title, "grid": r.grid,
                    "rule": r.rule, "rule_family": r.rule_family, "behaviour": r.behaviour,
                    "source_kind": r.source_kind, "source_id": r.source_id, "score": r.score,
                    "status": r.status, "created": r.created, "description": r.description,
                })
            })
            .collect();
        println!("{}", serde_json::to_string_pretty(&rows)?);
        return Ok(());
    }
    println!("catalog: {} ({} entries)", lib.path().display(), v.len());
    for r in &v {
        println!(
            "find:{:<24} {:<9} {:<8} {:<28} {:<14} {}",
            r.id,
            r.status.name(),
            r.source_kind,
            r.title.chars().take(28).collect::<String>(),
            r.rule.chars().take(14).collect::<String>(),
            r.grid
        );
    }
    Ok(())
}

fn print_adds(v: &[(String, AddOutcome)]) {
    for (id, o) in v {
        match o {
            AddOutcome::Added => println!("  added  {id}"),
            AddOutcome::Merged { seen, new_rule } => println!(
                "  merged {id} (seen {seen}{})",
                if *new_rule { ", new rule" } else { "" }
            ),
        }
    }
}

fn list(lib: &Library, l: ListArgs) -> Result<()> {
    let q = Query {
        rule: l.rule,
        works_in: l.works_in,
        neighborhood: None,
        order: l.order,
        offset: l.offset,
        tiling: l.tiling,
        class: l
            .class
            .as_deref()
            .map(PatternClass::parse)
            .transpose()
            .map_err(anyhow::Error::msg)?,
        min_period: l.min_period,
        max_period: l.max_period,
        min_score: l.min_score,
        state: l
            .state
            .as_deref()
            .map(CurationState::parse)
            .transpose()
            .map_err(anyhow::Error::msg)?,
        tag: l.tag,
        text: l.text,
        discovered_by: l
            .by
            .as_deref()
            .map(DiscoveredBy::parse)
            .transpose()
            .map_err(anyhow::Error::msg)?,
        limit: l.limit,
    };
    let v = msg(lib.query(&q))?;
    if l.json {
        println!("{}", serde_json::to_string_pretty(&v)?);
        return Ok(());
    }
    if l.ids {
        for r in &v {
            println!("{}", r.id);
        }
        return Ok(());
    }
    println!(
        "catalog: {} ({} shown of {} matching)",
        lib.path().display(),
        v.len(),
        msg(lib.count(&q))?
    );
    for r in &v {
        println!("{}", r.summary());
    }
    Ok(())
}

fn print_ascii(rec: &PatternRecord, phases: u64) -> Result<()> {
    for t in 0..phases.max(1) {
        match msg(library::ascii(rec, t))? {
            Some(s) => {
                println!("phase {t}:");
                print!("{s}");
            }
            None => {
                println!(
                    "phase {t}: {}",
                    rec.phases[(t % rec.period.max(1)) as usize]
                );
            }
        }
    }
    Ok(())
}

fn show(lib: &Library, id: &str, json: bool, phases: u64) -> Result<()> {
    let r = msg(lib.get(id))?;
    if json {
        println!("{}", r.to_json());
        return Ok(());
    }
    println!("id:          {}", r.id);
    println!("label:       {}", r.label());
    println!(
        "rule:        {} ({:?} neighbours, {} step(s) per generation)",
        r.rule, r.neighborhood, r.steps_per_gen
    );
    println!(
        "tiling:      {} ({} tile type(s) per unit cell)",
        r.tiling_name, r.tile_types
    );
    println!("class:       {}, period {}", r.class.name(), r.period);
    if r.rules.len() > 1 {
        println!("verified in: {}", r.rules.join(", "));
    }
    if let Some(x) = &r.range {
        println!("works in:    {}", x.describe());
    }
    if let (Some(d), Some(xy)) = (r.displacement, r.displacement_xy) {
        println!(
            "moves:       {} : lattice vector ({}, {}) per period = ({:.3}, {:.3}) world units",
            r.speed.as_deref().unwrap_or("?"),
            d[0],
            d[1],
            xy[0],
            xy[1]
        );
        // Direction in each orientation, for `collide --orientation-b` and pasting.
        let dirs: Vec<String> = msg(library::orientation_moves(&r))?
            .iter()
            .enumerate()
            .map(|(k, v)| format!("{k}:({},{})", v[0], v[1]))
            .collect();
        println!("orientations: {}", dirs.join(" "));
    }
    println!(
        "cells:       {} (phases {}..{}), bounding box {}x{}",
        r.cells,
        r.min_cells,
        r.max_cells,
        r.bbox[2] + 1,
        r.bbox[3] + 1
    );
    println!("seen:        {}", r.seen);
    for s in msg(lib.sightings(&r.id))? {
        println!(
            "             {} x{} in {} soup(s), job {} ({} to {})",
            s.rule, s.count, s.soups, s.job, s.first_seen, s.last_seen
        );
    }
    println!(
        "score:       {}{}",
        r.score,
        r.rating
            .map(|x| format!("  rating {x}/5"))
            .unwrap_or_default()
    );
    println!(
        "state:       {}  (discovered by {:?})",
        r.state.name(),
        r.discovered_by
    );
    if !r.tags.is_empty() {
        println!("tags:        {}", r.tags.join(", "));
    }
    println!(
        "verified:    {} periods ({} generations) on {}, JETCARP {} {}",
        r.verified.periods,
        r.verified.generations,
        r.verified.field,
        r.verified.jetcarp_version,
        r.verified.date
    );
    let p = &r.provenance;
    println!(
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
    if let Some(s) = &r.site {
        println!(
            "site:        {} tiles within {} steps, anchor cell {} on {} {:?}",
            s.tiles.len(),
            s.radius,
            s.anchor,
            s.patch.family,
            s.patch.size
        );
    }
    println!("canonical:   {}", r.canonical);
    if !r.notes.is_empty() {
        println!("notes:\n{}", r.notes);
    }
    println!();
    print_ascii(&r, phases)
}

fn search_cmd(lib: &Library, a: SearchArgs) -> Result<()> {
    set_threads(a.threads)?;
    let mut rules = Vec::new();
    if let Some(r) = &a.rules {
        rules.extend(
            r.split([';', ' ', '\t', '\n'])
                .map(str::trim)
                .filter(|x| !x.is_empty())
                .map(String::from),
        );
    }
    if let Some(p) = &a.rules_file {
        let text =
            std::fs::read_to_string(p).with_context(|| format!("reading {}", p.display()))?;
        rules.extend(census::parse_rule_list(&text));
    }
    if rules.is_empty() {
        bail!("no rules: use --rules or --rules-file");
    }
    let spec = SearchSpec {
        rules,
        tilings: msg(library::tiling_list(&a.tilings))?,
        size: parse_size(&a.size)?,
        boundary: parse_boundary(&a.boundary)?,
        neighborhood: match a.nb.as_deref() {
            None => None,
            Some("touch") | Some("t") => Some(Nb::Touch),
            Some("edge") | Some("e") => Some(Nb::Edge),
            Some(x) => bail!("--nb is touch or edge, not {x}"),
        },
        soups: a.soups,
        density: a.density,
        gens: a.gens,
        seed: a.seed,
        max_period: a.max_period,
        explode_fraction: 0.5,
        soup_radius: a.soup_radius,
    };
    let t0 = std::time::Instant::now();
    let quiet = a.quiet;
    let progress = move |d: usize, n: usize| {
        if !quiet && (d.is_multiple_of((n / 100).max(1)) || d == n) {
            eprint!("\r{d}/{n} soups");
            if d == n {
                eprintln!();
            }
        }
    };
    let mut out = msg(search(&spec, Some(&progress)))?;
    let secs = t0.elapsed().as_secs_f64();
    let mut stored = None;
    if !a.no_store {
        let st = msg(lib.store_search(&out))?;
        if let Some((id, e)) = st.failed.first() {
            bail!("storing {id}: {e}");
        }
        for g in &mut out.report.groups {
            for f in &mut g.found {
                f.new = st.added.contains(&f.id);
            }
        }
        stored = Some(st);
    }
    if let Some(p) = &a.json {
        std::fs::write(p, serde_json::to_string_pretty(&out.report)?)
            .with_context(|| format!("writing {}", p.display()))?;
    }
    let r = &out.report;
    println!(
        "search: {} rule(s) x {} tiling(s) x {} soups, {:?} {:?}, density {}, up to {} generations, seeds {}..{}",
        spec.rules.len(),
        spec.tilings.len(),
        spec.soups,
        spec.size,
        spec.boundary,
        spec.density,
        spec.gens,
        spec.seed,
        spec.seed + spec.soups as u64 - 1
    );
    let mut ships = Vec::new();
    for g in &r.groups {
        println!("\n== {} on {}", g.rule, g.tiling);
        if let Some(e) = &g.error {
            println!("   error: {e}");
            continue;
        }
        println!(
            "   soups: {} settled, {} still active, {} exploded; objects: {}",
            g.settled,
            g.active,
            g.exploded,
            g.objects
                .iter()
                .map(|(k, v)| format!("{k} {v}"))
                .collect::<Vec<_>>()
                .join(", ")
        );
        let (mut sl, mut osc, mut sh) = (0, 0, 0);
        for f in &g.found {
            match f.class {
                PatternClass::StillLife => sl += 1,
                PatternClass::Oscillator => osc += 1,
                PatternClass::Spaceship => sh += 1,
                _ => {}
            }
        }
        println!(
            "   distinct: {} still lifes, {} oscillators, {} spaceships",
            sl, osc, sh
        );
        for f in g.found.iter().take(a.top) {
            println!(
                "   {:>6} x  {:<36} {:<11} p{:<3} {:>3}c  {}{}{}",
                f.count,
                f.label,
                f.class.name(),
                f.period,
                f.cells,
                &f.id[..8],
                if f.new { "  NEW" } else { "" },
                f.error
                    .as_deref()
                    .map(|e| format!("  UNVERIFIED: {e}"))
                    .unwrap_or_default()
            );
        }
        if g.found.len() > a.top {
            println!("   ... {} more", g.found.len() - a.top);
        }
        for f in g
            .found
            .iter()
            .filter(|f| f.class == PatternClass::Spaceship)
        {
            ships.push(format!(
                "{} on {}: {} ({}, {} cells, seen {}){}",
                g.rule,
                g.tiling,
                f.id,
                f.speed.as_deref().unwrap_or("?"),
                f.cells,
                f.count,
                if f.new { ", new" } else { "" }
            ));
        }
    }
    println!(
        "\nspaceships: {}",
        if ships.is_empty() {
            "none".into()
        } else {
            ships.len().to_string()
        }
    );
    for s in &ships {
        println!("  {s}");
    }
    if let Some(st) = &stored {
        println!(
            "catalog: {} new pattern(s), {} already known ({} of them now verified in another rule); job {} in {}",
            st.added.len(),
            st.merged.len(),
            st.new_rules.len(),
            st.job,
            lib.path().display()
        );
    }
    println!(
        "time: {secs:.1}s on {} threads",
        rayon::current_num_threads()
    );
    if let Some(p) = &a.json {
        println!("json: {}", p.display());
    }
    Ok(())
}

fn collide_cmd(lib: &Library, c: CollideArgs) -> Result<()> {
    set_threads(c.threads)?;
    let a = msg(lib.get(&c.a))?;
    let b = msg(lib.get(&c.b))?;
    let both = parse_range(&c.offsets)?;
    let phases = if c.phases.eq_ignore_ascii_case("all") {
        vec![]
    } else {
        c.phases
            .split(',')
            .map(|x| x.trim().parse::<u32>())
            .collect::<Result<_, _>>()
            .context("--phases")?
    };
    let spec = CollideSpec {
        dx: c
            .dx
            .as_deref()
            .map(parse_range)
            .transpose()?
            .unwrap_or(both),
        dy: c
            .dy
            .as_deref()
            .map(parse_range)
            .transpose()?
            .unwrap_or(both),
        phases,
        orientation_a: c.orientation_a,
        orientation_b: c.orientation_b,
        gens: c.gens,
        margin: c.margin,
        rule: c.rule.clone(),
        ..Default::default()
    };
    let quiet = c.quiet;
    let progress = move |d: usize, n: usize| {
        if !quiet && (d.is_multiple_of((n / 100).max(1)) || d == n) {
            eprint!("\r{d}/{n} placements");
            if d == n {
                eprintln!();
            }
        }
    };
    let t0 = std::time::Instant::now();
    let thumb = if c.sheet.is_some() { c.thumb } else { 0 };
    let out = msg(collide(&a, &b, &spec, thumb, Some(&progress)))?;
    let r = &out.report;
    println!(
        "collide: A = {} ({}), B = {} ({})",
        r.a_label, r.a, r.b_label, r.b
    );
    println!(
        "rule {} on {}; B offsets dx {:?} dy {:?}, phases {:?}",
        r.rule,
        r.tiling,
        spec.dx,
        spec.dy,
        if spec.phases.is_empty() {
            "all".to_string()
        } else {
            format!("{:?}", spec.phases)
        }
    );
    println!(
        "outcomes: {}",
        r.counts
            .iter()
            .map(|(k, v)| format!("{k} {v}"))
            .collect::<Vec<_>>()
            .join(", ")
    );
    let mut new_ids = Vec::new();
    if !c.no_store && !out.records.is_empty() {
        let job = msg(lib.create_job("collide", &serde_json::to_value(&r.spec)?))?;
        let res = msg(lib.add_many(out.records.clone(), Some(job)))?;
        for (rec, o) in out.records.iter().zip(res) {
            if msg(o)? == AddOutcome::Added {
                new_ids.push(rec.id.clone());
            }
        }
        msg(lib.update_job(job, Some(JobStatus::Done), None, None))?;
    }
    println!(
        "products: {} distinct verified object(s){}",
        out.records.len(),
        if c.no_store {
            String::new()
        } else {
            format!(", {} new to the library", new_ids.len())
        }
    );
    for rec in &out.records {
        println!(
            "  {}  {} (from {} run(s)){}",
            rec.id,
            rec.label(),
            rec.seen,
            if new_ids.contains(&rec.id) {
                "  NEW"
            } else {
                ""
            }
        );
    }
    if let Some(p) = &c.csv {
        let mut s = String::from("dx,dy,phase,outcome,gens,result,products\n");
        for x in &r.runs {
            s.push_str(&format!(
                "{},{},{},{},{},\"{}\",{}\n",
                x.dx,
                x.dy,
                x.phase,
                x.outcome.name(),
                x.gens,
                x.result.replace('"', "'"),
                x.products.join(" ")
            ));
        }
        std::fs::write(p, s).with_context(|| format!("writing {}", p.display()))?;
        println!("csv:   {}", p.display());
    }
    if let Some(p) = &c.json {
        std::fs::write(p, serde_json::to_string_pretty(r)?)
            .with_context(|| format!("writing {}", p.display()))?;
        println!("json:  {}", p.display());
    }
    if let Some(p) = &c.sheet {
        let items: Vec<(sheet::Label, &render::Image)> = out
            .thumbs
            .iter()
            .map(|(l, o, img)| {
                (
                    vec![
                        (l.clone(), [0xd0, 0xd4, 0xdc, 0xff]),
                        (o.name().to_string(), o.color()),
                    ],
                    img,
                )
            })
            .collect();
        let img = sheet::labelled_sheet(&items, c.cols);
        let meta = vec![
            ("jetcarp:version", VERSION.to_string()),
            ("jetcarp:collide", serde_json::to_string(&r.spec)?),
        ];
        msg(write_png(p, &img, &meta))?;
        println!("sheet: {} ({} runs)", p.display(), items.len());
    }
    println!(
        "time: {:.1}s on {} threads",
        t0.elapsed().as_secs_f64(),
        rayon::current_num_threads()
    );
    Ok(())
}

fn paste_cmd(lib: &Library, p: PasteArgs) -> Result<()> {
    struct Place {
        rec: PatternRecord,
        at: PasteAt,
        orientation: u32,
        phase: u32,
    }
    let mut places = Vec::new();
    for s in &p.placements {
        let (id, rest) = s.split_once('@').unwrap_or((s.as_str(), ""));
        let rec = msg(lib.get(id))?;
        let mut parts = rest.split(',').map(str::trim).filter(|x| !x.is_empty());
        let mut at = if rec.site.is_some() {
            PasteAt::Original
        } else {
            PasteAt::Center
        };
        let (mut orientation, mut phase) = (0, 0);
        let first = parts.next();
        if let Some(x) = first {
            if let Some(cell) = x.strip_prefix('#') {
                at = PasteAt::Index {
                    cell: cell.parse().context("cell index after #")?,
                };
            } else {
                let y = parts.next().context("placement is ID@X,Y")?;
                at = PasteAt::Cell {
                    x: x.parse().context("x")?,
                    y: y.parse().context("y")?,
                };
            }
        }
        for q in parts {
            if let Some(o) = q.strip_prefix('o') {
                orientation = o.parse().context("orientation")?;
            } else if let Some(ph) = q.strip_prefix('p') {
                phase = ph.parse().context("phase")?;
            } else {
                bail!("unknown placement option '{q}' (oK = orientation, pK = phase)");
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
        if library::tiling_key(&pl.rec.tiling) != library::tiling_key(&first.tiling) {
            bail!(
                "all patterns must be on one tiling ({} vs {})",
                first.tiling_name,
                pl.rec.tiling_name
            );
        }
    }
    let mut tiling = match &first.site {
        Some(s) => s.patch.clone(),
        None => library::tiling_identity(&first.tiling),
    };
    if first.site.is_none() {
        tiling.size = Some(parse_size(&p.size)?);
        tiling.boundary = parse_boundary(&p.boundary)?;
    }
    let mut sc = Scene::new(tiling);
    sc.name = p
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
        let ca = rules.iter().position(|x| *x == k).unwrap() as u32 + 1;
        sc.commands.push(Command::PastePattern {
            id: ca,
            pattern: library::paste::source_from_record(&pl.rec, pl.phase as u64),
            at: pl.at.clone(),
            orientation: pl.orientation,
            phase: 0,
        });
    }
    let mut w = msg(World::from_scene(&sc))?;
    std::fs::write(&p.out, sc.to_json() + "\n")
        .with_context(|| format!("writing {}", p.out.display()))?;
    println!(
        "scene: {} ({} patterns, {} CA(s), hash {})",
        p.out.display(),
        places.len(),
        sc.cas.len(),
        w.hash_hex()
    );
    if let Some(png) = &p.png {
        if p.gens > 0 {
            msg(w.apply(Command::Step { gens: p.gens }))?;
        }
        let opts = render::RenderOptions::default();
        msg(render::write_png_streaming(
            png,
            &w,
            &opts,
            &render::png_metadata(&w),
        ))?;
        println!("png:   {} (generation {})", png.display(), w.generation);
    }
    Ok(())
}

fn update_cmd(lib: &Library, u: UpdateArgs) -> Result<()> {
    let up = Update {
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
        rating: u.rating,
        state: u
            .state
            .as_deref()
            .map(CurationState::parse)
            .transpose()
            .map_err(anyhow::Error::msg)?,
        discovered_by: u
            .discovered_by
            .as_deref()
            .map(DiscoveredBy::parse)
            .transpose()
            .map_err(anyhow::Error::msg)?,
    };
    let r = msg(lib.update(&u.id, &up))?;
    if u.json {
        println!("{}", r.to_json());
    } else {
        println!("updated {} ({}), state {}", r.id, r.label(), r.state.name());
    }
    Ok(())
}

/// `jetcarp library verify`: verify stored patterns again, in pages of 5,000 (bounded memory).
fn verify_cmd(
    lib: &Library,
    all: bool,
    sample: usize,
    periods: Option<u64>,
    drop: bool,
) -> Result<()> {
    use rayon::prelude::*;
    let t0 = std::time::Instant::now();
    let (mut checked, mut sampled) = (0usize, 0usize);
    let mut failed: Vec<(String, String, String)> = Vec::new();
    let mut by_tiling: std::collections::BTreeMap<String, (usize, usize)> = Default::default();
    let mut offset = 0;
    loop {
        let page = msg(lib.query(&Query {
            order: Some("id".into()),
            limit: Some(5000),
            offset: Some(offset),
            ..Default::default()
        }))?;
        if page.is_empty() {
            break;
        }
        offset += page.len();
        let pick: Vec<PatternRecord> = page
            .into_iter()
            .filter(|r| {
                let main = matches!(r.tiling.family.as_str(), "square" | "hex");
                if all || !main {
                    true
                } else if sampled < sample {
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
                let n = periods.unwrap_or_else(|| jetcarp_core::library::default_periods(r.period));
                let e = jetcarp_core::library::verify(r, n).err();
                (r.id.clone(), r.tiling_name.clone(), e)
            })
            .collect();
        for (id, t, e) in res {
            checked += 1;
            let x = by_tiling.entry(t.clone()).or_default();
            x.0 += 1;
            if let Some(e) = e {
                x.1 += 1;
                failed.push((id, t, e));
            }
        }
        eprint!("\r{checked} checked, {} failed", failed.len());
    }
    eprintln!();
    for (t, (n, f)) in &by_tiling {
        if *f > 0 {
            println!("  {t}: {f} of {n} failed");
        }
    }
    for (id, t, e) in failed.iter().take(5) {
        println!("  e.g. {id} on {t}: {e}");
    }
    println!(
        "{checked} patterns verified ({sampled} of them on squares and hexagons) in {:.0} s: {} failed",
        t0.elapsed().as_secs_f64(),
        failed.len()
    );
    if drop && !failed.is_empty() {
        for (id, _, _) in &failed {
            msg(lib.remove(id))?;
        }
        println!("removed {} patterns", failed.len());
    }
    Ok(())
}
