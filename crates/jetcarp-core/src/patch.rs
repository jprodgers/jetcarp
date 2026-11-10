//! Neighbour graphs for explicit patches of polygons (aperiodic tilings).
//!
//! The Spectre has its own exact corner-matching fast path (`spectre::spectre_graph`). Every other
//! explicit patch goes through [`build_graph`], which measures contact between nearby polygons with
//! [`crate::topology::contact`], so it also handles T-junctions (a corner landing mid-edge).

use crate::geom::*;
use crate::topology::contact;

/// Neighbour lists in clockwise order on a y-down screen, plus open-boundary flags.
pub struct PatchGraph {
    pub edge: Vec<Vec<u32>>,
    pub touch: Vec<Vec<u32>>,
    /// Tiles with some stretch of boundary not shared with another tile.
    pub open: Vec<bool>,
}

/// Contact tolerance relative to the typical edge length.
fn tolerance(polys: &[Vec<P>]) -> f64 {
    let mut lens: Vec<f64> = polys
        .iter()
        .take(64)
        .flat_map(|p| (0..p.len()).map(move |i| dist(p[i], p[(i + 1) % p.len()])))
        .filter(|l| *l > 0.0)
        .collect();
    lens.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let typical = lens.get(lens.len() / 2).copied().unwrap_or(1.0);
    (typical * 1e-4).max(1e-9)
}

/// Build edge and touch neighbour lists for a set of polygons that tile a region without overlap.
pub fn build_graph(polys: &[Vec<P>]) -> PatchGraph {
    let n = polys.len();
    let eps = tolerance(polys);
    let boxes: Vec<[f64; 4]> = polys.iter().map(|p| bbox(p)).collect();
    // Uniform grid over bounding boxes.
    let mut world = [
        f64::INFINITY,
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::NEG_INFINITY,
    ];
    let mut size = 0.0f64;
    for b in &boxes {
        world = [
            world[0].min(b[0]),
            world[1].min(b[1]),
            world[2].max(b[2]),
            world[3].max(b[3]),
        ];
        size = size.max(b[2] - b[0]).max(b[3] - b[1]);
    }
    let cell = size.max(eps) * 1.01;
    let cols = (((world[2] - world[0]) / cell).ceil() as usize).max(1);
    let rows = (((world[3] - world[1]) / cell).ceil() as usize).max(1);
    let mut buckets: Vec<Vec<u32>> = vec![Vec::new(); cols * rows];
    let at = |x: f64, y: f64| -> (usize, usize) {
        (
            (((x - world[0]) / cell).floor().max(0.0) as usize).min(cols - 1),
            (((y - world[1]) / cell).floor().max(0.0) as usize).min(rows - 1),
        )
    };
    for (i, b) in boxes.iter().enumerate() {
        let (c, r) = at((b[0] + b[2]) / 2.0, (b[1] + b[3]) / 2.0);
        buckets[r * cols + c].push(i as u32);
    }
    use rayon::prelude::*;
    // Per tile: (position along own boundary, other tile, shares an edge).
    type Contact = ((i64, i64), u32, bool);
    let contacts: Vec<Vec<Contact>> = (0..n)
        .into_par_iter()
        .map(|i| {
            let bi = boxes[i];
            let (c, r) = at((bi[0] + bi[2]) / 2.0, (bi[1] + bi[3]) / 2.0);
            let mut out = Vec::new();
            for rr in r.saturating_sub(1)..=(r + 1).min(rows - 1) {
                for cc in c.saturating_sub(1)..=(c + 1).min(cols - 1) {
                    for &j in &buckets[rr * cols + cc] {
                        let j = j as usize;
                        if j == i {
                            continue;
                        }
                        let bj = boxes[j];
                        if bi[0] > bj[2] + eps
                            || bj[0] > bi[2] + eps
                            || bi[1] > bj[3] + eps
                            || bj[1] > bi[3] + eps
                        {
                            continue;
                        }
                        let (k, pos) = contact(&polys[i], &polys[j], eps);
                        if k >= 1 {
                            // Quantised and wrapped (see `topology::build_lattice`): float
                            // noise never reorders; tiles at one corner go by angle.
                            let m = polys[i].len() as i64 * 1_000_000;
                            let qpos = ((pos * 1e6).round() as i64).rem_euclid(m);
                            let ang = crate::geom::angle_of(crate::geom::sub(
                                centroid(&polys[j]),
                                centroid(&polys[i]),
                            ));
                            out.push(((qpos, (ang * 1e6).round() as i64), j as u32, k == 2));
                        }
                    }
                }
            }
            out.sort_by_key(|a| (a.0, a.1));
            if signed_area2(&polys[i]) < 0.0 {
                out.reverse();
            }
            out
        })
        .collect();
    let touch: Vec<Vec<u32>> = contacts
        .iter()
        .map(|l| l.iter().map(|x| x.1).collect())
        .collect();
    let edge: Vec<Vec<u32>> = contacts
        .iter()
        .map(|l| l.iter().filter(|x| x.2).map(|x| x.1).collect())
        .collect();
    let open: Vec<bool> = (0..n)
        .into_par_iter()
        .map(|i| {
            !boundary_covered(
                &polys[i],
                edge[i].iter().map(|&j| &polys[j as usize][..]),
                eps,
            )
        })
        .collect();
    PatchGraph { edge, touch, open }
}

/// Is every stretch of `p`'s boundary shared with one of `others`?
fn boundary_covered<'a>(p: &[P], others: impl Iterator<Item = &'a [P]> + Clone, eps: f64) -> bool {
    let m = p.len();
    for i in 0..m {
        let (a, b) = (p[i], p[(i + 1) % m]);
        let l = dist(a, b);
        if l < eps {
            continue;
        }
        let d = scale(sub(b, a), 1.0 / l);
        let mut spans: Vec<(f64, f64)> = Vec::new();
        for q in others.clone() {
            for j in 0..q.len() {
                let (c, e) = (q[j], q[(j + 1) % q.len()]);
                let off = |x: P| cross(d, sub(x, a));
                if off(c).abs() > eps || off(e).abs() > eps {
                    continue;
                }
                let (t1, t2) = (dot(d, sub(c, a)), dot(d, sub(e, a)));
                let (lo, hi) = (t1.min(t2).max(0.0), t1.max(t2).min(l));
                if hi > lo {
                    spans.push((lo, hi));
                }
            }
        }
        spans.sort_by(|x, y| x.0.partial_cmp(&y.0).unwrap());
        let mut reach = 0.0;
        for (lo, hi) in spans {
            if lo > reach + eps * 10.0 {
                return false;
            }
            reach = f64::max(reach, hi);
        }
        if reach < l - eps * 10.0 {
            return false;
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unit_squares_with_t_junction() {
        // A 2x1 brick above two unit squares: the brick's bottom edge meets both squares.
        let sq = |x: f64, y: f64, w: f64| vec![[x, y], [x + w, y], [x + w, y + 1.0], [x, y + 1.0]];
        let polys = vec![sq(0.0, 0.0, 1.0), sq(1.0, 0.0, 1.0), sq(0.0, 1.0, 2.0)];
        let g = build_graph(&polys);
        assert_eq!(g.edge[2].len(), 2);
        assert_eq!(g.edge[0].len(), 2);
        assert!(g.open.iter().all(|&o| o));
    }
}
