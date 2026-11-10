//! Completion detection: is there anything left for the running CAs to do?
//!
//! [`World::done`] is true when no enabled CA can change the world any more:
//!
//! - an elementary (`W`, `HexW`, `HexW3`, `HexT`, `HexT3`) rule has written the bottom row
//!   (for `HexT`, the bottom half-row layer): the picture fills the grid once and stops, on
//!   any kind of edge;
//! - a layered rule (`Layer`) has written its last layer (the farthest distance from the seed);
//! - a sandpile is stable (no cell can topple) and drops no grains per generation;
//! - all of a turmite's ants have fallen off;
//! - a field rule whose empty field stays empty (Life-like and Generations without B0, WireWorld,
//!   cyclic, Greenberg-Hastings, hodgepodge, Larger than Life without B0, the Spectre glider rule,
//!   von Neumann's 29 states, Margolus rules that keep an empty block empty) has an empty field
//!   (dead or wrap edges);
//! - or the whole world is frozen: the exact period watch saw period 1, or (when changes are
//!   counted, see [`World::set_track_changes`]) the last generation changed no cell, moved no ant,
//!   and wrote no row. Stochastic (forest fire), B0, and Margolus rules (whose partition
//!   alternates) never count as frozen this way.
//!
//! Done-ness is bookkeeping only: it is not part of the state hash, scenes, or the command log.
//! The `done` script trigger measures changes automatically while it is the running step's
//! trigger, on the CPU and on the GPU alike (the GPU stops after any generation that changed no
//! cell or left the running fields empty, and the engine then decides from the same counts).

use super::{Agent, CaInstance, World};
use crate::rules::Rule;
use crate::script::Trigger;
use crate::topology::Topology;

/// A running field's index, states, heights, and continuous values.
type FieldCopy = (usize, Vec<u8>, Vec<u32>, Vec<f32>);

/// The running fields, cursors, and ants before a generation, for counting changes.
pub(super) struct Before {
    fields: Vec<FieldCopy>,
    cursors: Vec<u32>,
    agents: Vec<Vec<Agent>>,
}

impl World {
    /// Count changed cells after every generation, so [`World::done`] sees a frozen world at
    /// once (rather than when the exact period watch next compares) and
    /// [`World::last_changed`] is available. Costs one copy of the running fields per generation.
    /// Not part of the state or the scene; kept across resets and tiling changes.
    pub fn set_track_changes(&mut self, on: bool) {
        self.track_changes = on;
    }
    pub fn track_changes(&self) -> bool {
        self.track_changes
    }

    /// Cells whose state (or grain count) changed during the last generation, summed over the
    /// fields that ran, when that was measured (changes tracked, or a script trigger needed it).
    /// `None` after any command and after GPU batches that did not count changes.
    pub fn last_changed(&self) -> Option<u64> {
        self.last_changed
    }

    /// True when no enabled CA can change the world any more. See the module docs.
    pub fn done(&self) -> bool {
        self.done_reason().is_some()
    }

    /// Why the world is done, in words, or `None` while something can still change.
    pub fn done_reason(&self) -> Option<String> {
        if self.watch.found.is_some_and(|f| f.0 == 1) {
            return Some("frozen (exact repeat check)".into());
        }
        if self.last_changed == Some(0) && !self.last_moved && self.quiet_means_frozen() {
            return Some("frozen (the last generation changed nothing)".into());
        }
        let mut reasons: Vec<&'static str> = Vec::new();
        let mut any = false;
        for ca in self.cas.iter().filter(|c| c.enabled) {
            any = true;
            let r = self.ca_done(ca)?;
            if !reasons.contains(&r) {
                reasons.push(r);
            }
        }
        if !any {
            return Some("no CA is running".into());
        }
        Some(reasons.join("; "))
    }

    /// One CA's own reason to be done, ignoring the others.
    fn ca_done(&self, ca: &CaInstance) -> Option<&'static str> {
        let topo = &self.topo;
        let f = &self.fields[ca.field];
        match &ca.rule {
            Rule::Wolfram(_) | Rule::HexWolfram { .. } => {
                let g = match ca.rule {
                    Rule::Wolfram(_) => topo.rows(),
                    _ => topo.hex_rows(),
                }?;
                if ca.cursor + 1 < g.h {
                    None
                } else if g.wrap {
                    Some("a full pass reached the bottom row")
                } else {
                    Some("reached the bottom row")
                }
            }
            Rule::HexTri { .. } => {
                let g = topo.flat_hex_rows()?;
                if ca.cursor + 1 < crate::step::hext_layers(g) {
                    None
                } else if g.wrap {
                    Some("a full pass reached the bottom row")
                } else {
                    Some("reached the bottom row")
                }
            }
            Rule::Layered { .. } => {
                let layers = ca.layers.as_ref()?;
                let next = ca.cursor + 1;
                (!layers.contains(&next)).then_some("wrote the last layer")
            }
            Rule::Sandpile => (ca.drop_per_gen == 0 && crate::sandpile::is_stable(topo, ca.nb, f))
                .then_some("stable: no cell can topple"),
            Rule::Turmite(_) => ca.agents.is_empty().then_some("no ants left"),
            r if quiescent(r, topo) => f.is_empty().then_some("empty"),
            _ => None,
        }
    }

    /// Is "nothing changed in the last generation" proof that nothing ever will?
    fn quiet_means_frozen(&self) -> bool {
        self.cas.iter().filter(|c| c.enabled).all(|c| {
            !c.rule.is_stochastic() && !is_b0(&c.rule) && !matches!(c.rule, Rule::Margolus(_))
        })
    }

    /// Script steps whose trigger needs each generation's change count.
    pub(super) fn script_wants_changes(&self) -> bool {
        match &self.script {
            Some(sc) if !self.script_state.finished => {
                let t = &sc.steps[self.script_state.step].until;
                t.needs_activity() || *t == Trigger::Done
            }
            _ => false,
        }
    }

    /// The running script step's trigger is `done`.
    pub(super) fn script_wants_done(&self) -> bool {
        match &self.script {
            Some(sc) if !self.script_state.finished => {
                sc.steps[self.script_state.step].until == Trigger::Done
            }
            _ => false,
        }
    }

    pub(super) fn running_snapshot(&self) -> Before {
        Before {
            fields: self
                .running_fields()
                .into_iter()
                .map(|f| {
                    (
                        f,
                        self.fields[f].state.clone(),
                        self.fields[f].height.clone(),
                        self.fields[f].value.clone(),
                    )
                })
                .collect(),
            cursors: self.cas.iter().map(|c| c.cursor).collect(),
            agents: self.cas.iter().map(|c| c.agents.clone()).collect(),
        }
    }

    /// (changed cells, cells compared) since `b`; also records whether any row cursor or ant moved.
    pub(super) fn count_changes(&mut self, b: &Before) -> (u64, u64) {
        let mut changed = 0u64;
        let mut total = 0u64;
        for (fi, st, h, v) in &b.fields {
            let f = &self.fields[*fi];
            if f.kind == crate::rules::FieldKind::Continuous {
                // Exact: any value that moved counts.
                changed += v
                    .iter()
                    .zip(&f.value)
                    .filter(|(a, b)| a.to_bits() != b.to_bits())
                    .count() as u64;
                total += self.topo.len() as u64;
                continue;
            }
            changed += st.iter().zip(&f.state).filter(|(a, b)| a != b).count() as u64;
            changed += h.iter().zip(&f.height).filter(|(a, b)| a != b).count() as u64;
            total += self.topo.len() as u64;
        }
        self.last_moved = self.cas.len() != b.cursors.len()
            || self.cas.iter().zip(&b.cursors).any(|(c, &k)| c.cursor != k)
            || self.cas.iter().zip(&b.agents).any(|(c, a)| &c.agents != a);
        (changed, total)
    }

    /// [`World::done`] for a generation computed outside the engine, from its metrics alone
    /// (the fields themselves are written back only at the end of a batch). External steppers
    /// run only field rules, so this matches the CPU's answer exactly.
    pub(super) fn done_external(&self, population: u64, changed: u64) -> bool {
        if self.watch.found.is_some_and(|f| f.0 == 1) {
            return true;
        }
        if changed == 0 && self.quiet_means_frozen() {
            return true;
        }
        let mut cas = self.cas.iter().filter(|c| c.enabled).peekable();
        cas.peek().is_none() || (population == 0 && cas.all(|c| quiescent(&c.rule, &self.topo)))
    }
}

fn is_b0(r: &Rule) -> bool {
    match r {
        Rule::LifeLike { birth, .. } | Rule::Generations { birth, .. } => birth & 1 == 1,
        Rule::Ltl(l) => l.birth.first().copied().unwrap_or(false),
        _ => false,
    }
}

/// Field rules under which an all-zero field stays all zero.
fn quiescent(r: &Rule, topo: &Topology) -> bool {
    let live = topo.live_edges();
    match r {
        Rule::LifeLike { .. } | Rule::Generations { .. } | Rule::Ltl(_) => !is_b0(r) && !live,
        Rule::WireWorld | Rule::SpectreGlider | Rule::VonNeumann | Rule::Hodgepodge { .. } => true,
        Rule::Rps { .. } => true,
        // Block rules that keep an empty block empty.
        Rule::Margolus(t) => t[0] == 0,
        Rule::Lenia(l) => l.quiescent(),
        Rule::Cyclic { threshold, .. } | Rule::GreenbergHastings { threshold, .. } => {
            *threshold >= 1
        }
        _ => false,
    }
}
