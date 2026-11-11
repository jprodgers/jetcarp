//! Helpers shared by the operation tests.

#![allow(dead_code)]

use jetcarp_api::ApiError;
use jetcarp_api::ErrorKind;
use std::path::PathBuf;

/// A fresh folder for one test, in Cargo's scratch folder for this target
/// (`target/tmp/api-tests`), never in the user's own folders. A test that needs it inside the
/// output root (files.list) passes it to `Ctx::with_out_root`.
pub fn scratch(name: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join("api-tests")
        .join(format!("{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    jetcarp_api::out::absolute(&dir)
}

/// The error of a failed call, with its kind checked.
pub fn fails<T: std::fmt::Debug>(r: Result<T, ApiError>, kind: ErrorKind) -> ApiError {
    let e = r.expect_err("the call should fail");
    assert_eq!(e.kind, kind, "{e}");
    e
}

/// Keys of a JSON object, sorted.
pub fn keys(v: &serde_json::Value) -> Vec<String> {
    v.as_object().expect("an object").keys().cloned().collect()
}

/// A catalog in `dir` holding Life's glider and blinker (found by the object finder, as a
/// script's `lib.catalog(w)` stores them) and Langton's ant's highway, and a context on it.
pub fn catalog(dir: &std::path::Path) -> (jetcarp_core::library::Library, jetcarp_api::Ctx) {
    use jetcarp_core::library::Library;
    use jetcarp_core::objects::{ObjClass, ObjectOptions, find_objects, place_rows};
    use jetcarp_core::pattern::{DiscoveredBy, Provenance, record_from_object};
    use jetcarp_core::{Boundary, CaSpec, Command, SeedSpec, TilingSpec, World};
    let mut w = World::new(TilingSpec::rect("square", 40, 40, Boundary::Wrap), 1).unwrap();
    let g = w.topo.lattice().unwrap().clone();
    let mut cells = place_rows(&g, ".o./..o/ooo", 10, 10);
    cells.extend(place_rows(&g, "ooo", 25, 25));
    w.apply(Command::AddCa {
        ca: CaSpec::new("B3/S23").seed(SeedSpec::Cells { cells }),
    })
    .unwrap();
    let lib = Library::open(dir.join("catalog"));
    let found = find_objects(&w, &ObjectOptions::default()).unwrap();
    for o in found
        .objects
        .iter()
        .filter(|o| !matches!(o.class, ObjClass::Other { .. }))
    {
        let prov = Provenance {
            source: "api test".into(),
            jetcarp_version: jetcarp_core::VERSION.into(),
            date: jetcarp_core::pattern::now_utc(),
            ..Default::default()
        };
        let rec = record_from_object(&w, w.cas[0].id, o, prov, DiscoveredBy::User).unwrap();
        lib.add(rec).unwrap();
    }
    // Langton's ant: a highway.
    let m = jetcarp_core::ants::search::parse_rule("RL").unwrap();
    let t = jetcarp_core::pattern::tiling_list("square").unwrap();
    let lim = jetcarp_core::ants::run::Limits {
        steps: 20_000,
        max_period: 2000,
        symmetry: true,
    };
    let row =
        jetcarp_core::ants::job::run_one(&m, &t[0], &jetcarp_core::ants::run::Start::Empty, &lim)
            .unwrap();
    lib.ants_flush(0, std::slice::from_ref(&row), None, None)
        .unwrap();
    let cx = jetcarp_api::Ctx::new().with_library(Some(lib.path().to_path_buf()));
    (lib, cx)
}

/// The id of the one ant run in [`catalog`].
pub fn ant_id(lib: &jetcarp_core::library::Library) -> String {
    lib.ants(&Default::default()).unwrap()[0].id.clone()
}
