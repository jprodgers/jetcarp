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
mod broken_pipe;
mod demo_cmd;
mod hashlife_cmd;
mod host_cmd;
mod jobs_cmd;
mod launcher_cmd;
mod library;
mod machine_cmd;
mod media_cmd;
mod movers_cmd;
mod pack_cmd;
mod platform;
mod research;
mod script_cmd;
mod search_cmd;
mod setup_cmd;
mod term;
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
        #[command(flatten)]
        gpu: platform::GpuFlags,
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
    /// The app's keys: moving about it, the menus' shortcuts, and the grid's cell cursor, on
    /// Windows and Linux and on macOS, as Help → Keyboard shortcuts lists them.
    Shortcuts,
    /// Check that a saved scene still replays to its recorded hash (or a PNG JETCARP wrote, which
    /// carries its scene).
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
        /// A palette (`jetcarp palettes` lists them; names are case-sensitive) or hex colours
        /// like `#103050-#4fc3ff`.
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
    /// Packs: publish the library as a signed, content-addressed store (Parquet and JSON that
    /// any tool reads), export a version for a single download, summarise, check or import one:
    /// `key`, `rater`, `publish`, `export`, `info`, `check`, `import`.
    Pack(pack_cmd::PackArgs),
    /// Media for the catalog: per-step frames, a GIF or animated WebP, a poster, and a
    /// thumbnail for a pattern (alone), a Quick Start entry (from the seed), an ant run, or a
    /// scene; `--missing` renders everything missing or stale.
    Media(media_cmd::MediaArgs),
    /// Website export: the public catalog as JSON and media with a manifest and a viewer page,
    /// incrementally.
    Site(media_cmd::SiteArgs),
    /// List tiling families with their neighbour counts.
    Tilings,
    /// What JETCARP sees on this machine: version, the hardware and its score, folders, graphics
    /// adapters, desktop integration. Include it in bug reports.
    Doctor,
    /// Measure this machine: about a minute of JETCARP's own work (Life soups on five
    /// non-square tilings, one core, a hunt, the GPU, memory, the disk), each answer checked, for
    /// the JETCARP Hardware Score and its parts. Kept in machine.json; `--report FILE` also writes
    /// a report. It never runs by itself.
    Score(machine_cmd::ScoreArgs),
    /// This machine's limits (the largest world, the desktop background's, HashLife's, an AI's
    /// share...): each your own value if you set one, else the machine's suggestion from its
    /// memory, never above a ceiling. `set NAME VALUE`, `reset [NAME]`.
    Limits(machine_cmd::LimitsArgs),
    /// The strobe guard's switches: on for the app and for the desktop background and screen
    /// saver unless turned off in the app (Settings → Safety). `--on` turns them back on; nothing
    /// here turns them off.
    Guard {
        /// Turn the strobe guard back on (`app`, `desktop`, or `both`).
        #[arg(long, value_name = "WHERE", num_args = 0..=1, default_missing_value = "both")]
        on: Option<String>,
    },
    /// JETCARP among the system's apps, with its icon: `install` puts it in the applications menu
    /// (Linux), Applications (macOS, as JETCARP.app) or the Start menu (Windows), starting this
    /// program; `remove` takes it out; `status` says whether it is there. The installers run
    /// `install`, and `jetcarp uninstall` removes it.
    Launcher {
        #[arg(value_enum)]
        action: launcher_cmd::LauncherAction,
    },
    /// Undo what JETCARP set up outside its folders (the host and its still on the desktop,
    /// login entry, GNOME extension, screen saver, launcher), and optionally delete its settings
    /// and data. Shows the plan unless --yes. The program itself stays; this ends with the
    /// command that removes it.
    Uninstall(platform::UninstallArgs),
    /// Print the settings, data, output, and runtime folders JETCARP uses (environment overrides
    /// included), and the folder of packs that came with the program.
    Paths,
    /// First-run setup: choose a profile (desktop-art, explorer, researcher, everything,
    /// blank-slate), and JETCARP imports its packs and builds Quick Start entries of its own, as
    /// jobs. Asks in a terminal, the photosensitive seizure warning first; `--profile NAME --yes`
    /// asks nothing (`--seizure-warning` answers the warning the first time). `--status` shows
    /// where things stand.
    Setup(setup_cmd::SetupArgs),
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
    /// Run a Rhai script: worlds, runs, images, parallel rule sweeps (see the book's "Scripting (Rhai) API" reference). `--examples` lists the example scripts built in, `--example NAME` runs one.
    Script(script_cmd::ScriptArgs),
    /// Control the running JETCARP app (AI control on, or `jetcarp --control`): `status`,
    /// `screensaver on|off [--profile NAME]`, `profiles`, `display ...`, `play`, `pause`,
    /// `speed N`, `next`, `previous`,
    /// `call <tool> <json>`.
    App(app_cmd::AppArgs),
    /// The background host, which keeps scenes running with no app window: `start`, `stop`,
    /// `status`, `next`, `previous`, `pause`, `resume`, `rotation`, `caption`, `fps`, `login`.
    Host(host_cmd::HostArgs),
    /// MCP server on stdin/stdout so AI assistants (Claude Code, Claude Desktop, LM Studio) can
    /// drive JETCARP. Its tools read and write files only in JETCARP's output and library folders
    /// (and folders given with --allow).
    Mcp {
        /// Act on the world in the running JETCARP app (switch on AI control there first).
        #[arg(long)]
        app: bool,
        /// Let the tools read and write in this folder too (repeatable).
        #[arg(long, value_name = "DIR")]
        allow: Vec<PathBuf>,
        /// App port (default: the one the app recorded when AI control was switched on).
        #[arg(long)]
        port: Option<u16>,
        /// App token (default: the recorded one).
        #[arg(long)]
        token: Option<String>,
        /// Serve MCP over HTTP (Streamable HTTP) instead of standard input and output, at
        /// http://127.0.0.1:PORT/mcp on this computer only (default port 47812, or a free one when
        /// that is taken). Clients send the token it prints as an `Authorization: Bearer` header.
        /// With --json, prints the URL and token as JSON. Ctrl-C stops it.
        #[arg(long, value_name = "PORT", num_args = 0..=1)]
        http: Option<Option<u16>>,
        /// With --http: keep the token for the next start, and take the one kept. It is kept in
        /// a private file in the settings folder, survives restarts and reboots, and stays valid
        /// until --new-token. Without it, the kept token is deleted.
        #[arg(long, requires = "http")]
        keep_token: bool,
        /// With --http: make a new token even when one is kept, and keep it; the old one stops
        /// working.
        #[arg(long, requires = "http")]
        new_token: bool,
        /// With --http: let web pages from this exact origin connect, such as
        /// https://client.example or http://localhost:6274 (repeatable; no wildcards). They still
        /// need the token.
        #[arg(long, value_name = "URL", requires = "http")]
        allow_origin: Vec<String>,
    },
    /// JETCARP's operations by name, for automation and tests: `list`, `schema OP`,
    /// `call OP [JSON]` (the result as JSON).
    #[command(hide = true)]
    Api(api_cmd::ApiArgs),
    /// Quick Start examples: list the built-in ones, check that each one works, or render a
    /// contact sheet; `publish` an explore job's best runs into the Quick Start as its atlas, or
    /// `build` some: a search of so many minutes, then its atlas published.
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
/// `jetcarp shortcuts`: the app's table of keys, as text or JSON (`{"groups": [...]}`, which the
/// book's keyboard page is generated from).
fn print_shortcuts(json: bool) -> Result<()> {
    let groups = jetcarp_app::shortcuts();
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({ "groups": groups }))?
        );
        return Ok(());
    }
    let dash = |s: &str| {
        if s.is_empty() {
            "-".to_string()
        } else {
            s.to_string()
        }
    };
    for g in &groups {
        println!("{}", g.name);
        println!("  {:<24} {:<18} What it does", "Windows and Linux", "macOS");
        for s in &g.keys {
            println!("  {:<24} {:<18} {}", dash(&s.keys), dash(&s.mac), s.what);
        }
        println!();
    }
    Ok(())
}

pub(crate) fn show<T: serde::Serialize + Report>(out: &T, json: bool) -> Result<()> {
    if json {
        println!("{}", op::to_pretty(out));
    } else {
        print!("{}", out.text());
        eprint!("{}", out.notes());
    }
    out.check().map_err(anyhow::Error::new)
}

/// Progress on standard error. In a terminal: one line rewritten in place (`\r` and the erase
/// code), fitted to the terminal's width (#104), which the caller ends ([`ProgressLine::end`] or
/// [`ProgressLine::erase`]). To a file or a pipe: plain lines, with no escape codes and no `\r`
/// (#299): every step on a line of its own ([`ProgressLine::steps`], a benchmark's few steps), or
/// a count at most every five seconds ([`ProgressLine::counts`], a library check's many).
pub(crate) struct ProgressLine {
    tty: bool,
    /// Off a terminal, the shortest time between two lines (none: every report).
    every: Option<f64>,
    /// When a line was last written off a terminal; whether one shows in the terminal.
    last: std::sync::Mutex<(Option<std::time::Instant>, bool)>,
}

impl ProgressLine {
    fn new(every: Option<f64>) -> ProgressLine {
        use std::io::IsTerminal;
        ProgressLine {
            tty: std::io::stderr().is_terminal(),
            every,
            last: std::sync::Mutex::new((None, false)),
        }
    }

    /// Each report a step of its own.
    pub(crate) fn steps() -> ProgressLine {
        ProgressLine::new(None)
    }

    /// Reports that count up, as often as many times a second.
    pub(crate) fn counts() -> ProgressLine {
        ProgressLine::new(Some(5.0))
    }

    /// In a terminal, end the line shown, so it stays above what follows; elsewhere nothing.
    pub(crate) fn end(&self) {
        let mut l = self.last.lock().unwrap_or_else(|e| e.into_inner());
        if l.1 {
            eprintln!();
            l.1 = false;
        }
    }

    /// In a terminal, erase the line shown (what follows says the same); elsewhere nothing.
    pub(crate) fn erase(&self) {
        let mut l = self.last.lock().unwrap_or_else(|e| e.into_inner());
        if l.1 {
            eprint!("{}", term::clear_line());
            l.1 = false;
        }
    }
}

impl Progress for ProgressLine {
    fn report(&self, _done: u64, _total: Option<u64>, msg: &str) {
        let mut l = self.last.lock().unwrap_or_else(|e| e.into_inner());
        if self.tty {
            eprint!("{}{}", term::clear_line(), term::fit_to_stderr(msg));
            l.1 = true;
        } else if self
            .every
            .is_none_or(|s| l.0.is_none_or(|t| t.elapsed().as_secs_f64() >= s))
        {
            eprintln!("{msg}");
            l.0 = Some(std::time::Instant::now());
        }
    }
}

/// Commands that print no result of their own, so `--json` means nothing to them.
#[derive(Subcommand)]
enum QuickstartSub {
    /// Publish an explore job's confirmed best runs into the library's Quick Start as its atlas
    /// (JOB default: the newest explore job); the job's earlier atlas entries are replaced.
    Publish {
        job: Option<i64>,
        /// Entries per category, family, and tiling.
        #[arg(long, default_value_t = 12)]
        per_niche: usize,
        /// Lowest confirmed score (contrast picks are exempt).
        #[arg(long, default_value_t = 1.25)]
        min_score: f64,
        #[arg(long, default_value_t = 12_000)]
        max_entries: usize,
        /// Catalog: a .sqlite file, or a folder holding library.sqlite.
        #[arg(long)]
        library: Option<PathBuf>,
    },
    /// Build Quick Start entries of your own: an explore search of MINUTES in all (on the GPU
    /// when there is one), then its best runs published into the Quick Start, as one job.
    /// Ctrl-C stops it; `jetcarp jobs resume JOB` carries it on to its end.
    Build {
        /// Minutes to search before publishing.
        #[arg(long, default_value_t = 10.0)]
        minutes: f64,
        #[command(flatten)]
        choice: search_cmd::ChoiceFlags,
        /// Run on the CPU only.
        #[arg(long)]
        no_gpu: bool,
        /// Worker threads (default: every core but two, a third of them for the GPU's batches).
        #[arg(long)]
        threads: Option<usize>,
        /// Run in a process of its own, and return once that process has taken the job.
        #[arg(long)]
        detach: bool,
        /// Catalog: a .sqlite file, or a folder holding library.sqlite.
        #[arg(long)]
        library: Option<PathBuf>,
        /// No progress on standard error.
        #[arg(long)]
        quiet: bool,
    },
}

fn without_json(cmd: &Cmd) -> Option<&'static str> {
    Some(match cmd {
        Cmd::Demo(_) => "demo",
        Cmd::Mcp { http: None, .. } => "mcp",
        _ => return None,
    })
}

fn main() -> Result<()> {
    // A PastePattern naming a library pattern finds it in the default library.
    jetcarp_store::install();
    // Escape codes on in Windows' old console window, for the progress lines (#127).
    term::init();
    // GPU self-test results are kept in the settings folder by the program, never by code that
    // uses the GPU crate as a library (the test suites).
    jetcarp_gpu::select::keep_checks(true);
    // A closed pipe (`jetcarp --json quickstart | head`) ends the program quietly, whichever
    // command was writing.
    broken_pipe::install();
    // Windows: the Start menu entry's launcher starts this program from now on if the program it
    // started has gone (an upgrade moves it, for winget and Scoop); never in place of another
    // that is still there (#105).
    jetcarp_display::launcher::heal();
    // This machine's limits (its memory, and the person's own values in limits.json), never the
    // reference machine's that the crates' tests use.
    let (_, note) = jetcarp_core::machine::use_this_machine();
    if let Some(n) = note {
        eprintln!("note: {n}");
    }
    let r = run();
    // This program's record in JETCARP's memory pool goes with it.
    jetcarp_core::machine::pool::stop();
    if let Err(e) = &r
        && broken_pipe::is_broken(e)
    {
        broken_pipe::exit();
    }
    r
}

/// What this program is, in words, for its record in JETCARP's memory pool: `the app`, `the
/// desktop background`, `an MCP server`, `a search job`, or `jetcarp <command>`.
fn program_name(cmd: &Option<Cmd>) -> String {
    match cmd {
        None => "the app".into(),
        Some(Cmd::Host(h)) if matches!(h.cmd, host_cmd::HostCmd::Serve(_)) => {
            "the desktop background".into()
        }
        Some(Cmd::Mcp { .. }) => "an MCP server".into(),
        Some(Cmd::Jobs(j)) if j.resumes() => "a search job".into(),
        Some(_) => {
            let names: Vec<String> = Cli::command()
                .get_subcommands()
                .map(|c| c.get_name().to_string())
                .collect();
            let word = std::env::args().skip(1).find(|a| names.contains(a));
            format!("jetcarp {}", word.unwrap_or_default())
        }
    }
}

fn run() -> Result<()> {
    let cli = Cli::parse_from(args());
    // Every JETCARP program records the memory it holds, so each sees what the others leave of
    // the one pool reserved for JETCARP (`jetcarp_core::machine::pool`). All but `uninstall` and
    // `launcher`, which build nothing and are over in a moment: `uninstall` removes the records'
    // folder and the runtime folder it is in, which its own record would make again, and the
    // installers' `--uninstall` runs `launcher` after it (#135).
    if !matches!(cli.cmd, Some(Cmd::Uninstall(_) | Cmd::Launcher { .. })) {
        jetcarp_core::machine::pool::start(
            &program_name(&cli.cmd),
            jetcarp_search::jobs::explore::memory_bytes,
        );
    }
    let Some(cmd) = cli.cmd else {
        return jetcarp_app::run(cli.app).map_err(|e| anyhow::anyhow!(e));
    };
    let json = cli.json;
    if json && let Some(name) = without_json(&cmd) {
        bail!(
            "`jetcarp {name}` has no JSON output; --json works with the commands that report a result (run, verify, paths, doctor, score, limits, uninstall, rules, tilings, isohedral, palettes, search, jobs, pile, mcp --http, and the library, media, host, and app commands that print records)"
        );
    }
    // Scripts may ask for GPU toppling of large sandpiles.
    // Scripts step large worlds on the GPU as the GPU setting says.
    jetcarp_api::ctx::set_gpu_platform(std::sync::Arc::new(jetcarp_app::gpu_info::GpuOnly));
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
                gpu: gpu.asked(),
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
        Cmd::Pack(a) => pack_cmd::run(a, json)?,
        Cmd::Site(a) => media_cmd::run_site(a, json)?,
        Cmd::Wallpaper(a) => wallpaper_cmd::run(&cx, *a, json)?,
        Cmd::Doctor => show(&system::SystemDoctor::run(&cx, NoInput {}, &Quiet)?, json)?,
        Cmd::Score(a) => machine_cmd::score(&cx, a, json)?,
        Cmd::Limits(a) => machine_cmd::limits(&cx, a, json)?,
        Cmd::Guard { on } => match on {
            Some(place) => show(
                &system::SystemStrobeGuardOn::run(
                    &cx,
                    system::GuardOnIn { place: Some(place) },
                    &Quiet,
                )?,
                json,
            )?,
            None => show(
                &system::SystemStrobeGuard::run(&cx, NoInput {}, &Quiet)?,
                json,
            )?,
        },
        Cmd::Launcher { action } => launcher_cmd::run(action, json)?,
        Cmd::Uninstall(a) => {
            let a = system::UninstallIn {
                yes: a.yes,
                settings: a.settings,
                data: a.data,
                publisher_key: a.publisher_key,
            };
            show(&system::SystemUninstall::run(&cx, a, &Quiet)?, json)?;
        }
        Cmd::Paths => show(&system::SystemPaths::run(&cx, NoInput {}, &Quiet)?, json)?,
        Cmd::Setup(a) => setup_cmd::run(a, json)?,
        Cmd::Palettes => show(
            &reference::ReferencePalettes::run(&cx, NoInput {}, &Quiet)?,
            json,
        )?,
        Cmd::Shortcuts => print_shortcuts(json)?,
        Cmd::Tilings => show(
            &reference::ReferenceTilings::run(&cx, Default::default(), &Quiet)?,
            json,
        )?,
        Cmd::Quickstart {
            sub:
                Some(QuickstartSub::Publish {
                    job,
                    per_niche,
                    min_score,
                    max_entries,
                    library,
                }),
            ..
        } => {
            let cx = platform::ctx().with_library(library.map(jetcarp_store::Library::file_for));
            let a = quickstart::PublishIn {
                job,
                per_niche,
                min_score,
                max_entries,
            };
            show(&quickstart::QuickstartPublish::run(&cx, a, &Quiet)?, json)?;
        }
        Cmd::Quickstart {
            sub:
                Some(QuickstartSub::Build {
                    minutes,
                    choice,
                    no_gpu,
                    threads,
                    detach,
                    library,
                    quiet,
                }),
            ..
        } => {
            let cx = platform::ctx().with_library(library.map(jetcarp_store::Library::file_for));
            let a = quickstart::BuildIn {
                minutes: Some(minutes),
                search: choice.explore_in(),
                gpu: no_gpu.then_some(false),
                threads,
                detach,
                ..Default::default()
            };
            if !quiet {
                eprintln!(
                    "jetcarp: {}",
                    jetcarp_api::ops::search::EXPLORE_NOT_REPRODUCIBLE
                );
            }
            let p = search_cmd::CliProgress::new(quiet || json);
            let r = quickstart::QuickstartBuild::run(&cx, a, &p);
            p.finish();
            show(&r?, json)?;
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
        Cmd::Mcp {
            app,
            allow,
            port,
            token,
            http,
            keep_token,
            new_token,
            allow_origin,
        } => {
            // The tools get this machine: the GPU, and jobs in processes of their own.
            jetcarp_mcp::tools::set_platform(std::sync::Arc::new(platform::Machine));
            jetcarp_mcp::tools::allow_dirs(allow);
            match http {
                None => jetcarp_mcp::stdio::run(app, port, token),
                Some(http_port) => jetcarp_mcp::http::serve(
                    jetcarp_mcp::http::Serve {
                        port: http_port,
                        app,
                        app_port: port,
                        app_token: token,
                        json,
                        keep_token,
                        new_token,
                        allow_origins: allow_origin,
                    },
                    jetcarp_mcp::http::stop_on_ctrl_c(),
                ),
            }
            .map_err(anyhow::Error::msg)?
        }
        Cmd::Rules => show(
            &reference::ReferenceRules::run(&cx, Default::default(), &Quiet)?,
            json,
        )?,
        Cmd::Api(a) => api_cmd::run(&cx, a, json)?,
    }
    Ok(())
}
