//! Portable geometry: the bits of every built topology are pinned.
//!
//! For every tiling family at its default size (the hand-coded families, every Archimedean and
//! k-uniform tiling of the catalog, and every isohedral type with its default parameters) this
//! hashes the cell polygons' coordinates bit for bit and, separately, the neighbour lists of
//! both neighbourhoods in slot order; and it hashes Lenia kernel weights on a few tilings.
//! Tiling construction uses only `detmath` and IEEE-754 arithmetic (no `sin`, `cos`, `hypot` or
//! `atan2` from the platform's maths library), so these hashes are the same on every OS and
//! CPU. `tests/data/geometry_pins.txt` holds the expected values; `JETCARP_PIN_WRITE=1` rewrites
//! it (only after a deliberate geometry change).
use jetcarp_core::rules::Rule;
use jetcarp_core::*;
use rayon::prelude::*;
use std::collections::BTreeMap;

const FNV: u64 = 0xcbf2_9ce4_8422_2325;
fn fnv(h: &mut u64, v: u64) {
    *h = (*h ^ v).wrapping_mul(0x100_0000_01b3);
}

fn specs() -> Vec<(String, TilingSpec)> {
    let mut out = vec![];
    for f in jetcarp_core::families::FAMILIES {
        if f.id == "periodic" {
            continue;
        }
        out.push((f.id.to_string(), TilingSpec::new(f.id)));
    }
    for t in jetcarp_core::catalog::all() {
        let mut s = TilingSpec::new("periodic");
        s.params = Some(serde_json::json!({ "catalog": t.id }));
        out.push((format!("periodic:{}", t.id), s));
    }
    for n in 1..=93 {
        if let Some(id) = jetcarp_core::isohedral::id_for(n) {
            out.push((id.to_string(), TilingSpec::new(id)));
        }
    }
    out
}

/// `(geometry hash, graph hash, cells)` of a topology.
fn hashes(t: &Topology) -> (u64, u64, usize) {
    let (mut g, mut n) = (FNV, FNV);
    for i in 0..t.len() {
        for p in t.polygon(i) {
            fnv(&mut g, p[0].to_bits());
            fnv(&mut g, p[1].to_bits());
        }
        for nb in [Nb::Edge, Nb::Touch] {
            fnv(&mut n, u64::MAX);
            t.for_each_neighbor(i, nb, |j| fnv(&mut n, j as u64));
        }
    }
    (g, n, t.len())
}

fn kernel_hash(spec: TilingSpec, rule: &str) -> u64 {
    let t = Topology::build(&spec).unwrap();
    let Rule::Lenia(l) = Rule::parse(rule).unwrap().0 else {
        panic!("not Lenia")
    };
    let k = jetcarp_core::lenia::kernel(&t, &l).unwrap();
    let mut h = FNV;
    match k {
        jetcarp_core::lenia::Kernel::Lattice(v) => {
            for (ty, taps) in v.iter().enumerate() {
                fnv(&mut h, ty as u64);
                for &(di, dj, k2, w) in taps {
                    fnv(&mut h, di as u64);
                    fnv(&mut h, dj as u64);
                    fnv(&mut h, k2 as u64);
                    fnv(&mut h, w.to_bits() as u64);
                }
            }
        }
        jetcarp_core::lenia::Kernel::Explicit(v) => {
            for taps in &v {
                fnv(&mut h, u64::MAX);
                for &(c, w) in taps {
                    fnv(&mut h, c as u64);
                    fnv(&mut h, w.to_bits() as u64);
                }
            }
        }
    }
    h
}

fn kernels() -> Vec<(String, u64)> {
    let r = "Lenia R=6,T=10,b=1,m=0.15,s=0.017";
    vec![
        (
            "lenia:hex".into(),
            kernel_hash(TilingSpec::rect("hex", 40, 40, Boundary::Wrap), r),
        ),
        (
            "lenia:triangle".into(),
            kernel_hash(TilingSpec::rect("triangle", 40, 40, Boundary::Wrap), r),
        ),
        (
            "lenia:oct".into(),
            kernel_hash(TilingSpec::rect("oct", 30, 30, Boundary::Wrap), r),
        ),
        (
            "lenia:snubhex".into(),
            kernel_hash(TilingSpec::rect("snubhex", 20, 20, Boundary::Wrap), r),
        ),
        (
            "lenia:cairo".into(),
            kernel_hash(TilingSpec::rect("cairo", 30, 30, Boundary::Wrap), r),
        ),
        (
            "lenia:spectre".into(),
            kernel_hash(
                TilingSpec::new("spectre").with_size(SizeSpec::Level(3)),
                "Lenia R=4,T=10,b=1,m=0.15,s=0.017",
            ),
        ),
        (
            "lenia:penrose-rhomb".into(),
            kernel_hash(
                TilingSpec::new("penrose-rhomb").with_size(SizeSpec::Level(5)),
                "Lenia R=4,T=10,b=1,m=0.15,s=0.017",
            ),
        ),
    ]
}

fn current() -> BTreeMap<String, String> {
    let mut m: BTreeMap<String, String> = specs()
        .par_iter()
        .map(|(id, s)| {
            let v = match Topology::build(s) {
                Ok(t) => {
                    let (g, n, c) = hashes(&t);
                    format!("{g:016x} {n:016x} {c}")
                }
                Err(e) => format!("error {}", e.replace(char::is_whitespace, "_")),
            };
            (id.clone(), v)
        })
        .collect();
    for (id, h) in kernels() {
        m.insert(id, format!("{h:016x}"));
    }
    m
}

const PATH: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data/geometry_pins.txt");

#[test]
fn topology_and_kernel_hashes_are_pinned() {
    let now = current();
    if std::env::var("JETCARP_PIN_WRITE").is_ok() {
        let mut s = String::from(
            "# Pinned geometry hashes (tests/geometry_pins.rs): id, polygon-coordinate hash,\n\
             # neighbour-list hash, cells; Lenia rows: kernel-weight hash.\n",
        );
        for (k, v) in &now {
            s.push_str(&format!("{k} {v}\n"));
        }
        std::fs::create_dir_all(std::path::Path::new(PATH).parent().unwrap()).unwrap();
        std::fs::write(PATH, s).unwrap();
        return;
    }
    let text = std::fs::read_to_string(PATH).expect("tests/data/geometry_pins.txt");
    let want: BTreeMap<String, String> = text
        .lines()
        .filter(|l| !l.starts_with('#') && !l.trim().is_empty())
        .filter_map(|l| l.split_once(' '))
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
    let mut bad = vec![];
    for (k, v) in &now {
        match want.get(k) {
            Some(w) if w == v => {}
            Some(w) => bad.push(format!("{k}: pinned {w}, now {v}")),
            None => bad.push(format!("{k}: not pinned (now {v})")),
        }
    }
    for k in want.keys() {
        if !now.contains_key(k) {
            bad.push(format!("{k}: pinned but no longer built"));
        }
    }
    assert!(
        bad.is_empty(),
        "{} of {} geometry hashes differ:\n{}",
        bad.len(),
        now.len(),
        bad.iter().take(40).cloned().collect::<Vec<_>>().join("\n")
    );
}

/// Tiling construction calls no platform maths: the source of every module that builds
/// geometry is free of `sin`, `cos`, `tan`, `hypot`, `atan2`, `exp`, `ln`, `powf` and `powi`
/// calls (they go through `detmath`).
#[test]
fn geometry_sources_use_detmath() {
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/src/");
    let files = [
        "geom.rs",
        "families.rs",
        "catalog.rs",
        "topology.rs",
        "penrose.rs",
        "spectre.rs",
        "hat.rs",
        "aperiodic.rs",
        "exact.rs",
        "exact_graph.rs",
        "sizeclass.rs",
        "zomega.rs",
        "cellcheck.rs",
        "edges.rs",
        "patch.rs",
        "lenia.rs",
        "isohedral/mod.rs",
        "isohedral/data.rs",
        "window.rs",
        "hunt/soup.rs",
        "pattern/site_graph.rs",
        "pile/mod.rs",
    ];
    let banned = [
        ".sin()",
        ".cos()",
        ".tan()",
        "sin_cos()",
        ".hypot(",
        ".atan2(",
        ".atan()",
        ".asin()",
        ".acos()",
        ".exp()",
        ".ln()",
        ".powf(",
        ".powi(",
        ".cbrt()",
    ];
    let mut bad = vec![];
    for f in files {
        let src = std::fs::read_to_string(format!("{dir}{f}")).unwrap();
        // Test modules may compare with the platform.
        let body = src.split("#[cfg(test)]").next().unwrap();
        for (n, line) in body.lines().enumerate() {
            let code = line.split("//").next().unwrap();
            for b in banned {
                if code.contains(b) {
                    bad.push(format!("{f}:{}: {}", n + 1, line.trim()));
                }
            }
        }
    }
    assert!(
        bad.is_empty(),
        "platform maths in geometry:\n{}",
        bad.join("\n")
    );
}
