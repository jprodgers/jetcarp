//! Lenia: a continuous cellular automaton (Bert Chan, "Lenia — Biology of Artificial Life",
//! Complex Systems 28(3), 2019; reference code github.com/Chakazul/Lenia, MIT licence).
//!
//! Each cell holds a value in [0, 1]. Every step, the potential U of a cell is the weighted sum of
//! the values around it with a ring-shaped kernel K of radius R (normalised to sum to 1), and the
//! value moves by dt = 1/T times the growth G(U), clipped to [0, 1]:
//! `A <- clip(A + G(K * A) / T, 0, 1)`.
//!
//! Parameters follow Chan's reference code (`LeniaND.py`): `R` kernel radius in cells, `T` time
//! resolution, `b` kernel peaks (one ring per peak, as fractions), `m` growth centre (mu), `s`
//! growth width (sigma), `kn` kernel core (1 polynomial `(4r(1-r))^4`, 2 exponential
//! `exp(4 - 1/(r(1-r)))`, 3 step, 4 staircase) and `gn` growth function (1 polynomial
//! `max(0, 1 - (U-m)^2/(9s^2))^4 * 2 - 1`, 2 Gaussian `2 exp(-(U-m)^2/(2s^2)) - 1`, 3 step).
//! Notation: `Lenia R=13,T=10,b=1,m=0.15,s=0.015` (`kn`/`gn` given when not 1;
//! `b=1/2,1` lists peaks).
//!
//! Distances are between cell centroids in units of the mean cell size (the square root of the
//! mean tile area), so the same rule runs on any lattice tiling (squares as in Chan's work,
//! hexagons, triangles, Archimedean and isohedral tilings) and on aperiodic patches. Cells past a
//! dead edge count as 0. Sums run over the kernel taps in a fixed order for every cell, so the
//! result does not depend on the thread count.

use crate::detmath::{dist, exp, pow4};
use crate::rules::FieldKind;
use crate::topology::{Storage, Topology};
use crate::world::Field;
use rayon::prelude::*;

#[derive(Clone, Debug, PartialEq)]
pub struct LeniaRule {
    /// Kernel radius in cells.
    pub r: f64,
    /// Time resolution: dt = 1 / t.
    pub t: f64,
    /// Kernel peaks, one per ring.
    pub b: Vec<f64>,
    /// Growth centre.
    pub m: f64,
    /// Growth width.
    pub s: f64,
    /// Kernel core, 1-4 (see the module docs).
    pub kn: u8,
    /// Growth function, 1-3.
    pub gn: u8,
}

impl Default for LeniaRule {
    /// Orbium's parameters (Chan 2019).
    fn default() -> Self {
        LeniaRule {
            r: 13.0,
            t: 10.0,
            b: vec![1.0],
            m: 0.15,
            s: 0.015,
            kn: 1,
            gn: 1,
        }
    }
}

fn frac(x: f64) -> String {
    for d in 1..=12u32 {
        let n = (x * d as f64).round();
        if (n / d as f64 - x).abs() < 1e-9 {
            return if d == 1 {
                format!("{n}")
            } else {
                format!("{n}/{d}")
            };
        }
    }
    format!("{x}")
}

impl LeniaRule {
    /// `Lenia R=13,T=10,b=1,m=0.15,s=0.015[,kn=1][,gn=1]`; also Chan's `b=[1/2,1]` and spaces.
    /// `None` when the text is not a Lenia rule.
    pub fn parse(text: &str) -> Option<Result<LeniaRule, String>> {
        let low = text.trim().to_ascii_lowercase();
        let rest = low.strip_prefix("lenia")?;
        Some(Self::parse_params(rest))
    }

    fn parse_params(rest: &str) -> Result<LeniaRule, String> {
        let mut r = LeniaRule::default();
        let s: String = rest
            .chars()
            .filter(|c| !c.is_whitespace() && *c != '[' && *c != ']')
            .collect();
        if s.is_empty() {
            return Ok(r);
        }
        // Split into key=value items; values of b may hold commas, so a new item starts only
        // at a comma followed by a key and '='.
        let mut items: Vec<String> = Vec::new();
        for part in s.split(',') {
            let is_key = part
                .split_once('=')
                .is_some_and(|(k, _)| ["r", "t", "b", "m", "s", "kn", "gn"].contains(&k));
            match items.last_mut() {
                Some(last) if !is_key => {
                    last.push(',');
                    last.push_str(part);
                }
                _ => items.push(part.to_string()),
            }
        }
        let num = |v: &str, what: &str| -> Result<f64, String> {
            let x = if let Some((a, b)) = v.split_once('/') {
                let (a, b): (f64, f64) = (
                    a.parse()
                        .map_err(|_| format!("Lenia {what}: '{v}' is not a number"))?,
                    b.parse()
                        .map_err(|_| format!("Lenia {what}: '{v}' is not a number"))?,
                );
                a / b
            } else {
                v.parse()
                    .map_err(|_| format!("Lenia {what}: '{v}' is not a number"))?
            };
            if x.is_finite() {
                Ok(x)
            } else {
                Err(format!("Lenia {what}: '{v}' is not finite"))
            }
        };
        for it in &items {
            let (k, v) = it.split_once('=').ok_or_else(|| {
                format!("Lenia parameters look like R=13,T=10,b=1,m=0.15,s=0.015; got '{it}'")
            })?;
            match k {
                "r" => r.r = num(v, "R")?,
                "t" => r.t = num(v, "T")?,
                "m" => r.m = num(v, "m")?,
                "s" => r.s = num(v, "s")?,
                "b" => {
                    r.b = v
                        .split(',')
                        .filter(|x| !x.is_empty())
                        .map(|x| num(x, "b"))
                        .collect::<Result<_, _>>()?
                }
                "kn" => r.kn = num(v, "kn")? as u8,
                "gn" => r.gn = num(v, "gn")? as u8,
                _ => {
                    return Err(format!(
                        "Lenia has no parameter '{k}' (R, T, b, m, s, kn, gn)"
                    ));
                }
            }
        }
        if !(1.0..=64.0).contains(&r.r) {
            return Err("Lenia R (kernel radius in cells) must be 1 to 64".into());
        }
        if !(r.t >= 1.0 && r.t <= 10_000.0) {
            return Err("Lenia T must be 1 to 10000".into());
        }
        if r.b.is_empty() || r.b.len() > 8 || r.b.iter().any(|&x| !(0.0..=1.0).contains(&x)) {
            return Err("Lenia b lists 1 to 8 kernel peaks between 0 and 1".into());
        }
        if r.s <= 0.0 {
            return Err("Lenia s must be positive".into());
        }
        if !(1..=4).contains(&r.kn) || !(1..=3).contains(&r.gn) {
            return Err("Lenia kn is 1-4 and gn is 1-3".into());
        }
        Ok(r)
    }

    pub fn notation(&self) -> String {
        let b: Vec<String> = self.b.iter().map(|&x| frac(x)).collect();
        let mut s = format!(
            "Lenia R={},T={},b={},m={},s={}",
            self.r,
            self.t,
            b.join(","),
            self.m,
            self.s
        );
        if self.kn != 1 {
            s.push_str(&format!(",kn={}", self.kn));
        }
        if self.gn != 1 {
            s.push_str(&format!(",gn={}", self.gn));
        }
        s
    }

    /// The kernel before normalisation at `r` = distance / R.
    pub fn shell(&self, r: f64) -> f64 {
        if !(0.0..1.0).contains(&r) {
            return 0.0;
        }
        let nb = self.b.len();
        let br = nb as f64 * r;
        let peak = self.b[(br.floor() as usize).min(nb - 1)];
        let x = br.fract();
        let core = match self.kn {
            1 => pow4(4.0 * x * (1.0 - x)),
            2 => {
                if x <= 0.0 || x >= 1.0 {
                    0.0
                } else {
                    exp(4.0 - 1.0 / (x * (1.0 - x)))
                }
            }
            3 => ((0.25..=0.75).contains(&x)) as u8 as f64,
            _ => ((0.25..=0.75).contains(&x)) as u8 as f64 + if x < 0.25 { 0.5 } else { 0.0 },
        };
        core * peak
    }

    /// Growth for potential `u`, in [-1, 1].
    pub fn growth(&self, u: f64) -> f64 {
        let d = u - self.m;
        match self.gn {
            1 => pow4((1.0 - d * d / (9.0 * self.s * self.s)).max(0.0)) * 2.0 - 1.0,
            2 => exp(-d * d / (2.0 * self.s * self.s)) * 2.0 - 1.0,
            _ => {
                if d.abs() <= self.s {
                    1.0
                } else {
                    -1.0
                }
            }
        }
    }

    /// An empty field stays empty (growth at zero potential is not positive).
    pub fn quiescent(&self) -> bool {
        self.growth(0.0) <= 0.0
    }
}

/// Precomputed kernel taps. Lattices: per tile type, `(di, dj, type, weight)` relative to the
/// cell's lattice position. Aperiodic patches: per cell, `(cell, weight)`.
#[derive(Debug)]
pub enum Kernel {
    Lattice(Vec<Vec<(i32, i32, u32, f32)>>),
    Explicit(Vec<Vec<(u32, f32)>>),
}

impl Kernel {
    /// Sum of the weights of each type's (or cell's) taps.
    pub fn sums(&self) -> Vec<f64> {
        match self {
            Kernel::Lattice(t) => t
                .iter()
                .map(|v| v.iter().map(|x| x.3 as f64).sum())
                .collect(),
            Kernel::Explicit(t) => t
                .iter()
                .map(|v| v.iter().map(|x| x.1 as f64).sum())
                .collect(),
        }
    }
    pub fn taps(&self) -> usize {
        match self {
            Kernel::Lattice(t) => t.iter().map(|v| v.len()).max().unwrap_or(0),
            Kernel::Explicit(t) => t.iter().map(|v| v.len()).sum(),
        }
    }
}

/// Largest number of kernel taps over all cells of an aperiodic patch.
const MAX_EXPLICIT_TAPS: usize = 60_000_000;

/// Build the kernel for a tiling. Weights are normalised to sum to 1 for each tile type; on
/// aperiodic patches for each cell (cells near the patch edge by the median full kernel, so the
/// cells past the edge count as 0).
pub fn kernel(topo: &Topology, rule: &LeniaRule) -> Result<Kernel, String> {
    kernel_while(topo, rule, &|| true).unwrap_or_else(|| Err("stopped".into()))
}

/// [`kernel`], asking `go` now and then while it works through an aperiodic patch's cells,
/// which takes seconds (about 14 s on one thread for R = 17 on a 64,000-cell Penrose patch);
/// `None` once `go` says stop. A kernel it finishes is [`kernel`]'s.
pub fn kernel_while(
    topo: &Topology,
    rule: &LeniaRule,
    go: &(dyn Fn() -> bool + Sync),
) -> Option<Result<Kernel, String>> {
    let stopped = std::sync::atomic::AtomicBool::new(false);
    let r = kernel_in(topo, rule, &|i| {
        use std::sync::atomic::Ordering::Relaxed;
        if stopped.load(Relaxed) {
            return false;
        }
        if i % 256 == 0 && !go() {
            stopped.store(true, Relaxed);
            return false;
        }
        true
    });
    if stopped.load(std::sync::atomic::Ordering::Relaxed) {
        None
    } else {
        Some(r)
    }
}

/// [`kernel`], asking `go(cell)` before each aperiodic cell's taps (false: they are left empty,
/// and the caller throws the kernel away).
fn kernel_in(
    topo: &Topology,
    rule: &LeniaRule,
    go: &(dyn Fn(usize) -> bool + Sync),
) -> Result<Kernel, String> {
    match &topo.storage {
        Storage::Lattice(g) => {
            let nt = g.ntypes as usize;
            let cs = (crate::geom::cross(g.a1, g.a2).abs() / nt as f64).sqrt();
            let reach = rule.r * cs;
            let cent: Vec<crate::geom::P> = (0..nt)
                .map(|k| {
                    let (m, s) = &g.types[k];
                    let poly: Vec<_> = topo.shapes[*s].iter().map(|&p| m.apply(p)).collect();
                    crate::geom::centroid(&poly)
                })
                .collect();
            let lj = (reach / g.a2[1].abs()).ceil() as i32 + 2;
            let mut out = Vec::with_capacity(nt);
            for (k, ck) in cent.iter().enumerate() {
                let mut taps: Vec<(i32, i32, u32, f64)> = Vec::new();
                for dj in -lj..=lj {
                    let li =
                        ((reach + (dj as f64 * g.a2[0]).abs()) / g.a1[0].abs()).ceil() as i32 + 2;
                    for di in -li..=li {
                        for (k2, c2) in cent.iter().enumerate() {
                            let p = [
                                c2[0] + di as f64 * g.a1[0] + dj as f64 * g.a2[0],
                                c2[1] + di as f64 * g.a1[1] + dj as f64 * g.a2[1],
                            ];
                            let d = dist(p, *ck) / reach;
                            let wt = rule.shell(d);
                            if wt > 0.0 {
                                taps.push((di, dj, k2 as u32, wt));
                            }
                        }
                    }
                }
                let sum: f64 = taps.iter().map(|x| x.3).sum();
                if sum <= 0.0 {
                    return Err(format!(
                        "Lenia kernel is empty for R={} on this tiling",
                        rule.r
                    ));
                }
                let _ = k;
                out.push(
                    taps.into_iter()
                        .map(|(a, b, c, w)| (a, b, c, (w / sum) as f32))
                        .collect(),
                );
            }
            Ok(Kernel::Lattice(out))
        }
        Storage::Explicit(_) => {
            let n = topo.len();
            let cents: Vec<crate::geom::P> = (0..n).map(|i| topo.centroid(i)).collect();
            let area: f64 = (0..n)
                .map(|i| crate::geom::area(&topo.polygon(i)).abs())
                .sum::<f64>()
                / n.max(1) as f64;
            let reach = rule.r * area.sqrt();
            let lists: Vec<Vec<(u32, f64)>> = (0..n)
                .into_par_iter()
                .map(|i| {
                    if !go(i) {
                        return Vec::new();
                    }
                    // Breadth-first over touching cells until past the kernel's reach.
                    let mut seen = std::collections::HashSet::new();
                    let mut q = std::collections::VecDeque::new();
                    seen.insert(i);
                    q.push_back(i);
                    let mut taps: Vec<(u32, f64)> = Vec::new();
                    while let Some(c) = q.pop_front() {
                        let d = dist(cents[c], cents[i]) / reach;
                        let wt = rule.shell(d);
                        if wt > 0.0 {
                            taps.push((c as u32, wt));
                        }
                        topo.for_each_neighbor(c, crate::topology::Nb::Touch, |x| {
                            if dist(cents[x], cents[i]) <= reach * 1.05 + 1e-9 && seen.insert(x) {
                                q.push_back(x);
                            }
                        });
                    }
                    taps.sort_by_key(|x| x.0);
                    taps
                })
                .collect();
            // Normalise by the median full kernel, so cells near the patch's edge see the
            // missing cells as 0 (and interior cells, whose sums differ slightly with the local
            // geometry, by their own sum).
            let sums: Vec<f64> = lists.iter().map(|v| v.iter().map(|x| x.1).sum()).collect();
            let mut sorted = sums.clone();
            sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
            let median = sorted.get(sorted.len() / 2).copied().unwrap_or(0.0);
            if median <= 0.0 {
                return Err(format!(
                    "Lenia kernel is empty for R={} on this patch",
                    rule.r
                ));
            }
            let lists: Vec<Vec<(u32, f32)>> = lists
                .into_iter()
                .zip(&sums)
                .map(|(v, &s)| {
                    let norm = s.max(median);
                    v.into_iter().map(|(c, w)| (c, (w / norm) as f32)).collect()
                })
                .collect();
            let total: usize = lists.iter().map(|v| v.len()).sum();
            if total > MAX_EXPLICIT_TAPS {
                return Err(format!(
                    "Lenia R={} on this patch needs {total} kernel taps; use a smaller R or patch",
                    rule.r
                ));
            }
            Ok(Kernel::Explicit(lists))
        }
    }
}

/// One Lenia step on a continuous field.
pub fn step(topo: &Topology, rule: &LeniaRule, k: &Kernel, f: &mut Field) {
    if f.kind != FieldKind::Continuous {
        return;
    }
    let dt = 1.0 / rule.t;
    let a = &f.value;
    let update = |i: usize, u: f32| -> f32 {
        (a[i] as f64 + dt * rule.growth(u as f64)).clamp(0.0, 1.0) as f32
    };
    match (k, &topo.storage) {
        (Kernel::Lattice(types), Storage::Lattice(g)) => {
            let (w, h, nt) = (g.w as i32, g.h as i32, g.ntypes as usize);
            let row_len = w as usize * nt;
            f.back_v
                .par_chunks_mut(row_len)
                .enumerate()
                .for_each(|(j, out)| {
                    let j = j as i32;
                    let rs = |jj: i32| -> i32 { g.row_start[jj.rem_euclid(h) as usize] };
                    for (k, taps) in types.iter().enumerate() {
                        // Per row and type: linear offsets for cells whose taps all stay inside.
                        let mut lin: Vec<(isize, f32)> = Vec::with_capacity(taps.len());
                        let (mut umin, mut umax) = (0i32, w - 1);
                        let mut rows_ok = true;
                        for &(di, dj, k2, wt) in taps {
                            let jj = j + dj;
                            if jj < 0 || jj >= h {
                                rows_ok = false;
                                break;
                            }
                            let du = di + rs(j) - rs(jj);
                            umin = umin.max(-du);
                            umax = umax.min(w - 1 - du);
                            lin.push((
                                (dj as isize * w as isize + du as isize) * nt as isize
                                    + k2 as isize
                                    - k as isize,
                                wt,
                            ));
                        }
                        if !rows_ok {
                            (umin, umax) = (0, -1);
                        }
                        for u in 0..w {
                            let i = (j as usize * w as usize + u as usize) * nt + k;
                            let mut sum = 0f32;
                            if u >= umin && u <= umax {
                                for &(o, wt) in &lin {
                                    sum += wt * a[(i as isize + o) as usize];
                                }
                            } else {
                                let base = u + g.row_start[j as usize];
                                for &(di, dj, k2, wt) in taps {
                                    if let Some(x) = g.resolve(base + di, j + dj, k2) {
                                        sum += wt * a[x];
                                    }
                                }
                            }
                            out[u as usize * nt + k] = update(i, sum);
                        }
                    }
                });
        }
        (Kernel::Explicit(lists), _) => {
            f.back_v
                .par_iter_mut()
                .enumerate()
                .with_min_len(1024)
                .for_each(|(i, o)| {
                    let mut sum = 0f32;
                    for &(c, wt) in &lists[i] {
                        sum += wt * a[c as usize];
                    }
                    *o = update(i, sum);
                });
        }
        _ => return,
    }
    std::mem::swap(&mut f.value, &mut f.back_v);
    f.sync_mirror();
}

/// Orbium unicaudatus (Chan 2019), from `animals.json` in Chan's reference code
/// (github.com/Chakazul/Lenia, MIT licence, Copyright (c) 2018 Bert Chan), in Chan's run-length
/// format: `.` 0, `A`-`X` 1-24, `pA`-`yO` 25-255 (two letters), counts before a symbol repeat
/// it, `$` ends a row, `!` ends the pattern. Values are /255. Rule: R=13, T=10, b=1, m=0.15,
/// s=0.015 (kernel and growth 1, polynomial).
pub const ORBIUM: &str = "7.MD6.qL$6.pKqEqFURpApBRAqQ$5.VqTrSsBrOpXpWpTpWpUpCrQ$4.CQrQsTsWsApITNPpGqGvL$3.IpIpWrOsGsBqXpJ4.LsFrL$A.DpKpSpJpDqOqUqSqE5.ExD$qL.pBpTT2.qCrGrVrWqM5.sTpP$.pGpWpD3.qUsMtItQtJ6.tL$.uFqGH3.pXtOuR2vFsK5.sM$.tUqL4.GuNwAwVxBwNpC4.qXpA$2.uH5.vBxGyEyMyHtW4.qIpL$2.wV5.tIyG3yOxQqW2.FqHpJ$2.tUS4.rM2yOyJyOyHtVpPMpFqNV$2.HsR4.pUxAyOxLxDxEuVrMqBqGqKJ$3.sLpE3.pEuNxHwRwGvUuLsHrCqTpR$3.TrMS2.pFsLvDvPvEuPtNsGrGqIP$4.pRqRpNpFpTrNtGtVtStGsMrNqNpF$5.pMqKqLqRrIsCsLsIrTrFqJpHE$6.RpSqJqPqVqWqRqKpRXE$8.OpBpIpJpFTK!";

/// Most values a decoded Lenia pattern may hold, rows times columns once short rows are padded:
/// 2^24 (16,777,216 values, 64 MiB of `f32`). Chan's published creatures are a few hundred
/// values across at most, and a pattern is stamped onto a field of at most
/// [`crate::topology::MAX_CELLS`] cells, so this leaves room for any real pattern while a text of
/// a few bytes can no longer ask for gigabytes.
pub const MAX_PATTERN_VALUES: usize = 1 << 24;

/// Most rows, and most values in a row, a decoded Lenia pattern may have: 2^16 (65,536). Rows
/// are counted separately from [`MAX_PATTERN_VALUES`] because empty rows hold no values but still
/// cost memory.
pub const MAX_PATTERN_SIDE: usize = 1 << 16;

/// Decode Chan's 2D run-length format into rows of values in [0, 1] (short rows padded with 0).
///
/// A text that is not in that format is refused with an error, and so is one that would decode
/// to more than [`MAX_PATTERN_SIDE`] rows or columns, or more than [`MAX_PATTERN_VALUES`] values
/// in all. The sizes are checked before anything is allocated, so a run count of any size costs
/// nothing.
pub fn decode_rle(st: &str) -> Result<Vec<Vec<f32>>, String> {
    let mut rows: Vec<Vec<f32>> = vec![vec![]];
    let mut width = 0usize;
    let mut count = String::new();
    let mut prefix: Option<char> = None;
    let take = |count: &mut String| -> Result<usize, String> {
        let n = if count.is_empty() {
            1
        } else {
            count
                .parse::<usize>()
                .ok()
                .filter(|&n| n <= MAX_PATTERN_SIDE)
                .ok_or_else(|| {
                    let shown = if count.len() > 20 {
                        format!("{}… ({} digits)", &count[..20], count.len())
                    } else {
                        count.clone()
                    };
                    format!(
                        "Lenia RLE run count {shown} is too large (a pattern is at most \
                         {MAX_PATTERN_SIDE} values across and {MAX_PATTERN_SIDE} rows)"
                    )
                })?
        };
        count.clear();
        Ok(n)
    };
    // Rows times the widest row so far: what the padded pattern would hold.
    let fits = |rows: usize, width: usize| -> Result<(), String> {
        if rows > MAX_PATTERN_SIDE || width > MAX_PATTERN_SIDE {
            return Err(format!(
                "Lenia RLE pattern is too large: {width} values across and {rows} rows (at most \
                 {MAX_PATTERN_SIDE} of each)"
            ));
        }
        if rows.saturating_mul(width) > MAX_PATTERN_VALUES {
            return Err(format!(
                "Lenia RLE pattern is too large: {width} values across and {rows} rows, more \
                 than {MAX_PATTERN_VALUES} values in all"
            ));
        }
        Ok(())
    };
    for ch in st.trim().trim_end_matches('!').chars() {
        if ch.is_ascii_digit() || ch == '$' || ('p'..='y').contains(&ch) {
            // A two-letter value's first letter must be followed by its second.
            if let Some(p) = prefix {
                return Err(format!("bad Lenia RLE symbol {p}{ch}"));
            }
        }
        if ch.is_ascii_digit() {
            count.push(ch);
        } else if ('p'..='y').contains(&ch) {
            prefix = Some(ch);
        } else if ch == '$' {
            let n = take(&mut count)?;
            fits(rows.len() + n, width)?;
            for _ in 0..n {
                rows.push(vec![]);
            }
        } else {
            let v: u32 = match (prefix.take(), ch) {
                (None, '.') | (None, 'b') => 0,
                (None, 'o') => 255,
                (None, c @ 'A'..='X') => c as u32 - 'A' as u32 + 1,
                (Some(p), c @ 'A'..='X') => {
                    (p as u32 - 'p' as u32) * 24 + (c as u32 - 'A' as u32 + 25)
                }
                (p, c) => return Err(format!("bad Lenia RLE symbol {p:?}{c}")),
            };
            let n = take(&mut count)?;
            let len = rows.last().map_or(0, |r| r.len()) + n;
            fits(rows.len(), width.max(len))?;
            width = width.max(len);
            let row = rows.last_mut().unwrap();
            row.extend(std::iter::repeat_n(v.min(255) as f32 / 255.0, n));
        }
    }
    if let Some(p) = prefix {
        return Err(format!("bad Lenia RLE symbol {p} at the end"));
    }
    while rows.last().is_some_and(|r| r.is_empty()) {
        rows.pop();
    }
    for r in &mut rows {
        r.resize(width, 0.0);
    }
    Ok(rows)
}

/// A built-in Lenia pattern by name, as rows of values.
pub fn pattern(name: &str) -> Option<Vec<Vec<f32>>> {
    match name.to_ascii_lowercase().as_str() {
        "orbium" | "orbium unicaudatus" | "o2u" => decode_rle(ORBIUM).ok(),
        _ => None,
    }
}

#[cfg(test)]
mod rle_tests {
    use super::*;

    /// Orbium decodes as it always did: 20 by 20, every value in [0, 1].
    #[test]
    fn orbium_still_decodes() {
        let p = decode_rle(ORBIUM).unwrap();
        assert_eq!((p.len(), p[0].len()), (20, 20));
        assert!(p.iter().all(|r| r.len() == 20));
        assert_eq!(p[0][7], 13.0 / 255.0);
        assert_eq!(pattern("orbium"), Some(p));
    }

    /// Run counts of any size are refused with an error, before anything is allocated: one too
    /// many digits for a number, one past the side, and the largest that fits.
    #[test]
    fn huge_run_counts_are_refused() {
        let e = decode_rle(&format!("{}A!", "9".repeat(100_000))).unwrap_err();
        assert!(e.contains("(100000 digits)") && e.len() < 200, "{e}");
        for text in [
            "99999999999999999999999999A!",
            "18446744073709551616.!",
            "4294967296o!",
            "65537A!",
            "65537$A!",
            "4000000000$A!",
            "A99999999999999999999$B!",
        ] {
            let e = decode_rle(text).unwrap_err();
            assert!(e.contains("too large"), "{text}: {e}");
        }
        let p = decode_rle("65536A!").unwrap();
        assert_eq!((p.len(), p[0].len()), (1, MAX_PATTERN_SIDE));
    }

    /// Rows times the widest row is what the padded pattern holds: refused past
    /// MAX_PATTERN_VALUES even when every run is small, and let through just under it.
    #[test]
    fn a_pattern_just_under_and_over_the_limit() {
        // 256 rows of one value, then a row 65,536 wide: 257 x 65,536 > 2^24 once padded.
        let over = format!("{}65536A!", "A$".repeat(256));
        let e = decode_rle(&over).unwrap_err();
        assert!(e.contains("more than 16777216 values"), "{e}");
        // 255 rows of one value, then a row 65,536 wide: 256 x 65,536 = 2^24 exactly.
        let under = format!("{}65536A!", "A$".repeat(255));
        let p = decode_rle(&under).unwrap();
        assert_eq!(p.len() * p[0].len(), MAX_PATTERN_VALUES);
        // Long rows built from many small runs count as much as one long run.
        let long = format!("{}A!", "9999A".repeat(7));
        assert!(decode_rle(&long).unwrap_err().contains("too large"));
    }

    /// A first letter of a two-letter value must be followed by its second.
    #[test]
    fn malformed_text_is_refused() {
        for text in ["pA$p$A!", "p3A!", "pqA!", "AB p!", "Z!", "pZ!", "A p"] {
            assert!(decode_rle(text).is_err(), "{text}");
        }
        assert_eq!(decode_rle("").unwrap(), Vec::<Vec<f32>>::new());
        assert_eq!(decode_rle("2$!").unwrap(), Vec::<Vec<f32>>::new());
    }
}
