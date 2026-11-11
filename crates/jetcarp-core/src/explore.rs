//! Exploratory search over tilings, rules, seedings and palettes: a quality-diversity search
//! (MAP-Elites) that runs as a catalog job of kind `explore` (`jetcarp_search::explore`). The
//! engine's half is here:
//!
//! - [`batch`]: many worlds run side by side on one topology, each producing a per-generation
//!   trace and some frames; the [`batch::Backend`] trait the GPU batch stepper (`jetcarp-gpu`)
//!   implements.
//! - [`features`], [`space`], [`cand`], [`seeds`]: what a run measures, the search space, one
//!   candidate, and its starting pattern.
//! - [`atlas`]: reading the atlas Quick Start browses (the search writes it).

pub mod atlas;
pub mod batch;
pub mod cand;
pub mod features;
pub mod seeds;
pub mod space;
