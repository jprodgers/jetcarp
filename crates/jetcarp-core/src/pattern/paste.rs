//! `Command::PastePattern`: paste a library pattern (by id or inline) into a CA's field.
//!
//! In the command log a paste is always stored resolved (the cells inline, the library id kept
//! as a label), so a scene replays without the library.
//!
//! Lattice tilings: the pattern is turned by `orientation` (an index into the tiling's
//! symmetries, see `pattern::symmetry`: 0 = as stored; 0-7 on squares, 0-11 on hexagons, the
//! tiling's own symmetry group on others), its bounding box moved to (0, 0), and placed with that
//! corner at `at`. Cells off
//! a dead edge refuse the paste; on wrapped grids they wrap. Aperiodic patches: the pattern's
//! anchor tile goes on `at` (a cell index, or `original`) and the surrounding tiles must match.

use super::{PatternRecord, decode_cells, encode_cells, orient, site};
use crate::library::Library;
use crate::objects::{LCell, Phase};
use crate::topology::{Nb, TilingSpec, Topology};
use serde::{Deserialize, Serialize};

/// What to paste: a library pattern, or cells given inline.
#[derive(Clone, Debug, PartialEq, Default, Serialize, Deserialize)]
pub struct PatternSource {
    /// Library id (or unique prefix, or name). Only a label when `cells` is given.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub library: Option<String>,
    /// One phase as `i,j,k,s` tuples (see [`super::PatternRecord`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cells: Option<String>,
    /// The tiling the cells belong to (family, parameters, edges); checked against the world's.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tiling: Option<TilingSpec>,
    /// Aperiodic patterns: the surrounding tiles.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub site: Option<Box<site::Site>>,
}

/// Where the pattern goes.
#[derive(Clone, Debug, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PasteAt {
    /// Centred on the grid (aperiodic: anchor on the patch's centre tile).
    #[default]
    Center,
    /// Lattice: the pattern's top-left (minimum) corner at column `x`, row `y`, the same `x, y`
    /// as the MCP tools and `set_cells` (`x` counts tiles along the row, so on a tiling with k
    /// tile types per unit cell the corner is unit cell `x / k`).
    Cell { x: i32, y: i32 },
    /// The corner (lattice) or the anchor tile (aperiodic) on this cell index.
    Index { cell: u32 },
    /// Aperiodic: where the pattern was found (same patch, or the same spot on a larger one).
    Original,
}

/// Fill in `cells`, `tiling` and `site` from the library when only `library` is given, taking
/// phase `phase` (modulo the period). Inline sources are returned unchanged.
pub fn resolve(
    src: &PatternSource,
    phase: u32,
    lib: Option<&Library>,
) -> Result<PatternSource, String> {
    if src.cells.is_some() {
        if phase != 0 {
            return Err("phase applies to library patterns; inline cells are one phase".into());
        }
        return Ok(src.clone());
    }
    let key = src
        .library
        .as_deref()
        .ok_or("paste_pattern needs a library id or inline cells")?;
    let owned;
    let lib = match lib {
        Some(l) => l,
        None => {
            owned = Library::open_default();
            &owned
        }
    };
    let rec = lib.get(key)?;
    Ok(source_from_record(&rec, phase as u64))
}

/// An inline source for one phase of a record.
pub fn source_from_record(rec: &PatternRecord, phase: u64) -> PatternSource {
    let p = rec.phases.len().max(1) as u64;
    PatternSource {
        library: Some(rec.id.clone()),
        cells: Some(
            rec.phases
                .get((phase % p) as usize)
                .cloned()
                .unwrap_or_default(),
        ),
        tiling: Some(rec.tiling.clone()),
        site: rec.site.clone().map(Box::new),
    }
}

fn same_tiling(a: &TilingSpec, b: &TilingSpec) -> bool {
    super::tiling_key(a) == super::tiling_key(b)
}

/// Lattice cells in pattern coordinates turned by `orientation` and moved to (0, 0).
pub fn oriented(
    topo: &Topology,
    nb: Nb,
    cells: &[LCell],
    orientation: u32,
) -> Result<Phase, String> {
    let syms = orientations(topo, nb);
    let m = syms.get(orientation as usize).ok_or_else(|| {
        format!(
            "orientation must be 0 to {} on this tiling",
            syms.len().saturating_sub(1)
        )
    })?;
    Ok(orient(cells, m).0)
}

/// The lattice symmetries a pattern can be turned by (identity first).
pub fn orientations(topo: &Topology, nb: Nb) -> Vec<super::CellMap> {
    super::canonical_symmetries(topo, nb, true)
}

/// `[cell, state]` pairs for a resolved source pasted on `topo`.
pub fn paste_cells(
    topo: &Topology,
    nb: Nb,
    src: &PatternSource,
    at: &PasteAt,
    orientation: u32,
) -> Result<Vec<[u32; 2]>, String> {
    let text = src
        .cells
        .as_deref()
        .ok_or("unresolved pattern (no cells)")?;
    let cells = decode_cells(text)?;
    if cells.is_empty() {
        return Err("the pattern has no cells".into());
    }
    if let Some(t) = &src.tiling
        && !same_tiling(t, &topo.spec)
    {
        return Err(format!(
            "the pattern belongs to the {} tiling; this world is {}",
            super::tiling_display_name(t),
            super::tiling_display_name(&topo.spec)
        ));
    }
    let Some(g) = topo.lattice() else {
        let site = src
            .site
            .as_ref()
            .ok_or("on an aperiodic patch a pattern needs its surrounding tiles (a library pattern found on one)")?;
        if orientation != 0 {
            return Err(
                "orientation is fixed on aperiodic patches (the surrounding tiles decide it)"
                    .into(),
            );
        }
        let anchor = match at {
            PasteAt::Index { cell } => *cell as usize,
            PasteAt::Original => site::original_anchor(topo, site)?,
            PasteAt::Center => topo.center_cell(),
            PasteAt::Cell { .. } => {
                return Err("aperiodic patches have no x, y; give a cell index".into());
            }
        };
        let map = site::match_site(topo, site, anchor)?;
        return cells
            .iter()
            .map(|c| {
                map.get(c.i as usize)
                    .map(|&x| [x, c.s as u32])
                    .ok_or_else(|| format!("pattern cell {} is not one of its site's tiles", c.i))
            })
            .collect();
    };
    if src.site.is_some() {
        return Err(
            "this pattern was found on an aperiodic patch; it can only be pasted on one".into(),
        );
    }
    if let Some(c) = cells.iter().find(|c| c.k >= g.ntypes) {
        return Err(format!(
            "tile type {} does not exist on this tiling ({} types)",
            c.k, g.ntypes
        ));
    }
    let v = oriented(topo, nb, &cells, orientation)?;
    let (pw, ph) = (
        v.iter().map(|c| c.i).max().unwrap_or(0) + 1,
        v.iter().map(|c| c.j).max().unwrap_or(0) + 1,
    );
    let (i0, j0) = match at {
        PasteAt::Cell { x, y } => {
            // x is a grid column as in MCP and `set_cells` (index = y * width + x), which counts
            // tiles: on a tiling with k tile types per unit cell, unit column x / k. Row y starts
            // at lattice column -floor(y * shear) (rows outside the grid included).
            let shear = g.a2[0] / g.a1[0];
            (
                x.div_euclid(g.ntypes as i32) - (*y as f64 * shear + 1e-9).floor() as i32,
                *y,
            )
        }
        PasteAt::Index { cell } => {
            if *cell as usize >= topo.len() {
                return Err(format!(
                    "no cell {cell} (the grid has {} cells)",
                    topo.len()
                ));
            }
            let (u, j, _) = g.decompose(*cell as usize);
            (u + g.row_start[j as usize], j)
        }
        PasteAt::Center => {
            let j = (g.h as i32 - ph) / 2;
            let row = j.clamp(0, g.h as i32 - 1) as usize;
            ((g.w as i32 - pw) / 2 + g.row_start[row], j)
        }
        PasteAt::Original => {
            return Err("`original` is for aperiodic patches; use center, cell, or index".into());
        }
    };
    let mut out = Vec::with_capacity(v.len());
    let mut off = 0;
    for c in &v {
        match g.resolve(i0 + c.i, j0 + c.j, c.k) {
            Some(x) => out.push([x as u32, c.s as u32]),
            None => off += 1,
        }
    }
    if off > 0 {
        return Err(format!(
            "the pattern does not fit there: {off} of {} cells fall off the grid's dead edge (pattern {pw}x{ph}, grid {}x{})",
            v.len(),
            g.w,
            g.h
        ));
    }
    Ok(out)
}

/// Inline cells for a phase of a record, turned by `orientation` (for building scenes).
pub fn oriented_cells(
    rec: &PatternRecord,
    topo: &Topology,
    phase: u64,
    orientation: u32,
) -> Result<String, String> {
    let c = rec.phase(phase % rec.period.max(1))?;
    if topo.lattice().is_none() {
        return Ok(encode_cells(&c));
    }
    Ok(encode_cells(&oriented(
        topo,
        rec.neighborhood,
        &c,
        orientation,
    )?))
}
