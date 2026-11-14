//! GPU simulation for JETCARP: `wgpu` compute shaders that step CAs on lattice storage (square,
//! hex, any periodic tiling) and explicit storage (aperiodic patches, via a CSR neighbour
//! buffer), with dead, wrap, and live edges. The rules:
//!
//! - Life-like and Generations rules, WireWorld, cyclic, rock-paper-scissors, Greenberg–Hastings,
//!   hodgepodge, the Spectre glider, and priority tables (`jetcarp search evolve`)
//! - JACA, with its annihilation glow and counts
//! - B0 Life-like rules and Margolus blocks, which follow the step count's parity
//! - Larger than Life, its reach in an aux buffer
//! - forest fire, with the CPU's 64-bit random numbers
//! - sandpiles, with 64-bit grain totals and losses
//!
//! Results are bit-identical to the CPU steppers in `jetcarp-core`: everything the state hash
//! covers, and JACA's glow and counts. The engine keeps every piece of bookkeeping (see
//! `jetcarp_core::world::external`):
//!
//! - Generations run in batches without reading the field back. Only a few bytes of per-generation
//!   metrics are read after each batch; the fields are read back once, at the end of [`GpuSim::run`].
//! - **Period detection** stays exact: the GPU compares every generation's states with the Brent
//!   snapshot (bytes, kept on the GPU) and takes new snapshots on the same schedule as the CPU, so
//!   `World::status` reports the same period at the same generation.
//! - **Scripts** keep working: a batch ends where a step's generation count or time limit ends, and
//!   the GPU stops itself after any generation where a population, activity, or repeat trigger
//!   could fire. The engine then checks the trigger exactly as on the CPU.
//! - Anything else (turmites and elementary rules, for example) is reported by
//!   [`GpuSim::check`]; the caller runs those worlds on the CPU.

use jetcarp_core::jaca::JacaRule;
use jetcarp_core::ltl::{Hood, Reach};
use jetcarp_core::rules::{FieldKind, Rule};
use jetcarp_core::world::{ExternalPlan, GenMetrics, Halt};
use jetcarp_core::{Storage, Topology, World};
use rayon::prelude::*;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, Weak};
use std::time::{Duration, Instant};

pub mod pile;

pub use wgpu;

pub mod hunt;

pub mod batch;

pub mod select;

const WG: u32 = 256;
/// Generations per batch at most (per-generation metrics slots).
const MAX_BATCH: u64 = 1024;
/// Dispatches per batch at most (256-byte parameter slots).
const MAX_ENTRIES: usize = 16384;
const PARAM_STRIDE: usize = 256;
const CTRL_HEADER: usize = 8;
/// Activity halts the control shader tracks at once (one streak each).
const MAX_ACT_HALTS: usize = 4;
/// Statistics slots (JACA's counts, a sandpile's lost grains; four words each, one per such step
/// in a batch) start here in the control buffer, after the per-generation metrics. `STAT_BASE` in
/// sim.wgsl counts from the end of the header.
const STAT_BASE: usize = CTRL_HEADER + 4 * MAX_BATCH as usize;
/// "No statistics slot" in [`Params::stat`]; "no open cells" for sandpiles.
const NONE: u32 = u32::MAX;
/// Entries a patch's Larger than Life reach may have (one word each on the GPU): 256 MB.
const REACH_CAP: usize = 1 << 26;

// Rule codes, as in sim.wgsl.
const R_JACA: u32 = 7;
const R_B0: u32 = 8;
const R_MARGOLUS: u32 = 9;
const R_LTL: u32 = 10;
const R_FOREST: u32 = 11;
const R_SANDPILE: u32 = 12;
/// Fields smaller than this are packed and unpacked on one thread.
const PAR_CELLS: usize = 1 << 20;

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
    /// JACA: where the blast zone is read and written (offsets into the aux buffer).
    zin: u32,
    zout: u32,
    halt_repeat: u32,
    compare: u32,
    /// The step's statistics slot, or [`NONE`].
    stat: u32,
    /// Priority rules: the packed rows (see [`priority_words`]), `k1` words.
    /// Margolus: the block table in `table[0]`.
    /// Forest fire: the noise key and thresholds (see [`forest_threshold`]).
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
    /// JACA's contact cells (a blast radius above 0).
    Contact,
    /// JACA's blast zone, grown one neighbourhood step.
    Dilate,
}

const FLAG_COMPARE: u32 = 1;
const FLAG_POP: u32 = 2;
const FLAG_CHANGES: u32 = 4;

/// Forest fire: the CPU's test `noise01(..) < p`, where the noise is `x / 2^53` for a 53-bit
/// integer `x`, holds exactly when `x < ceil(p * 2^53)` (the product is exact in f64). The
/// threshold as (low, high) words, so the GPU compares integers only.
fn forest_threshold(p: f64) -> [u32; 2] {
    let t: u64 = if p.is_nan() || p <= 0.0 {
        0
    } else if p >= 1.0 {
        1 << 53
    } else {
        (p * (1u64 << 53) as f64).ceil() as u64
    };
    [t as u32, (t >> 32) as u32]
}

/// The step's noise key: the part of the CPU's `noise01` that does not depend on the cell.
fn forest_key(seed: u64, tick: u64) -> u64 {
    seed ^ tick.wrapping_mul(0x9E37_79B9_7F4A_7C15)
}

/// Larger than Life's birth and survival tables as bitsets of equal length, long enough for
/// counts up to `max` (the most cells any cell reaches; counts past the CPU's tables are false).
fn ltl_table(l: &jetcarp_core::ltl::LtlRule, max: u32) -> Vec<u32> {
    let words = (max as usize + 1).div_ceil(32);
    let mut out = Vec::with_capacity(2 * words);
    for v in [&l.birth, &l.survive] {
        out.extend((0..words).map(|w| {
            (0..32).fold(0u32, |a, b| {
                a | (v.get(w * 32 + b).copied().unwrap_or(false) as u32) << b
            })
        }));
    }
    out
}

/// Shader dispatches one step of this rule takes: JACA with a blast radius runs a contact pass and
/// one pass per neighbourhood step of the blast before the step itself.
fn passes(p: &Params) -> usize {
    if p.rule == R_JACA && p.thresh > 0 {
        p.thresh as usize + 2
    } else {
        1
    }
}

/// The rule's shader parameters, or why it cannot run on the GPU.
fn rule_params(rule: &Rule) -> Result<Params, ()> {
    let mut p = Params {
        stat: NONE,
        ..Default::default()
    };
    match *rule {
        // B0 Life-like rules alternate complementary rules, by the parity of the CA's step count.
        Rule::LifeLike { birth, survive } if birth & 1 == 1 => {
            (p.rule, p.birth, p.survive, p.states) = (R_B0, birth, survive, 2)
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
        Rule::Jaca(ref j) => {
            let JacaRule {
                pos_birth,
                pos_survive,
                neg_birth,
                neg_survive,
                blast,
            } = **j;
            (p.rule, p.states, p.birth, p.survive, p.k1, p.k2, p.thresh) = (
                R_JACA,
                3,
                pos_birth,
                pos_survive,
                neg_birth,
                neg_survive,
                blast as u32,
            );
        }
        Rule::Margolus(t) => {
            p.rule = R_MARGOLUS;
            for (k, &b) in t.iter().enumerate() {
                p.table[0][k / 4] |= (b as u32) << (8 * (k % 4));
            }
        }
        // The reach and tables go to the aux buffer (see `GpuSim::prepare_aux`).
        Rule::Ltl(ref l) => (p.rule, p.states) = (R_LTL, l.states as u32),
        Rule::ForestFire { p: grow, f: strike } => {
            p.rule = R_FOREST;
            let (g, s) = (forest_threshold(grow), forest_threshold(strike));
            (p.table[0][2], p.table[0][3]) = (g[0], g[1]);
            (p.table[1][0], p.table[1][1]) = (s[0], s[1]);
        }
        Rule::Sandpile => p.rule = R_SANDPILE,
        _ => return Err(()),
    }
    Ok(p)
}

/// Can this rule run on the GPU?
pub fn supports_rule(rule: &Rule) -> bool {
    rule_params(rule).is_ok()
}

/// A paced display (a wallpaper, a screen saver, the app at a set speed) steps on the GPU from
/// this many cells, when the policy is automatic. Below it the processor's step and the GPU's
/// round trip cost about the same; above it the GPU frees the processor. Measured with
/// `examples/bench.rs` (see its `--paced` table).
pub const GPU_PACED_CELLS: usize = 16_000;

/// An unpaced run (`jetcarp run`, scripts, the MCP server, maximum speed) steps on the GPU from
/// this many neighbour reads per generation (cells times neighbours, summed over every CA's
/// passes), when the policy is automatic. Measured with `examples/bench.rs`.
pub const GPU_UNPACED_READS: u64 = 1_000_000;

/// Neighbour reads one generation of the world's enabled CAs makes: cells times neighbours (the
/// tiling's mean for each CA's neighbourhood), summed over each CA's passes.
pub fn neighbour_reads(world: &World) -> u64 {
    let n = world.topo.len() as u64;
    let degree = |nb: jetcarp_core::Nb| -> f64 {
        let k = nb as usize;
        match &world.topo.storage {
            Storage::Lattice(g) => {
                let types = (g.ntypes as usize).max(1);
                g.offsets[k].iter().map(|o| o.len()).sum::<usize>() as f64 / types as f64
            }
            Storage::Explicit(e) => e.csr[k].nb.len() as f64 / n.max(1) as f64,
        }
    };
    world
        .cas
        .iter()
        .filter(|c| c.enabled)
        .map(|c| (n as f64 * degree(c.nb)) as u64 * c.steps_per_gen as u64)
        .sum()
}

impl GpuSim {
    /// Is the world large enough that the GPU beats the processor (the automatic policy)? A
    /// paced display from [`GPU_PACED_CELLS`] cells, anything else from [`GPU_UNPACED_READS`]
    /// neighbour reads per generation. False when no running CA can run on the GPU.
    pub fn worth(world: &World, paced: bool) -> bool {
        let runs = world
            .cas
            .iter()
            .any(|c| c.enabled && supports_rule(&c.rule));
        runs && if paced {
            world.topo.len() >= GPU_PACED_CELLS
        } else {
            neighbour_reads(world) >= GPU_UNPACED_READS
        }
    }
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
    /// A sandpile's field: one grain count per word, and a snapshot of whole words.
    height: bool,
    /// The JACA CA (by id) whose annihilation glow the cells carry in bits 8-15.
    glow_owner: Option<u32>,
}

/// One section of the aux buffer.
#[derive(Clone, PartialEq)]
enum AuxKey {
    /// A Larger than Life reach: neighbourhood (0 Moore, 1 von Neumann, 2 cross) and range.
    Reach(u8, u8),
    /// Larger than Life birth and survival bits.
    Table(Vec<u32>),
    /// A patch's open boundary cells, one bit each.
    Open,
}

/// A Larger than Life reach as the shader reads it, with the most cells any cell reaches.
struct ReachSec {
    words: Vec<u32>,
    max: u32,
}

/// Reaches built for one topology (held weakly, so a world's old tiling is freed), by
/// (neighbourhood, range).
type ReachCache = Option<(
    Weak<Topology>,
    HashMap<(u8, u8), Result<Arc<ReachSec>, String>>,
)>;

struct Resident {
    topo: Arc<Topology>,
    bind0: wgpu::BindGroup,
    _bufs: [wgpu::Buffer; 3],
    gx: u32,
    fields: Vec<Option<GpuField>>,
    rev: u64,
    /// The aux buffer, bound as group 2: the sections in `aux_keys` (at `aux_at`), `aux_len`
    /// words in all, then room for JACA's blast zone (`zone_words`: 2n when a blast runs).
    aux: wgpu::Buffer,
    aux_keys: Vec<AuxKey>,
    aux_at: Vec<u32>,
    aux_len: u32,
    zone_words: usize,
    bind2: wgpu::BindGroup,
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
    layout2: wgpu::BindGroupLayout,
    step: wgpu::ComputePipeline,
    pack: wgpu::ComputePipeline,
    control: wgpu::ComputePipeline,
    contact: wgpu::ComputePipeline,
    dilate: wgpu::ComputePipeline,
    reaches: Mutex<ReachCache>,
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
    /// Open the compute adapter the policy chooses ([`select`]), without a window (CLI, tests,
    /// benchmarks).
    pub fn headless() -> Result<GpuSim, String> {
        let (device, queue, name) = select::compute_device("jetcarp-gpu")?;
        Ok(GpuSim::new(device, queue, name))
    }

    /// Use an existing device (for example the app's `egui_wgpu::RenderState` device), after
    /// checking that its adapter is decent and passes the self-test ([`select::verify`]).
    pub fn from_device(
        adapter: &wgpu::Adapter,
        device: wgpu::Device,
        queue: wgpu::Queue,
    ) -> Result<GpuSim, String> {
        select::verify(adapter)?;
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
        let layout2 = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("jetcarp-gpu aux"),
            entries: &[storage(0, false)],
        });
        let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("jetcarp-gpu"),
            bind_group_layouts: &[Some(&layout0), Some(&layout1), Some(&layout2)],
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
        let (contact, dilate) = (pipe("contact"), pipe("dilate"));
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
        let ctrl_size = ((STAT_BASE + 4 * MAX_ENTRIES) * 4) as u64;
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
            layout2,
            step,
            pack,
            control,
            contact,
            dilate,
            reaches: Mutex::new(None),
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
        let mut jaca = vec![false; world.fields.len()];
        let mut dispatches = 0usize;
        for ca in world.cas.iter().filter(|c| c.enabled) {
            any = true;
            let cpu_only = || format!("{} ({}) runs only on the CPU", ca.name, ca.notation());
            let p = rule_params(&ca.rule).map_err(|_| cpu_only())?;
            let height = world.fields[ca.field].kind == FieldKind::Height;
            if height != (p.rule == R_SANDPILE)
                || world.fields[ca.field].kind == FieldKind::Continuous
            {
                return Err(cpu_only());
            }
            match ca.rule {
                Rule::Jaca(ref j) => {
                    // The glow lives in the field's cells, so one JACA CA per field at a time.
                    if std::mem::replace(&mut jaca[ca.field], true) {
                        return Err("two JACA CAs running on one field run on the CPU".into());
                    }
                    if j.blast > 0 && 2 * n > self.max_cells {
                        return Err(format!(
                            "JACA's blast zone for {n} cells exceeds this GPU's buffer limit"
                        ));
                    }
                }
                Rule::Margolus(_) if world.topo.rows().is_none() => {
                    return Err("Margolus rules on this tiling run on the CPU".into());
                }
                Rule::Ltl(ref l) => {
                    self.reach(&world.topo, l.hood, l.range)
                        .map_err(|e| format!("{}: {e}", cpu_only()))?;
                }
                Rule::Sandpile if plan.halts.contains(&Halt::Settled) => {
                    // The CPU's `done` asks whether a sandpile can still topple, which the GPU
                    // does not measure.
                    return Err("a done trigger on a sandpile runs on the CPU".into());
                }
                _ => {}
            }
            subs[ca.field] += ca.steps_per_gen as usize;
            dispatches += ca.steps_per_gen as usize * passes(&p);
        }
        if !any {
            return Err("no CA is running".into());
        }
        let per_gen: usize = dispatches + plan.running_fields.len() + 1;
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

    /// A Larger than Life reach for the shader, built once per topology: on a lattice, per tile
    /// type a (start, count) pair, then the offsets as (di, dj, type); on a patch, a CSR (n + 1
    /// offsets, then the cells), at most [`REACH_CAP`] cells.
    fn reach(&self, topo: &Arc<Topology>, hood: Hood, r: u8) -> Result<Arc<ReachSec>, String> {
        let mut cache = self.reaches.lock().unwrap_or_else(|e| e.into_inner());
        let same =
            |(t, _): &(Weak<Topology>, _)| t.upgrade().is_some_and(|t| Arc::ptr_eq(&t, topo));
        if !cache.as_ref().is_some_and(same) {
            *cache = Some((Arc::downgrade(topo), HashMap::new()));
        }
        let map = &mut cache.as_mut().unwrap().1;
        let key = (hood as u8, r);
        map.entry(key)
            .or_insert_with(|| {
                let words_max = match jetcarp_core::ltl::reach(topo, hood, r)? {
                    Reach::Lattice(per) => {
                        let mut w = Vec::new();
                        let mut start = 0u32;
                        for list in &per {
                            w.extend_from_slice(&[start, list.len() as u32]);
                            start += list.len() as u32;
                        }
                        for o in per.iter().flatten() {
                            w.extend_from_slice(&[o.di as u32, o.dj as u32, o.k]);
                        }
                        let max = per.iter().map(|l| l.len()).max().unwrap_or(0) as u32;
                        (w, max)
                    }
                    Reach::Explicit(csr) => {
                        if csr.nb.len() > REACH_CAP.min(self.max_cells) {
                            return Err(format!(
                                "its reach on this patch has {} entries, more than the GPU takes ({})",
                                csr.nb.len(),
                                REACH_CAP.min(self.max_cells)
                            ));
                        }
                        let max = csr.off.windows(2).map(|w| w[1] - w[0]).max().unwrap_or(0);
                        let mut w = csr.off;
                        w.extend_from_slice(&csr.nb);
                        (w, max)
                    }
                };
                Ok(Arc::new(ReachSec {
                    words: words_max.0,
                    max: words_max.1,
                }))
            })
            .clone()
    }

    /// Make the aux buffer hold the sections the enabled CAs need (see [`AuxKey`]), with room
    /// after them for JACA's blast zone.
    fn prepare_aux(&mut self, world: &World) -> Result<(), String> {
        let n = world.topo.len();
        let explicit = matches!(world.topo.storage, Storage::Explicit(_));
        let mut keys: Vec<AuxKey> = Vec::new();
        let mut blast = false;
        for ca in world.cas.iter().filter(|c| c.enabled) {
            let mut add = |k: AuxKey| {
                if !keys.contains(&k) {
                    keys.push(k);
                }
            };
            match ca.rule {
                Rule::Ltl(ref l) => {
                    let sec = self.reach(&world.topo, l.hood, l.range)?;
                    add(AuxKey::Reach(l.hood as u8, l.range));
                    add(AuxKey::Table(ltl_table(l, sec.max)));
                }
                Rule::Sandpile if explicit => add(AuxKey::Open),
                Rule::Jaca(ref j) if j.blast > 0 => blast = true,
                _ => {}
            }
        }
        let zone_words = if blast { 2 * n } else { 0 };
        let res = self.res.as_ref().unwrap();
        if res.aux_keys == keys && res.zone_words >= zone_words {
            return Ok(());
        }
        let mut words: Vec<u32> = Vec::new();
        let mut at = Vec::new();
        for k in &keys {
            at.push(words.len() as u32);
            match k {
                AuxKey::Reach(h, r) => {
                    let hood = [Hood::Moore, Hood::VonNeumann, Hood::Cross][*h as usize];
                    let sec = self.reach(&world.topo, hood, *r)?;
                    words.extend_from_slice(&sec.words);
                }
                AuxKey::Table(t) => words.extend_from_slice(t),
                AuxKey::Open => {
                    let mut bits = vec![0u32; n.div_ceil(32)];
                    for (i, b) in bits.iter_mut().enumerate() {
                        for k in 0..32 {
                            let c = i * 32 + k;
                            if c < n && world.topo.is_open(c) {
                                *b |= 1 << k;
                            }
                        }
                    }
                    words.extend_from_slice(&bits);
                }
            }
        }
        let total = words.len() + zone_words;
        if total > self.max_cells {
            return Err(
                "the rules' neighbourhood tables and JACA's blast zone exceed this GPU's buffer limit"
                    .into(),
            );
        }
        let aux = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("jetcarp-gpu aux"),
            size: (total.max(4) * 4) as u64,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        if !words.is_empty() {
            self.queue
                .write_buffer(&aux, 0, bytemuck::cast_slice(&words));
        }
        let bind2 = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("jetcarp-gpu aux"),
            layout: &self.layout2,
            entries: &[entry(0, &aux)],
        });
        let res = self.res.as_mut().unwrap();
        (res.aux, res.bind2) = (aux, bind2);
        res.aux_keys = keys;
        res.aux_at = at;
        res.aux_len = words.len() as u32;
        res.zone_words = zone_words;
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
            if let Err(e) = self
                .check_plan(world, &plan)
                .and_then(|_| self.prepare_aux(world))
            {
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
        // A placeholder until `prepare_aux` sizes it.
        let aux = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("jetcarp-gpu aux"),
            size: 16,
            usage: wgpu::BufferUsages::STORAGE,
            mapped_at_creation: false,
        });
        let bind2 = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("jetcarp-gpu aux"),
            layout: &self.layout2,
            entries: &[entry(0, &aux)],
        });
        self.res = Some(Resident {
            topo,
            bind0,
            _bufs: bufs,
            gx,
            fields: Vec::new(),
            rev: world.revision(),
            aux,
            aux_keys: Vec::new(),
            aux_at: Vec::new(),
            aux_len: 0,
            zone_words: 0,
            bind2,
        });
    }

    /// Make every running field resident and current; read back fields that stopped running.
    fn prepare_fields(&mut self, world: &mut World, plan: &ExternalPlan) {
        let running = plan.running_fields.clone();
        // The JACA CA whose glow each running field carries (`check_plan` allows one).
        let owner = |f: usize| {
            world
                .cas
                .iter()
                .find(|c| c.enabled && c.field == f && matches!(c.rule, Rule::Jaca(_)))
                .map(|c| c.id)
        };
        let owners: Vec<Option<u32>> = (0..world.fields.len()).map(owner).collect();
        // Fields that stopped running, or whose glow changes hands, go back to the world first.
        let moved: Vec<usize> = self.res.as_ref().map_or(vec![], |r| {
            r.fields
                .iter()
                .enumerate()
                .filter_map(|(f, g)| g.as_ref().map(|g| (f, g)))
                .filter(|(f, g)| g.valid && owners.get(*f).is_some_and(|o| *o != g.glow_owner))
                .map(|(f, _)| f)
                .collect()
        });
        self.read_back(world, |f| !running.contains(&f) || moved.contains(&f));
        let n = world.topo.len();
        let res = self.res.as_mut().unwrap();
        for &f in &moved {
            if let Some(g) = res.fields[f].as_mut() {
                g.valid = false;
            }
        }
        if res.fields.len() < world.fields.len() {
            res.fields.resize_with(world.fields.len(), || None);
        }
        for &fi in &running {
            let height = world.fields[fi].kind == FieldKind::Height;
            if res.fields[fi].as_ref().is_some_and(|g| g.height != height) {
                res.fields[fi] = None;
            }
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
                    size: snap_bytes(n, height),
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
                    height,
                    glow_owner: None,
                });
            }
            let gf = res.fields[fi].as_mut().unwrap();
            if gf.valid {
                continue;
            }
            let f = &world.fields[fi];
            gf.glow_owner = owners[fi];
            gf.cur = 0;
            if height {
                self.queue
                    .write_buffer(&gf.bufs[0], 0, bytemuck::cast_slice(&f.height));
                if let Some(h) = world.watch_snapshot_heights(fi) {
                    let mut words = h.to_vec();
                    words.resize(n.max(1), 0);
                    self.queue
                        .write_buffer(&gf.snap, 0, bytemuck::cast_slice(&words));
                }
            } else {
                // The owner's glow, unless it has none yet for these cells (the CPU starts it
                // at 0 then).
                let glow = gf.glow_owner.and_then(|id| {
                    let ca = world.cas.iter().find(|c| c.id == id)?;
                    ca.heat.as_ref().map(|h| &h.glow).filter(|g| g.len() == n)
                });
                let pack = |(i, (&s, &a)): (usize, (&u8, &u16))| {
                    let g = glow.map_or(0, |g| g[i] as u32);
                    s as u32 | g << 8 | (a as u32) << 16
                };
                // Waking every worker thread for a small field costs more processor time
                // than the copy (a wallpaper sends its world each time it changes).
                let packed: Vec<u32> = if n < PAR_CELLS {
                    f.state.iter().zip(&f.age).enumerate().map(pack).collect()
                } else {
                    f.state
                        .par_iter()
                        .zip(f.age.par_iter())
                        .enumerate()
                        .map(pack)
                        .collect()
                };
                self.queue
                    .write_buffer(&gf.bufs[0], 0, bytemuck::cast_slice(&packed));
                if let Some(s) = world.watch_snapshot_states(fi) {
                    let mut bytes = s.to_vec();
                    bytes.resize(n.div_ceil(4).max(1) * 4, 0);
                    self.queue.write_buffer(&gf.snap, 0, &bytes);
                }
            }
            gf.valid = true;
            gf.dirty = false;
        }
    }

    /// A CA's shader parameters for this world: the rule's, plus what depends on the topology
    /// and the aux buffer (`prepare_aux` has run).
    fn ca_params(&self, world: &World, ci: usize) -> Params {
        let ca = &world.cas[ci];
        let mut p = rule_params(&ca.rule).unwrap();
        p.nb = ca.nb as u32;
        let res = self.res.as_ref().unwrap();
        let at = |k: &AuxKey| {
            res.aux_keys
                .iter()
                .position(|x| x == k)
                .map_or(NONE, |i| res.aux_at[i])
        };
        match ca.rule {
            Rule::Margolus(_) => {
                // Blocks wrap across the seam only on an axis of even length.
                if let Some(g) = world.topo.rows() {
                    p.gg = (g.wrap && g.w % 2 == 0) as u32 | ((g.wrap && g.h % 2 == 0) as u32) << 1;
                }
            }
            Rule::Ltl(ref l) => {
                let sec = self.reach(&world.topo, l.hood, l.range).unwrap();
                let table = ltl_table(l, sec.max);
                p.k1 = at(&AuxKey::Reach(l.hood as u8, l.range));
                p.k2 = at(&AuxKey::Table(table.clone()));
                p.gg = (table.len() / 2) as u32;
            }
            Rule::Sandpile => {
                p.k2 = world.topo.center_cell() as u32;
                p.gg = at(&AuxKey::Open);
            }
            _ => {}
        }
        p
    }

    /// Run one batch of at most `len` generations; returns the generations done.
    fn batch(&mut self, world: &mut World, plan: &ExternalPlan, len: u64) -> Result<u64, String> {
        let enabled: Vec<usize> = (0..world.cas.len())
            .filter(|&c| world.cas[c].enabled)
            .collect();
        let nf = world.fields.len();
        let n = world.topo.len();
        // Last dispatch touching each field in a generation gets the metric flags.
        let mut last = vec![usize::MAX; nf];
        let mut subs = vec![0u64; nf];
        // Each step of a generation: (index into `enabled`, step within the generation).
        let mut order: Vec<(usize, u64)> = Vec::new();
        for (ei, &ci) in enabled.iter().enumerate() {
            let ca = &world.cas[ci];
            for s in 0..ca.steps_per_gen as u64 {
                last[ca.field] = order.len();
                order.push((ei, s));
            }
            subs[ca.field] += ca.steps_per_gen as u64;
        }
        let rule: Vec<Params> = enabled
            .iter()
            .map(|&ci| self.ca_params(world, ci))
            .collect();
        let per_gen = order
            .iter()
            .map(|&(ei, _)| passes(&rule[ei]))
            .sum::<usize>()
            + plan.running_fields.len()
            + 1;
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
        // Statistics slots in order: (generation, index into `enabled`).
        let mut stats: Vec<(usize, usize)> = Vec::new();
        let push = |p: Params, bytes: &mut Vec<u8>| {
            let at = bytes.len();
            bytes.resize(at + PARAM_STRIDE, 0);
            bytes[at..at + std::mem::size_of::<Params>()].copy_from_slice(bytemuck::bytes_of(&p));
        };
        for (g, &(compare, take)) in sched.iter().enumerate() {
            for (d, &(ei, s)) in order.iter().enumerate() {
                let ca = &world.cas[enabled[ei]];
                let f = ca.field;
                let mut p = rule[ei];
                p.op = 0;
                p.gen_slot = g as u32;
                // The CA's step count before this step, as `CaInstance::ticks` counts them.
                let ticks = ca.ticks + g as u64 * ca.steps_per_gen as u64 + s;
                match p.rule {
                    // B0: the 1-based step number's parity, and whether it is the first step.
                    R_B0 => p.k1 = ((ticks + 1) & 1) as u32 | ((ticks == 0) as u32) << 1,
                    R_MARGOLUS => p.k2 = (ticks & 1) as u32,
                    R_FOREST => {
                        let key = forest_key(ca.noise, ticks + 1);
                        (p.table[0][0], p.table[0][1]) = (key as u32, (key >> 32) as u32);
                    }
                    R_SANDPILE => {
                        // Grains drop on the first step of a generation.
                        p.k1 = if s == 0 { ca.drop_per_gen } else { 0 };
                        p.stat = stats.len() as u32;
                        stats.push((g, ei));
                    }
                    R_JACA => {
                        p.stat = stats.len() as u32;
                        stats.push((g, ei));
                        if p.thresh > 0 {
                            // Contact cells into the zone's first half, then one pass per blast
                            // step, alternating halves; the step reads the half written last.
                            let (z0, half) = (res.aux_len, n as u32);
                            let pass = |op, zin, zout| Params {
                                op,
                                rule: R_JACA,
                                nb: p.nb,
                                gen_slot: g as u32,
                                zin,
                                zout,
                                stat: p.stat,
                                ..Default::default()
                            };
                            push(pass(3, z0, z0), &mut bytes);
                            list.push((Op::Contact, Some((f, cur[f]))));
                            for k in 0..p.thresh {
                                let (zin, zout) = (z0 + (k % 2) * half, z0 + ((k + 1) % 2) * half);
                                push(pass(4, zin, zout), &mut bytes);
                                list.push((Op::Dilate, Some((f, cur[f]))));
                            }
                            p.zin = z0 + (p.thresh % 2) * half;
                        }
                    }
                    _ => {}
                }
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
                    let height = res.fields[f].as_ref().is_some_and(|g| g.height);
                    push(
                        Params {
                            op: 1,
                            rule: height as u32,
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
                        pop_n: pop_max as u32,
                        k1: (pop_max >> 32) as u32,
                        thresh: pop_min as u32,
                        k2: (pop_min >> 32) as u32,
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
        let ctrl_words = if stats.is_empty() {
            CTRL_HEADER + 4 * len as usize
        } else {
            STAT_BASE + 4 * stats.len()
        };
        let mut init = vec![0u32; ctrl_words];
        for (k, a) in acts.iter().enumerate() {
            init[2 + k] = a.3.min(u32::MAX as u64) as u32;
        }
        self.queue
            .write_buffer(&self.ctrl, 0, bytemuck::cast_slice(&init));

        let groups = |items: u32| {
            let gtotal = items.div_ceil(WG).max(1);
            (gtotal.min(res.gx), gtotal.div_ceil(res.gx))
        };
        let (sx, sy) = groups(n as u32);
        let (px, py) = groups((n as u32).div_ceil(4));
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
            pass.set_bind_group(2, &res.bind2, &[]);
            let mut bound: Option<Op> = None;
            for (k, (op, field)) in list.iter().enumerate() {
                if bound != Some(*op) {
                    pass.set_pipeline(match op {
                        Op::Step => &self.step,
                        Op::Pack => &self.pack,
                        Op::Control => &self.control,
                        Op::Contact => &self.contact,
                        Op::Dilate => &self.dilate,
                    });
                    bound = Some(*op);
                }
                pass.set_bind_group(0, &res.bind0, &[(k * PARAM_STRIDE) as u32]);
                if let Some((f, parity)) = field {
                    let gf = res.fields[*f].as_ref().unwrap();
                    pass.set_bind_group(1, &gf.bind[*parity], &[]);
                }
                match op {
                    Op::Step | Op::Contact | Op::Dilate => pass.dispatch_workgroups(sx, sy, 1),
                    Op::Pack => {
                        let height = field
                            .is_some_and(|(f, _)| res.fields[f].as_ref().is_some_and(|g| g.height));
                        let (x, y) = if height { (sx, sy) } else { (px, py) };
                        pass.dispatch_workgroups(x, y, 1)
                    }
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
                    population: m[0] as u64 | (m[3] as u64) << 32,
                    changed: m[1] as u64,
                    repeat: sched[g].0 && m[2] == 0,
                    counted,
                }
            })
            .collect();
        // JACA's counts and sandpiles' lost grains, for the generations done (as `step_ca`
        // records them, step by step).
        for (slot, &(g, ei)) in stats.iter().enumerate() {
            if g as u64 >= k {
                break;
            }
            let w = &words[STAT_BASE + 4 * slot..STAT_BASE + 4 * slot + 4];
            let ca = &mut world.cas[enabled[ei]];
            if matches!(ca.rule, Rule::Jaca(_)) {
                let heat = Arc::make_mut(ca.heat.get_or_insert_with(Default::default));
                heat.last = jetcarp_core::jaca::Stats {
                    contacts: w[0] as u64,
                    destroyed: w[1] as u64,
                    cancelled: w[2] as u64,
                };
                heat.total_contacts += w[0] as u64;
                heat.total_destroyed += w[1] as u64;
            } else {
                let f = ca.field;
                world.fields[f].lost += w[0] as u64 | (w[1] as u64) << 32;
            }
        }
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
        let recorded = world.finish_external(&metrics, || this.read_snapshots(&running, n));
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
    fn apply_edits(
        &mut self,
        world: &mut World,
        pending: Vec<(usize, jetcarp_core::reseed::Edits)>,
    ) {
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

    /// The period-watch snapshots of these fields: (field, states, heights), the states of a
    /// state field and the grain counts of a sandpile's.
    fn read_snapshots(&self, fields: &[usize], n: usize) -> Vec<(usize, Vec<u8>, Vec<u32>)> {
        let res = self.res.as_ref().unwrap();
        fields
            .iter()
            .map(|&f| {
                let gf = res.fields[f].as_ref().unwrap();
                let bytes = snap_bytes(n, gf.height);
                if gf.height {
                    let h: Vec<u32> = self.copy_out(&gf.snap, bytes).unwrap_or_default();
                    (f, vec![], h)
                } else {
                    let v: Vec<u8> = self.copy_out(&gf.snap, bytes).unwrap_or_default();
                    (f, v, vec![])
                }
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

    /// Copy dirty fields that `pick` selects back into the world: states and ages (and the
    /// owner's JACA glow), or a sandpile's grain counts.
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
                if gf.height {
                    f.height.copy_from_slice(&words[..n]);
                } else {
                    let unpack = |((s, a), &x): ((&mut u8, &mut u16), &u32)| {
                        *s = (x & 0xff) as u8;
                        *a = (x >> 16) as u16;
                    };
                    // Waking every worker thread for a small field costs more processor time
                    // than the copy (a wallpaper reads its world back every frame).
                    let small = n < PAR_CELLS;
                    if small {
                        f.state
                            .iter_mut()
                            .zip(f.age.iter_mut())
                            .zip(&words[..n])
                            .for_each(unpack);
                    } else {
                        f.state
                            .par_iter_mut()
                            .zip(f.age.par_iter_mut())
                            .zip(words[..n].par_iter())
                            .for_each(unpack);
                    }
                    let owner = gf
                        .glow_owner
                        .and_then(|id| world.cas.iter_mut().find(|c| c.id == id));
                    if let Some(ca) = owner {
                        let heat = Arc::make_mut(ca.heat.get_or_insert_with(Default::default));
                        heat.glow.resize(n, 0);
                        let glow = |(g, &x): (&mut u8, &u32)| *g = (x >> 8) as u8;
                        if small {
                            heat.glow.iter_mut().zip(&words[..n]).for_each(glow);
                        } else {
                            heat.glow
                                .par_iter_mut()
                                .zip(words[..n].par_iter())
                                .for_each(glow);
                        }
                    }
                }
            }
            staging.unmap();
            gf.dirty = false;
        }
    }
}

/// Bytes of a field's period-watch snapshot on the GPU: a byte per state, or a word per grain
/// count.
fn snap_bytes(n: usize, height: bool) -> u64 {
    if height {
        (n.max(1) * 4) as u64
    } else {
        (n.div_ceil(4).max(1) * 4) as u64
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
    /// The adapter the GPU simulation computes on ([`select::pick`] for computing).
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
    let adapters = pollster::block_on(instance.enumerate_adapters(wgpu::Backends::all()));
    let policy = select::Policy::load();
    let chosen = select::pick_from(&adapters, &policy, select::Purpose::Compute, None)
        .ok()
        .map(|c| c.info);
    adapters
        .into_iter()
        .map(|a| {
            let info = a.get_info();
            let problem = select::requirements(
                &info,
                &a.limits(),
                &a.get_downlevel_capabilities(),
                &policy.want,
            )
            .err()
            .or_else(|| {
                let t = select::self_test(&a);
                (!t.passed).then_some(t.note)
            });
            let driver = [info.driver.as_str(), info.driver_info.as_str()]
                .iter()
                .filter(|s| !s.is_empty())
                .copied()
                .collect::<Vec<_>>()
                .join(" ");
            AdapterReport {
                chosen: chosen.as_ref().is_some_and(|c| {
                    c.name == info.name && c.backend == info.backend && c.driver == info.driver
                }),
                name: info.name,
                backend: format!("{:?}", info.backend),
                kind: format!("{:?}", info.device_type),
                driver,
                problem,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The integer threshold decides exactly as the CPU's `x / 2^53 < p` in f64, right at the
    /// boundary, for probabilities of every size.
    #[test]
    fn forest_threshold_is_exact() {
        let two53 = (1u64 << 53) as f64;
        let mut ps = vec![
            0.0, 1.0, 0.5, 0.25, 1e-300, 1e-17, 1e-9, 0.01, 0.0001, 0.3, 0.999999,
        ];
        ps.extend([
            1.0 / two53,
            3.0 / two53,
            1.0 - 1.0 / two53,
            f64::MIN_POSITIVE,
        ]);
        let mut x = 0x2545_f491_4f6c_dd1du64;
        for _ in 0..2000 {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            ps.push((x >> 11) as f64 / two53);
            ps.push(((x >> 11) as f64 / two53).powi(5));
        }
        for p in ps {
            let [lo, hi] = forest_threshold(p);
            let t = lo as u64 | (hi as u64) << 32;
            for x in t.saturating_sub(2)..(t + 2).min(1 << 53) {
                assert_eq!(x < t, (x as f64) / two53 < p, "p = {p:e}, x = {x}");
            }
        }
        assert_eq!(forest_threshold(f64::NAN), [0, 0]);
        assert_eq!(forest_threshold(-1.0), [0, 0]);
        assert_eq!(forest_threshold(2.0), [0, 1 << 21]);
    }
}
