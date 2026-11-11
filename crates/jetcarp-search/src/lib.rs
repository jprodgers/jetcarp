//! JETCARP's searches: everything that runs as a catalog job, on top of the engine
//! (`jetcarp-core`) and the catalog it stores its finds in.
//!
//! - [`jobs`]: the job runner (a job's spec, its strategy, the runner, handles, the registry,
//!   budgets, and the sink a step writes to) and each kind's strategy.
//! - [`hunt`], [`bounded`], [`explore`], [`evolve`], [`ants`], [`window`], [`census`],
//!   [`pile`]: the searches the strategies run: soup hunts on the plane, soup searches on bounded
//!   grids, the broad explore search (with its working database and atlas publishing), the
//!   evolutionary glider search's catalog of finds, ant and turmite searches, movers on a
//!   sliding window, rule censuses, and large sandpiles. Collisions are
//!   `jetcarp_core::pattern::collide`, run by [`jobs::collide`].
//! - [`dashboard`]: what the dashboard shows of the jobs (summaries, heat maps, finds).
//!
//! The traits a GPU backend implements stay in the engine (`jetcarp_core::hunt::soup::SoupBatcher`,
//! `jetcarp_core::explore::batch::Backend`, `jetcarp_core::pile::Toppler`), so `jetcarp-gpu` does
//! not depend on this crate.

pub mod ants;
pub mod bounded;
pub mod census;
pub mod dashboard;
pub mod evolve;
pub mod explore;
pub mod hunt;
pub mod jobs;
pub mod pile;
pub mod window;
