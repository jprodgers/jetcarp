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
