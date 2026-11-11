//! Canonical forms of pattern records, so one pattern has one id however it was stored.
//!
//! A pattern found in another phase, orientation, or position, or a record whose phases were
//! written starting at another generation, is the same pattern. The id is a hash of the
//! record's *canonical* form ([`super::range::identity`]), computed here from the record's own
//! cells, never from how they happen to be stored (an importer that rotated a record's phases by
//! 0 to 4 generations used to give one ship 5 ids):
//!
//! - **Lattice tilings.** The phases are decoded and put in canonical form with
//!   [`super::canonical_phases`]: the minimum, over every stored phase and every symmetry of the
//!   tiling ([`super::symmetry`]: 8 on squares, 12 on hexagons, the tiling's point group with
//!   its tile-type permutation on the Archimedean and k-uniform tilings), of the phase moved to
//!   the origin; the period then starts at that phase, in that orientation, at the origin
//!   (phase rotation, orientation, translation). The displacement is carried into the same
//!   orientation. Rule tables are not isotropic, so only translations and phase rotation apply.
//!   The symmetries are every symmetry of the tiling ([`super::symmetry`] says why). On a unit
//!   cell that holds its tile pattern more than once (41 of the tilings: flat-top hexagons, the
//!   glide and fourfold families, 38 isohedral types), the minimum is over the whole group as
//!   isometries ([`super::group`]), since some of it cannot be written as cell maps, and a
//!   spaceship's period is first made its shortest over every translation of the tiling (see
//!   [`general_form`]). Rule tables on such cells: identity only (their slots need not match).
//! - **Aperiodic patches with a pattern graph** (outer-totalistic rules). The stored graph is
//!   labelled canonically again ([`super::site_graph`]: colour refinement and individualization,
//!   minimised over the starting phase and, on chiral tilings, a global mirror), which is
//!   invariant under the patch's rotations and reflections and under where on the patch the
//!   pattern sits; the phases are relabelled to match.
//! - **Aperiodic patches with a geometric site** (rule tables, disconnected or very symmetric
//!   patterns). The phases are rotated to the start with the smallest canonical text, with the
//!   stored anchor tile. *Gap:* the choice of anchor tile, which the site builder minimises over,
//!   is taken as stored, so a geometric-site record re-anchored by another tool would get another
//!   id; [`PatternRecord::canonicalize`] rebuilds such a site from its patch, which fixes it.
//!
//! Puffers and movers store only their starting configuration, so phase rotation does not apply
//! to them (their identity also includes the exact rule); orientation and translation do.
//!
//! [`PatternRecord::canonicalize`] rewrites a record into this form (and sets its pin and id),
//! for importers: on lattices from the record alone, on aperiodic patches by matching the site
//! on its patch and building it again.

use super::group::{Iso, TilingGroup, tiling_group};
use super::site::{Site, describe, match_site, original_anchor, patch_topology, site_canonical};
use super::symmetry::CellMap;
use super::{
    PatternClass, PatternRecord, canonical_phases_map, decode_cells, encode_cells, tiling_identity,
    tiling_key,
};
use crate::objects::{LCell, ObjClass, Phase};
use crate::rules::Rule;
use crate::topology::{Boundary, Nb, SizeSpec, TilingSpec, Topology};
use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

/// A record's canonical cells and canonical text.
#[derive(Clone, Debug, PartialEq)]
pub struct CanonicalForm {
    /// One period (the starting configuration for puffers and movers), phase 0 first.
    pub phases: Vec<Phase>,
    /// Lattice displacement per period in the canonical orientation (`[0, 0]` if none).
    pub displacement: [i32; 2],
    /// The canonical form text (`rows`, `k@i,j` lists, `graph ...`, `site r2 ...`).
    pub canonical: String,
    /// The period in the canonical orientation: the record's, except for a spaceship on a unit
    /// cell that holds its tile pattern more than once, whose period over the cell's lattice can
    /// depend on its direction (see [`general_form`]).
    pub period: u64,
}

/// The symmetry that took a record's stored orientation to its canonical one.
enum Chosen {
    Cell(CellMap),
    Geo(Iso, Arc<TilingGroup>),
}

/// Does orientation count for this rule (isotropic rules)? Rule tables are not isotropic.
pub fn isotropic(rule: &str) -> bool {
    Rule::parse(rule)
        .ok()
        .and_then(|(r, _)| crate::objects::supported(&r).ok())
        .unwrap_or(false)
}

/// The symmetries of a lattice tiling as cell maps, for a neighbourhood (identity only when not
/// `isotropic`), computed once per tiling in this process.
pub fn tiling_symmetries(
    tiling: &TilingSpec,
    nb: Nb,
    isotropic: bool,
) -> Result<Arc<Vec<CellMap>>, String> {
    static CACHE: OnceLock<Mutex<HashMap<String, Arc<Vec<CellMap>>>>> = OnceLock::new();
    let key = format!("{}|{nb:?}|{isotropic}", tiling_key(tiling));
    let cache = CACHE.get_or_init(Default::default);
    if let Some(s) = cache.lock().unwrap().get(&key) {
        return Ok(s.clone());
    }
    let mut spec = tiling_identity(tiling);
    spec.size = Some(SizeSpec::Rect { w: 8, h: 8 });
    spec.boundary = Boundary::Dead;
    let topo = Topology::build(&spec)?;
    if topo.lattice().is_none() {
        return Err(format!("{} is not a lattice tiling", tiling.family));
    }
    let s = Arc::new(super::canonical_symmetries(&topo, nb, isotropic));
    cache.lock().unwrap().insert(key, s.clone());
    Ok(s)
}

/// The canonical form of a record, from its cells (see the module notes).
pub fn canonical_form(rec: &PatternRecord) -> Result<CanonicalForm, String> {
    match &rec.site {
        None => lattice_form(rec).map(|x| x.0),
        Some(site) => site_form(rec, site),
    }
}

/// A lattice record's canonical form, and the map from its stored orientation to the canonical
/// one.
fn lattice_form(rec: &PatternRecord) -> Result<(CanonicalForm, Chosen), String> {
    let info = super::verify::lattice_info(&rec.tiling, rec.neighborhood)?;
    let iso = isotropic(&rec.rule);
    let phases = rec.all_phases()?;
    if phases.is_empty() || phases.iter().all(|p| p.is_empty()) {
        return Err(format!("{}: no cells", rec.id));
    }
    if iso {
        let grp = tiling_group(&rec.tiling, rec.neighborhood)?;
        if !grp.primitive() {
            return general_form(rec, &phases, grp, info.ntypes);
        }
    }
    let syms = tiling_symmetries(&rec.tiling, rec.neighborhood, iso)?;
    let d = rec.displacement.unwrap_or([0, 0]);
    let (ph, d2, mi) = canonical_phases_map(&phases, d, &syms);
    let canonical = crate::objects::picture(&ph[0], info.ntypes);
    Ok((
        CanonicalForm {
            phases: ph,
            displacement: d2,
            canonical,
            period: rec.period,
        },
        Chosen::Cell(syms[mi].clone()),
    ))
}

/// A record's cells moved to the origin (the smallest row and column 0), sorted, and the shift.
fn at_origin(mut v: Phase) -> (Phase, [i32; 2]) {
    let mi = v.iter().map(|c| c.i).min().unwrap_or(0);
    let mj = v.iter().map(|c| c.j).min().unwrap_or(0);
    for c in &mut v {
        c.i -= mi;
        c.j -= mj;
    }
    v.sort_unstable();
    (v, [mi, mj])
}

/// The canonical form on a unit cell that holds its tile pattern more than once, over the whole
/// symmetry group ([`super::group`]). Two things differ from a primitive cell. The orientation is
/// the minimum over every symmetry, including those (rotations of the true translation lattice)
/// that no cell map can write. And a spaceship's period is first made its shortest over every
/// translation of the tiling: on such a cell the object finder sees a ship again only when it
/// has moved by a whole cell vector, so a ship moving half a cell per period is found with twice
/// its period, in some directions and not others. In the canonical orientation the period is
/// the shortest multiple of that true period after which the ship has moved by a cell vector,
/// so the same ship found in any direction gets one canonical period, displacement, and phases.
/// Puffers (only their start is stored) and guns (their stream must stay a cell vector) use the
/// symmetries that keep those vectors on the cell's lattice.
fn general_form(
    rec: &PatternRecord,
    phases: &[Phase],
    grp: Arc<TilingGroup>,
    ntypes: u32,
) -> Result<(CanonicalForm, Chosen), String> {
    let n = phases.len() as u64;
    let d = rec.displacement.unwrap_or([0, 0]);
    let dw = grp.world(d);
    // Generation `t` of the stored evolution (spaceships move on by `d` each stored period).
    let at = |t: u64| -> Phase {
        let k = (t / n) as i32;
        phases[(t % n) as usize]
            .iter()
            .map(|c| LCell {
                i: c.i + d[0] * k,
                j: c.j + d[1] * k,
                ..*c
            })
            .collect()
    };
    let image = |g: &Iso, ph: &Phase| -> Result<Phase, String> {
        ph.iter()
            .map(|c| grp.apply(g, c))
            .collect::<Option<Phase>>()
            .ok_or_else(|| format!("{}: a symmetry carried a cell off the tiles", rec.id))
    };
    let spaceship = rec.class == PatternClass::Spaceship;
    // The shortest period over every translation of the tiling.
    let mut q = n;
    if spaceship {
        for q2 in (1..n).filter(|q2| n % q2 == 0) {
            let v = [dw[0] * q2 as f64 / n as f64, dw[1] * q2 as f64 / n as f64];
            if !grp.is_translation(v) {
                continue;
            }
            let back = Iso {
                r: Iso::ID.r,
                t: [-v[0], -v[1]],
            };
            let mut moved = image(&back, &at(q2))?;
            moved.sort_unstable();
            if moved == phases[0] {
                q = q2;
                break;
            }
        }
    }
    let v = [dw[0] * q as f64 / n as f64, dw[1] * q as f64 / n as f64];
    let keeps_lattice = |g: &Iso| match rec.class {
        PatternClass::Puffer => grp.lattice(g.turn(dw)).is_some(),
        PatternClass::Gun => rec.gun.as_ref().is_none_or(|x| {
            grp.lattice(g.turn(grp.world(x.ship_displacement)))
                .is_some()
        }),
        _ => true,
    };
    let mut best: Option<(Phase, usize, u64, [i32; 2])> = None;
    for (gi, g) in grp.isos.iter().enumerate() {
        if !keeps_lattice(g) {
            continue;
        }
        for t in 0..q {
            let (img, shift) = at_origin(image(g, &at(t))?);
            if best.as_ref().is_none_or(|b| img < b.0) {
                best = Some((img, gi, t, shift));
            }
        }
    }
    let (_, gi, t0, shift) = best.ok_or_else(|| format!("{}: no symmetry fits", rec.id))?;
    let g = grp.isos[gi];
    let vs = g.turn(v);
    let (period, count, disp) = if rec.class == PatternClass::Puffer {
        (rec.period, 1, grp.lattice(vs).unwrap_or([0, 0]))
    } else {
        let m = (1..=24u64)
            .find(|m| {
                grp.lattice([vs[0] * *m as f64, vs[1] * *m as f64])
                    .is_some()
            })
            .ok_or_else(|| format!("{}: its displacement is not a translation", rec.id))?;
        let dl = grp
            .lattice([vs[0] * m as f64, vs[1] * m as f64])
            .unwrap_or([0, 0]);
        (m * q, m * q, dl)
    };
    let out = (0..count)
        .map(|u| {
            let mut ph = image(&g, &at(t0 + u))?;
            for c in &mut ph {
                c.i -= shift[0];
                c.j -= shift[1];
            }
            ph.sort_unstable();
            Ok(ph)
        })
        .collect::<Result<Vec<Phase>, String>>()?;
    let canonical = crate::objects::picture(&out[0], ntypes);
    Ok((
        CanonicalForm {
            phases: out,
            displacement: disp,
            canonical,
            period,
        },
        Chosen::Geo(g, grp),
    ))
}

/// An aperiodic record's canonical form, from its stored site.
fn site_form(rec: &PatternRecord, site: &Site) -> Result<CanonicalForm, String> {
    let phases = rec.all_phases()?;
    let p = phases.len();
    if p == 0 {
        return Err(format!("{}: no phases", rec.id));
    }
    if let Some(g) = &site.graph {
        let n = g.cells.len();
        if g.cells.iter().any(|c| c.states.len() != p)
            || phases
                .iter()
                .flatten()
                .any(|c| c.i < 0 || c.i as usize >= n)
        {
            return Err(format!(
                "{}: its pattern graph does not match its phases",
                rec.id
            ));
        }
        let (code, order, t0, _) = super::site_graph::canonical_of(g)
            .ok_or_else(|| format!("{}: its pattern graph cannot be labelled", rec.id))?;
        let mut pos = vec![0i32; n];
        for (i, &v) in order.iter().enumerate() {
            pos[v] = i as i32;
        }
        let out = (0..p)
            .map(|u| {
                let mut v: Phase = phases[(t0 + u) % p]
                    .iter()
                    .map(|c| LCell {
                        i: pos[c.i as usize],
                        j: 0,
                        k: 0,
                        s: c.s,
                    })
                    .collect();
                v.sort_unstable();
                v
            })
            .collect();
        return Ok(CanonicalForm {
            phases: out,
            displacement: [0, 0],
            period: rec.period,
            canonical: super::site_graph::graph_canonical_text(
                &code,
                rec.class == PatternClass::Mover,
                g.degrees,
                g.chirality,
            ),
        });
    }
    // A geometric site: the start with the smallest text, the tiles as stored.
    let n = site.tiles.len();
    if phases
        .iter()
        .flatten()
        .any(|c| c.i < 0 || c.i as usize >= n)
    {
        return Err(format!(
            "{}: its phases name tiles outside its site",
            rec.id
        ));
    }
    let tiles = site
        .tiles
        .iter()
        .map(describe)
        .collect::<Vec<_>>()
        .join(" ");
    let mut best: Option<(String, usize)> = None;
    for t in 0..p {
        let mut text = tiles.clone();
        for u in 0..p {
            let mut v: Vec<(i32, u8)> = phases[(t + u) % p].iter().map(|c| (c.i, c.s)).collect();
            v.sort_unstable();
            text.push_str(" |");
            for (k, s) in v {
                text.push_str(&format!(" {k}:{s}"));
            }
        }
        if best.as_ref().is_none_or(|b| text < b.0) {
            best = Some((text, t));
        }
    }
    let (text, t0) = best.unwrap();
    Ok(CanonicalForm {
        phases: (0..p).map(|u| phases[(t0 + u) % p].clone()).collect(),
        displacement: [0, 0],
        canonical: site_canonical(site.radius, &text),
        period: rec.period,
    })
}

impl PatternRecord {
    /// Rewrite the record in its canonical form (see the module notes): its phases, canonical
    /// text, displacement and the fields that follow from them (bounding box, cell counts,
    /// speed, a gun's ship in the same orientation), then set its geometry pin and its id.
    /// Every importer calls this; records built by JETCARP's own searches are canonical already.
    /// On aperiodic patches a record that is not canonical is matched on its patch and built
    /// again there, which fails if the patch no longer has its site.
    pub fn canonicalize(&mut self) -> Result<(), String> {
        match self.site.clone() {
            None => self.canonicalize_lattice()?,
            Some(site) => {
                let form = site_form(self, &site)?;
                if form.canonical != self.canonical || form.phases != self.all_phases()? {
                    self.rebuild_site(&site)?;
                }
            }
        }
        self.geometry_pin = super::range::record_pin(self)?.pin;
        self.id = super::range::pattern_id(self)?;
        Ok(())
    }

    fn canonicalize_lattice(&mut self) -> Result<(), String> {
        let (form, m) = lattice_form(self)?;
        let info = super::verify::lattice_info(&self.tiling, self.neighborhood)?;
        let moving = matches!(self.class, PatternClass::Spaceship | PatternClass::Puffer);
        let d = form.displacement;
        self.period = form.period;
        self.phases = form.phases.iter().map(|p| encode_cells(p)).collect();
        self.canonical = form.canonical;
        set_counts(self, &form.phases);
        if !moving && self.displacement.is_some() {
            self.displacement = Some(d);
        }
        if moving {
            self.displacement = Some(d);
            let xy = [
                d[0] as f64 * info.a1[0] + d[1] as f64 * info.a2[0],
                d[0] as f64 * info.a1[1] + d[1] as f64 * info.a2[1],
            ];
            self.displacement_xy = Some([(xy[0] * 1e6).round() / 1e6, (xy[1] * 1e6).round() / 1e6]);
            self.speed = ObjClass::Spaceship {
                period: self.period,
                dx: d[0],
                dy: d[1],
            }
            .speed_in(Some((info.a1, info.a2)));
        }
        if let Some(g) = &mut self.gun {
            match &m {
                Chosen::Cell(m) => {
                    g.ship_displacement = m.vector(g.ship_displacement);
                    g.ship_phases = g
                        .ship_phases
                        .iter()
                        .map(|s| decode_cells(s).map(|v| encode_cells(&super::orient(&v, m).0)))
                        .collect::<Result<_, _>>()?;
                }
                Chosen::Geo(iso, grp) => {
                    g.ship_displacement = grp
                        .lattice(iso.turn(grp.world(g.ship_displacement)))
                        .ok_or("the gun's ship leaves off the cell lattice")?;
                    g.ship_phases = g
                        .ship_phases
                        .iter()
                        .map(|s| {
                            let v = decode_cells(s)?;
                            let w = v
                                .iter()
                                .map(|c| grp.apply(iso, c))
                                .collect::<Option<Phase>>()
                                .ok_or("a symmetry carried a ship cell off the tiles")?;
                            Ok(encode_cells(&at_origin(w).0))
                        })
                        .collect::<Result<_, String>>()?;
                }
            }
        }
        Ok(())
    }

    /// Build an aperiodic record's site again from its patch, at its original anchor.
    fn rebuild_site(&mut self, site: &Site) -> Result<(), String> {
        let why = |e: String| format!("{}: cannot put it in canonical form: {e}", self.id);
        let topo = patch_topology(&site.patch).map_err(why)?;
        let anchor = original_anchor(&topo, site).map_err(why)?;
        let cells = match_site(&topo, site, anchor).map_err(why)?;
        let phases: Vec<Phase> = self
            .all_phases()?
            .into_iter()
            .map(|ph| {
                let mut v: Phase = ph
                    .into_iter()
                    .map(|c| LCell {
                        i: cells[c.i as usize] as i32,
                        j: 0,
                        k: 0,
                        s: c.s,
                    })
                    .collect();
                v.sort_unstable();
                v
            })
            .collect();
        let rule = Rule::parse(&self.rule)?.0;
        let (site2, ph, canonical) = if self.class == PatternClass::Mover {
            super::site_graph::build_mover_site(&topo, self.neighborhood, &rule, &phases[0])
        } else {
            super::site_graph::build_site_for(&topo, self.neighborhood, &rule, &phases)
        }
        .map_err(why)?;
        self.site = Some(site2);
        self.phases = ph.iter().map(|p| encode_cells(p)).collect();
        self.canonical = canonical;
        set_counts(self, &ph);
        Ok(())
    }
}

/// Cell counts and the phase-0 bounding box of canonical phases.
fn set_counts(rec: &mut PatternRecord, phases: &[Phase]) {
    let counts: Vec<usize> = phases.iter().map(|p| p.len()).collect();
    let p0 = &phases[0];
    rec.cells = p0.len();
    if rec.class == PatternClass::Mover {
        // A mover's fewest and most cells are over its whole track, not its start.
        return;
    }
    rec.min_cells = counts.iter().copied().min().unwrap_or(0);
    rec.max_cells = counts.iter().copied().max().unwrap_or(0);
    if rec.site.is_none() {
        rec.bbox = [
            0,
            0,
            p0.iter().map(|c| c.i).max().unwrap_or(0) - p0.iter().map(|c| c.i).min().unwrap_or(0),
            p0.iter().map(|c| c.j).max().unwrap_or(0) - p0.iter().map(|c| c.j).min().unwrap_or(0),
        ];
    }
}
