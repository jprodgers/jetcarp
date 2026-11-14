//! Breeding JACA rules: which elites make good parents, a rule moved to a grid of another degree,
//! and variants that help a losing sign. JACA is harder to tune than a plain B/S rule, since both
//! signs have to hold their own; the search ([`crate::explore::job`]) uses these for its
//! `transplant`, `transplant-scaled` and `balance` candidates, and for its parents.

use crate::explore::archive::Eval;
use jetcarp_core::explore::features::Features;
use jetcarp_core::explore::space::Fam;
use jetcarp_core::jaca::JacaRule;
use jetcarp_core::rng::Rng;
use jetcarp_core::seeding::SeedStates;
use jetcarp_core::world::SeedSpec;
use serde::{Deserialize, Serialize};

/// Does a run look like noise (the built-in score's like-shuffled noise, or for JACA, signs
/// mixed like salt and pepper)?
fn noisy(f: &Features) -> bool {
    (f.smooth < 0.07 && f.entropy > 0.7) || f.signs.is_some_and(|s| s.sorted < 0.1)
}

/// A JACA run whose signs both held to the end (neither fell below 1% of live cells), and that
/// is not noise: a parent for mutations, new seeds and transplants.
pub fn strong(ev: &Eval) -> bool {
    ev.cand.fam == Fam::Jaca && ev.feat.signs.is_some_and(|s| s.lost >= 1.0) && !noisy(&ev.feat)
}

/// A sign of a JACA rule.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Sign {
    /// +1, state 1.
    Pos,
    /// -1, state 2.
    Neg,
}

/// The weaker sign of a JACA run whose weaker sign is losing: low balance (below 0.3 of live
/// cells), or a sign lost in the second half. `None` for a run that lost a sign in the first half
/// (a Life-like run), for balanced runs, and for runs without sign measures.
pub fn losing(f: &Features) -> Option<Sign> {
    let s = f.signs?;
    if s.lost_early() || (s.balance >= 0.3 && s.lost >= 1.0) {
        return None;
    }
    Some(if s.pos < 0.5 { Sign::Pos } else { Sign::Neg })
}

/// A count mask moved from a grid of `d_old` neighbours to one of `d_new` (mean degrees): each
/// count k becomes round(k x d_new / d_old), at most `max_new`. Zero stays zero.
fn scale_mask(m: u32, d_old: f64, d_new: f64, max_new: usize) -> u32 {
    let mut out = 0u32;
    for k in 0..32u32 {
        if m >> k & 1 == 1 {
            let j = ((k as f64 * d_new / d_old.max(1e-9)).round() as usize).min(max_new.min(31));
            out |= 1 << j;
        }
    }
    out
}

/// A JACA rule moved to a grid of another degree: every birth and survival count scaled
/// ([`scale_mask`]).
pub fn scaled(j: &JacaRule, d_old: f64, d_new: f64, max_new: usize) -> JacaRule {
    let s = |m| scale_mask(m, d_old, d_new, max_new);
    JacaRule {
        pos_birth: s(j.pos_birth),
        pos_survive: s(j.pos_survive),
        neg_birth: s(j.neg_birth),
        neg_survive: s(j.neg_survive),
        blast: j.blast,
    }
}

/// The share of -1 cells a seed asks for, when it is a signed one.
fn negative_of(seed: &SeedSpec) -> Option<f64> {
    match seed {
        SeedSpec::Signed { negative, .. } => Some(*negative),
        SeedSpec::Pattern { fill, .. } => match fill.states {
            SeedStates::Signed { negative } => Some(negative),
            _ => None,
        },
        _ => None,
    }
}

/// The seed with its share of -1 cells set (a signed seed only).
fn with_negative(seed: &SeedSpec, n: f64) -> Option<SeedSpec> {
    let mut s = seed.clone();
    match &mut s {
        SeedSpec::Signed { negative, .. } => *negative = n,
        SeedSpec::Pattern { fill, .. } => match &mut fill.states {
            SeedStates::Signed { negative } => *negative = n,
            _ => return None,
        },
        _ => return None,
    }
    Some(s)
}

/// A count not in `m`, from `lo` to `deg` (none when every one is).
fn free_count(rng: &mut Rng, m: u32, lo: u32, deg: usize) -> Option<u32> {
    let hi = (deg as u32).min(31);
    let free: Vec<u32> = (lo..=hi).filter(|k| m >> k & 1 == 0).collect();
    (!free.is_empty()).then(|| free[rng.below(free.len() as u64) as usize])
}

/// A count in `m` (none unless it has two or more: a sign keeps at least one).
fn spare_count(rng: &mut Rng, m: u32) -> Option<u32> {
    let set: Vec<u32> = (0..32).filter(|k| m >> k & 1 == 1).collect();
    (set.len() >= 2).then(|| set[rng.below(set.len() as u64) as usize])
}

/// A variant of a JACA rule and its seed that helps the `weaker` sign: a survival or birth count
/// added to the weaker sign's rule, one taken from the stronger's, or a larger share of the weaker
/// sign in a signed seed. `deg` is the grid's largest neighbour count. `None` when nothing could
/// change.
pub fn balance_variant(
    j: &JacaRule,
    seed: &SeedSpec,
    weaker: Sign,
    deg: usize,
    rng: &mut Rng,
) -> Option<(JacaRule, SeedSpec)> {
    let mut r = j.clone();
    let start = rng.below(4);
    for t in 0..4 {
        let (wb, ws, sb, ss) = match weaker {
            Sign::Pos => (
                &mut r.pos_birth,
                &mut r.pos_survive,
                &mut r.neg_birth,
                &mut r.neg_survive,
            ),
            Sign::Neg => (
                &mut r.neg_birth,
                &mut r.neg_survive,
                &mut r.pos_birth,
                &mut r.pos_survive,
            ),
        };
        match (start + t) % 4 {
            0 => {
                if let Some(k) = free_count(rng, *ws, 0, deg) {
                    *ws |= 1 << k;
                    return Some((r, seed.clone()));
                }
            }
            1 => {
                if let Some(k) = free_count(rng, *wb, 1, deg) {
                    *wb |= 1 << k;
                    return Some((r, seed.clone()));
                }
            }
            2 => {
                let take_birth = rng.below(2) == 0;
                let m = if take_birth { sb } else { ss };
                if let Some(k) = spare_count(rng, *m) {
                    *m &= !(1 << k);
                    return Some((r, seed.clone()));
                }
            }
            _ => {
                if let Some(n) = negative_of(seed) {
                    let n2 = match weaker {
                        Sign::Neg => n + 0.1,
                        Sign::Pos => n - 0.1,
                    }
                    .clamp(0.05, 0.95);
                    if (n2 - n).abs() > 1e-9 {
                        return Some((j.clone(), with_negative(seed, n2)?));
                    }
                }
            }
        }
    }
    None
}

/// A rule another job found, to transplant and tune early in a search (`--parents-from`). Kept in
/// the job's spec, without decimals, so it survives a pack.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Parent {
    /// Where it came from: the job, its run, and why it was taken (a verdict, or `best`).
    pub job: i64,
    pub run: i64,
    pub why: String,
    pub rule: String,
    pub tiling: String,
    /// 0 touch, 1 edge.
    pub nb: u8,
    /// Its starting pattern, as JSON.
    pub seed: String,
    /// Its signs both held to the end ([`strong`]).
    #[serde(default)]
    pub strong: bool,
    /// The weaker sign, when it was losing ([`losing`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub weaker: Option<Sign>,
}

impl Parent {
    /// A parent from a stored run.
    pub fn of(job: i64, run: i64, why: &str, ev: &Eval) -> Parent {
        Parent {
            job,
            run,
            why: why.into(),
            rule: ev.cand.rule.clone(),
            tiling: ev.cand.tiling.clone(),
            nb: ev.cand.nb as u8,
            seed: serde_json::to_string(&ev.cand.seed).unwrap_or_default(),
            strong: strong(ev),
            weaker: losing(&ev.feat),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use jetcarp_core::explore::features::SignMeasures;

    fn signs(pos: f32, balance: f32, lost: f32) -> Features {
        Features {
            smooth: 0.4,
            entropy: 0.8,
            signs: Some(SignMeasures {
                pos,
                balance,
                sorted: 0.6,
                trade: 0.1,
                lost,
            }),
            ..Default::default()
        }
    }

    #[test]
    fn losing_sides_and_strong_parents() {
        assert_eq!(losing(&signs(0.2, 0.2, 1.0)), Some(Sign::Pos));
        assert_eq!(losing(&signs(0.9, 0.1, 1.0)), Some(Sign::Neg));
        assert_eq!(losing(&signs(0.9, 0.35, 0.8)), Some(Sign::Neg));
        assert_eq!(losing(&signs(0.5, 0.45, 1.0)), None);
        assert_eq!(losing(&signs(1.0, 0.0, 0.2)), None, "a Life-like run");
        assert_eq!(losing(&Features::default()), None);
    }

    #[test]
    fn counts_scale_with_the_degree() {
        let j = JacaRule::symmetric(1 << 3, (1 << 2) | (1 << 3), 1);
        let s = scaled(&j, 8.0, 6.0, 6);
        // 3 -> 2.25 -> 2; 2 -> 1.5 -> 2.
        assert_eq!((s.pos_birth, s.pos_survive, s.blast), (1 << 2, 1 << 2, 1));
        let s = scaled(&j, 6.0, 12.0, 12);
        assert_eq!((s.pos_birth, s.pos_survive), (1 << 6, (1 << 4) | (1 << 6)));
        // Clipped to the new grid's largest count.
        let big = JacaRule::symmetric(1 << 8, 1 << 7, 0);
        assert_eq!(scaled(&big, 8.0, 12.0, 10).neg_birth, 1 << 10);
    }

    #[test]
    fn balance_variants_help_the_weaker_sign() {
        let j = JacaRule::symmetric(1 << 3, (1 << 2) | (1 << 3), 0);
        let seed = SeedSpec::Signed {
            density: 0.4,
            negative: 0.5,
        };
        let mut rng = Rng::new(1);
        let (mut more, mut fewer, mut seeded) = (0, 0, 0);
        for _ in 0..200 {
            let (r, s) = balance_variant(&j, &seed, Sign::Neg, 8, &mut rng).unwrap();
            let bits = |m: u32| m.count_ones();
            if bits(r.neg_birth) + bits(r.neg_survive) > 3 {
                more += 1;
                assert_eq!((r.pos_birth, r.pos_survive), (j.pos_birth, j.pos_survive));
            } else if bits(r.pos_birth) + bits(r.pos_survive) < 3 {
                fewer += 1;
                assert_eq!((r.neg_birth, r.neg_survive), (j.neg_birth, j.neg_survive));
            } else {
                assert_eq!(r, j);
                assert_eq!(negative_of(&s), Some(0.6));
                seeded += 1;
            }
        }
        assert!(
            more > 0 && fewer > 0 && seeded > 0,
            "{more} {fewer} {seeded}"
        );
        // A seed of the weaker positive sign: fewer -1 cells.
        let mut rng = Rng::new(2);
        let p = (0..50)
            .filter_map(|_| balance_variant(&j, &seed, Sign::Pos, 8, &mut rng))
            .filter_map(|(_, s)| negative_of(&s))
            .find(|n| *n != 0.5);
        assert_eq!(p, Some(0.4));
    }
}
