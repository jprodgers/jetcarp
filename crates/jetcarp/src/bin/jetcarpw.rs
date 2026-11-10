//! `jetcarpw.exe`: JETCARP's windowless launcher for Windows.
//!
//! `jetcarp.exe` is a console program, so starting it from a shortcut, at login, or as a screen
//! saver would open a console window. This launcher is a GUI-subsystem program that runs
//! `jetcarp.exe` with the same arguments and no console, waits for it, and returns its exit code.
//!
//! Copied as `JETCARP.scr`, it is the screen saver: Windows starts it with `/s`, `/p <hwnd>` or
//! `/c`, and it runs `jetcarp.exe host scr <those arguments>`.
//!
//! It finds `jetcarp.exe` beside itself, else in the folder of the recorded host command
//! (`<settings>/host-program`, written by the host and by `jetcarp host saver install`). Only the
//! standard library is used, so it stays small.

#![cfg_attr(windows, windows_subsystem = "windows")]

use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

fn settings_dir() -> Option<PathBuf> {
    if let Some(d) = std::env::var_os("JETCARP_CONFIG_DIR").filter(|d| !d.is_empty()) {
        return Some(PathBuf::from(d));
    }
    // jetcarp_core::paths::config_dir on Windows: %APPDATA%\jetcarp.
    std::env::var_os("APPDATA").map(|a| Path::new(&a).join("jetcarp"))
}

fn find_jetcarp(me: &Path) -> Option<PathBuf> {
    let beside = me.with_file_name("jetcarp.exe");
    if beside.exists() {
        return Some(beside);
    }
    let text = std::fs::read_to_string(settings_dir()?.join("host-program")).ok()?;
    let recorded = PathBuf::from(text.lines().next()?.trim());
    let main = recorded.with_file_name("jetcarp.exe");
    main.exists().then_some(main)
}

#[cfg(windows)]
fn no_console(c: &mut Command) {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    c.creation_flags(CREATE_NO_WINDOW);
}

#[cfg(not(windows))]
fn no_console(_: &mut Command) {}

fn main() -> ExitCode {
    let Ok(me) = std::env::current_exe() else {
        return ExitCode::FAILURE;
    };
    let Some(jetcarp) = find_jetcarp(&me) else {
        // No console to print to: nothing useful can be shown without extra dependencies.
        return ExitCode::from(2);
    };
    let is_scr = me
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("scr"));
    let mut c = Command::new(jetcarp);
    if is_scr {
        c.args(["host", "scr"]);
    }
    c.args(std::env::args_os().skip(1));
    no_console(&mut c);
    match c.status() {
        Ok(s) => ExitCode::from(s.code().unwrap_or(1).clamp(0, 255) as u8),
        Err(_) => ExitCode::FAILURE,
    }
}
