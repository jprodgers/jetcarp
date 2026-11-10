//! Penrose tilings by Robinson-triangle deflation: P2 (kites and darts) and P3 (thick and thin
//! rhombs).
//!
//! Written in JETCARP from the geometry of Robinson's half-tiles (see Grünbaum and Shephard, *Tilings
//! and Patterns*, §10.3; de Bruijn 1981). The P3 deflation and seed were rewritten clean-room in
//! November 2026 from the mathematics alone, after an audit found the earlier version followed
//! another program's unlicensed code. Each tile is two mirror-image golden triangles. The triangles deflate
//! exactly; after the last deflation each triangle is joined to its mirror partner across their
//! shared edge, and triangles whose partner fell outside the patch are dropped.
//!
//! Both tilings start from a five-fold "sun" centred on the origin, so the patch keeps a five-fold
//! centre. Coordinates come out with the long edge (P2) or the edge (P3) of length 1.

use crate::geom::*;
use std::collections::HashMap;

pub const PHI: f64 = std::f64::consts::GOLDEN_RATIO;

/// A Robinson triangle. `kind` 0 is the acute golden triangle (36° apex), 1 the obtuse (108°).
/// `a` is the apex. Which edge joins the triangle to its mirror partner depends on the tiling:
/// the base `b`–`c` for P3, the leg `a`–`b` for P2.
#[derive(Clone, Copy, Debug)]
struct Tri {
    kind: u8,
    a: P,
    b: P,
    c: P,
}

fn lerp(p: P, q: P, t: f64) -> P {
    add(p, scale(sub(q, p), t))
}

fn deg(d: f64) -> P {
    let (s, c) = crate::detmath::sin_cos(d.to_radians());
    [c, s]
}

/// Starting patch for the Penrose rhomb (P3) tiling: a "sun" of five thick rhombs whose 72°
/// corners all meet at the origin, cut into ten obtuse Robinson triangles (kind 1).
///
/// `scale_s` is the rhomb edge length, i.e. the triangles' leg length. `deg(d)` returns the unit
/// vector at `d` degrees; only multiples of 36° are asked for, so any rotation the caller puts
/// inside `deg` turns the whole patch rigidly.
///
/// Rhomb j (j = 0..4) has its long diagonal on the ray at 72·j degrees, from the origin to its far
/// 72° corner at distance φ·scale_s. Its two 108° corners lie at distance scale_s, at 72·j − 36 and
/// 72·j + 36 degrees. Each corner point is shared with the neighbouring rhomb, so it is computed
/// once and reused: neighbours then agree on it to the last bit, and [`deflate_p3`] keeps that
/// property for every point it creates.
///
/// The rhomb is cut along its long diagonal; each half is an obtuse triangle with apex a = a 108°
/// corner, b = the far tip and c = the origin. In the vertex convention of [`deflate_p3`] that
/// makes all five spokes at the origin "split" edges (a–c) of the triangles on both sides of them,
/// so the edge markings agree all the way round. This centre is a legal Penrose vertex (one of de
/// Bruijn's eight P3 vertex types): four deflations give it back exactly.
///
/// Nesting: one deflation turns this sun into a smaller sun with the same five axes but with the
/// "whole" edges (a–b) at the centre; a second deflation turns that into a copy of this seed
/// rotated by 36°. So two deflations of the seed at size φ²·s contain, triangle for triangle, the
/// seed at size s turned by 36°, and [`generate`] turns the seed by 36° on alternate levels so that
/// each level's patch contains the previous one. No five-fold seed can nest without that turn.
///
/// Written for JETCARP from the geometry of Robinson triangles (Grünbaum and Shephard, *Tilings
/// and Patterns*, §10.3; de Bruijn 1981), without reference to other Penrose tiling code.
fn p3_seed(scale_s: f64, deg: impl Fn(f64) -> P) -> Vec<Tri> {
    let origin: P = [0.0, 0.0];
    let corner: Vec<P> = (0..5)
        .map(|j| scale(deg(36.0 + 72.0 * j as f64), scale_s))
        .collect();
    let mut out = Vec::with_capacity(10);
    for j in 0..5 {
        let tip = scale(deg(72.0 * j as f64), scale_s * PHI);
        for a in [corner[(j + 4) % 5], corner[j]] {
            out.push(Tri {
                kind: 1,
                a,
                b: tip,
                c: origin,
            });
        }
    }
    out
}

/// One Robinson-triangle deflation step for the Penrose rhomb (P3) tiling.
///
/// Each triangle is replaced by smaller Robinson triangles whose sides are 1/φ times the parent's,
/// so the patch keeps its overall size and the number of tiles grows by about φ² per step.
///
/// Vertex order carries the handedness. In a triangle (a; b, c) the two legs a–b and a–c are rhomb
/// edges of the same length L, but they are different kinds of edge, and the substitution treats
/// them differently:
///   * a–b, the "whole" edge, becomes the long side of a single child obtuse triangle (so a child
///     thick rhomb straddles it);
///   * a–c, the "split" edge, gets a new point at distance L/φ from one of its ends: from a in an
///     acute triangle, from c in an obtuse one.
///
/// The other half of a triangle's rhomb shares b and c in the same roles (so the two halves have
/// opposite orientation), and the two halves deflate as mirror images of each other. The edge
/// kinds and their directions are Penrose's arrow markings on the rhombs. Each child is listed
/// with its own a, b, c chosen so that tiles meeting along an edge again agree on the kind and
/// direction of that edge, which is what makes the result a Penrose tiling and not just some rhomb
/// tiling.
///
/// Acute triangle (kind 0, 36° at a, legs L, base L/φ) becomes 2 triangles. Let d be the point on
/// a–c at distance L/φ from a. Then:
///   * acute (b; c, d): its apex is the old base corner b and its base d–c is the short end of the
///     split edge;
///   * obtuse (d; a, b): its long side is the whole edge a–b.
///
/// Obtuse triangle (kind 1, 108° at a, legs L, base φL) becomes 3 triangles. Let e be the point on
/// the base at distance L from c (L/φ from b), and d the point on a–c at distance L/φ from c. Then:
///   * obtuse (e; a, b): its long side is the whole edge a–b;
///   * obtuse (d; c, e): its long side c–e lies on the parent's base, so with its mirror partner it
///     makes a complete small thick rhomb filling the parent rhomb's 72° corner at c;
///   * acute (e; a, d): its base a–d is the short end of the split edge.
///
/// (Areas: acute = acute + obtuse and obtuse = acute + 2 obtuse at 1/φ scale, which is why
/// thick:thin tends to φ.)
///
/// Every new point lies on an edge or base and is computed from that edge's ends in the same order
/// by both triangles that share it, so shared points come out bit-for-bit identical as long as the
/// input's shared points are.
fn deflate_p3(ts: &[Tri]) -> Vec<Tri> {
    let t = 1.0 / PHI;
    let mut out = Vec::with_capacity(ts.len() * 3);
    for &Tri { kind, a, b, c } in ts {
        if kind == 0 {
            let d = lerp(a, c, t);
            out.push(Tri {
                kind: 0,
                a: b,
                b: c,
                c: d,
            });
            out.push(Tri {
                kind: 1,
                a: d,
                b: a,
                c: b,
            });
        } else {
            let e = lerp(c, b, t);
            let d = lerp(c, a, t);
            out.push(Tri {
                kind: 1,
                a: e,
                b: a,
                c: b,
            });
            out.push(Tri {
                kind: 1,
                a: d,
                b: c,
                c: e,
            });
            out.push(Tri {
                kind: 0,
                a: e,
                b: a,
                c: d,
            });
        }
    }
    out
}

/// P2: partners meet across the leg `a`–`b`, the tile's axis of symmetry.
/// Acute = half kite: `a` head, `b` tail, `c` side corner (long edge `a`–`c`, short `c`–`b`).
/// Obtuse = half dart: `a` the reflex corner, `b` the nose, `c` a wing tip.
fn deflate_p2(ts: &[Tri]) -> Vec<Tri> {
    let mut out = Vec::with_capacity(ts.len() * 3);
    for t in ts {
        let (a, b, c) = (t.a, t.b, t.c);
        if t.kind == 0 {
            // A whole kite headed at the side corner, and half a dart nosed at the head.
            let d = lerp(a, b, 1.0 / PHI);
            let e = lerp(c, a, 1.0 / PHI);
            out.push(Tri {
                kind: 0,
                a: c,
                b: d,
                c: b,
            });
            out.push(Tri {
                kind: 0,
                a: c,
                b: d,
                c: e,
            });
            out.push(Tri {
                kind: 1,
                a: e,
                b: a,
                c: d,
            });
        } else {
            // Half a kite from the nose to the reflex corner, and half a dart nosed at the wing.
            let x = lerp(b, c, 1.0 / PHI);
            out.push(Tri {
                kind: 0,
                a: b,
                b: a,
                c: x,
            });
            out.push(Tri {
                kind: 1,
                a: x,
                b: c,
                c: a,
            });
        }
    }
    out
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Penrose {
    /// Kites (shape 0) and darts (shape 1).
    P2,
    /// Thick rhombs (shape 0) and thin rhombs (shape 1).
    P3,
}

/// Prototile polygons. Each has its axis of symmetry from the origin along +x.
pub fn shapes(kind: Penrose) -> Vec<Vec<P>> {
    let (s36, c36) = crate::detmath::sin_cos(36f64.to_radians());
    match kind {
        Penrose::P2 => vec![
            // Kite: head, side, tail, side. Long edges 1, short edges 1/φ.
            vec![[0.0, 0.0], [c36, s36], [1.0, 0.0], [c36, -s36]],
            // Dart: nose, wing, reflex corner, wing.
            vec![[0.0, 0.0], [c36, s36], [1.0 / PHI, 0.0], [c36, -s36]],
        ],
        Penrose::P3 => {
            let (s18, c18) = crate::detmath::sin_cos(18f64.to_radians());
            vec![
                // Thick rhomb, 108° at the origin.
                vec![[0.0, 0.0], [s36, c36], [2.0 * s36, 0.0], [s36, -c36]],
                // Thin rhomb, 36° at the origin.
                vec![[0.0, 0.0], [c18, s18], [2.0 * c18, 0.0], [c18, -s18]],
            ]
        }
    }
}

/// Length of each shape's axis (origin to the far corner on the x axis).
fn axis_len(kind: Penrose, shape: u8) -> f64 {
    match (kind, shape) {
        (Penrose::P2, 0) => 1.0,
        (Penrose::P2, _) => 1.0 / PHI,
        (Penrose::P3, 0) => 2.0 * crate::detmath::sin(36f64.to_radians()),
        (Penrose::P3, _) => 2.0 * crate::detmath::cos(18f64.to_radians()),
    }
}

/// The tiles after `2k` deflations of the sun, as (shape, transform), centred on the origin.
///
/// Two deflations turn the sun back into a sun rotated by 36°, so the seed turns by 36° on odd
/// `k`: every `k` then gives the same tiling around the centre, and patches nest.
pub fn generate(kind: Penrose, k: u32) -> Vec<(u16, Affine)> {
    let depth = 2 * k;
    let turn = if k % 2 == 1 { 36.0 } else { 0.0 };
    let deg = |d: f64| deg(d + turn);
    // Start with tile edges φ^depth long so the final tiles have unit edges.
    let s = crate::detmath::powi(PHI, depth as i32);
    let o = [0.0, 0.0];
    let mut ts: Vec<Tri> = Vec::new();
    match kind {
        Penrose::P3 => {
            // A sun of five thick rhombs (see [`p3_seed`]). Exact coordinates draw P3 through a
            // −18° turn ([`exact_frame`]), so the float seed sits at −18° + 36°·k to match.
            ts = p3_seed(s, |d| deg(d - 18.0));
        }
        Penrose::P2 => {
            // Five kites with their heads at the centre (the "sun" vertex), long edges φ^depth.
            for k in 0..5 {
                let th = 72.0 * k as f64;
                let tail = scale(deg(th), s);
                for side in [-36.0, 36.0] {
                    ts.push(Tri {
                        kind: 0,
                        a: o,
                        b: tail,
                        c: scale(deg(th + side), s),
                    });
                }
            }
        }
    }
    for _ in 0..depth {
        ts = match kind {
            Penrose::P3 => deflate_p3(&ts),
            Penrose::P2 => deflate_p2(&ts),
        };
    }
    // The pair edge of each triangle, and the two corners that become the tile's axis.
    let pair_edge = |t: &Tri| match kind {
        Penrose::P3 => (t.b, t.c),
        Penrose::P2 => (t.a, t.b),
    };
    let q = |p: P| ((p[0] * 1e5).round() as i64, (p[1] * 1e5).round() as i64);
    let key = |t: &Tri| {
        let (u, v) = pair_edge(t);
        let (ku, kv) = (q(u), q(v));
        (t.kind, ku.min(kv), ku.max(kv))
    };
    type Key = (u8, (i64, i64), (i64, i64));
    let mut first: HashMap<Key, usize> = HashMap::new();
    let mut tiles = Vec::new();
    for (i, t) in ts.iter().enumerate() {
        let k = key(t);
        if let Some(j) = first.remove(&k) {
            let u = &ts[j];
            // Axis from one end to the other. For P3, start at the apex of the half with
            // positive orientation, so corner 0 always comes from the same Robinson vertex.
            let (from, to) = match kind {
                Penrose::P3 if cross(sub(t.b, t.a), sub(t.c, t.a)) > 0.0 => (t.a, u.a),
                Penrose::P3 => (u.a, t.a),
                Penrose::P2 if t.kind == 0 => (t.a, t.b),
                Penrose::P2 => (t.b, t.a),
            };
            let shape = match kind {
                Penrose::P2 => t.kind,
                Penrose::P3 => t.kind ^ 1,
            };
            let d = sub(to, from);
            debug_assert!((len(d) - axis_len(kind, shape)).abs() < 1e-6);
            let m = Affine::rot(angle_of(d)).with_translation(from);
            tiles.push((shape as u16, m));
        } else {
            first.insert(k, i);
        }
    }
    tiles
}

// ---------------------------------------------------------------------------------------------
// Exact construction (Z[ζ₁₀]).
//
// Every corner is an integer combination of 10th roots of unity: the seed corners are φ^depth
// times roots of unity, and a deflation step only adds (q − p)/φ, with 1/φ = ζ² − ζ³. P3 corners
// lie on the 18°-rotated copy of the ring, so they are stored rotated by +18° ("w" coordinates)
// and drawn with a −18° frame.

use crate::exact::{Cyclo, Iso, Z4, Z10, zadd, zsub};

/// 1/φ = ζ² − ζ³ and φ = 1 + ζ² − ζ³ in Z[ζ₁₀].
const INV_PHI: Z4 = [0, 0, 1, -1];
const PHI_Z: Z4 = [1, 0, 1, -1];

#[derive(Clone, Copy, Debug)]
struct XTri {
    kind: u8,
    a: Z4,
    b: Z4,
    c: Z4,
}

fn xlerp(p: Z4, q: Z4) -> Z4 {
    zadd(p, Z10::mul(zsub(q, p), INV_PHI))
}

/// [`deflate_p3`] and [`deflate_p2`] for one triangle, exactly.
fn xdeflate(kind: Penrose, t: &XTri, out: &mut Vec<XTri>) {
    let (a, b, c) = (t.a, t.b, t.c);
    let tri = |kind, a, b, c| XTri { kind, a, b, c };
    match (kind, t.kind) {
        // [`deflate_p3`], exactly: division points measured from the same ends.
        (Penrose::P3, 0) => {
            let d = xlerp(a, c);
            out.push(tri(0, b, c, d));
            out.push(tri(1, d, a, b));
        }
        (Penrose::P3, _) => {
            let e = xlerp(c, b);
            let d = xlerp(c, a);
            out.push(tri(1, e, a, b));
            out.push(tri(1, d, c, e));
            out.push(tri(0, e, a, d));
        }
        (Penrose::P2, 0) => {
            let d = xlerp(a, b);
            let e = xlerp(c, a);
            out.push(tri(0, c, d, b));
            out.push(tri(0, c, d, e));
            out.push(tri(1, e, a, d));
        }
        (Penrose::P2, _) => {
            let x = xlerp(b, c);
            out.push(tri(0, b, a, x));
            out.push(tri(1, x, c, a));
        }
    }
}

/// Exact coordinates are drawn through `frame` (P3: a −18° turn; P2: none). Returns (pre, post)
/// with world = post ∘ iso ∘ pre for a tile placed by `iso` in exact coordinates.
pub fn exact_frame(kind: Penrose) -> (Affine, Affine) {
    match kind {
        Penrose::P2 => (Affine::ID, Affine::ID),
        Penrose::P3 => {
            let r = 18f64.to_radians();
            (Affine::rot(r), Affine::rot(-r))
        }
    }
}

/// The prototiles of [`shapes`] in exact coordinates (P3 turned by +18°).
pub fn exact_shapes(kind: Penrose) -> Vec<Vec<Z4>> {
    let z = |k: i64| Z10::unit(k);
    match kind {
        // Kite: 0, ζ, 1, ζ⁻¹. Dart: 0, ζ, 1/φ, ζ⁻¹.
        Penrose::P2 => vec![
            vec![[0; 4], z(1), z(0), z(-1)],
            vec![[0; 4], z(1), INV_PHI, z(-1)],
        ],
        // Thick: 0, ζ², ζ² + ζ⁻¹, ζ⁻¹. Thin: 0, ζ, 1 + ζ, 1.
        Penrose::P3 => vec![
            vec![[0; 4], z(2), zadd(z(2), z(-1)), z(-1)],
            vec![[0; 4], z(1), zadd(z(0), z(1)), z(0)],
        ],
    }
}

/// The tiles of [`generate`] at the same `k`, built exactly and only near the centre: triangles
/// whose float image lies farther than `radius` from the centre are not deflated further. Each
/// tile is (shape, exact placement of [`exact_shapes`]); tiles come in depth-first order.
pub fn generate_near_exact(kind: Penrose, k: u32, radius: f64) -> Result<Vec<(u16, Iso)>, String> {
    let mut out = Vec::new();
    generate_near_with(kind, k, radius, &mut |s, i| {
        out.push((s, i));
        Ok(())
    })?;
    Ok(out)
}

/// [`generate_near_exact`], passing each tile to `out`.
pub fn generate_near_with(
    kind: Penrose,
    k: u32,
    radius: f64,
    out: &mut dyn FnMut(u16, Iso) -> Result<(), String>,
) -> Result<(), String> {
    let (_, post) = exact_frame(kind);
    let world = |z: Z4| post.apply(Z10::to_f64(z));
    // Distance from the centre to a triangle (0 inside).
    let far = |t: &XTri| -> f64 {
        let (a, b, c) = (world(t.a), world(t.b), world(t.c));
        let o = [0.0, 0.0];
        let s1 = cross(sub(b, a), sub(o, a));
        let s2 = cross(sub(c, b), sub(o, b));
        let s3 = cross(sub(a, c), sub(o, c));
        if (s1 >= 0.0 && s2 >= 0.0 && s3 >= 0.0) || (s1 <= 0.0 && s2 <= 0.0 && s3 <= 0.0) {
            return 0.0;
        }
        let seg = |p: P, q: P| {
            let d = sub(q, p);
            let t = (dot(sub(o, p), d) / dot(d, d)).clamp(0.0, 1.0);
            len(add(p, scale(d, t)))
        };
        seg(a, b).min(seg(b, c)).min(seg(c, a))
    };
    generate_kept(kind, k, &|t| far(t) <= radius, out)
}

/// The tiles of [`generate`] at the same `k`, exactly, expanding only the Robinson triangles
/// for which `keep` holds (given their corners as float images of the exact coordinates, the
/// frame of [`exact_shapes`]; a triangle that is not kept is dropped with its descendants).
/// Tiles whose two halves were both kept are passed to `out`, in depth-first order.
pub fn generate_window(
    kind: Penrose,
    k: u32,
    keep: &dyn Fn([P; 3]) -> bool,
    out: &mut dyn FnMut(u16, Iso) -> Result<(), String>,
) -> Result<(), String> {
    let f = |z: Z4| Z10::to_f64(z);
    generate_kept(kind, k, &|t| keep([f(t.a), f(t.b), f(t.c)]), out)
}

fn generate_kept(
    kind: Penrose,
    k: u32,
    keep: &dyn Fn(&XTri) -> bool,
    out: &mut dyn FnMut(u16, Iso) -> Result<(), String>,
) -> Result<(), String> {
    let depth = 2 * k;
    let turn = (k % 2) as i64;
    let mut s: Z4 = [1, 0, 0, 0];
    for _ in 0..depth {
        s = Z10::mul(s, PHI_Z);
    }
    let unit = |j: i64| Z10::mul(s, Z10::unit(j + turn));
    let o = [0i64; 4];
    let mut seeds = Vec::new();
    match kind {
        Penrose::P3 => {
            // [`p3_seed`], exactly: 108° corners at s·ζ^(2j+1), tips at φ·s·ζ^(2j).
            for j in 0..5i64 {
                let tip = Z10::mul(unit(2 * j), PHI_Z);
                for corner in [unit(2 * j - 1), unit(2 * j + 1)] {
                    seeds.push(XTri {
                        kind: 1,
                        a: corner,
                        b: tip,
                        c: o,
                    });
                }
            }
        }
        Penrose::P2 => {
            for k5 in 0..5i64 {
                let tail = unit(2 * k5);
                for side in [-1, 1] {
                    seeds.push(XTri {
                        kind: 0,
                        a: o,
                        b: tail,
                        c: unit(2 * k5 + side),
                    });
                }
            }
        }
    }
    fn rec(kind: Penrose, t: XTri, left: u32, keep: &dyn Fn(&XTri) -> bool, out: &mut Vec<XTri>) {
        if !keep(&t) {
            return;
        }
        if left == 0 {
            out.push(t);
            return;
        }
        let mut kids = Vec::with_capacity(3);
        xdeflate(kind, &t, &mut kids);
        for c in kids {
            rec(kind, c, left - 1, keep, out);
        }
    }
    let protos = exact_shapes(kind);
    let mut first: crate::exact::FxMap<(u8, Z4, Z4), XTri> = Default::default();
    let mut tris = Vec::new();
    for seed in seeds {
        tris.clear();
        rec(kind, seed, depth, keep, &mut tris);
        for t in &tris {
            let (u, v) = match kind {
                Penrose::P3 => (t.b, t.c),
                Penrose::P2 => (t.a, t.b),
            };
            let key = (t.kind, u.min(v), u.max(v));
            let Some(w) = first.remove(&key) else {
                first.insert(key, *t);
                continue;
            };
            let (from, to) = match kind {
                Penrose::P3
                    if Z10::sign_im(Z10::mul(Z10::conj(zsub(t.b, t.a)), zsub(t.c, t.a)))
                        == std::cmp::Ordering::Greater =>
                {
                    (t.a, w.a)
                }
                Penrose::P3 => (w.a, t.a),
                Penrose::P2 if t.kind == 0 => (t.a, t.b),
                Penrose::P2 => (t.b, t.a),
            };
            let shape = match kind {
                Penrose::P2 => t.kind,
                Penrose::P3 => t.kind ^ 1,
            };
            let proto = &protos[shape as usize];
            let d = zsub(to, from);
            let rot = (0..10)
                .find(|&r| Z10::rotate(proto[2], r) == d)
                .ok_or("a Penrose tile's axis is not a turn of the prototile's")?;
            let iso = Iso {
                rot: rot as u8,
                refl: false,
                t: from,
            };
            // The placed prototile's corners are the two triangles' corners.
            let mut want = vec![t.a, t.b, t.c, w.a, w.b, w.c];
            want.sort_unstable();
            want.dedup();
            let mut got: Vec<Z4> = proto.iter().map(|&p| iso.apply::<Z10>(p)).collect();
            got.sort_unstable();
            if got != want {
                return Err("a Penrose tile does not match its two half-tiles".into());
            }
            out(shape as u16, iso)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    // The exact construction gives the float construction's tiles (as sets of corners).
    #[test]
    fn exact_matches_float() {
        for kind in [Penrose::P2, Penrose::P3] {
            for k in 1..=3 {
                let fl = generate(kind, k);
                let ex = generate_near_exact(kind, k, f64::INFINITY).unwrap();
                assert_eq!(fl.len(), ex.len());
                let (pre, post) = exact_frame(kind);
                let shp = shapes(kind);
                let key = |s: u16, m: &Affine| {
                    let c = centroid(
                        &shp[s as usize]
                            .iter()
                            .map(|&p| m.apply(p))
                            .collect::<Vec<_>>(),
                    );
                    (s, (c[0] * 1e6).round() as i64, (c[1] * 1e6).round() as i64)
                };
                let mut a: Vec<_> = fl.iter().map(|(s, m)| key(*s, m)).collect();
                let mut b: Vec<_> = ex
                    .iter()
                    .map(|(s, i)| key(*s, &post.mul(&i.to_affine::<Z10>()).mul(&pre)))
                    .collect();
                a.sort_unstable();
                b.sort_unstable();
                assert_eq!(a, b, "{k}");
                // Transforms agree too, tile for tile by position.
                for (s, i) in &ex {
                    let m = post.mul(&i.to_affine::<Z10>()).mul(&pre);
                    let f = fl.iter().find(|(t, n)| {
                        t == s
                            && dist(n.translation(), m.translation()) < 1e-9
                            && dist(n.apply([1.0, 0.0]), m.apply([1.0, 0.0])) < 1e-9
                    });
                    assert!(f.is_some());
                }
            }
        }
    }

    #[test]
    fn deflation_keeps_area() {
        for kind in [Penrose::P2, Penrose::P3] {
            let area_of = |ts: &[Tri]| -> f64 { ts.iter().map(|t| area(&[t.a, t.b, t.c])).sum() };
            let mut ts = vec![Tri {
                kind: 0,
                a: [0.0, 0.0],
                b: deg(0.0),
                c: deg(36.0),
            }];
            if kind == Penrose::P3 {
                ts[0] = Tri {
                    kind: 0,
                    a: [0.0, 0.0],
                    b: deg(-18.0),
                    c: deg(18.0),
                };
            }
            let a0 = area_of(&ts);
            for _ in 0..4 {
                ts = match kind {
                    Penrose::P3 => deflate_p3(&ts),
                    Penrose::P2 => deflate_p2(&ts),
                };
            }
            assert!((area_of(&ts) - a0).abs() < 1e-9);
        }
    }
}
