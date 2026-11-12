//! Exploratory search over tilings, rules, seedings and palettes: a quality-diversity search
//! (MAP-Elites) that runs as a catalog job of kind `explore` (`jetcarp_search::explore`). The
//! engine's half is here:
//!
//! - [`batch`]: many worlds run side by side on one topology, each producing a per-generation
//!   trace and some frames; the [`batch::Backend`] trait the GPU batch stepper (`jetcarp-gpu`)
//!   implements.
//! - [`features`], [`space`], [`cand`], [`seeds`]: what a run measures, the search space, one
//!   candidate, and its starting pattern.
//!
//! The atlas Quick Start browses is picked from the search's runs (`jetcarp_search::explore::atlas`)
//! and kept in the library's Quick Start table (`jetcarp_store::quickstart`).

pub mod batch;
pub mod cand;
pub mod features;
pub mod seeds;
pub mod space;
