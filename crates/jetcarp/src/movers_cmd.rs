//! `jetcarp library movers`: follow a pattern across growing aperiodic patches and report how far
//! it was verified (`jetcarp_core::library::movers`), track a library mover again, and list the
//! movers. Following one far on a sliding window is a job: `jetcarp search mover`.

use anyhow::{Result, anyhow, bail};
use jetcarp_api::Ctx;
use jetcarp_api::args;
use jetcarp_core::library::movers::{TrackSpec, mover_record, retrack, track};
use jetcarp_core::library::{
    DiscoveredBy, Library, PatternClass, Provenance, Query, now_utc, sort_records,
};
use jetcarp_core::topology::{Nb, SizeSpec, TilingSpec};
use std::path::PathBuf;

#[derive(clap::Args)]
pub struct MoversArgs {
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
    /// Generation limit over all levels (default 20000).
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
}

/// `--nb`: touch or edge.
fn parse_nb(s: &str) -> Result<Nb> {
    args::nb_word(s).ok_or_else(|| {
        anyhow!(
            "neighbourhood '{}' is not touch or edge",
            s.trim().to_ascii_lowercase()
        )
    })
}

pub fn run(cx: &Ctx, a: MoversArgs, json: bool) -> Result<()> {
    let lib = cx.library();
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
        MoversCmd::List { json: j } => {
            let json = json || j;
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
    }
}

fn gens_limit(a: &TrackArgs) -> u64 {
    a.gens.unwrap_or(20_000)
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
    for cells in starts {
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
    println!(
        "start: {}",
        spec.cells
            .iter()
            .map(|(c, s)| format!("{c}:{s}"))
            .collect::<Vec<_>>()
            .join(",")
    );
    println!("{}", rep.summary);
    print_levels(&rep);
    println!("{}", rep.note);
    if let Some(p) = &a.json_out {
        std::fs::write(p, serde_json::to_string_pretty(&rep)? + "\n")?;
        println!("wrote {}", p.display());
    }
    if a.store && rep.verified {
        let prov = Provenance {
            source: "jetcarp library movers track".into(),
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
