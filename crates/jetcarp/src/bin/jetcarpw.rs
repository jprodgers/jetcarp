//! `jetcarpw.exe`: JETCARP's windowless launcher for Windows.
//!
//! `jetcarp.exe` is a console program, so starting it from a shortcut, at login, or as a screen
//! saver would open a console window. This launcher is a GUI-subsystem program that runs
//! `jetcarp.exe` with the same arguments and no console, waits for it, and returns its exit code;
//! at login (`--login`) it starts it and exits at once, so Windows needn't wait on it (#78).
//!
//! Copied as `JETCARP.scr`, it is the screen saver: Windows starts it with `/s`, `/p <hwnd>` or
//! `/c`, and it runs `jetcarp.exe host scr <those arguments>`.
//!
//! Copied into a folder of JETCARP's own by `jetcarp launcher install`, it is what the Start menu
//! entry starts, so the entry outlives an upgrade that moves the program (#105).
//!
//! It finds `jetcarp.exe` beside itself; else as the record beside it names it (the Start menu's
//! copy); else in the folder of the recorded host command (`<settings>/host-program`, written by
//! the host and by `jetcarp host saver install`); else on the `PATH` (`launcher_find.rs`). Only the
//! standard library is used, so it stays small.

#![cfg_attr(windows, windows_subsystem = "windows")]

#[path = "../../launcher_find.rs"]
mod launcher_find;

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
    launcher_find::find(
        me,
        settings_dir().as_deref(),
        std::env::var_os("PATH").as_deref(),
    )
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
    // At login (the Run value is `jetcarpw.exe host serve --login`), start the host and go:
    // Windows runs the Run entries one at a time and waits on each, so a launcher waiting on the
    // host kept Windows waiting too (#78). A screen saver and a shortcut still wait, for the exit
    // code.
    if std::env::args_os().any(|a| a == "--login") {
        return match c.spawn() {
            Ok(_) => ExitCode::SUCCESS,
            Err(_) => ExitCode::FAILURE,
        };
    }
    match c.status() {
        // The program's own code, whole: Windows codes are 32-bit, and a crash (0xC0000005)
        // is negative as an i32, so it must not be clamped into 0..=255 (it would read as 0).
        Ok(s) => std::process::exit(s.code().unwrap_or(1)),
        Err(_) => ExitCode::FAILURE,
    }
}
