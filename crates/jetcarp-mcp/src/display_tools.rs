//! Reseeding and wallpapers: `reseed` (reseed now, set or clear the automatic policies, report
//! their status and the recent activity) and `export_wallpaper` (a desktop-sized PNG under
//! <Pictures>/JETCARP/wallpapers). Both act through the host's commands, so they work on the headless
//! session and, through `use_app`, on the world in the app; they are the operations
//! `world.reseed` and `wallpaper.export`.

use crate::tools::{Res, ToolOutput, boolean, check_unknown, err, given, int, string, tool};
use jetcarp_api::op::{self, Quiet, WorldOp};
use jetcarp_api::ops::wallpaper::{WallpaperExport, WallpaperIn};
use jetcarp_api::ops::world::{ReseedIn, WorldReseed};
use jetcarp_api::world::Host;
use serde_json::{Value, json};

pub const NAMES: &[&str] = &["reseed", "export_wallpaper"];

pub fn handles(name: &str) -> bool {
    NAMES.contains(&name)
}

pub fn definitions() -> Vec<Value> {
    vec![
        tool(
            "reseed",
            "Reseed the world, or set automatic reseeding",
            false,
            "Puts new life into a settled world, now or automatically, and reports reseed status. Modes: {\"kind\": \"random\", \"flip\": 0.05} flips each cell with that probability; {\"kind\": \"rain\", \"drops\": 24, \"min_size\": 2, \"max_size\": 10, \"density\": 0.5} scatters round drops (sizes in tiles across, small ones more likely) whose dead cells come alive with that probability; {\"kind\": \"blobs\", \"count\": 1, \"min_radius\": 0.06, \"max_radius\": 0.18, \"live\": 0.35} drops large lumpy patches (radius as a fraction of the grid's shorter side) set live at the `live` ratio and dead otherwise. A mode may also be just \"random\", \"rain\", or \"blobs\" (defaults). On JACA CAs (cells +1 or -1) every mode also takes \"sign\": \"either\" (the default: each drop or blob all +1 or all -1 at random), \"mixed\" (each cell at random), \"alternate\" (drops and blobs alternate, and the first sign flips each reseed), \"pos\", or \"neg\"; other rules ignore it. Give `mode` to reseed now (the running CAs' fields, or `ca`); give `policies` to replace the automatic policies ([] clears them); a policy is {\"mode\": ..., \"when\": {\"kind\": \"settled\", \"activity\": 0.002, \"for_gens\": 60, \"cycle\": 12, \"period\": 6} or {\"kind\": \"every\", \"gens\": 500}, \"cooldown\": 200, \"ca\": 1, \"enabled\": true}. Settled fires (checked every 16 generations, after the cooldown) when the fraction of cells changing per generation stayed below `activity` for `for_gens` generations, or the changed count repeated with a period up to `cycle` that long (only oscillators and gliders left), or an exact period up to `period` was found, or the world is empty. Everything is a logged command, saved in the scene and deterministic. With no arguments: the policies, what each has done, and the recent activity. Examples: {\"mode\": \"blobs\"}, {\"policies\": [{\"mode\": \"rain\", \"when\": {\"kind\": \"settled\"}, \"cooldown\": 300}]}, {}.",
            json!({"type": "object", "properties": {
                "mode": {"type": ["object", "string"], "description": "Reseed now with this mode."},
                "ca": {"type": ["integer", "string"], "description": "With mode: only this CA's field (id or name). Default: every running CA's field."},
                "policies": {"type": "array", "items": {"type": "object"}, "description": "Replace the automatic policies ([] clears them)."},
                "activity_gens": {"type": "integer", "minimum": 0, "maximum": 512, "description": "Recent generations of activity to report (default 64)."},
            }, "additionalProperties": false}),
        ),
        tool(
            "export_wallpaper",
            "Export a desktop wallpaper",
            false,
            "Writes a desktop-sized PNG of the current world (scene, version, and hash embedded) under <Pictures>/JETCARP/wallpapers/ (or `path`) and returns the file paths and sizes. `fit`: cover (default: the grid fills the image, cropped, no stage colour showing), contain (the whole grid, letterboxed), repeat (a wrap-edge grid tiled at `cell_px`), or `center` and `scale` for a given view. `monitors` (WIDTHxHEIGHT+X+Y, comma separated) renders the whole span; with `split`, one image per monitor, cut so they line up. `fresh` renders a new grid of the world's tiling sized to fill the image at `cell_px` (the current CAs and seeds, run `gens` generations), leaving the current world alone. `palette` recolours the current world's CAs first (a logged update_ca). Examples: {\"size\": \"2560x1440\"}, {\"size\": \"4K\", \"fresh\": true, \"cell_px\": 3, \"gens\": 300, \"palette\": \"Paper\"}, {\"monitors\": \"2560x1440+0+0,1920x1080+2560+180\", \"split\": true}.",
            json!({"type": "object", "properties": {
                "size": {"type": "string", "description": "WIDTHxHEIGHT or a preset: HD, WUXGA, QHD, WQXGA, UWQHD, 4K, 5K, 8K, Phone (default 3840x2160). Ignored with monitors."},
                "fit": {"type": "string", "enum": ["cover", "contain", "repeat"], "description": "Default cover."},
                "center": {"type": "array", "items": {"type": "number"}, "minItems": 2, "maxItems": 2, "description": "World point at the image centre (with scale)."},
                "scale": {"type": "number", "exclusiveMinimum": 0, "description": "Pixels per world unit (with center)."},
                "cell_px": {"type": "number", "minimum": 1, "maximum": 256, "description": "Tile size in pixels for fresh and repeat (default 4)."},
                "fresh": {"type": "boolean", "description": "Render a new grid sized for the image instead of the current world."},
                "gens": {"type": "integer", "minimum": 0, "description": "With fresh: generations to run first (default 0)."},
                "palette": {"type": "string", "description": "Recolour every CA with this palette first."},
                "monitors": {"type": "string", "description": "Monitor layout, e.g. \"2560x1440+0+0,1920x1080+2560+180\"."},
                "split": {"type": "boolean", "description": "With monitors: one image per monitor."},
                "outlines": {"type": "number", "minimum": 0, "description": "Draw tile outlines when tiles are at least this many pixels (default 0: never)."},
                "path": {"type": "string", "description": "Output .png file or folder (default <Pictures>/JETCARP/wallpapers/)."},
            }, "additionalProperties": false}),
        ),
    ]
}

pub fn call(host: &mut dyn Host, name: &str, args: &Value) -> Res {
    match name {
        "reseed" => reseed(host, args),
        "export_wallpaper" => export(host, args),
        _ => Err(format!("unknown tool {name}")),
    }
}

fn reseed(host: &mut dyn Host, args: &Value) -> Res {
    check_unknown(args, &["mode", "ca", "policies", "activity_gens"])?;
    let a = ReseedIn {
        mode: given(args, "mode"),
        ca: given(args, "ca"),
        policies: given(args, "policies"),
        activity_gens: int(args, "activity_gens")?,
    };
    let r = WorldReseed::run(crate::tools::ctx(), host, a, &Quiet).map_err(err)?;
    Ok(ToolOutput::json(&op::to_value(&r)))
}

fn export(host: &mut dyn Host, args: &Value) -> Res {
    check_unknown(
        args,
        &[
            "size", "fit", "center", "scale", "cell_px", "fresh", "gens", "palette", "monitors",
            "split", "outlines", "path",
        ],
    )?;
    let (center, scale) = match (
        args.get("center"),
        args.get("scale").and_then(|v| v.as_f64()),
    ) {
        (Some(Value::Array(c)), Some(scale)) if c.len() == 2 => (
            Some([
                c[0].as_f64().ok_or("`center` needs two numbers")?,
                c[1].as_f64().ok_or("`center` needs two numbers")?,
            ]),
            Some(scale),
        ),
        (None, None) => (None, None),
        _ => return Err("`center` ([x, y]) and `scale` go together".into()),
    };
    let fresh = boolean(args, "fresh")?;
    let a = WallpaperIn {
        source: None,
        monitors: string(args, "monitors")?.map(String::from),
        split: boolean(args, "split")?,
        size: string(args, "size")?.map(String::from),
        cell_px: args.get("cell_px").and_then(|v| v.as_f64()).unwrap_or(4.0),
        center,
        scale,
        fit: string(args, "fit")?.map(String::from),
        palette: string(args, "palette")?.map(String::from),
        outline: args.get("outlines").and_then(|v| v.as_f64()).unwrap_or(0.0),
        fresh,
        // Given (even as null) without `fresh` is an error the operation reports.
        gens: match args.get("gens") {
            Some(_) if fresh => Some(int(args, "gens")?.unwrap_or(0).max(0) as u64),
            Some(_) => Some(0),
            None => None,
        },
        gpu: false,
        out: string(args, "path")?.map(String::from),
    };
    let r = WallpaperExport::run(crate::tools::ctx(), host, a, &Quiet).map_err(err)?;
    Ok(ToolOutput::json(&op::to_value(&r)))
}
