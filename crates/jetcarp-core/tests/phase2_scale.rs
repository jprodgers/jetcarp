//! Phase 2 scale: streaming PNG export, constant-time lattice lookup, large grids.
use jetcarp_core::render::{RenderOptions, render, write_png_streaming};
use jetcarp_core::*;

fn read_png(p: &std::path::Path) -> (u32, u32, Vec<u8>) {
    let dec = png::Decoder::new(std::io::BufReader::new(std::fs::File::open(p).unwrap()));
    let mut r = dec.read_info().unwrap();
    let mut buf = vec![0; r.output_buffer_size().unwrap()];
    let info = r.next_frame(&mut buf).unwrap();
    buf.truncate(info.buffer_size());
    (info.width, info.height, buf)
}

// The strip-by-strip PNG writer gives exactly the in-memory render.
#[test]
fn streaming_png_matches_render() {
    let dir = std::env::temp_dir().join(format!("jetcarp-stream-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    for (k, t) in [
        TilingSpec::rect("square", 90, 60, Boundary::Wrap),
        TilingSpec::new("trunctrihex").with_size(SizeSpec::Cells(3000)),
        TilingSpec::new("cairo").with_size(SizeSpec::Cells(2000)),
        TilingSpec::new("spectre").with_size(SizeSpec::Level(3)),
    ]
    .into_iter()
    .enumerate()
    {
        let mut w = World::new(t, 5).unwrap();
        w.add_ca(CaSpec::new("B3/S23")).unwrap();
        w.apply(Command::Step { gens: 7 }).unwrap();
        let opts = RenderOptions {
            scale: Some(9.0),
            ..Default::default()
        };
        let img = render(&w, &opts);
        let p = dir.join(format!("s{k}.png"));
        let (ww, hh) = write_png_streaming(&p, &w, &opts, &[]).unwrap();
        let (pw, ph, px) = read_png(&p);
        assert_eq!((ww, hh), (img.width, img.height));
        assert_eq!((pw, ph), (img.width, img.height));
        assert!(px == img.rgba, "tiling {k}");
    }
    let _ = std::fs::remove_dir_all(&dir);
}

// Lattice lookup agrees with a brute-force point-in-polygon search.
#[test]
fn lattice_locate_matches_brute_force() {
    for t in [
        TilingSpec::rect("square", 20, 15, Boundary::Dead),
        TilingSpec::new("hex").with_size(SizeSpec::Cells(400)),
        TilingSpec::new("snubsq").with_size(SizeSpec::Cells(500)),
        TilingSpec::new("herring").with_size(SizeSpec::Cells(400)),
        TilingSpec::new("cairo").with_size(SizeSpec::Cells(400)),
    ] {
        let topo = Topology::build(&t).unwrap();
        let b = topo.bounds;
        let mut rng = jetcarp_core::rng::Rng::new(3);
        let polys: Vec<_> = (0..topo.len()).map(|i| topo.polygon(i)).collect();
        for _ in 0..2000 {
            let p = [
                b[0] + rng.next_f64() * (b[2] - b[0]),
                b[1] + rng.next_f64() * (b[3] - b[1]),
            ];
            let brute = (0..topo.len()).find(|&i| geom::point_in_poly(p, &polys[i]));
            assert_eq!(topo.locate(p), brute, "{} at {p:?}", t.family);
        }
    }
}

// A 4-million-cell grid builds and steps; lookup stays instant.
#[test]
fn large_grid_builds_and_steps() {
    let mut w = World::new(TilingSpec::rect("square", 2000, 2000, Boundary::Wrap), 1).unwrap();
    w.add_ca(CaSpec::new("B3/S23")).unwrap();
    w.apply(Command::Step { gens: 3 }).unwrap();
    let c = w.topo.locate([1000.5, 1000.5]).unwrap();
    assert_eq!(w.topo.lattice().unwrap().decompose(c).1, 1000);
}

// A tiling built elsewhere (the app builds large patches on a background thread) gives the same
// world, log, and hash as `SetTiling`.
#[test]
fn set_tiling_built_equals_set_tiling() {
    use std::sync::Arc;
    let start = || {
        let mut w = World::new(TilingSpec::rect("square", 40, 30, Boundary::Wrap), 9).unwrap();
        w.add_ca(CaSpec::new("B3/S23")).unwrap();
        w.apply(Command::Step { gens: 3 }).unwrap();
        w
    };
    let spec = TilingSpec::new("spectre").with_size(SizeSpec::Level(4));
    let mut a = start();
    a.apply(Command::SetTiling {
        tiling: spec.clone(),
    })
    .unwrap();
    a.apply(Command::Step { gens: 5 }).unwrap();
    let mut b = start();
    let topo = Arc::new(Topology::build(&spec).unwrap());
    b.set_tiling_built(topo.clone()).unwrap();
    assert!(Arc::ptr_eq(&b.topo, &topo));
    b.apply(Command::Step { gens: 5 }).unwrap();
    assert_eq!(a.hash_hex(), b.hash_hex());
    assert_eq!(a.to_scene().to_json(), b.to_scene().to_json());
    // Only that one command uses it.
    b.apply(Command::SetTiling { tiling: spec }).unwrap();
    assert!(!Arc::ptr_eq(&b.topo, &topo));
}
