//! Large sandpile bindings: `sandpile_pile(#{...})` builds one pile into its own folder under
//! `<out>/piles/`, and `sandpile_piles(#{...})` grows the same N on several tilings and writes a
//! contact sheet of their previews.

use crate::{Ctx, Progress, out};
use jetcarp_core::SizeSpec;
use jetcarp_core::pile::{self, Centre, Ctl, Pile, PileSpec, RunOptions, export::ExportOptions};
use jetcarp_core::render::Image;
use rhai::{Array, Dynamic, Engine, EvalAltResult, INT, Map};
use serde_json::{Value, json};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

type R<T> = Result<T, Box<EvalAltResult>>;

/// Keys `sandpile_pile` accepts.
const KEYS: &[&str] = &[
    "tiling",
    "grains",
    "neighborhood",
    "nb",
    "centre",
    "center",
    "level",
    "png",
    "px",
    "margin",
    "palette",
    "reverse",
    "preview",
    "folder",
    "gpu",
    "steps",
    "label",
];

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
            .map_err(|_| format!("sandpile_pile: '{k}' must be a number").into()),
    }
}

fn flag(m: &Map, k: &str, default: bool) -> R<bool> {
    match m.get(k) {
        None => Ok(default),
        Some(v) => v
            .as_bool()
            .map_err(|_| format!("sandpile_pile: '{k}' must be true or false").into()),
    }
}

/// One pile. Returns the summary map.
fn run_pile(ctx: &Arc<Ctx>, m: &Map, parent: Option<&PathBuf>) -> R<Map> {
    for k in m.keys() {
        if !KEYS.contains(&k.as_str()) {
            return Err(format!(
                "sandpile_pile: unknown key '{k}' (keys: {})",
                KEYS.join(", ")
            )
            .into());
        }
    }
    let tiling_id = text(m, &["tiling"]).unwrap_or_else(|| "square".into());
    let mut tiling = crate::spec::tiling_by_id(&tiling_id)?;
    if let Some(l) = num(m, "level")? {
        tiling.size = Some(SizeSpec::Level(l as u32));
    }
    let grains = match m.get("grains") {
        Some(v) => grains_of(v)?,
        None => 10_000,
    };
    let nb = pile::parse_nb(&text(m, &["neighborhood", "nb"]).unwrap_or_else(|| "edge".into()))?;
    let centre = Centre::parse(&text(m, &["centre", "center"]).unwrap_or_default())?;
    let spec = PileSpec {
        tiling,
        nb,
        grains,
        centre,
    };
    // The folder: `folder` (resumed if it holds step files), else a new one under piles/.
    let (mut p, dir) = match m.get("folder").map(|v| v.to_string()) {
        Some(f) => {
            let dir = ctx.writable_dir(&f)?;
            match pile::latest_step_file(&dir) {
                Some(last) => {
                    let p = pile::file::load(&last)?;
                    if p.spec != spec {
                        return Err(format!(
                            "sandpile_pile: {} holds a different pile; use a new folder",
                            dir.display()
                        )
                        .into());
                    }
                    ctx.log(format!("resuming {} from step {}", dir.display(), p.done));
                    (p, dir)
                }
                None => (Pile::new(spec)?, dir),
            }
        }
        None => {
            let root = parent.cloned().unwrap_or_else(|| ctx.root.join("piles"));
            std::fs::create_dir_all(&root).map_err(|e| format!("{}: {e}", root.display()))?;
            let label = text(m, &["label"]).unwrap_or_else(|| format!("{tiling_id}-{grains}"));
            let dir = out::make_out_dir(&root, &label)?;
            (Pile::new(spec)?, dir)
        }
    };
    if parent.is_none() {
        ctx.add_out_dir(&dir);
    }
    let png = m.get("png");
    // The colouring: given, else the folder's own (a resumed run keeps it), else Ember.
    let kept = pile::export::read_look_file(&dir);
    let palette = text(m, &["palette"])
        .or_else(|| kept.as_ref().map(|k| k.0.clone()))
        .unwrap_or_else(|| "Ember".into());
    jetcarp_core::render::builtin_palette(&palette).map_err(|e| format!("sandpile_pile: {e}"))?;
    let reverse = match m.get("reverse") {
        Some(_) => flag(m, "reverse", false)?,
        None => kept.is_some_and(|k| k.1),
    };
    pile::export::write_look_file(&dir, &palette, reverse)?;
    let export = ExportOptions {
        px_per_cell: num(m, "px")?.unwrap_or(2.0),
        margin: num(m, "margin")?.unwrap_or(4.0),
        palette,
        reverse,
        preview_side: num(m, "preview")?.unwrap_or(1024.0).max(0.0) as u32,
        png: png.is_none_or(|v| v.as_bool().unwrap_or(true)),
    };
    let opts = RunOptions {
        export,
        save_steps: flag(m, "steps", true)?,
        extra: json!({
            "tiling_id": tiling_id,
            "host": out::host_name(),
            "finished_at": out::now_rfc3339(),
            "script": true,
        }),
    };
    let mut gpu = if flag(m, "gpu", false)? {
        if pile::is_patch(&p.spec.tiling) {
            ctx.log(format!("{tiling_id}: aperiodic patches topple on the CPU"));
            None
        } else {
            match pile::gpu_toppler() {
                Some(t) => Some(t?),
                None => return Err("sandpile_pile: this host has no GPU toppler".into()),
            }
        }
    } else {
        None
    };
    let cancel = AtomicBool::new(false);
    let records = std::cell::RefCell::new(p.records.clone());
    let c2 = ctx.clone();
    let cancel_ref = &cancel;
    let label = format!("pile {tiling_id} {grains}");
    let mut ctl = Ctl {
        cancel: Some(&cancel),
        tick_every: 0.5,
        on_tick: Some(Box::new(|info, t, _| {
            if c2.stopped() {
                cancel_ref.store(true, Ordering::Relaxed);
            }
            c2.progress(Progress {
                label: "pile".into(),
                done: info.step.saturating_sub(1) as u64,
                total: info.steps as u64,
                item: format!(
                    "{label}: step {}/{} ({} grains), {} sweeps",
                    info.step, info.steps, info.grains, t.sweeps
                ),
                elapsed: c2.elapsed(),
                eta: pile::eta_seconds(&records.borrow(), info.steps, t.seconds),
            });
        })),
        ..Default::default()
    };
    if ctx.stopped() {
        return Err("stopped".into());
    }
    // Run step by step so cancelling is noticed between steps too.
    std::fs::create_dir_all(dir.join("steps")).map_err(|e| e.to_string())?;
    while !p.finished() {
        if ctx.stopped() {
            cancel.store(true, Ordering::Relaxed);
        }
        let r = match gpu.as_deref_mut() {
            Some(g) => p.next_step(Some(g), &mut ctl),
            None => p.next_step(None, &mut ctl),
        };
        let r = r.map_err(|e| {
            if cancel.load(Ordering::Relaxed) {
                format!(
                    "sandpile_pile stopped in step {}; resume with folder: \"{}\"",
                    p.done + 1,
                    dir.display()
                )
            } else {
                e
            }
        })?;
        records.borrow_mut().push(r);
        if opts.save_steps {
            pile::file::save(&p, &dir.join(format!("steps/step-{:02}.pile", p.done)))?;
            pile::write_growth(&p, &dir)?;
        }
    }
    drop(ctl);
    let mut quiet = Ctl::default();
    let opts = RunOptions {
        save_steps: false,
        ..opts
    };
    let s = pile::run_to_dir(&mut p, &dir, None, &mut quiet, &opts)?;
    if s.png.is_some() || s.preview.is_some() {
        ctx.files_written
            .fetch_add(1 + s.png.is_some() as u64, Ordering::Relaxed);
    }
    let (top, sweeps, secs) = p.totals();
    let mut out = Map::new();
    let put = |o: &mut Map, k: &str, v: Dynamic| {
        o.insert(k.into(), v);
    };
    put(&mut out, "folder", dir.display().to_string().into());
    put(
        &mut out,
        "png",
        s.png
            .map(|p| Dynamic::from(p.display().to_string()))
            .unwrap_or(Dynamic::UNIT),
    );
    put(
        &mut out,
        "preview",
        s.preview
            .map(|p| Dynamic::from(p.display().to_string()))
            .unwrap_or(Dynamic::UNIT),
    );
    put(&mut out, "tiling", tiling_id.into());
    put(&mut out, "grains", (grains as INT).into());
    put(&mut out, "steps", (p.steps as INT).into());
    put(
        &mut out,
        "topplings",
        i64::try_from(top).map_or(Dynamic::from(top as f64), Dynamic::from),
    );
    put(&mut out, "sweeps", (sweeps as INT).into());
    put(&mut out, "seconds", secs.into());
    let (w, h) = p.extent();
    put(&mut out, "width", w.into());
    put(&mut out, "height", h.into());
    if let Some((iw, ih)) = s.image_size {
        put(&mut out, "image_width", (iw as INT).into());
        put(&mut out, "image_height", (ih as INT).into());
    }
    put(
        &mut out,
        "backend",
        p.records
            .last()
            .map(|r| r.backend.clone())
            .unwrap_or_default()
            .into(),
    );
    Ok(out)
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
        jetcarp_core::render::write_png(&p, &sheet, &[("jetcarp:version", jetcarp_core::VERSION.into())])?;
        out.insert("sheet".into(), p.display().to_string().into());
    }
    let summary: Vec<Value> = runs
        .iter()
        .map(|r| crate::api::to_json(r).unwrap_or(Value::Null))
        .collect();
    let jp = dir.join("piles.json");
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
