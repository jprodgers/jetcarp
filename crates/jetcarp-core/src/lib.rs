//! JETCARP engine. No window or GPU dependency: tests, the CLI, and the app all drive this crate.

pub mod ants;
pub mod aperiodic;
pub mod catalog;
pub mod cellcheck;
pub mod census;
pub mod computing;
pub mod dashboard;
pub mod detmath;
pub mod edges;
pub mod evolve;
pub mod explore;
pub mod exact;
pub mod exact_graph;
pub mod families;
pub mod geom;
pub mod hashlife;
pub mod hat;
pub mod hunt;
pub mod isohedral;
pub mod jaca;
pub mod lenia;
pub mod library;
pub mod ltl;
pub mod media;
pub mod objects;
pub mod patch;
pub mod paths;
pub mod penrose;
pub mod pile;
pub mod pipeline;
pub mod qsmenu;
pub mod quickstart;
pub mod render;
pub mod reseed;
pub mod rng;
pub mod rules;
pub mod sandpile;
pub mod script;
pub mod seeding;
pub mod sheet;
pub mod sizeclass;
pub mod spectre;
pub mod stage1;
pub mod step;
pub mod table;
pub mod topology;
pub mod turmite;
pub mod vonneumann;
pub mod wallpaper;
pub mod website;
pub mod window;
pub mod world;
pub mod zomega;

pub use seeding::{SeedFill, SeedShape, SeedStates};
pub use topology::{Boundary, Nb, SizeSpec, Storage, TilingSpec, Topology};
pub use world::{CaChange, CaSpec, Command, Scene, SeedSpec, Status, World};

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
pub fn version() -> &'static str {
    VERSION
}
