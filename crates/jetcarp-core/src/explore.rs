//! Exploratory search over tilings, rules, seedings and palettes.
//!
//! - [`batch`]: many worlds run side by side on one topology (the GPU backend lives in `jetcarp-gpu`),
//!   each producing a per-generation trace and some frames.

pub mod archive;
pub mod atlas;
pub mod batch;
pub mod cand;
pub mod features;
pub mod job;
pub mod seeds;
pub mod sheet;
pub mod space;

/// A read-only database handle (the search's database, opened for queries).
pub type Db = rusqlite::Connection;

/// Open a search database for reading.
pub fn open_read(path: &std::path::Path) -> Result<Db, String> {
    rusqlite::Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map_err(|e| e.to_string())
}
