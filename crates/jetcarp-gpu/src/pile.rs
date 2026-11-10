//! Large sandpiles on the GPU: [`PileGpu`] topples a `jetcarp_core::pile` lattice board with the
//! same synchronous sweeps as the CPU, so the pile, the toppling count, and the sweep count are
//! identical (tested).
//!
//! The board's bytes are uploaded as they are (four heights to a word, padding included). Sweeps
//! run in batches; each batch first copies the board to a snapshot. If any grain comes within
//! reach of the box edge during a batch, the batch is undone from the snapshot, the board grows
//! on the CPU, and the batch runs again, so no grain is ever lost.

use crate::{check_adapter, describe, map_read};
use jetcarp_core::pile::lattice::{LatticeBoard, TILE};
use jetcarp_core::pile::{BoardRef, Ctl, StabStats, Tick, Toppler};
use std::time::Instant;

#[repr(C)]
#[derive(Clone, Copy, Default, bytemuck::Pod, bytemuck::Zeroable)]
struct Params {
    wr: u32,
    ph: u32,
    lw: u32,
    lh: u32,
    nt: u32,
    slot: u32,
    band: u32,
    ystride: u32,
    x0: u32,
    rw: u32,
    j0: u32,
    rh: u32,
    nx0: u32,
    nx1: u32,
    nj0: u32,
    nj1: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Default, bytemuck::Pod, bytemuck::Zeroable)]
struct Off {
    q: i32,
    s: u32,
    dj: i32,
    k: u32,
}

const STRIDE: usize = 256;
/// Sweeps per batch at most (parameter slots in a 64 KB uniform buffer).
const MAX_BATCH: usize = 240;
const YSTRIDE: u32 = 32768;
/// Stats words before the per-sweep counts: edge flag, nonzero box (words, rows).
const STATS_HEAD: usize = 5;

/// The part of the box swept: logical words `x0..x1`, rows `j0..j1` (all tile types).
#[derive(Clone, Copy, Debug)]
struct Region {
    x0: u32,
    x1: u32,
    j0: u32,
    j1: u32,
}

struct Bufs {
    bytes: u64,
    /// The offset and type tables (bytes) the buffers were made for.
    tables: Vec<u8>,
    a: wgpu::Buffer,
    _b: wgpu::Buffer,
    snap: wgpu::Buffer,
    staging: wgpu::Buffer,
    offs: wgpu::Buffer,
    tinfo: wgpu::Buffer,
    bind: [wgpu::BindGroup; 2],
}

/// The GPU toppler.
pub struct PileGpu {
    device: wgpu::Device,
    queue: wgpu::Queue,
    name: String,
    max_bytes: u64,
    layout: wgpu::BindGroupLayout,
    pipe: wgpu::ComputePipeline,
    params: wgpu::Buffer,
    stats: wgpu::Buffer,
    stats_read: wgpu::Buffer,
    bufs: Option<Bufs>,
    batch: usize,
}

impl PileGpu {
    /// Open the default high-performance adapter without a window.
    pub fn headless() -> Result<PileGpu, String> {
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
            label: Some("jetcarp-gpu pile"),
            required_features: wgpu::Features::empty(),
            required_limits: limits,
            experimental_features: Default::default(),
            memory_hints: wgpu::MemoryHints::Performance,
            trace: Default::default(),
        }))
        .map_err(|e| format!("GPU device: {e}"))?;
        Ok(PileGpu::new(device, queue, describe(&info)))
    }

    pub fn new(device: wgpu::Device, queue: wgpu::Queue, name: String) -> PileGpu {
        let lim = device.limits();
        let max_bytes = lim.max_storage_buffer_binding_size.min(lim.max_buffer_size);
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("jetcarp-gpu pile"),
            source: wgpu::ShaderSource::Wgsl(include_str!("pile.wgsl").into()),
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
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("jetcarp-gpu pile"),
            entries: &[
                storage(0, true),
                storage(1, false),
                storage(2, true),
                storage(3, true),
                storage(4, false),
                wgpu::BindGroupLayoutEntry {
                    binding: 5,
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
        let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("jetcarp-gpu pile"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipe = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("pile sweep"),
            layout: Some(&pl),
            module: &module,
            entry_point: Some("sweep"),
            compilation_options: Default::default(),
            cache: None,
        });
        let params = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("pile params"),
            size: (MAX_BATCH * STRIDE) as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let sbytes = ((MAX_BATCH + STATS_HEAD) * 4) as u64;
        let stats = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("pile stats"),
            size: sbytes,
            usage: wgpu::BufferUsages::STORAGE
                | wgpu::BufferUsages::COPY_DST
                | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });
        let stats_read = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("pile stats readback"),
            size: sbytes,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        PileGpu {
            device,
            queue,
            name,
            max_bytes,
            layout,
            pipe,
            params,
            stats,
            stats_read,
            bufs: None,
            batch: 8,
        }
    }

    /// Largest board (bytes) the device holds in one buffer.
    pub fn max_bytes(&self) -> u64 {
        self.max_bytes
    }

    fn buffers(&mut self, board: &LatticeBoard) -> Result<(), String> {
        let bytes = board.cur.len() as u64;
        if bytes > self.max_bytes {
            return Err(format!(
                "the board ({} MB) is larger than the GPU's largest buffer ({} MB)",
                bytes >> 20,
                self.max_bytes >> 20
            ));
        }
        let lat = &board.lat;
        let mut offs = Vec::new();
        let mut tinfo: Vec<[u32; 4]> = Vec::new();
        for k in 0..lat.nt {
            tinfo.push([
                offs.len() as u32,
                lat.offs[k].len() as u32,
                lat.deg[k] as u32,
                0,
            ]);
            for o in &lat.offs[k] {
                offs.push(Off {
                    q: o.di.div_euclid(4),
                    s: o.di.rem_euclid(4) as u32,
                    dj: o.dj,
                    k: o.k,
                });
            }
        }
        let dev = &self.device;
        let mk = |label, size, usage| {
            dev.create_buffer(&wgpu::BufferDescriptor {
                label: Some(label),
                size,
                usage,
                mapped_at_creation: false,
            })
        };
        use wgpu::BufferUsages as U;
        let mut tables: Vec<u8> = bytemuck::cast_slice(&offs).to_vec();
        tables.extend_from_slice(bytemuck::cast_slice(&tinfo));
        let reuse = self
            .bufs
            .as_ref()
            .is_some_and(|b| b.bytes == bytes && b.tables == tables);
        if !reuse {
            self.bufs = None;
            let a = mk("pile a", bytes, U::STORAGE | U::COPY_DST | U::COPY_SRC);
            let b = mk("pile b", bytes, U::STORAGE | U::COPY_DST | U::COPY_SRC);
            let snap = mk("pile snapshot", bytes, U::COPY_DST | U::COPY_SRC);
            let staging = mk("pile readback", bytes, U::MAP_READ | U::COPY_DST);
            let offs_b = mk(
                "pile offsets",
                (offs.len().max(1) * 16) as u64,
                U::STORAGE | U::COPY_DST,
            );
            let tinfo_b = mk(
                "pile types",
                (tinfo.len().max(1) * 16) as u64,
                U::STORAGE | U::COPY_DST,
            );
            let bind = |s: &wgpu::Buffer, d: &wgpu::Buffer| {
                dev.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("pile"),
                    layout: &self.layout,
                    entries: &[
                        wgpu::BindGroupEntry {
                            binding: 0,
                            resource: s.as_entire_binding(),
                        },
                        wgpu::BindGroupEntry {
                            binding: 1,
                            resource: d.as_entire_binding(),
                        },
                        wgpu::BindGroupEntry {
                            binding: 2,
                            resource: offs_b.as_entire_binding(),
                        },
                        wgpu::BindGroupEntry {
                            binding: 3,
                            resource: tinfo_b.as_entire_binding(),
                        },
                        wgpu::BindGroupEntry {
                            binding: 4,
                            resource: self.stats.as_entire_binding(),
                        },
                        wgpu::BindGroupEntry {
                            binding: 5,
                            resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                                buffer: &self.params,
                                offset: 0,
                                size: wgpu::BufferSize::new(std::mem::size_of::<Params>() as u64),
                            }),
                        },
                    ],
                })
            };
            let bind = [bind(&a, &b), bind(&b, &a)];
            self.bufs = Some(Bufs {
                bytes,
                tables,
                a,
                _b: b,
                snap,
                staging,
                offs: offs_b,
                tinfo: tinfo_b,
                bind,
            });
        }
        let bufs = self.bufs.as_ref().unwrap();
        self.queue
            .write_buffer(&bufs.offs, 0, bytemuck::cast_slice(&offs));
        self.queue
            .write_buffer(&bufs.tinfo, 0, bytemuck::cast_slice(&tinfo));
        self.queue.write_buffer(&bufs.a, 0, &board.cur);
        Ok(())
    }

    fn download(&self, board: &mut LatticeBoard) -> Result<(), String> {
        let bufs = self.bufs.as_ref().unwrap();
        let mut enc = self.device.create_command_encoder(&Default::default());
        enc.copy_buffer_to_buffer(&bufs.a, 0, &bufs.staging, 0, bufs.bytes);
        self.queue.submit([enc.finish()]);
        let v: Vec<u8> = map_read(&self.device, &bufs.staging, bufs.bytes)?;
        board.cur.copy_from_slice(&v);
        Ok(())
    }

    /// Run `k` sweeps (even) from buffer A over `r`; returns the stats words.
    fn run_batch(
        &self,
        board: &LatticeBoard,
        k: usize,
        r: Region,
        nz: Region,
    ) -> Result<Vec<u32>, String> {
        let bufs = self.bufs.as_ref().unwrap();
        let (w, h) = board.dims();
        let lat = &board.lat;
        let base = Params {
            wr: (board.pw / 4) as u32,
            ph: board.ph as u32,
            lw: (w / 4) as u32,
            lh: h as u32,
            nt: lat.nt as u32,
            slot: 0,
            band: lat.reach as u32 + 1,
            ystride: YSTRIDE,
            x0: r.x0,
            rw: r.x1 - r.x0,
            j0: r.j0,
            rh: r.j1 - r.j0,
            nx0: nz.x0,
            nx1: nz.x1,
            nj0: nz.j0,
            nj1: nz.j1,
        };
        let mut slots = vec![0u8; k * STRIDE];
        for s in 0..k {
            let p = Params {
                slot: s as u32,
                ..base
            };
            slots[s * STRIDE..s * STRIDE + std::mem::size_of::<Params>()]
                .copy_from_slice(bytemuck::bytes_of(&p));
        }
        self.queue.write_buffer(&self.params, 0, &slots);
        let mut head = vec![0u32; STATS_HEAD + k];
        head[1] = u32::MAX;
        head[3] = u32::MAX;
        self.queue
            .write_buffer(&self.stats, 0, bytemuck::cast_slice(&head));
        let rows = lat.nt as u32 * base.rh;
        let gx = base.rw.div_ceil(64);
        let (gy, gz) = (rows.min(YSTRIDE), rows.div_ceil(YSTRIDE));
        let mut enc = self.device.create_command_encoder(&Default::default());
        enc.copy_buffer_to_buffer(&bufs.a, 0, &bufs.snap, 0, bufs.bytes);
        {
            let mut pass = enc.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("pile batch"),
                timestamp_writes: None,
            });
            pass.set_pipeline(&self.pipe);
            for s in 0..k {
                pass.set_bind_group(0, &bufs.bind[s % 2], &[(s * STRIDE) as u32]);
                pass.dispatch_workgroups(gx, gy, gz);
            }
        }
        let sb = ((k + STATS_HEAD) * 4) as u64;
        enc.copy_buffer_to_buffer(&self.stats, 0, &self.stats_read, 0, sb);
        self.queue.submit([enc.finish()]);
        map_read(&self.device, &self.stats_read, sb)
    }

    /// The nonzero box of the board in logical words and rows, if any.
    fn nonzero(board: &LatticeBoard) -> Option<Region> {
        let b = board.nonzero_box()?;
        Some(Region {
            x0: ((b[0] - board.i0) / 4) as u32,
            x1: ((b[2] - board.i0) / 4) as u32 + 1,
            j0: (b[1] - board.j0) as u32,
            j1: (b[3] - board.j0) as u32 + 1,
        })
    }

    /// `nz` widened by `cells` on every side, clamped to the box.
    fn widen(board: &LatticeBoard, nz: Region, cells: usize) -> Region {
        let (w, h) = board.dims();
        let dw = (cells / 4 + 1) as u32;
        Region {
            x0: nz.x0.saturating_sub(dw),
            x1: (nz.x1 + dw).min((w / 4) as u32),
            j0: nz.j0.saturating_sub(cells as u32),
            j1: (nz.j1 + cells as u32).min(h as u32),
        }
    }

    fn restore(&self) {
        let bufs = self.bufs.as_ref().unwrap();
        let mut enc = self.device.create_command_encoder(&Default::default());
        enc.copy_buffer_to_buffer(&bufs.snap, 0, &bufs.a, 0, bufs.bytes);
        self.queue.submit([enc.finish()]);
    }
}

impl Toppler for PileGpu {
    fn name(&self) -> String {
        format!("gpu ({})", self.name)
    }

    fn stabilize(&mut self, board: &mut LatticeBoard, ctl: &mut Ctl) -> Result<StabStats, String> {
        // Bytes must stay below 128 for the per-byte arithmetic: degree at most 64.
        if board.lat.max_degree() > 64 {
            return board.stabilize(ctl);
        }
        let mut st = StabStats::default();
        if !board.is_unstable() {
            return Ok(st);
        }
        board.ensure_margin(TILE);
        self.buffers(board)?;
        // Every cell that has held a grain this step lies in `nz`; a batch of k sweeps can only
        // reach k × reach cells further.
        let mut nz = PileGpu::nonzero(board).ok_or("empty board")?;
        let start = Instant::now();
        let mut last_tick = Instant::now();
        loop {
            if ctl.cancelled() {
                self.download(board)?;
                board.sync();
                return Err("cancelled".into());
            }
            let k = self.batch;
            let region = PileGpu::widen(board, nz, k * board.lat.reach + 1);
            let t0 = Instant::now();
            let words = self.run_batch(board, k, region, nz)?;
            let dt = t0.elapsed().as_secs_f64();
            if words[0] != 0 {
                // A grain neared the edge: undo the batch, grow, and run it again.
                self.restore();
                self.download(board)?;
                board.ensure_margin(TILE);
                board.sync();
                st.grows += 1;
                self.buffers(board)?;
                nz = PileGpu::nonzero(board).ok_or("empty board")?;
                continue;
            }
            if words[1] != u32::MAX {
                nz = Region {
                    x0: nz.x0.min(words[1]),
                    x1: nz.x1.max(words[2] + 1),
                    j0: nz.j0.min(words[3]),
                    j1: nz.j1.max(words[4] + 1),
                };
            }
            let mut stable = false;
            for s in 0..k {
                let f = words[STATS_HEAD + s] as u64;
                if f == 0 {
                    stable = true;
                    break;
                }
                st.topplings += f;
                st.sweeps += 1;
            }
            if stable {
                break;
            }
            // Aim for batches of about 50 ms.
            if dt < 0.025 && self.batch < MAX_BATCH {
                self.batch = (self.batch * 2).min(MAX_BATCH);
            } else if dt > 0.1 && self.batch > 2 {
                self.batch = (self.batch / 2).max(2);
            }
            self.batch &= !1;
            if last_tick.elapsed().as_secs_f64() >= ctl.tick_every {
                last_tick = Instant::now();
                if ctl.needs_board {
                    self.download(board)?;
                }
                let tick = Tick {
                    sweeps: st.sweeps,
                    topplings: st.topplings,
                    seconds: start.elapsed().as_secs_f64(),
                };
                ctl.tick(&tick, BoardRef::Lattice(board));
            }
        }
        self.download(board)?;
        board.sync();
        Ok(st)
    }
}
