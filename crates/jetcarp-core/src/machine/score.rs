//! The JETCARP Hardware Score: its own arbitrary unit, from work JETCARP already does. Score v1
//! is built on Game of Life soups on five non-square tilings (the hat, the Spectre, Penrose's
//! rhombs, the 3.4.6.4 Archimedean tiling and a 2-uniform tiling), each a fixed batch of soups
//! with fixed seeds, run the way JETCARP's explore searches run them ([`super::bench`]). The
//! score is a fixed formula of those rates: their geometric mean, in soups a second, times
//! [`SCALE`]. No machine is pinned to a value: whatever number a machine gets is its number.
//!
//! Its parts are kept and shown (one core, every core, each GPU, memory per cell, the disk),
//! because limits come from the parts, not the single number.

use serde::{Deserialize, Serialize};

/// The benchmark's version. A JETCARP that changes the workloads raises it, and older scores are
/// then shown as from an older benchmark, with a suggestion to measure again.
pub const BENCHMARK: u32 = 1;

/// The constant of score v1: the geometric mean of the five soup rates (soups a second) is
/// multiplied by this.
pub const SCALE: f64 = 1.0;

/// One measured part.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Part {
    /// `soups-hat`, `core-square`, `gpu-crossover`, `memory-life`, `disk`.
    pub id: String,
    /// In words: `Life soups on the hat`.
    pub label: String,
    /// `processor (32 threads)`, `processor (one core)`, or a GPU's name.
    pub device: String,
    pub value: f64,
    /// `soups/s`, `cell-generations/s`, `cells`, `bytes/cell`, `items/s`.
    pub unit: String,
    /// The answer check: the final state's digest equalled the one stored with the benchmark.
    /// None for parts with no answer (memory, the disk).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub correct: Option<bool>,
    /// Part of the score.
    #[serde(default)]
    pub scored: bool,
}

/// A run of the benchmark.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Score {
    /// [`BENCHMARK`] when it ran.
    pub benchmark: u32,
    /// The JETCARP that ran it.
    pub jetcarp: String,
    /// When, as `2026-11-21 14:05` local time.
    #[serde(default)]
    pub date: String,
    /// The JETCARP Hardware Score (0 when no device gave right answers).
    pub score: f64,
    pub parts: Vec<Part>,
    /// Wrong answers and devices that could not run, in words.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub problems: Vec<String>,
    /// A short run (answers only, as the tests run it): its rates mean little.
    #[serde(default)]
    pub quick: bool,
    /// How long it took, seconds.
    pub seconds: f64,
}

impl Score {
    /// The score from the parts: for each scored workload the fastest device that gave the right
    /// answer, then [`formula`].
    pub fn from_parts(parts: &[Part]) -> f64 {
        let mut ids: Vec<&str> = parts
            .iter()
            .filter(|p| p.scored)
            .map(|p| p.id.as_str())
            .collect();
        ids.sort();
        ids.dedup();
        let best: Vec<f64> = ids
            .iter()
            .map(|id| {
                parts
                    .iter()
                    .filter(|p| p.id == *id && p.correct == Some(true))
                    .map(|p| p.value)
                    .fold(0.0, f64::max)
            })
            .collect();
        formula(&best)
    }

    /// The part `id` from the fastest device that gave the right answer (or had none to give):
    /// what the defaults that depend on time read (`gpu-crossover` for the GPU thresholds, the
    /// one-core and soup rates for Fast Fit's starting sizes).
    pub fn best(&self, id: &str) -> Option<&Part> {
        self.parts
            .iter()
            .filter(|p| p.id == id && p.correct != Some(false))
            .max_by(|a, b| a.value.total_cmp(&b.value))
    }

    /// The parts with a wrong answer.
    pub fn wrong(&self) -> impl Iterator<Item = &Part> {
        self.parts.iter().filter(|p| p.correct == Some(false))
    }

    /// The score as people read it: `1,234`, or `none (wrong answers)`.
    pub fn text(&self) -> String {
        if self.score <= 0.0 {
            return "none: no device gave the right answers".into();
        }
        super::count_text(self.score.round() as u64)
    }

    /// The report in Markdown: the machine, the score and its parts, and the limits.
    pub fn markdown(
        &self,
        hw: &super::Hardware,
        limits: &super::Limits,
        gpu_names: &[String],
    ) -> String {
        use std::fmt::Write as _;
        let mut s = String::new();
        let _ = writeln!(s, "# JETCARP Hardware Score\n");
        let _ = writeln!(
            s,
            "**Score v{}: {}**{} (JETCARP {}, {}, {:.0} s)\n",
            self.benchmark,
            self.text(),
            if self.quick { " from a short run" } else { "" },
            self.jetcarp,
            self.date,
            self.seconds
        );
        let _ = writeln!(s, "## The machine\n");
        let _ = writeln!(s, "| | |\n| --- | --- |");
        let _ = writeln!(s, "| System | {} {} |", hw.os, hw.arch);
        let _ = writeln!(
            s,
            "| Processor | {} ({} threads{}) |",
            hw.cpu,
            hw.threads,
            hw.cores.map_or(String::new(), |c| format!(", {c} cores"))
        );
        let _ = writeln!(s, "| Memory | {} |", super::bytes_text(hw.memory_total));
        for g in &hw.gpus {
            let _ = writeln!(
                s,
                "| GPU memory | {}{}{} ({}) |",
                if g.name.is_empty() {
                    format!("{:04x}:{:04x} ", g.vendor, g.device)
                } else {
                    format!("{} ", g.name)
                },
                g.dedicated
                    .map_or(String::new(), |d| format!("{} own", super::bytes_text(d))),
                g.shared.map_or(String::new(), |d| format!(
                    ", {} shared",
                    super::bytes_text(d)
                )),
                g.source
            );
        }
        for n in gpu_names {
            let _ = writeln!(s, "| GPU | {n} |");
        }
        let _ = writeln!(s, "\n## The parts\n");
        let _ = writeln!(
            s,
            "| Part | Device | Value | Answer |\n| --- | --- | --- | --- |"
        );
        for p in &self.parts {
            let _ = writeln!(
                s,
                "| {}{} | {} | {} {} | {} |",
                p.label,
                if p.scored { " (scored)" } else { "" },
                p.device,
                rate_text(p.value),
                p.unit,
                match p.correct {
                    Some(true) => "right",
                    Some(false) => "**wrong**",
                    None => "",
                }
            );
        }
        if !self.problems.is_empty() {
            let _ = writeln!(s, "\n## Problems\n");
            for p in &self.problems {
                let _ = writeln!(s, "- {p}");
            }
        }
        let _ = writeln!(s, "\n## The limits\n");
        let _ = writeln!(
            s,
            "| Limit | Value | From | Ceiling |\n| --- | --- | --- | --- |"
        );
        for e in &limits.entries {
            let _ = writeln!(
                s,
                "| {} | {} | {} | {} |",
                e.label,
                e.text(),
                match e.source {
                    super::Source::Own => "your own value".to_string(),
                    _ => e.why.clone(),
                },
                super::limits::value_text(e.unit, e.ceiling)
            );
        }
        s
    }
}

/// [`SCALE`] times the geometric mean of `rates`; 0 when any is 0 (a workload with no right
/// answer) or there are none.
pub fn formula(rates: &[f64]) -> f64 {
    if rates.is_empty() || rates.iter().any(|&r| r <= 0.0 || !r.is_finite()) {
        return 0.0;
    }
    let mean_log = rates.iter().map(|r| r.ln()).sum::<f64>() / rates.len() as f64;
    SCALE * mean_log.exp()
}

/// A rate with three significant figures: `1,234`, `56.7`, `0.123`, `2.5 billion`.
pub fn rate_text(v: f64) -> String {
    if v >= 1000.0 {
        super::count_text(v.round() as u64)
    } else if v >= 100.0 {
        format!("{v:.0}")
    } else if v >= 10.0 {
        format!("{v:.1}")
    } else {
        format!("{v:.3}")
            .trim_end_matches('0')
            .trim_end_matches('.')
            .to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn part(id: &str, v: f64, correct: bool) -> Part {
        Part {
            id: id.into(),
            value: v,
            correct: Some(correct),
            scored: true,
            ..Default::default()
        }
    }

    #[test]
    fn the_formula_is_a_geometric_mean() {
        assert_eq!(formula(&[]), 0.0);
        assert!((formula(&[100.0]) - 100.0 * SCALE).abs() < 1e-9);
        assert!((formula(&[10.0, 1000.0]) - 100.0 * SCALE).abs() < 1e-9);
        // A workload with no right answer leaves no score.
        assert_eq!(formula(&[10.0, 0.0]), 0.0);
    }

    #[test]
    fn the_fastest_right_device_counts() {
        let parts = vec![
            part("soups-hat", 100.0, true),
            // The GPU is faster but wrong: it does not count.
            part("soups-hat", 900.0, false),
            part("soups-spectre", 50.0, true),
            part("soups-spectre", 200.0, true),
            Part {
                id: "disk".into(),
                value: 1e6,
                ..Default::default()
            },
        ];
        assert!((Score::from_parts(&parts) - formula(&[100.0, 200.0])).abs() < 1e-9);
        // What the time-based defaults read: the fastest right device's part.
        let s = Score {
            parts: parts.clone(),
            ..Default::default()
        };
        assert_eq!(s.best("soups-hat").unwrap().value, 100.0);
        assert_eq!(s.best("soups-spectre").unwrap().value, 200.0);
        assert_eq!(s.best("disk").unwrap().value, 1e6);
        assert!(s.best("gpu-crossover").is_none());
        // Nothing right on a workload: no score.
        let none = vec![
            part("soups-hat", 100.0, false),
            part("soups-spectre", 5.0, true),
        ];
        assert_eq!(Score::from_parts(&none), 0.0);
    }

    #[test]
    fn rates_for_people() {
        assert_eq!(rate_text(1234.4), "1,234");
        assert_eq!(rate_text(56.71), "56.7");
        assert_eq!(rate_text(0.1234), "0.123");
        assert_eq!(rate_text(2.0), "2");
        assert_eq!(rate_text(2.5e9), "2.5 billion");
    }
}
