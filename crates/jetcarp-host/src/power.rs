//! What the computer is doing, for the host's pause rules: whether it runs on battery, and how
//! busy its processor is.

/// Whether the computer is running on battery. None when it can't be told; false when it has no
/// battery.
pub fn on_battery() -> Option<bool> {
    imp::on_battery()
}

/// The share of all processor time that was busy between one sample and the next.
#[derive(Default)]
pub struct CpuMeter {
    last: Option<(u64, u64)>,
}

impl CpuMeter {
    /// Busy share since the last call, 0 to 1. None on the first call, or when it can't be read.
    pub fn sample(&mut self) -> Option<f64> {
        let (busy, total) = imp::cpu_times()?;
        let before = self.last.replace((busy, total));
        let (b0, t0) = before?;
        let dt = total.checked_sub(t0)?;
        if dt == 0 {
            return None;
        }
        Some((busy.saturating_sub(b0) as f64 / dt as f64).clamp(0.0, 1.0))
    }
}

/// The processor time this process has used so far, on all its threads (the GPU driver's
/// included), from the system's own accounting: `getrusage` on Linux and macOS,
/// `GetProcessTimes` on Windows. None where it can't be read.
pub fn process_cpu_time() -> Option<std::time::Duration> {
    process_time()
}

/// This process's share of the machine's processor time between one sample and the next: its
/// processor time per second of wall-clock time, over every core the process may use.
pub struct ProcessMeter {
    wall: std::time::Instant,
    cpu: std::time::Duration,
    cores: f64,
}

impl ProcessMeter {
    /// Starts measuring now. None where the process's processor time can't be read.
    pub fn start() -> Option<ProcessMeter> {
        Some(ProcessMeter {
            wall: std::time::Instant::now(),
            cpu: process_cpu_time()?,
            cores: std::thread::available_parallelism().map_or(1, |n| n.get()) as f64,
        })
    }

    /// The share (0 to 1) since [`start`](Self::start).
    pub fn share(&self) -> f64 {
        let wall = self.wall.elapsed().as_secs_f64();
        let cpu = process_cpu_time()
            .unwrap_or(self.cpu)
            .saturating_sub(self.cpu)
            .as_secs_f64();
        if wall <= 0.0 {
            return 0.0;
        }
        (cpu / wall / self.cores).clamp(0.0, 1.0)
    }
}

/// The GPU time this process has used so far, as the kernel counts it for each program (Linux's
/// DRM `fdinfo`: every engine's `drm-engine-*` time, each GPU client once). None where the system
/// does not count it, or before this process has opened a GPU.
pub fn process_gpu_time() -> Option<std::time::Duration> {
    #[cfg(target_os = "linux")]
    {
        let mut seen = std::collections::BTreeSet::new();
        let mut total: Option<u64> = None;
        for e in std::fs::read_dir("/proc/self/fdinfo").ok()?.flatten() {
            let Ok(text) = std::fs::read_to_string(e.path()) else {
                continue;
            };
            if let Some(ns) = drm_engine_ns(&text, &mut seen) {
                *total.get_or_insert(0) += ns;
            }
        }
        total.map(std::time::Duration::from_nanos)
    }
    #[cfg(not(target_os = "linux"))]
    None
}

/// The engine time in one `fdinfo` file of a GPU client not already in `seen` (the same client
/// can be open on several descriptors). None for a file that is not a GPU client's.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn drm_engine_ns(text: &str, seen: &mut std::collections::BTreeSet<String>) -> Option<u64> {
    let mut driver = false;
    let mut client = None;
    let mut ns = 0u64;
    for line in text.lines() {
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        let value = value.trim();
        match key.trim() {
            "drm-driver" => driver = true,
            "drm-client-id" => client = Some(value.to_string()),
            k if k.starts_with("drm-engine-") && !k.starts_with("drm-engine-capacity-") => {
                if let Some(n) = value
                    .strip_suffix("ns")
                    .and_then(|n| n.trim().parse::<u64>().ok())
                {
                    ns += n;
                }
            }
            _ => {}
        }
    }
    if !driver {
        return None;
    }
    match client {
        Some(c) if !seen.insert(c.clone()) => Some(0),
        _ => Some(ns),
    }
}

#[cfg(unix)]
fn process_time() -> Option<std::time::Duration> {
    let mut u: libc::rusage = unsafe { std::mem::zeroed() };
    if unsafe { libc::getrusage(libc::RUSAGE_SELF, &mut u) } != 0 {
        return None;
    }
    let tv = |t: libc::timeval| {
        std::time::Duration::new(t.tv_sec.max(0) as u64, (t.tv_usec.max(0) as u32) * 1000)
    };
    Some(tv(u.ru_utime) + tv(u.ru_stime))
}

#[cfg(windows)]
fn process_time() -> Option<std::time::Duration> {
    use windows_sys::Win32::Foundation::FILETIME;
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, GetProcessTimes};
    let mut t: [FILETIME; 4] = unsafe { std::mem::zeroed() };
    let [created, exited, kernel, user] = &mut t;
    if unsafe { GetProcessTimes(GetCurrentProcess(), created, exited, kernel, user) } == 0 {
        return None;
    }
    let ticks = |f: &FILETIME| (f.dwHighDateTime as u64) << 32 | f.dwLowDateTime as u64;
    // In units of 100 ns.
    Some(std::time::Duration::from_nanos(
        (ticks(&t[2]) + ticks(&t[3])).saturating_mul(100),
    ))
}

#[cfg(not(any(unix, windows)))]
fn process_time() -> Option<std::time::Duration> {
    None
}

/// Samples in a row over (or well under) the limit before the answer changes.
const BUSY_SAMPLES: u32 = 3;
/// Calm needs the load this many points under the limit.
const BUSY_MARGIN: f64 = 20.0;

/// Whether the processor counts as busy, from load samples: busy after [`BUSY_SAMPLES`] at or
/// over the limit, calm again after as many [`BUSY_MARGIN`] points under it.
#[derive(Default)]
pub struct BusyRule {
    busy: bool,
    over: u32,
    under: u32,
}

impl BusyRule {
    /// Takes a load in percent and the limit (0: never busy); answers whether it is busy now.
    pub fn feed(&mut self, load: f64, limit: u8) -> bool {
        let limit = limit as f64;
        if limit <= 0.0 {
            *self = BusyRule::default();
            return false;
        }
        if load >= limit {
            self.over += 1;
            self.under = 0;
        } else if load < limit - BUSY_MARGIN {
            self.under += 1;
            self.over = 0;
        } else {
            self.over = 0;
            self.under = 0;
        }
        if self.over >= BUSY_SAMPLES {
            self.busy = true;
        } else if self.under >= BUSY_SAMPLES {
            self.busy = false;
        }
        self.busy
    }
}

#[cfg(target_os = "linux")]
mod imp {
    use std::fs;
    use std::path::Path;

    fn read(p: &Path, name: &str) -> String {
        fs::read_to_string(p.join(name))
            .map(|s| s.trim().to_string())
            .unwrap_or_default()
    }

    pub fn on_battery() -> Option<bool> {
        let dir = fs::read_dir("/sys/class/power_supply").ok()?;
        let (mut mains, mut discharging) = (false, false);
        for e in dir.flatten() {
            let p = e.path();
            match read(&p, "type").as_str() {
                "Mains" | "USB" if read(&p, "online") == "1" => mains = true,
                // A mouse's or keyboard's battery is not the computer's.
                "Battery" if read(&p, "scope") != "Device" => {
                    discharging |= read(&p, "status") == "Discharging";
                }
                _ => {}
            }
        }
        Some(discharging && !mains)
    }

    /// (busy, total) in clock ticks, from the first line of `/proc/stat`.
    pub fn cpu_times() -> Option<(u64, u64)> {
        let stat = fs::read_to_string("/proc/stat").ok()?;
        let line = stat.lines().next()?.strip_prefix("cpu ")?;
        let v: Vec<u64> = line
            .split_whitespace()
            .take(8)
            .filter_map(|n| n.parse().ok())
            .collect();
        if v.len() < 5 {
            return None;
        }
        let total: u64 = v.iter().sum();
        // idle and iowait
        Some((total - v[3] - v[4], total))
    }
}

#[cfg(windows)]
mod imp {
    use windows_sys::Win32::Foundation::FILETIME;
    use windows_sys::Win32::System::Power::{GetSystemPowerStatus, SYSTEM_POWER_STATUS};
    use windows_sys::Win32::System::Threading::GetSystemTimes;

    pub fn on_battery() -> Option<bool> {
        let mut s: SYSTEM_POWER_STATUS = unsafe { std::mem::zeroed() };
        if unsafe { GetSystemPowerStatus(&mut s) } == 0 {
            return None;
        }
        match s.ACLineStatus {
            0 => Some(true),
            1 => Some(false),
            _ => None,
        }
    }

    fn ticks(f: &FILETIME) -> u64 {
        (f.dwHighDateTime as u64) << 32 | f.dwLowDateTime as u64
    }

    pub fn cpu_times() -> Option<(u64, u64)> {
        let mut idle: FILETIME = unsafe { std::mem::zeroed() };
        let mut kernel = idle;
        let mut user = idle;
        if unsafe { GetSystemTimes(&mut idle, &mut kernel, &mut user) } == 0 {
            return None;
        }
        // Kernel time includes idle time.
        let total = ticks(&kernel) + ticks(&user);
        Some((total.saturating_sub(ticks(&idle)), total))
    }
}

#[cfg(target_os = "macos")]
mod imp {
    use std::ffi::{c_char, c_void};
    use std::sync::OnceLock;

    #[link(name = "IOKit", kind = "framework")]
    unsafe extern "C" {
        fn IOPSCopyPowerSourcesInfo() -> *const c_void;
        fn IOPSGetProvidingPowerSourceType(snapshot: *const c_void) -> *const c_void;
    }

    #[link(name = "CoreFoundation", kind = "framework")]
    unsafe extern "C" {
        fn CFRelease(cf: *const c_void);
        fn CFStringGetCString(s: *const c_void, buf: *mut c_char, len: isize, enc: u32) -> u8;
    }

    const UTF8: u32 = 0x0800_0100;

    pub fn on_battery() -> Option<bool> {
        unsafe {
            let info = IOPSCopyPowerSourcesInfo();
            if info.is_null() {
                return None;
            }
            // Not owned (the Get rule), so only `info` is released.
            let kind = IOPSGetProvidingPowerSourceType(info);
            let mut buf = [0 as c_char; 64];
            let ok = !kind.is_null()
                && CFStringGetCString(kind, buf.as_mut_ptr(), buf.len() as isize, UTF8) != 0;
            CFRelease(info);
            if !ok {
                return None;
            }
            let text = std::ffi::CStr::from_ptr(buf.as_ptr()).to_string_lossy();
            Some(text == "Battery Power")
        }
    }

    unsafe extern "C" {
        fn mach_host_self() -> u32;
        fn host_statistics(host: u32, flavor: i32, info: *mut i32, count: *mut u32) -> i32;
    }

    const HOST_CPU_LOAD_INFO: i32 = 3;

    pub fn cpu_times() -> Option<(u64, u64)> {
        // Each call to mach_host_self takes a port right, so it is taken once.
        static HOST: OnceLock<u32> = OnceLock::new();
        let host = *HOST.get_or_init(|| unsafe { mach_host_self() });
        // user, system, idle, nice
        let mut ticks = [0u32; 4];
        let mut count = 4u32;
        let r = unsafe {
            host_statistics(
                host,
                HOST_CPU_LOAD_INFO,
                ticks.as_mut_ptr().cast(),
                &mut count,
            )
        };
        if r != 0 {
            return None;
        }
        let total: u64 = ticks.iter().map(|&t| t as u64).sum();
        Some((total - ticks[2] as u64, total))
    }
}

#[cfg(not(any(target_os = "linux", windows, target_os = "macos")))]
mod imp {
    pub fn on_battery() -> Option<bool> {
        None
    }
    pub fn cpu_times() -> Option<(u64, u64)> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn busy_needs_a_run_over_the_limit_and_calm_a_run_well_under() {
        let mut r = BusyRule::default();
        assert!(
            !r.feed(95.0, 80) && !r.feed(95.0, 80),
            "two samples are not enough"
        );
        assert!(r.feed(95.0, 80));
        assert!(
            r.feed(70.0, 80),
            "between the limit and the margin: still busy"
        );
        assert!(r.feed(50.0, 80) && r.feed(50.0, 80));
        assert!(!r.feed(50.0, 80), "three calm samples");
        assert!(!r.feed(85.0, 80) && !r.feed(70.0, 80) && !r.feed(85.0, 80) && !r.feed(85.0, 80));
        assert!(r.feed(85.0, 80));
        assert!(!r.feed(100.0, 0), "0 turns it off");
    }

    #[test]
    fn this_process_s_processor_time_counts_every_thread() {
        let before = process_cpu_time().expect("this platform reads it");
        let meter = ProcessMeter::start().unwrap();
        // Two threads busy for about 300 ms each.
        let spin = || {
            let t = std::time::Instant::now();
            let mut x = 0u64;
            while t.elapsed() < std::time::Duration::from_millis(300) {
                x = x.wrapping_mul(6364136223846793005).wrapping_add(1);
            }
            std::hint::black_box(x);
        };
        std::thread::scope(|s| {
            s.spawn(spin);
            s.spawn(spin);
        });
        let used = process_cpu_time().unwrap().saturating_sub(before);
        // Both threads count (allowing for a busy test machine that gives them less).
        assert!(
            used >= std::time::Duration::from_millis(350),
            "only {used:?} of about 600 ms"
        );
        let share = meter.share();
        let cores = std::thread::available_parallelism().map_or(1, |n| n.get()) as f64;
        // About two cores' worth of the machine, give or take.
        assert!(share > 0.0 && share <= 1.0, "{share}");
        assert!(share * cores <= 2.5, "{share} of {cores} cores");
    }

    #[test]
    fn gpu_time_is_read_from_each_gpu_client_once() {
        let fd = "pos:\t0\ndrm-driver:\tamdgpu\ndrm-client-id:\t7\ndrm-engine-gfx:\t1500 ns\n\
                  drm-engine-compute:\t500 ns\ndrm-engine-capacity-gfx:\t2\ndrm-total-vram:\t12 KiB\n";
        let mut seen = std::collections::BTreeSet::new();
        assert_eq!(drm_engine_ns(fd, &mut seen), Some(2000));
        assert_eq!(
            drm_engine_ns(fd, &mut seen),
            Some(0),
            "the same client again"
        );
        assert_eq!(
            drm_engine_ns("pos:\t0\nflags:\t02\n", &mut seen),
            None,
            "not a GPU"
        );
        let idle = "drm-driver:\ti915\ndrm-client-id:\t9\n";
        assert_eq!(drm_engine_ns(idle, &mut seen), Some(0), "no work yet");
        // Whatever this machine is, reading it is not an error.
        let _ = process_gpu_time();
    }

    #[test]
    fn the_processor_load_reads_as_a_share() {
        let mut m = CpuMeter::default();
        assert_eq!(
            m.sample(),
            None,
            "the first sample has nothing to compare with"
        );
        std::thread::sleep(std::time::Duration::from_millis(200));
        if let Some(f) = m.sample() {
            assert!((0.0..=1.0).contains(&f));
        }
        // Whatever this machine is, the answer is not an error.
        let _ = on_battery();
    }
}
