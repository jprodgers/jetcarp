//! Fitted score models: a person's taste, learnt from their keep, maybe and drop reviews, used in
//! place of the built-in score ([`crate::explore::archive::score`]).
//!
//! A model is a ridge logistic regression of keep against drop on the measures the built-in score
//! reads ([`BASE_TERMS`]), with a term per rule family when the reviews cover more than one, and
//! JACA's sign measures ([`JACA_TERMS`]) when they cover JACA runs. Maybe counts as half a keep:
//! leaving maybes out would drop nearly half of a typical rating round, and as 0.5 they pull their
//! runs to the middle without taking a side (the built-in score was fitted with maybe as 0.45).
//! Terms are standardised before the fit (ridge 8 on the measures, 5 on the family terms, the
//! built-in score's penalties) and turned back to raw units after it.
//!
//! **Scoring** keeps the built-in score's scale and its gates: a run that died scores
//! `0.25 x life`, one that froze or cycled early `0.1 + 0.6 x life + ...`, and a JACA run that
//! lost a sign in the first half keeps [`JACA_LOST_EARLY`](crate::explore::archive::JACA_LOST_EARLY)
//! of its score; the rest score `scale / (1 + exp(-linear))`, `scale` 3 as the built-in score, so
//! niches and the atlas's thresholds behave. Row rules (Wolfram, layers) keep the built-in score.
//!
//! **The file** is JSON ([`ScoreModel`], format [`MODEL_FORMAT`]): terms, weights, intercept,
//! what it was fitted on and how well it did. Its numbers read as JSON numbers or as decimal
//! strings (a pack writes decimals as strings), so a model kept in a job's spec survives a pack.

use crate::explore::archive::{gated, jaca_gate};
use jetcarp_core::explore::features::{Fate, Features};
use jetcarp_core::explore::space::Fam;
use jetcarp_core::rng::Rng;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;

/// What a model file's `format` says.
pub const MODEL_FORMAT: &str = "jetcarp-score-model 1";
/// What its `kind` says.
pub const KIND: &str = "logistic 1";
/// The output scale (as the built-in score's).
pub const SCALE: f64 = 3.0;
/// Ridge penalties on the standardised measures and on the family terms.
pub const RIDGE: f64 = 8.0;
pub const FAMILY_RIDGE: f64 = 5.0;
/// What a maybe counts as.
pub const MAYBE: f64 = 0.5;
/// Newton steps.
const STEPS: usize = 60;
/// Folds and repeats of the held-out AUC.
pub const FOLDS: usize = 5;
pub const REPEATS: usize = 8;
/// Reviewed runs needed for a fit, with at least this many keeps and drops each.
pub const MIN_RUNS: usize = 20;
pub const MIN_EACH: usize = 5;

/// The built-in score's terms, in its order.
pub const BASE_TERMS: [&str; 18] = [
    "settle_frac",
    "density_late",
    "sqrt_activity_late",
    "sqrt_activity_early",
    "sqrt_pop_cv",
    "growth",
    "entropy",
    "bits",
    "same_nb",
    "smooth",
    "sqrt_comps",
    "largest",
    "size_entropy",
    "patchiness",
    "travel",
    "movers",
    "froze",
    "cycle",
];

/// JACA's terms (0 for other families): its sign measures, and whether a sign was lost in the
/// first half.
pub const JACA_TERMS: [&str; 5] = [
    "jaca:sign_balance",
    "jaca:sign_sorted",
    "jaca:sign_trade",
    "jaca:sign_lost",
    "jaca:sign_lost_early",
];

/// A number read as a number or a decimal string.
fn num(v: &Value) -> Option<f64> {
    match v {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.trim().parse().ok(),
        _ => None,
    }
    .filter(|x: &f64| x.is_finite())
}

fn de_f64<'de, D: Deserializer<'de>>(d: D) -> Result<f64, D::Error> {
    let v = Value::deserialize(d)?;
    num(&v).ok_or_else(|| serde::de::Error::custom(format!("{v} is not a number")))
}

fn de_vec<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<f64>, D::Error> {
    let v = Value::deserialize(d)?;
    let Value::Array(a) = &v else {
        return Err(serde::de::Error::custom(format!("{v} is not a list")));
    };
    a.iter()
        .map(|x| num(x).ok_or_else(|| serde::de::Error::custom(format!("{x} is not a number"))))
        .collect()
}

/// What a model was fitted on.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct FittedOn {
    /// The explore jobs whose runs' reviews it learnt, and their searches (uuids).
    pub jobs: Vec<i64>,
    pub searches: Vec<String>,
    /// The raters whose reviews it learnt (all of them unless one was chosen).
    pub raters: Vec<String>,
    /// The families of the runs.
    pub families: Vec<String>,
    /// Reviews per verdict (one per rater and run: a rater's latest), and the runs they cover.
    pub keep: u64,
    pub maybe: u64,
    pub drop: u64,
    pub runs: u64,
    /// Runs whose stored measures lacked ones the model reads, replayed to measure them.
    pub replayed: u64,
    /// What a maybe counted as, and the ridge penalties.
    #[serde(deserialize_with = "de_f64")]
    pub maybe_as: f64,
    #[serde(deserialize_with = "de_f64")]
    pub ridge: f64,
    #[serde(deserialize_with = "de_f64")]
    pub family_ridge: f64,
    /// When (UTC) and by which JETCARP.
    pub created: String,
    pub engine_version: String,
}

/// How well a model did.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Metrics {
    /// AUC of keep against drop on runs held out of the fit: [`FOLDS`]-fold, the mean of
    /// [`REPEATS`] shuffles. 0.5 is chance, 1 perfect.
    #[serde(deserialize_with = "de_f64")]
    pub auc: f64,
    pub folds: u32,
    pub repeats: u32,
    /// The same AUC on the runs it was fitted to, and the built-in score's on the same runs.
    #[serde(deserialize_with = "de_f64")]
    pub auc_fitted: f64,
    #[serde(deserialize_with = "de_f64")]
    pub auc_builtin: f64,
}

/// A fitted score model (see the module notes).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ScoreModel {
    /// [`MODEL_FORMAT`].
    pub format: String,
    /// [`KIND`].
    pub kind: String,
    /// Only runs of this family; the others keep the built-in score. None: every family (but
    /// row rules).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub family: Option<String>,
    /// Term names ([`BASE_TERMS`], `family:<name>`, [`JACA_TERMS`]), with their weights in raw
    /// units.
    pub features: Vec<String>,
    #[serde(deserialize_with = "de_vec")]
    pub weights: Vec<f64>,
    #[serde(deserialize_with = "de_f64")]
    pub intercept: f64,
    /// What a term counts as when a run cannot give it (JACA's terms for a JACA run measured
    /// before sign measures: their mean over the fit, so they add nothing).
    #[serde(default, deserialize_with = "de_vec")]
    pub fill: Vec<f64>,
    /// Each term's standard deviation over the runs fitted (weight x sd: the term's pull per
    /// standard deviation, comparable across terms).
    #[serde(default, deserialize_with = "de_vec")]
    pub sd: Vec<f64>,
    /// The output: `scale / (1 + exp(-linear))`.
    #[serde(deserialize_with = "de_f64")]
    pub scale: f64,
    #[serde(default)]
    pub fitted_on: FittedOn,
    #[serde(default)]
    pub metrics: Metrics,
    /// A name for people (what it was fitted to, by default).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

/// One term of a run: `None` when the run cannot give it.
fn term(name: &str, fate: Fate, f: &Features, gens: u32, fam: Fam) -> Option<f64> {
    let fin = |x: f32| -> f64 {
        if x.is_finite() { x as f64 } else { 0.0 }
    };
    let x = match name {
        "settle_frac" => f.settle as f64 / gens.max(1) as f64,
        "density_late" => fin(f.density_late),
        "sqrt_activity_late" => fin(f.activity_late.max(0.0).sqrt()),
        "sqrt_activity_early" => fin(f.activity_early.max(0.0).sqrt()),
        "sqrt_pop_cv" => fin(f.pop_cv.max(0.0).sqrt()),
        "growth" => fin(f.growth),
        "entropy" => fin(f.entropy),
        "bits" => fin(f.bits),
        "same_nb" => fin(f.same_nb),
        "smooth" => fin(f.smooth),
        "sqrt_comps" => fin(f.comps_per_live.max(0.0).sqrt()),
        "largest" => fin(f.largest),
        "size_entropy" => fin(f.size_entropy),
        "patchiness" => fin(f.patchiness),
        "travel" => fin(f.travel),
        "movers" => f.movers.min(20) as f64 / 20.0,
        "froze" => (fate == Fate::Froze) as u32 as f64,
        "cycle" => (fate == Fate::Cycle) as u32 as f64,
        _ => {
            if let Some(fname) = name.strip_prefix("family:") {
                return Some((fam.name() == fname) as u32 as f64);
            }
            let m = name.strip_prefix("jaca:")?;
            if fam != Fam::Jaca {
                return Some(0.0);
            }
            let s = f.signs?;
            match m {
                "sign_balance" => fin(s.balance),
                "sign_sorted" => fin(s.sorted),
                "sign_trade" => fin(s.trade),
                "sign_lost" => fin(s.lost),
                "sign_lost_early" => s.lost_early() as u32 as f64,
                _ => return None,
            }
        }
    };
    Some(x)
}

fn known_term(name: &str) -> bool {
    BASE_TERMS.contains(&name)
        || JACA_TERMS.contains(&name)
        || name
            .strip_prefix("family:")
            .is_some_and(|f| crate::explore::store::fam_named(f).is_some())
}

impl ScoreModel {
    /// Read a model from JSON text, checked.
    pub fn from_json(text: &str) -> Result<ScoreModel, String> {
        let m: ScoreModel =
            serde_json::from_str(text).map_err(|e| format!("not a score model: {e}"))?;
        m.check()?;
        Ok(m)
    }

    /// A model from a JSON value (a file's text parsed, or a model kept in a spec), checked.
    pub fn from_value(v: &Value) -> Result<ScoreModel, String> {
        let m: ScoreModel =
            serde_json::from_value(v.clone()).map_err(|e| format!("not a score model: {e}"))?;
        m.check()?;
        Ok(m)
    }

    /// Read a model file.
    pub fn load(path: &std::path::Path) -> Result<ScoreModel, String> {
        let text =
            std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        Self::from_json(&text).map_err(|e| format!("{}: {e}", path.display()))
    }

    /// Is it a model this JETCARP can use?
    pub fn check(&self) -> Result<(), String> {
        if self.format != MODEL_FORMAT {
            return Err(format!(
                "a score model of format '{}', not '{MODEL_FORMAT}' (update JETCARP to use it)",
                self.format
            ));
        }
        if self.kind != KIND {
            return Err(format!("a score model of kind '{}', not '{KIND}'", self.kind));
        }
        if self.features.len() != self.weights.len()
            || (!self.fill.is_empty() && self.fill.len() != self.features.len())
            || (!self.sd.is_empty() && self.sd.len() != self.features.len())
        {
            return Err("a score model's features, weights, fill and sd differ in length".into());
        }
        if let Some(t) = self.features.iter().find(|t| !known_term(t)) {
            return Err(format!("a score model term this JETCARP does not know: '{t}'"));
        }
        if let Some(f) = &self.family
            && crate::explore::store::fam_named(f).is_none()
        {
            return Err(format!("a score model for no family JETCARP knows: '{f}'"));
        }
        if self.scale <= 0.0 || !self.scale.is_finite() {
            return Err("a score model's scale must be above 0".into());
        }
        Ok(())
    }

    /// Does it score runs of this family?
    pub fn applies(&self, fam: Fam) -> bool {
        !fam.is_rows()
            && self
                .family
                .as_deref()
                .is_none_or(|f| crate::explore::store::fam_named(f) == Some(fam))
    }

    /// Its linear part for a run.
    pub fn linear(&self, fate: Fate, f: &Features, gens: u32, fam: Fam) -> f64 {
        let mut z = self.intercept;
        for (i, (t, w)) in self.features.iter().zip(&self.weights).enumerate() {
            let x = term(t, fate, f, gens, fam)
                .unwrap_or_else(|| self.fill.get(i).copied().unwrap_or(0.0));
            z += w * x;
        }
        z
    }

    /// The score of a run (whatever its family: see [`applies`](Self::applies)).
    pub fn score(&self, fate: Fate, f: &Features, gens: u32, fam: Fam) -> f32 {
        if let Some(s) = gated(fate, f, gens) {
            return s;
        }
        let z = self.linear(fate, f, gens, fam);
        let s = (self.scale / (1.0 + (-z).exp())) as f32;
        jaca_gate(f, fam, s)
    }

    /// The terms by their pull per standard deviation (weight x sd), strongest first.
    pub fn strongest(&self) -> Vec<(String, f64)> {
        let mut v: Vec<(String, f64)> = self
            .features
            .iter()
            .zip(&self.weights)
            .enumerate()
            .map(|(i, (t, w))| (t.clone(), w * self.sd.get(i).copied().unwrap_or(1.0)))
            .collect();
        v.sort_by(|a, b| b.1.abs().total_cmp(&a.1.abs()).then(a.0.cmp(&b.0)));
        v
    }
}

/// A reviewed run, as a fit takes it.
#[derive(Clone, Debug)]
pub struct Rated {
    pub fate: Fate,
    pub feat: Features,
    pub gens: u32,
    pub fam: Fam,
    /// keep, maybe, or drop.
    pub verdict: String,
}

impl Rated {
    fn target(&self) -> f64 {
        match self.verdict.as_str() {
            "keep" => 1.0,
            "maybe" => MAYBE,
            _ => 0.0,
        }
    }
}

/// AUC of `keep` against `drop` scores: the chance a keep outscores a drop (ties count half).
pub fn auc(keep: &[f64], drop: &[f64]) -> f64 {
    if keep.is_empty() || drop.is_empty() {
        return f64::NAN;
    }
    let mut d = drop.to_vec();
    d.sort_by(|a, b| a.total_cmp(b));
    let mut sum = 0.0;
    for &k in keep {
        let below = d.partition_point(|x| *x < k);
        let upto = d.partition_point(|x| *x <= k);
        sum += below as f64 + 0.5 * (upto - below) as f64;
    }
    sum / (keep.len() * d.len()) as f64
}

/// The AUC of a scoring of reviewed runs.
fn auc_of(runs: &[Rated], score: &dyn Fn(&Rated) -> f64) -> f64 {
    let (mut k, mut d) = (Vec::new(), Vec::new());
    for r in runs {
        match r.verdict.as_str() {
            "keep" => k.push(score(r)),
            "drop" => d.push(score(r)),
            _ => {}
        }
    }
    auc(&k, &d)
}

/// Solve `a x = b` (a square, row-major) by Gaussian elimination with partial pivoting.
fn solve(mut a: Vec<f64>, mut b: Vec<f64>) -> Option<Vec<f64>> {
    let n = b.len();
    for c in 0..n {
        let p = (c..n).max_by(|&i, &j| a[i * n + c].abs().total_cmp(&a[j * n + c].abs()))?;
        if a[p * n + c].abs() < 1e-12 {
            return None;
        }
        if p != c {
            for k in 0..n {
                a.swap(p * n + k, c * n + k);
            }
            b.swap(p, c);
        }
        for r in c + 1..n {
            let f = a[r * n + c] / a[c * n + c];
            if f != 0.0 {
                for k in c..n {
                    a[r * n + k] -= f * a[c * n + k];
                }
                b[r] -= f * b[c];
            }
        }
    }
    let mut x = vec![0.0; n];
    for c in (0..n).rev() {
        let s: f64 = (c + 1..n).map(|k| a[c * n + k] * x[k]).sum();
        x[c] = (b[c] - s) / a[c * n + c];
    }
    Some(x)
}

/// Ridge logistic regression by Newton's method: `z` rows of `p` terms, soft targets `y`, a
/// penalty per term; the intercept (last) is not penalised. Returns the weights and intercept.
fn logistic(z: &[Vec<f64>], y: &[f64], pen: &[f64]) -> (Vec<f64>, f64) {
    let p = pen.len();
    let n = p + 1;
    let mut w = vec![0.0; n];
    for _ in 0..STEPS {
        let mut g = vec![0.0; n];
        let mut h = vec![0.0; n * n];
        for (row, &t) in z.iter().zip(y) {
            let lin: f64 = row.iter().zip(&w).map(|(a, b)| a * b).sum::<f64>() + w[p];
            let q = 1.0 / (1.0 + (-lin).exp());
            let s = (q * (1.0 - q)).max(1e-9);
            for i in 0..n {
                let xi = if i < p { row[i] } else { 1.0 };
                g[i] += (q - t) * xi;
                for j in i..n {
                    let xj = if j < p { row[j] } else { 1.0 };
                    h[i * n + j] += s * xi * xj;
                }
            }
        }
        for i in 0..p {
            g[i] += pen[i] * w[i];
            h[i * n + i] += pen[i];
        }
        for i in 0..n {
            for j in 0..i {
                h[i * n + j] = h[j * n + i];
            }
        }
        let Some(step) = solve(h, g) else { break };
        let mut moved = 0.0f64;
        for i in 0..n {
            w[i] -= step[i];
            moved = moved.max(step[i].abs());
        }
        if moved < 1e-10 {
            break;
        }
    }
    let b = w.pop().unwrap_or(0.0);
    (w, b)
}

/// How a fit is set up.
#[derive(Clone, Debug, Default)]
pub struct FitOptions {
    /// Only runs of this family (and the model scores only them).
    pub family: Option<Fam>,
}

/// Fit a model to reviewed runs (keep, maybe and drop; row rules are left out). `fitted_on` is
/// filled in by the caller but for its counts; the metrics are the held-out AUC and the rest.
pub fn fit(runs: &[Rated], opt: &FitOptions) -> Result<ScoreModel, String> {
    let runs: Vec<Rated> = runs
        .iter()
        .filter(|r| !r.fam.is_rows() && opt.family.is_none_or(|f| f == r.fam))
        .filter(|r| ["keep", "maybe", "drop"].contains(&r.verdict.as_str()))
        .cloned()
        .collect();
    let keeps = runs.iter().filter(|r| r.verdict == "keep").count();
    let drops = runs.iter().filter(|r| r.verdict == "drop").count();
    if runs.len() < MIN_RUNS || keeps < MIN_EACH || drops < MIN_EACH {
        return Err(format!(
            "a fit needs at least {MIN_RUNS} reviewed runs with {MIN_EACH} keeps and {MIN_EACH} drops; these have {} runs, {keeps} keeps and {drops} drops",
            runs.len()
        ));
    }
    let mut m = fit_once(&runs, opt);
    m.metrics.auc_fitted = auc_of(&runs, &|r| m.score(r.fate, &r.feat, r.gens, r.fam) as f64);
    m.metrics.auc_builtin = auc_of(&runs, &|r| {
        crate::explore::archive::score(r.fate, &r.feat, r.gens, r.fam) as f64
    });
    // Held out: each fold scored by a model fitted to the others.
    let mut rng = Rng::new(3);
    let mut sum = 0.0;
    let mut reps = 0;
    for _ in 0..REPEATS {
        let mut idx: Vec<usize> = (0..runs.len()).collect();
        for i in (1..idx.len()).rev() {
            let j = rng.below(i as u64 + 1) as usize;
            idx.swap(i, j);
        }
        let mut score = vec![0.0; runs.len()];
        for fold in 0..FOLDS {
            let (test, train): (Vec<usize>, Vec<usize>) =
                (0..idx.len()).partition(|k| k % FOLDS == fold);
            let tr: Vec<Rated> = train.iter().map(|&k| runs[idx[k]].clone()).collect();
            let mk = fit_once(&tr, opt);
            for &k in &test {
                let r = &runs[idx[k]];
                score[idx[k]] = mk.score(r.fate, &r.feat, r.gens, r.fam) as f64;
            }
        }
        let (mut k, mut d) = (Vec::new(), Vec::new());
        for (r, s) in runs.iter().zip(&score) {
            match r.verdict.as_str() {
                "keep" => k.push(*s),
                "drop" => d.push(*s),
                _ => {}
            }
        }
        let a = auc(&k, &d);
        if a.is_finite() {
            sum += a;
            reps += 1;
        }
    }
    m.metrics.auc = if reps > 0 { sum / reps as f64 } else { f64::NAN };
    m.metrics.folds = FOLDS as u32;
    m.metrics.repeats = REPEATS as u32;
    let fams: std::collections::BTreeSet<&str> = runs.iter().map(|r| r.fam.name()).collect();
    m.fitted_on.families = fams.into_iter().map(String::from).collect();
    m.fitted_on.keep = keeps as u64;
    m.fitted_on.drop = drops as u64;
    m.fitted_on.maybe = (runs.len() - keeps - drops) as u64;
    m.fitted_on.maybe_as = MAYBE;
    m.fitted_on.ridge = RIDGE;
    m.fitted_on.family_ridge = FAMILY_RIDGE;
    m.fitted_on.created = jetcarp_core::pattern::now_utc();
    m.fitted_on.engine_version = jetcarp_core::VERSION.into();
    Ok(m)
}

/// One fit (no metrics).
fn fit_once(runs: &[Rated], opt: &FitOptions) -> ScoreModel {
    let mut fams: Vec<Fam> = runs.iter().map(|r| r.fam).collect();
    fams.sort();
    fams.dedup();
    let with_jaca = fams.contains(&Fam::Jaca);
    let family_terms: Vec<Fam> = if fams.len() > 1 { fams.clone() } else { vec![] };
    let mut names: Vec<String> = BASE_TERMS.iter().map(|s| s.to_string()).collect();
    let nb = names.len();
    if with_jaca {
        names.extend(JACA_TERMS.iter().map(|s| s.to_string()));
    }
    let nj = names.len();
    names.extend(family_terms.iter().map(|f| format!("family:{}", f.name())));
    let p = names.len();
    // Raw terms (None: not measured).
    let raw: Vec<Vec<Option<f64>>> = runs
        .iter()
        .map(|r| {
            names[..nj]
                .iter()
                .map(|t| term(t, r.fate, &r.feat, r.gens, r.fam))
                .collect()
        })
        .collect();
    // Means and spreads: the measures over every run; JACA's over the JACA runs that have them.
    let mut mu = vec![0.0; nj];
    let mut sd = vec![1.0; nj];
    for k in 0..nj {
        let vals: Vec<f64> = runs
            .iter()
            .zip(&raw)
            .filter(|(r, _)| k < nb || r.fam == Fam::Jaca)
            .filter_map(|(_, x)| x[k])
            .collect();
        if vals.is_empty() {
            continue;
        }
        let m = vals.iter().sum::<f64>() / vals.len() as f64;
        let v = vals.iter().map(|x| (x - m) * (x - m)).sum::<f64>() / vals.len() as f64;
        mu[k] = m;
        sd[k] = v.sqrt() + 1e-9;
    }
    let z: Vec<Vec<f64>> = runs
        .iter()
        .zip(&raw)
        .map(|(r, x)| {
            let mut row = Vec::with_capacity(p);
            for k in 0..nj {
                let v = if k >= nb && r.fam != Fam::Jaca {
                    0.0
                } else {
                    x[k].map_or(0.0, |v| (v - mu[k]) / sd[k])
                };
                row.push(v);
            }
            for f in &family_terms {
                row.push((r.fam == *f) as u32 as f64);
            }
            row
        })
        .collect();
    let y: Vec<f64> = runs.iter().map(Rated::target).collect();
    let mut pen = vec![RIDGE; nj];
    pen.extend(std::iter::repeat_n(FAMILY_RIDGE, family_terms.len()));
    let (wz, b) = logistic(&z, &y, &pen);
    // Back to raw units. A JACA term's mean shifts only JACA runs: into the JACA family term, or
    // the intercept when every run is a JACA run.
    let mut weights = vec![0.0; p];
    let mut intercept = b;
    let mut jaca_shift = 0.0;
    for k in 0..nj {
        weights[k] = wz[k] / sd[k];
        if k < nb {
            intercept -= weights[k] * mu[k];
        } else {
            jaca_shift -= weights[k] * mu[k];
        }
    }
    for (i, f) in family_terms.iter().enumerate() {
        weights[nj + i] = wz[nj + i];
        if *f == Fam::Jaca {
            weights[nj + i] += jaca_shift;
        }
    }
    if with_jaca && family_terms.is_empty() {
        intercept += jaca_shift;
    }
    let mut fill = vec![0.0; p];
    fill[nb..nj].copy_from_slice(&mu[nb..nj]);
    let mut sds = vec![1.0; p];
    sds[..nj].copy_from_slice(&sd);
    ScoreModel {
        format: MODEL_FORMAT.into(),
        kind: KIND.into(),
        family: opt.family.map(|f| f.name().to_string()),
        features: names,
        weights,
        intercept,
        fill,
        sd: sds,
        scale: SCALE,
        fitted_on: FittedOn::default(),
        metrics: Metrics::default(),
        name: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auc_counts_ties_half() {
        assert_eq!(auc(&[3.0, 4.0], &[1.0, 2.0]), 1.0);
        assert_eq!(auc(&[1.0], &[1.0]), 0.5);
        assert_eq!(auc(&[1.0, 3.0], &[2.0]), 0.5);
        assert!(auc(&[], &[1.0]).is_nan());
    }

    fn jaca(lost: f32, sorted: f32, activity: f32) -> Features {
        Features {
            settle: 1024,
            density_late: 0.3,
            activity_late: activity,
            activity_early: 0.2,
            smooth: 0.4,
            entropy: 0.8,
            comps_per_live: 0.05,
            largest: 0.5,
            signs: Some(jetcarp_core::explore::features::SignMeasures {
                pos: if lost < 1.0 { 1.0 } else { 0.6 },
                balance: if lost < 1.0 { 0.0 } else { 0.4 },
                sorted,
                trade: 0.1,
                lost,
            }),
            ..Default::default()
        }
    }

    /// A taste that keeps sorted battles and drops runs that lost a sign is learnt, held out,
    /// and the model reads back from its file, numbers as decimal strings too.
    #[test]
    fn a_fit_learns_a_taste_and_reads_back() {
        let mut rng = Rng::new(11);
        let mut runs = Vec::new();
        for i in 0..120 {
            let sorted = rng.next_f64() as f32;
            let lost = if i % 3 == 0 { 0.2 } else { 1.0 };
            let act = rng.next_f64() as f32 * 0.5;
            let good = lost == 1.0 && sorted + 0.2 * (rng.next_f64() as f32) > 0.55;
            let verdict = if good {
                "keep"
            } else if lost == 1.0 && sorted > 0.3 {
                "maybe"
            } else {
                "drop"
            };
            runs.push(Rated {
                fate: Fate::Running,
                feat: jaca(lost, sorted, act),
                gens: 1024,
                fam: Fam::Jaca,
                verdict: verdict.into(),
            });
        }
        let m = fit(
            &runs,
            &FitOptions {
                family: Some(Fam::Jaca),
            },
        )
        .unwrap();
        assert!(m.metrics.auc > 0.85, "{:?}", m.metrics);
        assert_eq!(m.fitted_on.keep + m.fitted_on.maybe + m.fitted_on.drop, 120);
        let top = m.strongest();
        assert!(
            top[..3].iter().any(|(t, _)| t == "jaca:sign_sorted"),
            "{top:?}"
        );
        // Only JACA runs; row rules never.
        assert!(m.applies(Fam::Jaca) && !m.applies(Fam::LifeLike) && !m.applies(Fam::Wolfram));
        // A run that lost a sign early is gated low; a sorted battle scores high.
        let battle = m.score(Fate::Running, &jaca(1.0, 0.9, 0.1), 1024, Fam::Jaca);
        let lost = m.score(Fate::Running, &jaca(0.2, 0.9, 0.1), 1024, Fam::Jaca);
        assert!(battle > 1.5 && lost < 0.9 && battle <= 3.0, "{battle} {lost}");
        // A run without sign measures takes the fill: its JACA terms add nothing.
        let mut f = jaca(1.0, 0.9, 0.1);
        f.signs = None;
        let z = m.linear(Fate::Running, &f, 1024, Fam::Jaca);
        let mean: f64 = m
            .features
            .iter()
            .zip(&m.weights)
            .zip(&m.fill)
            .filter(|((t, _), _)| t.starts_with("jaca:"))
            .map(|((_, w), x)| w * x)
            .sum();
        let base = ScoreModel {
            features: m.features[..BASE_TERMS.len()].to_vec(),
            weights: m.weights[..BASE_TERMS.len()].to_vec(),
            fill: vec![],
            sd: vec![],
            ..m.clone()
        };
        assert!((z - base.linear(Fate::Running, &f, 1024, Fam::Jaca) - mean).abs() < 1e-9);
        // The file, and the same with every number as text (as a pack writes them).
        let text = serde_json::to_string_pretty(&m).unwrap();
        assert_eq!(ScoreModel::from_json(&text).unwrap(), m);
        let mut v: Value = serde_json::from_str(&text).unwrap();
        fn stringify(v: &mut Value) {
            match v {
                Value::Number(n) if n.as_f64().is_some_and(|x| x.fract() != 0.0) => {
                    *v = Value::String(n.to_string())
                }
                Value::Array(a) => a.iter_mut().for_each(stringify),
                Value::Object(o) => o.values_mut().for_each(stringify),
                _ => {}
            }
        }
        stringify(&mut v);
        assert_eq!(ScoreModel::from_value(&v).unwrap(), m);
        // Refused: another format, an unknown term.
        let mut bad = m.clone();
        bad.features[0] = "nonsense".into();
        assert!(bad.check().is_err());
        bad = m.clone();
        bad.format = "jetcarp-score-model 9".into();
        assert!(bad.check().is_err());
    }

    /// Several families: a term each, and JACA's sign terms touch only JACA runs.
    #[test]
    fn family_terms_and_jaca_terms_apart() {
        let mut rng = Rng::new(5);
        let mut runs = Vec::new();
        for i in 0..90 {
            let fam = if i % 2 == 0 { Fam::Jaca } else { Fam::Cyclic };
            let mut f = jaca(1.0, rng.next_f64() as f32, 0.1);
            if fam != Fam::Jaca {
                f.signs = None;
            }
            let verdict = match (fam, i % 3) {
                (Fam::Jaca, 0) => "keep",
                (Fam::Jaca, _) => "maybe",
                (_, 0) => "maybe",
                _ => "drop",
            };
            runs.push(Rated {
                fate: Fate::Running,
                feat: f,
                gens: 1024,
                fam,
                verdict: verdict.into(),
            });
        }
        let m = fit(&runs, &FitOptions::default()).unwrap();
        assert!(m.features.iter().any(|t| t == "family:jaca"));
        assert!(m.features.iter().any(|t| t == "family:cyclic"));
        assert!(m.applies(Fam::LifeLike) && m.family.is_none());
        let f = jaca(1.0, 0.5, 0.1);
        let mut g = f;
        g.signs = None;
        // A cyclic run's score does not depend on sign measures.
        assert_eq!(
            m.score(Fate::Running, &f, 1024, Fam::Cyclic),
            m.score(Fate::Running, &g, 1024, Fam::Cyclic)
        );
        assert!(
            m.score(Fate::Running, &f, 1024, Fam::Jaca)
                > m.score(Fate::Running, &f, 1024, Fam::Cyclic)
        );
    }

    /// The built-in score and niches: a JACA run that lost a sign early scores low and has a
    /// niche of its own; runs without sign measures keep their old bins.
    #[test]
    fn builtin_score_and_bins_see_a_lost_sign() {
        use crate::explore::archive::{JACA_LOST_EARLY, behaviour_bin, score};
        let kept = jaca(1.0, 0.8, 0.1);
        let early = jaca(0.2, 0.8, 0.1);
        let late = jaca(0.8, 0.8, 0.1);
        let s = score(Fate::Running, &kept, 1024, Fam::Jaca);
        let e = score(Fate::Running, &early, 1024, Fam::Jaca);
        assert!((e - s * JACA_LOST_EARLY).abs() < 1e-6, "{s} {e}");
        let mut none = kept;
        none.signs = None;
        let b = |f: &Features| behaviour_bin(Fate::Running, f, 1024);
        assert_eq!(b(&kept), b(&none));
        let bins = [b(&kept), b(&late), b(&early)];
        assert!(bins[0] != bins[1] && bins[1] != bins[2] && bins[0] != bins[2]);
    }

    #[test]
    fn newton_finds_a_separating_direction() {
        // y follows the first term; the second is noise.
        let mut rng = Rng::new(7);
        let mut z = Vec::new();
        let mut y = Vec::new();
        for _ in 0..400 {
            let a = rng.next_f64() * 2.0 - 1.0;
            let b = rng.next_f64() * 2.0 - 1.0;
            z.push(vec![a, b]);
            y.push(if a + 0.1 * (rng.next_f64() - 0.5) > 0.0 { 1.0 } else { 0.0 });
        }
        let (w, _) = logistic(&z, &y, &[1.0, 1.0]);
        assert!(w[0] > 5.0 && w[1].abs() < 1.0, "{w:?}");
    }
}
