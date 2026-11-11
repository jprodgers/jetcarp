//! `jetcarp objects`: the still lifes, oscillators, and spaceships in a scene. (Censuses are
//! jobs: `jetcarp search census`.)

use anyhow::{Context, Result};
use jetcarp_core::objects::{ObjectOptions, find_objects};
use jetcarp_core::render::{RenderOptions, png_metadata, render_with_colors, write_png};
use jetcarp_core::*;
use std::path::PathBuf;

#[derive(clap::Args)]
pub struct ObjectsArgs {
    scene: PathBuf,
    /// Generations to run after replaying the scene's commands.
    #[arg(long, default_value_t = 0)]
    gens: u64,
    /// Print the full report as JSON.
    #[arg(long)]
    json: bool,
    /// List every object, not just the counts.
    #[arg(long)]
    list: bool,
    /// PNG with each object's cells coloured by class.
    #[arg(long)]
    png: Option<PathBuf>,
    #[arg(long)]
    scale: Option<f64>,
    #[arg(long, default_value_t = 1600)]
    max_side: u32,
    /// CA id to analyse (default: the first enabled B/S, Generations, WireWorld, or table CA).
    #[arg(long)]
    ca: Option<u32>,
    /// Longest period looked for.
    #[arg(long, default_value_t = 64)]
    max_period: u64,
    /// Cells within this many neighbour steps start in one group.
    #[arg(long, default_value_t = 2)]
    merge: u32,
    /// Do not split merged groups into non-interacting parts.
    #[arg(long)]
    no_split: bool,
    #[arg(long)]
    threads: Option<usize>,
}

fn set_threads(t: Option<usize>) -> Result<()> {
    if let Some(t) = t {
        rayon::ThreadPoolBuilder::new()
            .num_threads(t)
            .build_global()
            .context("thread pool")?;
    }
    Ok(())
}

pub fn objects(a: ObjectsArgs) -> Result<()> {
    set_threads(a.threads)?;
    let text = std::fs::read_to_string(&a.scene)
        .with_context(|| format!("reading {}", a.scene.display()))?;
    let sc = Scene::from_json(&text).map_err(anyhow::Error::msg)?;
    let mut w = World::from_scene(&sc).map_err(anyhow::Error::msg)?;
    if a.gens > 0 {
        w.apply(Command::Step { gens: a.gens })
            .map_err(anyhow::Error::msg)?;
    }
    let t0 = std::time::Instant::now();
    let opts = ObjectOptions {
        ca: a.ca,
        max_period: a.max_period,
        merge_distance: a.merge,
        separate: !a.no_split,
        ..Default::default()
    };
    let r = find_objects(&w, &opts).map_err(anyhow::Error::msg)?;
    let secs = t0.elapsed().as_secs_f64();
    if let Some(p) = &a.png {
        let img = render_with_colors(
            &w,
            &r.colors(w.topo.len()),
            &RenderOptions {
                scale: a.scale,
                max_side: a.max_side,
                ..Default::default()
            },
        );
        let mut meta = png_metadata(&w);
        meta.push(("jetcarp:objects", r.summary_line()));
        write_png(p, &img, &meta).map_err(anyhow::Error::msg)?;
    }
    if a.json {
        println!("{}", r.to_json());
        return Ok(());
    }
    let t = r.class_totals();
    println!("scene:      {}", a.scene.display());
    println!("tiling:     {}", r.tiling);
    println!("generation: {}  hash {}", r.generation, r.hash);
    println!(
        "rule:       {} (CA {}, field {}), {} live cells, dead boundary {}",
        r.rule, r.ca_id, r.field, r.live_cells, r.dead_boundary
    );
    println!(
        "objects:    {} (still lifes {}, oscillators {}, spaceships {}, other {}), {} symmetries, periods up to {}",
        r.objects.len(),
        t[0],
        t[1],
        t[2],
        t[3],
        r.symmetries,
        r.max_period
    );
    println!("census:     {}", r.summary_line());
    println!(
        "\n{:>6}  {:<11} {:>6} {:>6}  {:<8}  name",
        "count", "class", "period", "cells", "key"
    );
    for c in &r.counts {
        let first = &r.objects[c.objects[0]];
        println!(
            "{:>6}  {:<11} {:>6} {:>6}  {:<8}  {}{}",
            c.count,
            c.class.kind(),
            c.class
                .period()
                .map(|p| p.to_string())
                .unwrap_or_else(|| "-".into()),
            first.size,
            if c.key.starts_with("other") {
                ""
            } else {
                &c.key
            },
            c.label(),
            c.class
                .speed_in(r.lattice)
                .map(|s| format!(", {s}"))
                .unwrap_or_default()
        );
    }
    if a.list {
        println!();
        for o in &r.objects {
            println!(
                "#{:<5} {:<28} cells {:>4}  centroid ({:.1}, {:.1})  {}",
                o.id, o.name, o.size, o.centroid[0], o.centroid[1], o.canonical
            );
        }
    }
    if let Some(p) = &a.png {
        println!("png:        {}", p.display());
    }
    println!(
        "time:       {secs:.2}s on {} threads",
        rayon::current_num_threads()
    );
    Ok(())
}
