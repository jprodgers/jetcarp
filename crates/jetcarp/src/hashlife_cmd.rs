//! `jetcarp hashlife`: jump a pattern or a wrap-edge world far ahead with HashLife.

use anyhow::{Context, Result, bail};
use jetcarp_core::hashlife::{self, Options, Plane};
use jetcarp_core::render::{RenderOptions, png_metadata, write_png};
use jetcarp_core::*;
use std::path::PathBuf;

#[derive(clap::Args)]
pub struct HashlifeArgs {
    /// A scene (.json), a pattern-library record (.json), or a pattern in RLE or plaintext.
    input: PathBuf,
    /// Generations to jump: 1000, 2^40, 1e6.
    #[arg(long, default_value = "0")]
    gens: String,
    /// Jump the scene's wrap-edge world in place (its torus sides must be powers of two),
    /// exactly as stepping it would on states. HashLife does not track ages: every nonzero cell
    /// ends with age 1, so `--compare` checks states, not the full hash. Default: the unbounded
    /// plane (exported cells also start at age 1).
    #[arg(long)]
    torus: bool,
    /// Plane mode: the exported region `i0,j0,w,h` in unit cells (lattice coordinates), or
    /// `world` for the source scene's own frame. Default: the pattern's bounding box plus
    /// --margin.
    #[arg(long)]
    region: Option<String>,
    #[arg(long, default_value_t = 8)]
    margin: u64,
    /// Edges of the exported world: dead, wrap, or live.
    #[arg(long, default_value = "dead")]
    boundary: String,
    /// Override the rule (RLE and plaintext default to the RLE header, else B3/S23).
    #[arg(long)]
    rule: Option<String>,
    /// Write a PNG: the JETCARP renderer up to --max-cells, a density map above that.
    #[arg(long)]
    png: Option<PathBuf>,
    /// The PNG's palette (`jetcarp palettes` lists them): every CA's colours for the JETCARP
    /// renderer, the stops and background of the density map. Default: the scene's own
    /// palettes, and Ember for the density map.
    #[arg(long)]
    palette: Option<String>,
    /// Largest side of the PNG in pixels.
    #[arg(long, default_value_t = 1600)]
    max_side: u32,
    /// Pixels per world unit for the JETCARP renderer (default: fit --max-side).
    #[arg(long)]
    scale: Option<f64>,
    /// Regions with more cells than this are drawn as a density map.
    #[arg(long, default_value_t = 16_000_000)]
    max_cells: u64,
    /// Save the result as a scene (plane mode: the region as a new world).
    #[arg(long)]
    save: Option<PathBuf>,
    /// Live nodes before a garbage collection (each node is about 40 bytes with its table slot).
    #[arg(long, default_value_t = hashlife::DEFAULT_NODE_CAP)]
    node_cap: usize,
    /// Largest node level stepped by the bit-parallel leaf (B/S and Generations rules without
    /// B0; at most 6, 0 = off). Results never depend on it.
    #[arg(long, default_value_t = hashlife::DEFAULT_LEAF_LEVEL)]
    leaf_level: u8,
    /// Also run the ordinary engine (`cpu` or `gpu`) and compare cell states (torus mode, or
    /// plane mode with `--region world`).
    #[arg(long)]
    compare: Option<String>,
    #[arg(long)]
    json: bool,
}

fn parse_region(s: &str) -> Result<(i64, i64, u64, u64)> {
    let v: Vec<&str> = s.split(',').map(|x| x.trim()).collect();
    if v.len() != 4 {
        bail!("--region wants i0,j0,w,h");
    }
    Ok((v[0].parse()?, v[1].parse()?, v[2].parse()?, v[3].parse()?))
}

fn boundary(s: &str) -> Result<Boundary> {
    Ok(match s {
        "dead" => Boundary::Dead,
        "wrap" => Boundary::Wrap,
        "live" => Boundary::Live,
        _ => bail!("--boundary is dead, wrap, or live"),
    })
}

/// Run the ordinary engine on `w` for `gens`, returning (seconds, backend).
fn reference(mut w: World, gens: u64, how: &str) -> Result<(World, f64, String)> {
    let t0 = std::time::Instant::now();
    let backend = match how {
        "gpu" => {
            let mut sim = jetcarp_gpu::GpuSim::headless().map_err(anyhow::Error::msg)?;
            let m = sim.step_mixed(&mut w, gens).map_err(anyhow::Error::msg)?;
            match m.fallback {
                None => format!("gpu ({})", sim.name()),
                Some(why) => format!("gpu with cpu fallback: {why}"),
            }
        }
        "cpu" => {
            w.apply(Command::Step { gens })
                .map_err(anyhow::Error::msg)?;
            format!("cpu ({} threads)", rayon::current_num_threads())
        }
        _ => bail!("--compare is cpu or gpu"),
    };
    Ok((w, t0.elapsed().as_secs_f64(), backend))
}

pub fn run(a: HashlifeArgs) -> Result<()> {
    let gens = hashlife::parse_gens(&a.gens).map_err(anyhow::Error::msg)?;
    let palette = a
        .palette
        .as_deref()
        .map(render::builtin_palette)
        .transpose()
        .map_err(anyhow::Error::msg)?;
    let opts = Options {
        node_cap: a.node_cap,
        leaf_level: a.leaf_level,
    };
    let text = std::fs::read_to_string(&a.input)
        .with_context(|| format!("reading {}", a.input.display()))?;
    let scene = text
        .trim_start()
        .starts_with('{')
        .then(|| Scene::from_json(&text).ok())
        .flatten()
        .filter(|s| !s.cas.is_empty());
    let mut out = serde_json::json!({
        "input": a.input.display().to_string(), "generations": gens, "jetcarp_version": VERSION,
    });
    let mut lines: Vec<String> = vec![format!("input:      {}", a.input.display())];
    let mut failed = None;

    if a.torus {
        let Some(sc) = &scene else {
            bail!("--torus needs a scene file")
        };
        let mut sc = sc.clone();
        if let Some(r) = &a.rule {
            for c in &mut sc.cas {
                c.rule = r.clone();
            }
        }
        let w0 = World::from_scene(&sc).map_err(anyhow::Error::msg)?;
        if let Some(p) = hashlife::torus_problem(&w0) {
            bail!("{p}");
        }
        let cmp_world = a
            .compare
            .as_ref()
            .map(|_| World::from_scene(&sc))
            .transpose();
        let mut w = w0;
        let start_gen = w.generation;
        // Through the command, so the jump is in the log and the saved scene replays.
        hashlife::with_options(opts, || w.apply(Command::Jump { gens }))
            .map_err(anyhow::Error::msg)?;
        let rep = hashlife::last_report().unwrap_or_default();
        lines.push(format!("mode:       torus ({})", w.topo.describe()));
        lines.push(format!(
            "generation: {} (+{gens} in {:.3}s)",
            w.generation, rep.seconds
        ));
        lines.push(format!(
            "population: {}",
            w.fields[w.cas[0].field].population()
        ));
        lines.push(format!("states:     {:016x}", w.states_hash()));
        lines.push(format!("hash:       {}", w.hash_hex()));
        lines.push(format!(
            "nodes:      {} live, {} peak, {} collections",
            rep.stats.nodes, rep.stats.peak_nodes, rep.stats.collections
        ));
        out["mode"] = "torus".into();
        out["start_generation"] = start_gen.into();
        out["generation"] = w.generation.into();
        out["seconds"] = rep.seconds.into();
        out["states_hash"] = format!("{:016x}", w.states_hash()).into();
        out["hash"] = w.hash_hex().into();
        out["population"] = w.fields[w.cas[0].field].population().into();
        out["stats"] = serde_json::to_value(rep.stats)?;
        if let (Some(how), Some(cw)) = (&a.compare, cmp_world.map_err(anyhow::Error::msg)?) {
            let (rw, secs, backend) = reference(cw, gens, how)?;
            let same = rw.states_hash() == w.states_hash();
            lines.push(format!(
                "compare:    {backend} {secs:.3}s, states {}",
                if same { "equal" } else { "DIFFERENT" }
            ));
            out["compare"] =
                serde_json::json!({"backend": backend, "seconds": secs, "equal": same});
            if !same {
                failed = Some("HashLife and the stepper disagree".to_string());
            }
        }
        if let Some(p) = &a.png {
            let ro = RenderOptions {
                scale: a.scale,
                max_side: a.max_side,
                ..Default::default()
            };
            if let Some(pal) = &palette {
                render::recolor(&mut w, &pal.name);
            }
            let (pw, ph) = render::write_png_streaming(p, &w, &ro, &png_metadata(&w))
                .map_err(anyhow::Error::msg)?;
            lines.push(format!("png:        {} ({pw}x{ph})", p.display()));
        }
        if let Some(p) = &a.save {
            let mut s = w.to_scene();
            s.name = sc.name.clone();
            s.description = sc.description.clone();
            std::fs::write(p, s.to_json()).with_context(|| format!("writing {}", p.display()))?;
            lines.push(format!("saved:      {}", p.display()));
        }
    } else {
        let mut p: Plane = match &scene {
            Some(sc) => {
                let mut sc = sc.clone();
                if let Some(r) = &a.rule {
                    for c in &mut sc.cas {
                        c.rule = r.clone();
                    }
                }
                let w = World::from_scene(&sc).map_err(anyhow::Error::msg)?;
                Plane::from_world(&w, &opts).map_err(anyhow::Error::msg)?
            }
            None => hashlife::plane_from_text(&text, a.rule.as_deref(), &opts)
                .map_err(anyhow::Error::msg)?,
        };
        let pop0 = p.tile_population();
        let t0 = std::time::Instant::now();
        p.step(gens).map_err(anyhow::Error::msg)?;
        let secs = t0.elapsed().as_secs_f64();
        let st = p.stats();
        let bounds = p.bounds();
        lines.push(format!(
            "mode:       plane ({} {})",
            p.tiling.family,
            p.ca().rule
        ));
        lines.push(format!(
            "generation: {} (+{gens} in {secs:.3}s)",
            p.generation
        ));
        lines.push(format!("population: {} (was {pop0})", p.tile_population()));
        match bounds {
            Some(b) => lines.push(format!(
                "bounds:     i {}..{}, j {}..{} ({} × {} unit cells)",
                b[0],
                b[2],
                b[1],
                b[3],
                b[2] - b[0] + 1,
                b[3] - b[1] + 1
            )),
            None => lines.push("bounds:     empty".into()),
        }
        lines.push(format!(
            "nodes:      {} live, {} peak, {} collections",
            st.nodes, st.peak_nodes, st.collections
        ));
        out["mode"] = "plane".into();
        out["generation"] = p.generation.into();
        out["seconds"] = secs.into();
        out["population"] = p.tile_population().to_string().into();
        out["bounds"] = serde_json::json!(bounds.map(|b| b.map(|v| v.to_string())));
        out["stats"] = serde_json::to_value(st)?;
        let src_frame = scene.as_ref().and_then(|sc| match sc.tiling.size {
            Some(SizeSpec::Rect { w, h }) => Some((w as u64, h as u64, sc.tiling.boundary)),
            _ => None,
        });
        let (region, bnd) = match a.region.as_deref() {
            Some("world") => {
                let Some((w, h, b)) = src_frame else {
                    bail!("--region world needs a scene with a w × h size")
                };
                ((0, 0, w, h), b)
            }
            Some(r) => (parse_region(r)?, boundary(&a.boundary)?),
            None => (
                p.default_region(a.margin).unwrap_or((0, 0, 16, 16)),
                boundary(&a.boundary)?,
            ),
        };
        out["region"] = serde_json::json!([region.0, region.1, region.2, region.3]);
        lines.push(format!(
            "region:     i0 {}, j0 {}, {} × {}",
            region.0, region.1, region.2, region.3
        ));
        let cells = region.2 as u128 * region.3 as u128 * p.ntypes() as u128;
        let need_world = a.save.is_some()
            || a.compare.is_some()
            || (a.png.is_some() && cells <= a.max_cells as u128);
        let mut exported = if need_world {
            let sc = p.to_scene(region, bnd).map_err(anyhow::Error::msg)?;
            Some((World::from_scene(&sc).map_err(anyhow::Error::msg)?, sc))
        } else {
            None
        };
        if let Some((ew, _)) = &exported {
            lines.push(format!(
                "states:     {:016x} (exported world)",
                ew.states_hash()
            ));
            out["states_hash"] = format!("{:016x}", ew.states_hash()).into();
        }
        if let Some(how) = &a.compare {
            let Some(sc) = &scene else {
                bail!("--compare needs a scene input")
            };
            if a.region.as_deref() != Some("world") {
                bail!("plane --compare needs --region world");
            }
            let w = World::from_scene(sc).map_err(anyhow::Error::msg)?;
            let (rw, secs, backend) = reference(w, gens, how)?;
            let same = exported.as_ref().unwrap().0.states_hash() == rw.states_hash();
            lines.push(format!(
                "compare:    {backend} {secs:.3}s, states {}",
                if same {
                    "equal"
                } else {
                    "DIFFERENT (edges reached?)"
                }
            ));
            out["compare"] =
                serde_json::json!({"backend": backend, "seconds": secs, "equal": same});
            if !same {
                failed = Some("HashLife and the stepper disagree".to_string());
            }
        }
        if let Some(path) = &a.png {
            let mut meta = vec![
                ("jetcarp:version", VERSION.to_string()),
                ("jetcarp:generation", p.generation.to_string()),
                ("jetcarp:hashlife", format!("plane, region {region:?}")),
            ];
            if cells <= a.max_cells as u128 {
                let (ew, _) = exported.as_mut().unwrap();
                if let Some(pal) = &palette {
                    render::recolor(ew, &pal.name);
                }
                let ew = &*ew;
                let ro = RenderOptions {
                    scale: a.scale,
                    max_side: a.max_side,
                    ..Default::default()
                };
                meta.extend(
                    png_metadata(ew)
                        .into_iter()
                        .filter(|(k, _)| *k != "jetcarp:generation"),
                );
                let (pw, ph) = render::write_png_streaming(path, ew, &ro, &meta)
                    .map_err(anyhow::Error::msg)?;
                lines.push(format!("png:        {} ({pw}x{ph})", path.display()));
            } else {
                let img = match &palette {
                    Some(pal) => {
                        p.render_density_in(region, a.max_side, &hashlife::HlColors::new(pal))
                    }
                    None => p.render_density(region, a.max_side),
                };
                write_png(path, &img, &meta).map_err(anyhow::Error::msg)?;
                lines.push(format!(
                    "png:        {} ({}x{}, density map)",
                    path.display(),
                    img.width,
                    img.height
                ));
            }
        }
        if let Some(path) = &a.save {
            let (_, sc) = exported.as_ref().unwrap();
            std::fs::write(path, sc.to_json())
                .with_context(|| format!("writing {}", path.display()))?;
            lines.push(format!("saved:      {}", path.display()));
        }
    }
    if a.json {
        println!("{}", serde_json::to_string_pretty(&out)?);
    } else {
        for l in lines {
            println!("{l}");
        }
    }
    if let Some(e) = failed {
        bail!("{e}");
    }
    Ok(())
}
