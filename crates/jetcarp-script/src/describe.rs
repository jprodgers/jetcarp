//! Cheap descriptors of a run's final state, for manifests and later pattern hunting.
//!
//! Row descriptors (lattice tilings) look at the field as a stack of rows from top to bottom:
//! unit-cell rows on most lattices, and half-row layers on the flat-top hexagon grid (the rows
//! the three-parent rules write). Positions along a row are physical: on the pointy-top hexagon
//! grid each row sits half a column to the side of the one above, and on the flat-top grid each
//! layer half a column (one hexagon) to the side, so a picture that stands still has shift 0.
//!
//! Classes (`row_class`), decided on the bottom half of the rows (the settled tail):
//! - `empty`: no nonzero cell anywhere;
//! - `uniform`: at most two distinct rows in the whole picture;
//! - `periodic`: the bottom rows repeat exactly with period `row_period` and no sideways shift;
//! - `shifting`: no such period, but the bottom rows repeat exactly after `row_period` rows moved
//!   sideways by `row_shift` columns (negative = left): diagonal stripes, gliders on a periodic
//!   background. With dead or live edges only cells at least `SHIFT_MARGIN` columns from either
//!   side are compared, so the edges may differ;
//! - `aperiodic`: neither. `row_entropy` estimates the entropy rate of the bottom rows: the growth
//!   of the number of distinct blocks of k consecutive cells from k = k2/2 to k2 (k2 as large as
//!   the number of blocks sampled allows: states^k2 at most a quarter of them), as a fraction of
//!   the maximum, log(states) per cell. Simple pictures repeat a few blocks and score low; chaotic
//!   ones contain nearly every block and score near 1. `row_hint` is `nested` when the bottom
//!   rows hold at least `NESTED_HOLES` holes (regions of equal cells at least 2 x 2 enclosed by
//!   the picture, so not the quiet space around a light cone or a domain reaching the top; a
//!   background that flips every row is first made steady) and the largest holds a square at
//!   least max(4, rows/16) on a side, and `row_entropy` < `NESTED_BELOW` or that square is at
//!   least max(8, rows/8): the holes of a self-similar picture grow with it, while noise from dead
//!   edges can raise the entropy of a nested light cone. Otherwise `chaotic` when `row_entropy` is
//!   at least `NESTED_BELOW`, else `regular` (several periodic domains with walls between them, too
//!   many for one period and shift). A cheap hint, not a proof.

use jetcarp_core::rules::FieldKind;
use jetcarp_core::{Status, World};
use serde_json::{Map, Value, json};

/// Cells ignored at each side of a row (dead or live edges) when matching shifted rows.
pub const SHIFT_MARGIN: usize = 2;
/// `row_entropy` below this is reported as `nested` or `regular`, at or above as `chaotic` unless
/// the picture has the large holes of a nested one.
pub const NESTED_BELOW: f64 = 0.55;
/// Smallest hole counted (side of the largest square of equal cells in it).
const HOLE_MIN: usize = 4;
/// A nested picture has at least this many separate holes in its bottom rows.
const NESTED_HOLES: usize = 4;

/// Descriptors of the field of CA `ca` (default: the first CA): population, density, per-state
/// fractions, exact period if one was seen, and on lattice tilings the row structure: number of
/// distinct rows, the smallest vertical period of the bottom rows (and sideways shift), the row it
/// starts from, the class, and for aperiodic pictures a nested/chaotic hint.
pub fn describe(w: &World, ca: Option<u32>) -> Map<String, Value> {
    let mut m = Map::new();
    let n = w.topo.len().max(1) as f64;
    let fi = ca
        .and_then(|id| w.ca(id))
        .or_else(|| w.cas.first())
        .map(|c| c.field)
        .unwrap_or(0);
    let Some(f) = w.fields.get(fi) else {
        return m;
    };
    let pop = f.population();
    m.insert("population".into(), json!(pop));
    if f.kind == FieldKind::Height {
        m.insert("mean_height".into(), json!(round6(pop as f64 / n)));
    } else if f.kind == FieldKind::Continuous {
        // Population counts nonzero cells; mass is the sum of the values (Lenia's mass).
        let mass = f.mass();
        m.insert("density".into(), json!(round6(pop as f64 / n)));
        m.insert("mass".into(), json!(round6(mass)));
        m.insert("mean_value".into(), json!(round6(mass / n)));
    } else {
        m.insert("density".into(), json!(round6(pop as f64 / n)));
        let k = f.max_state() as usize + 1;
        let mut counts = vec![0u64; k.max(2)];
        for &s in &f.state {
            if (s as usize) < counts.len() {
                counts[s as usize] += 1;
            }
        }
        for (s, c) in counts.iter().enumerate() {
            m.insert(format!("frac_{s}"), json!(round6(*c as f64 / n)));
        }
    }
    m.insert(
        "period".into(),
        match w.status() {
            Status::Repeats { period, .. } => json!(period),
            _ => Value::Null,
        },
    );
    if let Some(r) = row_structure(w, fi) {
        let opt = |x: Option<usize>| x.map_or(Value::Null, |p| json!(p));
        m.insert("distinct_rows".into(), json!(r.distinct));
        m.insert("row_period".into(), opt(r.period));
        m.insert(
            "row_shift".into(),
            r.shift.map_or(Value::Null, |s| json!(s)),
        );
        m.insert("periodic_from".into(), opt(r.from));
        m.insert("row_class".into(), json!(r.class));
        m.insert(
            "row_entropy".into(),
            r.entropy
                .map_or(Value::Null, |e| json!((e * 1000.0).round() / 1000.0)),
        );
        m.insert("row_hint".into(), r.hint.map_or(Value::Null, |h| json!(h)));
    }
    m
}

fn round6(x: f64) -> f64 {
    (x * 1e6).round() / 1e6
}

pub struct Rows {
    pub distinct: usize,
    /// Smallest p such that row y + p equals row y (shifted by `shift` columns for `shifting`)
    /// for every row in the bottom half (and the bottom half holds at least two periods).
    pub period: Option<usize>,
    /// Sideways shift per period in columns (0 for `periodic`); halves occur on hexagon grids.
    pub shift: Option<f64>,
    /// First row from which the picture repeats with that period and shift.
    pub from: Option<usize>,
    /// `empty`, `uniform`, `periodic`, `shifting`, or `aperiodic` (see the module docs).
    pub class: &'static str,
    /// Entropy-rate estimate of the bottom rows, 0..1 (aperiodic pictures only).
    pub entropy: Option<f64>,
    /// `nested` or `chaotic` (aperiodic pictures only).
    pub hint: Option<&'static str>,
}

/// A field as rows from top to bottom. Row `r` holds `n` positions of `stride` states each;
/// position `u` sits at `2u + off[r]` half-columns from the left.
struct Lines {
    rows: Vec<Vec<u8>>,
    stride: usize,
    off: Vec<i64>,
    wrap: bool,
    /// Columns per half-position (0.5 on lattices; 1 on the flat-top grid, where a layer's
    /// positions are every other hexagon column).
    unit: f64,
}

impl Lines {
    fn n(&self) -> usize {
        self.rows.first().map_or(0, |r| r.len() / self.stride)
    }

    /// Positions `lo..hi` of row `r` from the first to the last that differs from the background
    /// (the cell `SHIFT_MARGIN` in from that side), kept `SHIFT_MARGIN` from the edges.
    fn span(&self, r: usize) -> (i64, i64) {
        let (n, s, m) = (self.n(), self.stride, SHIFT_MARGIN);
        if n <= 2 * m + 1 {
            return (0, 0);
        }
        let row = &self.rows[r];
        let at = |u: usize| &row[u * s..(u + 1) * s];
        let (bl, br) = (at(m), at(n - 1 - m));
        let first = (m..n - m).find(|&u| at(u) != bl).unwrap_or(n - m);
        let last = (m..n - m).rev().find(|&u| at(u) != br).map_or(m, |u| u + 1);
        // A uniform row has no active part; compare it whole.
        if first >= last {
            return (m as i64, (n - m) as i64);
        }
        (first as i64, last as i64)
    }

    /// Does row `b` equal row `a` moved `d` half-columns to the right?
    fn matches(&self, a: usize, b: usize, d: i64) -> bool {
        let delta2 = self.off[b] - self.off[a] - d;
        if delta2 % 2 != 0 {
            return false;
        }
        let du = delta2 / 2;
        let n = self.n() as i64;
        let s = self.stride;
        let (ra, rb) = (&self.rows[a], &self.rows[b]);
        if self.wrap {
            if du == 0 {
                return ra == rb;
            }
            return (0..n).all(|u| {
                let v = (u + du).rem_euclid(n);
                rb[u as usize * s..(u as usize + 1) * s] == ra[v as usize * s..(v as usize + 1) * s]
            });
        }
        // Compare where both rows are active (between the first and last cells that differ from
        // the row's background at each side), so a light cone that is still growing can repeat
        // inside. When fewer than 16 positions (or half the row) overlap, whole rows (less
        // `SHIFT_MARGIN` at each side) are compared instead.
        let (sa, sb) = (self.span(a), self.span(b));
        let need = (n / 2).min(16);
        let m = SHIFT_MARGIN as i64;
        let (mut lo, mut hi) = (sb.0.max(sa.0 - du) + m, sb.1.min(sa.1 - du) - m);
        if hi - lo < need {
            (lo, hi) = (m.max(m - du), (n - m).min(n - m - du));
            if hi - lo < n / 2 {
                return false;
            }
        }
        rb[lo as usize * s..hi as usize * s] == ra[(lo + du) as usize * s..(hi + du) as usize * s]
    }
}

fn lines(w: &World, field: usize) -> Option<Lines> {
    let f = w.fields.get(field)?;
    if matches!(f.kind, FieldKind::Height | FieldKind::Continuous) {
        return None;
    }
    let st = &f.state;
    if let Some(g) = w.topo.flat_hex_rows() {
        // Half-row layers: layer l is type l % 2 of unit-cell row l / 2, one hexagon to the side.
        let rows = (0..2 * g.h)
            .map(|l| {
                (0..g.w as i32)
                    .map(|u| st[g.index(u, (l / 2) as i32, l % 2)])
                    .collect()
            })
            .collect();
        let off = (0..2 * g.h as i64).map(|l| l % 2).collect();
        return Some(Lines {
            rows,
            stride: 1,
            off,
            wrap: g.wrap,
            unit: 1.0,
        });
    }
    let g = w.topo.lattice()?;
    let nt = g.ntypes as usize;
    let rows = (0..g.h as usize)
        .map(|j| st[j * g.w as usize * nt..(j + 1) * g.w as usize * nt].to_vec())
        .collect();
    // Physical position of a row's first unit cell, in half lattice steps (a1). Sheared lattices
    // whose shear is not a multiple of half a step keep their lattice positions.
    let shear = 2.0 * g.a2[0] / g.a1[0];
    let exact = (shear - shear.round()).abs() < 1e-6;
    let off = (0..g.h as usize)
        .map(|j| {
            2 * g.row_start[j] as i64
                + if exact {
                    j as i64 * shear.round() as i64
                } else {
                    0
                }
        })
        .collect();
    Some(Lines {
        rows,
        stride: nt,
        off,
        wrap: g.wrap,
        unit: 0.5,
    })
}

/// Row structure of a field on a lattice tiling (see the module docs).
pub fn row_structure(w: &World, field: usize) -> Option<Rows> {
    let l = lines(w, field)?;
    let f = w.fields.get(field)?;
    let h = l.rows.len();
    let mut seen: std::collections::HashMap<&[u8], usize> = Default::default();
    for r in &l.rows {
        let next = seen.len();
        seen.entry(r.as_slice()).or_insert(next);
    }
    let distinct = seen.len();
    let empty = f.state.iter().all(|&s| s == 0);
    let half = h / 2;
    let lo = h - half;
    let fits = |p: usize, d: i64| (lo..h - p).all(|y| l.matches(y, y + p, d));
    let mut found: Option<(usize, i64)> = (1..=half / 2).find(|&p| fits(p, 0)).map(|p| (p, 0));
    if found.is_none() && !empty && distinct > 2 {
        'p: for p in 1..=half / 2 {
            // Up to two columns per row either way, nearest first, left before right.
            for a in 1..=4 * p as i64 {
                for d in [-a, a] {
                    if fits(p, d) {
                        found = Some((p, d));
                        break 'p;
                    }
                }
            }
        }
    }
    let from = found.map(|(p, d)| {
        let mut y0 = h - p;
        while y0 > 0 && l.matches(y0 - 1, y0 - 1 + p, d) {
            y0 -= 1;
        }
        y0
    });
    let class = if empty {
        "empty"
    } else if distinct <= 2 {
        "uniform"
    } else {
        match found {
            Some((_, 0)) => "periodic",
            Some(_) => "shifting",
            None => "aperiodic",
        }
    };
    let (entropy, hint) = if class == "aperiodic" {
        let k = (f.max_state() as usize + 1).max(2);
        let e = block_entropy(&l, lo, k);
        let (big, count) = holes(&l, lo);
        let nested = big >= (half / 8).max(4) && count >= NESTED_HOLES;
        let hint = e.map(|e| {
            if nested && (e < NESTED_BELOW || big >= (half / 4).max(8)) {
                "nested"
            } else if e >= NESTED_BELOW {
                "chaotic"
            } else {
                "regular"
            }
        });
        (e, hint)
    } else {
        (None, None)
    };
    Some(Rows {
        distinct,
        period: found.map(|x| x.0),
        shift: found.map(|x| x.1 as f64 * l.unit),
        from,
        class,
        entropy,
        hint,
    })
}

/// The picture with a background that alternates between two values every row (rules with
/// `000 -> 1` and `111 -> 0`) made steady: on every other row those two values are exchanged.
/// The background is read where the first rows are still quiet (a quarter of the way in from
/// the left, above the light cones of the edges and the centre). Other pictures are unchanged.
fn steady(l: &Lines) -> Vec<Vec<u8>> {
    let (n, s, h) = (l.n(), l.stride, l.rows.len());
    let p = n / 4;
    let quiet = (n / 4).min(h);
    let bg = |t: usize| &l.rows[t][p * s..(p + 1) * s];
    let flips = quiet >= 6 && bg(1) != bg(2) && (1..quiet - 2).all(|t| bg(t) == bg(t + 2));
    if !flips {
        return l.rows.clone();
    }
    let (a, b) = (bg(2).to_vec(), bg(1).to_vec());
    (0..h)
        .map(|t| {
            let mut row = l.rows[t].clone();
            if t % 2 == 1 {
                for chunk in row.chunks_mut(s) {
                    if chunk == a.as_slice() {
                        chunk.copy_from_slice(&b);
                    } else if chunk == b.as_slice() {
                        chunk.copy_from_slice(&a);
                    }
                }
            }
            row
        })
        .collect()
}

/// Holes of the bottom rows `lo..`: regions of equal cells (after `steady`, at least 2 x 2) enclosed by the
/// picture. Regions that reach the top three rows are the quiet space around light cones or a
/// background domain, not holes. Returns the side of the largest square of equal cells inside a
/// hole and the number of holes holding a square of at least `HOLE_MIN`. A nested picture has
/// holes at many places and sizes; chaos keeps them small, and a few domains with walls between
/// them leave none.
fn holes(l: &Lines, lo: usize) -> (usize, usize) {
    let (n, s) = (l.n(), l.stride);
    let rows = steady(l);
    let h = rows.len();
    let at = |t: usize, u: usize| &rows[t][u * s..(u + 1) * s];
    // Regions: 2 x 2 blocks of equal cells, joined when they overlap side by side with the same
    // value, so a hole does not leak through a gap one cell wide (as between the cells of a
    // Sierpinski line). A cell's region is that of a block holding it.
    const NONE: usize = usize::MAX;
    let (bh, bw) = (h.saturating_sub(1), n.saturating_sub(1));
    let solid = |t: usize, u: usize| {
        let v = at(t, u);
        at(t, u + 1) == v && at(t + 1, u) == v && at(t + 1, u + 1) == v
    };
    let mut block = vec![NONE; bh * bw];
    let mut ncomp = 0;
    let mut stack = Vec::new();
    for start in 0..bh * bw {
        if block[start] != NONE || !solid(start / bw, start % bw) {
            continue;
        }
        block[start] = ncomp;
        stack.push(start);
        while let Some(c) = stack.pop() {
            let (t, u) = (c / bw, c % bw);
            let v = at(t, u);
            let near = [
                (t > 0).then(|| (t - 1, u)),
                (t + 1 < bh).then(|| (t + 1, u)),
                (u > 0).then(|| (t, u - 1)),
                (u + 1 < bw).then(|| (t, u + 1)),
            ];
            for (tt, uu) in near.into_iter().flatten() {
                let d = tt * bw + uu;
                if block[d] == NONE && at(tt, uu) == v && solid(tt, uu) {
                    block[d] = ncomp;
                    stack.push(d);
                }
            }
        }
        ncomp += 1;
    }
    let mut comp = vec![NONE; h * n];
    for t in 0..bh {
        for u in 0..bw {
            let b = block[t * bw + u];
            if b != NONE {
                for c in [
                    t * n + u,
                    t * n + u + 1,
                    (t + 1) * n + u,
                    (t + 1) * n + u + 1,
                ] {
                    if comp[c] == NONE {
                        comp[c] = b;
                    }
                }
            }
        }
    }
    let mut outside = vec![false; ncomp];
    for &b in &block[..3.min(bh) * bw] {
        if b != NONE {
            outside[b] = true;
        }
    }
    // Largest square of equal cells ending at each cell, inside holes only.
    let mut best_in = vec![0usize; ncomp];
    let mut prev = vec![0usize; n];
    for t in lo..h {
        let mut cur = vec![0usize; n];
        for u in 0..n {
            let c = comp[t * n + u];
            if c == NONE || outside[c] {
                continue;
            }
            cur[u] = if t > lo
                && u > 0
                && comp[t * n + u - 1] == c
                && comp[(t - 1) * n + u] == c
                && comp[(t - 1) * n + u - 1] == c
            {
                1 + prev[u].min(prev[u - 1]).min(cur[u - 1])
            } else {
                1
            };
            best_in[c] = best_in[c].max(cur[u]);
        }
        prev = cur;
    }
    let largest = best_in.iter().copied().max().unwrap_or(0);
    let count = best_in.iter().filter(|&&b| b >= HOLE_MIN).count();
    (largest, count)
}

/// Entropy-rate estimate of rows `lo..` ([`jetcarp_core::looks::block_entropy`]).
fn block_entropy(l: &Lines, lo: usize, states: usize) -> Option<f64> {
    jetcarp_core::looks::block_entropy(&l.rows[lo..], l.stride, states)
}
