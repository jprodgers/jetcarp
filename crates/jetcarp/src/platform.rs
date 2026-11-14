//! What this program knows about the machine that `jetcarp-api` cannot reach itself: the
//! graphics adapters and the GPU simulator (`jetcarp-gpu`), and the desktop integration (the
//! background host, start at login, the GNOME Shell extension, the screen savers). The
//! operations `system.doctor`, `system.uninstall`, and `scene.run` (with `gpu`) ask for them
//! through [`jetcarp_api::Platform`]; so do `search.start` with `gpu` (the GPU soup hunter for
//! hunts, the GPU batch stepper for explore searches) and with `detach` (a job in a process of
//! its own, `jetcarp jobs resume JOB`).

use clap::Args;
use jetcarp_api::ctx::{Desktop, GpuAdapter, GpuMode, GpuPolicy, GpuStepper, UndoStep};
use jetcarp_api::{Ctx, Platform};
use jetcarp_core::World;
use jetcarp_core::explore::batch::Backend;
use jetcarp_core::hunt::soup::SoupBatcher;
use jetcarp_display::host_settings::HostSettings;
use jetcarp_display::{autostart, desktop_picture, gnome, saver};
use jetcarp_mcp::link::{host_call, host_gone};
use serde_json::json;
use std::path::Path;
use std::sync::Arc;

/// The machine as the `jetcarp` program sees it.
pub struct Machine;

/// The command line's context: no limits, files anywhere, this machine.
pub fn ctx() -> Ctx {
    Ctx::new().with_platform(Arc::new(Machine))
}

fn host_running() -> bool {
    host_call(&json!({"action": "status"})).is_ok()
}

impl Platform for Machine {
    fn soup_batcher(&self) -> Result<Arc<dyn SoupBatcher>, String> {
        jetcarp_app::gpu_info::soup_batcher()
    }

    fn explore_backend(&self) -> Result<Box<dyn Backend>, String> {
        jetcarp_gpu::batch::BatchSim::headless().map(|g| Box::new(g) as Box<dyn Backend>)
    }

    fn can_detach(&self) -> bool {
        true
    }

    fn spawn_job(&self, catalog: &Path, job: i64, args: &[String]) -> Result<(), String> {
        let mut a = vec![
            "jobs".to_string(),
            "resume".into(),
            job.to_string(),
            "--library".into(),
            catalog.display().to_string(),
            "--quiet".into(),
        ];
        a.extend(args.iter().cloned());
        let a: Vec<&str> = a.iter().map(String::as_str).collect();
        autostart::self_command(&a)?.spawn_detached()
    }

    fn gpu_adapters(&self) -> Vec<GpuAdapter> {
        jetcarp_app::gpu_info::adapters()
    }

    fn gpu_policy(&self) -> Option<GpuPolicy> {
        Some(jetcarp_app::gpu_info::policy())
    }

    fn gpu_stepper(&self) -> Result<Box<dyn GpuStepper>, String> {
        jetcarp_app::gpu_info::stepper()
    }

    fn gpu_mode(&self) -> GpuMode {
        jetcarp_app::gpu_info::mode()
    }

    fn gpu_forced(&self) -> Option<GpuMode> {
        jetcarp_app::gpu_info::forced()
    }

    fn gpu_worth(&self, w: &World, paced: bool) -> bool {
        jetcarp_gpu::GpuSim::worth(w, paced)
    }
    fn gpu_worth_run(&self, w: &World, gens: u64) -> bool {
        jetcarp_gpu::GpuSim::worth_run(w, gens)
    }

    fn desktop(&self) -> Desktop {
        Desktop {
            host_running: host_running(),
            starts_at_login: autostart::enabled(),
            gnome_extension: cfg!(target_os = "linux").then(|| gnome::state().describe()),
            screen_saver: cfg!(any(windows, target_os = "macos")).then(saver::describe),
        }
    }

    fn undo_steps(&self) -> Vec<UndoStep> {
        let step = |what: &str, run: Box<dyn Fn() -> Result<(), String> + Send + Sync>| UndoStep {
            what: what.into(),
            run,
        };
        let mut steps = vec![];
        let running = host_running();
        if running {
            steps.push(step("stop the background host", Box::new(stop_host)));
        }
        let still_wanted = desktop_picture::wanted(&HostSettings::load().0);
        if picture_step(desktop_picture::showing(), running, still_wanted) {
            steps.push(step(
                "put your own desktop picture back (the host left a still there)",
                Box::new(|| desktop_picture::restore().map(|_| ())),
            ));
        }
        if autostart::enabled() {
            steps.push(step(
                "stop the host starting at login",
                Box::new(|| autostart::set(false, &autostart::login_command()?)),
            ));
        }
        // The files are looked for in their folder as well as asked of GNOME Shell, which
        // cannot be reached outside a GNOME session (another desktop, a text console).
        if cfg!(target_os = "linux")
            && (gnome::installed()
                || !matches!(
                    gnome::state(),
                    gnome::ExtensionState::NoGnome | gnome::ExtensionState::NotInstalled
                ))
        {
            steps.push(step(
                "remove the GNOME Shell extension",
                Box::new(gnome::remove),
            ));
        }
        if cfg!(windows) && jetcarp_display::winsaver::scr_path().is_ok_and(|p| p.exists()) {
            steps.push(step(
                "remove the Windows screen saver",
                Box::new(|| saver::remove().map(|_| ())),
            ));
        }
        if cfg!(target_os = "macos")
            && jetcarp_display::macsaver::bundle_path().is_ok_and(|p| p.exists())
        {
            steps.push(step(
                "remove the macOS screen saver",
                Box::new(|| saver::remove().map(|_| ())),
            ));
        }
        steps
    }

    fn desktop_files(&self) -> Vec<std::path::PathBuf> {
        desktop_picture::shown_files()
    }
}

/// Asks the host to quit and waits until it has gone. It may leave a still on the desktop as it
/// goes (`host still on`), and that has to be written before the user's own picture can be put
/// back, so this waits for its port to close (which comes after the still) rather than only for
/// the answer to "quit".
fn stop_host() -> Result<(), String> {
    match host_call(&json!({"action": "quit"})) {
        Err(e) if e.contains("not running") => return Ok(()),
        // The answer can be lost as the host closes its end (seen on Windows); it may still
        // have quit, which the wait below shows.
        Ok(_) | Err(_) => {}
    }
    let t0 = std::time::Instant::now();
    while t0.elapsed() < std::time::Duration::from_secs(20) {
        if host_gone() {
            return Ok(());
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    Err("the host did not quit within 20 seconds; `jetcarp host stop` or end it by hand".into())
}

/// Whether uninstalling puts the user's own desktop picture back: when the host has left a still
/// (it saved the user's picture beside the settings), or will leave one as it stops (it runs,
/// and its settings want a still).
fn picture_step(still_showing: bool, host_running: bool, still_wanted: bool) -> bool {
    still_showing || (host_running && still_wanted)
}

/// `--gpu` and `--cpu`: where a command steps, when not as JETCARP's GPU setting says
/// (`JETCARP_GPU`, else `gpu.json` in the settings folder, else automatic).
#[derive(Args, Debug, Default, Clone, Copy)]
pub struct GpuFlags {
    /// Use the GPU where the rules allow it, whatever the GPU setting says (results are
    /// identical). Default: as the GPU setting says (automatic: the GPU once the work is large
    /// enough to gain).
    #[arg(long, conflicts_with = "cpu")]
    pub gpu: bool,
    /// Use only the CPU, whatever the GPU setting says.
    #[arg(long)]
    pub cpu: bool,
}

impl GpuFlags {
    /// `Some(true)` for `--gpu`, `Some(false)` for `--cpu`, `None`: the GPU setting decides.
    pub fn asked(self) -> Option<bool> {
        match (self.gpu, self.cpu) {
            (true, _) => Some(true),
            (_, true) => Some(false),
            _ => None,
        }
    }
}

#[derive(Args, Debug)]
pub struct UninstallArgs {
    /// Do it. Without this, only show what would be done.
    #[arg(long)]
    pub yes: bool,
    /// Also delete JETCARP's settings folder (except a picture the desktop still shows).
    #[arg(long)]
    pub settings: bool,
    /// Also delete JETCARP's data folder (the pattern library, search databases, packs). This
    /// cannot be undone.
    #[arg(long)]
    pub data: bool,
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_picture_comes_back_when_a_still_is_or_will_be_up() {
        use super::picture_step;
        assert!(picture_step(true, false, false), "a still the host left");
        assert!(picture_step(true, true, false));
        assert!(
            picture_step(false, true, true),
            "the host leaves one as it stops"
        );
        assert!(!picture_step(false, true, false), "the host leaves none");
        assert!(
            !picture_step(false, false, true),
            "no host and no still: nothing to put back"
        );
    }
}
