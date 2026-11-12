//! How a program that has used the GPU ends: the app when its window closes, the background host
//! when it quits.
//!
//! Destroying the last wgpu instance unloads the graphics driver's library (the Vulkan loader and
//! OpenGL both free it), but the driver may still have threads of its own running, which then run
//! code that is no longer there. On the Acer (NVIDIA, Windows 11) the app ended once in three with
//! 0xC0000005 in `nvoglv64.dll_unloaded`, exception data 8: an attempt to execute code in the
//! unloaded driver. The headless host on Windows CI (WARP, no GPU) ended with 0xC0000005 the same
//! way. The program can join its own threads, but not the driver's.
//!
//! So the programs end in two steps. First everything of their own is put in order while the
//! device still lives: settings saved, jobs stopped at a checkpoint, worker threads stopped, the
//! link's endpoint file removed, and the GPU waited on until it is idle ([`wait_idle`]). Then
//! [`end_process`] ends the process at once, without destroying the device or the instance, so
//! the driver is never unloaded while it runs: the system reclaims the memory, handles and GPU
//! work of a process that has ended. Browsers and game engines end the same way.

use std::time::Duration;

/// Longest [`wait_idle`] waits for the GPU.
const IDLE_WAIT: Duration = Duration::from_secs(2);

/// Waits until the work submitted to `device` has finished (at most a couple of seconds, so a hung
/// GPU can't keep the program from ending).
pub fn wait_idle(device: &wgpu::Device) {
    let _ = device.poll(wgpu::PollType::Wait {
        submission_index: None,
        timeout: Some(IDLE_WAIT),
    });
}

/// Ends the process now with `code`, once standard output and error are flushed. Nothing else
/// runs: no destructors, and none of the code libraries leave to run at exit (on Windows no
/// `DLL_PROCESS_DETACH`, through `TerminateProcess`; elsewhere no `atexit` handlers, through
/// `_exit`). Call it only when everything that must be saved is saved.
pub fn end_process(code: i32) -> ! {
    use std::io::Write;
    let _ = std::io::stdout().flush();
    let _ = std::io::stderr().flush();
    terminate(code)
}

#[cfg(windows)]
fn terminate(code: i32) -> ! {
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, TerminateProcess};
    // Returns only if it failed, which it does not for the calling process itself.
    unsafe { TerminateProcess(GetCurrentProcess(), code as u32) };
    std::process::exit(code)
}

#[cfg(unix)]
fn terminate(code: i32) -> ! {
    unsafe extern "C" {
        /// POSIX `_exit`, in every Unix C library.
        fn _exit(status: std::ffi::c_int) -> !;
    }
    unsafe { _exit(code) }
}

#[cfg(not(any(windows, unix)))]
fn terminate(code: i32) -> ! {
    std::process::exit(code)
}
