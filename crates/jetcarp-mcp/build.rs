//! Embeds the MCP resources: the book's pages and the JSON Schemas.
//!
//! In the repository the book itself is the source (`docs/src/**/*.md` and
//! `docs/src/reference/*.schema.json`), so editing a page never leaves the server behind. A
//! packaged crate (crates.io) has no book beside it and embeds its copies instead
//! (`resources/docs`, `resources/schema`), which `docs/tools/copy-mcp-resources.py` refreshes and
//! the release workflow checks before publishing.

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

fn table(name: &str, root: &Path, ext: &str, rename: fn(String) -> String) -> String {
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
            (rename(rel), p.display().to_string())
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
    let book = here.join("../../docs/src");
    let code = if book.join("SUMMARY.md").is_file() {
        println!("cargo:rerun-if-changed={}", book.display());
        let mut code = table("DOCS", &book, "md", |r| r);
        // `reference/scene.schema.json` is the resource `scene.json`.
        code.push_str(&table("SCHEMAS", &book.join("reference"), "json", |r| {
            r.replace(".schema.json", ".json")
        }));
        code
    } else {
        let res = here.join("resources");
        println!("cargo:rerun-if-changed=resources");
        let mut code = table("DOCS", &res.join("docs"), "md", |r| r);
        code.push_str(&table("SCHEMAS", &res.join("schema"), "json", |r| r));
        code
    };
    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("resources.rs");
    std::fs::write(out, code).unwrap();
}
