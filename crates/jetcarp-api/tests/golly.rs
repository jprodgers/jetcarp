//! JETCARP's RLE checked against Golly (#145): Golly reads what JETCARP writes as the same cells,
//! JETCARP reads what Golly writes back as the same cells, and the two run a pattern to the same
//! generations, on the square grid (Moore and von Neumann) and the hexagon grid, where JETCARP's
//! offset rows and Golly's sheared grid differ.
//!
//! Ignored: it needs Golly's command-line program, `bgolly` (in Golly 5.0's packages), on the PATH
//! or named by `JETCARP_BGOLLY`, a command whose words are split on spaces (`flatpak-spawn --host
//! bgolly` from inside a Flatpak). The release candidates run it:
//! `cargo test --release -p jetcarp-api --test golly -- --ignored`.

mod common;

use jetcarp_api::Ctx;
use jetcarp_api::cells::{Grid, parse_rle};
use jetcarp_api::op::{Quiet, WorldOp};
use jetcarp_api::ops::world::*;
use jetcarp_api::rle;
use jetcarp_api::world::{Host, Session};
use jetcarp_core::Command;
use serde_json::{Value, json};
use std::collections::BTreeSet;
use std::path::Path;
use std::process::Command as Process;

fn new_world(cx: &Ctx, h: &mut Session, v: Value) {
    WorldNew::run(cx, h, serde_json::from_value(v).unwrap(), &Quiet).unwrap();
}

fn draw(cx: &Ctx, h: &mut Session, v: Value) {
    WorldDraw::run(cx, h, serde_json::from_value(v).unwrap(), &Quiet).unwrap();
}

/// `bgolly`, as a program and its first arguments.
fn bgolly() -> Vec<String> {
    std::env::var("JETCARP_BGOLLY")
        .unwrap_or_else(|_| "bgolly".into())
        .split_whitespace()
        .map(String::from)
        .collect()
}

/// Golly's pattern after `gens` generations of the RLE `text` (0: read and written back).
fn golly(dir: &Path, name: &str, text: &str, gens: u64) -> String {
    let input = dir.join(format!("{name}.rle"));
    let output = dir.join(format!("{name}-golly-{gens}.rle"));
    std::fs::write(&input, text).unwrap();
    let _ = std::fs::remove_file(&output);
    let cmd = bgolly();
    let out = Process::new(&cmd[0])
        .args(&cmd[1..])
        .args(["-q", "-q", "-m", &gens.to_string(), "-o"])
        .arg(&output)
        .arg(&input)
        .output()
        .unwrap_or_else(|e| {
            panic!("this test needs Golly's bgolly (or JETCARP_BGOLLY naming it): {e}")
        });
    assert!(
        out.status.success(),
        "bgolly failed on {name}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    std::fs::read_to_string(&output)
        .unwrap_or_else(|e| panic!("bgolly wrote no {}: {e}", output.display()))
}

/// The live cells of an RLE, in its own coordinates, moved so the lowest x and y are 0.
fn cells(text: &str) -> BTreeSet<(i64, i64)> {
    let p = parse_rle(text).unwrap();
    let x0 = p.iter().map(|c| c.0).min().unwrap_or(0);
    let y0 = p.iter().map(|c| c.1).min().unwrap_or(0);
    p.iter()
        .filter(|c| c.2 != 0)
        .map(|c| (c.0 - x0, c.1 - y0))
        .collect()
}

/// A random soup as a pattern's text, `w` by `h`, about `fill` live (a fixed generator, so every
/// run checks the same soups).
fn soup(seed: u64, w: usize, h: usize, fill: f64) -> String {
    let mut s = seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1;
    let mut rows = Vec::new();
    for _ in 0..h {
        let mut row = String::new();
        for _ in 0..w {
            s ^= s << 13;
            s ^= s >> 7;
            s ^= s << 17;
            row.push(if (s >> 11) as f64 / (1u64 << 53) as f64 <= fill {
                'O'
            } else {
                '.'
            });
        }
        rows.push(row);
    }
    rows.join("\n")
}

/// One case: a tiling, a rule and how JETCARP's CA names its neighbours.
struct Case {
    tiling: &'static str,
    rule: &'static str,
    nb: &'static str,
    gens: &'static [u64],
}

const CASES: &[Case] = &[
    Case {
        tiling: "square",
        rule: "B3/S23",
        nb: "touch",
        gens: &[1, 2, 7, 30, 100],
    },
    Case {
        tiling: "square",
        rule: "B36/S23",
        nb: "touch",
        gens: &[1, 5, 40],
    },
    Case {
        tiling: "square",
        rule: "B3678/S34678",
        nb: "touch",
        gens: &[1, 5, 40],
    },
    Case {
        tiling: "square",
        rule: "B2/S",
        nb: "touch",
        gens: &[1, 3, 8],
    },
    Case {
        tiling: "square",
        rule: "B2/S013",
        nb: "edge",
        gens: &[1, 4, 12],
    },
    Case {
        tiling: "square",
        rule: "B3/S23",
        nb: "edge",
        gens: &[1, 5, 20],
    },
    Case {
        tiling: "hex",
        rule: "B2/S34",
        nb: "touch",
        gens: &[1, 2, 7, 30, 60],
    },
    Case {
        tiling: "hex",
        rule: "B24/S35",
        nb: "touch",
        gens: &[1, 5, 30],
    },
    Case {
        tiling: "hex",
        rule: "B3/S23",
        nb: "touch",
        gens: &[1, 5, 30],
    },
];

/// The world's size, and where the soup goes, far enough from the dead edges that nothing reaches
/// them in the generations run (checked).
const SIZE: i64 = 300;
const SOUP: usize = 24;

/// Golly reads JETCARP's RLE as the same cells, writes it back as JETCARP reads it, and runs it to
/// the same generations, for soups of each rule on squares and hexagons, placed on rows of both
/// parities (where a hexagon's neighbours differ in JETCARP's offset rows).
#[test]
#[ignore = "needs Golly's bgolly; run on the release candidates"]
fn golly_agrees_on_jetcarp_s_rle() {
    let dir = common::scratch("golly");
    let cx = Ctx::interactive();
    let mut checked = 0;
    for (ci, c) in CASES.iter().enumerate() {
        for (si, (seed, fill, row)) in [(1u64, 0.35, 137i64), (2, 0.5, 138)].iter().enumerate() {
            let name = format!("{}-{}-{}-{si}", c.tiling, c.nb, c.rule.replace('/', ""));
            let world = json!({"tiling": c.tiling, "width": SIZE, "height": SIZE,
                "boundary": "dead",
                "cas": [{"rule": c.rule, "seed": "empty", "neighborhood": c.nb}]});
            let mut h = Session::new();
            new_world(&cx, &mut h, world.clone());
            let x = SIZE / 2 - SOUP as i64 / 2;
            draw(
                &cx,
                &mut h,
                json!({"pattern": soup(*seed + ci as u64 * 10, SOUP, SOUP, *fill), "x": x, "y": row}),
            );
            let start = rle::of_world(h.world(), &[]).unwrap();
            let rule_line = rle::header_rule(&start).unwrap();
            assert_eq!(
                rle::header_rule(&golly(&dir, &name, &start, 0)).as_deref(),
                Some(rule_line.as_str()),
                "{name}: Golly names the rule as JETCARP wrote it"
            );

            // Read and written back by Golly: the same cells.
            let back = golly(&dir, &name, &start, 0);
            assert_eq!(cells(&back), cells(&start), "{name}: Golly read\n{start}");

            // Golly's RLE pasted into JETCARP (hexagons turned back from Golly's layout) and
            // copied again: the same cells.
            let mut p = Session::new();
            new_world(&cx, &mut p, world.clone());
            draw(&cx, &mut p, json!({"rle": back, "x": x, "y": row}));
            let again = rle::of_world(p.world(), &[]).unwrap();
            assert_eq!(
                cells(&again),
                cells(&start),
                "{name}: JETCARP read Golly's\n{back}"
            );

            // The same generations.
            let mut done = 0;
            for &g in c.gens {
                h.exec(Command::Step { gens: g - done }).unwrap();
                done = g;
                let w = h.world();
                let ours = rle::of_world(w, &[]).unwrap();
                let live = cells(&ours);
                // Nothing came near the edges, where JETCARP's dead boundary and Golly's unbounded
                // plane would part.
                let grid = Grid::of(&w.topo).unwrap();
                let (gw, gh) = (grid.w, grid.h);
                let near_edge = (0..w.topo.len()).any(|i| {
                    w.fields[0].state[i] != 0 && {
                        let (cx, cy) = grid.xy(i);
                        cx < 4 || cy < 4 || cx + 4 >= gw || cy + 4 >= gh
                    }
                });
                assert!(!near_edge, "{name}: reached the edge by generation {g}");
                let theirs = cells(&golly(&dir, &name, &start, g));
                assert_eq!(
                    live,
                    theirs,
                    "{name}: generation {g} differs ({} live here, {} in Golly)",
                    live.len(),
                    theirs.len()
                );
                checked += 1;
            }
        }
    }
    eprintln!(
        "golly: {checked} generations agree, over {} soups",
        CASES.len() * 2
    );
}

/// Every find in a library that Golly can hold (two-state B/S rules on squares and hexagons), copied
/// as RLE, pastes back into JETCARP as the same cells and runs to the same generations in Golly:
/// over four of its periods, and at least 30 generations. Ignored: it needs `bgolly` and
/// `JETCARP_GOLLY_LIBRARY`, a library file to read (give it a copy; opening it may update its
/// format). The release candidates run it on a library of fresh hunts.
#[test]
#[ignore = "needs Golly's bgolly and a library; run on the release candidates"]
fn golly_agrees_on_a_library_s_finds() {
    let path = std::env::var_os("JETCARP_GOLLY_LIBRARY")
        .expect("set JETCARP_GOLLY_LIBRARY to a library file");
    let lib = jetcarp_store::Library::open(std::path::PathBuf::from(path));
    let records = lib.list().unwrap();
    let dir = common::scratch("golly-finds");
    let cx = Ctx::interactive();
    let (mut checked, mut skipped) = (0usize, 0usize);
    let mut classes: std::collections::BTreeMap<String, usize> = Default::default();
    for r in &records {
        let made = jetcarp_core::pattern::promote::showcase_scene(r, None, None)
            .and_then(|s| jetcarp_core::World::from_scene(&s))
            .and_then(|w| rle::of_world(&w, &[]));
        // Not something Golly holds (another tiling, rule family, or a B0 rule).
        let Ok(text) = made else {
            skipped += 1;
            continue;
        };
        let rule = rle::header_rule(&text).unwrap();
        let (tiling, nb, bs) = if let Some(b) = rule.strip_suffix('H') {
            ("hex", "touch", b.to_string())
        } else if let Some(b) = rule.strip_suffix('V') {
            ("square", "edge", b.to_string())
        } else {
            ("square", "touch", rule.clone())
        };
        let gens = (r.period.max(1) * 4).clamp(30, 400);
        let extent = cells(&text).iter().fold(0i64, |m, &(x, y)| m.max(x).max(y)) + 1;
        let size = (extent + 2 * gens as i64 + 24).min(2000);
        let world = json!({"tiling": tiling, "width": size, "height": size, "boundary": "dead",
            "cas": [{"rule": bs, "seed": "empty", "neighborhood": nb}]});
        let mut h = Session::new();
        new_world(&cx, &mut h, world);
        let at = size / 2 - extent / 2;
        draw(&cx, &mut h, json!({"rle": text, "x": at, "y": at}));
        let start = rle::of_world(h.world(), &[]).unwrap();
        let name = format!("{}-{}", r.id, r.class.key());
        assert_eq!(
            cells(&start),
            cells(&text),
            "{name}: pasted back as other cells\n{text}"
        );
        h.exec(Command::Step { gens }).unwrap();
        let ours = cells(&rle::of_world(h.world(), &[]).unwrap());
        let theirs = cells(&golly(&dir, &name, &start, gens));
        assert_eq!(
            ours,
            theirs,
            "{name} ({rule}): generation {gens} differs ({} live here, {} in Golly)",
            ours.len(),
            theirs.len()
        );
        checked += 1;
        *classes.entry(r.class.key().to_string()).or_default() += 1;
    }
    eprintln!(
        "golly: {checked} finds agree ({classes:?}); {skipped} of {} are not for Golly",
        records.len()
    );
    assert!(checked > 0, "no find in the library is one Golly holds");
}
