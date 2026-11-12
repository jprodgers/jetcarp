//! Embeds the MCP resources: the book's pages (`resources/docs/**/*.md`, copies of `docs/src`
//! that `docs/tools/copy-mcp-resources.py` refreshes) and the JSON Schemas
//! (`resources/schema/*.json`). The files live inside the crate, so `cargo package` takes them.

use std::path::{Path, PathBuf};

fn walk(dir: &Path, ext: &str, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    for e in rd.flatten() {
        let p = e.path();
        if p.is_dir() {
            walk(&p, ext, out);
        } else if p.extension().is_some_and(|x| x == ext) {
            out.push(p);
        }
    }
}

fn table(name: &str, root: &Path, ext: &str) -> String {
    let mut files = vec![];
    walk(root, ext, &mut files);
    let mut rows: Vec<(String, String)> = files
        .iter()
        .map(|p| {
            let rel = p
                .strip_prefix(root)
                .unwrap()
                .components()
                .map(|c| c.as_os_str().to_string_lossy().to_string())
                .collect::<Vec<_>>()
                .join("/");
            (rel, p.display().to_string())
        })
        .collect();
    rows.sort();
    let mut s = format!("pub static {name}: &[(&str, &str)] = &[\n");
    for (rel, abs) in rows {
        s.push_str(&format!("    ({rel:?}, include_str!({abs:?})),\n"));
    }
    s.push_str("];\n");
    s
}

fn main() {
    let here = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let res = here.join("resources");
    println!("cargo:rerun-if-changed=resources");
    let mut code = table("DOCS", &res.join("docs"), "md");
    code.push_str(&table("SCHEMAS", &res.join("schema"), "json"));
    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("resources.rs");
    std::fs::write(out, code).unwrap();
}
