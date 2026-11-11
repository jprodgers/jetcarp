//! Verified gliders from a search into the pattern library: exact spaceships on lattices (the
//! object detector and the library's recurrence check), movers verified to a distance on
//! aperiodic patches (the mover tracker). [`glider_records`] classifies them and gives the
//! records to store; an evolve job (`crate::jobs::evolve`) stores them through its sink,
//! credited to the job.

use jetcarp_core::evolve::arena::{Arena, is_aperiodic};
use jetcarp_core::evolve::{Champion, EvolveReport};
use jetcarp_core::library::prepare;
use jetcarp_core::objects::{ObjClass, ObjectOptions, find_objects};
use jetcarp_core::pattern::movers::{MoverReport, TrackSpec, mover_record, track};
use jetcarp_core::pattern::split::records_from_object;
use jetcarp_core::pattern::{DiscoveredBy, PatternRecord, Provenance, now_utc};
use jetcarp_core::world::{CaSpec, Command, SeedSpec, World};
use serde::{Deserialize, Serialize};

/// One verified glider's fate.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FoundMover {
    pub rule: String,
    /// `spaceship` (lattice, exact), `mover` (aperiodic, verified to a distance), or `none`.
    pub kind: String,
    /// Library ids stored (or that would be stored).
    pub ids: Vec<String>,
    /// Ids that were new to the library.
    pub added: Vec<String>,
    pub summary: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tracker: Option<MoverReport>,
}

fn provenance(
    report: &EvolveReport,
    c: &Champion,
    tiling: &jetcarp_core::topology::TilingSpec,
) -> Provenance {
    Provenance {
        source: format!(
            "evolve seed {} gen {} (seed bank #{})",
            report.spec.seed, c.first_generation, c.seed_index
        ),
        tiling: Some(tiling.clone()),
        rng_state: Some(report.spec.seed),
        soup: None,
        density: None,
        generation: None,
        jetcarp_version: jetcarp_core::VERSION.into(),
        date: now_utc(),
    }
}

/// A glider that could not be cataloged.
pub fn not_cataloged(rule: &str, e: &str) -> FoundMover {
    FoundMover {
        rule: rule.to_string(),
        kind: "none".into(),
        ids: vec![],
        added: vec![],
        summary: format!("could not catalog: {e}"),
        tracker: None,
    }
}

/// Every verified glider of a search classified (an exact spaceship on a lattice, a mover verified
/// to a distance on an aperiodic patch, or neither), with the records to store for it, in order
/// (`added` left empty). With `ready`, the records are made ready to store (verified, their rule
/// ranges computed: [`jetcarp_core::library::prepare`]); a glider whose record cannot be is
/// reported as not cataloged, with the records before it.
pub fn glider_records(
    report: &EvolveReport,
    track_gens: u64,
    ready: bool,
) -> Result<Vec<(FoundMover, Vec<PatternRecord>)>, String> {
    let mut out = Vec::new();
    if report.gliders.is_empty() {
        return Ok(out);
    }
    let spec = &report.spec;
    let arena = Arena::build_with_room(
        &spec.tiling,
        spec.nb,
        spec.radius,
        spec.radius * spec.verify_factors.iter().copied().max().unwrap_or(1),
    )?;
    for c in &report.gliders {
        let cells: Vec<(u32, u8)> = c
            .seed
            .iter()
            .map(|&(x, s)| (arena.cell_of[x as usize], s))
            .collect();
        let tiling = arena.tiling.clone();
        let mut recs = Vec::new();
        let f = if is_aperiodic(&spec.tiling.family) {
            aperiodic_mover(report, c, &tiling, cells, track_gens, ready, &mut recs)
        } else {
            lattice_ships(report, c, &arena, cells, ready, &mut recs)
        };
        let f = f.unwrap_or_else(|e| not_cataloged(&c.rule, &e));
        out.push((f, recs));
    }
    Ok(out)
}

/// Follow the seed with the mover tracker over growing patch levels; its record if verified.
fn aperiodic_mover(
    report: &EvolveReport,
    c: &Champion,
    tiling: &jetcarp_core::topology::TilingSpec,
    cells: Vec<(u32, u8)>,
    track_gens: u64,
    ready: bool,
    recs: &mut Vec<PatternRecord>,
) -> Result<FoundMover, String> {
    let spec = &report.spec;
    let mut ts = TrackSpec::new(&c.rule, spec.nb, tiling.clone(), cells);
    ts.max_gens = track_gens;
    let rep = track(&ts)?;
    let mut f = FoundMover {
        rule: c.rule.clone(),
        kind: if rep.verified { "mover" } else { "none" }.into(),
        ids: vec![],
        added: vec![],
        summary: rep.summary.clone(),
        tracker: None,
    };
    if rep.verified {
        let rec = mover_record(
            &ts,
            &rep,
            provenance(report, c, tiling),
            DiscoveredBy::Search,
        )?;
        f.ids.push(rec.id.clone());
        recs.push(if ready { prepare(rec)? } else { rec });
    }
    f.tracker = Some(rep);
    Ok(f)
}

/// Run the seed on the arena's dead-edged grid until it is well away from the start, then take
/// the exact spaceships the object detector finds.
fn lattice_ships(
    report: &EvolveReport,
    c: &Champion,
    arena: &Arena,
    cells: Vec<(u32, u8)>,
    ready: bool,
    recs: &mut Vec<PatternRecord>,
) -> Result<FoundMover, String> {
    let mut w = World::on_topology(arena.topo.clone(), 1);
    w.apply(Command::AddCa {
        ca: CaSpec::new(&c.rule)
            .nb(report.spec.nb)
            .seed(SeedSpec::Empty),
    })?;
    let id = w.cas[0].id;
    w.apply(Command::SetCells {
        id,
        cells: cells.iter().map(|&(x, s)| [x, s as u32]).collect(),
    })?;
    // Run until the activity has crossed about two thirds of the arena (the GA run reached the
    // boundary ring at `stats.generation`).
    let gens = (c.stats.generation * 2 / 3).max(8);
    w.apply(Command::Step { gens })?;
    let rep = find_objects(
        &w,
        &ObjectOptions {
            max_period: 256,
            ..Default::default()
        },
    )?;
    let prov = provenance(report, c, &arena.topo.spec);
    let mut f = FoundMover {
        rule: c.rule.clone(),
        kind: "none".into(),
        ids: vec![],
        added: vec![],
        summary: String::new(),
        tracker: None,
    };
    let mut kinds = Vec::new();
    for o in &rep.objects {
        if !matches!(o.class, ObjClass::Spaceship { .. }) {
            continue;
        }
        for r in records_from_object(&w, rep.ca_id, o, prov.clone(), DiscoveredBy::Search)? {
            if f.ids.contains(&r.id) {
                continue;
            }
            kinds.push(format!(
                "{} {} ({} cells, period {})",
                r.speed.clone().unwrap_or_default(),
                r.class.name(),
                r.cells,
                r.period
            ));
            f.ids.push(r.id.clone());
            recs.push(if ready { prepare(r)? } else { r });
        }
    }
    if f.ids.is_empty() {
        f.summary = format!(
            "no exact spaceship after {gens} generations (objects: {})",
            rep.summary_line()
        );
    } else {
        f.kind = "spaceship".into();
        f.summary = format!("exact spaceship(s): {}", kinds.join(", "));
    }
    Ok(f)
}
