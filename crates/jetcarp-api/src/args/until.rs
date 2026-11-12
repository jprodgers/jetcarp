//! When a run stops.

use super::json::{get, get_f64, get_u64};
use jetcarp_core::script::Trigger;
use serde_json::Value;

/// When a run stops: a trigger (as in scene scripts, plus `done`) and a generation limit.
#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub struct Until {
    pub trigger: Trigger,
    pub max_gens: u64,
    /// The trigger was given (`until`). Without one a run counts `max_gens` generations, and
    /// says it stopped at `max_gens`.
    #[serde(skip)]
    pub given: bool,
}

/// Keys accepted by [`parse_until`].
pub const UNTIL_KEYS: &[&str] = &[
    "until",
    "max_gens",
    "gens",
    "n",
    "fraction",
    "for_gens",
    "max_period",
];

/// `until` (`"done"`, `"repeats"`, `"empty"`, `"generations"`, `"population_below"`,
/// `"population_above"`, `"activity_below"`, `"activity_above"`, `"never"`, or a scene-format
/// trigger map), with `n`, `fraction`, `for_gens`, `max_period`, and `max_gens` (or `gens`).
/// Without `until`, runs `gens`/`max_gens` generations.
pub fn parse_until(v: &Value, default_max: u64) -> Result<Until, String> {
    let max_gens = get_u64(v, "max_gens")?.or(get_u64(v, "gens")?);
    let n = get_u64(v, "n")?;
    let trigger = match get(v, "until") {
        None => Trigger::Generations {
            n: max_gens.or(n).unwrap_or(default_max),
        },
        Some(t @ Value::Object(_)) => {
            serde_json::from_value(t.clone()).map_err(|e| format!("bad until map: {e}"))?
        }
        Some(Value::String(s)) => {
            let fraction = get_f64(v, "fraction")?;
            let for_gens = get_u64(v, "for_gens")?.unwrap_or(1);
            let need_n = |what: &str| n.ok_or(format!("until: \"{what}\" needs `n`"));
            match s
                .trim()
                .to_ascii_lowercase()
                .replace([' ', '-'], "_")
                .as_str()
            {
                "done" | "finished" | "bottom" => Trigger::Done,
                "repeats" | "repeat" | "settled" | "periodic" => Trigger::Repeats {
                    max_period: get_u64(v, "max_period")?.unwrap_or(1000),
                },
                "frozen" => Trigger::Repeats { max_period: 1 },
                "empty" | "dead" | "died" => Trigger::PopulationBelow { n: 0 },
                "generations" | "gens" => Trigger::Generations {
                    n: n.or(max_gens).unwrap_or(default_max),
                },
                "population_below" => Trigger::PopulationBelow {
                    n: need_n("population_below")?,
                },
                "population_above" => Trigger::PopulationAbove {
                    n: need_n("population_above")?,
                },
                "activity_below" => Trigger::ActivityBelow {
                    fraction: fraction.ok_or("until: \"activity_below\" needs `fraction`")?,
                    for_gens,
                },
                "activity_above" => Trigger::ActivityAbove {
                    fraction: fraction.ok_or("until: \"activity_above\" needs `fraction`")?,
                    for_gens,
                },
                "never" => Trigger::Never,
                other => {
                    return Err(format!(
                        "unknown until \"{other}\"; use done, repeats, frozen, empty, generations, population_below, population_above, activity_below, activity_above, or never"
                    ));
                }
            }
        }
        Some(x) => return Err(format!("`until` should be text like \"done\", got {x}")),
    };
    let max_gens = match (&trigger, max_gens) {
        (_, Some(m)) => m,
        (Trigger::Generations { n }, None) => *n,
        _ => default_max,
    };
    Ok(Until {
        trigger,
        max_gens,
        given: get(v, "until").is_some(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn triggers_and_limits() {
        let u = parse_until(&json!({"until": "done", "max_gens": 5000}), 1000).unwrap();
        assert_eq!(
            u,
            Until {
                trigger: Trigger::Done,
                max_gens: 5000,
                given: true,
            }
        );
        let u = parse_until(&json!({"gens": 50}), 1000).unwrap();
        assert_eq!(u.trigger, Trigger::Generations { n: 50 });
        assert_eq!(u.max_gens, 50);
        assert!(!u.given);
        let u = parse_until(&json!({"until": "Population below", "n": 3}), 1000).unwrap();
        assert_eq!(u.trigger, Trigger::PopulationBelow { n: 3 });
        assert_eq!(u.max_gens, 1000);
        assert!(parse_until(&json!({"until": "population_above"}), 1000).is_err());
        assert!(parse_until(&json!({"until": "soon"}), 1000).is_err());
    }
}
