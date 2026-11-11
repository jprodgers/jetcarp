//! Exploratory search over tilings, rules, seedings and palettes: a quality-diversity search
//! (MAP-Elites) that runs as a catalog job of kind `explore` (`crate::jobs::explore`).
//!
//! - [`batch`]: many worlds run side by side on one topology, each producing a per-generation
//!   trace and some frames; the [`batch::Backend`] trait the GPU batch stepper (`jetcarp-gpu`)
//!   implements.
//! - [`features`], [`space`], [`cand`], [`seeds`]: what a run measures, the search space, one
//!   candidate, and its starting pattern.
//! - [`archive`]: scores, behaviour bins, and the archive of the best runs of each niche.
//! - [`job`]: the search ([`job::Search`]), run in steps.
//! - [`store`]: its working database (`searches/<uuid>/search.sqlite` beside the catalog):
//!   every run, reviews, and the search's state, so a stopped search carries on without reading
//!   its runs again; and importing an old CART explore database.
//! - [`sheet`], [`atlas`]: contact sheets, clips for rating, and the atlas Quick Start reads.

pub mod archive;
pub mod atlas;
pub mod batch;
pub mod cand;
pub mod features;
pub mod job;
pub mod seeds;
pub mod sheet;
pub mod space;
pub mod store;

/// A read-only database handle (a search's database, opened for queries).
pub type Db = rusqlite::Connection;

/// Open a search database for reading.
pub fn open_read(path: &std::path::Path) -> Result<Db, String> {
    store::open_read(path)
}
