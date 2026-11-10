//! The stage 1 interest score: a weighted sum of features computed from a setting's
//! [`Descriptors`], 0 to 100, refit from ratings.
//!
//! **Features** (each 0 to 1; "band" terms are 1 inside a middle band and fall off outside it,
//! by a factor e for each factor of 3 past the band's edge, on a log scale):
//!
//! | Feature | From | Rewards |
//! | --- | --- | --- |
//! | `lives` | died, filled, froze early | soups that neither die, fill the grid, nor freeze in the first 5% |
//! | `transient` | settle generation | long transients (log scale; still active = 1, less for soups still boiling at the end: activity above 6%, none above 16%) |
//! | `activity` | mean activity | band 0.4% to 6% of cells changing per generation |
//! | `lasting` | activity in the last eighth | band 0.2% to 6% |
//! | `texture` | final frame's compressed size against noise | band 0.2 to 0.7 (linear, 0 at 0.25 outside): neither noise nor uniform; scaled down below 2% final density |
//! | `entropy` | final frame's block entropy | band 0.25 to 0.7 (linear, 0 at 0.25 outside); scaled down below 2% final density |
//! | `mid_texture` | middle frames' compressed size | band 0.25 to 0.75 (linear, 0 at 0.25 outside) |
//! | `density` | final density | band 3% to 45% |
//! | `ships` | spaceship kinds | log2(1 + kinds) / 2, at most 1 |
//! | `oscillators` | oscillator kinds | log2(1 + kinds) / 3 |
//! | `long_period` | longest object period | log2(period) / 6 |
//! | `variety` | distinct objects | log2(1 + distinct) / 4 |
//! | `symmetry` | objects with a symmetry | symmetric / distinct |
//! | `consistency` | spread across soups | lives x (1 - 2 x spread) |
//! | `density_robust` | spread against the density probes | lives x (1 - 2 x spread) |
//! | `novelty` | distance to the Quick Start and stored settings | lives x distance / 0.12 |
//! | `growth` | small soups that grow | log2(growth) / 4 (soups smaller than the grid only) |
//!
//! **Score** = clamp(bias + sum of weight x feature, 0, 100). The hand-set weights
//! ([`hand_weights`]) sum to 100, so a setting perfect on every count scores 100, and a setting
//! that dies, fills, or turns to noise scores low on most terms at once.
//!
//! **Refits** ([`refit`]). Ratings (1 to 5) of settings, by the user (weight 1) or a model
//! (weight 0.5), become targets `25 (rating - 1)`; ridge regression pulls the weights and bias
//! towards the current model: minimise `sum c (y - b - w.f)^2 + lambda |w - w0|^2 +
//! lambda |b - b0|^2`, solved exactly (normal equations, Gaussian elimination with partial
//! pivoting), so the result is deterministic. Fewer than [`MIN_RATED`] rated settings are
//! refused.

use super::eval::Descriptors;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Feature names, in order.
pub const FEATURES: [&str; 17] = [
    "lives",
    "transient",
    "activity",
    "lasting",
    "texture",
    "entropy",
    "mid_texture",
    "density",
    "ships",
    "oscillators",
    "long_period",
    "variety",
    "symmetry",
    "consistency",
    "density_robust",
    "novelty",
    "growth",
];

/// Rated settings needed for a refit.
pub const MIN_RATED: usize = 20;

/// Default thresholds: above `AUTO` a setting is marked `auto`; from `CANDIDATE`
/// it waits for approval.
pub const AUTO: f64 = 70.0;
pub const CANDIDATE: f64 = 55.0;

/// The hand-set weights (they sum to 100).
pub fn hand_weights() -> BTreeMap<String, f64> {
    [
        ("lives", 12.0),
        ("transient", 6.0),
        ("activity", 8.0),
        ("lasting", 7.0),
        ("texture", 8.0),
        ("entropy", 6.0),
        ("mid_texture", 4.0),
        ("density", 4.0),
        ("ships", 12.0),
        ("oscillators", 5.0),
        ("long_period", 4.0),
        ("variety", 5.0),
        ("symmetry", 1.0),
        ("consistency", 4.0),
        ("density_robust", 3.0),
        ("novelty", 9.0),
        ("growth", 2.0),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_string(), v))
    .collect()
}

/// 1 inside `[lo, hi]`, falling off by a factor e per factor of 3 outside (log scale).
pub fn band(x: f64, lo: f64, hi: f64) -> f64 {
    if x.is_nan() || x <= 0.0 {
        return 0.0;
    }
    let l3 = 3f64.ln();
    if x < lo {
        let z = (lo / x).ln() / l3;
        (-z * z).exp()
    } else if x > hi {
        let z = (x / hi).ln() / l3;
        (-z * z).exp()
    } else {
        1.0
    }
}

/// 1 inside `[lo, hi]`, falling linearly to 0 at `width` outside (for ratios near 0 to 1).
pub fn band_lin(x: f64, lo: f64, hi: f64, width: f64) -> f64 {
    if x < lo {
        (1.0 - (lo - x) / width).max(0.0)
    } else if x > hi {
        (1.0 - (x - hi) / width).max(0.0)
    } else {
        1.0
    }
}

fn log_cap(x: f64, div: f64) -> f64 {
    ((1.0 + x).log2() / div).clamp(0.0, 1.0)
}

/// The features of a setting (see the module notes), in [`FEATURES`] order.
pub fn features(d: &Descriptors, small_soup: bool) -> [f64; 17] {
    let lives = ((1.0 - d.died) * (1.0 - d.filled) * (1.0 - d.froze_early)).clamp(0.0, 1.0);
    let symmetry = if d.distinct > 0 {
        d.symmetric as f64 / d.distinct as f64
    } else {
        0.0
    };
    // Soups still busy at the end count as long transients only while their activity is
    // moderate: a run that boils to the end is noise, not a transient.
    let boiling = d.active * ((d.activity_end - 0.06) / 0.1).clamp(0.0, 1.0);
    // A nearly empty final frame has no texture to speak of.
    let present = (d.final_density / 0.02).clamp(0.0, 1.0);
    [
        lives,
        d.transient.clamp(0.0, 1.0) * (1.0 - boiling),
        band(d.activity_mean, 0.004, 0.06),
        band(d.activity_end, 0.002, 0.06),
        present * band_lin(d.compress, 0.2, 0.7, 0.25),
        present * band_lin(d.entropy, 0.25, 0.7, 0.25),
        band_lin(d.compress_mid, 0.25, 0.75, 0.25),
        band(d.final_density, 0.03, 0.45),
        log_cap(d.ship_kinds as f64, 2.0),
        log_cap(d.oscillator_kinds as f64, 3.0),
        if d.longest_period >= 2 {
            ((d.longest_period as f64).log2() / 6.0).min(1.0)
        } else {
            0.0
        },
        log_cap(d.distinct as f64, 4.0),
        symmetry,
        lives * (1.0 - 2.0 * d.spread).clamp(0.0, 1.0),
        lives * (1.0 - 2.0 * d.density_spread).clamp(0.0, 1.0),
        lives * (d.novelty.unwrap_or(0.0) / 0.12).clamp(0.0, 1.0),
        if small_soup && d.growth > 1.0 {
            (d.growth.log2() / 4.0).min(1.0)
        } else {
            0.0
        },
    ]
}

/// The descriptor vector novelty is measured on (each component 0 to 1).
pub fn novelty_vector(d: &Descriptors) -> [f64; 16] {
    let f = features(d, false);
    [
        d.died,
        d.filled,
        d.settled,
        d.active,
        d.transient,
        (d.activity_mean * 10.0).min(1.0),
        (d.activity_end * 10.0).min(1.0),
        d.final_density.min(1.0),
        d.compress.min(1.0),
        d.entropy,
        d.compress_mid.min(1.0),
        d.entropy_mid,
        f[8],
        f[9],
        f[10],
        f[11],
    ]
}

/// Root-mean-square distance between two novelty vectors.
pub fn distance(a: &[f64; 16], b: &[f64; 16]) -> f64 {
    (a.iter().zip(b).map(|(x, y)| (x - y) * (x - y)).sum::<f64>() / 16.0).sqrt()
}

/// One version of the score: weights, bias, thresholds, and how it was made.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ScoreModel {
    /// Row id in `score_models` (0: the built-in hand-set model, not stored yet).
    #[serde(default)]
    pub id: i64,
    /// `hand`, `refit`, or `thresholds` (same weights, new thresholds).
    pub source: String,
    pub weights: BTreeMap<String, f64>,
    pub bias: f64,
    pub auto: f64,
    pub candidate: f64,
    /// Ratings used (refits).
    #[serde(default)]
    pub ratings: usize,
    #[serde(default)]
    pub rated_settings: usize,
    #[serde(default)]
    pub lambda: f64,
    /// Root-mean-square error on the ratings' targets, before and after the fit.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rmse_before: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rmse_after: Option<f64>,
    #[serde(default)]
    pub created: String,
}

impl Default for ScoreModel {
    fn default() -> Self {
        ScoreModel::hand()
    }
}

impl ScoreModel {
    /// The hand-set model with the default thresholds.
    pub fn hand() -> ScoreModel {
        ScoreModel {
            id: 0,
            source: "hand".into(),
            weights: hand_weights(),
            bias: 0.0,
            auto: AUTO,
            candidate: CANDIDATE,
            ratings: 0,
            rated_settings: 0,
            lambda: 0.0,
            rmse_before: None,
            rmse_after: None,
            created: String::new(),
        }
    }

    fn w(&self) -> [f64; 17] {
        let mut w = [0.0; 17];
        for (i, f) in FEATURES.iter().enumerate() {
            w[i] = self.weights.get(*f).copied().unwrap_or(0.0);
        }
        w
    }

    /// Raw linear score of features (not clamped).
    pub fn raw(&self, f: &[f64; 17]) -> f64 {
        self.bias + self.w().iter().zip(f).map(|(a, b)| a * b).sum::<f64>()
    }

    /// The score, 0 to 100, rounded to 0.01.
    pub fn score(&self, f: &[f64; 17]) -> f64 {
        (self.raw(f).clamp(0.0, 100.0) * 100.0).round() / 100.0
    }

    /// Each feature's contribution (weight x feature), for explaining a score.
    pub fn terms(&self, f: &[f64; 17]) -> BTreeMap<String, f64> {
        let w = self.w();
        FEATURES
            .iter()
            .enumerate()
            .map(|(i, n)| (n.to_string(), (w[i] * f[i] * 100.0).round() / 100.0))
            .collect()
    }

    /// `auto`, `candidate`, or `stored` for a score.
    pub fn status_for(&self, score: f64) -> &'static str {
        if score >= self.auto {
            "auto"
        } else if score >= self.candidate {
            "candidate"
        } else {
            "stored"
        }
    }
}

/// One rating sample for a refit: the rated setting's features, the rating (1 to 5), and the
/// sample weight (user 1, model 0.5).
#[derive(Clone, Debug)]
pub struct Sample {
    pub features: [f64; 17],
    pub rating: f64,
    pub weight: f64,
}

/// Fit a new model from rating samples by ridge regression towards `prior` (see the module
/// notes). `rated` is the number of distinct rated settings behind the samples.
pub fn refit(
    prior: &ScoreModel,
    samples: &[Sample],
    rated: usize,
    lambda: f64,
) -> Result<ScoreModel, String> {
    if rated < MIN_RATED {
        return Err(format!(
            "a refit needs ratings of at least {MIN_RATED} settings; there are {rated}"
        ));
    }
    if lambda.is_nan() || lambda <= 0.0 {
        return Err("lambda must be above 0".into());
    }
    const D: usize = 18;
    let w0 = prior.w();
    let mut theta0 = [0.0; D];
    theta0[0] = prior.bias;
    theta0[1..].copy_from_slice(&w0);
    let mut a = [[0.0f64; D]; D];
    let mut b = [0.0f64; D];
    for s in samples {
        let mut x = [0.0; D];
        x[0] = 1.0;
        x[1..].copy_from_slice(&s.features);
        let y = 25.0 * (s.rating.clamp(1.0, 5.0) - 1.0);
        for i in 0..D {
            b[i] += s.weight * x[i] * y;
            for j in 0..D {
                a[i][j] += s.weight * x[i] * x[j];
            }
        }
    }
    for i in 0..D {
        a[i][i] += lambda;
        b[i] += lambda * theta0[i];
    }
    let theta = solve(a, b).ok_or("the refit's equations are singular")?;
    let rmse = |m: &ScoreModel| -> f64 {
        let (mut se, mut wsum) = (0.0, 0.0);
        for s in samples {
            let e = m.raw(&s.features).clamp(0.0, 100.0) - 25.0 * (s.rating.clamp(1.0, 5.0) - 1.0);
            se += s.weight * e * e;
            wsum += s.weight;
        }
        (se / wsum.max(1e-9)).sqrt()
    };
    let mut m = ScoreModel {
        id: 0,
        source: "refit".into(),
        weights: FEATURES
            .iter()
            .enumerate()
            .map(|(i, f)| (f.to_string(), (theta[i + 1] * 1e4).round() / 1e4))
            .collect(),
        bias: (theta[0] * 1e4).round() / 1e4,
        auto: prior.auto,
        candidate: prior.candidate,
        ratings: samples.len(),
        rated_settings: rated,
        lambda,
        rmse_before: Some((rmse(prior) * 100.0).round() / 100.0),
        rmse_after: None,
        created: String::new(),
    };
    m.rmse_after = Some((rmse(&m) * 100.0).round() / 100.0);
    Ok(m)
}

/// Solve `a x = b` (Gaussian elimination with partial pivoting).
fn solve<const D: usize>(mut a: [[f64; D]; D], mut b: [f64; D]) -> Option<[f64; D]> {
    for c in 0..D {
        let p = (c..D).max_by(|&i, &j| a[i][c].abs().total_cmp(&a[j][c].abs()))?;
        if a[p][c].abs() < 1e-12 {
            return None;
        }
        a.swap(c, p);
        b.swap(c, p);
        for r in c + 1..D {
            let f = a[r][c] / a[c][c];
            if f != 0.0 {
                let pivot = a[c];
                for (x, p) in a[r][c..].iter_mut().zip(&pivot[c..]) {
                    *x -= f * p;
                }
                b[r] -= f * b[c];
            }
        }
    }
    let mut x = [0.0; D];
    for r in (0..D).rev() {
        let s: f64 = (r + 1..D).map(|k| a[r][k] * x[k]).sum();
        x[r] = (b[r] - s) / a[r][r];
    }
    Some(x)
}
