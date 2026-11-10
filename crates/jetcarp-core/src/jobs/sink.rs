//! Where a step puts its results ([`Sink`]).

use crate::library::{AddOutcome, AntRow, PatternRecord, Sighting};
use std::path::PathBuf;

/// The results of a step, written by the runner in the step's transaction together with its
/// progress, checkpoint, and heartbeat (so a crash never leaves rows without the checkpoint
/// that counts them, nor a checkpoint without its rows).
#[derive(Debug, Default)]
pub struct Sink {
    /// Pattern records, each with the soups it appeared in, credited to the job (verified
    /// first if they are not yet).
    pub patterns: Vec<(PatternRecord, u64)>,
    /// Sightings of patterns already stored.
    pub sightings: Vec<Sighting>,
    /// Ant runs.
    pub ants: Vec<AntRow>,
    /// Files the run wrote (pictures, reports), for the caller.
    pub files: Vec<PathBuf>,
    /// What became of the records written so far (one per record, in order).
    pub stored: Vec<Result<AddOutcome, String>>,
}

impl Sink {
    /// Nothing waits to be written.
    pub fn is_empty(&self) -> bool {
        self.patterns.is_empty() && self.sightings.is_empty() && self.ants.is_empty()
    }
}
