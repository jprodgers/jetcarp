//! How `jetcarpw.exe`, JETCARP's windowless launcher on Windows, finds the `jetcarp.exe` it starts.
//! Shared by `src/bin/jetcarpw.rs` and its test (`tests/jetcarpw_find.rs`); the standard library
//! only, so the launcher stays small.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};

/// The program the launcher starts.
pub const EXE: &str = "jetcarp.exe";

/// The file beside a copy of the launcher that names the `jetcarp.exe` it starts
/// (`jetcarp_display::launcher::RECORD`).
pub const RECORD: &str = "jetcarp-program";

/// The program a record names: its first line, trimmed.
pub fn recorded(text: &str) -> Option<PathBuf> {
    let line = text.lines().next()?.trim();
    (!line.is_empty()).then(|| PathBuf::from(line))
}

/// `jetcarp.exe` for the launcher at `me`, the first of these that is there:
///
/// 1. beside it (a release's folder, the installer's, Scoop's and winget's own);
/// 2. as the record beside it names it ([`RECORD`]): the Start menu entry starts a copy of the
///    launcher in a folder of JETCARP's own, which no upgrade touches, and a run of `jetcarp` puts
///    its own path in the record when the program it names has gone;
/// 3. beside the program of the host command recorded in the settings folder `settings`
///    (`host-program`): the screen saver, a copy of the launcher named `JETCARP.scr`;
/// 4. in a folder on the `PATH` (`path`), where winget's links and Scoop's shims lead to the
///    current version: so after an upgrade that took the recorded program away, the entry still
///    starts JETCARP, and that run mends the record.
pub fn find(me: &Path, settings: Option<&Path>, path: Option<&OsStr>) -> Option<PathBuf> {
    let beside = me.with_file_name(EXE);
    if beside.is_file() {
        return Some(beside);
    }
    let named = std::fs::read_to_string(me.with_file_name(RECORD))
        .ok()
        .and_then(|t| recorded(&t))
        .filter(|p| p.is_file());
    if named.is_some() {
        return named;
    }
    let host = settings
        .and_then(|s| std::fs::read_to_string(s.join("host-program")).ok())
        .and_then(|t| recorded(&t))
        .map(|p| p.with_file_name(EXE))
        .filter(|p| p.is_file());
    if host.is_some() {
        return host;
    }
    std::env::split_paths(path?)
        .filter(|d| d.is_absolute())
        .map(|d| d.join(EXE))
        .find(|p| p.is_file())
}
