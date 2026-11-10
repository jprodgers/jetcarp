//! Arguments as people and programs write them: tilings, sizes, boundaries, neighbourhoods,
//! rule names, seeds, run limits, and ranges.
//!
//! Every front end reads the same settings: the command line from flags, the MCP tools from JSON
//! arguments, Rhai from script maps (converted to JSON first), and the app's sweep form through
//! the scripts. The parsers live here so a setting means the same thing everywhere.
//!
//! - [`json`]: reading options out of a JSON map (`get_str`, `get_u64`, `check_keys`, ...).
//! - [`size`]: the screen a script fills, and sizes and ranges as text (`"64x64"`, `"L5"`,
//!   `"-3..3"`).
//! - [`tiling`]: tilings by id or as maps, tiling lists, boundaries, and the scripts' tiling
//!   options (`tiling`, `size`, `boundary`, `level`, ...).
//! - [`rule`]: rule names, neighbourhoods, and CA options.
//! - [`seed`]: seed shorthands (`"random:0.2"`, `"pattern:blobs"`, ...).
//! - [`until`]: when a run stops.
//! - [`mcp`]: the MCP tools' own readings of rule names, seeds, and CA items, kept where they
//!   give different results from the shared ones.
//!
//! Where the front ends accepted different spellings for the same thing (`moore` or `vn` for a
//! neighbourhood, `torus` for a wrapped boundary), the parsers here accept all of them; each
//! front end keeps its own error messages and defaults.

pub mod json;
pub mod mcp;
pub mod rule;
pub mod seed;
pub mod size;
pub mod tiling;
pub mod until;

pub use json::{check_keys, get, get_bool, get_f64, get_pair, get_str, get_u64};
pub use rule::{
    CA_KEYS, apply_ca_options, ca_spec, nb_word, parse_nb, parse_pile_nb, resolve_rule,
};
pub use seed::parse_seed;
pub use size::{Screen, count_range, offset_range, size_pair, size_spec};
pub use tiling::{
    TILING_KEYS, is_aperiodic, parse_boundary, tiling_by_id, tiling_spec, tilings_or_square,
};
pub use until::{UNTIL_KEYS, Until, parse_until};
