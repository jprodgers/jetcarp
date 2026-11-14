//! "GPU simulation": steps the world with `jetcarp-gpu` on a display's own wgpu device when the
//! policy and the running CAs allow it, and on the CPU otherwise. Results are identical either
//! way.
//!
//! The policy ([`jetcarp_gpu::select::Policy`]) says off, automatic or on. Automatic steps a world
//! on the GPU once it has [`jetcarp_gpu::GPU_PACED_CELLS`] cells (a paced display) or enough
//! neighbour reads per generation (maximum speed), see [`GpuSim::worth`]. The simulator is made,
//! and the adapter checked with the self-test, the first time a world is stepped on the GPU.

use jetcarp_core::World;
use jetcarp_gpu::GpuSim;
use jetcarp_gpu::select::{self, Mode, Policy};
use std::time::Instant;

pub struct SimBackend {
    /// The display's device, until the simulator is made on it.
    device: Option<(wgpu::Adapter, wgpu::Device, wgpu::Queue)>,
    sim: Option<GpuSim>,
    /// Why the GPU simulator could not be made.
    unavailable: Option<String>,
    /// What is showing lets the GPU step (a profile's `gpu`, the host's setting).
    pub on: bool,
    /// Off, automatic (by the world's size) or on (whenever the rules allow).
    pub mode: Mode,
    /// What the last stepped frame ran on.
    last: Last,
}

enum Last {
    Idle,
    Gpu,
    /// Automatic, and the world is small enough for the CPU.
    Small,
    Cpu(String),
}

/// What the simulation is running on, to show the user.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SimStatus<'a> {
    /// GPU simulation is off (or not available).
    Cpu,
    /// Stepping on the GPU, with the GPU's name.
    Gpu(&'a str),
    /// Automatic, and this world is small enough for the CPU.
    Small,
    /// The GPU is wanted but this world runs on the CPU, for this reason.
    Fallback(&'a str),
    /// The GPU is wanted and nothing has been stepped yet.
    Waiting,
}

impl SimBackend {
    /// Steps on this device under the saved policy, once the adapter passes the self-test.
    pub fn from_device(
        adapter: &wgpu::Adapter,
        device: wgpu::Device,
        queue: wgpu::Queue,
    ) -> SimBackend {
        let policy = Policy::load();
        let info = adapter.get_info();
        // The quick checks now; the self-test when a world first needs the GPU.
        match select::requirements(
            &info,
            &adapter.limits(),
            &adapter.get_downlevel_capabilities(),
            &policy.want,
        ) {
            Ok(()) => SimBackend {
                device: Some((adapter.clone(), device, queue)),
                ..SimBackend::with(None, policy.mode)
            },
            Err(e) => SimBackend::with(Some(format!("{}: {e}", info.name)), policy.mode),
        }
    }

    /// No GPU simulation, for this reason.
    pub fn unavailable(why: String) -> SimBackend {
        SimBackend::with(Some(why), Mode::Off)
    }

    fn with(unavailable: Option<String>, mode: Mode) -> SimBackend {
        SimBackend {
            device: None,
            sim: None,
            unavailable,
            on: true,
            mode,
            last: Last::Idle,
        }
    }

    /// The simulator, made (and its adapter tested) the first time.
    fn ensure(&mut self) -> Option<&mut GpuSim> {
        if self.sim.is_none()
            && let Some((adapter, device, queue)) = self.device.take()
        {
            match GpuSim::from_device(&adapter, device, queue) {
                Ok(s) => self.sim = Some(s),
                Err(e) => self.unavailable = Some(e),
            }
        }
        self.sim.as_mut()
    }

    /// Would this world step on the GPU now? `paced`: generations at a set speed, as opposed to
    /// as fast as possible.
    pub fn wants(&self, world: &World, paced: bool) -> bool {
        self.on
            && self.available()
            && match self.mode {
                Mode::Off => false,
                Mode::On => true,
                Mode::Auto => GpuSim::worth(world, paced),
            }
    }

    /// Step up to `gens` generations on the GPU before `deadline`; returns how many ran there.
    /// Zero means the caller should step on the CPU (the GPU is off, the world is small, or it
    /// needs the CPU). `paced`: as [`wants`](Self::wants).
    pub fn step(&mut self, world: &mut World, gens: u64, deadline: Instant, paced: bool) -> u64 {
        if !self.wants(world, paced) {
            let small = self.on && self.available() && self.mode == Mode::Auto;
            self.last = if small { Last::Small } else { Last::Idle };
            return 0;
        }
        let Some(sim) = self.ensure() else {
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

    /// Whether the GPU simulator can run here (as far as is known: the self-test runs when a
    /// world first needs it).
    pub fn available(&self) -> bool {
        self.unavailable.is_none() && (self.sim.is_some() || self.device.is_some())
    }

    /// The adapter's name.
    fn name(&self) -> String {
        match (&self.sim, &self.device) {
            (Some(s), _) => s.name().to_string(),
            (None, Some((a, _, _))) => jetcarp_gpu::describe(&a.get_info()),
            _ => String::new(),
        }
    }

    /// Hover text for the GPU simulation setting.
    pub fn hint(&self) -> String {
        if let Some(e) = &self.unavailable {
            return format!("GPU simulation unavailable: {e}");
        }
        if !self.available() {
            return String::new();
        }
        format!(
            "{} on the GPU ({}). Automatic steps worlds of {} cells or more there (at maximum \
             speed, worlds with {} neighbour reads a generation); on steps every world there. \
             Results are identical to the CPU; CAs the GPU cannot run stay on the CPU.",
            match self.mode {
                Mode::Off => "Off: nothing steps",
                Mode::Auto => "Automatic: large worlds step",
                Mode::On => "On: every world steps",
            },
            self.name(),
            thousands(jetcarp_gpu::GPU_PACED_CELLS as u64),
            thousands(jetcarp_gpu::GPU_UNPACED_READS),
        )
    }

    pub fn status(&self) -> SimStatus<'_> {
        let wanted = self.on && self.mode != Mode::Off && self.available();
        match (&self.last, wanted) {
            (_, false) => SimStatus::Cpu,
            (Last::Gpu, true) => SimStatus::Gpu(self.sim.as_ref().map(|s| s.name()).unwrap_or("")),
            (Last::Small, true) => SimStatus::Small,
            (Last::Cpu(why), true) => SimStatus::Fallback(why),
            (Last::Idle, true) => SimStatus::Waiting,
        }
    }
}

/// `16,000`.
fn thousands(n: u64) -> String {
    let s = n.to_string();
    let mut out = String::new();
    for (k, c) in s.chars().enumerate() {
        if k > 0 && (s.len() - k).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use jetcarp_core::{Boundary, CaSpec, Scene, SeedSpec, TilingSpec};

    fn world(side: u32) -> World {
        let mut sc = Scene::new(TilingSpec::rect("square", side, side, Boundary::Wrap));
        sc.cas = vec![CaSpec::new("B3/S23").seed(SeedSpec::Empty)];
        World::from_scene(&sc).unwrap()
    }

    #[test]
    fn without_a_device_everything_steps_on_the_cpu() {
        let mut s = SimBackend::unavailable("no GPU".into());
        let mut w = world(200);
        assert!(!s.available() && !s.wants(&w, true));
        assert_eq!(s.step(&mut w, 5, Instant::now(), true), 0);
        assert_eq!(s.status(), SimStatus::Cpu);
        assert!(s.hint().contains("no GPU"));
    }

    #[test]
    fn numbers_read_with_commas() {
        assert_eq!(thousands(16_000), "16,000");
        assert_eq!(thousands(1_000_000), "1,000,000");
        assert_eq!(thousands(999), "999");
    }
}
