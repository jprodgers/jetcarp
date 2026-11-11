//! Where a step puts its results ([`Sink`]).

use jetcarp_core::library::store::StepWrite;
use jetcarp_core::library::{AntRow, JobStatus, Sighting, UnstoredFlush};
use jetcarp_core::pattern::PatternRecord;
use serde_json::Value;
use std::path::PathBuf;

/// The results of a step, written by the runner in the step's transaction together with its
/// progress, checkpoint, and heartbeat (so a crash never leaves rows without the checkpoint
/// that counts them, nor a checkpoint without its rows). What became of the records is told to
/// the strategy in that transaction ([`super::Strategy::stored`]), before its progress and
/// checkpoint are taken.
#[derive(Debug, Default)]
pub struct Sink {
    /// Pattern records, each with the soups it appeared in, credited to the job (verified first
    /// if they are not yet, and given their rule range).
    pub patterns: Vec<(PatternRecord, u64)>,
    /// Verified records stored as they are, with their soups (no rule range; unverified ones
    /// are refused): a hunt's finds.
    pub verified: Vec<(PatternRecord, u64)>,
    /// Sightings of patterns already stored.
    pub sightings: Vec<Sighting>,
    /// Objects a hunt counts but does not store as records (its storage policy): copies added,
    /// and the rows of objects now stored removed.
    pub unstored: UnstoredFlush,
    /// Ant runs.
    pub ants: Vec<AntRow>,
    /// Census rule summaries: (rule index, rule, summary).
    pub census: Vec<(usize, String, Value)>,
    /// Files the run wrote (pictures, reports), for the caller.
    pub files: Vec<PathBuf>,
}

impl Sink {
    /// Nothing waits to be written.
    pub fn is_empty(&self) -> bool {
        self.patterns.is_empty()
            && self.verified.is_empty()
            && self.sightings.is_empty()
            && self.unstored.add.is_empty()
            && self.unstored.remove.is_empty()
            && self.ants.is_empty()
            && self.census.is_empty()
    }

    /// The rows to write, taken out (the files stay).
    pub(super) fn take(&mut self, status: Option<JobStatus>, heartbeat: i64) -> StepWrite {
        StepWrite {
            patterns: std::mem::take(&mut self.patterns),
            verified: std::mem::take(&mut self.verified),
            sightings: std::mem::take(&mut self.sightings),
            unstored: std::mem::take(&mut self.unstored),
            ants: std::mem::take(&mut self.ants),
            census: std::mem::take(&mut self.census),
            status,
            heartbeat,
        }
    }
}
