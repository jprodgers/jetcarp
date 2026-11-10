//! Helpers shared by the operation tests.

#![allow(dead_code)]

use jetcarp_api::ApiError;
use jetcarp_api::ErrorKind;
use std::path::PathBuf;

/// A fresh folder for one test, under the output folder (`JETCARP_OUT` when the tests run with
/// their own folders), so files.list can see it.
pub fn scratch(name: &str) -> PathBuf {
    let dir = jetcarp_core::paths::output_dir()
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
