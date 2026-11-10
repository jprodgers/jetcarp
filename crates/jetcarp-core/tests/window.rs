//! Sliding-window patches: a windowed run equals the run on a fixed exact patch cell for cell
//! while both fit, on the Spectre, the hat, and both Penrose tilings; and a mover followed on a
//! window goes far past the largest fixed patch.

use jetcarp_core::evolve::arena::deepest;
use jetcarp_core::exact::{Iso, Place, Z12};
use jetcarp_core::library::movers::{Stop, TrackSpec, track};
use jetcarp_core::library::site::patch_topology;
use jetcarp_core::topology::{Nb, SizeSpec, Storage, TilingSpec, Topology};
use jetcarp_core::window::track::{Tracker, WindowOpts, track_window};
use jetcarp_core::world::{CaSpec, Command, SeedSpec, World};
use std::collections::VecDeque;
use std::sync::Arc;

/// The published glider pair (states 1 and 3) near the deepest tile of a Spectre level-4 patch.
fn spectre_glider() -> TrackSpec {
    let patch = TilingSpec::new("spectre").with_size(SizeSpec::Level(4));
    let topo = patch_topology(&patch).unwrap();
    let (d, _) = deepest(&topo);
    let mut roots: Vec<usize> = vec![d];
    topo.for_each_neighbor(d, Nb::Edge, |y| roots.push(y));
    for r in roots {
        for k in 0..topo.degree(r, Nb::Edge) {
            let u = topo.slot(r, Nb::Edge, k).unwrap();
            let mut spec = TrackSpec::new(
                "SpectreGlider",
                Nb::Touch,
                patch.clone(),
                vec![(r as u32, 1), (u as u32, 3)],
            );
            spec.max_level = Some(5);
            if let Ok(rep) = track(&spec)
                && rep.verified
            {
                spec.max_level = None;
                return spec;
            }
        }
    }
    panic!("no glider");
}

fn places(t: &Topology) -> &[Place] {
    match &t.storage {
        Storage::Explicit(e) => e.transforms.exact().expect("an exact patch"),
        _ => panic!("not a patch"),
    }
}

fn shapes(t: &Topology) -> Vec<u16> {
    (0..t.len()).map(|i| t.shape_of(i) as u16).collect()
}

fn bfs(topo: &Topology, nb: Nb, src: impl Iterator<Item = usize>) -> Vec<u32> {
    let mut d = vec![u32::MAX; topo.len()];
    let mut q = VecDeque::new();
    for s in src {
        d[s] = 0;
        q.push_back(s);
    }
    while let Some(x) = q.pop_front() {
        topo.for_each_neighbor(x, nb, |y| {
            if d[y] == u32::MAX {
                d[y] = d[x] + 1;
                q.push_back(y);
            }
        });
    }
    d
}

/// Run `spec` on the fixed patch `big` (whose tile `i` sits at `to_base ∘ place(i)` in the start
/// patch's frame) and on a window, comparing the live cells every generation until the fixed run
/// nears its edge. Returns (generations compared, fixed rings at the end, window rings).
fn compare(
    spec: &TrackSpec,
    opts: &WindowOpts,
    big: &Arc<Topology>,
    to_base: Iso,
) -> (u64, u32, u32) {
    let small = patch_topology(&spec.patch).unwrap();
    let ring = if spec.patch.family.starts_with("penrose") {
        10
    } else {
        12
    };
    let base = |s: u16, p: &Place| -> (u16, Place) {
        let iso = if ring == 12 {
            to_base.mul::<Z12>(&p.iso())
        } else {
            to_base.mul::<jetcarp_core::exact::Z10>(&p.iso())
        };
        (s, iso.compact().unwrap())
    };
    let bp = places(big);
    let bs = shapes(big);
    // The start cells on the big patch: by position.
    let mut index = std::collections::HashMap::new();
    for i in 0..big.len() {
        index.insert(base(bs[i], &bp[i]), i);
    }
    let start: Vec<(u32, u8)> = spec
        .cells
        .iter()
        .map(|&(x, s)| {
            if spec.patch.family == "spectre" {
                let k = (0u16, places(&small)[x as usize]);
                (index[&k] as u32, s)
            } else {
                // The size classes nest in place: the same tile at the same position.
                let c = small.centroid(x as usize);
                let y = (0..big.len())
                    .find(|&y| jetcarp_core::geom::dist(big.centroid(y), c) < 1e-6)
                    .unwrap();
                (y as u32, s)
            }
        })
        .collect();
    let mut w = World::on_topology(big.clone(), 1);
    let ca = CaSpec::new(&spec.rule)
        .nb(spec.nb)
        .steps(spec.steps_per_gen)
        .seed(SeedSpec::Empty);
    w.apply(Command::AddCa { ca }).unwrap();
    let id = w.cas[0].id;
    let fi = w.cas[0].field;
    w.apply(Command::SetCells {
        id,
        cells: start.iter().map(|&(x, s)| [x, s as u32]).collect(),
    })
    .unwrap();
    let open = bfs(big, spec.nb, (0..big.len()).filter(|&x| big.is_open(x)));
    let mut t = Tracker::new(spec, opts).unwrap();
    let live_big = |w: &World| -> Vec<((u16, Place), u8)> {
        let mut v: Vec<((u16, Place), u8)> = w.fields[fi]
            .state
            .iter()
            .enumerate()
            .filter(|(_, s)| **s != 0)
            .map(|(x, &s)| (base(bs[x], &bp[x]), s))
            .collect();
        v.sort_unstable_by_key(|(k, _)| (k.0, k.1.t, k.1.rot, k.1.refl));
        v
    };
    assert_eq!(live_big(&w), t.live_keys(), "generation 0");
    let mut g = 0;
    let mut last = vec![];
    loop {
        w.step();
        let now: Vec<(u32, u8)> = w.fields[fi]
            .state
            .iter()
            .enumerate()
            .filter(|(_, s)| **s != 0)
            .map(|(x, &s)| (x as u32, s))
            .collect();
        if now.is_empty()
            || now
                .iter()
                .any(|&(x, _)| open[x as usize] < spec.edge_margin)
        {
            break;
        }
        let stop = t.step_once().unwrap();
        g += 1;
        assert_eq!(live_big(&w), t.live_keys(), "generation {g}");
        last = now;
        if stop.is_some() || g >= spec.max_gens {
            break;
        }
    }
    let rings = bfs(big, Nb::Edge, start.iter().map(|c| c.0 as usize));
    let fixed = last.iter().map(|c| rings[c.0 as usize]).min().unwrap_or(0);
    let far = last.iter().map(|c| rings[c.0 as usize]).max().unwrap_or(0);
    assert_eq!(
        far,
        t.report().reach.rings_far,
        "farthest live tile's rings"
    );
    (g, fixed, t.rings())
}

fn spectre_to_base(from: u32, to: u32) -> Iso {
    let mut m = Iso::ID;
    for l in from..to {
        m = jetcarp_core::spectre::level_embedding_exact(l).mul::<Z12>(&m);
    }
    m.inv::<Z12>()
}

#[test]
fn spectre_window_equals_level_7_cell_for_cell() {
    let spec = spectre_glider();
    let big = patch_topology(&TilingSpec::new("spectre").with_size(SizeSpec::Level(7))).unwrap();
    let (g, fixed, win) = compare(&spec, &WindowOpts::default(), &big, spectre_to_base(4, 7));
    println!("{g} generations equal; rings: fixed {fixed}, window {win}");
    assert!(g > 300, "{g}");
    assert_eq!(fixed, win);
}

// The 17-million-tile patch: `cargo test --release -p jetcarp-core --test window -- --ignored`.
#[test]
#[ignore]
fn spectre_window_equals_level_8_cell_for_cell() {
    let spec = spectre_glider();
    let big = patch_topology(&TilingSpec::new("spectre").with_size(SizeSpec::Level(8))).unwrap();
    let (g, fixed, win) = compare(&spec, &WindowOpts::default(), &big, spectre_to_base(4, 8));
    println!("{g} generations equal; rings: fixed {fixed}, window {win}");
    assert!(g > 1500, "{g}");
    assert_eq!(fixed, win);
}

/// A Life soup of `n` cells nearest the centre of a level-4 patch.
fn soup(family: &str, rule: &str, n: usize, seed: u64) -> TrackSpec {
    let patch = TilingSpec::new(family).with_size(SizeSpec::Level(4));
    let topo = patch_topology(&patch).unwrap();
    let c = topo.centroid(topo.center_cell());
    let mut near: Vec<usize> = (0..topo.len()).collect();
    near.sort_by(|&a, &b| {
        jetcarp_core::geom::dist(topo.centroid(a), c)
            .total_cmp(&jetcarp_core::geom::dist(topo.centroid(b), c))
    });
    let mut rng = jetcarp_core::rng::Rng::new(seed);
    let cells: Vec<(u32, u8)> = near[..3 * n]
        .iter()
        .filter(|_| rng.next_f64() < 0.4)
        .map(|&x| (x as u32, 1))
        .collect();
    let mut s = TrackSpec::new(rule, Nb::Touch, patch, cells);
    s.max_gens = 120;
    s
}

/// The hat glider of the movers section: a 4-row priority rule from tiles 0 and 3 of level 4.
fn hat_glider() -> TrackSpec {
    TrackSpec::new(
        "Priority 4: 3|n2>=1->2; 0|n3>=2->0; 0|n2>=1->3; 0|n1>=1->2",
        Nb::Touch,
        TilingSpec::new("hat").with_size(SizeSpec::Level(4)),
        vec![(0, 1), (3, 2)],
    )
}

#[test]
fn hat_window_equals_level_7_cell_for_cell() {
    let spec = hat_glider();
    let big = patch_topology(&TilingSpec::new("hat").with_size(SizeSpec::Level(7))).unwrap();
    let (g, fixed, win) = compare(&spec, &WindowOpts::default(), &big, Iso::ID);
    println!("hat glider: {g} generations equal; rings: fixed {fixed}, window {win}");
    assert!(g > 1000, "{g}");
    assert_eq!(fixed, win);
}

// A growing Life-like soup (B34/S34 on touching neighbours) moves and widens the window on
// both Penrose tilings until it passes 2,000 cells.
#[test]
fn penrose_windows_equal_level_7_cell_for_cell() {
    for family in ["penrose-kd", "penrose-rhomb"] {
        let mut spec = soup(family, "B34/S34", 40, 1);
        spec.max_gens = 400;
        let big = patch_topology(&TilingSpec::new(family).with_size(SizeSpec::Level(7))).unwrap();
        let (g, fixed, win) = compare(&spec, &WindowOpts::default(), &big, Iso::ID);
        println!("{family}: {g} generations equal; rings: fixed {fixed}, window {win}");
        assert!(g >= 100, "{family}: {g}");
    }
}

#[test]
fn spectre_glider_goes_past_level_8_on_a_window() {
    let spec = spectre_glider();
    let opts = WindowOpts {
        target_rings: Some(3000),
        ..Default::default()
    };
    let mut s = spec.clone();
    s.max_gens = 20_000;
    let rep = track_window(&s, &opts).unwrap();
    println!("{}", rep.summary);
    let w = rep.window.as_ref().unwrap();
    println!(
        "{:?} {:.0} gens/s, {} windows, {:.2} s of {:.2} s building them",
        w.frame, w.gens_per_s, w.recentres, w.build_seconds, w.seconds
    );
    assert!(rep.verified, "{}", rep.summary);
    assert_eq!(rep.stop, Stop::Target);
    assert!(rep.reach.rings >= 3000);
    assert!(w.sliding && w.top_level > 8);
    assert!(rep.max_population <= 16);
}

// A run stopped at a checkpoint and resumed (through JSON, as a job stores it) ends exactly as
// an uninterrupted one; the catalog job does the same and stores the mover with its flag.
#[test]
fn window_runs_resume_exactly() {
    use jetcarp_core::window::track::WindowCheckpoint;
    let mut spec = spectre_glider();
    spec.max_gens = 9_000;
    let opts = WindowOpts::default();
    let mut whole = Tracker::new(&spec, &opts).unwrap();
    whole.run(None, None).unwrap();
    let mut a = Tracker::new(&spec, &opts).unwrap();
    while a.generation() < 5_000 || a.checkpoint().is_none() {
        a.step_once().unwrap();
    }
    let cp = a.checkpoint().unwrap();
    let txt = serde_json::to_string(&cp).unwrap();
    let cp: WindowCheckpoint = serde_json::from_str(&txt).unwrap();
    let mut b = Tracker::resume(&spec, &opts, &cp).unwrap();
    assert_eq!(b.live_keys(), a.live_keys());
    b.run(None, None).unwrap();
    let (rw, rb) = (whole.report(), b.report());
    assert_eq!(b.live_keys(), whole.live_keys());
    assert_eq!(rb.generations, 9_000);
    assert_eq!(rb.reach, rw.reach);
    assert_eq!(
        rb.window.as_ref().unwrap().frame,
        rw.window.as_ref().unwrap().frame
    );
    assert_eq!(rb.stop, Stop::GenLimit);

    // As a catalog job on the job runner, stopped by request on the way and resumed.
    use jetcarp_core::jobs::{RunOptions, Runner, mover::MoverRun};
    use jetcarp_core::window::job::{MoverJobSpec, MoverProgress, start};
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    let d = std::env::temp_dir().join(format!("jetcarp-window-job-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    let lib = jetcarp_core::library::Library::open(&d);
    let js = MoverJobSpec {
        track: spec.clone(),
        window: opts.clone(),
        store: true,
        source: "test".into(),
    };
    let job = start(&lib, &js).unwrap();
    // Stopped before its first step, it pauses with nothing run.
    let stop = Arc::new(AtomicBool::new(true));
    let o = RunOptions {
        stop: Some(stop.clone()),
        ..Default::default()
    };
    let p: MoverProgress = Runner::new(lib.clone()).run(job, &o).unwrap().progress_as();
    assert_eq!((p.status.as_str(), p.generation), ("paused", 0), "{p:?}");
    assert!(lib.job_checkpoint(job).unwrap().is_none());
    // In short steps, stopped once a step has gone past the repeat check's 4,096 generations
    // (its step ends at a new window, which it checkpoints): it pauses there.
    stop.store(false, Ordering::SeqCst);
    let s2 = stop.clone();
    let mut run = MoverRun::default();
    run.seconds = 0.02;
    run.on_progress = Some(Arc::new(move |p: &MoverProgress| {
        if p.status == "running" && p.generation > 4096 {
            s2.store(true, Ordering::SeqCst);
        }
    }));
    let h = Runner::new(lib.clone())
        .spawn_with(job, Box::new(run), o)
        .unwrap();
    let p: MoverProgress = h.wait().unwrap().progress_as();
    assert_eq!(p.status, "paused", "{p:?}");
    assert!(p.generation > 4096 && p.generation < 9_000, "{p:?}");
    assert!(lib.job_checkpoint(job).unwrap().is_some());
    let p: MoverProgress = Runner::new(lib.clone())
        .spawn(job, RunOptions::default())
        .unwrap()
        .wait()
        .unwrap()
        .progress_as();
    assert_eq!(p.status, "done");
    let rep = p.report.unwrap();
    assert_eq!(rep.generations, 9_000);
    assert_eq!(rep.reach, rw.reach);
    let rec = lib.get(p.record.as_ref().unwrap()).unwrap();
    let m = rec.mover.unwrap();
    assert_eq!(m.distance_rings, rw.reach.rings);
    assert!(m.window.unwrap().sliding);
    assert!(rec.verified.field.contains("sliding window"));
}
