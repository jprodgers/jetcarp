//! How busy the GPU is, with every program's work, read as a measuring starts (#301) on each
//! system JETCARP runs on. A busy GPU lowers the JETCARP Hardware Score's GPU parts, so the
//! measuring looks first ([`super::quiet::Load::sample`]) and keeps what it read beside the
//! score, with where it read it from ([`GpuSource`]).
//!
//! | System | Read from | What it counts |
//! | --- | --- | --- |
//! | Windows | PDH's `\GPU Engine(*)\Utilization Percentage`, every vendor | other programs' 3D and compute engines, summed per engine as Task Manager does |
//! | macOS | IOKit: each `IOAccelerator`'s `PerformanceStatistics`, `Device Utilization %` | every program's, no root needed |
//! | Linux, AMD | the driver's `gpu_busy_percent` | every program's |
//! | Linux, NVIDIA | NVML (`libnvidia-ml.so.1`, opened while running, so no build dependency: [`super::nvml`]) | every program's |
//! | Linux, Intel and others | DRM `fdinfo` of every process this user can read | those programs' render and compute engines |
//!
//! The GPU's figure is the busiest engine's share of its time, of the busiest GPU: a game keeps
//! its GPU's 3D engine busy, while a video decoded or a copy runs on engines of its own and slows
//! JETCARP's work little, so those are not counted. Where a figure is a rate (PDH's counters,
//! `fdinfo`'s times) it is the share between two readings; the others are the driver's own
//! recent figure. Where none can be read, the reading says why ([`Reading::not_read`]) and the
//! measuring goes on: a GPU not read never stops one, nor marks it as taken under load.
//!
//! **Limits.** `fdinfo` shows only the processes this user may read: another user's programs
//! (a second login's game) are not seen, nor, inside a sandbox such as Flatpak's, programs
//! outside it. The i915 and xe drivers' own counters (their PMU, through `perf_event_open`) would
//! see every program, but need privileges a desktop program does not have. NVIDIA's figure needs
//! its management library, which comes with its driver (some distributions package it on its
//! own); without it the reading says so.

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

impl GpuSource {
    /// In words, after "from": `Windows' GPU engine counters`.
    pub fn text(self) -> &'static str {
        match self {
            GpuSource::Pdh => "Windows' GPU engine counters",
            GpuSource::Iokit => "macOS's IOAccelerator statistics",
            GpuSource::Fdinfo => "the kernel's DRM fdinfo",
            GpuSource::Nvml => "NVIDIA's NVML",
            GpuSource::Amdgpu => "AMD's gpu_busy_percent",
        }
    }
}

/// One reading of the GPU's load.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Reading {
    /// The busiest GPU's busy figure, 0 to 1 (none: not read).
    pub busy: Option<f64>,
    /// Where it was read from.
    pub source: Option<GpuSource>,
    /// Why a GPU's load was not read, in words (beside a figure: another GPU's was).
    pub not_read: Option<&'static str>,
}

/// No GPU here gives a figure JETCARP can read.
pub const NO_FIGURE: &str = "no GPU driver here says how busy it is";
/// An NVIDIA GPU on Linux, without the library that says how busy it is.
pub const NO_NVML: &str =
    "the NVIDIA GPU's management library, libnvidia-ml.so.1, could not be opened";

/// The busiest of `readings`, with its source (a tie goes to the first).
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn busiest(readings: impl IntoIterator<Item = (f64, GpuSource)>) -> Option<(f64, GpuSource)> {
    readings
        .into_iter()
        .filter(|(b, _)| b.is_finite())
        .fold(None, |best, (b, s)| match best {
            Some((bb, _)) if bb >= b => best,
            _ => Some((b.clamp(0.0, 1.0), s)),
        })
}

/// The counter Windows' GPU figures come from, in English (`PdhAddEnglishCounterW` takes the
/// English names on any language of Windows), every instance of it: one per process, adapter and
/// engine.
pub const COUNTER_PATH: &str = r"\GPU Engine(*)\Utilization Percentage";

/// A GPU engine counter's instance name, `pid_1234_luid_…_phys_0_eng_0_engtype_3D`: its process,
/// its engine (`luid_…_phys_0_eng_0`, the same for every process using it) and the engine's type
/// (`3D`). None for a name not of that form (`_Total`).
pub fn parse_instance(name: &str) -> Option<(u32, &str, &str)> {
    let rest = name.strip_prefix("pid_")?;
    let (pid, rest) = rest.split_once('_')?;
    let (engine, kind) = rest.rsplit_once("_engtype_")?;
    Some((pid.parse().ok()?, engine, kind))
}

/// The engine types whose work slows JETCARP's: drawing and compute (`3D`, `Compute_0`, `Cuda`,
/// `Graphics_1`), not video or copies.
fn pdh_counts(kind: &str) -> bool {
    let k = kind.to_ascii_lowercase();
    k == "3d" || k.starts_with("compute") || k.starts_with("cuda") || k.starts_with("graphics")
}

/// The busiest GPU engine's utilization (0 to 1) from the counters' instances, each `(name,
/// utilization percentage)`: each engine's processes summed, as Task Manager does, leaving out
/// process `own` (the load is other programs'). Only 3D and compute engines count. None when the
/// instances name no such engine.
pub fn pdh_busy<'a>(instances: impl IntoIterator<Item = (&'a str, f64)>, own: u32) -> Option<f64> {
    let mut engines: BTreeMap<String, f64> = BTreeMap::new();
    for (name, percent) in instances {
        let Some((pid, engine, kind)) = parse_instance(name) else {
            continue;
        };
        if !pdh_counts(kind) || !percent.is_finite() {
            continue;
        }
        let e = engines.entry(engine.to_ascii_lowercase()).or_insert(0.0);
        if pid != own {
            *e += percent.max(0.0);
        }
    }
    engines
        .into_values()
        .reduce(f64::max)
        .map(|p| (p / 100.0).clamp(0.0, 1.0))
}

/// The utilization (0 to 1) in one IOAccelerator's `PerformanceStatistics`, its numbers by key:
/// `Device Utilization %` (Apple's GPUs, and AMD's in an Intel Mac), else `GPU Activity(%)`
/// (older drivers). None when it gives neither.
pub fn iokit_busy(stats: &[(String, f64)]) -> Option<f64> {
    ["Device Utilization %", "GPU Activity(%)"]
        .iter()
        .find_map(|k| stats.iter().find(|(n, _)| n == k).map(|(_, v)| *v))
        .filter(|v| v.is_finite())
        .map(|v| (v / 100.0).clamp(0.0, 1.0))
}

/// One GPU client's counters, from its `fdinfo` (the kernel's `drm-usage-stats`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Client {
    /// `drm-driver`: `amdgpu`, `i915`, `xe`.
    pub driver: String,
    /// `drm-pdev`, the GPU's PCI address: `0000:c3:00.0`.
    pub pdev: String,
    /// `drm-client-id`: the same client open on several descriptors, or shared by processes, has
    /// one.
    pub id: String,
    /// Each engine's busy time so far, `drm-engine-<engine>`, in nanoseconds.
    pub engines: BTreeMap<String, u64>,
    /// Each engine's busy cycles and the GPU's cycles so far, `drm-cycles-<engine>` and
    /// `drm-total-cycles-<engine>` (xe).
    pub cycles: BTreeMap<String, (u64, u64)>,
    /// How many engines of a kind there are, `drm-engine-capacity-<engine>` (1 when not given).
    pub capacity: BTreeMap<String, u32>,
}

/// Every GPU client seen, by GPU and client id.
pub type Snapshot = BTreeMap<(String, String), Client>;

/// A GPU client's counters from one `fdinfo` file. None for a file that is not a GPU client's.
pub fn parse_fdinfo(text: &str) -> Option<Client> {
    let mut c = Client::default();
    let mut driver = false;
    let mut totals: BTreeMap<String, u64> = BTreeMap::new();
    let mut busy_cycles: BTreeMap<String, u64> = BTreeMap::new();
    for line in text.lines() {
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        let (key, value) = (key.trim(), value.trim());
        let number = || value.split_whitespace().next()?.parse::<u64>().ok();
        if key == "drm-driver" {
            driver = true;
            c.driver = value.to_string();
        } else if key == "drm-pdev" {
            c.pdev = value.to_string();
        } else if key == "drm-client-id" {
            c.id = value.to_string();
        } else if let Some(e) = key.strip_prefix("drm-engine-capacity-") {
            if let Some(n) = number() {
                c.capacity.insert(e.to_string(), n.max(1) as u32);
            }
        } else if let Some(e) = key.strip_prefix("drm-engine-") {
            if let Some(n) = value
                .strip_suffix("ns")
                .and_then(|n| n.trim().parse::<u64>().ok())
            {
                c.engines.insert(e.to_string(), n);
            }
        } else if let Some(e) = key.strip_prefix("drm-total-cycles-") {
            if let Some(n) = number() {
                totals.insert(e.to_string(), n);
            }
        } else if let Some(e) = key.strip_prefix("drm-cycles-")
            && let Some(n) = number()
        {
            busy_cycles.insert(e.to_string(), n);
        }
    }
    for (e, n) in busy_cycles {
        if let Some(&t) = totals.get(&e) {
            c.cycles.insert(e, (n, t));
        }
    }
    driver.then_some(c)
}

/// The engines whose work slows JETCARP's: drawing and compute (amdgpu's `gfx` and `compute`,
/// i915's `render` and `compute`, xe's `rcs` and `ccs`, and the single engines of other drivers),
/// not copies, video or the codecs.
fn fdinfo_counts(engine: &str) -> bool {
    const NOT: [&str; 13] = [
        "copy", "video", "vcs", "vecs", "bcs", "dec", "enc", "jpeg", "dma", "vcn", "uvd", "vce",
        "vpe",
    ];
    let e = engine.to_ascii_lowercase();
    !NOT.iter().any(|n| e.contains(n))
}

/// Each GPU's busy figure (0 to 1, by `drm-pdev`) between two snapshots `elapsed_ns` apart: per
/// engine, its clients' shares of the time summed, over the engine's capacity; the GPU's figure
/// is its busiest engine's. Only clients in both snapshots count (one seen once has nothing to
/// compare), and a GPU whose clients were idle reads zero.
pub fn fdinfo_busy(before: &Snapshot, after: &Snapshot, elapsed_ns: u64) -> BTreeMap<String, f64> {
    let mut per: BTreeMap<(&str, &str), f64> = BTreeMap::new();
    let mut gpus: BTreeMap<String, f64> = BTreeMap::new();
    for (key, a) in after {
        let Some(b) = before.get(key) else { continue };
        gpus.entry(a.pdev.clone()).or_insert(0.0);
        let cap = |e: &str| a.capacity.get(e).copied().unwrap_or(1).max(1) as f64;
        if elapsed_ns > 0 {
            for (e, &ns) in &a.engines {
                if !fdinfo_counts(e) {
                    continue;
                }
                // An engine a client has not used yet is not listed (amdgpu): none before.
                let was = b.engines.get(e).copied().unwrap_or(0);
                let share = ns.saturating_sub(was) as f64 / elapsed_ns as f64 / cap(e);
                *per.entry((a.pdev.as_str(), e.as_str())).or_insert(0.0) += share;
            }
        }
        for (e, &(n, t)) in &a.cycles {
            if !fdinfo_counts(e) {
                continue;
            }
            let (n0, t0) = b.cycles.get(e).copied().unwrap_or((0, t));
            let dt = t.saturating_sub(t0);
            if dt > 0 {
                let share = n.saturating_sub(n0) as f64 / dt as f64 / cap(e);
                *per.entry((a.pdev.as_str(), e.as_str())).or_insert(0.0) += share;
            }
        }
    }
    for ((pdev, _), share) in per {
        if let Some(g) = gpus.get_mut(pdev) {
            *g = g.max(share.clamp(0.0, 1.0));
        }
    }
    gpus
}

/// The GPU's load, read again and again from when it starts ([`Meter::read`]).
pub struct Meter(imp::Meter);

impl Meter {
    /// Start reading: a rate's first sample is taken now.
    pub fn start() -> Meter {
        Meter(imp::Meter::start())
    }

    /// The load since the last reading (for a rate) or now (for the drivers' own figures).
    pub fn read(&mut self) -> Reading {
        self.0.read()
    }
}

#[cfg(target_os = "linux")]
mod imp {
    use super::*;
    use std::path::PathBuf;
    use std::time::Instant;

    /// The GPUs under `/sys/class/drm`, by how their load is read.
    pub struct Meter {
        /// AMD's: their `gpu_busy_percent`.
        amd: Vec<PathBuf>,
        /// The rest but NVIDIA's (Intel's, others'): their PCI addresses, read from `fdinfo`.
        fd_gpus: Vec<String>,
        /// The last `fdinfo` snapshot, and when it was taken.
        fd: Option<(Instant, Snapshot)>,
        /// An NVIDIA GPU is here.
        nvidia: bool,
        nvml: Option<crate::machine::nvml::Session>,
    }

    impl Meter {
        pub fn start() -> Meter {
            let mut m = Meter {
                amd: vec![],
                fd_gpus: vec![],
                fd: None,
                nvidia: std::path::Path::new("/proc/driver/nvidia/gpus").exists(),
                nvml: None,
            };
            for e in std::fs::read_dir("/sys/class/drm")
                .into_iter()
                .flatten()
                .flatten()
            {
                let n = e.file_name();
                let n = n.to_string_lossy();
                if !n.starts_with("card") || n.contains('-') {
                    continue;
                }
                let dev = e.path().join("device");
                let vendor = std::fs::read_to_string(dev.join("vendor")).unwrap_or_default();
                let busy = dev.join("gpu_busy_percent");
                if vendor.trim() == "0x10de" {
                    m.nvidia = true;
                } else if vendor.trim() == "0x1002" && std::fs::read_to_string(&busy).is_ok() {
                    m.amd.push(busy);
                } else if let Some(pdev) = std::fs::canonicalize(&dev)
                    .ok()
                    .and_then(|p| Some(p.file_name()?.to_string_lossy().into_owned()))
                {
                    m.fd_gpus.push(pdev);
                }
            }
            if m.nvidia {
                m.nvml = crate::machine::nvml::Session::open();
            }
            if !m.fd_gpus.is_empty() {
                m.fd = Some((Instant::now(), fdinfo_snapshot()));
            }
            m
        }

        pub fn read(&mut self) -> Reading {
            let mut readings = vec![];
            for f in &self.amd {
                if let Some(p) = std::fs::read_to_string(f)
                    .ok()
                    .and_then(|t| t.trim().parse::<f64>().ok())
                {
                    readings.push((p / 100.0, GpuSource::Amdgpu));
                }
            }
            if let Some(b) = self.nvml.as_ref().and_then(|s| s.busy()) {
                readings.push((b, GpuSource::Nvml));
            }
            if let Some((at, before)) = &self.fd {
                let now = Instant::now();
                let after = fdinfo_snapshot();
                let ns = now.duration_since(*at).as_nanos() as u64;
                let busy = fdinfo_busy(before, &after, ns);
                for g in &self.fd_gpus {
                    // A GPU no program seen here uses: idle, as far as those programs go.
                    readings.push((busy.get(g).copied().unwrap_or(0.0), GpuSource::Fdinfo));
                }
                self.fd = Some((now, after));
            }
            let not_read = if self.nvidia && self.nvml.is_none() {
                Some(NO_NVML)
            } else if readings.is_empty() {
                Some(NO_FIGURE)
            } else {
                None
            };
            let b = busiest(readings);
            Reading {
                busy: b.map(|b| b.0),
                source: b.map(|b| b.1),
                not_read,
            }
        }
    }

    /// Every GPU client of every process this user can read, but this one's, by its `fdinfo`.
    pub fn fdinfo_snapshot() -> Snapshot {
        let own = std::process::id().to_string();
        let mut snap = Snapshot::new();
        for p in std::fs::read_dir("/proc").into_iter().flatten().flatten() {
            let pid = p.file_name();
            let pid = pid.to_string_lossy();
            if !pid.bytes().all(|b| b.is_ascii_digit()) || pid == own {
                continue;
            }
            let Ok(fds) = std::fs::read_dir(p.path().join("fd")) else {
                continue;
            };
            for fd in fds.flatten() {
                // Only a GPU's descriptors are read: a link to `/dev/dri/...`.
                if !std::fs::read_link(fd.path())
                    .is_ok_and(|l| l.as_os_str().as_encoded_bytes().starts_with(b"/dev/dri/"))
                {
                    continue;
                }
                let info = p.path().join("fdinfo").join(fd.file_name());
                if let Some(c) = std::fs::read_to_string(info)
                    .ok()
                    .and_then(|t| parse_fdinfo(&t))
                    .filter(|c| !c.pdev.is_empty())
                {
                    snap.insert((c.pdev.clone(), c.id.clone()), c);
                }
            }
        }
        snap
    }
}

#[cfg(windows)]
mod imp {
    use super::*;

    /// Windows' GPU engine counters, every vendor's.
    pub struct Meter {
        query: Option<pdh::Query>,
    }

    /// The counters can't be read here.
    const NO_PDH: &str = "Windows' GPU engine counters could not be read";

    impl Meter {
        pub fn start() -> Meter {
            Meter {
                query: pdh::Query::open(COUNTER_PATH).filter(|q| q.collect()),
            }
        }

        pub fn read(&mut self) -> Reading {
            let Some(q) = &self.query else {
                return Reading {
                    not_read: Some(NO_PDH),
                    ..Reading::default()
                };
            };
            let busy = if q.collect() {
                q.values().and_then(|v| {
                    pdh_busy(v.iter().map(|(n, p)| (n.as_str(), *p)), std::process::id())
                })
            } else {
                None
            };
            Reading {
                busy,
                source: busy.map(|_| GpuSource::Pdh),
                not_read: busy.is_none().then_some(NO_FIGURE),
            }
        }
    }
}

#[cfg(target_os = "macos")]
mod imp {
    use super::*;

    /// No IOAccelerator gives its utilization.
    const NO_IOKIT: &str = "no IOAccelerator gives its Device Utilization %";

    /// Each IOAccelerator's statistics, read afresh each time.
    pub struct Meter;

    impl Meter {
        pub fn start() -> Meter {
            Meter
        }

        pub fn read(&mut self) -> Reading {
            let b = iokit::accelerators()
                .iter()
                .filter_map(|s| iokit_busy(s))
                .reduce(f64::max);
            Reading {
                busy: b,
                source: b.map(|_| GpuSource::Iokit),
                not_read: b.is_none().then_some(NO_IOKIT),
            }
        }
    }

    /// IOKit and Core Foundation, the system's own frameworks.
    mod iokit {
        use std::ffi::{c_char, c_void};

        type CFTypeRef = *const c_void;
        type CFIndex = isize;

        #[link(name = "IOKit", kind = "framework")]
        unsafe extern "C" {
            fn IOServiceMatching(name: *const c_char) -> *mut c_void;
            fn IOServiceGetMatchingServices(main: u32, matching: *mut c_void, it: *mut u32) -> i32;
            fn IOIteratorNext(it: u32) -> u32;
            fn IOObjectRelease(o: u32) -> i32;
            fn IORegistryEntryCreateCFProperty(
                entry: u32,
                key: CFTypeRef,
                allocator: CFTypeRef,
                options: u32,
            ) -> CFTypeRef;
        }

        #[link(name = "CoreFoundation", kind = "framework")]
        unsafe extern "C" {
            fn CFStringCreateWithCString(
                alloc: CFTypeRef,
                s: *const c_char,
                encoding: u32,
            ) -> CFTypeRef;
            fn CFStringGetCString(s: CFTypeRef, buf: *mut c_char, size: CFIndex, enc: u32) -> u8;
            fn CFRelease(cf: CFTypeRef);
            fn CFGetTypeID(cf: CFTypeRef) -> usize;
            fn CFDictionaryGetTypeID() -> usize;
            fn CFStringGetTypeID() -> usize;
            fn CFNumberGetTypeID() -> usize;
            fn CFDictionaryGetCount(d: CFTypeRef) -> CFIndex;
            fn CFDictionaryGetKeysAndValues(d: CFTypeRef, k: *mut CFTypeRef, v: *mut CFTypeRef);
            fn CFNumberGetValue(n: CFTypeRef, kind: CFIndex, value: *mut c_void) -> u8;
        }

        const UTF8: u32 = 0x0800_0100;
        /// `kCFNumberDoubleType`.
        const DOUBLE: CFIndex = 13;

        /// Each IOAccelerator's `PerformanceStatistics`, its numbers by key.
        pub fn accelerators() -> Vec<Vec<(String, f64)>> {
            let mut out = vec![];
            // SAFETY: IOKit's and Core Foundation's calls as documented: the matching
            // dictionary is consumed by the lookup, each object and property got is released
            // once, and keys and values are read only while the dictionary lives.
            unsafe {
                let matching = IOServiceMatching(c"IOAccelerator".as_ptr());
                if matching.is_null() {
                    return out;
                }
                let mut it = 0u32;
                // kIOMainPortDefault is the null port.
                if IOServiceGetMatchingServices(0, matching, &mut it) != 0 {
                    return out;
                }
                let key = CFStringCreateWithCString(
                    std::ptr::null(),
                    c"PerformanceStatistics".as_ptr(),
                    UTF8,
                );
                loop {
                    let s = IOIteratorNext(it);
                    if s == 0 {
                        break;
                    }
                    if !key.is_null() {
                        let d = IORegistryEntryCreateCFProperty(s, key, std::ptr::null(), 0);
                        if !d.is_null() {
                            if CFGetTypeID(d) == CFDictionaryGetTypeID() {
                                out.push(numbers(d));
                            }
                            CFRelease(d);
                        }
                    }
                    IOObjectRelease(s);
                }
                if !key.is_null() {
                    CFRelease(key);
                }
                IOObjectRelease(it);
            }
            out
        }

        /// A dictionary's numbers, by their keys.
        ///
        /// # Safety
        /// `d` is a live CFDictionary.
        unsafe fn numbers(d: CFTypeRef) -> Vec<(String, f64)> {
            let mut out = vec![];
            // SAFETY: the caller's dictionary; the arrays hold as many as it says.
            unsafe {
                let n = CFDictionaryGetCount(d).max(0) as usize;
                let mut keys = vec![std::ptr::null(); n];
                let mut values = vec![std::ptr::null(); n];
                CFDictionaryGetKeysAndValues(d, keys.as_mut_ptr(), values.as_mut_ptr());
                for (k, v) in keys.into_iter().zip(values) {
                    if k.is_null()
                        || v.is_null()
                        || CFGetTypeID(k) != CFStringGetTypeID()
                        || CFGetTypeID(v) != CFNumberGetTypeID()
                    {
                        continue;
                    }
                    let mut buf = [0 as c_char; 256];
                    if CFStringGetCString(k, buf.as_mut_ptr(), buf.len() as CFIndex, UTF8) == 0 {
                        continue;
                    }
                    let mut x = 0f64;
                    CFNumberGetValue(v, DOUBLE, (&mut x as *mut f64).cast());
                    let name = std::ffi::CStr::from_ptr(buf.as_ptr()).to_string_lossy();
                    out.push((name.into_owned(), x));
                }
            }
            out
        }
    }
}

#[cfg(not(any(target_os = "linux", windows, target_os = "macos")))]
mod imp {
    use super::*;

    pub struct Meter;

    impl Meter {
        pub fn start() -> Meter {
            Meter
        }

        pub fn read(&mut self) -> Reading {
            Reading {
                not_read: Some("JETCARP doesn't read the GPU's load on this system"),
                ..Reading::default()
            }
        }
    }
}

/// The PDH calls (the Performance Data Helper, `pdh.dll`), through `windows-sys`: a query of one
/// counter, every instance's value. *Fit to this computer* reads this process's GPU engines with
/// it too (the host's `gpu_engine`).
#[cfg(windows)]
pub mod pdh {
    use windows_sys::Win32::System::Performance::{
        PDH_CSTATUS_NEW_DATA, PDH_CSTATUS_VALID_DATA, PDH_FMT_COUNTERVALUE_ITEM_W, PDH_FMT_DOUBLE,
        PDH_HCOUNTER, PDH_HQUERY, PDH_MORE_DATA, PdhAddEnglishCounterW, PdhCloseQuery,
        PdhCollectQueryData, PdhGetFormattedCounterArrayW, PdhOpenQueryW,
    };

    /// `ERROR_SUCCESS`, as PDH's functions return it.
    const OK: u32 = 0;

    /// A query with one counter, closed when dropped.
    pub struct Query {
        query: PDH_HQUERY,
        counter: PDH_HCOUNTER,
    }

    impl Query {
        /// A query of the live counters (no log file) with the counter `path` added.
        pub fn open(path: &str) -> Option<Query> {
            let mut query: PDH_HQUERY = std::ptr::null_mut();
            // SAFETY: a null data source means the live counters; `query` is written on success.
            if unsafe { PdhOpenQueryW(std::ptr::null(), 0, &mut query) } != OK {
                return None;
            }
            let wide: Vec<u16> = path.encode_utf16().chain(Some(0)).collect();
            let mut counter: PDH_HCOUNTER = std::ptr::null_mut();
            // SAFETY: `wide` is NUL-terminated and outlives the call; `counter` is written on
            // success.
            if unsafe { PdhAddEnglishCounterW(query, wide.as_ptr(), 0, &mut counter) } != OK {
                // SAFETY: the query was opened above and is closed once.
                unsafe { PdhCloseQuery(query) };
                return None;
            }
            Some(Query { query, counter })
        }

        /// Takes a sample of every instance. False when it could not.
        pub fn collect(&self) -> bool {
            // SAFETY: the query is open until `self` drops.
            unsafe { PdhCollectQueryData(self.query) == OK }
        }

        /// Each instance's value over the last two samples, by name, leaving out any without a
        /// valid value (an instance new since the sample before). None when they can't be had.
        pub fn values(&self) -> Option<Vec<(String, f64)>> {
            // Asked for its size first; asked again if more instances appeared in between.
            for _ in 0..4 {
                let (mut bytes, mut count) = (0u32, 0u32);
                // SAFETY: with a size of 0 and no buffer, PDH only writes the size it needs.
                let r = unsafe {
                    PdhGetFormattedCounterArrayW(
                        self.counter,
                        PDH_FMT_DOUBLE,
                        &mut bytes,
                        &mut count,
                        std::ptr::null_mut(),
                    )
                };
                match r {
                    OK => return Some(vec![]),
                    PDH_MORE_DATA => {}
                    _ => return None,
                }
                // Whole u64s: the items hold doubles and pointers, so the buffer must be 8-byte
                // aligned. The names are written after the items, in the same buffer.
                let mut buf = vec![0u64; (bytes as usize).div_ceil(8)];
                let items = buf.as_mut_ptr().cast::<PDH_FMT_COUNTERVALUE_ITEM_W>();
                // SAFETY: `buf` holds `bytes` bytes, suitably aligned, and outlives the call.
                let r = unsafe {
                    PdhGetFormattedCounterArrayW(
                        self.counter,
                        PDH_FMT_DOUBLE,
                        &mut bytes,
                        &mut count,
                        items,
                    )
                };
                match r {
                    OK => {}
                    PDH_MORE_DATA => continue,
                    _ => return None,
                }
                // SAFETY: PDH wrote `count` items at the start of `buf`, which is still alive.
                let items = unsafe { std::slice::from_raw_parts(items, count as usize) };
                let mut out = Vec::with_capacity(items.len());
                for it in items {
                    let status = it.FmtValue.CStatus;
                    if status != PDH_CSTATUS_VALID_DATA && status != PDH_CSTATUS_NEW_DATA {
                        continue;
                    }
                    // SAFETY: asked for PDH_FMT_DOUBLE, so the double is the value; the name is a
                    // NUL-terminated string inside `buf`.
                    let (value, name) =
                        unsafe { (it.FmtValue.Anonymous.doubleValue, wide_str(it.szName)) };
                    out.push((name, value));
                }
                return Some(out);
            }
            None
        }
    }

    impl Drop for Query {
        fn drop(&mut self) {
            // SAFETY: the query was opened in `open` and is closed only here (its counter with it).
            unsafe { PdhCloseQuery(self.query) };
        }
    }

    /// A NUL-terminated UTF-16 string as a `String`.
    ///
    /// # Safety
    /// `p` is null or points at a NUL-terminated UTF-16 string.
    unsafe fn wide_str(p: *const u16) -> String {
        if p.is_null() {
            return String::new();
        }
        let mut n = 0;
        // SAFETY: the caller promises a NUL-terminated string.
        unsafe {
            while *p.add(n) != 0 {
                n += 1;
            }
            String::from_utf16_lossy(std::slice::from_raw_parts(p, n))
        }
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

    /// Every process's GPU clients this user can read, twice, give figures between 0 and 1 (none
    /// where no program here has a GPU open, as on a runner without one).
    #[cfg(target_os = "linux")]
    #[test]
    fn fdinfo_is_read_from_every_process_here() {
        let t = std::time::Instant::now();
        let a = imp::fdinfo_snapshot();
        std::thread::sleep(std::time::Duration::from_millis(200));
        let b = imp::fdinfo_snapshot();
        let busy = fdinfo_busy(&a, &b, t.elapsed().as_nanos() as u64);
        eprintln!("{} GPU clients here; by GPU: {busy:?}", b.len());
        assert!(busy.values().all(|b| (0.0..=1.0).contains(b)), "{busy:?}");
        assert!(b.keys().all(|(pdev, _)| !pdev.is_empty()));
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
