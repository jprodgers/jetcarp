//! Exploratory search over tilings, rules, seedings and palettes: a quality-diversity search
//! (MAP-Elites) that runs as a catalog job of kind `explore` (`crate::jobs::explore`). What a
//! run is and measures (batches, features, the search space, candidates, seeds) is the engine's
//! (`jetcarp_core::explore`); the search is here:
//!
//! - [`archive`]: scores, behaviour bins, and the archive of the best runs of each niche.
//! - [`job`]: the search ([`job::Search`]), run in steps.
//! - [`runs`]: its run records, sealed Parquet segments ([`segments`]) and the newest runs in
//!   its working database ([`store`], `searches/<uuid>/search.sqlite` beside the library, with
//!   the search's state, so a stopped search carries on without reading its runs again); and
//!   importing an old CART explore database, whose runs [`remeasure`] checks (and runs again
//!   where a tiling no longer reproduces them).
//! - [`reviews`]: verdicts on runs, kept as ratings in the library.
//! - [`sheet`], [`atlas`]: contact sheets, clips for rating, and publishing the atlas Quick
//!   Start reads.

pub mod archive;
pub mod atlas;
pub mod job;
pub mod remeasure;
pub mod reviews;
pub mod runs;
pub mod segments;
pub mod sheet;
pub mod store;

/// A search's runs opened for reading.
pub type Db = runs::Runs;

/// Open a search's runs for reading (`path`: its working database, `search.sqlite`).
pub fn open_read(path: &std::path::Path) -> Result<Db, String> {
    runs::Runs::open_read(path)
}
