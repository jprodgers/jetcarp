//! Kaplan's isohedral tiling types IH01–IH93: the 81 that his tactile library supports (the other
//! 12 need straight edges in a special arrangement and add nothing to edit).
//!
//! Each type is a reshapeable family with id `ih01` … `ih93` (`families::family` finds them; they
//! are kept out of `FAMILIES` so the flat family lists stay short). Parameters (`{"p": [...]}`)
//! move the tiling vertices; tactile's aspects and translation vectors place the tiles of one unit
//! cell; each edge class is one of tactile's edge shapes (J free, U mirror, S half-turn,
//! I straight) and every edge says how it places that shape (`edges::Orient`). The lattice, the
//! neighbour table (measured from contact), storage, rendering, and CAs are the same as for the
//! other reshapeable families.
//!
//! Data tables: tactile-js, Copyright (c) 2018 Craig S. Kaplan, BSD-3-Clause
//! (`LICENSE-tactile` in this folder), converted by `tools/gen_isohedral.py`.

mod data;
pub mod sheet;

pub use data::{IH_FAMILIES, IH_TYPES};

use crate::families::Family;
use crate::geom::*;
use serde_json::{Value, json};
use std::sync::OnceLock;

/// One isohedral type's tactile data. Coefficient rows have `np + 1` entries (the last multiplies
/// 1): vertices are `nv` points, translations 2 points, aspects `na` row-major 2×3 matrices.
#[derive(Debug)]
pub struct IhType {
    pub num: u8,
    pub np: usize,
    pub na: usize,
    pub nv: usize,
    pub defaults: &'static [f64],
    pub vertex: &'static [f64],
    pub translation: &'static [f64],
    pub aspect: &'static [f64],
}

impl IhType {
    fn point(&self, c: &[f64], off: usize, p: &[f64]) -> P {
        let n = self.np + 1;
        let at = |o: usize, k: usize| if k < self.np { p[k] } else { 1.0 } * c[o + k];
        [
            (0..n).map(|k| at(off, k)).sum(),
            (0..n).map(|k| at(off + n, k)).sum(),
        ]
    }
    /// Tiling vertices (the prototile's corners) for parameters `p` (length `np`).
    pub fn vertices(&self, p: &[f64]) -> Vec<P> {
        (0..self.nv)
            .map(|i| self.point(self.vertex, i * 2 * (self.np + 1), p))
            .collect()
    }
    /// Translation vectors of the unit cell.
    pub fn translations(&self, p: &[f64]) -> (P, P) {
        (
            self.point(self.translation, 0, p),
            self.point(self.translation, 2 * (self.np + 1), p),
        )
    }
    /// Transforms placing the prototile in one unit cell.
    pub fn aspects(&self, p: &[f64]) -> Vec<Affine> {
        let n = self.np + 1;
        (0..self.na)
            .map(|a| {
                let mut m = [0.0; 6];
                for (r, v) in m.iter_mut().enumerate() {
                    let off = 6 * n * a + r * n;
                    *v = (0..n)
                        .map(|k| self.aspect[off + k] * if k < self.np { p[k] } else { 1.0 })
                        .sum();
                }
                Affine(m)
            })
            .collect()
    }
    /// How vertex `i` moves per unit of each parameter: (d x / d p_k, d y / d p_k).
    pub fn jacobian(&self, i: usize) -> Vec<P> {
        let off = i * 2 * (self.np + 1);
        (0..self.np)
            .map(|k| [self.vertex[off + k], self.vertex[off + self.np + 1 + k]])
            .collect()
    }
}

/// Position of an `ihNN` id in `IH_TYPES`.
pub fn index_of(id: &str) -> Option<usize> {
    let n: u8 = id.strip_prefix("ih")?.parse().ok()?;
    IH_TYPES.iter().position(|t| t.num == n)
}

pub fn family(id: &str) -> Option<&'static Family> {
    index_of(id).map(|i| &IH_FAMILIES[i])
}

/// Family id for a type number (`41` → `ih41`), if tactile supports it.
pub fn id_for(num: u32) -> Option<&'static str> {
    IH_TYPES
        .iter()
        .position(|t| t.num as u32 == num)
        .map(|i| IH_FAMILIES[i].id)
}

/// Parameter values from tile JSON (`{"p": [...]}`); missing means tactile's defaults.
pub fn params_of(t: &IhType, v: &Value) -> Result<Vec<f64>, String> {
    let Some(arr) = v.get("p") else {
        return Ok(t.defaults.to_vec());
    };
    let arr = arr
        .as_array()
        .ok_or("isohedral parameter 'p' must be a list of numbers")?;
    let p: Option<Vec<f64>> = arr.iter().map(|x| x.as_f64()).collect();
    let p = p.ok_or("isohedral parameter 'p' must be a list of numbers")?;
    if p.len() != t.np {
        return Err(format!(
            "IH{:02} takes {} parameters, got {}",
            t.num,
            t.np,
            p.len()
        ));
    }
    if p.iter().any(|x| !x.is_finite()) {
        return Err("isohedral parameters must be finite".into());
    }
    Ok(p)
}

pub fn init_params(id: &str) -> Option<Value> {
    index_of(id).map(|i| json!({ "p": IH_TYPES[i].defaults }))
}

pub fn corners(id: &str, v: &Value) -> Option<Result<Vec<P>, String>> {
    let t = &IH_TYPES[index_of(id)?];
    Some(params_of(t, v).map(|p| t.vertices(&p)))
}

/// The unit cell: aspects (prototile → tile), translation vectors, and the prototile's corners.
pub type Cell = (Vec<Affine>, P, P, Vec<P>);

pub fn cell(id: &str, v: &Value) -> Option<Result<Cell, String>> {
    let t = &IH_TYPES[index_of(id)?];
    Some(params_of(t, v).map(|p| {
        let (t1, t2) = t.translations(&p);
        (t.aspects(&p), t1, t2, t.vertices(&p))
    }))
}

/// Corners that move with the parameters (the editor's drag handles).
pub fn handles(id: &str) -> Option<&'static [usize]> {
    static H: OnceLock<Vec<Vec<usize>>> = OnceLock::new();
    let i = index_of(id)?;
    let all = H.get_or_init(|| {
        IH_TYPES
            .iter()
            .map(|t| {
                (0..t.nv)
                    .filter(|&c| t.jacobian(c).iter().any(|d| d[0] != 0.0 || d[1] != 0.0))
                    .collect()
            })
            .collect()
    });
    Some(&all[i])
}

/// Move corner `corner` towards `pt` with the smallest parameter change (least squares).
/// Returns false when `id` is not an isohedral type.
pub fn set_handle(id: &str, v: &mut Value, corner: usize, pt: P) -> bool {
    let Some(i) = index_of(id) else {
        return false;
    };
    let t = &IH_TYPES[i];
    if corner >= t.nv || t.np == 0 {
        return true;
    }
    let Ok(mut p) = params_of(t, v) else {
        return true;
    };
    let r = sub(pt, t.vertices(&p)[corner]);
    let jac = t.jacobian(corner);
    // dp = J^T (J J^T)^+ r, with the 2×2 pseudo-inverse from its eigen-decomposition.
    let (mut a, mut b, mut d) = (0.0, 0.0, 0.0);
    for g in &jac {
        a += g[0] * g[0];
        b += g[0] * g[1];
        d += g[1] * g[1];
    }
    let tr = a + d;
    if tr < 1e-12 {
        return true;
    }
    let mut y = [0.0, 0.0];
    if b.abs() <= 1e-12 * tr {
        for (k, m) in [a, d].into_iter().enumerate() {
            if m > 1e-9 * tr {
                y[k] = r[k] / m;
            }
        }
    } else {
        let disc = (((a - d) / 2.0) * ((a - d) / 2.0) + b * b).sqrt();
        for lam in [tr / 2.0 + disc, tr / 2.0 - disc] {
            if lam > 1e-9 * tr {
                let e = [lam - d, b];
                let e = scale(e, 1.0 / len(e));
                y = add(y, scale(e, dot(e, r) / lam));
            }
        }
    }
    for (k, g) in jac.iter().enumerate() {
        p[k] += dot(*g, y);
    }
    v["p"] = json!(p);
    true
}

/// Tiles around the prototile (as drawn at its corners), for the editor's preview.
pub fn ghosts(id: &str, v: &Value) -> Option<Result<Vec<Affine>, String>> {
    let (asp, t1, t2, corners) = match cell(id, v)? {
        Ok(c) => c,
        Err(e) => return Some(Err(e)),
    };
    let base = asp[0].inv();
    let c = centroid(&corners);
    let diam = corners.iter().map(|&q| dist(q, c)).fold(0.0, f64::max) * 2.0;
    let mut out: Vec<Affine> = Vec::new();
    for i in -4..=4 {
        for j in -4..=4 {
            let tr = add(scale(t1, i as f64), scale(t2, j as f64));
            for m in &asp {
                let g = Affine::trans(tr[0], tr[1]).mul(m).mul(&base);
                let gc = g.apply(c);
                if dist(gc, c) < 1e-6 * diam.max(1e-9) || dist(gc, c) > 1.6 * diam {
                    continue;
                }
                out.push(g);
            }
        }
    }
    Some(Ok(out))
}
