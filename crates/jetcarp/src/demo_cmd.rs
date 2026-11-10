//! `jetcarp demo`: build the computing demos (WireWorld adder, Life logic gates) for any inputs.

use anyhow::{Context, Result, bail};
use jetcarp_core::computing;
use jetcarp_core::render::{RenderOptions, png_metadata};
use jetcarp_core::*;
use std::path::PathBuf;

#[derive(clap::Args)]
pub struct DemoArgs {
    /// wireworld-adder, life-not, life-and, life-or.
    kind: String,
    /// First input (adder: 0-3; gates: 0 or 1).
    #[arg(long, default_value_t = 1)]
    a: u32,
    /// Second input (adder: 0-3; gates: 0 or 1; NOT ignores it).
    #[arg(long, default_value_t = 1)]
    b: u32,
    /// Life gates: every input combination side by side (the Quick Start scenes).
    #[arg(long)]
    table: bool,
    /// Save the scene here (open it in the app or with `jetcarp run`).
    #[arg(long)]
    save: Option<PathBuf>,
    /// Run this many generations and write a PNG.
    #[arg(long)]
    png: Option<PathBuf>,
    #[arg(long)]
    gens: Option<u64>,
    /// Pixels per world unit for the PNG.
    #[arg(long, default_value_t = 4.0)]
    scale: f64,
}

pub fn run(a: DemoArgs) -> Result<()> {
    // The scene, generations to run, and output probes (cells read as "1" when a head (WireWorld)
    // or any live cell (Life) appears there during the last `window` generations).
    let (scene, gens, probes, width, window, head_only): (
        Scene,
        u64,
        Vec<[i32; 4]>,
        u32,
        u64,
        bool,
    ) = match a.kind.as_str() {
        "wireworld-adder" | "adder" => {
            let ad = computing::wireworld_adder(a.a, a.b).map_err(anyhow::Error::msg)?;
            let p = computing::WW_PERIOD as u64;
            let probes = ad
                .outputs
                .iter()
                .map(|&(x, y)| [x, y, x + 1, y + 1])
                .collect();
            (ad.scene, ad.first_output + p, probes, ad.width, p, true)
        }
        k if k.starts_with("life-") => {
            let gate = computing::Gate::parse(&k[5..]).map_err(anyhow::Error::msg)?;
            if a.table {
                let (sc, dets) = computing::life_truth_table(gate);
                let w = match sc.tiling.size {
                    Some(SizeSpec::Rect { w, .. }) => w,
                    _ => 0,
                };
                let probes = dets.iter().map(|d| d.1).collect();
                (sc, computing::LIFE_SETTLE + 60, probes, w, 60, false)
            } else {
                let lg =
                    computing::life_gate(gate, a.a != 0, a.b != 0).map_err(anyhow::Error::msg)?;
                (
                    lg.scene,
                    lg.settle + 60,
                    vec![lg.detector],
                    lg.width,
                    60,
                    false,
                )
            }
        }
        _ => bail!("demos: wireworld-adder, life-not, life-and, life-or"),
    };
    println!("{}", scene.name);
    println!("{}", scene.description);
    if let Some(p) = &a.save {
        std::fs::write(p, scene.to_json()).with_context(|| format!("writing {}", p.display()))?;
        println!("saved: {}", p.display());
    }
    // Run and read the outputs.
    let gens = a.gens.unwrap_or(gens).max(window);
    let mut w = World::from_scene(&scene).map_err(anyhow::Error::msg)?;
    w.apply(Command::Step {
        gens: gens - window,
    })
    .map_err(anyhow::Error::msg)?;
    let mut seen = vec![false; probes.len()];
    for _ in 0..window {
        w.step();
        for (k, b) in probes.iter().enumerate() {
            for y in b[1]..b[3] {
                for x in b[0]..b[2] {
                    let s = w.fields[0].state[(y as u32 * width + x as u32) as usize];
                    if (head_only && s == 1) || (!head_only && s != 0) {
                        seen[k] = true;
                    }
                }
            }
        }
    }
    let bits: Vec<String> = seen.iter().map(|&s| (s as u8).to_string()).collect();
    let names = match probes.len() {
        3 => "s0 s1 s2",
        1 => "out",
        _ => "per frame, left to right",
    };
    println!(
        "outputs ({names}) over generations {}..{}: {}",
        gens - window + 1,
        gens,
        bits.join(" ")
    );
    if let Some(p) = &a.png {
        let opts = RenderOptions {
            scale: Some(a.scale),
            max_side: 8000,
            ..Default::default()
        };
        let (pw, ph) = render::write_png_streaming(p, &w, &opts, &png_metadata(&w))
            .map_err(anyhow::Error::msg)?;
        println!(
            "png: {} ({pw}x{ph}) at generation {}",
            p.display(),
            w.generation
        );
    }
    Ok(())
}
