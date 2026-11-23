//! What the machine holds, read from the system in milliseconds, with no benchmark: memory, the
//! processor and its cores, and each GPU's memory where the system says it.
//!
//! - **Linux**: `/proc/meminfo` (MemTotal, MemAvailable), `/proc/cpuinfo`, and each GPU's
//!   memory ([`read_drm`]): for AMD's driver `/sys/class/drm/card*/device/mem_info_vram_total`
//!   and `mem_info_gtt_total`; for NVIDIA's, its management library ([`super::nvml`]); for the
//!   others, the kernel's `/sys/fs/cgroup/dmem.capacity` (Intel's xe driver, Linux 6.14 and
//!   later).
//! - **Windows**: `GlobalMemoryStatusEx`, the registry's `ProcessorNameString`, and DXGI's
//!   dedicated and shared memory for each adapter.
//! - **macOS**: `sysctl` (`hw.memsize`, `machdep.cpu.brand_string`, `hw.physicalcpu`) and the
//!   virtual memory statistics; the GPU shares the one pool of memory.
//!
//! A GPU whose memory cannot be read is listed with the reason (on Linux), or left out; its
//! limits then come from its buffer limits and a cautious share of system memory, said as an
//! estimate ([`super::limits`]).

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
    /// Each GPU whose memory the system reports, and on Linux each other GPU with why its memory
    /// was not read.
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
    /// Where it was read: `sysfs`, `NVML`, `dmem.capacity`, `DXGI`, `unified memory` (empty when
    /// it was not).
    pub source: String,
    /// Why its memory was not read, where it wasn't: its limit is then an estimate.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub not_read: Option<String>,
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
        // A GPU whose memory was not read is left out, as it was before such GPUs were listed.
        let mut gpus: Vec<String> = self
            .gpus
            .iter()
            .filter(|g| g.dedicated.is_some() || g.shared.is_some() || g.unified)
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

/// A PCI address, `0000:c3:00.0`: its domain, bus, device and function.
pub type PciAddress = (u32, u32, u32, u32);

/// A PCI address in text, in hexadecimal of either case: `0000:c3:00.0` (sysfs),
/// `00000000:0A:00.0` (NVML), or `01:00.0` without its domain (0).
pub fn parse_pci_address(text: &str) -> Option<PciAddress> {
    let (rest, function) = text.trim().rsplit_once('.')?;
    let hex = |t: &str| u32::from_str_radix(t, 16).ok();
    let parts: Vec<&str> = rest.split(':').collect();
    let (domain, bus, device) = match parts[..] {
        [d, b, v] => (hex(d)?, b, v),
        [b, v] => (0, b, v),
        _ => return None,
    };
    Some((domain, hex(bus)?, hex(device)?, hex(function)?))
}

/// Each GPU's own memory, in bytes by its PCI address, from the kernel's device-memory cgroup
/// (Linux 6.14 and later): `/sys/fs/cgroup/dmem.capacity`, which any program may read, has a
/// line for each region of device memory a driver registers, `drm/0000:03:00.0/vram0
/// 17179869184`. Only the `vram` regions count, added up for a GPU with more than one (xe
/// registers one for each tile, amdgpu one).
pub fn parse_dmem_capacity(text: &str) -> std::collections::BTreeMap<PciAddress, u64> {
    let mut out = std::collections::BTreeMap::new();
    for line in text.lines() {
        let mut it = line.split_whitespace();
        let (Some(name), Some(size)) = (it.next(), it.next()) else {
            continue;
        };
        let Some((pdev, region)) = name.strip_prefix("drm/").and_then(|r| r.rsplit_once('/'))
        else {
            continue;
        };
        let (Some(a), Ok(n)) = (parse_pci_address(pdev), size.parse::<u64>()) else {
            continue;
        };
        if region.starts_with("vram") {
            *out.entry(a).or_insert(0u64) += n;
        }
    }
    out
}

/// NVML is there, but lists no GPU at this one's PCI address, or not its memory.
pub const NVML_SILENT: &str = "NVIDIA's NVML doesn't give this GPU's memory";
/// An integrated Intel GPU: its memory is the system's.
pub const INTEGRATED: &str = "an integrated GPU, which shares the system's memory";

/// PCI vendor ids.
const NVIDIA: u32 = 0x10de;
const INTEL: u32 = 0x8086;

/// Where a GPU's memory is read on Linux besides the DRM folder's own files. Each is asked only
/// when a GPU needs it, and at most once.
pub struct Sources<'a> {
    /// The text of `/sys/fs/cgroup/dmem.capacity` ([`parse_dmem_capacity`]), where it was read.
    pub dmem: Option<&'a str>,
    /// NVIDIA's NVML: each of its GPUs' PCI address and memory ([`super::nvml::memory`]); none
    /// when the library could not be opened or started.
    pub nvml: &'a dyn Fn() -> Option<Vec<(PciAddress, u64)>>,
    /// The GPUs the Vulkan drivers list ([`super::vulkan::gpus`]), for a GPU nothing else gives.
    pub vulkan: &'a dyn Fn() -> Vec<super::vulkan::VulkanGpu>,
}

/// A value in a device's `uevent` (`DRIVER=amdgpu`, `PCI_SLOT_NAME=0000:c3:00.0`).
fn uevent(dev: &std::path::Path, key: &str) -> Option<String> {
    let t = std::fs::read_to_string(dev.join("uevent")).ok()?;
    t.lines()
        .find_map(|l| l.strip_prefix(key)?.strip_prefix('='))
        .map(|v| v.trim().to_string())
}

/// A card's PCI address: its `uevent`'s `PCI_SLOT_NAME`, else the name of the folder its
/// `device` link leads to.
fn pci_slot(dev: &std::path::Path) -> Option<PciAddress> {
    uevent(dev, "PCI_SLOT_NAME")
        .and_then(|s| parse_pci_address(&s))
        .or_else(|| {
            let real = std::fs::canonicalize(dev).ok()?;
            parse_pci_address(&real.file_name()?.to_string_lossy())
        })
}

/// The GPUs under a DRM folder (`/sys/class/drm`), each `cardN` once (connectors such as
/// `card0-DP-1` skipped, a device seen through two cards once), with its memory:
///
/// 1. AMD's driver's own files, `mem_info_vram_total` and `mem_info_gtt_total`, as before;
/// 2. for an NVIDIA GPU (unless on nouveau), NVML's figure for the GPU at the card's PCI address
///    (NVML numbers its GPUs its own way, so never by index);
/// 3. else the card's `vram` in the kernel's `dmem.capacity` (Intel's xe driver);
/// 4. else its Vulkan driver's largest device-local heap ([`super::vulkan`]): nouveau's NVIDIA
///    GPUs on Mesa's NVK, i915's discrete Intel GPUs, NVIDIA's without NVML. An integrated GPU's
///    is the system's memory, so it is memory the GPU may borrow ([`GpuMemory::shared`]).
///
/// A GPU none of these gives is listed with why ([`GpuMemory::not_read`]), so its limit is the
/// estimate: NVIDIA's without NVML in the words its load gives ([`super::gpu_load::NO_NVML`]);
/// an integrated Intel GPU, which shares the system's memory ([`INTEGRATED`]), and for which
/// Vulkan is not opened; one whose memory neither its driver nor Vulkan gives. A card with neither
/// memory files nor a PCI vendor (a firmware framebuffer's, simpledrm) is not a GPU, and left out.
pub fn read_drm(dir: &std::path::Path, src: &Sources<'_>) -> Vec<GpuMemory> {
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
    let dmem = src.dmem.map(parse_dmem_capacity).unwrap_or_default();
    let nvml = std::cell::OnceCell::new();
    let vulkan = std::cell::OnceCell::new();
    let mut out: Vec<GpuMemory> = vec![];
    let mut seen = vec![];
    for n in names {
        let dev = dir.join(&n).join("device");
        let read = |f: &str| {
            std::fs::read_to_string(dev.join(f))
                .ok()
                .and_then(|t| parse_sysfs_number(&t))
        };
        let vram = read("mem_info_vram_total");
        let vendor = read("vendor").unwrap_or(0) as u32;
        if vram.is_none() && vendor == 0 {
            continue;
        }
        let real = std::fs::canonicalize(&dev).unwrap_or(dev.clone());
        if seen.contains(&real) {
            continue;
        }
        seen.push(real);
        let mut g = GpuMemory {
            vendor,
            device: read("device").unwrap_or(0) as u32,
            ..Default::default()
        };
        if let Some(v) = vram {
            g.dedicated = Some(v);
            g.shared = read("mem_info_gtt_total");
            g.source = "sysfs".into();
            out.push(g);
            continue;
        }
        let slot = pci_slot(&dev);
        let driver = uevent(&dev, "DRIVER");
        let mut why = None;
        if vendor == NVIDIA && driver.as_deref() != Some("nouveau") {
            match nvml.get_or_init(|| (src.nvml)()) {
                None => why = Some(super::gpu_load::NO_NVML.to_string()),
                Some(list) => match slot.and_then(|s| list.iter().find(|(a, _)| *a == s)) {
                    Some(&(_, total)) => {
                        g.dedicated = Some(total);
                        g.source = "NVML".into();
                    }
                    None => why = Some(NVML_SILENT.to_string()),
                },
            }
        }
        if g.dedicated.is_none()
            && let Some(&v) = slot.and_then(|s| dmem.get(&s))
        {
            g.dedicated = Some(v);
            g.source = "dmem.capacity".into();
            why = None;
        }
        let integrated = vendor == INTEL && slot.is_some_and(|(_, b, d, _)| b == 0 && d == 2);
        if g.dedicated.is_none()
            && !integrated
            && let Some(v) =
                vulkan
                    .get_or_init(|| (src.vulkan)())
                    .iter()
                    .find(|v| match (v.address, slot) {
                        (Some(a), Some(s)) => a == s,
                        _ => (v.vendor, v.device) == (g.vendor, g.device),
                    })
            && let Some(heap) = v.device_local()
        {
            if v.integrated {
                g.shared = Some(heap);
            } else {
                g.dedicated = Some(heap);
            }
            g.source = "Vulkan".into();
            why = None;
        }
        if g.dedicated.is_none() && g.shared.is_none() {
            g.not_read = Some(why.unwrap_or_else(|| {
                if integrated {
                    INTEGRATED.into()
                } else {
                    match &driver {
                        Some(d) => format!("neither its driver, {d}, nor Vulkan says its memory"),
                        None => "neither its driver nor Vulkan says its memory".into(),
                    }
                }
            }));
        }
        out.push(g);
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
        let dmem = std::fs::read_to_string("/sys/fs/cgroup/dmem.capacity").ok();
        h.gpus = read_drm(
            std::path::Path::new("/sys/class/drm"),
            &Sources {
                dmem: dmem.as_deref(),
                nvml: &|| crate::machine::nvml::Session::open().map(|s| s.memory()),
                vulkan: &crate::machine::vulkan::gpus,
            },
        );
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
                not_read: None,
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
                not_read: None,
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
    use crate::machine::vulkan::VulkanGpu;

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
        let x86 = "processor\t: 0\nmodel name\t: Example  Processor   8-Core\nphysical id\t: 0\ncore id\t: 0\n\n\
                   processor\t: 1\nmodel name\t: Example  Processor   8-Core\nphysical id\t: 0\ncore id\t: 0\n\n\
                   processor\t: 2\nmodel name\t: Example  Processor   8-Core\nphysical id\t: 0\ncore id\t: 1\n";
        assert_eq!(
            parse_cpuinfo(x86),
            ("Example Processor 8-Core".to_string(), Some(2))
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

    /// A stand-in DRM folder (`/sys/class/drm`): each card's `device` folder with its files.
    fn drm(name: &str, cards: &[(&str, &[(&str, &str)])]) -> crate::scratch::ScratchDir {
        let d = crate::scratch::ScratchDir::new(name);
        for (card, files) in cards {
            let dev = d.join(card).join("device");
            std::fs::create_dir_all(&dev).unwrap();
            for (f, v) in *files {
                std::fs::write(dev.join(f), v).unwrap();
            }
        }
        d
    }

    /// Where the memory is read besides the DRM folder.
    fn src<'a>(
        dmem: Option<&'a str>,
        nvml: &'a dyn Fn() -> Option<Vec<(PciAddress, u64)>>,
    ) -> Sources<'a> {
        Sources {
            dmem,
            nvml,
            vulkan: &no_vulkan,
        }
    }

    /// No Vulkan driver here.
    fn no_vulkan() -> Vec<VulkanGpu> {
        vec![]
    }

    /// No NVML here (the library could not be opened).
    fn no_nvml() -> Option<Vec<(PciAddress, u64)>> {
        None
    }

    /// AMD's driver gives the memory as it did before NVIDIA's and Intel's were read: its own
    /// files win over the kernel's dmem.capacity, which lists it too, and NVML is not asked.
    #[test]
    fn an_amd_card_reads_as_before() {
        let d = drm(
            "machine-drm-amd",
            &[(
                "card1",
                &[
                    ("vendor", "0x1002\n"),
                    ("device", "0x73df\n"),
                    ("uevent", "DRIVER=amdgpu\nPCI_SLOT_NAME=0000:c3:00.0\n"),
                    ("mem_info_vram_total", "12884901888\n"),
                    ("mem_info_gtt_total", "17179869184\n"),
                ],
            )],
        );
        // A connector: skipped.
        std::fs::create_dir_all(d.join("card1-DP-1")).unwrap();
        let asked = std::cell::Cell::new(false);
        let g = read_drm(
            &d,
            &src(Some("drm/0000:c3:00.0/vram 999\n"), &|| {
                asked.set(true);
                None
            }),
        );
        assert_eq!(
            g,
            vec![GpuMemory {
                vendor: 0x1002,
                device: 0x73df,
                dedicated: Some(12 << 30),
                shared: Some(16 << 30),
                source: "sysfs".into(),
                ..Default::default()
            }]
        );
        assert!(!asked.get(), "NVML started for an AMD GPU");
        assert_eq!(read_drm(&d.join("missing"), &src(None, &no_nvml)), vec![]);
    }

    /// Two NVIDIA cards, which NVML lists in the other order (by its own index, not the cards'):
    /// each card gets the memory of the GPU at its own PCI address, from `uevent` (NVML writes it
    /// in capitals with a longer domain, `00000000:0A:00.0`).
    fn two_nvidia_cards(name: &str) -> crate::scratch::ScratchDir {
        drm(
            name,
            &[
                (
                    "card0",
                    &[
                        ("vendor", "0x10de\n"),
                        ("device", "0x2684\n"),
                        ("uevent", "DRIVER=nvidia\nPCI_SLOT_NAME=0000:01:00.0\n"),
                    ],
                ),
                (
                    "card1",
                    &[
                        ("vendor", "0x10de\n"),
                        ("device", "0x2206\n"),
                        ("uevent", "DRIVER=nvidia\nPCI_SLOT_NAME=0000:0a:00.0\n"),
                    ],
                ),
            ],
        )
    }

    #[test]
    fn an_nvidia_card_reads_its_memory_from_nvml_by_pci_address() {
        let d = two_nvidia_cards("machine-drm-nvidia");
        // As NVML gives them ([`crate::machine::nvml::memory`]): its GPU 0 is the second card.
        let nvml = || Some(vec![((0, 0x0a, 0, 0), 10 << 30), ((0, 1, 0, 0), 24 << 30)]);
        let g = read_drm(&d, &src(None, &nvml));
        let want = |device, gib: u64| GpuMemory {
            vendor: 0x10de,
            device,
            dedicated: Some(gib << 30),
            source: "NVML".into(),
            ..Default::default()
        };
        assert_eq!(g, vec![want(0x2684, 24), want(0x2206, 10)]);
        // NVML there, but not listing a card's GPU: that card says so.
        let g = read_drm(&d, &src(None, &|| Some(vec![((0, 1, 0, 0), 24 << 30)])));
        assert_eq!(g[0], want(0x2684, 24));
        assert_eq!(
            (g[1].dedicated, g[1].not_read.as_deref()),
            (None, Some(NVML_SILENT))
        );
    }

    /// Without NVML (some distributions package it on its own), an NVIDIA card is listed with
    /// the reason the GPU's load gives, its memory unread: its limit stays the estimate, and the
    /// machine's fingerprint stays what it was when such a card was left out.
    #[test]
    fn without_nvml_an_nvidia_card_says_why_and_keeps_the_estimate() {
        let d = two_nvidia_cards("machine-drm-no-nvml");
        let g = read_drm(&d, &src(None, &no_nvml));
        assert_eq!(g.len(), 2);
        for c in &g {
            assert_eq!(c.vendor, 0x10de);
            assert_eq!(c.dedicated, None);
            assert_eq!(c.not_read.as_deref(), Some(super::super::gpu_load::NO_NVML));
            assert!(c.source.is_empty());
        }
        let before = Hardware {
            os: "linux".into(),
            memory_total: 64 << 30,
            ..Default::default()
        };
        let now = Hardware {
            gpus: g,
            ..before.clone()
        };
        assert_eq!(now.fingerprint(), before.fingerprint());
        let l = super::super::Limits::compute(&now, &Default::default());
        let why = &l.get(super::super::Id::GpuWorld).why;
        assert!(why.starts_with("an estimate"), "{why}");
        assert!(why.contains(super::super::gpu_load::NO_NVML), "{why}");
    }

    /// Intel: a discrete GPU on xe has its memory in the kernel's dmem.capacity (Linux 6.14 and
    /// later); an integrated one (at 00:02.0) shares the system's memory, so it keeps the estimate
    /// and says so; a discrete one on i915, whose memory no file gives (nor here Vulkan), says
    /// that.
    #[test]
    fn an_intel_card_reads_its_memory_where_its_driver_gives_it() {
        let d = drm(
            "machine-drm-intel",
            &[
                (
                    "card0",
                    &[
                        ("vendor", "0x8086\n"),
                        ("device", "0xa7a0\n"),
                        ("uevent", "DRIVER=i915\nPCI_SLOT_NAME=0000:00:02.0\n"),
                    ],
                ),
                (
                    "card1",
                    &[
                        ("vendor", "0x8086\n"),
                        ("device", "0xe20b\n"),
                        ("uevent", "DRIVER=xe\nPCI_SLOT_NAME=0000:03:00.0\n"),
                    ],
                ),
                (
                    "card2",
                    &[
                        ("vendor", "0x8086\n"),
                        ("device", "0x56a0\n"),
                        ("uevent", "DRIVER=i915\nPCI_SLOT_NAME=0000:04:00.0\n"),
                    ],
                ),
            ],
        );
        let dmem = "drm/0000:03:00.0/vram0 12868124672\ndrm/0000:c3:00.0/vram 34359738368\n";
        let g = read_drm(&d, &src(Some(dmem), &no_nvml));
        assert_eq!(g.len(), 3, "{g:?}");
        assert_eq!(
            (g[0].device, g[0].dedicated, g[0].not_read.as_deref()),
            (0xa7a0, None, Some(INTEGRATED))
        );
        assert_eq!(
            g[1],
            GpuMemory {
                vendor: 0x8086,
                device: 0xe20b,
                dedicated: Some(12_868_124_672),
                source: "dmem.capacity".into(),
                ..Default::default()
            }
        );
        assert_eq!(
            (g[2].device, g[2].dedicated, g[2].not_read.as_deref()),
            (
                0x56a0,
                None,
                Some("neither its driver, i915, nor Vulkan says its memory")
            )
        );
        // Without dmem.capacity (an older kernel), the xe card says so too.
        let g = read_drm(&d, &src(None, &no_nvml));
        assert_eq!(
            g[1].not_read.as_deref(),
            Some("neither its driver, xe, nor Vulkan says its memory")
        );
    }

    /// A card with no PCI vendor and no memory files, as a firmware framebuffer's (simpledrm): not
    /// a GPU, left out.
    #[test]
    fn a_framebuffer_card_is_left_out() {
        let d = drm(
            "machine-drm-fb",
            &[("card0", &[("uevent", "DRIVER=simple-framebuffer\n")])],
        );
        assert_eq!(read_drm(&d, &src(None, &no_nvml)), vec![]);
    }

    /// Where neither the driver's files, NVML nor dmem.capacity give a GPU's memory, its Vulkan
    /// driver's largest device-local heap does, for the GPU at the card's PCI address (or, when
    /// the driver gives no address, with the card's ids): an NVIDIA GPU on nouveau (Mesa's NVK;
    /// NVML is not asked), a discrete Intel GPU on i915, an NVIDIA GPU without NVML. A GPU Vulkan
    /// calls integrated may borrow that heap, which is the system's memory. Vulkan is asked once,
    /// and never for an integrated Intel GPU, whose memory is the system's anyway.
    #[test]
    fn vulkan_gives_the_memory_where_nothing_else_does() {
        let d = drm(
            "machine-drm-vulkan",
            &[
                (
                    "card0",
                    &[
                        ("vendor", "0x8086\n"),
                        ("device", "0xa7a8\n"),
                        ("uevent", "DRIVER=i915\nPCI_SLOT_NAME=0000:00:02.0\n"),
                    ],
                ),
                (
                    "card1",
                    &[
                        ("vendor", "0x10de\n"),
                        ("device", "0x25a2\n"),
                        ("uevent", "DRIVER=nouveau\nPCI_SLOT_NAME=0000:01:00.0\n"),
                    ],
                ),
                (
                    "card2",
                    &[
                        ("vendor", "0x8086\n"),
                        ("device", "0x56a0\n"),
                        ("uevent", "DRIVER=i915\nPCI_SLOT_NAME=0000:04:00.0\n"),
                    ],
                ),
                (
                    "card3",
                    &[
                        ("vendor", "0x10de\n"),
                        ("device", "0x2684\n"),
                        ("uevent", "DRIVER=nvidia\nPCI_SLOT_NAME=0000:05:00.0\n"),
                    ],
                ),
                (
                    "card4",
                    &[
                        ("vendor", "0x1d17\n"),
                        ("device", "0x3a04\n"),
                        ("uevent", "DRIVER=zx\nPCI_SLOT_NAME=0000:06:00.0\n"),
                    ],
                ),
            ],
        );
        let gib = |n: u64| n << 30;
        let vk = |address, vendor, device, integrated, heaps: Vec<(u64, bool)>| VulkanGpu {
            address,
            vendor,
            device,
            integrated,
            heaps,
        };
        let asked = std::cell::Cell::new(0);
        let nvml_asked = std::cell::Cell::new(0);
        let vulkan = || {
            asked.set(asked.get() + 1);
            vec![
                // The integrated Intel GPU, as ANV lists it: never used.
                vk(
                    Some((0, 0, 2, 0)),
                    0x8086,
                    0xa7a8,
                    true,
                    vec![(gib(11), true)],
                ),
                // NVK: its 5.8 GB, the system's memory, and the bar the processor reaches.
                vk(
                    Some((0, 1, 0, 0)),
                    0x10de,
                    0x25a2,
                    false,
                    vec![(5_900 << 20, true), (gib(16), false), (256 << 20, true)],
                ),
                // ANV on the discrete Intel GPU, without its PCI address: found by its ids.
                vk(
                    None,
                    0x8086,
                    0x56a0,
                    false,
                    vec![(gib(16), true), (gib(16), false)],
                ),
                // NVIDIA's own Vulkan driver.
                vk(
                    Some((0, 5, 0, 0)),
                    0x10de,
                    0x2684,
                    false,
                    vec![(gib(24), true)],
                ),
                // Another vendor's integrated GPU.
                vk(
                    Some((0, 6, 0, 0)),
                    0x1d17,
                    0x3a04,
                    true,
                    vec![(gib(12), true)],
                ),
            ]
        };
        let g = read_drm(
            &d,
            &Sources {
                dmem: None,
                nvml: &|| {
                    nvml_asked.set(nvml_asked.get() + 1);
                    None
                },
                vulkan: &vulkan,
            },
        );
        let own = |device, bytes| GpuMemory {
            vendor: if device == 0x56a0 { 0x8086 } else { 0x10de },
            device,
            dedicated: Some(bytes),
            source: "Vulkan".into(),
            ..Default::default()
        };
        assert_eq!(g.len(), 5, "{g:?}");
        assert_eq!(
            (g[0].dedicated, g[0].shared, g[0].not_read.as_deref()),
            (None, None, Some(INTEGRATED))
        );
        assert_eq!(g[1], own(0x25a2, 5_900 << 20));
        assert_eq!(g[2], own(0x56a0, gib(16)));
        assert_eq!(g[3], own(0x2684, gib(24)));
        assert_eq!(
            g[4],
            GpuMemory {
                vendor: 0x1d17,
                device: 0x3a04,
                shared: Some(gib(12)),
                source: "Vulkan".into(),
                ..Default::default()
            }
        );
        assert_eq!(asked.get(), 1, "Vulkan asked more than once");
        // NVML only for the GPU on NVIDIA's driver, not the one on nouveau.
        assert_eq!(nvml_asked.get(), 1);
        // Only an integrated Intel GPU and an AMD one: Vulkan is not asked.
        let d = drm(
            "machine-drm-no-vulkan",
            &[
                (
                    "card0",
                    &[
                        ("vendor", "0x8086\n"),
                        ("device", "0xa7a8\n"),
                        ("uevent", "DRIVER=xe\nPCI_SLOT_NAME=0000:00:02.0\n"),
                    ],
                ),
                (
                    "card1",
                    &[
                        ("vendor", "0x1002\n"),
                        ("device", "0x73df\n"),
                        ("mem_info_vram_total", "12884901888\n"),
                    ],
                ),
            ],
        );
        let g = read_drm(
            &d,
            &Sources {
                dmem: None,
                nvml: &no_nvml,
                vulkan: &vulkan,
            },
        );
        assert_eq!(g.len(), 2);
        assert_eq!(asked.get(), 1, "Vulkan asked for an integrated Intel GPU");
    }

    #[test]
    fn pci_addresses_and_dmem_capacity() {
        assert_eq!(parse_pci_address("0000:c3:00.0"), Some((0, 0xc3, 0, 0)));
        assert_eq!(
            parse_pci_address("00000000:0A:00.1\n"),
            Some((0, 0x0a, 0, 1))
        );
        assert_eq!(parse_pci_address("0001:02:1f.3"), Some((1, 2, 0x1f, 3)));
        assert_eq!(parse_pci_address("02:00.0"), Some((0, 2, 0, 0)));
        assert_eq!(parse_pci_address("device"), None);
        assert_eq!(parse_pci_address("0000:zz:00.0"), None);
        // A GPU's vram regions added up (xe gives one a tile); other regions, and lines of
        // another kind, left out.
        let m = parse_dmem_capacity(
            "drm/0000:03:00.0/vram0 8573157376\ndrm/0000:03:00.0/vram1 8573157376\n\
             drm/0000:c3:00.0/vram 34359738368\ndrm/0000:05:00.0/stolen 1000\nnot a line\n",
        );
        assert_eq!(
            m.into_iter().collect::<Vec<_>>(),
            vec![
                ((0, 3, 0, 0), 2 * 8_573_157_376),
                ((0, 0xc3, 0, 0), 32 << 30)
            ]
        );
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
