//! What this program knows about the machine that `jetcarp-api` cannot reach itself: the
//! graphics adapters and the GPU simulator (`jetcarp-gpu`), and the desktop integration (the
//! background host, start at login, the GNOME Shell extension, the screen savers). The
//! operations `system.doctor`, `system.uninstall`, and `scene.run` (with `gpu`) ask for them
//! through [`jetcarp_api::Platform`]; so do `search.start` with `gpu` (the GPU soup hunter for
//! hunts, the GPU batch stepper for explore searches) and with `detach` (a job in a process of
//! its own, `jetcarp jobs resume JOB`).

use clap::Args;
use jetcarp_api::ctx::{Desktop, GpuAdapter, GpuStepper, GpuSteps, UndoStep};
use jetcarp_api::{Ctx, Platform};
use jetcarp_core::World;
use jetcarp_core::explore::batch::Backend;
use jetcarp_core::hunt::job::SoupBatcher;
use jetcarp_display::{autostart, gnome, saver};
use jetcarp_mcp::link::host_call;
use serde_json::json;
use std::path::Path;
use std::sync::Arc;

/// The machine as the `jetcarp` program sees it.
pub struct Machine;

/// The command line's context: no limits, files anywhere, this machine.
pub fn ctx() -> Ctx {
    Ctx::new().with_platform(Arc::new(Machine))
}

struct Gpu(jetcarp_gpu::GpuSim);

impl GpuStepper for Gpu {
    fn name(&self) -> String {
        self.0.name().to_string()
    }
    fn step_mixed(&mut self, w: &mut World, gens: u64) -> Result<GpuSteps, String> {
        let m = self.0.step_mixed(w, gens)?;
        Ok(GpuSteps {
            cpu_gens: m.cpu_gens,
            fallback: m.fallback,
        })
    }
}

fn host_running() -> bool {
    host_call(&json!({"action": "status"})).is_ok()
}

impl Platform for Machine {
    fn soup_batcher(&self) -> Result<Arc<dyn SoupBatcher>, String> {
        jetcarp_gpu::hunt::GpuHunter::headless().map(|g| Arc::new(g) as Arc<dyn SoupBatcher>)
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
        jetcarp_gpu::adapter_reports()
            .into_iter()
            .map(|g| GpuAdapter {
                name: g.name,
                backend: g.backend,
                kind: g.kind,
                driver: g.driver,
                chosen: g.chosen,
                problem: g.problem,
            })
            .collect()
    }

    fn gpu_stepper(&self) -> Result<Box<dyn GpuStepper>, String> {
        jetcarp_gpu::GpuSim::headless().map(|s| Box::new(Gpu(s)) as Box<dyn GpuStepper>)
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
        if host_running() {
            steps.push(step(
                "stop the background host (it puts your own desktop picture back)",
                Box::new(|| host_call(&json!({"action": "quit"})).map(|_| ())),
            ));
        }
        if autostart::enabled() {
            steps.push(step(
                "stop the host starting at login",
                Box::new(|| autostart::set(false, &autostart::host_command()?)),
            ));
        }
        if cfg!(target_os = "linux")
            && !matches!(
                gnome::state(),
                gnome::ExtensionState::NoGnome | gnome::ExtensionState::NotInstalled
            )
        {
            steps.push(step(
                "remove the GNOME Shell extension",
                Box::new(gnome::remove),
            ));
        }
        if cfg!(windows) && jetcarp_display::winsaver::scr_path().is_ok_and(|p| p.exists()) {
            steps.push(step(
                "remove the Windows screen saver",
                Box::new(saver::remove),
            ));
        }
        if cfg!(target_os = "macos")
            && jetcarp_display::macsaver::bundle_path().is_ok_and(|p| p.exists())
        {
            steps.push(step(
                "remove the macOS screen saver",
                Box::new(saver::remove),
            ));
        }
        steps
    }
}

#[derive(Args, Debug)]
pub struct UninstallArgs {
    /// Do it. Without this, only show what would be done.
    #[arg(long)]
    pub yes: bool,
    /// Also delete JETCARP's settings folder.
    #[arg(long)]
    pub settings: bool,
    /// Also delete JETCARP's data folder (the pattern library, search databases, packs). This
    /// cannot be undone.
    #[arg(long)]
    pub data: bool,
}
