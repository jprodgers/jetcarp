//! Patterns on aperiodic patches (Spectre, hat, Penrose), stored with their surrounding tiles.
//!
//! An aperiodic patch has no lattice to translate along, so a pattern only means something
//! together with the tiles around it. A [`Site`] keeps every tile within `radius` neighbour
//! steps of any cell the pattern uses in any phase, each as its shape and corner positions
//! relative to an anchor tile (one of the pattern's cells). The labelling is canonical: tiles
//! are sorted by (distance, shape, rounded corners), and the anchor and starting phase are the
//! choice giving the smallest description, so the same oscillator found at two sites with the
//! same surroundings gets the same id (up to the anchor tile's own symmetry, which is not
//! factored out).
//!
//! Pasting maps the anchor onto a target tile of the same shape by the rigid motion between their
//! placements, then requires every site tile to land exactly on a tile of the patch with the same
//! shape and corners (and, inside the radius, not on the patch's open boundary). Any difference
//! refuses the paste.
//!
//! **Pattern graphs (dedup across sites).** Records made for outer-totalistic rules store a
//! [`SiteGraph`] instead (see [`super::site_graph`]): the pattern's tiles as a labelled graph,
//! canonical up to the patch's rotations and reflections, and pasted wherever the same graph
//! embeds and the tiles around it stay quiet under the rule.

use super::{PatternRecord, now_utc, verify::Verification};
use crate::geom::{Affine, P, centroid, point_in_poly, triangulate};
use crate::objects::{LCell, Phase};
use crate::topology::{Nb, TilingSpec, Topology};
use crate::world::{Command, World};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, Mutex, OnceLock};

pub use super::site_graph::{GraphCell, SiteGraph, build_site_for, other_sites};

/// Neighbour steps of surrounding tiles kept around a pattern.
pub const SITE_RADIUS: u32 = 2;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SiteTile {
    pub shape: u16,
    /// Neighbour steps from the nearest pattern cell (0 = a cell the pattern uses).
    pub dist: u32,
    /// A point inside the tile, in the anchor's shape coordinates.
    pub point: P,
    /// Corners in the anchor's shape coordinates, sorted, rounded to 1e-4.
    pub corners: Vec<P>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Site {
    /// The patch the pattern was found on (family and level).
    pub patch: TilingSpec,
    /// The anchor tile on that patch.
    pub anchor: u32,
    pub anchor_shape: u16,
    /// A point inside the anchor tile, in world coordinates (to find the site on other patches).
    pub anchor_point: P,
    pub radius: u32,
    /// The surrounding tiles in canonical order; pattern cells refer to them by index.
    pub tiles: Vec<SiteTile>,
    /// The pattern graph (newer records): `tiles[k]` is graph cell `k` for the pattern's
    /// tiles (the tiles around follow), and matching uses the graph instead of the geometry.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub graph: Option<Box<SiteGraph>>,
    /// The neighbourhood the graph was built with.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub nb: Option<Nb>,
}

/// A point rounded to 1e-6 (so it survives a JSON round trip exactly).
pub(super) fn r6(p: P) -> P {
    p.map(|x| {
        let v = (x * 1e6).round() / 1e6;
        if v == 0.0 { 0.0 } else { v }
    })
}

fn r4(x: f64) -> f64 {
    let v = (x * 1e4).round() / 1e4;
    if v == 0.0 { 0.0 } else { v }
}

/// A point inside a polygon: its centroid if inside, else the centroid of its largest triangle.
pub fn interior_point(poly: &[P]) -> P {
    let c = centroid(poly);
    if point_in_poly(c, poly) {
        return c;
    }
    let mut best = (0.0, c);
    for t in triangulate(poly) {
        let (a, b, d) = (
            poly[t[0] as usize],
            poly[t[1] as usize],
            poly[t[2] as usize],
        );
        let area = ((b[0] - a[0]) * (d[1] - a[1]) - (d[0] - a[0]) * (b[1] - a[1])).abs();
        if area > best.0 {
            best = (
                area,
                [(a[0] + b[0] + d[0]) / 3.0, (a[1] + b[1] + d[1]) / 3.0],
            );
        }
    }
    best.1
}

pub(super) fn tile_in_frame(topo: &Topology, x: usize, inv: &Affine) -> (Vec<P>, P) {
    let poly = topo.polygon(x);
    let mut corners: Vec<P> = poly
        .iter()
        .map(|&p| {
            let q = inv.apply(p);
            [r4(q[0]), r4(q[1])]
        })
        .collect();
    corners.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let ip = inv.apply(interior_point(&poly));
    (corners, [r4(ip[0]), r4(ip[1])])
}

/// One tile in a geometric site's canonical text: distance, shape, then its corners.
pub(crate) fn describe(t: &SiteTile) -> String {
    let mut s = format!("{}:{}:", t.dist, t.shape);
    for c in &t.corners {
        s.push_str(&format!("{:.4},{:.4};", c[0], c[1]));
    }
    s
}

/// The canonical form of a geometric site: `site r<radius>` and 32 hex digits of SHA-256 of its
/// text (every tile described in order, then each phase's ` |` and ` <tile>:<state>` pairs).
pub(crate) fn site_canonical(radius: u32, text: &str) -> String {
    format!(
        "site r{radius} {}",
        &crate::sha256::sha256_hex(text.as_bytes())[..32]
    )
}

/// Build the site of an object on an explicit patch. `phases` is one period with cell ids in
/// `i`. Returns the site, the phases as site-tile indices (canonical start), and the canonical
/// text.
pub fn build_site(
    topo: &Topology,
    nb: Nb,
    phases: &[Phase],
) -> Result<(Site, Vec<Phase>, String), String> {
    if topo.lattice().is_some() {
        return Err("sites are for aperiodic patches".into());
    }
    // Tiles within SITE_RADIUS steps of any cell used in any phase.
    let mut dist: BTreeMap<usize, u32> = BTreeMap::new();
    let mut frontier: Vec<usize> = Vec::new();
    for ph in phases {
        for c in ph {
            if dist.insert(c.i as usize, 0).is_none() {
                frontier.push(c.i as usize);
            }
        }
    }
    for d in 1..=SITE_RADIUS {
        let mut next = Vec::new();
        for &x in &frontier {
            topo.for_each_neighbor(x, nb, |y| {
                if let std::collections::btree_map::Entry::Vacant(e) = dist.entry(y) {
                    e.insert(d);
                    next.push(y);
                }
            });
        }
        frontier = next;
    }
    for (&x, &d) in &dist {
        if d < SITE_RADIUS && topo.is_open(x) {
            return Err("the pattern is too close to the patch's open boundary".into());
        }
    }
    let p = phases.len();
    // (description, tiles with their cells, anchor, starting phase)
    type Best = (String, Vec<(usize, SiteTile)>, usize, usize);
    let mut best: Option<Best> = None;
    for (t, ph) in phases.iter().enumerate() {
        for a in ph {
            let anchor = a.i as usize;
            let inv = topo.transform(anchor).inv();
            let mut tiles: Vec<(usize, SiteTile)> = dist
                .iter()
                .map(|(&x, &d)| {
                    let (corners, point) = tile_in_frame(topo, x, &inv);
                    (
                        x,
                        SiteTile {
                            shape: topo.shape_of(x) as u16,
                            dist: d,
                            point,
                            corners,
                        },
                    )
                })
                .collect();
            tiles.sort_by(|a, b| {
                (a.1.dist, a.1.shape)
                    .cmp(&(b.1.dist, b.1.shape))
                    .then(a.1.corners.partial_cmp(&b.1.corners).unwrap())
            });
            let index: HashMap<usize, usize> = tiles
                .iter()
                .enumerate()
                .map(|(k, (x, _))| (*x, k))
                .collect();
            let mut text: String = tiles
                .iter()
                .map(|(_, s)| describe(s))
                .collect::<Vec<_>>()
                .join(" ");
            for u in 0..p {
                let mut v: Vec<(usize, u8)> = phases[(t + u) % p]
                    .iter()
                    .map(|c| (index[&(c.i as usize)], c.s))
                    .collect();
                v.sort_unstable();
                text.push_str(" |");
                for (k, s) in v {
                    text.push_str(&format!(" {k}:{s}"));
                }
            }
            if best.as_ref().is_none_or(|b| text < b.0) {
                best = Some((text, tiles, anchor, t));
            }
        }
    }
    let (text, tiles, anchor, t0) = best.ok_or("empty pattern")?;
    let index: HashMap<usize, usize> = tiles
        .iter()
        .enumerate()
        .map(|(k, (x, _))| (*x, k))
        .collect();
    let out: Vec<Phase> = (0..p)
        .map(|u| {
            let mut v: Phase = phases[(t0 + u) % p]
                .iter()
                .map(|c| LCell {
                    i: index[&(c.i as usize)] as i32,
                    j: 0,
                    k: 0,
                    s: c.s,
                })
                .collect();
            v.sort_unstable();
            v
        })
        .collect();
    let site = Site {
        patch: topo.spec.clone(),
        anchor: anchor as u32,
        anchor_shape: topo.shape_of(anchor) as u16,
        anchor_point: r6(interior_point(&topo.polygon(anchor))),
        radius: SITE_RADIUS,
        tiles: tiles.into_iter().map(|(_, s)| s).collect(),
        graph: None,
        nb: None,
    };
    let canonical = site_canonical(SITE_RADIUS, &text);
    Ok((site, out, canonical))
}

/// Map the site onto `topo` with its anchor on tile `target`. Returns the cell of every site
/// tile, or why the surroundings do not match.
pub fn match_site(topo: &Topology, site: &Site, target: usize) -> Result<Vec<u32>, String> {
    if let Some(g) = &site.graph {
        return super::site_graph::match_graph(topo, site, g, target);
    }
    if topo.lattice().is_some() {
        return Err(
            "this pattern was found on an aperiodic patch; it can only be pasted on one".into(),
        );
    }
    if target >= topo.len() {
        return Err(format!(
            "no cell {target} (the patch has {} cells)",
            topo.len()
        ));
    }
    if topo.shape_of(target) as u16 != site.anchor_shape {
        return Err(format!(
            "cell {target} has tile shape {}, the pattern's anchor tile has shape {}",
            topo.shape_of(target),
            site.anchor_shape
        ));
    }
    let m = topo.transform(target);
    let mut out = Vec::with_capacity(site.tiles.len());
    for (k, t) in site.tiles.iter().enumerate() {
        let what = |why: &str| {
            format!(
                "the tiles around cell {target} do not match the pattern's surroundings: site tile {k} (distance {}) {why}. Paste it where the local tile configuration matches, such as its original site (cell {} on {} level {:?})",
                t.dist, site.anchor, site.patch.family, site.patch.size
            )
        };
        let p = m.apply(t.point);
        let Some(x) = topo.locate(p) else {
            return Err(what("falls outside the patch"));
        };
        if topo.shape_of(x) as u16 != t.shape {
            return Err(what("has a different shape there"));
        }
        let poly = topo.polygon(x);
        let ok = poly.len() == t.corners.len()
            && t.corners.iter().all(|c| {
                let q = m.apply(*c);
                poly.iter()
                    .any(|v| (v[0] - q[0]).abs() < 2e-3 && (v[1] - q[1]).abs() < 2e-3)
            });
        if !ok {
            return Err(what("is placed differently there"));
        }
        if t.dist < site.radius && topo.is_open(x) {
            return Err(what("lies on the patch's open boundary"));
        }
        out.push(x as u32);
    }
    Ok(out)
}

/// The site's anchor tile on `topo`: the original id on the original patch, else the tile at the
/// anchor's original position.
pub fn original_anchor(topo: &Topology, site: &Site) -> Result<usize, String> {
    if super::tiling_key(&topo.spec) == super::tiling_key(&site.patch)
        && topo.spec.size == site.patch.size
    {
        return Ok(site.anchor as usize);
    }
    topo.locate(site.anchor_point)
        .ok_or_else(|| "the pattern's original site is outside this patch".to_string())
}

/// Cached patch topologies for verification.
pub fn patch_topology(spec: &TilingSpec) -> Result<Arc<Topology>, String> {
    static CACHE: OnceLock<Mutex<HashMap<String, Arc<Topology>>>> = OnceLock::new();
    let key = serde_json::to_string(spec).unwrap_or_default();
    let cache = CACHE.get_or_init(Default::default);
    if let Some(t) = cache.lock().unwrap().get(&key) {
        return Ok(t.clone());
    }
    let t = Arc::new(Topology::build(spec)?);
    let mut c = cache.lock().unwrap();
    // At most 8 patches and 24 million tiles (a Spectre level 8 is 17 million).
    let tiles: usize = c.values().map(|t| t.len()).sum();
    if c.len() > 8 || tiles + t.len() > 24_000_000 {
        c.clear();
    }
    c.insert(key, t.clone());
    Ok(t)
}

/// Extra matching sites a neighbourhood-graph pattern is verified at (besides its own).
pub const EXTRA_SITES: usize = 3;

/// Run the pattern at its original site and compare every generation with the stored phases;
/// patterns with a neighbourhood graph are also run at up to [`EXTRA_SITES`] other sites of the
/// patch where the graph matches.
pub fn verify_on_patch(
    rec: &PatternRecord,
    site: &Site,
    periods: u64,
) -> Result<Verification, String> {
    let topo = patch_topology(&site.patch)?;
    let map = match_site(&topo, site, site.anchor as usize)?;
    let p = rec.period.max(1);
    let gens = periods * p;
    run_at(rec, &topo, &map, gens).map_err(|e| format!("at its site: {e}"))?;
    let others = other_sites(&topo, site, EXTRA_SITES);
    for (x, m) in &others {
        run_at(rec, &topo, m, gens).map_err(|e| format!("at matching site (cell {x}): {e}"))?;
    }
    Ok(Verification {
        periods,
        generations: gens,
        field: format!(
            "patch {} {:?}, at its site (cell {}){}",
            site.patch.family,
            site.patch.size,
            site.anchor,
            if others.is_empty() {
                String::new()
            } else {
                format!(
                    " and {} other matching site(s) (cells {})",
                    others.len(),
                    others
                        .iter()
                        .map(|(x, _)| x.to_string())
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            }
        ),
        sites: 1 + others.len() as u32,
        jetcarp_version: crate::VERSION.into(),
        date: now_utc(),
    })
}

fn run_at(rec: &PatternRecord, topo: &Arc<Topology>, map: &[u32], gens: u64) -> Result<(), String> {
    let to_cells = |ph: &Phase| -> Result<Vec<(u32, u8)>, String> {
        let mut v: Vec<(u32, u8)> = ph
            .iter()
            .map(|c| {
                map.get(c.i as usize)
                    .map(|&x| (x, c.s))
                    .ok_or("cell outside the site".to_string())
            })
            .collect::<Result<_, _>>()?;
        v.sort_unstable();
        Ok(v)
    };
    let mut w: World = super::verify::rule_world(rec, topo.clone())?;
    let id = w.cas[0].id;
    let fi = w.cas[0].field;
    let start = to_cells(&rec.phase(0)?)?;
    w.apply(Command::SetCells {
        id,
        cells: start.iter().map(|&(x, s)| [x, s as u32]).collect(),
    })?;
    let p = rec.period.max(1);
    for t in 1..=gens {
        w.apply(Command::Step { gens: 1 })?;
        let live: Vec<(u32, u8)> = w.fields[fi]
            .state
            .iter()
            .enumerate()
            .filter(|(_, s)| **s != 0)
            .map(|(x, &s)| (x as u32, s))
            .collect();
        let want = to_cells(&rec.phase(t)?)?;
        if live != want {
            return Err(format!(
                "generation {t}: {} live cells, expected {} as stored for phase {}",
                live.len(),
                want.len(),
                t % p
            ));
        }
        if live.iter().any(|&(x, _)| topo.is_open(x as usize)) {
            return Err(format!(
                "generation {t}: the pattern reached the patch boundary"
            ));
        }
    }
    Ok(())
}
