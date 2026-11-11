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
//! - *Aperiodic tilings* (hat, Spectre, Penrose): there is no unit cell, and a record is tied
//!   to the patch it was found on, so the pin hashes that patch, named by its spec (family,
//!   parameters, edges and substitution level; [`pin_spec`]): every tile's shape index and
//!   corners, in tile order, then each neighbourhood's neighbour lists and the open-boundary
//!   flags. Two levels of one family are two patches with two pins.
//!
//! The bytes hashed, in order (integers little-endian, `u32` unless marked, floats as `u64`
//! bits):
//!
//! ```text
//! polygon_hash   = SHA-256("jetcarp-geometry-v1 polygons\n" + body)
//!   periodic:  "lattice" a1.x a1.y a2.x a2.y ntypes, then per tile type: shape, n, n × (x, y)
//!   aperiodic: "patch" cells, then per tile: shape, n, n × (x, y)
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

/// Is this tiling family aperiodic (pinned by its patch, not a unit cell)?
pub fn is_aperiodic(t: &TilingSpec) -> bool {
    families::family(&t.family).is_some_and(|f| f.kind == FamilyKind::Aperiodic)
}

/// The spec a pin is computed from: the tiling's identity (family, parameters, edges; no size,
/// dead edges) for periodic tilings; for aperiodic ones the identity with the patch's
/// substitution level (the spec's `level` size, else its `level` parameter, else the family's
/// default level, as [`Topology::build`] reads it).
pub fn pin_spec(t: &TilingSpec) -> TilingSpec {
    let mut s = crate::pattern::tiling_identity(t);
    if is_aperiodic(t) {
        let level = match t.size {
            Some(SizeSpec::Level(l)) => l,
            _ => t
                .params
                .as_ref()
                .and_then(|p| p.get("level"))
                .and_then(|v| v.as_u64())
                .map(|l| l as u32)
                .unwrap_or_else(|| crate::aperiodic::default_level(&t.family)),
        };
        s.size = Some(SizeSpec::Level(level));
    }
    s
}

/// The key of a tiling's pin: the JSON of [`pin_spec`]. For a periodic tiling this is the
/// tiling key the library uses everywhere ([`crate::pattern::tiling_key`]); for an aperiodic
/// one it also names the patch's level.
pub fn pin_key(t: &TilingSpec) -> String {
    serde_json::to_string(&pin_spec(t)).unwrap_or_default()
}

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
/// aperiodic ones as the patch their spec names.
pub fn geometry_pin(t: &TilingSpec) -> Result<GeometryPin, String> {
    static CACHE: OnceLock<Mutex<HashMap<String, GeometryPin>>> = OnceLock::new();
    let key = pin_key(t);
    let cache = CACHE.get_or_init(Default::default);
    if let Some(p) = cache.lock().unwrap().get(&key) {
        return Ok(p.clone());
    }
    let mut spec = pin_spec(t);
    let pin = if is_aperiodic(&spec) {
        let topo = crate::pattern::site::patch_topology(&spec)?;
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

    // An aperiodic pin names its patch: the level is part of it, and an explicit level equals
    // the default one when they agree.
    #[test]
    fn aperiodic_pins_name_the_patch() {
        let l = |n| TilingSpec::new("hat").with_size(SizeSpec::Level(n));
        let p3 = geometry_pin(&l(3)).unwrap();
        let p4 = geometry_pin(&l(4)).unwrap();
        assert_ne!(p3.pin, p4.pin);
        assert_ne!(p3.key, p4.key);
        let d = crate::aperiodic::default_level("hat");
        assert_eq!(
            geometry_pin(&TilingSpec::new("hat")).unwrap(),
            geometry_pin(&l(d)).unwrap()
        );
        assert!(p3.key.contains("\"level\":3"), "{}", p3.key);
    }
}
