//! Pattern seeds: where cells may be set ([`SeedShape`]) and what goes into them ([`SeedFill`]).
//!
//! `SeedSpec::Pattern { shape, fill }` works on every tiling: shapes use only cell centroids, the
//! tiling's bounds, and the typical tile size ([`crate::world::cell_size`]), never lattice
//! coordinates. Unless a field name says `_tiles`, sizes are fractions of the tiling's shorter
//! side (so `radius: 0.5` reaches the edges). Angles are in degrees.
//!
//! A seed draws two numbers from the world RNG, however large the tiling is. Everything else is
//! derived from those, and per-cell decisions are hashes of the cell, so a seed is deterministic
//! for a given `rng_state` and independent of the thread count.
//!
//! Wrap-edge lattices wrap blobs, clusters, and Voronoi regions across their edges; stripes,
//! checkers, gradients, and noise are laid out across the grid's rectangle and may show a seam.
//!
//! # What a fill means for each field kind
//!
//! A cell inside the shape is chosen with probability `density` (times the shape's weight, for
//! the gradients). Then, by field:
//!
//! | Field | Result |
//! |---|---|
//! | binary | state 1, whatever `states` says (except [`SeedStates::Signed`], which is refused) |
//! | multi-state | `states` picks a nonzero state; states past the field's last are clamped to it |
//! | JACA | as multi-state: `Uniform` and `ByRegion` pick +1 or -1, `One` is +1 |
//! | continuous | no coin: the value is `density x weight x edge x amplitude` (below) |
//! | height | no coin: mean grains are `2 x density x weight x edge x amplitude x degree` (below) |
//!
//! Continuous and height fields use a smooth *edge* factor that falls from 1 inside a disc, blob,
//! cluster, ring, stripe, or noise patch to 0 on its outline. Discrete fields have hard outlines.
//!
//! The amplitude comes from `states`:
//!
//! - `One`: 0.75 to 1, per cell (a little roughness, as a perfectly flat field stays flat)
//! - `Uniform`: in (0, 1], per cell
//! - `ByRegion`: in (0.25, 1], per region
//! - `Range { lo, hi }`: continuous fields, `lo / 255` to `hi / 255`, per cell; height fields do
//!   not use an amplitude, but take `lo..=hi` grains on a cell chosen with probability
//!   `density x weight`
//!
//! So on a sandpile `density: 0.5` puts about one toppling's worth of grains (the cell's degree)
//! on each chosen cell, and `1.0` twice that. Grains are added to what is there; fractions are
//! rounded up or down at random so the mean is exact. `Signed` is refused on these fields.

use crate::detmath::{dist, sin_cos};
use crate::geom::P;
use crate::jaca;
use crate::reseed::{Outline, mix, unit};
use crate::rng::Rng;
use crate::rules::{FieldKind, Rule};
use crate::topology::{Nb, Topology};
use crate::world::{Field, cell_size};
use rayon::prelude::*;
use serde::{Deserialize, Serialize};

/// [`Placement::region`] of a cell outside the shape.
pub const NO_REGION: u32 = u32::MAX;

/// Where a seed may set cells. In JSON, an object with a `kind`; missing parameters take defaults.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SeedShape {
    /// Every cell.
    All,
    /// A disc at the centre of the tiling.
    Disc {
        #[serde(default = "d_radius")]
        radius: f64,
    },
    /// Round wobbly blobs at random cells. `wobble` scales the outline's lumps (0 is a circle, 1
    /// matches the reseed blobs, at most 2). A positive `min_gap` keeps blob outlines at least
    /// that far apart, and at 0 blobs may overlap. If the gap leaves no room, fewer are placed.
    Blobs {
        #[serde(default = "d_blobs")]
        count: u32,
        #[serde(default = "d_min_radius")]
        min_radius: f64,
        #[serde(default = "d_max_radius")]
        max_radius: f64,
        #[serde(default = "d_one")]
        wobble: f64,
        #[serde(default = "d_min_gap")]
        min_gap: f64,
    },
    /// Many tiny round clusters at random cells, `radius_tiles` tiles in radius.
    Clusters {
        #[serde(default = "d_clusters")]
        count: u32,
        #[serde(default = "d_radius_tiles")]
        radius_tiles: f64,
    },
    /// Groups of 3 to 5 touching cells on a staggered grid `spacing_tiles` apart. `jitter` moves
    /// each group by up to half the spacing times `jitter` (0 is regular, 1 is well shaken).
    Dots {
        #[serde(default = "d_spacing")]
        spacing_tiles: f64,
        #[serde(default = "d_jitter")]
        jitter: f64,
    },
    /// A ring around the centre; `width` is its full width.
    Ring {
        #[serde(default = "d_radius")]
        radius: f64,
        #[serde(default = "d_width")]
        width: f64,
    },
    /// `count` concentric rings, evenly spaced from the centre to the edge.
    Rings {
        #[serde(default = "d_rings")]
        count: u32,
        #[serde(default = "d_width")]
        width: f64,
    },
    /// Parallel stripes running at `angle` (0 is along x). `period` is the repeat distance and
    /// `duty` the share of each period that is filled.
    Stripes {
        #[serde(default = "d_period")]
        period: f64,
        #[serde(default = "d_duty")]
        duty: f64,
        #[serde(default)]
        angle: f64,
    },
    /// A checkerboard of squares `size` across, by centroid.
    Checker {
        #[serde(default = "d_period")]
        size: f64,
    },
    /// Density multiplied by `low` on one side of the grid, rising linearly to `high` on the
    /// other, in the direction `angle` (0 rises along x).
    Gradient {
        #[serde(default)]
        angle: f64,
        #[serde(default)]
        low: f64,
        #[serde(default = "d_one")]
        high: f64,
    },
    /// Density multiplied by `centre` at the centre and `edge` from half the shorter side out.
    RadialGradient {
        #[serde(default = "d_one")]
        centre: f64,
        #[serde(default)]
        edge: f64,
    },
    /// Organic patches: value noise of feature size `scale`, kept where it exceeds `threshold`
    /// (the noise lies in 0 to 1, centred near 0.5). Connected patches are regions.
    Noise {
        #[serde(default = "d_scale")]
        scale: f64,
        #[serde(default = "d_duty")]
        threshold: f64,
        #[serde(default = "d_octaves")]
        octaves: u32,
    },
    /// Random sites partition the grid; a fraction `keep` of the regions (rounded to a whole
    /// number of them) is filled.
    Voronoi {
        #[serde(default = "d_sites")]
        sites: u32,
        #[serde(default = "d_duty")]
        keep: f64,
    },
    /// A random seed in a disc, symmetric under rotation by a full turn / `order` (and a mirror if
    /// set). The centre and mirror axis are picked near the middle of the tiling as the best
    /// the tiling allows, so the seed is exact where the tiling has that symmetry (see
    /// [`symmetry_frame`]), and close to it elsewhere (aperiodic patches, mismatched orders).
    /// Cells whose image falls off the tiling are left out, which keeps the seed symmetric.
    Symmetric {
        #[serde(default = "d_order")]
        order: u32,
        #[serde(default)]
        mirror: bool,
        #[serde(default = "d_radius")]
        radius: f64,
    },
}

/// What goes into the cells a shape chooses. See the [module docs](self) for each field kind.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SeedFill {
    /// The probability a chosen cell is set; 1.0 is solid.
    #[serde(default = "d_one", skip_serializing_if = "is_one")]
    pub density: f64,
    #[serde(default, skip_serializing_if = "SeedStates::is_one")]
    pub states: SeedStates,
}

impl Default for SeedFill {
    fn default() -> Self {
        SeedFill {
            density: 1.0,
            states: SeedStates::One,
        }
    }
}

/// Which state a set cell gets. In JSON: `"one"`, `"uniform"`, `"by_region"`,
/// `{"range": {"lo": 1, "hi": 3}}`, `{"signed": {"negative": 0.5}}`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SeedStates {
    /// State 1 (+1 on JACA fields).
    #[default]
    One,
    /// Uniformly random nonzero states of the field, per cell.
    Uniform,
    /// Uniformly random states `lo..=hi` (at least 1).
    Range { lo: u32, hi: u32 },
    /// JACA only: -1 with probability `negative`, else +1.
    Signed { negative: f64 },
    /// One random nonzero state (or one JACA sign) per region of the shape: each blob, cluster,
    /// dot, ring, stripe, checker square, noise patch, or Voronoi cell. Shapes with no regions
    /// are one region.
    ByRegion,
}

impl SeedStates {
    pub fn is_one(&self) -> bool {
        *self == SeedStates::One
    }
}

fn is_one(v: &f64) -> bool {
    *v == 1.0
}
fn d_one() -> f64 {
    1.0
}
fn d_radius() -> f64 {
    0.25
}
fn d_blobs() -> u32 {
    3
}
fn d_min_radius() -> f64 {
    0.08
}
fn d_max_radius() -> f64 {
    0.16
}
fn d_min_gap() -> f64 {
    0.03
}
fn d_clusters() -> u32 {
    40
}
fn d_radius_tiles() -> f64 {
    2.0
}
fn d_spacing() -> f64 {
    12.0
}
fn d_jitter() -> f64 {
    0.5
}
fn d_width() -> f64 {
    0.05
}
fn d_rings() -> u32 {
    3
}
fn d_period() -> f64 {
    0.125
}
fn d_duty() -> f64 {
    0.5
}
fn d_scale() -> f64 {
    0.1
}
fn d_octaves() -> u32 {
    3
}
fn d_sites() -> u32 {
    24
}
fn d_order() -> u32 {
    6
}

/// A shape as its object or just its name (`"all"`, `"blobs"`: the defaults).
pub fn shape_or_name<'de, D: serde::Deserializer<'de>>(d: D) -> Result<SeedShape, D::Error> {
    let v = serde_json::Value::deserialize(d)?;
    let v = match v {
        serde_json::Value::String(s) => serde_json::json!({ "kind": s }),
        v => v,
    };
    serde_json::from_value(v).map_err(serde::de::Error::custom)
}

impl SeedShape {
    /// Check the parameters.
    pub fn validate(&self) -> Result<(), String> {
        let frac = |x: f64, what: &str| {
            if (0.0..=1.0).contains(&x) {
                Ok(())
            } else {
                Err(format!("seed: {what} must be between 0 and 1 (got {x})"))
            }
        };
        let positive = |x: f64, what: &str, max: f64| {
            if x > 0.0 && x <= max {
                Ok(())
            } else {
                Err(format!(
                    "seed: {what} must be above 0 and at most {max} (got {x})"
                ))
            }
        };
        let finite = |x: f64, what: &str| {
            if x.is_finite() {
                Ok(())
            } else {
                Err(format!("seed: {what} must be a number (got {x})"))
            }
        };
        let count = |x: u32, what: &str, max: u32| {
            if (1..=max).contains(&x) {
                Ok(())
            } else {
                Err(format!("seed: {what} must be 1 to {max} (got {x})"))
            }
        };
        match *self {
            SeedShape::All => Ok(()),
            SeedShape::Disc { radius } => positive(radius, "radius", 2.0),
            SeedShape::Blobs {
                count: c,
                min_radius,
                max_radius,
                wobble,
                min_gap,
            } => {
                count(c, "blob count", 10_000)?;
                positive(min_radius, "min_radius", 2.0)?;
                positive(max_radius, "max_radius", 2.0)?;
                if max_radius < min_radius {
                    return Err(format!(
                        "seed: blob radii need min_radius <= max_radius (got {min_radius}..{max_radius})"
                    ));
                }
                if !(0.0..=2.0).contains(&wobble) {
                    return Err(format!("seed: wobble must be 0 to 2 (got {wobble})"));
                }
                if !(0.0..=2.0).contains(&min_gap) {
                    return Err(format!("seed: min_gap must be 0 to 2 (got {min_gap})"));
                }
                Ok(())
            }
            SeedShape::Clusters {
                count: c,
                radius_tiles,
            } => {
                count(c, "cluster count", 1_000_000)?;
                positive(radius_tiles, "radius_tiles", 10_000.0)
            }
            SeedShape::Dots {
                spacing_tiles,
                jitter,
            } => {
                if !(1.0..=100_000.0).contains(&spacing_tiles) {
                    return Err(format!(
                        "seed: spacing_tiles must be 1 to 100000 (got {spacing_tiles})"
                    ));
                }
                frac(jitter, "jitter")
            }
            SeedShape::Ring { radius, width } => {
                positive(radius, "radius", 2.0)?;
                positive(width, "width", 2.0)
            }
            SeedShape::Rings { count: c, width } => {
                count(c, "ring count", 1000)?;
                positive(width, "width", 2.0)
            }
            SeedShape::Stripes {
                period,
                duty,
                angle,
            } => {
                positive(period, "period", 4.0)?;
                frac(duty, "duty")?;
                finite(angle, "angle")
            }
            SeedShape::Checker { size } => positive(size, "size", 4.0),
            SeedShape::Gradient { angle, low, high } => {
                finite(angle, "angle")?;
                frac(low, "low")?;
                frac(high, "high")
            }
            SeedShape::RadialGradient { centre, edge } => {
                frac(centre, "centre")?;
                frac(edge, "edge")
            }
            SeedShape::Noise {
                scale,
                threshold,
                octaves,
            } => {
                positive(scale, "scale", 16.0)?;
                frac(threshold, "threshold")?;
                count(octaves, "octaves", 12)
            }
            SeedShape::Voronoi { sites, keep } => {
                count(sites, "sites", 1_000_000)?;
                frac(keep, "keep")
            }
            SeedShape::Symmetric { order, radius, .. } => {
                count(order, "order", 64)?;
                positive(radius, "radius", 2.0)
            }
        }
    }

    /// A short label, such as `3 blobs r0.1` or `Sym order 6, disc r0.3`.
    pub fn describe(&self) -> String {
        match *self {
            SeedShape::All => "all".into(),
            SeedShape::Disc { radius } => format!("disc r{}", num(radius)),
            SeedShape::Blobs {
                count,
                min_radius,
                max_radius,
                ..
            } => {
                let r = if min_radius == max_radius {
                    num(min_radius)
                } else {
                    format!("{}-{}", num(min_radius), num(max_radius))
                };
                format!("{count} blob{} r{r}", plural(count))
            }
            SeedShape::Clusters {
                count,
                radius_tiles,
            } => format!("{count} cluster{} r{}", plural(count), num(radius_tiles)),
            SeedShape::Dots { spacing_tiles, .. } => {
                format!("dots every {} tiles", num(spacing_tiles))
            }
            SeedShape::Ring { radius, width } => {
                format!("ring r{} w{}", num(radius), num(width))
            }
            SeedShape::Rings { count, width } => {
                format!("{count} ring{} w{}", plural(count), num(width))
            }
            SeedShape::Stripes {
                period,
                duty,
                angle,
            } => format!("stripes p{} {}{}", num(period), pct(duty), deg(angle)),
            SeedShape::Checker { size } => format!("checker {}", num(size)),
            SeedShape::Gradient { angle, low, high } => {
                format!("gradient {}-{}{}", pct(low), pct(high), deg(angle))
            }
            SeedShape::RadialGradient { centre, edge } => {
                format!("radial {}-{}", pct(centre), pct(edge))
            }
            SeedShape::Noise {
                scale, threshold, ..
            } => format!("noise s{} t{}", num(scale), num(threshold)),
            SeedShape::Voronoi { sites, keep } => {
                format!("voronoi {sites} keep {}", pct(keep))
            }
            SeedShape::Symmetric {
                order,
                mirror,
                radius,
            } => format!(
                "Sym order {order}{}, disc r{}",
                if mirror { " mirror" } else { "" },
                num(radius)
            ),
        }
    }
}

impl SeedFill {
    /// A fill of density `density` in state 1.
    pub fn density(density: f64) -> SeedFill {
        SeedFill {
            density,
            states: SeedStates::One,
        }
    }
    /// The same fill with other states.
    pub fn states(mut self, states: SeedStates) -> SeedFill {
        self.states = states;
        self
    }
    /// Check the parameters.
    pub fn validate(&self) -> Result<(), String> {
        if !(0.0..=1.0).contains(&self.density) {
            return Err(format!(
                "seed: density must be between 0 and 1 (got {})",
                self.density
            ));
        }
        match self.states {
            SeedStates::Range { lo, hi } if lo == 0 || hi < lo => Err(format!(
                "seed: a state range needs 1 <= lo <= hi (got {lo}..{hi})"
            )),
            SeedStates::Signed { negative } if !(0.0..=1.0).contains(&negative) => Err(format!(
                "seed: negative must be between 0 and 1 (got {negative})"
            )),
            _ => Ok(()),
        }
    }
    /// A short label, such as `40% density, by region`; empty for a solid fill of state 1.
    pub fn describe(&self) -> String {
        let mut parts: Vec<String> = Vec::new();
        if self.density != 1.0 {
            parts.push(format!("{} density", pct(self.density)));
        }
        match self.states {
            SeedStates::One => {}
            SeedStates::Uniform => parts.push("random states".into()),
            SeedStates::Range { lo, hi } => parts.push(format!("states {lo}-{hi}")),
            SeedStates::Signed { negative } => parts.push(format!("signed {} neg", pct(negative))),
            SeedStates::ByRegion => parts.push("by region".into()),
        }
        parts.join(", ")
    }
}

fn plural(n: u32) -> &'static str {
    if n == 1 { "" } else { "s" }
}

/// A number with at most three significant decimals, without trailing zeros.
pub(crate) fn num(x: f64) -> String {
    let s = format!("{x:.3}");
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

/// A share as a percentage (`40%`, `0.5%`).
pub(crate) fn pct(x: f64) -> String {
    let p = x * 100.0;
    if p != 0.0 && p.abs() < 1.0 {
        format!("{}%", num(p))
    } else {
        format!("{p:.0}%")
    }
}

fn deg(angle: f64) -> String {
    if angle == 0.0 {
        String::new()
    } else {
        format!(" {}deg", num(angle))
    }
}

/// Where a shape landed on a tiling, for one set of RNG draws.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Placement {
    /// Per cell: the region it is in, or [`NO_REGION`] outside the shape.
    pub region: Vec<u32>,
    /// Per cell: multiplies the density (the gradients). Empty means 1 everywhere.
    pub weight: Vec<f32>,
    /// Per cell: the soft outline factor for continuous and height fields. Empty means 1.
    pub edge: Vec<f32>,
    /// Per cell: the cell whose random decisions it shares (symmetric seeds). Empty means itself.
    pub rep: Vec<u32>,
}

impl Placement {
    fn empty(n: usize) -> Placement {
        Placement {
            region: vec![NO_REGION; n],
            ..Default::default()
        }
    }
    fn at(v: &[f32], c: usize) -> f64 {
        v.get(c).map_or(1.0, |&w| w as f64)
    }
}

/// Everything a shape needs to know about the tiling.
struct Frame {
    b: [f64; 4],
    centre: P,
    side: f64,
    tile: f64,
}

impl Frame {
    fn of(topo: &Topology) -> Frame {
        let b = topo.bounds;
        let tile = cell_size(topo);
        Frame {
            b,
            centre: [(b[0] + b[2]) / 2.0, (b[1] + b[3]) / 2.0],
            side: (b[2] - b[0]).min(b[3] - b[1]).max(tile),
            tile,
        }
    }
}

/// Smoothstep of `x` clamped to 0..1.
fn smooth(x: f64) -> f64 {
    let t = x.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// The share of a radius over which a soft edge falls to zero.
const SOFT: f64 = 0.35;

/// The soft factor at `ratio` of the way from a patch's centre to its outline.
fn radial_edge(ratio: f64) -> f32 {
    smooth((1.0 - ratio) / SOFT) as f32
}

/// The distance between two points, through wrap edges if the tiling has them.
fn torus_dist(a: P, b: P, periods: Option<(P, P)>) -> f64 {
    let Some((t1, t2)) = periods else {
        return dist(a, b);
    };
    let mut best = f64::MAX;
    for i in -1..=1 {
        for j in -1..=1 {
            let (i, j) = (i as f64, j as f64);
            let q = [b[0] + i * t1[0] + j * t2[0], b[1] + i * t1[1] + j * t2[1]];
            best = best.min(dist(a, q));
        }
    }
    best
}

/// Region, density weight, and edge for one cell, in parallel over all cells.
fn per_cell(topo: &Topology, f: impl Fn(usize, P) -> (u32, f32, f32) + Sync) -> Placement {
    let hits: Vec<(u32, f32, f32)> = (0..topo.len())
        .into_par_iter()
        .with_min_len(1 << 12)
        .map(|c| f(c, topo.centroid(c)))
        .collect();
    let mut p = Placement {
        region: Vec::with_capacity(hits.len()),
        weight: Vec::with_capacity(hits.len()),
        edge: Vec::with_capacity(hits.len()),
        rep: Vec::new(),
    };
    for (r, w, e) in hits {
        p.region.push(r);
        p.weight.push(w);
        p.edge.push(e);
    }
    p
}

const OUT: (u32, f32, f32) = (NO_REGION, 1.0, 1.0);

/// A direction for an angle in degrees.
fn direction(angle: f64) -> P {
    let (s, c) = sin_cos(angle.rem_euclid(360.0).to_radians());
    [c, s]
}

/// Lay a shape out on a tiling. Draws one number from `rng`.
pub fn place(topo: &Topology, shape: &SeedShape, rng: &mut Rng) -> Result<Placement, String> {
    shape.validate()?;
    let seed = rng.next_u64();
    let n = topo.len();
    if n == 0 {
        return Ok(Placement::default());
    }
    let fr = Frame::of(topo);
    let (b, centre, side, tile) = (fr.b, fr.centre, fr.side, fr.tile);
    let periods = crate::render::torus_periods(topo);
    // Layout draws (positions, sizes) come from this stream, in order.
    let mut r = Rng::new(mix(seed, 1));
    Ok(match *shape {
        SeedShape::All => per_cell(topo, |_, _| (0, 1.0, 1.0)),
        SeedShape::Disc { radius } => {
            let rr = radius * side;
            per_cell(topo, |_, p| {
                let d = dist(p, centre);
                if d <= rr {
                    (0, 1.0, radial_edge(d / rr))
                } else {
                    OUT
                }
            })
        }
        SeedShape::Blobs {
            count,
            min_radius,
            max_radius,
            wobble,
            min_gap,
        } => {
            let gap = min_gap * side;
            let mut blobs: Vec<(P, f64, Outline, f64)> = Vec::new();
            for _ in 0..count {
                let mut placed = None;
                for _ in 0..400 {
                    let at = topo.centroid(r.below(n as u64) as usize);
                    let rr = side * (min_radius + (max_radius - min_radius) * r.next_f64());
                    let outline = Outline::draw_wobbled(&mut r, wobble);
                    let reach = rr * outline.max_factor();
                    let clear = gap == 0.0
                        || blobs
                            .iter()
                            .all(|o| torus_dist(at, o.0, periods) >= reach + o.3 + gap);
                    if clear {
                        placed = Some((at, rr, outline, reach));
                        break;
                    }
                }
                match placed {
                    Some(blob) => blobs.push(blob),
                    None => break,
                }
            }
            let mut out = Placement::empty(n);
            out.edge = vec![1.0; n];
            for (k, (at, rr, outline, reach)) in blobs.iter().enumerate() {
                let bx = [at[0] - reach, at[1] - reach, at[0] + reach, at[1] + reach];
                topo.for_cells_in_box(bx, |c, p| {
                    let ratio = outline.ratio([p[0] - at[0], p[1] - at[1]], *rr);
                    if ratio <= 1.0 {
                        out.region[c] = k as u32;
                        out.edge[c] = radial_edge(ratio);
                    }
                });
            }
            out
        }
        SeedShape::Clusters {
            count,
            radius_tiles,
        } => {
            let rr = radius_tiles * tile;
            let mut out = Placement::empty(n);
            out.edge = vec![1.0; n];
            for k in 0..count {
                let at = topo.centroid(r.below(n as u64) as usize);
                let bx = [at[0] - rr, at[1] - rr, at[0] + rr, at[1] + rr];
                topo.for_cells_in_box(bx, |c, p| {
                    let d = dist(p, at);
                    if d <= rr {
                        out.region[c] = k;
                        out.edge[c] = radial_edge(d / rr);
                    }
                });
            }
            out
        }
        SeedShape::Dots {
            spacing_tiles,
            jitter,
        } => {
            let s = spacing_tiles * tile;
            let row = s * 0.866;
            let rows = ((b[3] - b[1]) / row).ceil() as i64 + 1;
            let cols = ((b[2] - b[0]) / s).ceil() as i64 + 1;
            let mut out = Placement::empty(n);
            let mut k = 0u32;
            let mut near = Vec::new();
            for j in 0..rows {
                let stagger = if j % 2 == 1 { 0.5 } else { 0.0 };
                for i in 0..cols {
                    let dx = (r.next_f64() - 0.5) * jitter * s;
                    let dy = (r.next_f64() - 0.5) * jitter * s;
                    let extra = 2 + r.below(3) as usize;
                    let shuffle = r.next_u64();
                    let p = [
                        b[0] + (i as f64 + stagger) * s + dx,
                        b[1] + j as f64 * row + dy,
                    ];
                    let Some(c) = topo.locate(p) else { continue };
                    near.clear();
                    topo.for_each_neighbor(c, Nb::Touch, |x| near.push(x as u32));
                    // A fixed-order sort by a hash: which neighbours join is random.
                    near.sort_by_key(|&x| mix(shuffle, x as u64));
                    out.region[c] = k;
                    for &x in near.iter().take(extra) {
                        out.region[x as usize] = k;
                    }
                    k += 1;
                }
            }
            out
        }
        SeedShape::Ring { radius, width } => {
            let (rr, half) = (radius * side, width * side / 2.0);
            per_cell(topo, |_, p| {
                let off = (dist(p, centre) - rr).abs();
                if off <= half {
                    (0, 1.0, smooth(1.0 - off / half) as f32)
                } else {
                    OUT
                }
            })
        }
        SeedShape::Rings { count, width } => {
            let step = side / 2.0 / (count + 1) as f64;
            let half = width * side / 2.0;
            per_cell(topo, |_, p| {
                let d = dist(p, centre);
                let i = ((d / step).round() - 1.0).clamp(0.0, count as f64 - 1.0);
                let off = (d - (i + 1.0) * step).abs();
                if off <= half {
                    (i as u32, 1.0, smooth(1.0 - off / half) as f32)
                } else {
                    OUT
                }
            })
        }
        SeedShape::Stripes {
            period,
            duty,
            angle,
        } => {
            let along = direction(angle);
            let normal = [-along[1], along[0]];
            let width = period * side;
            per_cell(topo, |_, p| {
                let u = ((p[0] - centre[0]) * normal[0] + (p[1] - centre[1]) * normal[1]) / width;
                let i = u.floor();
                let phase = u - i;
                if phase < duty {
                    let e = if duty >= 1.0 {
                        1.0
                    } else {
                        smooth(phase.min(duty - phase) / (0.25 * duty))
                    };
                    ((i as i64).rem_euclid(1 << 30) as u32, 1.0, e as f32)
                } else {
                    OUT
                }
            })
        }
        SeedShape::Checker { size } => {
            let s = size * side;
            per_cell(topo, |_, p| {
                let ix = ((p[0] - centre[0]) / s).floor() as i64;
                let iy = ((p[1] - centre[1]) / s).floor() as i64;
                if (ix + iy).rem_euclid(2) == 0 {
                    let region = ix.rem_euclid(1 << 15) << 16 | iy.rem_euclid(1 << 16);
                    (region as u32, 1.0, 1.0)
                } else {
                    OUT
                }
            })
        }
        SeedShape::Gradient { angle, low, high } => {
            let dir = direction(angle);
            let along = |x: f64, y: f64| x * dir[0] + y * dir[1];
            let corners = [
                along(b[0], b[1]),
                along(b[2], b[1]),
                along(b[0], b[3]),
                along(b[2], b[3]),
            ];
            let lo = corners.iter().copied().fold(f64::MAX, f64::min);
            let hi = corners.iter().copied().fold(f64::MIN, f64::max);
            let span = (hi - lo).max(1e-9);
            per_cell(topo, |_, p| {
                let t = ((along(p[0], p[1]) - lo) / span).clamp(0.0, 1.0);
                (0, (low + (high - low) * t) as f32, 1.0)
            })
        }
        SeedShape::RadialGradient {
            centre: inner,
            edge: outer,
        } => {
            let reach = side / 2.0;
            per_cell(topo, |_, p| {
                let t = (dist(p, centre) / reach).min(1.0);
                (0, (inner + (outer - inner) * t) as f32, 1.0)
            })
        }
        SeedShape::Noise {
            scale,
            threshold,
            octaves,
        } => {
            let size = scale * side;
            let nseed = mix(seed, 2);
            let mut p = per_cell(topo, |_, p| {
                let v = value_noise(nseed, (p[0] - b[0]) / size, (p[1] - b[1]) / size, octaves);
                if v > threshold {
                    (0, 1.0, smooth((v - threshold) / 0.08) as f32)
                } else {
                    OUT
                }
            });
            label_components(topo, &mut p.region);
            p.weight = Vec::new();
            p
        }
        SeedShape::Voronoi { sites, keep } => {
            let at: Vec<P> = (0..sites)
                .map(|_| topo.centroid(r.below(n as u64) as usize))
                .collect();
            let index = SiteIndex::new(&at, b, periods);
            let mut order: Vec<u32> = (0..sites).collect();
            for i in (1..order.len()).rev() {
                order.swap(i, r.below(i as u64 + 1) as usize);
            }
            let kept = (keep * sites as f64).round() as usize;
            let mut filled = vec![false; sites as usize];
            for &s in order.iter().take(kept) {
                filled[s as usize] = true;
            }
            let mut p = per_cell(topo, |_, p| {
                let s = index.nearest(p);
                if filled[s as usize] {
                    (s, 1.0, 1.0)
                } else {
                    OUT
                }
            });
            p.weight = Vec::new();
            p.edge = Vec::new();
            p
        }
        SeedShape::Symmetric {
            order,
            mirror,
            radius,
        } => {
            let (q, axis) = symmetry_frame(topo, order, mirror);
            let group = group_of(order, mirror, axis);
            let rr = radius * side;
            let slack = rr + 2.0 * tile;
            let hits: Vec<(u32, f32)> = (0..n)
                .into_par_iter()
                .with_min_len(1 << 10)
                .map(|c| {
                    let p = topo.centroid(c);
                    if dist(p, q) > slack {
                        return (NO_REGION, 1.0);
                    }
                    let mut rep = c;
                    for g in &group {
                        match image_cell(topo, g.apply(q, p), tile) {
                            Some(d) => rep = rep.min(d),
                            None => return (NO_REGION, 1.0),
                        }
                    }
                    // Judged at the representative, so a whole orbit is in or out together
                    // even when rounding differs from one member's radius to another's.
                    let d = dist(topo.centroid(rep), q);
                    if d <= rr {
                        (rep as u32, radial_edge(d / rr))
                    } else {
                        (NO_REGION, 1.0)
                    }
                })
                .collect();
            let mut p = Placement::empty(n);
            p.rep = vec![0; n];
            p.edge = vec![1.0; n];
            for (c, (rep, e)) in hits.into_iter().enumerate() {
                if rep != NO_REGION {
                    p.region[c] = 0;
                    p.rep[c] = rep;
                    p.edge[c] = e;
                }
            }
            p
        }
    })
}

/// Fractal value noise in 0..1: `octaves` layers on integer lattices, each twice as fine and half
/// as strong.
fn value_noise(seed: u64, x: f64, y: f64, octaves: u32) -> f64 {
    let (mut sum, mut total, mut amp, mut f) = (0.0, 0.0, 1.0, 1.0);
    for o in 0..octaves as u64 {
        let (px, py) = (x * f, y * f);
        let (ix, iy) = (px.floor(), py.floor());
        let (fx, fy) = (smooth(px - ix), smooth(py - iy));
        let (ix, iy) = (ix as i64, iy as i64);
        let at = |i: i64, j: i64| unit(mix(mix(seed ^ o, i as u64), j as u64));
        let top = at(ix, iy) * (1.0 - fx) + at(ix + 1, iy) * fx;
        let bottom = at(ix, iy + 1) * (1.0 - fx) + at(ix + 1, iy + 1) * fx;
        sum += amp * (top * (1.0 - fy) + bottom * fy);
        total += amp;
        amp *= 0.5;
        f *= 2.0;
    }
    sum / total
}

/// Replace the regions of cells inside the shape with their connected component (through shared
/// edges), numbered in order of each component's lowest cell.
fn label_components(topo: &Topology, region: &mut [u32]) {
    let n = topo.len();
    const TODO: u32 = NO_REGION - 1;
    for r in region.iter_mut() {
        if *r != NO_REGION {
            *r = TODO;
        }
    }
    let mut next = 0;
    let mut stack = Vec::new();
    for c in 0..n {
        if region[c] != TODO {
            continue;
        }
        region[c] = next;
        stack.push(c);
        while let Some(x) = stack.pop() {
            topo.for_each_neighbor(x, Nb::Edge, |y| {
                if region[y] == TODO {
                    region[y] = next;
                    stack.push(y);
                }
            });
        }
        next += 1;
    }
}

/// Nearest-site lookup by buckets. Wrap edges are handled by indexing each site's torus images,
/// so a query is a plain nearest-point search.
struct SiteIndex {
    origin: P,
    size: P,
    cols: i64,
    rows: i64,
    buckets: Vec<Vec<(P, u32)>>,
}

impl SiteIndex {
    fn new(sites: &[P], b: [f64; 4], periods: Option<(P, P)>) -> SiteIndex {
        let mut points: Vec<(P, u32)> = Vec::with_capacity(sites.len());
        let (mut x0, mut y0, mut x1, mut y1) = (b[0], b[1], b[2], b[3]);
        for (id, &s) in sites.iter().enumerate() {
            match periods {
                None => points.push((s, id as u32)),
                Some((t1, t2)) => {
                    for i in -1..=1 {
                        for j in -1..=1 {
                            let (i, j) = (i as f64, j as f64);
                            let p = [s[0] + i * t1[0] + j * t2[0], s[1] + i * t1[1] + j * t2[1]];
                            x0 = x0.min(p[0]);
                            y0 = y0.min(p[1]);
                            x1 = x1.max(p[0]);
                            y1 = y1.max(p[1]);
                            points.push((p, id as u32));
                        }
                    }
                }
            }
        }
        let (w, h) = ((x1 - x0).max(1e-9), (y1 - y0).max(1e-9));
        let k = ((points.len() as f64 * w / h).sqrt().ceil() as i64).max(1);
        let l = ((points.len() as f64 * h / w).sqrt().ceil() as i64).max(1);
        let size = [w / k as f64, h / l as f64];
        let mut buckets = vec![Vec::new(); (k * l) as usize];
        for &(p, id) in &points {
            let c = (((p[0] - x0) / size[0]) as i64).clamp(0, k - 1);
            let r = (((p[1] - y0) / size[1]) as i64).clamp(0, l - 1);
            buckets[(r * k + c) as usize].push((p, id));
        }
        SiteIndex {
            origin: [x0, y0],
            size,
            cols: k,
            rows: l,
            buckets,
        }
    }

    /// The nearest site; equally near sites go to the lowest id.
    fn nearest(&self, p: P) -> u32 {
        let cx = (((p[0] - self.origin[0]) / self.size[0]) as i64).clamp(0, self.cols - 1);
        let cy = (((p[1] - self.origin[1]) / self.size[1]) as i64).clamp(0, self.rows - 1);
        let step = self.size[0].min(self.size[1]);
        let mut best = (f64::MAX, u32::MAX);
        for k in 0..=self.cols.max(self.rows) {
            // Anything in ring `k` is at least (k - 1) bucket sizes away.
            if best.1 != u32::MAX && best.0 < (k - 1).max(0) as f64 * step {
                break;
            }
            for r in cy - k..=cy + k {
                if r < 0 || r >= self.rows {
                    continue;
                }
                let ring = (r - cy).abs() == k;
                let dc = if ring { 1 } else { (2 * k).max(1) };
                let mut c = cx - k;
                while c <= cx + k {
                    if c >= 0 && c < self.cols {
                        for &(s, id) in &self.buckets[(r * self.cols + c) as usize] {
                            let d = dist(s, p);
                            if (d, id) < best {
                                best = (d, id);
                            }
                        }
                    }
                    c += dc;
                }
            }
        }
        best.1
    }
}

/// One element of a symmetry group about a centre: a rotation, after a reflection if `reflect`.
#[derive(Clone, Copy, Debug)]
struct Element {
    cos: f64,
    sin: f64,
    /// (cos 2a, sin 2a) of the mirror axis at angle `a`, if this element reflects.
    reflect: Option<(f64, f64)>,
}

impl Element {
    fn apply(&self, q: P, p: P) -> P {
        let (mut x, mut y) = (p[0] - q[0], p[1] - q[1]);
        if let Some((c2, s2)) = self.reflect {
            (x, y) = (c2 * x + s2 * y, s2 * x - c2 * y);
        }
        [
            q[0] + self.cos * x - self.sin * y,
            q[1] + self.sin * x + self.cos * y,
        ]
    }
}

/// The non-identity elements of the group of `order` rotations (and their reflections across the
/// axis at `axis` radians, if `mirror`).
fn group_of(order: u32, mirror: bool, axis: f64) -> Vec<Element> {
    let mut out = Vec::new();
    let mirrored = sin_cos(2.0 * axis);
    for k in 0..order {
        let (sin, cos) = sin_cos(std::f64::consts::TAU * k as f64 / order as f64);
        if k > 0 {
            out.push(Element {
                cos,
                sin,
                reflect: None,
            });
        }
        if mirror {
            out.push(Element {
                cos,
                sin,
                reflect: Some((mirrored.1, mirrored.0)),
            });
        }
    }
    out
}

/// The cell whose centroid is at a world point, or else the cell containing it.
fn image_cell(topo: &Topology, p: P, tile: f64) -> Option<usize> {
    let d = topo.locate(p)?;
    let eps = 1e-6 * tile;
    if dist(topo.centroid(d), p) <= eps {
        return Some(d);
    }
    // A concave tile's vertex average can lie in a neighbour: find the exact match if there is
    // one, so such tilings are still exactly symmetric.
    let mut exact = None;
    topo.for_each_neighbor(d, Nb::Touch, |x| {
        if exact.is_none() && dist(topo.centroid(x), p) <= eps {
            exact = Some(x);
        }
    });
    Some(exact.unwrap_or(d))
}

/// How far `group` moves `sample` centroids from any cell's centroid, on average, in tiles.
fn mismatch(topo: &Topology, q: P, sample: &[P], group: &[Element], tile: f64) -> f64 {
    let mut total = 0.0;
    let mut count = 0.0;
    for &p in sample {
        for g in group {
            let img = g.apply(q, p);
            let off = match topo.locate(img) {
                Some(d) => (dist(topo.centroid(d), img) / tile).min(0.5),
                None => 0.5,
            };
            total += off;
            count += 1.0;
        }
    }
    if count == 0.0 { 0.0 } else { total / count }
}

/// Scores are rounded to this many tiles, so that exact symmetries tie regardless of float noise.
const SYM_TOLERANCE: f64 = 1e-5;

/// The centre (and mirror axis, in radians) of a symmetric seed: the candidate near the middle of
/// the tiling that best maps cell centroids to cell centroids. Candidates are the cell centroids,
/// polygon vertices, and edge midpoints around the middle, so a hexagon grid gets a hexagon's
/// centre, and a trihexagonal tiling a hexagon rather than a triangle for order 6. Equally good
/// candidates go to the one nearest the middle.
pub fn symmetry_frame(topo: &Topology, order: u32, mirror: bool) -> (P, f64) {
    let fr = Frame::of(topo);
    let (centre, tile) = (fr.centre, fr.tile);
    let reach = (4.0 * tile).min(fr.side / 2.0);
    let mut near: Vec<(f64, usize, P)> = Vec::new();
    topo.for_cells_in_box(
        [
            centre[0] - reach,
            centre[1] - reach,
            centre[0] + reach,
            centre[1] + reach,
        ],
        |c, p| near.push((dist(p, centre), c, p)),
    );
    near.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
    if near.is_empty() {
        return (centre, 0.0);
    }
    let sample: Vec<P> = near.iter().take(64).map(|x| x.2).collect();

    let mut candidates: Vec<P> = vec![centre];
    for &(_, c, p) in near.iter().take(24) {
        candidates.push(p);
        // The polygon is at the cell's own position; shift it to the image the box reported.
        let poly = topo.polygon(c);
        let at = crate::geom::centroid(&poly);
        let off = [p[0] - at[0], p[1] - at[1]];
        let m = poly.len();
        for i in 0..m {
            let (a, b) = (poly[i], poly[(i + 1) % m]);
            candidates.push([a[0] + off[0], a[1] + off[1]]);
            candidates.push([(a[0] + b[0]) / 2.0 + off[0], (a[1] + b[1]) / 2.0 + off[1]]);
        }
    }
    let key = |p: &P| {
        (
            (p[0] / tile / SYM_TOLERANCE).round() as i64,
            (p[1] / tile / SYM_TOLERANCE).round() as i64,
        )
    };
    candidates.sort_by_key(key);
    candidates.dedup_by_key(|p| key(p));
    candidates.sort_by(|a, b| {
        dist(*a, centre)
            .total_cmp(&dist(*b, centre))
            .then(a[0].total_cmp(&b[0]))
            .then(a[1].total_cmp(&b[1]))
    });
    candidates.truncate(40);

    let rotations = group_of(order, false, 0.0);
    let rounded = |x: f64| (x / SYM_TOLERANCE).round() as i64;
    let mut scored: Vec<(i64, usize)> = candidates
        .iter()
        .enumerate()
        .map(|(i, &q)| (rounded(mismatch(topo, q, &sample, &rotations, tile)), i))
        .collect();
    scored.sort();
    if !mirror {
        return (candidates[scored[0].1], 0.0);
    }
    // Mirror axes through the best few centres, at every 15 degrees (the mirrors of every lattice).
    let best_score = scored[0].0;
    let mut best: Option<(i64, usize, usize)> = None;
    for &(score, i) in scored
        .iter()
        .take_while(|s| s.0 == best_score)
        .take(8)
        .chain(scored.iter().take(1))
    {
        let q = candidates[i];
        for k in 0..12 {
            let axis = k as f64 * std::f64::consts::PI / 12.0;
            let (s2, c2) = sin_cos(2.0 * axis);
            let reflect = [Element {
                cos: 1.0,
                sin: 0.0,
                reflect: Some((c2, s2)),
            }];
            let total = score + rounded(mismatch(topo, q, &sample, &reflect, tile));
            if best.is_none_or(|b| (total, i, k) < b) {
                best = Some((total, i, k));
            }
        }
    }
    let (_, i, k) = best.expect("at least one candidate");
    (candidates[i], k as f64 * std::f64::consts::PI / 12.0)
}

/// Apply a pattern seed to a field: lay the shape out, then fill it. Draws two numbers from `rng`.
pub fn apply(
    topo: &Topology,
    rule: &Rule,
    nb: Nb,
    field: &mut Field,
    shape: &SeedShape,
    fill: &SeedFill,
    rng: &mut Rng,
) -> Result<(), String> {
    fill.validate()?;
    match rule {
        Rule::Turmite(_) => {
            return Err("pattern seeds set cells; turmites start from ants (ants:3)".into());
        }
        Rule::Wolfram(_) | Rule::HexWolfram { .. } | Rule::HexTri { .. } => {
            return Err(
                "pattern seeds fill the whole grid; elementary rules grow from a top row (top_center, top_random)"
                    .into(),
            );
        }
        _ => {}
    }
    if matches!(fill.states, SeedStates::Signed { .. }) && !matches!(rule, Rule::Jaca(_)) {
        return Err("signed fills need a JACA CA".into());
    }
    let placed = place(topo, shape, rng)?;
    let seed = rng.next_u64();
    let (coin, state, region_seed, amp, round) = (
        mix(seed, 11),
        mix(seed, 12),
        mix(seed, 13),
        mix(seed, 14),
        mix(seed, 15),
    );
    for c in 0..placed.region.len() {
        let region = placed.region[c];
        if region == NO_REGION {
            continue;
        }
        let key = placed.rep.get(c).map_or(c as u64, |&r| r as u64);
        let weight = Placement::at(&placed.weight, c);
        let edge = Placement::at(&placed.edge, c);
        let d = fill.density * weight;
        match field.kind {
            FieldKind::Binary | FieldKind::Multi(_) => {
                if unit(mix(coin, key)) >= d {
                    continue;
                }
                let top = field.max_state() as u64;
                let pick = |h: u64| 1 + h % top.max(1);
                let s = match fill.states {
                    SeedStates::One => 1,
                    SeedStates::Uniform => pick(mix(state, key)),
                    SeedStates::Range { lo, hi } => {
                        let (lo, hi) = (lo as u64, hi as u64);
                        (lo + mix(state, key) % (hi - lo + 1)).min(top.max(1))
                    }
                    SeedStates::Signed { negative } => {
                        if unit(mix(state, key)) < negative {
                            jaca::NEG as u64
                        } else {
                            jaca::POS as u64
                        }
                    }
                    SeedStates::ByRegion => pick(mix(region_seed, region as u64)),
                };
                field.set(c, s as u32);
            }
            FieldKind::Continuous => {
                let a = amplitude(fill.states, amp, region_seed, key, region);
                let v = (d * edge * a) as f32;
                if v > 0.0 {
                    field.set_value(c, v);
                }
            }
            FieldKind::Height => {
                if let SeedStates::Range { lo, hi } = fill.states {
                    if unit(mix(coin, key)) < d {
                        let (lo, hi) = (lo as u64, hi as u64);
                        field.set(c, (lo + mix(state, key) % (hi - lo + 1)) as u32);
                    }
                    continue;
                }
                let degree = topo.degree(c, nb).max(1) as f64;
                let a = amplitude(fill.states, amp, region_seed, key, region);
                let grains = 2.0 * d * edge * a * degree;
                let whole = grains.floor();
                let extra = (unit(mix(round, key)) < grains - whole) as u32;
                let total = whole as u32 + extra;
                if total > 0 {
                    field.set(c, total);
                }
            }
        }
    }
    Ok(())
}

/// The amplitude of a continuous or height cell (see the module docs).
fn amplitude(states: SeedStates, amp: u64, region_seed: u64, key: u64, region: u32) -> f64 {
    let u = unit(mix(amp, key));
    match states {
        SeedStates::One => 0.75 + 0.25 * u,
        SeedStates::Uniform => 1.0 - u,
        SeedStates::Range { lo, hi } => (lo as f64 + (hi as f64 - lo as f64) * u) / 255.0,
        // Refused for these fields before filling.
        SeedStates::Signed { .. } => 1.0,
        SeedStates::ByRegion => 0.25 + 0.75 * (1.0 - unit(mix(region_seed, region as u64))),
    }
}
