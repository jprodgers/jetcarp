//! The MCP tools. Each one reads the host's world or changes it through [`Command`]s.
//!
//! Descriptions are written for a model that has never seen JETCARP: they say what the tool does,
//! give a minimal example, and name the tool to call next.

use crate::cells::{self, Grid};
use crate::host::Host;
use crate::util;
use jetcarp_core::render::{self, RenderOptions};
use jetcarp_core::rules::{CatalogEntry, FieldKind};
use jetcarp_core::world::{CaInstance, CaSpec, Command};
use jetcarp_core::*;
use serde_json::{Value, json};
use std::fmt::Write as _;

/// Largest world `new_world` or `set_tiling` will build.
pub const MAX_CELLS: u64 = 4_000_000;
/// Most generations one call may run.
pub const MAX_GENS: u64 = 1_000_000;
/// How far `step` runs with `stop_when_settled` when no count is given.
const SETTLE_GENS: i64 = 5000;
/// Most cell updates (cells x generations) one call may run.
pub const MAX_WORK: u64 = 2_000_000_000;
/// Most cells `read_cells` prints as a picture.
pub const MAX_PICTURE: usize = 120 * 120;
/// Most cells one `set_cells` call may write.
pub const MAX_SET: usize = 200_000;
/// Longest `run_script` run, in seconds.
pub const MAX_SCRIPT_SECONDS: f64 = 600.0;

/// What a tool returns: MCP content blocks.
#[derive(Clone, Debug)]
pub struct ToolOutput {
    pub content: Vec<Value>,
    pub is_error: bool,
}

impl ToolOutput {
    pub fn text(s: impl Into<String>) -> ToolOutput {
        ToolOutput {
            content: vec![json!({"type": "text", "text": s.into()})],
            is_error: false,
        }
    }
    pub fn error(s: impl Into<String>) -> ToolOutput {
        ToolOutput {
            is_error: true,
            ..ToolOutput::text(s)
        }
    }
    pub fn json(v: &Value) -> ToolOutput {
        ToolOutput::text(serde_json::to_string_pretty(v).unwrap_or_default())
    }
    /// All text blocks joined (for tests and logs).
    pub fn joined_text(&self) -> String {
        self.content
            .iter()
            .filter_map(|c| c.get("text").and_then(|t| t.as_str()))
            .collect::<Vec<_>>()
            .join("\n")
    }
}

pub(crate) type Res = Result<ToolOutput, String>;

/// Progress of a long tool call: `(progress, total, message)`, sent by the transport as an MCP
/// `notifications/progress` when the client asked for it (a `progressToken` in `_meta`).
pub type ProgressFn = std::sync::Arc<dyn Fn(f64, Option<f64>, String) + Send + Sync>;

/// Set when the client cancels the call (`notifications/cancelled`).
pub type Cancel = std::sync::Arc<std::sync::atomic::AtomicBool>;

fn help(args: &Value) -> Res {
    check_unknown(args, &["topic"])?;
    Ok(ToolOutput::text(
        match string(args, "topic")?.unwrap_or("overview") {
            "overview" => HELP.to_string(),
            "catalog" => HELP_CATALOG.to_string(),
            "jobs" => HELP_JOBS.to_string(),
            "scripts" | "script" => jetcarp_script::help_text().to_string(),
            t => {
                return Err(format!(
                    "`topic` is overview, catalog, jobs, or scripts, not {t}"
                ));
            }
        },
    ))
}

pub const NAMES: &[&str] = &[
    "jetcarp_help",
    "list_rules",
    "list_tilings",
    "new_world",
    "world_status",
    "set_cells",
    "read_cells",
    "inspect_cell",
    "step",
    "render_image",
    "apply_commands",
    "get_scene",
    "load_scene",
    "run_script",
    // The catalog (`library_tools.rs`): one list, get, and update tool for every kind of record.
    "library_list",
    "library_get",
    "library_search",
    "paste_pattern",
    "library_update",
    "library_promote",
    "library_collide",
    // Background jobs: one start tool per kind (`hunt_tools.rs`, `stage1_tools.rs`,
    // `job_tools.rs`), one status and one stop tool for all of them.
    "hunt_start",
    "stage1_start",
    "ants_start",
    "pile_start",
    "evolve_start",
    "mover_start",
    "job_status",
    "job_stop",
    // Media and the website export (`media_tools.rs`).
    "media_render",
    "site_export",
    // Output folders, read only (`files_tools.rs`).
    "files_list",
    // The exploratory search (`explore_tools.rs`).
    "explore_start",
    "explore_status",
    "explore_stop",
    "explore_query",
    "explore_sheet",
    "explore_try",
    "explore_scene",
    "explore_review",
    // Reseeding and wallpapers (`display_tools.rs`).
    "reseed",
    "export_wallpaper",
];

pub fn exists(name: &str) -> bool {
    NAMES.contains(&name)
}

pub(crate) fn tool(
    name: &str,
    title: &str,
    read_only: bool,
    description: &str,
    schema: Value,
) -> Value {
    json!({
        "name": name,
        "title": title,
        "description": description,
        "inputSchema": schema,
        "annotations": {
            "title": title,
            "readOnlyHint": read_only,
            "destructiveHint": matches!(name, "new_world" | "load_scene"),
            "idempotentHint": read_only,
            "openWorldHint": false,
        },
    })
}

fn no_args() -> Value {
    json!({"type": "object", "properties": {}, "additionalProperties": false})
}

fn ca_arg() -> Value {
    json!({
        "description": "Which CA (cellular automaton) to use: its id number or its name, from world_status. Default: the first CA.",
        "type": ["integer", "string"],
    })
}

/// Tool definitions for `tools/list`, in a fixed order.
pub fn definitions() -> Vec<Value> {
    vec![
        tool(
            "jetcarp_help",
            "JETCARP help and worked examples",
            true,
            "Explains JETCARP and shows worked examples. `topic`: \"overview\" (default: what JETCARP is, coordinates, a glider in Life, Brian's Brain on hexagons, sandpiles, several CAs at once), \"catalog\" (finding, listing, curating, and promoting patterns, ants, settings), \"jobs\" (long searches in the background: start, job_status, job_stop, resume), or \"scripts\" (the full function reference for run_script). Call this first if you are unsure how the tools fit together.",
            json!({"type": "object", "properties": {
                "topic": {"type": "string", "enum": ["overview", "catalog", "jobs", "scripts"], "description": "Default overview."},
            }, "additionalProperties": false}),
        ),
        tool(
            "list_rules",
            "List cellular-automaton rules",
            true,
            "Lists the named rules (name, notation, family, one-line description) and explains rule notation. Any name or notation from this list can be used as a CA's `rule` in new_world, e.g. \"Life\", \"Brian's Brain\", \"B36/S23\", \"W30\", \"Ant RL\".",
            json!({"type": "object", "properties": {
                "family": {"type": "string", "description": "Only list rules of this family, e.g. \"Life-like\", \"Generations\", \"Elementary\", \"Agents\". Omit to list all."}
            }, "additionalProperties": false}),
        ),
        tool(
            "list_tilings",
            "List tilings",
            true,
            "Lists the tilings (cell shapes and grids) a world can use: id, name, how many neighbours each cell has (edge = sharing an edge, touch = sharing an edge or a corner), and how size works. Use the id as `tiling` in new_world, e.g. \"square\", \"hex\", \"triangle\", \"cairo\", \"spectre\"; vertex configurations such as \"3.4.6.4\" or \"4.8.8\" work too. Without arguments: the tiling families (about 30) and a summary of the catalog (1,248 k-uniform tilings, 81 isohedral types). `search` lists catalog tilings whose id or name contains the text (e.g. \"k2-\", \"3.4.6.4\", \"ih4\"), at most 60.",
            json!({"type": "object", "properties": {
                "search": {"type": "string", "description": "List catalog tilings whose id or name contains this text (case-insensitive)."},
            }, "additionalProperties": false}),
        ),
        tool(
            "new_world",
            "Create a new world",
            false,
            "Replaces the current world with a new one: a tiling (the grid of cells) and the cellular automata (CAs) that run on it. Example: {\"tiling\": \"hex\", \"width\": 60, \"height\": 60, \"boundary\": \"wrap\", \"cas\": [\"Brian's Brain\"]}. Each CA starts from its rule's default seed (random cells for Life-like rules) unless you set `seed`; use {\"rule\": \"Life\", \"seed\": \"empty\"} for a blank field you then draw on with set_cells. Returns the world status with each CA's id. Next: set_cells, step, read_cells, render_image.",
            json!({"type": "object", "properties": {
                "tiling": {"type": "string", "description": "Tiling id from list_tilings: \"square\" (default), \"hex\", \"triangle\", \"cairo\", \"spectre\" (aperiodic), and others."},
                "width": {"type": "integer", "minimum": 8, "maximum": 4096, "description": "Grid width in unit cells (on square and hex grids one unit cell is one cell). Give with height."},
                "height": {"type": "integer", "minimum": 8, "maximum": 4096, "description": "Grid height in unit cells (rows)."},
                "cells": {"type": "integer", "minimum": 64, "maximum": MAX_CELLS, "description": "Instead of width/height: about this many cells in a roughly square region."},
                "level": {"type": "integer", "minimum": 1, "maximum": 7, "description": "Aperiodic tilings only. Spectre: substitution level, 3 = 559 tiles, 4 = 4,401, 5 = 34,649, 6 = 272,791, 7 = 2,147,679. Hat and Penrose: 3 = 500, 4 = 4,000, 5 = 35,000, 6 = 280,000, 7 = 2,240,000."},
                "boundary": {"type": "string", "enum": ["wrap", "dead", "live"], "description": "What lies past the grid edge: \"wrap\" (default: the grid is a torus, patterns leaving one side come back on the other), \"dead\" (empty cells), \"live\" (cells counting as alive). Aperiodic tilings are always dead."},
                "cas": {
                    "type": "array",
                    "description": "The CAs to run, in order. Each item is a rule name or notation string (\"Life\", \"B3/S23\", \"Brian's Brain\", \"W30\", \"Ant RL\", \"Sandpile\") or an object {\"rule\": ..., \"seed\": ..., \"name\": ..., \"neighborhood\": \"touch\"|\"edge\", \"palette\": ..., \"steps_per_gen\": n}. seed is one of \"empty\", \"random\" (30% live), \"random:0.2\" (20% live), \"center\", \"top_center\", \"random_states\", \"ants:3\", \"pile:10000\". Pattern seeds (any tiling) are a shape plus a fill: \"pattern:blobs\", \"pattern:voronoi:0.8:by_region\" (shape, density, states), or {\"shape\": \"blobs\", \"count\": 3, \"min_radius\": 0.05, \"max_radius\": 0.1, \"density\": 0.4, \"states\": \"by_region\"}. Shapes: all, disc, blobs, clusters, dots, ring, rings, stripes, checker, gradient, radial_gradient, noise, voronoi, symmetric. States: one, uniform, range:lo:hi, signed:share (JACA), by_region (one random state or sign per region). neighborhood \"touch\" (default for B/S rules) counts cells sharing an edge or a corner: 8 on squares, 6 on hexagons, 12 on triangles; \"edge\" counts edge-sharing cells only: 4, 6, 3. Binary CAs share one field by default, so two Life-like CAs act on the same cells.",
                    "items": {"anyOf": [{"type": "string"}, {"type": "object"}]},
                },
                "rng_seed": {"type": "integer", "minimum": 0, "description": "Random seed (default 1). The same seed gives the same run."},
                "name": {"type": "string", "description": "Optional scene name."},
                "tiling_params": {"type": "object", "description": "Advanced: tile shape parameters (JETCARP tile JSON `params`). Usually omitted."},
            }, "additionalProperties": false}),
        ),
        tool(
            "world_status",
            "World status",
            true,
            "Reports the current world: tiling and grid size (width x height for coordinates), generation, state hash, whether it is running, empty, frozen, or repeating with a period (checked exactly), population per field, and the list of CAs with their ids and rules. Cheap; call it whenever you need ids or sizes.",
            no_args(),
        ),
        tool(
            "set_cells",
            "Draw cells",
            false,
            "Draws cells into a CA's field. Coordinates: x = column, y = row, (0, 0) is the top-left cell; world_status gives the grid width and height. Give ONE of: `pattern` (a text picture, rows separated by \\n, '.' empty and 'O' live, digits 1-9 for higher states), `rle` (run-length encoded, e.g. \"bo$2bo$3o!\"), `cells` ([[x, y], [x, y, state], ...]), or `indices` ([index, ...] or [[index, state], ...]; the only form on aperiodic tilings). `x`, `y` place the pattern's top-left corner (default: centred). Example, a glider at column 10, row 10 moving down-right: {\"pattern\": \".O.\\n..O\\nOOO\", \"x\": 10, \"y\": 10}. Only non-empty pattern cells are written; set clear_first to empty the field first, or erase to clear the listed cells. On sandpile fields values are grains added. Next: step, then read_cells.",
            json!({"type": "object", "properties": {
                "ca": ca_arg(),
                "pattern": {"type": "string", "description": "Text picture, one line per row: '.' empty, 'O' (or '*', '#', 'X') live, '1'-'9' states."},
                "rle": {"type": "string", "description": "Run-length encoded pattern as on LifeWiki: b = empty, o = live, $ = next row, ! = end, numbers repeat. Header lines (x = ..) are ignored."},
                "x": {"type": "integer", "description": "Column of the pattern's top-left corner."},
                "y": {"type": "integer", "description": "Row of the pattern's top-left corner."},
                "cells": {"type": "array", "items": {"type": "array", "items": {"type": "integer"}}, "description": "Cells as [x, y] or [x, y, state]."},
                "indices": {"type": "array", "items": {"anyOf": [{"type": "integer"}, {"type": "array", "items": {"type": "integer"}}]}, "description": "Cells by index, as index or [index, state]."},
                "state": {"type": "integer", "minimum": 0, "description": "State for cells given without one (default 1)."},
                "clear_first": {"type": "boolean", "description": "Empty the whole field before drawing."},
                "erase": {"type": "boolean", "description": "Set the listed or pattern cells to empty (0) instead of drawing them."},
            }, "additionalProperties": false}),
        ),
        tool(
            "read_cells",
            "Read cells as text",
            true,
            "Shows a CA's field as a text picture so you can inspect patterns: one row per line, prefixed by its row number y, with a ruler of column numbers; '.' empty, 'O' live, digits for higher states or sandpile grains. With no region it shows the smallest box around all non-empty cells (plus 1 cell of margin) and reports the live-cell count and bounding box (x and y ranges), which tells you where a pattern is. Give x, y, width, height to see a fixed region. format \"list\" returns [x, y, state] triples instead.",
            json!({"type": "object", "properties": {
                "ca": ca_arg(),
                "x": {"type": "integer", "minimum": 0, "description": "Left column of the region."},
                "y": {"type": "integer", "minimum": 0, "description": "Top row of the region."},
                "width": {"type": "integer", "minimum": 1, "maximum": 200, "description": "Region width (default 40 when x, y are given)."},
                "height": {"type": "integer", "minimum": 1, "maximum": 200, "description": "Region height (default 40 when x, y are given)."},
                "format": {"type": "string", "enum": ["picture", "list"], "description": "\"picture\" (default) or \"list\" of non-empty cells."},
            }, "additionalProperties": false}),
        ),
        tool(
            "inspect_cell",
            "Inspect one cell",
            true,
            "Describes one cell: its index and (x, y), tile type, centre point, value in every field, and its neighbours in clockwise order (edge-sharing and touching), each with coordinates. Use it to learn the neighbour layout on hexagon, triangle, or other non-square tilings before drawing.",
            json!({"type": "object", "properties": {
                "x": {"type": "integer", "description": "Column."},
                "y": {"type": "integer", "description": "Row."},
                "index": {"type": "integer", "minimum": 0, "description": "Cell index (instead of x, y; required on aperiodic tilings)."},
            }, "additionalProperties": false}),
        ),
        tool(
            "step",
            "Run generations",
            false,
            "Advances the simulation by `generations` (default 1); every enabled CA steps together. Returns the new generation, populations, and status (running, empty, frozen, or repeating with its period). Set stop_when_settled to stop early once the world is empty or repeating.",
            json!({"type": "object", "properties": {
                "generations": {"type": "integer", "minimum": 1, "maximum": MAX_GENS, "description": "How many generations to run (default 1, or at most 5000 with stop_when_settled)."},
                "stop_when_settled": {"type": "boolean", "description": "Stop as soon as the world is empty, frozen, or exactly periodic."},
            }, "additionalProperties": false}),
        ),
        tool(
            "render_image",
            "Render a picture",
            true,
            "Renders the whole world as a PNG image and returns it, so you (and the user) can see it. Empty cells are dark; live cells use the CA's palette, brighter for young cells. Optionally also saves the PNG to a file (with the scene embedded).",
            json!({"type": "object", "properties": {
                "max_side": {"type": "integer", "minimum": 64, "maximum": 2048, "description": "Longest image side in pixels (default 640)."},
                "outlines": {"type": "boolean", "description": "Draw tile outlines (default: only when tiles are at least 7 px)."},
                "save_path": {"type": "string", "description": "Also write the PNG to this file path (folders are created; relative paths are from the server's working directory)."},
            }, "additionalProperties": false}),
        ),
        tool(
            "apply_commands",
            "Apply JETCARP commands",
            false,
            "Applies JETCARP commands in order: the same commands the app's buttons issue, recorded in the scene's command log. Stops at the first failing command and says which. Each command is an object with \"op\": {\"op\":\"add_ca\",\"ca\":{\"rule\":\"B3/S23\",\"seed\":\"empty\"}} (returns the new id); {\"op\":\"remove_ca\",\"id\":2}; {\"op\":\"update_ca\",\"id\":1,\"change\":{\"rule\":\"B36/S23\",\"enabled\":false,\"palette\":\"Tide\",\"steps_per_gen\":10,\"neighborhood\":\"edge\"}}; {\"op\":\"seed\",\"id\":1,\"seed\":\"random:0.2\"}; {\"op\":\"clear\",\"id\":1}; {\"op\":\"set_cells\",\"id\":1,\"cells\":[[index,state]]}; {\"op\":\"step\",\"gens\":10}; {\"op\":\"reset\"} (clear and reseed every CA, generation 0); {\"op\":\"set_rng_state\",\"state\":42}; {\"op\":\"set_tiling\",\"tiling\":{\"family\":\"hex\",\"size\":{\"rect\":{\"w\":60,\"h\":60}},\"boundary\":\"wrap\"}}; {\"op\":\"sandpile\",\"id\":1,\"action\":\"stabilize\"} (also \"identity\", {\"add_everywhere\":{\"grains\":3}}, {\"add_random\":{\"count\":1000}}); {\"op\":\"set_palette\",\"palette\":{\"name\":\"Mine\",\"stops\":[[0,0,0],[255,200,0]],\"background\":[250,245,235]}} (optional background and stage colours); {\"op\":\"reseed\",\"mode\":{\"kind\":\"rain\"}} and {\"op\":\"set_reseed\",\"policies\":[...]} (see the reseed tool). Palettes: Ember, Tide, Prism, Ink, Mono, Aurora, Synthwave, Moss, Candy, Lava, Glacier, Phosphor, Koi, Desert, Thermal, Deep Sea, Paper, Blueprint, Neon, Autumn (Candy, Glacier, Desert, and Paper have light backgrounds).",
            json!({"type": "object", "properties": {
                "commands": {"type": "array", "items": {"type": "object"}, "description": "Command objects, each with an \"op\" field."},
            }, "required": ["commands"], "additionalProperties": false}),
        ),
        tool(
            "get_scene",
            "Get or save the scene",
            false,
            "Returns the current world as JETCARP scene JSON: tiling, CA specs, random seed, and the command log since the last reset; load_scene with it reproduces the exact state (same hash). With `path`, writes it to that file instead (the JETCARP app and `jetcarp run` open it), e.g. {\"path\": \"out/glider.json\"}.",
            json!({"type": "object", "properties": {
                "path": {"type": "string", "description": "Also write the scene to this .json file."},
            }, "additionalProperties": false}),
        ),
        tool(
            "load_scene",
            "Load a scene",
            false,
            "Replaces the current world with a JETCARP scene: either `scene` (the JSON object or text from get_scene) or `path` (a .json scene file on this computer). Replays the scene's commands, so the state matches the saved one.",
            json!({"type": "object", "properties": {
                "scene": {"type": ["object", "string"], "description": "Scene JSON (object or text)."},
                "path": {"type": "string", "description": "Path of a scene .json file."},
            }, "additionalProperties": false}),
        ),
        tool(
            "run_script",
            "Run a JETCARP script",
            false,
            "Runs a JETCARP script (Rhai language) and returns its log, errors with line numbers, output folders, and sweep summaries. Scripts make their own worlds (the current world is not changed), run them until something happens, save PNGs, and run parallel rule sweeps into <Pictures>/JETCARP. Example: `let w = world(#{ tiling: \"hex\", size: [120, 80], boundary: \"dead\" }); w.add_ca(\"HexW3 8229\", #{ seed: \"top_center\" }); let r = w.run(#{ until: \"done\" }); print(r.generations); w.save_png(out_dir(\"test\") + \"/a.png\");` or `sweep(#{ rules: \"W\", until: \"done\", png: #{ size: [400, 300] } });`. jetcarp_help {\"topic\": \"scripts\"} lists every function. Runs are limited in time (max_seconds) and world size.",
            json!({"type": "object", "properties": {
                "script": {"type": "string", "description": "The script text (Rhai)."},
                "params": {"type": "object", "description": "Values the script reads as `params.key`."},
                "max_seconds": {"type": "number", "minimum": 1, "maximum": MAX_SCRIPT_SECONDS, "description": "Stop the script after this many seconds (default 60)."},
                "screen": {"type": "string", "description": "Screen size for size: \"screen\" worlds and default image size, e.g. \"1920x1080\" (default)."},
                "cell_px": {"type": "number", "minimum": 1, "maximum": 256, "description": "Tile size in pixels for screen-sized worlds (default 8)."},
                "log_lines": {"type": "integer", "minimum": 1, "maximum": 2000, "description": "Most log lines to return, the last ones (default 200)."},
                "check_only": {"type": "boolean", "description": "Only check the script for syntax errors."},
            }, "required": ["script"], "additionalProperties": false}),
        ),
    ]
    .into_iter()
    .chain(crate::library_tools::definitions())
    .chain(crate::hunt_tools::definitions())
    .chain(crate::stage1_tools::definitions())
    .chain(crate::job_tools::definitions())
    .chain(crate::media_tools::definitions())
    .chain(crate::files_tools::definitions())
    .chain(crate::explore_tools::definitions())
    .chain(crate::display_tools::definitions())
    .collect()
}

/// Run a tool. `name` must be one of [`NAMES`].
pub fn call(host: &mut dyn Host, name: &str, args: &Value) -> ToolOutput {
    call_with_progress(host, name, args, None)
}

/// [`call`], reporting progress of long calls (`run_script`) through `progress`.
pub fn call_with_progress(
    host: &mut dyn Host,
    name: &str,
    args: &Value,
    progress: Option<ProgressFn>,
) -> ToolOutput {
    call_ctx(host, name, args, progress, None)
}

/// [`call`] with progress and a cancel flag: `run_script`, `library_search`, and `step` stop
/// soon after the flag is set (an MCP `notifications/cancelled` for the request).
pub fn call_ctx(
    host: &mut dyn Host,
    name: &str,
    args: &Value,
    progress: Option<ProgressFn>,
    cancel: Option<Cancel>,
) -> ToolOutput {
    let r = match name {
        "jetcarp_help" => help(args),
        "list_rules" => list_rules(args),
        "list_tilings" => list_tilings(args),
        "new_world" => new_world(host, args),
        "world_status" => Ok(ToolOutput::json(&status(host))),
        "set_cells" => set_cells(host, args),
        "read_cells" => read_cells(host, args),
        "inspect_cell" => inspect_cell(host, args),
        "step" => step(host, args, cancel.as_deref()),
        "render_image" => render_image(host, args),
        "apply_commands" => apply_commands(host, args),
        "get_scene" => get_scene(host, args),
        "load_scene" => load_scene(host, args),
        "run_script" => run_script(args, progress, cancel),
        n if crate::library_tools::handles(n) => {
            crate::library_tools::call(host, n, args, progress, cancel)
        }
        n if crate::explore_tools::handles(n) => crate::explore_tools::call(n, args),
        n if crate::hunt_tools::handles(n) => crate::hunt_tools::call(n, args),
        n if crate::stage1_tools::handles(n) => crate::stage1_tools::call(n, args),
        "job_status" => crate::job_tools::status_wait(args, cancel.as_deref()),
        n if crate::job_tools::handles(n) => crate::job_tools::call(n, args),
        n if crate::media_tools::handles(n) => crate::media_tools::call(n, args),
        n if crate::files_tools::handles(n) => crate::files_tools::call(n, args),
        n if crate::display_tools::handles(n) => crate::display_tools::call(host, n, args),
        n if crate::app_tools::handles(n) => crate::app_tools::call(host, n, args),
        _ => Err(format!("unknown tool {name}")),
    };
    r.unwrap_or_else(ToolOutput::error)
}

// ---------------------------------------------------------------------------------------------
// Argument helpers

pub(crate) fn int(args: &Value, key: &str) -> Result<Option<i64>, String> {
    match args.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => v
            .as_i64()
            .or_else(|| v.as_f64().filter(|f| f.fract() == 0.0).map(|f| f as i64))
            .or_else(|| v.as_str().and_then(|s| s.trim().parse().ok()))
            .map(Some)
            .ok_or_else(|| format!("`{key}` must be an integer, got {v}")),
    }
}

pub(crate) fn boolean(args: &Value, key: &str) -> Result<bool, String> {
    match args.get(key) {
        None | Some(Value::Null) => Ok(false),
        Some(Value::Bool(b)) => Ok(*b),
        Some(Value::String(s)) if s == "true" || s == "false" => Ok(s == "true"),
        Some(v) => Err(format!("`{key}` must be true or false, got {v}")),
    }
}

pub(crate) fn string<'a>(args: &'a Value, key: &str) -> Result<Option<&'a str>, String> {
    match args.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) => Ok(Some(s)),
        Some(v) => Err(format!("`{key}` must be a string, got {v}")),
    }
}

pub(crate) fn check_unknown(args: &Value, allowed: &[&str]) -> Result<(), String> {
    if let Some(o) = args.as_object() {
        for k in o.keys() {
            if !allowed.contains(&k.as_str()) {
                return Err(format!(
                    "unknown argument `{k}`. Allowed: {}",
                    allowed.join(", ")
                ));
            }
        }
    }
    Ok(())
}

fn squash(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(|c| c.to_lowercase())
        .collect()
}

/// Every named rule. (Once main's `rules::catalog()` is merged, return that: the full list.)
fn rule_catalog() -> &'static [CatalogEntry] {
    jetcarp_core::rules::catalog()
}

/// A catalog rule name ("Brian's Brain", "life") becomes its notation; anything else is kept.
pub fn resolve_rule(text: &str) -> String {
    let k = squash(text);
    let aliases = [
        ("gameoflife", "Life"),
        ("conway", "Life"),
        ("conwayslife", "Life"),
    ];
    let k = aliases
        .iter()
        .find(|a| a.0 == k)
        .map(|a| squash(a.1))
        .unwrap_or(k);
    rule_catalog()
        .iter()
        .find(|e| squash(e.name) == k)
        .map(|e| e.notation.to_string())
        .unwrap_or_else(|| text.to_string())
}

/// Seed shorthands: "random", "random:0.2", "ants:3", "pile:5000", {"random": 0.2}.
pub fn parse_seed(v: &Value) -> Result<SeedSpec, String> {
    let bad = |e: String| {
        format!(
            "bad seed {v}: {e}. Use \"empty\", \"random\", \"random:0.2\", \"center\", \"top_center\", \"random_states\", \"ants:3\", \"pile:10000\", JACA \"signed:0.3:0.5\" (density, share of -1) or \"halves:0.3\" (+1 left, -1 right), \"pattern:blobs:0.4:by_region\", {{\"shape\": \"blobs\", \"count\": 3, \"density\": 0.4}}, or {{\"random\": {{\"density\": 0.2}}}}."
        )
    };
    if let Some(s) = v.as_str() {
        let s = s.trim().to_ascii_lowercase();
        let (head, arg) = match s.split_once([':', '=', ' ']) {
            Some((h, a)) => (h.trim().to_string(), Some(a.trim().to_string())),
            None => (s.clone(), None),
        };
        let num = |d: f64| -> Result<f64, String> {
            arg.as_deref()
                .map(|a| {
                    a.trim_end_matches('%')
                        .parse::<f64>()
                        .map_err(|e| e.to_string())
                })
                .transpose()
                .map(|x| x.unwrap_or(d))
        };
        return match head.as_str() {
            "random" => {
                let mut d = num(0.3).map_err(bad)?;
                if d > 1.0 {
                    d /= 100.0;
                }
                if !(0.0..=1.0).contains(&d) {
                    return Err(bad("density must be between 0 and 1".into()));
                }
                Ok(SeedSpec::Random { density: d })
            }
            "ants" => Ok(SeedSpec::Ants {
                count: num(1.0).map_err(bad)?.clamp(1.0, 10_000.0) as u32,
            }),
            "pile" => Ok(SeedSpec::Pile {
                grains: num(10_000.0).map_err(bad)?.clamp(1.0, 1e12) as u64,
            }),
            // Lenia: a built-in creature at the centre ("lenia:orbium"), or Gaussian bumps.
            "lenia" => Ok(SeedSpec::Lenia {
                name: arg.unwrap_or_else(|| "orbium".into()),
                scale: 1.0,
            }),
            "gauss" | "gaussian" => Ok(SeedSpec::Gaussian {
                count: num(1.0).map_err(bad)?.clamp(1.0, 10_000.0) as u32,
                radius: 8.0,
                peak: 1.0,
            }),
            "signed" | "mixed" | "halves" | "half" | "pattern" => jetcarp_script::spec::parse_seed(v),
            _ => serde_json::from_value(Value::String(head)).map_err(|e| bad(e.to_string())),
        };
    }
    if v.get("shape").is_some() || v.get("pattern").is_some() {
        return jetcarp_script::spec::parse_seed(v);
    }
    if let Some(d) = v.get("random").and_then(|d| d.as_f64()) {
        return parse_seed(&json!(format!("random:{d}")));
    }
    serde_json::from_value(v.clone()).map_err(|e| bad(e.to_string()))
}

/// A CA item from new_world or add_ca: a rule string or a CaSpec-like object.
fn parse_ca(v: &Value) -> Result<CaSpec, String> {
    if let Some(s) = v.as_str() {
        return Ok(CaSpec::new(&resolve_rule(s)));
    }
    let mut o = v
        .as_object()
        .cloned()
        .ok_or_else(|| format!("a CA must be a rule string or an object, got {v}"))?;
    let rule = o
        .get("rule")
        .and_then(|r| r.as_str())
        .ok_or("a CA object needs \"rule\", e.g. {\"rule\": \"Life\"}")?;
    o.insert("rule".into(), json!(resolve_rule(rule)));
    let seed = o.remove("seed").map(|s| parse_seed(&s)).transpose()?;
    let mut spec: CaSpec =
        serde_json::from_value(Value::Object(o)).map_err(|e| format!("bad CA {v}: {e}"))?;
    if seed.is_some() {
        spec.seed = seed;
    }
    Ok(spec)
}

fn check_tiling_size(spec: &TilingSpec) -> Result<(), String> {
    match spec.size {
        Some(SizeSpec::Rect { w, h }) => {
            if w < 8 || h < 8 || w > 4096 || h > 4096 {
                return Err(format!("width and height must be 8 to 4096, got {w} x {h}"));
            }
            if w as u64 * h as u64 > MAX_CELLS {
                return Err(format!(
                    "{w} x {h} is too large; keep width x height at most {MAX_CELLS}"
                ));
            }
        }
        Some(SizeSpec::Cells(n)) if n as u64 > MAX_CELLS => {
            return Err(format!("at most {MAX_CELLS} cells"));
        }
        Some(SizeSpec::Level(l)) => {
            let hi = max_level(&spec.family);
            if !(1..=hi).contains(&l) {
                return Err(format!("level must be 1 to {hi}"));
            }
        }
        _ => {}
    }
    Ok(())
}

/// Largest aperiodic level served over MCP: the family's largest with at most [`MAX_CELLS`]
/// tiles (Spectre 7, hat and Penrose 6).
fn max_level(family: &str) -> u32 {
    let (lo, hi) = jetcarp_core::aperiodic::level_range(family);
    (lo..=hi)
        .rev()
        .find(|&l| {
            jetcarp_core::aperiodic::tile_count(family, l).is_none_or(|n| n as u64 <= MAX_CELLS)
        })
        .unwrap_or(lo)
}

fn tiling_from_args(args: &Value) -> Result<TilingSpec, String> {
    let id = string(args, "tiling")?.unwrap_or("square").trim();
    let low = id.to_ascii_lowercase();
    let mut spec = if families::family(&low).is_some() {
        TilingSpec::new(&low)
    } else if catalog::get(&low).is_some() {
        let mut s = TilingSpec::new("periodic");
        s.params = Some(json!({"catalog": low}));
        s
    } else {
        let alias = families::alias(&low);
        match alias {
            Some(a) => TilingSpec::new(a),
            None => {
                let ids: Vec<&str> = families::FAMILIES.iter().map(|f| f.id).collect();
                return Err(format!(
                    "unknown tiling '{id}'. Tiling ids: {}. See list_tilings.",
                    ids.join(", ")
                ));
            }
        }
    };
    let fam = families::family(&spec.family).unwrap();
    let aperiodic = fam.kind == families::FamilyKind::Aperiodic;
    let (w, h, cells, level) = (
        int(args, "width")?,
        int(args, "height")?,
        int(args, "cells")?,
        int(args, "level")?,
    );
    spec.size = match (w, h, cells, level) {
        (Some(_), None, _, _) | (None, Some(_), _, _) => {
            return Err("give both width and height".into());
        }
        (Some(_), Some(_), Some(_), _) => return Err("give width/height or cells, not both".into()),
        (Some(_), Some(_), _, _) if aperiodic => {
            return Err(format!(
                "{} is aperiodic: give `level` (1-{}) instead of width/height",
                fam.id,
                max_level(fam.id)
            ));
        }
        (Some(w), Some(h), None, _) => {
            if !(8..=4096).contains(&w) || !(8..=4096).contains(&h) {
                return Err(format!("width and height must be 8 to 4096, got {w} x {h}"));
            }
            Some(SizeSpec::Rect {
                w: w as u32,
                h: h as u32,
            })
        }
        (None, None, Some(c), _) => {
            if !(64..=MAX_CELLS as i64).contains(&c) {
                return Err(format!("cells must be 64 to {MAX_CELLS}"));
            }
            Some(SizeSpec::Cells(c as u32))
        }
        (None, None, None, Some(l)) => {
            if !aperiodic {
                return Err(format!(
                    "`level` is only for aperiodic tilings; {} takes width/height or cells",
                    fam.id
                ));
            }
            let hi = max_level(fam.id) as i64;
            if !(1..=hi).contains(&l) {
                return Err(format!("level must be 1 to {hi}"));
            }
            Some(SizeSpec::Level(l as u32))
        }
        (None, None, None, None) if aperiodic => None,
        (None, None, None, None) => Some(SizeSpec::Cells(4096)),
    };
    spec.boundary = match string(args, "boundary")?.map(|s| s.to_ascii_lowercase()) {
        None => Boundary::Wrap,
        Some(b) => match b.as_str() {
            "wrap" | "torus" | "wrapped" | "periodic" => Boundary::Wrap,
            "dead" | "none" | "empty" => Boundary::Dead,
            "live" | "alive" => Boundary::Live,
            _ => return Err(format!("boundary must be wrap, dead, or live, got '{b}'")),
        },
    };
    if aperiodic {
        spec.boundary = Boundary::Dead;
    }
    if let Some(p) = args.get("tiling_params").filter(|p| !p.is_null()) {
        if !p.is_object() {
            return Err("tiling_params must be an object".into());
        }
        let mut merged = spec.params.take().unwrap_or(json!({}));
        for (k, v) in p.as_object().unwrap() {
            merged[k] = v.clone();
        }
        spec.params = Some(merged);
    }
    Ok(spec)
}

/// Index into `world.cas` for a CA argument (id, name, or omitted for the first).
pub(crate) fn ca_index(w: &World, v: Option<&Value>) -> Result<usize, String> {
    if w.cas.is_empty() {
        return Err("this world has no CAs yet. Create a world with new_world (e.g. cas: [\"Life\"]) or add one with apply_commands add_ca.".into());
    }
    let list = || {
        w.cas
            .iter()
            .map(|c| format!("id {} '{}' ({})", c.id, c.name, c.notation()))
            .collect::<Vec<_>>()
            .join(", ")
    };
    let found = match v {
        None | Some(Value::Null) => Some(0),
        Some(Value::Number(n)) => n
            .as_u64()
            .and_then(|id| w.cas.iter().position(|c| c.id as u64 == id)),
        Some(Value::String(s)) => {
            let s = s.trim();
            s.parse::<u32>()
                .ok()
                .and_then(|id| w.cas.iter().position(|c| c.id == id))
                .or_else(|| w.cas.iter().position(|c| c.name.eq_ignore_ascii_case(s)))
                .or_else(|| w.cas.iter().position(|c| squash(&c.name) == squash(s)))
                .or_else(|| {
                    let n = resolve_rule(s).to_ascii_uppercase();
                    w.cas
                        .iter()
                        .position(|c| c.notation().to_ascii_uppercase() == n)
                })
        }
        Some(_) => None,
    };
    found.ok_or_else(|| format!("no CA {}. CAs: {}", v.unwrap(), list()))
}

fn nb_name(nb: Nb) -> &'static str {
    match nb {
        Nb::Touch => "touch",
        Nb::Edge => "edge",
    }
}

fn ca_value(c: &CaInstance) -> Value {
    let mut v = json!({
        "id": c.id, "name": c.name, "rule": c.notation(), "enabled": c.enabled,
        "field": c.field, "neighborhood": nb_name(c.nb), "affects_others": c.affects_others,
        "steps_per_gen": c.steps_per_gen, "palette": c.palette,
    });
    if c.rule.is_agent() {
        v["ants"] = json!(c.agents.len());
    }
    if matches!(c.rule, jetcarp_core::rules::Rule::Jaca(_)) {
        v["palette_neg"] = json!(c.palette_neg);
        let h = c.heat.as_deref().cloned().unwrap_or_default();
        v["annihilation"] = json!({
            "last_touching": h.last.contacts, "last_destroyed": h.last.destroyed,
            "last_cancelled_births": h.last.cancelled,
            "total_touching": h.total_contacts, "total_destroyed": h.total_destroyed,
        });
    }
    v
}

fn boundary_name(t: &Topology) -> &'static str {
    if t.wraps() {
        "wrap"
    } else if t.live_edges() {
        "live"
    } else {
        "dead"
    }
}

/// The `world_status` report.
pub fn status(host: &dyn Host) -> Value {
    let w = host.world();
    let t = &w.topo;
    let grid = Grid::of(t).map(|g| {
        json!({
            "width": g.w, "height": g.h, "tile_types_per_unit_cell": g.types,
            "coordinates": "x = column 0..width-1, y = row 0..height-1 from the top; index = y * width + x",
        })
    });
    let fields: Vec<Value> = w
        .fields
        .iter()
        .enumerate()
        .map(|(i, f)| {
            let users: Vec<u32> = w
                .cas
                .iter()
                .filter(|c| c.field == i)
                .map(|c| c.id)
                .collect();
            let mut v = json!({
                "field": i, "kind": f.kind.name(), "shared": f.shared,
                "population": f.population(), "ca_ids": users,
            });
            if f.kind == FieldKind::Height {
                v["population_means"] = json!("total grains");
                v["grains_lost_off_edges"] = json!(f.lost);
                v["stable"] = json!(sandpile_stable(w, i));
            } else {
                v["max_state"] = json!(f.max_state());
            }
            if w.cas.iter().any(|c| c.field == i && matches!(c.rule, jetcarp_core::rules::Rule::Jaca(_))) {
                let count = |s: u8| f.state.iter().filter(|&&x| x == s).count();
                v["positive"] = json!(count(jetcarp_core::jaca::POS));
                v["negative"] = json!(count(jetcarp_core::jaca::NEG));
            }
            v
        })
        .collect();
    let script = w.script.as_ref().map(|s| {
        let st = &w.script_state;
        json!({"steps": s.steps.len(), "current_step": st.step,
               "current_name": s.steps.get(st.step).map(|x| x.name.clone()), "finished": st.finished})
    });
    json!({
        "generation": w.generation,
        "status": status_text(w),
        "done": w.done_reason().unwrap_or_else(|| "no: something can still change".into()),
        "hash": w.hash_hex(),
        "hash_note": "`hash` covers the generation number and cell ages too, so it never repeats; `cells_hash` covers only the cell states: equal cells_hash = same picture. For periods, trust `status` (an exact check).",
        "cells_hash": format!("{:016x}", w.states_hash()),
        "cells": t.len(),
        "tiling": {
            "id": t.family.id, "name": t.family.name, "boundary": boundary_name(t),
            "grid": grid.unwrap_or(json!("none: aperiodic patch, address cells by index")),
            "description": t.describe(),
        },
        "cas": w.cas.iter().map(ca_value).collect::<Vec<_>>(),
        "fields": fields,
        "script": script,
        "host": host.info(),
    })
}

/// A height field is stable when every cell (sinks aside) holds fewer grains than it has
/// neighbours, so nothing can topple.
fn sandpile_stable(w: &World, field: usize) -> bool {
    let Some(ca) = w.cas.iter().find(|c| c.field == field) else {
        return true;
    };
    let (t, f) = (&w.topo, &w.fields[field]);
    (0..t.len()).all(|i| t.is_open(i) || (f.height[i] as usize) < t.degree(i, ca.nb))
}

/// The world's status in words. A world whose cells are all dead reads "Empty" even when the
/// exact repeat check has also seen it (an empty world is trivially frozen).
fn status_text(w: &World) -> String {
    let dead = w.fields.iter().all(|f| f.is_empty()) && w.cas.iter().all(|c| c.agents.is_empty());
    match w.status() {
        Status::Repeats { seen_at, .. } if dead => {
            format!("Empty: every cell is dead (died out by generation {seen_at})")
        }
        Status::Empty => "Empty: every cell is dead".into(),
        s => s.to_string(),
    }
}

pub(crate) fn short_status(w: &World) -> String {
    let pops: Vec<String> = w
        .fields
        .iter()
        .enumerate()
        .map(|(i, f)| {
            let users: Vec<String> = w
                .cas
                .iter()
                .filter(|c| c.field == i)
                .map(|c| format!("{} (id {})", c.name, c.id))
                .collect();
            let what = if f.kind == FieldKind::Height {
                if sandpile_stable(w, i) {
                    "grains (stable: no cell can topple)"
                } else {
                    "grains (not yet stable: cells will topple when stepped)"
                }
            } else {
                "live"
            };
            format!(
                "field {i} [{}]: {} {what}",
                users.join(", "),
                f.population()
            )
        })
        .collect();
    format!(
        "Generation {}. Status: {}. Hash {} (with generation and ages); cells hash {} (cell states only: equal when the cells repeat).\n{}",
        w.generation,
        status_text(w),
        w.hash_hex(),
        format_args!("{:016x}", w.states_hash()),
        pops.join("\n")
    )
}

// ---------------------------------------------------------------------------------------------
// Tools

fn list_rules(args: &Value) -> Res {
    check_unknown(args, &["family"])?;
    let fam = string(args, "family")?.map(squash);
    let all = rule_catalog();
    let mut families: Vec<&str> = Vec::new();
    for e in all {
        if !families.contains(&e.family) {
            families.push(e.family);
        }
    }
    let picked: Vec<_> = all
        .iter()
        .filter(|e| {
            fam.as_ref()
                .is_none_or(|f| squash(e.family).contains(f.as_str()))
        })
        .collect();
    if picked.is_empty() {
        return Err(format!(
            "no rules in that family. Families: {}",
            families.join(", ")
        ));
    }
    let mut s = String::from("Named rules (use the name or the notation as `rule`):\n");
    if picked.len() <= 80 {
        s.push('\n');
        for e in &picked {
            let _ = writeln!(
                s,
                "- {} | {} | {} | {}",
                e.name, e.notation, e.family, e.blurb
            );
        }
    } else {
        // Long lists: names and notation only, grouped by family.
        s.push_str("(Too many to describe each; pass `family` for one-line descriptions.)\n");
        for f in &families {
            let names: Vec<String> = picked
                .iter()
                .filter(|e| e.family == *f)
                .map(|e| format!("{} ({})", e.name, e.notation))
                .collect();
            if !names.is_empty() {
                let _ = writeln!(s, "\n{f}: {}", names.join("; "));
            }
        }
    }
    s.push_str(NOTATION);
    Ok(ToolOutput::text(s))
}

fn list_tilings(args: &Value) -> Res {
    check_unknown(args, &["search"])?;
    let Some(q) = string(args, "search")?.map(|q| q.trim().to_ascii_lowercase()) else {
        return Ok(ToolOutput::text(tilings_text()));
    };
    let mut hits: Vec<String> = families::FAMILIES
        .iter()
        .filter(|f| f.id.contains(&q) || f.name.to_ascii_lowercase().contains(&q))
        .map(|f| format!("- {} | {} | {}", f.id, f.name, f.group))
        .collect();
    hits.extend(
        catalog::all()
            .iter()
            .filter(|t| families::family(&t.id).is_none())
            .filter(|t| {
                t.id.to_ascii_lowercase().contains(&q) || t.name.to_ascii_lowercase().contains(&q)
            })
            .map(|t| format!("- {} | {} | {}", t.id, t.name, t.group)),
    );
    hits.extend(
        jetcarp_core::isohedral::IH_FAMILIES
            .iter()
            .filter(|f| f.id.contains(&q) || f.name.to_ascii_lowercase().contains(&q))
            .map(|f| format!("- {} | {} | isohedral", f.id, f.name)),
    );
    let n = hits.len();
    let mut s = format!("{n} tilings match \"{q}\"");
    if n > 60 {
        let _ = write!(s, " (first 60 shown; search for something narrower)");
    }
    s.push_str(":\n");
    for h in hits.into_iter().take(60) {
        s.push_str(&h);
        s.push('\n');
    }
    Ok(ToolOutput::text(s))
}

fn tilings_text() -> String {
    use std::sync::OnceLock;
    static TEXT: OnceLock<String> = OnceLock::new();
    TEXT.get_or_init(|| {
        let mut s = String::from(
            "Tilings (id | name | group | neighbours per cell: edge-sharing / touching | note).\n\
             Size: width x height unit cells, or `cells` (about that many); aperiodic tilings use `level`.\n\
             Life-like rules count touching neighbours by default (Moore on squares: 8); add V to a B/S rule \
             (B3/S23V) or neighborhood \"edge\" to count edge neighbours only.\n\n",
        );
        for f in families::FAMILIES {
            let deg = match Topology::build(&TilingSpec::new(f.id)) {
                Ok(t) => {
                    let (e, to) = t.degree_sets();
                    format!("{e:?} / {to:?}")
                }
                Err(_) => "?".into(),
            };
            let _ = writeln!(s, "- {} | {} | {} | {} | {}", f.id, f.name, f.group, deg, f.note);
        }
        // The catalog is large (over 1,300 entries): summarise it and point to `search`.
        let extra: Vec<_> = catalog::all()
            .iter()
            .filter(|t| families::family(&t.id).is_none())
            .collect();
        let kuni = extra.iter().filter(|t| t.k > 0 && t.id.starts_with('k')).count();
        let _ = writeln!(
            s,
            "\nAlso usable as `tiling` (list them with list_tilings {{\"search\": \"...\"}}): {kuni} k-uniform \
             tilings k1-001 .. k6-673 (ids like \"k2-005\"; search \"k2-\" for all 2-uniform ones), {} other \
             catalog tilings, and the {} isohedral tile types ih01 .. ih93.",
            extra.len() - kuni,
            jetcarp_core::isohedral::IH_FAMILIES.len()
        );
        s
    })
    .clone()
}

fn new_world(host: &mut dyn Host, args: &Value) -> Res {
    check_unknown(
        args,
        &[
            "tiling",
            "width",
            "height",
            "cells",
            "level",
            "boundary",
            "cas",
            "rng_seed",
            "name",
            "tiling_params",
        ],
    )?;
    let tiling = tiling_from_args(args)?;
    let mut scene = Scene::new(tiling);
    scene.rng_state = int(args, "rng_seed")?.unwrap_or(1).max(0) as u64;
    scene.name = string(args, "name")?.unwrap_or("").to_string();
    match args.get("cas") {
        None | Some(Value::Null) => {}
        Some(Value::Array(items)) => {
            for it in items {
                scene.cas.push(parse_ca(it)?);
            }
        }
        Some(Value::String(s)) => scene.cas.push(CaSpec::new(&resolve_rule(s))),
        Some(v) => return Err(format!("cas must be a list, got {v}")),
    }
    if scene.cas.len() > 32 {
        return Err("at most 32 CAs".into());
    }
    host.replace(scene)?;
    let mut out = ToolOutput::text(format!(
        "New world created.\n{}",
        serde_json::to_string_pretty(&status(host)).unwrap_or_default()
    ));
    if host.world().cas.is_empty() {
        out.content.push(json!({"type": "text", "text": "No CAs yet: add one with apply_commands {\"op\":\"add_ca\",\"ca\":{\"rule\":\"Life\"}}."}));
    }
    Ok(out)
}

fn field_of(w: &World, i: usize) -> &jetcarp_core::world::Field {
    &w.fields[w.cas[i].field]
}

fn set_cells(host: &mut dyn Host, args: &Value) -> Res {
    check_unknown(
        args,
        &[
            "ca",
            "pattern",
            "rle",
            "x",
            "y",
            "cells",
            "indices",
            "state",
            "clear_first",
            "erase",
        ],
    )?;
    let w = host.world();
    let ci = ca_index(w, args.get("ca"))?;
    let (id, ca_name) = (w.cas[ci].id, w.cas[ci].name.clone());
    let field = field_of(w, ci);
    let height = field.kind == FieldKind::Height;
    let max_state = if height {
        u32::MAX
    } else {
        field.max_state() as u32
    };
    let n = w.topo.len();
    let grid = Grid::of(&w.topo);
    let erase = boolean(args, "erase")?;
    let clear_first = boolean(args, "clear_first")?;
    if erase && height {
        return Err("erase does not apply to sandpile grains; use apply_commands {\"op\":\"clear\",\"id\":..} to empty the field".into());
    }
    let default_state = int(args, "state")?.unwrap_or(1);
    if default_state < 0 {
        return Err("state must be 0 or more".into());
    }
    let need_grid = || {
        grid.ok_or_else(|| {
            "this tiling is an aperiodic patch with no x/y grid; give `indices` instead (see inspect_cell)".to_string()
        })
    };
    let mut writes: Vec<[u32; 2]> = Vec::new();
    let mut given = 0;
    let pattern = match (string(args, "pattern")?, string(args, "rle")?) {
        (Some(_), Some(_)) => return Err("give pattern or rle, not both".into()),
        (Some(p), None) => Some(cells::parse_plaintext(p)?),
        (None, Some(r)) => Some(cells::parse_rle(r)?),
        (None, None) => None,
    };
    let mut placed = None;
    if let Some(p) = pattern {
        given += 1;
        let g = need_grid()?;
        if p.is_empty() {
            return Err("the pattern has no live cells (use 'O' for live, '.' for empty)".into());
        }
        let pw = p.iter().map(|c| c.0).max().unwrap() + 1;
        let ph = p.iter().map(|c| c.1).max().unwrap() + 1;
        let ox = int(args, "x")?.unwrap_or((g.w as i64 - pw) / 2);
        let oy = int(args, "y")?.unwrap_or((g.h as i64 - ph) / 2);
        if p.len() > MAX_SET {
            return Err(format!("pattern has more than {MAX_SET} cells"));
        }
        for (dx, dy, v) in &p {
            let i = g.index(ox + dx, oy + dy)?;
            writes.push([i as u32, if erase { 0 } else { *v }]);
        }
        placed = Some((ox, oy, pw, ph));
    } else if args.get("x").is_some_and(|v| !v.is_null())
        || args.get("y").is_some_and(|v| !v.is_null())
    {
        return Err(
            "x and y place a `pattern` or `rle`; for single cells use cells: [[x, y]]".into(),
        );
    }
    if let Some(list) = args.get("cells").filter(|v| !v.is_null()) {
        given += 1;
        let g = need_grid()?;
        let items = list
            .as_array()
            .ok_or("cells must be a list like [[x, y], [x, y, state]]")?;
        if items.len() > MAX_SET {
            return Err(format!("at most {MAX_SET} cells per call"));
        }
        for it in items {
            let xy: Option<Vec<i64>> = match it {
                Value::Array(a) => a.iter().map(|v| v.as_i64()).collect(),
                Value::Object(o) => ["x", "y", "state"]
                    .iter()
                    .filter_map(|k| o.get(*k))
                    .map(|v| v.as_i64())
                    .collect(),
                _ => None,
            };
            let xy = xy
                .filter(|v| v.len() == 2 || v.len() == 3)
                .ok_or_else(|| format!("each cell must be [x, y] or [x, y, state], got {it}"))?;
            let v = if erase {
                0
            } else {
                xy.get(2).copied().unwrap_or(default_state)
            };
            if v < 0 {
                return Err(format!("negative state in {it}"));
            }
            writes.push([g.index(xy[0], xy[1])? as u32, v as u32]);
        }
    }
    if let Some(list) = args.get("indices").filter(|v| !v.is_null()) {
        given += 1;
        let items = list.as_array().ok_or("indices must be a list")?;
        if items.len() > MAX_SET {
            return Err(format!("at most {MAX_SET} cells per call"));
        }
        for it in items {
            let (i, v) = match it {
                Value::Number(_) => (it.as_i64(), Some(default_state)),
                Value::Array(a) if a.len() == 2 => (a[0].as_i64(), a[1].as_i64()),
                _ => (None, None),
            };
            let (Some(i), Some(v)) = (i, v) else {
                return Err(format!(
                    "each index must be a number or [index, state], got {it}"
                ));
            };
            if i < 0 || i as usize >= n || v < 0 {
                return Err(format!("cell index {i} is out of range (0..{})", n - 1));
            }
            writes.push([i as u32, if erase { 0 } else { v as u32 }]);
        }
    }
    if given == 0 && !clear_first {
        return Err(
            "nothing to draw: give pattern, rle, cells, or indices (or clear_first: true)".into(),
        );
    }
    if let Some(bad) = writes.iter().find(|c| c[1] > max_state) {
        return Err(format!(
            "state {} is too high: CA '{ca_name}' has states 0 to {max_state}",
            bad[1]
        ));
    }
    if clear_first {
        host.exec(Command::Clear { id })?;
    }
    let count = writes.len();
    if !writes.is_empty() {
        host.exec(Command::SetCells { id, cells: writes })?;
    }
    let w = host.world();
    let mut s = format!(
        "{} {count} cell(s) in CA '{ca_name}' (id {id}){}.",
        if erase { "Erased" } else { "Set" },
        if clear_first {
            " after clearing its field"
        } else {
            ""
        }
    );
    if let Some((ox, oy, pw, ph)) = placed {
        let _ = write!(
            s,
            " Pattern box: x {ox}..{}, y {oy}..{}.",
            ox + pw - 1,
            oy + ph - 1
        );
    }
    let _ = write!(
        s,
        " Field population now {}. Generation {}.",
        field_of(w, ci).population(),
        w.generation
    );
    Ok(ToolOutput::text(s))
}

fn read_cells(host: &mut dyn Host, args: &Value) -> Res {
    check_unknown(args, &["ca", "x", "y", "width", "height", "format"])?;
    let w = host.world();
    let ci = ca_index(w, args.get("ca"))?;
    let ca = &w.cas[ci];
    let f = field_of(w, ci);
    let binary = f.kind == FieldKind::Binary;
    let n = w.topo.len();
    let list_mode = match string(args, "format")? {
        None | Some("picture") => false,
        Some("list") => true,
        Some(o) => return Err(format!("format must be picture or list, got {o}")),
    };
    let mut s = format!(
        "CA '{}' (id {}, {}), field {}, generation {}.\n",
        ca.name,
        ca.id,
        ca.notation(),
        ca.field,
        w.generation
    );
    let nonzero: Vec<usize> = (0..n).filter(|&i| cells::value(f, i) != 0).collect();
    let agents = |s: &mut String, g: Option<Grid>| {
        if ca.rule.is_agent() {
            let a: Vec<String> = ca
                .agents
                .iter()
                .map(|a| match g {
                    Some(g) => {
                        let (x, y) = g.xy(a.cell as usize);
                        format!("({x}, {y})")
                    }
                    None => format!("cell {}", a.cell),
                })
                .collect();
            let _ = writeln!(s, "Ants at: {}", a.join(", "));
        }
    };
    let Some(g) = Grid::of(&w.topo) else {
        // Aperiodic patch: list cells with centre points.
        let _ = writeln!(
            s,
            "Aperiodic tiling: no grid. {} non-empty of {n} cells.",
            nonzero.len()
        );
        for &i in nonzero.iter().take(500) {
            let c = w.topo.centroid(i);
            let _ = writeln!(
                s,
                "index {i}: {} at ({:.2}, {:.2})",
                cells::value(f, i),
                c[0],
                c[1]
            );
        }
        if nonzero.len() > 500 {
            s.push_str("... (first 500 shown)\n");
        }
        agents(&mut s, None);
        return Ok(ToolOutput::text(s));
    };
    let bbox = (!nonzero.is_empty()).then(|| {
        let mut b = (usize::MAX, usize::MAX, 0, 0);
        for &i in &nonzero {
            let (x, y) = g.xy(i);
            b = (b.0.min(x), b.1.min(y), b.2.max(x), b.3.max(y));
        }
        b
    });
    let _ = write!(
        s,
        "Grid {} x {}. Non-empty cells in the whole field: {}",
        g.w,
        g.h,
        nonzero.len()
    );
    if let Some(b) = bbox {
        let _ = write!(
            s,
            "; bounding box x {}..{}, y {}..{} ({} x {})",
            b.0,
            b.2,
            b.1,
            b.3,
            b.2 - b.0 + 1,
            b.3 - b.1 + 1
        );
        if g.wrap && (b.2 - b.0 + 1 == g.w || b.3 - b.1 + 1 == g.h) {
            s.push_str(" (spans a wrapped edge or the whole grid)");
        }
    }
    s.push_str(".\n");
    agents(&mut s, Some(g));
    let (x, y, rw, rh) = match (int(args, "x")?, int(args, "y")?) {
        (None, None) if args.get("width").is_none() && args.get("height").is_none() => {
            let Some(b) = bbox else {
                s.push_str("The field is empty.\n");
                return Ok(ToolOutput::text(s));
            };
            let x0 = b.0.saturating_sub(1);
            let y0 = b.1.saturating_sub(1);
            let x1 = (b.2 + 1).min(g.w - 1);
            let y1 = (b.3 + 1).min(g.h - 1);
            (x0, y0, x1 - x0 + 1, y1 - y0 + 1)
        }
        (x, y) => {
            let x = x.unwrap_or(0);
            let y = y.unwrap_or(0);
            if x < 0 || y < 0 || x as usize >= g.w || y as usize >= g.h {
                return Err(format!(
                    "x, y must be inside the grid (0..{}, 0..{})",
                    g.w - 1,
                    g.h - 1
                ));
            }
            let rw = int(args, "width")?.unwrap_or(40).clamp(1, 200) as usize;
            let rh = int(args, "height")?.unwrap_or(40).clamp(1, 200) as usize;
            let (x, y) = (x as usize, y as usize);
            (x, y, rw.min(g.w - x), rh.min(g.h - y))
        }
    };
    if list_mode {
        let mut items = Vec::new();
        for yy in y..y + rh {
            for xx in x..x + rw {
                let v = cells::value(f, yy * g.w + xx);
                if v != 0 {
                    items.push(json!([xx, yy, v]));
                }
            }
        }
        let total = items.len();
        items.truncate(4000);
        let _ = writeln!(
            s,
            "Non-empty cells in x {}..{}, y {}..{} as [x, y, state] ({total}{}):\n{}",
            x,
            x + rw - 1,
            y,
            y + rh - 1,
            if total > 4000 { ", first 4000" } else { "" },
            Value::Array(items)
        );
        return Ok(ToolOutput::text(s));
    }
    if rw * rh > MAX_PICTURE {
        return Err(format!(
            "{s}The region x {x}..{}, y {y}..{} ({rw} x {rh}) is too large to print. Give x, y, width, height for a region of at most {MAX_PICTURE} cells (e.g. 100 x 100), or use format \"list\".",
            x + rw - 1,
            y + rh - 1
        ));
    }
    let _ = writeln!(
        s,
        "Region x {}..{}, y {}..{}. '.' = empty, {}. Row 0 is the top.",
        x,
        x + rw - 1,
        y,
        y + rh - 1,
        if binary {
            "'O' = live"
        } else if f.kind == FieldKind::Height {
            "digits/letters = grains (a = 10, ..., # = 36 or more)"
        } else {
            "digits = state (a = 10, ...)"
        }
    );
    // Ruler: tens digit line (when wide) and units digit line.
    let tens: String = (x..x + rw)
        .map(|c| {
            if c % 10 == 0 {
                char::from(b'0' + ((c / 10) % 10) as u8)
            } else {
                ' '
            }
        })
        .collect();
    let units: String = (x..x + rw)
        .map(|c| char::from(b'0' + (c % 10) as u8))
        .collect();
    if rw > 10 {
        let _ = writeln!(s, "x tens  {tens}");
    }
    let _ = writeln!(s, "x units {units}");
    for yy in y..y + rh {
        let row: String = (x..x + rw)
            .map(|xx| cells::state_char(cells::value(f, yy * g.w + xx), binary))
            .collect();
        let _ = writeln!(s, "y={yy:<5} {row}");
    }
    Ok(ToolOutput::text(s))
}

fn inspect_cell(host: &mut dyn Host, args: &Value) -> Res {
    check_unknown(args, &["x", "y", "index"])?;
    let w = host.world();
    let t = &w.topo;
    let g = Grid::of(t);
    let i = match (int(args, "index")?, int(args, "x")?, int(args, "y")?) {
        (Some(i), None, None) => {
            if i < 0 || i as usize >= t.len() {
                return Err(format!("index must be 0 to {}", t.len() - 1));
            }
            i as usize
        }
        (None, Some(x), Some(y)) => g
            .ok_or("this tiling has no x/y grid; give `index`")?
            .index(x, y)?,
        _ => return Err("give x and y, or index".into()),
    };
    let at = |j: usize| match g {
        Some(g) => {
            let (x, y) = g.xy(j);
            json!({"x": x, "y": y, "index": j})
        }
        None => json!({"index": j}),
    };
    let nbs = |nb: Nb| -> Vec<Value> {
        (0..t.degree(i, nb))
            .map(|k| t.slot(i, nb, k).map(at).unwrap_or(json!("past the edge")))
            .collect()
    };
    let c = t.centroid(i);
    let values: Vec<Value> = w
        .fields
        .iter()
        .enumerate()
        .map(|(fi, f)| json!({"field": fi, "kind": f.kind.name(), "value": cells::value(f, i)}))
        .collect();
    let mut v = at(i);
    v["tile_type"] = json!(g.map(|g| i % g.types).unwrap_or(t.shape_of(i)));
    v["corners"] = json!(t.polygon(i).len());
    v["center"] = json!([
        (c[0] * 1000.0).round() / 1000.0,
        (c[1] * 1000.0).round() / 1000.0
    ]);
    v["values"] = json!(values);
    v["edge_neighbors_clockwise"] = json!(nbs(Nb::Edge));
    v["touch_neighbors_clockwise"] = json!(nbs(Nb::Touch));
    Ok(ToolOutput::json(&v))
}

fn work_check(w: &World, gens: u64) -> Result<(), String> {
    if gens > MAX_GENS {
        return Err(format!("at most {MAX_GENS} generations per call"));
    }
    let work = gens.saturating_mul(w.topo.len() as u64);
    if work > MAX_WORK {
        let ok = (MAX_WORK / w.topo.len().max(1) as u64).max(1);
        return Err(format!(
            "{gens} generations on {} cells is too much for one call; run at most {ok} at a time",
            w.topo.len()
        ));
    }
    Ok(())
}

fn step(host: &mut dyn Host, args: &Value, cancel: Option<&std::sync::atomic::AtomicBool>) -> Res {
    let cancelled = || cancel.is_some_and(|c| c.load(std::sync::atomic::Ordering::Relaxed));
    check_unknown(args, &["generations", "stop_when_settled"])?;
    let settle = boolean(args, "stop_when_settled")?;
    let gens = int(args, "generations")?.unwrap_or(if settle { SETTLE_GENS } else { 1 });
    if gens < 1 {
        return Err("generations must be at least 1".into());
    }
    let gens = gens as u64;
    work_check(host.world(), gens)?;
    if host.world().cas.iter().all(|c| !c.enabled) {
        return Err("no CA is enabled, so nothing would change. Enable one with apply_commands update_ca {\"enabled\": true}, or add one.".into());
    }
    let g0 = host.world().generation;
    let t0 = std::time::Instant::now();
    if settle {
        for _ in 0..gens {
            host.exec(Command::Step { gens: 1 })?;
            if host.world().status() != Status::Running || cancelled() {
                break;
            }
        }
    } else {
        // In chunks of about a second's work, so a cancel stops it.
        let cells = host.world().topo.len().max(1) as u64;
        let chunk = (50_000_000 / cells).clamp(1, gens);
        let mut left = gens;
        while left > 0 && !cancelled() {
            let n = chunk.min(left);
            host.exec(Command::Step { gens: n })?;
            left -= n;
        }
    }
    let w = host.world();
    let ran = w.generation - g0;
    let dead = w.fields.iter().all(|f| f.is_empty()) && w.cas.iter().all(|c| c.agents.is_empty());
    let hint = if dead {
        "\nThe world has died out: nothing will change from here. Some rules die out on some tilings \
         from any random start (other seeds usually behave the same). If the task asked for this run, \
         report that it died out; otherwise try another rule, tiling, or neighbourhood."
    } else {
        ""
    };
    Ok(ToolOutput::text(format!(
        "Ran {ran} generation(s) in {:.2}s{}.\n{}{hint}",
        t0.elapsed().as_secs_f64(),
        if cancelled() && ran < gens {
            " (cancelled)"
        } else if settle && ran < gens {
            " (stopped early: settled)"
        } else {
            ""
        },
        short_status(w)
    )))
}

fn render_image(host: &mut dyn Host, args: &Value) -> Res {
    check_unknown(args, &["max_side", "outlines", "save_path"])?;
    let side = int(args, "max_side")?.unwrap_or(640);
    if !(64..=2048).contains(&side) {
        return Err("max_side must be 64 to 2048".into());
    }
    let mut opts = RenderOptions {
        max_side: side as u32,
        ..Default::default()
    };
    match args.get("outlines") {
        Some(Value::Bool(true)) => opts.outline_min_px = 2.0,
        Some(Value::Bool(false)) => opts.outline_min_px = 0.0,
        _ => {}
    }
    let w = host.world();
    let img = render::render(w, &opts);
    let r = w.record();
    let bytes = util::png_bytes(
        &img,
        &[
            ("jetcarp:version", r.jetcarp_version.clone()),
            ("jetcarp:generation", r.generation.to_string()),
            ("jetcarp:hash", r.hash.clone()),
        ],
    )?;
    let mut text = format!(
        "{} x {} px picture of generation {} ({}, {} cells; hash {}).",
        img.width,
        img.height,
        w.generation,
        w.topo.family.name,
        w.topo.len(),
        r.hash
    );
    if let Some(p) = string(args, "save_path")? {
        let path = output_path(p)?;
        render::write_png(&path, &img, &render::png_metadata(w))?;
        let _ = write!(text, " Saved to {}.", path.display());
    }
    Ok(ToolOutput {
        content: vec![
            json!({"type": "text", "text": text}),
            json!({"type": "image", "data": util::base64(&bytes), "mimeType": "image/png"}),
        ],
        is_error: false,
    })
}

/// Rule-name and seed shorthands inside raw commands, plus size limits.
fn prepare_command(v: &Value, w: &World) -> Result<Command, String> {
    let mut v = v.clone();
    let op = v
        .get("op")
        .and_then(|o| o.as_str())
        .unwrap_or("")
        .to_string();
    if op.is_empty() {
        return Err(format!(
            "command {v} has no \"op\". Ops: set_tiling, set_rng_state, reset, add_ca, remove_ca, update_ca, seed, clear, set_cells, step, add_table, set_script, script_goto, set_palette, sandpile, reseed, set_reseed"
        ));
    }
    if op == "add_ca"
        && let Some(ca) = v.get("ca")
    {
        let spec = parse_ca(ca)?;
        v["ca"] = serde_json::to_value(spec).map_err(|e| e.to_string())?;
    }
    if let Some(r) = v.pointer("/change/rule").and_then(|r| r.as_str()) {
        v["change"]["rule"] = json!(resolve_rule(r));
    }
    for ptr in ["/seed", "/change/seed"] {
        if let Some(s) = v.pointer(ptr).cloned() {
            let seed = serde_json::to_value(parse_seed(&s)?).map_err(|e| e.to_string())?;
            *v.pointer_mut(ptr).unwrap() = seed;
        }
    }
    if op == "sandpile"
        && let Some(a) = v.get("action").and_then(|a| a.as_str())
    {
        v["action"] = json!(a.to_ascii_lowercase());
    }
    let cmd: Command = serde_json::from_value(v.clone()).map_err(|e| {
        format!("could not read command {v}: {e}. See the apply_commands description for the command shapes.")
    })?;
    match &cmd {
        Command::Step { gens } => work_check(w, *gens)?,
        Command::SetTiling { tiling } => check_tiling_size(tiling)?,
        Command::SetCells { cells, .. } if cells.len() > MAX_SET => {
            return Err(format!("at most {MAX_SET} cells per set_cells"));
        }
        Command::Sandpile {
            action: jetcarp_core::world::SandpileOp::AddRandom { count },
            ..
        } if *count > 100_000_000 => {
            return Err("add_random count must be at most 100,000,000".into());
        }
        _ => {}
    }
    Ok(cmd)
}

fn apply_commands(host: &mut dyn Host, args: &Value) -> Res {
    check_unknown(args, &["commands"])?;
    let list =
        match args.get("commands") {
            Some(Value::Array(a)) => a.clone(),
            Some(o @ Value::Object(_)) => vec![o.clone()],
            Some(Value::String(s)) => match serde_json::from_str::<Value>(s) {
                Ok(Value::Array(a)) => a,
                Ok(o @ Value::Object(_)) => vec![o],
                _ => return Err("commands must be a list of command objects".into()),
            },
            _ => return Err(
                "commands must be a list of command objects, e.g. [{\"op\":\"step\",\"gens\":10}]"
                    .into(),
            ),
        };
    if list.is_empty() {
        return Err("commands is empty".into());
    }
    let mut lines = Vec::new();
    for (k, v) in list.iter().enumerate() {
        let cmd = match prepare_command(v, host.world()) {
            Ok(c) => c,
            Err(e) => {
                lines.push(format!("{}. not applied: {e}", k + 1));
                return Err(format!(
                    "{}\nStopped; later commands were not applied.\n{}",
                    lines.join("\n"),
                    short_status(host.world())
                ));
            }
        };
        let op = v
            .get("op")
            .and_then(|o| o.as_str())
            .unwrap_or("?")
            .to_string();
        match host.exec(cmd) {
            Ok(a) => lines.push(match a.new_id {
                Some(id) => format!("{}. {op}: ok, new CA id {id}", k + 1),
                None => format!("{}. {op}: ok", k + 1),
            }),
            Err(e) => {
                lines.push(format!("{}. {op}: failed: {e}", k + 1));
                return Err(format!(
                    "{}\nStopped; later commands were not applied.\n{}",
                    lines.join("\n"),
                    short_status(host.world())
                ));
            }
        }
    }
    Ok(ToolOutput::text(format!(
        "{}\n{}",
        lines.join("\n"),
        short_status(host.world())
    )))
}

fn get_scene(host: &mut dyn Host, args: &Value) -> Res {
    check_unknown(args, &["path"])?;
    let Some(path) = string(args, "path")? else {
        return Ok(ToolOutput::text(host.scene().to_json()));
    };
    let p = output_path(path)?;
    let json = host.scene().to_json();
    std::fs::write(&p, &json).map_err(|e| format!("could not write {}: {e}", p.display()))?;
    Ok(ToolOutput::text(format!(
        "Saved the scene ({} bytes, generation {}, hash {}) to {}.",
        json.len(),
        host.world().generation,
        host.world().hash_hex(),
        p.display()
    )))
}

/// A file path to write: made absolute (relative to the server's working directory), with its
/// folder created if missing.
fn output_path(p: &str) -> Result<std::path::PathBuf, String> {
    let p = p.trim();
    if p.is_empty() {
        return Err("the path is empty".into());
    }
    let path = std::path::absolute(p).map_err(|e| format!("bad path {p}: {e}"))?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)
            .map_err(|e| format!("could not create folder {}: {e}", dir.display()))?;
    }
    Ok(path)
}

fn run_script(args: &Value, progress: Option<ProgressFn>, cancel: Option<Cancel>) -> Res {
    check_unknown(
        args,
        &[
            "script",
            "params",
            "max_seconds",
            "screen",
            "cell_px",
            "log_lines",
            "check_only",
        ],
    )?;
    let src = string(args, "script")?.ok_or("give the script text in `script`")?;
    if boolean(args, "check_only")? {
        return Ok(match jetcarp_script::check(src) {
            Ok(()) => ToolOutput::text("ok: no syntax errors"),
            Err(e) => ToolOutput::error(format!("syntax error at {e}")),
        });
    }
    let mut opts = jetcarp_script::RunOptions::default();
    if let Some(s) = string(args, "screen")? {
        let (w, h) = jetcarp_script::Screen::parse_size(s)?;
        opts.screen.width = w;
        opts.screen.height = h;
    }
    if let Some(px) = args.get("cell_px").and_then(|v| v.as_f64()) {
        opts.screen.cell_px = px.clamp(1.0, 256.0);
    }
    if let Some(p) = args.get("params").filter(|p| !p.is_null()) {
        for (k, v) in p.as_object().ok_or("`params` must be an object")? {
            let text = match v {
                Value::String(s) => s.clone(),
                v => v.to_string(),
            };
            opts.params.insert(k.clone(), text);
        }
    }
    let secs = args
        .get("max_seconds")
        .and_then(|v| v.as_f64())
        .unwrap_or(60.0);
    opts.limits.max_seconds = Some(secs.clamp(1.0, MAX_SCRIPT_SECONDS));
    opts.limits.max_cells = Some(MAX_CELLS);
    opts.limits.restrict_reads = true;
    let keep = int(args, "log_lines")?.unwrap_or(200).clamp(1, 2000) as usize;
    opts.limits.max_log_lines = 20_000;
    // Progress (sweeps, `progress()` in scripts) at most once a second, and at the end.
    let last = std::sync::Mutex::new(None::<std::time::Instant>);
    let on_event = move |e: jetcarp_script::Event| {
        if let (Some(f), jetcarp_script::Event::Progress(p)) = (&progress, e) {
            let mut l = last.lock().unwrap();
            let end = p.total > 0 && p.done >= p.total;
            if end || l.is_none_or(|t| t.elapsed().as_secs_f64() >= 1.0) {
                *l = Some(std::time::Instant::now());
                f(
                    p.done as f64,
                    (p.total > 0).then_some(p.total as f64),
                    p.line(),
                );
            }
        }
    };
    let stop = cancel.unwrap_or_default();
    let s = jetcarp_script::run(src, opts, on_event, stop.clone());
    if stop.load(std::sync::atomic::Ordering::Relaxed) {
        return Ok(ToolOutput::error(format!(
            "cancelled after {:.1} s ({} log lines; output folders: {})",
            s.seconds,
            s.log.len(),
            s.output_dirs
                .iter()
                .map(|d| d.display().to_string())
                .collect::<Vec<_>>()
                .join(", ")
        )));
    }
    let skip = s.log.len().saturating_sub(keep);
    let mut out = String::new();
    match &s.error {
        None => {
            let _ = writeln!(out, "ok: finished in {:.2} s", s.seconds);
        }
        Some(e) if s.timed_out => {
            let _ = writeln!(
                out,
                "stopped at the time limit after {:.1} s ({e})",
                s.seconds
            );
        }
        Some(e) => {
            let _ = writeln!(out, "error at {e}");
        }
    }
    for d in &s.output_dirs {
        let _ = writeln!(out, "output folder: {}", d.display());
    }
    for sw in &s.sweeps {
        let _ = writeln!(out, "sweep: {sw}");
    }
    if !s.result.is_empty() {
        let r: String = s.result.chars().take(2000).collect();
        let _ = writeln!(out, "result: {r}");
    }
    if skip > 0 || s.dropped_log_lines > 0 {
        let _ = writeln!(
            out,
            "log (last {keep} of {} lines):",
            s.log.len() as u64 + s.dropped_log_lines
        );
    } else if !s.log.is_empty() {
        let _ = writeln!(out, "log:");
    }
    for l in &s.log[skip..] {
        let _ = writeln!(out, "{l}");
    }
    Ok(if s.ok {
        ToolOutput::text(out)
    } else {
        ToolOutput::error(out)
    })
}

fn load_scene(host: &mut dyn Host, args: &Value) -> Res {
    check_unknown(args, &["scene", "path"])?;
    let scene = match (
        args.get("scene").filter(|v| !v.is_null()),
        string(args, "path")?,
    ) {
        (Some(_), Some(_)) => return Err("give scene or path, not both".into()),
        (Some(Value::String(s)), None) => Scene::from_json(s)?,
        (Some(v), None) => {
            serde_json::from_value(v.clone()).map_err(|e| format!("scene JSON: {e}"))?
        }
        (None, Some(p)) => {
            let text =
                std::fs::read_to_string(p).map_err(|e| format!("could not read {p}: {e}"))?;
            Scene::from_json(&text)?
        }
        (None, None) => return Err("give `scene` (JSON) or `path`".into()),
    };
    check_tiling_size(&scene.tiling)?;
    let recorded = scene.record.clone();
    host.replace(scene)?;
    let w = host.world();
    let check = match recorded {
        Some(r) if r.generation == w.generation && r.hash == w.hash_hex() => {
            " Replayed to its recorded hash.".to_string()
        }
        Some(r) => format!(
            " Warning: recorded generation {} hash {}, replayed generation {} hash {}.",
            r.generation,
            r.hash,
            w.generation,
            w.hash_hex()
        ),
        None => String::new(),
    };
    Ok(ToolOutput::text(format!(
        "Scene loaded.{check}\n{}",
        serde_json::to_string_pretty(&status(host)).unwrap_or_default()
    )))
}

// ---------------------------------------------------------------------------------------------
// Help text

const NOTATION: &str = "
Rule notation (any of these works as `rule`):
- Life-like B/S: B<birth counts>/S<survival counts>, e.g. B3/S23 (Life), B36/S23. Counts >= 10 use commas: B3,10/S2,3. Add V for edge-only neighbours: B3/S23V.
- Generations: B<..>/S<..>/C<states>, e.g. B2/S/C3 (Brian's Brain). State 1 is firing; higher states are dying.
- Elementary (square grid only, one row per generation from the top): W<0-255>, e.g. W30, W110.
- Hexagonal elementary (hex grid only): HexW <0-15>, HexW3 <0-19682>.
- Three-parent hexagonal elementary (flat-top hexagon grid `hex_flat` only; parents NW, N, NE): HexT <0-255> (index NW*4+N*2+NE), HexT3 <0-7625597484986>, HexT3 sum <0-2186> (by parent sum), HexT3 sym <n>, HexT sum <0-15>.
- Margolus block rules (square grid; 2x2 blocks, partition shifts every generation): Margolus <16 block outputs, upper left 1, upper right 2, lower left 4, lower right 8>, or Margolus BBM / Critters / Tron / SingleRotation / HPP.
- Lenia (continuous values 0-1): Lenia R=13,T=10,b=1,m=0.15,s=0.015 (Orbium; seed \"lenia:orbium\", or blobs \"random:0.5\", \"gauss:3\").
- Turmites/ants: Ant RL (Langton's ant), Turmite LLRR (letters L, R, N, U per colour).
- Cyclic <states>/<threshold>, GH <states>/<threshold> (Greenberg-Hastings), RPS <species>/<threshold>, ForestFire <p>/<f>, Hodgepodge <n>/<k1>/<k2>/<g>.
- JACA (Jimmie's Annihilating CA; cells -1, 0, +1): JACA B3/S23 (one B/S rule for both signs), JACA +B3/S23 -B36/S23 (positive, then negative), with optional blast <0-64>: JACA B3/S23 blast 2. Each sign counts only its own neighbours, so one sign alone runs the plain B/S rule; live cells touching the opposite sign are destroyed next generation, with every cell within `blast` neighbourhood steps of them. In set_cells, state 1 is +1 and state 2 is -1. Seeds \"halves:0.3\" (+1 left, -1 right) and \"signed:0.3:0.5\" (share of -1). CA options palette (for +1), palette_neg (for -1), glow (false hides the annihilation flash); palettes may be colours, \"#4fc3ff\" or \"#103050-#4fc3ff\".
- Others: WireWorld, Sandpile, SpectreGlider, Table <name>.
";

pub const HELP: &str = r#"JETCARP: a cellular-automaton (CA) research tool.

Concepts
- A world = a tiling (a grid of cells: squares, hexagons, triangles, Archimedean tilings, or the aperiodic Spectre) + one or more CAs (rules) running on it.
- Each CA works on a field (one value per cell). Binary rules (Life-like) share one field by default; multi-state rules (Brian's Brain, WireWorld) and sandpiles get their own.
- Every change is a command recorded in a scene, so the whole run can be saved (get_scene, with a path to write a file) and replayed exactly. The state hash identifies a state.
- Coordinates on grid tilings: x = column, y = row, (0, 0) at the top-left; world_status shows width and height. Aperiodic tilings use cell indices.
- Life-like rules count touching neighbours (8 on squares, 6 on hexagons, 12 on triangles).

Example 1: draw a glider in Life and find it 8 generations later
1. new_world {"tiling": "square", "width": 40, "height": 40, "boundary": "wrap", "cas": [{"rule": "Life", "seed": "empty"}]}
2. set_cells {"pattern": ".O.\n..O\nOOO", "x": 5, "y": 5}
3. step {"generations": 8}
4. read_cells {}   -> the glider has moved 2 cells right and 2 down (bounding box x 7..9, y 7..9). A glider moves 1 cell diagonally every 4 generations.

Example 2: Brian's Brain on hexagons, then a picture
1. new_world {"tiling": "hex", "width": 80, "height": 80, "boundary": "wrap", "cas": ["Brian's Brain"]}   (random start)
2. step {"generations": 200}
3. render_image {}

Example 3: a sandpile
1. new_world {"tiling": "square", "width": 41, "height": 41, "boundary": "dead", "cas": [{"rule": "Sandpile", "seed": "pile:20000"}]}
2. step {"generations": 5000, "stop_when_settled": true}
3. render_image {}

Example 4: two CAs on one grid, edit with commands
1. new_world {"tiling": "square", "width": 120, "height": 80, "cas": [{"rule": "W30", "seed": "top_center"}, {"rule": "Ant RL", "seed": "ants:3", "steps_per_gen": 20}]}
2. apply_commands {"commands": [{"op": "step", "gens": 60}, {"op": "update_ca", "id": 1, "change": {"enabled": false}}, {"op": "step", "gens": 500}]}
3. world_status {}

Tips
- list_rules and list_tilings show what exists. Rule names ("Life", "HighLife", "Seeds", "Day & Night", "Brian's Brain", "Langton's ant", "WireWorld") or notation both work.
- read_cells with no arguments shows the live cells' bounding box and a text picture of it.
- inspect_cell shows a cell's neighbours, useful on hexagons and triangles.
- step with stop_when_settled reports when a pattern dies out, freezes, or repeats (period found by exact comparison).
- Errors explain what to change; fix the arguments and call again.
- To keep a world going for hours (a screensaver), reseed it when it settles: reseed
  {"policies": [{"mode": "blobs", "when": {"kind": "settled"}, "cooldown": 300}]}; reseed {}
  reports what the policies did and the recent activity. export_wallpaper writes desktop-sized
  PNGs (any size, a span of monitors, or one per monitor), with palettes that have their own
  background colours (Paper, Candy, Glacier, and Desert are light).
- For many runs (every rule in a series, loops over tilings, output folders of PNGs with a
  manifest), write a script for run_script; jetcarp_help {"topic": "scripts"} lists its functions.
  world_status reports `done` when nothing can change any more (e.g. an elementary rule reached
  the bottom).
- The catalog (patterns, ant runs, settings, Quick Start entries): jetcarp_help {"topic": "catalog"}.
  Long searches run in the background as jobs: jetcarp_help {"topic": "jobs"}. files_list shows what
  runs wrote.
"#;

/// `jetcarp_help {"topic": "catalog"}`.
pub const HELP_CATALOG: &str = r#"The catalog: one database of what JETCARP has found, shared by the app, the CLI, scripts, and these tools.

Kinds of record (library_list "kind"):
- pattern: verified still lifes, oscillators, spaceships, guns. Rule-free: `rules` lists every rule it was verified in and `range` the rules it runs identically in.
- ant: one ant or turmite run (a turn string such as RL or LLRR, or a table) on a tiling: outcome highway (proved exactly: period, displacement d per period, onset step), periodic, symmetric (grows mirror-symmetrically), bounded, or chaotic.
- setting: a stage 1 setting (rule, tiling, soup density and size, grid, generations) with an interest score; candidates wait for approval.
- quickstart: entries of the app's Quick Start ("My finds").

Example 1: the best spaceships on hexagons, then one of them in a world
1. library_list {"class": "spaceship", "tiling": "hex", "limit": 5}
2. library_get {"id": "<id from 1>"}   (text drawings of its phases, rules, range)
3. new_world {"tiling": "hex", "width": 60, "height": 60, "cas": [{"rule": "<its rule>", "seed": "empty"}]}
4. paste_pattern {"id": "<id>"}, then step and read_cells

Example 2: Langton's ant highway and other highways on hexagons
1. ants_start {"rules": ["RL"]}, then job_status {"job": <job>, "wait_s": 300} (waits until done)
2. library_list {"kind": "ant", "outcome": "highway", "sort": "onset"}   (RL: period 104, d (-2, 2), onset 9977)
3. ants_start {"tilings": "hex", "strings": [2, 4]}; library_list {"kind": "ant", "tiling": "hex", "outcome": "highway"}
4. media_render {"ant": "<id>", "poster": true}   (a highway's loop; mode "seed" shows the build-up)

Example 3: curate
- library_update {"id": "<pattern id>", "rating": 4, "append_notes": "LLM: clean p3 oscillator"}
- library_update {"id": "setting-12", "decision": "approve"}   (or "rating": 2)
- library_update {"id": "ant-...", "name": "slow hex highway", "rating": 5}
- library_promote {"id": "<pattern or ant id>"}   (adds it to the Quick Start)
- library_collide {"a": "block", "b": "glider", "offsets": 4}   (collisions; new products are stored)
"#;

/// `jetcarp_help {"topic": "jobs"}`.
pub const HELP_JOBS: &str = r#"Long searches run in the background as catalog jobs; a start tool returns the job id at once.

Start (each takes {"resume": job} to continue a stopped job):
- hunt_start: soup hunts (patterns in B/S and Generations rules on periodic tilings).
- stage1_start: the settings search (which rules and settings look interesting).
- ants_start: ant and turmite searches (highways).
- pile_start: one large sandpile (pictures in its folder under <Pictures>/JETCARP/piles).
- mover_start: follow a mover on an aperiodic tiling on a sliding window, far past the fixed
  patches (default: the Spectre glider to 10,000 rings; {"rings": 1000000} takes minutes).
- evolve_start: the evolutionary glider search (not resumable; resume runs it again).

Watch and stop (any kind, started anywhere: here, the CLI, a script, the app):
- job_status {"job": 3}: progress now; {"job": 3, "wait_s": 300} waits up to 5 minutes for it to finish (one call instead of many polls); job_status {} lists jobs, {"kind": "ants"} only ant searches.
- job_stop {"job": 3}: stops at the next checkpoint; the job stays resumable.

Results: library_list (patterns, {"kind": "ant"}, {"kind": "setting"}), and files_list for folders
(piles, sweeps, media). run_script, library_search, and step stop early when the client cancels
the request (notifications/cancelled).

Example: an ant search on squares
1. ants_start {"tilings": "square", "strings": [2, 6]}   -> {"job": 7, ...}
2. job_status {"job": 7}   -> runs done, outcomes (highway, symmetric, chaotic, ...), newest highways
3. library_list {"kind": "ant", "job": 7, "outcome": "highway", "sort": "onset"}
"#;
