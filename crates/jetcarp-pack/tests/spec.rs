//! The written spec (`docs/src/reference/pack-format.md`) against the code: every table's
//! columns, types and nullability as the page lists them, and the frozen palette registry.

use jetcarp_pack::columns::{Ty, type_name};
use jetcarp_pack::tables;

fn page() -> String {
    let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../docs/src/reference/pack-format.md");
    std::fs::read_to_string(p).unwrap()
}

/// The `name: type` cells of the page's tables, as one text.
fn columns_on_page(page: &str) -> String {
    page.lines()
        .filter(|l| l.starts_with('|'))
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn every_column_is_on_the_page() {
    let page = page();
    let cols = columns_on_page(&page);
    let mut missing = vec![];
    for t in tables::all().iter().chain([tables::reference_patch()]) {
        assert!(
            page.contains(t.schema)
                || t.name == "reference"
                || page.contains(&format!("`{}`", t.name)),
            "{} not on the page",
            t.name
        );
        for c in &t.cols {
            // The site struct is written out in its own block.
            if matches!(c.ty, Ty::Struct(_)) {
                assert!(
                    page.contains(&format!("{}: struct", c.name)),
                    "{}.{}",
                    t.name,
                    c.name
                );
                continue;
            }
            let ty = type_name(&c.ty);
            let want = format!("{}: {}{}", c.name, ty, if c.nullable { "?" } else { "" });
            if !cols.contains(&want) && !page.contains(&format!("`{want}`")) {
                missing.push(format!("{}.{want}", t.name));
            }
        }
    }
    // Tables given in prose rather than as a table.
    missing.retain(|m| {
        !(m.starts_with("score_models.") || m.starts_with("aliases."))
            || !page.contains(m.split_once('.').unwrap().1)
    });
    assert!(
        missing.is_empty(),
        "columns the page does not list: {missing:?}"
    );
    for t in tables::all() {
        assert!(
            page.contains(t.schema) || page.contains(&format!("`{}`", t.name)),
            "{}",
            t.name
        );
    }
}

/// The page's table of schema ids lists exactly the ids the code reads and writes.
#[test]
fn the_schema_ids_table_is_every_schema_id() {
    let page = page();
    let section = page
        .split("### Schema ids")
        .nth(1)
        .and_then(|s| s.split("\n### ").next())
        .expect("the schema ids on the page");
    let listed: Vec<String> = section
        .lines()
        .filter_map(|l| l.strip_prefix("| `"))
        .filter_map(|l| l.split_once('`'))
        .map(|(id, _)| id.to_string())
        .collect();
    let mut want: Vec<String> = tables::all()
        .iter()
        .chain([tables::reference_patch()])
        .map(|t| t.schema.to_string())
        .collect();
    want.extend(
        [
            "jetcarp-search 1",
            "jetcarp-explore-runs 1",
            "jetcarp-explore-runs 2",
            "jetcarp-explore-scores 1",
        ]
        .map(String::from),
    );
    let (mut a, mut b) = (listed.clone(), want.clone());
    a.sort();
    b.sort();
    assert_eq!(a, b);
}

#[test]
fn the_palette_registry_is_every_built_in_palette() {
    let page = page();
    let block = page
        .split("<!-- palette-registry -->")
        .nth(1)
        .and_then(|b| b.split("<!-- /palette-registry -->").next())
        .expect("the registry on the page");
    let listed: Vec<String> = block
        .split('`')
        .skip(1)
        .step_by(2)
        .map(String::from)
        .collect();
    let built: Vec<String> = jetcarp_core::render::palette_names()
        .map(String::from)
        .collect();
    // Every name on the page is a palette for good, and every built-in palette is on the page.
    for n in &listed {
        assert!(
            built.contains(n),
            "the registry names {n}, which is not built in"
        );
    }
    for n in &built {
        assert!(
            listed.contains(n),
            "palette {n} is not in the page's registry"
        );
    }
}
