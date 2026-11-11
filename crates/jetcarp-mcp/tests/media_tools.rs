//! MCP media tools: render a pattern's media (with the poster inline) and export a site. One
//! test in this binary, since it points JETCARP_LIBRARY at a temp folder.

use jetcarp_mcp::{Server, Session};
use serde_json::{Value, json};

fn call(server: &mut Server, host: &mut Session, name: &str, args: Value) -> Value {
    let r = server
        .handle(
            host,
            &json!({"jsonrpc": "2.0", "id": 7, "method": "tools/call",
                    "params": {"name": name, "arguments": args}}),
        )
        .expect("reply");
    let res = r["result"].clone();
    assert_eq!(res["isError"], false, "{name} failed: {res}");
    res
}

fn text(res: &Value) -> Value {
    let t: String = res["content"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|c| c["text"].as_str())
        .collect();
    serde_json::from_str(&t).unwrap_or(Value::String(t))
}

#[test]
fn media_render_and_site_export_over_mcp() {
    let dir = std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("jetcarp-mcp-media-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    // SAFETY: the only test in this binary, before any thread reads the environment.
    unsafe { std::env::set_var("JETCARP_LIBRARY", &dir) };
    let (mut s, mut h) = (Server::new(), Session::new());
    // A glider in the catalog, from a script.
    call(
        &mut s,
        &mut h,
        "run_script",
        json!({"script": "let w = world(#{ tiling: \"square\", size: [32, 32], boundary: \"wrap\", cas: [#{ rule: \"B3/S23\", seed: \"empty\" }] });\nw.set_cells([[11, 10, 1], [12, 11, 1], [10, 12, 1], [11, 12, 1], [12, 12, 1]]);\nlibrary().catalog(w);"}),
    );
    let list = text(&call(
        &mut s,
        &mut h,
        "library_list",
        json!({"class": "spaceship"}),
    ));
    let id = list.to_string();
    let id = id
        .split("\"id\":")
        .nth(1)
        .and_then(|x| x.split('"').nth(1))
        .expect("a ship id")
        .to_string();
    let res = call(
        &mut s,
        &mut h,
        "media_render",
        json!({"pattern": id, "poster": true}),
    );
    let r = text(&res);
    assert_eq!(r["written"], 1, "{r}");
    let anim = r["done"][0]["files"]["anim"].as_str().unwrap();
    // Content-addressed: media/<two hex digits>/<sha256>.gif beside the library.
    let p = std::path::Path::new(anim);
    assert!(anim.ends_with(".gif") && p.exists(), "{anim}");
    assert_eq!(
        p.file_stem().unwrap().to_string_lossy(),
        jetcarp_core::media::content_hash(&std::fs::read(p).unwrap())
    );
    let img = res["content"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["type"] == "image")
        .expect("poster inline");
    assert_eq!(img["mimeType"], "image/png");
    // Again: up to date.
    let r2 = text(&call(
        &mut s,
        &mut h,
        "media_render",
        json!({"pattern": id}),
    ));
    assert_eq!(
        (r2["written"].as_u64(), r2["skipped"].as_u64()),
        (Some(0), Some(1))
    );
    // Promote it and export.
    call(
        &mut s,
        &mut h,
        "library_update",
        json!({"id": id, "state": "promoted"}),
    );
    let site = dir.join("site");
    let e = text(&call(
        &mut s,
        &mut h,
        "site_export",
        json!({"to": site.display().to_string(), "quickstart": "none"}),
    ));
    assert_eq!(e["patterns"], 1, "{e}");
    assert!(site.join("manifest.json").exists());
    assert!(site.join(format!("patterns/{id}.json")).exists());
}
