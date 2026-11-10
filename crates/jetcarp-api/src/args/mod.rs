//! Arguments as people and programs write them: tilings, sizes, boundaries, neighbourhoods,
//! rule names, seeds, and run limits.
//!
//! Every front end reads the same settings: the command line from flags, the MCP tools from JSON
//! arguments, Rhai from script maps (converted to JSON first), and the app's sweep form through
//! the scripts. The parsers live here so a setting means the same thing everywhere.
//!
//! - [`json`]: reading options out of a JSON map (`get_str`, `get_u64`, `check_keys`, ...).
//! - [`size`]: the screen a script fills.
//! - [`tiling`]: tilings by id or as maps, boundaries, and the scripts' tiling options
//!   (`tiling`, `size`, `boundary`, `level`, ...).
//! - [`rule`]: rule names, neighbourhoods, and CA options.
//! - [`seed`]: seed shorthands (`"random:0.2"`, `"pattern:blobs"`, ...).
//! - [`until`]: when a run stops.

pub mod json;
pub mod rule;
pub mod seed;
pub mod size;
pub mod tiling;
pub mod until;

pub use json::{check_keys, get, get_bool, get_f64, get_str, get_u64};
pub use rule::{CA_KEYS, apply_ca_options, ca_spec, parse_nb, resolve_rule};
pub use seed::parse_seed;
pub use size::Screen;
pub use tiling::{TILING_KEYS, is_aperiodic, parse_boundary, tiling_by_id, tiling_spec};
pub use until::{UNTIL_KEYS, Until, parse_until};
