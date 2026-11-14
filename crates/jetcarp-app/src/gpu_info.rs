//! What the operations ask about the GPU (`jetcarp_api::Platform`), answered from
//! `jetcarp_gpu` the same way for the command line and the app: the adapters, the GPU policy
//! and each purpose's choice (`jetcarp doctor`).

use jetcarp_api::ctx::{GpuAdapter, GpuChoice, GpuMode, GpuPolicy, GpuSelfTest};
use jetcarp_gpu::select::{self, Mode, Setting};

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

fn mode_of(m: Mode) -> GpuMode {
    match m {
        Mode::Off => GpuMode::Off,
        Mode::Auto => GpuMode::Auto,
        Mode::On => GpuMode::On,
    }
}

/// The policy and each purpose's adapter, with the self-test (run now if it has not been).
pub fn policy() -> GpuPolicy {
    let (p, choices) = select::report();
    GpuPolicy {
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
