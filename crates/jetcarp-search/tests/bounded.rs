//! Bounded soup searches on aperiodic patches: pattern-graph dedup gives few records per pattern.

use jetcarp_core::topology::{Boundary, SizeSpec, TilingSpec};
use jetcarp_search::bounded::{SearchSpec, search};
use std::collections::BTreeMap;

#[test]
fn aperiodic_dedup_gives_few_records_per_pattern() {
    // Soups on the hat and the Spectre: every distinct still life or oscillator is one record
    // per pattern graph, not one per surrounding configuration.
    let spec = SearchSpec {
        rules: vec!["B3/S12".into(), "B3/S23".into()],
        tilings: vec![TilingSpec::new("hat"), TilingSpec::new("spectre")],
        size: SizeSpec::Level(4),
        boundary: Boundary::Dead,
        soups: 8,
        gens: 1000,
        seed: 1,
        ..Default::default()
    };
    let out = search(&spec, None).unwrap();
    let mut by_kind: BTreeMap<(String, String, usize, u64), (usize, u64)> = BTreeMap::new();
    for r in &out.records {
        let site = r.site.as_ref().unwrap();
        assert!(
            site.graph.is_some(),
            "{} fell back to the geometric site",
            r.id
        );
        let e = by_kind
            .entry((r.rule.clone(), r.tiling.family.clone(), r.cells, r.period))
            .or_default();
        e.0 += 1;
        e.1 += r.seen;
    }
    for (k, v) in &by_kind {
        println!("{k:?}: {} records, seen {}", v.0, v.1);
    }
    // The two-tile still life of B3/S12: one record on the Spectre (one tile shape, no
    // reflected tiles), two on the hat (hat-hat and hat-antihat), each seen many times.
    let dom = |fam: &str| by_kind[&("B3/S12".into(), fam.into(), 2, 1)];
    assert_eq!(dom("spectre").0, 1);
    assert!(dom("hat").0 <= 2);
    assert!(dom("spectre").1 >= 50 && dom("hat").1 >= 50);
    // Overall, far fewer records than objects found.
    let records: usize = by_kind.values().map(|v| v.0).sum();
    let seen: u64 = by_kind.values().map(|v| v.1).sum();
    assert!(
        seen >= 8 * records as u64,
        "{records} records for {seen} objects"
    );
    // Each record verifies at its own site and at other matching sites of the patch.
    let multi = out.records.iter().filter(|r| r.verified.sites > 1).count();
    assert!(
        multi * 2 > out.records.len(),
        "{multi} of {}",
        out.records.len()
    );
}
