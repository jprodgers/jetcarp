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
//!   Orientation is only as complete as the symmetry search ([`super::symmetry`]: a closed point
//!   group on every periodic tiling built today, but not proven complete).
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
fn lattice_form(rec: &PatternRecord) -> Result<(CanonicalForm, CellMap), String> {
    let info = super::verify::lattice_info(&rec.tiling, rec.neighborhood)?;
    let syms = tiling_symmetries(&rec.tiling, rec.neighborhood, isotropic(&rec.rule))?;
    let phases = rec.all_phases()?;
    if phases.is_empty() || phases.iter().all(|p| p.is_empty()) {
        return Err(format!("{}: no cells", rec.id));
    }
    let d = rec.displacement.unwrap_or([0, 0]);
    let (ph, d2, mi) = canonical_phases_map(&phases, d, &syms);
    let canonical = crate::objects::picture(&ph[0], info.ntypes);
    Ok((
        CanonicalForm {
            phases: ph,
            displacement: d2,
            canonical,
        },
        syms[mi].clone(),
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
            g.ship_displacement = m.vector(g.ship_displacement);
            g.ship_phases = g
                .ship_phases
                .iter()
                .map(|s| decode_cells(s).map(|v| encode_cells(&super::orient(&v, &m).0)))
                .collect::<Result<_, _>>()?;
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
