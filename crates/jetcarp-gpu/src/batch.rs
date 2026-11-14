//! GPU batch stepper: many worlds on one topology, each with its own rule, stepped together and
//! measured every generation. The traces and frames equal `jetcarp_core::explore::batch::run_cpu`.
//!
//! All worlds live in one pair of state buffers. A world's stride is the cell count rounded up to
//! a multiple of the workgroup size, so a workgroup belongs to exactly one world. Every topology
//! kind runs through the same neighbour lists (`neighbor_csr`), stored as records of deltas shared
//! between cells so that the table fits in the GPU's cache (see `intern_records`); without
//! that, large worlds run several times slower. Generations run in short submissions (75 ms by
//! default, see [`BatchSim::set_target_secs`]); a generation too large for one submission is
//! split into ranges of worlds, so the driver never sees a long-running dispatch.
//!
//! [`BatchSim`] is an explore search's GPU backend (`jetcarp_core::explore::batch::Backend`).

use jetcarp_core::Topology;
use jetcarp_core::explore::batch::{Backend, BatchJob, GIVEN_UP, WorldTrace, mix, neighbor_csr};
use jetcarp_core::jaca::JacaRule;
use jetcarp_core::ltl::{Hood, LtlRule, Reach};
use jetcarp_core::rules::Rule;
use jetcarp_core::topology::{Csr, Nb, Storage};
use rayon::prelude::*;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

const WG: usize = 256;
/// Dynamic uniform slots (256 bytes each) available per submission.
const MAX_SLOTS: usize = 16384;
const SLOT_BYTES: u64 = 256;
/// Generations of trace data one submission may produce at most.
const MAX_CHUNK_GENS: u32 = 1024;
/// The largest trace buffer a run allocates.
const TRACE_BUDGET: u64 = 128 << 20;
/// Bytes of trace per world and generation: population, changed, hash, and cells in state 2.
const TRACE_ENTRY: u64 = 16;
/// How long one submission should take.
const TARGET_SECS: f64 = 0.075;
/// Bytes of init data per submission, so upload staging stays small.
const UPLOAD_BYTES: usize = 128 << 20;

const KIND_BS: u32 = 0;
const KIND_WIRE: u32 = 1;
const KIND_CYCLIC: u32 = 2;
const KIND_GH: u32 = 3;
const KIND_HODGEPODGE: u32 = 4;
const KIND_GLIDER: u32 = 5;
const KIND_PRIORITY: u32 = 6;
const KIND_LTL: u32 = 7;
const KIND_JACA: u32 = 8;
const KIND_B0: u32 = 9;

#[repr(C)]
#[derive(Clone, Copy, Default, bytemuck::Pod, bytemuck::Zeroable)]
struct WorldParams {
    kind: u32,
    states: u32,
    birth: u32,
    survive: u32,
    a: u32,
    b: u32,
    c: u32,
    /// The CSR this world uses (0, or 1 for Larger than Life in a mixed batch).
    d: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Default, bytemuck::Pod, bytemuck::Zeroable)]
struct Cfg {
    n: u32,
    stride: u32,
    gx: u32,
    worlds: u32,
    live: u32,
    pad: [u32; 3],
}

/// The bitset words a Larger than Life table needs per direction: counts 0..=`max_count`.
fn ltl_words(max_count: u32) -> u32 {
    (max_count + 1).div_ceil(32)
}

fn bitset(v: &[bool], words: u32) -> impl Iterator<Item = u32> + '_ {
    (0..words as usize).map(move |w| {
        (0..32).fold(0u32, |a, b| {
            a | ((v.get(w * 32 + b).copied().unwrap_or(false) as u32) << b)
        })
    })
}

/// Fill `tables` as needed and return the shader parameters, or `None` when the rule is not
/// supported in a batch. `words` is the bitset length of Larger than Life tables.
fn world_params(rule: &Rule, tables: &mut Vec<u32>, words: u32) -> Option<WorldParams> {
    let mut p = WorldParams::default();
    match rule {
        Rule::LifeLike { birth, survive } if birth & 1 == 1 => {
            (p.kind, p.birth, p.survive) = (KIND_B0, *birth, *survive);
        }
        Rule::LifeLike { birth, survive } => {
            (p.kind, p.birth, p.survive, p.states) = (KIND_BS, *birth, *survive, 2);
        }
        Rule::Generations {
            birth,
            survive,
            states,
        } => (p.kind, p.birth, p.survive, p.states) = (KIND_BS, *birth, *survive, *states as u32),
        Rule::WireWorld => p.kind = KIND_WIRE,
        Rule::Cyclic { states, threshold } => {
            (p.kind, p.states, p.a) = (KIND_CYCLIC, *states as u32, *threshold as u32);
        }
        Rule::Rps { states, threshold } => {
            (p.kind, p.states, p.a) = (
                KIND_CYCLIC,
                *states as u32,
                threshold.wrapping_add(1) as u32,
            );
        }
        Rule::GreenbergHastings { states, threshold } => {
            (p.kind, p.states, p.a) = (KIND_GH, *states as u32, *threshold as u32);
        }
        Rule::Hodgepodge { n, k1, k2, g } if *k1 > 0 && *k2 > 0 => {
            (p.kind, p.states, p.a, p.b, p.c) = (KIND_HODGEPODGE, *n as u32, *k1, *k2, *g);
        }
        Rule::SpectreGlider => p.kind = KIND_GLIDER,
        Rule::Priority(r) => {
            if r.states > 8
                || r.rows.iter().any(|row| {
                    row.next >= 8
                        || row.own.is_some_and(|o| o >= 8)
                        || row.conds.iter().any(|&(s, _)| s >= 8)
                })
            {
                return None;
            }
            let w = crate::pack_priority(r);
            (p.kind, p.states, p.a, p.b) = (
                KIND_PRIORITY,
                r.states as u32,
                tables.len() as u32,
                w.len() as u32,
            );
            tables.extend_from_slice(&w);
        }
        Rule::Ltl(l) => {
            let LtlRule {
                states,
                birth,
                survive,
                ..
            } = l.as_ref();
            (p.kind, p.states, p.a, p.b) = (KIND_LTL, *states as u32, tables.len() as u32, words);
            tables.extend(bitset(birth, words));
            tables.extend(bitset(survive, words));
        }
        Rule::Jaca(j) => {
            let JacaRule {
                pos_birth,
                pos_survive,
                neg_birth,
                neg_survive,
                blast,
            } = j.as_ref();
            (p.kind, p.birth, p.survive, p.a, p.b, p.c) = (
                KIND_JACA,
                *pos_birth,
                *pos_survive,
                *neg_birth,
                *neg_survive,
                *blast as u32,
            );
        }
        _ => return None,
    }
    Some(p)
}

/// A storage buffer that is kept while later requests are about the same size.
struct Pool {
    buf: Option<wgpu::Buffer>,
    usage: wgpu::BufferUsages,
    label: &'static str,
}

impl Pool {
    fn new(label: &'static str, usage: wgpu::BufferUsages) -> Pool {
        Pool {
            buf: None,
            usage,
            label,
        }
    }

    fn ensure(&mut self, device: &wgpu::Device, need: u64) -> &wgpu::Buffer {
        let need = need.max(16);
        let keep = self
            .buf
            .as_ref()
            .is_some_and(|b| b.size() >= need && b.size() / 4 <= need.max(1 << 20));
        if !keep {
            // Free the old buffer first: both can be large on this machine's shared memory.
            self.buf = None;
            self.buf = Some(
                device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some(self.label),
                    size: need
                        .next_multiple_of(1 << 20)
                        .min(device.limits().max_buffer_size),
                    usage: self.usage,
                    mapped_at_creation: false,
                }),
            );
        }
        self.buf.as_ref().unwrap()
    }

    fn get(&self) -> &wgpu::Buffer {
        self.buf.as_ref().unwrap()
    }
}

/// The neighbour lists resident in `topo`.
struct TopoKey {
    topo: Arc<Topology>,
    nb: Nb,
    needs_nb: bool,
    ltl: Option<(Hood, u8)>,
}

impl TopoKey {
    fn matches(
        &self,
        topo: &Arc<Topology>,
        nb: Nb,
        needs_nb: bool,
        ltl: Option<(Hood, u8)>,
    ) -> bool {
        Arc::ptr_eq(&self.topo, topo)
            && self.needs_nb == needs_nb
            && self.ltl == ltl
            && (!needs_nb || self.nb == nb)
    }
}

struct TopoInfo {
    key: TopoKey,
    /// Largest count a neighbourhood can reach (present neighbours plus missing slots).
    max_count: u32,
    /// Neighbour-list entries per cell (a cost estimate).
    avg_edges: f64,
    /// Sum of `mix` over the cells: the hash of an all-zero world.
    base_hash: u32,
}

/// Build the shared neighbour records of one list: `cells[i]` is the offset (into the pool) of a
/// record `[deg | missing << 16, delta...]`, where each delta is `neighbour - cell`, wrapping.
/// Records are shared between cells with the same deltas, so a lattice needs only a handful: this
/// is what keeps the table in the GPU's second-level cache, however many cells a world has.
fn intern_records(
    csr: &Csr,
    slots: &(dyn Fn(usize) -> usize + Sync),
    n: usize,
    pool: &mut Vec<u32>,
) -> Vec<u32> {
    // A record's hash leads to the offsets of records with that hash; identity is by comparing
    // the record's words. One scratch list for the deltas keeps memory near the size of `pool`.
    let hash = |w: &[u32]| {
        w.iter().fold(0xcbf29ce484222325u64, |h, &x| {
            (h ^ x as u64).wrapping_mul(0x100000001b3)
        })
    };
    let mut seen: HashMap<u64, Vec<u32>> = HashMap::new();
    let mut rec: Vec<u32> = Vec::new();
    (0..n)
        .map(|i| {
            rec.clear();
            rec.push(0);
            rec.extend(csr.row(i).iter().map(|&x| x.wrapping_sub(i as u32)));
            rec[1..].sort_unstable();
            let deg = rec.len() - 1;
            let missing = (slots(i) as u32).saturating_sub(deg as u32);
            assert!(deg < 65536 && missing < 65536, "neighbourhood too large");
            rec[0] = deg as u32 | (missing << 16);
            let bucket = seen.entry(hash(&rec)).or_default();
            if let Some(&at) = bucket
                .iter()
                .find(|&&at| pool[at as usize..at as usize + rec.len()] == rec[..])
            {
                return at;
            }
            let at = pool.len() as u32;
            pool.extend_from_slice(&rec);
            bucket.push(at);
            at
        })
        .collect()
}

/// Steps batches of worlds on one `wgpu` device.
pub struct BatchSim {
    device: wgpu::Device,
    queue: wgpu::Queue,
    name: String,
    max_cells: usize,
    layout0: wgpu::BindGroupLayout,
    layout1: wgpu::BindGroupLayout,
    layout2: wgpu::BindGroupLayout,
    step: wgpu::ComputePipeline,
    contact: wgpu::ComputePipeline,
    dilate: wgpu::ComputePipeline,
    cfg: wgpu::Buffer,
    disp: wgpu::Buffer,
    no_zone: [wgpu::Buffer; 2],
    state: [Pool; 2],
    zone: [Pool; 2],
    topo: Pool,
    worlds: Pool,
    tables: Pool,
    trace: Pool,
    trace_read: Pool,
    frame_read: Pool,
    info: Option<TopoInfo>,
    /// Seconds per neighbour-list entry processed, measured from recent submissions.
    secs_per_edge: f64,
    target_secs: f64,
    stats: BatchStats,
}

/// How the last [`BatchSim::run`] used the GPU.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct BatchStats {
    /// Queue submissions used for stepping.
    pub submissions: u32,
    /// The longest of them, in seconds (time from submission to its results being ready).
    pub longest_secs: f64,
}

fn entry(binding: u32, buf: &wgpu::Buffer) -> wgpu::BindGroupEntry<'_> {
    wgpu::BindGroupEntry {
        binding,
        resource: buf.as_entire_binding(),
    }
}

impl BatchSim {
    /// Open the compute adapter the policy chooses ([`crate::select`]), without a window.
    pub fn headless() -> Result<BatchSim, String> {
        let (device, queue, name) = crate::select::compute_device("jetcarp-gpu batch")?;
        Ok(BatchSim::new(device, queue, name))
    }

    pub(crate) fn new(device: wgpu::Device, queue: wgpu::Queue, name: String) -> BatchSim {
        let lim = device.limits();
        let max_bytes = lim.max_storage_buffer_binding_size.min(lim.max_buffer_size);
        let max_cells = (max_bytes / 4).min(u32::MAX as u64 / 2) as usize;
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("jetcarp-gpu batch"),
            source: wgpu::ShaderSource::Wgsl(include_str!("batch.wgsl").into()),
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
        let uniform = |binding, dynamic: bool, min: u64| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::COMPUTE,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: dynamic,
                min_binding_size: wgpu::BufferSize::new(min),
            },
            count: None,
        };
        let layout0 = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("batch shared"),
            entries: &[
                uniform(0, false, std::mem::size_of::<Cfg>() as u64),
                uniform(1, true, 16),
                storage(2, true),
                storage(3, true),
                storage(4, true),
                storage(5, false),
            ],
        });
        let layout1 = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("batch state"),
            entries: &[storage(0, true), storage(1, false)],
        });
        let layout2 = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("batch zone"),
            entries: &[storage(0, true), storage(1, false)],
        });
        let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("jetcarp-gpu batch"),
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
        let (step, contact, dilate) = (pipe("step"), pipe("contact"), pipe("dilate"));
        let cfg = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("batch cfg"),
            size: std::mem::size_of::<Cfg>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let disp = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("batch dispatches"),
            size: MAX_SLOTS as u64 * SLOT_BYTES,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let no_zone = [(), ()].map(|_| {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("batch no zone"),
                size: 16,
                usage: wgpu::BufferUsages::STORAGE,
                mapped_at_creation: false,
            })
        });
        use wgpu::BufferUsages as U;
        let cells = U::STORAGE | U::COPY_DST | U::COPY_SRC;
        BatchSim {
            device,
            queue,
            name,
            max_cells,
            layout0,
            layout1,
            layout2,
            step,
            contact,
            dilate,
            cfg,
            disp,
            no_zone,
            state: [
                Pool::new("batch state a", cells),
                Pool::new("batch state b", cells),
            ],
            zone: [
                Pool::new("batch zone a", U::STORAGE),
                Pool::new("batch zone b", U::STORAGE),
            ],
            topo: Pool::new("batch topo", U::STORAGE | U::COPY_DST),
            worlds: Pool::new("batch worlds", U::STORAGE | U::COPY_DST),
            tables: Pool::new("batch tables", U::STORAGE | U::COPY_DST),
            trace: Pool::new("batch trace", U::STORAGE | U::COPY_DST | U::COPY_SRC),
            trace_read: Pool::new("batch trace read", U::MAP_READ | U::COPY_DST),
            frame_read: Pool::new("batch frame read", U::MAP_READ | U::COPY_DST),
            info: None,
            secs_per_edge: 2e-11,
            target_secs: TARGET_SECS,
            stats: BatchStats::default(),
        }
    }

    /// Adapter name and backend, for status lines.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Can this rule run in a batch?
    pub fn supports(rule: &Rule) -> bool {
        world_params(rule, &mut Vec::new(), 0).is_some()
    }

    /// Most cells (over all worlds, after padding) one batch may hold on this device.
    pub fn max_cells(&self) -> usize {
        self.max_cells
    }

    /// How long one queue submission should take at most, about (default 75 ms). Lower it to
    /// leave the desktop more GPU time; the batch takes longer overall.
    pub fn set_target_secs(&mut self, secs: f64) {
        self.target_secs = secs.max(1e-6);
    }

    /// Submission counts and durations of the last run.
    pub fn last_stats(&self) -> BatchStats {
        self.stats
    }

    /// Upload the neighbour lists unless the same ones are already resident.
    fn bind_topology(
        &mut self,
        job: &BatchJob,
        needs_nb: bool,
        ltl: Option<(Hood, u8)>,
    ) -> Result<(), String> {
        if self
            .info
            .as_ref()
            .is_some_and(|i| i.key.matches(&job.topo, job.nb, needs_nb, ltl))
        {
            return Ok(());
        }
        let topo = &job.topo;
        let n = topo.len();
        let mut lists: Vec<(Csr, Vec<u32>)> = Vec::new();
        let mut pool: Vec<u32> = Vec::new();
        if needs_nb {
            let csr = neighbor_csr(topo, job.nb, None)?;
            let slots = |i: usize| topo.degree(i, job.nb);
            let cells = intern_records(&csr, &slots, n, &mut pool);
            lists.push((csr, cells));
        }
        if let Some((hood, r)) = ltl {
            let csr = neighbor_csr(topo, job.nb, Some((hood, r)))?;
            let reach = jetcarp_core::ltl::reach(topo, hood, r)?;
            let cells = match (&reach, &topo.storage) {
                (Reach::Lattice(per), Storage::Lattice(g)) => {
                    let slots = |i: usize| per[g.decompose(i).2 as usize].len();
                    intern_records(&csr, &slots, n, &mut pool)
                }
                _ => {
                    let slots = |i: usize| (csr.off[i + 1] - csr.off[i]) as usize;
                    intern_records(&csr, &slots, n, &mut pool)
                }
            };
            lists.push((csr, cells));
        }
        for (csr, _) in &lists {
            // Every neighbour index is below the cell count, by construction of the lists, so
            // `cell + delta` stays inside its world.
            if csr.nb.par_iter().any(|&x| x as usize >= n) {
                return Err("a neighbour list points past the world".into());
            }
        }
        let table_len = lists.len() * n;
        let mut words: Vec<u32> = Vec::with_capacity(table_len + pool.len());
        for (_, cells) in &lists {
            words.extend(cells.iter().map(|&at| at + table_len as u32));
        }
        words.extend_from_slice(&pool);
        let edges: usize = lists.iter().map(|(c, _)| c.nb.len()).sum();
        let mut max_count = 0u32;
        let mut at = 0usize;
        while at < pool.len() {
            let w = pool[at];
            max_count = max_count.max((w & 0xffff) + (w >> 16));
            at += 1 + (w & 0xffff) as usize;
        }
        let buf = self.topo.ensure(&self.device, words.len() as u64 * 4);
        self.queue
            .write_buffer(buf, 0, bytemuck::cast_slice(&words));
        let avg_edges = edges as f64 / (n.max(1) * lists.len().max(1)) as f64;
        let base_hash = (0..n as u32)
            .into_par_iter()
            .map(mix)
            .reduce(|| 0, u32::wrapping_add);
        self.info = Some(TopoInfo {
            key: TopoKey {
                topo: topo.clone(),
                nb: job.nb,
                needs_nb,
                ltl,
            },
            max_count,
            avg_edges,
            base_hash,
        });
        Ok(())
    }

    /// Wait for the queue, then hand `f` the first `size` bytes of `buf`.
    fn read_mapped<T>(
        &self,
        buf: &wgpu::Buffer,
        size: u64,
        f: impl FnOnce(&[u8]) -> T,
    ) -> Result<T, String> {
        let slice = buf.slice(0..size);
        let (tx, rx) = std::sync::mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |r| {
            let _ = tx.send(r);
        });
        self.device
            .poll(wgpu::PollType::wait_indefinitely())
            .map_err(|e| format!("GPU poll: {e}"))?;
        rx.recv()
            .map_err(|e| e.to_string())?
            .map_err(|e| format!("GPU readback: {e}"))?;
        let out = {
            let view = slice.get_mapped_range().map_err(|e| format!("{e:?}"))?;
            f(&view)
        };
        buf.unmap();
        Ok(out)
    }

    fn poll(&self) -> Result<(), String> {
        self.device
            .poll(wgpu::PollType::wait_indefinitely())
            .map(|_| ())
            .map_err(|e| format!("GPU poll: {e}"))
    }

    /// Step every world of the job and measure it.
    pub fn run(&mut self, job: &BatchJob) -> Result<Vec<WorldTrace>, String> {
        self.run_until(job, &|| false)
    }

    /// [`run`](Self::run), giving the batch up between two submissions once `cut` says so (the
    /// error [`GIVEN_UP`]), so a stop or a search's end waits for one submission at most (about
    /// [`set_target_secs`](Self::set_target_secs)), not for the whole batch.
    pub fn run_until(
        &mut self,
        job: &BatchJob,
        cut: &dyn Fn() -> bool,
    ) -> Result<Vec<WorldTrace>, String> {
        let n = job.topo.len();
        let nw = job.worlds.len();
        if job.worlds.iter().any(|w| w.init.len() != n) {
            return Err("init has the wrong length".into());
        }
        let mut traces: Vec<WorldTrace> = job
            .worlds
            .iter()
            .map(|w| {
                let mut tr = WorldTrace::with_capacity(job.gens);
                let (p, h) = jetcarp_core::explore::batch::measure(&w.init);
                tr.pop.push(p);
                tr.changed.push(0);
                tr.hash.push(h);
                tr.neg
                    .push(jetcarp_core::explore::batch::count_neg(&w.init));
                if job.frames_at.first() == Some(&0) {
                    tr.frames.push(w.init.clone());
                }
                tr
            })
            .collect();
        if nw == 0 || job.gens == 0 {
            return Ok(traces);
        }
        if n == 0 {
            for tr in &mut traces {
                let frames = (job.frames_at.first() == Some(&0)) as usize;
                let frames = job.frames_at[frames..]
                    .iter()
                    .zip(1..)
                    .take_while(|&(&f, k)| f == k)
                    .count();
                tr.frames.extend(std::iter::repeat_n(Vec::new(), frames));
                for _ in 0..job.gens {
                    tr.pop.push(0);
                    tr.changed.push(0);
                    tr.hash.push(0);
                    tr.neg.push(0);
                }
            }
            return Ok(traces);
        }
        let stride = n.next_multiple_of(WG);
        let total = stride
            .checked_mul(nw)
            .filter(|&t| t <= self.max_cells)
            .ok_or_else(|| {
                format!(
                    "{nw} worlds of {n} cells exceed this GPU's limit ({} cells per batch)",
                    self.max_cells
                )
            })?;
        let mut ltl: Option<(Hood, u8)> = None;
        let mut needs_nb = false;
        for w in &job.worlds {
            match &w.rule {
                Rule::Ltl(l) => match ltl {
                    None => ltl = Some((l.hood, l.range)),
                    Some(k) if k != (l.hood, l.range) => {
                        return Err(
                            "Larger than Life rules in one batch must share hood and range".into(),
                        );
                    }
                    _ => {}
                },
                r => {
                    if !BatchSim::supports(r) {
                        return Err(format!("{} cannot run in a batch", r.notation()));
                    }
                    needs_nb = true;
                }
            }
        }
        self.bind_topology(job, needs_nb, ltl)?;
        let info = self.info.as_ref().unwrap();
        let (base_hash, avg_edges) = (info.base_hash, info.avg_edges);
        let words = ltl_words(info.max_count);

        // Rule parameters. CSR 0 is the job's neighbourhood when any world uses it, else the
        // Larger than Life reach; CSR 1 is the reach in a mixed batch.
        let mut tables: Vec<u32> = Vec::new();
        let ltl_csr = needs_nb as u32;
        let mut params: Vec<WorldParams> = Vec::with_capacity(nw);
        let mut edges = 0f64;
        let mut max_blast = 0u32;
        for w in &job.worlds {
            let mut p = world_params(&w.rule, &mut tables, words)
                .ok_or_else(|| format!("{} cannot run in a batch", w.rule.notation()))?;
            let mut passes = 1.0;
            if p.kind == KIND_LTL {
                p.d = ltl_csr;
            } else if p.kind == KIND_JACA && p.c > 0 {
                max_blast = max_blast.max(p.c);
                passes += p.c as f64 + 1.0;
            }
            edges += (avg_edges + 1.0) * n as f64 * passes;
            params.push(p);
        }
        let dev = &self.device;
        let buf = self
            .worlds
            .ensure(dev, (nw * size_of::<WorldParams>()) as u64);
        self.queue
            .write_buffer(buf, 0, bytemuck::cast_slice(&params));
        let buf = self.tables.ensure(dev, (tables.len().max(1) * 4) as u64);
        if !tables.is_empty() {
            self.queue
                .write_buffer(buf, 0, bytemuck::cast_slice(&tables));
        }

        // Dispatches are (gx, gy, worlds), so workgroup_id.z is the world.
        let groups = (stride / WG) as u32;
        let dim = self.device.limits().max_compute_workgroups_per_dimension;
        let gx = groups.min(dim);
        let gy = groups.div_ceil(gx);
        let cfg = Cfg {
            n: n as u32,
            stride: stride as u32,
            gx,
            worlds: nw as u32,
            live: job.topo.live_edges() as u32,
            pad: [0; 3],
        };
        self.queue
            .write_buffer(&self.cfg, 0, bytemuck::bytes_of(&cfg));

        // Worlds' starting states.
        let bytes = total as u64 * 4;
        self.state[0].ensure(&self.device, bytes);
        self.state[1].ensure(&self.device, bytes);
        if max_blast > 0 {
            self.zone[0].ensure(&self.device, bytes);
            self.zone[1].ensure(&self.device, bytes);
        }
        self.upload_init(job, stride)?;

        let dev = &self.device;
        let cap_gens = MAX_CHUNK_GENS.min((TRACE_BUDGET / (nw as u64 * TRACE_ENTRY)).max(1) as u32);
        let trace_bytes = cap_gens as u64 * nw as u64 * TRACE_ENTRY;
        self.trace.ensure(dev, trace_bytes);
        self.trace_read.ensure(dev, trace_bytes);

        // Bind groups for this run.
        let bg0 = dev.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("batch shared"),
            layout: &self.layout0,
            entries: &[
                entry(0, &self.cfg),
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                        buffer: &self.disp,
                        offset: 0,
                        size: wgpu::BufferSize::new(16),
                    }),
                },
                entry(2, self.topo.get()),
                entry(3, self.worlds.get()),
                entry(4, self.tables.get()),
                entry(5, self.trace.get()),
            ],
        });
        let state_bg = |a: usize| {
            dev.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("batch state"),
                layout: &self.layout1,
                entries: &[
                    entry(0, self.state[a].get()),
                    entry(1, self.state[1 - a].get()),
                ],
            })
        };
        let bg1 = [state_bg(0), state_bg(1)];
        let zone_buf = |a: usize| -> &wgpu::Buffer {
            if max_blast > 0 {
                self.zone[a].get()
            } else {
                &self.no_zone[a]
            }
        };
        let zone_bg = |a: usize| {
            dev.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("batch zone"),
                layout: &self.layout2,
                entries: &[entry(0, zone_buf(a)), entry(1, zone_buf(1 - a))],
            })
        };
        let bg2 = [zone_bg(0), zone_bg(1)];

        let mut cur = 0usize;
        let mut done = 0u32;
        let mut next_frame = usize::from(job.frames_at.first() == Some(&0));
        let slots_per = 1 + max_blast as usize;
        let mut per_gen = edges * self.secs_per_edge;
        let target = self.target_secs;
        self.stats = BatchStats::default();
        // The first submission uses the last batch's speed, which may not suit this one: start
        // small and let the pacing below grow it.
        let mut hint = ((target / per_gen.max(1e-7)) as u32).clamp(1, 16);
        let mut slot_bytes: Vec<u8> = Vec::new();
        while done < job.gens {
            if cut() {
                return Err(GIVEN_UP.into());
            }
            let mut gens = (job.gens - done).min(hint).min(cap_gens);
            if let Some(&f) = job.frames_at.get(next_frame)
                && f > done
            {
                gens = gens.min(f - done);
            }
            // A generation too heavy for one submission runs as several ranges of worlds.
            let by_time = (per_gen / target).ceil() as usize;
            let slices = by_time
                .max((nw as u32).div_ceil(dim) as usize)
                .clamp(1, nw.min(MAX_SLOTS / slots_per));
            if slices > 1 {
                gens = 1;
            }
            gens = gens.min(((MAX_SLOTS / (slots_per * slices)) as u32).max(1));
            let world_range = |s: usize| -> (u32, u32) {
                let a = (s * nw / slices) as u32;
                let b = ((s + 1) * nw / slices) as u32;
                (a, b - a)
            };

            // One dynamic uniform slot per pass.
            slot_bytes.clear();
            slot_bytes.resize(gens as usize * slices * slots_per * SLOT_BYTES as usize, 0);
            for g in 0..gens as usize {
                for s in 0..slices {
                    for p in 0..slots_per {
                        let at = ((g * slices + s) * slots_per + p) * SLOT_BYTES as usize;
                        let e = [
                            g as u32,
                            world_range(s).0,
                            p.wrapping_sub(1) as u32,
                            done + g as u32 + 1,
                        ];
                        slot_bytes[at..at + 16].copy_from_slice(bytemuck::cast_slice(&e));
                    }
                }
            }
            self.queue.write_buffer(&self.disp, 0, &slot_bytes);

            let chunk_bytes = gens as u64 * nw as u64 * TRACE_ENTRY;
            let start = Instant::now();
            let encode_slice = |pass: &mut wgpu::ComputePass, g: usize, s: usize, cur: usize| {
                let (_, count) = world_range(s);
                let slot = (g * slices + s) * slots_per;
                let off = |p: usize| ((slot + p) as u64 * SLOT_BYTES) as u32;
                if max_blast > 0 {
                    pass.set_pipeline(&self.contact);
                    pass.set_bind_group(0, &bg0, &[off(0)]);
                    pass.set_bind_group(1, &bg1[cur], &[]);
                    pass.set_bind_group(2, &bg2[1], &[]);
                    pass.dispatch_workgroups(gx, gy, count);
                    pass.set_pipeline(&self.dilate);
                    for p in 0..max_blast as usize {
                        pass.set_bind_group(0, &bg0, &[off(p + 1)]);
                        pass.set_bind_group(2, &bg2[p % 2], &[]);
                        pass.dispatch_workgroups(gx, gy, count);
                    }
                }
                pass.set_pipeline(&self.step);
                pass.set_bind_group(0, &bg0, &[off(0)]);
                pass.set_bind_group(1, &bg1[cur], &[]);
                pass.set_bind_group(2, &bg2[max_blast as usize % 2], &[]);
                pass.dispatch_workgroups(gx, gy, count);
            };

            let clear = |enc: &mut wgpu::CommandEncoder| {
                enc.clear_buffer(self.trace.get(), 0, Some(chunk_bytes));
            };
            if slices == 1 {
                let mut enc = self.device.create_command_encoder(&Default::default());
                clear(&mut enc);
                {
                    let mut pass = enc.begin_compute_pass(&Default::default());
                    for g in 0..gens as usize {
                        encode_slice(&mut pass, g, 0, cur);
                        cur ^= 1;
                    }
                }
                enc.copy_buffer_to_buffer(
                    self.trace.get(),
                    0,
                    self.trace_read.get(),
                    0,
                    chunk_bytes,
                );
                self.queue.submit([enc.finish()]);
                self.stats.submissions += 1;
            } else {
                // One submission per range, with a wait in between.
                for s in 0..slices {
                    // The ranges before were waited for, so nothing is left on the device.
                    if s > 0 && cut() {
                        return Err(GIVEN_UP.into());
                    }
                    let mut enc = self.device.create_command_encoder(&Default::default());
                    if s == 0 {
                        clear(&mut enc);
                    }
                    {
                        let mut pass = enc.begin_compute_pass(&Default::default());
                        encode_slice(&mut pass, 0, s, cur);
                    }
                    if s + 1 == slices {
                        enc.copy_buffer_to_buffer(
                            self.trace.get(),
                            0,
                            self.trace_read.get(),
                            0,
                            chunk_bytes,
                        );
                    }
                    let sub = Instant::now();
                    self.queue.submit([enc.finish()]);
                    self.stats.submissions += 1;
                    if s + 1 < slices {
                        self.poll()?;
                        self.stats.longest_secs =
                            self.stats.longest_secs.max(sub.elapsed().as_secs_f64());
                    }
                }
                cur ^= 1;
            }
            let trace_read = self.trace_read.get();
            self.read_mapped(trace_read, chunk_bytes, |bytes| {
                let words: &[u32] = bytemuck::cast_slice(bytes);
                for g in 0..gens as usize {
                    for (w, tr) in traces.iter_mut().enumerate() {
                        let at = (g * nw + w) * 4;
                        tr.pop.push(words[at]);
                        tr.changed.push(words[at + 1]);
                        tr.hash.push(words[at + 2].wrapping_add(base_hash));
                        tr.neg.push(words[at + 3]);
                    }
                }
            })?;

            // Pacing: aim for the target per submission, remember the speed for the next batch.
            let elapsed = start.elapsed().as_secs_f64();
            if slices == 1 {
                self.stats.longest_secs = self.stats.longest_secs.max(elapsed);
            }
            let per = elapsed / gens as f64;
            per_gen = per;
            self.secs_per_edge = per / edges.max(1.0);
            let want = (target / per.max(1e-7)) as u32;
            hint = want.clamp(1, MAX_CHUNK_GENS).min(hint.saturating_mul(4));
            done += gens;

            if job.frames_at.get(next_frame) == Some(&done) {
                next_frame += 1;
                self.copy_frame(job, stride, cur, &mut traces)?;
            }
        }
        Ok(traces)
    }

    /// Fill the first state buffer from the jobs' starting states, in bounded submissions.
    fn upload_init(&mut self, job: &BatchJob, stride: usize) -> Result<(), String> {
        let buf = self.state[0].get();
        let per_world = stride * 4;
        // Worlds per submission; a world larger than the chunk size goes alone.
        let per_submit = (UPLOAD_BYTES / per_world).max(1);
        let mut words: Vec<u32> = Vec::new();
        for (c, part) in job.worlds.chunks(per_submit).enumerate() {
            let at = (c * per_submit * per_world) as u64;
            words.clear();
            words.resize(part.len() * stride, 0);
            words
                .par_chunks_mut(stride)
                .zip(part.par_iter())
                .for_each(|(out, w)| {
                    for (x, &s) in out.iter_mut().zip(&w.init) {
                        *x = s as u32;
                    }
                });
            self.queue
                .write_buffer(buf, at, bytemuck::cast_slice(&words));
            self.queue.submit([]);
            self.poll()?;
        }
        Ok(())
    }

    fn copy_frame(
        &mut self,
        job: &BatchJob,
        stride: usize,
        cur: usize,
        traces: &mut [WorldTrace],
    ) -> Result<(), String> {
        let n = job.topo.len();
        let bytes = (stride * job.worlds.len() * 4) as u64;
        self.frame_read.ensure(&self.device, bytes);
        let mut enc = self.device.create_command_encoder(&Default::default());
        enc.copy_buffer_to_buffer(self.state[cur].get(), 0, self.frame_read.get(), 0, bytes);
        self.queue.submit([enc.finish()]);
        let buf = self.frame_read.get();
        self.read_mapped(buf, bytes, |view| {
            traces.par_iter_mut().enumerate().for_each(|(w, tr)| {
                let at = w * stride * 4;
                let frame: Vec<u8> = view[at..at + n * 4]
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .map(|c| c[0])
                    .collect();
                tr.frames.push(frame);
            });
        })
    }
}

/// The explore search's GPU loop runs its batches here; a lost device is replaced by a new one.
impl Backend for BatchSim {
    fn name(&self) -> String {
        BatchSim::name(self).to_string()
    }
    fn supports(&self, rule: &Rule) -> bool {
        BatchSim::supports(rule)
    }
    fn max_cells(&self) -> usize {
        BatchSim::max_cells(self)
    }
    fn run(&mut self, job: &BatchJob) -> Result<Vec<WorldTrace>, String> {
        BatchSim::run(self, job)
    }
    fn run_until(
        &mut self,
        job: &BatchJob,
        cut: &dyn Fn() -> bool,
    ) -> Result<Vec<WorldTrace>, String> {
        BatchSim::run_until(self, job, cut)
    }
    fn reset(&mut self) -> bool {
        match BatchSim::headless() {
            Ok(s) => {
                *self = s;
                true
            }
            Err(_) => false,
        }
    }
}
