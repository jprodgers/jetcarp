//! Geometry pins recomputed from `tilings` rows: test vectors for independent readers.
//!
//! `tests/data/pin_vectors.jsonl` holds the `tilings` row (as JSON) of a set of periodic
//! tilings, written by JETCARP. A reader that follows the pack format's byte layout must get
//! each row's `polygon_hash`, `neighbour_hash` and `pin` from its geometry columns. This test
//! checks that JETCARP's rows are still those, and that the layout gives their pins.

use jetcarp_pack::rows::{pin_of_rows, tiling_row};
use jetcarp_pack::tables::TilingRow;

/// The tilings of the vectors: the regular and Archimedean ones, some k-uniform and
/// isohedral types, and reshapeable families at their default shapes.
const KEYS: [&str; 12] = [
    r#"{"family":"square","boundary":"dead"}"#,
    r#"{"family":"hex","boundary":"dead"}"#,
    r#"{"family":"triangle","boundary":"dead"}"#,
    r#"{"family":"trihex","boundary":"dead"}"#,
    r#"{"family":"snubsq","boundary":"dead"}"#,
    r#"{"family":"oct","boundary":"dead"}"#,
    r#"{"family":"periodic","params":{"catalog":"k2-001"},"boundary":"dead"}"#,
    r#"{"family":"periodic","params":{"catalog":"k3-019"},"boundary":"dead"}"#,
    r#"{"family":"ih58","boundary":"dead"}"#,
    r#"{"family":"ih79","boundary":"dead"}"#,
    r#"{"family":"cairo","boundary":"dead"}"#,
    r#"{"family":"brick","boundary":"dead"}"#,
];

fn path() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/data/pin_vectors.jsonl")
}

#[test]
fn pins_follow_from_the_tilings_columns() {
    if std::env::var_os("JETCARP_PIN_VECTORS_WRITE").is_some() {
        let lines: Vec<String> = KEYS
            .iter()
            .map(|k| serde_json::to_string(&tiling_row(k).unwrap().0).unwrap())
            .collect();
        std::fs::create_dir_all(path().parent().unwrap()).unwrap();
        std::fs::write(path(), lines.join("\n") + "\n").unwrap();
    }
    let text = std::fs::read_to_string(path()).unwrap();
    let mut n = 0;
    for line in text.lines() {
        let row: TilingRow = serde_json::from_str(line).unwrap();
        let (pin, ph, nh) = pin_of_rows(&row, None).unwrap();
        assert_eq!(
            (pin.as_str(), ph.as_str(), nh.as_str()),
            (
                row.pin.as_str(),
                row.polygon_hash.as_str(),
                row.neighbour_hash.as_str()
            ),
            "{}",
            row.pin_key
        );
        // JETCARP still writes this row (its geometry has not changed).
        assert_eq!(tiling_row(&row.pin_key).unwrap().0, row, "{}", row.pin_key);
        n += 1;
    }
    assert_eq!(n, KEYS.len());
    // The square grid's pin is the one the book gives.
    let sq: TilingRow = serde_json::from_str(text.lines().next().unwrap()).unwrap();
    assert_eq!(sq.pin, "0145310ebd49abfe89eb01c42ecaf459");
}
