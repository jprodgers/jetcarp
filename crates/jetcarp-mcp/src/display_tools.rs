//! Reseeding and wallpapers: `reseed` (reseed now, set or clear the automatic policies, report
//! their status and the recent activity) and `export_wallpaper` (a desktop-sized PNG under
//! <Pictures>/JETCARP/wallpapers). Both act through the host's commands, so they work on the headless
//! session and, through `use_app`, on the world in the app.

use crate::tools::{
    MAX_CELLS, Res, ToolOutput, boolean, check_unknown, err, given, int, string, tool,
};
use jetcarp_api::op::{self, Quiet, WorldOp};
use jetcarp_api::ops::world::{ReseedIn, WorldReseed};
use jetcarp_api::world::Host;
use jetcarp_core::wallpaper::{self, Fit};
use jetcarp_core::{CaChange, Command, World};
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
    let monitors = string(args, "monitors")?
        .map(wallpaper::parse_monitors)
        .transpose()?;
    let split = boolean(args, "split")?;
    if split && monitors.is_none() {
        return Err("`split` needs `monitors`".into());
    }
    let (width, height) = match &monitors {
        Some(m) => {
            let (_, _, w, h) = wallpaper::span_box(m);
            (w, h)
        }
        None => wallpaper::parse_size(string(args, "size")?.unwrap_or("3840x2160"))?,
    };
    let cell_px = args.get("cell_px").and_then(|v| v.as_f64()).unwrap_or(4.0);
    let fit = match (
        args.get("center"),
        args.get("scale").and_then(|v| v.as_f64()),
    ) {
        (Some(Value::Array(c)), Some(scale)) if c.len() == 2 => Fit::View {
            center: [
                c[0].as_f64().ok_or("`center` needs two numbers")?,
                c[1].as_f64().ok_or("`center` needs two numbers")?,
            ],
            scale,
        },
        (None, None) => match string(args, "fit")?.unwrap_or("cover") {
            "repeat" => Fit::Repeat { cell_px },
            f => Fit::parse(f)?,
        },
        _ => return Err("`center` ([x, y]) and `scale` go together".into()),
    };
    if let Some(p) = string(args, "palette")? {
        let w = host.world();
        if !w.palette_names().iter().any(|n| n == p) {
            return Err(format!(
                "no palette named {p}. Palettes: {}",
                w.palette_names().join(", ")
            ));
        }
        let ids: Vec<u32> = w.cas.iter().map(|c| c.id).collect();
        for id in ids {
            host.exec(Command::UpdateCa {
                id,
                change: CaChange {
                    palette: Some(p.to_string()),
                    ..Default::default()
                },
            })?;
        }
    }
    let outline = args.get("outlines").and_then(|v| v.as_f64()).unwrap_or(0.0);
    let fresh_world;
    let w: &World = if boolean(args, "fresh")? {
        let mut sc = host.scene();
        sc.tiling = wallpaper::fresh_tiling(&sc.tiling, width, height, cell_px);
        sc.commands.clear();
        sc.record = None;
        let probe = jetcarp_core::Topology::build(&sc.tiling)?;
        if probe.len() as u64 > MAX_CELLS * 4 {
            return Err(format!(
                "a fresh grid of {} cells is too large here (at most {}); use a larger cell_px",
                probe.len(),
                MAX_CELLS * 4
            ));
        }
        let mut fw = World::from_scene(&sc)?;
        let gens = int(args, "gens")?.unwrap_or(0).max(0) as u64;
        if gens * fw.topo.len() as u64 > crate::tools::MAX_WORK * 4 {
            return Err(
                "too many generations for a grid this size; lower gens or raise cell_px".into(),
            );
        }
        fw.apply(Command::Step { gens })?;
        fresh_world = fw;
        &fresh_world
    } else {
        if args.get("gens").is_some() {
            return Err("`gens` goes with `fresh` (use step for the current world)".into());
        }
        host.world()
    };
    let img = wallpaper::wallpaper(w, width, height, &fit, outline)?;
    let name = {
        let s = host.scene().name;
        jetcarp_api::out::sanitize(if s.is_empty() { "wallpaper" } else { &s })
    };
    let stamp = jetcarp_api::out::stamp();
    let (dir, stem) = match string(args, "path")? {
        Some(p) if p.to_ascii_lowercase().ends_with(".png") => {
            let p = jetcarp_api::out::absolute(std::path::Path::new(p));
            (
                p.parent().map(|d| d.to_path_buf()).unwrap_or_default(),
                Some(p.file_stem().unwrap().to_string_lossy().to_string()),
            )
        }
        Some(p) => (jetcarp_api::out::absolute(std::path::Path::new(p)), None),
        None => (jetcarp_api::out::out_root(None).join("wallpapers"), None),
    };
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let mut meta = jetcarp_core::render::png_metadata(w);
    meta.push((
        "jetcarp:wallpaper",
        serde_json::to_string(&fit).unwrap_or_default(),
    ));
    let mut files = Vec::new();
    match (&monitors, split) {
        (Some(m), true) => {
            for (k, (mon, part)) in m.iter().zip(wallpaper::split(&img, m)).enumerate() {
                let file = match &stem {
                    Some(s) => format!("{s}-{}.png", k + 1),
                    None => format!("{name}-{}-{}x{}-{stamp}.png", k + 1, mon.width, mon.height),
                };
                let path = dir.join(file);
                jetcarp_core::render::write_png(&path, &part, &meta)?;
                files.push(
                    json!({"path": path, "width": part.width, "height": part.height,
                    "monitor": format!("{}x{}+{}+{}", mon.width, mon.height, mon.x, mon.y)}),
                );
            }
        }
        _ => {
            let file = match &stem {
                Some(s) => format!("{s}.png"),
                None => format!("{name}-{width}x{height}-{stamp}.png"),
            };
            let path = dir.join(file);
            jetcarp_core::render::write_png(&path, &img, &meta)?;
            files.push(json!({"path": path, "width": width, "height": height}));
        }
    }
    Ok(ToolOutput::json(&json!({
        "files": files,
        "fit": fit,
        "generation": w.generation,
        "hash": w.hash_hex(),
    })))
}
