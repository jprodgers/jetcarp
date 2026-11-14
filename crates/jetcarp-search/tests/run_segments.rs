//! Run segments and score sidecars (`jetcarp_search::explore::segments`): what is written reads
//! back, whole and a few rows at a time, and a sidecar's scores replace the segment's.

use jetcarp_core::explore::features::{Features, SignMeasures};
use jetcarp_search::explore::segments::*;
use std::path::PathBuf;

fn rec(id: i64) -> RunRec {
    RunRec {
        id,
        stage: 1 + (id % 5 == 0) as i64,
        parent: (id % 5 == 0).then_some(id - 1),
        tiling: ["square", "hex", "k3-019"][(id % 3) as usize].into(),
        nb: id % 2,
        fam: "life_like".into(),
        rule: format!("B3/S{}", id % 9),
        seed: format!("{{\"random\":{{\"density\":0.{}}}}}", id % 7),
        seed_kind: "random".into(),
        cells: 16_000,
        gens: 1024,
        steps: 1,
        rng: (id as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15) as i64,
        origin: "random".into(),
        fate: ["died", "froze", "cycle", "running"][(id % 4) as usize].into(),
        settle: id % 1000,
        period: id % 7,
        score: ((id % 300) as f32 / 100.0) as f64,
        bin: u32::MAX as i64 - id,
        feat: Features {
            settle: (id % 1000) as u32,
            period: (id % 7) as u32,
            density: id as f32 / 3.0,
            travel: 0.5,
            movers: (id % 11) as u32,
            mover_speed: 0.25,
            // Every third run a JACA run with sign measures; the rest without (nulls).
            signs: (id % 3 == 0).then(|| SignMeasures {
                pos: 1.0 - (id % 50) as f32 / 100.0,
                balance: (id % 50) as f32 / 100.0,
                sorted: 0.75,
                trade: (id % 7) as f32 / 10.0,
                lost: if id % 2 == 0 { 1.0 } else { 0.25 },
            }),
            ..Default::default()
        },
        ts: 1_700_000_000 + id,
    }
}

fn scratch(name: &str) -> PathBuf {
    let d = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join("segment-tests")
        .join(format!("{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

#[test]
fn a_segment_reads_back_what_was_written() {
    let d = scratch("roundtrip");
    let p = d.join("part-000001.parquet");
    let recs: Vec<RunRec> = (1..=300_000).map(|i| rec(i * 2)).collect();
    let mut w = SegmentWriter::create(&p, &[("jetcarp.search".into(), "x".into())]).unwrap();
    for r in &recs {
        w.push(r.clone()).unwrap();
    }
    assert!(w.push(rec(4)).is_err(), "ids must climb");
    let out = w.finish().unwrap();
    assert_eq!((out.rows, out.first_id, out.last_id), (300_000, 2, 600_000));
    assert_eq!(out.groups, vec![131_072, 131_072, 37_856]);
    let s = Segment::open(&p).unwrap();
    assert_eq!(s.rows(), 300_000);
    assert_eq!(s.meta("jetcarp.schema").as_deref(), Some(RUN_SCHEMA));
    assert_eq!(s.meta("jetcarp.search").as_deref(), Some("x"));
    assert_eq!(s.int_range(1, col::ID), Some((262_146, 524_288)));
    // Whole, and a few rows of a row group with every column.
    let mut back = Vec::new();
    for rg in 0..s.row_groups() {
        s.read(rg, ALL, None, None, &mut |_, b| {
            back.extend((0..b.len).map(|i| b.rec(i)));
            Ok(())
        })
        .unwrap();
    }
    assert_eq!(back.len(), recs.len());
    assert!(back == recs, "the rows read back are the rows written");
    let mut some = Vec::new();
    s.read(
        2,
        ALL,
        Some(&[(5, 7), (30_000, 30_001)]),
        None,
        &mut |at, b| {
            some.extend((0..b.len).map(|i| (at + i, b.rec(i))));
            Ok(())
        },
    )
    .unwrap();
    let base = 2 * 131_072;
    assert_eq!(
        some,
        vec![
            (5, recs[base + 5].clone()),
            (6, recs[base + 6].clone()),
            (30_000, recs[base + 30_000].clone())
        ]
    );
    // A sidecar: its scores replace the segment's.
    let sp = d.join("scores.parquet");
    let mut w = SegmentWriter::create_scores(&sp, &[], s.group_rows()).unwrap();
    for r in &recs {
        let mut r = r.clone();
        r.score = -r.score - 1.0;
        w.push(r).unwrap();
    }
    assert_eq!(w.finish().unwrap().groups, s.group_rows());
    let side = Segment::open(&sp).unwrap();
    let mut got = Vec::new();
    s.read(
        0,
        cols(&[col::ID, col::SCORE]),
        Some(&[(10, 12)]),
        Some(&side),
        &mut |_, b| {
            got.extend((0..b.len).map(|i| (b.id(i), b.score(i))));
            Ok(())
        },
    )
    .unwrap();
    assert_eq!(
        got,
        vec![
            (recs[10].id, (-recs[10].score - 1.0) as f32 as f64),
            (recs[11].id, (-recs[11].score - 1.0) as f32 as f64)
        ]
    );
}

/// A segment of schema 1 (before the sign columns) still reads: its runs have no sign measures,
/// and asking for the sign columns finds them null. A feature blob of 20 floats reads the same.
#[test]
fn a_schema_1_segment_reads_without_sign_measures() {
    use jetcarp_search::explore::store::{feat_blob, feat_from_blob};
    let d = scratch("schema1");
    let p = d.join("part-000001.parquet");
    let recs: Vec<RunRec> = (1..=500).map(rec).collect();
    let mut w = SegmentWriter::create_schema1(&p, &[]).unwrap();
    for r in &recs {
        w.push(r.clone()).unwrap();
    }
    w.finish().unwrap();
    let s = Segment::open(&p).unwrap();
    assert_eq!(s.meta("jetcarp.schema").as_deref(), Some(RUN_SCHEMAS[0]));
    let mut back = Vec::new();
    s.read(0, ALL, None, None, &mut |_, b| {
        assert!(!b.has(col::SIGN0));
        back.extend((0..b.len).map(|i| b.rec(i)));
        Ok(())
    })
    .unwrap();
    let want: Vec<RunRec> = recs
        .into_iter()
        .map(|mut r| {
            r.feat.signs = None;
            r
        })
        .collect();
    assert!(back == want);
    // Blobs: 24 floats now, 20 before; both read.
    let mut f = rec(3).feat;
    assert!(f.signs.is_some());
    let blob = feat_blob(&f);
    assert_eq!(blob.len(), 100);
    assert_eq!(feat_from_blob(&blob), f);
    f.signs = None;
    assert_eq!(feat_from_blob(&blob[..80]), f);
    let none = feat_blob(&f);
    assert_eq!(feat_from_blob(&none), f, "NaN signs read as none");
}
