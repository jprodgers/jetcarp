//! Hat and Penrose size classes built with exact geometry (levels 7 and 8, and any size).
//!
//! A size class is the `n` tiles nearest a fixed centre of one substitution tiling
//! ([`crate::aperiodic`]). Here the tiles near the centre are generated with pruning (only
//! supertiles that come near the centre are expanded), placed exactly (hats snapped to the
//! Eisenstein lattice their corners lie on and checked; Penrose tiles deflated in Z[ζ₁₀]),
//! ordered by the exact squared distance of their vertex average from the centre (ties by
//! generation order), and checked to be complete: every tile of the infinite tiling within the
//! chosen radius was generated (the generated patch's boundary is farther than the farthest
//! chosen tile plus a tile diameter). The neighbour graph is [`crate::exact_graph`].

use crate::aperiodic::BuiltPatch;
use crate::exact::{Cyclo, Iso, OPEN, Place, Z4, Z10, Z12, zadd, zsub};
use crate::exact_graph::{ExactShape, ExactTiles};
use crate::geom::*;
use crate::penrose::{self, Penrose};
use crate::topology::Placements;
use std::cmp::Ordering;

/// A family's exact candidates: shapes (float and exact), frames, and a generator of the tiles
/// near the centre for a substitution depth `k` and radius.
struct Family {
    float_shapes: Vec<Vec<P>>,
    shapes: Vec<ExactShape>,
    ring: u8,
    pre: Affine,
    post: Affine,
    /// Exact centre × the number of corners of shape 0 (all shapes of a family have the same
    /// number of corners), in exact coordinates.
    centre: Z4,
    /// Mean tile area (world units), for choosing the radius.
    area: f64,
    /// Smallest substitution depth to try.
    k0: u32,
}

fn hat_family() -> Family {
    let hex = |(x, y): (i64, i64)| -> Z4 { [x, 0, y, 0] };
    let corners: Vec<Z4> = crate::hat::HAT_HEX.iter().map(|&c| hex(c)).collect();
    let m = corners.len();
    // Edges of length 2 (Eisenstein norm 4) take other hats' corners at their midpoints.
    let split = (0..m)
        .map(|i| {
            let (a, b) = (crate::hat::HAT_HEX[i], crate::hat::HAT_HEX[(i + 1) % m]);
            let (dx, dy) = (b.0 - a.0, b.1 - a.1);
            dx * dx + dx * dy + dy * dy == 4
        })
        .collect();
    let outline = crate::hat::hat_outline();
    let (_, c) = crate::hat::generate(1);
    Family {
        float_shapes: vec![outline.clone()],
        shapes: vec![ExactShape { corners, split }],
        ring: 12,
        pre: Affine::ID,
        post: Affine::trans(-c[0], -c[1]),
        centre: snap_eisenstein(scale(c, m as f64)).expect("hat centre"),
        area: area(&outline),
        k0: 6,
    }
}

fn penrose_family(kind: Penrose) -> Family {
    let float_shapes = penrose::shapes(kind);
    let exact = penrose::exact_shapes(kind);
    let (pre, post) = penrose::exact_frame(kind);
    let (a0, a1) = (area(&float_shapes[0]), area(&float_shapes[1]));
    let phi = penrose::PHI;
    Family {
        float_shapes,
        shapes: exact
            .into_iter()
            .map(|corners| ExactShape {
                split: vec![false; corners.len()],
                corners,
            })
            .collect(),
        ring: 10,
        pre,
        post,
        centre: [0; 4],
        area: (phi * a0 + a1) / (phi + 1.0),
        k0: 4,
    }
}

/// The Eisenstein integer a + b ω (ω = e^{iπ/3}) at a float point, if it is within 1e-6.
fn snap_eisenstein(p: P) -> Option<Z4> {
    let b = (2.0 * p[1] / S3).round();
    let a = (p[0] - b / 2.0).round();
    let q = [a + b / 2.0, b * S3 / 2.0];
    (dist(p, q) < 1e-6).then_some([a as i64, 0, b as i64, 0])
}

/// A float hat transform as an exact placement (rotation by a multiple of 30°, maybe a
/// reflection, translation on the Eisenstein lattice), checked against the float map.
fn snap_hat(m: &Affine) -> Result<Iso, String> {
    let refl = m.det() < 0.0;
    let ang = crate::detmath::atan2(m.0[3], m.0[0]);
    let r = (ang / (std::f64::consts::PI / 6.0)).round();
    let t = snap_eisenstein(m.translation()).ok_or("a hat is off the lattice")?;
    let iso = Iso {
        rot: (r as i64).rem_euclid(12) as u8,
        refl,
        t,
    };
    let back = iso.to_affine::<Z12>();
    if (0..6).any(|k| (back.0[k] - m.0[k]).abs() > 1e-6) {
        return Err(format!("a hat transform is not a lattice motion: {m:?}"));
    }
    Ok(iso)
}

/// Candidates near the centre for depth `k`: shapes and exact placements, in generation order.
fn candidates(id: &str, k: u32, radius: f64) -> Result<(Vec<u16>, Vec<Place>), String> {
    let (mut shape, mut places) = (Vec::new(), Vec::new());
    let fit = |i: Iso| {
        i.compact()
            .ok_or_else(|| "coordinates overflow 32 bits".to_string())
    };
    match id {
        "hat" => {
            let mid = centroid(&crate::hat::hat_outline());
            let mut err = None;
            let mut keep = |m: Affine, c: P| {
                if err.is_some() || dist(m.apply(mid), c) > radius {
                    return;
                }
                match snap_hat(&m).and_then(fit) {
                    Ok(p) => {
                        shape.push(0u16);
                        places.push(p);
                    }
                    Err(e) => err = Some(e),
                }
            };
            let (_, c) = crate::hat::generate(1);
            crate::hat::generate_near(k, radius, 8.0, &mut |m| keep(m, c));
            if let Some(e) = err {
                return Err(e);
            }
        }
        "penrose-kd" | "penrose-rhomb" => {
            let kind = if id == "penrose-kd" {
                Penrose::P2
            } else {
                Penrose::P3
            };
            penrose::generate_near_with(kind, k, radius, &mut |s, i| {
                shape.push(s);
                places.push(fit(i)?);
                Ok(())
            })?;
        }
        _ => return Err(format!("no exact size classes for '{id}'")),
    }
    Ok((shape, places))
}

/// Exact squared distance from the centre, as an element of the real subring, scaled by the
/// square of the corner count: |Σ corners − centre·m|².
fn norm_key<C: Cyclo>(f: &Family, shape: u16, place: &Place) -> Z4 {
    let s = &f.shapes[shape as usize].corners;
    let iso = place.iso();
    let mut sum = [0i64; 4];
    for &z in s {
        sum = zadd(sum, iso.apply::<C>(z));
    }
    let d = zsub(sum, f.centre);
    C::mul(d, C::conj(d))
}

fn cmp_real<C: Cyclo>(a: Z4, b: Z4) -> Ordering {
    C::sign_re(zsub(a, b))
}

/// Build the `n` tiles of `id` nearest its centre, exactly.
pub fn build(id: &str, n: usize) -> Result<BuiltPatch, String> {
    match id {
        "hat" => build_with::<Z12>(id, hat_family(), n),
        "penrose-kd" => build_with::<Z10>(id, penrose_family(Penrose::P2), n),
        "penrose-rhomb" => build_with::<Z10>(id, penrose_family(Penrose::P3), n),
        _ => Err(format!("no exact size classes for '{id}'")),
    }
}

fn build_with<C: Cyclo>(id: &str, f: Family, n: usize) -> Result<BuiltPatch, String> {
    use rayon::prelude::*;
    let diam = f
        .float_shapes
        .iter()
        .map(|s| {
            let c = centroid(s);
            2.0 * s.iter().map(|&p| dist(p, c)).fold(0.0, f64::max)
        })
        .fold(0.0, f64::max);
    let to_world = |iso: &Iso| -> Affine { f.post.mul(&iso.to_affine::<C>()).mul(&f.pre) };
    let mids: Vec<P> = f.float_shapes.iter().map(|s| centroid(s)).collect();
    // Radius holding about n tiles, with room for the ragged rim.
    let mut radius = (n as f64 * f.area / std::f64::consts::PI).sqrt() * 1.04 + 4.0 * diam;
    let mut last = String::new();
    for k in f.k0..f.k0 + 8 {
        let (shape, places) = candidates(id, k, radius)?;
        if places.len() < n {
            last = format!(
                "depth {k}: {} tiles within radius {radius:.0}",
                places.len()
            );
            continue;
        }
        // The generated patch's boundary: how far from the centre it is complete.
        let reach = {
            let tiles = ExactTiles::<C>::new(&f.shapes, &shape, &places)?;
            let partner = tiles.pairs()?;
            let st = partner.len() / places.len();
            (0..places.len())
                .into_par_iter()
                .map(|t| {
                    let m = shape[t] as usize;
                    let d = f.shapes[m].corners.len()
                        + f.shapes[m].split.iter().filter(|&&x| x).count();
                    let mut best = f64::INFINITY;
                    for k in 0..d {
                        if partner[t * st + k] == OPEN {
                            let a = f.post.apply(C::to_f64(tiles.corner(t, k)));
                            let b = f.post.apply(C::to_f64(tiles.corner(t, (k + 1) % d)));
                            let e = sub(b, a);
                            let s = (-dot(a, e) / dot(e, e)).clamp(0.0, 1.0);
                            best = best.min(len(add(a, scale(e, s))));
                        }
                    }
                    best
                })
                .reduce(|| f64::INFINITY, f64::min)
        };
        // Nearest first: by float distance, exactly where floats are close.
        let dist0: Vec<f64> = (0..places.len())
            .into_par_iter()
            .map(|i| {
                let s = shape[i] as usize;
                len(to_world(&places[i].iso()).apply(mids[s]))
            })
            .collect();
        let mut order: Vec<u32> = (0..places.len() as u32).collect();
        order.par_sort_by(|&a, &b| {
            let (fa, fb) = (dist0[a as usize], dist0[b as usize]);
            if (fa - fb).abs() > 1e-9 * (1.0 + fa.max(fb)) {
                fa.total_cmp(&fb)
            } else {
                let ka = norm_key::<C>(&f, shape[a as usize], &places[a as usize]);
                let kb = norm_key::<C>(&f, shape[b as usize], &places[b as usize]);
                cmp_real::<C>(ka, kb)
            }
            .then(a.cmp(&b))
        });
        let rn = dist0[order[n - 1] as usize];
        drop(dist0);
        if rn + diam >= reach {
            last = format!(
                "depth {k}: the {n}th tile is {rn:.1} from the centre, the generated patch only complete to {reach:.1}"
            );
            // The substitution region may be too small, or the radius: widen both.
            radius = radius.max(rn + 3.0 * diam);
            continue;
        }
        order.truncate(n);
        let sel_places: Vec<Place> = order.iter().map(|&i| places[i as usize]).collect();
        drop(places);
        let sel_shape: Vec<u16> = order.iter().map(|&i| shape[i as usize]).collect();
        drop(shape);
        drop(order);
        let tiles = ExactTiles::<C>::new(&f.shapes, &sel_shape, &sel_places)?;
        let partner = tiles.pairs()?;
        let (touch, edge, open) = tiles.graph(&partner);
        drop(partner);
        drop(tiles);
        return Ok(BuiltPatch {
            shapes: f.float_shapes.clone(),
            shape: sel_shape,
            placements: Placements::Exact {
                ring: f.ring,
                pre: f.pre,
                post: f.post,
                places: sel_places,
            },
            touch,
            edge,
            open,
        });
    }
    Err(format!("could not build {n} {id} tiles exactly ({last})"))
}

/// Exact verification of a patch built here (see [`crate::exact::verify_patch`]).
pub fn verify(p: &BuiltPatch, id: &str) -> Result<crate::exact::PatchStats, String> {
    let Placements::Exact { places, .. } = &p.placements else {
        return Err("not an exact patch".into());
    };
    let f = match id {
        "hat" => hat_family(),
        "penrose-kd" => penrose_family(Penrose::P2),
        _ => penrose_family(Penrose::P3),
    };
    if f.ring == 12 {
        let t = ExactTiles::<Z12>::new(&f.shapes, &p.shape, places)?;
        t.verify(&t.pairs()?)
    } else {
        let t = ExactTiles::<Z10>::new(&f.shapes, &p.shape, places)?;
        t.verify(&t.pairs()?)
    }
}

fn family_of(id: &str) -> Result<Family, String> {
    match id {
        "hat" => Ok(hat_family()),
        "penrose-kd" => Ok(penrose_family(Penrose::P2)),
        "penrose-rhomb" => Ok(penrose_family(Penrose::P3)),
        _ => Err(format!("no exact tiles for '{id}'")),
    }
}

/// A patch of hat or Penrose tiles at exact placements (any set of tiles of the family's
/// tiling, such as a sliding window's, [`crate::window`]), with its exact neighbour graph, in
/// the frame and neighbour order of the size classes.
pub fn from_places(id: &str, shape: Vec<u16>, places: Vec<Place>) -> Result<BuiltPatch, String> {
    let f = family_of(id)?;
    let (touch, edge, open) = if f.ring == 12 {
        let t = ExactTiles::<Z12>::new(&f.shapes, &shape, &places)?;
        t.graph(&t.pairs()?)
    } else {
        let t = ExactTiles::<Z10>::new(&f.shapes, &shape, &places)?;
        t.graph(&t.pairs()?)
    };
    Ok(BuiltPatch {
        shapes: f.float_shapes,
        shape,
        placements: Placements::Exact {
            ring: f.ring,
            pre: f.pre,
            post: f.post,
            places,
        },
        touch,
        edge,
        open,
    })
}

/// The exact prototiles of a family (shape order) and its drawing frame: a tile placed by
/// `iso` is drawn by `post ∘ iso ∘ pre`.
pub fn exact_frame(id: &str) -> Result<(Vec<ExactShape>, Affine, Affine), String> {
    let f = family_of(id)?;
    Ok((f.shapes, f.pre, f.post))
}

/// Indices of `tiles` in the size classes' order: nearest the centre first (by float distance,
/// exactly where floats are close), ties in the given order.
pub(crate) fn size_class_order(id: &str, tiles: &[(u16, Place)]) -> Result<Vec<u32>, String> {
    let f = family_of(id)?;
    let mids: Vec<P> = f.float_shapes.iter().map(|s| centroid(s)).collect();
    let to_world = |iso: &Iso| -> Affine {
        if f.ring == 12 {
            f.post.mul(&iso.to_affine::<Z12>()).mul(&f.pre)
        } else {
            f.post.mul(&iso.to_affine::<Z10>()).mul(&f.pre)
        }
    };
    let d: Vec<f64> = tiles
        .iter()
        .map(|(s, p)| len(to_world(&p.iso()).apply(mids[*s as usize])))
        .collect();
    let mut order: Vec<u32> = (0..tiles.len() as u32).collect();
    order.sort_by(|&a, &b| {
        let (fa, fb) = (d[a as usize], d[b as usize]);
        if (fa - fb).abs() > 1e-9 * (1.0 + fa.max(fb)) {
            fa.total_cmp(&fb)
        } else {
            let (sa, pa) = &tiles[a as usize];
            let (sb, pb) = &tiles[b as usize];
            if f.ring == 12 {
                cmp_real::<Z12>(norm_key::<Z12>(&f, *sa, pa), norm_key::<Z12>(&f, *sb, pb))
            } else {
                cmp_real::<Z10>(norm_key::<Z10>(&f, *sa, pa), norm_key::<Z10>(&f, *sb, pb))
            }
        }
        .then(a.cmp(&b))
    });
    Ok(order)
}

/// [`snap_hat`] for the sliding window, whose hierarchy goes many levels deeper than the size
/// classes': the float translation may be off by up to `tol` (lattice points are a unit apart;
/// a wrong snap would show as overlapping or unmatched edges in the exact graph). Returns the
/// placement and the translation's distance from it.
pub(crate) fn snap_hat_place(m: &Affine, tol: f64) -> Result<(Iso, f64), String> {
    let refl = m.det() < 0.0;
    let ang = crate::detmath::atan2(m.0[3], m.0[0]);
    let r = (ang / (std::f64::consts::PI / 6.0)).round();
    let p = m.translation();
    let b = (2.0 * p[1] / S3).round();
    let a = (p[0] - b / 2.0).round();
    let q = [a + b / 2.0, b * S3 / 2.0];
    let off = dist(p, q);
    if off > tol {
        return Err(format!(
            "a hat is {off:.2e} off the lattice (the float hierarchy is too deep)"
        ));
    }
    let iso = Iso {
        rot: (r as i64).rem_euclid(12) as u8,
        refl,
        t: [a as i64, 0, b as i64, 0],
    };
    let back = iso.to_affine::<Z12>();
    if [0, 1, 3, 4]
        .iter()
        .any(|&k| (back.0[k] - m.0[k]).abs() > 1e-6)
    {
        return Err(format!("a hat transform is not a lattice motion: {m:?}"));
    }
    Ok((iso, off))
}
