//! Framing a card on its live cells.
//!
//! A card shows its whole world, unless the live cells fill only a small part of it (a pack's
//! spaceship in a grid it can travel, a glider on the hat): then it is framed on them. The frame
//! is the live cells' bounding box grown by a margin ([`MARGIN_SHARE`] of the box, at least
//! [`MARGIN_CELLS`] cells), at least [`MIN_CELLS`] cells high, in the card's shape. A card is
//! framed when that frame is at most [`FRAME_BELOW`] of the width the whole world shows at (the
//! world cropped to the card, as the card crops its picture), or when that crop would cut the
//! live cells off (a ship travelling a tall grid) and the frame is no wider than it. It shows the
//! whole world again only once the frame would be wider than the whole world's crop.
//!
//! During a run the frame only zooms out (with [`HEADROOM`]), so an oscillator's phases don't
//! pump it, and its centre eases towards the live cells (a ship) without ever letting them leave
//! it. On a wrap-edge lattice the live cells are measured around the frame, so a ship crossing
//! the edge stays one compact box.
//!
//! Measuring counts the live cells in one pass over the states (a card shown whole colours every
//! cell each step anyway); more than [`DENSE`] of the cells live (a texture, a full grid) ends it
//! there, and otherwise a second pass places at most [`SAMPLE`] of them. Cards shown whole
//! measure only every [`LOOK_EVERY`] steps.

use jetcarp_core::World;
use jetcarp_core::geom::{P, centroid};
use jetcarp_core::render::torus_periods;
use jetcarp_core::rules::FieldKind;

/// The card picture's height over its width (`start.rs` lays cards out in this shape).
pub const PICTURE_HEIGHT: f32 = 0.62;
/// The card picture's width over its height.
pub const ASPECT: f64 = 1.0 / PICTURE_HEIGHT as f64;
/// Cells across the frame's short side, at least: one cell never fills a card.
pub const MIN_CELLS: f64 = 16.0;
/// The margin around the live cells: this share of the box's longer side...
pub const MARGIN_SHARE: f64 = 0.3;
/// ...and at least this many cells.
pub const MARGIN_CELLS: f64 = 3.0;
/// A card is framed when its frame is at most this share of the width the whole world shows.
pub const FRAME_BELOW: f64 = 0.55;
/// Live cells above this share of all cells: a texture or a full grid, never framed.
pub const DENSE: f64 = 1.0 / 3.0;
/// Live cells placed at most when measuring; more are sampled evenly.
pub const SAMPLE: usize = 4096;
/// Zooming out goes this much further than needed, so a growing pattern isn't refitted often.
pub const HEADROOM: f64 = 1.25;
/// Share of the way to the live cells' centre the frame moves each step...
pub const EASE: f64 = 0.3;
/// ...once they are more than this many cells from its centre (an oscillator's phases don't
/// move it).
pub const STILL_WITHIN: f64 = 0.75;
/// Steps between looks at a card shown whole (a soup that dies down to a ship gets framed).
pub const LOOK_EVERY: u32 = 10;

/// A world rectangle, `[x0, y0, x1, y1]`.
pub type Box4 = [f64; 4];

/// What measuring the live cells found.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Live {
    /// No live cell.
    Empty,
    /// Too many to frame (more than [`DENSE`] of the cells).
    Dense,
    /// The box their centres span.
    Box(Box4),
}

/// Whether cell `i` is live in any field (a height field: holds grains).
fn live_at(world: &World, i: usize) -> bool {
    world.fields.iter().any(|f| {
        if f.kind == FieldKind::Height {
            f.height.get(i).is_some_and(|&h| h != 0)
        } else {
            f.state.get(i).is_some_and(|&s| s != 0)
        }
    })
}

/// The states of a world's one field, when it has one and it isn't a height field: the usual
/// case, scanned as a plain slice.
fn only_states(world: &World) -> Option<&[u8]> {
    match world.fields.as_slice() {
        [f] if f.kind != FieldKind::Height && f.state.len() == world.topo.len() => Some(&f.state),
        _ => None,
    }
}

/// Measures the live cells (and agents) of a world. On a wrap-edge lattice each is taken at its
/// copy nearest `near` (the frame's centre; the first live cell's when there is no frame yet).
pub fn live_box(world: &World, near: Option<P>) -> Live {
    let topo = &world.topo;
    let n = topo.len();
    let states = only_states(world);
    let count = match states {
        Some(st) => st.iter().filter(|&&s| s != 0).count(),
        None => (0..n).filter(|&i| live_at(world, i)).count(),
    };
    let agents = world
        .cas
        .iter()
        .flat_map(|c| c.agents.iter().map(|a| a.cell as usize));
    if count == 0 && world.cas.iter().all(|c| c.agents.is_empty()) {
        return Live::Empty;
    }
    if count as f64 > DENSE * n as f64 {
        return Live::Dense;
    }
    let stride = count.div_ceil(SAMPLE).max(1);
    let mut centres: Vec<Option<P>> = vec![None; topo.shapes.len()];
    let mut at = |i: usize| -> P {
        let k = topo.shape_of(i);
        let c = *centres[k].get_or_insert_with(|| centroid(&topo.shapes[k]));
        topo.transform(i).apply(c)
    };
    let periods = torus_periods(topo);
    let mut reference = near;
    let mut b = [
        f64::INFINITY,
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::NEG_INFINITY,
    ];
    let mut add = |p: P, reference: &mut Option<P>| {
        let p = match (periods, *reference) {
            (Some(t), Some(r)) => nearest_copy(p, r, t),
            _ => p,
        };
        reference.get_or_insert(p);
        b = [
            b[0].min(p[0]),
            b[1].min(p[1]),
            b[2].max(p[0]),
            b[3].max(p[1]),
        ];
    };
    let mut seen = 0usize;
    let mut place = |i: usize| {
        if seen.is_multiple_of(stride) {
            add(at(i), &mut reference);
        }
        seen += 1;
    };
    match states {
        Some(st) => st
            .iter()
            .enumerate()
            .filter(|(_, s)| **s != 0)
            .for_each(|(i, _)| place(i)),
        None => (0..n).filter(|&i| live_at(world, i)).for_each(place),
    }
    for i in agents {
        if i < n {
            add(at(i), &mut reference);
        }
    }
    Live::Box(b)
}

/// The copy of `p` under the torus translations `t` nearest `r` (in the lattice's own sense).
pub fn nearest_copy(p: P, r: P, (t1, t2): (P, P)) -> P {
    let det = t1[0] * t2[1] - t1[1] * t2[0];
    if det.abs() < 1e-12 {
        return p;
    }
    let d = [p[0] - r[0], p[1] - r[1]];
    let u = ((d[0] * t2[1] - d[1] * t2[0]) / det).round();
    let v = ((t1[0] * d[1] - t1[1] * d[0]) / det).round();
    [p[0] - u * t1[0] - v * t2[0], p[1] - u * t1[1] - v * t2[1]]
}

/// The part of the world a card shows when it shows it whole: its bounds cropped to the card's
/// shape about their centre (as the card crops the picture).
pub fn whole_view(bounds: Box4, aspect: f64) -> Box4 {
    let (w, h) = (
        (bounds[2] - bounds[0]).max(1e-9),
        (bounds[3] - bounds[1]).max(1e-9),
    );
    let (vw, vh) = if w / h > aspect {
        (h * aspect, h)
    } else {
        (w, w / aspect)
    };
    let c = middle(bounds);
    [
        c[0] - vw / 2.0,
        c[1] - vh / 2.0,
        c[0] + vw / 2.0,
        c[1] + vh / 2.0,
    ]
}

/// The width (world units) of the frame a live box needs: the box grown by the margin (and by
/// a cell, since the box spans cell centres), at least [`MIN_CELLS`] cells high, in the card's
/// shape.
pub fn frame_width(b: Box4, cell: f64, aspect: f64) -> f64 {
    let (bw, bh) = ((b[2] - b[0]).max(0.0), (b[3] - b[1]).max(0.0));
    let m = (MARGIN_SHARE * bw.max(bh)).max(MARGIN_CELLS * cell);
    let (w, h) = (bw + 2.0 * m + cell, bh + 2.0 * m + cell);
    w.max(h * aspect).max(MIN_CELLS * cell * aspect)
}

/// The widest frame worth showing for the live box `b` rather than the whole world, which shows
/// `whole` (see [`whole_view`]); `framed`: the card is framed already.
pub fn widest_frame(b: Box4, whole: Box4, framed: bool) -> f64 {
    let whole_w = whole[2] - whole[0];
    let cut_off = b[0] < whole[0] || b[1] < whole[1] || b[2] > whole[2] || b[3] > whole[3];
    if framed || cut_off {
        whole_w
    } else {
        whole_w * FRAME_BELOW
    }
}

/// The frame's next centre: from `cam`, eased towards the live box's centre (or at it, with
/// `snap`), keeping the box inside the `w` x `h` frame (with a margin where there's room), and
/// the frame inside `bounds` where given (a world without wrap edges) and wider than it.
pub fn follow(
    cam: P,
    b: Box4,
    (w, h): (f64, f64),
    cell: f64,
    bounds: Option<Box4>,
    snap: bool,
) -> P {
    let axis = |k: usize, half: f64| {
        let (lo, hi) = (b[k], b[k + 2]);
        let target = (lo + hi) / 2.0;
        let mut c = cam[k];
        if snap {
            c = target;
        } else if (target - c).abs() > STILL_WITHIN * cell {
            c += (target - c) * EASE;
        }
        let room = (half - (hi - lo) / 2.0).max(0.0);
        let pad = (MARGIN_CELLS * cell).min(room);
        let (cmin, cmax) = (hi + pad - half, lo - pad + half);
        c = if cmin <= cmax {
            c.clamp(cmin, cmax)
        } else {
            target
        };
        if let Some(wb) = bounds {
            let (a, z) = (wb[k], wb[k + 2]);
            c = if z - a > 2.0 * half {
                c.clamp(a + half, z - half)
            } else {
                (a + z) / 2.0
            };
        }
        c
    };
    [axis(0, w / 2.0), axis(1, h / 2.0)]
}

/// The centre of a box.
pub fn middle(b: Box4) -> P {
    [(b[0] + b[2]) / 2.0, (b[1] + b[3]) / 2.0]
}

#[cfg(test)]
mod tests {
    use super::*;
    use jetcarp_core::topology::{Boundary, TilingSpec};
    use jetcarp_core::{CaSpec, Scene, SeedSpec};

    #[test]
    fn the_whole_world_is_cropped_to_the_card() {
        // Wider than a card: its height shows, and a card's width of it, in the middle.
        assert_eq!(
            whole_view([0.0, 0.0, 400.0, 100.0], 1.6),
            [120.0, 0.0, 280.0, 100.0]
        );
        // Taller: its width shows.
        assert_eq!(
            whole_view([0.0, 0.0, 100.0, 400.0], 1.6),
            [0.0, 168.75, 100.0, 231.25]
        );
    }

    #[test]
    fn a_frame_has_a_margin_a_least_height_and_the_cards_shape() {
        let cell = 1.0;
        // One cell: the least height, MIN_CELLS cells, in the card's shape.
        let w = frame_width([5.0, 5.0, 5.0, 5.0], cell, 1.6);
        assert!((w - MIN_CELLS * 1.6).abs() < 1e-9, "{w}");
        // A 40 x 10 box: 30% of 40 = 12 each side, plus a cell, wider than the card's shape.
        let w = frame_width([0.0, 0.0, 40.0, 10.0], cell, 1.6);
        assert!((w - (40.0 + 24.0 + 1.0)).abs() < 1e-9, "{w}");
        // A 10 x 40 box: the height sets the width.
        let w = frame_width([0.0, 0.0, 10.0, 40.0], cell, 1.6);
        assert!((w - (40.0 + 24.0 + 1.0) * 1.6).abs() < 1e-9, "{w}");
        // A small box: the margin is at least MARGIN_CELLS cells (3 x 3 box: 3 + 6 + 1 = 10 high,
        // under the least height).
        let w = frame_width([0.0, 0.0, 30.0, 3.0], cell, 1.6);
        assert!((w - (30.0 + 9.0 * 2.0 + 1.0)).abs() < 1e-9, "{w}");
    }

    #[test]
    fn framing_needs_a_small_part_of_the_world_or_a_cut_off_pattern_and_holds_on() {
        let whole = [0.0, 0.0, 100.0, 62.5];
        let middle = [45.0, 30.0, 55.0, 35.0];
        // A pattern the whole view shows: framed up to FRAME_BELOW of its width...
        assert_eq!(widest_frame(middle, whole, false), 100.0 * FRAME_BELOW);
        // ...and, once framed, kept framed up to the whole width.
        assert_eq!(widest_frame(middle, whole, true), 100.0);
        // A pattern the whole view cuts off (past its bottom): framed up to the whole width.
        assert_eq!(widest_frame([45.0, 60.0, 55.0, 70.0], whole, false), 100.0);
    }

    #[test]
    fn the_frame_follows_without_losing_or_jittering() {
        let cell = 1.0;
        let size = (32.0, 20.0);
        // At the start it is centred on the box.
        let b = [10.0, 10.0, 13.0, 12.0];
        let c = follow([0.0, 0.0], b, size, cell, None, true);
        assert_eq!(c, [11.5, 11.0]);
        // A box that moved under STILL_WITHIN cells doesn't move it (an oscillator's phases).
        let c2 = follow(c, [10.5, 10.0, 13.5, 12.0], size, cell, None, false);
        assert_eq!(c2, c);
        // A box that moved further: part of the way.
        let c3 = follow(c, [12.0, 10.0, 15.0, 12.0], size, cell, None, false);
        assert!(c3[0] > c[0] && c3[0] < 13.5, "{c3:?}");
        // A box that jumped far: it stays inside the frame, with the margin.
        let far = [100.0, 10.0, 103.0, 12.0];
        let c4 = follow(c, far, size, cell, None, false);
        assert!(c4[0] - 16.0 <= 100.0 - MARGIN_CELLS && c4[0] + 16.0 >= 103.0 + MARGIN_CELLS);
        // In a world without wrap edges, the frame stays inside the world...
        let world = [0.0, 0.0, 200.0, 100.0];
        let c5 = follow(c, [1.0, 1.0, 2.0, 2.0], size, cell, Some(world), true);
        assert_eq!(c5, [16.0, 10.0]);
        // ...and is centred on it along a side shorter than the frame.
        let c6 = follow(
            c,
            [1.0, 1.0, 2.0, 2.0],
            size,
            cell,
            Some([0.0, 0.0, 200.0, 12.0]),
            true,
        );
        assert_eq!(c6, [16.0, 6.0]);
    }

    #[test]
    fn copies_on_a_torus_are_taken_near_the_frame() {
        let t = ([100.0, 0.0], [10.0, 50.0]);
        // Just across the right edge from a point near the left: one width back.
        assert_eq!(nearest_copy([98.0, 5.0], [2.0, 5.0], t), [-2.0, 5.0]);
        // Across the bottom: the shifted row comes back.
        assert_eq!(nearest_copy([12.0, 48.0], [2.0, 2.0], t), [2.0, -2.0]);
        assert_eq!(nearest_copy([3.0, 4.0], [2.0, 2.0], t), [3.0, 4.0]);
    }

    fn world(w: u32, h: u32, boundary: Boundary, seed: SeedSpec) -> World {
        let mut s = Scene::new(TilingSpec::rect("square", w, h, boundary));
        s.cas = vec![CaSpec::new("B3/S23").seed(seed)];
        World::from_scene(&s).unwrap()
    }

    #[test]
    fn live_cells_are_measured_compactly_even_across_a_wrap_edge() {
        // Nothing live.
        let w = world(64, 48, Boundary::Wrap, SeedSpec::Cells { cells: vec![] });
        assert_eq!(live_box(&w, None), Live::Empty);
        // A random soup is a texture.
        let w = world(64, 48, Boundary::Wrap, SeedSpec::Random { density: 0.5 });
        assert_eq!(live_box(&w, None), Live::Dense);
        // Three cells in a row near the middle: a box two cells wide.
        let row = |y: u32, xs: &[u32]| xs.iter().map(|&x| [y * 64 + x, 1]).collect::<Vec<_>>();
        let w = world(
            64,
            48,
            Boundary::Wrap,
            SeedSpec::Cells {
                cells: row(20, &[30, 31, 32]),
            },
        );
        let Live::Box(b) = live_box(&w, None) else {
            panic!("a box")
        };
        assert!(
            (b[2] - b[0] - 2.0).abs() < 1e-9 && (b[3] - b[1]).abs() < 1e-9,
            "{b:?}"
        );
        // The same row across the wrap edge (columns 63, 0, 1): still two cells wide.
        let w = world(
            64,
            48,
            Boundary::Wrap,
            SeedSpec::Cells {
                cells: row(20, &[63, 0, 1]),
            },
        );
        let Live::Box(b) = live_box(&w, None) else {
            panic!("a box")
        };
        assert!((b[2] - b[0] - 2.0).abs() < 1e-9, "{b:?}");
        // Without wrap edges, the far columns are far apart.
        let w = world(
            64,
            48,
            Boundary::Dead,
            SeedSpec::Cells {
                cells: row(20, &[63, 0, 1]),
            },
        );
        let Live::Box(b) = live_box(&w, None) else {
            panic!("a box")
        };
        assert!(b[2] - b[0] > 60.0, "{b:?}");
    }
}
