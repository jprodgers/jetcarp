//! RFC 8785 canonical JSON checked against an independent implementation.
//!
//! `tests/data/jcs_vectors.txt` was written by the pure-Python `rfc8785` package
//! (`tools/jcs_vectors.py`, a fixed seed): 300 random JSON values with their canonical text, and
//! 300 random tiling specs (parameters, edge shapes, sizes, boundaries, aperiodic levels) with the
//! `tiling=` line of an identity text on them. JETCARP's writer ([`jetcarp_core::jcs`],
//! [`jetcarp_core::pattern::identity_key`]) must give every line byte for byte.
use jetcarp_core::TilingSpec;
use jetcarp_core::jcs;
use jetcarp_core::pattern::identity_key;

#[test]
fn canonical_json_matches_the_rfc8785_package() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/jcs_vectors.txt");
    let text = std::fs::read_to_string(&path).unwrap();
    let (mut values, mut tilings) = (0, 0);
    let mut bad = vec![];
    for (n, line) in text.lines().enumerate() {
        if line.starts_with('#') || line.is_empty() {
            continue;
        }
        let f: Vec<&str> = line.splitn(3, '\t').collect();
        assert_eq!(f.len(), 3, "line {}: {line}", n + 1);
        let got = match f[0] {
            "value" => {
                values += 1;
                jcs::to_string(&serde_json::from_str(f[1]).unwrap())
            }
            "tiling" => {
                tilings += 1;
                let t: TilingSpec = serde_json::from_str(f[1]).unwrap();
                identity_key(&t)
            }
            k => panic!("line {}: unknown kind {k}", n + 1),
        };
        if got != f[2] {
            bad.push(format!(
                "line {}: {}\n  want {}\n  got  {got}",
                n + 1,
                f[1],
                f[2]
            ));
        }
    }
    assert!(
        values >= 300 && tilings >= 300,
        "{values} values, {tilings} tilings"
    );
    assert!(
        bad.is_empty(),
        "{} lines differ:\n{}",
        bad.len(),
        bad.join("\n")
    );
}

/// The Python function library-format.md gives for the `tiling=` line, as the page has it.
const DOCUMENTED: &str = r#"import rfc8785

APERIODIC = {"hat", "spectre", "penrose-kd", "penrose-rhomb"}

def tiling_key(t):
    jcs = lambda v: rfc8785.dumps(v).decode()
    out = ['"family":' + jcs(t["family"])]
    params = t.get("params")
    if isinstance(params, dict) and t["family"] in APERIODIC:
        params = {k: v for k, v in params.items() if k != "level"} or None
    if params is not None:
        out.append('"params":' + jcs(params))
    if t.get("edges") is not None:
        edges = [{"mode": e["mode"], "curved": e.get("curved", True),
                  "nodes": [{"t": n["t"], "d": n["d"]} for n in e.get("nodes", [])]}
                 for e in t["edges"]]
        out.append('"edges":' + jcs(edges))
    return "{" + ",".join(out + ['"boundary":"dead"']) + "}"
"#;

/// [`DOCUMENTED`], transcribed line for line, on a tiling read as plain JSON.
fn documented_tiling_key(t: &serde_json::Value) -> String {
    use serde_json::{Value, json};
    const APERIODIC: [&str; 4] = ["hat", "spectre", "penrose-kd", "penrose-rhomb"];
    let mut out = vec![format!("\"family\":{}", jcs::to_string(&t["family"]))];
    let mut params = t.get("params").filter(|p| !p.is_null()).cloned();
    if let Some(Value::Object(m)) = &params
        && APERIODIC.contains(&t["family"].as_str().unwrap_or(""))
    {
        let kept: serde_json::Map<String, Value> = m
            .iter()
            .filter(|(k, _)| *k != "level")
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        params = (!kept.is_empty()).then_some(Value::Object(kept));
    }
    if let Some(p) = &params {
        out.push(format!("\"params\":{}", jcs::to_string(p)));
    }
    if let Some(Value::Array(edges)) = t.get("edges") {
        let edges: Vec<Value> = edges
            .iter()
            .map(|e| {
                let nodes: Vec<Value> = e
                    .get("nodes")
                    .and_then(Value::as_array)
                    .map(|ns| ns.iter().map(|n| json!({"t": n["t"], "d": n["d"]})).collect())
                    .unwrap_or_default();
                json!({"mode": e["mode"], "curved": e.get("curved").cloned().unwrap_or(json!(true)), "nodes": nodes})
            })
            .collect();
        out.push(format!(
            "\"edges\":{}",
            jcs::to_string(&Value::Array(edges))
        ));
    }
    out.push("\"boundary\":\"dead\"".into());
    format!("{{{}}}", out.join(","))
}

// The function the book gives for a tiling key writes every tiling line of the independent
// vectors (edges normalised as the engine reads them), and is the one on the page.
#[test]
fn the_documented_tiling_key_function_matches_every_vector() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let page =
        std::fs::read_to_string(dir.join("../../docs/src/reference/library-format.md")).unwrap();
    let code: String = page
        .split("```python\n")
        .find(|b| b.contains("def tiling_key(t):"))
        .and_then(|b| b.split("  ```").next())
        .expect("the page's tiling_key function")
        .lines()
        .map(|l| l.strip_prefix("  ").unwrap_or(l))
        .collect::<Vec<_>>()
        .join("\n");
    assert_eq!(
        code.trim_end(),
        DOCUMENTED.trim_end(),
        "the page's function changed"
    );
    let text = std::fs::read_to_string(dir.join("tests/data/jcs_vectors.txt")).unwrap();
    let mut n = 0;
    for line in text.lines().filter(|l| l.starts_with("tiling\t")) {
        let f: Vec<&str> = line.splitn(3, '\t').collect();
        let t: serde_json::Value = serde_json::from_str(f[1]).unwrap();
        assert_eq!(documented_tiling_key(&t), f[2], "{}", f[1]);
        n += 1;
    }
    assert!(n >= 300, "{n}");
}
