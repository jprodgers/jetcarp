//! JETCARP's store: what JETCARP keeps, on top of the engine (`jetcarp-core`).
//!
//! - [`library`]: the pattern library, the SQLite catalog (`JETCARP_LIBRARY`, else
//!   `<data>/library.sqlite`): its tables and migrations, patterns and their sightings, jobs,
//!   Quick Start rows, ant runs, ratings, and media rows. Its main types are re-exported here
//!   ([`Library`], [`Query`], [`Update`], ...). What a pattern is (its record, identity, and
//!   verification) is the engine's (`jetcarp_core::pattern`).
//! - [`quickstart`]: the Quick Start, one table of entries from every source (built-in, packs,
//!   the atlas a search publishes, promotions): the built-in rows, promotion, reading an entry
//!   by handle, and [`quickstart::menu`], the menu.
//! - [`media`]: the media index (files rendered for catalog targets and recorded in the
//!   catalog); the rendering and encoding are the engine's (`jetcarp_core::media`).
//! - [`website`]: the static site export, with its page template.
//!
//! [`install`] connects the engine to the store: a `PastePattern` command that names a library
//! pattern then finds it in the default library.

pub mod library;
pub mod media;
pub mod quickstart;
pub mod website;

pub use library::{
    AddOutcome, AntCensus, AntQuery, AntRow, AntUpdate, JobStatus, Library, QsSource,
    QuickStartRow, RatingRow, Sighting, SightingRow, ant_id, ant_score, prepare,
};
pub use library::{FindRow, HeatQuery, HeatRow, PruneReport, UnstoredFlush, UnstoredRow, UsageRow};
pub use library::{Query, Update, sort_records};

/// Let the engine find library patterns: a `PastePattern` command (or
/// `jetcarp_core::pattern::paste::resolve` with no library) that names a pattern by id, prefix, or
/// name looks it up in the default library (`JETCARP_LIBRARY`, else `<data>/library.sqlite`),
/// opened for each lookup. Front ends call this once at start; calling it again does nothing.
pub fn install() {
    jetcarp_core::pattern::paste::set_default_library(|key| Library::open_default().get(key));
}

impl jetcarp_core::pattern::paste::Patterns for Library {
    fn pattern(&self, key: &str) -> Result<jetcarp_core::pattern::PatternRecord, String> {
        self.get(key)
    }
}
