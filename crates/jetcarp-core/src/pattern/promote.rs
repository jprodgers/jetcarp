//! Scenes built from patterns (what promotion to Quick Start shows), and pictures of a pattern's
//! phases.
//!
//! A promoted pattern becomes a row of the catalog's `quickstart` table with id = the pattern id
//! (the Quick Start lists these under "My finds"; `jetcarp_store::quickstart::promote`). Its scene
//! ([`showcase_scene`]) is a wrapped grid with the pattern pasted in the
//! centre through `Command::PastePattern`: still lifes and oscillators get a grid about three
//! times their size; spaceships a grid at least 4 times their size and 48 x 32 unit cells, so they
//! fly across and wrap around. Grids stay under the Quick Start's preview size (6,000 cells)
//! where the pattern allows, so the preview shows the same scene. Aperiodic patterns go on the
//! patch they were found on, at their site.

use super::paste::source_from_record;
use super::verify::{Frame, envelope, lattice_info, rule_world};
use super::{PasteAt, PatternClass, PatternRecord};
use crate::render::{Image, RenderOptions, render};
use crate::sheet::{Label, labelled_sheet};
use crate::topology::{Boundary, SizeSpec};
use crate::world::{CaSpec, Command, Scene, SeedSpec, World};

/// Largest preview the Quick Start uses without shrinking the grid.
const PREVIEW_CELLS: u64 = 6000;

/// A short description of a record for scene descriptions and listings.
pub fn describe(rec: &PatternRecord) -> String {
    let what = match rec.class {
        PatternClass::StillLife => "A still life".to_string(),
        PatternClass::Oscillator => format!("A period-{} oscillator", rec.period),
        PatternClass::Spaceship => format!(
            "A {} spaceship (period {}, moving {:?} lattice steps per period)",
            rec.speed.as_deref().unwrap_or("?"),
            rec.period,
            rec.displacement.unwrap_or([0, 0])
        ),
        c => format!("A {}", c.name()),
    };
    let date = rec.provenance.date.get(..10).unwrap_or("");
    format!(
        "{what} of {} cells in {} on {}, found by {} on {date}, seen {} time{}; verified for {} periods.{}",
        rec.cells,
        rec.rule,
        rec.tiling_name,
        match rec.discovered_by {
            super::DiscoveredBy::Search => "soup search",
            super::DiscoveredBy::Script => "a script",
            super::DiscoveredBy::Llm => "an LLM",
            super::DiscoveredBy::User => "hand",
        },
        rec.seen,
        if rec.seen == 1 { "" } else { "s" },
        rec.verified.periods,
        if rec.notes.is_empty() {
            String::new()
        } else {
            format!(" {}", first_paragraph(&rec.notes))
        }
    )
}

fn first_paragraph(s: &str) -> String {
    s.split("\n\n").next().unwrap_or("").trim().to_string()
}

/// A scene showing the pattern alone, centred on a wrapped grid sized for it.
pub fn showcase_scene(
    rec: &PatternRecord,
    title: Option<&str>,
    description: Option<&str>,
) -> Result<Scene, String> {
    let mut ca = CaSpec::new(&rec.rule)
        .nb(rec.neighborhood)
        .steps(rec.steps_per_gen)
        .seed(SeedSpec::Empty);
    ca.palette = Some(
        match rec.class {
            PatternClass::Spaceship => "Prism",
            PatternClass::Oscillator => "Ember",
            _ => "Tide",
        }
        .into(),
    );
    let (tiling, at) = match &rec.site {
        Some(site) => (site.patch.clone(), PasteAt::Original),
        None => {
            let info = lattice_info(&rec.tiling, rec.neighborhood)?;
            let env = envelope(rec, rec.period)?;
            let span_i = (env.iter().map(|c| c.i).max().unwrap()
                - env.iter().map(|c| c.i).min().unwrap()
                + 1) as f64;
            let span_j = (env.iter().map(|c| c.j).max().unwrap()
                - env.iter().map(|c| c.j).min().unwrap()
                + 1) as f64;
            // Sheared lattices need extra width for the slant.
            let span_i = span_i + span_j * info.shear.abs();
            let (mut w, mut h) = if rec.class == PatternClass::Spaceship {
                ((4.0 * span_i).max(48.0), (4.0 * span_j).max(32.0))
            } else {
                ((3.0 * span_i).max(16.0), (3.0 * span_j).max(12.0))
            };
            // Shrink towards the preview budget, never below twice the pattern plus a margin.
            let cells = |w: f64, h: f64| (w * h) as u64 * info.ntypes as u64;
            while cells(w, h) > PREVIEW_CELLS && w > 2.0 * span_i + 8.0 && h > 2.0 * span_j + 8.0 {
                w *= 0.9;
                h *= 0.9;
            }
            let mut t = super::tiling_identity(&rec.tiling);
            t.size = Some(SizeSpec::Rect {
                w: (w.ceil() as u32).max(8),
                h: (h.ceil() as u32).max(8),
            });
            t.boundary = Boundary::Wrap;
            (t, PasteAt::Center)
        }
    };
    let mut sc = Scene::new(tiling);
    sc.name = title.map(String::from).unwrap_or_else(|| rec.label());
    sc.description = description
        .map(String::from)
        .unwrap_or_else(|| describe(rec));
    sc.rng_state = 1;
    sc.cas = vec![ca];
    sc.commands = vec![Command::PastePattern {
        id: 1,
        pattern: source_from_record(rec, 0),
        at,
        orientation: 0,
        phase: 0,
    }];
    // Check that it builds and the paste lands.
    World::from_scene(&sc)?;
    Ok(sc)
}

/// A picture of every phase (at most `max_phases`), side by side and labelled, each on the same
/// frame so a spaceship's motion shows. `px` is the size of each panel.
pub fn render_phases(rec: &PatternRecord, px: u32, max_phases: u32) -> Result<Image, String> {
    render_phases_in(rec, px, max_phases, None)
}

/// [`render_phases`] in a palette (`None`: the rule's default colours).
pub fn render_phases_in(
    rec: &PatternRecord,
    px: u32,
    max_phases: u32,
    palette: Option<&str>,
) -> Result<Image, String> {
    let n = (rec.period as u32).min(max_phases.max(1));
    let mut panels: Vec<(Label, Image)> = Vec::new();
    let white = [0xd0, 0xd4, 0xdc, 0xff];
    match &rec.site {
        Some(site) => {
            let topo = super::site::patch_topology(&site.patch)?;
            let map = super::site::match_site(&topo, site, site.anchor as usize)?;
            for t in 0..n {
                let mut w = rule_world(rec, topo.clone())?;
                let id = w.cas[0].id;
                let cells = rec
                    .phase(t as u64)?
                    .iter()
                    .map(|c| [map[c.i as usize], c.s as u32])
                    .collect();
                w.apply(Command::SetCells { id, cells })?;
                if let Some(p) = palette {
                    crate::render::recolor(&mut w, p);
                }
                let img = render(
                    &w,
                    &RenderOptions {
                        max_side: px,
                        outline_min_px: 0.0,
                        ..Default::default()
                    },
                );
                panels.push((vec![(format!("phase {t}"), white)], img));
            }
        }
        None => {
            let info = lattice_info(&rec.tiling, rec.neighborhood)?;
            let env = envelope(rec, n as u64)?;
            let frame = Frame::around(&rec.tiling, &info, &env, 1)?;
            for t in 0..n {
                let mut w = rule_world(rec, frame.topo.clone())?;
                let id = w.cas[0].id;
                w.apply(Command::SetCells {
                    id,
                    cells: frame.cells(&rec.phase(t as u64)?)?,
                })?;
                if let Some(p) = palette {
                    crate::render::recolor(&mut w, p);
                }
                let img = render(
                    &w,
                    &RenderOptions {
                        max_side: px,
                        outline_min_px: 3.0,
                        ..Default::default()
                    },
                );
                panels.push((vec![(format!("phase {t}"), white)], img));
            }
        }
    }
    let items: Vec<(Label, &Image)> = panels.iter().map(|(l, i)| (l.clone(), i)).collect();
    Ok(labelled_sheet(&items, n.min(8)))
}
