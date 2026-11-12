//! The resources the server embeds: the book's pages must be exact copies of `docs/src` (run
//! `python3 docs/tools/copy-mcp-resources.py` after editing the book), and the JSON Schemas must
//! accept every scene that ships (`crates/jetcarp-core/data/scenes`, `docs/scenes`) and every
//! golden pack's manifest, and refuse what is not one.
//!
//! The schemas are checked with a small draft-07 validator below (the keywords they use), so the
//! tests need no JSON Schema crate.

use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn files_under(dir: &Path, ext: &str) -> Vec<PathBuf> {
    let mut out = vec![];
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d).unwrap().flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else if p.extension().is_some_and(|x| x == ext) {
                out.push(p);
            }
        }
    }
    out.sort();
    out
}

fn rel(p: &Path, root: &Path) -> String {
    p.strip_prefix(root)
        .unwrap()
        .components()
        .map(|c| c.as_os_str().to_string_lossy().to_string())
        .collect::<Vec<_>>()
        .join("/")
}

#[test]
fn resources_match_the_book() {
    let src = repo().join("docs/src");
    if !src.is_dir() {
        // A packaged crate has no book beside it; the copies are what it ships.
        return;
    }
    let book: BTreeMap<String, Vec<u8>> = files_under(&src, "md")
        .iter()
        .map(|p| (rel(p, &src), std::fs::read(p).unwrap()))
        .collect();
    let embedded: BTreeMap<String, Vec<u8>> = jetcarp_mcp::docs::files()
        .iter()
        .map(|(p, t)| (p.to_string(), t.as_bytes().to_vec()))
        .collect();
    let stale: Vec<&String> = book
        .iter()
        .filter(|(p, b)| embedded.get(*p) != Some(*b))
        .map(|(p, _)| p)
        .collect();
    let extra: Vec<&String> = embedded.keys().filter(|p| !book.contains_key(*p)).collect();
    assert!(
        stale.is_empty() && extra.is_empty(),
        "the MCP server's copies of the book are out of date (run python3 docs/tools/copy-mcp-resources.py): differ or missing {stale:?}, no longer in the book {extra:?}"
    );
    for (name, text) in jetcarp_mcp::docs::schemas() {
        let book = src
            .join("reference")
            .join(name.replace(".json", ".schema.json"));
        assert_eq!(
            std::fs::read_to_string(&book).unwrap(),
            *text,
            "{name} differs from {}: run docs/tools/copy-mcp-resources.py",
            book.display()
        );
    }
    assert_eq!(jetcarp_mcp::docs::schemas().len(), 2);
    // The pages come in the book's order, each with its title.
    let pages = jetcarp_mcp::docs::pages();
    assert_eq!(pages[0].path, "introduction");
    assert!(pages.iter().all(|p| !p.title.is_empty()));
    assert!(pages.iter().any(|p| p.path == "reference/mcp"));
    assert_eq!(pages.len(), book.len() - 1, "every page but SUMMARY.md");
}

#[test]
fn the_crate_packages_small() {
    let res = Path::new(env!("CARGO_MANIFEST_DIR")).join("resources");
    let bytes: u64 = files_under(&res, "md")
        .iter()
        .chain(files_under(&res, "json").iter())
        .map(|p| p.metadata().unwrap().len())
        .sum();
    assert!(bytes < 8 << 20, "resources are {bytes} bytes");
}

// ---------------------------------------------------------------------------------------------
// A small JSON Schema (draft 07) validator: type, enum, const, properties, required,
// additionalProperties, items, minItems, maxItems, minProperties, maxProperties, minimum,
// maximum, pattern, oneOf, anyOf, and local $refs.

fn type_ok(t: &str, v: &Value) -> bool {
    match t {
        "object" => v.is_object(),
        "array" => v.is_array(),
        "string" => v.is_string(),
        "boolean" => v.is_boolean(),
        "null" => v.is_null(),
        "number" => v.is_number(),
        "integer" => v.as_i64().is_some() || v.as_u64().is_some(),
        _ => false,
    }
}

/// A tiny regular-expression matcher for the patterns the schemas use: anchors, literals,
/// escapes, `[...]` classes with ranges, and `{n}`, `{n,m}`, `+`, `*`, `?`.
fn regex_match(pat: &str, s: &str) -> bool {
    #[derive(Debug)]
    enum Atom {
        Any(Vec<(char, char)>),
        Lit(char),
    }
    let p: Vec<char> = pat.chars().collect();
    let (mut i, mut atoms) = (0, vec![]);
    let (anchored_start, mut anchored_end) = (p.first() == Some(&'^'), false);
    if anchored_start {
        i = 1;
    }
    while i < p.len() {
        let atom = match p[i] {
            '$' if i == p.len() - 1 => {
                anchored_end = true;
                i += 1;
                continue;
            }
            '[' => {
                let mut ranges = vec![];
                i += 1;
                while p[i] != ']' {
                    let a = if p[i] == '\\' {
                        i += 1;
                        p[i]
                    } else {
                        p[i]
                    };
                    if p[i + 1] == '-' && p[i + 2] != ']' {
                        ranges.push((a, p[i + 2]));
                        i += 3;
                    } else {
                        ranges.push((a, a));
                        i += 1;
                    }
                }
                Atom::Any(ranges)
            }
            '\\' => {
                i += 1;
                Atom::Lit(p[i])
            }
            c => Atom::Lit(c),
        };
        i += 1;
        let (lo, hi) = match p.get(i) {
            Some('+') => {
                i += 1;
                (1, usize::MAX)
            }
            Some('*') => {
                i += 1;
                (0, usize::MAX)
            }
            Some('?') => {
                i += 1;
                (0, 1)
            }
            Some('{') => {
                let close = i + p[i..].iter().position(|c| *c == '}').unwrap();
                let body: String = p[i + 1..close].iter().collect();
                i = close + 1;
                match body.split_once(',') {
                    Some((a, b)) => (a.parse().unwrap(), b.parse().unwrap()),
                    None => (body.parse().unwrap(), body.parse().unwrap()),
                }
            }
            _ => (1, 1),
        };
        atoms.push((atom, lo, hi));
    }
    fn matches(a: &Atom, c: char) -> bool {
        match a {
            Atom::Lit(x) => *x == c,
            Atom::Any(r) => r.iter().any(|(lo, hi)| (*lo..=*hi).contains(&c)),
        }
    }
    fn go(atoms: &[(Atom, usize, usize)], s: &[char], end: bool) -> bool {
        let Some(((a, lo, hi), rest)) = atoms.split_first() else {
            return !end || s.is_empty();
        };
        let mut n = 0;
        while n < *hi && n < s.len() && matches(a, s[n]) {
            n += 1;
        }
        (*lo..=n).rev().any(|k| go(rest, &s[k..], end))
    }
    let s: Vec<char> = s.chars().collect();
    if anchored_start {
        go(&atoms, &s, anchored_end)
    } else {
        (0..=s.len()).any(|k| go(&atoms, &s[k..], anchored_end))
    }
}

fn resolve<'a>(root: &'a Value, r: &str) -> &'a Value {
    let mut v = root;
    for part in r.trim_start_matches("#/").split('/') {
        v = &v[part];
    }
    assert!(!v.is_null(), "no {r}");
    v
}

/// The errors of `v` against `s` (empty: valid).
fn validate(root: &Value, s: &Value, v: &Value, at: &str, errs: &mut Vec<String>) {
    if let Some(r) = s.get("$ref").and_then(Value::as_str) {
        return validate(root, resolve(root, r), v, at, errs);
    }
    if let Some(t) = s.get("type") {
        let ok = match t {
            Value::String(t) => type_ok(t, v),
            Value::Array(ts) => ts.iter().any(|t| type_ok(t.as_str().unwrap(), v)),
            _ => true,
        };
        if !ok {
            errs.push(format!("{at}: {v} is not {t}"));
            return;
        }
    }
    if let Some(e) = s.get("enum").and_then(Value::as_array)
        && !e.contains(v)
    {
        errs.push(format!("{at}: {v} is not one of {e:?}"));
    }
    if let Some(c) = s.get("const")
        && c != v
    {
        errs.push(format!("{at}: {v} is not {c}"));
    }
    if let (Some(n), Some(x)) = (s.get("minimum").and_then(Value::as_f64), v.as_f64())
        && x < n
    {
        errs.push(format!("{at}: {x} < {n}"));
    }
    if let (Some(n), Some(x)) = (s.get("maximum").and_then(Value::as_f64), v.as_f64())
        && x > n
    {
        errs.push(format!("{at}: {x} > {n}"));
    }
    if let (Some(p), Some(x)) = (s.get("pattern").and_then(Value::as_str), v.as_str())
        && !regex_match(p, x)
    {
        errs.push(format!("{at}: {x:?} does not match {p}"));
    }
    if let Some(o) = v.as_object() {
        let props = s.get("properties").and_then(Value::as_object);
        if let Some(req) = s.get("required").and_then(Value::as_array) {
            for k in req {
                if !o.contains_key(k.as_str().unwrap()) {
                    errs.push(format!("{at}: missing {k}"));
                }
            }
        }
        for (k, x) in o {
            match props.and_then(|p| p.get(k)) {
                Some(ps) => validate(root, ps, x, &format!("{at}.{k}"), errs),
                None => match s.get("additionalProperties") {
                    Some(Value::Bool(false)) => errs.push(format!("{at}: unexpected {k}")),
                    Some(a @ Value::Object(_)) => validate(root, a, x, &format!("{at}.{k}"), errs),
                    _ => {}
                },
            }
        }
        if let Some(n) = s.get("minProperties").and_then(Value::as_u64)
            && (o.len() as u64) < n
        {
            errs.push(format!("{at}: fewer than {n} members"));
        }
        if let Some(n) = s.get("maxProperties").and_then(Value::as_u64)
            && (o.len() as u64) > n
        {
            errs.push(format!("{at}: more than {n} members"));
        }
    }
    if let Some(a) = v.as_array() {
        if let Some(items) = s.get("items") {
            for (i, x) in a.iter().enumerate() {
                validate(root, items, x, &format!("{at}[{i}]"), errs);
            }
        }
        if let Some(n) = s.get("minItems").and_then(Value::as_u64)
            && (a.len() as u64) < n
        {
            errs.push(format!("{at}: fewer than {n} items"));
        }
        if let Some(n) = s.get("maxItems").and_then(Value::as_u64)
            && (a.len() as u64) > n
        {
            errs.push(format!("{at}: more than {n} items"));
        }
    }
    for (kw, exactly_one) in [("oneOf", true), ("anyOf", false)] {
        if let Some(alts) = s.get(kw).and_then(Value::as_array) {
            let passing = alts
                .iter()
                .filter(|alt| {
                    let mut e = vec![];
                    validate(root, alt, v, at, &mut e);
                    e.is_empty()
                })
                .count();
            if (exactly_one && passing != 1) || passing == 0 {
                errs.push(format!(
                    "{at}: {passing} of the {kw} alternatives match {v}"
                ));
            }
        }
    }
}

fn errors(schema: &Value, v: &Value) -> Vec<String> {
    let mut e = vec![];
    validate(schema, schema, v, "$", &mut e);
    e
}

fn schema(name: &str) -> Value {
    serde_json::from_str(jetcarp_mcp::docs::schema(name).expect("embedded")).unwrap()
}

#[test]
fn the_validator_itself() {
    assert!(regex_match(
        "^sha256:[0-9a-f]{64}$",
        &format!("sha256:{}", "ab".repeat(32))
    ));
    assert!(!regex_match("^sha256:[0-9a-f]{64}$", "sha256:abc"));
    assert!(regex_match("^[A-Za-z0-9-]{1,64}$", "jetcarp-starter"));
    assert!(!regex_match("^[A-Za-z0-9-]{1,64}$", "no spaces"));
    assert!(regex_match("^1\\.[0-9]+$", "1.0"));
    assert!(!regex_match("^1\\.[0-9]+$", "2.0"));
    let s = json!({"type": "object", "required": ["a"], "properties": {"a": {"oneOf": [{"type": "integer"}, {"type": "string"}]}}});
    assert!(errors(&s, &json!({"a": 1})).is_empty());
    assert!(!errors(&s, &json!({"a": true})).is_empty());
    assert!(!errors(&s, &json!({})).is_empty());
}

#[test]
fn every_shipped_scene_fits_the_scene_schema() {
    let s = schema("scene.json");
    let mut files = files_under(&repo().join("crates/jetcarp-core/data/scenes"), "json");
    let docs = repo().join("docs/scenes");
    if docs.is_dir() {
        files.extend(files_under(&docs, "json"));
    }
    assert!(files.len() > 100, "{} scenes", files.len());
    let mut n = 0;
    for f in &files {
        let v: Value = serde_json::from_str(&std::fs::read_to_string(f).unwrap()).unwrap();
        // A preset list (`presets/*.json`) holds scenes under `scene`.
        let scenes: Vec<&Value> = match &v {
            Value::Array(items) => items.iter().map(|i| &i["scene"]).collect(),
            v => vec![v],
        };
        for sc in scenes {
            let e = errors(&s, sc);
            assert!(e.is_empty(), "{}: {e:?}", f.display());
            n += 1;
        }
    }
    assert!(n > 200, "{n} scenes checked");
    // A saved scene (every default written, a command log, a record) fits too.
    let mut w = jetcarp_core::World::new(
        jetcarp_core::TilingSpec::rect("hex", 20, 20, jetcarp_core::Boundary::Wrap),
        7,
    )
    .unwrap();
    for cmd in [
        json!({"op": "add_ca", "ca": {"rule": "B2/S/C3", "seed": {"random": {"density": 0.3}}}}),
        json!({"op": "step", "gens": 3}),
        json!({"op": "reseed", "mode": {"kind": "rain"}}),
        json!({"op": "set_cells", "id": 1, "cells": [[3, 1]]}),
    ] {
        w.apply(serde_json::from_value(cmd).unwrap()).unwrap();
    }
    let mut sc = w.to_scene();
    sc.record = Some(w.record());
    let v: Value = serde_json::from_str(&sc.to_json()).unwrap();
    assert!(errors(&s, &v).is_empty(), "{:?}", errors(&s, &v));
    // And what is not a scene is refused.
    for bad in [
        json!({"cas": []}),
        json!({"tiling": {"size": {"rect": {"w": 3, "h": 3}}}}),
        json!({"tiling": {"family": "square", "boundary": "torus"}}),
        json!({"tiling": {"family": "square"}, "cas": [{"seed": "empty"}]}),
        json!({"tiling": {"family": "square"}, "cas": [{"rule": "Life", "seed": "random:0.2"}]}),
        json!({"tiling": {"family": "square"}, "commands": [{"op": "fly"}]}),
        json!({"tiling": {"family": "square"}, "record": {"generation": 1, "hash": "xyz"}}),
    ] {
        assert!(!errors(&s, &bad).is_empty(), "accepted {bad}");
    }
}

#[test]
fn every_golden_manifest_fits_the_pack_schema() {
    let s = schema("pack.json");
    let golden = repo().join("crates/jetcarp-pack/tests/golden");
    let mut manifests = vec![];
    for dir in std::fs::read_dir(&golden).unwrap().flatten() {
        let d = dir.path();
        if d.join("pack.json").is_file() {
            manifests.push(d.join("pack.json"));
        }
        // A store: every manifest its feeds' refs reach, parents included.
        let refs = d.join("refs");
        if !refs.is_dir() {
            continue;
        }
        let mut todo: Vec<String> = files_under(&refs, "json")
            .iter()
            .map(|r| {
                let v: Value = serde_json::from_slice(&std::fs::read(r).unwrap()).unwrap();
                v["manifest"].as_str().unwrap().to_string()
            })
            .collect();
        let mut seen = std::collections::HashSet::new();
        while let Some(h) = todo.pop() {
            if !seen.insert(h.clone()) {
                continue;
            }
            let hex = h.trim_start_matches("sha256:");
            let p = d
                .join("objects/sha256")
                .join(&hex[..2])
                .join(format!("{hex}.json"));
            let v: Value = serde_json::from_slice(&std::fs::read(&p).unwrap()).unwrap();
            for parent in v["parents"].as_array().into_iter().flatten() {
                todo.push(parent.as_str().unwrap().to_string());
            }
            manifests.push(p);
        }
    }
    assert!(manifests.len() >= 8, "{} manifests", manifests.len());
    for m in &manifests {
        let v: Value = serde_json::from_slice(&std::fs::read(m).unwrap()).unwrap();
        let e = errors(&s, &v);
        assert!(e.is_empty(), "{}: {e:?}", m.display());
    }
    // What is not a manifest is refused.
    let good: Value = serde_json::from_slice(&std::fs::read(&manifests[0]).unwrap()).unwrap();
    for (k, x) in [
        ("format", json!("jetcarp-store")),
        ("version", json!("2.0")),
        ("feed", json!("has spaces")),
        ("publisher", json!("SHA256:short")),
        ("parents", json!(["sha256:abc"])),
        ("created", json!("yesterday")),
    ] {
        let mut bad = good.clone();
        bad[k] = x.clone();
        assert!(!errors(&s, &bad).is_empty(), "accepted {k} = {x}");
    }
    let mut bad = good;
    bad.as_object_mut().unwrap().remove("feed");
    assert!(!errors(&s, &bad).is_empty());
}
