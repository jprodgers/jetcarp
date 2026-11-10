//! Batch runs: many worlds on one topology, each with its own rule and starting states, stepped
//! together and measured every generation.
//!
//! [`run_cpu`] is the reference implementation. A backend (the GPU batch stepper in `jetcarp-gpu`)
//! must produce exactly the same [`WorldTrace`] for every world it accepts.

use crate::ltl::{Hood, Reach};
use crate::rules::{FieldKind, Rule};
use crate::topology::{Csr, Nb, Storage, Topology};
use crate::world::{CaSpec, Field, SeedSpec, World};
use rayon::prelude::*;
use std::sync::Arc;

/// The per-cell constant of the generation hash.
#[inline]
pub fn mix(i: u32) -> u32 {
    (i.wrapping_mul(0x9E37_79B1) ^ (i >> 7)) | 1
}

/// One world in a batch.
#[derive(Clone, Debug)]
pub struct BatchWorld {
    pub rule: Rule,
    /// Starting state of every cell (the topology's length).
    pub init: Vec<u8>,
}

/// Many worlds on one topology. All share the neighbourhood: `nb` for every rule except Larger
/// than Life, whose rules must all have the same hood and range in one batch.
#[derive(Clone, Debug)]
pub struct BatchJob {
    pub topo: Arc<Topology>,
    pub nb: Nb,
    pub worlds: Vec<BatchWorld>,
    /// Generations to run.
    pub gens: u32,
    /// Generations (ascending, at most `gens`; 0 is the start) to return full frames for.
    pub frames_at: Vec<u32>,
}

/// What one world did. The three series have `gens + 1` entries: index `t` describes generation
/// `t`, and `changed[0]` is 0.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct WorldTrace {
    /// Cells in a nonzero state.
    pub pop: Vec<u32>,
    /// Cells whose state differs from the previous generation.
    pub changed: Vec<u32>,
    /// Wrapping sum over cells of `(state + 1) * mix(cell)`.
    pub hash: Vec<u32>,
    /// One frame per `frames_at`, one byte per cell.
    pub frames: Vec<Vec<u8>>,
}

impl WorldTrace {
    /// An empty trace with room for `gens` generations.
    pub fn with_capacity(gens: u32) -> WorldTrace {
        let n = gens as usize + 1;
        WorldTrace {
            pop: Vec::with_capacity(n),
            changed: Vec::with_capacity(n),
            hash: Vec::with_capacity(n),
            frames: Vec::new(),
        }
    }
}

/// `(pop, hash)` of a state array.
pub fn measure(state: &[u8]) -> (u32, u32) {
    let (mut pop, mut hash) = (0u32, 0u32);
    for (i, &s) in state.iter().enumerate() {
        pop += (s != 0) as u32;
        hash = hash.wrapping_add((s as u32 + 1).wrapping_mul(mix(i as u32)));
    }
    (pop, hash)
}

/// A field as one byte per cell: states as they are, sandpile heights capped, and continuous
/// values in 16 levels (0 only when the value is below 1/255).
pub fn field_bytes(f: &Field) -> Vec<u8> {
    match f.kind {
        FieldKind::Height => f.height.iter().map(|&h| h.min(255) as u8).collect(),
        FieldKind::Continuous => f
            .value
            .iter()
            .map(|&v| {
                if v < 1.0 / 255.0 {
                    0
                } else {
                    1 + (v * 15.0).min(15.0) as u8
                }
            })
            .collect(),
        _ => f.state.clone(),
    }
}

/// The neighbour lists a batch's rules use: `nb`, or the Larger than Life reach. Cells past a
/// dead edge are left out.
pub fn neighbor_csr(topo: &Topology, nb: Nb, ltl: Option<(Hood, u8)>) -> Result<Csr, String> {
    let n = topo.len();
    let lists: Vec<Vec<u32>> = match ltl {
        None => (0..n)
            .into_par_iter()
            .map(|i| {
                let mut v = Vec::new();
                topo.for_each_neighbor(i, nb, |x| v.push(x as u32));
                v
            })
            .collect(),
        Some((hood, r)) => match (crate::ltl::reach(topo, hood, r)?, &topo.storage) {
            (Reach::Explicit(c), _) => return Ok(c),
            (Reach::Lattice(per), Storage::Lattice(g)) => (0..n)
                .into_par_iter()
                .map(|i| {
                    let (u, j, k) = g.decompose(i);
                    let base = u + g.row_start[j as usize];
                    per[k as usize]
                        .iter()
                        .filter_map(|o| g.resolve(base + o.di, j + o.dj, o.k))
                        .map(|x| x as u32)
                        .collect()
                })
                .collect(),
            _ => return Err("reach and storage disagree".into()),
        },
    };
    Ok(Csr::from_lists(&lists))
}

/// Step one prepared world (one running CA, on field 0) and measure it.
pub fn trace_world(world: &mut World, gens: u32, frames_at: &[u32]) -> Result<WorldTrace, String> {
    if world.fields.is_empty() {
        return Err("the world has no field".into());
    }
    let mut tr = WorldTrace::with_capacity(gens);
    let mut prev = field_bytes(&world.fields[0]);
    let (p, h) = measure(&prev);
    tr.pop.push(p);
    tr.changed.push(0);
    tr.hash.push(h);
    let mut next_frame = 0;
    if frames_at.first() == Some(&0) {
        tr.frames.push(prev.clone());
        next_frame = 1;
    }
    for t in 1..=gens {
        world.step();
        let s = field_bytes(&world.fields[0]);
        let (p, h) = measure(&s);
        tr.pop.push(p);
        tr.hash.push(h);
        tr.changed
            .push(s.iter().zip(&prev).filter(|(a, b)| a != b).count() as u32);
        prev.copy_from_slice(&s);
        if frames_at.get(next_frame) == Some(&t) {
            tr.frames.push(prev.clone());
            next_frame += 1;
        }
    }
    Ok(tr)
}

/// The reference implementation: every world on the CPU.
pub fn run_cpu(job: &BatchJob) -> Result<Vec<WorldTrace>, String> {
    job.worlds
        .par_iter()
        .map(|w| {
            let mut world = World::on_topology(job.topo.clone(), 1);
            // The default seed (a WireWorld demo, say) may not exist on this tiling.
            world.add_ca(
                CaSpec::new(&w.rule.notation())
                    .nb(job.nb)
                    .seed(SeedSpec::Empty),
            )?;
            let f = &mut world.fields[0];
            if w.init.len() != f.state.len() {
                return Err("init has the wrong length".to_string());
            }
            f.state.copy_from_slice(&w.init);
            trace_world(&mut world, job.gens, &job.frames_at)
        })
        .collect()
}
