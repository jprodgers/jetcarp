//! Geometry pins: a short hash of the geometry a record was verified on, so a library can tell
//! when the engine's geometry has changed under it.
//!
//! A record (a pattern, an ant run, a Quick Start scene) means something only on the tiles it
//! ran on. When the engine's geometry changes (CART's portable-geometry change re-ordered the
//! neighbours on 1,065 of 1,366 tilings, and 187,512 stored patterns silently stopped
//! reproducing), the record must be checked again. So every record stores the **pin** of its
//! tiling, and is part of its content id; the library compares stored pins with the running
//! engine's and flags the records whose pin differs (`needs_reverify`).
//!
//! **What a pin hashes.** It must not depend on the grid size a record happened to be built on:
//!
//! - *Periodic tilings* (every lattice, from the square grid to the k-uniform tilings): the
//!   unit cell. The polygon hash covers the lattice vectors `a1`, `a2`, then for each tile type
//!   of the unit cell its shape index and its polygon's corners (placed in the unit cell); the
//!   neighbour hash covers the offset table (for each neighbourhood, edge then touch, and each
//!   tile type: its slots in order, each `(di, dj, k)`). Floats are hashed by their exact bits.
//! - *Aperiodic tilings* (hat, Spectre, Penrose): there is no unit cell, so the pin hashes one
//!   fixed **reference patch** of the family ([`REFERENCE_LEVELS`]: level 4 of each, 4,000 to
//!   4,401 tiles), whatever level the record was found on: every tile's shape index and corners,
//!   in tile order, then each neighbourhood's neighbour lists and the open-boundary flags. The
//!   level is the size of an aperiodic patch, so it is no more part of the pin than a lattice
//!   grid's size: a pattern's identity on these tilings is already free of its position (a
//!   pattern graph or a site), the same pattern found on two levels is one pattern, and the
//!   record keeps the patch it was found on (its site) for re-verification. Level 4 is the
//!   smallest level that holds every local configuration of the next level (each tile's area,
//!   handedness relative to the patch, and neighbour counts, with its neighbours' and theirs, two
//!   steps out, away from the patch's edge), for all four families; level 3 holds fewer (a test
//!   checks both). A change to any tile's geometry or neighbours therefore shows in the pin. The
//!   levels are fixed here, not taken from the families' default levels, so changing a default
//!   never changes a pin.
//!
//! The bytes hashed, in order (integers little-endian, `u32` unless marked, floats as `u64`
//! bits):
//!
//! ```text
//! polygon_hash   = SHA-256("jetcarp-geometry-v1 polygons\n" + body)
//!   periodic:  "lattice" a1.x a1.y a2.x a2.y ntypes, then per tile type: shape, n, n × (x, y)
//!   aperiodic: "patch" cells, then per tile: shape, n, n × (x, y), on the reference patch
//! neighbour_hash = SHA-256("jetcarp-geometry-v1 neighbours\n" + body)
//!   periodic:  for nb in [edge, touch], per tile type: slots, slots × (di i32, dj i32, k)
//!   aperiodic: for nb in [edge, touch], per tile: degree, degree × neighbour; then per tile: open u8
//! pin            = first 32 hex digits of SHA-256("jetcarp-geometry-v1\n" + polygon_hash
//!                  + "\n" + neighbour_hash), the two hashes as 64 hex digits each
//! ```
//!
//! `tests/geometry_pins.rs` keeps its own FNV hashes of every tiling at its default size as the
//! engine's regression test; pins are what records carry.

use crate::families::{self, FamilyKind};
use crate::sha256::{Sha256, hex};
use crate::topology::{Boundary, Nb, SizeSpec, Storage, TilingSpec, Topology};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

/// Prefix of the hashed text (bump it if what a pin covers ever changes).
pub const PIN_VERSION: &str = "jetcarp-geometry-v1";

/// A tiling's geometry pin, with the two hashes it is made of (a library keeps them per tiling
/// so a changed pin can say whether the polygons or the neighbours changed).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GeometryPin {
    /// What the pin is the pin of ([`pin_key`]): a library's `engine_pins` key.
    pub key: String,
    /// 32 hex digits.
    pub pin: String,
    /// 64 hex digits each.
    pub polygon_hash: String,
    pub neighbour_hash: String,
}

/// Is this tiling family aperiodic (pinned by its reference patch, not a unit cell)?
pub fn is_aperiodic(t: &TilingSpec) -> bool {
    families::family(&t.family).is_some_and(|f| f.kind == FamilyKind::Aperiodic)
}

/// The level of the patch each aperiodic family is pinned on (see the module notes). Fixed: a
/// change here changes every pin on that family, and so every id.
pub const REFERENCE_LEVELS: [(&str, u32); 4] = [
    ("hat", 4),
    ("spectre", 4),
    ("penrose-kd", 4),
    ("penrose-rhomb", 4),
];

/// The reference level of an aperiodic family (level 4 for one not listed).
pub fn reference_level(family: &str) -> u32 {
    REFERENCE_LEVELS
        .iter()
        .find(|(f, _)| *f == family)
        .map(|x| x.1)
        .unwrap_or(4)
}

/// The spec a pin is computed from: the tiling's identity without an aperiodic family's level
/// ([`crate::pattern::identity_tiling`]), and for aperiodic families the reference level as its
/// size.
pub fn pin_spec(t: &TilingSpec) -> TilingSpec {
    let mut s = crate::pattern::identity_tiling(t);
    if is_aperiodic(t) {
        s.size = Some(SizeSpec::Level(reference_level(&t.family)));
    }
    s
}

/// The key of a tiling's pin, a library's `engine_pins` key: the tiling's identity key
/// ([`crate::pattern::identity_key`]: the tiling key, without an aperiodic family's level).
pub fn pin_key(t: &TilingSpec) -> String {
    crate::pattern::identity_key(t)
}

/// The reference patches built for pins in this process (their specs as JSON), for tests and
/// for knowing what opening a library costs.
pub fn built_patches() -> Vec<String> {
    BUILT.lock().unwrap().clone()
}

static BUILT: Mutex<Vec<String>> = Mutex::new(Vec::new());

fn f(h: &mut Sha256, x: f64) {
    h.update(&x.to_bits().to_le_bytes());
}
fn u(h: &mut Sha256, x: u32) {
    h.update(&x.to_le_bytes());
}
fn i(h: &mut Sha256, x: i32) {
    h.update(&x.to_le_bytes());
}

/// The pin of a built topology (periodic: its unit cell; explicit: every tile of the patch).
pub fn pin_of(topo: &Topology, key: String) -> GeometryPin {
    let mut ph = Sha256::new();
    ph.text(PIN_VERSION);
    ph.text(" polygons\n");
    let mut nh = Sha256::new();
    nh.text(PIN_VERSION);
    nh.text(" neighbours\n");
    match &topo.storage {
        Storage::Lattice(g) => {
            ph.text("lattice");
            for x in [g.a1[0], g.a1[1], g.a2[0], g.a2[1]] {
                f(&mut ph, x);
            }
            u(&mut ph, g.ntypes);
            for (m, s) in &g.types {
                let poly = &topo.shapes[*s];
                u(&mut ph, *s as u32);
                u(&mut ph, poly.len() as u32);
                for &p in poly {
                    let q = m.apply(p);
                    f(&mut ph, q[0]);
                    f(&mut ph, q[1]);
                }
            }
            for nb in [Nb::Edge, Nb::Touch] {
                for slots in &g.offsets[nb as usize] {
                    u(&mut nh, slots.len() as u32);
                    for o in slots {
                        i(&mut nh, o.di);
                        i(&mut nh, o.dj);
                        u(&mut nh, o.k);
                    }
                }
            }
        }
        Storage::Explicit(_) => {
            ph.text("patch");
            u(&mut ph, topo.len() as u32);
            for x in 0..topo.len() {
                let poly = topo.polygon(x);
                u(&mut ph, topo.shape_of(x) as u32);
                u(&mut ph, poly.len() as u32);
                for p in poly {
                    f(&mut ph, p[0]);
                    f(&mut ph, p[1]);
                }
            }
            for nb in [Nb::Edge, Nb::Touch] {
                for x in 0..topo.len() {
                    u(&mut nh, topo.degree(x, nb) as u32);
                    topo.for_each_neighbor(x, nb, |y| u(&mut nh, y as u32));
                }
            }
            for x in 0..topo.len() {
                nh.update(&[topo.is_open(x) as u8]);
            }
        }
    }
    let polygon_hash = ph.finish_hex();
    let neighbour_hash = nh.finish_hex();
    let mut h = Sha256::new();
    h.text(PIN_VERSION);
    h.text("\n");
    h.text(&polygon_hash);
    h.text("\n");
    h.text(&neighbour_hash);
    let pin = hex(&h.finish()[..16]);
    GeometryPin {
        key,
        pin,
        polygon_hash,
        neighbour_hash,
    }
}

/// The geometry pin of a tiling (see the module notes), computed once per [`pin_key`] in this
/// process. Periodic tilings are built on a small grid (the pin covers only the unit cell);
/// aperiodic ones as their family's reference patch, whatever level the spec names.
pub fn geometry_pin(t: &TilingSpec) -> Result<GeometryPin, String> {
    static CACHE: OnceLock<Mutex<HashMap<String, GeometryPin>>> = OnceLock::new();
    let key = pin_key(t);
    let cache = CACHE.get_or_init(Default::default);
    if let Some(p) = cache.lock().unwrap().get(&key) {
        return Ok(p.clone());
    }
    let mut spec = pin_spec(t);
    let pin = if is_aperiodic(&spec) {
        // Built and dropped here (not kept in the patch cache): a pin is all that is needed.
        let topo = Topology::build(&spec)?;
        BUILT
            .lock()
            .unwrap()
            .push(serde_json::to_string(&spec).unwrap_or_default());
        pin_of(&topo, key.clone())
    } else {
        spec.size = Some(SizeSpec::Rect { w: 8, h: 8 });
        spec.boundary = Boundary::Dead;
        pin_of(&Topology::build(&spec)?, key.clone())
    };
    cache.lock().unwrap().insert(key, pin.clone());
    Ok(pin)
}

/// What the engine's geometry is, as one string a library compares with the one its
/// `engine_pins` were computed by: the JETCARP version and a hash of the geometry pins this
/// build was tested against (`tests/data/geometry_pins.txt`, which changes exactly when the
/// geometry is deliberately changed and re-pinned). A library whose cached pins were computed by
/// another engine computes them again when it is opened.
pub fn engine_geometry() -> &'static str {
    static E: OnceLock<String> = OnceLock::new();
    E.get_or_init(|| {
        let pins = include_str!("../tests/data/geometry_pins.txt");
        format!(
            "{}+g{}",
            crate::VERSION,
            &crate::sha256::sha256_hex(pins.as_bytes())[..12]
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    // A periodic pin is the unit cell's: the same on any grid size, boundary, or size spec, and
    // different between tilings and between a reshaped tile and the default one.
    #[test]
    fn periodic_pins_ignore_the_grid() {
        let base = |fam: &str| geometry_pin(&TilingSpec::new(fam)).unwrap();
        for fam in ["square", "hex", "oct", "trihex"] {
            let p = base(fam);
            assert_eq!(p.pin.len(), 32);
            assert_eq!(p.key, crate::pattern::tiling_key(&TilingSpec::new(fam)));
            for spec in [
                TilingSpec::rect(fam, 64, 40, Boundary::Wrap),
                TilingSpec::rect(fam, 9, 300, Boundary::Live),
                TilingSpec::new(fam).with_size(SizeSpec::Cells(5000)),
            ] {
                // Built directly (not from the cache), on that grid.
                let topo = Topology::build(&spec).unwrap();
                assert_eq!(pin_of(&topo, pin_key(&spec)), p, "{fam} {:?}", spec.size);
            }
        }
        let mut k = TilingSpec::new("periodic");
        k.params = Some(serde_json::json!({ "catalog": "k3-019" }));
        let a = geometry_pin(&k).unwrap();
        let mut big = k.clone();
        big.size = Some(SizeSpec::Rect { w: 50, h: 20 });
        assert_eq!(pin_of(&Topology::build(&big).unwrap(), pin_key(&big)), a);
        let pins: std::collections::HashSet<String> = ["square", "hex", "oct", "trihex"]
            .iter()
            .map(|f| base(f).pin)
            .chain([a.pin.clone()])
            .collect();
        assert_eq!(pins.len(), 5);
    }

    // An aperiodic pin is its family's reference patch's: the same for every level, given as a
    // size or as a parameter, and building the pin of a level-7 spec builds only the reference.
    #[test]
    fn aperiodic_pins_ignore_the_level() {
        for fam in ["hat", "spectre", "penrose-kd", "penrose-rhomb"] {
            let at = |n| TilingSpec::new(fam).with_size(SizeSpec::Level(n));
            let mut param = TilingSpec::new(fam);
            param.params = Some(serde_json::json!({ "level": 5 }));
            let p = geometry_pin(&at(3)).unwrap();
            for spec in [at(4), at(5), at(7), param, TilingSpec::new(fam)] {
                assert_eq!(geometry_pin(&spec).unwrap(), p, "{fam} {spec:?}");
            }
            assert!(!p.key.contains("level"), "{}", p.key);
            // The reference patch built from scratch has that pin.
            let r = TilingSpec::new(fam).with_size(SizeSpec::Level(reference_level(fam)));
            assert_eq!(pin_of(&Topology::build(&r).unwrap(), p.key.clone()), p);
        }
        // Only reference patches were built for pins (never level 7's millions of tiles).
        for b in built_patches() {
            assert!(b.contains("\"level\":4"), "{b}");
        }
        // Every aperiodic family has its reference level listed.
        for f in families::FAMILIES {
            if f.kind == FamilyKind::Aperiodic {
                assert!(REFERENCE_LEVELS.iter().any(|x| x.0 == f.id), "{}", f.id);
            }
        }
    }

    /// The local configurations of a patch away from its edge: each tile's area, handedness and
    /// neighbour counts with those of its neighbours, and theirs, to two steps.
    fn configurations(topo: &Topology) -> std::collections::BTreeSet<String> {
        let n = topo.len();
        let mut dist = vec![u32::MAX; n];
        let mut q = std::collections::VecDeque::new();
        for x in (0..n).filter(|&x| topo.is_open(x)) {
            dist[x] = 0;
            q.push_back(x);
        }
        while let Some(x) = q.pop_front() {
            let d = dist[x];
            topo.for_each_neighbor(x, Nb::Touch, |y| {
                if dist[y] == u32::MAX {
                    dist[y] = d + 1;
                    q.push_back(y);
                }
            });
        }
        // Handedness relative to most of the patch (a Spectre patch is the mirror image of the
        // level before it).
        let mirrored = (0..n).filter(|&x| topo.transform(x).det() < 0.0).count() * 2 > n;
        let sig = |y: usize| {
            let p = topo.polygon(y);
            let m = p.len();
            let a: f64 = (0..m)
                .map(|i| p[i][0] * p[(i + 1) % m][1] - p[(i + 1) % m][0] * p[i][1])
                .sum();
            format!(
                "{}:{}:{}:{}",
                (a.abs() * 500.0).round(),
                (topo.transform(y).det() < 0.0) != mirrored,
                topo.degree(y, Nb::Touch),
                topo.degree(y, Nb::Edge)
            )
        };
        let nbrs = |y: usize| {
            let mut v = vec![];
            topo.for_each_neighbor(y, Nb::Touch, |z| v.push(z));
            v
        };
        (0..n)
            .filter(|&x| dist[x] >= 4)
            .map(|x| {
                let mut ring: Vec<String> = nbrs(x)
                    .into_iter()
                    .map(|y| {
                        let mut v: Vec<String> = nbrs(y).into_iter().map(sig).collect();
                        v.sort();
                        format!("{}[{}]", sig(y), v.join(","))
                    })
                    .collect();
                ring.sort();
                format!("{} {}", sig(x), ring.join(" "))
            })
            .collect()
    }

    // The reference patch holds every local configuration (to two steps) of the next level, so a
    // change to any tile's geometry or neighbours shows in the pin; the level below does not.
    #[test]
    fn reference_patches_hold_every_local_configuration() {
        for (fam, level) in REFERENCE_LEVELS {
            let at = |l| Topology::build(&TilingSpec::new(fam).with_size(SizeSpec::Level(l)));
            let reference = configurations(&at(level).unwrap());
            let next = configurations(&at(level + 1).unwrap());
            let below = configurations(&at(level - 1).unwrap());
            let missing: Vec<&String> = next.difference(&reference).collect();
            assert!(
                missing.is_empty(),
                "{fam}: level {level} misses {} of {}, e.g. {:?}",
                missing.len(),
                next.len(),
                &missing[..missing.len().min(2)]
            );
            assert!(
                below.len() < reference.len(),
                "{fam}: level {} would do",
                level - 1
            );
        }
    }
}
