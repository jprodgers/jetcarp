//! Verified gliders from a search into the pattern library: exact spaceships on lattices (the
//! object detector and the library's recurrence check), movers verified to a distance on
//! aperiodic patches (the mover tracker).

use super::arena::{Arena, is_aperiodic};
use super::{Champion, EvolveReport};
use crate::library::movers::{MoverReport, TrackSpec, mover_record, track};
use crate::library::split::records_from_object;
use crate::library::{AddOutcome, DiscoveredBy, Library, Provenance, now_utc};
use crate::objects::{ObjClass, ObjectOptions, find_objects};
use crate::world::{CaSpec, Command, SeedSpec, World};
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
    tiling: &crate::topology::TilingSpec,
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
        jetcarp_version: crate::VERSION.into(),
        date: now_utc(),
    }
}

/// Catalog every verified glider of a search (`lib = None`: classify only, store nothing).
pub fn catalog_gliders(
    report: &EvolveReport,
    lib: Option<&Library>,
    track_gens: u64,
) -> Result<Vec<FoundMover>, String> {
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
        let f = if is_aperiodic(&spec.tiling.family) {
            aperiodic_mover(report, c, &tiling, cells, lib, track_gens)
        } else {
            lattice_ships(report, c, &arena, cells, lib)
        };
        let f = f.unwrap_or_else(|e| FoundMover {
            rule: c.rule.clone(),
            kind: "none".into(),
            ids: vec![],
            added: vec![],
            summary: format!("could not catalog: {e}"),
            tracker: None,
        });
        out.push(f);
    }
    Ok(out)
}

/// Follow the seed with the mover tracker over growing patch levels; store it if verified.
fn aperiodic_mover(
    report: &EvolveReport,
    c: &Champion,
    tiling: &crate::topology::TilingSpec,
    cells: Vec<(u32, u8)>,
    lib: Option<&Library>,
    track_gens: u64,
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
        if let Some(l) = lib
            && l.add(rec.clone())? == AddOutcome::Added
        {
            f.added.push(rec.id.clone());
        }
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
    lib: Option<&Library>,
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
            if let Some(l) = lib
                && l.add(r.clone())? == AddOutcome::Added
            {
                f.added.push(r.id.clone());
            }
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
