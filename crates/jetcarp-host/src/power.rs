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
