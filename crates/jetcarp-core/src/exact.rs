//! Exact plane geometry for aperiodic patches: points in the cyclotomic integers Z[ζ₁₂] (Spectre,
//! hat) and Z[ζ₁₀] (Penrose), rigid motions that rotate by a multiple of 2π/N, reflect, and
//! translate by a ring element, and an exact edge matcher for edge-to-edge patches.
//!
//! A point is an integer 4-vector in the basis 1, ζ, ζ², ζ³ (ζ⁴ = ζ² − 1 for N = 12,
//! ζ⁴ = ζ³ − ζ² + ζ − 1 for N = 10), the representation of [`crate::zomega`]. Coordinates stay
//! integers through every substitution step, so vertex identity is equality of integer vectors:
//! tile placement and neighbour detection are hash lookups with no tolerance. Floats appear only
//! when a point is converted for drawing ([`Cyclo::to_f64`]).

use crate::geom::{Affine, P};
use std::cmp::Ordering;
use std::hash::{BuildHasherDefault, Hasher};

pub type Z4 = [i64; 4];

pub fn zadd(a: Z4, b: Z4) -> Z4 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2], a[3] + b[3]]
}
pub fn zsub(a: Z4, b: Z4) -> Z4 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2], a[3] - b[3]]
}
pub fn zscale(a: Z4, k: i64) -> Z4 {
    a.map(|x| x * k)
}

/// Sign of `a + b √d` (d > 0, not a square), exactly.
pub fn sign_surd(a: i128, b: i128, d: i128) -> Ordering {
    let sa = a.cmp(&0);
    let sb = b.cmp(&0);
    if sb == Ordering::Equal {
        return sa;
    }
    if sa == Ordering::Equal || sa == sb {
        return sb;
    }
    // Opposite signs: compare a² with d b².
    let (a2, db2) = (a * a, d * b * b);
    if sa == Ordering::Greater {
        a2.cmp(&db2)
    } else {
        db2.cmp(&a2)
    }
}

/// A cyclotomic ring Z[ζ_N] of rank 4 (N = 10 or 12).
pub trait Cyclo: Copy + Send + Sync + 'static {
    /// Order of ζ.
    const N: u8;
    /// ζ · z.
    fn mul_zeta(z: Z4) -> Z4;
    /// Complex conjugate.
    fn conj(z: Z4) -> Z4;
    /// Float images of 1, ζ, ζ², ζ³.
    fn basis() -> [P; 4];
    /// Signs of the real and imaginary parts.
    fn sign_re(z: Z4) -> Ordering;
    fn sign_im(z: Z4) -> Ordering;
    /// Exact coordinates of the imaginary part: two integers that determine Im(z) (a fixed
    /// positive multiple of `x + y √3` for N = 12, of `x + y φ` for N = 10).
    fn im_parts(z: Z4) -> (i128, i128);

    fn to_f64(z: Z4) -> P {
        let b = Self::basis();
        let mut p = [0.0, 0.0];
        for k in 0..4 {
            p[0] += z[k] as f64 * b[k][0];
            p[1] += z[k] as f64 * b[k][1];
        }
        p
    }
    /// ζᵏ · z.
    fn rotate(z: Z4, k: i64) -> Z4 {
        let mut z = z;
        for _ in 0..k.rem_euclid(Self::N as i64) {
            z = Self::mul_zeta(z);
        }
        z
    }
    /// ζᵏ.
    fn unit(k: i64) -> Z4 {
        Self::rotate([1, 0, 0, 0], k)
    }
    /// a · b.
    fn mul(a: Z4, b: Z4) -> Z4 {
        let mut out = [0; 4];
        let mut p = b;
        for &ak in &a {
            out = zadd(out, zscale(p, ak));
            p = Self::mul_zeta(p);
        }
        out
    }
    /// Order of the directions of `a` and `b` by `atan2` angle in (-π, π] (the order of
    /// [`crate::geom::angle_of`]), exactly. Zero vectors sort first.
    fn cmp_angle(a: Z4, b: Z4) -> Ordering {
        let half = |z: Z4| -> u8 {
            match (Self::sign_im(z), Self::sign_re(z)) {
                (Ordering::Less, _) => 1,
                (Ordering::Equal, Ordering::Greater) => 2,
                (Ordering::Greater, _) => 3,
                (Ordering::Equal, Ordering::Less) => 4,
                (Ordering::Equal, Ordering::Equal) => 0,
            }
        };
        let (ha, hb) = (half(a), half(b));
        if ha != hb {
            return ha.cmp(&hb);
        }
        if ha == 1 || ha == 3 {
            // Same open half-plane: b is counterclockwise of a when Im(conj(a) b) > 0.
            return Self::sign_im(Self::mul(Self::conj(a), b)).reverse();
        }
        Ordering::Equal
    }
}

/// Z[ζ₁₂], ζ = e^{iπ/6}: Spectre and hat corners (every edge a unit or √3 vector at a multiple
/// of 30°).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Z12;

const H3: f64 = 0.866_025_403_784_438_6; // √3 / 2

impl Cyclo for Z12 {
    const N: u8 = 12;
    fn mul_zeta(z: Z4) -> Z4 {
        crate::zomega::wmul(z)
    }
    fn conj(z: Z4) -> Z4 {
        // conj ζ = ζ - ζ³, conj ζ² = 1 - ζ², conj ζ³ = -ζ³.
        let [a, b, c, d] = z;
        [a + c, b, -c, -(b + d)]
    }
    fn basis() -> [P; 4] {
        [[1.0, 0.0], [H3, 0.5], [0.5, H3], [0.0, 1.0]]
    }
    fn sign_re(z: Z4) -> Ordering {
        // 2 Re = (2a + c) + b √3.
        sign_surd(2 * z[0] as i128 + z[2] as i128, z[1] as i128, 3)
    }
    fn sign_im(z: Z4) -> Ordering {
        // 2 Im = (b + 2d) + c √3.
        let (x, y) = Self::im_parts(z);
        sign_surd(x, y, 3)
    }
    fn im_parts(z: Z4) -> (i128, i128) {
        (z[1] as i128 + 2 * z[3] as i128, z[2] as i128)
    }
}

/// Z[ζ₁₀], ζ = e^{iπ/5}: Penrose corners (edges at multiples of 36°, lengths in Z[φ]).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Z10;

impl Cyclo for Z10 {
    const N: u8 = 10;
    fn mul_zeta(z: Z4) -> Z4 {
        // ζ⁴ = ζ³ - ζ² + ζ - 1.
        let [a, b, c, d] = z;
        [-d, a + d, b - d, c + d]
    }
    fn conj(z: Z4) -> Z4 {
        // conj ζᵏ = ζ¹⁰⁻ᵏ: ζ⁹ = -ζ⁴, ζ⁸ = -ζ³, ζ⁷ = -ζ².
        let [a, b, c, d] = z;
        let m4 = [1, -1, 1, -1]; // -ζ⁴
        let mut out = [a, 0, 0, 0];
        out = zadd(out, zscale(m4, b));
        out = zadd(out, [0, 0, 0, -c]);
        out = zadd(out, [0, 0, -d, 0]);
        out
    }
    fn basis() -> [P; 4] {
        let u = |k: f64| {
            let (s, c) = crate::detmath::sin_cos(k * std::f64::consts::PI / 5.0);
            [c, s]
        };
        [[1.0, 0.0], u(1.0), u(2.0), u(3.0)]
    }
    fn sign_re(z: Z4) -> Ordering {
        // 2 Re = (2a - c + d) + (b + c - d) φ, φ = (1 + √5) / 2.
        let (x, y) = (
            2 * z[0] as i128 - z[2] as i128 + z[3] as i128,
            z[1] as i128 + z[2] as i128 - z[3] as i128,
        );
        sign_surd(2 * x + y, y, 5)
    }
    fn sign_im(z: Z4) -> Ordering {
        // Im = sin 36° (b + (c + d) φ).
        let (x, y) = Self::im_parts(z);
        sign_surd(2 * x + y, y, 5)
    }
    fn im_parts(z: Z4) -> (i128, i128) {
        (z[1] as i128, z[2] as i128 + z[3] as i128)
    }
}

/// A rigid motion z ↦ ζ^rot · (conj z if `refl`) + t.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Iso {
    pub rot: u8,
    pub refl: bool,
    pub t: Z4,
}

impl Iso {
    pub const ID: Iso = Iso {
        rot: 0,
        refl: false,
        t: [0; 4],
    };
    pub fn rot(k: i64, n: u8) -> Iso {
        Iso {
            rot: k.rem_euclid(n as i64) as u8,
            ..Iso::ID
        }
    }
    pub fn trans(t: Z4) -> Iso {
        Iso { t, ..Iso::ID }
    }
    pub fn apply<C: Cyclo>(&self, z: Z4) -> Z4 {
        let z = if self.refl { C::conj(z) } else { z };
        zadd(C::rotate(z, self.rot as i64), self.t)
    }
    /// The linear part only.
    pub fn apply_vec<C: Cyclo>(&self, z: Z4) -> Z4 {
        let z = if self.refl { C::conj(z) } else { z };
        C::rotate(z, self.rot as i64)
    }
    /// `self ∘ b` (apply `b` first).
    pub fn mul<C: Cyclo>(&self, b: &Iso) -> Iso {
        let n = C::N as i64;
        let br = if self.refl {
            -(b.rot as i64)
        } else {
            b.rot as i64
        };
        Iso {
            rot: (self.rot as i64 + br).rem_euclid(n) as u8,
            refl: self.refl ^ b.refl,
            t: self.apply::<C>(b.t),
        }
    }
    pub fn inv<C: Cyclo>(&self) -> Iso {
        let n = C::N as i64;
        // z = ζ^-r (w - t) (then conj if refl).
        let lin = Iso {
            rot: if self.refl {
                self.rot
            } else {
                (-(self.rot as i64)).rem_euclid(n) as u8
            },
            refl: self.refl,
            t: [0; 4],
        };
        Iso {
            t: zscale(lin.apply::<C>(self.t), -1),
            ..lin
        }
    }
    /// The float affine map, for drawing.
    pub fn to_affine<C: Cyclo>(&self) -> Affine {
        let a = self.rot as f64 * 2.0 * std::f64::consts::PI / C::N as f64;
        let (s, c) =
            exact_sin_cos(self.rot as u32, C::N as u32).unwrap_or(crate::detmath::sin_cos(a));
        let t = C::to_f64(self.t);
        if self.refl {
            Affine([c, s, t[0], s, -c, t[1]])
        } else {
            Affine([c, -s, t[0], s, c, t[1]])
        }
    }
    /// Compact form (translation in i32), if it fits.
    pub fn compact(&self) -> Option<Place> {
        let mut t = [0i32; 4];
        for (o, &x) in t.iter_mut().zip(&self.t) {
            *o = i32::try_from(x).ok()?;
        }
        Some(Place {
            t,
            rot: self.rot,
            refl: self.refl,
        })
    }
}

/// Correctly rounded sin and cos of k · 2π/n for the angles whose values are short surds, so
/// that axis-aligned rotations are exact (cos 90° = 0, not 6e-17).
fn exact_sin_cos(k: u32, n: u32) -> Option<(f64, f64)> {
    let k = k % n;
    // Quarter turns.
    if (4 * k).is_multiple_of(n) {
        return Some(match 4 * k / n {
            0 => (0.0, 1.0),
            1 => (1.0, 0.0),
            2 => (0.0, -1.0),
            _ => (-1.0, 0.0),
        });
    }
    if n == 12 {
        let t = [
            (0.0, 1.0),
            (0.5, H3),
            (H3, 0.5),
            (1.0, 0.0),
            (H3, -0.5),
            (0.5, -H3),
            (0.0, -1.0),
            (-0.5, -H3),
            (-H3, -0.5),
            (-1.0, 0.0),
            (-H3, 0.5),
            (-0.5, H3),
        ];
        return Some(t[k as usize]);
    }
    None
}

/// An [`Iso`] stored in 20 bytes (translation coordinates as i32).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub struct Place {
    pub t: [i32; 4],
    pub rot: u8,
    pub refl: bool,
}

impl Place {
    pub fn iso(&self) -> Iso {
        Iso {
            rot: self.rot,
            refl: self.refl,
            t: self.t.map(|x| x as i64),
        }
    }
}

/// FxHash (the rustc hasher): fast, deterministic, good enough for integer keys.
#[derive(Default, Clone, Copy)]
pub struct Fx(u64);

impl Hasher for Fx {
    #[inline]
    fn write(&mut self, bytes: &[u8]) {
        for chunk in bytes.chunks(8) {
            let mut b = [0u8; 8];
            b[..chunk.len()].copy_from_slice(chunk);
            self.write_u64(u64::from_le_bytes(b));
        }
    }
    #[inline]
    fn write_u64(&mut self, x: u64) {
        self.0 = (self.0.rotate_left(5) ^ x).wrapping_mul(0x51_7c_c1_b7_27_22_0a_95);
    }
    #[inline]
    fn write_u32(&mut self, x: u32) {
        self.write_u64(x as u64);
    }
    #[inline]
    fn write_i32(&mut self, x: i32) {
        self.write_u64(x as u32 as u64);
    }
    #[inline]
    fn write_i64(&mut self, x: i64) {
        self.write_u64(x as u64);
    }
    #[inline]
    fn write_u8(&mut self, x: u8) {
        self.write_u64(x as u64);
    }
    #[inline]
    fn write_usize(&mut self, x: usize) {
        self.write_u64(x as u64);
    }
    #[inline]
    fn finish(&self) -> u64 {
        self.0
    }
}

pub type FxMap<K, V> = std::collections::HashMap<K, V, BuildHasherDefault<Fx>>;

/// No partner across this edge (the patch boundary).
pub const OPEN: u32 = u32::MAX;

/// Match the edges of an edge-to-edge patch exactly.
///
/// Tile `t` has `deg(t)` corners `corner(t, k)` (every tile the same number of slots, `stride`),
/// listed counterclockwise; edge `k` runs from corner `k` to corner `k + 1`. Returns, per tile and
/// edge, the matching edge `u * stride + j` of the neighbour across it (which runs the other
/// way), or [`OPEN`]. Fails if two tiles share an edge in the same direction (they overlap) or
/// three tiles share one edge.
///
/// Tiles are matched in parallel over contiguous chunks, each with its own map of unmatched
/// edges; the leftovers (edges on the chunks' outlines) are matched at the end. Patches built by
/// substitution list tiles supertile by supertile, so a chunk's leftovers are few.
pub fn pair_edges<F>(
    n: usize,
    stride: usize,
    deg: impl Fn(usize) -> usize + Sync,
    corner: F,
) -> Result<Vec<u32>, String>
where
    F: Fn(usize, usize) -> Z4 + Sync,
{
    use rayon::prelude::*;
    assert!(
        (n as u64) * (stride as u64) < OPEN as u64,
        "patch too large for 32-bit edge ids"
    );
    // An edge's key: the sum of its ends (twice its midpoint) and its direction up to sign, in
    // 32-bit coordinates. Two distinct edges of a valid patch never share a midpoint and a line.
    type Key = ([i32; 4], [i32; 4]);
    let narrow = |z: Z4| -> Option<[i32; 4]> {
        Some([
            i32::try_from(z[0]).ok()?,
            i32::try_from(z[1]).ok()?,
            i32::try_from(z[2]).ok()?,
            i32::try_from(z[3]).ok()?,
        ])
    };
    let key = |a: Z4, b: Z4| -> Result<(Key, bool), String> {
        let d = zsub(b, a);
        let neg = zscale(d, -1);
        let (d, fwd) = if d > neg { (d, true) } else { (neg, false) };
        match (narrow(zadd(a, b)), narrow(d)) {
            (Some(s), Some(d)) => Ok(((s, d), fwd)),
            _ => Err("corner coordinates overflow 32 bits".into()),
        }
    };
    let overlap = |x: u32, y: u32| {
        format!(
            "tiles {} and {} overlap along an edge",
            x as usize / stride,
            y as usize / stride
        )
    };
    let chunk = 1 << 15;
    let mut partner = vec![OPEN; n * stride];
    type Left = Vec<(Key, bool, u32)>;
    // Unmatched edges of the chunks so far (the outline of the region done), merged a batch of
    // chunks at a time so only one batch's leftovers are held at once.
    let mut open: FxMap<Key, (bool, u32)> = FxMap::default();
    let nchunks = n.div_ceil(chunk);
    let batch = rayon::current_num_threads().max(1) * 2;
    let mut c0 = 0;
    while c0 < nchunks {
        let c1 = (c0 + batch).min(nchunks);
        let lo = c0 * chunk * stride;
        let hi = (c1 * chunk).min(n) * stride;
        let parts: Vec<Result<Left, String>> = partner[lo..hi]
            .par_chunks_mut(chunk * stride)
            .enumerate()
            .map(|(i, out)| {
                let t0 = (c0 + i) * chunk;
                let t1 = (t0 + chunk).min(n);
                let mut open: FxMap<Key, (bool, u32)> = FxMap::default();
                for t in t0..t1 {
                    let m = deg(t);
                    for k in 0..m {
                        let (kk, fwd) = key(corner(t, k), corner(t, (k + 1) % m))?;
                        let id = (t * stride + k) as u32;
                        match open.remove(&kk) {
                            Some((f2, other)) => {
                                if f2 == fwd {
                                    return Err(overlap(other, id));
                                }
                                out[id as usize - t0 * stride] = other;
                                out[other as usize - t0 * stride] = id;
                            }
                            None => {
                                open.insert(kk, (fwd, id));
                            }
                        }
                    }
                }
                let mut left: Left = open.into_iter().map(|(k, (f, id))| (k, f, id)).collect();
                left.sort_unstable_by_key(|x| x.2);
                Ok(left)
            })
            .collect();
        for p in parts {
            for (kk, fwd, id) in p? {
                match open.remove(&kk) {
                    Some((f2, other)) => {
                        if f2 == fwd {
                            return Err(overlap(other, id));
                        }
                        partner[id as usize] = other;
                        partner[other as usize] = id;
                    }
                    None => {
                        open.insert(kk, (fwd, id));
                    }
                }
            }
        }
        c0 = c1;
    }
    Ok(partner)
}

/// The tiles around corner `k` of tile `t`, in rotational order starting across edge `k`
/// (excluding `t`), walking over matched edges; and whether the walk closed (an interior
/// vertex). `corner` and `partner` are as in [`pair_edges`].
pub fn around_vertex(
    t: usize,
    k: usize,
    stride: usize,
    deg: &impl Fn(usize) -> usize,
    corner: &impl Fn(usize, usize) -> Z4,
    partner: &[u32],
    out: &mut Vec<u32>,
) -> bool {
    out.clear();
    let v = corner(t, k);
    let m = deg(t);
    // Walk one way from edge k, then (if the boundary stops it) the other way from edge k-1.
    for (start, back) in [(k, false), ((k + m - 1) % m, true)] {
        let mut cur = (t, start);
        let mut steps = 0;
        loop {
            let p = partner[cur.0 * stride + cur.1];
            if p == OPEN {
                break;
            }
            let (u, j) = (p as usize / stride, p as usize % stride);
            if u == t {
                return true;
            }
            steps += 1;
            assert!(steps < 64, "vertex walk did not close");
            if back {
                out.insert(0, u as u32);
            } else {
                out.push(u as u32);
            }
            let mu = deg(u);
            // The other edge of u at v.
            let e = if corner(u, j) == v {
                (j + mu - 1) % mu
            } else {
                (j + 1) % mu
            };
            cur = (u, e);
        }
    }
    false
}

/// Direction of a nonzero vector as a multiple of 2π/N, if it is one.
pub fn dir_index<C: Cyclo>(d: Z4) -> Option<u8> {
    let mut z = d;
    // z = d · ζ^-k; the direction is k when z is a positive real.
    for k in 0..C::N {
        if C::sign_im(z) == Ordering::Equal && C::sign_re(z) == Ordering::Greater {
            return Some(k);
        }
        // Multiply by ζ^-1 = ζ^(N-1).
        z = C::rotate(z, C::N as i64 - 1);
    }
    None
}

/// Exact facts about a patch, from [`verify_patch`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PatchStats {
    pub tiles: usize,
    pub vertices: usize,
    pub edges: usize,
    pub boundary_edges: usize,
    /// V − E + F (1 for a disc).
    pub euler: i64,
    /// Twice the area enclosed by the boundary, and twice the summed tile areas, as exact
    /// coordinates of the imaginary part (see [`Cyclo::im_parts`]); equal when the tiles fill
    /// the boundary.
    pub boundary_area2: (i128, i128),
    pub tile_area2: (i128, i128),
}

/// Check exactly that a patch of counterclockwise polygons is a tiling of a disc, with no gaps
/// or overlaps:
///
/// 1. every edge is matched to at most one other edge, running the other way (by construction
///    of `partner`, checked here for symmetry);
/// 2. at every interior vertex (where the walk over matched edges closes) the corner angles add
///    up to exactly one turn, so the tiles around it neither overlap nor leave a gap;
/// 3. the unmatched edges form one closed loop through distinct vertices, and no two of its
///    edges cross: the boundary is a simple closed curve;
/// 4. V − E + F = 1, and the area inside the boundary equals the summed tile areas.
///
/// (1) and (2) make the patch a locally injective map of a surface into the plane; (3) and (4)
/// make that surface a disc whose boundary is a simple curve, so the map is one to one: the tiles
/// cover the region inside the boundary exactly once.
pub fn verify_patch<C: Cyclo>(
    n: usize,
    stride: usize,
    deg: impl Fn(usize) -> usize + Sync,
    corner: impl Fn(usize, usize) -> Z4 + Sync,
    partner: &[u32],
) -> Result<PatchStats, String> {
    use rayon::prelude::*;
    let half = C::N as i64 / 2;
    // Interior angle at corner k of tile t, in units of 2π/N.
    let angle = |t: usize, k: usize| -> Result<i64, String> {
        let m = deg(t);
        let (a, b, c) = (
            corner(t, (k + m - 1) % m),
            corner(t, k),
            corner(t, (k + 1) % m),
        );
        let di = dir_index::<C>(zsub(b, a)).ok_or("edge direction off the grid")? as i64;
        let dout = dir_index::<C>(zsub(c, b)).ok_or("edge direction off the grid")? as i64;
        let mut turn = (dout - di).rem_euclid(C::N as i64);
        if turn > half {
            turn -= C::N as i64;
        }
        if turn == half || turn == -half {
            return Err(format!("tile {t} folds back at corner {k}"));
        }
        Ok(half - turn)
    };
    struct Acc {
        vertices: usize,
        matched: usize,
        open: usize,
        tile_area: [i128; 4],
    }
    let acc = (0..n)
        .into_par_iter()
        .map(|t| -> Result<Acc, String> {
            let m = deg(t);
            let mut a = Acc {
                vertices: 0,
                matched: 0,
                open: 0,
                tile_area: [0; 4],
            };
            let mut around = Vec::new();
            for k in 0..m {
                let p = partner[t * stride + k];
                if p == OPEN {
                    a.open += 1;
                } else {
                    if partner[p as usize] != (t * stride + k) as u32 {
                        return Err(format!("edge {k} of tile {t}: partner not mutual"));
                    }
                    let (u, j) = (p as usize / stride, p as usize % stride);
                    let mu = deg(u);
                    if corner(u, j) != corner(t, (k + 1) % m)
                        || corner(u, (j + 1) % mu) != corner(t, k)
                    {
                        return Err(format!("edge {k} of tile {t}: partner is not the same edge reversed"));
                    }
                    a.matched += 1;
                }
                let (v, w) = (corner(t, k), corner(t, (k + 1) % m));
                let x = C::mul(C::conj(v), w);
                for (o, &xi) in a.tile_area.iter_mut().zip(&x) {
                    *o += xi as i128;
                }
                // Each vertex is counted by the lowest-numbered tile at it.
                let closed = around_vertex(t, k, stride, &deg, &corner, partner, &mut around);
                if around.iter().all(|&u| (u as usize) > t) {
                    a.vertices += 1;
                    if closed {
                        let v = corner(t, k);
                        let mut sum = angle(t, k)?;
                        for &u in &around {
                            let u = u as usize;
                            let j = (0..deg(u))
                                .find(|&j| corner(u, j) == v)
                                .ok_or("vertex walk left the vertex")?;
                            sum += angle(u, j)?;
                        }
                        if sum != C::N as i64 {
                            return Err(format!(
                                "corner angles at an interior vertex of tile {t} add up to {sum} of {} units",
                                C::N
                            ));
                        }
                    }
                }
            }
            Ok(a)
        })
        .try_reduce(
            || Acc {
                vertices: 0,
                matched: 0,
                open: 0,
                tile_area: [0; 4],
            },
            |x, y| {
                Ok(Acc {
                    vertices: x.vertices + y.vertices,
                    matched: x.matched + y.matched,
                    open: x.open + y.open,
                    tile_area: [
                        x.tile_area[0] + y.tile_area[0],
                        x.tile_area[1] + y.tile_area[1],
                        x.tile_area[2] + y.tile_area[2],
                        x.tile_area[3] + y.tile_area[3],
                    ],
                })
            },
        )?;
    // The boundary: one outgoing open edge per boundary vertex, one loop.
    let mut next: FxMap<Z4, (Z4, usize)> = FxMap::default();
    let mut boundary = Vec::new();
    for t in 0..n {
        let m = deg(t);
        for k in 0..m {
            if partner[t * stride + k] == OPEN {
                let (a, b) = (corner(t, k), corner(t, (k + 1) % m));
                if next.insert(a, (b, t)).is_some() {
                    return Err(format!(
                        "the boundary passes twice through a corner of tile {t}"
                    ));
                }
                boundary.push((a, b));
            }
        }
    }
    let mut boundary_area = [0i128; 4];
    if let Some(&(start, _)) = boundary.first() {
        let mut v = start;
        let mut steps = 0;
        loop {
            let (w, _) = *next.get(&v).ok_or("the boundary does not close")?;
            let x = C::mul(C::conj(v), w);
            for i in 0..4 {
                boundary_area[i] += x[i] as i128;
            }
            steps += 1;
            v = w;
            if v == start {
                break;
            }
            if steps > boundary.len() {
                return Err("the boundary does not close".into());
            }
        }
        if steps != boundary.len() {
            return Err(format!(
                "the boundary is more than one loop (one has {steps} of {} edges): the patch has holes",
                boundary.len()
            ));
        }
    }
    simple_loop::<C>(&boundary)?;
    let edges = acc.matched / 2 + acc.open;
    let euler = acc.vertices as i64 - edges as i64 + n as i64;
    let to_i128 = |z: [i128; 4]| -> Z4 { z.map(|x| i64::try_from(x).expect("area overflow")) };
    let stats = PatchStats {
        tiles: n,
        vertices: acc.vertices,
        edges,
        boundary_edges: acc.open,
        euler,
        boundary_area2: C::im_parts(to_i128(boundary_area)),
        tile_area2: C::im_parts(to_i128(acc.tile_area)),
    };
    if euler != 1 {
        return Err(format!("V - E + F = {euler}, not 1: not a disc"));
    }
    if stats.boundary_area2 != stats.tile_area2 {
        return Err(format!(
            "area inside the boundary {:?} differs from the tiles' area {:?}",
            stats.boundary_area2, stats.tile_area2
        ));
    }
    Ok(stats)
}

/// No two edges of a closed loop cross or touch except consecutive edges at their shared end.
fn simple_loop<C: Cyclo>(edges: &[(Z4, Z4)]) -> Result<(), String> {
    if edges.is_empty() {
        return Ok(());
    }
    // Bucket edges by float bounding box on a grid; test candidate pairs exactly.
    let pts: Vec<(P, P)> = edges
        .iter()
        .map(|&(a, b)| (C::to_f64(a), C::to_f64(b)))
        .collect();
    let len = pts
        .iter()
        .map(|(a, b)| crate::detmath::hypot(a[0] - b[0], a[1] - b[1]))
        .fold(0.0, f64::max)
        .max(1e-9);
    let cell = 2.0 * len;
    let key = |x: f64| (x / cell).floor() as i64;
    let mut grid: FxMap<(i64, i64), Vec<u32>> = FxMap::default();
    for (i, (a, b)) in pts.iter().enumerate() {
        for gx in key(a[0].min(b[0]) - 1e-6)..=key(a[0].max(b[0]) + 1e-6) {
            for gy in key(a[1].min(b[1]) - 1e-6)..=key(a[1].max(b[1]) + 1e-6) {
                grid.entry((gx, gy)).or_default().push(i as u32);
            }
        }
    }
    let orient =
        |a: Z4, b: Z4, c: Z4| -> Ordering { C::sign_im(C::mul(C::conj(zsub(b, a)), zsub(c, a))) };
    // Does point p lie on the closed segment a-b (given collinear)?
    let on_seg = |a: Z4, b: Z4, p: Z4| -> bool {
        // Collinear: p - a = s (b - a) with 0 <= s <= 1: compare projections exactly.
        let d = zsub(b, a);
        let ap = zsub(p, a);
        let bp = zsub(p, b);
        // Re(conj(d) (p - a)) >= 0 and Re(conj(d) (p - b)) <= 0.
        C::sign_re(C::mul(C::conj(d), ap)) != Ordering::Less
            && C::sign_re(C::mul(C::conj(d), bp)) != Ordering::Greater
    };
    for cands in grid.values() {
        for (x, &i) in cands.iter().enumerate() {
            for &j in &cands[x + 1..] {
                let ((a, b), (c, d)) = (edges[i as usize], edges[j as usize]);
                // Shared endpoints are allowed only between consecutive edges (b == c or d == a).
                let shared = (a == c) as u8 + (a == d) as u8 + (b == c) as u8 + (b == d) as u8;
                let (o1, o2) = (orient(a, b, c), orient(a, b, d));
                let (o3, o4) = (orient(c, d, a), orient(c, d, b));
                if shared > 0 {
                    if a == c || b == d || shared > 1 {
                        return Err("the boundary runs twice along or through a corner".into());
                    }
                    // Consecutive: the other ends must not fold back onto the shared edge.
                    if o1 == Ordering::Equal
                        && o2 == Ordering::Equal
                        && ((b == c && on_seg(a, b, d)) || (d == a && on_seg(c, d, b)))
                    {
                        return Err("the boundary folds back on itself".into());
                    }
                    continue;
                }
                let cross = (o1 != o2 && o3 != o4)
                    || (o1 == Ordering::Equal && on_seg(a, b, c))
                    || (o2 == Ordering::Equal && on_seg(a, b, d))
                    || (o3 == Ordering::Equal && on_seg(c, d, a))
                    || (o4 == Ordering::Equal && on_seg(c, d, b));
                if cross {
                    return Err("two boundary edges cross: the patch overlaps itself".into());
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(p: P, q: P) -> bool {
        (p[0] - q[0]).abs() < 1e-12 && (p[1] - q[1]).abs() < 1e-12
    }

    fn check_ring<C: Cyclo>() {
        let n = C::N as i64;
        for k in 0..n {
            let a = k as f64 * 2.0 * std::f64::consts::PI / n as f64;
            let u = C::unit(k);
            assert!(close(C::to_f64(u), [a.cos(), a.sin()]), "N={n} k={k}");
            let c = C::to_f64(C::conj(u));
            assert!(close(c, [a.cos(), -a.sin()]), "conj N={n} k={k}");
            assert_eq!(C::conj(C::conj(u)), u);
        }
        assert_eq!(C::unit(n), [1, 0, 0, 0]);
        // Products and signs against floats on a few mixed elements.
        let zs: [Z4; 5] = [
            [3, -2, 5, 1],
            [0, 1, -1, 2],
            [-4, 0, 3, -2],
            [1, 1, 1, 1],
            [2, -5, 0, 4],
        ];
        for &a in &zs {
            for &b in &zs {
                let (pa, pb, pm) = (C::to_f64(a), C::to_f64(b), C::to_f64(C::mul(a, b)));
                let want = [pa[0] * pb[0] - pa[1] * pb[1], pa[0] * pb[1] + pa[1] * pb[0]];
                assert!((pm[0] - want[0]).abs() < 1e-9 && (pm[1] - want[1]).abs() < 1e-9);
                let fa = crate::geom::angle_of(pa);
                let fb = crate::geom::angle_of(pb);
                assert_eq!(
                    C::cmp_angle(a, b),
                    fa.partial_cmp(&fb).unwrap(),
                    "{a:?} {b:?}"
                );
            }
            let p = C::to_f64(a);
            assert_eq!(C::sign_re(a), p[0].partial_cmp(&0.0).unwrap());
            assert_eq!(C::sign_im(a), p[1].partial_cmp(&0.0).unwrap());
        }
        // Isometries compose and invert like their float maps.
        let isos = [
            Iso {
                rot: 3,
                refl: false,
                t: [1, 2, -1, 0],
            },
            Iso {
                rot: 7,
                refl: true,
                t: [0, -3, 2, 5],
            },
            Iso {
                rot: 1,
                refl: true,
                t: [4, 0, 0, -1],
            },
        ];
        for a in &isos {
            for b in &isos {
                let m = a.mul::<C>(b).to_affine::<C>();
                let f = a.to_affine::<C>().mul(&b.to_affine::<C>());
                for k in 0..6 {
                    assert!((m.0[k] - f.0[k]).abs() < 1e-9);
                }
                for &z in &zs {
                    assert!(
                        close(
                            C::to_f64(a.apply::<C>(z)),
                            a.to_affine::<C>().apply(C::to_f64(z))
                        ) || {
                            let (p, q) = (
                                C::to_f64(a.apply::<C>(z)),
                                a.to_affine::<C>().apply(C::to_f64(z)),
                            );
                            (p[0] - q[0]).abs() < 1e-9 && (p[1] - q[1]).abs() < 1e-9
                        }
                    );
                }
            }
            assert_eq!(a.mul::<C>(&a.inv::<C>()), Iso::ID);
        }
    }

    #[test]
    fn rings_match_floats() {
        check_ring::<Z12>();
        check_ring::<Z10>();
    }

    #[test]
    fn sign_of_surds() {
        assert_eq!(sign_surd(-1, 1, 3), Ordering::Greater);
        assert_eq!(sign_surd(2, -1, 3), Ordering::Greater);
        assert_eq!(sign_surd(1, -1, 3), Ordering::Less);
        assert_eq!(sign_surd(0, 0, 3), Ordering::Equal);
    }
}
