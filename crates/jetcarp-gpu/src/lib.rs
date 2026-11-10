//! GPU simulation for JETCARP: `wgpu` compute shaders that step Life-like, Generations, WireWorld,
//! cyclic, rock-paper-scissors, Greenberg–Hastings, hodgepodge, Spectre-glider, and priority-table
//! (`jetcarp evolve`) CAs on lattice
//! storage (square, hex, any periodic tiling) and explicit storage (aperiodic patches, via a CSR
//! neighbour buffer), with dead, wrap, and live edges.
//!
//! Results are bit-identical to the CPU steppers in `jetcarp-core`: states, ages, generation, and the
//! state hash. The engine keeps every piece of bookkeeping (see `jetcarp_core::world::external`):
//!
//! - Generations run in batches without reading the field back. Only a few bytes of per-generation
//!   metrics are read after each batch; the fields are read back once, at the end of [`GpuSim::run`].
//! - **Period detection** stays exact: the GPU compares every generation's states with the Brent
//!   snapshot (bytes, kept on the GPU) and takes new snapshots on the same schedule as the CPU, so
//!   `World::status` reports the same period at the same generation.
//! - **Scripts** keep working: a batch ends where a step's generation count or time limit ends, and
//!   the GPU stops itself after any generation where a population, activity, or repeat trigger
//!   could fire. The engine then checks the trigger exactly as on the CPU.
//! - Anything else (turmites, elementary rules, sandpiles, tables, forest fire) is reported by
//!   [`GpuSim::check`]; the caller runs those worlds on the CPU.

use jetcarp_core::rules::Rule;
use jetcarp_core::world::{ExternalPlan, GenMetrics, Halt};
use jetcarp_core::{Storage, Topology, World};
use rayon::prelude::*;
use std::sync::Arc;
use std::time::{Duration, Instant};

pub mod pile;

pub use wgpu;

pub mod hunt;

pub mod batch;

const WG: u32 = 256;
/// Generations per batch at most (per-generation metrics slots).
const MAX_BATCH: u64 = 1024;
/// Dispatches per batch at most (256-byte parameter slots).
const MAX_ENTRIES: usize = 16384;
const PARAM_STRIDE: usize = 256;
const CTRL_HEADER: usize = 8;
/// Activity halts the control shader tracks at once (one streak each).
const MAX_ACT_HALTS: usize = 4;

#[repr(C)]
#[derive(Clone, Copy, Default, bytemuck::Pod, bytemuck::Zeroable)]
struct Params {
    op: u32,
    rule: u32,
    nb: u32,
    states: u32,
    birth: u32,
    survive: u32,
    thresh: u32,
    k1: u32,
    k2: u32,
    gg: u32,
    flags: u32,
    gen_slot: u32,
    pop_mode: u32,
    pop_n: u32,
    act_mode: u32,
    act_limit: u32,
    act_for: u32,
    halt_repeat: u32,
    compare: u32,
    pad: u32,
    /// Priority rules: the packed rows (see [`priority_words`]), `k1` words.
    table: [[u32; 4]; 11],
}

/// Words a priority table may use in the parameter slot (it fills the slot's 256 bytes).
const TABLE_WORDS: usize = 44;

/// A priority rule packed for the shader, one word per row with at most two conditions:
/// bits 0-3 own state (8 = any), 4-6 next state, 7 "the row's conditions continue in the next
/// word", then two conditions of a state (4 bits, 15 = none) and a minimum count (8 bits) at bits
/// 8 and 20. A row with more conditions continues over further words (own and next unused).
/// `None` if the rule needs more than [`TABLE_WORDS`] words.
fn priority_words(r: &jetcarp_core::evolve::priority::PriorityRule) -> Option<Vec<u32>> {
    let w = pack_priority(r);
    (w.len() <= TABLE_WORDS).then_some(w)
}

/// The packed words of a priority rule, however many there are (see [`priority_words`]).
fn pack_priority(r: &jetcarp_core::evolve::priority::PriorityRule) -> Vec<u32> {
    let mut w = Vec::new();
    for row in &r.rows {
        let head = row.own.map_or(8, |o| o as u32) | ((row.next as u32 & 7) << 4);
        let mut conds = row.conds.iter();
        let mut first = true;
        loop {
            let pair = [conds.next(), conds.next()];
            let mut x = if first { head } else { 0 };
            for (k, c) in pair.iter().enumerate() {
                let (st, min) = c.map_or((15, 0), |&(s, m)| (s as u32, m as u32));
                x |= (st | (min << 4)) << (8 + 12 * k);
            }
            let more = conds.len() > 0;
            if more {
                x |= 1 << 7;
            }
            w.push(x);
            first = false;
            if !more {
                break;
            }
        }
    }
    w
}

#[derive(Clone, Copy, PartialEq)]
enum Op {
    Step,
    Pack,
    Control,
}

const FLAG_COMPARE: u32 = 1;
const FLAG_POP: u32 = 2;
const FLAG_CHANGES: u32 = 4;

/// The rule's shader parameters, or why it cannot run on the GPU.
fn rule_params(rule: &Rule) -> Result<Params, ()> {
    let mut p = Params::default();
    match *rule {
        // B0 Life-like rules run on the CPU as alternating complementary rules (counted in `ticks`).
        Rule::LifeLike { birth, .. } if birth & 1 == 1 => {
            return Err(());
        }
        Rule::LifeLike { birth, survive } => {
            (p.rule, p.birth, p.survive, p.states) = (0, birth, survive, 2)
        }
        Rule::Generations {
            birth,
            survive,
            states,
        } => (p.rule, p.birth, p.survive, p.states) = (0, birth, survive, states as u32),
        Rule::WireWorld => p.rule = 1,
        Rule::Cyclic { states, threshold } => {
            (p.rule, p.states, p.thresh) = (2, states as u32, threshold as u32)
        }
        // The CPU converts on strictly more than the threshold: at least threshold + 1 (as u8).
        Rule::Rps { states, threshold } => {
            (p.rule, p.states, p.thresh) = (2, states as u32, threshold.wrapping_add(1) as u32)
        }
        Rule::GreenbergHastings { states, threshold } => {
            (p.rule, p.states, p.thresh) = (3, states as u32, threshold as u32)
        }
        Rule::Hodgepodge { n, k1, k2, g } if k1 > 0 && k2 > 0 => {
            (p.rule, p.states, p.k1, p.k2, p.gg) = (4, n as u32, k1, k2, g)
        }
        Rule::SpectreGlider => p.rule = 5,
        Rule::Priority(ref r) => {
            let w = priority_words(r).ok_or(())?;
            (p.rule, p.states, p.k1) = (6, r.states as u32, w.len() as u32);
            for (k, x) in w.into_iter().enumerate() {
                p.table[k / 4][k % 4] = x;
            }
        }
        _ => return Err(()),
    }
    Ok(p)
}

/// Can this rule run on the GPU?
pub fn supports_rule(rule: &Rule) -> bool {
    rule_params(rule).is_ok()
}

/// How a call to [`GpuSim::run`] ended.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Outcome {
    /// Generations stepped on the GPU.
    pub gens: u64,
    /// Set when the GPU stopped early because the world can no longer run there (for example a
    /// script step switched on a CPU-only CA). The caller should run the rest on the CPU.
    pub stopped: Option<String>,
}

/// Generations stepped by [`GpuSim::step_mixed`] on each backend.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Mixed {
    pub gpu_gens: u64,
    pub cpu_gens: u64,
    /// Why the last CPU generation could not run on the GPU.
    pub fallback: Option<String>,
}

struct GpuField {
    bufs: [wgpu::Buffer; 2],
    snap: wgpu::Buffer,
    bind: [wgpu::BindGroup; 2],
    cur: usize,
    /// GPU copy equals the world's field (and its period-watch snapshot).
    valid: bool,
    /// GPU copy is newer than the world's field.
    dirty: bool,
}

struct Resident {
    topo: Arc<Topology>,
    bind0: wgpu::BindGroup,
    _bufs: [wgpu::Buffer; 3],
    gx: u32,
    fields: Vec<Option<GpuField>>,
    rev: u64,
}

/// A GPU stepper bound to one `wgpu` device. Keeps the running fields on the GPU between calls
/// while the world does not change.
pub struct GpuSim {
    device: wgpu::Device,
    queue: wgpu::Queue,
    name: String,
    max_cells: usize,
    layout1: wgpu::BindGroupLayout,
    layout0: wgpu::BindGroupLayout,
    step: wgpu::ComputePipeline,
    pack: wgpu::ComputePipeline,
    control: wgpu::ComputePipeline,
    /// Applies reseed edits to a resident field (`edit.wgsl`).
    edit: wgpu::ComputePipeline,
    layout_e: wgpu::BindGroupLayout,
    params: wgpu::Buffer,
    ctrl: wgpu::Buffer,
    ctrl_read: wgpu::Buffer,
    staging: Option<wgpu::Buffer>,
    res: Option<Resident>,
    /// Generations per batch, adapted to keep each batch near the target time.
    batch_hint: u64,
}

impl GpuSim {
    /// Open the default high-performance adapter without a window (CLI, tests, benchmarks).
    pub fn headless() -> Result<GpuSim, String> {
        let instance =
            wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env());
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            ..Default::default()
        }))
        .map_err(|e| format!("no GPU adapter: {e}"))?;
        let info = adapter.get_info();
        if info.device_type == wgpu::DeviceType::Cpu {
            return Err(format!(
                "only a software adapter is available ({})",
                info.name
            ));
        }
        check_adapter(&adapter)?;
        let mut limits = wgpu::Limits::default();
        let al = adapter.limits();
        limits.max_storage_buffer_binding_size = al.max_storage_buffer_binding_size;
        limits.max_buffer_size = al.max_buffer_size;
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("jetcarp-gpu"),
            required_features: wgpu::Features::empty(),
            required_limits: limits,
            experimental_features: Default::default(),
            memory_hints: wgpu::MemoryHints::Performance,
            trace: Default::default(),
        }))
        .map_err(|e| format!("GPU device: {e}"))?;
        Ok(GpuSim::new(device, queue, describe(&info)))
    }

    /// Use an existing device (for example the app's `egui_wgpu::RenderState` device), after
    /// checking that its adapter runs compute shaders.
    pub fn from_device(
        adapter: &wgpu::Adapter,
        device: wgpu::Device,
        queue: wgpu::Queue,
    ) -> Result<GpuSim, String> {
        check_adapter(adapter)?;
        Ok(GpuSim::new(device, queue, describe(&adapter.get_info())))
    }

    /// Use an existing device that is known to run compute shaders.
    pub fn new(device: wgpu::Device, queue: wgpu::Queue, name: String) -> GpuSim {
        let lim = device.limits();
        let max_bytes = lim.max_storage_buffer_binding_size.min(lim.max_buffer_size);
        let max_cells = (max_bytes / 4).min(u32::MAX as u64 / 2) as usize;
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("jetcarp-gpu sim"),
            source: wgpu::ShaderSource::Wgsl(include_str!("sim.wgsl").into()),
        });
        let storage = |binding, read_only| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::COMPUTE,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Storage { read_only },
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        };
        let layout0 = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("jetcarp-gpu topology"),
            entries: &[
                storage(0, true),
                storage(1, true),
                storage(2, true),
                storage(3, false),
                wgpu::BindGroupLayoutEntry {
                    binding: 4,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: true,
                        min_binding_size: wgpu::BufferSize::new(
                            std::mem::size_of::<Params>() as u64
                        ),
                    },
                    count: None,
                },
            ],
        });
        let layout1 = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("jetcarp-gpu field"),
            entries: &[storage(0, true), storage(1, false), storage(2, false)],
        });
        let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("jetcarp-gpu"),
            bind_group_layouts: &[Some(&layout0), Some(&layout1)],
            immediate_size: 0,
        });
        let pipe = |entry: &str| {
            device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some(entry),
                layout: Some(&pl),
                module: &module,
                entry_point: Some(entry),
                compilation_options: Default::default(),
                cache: None,
            })
        };
        let (step, pack, control) = (pipe("step"), pipe("pack"), pipe("control"));
        let layout_e = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("jetcarp-gpu edits"),
            entries: &[storage(0, true)],
        });
        let pl_e = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("jetcarp-gpu edit"),
            bind_group_layouts: &[Some(&layout_e), Some(&layout1)],
            immediate_size: 0,
        });
        let edit_module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("jetcarp-gpu edit"),
            source: wgpu::ShaderSource::Wgsl(include_str!("edit.wgsl").into()),
        });
        let edit = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("edit"),
            layout: Some(&pl_e),
            module: &edit_module,
            entry_point: Some("edit"),
            compilation_options: Default::default(),
            cache: None,
        });
        let params = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("jetcarp-gpu params"),
            size: (MAX_ENTRIES * PARAM_STRIDE) as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let ctrl_size = ((CTRL_HEADER + 4 * MAX_BATCH as usize) * 4) as u64;
        let ctrl = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("jetcarp-gpu control"),
            size: ctrl_size,
            usage: wgpu::BufferUsages::STORAGE
                | wgpu::BufferUsages::COPY_DST
                | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });
        let ctrl_read = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("jetcarp-gpu control readback"),
            size: ctrl_size,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        GpuSim {
            device,
            queue,
            name,
            max_cells,
            layout0,
            layout1,
            step,
            pack,
            control,
            edit,
            layout_e,
            params,
            ctrl,
            ctrl_read,
            staging: None,
            res: None,
            batch_hint: 4,
        }
    }

    /// Adapter name and backend, for status lines.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Can the world's next generation run on the GPU? `Err` says why not.
    pub fn check(&self, world: &World) -> Result<(), String> {
        self.check_plan(world, &world.external_plan())
    }

    fn check_plan(&self, world: &World, plan: &ExternalPlan) -> Result<(), String> {
        let n = world.topo.len();
        if n > self.max_cells {
            return Err(format!(
                "{n} cells exceed this GPU's buffer limit ({} cells)",
                self.max_cells
            ));
        }
        if let Storage::Explicit(e) = &world.topo.storage {
            // Both neighbour lists go to the GPU in one buffer.
            let nbs: usize = e.csr.iter().map(|c| c.nb.len()).sum();
            if nbs.max(2 * n + 2) > self.max_cells {
                return Err(format!(
                    "the patch's {nbs} neighbour entries exceed this GPU's buffer limit ({} entries)",
                    self.max_cells
                ));
            }
        }
        let mut any = false;
        let mut subs = vec![0usize; world.fields.len()];
        for ca in world.cas.iter().filter(|c| c.enabled) {
            any = true;
            if rule_params(&ca.rule).is_err() {
                return Err(format!(
                    "{} ({}) runs only on the CPU",
                    ca.name,
                    ca.notation()
                ));
            }
            subs[ca.field] += ca.steps_per_gen as usize;
        }
        if !any {
            return Err("no CA is running".into());
        }
        let per_gen: usize = subs.iter().sum::<usize>() + plan.running_fields.len() + 1;
        if per_gen > MAX_ENTRIES {
            return Err("too many steps per generation for the GPU".into());
        }
        if plan.halts.iter().any(|h| h.needs_changes()) && subs.iter().any(|&s| s > 1) {
            return Err(
                "activity triggers with several steps per generation on one field run on the CPU"
                    .into(),
            );
        }
        if plan.count_population && subs.iter().any(|&s| s > 1) {
            return Err(
                "settled reseed policies with several steps per generation on one field run on the CPU"
                    .into(),
            );
        }
        if act_halts(&plan.halts).len() > MAX_ACT_HALTS {
            return Err(format!(
                "more than {MAX_ACT_HALTS} activity triggers at once run on the CPU"
            ));
        }
        Ok(())
    }

    /// Step `gens` generations on the GPU (or until `deadline`, after at least one batch).
    /// The world is fully up to date when this returns. `Err` means nothing ran: run on the CPU.
    pub fn run(
        &mut self,
        world: &mut World,
        gens: u64,
        deadline: Option<Instant>,
    ) -> Result<Outcome, String> {
        self.check(world)?;
        self.bind_topology(world);
        let mut done = 0u64;
        let mut stopped = None;
        while done < gens {
            if deadline.is_some_and(|d| Instant::now() >= d) && done > 0 {
                break;
            }
            let plan = world.external_plan();
            if let Err(e) = self.check_plan(world, &plan) {
                stopped = Some(e);
                break;
            }
            self.prepare_fields(world, &plan);
            let mut len = (gens - done).min(MAX_BATCH).min(self.batch_hint);
            if let Some(c) = plan.max_gens {
                len = len.min(c);
            }
            let t0 = Instant::now();
            let k = self.batch(world, &plan, len)?;
            done += k;
            // Aim for ~40 ms batches, or what is left before the deadline.
            let per_gen = t0.elapsed().as_secs_f64() / k.max(1) as f64;
            let target = match deadline {
                Some(d) => d
                    .saturating_duration_since(Instant::now())
                    .min(Duration::from_millis(12)),
                None => Duration::from_millis(40),
            };
            let want = (target.as_secs_f64() / per_gen.max(1e-7)) as u64;
            self.batch_hint = want.clamp(1, MAX_BATCH).min(self.batch_hint * 4);
        }
        self.read_back(world, |_| true);
        let res = self.res.as_mut().unwrap();
        res.rev = world.revision();
        Ok(Outcome {
            gens: done,
            stopped,
        })
    }

    /// Step `gens` generations, on the GPU whenever the world allows it and on the CPU otherwise
    /// (for example while a script step runs a CPU-only CA). The result is the same as
    /// `world.apply(Command::Step { gens })`.
    pub fn step_mixed(&mut self, world: &mut World, gens: u64) -> Result<Mixed, String> {
        let mut m = Mixed::default();
        while m.gpu_gens + m.cpu_gens < gens {
            let left = gens - m.gpu_gens - m.cpu_gens;
            let why = match self.run(world, left, None) {
                Ok(out) => {
                    m.gpu_gens += out.gens;
                    out.stopped
                }
                Err(e) => Some(e),
            };
            if m.gpu_gens + m.cpu_gens < gens {
                world.apply(jetcarp_core::Command::Step { gens: 1 })?;
                m.cpu_gens += 1;
                if why.is_some() {
                    m.fallback = why;
                }
            }
        }
        Ok(m)
    }

    fn bind_topology(&mut self, world: &World) {
        if let Some(r) = &mut self.res
            && Arc::ptr_eq(&r.topo, &world.topo)
        {
            if r.rev != world.revision() {
                for f in r.fields.iter_mut().flatten() {
                    f.valid = false;
                    f.dirty = false;
                }
            }
            return;
        }
        let topo = world.topo.clone();
        let n = topo.len();
        let groups = (n as u32).div_ceil(WG).max(1);
        let gx = groups.min(32768);
        let mut t: Vec<i32> = vec![0; 16];
        let (mut off, mut nbs) = (vec![0u32], vec![0u32]);
        t[0] = n as i32;
        t[8] = gx as i32;
        match &topo.storage {
            Storage::Lattice(g) => {
                t[1] = g.w as i32;
                t[2] = g.h as i32;
                t[3] = g.ntypes as i32;
                t[4] = g.wrap as i32;
                t[5] = g.wrap_shift;
                t[7] = topo.live_edges() as i32;
                t[9] = t.len() as i32;
                t.extend_from_slice(&g.row_start);
                t[10] = t.len() as i32;
                let mut start = 0;
                for nb in 0..2 {
                    for k in 0..g.ntypes as usize {
                        let c = g.offsets[nb][k].len() as i32;
                        t.extend_from_slice(&[start, c]);
                        start += c;
                    }
                }
                t[11] = t.len() as i32;
                for nb in 0..2 {
                    for k in 0..g.ntypes as usize {
                        for o in &g.offsets[nb][k] {
                            t.extend_from_slice(&[o.di, o.dj, o.k as i32]);
                        }
                    }
                }
            }
            Storage::Explicit(e) => {
                t[6] = 1;
                off.clear();
                nbs.clear();
                for csr in &e.csr {
                    let base = nbs.len() as u32;
                    off.extend(csr.off.iter().map(|&o| o + base));
                    nbs.extend_from_slice(&csr.nb);
                }
                if nbs.is_empty() {
                    nbs.push(0);
                }
            }
        }
        use wgpu::util::DeviceExt;
        let mk = |label, data: &[u8]| {
            self.device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some(label),
                    contents: data,
                    usage: wgpu::BufferUsages::STORAGE,
                })
        };
        let bufs = [
            mk("jetcarp-gpu topo", bytemuck::cast_slice(&t)),
            mk("jetcarp-gpu csr off", bytemuck::cast_slice(&off)),
            mk("jetcarp-gpu csr nb", bytemuck::cast_slice(&nbs)),
        ];
        let bind0 = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("jetcarp-gpu topology"),
            layout: &self.layout0,
            entries: &[
                entry(0, &bufs[0]),
                entry(1, &bufs[1]),
                entry(2, &bufs[2]),
                entry(3, &self.ctrl),
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                        buffer: &self.params,
                        offset: 0,
                        size: wgpu::BufferSize::new(std::mem::size_of::<Params>() as u64),
                    }),
                },
            ],
        });
        self.res = Some(Resident {
            topo,
            bind0,
            _bufs: bufs,
            gx,
            fields: Vec::new(),
            rev: world.revision(),
        });
    }

    /// Make every running field resident and current; read back fields that stopped running.
    fn prepare_fields(&mut self, world: &mut World, plan: &ExternalPlan) {
        let running = plan.running_fields.clone();
        self.read_back(world, |f| !running.contains(&f));
        let n = world.topo.len();
        let res = self.res.as_mut().unwrap();
        if res.fields.len() < world.fields.len() {
            res.fields.resize_with(world.fields.len(), || None);
        }
        for &fi in &running {
            if res.fields[fi].is_none() {
                let size = (n.max(1) * 4) as u64;
                let mkb = |label| {
                    self.device.create_buffer(&wgpu::BufferDescriptor {
                        label: Some(label),
                        size,
                        usage: wgpu::BufferUsages::STORAGE
                            | wgpu::BufferUsages::COPY_DST
                            | wgpu::BufferUsages::COPY_SRC,
                        mapped_at_creation: false,
                    })
                };
                let bufs = [mkb("jetcarp-gpu field a"), mkb("jetcarp-gpu field b")];
                let snap = self.device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("jetcarp-gpu snapshot"),
                    size: (n.div_ceil(4).max(1) * 4) as u64,
                    usage: wgpu::BufferUsages::STORAGE
                        | wgpu::BufferUsages::COPY_DST
                        | wgpu::BufferUsages::COPY_SRC,
                    mapped_at_creation: false,
                });
                let bg = |a: usize| {
                    self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                        label: Some("jetcarp-gpu field"),
                        layout: &self.layout1,
                        entries: &[entry(0, &bufs[a]), entry(1, &bufs[1 - a]), entry(2, &snap)],
                    })
                };
                let bind = [bg(0), bg(1)];
                res.fields[fi] = Some(GpuField {
                    bufs,
                    snap,
                    bind,
                    cur: 0,
                    valid: false,
                    dirty: false,
                });
            }
            let gf = res.fields[fi].as_mut().unwrap();
            if gf.valid {
                continue;
            }
            let f = &world.fields[fi];
            let packed: Vec<u32> = f
                .state
                .par_iter()
                .zip(f.age.par_iter())
                .map(|(&s, &a)| s as u32 | (a as u32) << 16)
                .collect();
            gf.cur = 0;
            self.queue
                .write_buffer(&gf.bufs[0], 0, bytemuck::cast_slice(&packed));
            if let Some(s) = world.watch_snapshot_states(fi) {
                let mut bytes = s.to_vec();
                bytes.resize(n.div_ceil(4).max(1) * 4, 0);
                self.queue.write_buffer(&gf.snap, 0, &bytes);
            }
            gf.valid = true;
            gf.dirty = false;
        }
    }

    /// Run one batch of at most `len` generations; returns the generations done.
    fn batch(&mut self, world: &mut World, plan: &ExternalPlan, len: u64) -> Result<u64, String> {
        let enabled: Vec<_> = world.cas.iter().filter(|c| c.enabled).collect();
        let nf = world.fields.len();
        // Last dispatch touching each field in a generation gets the metric flags.
        let mut last = vec![usize::MAX; nf];
        let mut subs = vec![0u64; nf];
        let mut order = Vec::new();
        for (ci, ca) in enabled.iter().enumerate() {
            for _ in 0..ca.steps_per_gen {
                last[ca.field] = order.len();
                order.push(ci);
            }
            subs[ca.field] += ca.steps_per_gen as u64;
        }
        let per_gen = order.len() + plan.running_fields.len() + 1;
        let len = len.min((MAX_ENTRIES / per_gen) as u64).max(1);
        let sched = plan
            .watch
            .map(|w| w.schedule(len))
            .unwrap_or_else(|| vec![(false, false); len as usize]);
        // Population halts combine exactly: stop when population <= the largest "at most" bound
        // or >= the smallest "at least" bound. A `done` trigger (`Halt::Settled`) stops after a
        // generation that changed nothing or left the running fields empty; the engine then
        // decides from the same counts.
        let (mut pop_mode, mut pop_max, mut pop_min) = (0u32, 0u64, u64::MAX);
        for h in &plan.halts {
            match *h {
                Halt::PopulationAtMost(n) => (pop_mode, pop_max) = (pop_mode | 1, pop_max.max(n)),
                Halt::Settled => pop_mode |= 1,
                Halt::PopulationAtLeast(n) => (pop_mode, pop_min) = (pop_mode | 2, pop_min.min(n)),
                _ => {}
            }
        }
        let acts = act_halts(&plan.halts);
        let counted = !acts.is_empty() || (plan.count_changes && subs.iter().all(|&s| s <= 1));
        let halt_repeat = plan.halt_on_repeat && plan.watch.is_some();
        let control = pop_mode != 0 || !acts.is_empty() || halt_repeat;
        let mut act_table = [[0u32; 4]; 11];
        for (k, a) in acts.iter().enumerate() {
            act_table[k] = [
                a.0,
                a.1.min(u32::MAX as u64) as u32,
                a.2.min(u32::MAX as u64) as u32,
                0,
            ];
        }
        let c32 = |v: u64| v.min(u32::MAX as u64) as u32;
        let rule: Vec<Params> = enabled
            .iter()
            .map(|c| {
                let mut p = rule_params(&c.rule).unwrap();
                p.nb = c.nb as u32;
                p
            })
            .collect();

        // Parameters and the dispatch list.
        let res = self.res.as_ref().unwrap();
        let mut bytes = vec![0u8; 0];
        let mut list: Vec<(Op, Option<(usize, usize)>)> = Vec::new();
        let mut cur: Vec<usize> = (0..nf)
            .map(|f| {
                res.fields
                    .get(f)
                    .and_then(|x| x.as_ref())
                    .map_or(0, |g| g.cur)
            })
            .collect();
        let push = |p: Params, bytes: &mut Vec<u8>| {
            let at = bytes.len();
            bytes.resize(at + PARAM_STRIDE, 0);
            bytes[at..at + std::mem::size_of::<Params>()].copy_from_slice(bytemuck::bytes_of(&p));
        };
        for (g, &(compare, take)) in sched.iter().enumerate() {
            for (d, &ci) in order.iter().enumerate() {
                let f = enabled[ci].field;
                let mut p = rule[ci];
                p.op = 0;
                p.gen_slot = g as u32;
                if last[f] == d {
                    p.flags = if compare { FLAG_COMPARE } else { 0 }
                        | if pop_mode != 0 || plan.count_population {
                            FLAG_POP
                        } else {
                            0
                        }
                        | if counted { FLAG_CHANGES } else { 0 };
                }
                push(p, &mut bytes);
                list.push((Op::Step, Some((f, cur[f]))));
                cur[f] ^= 1;
            }
            if take {
                for &f in &plan.running_fields {
                    push(
                        Params {
                            op: 1,
                            ..Default::default()
                        },
                        &mut bytes,
                    );
                    list.push((Op::Pack, Some((f, cur[f]))));
                }
            }
            if control {
                push(
                    Params {
                        op: 2,
                        gen_slot: g as u32,
                        pop_mode,
                        pop_n: c32(pop_max),
                        thresh: c32(pop_min),
                        act_mode: acts.len() as u32,
                        halt_repeat: halt_repeat as u32,
                        compare: compare as u32,
                        table: act_table,
                        ..Default::default()
                    },
                    &mut bytes,
                );
                list.push((Op::Control, None));
            }
        }
        self.queue.write_buffer(&self.params, 0, &bytes);
        let ctrl_words = CTRL_HEADER + 4 * len as usize;
        let mut init = vec![0u32; ctrl_words];
        for (k, a) in acts.iter().enumerate() {
            init[2 + k] = c32(a.3);
        }
        self.queue
            .write_buffer(&self.ctrl, 0, bytemuck::cast_slice(&init));

        let n = world.topo.len() as u32;
        let groups = |items: u32| {
            let gtotal = items.div_ceil(WG).max(1);
            (gtotal.min(res.gx), gtotal.div_ceil(res.gx))
        };
        let (sx, sy) = groups(n);
        let (px, py) = groups(n.div_ceil(4));
        let mut enc = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("jetcarp-gpu"),
            });
        {
            let mut pass = enc.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("jetcarp-gpu batch"),
                timestamp_writes: None,
            });
            let mut bound: Option<Op> = None;
            for (k, (op, field)) in list.iter().enumerate() {
                if bound != Some(*op) {
                    pass.set_pipeline(match op {
                        Op::Step => &self.step,
                        Op::Pack => &self.pack,
                        Op::Control => &self.control,
                    });
                    bound = Some(*op);
                }
                pass.set_bind_group(0, &res.bind0, &[(k * PARAM_STRIDE) as u32]);
                if let Some((f, parity)) = field {
                    let gf = res.fields[*f].as_ref().unwrap();
                    pass.set_bind_group(1, &gf.bind[*parity], &[]);
                }
                match op {
                    Op::Step => pass.dispatch_workgroups(sx, sy, 1),
                    Op::Pack => pass.dispatch_workgroups(px, py, 1),
                    Op::Control => pass.dispatch_workgroups(1, 1, 1),
                }
            }
        }
        let ctrl_bytes = (ctrl_words * 4) as u64;
        enc.copy_buffer_to_buffer(&self.ctrl, 0, &self.ctrl_read, 0, ctrl_bytes);
        self.queue.submit([enc.finish()]);
        let words: Vec<u32> = map_read(&self.device, &self.ctrl_read, ctrl_bytes)?;

        let k = if words[0] != 0 { words[1] as u64 } else { len };
        let metrics: Vec<GenMetrics> = (0..k as usize)
            .map(|g| {
                let m = &words[CTRL_HEADER + 4 * g..CTRL_HEADER + 4 * g + 4];
                GenMetrics {
                    population: m[0] as u64,
                    changed: m[1] as u64,
                    repeat: sched[g].0 && m[2] == 0,
                    counted,
                }
            })
            .collect();
        let res = self.res.as_mut().unwrap();
        for (f, &sf) in subs.iter().enumerate() {
            if sf > 0 {
                let gf = res.fields[f].as_mut().unwrap();
                gf.cur ^= ((k * sf) & 1) as usize;
                gf.dirty = true;
            }
        }
        let running = plan.running_fields.clone();
        let this = &*self;
        let recorded =
            world.finish_external(&metrics, || this.read_snapshots(&running, n as usize));
        if recorded != k {
            // The engine and the GPU disagree about where the batch ended; drop GPU state.
            self.res = None;
            return Err(format!(
                "GPU batch ended after {k} generations but the script moved on or a reseed fired after {recorded}"
            ));
        }
        let pending = world.take_pending_reseed();
        if !pending.is_empty() {
            self.apply_edits(world, pending);
        }
        Ok(k)
    }

    /// Apply reseed edits planned at the end of a batch: on the GPU for fields whose newest copy
    /// is resident, on the world's copy otherwise (and that field's GPU copy is dropped).
    fn apply_edits(&mut self, world: &mut World, pending: Vec<(usize, jetcarp_core::reseed::Edits)>) {
        use wgpu::util::DeviceExt;
        let res = self.res.as_mut().unwrap();
        for (f, edits) in pending {
            match res.fields.get_mut(f).and_then(|g| g.as_mut()) {
                Some(gf) if gf.dirty => {
                    if edits.is_empty() {
                        continue;
                    }
                    let count = edits.len() as u32;
                    let groups = count.div_ceil(WG).max(1);
                    let gx = groups.min(32768);
                    let gy = groups.div_ceil(gx);
                    let mut words = Vec::with_capacity(edits.len() + 2);
                    words.extend_from_slice(&[count, gx]);
                    words.extend_from_slice(&edits.words);
                    let buf = self
                        .device
                        .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                            label: Some("jetcarp-gpu edits"),
                            contents: bytemuck::cast_slice(&words),
                            usage: wgpu::BufferUsages::STORAGE,
                        });
                    let bg = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                        label: Some("jetcarp-gpu edits"),
                        layout: &self.layout_e,
                        entries: &[entry(0, &buf)],
                    });
                    let mut enc = self.device.create_command_encoder(&Default::default());
                    {
                        let mut pass = enc.begin_compute_pass(&wgpu::ComputePassDescriptor {
                            label: Some("jetcarp-gpu edit"),
                            timestamp_writes: None,
                        });
                        pass.set_pipeline(&self.edit);
                        pass.set_bind_group(0, &bg, &[]);
                        // bind[1 - cur] writes into bufs[cur], the current generation.
                        pass.set_bind_group(1, &gf.bind[1 - gf.cur], &[]);
                        pass.dispatch_workgroups(gx, gy, 1);
                    }
                    self.queue.submit([enc.finish()]);
                }
                other => {
                    if let Some(gf) = other {
                        gf.valid = false;
                    }
                    if let Some(field) = world.fields.get_mut(f) {
                        edits.apply(field);
                    }
                }
            }
        }
    }

    fn read_snapshots(&self, fields: &[usize], n: usize) -> Vec<(usize, Vec<u8>)> {
        let res = self.res.as_ref().unwrap();
        fields
            .iter()
            .map(|&f| {
                let gf = res.fields[f].as_ref().unwrap();
                let bytes = (n.div_ceil(4).max(1) * 4) as u64;
                let v: Vec<u8> = self.copy_out(&gf.snap, bytes).unwrap_or_default();
                (f, v)
            })
            .collect()
    }

    fn copy_out<T: bytemuck::Pod>(&self, buf: &wgpu::Buffer, bytes: u64) -> Result<Vec<T>, String> {
        let staging = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("jetcarp-gpu readback"),
            size: bytes,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let mut enc = self.device.create_command_encoder(&Default::default());
        enc.copy_buffer_to_buffer(buf, 0, &staging, 0, bytes);
        self.queue.submit([enc.finish()]);
        map_read(&self.device, &staging, bytes)
    }

    /// Copy dirty fields that `pick` selects back into the world.
    fn read_back(&mut self, world: &mut World, pick: impl Fn(usize) -> bool) {
        let n = world.topo.len();
        let bytes = (n.max(1) * 4) as u64;
        let Some(res) = self.res.as_mut() else { return };
        for (fi, gf) in res.fields.iter_mut().enumerate() {
            let Some(gf) = gf.as_mut() else { continue };
            if !gf.dirty || !pick(fi) {
                continue;
            }
            if self.staging.as_ref().is_none_or(|s| s.size() < bytes) {
                self.staging = Some(self.device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("jetcarp-gpu field readback"),
                    size: bytes,
                    usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                }));
            }
            let staging = self.staging.as_ref().unwrap();
            let mut enc = self.device.create_command_encoder(&Default::default());
            enc.copy_buffer_to_buffer(&gf.bufs[gf.cur], 0, staging, 0, bytes);
            self.queue.submit([enc.finish()]);
            let slice = staging.slice(0..bytes);
            slice.map_async(wgpu::MapMode::Read, |_| {});
            let _ = self.device.poll(wgpu::PollType::wait_indefinitely());
            {
                let view = slice.get_mapped_range().expect("mapped readback");
                let words: &[u32] = bytemuck::cast_slice(&view);
                let f = &mut world.fields[fi];
                f.state
                    .par_iter_mut()
                    .zip(f.age.par_iter_mut())
                    .zip(words[..n].par_iter())
                    .for_each(|((s, a), &x)| {
                        *s = (x & 0xff) as u8;
                        *a = (x >> 16) as u16;
                    });
            }
            staging.unmap();
            gf.dirty = false;
        }
    }
}

/// Activity halts as (1 below / 2 at least, limit, generations in a row, streak so far).
fn act_halts(halts: &[Halt]) -> Vec<(u32, u64, u64, u64)> {
    halts
        .iter()
        .filter_map(|h| match *h {
            Halt::ChangedBelow {
                limit,
                for_gens,
                streak,
            } => Some((1, limit, for_gens, streak)),
            Halt::ChangedAtLeast {
                limit,
                for_gens,
                streak,
            } => Some((2, limit, for_gens, streak)),
            Halt::Settled => Some((1, 1, 1, 0)),
            _ => None,
        })
        .collect()
}

fn check_adapter(adapter: &wgpu::Adapter) -> Result<(), String> {
    let caps = adapter.get_downlevel_capabilities();
    if !caps.flags.contains(wgpu::DownlevelFlags::COMPUTE_SHADERS) {
        return Err(format!(
            "{} has no compute shaders",
            adapter.get_info().name
        ));
    }
    Ok(())
}

fn entry(binding: u32, buf: &wgpu::Buffer) -> wgpu::BindGroupEntry<'_> {
    wgpu::BindGroupEntry {
        binding,
        resource: buf.as_entire_binding(),
    }
}

fn map_read<T: bytemuck::Pod>(
    device: &wgpu::Device,
    buf: &wgpu::Buffer,
    bytes: u64,
) -> Result<Vec<T>, String> {
    let slice = buf.slice(0..bytes);
    let (tx, rx) = std::sync::mpsc::channel();
    slice.map_async(wgpu::MapMode::Read, move |r| {
        let _ = tx.send(r);
    });
    device
        .poll(wgpu::PollType::wait_indefinitely())
        .map_err(|e| format!("GPU poll: {e}"))?;
    rx.recv()
        .map_err(|e| e.to_string())?
        .map_err(|e| format!("GPU readback: {e}"))?;
    let out = {
        let view = slice.get_mapped_range().map_err(|e| format!("{e:?}"))?;
        bytemuck::cast_slice::<u8, T>(&view).to_vec()
    };
    buf.unmap();
    Ok(out)
}

/// "AMD Radeon 8060S Graphics (RADV STRIX_HALO), Vulkan".
pub fn describe(info: &wgpu::AdapterInfo) -> String {
    let drv = if info.driver.is_empty() {
        String::new()
    } else {
        format!(" ({})", info.driver)
    };
    format!("{}{drv}, {:?}", info.name, info.backend)
}

/// One graphics adapter as the system reports it (for `jetcarp doctor`).
#[derive(Clone, Debug)]
pub struct AdapterReport {
    pub name: String,
    /// Vulkan, Metal, Dx12, Gl.
    pub backend: String,
    /// DiscreteGpu, IntegratedGpu, VirtualGpu, Cpu, Other.
    pub kind: String,
    pub driver: String,
    /// The adapter the GPU simulation would pick (high performance, not software).
    pub chosen: bool,
    /// Why the GPU simulation cannot use it, if it can't.
    pub problem: Option<String>,
}

/// Every adapter on every backend, marking the one the GPU simulation would use. Identical
/// entries (the same driver installed twice, as inside a Flatpak) are listed once.
pub fn adapter_reports() -> Vec<AdapterReport> {
    let mut seen = std::collections::HashSet::new();
    adapter_list()
        .into_iter()
        .filter(|r| seen.insert((r.name.clone(), r.backend.clone(), r.driver.clone())))
        .collect()
}

fn adapter_list() -> Vec<AdapterReport> {
    let instance =
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env());
    let chosen = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::HighPerformance,
        ..Default::default()
    }))
    .ok()
    .map(|a| a.get_info())
    .filter(|i| i.device_type != wgpu::DeviceType::Cpu);
    pollster::block_on(instance.enumerate_adapters(wgpu::Backends::all()))
        .into_iter()
        .map(|a| {
            let info = a.get_info();
            let problem = if info.device_type == wgpu::DeviceType::Cpu {
                Some("software adapter: the GPU simulation does not use it".to_string())
            } else {
                check_adapter(&a).err()
            };
            let driver = [info.driver.as_str(), info.driver_info.as_str()]
                .iter()
                .filter(|s| !s.is_empty())
                .copied()
                .collect::<Vec<_>>()
                .join(" ");
            AdapterReport {
                chosen: chosen
                    .as_ref()
                    .is_some_and(|c| c.name == info.name && c.backend == info.backend),
                name: info.name,
                backend: format!("{:?}", info.backend),
                kind: format!("{:?}", info.device_type),
                driver,
                problem,
            }
        })
        .collect()
}
