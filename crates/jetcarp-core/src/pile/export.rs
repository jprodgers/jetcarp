//! Pictures of a pile: the pile's bounding box plus a margin at a chosen number of pixels per
//! cell, written to PNG in horizontal strips (so images far larger than memory work), a
//! downscaled preview, and the metadata JSON.

use super::{BoardRef, Pile, RunOptions, RunSummary};
use crate::geom::*;
use crate::render::{Image, Palette, Rgba, draw_line, fill_polygon, ramp_palette};
use rayon::prelude::*;
use serde_json::{Value, json};
use std::path::Path;
use std::sync::atomic::{AtomicU32, Ordering};

/// How to draw a pile.
#[derive(Clone, Debug)]
pub struct ExportOptions {
    /// Pixels across an average cell (the square root of its area). Below 1.5, each pixel
    /// averages the colours of the cells whose centres fall in it.
    pub px_per_cell: f64,
    /// Empty cells kept around the pile, in cells.
    pub margin: f64,
    /// A built-in palette name (`jetcarp palettes` lists them): its stops colour the heights, its
    /// background empty cells, its stage the area around the pile.
    pub palette: String,
    /// Run the palette the other way (height 1 at its last stop).
    pub reverse: bool,
    /// Longest side of `preview.png` (0 = none).
    pub preview_side: u32,
    /// Write the full `pile.png`.
    pub png: bool,
}

impl Default for ExportOptions {
    fn default() -> Self {
        ExportOptions {
            px_per_cell: 2.0,
            margin: 4.0,
            palette: "Ember".into(),
            reverse: false,
            preview_side: 1024,
            png: true,
        }
    }
}

/// How a pile is coloured: a palette (stops for heights, background for empty cells, stage
/// around the pile, outlines from the background) and the direction of its ramp.
#[derive(Clone, Debug, PartialEq)]
pub struct Look {
    pub palette: Palette,
    pub reverse: bool,
}

impl Look {
    /// A built-in palette by name (Ember when unknown).
    pub fn named(name: &str, reverse: bool) -> Look {
        Look {
            palette: Palette::builtin(name).unwrap_or_else(|| Palette::builtin("Ember").unwrap()),
            reverse,
        }
    }
    /// The look an export asks for.
    pub fn of(o: &ExportOptions) -> Look {
        Look::named(&o.palette, o.reverse)
    }
}

impl Default for Look {
    fn default() -> Self {
        Look::named("Ember", false)
    }
}

/// The colours a render needs: one per height, the stage, the outline.
pub struct Colors {
    pub lut: [Rgba; 256],
    pub stage: Rgba,
    pub outline: Rgba,
}

impl Colors {
    pub fn new(maxd: u8, look: &Look) -> Colors {
        Colors {
            lut: height_lut(maxd, look),
            stage: look.palette.stage_rgba(),
            outline: look.palette.outline_rgba(),
        }
    }
}

/// The colouring chosen for a run folder, kept in `look.json` so a run resumed from the folder
/// (`jetcarp pile --resume <folder>`, a script's `folder:`) exports with it.
pub fn write_look_file(dir: &Path, palette: &str, reverse: bool) -> Result<(), String> {
    let p = dir.join("look.json");
    let v = json!({"palette": palette, "reverse": reverse});
    std::fs::write(&p, serde_json::to_string_pretty(&v).unwrap())
        .map_err(|e| format!("{}: {e}", p.display()))
}

/// The folder's `look.json` (palette, reverse), if any.
pub fn read_look_file(dir: &Path) -> Option<(String, bool)> {
    let v: Value = serde_json::from_slice(&std::fs::read(dir.join("look.json")).ok()?).ok()?;
    Some((
        v["palette"].as_str()?.to_string(),
        v["reverse"].as_bool().unwrap_or(false),
    ))
}

/// Colours by height: 0 is the palette's background, 1 .. maxd−1 run along the palette (the
/// other way when reversed), and anything at or above the largest degree (only mid-run) is the
/// ramp's far end.
pub fn height_lut(maxd: u8, look: &Look) -> [Rgba; 256] {
    let m = maxd.max(2) as usize;
    let at = |f: f64| ramp_palette(&look.palette, if look.reverse { 1.0 - f } else { f });
    let mut lut = [at(1.0); 256];
    lut[0] = look.palette.background_rgba();
    for (h, c) in lut.iter_mut().enumerate().take(m).skip(1) {
        *c = at((h - 1) as f64 / (m - 2).max(1) as f64);
    }
    lut
}

fn max_degree(b: &BoardRef) -> u8 {
    match b {
        BoardRef::Lattice(l) => l.lat.max_degree(),
        BoardRef::Patch(p) => p.deg.iter().copied().max().unwrap_or(1),
    }
}

/// Square root of a mean cell's area.
pub fn cell_size(b: &BoardRef) -> f64 {
    match b {
        BoardRef::Lattice(l) => l.lat.cell_area().sqrt(),
        BoardRef::Patch(p) => {
            let t = &p.topo;
            (area(&t.shapes[0])).sqrt()
        }
    }
}

/// World bounding box of the nonzero cells (their polygons), widened by `margin` cells.
pub fn world_bbox(b: &BoardRef, margin: f64) -> Option<[f64; 4]> {
    let mut bb = [
        f64::INFINITY,
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::NEG_INFINITY,
    ];
    let mut grow = |p: &[P]| {
        let q = bbox(p);
        bb[0] = bb[0].min(q[0]);
        bb[1] = bb[1].min(q[1]);
        bb[2] = bb[2].max(q[2]);
        bb[3] = bb[3].max(q[3]);
    };
    match b {
        BoardRef::Lattice(l) => {
            let bx = l.nonzero_box()?;
            // Only cells on each row's ends can reach the box; scan rows for their extremes.
            let nt = l.lat.nt;
            for j in bx[1]..=bx[3] {
                for k in 0..nt {
                    let row = l.row(k, j).unwrap();
                    if let Some(a) = row.iter().position(|&x| x != 0) {
                        let z = row.iter().rposition(|&x| x != 0).unwrap();
                        grow(&l.lat.polygon(l.i0 + a as i64, j, k));
                        grow(&l.lat.polygon(l.i0 + z as i64, j, k));
                    }
                }
            }
        }
        BoardRef::Patch(p) => {
            let mut any = false;
            for (i, &h) in p.cur.iter().enumerate() {
                if h != 0 {
                    any = true;
                    grow(&p.topo.polygon(i));
                }
            }
            if !any {
                return None;
            }
        }
    }
    if !bb[0].is_finite() {
        return None;
    }
    let m = margin * cell_size(b);
    Some([bb[0] - m, bb[1] - m, bb[2] + m, bb[3] + m])
}

/// A view: world rectangle `rect` drawn at `s` pixels per world unit.
#[derive(Clone, Copy, Debug)]
pub struct View {
    pub rect: [f64; 4],
    pub s: f64,
    pub width: u32,
    pub height: u32,
}

impl View {
    pub fn new(rect: [f64; 4], s: f64) -> View {
        View {
            rect,
            s,
            width: (((rect[2] - rect[0]) * s).ceil() as u32).max(1),
            height: (((rect[3] - rect[1]) * s).ceil() as u32).max(1),
        }
    }
    /// Fit `rect` into `side` pixels on its longer side.
    pub fn fit(rect: [f64; 4], side: u32) -> View {
        let s = side as f64 / (rect[2] - rect[0]).max(rect[3] - rect[1]).max(1e-9);
        View::new(rect, s)
    }
}

/// Cells (with their outline transform) whose bounding box meets world rect `r`, in a callback.
fn for_cells_in(b: &BoardRef, r: [f64; 4], f: &mut dyn FnMut(Affine, usize, u8)) {
    match b {
        BoardRef::Lattice(l) => {
            let lat = &*l.lat;
            for k in 0..lat.nt {
                let (m, sh) = lat.types[k];
                let lb = bbox(
                    &lat.outlines[sh]
                        .iter()
                        .map(|&p| m.apply(p))
                        .collect::<Vec<_>>(),
                );
                let (a1x, a2x, a2y) = (lat.a1[0], lat.a2[0], lat.a2[1]);
                let ja = ((r[1] - lb[3]) / a2y).ceil() as i64;
                let jb = ((r[3] - lb[1]) / a2y).floor() as i64;
                for j in ja..=jb {
                    let ia = ((r[0] - j as f64 * a2x - lb[2]) / a1x).ceil() as i64;
                    let ib = ((r[2] - j as f64 * a2x - lb[0]) / a1x).floor() as i64;
                    let row = l.row(k, j);
                    for i in ia..=ib {
                        let li = i - l.i0;
                        let h = match row {
                            Some(row) if li >= 0 && (li as usize) < row.len() => row[li as usize],
                            _ => 0,
                        };
                        f(lat.transform(i, j, k), sh, h);
                    }
                }
            }
        }
        BoardRef::Patch(p) => {
            let t = &p.topo;
            // Tiles sorted by their first corner's y; a tile lies within `reach` of that corner.
            let reach = 4.0 * cell_size(b);
            let idx = p.y_index();
            let lo = idx.partition_point(|e| (e.0 as f64) < r[1] - reach);
            for &(y, i) in &idx[lo..] {
                if y as f64 > r[3] + reach {
                    break;
                }
                let i = i as usize;
                let m = t.transform(i);
                let c = m.apply(t.outlines[t.shape_of(i)][0]);
                if c[0] < r[0] - reach || c[0] > r[2] + reach {
                    continue;
                }
                f(m, t.shape_of(i), p.cur[i]);
            }
        }
    }
}

fn outlines<'a>(b: &'a BoardRef) -> &'a [Vec<P>] {
    match b {
        BoardRef::Lattice(l) => &l.lat.outlines,
        BoardRef::Patch(p) => &p.topo.outlines,
    }
}

/// Render pixel rows `y0..y1` of a view.
pub fn render_rows(b: &BoardRef, v: &View, cols: &Colors, y0: u32, y1: u32) -> Image {
    let lut = &cols.lut;
    let mut img = Image {
        width: v.width,
        height: y1 - y0,
        rgba: cols.stage.repeat(v.width as usize * (y1 - y0) as usize),
    };
    let s = v.s;
    let rect = [
        v.rect[0],
        v.rect[1] + y0 as f64 / s,
        v.rect[0] + v.width as f64 / s,
        v.rect[1] + y1 as f64 / s,
    ];
    let to_px = |p: P| [(p[0] - v.rect[0]) * s, (p[1] - v.rect[1]) * s - y0 as f64];
    let outline = cell_size(b) * s >= 7.0;
    let shapes = outlines(b);
    let mut poly: Vec<P> = Vec::with_capacity(16);
    for_cells_in(b, rect, &mut |m, sh, h| {
        poly.clear();
        poly.extend(shapes[sh].iter().map(|&p| to_px(m.apply(p))));
        fill_polygon(&mut img, &poly, lut[h as usize]);
        if outline {
            for k in 0..poly.len() {
                draw_line(&mut img, poly[k], poly[(k + 1) % poly.len()], cols.outline);
            }
        }
    });
    img
}

/// A whole view in memory. When cells are smaller than a pixel, each pixel shows the average
/// colour of the cells whose centres fall in it.
pub fn render_view(b: &BoardRef, v: &View, look: &Look) -> Image {
    let cols = Colors::new(max_degree(b), look);
    let strips: Vec<(u32, u32)> = (0..v.height)
        .step_by(64)
        .map(|y| (y, (y + 64).min(v.height)))
        .collect();
    let parts: Vec<Image> = strips
        .par_iter()
        .map(|&(a, z)| render_rows(b, v, &cols, a, z))
        .collect();
    let mut img = Image {
        width: v.width,
        height: v.height,
        rgba: Vec::with_capacity(v.width as usize * v.height as usize * 4),
    };
    for p in parts {
        img.rgba.extend_from_slice(&p.rgba);
    }
    if cell_size(b) * v.s < 1.5 {
        average_into(b, v, &cols.lut, &mut img);
    }
    img
}

fn average_into(b: &BoardRef, v: &View, lut: &[Rgba; 256], img: &mut Image) {
    let (w, h) = (v.width as usize, v.height as usize);
    let acc: Vec<AtomicU32> = (0..w * h * 4).map(|_| AtomicU32::new(0)).collect();
    let shapes = outlines(b);
    let cents: Vec<P> = shapes.iter().map(|o| centroid(o)).collect();
    let add = |m: Affine, sh: usize, hgt: u8| {
        let c = m.apply(cents[sh]);
        let x = ((c[0] - v.rect[0]) * v.s).floor();
        let y = ((c[1] - v.rect[1]) * v.s).floor();
        if x < 0.0 || y < 0.0 || x >= w as f64 || y >= h as f64 {
            return;
        }
        let o = (y as usize * w + x as usize) * 4;
        let col = lut[hgt as usize];
        for q in 0..3 {
            acc[o + q].fetch_add(col[q] as u32, Ordering::Relaxed);
        }
        acc[o + 3].fetch_add(1, Ordering::Relaxed);
    };
    // Bands of the rectangle in parallel.
    let bands = 64.min(h.max(1));
    (0..bands).into_par_iter().for_each(|k| {
        let y0 = v.rect[1] + (v.rect[3] - v.rect[1]) * k as f64 / bands as f64;
        let y1 = v.rect[1] + (v.rect[3] - v.rect[1]) * (k + 1) as f64 / bands as f64;
        for_cells_in(b, [v.rect[0], y0, v.rect[2], y1], &mut |m, sh, hgt| {
            // Count each cell once: in the band holding its centre.
            let c = m.apply(cents[sh]);
            if c[1] >= y0 && (c[1] < y1 || k == bands - 1) {
                add(m, sh, hgt)
            }
        });
    });
    for p in 0..w * h {
        let n = acc[p * 4 + 3].load(Ordering::Relaxed);
        for q in 0..3 {
            if let Some(v) = acc[p * 4 + q].load(Ordering::Relaxed).checked_div(n) {
                img.rgba[p * 4 + q] = v as u8;
            }
        }
    }
}

/// A preview of the pile fitted into `side` pixels (an empty board gives a small blank image).
pub fn preview(b: &BoardRef, side: u32, look: &Look) -> Image {
    match world_bbox(b, 2.0) {
        Some(r) => render_view(b, &View::fit(r, side), look),
        None => Image {
            width: 16,
            height: 16,
            rgba: look.palette.stage_rgba().repeat(256),
        },
    }
}

/// Write the pile's crop to a PNG in strips. Returns the image size.
pub fn write_png(
    b: &BoardRef,
    path: &Path,
    opts: &ExportOptions,
    text: &[(&str, String)],
) -> Result<(u32, u32), String> {
    let rect = world_bbox(b, opts.margin).ok_or("the pile is empty")?;
    let v = View::new(rect, opts.px_per_cell / cell_size(b));
    let (w, h) = (v.width, v.height);
    if w as u64 * h as u64 > 4_000_000_000 {
        return Err(format!(
            "{w} x {h} pixels is too large (limit 4 billion pixels)"
        ));
    }
    let file = std::fs::File::create(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut enc = png::Encoder::new(std::io::BufWriter::new(file), w, h);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    for (k, val) in text {
        enc.add_itxt_chunk(k.to_string(), val.clone())
            .map_err(|e| e.to_string())?;
    }
    let mut wr = enc.write_header().map_err(|e| e.to_string())?;
    let mut stream = wr.stream_writer().map_err(|e| e.to_string())?;
    let cols = Colors::new(max_degree(b), &Look::of(opts));
    let strip = ((16 << 20) / (w as usize * 4)).clamp(8, 4096) as u32;
    let batch = rayon::current_num_threads().max(1) as u32;
    let small = cell_size(b) * v.s < 1.5;
    let mut y = 0;
    while y < h {
        let starts: Vec<u32> = (0..batch)
            .map(|k| y + k * strip)
            .filter(|&s| s < h)
            .collect();
        let imgs: Vec<Image> = starts
            .par_iter()
            .map(|&s| {
                let e = (s + strip).min(h);
                let mut img = render_rows(b, &v, &cols, s, e);
                if small {
                    let sv = View {
                        rect: [
                            v.rect[0],
                            v.rect[1] + s as f64 / v.s,
                            v.rect[2],
                            v.rect[1] + e as f64 / v.s,
                        ],
                        s: v.s,
                        width: v.width,
                        height: e - s,
                    };
                    average_into(b, &sv, &cols.lut, &mut img);
                }
                img
            })
            .collect();
        for img in imgs {
            std::io::Write::write_all(&mut stream, &img.rgba).map_err(|e| e.to_string())?;
        }
        y = starts.last().map(|s| (s + strip).min(h)).unwrap_or(h);
    }
    stream.finish().map_err(|e| e.to_string())?;
    Ok((w, h))
}

/// The run's metadata.
pub fn metadata(pile: &Pile, opts: &RunOptions, image: Option<Value>) -> Value {
    let (topplings, sweeps, secs) = pile.totals();
    let b = pile.board.as_ref();
    let board = match &b {
        BoardRef::Lattice(l) => {
            let (w, h) = l.dims();
            json!({"kind": "lattice", "i0": l.i0, "j0": l.j0, "w": w, "h": h, "tile_types": l.lat.nt,
                   "degrees": l.lat.deg, "cells": l.cells(), "bytes": pile.board.bytes()})
        }
        BoardRef::Patch(p) => {
            json!({"kind": "patch", "level": p.level, "cells": p.cells(), "bytes": pile.board.bytes()})
        }
    };
    let (w, h) = pile.extent();
    let mut v = json!({
        "jetcarp_version": crate::VERSION,
        "tiling": pile.spec.tiling,
        "neighborhood": match pile.spec.nb { crate::Nb::Edge => "edge", crate::Nb::Touch => "touch" },
        "grains": pile.spec.grains,
        "centre": pile.spec.centre,
        "sources": pile.sources,
        "steps": pile.steps,
        "steps_done": pile.done,
        "finished": pile.finished(),
        "grains_now": pile.grains_after(pile.done),
        "topplings": topplings,
        "sweeps": sweeps,
        "wall_seconds": secs,
        "pile_width": w,
        "pile_height": h,
        "board": board,
        "records": pile.records,
        "image": image,
    });
    if let Value::Object(extra) = &opts.extra {
        for (k, x) in extra {
            v[k] = x.clone();
        }
    }
    v
}

/// Export the finished pile: `pile.png`, `preview.png`, `pile.json`.
pub fn finish(pile: &Pile, dir: &Path, opts: &RunOptions) -> Result<RunSummary, String> {
    let b = pile.board.as_ref();
    let mut png = None;
    let mut size = None;
    let text = vec![
        ("jetcarp:version", crate::VERSION.to_string()),
        ("jetcarp:pile", serde_json::to_string(&pile.spec).unwrap()),
    ];
    if opts.export.png && pile.board.grains() > 0 {
        let p = dir.join("pile.png");
        size = Some(write_png(&b, &p, &opts.export, &text)?);
        png = Some(p);
    }
    let mut prev = None;
    if opts.export.preview_side > 0 {
        let img = preview(&b, opts.export.preview_side, &Look::of(&opts.export));
        let p = dir.join("preview.png");
        crate::render::write_png(&p, &img, &text)?;
        prev = Some(p);
    }
    let image = json!({
        "file": png.as_ref().map(|_| "pile.png"),
        "width": size.map(|s| s.0),
        "height": size.map(|s| s.1),
        "px_per_cell": opts.export.px_per_cell,
        "margin_cells": opts.export.margin,
        "palette": opts.export.palette,
        "reverse": opts.export.reverse,
        "preview": prev.as_ref().map(|_| "preview.png"),
    });
    let meta = crate::json_int::safe(metadata(pile, opts, Some(image)));
    super::write_json(&dir.join("pile.json"), &meta)?;
    Ok(RunSummary {
        dir: dir.to_path_buf(),
        png,
        preview: prev,
        image_size: size,
        metadata: meta,
    })
}
