//! "GPU simulation" toggle: steps the world with `jetcarp-gpu` on a display's own wgpu device when
//! the running CAs allow it, and on the CPU otherwise. Results are identical either way.

use jetcarp_core::World;
use jetcarp_gpu::GpuSim;
use std::time::Instant;

pub struct SimBackend {
    sim: Option<GpuSim>,
    /// Why the GPU simulator could not be created.
    unavailable: Option<String>,
    pub on: bool,
    /// What the last stepped frame ran on.
    last: Last,
}

enum Last {
    Idle,
    Gpu,
    Cpu(String),
}

/// What the simulation is running on, to show the user.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SimStatus<'a> {
    /// The toggle is off.
    Cpu,
    /// Stepping on the GPU, with the GPU's name.
    Gpu(&'a str),
    /// The toggle is on but this world runs on the CPU, for this reason.
    Fallback(&'a str),
    /// The toggle is on and nothing has been stepped yet.
    Waiting,
}

impl SimBackend {
    /// Steps on this device, after checking that its adapter can.
    pub fn from_device(
        adapter: &wgpu::Adapter,
        device: wgpu::Device,
        queue: wgpu::Queue,
    ) -> SimBackend {
        match GpuSim::from_device(adapter, device, queue) {
            Ok(s) => SimBackend::with(Some(s), None),
            Err(e) => SimBackend::unavailable(e),
        }
    }

    /// No GPU simulation, for this reason.
    pub fn unavailable(why: String) -> SimBackend {
        SimBackend::with(None, Some(why))
    }

    fn with(sim: Option<GpuSim>, unavailable: Option<String>) -> SimBackend {
        let on = sim.is_some() && std::env::var("JETCARP_GPU").is_ok_and(|v| v != "0");
        SimBackend {
            sim,
            unavailable,
            on,
            last: Last::Idle,
        }
    }

    /// Step up to `gens` generations on the GPU before `deadline`; returns how many ran there.
    /// Zero means the caller should step on the CPU (toggle off, or the world needs the CPU).
    pub fn step(&mut self, world: &mut World, gens: u64, deadline: Instant) -> u64 {
        let Some(sim) = self.sim.as_mut().filter(|_| self.on) else {
            self.last = Last::Idle;
            return 0;
        };
        match sim.run(world, gens, Some(deadline)) {
            Ok(out) => {
                self.last = match out.stopped {
                    Some(why) => Last::Cpu(why),
                    None => Last::Gpu,
                };
                out.gens
            }
            Err(why) => {
                self.last = Last::Cpu(why);
                0
            }
        }
    }

    /// Whether the GPU simulator could be created.
    pub fn available(&self) -> bool {
        self.sim.is_some()
    }

    /// Hover text for the GPU simulation toggle.
    pub fn hint(&self) -> String {
        match (&self.sim, &self.unavailable) {
            (Some(s), _) => format!(
                "Step on the GPU ({}). Results are identical to the CPU; CAs the GPU cannot run stay on the CPU.",
                s.name()
            ),
            (None, Some(e)) => format!("GPU simulation unavailable: {e}"),
            _ => String::new(),
        }
    }

    pub fn status(&self) -> SimStatus<'_> {
        match (&self.last, self.on) {
            (_, false) => SimStatus::Cpu,
            (Last::Gpu, true) => SimStatus::Gpu(self.sim.as_ref().map(|s| s.name()).unwrap_or("")),
            (Last::Cpu(why), true) => SimStatus::Fallback(why),
            (Last::Idle, true) => SimStatus::Waiting,
        }
    }
}
