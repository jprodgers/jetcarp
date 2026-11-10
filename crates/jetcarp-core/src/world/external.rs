//! Hooks for stepping a world outside the engine, used by the `jetcarp-gpu` crate.
//!
//! An external stepper computes a batch of generations on its own copy of the running fields and
//! then hands the result back. The engine keeps all bookkeeping (generation count, the exact
//! period watch, scripts, the command log), so a world stepped externally ends in the same state,
//! with the same hash, as one stepped with [`World::step`]:
//!
//! 1. [`World::external_plan`] says which fields run, how many generations may run before a script
//!    step's count or time limit ends (or a reseed policy is due), the period-watch schedule, and
//!    the conditions after which the stepper must stop because a script trigger or a settled
//!    reseed policy may fire.
//! 2. The stepper runs at most that many generations and records [`GenMetrics`] for each.
//! 3. It calls [`World::finish_external`], which replays the bookkeeping generation by generation
//!    and logs one `Step` command, and writes the new states and ages into `World::fields`.
//! 4. If a reseed policy fired at the batch's last generation, its planned edits wait in the
//!    world: the stepper either applies [`World::take_pending_reseed`] to its own copy (and to
//!    the world's copy of any field it does not hold), or writes the fields back and calls
//!    [`World::flush_pending_reseed`]. `World::step` and `World::apply` flush them first too.
//!
//! [`World::revision`] changes after every command and step, so a stepper can tell whether its
//! copy of the fields is still current.

use super::{Command, Snapshot, World};
use crate::script::Trigger;
use std::sync::atomic::{AtomicU64, Ordering};

static REVISION: AtomicU64 = AtomicU64::new(1);

pub(super) fn next_rev() -> u64 {
    REVISION.fetch_add(1, Ordering::Relaxed)
}

/// What an external stepper must do for the next batch of generations.
#[derive(Clone, Debug, PartialEq)]
pub struct ExternalPlan {
    /// Fields written by the enabled CAs, in index order.
    pub running_fields: Vec<usize>,
    /// Run at most this many generations: the running script step ends by count or time limit then.
    pub max_gens: Option<u64>,
    /// The exact period watch, or `None` once a period has been found.
    pub watch: Option<WatchPlan>,
    /// Stop after the first generation where the period watch's comparison matches
    /// (a script `repeats` trigger may fire).
    pub halt_on_repeat: bool,
    /// Stop after the first generation where any of these holds (a script trigger may fire).
    /// Each activity halt keeps its own streak.
    pub halts: Vec<Halt>,
    /// The engine wants each generation's changed-cell count (changes tracked, or a settled
    /// reseed policy measuring). Halts that need changes count them anyway.
    pub count_changes: bool,
    /// The engine wants each generation's population (a settled reseed policy checks for an
    /// empty world). Population halts count it anyway.
    pub count_population: bool,
}

/// Brent cycle detection state. After each generation the stepper compares the running fields'
/// states with the snapshot (when one exists), then takes a new snapshot on schedule.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WatchPlan {
    pub has_snapshot: bool,
    pub power: u64,
    pub lam: u64,
}

impl WatchPlan {
    /// For generations 1..=len of a batch, assuming no repeat is found: (compare with the snapshot
    /// after this generation, then take a new snapshot after it).
    pub fn schedule(&self, len: u64) -> Vec<(bool, bool)> {
        let (mut has, mut power, mut lam) = (self.has_snapshot, self.power, self.lam);
        (0..len)
            .map(|_| {
                lam += 1;
                let compare = has;
                let take = lam == power;
                if take {
                    has = true;
                    power *= 2;
                    lam = 0;
                }
                (compare, take)
            })
            .collect()
    }
}

/// Conditions on one generation's [`GenMetrics`] after which a script trigger may fire.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Halt {
    /// Population (summed over the running fields) at most this.
    PopulationAtMost(u64),
    /// Population at least this.
    PopulationAtLeast(u64),
    /// `changed < limit` (cells whose state changed, summed over the running fields) has held for
    /// `for_gens` generations in a row, counting the `streak` already reached before the batch.
    ChangedBelow {
        limit: u64,
        for_gens: u64,
        streak: u64,
    },
    /// `changed >= limit` has held for `for_gens` generations in a row.
    ChangedAtLeast {
        limit: u64,
        for_gens: u64,
        streak: u64,
    },
    /// `changed == 0` or `population == 0` (a `done` trigger may fire; see `World::done`).
    Settled,
}

impl Halt {
    /// Activity triggers need each generation's changed-cell count.
    pub fn needs_changes(&self) -> bool {
        matches!(
            self,
            Halt::ChangedBelow { .. } | Halt::ChangedAtLeast { .. } | Halt::Settled
        )
    }
}

/// Measurements an external stepper records after each generation.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct GenMetrics {
    /// Nonzero cells over the running fields (needed only for population halts).
    pub population: u64,
    /// Cells whose state changed during the generation (needed only for activity and settled
    /// halts).
    pub changed: u64,
    /// The comparison scheduled for this generation matched the snapshot exactly.
    pub repeat: bool,
    /// `changed` was counted for this generation.
    pub counted: bool,
}

impl World {
    /// Changes after every command and every step, and never repeats, even across worlds.
    /// Code that changes `fields` directly should go through a command instead.
    pub fn revision(&self) -> u64 {
        self.rev
    }

    /// The plan for the next externally stepped batch.
    pub fn external_plan(&self) -> ExternalPlan {
        let running_fields = self.running_fields();
        let watch = self.watch.found.is_none().then_some(WatchPlan {
            has_snapshot: self.watch.snap.is_some(),
            power: self.watch.power,
            lam: self.watch.lam,
        });
        let total = (self.topo.len() * running_fields.len()).max(1) as u64;
        let mut plan = ExternalPlan {
            running_fields,
            max_gens: None,
            watch,
            halt_on_repeat: false,
            halts: Vec::new(),
            count_changes: self.track_changes,
            count_population: false,
        };
        let mut cap: Option<u64> = None;
        let mut min = |c: u64| cap = Some(cap.map_or(c, |x: u64| x.min(c)));
        // Reseed policies: due dates, cooldown ends, and settled checks cap the batch; ready
        // settled policies need every generation's changes and population.
        let (due, measuring) = self.reseed_plan();
        if let Some(c) = due {
            min(c);
        }
        if measuring {
            plan.count_changes = true;
            plan.count_population = true;
        }
        if let Some(sc) = &self.script
            && !self.script_state.finished
        {
            let st = &self.script_state;
            let step = &sc.steps[st.step];
            // A trigger at "generation - started >= n" first fires after generation
            // max(started + n, now + 1).
            let until = |n: u64| (st.started + n).max(self.generation + 1) - self.generation;
            if let Some(m) = step.max_gens {
                min(until(m));
            }
            match step.until {
                Trigger::Generations { n } => min(until(n)),
                Trigger::PopulationBelow { n } => plan.halts.push(Halt::PopulationAtMost(n)),
                Trigger::PopulationAbove { n } => plan.halts.push(Halt::PopulationAtLeast(n)),
                Trigger::ActivityBelow { fraction, for_gens } => {
                    // Smallest count that is not below the fraction, with the engine's f64
                    // arithmetic.
                    let limit = first_count(total, |c| c as f64 / total as f64 >= fraction);
                    plan.halts.push(Halt::ChangedBelow {
                        limit,
                        for_gens: for_gens.max(1),
                        streak: st.streak,
                    });
                }
                Trigger::ActivityAbove { fraction, for_gens } => {
                    let limit = first_count(total, |c| c as f64 / total as f64 > fraction);
                    plan.halts.push(Halt::ChangedAtLeast {
                        limit,
                        for_gens: for_gens.max(1),
                        streak: st.streak,
                    });
                }
                Trigger::Repeats { .. } => plan.halt_on_repeat = true,
                Trigger::Done => plan.halts.push(Halt::Settled),
                Trigger::Never => {}
            }
        }
        plan.max_gens = cap;
        plan
    }

    /// The period watch's snapshot of a field's states, if one is held.
    pub fn watch_snapshot_states(&self, field: usize) -> Option<&[u8]> {
        self.watch
            .snap
            .as_ref()
            .and_then(|s| s.states.get(field))
            .map(|v| v.as_slice())
    }

    /// Record generations computed outside the engine, one [`GenMetrics`] each. The caller has
    /// written (or will write, before anything reads the world) the running fields' new states and
    /// ages. `snapshot` is called at most once, when the period watch took a snapshot during the
    /// batch; it returns the running fields' states at the last such generation.
    ///
    /// Returns the number of generations recorded. That is all of them unless the script moved on
    /// before the last one, which means the stepper ignored [`ExternalPlan`].
    pub fn finish_external(
        &mut self,
        gens: &[GenMetrics],
        snapshot: impl FnOnce() -> Vec<(usize, Vec<u8>)>,
    ) -> u64 {
        let mut took = false;
        let mut done = 0u64;
        self.last_changed = None;
        self.last_moved = false;
        for m in gens {
            let running = self.running_fields().len();
            let total = (self.topo.len() * running).max(1) as u64;
            let activity = self
                .script
                .as_ref()
                .filter(|sc| {
                    !self.script_state.finished
                        && sc.steps[self.script_state.step].until.needs_activity()
                })
                .filter(|_| m.counted)
                .map(|_| m.changed as f64 / total as f64);
            self.last_changed = m.counted.then_some(m.changed);
            self.generation += 1;
            if m.counted {
                self.push_activity(m.changed, total);
            }
            done += 1;
            let w = &mut self.watch;
            if w.found.is_none() {
                w.lam += 1;
                if (w.snap.is_some() || took) && m.repeat {
                    w.found = Some((w.lam, self.generation));
                } else if w.lam == w.power {
                    took = true;
                    w.power *= 2;
                    w.lam = 0;
                }
            }
            let done = self
                .script_wants_done()
                .then(|| self.done_external(m.population, m.changed));
            let moved_on = self.script_check(m.population, activity, done);
            let fraction = m.counted.then(|| m.changed as f64 / total as f64);
            let reseeded = self.reseed_after_step(fraction, m.population, running > 0, true);
            if moved_on || reseeded {
                // Entering a step or reseeding resets the watch, so any snapshot taken so far is
                // discarded.
                took = false;
                break;
            }
        }
        if took && self.watch.found.is_none() {
            let mut states: Vec<Vec<u8>> = self.fields.iter().map(|f| f.state.clone()).collect();
            for (f, mut s) in snapshot() {
                s.truncate(self.topo.len());
                states[f] = s;
            }
            self.watch.snap = Some(Snapshot {
                states,
                heights: self.fields.iter().map(|f| f.height.clone()).collect(),
                agents: self.cas.iter().map(|c| c.agents.clone()).collect(),
                cursors: self.cas.iter().map(|c| c.cursor).collect(),
                phases: self.phases(),
                values: self.fields.iter().map(|f| f.value.clone()).collect(),
            });
        }
        if done > 0 {
            self.push_log(Command::Step { gens: done });
        }
        self.rev = next_rev();
        done
    }
}

/// Smallest count in 0..=total+1 for which `pred` holds (`pred` is monotone), else total + 1.
fn first_count(total: u64, pred: impl Fn(u64) -> bool) -> u64 {
    let (mut lo, mut hi) = (0u64, total + 1);
    while lo < hi {
        let mid = lo + (hi - lo) / 2;
        if pred(mid) { hi = mid } else { lo = mid + 1 }
    }
    lo
}
