//! The background host: runs live scenes with no app window, so they keep going while the app
//! opens and closes. It owns the simulation and the rotations, answers `jetcarp host` and the app over
//! the same loopback link the app uses, and on Windows and macOS sits in the tray. The `jetcarp`
//! program runs it as `jetcarp host serve` ([`run`]) and, on Windows, as the `.scr` screen saver
//! ([`run_scr`]).

/// `eprintln!` that never panics. `eprintln!` panics when stderr is a pipe that was closed, which
/// happens when a shell starts the host with its output piped and then moves on: the first line
/// the host printed (such as a lost GPU's) panicked inside the event loop and hung the host.
macro_rules! say {
    ($($arg:tt)*) => {{
        use std::io::Write as _;
        let _ = writeln!(std::io::stderr(), $($arg)*);
    }};
}

mod core;
#[cfg(target_os = "macos")]
mod desktop_mac;
#[cfg(windows)]
mod desktop_win;
mod power;
#[cfg(any(windows, test))]
mod saver_args;
mod targets;
#[cfg(any(windows, target_os = "macos"))]
mod tray;
pub mod tune;
mod window;

pub use tune::TuneArgs;

use std::time::{Duration, Instant};

/// `jetcarp host serve` options.
#[derive(clap::Args, Debug, Clone)]
pub struct HostArgs {
    /// Run with no window or tray (scripts, and automated tests).
    #[arg(long)]
    headless: bool,
    /// Canvas size with --headless, as WxH.
    #[arg(long, value_parser = parse_canvas, default_value = "1280x720")]
    canvas: [f64; 2],
    /// Run this rotation on the first screen, for this run only.
    #[arg(long)]
    rotation: Option<String>,
    /// Quit after this many seconds.
    #[arg(long)]
    seconds: Option<f64>,
    /// Run as a screen saver: full screen on every monitor until there is input.
    #[arg(long)]
    saver: bool,
    /// Started at login (what the login entry runs): show only the screens placed on the desktop,
    /// and open no windows. With none, the host waits in the tray or menu bar.
    #[arg(long)]
    login: bool,
}

fn parse_canvas(s: &str) -> Result<[f64; 2], String> {
    let bad = || format!("{s:?}: give WxH, such as 1920x1080");
    let (w, h) = s.split_once('x').ok_or_else(bad)?;
    let w: f64 = w.trim().parse().map_err(|_| bad())?;
    let h: f64 = h.trim().parse().map_err(|_| bad())?;
    if w >= 1.0 && h >= 1.0 {
        Ok([w, h])
    } else {
        Err(bad())
    }
}

/// Holds the single-instance lock for as long as the host runs. The OS drops it on any exit, so a
/// crash cannot leave the host locked out.
fn single_instance() -> Result<std::fs::File, String> {
    let path = jetcarp_mcp::link::runtime_file("host.lock").ok_or("no runtime folder")?;
    let dir = path.parent().expect("a folder");
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let f = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(&path)
        .map_err(|e| format!("{}: {e}", path.display()))?;
    match f.try_lock() {
        Ok(()) => Ok(f),
        Err(std::fs::TryLockError::WouldBlock) => {
            Err("the JETCARP background host is already running".into())
        }
        Err(std::fs::TryLockError::Error(e)) => Err(format!("{}: {e}", path.display())),
    }
}

fn run_headless(mut core: core::Core, seconds: Option<f64>) -> ! {
    let gpu = core::SharedGpu::headless();
    core.attach_gpu(gpu);
    let started = Instant::now();
    while !core.finished() && seconds.is_none_or(|s| started.elapsed().as_secs_f64() < s) {
        let frame = core.frame_time();
        let t0 = Instant::now();
        core.tick();
        core.frame_done(t0.elapsed().as_secs_f64());
        if let Some(rest) = frame.checked_sub(t0.elapsed()) {
            std::thread::sleep(rest.min(Duration::from_millis(250)));
        }
    }
    core.shutdown();
    // `core` keeps the device: it ends with the process (see `jetcarp_display::exit`).
    jetcarp_display::exit::end_process(0)
}

fn gpu_setup() {
    jetcarp_core::pile::set_gpu_factory(|| {
        jetcarp_gpu::pile::PileGpu::headless()
            .map(|g| Box::new(g) as Box<dyn jetcarp_core::pile::Toppler>)
    });
}

/// A screen saver or its preview, which nothing else controls and which may run beside a host.
fn run_saver(mode: window::Mode, seconds: Option<f64>) -> ! {
    gpu_setup();
    match core::Core::for_saver() {
        Ok(c) => window::run(c, mode, seconds),
        Err(e) => {
            say!("jetcarp-host: {e}");
            std::process::exit(1);
        }
    }
}

/// The screen saver's Settings button opens the app, where its choices are.
#[cfg(windows)]
fn open_settings() {
    let opened = jetcarp_display::autostart::app_command()
        .and_then(|c| c.spawn_detached())
        .is_ok();
    if !opened {
        desktop_win::message(
            "JETCARP's screen saver settings are in the JETCARP app: View, then Screensaver settings.",
        );
    }
}

/// Runs as `JETCARP.scr`: `args` are the ones Windows gave the screen saver (`/s`, `/p <hwnd>`,
/// `/c`), passed on by the `jetcarpw` launcher that the `.scr` is a copy of.
#[cfg(windows)]
pub fn run_scr(args: &[String]) {
    use saver_args::ScrMode;
    match saver_args::parse(args) {
        ScrMode::Run => run_saver(window::Mode::Saver, None),
        ScrMode::Preview(parent) => run_saver(window::Mode::Preview(parent), None),
        ScrMode::Configure => open_settings(),
    }
}

/// Brings an installed GNOME extension up to this version's files, so an upgrade of JETCARP
/// upgrades its extension too (GNOME Shell loads them at the next login).
fn refresh_gnome_extension() {
    if !cfg!(target_os = "linux") {
        return;
    }
    match jetcarp_display::gnome::refresh() {
        Ok(true) => say!(
            "jetcarp-host: updated the GNOME extension to this version's; GNOME loads it at the next login"
        ),
        Ok(false) => {}
        Err(e) => say!("jetcarp-host: could not update the GNOME extension: {e}"),
    }
}

/// Runs the host until it is told to quit (or `--seconds` pass), then ends the process.
pub fn run(args: HostArgs) -> ! {
    if args.saver {
        run_saver(window::Mode::Saver, args.seconds);
    }
    gpu_setup();
    let _lock = match single_instance() {
        Ok(f) => f,
        Err(e) => {
            say!("jetcarp-host: {e}");
            std::process::exit(3);
        }
    };
    if let Ok(cmd) = jetcarp_display::autostart::host_command() {
        // The GNOME menu and the screen saver start the host with this command.
        let _ = jetcarp_display::autostart::record_host_program(&cmd);
    }
    refresh_gnome_extension();
    let mut core = match core::Core::new(args.rotation, args.headless.then_some(args.canvas)) {
        Ok(c) => c,
        Err(e) => {
            say!("jetcarp-host: {e}");
            std::process::exit(1);
        }
    };
    core.login = args.login;
    if args.headless {
        run_headless(core, args.seconds)
    } else {
        window::run(core, window::Mode::Host, args.seconds)
    }
}
