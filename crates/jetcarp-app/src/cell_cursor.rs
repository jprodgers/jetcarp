//! The grid's cell cursor (#219): drawing, erasing, selecting and moving patterns from the
//! keyboard. This is the geometry, over any tiling: the next cell in a direction, the cells a
//! selection covers, and where a selected pattern goes when it moves. It knows nothing of egui,
//! so it is tested on its own; `app/keyboard.rs` drives it from the arrow keys.
//!
//! **The next cell** in a direction is found from cell centres: among the cells touching the
//! cursor's cell, those whose centres lie less than 57° from the direction (so a triangle's
//! neighbour across its side, at 60°, is not taken for the one above it; where none does, less
//! than 81°), the nearest one in that direction that keeps closest to the line the cursor is
//! moving along. Keeping to a line (the
//! row's height for moves left and right, the column's for moves up and down) is what makes a run
//! of moves follow a row of triangles or a column of hexagons, zigzagging about it, rather than
//! drifting off it. On a grid with wrapped edges the cells across an edge count as next to each
//! other, as they do for the rules.
//!
//! **A selection** is every cell whose centre lies in the rectangle spanned by the centres of its
//! two corner cells (where the selection began, and the cursor).
//!
//! **A pattern moves** by a translation of the tiling where it has one: on a periodic tiling, the
//! shortest lattice vector within 45° of the direction, which maps every tile onto a tile of the
//! same shape, so the pattern keeps its shape. An aperiodic patch has no translations: there the
//! pattern moves by the step from the cursor's cell to the next one, each cell landing in the
//! cell under its moved centre, which can change the pattern's shape.

use jetcarp_core::geom::P;
use jetcarp_core::{Nb, Topology};

/// A direction on screen. The view draws world y downwards, so Up is −y.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Dir {
    Left,
    Right,
    Up,
    Down,
}

impl Dir {
    #[cfg(test)]
    pub const ALL: [Dir; 4] = [Dir::Left, Dir::Right, Dir::Up, Dir::Down];

    /// The unit vector, in world units.
    pub fn vec(self) -> P {
        match self {
            Dir::Left => [-1.0, 0.0],
            Dir::Right => [1.0, 0.0],
            Dir::Up => [0.0, -1.0],
            Dir::Down => [0.0, 1.0],
        }
    }

    /// Left or right.
    pub fn horizontal(self) -> bool {
        matches!(self, Dir::Left | Dir::Right)
    }

    #[cfg(test)]
    pub fn opposite(self) -> Dir {
        match self {
            Dir::Left => Dir::Right,
            Dir::Right => Dir::Left,
            Dir::Up => Dir::Down,
            Dir::Down => Dir::Up,
        }
    }

    pub fn word(self) -> &'static str {
        match self {
            Dir::Left => "left",
            Dir::Right => "right",
            Dir::Up => "up",
            Dir::Down => "down",
        }
    }
}

fn sub(a: P, b: P) -> P {
    [a[0] - b[0], a[1] - b[1]]
}

fn add(a: P, b: P) -> P {
    [a[0] + b[0], a[1] + b[1]]
}

fn dot(a: P, b: P) -> f64 {
    a[0] * b[0] + a[1] * b[1]
}

fn len(a: P) -> f64 {
    dot(a, a).sqrt()
}

/// The line moves keep to: for moves left and right a height (y), for moves up and down an x.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Line {
    pub horizontal: bool,
    pub at: f64,
}

/// `v` (a difference of two positions) as its shortest image on a wrapped grid: the cells across a
/// wrapped edge are next to each other, though their centres are far apart.
pub fn wrapped(topo: &Topology, v: P) -> P {
    let Some(g) = topo.lattice().filter(|g| g.wrap) else {
        return v;
    };
    // The torus maps (i + w, j) and (i − wrap_shift, j + h) onto the cell at (i, j).
    let p1 = [g.w as f64 * g.a1[0], g.w as f64 * g.a1[1]];
    let s = g.wrap_shift as f64;
    let p2 = [
        g.h as f64 * g.a2[0] + s * g.a1[0],
        g.h as f64 * g.a2[1] + s * g.a1[1],
    ];
    let mut best = v;
    for m in -1..=1 {
        for n in -1..=1 {
            let c = [
                v[0] + m as f64 * p1[0] + n as f64 * p2[0],
                v[1] + m as f64 * p1[1] + n as f64 * p2[1],
            ];
            if len(c) < len(best) - 1e-9 {
                best = c;
            }
        }
    }
    best
}

/// The cell next to `from` in direction `d`, or `None` at an edge of the tiling. `line` is the
/// line the cursor keeps to; a move across it (up after left, say) starts a new one.
pub fn step(topo: &Topology, from: usize, d: Dir, line: &mut Option<Line>) -> Option<usize> {
    let c0 = topo.centroid(from);
    let dv = d.vec();
    let perp = [-dv[1], dv[0]];
    let keep = match *line {
        Some(l) if l.horizontal == d.horizontal() => l,
        _ => Line {
            horizontal: d.horizontal(),
            at: if d.horizontal() { c0[1] } else { c0[0] },
        },
    };
    // Where the line runs, as a point on it.
    let on_line = if keep.horizontal {
        [c0[0], keep.at]
    } else {
        [keep.at, c0[1]]
    };
    let mut near = vec![];
    topo.for_each_neighbor(from, Nb::Touch, |n| {
        if n != from {
            near.push(n)
        }
    });
    near.sort_unstable();
    near.dedup();
    let pick = |cone: f64| {
        let mut best: Option<(f64, f64, usize)> = None;
        for &n in &near {
            let v = wrapped(topo, sub(topo.centroid(n), c0));
            let l = len(v);
            if l < 1e-12 {
                continue;
            }
            let along = dot(v, dv);
            if along / l < cone {
                continue;
            }
            // How far its centre is from the line, and which side (for a tie: the right or lower
            // side, so a tie is broken the same way every time).
            let across = dot(add(c0, v), perp) - dot(on_line, perp);
            let score = along + across.abs();
            let better = match best {
                None => true,
                Some((s, a, _)) => {
                    score < s - 1e-9 || ((score - s).abs() <= 1e-9 && across > a + 1e-9)
                }
            };
            if better {
                best = Some((score, across, n));
            }
        }
        best.map(|b| b.2)
    };
    let next = pick(0.55).or_else(|| pick(0.15))?;
    *line = Some(keep);
    Some(next)
}

/// The cells of a selection from `a` to `b`: those whose centres lie in the rectangle their
/// centres span, in order.
pub fn selection(topo: &Topology, a: usize, b: usize) -> Vec<usize> {
    let (ca, cb) = (topo.centroid(a), topo.centroid(b));
    let e = 1e-6;
    let bx = [
        ca[0].min(cb[0]) - e,
        ca[1].min(cb[1]) - e,
        ca[0].max(cb[0]) + e,
        ca[1].max(cb[1]) + e,
    ];
    let mut v = vec![];
    topo.for_cells_in_box(bx, |c, _| v.push(c));
    v.sort_unstable();
    v.dedup();
    if v.is_empty() {
        v.push(a);
    }
    v
}

/// How a selected pattern moves one step.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Shift {
    /// By this many lattice vectors: every tile onto one of the same shape.
    Lattice { di: i32, dj: i32 },
    /// By this vector, each cell into the cell under its moved centre (aperiodic patches).
    Vector(P),
}

/// The step a pattern moves by in direction `d`, from the cursor's cell `from`. `drift` is how
/// far earlier moves along this axis strayed across it, so that where two lattice vectors are
/// equally good (up on a grid of hexagons), moves take them in turn and keep to a column.
pub fn shift(topo: &Topology, from: usize, d: Dir, drift: &mut f64) -> Option<Shift> {
    let dv = d.vec();
    let perp = [-dv[1], dv[0]];
    if let Some(g) = topo.lattice() {
        let mut best: Option<(f64, f64, i32, i32)> = None;
        for di in -3..=3i32 {
            for dj in -3..=3i32 {
                if di == 0 && dj == 0 {
                    continue;
                }
                let v = [
                    di as f64 * g.a1[0] + dj as f64 * g.a2[0],
                    di as f64 * g.a1[1] + dj as f64 * g.a2[1],
                ];
                let l = len(v);
                let cos = dot(v, dv) / l;
                if cos < 0.7 {
                    continue;
                }
                let score = l / cos;
                let stray = (*drift + dot(v, perp)).abs();
                let better = match best {
                    None => true,
                    Some((s, st, _, _)) => {
                        score < s - 1e-9 || ((score - s).abs() <= 1e-9 && stray < st - 1e-9)
                    }
                };
                if better {
                    best = Some((score, stray, di, dj));
                }
            }
        }
        let (_, _, di, dj) = best?;
        *drift += dot(
            [
                di as f64 * g.a1[0] + dj as f64 * g.a2[0],
                di as f64 * g.a1[1] + dj as f64 * g.a2[1],
            ],
            perp,
        );
        return Some(Shift::Lattice { di, dj });
    }
    let mut line = None;
    let to = step(topo, from, d, &mut line)?;
    Some(Shift::Vector(wrapped(
        topo,
        sub(topo.centroid(to), topo.centroid(from)),
    )))
}

/// Where cell `c` goes when a pattern moves by `s`, or `None` off the tiling's edge.
pub fn moved(topo: &Topology, c: usize, s: Shift) -> Option<usize> {
    match s {
        Shift::Lattice { di, dj } => {
            let g = topo.lattice()?;
            let (u, j, k) = g.decompose(c);
            g.resolve(u + g.row_start[j as usize] + di, j + dj, k)
        }
        Shift::Vector(v) => topo.locate(add(topo.centroid(c), v)),
    }
}

/// Where the cursor is, and the selection's other corner.
#[derive(Clone, Debug, Default)]
pub struct Cursor {
    pub cell: Option<usize>,
    /// Where the selection began (Shift+arrows), while there is one.
    pub anchor: Option<usize>,
    /// The line moves keep to.
    pub line: Option<Line>,
    /// Moves of a pattern along one axis: how far they strayed across it ([`shift`]).
    pub drift: (Option<bool>, f64),
    /// The cells of the selection last asked for, by its corners and tiling ([`Cursor::selected`]):
    /// a selection over millions of cells is found once, not each frame.
    cache: std::cell::RefCell<Option<((usize, usize, usize, usize), std::rc::Rc<Vec<usize>>)>>,
}

impl Cursor {
    /// Forget cells that no longer exist (a new tiling).
    pub fn fit(&mut self, topo: &Topology) {
        let n = topo.len();
        if self.cell.is_some_and(|c| c >= n) {
            self.cell = None;
        }
        if self.anchor.is_some_and(|c| c >= n) || self.cell.is_none() {
            self.anchor = None;
        }
    }

    /// Move one cell in `d`; with `extend`, the selection grows from where it began (or from
    /// here). Returns whether the cursor moved (not at an edge).
    pub fn go(&mut self, topo: &Topology, d: Dir, extend: bool) -> bool {
        let Some(c) = self.cell else {
            return false;
        };
        if extend {
            self.anchor.get_or_insert(c);
        } else {
            self.anchor = None;
        }
        match step(topo, c, d, &mut self.line) {
            Some(n) => {
                self.cell = Some(n);
                true
            }
            None => false,
        }
    }

    /// The cells selected, or just the cursor's cell (tests; the app keeps [`Cursor::selected`]).
    #[cfg(test)]
    pub fn cells(&self, topo: &Topology) -> Vec<usize> {
        self.selected(topo).to_vec()
    }

    /// [`Cursor::cells`], found again only when the selection or the tiling changed.
    pub fn selected(&self, topo: &Topology) -> std::rc::Rc<Vec<usize>> {
        let (Some(c), Some(a)) = (self.cell, self.anchor) else {
            return std::rc::Rc::new(self.cell.into_iter().collect());
        };
        let key = (a, c, topo as *const Topology as usize, topo.len());
        if let Some((k, v)) = &*self.cache.borrow()
            && *k == key
        {
            return v.clone();
        }
        let v = std::rc::Rc::new(selection(topo, a, c));
        *self.cache.borrow_mut() = Some((key, v.clone()));
        v
    }

    /// The step a pattern moves by in `d`.
    pub fn shift(&mut self, topo: &Topology, d: Dir) -> Option<Shift> {
        let c = self.cell?;
        if self.drift.0 != Some(d.horizontal()) {
            self.drift = (Some(d.horizontal()), 0.0);
        }
        shift(topo, c, d, &mut self.drift.1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use jetcarp_core::{Boundary, TilingSpec};

    fn topo(family: &str, w: u32, h: u32, b: Boundary) -> Topology {
        Topology::build(&TilingSpec::rect(family, w, h, b)).unwrap()
    }

    /// The cell nearest the middle of the tiling.
    fn middle(t: &Topology) -> usize {
        let b = t.bounds;
        t.locate([(b[0] + b[2]) / 2.0, (b[1] + b[3]) / 2.0])
            .unwrap_or(t.center_cell())
    }

    #[test]
    fn on_squares_the_arrows_go_to_the_cell_beside() {
        let t = topo("square", 12, 9, Boundary::Dead);
        let g = t.lattice().unwrap();
        let c = g.index(5, 4, 0);
        let mut line = None;
        assert_eq!(step(&t, c, Dir::Right, &mut line), Some(g.index(6, 4, 0)));
        assert_eq!(step(&t, c, Dir::Left, &mut line), Some(g.index(4, 4, 0)));
        assert_eq!(step(&t, c, Dir::Up, &mut line), Some(g.index(5, 3, 0)));
        assert_eq!(step(&t, c, Dir::Down, &mut line), Some(g.index(5, 5, 0)));
        // A dead edge stops it; a wrapped one goes round.
        let edge = g.index(11, 4, 0);
        assert_eq!(step(&t, edge, Dir::Right, &mut None), None);
        let w = topo("square", 12, 9, Boundary::Wrap);
        let g = w.lattice().unwrap();
        assert_eq!(
            step(&w, g.index(11, 4, 0), Dir::Right, &mut None),
            Some(g.index(0, 4, 0))
        );
        assert_eq!(
            step(&w, g.index(3, 0, 0), Dir::Up, &mut None),
            Some(g.index(3, 8, 0))
        );
    }

    /// On every kind of tiling, each arrow moves the cursor that way, and a run of moves keeps to
    /// its row or column; the opposite arrow comes back.
    #[test]
    fn every_tiling_moves_that_way_and_keeps_to_its_line() {
        for family in [
            "square",
            "hex",
            "triangle",
            "trihex",
            "cairo",
            "snubsq",
            "oct",
            "rhombitrihex",
            "trunctrihex",
            "penrose-rhomb",
            "penrose-kd",
            "hat",
            "spectre",
        ] {
            let spec = if jetcarp_core::families::family(family)
                .is_some_and(|f| f.kind == jetcarp_core::families::FamilyKind::Aperiodic)
            {
                TilingSpec::new(family)
            } else {
                TilingSpec::rect(family, 24, 24, Boundary::Dead)
            };
            let Ok(t) = Topology::build(&spec) else {
                panic!("{family} did not build");
            };
            let start = middle(&t);
            let unit = jetcarp_core::geom::area(&t.polygon(start)).sqrt();
            for d in Dir::ALL {
                let mut line = None;
                let mut c = start;
                let c0 = t.centroid(start);
                let dv = d.vec();
                let mut seen = vec![c];
                for k in 1..=5 {
                    let n = step(&t, c, d, &mut line)
                        .unwrap_or_else(|| panic!("{family}: no cell {d:?} of {c} (move {k})"));
                    let (a, b) = (t.centroid(c), t.centroid(n));
                    assert!(
                        dot(sub(b, a), dv) > 0.0,
                        "{family}: {d:?} went the wrong way ({a:?} to {b:?})"
                    );
                    assert!(!seen.contains(&n), "{family}: {d:?} came back to {n}");
                    seen.push(n);
                    c = n;
                }
                // Five moves stay near the line they started on.
                let p = [-dv[1], dv[0]];
                let off = dot(sub(t.centroid(c), c0), p).abs();
                assert!(
                    off <= 1.6 * unit,
                    "{family}: {d:?} strayed {off:.2} from its line (cell size {unit:.2})"
                );
                // And on the periodic tilings, as many moves back come home.
                if t.lattice().is_some() {
                    let mut back = c;
                    let mut line = line;
                    for _ in 0..5 {
                        back = step(&t, back, d.opposite(), &mut line).unwrap();
                    }
                    let home = dot(sub(t.centroid(back), c0), dv).abs();
                    assert!(
                        home <= 0.6 * unit,
                        "{family}: back {d:?} ended {home:.2} from the start"
                    );
                }
            }
        }
    }

    /// Up and down a column of hexagons zigzag about it; left and right along a row of triangles
    /// visit every triangle, up and down alike.
    #[test]
    fn hexagons_and_triangles_are_followed_cell_by_cell() {
        let t = topo("hex", 20, 20, Boundary::Dead);
        let mut c = middle(&t);
        let x0 = t.centroid(c)[0];
        // Half the distance between neighbouring columns of hexagons, and a little.
        let half = 0.55 * jetcarp_core::geom::area(&t.polygon(c)).sqrt();
        let mut line = None;
        for _ in 0..8 {
            c = step(&t, c, Dir::Up, &mut line).unwrap();
            let dx = (t.centroid(c)[0] - x0).abs();
            assert!(dx < half, "the column drifted by {dx}");
        }
        let t = topo("triangle", 20, 10, Boundary::Dead);
        let mut c = middle(&t);
        let y0 = t.centroid(c)[1];
        let mut line = None;
        let mut xs = vec![t.centroid(c)[0]];
        for _ in 0..8 {
            c = step(&t, c, Dir::Right, &mut line).unwrap();
            xs.push(t.centroid(c)[0]);
            assert!((t.centroid(c)[1] - y0).abs() < 0.5, "left the row");
        }
        // Neighbouring triangles' centres are half an edge apart: none was skipped.
        for w in xs.windows(2) {
            assert!(w[1] - w[0] < 0.6, "skipped a triangle: {xs:?}");
        }
    }

    #[test]
    fn a_selection_is_the_cells_between_its_corners() {
        let t = topo("square", 10, 10, Boundary::Dead);
        let g = t.lattice().unwrap();
        let s = selection(&t, g.index(2, 3, 0), g.index(4, 5, 0));
        assert_eq!(s.len(), 9);
        for u in 2..=4 {
            for j in 3..=5 {
                assert!(s.contains(&g.index(u, j, 0)));
            }
        }
        // Either way round, and a single cell.
        assert_eq!(selection(&t, g.index(4, 5, 0), g.index(2, 3, 0)), s);
        assert_eq!(
            selection(&t, g.index(1, 1, 0), g.index(1, 1, 0)),
            vec![g.index(1, 1, 0)]
        );
        // Shift+arrows grow it from where it began.
        let mut k = Cursor {
            cell: Some(g.index(5, 5, 0)),
            ..Default::default()
        };
        assert!(k.go(&t, Dir::Right, true));
        assert!(k.go(&t, Dir::Down, true));
        assert_eq!(k.anchor, Some(g.index(5, 5, 0)));
        assert_eq!(k.cells(&t).len(), 4);
        // A plain arrow ends it.
        assert!(k.go(&t, Dir::Left, false));
        assert_eq!(k.anchor, None);
        assert_eq!(k.cells(&t), vec![g.index(5, 6, 0)]);
    }

    /// A pattern moves by a translation of the tiling, so it keeps its shape: on squares one cell,
    /// on triangles one pair of triangles, on hexagons up a column in turn left and right.
    #[test]
    fn patterns_move_by_the_tiling_s_own_translations() {
        let t = topo("square", 10, 10, Boundary::Wrap);
        let g = t.lattice().unwrap();
        let mut drift = 0.0;
        let s = shift(&t, g.index(3, 3, 0), Dir::Right, &mut drift).unwrap();
        assert_eq!(s, Shift::Lattice { di: 1, dj: 0 });
        assert_eq!(moved(&t, g.index(3, 3, 0), s), Some(g.index(4, 3, 0)));
        // Off a wrapped edge comes back on the other side.
        assert_eq!(moved(&t, g.index(9, 3, 0), s), Some(g.index(0, 3, 0)));
        let up = shift(&t, g.index(3, 3, 0), Dir::Up, &mut 0.0).unwrap();
        assert_eq!(moved(&t, g.index(3, 3, 0), up), Some(g.index(3, 2, 0)));
        // A dead edge drops what goes off it.
        let d = topo("square", 10, 10, Boundary::Dead);
        let g = d.lattice().unwrap();
        let s = shift(&d, g.index(3, 3, 0), Dir::Right, &mut 0.0).unwrap();
        assert_eq!(moved(&d, g.index(9, 3, 0), s), None);

        // Triangles: the same type of triangle, a whole cell of the lattice along.
        let t = topo("triangle", 12, 12, Boundary::Dead);
        let c = middle(&t);
        let s = shift(&t, c, Dir::Right, &mut 0.0).unwrap();
        let to = moved(&t, c, s).unwrap();
        assert_eq!(t.shape_of(to), t.shape_of(c));
        let v = sub(t.centroid(to), t.centroid(c));
        assert!(v[0] > 0.5 && v[1].abs() < 1e-9, "{v:?}");

        // Hexagons: up twice comes back above the start.
        let t = topo("hex", 12, 12, Boundary::Dead);
        let c = middle(&t);
        let mut k = Cursor {
            cell: Some(c),
            ..Default::default()
        };
        let a = k.shift(&t, Dir::Up).unwrap();
        let c1 = moved(&t, c, a).unwrap();
        k.cell = Some(c1);
        let b = k.shift(&t, Dir::Up).unwrap();
        let c2 = moved(&t, c1, b).unwrap();
        let dx = t.centroid(c2)[0] - t.centroid(c)[0];
        assert!(dx.abs() < 1e-9, "two moves up drifted {dx}");
        assert!(t.centroid(c2)[1] < t.centroid(c)[1]);
    }

    /// On an aperiodic patch a pattern moves by the step to the next cell.
    #[test]
    fn on_an_aperiodic_patch_a_pattern_moves_to_the_cells_under_it() {
        let t = Topology::build(&TilingSpec::new("penrose-rhomb")).unwrap();
        let c = middle(&t);
        let s = shift(&t, c, Dir::Right, &mut 0.0).unwrap();
        let Shift::Vector(v) = s else { panic!("{s:?}") };
        assert!(v[0] > 0.0);
        let to = moved(&t, c, s).unwrap();
        assert_eq!(Some(to), step(&t, c, Dir::Right, &mut None));
    }

    #[test]
    fn a_new_tiling_forgets_cells_it_does_not_have() {
        let t = topo("square", 8, 8, Boundary::Dead);
        let mut k = Cursor {
            cell: Some(100),
            anchor: Some(3),
            ..Default::default()
        };
        k.fit(&t);
        assert_eq!((k.cell, k.anchor), (None, None));
        let mut k = Cursor {
            cell: Some(5),
            anchor: Some(99),
            ..Default::default()
        };
        k.fit(&t);
        assert_eq!((k.cell, k.anchor), (Some(5), None));
    }
}
