//! The MCP tools. Each one reads the host's world or changes it through [`Command`]s.
//!
//! Descriptions are written for a model that has never seen JETCARP: they say what the tool does,
//! give a minimal example, and name the tool to call next.
//!
//! The world and reference tools read their arguments as they always have (numbers given as
//! text, `"true"` for true) and call the operations in `jetcarp_api::ops`; an operation's text
//! is the tool's result and its error message the tool's error. Limits are the interactive ones
//! ([`ctx`]).
//!
//! [`Command`]: jetcarp_core::Command

use crate::util;
use jetcarp_api::op::{self, Callbacks, Quiet, Report, WorldOp};
use jetcarp_api::ops::reference::{ReferenceRules, ReferenceTilings, RulesIn, TilingsIn};
use jetcarp_api::ops::world::{
    ApplyIn, DrawIn, InspectIn, NewWorldIn, OpenIn, ReadIn, RenderIn, SaveIn, StatusIn, StepIn,
    WorldApply, WorldDraw, WorldInspect, WorldNew, WorldOpen, WorldRead, WorldRender, WorldSave,
    WorldStatusOp, WorldStep,
};
use jetcarp_api::world::Host;
use jetcarp_api::{ApiError, Ctx, Limits, Op, ops};
use serde_json::{Value, json};
use std::fmt::Write as _;
use std::sync::OnceLock;

/// Largest world `new_world` or `set_tiling` will build.
pub const MAX_CELLS: u64 = Limits::INTERACTIVE.max_cells;
/// Most generations one call may run.
pub const MAX_GENS: u64 = Limits::INTERACTIVE.max_gens;
/// Most cell updates (cells x generations) one call may run.
pub const MAX_WORK: u64 = Limits::INTERACTIVE.max_work;
/// Most cells `read_cells` prints as a picture.
pub const MAX_PICTURE: usize = Limits::INTERACTIVE.max_picture;
/// Most cells one `set_cells` call may write.
pub const MAX_SET: usize = Limits::INTERACTIVE.max_set;
/// Longest `run_script` run, in seconds.
pub const MAX_SCRIPT_SECONDS: f64 = Limits::INTERACTIVE.max_script_seconds;

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
    // Background jobs (`job_tools.rs`): one start tool per kind, one status and one stop tool for
    // all of them.
    "hunt_start",
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
            "Explains JETCARP and shows worked examples. `topic`: \"overview\" (default: what JETCARP is, coordinates, a glider in Life, Brian's Brain on hexagons, sandpiles, several CAs at once), \"catalog\" (finding, listing, curating, and promoting patterns, ants, Quick Start entries), \"jobs\" (long searches in the background: start, job_status, job_stop, resume), or \"scripts\" (the full function reference for run_script). Call this first if you are unsure how the tools fit together.",
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
        "step" => step(host, args, cancel.clone()),
        "render_image" => render_image(host, args),
        "apply_commands" => apply_commands(host, args),
        "get_scene" => get_scene(host, args),
        "load_scene" => load_scene(host, args),
        "run_script" => run_script(args, progress, cancel),
        n if crate::library_tools::handles(n) => {
            crate::library_tools::call(host, n, args, progress, cancel)
        }
        n if crate::explore_tools::handles(n) => crate::explore_tools::call(n, args),
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

/// [`string`], owned.
fn text(args: &Value, key: &str) -> Result<Option<String>, String> {
    Ok(string(args, key)?.map(String::from))
}

/// The value under `key`, unless it is missing or `null`.
pub(crate) fn given(args: &Value, key: &str) -> Option<Value> {
    args.get(key).filter(|v| !v.is_null()).cloned()
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

static PLATFORM: OnceLock<std::sync::Arc<dyn jetcarp_api::Platform>> = OnceLock::new();

/// The machine the tools run on: the `jetcarp` program sets its own before it serves (the GPU,
/// jobs in processes of their own); without one, the tools run headless. Set it before the first
/// tool call (first call wins).
pub fn set_platform(p: std::sync::Arc<dyn jetcarp_api::Platform>) {
    let _ = PLATFORM.set(p);
}

/// What the tools run with: the interactive limits ([`Limits::INTERACTIVE`]), files anywhere
/// (paths come from the client, as they always have), and the platform set by [`set_platform`].
pub fn ctx() -> &'static Ctx {
    static CX: OnceLock<Ctx> = OnceLock::new();
    CX.get_or_init(|| {
        let cx = Ctx::interactive();
        match PLATFORM.get() {
            Some(p) => cx.with_platform(p.clone()),
            None => cx,
        }
    })
}

/// An operation's error as the tool's error text.
pub(crate) fn err(e: ApiError) -> String {
    e.to_string()
}

/// The `world_status` report.
pub fn status(host: &dyn Host) -> Value {
    op::to_value(&ops::world::world_status(host))
}

// ---------------------------------------------------------------------------------------------
// Tools: each reads its arguments as the tools always have and calls an operation
// (`jetcarp_api::ops`), whose text is the tool's result.

fn list_rules(args: &Value) -> Res {
    check_unknown(args, &["family"])?;
    let out = ReferenceRules::run(
        ctx(),
        RulesIn {
            family: text(args, "family")?,
        },
        &Quiet,
    )
    .map_err(err)?;
    let mut s = String::from("Named rules (use the name or the notation as `rule`):\n");
    if out.rules.len() <= 80 {
        s.push('\n');
        for e in &out.rules {
            let _ = writeln!(
                s,
                "- {} | {} | {} | {}",
                e.name, e.notation, e.family, e.blurb
            );
        }
    } else {
        // Long lists: names and notation only, grouped by family.
        s.push_str("(Too many to describe each; pass `family` for one-line descriptions.)\n");
        for f in &out.families {
            let names: Vec<String> = out
                .rules
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
    let search = text(args, "search")?;
    let out = ReferenceTilings::run(
        ctx(),
        TilingsIn {
            search: search.clone(),
            limit: None,
        },
        &Quiet,
    )
    .map_err(err)?;
    if search.is_some() {
        return Ok(ToolOutput::text(out.matches_text()));
    }
    let mut s = String::from(
        "Tilings (id | name | group | neighbours per cell: edge-sharing / touching | note).\n\
         Size: width x height unit cells, or `cells` (about that many); aperiodic tilings use `level`.\n\
         Life-like rules count touching neighbours by default (Moore on squares: 8); add V to a B/S rule \
         (B3/S23V) or neighborhood \"edge\" to count edge neighbours only.\n\n",
    );
    for f in &out.families {
        let deg = match &f.error {
            None => format!("{:?} / {:?}", f.edge, f.touch),
            Some(_) => "?".into(),
        };
        let _ = writeln!(
            s,
            "- {} | {} | {} | {} | {}",
            f.id, f.name, f.group, deg, f.note
        );
    }
    // The catalog is large (over 1,300 entries): summarise it and point to `search`.
    let _ = writeln!(
        s,
        "\nAlso usable as `tiling` (list them with list_tilings {{\"search\": \"...\"}}): {} k-uniform \
         tilings k1-001 .. k6-673 (ids like \"k2-005\"; search \"k2-\" for all 2-uniform ones), {} other \
         catalog tilings, and the {} isohedral tile types ih01 .. ih93.",
        out.catalog.k_uniform, out.catalog.other, out.catalog.isohedral
    );
    Ok(ToolOutput::text(s))
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
    let a = NewWorldIn {
        tiling: text(args, "tiling")?,
        width: int(args, "width")?,
        height: int(args, "height")?,
        cells: int(args, "cells")?,
        level: int(args, "level")?,
        boundary: text(args, "boundary")?,
        cas: given(args, "cas"),
        rng_seed: int(args, "rng_seed")?,
        name: text(args, "name")?,
        tiling_params: given(args, "tiling_params"),
    };
    let out = WorldNew::run(ctx(), host, a, &Quiet).map_err(err)?;
    let mut t = ToolOutput::text(out.text());
    if host.world().cas.is_empty() {
        t.content.push(json!({"type": "text", "text": "No CAs yet: add one with apply_commands {\"op\":\"add_ca\",\"ca\":{\"rule\":\"Life\"}}."}));
    }
    Ok(t)
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
    let a = DrawIn {
        ca: given(args, "ca"),
        pattern: text(args, "pattern")?,
        rle: text(args, "rle")?,
        x: int(args, "x")?,
        y: int(args, "y")?,
        cells: given(args, "cells"),
        indices: given(args, "indices"),
        state: int(args, "state")?,
        clear_first: boolean(args, "clear_first")?,
        erase: boolean(args, "erase")?,
    };
    let out = WorldDraw::run(ctx(), host, a, &Quiet).map_err(err)?;
    Ok(ToolOutput::text(out.text()))
}

fn read_cells(host: &mut dyn Host, args: &Value) -> Res {
    check_unknown(args, &["ca", "x", "y", "width", "height", "format"])?;
    let a = ReadIn {
        ca: given(args, "ca"),
        x: int(args, "x")?,
        y: int(args, "y")?,
        width: int(args, "width")?,
        height: int(args, "height")?,
        format: text(args, "format")?,
    };
    let out = WorldRead::run(ctx(), host, a, &Quiet).map_err(err)?;
    Ok(ToolOutput::text(out.text()))
}

fn inspect_cell(host: &mut dyn Host, args: &Value) -> Res {
    check_unknown(args, &["x", "y", "index"])?;
    let a = InspectIn {
        x: int(args, "x")?,
        y: int(args, "y")?,
        index: int(args, "index")?,
    };
    let out = WorldInspect::run(ctx(), host, a, &Quiet).map_err(err)?;
    Ok(ToolOutput::json(&op::to_value(&out)))
}

fn step(host: &mut dyn Host, args: &Value, cancel: Option<Cancel>) -> Res {
    check_unknown(args, &["generations", "stop_when_settled"])?;
    let a = StepIn {
        generations: int(args, "generations")?,
        stop_when_settled: boolean(args, "stop_when_settled")?,
    };
    let p = Callbacks {
        on_report: None,
        cancel,
    };
    let out = WorldStep::run(ctx(), host, a, &p).map_err(err)?;
    Ok(ToolOutput::text(out.text()))
}

fn render_image(host: &mut dyn Host, args: &Value) -> Res {
    check_unknown(args, &["max_side", "outlines", "save_path"])?;
    let a = RenderIn {
        max_side: int(args, "max_side")?,
        outlines: args.get("outlines").and_then(|v| v.as_bool()),
        save_path: text(args, "save_path")?,
    };
    let out = WorldRender::run(ctx(), host, a, &Quiet).map_err(err)?;
    Ok(ToolOutput {
        content: vec![
            json!({"type": "text", "text": out.text()}),
            json!({"type": "image", "data": util::base64(&out.png), "mimeType": "image/png"}),
        ],
        is_error: false,
    })
}

fn apply_commands(host: &mut dyn Host, args: &Value) -> Res {
    check_unknown(args, &["commands"])?;
    let a = ApplyIn {
        commands: args.get("commands").cloned().unwrap_or(Value::Null),
    };
    let out = WorldApply::run(ctx(), host, a, &Quiet).map_err(err)?;
    Ok(ToolOutput::text(out.text()))
}

fn get_scene(host: &mut dyn Host, args: &Value) -> Res {
    check_unknown(args, &["path"])?;
    let reply = match text(args, "path")? {
        Some(path) => WorldSave::run(ctx(), host, SaveIn { path }, &Quiet)
            .map_err(err)?
            .text(),
        None => WorldStatusOp::run(ctx(), host, StatusIn { scene: true }, &Quiet)
            .map_err(err)?
            .text(),
    };
    Ok(ToolOutput::text(reply))
}

fn load_scene(host: &mut dyn Host, args: &Value) -> Res {
    check_unknown(args, &["scene", "path"])?;
    let a = OpenIn {
        scene: given(args, "scene"),
        path: text(args, "path")?,
    };
    let out = WorldOpen::run(ctx(), host, a, &Quiet).map_err(err)?;
    Ok(ToolOutput::text(out.text()))
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
    let limits = &ctx().limits;
    let mut opts = jetcarp_script::RunOptions::default();
    if let Some(s) = string(args, "screen")? {
        let (w, h) = jetcarp_api::args::Screen::parse_size(s)?;
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
    opts.limits.max_seconds = Some(secs.clamp(1.0, limits.max_script_seconds));
    opts.limits.max_cells = Some(limits.max_cells);
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
- The catalog (patterns, ant runs, Quick Start entries): jetcarp_help {"topic": "catalog"}.
  Long searches run in the background as jobs: jetcarp_help {"topic": "jobs"}. files_list shows what
  runs wrote.
"#;

/// `jetcarp_help {"topic": "catalog"}`.
pub const HELP_CATALOG: &str = r#"The catalog: one database of what JETCARP has found, shared by the app, the CLI, scripts, and these tools.

Kinds of record (library_list "kind"):
- pattern: verified still lifes, oscillators, spaceships, guns. Rule-free: `rules` lists every rule it was verified in and `range` the rules it runs identically in.
- ant: one ant or turmite run (a turn string such as RL or LLRR, or a table) on a tiling: outcome highway (proved exactly: period, displacement d per period, onset step), periodic, symmetric (grows mirror-symmetrically), bounded, or chaotic.
- quickstart: entries of the app's Quick Start: promotions ("My finds"), entries from packs, and the atlas an explore search published (`source` builtin lists the built-in examples). An entry is named by its handle: find:<id>, atlas:<...>, qs-<hex>, or a built-in id such as life.

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
- library_update {"id": "find:<id>", "decision": "reject"}   (hides a Quick Start entry; "approve" shows it)
- library_update {"id": "ant-...", "name": "slow hex highway", "rating": 5}
- library_promote {"id": "<pattern or ant id>"}   (adds it to the Quick Start)
- library_collide {"a": "block", "b": "glider", "offsets": 4}   (collisions; new products are stored)
"#;

/// `jetcarp_help {"topic": "jobs"}`.
pub const HELP_JOBS: &str = r#"Long searches run in the background as catalog jobs; a start tool returns the job id at once.

Start (each takes {"resume": job} to continue a stopped job):
- hunt_start: soup hunts (patterns in B/S and Generations rules on periodic tilings).
- ants_start: ant and turmite searches (highways).
- pile_start: one large sandpile (pictures in its folder under <Pictures>/JETCARP/piles).
- mover_start: follow a mover on an aperiodic tiling on a sliding window, far past the fixed
  patches (default: the Spectre glider to 10,000 rings; {"rings": 1000000} takes minutes).
- evolve_start: the evolutionary glider search (not resumable; resume runs it again).

Watch and stop (any kind, started anywhere: here, the CLI, a script, the app):
- job_status {"job": 3}: progress now; {"job": 3, "wait_s": 300} waits up to 5 minutes for it to finish (one call instead of many polls); job_status {} lists jobs, {"kind": "ants"} only ant searches.
- job_stop {"job": 3}: stops at the next checkpoint; the job stays resumable.

Results: library_list (patterns, {"kind": "ant"}), and files_list for folders
(piles, sweeps, media). run_script, library_search, and step stop early when the client cancels
the request (notifications/cancelled).

Example: an ant search on squares
1. ants_start {"tilings": "square", "strings": [2, 6]}   -> {"job": 7, ...}
2. job_status {"job": 7}   -> runs done, outcomes (highway, symmetric, chaotic, ...), newest highways
3. library_list {"kind": "ant", "job": 7, "outcome": "highway", "sort": "onset"}
"#;
