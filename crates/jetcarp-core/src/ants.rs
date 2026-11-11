//! Ant and turmite search (Phase 4, catalog deliverable 11): run turn strings and turmite
//! tables on an unbounded periodic tiling, detect highways exactly, and class every run
//! (`run`), and enumerate the rules to search (`search`). Searches run as resumable catalog jobs
//! (`jetcarp_search::ants`), and a stored run's media are drawn by the store
//! (`jetcarp_store::media::ants`). See the book's "Langton's ant and turmites" page.

pub mod plane;
pub mod run;
pub mod search;
