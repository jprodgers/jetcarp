//! What the machine holds, read from the system in milliseconds, with no benchmark: memory, the
//! processor and its cores, and each GPU's memory where the system says it.
//!
//! - **Linux**: `/proc/meminfo` (MemTotal, MemAvailable), `/proc/cpuinfo`, and for AMD and Intel
//!   GPUs `/sys/class/drm/card*/device/mem_info_vram_total` and `mem_info_gtt_total`.
//! - **Windows**: `GlobalMemoryStatusEx`, the registry's `ProcessorNameString`, and DXGI's
//!   dedicated and shared memory for each adapter.
//! - **macOS**: `sysctl` (`hw.memsize`, `machdep.cpu.brand_string`, `hw.physicalcpu`) and the
//!   virtual memory statistics; the GPU shares the one pool of memory.
//!
//! A GPU whose memory cannot be read is left out here; its limits then come from its buffer
//! limits and a cautious share of system memory, said as an estimate ([`super::limits`]).

use serde::{Deserialize, Serialize};

/// What the machine holds.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Hardware {
    /// `linux`, `windows`, `macos`.
    pub os: String,
    /// `x86_64`, `aarch64`.
    pub arch: String,
    /// The processor's model, as the system names it.
    pub cpu: String,
    /// Threads JETCARP can run at once.
    pub threads: usize,
    /// Physical cores, where the system says.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cores: Option<usize>,
    /// Memory the system has, in bytes (on a machine whose firmware sets aside part of it for
    /// the GPU, what is left).
    pub memory_total: u64,
    /// Memory free for programs now, in bytes, where the system says.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub memory_available: Option<u64>,
    /// Each GPU whose memory the system reports.
    #[serde(default)]
    pub gpus: Vec<GpuMemory>,
}

/// One GPU's memory as the system reports it.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct GpuMemory {
    /// Its name, where the system gives one (DXGI, macOS); else empty.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub name: String,
    /// PCI vendor and device ids (0 when unknown), to match wgpu's adapters.
    pub vendor: u32,
    pub device: u32,
    /// Its own memory (VRAM, or the share the firmware sets aside on an integrated GPU), bytes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dedicated: Option<u64>,
    /// System memory it may borrow besides (Linux's GTT, DXGI's shared memory), bytes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shared: Option<u64>,
    /// It uses the system's memory (Apple silicon): its worlds count against the same pool.
    #[serde(default)]
    pub unified: bool,
    /// Where it was read: `sysfs`, `DXGI`, `unified memory`.
    pub source: String,
}

impl Hardware {
    /// This machine, read now.
    pub fn read() -> Hardware {
        let threads = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(1);
        let mut h = Hardware {
            os: std::env::consts::OS.into(),
            arch: std::env::consts::ARCH.into(),
            threads,
            ..Default::default()
        };
        sys::read(&mut h);
        if h.cpu.is_empty() {
            h.cpu = "unknown processor".into();
        }
        h
    }

    /// The machine's identity for noticing a change: its system, processor, threads, memory
    /// (to the nearest GiB, so a new firmware split counts and the kernel's small changes do
    /// not) and GPUs with their memory. Free memory is left out.
    pub fn fingerprint(&self) -> String {
        let gib = |b: u64| (b + (1 << 29)) >> 30;
        let mut s = format!(
            "{}|{}|{}|{}|{}",
            self.os,
            self.arch,
            self.cpu.trim(),
            self.threads,
            gib(self.memory_total)
        );
        let mut gpus: Vec<String> = self
            .gpus
            .iter()
            .map(|g| {
                format!(
                    "{:04x}:{:04x}:{}:{}",
                    g.vendor,
                    g.device,
                    g.dedicated.map_or(0, gib),
                    g.unified
                )
            })
            .collect();
        gpus.sort();
        for g in gpus {
            s.push('|');
            s.push_str(&g);
        }
        crate::sha256::sha256_hex(s.as_bytes())[..16].to_string()
    }
}

/// `MemTotal` and `MemAvailable` (bytes) from `/proc/meminfo`'s text.
pub fn parse_meminfo(text: &str) -> (Option<u64>, Option<u64>) {
    let field = |name: &str| {
        text.lines().find_map(|l| {
            let rest = l.strip_prefix(name)?.strip_prefix(':')?;
            let mut it = rest.split_whitespace();
            let n: u64 = it.next()?.parse().ok()?;
            Some(match it.next() {
                Some("kB") | Some("KB") => n * 1024,
                Some("mB") | Some("MB") => n << 20,
                _ => n,
            })
        })
    };
    (field("MemTotal"), field("MemAvailable"))
}

/// The processor's model and its physical cores from `/proc/cpuinfo`'s text: `model name`
/// (x86), else `Hardware`, `Model` or `cpu model` (Arm and others); cores from the distinct
/// `physical id` and `core id` pairs, where given.
pub fn parse_cpuinfo(text: &str) -> (String, Option<usize>) {
    let value = |l: &str, key: &str| -> Option<String> {
        let (k, v) = l.split_once(':')?;
        (k.trim() == key).then(|| v.trim().to_string())
    };
    let mut name = None;
    for key in ["model name", "Hardware", "Model", "cpu model"] {
        name = text
            .lines()
            .find_map(|l| value(l, key))
            .filter(|v| !v.is_empty());
        if name.is_some() {
            break;
        }
    }
    let mut cores = std::collections::BTreeSet::new();
    let mut phys = String::new();
    for l in text.lines() {
        if let Some(v) = value(l, "physical id") {
            phys = v;
        } else if let Some(v) = value(l, "core id") {
            cores.insert((phys.clone(), v));
        }
    }
    (
        squash_spaces(&name.unwrap_or_default()),
        (!cores.is_empty()).then_some(cores.len()),
    )
}

/// Runs of spaces as one (Intel's model names pad with them).
fn squash_spaces(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// A number in a sysfs file: decimal, or hexadecimal with `0x`.
pub fn parse_sysfs_number(text: &str) -> Option<u64> {
    let t = text.trim();
    match t.strip_prefix("0x") {
        Some(h) => u64::from_str_radix(h, 16).ok(),
        None => t.parse().ok(),
    }
}

/// The GPUs under a DRM folder (`/sys/class/drm`): each `cardN` whose device reports its memory
/// (amdgpu's `mem_info_vram_total`, and `mem_info_gtt_total`). Connectors (`card0-DP-1`) are
/// skipped, and a device seen through two cards once.
pub fn read_drm(dir: &std::path::Path) -> Vec<GpuMemory> {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return vec![];
    };
    let mut names: Vec<String> = rd
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| {
            n.strip_prefix("card")
                .is_some_and(|d| !d.is_empty() && d.bytes().all(|b| b.is_ascii_digit()))
        })
        .collect();
    names.sort_by_key(|n| n[4..].parse::<u32>().unwrap_or(u32::MAX));
    let mut out: Vec<GpuMemory> = vec![];
    let mut seen = vec![];
    for n in names {
        let dev = dir.join(&n).join("device");
        let read = |f: &str| {
            std::fs::read_to_string(dev.join(f))
                .ok()
                .and_then(|t| parse_sysfs_number(&t))
        };
        let Some(vram) = read("mem_info_vram_total") else {
            continue;
        };
        let real = std::fs::canonicalize(&dev).unwrap_or(dev.clone());
        if seen.contains(&real) {
            continue;
        }
        seen.push(real);
        out.push(GpuMemory {
            name: String::new(),
            vendor: read("vendor").unwrap_or(0) as u32,
            device: read("device").unwrap_or(0) as u32,
            dedicated: Some(vram),
            shared: read("mem_info_gtt_total"),
            unified: false,
            source: "sysfs".into(),
        });
    }
    out
}

#[cfg(target_os = "linux")]
mod sys {
    use super::*;

    pub fn read(h: &mut Hardware) {
        if let Ok(t) = std::fs::read_to_string("/proc/meminfo") {
            let (total, avail) = parse_meminfo(&t);
            h.memory_total = total.unwrap_or(0);
            h.memory_available = avail;
        }
        if let Ok(t) = std::fs::read_to_string("/proc/cpuinfo") {
            let (name, cores) = parse_cpuinfo(&t);
            h.cpu = name;
            h.cores = cores;
        }
        h.gpus = read_drm(std::path::Path::new("/sys/class/drm"));
    }
}

#[cfg(windows)]
mod sys {
    use super::*;
    use windows_sys::Win32::System::Registry::{HKEY_LOCAL_MACHINE, RRF_RT_REG_SZ, RegGetValueW};
    use windows_sys::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    fn processor_name() -> Option<String> {
        let key = wide(r"HARDWARE\DESCRIPTION\System\CentralProcessor\0");
        let value = wide("ProcessorNameString");
        let mut buf = [0u16; 256];
        let mut len = (buf.len() * 2) as u32;
        // SAFETY: the key and value names end in NUL, and `len` is the buffer's size in bytes.
        let r = unsafe {
            RegGetValueW(
                HKEY_LOCAL_MACHINE,
                key.as_ptr(),
                value.as_ptr(),
                RRF_RT_REG_SZ,
                std::ptr::null_mut(),
                buf.as_mut_ptr().cast(),
                &mut len,
            )
        };
        if r != 0 {
            return None;
        }
        let n = (len as usize / 2).min(buf.len());
        let s = String::from_utf16_lossy(&buf[..n]);
        Some(squash_spaces(s.trim_end_matches('\0')))
    }

    fn adapters() -> Vec<GpuMemory> {
        use windows::Win32::Graphics::Dxgi::{
            CreateDXGIFactory1, DXGI_ADAPTER_FLAG_SOFTWARE, IDXGIFactory1,
        };
        // SAFETY: plain COM calls; each interface is released when dropped.
        let Ok(f) = (unsafe { CreateDXGIFactory1::<IDXGIFactory1>() }) else {
            return vec![];
        };
        let mut out = vec![];
        for i in 0.. {
            let Ok(a) = (unsafe { f.EnumAdapters1(i) }) else {
                break;
            };
            let Ok(d) = (unsafe { a.GetDesc1() }) else {
                continue;
            };
            if d.Flags & DXGI_ADAPTER_FLAG_SOFTWARE.0 as u32 != 0 {
                continue;
            }
            let n = d
                .Description
                .iter()
                .position(|&c| c == 0)
                .unwrap_or(d.Description.len());
            out.push(GpuMemory {
                name: squash_spaces(&String::from_utf16_lossy(&d.Description[..n])),
                vendor: d.VendorId,
                device: d.DeviceId,
                dedicated: Some(d.DedicatedVideoMemory as u64),
                shared: Some(d.SharedSystemMemory as u64),
                unified: false,
                source: "DXGI".into(),
            });
        }
        out
    }

    pub fn read(h: &mut Hardware) {
        let mut m: MEMORYSTATUSEX = unsafe { std::mem::zeroed() };
        m.dwLength = std::mem::size_of::<MEMORYSTATUSEX>() as u32;
        // SAFETY: `m` is a MEMORYSTATUSEX with its length set, as the call requires.
        if unsafe { GlobalMemoryStatusEx(&mut m) } != 0 {
            h.memory_total = m.ullTotalPhys;
            h.memory_available = Some(m.ullAvailPhys);
        }
        h.cpu = processor_name().unwrap_or_default();
        h.gpus = adapters();
    }
}

#[cfg(target_os = "macos")]
mod sys {
    use super::*;

    fn sysctl_u64(name: &str) -> Option<u64> {
        let c = std::ffi::CString::new(name).ok()?;
        let mut v: u64 = 0;
        let mut len = std::mem::size_of::<u64>();
        // SAFETY: `c` ends in NUL and `v` has the room `len` says.
        let r = unsafe {
            libc::sysctlbyname(
                c.as_ptr(),
                (&mut v as *mut u64).cast(),
                &mut len,
                std::ptr::null_mut(),
                0,
            )
        };
        match (r, len) {
            (0, 8) => Some(v),
            (0, 4) => Some(v & 0xffff_ffff),
            _ => None,
        }
    }

    fn sysctl_text(name: &str) -> Option<String> {
        let c = std::ffi::CString::new(name).ok()?;
        let mut buf = [0u8; 256];
        let mut len = buf.len();
        // SAFETY: `c` ends in NUL and `buf` has the room `len` says.
        let r = unsafe {
            libc::sysctlbyname(
                c.as_ptr(),
                buf.as_mut_ptr().cast(),
                &mut len,
                std::ptr::null_mut(),
                0,
            )
        };
        if r != 0 {
            return None;
        }
        let n = buf[..len.min(buf.len())]
            .iter()
            .position(|&b| b == 0)
            .unwrap_or(len.min(buf.len()));
        Some(squash_spaces(&String::from_utf8_lossy(&buf[..n])))
    }

    /// Free, inactive and purgeable pages: memory programs can have without swapping.
    #[allow(deprecated)]
    fn available() -> Option<u64> {
        let mut info: libc::vm_statistics64 = unsafe { std::mem::zeroed() };
        let mut count = (std::mem::size_of::<libc::vm_statistics64>()
            / std::mem::size_of::<libc::integer_t>())
            as libc::mach_msg_type_number_t;
        // SAFETY: `info` has the room `count` says, in `integer_t`s, as the call requires.
        let r = unsafe {
            libc::host_statistics64(
                libc::mach_host_self(),
                libc::HOST_VM_INFO64,
                (&mut info as *mut libc::vm_statistics64).cast(),
                &mut count,
            )
        };
        if r != 0 {
            return None;
        }
        let page = sysctl_u64("hw.pagesize").unwrap_or(16384);
        Some(
            (info.free_count as u64 + info.inactive_count as u64 + info.purgeable_count as u64)
                * page,
        )
    }

    pub fn read(h: &mut Hardware) {
        h.memory_total = sysctl_u64("hw.memsize").unwrap_or(0);
        h.memory_available = available();
        h.cpu = sysctl_text("machdep.cpu.brand_string").unwrap_or_default();
        h.cores = sysctl_u64("hw.physicalcpu").map(|n| n as usize);
        // Apple silicon: one pool of memory for the processor and the GPU.
        if std::env::consts::ARCH == "aarch64" {
            h.gpus = vec![GpuMemory {
                name: h.cpu.clone(),
                vendor: 0x106b,
                device: 0,
                dedicated: None,
                shared: Some(h.memory_total),
                unified: true,
                source: "unified memory".into(),
            }];
        }
    }
}

#[cfg(not(any(target_os = "linux", windows, target_os = "macos")))]
mod sys {
    use super::*;
    pub fn read(_: &mut Hardware) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn meminfo_in_bytes() {
        let t = "MemTotal:       98304000 kB\nMemFree:  1000 kB\nMemAvailable:   65536000 kB\n";
        assert_eq!(
            parse_meminfo(t),
            (Some(98_304_000 * 1024), Some(65_536_000 * 1024))
        );
        assert_eq!(parse_meminfo("MemTotal: 1024\n"), (Some(1024), None));
        assert_eq!(parse_meminfo(""), (None, None));
        // `MemTotalX` is not `MemTotal`.
        assert_eq!(parse_meminfo("MemTotalX: 5 kB\n"), (None, None));
    }

    #[test]
    fn cpuinfo_model_and_cores() {
        let x86 = "processor\t: 0\nmodel name\t: AMD Ryzen  AI Max+ 395\nphysical id\t: 0\ncore id\t: 0\n\n\
                   processor\t: 1\nmodel name\t: AMD Ryzen  AI Max+ 395\nphysical id\t: 0\ncore id\t: 0\n\n\
                   processor\t: 2\nmodel name\t: AMD Ryzen  AI Max+ 395\nphysical id\t: 0\ncore id\t: 1\n";
        assert_eq!(
            parse_cpuinfo(x86),
            ("AMD Ryzen AI Max+ 395".to_string(), Some(2))
        );
        let arm = "processor\t: 0\nBogoMIPS\t: 48.00\n\nHardware\t: Qualcomm Snapdragon\n";
        assert_eq!(
            parse_cpuinfo(arm),
            ("Qualcomm Snapdragon".to_string(), None)
        );
        assert_eq!(parse_cpuinfo(""), (String::new(), None));
    }

    #[test]
    fn sysfs_numbers() {
        assert_eq!(parse_sysfs_number("0x1002\n"), Some(0x1002));
        assert_eq!(parse_sysfs_number("34359738368\n"), Some(34_359_738_368));
        assert_eq!(parse_sysfs_number("n/a"), None);
    }

    #[test]
    fn drm_cards_with_memory() {
        let d = crate::scratch::ScratchDir::new("machine-drm");
        let card = |n: &str, files: &[(&str, &str)]| {
            let dev = d.join(n).join("device");
            std::fs::create_dir_all(&dev).unwrap();
            for (f, v) in files {
                std::fs::write(dev.join(f), v).unwrap();
            }
        };
        card(
            "card1",
            &[
                ("vendor", "0x1002\n"),
                ("device", "0x1586\n"),
                ("mem_info_vram_total", "34359738368\n"),
                ("mem_info_gtt_total", "51539607552\n"),
            ],
        );
        // No memory files (another driver): left out. A connector: skipped.
        card("card0", &[("vendor", "0x8086\n"), ("device", "0x46a6\n")]);
        std::fs::create_dir_all(d.join("card1-DP-1")).unwrap();
        let g = read_drm(&d);
        assert_eq!(g.len(), 1);
        assert_eq!((g[0].vendor, g[0].device), (0x1002, 0x1586));
        assert_eq!(g[0].dedicated, Some(32 << 30));
        assert_eq!(g[0].shared, Some(48 << 30));
        assert_eq!(read_drm(&d.join("missing")), vec![]);
    }

    #[test]
    fn the_fingerprint_follows_what_matters() {
        let h = Hardware {
            os: "linux".into(),
            arch: "x86_64".into(),
            cpu: "A processor".into(),
            threads: 16,
            cores: Some(8),
            memory_total: 64 << 30,
            memory_available: Some(10 << 30),
            gpus: vec![],
        };
        let f = h.fingerprint();
        // Free memory and a few megabytes of total memory do not change it.
        let same = Hardware {
            memory_available: Some(50 << 30),
            memory_total: (64 << 30) - (40 << 20),
            ..h.clone()
        };
        assert_eq!(same.fingerprint(), f);
        // A new firmware split, another processor, or a GPU does.
        for other in [
            Hardware {
                memory_total: 96 << 30,
                ..h.clone()
            },
            Hardware {
                cpu: "Another".into(),
                ..h.clone()
            },
            Hardware {
                gpus: vec![GpuMemory {
                    vendor: 0x10de,
                    device: 1,
                    dedicated: Some(8 << 30),
                    source: "sysfs".into(),
                    ..Default::default()
                }],
                ..h.clone()
            },
        ] {
            assert_ne!(other.fingerprint(), f);
        }
    }

    #[test]
    fn this_machine_reads() {
        let h = Hardware::read();
        assert!(h.threads >= 1);
        assert!(!h.cpu.is_empty());
        if cfg!(any(target_os = "linux", windows, target_os = "macos")) {
            assert!(h.memory_total > 0, "{h:?}");
        }
    }
}
