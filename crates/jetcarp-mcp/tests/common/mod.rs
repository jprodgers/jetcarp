//! Helpers shared by the MCP tests.
//!
//! Every test binary points JETCARP's folders (output, library, data, settings, runtime) at a
//! scratch folder of its own under Cargo's `CARGO_TARGET_TMPDIR` before the first tool call, so no
//! test reads or writes the user's own folders, or finds a running app or host of theirs.

#![allow(dead_code)]

use jetcarp_mcp::{Host, Server};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// This test binary's scratch folder (made once, emptied first), with JETCARP's folders in it.
pub fn setup(name: &str) -> &'static Path {
    static ROOT: OnceLock<PathBuf> = OnceLock::new();
    ROOT.get_or_init(|| {
        let root = jetcarp_api::out::absolute(
            &PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
                .join("mcp-tests")
                .join(format!("{name}-{}", std::process::id())),
        );
        let _ = std::fs::remove_dir_all(&root);
        for d in ["out", "lib", "data", "config", "runtime"] {
            std::fs::create_dir_all(root.join(d)).unwrap();
        }
        // SAFETY: set once, before this binary's first tool call reads them (every test calls
        // `setup` first, and the `OnceLock` makes the others wait).
        unsafe {
            std::env::set_var("JETCARP_OUT", root.join("out"));
            std::env::set_var("JETCARP_LIBRARY", root.join("lib"));
            std::env::set_var("JETCARP_DATA_DIR", root.join("data"));
            std::env::set_var("JETCARP_CONFIG_DIR", root.join("config"));
            std::env::set_var("JETCARP_RUNTIME_DIR", root.join("runtime"));
        }
        root
    })
}

/// The environment a server process of the tests gets (the same folders).
pub fn env_of(root: &Path) -> Vec<(&'static str, PathBuf)> {
    vec![
        ("JETCARP_OUT", root.join("out")),
        ("JETCARP_LIBRARY", root.join("lib")),
        ("JETCARP_DATA_DIR", root.join("data")),
        ("JETCARP_CONFIG_DIR", root.join("config")),
        ("JETCARP_RUNTIME_DIR", root.join("runtime")),
    ]
}

pub fn req(id: i64, method: &str, params: Value) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params})
}

/// A tool call's result object.
pub fn call(server: &mut Server, host: &mut dyn Host, name: &str, args: Value) -> Value {
    let r = server
        .handle(
            host,
            &req(7, "tools/call", json!({"name": name, "arguments": args})),
        )
        .expect("reply");
    assert_eq!(r["id"], 7, "{r}");
    assert!(r.get("error").is_none(), "{name}: {r}");
    r["result"].clone()
}

/// A result's text blocks, joined.
pub fn text(result: &Value) -> String {
    result["content"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|c| c["text"].as_str())
                .collect::<Vec<_>>()
                .join("\n")
        })
        .unwrap_or_default()
}

/// A successful call's text.
pub fn ok(result: &Value) -> String {
    assert_eq!(result["isError"], false, "tool failed: {}", text(result));
    text(result)
}

/// A failed call's text.
pub fn fails(result: &Value) -> String {
    assert_eq!(
        result["isError"],
        true,
        "the call should fail: {}",
        text(result)
    );
    text(result)
}

/// A successful call's first text block as JSON.
pub fn json_of(result: &Value) -> Value {
    ok(result);
    let t = result["content"][0]["text"].as_str().unwrap();
    serde_json::from_str(t).unwrap_or_else(|e| panic!("not JSON ({e}): {t}"))
}

/// A successful call's last text block as JSON (the facts after a summary).
pub fn last_json(result: &Value) -> Value {
    ok(result);
    let t = result["content"]
        .as_array()
        .unwrap()
        .iter()
        .rev()
        .find_map(|c| c["text"].as_str())
        .unwrap();
    serde_json::from_str(t).unwrap_or_else(|e| panic!("not JSON ({e}): {t}"))
}

/// Whether a result carries a PNG image.
pub fn has_png(result: &Value) -> bool {
    result["content"].as_array().is_some_and(|a| {
        a.iter()
            .any(|c| c["type"] == "image" && c["mimeType"] == "image/png")
    })
}

/// A path as JSON text, escaped (Windows paths have backslashes).
pub fn path_json(p: &Path) -> Value {
    json!(p.display().to_string())
}
