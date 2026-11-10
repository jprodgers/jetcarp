//! MCP tools for catalog media and the website export: `media_render` renders frames, an
//! animation (GIF or animated WebP), a poster, and a thumbnail and returns their paths (and,
//! on request, the poster inline); `site_export` writes the incremental website export. The
//! argument handling is shared with the script functions (`jetcarp_script::media`).

use crate::tools::{Res, ToolOutput, boolean, check_unknown, string, tool};
use jetcarp_core::library::Library;
use serde_json::{Value, json};

pub const NAMES: &[&str] = &["media_render", "site_export"];

pub fn handles(name: &str) -> bool {
    NAMES.contains(&name)
}

pub fn definitions() -> Vec<Value> {
    vec![
        tool(
            "media_render",
            "Render pattern media",
            false,
            "Renders media for the catalog and returns the file paths: per-step PNG frames, an animation (a GIF for small ones, an animated WebP for large ones such as whole soups), a poster frame, and a thumbnail, in one house style, under <catalog folder>/media/<kind>/<id>/<alone|seed>/. A pattern is shown alone on an empty field for one seamless period (spaceships in a window that moves with them, guns with their stream); a Quick Start entry or setting is shown from its seed; an ant highway as one seamless period in a window moving with the ant (mode seed: the run from step 0, the only choice for other ant outcomes). Up-to-date media are not rendered again. Give one of pattern, quickstart, setting, ant, or missing. Examples: {\"pattern\": \"glider\", \"poster\": true}, {\"quickstart\": \"sweep-1a2b\", \"gens\": 300}, {\"missing\": true, \"class\": \"spaceship\"}, {\"pattern\": \"glider\", \"palette\": \"Paper\"}.",
            json!({"type": "object", "properties": {
                "pattern": {"type": "string", "description": "A catalog pattern: id, unique id prefix, or name."},
                "quickstart": {"type": "string", "description": "A Quick Start entry id."},
                "setting": {"type": "string", "description": "A stage 1 setting id (through its Quick Start entry)."},
                "ant": {"type": "string", "description": "An ant run id (ant-..., from library_list {\"kind\": \"ant\"})."},
                "missing": {"type": "boolean", "description": "Every pattern and shown Quick Start entry whose media are missing or stale."},
                "class": {"type": "string", "description": "With missing: only patterns of these classes, comma-separated (spaceship, oscillator, gun, ...)."},
                "limit": {"type": "integer", "minimum": 0, "description": "With missing: at most this many targets (default no limit)."},
                "mode": {"type": "string", "enum": ["auto", "alone", "seed"], "description": "auto (default): patterns alone, everything else from the seed."},
                "format": {"type": "string", "enum": ["auto", "gif", "webp"], "description": "auto (default): GIF up to 320x320 pixels, 8 million pixels in all, and 256 colours; else WebP."},
                "periods": {"type": "integer", "minimum": 1, "description": "Alone: full periods shown (default 1)."},
                "gens": {"type": "integer", "minimum": 1, "description": "From the seed: generations (default 200; row rules one per row)."},
                "every": {"type": "integer", "minimum": 1, "description": "From the seed: a frame every this many generations (default 1)."},
                "frames": {"type": "boolean", "description": "Write the per-step PNG frames (default true)."},
                "force": {"type": "boolean", "description": "Render even when up to date."},
                "palette": {"type": "string", "description": "Colour the media with this built-in palette (its background, stage, and stops) instead of the house style: Ember, Tide, Prism, Ink, Mono, Aurora, Synthwave, Moss, Candy, Lava, Glacier, Phosphor, Koi, Desert, Thermal, Deep Sea, Paper, Blueprint, Neon, Autumn. Part of the render key: media stored in another palette render again."},
                "poster": {"type": "boolean", "description": "Also return the first target's poster image inline (PNG)."},
            }, "additionalProperties": false}),
        ),
        tool(
            "site_export",
            "Export the catalog for a website",
            false,
            "Writes the public part of the catalog to a folder for a static website: manifest.json (format version, export time, every item with its content hash and media links), patterns/<id>.json (the rule-free record, rule range, rules verified in, seen counts per rule, class, period, speed, name, tags, rating), quickstart/<id>.json, and media files (animation, poster, thumbnail; frames on request). Incremental: only new or changed files are written, and items no longer public are deleted. Public by default: patterns promoted or reviewed (never rejected), Quick Start entries auto or approved. Missing media are rendered first. Example: {\"to\": \"/home/me/site/data\", \"min_score\": 40}.",
            json!({"type": "object", "properties": {
                "to": {"type": "string", "description": "The export folder."},
                "state": {"type": "string", "description": "Curation states exported, comma-separated (default promoted,reviewed)."},
                "min_score": {"type": "number", "description": "Only patterns with at least this interest score."},
                "min_rating": {"type": "integer", "minimum": 1, "maximum": 5, "description": "Only patterns rated at least this."},
                "class": {"type": "string", "description": "Only these classes, comma-separated."},
                "quickstart": {"type": "string", "description": "Quick Start statuses exported, comma-separated (default auto,approved; none for none)."},
                "frames": {"type": "boolean", "description": "Copy the per-step PNG frames too (default false)."},
                "render": {"type": "boolean", "description": "Render missing media first (default true)."},
                "palette": {"type": "string", "description": "Render missing media in this built-in palette instead of the house style."},
            }, "required": ["to"], "additionalProperties": false}),
        ),
    ]
}

pub fn call(name: &str, args: &Value) -> Res {
    let lib = Library::open_default();
    match name {
        "media_render" => render(&lib, args),
        "site_export" => export(&lib, args),
        _ => Err(format!("unknown tool {name}")),
    }
}

fn render(lib: &Library, args: &Value) -> Res {
    let mut a = args.clone();
    let poster = boolean(args, "poster")?;
    if let Some(o) = a.as_object_mut() {
        o.remove("poster");
    }
    let r = jetcarp_script::media::media_into(lib, &a)?;
    let mut content =
        vec![json!({"type": "text", "text": serde_json::to_string_pretty(&r).unwrap_or_default()})];
    if poster && let Some(p) = r["done"][0]["files"]["poster"].as_str() {
        let bytes = std::fs::read(p).map_err(|e| format!("{p}: {e}"))?;
        content.push(
            json!({"type": "image", "data": crate::util::base64(&bytes), "mimeType": "image/png"}),
        );
    }
    let failed_all = r["done"].as_array().is_none_or(|d| d.is_empty())
        && r["failed"].as_array().is_some_and(|f| !f.is_empty());
    Ok(ToolOutput {
        content,
        is_error: failed_all,
    })
}

fn export(lib: &Library, args: &Value) -> Res {
    check_unknown(
        args,
        &[
            "to",
            "state",
            "min_score",
            "min_rating",
            "class",
            "quickstart",
            "frames",
            "render",
            "palette",
        ],
    )?;
    let to = string(args, "to")?.ok_or("give `to`, the export folder")?;
    let dir = std::path::absolute(to).map_err(|e| format!("bad path {to}: {e}"))?;
    let r = jetcarp_script::media::site_into(lib, args, &dir)?;
    Ok(ToolOutput::json(&r))
}
