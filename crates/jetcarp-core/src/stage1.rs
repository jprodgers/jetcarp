//! Stage 1 of the catalog search (Phase 4): find visually interesting **settings** (rule, tiling
//! and neighbourhood, soup density and size, grid size and boundary, generations) on every
//! periodic grid.
//!
//! - [`eval`]: one setting run from several seeded soups into a descriptor vector.
//! - [`score`]: the interest score (features, hand-set weights, ridge refits from ratings).
//! - [`job`]: the adaptive search (coarse pass, refinement, rule-space neighbours) as a resumable
//!   catalog job that writes `settings` rows and Quick Start entries.
//! - [`sheet`]: contact sheets of the top settings, with a JSON sidecar for a model shortlist.

pub mod eval;
pub mod job;
pub mod score;
pub mod sheet;
