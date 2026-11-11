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
