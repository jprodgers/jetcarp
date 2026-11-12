//! Large sandpiles: the GPU toppler gives the same pile, toppling count, and sweep count as the
//! CPU on every step, on several tilings and neighbourhoods.

use jetcarp_core::pile::{Board, Centre, Ctl, Pile, PileSpec, Toppler};
use jetcarp_core::{Nb, TilingSpec};
use jetcarp_gpu::pile::PileGpu;
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

fn cells(p: &Pile) -> (Option<[i64; 4]>, Vec<u8>) {
    let Board::Lattice(b) = &p.board else {
        panic!()
    };
    let bx = b.nonzero_box();
    let mut v = Vec::new();
    if let Some(bx) = bx {
        for k in 0..b.lat.nt {
            for j in bx[1]..=bx[3] {
                for i in bx[0]..=bx[2] {
                    v.push(b.get(i, j, k));
                }
            }
        }
    }
    (bx, v)
}

#[test]
fn gpu_piles_equal_cpu_piles() {
    let mut gpu = match PileGpu::headless() {
        Ok(g) => g,
        Err(e) => {
            eprintln!("note: skipping GPU pile test: {e}");
            return;
        }
    };
    eprintln!("GPU pile test on {}", gpu.name());
    for (id, nb, n, centre) in [
        ("square", Nb::Edge, 70_000, Centre::Cell),
        ("square", Nb::Touch, 30_001, Centre::Vertex),
        ("hex", Nb::Edge, 40_000, Centre::Cell),
        ("triangle", Nb::Touch, 50_000, Centre::Edge),
        ("trunctrihex", Nb::Edge, 20_000, Centre::Cell),
        ("snubsq", Nb::Touch, 25_000, Centre::Vertex),
        ("k3-012", Nb::Edge, 20_000, Centre::Cell),
        ("ih41", Nb::Touch, 15_000, Centre::Cell),
    ] {
        let spec = PileSpec {
            tiling: spec_of(id),
            nb,
            grains: n,
            centre,
        };
        let mut a = Pile::new(spec.clone()).unwrap();
        let mut b = Pile::new(spec).unwrap();
        let mut ctl = Ctl::default();
        while !a.finished() {
            let ra = a.next_step(None, &mut ctl).unwrap();
            let rb = b
                .next_step(Some(&mut gpu as &mut dyn Toppler), &mut ctl)
                .unwrap();
            assert_eq!(
                (ra.grains, ra.topplings, ra.sweeps),
                (rb.grains, rb.topplings, rb.sweeps),
                "{id} {nb:?} step {}",
                ra.step
            );
            assert_eq!(cells(&a), cells(&b), "{id} {nb:?} step {}", ra.step);
        }
    }
}

/// A GPU step stopped part way keeps its work: the board comes back from the GPU as its sweeps
/// left it, saved as a checkpoint, and the run carried on from the folder (on the GPU again)
/// gives the CPU's uninterrupted pile and counts.
#[test]
fn a_gpu_step_stopped_part_way_carries_on() {
    use jetcarp_core::pile;
    use std::sync::atomic::{AtomicBool, Ordering};
    let mut gpu = match PileGpu::headless() {
        Ok(g) => g,
        Err(e) => {
            eprintln!("note: skipping GPU pile test: {e}");
            return;
        }
    };
    let dir = std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("jetcarp-gpu-pile-stops-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    for (id, nb, n) in [("square", Nb::Edge, 120_000), ("hex", Nb::Touch, 60_000)] {
        let spec = PileSpec {
            tiling: spec_of(id),
            nb,
            grains: n,
            centre: Centre::Cell,
        };
        let mut cpu = Pile::new(spec.clone()).unwrap();
        let mut ctl = Ctl::default();
        while !cpu.finished() {
            cpu.next_step(None, &mut ctl).unwrap();
        }
        let d = dir.join(id);
        let mut p = Pile::new(spec).unwrap();
        let mut stopped = 0;
        while !p.finished() {
            let cancel = AtomicBool::new(false);
            let ticks = std::cell::Cell::new(0u32);
            let mut ctl = Ctl {
                cancel: Some(&cancel),
                tick_every: 0.0,
                on_tick: Some(Box::new(|_, _, _| {
                    ticks.set(ticks.get() + 1);
                    if ticks.get() >= 2 {
                        cancel.store(true, Ordering::SeqCst);
                    }
                })),
                ..Default::default()
            };
            let r = p.next_step(Some(&mut gpu as &mut dyn Toppler), &mut ctl);
            drop(ctl);
            pile::save_to_dir(&p, &d).unwrap();
            if r.is_err() {
                assert!(p.in_step.is_some());
                stopped += 1;
                p = pile::file::load(&pile::latest_step_file(&d).unwrap()).unwrap();
            }
        }
        assert!(stopped > 0, "{id}: never stopped");
        let key = |p: &Pile| {
            p.records
                .iter()
                .map(|r| (r.grains, r.topplings, r.sweeps))
                .collect::<Vec<_>>()
        };
        assert_eq!(key(&p), key(&cpu), "{id}: counts");
        assert_eq!(cells(&p), cells(&cpu), "{id}: heights");
    }
    let _ = std::fs::remove_dir_all(&dir);
}
