//! How busy the GPU is with every program, read as a measuring starts (#301), on each system
//! JETCARP runs on.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Where the GPU's busy figure was read from, kept beside the score as `gpu_load_source`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum GpuSource {
    /// Windows' `\GPU Engine(*)\Utilization Percentage` counters, through PDH.
    Pdh,
    /// macOS's IOAccelerator `PerformanceStatistics`, `Device Utilization %`.
    Iokit,
    /// Linux's DRM `fdinfo`: each program's engine time.
    Fdinfo,
    /// NVIDIA's management library.
    Nvml,
    /// AMD's driver on Linux, `gpu_busy_percent`.
    Amdgpu,
}

/// One reading of the GPU's load.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Reading {
    /// The busiest GPU's busy figure, 0 to 1 (none: not read).
    pub busy: Option<f64>,
    /// Where it was read from.
    pub source: Option<GpuSource>,
    /// Why a GPU's load was not read, in words.
    pub not_read: Option<&'static str>,
}

/// A GPU engine counter's instance name, `pid_1234_luid_…_phys_0_eng_0_engtype_3D`: its process,
/// its engine (`luid_…_phys_0_eng_0`) and the engine's type (`3D`).
pub fn parse_instance(_name: &str) -> Option<(u32, &str, &str)> {
    None
}

/// The busiest GPU engine's utilization from the counters' instances.
pub fn pdh_busy<'a>(
    _instances: impl IntoIterator<Item = (&'a str, f64)>,
    _own: u32,
) -> Option<f64> {
    None
}

/// The utilization in one IOAccelerator's `PerformanceStatistics`.
pub fn iokit_busy(_stats: &[(String, f64)]) -> Option<f64> {
    None
}

/// One GPU client's counters, from its `fdinfo`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Client {
    pub driver: String,
    pub pdev: String,
    pub id: String,
    pub engines: BTreeMap<String, u64>,
    pub cycles: BTreeMap<String, (u64, u64)>,
    pub capacity: BTreeMap<String, u32>,
}

/// Every GPU client seen, by device and client id.
pub type Snapshot = BTreeMap<(String, String), Client>;

/// A GPU client's counters from one `fdinfo` file. None for a file that is not a GPU client's.
pub fn parse_fdinfo(_text: &str) -> Option<Client> {
    None
}

/// Each device's busy figure between two snapshots `elapsed_ns` apart.
pub fn fdinfo_busy(
    _before: &Snapshot,
    _after: &Snapshot,
    _elapsed_ns: u64,
) -> BTreeMap<String, f64> {
    BTreeMap::new()
}

/// `nvmlUtilization_t`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct NvmlUtilization {
    pub gpu: u32,
    pub memory: u32,
}

/// NVML's calls used here.
pub struct NvmlApi {
    pub count: unsafe extern "C" fn(*mut u32) -> i32,
    pub handle: unsafe extern "C" fn(u32, *mut *mut std::ffi::c_void) -> i32,
    pub utilization: unsafe extern "C" fn(*mut std::ffi::c_void, *mut NvmlUtilization) -> i32,
}

/// The busiest NVIDIA GPU's utilization.
pub fn nvml_busy(_api: &NvmlApi) -> Option<f64> {
    None
}

/// The GPU's load, read again and again.
pub struct Meter;

impl Meter {
    pub fn start() -> Meter {
        Meter
    }

    pub fn read(&mut self) -> Reading {
        Reading::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Windows' counters on the Acer (an RTX and an Intel UHD), as `PdhGetFormattedCounterArrayW`
    /// lists them: a game's 3D engine on the RTX beside the compositor's, video decode and copy
    /// engines (not counted), a CUDA program's compute engine, and this program's own 3D work
    /// (left out: the load is other programs').
    const PDH: &[(&str, f64)] = &[
        (
            "pid_4120_luid_0x00000000_0x00011C92_phys_0_eng_0_engtype_3D",
            61.25,
        ),
        (
            "pid_1388_luid_0x00000000_0x00011C92_phys_0_eng_0_engtype_3D",
            10.0,
        ),
        (
            "pid_777_luid_0x00000000_0x00011C92_phys_0_eng_0_engtype_3D",
            40.0,
        ),
        (
            "pid_4120_luid_0x00000000_0x00011C92_phys_0_eng_2_engtype_Copy",
            90.0,
        ),
        (
            "pid_5232_luid_0x00000000_0x00011C92_phys_0_eng_3_engtype_VideoDecode",
            50.0,
        ),
        (
            "pid_6001_luid_0x00000000_0x00011C92_phys_0_eng_5_engtype_Compute_0",
            30.0,
        ),
        (
            "pid_1388_luid_0x00000000_0x0001195E_phys_0_eng_0_engtype_3D",
            8.0,
        ),
        (
            "pid_1388_luid_0x00000000_0x0001195E_phys_0_eng_1_engtype_VideoProcessing",
            70.0,
        ),
        ("_Total", 100.0),
    ];

    #[test]
    fn pdh_counters_give_the_busiest_engine_of_other_programs() {
        assert_eq!(
            parse_instance("pid_4120_luid_0x00000000_0x00011C92_phys_0_eng_0_engtype_3D"),
            Some((4120, "luid_0x00000000_0x00011C92_phys_0_eng_0", "3D"))
        );
        assert_eq!(parse_instance("_Total"), None);
        let b = pdh_busy(PDH.iter().copied(), 777).unwrap();
        assert!((b - 0.7125).abs() < 1e-9, "{b}");
        // This program's own work counts when it is another's.
        let b = pdh_busy(PDH.iter().copied(), 1).unwrap();
        assert!((b - 1.0).abs() < 1e-9, "{b}");
        // Idle engines read zero, not unknown; no 3D or compute engine at all is unknown.
        let idle = [(
            "pid_1388_luid_0x00000000_0x0001195E_phys_0_eng_0_engtype_3d",
            0.0,
        )];
        assert_eq!(pdh_busy(idle, 777), Some(0.0));
        assert_eq!(pdh_busy(PDH[3..6].iter().take(2).copied(), 777), None);
        assert_eq!(pdh_busy([], 777), None);
    }

    /// An IOAccelerator's `PerformanceStatistics` as `ioreg -r -d 1 -c IOAccelerator` prints it,
    /// on an M2 Max running a game.
    const IOREG_M2: &str = r#""PerformanceStatistics" = {"In use system memory (driver)"=0,"Alloc system memory"=1744830464,"Tiler Utilization %"=31,"recoveryCount"=0,"lastRecoveryTime"=0,"Renderer Utilization %"=44,"TiledSceneBytes"=2097152,"Device Utilization %"=47,"SplitSceneCount"=0,"Allocated PB Size"=40632320,"In use system memory"=436207616}"#;
    /// An Intel Mac's AMD GPU, which names it `GPU Activity(%)`.
    const IOREG_INTEL_MAC: &str = r#""PerformanceStatistics" = {"hardwareWaitTime"=0,"GPU Activity(%)"=12,"vramFreeBytes"=3911090176}"#;

    /// The numbers in an `ioreg` dictionary's text.
    fn ioreg_stats(text: &str) -> Vec<(String, f64)> {
        let body = text.split_once('{').unwrap().1.trim_end_matches('}');
        body.split(',')
            .filter_map(|kv| {
                let (k, v) = kv.rsplit_once('=')?;
                Some((k.trim_matches('"').to_string(), v.parse().ok()?))
            })
            .collect()
    }

    #[test]
    fn iokit_statistics_give_the_device_s_utilization() {
        let m2 = ioreg_stats(IOREG_M2);
        assert_eq!(m2.len(), 11);
        assert_eq!(iokit_busy(&m2), Some(0.47));
        assert_eq!(iokit_busy(&ioreg_stats(IOREG_INTEL_MAC)), Some(0.12));
        assert_eq!(iokit_busy(&[("recoveryCount".into(), 0.0)]), None);
        assert_eq!(
            iokit_busy(&[("Device Utilization %".into(), 140.0)]),
            Some(1.0)
        );
    }

    /// `fdinfo` of an amdgpu client, a second later: graphics and compute engines (counted), a
    /// video decode engine (not).
    const AMD_0: &str = "pos:\t0\nflags:\t02100002\nmnt_id:\t26\nino:\t1100\ndrm-driver:\tamdgpu\n\
        drm-client-id:\t52102\ndrm-pdev:\t0000:c3:00.0\ndrm-total-vram:\t310292 KiB\n\
        drm-engine-gfx:\t1000000000 ns\ndrm-engine-dec:\t5 ns\n";
    const AMD_1: &str = "pos:\t0\ndrm-driver:\tamdgpu\ndrm-client-id:\t52102\n\
        drm-pdev:\t0000:c3:00.0\ndrm-engine-gfx:\t1300000000 ns\ndrm-engine-compute:\t\
        100000000 ns\ndrm-engine-dec:\t900000005 ns\n";
    /// Another amdgpu client of the same GPU, which drew only now.
    const AMD_OTHER_0: &str =
        "drm-driver:\tamdgpu\ndrm-client-id:\t52100\ndrm-pdev:\t0000:c3:00.0\n";
    const AMD_OTHER_1: &str = "drm-driver:\tamdgpu\ndrm-client-id:\t52100\n\
        drm-pdev:\t0000:c3:00.0\ndrm-engine-gfx:\t200000000 ns\n";
    /// An Intel GPU on i915: two video engines, so a capacity of 2.
    const I915_0: &str = "drm-driver:\ti915\ndrm-client-id:\t9\ndrm-pdev:\t0000:00:02.0\n\
        drm-engine-render:\t500 ns\ndrm-engine-copy:\t0 ns\ndrm-engine-video:\t0 ns\n\
        drm-engine-capacity-video:\t2\n";
    const I915_1: &str = "drm-driver:\ti915\ndrm-client-id:\t9\ndrm-pdev:\t0000:00:02.0\n\
        drm-engine-render:\t250000500 ns\ndrm-engine-copy:\t900000000 ns\n\
        drm-engine-video:\t2000000000 ns\ndrm-engine-capacity-video:\t2\n";
    /// An Intel GPU on xe, which counts in GPU cycles.
    const XE_0: &str = "drm-driver:\txe\ndrm-client-id:\t3\ndrm-pdev:\t0000:00:02.0\n\
        drm-cycles-rcs:\t100\ndrm-total-cycles-rcs:\t1000\ndrm-cycles-bcs:\t0\n\
        drm-total-cycles-bcs:\t1000\ndrm-cycles-ccs:\t0\ndrm-total-cycles-ccs:\t1000\n\
        drm-engine-capacity-ccs:\t4\n";
    const XE_1: &str = "drm-driver:\txe\ndrm-client-id:\t3\ndrm-pdev:\t0000:00:02.0\n\
        drm-cycles-rcs:\t140\ndrm-total-cycles-rcs:\t1100\ndrm-cycles-bcs:\t100\n\
        drm-total-cycles-bcs:\t1100\ndrm-cycles-ccs:\t200\ndrm-total-cycles-ccs:\t1100\n\
        drm-engine-capacity-ccs:\t4\n";

    fn snap(texts: &[&str]) -> Snapshot {
        texts
            .iter()
            .filter_map(|t| parse_fdinfo(t))
            .map(|c| ((c.pdev.clone(), c.id.clone()), c))
            .collect()
    }

    #[test]
    fn fdinfo_gives_each_gpu_s_busiest_engine_over_its_clients() {
        let c = parse_fdinfo(AMD_1).unwrap();
        assert_eq!(
            (c.driver.as_str(), c.pdev.as_str(), c.id.as_str()),
            ("amdgpu", "0000:c3:00.0", "52102")
        );
        assert_eq!(c.engines["gfx"], 1_300_000_000);
        assert_eq!(
            parse_fdinfo("pos:\t0\nflags:\t0100002\n"),
            None,
            "not a GPU"
        );
        let x = parse_fdinfo(XE_1).unwrap();
        assert_eq!(x.cycles["rcs"], (140, 1100));
        assert_eq!(x.capacity["ccs"], 4);

        let second = 1_000_000_000;
        let amd = fdinfo_busy(
            &snap(&[AMD_0, AMD_OTHER_0]),
            &snap(&[AMD_1, AMD_OTHER_1]),
            second,
        );
        let b = amd["0000:c3:00.0"];
        // Graphics: 0.3 s of one client and 0.2 s of the other; video decode not counted.
        assert!((b - 0.5).abs() < 1e-9, "{b}");

        let intel = fdinfo_busy(&snap(&[I915_0]), &snap(&[I915_1]), second);
        let b = intel["0000:00:02.0"];
        assert!((b - 0.25).abs() < 1e-9, "{b}");

        let xe = fdinfo_busy(&snap(&[XE_0]), &snap(&[XE_1]), second);
        // rcs 40 of 100 cycles; ccs 200 of 100 over four engines (0.5); bcs (copy) not counted.
        let b = xe["0000:00:02.0"];
        assert!((b - 0.5).abs() < 1e-9, "{b}");

        // A client seen only once counts nothing; an idle GPU with clients reads zero.
        let fresh = fdinfo_busy(&snap(&[]), &snap(&[AMD_1]), second);
        assert!(fresh.get("0000:c3:00.0").is_none_or(|&b| b == 0.0));
        let idle = fdinfo_busy(&snap(&[AMD_1]), &snap(&[AMD_1]), second);
        assert_eq!(idle["0000:c3:00.0"], 0.0);
    }

    // NVML stand-ins: three GPUs, the second of which can't say (NVML_ERROR_NOT_SUPPORTED).
    unsafe extern "C" fn count3(n: *mut u32) -> i32 {
        unsafe { *n = 3 };
        0
    }
    unsafe extern "C" fn handle_of(i: u32, h: *mut *mut std::ffi::c_void) -> i32 {
        unsafe { *h = (i as usize + 1) as *mut std::ffi::c_void };
        0
    }
    unsafe extern "C" fn util_of(h: *mut std::ffi::c_void, u: *mut NvmlUtilization) -> i32 {
        let pct = match h as usize {
            1 => 12,
            2 => return 3,
            _ => 87,
        };
        unsafe {
            *u = NvmlUtilization {
                gpu: pct,
                memory: 5,
            }
        };
        0
    }
    unsafe extern "C" fn count_fails(_: *mut u32) -> i32 {
        999
    }
    unsafe extern "C" fn none_say(_: *mut std::ffi::c_void, _: *mut NvmlUtilization) -> i32 {
        3
    }

    #[test]
    fn nvml_gives_the_busiest_gpu() {
        let api = NvmlApi {
            count: count3,
            handle: handle_of,
            utilization: util_of,
        };
        assert_eq!(nvml_busy(&api), Some(0.87));
        let api = NvmlApi {
            count: count_fails,
            ..api
        };
        assert_eq!(nvml_busy(&api), None);
        let api = NvmlApi {
            count: count3,
            handle: handle_of,
            utilization: none_say,
        };
        assert_eq!(nvml_busy(&api), None);
    }

    /// The GPU's load is read on this machine where it can be: on Linux with an AMD GPU (the
    /// development desktop's Radeon) from its driver. Elsewhere a reading, or why there is none.
    #[test]
    fn the_gpu_load_is_read_here() {
        let mut m = Meter::start();
        std::thread::sleep(std::time::Duration::from_millis(300));
        let r = m.read();
        eprintln!("GPU load here: {r:?}");
        if let Some(b) = r.busy {
            assert!((0.0..=1.0).contains(&b), "{b}");
            assert!(r.source.is_some());
        } else {
            assert!(r.not_read.is_some(), "no reading, and no reason given");
        }
        #[cfg(target_os = "linux")]
        if amd_busy_files() {
            assert!(r.busy.is_some(), "an AMD GPU, but no reading: {r:?}");
        }
    }

    /// An AMD GPU here that says how busy it is.
    #[cfg(target_os = "linux")]
    fn amd_busy_files() -> bool {
        std::fs::read_dir("/sys/class/drm").is_ok_and(|d| {
            d.flatten()
                .any(|e| e.path().join("device/gpu_busy_percent").exists())
        })
    }

    /// By hand, with the desktop idle: the GPU reads low (`cargo test -- --ignored`).
    #[test]
    #[ignore]
    fn an_idle_gpu_reads_low() {
        let mut m = Meter::start();
        std::thread::sleep(std::time::Duration::from_secs(2));
        let r = m.read();
        eprintln!("GPU load here, idle: {r:?}");
        assert!(r.busy.is_some_and(|b| b < 0.25), "{r:?}");
    }
}
