//! `jetcarp`: JETCARP's one program. With no subcommand it opens the app; the subcommands are
//! the command line (run scenes headless, print state hashes, export PNGs, search, catalogue),
//! the background host (`jetcarp host serve`), and the MCP server (`jetcarp mcp`).

use anyhow::{Result, bail};
use clap::{CommandFactory, Parser, Subcommand};
use jetcarp_api::op::{self, NoInput, Op, Progress, Quiet, Report};
use jetcarp_api::ops::{quickstart, reference, scene, sheet, system};
use std::path::PathBuf;

mod ants_cmd;
mod api_cmd;
mod app_cmd;
mod demo_cmd;
mod hashlife_cmd;
mod host_cmd;
mod jobs_cmd;
mod library;
mod media_cmd;
mod movers_cmd;
mod platform;
mod research;
mod script_cmd;
mod search_cmd;
mod wallpaper_cmd;

#[derive(Parser)]
#[command(
    name = "jetcarp",
    version,
    about = "JETCARP: Jimmie's Euclidean Tilings and Cellular Automata Research Program",
    long_about = "JETCARP: Jimmie's Euclidean Tilings and Cellular Automata Research Program.\n\n\
                  With no subcommand, `jetcarp` opens the app (the app options below apply). \
                  The subcommands run JETCARP from the command line.",
    args_conflicts_with_subcommands = true
)]
struct Cli {
    /// Print the result as JSON instead of text (commands that report a result).
    #[arg(long, global = true)]
    json: bool,
    #[command(flatten)]
    app: jetcarp_app::LaunchArgs,
    #[command(subcommand)]
    cmd: Option<Cmd>,
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
        /// Step the --gens generations on the GPU where the rules allow it (results are identical).
        #[arg(long)]
        gpu: bool,
        /// Install automatic reseed policies before running: one policy or a list, as JSON
        /// (the book's "Reseeding" guide), e.g.
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
    /// Find still lifes, oscillators, and spaceships in a scene after running it.
    Objects(research::ObjectsArgs),
    /// Pattern library: list, show, curate, paste, promote to Quick Start; its ant runs and
    /// movers.
    Library(library::LibraryArgs),
    /// Start or carry on a search as a resumable catalog job: `hunt` (soup), `bounded`, `ants`,
    /// `census`, `mover`, `pile`, `collide`, `evolve`, `explore`; or `try` one configuration.
    Search(search_cmd::SearchArgs),
    /// The catalog's jobs, whatever started them: `list`, `status`, `stop`, `resume`, `report`,
    /// `remove`; an explore job's `runs`, `sheet`, `scene`, `review`, `clips`, `rescore`.
    Jobs(jobs_cmd::JobsArgs),
    /// Jump a pattern (unbounded plane) or a wrap-edge world (power-of-two torus) far ahead
    /// with HashLife; export a region as a scene or PNG.
    Hashlife(Box<hashlife_cmd::HashlifeArgs>),
    /// Computing demos: a WireWorld adder and Life logic gates for any inputs.
    Demo(demo_cmd::DemoArgs),
    /// Large sandpile: N grains on one spot of an unbounded grid, built by doubling, with a
    /// cropped PNG, a preview, metadata, and a resumable pile file per step (the same as
    /// `jetcarp search pile`).
    Pile(Box<search_cmd::PileArgs>),
    /// Media for the catalog: per-step frames, a GIF or animated WebP, a poster, and a
    /// thumbnail for a pattern (alone), a Quick Start entry (from the seed), an ant run, or a
    /// scene; `--missing` renders everything missing or stale.
    Media(media_cmd::MediaArgs),
    /// Website export: the public catalog as JSON and media with a manifest and a viewer page,
    /// incrementally.
    Site(media_cmd::SiteArgs),
    /// List tiling families with their neighbour counts.
    Tilings,
    /// What JETCARP sees on this machine: version, folders, graphics adapters, desktop
    /// integration. Include it in bug reports.
    Doctor,
    /// Undo what JETCARP set up outside its folders (login entry, GNOME extension, screen
    /// saver), and optionally delete its settings and data. Shows the plan unless --yes.
    Uninstall(platform::UninstallArgs),
    /// Print the settings, data, and output folders JETCARP uses (environment overrides
    /// included).
    Paths,
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
    /// Run a Rhai script: worlds, runs, images, parallel rule sweeps (see the book's "Scripting (Rhai) API" reference).
    Script(script_cmd::ScriptArgs),
    /// Control the running JETCARP app (AI control on, or `jetcarp --control`): `status`,
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
    /// JETCARP's operations by name, for automation and tests: `list`, `schema OP`,
    /// `call OP [JSON]` (the result as JSON).
    #[command(hide = true)]
    Api(api_cmd::ApiArgs),
    /// Quick Start examples: list them, check that each one works, or render a contact sheet;
    /// `publish` an explore job's best runs as the atlas the menu offers.
    #[command(args_conflicts_with_subcommands = true)]
    Quickstart {
        #[command(subcommand)]
        sub: Option<QuickstartSub>,
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

/// The arguments, with a `--json` given before the subcommand moved after it: the app's flags
/// cannot be mixed with subcommands, but `--json` applies to them (`jetcarp --json paths`).
fn args() -> Vec<std::ffi::OsString> {
    let mut args: Vec<std::ffi::OsString> = std::env::args_os().collect();
    let cmd = Cli::command();
    let names: Vec<String> = cmd
        .get_subcommands()
        .flat_map(|c| {
            std::iter::once(c.get_name().to_string()).chain(c.get_all_aliases().map(String::from))
        })
        .collect();
    let sub = args
        .iter()
        .skip(1)
        .position(|a| a.to_str().is_some_and(|a| names.iter().any(|n| n == a)))
        .map(|i| i + 1);
    if let Some(sub) = sub
        && let Some(j) = args[1..sub].iter().position(|a| a == "--json")
    {
        let flag = args.remove(j + 1);
        args.insert(sub, flag);
    }
    args
}

/// Print an operation's output, as JSON with `--json`, else as text (and its notes on standard
/// error); then fail if the output's own check failed (`run --expect`).
pub(crate) fn show<T: serde::Serialize + Report>(out: &T, json: bool) -> Result<()> {
    if json {
        println!("{}", op::to_pretty(out));
    } else {
        print!("{}", out.text());
        eprint!("{}", out.notes());
    }
    out.check().map_err(anyhow::Error::new)
}

/// Progress as one line on standard error, rewritten in place (`\r`); the caller ends it.
pub(crate) struct ProgressLine;
impl Progress for ProgressLine {
    fn report(&self, _done: u64, _total: Option<u64>, msg: &str) {
        eprint!("\r{msg}");
    }
}

/// Commands that print no result of their own, so `--json` means nothing to them.
#[derive(Subcommand)]
enum QuickstartSub {
    /// Publish an explore job's confirmed best runs as the atlas the Quick Start menu offers
    /// (JOB default: the newest explore job).
    Publish {
        job: Option<i64>,
        /// The atlas file (default: where the menu reads it, $JETCARP_ATLAS when set, else
        /// <data>/explore/atlas.sqlite); it is replaced.
        #[arg(long)]
        out: Option<PathBuf>,
        /// Entries per category, family, and tiling.
        #[arg(long, default_value_t = 12)]
        per_niche: usize,
        /// Lowest confirmed score (contrast picks are exempt).
        #[arg(long, default_value_t = 1.25)]
        min_score: f64,
        #[arg(long, default_value_t = 12_000)]
        max_entries: usize,
        /// Catalog: a .sqlite file, or a folder holding catalog.sqlite.
        #[arg(long)]
        library: Option<PathBuf>,
    },
}

fn without_json(cmd: &Cmd) -> Option<&'static str> {
    Some(match cmd {
        Cmd::Demo(_) => "demo",
        Cmd::Mcp { .. } => "mcp",
        _ => return None,
    })
}

fn main() -> Result<()> {
    let cli = Cli::parse_from(args());
    let Some(cmd) = cli.cmd else {
        return jetcarp_app::run(cli.app).map_err(|e| anyhow::anyhow!(e));
    };
    let json = cli.json;
    if json && let Some(name) = without_json(&cmd) {
        bail!(
            "`jetcarp {name}` has no JSON output; --json works with the commands that report a result (run, verify, paths, doctor, uninstall, rules, tilings, isohedral, palettes, search, jobs, pile, and the library, media, host, and app commands that print records)"
        );
    }
    // Scripts may ask for GPU toppling of large sandpiles.
    jetcarp_core::pile::set_gpu_factory(|| {
        jetcarp_gpu::pile::PileGpu::headless()
            .map(|g| Box::new(g) as Box<dyn jetcarp_core::pile::Toppler>)
    });
    let cx = platform::ctx();
    match cmd {
        Cmd::Run {
            scene,
            gens,
            png,
            scale,
            max_side,
            save,
            threads,
            expect,
            gpu,
            reseed,
            reseed_now,
        } => {
            use anyhow::Context;
            // Checked here as well, so a missing file or a bad value reads as it always has.
            std::fs::metadata(&scene).with_context(|| format!("reading {}", scene.display()))?;
            let reseed = reseed
                .map(|text| -> Result<serde_json::Value> {
                    let v: serde_json::Value =
                        serde_json::from_str(&text).context("--reseed: not JSON")?;
                    jetcarp_api::args::reseed_policies(&v)
                        .map_err(anyhow::Error::msg)
                        .context("--reseed: not a reseed policy")?;
                    Ok(v)
                })
                .transpose()?;
            let reseed_now = reseed_now
                .map(|text| -> Result<serde_json::Value> {
                    let v = serde_json::json!(text);
                    jetcarp_api::args::reseed_mode(&v)
                        .map_err(anyhow::Error::msg)
                        .context("--reseed-now: not a reseed mode")?;
                    Ok(v)
                })
                .transpose()?;
            let a = scene::RunIn {
                scene: scene.display().to_string(),
                gens,
                png: png.map(|p| p.display().to_string()),
                scale,
                max_side,
                save: save.map(|p| p.display().to_string()),
                threads,
                expect,
                gpu,
                reseed,
                reseed_now,
            };
            show(&scene::SceneRun::run(&cx, a, &Quiet)?, json)?;
        }
        Cmd::Verify { scene } => {
            std::fs::metadata(&scene)?;
            let a = scene::SceneSource {
                scene: None,
                path: Some(scene.display().to_string()),
            };
            show(&scene::SceneVerify::run(&cx, a, &Quiet)?, json)?;
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
            let png = png.display().to_string();
            if let Some(folder) = folder {
                let folder = folder.display().to_string();
                if runs.is_empty() {
                    // Checked here first, so a folder that is not a sweep reads as it always has.
                    jetcarp_api::sweeps::Manifest::load(std::path::Path::new(&folder))
                        .map_err(anyhow::Error::msg)?;
                    bail!("give the runs to show with --runs (rule, #index, or n=number)");
                }
                let a = sheet::SweepIn {
                    folder,
                    runs,
                    labels,
                    thumb,
                    cols,
                    label_scale,
                    png,
                };
                show(&sheet::SheetSweep::run(&cx, a, &Quiet)?, json)?;
                return Ok(());
            }
            let a = sheet::RulesIn {
                kind,
                from,
                count,
                cols,
                width,
                thumb,
                random_row,
                palette,
                png,
            };
            show(&sheet::SheetRules::run(&cx, a, &Quiet)?, json)?;
        }
        Cmd::Script(a) => script_cmd::script(a)?,
        Cmd::Objects(a) => research::objects(a)?,
        Cmd::Library(a) => library::run(a, json)?,
        Cmd::Search(a) => search_cmd::run(a, json)?,
        Cmd::Jobs(a) => jobs_cmd::run(a, json)?,
        Cmd::Hashlife(a) => hashlife_cmd::run(*a)?,
        Cmd::Demo(a) => demo_cmd::run(a)?,
        Cmd::Pile(a) => search_cmd::pile(*a, json)?,
        Cmd::Media(a) => media_cmd::run(a, json)?,
        Cmd::Site(a) => media_cmd::run_site(a, json)?,
        Cmd::Wallpaper(a) => wallpaper_cmd::run(&cx, *a, json)?,
        Cmd::Doctor => show(&system::SystemDoctor::run(&cx, NoInput {}, &Quiet)?, json)?,
        Cmd::Uninstall(a) => {
            let a = system::UninstallIn {
                yes: a.yes,
                settings: a.settings,
                data: a.data,
            };
            show(&system::SystemUninstall::run(&cx, a, &Quiet)?, json)?;
        }
        Cmd::Paths => show(&system::SystemPaths::run(&cx, NoInput {}, &Quiet)?, json)?,
        Cmd::Palettes => show(
            &reference::ReferencePalettes::run(&cx, NoInput {}, &Quiet)?,
            json,
        )?,
        Cmd::Tilings => show(
            &reference::ReferenceTilings::run(&cx, Default::default(), &Quiet)?,
            json,
        )?,
        Cmd::Quickstart {
            sub:
                Some(QuickstartSub::Publish {
                    job,
                    out,
                    per_niche,
                    min_score,
                    max_entries,
                    library,
                }),
            ..
        } => {
            let cx =
                platform::ctx().with_library(library.map(jetcarp_core::library::Library::file_for));
            let a = quickstart::PublishIn {
                job,
                out: out.map(|p| p.display().to_string()),
                per_niche,
                min_score,
                max_entries,
            };
            show(&quickstart::QuickstartPublish::run(&cx, a, &Quiet)?, json)?;
        }
        Cmd::Quickstart {
            sub: None,
            check,
            survey,
            survey_tilings,
            gens,
            sheet,
            filter,
            thumb,
            cols,
        } => {
            let a = quickstart::CheckIn {
                filter,
                check,
                survey,
                survey_tilings,
                gens,
                sheet: sheet.map(|p| p.display().to_string()),
                thumb,
                cols,
            };
            show(&quickstart::QuickstartCheck::run(&cx, a, &Quiet)?, json)?;
        }
        Cmd::Isohedral {
            png,
            straight,
            cols,
            thumb,
            seed,
        } => match png {
            Some(png) => {
                let a = sheet::IsohedralIn {
                    straight,
                    cols,
                    thumb,
                    seed,
                    png: png.display().to_string(),
                };
                show(&sheet::SheetIsohedral::run(&cx, a, &Quiet)?, json)?
            }
            None => show(
                &reference::ReferenceIsohedral::run(&cx, NoInput {}, &Quiet)?,
                json,
            )?,
        },
        Cmd::App(a) => app_cmd::run(a)?,
        Cmd::Host(a) => host_cmd::run(a)?,
        Cmd::Mcp { app, port, token } => {
            // The tools get this machine: the GPU, and jobs in processes of their own.
            jetcarp_mcp::tools::set_platform(std::sync::Arc::new(platform::Machine));
            jetcarp_mcp::stdio::run(app, port, token).map_err(anyhow::Error::msg)?
        }
        Cmd::Rules => show(
            &reference::ReferenceRules::run(&cx, Default::default(), &Quiet)?,
            json,
        )?,
        Cmd::Api(a) => api_cmd::run(&cx, a, json)?,
    }
    Ok(())
}
