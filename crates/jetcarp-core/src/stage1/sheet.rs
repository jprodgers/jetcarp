//! Contact sheets of stage 1 settings, for the user and a local model's shortlist.
//!
//! Each thumbnail is soup 0 of a setting at a few generations side by side (by default a tenth of
//! the way and the end), rendered from the setting's own scene (the world makes the soup; the bit-parallel board runs it), under a label
//! (`A`, `B`, ... or the setting id), the score, the rule, and the behaviour. The JSON sidecar
//! lists every thumbnail with its label, position, setting id, Quick Start entry id, rule,
//! tiling, score, status, behaviour, tags, the setting itself, and the main descriptors, so a
//! script can map a model's picks back to settings and write them as ratings.

use super::board::Board;
use crate::library::SettingRow;
use crate::render::{Image, RenderOptions, render};
use crate::sheet::{Label, labelled_sheet_scaled};
use crate::world::World;
use rayon::prelude::*;
use serde_json::{Value, json};

/// What to draw.
#[derive(Clone, Debug)]
pub struct SheetOptions {
    /// Thumbnail size (longest side, pixels).
    pub thumb: u32,
    pub cols: u32,
    /// Labels per setting (default: `#<id>`).
    pub labels: Option<Vec<String>>,
    /// Generations shown side by side, as fractions of the setting's generations (1 = the
    /// end).
    pub at: Vec<f64>,
    /// Label text scale (at most).
    pub label_scale: u32,
    /// Draw every setting in this palette (a built-in name) instead of its own.
    pub palette: Option<String>,
}

impl Default for SheetOptions {
    fn default() -> Self {
        SheetOptions {
            thumb: 240,
            cols: 6,
            labels: None,
            at: vec![0.1, 1.0],
            label_scale: 2,
            palette: None,
        }
    }
}

/// A world holding soup 0 of a setting at generation `gen`.
pub fn world_at(row: &SettingRow, generation: u64) -> Result<World, String> {
    let s = &row.setting;
    let mut w = World::from_scene(&s.scene(0)?)?;
    let (rule, nb) = s.rule()?;
    let mut b = Board::for_topology(&rule, &w.topo, nb)?;
    let f = w.cas[0].field;
    b.load(&w.fields[f].state);
    b.run(generation);
    w.fields[f].state = b.states();
    Ok(w)
}

/// One thumbnail: the frames at these fractions of the run, side by side, each `px` on its
/// longest side.
pub fn thumbnail(row: &SettingRow, at: &[f64], px: u32) -> Result<Image, String> {
    thumbnail_in(row, at, px, None)
}

/// [`thumbnail`] in a palette (`None`: the setting's own).
pub fn thumbnail_in(
    row: &SettingRow,
    at: &[f64],
    px: u32,
    palette: Option<&str>,
) -> Result<Image, String> {
    let mut frames = Vec::new();
    for &a in at {
        let g = ((row.setting.gens as f64 * a).round() as u64).min(row.setting.gens);
        let mut w = world_at(row, g)?;
        if let Some(p) = palette {
            crate::render::recolor(&mut w, p);
        }
        frames.push(render(
            &w,
            &RenderOptions {
                max_side: px,
                outline_min_px: 0.0,
                ..Default::default()
            },
        ));
    }
    let gap = 4;
    let h = frames.iter().map(|f| f.height).max().unwrap_or(1);
    let wsum: u32 =
        frames.iter().map(|f| f.width).sum::<u32>() + gap * (frames.len() as u32).saturating_sub(1);
    let mut img = Image {
        width: wsum.max(1),
        height: h,
        rgba: [16u8, 16, 22, 255]
            .iter()
            .copied()
            .cycle()
            .take((wsum.max(1) * h * 4) as usize)
            .collect(),
    };
    let mut x0 = 0;
    for f in &frames {
        for y in 0..f.height {
            let src = (y * f.width * 4) as usize;
            let dst = ((y * img.width + x0) * 4) as usize;
            img.rgba[dst..dst + (f.width * 4) as usize]
                .copy_from_slice(&f.rgba[src..src + (f.width * 4) as usize]);
        }
        x0 += f.width + gap;
    }
    Ok(img)
}

/// The sheet and its JSON sidecar.
pub fn contact_sheet(rows: &[SettingRow], o: &SheetOptions) -> Result<(Image, Value), String> {
    if rows.is_empty() {
        return Err("no settings to draw".into());
    }
    if let Some(p) = &o.palette {
        crate::render::builtin_palette(p)?;
    }
    let thumbs: Vec<Result<Image, String>> = rows
        .par_iter()
        .map(|r| thumbnail_in(r, &o.at, o.thumb, o.palette.as_deref()))
        .collect();
    let labels: Vec<String> = match &o.labels {
        Some(l) if l.len() >= rows.len() => l[..rows.len()].to_vec(),
        Some(_) => return Err("fewer labels than settings".into()),
        None => rows.iter().map(|r| format!("#{}", r.id)).collect(),
    };
    let white = [235, 235, 235, 255];
    let grey = [160, 160, 170, 255];
    let mut items: Vec<(Label, Image)> = Vec::new();
    let mut entries = Vec::new();
    let cols = o.cols.max(1);
    for (k, (r, t)) in rows.iter().zip(thumbs).enumerate() {
        let img = t?;
        let score = r.score.unwrap_or(0.0);
        items.push((
            vec![
                (format!("{} {:.0} {}", labels[k], score, r.notation), white),
                (
                    format!(
                        "{} {}",
                        r.tiling_name.to_lowercase(),
                        r.descriptors.behaviour
                    ),
                    grey,
                ),
            ],
            img,
        ));
        let d = &r.descriptors;
        entries.push(json!({
            "label": labels[k],
            "row": k as u32 / cols,
            "col": k as u32 % cols,
            "setting_id": r.id,
            "entry_id": r.entry_id(),
            "rule": r.notation,
            "tiling": r.tiling_name,
            "score": r.score,
            "status": r.status.name(),
            "behaviour": d.behaviour,
            "tags": d.tags,
            "setting": r.setting,
            "facts": {
                "died": d.died, "settled": d.settled, "active": d.active,
                "median_settle": d.median_settle,
                "activity_end": d.activity_end, "final_density": d.final_density,
                "compress": d.compress, "entropy": d.entropy,
                "ship_kinds": d.ship_kinds, "oscillator_kinds": d.oscillator_kinds,
                "longest_period": d.longest_period, "distinct": d.distinct,
                "novelty": d.novelty,
            },
        }));
    }
    let refs: Vec<(Label, &Image)> = items.iter().map(|(l, i)| (l.clone(), i)).collect();
    let img = labelled_sheet_scaled(&refs, cols, o.label_scale);
    let side = json!({
        "jetcarp_version": crate::VERSION,
        "kind": "stage1-sheet",
        "cols": cols,
        "thumb": o.thumb,
        "at": o.at,
        "width": img.width,
        "height": img.height,
        "entries": entries,
    });
    Ok((img, side))
}

/// Write the sheet as a PNG and its sidecar next to it (`<name>.json`).
pub fn write_sheet(
    rows: &[SettingRow],
    o: &SheetOptions,
    png: &std::path::Path,
) -> Result<std::path::PathBuf, String> {
    let (img, side) = contact_sheet(rows, o)?;
    if let Some(d) = png.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(d).map_err(|e| format!("{}: {e}", d.display()))?;
    }
    crate::render::write_png(png, &img, &[("jetcarp:kind", "stage1-sheet".into())])
        .map_err(|e| format!("{}: {e}", png.display()))?;
    let js = png.with_extension("json");
    std::fs::write(&js, serde_json::to_string_pretty(&side).unwrap())
        .map_err(|e| format!("{}: {e}", js.display()))?;
    Ok(js)
}
