//! What a world costs in memory, before it is built: bytes per cell for each kind of cell field
//! and the extra buffers some rules keep, measured (`tests/memory_per_cell.rs` counts every
//! allocation while a world of about a million cells of each kind is built, seeded and stepped,
//! and checks this table against it).
//!
//! The estimate only decides whether a world may be built ([`super::limits`]): it never changes
//! a world, so a result never depends on it.

use crate::rules::{FieldKind, Rule};
use crate::topology::{SizeSpec, Topology};
use crate::world::CaSpec;
use crate::{TilingSpec, families};

/// Bytes per cell of a field of `kind`, at the peak of building, seeding and stepping it: its
/// state and age and the back buffers a step writes (state `u8`, age `u16`, each twice), and the
/// stepper's own per-cell buffers; a sandpile's heights (`u32`, twice) and its toppling queue; a
/// continuous field's values (`f32`, twice) besides its 8-bit copy.
pub fn field_bytes(kind: FieldKind) -> u64 {
    match kind {
        FieldKind::Binary | FieldKind::Multi(_) => 8,
        FieldKind::Height => 16,
        FieldKind::Continuous => 14,
    }
}

/// Bytes per cell a rule keeps (or allocates while it steps) beyond its field.
pub fn rule_extra_bytes(rule: &Rule) -> u64 {
    match rule {
        // The potential of every cell while it steps.
        Rule::Lenia(_) => 6,
        // The layer each cell belongs to.
        Rule::Layered { .. } => 4,
        // The glow of annihilated cells.
        Rule::Jaca(_) => 1,
        _ => 0,
    }
}

/// Bytes per tile of an aperiodic patch's own graph at the peak of building it: every tile's
/// neighbours (both neighbourhoods), its placement and centre, and the substitution it was cut
/// from (about 80 bytes a tile once built).
pub const PATCH_BYTES_PER_TILE: u64 = 200;

/// Bytes a world of `cells` cells on `tiling` with `cas` takes: every CA's field and extras
/// (fields shared by several CAs are counted for each, so the estimate errs high), and a
/// patch's graph.
pub fn world_bytes(tiling: &TilingSpec, cells: u64, cas: &[CaSpec]) -> u64 {
    let per_ca: u64 = cas
        .iter()
        .map(|c| match Rule::parse(&c.rule) {
            Ok((r, _)) => field_bytes(r.field_kind()) + rule_extra_bytes(&r),
            // A rule this table does not know (a scene's own rule table): the largest field.
            Err(_) => field_bytes(FieldKind::Height) + 6,
        })
        .sum();
    let graph = if is_patch(tiling) {
        PATCH_BYTES_PER_TILE
    } else {
        0
    };
    cells.saturating_mul(per_ca.max(field_bytes(FieldKind::Binary)) + graph)
}

/// The tiling is an aperiodic patch (its size is a level).
pub fn is_patch(tiling: &TilingSpec) -> bool {
    families::family(&tiling.family).is_some_and(|f| f.kind == families::FamilyKind::Aperiodic)
}

/// The cells a world on `tiling` will have, without building it: a lattice's unit cells times
/// its tiles in each, a patch's tile count at its level. `None` when it cannot be told (an
/// unknown family, or a level out of range: building it says why).
pub fn cells_of(tiling: &TilingSpec) -> Option<u64> {
    if is_patch(tiling) {
        // The level as `Topology::build` reads it.
        let level = match tiling.size {
            Some(SizeSpec::Level(l)) => l,
            _ => tiling
                .params
                .as_ref()
                .and_then(|p| p.get("level"))
                .and_then(|v| v.as_u64())
                .map(|l| l as u32)
                .unwrap_or_else(|| crate::aperiodic::default_level(&tiling.family)),
        };
        return crate::aperiodic::tile_count(&tiling.family, level).map(|n| n as u64);
    }
    Topology::cell_count(tiling).ok().flatten()
}
