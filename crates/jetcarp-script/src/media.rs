//! Media and website export bindings: `lib.media(#{...})` / `media(#{...})` render frames, an
//! animation, a poster, and a thumbnail for catalog targets (`jetcarp_core::media`);
//! `w.media(#{...})` renders a world from its current state into an output folder; and
//! `lib.site_export(#{...})` writes the incremental website export (`jetcarp_core::website`).

use crate::Ctx;
use crate::api::{LibHandle, from_json, to_json};
use crate::sim::Shared;
use jetcarp_api::args::{check_keys, get_bool, get_f64, get_str, get_u64};
use jetcarp_core::library::{CurationState, Library, PatternClass, QsStatus};
use jetcarp_core::media::{
    self, BatchReport, MediaFormat, MediaMode, MediaOptions, MediaRow, MediaStyle, MediaTarget,
    MissingFilter,
};
use jetcarp_core::website::{self, ExportFilter, ExportOptions};
use rhai::{Dynamic, Engine, EvalAltResult, Map};
use serde_json::{Value, json};
use std::sync::Arc;

type R<T> = Result<T, Box<EvalAltResult>>;

const MEDIA_KEYS: &[&str] = &[
    "pattern",
    "quickstart",
    "ant",
    "missing",
    "class",
    "limit",
    "mode",
    "format",
    "periods",
    "gens",
    "every",
    "frames",
    "force",
    "palette",
];

const WORLD_KEYS: &[&str] = &["gens", "every", "format", "frames", "folder", "palette"];

const SITE_KEYS: &[&str] = &[
    "to",
    "state",
    "min_score",
    "min_rating",
    "min_osc_period",
    "class",
    "quickstart",
    "frames",
    "render",
    "palette",
];

fn e(s: String) -> Box<EvalAltResult> {
    s.into()
}

fn map_json(m: &Map) -> R<Value> {
    to_json(&Dynamic::from_map(m.clone()))
}

/// Text or a list of text.
fn list(v: &Value, key: &str) -> Vec<String> {
    match v.get(key) {
        Some(Value::String(s)) => s
            .split(',')
            .map(|x| x.trim().to_string())
            .filter(|x| !x.is_empty())
            .collect(),
        Some(Value::Array(a)) => a
            .iter()
            .filter_map(|x| x.as_str().map(String::from))
            .collect(),
        _ => Vec::new(),
    }
}

fn classes(v: &Value) -> Result<Vec<PatternClass>, String> {
    list(v, "class")
        .iter()
        .map(|c| PatternClass::parse(c))
        .collect()
}

fn options(v: &Value) -> Result<MediaOptions, String> {
    Ok(MediaOptions {
        mode: MediaMode::parse(get_str(v, "mode")?.unwrap_or("auto"))?,
        format: MediaFormat::parse(get_str(v, "format")?.unwrap_or("auto"))?,
        periods: get_u64(v, "periods")?.unwrap_or(1).max(1),
        gens: get_u64(v, "gens")?,
        every: get_u64(v, "every")?.unwrap_or(1).max(1),
        frames: get_bool(v, "frames")?.unwrap_or(true),
        force: get_bool(v, "force")?.unwrap_or(false),
        style: MediaStyle::with_palette(get_str(v, "palette")?)?,
    })
}

fn files(rows: &[MediaRow]) -> Value {
    let mut m = serde_json::Map::new();
    for r in rows {
        let k = match r.kind.as_str() {
            "gif" | "webp" => "anim",
            k => k,
        };
        m.insert(k.into(), json!(r.abs_path.display().to_string()));
    }
    m.insert(
        "width".into(),
        json!(rows.first().map(|r| r.width).unwrap_or(0)),
    );
    m.insert(
        "height".into(),
        json!(rows.first().map(|r| r.height).unwrap_or(0)),
    );
    Value::Object(m)
}

fn report_json(r: &BatchReport) -> Value {
    json!({
        "written": r.written(),
        "skipped": r.skipped(),
        "done": r.done.iter().map(|d| json!({
            "kind": d.target_kind, "id": d.target_id, "mode": d.mode.name(),
            "written": d.written, "files": files(&d.rows),
        })).collect::<Vec<_>>(),
        "failed": r.failed.iter().map(|(k, id, x)| json!({"kind": k, "id": id, "error": x}))
            .collect::<Vec<_>>(),
    })
}

/// `lib.media(#{...})`.
pub fn media_into(lib: &Library, v: &Value) -> Result<Value, String> {
    check_keys(v, MEDIA_KEYS, "media")?;
    let o = options(v)?;
    let r = if get_bool(v, "missing")?.unwrap_or(false) {
        let cls = classes(v)?;
        media::render_missing(
            lib,
            &MissingFilter {
                patterns: true,
                quickstart: cls.is_empty(),
                ants: cls.is_empty(),
                classes: cls,
                limit: get_u64(v, "limit")?.unwrap_or(0) as usize,
            },
            &o,
        )?
    } else {
        let t = if let Some(p) = get_str(v, "pattern")? {
            MediaTarget::Pattern(p.into())
        } else if let Some(q) = get_str(v, "quickstart")? {
            MediaTarget::QuickStart(q.into())
        } else if let Some(a) = get_str(v, "ant")? {
            MediaTarget::Ant(a.into())
        } else {
            return Err("media: give pattern, quickstart, ant, or missing: true".into());
        };
        media::render_many(lib, &[t], &o)?
    };
    Ok(report_json(&r))
}

/// `lib.site_export(#{...})`; `to` is already a checked, writable folder.
pub fn site_into(lib: &Library, v: &Value, to: &std::path::Path) -> Result<Value, String> {
    check_keys(v, SITE_KEYS, "site_export")?;
    let mut f = ExportFilter::default();
    let states = list(v, "state");
    if !states.is_empty() {
        f.states = states
            .iter()
            .map(|s| CurationState::parse(s))
            .collect::<Result<_, _>>()?;
    }
    f.min_score = get_f64(v, "min_score")?;
    f.min_rating = get_u64(v, "min_rating")?.map(|x| x.min(5) as u8);
    f.min_osc_period = get_u64(v, "min_osc_period")?;
    f.classes = classes(v)?;
    if v.get("quickstart").is_some() {
        f.quickstart = list(v, "quickstart")
            .iter()
            .filter(|s| s.as_str() != "none")
            .map(|s| QsStatus::parse(s))
            .collect::<Result<_, _>>()?;
    }
    let o = ExportOptions {
        filter: f,
        frames: get_bool(v, "frames")?.unwrap_or(false),
        render: get_bool(v, "render")?.unwrap_or(true),
        media: MediaOptions {
            style: MediaStyle::with_palette(get_str(v, "palette")?)?,
            ..Default::default()
        },
    };
    let r = website::export(lib, to, &o)?;
    serde_json::to_value(&r).map_err(|x| x.to_string())
}

pub(crate) fn register(eng: &mut Engine, ctx: &Arc<Ctx>) {
    eng.register_fn("media", |l: &mut LibHandle, m: Map| -> R<Map> {
        let v = map_json(&m)?;
        Ok(from_json(&media_into(&l.0, &v).map_err(e)?).cast::<Map>())
    });
    let c = ctx.clone();
    eng.register_fn("media", move |m: Map| -> R<Map> {
        let lib = match &c.opts.library {
            Some(p) => Library::open(p.clone()),
            None => Library::open_default(),
        };
        let v = map_json(&m)?;
        Ok(from_json(&media_into(&lib, &v).map_err(e)?).cast::<Map>())
    });
    // A world from its current state: w.media(#{ gens, every, format, frames, folder }).
    let c = ctx.clone();
    eng.register_fn("media", move |w: &mut Shared, m: Map| -> R<Map> {
        let v = map_json(&m)?;
        check_keys(&v, WORLD_KEYS, "w.media").map_err(e)?;
        let o = MediaOptions {
            mode: MediaMode::Seed,
            ..options(&v).map_err(e)?
        };
        let dir = match get_str(&v, "folder").map_err(e)? {
            Some(f) => c.writable_dir(f).map_err(e)?,
            None => c.out_dir("media").map_err(e)?,
        };
        let scene = w.lock().world.to_scene();
        let rows = media::render_scene_to(&scene, &dir, &o).map_err(e)?;
        let mut out = files(&rows);
        out["folder"] = json!(dir.display().to_string());
        out["frames_count"] = json!(rows.first().map(|r| r.frames).unwrap_or(0));
        Ok(from_json(&out).cast::<Map>())
    });
    let c = ctx.clone();
    eng.register_fn("site_export", move |l: &mut LibHandle, m: Map| -> R<Map> {
        let v = map_json(&m)?;
        let to = get_str(&v, "to")
            .map_err(e)?
            .ok_or_else(|| e("site_export: give `to`, the export folder".into()))?;
        let dir = c.writable_dir(to).map_err(e)?;
        Ok(from_json(&site_into(&l.0, &v, &dir).map_err(e)?).cast::<Map>())
    });
}
