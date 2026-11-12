//! The world tools: `world_new`, `world_status`, `world_read`, `world_edit`, `world_step` and
//! `world_render`. They act on the host's world (the server's own headless one, or the app's
//! through `use_app`), and every change is a command recorded in its scene.

use crate::schema::{self, named, object, words};
use crate::tools::{Call, Res, Tool, ToolOutput, ctx, err};
use jetcarp_api::op::{self, Quiet, Report, WorldOp, input_schema};
use jetcarp_api::ops::wallpaper::{WallpaperExport, WallpaperIn};
use jetcarp_api::ops::world::{
    ApplyIn, DrawIn, InspectIn, NewWorldIn, ObjectsIn, OpenIn, PasteIn, ReadIn, RenderIn, ReseedIn,
    SaveIn, StatusIn, StepIn, WorldApply, WorldDraw, WorldInspect, WorldNew, WorldObjects,
    WorldOpen, WorldPaste, WorldRead, WorldRender, WorldReseed, WorldSave, WorldStatusOp,
    WorldStep,
};
use serde_json::{Value, json};

const OPEN_KEYS: &[&str] = &["scene", "path", "quickstart", "pattern"];
const NEW_KEYS: &[&str] = &[
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
];

pub(crate) fn tools() -> Vec<Tool> {
    vec![
        Tool {
            name: "world_new",
            title: "Make or open a world",
            read_only: false,
            destructive: true,
            description: || {
                format!(
                    "Replaces the current world. Build one from a tiling (the grid of cells) and the CAs (cellular automata) that run on it: {{\"tiling\": \"hex\", \"width\": 60, \"height\": 60, \"boundary\": \"wrap\", \"cas\": [\"B24/S35\"]}}. Each CA starts from its rule's default seed (a random soup for Life-like rules) unless given one: {{\"cas\": [{{\"rule\": \"Life\", \"seed\": \"empty\"}}]}} is a blank field to draw on (reference {{\"what\": \"seeds\"}} lists the seeds, {{\"what\": \"tilings\"}} the tilings). Size: width and height (8 to 4096 unit cells each), or `cells` (about that many, up to {}), or `level` on aperiodic tilings. Or open one: `quickstart` (a Quick Start entry by handle, e.g. \"life\"), `pattern` (a library pattern on a grid sized for it, e.g. \"glider\"), `scene` (scene JSON, as world_status {{\"scene\": true}} returns it) or `path` (a scene file). Returns the world's status with each CA's id. Next: world_edit, world_step, world_read, world_render.",
                    crate::tools::MAX_CELLS
                )
            },
            schema: || {
                let mut p = schema::props(&input_schema::<NewWorldIn>());
                p.extend(schema::props(&input_schema::<OpenIn>()));
                schema::retell(
                    &mut p,
                    "cas",
                    "The CAs, in order: rule names or notation (\"Life\", \"B3/S23\"), or CA objects as in a scene ({\"rule\": \"Life\", \"seed\": \"random:0.2\"}); one string is one CA.",
                );
                schema::retell(
                    &mut p,
                    "pattern",
                    "A library pattern (id, prefix or name, e.g. `glider`) alone on a wrapped grid sized for it, in a rule it runs in.",
                );
                object(p, &[])
            },
            run: world_new,
        },
        Tool {
            name: "world_status",
            title: "World status",
            read_only: false,
            destructive: false,
            description: || {
                "Reports the current world: tiling and grid size (width x height for coordinates), generation, state hash, whether it is running, empty, frozen, or repeating with a period (checked exactly), population per field, and the CAs with their ids and rules. Cheap; call it whenever you need ids or sizes. {\"scene\": true} returns the world's scene JSON instead (world_new {\"scene\": ...} reproduces this exact state); {\"save_path\": \"glider.json\"} also writes it to a file (the app and `jetcarp run` open it).".into()
            },
            schema: || {
                let mut p = schema::props(&input_schema::<StatusIn>());
                let save = schema::props(&input_schema::<SaveIn>());
                p.insert(
                    "save_path".into(),
                    json!({"type": "string", "description": save["path"]["description"]}),
                );
                object(p, &[])
            },
            run: world_status,
        },
        Tool {
            name: "world_read",
            title: "Read cells, a cell, or the objects",
            read_only: false,
            destructive: false,
            description: || {
                "Looks at a CA's field. With no arguments: a text picture of the smallest box around the non-empty cells (one row per line, prefixed by its row number y, with a ruler of column numbers; '.' empty, 'O' live, digits for higher states or sandpile grains), the live-cell count and the bounding box. x, y, width, height show a fixed region; {\"format\": \"list\"} gives [x, y, state] triples instead. {\"format\": \"objects\"} runs the object finder: every still life, oscillator, spaceship and gun in the field, named and classed, with where it is (`catalog`: true also stores each distinct one in the library). {\"cell\": {\"x\": 3, \"y\": 4}} describes one cell: its tile type, centre, value in every field, and its neighbours in clockwise order (the neighbour layout on hexagons, triangles and other tilings, before you draw).".into()
            },
            schema: || {
                let read = schema::describe(
                    input_schema::<ReadIn>(),
                    "format",
                    "\"picture\" (default), \"list\" of non-empty cells as [x, y, state], or \"objects\" (the object finder).",
                );
                let mut p = schema::props(&read);
                p.insert(
                    "format".into(),
                    words(
                        &["picture", "list", "objects"],
                        "\"picture\" (default), \"list\" of non-empty cells as [x, y, state], or \"objects\" (the object finder).",
                    ),
                );
                let objs = schema::props(&input_schema::<ObjectsIn>());
                for k in ["max_period", "catalog"] {
                    let mut v = objs[k].clone();
                    let d = v["description"].as_str().unwrap_or("").to_string();
                    v["description"] = json!(format!("With format objects: {d}"));
                    p.insert(k.into(), v);
                }
                p.insert(
                    "cell".into(),
                    schema::op(
                        input_schema::<InspectIn>(),
                        Some("One cell to describe instead: {\"x\", \"y\"} or {\"index\"}."),
                    ),
                );
                object(p, &[])
            },
            run: world_read,
        },
        Tool {
            name: "world_edit",
            title: "Draw, paste, reseed, or apply commands",
            read_only: false,
            destructive: false,
            description: || {
                "Changes the world through logged commands (world_status {\"scene\": true} replays them). Give one of: `cells` to draw a text picture, RLE, [x, y] cells or cell indices, e.g. {\"cells\": {\"pattern\": \".O.\\n..O\\nOOO\", \"x\": 10, \"y\": 10}} (a glider moving down-right; x = column, y = row, (0, 0) is the top-left cell); `paste` to place a library pattern, e.g. {\"paste\": {\"id\": \"glider\", \"x\": 10, \"y\": 10, \"orientation\": 1}}; `reseed` to put new life into a settled world, now ({\"reseed\": {\"mode\": \"blobs\"}}) or by policies; or `commands`, the app's own commands in order, e.g. {\"commands\": [{\"op\": \"add_ca\", \"ca\": {\"rule\": \"Ant RL\", \"seed\": \"ants:3\"}}]}; reference {\"what\": \"commands\"} lists them. `ca` picks the CA (id or name; default the first) for cells, paste and reseed. Next: world_step, world_read.".into()
            },
            schema: || {
                let ca = schema::props(&input_schema::<DrawIn>())["ca"].clone();
                let apply = schema::props(&input_schema::<ApplyIn>());
                let mut commands = apply["commands"].clone();
                commands["type"] = json!("array");
                commands["items"] = json!({"type": "object"});
                object(
                    named(vec![
                        ("ca", ca),
                        (
                            "cells",
                            schema::without(
                                schema::op(
                                    input_schema::<DrawIn>(),
                                    Some(
                                        "Cells to draw into a CA's field: one of `pattern`, `rle`, `cells` or `indices` (or only `clear_first`). Only the live cells of a pattern are written; `clear_first` empties the field first, and `erase` clears the cells instead. On sandpiles the values are grains added.",
                                    ),
                                ),
                                &["ca"],
                            ),
                        ),
                        (
                            "paste",
                            schema::without(
                                schema::op(
                                    input_schema::<PasteIn>(),
                                    Some(
                                        "A library pattern to paste, and where. The world must use the pattern's tiling; on aperiodic patches give `index`, or `at` \"original\" (where it was found).",
                                    ),
                                ),
                                &["ca"],
                            ),
                        ),
                        (
                            "reseed",
                            schema::without(
                                schema::op(
                                    input_schema::<ReseedIn>(),
                                    Some(
                                        "Reseed now, set the automatic policies, or report them. `mode` reseeds now; with neither `mode` nor `policies` it reports the policies and the recent activity. Policies: {\"policies\": [{\"mode\": \"rain\", \"when\": {\"kind\": \"settled\"}, \"cooldown\": 300}]}.",
                                    ),
                                ),
                                &["ca"],
                            ),
                        ),
                        ("commands", commands),
                    ]),
                    &[],
                )
            },
            run: world_edit,
        },
        Tool {
            name: "world_step",
            title: "Run generations",
            read_only: false,
            destructive: false,
            description: || {
                format!(
                    "Advances the world by `generations` (default 1; at most {} per call, and at most 2 billion cell updates, cells x generations); every enabled CA steps together. Returns the new generation, populations and status (running, empty, frozen, or repeating with its period). {{\"stop_when_settled\": true}} stops as soon as the world is empty, frozen or exactly periodic (up to 5000 generations when no count is given). Stops early when the client cancels the call.",
                    jetcarp_search::dashboard::digits(crate::tools::MAX_GENS)
                )
            },
            schema: || object(schema::props(&input_schema::<StepIn>()), &[]),
            run: world_step,
        },
        Tool {
            name: "world_render",
            title: "Render a picture",
            read_only: false,
            destructive: false,
            description: || {
                "Renders the whole world as a PNG and returns it, so you can see it: empty cells dark, live cells in the CA's palette, brighter for young cells. The user sees it only where the client shows tool images (terminal clients don't), so pass `save_path` when they want the picture: it also writes the PNG (scene embedded) and says where. {\"wallpaper\": {\"size\": \"4K\"}} writes desktop-sized pictures instead and returns their paths (also on a fresh grid sized for the picture, or one picture per monitor, cut so they line up).".into()
            },
            schema: || {
                let mut p = schema::props(&input_schema::<RenderIn>());
                p.insert(
                    "wallpaper".into(),
                    schema::without(
                        schema::op(
                            input_schema::<WallpaperIn>(),
                            Some("Write desktop-sized pictures of the world instead. They go under the output folder's wallpapers/ unless `out` is given. E.g. {\"size\": \"4K\"}; {\"size\": \"2560x1440\", \"fresh\": true, \"cell_px\": 3, \"gens\": 300, \"palette\": \"Paper\"} (a new grid of the world's tiling sized for the picture); {\"monitors\": \"2560x1440+0+0,1920x1080+2560+180\", \"split\": true} (one picture per monitor, cut so they line up)."),
                        ),
                        &["source", "gpu"],
                    ),
                );
                object(p, &[])
            },
            run: world_render,
        },
    ]
}

fn world_new(c: &mut Call) -> Res {
    let a = c.args;
    let open = OPEN_KEYS
        .iter()
        .any(|k| a.get(*k).is_some_and(|v| !v.is_null()));
    if open {
        let extra: Vec<&str> = NEW_KEYS
            .iter()
            .copied()
            .filter(|k| a.get(*k).is_some_and(|v| !v.is_null()))
            .collect();
        if !extra.is_empty() {
            return Err(format!(
                "world_new: `{}` builds a new world, while scene, path, quickstart and pattern open one; give one or the other",
                extra.join("`, `")
            ));
        }
        let s = input_schema::<OpenIn>();
        let input: OpenIn = schema::read(&s, "world_new", schema::pick(a, OPEN_KEYS))?;
        let out = WorldOpen::run(ctx(), c.host, input, &Quiet).map_err(err)?;
        return Ok(ToolOutput::text(out.text()));
    }
    let s = input_schema::<NewWorldIn>();
    let input: NewWorldIn = schema::read(&s, "world_new", schema::pick(a, NEW_KEYS))?;
    let out = WorldNew::run(ctx(), c.host, input, &Quiet).map_err(err)?;
    let mut t = ToolOutput::text(out.text());
    if c.host.world().cas.is_empty() {
        t.content.push(json!({"type": "text", "text": "No CAs yet: add one with world_edit {\"commands\": [{\"op\": \"add_ca\", \"ca\": {\"rule\": \"Life\"}}]}, or give `cas`."}));
    }
    Ok(t)
}

fn world_status(c: &mut Call) -> Res {
    let a = c.args;
    let scene = schema::read::<StatusIn>(
        &input_schema::<StatusIn>(),
        "world_status",
        schema::pick(a, &["scene"]),
    )?;
    let mut saved = None;
    if let Some(p) = a.get("save_path").and_then(Value::as_str) {
        let s = WorldSave::run(ctx(), c.host, SaveIn { path: p.into() }, &Quiet).map_err(err)?;
        saved = Some(s.text());
    } else if a.get("save_path").is_some_and(|v| !v.is_null()) {
        return Err("world_status: `save_path` is the scene file's path, as text".into());
    }
    let out = WorldStatusOp::run(ctx(), c.host, scene, &Quiet).map_err(err)?;
    let mut t = ToolOutput::text(out.text());
    if let Some(s) = saved {
        t.content.insert(0, json!({"type": "text", "text": s}));
    }
    Ok(t)
}

fn world_read(c: &mut Call) -> Res {
    let a = c.args;
    if let Some(cell) = a.get("cell").filter(|v| !v.is_null()) {
        let others: Vec<&str> = [
            "x",
            "y",
            "width",
            "height",
            "format",
            "max_period",
            "catalog",
        ]
        .into_iter()
        .filter(|k| a.get(*k).is_some_and(|v| !v.is_null()))
        .collect();
        if !others.is_empty() {
            return Err(format!(
                "world_read: `cell` describes one cell; leave out `{}`",
                others.join("`, `")
            ));
        }
        let input: InspectIn = schema::read(
            &input_schema::<InspectIn>(),
            "world_read cell",
            cell.clone(),
        )?;
        let out = WorldInspect::run(ctx(), c.host, input, &Quiet).map_err(err)?;
        return Ok(ToolOutput::json(&op::to_value(&out)));
    }
    if a.get("format").and_then(Value::as_str) == Some("objects") {
        let others: Vec<&str> = ["x", "y", "width", "height"]
            .into_iter()
            .filter(|k| a.get(*k).is_some_and(|v| !v.is_null()))
            .collect();
        if !others.is_empty() {
            return Err(format!(
                "world_read: format objects looks at the whole field; leave out `{}`",
                others.join("`, `")
            ));
        }
        let input: ObjectsIn = schema::read(
            &input_schema::<ObjectsIn>(),
            "world_read",
            schema::pick(a, &["ca", "max_period", "catalog"]),
        )?;
        let out = WorldObjects::run(ctx(), c.host, input, &Quiet).map_err(err)?;
        return Ok(ToolOutput::text(out.text()));
    }
    for k in ["max_period", "catalog"] {
        if a.get(k).is_some_and(|v| !v.is_null()) {
            return Err(format!(
                "world_read: `{k}` goes with {{\"format\": \"objects\"}}"
            ));
        }
    }
    let input: ReadIn = schema::read(
        &input_schema::<ReadIn>(),
        "world_read",
        schema::pick(a, &["ca", "x", "y", "width", "height", "format"]),
    )?;
    let out = WorldRead::run(ctx(), c.host, input, &Quiet).map_err(err)?;
    Ok(ToolOutput::text(out.text()))
}

/// An action's input: its own object, with the tool's `ca` put in when the action takes one.
fn with_ca(c: &Call, part: &Value, takes_ca: bool) -> Result<Value, String> {
    let mut v = match part {
        Value::Null => json!({}),
        Value::Object(_) => part.clone(),
        Value::String(s) if s.trim_start().starts_with('{') => {
            serde_json::from_str(s).map_err(|e| format!("world_edit: not a JSON object ({e})"))?
        }
        other => return Err(format!("world_edit: expected an object, got {other}")),
    };
    if let Some(ca) = c.args.get("ca").filter(|v| !v.is_null()) {
        if !takes_ca {
            return Err(
                "world_edit: `ca` goes with cells, paste or reseed; commands name their CA by id"
                    .into(),
            );
        }
        if v.get("ca").is_some() {
            return Err("world_edit: give `ca` once".into());
        }
        v["ca"] = ca.clone();
    }
    Ok(v)
}

fn world_edit(c: &mut Call) -> Res {
    let a = c.args;
    let which = schema::one_of("world_edit", a, &["cells", "paste", "reseed", "commands"])?
        .ok_or("world_edit: give one of `cells` (draw), `paste` (a library pattern), `reseed`, or `commands`")?;
    let part = a[which].clone();
    let text = match which {
        "cells" => {
            let input: DrawIn = schema::read(
                &input_schema::<DrawIn>(),
                "world_edit cells",
                with_ca(c, &part, true)?,
            )?;
            WorldDraw::run(ctx(), c.host, input, &Quiet)
                .map_err(err)?
                .text()
        }
        "paste" => {
            let input: PasteIn = schema::read(
                &input_schema::<PasteIn>(),
                "world_edit paste",
                with_ca(c, &part, true)?,
            )?;
            WorldPaste::run(ctx(), c.host, input, &Quiet)
                .map_err(err)?
                .text()
        }
        "reseed" => {
            let input: ReseedIn = schema::read(
                &input_schema::<ReseedIn>(),
                "world_edit reseed",
                with_ca(c, &part, true)?,
            )?;
            let out = WorldReseed::run(ctx(), c.host, input, &Quiet).map_err(err)?;
            serde_json::to_string_pretty(&op::to_value(&out)).unwrap_or_default()
        }
        _ => {
            with_ca(c, &json!({}), false)?;
            let input = ApplyIn {
                commands: part.clone(),
            };
            WorldApply::run(ctx(), c.host, input, &Quiet)
                .map_err(err)?
                .text()
        }
    };
    Ok(ToolOutput::text(text))
}

fn world_step(c: &mut Call) -> Res {
    let input: StepIn = schema::read(&input_schema::<StepIn>(), "world_step", c.args.clone())?;
    let p = c.callbacks();
    let out = WorldStep::run(ctx(), c.host, input, &p).map_err(err)?;
    Ok(ToolOutput::text(out.text()))
}

fn world_render(c: &mut Call) -> Res {
    let a = c.args;
    if let Some(w) = a.get("wallpaper").filter(|v| !v.is_null()) {
        let others: Vec<&str> = ["max_side", "outlines", "save_path"]
            .into_iter()
            .filter(|k| a.get(*k).is_some_and(|v| !v.is_null()))
            .collect();
        if !others.is_empty() {
            return Err(format!(
                "world_render: with `wallpaper`, give its options inside it (leave out `{}`)",
                others.join("`, `")
            ));
        }
        let s = schema::without(input_schema::<WallpaperIn>(), &["source", "gpu"]);
        let input: WallpaperIn = schema::read(&s, "world_render wallpaper", w.clone())?;
        if input.source.is_some() {
            return Err("world_render wallpaper: open the scene with world_new first".into());
        }
        let out = WallpaperExport::run(ctx(), c.host, input, &Quiet).map_err(err)?;
        return Ok(ToolOutput::json(&op::to_value(&out)));
    }
    let input: RenderIn = schema::read(
        &input_schema::<RenderIn>(),
        "world_render",
        schema::pick(a, &["max_side", "outlines", "save_path"]),
    )?;
    let out = WorldRender::run(ctx(), c.host, input, &Quiet).map_err(err)?;
    Ok(ToolOutput::with_png(out.text(), &out.png))
}
