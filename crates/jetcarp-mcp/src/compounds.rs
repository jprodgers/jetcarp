//! `library_curate {"compounds": {...}}`: the library pass that finds stored compounds (several
//! independent objects stored as one pattern) and, with `apply`, stores their parts instead.

use crate::tools::{Call, Res};
use serde_json::Value;

/// The `compounds` argument's schema, when this build has the pass.
pub(crate) fn schema() -> Option<Value> {
    None
}

pub(crate) fn run(_c: &mut Call, _v: &Value) -> Res {
    Err("library_curate: this build has no compounds pass".into())
}
