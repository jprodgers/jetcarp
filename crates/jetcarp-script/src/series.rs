//! Rule series for sweeps: templates (`"HexW3 {n}"` from/to), named series (`"W"`, `"HexW"`,
//! `"HexW3"`, `"HexT"`, `"HexT3 sum"`, `"HexT3"`, `"B/S"`), random samples of any numbered
//! series (`sample: n`, `sample_seed: s`), and plain lists.

use crate::spec::{get, get_str, get_u64};
use jetcarp_core::census;
use serde_json::Value;

/// One run of a sweep.
#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub struct Item {
    /// Position in the sweep (0-based).
    pub index: usize,
    /// The series number (`n` in a template), if the sweep has one.
    pub n: Option<u64>,
    pub rule: String,
    /// File name without extension, e.g. `HexW3-08229`, `W030`, `0007_B36_S23`.
    pub stem: String,
}

/// Named series: (names, template, last number).
const NAMED: &[(&[&str], &str, u64)] = &[
    (&["w", "wolfram", "elementary"], "W{n}", 255),
    (&["hexw", "hexw2"], "HexW {n}", 15),
    (&["hexw3"], "HexW3 {n}", 19682),
    (&["hext", "hext2"], "HexT {n}", 255),
    (&["hext sum", "hextsum", "hext2 sum"], "HexT sum {n}", 15),
    (&["hext3 sum", "hext3sum"], "HexT3 sum {n}", 2186),
    (&["hext3 sym", "hext3sym"], "HexT3 sym {n}", 387_420_488),
    // 3^27 rules: sweep a range (from/to) or a random sample (`sample: n`).
    (&["hext3"], "HexT3 {n}", 7_625_597_484_986),
];

/// Keys used by [`expand`].
pub const SERIES_KEYS: &[&str] = &[
    "rules",
    "from",
    "to",
    "birth",
    "survive",
    "states",
    "sample",
    "sample_seed",
];

/// The runs of a sweep from `rules` and `from`/`to` (templates and named series), or
/// `birth`/`survive`/`states`/`sample` (the `"B/S"` series).
pub fn expand(v: &Value) -> Result<Vec<Item>, String> {
    let from = get_u64(v, "from")?;
    let to = get_u64(v, "to")?;
    match get(v, "rules") {
        None => Err("a sweep needs `rules`: a template like \"HexW3 {n}\" with from/to, a series name (\"W\", \"HexW\", \"HexW3\", \"B/S\"), or a list of rules".into()),
        Some(Value::Array(a)) => {
            let rules: Vec<String> = a
                .iter()
                .map(|r| r.as_str().map(String::from).ok_or(format!("rule list items must be text, got {r}")))
                .collect::<Result<_, _>>()?;
            Ok(list(rules))
        }
        Some(Value::String(s)) => {
            let low = s.split_whitespace().collect::<Vec<_>>().join(" ").to_ascii_lowercase();
            let sample = get_u64(v, "sample")?.map(|n| (n, get_u64(v, "sample_seed").ok().flatten().unwrap_or(1)));
            if let Some((_, t, last)) = NAMED.iter().find(|(names, _, _)| names.contains(&low.as_str())) {
                if sample.is_none() && to.is_none() && *last >= 10_000_000 {
                    return Err(format!(
                        "{s} has {} rules: give `sample: n` (with `sample_seed`) for a random sample, or from/to",
                        *last as u128 + 1
                    ));
                }
                return numbered(t, from.unwrap_or(0), to.unwrap_or(*last), *last, sample);
            }
            if matches!(low.as_str(), "b/s" | "bs" | "life-like" | "lifelike") {
                return bs(v);
            }
            if s.contains("{n}") {
                let last = NAMED
                    .iter()
                    .find(|(_, t, _)| t.eq_ignore_ascii_case(s.trim()))
                    .map(|x| x.2);
                let to = to.or(last).ok_or("a rule template needs `to` (the last number)")?;
                return numbered(s.trim(), from.unwrap_or(0), to, last.unwrap_or(to), sample);
            }
            Ok(list(census::parse_rule_list(s)))
        }
        Some(x) => Err(format!("`rules` should be text or a list, got {x}")),
    }
}

/// A numbered series, whole or as a random sample of `(count, seed)`.
fn numbered(
    t: &str,
    from: u64,
    to: u64,
    last: u64,
    sample: Option<(u64, u64)>,
) -> Result<Vec<Item>, String> {
    match sample {
        Some((n, seed)) => sampled(t, from, to, last, n, seed),
        None => template(t, from, to, last),
    }
}

/// `n` distinct numbers drawn uniformly from `from..=to` (SplitMix64 from `seed`, so the same
/// seed gives the same sample on every machine), in increasing order.
pub fn sample_numbers(from: u64, to: u64, n: u64, seed: u64) -> Vec<u64> {
    let span = to - from + 1;
    if n >= span {
        return (from..=to).collect();
    }
    let mut rng = jetcarp_core::rng::Rng::new(seed);
    let mut set = std::collections::BTreeSet::new();
    while (set.len() as u64) < n {
        set.insert(from + rng.below(span));
    }
    set.into_iter().collect()
}

fn sampled(t: &str, from: u64, to: u64, last: u64, n: u64, seed: u64) -> Result<Vec<Item>, String> {
    if from > to {
        return Err(format!("from ({from}) is after to ({to})"));
    }
    if to > last {
        return Err(format!("this series ends at {last}, but to is {to}"));
    }
    if n == 0 || n > 10_000_000 {
        return Err("`sample` takes 1 to 10 million runs".into());
    }
    let width = digits(last.max(to));
    let stem_t = file_safe(t);
    Ok(sample_numbers(from, to, n, seed)
        .into_iter()
        .enumerate()
        .map(|(index, n)| Item {
            index,
            n: Some(n),
            rule: t.replace("{n}", &n.to_string()),
            stem: stem_t.replace("{n}", &format!("{n:0width$}")),
        })
        .collect())
}

fn template(t: &str, from: u64, to: u64, last: u64) -> Result<Vec<Item>, String> {
    if from > to {
        return Err(format!("from ({from}) is after to ({to})"));
    }
    if to > last {
        return Err(format!("this series ends at {last}, but to is {to}"));
    }
    if to - from >= 10_000_000 {
        return Err("at most 10 million runs in one sweep".into());
    }
    let width = digits(last.max(to));
    let stem_t = file_safe(t);
    Ok((from..=to)
        .enumerate()
        .map(|(index, n)| Item {
            index,
            n: Some(n),
            rule: t.replace("{n}", &n.to_string()),
            stem: stem_t.replace("{n}", &format!("{n:0width$}")),
        })
        .collect())
}

fn list(rules: Vec<String>) -> Vec<Item> {
    let width = digits(rules.len().saturating_sub(1) as u64);
    rules
        .into_iter()
        .enumerate()
        .map(|(index, rule)| Item {
            index,
            n: None,
            stem: format!("{index:0width$}_{}", file_safe(&rule)),
            rule,
        })
        .collect()
}

fn bs(v: &Value) -> Result<Vec<Item>, String> {
    let birth = census::parse_digits(get_str(v, "birth")?.unwrap_or("1-8"))?;
    let survive = census::parse_digits(get_str(v, "survive")?.unwrap_or("0-8"))?;
    let states = get_u64(v, "states")?.map(|s| s.clamp(2, 255) as u8);
    let rules = match get_u64(v, "sample")? {
        Some(n) => census::sample_bs_rules(
            &birth,
            &survive,
            states,
            n as usize,
            get_u64(v, "sample_seed")?.unwrap_or(1),
        ),
        None => census::all_bs_rules(&birth, &survive, states)?,
    };
    Ok(list(rules))
}

fn digits(n: u64) -> usize {
    n.max(1).to_string().len()
}

/// A rule string as part of a file name: spaces become `-`, slashes and other characters that
/// are not allowed in file names become `_`.
pub fn file_safe(s: &str) -> String {
    s.trim()
        .chars()
        .map(|c| match c {
            ' ' => '-',
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            c if c.is_control() => '_',
            c => c,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn names_and_padding() {
        let v = expand(&json!({"rules": "HexW3 {n}", "from": 0, "to": 19682})).unwrap();
        assert_eq!(v.len(), 19683);
        assert_eq!(v[8229].rule, "HexW3 8229");
        assert_eq!(v[8229].stem, "HexW3-08229");
        let v = expand(&json!({"rules": "W"})).unwrap();
        assert_eq!(v.len(), 256);
        assert_eq!(v[30].stem, "W030");
        let v = expand(&json!({"rules": "HexW", "from": 3, "to": 5})).unwrap();
        assert_eq!(
            v.iter().map(|i| i.stem.as_str()).collect::<Vec<_>>(),
            ["HexW-03", "HexW-04", "HexW-05"]
        );
        let v = expand(&json!({"rules": ["B3/S23", "B36/S23"]})).unwrap();
        assert_eq!(v[1].stem, "1_B36_S23");
        let v = expand(&json!({"rules": "B/S", "birth": "2-3", "survive": "2-3"})).unwrap();
        assert_eq!(v.len(), 16);
        assert!(expand(&json!({"rules": "W", "to": 300})).is_err());
        let v = expand(&json!({"rules": "HexT"})).unwrap();
        assert_eq!(
            (v.len(), v[30].rule.as_str(), v[30].stem.as_str()),
            (256, "HexT 30", "HexT-030")
        );
        let v = expand(&json!({"rules": "HexT3 sum"})).unwrap();
        assert_eq!((v.len(), v[777].stem.as_str()), (2187, "HexT3-sum-0777"));
        // The full ternary space only as a sample (or a range); samples are reproducible.
        assert!(expand(&json!({"rules": "HexT3"})).is_err());
        let a = expand(&json!({"rules": "HexT3", "sample": 50, "sample_seed": 7})).unwrap();
        let b = expand(&json!({"rules": "HexT3", "sample": 50, "sample_seed": 7})).unwrap();
        let c = expand(&json!({"rules": "HexT3", "sample": 50, "sample_seed": 8})).unwrap();
        assert_eq!(a, b);
        assert_ne!(a, c);
        assert_eq!(a.len(), 50);
        assert!(a.windows(2).all(|w| w[0].n < w[1].n));
        assert_eq!(a[0].stem.len(), "HexT3-".len() + 13);
        for it in &a {
            jetcarp_core::rules::Rule::parse(&it.rule).unwrap();
        }
        let v = expand(&json!({"rules": "HexT3 sum {n}", "from": 0, "to": 2186, "sample": 10}))
            .unwrap();
        assert_eq!(v.len(), 10);
    }
}
