//! Helpers shared by the display tests.

#![allow(dead_code)]

use jetcarp_gpu::select::{self, Purpose};
use std::sync::{Mutex, OnceLock};

/// The display adapter as JETCARP chooses it (no surface), and a device of the test's own on it,
/// made with `desc`. None after [`jetcarp_gpu::no_gpu`], which fails the test instead under
/// `JETCARP_REQUIRE_GPU=1`; `what` names the tests in its note.
///
/// A test program opens the GPU as the program does (#323, #370):
/// - **One instance, through [`select::new_instance`].** The process keeps it to its end, so the
///   drivers stay loaded, and it has the backends JETCARP opens: no OpenGL on Windows. Each test
///   made an instance of its own with every backend before, and dropped it once its device was
///   open, so on Windows every test started OpenGL (a hidden window, a thread and a context) and
///   the tests' threads made and destroyed instances side by side.
/// - **One list of adapters and one choice**, made by the first test to ask. On DirectX 12,
///   listing the adapters makes a device on each, and every test listed them as it started.
/// - **Devices opened one at a time.** `strobe_gpu` crashed once on Windows CI's WARP as its tests
///   opened theirs together (0xC0000005, #370).
///
/// Each test still has a device of its own, as the app has one for its window beside the
/// simulator's: a device's readbacks come back in callbacks that whichever thread polls the device
/// runs, so with one device between tests, one test's wait could end before another test's poll
/// ran the callback with its reading, and the strobe guard's tests would see that reading a frame
/// late.
pub fn device(
    what: &str,
    desc: impl FnOnce(&wgpu::Adapter) -> wgpu::DeviceDescriptor<'static>,
) -> Option<(wgpu::Adapter, wgpu::Device, wgpu::Queue)> {
    static ADAPTER: OnceLock<Result<wgpu::Adapter, String>> = OnceLock::new();
    static OPENING: Mutex<()> = Mutex::new(());
    let adapter = ADAPTER.get_or_init(|| {
        let instance = select::new_instance();
        let choice = select::pick(&instance, Purpose::Display, None)?;
        choice.usable.clone()?;
        Ok(choice.adapter)
    });
    let adapter = match adapter {
        Ok(a) => a,
        Err(e) => {
            jetcarp_gpu::no_gpu(what, e);
            return None;
        }
    };
    let _one_at_a_time = OPENING.lock().unwrap_or_else(|e| e.into_inner());
    match pollster::block_on(adapter.request_device(&desc(adapter))) {
        Ok((device, queue)) => Some((adapter.clone(), device, queue)),
        Err(e) => {
            let name = adapter.get_info().name;
            jetcarp_gpu::no_gpu(what, &format!("{name} refused a device: {e}"));
            None
        }
    }
}
