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
//! So the programs end in two steps. First everything of their own that isn't on the GPU is put
//! in order: settings saved, jobs stopped at a checkpoint, the link closed and its endpoint file
//! removed. Then [`end_process`] ends the process at once. Nothing on the GPU is touched: no
//! buffer is dropped, no device waited on, and the device and the instance are never destroyed,
//! so the driver is never unloaded while it runs. (Dropping the host's GPU buffers and waiting on
//! its device before ending still crashed WARP on Windows CI.) The system reclaims the memory,
//! handles and GPU work of a process that has ended; browsers and game engines end the same way.

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
