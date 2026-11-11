//! Ant and turmite search (Phase 4, catalog deliverable 11): run turn strings and turmite
//! tables on an unbounded periodic tiling, detect highways exactly, and class every run
//! (`run`), enumerate the rules to search (`search`), run searches as resumable catalog jobs
//! (`job`), and render media (`media`). See the book's "Langton's ant and turmites" page.

pub mod job;
pub mod media;
pub mod plane;
pub mod run;
pub mod search;
