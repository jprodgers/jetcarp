//! `jetcarp movers`: follow a pattern across growing aperiodic patches and report how far it was
//! verified (`jetcarp_core::library::movers`); store verified movers in the pattern library.

use crate::evolve_cmd::parse_nb;
use anyhow::{Result, anyhow, bail};
use jetcarp_core::library::movers::{MoverReport, TrackSpec, mover_record, retrack, track};
use jetcarp_core::library::{
    DiscoveredBy, Library, PatternClass, Provenance, Query, now_utc, sort_records,
};
use jetcarp_core::topology::{Nb, SizeSpec, TilingSpec};
use jetcarp_core::window::track::{Tracker, WindowOpts};
use std::path::PathBuf;

#[derive(clap::Args)]
pub struct MoversArgs {
    /// Catalog: a .sqlite file or a folder (default: $JETCARP_LIBRARY, else
    /// <data>/catalog.sqlite).
    #[arg(long, global = true)]
    library: Option<PathBuf>,
    #[command(subcommand)]
    cmd: MoversCmd,
}

#[derive(clap::Subcommand)]
enum MoversCmd {
    /// Track a pattern over growing patch levels and report the distance it was verified to.
    Track(Box<TrackArgs>),
    /// Track a library mover again from its stored site and update its record.
    Verify {
        id: String,
        #[arg(long, default_value_t = 20000)]
        gens: u64,
    },
    /// List the movers in the library (farthest verified first).
    List {
        #[arg(long)]
        json: bool,
    },
    /// Continue a stopped sliding-window run (a catalog job of kind `mover`).
    Resume { job: i64 },
}

#[derive(clap::Args)]
struct TrackArgs {
    /// Rule notation (SpectreGlider, "Priority 4: ...", B/S).
    #[arg(long, default_value = "SpectreGlider")]
    rule: String,
    /// Aperiodic tiling: spectre, hat, penrose-kd, penrose-rhomb.
    #[arg(long, default_value = "spectre")]
    tiling: String,
    /// Patch level the cells are given on (the tracker continues on every larger level).
    #[arg(long, default_value_t = 4)]
    level: u32,
    /// Largest level to use (default: 7 on the Spectre, where 8 is on request; the largest elsewhere).
    #[arg(long)]
    max_level: Option<u32>,
    #[arg(long, default_value = "touch")]
    nb: String,
    /// Starting cells as CELL:STATE pairs ("1203:1,1204:3").
    #[arg(long)]
    cells: Option<String>,
    /// Instead of --cells: put these states (comma separated, "1,3") on the tile --at and its
    /// edge neighbours in order; with no --at, try pairs around the patch's deepest tile until
    /// one is verified.
    #[arg(long)]
    states: Option<String>,
    /// The tile for --states.
    #[arg(long)]
    at: Option<u32>,
    /// Generation limit over all levels (default 20000; with --window --rings, 100 per ring
    /// more).
    #[arg(long)]
    gens: Option<u64>,
    /// Stop as grown above this many live cells.
    #[arg(long, default_value_t = 2000)]
    max_pop: usize,
    /// Add the mover to the library when verified.
    #[arg(long)]
    store: bool,
    /// Write the full report as JSON.
    #[arg(long)]
    json_out: Option<PathBuf>,
    /// Follow the pattern on a sliding window (supertiles generated around it) instead of fixed
    /// patch levels, as far as --rings or --gens allow.
    #[arg(long)]
    window: bool,
    /// With --window: stop, verified, at this many edge rings.
    #[arg(long)]
    rings: Option<u32>,
    /// With --window: world units of tiles kept around the live cells.
    #[arg(long, default_value_t = 24.0)]
    margin: f64,
    /// With --window (Spectre): follow the fixed patches' Delta chain up to this level.
    #[arg(long, default_value_t = 8)]
    fixed_levels: u32,
    /// With --window: run as a resumable catalog job (kind `mover`; `jetcarp movers resume JOB`).
    #[arg(long)]
    job: bool,
}

fn open_lib(p: &Option<PathBuf>) -> Library {
    match p {
        Some(p) => Library::open(p),
        None => Library::open_default(),
    }
}

pub fn run(a: MoversArgs) -> Result<()> {
    let lib = open_lib(&a.library);
    match a.cmd {
        MoversCmd::Track(t) => track_cmd(*t, &lib),
        MoversCmd::Verify { id, gens } => {
            let rec = lib.get(&id).map_err(|e| anyhow!(e))?;
            if rec.class != PatternClass::Mover {
                bail!("{} is a {}, not a mover", rec.id, rec.class.name());
            }
            let (spec, rep) = retrack(&rec, Some(gens)).map_err(|e| anyhow!(e))?;
            println!("{}", rep.summary);
            print_levels(&rep);
            if rep.verified {
                let mut fresh =
                    mover_record(&spec, &rep, rec.provenance.clone(), rec.discovered_by)
                        .map_err(|e| anyhow!(e))?;
                fresh.seen = 0;
                lib.add(fresh).map_err(|e| anyhow!(e))?;
            }
            Ok(())
        }
        MoversCmd::List { json } => {
            let mut v = lib
                .query(&Query {
                    class: Some(PatternClass::Mover),
                    ..Default::default()
                })
                .map_err(|e| anyhow!(e))?;
            sort_records(&mut v);
            v.sort_by_key(|r| std::cmp::Reverse(r.mover.as_ref().map(|m| m.distance_rings)));
            if json {
                println!("{}", serde_json::to_string_pretty(&v)?);
                return Ok(());
            }
            for r in &v {
                let m = r.mover.as_ref();
                println!(
                    "{}  {:<10} {:>4} rings {:>8.1} units {:>6} gens  L{}  {}",
                    r.id,
                    r.tiling.family,
                    m.map(|m| m.distance_rings).unwrap_or(0),
                    m.map(|m| m.distance_euclid).unwrap_or(0.0),
                    m.map(|m| m.generations).unwrap_or(0),
                    m.map(|m| m.level).unwrap_or(0),
                    r.rule
                );
            }
            println!("{} movers", v.len());
            Ok(())
        }
        MoversCmd::Resume { job } => run_mover_job(&lib, job),
    }
}

fn gens_limit(a: &TrackArgs) -> u64 {
    a.gens.unwrap_or(match (a.window, a.rings) {
        (true, Some(r)) => r as u64 * 100 + 20_000,
        _ => 20_000,
    })
}

fn window_opts(a: &TrackArgs) -> WindowOpts {
    WindowOpts {
        margin: a.margin,
        fixed_levels: a.fixed_levels,
        target_rings: a.rings,
    }
}

/// Does the start look like a mover on a window (it travels for a few hundred generations)?
fn window_start_moves(spec: &TrackSpec, opts: &WindowOpts) -> bool {
    let mut s = spec.clone();
    s.max_gens = s.max_gens.min(600);
    let Ok(mut t) = Tracker::new(&s, opts) else {
        return false;
    };
    match t.run(None, None) {
        Ok(Some(st)) => st.verifies() && t.rings() >= 2,
        _ => false,
    }
}

/// Follow a pattern on a sliding window in this process, printing progress.
fn run_window(spec: &TrackSpec, opts: &WindowOpts) -> Result<MoverReport> {
    let mut t = Tracker::new(spec, opts).map_err(|e| anyhow!(e))?;
    let start = std::time::Instant::now();
    loop {
        let until = std::time::Instant::now() + std::time::Duration::from_secs(5);
        let done = t.run(Some(until), None).map_err(|e| anyhow!(e))?;
        let secs = start.elapsed().as_secs_f64().max(1e-9);
        eprintln!(
            "  generation {}: {} rings, level {}, {} tiles, {:.0} gens/s, {:.0} rings/s",
            t.generation(),
            t.rings(),
            t.level(),
            t.window().len(),
            t.generation() as f64 / secs,
            t.rings() as f64 / secs
        );
        if done.is_some() {
            return Ok(t.report());
        }
    }
}

fn run_mover_job(lib: &Library, job: i64) -> Result<()> {
    use jetcarp_core::window::job as mj;
    println!("mover job {job} in {}", lib.path().display());
    let h = mj::MoverHandle::spawn(lib.clone(), job);
    while !h.is_finished() {
        std::thread::sleep(std::time::Duration::from_secs(5));
        eprintln!("  {}", h.progress().line());
    }
    let p = h.join().map_err(|e| anyhow!(e))?;
    println!("{}", p.line());
    if let Some(r) = &p.record {
        println!("library: {r}");
    }
    if let Some(e) = &p.error {
        println!("error: {e}");
    }
    Ok(())
}

fn print_levels(rep: &jetcarp_core::library::movers::MoverReport) {
    for l in &rep.levels {
        println!(
            "  level {} ({} tiles): {} generations, {:?}, {} rings, population {}-{}{}, {:.1}s",
            l.level,
            l.tiles,
            l.generations,
            l.stop,
            l.reach.rings,
            l.min_population,
            l.max_population,
            match l.agrees_with_previous {
                Some(true) => ", agrees with the smaller level",
                Some(false) => ", DISAGREES with the smaller level",
                None => "",
            },
            l.seconds
        );
    }
    for g in &rep.groups {
        println!(
            "  group of {} cells at {:.1} units, heading {:.1} degrees",
            g.cells, g.euclid, g.heading_deg
        );
    }
}

fn parse_cells(s: &str) -> Result<Vec<(u32, u8)>> {
    s.split([',', ' ', ';'])
        .filter(|t| !t.is_empty())
        .map(|t| {
            let (c, v) = t
                .split_once(':')
                .ok_or_else(|| anyhow!("cells are CELL:STATE, got '{t}'"))?;
            Ok((c.trim().parse()?, v.trim().parse()?))
        })
        .collect()
}

fn track_cmd(a: TrackArgs, lib: &Library) -> Result<()> {
    let nb = parse_nb(&a.nb)?;
    let patch = TilingSpec::new(&a.tiling).with_size(SizeSpec::Level(a.level));
    let topo = jetcarp_core::library::site::patch_topology(&patch).map_err(|e| anyhow!(e))?;
    let starts: Vec<Vec<(u32, u8)>> = if let Some(c) = &a.cells {
        vec![parse_cells(c)?]
    } else {
        let states: Vec<u8> = a
            .states
            .as_deref()
            .unwrap_or("1,3")
            .split(',')
            .map(|x| x.trim().parse::<u8>())
            .collect::<Result<_, _>>()?;
        let roots: Vec<usize> = match a.at {
            Some(x) => vec![x as usize],
            None => {
                let (deep, _) = jetcarp_core::evolve::arena::deepest(&topo);
                let mut near: Vec<usize> = (0..topo.len()).collect();
                let c = topo.centroid(deep);
                near.sort_by(|&p, &q| {
                    jetcarp_core::geom::dist(topo.centroid(p), c)
                        .total_cmp(&jetcarp_core::geom::dist(topo.centroid(q), c))
                });
                near.truncate(12);
                near
            }
        };
        let mut v = Vec::new();
        for r in roots {
            for k in 0..topo.degree(r, Nb::Edge) {
                if let Some(u) = topo.slot(r, Nb::Edge, k) {
                    let mut cells = vec![(r as u32, states[0])];
                    if let Some(&s) = states.get(1) {
                        cells.push((u as u32, s));
                    }
                    v.push(cells);
                }
                if states.len() < 2 {
                    break;
                }
            }
        }
        v
    };
    let mut last = None;
    if a.window {
        let opts = window_opts(&a);
        let pick = starts.iter().find(|cells| {
            let spec = TrackSpec::new(&a.rule, nb, patch.clone(), (*cells).clone());
            starts.len() == 1 || window_start_moves(&spec, &opts)
        });
        let cells = pick
            .ok_or_else(|| anyhow!("no start travels on a window"))?
            .clone();
        let mut spec = TrackSpec::new(&a.rule, nb, patch.clone(), cells);
        spec.max_gens = gens_limit(&a);
        spec.max_pop = a.max_pop;
        println!(
            "start: {}",
            spec.cells
                .iter()
                .map(|(c, s)| format!("{c}:{s}"))
                .collect::<Vec<_>>()
                .join(",")
        );
        if a.job {
            let js = jetcarp_core::window::job::MoverJobSpec {
                track: spec,
                window: opts,
                store: a.store,
                source: "jetcarp movers track --window --job".into(),
            };
            let job = jetcarp_core::window::job::start(lib, &js).map_err(|e| anyhow!(e))?;
            return run_mover_job(lib, job);
        }
        let rep = run_window(&spec, &opts)?;
        last = Some((spec, rep));
    }
    for cells in if a.window { vec![] } else { starts } {
        let mut spec = TrackSpec::new(&a.rule, nb, patch.clone(), cells);
        spec.max_level = a.max_level;
        spec.max_gens = gens_limit(&a);
        spec.max_pop = a.max_pop;
        let rep = match track(&spec) {
            Ok(r) => r,
            Err(e) => {
                eprintln!("{e}");
                continue;
            }
        };
        let ok = rep.verified;
        last = Some((spec, rep));
        if ok || a.cells.is_some() || a.at.is_some() {
            break;
        }
    }
    let (spec, rep) = last.ok_or_else(|| anyhow!("no start could be tracked"))?;
    if !a.window {
        println!(
            "start: {}",
            spec.cells
                .iter()
                .map(|(c, s)| format!("{c}:{s}"))
                .collect::<Vec<_>>()
                .join(",")
        );
    }
    println!("{}", rep.summary);
    print_levels(&rep);
    println!("{}", rep.note);
    if let Some(p) = &a.json_out {
        std::fs::write(p, serde_json::to_string_pretty(&rep)? + "\n")?;
        println!("wrote {}", p.display());
    }
    if a.store && rep.verified {
        let prov = Provenance {
            source: "jetcarp movers track".into(),
            tiling: Some(patch.clone()),
            jetcarp_version: jetcarp_core::VERSION.into(),
            date: now_utc(),
            ..Default::default()
        };
        let rec = mover_record(&spec, &rep, prov, DiscoveredBy::User).map_err(|e| anyhow!(e))?;
        let id = rec.id.clone();
        let out = lib.add(rec).map_err(|e| anyhow!(e))?;
        println!("library: {id} ({out:?}) in {}", lib.path().display());
    }
    Ok(())
}
