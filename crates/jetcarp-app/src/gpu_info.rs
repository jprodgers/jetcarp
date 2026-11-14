//! What the operations ask about the GPU (`jetcarp_api::Platform`), answered from
//! `jetcarp_gpu` the same way for the command line and the app: the adapters, the GPU policy
//! and each purpose's choice (`jetcarp doctor`).

use jetcarp_api::Platform;
use jetcarp_api::ctx::{
    GpuAdapter, GpuChoice, GpuMode, GpuPolicy, GpuProgram, GpuSelfTest, GpuStepper, GpuSteps,
};
use jetcarp_core::World;
use jetcarp_core::explore::batch::Backend;
use jetcarp_core::hunt::soup::SoupBatcher;
use jetcarp_gpu::GpuSim;
use jetcarp_gpu::select::{self, Mode, Setting};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};

/// The process's world stepper, opened once: opening a GPU takes tens of milliseconds, and a
/// world left on it between steps need not be sent again.
static STEPPER: Mutex<Option<Result<GpuSim, String>>> = Mutex::new(None);

/// A turn on the shared stepper.
struct Turn(MutexGuard<'static, Option<Result<GpuSim, String>>>);

impl Turn {
    fn sim(&mut self) -> &mut GpuSim {
        match self.0.as_mut() {
            Some(Ok(s)) => s,
            _ => unreachable!("a turn is only given on a stepper"),
        }
    }
}

impl GpuStepper for Turn {
    fn name(&self) -> String {
        match self.0.as_ref() {
            Some(Ok(s)) => s.name().to_string(),
            _ => String::new(),
        }
    }
    fn step_mixed(&mut self, w: &mut World, gens: u64) -> Result<GpuSteps, String> {
        let m = self.sim().step_mixed(w, gens)?;
        Ok(GpuSteps {
            cpu_gens: m.cpu_gens,
            fallback: m.fallback,
        })
    }
}

/// The shared GPU world stepper on the compute adapter. It is never waited for: while another
/// world has it (a sweep's other worker), this one gets an error and steps on the CPU, which
/// gives the same result.
pub fn stepper() -> Result<Box<dyn GpuStepper>, String> {
    let mut g = STEPPER
        .try_lock()
        .map_err(|_| "the GPU is busy with another world".to_string())?;
    if g.is_none() {
        *g = Some(GpuSim::headless());
    }
    match g.as_ref() {
        Some(Err(e)) => Err(e.clone()),
        _ => Ok(Box::new(Turn(g))),
    }
}

/// The GPU parts of a platform, for code with none of its own (scripts: see
/// `jetcarp_api::ctx::set_gpu_platform`). No desktop, no detached jobs.
pub struct GpuOnly;

impl Platform for GpuOnly {
    fn gpu_adapters(&self) -> Vec<GpuAdapter> {
        adapters()
    }
    fn gpu_stepper(&self) -> Result<Box<dyn GpuStepper>, String> {
        stepper()
    }
    fn gpu_policy(&self) -> Option<GpuPolicy> {
        Some(policy())
    }
    fn gpu_mode(&self) -> GpuMode {
        mode()
    }
    fn gpu_forced(&self) -> Option<GpuMode> {
        forced()
    }
    fn gpu_worth(&self, w: &World, paced: bool) -> bool {
        GpuSim::worth(w, paced)
    }
    fn gpu_worth_run(&self, w: &World, gens: u64) -> bool {
        GpuSim::worth_run(w, gens)
    }
    fn soup_batcher(&self) -> Result<Arc<dyn SoupBatcher>, String> {
        soup_batcher()
    }
    fn explore_backend(&self) -> Result<Box<dyn Backend>, String> {
        jetcarp_gpu::batch::BatchSim::headless().map(|g| Box::new(g) as Box<dyn Backend>)
    }
}

/// The GPU soup hunter, made once per process.
pub fn soup_batcher() -> Result<Arc<dyn SoupBatcher>, String> {
    static HUNTER: OnceLock<Result<Arc<dyn SoupBatcher>, String>> = OnceLock::new();
    HUNTER
        .get_or_init(|| {
            jetcarp_gpu::hunt::GpuHunter::headless().map(|g| Arc::new(g) as Arc<dyn SoupBatcher>)
        })
        .clone()
}

/// Every graphics adapter, the one used for computing marked.
pub fn adapters() -> Vec<GpuAdapter> {
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

/// The policy's mode: whether to use the GPU when a command leaves it to the policy.
pub fn mode() -> GpuMode {
    mode_of(select::Policy::load().mode)
}

/// The mode `JETCARP_GPU` sets, over a command's own request.
pub fn forced() -> Option<GpuMode> {
    select::env_mode().map(mode_of)
}

/// What a program whose own choice is `own` does under the policy `p`.
pub fn program(p: &select::Policy, name: &'static str, own: select::Own) -> GpuProgram {
    let q = p.clone().with_own(own, name);
    GpuProgram {
        name: name.into(),
        choice: own.name().into(),
        mode: mode_of(q.mode),
        from: q.origin(),
    }
}

fn mode_of(m: Mode) -> GpuMode {
    match m {
        Mode::Off => GpuMode::Off,
        Mode::Auto => GpuMode::Auto,
        Mode::On => GpuMode::On,
    }
}

/// The policy and each purpose's adapter, with the self-test (run now if it has not been), and
/// what the app and the desktop background each do with their own choices.
pub fn policy() -> GpuPolicy {
    let (p, choices) = select::report();
    let programs = [
        (crate::simgpu::OWNER, crate::app::saved_gpu_choice()),
        (
            jetcarp_display::host_settings::OWNER,
            jetcarp_display::host_settings::HostSettings::load().0.gpu,
        ),
    ]
    .into_iter()
    .map(|(name, own)| program(&p, name, own))
    .collect();
    GpuPolicy {
        programs,
        mode: mode_of(p.mode),
        adapter: p.want.text(),
        source: p.source.name().into(),
        summary: p.describe(),
        setting_file: Setting::file(),
        note: p.note.clone(),
        choices: choices
            .into_iter()
            .map(|c| GpuChoice {
                purpose: c.purpose.name().into(),
                adapter: c.adapter,
                kind: c.kind,
                why: c.why,
                limits: c.limits,
                self_test: c.test.map(|t| GpuSelfTest {
                    passed: t.passed,
                    note: t.note,
                    millis: t.millis,
                    cached: t.cached,
                }),
                problem: c.problem,
            })
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use jetcarp_gpu::select::{Own, Policy};

    #[test]
    fn each_program_says_what_it_does_and_why() {
        let shared = Policy::from_setting(&Setting {
            gpu: Mode::On,
            gpu_adapter: None,
        });
        let follow = program(&shared, "the app", Own::Follow);
        assert_eq!(
            (follow.choice.as_str(), follow.mode, follow.from.as_str()),
            ("follow", GpuMode::On, "the setting in gpu.json")
        );
        let off = program(&shared, "the desktop background", Own::Off);
        assert_eq!(
            (off.mode, off.from.as_str()),
            (GpuMode::Off, "the desktop background's own setting")
        );
        let env = program(&Policy::from_env("auto"), "the app", Own::Off);
        assert_eq!(
            (env.mode, env.from.as_str()),
            (GpuMode::Auto, "JETCARP_GPU=auto")
        );
    }
}
