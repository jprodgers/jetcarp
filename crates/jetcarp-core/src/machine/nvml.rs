//! NVIDIA's management library, NVML, on Linux: how busy an NVIDIA GPU is
//! ([`super::gpu_load`]) and how much memory it has ([`super::hardware`]). It comes with NVIDIA's
//! driver (some distributions package it on its own) and is opened while running (`dlopen`), so
//! JETCARP has no build dependency on it and runs where it is missing.
//!
//! Its calls are reached through [`Api`], so the tests stand in for them on every system.

use super::hardware::{PciAddress, parse_pci_address};

/// `nvmlUtilization_t`: the share of the last sample period (a sixth of a second to a second)
/// in which a kernel ran, and the memory's.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct Utilization {
    pub gpu: u32,
    pub memory: u32,
}

/// `nvmlMemory_t`: the GPU's memory, in bytes.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct Memory {
    pub total: u64,
    pub free: u64,
    pub used: u64,
}

/// `nvmlPciInfo_t`, as `nvmlDeviceGetPciInfo_v3` writes it: where the GPU is on the PCI bus.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct PciInfo {
    /// `busIdLegacy`: `0000:01:00.0`, ending in NUL.
    pub bus_id_legacy: [u8; 16],
    pub domain: u32,
    pub bus: u32,
    pub device: u32,
    /// `pciDeviceId`: the device's id in the high 16 bits, its vendor's in the low.
    pub pci_device_id: u32,
    pub pci_subsystem_id: u32,
    /// `busId`: `00000000:01:00.0`, ending in NUL.
    pub bus_id: [u8; 32],
}

impl PciInfo {
    /// Its PCI address: from `busId`, else `busIdLegacy`, else its domain, bus and device.
    pub fn address(&self) -> PciAddress {
        let text = |b: &[u8]| {
            let n = b.iter().position(|&c| c == 0).unwrap_or(b.len());
            std::str::from_utf8(&b[..n])
                .ok()
                .and_then(parse_pci_address)
        };
        text(&self.bus_id)
            .or_else(|| text(&self.bus_id_legacy))
            .unwrap_or((self.domain, self.bus, self.device, 0))
    }
}

/// A GPU's handle, NVML's own (`nvmlDevice_t`).
pub type Device = *mut std::ffi::c_void;

/// NVML's calls used here, after `nvmlInit_v2`. Each returns `NVML_SUCCESS` (0) or an error.
pub struct Api {
    /// `nvmlDeviceGetCount_v2`.
    pub count: unsafe extern "C" fn(*mut u32) -> i32,
    /// `nvmlDeviceGetHandleByIndex_v2`.
    pub handle: unsafe extern "C" fn(u32, *mut Device) -> i32,
    /// `nvmlDeviceGetUtilizationRates`.
    pub utilization: unsafe extern "C" fn(Device, *mut Utilization) -> i32,
    /// `nvmlDeviceGetPciInfo_v3`.
    pub pci: unsafe extern "C" fn(Device, *mut PciInfo) -> i32,
    /// `nvmlDeviceGetMemoryInfo`.
    pub memory: unsafe extern "C" fn(Device, *mut Memory) -> i32,
}

/// Each GPU's handle, of those NVML gives one.
fn devices(api: &Api) -> Vec<Device> {
    let mut n = 0u32;
    // SAFETY: the call writes the count given.
    if unsafe { (api.count)(&mut n) } != 0 {
        return vec![];
    }
    (0..n)
        .filter_map(|i| {
            let mut h = std::ptr::null_mut();
            // SAFETY: the call writes only the handle it is given.
            (unsafe { (api.handle)(i, &mut h) } == 0).then_some(h)
        })
        .collect()
}

/// The busiest NVIDIA GPU's utilization (0 to 1), over the GPUs that say. None when none does.
pub fn busy(api: &Api) -> Option<f64> {
    devices(api)
        .into_iter()
        .filter_map(|h| {
            let mut u = Utilization::default();
            // SAFETY: the call writes only what it is given; the handle is NVML's own.
            (unsafe { (api.utilization)(h, &mut u) } == 0).then_some(u.gpu)
        })
        .max()
        .map(|p| (p as f64 / 100.0).clamp(0.0, 1.0))
}

/// Each NVIDIA GPU's PCI address and memory (`nvmlMemory_t`'s total, in bytes), in NVML's order,
/// of the GPUs that give both.
pub fn memory(api: &Api) -> Vec<(PciAddress, u64)> {
    devices(api)
        .into_iter()
        .filter_map(|h| {
            let mut p = PciInfo::default();
            let mut m = Memory::default();
            // SAFETY: each call writes only what it is given; the handle is NVML's own.
            let ok = unsafe { (api.pci)(h, &mut p) == 0 && (api.memory)(h, &mut m) == 0 };
            ok.then(|| (p.address(), m.total))
        })
        .collect()
}

#[cfg(target_os = "linux")]
pub use lib::Session;

#[cfg(target_os = "linux")]
mod lib {
    use super::{Api, Device, Memory, PciInfo, Utilization};
    use crate::machine::hardware::PciAddress;
    use std::sync::OnceLock;

    struct Lib {
        init: unsafe extern "C" fn() -> i32,
        shutdown: unsafe extern "C" fn() -> i32,
        api: Api,
    }

    /// The library, opened once (and kept open: NVML may keep threads of its own).
    fn lib() -> Option<&'static Lib> {
        static LIB: OnceLock<Option<Lib>> = OnceLock::new();
        LIB.get_or_init(|| {
            ["libnvidia-ml.so.1", "libnvidia-ml.so"]
                .iter()
                .find_map(|name| open(name))
        })
        .as_ref()
    }

    fn open(name: &str) -> Option<Lib> {
        let c = std::ffi::CString::new(name).ok()?;
        // SAFETY: a NUL-terminated name; a null handle means it isn't there.
        let h = unsafe { libc::dlopen(c.as_ptr(), libc::RTLD_NOW | libc::RTLD_LOCAL) };
        if h.is_null() {
            return None;
        }
        let sym = |s: &std::ffi::CStr| {
            // SAFETY: the handle is open; a null address means no such function.
            let p = unsafe { libc::dlsym(h, s.as_ptr()) };
            (!p.is_null()).then_some(p)
        };
        // SAFETY: each address is NVML's function of that name, of the type its header gives.
        unsafe {
            Some(Lib {
                init: std::mem::transmute::<*mut libc::c_void, unsafe extern "C" fn() -> i32>(sym(
                    c"nvmlInit_v2",
                )?),
                shutdown: std::mem::transmute::<*mut libc::c_void, unsafe extern "C" fn() -> i32>(
                    sym(c"nvmlShutdown")?,
                ),
                api: Api {
                    count: std::mem::transmute::<
                        *mut libc::c_void,
                        unsafe extern "C" fn(*mut u32) -> i32,
                    >(sym(c"nvmlDeviceGetCount_v2")?),
                    handle: std::mem::transmute::<
                        *mut libc::c_void,
                        unsafe extern "C" fn(u32, *mut Device) -> i32,
                    >(sym(c"nvmlDeviceGetHandleByIndex_v2")?),
                    utilization: std::mem::transmute::<
                        *mut libc::c_void,
                        unsafe extern "C" fn(Device, *mut Utilization) -> i32,
                    >(sym(c"nvmlDeviceGetUtilizationRates")?),
                    pci: std::mem::transmute::<
                        *mut libc::c_void,
                        unsafe extern "C" fn(Device, *mut PciInfo) -> i32,
                    >(sym(c"nvmlDeviceGetPciInfo_v3")?),
                    memory: std::mem::transmute::<
                        *mut libc::c_void,
                        unsafe extern "C" fn(Device, *mut Memory) -> i32,
                    >(sym(c"nvmlDeviceGetMemoryInfo")?),
                },
            })
        }
    }

    /// NVML started, until dropped.
    pub struct Session(&'static Lib);

    impl Session {
        /// None when the library can't be opened, or won't start (no NVIDIA driver loaded).
        pub fn open() -> Option<Session> {
            let lib = lib()?;
            // SAFETY: NVML's own start; paired with the shutdown on drop.
            (unsafe { (lib.init)() } == 0).then_some(Session(lib))
        }

        /// [`super::busy`].
        pub fn busy(&self) -> Option<f64> {
            super::busy(&self.0.api)
        }

        /// [`super::memory`].
        pub fn memory(&self) -> Vec<(PciAddress, u64)> {
            super::memory(&self.0.api)
        }
    }

    impl Drop for Session {
        fn drop(&mut self) {
            // SAFETY: started in `open`; NVML counts its starts.
            unsafe { (self.0.shutdown)() };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // NVML stand-ins: three GPUs, the second of which can't say (NVML_ERROR_NOT_SUPPORTED).
    unsafe extern "C" fn count3(n: *mut u32) -> i32 {
        unsafe { *n = 3 };
        0
    }
    unsafe extern "C" fn handle_of(i: u32, h: *mut Device) -> i32 {
        unsafe { *h = (i as usize + 1) as Device };
        0
    }
    unsafe extern "C" fn util_of(h: Device, u: *mut Utilization) -> i32 {
        let pct = match h as usize {
            1 => 12,
            2 => return 3,
            _ => 87,
        };
        unsafe {
            *u = Utilization {
                gpu: pct,
                memory: 5,
            }
        };
        0
    }
    unsafe extern "C" fn count_fails(_: *mut u32) -> i32 {
        999
    }
    unsafe extern "C" fn none_say(_: Device, _: *mut Utilization) -> i32 {
        3
    }
    unsafe extern "C" fn no_pci(_: Device, _: *mut PciInfo) -> i32 {
        3
    }
    unsafe extern "C" fn no_memory(_: Device, _: *mut Memory) -> i32 {
        3
    }

    fn api() -> Api {
        Api {
            count: count3,
            handle: handle_of,
            utilization: util_of,
            pci: no_pci,
            memory: no_memory,
        }
    }

    /// NVML's text for a PCI address: `00000000:0A:00.0`, ending in NUL.
    fn bus_id(text: &str) -> [u8; 32] {
        let mut b = [0u8; 32];
        b[..text.len()].copy_from_slice(text.as_bytes());
        b
    }
    // Three GPUs: the first at 0000:0a:00.0 with 24 GB, the second can't say its memory
    // (NVML_ERROR_NOT_SUPPORTED), the third gives no address in text, only in numbers (bus 1).
    unsafe extern "C" fn pci_of(h: Device, p: *mut PciInfo) -> i32 {
        let info = match h as usize {
            1 => PciInfo {
                bus_id: bus_id("00000000:0A:00.0"),
                domain: 0,
                bus: 10,
                pci_device_id: 0x2684_10de,
                ..Default::default()
            },
            2 => PciInfo {
                bus_id: bus_id("00000000:0B:00.0"),
                bus: 11,
                ..Default::default()
            },
            _ => PciInfo {
                bus: 1,
                ..Default::default()
            },
        };
        unsafe { *p = info };
        0
    }
    unsafe extern "C" fn memory_of(h: Device, m: *mut Memory) -> i32 {
        let total: u64 = match h as usize {
            1 => 24 << 30,
            2 => return 3,
            _ => 8 << 30,
        };
        unsafe {
            *m = Memory {
                total,
                free: total / 2,
                used: total / 2,
            }
        };
        0
    }

    #[test]
    fn nvml_gives_each_gpu_s_memory_with_its_pci_address() {
        let a = Api {
            pci: pci_of,
            memory: memory_of,
            ..api()
        };
        assert_eq!(
            memory(&a),
            vec![((0, 0x0a, 0, 0), 24 << 30), ((0, 1, 0, 0), 8 << 30)]
        );
        // NVML that lists no GPU, or whose GPUs give no address, gives nothing.
        assert_eq!(
            memory(&Api {
                count: count_fails,
                ..a
            }),
            vec![]
        );
        assert_eq!(
            memory(&Api {
                pci: no_pci,
                memory: memory_of,
                ..api()
            }),
            vec![]
        );
    }

    #[test]
    fn nvml_gives_the_busiest_gpu() {
        assert_eq!(busy(&api()), Some(0.87));
        let a = Api {
            count: count_fails,
            ..api()
        };
        assert_eq!(busy(&a), None);
        let a = Api {
            utilization: none_say,
            ..api()
        };
        assert_eq!(busy(&a), None);
    }
}
