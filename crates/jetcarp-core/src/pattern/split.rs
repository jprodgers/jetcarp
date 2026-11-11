//! Splitting compound objects into the independent objects they are made of.
//!
//! `objects::find_objects` splits a group into its touching parts when each part repeats alone
//! and the parts together reproduce the group. That fails when one part is itself disconnected in
//! the phase it was found in (a toad in its vertical phase is two pieces, neither of which lives
//! alone), so a toad next to a beehive came out as one "12-cell p2 oscillator". Here every phase
//! of a still life or oscillator is tried: if in some phase its touching components each repeat
//! alone (period dividing the whole object's period) and together reproduce every generation of
//! the object exactly, it is reported as those parts. Lattice tilings only; spaceships are left
//! as they are.

use super::verify::{Frame, envelope, lattice_info, rule_world};
use super::{
    DiscoveredBy, PatternClass, PatternRecord, Provenance, RecordParts, build_record,
    record_from_object,
};
use crate::objects::{Object, Phase};
use crate::world::{Command, World};
use std::collections::HashMap;

/// One part of a compound pattern: class, period, one period of phases.
pub type Part = (PatternClass, u64, Vec<Phase>);

/// The parts of a compound still life or oscillator: (class, period, one period of phases), or
/// `None` when it does not come apart.
pub fn split_compound(rec: &PatternRecord) -> Result<Option<Vec<Part>>, String> {
    if rec.site.is_some()
        || !matches!(
            rec.class,
            PatternClass::StillLife | PatternClass::Oscillator
        )
    {
        return Ok(None);
    }
    let p = rec.period.max(1);
    let phases = rec.all_phases()?;
    let info = lattice_info(&rec.tiling, rec.neighborhood)?;
    let env = envelope(rec, p)?;
    let frame = Frame::around(&rec.tiling, &info, &env, 4 * info.reach + 4)?;
    let g = frame.topo.lattice().unwrap().clone();
    let offs = &g.offsets[rec.neighborhood as usize];
    'phase: for t in 0..p as usize {
        // Touching components of phase t.
        let cells = &phases[t];
        let at: HashMap<(i32, i32, u32), usize> = cells
            .iter()
            .enumerate()
            .map(|(n, c)| ((c.i, c.j, c.k), n))
            .collect();
        let mut comp = vec![usize::MAX; cells.len()];
        let mut comps: Vec<Phase> = Vec::new();
        for s in 0..cells.len() {
            if comp[s] != usize::MAX {
                continue;
            }
            let id = comps.len();
            comp[s] = id;
            let mut stack = vec![s];
            let mut members = Vec::new();
            while let Some(x) = stack.pop() {
                members.push(cells[x]);
                let c = cells[x];
                for o in &offs[c.k as usize] {
                    if let Some(&y) = at.get(&(c.i + o.di, c.j + o.dj, o.k))
                        && comp[y] == usize::MAX
                    {
                        comp[y] = id;
                        stack.push(y);
                    }
                }
            }
            members.sort_unstable();
            comps.push(members);
        }
        if comps.len() < 2 {
            continue;
        }
        // Each component alone for p generations: it must repeat (period dividing p).
        let mut runs: Vec<Vec<Phase>> = Vec::new();
        for c in &comps {
            let mut w: World = rule_world(rec, frame.topo.clone())?;
            let id = w.cas[0].id;
            let fi = w.cas[0].field;
            w.apply(Command::SetCells {
                id,
                cells: frame.cells(c)?,
            })?;
            let mut run = vec![c.clone()];
            for _ in 0..p {
                w.apply(Command::Step { gens: 1 })?;
                let (live, edge) = frame.live(&w.fields[fi].state, rec.neighborhood);
                if live.is_empty() || edge {
                    continue 'phase;
                }
                run.push(live);
            }
            if run[p as usize] != *c {
                continue 'phase;
            }
            run.pop();
            runs.push(run);
        }
        // Together they must give exactly the object, every generation.
        for u in 0..p as usize {
            let mut all: Phase = runs.iter().flat_map(|r| r[u].iter().copied()).collect();
            all.sort_unstable();
            if all != phases[(t + u) % p as usize] {
                continue 'phase;
            }
        }
        let parts = runs
            .into_iter()
            .map(|r| {
                let q = (1..=p as usize)
                    .find(|&q| (p as usize).is_multiple_of(q) && (q == p as usize || r[q] == r[0]))
                    .unwrap();
                let class = if q == 1 {
                    PatternClass::StillLife
                } else {
                    PatternClass::Oscillator
                };
                (class, q as u64, r[..q].to_vec())
            })
            .collect();
        return Ok(Some(parts));
    }
    Ok(None)
}

/// [`record_from_object`], with compound objects split into their independent parts (see the
/// module notes). Parts get no name; a part already in the library keeps its stored name when
/// merged.
pub fn records_from_object(
    world: &World,
    ca_id: u32,
    obj: &Object,
    provenance: Provenance,
    discovered_by: DiscoveredBy,
) -> Result<Vec<PatternRecord>, String> {
    let rec = record_from_object(world, ca_id, obj, provenance.clone(), discovered_by)?;
    if obj.known_name {
        return Ok(vec![rec]);
    }
    let Some(parts) = split_compound(&rec)? else {
        return Ok(vec![rec]);
    };
    let ca = world.ca(ca_id).ok_or("no such CA")?;
    let mut out = Vec::new();
    for (class, period, phases) in parts {
        let what = RecordParts {
            rule: rec.rule.clone(),
            nb: rec.neighborhood,
            steps_per_gen: rec.steps_per_gen,
            isotropic: crate::objects::supported(&ca.rule)?,
            class,
            period,
            d: [0, 0],
            name: None,
        };
        // Equal parts (two blocks) each count.
        out.push(build_record(
            &world.topo,
            &what,
            &phases,
            provenance.clone(),
            discovered_by,
        )?);
    }
    Ok(out)
}
