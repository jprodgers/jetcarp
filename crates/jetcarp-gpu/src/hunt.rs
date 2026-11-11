//! GPU batches for the soup hunter (`jetcarp_core::hunt`): many soups per dispatch.
//!
//! Each workgroup runs one soup inside its **box**: the soup's region (its box plus margin, as in
//! `jetcarp_core::hunt::soup`) widened by two generations' reach, held as bit planes (32 cells of a
//! row per word, one plane per tile type, plus Generations decay planes) in workgroup memory. The
//! shader is generated per rule and tiling with the neighbour offsets and the rule baked in, and
//! steps exactly as the CPU's bit engine: the same bit-sliced counts, the same rule terms, and
//! the same state hash (the wrapping sum of `chunk_hash` over every non-zero 32-cell chunk). After
//! every generation it applies the CPU's prefix rules in the same order: died, exploded, a cell
//! outside the region (hand over to the CPU), settled (hash and population equal `p`
//! generations ago), generation limit. So a soup's prefix on the GPU equals
//! `jetcarp_core::hunt::soup::prefix_cpu`, and the hunt's results equal a CPU-only hunt
//! (`tests/hunt.rs` checks both).
//!
//! Soups that leave the region or run long go back to the CPU, which finishes them with ship
//! cutting and the census.

use jetcarp_core::hunt::bits::Universe;
use jetcarp_core::hunt::separate::Census;
use jetcarp_core::hunt::soup::SoupBatcher;
use jetcarp_core::hunt::soup::{End, Limits, Prefix, Ring};
use jetcarp_core::objects::LCell;
use rayon::prelude::*;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// Threads per workgroup.
const TPB: u32 = 128;
/// Generations per dispatch (keeps each submission short).
const GENS_PER_DISPATCH: u32 = 512;
/// Generations the GPU runs a soup at most; longer soups continue on the CPU.
const GPU_MAX_GENS: u64 = 16_384;
/// Soups per submission at most.
const CHUNK: usize = 16_384;

/// Box geometry for one group.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct Geo {
    /// Absolute column of bit 0 of word 0 (a multiple of 32) and row of row 0.
    bi0: i32,
    bj0: i32,
    w: u32,
    h: u32,
    /// Region rows in box coordinates.
    rr0: u32,
    rr1: u32,
    /// Per word: the bits inside the region's columns.
    inmask: Vec<u32>,
}

impl Geo {
    fn new(ri: i32, rj: i32, region: [i32; 4]) -> Geo {
        let lo_i = region[0] - 2 * ri;
        let hi_i = region[1] + 2 * ri;
        let f0 = lo_i.div_euclid(32) * 32;
        let f1 = hi_i.div_euclid(32) * 32;
        let bi0 = f0 - 32;
        let w = ((f1 - f0) / 32 + 3) as u32;
        let bj0 = region[2] - 2 * rj;
        let h = (region[3] - region[2] + 1 + 4 * rj) as u32;
        let inmask = (0..w)
            .map(|x| {
                let mut m = 0u32;
                for b in 0..32 {
                    let i = bi0 + 32 * x as i32 + b;
                    if i >= region[0] && i <= region[1] {
                        m |= 1 << b;
                    }
                }
                m
            })
            .collect();
        Geo {
            bi0,
            bj0,
            w,
            h,
            rr0: (region[2] - bj0) as u32,
            rr1: (region[3] - bj0) as u32,
            inmask,
        }
    }
}

struct Pipe {
    pipeline: wgpu::ComputePipeline,
    size: u32,
}

/// The GPU soup batcher.
pub struct GpuHunter {
    device: wgpu::Device,
    queue: wgpu::Queue,
    name: String,
    /// Workgroup memory available (bytes).
    wg_bytes: u32,
    layout: wgpu::BindGroupLayout,
    pipes: Mutex<HashMap<String, Arc<Pipe>>>,
    /// One submission at a time.
    busy: Mutex<()>,
}

#[repr(C)]
#[derive(Clone, Copy, Default, bytemuck::Pod, bytemuck::Zeroable)]
struct Params {
    max_gens: u32,
    cap: u32,
    gens: u32,
    c0: i32,
    j0: i32,
    n_soups: u32,
    pad0: u32,
    pad1: u32,
}

impl GpuHunter {
    /// Open the default high-performance adapter without a window.
    pub fn headless() -> Result<GpuHunter, String> {
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
        let al = adapter.limits();
        let limits = wgpu::Limits {
            max_storage_buffer_binding_size: al.max_storage_buffer_binding_size,
            max_buffer_size: al.max_buffer_size,
            max_compute_workgroup_storage_size: al.max_compute_workgroup_storage_size,
            ..Default::default()
        };
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("jetcarp-gpu hunter"),
            required_features: wgpu::Features::empty(),
            required_limits: limits,
            experimental_features: Default::default(),
            memory_hints: wgpu::MemoryHints::Performance,
            trace: Default::default(),
        }))
        .map_err(|e| format!("GPU device: {e}"))?;
        Ok(GpuHunter::new(device, queue, crate::describe(&info)))
    }

    /// Use an existing device.
    pub fn new(device: wgpu::Device, queue: wgpu::Queue, name: String) -> GpuHunter {
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
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("jetcarp-gpu hunter"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                storage(1, false),
                storage(2, false),
                storage(3, false),
                storage(4, true),
            ],
        });
        let wg_bytes = device.limits().max_compute_workgroup_storage_size;
        GpuHunter {
            device,
            queue,
            name,
            wg_bytes,
            layout,
            pipes: Mutex::new(HashMap::new()),
            busy: Mutex::new(()),
        }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    fn fits(&self, census: &Census, geo: &Geo) -> bool {
        let p = &census.plan;
        let size = p.ntypes as u64 * p.planes() as u64 * geo.h as u64 * geo.w as u64;
        let ring = census_ring(census) as u64;
        p.reach_i < 32 && (2 * size + 3 * ring + 16) * 4 <= self.wg_bytes as u64
    }

    fn pipe(&self, census: &Census, geo: &Geo, ring: u32) -> Arc<Pipe> {
        let src = shader(census, geo, ring);
        let mut m = self.pipes.lock().unwrap();
        if let Some(p) = m.get(&src) {
            return p.clone();
        }
        let module = self
            .device
            .create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("jetcarp-gpu hunter"),
                source: wgpu::ShaderSource::Wgsl(src.clone().into()),
            });
        let pl = self
            .device
            .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("jetcarp-gpu hunter"),
                bind_group_layouts: &[Some(&self.layout)],
                immediate_size: 0,
            });
        let pipeline = self
            .device
            .create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some("jetcarp-gpu hunter"),
                layout: Some(&pl),
                module: &module,
                entry_point: Some("main"),
                compilation_options: Default::default(),
                cache: None,
            });
        let p = &census.plan;
        let pipe = Arc::new(Pipe {
            pipeline,
            size: p.ntypes as u32 * p.planes() as u32 * geo.h * geo.w,
        });
        if m.len() > 64 {
            m.clear();
        }
        m.insert(src, pipe.clone());
        pipe
    }

    fn run_chunk(
        &self,
        census: &Census,
        geo: &Geo,
        limits: Limits,
        soups: &[Vec<LCell>],
    ) -> Result<Vec<Prefix>, String> {
        let ring_len = limits.settle_period as u32 + 1;
        let pipe = self.pipe(census, geo, ring_len);
        let plan = &census.plan;
        let np = plan.planes();
        let (w, h) = (geo.w as usize, geo.h as usize);
        let size = pipe.size as usize;
        let n = soups.len();
        // Initial planes, rings, and control words (empty soups are done at generation 0).
        let packed: Vec<(Vec<u32>, Vec<u32>, [u32; 4])> = soups
            .par_iter()
            .map(|cells| {
                let mut st = vec![0u32; size];
                for c in cells {
                    let r = (c.j - geo.bj0) as usize;
                    let b = (c.i - geo.bi0) as usize;
                    let x = ((c.k as usize * np) * h + r) * w + b / 32;
                    st[x] |= 1 << (b % 32);
                }
                let u = Universe::with_cells(plan.clone(), cells);
                let mut ring = Ring::new(limits.settle_period);
                ring.put(0, u.hash(), u.population());
                let mut rw = vec![0u32; 3 * ring_len as usize];
                for x in 0..ring_len as usize {
                    rw[3 * x] = (ring.hashes[x] >> 32) as u32;
                    rw[3 * x + 1] = ring.hashes[x] as u32;
                    rw[3 * x + 2] = ring.pops[x].min(u32::MAX as u64) as u32;
                }
                let ctl = if u.is_empty() { [1, 0, 0, 0] } else { [0; 4] };
                (st, rw, ctl)
            })
            .collect();
        let mut state = Vec::with_capacity(n * size);
        let mut rings = Vec::with_capacity(n * 3 * ring_len as usize);
        let mut ctl = Vec::with_capacity(n * 4);
        for (s, r, c) in &packed {
            state.extend_from_slice(s);
            rings.extend_from_slice(r);
            ctl.extend_from_slice(c);
        }
        drop(packed);
        use wgpu::util::DeviceExt;
        let usage = wgpu::BufferUsages::STORAGE
            | wgpu::BufferUsages::COPY_SRC
            | wgpu::BufferUsages::COPY_DST;
        let mk = |label, data: &[u32], usage| {
            self.device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some(label),
                    contents: bytemuck::cast_slice(data),
                    usage,
                })
        };
        let b_state = mk("hunt state", &state, usage);
        let b_ring = mk("hunt ring", &rings, usage);
        let b_ctl = mk("hunt ctl", &ctl, usage);
        let b_mask = mk("hunt mask", &geo.inmask, wgpu::BufferUsages::STORAGE);
        let prm = Params {
            max_gens: limits.max_gens.min(u32::MAX as u64 - 1) as u32,
            cap: limits.pop_cap.min(u32::MAX as u64 - 1) as u32,
            gens: GENS_PER_DISPATCH,
            c0: geo.bi0 / 32,
            j0: geo.bj0,
            n_soups: n as u32,
            pad0: 0,
            pad1: 0,
        };
        let b_prm = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("hunt params"),
                contents: bytemuck::bytes_of(&prm),
                usage: wgpu::BufferUsages::UNIFORM,
            });
        let bind = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("jetcarp-gpu hunter"),
            layout: &self.layout,
            entries: &[
                crate::entry(0, &b_prm),
                crate::entry(1, &b_state),
                crate::entry(2, &b_ring),
                crate::entry(3, &b_ctl),
                crate::entry(4, &b_mask),
            ],
        });
        let gx = (n as u32).min(65535);
        let gy = (n as u32).div_ceil(65535);
        let mut ran = 0u64;
        let ctl_bytes = (n * 16) as u64;
        let read_ctl = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("hunt ctl read"),
            size: ctl_bytes,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let mut ctl_now: Vec<u32>;
        loop {
            let mut enc = self
                .device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("jetcarp-gpu hunter"),
                });
            {
                let mut pass = enc.begin_compute_pass(&wgpu::ComputePassDescriptor {
                    label: Some("jetcarp-gpu hunter"),
                    timestamp_writes: None,
                });
                pass.set_pipeline(&pipe.pipeline);
                pass.set_bind_group(0, &bind, &[]);
                pass.dispatch_workgroups(gx, gy, 1);
            }
            enc.copy_buffer_to_buffer(&b_ctl, 0, &read_ctl, 0, ctl_bytes);
            self.queue.submit([enc.finish()]);
            ctl_now = map_read(&self.device, &read_ctl, ctl_bytes)?;
            ran += GENS_PER_DISPATCH as u64;
            let running = ctl_now.chunks(4).any(|c| c[0] == 0);
            if !running || ran >= GPU_MAX_GENS {
                break;
            }
        }
        let state: Vec<u32> = self.read(&b_state, (n * size * 4) as u64)?;
        let rings: Vec<u32> = self.read(&b_ring, (n * 12 * ring_len as usize) as u64)?;
        let maxs = plan.states - 1;
        let out = (0..n)
            .into_par_iter()
            .map(|s| {
                let st = &state[s * size..(s + 1) * size];
                let mut cells = Vec::new();
                for k in 0..plan.ntypes {
                    for r in 0..h {
                        for x in 0..w {
                            let at = |p: usize| st[((k * np + p) * h + r) * w + x];
                            let mut occ = 0u32;
                            for p in 0..np {
                                occ |= at(p);
                            }
                            while occ != 0 {
                                let b = occ.trailing_zeros();
                                occ &= occ - 1;
                                let s1 = at(0) >> b & 1 == 1;
                                let v = if s1 {
                                    1
                                } else {
                                    let mut v = 0u8;
                                    for p in 1..np {
                                        v |= ((at(p) >> b & 1) as u8) << (p - 1);
                                    }
                                    v.min(maxs)
                                };
                                cells.push(LCell {
                                    i: geo.bi0 + (32 * x as u32 + b) as i32,
                                    j: geo.bj0 + r as i32,
                                    k: k as u32,
                                    s: v,
                                });
                            }
                        }
                    }
                }
                cells.sort_unstable();
                let rw = &rings[s * 3 * ring_len as usize..(s + 1) * 3 * ring_len as usize];
                let ring = Ring {
                    len: ring_len as usize,
                    hashes: (0..ring_len as usize)
                        .map(|x| ((rw[3 * x] as u64) << 32) | rw[3 * x + 1] as u64)
                        .collect(),
                    pops: (0..ring_len as usize)
                        .map(|x| {
                            let p = rw[3 * x + 2];
                            if p == u32::MAX { u64::MAX } else { p as u64 }
                        })
                        .collect(),
                };
                let c = &ctl_now[4 * s..4 * s + 4];
                let end = match c[0] {
                    1 => Some((End::Died, 0)),
                    2 => Some((End::Exploded, 0)),
                    3 => Some((End::Settled, c[2] as u64)),
                    4 => Some((End::Unsettled, 0)),
                    _ => None,
                };
                Prefix {
                    cells,
                    generation: c[1] as u64,
                    ring,
                    end,
                    pending: c[0] == 5,
                }
            })
            .collect();
        Ok(out)
    }

    fn read(&self, buf: &wgpu::Buffer, bytes: u64) -> Result<Vec<u32>, String> {
        let dst = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("hunt read"),
            size: bytes.max(4),
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let mut enc = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("jetcarp-gpu hunter read"),
            });
        enc.copy_buffer_to_buffer(buf, 0, &dst, 0, bytes);
        self.queue.submit([enc.finish()]);
        map_read(&self.device, &dst, bytes)
    }
}

fn census_ring(census: &Census) -> u32 {
    // Ring::new(settle_period) has settle_period + 1 entries; the hunt's settle period is its
    // max_period (see `HuntSpec::limits`), passed again with the limits.
    census.max_period.clamp(1, 1024) as u32 + 1
}

fn map_read(device: &wgpu::Device, buf: &wgpu::Buffer, bytes: u64) -> Result<Vec<u32>, String> {
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
        bytemuck::cast_slice::<u8, u32>(&view).to_vec()
    };
    buf.unmap();
    Ok(out)
}

impl SoupBatcher for GpuHunter {
    fn name(&self) -> String {
        self.name.clone()
    }
    fn supports(&self, census: &Census, region: [i32; 4]) -> bool {
        let p = &census.plan;
        self.fits(census, &Geo::new(p.reach_i, p.reach_j, region))
    }
    fn prefix(
        &self,
        census: &Census,
        region: [i32; 4],
        limits: Limits,
        soups: &[Vec<LCell>],
    ) -> Result<Vec<Prefix>, String> {
        let p = &census.plan;
        let geo = Geo::new(p.reach_i, p.reach_j, region);
        if !self.fits(census, &geo) {
            return Err("this group does not fit in GPU workgroup memory".into());
        }
        let _g = self.busy.lock().unwrap();
        let mut out = Vec::with_capacity(soups.len());
        for chunk in soups.chunks(CHUNK) {
            out.extend(self.run_chunk(census, &geo, limits, chunk)?);
        }
        Ok(out)
    }
}

// ---------------------------------------------------------------------------------------------
// Shader generation

/// `r + dj` as a u32 expression.
fn row(dj: i32) -> String {
    match dj {
        0 => "r".into(),
        d if d > 0 => format!("(r + {d}u)"),
        d => format!("(r - {}u)", -d),
    }
}

/// The boolean function of the count bits that is true for counts in `mask`.
fn terms(mask: u32, max_degree: usize, cb: usize) -> String {
    let ins: Vec<u32> = (0..=max_degree as u32)
        .filter(|n| mask >> n & 1 == 1)
        .collect();
    let outs: Vec<u32> = (0..=max_degree as u32)
        .filter(|n| mask >> n & 1 == 0)
        .collect();
    let (invert, ns) = if outs.len() < ins.len() {
        (true, outs)
    } else {
        (false, ins)
    };
    let eq = |n: u32| -> String {
        (0..cb)
            .map(|b| {
                if n >> b & 1 == 1 {
                    format!("c{b}")
                } else {
                    format!("(~c{b})")
                }
            })
            .collect::<Vec<_>>()
            .join(" & ")
    };
    let or = if ns.is_empty() {
        "0u".to_string()
    } else {
        ns.iter()
            .map(|&n| format!("({})", eq(n)))
            .collect::<Vec<_>>()
            .join(" | ")
    };
    if invert { format!("(~({or}))") } else { or }
}

fn shader(census: &Census, geo: &Geo, ring: u32) -> String {
    let p = &census.plan;
    let nt = p.ntypes;
    let np = p.planes();
    let (w, h) = (geo.w as usize, geo.h as usize);
    let size = nt * np * h * w;
    let cb = p.cbits;
    let db = p.dbits;
    let rj = p.reach_j as usize;
    let states = p.states as u32;
    let mut body = String::new();
    for k in 0..nt {
        body += "    {\n";
        for b in 0..cb {
            body += &format!("      var c{b} = 0u;\n");
        }
        body += "      var cy = 0u; var tt = 0u;\n";
        for &(di, dj, k2) in &p.offs[k] {
            let base = format!("cur + {}u + {} * {w}u", (k2 as usize * np) * h * w, row(dj));
            let x = if di == 0 {
                format!("m[{base} + x]")
            } else if di > 0 {
                format!(
                    "((m[{base} + x] >> {di}u) | (m[{base} + x + 1u] << {}u))",
                    32 - di
                )
            } else {
                format!(
                    "((m[{base} + x] << {}u) | (m[{base} + x - 1u] >> {}u))",
                    -di,
                    32 + di
                )
            };
            body += &format!("      cy = {x};\n");
            for b in 0..cb {
                body += &format!("      tt = c{b} & cy; c{b} = c{b} ^ cy; cy = tt;\n");
            }
        }
        body += &format!("      let bm = {};\n", terms(p.birth, p.max_degree, cb));
        body += &format!("      let sm = {};\n", terms(p.survive, p.max_degree, cb));
        let ia = |pl: usize| format!("{}u + r * {w}u + x", (k * np + pl) * h * w);
        body += &format!("      let a = m[cur + {}];\n", ia(0));
        if db == 0 {
            body += "      let na = (~a & bm) | (a & sm);\n";
            body += &format!("      m[nxt + {}] = na;\n", ia(0));
            body += &format!("      if (na != 0u) {{ hsh({}u, r, x, na); }}\n", k * np);
            body += "      let o = na;\n";
        } else {
            let mut nz = Vec::new();
            for q in 0..db {
                body += &format!("      var v{q} = m[cur + {}];\n", ia(1 + q));
                nz.push(format!("v{q}"));
            }
            body += &format!("      let nz = {};\n", nz.join(" | "));
            body += "      let empty = ~(a | nz);\n";
            body += "      let na = (empty & bm) | (a & sm);\n";
            body += "      let dying = a & ~sm;\n";
            body += "      var carry = nz;\n";
            for q in 0..db {
                body += &format!("      tt = v{q} & carry; v{q} = v{q} ^ carry; carry = tt;\n");
            }
            if states >> db == 0 {
                let eq: Vec<String> = (0..db)
                    .map(|q| {
                        if states >> q & 1 == 1 {
                            format!("v{q}")
                        } else {
                            format!("(~v{q})")
                        }
                    })
                    .collect();
                body += &format!("      let eq = {};\n", eq.join(" & "));
                for q in 0..db {
                    body += &format!("      v{q} = v{q} & ~eq;\n");
                }
            }
            body += "      v1 = v1 | dying;\n";
            body += &format!("      m[nxt + {}] = na;\n", ia(0));
            body += &format!("      if (na != 0u) {{ hsh({}u, r, x, na); }}\n", k * np);
            let mut o = vec!["na".to_string()];
            for q in 0..db {
                body += &format!("      m[nxt + {}] = v{q};\n", ia(1 + q));
                body += &format!(
                    "      if (v{q} != 0u) {{ hsh({}u, r, x, v{q}); }}\n",
                    k * np + 1 + q
                );
                o.push(format!("v{q}"));
            }
            body += &format!("      let o = {};\n", o.join(" | "));
        }
        body += "      lpop = lpop + countOneBits(o);\n";
        body += "      occ = occ | o;\n";
        body += "    }\n";
    }
    format!(
        r#"// Generated by jetcarp-gpu hunt.rs: one soup per workgroup, bit planes in workgroup memory.
struct Params {{ max_gens: u32, cap: u32, gens: u32, c0: i32, j0: i32, n_soups: u32, pad0: u32, pad1: u32 }};
@group(0) @binding(0) var<uniform> prm: Params;
@group(0) @binding(1) var<storage, read_write> state: array<u32>;
@group(0) @binding(2) var<storage, read_write> rings: array<u32>;
@group(0) @binding(3) var<storage, read_write> ctl: array<u32>;
@group(0) @binding(4) var<storage, read> inmask: array<u32>;

const W: u32 = {w}u;
const H: u32 = {h}u;
const SIZE: u32 = {size}u;
const RING: u32 = {ring}u;
const RJ: u32 = {rj}u;
const RR0: u32 = {rr0}u;
const RR1: u32 = {rr1}u;

var<workgroup> m: array<u32, {m2}>;
var<workgroup> rh: array<u32, {r3}>;
var<workgroup> acc: array<atomic<u32>, 4>;
var<workgroup> flag: u32;

var<private> lha: u32;
var<private> lhb: u32;

fn mix32(x0: u32) -> u32 {{
  var x = x0;
  x = x ^ (x >> 16u); x = x * 0x7feb352du; x = x ^ (x >> 15u); x = x * 0x846ca68bu; x = x ^ (x >> 16u);
  return x;
}}

fn hsh(kp: u32, r: u32, x: u32, v: u32) {{
  let j = bitcast<u32>(prm.j0 + i32(r));
  let c = bitcast<u32>(prm.c0 + i32(x));
  let pos = (j * 0x9e3779b1u) ^ (c * 0x85ebca77u) ^ (kp * 0xc2b2ae3du);
  let a = (v ^ pos) * 0x27d4eb2fu;
  let b = (((v << 16u) | (v >> 16u)) ^ pos ^ 0x632be5abu) * 0x165667b1u;
  lha = lha + (a ^ (a >> 15u));
  lhb = lhb + (b ^ (b >> 13u));
}}

@compute @workgroup_size({tpb})
fn main(@builtin(workgroup_id) wid: vec3<u32>, @builtin(local_invocation_index) li: u32) {{
  let s = wid.x + wid.y * 65535u;
  if (li == 0u) {{
    var st = 1u;
    if (s < prm.n_soups) {{ st = ctl[s * 4u]; }}
    flag = st;
    for (var q = 0u; q < 4u; q = q + 1u) {{ atomicStore(&acc[q], 0u); }}
  }}
  if (workgroupUniformLoad(&flag) != 0u) {{ return; }}
  let base = s * SIZE;
  for (var q = li; q < SIZE; q = q + {tpb}u) {{
    m[q] = state[base + q];
    m[SIZE + q] = 0u;
  }}
  for (var q = li; q < RING * 3u; q = q + {tpb}u) {{ rh[q] = rings[s * RING * 3u + q]; }}
  var gen = ctl[s * 4u + 1u];
  var cur = 0u;
  workgroupBarrier();
  for (var it = 0u; it < prm.gens; it = it + 1u) {{
    let nxt = SIZE - cur;
    lha = 0u; lhb = 0u;
    var lpop = 0u;
    var lout = 0u;
    for (var q = li; q < H * W; q = q + {tpb}u) {{
      let r = q / W;
      let x = q % W;
      if (x == 0u || x == W - 1u || r < RJ || r >= H - RJ) {{ continue; }}
      var occ = 0u;
{body}
      if (r < RR0 || r > RR1) {{ lout = lout | occ; }} else {{ lout = lout | (occ & ~inmask[x]); }}
    }}
    if (lpop != 0u) {{ atomicAdd(&acc[0], lpop); }}
    if (lha != 0u) {{ atomicAdd(&acc[1], lha); }}
    if (lhb != 0u) {{ atomicAdd(&acc[2], lhb); }}
    if (lout != 0u) {{ atomicOr(&acc[3], 1u); }}
    workgroupBarrier();
    gen = gen + 1u;
    if (li == 0u) {{
      let pop = atomicLoad(&acc[0]);
      let ha = atomicLoad(&acc[1]);
      let hb = atomicLoad(&acc[2]);
      let out = atomicLoad(&acc[3]);
      var st = 0u;
      if (pop == 0u) {{ st = 1u; }}
      else if (pop > prm.cap) {{ st = 2u; }}
      else if (out != 0u) {{ st = 5u; }}
      else {{
        let lim = min(RING, gen + 1u);
        for (var p = 1u; p < lim; p = p + 1u) {{
          let y = ((gen - p) % RING) * 3u;
          if (rh[y] == ha && rh[y + 1u] == hb && rh[y + 2u] == pop) {{
            st = 3u;
            ctl[s * 4u + 2u] = p;
            break;
          }}
        }}
        if (st == 0u) {{
          let y = (gen % RING) * 3u;
          rh[y] = ha; rh[y + 1u] = hb; rh[y + 2u] = pop;
          if (gen >= prm.max_gens) {{ st = 4u; }}
        }}
      }}
      for (var q = 0u; q < 4u; q = q + 1u) {{ atomicStore(&acc[q], 0u); }}
      flag = st;
    }}
    cur = nxt;
    if (workgroupUniformLoad(&flag) != 0u) {{ break; }}
  }}
  workgroupBarrier();
  for (var q = li; q < SIZE; q = q + {tpb}u) {{ state[base + q] = m[cur + q]; }}
  for (var q = li; q < RING * 3u; q = q + {tpb}u) {{ rings[s * RING * 3u + q] = rh[q]; }}
  if (li == 0u) {{
    ctl[s * 4u] = flag;
    ctl[s * 4u + 1u] = gen;
  }}
}}
"#,
        rr0 = geo.rr0,
        rr1 = geo.rr1,
        m2 = 2 * size,
        r3 = 3 * ring,
        tpb = TPB,
        body = body,
    )
}
