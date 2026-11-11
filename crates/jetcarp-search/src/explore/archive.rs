//! Interest scores, behaviour bins and the archive of the best run found in each niche.

use jetcarp_core::explore::cand::Cand;
use jetcarp_core::explore::features::{Fate, Features};
use jetcarp_core::explore::space::Fam;
use jetcarp_core::rng::Rng;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// A measured run.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Eval {
    pub cand: Cand,
    pub fate: Fate,
    pub feat: Features,
    pub score: f32,
    /// See [`behaviour_bin`].
    pub bin: u32,
}

fn tri(x: f32, lo: f32, peak: f32, hi: f32) -> f32 {
    if x <= lo || x >= hi {
        0.0
    } else if x <= peak {
        (x - lo) / (peak - lo)
    } else {
        (hi - x) / (hi - peak)
    }
}

/// How interesting a run looks; about 0 to 3, higher is better. The stored features allow
/// rescoring without rerunning, so this function can change between searches.
pub fn score(fate: Fate, f: &Features, gens: u32, fam: Fam) -> f32 {
    let life = (1.0 + f.settle as f32).log2() / (1.0 + gens as f32).log2();
    if fam.is_rows() {
        // Rows and layers are judged as a picture.
        let pic = tri(f.bits, 0.05, 0.75, 1.9) + 0.5 * tri(f.smooth, 0.1, 0.45, 0.95);
        let bal = tri(f.density, 0.04, 0.4, 0.9);
        return pic + 0.5 * bal + 0.3 * tri(f.size_entropy, 0.2, 2.0, 5.0);
    }
    match fate {
        Fate::Died => return 0.25 * life,
        Fate::Froze | Fate::Cycle if life < 0.3 => {
            return 0.1 + 0.6 * life + 0.3 * tri(f.smooth, 0.05, 0.4, 0.95);
        }
        _ => {}
    }
    let mut s = taste(fate, f, gens, fam);
    // What the fit can't see, from looking at its top picks. Like-shuffled noise:
    if f.smooth < 0.07 && f.entropy > 0.7 {
        s *= 0.4;
    }
    // A world that ends as one flat colour:
    if fate == Fate::Froze && f.density_late > 0.97 {
        s *= 0.5;
    }
    // Forest fires are static once they get going, and were nearly all dropped.
    if fam == Fam::ForestFire {
        s *= 0.75;
    }
    s
}

/// How much people liked runs like this one, 0 to 3: a logistic fit to 829 field-rule clips rated
/// by eye over four rounds, most of them by Emily Teich (keep 1, maybe 0.45, drop 0; ridge penalty
/// 8, family terms penalty 5;
/// row rules and WireWorld circuits are judged elsewhere). Fitted to the first three rounds, it
/// reached AUC 0.84 for keep against drop on the fourth, which it had not seen. The ratings favour
/// runs that settle late, start busy, and build large connected structures rather than dust, and
/// some kinds of rule more than others.
fn taste(fate: Fate, f: &Features, gens: u32, fam: Fam) -> f32 {
    let settled = f.settle as f32 / gens.max(1) as f32;
    let movers = f.movers.min(20) as f32 / 20.0;
    let terms = [
        (1.39790, settled),
        (0.03599, f.density_late),
        (-0.78226, f.activity_late.sqrt()),
        (1.23215, f.activity_early.sqrt()),
        (0.07296, f.pop_cv.sqrt()),
        (0.05969, f.growth),
        (0.37119, f.entropy),
        (0.00976, f.bits),
        (-0.69194, f.same_nb),
        (0.83742, f.smooth),
        (-1.46980, f.comps_per_live.sqrt()),
        (-0.40279, f.largest),
        (-0.02808, f.size_entropy),
        (-0.01143, f.patchiness),
        (0.44945, f.travel),
        (0.56432, movers),
        (0.54866, (fate == Fate::Froze) as u32 as f32),
        (0.91522, (fate == Fate::Cycle) as u32 as f32),
    ];
    let linear: f32 = terms
        .iter()
        .filter(|(_, x)| x.is_finite())
        .map(|(w, x)| w * x)
        .sum::<f32>()
        + family_taste(fam)
        - 1.53943;
    3.0 / (1.0 + (-linear).exp())
}

/// The family term of [`taste`]. Families the ratings don't cover count as neutral.
fn family_taste(fam: Fam) -> f32 {
    match fam {
        Fam::GreenbergHastings => 0.75721,
        Fam::Lenia => 0.52213,
        Fam::Hodgepodge => 0.38799,
        Fam::SpectreGlider => 0.33215,
        Fam::Ltl => 0.32333,
        Fam::Rps => 0.11606,
        Fam::Sandpile => 0.07480,
        Fam::Jaca => 0.02563,
        Fam::Table => 0.02063,
        Fam::Cyclic => -0.03297,
        Fam::Margolus => -0.04242,
        Fam::ForestFire => -0.21730,
        Fam::Turmite => -0.23428,
        Fam::Generations => -0.29203,
        Fam::WireWorld => -0.36683,
        Fam::Priority => -0.42430,
        Fam::LifeLike => -0.94976,
        _ => 0.0,
    }
}

fn bucket(x: f32, edges: &[f32]) -> u32 {
    edges.iter().take_while(|&&e| x >= e).count() as u32
}

/// The coarse behaviour class of a run (packed into an integer).
pub fn behaviour_bin(fate: Fate, f: &Features, gens: u32) -> u32 {
    let fate_bin = match fate {
        Fate::Died => 0,
        Fate::Froze => 1,
        Fate::Cycle if f.period <= 4 => 2,
        Fate::Cycle => 3,
        Fate::Running => 4,
    };
    let frac = f.settle as f32 / gens.max(1) as f32;
    let trans = bucket(frac, &[0.05, 0.25, 0.75]);
    let dens = bucket(f.density_late, &[0.02, 0.1, 0.3, 0.6]);
    let act = bucket(f.activity_late, &[1e-4, 0.005, 0.03, 0.15]);
    let shape = if f.comps_per_live > 0.4 {
        0
    } else if f.comps_per_live > 0.05 {
        1
    } else if f.largest < 0.7 {
        2
    } else if f.smooth > 0.5 {
        3
    } else {
        4
    };
    let mover = (f.movers > 0) as u32;
    fate_bin * 5 * 5 * 5 * 5 * 2
        + trans * 5 * 5 * 5 * 2
        + dens * 5 * 5 * 2
        + act * 5 * 2
        + shape * 2
        + mover
}

/// How many runs each niche keeps.
const PER_NICHE: usize = 3;

/// A niche: rule family, tiling group, and behaviour.
pub type Niche = (Fam, u8, u32);

/// The best runs of each niche.
#[derive(Default)]
pub struct Archive {
    pub cells: HashMap<Niche, Vec<(i64, f32)>>,
    /// Runs by id (kept while they are elites).
    pub runs: HashMap<i64, Eval>,
    /// Index into `order` for sampling elites uniformly by niche.
    pub order: Vec<Niche>,
    pub confirmed: std::collections::HashSet<i64>,
    /// The tiling group of each elite (the niche's second part), for saving.
    groups: HashMap<i64, u8>,
    /// Elites added (`true`) and dropped (`false`) by [`offer`](Self::offer) since the last
    /// [`take_changes`](Self::take_changes), in order.
    changes: Vec<(i64, bool)>,
}

impl Archive {
    /// Put an elite back as it was saved (no checks: a saved archive is already one).
    pub fn insert(&mut self, id: i64, ev: Eval, group: u8) {
        let key: Niche = (ev.cand.fam, group, ev.bin);
        let list = match self.cells.entry(key) {
            std::collections::hash_map::Entry::Occupied(e) => e.into_mut(),
            std::collections::hash_map::Entry::Vacant(v) => {
                self.order.push(key);
                v.insert(Vec::new())
            }
        };
        list.push((id, ev.score));
        list.sort_by(|a, b| b.1.total_cmp(&a.1));
        self.groups.insert(id, group);
        self.runs.insert(id, ev);
    }

    /// The elites added and dropped since the last call, as each one's last state: its tiling
    /// group when it is an elite now, `None` when it was dropped.
    pub fn take_changes(&mut self) -> Vec<(i64, Option<u8>)> {
        let mut last: HashMap<i64, bool> = HashMap::new();
        let mut order = Vec::new();
        for (id, added) in std::mem::take(&mut self.changes) {
            if last.insert(id, added).is_none() {
                order.push(id);
            }
        }
        order
            .into_iter()
            .map(|id| {
                (
                    id,
                    last[&id].then(|| self.groups.get(&id).copied()).flatten(),
                )
            })
            .collect()
    }

    /// Try to place a run. Returns `true` if it became an elite.
    pub fn offer(&mut self, id: i64, ev: Eval, group: u8) -> bool {
        let fam = ev.cand.fam;
        let key: Niche = (fam, group, ev.bin);
        let score = ev.score;
        let list = match self.cells.entry(key) {
            std::collections::hash_map::Entry::Occupied(e) => e.into_mut(),
            std::collections::hash_map::Entry::Vacant(v) => {
                self.order.push(key);
                v.insert(Vec::new())
            }
        };
        // One per rule text in a niche: keep the better one.
        let same_rule = list.iter().position(|(i, _)| {
            self.runs
                .get(i)
                .is_some_and(|r| r.cand.rule == ev.cand.rule)
        });
        if let Some(p) = same_rule {
            if list[p].1 >= score {
                return false;
            }
            let (old, _) = list.remove(p);
            self.runs.remove(&old);
            self.groups.remove(&old);
            self.changes.push((old, false));
        }
        if list.len() >= PER_NICHE && list.iter().all(|&(_, s)| s >= score) {
            return false;
        }
        list.push((id, score));
        list.sort_by(|a, b| b.1.total_cmp(&a.1));
        while list.len() > PER_NICHE {
            let (old, _) = list.pop().unwrap();
            self.runs.remove(&old);
            self.groups.remove(&old);
            self.changes.push((old, false));
        }
        self.runs.insert(id, ev);
        self.groups.insert(id, group);
        self.changes.push((id, true));
        true
    }

    /// A random elite, by niche, biased to the better runs of the niche. `fam` narrows the choice.
    pub fn sample(&self, rng: &mut Rng, fam: Option<Fam>) -> Option<(i64, &Eval)> {
        if self.order.is_empty() {
            return None;
        }
        for _ in 0..64 {
            let k = self.order[rng.below(self.order.len() as u64) as usize];
            if fam.is_some_and(|f| f != k.0) {
                continue;
            }
            let list = self.cells.get(&k)?;
            if list.is_empty() {
                continue;
            }
            let i = (rng.below(list.len() as u64) * rng.below(list.len() as u64 + 1)) as usize
                / list.len().max(1);
            let (id, _) = list[i.min(list.len() - 1)];
            return self.runs.get(&id).map(|e| (id, e));
        }
        None
    }

    /// Elites not yet confirmed, up to `n`: best first within a family, families taking turns,
    /// the family with the fewest confirmed elites first. Scores are not comparable across
    /// families, so one ranking would leave the lower-scoring families unconfirmed.
    pub fn unconfirmed(&self, n: usize) -> Vec<(i64, Eval)> {
        let mut pending: HashMap<Fam, Vec<(i64, f32)>> = HashMap::new();
        let mut done: HashMap<Fam, usize> = HashMap::new();
        for (id, e) in &self.runs {
            if self.confirmed.contains(id) {
                *done.entry(e.cand.fam).or_insert(0) += 1;
            } else {
                pending.entry(e.cand.fam).or_default().push((*id, e.score));
            }
        }
        let mut fams: Vec<(usize, Fam, Vec<(i64, f32)>)> = pending
            .into_iter()
            .map(|(f, mut v)| {
                v.sort_by(|a, b| b.1.total_cmp(&a.1));
                (done.get(&f).copied().unwrap_or(0), f, v)
            })
            .collect();
        fams.sort_by_key(|(d, f, _)| (*d, *f));
        let mut out = Vec::with_capacity(n);
        for i in 0.. {
            let before = out.len();
            for (_, _, v) in &fams {
                if let Some(&(id, _)) = v.get(i)
                    && out.len() < n
                {
                    out.push((id, self.runs[&id].clone()));
                }
            }
            if out.len() == before || out.len() >= n {
                break;
            }
        }
        out
    }
}
