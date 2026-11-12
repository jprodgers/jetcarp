//! Where JETCARP keeps its files. Every location has an environment override, so tests,
//! scripts and portable installs can move it.
//!
//! | Folder | Holds | Override | Default |
//! | --- | --- | --- | --- |
//! | settings | app and host settings, screensaver profiles, rotations | `JETCARP_CONFIG_DIR` | `<config>/jetcarp` |
//! | data | the pattern catalog, search databases, the Quick Start atlas, packs | `JETCARP_DATA_DIR` | `<local data>/jetcarp` |
//! | output | pictures, sweeps, media, sandpiles, wallpapers | `JETCARP_OUT` | `<Pictures>/JETCARP` |
//!
//! `<config>` and `<local data>` are the operating system's folders: `~/.config` and
//! `~/.local/share` on Linux, `~/Library/Application Support` on macOS, `%APPDATA%` and
//! `%LOCALAPPDATA%` on Windows. Data uses the local folder, not the roaming one, because search
//! databases can be large. On macOS both are the same folder, so data goes in a `data` subfolder
//! there.
//!
//! One more folder belongs to the program, not the user: the packs that came with it (the
//! starter pack), which JETCARP reads but never writes; see [`bundled_packs_dir`].

use std::ffi::OsStr;
use std::path::{Path, PathBuf};

/// The variable that names the folder of packs that came with the program.
pub const BUNDLED_PACKS_ENV: &str = "JETCARP_BUNDLED_PACKS";

/// The folder name inside the operating system's settings and data folders.
pub const APP_DIR: &str = "jetcarp";

/// The folder name inside the user's Pictures folder.
pub const OUTPUT_DIR: &str = "JETCARP";

fn env_dir(var: &str) -> Option<PathBuf> {
    std::env::var_os(var)
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
}

/// The settings folder: `$JETCARP_CONFIG_DIR`, else `<config>/jetcarp`. `None` when the system
/// has no settings folder (settings are then not saved).
pub fn config_dir() -> Option<PathBuf> {
    env_dir("JETCARP_CONFIG_DIR").or_else(|| Some(dirs::config_dir()?.join(APP_DIR)))
}

/// The data folder: `$JETCARP_DATA_DIR`, else `<local data>/jetcarp` (with a `data` subfolder
/// when that is also the settings folder, as on macOS), else `./jetcarp-data`.
pub fn data_dir() -> PathBuf {
    if let Some(d) = env_dir("JETCARP_DATA_DIR") {
        return d;
    }
    let Some(base) = dirs::data_local_dir() else {
        return PathBuf::from("jetcarp-data");
    };
    let d = base.join(APP_DIR);
    if dirs::config_dir().is_some_and(|c| c.join(APP_DIR) == d) {
        d.join("data")
    } else {
        d
    }
}

/// The output folder: `$JETCARP_OUT`, else `<Pictures>/JETCARP`, else `~/Pictures/JETCARP`,
/// else `./jetcarp-out`.
pub fn output_dir() -> PathBuf {
    if let Some(d) = env_dir("JETCARP_OUT") {
        return d;
    }
    dirs::picture_dir()
        .or_else(|| dirs::home_dir().map(|h| h.join("Pictures")))
        .map(|p| p.join(OUTPUT_DIR))
        .unwrap_or_else(|| PathBuf::from("jetcarp-out"))
}

/// The folder of packs that came with the program (each a pack store, as `pack-format.md` lays
/// one out), for the first-run setup to offer: `$JETCARP_BUNDLED_PACKS` when set, else the first
/// of `<program folder>/../share/jetcarp/packs` and `<program folder>/packs` that is a folder.
/// `None` when there is none, as after `cargo install` or with a release built without the
/// starter pack.
///
/// The second place is where prefix installs put it (Homebrew's `pkgshare`, a Python
/// environment's `share` folder from the wheel); the third is the release archives' own layout.
/// `<program folder>` is the running program's folder with symbolic links resolved, so a link on
/// the `PATH` (pipx, uv, Homebrew, winget, the shell installer) leads back to the folder the
/// program really lives in.
pub fn bundled_packs_dir() -> Option<PathBuf> {
    let env = std::env::var_os(BUNDLED_PACKS_ENV);
    bundled_packs_from(env.as_deref(), program_path().as_deref())
}

/// The places [`bundled_packs_dir`] looks, in order: `$JETCARP_BUNDLED_PACKS` alone when set,
/// else the two folders beside the program. For `jetcarp doctor` when none is found.
pub fn bundled_packs_candidates() -> Vec<PathBuf> {
    let env = std::env::var_os(BUNDLED_PACKS_ENV);
    bundled_packs_places(env.as_deref(), program_path().as_deref())
}

/// [`bundled_packs_dir`] with the variable's value and the program's path given (for tests).
pub fn bundled_packs_from(env: Option<&OsStr>, exe: Option<&Path>) -> Option<PathBuf> {
    let places = bundled_packs_places(env, exe);
    if env.is_some_and(|v| !v.is_empty()) {
        // An explicit choice stands even when the folder is missing, so the doctor can say so.
        return places.into_iter().next();
    }
    places.into_iter().find(|p| p.is_dir())
}

fn bundled_packs_places(env: Option<&OsStr>, exe: Option<&Path>) -> Vec<PathBuf> {
    if let Some(v) = env.filter(|v| !v.is_empty()) {
        return vec![PathBuf::from(v)];
    }
    let Some(dir) = exe.and_then(Path::parent) else {
        return Vec::new();
    };
    let mut places = Vec::new();
    if let Some(prefix) = dir.parent() {
        places.push(prefix.join("share").join(APP_DIR).join("packs"));
    }
    places.push(dir.join("packs"));
    places
}

/// The running program's file with symbolic links resolved (winget's portable installs link the
/// program into a folder on the `PATH` on Windows too).
fn program_path() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let real = std::fs::canonicalize(&exe).unwrap_or(exe);
    Some(if cfg!(windows) {
        plain_path(real)
    } else {
        real
    })
}

/// `\\?\C:\x` as `C:\x`: Windows' canonical paths are verbatim ones, which read badly in
/// `jetcarp paths`. Other forms (UNC shares, paths that are not Unicode) are kept as they are.
pub fn plain_path(p: PathBuf) -> PathBuf {
    if let Some(rest) = p.to_str().and_then(|s| s.strip_prefix(r"\\?\")) {
        let b = rest.as_bytes();
        if b.len() >= 3 && b[0].is_ascii_alphabetic() && b[1] == b':' && b[2] == b'\\' {
            return PathBuf::from(rest);
        }
    }
    p
}

/// The search databases' folder: `<data>/explore`.
pub fn explore_dir() -> PathBuf {
    data_dir().join("explore")
}

/// A path made absolute against the working directory (no symlinks resolved).
pub fn absolute(p: &Path) -> PathBuf {
    if p.is_absolute() {
        p.to_path_buf()
    } else {
        std::env::current_dir()
            .map(|d| d.join(p))
            .unwrap_or_else(|_| p.to_path_buf())
    }
}

/// This machine's name, for job owners (`host:pid` while a job runs; no extra dependency:
/// `HOSTNAME` or `COMPUTERNAME`, then `/etc/hostname`, else `unknown`). Run records leave it out,
/// since their folders are made to be shared.
pub fn host_name() -> String {
    for k in ["HOSTNAME", "COMPUTERNAME"] {
        if let Ok(h) = std::env::var(k)
            && !h.trim().is_empty()
        {
            return h.trim().to_string();
        }
    }
    std::fs::read_to_string("/etc/hostname")
        .map(|s| s.trim().to_string())
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "unknown".into())
}

/// One line per folder, for `jetcarp paths` and bug reports.
pub fn describe() -> String {
    let show = |p: Option<PathBuf>| {
        p.map(|p| p.display().to_string())
            .unwrap_or_else(|| "(none on this system)".into())
    };
    format!(
        "settings: {}\ndata:     {}\noutput:   {}\nbundled:  {}\n",
        show(config_dir()),
        data_dir().display(),
        output_dir().display(),
        bundled_packs_dir()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| "(no packs came with the program)".into())
    )
}
