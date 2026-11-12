//! Scripted sequences: each step chooses which CAs run, and a trigger moves on to the next step.
//!
//! Triggers are checked after every generation from the world's state, so a scripted run replays
//! exactly from its scene. "Oscillators of a set generation limit" map to `repeats`, and a
//! "rolling boil" maps to an activity threshold.

use serde::{Deserialize, Serialize};

/// A CA by id or by name.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum CaRef {
    Id(u32),
    Name(String),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "when", rename_all = "snake_case")]
pub enum Trigger {
    /// After this many generations in the step.
    Generations {
        #[serde(with = "crate::json_int::uint")]
        n: u64,
    },
    /// Live cells (summed over the running CAs' fields) at or below `n`.
    PopulationBelow {
        #[serde(with = "crate::json_int::uint")]
        n: u64,
    },
    /// Live cells at or above `n`.
    PopulationAbove {
        #[serde(with = "crate::json_int::uint")]
        n: u64,
    },
    /// Fewer than `fraction` of cells changed, for `for_gens` generations in a row.
    ActivityBelow {
        fraction: f64,
        #[serde(with = "crate::json_int::uint")]
        for_gens: u64,
    },
    /// More than `fraction` of cells changed, for `for_gens` generations in a row.
    ActivityAbove {
        fraction: f64,
        #[serde(with = "crate::json_int::uint")]
        for_gens: u64,
    },
    /// The state repeats exactly with a period of at most `max_period` (1 = frozen).
    Repeats {
        #[serde(with = "crate::json_int::uint")]
        max_period: u64,
    },
    /// No running CA can change anything any more ([`crate::World::done`]): an elementary rule
    /// reached the bottom row, the field froze or died out, a sandpile is stable, ...
    Done,
    /// Stay on this step.
    Never,
}

impl Trigger {
    pub fn needs_activity(&self) -> bool {
        matches!(
            self,
            Trigger::ActivityBelow { .. } | Trigger::ActivityAbove { .. }
        )
    }
    pub fn describe(&self) -> String {
        match self {
            Trigger::Generations { n } => format!("after {n} generations"),
            Trigger::PopulationBelow { n } => format!("when population ≤ {n}"),
            Trigger::PopulationAbove { n } => format!("when population ≥ {n}"),
            Trigger::ActivityBelow { fraction, for_gens } => {
                format!(
                    "when under {:.2}% of cells change for {for_gens} generations",
                    fraction * 100.0
                )
            }
            Trigger::ActivityAbove { fraction, for_gens } => {
                format!(
                    "when over {:.2}% of cells change for {for_gens} generations",
                    fraction * 100.0
                )
            }
            Trigger::Repeats { max_period } => {
                format!("when it repeats with period ≤ {max_period}")
            }
            Trigger::Done => "when it is done (nothing can change any more)".into(),
            Trigger::Never => "never".into(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ScriptStep {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub name: String,
    /// CAs that run during this step. Every other CA is switched off.
    pub run: Vec<CaRef>,
    pub until: Trigger,
    /// Optional safety limit: move on after this many generations even if `until` has not fired.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::json_int::uint_opt"
    )]
    pub max_gens: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Default)]
pub struct Script {
    pub steps: Vec<ScriptStep>,
    /// Start again from the first step after the last one.
    #[serde(default)]
    pub repeat: bool,
}

/// Where a running script is.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct ScriptState {
    pub step: usize,
    /// Generation when the current step began.
    pub started: u64,
    /// Consecutive generations meeting an activity condition.
    pub streak: u64,
    pub finished: bool,
    /// Fraction of cells that changed in the last generation (when measured).
    pub last_activity: Option<f64>,
}

/// Measurements after a generation, for trigger checks.
pub struct Metrics {
    pub generation: u64,
    pub population: u64,
    pub activity: Option<f64>,
    pub period: Option<u64>,
    /// [`crate::World::done`] after this generation (measured only for `done` triggers).
    pub done: bool,
}

/// Does `t` fire now? Updates the activity streak.
pub fn fires(t: &Trigger, st: &mut ScriptState, m: &Metrics) -> bool {
    let in_step = m.generation - st.started;
    match t {
        Trigger::Generations { n } => in_step >= *n,
        Trigger::PopulationBelow { n } => m.population <= *n,
        Trigger::PopulationAbove { n } => m.population >= *n,
        Trigger::ActivityBelow { fraction, for_gens }
        | Trigger::ActivityAbove { fraction, for_gens } => {
            let below = matches!(t, Trigger::ActivityBelow { .. });
            let a = m.activity.unwrap_or(if below { 1.0 } else { 0.0 });
            let ok = if below { a < *fraction } else { a > *fraction };
            st.streak = if ok { st.streak + 1 } else { 0 };
            st.streak >= (*for_gens).max(1)
        }
        Trigger::Repeats { max_period } => m.period.is_some_and(|p| p <= *max_period),
        Trigger::Done => m.done,
        Trigger::Never => false,
    }
}
