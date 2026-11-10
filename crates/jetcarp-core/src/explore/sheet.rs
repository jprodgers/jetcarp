//! Contact sheets of search results.

use super::cand::Cand;
use super::job::feat_from_blob;
use super::space::{Fam, sized_spec, ALL_FAMS};
use crate::render::{Image, RenderOptions, render_fitted};
use crate::sheet::{Label, labelled_sheet_scaled};
use crate::topology::Nb;
use crate::world::World;
use rayon::prelude::*;
use rusqlite::Connection;

/// A stored run as a row.
#[derive(Clone, Debug)]
pub struct RunRow {
    pub id: i64,
    pub cand: Cand,
    pub fate: String,
    pub score: f64,
    pub bin: i64,
    pub feats: Vec<u8>,
}

/// Read runs. `sql` is a query that returns `id` first (typically `SELECT id FROM runs WHERE ...`).
pub fn query_runs(conn: &Connection, sql: &str) -> Result<Vec<RunRow>, String> {
    let e = |x: rusqlite::Error| x.to_string();
    let mut st = conn.prepare(sql).map_err(e)?;
    let ids: Vec<i64> = st
        .query_map([], |r| r.get(0))
        .map_err(e)?
        .collect::<Result<_, _>>()
        .map_err(e)?;
    let mut out = Vec::with_capacity(ids.len());
    let mut st = conn
        .prepare(
            "SELECT tiling,nb,fam,rule,seed,cells,gens,steps,rng,origin,fate,score,bin,feats
             FROM runs WHERE id=?",
        )
        .map_err(e)?;
    for id in ids {
        let row = st.query_row([id], |r| {
            let fam: String = r.get(2)?;
            let seed: String = r.get(4)?;
            Ok((
                Cand {
                    tiling: r.get(0)?,
                    nb: if r.get::<_, i64>(1)? == 0 {
                        Nb::Touch
                    } else {
                        Nb::Edge
                    },
                    fam: ALL_FAMS
                        .iter()
                        .copied()
                        .find(|f| f.name() == fam)
                        .unwrap_or(Fam::LifeLike),
                    rule: r.get(3)?,
                    seed: serde_json::from_str(&seed).unwrap_or(crate::world::SeedSpec::Empty),
                    cells: r.get::<_, i64>(5)? as u32,
                    gens: r.get::<_, i64>(6)? as u32,
                    steps: r.get::<_, i64>(7)? as u32,
                    rng: r.get::<_, i64>(8)? as u64,
                    origin: r.get(9)?,
                },
                r.get::<_, String>(10)?,
                r.get::<_, f64>(11)?,
                r.get::<_, i64>(12)?,
                r.get::<_, Vec<u8>>(13)?,
            ))
        });
        if let Ok((cand, fate, score, bin, feats)) = row {
            out.push(RunRow {
                id,
                cand,
                fate,
                score,
                bin,
                feats,
            });
        }
    }
    Ok(out)
}

/// Replay a run to `gens` generations and return the world.
pub fn replay(cand: &Cand, gens: u32) -> Result<World, String> {
    let (spec, topo) = sized_spec(&cand.tiling, cand.cells)?;
    let _ = spec;
    let mut w = World::on_topology(topo, cand.rng);
    w.add_ca(cand.ca())?;
    for _ in 0..gens {
        w.step();
    }
    Ok(w)
}

/// A picture of a run at a fraction of its generations.
pub fn thumb(cand: &Cand, frac: f64, px: u32, palette: Option<&str>) -> Option<Image> {
    let (spec, topo) = sized_spec(&cand.tiling, cand.cells).ok()?;
    let _ = spec;
    let mut w = World::on_topology(topo, cand.rng);
    let mut ca = cand.ca();
    if let Some(p) = palette {
        ca.palette = Some(p.to_string());
    }
    w.add_ca(ca).ok()?;
    let gens = (cand.gens as f64 * frac) as u32;
    for _ in 0..gens {
        w.step();
    }
    let opts = RenderOptions {
        outline_min_px: 0.0,
        ..Default::default()
    };
    Some(render_fitted(&w, px, px, &opts))
}

/// One image per run per fraction (of the run), labelled and laid out in columns. A run's
/// images sit next to each other.
pub fn sheet(
    runs: &[RunRow],
    fracs: &[f64],
    px: u32,
    per_row: u32,
    palette: Option<&str>,
) -> Image {
    let imgs: Vec<(usize, Vec<Option<Image>>)> = runs
        .par_iter()
        .enumerate()
        .map(|(i, r)| {
            let v = fracs
                .iter()
                .map(|f| thumb(&r.cand, *f, px, palette))
                .collect();
            (i, v)
        })
        .collect();
    let blank = Image {
        width: px,
        height: px,
        rgba: vec![30; (px * px * 4) as usize],
    };
    let white = [240, 240, 240, 255];
    let dim = [160, 170, 190, 255];
    let mut items: Vec<(Label, &Image)> = Vec::new();
    for (i, v) in &imgs {
        let r = &runs[*i];
        let f = feat_from_blob(&r.feats);
        let line1 = format!("{} {} {:.2}", r.id, r.cand.tiling, r.score);
        let line2 = format!("{} {}", r.fate, shorten(&r.cand.rule, 34));
        let line3 = format!(
            "s{} d{:.2} a{:.3} m{} sm{:.2}",
            f.settle, f.density_late, f.activity_late, f.movers, f.smooth
        );
        for (k, img) in v.iter().enumerate() {
            let label: Label = if k == 0 {
                vec![(line1.clone(), white), (line2.clone(), dim), (line3.clone(), dim)]
            } else {
                vec![
                    (format!("t={}", (r.cand.gens as f64 * fracs[k]) as u32), dim),
                    (String::new(), dim),
                    (String::new(), dim),
                ]
            };
            items.push((label, img.as_ref().unwrap_or(&blank)));
        }
    }
    labelled_sheet_scaled(&items, per_row, 1)
}

/// One row per run, one column per palette, each at the same point in the run.
pub fn palette_sheet(runs: &[RunRow], palettes: &[&str], frac: f64, px: u32) -> Image {
    let imgs: Vec<Option<Image>> = runs
        .par_iter()
        .map(|r| {
            palettes
                .iter()
                .map(|p| thumb(&r.cand, frac, px, Some(p)))
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>()
        .into_iter()
        .flatten()
        .collect();
    let blank = Image {
        width: px,
        height: px,
        rgba: vec![30; (px * px * 4) as usize],
    };
    let white = [240, 240, 240, 255];
    let dim = [160, 170, 190, 255];
    let mut items: Vec<(Label, &Image)> = Vec::new();
    for (i, img) in imgs.iter().enumerate() {
        let r = &runs[i / palettes.len()];
        let label: Label = vec![
            (palettes[i % palettes.len()].to_string(), white),
            (format!("{} {}", r.id, r.cand.fam.name()), dim),
            (r.fate.clone(), dim),
        ];
        items.push((label, img.as_ref().unwrap_or(&blank)));
    }
    labelled_sheet_scaled(&items, palettes.len() as u32, 1)
}

fn shorten(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        s.to_string()
    } else {
        s.chars().take(n).collect()
    }
}
