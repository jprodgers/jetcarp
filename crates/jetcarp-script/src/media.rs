//! Media and website export bindings: `lib.media(#{...})` / `media(#{...})` render frames, an
//! animation, a poster, and a thumbnail for catalog targets; `w.media(#{...})` renders a world
//! from its current state into an output folder; and `lib.site_export(#{...})` writes the
//! incremental website export. All three are the operations `media.render` and `site.export`.

use crate::Ctx;
use crate::api::{LibHandle, from_json, to_json};
use crate::sim::Shared;
use jetcarp_api::args::{check_keys, get_bool, get_str, get_u64};
use jetcarp_api::op::{self, Op, Quiet};
use jetcarp_api::ops::media::{MediaRender, MediaRenderIn, SiteExport, SiteIn};
use jetcarp_core::library::Library;
use rhai::{Dynamic, Engine, EvalAltResult, Map};
use serde_json::{Value, json};
use std::sync::Arc;

type R<T> = Result<T, Box<EvalAltResult>>;

const WORLD_KEYS: &[&str] = &["gens", "every", "format", "frames", "folder", "palette"];

fn e(s: String) -> Box<EvalAltResult> {
    s.into()
}

fn api(e: jetcarp_api::ApiError) -> Box<EvalAltResult> {
    e.to_string().into()
}

fn map_json(m: &Map) -> R<Value> {
    to_json(&Dynamic::from_map(m.clone()))
}

/// `media.render` on catalog `lib` with the options of a script map.
fn media_into(c: &Ctx, lib: Library, m: &Map) -> R<Map> {
    let input = MediaRenderIn::from_map(&map_json(m)?).map_err(e)?;
    let r = MediaRender::run(&c.api(Some(lib)), input, &Quiet).map_err(api)?;
    Ok(from_json(&op::to_value(&r)).cast::<Map>())
}

pub(crate) fn register(eng: &mut Engine, ctx: &Arc<Ctx>) {
    let c = ctx.clone();
    eng.register_fn("media", move |l: &mut LibHandle, m: Map| -> R<Map> {
        media_into(&c, l.0.clone(), &m)
    });
    let c = ctx.clone();
    eng.register_fn("media", move |m: Map| -> R<Map> {
        let lib = match &c.opts.library {
            Some(p) => Library::open(p.clone()),
            None => Library::open_default(),
        };
        media_into(&c, lib, &m)
    });
    // A world from its current state: w.media(#{ gens, every, format, frames, folder }).
    let c = ctx.clone();
    eng.register_fn("media", move |w: &mut Shared, m: Map| -> R<Map> {
        let v = map_json(&m)?;
        check_keys(&v, WORLD_KEYS, "w.media").map_err(e)?;
        let input = MediaRenderIn {
            mode: Some("seed".into()),
            format: get_str(&v, "format").map_err(e)?.map(String::from),
            gens: get_u64(&v, "gens").map_err(e)?,
            every: get_u64(&v, "every").map_err(e)?,
            frames: get_bool(&v, "frames").map_err(e)?,
            palette: get_str(&v, "palette").map_err(e)?.map(String::from),
            ..Default::default()
        };
        // Checked before a folder is made for it.
        input.media_options().map_err(api)?;
        let dir = match get_str(&v, "folder").map_err(e)? {
            Some(f) => c.writable_dir(f).map_err(e)?,
            None => c.out_dir("media").map_err(e)?,
        };
        let scene = w.lock().world.to_scene();
        let input = MediaRenderIn {
            scene: Some(serde_json::to_value(&scene).map_err(|x| e(x.to_string()))?),
            out: Some(dir.display().to_string()),
            ..input
        };
        let r = MediaRender::run(&c.api(None), input, &Quiet).map_err(api)?;
        let mut out = r.files.unwrap_or_else(|| json!({}));
        out["folder"] = json!(dir.display().to_string());
        out["frames_count"] = json!(r.frames_count.unwrap_or(0));
        Ok(from_json(&out).cast::<Map>())
    });
    let c = ctx.clone();
    eng.register_fn("site_export", move |l: &mut LibHandle, m: Map| -> R<Map> {
        let v = map_json(&m)?;
        let to = get_str(&v, "to")
            .map_err(e)?
            .ok_or_else(|| e("site_export: give `to`, the export folder".into()))?;
        let dir = c.writable_dir(to).map_err(e)?;
        let input = SiteIn::from_map(&v, &dir.display().to_string()).map_err(e)?;
        let r = SiteExport::run(&c.api(Some(l.0.clone())), input, &Quiet).map_err(api)?;
        Ok(from_json(&op::to_value(&r)).cast::<Map>())
    });
}
