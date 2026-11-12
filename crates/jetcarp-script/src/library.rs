//! Cataloging a world's objects into the pattern library (`jetcarp_store::library`), shared by the
//! Rhai `library()` handle and the app (sweep favourites). The work is `jetcarp_api::catalog`'s,
//! which the operation `world.objects` uses too.

use jetcarp_core::pattern::PatternRecord;
use serde_json::{Value, json};

pub use jetcarp_api::catalog::{CatalogReport, catalog_world};

/// A record as JSON for scripts (the stored fields).
pub fn record_json(r: &PatternRecord) -> Value {
    let mut v = serde_json::to_value(r).unwrap_or(Value::Null);
    v["label"] = json!(r.label());
    v
}
