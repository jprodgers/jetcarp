//! TEMPORARY (#321): the soup hunt's generated kernel on Windows' WARP, opened directly (no
//! self-test), against the CPU. Ignored: run with `--ignored --nocapture`.

use jetcarp_core::hunt::separate::Census;
use jetcarp_core::hunt::soup::{Layout, SoupBatcher, prefix_cpu, soup_state};
use jetcarp_gpu::hunt::GpuHunter;
use jetcarp_gpu::select;
use jetcarp_search::hunt::HuntSpec;

/// Keeps wgpu's HLSL in `JETCARP_WARP_HLSL_DIR`, for FXC's disassembly.
struct Keep;

impl log::Log for Keep {
    fn enabled(&self, m: &log::Metadata) -> bool {
        m.target() == "wgpu_hal::dx12::device" || m.level() <= log::Level::Warn
    }
    fn log(&self, r: &log::Record) {
        let text = r.args().to_string();
        if let Some(rest) = text.strip_prefix("Naga generated shader for \"")
            && let Some(dir) = std::env::var_os("JETCARP_WARP_HLSL_DIR")
        {
            let entry = rest.split('"').next().unwrap_or("x").to_string();
            let body = rest.split_once('\n').map_or("", |(_, b)| b);
            let dir = std::path::PathBuf::from(dir);
            let _ = std::fs::create_dir_all(&dir);
            let _ = std::fs::write(dir.join(format!("{entry}.hlsl")), body);
        } else if r.level() <= log::Level::Warn {
            eprintln!("[{} {}] {text}", r.level(), r.target());
        }
    }
    fn flush(&self) {}
}

#[test]
#[ignore]
fn the_hunt_kernel_on_warp() {
    let _ = log::set_logger(&Keep);
    log::set_max_level(log::LevelFilter::Debug);
    let instance = select::new_instance();
    let all = pollster::block_on(instance.enumerate_adapters(wgpu::Backends::all()));
    let Some(a) = all.into_iter().find(|a| {
        let i = a.get_info();
        i.device_type == wgpu::DeviceType::Cpu
            && (i.backend == wgpu::Backend::Dx12
                || std::env::var_os("JETCARP_WARP_PROBE_ANY").is_some())
    }) else {
        eprintln!("no WARP adapter here");
        return;
    };
    eprintln!("hunt probe on {:?}", a.get_info());
    let (device, queue) = select::open_device(&a, "hunt probe").expect("device");
    let g = GpuHunter::new(device, queue, "warp".into());
    for (rule, tiling, n) in [("B3/S23", "square", 200u64), ("B2/S34", "hex", 100)] {
        let t = jetcarp_core::pattern::tiling_from_key(tiling).unwrap();
        let spec = HuntSpec::default();
        let c = Census::new(rule, &t, None, spec.max_period).unwrap();
        let layout = Layout::new(&c.topo, spec.soup, spec.margin()).unwrap();
        let limits = spec.limits(layout.cells.len());
        let soups: Vec<_> = (0..n).map(|i| layout.soup(soup_state(9, i))).collect();
        eprintln!("BEGIN hunt {rule} on {tiling}");
        let got = g.prefix(&c, layout.region, limits, &soups).unwrap();
        for (i, s) in soups.iter().enumerate() {
            assert_eq!(
                got[i],
                prefix_cpu(&c, layout.region, limits, s),
                "{rule} on {tiling}, soup {i}"
            );
        }
        eprintln!("END hunt {rule} on {tiling}: {n} prefixes equal the CPU's");
    }
    eprintln!("HUNT PROBE DONE");
}
