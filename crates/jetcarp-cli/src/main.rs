//! `jetcarp`: run JETCARP scenes headless, print state hashes, export PNGs.

use anyhow::{Context, Result, bail};
use jetcarp_core::render::{RenderOptions, png_metadata, write_png};
use jetcarp_core::*;
use clap::{Parser, Subcommand};
use std::path::PathBuf;

mod ants_cmd;
mod app_cmd;
mod demo_cmd;
mod evolve_cmd;
mod explore_cmd;
mod hashlife_cmd;
mod host_cmd;
mod hunt_cmd;
mod library;
mod media_cmd;
mod movers_cmd;
mod pile_cmd;
mod pipeline_cmd;
mod research;
mod script_cmd;
mod stage1_cmd;
mod wallpaper_cmd;

#[derive(Parser)]
#[command(
    name = "jetcarp",
    version,
    about = "JETCARP cellular automata research tool (command line)"
)]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Load a scene, run it, and report generation, hash, and populations.
    Run {
        scene: PathBuf,
        /// Generations to run after replaying the scene's own commands.
        #[arg(long, default_value_t = 0)]
        gens: u64,
        /// Write a PNG of the final state (scene, version, and hash are embedded as text chunks).
        #[arg(long)]
        png: Option<PathBuf>,
        /// Pixels per world unit (tile edges are about 1 unit). Default: fit into --max-side.
        #[arg(long)]
        scale: Option<f64>,
        #[arg(long, default_value_t = 1600)]
        max_side: u32,
        /// Save the resulting scene (with its generation and hash recorded).
        #[arg(long)]
        save: Option<PathBuf>,
        /// Worker threads (default: all cores). The hash must not depend on this.
        #[arg(long)]
        threads: Option<usize>,
        /// Fail unless the final hash equals this.
        #[arg(long)]
        expect: Option<String>,
        /// Print the summary as JSON.
        #[arg(long)]
        json: bool,
        /// Step the --gens generations on the GPU where the rules allow it (results are identical).
        #[arg(long)]
        gpu: bool,
        /// Install automatic reseed policies before running: one policy or a list, as JSON
        /// (README "Reseeding"), e.g.
        /// '{"mode":{"kind":"blobs"},"when":{"kind":"settled"},"cooldown":200}'.
        #[arg(long)]
        reseed: Option<String>,
        /// Reseed once before running, with a mode as JSON, e.g. '{"kind":"rain","drops":40}'.
        #[arg(long)]
        reseed_now: Option<String>,
    },
    /// Desktop wallpapers: a scene or Quick Start entry at a monitor's size, a span of monitors,
    /// or one image per monitor cut so they line up.
    Wallpaper(Box<wallpaper_cmd::WallpaperArgs>),
    /// List the palettes with their background colours.
    Palettes,
    /// Check that a saved scene still replays to its recorded hash.
    Verify { scene: PathBuf },
    /// Contact sheet: one thumbnail per elementary rule, labelled with its number.
    Sheet {
        /// w (squares, 0-255), hexw (binary hexagon, 0-15), hexw3 (ternary hexagon, 0-19682),
        /// hext (flat-top hexagons, three parents, 0-255), hext3 (0 to 3^27 - 1), or hext3sum
        /// (totalistic ternary three-parent rules, 0-2186).
        #[arg(long, default_value = "w")]
        kind: String,
        #[arg(long, default_value_t = 0)]
        from: u64,
        #[arg(long, default_value_t = 64)]
        count: u32,
        #[arg(long, default_value_t = 8)]
        cols: u32,
        /// Cells across each thumbnail.
        #[arg(long, default_value_t = 81)]
        width: u32,
        #[arg(long, default_value_t = 180)]
        thumb: u32,
        /// Start from a random top row with this density instead of one cell.
        #[arg(long)]
        random_row: Option<f64>,
        #[arg(long, default_value = "Ember")]
        palette: String,
        #[arg(long)]
        png: PathBuf,
        /// A sweep folder: lay out its pictures instead of running rules (with --runs).
        #[arg(long)]
        folder: Option<PathBuf>,
        /// With --folder: the runs, comma separated: rule text ("HexT3 sum 588"), manifest
        /// index ("#12" or "12"), or series number ("n=588").
        #[arg(long, value_delimiter = ',')]
        runs: Vec<String>,
        /// With --folder: a label per run, comma separated (default: "#index"); the rule is
        /// printed under it.
        #[arg(long, value_delimiter = ',')]
        labels: Vec<String>,
        /// Largest label scale (the 3x5 font at this many pixels per dot).
        #[arg(long, default_value_t = 2)]
        label_scale: u32,
    },
    /// Rule census: run rules from seeded soups and classify each run (CSV, JSON, contact sheet).
    Census(Box<research::CensusArgs>),
    /// Find still lifes, oscillators, and spaceships in a scene after running it.
    Objects(research::ObjectsArgs),
    /// Pattern library: search soups, catalog, collide, paste, promote to Quick Start.
    Library(library::LibraryArgs),
    /// Broad search over tilings, rules, seeds and palettes: a GPU+CPU search kept in SQLite,
    /// with contact sheets for reviewing the results.
    Explore(Box<explore_cmd::ExploreArgs>),
    /// Fast soup hunter: small soups on an unbounded plane, bit-parallel, cataloged as a
    /// resumable job (`--resume JOB`, `--status JOB`, `--stop JOB`, `--list`).
    Hunt(Box<hunt_cmd::HuntArgs>),
    /// Ant and turmite search: turn strings and turmite tables on unbounded periodic tilings,
    /// highways proved exactly, every run classed and cataloged by a resumable job (`search`,
    /// `status`, `stop`, `run`, `list`, `show`, `census`, `update`, `remove`, `scene`,
    /// `promote`).
    Ants(Box<ants_cmd::AntsArgs>),
    /// Stage 1 settings search: score rules (with soup density, size, grid, generations) for
    /// visual interest, refine, expand to neighbours; the best go to the Quick Start. Resumable.
    Stage1(Box<stage1_cmd::Stage1Args>),
    /// Catalog pipeline: stage 1 on every grid, contact sheets and a local model's shortlist,
    /// the score refit, stage 2 hunts, media, and the website export, as one resumable job
    /// (`run --mode quick|standard|full`, `resume`, `status`, `stop`, `report`).
    Pipeline(Box<pipeline_cmd::PipelineArgs>),
    /// Evolutionary glider search (Edmond's method) on any tiling; verified movers go to the library.
    Evolve(Box<evolve_cmd::EvolveArgs>),
    /// Movers on aperiodic tilings: track across growing patch levels, verified to a distance.
    Movers(movers_cmd::MoversArgs),
    /// Jump a pattern (unbounded plane) or a wrap-edge world (power-of-two torus) far ahead
    /// with HashLife; export a region as a scene or PNG.
    Hashlife(Box<hashlife_cmd::HashlifeArgs>),
    /// Computing demos: a WireWorld adder and Life logic gates for any inputs.
    Demo(demo_cmd::DemoArgs),
    /// Large sandpile: N grains on one spot of an unbounded grid, built by doubling, with a
    /// cropped PNG, a preview, metadata, and a resumable pile file per step.
    Pile(Box<pile_cmd::PileArgs>),
    /// Media for the catalog: per-step frames, a GIF or animated WebP, a poster, and a
    /// thumbnail for a pattern (alone), a Quick Start entry or setting (from the seed), or a
    /// scene; `--missing` renders everything missing or stale.
    Media(media_cmd::MediaArgs),
    /// Website export: the public catalog as JSON and media with a manifest, incrementally.
    Site(media_cmd::SiteArgs),
    /// List tiling families with their neighbour counts.
    Tilings,
    /// Print the settings, data, and output folders JETCARP uses (environment overrides
    /// included).
    Paths {
        /// Print them as JSON.
        #[arg(long)]
        json: bool,
    },
    /// List Kaplan's isohedral types (IH01-IH93), or draw them all on a contact sheet.
    Isohedral {
        /// Write a contact sheet of all 81 types here instead of listing them.
        #[arg(long)]
        png: Option<PathBuf>,
        /// Keep edges straight (default: random edge shapes within each type's constraints).
        #[arg(long)]
        straight: bool,
        #[arg(long, default_value_t = 9)]
        cols: u32,
        #[arg(long, default_value_t = 200)]
        thumb: u32,
        #[arg(long, default_value_t = 7)]
        seed: u64,
    },
    /// List named rules and notation.
    Rules,
    /// Run a Rhai script: worlds, runs, images, parallel rule sweeps (see README "Scripting").
    Script(script_cmd::ScriptArgs),
    /// Control the running JETCARP app (AI control on, or `jetcarp-app --control`): `status`,
    /// `screensaver on|off [--profile NAME]`, `profiles`, `display ...`, `play`, `pause`,
    /// `speed N`, `next`, `previous`,
    /// `call <tool> <json>`.
    App(app_cmd::AppArgs),
    /// The background host, which keeps scenes running with no app window: `start`, `stop`,
    /// `status`, `next`, `previous`, `pause`, `resume`, `rotation`, `caption`, `fps`, `login`.
    Host(host_cmd::HostArgs),
    /// MCP server on stdin/stdout so AI tools (LM Studio, Claude Code) can drive JETCARP.
    Mcp {
        /// Act on the world in the running JETCARP app (switch on AI control there first).
        #[arg(long)]
        app: bool,
        /// App port (default: the one the app recorded when AI control was switched on).
        #[arg(long)]
        port: Option<u16>,
        /// App token (default: the recorded one).
        #[arg(long)]
        token: Option<String>,
    },
    /// Quick Start examples: list them, check that each one works, or render a contact sheet.
    Quickstart {
        /// Run each example (at preview size) and report whether it is alive and changing.
        #[arg(long)]
        check: bool,
        /// Try a range of starting patterns for each example and report how each does.
        #[arg(long)]
        survey: bool,
        /// For each tiling family, report the first candidate rule that stays lively (used to
        /// choose the tiling showcases).
        #[arg(long)]
        survey_tilings: bool,
        /// Generations for --check and --sheet.
        #[arg(long, default_value_t = 300)]
        gens: u64,
        /// Write a numbered contact sheet of every example (numbers match the list).
        #[arg(long)]
        sheet: Option<PathBuf>,
        /// Only entries whose name, rule, family, or tiling contains this text.
        #[arg(long)]
        filter: Option<String>,
        #[arg(long, default_value_t = 160)]
        thumb: u32,
        #[arg(long, default_value_t = 10)]
        cols: u32,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    // Scripts may ask for GPU toppling of large sandpiles.
    jetcarp_core::pile::set_gpu_factory(|| {
        jetcarp_gpu::pile::PileGpu::headless()
            .map(|g| Box::new(g) as Box<dyn jetcarp_core::pile::Toppler>)
    });
    match cli.cmd {
        Cmd::Run {
            scene,
            gens,
            png,
            scale,
            max_side,
            save,
            threads,
            expect,
            json,
            gpu,
            reseed,
            reseed_now,
        } => {
            if let Some(t) = threads {
                rayon::ThreadPoolBuilder::new()
                    .num_threads(t)
                    .build_global()
                    .context("thread pool")?;
            }
            let text = std::fs::read_to_string(&scene)
                .with_context(|| format!("reading {}", scene.display()))?;
            let sc = Scene::from_json(&text).map_err(anyhow::Error::msg)?;
            let t0 = std::time::Instant::now();
            let mut w = World::from_scene(&sc).map_err(anyhow::Error::msg)?;
            if let Some(text) = &reseed {
                let v: serde_json::Value =
                    serde_json::from_str(text).context("--reseed: not JSON")?;
                let list = if v.is_array() {
                    v
                } else {
                    serde_json::json!([v])
                };
                let policies: Vec<jetcarp_core::reseed::AutoReseed> =
                    serde_json::from_value(list).context("--reseed: not a reseed policy")?;
                w.apply(Command::SetReseed { policies })
                    .map_err(anyhow::Error::msg)?;
            }
            if let Some(text) = &reseed_now {
                let mode: jetcarp_core::reseed::ReseedMode =
                    serde_json::from_str(text).context("--reseed-now: not a reseed mode")?;
                w.apply(Command::Reseed { ca: None, mode })
                    .map_err(anyhow::Error::msg)?;
            }
            let cpu = format!("cpu ({} threads)", rayon::current_num_threads());
            let backend = match gpu.then(jetcarp_gpu::GpuSim::headless) {
                Some(Ok(mut sim)) => {
                    let m = sim.step_mixed(&mut w, gens).map_err(anyhow::Error::msg)?;
                    match m.fallback {
                        None => format!("gpu ({})", sim.name()),
                        Some(why) => format!(
                            "gpu ({}); {} of {gens} generations on the {cpu}: {why}",
                            sim.name(),
                            m.cpu_gens
                        ),
                    }
                }
                other => {
                    if gens > 0 {
                        w.apply(Command::Step { gens })
                            .map_err(anyhow::Error::msg)?;
                    }
                    match other {
                        Some(Err(e)) => format!("{cpu}; no GPU: {e}"),
                        _ => cpu,
                    }
                }
            };
            let secs = t0.elapsed().as_secs_f64();
            let hash = w.hash_hex();
            let mut png_size = None;
            if let Some(p) = &png {
                // Streams in strips, so very large images (e.g. --scale 2 on a huge grid) fit.
                let opts = RenderOptions {
                    scale,
                    max_side,
                    ..Default::default()
                };
                png_size = Some(
                    jetcarp_core::render::write_png_streaming(p, &w, &opts, &png_metadata(&w))
                        .map_err(anyhow::Error::msg)?,
                );
            }
            if let Some(p) = &save {
                let mut out = w.to_scene();
                out.name = sc.name.clone();
                out.description = sc.description.clone();
                out.view = sc.view.clone();
                std::fs::write(p, out.to_json())
                    .with_context(|| format!("writing {}", p.display()))?;
            }
            let fields: Vec<serde_json::Value> = w
                .fields
                .iter()
                .enumerate()
                .map(|(i, f)| {
                    let users: Vec<String> = w
                        .cas
                        .iter()
                        .filter(|c| c.field == i)
                        .map(|c| c.name.clone())
                        .collect();
                    serde_json::json!({
                        "field": i, "kind": f.kind.name(), "shared": f.shared,
                        "population": f.population(), "lost": f.lost, "cas": users,
                    })
                })
                .collect();
            if json {
                let v = serde_json::json!({
                    "scene": scene, "tiling": w.topo.describe(), "cells": w.topo.len(),
                    "generation": w.generation, "hash": hash, "status": w.status().to_string(),
                    "done": w.done(), "done_reason": w.done_reason(),
                    "dead_boundary": w.topo.has_boundary(), "threads": rayon::current_num_threads(),
                    "jetcarp_version": VERSION, "seconds": secs, "fields": fields, "backend": backend,
                    "png": png.as_ref().map(|p| p.display().to_string()), "png_size": png_size,
                    "reseed": reseed_json(&w),
                });
                println!("{}", serde_json::to_string_pretty(&v)?);
            } else {
                println!("scene:      {}", scene.display());
                println!("tiling:     {}", w.topo.describe());
                for warning in &w.topo.warnings {
                    println!("warning:    {warning}");
                }
                println!("backend:    {backend}");
                println!("generation: {}", w.generation);
                println!("hash:       {hash}");
                println!("status:     {}", w.status());
                match w.done_reason() {
                    Some(r) => println!("done:       yes ({r})"),
                    None => println!("done:       no"),
                }
                for (p, st) in w.reseed_policies().iter().zip(w.reseed_status()) {
                    let err = st
                        .error
                        .as_deref()
                        .map(|e| format!(" ({e})"))
                        .unwrap_or_default();
                    println!(
                        "reseed:     {} {}: fired {} times, last at {}{err}",
                        p.mode.name(),
                        match p.when {
                            jetcarp_core::reseed::ReseedWhen::Settled { .. } => "when settled".into(),
                            jetcarp_core::reseed::ReseedWhen::Every { gens } =>
                                format!("every {gens}"),
                        },
                        st.fired,
                        st.last_fired.map_or("-".into(), |g| g.to_string()),
                    );
                }
                if let Some(sc) = &w.script {
                    let st = &w.script_state;
                    let name = sc.steps.get(st.step).map(|x| x.name.as_str()).unwrap_or("");
                    let fin = if st.finished { " (finished)" } else { "" };
                    println!(
                        "script:     step {} of {}: {name}{fin}",
                        st.step + 1,
                        sc.steps.len()
                    );
                }
                for (i, f) in w.fields.iter().enumerate() {
                    let users: Vec<String> = w
                        .cas
                        .iter()
                        .filter(|c| c.field == i)
                        .map(|c| format!("{} [{}]", c.name, c.notation()))
                        .collect();
                    let what = if f.kind == rules::FieldKind::Height {
                        "grains"
                    } else {
                        "live"
                    };
                    let lost = if f.kind == rules::FieldKind::Height {
                        format!(", {} lost off edges", f.lost)
                    } else {
                        String::new()
                    };
                    println!(
                        "field {i}:    {} {}, {} {what}{lost}: {}",
                        if f.shared { "shared" } else { "private" },
                        f.kind.name(),
                        f.population(),
                        users.join(", ")
                    );
                }
                if let (Some(p), Some((pw, ph))) = (&png, png_size) {
                    println!("png:        {} ({pw}x{ph})", p.display());
                }
                if let Some(p) = &save {
                    println!("saved:      {}", p.display());
                }
                println!(
                    "time:       {secs:.2}s on {} threads",
                    rayon::current_num_threads()
                );
            }
            if let Some(e) = expect
                && e != hash
            {
                bail!("hash mismatch: expected {e}, got {hash}");
            }
        }
        Cmd::Verify { scene } => {
            let text = std::fs::read_to_string(&scene)?;
            let sc = Scene::from_json(&text).map_err(anyhow::Error::msg)?;
            let Some(rec) = sc.record.clone() else {
                bail!("scene has no recorded hash")
            };
            let w = World::from_scene(&sc).map_err(anyhow::Error::msg)?;
            if w.generation != rec.generation || w.hash_hex() != rec.hash {
                bail!(
                    "MISMATCH: recorded gen {} hash {}, replayed gen {} hash {}",
                    rec.generation,
                    rec.hash,
                    w.generation,
                    w.hash_hex()
                );
            }
            println!("ok: generation {} hash {}", w.generation, rec.hash);
        }
        Cmd::Sheet {
            kind,
            from,
            count,
            cols,
            width,
            thumb,
            random_row,
            palette,
            png,
            folder,
            runs,
            labels,
            label_scale,
        } => {
            if let Some(folder) = folder {
                let m =
                    jetcarp_script::results::Manifest::load(&folder).map_err(anyhow::Error::msg)?;
                if runs.is_empty() {
                    anyhow::bail!("give the runs to show with --runs (rule, #index, or n=number)");
                }
                let mut items = vec![];
                for (k, key) in runs.iter().enumerate() {
                    let row = m
                        .find(key)
                        .ok_or_else(|| anyhow::anyhow!("no run '{key}' in {}", folder.display()))?;
                    let r = &m.rows[row];
                    let label = labels
                        .get(k)
                        .cloned()
                        .unwrap_or_else(|| format!("#{}", r["index"]));
                    items.push((
                        row,
                        vec![label, jetcarp_script::results::text(r, "rule").into()],
                    ));
                }
                let img = jetcarp_script::results::contact_sheet(&m, &items, thumb, cols, label_scale)
                    .map_err(anyhow::Error::msg)?;
                let meta = vec![
                    ("jetcarp:version", VERSION.to_string()),
                    (
                        "jetcarp:sheet",
                        format!("{} runs {}", m.name(), runs.join(",")),
                    ),
                ];
                write_png(&png, &img, &meta).map_err(anyhow::Error::msg)?;
                println!("sheet: {} ({}x{})", png.display(), img.width, img.height);
                return Ok(());
            }
            let kind = jetcarp_core::sheet::SheetKind::parse(&kind).ok_or_else(|| {
                anyhow::anyhow!("kind is w, hexw, hexw3, hext, hext3, or hext3sum")
            })?;
            let o = jetcarp_core::sheet::SheetOptions {
                kind,
                first: from,
                count,
                cols,
                width,
                thumb_px: thumb,
                random_row,
                rng_state: 1,
                palette,
            };
            let img = jetcarp_core::sheet::contact_sheet(&o).map_err(anyhow::Error::msg)?;
            let meta = vec![
                ("jetcarp:version", VERSION.to_string()),
                (
                    "jetcarp:sheet",
                    format!("{kind:?} {from}+{count} width {width} random_row {random_row:?}"),
                ),
            ];
            write_png(&png, &img, &meta).map_err(anyhow::Error::msg)?;
            println!("sheet: {} ({}x{})", png.display(), img.width, img.height);
        }
        Cmd::Census(a) => research::census(*a)?,
        Cmd::Script(a) => script_cmd::script(a)?,
        Cmd::Objects(a) => research::objects(a)?,
        Cmd::Library(a) => library::run(a)?,
        Cmd::Explore(a) => explore_cmd::run(*a)?,
        Cmd::Hunt(a) => hunt_cmd::run(*a)?,
        Cmd::Ants(a) => ants_cmd::run(*a)?,
        Cmd::Stage1(a) => stage1_cmd::run(*a)?,
        Cmd::Pipeline(a) => pipeline_cmd::pipeline(*a)?,
        Cmd::Evolve(a) => evolve_cmd::run(*a)?,
        Cmd::Movers(a) => movers_cmd::run(a)?,
        Cmd::Hashlife(a) => hashlife_cmd::run(*a)?,
        Cmd::Demo(a) => demo_cmd::run(a)?,
        Cmd::Pile(a) => pile_cmd::run(*a)?,
        Cmd::Media(a) => media_cmd::run(a)?,
        Cmd::Site(a) => media_cmd::run_site(a)?,
        Cmd::Wallpaper(a) => wallpaper_cmd::run(*a)?,
        Cmd::Paths { json } => {
            use jetcarp_core::paths;
            if json {
                let v = serde_json::json!({
                    "settings": paths::config_dir(),
                    "data": paths::data_dir(),
                    "output": paths::output_dir(),
                });
                println!("{}", serde_json::to_string_pretty(&v)?);
            } else {
                print!("{}", paths::describe());
            }
        }
        Cmd::Palettes => {
            let hex = |c: [u8; 4]| format!("#{:02x}{:02x}{:02x}", c[0], c[1], c[2]);
            for name in jetcarp_core::render::palette_names() {
                let p = jetcarp_core::render::Palette::builtin(name).unwrap();
                let stops: Vec<String> = p
                    .stops
                    .iter()
                    .map(|c| hex([c[0], c[1], c[2], 255]))
                    .collect();
                println!(
                    "{name:10} background {} stage {} stops {}",
                    hex(p.background_rgba()),
                    hex(p.stage_rgba()),
                    stops.join(" ")
                );
            }
        }
        Cmd::Tilings => {
            for f in families::FAMILIES {
                let spec = TilingSpec::new(f.id);
                match Topology::build(&spec) {
                    Ok(t) => {
                        let (e, to) = t.degree_sets();
                        println!(
                            "{:9} {:28} {:5} storage {:9} edge {:?} touch {:?}",
                            f.id,
                            f.name,
                            f.sym,
                            t.storage_name(),
                            e,
                            to
                        );
                    }
                    Err(e) => println!("{:9} error: {e}", f.id),
                }
            }
        }
        Cmd::Quickstart {
            survey_tilings: true,
            gens,
            ..
        } => survey_tilings(gens)?,
        Cmd::Quickstart {
            check,
            survey,
            gens,
            sheet,
            filter,
            thumb,
            cols,
            ..
        } => quickstart_cmd(check, survey, gens, sheet, filter, thumb, cols)?,
        Cmd::Isohedral {
            png,
            straight,
            cols,
            thumb,
            seed,
        } => match png {
            Some(png) => {
                let o = jetcarp_core::isohedral::sheet::IhSheetOptions {
                    cols,
                    thumb_px: thumb,
                    curved: !straight,
                    rng_state: seed,
                    ..Default::default()
                };
                let img =
                    jetcarp_core::isohedral::sheet::contact_sheet(&o).map_err(anyhow::Error::msg)?;
                let meta = vec![
                    ("jetcarp:version", VERSION.to_string()),
                    (
                        "jetcarp:sheet",
                        format!("isohedral curved {} seed {seed}", !straight),
                    ),
                ];
                write_png(&png, &img, &meta).map_err(anyhow::Error::msg)?;
                println!("sheet: {} ({}x{})", png.display(), img.width, img.height);
            }
            None => {
                for (f, t) in jetcarp_core::isohedral::IH_FAMILIES
                    .iter()
                    .zip(&jetcarp_core::isohedral::IH_TYPES)
                {
                    match Topology::build(&TilingSpec::new(f.id)) {
                        Ok(topo) => {
                            let (e, to) = topo.degree_sets();
                            println!(
                                "{} {:5} params {} aspects {:2} edge {:?} touch {:?}",
                                f.id, f.sym, t.np, t.na, e, to
                            );
                        }
                        Err(e) => println!("{} error: {e}", f.id),
                    }
                }
            }
        },
        Cmd::App(a) => app_cmd::run(a)?,
        Cmd::Host(a) => host_cmd::run(a)?,
        Cmd::Mcp { app, port, token } => {
            jetcarp_mcp::stdio::run(app, port, token).map_err(anyhow::Error::msg)?
        }
        Cmd::Rules => {
            for e in rules::catalog() {
                println!("{:30} {:15} {}", e.name, e.notation, e.blurb);
            }
            println!(
                "\nAny B/S rule (B3/S23, add V for edge neighbours), Generations B2/S/C3, W0..W255, HexW 0..15, HexW3 0..19682,\nHexT 0..255 / HexT3 n / HexT3 sum 0..2186 (flat-top hexagons), Margolus <16 block outputs> or Margolus BBM,\nLenia R=13,T=10,b=1,m=0.15,s=0.015, Ant RL, Turmite LLRR. See the README for every family."
            );
        }
    }
    Ok(())
}

fn quickstart_cmd(
    check: bool,
    survey: bool,
    gens: u64,
    sheet: Option<PathBuf>,
    filter: Option<String>,
    thumb: u32,
    cols: u32,
) -> Result<()> {
    use jetcarp_core::quickstart;
    use rayon::prelude::*;
    let all = quickstart::entries();
    let f = filter.map(|s| s.to_lowercase());
    let picked: Vec<(usize, &quickstart::QuickStart)> = all
        .iter()
        .enumerate()
        .filter(|(_, e)| {
            f.as_ref().is_none_or(|f| {
                [&e.scene.name, &e.rule, &e.family, &e.tiling]
                    .iter()
                    .any(|x| x.to_lowercase().contains(f.as_str()))
            })
        })
        .collect();
    if survey {
        let mut seeds: Vec<SeedSpec> =
            [0.005, 0.01, 0.03, 0.1, 0.2, 0.3, 0.4, 0.45, 0.5, 0.55, 0.6]
                .iter()
                .map(|&d| SeedSpec::Random { density: d })
                .collect();
        for (r, d) in [(0.08, 0.5), (0.2, 0.4), (0.5, 0.3), (0.3, 0.55), (0.5, 0.5)] {
            seeds.push(SeedSpec::Blob {
                radius: r,
                density: d,
            });
        }
        for (k, e) in &picked {
            println!("{k:3} {} {}", e.scene.name, e.rule);
            let rows: Vec<String> = seeds
                .par_iter()
                .map(|sd| {
                    let mut sc = e.scene.clone();
                    sc.cas[0].seed = Some(sd.clone());
                    match quickstart::health(&sc, gens, quickstart::preview_cells(&e.scene, 6000)) {
                        Ok(h) => format!(
                            "      {:44} {} pop {:.3} act {:.3} {}",
                            format!("{sd:?}"),
                            if h.works() { "ok  " } else { "FAIL" },
                            h.population as f64 / h.cells as f64,
                            h.activity,
                            h.status
                        ),
                        Err(err) => format!("      {sd:?} error {err}"),
                    }
                })
                .collect();
            for r in rows {
                println!("{r}");
            }
        }
        return Ok(());
    }
    let health: Vec<Option<quickstart::Health>> = if check {
        picked
            .par_iter()
            .map(|(_, e)| {
                quickstart::health(&e.scene, gens, quickstart::preview_cells(&e.scene, 6000)).ok()
            })
            .collect()
    } else {
        vec![None; picked.len()]
    };
    let mut bad = 0;
    for ((k, e), h) in picked.iter().zip(&health) {
        let tail = match h {
            Some(h) => {
                if !h.works() {
                    bad += 1;
                }
                format!(
                    "  {} pop {:.3} act {:.3} {}",
                    if h.works() { "ok  " } else { "FAIL" },
                    h.population as f64 / h.cells as f64,
                    h.activity,
                    h.status
                )
            }
            None if check => "  FAIL could not load".to_string(),
            None => String::new(),
        };
        println!(
            "{:3} {:34} {:24} {:20} {:24}{}",
            k, e.scene.name, e.rule, e.family, e.tiling, tail
        );
    }
    if check {
        println!("{} examples, {} not working", picked.len(), bad);
    }
    if let Some(p) = sheet {
        // Numbers on the sheet are list positions, so render in runs of consecutive indices.
        let scenes: Vec<&Scene> = picked.iter().map(|(_, e)| &e.scene).collect();
        let first = picked.first().map(|x| x.0 as u32).unwrap_or(0);
        let contiguous = picked.windows(2).all(|w| w[1].0 == w[0].0 + 1);
        let img = quickstart::contact_sheet(
            &scenes,
            if contiguous { first } else { 0 },
            gens,
            thumb,
            cols,
        );
        write_png(&p, &img, &[]).map_err(anyhow::Error::msg)?;
        println!("wrote {} ({}x{})", p.display(), img.width, img.height);
    }
    Ok(())
}

fn survey_tilings(gens: u64) -> Result<()> {
    use jetcarp_core::quickstart::{health, preview_scene};
    use rayon::prelude::*;
    const CANDIDATES: &[&str] = &[
        "B3/S23",
        "B36/S23",
        "B3/S23V",
        "B2/S34",
        "B2/S34V",
        "B34/S34",
        "B2/S23V",
        "B35/S234",
        "B2/S/C3",
        "B2/S/C3V",
        "B23/S34/C5",
        "B2/S3/C4V",
    ];
    let mut fams: Vec<String> = jetcarp_core::families::FAMILIES
        .iter()
        .filter(|f| f.id != "periodic")
        .map(|f| f.id.to_string())
        .collect();
    fams.extend(
        [
            "ih01", "ih07", "ih21", "ih28", "ih41", "ih55", "ih74", "ih81", "ih90",
        ]
        .map(String::from),
    );
    let rows: Vec<String> = fams
        .par_iter()
        .map(|fam| {
            let size = if jetcarp_core::families::family(fam)
                .is_some_and(|f| f.kind == jetcarp_core::families::FamilyKind::Aperiodic)
            {
                SizeSpec::Level(jetcarp_core::aperiodic::default_level(fam))
            } else {
                SizeSpec::Cells(6000)
            };
            let mut found = Vec::new();
            for r in CANDIDATES {
                let mut sc = Scene::new(
                    TilingSpec::new(fam)
                        .with_size(size)
                        .with_boundary(Boundary::Wrap),
                );
                sc.cas = vec![CaSpec::new(r).seed(SeedSpec::Random { density: 0.35 })];
                if let Ok(h) = health(&preview_scene(&sc, 6000), gens, None) {
                    let pop = h.population as f64 / h.cells as f64;
                    if h.works() && h.activity >= 0.01 && (0.03..0.7).contains(&pop) {
                        found.push(format!("{r} (pop {pop:.2} act {:.2})", h.activity));
                    }
                }
            }
            format!("{fam:12} {}", found.join(" | "))
        })
        .collect();
    for r in rows {
        println!("{r}");
    }
    Ok(())
}

/// Reseed policies and their status, for `jetcarp run --json`.
fn reseed_json(w: &World) -> serde_json::Value {
    w.reseed_policies()
        .iter()
        .zip(w.reseed_status())
        .map(|(p, st)| serde_json::json!({"policy": p, "status": st}))
        .collect()
}
