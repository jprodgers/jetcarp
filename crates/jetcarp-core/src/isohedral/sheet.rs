//! Contact sheet of the isohedral types: one labelled thumbnail per type.

use super::{IH_FAMILIES, IH_TYPES};
use crate::edges::{self, EdgeMode, EdgeShape, Node};
use crate::families::{self, Family};
use crate::render::{Image, RenderOptions, STAGE, render};
use crate::rng::Rng;
use crate::sheet::draw_number;
use crate::world::{CaSpec, Command, SeedSpec, World};
use crate::{SizeSpec, TilingSpec};
use rayon::prelude::*;

/// Edge shapes within each class's constraint (free, mirror, half-turn, straight), drawn from
/// `rng`, shrunk until the tile outline does not cross itself. Straight edges stay straight.
pub fn sample_edges(fam: &Family, params: &serde_json::Value, rng: &mut Rng) -> Vec<EdgeShape> {
    let corners = families::corners(fam.id, params).unwrap_or_default();
    let base: Vec<EdgeShape> = fam
        .classes
        .iter()
        .map(|c| {
            let mode = edges::allowed_modes(c.kind)[0];
            let (t, d) = (0.2 + rng.next_f64() * 0.15, (rng.next_f64() - 0.5) * 0.6);
            let nodes = match mode {
                EdgeMode::J => vec![
                    Node { t, d },
                    Node {
                        t: 0.55 + rng.next_f64() * 0.25,
                        d: (rng.next_f64() - 0.5) * 0.6,
                    },
                ],
                EdgeMode::U => vec![Node { t, d }, Node { t: 1.0 - t, d }],
                EdgeMode::S => vec![Node { t, d }, Node { t: 1.0 - t, d: -d }],
                EdgeMode::I => vec![],
            };
            EdgeShape {
                mode,
                curved: true,
                nodes,
            }
        })
        .collect();
    let mut amp = 1.0;
    for _ in 0..8 {
        let shapes: Vec<EdgeShape> = base
            .iter()
            .map(|s| EdgeShape {
                nodes: s
                    .nodes
                    .iter()
                    .map(|q| Node {
                        t: q.t,
                        d: q.d * amp,
                    })
                    .collect(),
                ..s.clone()
            })
            .collect();
        if corners.is_empty()
            || !edges::self_intersects(&edges::tile_outline(fam.classes, &shapes, &corners, 10))
        {
            return shapes;
        }
        amp *= 0.6;
    }
    families::fresh_edges(fam)
}

#[derive(Clone, Debug)]
pub struct IhSheetOptions {
    pub cols: u32,
    pub thumb_px: u32,
    /// Cells per thumbnail (about).
    pub cells: u32,
    /// Reshape edges at random within their constraints.
    pub curved: bool,
    pub rng_state: u64,
    pub palette: String,
}

impl Default for IhSheetOptions {
    fn default() -> Self {
        IhSheetOptions {
            cols: 9,
            thumb_px: 200,
            cells: 120,
            curved: true,
            rng_state: 7,
            palette: "Prism".into(),
        }
    }
}

/// All 81 types, labelled with their IH number, each seeded with random Life cells.
pub fn contact_sheet(o: &IhSheetOptions) -> Result<Image, String> {
    let thumbs: Result<Vec<Image>, String> = (0..IH_FAMILIES.len())
        .into_par_iter()
        .map(|k| {
            let fam = &IH_FAMILIES[k];
            let params = families::init_params(fam.id);
            let mut rng = Rng::new(o.rng_state ^ (k as u64 + 1).wrapping_mul(0x9E37_79B9));
            let edges = if o.curved {
                sample_edges(fam, &params, &mut rng)
            } else {
                families::fresh_edges(fam)
            };
            let spec = TilingSpec {
                params: Some(params),
                edges: Some(edges),
                size: Some(SizeSpec::Cells(o.cells)),
                ..TilingSpec::new(fam.id)
            };
            let mut w = World::new(spec, o.rng_state)?;
            let mut ca = CaSpec::new("B3/S23").seed(SeedSpec::Random { density: 0.5 });
            ca.palette = Some(o.palette.clone());
            w.apply(Command::AddCa { ca })?;
            Ok(render(
                &w,
                &RenderOptions {
                    max_side: o.thumb_px,
                    outline_min_px: 4.0,
                    ..Default::default()
                },
            ))
        })
        .collect();
    let thumbs = thumbs?;
    let (tw, th) = (
        thumbs.iter().map(|t| t.width).max().unwrap_or(1),
        thumbs.iter().map(|t| t.height).max().unwrap_or(1),
    );
    let (label, pad) = (18, 8);
    let cols = o.cols.clamp(1, thumbs.len() as u32);
    let rows = (thumbs.len() as u32).div_ceil(cols);
    let (cw, ch) = (tw + pad, th + label + pad);
    let mut img = Image {
        width: cols * cw + pad,
        height: rows * ch + pad,
        rgba: vec![],
    };
    img.rgba = STAGE
        .iter()
        .copied()
        .cycle()
        .take((img.width * img.height * 4) as usize)
        .collect();
    for (k, t) in thumbs.iter().enumerate() {
        let (cx, cy) = ((k as u32 % cols) * cw + pad, (k as u32 / cols) * ch + pad);
        draw_number(
            &mut img,
            cx,
            cy + 2,
            IH_TYPES[k].num as u64,
            2,
            [0xd0, 0xd4, 0xdc, 0xff],
        );
        for y in 0..t.height {
            let src = (y * t.width * 4) as usize;
            let dst = (((cy + label + y) * img.width + cx) * 4) as usize;
            img.rgba[dst..dst + (t.width * 4) as usize]
                .copy_from_slice(&t.rgba[src..src + (t.width * 4) as usize]);
        }
    }
    Ok(img)
}
