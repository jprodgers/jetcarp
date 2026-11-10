//! Exact unit cells for tilings whose corners lie in Z[ω], ω = e^{iπ/6} (every edge-to-edge
//! tiling by unit regular polygons except those with octagons). A point is an integer 4-vector
//! in the basis 1, ω, ω², ω³ (ω⁴ = ω² − 1), the representation of Soto Sánchez et al. (2021).
//! Catalog files store cells this way (`"z": {a1, a2, p}`) and they decode to the same
//! [`CatalogTiling`] as plain-coordinate entries.

use crate::catalog::CatalogTiling;
use crate::geom::P;
use serde::Deserialize;

pub type Z4 = [i64; 4];

const H: f64 = 0.866_025_403_784_438_6; // √3 / 2

/// The point Σ zₖ ωᵏ in the plane.
pub fn point(z: Z4) -> P {
    let [a, b, c, d] = z.map(|v| v as f64);
    [a + b * H + c * 0.5, b * 0.5 + c * H + d]
}

/// ω · z.
pub fn wmul(z: Z4) -> Z4 {
    let [a, b, c, d] = z;
    [-d, a, b + d, c]
}

/// ωᵏ for k mod 12.
pub fn unit(k: i64) -> Z4 {
    let mut z = [1, 0, 0, 0];
    for _ in 0..k.rem_euclid(12) {
        z = wmul(z);
    }
    z
}

/// Corners of the regular unit n-gon with a corner at `x` and first edge along ω^dir,
/// counterclockwise. `n` must divide 12.
pub fn polygon(x: Z4, dir: i64, n: i64) -> Vec<P> {
    let step = 12 / n;
    let mut p = x;
    let mut out = Vec::with_capacity(n as usize);
    for s in 0..n {
        out.push(point(p));
        let u = unit(dir + s * step);
        for (pi, ui) in p.iter_mut().zip(u) {
            *pi += ui;
        }
    }
    out
}

#[derive(Clone, Debug, Deserialize)]
pub struct ExactCell {
    pub a1: Z4,
    pub a2: Z4,
    /// Polygons as `[x0, x1, x2, x3, dir, n]`.
    pub p: Vec<[i64; 6]>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct ExactTiling {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub group: String,
    #[serde(default)]
    pub k: u32,
    #[serde(default)]
    pub note: String,
    pub z: ExactCell,
}

/// One catalog record: plain coordinates or an exact Z[ω] cell.
#[derive(Clone, Debug, Deserialize)]
#[serde(untagged)]
pub enum Entry {
    Exact(ExactTiling),
    Plain(CatalogTiling),
}

impl Entry {
    pub fn into_tiling(self) -> CatalogTiling {
        match self {
            Entry::Plain(t) => t,
            Entry::Exact(t) => CatalogTiling {
                id: t.id,
                name: t.name,
                group: t.group,
                k: t.k,
                note: t.note,
                a1: point(t.z.a1),
                a2: point(t.z.a2),
                polys: t
                    .z
                    .p
                    .iter()
                    .map(|q| polygon([q[0], q[1], q[2], q[3]], q[4], q[5]))
                    .collect(),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn units_are_unit_and_twelve_cycle() {
        for k in 0..12 {
            let p = point(unit(k));
            let a = k as f64 * std::f64::consts::PI / 6.0;
            assert!((p[0] - a.cos()).abs() < 1e-12 && (p[1] - a.sin()).abs() < 1e-12);
        }
        assert_eq!(unit(12), [1, 0, 0, 0]);
        let hex = polygon([0, 0, 0, 0], 0, 6);
        assert!((hex[3][0] - 1.0).abs() < 1e-12 && (hex[3][1] - 2.0 * H).abs() < 1e-12);
    }
}
