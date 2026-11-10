//! Jobs of kinds without a strategy: listed and stoppable, never run.

use super::{Strategy, StrategyKind, base_summary, short_list};
use crate::dashboard::JobSummary;
use crate::library::Library;
use crate::library::store::JobRow;
use serde_json::Value;

pub struct OtherKind;
pub(super) static KIND: OtherKind = OtherKind;

/// A summary from what any job row has: the rules (or rule) of its spec for a title, and up to
/// eight numbers or texts of its progress as counters.
pub fn summary(mut s: JobSummary, row: &JobRow) -> JobSummary {
    s.title = row
        .spec
        .get("rules")
        .and_then(|r| r.as_array())
        .map(|a| {
            let v: Vec<String> = a
                .iter()
                .filter_map(|x| x.as_str().map(String::from))
                .collect();
            short_list(&v, 3)
        })
        .or_else(|| {
            row.spec
                .get("rule")
                .and_then(|r| r.as_str())
                .map(String::from)
        })
        .unwrap_or_default();
    s.unit = "items".into();
    if let Some(obj) = row.progress.as_object() {
        for (k, v) in obj.iter().take(8) {
            if v.is_number() || v.is_string() {
                s.counters.push((k.replace('_', " "), v.to_string()));
            }
        }
    }
    s
}

impl StrategyKind for OtherKind {
    fn kind(&self) -> &'static str {
        "other"
    }
    fn label(&self) -> &'static str {
        "Job"
    }
    fn deterministic(&self) -> bool {
        false
    }
    fn runnable(&self, _row: &JobRow) -> bool {
        false
    }
    fn create(&self, _lib: &Library, _spec: &Value) -> Result<i64, String> {
        Err("jobs of other kinds are made by their own tools".into())
    }
    fn build(&self, row: &JobRow) -> Result<Box<dyn Strategy + Send>, String> {
        Err(format!(
            "{} jobs cannot be run or resumed here (job {})",
            row.kind, row.id
        ))
    }
    fn summary(&self, row: &JobRow) -> JobSummary {
        let mut s = base_summary(row, self);
        s.kind_label = row.kind.clone();
        summary(s, row)
    }
}
