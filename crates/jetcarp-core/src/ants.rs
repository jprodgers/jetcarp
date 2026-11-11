//! Ant and turmite search (Phase 4, catalog deliverable 11): run turn strings and turmite
//! tables on an unbounded periodic tiling, detect highways exactly, and class every run
//! (`run`), enumerate the rules to search (`search`), and render media (`media`). Searches run
//! as resumable catalog jobs (`jetcarp_search::ants`). See the book's "Langton's ant and
//! turmites" page.

pub mod media;
pub mod plane;
pub mod run;
pub mod search;
