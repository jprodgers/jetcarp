//! Large sandpiles: doubling, the growing board, and active-region toppling against direct
//! stabilisation with the existing engine.

use jetcarp_core::pile::{self, Board, Centre, Ctl, Pile, PileSpec, Site};
use jetcarp_core::rules::FieldKind;
use jetcarp_core::world::Field;
use jetcarp_core::{Boundary, Nb, SizeSpec, Storage, TilingSpec, Topology};
use serde_json::json;

fn spec_of(id: &str) -> TilingSpec {
    if jetcarp_core::families::family(id).is_some() {
        TilingSpec::new(id)
    } else {
        let mut s = TilingSpec::new("periodic");
        s.params = Some(json!({ "catalog": id }));
        s
    }
}

fn build(t: TilingSpec, nb: Nb, grains: u64, centre: Centre) -> Pile {
    let mut p = Pile::new(PileSpec {
        tiling: t,
        nb,
        grains,
        centre,
    })
    .unwrap();
    let mut ctl = Ctl::default();
    while !p.finished() {
        p.next_step(None, &mut ctl).unwrap();
    }
    p
}

/// Stabilise the same sources directly (one tower per source, bulk toppling) on a dead-edge
/// rectangle and compare every cell.
fn check_lattice(id: &str, nb: Nb, grains: u64, centre: Centre) {
    let p = build(spec_of(id), nb, grains, centre);
    let Board::Lattice(b) = &p.board else {
        panic!()
    };
    assert_eq!(b.grains(), grains, "{id}: grains conserved");
    let mut t = spec_of(id);
    t.size = Some(SizeSpec::Rect { w: 70, h: 70 });
    t.boundary = Boundary::Dead;
    let topo = Topology::build(&t).unwrap();
    let g = topo.lattice().unwrap();
    let (cu, cj, _) = g.decompose(topo.center_cell());
    let ci = cu + g.row_start[cj as usize];
    let at = |i: i64, j: i64, k: u32| {
        g.resolve(ci + i as i32, cj + j as i32, k)
            .expect("source inside the reference grid")
    };
    let mut f = Field::new(FieldKind::Height, topo.len(), false);
    for s in &p.sources {
        let Site::Lattice { i, j, k } = s.site else {
            panic!()
        };
        f.height[at(i, j, k)] += s.grains as u32;
    }
    jetcarp_core::sandpile::stabilize(&topo, nb, &mut f).unwrap();
    assert_eq!(f.lost, 0, "{id}: reference grid too small");
    let mut n = 0u64;
    for c in 0..topo.len() {
        let (u, j, k) = g.decompose(c);
        let i = u + g.row_start[j as usize];
        let h = b.get((i - ci) as i64, (j - cj) as i64, k as usize);
        assert_eq!(
            h as u32, f.height[c],
            "{id} {nb:?} {centre:?} N={grains}: cell {i},{j},{k}"
        );
        n += h as u64;
    }
    assert_eq!(n, grains, "{id}: the pile fits in the reference grid");
    let (topplings, _, _) = p.totals();
    assert!(topplings > 0 || grains < 4);
}

#[test]
fn doubling_matches_direct_stabilisation() {
    for (id, nb, n) in [
        ("square", Nb::Edge, 1000),
        ("square", Nb::Edge, 4097),
        ("square", Nb::Touch, 3001),
        ("hex", Nb::Edge, 2500),
        ("triangle", Nb::Edge, 1500),
        ("triangle", Nb::Touch, 5003),
        ("snubsq", Nb::Edge, 2222),
        ("trunctrihex", Nb::Touch, 1999),
        ("cairo", Nb::Edge, 1777),
        ("hex_flat", Nb::Edge, 1234),
        ("k2-005", Nb::Edge, 2048),
        ("ih28", Nb::Touch, 999),
    ] {
        check_lattice(id, nb, n, Centre::Cell);
    }
}

#[test]
fn vertex_and_edge_centres() {
    for (id, nb) in [
        ("square", Nb::Edge),
        ("hex", Nb::Edge),
        ("triangle", Nb::Touch),
        ("trihex", Nb::Edge),
    ] {
        for c in [Centre::Vertex, Centre::Edge] {
            check_lattice(id, nb, 2003, c);
        }
    }
    // The split: a square's corner meets 4 cells, an edge 2; hex corners meet 3.
    let lat = pile::lattice::Lattice::new(&TilingSpec::new("square"), Nb::Edge).unwrap();
    let v = pile::lattice_sources(&lat, Centre::Vertex, 10);
    assert_eq!(
        v.iter().map(|s| s.grains).collect::<Vec<_>>(),
        vec![3, 3, 2, 2]
    );
    let e = pile::lattice_sources(&lat, Centre::Edge, 11);
    assert_eq!(e.iter().map(|s| s.grains).sum::<u64>(), 11);
    assert_eq!(e.len(), 2);
    let lat = pile::lattice::Lattice::new(&TilingSpec::new("hex"), Nb::Edge).unwrap();
    assert_eq!(pile::lattice_sources(&lat, Centre::Vertex, 9).len(), 3);
}

#[test]
fn patches_match_direct_stabilisation() {
    for (fam, level, nb, n) in [
        ("spectre", 5, Nb::Edge, 1500),
        ("hat", 5, Nb::Edge, 1200),
        ("penrose-rhomb", 5, Nb::Touch, 1600),
    ] {
        for centre in [Centre::Cell, Centre::Vertex] {
            let t = TilingSpec::new(fam).with_size(SizeSpec::Level(level));
            let p = build(t.clone(), nb, n, centre);
            let Board::Patch(b) = &p.board else { panic!() };
            let topo = Topology::build(&t).unwrap();
            let mut f = Field::new(FieldKind::Height, topo.len(), false);
            for s in &p.sources {
                let Site::Patch { cell } = s.site else {
                    panic!()
                };
                f.height[cell as usize] += s.grains as u32;
            }
            jetcarp_core::sandpile::stabilize(&topo, nb, &mut f).unwrap();
            assert_eq!(f.lost, 0, "{fam}: patch too small");
            let h: Vec<u32> = b.cur.iter().map(|&x| x as u32).collect();
            assert_eq!(h, f.height, "{fam} {centre:?}");
            assert!(matches!(topo.storage, Storage::Explicit(_)));
        }
    }
}

#[test]
fn patch_edge_is_an_error() {
    let t = TilingSpec::new("spectre").with_size(SizeSpec::Level(2));
    let mut p = Pile::new(PileSpec {
        tiling: t,
        nb: Nb::Edge,
        grains: 5000,
        centre: Centre::Cell,
    })
    .unwrap();
    let mut ctl = Ctl::default();
    let mut err = None;
    while !p.finished() {
        if let Err(e) = p.next_step(None, &mut ctl) {
            err = Some(e);
            break;
        }
    }
    assert!(err.unwrap().contains("open edge"));
}

#[test]
fn same_result_on_any_thread_count() {
    let run = |threads: usize, id: &str, nb: Nb| {
        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(threads)
            .build()
            .unwrap();
        pool.install(|| {
            let p = build(spec_of(id), nb, 60_000, Centre::Cell);
            let Board::Lattice(b) = &p.board else {
                panic!()
            };
            let bx = b.nonzero_box().unwrap();
            let mut cells = Vec::new();
            for k in 0..b.lat.nt {
                for j in bx[1]..=bx[3] {
                    for i in bx[0]..=bx[2] {
                        cells.push(b.get(i, j, k));
                    }
                }
            }
            (cells, p.records.clone())
        })
    };
    for (id, nb) in [("square", Nb::Edge), ("triangle", Nb::Touch)] {
        let one = run(1, id, nb);
        let all = run(0, id, nb);
        assert_eq!(one.0, all.0, "{id}: piles");
        let strip = |r: &Vec<pile::StepRecord>| {
            r.iter()
                .map(|r| (r.grains, r.topplings, r.sweeps))
                .collect::<Vec<_>>()
        };
        assert_eq!(
            strip(&one.1),
            strip(&all.1),
            "{id}: toppling and sweep counts"
        );
    }
}

#[test]
fn save_load_resume() {
    let dir = std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("jetcarp-pile-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    for id in ["hex", "spectre"] {
        let mut t = spec_of(id);
        if id == "spectre" {
            t.size = Some(SizeSpec::Level(5));
        }
        let spec = PileSpec {
            tiling: t,
            nb: Nb::Edge,
            grains: 3333,
            centre: Centre::Cell,
        };
        let mut full = Pile::new(spec.clone()).unwrap();
        let mut ctl = Ctl::default();
        while !full.finished() {
            full.next_step(None, &mut ctl).unwrap();
        }
        let mut part = Pile::new(spec).unwrap();
        for _ in 0..7 {
            part.next_step(None, &mut ctl).unwrap();
        }
        let path = dir.join(format!("{id}.pile"));
        pile::file::save(&part, &path).unwrap();
        let mut back = pile::file::load(&path).unwrap();
        assert_eq!(back.done, 7);
        while !back.finished() {
            back.next_step(None, &mut ctl).unwrap();
        }
        let key = |p: &Pile| {
            p.records
                .iter()
                .map(|r| (r.grains, r.topplings, r.sweeps))
                .collect::<Vec<_>>()
        };
        assert_eq!(key(&back), key(&full), "{id}");
        match (&back.board, &full.board) {
            (Board::Lattice(a), Board::Lattice(b)) => {
                let bx = b.nonzero_box().unwrap();
                assert_eq!(a.nonzero_box().unwrap(), bx);
                for j in bx[1]..=bx[3] {
                    for i in bx[0]..=bx[2] {
                        assert_eq!(a.get(i, j, 0), b.get(i, j, 0));
                    }
                }
            }
            (Board::Patch(a), Board::Patch(b)) => assert_eq!(a.cur, b.cur),
            _ => panic!(),
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// A pile file's stored region and heights (its header without the run's times and box).
fn stored(path: &std::path::Path) -> (serde_json::Value, Vec<u8>) {
    use std::io::Read;
    let b = std::fs::read(path).unwrap();
    let n = u32::from_le_bytes(b[12..16].try_into().unwrap()) as usize;
    let h: serde_json::Value = serde_json::from_slice(&b[16..16 + n]).unwrap();
    let mut heights = Vec::new();
    flate2::read::DeflateDecoder::new(&b[16 + n..])
        .read_to_end(&mut heights)
        .unwrap();
    (h["region"].clone(), heights)
}

/// Run a pile in `dir`, stopping every step after `stops` ticks (then again after more), each
/// stop saved as a checkpoint and the run carried on from the folder's latest file, as a resumed
/// job does. Returns the finished pile and how many times it stopped part way.
fn run_with_stops(spec: &PileSpec, dir: &std::path::Path, stops: &[u32]) -> (Pile, u32) {
    std::fs::create_dir_all(dir).unwrap();
    let mut p = Pile::new(spec.clone()).unwrap();
    let mut stopped = 0;
    let mut k = 0;
    while !p.finished() {
        let cancel = std::sync::atomic::AtomicBool::new(false);
        let ticks = std::cell::Cell::new(0u32);
        let at = stops.get(k).copied();
        let mut ctl = Ctl {
            cancel: Some(&cancel),
            tick_every: 0.0,
            on_tick: Some(Box::new(|_, _, _| {
                ticks.set(ticks.get() + 1);
                if at.is_some_and(|a| ticks.get() >= a) {
                    cancel.store(true, std::sync::atomic::Ordering::SeqCst);
                }
            })),
            ..Default::default()
        };
        match p.next_step(None, &mut ctl) {
            Ok(_) => {
                pile::save_to_dir(&p, dir).unwrap();
                k = 0;
            }
            Err(e) => {
                assert_eq!(e, "cancelled");
                let x = p.in_step.expect("a stopped step keeps its counts");
                assert_eq!(x.step, p.done + 1);
                assert!(x.sweeps > 0 && x.topplings > 0, "{x:?}");
                let f = pile::save_to_dir(&p, dir).unwrap();
                assert!(
                    f.ends_with(format!("step-{:02}.partial.pile", x.step)),
                    "{f:?}"
                );
                // Carried on from the folder, as a resumed run is.
                let last = pile::latest_step_file(dir).unwrap();
                assert_eq!(last, f);
                p = pile::file::load(&last).unwrap();
                assert_eq!(p.in_step, Some(x));
                stopped += 1;
                k += 1;
            }
        }
        drop(ctl);
    }
    // The checkpoints go once their steps finish.
    for e in std::fs::read_dir(dir.join("steps")).unwrap() {
        let name = e.unwrap().file_name().to_string_lossy().to_string();
        assert!(!name.contains("partial"), "{name} left behind");
    }
    (p, stopped)
}

/// A step stopped part way (a stop, or a job's time budget) keeps its work as a checkpoint, and
/// the run carried on from it gives the uninterrupted pile byte for byte: the same heights, the
/// same pictures, and every step's grain, toppling and sweep counts, on a periodic tiling and on
/// an aperiodic patch.
#[test]
fn a_step_stopped_part_way_carries_on() {
    let root = std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("jetcarp-pile-stops-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    for (id, grains) in [("hex", 30_000u64), ("spectre", 5_000)] {
        let mut t = spec_of(id);
        if id == "spectre" {
            t.size = Some(SizeSpec::Level(5));
        }
        let spec = PileSpec {
            tiling: t,
            nb: Nb::Edge,
            grains,
            centre: Centre::Cell,
        };
        let (full, none) = run_with_stops(&spec, &root.join(format!("{id}-full")), &[]);
        assert_eq!(none, 0);
        let (part, stopped) = run_with_stops(&spec, &root.join(format!("{id}-part")), &[2, 3, 1]);
        assert!(stopped >= 3, "{id}: stopped {stopped} times");
        let key = |p: &Pile| {
            p.records
                .iter()
                .map(|r| (r.step, r.grains, r.topplings, r.sweeps))
                .collect::<Vec<_>>()
        };
        assert_eq!(key(&part), key(&full), "{id}: counts");
        let last =
            |p: &Pile, which: &str| pile::step_path(&root.join(format!("{id}-{which}")), p.done);
        assert_eq!(
            stored(&last(&part, "part")),
            stored(&last(&full, "full")),
            "{id}: heights"
        );
        let opts = pile::RunOptions::default();
        for (p, which) in [(&full, "full"), (&part, "part")] {
            pile::export::finish(p, &root.join(format!("{id}-{which}")), &opts).unwrap();
        }
        for f in ["pile.png", "preview.png"] {
            let a = std::fs::read(root.join(format!("{id}-full/{f}"))).unwrap();
            let b = std::fs::read(root.join(format!("{id}-part/{f}"))).unwrap();
            assert!(a == b, "{id}: {f} differs");
        }
    }
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn run_to_dir_writes_everything() {
    let dir = std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("jetcarp-pile-run-{}", std::process::id()));
    let mut p = Pile::new(PileSpec {
        tiling: spec_of("trihex"),
        nb: Nb::Edge,
        grains: 2000,
        centre: Centre::Edge,
    })
    .unwrap();
    let ticks = std::cell::Cell::new(0);
    let mut ctl = Ctl {
        tick_every: 0.0,
        on_tick: Some(Box::new(|_, _, b| {
            let img = pile::export::preview(&b, 128, &Default::default());
            assert!(img.width <= 128 && img.height <= 128);
            ticks.set(ticks.get() + 1);
        })),
        ..Default::default()
    };
    let s = pile::run_to_dir(&mut p, &dir, None, &mut ctl, &Default::default()).unwrap();
    drop(ctl);
    assert!(ticks.get() > 0);
    assert!(s.png.unwrap().exists());
    assert!(s.preview.unwrap().exists());
    let meta: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(dir.join("pile.json")).unwrap()).unwrap();
    assert_eq!(meta["grains"], 2000);
    assert_eq!(meta["centre"], "edge");
    assert!(meta["topplings"].as_u64().unwrap() > 0);
    let steps = std::fs::read_dir(dir.join("steps")).unwrap().count();
    assert_eq!(steps as u32, p.steps);
    let csv = std::fs::read_to_string(dir.join("growth.csv")).unwrap();
    assert_eq!(csv.lines().count() as u32, p.steps + 1);
    assert!(
        pile::latest_step_file(&dir)
            .unwrap()
            .ends_with(format!("step-{:02}.pile", p.steps))
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn sixty_four_bit_grain_counts() {
    let p = Pile::new(PileSpec {
        tiling: TilingSpec::new("square"),
        nb: Nb::Edge,
        grains: u64::MAX,
        centre: Centre::Vertex,
    })
    .unwrap();
    assert_eq!(p.steps, 63); // 4 cells: three get 2^62, one 2^62 − 1
    assert_eq!(p.grains_after(p.steps), u64::MAX);
    let p = Pile::new(PileSpec {
        tiling: TilingSpec::new("square"),
        nb: Nb::Edge,
        grains: u64::MAX,
        centre: Centre::Cell,
    })
    .unwrap();
    assert_eq!(p.steps, 64);
    assert_eq!(p.grains_after(1), 1);
    assert_eq!(p.grains_after(3), 7);
}

#[test]
fn grain_notation() {
    assert_eq!(pile::parse_grains("1e7").unwrap(), 10_000_000);
    assert_eq!(pile::parse_grains("2.5e6").unwrap(), 2_500_000);
    assert_eq!(pile::parse_grains("2^30").unwrap(), 1 << 30);
    assert_eq!(pile::parse_grains("1_000").unwrap(), 1000);
    assert_eq!(
        pile::parse_grains("18446744073709551615").unwrap(),
        u64::MAX
    );
    assert!(pile::parse_grains("1e20").is_err());
    assert!(pile::parse_grains("1.5").is_err());
}

/// Pictures take the palette: its stops for heights, its background for empty cells, its stage
/// where there is no pile; *reverse* runs the stops the other way; the default is JETCARP's old
/// fixed colours exactly.
#[test]
fn pile_pictures_take_the_palette() {
    use jetcarp_core::pile::export::{Look, height_lut, preview};
    use jetcarp_core::render;
    // The default look: Ember on the dark slate, the old constants.
    let d = Look::default();
    let lut = height_lut(4, &d);
    assert_eq!(lut[0], render::BACKGROUND);
    assert_eq!(lut[1], render::ramp("Ember", 0.0));
    assert_eq!(lut[3], render::ramp("Ember", 1.0));
    assert_eq!(d.palette.stage_rgba(), render::STAGE);
    assert_eq!(d.palette.outline_rgba(), render::OUTLINE);
    // Paper (light): every pixel of the preview is a Paper colour; empty cells are its
    // background, so a corner of the margin is.
    let p = build(spec_of("square"), Nb::Edge, 3000, Centre::Cell);
    let paper = Look::named("Paper", false);
    let plut = height_lut(4, &paper);
    let img = preview(&p.board.as_ref(), 96, &paper);
    let px = |x: u32, y: u32| {
        let o = ((y * img.width + x) * 4) as usize;
        [
            img.rgba[o],
            img.rgba[o + 1],
            img.rgba[o + 2],
            img.rgba[o + 3],
        ]
    };
    let bg = paper.palette.background_rgba();
    assert_ne!(bg, render::BACKGROUND);
    assert_eq!(px(0, 0), bg, "margin cells take the palette's background");
    for y in 0..img.height {
        for x in 0..img.width {
            assert!(plut[..4].contains(&px(x, y)), "({x}, {y}) {:?}", px(x, y));
        }
    }
    let ember = preview(&p.board.as_ref(), 96, &d);
    assert_ne!(ember.rgba, img.rgba);
    // Reverse: height 1 takes the far end.
    let r = height_lut(4, &Look::named("Paper", true));
    assert_eq!((r[0], r[1], r[3]), (plut[0], plut[3], plut[1]));
    let rev = preview(&p.board.as_ref(), 96, &Look::named("Paper", true));
    assert_ne!(rev.rgba, img.rgba);
    assert_eq!(&rev.rgba[..4], &bg[..]);
    // No pile yet: the stage.
    let empty = Pile::new(PileSpec {
        tiling: spec_of("hex"),
        nb: Nb::Edge,
        grains: 10,
        centre: Centre::Cell,
    })
    .unwrap();
    let st = preview(&empty.board.as_ref(), 96, &paper);
    assert!(st.rgba.chunks(4).all(|c| c == paper.palette.stage_rgba()));
    // An unknown name falls back to Ember.
    assert_eq!(Look::named("Nope", false), d);
}
