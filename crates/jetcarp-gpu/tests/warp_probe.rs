//! TEMPORARY (#321): every GPU kernel on Windows' WARP, one rule at a time, saying what it is
//! about to run before it runs it, so a crash names its rule and kernel. Ignored: run with
//! `--ignored --nocapture --test-threads=1`.
//!
//! - `JETCARP_WARP_PROBE=<text>`: only the steps whose label contains it (`=<label>`: that one).
//! - `JETCARP_WARP_PROBE_SKIP=<label>\n<label>...`: leave these out (the runner's way past a crash).
//! - `JETCARP_WARP_PROBE_ANY=1`: any software adapter, not only WARP (to try the probe elsewhere).
//! - `exit_after_warp`: how a process that used WARP ends, three ways (see `exit_child`).

use jetcarp_core::explore::batch::{BatchJob, BatchWorld, run_cpu};
use jetcarp_core::explore::space::Grid;
use jetcarp_core::rules::Rule;
use jetcarp_core::topology::Nb;
use jetcarp_core::{CaSpec, Command, SeedSpec, World};
use jetcarp_gpu::GpuSim;
use jetcarp_gpu::batch::BatchSim;
use jetcarp_gpu::select;
use std::collections::BTreeMap;
use std::io::Write;
use std::sync::Arc;

struct Logger;

impl log::Log for Logger {
    fn enabled(&self, m: &log::Metadata) -> bool {
        m.level() <= log::Level::Warn
            || (m.level() <= log::Level::Info && m.target().starts_with("wgpu_hal"))
            || (m.target() == "wgpu_hal::dx12::device" && hlsl_dir().is_some())
    }
    fn log(&self, r: &log::Record) {
        if !self.enabled(r.metadata()) {
            return;
        }
        let text = r.args().to_string();
        // wgpu's HLSL for each entry point, kept for FXC's disassembly.
        if let Some(rest) = text.strip_prefix("Naga generated shader for \"")
            && let Some(dir) = hlsl_dir()
        {
            let entry = rest.split('"').next().unwrap_or("x").to_string();
            let body = rest.split_once('\n').map_or("", |(_, b)| b);
            let _ = std::fs::create_dir_all(&dir);
            let _ = std::fs::write(dir.join(format!("{entry}.hlsl")), body);
            eprintln!("[hlsl] {entry}: {} bytes", body.len());
            return;
        }
        if r.level() <= log::Level::Info {
            eprintln!("[{} {}] {text}", r.level(), r.target());
        }
    }
    fn flush(&self) {}
}

fn hlsl_dir() -> Option<std::path::PathBuf> {
    std::env::var_os("JETCARP_WARP_HLSL_DIR").map(Into::into)
}

fn logger() {
    let _ = log::set_logger(&Logger);
    log::set_max_level(if hlsl_dir().is_some() {
        log::LevelFilter::Debug
    } else {
        log::LevelFilter::Info
    });
}

fn say(s: &str) {
    eprintln!("{s}");
    let _ = std::io::stderr().flush();
}

fn wanted(label: &str) -> bool {
    if let Ok(skip) = std::env::var("JETCARP_WARP_PROBE_SKIP")
        && skip.lines().any(|l| l.trim() == label)
    {
        return false;
    }
    match std::env::var("JETCARP_WARP_PROBE") {
        Ok(f) if f.starts_with('=') => label == &f[1..],
        Ok(f) if !f.trim().is_empty() => label.contains(f.trim()),
        _ => true,
    }
}

fn is_target(i: &wgpu::AdapterInfo) -> bool {
    i.device_type == wgpu::DeviceType::Cpu
        && (i.backend == wgpu::Backend::Dx12
            || std::env::var_os("JETCARP_WARP_PROBE_ANY").is_some())
}

/// The adapter to probe: WARP, or with `JETCARP_WARP_PROBE_ANY=1` any software adapter.
fn adapter(instance: &wgpu::Instance, list: bool) -> Option<wgpu::Adapter> {
    let all = pollster::block_on(instance.enumerate_adapters(wgpu::Backends::all()));
    for a in all.iter().filter(|_| list) {
        say(&format!("adapter: {:?}", a.get_info()));
    }
    all.into_iter().find(|a| is_target(&a.get_info()))
}

fn world(grid: &Grid, rule: &Rule, nb: Nb, seed: &SeedSpec, drop: u32) -> Option<World> {
    let mut w = World::on_topology(grid.topo.clone(), 0x5eed);
    let mut ca = CaSpec::new(&rule.notation()).nb(nb).seed(seed.clone());
    ca.drop_per_gen = drop;
    w.add_ca(ca).ok()?;
    Some(w)
}

fn extra() -> Vec<(&'static str, SeedSpec, u32)> {
    vec![
        ("B3/S23", SeedSpec::Random { density: 0.35 }, 0),
        ("B013/S1", SeedSpec::Random { density: 0.35 }, 0),
        ("JACA B3/S23 blast 2", SeedSpec::Halves { density: 0.35 }, 0),
        ("Margolus Critters", SeedSpec::Random { density: 0.3 }, 0),
        ("B3-cnqy/S23-a", SeedSpec::Random { density: 0.35 }, 0),
        ("B01e2a/S2-i3", SeedSpec::Random { density: 0.35 }, 0),
        (
            "B2a3-ckn/S1e2-in3/C4",
            SeedSpec::Random { density: 0.35 },
            0,
        ),
        ("R2,C3,S5-9,B7-8", SeedSpec::Random { density: 0.4 }, 0),
        ("R2,C2,S0-5,B0-3", SeedSpec::Random { density: 0.35 }, 0),
        ("ForestFire 0.05/0.001", SeedSpec::RandomStates, 0),
        ("Sandpile", SeedSpec::Pile { grains: 2_000 }, 0),
        ("Sandpile", SeedSpec::Pile { grains: 2_000 }, 3),
        (
            "Sandpile",
            SeedSpec::Pile {
                grains: 4_294_967_000,
            },
            500,
        ),
    ]
}

enum Item {
    World {
        grid: usize,
        rule: Rule,
        nb: Nb,
        seed: SeedSpec,
        drop: u32,
    },
    Batch {
        job: BatchJob,
    },
}

fn items() -> (Vec<Grid>, Vec<(String, Item)>) {
    let mut grids = Vec::new();
    let mut out = Vec::new();
    for (key, cells) in [("square", 1024), ("rhombitrihex", 800), ("spectre", 800)] {
        let grid = Grid::build(key, cells).unwrap();
        let g = grids.len();
        let mut rules: Vec<(Rule, Nb, SeedSpec, u32)> =
            select::test_rules(&grid, jetcarp_gpu::supports_rule)
                .into_iter()
                .map(|(r, nb)| (r, nb, SeedSpec::RandomStates, 0))
                .collect();
        for (text, seed, drop) in extra() {
            if let Ok((rule, nb)) = Rule::parse(text)
                && jetcarp_gpu::supports_rule(&rule)
            {
                rules.push((rule, nb.unwrap_or_default(), seed, drop));
            }
        }
        for (rule, nb, seed, drop) in &rules {
            if world(&grid, rule, *nb, seed, *drop).is_none() {
                continue;
            }
            out.push((
                format!("world {} {nb:?} drop {drop} on {key}", rule.notation()),
                Item::World {
                    grid: g,
                    rule: rule.clone(),
                    nb: *nb,
                    seed: seed.clone(),
                    drop: *drop,
                },
            ));
        }
        let mut jobs: BTreeMap<String, BatchJob> = BTreeMap::new();
        for (rule, nb, seed, drop) in &rules {
            if *drop != 0 || !BatchSim::supports(rule) {
                continue;
            }
            let Some(w) = world(&grid, rule, *nb, seed, 0) else {
                continue;
            };
            let group = match rule {
                Rule::Ltl(l) => format!("ltl {:?} {}", l.hood, l.range),
                _ => format!("{nb:?}"),
            };
            jobs.entry(group)
                .or_insert_with(|| BatchJob {
                    topo: grid.topo.clone(),
                    nb: *nb,
                    worlds: vec![],
                    gens: 24,
                    frames_at: vec![24],
                })
                .worlds
                .push(BatchWorld {
                    rule: rule.clone(),
                    init: w.fields[0].state.clone(),
                });
        }
        for (group, job) in jobs {
            // One batch per rule, so a crash names it.
            for w in &job.worlds {
                let one = BatchJob {
                    topo: job.topo.clone(),
                    nb: job.nb,
                    worlds: vec![w.clone()],
                    gens: job.gens,
                    frames_at: job.frames_at.clone(),
                };
                out.push((
                    format!("batch {group} {} on {key}", w.rule.notation()),
                    Item::Batch { job: one },
                ));
            }
            out.push((format!("batch {group} all on {key}"), Item::Batch { job }));
        }
        grids.push(grid);
    }
    (grids, out)
}

#[test]
#[ignore]
fn every_kernel_on_warp() {
    logger();
    let instance = select::new_instance();
    let Some(a) = adapter(&instance, true) else {
        say("no WARP adapter here");
        return;
    };
    say(&format!("probing {:?}", a.get_info()));
    let (device, queue) = select::open_device(&a, "warp probe").expect("device");
    device.on_uncaptured_error(Arc::new(|e| say(&format!("UNCAPTURED GPU ERROR: {e}"))));
    let name = jetcarp_gpu::describe(&a.get_info());
    let (grids, items) = items();
    let mut sim: Option<GpuSim> = None;
    let mut batch: Option<BatchSim> = None;
    let mut bad: Vec<String> = Vec::new();
    let mut ran = 0;
    for (label, item) in &items {
        if !wanted(label) {
            continue;
        }
        let outcome = match item {
            Item::World {
                grid,
                rule,
                nb,
                seed,
                drop,
            } => {
                let grid = &grids[*grid];
                let mut cpu = world(grid, rule, *nb, seed, *drop).unwrap();
                let mut gpu = world(grid, rule, *nb, seed, *drop).unwrap();
                cpu.apply(Command::Step { gens: 24 }).unwrap();
                let sim = sim.get_or_insert_with(|| {
                    say("BEGIN making the world stepper's pipelines");
                    let s = GpuSim::new(device.clone(), queue.clone(), name.clone());
                    say("END making the world stepper's pipelines: done");
                    s
                });
                say(&format!("BEGIN {label}"));
                match sim.step_mixed(&mut gpu, 24) {
                    Ok(m) if m.gpu_gens == 0 => format!("not on the GPU ({:?})", m.fallback),
                    Ok(_) if cpu.hash_hex() == gpu.hash_hex() => "same as the CPU".into(),
                    Ok(_) => {
                        bad.push(label.clone());
                        "DIFFERENT from the CPU".into()
                    }
                    Err(e) => {
                        bad.push(label.clone());
                        format!("ERROR {e}")
                    }
                }
            }
            Item::Batch { job } => {
                let Ok(want) = run_cpu(job) else { continue };
                let batch = batch.get_or_insert_with(|| {
                    say("BEGIN making the batch stepper's pipelines");
                    let b = BatchSim::new(device.clone(), queue.clone(), name.clone());
                    say("END making the batch stepper's pipelines: done");
                    b
                });
                say(&format!("BEGIN {label}"));
                match batch.run(job) {
                    Ok(g) if g == want => "same as the CPU".into(),
                    Ok(_) => {
                        bad.push(label.clone());
                        "DIFFERENT from the CPU".into()
                    }
                    Err(e) => {
                        bad.push(label.clone());
                        format!("ERROR {e}")
                    }
                }
            }
        };
        ran += 1;
        say(&format!("END {label}: {outcome}"));
    }
    say(&format!(
        "PROBE DONE; {ran} ran, {} bad: {bad:?}",
        bad.len()
    ));
    assert!(bad.is_empty(), "{bad:?}");
}

/// A small compute job of wgpu's own (no JETCARP kernel): each word of a buffer plus one.
fn plain_compute(device: &wgpu::Device, queue: &wgpu::Queue) -> bool {
    let n = 1u32 << 16;
    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("plus one"),
        source: wgpu::ShaderSource::Wgsl(
            "@group(0) @binding(0) var<storage, read_write> b: array<u32>;
             @compute @workgroup_size(256) fn main(@builtin(global_invocation_id) g: vec3<u32>) {
               if (g.x < arrayLength(&b)) { b[g.x] = b[g.x] + g.x + 1u; }
             }"
            .into(),
        ),
    });
    let pipe = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: Some("plus one"),
        layout: None,
        module: &module,
        entry_point: Some("main"),
        compilation_options: Default::default(),
        cache: None,
    });
    let buf = device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: n as u64 * 4,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
        mapped_at_creation: false,
    });
    let read = device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: n as u64 * 4,
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: None,
        layout: &pipe.get_bind_group_layout(0),
        entries: &[wgpu::BindGroupEntry {
            binding: 0,
            resource: buf.as_entire_binding(),
        }],
    });
    let mut enc = device.create_command_encoder(&Default::default());
    {
        let mut p = enc.begin_compute_pass(&Default::default());
        p.set_pipeline(&pipe);
        p.set_bind_group(0, &bind, &[]);
        p.dispatch_workgroups(n / 256, 1, 1);
    }
    enc.copy_buffer_to_buffer(&buf, 0, &read, 0, n as u64 * 4);
    queue.submit([enc.finish()]);
    read.slice(..).map_async(wgpu::MapMode::Read, |_| {});
    let _ = device.poll(wgpu::PollType::wait_indefinitely());
    let v: Vec<u32> = bytemuck_free(&read.slice(..).get_mapped_range().expect("mapped"));
    read.unmap();
    v.iter().enumerate().all(|(i, &x)| x == i as u32 + 1)
}

fn bytemuck_free(b: &[u8]) -> Vec<u32> {
    b.chunks_exact(4)
        .map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]]))
        .collect()
}

#[cfg(windows)]
fn terminate(code: u32) -> ! {
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetCurrentProcess() -> *mut std::ffi::c_void;
        fn TerminateProcess(h: *mut std::ffi::c_void, code: u32) -> i32;
    }
    unsafe { TerminateProcess(GetCurrentProcess(), code) };
    std::process::exit(code as i32)
}

#[cfg(unix)]
fn terminate(code: u32) -> ! {
    unsafe extern "C" {
        fn _exit(status: std::ffi::c_int) -> !;
    }
    unsafe { _exit(code as i32) }
}

/// The child of `exit_after_warp`: opens WARP, runs a small compute job (and with
/// `JETCARP_WARP_EXIT_SIM=1` Life on the world stepper), then ends as `JETCARP_WARP_EXIT` says:
/// `drop`, everything dropped, the instance last (the process's only one), then returning as
/// usual; `keep`, the same with the process's first instance kept (as `select::new_instance` keeps
/// it, #315), then returning as usual; `end`, nothing dropped, the process ended at once as
/// `jetcarp_display::exit::end_process` ends it.
#[test]
#[ignore]
fn exit_child() {
    let Ok(mode) = std::env::var("JETCARP_WARP_EXIT") else {
        return;
    };
    let instance = if mode == "drop" {
        wgpu::Instance::new(select::instance_descriptor())
    } else {
        select::new_instance()
    };
    let a = adapter(&instance, false).expect("WARP");
    let (device, queue) = select::open_device(&a, "exit probe").expect("device");
    assert!(
        plain_compute(&device, &queue),
        "plain compute gave wrong words"
    );
    let mut sim = None;
    if std::env::var_os("JETCARP_WARP_EXIT_SIM").is_some() {
        let grid = Grid::build("square", 4096).unwrap();
        let rule = Rule::parse("B3/S23").unwrap().0;
        let mut w = world(
            &grid,
            &rule,
            Nb::Touch,
            &SeedSpec::Random { density: 0.35 },
            0,
        )
        .unwrap();
        let mut s = GpuSim::new(device.clone(), queue.clone(), "warp".into());
        s.step_mixed(&mut w, 100).expect("Life");
        sim = Some(s);
    }
    say(&format!("exit child: {mode}, the GPU work done"));
    match mode.as_str() {
        "end" => terminate(0),
        _ => {
            drop(sim);
            drop(queue);
            drop(device);
            drop(a);
            drop(instance);
            // Time for a driver thread to run code that has gone, if it would.
            std::thread::sleep(std::time::Duration::from_secs(2));
            say("exit child: returning");
        }
    }
}

#[test]
#[ignore]
fn exit_after_warp() {
    if adapter(&select::new_instance(), false).is_none() {
        say("no WARP adapter here");
        return;
    }
    let exe = std::env::current_exe().unwrap();
    let mut summary = Vec::new();
    for sim in [false, true] {
        for mode in ["drop", "keep", "end"] {
            let mut codes = Vec::new();
            for _ in 0..5 {
                let mut c = std::process::Command::new(&exe);
                c.args(["--exact", "exit_child", "--ignored", "--nocapture"])
                    .env("JETCARP_WARP_EXIT", mode);
                if sim {
                    c.env("JETCARP_WARP_EXIT_SIM", "1");
                }
                let st = c.status().unwrap();
                codes.push(match st.code() {
                    Some(k) => format!("0x{:08X}", k as u32),
                    None => format!("{st:?}"),
                });
            }
            let line = format!(
                "EXIT {mode}{}: {}",
                if sim { " +Life" } else { "" },
                codes.join(" ")
            );
            say(&line);
            summary.push(line);
        }
    }
    say(&summary.join("\n"));
}

/// The bindings of a JIT case: per group, (binding, kind): `r` read-only storage, `w` read-write
/// storage, `u` uniform, `d` uniform with a dynamic offset.
type Groups = &'static [&'static [(u32, char)]];

const SIM: Groups = &[
    &[(0, 'r'), (1, 'r'), (2, 'r'), (3, 'w'), (4, 'd')],
    &[(0, 'r'), (1, 'w'), (2, 'w')],
    &[(0, 'w')],
];
const EDIT: Groups = &[&[(0, 'r')], &[(0, 'r'), (1, 'w'), (2, 'w')]];
const BATCH: Groups = &[
    &[(0, 'u'), (1, 'd'), (2, 'r'), (3, 'r'), (4, 'r'), (5, 'w')],
    &[(0, 'r'), (1, 'w')],
    &[(0, 'r'), (1, 'w')],
];
const PILE: Groups = &[&[(0, 'r'), (1, 'w'), (2, 'r'), (3, 'r'), (4, 'w'), (5, 'd')]];
const ONE: Groups = &[&[(0, 'w')]];

/// `src` without the text from `from` (included) to `to` (left), with `put` in its place.
fn cut(src: &str, from: &str, to: &str, put: &str) -> String {
    let a = src.find(from).unwrap_or_else(|| panic!("no {from:?}"));
    let b = a + src[a..].find(to).unwrap_or_else(|| panic!("no {to:?}"));
    format!("{}{put}{}", &src[..a], &src[b..])
}

const C0: &str = "      // Life-like and Generations: only state-1 neighbours count";
const C1: &str = "      // WireWorld: ages are left as they are.";
const C6: &str = "      // Priority table: the first row whose own state matches";
const C7: &str = "      // JACA (jaca.rs): birth/survive";
const CDEF: &str = "      default: {}";
const TAIL: &str = "  if (p.flags != 0u || p.stat != NONE) {\n    workgroupBarrier();";
const AFTER_STEP: &str = "// JACA with a blast radius, part 1";
const HALT: &str = "  if (li == 0u) { wg_halt = atomicLoad(&ctrl.halted); }\n  if (workgroupUniformLoad(&wg_halt) != 0u) { return; }\n";

fn sim_variant(name: &str) -> String {
    let mut s = include_str!("../src/sim.wgsl").to_string();
    for part in name.split('+') {
        s = match part {
            "full" => s,
            "no_case6" => cut(&s, C6, C7, ""),
            "only_case0" => cut(&s, C1, CDEF, ""),
            "only_case6" => cut(&cut(&s, C7, CDEF, ""), C0, C6, ""),
            "only_case7" => cut(
                &cut(&s, C0, C7, ""),
                "      // B0 Life-like rules",
                CDEF,
                "",
            ),
            "no_tail" => cut(&s, TAIL, AFTER_STEP, "}\n\n"),
            "no_halt" => s.replacen(HALT, "", 1),
            p => panic!("no variant {p}"),
        };
    }
    s
}

const SMALL_HEAD: &str = "@group(0) @binding(0) var<storage, read_write> b: array<u32>;\nvar<workgroup> s: atomic<u32>;\nvar<workgroup> h: u32;\nfn bump(c: ptr<function, array<u32, 8>>, k: u32) { (*c)[k] = (*c)[k] + 1u; }\n";

fn small(body: &str) -> String {
    format!(
        "{SMALL_HEAD}@compute @workgroup_size(256)\nfn main(@builtin(local_invocation_index) li: u32) {{\n{body}\n}}\n"
    )
}

/// Every JIT case: (name, WGSL, entry point, bindings).
fn jit_cases() -> Vec<(String, String, &'static str, Groups)> {
    let mut v: Vec<(String, String, &'static str, Groups)> = Vec::new();
    for e in ["step", "pack", "control", "contact", "dilate"] {
        v.push((
            format!("sim {e}"),
            include_str!("../src/sim.wgsl").into(),
            e,
            SIM,
        ));
    }
    for name in [
        "no_case6",
        "only_case0",
        "only_case6",
        "only_case7",
        "no_tail",
        "no_halt",
        "no_tail+no_halt",
        "only_case0+no_tail",
        "only_case0+no_halt",
        "only_case0+no_tail+no_halt",
        "only_case6+no_tail+no_halt",
        "no_case6+no_tail",
    ] {
        v.push((format!("sim step {name}"), sim_variant(name), "step", SIM));
    }
    v.push((
        "edit edit".into(),
        include_str!("../src/edit.wgsl").into(),
        "edit",
        EDIT,
    ));
    for e in ["step", "contact", "dilate"] {
        v.push((
            format!("batch {e}"),
            include_str!("../src/batch.wgsl").into(),
            e,
            BATCH,
        ));
    }
    v.push((
        "pile sweep".into(),
        include_str!("../src/pile.wgsl").into(),
        "sweep",
        PILE,
    ));
    let arr = "  var c = array<u32, 8>(0u, 0u, 0u, 0u, 0u, 0u, 0u, 0u);\n  let k = b[li] % 8u;\n";
    for (name, body) in [
        (
            "small array+barrier",
            format!("{arr}  c[k] = c[k] + 1u;\n  atomicAdd(&s, c[b[li + 1u] % 8u]);\n  workgroupBarrier();\n  if (li == 0u) {{ b[0] = atomicLoad(&s); }}"),
        ),
        (
            "small array, no barrier",
            format!("{arr}  c[k] = c[k] + 1u;\n  b[li] = c[b[li + 1u] % 8u];"),
        ),
        (
            "small barrier, no array",
            "  atomicAdd(&s, b[li]);\n  workgroupBarrier();\n  if (li == 0u) { b[0] = atomicLoad(&s); }".to_string(),
        ),
        (
            "small constant-index array+barrier",
            format!("{arr}  c[3] = k;\n  atomicAdd(&s, c[3] + c[2]);\n  workgroupBarrier();\n  if (li == 0u) {{ b[0] = atomicLoad(&s); }}"),
        ),
        (
            "small array by pointer+barrier",
            format!("{arr}  bump(&c, k);\n  atomicAdd(&s, c[b[li + 1u] % 8u]);\n  workgroupBarrier();\n  if (li == 0u) {{ b[0] = atomicLoad(&s); }}"),
        ),
        (
            "small array live across barrier",
            format!("{arr}  c[k] = c[k] + 1u;\n  workgroupBarrier();\n  b[li] = c[b[li + 1u] % 8u];"),
        ),
        (
            "small uniform load+return, then array",
            format!("  if (li == 0u) {{ h = b[1000]; }}\n  if (workgroupUniformLoad(&h) != 0u) {{ return; }}\n{arr}  c[k] = c[k] + 1u;\n  b[li] = c[b[li + 1u] % 8u];"),
        ),
        (
            "small uniform load+return, no array",
            "  if (li == 0u) { h = b[1000]; }\n  if (workgroupUniformLoad(&h) != 0u) { return; }\n  b[li] = b[li] + 1u;".to_string(),
        ),
        (
            "small loop+barrier",
            "  var t = 0u;\n  for (var e = 0u; e < b[0]; e = e + 1u) { t += b[e + 1u]; }\n  atomicAdd(&s, t);\n  workgroupBarrier();\n  if (li == 0u) { b[0] = atomicLoad(&s); }".to_string(),
        ),
        (
            "small array in loop+barrier",
            format!("{arr}  for (var e = 0u; e < b[0]; e = e + 1u) {{ c[b[e + 1u] % 8u] += 1u; }}\n  atomicAdd(&s, c[k]);\n  workgroupBarrier();\n  if (li == 0u) {{ b[0] = atomicLoad(&s); }}"),
        ),
    ] {
        v.push((name.to_string(), small(&body), "main", ONE));
    }
    v
}

/// One JIT case (`JETCARP_WARP_JIT=<name>`; `list` names them all): its pipeline made, one
/// workgroup dispatched and waited for, then a second for WARP's compile tasks to finish.
#[test]
#[ignore]
fn jit_child() {
    let Ok(want) = std::env::var("JETCARP_WARP_JIT") else {
        return;
    };
    let cases = jit_cases();
    if want == "list" {
        for (name, ..) in &cases {
            println!("CASE {name}");
        }
        return;
    }
    logger();
    let (_, src, entry, groups) = cases
        .into_iter()
        .find(|c| c.0 == want)
        .unwrap_or_else(|| panic!("no case {want}"));
    let instance = select::new_instance();
    let a = adapter(&instance, false).expect("WARP");
    let (device, queue) = select::open_device(&a, "jit probe").expect("device");
    device.on_uncaptured_error(Arc::new(|e| say(&format!("UNCAPTURED GPU ERROR: {e}"))));
    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some(&want),
        source: wgpu::ShaderSource::Wgsl(src.into()),
    });
    let mut layouts = Vec::new();
    let mut binds = Vec::new();
    let mut keep = Vec::new();
    for g in groups {
        let entries: Vec<wgpu::BindGroupLayoutEntry> = g
            .iter()
            .map(|&(b, k)| wgpu::BindGroupLayoutEntry {
                binding: b,
                visibility: wgpu::ShaderStages::COMPUTE,
                ty: wgpu::BindingType::Buffer {
                    ty: match k {
                        'r' => wgpu::BufferBindingType::Storage { read_only: true },
                        'w' => wgpu::BufferBindingType::Storage { read_only: false },
                        _ => wgpu::BufferBindingType::Uniform,
                    },
                    has_dynamic_offset: k == 'd',
                    min_binding_size: None,
                },
                count: None,
            })
            .collect();
        let l = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: None,
            entries: &entries,
        });
        let bufs: Vec<wgpu::Buffer> = g
            .iter()
            .map(|&(_, k)| {
                device.create_buffer(&wgpu::BufferDescriptor {
                    label: None,
                    size: if k == 'u' || k == 'd' { 256 } else { 1 << 16 },
                    usage: if k == 'u' || k == 'd' {
                        wgpu::BufferUsages::UNIFORM
                    } else {
                        wgpu::BufferUsages::STORAGE
                    },
                    mapped_at_creation: false,
                })
            })
            .collect();
        let bg_entries: Vec<wgpu::BindGroupEntry> = g
            .iter()
            .zip(&bufs)
            .map(|(&(b, _), buf)| wgpu::BindGroupEntry {
                binding: b,
                resource: buf.as_entire_binding(),
            })
            .collect();
        binds.push(device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &l,
            entries: &bg_entries,
        }));
        layouts.push(l);
        keep.push(bufs);
    }
    let refs: Vec<Option<&wgpu::BindGroupLayout>> = layouts.iter().map(Some).collect();
    let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: None,
        bind_group_layouts: &refs,
        immediate_size: 0,
    });
    say(&format!("JIT {want}: making the pipeline"));
    let pipe = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: Some(&want),
        layout: Some(&pl),
        module: &module,
        entry_point: Some(entry),
        compilation_options: Default::default(),
        cache: None,
    });
    say(&format!("JIT {want}: dispatching"));
    let mut enc = device.create_command_encoder(&Default::default());
    {
        let mut p = enc.begin_compute_pass(&Default::default());
        p.set_pipeline(&pipe);
        for (i, (bg, g)) in binds.iter().zip(groups.iter()).enumerate() {
            let dynamic: Vec<u32> = g.iter().filter(|(_, k)| *k == 'd').map(|_| 0).collect();
            p.set_bind_group(i as u32, bg, &dynamic);
        }
        p.dispatch_workgroups(1, 1, 1);
    }
    queue.submit([enc.finish()]);
    let _ = device.poll(wgpu::PollType::wait_indefinitely());
    std::thread::sleep(std::time::Duration::from_secs(1));
    say(&format!("JIT {want}: survived"));
}
