//! Automatic reseeding (see [`crate::reseed`]) and the activity history.
//!
//! After every generation (on the CPU in [`World::step`], for external steppers in
//! [`World::finish_external`]) each enabled policy is checked, right after the script:
//!
//! - A policy cannot fire for `cooldown` generations after it was installed, after it last fired,
//!   or after a manual reseed.
//! - `every N` fires N generations after that point (or at the end of the cooldown if later).
//! - `settled` measures every generation once the cooldown is over (quiet generations in a row,
//!   and the per-generation changed counts in the activity history) and decides at generations
//!   that are multiples of [`SETTLE_CHECK`]: see [`ReseedWhen::Settled`].
//!
//! Firing plans [`Edits`] from the world RNG and applies them; the period watch restarts, so a
//! stale period cannot fire again at once. Everything here is deterministic, so a scene replays
//! the same reseeds at any thread count and on the GPU (which runs batches up to the next due
//! date or check, see [`World::external_plan`]).

use super::{World, external};
use crate::reseed::{self, AutoReseed, Edits, PolicyStatus, ReseedMode, ReseedWhen, SETTLE_CHECK};
use crate::rules::FieldKind;

/// Generations kept in [`World::activity_history`].
pub const ACTIVITY_HISTORY: usize = 512;

/// One generation's activity: cells whose state changed, and that as a fraction of the cells
/// compared (every cell of every running field).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Activity {
    pub generation: u64,
    pub changed: u64,
    pub fraction: f32,
}

impl World {
    /// The automatic reseed policies.
    pub fn reseed_policies(&self) -> &[AutoReseed] {
        &self.reseed
    }

    /// What each policy has done, in policy order.
    pub fn reseed_status(&self) -> &[PolicyStatus] {
        &self.reseed_status
    }

    /// Recent generations' activity, oldest first, for every generation whose changes were
    /// counted: while changes are tracked ([`World::set_track_changes`]), while a script trigger
    /// or a settled policy measures activity, and on the GPU whenever it counts changes. At most
    /// [`ACTIVITY_HISTORY`] entries; generations need not be consecutive.
    pub fn activity_history(&self) -> &std::collections::VecDeque<Activity> {
        &self.activity
    }

    /// The fraction of cells that changed in the last generation, if it was counted then.
    pub fn last_activity(&self) -> Option<f64> {
        self.activity
            .back()
            .filter(|a| a.generation == self.generation)
            .map(|a| a.fraction as f64)
    }

    /// Typical tile size (square root of the mean tile area), measured once.
    pub fn tile_size(&self) -> f64 {
        *self.tile_size.get_or_init(|| super::cell_size(&self.topo))
    }

    pub(super) fn push_activity(&mut self, changed: u64, total: u64) {
        if self.activity.len() >= ACTIVITY_HISTORY {
            self.activity.pop_front();
        }
        self.activity.push_back(Activity {
            generation: self.generation,
            changed,
            fraction: (changed as f64 / total.max(1) as f64) as f32,
        });
    }

    pub(super) fn set_reseed_policies(&mut self, policies: Vec<AutoReseed>) -> Result<(), String> {
        for p in &policies {
            p.validate()?;
        }
        self.reseed_status = policies
            .iter()
            .map(|_| PolicyStatus {
                since: self.generation,
                ..Default::default()
            })
            .collect();
        self.reseed = policies;
        Ok(())
    }

    /// Fields a reseed aimed at `ca` writes: that CA's field, or every running CA's field.
    fn reseed_targets(&self, ca: Option<u32>) -> Result<Vec<usize>, String> {
        match ca {
            Some(id) => {
                let c = self.ca(id).ok_or_else(|| format!("no CA with id {id}"))?;
                if self.fields[c.field].kind == FieldKind::Height {
                    return Err(format!(
                        "{} is a sandpile; reseeding needs a state field",
                        c.name
                    ));
                }
                Ok(vec![c.field])
            }
            None => Ok(self
                .running_fields()
                .into_iter()
                .filter(|&f| self.fields[f].kind != FieldKind::Height)
                .collect()),
        }
    }

    /// Is field `f` signed (a JACA CA runs on it)?
    pub fn field_is_signed(&self, f: usize) -> bool {
        self.cas
            .iter()
            .any(|c| c.field == f && matches!(c.rule, crate::rules::Rule::Jaca(_)))
    }

    /// Does a reseed aimed at `ca` (or every running CA) reach a JACA field, so its sign matters?
    pub fn reseed_is_signed(&self, ca: Option<u32>) -> bool {
        self.reseed_targets(ca)
            .is_ok_and(|t| t.iter().any(|&f| self.field_is_signed(f)))
    }

    /// Plan a reseed of the target fields from the world RNG. `turn` counts earlier reseeds of
    /// the same source (a policy's firings, or manual reseeds) for the alternate sign.
    fn plan_reseed(
        &mut self,
        ca: Option<u32>,
        mode: &ReseedMode,
        turn: u64,
    ) -> Result<Vec<(usize, Edits)>, String> {
        let targets = self.reseed_targets(ca)?;
        let tile = self.tile_size();
        let topo = self.topo.clone();
        let mut out = Vec::with_capacity(targets.len());
        for f in targets {
            let kind = self.fields[f].kind;
            let turn = self.field_is_signed(f).then_some(turn);
            out.push((
                f,
                reseed::plan_signed(&topo, tile, kind, mode, &mut self.rng, turn)?,
            ));
        }
        Ok(out)
    }

    /// `Command::Reseed`.
    pub(super) fn reseed_now(&mut self, ca: Option<u32>, mode: &ReseedMode) -> Result<(), String> {
        mode.validate()?;
        let targets = self.reseed_targets(ca)?;
        if targets.is_empty() {
            return Err("nothing to reseed: no CA is running (name a CA with `ca`)".into());
        }
        let turn = self.manual_reseeds;
        let edits = self.plan_reseed(ca, mode, turn)?;
        self.manual_reseeds += 1;
        for (f, e) in &edits {
            e.apply(&mut self.fields[*f]);
        }
        for st in &mut self.reseed_status {
            st.since = self.generation;
            st.streak = 0;
        }
        Ok(())
    }

    /// Apply reseeds planned during an external batch. The stepper calls this after writing the
    /// running fields back (or applies [`World::take_pending_reseed`] itself).
    pub fn flush_pending_reseed(&mut self) {
        if self.pending_reseed.is_empty() {
            return;
        }
        for (f, e) in std::mem::take(&mut self.pending_reseed) {
            if let Some(field) = self.fields.get_mut(f) {
                e.apply(field);
            }
        }
        self.rev = external::next_rev();
    }

    /// Reseeds planned at the end of the last externally stepped batch, as (field, edits), for
    /// a stepper that applies them to its own copy of the fields. Whatever is taken here the
    /// caller must apply to the world's fields too (directly, or by writing them back later).
    pub fn take_pending_reseed(&mut self) -> Vec<(usize, Edits)> {
        std::mem::take(&mut self.pending_reseed)
    }

    /// Is policy `p` past its cooldown at generation `g`?
    fn ready(p: &AutoReseed, st: &PolicyStatus, g: u64) -> bool {
        p.enabled && g >= st.since.saturating_add(p.cooldown)
    }

    /// A settled policy measures activity at generation `g` (the one about to be computed).
    pub(super) fn reseed_wants_changes(&self, g: u64) -> bool {
        self.reseed
            .iter()
            .zip(&self.reseed_status)
            .any(|(p, st)| matches!(p.when, ReseedWhen::Settled { .. }) && Self::ready(p, st, g))
    }

    /// A settled policy decides after the generation just computed (and needs the population).
    pub(super) fn reseed_wants_population(&self) -> bool {
        self.generation.is_multiple_of(SETTLE_CHECK) && self.reseed_wants_changes(self.generation)
    }

    /// The changed counts of the last `window` generations (all after `since`) repeat with a
    /// period of at most `cycle`.
    fn steady(&self, since: u64, window: u64, cycle: u64) -> bool {
        let need = (window + cycle) as usize;
        let h = &self.activity;
        if h.len() < need {
            return false;
        }
        let tail: Vec<&Activity> = h.iter().skip(h.len() - need).collect();
        // Consecutive generations, all after the last reseed, ending now.
        if tail[0].generation <= since
            || tail[need - 1].generation != self.generation
            || tail[need - 1].generation - tail[0].generation != need as u64 - 1
        {
            return false;
        }
        (1..=cycle as usize).any(|q| (q..need).all(|i| tail[i].changed == tail[i - q].changed))
    }

    /// Check every policy after a generation; true when one fired. `activity` is the fraction of
    /// cells that changed (when counted), `population` the live cells over the running fields
    /// (measured when a settled policy is due to decide). `external`: leave the edits pending for
    /// the stepper.
    pub(super) fn reseed_after_step(
        &mut self,
        activity: Option<f64>,
        population: u64,
        any_running: bool,
        external: bool,
    ) -> bool {
        let g = self.generation;
        let period = self.watch.found.map(|f| f.0);
        let check = g.is_multiple_of(SETTLE_CHECK);
        let mut fired = false;
        for k in 0..self.reseed.len() {
            let p = self.reseed[k].clone();
            let st = self.reseed_status[k].clone();
            if !Self::ready(&p, &st, g) {
                continue;
            }
            let fire = match p.when {
                ReseedWhen::Every { gens } => g >= st.since.saturating_add(gens.max(1)),
                ReseedWhen::Settled {
                    activity: limit,
                    for_gens,
                    cycle,
                    period: max_period,
                    latest,
                } => {
                    let mut streak = st.streak;
                    if any_running && let Some(a) = activity {
                        streak = if a < limit { streak + 1 } else { 0 };
                    }
                    let since = st.since;
                    let fire = any_running
                        && check
                        && (streak >= for_gens.max(1)
                            || population == 0
                            || latest.is_some_and(|l| g >= since.saturating_add(l))
                            || max_period.is_some_and(|m| period.is_some_and(|q| q <= m))
                            || cycle.is_some_and(|c| self.steady(since, for_gens.max(1), c)));
                    self.reseed_status[k].streak = streak;
                    fire
                }
            };
            if !fire {
                continue;
            }
            fired = true;
            let (ca, mode) = (p.ca, p.mode.clone());
            let planned = self.plan_reseed(ca, &mode, st.fired);
            let st = &mut self.reseed_status[k];
            st.since = g;
            st.streak = 0;
            st.fired += 1;
            st.last_fired = Some(g);
            match planned {
                Ok(edits) => {
                    st.error = None;
                    if external {
                        self.pending_reseed.extend(edits);
                    } else {
                        for (f, e) in &edits {
                            e.apply(&mut self.fields[*f]);
                        }
                    }
                }
                Err(e) => st.error = Some(e),
            }
        }
        if fired {
            self.watch = super::PeriodWatch {
                power: 1,
                ..Default::default()
            };
            self.last_changed = None;
        }
        fired
    }

    /// For [`World::external_plan`]: (generations the next batch may run before a policy must be
    /// checked, whether a settled policy measures changes and population during it).
    pub(super) fn reseed_plan(&self) -> (Option<u64>, bool) {
        let g = self.generation;
        let mut cap: Option<u64> = None;
        let mut min = |c: u64| cap = Some(cap.map_or(c, |x: u64| x.min(c)));
        let mut measuring = false;
        for (p, st) in self.reseed.iter().zip(&self.reseed_status) {
            if !p.enabled {
                continue;
            }
            let ready_at = st.since.saturating_add(p.cooldown);
            match p.when {
                ReseedWhen::Every { gens } => {
                    let at = ready_at.max(st.since.saturating_add(gens.max(1)));
                    min(at.max(g + 1) - g);
                }
                ReseedWhen::Settled { .. } => {
                    if g + 1 >= ready_at {
                        measuring = true;
                        // Up to the next check.
                        min(SETTLE_CHECK - g % SETTLE_CHECK);
                    } else {
                        // Up to the last generation of the cooldown, then plan again.
                        min(ready_at - 1 - g);
                    }
                }
            }
        }
        (cap, measuring)
    }
}
