//! The whole symmetry group of a lattice tiling, as isometries of the plane, for unit cells that
//! hold their tile pattern more than once.
//!
//! A cell map ([`super::symmetry::CellMap`]) writes a symmetry on lattice cells `(i, j, k)` with
//! an integer matrix, so it can only be a symmetry whose rotation part maps the unit cell's
//! lattice `L` onto itself. When the cell is primitive (its lattice is every translation of the
//! tiling) that is every symmetry. When the cell holds the tile pattern twice or more (flat-top
//! hexagons in a 3 x sqrt 3 rectangle, the squares of IH58 in a 1 x 2 cell), the tiling's true
//! translation lattice `L'` is finer than `L`, and a rotation of `L'` need not map `L` onto
//! itself: the quarter turn of IH58's squares maps the cell vector `(1, 0)` to `(0, 1)`, half a
//! cell. Such symmetries are written here as isometries `x -> R x + t` and applied to a cell
//! through its centroid, which lands on the centroid of the image tile.
//!
//! **The group, and why it is complete.** The translations inside a cell are the cell maps with
//! the identity matrix (the symmetry search finds every one: [`super::symmetry`]); with the cell
//! vectors they generate `L'`. A basis of `L'` is its two shortest independent vectors (in the
//! plane the successive minima of a lattice form a basis), found among the lattice vectors no
//! longer than the longer cell vector (and both cell vectors are in `L'`). Every symmetry's
//! rotation part maps `L'` onto itself (it conjugates translations to translations), so it is one
//! of the isometries of that lattice ([`super::symmetry::lattice_isometries`], a complete
//! search); every symmetry, after a translation of the tiling, carries tile 0 of the cell to a
//! tile type of the cell; and a candidate is kept when every tile of the cell lands exactly on a
//! tile. So [`TilingGroup::isos`] holds every symmetry modulo the cell's lattice translations.
//! Geometric symmetries keep contacts, so they keep both neighbourhoods.

use super::symmetry::lattice_isometries;
use crate::geom::{P, centroid};
use crate::objects::LCell;
use crate::topology::{Boundary, Nb, SizeSpec, TilingSpec, Topology};
use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

/// An isometry of the plane, `x -> r x + t` (`r` row-major).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Iso {
    pub r: [f64; 4],
    pub t: P,
}

impl Iso {
    pub const ID: Iso = Iso {
        r: [1.0, 0.0, 0.0, 1.0],
        t: [0.0, 0.0],
    };
    /// The image of a point.
    pub fn apply(&self, x: P) -> P {
        let v = self.turn(x);
        [v[0] + self.t[0], v[1] + self.t[1]]
    }
    /// The image of a vector (the rotation part only).
    pub fn turn(&self, v: P) -> P {
        let r = &self.r;
        [r[0] * v[0] + r[1] * v[1], r[2] * v[0] + r[3] * v[1]]
    }
}

/// Every symmetry of a lattice tiling (see the module notes).
#[derive(Debug)]
pub struct TilingGroup {
    pub a1: P,
    pub a2: P,
    pub ntypes: u32,
    det: f64,
    /// Tile type centroids in cell (0, 0).
    cents: Vec<P>,
    /// The translations of the tiling inside a cell, as world vectors, the zero vector first:
    /// one entry means the cell is primitive.
    pub taus: Vec<P>,
    /// Every symmetry modulo the cell's lattice translations, the identity first (empty on a
    /// primitive cell, where the cell maps are the whole group).
    pub isos: Vec<Iso>,
}

const EPS: f64 = 1e-5;

fn near_int(x: f64) -> bool {
    (x - x.round()).abs() < EPS
}

impl TilingGroup {
    /// The cell holds its tile pattern once: the cell maps are every symmetry.
    pub fn primitive(&self) -> bool {
        self.taus.len() == 1
    }
    fn to_lattice(&self, p: P) -> [f64; 2] {
        let (a1, a2) = (self.a1, self.a2);
        [
            (p[0] * a2[1] - p[1] * a2[0]) / self.det,
            (a1[0] * p[1] - a1[1] * p[0]) / self.det,
        ]
    }
    /// A cell-lattice vector as a world vector.
    pub fn world(&self, d: [i32; 2]) -> P {
        [
            d[0] as f64 * self.a1[0] + d[1] as f64 * self.a2[0],
            d[0] as f64 * self.a1[1] + d[1] as f64 * self.a2[1],
        ]
    }
    /// A world vector as a cell-lattice vector, if it is one.
    pub fn lattice(&self, v: P) -> Option<[i32; 2]> {
        let l = self.to_lattice(v);
        (near_int(l[0]) && near_int(l[1])).then(|| [l[0].round() as i32, l[1].round() as i32])
    }
    /// Is this world vector a translation of the tiling?
    pub fn is_translation(&self, v: P) -> bool {
        self.taus
            .iter()
            .any(|t| self.lattice([v[0] - t[0], v[1] - t[1]]).is_some())
    }
    /// The image of a cell under an isometry (`None` if it lands on no tile's centroid, which a
    /// symmetry never does).
    pub fn apply(&self, g: &Iso, c: &LCell) -> Option<LCell> {
        let b = self.world([c.i, c.j]);
        let x = self.cents.get(c.k as usize)?;
        let y = g.apply([b[0] + x[0], b[1] + x[1]]);
        self.cents.iter().enumerate().find_map(|(k2, c2)| {
            self.lattice([y[0] - c2[0], y[1] - c2[1]]).map(|l| LCell {
                i: l[0],
                j: l[1],
                k: k2 as u32,
                s: c.s,
            })
        })
    }
}

/// The symmetry group of a lattice tiling for a neighbourhood, computed once per tiling in this
/// process.
pub fn tiling_group(tiling: &TilingSpec, nb: Nb) -> Result<Arc<TilingGroup>, String> {
    static CACHE: OnceLock<Mutex<HashMap<String, Arc<TilingGroup>>>> = OnceLock::new();
    let key = format!("{}|{nb:?}", super::tiling_key(tiling));
    let cache = CACHE.get_or_init(Default::default);
    if let Some(g) = cache.lock().unwrap().get(&key) {
        return Ok(g.clone());
    }
    let grp = Arc::new(build(tiling, nb)?);
    cache.lock().unwrap().insert(key, grp.clone());
    Ok(grp)
}

fn build(tiling: &TilingSpec, nb: Nb) -> Result<TilingGroup, String> {
    let mut spec = super::tiling_identity(tiling);
    spec.size = Some(SizeSpec::Rect { w: 8, h: 8 });
    spec.boundary = Boundary::Dead;
    let topo = Topology::build(&spec)?;
    let g = topo
        .lattice()
        .ok_or_else(|| format!("{} is not a lattice tiling", tiling.family))?;
    let polys: Vec<Vec<P>> = g
        .types
        .iter()
        .map(|(m, s)| topo.shapes[*s].iter().map(|&p| m.apply(p)).collect())
        .collect();
    let cents: Vec<P> = polys.iter().map(|p| centroid(p)).collect();
    let (a1, a2) = (g.a1, g.a2);
    let det = a1[0] * a2[1] - a2[0] * a1[1];
    let mut grp = TilingGroup {
        a1,
        a2,
        ntypes: g.ntypes,
        det,
        cents,
        taus: vec![[0.0, 0.0]],
        isos: vec![],
    };
    // Translations inside the cell: the cell maps with the identity matrix.
    for m in super::canon::tiling_symmetries(tiling, nb, true)?.iter() {
        if m.m == [1, 0, 0, 1] && m.perm.iter().enumerate().any(|(k, &p)| p as usize != k) {
            let (k, o) = (m.perm[0] as usize, grp.world(m.off[0]));
            let t = [
                grp.cents[k][0] + o[0] - grp.cents[0][0],
                grp.cents[k][1] + o[1] - grp.cents[0][1],
            ];
            if !grp
                .taus
                .iter()
                .any(|x| grp.lattice([t[0] - x[0], t[1] - x[1]]).is_some())
            {
                grp.taus.push(t);
            }
        }
    }
    if grp.primitive() {
        return Ok(grp);
    }
    // A basis of the translation lattice: its two shortest independent vectors.
    let norm = |v: P| (v[0] * v[0] + v[1] * v[1]).sqrt();
    let (l1, l2) = (norm(a1), norm(a2));
    let reach = l1.max(l2);
    let k = (reach * reach / det.abs() + 1e-9).ceil() as i32 + 1;
    let mut vecs: Vec<P> = Vec::new();
    for i in -k..=k {
        for j in -k..=k {
            let w = grp.world([i, j]);
            for t in &grp.taus {
                let v = [w[0] + t[0], w[1] + t[1]];
                let n = norm(v);
                if n > EPS && n <= reach + EPS {
                    vecs.push(v);
                }
            }
        }
    }
    vecs.sort_by(|a, b| norm(*a).partial_cmp(&norm(*b)).unwrap());
    let b1 = *vecs.first().ok_or("no translation lattice")?;
    let b2 = *vecs
        .iter()
        .find(|v| (b1[0] * v[1] - b1[1] * v[0]).abs() > EPS * norm(b1) * norm(**v))
        .ok_or("no second translation")?;
    let bdet = b1[0] * b2[1] - b2[0] * b1[1];
    // Every isometry of that lattice, with every translation that takes tile 0 to a tile type.
    let tiles_land = |g: &Iso| -> bool {
        polys.iter().all(|poly| {
            let moved: Vec<P> = poly.iter().map(|&q| g.apply(q)).collect();
            let mc = centroid(&moved);
            polys.iter().zip(&grp.cents).any(|(p2, c2)| {
                let Some(l) = grp.lattice([mc[0] - c2[0], mc[1] - c2[1]]) else {
                    return false;
                };
                let s = grp.world(l);
                moved.len() == p2.len()
                    && moved.iter().all(|q| {
                        p2.iter().any(|v| {
                            (v[0] + s[0] - q[0]).abs() < EPS && (v[1] + s[1] - q[1]).abs() < EPS
                        })
                    })
            })
        })
    };
    let mut isos = vec![Iso::ID];
    for m in lattice_isometries(b1, b2) {
        // R = B M B^-1 (columns of B are b1, b2): the images of e1 and e2.
        let r = |p: P| -> P {
            let l = [
                (p[0] * b2[1] - p[1] * b2[0]) / bdet,
                (b1[0] * p[1] - b1[1] * p[0]) / bdet,
            ];
            let (x, y) = (
                m[0] as f64 * l[0] + m[1] as f64 * l[1],
                m[2] as f64 * l[0] + m[3] as f64 * l[1],
            );
            [x * b1[0] + y * b2[0], x * b1[1] + y * b2[1]]
        };
        let (e1, e2) = (r([1.0, 0.0]), r([0.0, 1.0]));
        let rm = [e1[0], e2[0], e1[1], e2[1]];
        for k0 in 0..grp.cents.len() {
            let rc0 = r(grp.cents[0]);
            let g = Iso {
                r: rm,
                t: [grp.cents[k0][0] - rc0[0], grp.cents[k0][1] - rc0[1]],
            };
            let known = isos.iter().any(|h: &Iso| {
                h.r.iter().zip(&g.r).all(|(x, y)| (x - y).abs() < EPS)
                    && grp.lattice([h.t[0] - g.t[0], h.t[1] - g.t[1]]).is_some()
            });
            if !known && tiles_land(&g) {
                isos.push(g);
            }
        }
    }
    grp.isos = isos;
    Ok(grp)
}
