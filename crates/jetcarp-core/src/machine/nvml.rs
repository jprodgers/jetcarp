//! NVIDIA's management library, NVML, on Linux: how busy an NVIDIA GPU is
//! ([`super::gpu_load`]). It comes with NVIDIA's driver (some distributions package it on its
//! own) and is opened while running (`dlopen`), so JETCARP has no build dependency on it and runs
//! where it is missing.
//!
//! Its calls are reached through [`Api`], so the tests stand in for them on every system.

/// `nvmlUtilization_t`: the share of the last sample period (a sixth of a second to a second)
/// in which a kernel ran, and the memory's.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct Utilization {
    pub gpu: u32,
    pub memory: u32,
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

#[cfg(target_os = "linux")]
pub use lib::Session;

#[cfg(target_os = "linux")]
mod lib {
    use super::{Api, Device, Utilization};
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

    fn api() -> Api {
        Api {
            count: count3,
            handle: handle_of,
            utilization: util_of,
        }
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
