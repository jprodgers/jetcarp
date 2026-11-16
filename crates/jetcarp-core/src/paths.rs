//! Where JETCARP keeps its files. Every location has an environment override, so tests,
//! scripts and portable installs can move it.
//!
//! | Folder | Holds | Override | Default |
//! | --- | --- | --- | --- |
//! | settings | app and host settings, screensaver profiles, rotations | `JETCARP_CONFIG_DIR` | `<config>/jetcarp` |
//! | data | the pattern catalog (with every Quick Start entry), search databases, packs | `JETCARP_DATA_DIR` | `<local data>/jetcarp` |
//! | output | pictures, sweeps, media, sandpiles, wallpapers | `JETCARP_OUT` | `<Pictures>/JETCARP` |
//!
//! `<config>` and `<local data>` are the operating system's folders: `~/.config` and
//! `~/.local/share` on Linux, `~/Library/Application Support` on macOS, `%APPDATA%` and
//! `%LOCALAPPDATA%` on Windows. Data uses the local folder, not the roaming one, because search
//! databases can be large. On macOS both are the same folder, so data goes in a `data` subfolder
//! there.
//!
//! The data folder can also be chosen once, for a large library on another disk: the first-run
//! setup (`setup.set_data_dir`) writes it to `paths.json` in the settings folder
//! ([`chosen_data_dir`]), and every lookup of the data folder honours it. `JETCARP_DATA_DIR`
//! still wins over it.
//!
//! One more folder belongs to the program, not the user: the packs that came with it (the
//! starter pack), which JETCARP reads but never writes; see [`bundled_packs_dir`].
//!
//! Paths are compared with their symbolic links followed ([`resolve`]); a file JETCARP names
//! itself inside a folder it was given is checked to stay inside that folder ([`inside`]).

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

/// The file in the settings folder that records a data folder chosen in place of the default.
pub const PATHS_FILE: &str = "paths.json";

/// The data folder: `$JETCARP_DATA_DIR`, else the folder chosen in the settings folder's
/// `paths.json` ([`chosen_data_dir`]), else [`default_data_dir`].
pub fn data_dir() -> PathBuf {
    data_dir_for(config_dir().as_deref())
}

/// [`data_dir`] with this settings folder's `paths.json` (none: the default).
pub fn data_dir_for(settings: Option<&Path>) -> PathBuf {
    data_dir_source(settings).0
}

/// Where the data folder comes from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DataSource {
    /// `JETCARP_DATA_DIR`.
    Env,
    /// The settings folder's `paths.json`.
    Chosen,
    /// The operating system's folder.
    Default,
}

impl DataSource {
    /// `JETCARP_DATA_DIR`, `paths.json`, `default`.
    pub fn name(self) -> &'static str {
        match self {
            DataSource::Env => "JETCARP_DATA_DIR",
            DataSource::Chosen => PATHS_FILE,
            DataSource::Default => "default",
        }
    }
}

/// [`data_dir_for`], and where it came from.
pub fn data_dir_source(settings: Option<&Path>) -> (PathBuf, DataSource) {
    data_dir_source_from(std::env::var_os("JETCARP_DATA_DIR").as_deref(), settings)
}

/// [`data_dir_source`] with the variable's value given (for tests).
pub fn data_dir_source_from(env: Option<&OsStr>, settings: Option<&Path>) -> (PathBuf, DataSource) {
    if let Some(d) = env.filter(|v| !v.is_empty()) {
        return (PathBuf::from(d), DataSource::Env);
    }
    if let Some(d) = settings.and_then(chosen_data_dir) {
        return (d, DataSource::Chosen);
    }
    (default_data_dir(), DataSource::Default)
}

/// The data folder recorded in `<settings>/paths.json` (`{"data_dir": "/path"}`), if there is
/// one: an absolute path. A file that is missing, cannot be read, or names no absolute folder
/// counts as no choice.
pub fn chosen_data_dir(settings: &Path) -> Option<PathBuf> {
    let text = std::fs::read_to_string(settings.join(PATHS_FILE)).ok()?;
    let v: serde_json::Value = serde_json::from_str(&text).ok()?;
    let d = PathBuf::from(v.get("data_dir")?.as_str()?.trim());
    (d.is_absolute()).then_some(d)
}

/// Record `dir` as the data folder in `<settings>/paths.json` (written whole, then renamed into
/// place), or with `None` remove the choice (the default comes back).
pub fn set_chosen_data_dir(settings: &Path, dir: Option<&Path>) -> Result<(), String> {
    let file = settings.join(PATHS_FILE);
    let Some(dir) = dir else {
        return match std::fs::remove_file(&file) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(format!("removing {}: {e}", file.display())),
        };
    };
    if !dir.is_absolute() {
        return Err(format!("{} is not an absolute path", dir.display()));
    }
    let text = dir
        .to_str()
        .ok_or_else(|| format!("{} is not valid Unicode", dir.display()))?;
    std::fs::create_dir_all(settings)
        .map_err(|e| format!("creating {}: {e}", settings.display()))?;
    let body = serde_json::to_string_pretty(&serde_json::json!({"version": 1, "data_dir": text}))
        .map_err(|e| e.to_string())?;
    let tmp = settings.join(format!("{PATHS_FILE}.tmp-{}", std::process::id()));
    std::fs::write(&tmp, body + "\n").map_err(|e| format!("writing {}: {e}", tmp.display()))?;
    std::fs::rename(&tmp, &file).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        format!("writing {}: {e}", file.display())
    })
}

/// The operating system's data folder for JETCARP: `<local data>/jetcarp` (with a `data`
/// subfolder when that is also the settings folder, as on macOS), else `./jetcarp-data`.
pub fn default_data_dir() -> PathBuf {
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
/// of `<program folder>/../share/jetcarp/packs` (only when the program's folder is a `bin` or
/// `Scripts` folder) and `<program folder>/packs` that is a folder. `None` when there is none, as
/// after `cargo install` or with a release built without the starter pack.
///
/// The second place is where prefix installs put it (Homebrew's `pkgshare`, a Python
/// environment's `share` folder from the wheel, whose programs are in `bin`, or `Scripts` on
/// Windows); the third is the release archives' and the installers' own layout, whose folder
/// (`~/.local/lib/jetcarp`, `%LOCALAPPDATA%\Programs\jetcarp`) has no `share` beside it.
/// `<program folder>` is the running program's folder with symbolic links resolved, so a link on
/// the `PATH` (pipx, uv, Homebrew, winget, the shell installer) leads back to the folder the
/// program really lives in. The folder found is then named as it is beside the program as it was
/// started, when that is the same folder ([`stable_name`]): Homebrew's
/// `/opt/homebrew/share/jetcarp/packs` rather than `Cellar/jetcarp/0.4.0/...`.
pub fn bundled_packs_dir() -> Option<PathBuf> {
    let env = std::env::var_os(BUNDLED_PACKS_ENV);
    let found = bundled_packs_from(env.as_deref(), program_path().as_deref())?;
    if env.is_some_and(|v| !v.is_empty()) {
        return Some(found);
    }
    Some(stable_name(found, std::env::current_exe().ok().as_deref()))
}

/// `found`, a folder beside the program with links resolved, by its name beside `launched` (the
/// program's path as it was started, links not resolved) when that resolves to the same folder:
/// Homebrew's `/opt/homebrew/share/jetcarp/packs` for `Cellar/jetcarp/0.4.0/share/jetcarp/packs`,
/// Scoop's `apps\jetcarp\current\packs` for `apps\jetcarp\0.4.0\packs`. A package manager's
/// upgrade deletes the versioned folder and keeps the link, so what records the path (a pack
/// import's job) can still find it. macOS and Windows give the path as started; Linux gives it
/// resolved, so there `found` stays.
pub fn stable_name(found: PathBuf, launched: Option<&Path>) -> PathBuf {
    let Ok(real) = std::fs::canonicalize(&found) else {
        return found;
    };
    for p in bundled_packs_places(None, launched) {
        if p != found && std::fs::canonicalize(&p).is_ok_and(|c| c == real) {
            return p;
        }
    }
    found
}

/// The places [`bundled_packs_dir`] looks, in order: `$JETCARP_BUNDLED_PACKS` alone when set,
/// else the folders beside the program that apply to how it was installed. For `jetcarp doctor`
/// when none is found.
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

/// [`bundled_packs_candidates`] with the variable's value and the program's path given (for
/// tests).
pub fn bundled_packs_candidates_for(env: Option<&OsStr>, exe: Option<&Path>) -> Vec<PathBuf> {
    bundled_packs_places(env, exe)
}

fn bundled_packs_places(env: Option<&OsStr>, exe: Option<&Path>) -> Vec<PathBuf> {
    if let Some(v) = env.filter(|v| !v.is_empty()) {
        return vec![PathBuf::from(v)];
    }
    let Some(dir) = exe.and_then(Path::parent) else {
        return Vec::new();
    };
    let mut places = Vec::new();
    // `<prefix>/share` holds packs only for a program in `<prefix>/bin` (`Scripts` in a Python
    // environment on Windows).
    let in_bin = dir
        .file_name()
        .and_then(|n| n.to_str())
        .is_some_and(|n| n.eq_ignore_ascii_case("bin") || n.eq_ignore_ascii_case("scripts"));
    if in_bin && let Some(prefix) = dir.parent() {
        places.push(prefix.join("share").join(APP_DIR).join("packs"));
    }
    places.push(dir.join("packs"));
    places
}

/// The running program's file with symbolic links resolved (winget's portable installs link the
/// program into a folder on the `PATH` on Windows too). `jetcarp uninstall` tells from it how the
/// program was installed.
pub fn program_path() -> Option<PathBuf> {
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

/// A control file in the runtime folder: `$JETCARP_RUNTIME_DIR/<name>`, else
/// `%LOCALAPPDATA%\jetcarp\<name>` on Windows, `$XDG_RUNTIME_DIR/jetcarp/<name>` or
/// `~/.cache/jetcarp/<name>` elsewhere. The host's single-instance lock (`host.lock`) and the
/// app's and host's endpoint files live here.
pub fn runtime_file(name: &str) -> Option<PathBuf> {
    runtime_dir().map(|d| d.join(name))
}

/// The runtime folder [`runtime_file`] uses (`jetcarp paths` names it, #85): none when the
/// system gives no place for one.
pub fn runtime_dir() -> Option<PathBuf> {
    if let Some(d) = env_dir("JETCARP_RUNTIME_DIR") {
        return Some(d);
    }
    let base = if cfg!(windows) {
        std::env::var_os("LOCALAPPDATA").map(PathBuf::from)
    } else {
        std::env::var_os("XDG_RUNTIME_DIR")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cache")))
    }?;
    Some(base.join(APP_DIR))
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

/// Most symbolic links one path may pass through (Linux's own limit), so that a loop of links
/// ends instead of being followed for ever.
const MAX_LINKS: u32 = 40;

/// `p` as the file system will see it when a file is opened or made there: made absolute, the
/// deepest part of it that exists resolved with every symbolic link followed
/// ([`std::fs::canonicalize`]), and the rest, which does not exist yet, joined on with its `.`
/// and `..` worked out.
///
/// The folder rules (`jetcarp_api::out::Guard`) and [`inside`] check this path rather than the
/// path's text, because a link may point anywhere. A link whose target does not exist is followed
/// by hand: `canonicalize` refuses it, but a write through it would make the target. `..` follows
/// the system's rule, so on Unix `link/..` is the folder holding the link's target, not the
/// folder holding the link.
pub fn resolve(p: &Path) -> std::io::Result<PathBuf> {
    resolve_links(&std::path::absolute(p)?, 0)
}

/// [`resolve`] for an absolute path, `links` links into it.
fn resolve_links(p: &Path, links: u32) -> std::io::Result<PathBuf> {
    use std::path::Component;
    let parts: Vec<Component> = p.components().collect();
    let mut k = parts.len();
    let mut real = PathBuf::new();
    while k > 0 {
        let head: PathBuf = parts[..k].iter().collect();
        if let Ok(r) = std::fs::canonicalize(&head) {
            real = r;
            break;
        }
        if matches!(parts[k - 1], Component::Normal(_))
            && std::fs::symlink_metadata(&head).is_ok_and(|m| m.file_type().is_symlink())
        {
            if links >= MAX_LINKS {
                return Err(std::io::Error::other(format!(
                    "{}: too many symbolic links",
                    p.display()
                )));
            }
            // A relative target starts in the folder that holds the link.
            let mut next = head
                .parent()
                .unwrap_or(Path::new(""))
                .join(std::fs::read_link(&head)?);
            next.extend(&parts[k..]);
            return resolve_links(&std::path::absolute(&next)?, links + 1);
        }
        k -= 1;
    }
    // What is left does not exist, so it holds no links; but a `..` in it can step back into
    // what does exist (`new/../link`), so then the result is resolved again.
    let mut stepped_back = false;
    for c in &parts[k..] {
        match c {
            Component::CurDir => {}
            Component::ParentDir => {
                real.pop();
                stepped_back = true;
            }
            c => real.push(c.as_os_str()),
        }
    }
    if stepped_back {
        resolve_links(&real, links)
    } else {
        Ok(real)
    }
}

/// `path` (relative paths start at `dir`), a file or folder JETCARP is about to write inside
/// the folder `dir` under a name it chose itself (an animation's frames, an export's files, a
/// pile's steps, a store's objects), if it is still inside `dir` once both are resolved
/// ([`resolve`]); else refused. Returns the path to open: `dir` as given followed by the rest of
/// the resolved path, so the file opened is the one checked.
///
/// The folder rules check the folder a tool, a script or a job is given; the files JETCARP then
/// names inside it are checked here, since a symbolic link already at one of those names, or at
/// a folder on the way to it, would take the write somewhere else. Write sites call this with the
/// final path before they create a folder or open a file. It resolves the path instead of opening
/// with `O_NOFOLLOW` or refusing every link, because that way it works the same on Linux, macOS
/// and Windows (std's `canonicalize` and `read_link` on each), it covers the folders on the way
/// as well as the last name (`O_NOFOLLOW` covers only the last, and only on Unix), and a link
/// that stays inside `dir` keeps working. It checks the links that are there when it runs: a link
/// made between the check and the write would need something that can already write in `dir`.
///
/// A refusal names the path as JETCARP would write it, never where the link leads, and reads
/// the same whether or not that place exists.
pub fn inside(dir: &Path, path: impl AsRef<Path>) -> Result<PathBuf, String> {
    // A name in `dir`, or a path already under it (`dir.join(name)`, as several writers pass):
    // joining that onto a relative `dir` again would double it.
    let path = path.as_ref();
    let path = if path.starts_with(dir) {
        path.to_path_buf()
    } else {
        dir.join(path)
    };
    let refused = || {
        format!(
            "JETCARP may only write inside {} (got {}, where a symbolic link leads out of it)",
            dir.display(),
            path.display()
        )
    };
    let real = resolve(&path).map_err(|_| refused())?;
    let base = resolve(dir).map_err(|_| refused())?;
    let rest = real.strip_prefix(&base).map_err(|_| refused())?;
    Ok(if rest.as_os_str().is_empty() {
        dir.to_path_buf()
    } else {
        dir.join(rest)
    })
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

// Making symbolic links needs a privilege on Windows, so these tests run on Unix.
#[cfg(all(test, unix))]
mod tests {
    use super::*;

    /// A link at a name JETCARP writes, or at a folder on the way, that leads out of the folder
    /// is refused, naming the path as written and reading the same whether or not the place it
    /// leads to exists; a link that stays inside is followed.
    #[test]
    fn a_name_inside_a_folder_stays_inside_it() {
        use std::os::unix::fs::symlink;
        let d = std::env::temp_dir().join(format!("jetcarp-inside-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        let (dir, outside) = (d.join("dir"), d.join("outside"));
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::write(outside.join("there.png"), "kept").unwrap();
        symlink(outside.join("there.png"), dir.join("a.png")).unwrap();
        symlink(outside.join("missing.png"), dir.join("b.png")).unwrap();
        symlink(&outside, dir.join("frames")).unwrap();
        let a = inside(&dir, "a.png").unwrap_err();
        let b = inside(&dir, "b.png").unwrap_err();
        assert_eq!(a.replace("a.png", "X"), b.replace("b.png", "X"));
        assert!(a.contains("may only write inside"), "{a}");
        assert!(a.contains(&dir.join("a.png").display().to_string()), "{a}");
        assert!(!a.contains("outside"), "{a}");
        assert!(inside(&dir, "frames/0000.png").is_err());
        assert!(inside(&dir, "frames").is_err());
        assert!(inside(&dir, "sub/../../outside/x").is_err());
        // Names that are not there yet, and a link that stays inside.
        assert_eq!(inside(&dir, "new/x.png").unwrap(), dir.join("new/x.png"));
        symlink(dir.join("sub"), dir.join("alias")).unwrap();
        assert_eq!(inside(&dir, "alias/x.png").unwrap(), dir.join("sub/x.png"));
        assert_eq!(inside(&dir, dir.join("sub")).unwrap(), dir.join("sub"));
        // A relative folder: a path already under it is not joined on again.
        let rel = Path::new("no-such-folder-here");
        assert_eq!(inside(rel, rel.join("x.tmp")).unwrap(), rel.join("x.tmp"));
        assert_eq!(inside(rel, "x.tmp").unwrap(), rel.join("x.tmp"));
        // A loop of links is refused the same way.
        symlink(dir.join("loop-b"), dir.join("loop-a")).unwrap();
        symlink(dir.join("loop-a"), dir.join("loop-b")).unwrap();
        assert!(inside(&dir, "loop-a").is_err());
        assert_eq!(
            std::fs::read_to_string(outside.join("there.png")).unwrap(),
            "kept"
        );
        assert!(!outside.join("missing.png").exists());
        let _ = std::fs::remove_dir_all(&d);
    }
}
