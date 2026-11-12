//! Large sandpile bindings: `sandpile_pile(#{...})` builds one pile (a catalog job, through the
//! operation `search.start`: `crate::search`) into its own folder under `<out>/piles/`, and
//! `sandpile_piles(#{...})` grows the same N on several tilings and writes a contact sheet of
//! their previews.

use crate::{Ctx, Progress};
use jetcarp_api::out;
use jetcarp_core::pile;
use jetcarp_core::render::Image;
use rhai::{Array, Dynamic, Engine, EvalAltResult, INT, Map};
use serde_json::Value;
use std::path::PathBuf;
use std::sync::Arc;

type R<T> = Result<T, Box<EvalAltResult>>;

fn grains_of(v: &Dynamic) -> R<u64> {
    if let Ok(n) = v.as_int() {
        if n < 0 {
            return Err("grains must be 0 or more".into());
        }
        return Ok(n as u64);
    }
    if let Ok(f) = v.as_float() {
        if f < 0.0 || f.fract() != 0.0 || f >= 1.8446744073709552e19 {
            return Err(format!("grains: {f} is not a whole number in range").into());
        }
        return Ok(f as u64);
    }
    if let Ok(s) = v.clone().into_string() {
        return pile::parse_grains(&s).map_err(|e| e.into());
    }
    Err("grains: a number or text like \"1e7\" or \"2^30\"".into())
}

fn text(m: &Map, keys: &[&str]) -> Option<String> {
    keys.iter().find_map(|k| m.get(*k)).map(|v| v.to_string())
}

fn num(m: &Map, k: &str) -> R<Option<f64>> {
    match m.get(k) {
        None => Ok(None),
        Some(v) => v
            .as_float()
            .or_else(|_| v.as_int().map(|i| i as f64))
            .map(Some)
            .map_err(|_| format!("sandpile_piles: '{k}' must be a number").into()),
    }
}

/// One pile: `crate::search::sandpile_pile`.
fn run_pile(ctx: &Arc<Ctx>, m: &Map, parent: Option<&PathBuf>) -> R<Map> {
    crate::search::sandpile_pile(ctx, m, parent.map(|p| p.as_path()))
}

/// The same N on several tilings, each in a subfolder, plus `sheet.png` of their previews.
fn run_piles(ctx: &Arc<Ctx>, m: &Map) -> R<Map> {
    let tilings: Vec<String> = match m.get("tilings") {
        Some(v) => v
            .clone()
            .into_typed_array::<Dynamic>()
            .map_err(|_| "sandpile_piles: tilings must be a list")?
            .iter()
            .map(|d| d.to_string())
            .collect(),
        None => [
            "square",
            "hex",
            "triangle",
            "trihex",
            "snubsq",
            "oct",
            "rhombitrihex",
            "cairo",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect(),
    };
    let thumb = num(m, "thumb")?.unwrap_or(320.0).max(16.0) as u32;
    let cols = num(m, "cols")?.unwrap_or(4.0).max(1.0) as u32;
    let grains = match m.get("grains") {
        Some(v) => grains_of(v)?,
        None => 10_000,
    };
    let root = ctx.root.join("piles");
    std::fs::create_dir_all(&root).map_err(|e| format!("{}: {e}", root.display()))?;
    let dir = out::make_out_dir(&root, &format!("sheet-{grains}"))?;
    ctx.add_out_dir(&dir);
    let mut base = m.clone();
    for k in ["tilings", "thumb", "cols"] {
        base.remove(k);
    }
    base.insert("preview".into(), (thumb as INT).into());
    let mut runs = Array::new();
    let mut thumbs: Vec<(String, Image)> = Vec::new();
    for (n, t) in tilings.iter().enumerate() {
        if ctx.stopped() {
            break;
        }
        ctx.progress(Progress {
            label: "piles".into(),
            done: n as u64,
            total: tilings.len() as u64,
            item: t.clone(),
            elapsed: ctx.elapsed(),
            eta: None,
        });
        let mut one = base.clone();
        one.insert("tiling".into(), t.clone().into());
        let r = run_pile(ctx, &one, Some(&dir))?;
        if let Some(p) = r.get("preview").and_then(|v| v.clone().into_string().ok())
            && let Ok(img) = read_png(&PathBuf::from(&p))
        {
            thumbs.push((t.clone(), img));
        }
        ctx.log(format!(
            "{t}: {} topplings, {:.1} s",
            r.get("topplings")
                .map(|v| v.to_string())
                .unwrap_or_default(),
            r.get("seconds")
                .and_then(|v| v.as_float().ok())
                .unwrap_or(0.0)
        ));
        runs.push(r.into());
    }
    let items: Vec<(jetcarp_core::sheet::Label, &Image)> = thumbs
        .iter()
        .map(|(t, img)| (vec![(t.clone(), [0xe6, 0xe6, 0xe6, 0xff])], img))
        .collect();
    let mut out = Map::new();
    if !items.is_empty() {
        // Thumbnails of different shapes: pad each to the thumbnail size, centred.
        let stage = jetcarp_core::render::Palette::builtin(
            &text(m, &["palette"]).unwrap_or_else(|| "Ember".into()),
        )
        .map(|p| p.stage_rgba())
        .unwrap_or(jetcarp_core::render::STAGE);
        let padded: Vec<Image> = items.iter().map(|(_, i)| pad_to(i, thumb, stage)).collect();
        let items: Vec<(jetcarp_core::sheet::Label, &Image)> = items
            .iter()
            .zip(&padded)
            .map(|((l, _), i)| (l.clone(), i))
            .collect();
        let sheet = jetcarp_core::sheet::labelled_sheet(&items, cols);
        let p = dir.join("sheet.png");
        jetcarp_core::render::write_png(
            &p,
            &sheet,
            &[("jetcarp:version", jetcarp_core::VERSION.into())],
        )?;
        out.insert("sheet".into(), p.display().to_string().into());
    }
    let summary: Vec<Value> = runs
        .iter()
        .map(|r| crate::api::to_json(r).unwrap_or(Value::Null))
        .collect();
    let jp = dir.join("piles.json");
    let summary = jetcarp_core::json_int::safe(Value::Array(summary));
    std::fs::write(&jp, serde_json::to_string_pretty(&summary).unwrap())
        .map_err(|e| format!("{}: {e}", jp.display()))?;
    out.insert("folder".into(), dir.display().to_string().into());
    out.insert("runs".into(), runs.into());
    Ok(out)
}

fn pad_to(img: &Image, side: u32, stage: jetcarp_core::render::Rgba) -> Image {
    let mut o = Image {
        width: side,
        height: side,
        rgba: stage.repeat((side * side) as usize),
    };
    let (dx, dy) = (
        (side.saturating_sub(img.width)) / 2,
        (side.saturating_sub(img.height)) / 2,
    );
    for y in 0..img.height.min(side) {
        for x in 0..img.width.min(side) {
            let s = ((y * img.width + x) * 4) as usize;
            let d = (((y + dy) * side + x + dx) * 4) as usize;
            o.rgba[d..d + 4].copy_from_slice(&img.rgba[s..s + 4]);
        }
    }
    o
}

fn read_png(p: &PathBuf) -> Result<Image, String> {
    let f = std::fs::File::open(p).map_err(|e| e.to_string())?;
    let mut dec = png::Decoder::new(std::io::BufReader::new(f));
    dec.set_transformations(png::Transformations::EXPAND);
    let mut r = dec.read_info().map_err(|e| e.to_string())?;
    let mut buf = vec![0; r.output_buffer_size().ok_or("png too large")?];
    let info = r.next_frame(&mut buf).map_err(|e| e.to_string())?;
    let rgba = match info.color_type {
        png::ColorType::Rgba => buf[..info.buffer_size()].to_vec(),
        png::ColorType::Rgb => buf[..info.buffer_size()]
            .chunks(3)
            .flat_map(|c| [c[0], c[1], c[2], 255])
            .collect(),
        _ => return Err("unexpected PNG colour type".into()),
    };
    Ok(Image {
        width: info.width,
        height: info.height,
        rgba,
    })
}

pub(crate) fn register(e: &mut Engine, ctx: &Arc<Ctx>) {
    let c = ctx.clone();
    e.register_fn("sandpile_pile", move |m: Map| run_pile(&c, &m, None));
    let c = ctx.clone();
    e.register_fn("sandpile_piles", move |m: Map| run_piles(&c, &m));
}
