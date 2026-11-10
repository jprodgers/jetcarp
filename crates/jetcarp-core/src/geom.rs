//! Plane geometry: points, affine maps, polygons.
//! Ported from JETCARP's JavaScript prototype, the CART tiling editor (2026) (the `mul`, `ap`, `inv`, `directMap`, `flipMap` helpers).

use serde::{Deserialize, Serialize};

pub type P = [f64; 2];

pub const S3: f64 = 1.732_050_807_568_877_2; // sqrt(3)

pub fn add(p: P, q: P) -> P {
    [p[0] + q[0], p[1] + q[1]]
}
pub fn sub(p: P, q: P) -> P {
    [p[0] - q[0], p[1] - q[1]]
}
pub fn scale(p: P, s: f64) -> P {
    [p[0] * s, p[1] * s]
}
pub fn dot(p: P, q: P) -> f64 {
    p[0] * q[0] + p[1] * q[1]
}
pub fn cross(p: P, q: P) -> f64 {
    p[0] * q[1] - p[1] * q[0]
}
pub fn len(p: P) -> f64 {
    crate::detmath::hypot(p[0], p[1])
}
pub fn dist(p: P, q: P) -> f64 {
    len(sub(p, q))
}
pub fn angle_of(v: P) -> f64 {
    crate::detmath::atan2(v[1], v[0])
}
/// Rotate `p` about `c` by `deg` degrees (positive = towards +y).
pub fn rot_about(p: P, c: P, deg: f64) -> P {
    let (s, co) = crate::detmath::sin_cos(deg.to_radians());
    let (x, y) = (p[0] - c[0], p[1] - c[1]);
    [c[0] + x * co - y * s, c[1] + x * s + y * co]
}

/// Affine map stored row-major as `[a, b, c, d, e, f]`: x' = a x + b y + c, y' = d x + e y + f.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Affine(pub [f64; 6]);

impl Affine {
    pub const ID: Affine = Affine([1.0, 0.0, 0.0, 0.0, 1.0, 0.0]);

    pub fn rot(rad: f64) -> Affine {
        let (s, c) = crate::detmath::sin_cos(rad);
        Affine([c, -s, 0.0, s, c, 0.0])
    }
    pub fn trans(x: f64, y: f64) -> Affine {
        Affine([1.0, 0.0, x, 0.0, 1.0, y])
    }
    /// `self ∘ b` (apply `b` first).
    pub fn mul(&self, b: &Affine) -> Affine {
        let a = &self.0;
        let b = &b.0;
        Affine([
            a[0] * b[0] + a[1] * b[3],
            a[0] * b[1] + a[1] * b[4],
            a[0] * b[2] + a[1] * b[5] + a[2],
            a[3] * b[0] + a[4] * b[3],
            a[3] * b[1] + a[4] * b[4],
            a[3] * b[2] + a[4] * b[5] + a[5],
        ])
    }
    pub fn apply(&self, p: P) -> P {
        let m = &self.0;
        [
            m[0] * p[0] + m[1] * p[1] + m[2],
            m[3] * p[0] + m[4] * p[1] + m[5],
        ]
    }
    /// Apply only the linear part.
    pub fn apply_vec(&self, p: P) -> P {
        let m = &self.0;
        [m[0] * p[0] + m[1] * p[1], m[3] * p[0] + m[4] * p[1]]
    }
    pub fn inv(&self) -> Affine {
        let m = &self.0;
        let det = m[0] * m[4] - m[1] * m[3];
        let (a, b, d, e) = (m[4] / det, -m[1] / det, -m[3] / det, m[0] / det);
        Affine([a, b, -(a * m[2] + b * m[5]), d, e, -(d * m[2] + e * m[5])])
    }
    pub fn det(&self) -> f64 {
        self.0[0] * self.0[4] - self.0[1] * self.0[3]
    }
    pub fn translation(&self) -> P {
        [self.0[2], self.0[5]]
    }
    pub fn with_translation(&self, t: P) -> Affine {
        let mut m = self.0;
        m[2] = t[0];
        m[5] = t[1];
        Affine(m)
    }
    pub fn linear(&self) -> [f64; 4] {
        [self.0[0], self.0[1], self.0[3], self.0[4]]
    }
}

/// Orientation-preserving map with p→s, q→r (a glue map between two edges).
pub fn direct_map(p: P, q: P, r: P, s: P) -> Affine {
    let th = angle_of(sub(r, s)) - angle_of(sub(q, p));
    let rm = Affine::rot(th);
    let o = rm.apply(p);
    Affine([rm.0[0], rm.0[1], s[0] - o[0], rm.0[3], rm.0[4], s[1] - o[1]])
}

/// Orientation-reversing map with p→r, q→s (a glide glue map).
pub fn flip_map(p: P, q: P, r: P, s: P) -> Affine {
    let th = angle_of(sub(s, r)) + angle_of(sub(q, p));
    let (n, c) = crate::detmath::sin_cos(th);
    let mut m = Affine([c, n, 0.0, n, -c, 0.0]);
    let o = m.apply(p);
    m.0[2] = r[0] - o[0];
    m.0[5] = r[1] - o[1];
    m
}

/// Vertex average, as the prototype's `polyCentroid`.
pub fn centroid(poly: &[P]) -> P {
    let n = poly.len() as f64;
    poly.iter()
        .fold([0.0, 0.0], |c, p| [c[0] + p[0] / n, c[1] + p[1] / n])
}

/// Shoelace sum (twice the signed area). Positive means clockwise on a y-down screen.
pub fn signed_area2(poly: &[P]) -> f64 {
    let n = poly.len();
    (0..n)
        .map(|i| {
            let (p, q) = (poly[i], poly[(i + 1) % n]);
            p[0] * q[1] - q[0] * p[1]
        })
        .sum()
}

pub fn area(poly: &[P]) -> f64 {
    signed_area2(poly).abs() / 2.0
}

pub fn bbox(poly: &[P]) -> [f64; 4] {
    let mut b = [
        f64::INFINITY,
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::NEG_INFINITY,
    ];
    for p in poly {
        b[0] = b[0].min(p[0]);
        b[1] = b[1].min(p[1]);
        b[2] = b[2].max(p[0]);
        b[3] = b[3].max(p[1]);
    }
    b
}

pub fn point_in_poly(pt: P, poly: &[P]) -> bool {
    let mut c = false;
    let n = poly.len();
    let mut j = n - 1;
    for i in 0..n {
        let (pi, pj) = (poly[i], poly[j]);
        if (pi[1] > pt[1]) != (pj[1] > pt[1])
            && pt[0] < (pj[0] - pi[0]) * (pt[1] - pi[1]) / (pj[1] - pi[1]) + pi[0]
        {
            c = !c;
        }
        j = i;
    }
    c
}

/// Ear-clipping triangulation of a simple polygon (either orientation).
/// Returns index triples into `poly`. Falls back to a fan if clipping gets stuck.
pub fn triangulate(poly: &[P]) -> Vec<[u32; 3]> {
    let n = poly.len();
    if n < 3 {
        return vec![];
    }
    let ccw = signed_area2(poly) > 0.0;
    let mut idx: Vec<usize> = (0..n).collect();
    let mut out = Vec::with_capacity(n - 2);
    let convex = |a: P, b: P, c: P| {
        let z = cross(sub(b, a), sub(c, b));
        if ccw { z > 1e-14 } else { z < -1e-14 }
    };
    let mut guard = 0usize;
    while idx.len() > 3 && guard < n * n {
        guard += 1;
        let m = idx.len();
        let mut clipped = false;
        for k in 0..m {
            let (ia, ib, ic) = (idx[(k + m - 1) % m], idx[k], idx[(k + 1) % m]);
            let (a, b, c) = (poly[ia], poly[ib], poly[ic]);
            if !convex(a, b, c) {
                continue;
            }
            let tri = [a, b, c];
            let blocked = idx.iter().any(|&j| {
                j != ia && j != ib && j != ic && {
                    let p = poly[j];
                    // Strict inside test against the ear triangle.
                    let s1 = cross(sub(b, a), sub(p, a));
                    let s2 = cross(sub(c, b), sub(p, b));
                    let s3 = cross(sub(a, c), sub(p, c));
                    let _ = tri;
                    if ccw {
                        s1 > 1e-12 && s2 > 1e-12 && s3 > 1e-12
                    } else {
                        s1 < -1e-12 && s2 < -1e-12 && s3 < -1e-12
                    }
                }
            });
            if blocked {
                continue;
            }
            out.push([ia as u32, ib as u32, ic as u32]);
            idx.remove(k);
            clipped = true;
            break;
        }
        if !clipped {
            // Degenerate input: drop a collinear vertex or fan the rest.
            let m = idx.len();
            let mut removed = false;
            for k in 0..m {
                let (a, b, c) = (
                    poly[idx[(k + m - 1) % m]],
                    poly[idx[k]],
                    poly[idx[(k + 1) % m]],
                );
                if cross(sub(b, a), sub(c, b)).abs() <= 1e-14 {
                    idx.remove(k);
                    removed = true;
                    break;
                }
            }
            if !removed {
                for k in 1..idx.len() - 1 {
                    out.push([idx[0] as u32, idx[k] as u32, idx[k + 1] as u32]);
                }
                return out;
            }
        }
    }
    if idx.len() == 3 {
        out.push([idx[0] as u32, idx[1] as u32, idx[2] as u32]);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn affine_roundtrip() {
        let m = Affine::trans(2.0, -1.0).mul(&Affine::rot(0.7));
        let p = [0.3, 4.0];
        let q = m.inv().apply(m.apply(p));
        assert!(dist(p, q) < 1e-12);
    }
    #[test]
    fn triangulate_concave() {
        let poly = [[0.0, 0.0], [4.0, 0.0], [4.0, 4.0], [2.0, 1.0], [0.0, 4.0]];
        let t = triangulate(&poly);
        assert_eq!(t.len(), 3);
        let a: f64 = t
            .iter()
            .map(|t| {
                area(&[
                    poly[t[0] as usize],
                    poly[t[1] as usize],
                    poly[t[2] as usize],
                ])
            })
            .sum();
        assert!((a - area(&poly)).abs() < 1e-9);
    }
}
