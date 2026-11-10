//! Sandpile tools: stabilisation and the identity element of the sandpile group.
//!
//! Grains leave through sinks: slots past a dead lattice edge, and the open boundary cells of an
//! aperiodic patch. A wrapped grid has no sink, so it has no identity.

use crate::rules::FieldKind;
use crate::step::step_sandpile;
use crate::topology::{Nb, Topology};
use crate::world::Field;

/// A height field is stable when no cell can topple: every cell holds fewer grains than it has
/// neighbours, and (on aperiodic patches) the open boundary cells, which drain, are empty.
pub fn is_stable(topo: &Topology, nb: Nb, f: &Field) -> bool {
    if f.kind != FieldKind::Height {
        return true;
    }
    let explicit = matches!(topo.storage, crate::topology::Storage::Explicit(_));
    (0..topo.len()).all(|i| {
        let h = f.height[i] as usize;
        if explicit && topo.is_open(i) {
            return h == 0;
        }
        let d = topo.degree(i, nb);
        d == 0 || h < d
    })
}

/// Topple until stable. Returns the number of sweeps.
pub fn stabilize(topo: &Topology, nb: Nb, f: &mut Field) -> Result<u64, String> {
    if !topo.has_boundary() {
        // Without a sink an over-full torus never stabilises.
        let total: u64 = f.height.iter().map(|&h| h as u64).sum();
        let cap: u64 = (0..topo.len())
            .map(|i| topo.degree(i, nb).saturating_sub(1) as u64)
            .sum();
        if total > cap {
            return Err("too many grains to stabilise on a wrapped grid (no sink)".into());
        }
    }
    let mut sweeps = 0;
    while step_sandpile(topo, nb, f) > 0 {
        sweeps += 1;
        if sweeps > 50_000_000 {
            return Err("did not stabilise".into());
        }
    }
    Ok(sweeps)
}

/// Largest stable height of each cell (degree − 1; sinks hold nothing).
fn max_stable(topo: &Topology, nb: Nb) -> Vec<u32> {
    (0..topo.len())
        .map(|i| {
            if topo.is_open(i) {
                0
            } else {
                topo.degree(i, nb).saturating_sub(1) as u32
            }
        })
        .collect()
}

/// The identity of the sandpile group: stab(2δ − stab(2δ)), δ the maximal stable configuration.
pub fn identity(topo: &Topology, nb: Nb) -> Result<Vec<u32>, String> {
    if !topo.has_boundary() {
        return Err("the sandpile identity needs dead edges: a wrapped grid has no sink".into());
    }
    let delta = max_stable(topo, nb);
    let mut f = Field::new(FieldKind::Height, topo.len(), false);
    f.height = delta.iter().map(|d| 2 * d).collect();
    stabilize(topo, nb, &mut f)?;
    let c1 = f.height.clone();
    f.height = delta.iter().zip(&c1).map(|(d, c)| 2 * d - c).collect();
    f.lost = 0;
    stabilize(topo, nb, &mut f)?;
    Ok(f.height)
}
