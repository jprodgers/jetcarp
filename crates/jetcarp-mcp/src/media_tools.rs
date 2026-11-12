//! `media_render`: frames, an animation (GIF or animated WebP), a poster and a thumbnail of
//! library records (`media.render`), returning their paths and, on request, the poster inline.

use crate::schema::{self, object};
use crate::tools::{Call, Res, Tool, ToolOutput, ctx, err};
use jetcarp_api::op::{self, Op, input_schema};
use jetcarp_api::ops::media::{MediaRender, MediaRenderIn};
use serde_json::{Value, json};

pub(crate) fn tools() -> Vec<Tool> {
    vec![Tool {
        name: "media_render",
        title: "Render animations and posters",
        read_only: false,
        destructive: false,
        description: || {
            "Renders media and returns the file paths: per-step PNG frames, an animation (GIF when small, else animated WebP), a poster frame and a thumbnail, in one house style. Give one of `pattern` (alone on an empty field for one seamless period; spaceships in a window that moves with them, guns with their stream), `ant` (a highway's loop in a window moving with the ant, or with mode seed the run from step 0), `quickstart` (from its seed), `scene` (from its seed, into the output folder) or `missing` (every record whose media are missing or stale). Library records' media are stored once by content in the library's media/ folder, and up-to-date media are not rendered again. `poster`: true also returns the first poster inline, to look at. Examples: {\"pattern\": \"glider\", \"poster\": true}, {\"ant\": \"ant-...\", \"mode\": \"seed\", \"gens\": 300}, {\"missing\": true, \"class\": [\"spaceship\"], \"limit\": 20}.".into()
        },
        schema: || {
            let mut p = schema::props(&schema::without(
                input_schema::<MediaRenderIn>(),
                &["threads"],
            ));
            schema::retell(
                &mut p,
                "quickstart",
                "A Quick Start entry: a catalog one (`find:...`, media stored in the catalog) or a built-in one (`life`), rendered from its seed into `out`.",
            );
            schema::retell(
                &mut p,
                "palette",
                "A palette instead of the house style: a built-in name, or hex colours like `#103050-#4fc3ff`.",
            );
            p.insert(
                "poster".into(),
                json!({"type": "boolean", "description": "Also return the first target's poster inline (PNG)."}),
            );
            object(p, &[])
        },
        run: media_render,
    }]
}

fn media_render(c: &mut Call) -> Res {
    let mut a = c.args.clone();
    let poster = match a.as_object_mut().and_then(|o| o.remove("poster")) {
        None | Some(Value::Null) => false,
        Some(Value::Bool(b)) => b,
        Some(Value::String(s)) if s == "true" || s == "false" => s == "true",
        Some(v) => return Err(format!("media_render: `poster` is true or false, not {v}")),
    };
    let input: MediaRenderIn = schema::read(&input_schema::<MediaRenderIn>(), "media_render", a)?;
    let r = op::to_value(&MediaRender::run(ctx(), input, &c.callbacks()).map_err(err)?);
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
