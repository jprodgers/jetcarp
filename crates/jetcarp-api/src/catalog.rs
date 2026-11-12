//! Cataloguing a world's objects into the pattern library: each distinct still life, oscillator
//! and spaceship a world holds now is verified and stored (copies add to its seen count). Shared
//! by `world.objects`, the Rhai `library()` handle, and the app (sweep favourites).

use jetcarp_core::World;
use jetcarp_core::objects::{ObjClass, ObjectOptions, find_objects};
use jetcarp_core::pattern::split::records_from_object;
use jetcarp_core::pattern::{DiscoveredBy, PatternRecord, Provenance, now_utc};
use jetcarp_store::{AddOutcome, Library};
use serde_json::{Value, json};
use std::collections::BTreeMap;

/// What [`catalog_world`] did.
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, schemars::JsonSchema)]
pub struct CatalogReport {
    /// Objects found (every copy).
    pub objects: usize,
    /// Distinct patterns stored for the first time.
    pub added: Vec<String>,
    /// Distinct patterns that were already in the library (their seen count grew).
    pub merged: Vec<String>,
    /// Objects that are not still lifes, oscillators, or spaceships (or could not be recorded).
    pub skipped: usize,
    /// Patterns that failed verification, with the reason.
    pub failed: Vec<(String, String)>,
}

impl CatalogReport {
    pub fn to_json(&self) -> Value {
        json!({
            "objects": self.objects,
            "added": self.added,
            "merged": self.merged,
            "ids": self.added.iter().chain(&self.merged).collect::<Vec<_>>(),
            "skipped": self.skipped,
            "failed": self.failed.iter().map(|(id, e)| json!({"id": id, "error": e})).collect::<Vec<_>>(),
        })
    }
}

/// Find the objects of CA `ca` (default: the first CA the detector supports) in `world`, verify
/// each distinct still life, oscillator, and spaceship, and add it to `lib` (copies add to its
/// seen count). `source` goes into the provenance.
pub fn catalog_world(
    lib: &Library,
    world: &World,
    ca: Option<u32>,
    max_period: Option<u64>,
    source: &str,
    by: DiscoveredBy,
) -> Result<CatalogReport, String> {
    let d = ObjectOptions::default();
    let opts = ObjectOptions {
        ca,
        max_period: max_period.unwrap_or(d.max_period),
        ..d
    };
    let rep = find_objects(world, &opts)?;
    let ca_id = rep.ca_id;
    let tiling = world.topo.spec.clone();
    let prov = Provenance {
        source: source.to_string(),
        tiling: Some(tiling),
        rng_state: Some(world.to_scene().rng_state),
        generation: Some(world.generation),
        jetcarp_version: jetcarp_core::VERSION.into(),
        date: now_utc(),
        ..Default::default()
    };
    let mut out = CatalogReport {
        objects: rep.objects.len(),
        ..Default::default()
    };
    let mut distinct: BTreeMap<String, PatternRecord> = BTreeMap::new();
    for o in &rep.objects {
        if matches!(o.class, ObjClass::Other { .. }) {
            out.skipped += 1;
            continue;
        }
        match records_from_object(world, ca_id, o, prov.clone(), by) {
            Ok(parts) => {
                for r in parts {
                    match distinct.get_mut(&r.id) {
                        Some(x) => x.seen += 1,
                        None => {
                            distinct.insert(r.id.clone(), r);
                        }
                    }
                }
            }
            Err(_) => out.skipped += 1,
        }
    }
    let ids: Vec<String> = distinct.keys().cloned().collect();
    let res = lib.add_many(distinct.into_values().collect(), None)?;
    for (id, r) in ids.into_iter().zip(res) {
        match r {
            Ok(AddOutcome::Added) => out.added.push(id),
            Ok(AddOutcome::Merged { .. }) => out.merged.push(id),
            Err(e) => out.failed.push((id, e)),
        }
    }
    Ok(out)
}
