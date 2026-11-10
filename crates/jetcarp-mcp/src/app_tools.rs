//! App-only tools: listed and answered only when the host is the JETCARP app (its
//! [`Host::has_display`](crate::Host::has_display) is true), or when `jetcarp mcp` forwards them
//! to the app. `app_display` reads and changes where the grid shows (window, full screen,
//! outputs spanning or mirroring monitors), screensaver mode and its playlist, keep-awake,
//! the cell size, and the chrome (menu bar, panes).

use crate::Host;
use crate::tools::{Res, ToolOutput, tool};
use serde_json::{Value, json};

pub const NAMES: &[&str] = &["app_display"];

pub fn handles(name: &str) -> bool {
    NAMES.contains(&name)
}

/// The error a host without a display gives.
pub const APP_ONLY: &str = "app_display is only available in the JETCARP app: start the app with AI control on (or `jetcarp --control`), then use `jetcarp mcp --app` or call use_app with attach=true";

pub fn definitions() -> Vec<Value> {
    vec![tool(
        "app_display",
        "The app's displays and screensaver",
        false,
        "Reads and changes how the JETCARP app shows the grid. With no arguments it only returns the state: where the grid shows (mode window, fullscreen, or outputs: chrome-less full-screen windows on chosen monitors that span one canvas or mirror one view), the monitors (deduplicated; index is what `monitors` and `fullscreen_monitor` take), the canvas size in pixels, the cell size, running state and speed, the menu and panes, keep-awake (and which backend holds it), and screensaver mode with its playlist, the screensaver profiles (`profiles`: name, built-in or yours, what it shows, cell size, reseeding) and the one showing (`screensaver.profile`). Arguments change those; several can be given at once and are applied in a sensible order (outputs and cell size before the screensaver starts). Examples: {\"screensaver\": true}, {\"screensaver\": true, \"profile\": \"Hodgepodge\"}, {\"saver\": {\"source\": \"playlist\", \"playlist\": [\"profile:Life\", \"profile:Star Wars\"], \"minutes\": 3}},{\"mode\": \"outputs\", \"monitors\": [0, 1], \"arrangement\": \"span\"}, {\"cell_px\": 1, \"refill\": true}, {\"playlist\": \"next\"}, {\"saver\": {\"source\": \"playlist\", \"playlist\": [\"rule:B3/S23\", \"scene:/home/me/a.json\"], \"minutes\": 2, \"shuffle\": true}}.",
        json!({"type": "object", "properties": {
            "screensaver": {"type": "boolean", "description": "Start (true) or end (false) screensaver mode: every control hidden, the grid on the outputs (or the main window full screen), running."},
            "playing": {"type": "boolean", "description": "Run (true) or pause (false) generations."},
            "speed": {"type": "number", "minimum": 0, "description": "Generations per second; 0 runs as fast as possible."},
            "gpu": {"type": "boolean", "description": "Step on the GPU where the rules allow it (results are identical)."},
            "mode": {"type": "string", "enum": ["window", "fullscreen", "outputs"], "description": "Where the grid shows: the main window, the main window full screen, or the output windows."},
            "fullscreen": {"type": "boolean", "description": "Shorthand: true is mode fullscreen, false is mode window."},
            "fullscreen_monitor": {"type": ["integer", "null"], "description": "Monitor index for full screen; null: the monitor the window is on."},
            "monitors": {"type": "array", "items": {"type": "integer"}, "description": "Output monitors by index (full screen on each). Also sets mode outputs unless mode is given."},
            "windows": {"type": "array", "items": {"type": "string"}, "description": "Outputs as plain windows instead of monitors, each \"WxH@X,Y\" (size in pixels at canvas position X,Y), for trying spans on one monitor. Also sets mode outputs unless mode is given."},
            "arrangement": {"type": "string", "enum": ["span", "mirror"], "description": "span: one canvas across the outputs by their positions; mirror: the same view on each."},
            "units": {"type": "string", "enum": ["pixels", "logical"], "description": "Canvas units for the span: physical pixels, or logical pixels (mixed scale factors with positions in logical pixels)."},
            "hide_preview": {"type": "boolean", "description": "With outputs on, draw no grid in the main window (saves GPU work)."},
            "menu_autohide": {"type": "boolean", "description": "Auto-hide the menu bar."},
            "panes_hidden": {"type": "boolean", "description": "Hide every pane (the grid fills the main window)."},
            "status_bar": {"type": "boolean", "description": "Show the status bar."},
            "cell_px": {"type": "number", "minimum": 1, "maximum": 64, "description": "Cell size in physical pixels (1 to 64), used by refill and pixel_zoom."},
            "refill": {"type": "boolean", "description": "true: rebuild the tiling now to fill the canvas at cell_px (periodic tilings), zoomed pixel-exact."},
            "pixel_zoom": {"type": "boolean", "description": "true: zoom so one cell edge is cell_px pixels, the camera snapped to whole pixels."},
            "keep_awake": {"type": "boolean", "description": "Keep the computer awake (no screen blanking, locking, or suspend) while the app is open."},
            "skip": {"type": "string", "enum": ["next", "previous"], "description": "While the screensaver runs a rotation: show the next or previous item now."},
            "identify": {"type": "boolean", "description": "true: show each monitor's index full screen on it for three seconds."},
            "profile": {"type": "string", "description": "A screensaver profile by name (the state's `profiles` lists them): the screensaver shows it from now on (source profile); with screensaver true it starts on it, and while the screensaver runs it switches at once. A profile is a complete setup: scene, palette, reseed policies, cell size, speed."},
            "rotation": {"type": "string", "description": "A rotation by name (the state's `rotations` lists them): the screensaver runs it from now on; with screensaver true it starts on it, and while the screensaver runs it switches."},
            "save_rotation": {"type": "object", "description": "Create or replace a rotation: {name, description, order (in_order, shuffle, daily), default_minutes (or null), look, script (Rhai), items: [{source, label, end, hold_s, look, schedule}]}. source is {kind: profile, name}, {kind: scene, path}, {kind: quick_start, id}, or {kind: series, rules, from, to, family, boundary, seed}. end is a list of {kind: minutes, minutes}, {kind: generations, gens}, {kind: settled, latest}, {kind: low_activity, below, for_gens}, {kind: done}, or {kind: clock, at: \"HH:MM\"}; any one ends the item. schedule is {days: [\"mon\", ...], from, to}."},
            "delete_rotation": {"type": "string", "description": "Delete a rotation of yours by name (a built-in's edited copy reverts to the built-in)."},
            "saver": {"type": "object", "description": "Screensaver settings to change (saved): source (profile, rotation, current, scene, quickstart), profile (a profile name), rotation (a rotation name), scene (a scene file), quickstart (a Quick Start id), cell_px, refill, speed (0: max), gpu (these four for the current world, scene files, and Quick Start ids; profiles carry their own), keep_awake, exit_any_input (else Esc only), control (start the AI-control endpoint), hide_windows, cursor_hide_s, caption (true or false to show what is running, or an object with show, corner (BottomLeft, BottomRight, TopLeft, TopRight), size, generation, seconds)."},
        }, "additionalProperties": false}),
    )]
}

pub fn call(host: &mut dyn Host, name: &str, args: &Value) -> Res {
    match name {
        "app_display" => host.display(args).map(|v| ToolOutput::json(&v)),
        _ => Err(format!("unknown tool {name}")),
    }
}
