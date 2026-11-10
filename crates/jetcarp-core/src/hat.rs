//! The hat monotile (Smith, Myers, Kaplan, Goodman-Strauss 2023) by its H/T/P/F metatile
//! substitution.
//!
//! Ported from Craig S. Kaplan's `hatviz` (`hat.js`, `geometry.js`;
//! <https://github.com/isohedral/hatviz>), BSD 3-Clause licence, Copyright (c) 2023, Craig S. Kaplan:
//!
//! > Redistribution and use in source and binary forms, with or without modification, are
//! > permitted provided that the following conditions are met: 1. Redistributions of source code
//! > must retain the above copyright notice, this list of conditions and the following disclaimer.
//! > 2. Redistributions in binary form must reproduce the above copyright notice, this list of
//! > conditions and the following disclaimer in the documentation and/or other materials provided
//! > with the distribution. 3. Neither the name of the copyright holder nor the names of its
//! > contributors may be used to endorse or promote products derived from this software without
//! > specific prior written permission.
//! >
//! > THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS IS" AND ANY EXPRESS
//! > OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE IMPLIED WARRANTIES OF
//! > MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE DISCLAIMED. IN NO EVENT SHALL THE
//! > COPYRIGHT HOLDER OR CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL,
//! > EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE
//! > GOODS OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED
//! > AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING
//! > NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED
//! > OF THE POSSIBILITY OF SUCH DAMAGE.
//!
//! Each metatile is an outline plus children placed by affine maps; the four level-1 metatiles
//! hold hats (the one reflected hat sits in H). A level step assembles a 29-metatile patch by
//! gluing outline edges, then reads the four next-level metatiles off it. Hats keep their size,
//! so every level is exact.

use crate::geom::*;
use std::rc::Rc;

const HR3: f64 = S3 / 2.0;

fn hex_pt(x: f64, y: f64) -> P {
    [x + 0.5 * y, HR3 * y]
}

/// The hat in hexagonal-grid coordinates: 13 corners, edges of length 1 and √3.
pub fn hat_outline() -> Vec<P> {
    HAT_HEX
        .iter()
        .map(|&(x, y)| hex_pt(x as f64, y as f64))
        .collect()
}

/// [`hat_outline`]'s corners as integer hexagonal-grid coordinates (x, y): the point
/// x + y ω with ω = e^{iπ/3}.
pub const HAT_HEX: [(i64, i64); 13] = [
    (0, 0),
    (-1, -1),
    (0, -2),
    (2, -2),
    (2, -1),
    (4, -2),
    (5, -1),
    (4, 0),
    (3, 0),
    (2, 2),
    (0, 3),
    (0, 2),
    (-1, 2),
];

#[derive(Clone)]
enum Geom {
    /// A hat leaf (the transform may reflect it: Kaplan's "H1" hat in each H).
    Hat,
    Meta(Rc<Meta>),
}

struct Meta {
    shape: Vec<P>,
    children: Vec<(Affine, Geom)>,
}

impl Geom {
    fn shape(&self) -> &[P] {
        match self {
            Geom::Hat => unreachable!("hat leaves are never glued by outline"),
            Geom::Meta(m) => &m.shape,
        }
    }
}

impl Meta {
    fn eval_child(&self, n: usize, i: usize) -> P {
        let (t, g) = &self.children[n];
        t.apply(g.shape()[i])
    }
    /// Move the outline's vertex average to the origin.
    fn recentre(&mut self) {
        let c = centroid(&self.shape);
        for p in &mut self.shape {
            *p = sub(*p, c);
        }
        let m = Affine::trans(-c[0], -c[1]);
        for ch in &mut self.children {
            ch.0 = m.mul(&ch.0);
        }
    }
}

/// Map the unit interval onto segment p→q (rotation and scale).
fn match_seg(p: P, q: P) -> Affine {
    Affine([
        q[0] - p[0],
        p[1] - q[1],
        p[0],
        q[1] - p[1],
        q[0] - p[0],
        p[1],
    ])
}

/// Map segment p1→q1 onto segment p2→q2.
fn match_two(p1: P, q1: P, p2: P, q2: P) -> Affine {
    match_seg(p2, q2).mul(&match_seg(p1, q1).inv())
}

fn rot_about_m(p: P, ang: f64) -> Affine {
    Affine::trans(p[0], p[1])
        .mul(&Affine::rot(ang))
        .mul(&Affine::trans(-p[0], -p[1]))
}

/// Intersection of the lines through p1, q1 and p2, q2.
fn intersect(p1: P, q1: P, p2: P, q2: P) -> P {
    let d = (q2[1] - p2[1]) * (q1[0] - p1[0]) - (q2[0] - p2[0]) * (q1[1] - p1[1]);
    let ua = ((q2[0] - p2[0]) * (p1[1] - p2[1]) - (q2[1] - p2[1]) * (p1[0] - p2[0])) / d;
    [p1[0] + ua * (q1[0] - p1[0]), p1[1] + ua * (q1[1] - p1[1])]
}

fn pt(x: f64, y: f64) -> P {
    [x, y]
}

/// The level-1 metatiles [H, T, P, F].
fn initial() -> [Rc<Meta>; 4] {
    let hat = hat_outline();
    let h_out = vec![
        pt(0.0, 0.0),
        pt(4.0, 0.0),
        pt(4.5, HR3),
        pt(2.5, 5.0 * HR3),
        pt(1.5, 5.0 * HR3),
        pt(-0.5, HR3),
    ];
    let h = Meta {
        children: vec![
            (match_two(hat[5], hat[7], h_out[5], h_out[0]), Geom::Hat),
            (match_two(hat[9], hat[11], h_out[1], h_out[2]), Geom::Hat),
            (match_two(hat[5], hat[7], h_out[3], h_out[4]), Geom::Hat),
            (
                Affine::trans(2.5, HR3).mul(
                    &Affine([-0.5, -HR3, 0.0, HR3, -0.5, 0.0])
                        .mul(&Affine([0.5, 0.0, 0.0, 0.0, -0.5, 0.0])),
                ),
                Geom::Hat,
            ),
        ],
        shape: h_out,
    };
    let t = Meta {
        shape: vec![pt(0.0, 0.0), pt(3.0, 0.0), pt(1.5, 3.0 * HR3)],
        children: vec![(Affine([0.5, 0.0, 0.5, 0.0, 0.5, HR3]), Geom::Hat)],
    };
    let second = Affine::trans(0.0, 2.0 * HR3)
        .mul(&Affine([0.5, HR3, 0.0, -HR3, 0.5, 0.0]).mul(&Affine([0.5, 0.0, 0.0, 0.0, 0.5, 0.0])));
    let p = Meta {
        shape: vec![
            pt(0.0, 0.0),
            pt(4.0, 0.0),
            pt(3.0, 2.0 * HR3),
            pt(-1.0, 2.0 * HR3),
        ],
        children: vec![
            (Affine([0.5, 0.0, 1.5, 0.0, 0.5, HR3]), Geom::Hat),
            (second, Geom::Hat),
        ],
    };
    let f = Meta {
        shape: vec![
            pt(0.0, 0.0),
            pt(3.0, 0.0),
            pt(3.5, HR3),
            pt(3.0, 2.0 * HR3),
            pt(-1.0, 2.0 * HR3),
        ],
        children: vec![
            (Affine([0.5, 0.0, 1.5, 0.0, 0.5, HR3]), Geom::Hat),
            (second, Geom::Hat),
        ],
    };
    [Rc::new(h), Rc::new(t), Rc::new(p), Rc::new(f)]
}

/// One rule of the patch construction (Kaplan's `constructPatch` table).
enum Rule {
    /// Place this metatile at the identity.
    First(usize),
    /// Glue edge `edge` of new metatile `shape` to edge `ch_edge` of child `ch` (reversed).
    Edge {
        ch: usize,
        ch_edge: usize,
        shape: usize,
        edge: usize,
    },
    /// Glue edge `edge` of `shape` to the segment from corner `qi` of child `q` to corner `pi`
    /// of child `p`.
    Span {
        p: usize,
        pi: usize,
        q: usize,
        qi: usize,
        shape: usize,
        edge: usize,
    },
}

const H: usize = 0;
const T: usize = 1;
const PP: usize = 2;
const F: usize = 3;

fn rules() -> Vec<Rule> {
    use Rule::*;
    let e = |ch, ch_edge, shape, edge| Edge {
        ch,
        ch_edge,
        shape,
        edge,
    };
    vec![
        First(H),
        e(0, 0, PP, 2),
        e(1, 0, H, 2),
        e(2, 0, PP, 2),
        e(3, 0, H, 2),
        e(4, 4, PP, 2),
        e(0, 4, F, 3),
        e(2, 4, F, 3),
        Span {
            p: 4,
            pi: 1,
            q: 3,
            qi: 2,
            shape: F,
            edge: 0,
        },
        e(8, 3, H, 0),
        e(9, 2, PP, 0),
        e(10, 2, H, 0),
        e(11, 4, PP, 2),
        e(12, 0, H, 2),
        e(13, 0, F, 3),
        e(14, 2, F, 1),
        e(15, 3, H, 4),
        e(8, 2, F, 1),
        e(17, 3, H, 0),
        e(18, 2, PP, 0),
        e(19, 2, H, 2),
        e(20, 4, F, 3),
        e(20, 0, PP, 2),
        e(22, 0, H, 2),
        e(23, 4, F, 3),
        e(23, 0, F, 3),
        e(16, 0, PP, 2),
        Span {
            p: 9,
            pi: 4,
            q: 0,
            qi: 2,
            shape: T,
            edge: 2,
        },
        e(4, 0, F, 3),
    ]
}

fn construct_patch(tiles: &[Rc<Meta>; 4]) -> Meta {
    let mut ret = Meta {
        shape: vec![],
        children: vec![],
    };
    for r in rules() {
        let (m, s) = match r {
            Rule::First(s) => (Affine::ID, s),
            Rule::Edge {
                ch,
                ch_edge,
                shape,
                edge,
            } => {
                let (t, g) = &ret.children[ch];
                let poly = g.shape();
                let p = t.apply(poly[(ch_edge + 1) % poly.len()]);
                let q = t.apply(poly[ch_edge]);
                let np = &tiles[shape].shape;
                (match_two(np[edge], np[(edge + 1) % np.len()], p, q), shape)
            }
            Rule::Span {
                p,
                pi,
                q,
                qi,
                shape,
                edge,
            } => {
                let pp = ret.eval_child(q, qi);
                let qq = ret.eval_child(p, pi);
                let np = &tiles[shape].shape;
                (
                    match_two(np[edge], np[(edge + 1) % np.len()], pp, qq),
                    shape,
                )
            }
        };
        ret.children.push((m, Geom::Meta(tiles[s].clone())));
    }
    ret
}

/// The next level's [H, T, P, F].
fn construct_metatiles(patch: &Meta) -> [Rc<Meta>; 4] {
    use std::f64::consts::PI;
    let ev = |n, i| patch.eval_child(n, i);
    let bps1 = ev(8, 2);
    let bps2 = ev(21, 2);
    let rbps = rot_about_m(bps1, -2.0 * PI / 3.0).apply(bps2);
    let p72 = ev(7, 2);
    let p252 = ev(25, 2);
    let llc = intersect(bps1, rbps, ev(6, 2), p72);
    let mut w = sub(ev(6, 2), llc);

    let mut h_out = vec![llc, bps1];
    w = Affine::rot(-PI / 3.0).apply(w);
    h_out.push(add(h_out[1], w));
    h_out.push(ev(14, 2));
    w = Affine::rot(-PI / 3.0).apply(w);
    h_out.push(sub(h_out[3], w));
    h_out.push(ev(6, 2));

    let pick = |idx: &[usize]| -> Vec<(Affine, Geom)> {
        idx.iter().map(|&i| patch.children[i].clone()).collect()
    };
    let p_out = vec![p72, add(p72, sub(bps1, llc)), bps1, llc];
    let f_out = vec![bps2, ev(24, 2), ev(25, 0), p252, add(p252, sub(llc, bps1))];
    let aaa = h_out[2];
    let bbb = add(h_out[1], sub(h_out[4], h_out[5]));
    let ccc = rot_about_m(bbb, -PI / 3.0).apply(aaa);
    let t_out = vec![bbb, ccc, aaa];

    let mut h = Meta {
        shape: h_out,
        children: pick(&[0, 9, 16, 27, 26, 6, 1, 8, 10, 15]),
    };
    let mut t = Meta {
        shape: t_out,
        children: pick(&[11]),
    };
    let mut p = Meta {
        shape: p_out,
        children: pick(&[7, 2, 3, 4, 28]),
    };
    let mut f = Meta {
        shape: f_out,
        children: pick(&[21, 20, 22, 23, 24, 25]),
    };
    h.recentre();
    p.recentre();
    f.recentre();
    t.recentre();
    [Rc::new(h), Rc::new(t), Rc::new(p), Rc::new(f)]
}

fn collect(g: &Geom, m: Affine, out: &mut Vec<Affine>) {
    match g {
        Geom::Hat => out.push(m),
        Geom::Meta(meta) => {
            for (t, c) in &meta.children {
                collect(c, m.mul(t), out);
            }
        }
    }
}

/// Which H inside each new H holds the previous level's H (index into the new H's children; its
/// first three children are H metatiles). The middle one keeps the centre deep inside.
const CHAIN: usize = 2;

/// The hats of the level-`level` H metatile (level 1 holds 4 hats), each as a transform of
/// [`hat_outline`], and a centre: the reflected hat of the level-1 H. Each H contains the
/// previous level's H; coordinates are those of the level-1 H, so patches cut around the centre
/// nest from level to level.
pub fn generate(level: u32) -> (Vec<Affine>, P) {
    let mut tiles = initial();
    // Child 3 of the level-1 H is its reflected hat.
    let centre = tiles[0].children[3].0.apply(centroid(&hat_outline()));
    // Level-1 H coordinates to current H coordinates.
    let mut embed = Affine::ID;
    for _ in 1..level {
        let patch = construct_patch(&tiles);
        tiles = construct_metatiles(&patch);
        embed = tiles[0].children[CHAIN].0.mul(&embed);
    }
    // Metatile coordinates hold hats at half size; scale by 2 so hat edges are 1 and √3.
    let s = Affine([2.0, 0.0, 0.0, 0.0, 2.0, 0.0]);
    let mut out = Vec::new();
    collect(&Geom::Meta(tiles[0].clone()), s.mul(&embed.inv()), &mut out);
    (out, s.apply(centre))
}

/// Like [`generate`], but only the hats of metatiles that come within `radius` of the centre
/// (by their outline's bounding circle, widened by `slack` world units and 15%, since hats
/// stick out of the metatile outlines a little), so the large levels stay small in memory.
/// The kept hats are passed to `out` in the order of [`generate`] (a subsequence of it); returns
/// the centre.
pub fn generate_near(level: u32, radius: f64, slack: f64, out: &mut dyn FnMut(Affine)) -> P {
    let mut tiles = initial();
    let centre = tiles[0].children[3].0.apply(centroid(&hat_outline()));
    let mut embed = Affine::ID;
    for _ in 1..level {
        let patch = construct_patch(&tiles);
        tiles = construct_metatiles(&patch);
        embed = tiles[0].children[CHAIN].0.mul(&embed);
    }
    let s = Affine([2.0, 0.0, 0.0, 0.0, 2.0, 0.0]);
    let c = s.apply(centre);
    fn near(g: &Geom, m: Affine, c: P, r: f64, slack: f64, out: &mut dyn FnMut(Affine)) {
        match g {
            Geom::Hat => out(m),
            Geom::Meta(meta) => {
                let mid = centroid(&meta.shape);
                let rho = meta.shape.iter().map(|&v| dist(v, mid)).fold(0.0, f64::max);
                let scale = m.det().abs().sqrt();
                if dist(m.apply(mid), c) - rho * scale * 1.15 - slack > r {
                    return;
                }
                for (t, ch) in &meta.children {
                    near(ch, m.mul(t), c, r, slack, out);
                }
            }
        }
    }
    near(
        &Geom::Meta(tiles[0].clone()),
        s.mul(&embed.inv()),
        c,
        radius,
        slack,
        out,
    );
    c
}

/// Like [`generate_near`] with any test: a metatile is expanded only when `keep` accepts its
/// bounding circle (centre and radius, widened by 15% since hats stick out of the metatile
/// outlines a little). Hats are passed to `out` in the order of [`generate`]; returns the centre.
pub fn generate_window(
    level: u32,
    keep: &dyn Fn(P, f64) -> bool,
    out: &mut dyn FnMut(Affine),
) -> P {
    let mut tiles = initial();
    let centre = tiles[0].children[3].0.apply(centroid(&hat_outline()));
    let mut embed = Affine::ID;
    for _ in 1..level {
        let patch = construct_patch(&tiles);
        tiles = construct_metatiles(&patch);
        embed = tiles[0].children[CHAIN].0.mul(&embed);
    }
    let s = Affine([2.0, 0.0, 0.0, 0.0, 2.0, 0.0]);
    fn rec(g: &Geom, m: Affine, keep: &dyn Fn(P, f64) -> bool, out: &mut dyn FnMut(Affine)) {
        match g {
            Geom::Hat => out(m),
            Geom::Meta(meta) => {
                let mid = centroid(&meta.shape);
                let rho = meta.shape.iter().map(|&v| dist(v, mid)).fold(0.0, f64::max);
                let scale = m.det().abs().sqrt();
                if !keep(m.apply(mid), rho * scale * 1.15) {
                    return;
                }
                for (t, ch) in &meta.children {
                    rec(ch, m.mul(t), keep, out);
                }
            }
        }
    }
    rec(
        &Geom::Meta(tiles[0].clone()),
        s.mul(&embed.inv()),
        keep,
        out,
    );
    s.apply(centre)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn hat_counts_grow() {
        let n: Vec<usize> = (1..=4).map(|l| generate(l).0.len()).collect();
        assert_eq!(n[0], 4);
        for w in n.windows(2) {
            let r = w[1] as f64 / w[0] as f64;
            assert!(r > 5.0 && r < 9.0, "{n:?}");
        }
    }
}
