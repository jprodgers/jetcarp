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

/// The length, in bytes, from which JETCARP refuses a path it is asked to read or write: Linux's
/// `PATH_MAX`, which counts the byte that ends the path. So every path Linux accepts, JETCARP
/// accepts, and on Windows, whose long paths reach 32,767 characters, JETCARP refuses the same
/// paths as on Linux (Jimmie, 20 November 2026). macOS's own limit, 1,024 bytes, still applies
/// there. A path is counted as UTF-8 (WTF-8 on Windows), so the same text counts the same on
/// every system.
pub const MAX_PATH_BYTES: usize = 4096;

/// The refusal [`resolve`] gives for a path of [`MAX_PATH_BYTES`] or more, so that the folder
/// rules can give its own sentence ([`too_long`]) instead of theirs.
#[derive(Debug)]
struct TooLong(String);

impl std::fmt::Display for TooLong {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for TooLong {}

/// `p` as given, made absolute by joining it to the working directory, without working out its
/// `.` and `..` (as the system receives it).
fn joined(p: &Path) -> PathBuf {
    if p.is_absolute() {
        p.to_path_buf()
    } else {
        std::env::current_dir().unwrap_or_default().join(p)
    }
}

/// `n` with a comma between each group of three digits.
fn thousands(n: usize) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// Nothing if `p` (absolute, or relative to the working directory) is shorter than
/// [`MAX_PATH_BYTES`]; else the sentence that refuses it: its first characters, its length and
/// the limit. It looks only at the text, so it reads the same whether or not the path exists.
pub fn check_length(p: &Path) -> Result<(), String> {
    let full = joined(p);
    let n = full.as_os_str().len();
    if n < MAX_PATH_BYTES {
        return Ok(());
    }
    let shown: String = full.to_string_lossy().chars().take(60).collect();
    Err(format!(
        "the path {shown}… is {} bytes long; JETCARP reads and writes only paths shorter than {} \
         bytes, the same on every system",
        thousands(n),
        thousands(MAX_PATH_BYTES)
    ))
}

/// The sentence of a refusal [`resolve`] gave because the path is too long ([`check_length`]),
/// if `e` is one.
pub fn too_long(e: &std::io::Error) -> Option<String> {
    e.get_ref()
        .and_then(|inner| inner.downcast_ref::<TooLong>())
        .map(|t| t.0.clone())
}

/// The names Windows keeps for its devices. A file or folder by one of these names, with or
/// without an extension, cannot be made or opened as an ordinary file on many Windows systems, and
/// tools there struggle to remove one made elsewhere.
const RESERVED: [&str; 30] = [
    "CON", "PRN", "AUX", "NUL", "COM0", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7",
    "COM8", "COM9", "COM¹", "COM²", "COM³", "LPT0", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6",
    "LPT7", "LPT8", "LPT9", "LPT¹", "LPT²", "LPT³",
];

/// The Windows device name `name` stands for, if any: the part before its first `.`, with
/// trailing spaces dropped, compared without regard to letter case (`CON`, `con.png`,
/// `Nul.tar.gz`, `AUX ` and `com1.` all are; `console.json`, `com10.json` and `lpt.json` are not).
pub fn reserved_on_windows(name: &str) -> Option<&'static str> {
    let base = name.split('.').next().unwrap_or(name).trim_end_matches(' ');
    let upper = base.to_uppercase();
    RESERVED.iter().find(|r| **r == upper).copied()
}

/// Nothing if `name` (one file or folder name) can be copied to any system; else the sentence
/// that refuses it. A `:` (on Windows it names a hidden data stream of a file: `plain.json:other`
/// writes beside `plain.json`, out of sight), Windows' device names ([`reserved_on_windows`]),
/// and a name ending in a dot or a space (Windows drops those, so `scene.json.` would become
/// `scene.json` there) are refused on every system, so that folders and scenes can be copied
/// between systems. `.` and `..` are not names and keep their meaning. It looks only at the text,
/// so it reads the same whether or not the name exists.
pub fn check_name(name: &str) -> Result<(), String> {
    if name == "." || name == ".." {
        return Ok(());
    }
    if name.contains(':') {
        return Err(format!(
            "a `:` in a name (`{name}`) is refused on every system: on Windows it names a hidden \
             data stream, so the name cannot be copied there"
        ));
    }
    if let Some(device) = reserved_on_windows(name) {
        let shown = if name == device {
            format!("`{name}`")
        } else {
            format!("`{name}` (as `{device}`)")
        };
        return Err(format!(
            "the name {shown} is reserved on Windows; JETCARP refuses it on every system so that \
             folders and scenes can be copied between systems"
        ));
    }
    for (end, what) in [('.', "a dot"), (' ', "a space")] {
        if name.ends_with(end) {
            return Err(format!(
                "the name `{name}` ends in {what}; Windows drops it, so JETCARP refuses it on \
                 every system so that folders and scenes can be copied between systems"
            ));
        }
    }
    Ok(())
}

/// [`check_name`] for each name in `rel` (its folders and its last name). A root, a drive (`C:`)
/// and `.` or `..` are not names.
pub fn check_names(rel: &Path) -> Result<(), String> {
    for c in rel.components() {
        if let std::path::Component::Normal(n) = c {
            check_name(&n.to_string_lossy())?;
        }
    }
    Ok(())
}

/// One part of a path as text: a name (`Some`), or a drive or root (`None` names, the text kept
/// for comparing).
type Part = (bool, String);

/// The parts of `p` made absolute against the working directory, with `.` and `..` worked out by
/// name and nothing looked up (unlike [`std::path::absolute`] on Windows, which can turn a device
/// name into the device). They stay text and are never put back together into a path: on
/// Windows a name such as `a:b.png` pushed onto a path on its own reads as the drive `A:` and
/// replaces everything before it.
fn lexical_parts(p: &Path) -> Vec<Part> {
    let mut out: Vec<Part> = Vec::new();
    for c in joined(p).components() {
        match c {
            std::path::Component::ParentDir => {
                if out.last().is_some_and(|(name, _)| *name) {
                    out.pop();
                }
            }
            std::path::Component::CurDir => {}
            std::path::Component::Normal(n) => out.push((true, n.to_string_lossy().into_owned())),
            c => out.push((false, c.as_os_str().to_string_lossy().into_owned())),
        }
    }
    out
}

/// The names that follow `dir`'s parts in `path`'s, if `path` is under `dir` by name: letter case
/// ignored on Windows and macOS, whose usual file systems ignore it.
fn below_by_name(dir: &[Part], path: &[Part]) -> Option<Vec<String>> {
    let fold = |s: &str| {
        if cfg!(any(windows, target_os = "macos")) {
            s.to_lowercase()
        } else {
            s.to_string()
        }
    };
    if path.len() < dir.len() {
        return None;
    }
    for ((_, d), (_, p)) in dir.iter().zip(path) {
        if fold(d) != fold(p) {
            return None;
        }
    }
    Some(path[dir.len()..].iter().map(|(_, n)| n.clone()).collect())
}

/// [`check_names`] for the names `path` has below the folder `dir`, worked out by name before
/// anything is looked up. The folder's own path is not checked: it is the person's own. A path
/// not under `dir` by name passes here (it is outside, or reaches `dir` another way, and the
/// caller checks what it resolves to: [`check_names_beyond`]).
pub fn check_names_below(dir: &Path, path: &Path) -> Result<(), String> {
    match below_by_name(&lexical_parts(dir), &lexical_parts(path)) {
        Some(names) => names.iter().try_for_each(|n| check_name(n)),
        None => Ok(()),
    }
}

/// [`check_name`] for each name of `full` after the parts of `base` (a path `full` is known to
/// start with: a resolved path and its resolved folder). The names are read from `full` itself,
/// whose drive and root come first, rather than from what `strip_prefix` leaves, which on Windows
/// would read a first name such as `a:b.png` as a drive.
pub fn check_names_beyond(base: &Path, full: &Path) -> Result<(), String> {
    for c in full.components().skip(base.components().count()) {
        if let std::path::Component::Normal(n) = c {
            check_name(&n.to_string_lossy())?;
        }
    }
    Ok(())
}

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
///
/// A path of [`MAX_PATH_BYTES`] or more, as given, is refused before anything is looked up
/// ([`too_long`] gives the sentence).
pub fn resolve(p: &Path) -> std::io::Result<PathBuf> {
    check_length(p)
        .map_err(|s| std::io::Error::new(std::io::ErrorKind::InvalidInput, TooLong(s)))?;
    resolve_links(&std::path::absolute(p)?, 0)
}

/// How many of `parts`' leading components exist (each prefix looked up with
/// [`std::fs::symlink_metadata`], so a link that leads nowhere still counts), found by halving:
/// a few lookups for a path of any depth, where looking up every folder on the way costs one
/// per folder, each longer than the last. A folder that does not exist has nothing under it, so
/// once a prefix is missing every longer one is. None when a lookup fails for another reason (a
/// loop of links, a folder that may not be searched): then nothing is assumed.
fn existing_prefix(parts: &[std::path::Component]) -> Option<usize> {
    use std::io::ErrorKind;
    let (mut lo, mut hi) = (0, parts.len() + 1);
    while hi - lo > 1 {
        let mid = (lo + hi) / 2;
        let head: PathBuf = parts[..mid].iter().collect();
        match std::fs::symlink_metadata(&head) {
            Ok(_) => lo = mid,
            Err(e) if matches!(e.kind(), ErrorKind::NotFound | ErrorKind::NotADirectory) => {
                hi = mid
            }
            Err(_) => return None,
        }
    }
    Some(lo)
}

/// [`resolve`] for an absolute path, `links` links into it.
fn resolve_links(p: &Path, links: u32) -> std::io::Result<PathBuf> {
    use std::path::Component;
    // A link may make the path longer than the one given: past the limit, the system would not
    // open it either. The refusal says nothing about where the link leads.
    if p.as_os_str().len() >= MAX_PATH_BYTES {
        return Err(std::io::Error::other(
            "the path is too long once its links are followed",
        ));
    }
    // The whole path exists: one call.
    if let Ok(r) = std::fs::canonicalize(p) {
        return Ok(r);
    }
    let parts: Vec<Component> = p.components().collect();
    // The deepest prefix that exists is where the folders still to be made start; nothing
    // deeper can be resolved, or be a link. Then, from there up, as before: the deepest prefix
    // that resolves, unless a link that leads nowhere comes first.
    let mut k = existing_prefix(&parts).unwrap_or(parts.len());
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
/// the same whether or not that place exists. A path of [`MAX_PATH_BYTES`] or more, and a name
/// below `dir` that cannot be copied to every system ([`check_name`]: a `:`, a Windows device
/// name), are refused in their own words, before anything is looked up.
pub fn inside(dir: &Path, path: impl AsRef<Path>) -> Result<PathBuf, String> {
    // A name in `dir`, or a path already under it (`dir.join(name)`, as several writers pass):
    // joining that onto a relative `dir` again would double it.
    let path = path.as_ref();
    // On Windows a name that starts with one letter and a `:` (`a:b.png`) reads as a drive, and
    // joining it onto `dir` would replace `dir`: refuse it as the name it is.
    if !path.is_absolute()
        && matches!(
            path.components().next(),
            Some(std::path::Component::Prefix(_))
        )
    {
        let first = path.to_string_lossy();
        let first = first.split(['/', '\\']).next().unwrap_or_default();
        check_name(first)?;
    }
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
    check_length(&path)?;
    check_names_below(dir, &path)?;
    let real = resolve(&path).map_err(|e| too_long(&e).unwrap_or_else(refused))?;
    let base = resolve(dir).map_err(|_| refused())?;
    let rest = real.strip_prefix(&base).map_err(|_| refused())?;
    // The names the write will really make, if a link on the way changed them.
    check_names_beyond(&base, &real)?;
    let open = if rest.as_os_str().is_empty() {
        dir.to_path_buf()
    } else {
        dir.join(rest)
    };
    check_length(&open)?;
    Ok(open)
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

#[cfg(test)]
mod length_tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("jetcarp-len-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("sub")).unwrap();
        d
    }

    /// A name that makes `dir/<name>` exactly `len` bytes long.
    fn name_for(dir: &Path, len: usize) -> String {
        let room = len - dir.join("x").as_os_str().len() + 1;
        "n".repeat(room)
    }

    /// A path one byte under the limit is let through; at the limit and over it, it is refused
    /// with its length and the limit, before anything is looked up, so the refusal reads the
    /// same whether or not the folders on the way exist.
    #[test]
    fn a_path_of_4096_bytes_or_more_is_refused_in_its_own_words() {
        let d = scratch("limit");
        let under = d.join(name_for(&d, MAX_PATH_BYTES - 1));
        assert_eq!(under.as_os_str().len(), MAX_PATH_BYTES - 1);
        assert!(check_length(&under).is_ok());
        for len in [MAX_PATH_BYTES, MAX_PATH_BYTES + 1, 6000] {
            let p = d.join(name_for(&d, len));
            assert_eq!(p.as_os_str().len(), len);
            let e = check_length(&p).unwrap_err();
            assert!(
                e.contains(&format!("is {} bytes long", thousands(len))),
                "{e}"
            );
            assert!(e.contains("shorter than 4,096 bytes"), "{e}");
            let r = resolve(&p).unwrap_err();
            assert_eq!(too_long(&r).as_deref(), Some(e.as_str()));
        }
        // The same answer whether or not the folder on the way exists: one path, refused before
        // its folder is made and after. (The refusal shows only the path's first characters, so
        // two different paths under a long temporary folder, as on Windows, can't be compared.)
        let place = d.join("sub");
        let p = place.join(name_for(&place, 5000));
        let _ = std::fs::remove_dir_all(&place);
        let missing = check_length(&p).unwrap_err();
        let missing_resolved = too_long(&resolve(&p).unwrap_err());
        std::fs::create_dir_all(&place).unwrap();
        assert_eq!(check_length(&p).unwrap_err(), missing);
        assert_eq!(too_long(&resolve(&p).unwrap_err()), missing_resolved);
        assert_eq!(missing_resolved.as_deref(), Some(missing.as_str()));
        // A name JETCARP writes inside a folder is refused in the same words, not as a link.
        let e = inside(&d, name_for(&d, 5000)).unwrap_err();
        assert!(
            e.contains("bytes long") && !e.contains("symbolic link"),
            "{e}"
        );
        let _ = std::fs::remove_dir_all(&d);
    }

    /// A path many folders deep, none of them made yet, is resolved to itself and can be
    /// written (macOS's own limit, 1,024 bytes, allows fewer folders there).
    #[test]
    fn a_path_many_folders_deep_is_resolved_and_written() {
        let d = scratch("deep");
        let depth = if cfg!(target_os = "macos") { 400 } else { 1500 };
        let rel = format!("{}deep.json", "a/".repeat(depth));
        let p = inside(&d, &rel).unwrap();
        assert_eq!(p, d.join(&rel));
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, "{}").unwrap();
        // Again, now that the folders exist, and for a new name beside the file.
        assert_eq!(inside(&d, &rel).unwrap(), p);
        assert_eq!(
            resolve(&p.with_file_name("new.json")).unwrap(),
            resolve(&d)
                .unwrap()
                .join(rel.replace("deep.json", "new.json"))
        );
        assert_eq!(std::fs::read_to_string(&p).unwrap(), "{}");
        let _ = std::fs::remove_dir_all(&d);
    }
}

#[cfg(test)]
mod name_tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("jetcarp-names-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    /// Windows' device names, with any extension, in any letter case, with trailing spaces or
    /// dots, are reserved; names that only start with the same letters are not.
    #[test]
    fn windows_device_names_are_known_in_every_form() {
        for name in [
            "CON",
            "con",
            "Con",
            "PRN",
            "AUX",
            "NUL",
            "nul.json",
            "con.png",
            "Nul.tar.gz",
            "AUX ",
            "com1.",
            "COM0",
            "COM9.json",
            "LPT0",
            "lpt9.txt",
            "COM¹",
            "com².png",
            "LPT³",
            "CON .txt",
        ] {
            assert!(reserved_on_windows(name).is_some(), "{name:?}");
            let e = check_name(name).unwrap_err();
            assert!(e.contains("is reserved on Windows"), "{e}");
            assert!(e.contains("refuses it on every system"), "{e}");
        }
        for name in [
            "console.json",
            "nullable.png",
            "com10.json",
            "lpt.json",
            "auxiliary",
            "prnt",
            "COM",
            "LPT10",
            "my-con.json",
            "concat",
            ".con",
            " CON",
        ] {
            assert!(reserved_on_windows(name).is_none(), "{name:?}");
            assert!(check_name(name).is_ok(), "{name:?}");
        }
        assert_eq!(
            check_name("CON").unwrap_err(),
            "the name `CON` is reserved on Windows; JETCARP refuses it on every system so that \
             folders and scenes can be copied between systems"
        );
        assert!(
            check_name("nul.json")
                .unwrap_err()
                .contains("`nul.json` (as `NUL`)")
        );
    }

    /// A `:` in a file's name or a folder's is refused in every place.
    #[test]
    fn a_colon_in_a_name_is_refused() {
        assert_eq!(
            check_name("plain.json:other").unwrap_err(),
            "a `:` in a name (`plain.json:other`) is refused on every system: on Windows it names \
             a hidden data stream, so the name cannot be copied there"
        );
        for rel in [
            "plain.json:other",
            "plain.json::$DATA",
            "plain.json:other:$DATA",
            "run 12:30/scene.json",
            "a/b:c/d.png",
        ] {
            let e = check_names(Path::new(rel)).unwrap_err();
            assert!(e.contains("a `:` in a name"), "{rel}: {e}");
        }
    }

    /// A name ending in a dot or a space is refused, as a file and as a folder on the way; `.`
    /// and `..` keep their meaning, and dots inside a name are fine.
    #[test]
    fn a_name_ending_in_a_dot_or_a_space_is_refused() {
        assert_eq!(
            check_name("scene.json.").unwrap_err(),
            "the name `scene.json.` ends in a dot; Windows drops it, so JETCARP refuses it on \
             every system so that folders and scenes can be copied between systems"
        );
        assert_eq!(
            check_name("scene.json ").unwrap_err(),
            "the name `scene.json ` ends in a space; Windows drops it, so JETCARP refuses it on \
             every system so that folders and scenes can be copied between systems"
        );
        for name in ["name.", "name ", "...", "a.b.", "x. ", "run  "] {
            assert!(check_name(name).is_err(), "{name:?}");
        }
        for name in [".", "..", "a.b.json", ".hidden", "v1.2.3.png", " lead"] {
            assert!(check_name(name).is_ok(), "{name:?}");
        }
        assert!(check_names(Path::new("dir./x.json")).is_err());
        assert!(check_names(Path::new("dir /x.json")).is_err());
        assert!(check_names(Path::new(".../x.json")).is_err());
        assert!(check_names(Path::new("./a/../b/a.b.json")).is_ok());
        let dir = scratch("dots");
        for rel in ["name.", "name ", "...", "dir./x.json", "sub/dir /x.png"] {
            let e = inside(&dir, rel).unwrap_err();
            assert!(e.contains("ends in a"), "{rel}: {e}");
        }
        assert!(!dir.join("dir.").exists() && !dir.join("sub").exists());
        assert_eq!(inside(&dir, "a.b.json").unwrap(), dir.join("a.b.json"));
        assert_eq!(inside(&dir, "x/../a.c.json").unwrap(), dir.join("a.c.json"));
        assert!(check_names_below(&dir, &dir.join("deep/./../ok.json")).is_ok());
        assert!(check_names_below(&dir, &dir.join("deep./x.json")).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Every folder on the way is checked, not only the last name, and `.` and `..` are not
    /// names.
    #[test]
    fn every_name_on_the_way_is_checked() {
        assert!(check_names(Path::new("AUX/x.json")).is_err());
        assert!(check_names(Path::new("a/con/b/x.json")).is_err());
        assert!(check_names(Path::new("a/./b/../x.json")).is_ok());
        assert!(check_names(Path::new("/srv/out/x.json")).is_ok());
    }

    /// Below a folder, the names are checked by name before anything is looked up, `..` worked
    /// out (so `deep/../CON.json` is `CON.json`); the folder's own path is not checked.
    #[test]
    fn names_below_a_folder_are_checked_and_the_folder_itself_is_not() {
        let root = scratch("below");
        let dir = root.join("aux"); // the person's own folder may have any name
        std::fs::create_dir_all(&dir).unwrap();
        assert!(check_names_below(&dir, &dir.join("scene.json")).is_ok());
        assert!(check_names_below(&dir, &dir.join("CON.json")).is_err());
        assert!(check_names_below(&dir, &dir.join("deep/../nul.json")).is_err());
        assert!(check_names_below(&dir, &dir.join("ok/AUX/x.png")).is_err());
        // Not under the folder by name: left to what it resolves to.
        assert!(check_names_below(&dir, &root.join("elsewhere/CON")).is_ok());
        let _ = std::fs::remove_dir_all(&root);
    }

    /// A name JETCARP writes inside a folder is refused in the same words whether or not it
    /// exists, and before anything is made; ordinary names are written.
    #[test]
    fn inside_refuses_names_every_system_cannot_hold() {
        let dir = scratch("inside");
        for rel in [
            "CON",
            "nul.json",
            "AUX/x.json",
            "plain.json:other",
            "sub/a:b.png",
            "a:b.png",
        ] {
            let e = inside(&dir, rel).unwrap_err();
            assert!(
                e.contains("reserved on Windows") || e.contains("a `:` in a name"),
                "{rel}: {e}"
            );
            assert!(!e.contains("symbolic link"), "{rel}: {e}");
        }
        assert!(!dir.join("AUX").exists() && !dir.join("sub").exists());
        // The same answer once a folder on the way exists.
        let missing = inside(&dir, "made/CON.png").unwrap_err();
        std::fs::create_dir_all(dir.join("made")).unwrap();
        assert_eq!(inside(&dir, "made/CON.png").unwrap_err(), missing);
        for rel in ["console.json", "nullable.png", "com10.json", "lpt.json"] {
            assert_eq!(inside(&dir, rel).unwrap(), dir.join(rel));
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// On Windows a name that starts with one letter and a `:` reads as a drive when it stands
    /// alone; it is still a name below a folder, with either separator, and still refused. (The
    /// paths are built from text, not with `join`, which would let such a name replace them.)
    #[cfg(windows)]
    #[test]
    fn on_windows_a_name_like_a_drive_is_still_refused_as_a_name() {
        let dir = scratch("drive-like");
        for rel in [
            "sub/a:b.png",
            r"sub\a:b.png",
            r"x/y\a:b.png",
            "a:b.png",
            r"a:b\c.png",
        ] {
            let e = inside(&dir, rel).unwrap_err();
            assert!(e.contains("a `:` in a name"), "{rel}: {e}");
        }
        let under = |rel: &str| PathBuf::from(format!("{}\\{rel}", dir.display()));
        for rel in ["a:b.png", "sub/a:b.png", r"sub\x/a:b.png"] {
            assert!(check_names_below(&dir, &under(rel)).is_err(), "{rel}");
            assert!(check_names_beyond(&dir, &under(rel)).is_err(), "{rel}");
        }
        assert!(check_names_beyond(&dir, &under("sub/ok.png")).is_ok());
        assert!(!dir.join("sub").exists() && !dir.join("x").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A drive at the start of a Windows path is not a name, so `C:\…` is not refused as a `:`.
    #[cfg(windows)]
    #[test]
    fn a_drive_letter_is_not_a_name() {
        assert!(check_names(Path::new(r"C:\Users\x\scene.json")).is_ok());
        let dir = scratch("drive");
        let p = inside(&dir, "scene.json").unwrap();
        assert!(p.to_string_lossy().contains(':'));
        let _ = std::fs::remove_dir_all(&dir);
    }
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
        assert!(inside(&dir, "loop-a/new/x.png").is_err());
        // Links at the end of folders that exist, many deep, with new folders after them: the
        // deepest folder that exists is found without looking up each one on the way.
        let deep: PathBuf = (0..60).map(|i| format!("d{i}")).collect();
        std::fs::create_dir_all(dir.join(&deep)).unwrap();
        symlink(&outside, dir.join(&deep).join("out")).unwrap();
        symlink(outside.join("missing"), dir.join(&deep).join("gone")).unwrap();
        symlink(dir.join("sub"), dir.join(&deep).join("stay")).unwrap();
        for p in [
            "out/new/a/x.png",
            "gone",
            "gone/new/x.png",
            "stay/../../x.png",
        ] {
            assert!(inside(&dir, deep.join(p)).is_err(), "{p}");
        }
        assert_eq!(
            inside(&dir, deep.join("stay/new/a/x.png")).unwrap(),
            dir.join("sub/new/a/x.png")
        );
        assert_eq!(
            inside(&dir, deep.join("new/../stay/x.png")).unwrap(),
            dir.join("sub/x.png")
        );
        assert!(!outside.join("new").exists() && !outside.join("missing").exists());
        assert_eq!(
            std::fs::read_to_string(outside.join("there.png")).unwrap(),
            "kept"
        );
        assert!(!outside.join("missing.png").exists());
        let _ = std::fs::remove_dir_all(&d);
    }
}
