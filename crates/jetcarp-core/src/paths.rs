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

use std::path::{Path, PathBuf};

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

/// One line per folder, for `jetcarp paths` and bug reports.
pub fn describe() -> String {
    let show = |p: Option<PathBuf>| {
        p.map(|p| p.display().to_string())
            .unwrap_or_else(|| "(none on this system)".into())
    };
    format!(
        "settings: {}\ndata:     {}\noutput:   {}\n",
        show(config_dir()),
        data_dir().display(),
        output_dir().display()
    )
}
