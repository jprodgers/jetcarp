//! Behaviour measures: from a world's trace and frames to a fixed feature vector, a fate, and
//! the moving objects seen in its last frames.

use super::batch::WorldTrace;
use super::space::Grid;
use crate::looks::{compressed_bits, state_entropy};
use crate::topology::Nb;
use serde::{Deserialize, Serialize};

/// How a run ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Fate {
    /// The population reached zero.
    Died,
    /// The pattern stopped changing.
    Froze,
    /// The pattern repeats with a period above 1.
    Cycle,
    /// Still doing something new at the end of the run.
    Running,
}

/// The measures of one run. Densities and activities are fractions of cells.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Features {
    /// Generation the run settled (died, froze, or entered its cycle); the run length if it never
    /// did.
    pub settle: u32,
    /// Period of the final cycle (1 frozen, 0 none seen).
    pub period: u32,
    /// Population at the end, as a fraction of cells.
    pub density: f32,
    /// Mean density over the last quarter.
    pub density_late: f32,
    /// Mean fraction of cells changing per generation over the last quarter.
    pub activity_late: f32,
    /// The same over the first quarter.
    pub activity_early: f32,
    /// Coefficient of variation of the population over the last half.
    pub pop_cv: f32,
    /// log2 of (late population / early population).
    pub growth: f32,
    /// Peak density reached.
    pub peak_density: f32,
    /// Shannon entropy of the final states (bits), divided by log2 of the states seen.
    pub entropy: f32,
    /// Bits per cell to store the last frame, deflated.
    pub bits: f32,
    /// Fraction of neighbour pairs in the same state.
    pub same_nb: f32,
    /// `same_nb` relative to what shuffled cells would give: 0 like noise, 1 fully smooth.
    pub smooth: f32,
    /// Connected groups of live cells per live cell (1 is dust, small for few big blobs).
    pub comps_per_live: f32,
    /// Largest group / live cells.
    pub largest: f32,
    /// Entropy of the group-size distribution (log2-spaced buckets), bits.
    pub size_entropy: f32,
    /// Standard deviation of density over coarse spatial bins, relative to the mean density.
    pub patchiness: f32,
    /// Cells that changed at all during the last frames / mean changes per generation over those
    /// frames, divided by the window: near 1/window for flicker in place, near 1 for changes that
    /// travel.
    pub travel: f32,
    /// Moving objects seen.
    pub movers: u32,
    /// Mean speed of the movers, in tiles per generation.
    pub mover_speed: f32,
}

impl Features {
    /// A flat numeric vector (for distances).
    pub fn vector(&self, gens: u32) -> [f32; 16] {
        let tr = (1.0 + self.settle as f32).log2() / (1.0 + gens as f32).log2();
        [
            tr,
            self.density_late,
            self.activity_late.powf(0.5),
            self.activity_early.powf(0.5),
            self.pop_cv.min(2.0),
            self.growth.clamp(-4.0, 4.0) / 4.0,
            self.entropy,
            self.bits.min(4.0) / 4.0,
            self.smooth,
            self.comps_per_live.powf(0.5),
            self.largest,
            self.size_entropy / 4.0,
            self.patchiness.min(3.0) / 3.0,
            self.travel,
            (1.0 + self.movers as f32).log2() / 6.0,
            (1.0 + self.period as f32).log2() / 8.0,
        ]
    }
}

/// The fate of a run, with its settle generation and period, from the per-generation series.
pub fn fate_of(t: &WorldTrace) -> (Fate, u32, u32) {
    let last = t.pop.len() - 1;
    if t.pop[last] == 0 {
        let at = t.pop.iter().position(|&p| p == 0).unwrap_or(last);
        return (Fate::Died, at as u32, 0);
    }
    // The most recent earlier generation that looks exactly like the last one.
    let mut prior = None;
    for x in (0..last).rev() {
        if t.hash[x] == t.hash[last] && t.pop[x] == t.pop[last] {
            prior = Some(x);
            break;
        }
    }
    let Some(prior) = prior else {
        return (Fate::Running, last as u32, 0);
    };
    let p = last - prior;
    let mut s = prior;
    while s > 0 && t.hash[s - 1] == t.hash[s - 1 + p] && t.pop[s - 1] == t.pop[s - 1 + p] {
        s -= 1;
    }
    (
        if p == 1 { Fate::Froze } else { Fate::Cycle },
        s as u32,
        p as u32,
    )
}

/// Union-find with path halving.
struct Dsu(Vec<u32>);
impl Dsu {
    fn find(&mut self, mut x: u32) -> u32 {
        while self.0[x as usize] != x {
            let p = self.0[x as usize];
            self.0[x as usize] = self.0[p as usize];
            x = self.0[x as usize];
        }
        x
    }
    fn union(&mut self, a: u32, b: u32) {
        let (a, b) = (self.find(a), self.find(b));
        if a != b {
            self.0[a.max(b) as usize] = a.min(b);
        }
    }
}

/// A connected group of live cells.
#[derive(Clone, Copy, Debug)]
struct Comp {
    size: u32,
    x: f32,
    y: f32,
}

/// Groups of live cells linked through the touch neighbourhood. `max_size` leaves out big groups
/// from `small`, and `all_sizes` lists every group size.
fn components(g: &Grid, frame: &[u8], max_size: u32) -> (Vec<Comp>, Vec<u32>) {
    let n = frame.len();
    let mut dsu = Dsu((0..n as u32).collect());
    let csr = g.csr(Nb::Touch);
    for i in 0..n {
        if frame[i] == 0 {
            continue;
        }
        for &x in &csr.nb[csr.off[i] as usize..csr.off[i + 1] as usize] {
            if frame[x as usize] != 0 {
                dsu.union(i as u32, x);
            }
        }
    }
    let wrap = wrap_vectors(g);
    let mut slot = vec![u32::MAX; n];
    let mut acc: Vec<(u32, f64, f64, [f32; 2])> = Vec::new();
    for i in 0..n {
        if frame[i] == 0 {
            continue;
        }
        let r = dsu.find(i as u32) as usize;
        if slot[r] == u32::MAX {
            slot[r] = acc.len() as u32;
            acc.push((0, 0.0, 0.0, g.cent[i]));
        }
        let e = &mut acc[slot[r] as usize];
        let (dx, dy) = delta(&wrap, e.3, g.cent[i]);
        e.0 += 1;
        e.1 += dx as f64;
        e.2 += dy as f64;
    }
    let mut small = Vec::new();
    let mut sizes = Vec::with_capacity(acc.len());
    for (s, x, y, base) in acc {
        sizes.push(s);
        if s <= max_size {
            small.push(Comp {
                size: s,
                x: base[0] + (x / s as f64) as f32,
                y: base[1] + (y / s as f64) as f32,
            });
        }
    }
    (small, sizes)
}

/// The torus of a wrapped grid.
struct Wrap {
    /// Empty for grids with an edge.
    v: Vec<[f32; 2]>,
    /// Displacements with a smaller squared length than this cannot be shortened by wrapping.
    safe2: f32,
}

/// The torus vectors of a wrapped grid.
fn wrap_vectors(g: &Grid) -> Wrap {
    let v = torus_vectors(g);
    let safe2 = if v.len() == 2 {
        let n = |a: [f32; 2]| a[0] * a[0] + a[1] * a[1];
        0.2 * n(v[0]).min(n(v[1]))
    } else {
        f32::MAX
    };
    Wrap { v, safe2 }
}

fn torus_vectors(g: &Grid) -> Vec<[f32; 2]> {
    match g.topo.lattice() {
        Some(l) if l.wrap => {
            let (w, h, shift) = (l.w as f64, l.h as f64, l.wrap_shift as f64);
            vec![
                [(l.a1[0] * w) as f32, (l.a1[1] * w) as f32],
                [
                    (l.a2[0] * h + l.a1[0] * shift) as f32,
                    (l.a2[1] * h + l.a1[1] * shift) as f32,
                ],
            ]
        }
        _ => Vec::new(),
    }
}

/// The shortest displacement from `a` to `b` on the grid's torus (straight on grids with edges).
#[inline]
fn delta(wrap: &Wrap, a: [f32; 2], b: [f32; 2]) -> (f32, f32) {
    let (mut dx, mut dy) = (b[0] - a[0], b[1] - a[1]);
    let d0 = dx * dx + dy * dy;
    if d0 >= wrap.safe2 {
        let wrap = &wrap.v;
        let mut best = d0;
        let (ox, oy) = (dx, dy);
        for i in -1..=1 {
            for j in -1..=1 {
                let cx = ox + i as f32 * wrap[0][0] + j as f32 * wrap[1][0];
                let cy = oy + i as f32 * wrap[0][1] + j as f32 * wrap[1][1];
                let d = cx * cx + cy * cy;
                if d < best {
                    best = d;
                    dx = cx;
                    dy = cy;
                }
            }
        }
    }
    (dx, dy)
}

/// Largest connected group that may count as a moving object.
pub const MOVER_MAX_SIZE: u32 = 64;

/// Track small objects through consecutive frames; returns (count, mean speed in tiles per
/// generation). An object is a mover if it can be followed through every frame with a similar
/// size, and it ends up clearly away from where it began along a fairly straight path.
///
/// A step can jump up to `2.6` tiles (fast rules move a cell per generation; a centroid shifts
/// more when the shape changes phase).
pub fn track_movers(g: &Grid, frames: &[&[u8]]) -> (u32, f32) {
    let m = frames.len();
    if m < 3 {
        return (0, 0.0);
    }
    let tile = g.tile.max(1e-6);
    let wrap = wrap_vectors(g);
    let reach = 2.6 * tile;
    let reach2 = reach * reach;
    let mut levels: Vec<Vec<Comp>> = Vec::with_capacity(m);
    for f in frames {
        let (c, _) = components(g, f, MOVER_MAX_SIZE);
        if c.is_empty() || c.len() > 6000 {
            return (0, 0.0);
        }
        levels.push(c);
    }
    let mut count = 0u32;
    let mut speed = 0f32;
    // From each object in the first frame, follow the nearest similar object forwards.
    'obj: for start in 0..levels[0].len() {
        let mut cur = levels[0][start];
        let (mut sx, mut sy) = (0f32, 0f32);
        let mut path = 0f32;
        let mut min_size = cur.size;
        let mut max_size = cur.size;
        for lev in &levels[1..] {
            let mut best: Option<(f32, f32, Comp)> = None;
            let mut best_d = reach2;
            for c in lev {
                if c.size * 2 < cur.size || c.size > cur.size * 2 {
                    continue;
                }
                let (dx, dy) = delta(&wrap, [cur.x, cur.y], [c.x, c.y]);
                let d = dx * dx + dy * dy;
                if d <= best_d {
                    best_d = d;
                    best = Some((dx, dy, *c));
                }
            }
            let Some((dx, dy, c)) = best else {
                continue 'obj;
            };
            sx += dx;
            sy += dy;
            path += (dx * dx + dy * dy).sqrt();
            min_size = min_size.min(c.size);
            max_size = max_size.max(c.size);
            cur = c;
        }
        let net = (sx * sx + sy * sy).sqrt();
        let gens = (m - 1) as f32;
        // At least 2 tiles of net travel and a fairly straight path.
        if net >= 2.0 * tile && net >= 0.55 * path && max_size <= min_size * 3 {
            count += 1;
            speed += net / gens / tile;
        }
    }
    (
        count,
        if count > 0 {
            speed / count as f32
        } else {
            0.0
        },
    )
}

/// Measure a run. `window` is how many of the last frames (consecutive generations, ending at
/// the run's last generation) were requested with [`frames_for`]; the frames before them are
/// ignored here.
pub fn measure_run(g: &Grid, t: &WorldTrace, gens: u32, window: usize) -> (Fate, Features) {
    let n = g.topo.len() as f32;
    let (fate, settle, period) = fate_of(t);
    let last = t.pop.len() - 1;
    let q = last / 4;
    let mean = |v: &[u32]| -> f32 {
        if v.is_empty() {
            0.0
        } else {
            v.iter().map(|&x| x as f64).sum::<f64>() as f32 / v.len() as f32
        }
    };
    let density = t.pop[last] as f32 / n;
    let density_late = mean(&t.pop[last - q..=last]) / n;
    let activity_late = mean(&t.changed[last - q..=last]) / n;
    let activity_early = if last > 0 {
        mean(&t.changed[1..=last.max(q + 1).min(q + 1)]) / n
    } else {
        0.0
    };
    let half = &t.pop[last / 2..=last];
    let m = mean(half);
    let pop_cv = if m > 0.0 {
        let v: f64 = half
            .iter()
            .map(|&x| {
                let d = x as f64 - m as f64;
                d * d
            })
            .sum::<f64>()
            / half.len() as f64;
        v.sqrt() as f32 / m
    } else {
        0.0
    };
    let early = mean(&t.pop[..=q]).max(1.0);
    let growth = (mean(&t.pop[last - q..=last]).max(1.0) / early).log2();
    let peak_density = t.pop.iter().copied().max().unwrap_or(0) as f32 / n;

    let mut f = Features {
        settle,
        period,
        density,
        density_late,
        activity_late,
        activity_early,
        pop_cv,
        growth,
        peak_density,
        ..Default::default()
    };
    let wnd = window.min(t.frames.len());
    if wnd == 0 {
        return (fate, f);
    }
    let frames = &t.frames[t.frames.len() - wnd..];
    let end = frames.last().unwrap();
    frame_features(g, end, &mut f);
    if wnd >= 2 {
        let mut touched = vec![false; end.len()];
        let mut total = 0u64;
        for pair in frames.windows(2) {
            for (i, (a, b)) in pair[0].iter().zip(pair[1].iter()).enumerate() {
                if a != b {
                    touched[i] = true;
                    total += 1;
                }
            }
        }
        let area = touched.iter().filter(|&&x| x).count() as f32;
        let per = total as f32 / (wnd - 1) as f32;
        f.travel = if per > 0.0 {
            (area / per / (wnd - 1) as f32).clamp(0.0, 1.0)
        } else {
            0.0
        };
    }
    // Movers only make sense while the board is mostly empty.
    if density < 0.35 && fate != Fate::Died && wnd >= 3 {
        let refs: Vec<&[u8]> = frames.iter().map(|v| v.as_slice()).collect();
        let (c, s) = track_movers(g, &refs);
        f.movers = c;
        f.mover_speed = s;
    }
    let _ = gens;
    (fate, f)
}

fn frame_features(g: &Grid, frame: &[u8], f: &mut Features) {
    let n = frame.len().max(1);
    let mut used = [false; 256];
    for &s in frame {
        used[s as usize] = true;
    }
    let distinct = used.iter().filter(|&&x| x).count();
    f.entropy = if distinct > 1 {
        (state_entropy(frame) / (distinct as f64).log2()) as f32
    } else {
        0.0
    };
    f.bits = compressed_bits(frame) as f32;
    // Same-state neighbour pairs.
    let csr = g.csr(Nb::Touch);
    let (mut same, mut pairs) = (0u64, 0u64);
    let mut counts = [0u64; 256];
    for i in 0..n {
        counts[frame[i] as usize] += 1;
        for &x in &csr.nb[csr.off[i] as usize..csr.off[i + 1] as usize] {
            same += (frame[i] == frame[x as usize]) as u64;
            pairs += 1;
        }
    }
    f.same_nb = same as f32 / pairs.max(1) as f32;
    let expect: f32 = counts
        .iter()
        .map(|&c| {
            let p = c as f32 / n as f32;
            p * p
        })
        .sum();
    f.smooth = if expect < 1.0 {
        ((f.same_nb - expect) / (1.0 - expect)).clamp(0.0, 1.0)
    } else {
        0.0
    };
    // Connected groups.
    let (_, sizes) = components(g, frame, 0);
    let live: u64 = sizes.iter().map(|&s| s as u64).sum();
    if live > 0 {
        f.comps_per_live = sizes.len() as f32 / live as f32;
        f.largest = *sizes.iter().max().unwrap() as f32 / live as f32;
        let mut hist = [0u64; 24];
        for &s in &sizes {
            hist[(31 - s.leading_zeros()) as usize] += 1;
        }
        let total = sizes.len() as f64;
        f.size_entropy = -hist
            .iter()
            .filter(|&&h| h > 0)
            .map(|&h| {
                let p = h as f64 / total;
                p * p.log2()
            })
            .sum::<f64>() as f32;
    }
    // Spatial patchiness.
    let mut live_in = vec![0u32; g.nbins];
    let mut all_in = vec![0u32; g.nbins];
    for (i, &s) in frame.iter().enumerate() {
        let b = g.bin[i] as usize;
        all_in[b] += 1;
        live_in[b] += (s != 0) as u32;
    }
    let dens: Vec<f32> = live_in
        .iter()
        .zip(&all_in)
        .filter(|(_, a)| **a >= 6)
        .map(|(l, a)| *l as f32 / *a as f32)
        .collect();
    if !dens.is_empty() {
        let m = dens.iter().sum::<f32>() / dens.len() as f32;
        if m > 1e-6 {
            let v = dens.iter().map(|d| (d - m) * (d - m)).sum::<f32>() / dens.len() as f32;
            f.patchiness = v.sqrt() / m;
        }
    }
}

/// The frame generations to ask for: `window` consecutive generations ending at `gens`.
pub fn frames_for(gens: u32, window: usize) -> Vec<u32> {
    let w = window as u32;
    let start = gens.saturating_sub(w - 1);
    (start..=gens).collect()
}
