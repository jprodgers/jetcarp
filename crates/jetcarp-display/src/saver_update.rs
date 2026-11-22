//! The macOS screen saver kept up to date (#287). macOS runs it sandboxed, so it shows what was
//! baked into its bundle when it was built ([`crate::macsaver`]). Whenever what it bakes changes
//! (the rotation or a scene in it, the caption or its font, the strobe guard setting or the
//! seizure warning's answer, the program itself), the installed bundle is built again, as
//! installing builds it: in a fresh folder, signed, then put in the old one's place.
//!
//! The check reads the files, so it sees a change however it was made: in the app, with the
//! command line or an AI tool, or by hand. The background host checks every couple of seconds
//! with its other settings, and so does the app while it is open; the command line checks once
//! after each command. The check itself is cheap (the size and time of each file the bake reads,
//! [`stamp`]); baking and building again happen only after a change, and on a thread of their own
//! in the host and the app. A rebuild that fails leaves the old bundle as it was, and the reason
//! is kept ([`STATE_FILE`]) for the app and `jetcarp doctor`.

use crate::macsaver::{self, BUNDLE, Baked};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// What the bundle was last built from, and the last rebuild's failure, in the settings folder.
pub const STATE_FILE: &str = "screen-saver-state.json";
/// How often the host and the app check.
pub const KEEP_EVERY: Duration = Duration::from_secs(2);

/// What [`STATE_FILE`] holds.
#[derive(Serialize, Deserialize, Default, Clone, Debug, PartialEq)]
pub struct BuiltFrom {
    /// The [`stamp`] the installed bundle was built from (or found to match).
    pub stamp: String,
    /// The files outside the settings folder the bake read: scene files, the font, its fallbacks.
    #[serde(default)]
    pub files: Vec<PathBuf>,
    /// The last rebuild that failed, if it came after the last that didn't.
    #[serde(default)]
    pub failed: Option<Failure>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Failure {
    /// The stamp it was for: it is tried again only after another change.
    pub stamp: String,
    pub why: String,
}

/// Where the bundle is built from and to, and how it is signed: the real ones in
/// [`keep_current`] and [`crate::macsaver::install`], stand-ins in tests.
pub struct Target<'a> {
    pub bundle: PathBuf,
    /// The saver's built code ([`crate::macsaver::built_dylib`]).
    pub dylib: PathBuf,
    pub sign: &'a (dyn Fn(&Path) -> Result<(), String> + Sync),
    /// The folders installed fonts are in, for the caption's last resort.
    pub installed_fonts: Vec<PathBuf>,
}

/// A stamp of what a bake reads, from settings folder `config`: its `host.json`, `app.json`,
/// `photosensitivity.json` and `paths.json`, its rotations and screensavers folders, `files`, the
/// saver's built code and this program's version.
pub fn stamp(config: &Path, files: &[PathBuf], dylib: &Path) -> String {
    use std::fmt::Write as _;
    let mut paths: Vec<PathBuf> = [
        "host.json",
        "app.json",
        jetcarp_core::photosensitivity::FILE,
        jetcarp_core::paths::PATHS_FILE,
    ]
    .iter()
    .map(|f| config.join(f))
    .collect();
    for d in ["rotations", "screensavers"] {
        if let Ok(rd) = std::fs::read_dir(config.join(d)) {
            paths.extend(rd.flatten().map(|e| e.path()));
        }
    }
    paths.extend(files.iter().cloned());
    paths.push(dylib.to_path_buf());
    paths.sort();
    paths.dedup();
    let mut text = format!("jetcarp {}\n", env!("CARGO_PKG_VERSION"));
    for p in &paths {
        let _ = match std::fs::metadata(p) {
            Ok(m) => {
                let t = m
                    .modified()
                    .ok()
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .map_or(0, |d| d.as_nanos());
                writeln!(text, "{} {} {t}", p.display(), m.len())
            }
            Err(_) => writeln!(text, "{} none", p.display()),
        };
    }
    jetcarp_core::sha256::sha256_hex(text.as_bytes())
}

/// What the bundle was built from, as saved in settings folder `config`.
pub fn built_from(config: &Path) -> BuiltFrom {
    std::fs::read_to_string(config.join(STATE_FILE))
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

fn save(config: &Path, state: &BuiltFrom) {
    let file = config.join(STATE_FILE);
    let tmp = config.join(format!("{STATE_FILE}.{}", std::process::id()));
    let text = serde_json::to_string_pretty(state).unwrap_or_default();
    if std::fs::create_dir_all(config).is_ok() && std::fs::write(&tmp, text + "\n").is_ok() {
        if std::fs::rename(&tmp, &file).is_err() {
            let _ = std::fs::remove_file(&tmp);
        }
    }
}

/// Whether the installed bundle is out of date: what it bakes has changed since it was built.
pub fn out_of_date(config: &Path, target: &Target) -> bool {
    let state = built_from(config);
    target.bundle.exists() && stamp(config, &state.files, &target.dylib) != state.stamp
}

/// Builds the bundle at `target.bundle` from `baked`, as installing does: written in a fresh
/// folder beside it and signed there, then put in the old one's place, so no file of a signed
/// bundle that macOS may have mapped is changed where it is. If anything fails, the old bundle is
/// left as it was.
pub fn build(target: &Target, baked: &Baked) -> Result<(), String> {
    let parent = target
        .bundle
        .parent()
        .ok_or("the screen saver's folder is unknown")?;
    std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
    // Hidden, so System Settings lists only the bundle itself.
    let staging = parent.join(format!(".jetcarp-rebuild-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&staging);
    let fresh = staging.join(BUNDLE);
    let built = macsaver::write_bundle(&fresh, &target.dylib, baked)
        .and_then(|()| (target.sign)(&fresh))
        .and_then(|()| swap(&fresh, &target.bundle, &staging.join("old")));
    let _ = std::fs::remove_dir_all(&staging);
    built
}

/// Puts `fresh` in `bundle`'s place, the old one moved to `old` first and back if that fails.
fn swap(fresh: &Path, bundle: &Path, old: &Path) -> Result<(), String> {
    let had = bundle.exists();
    if had {
        std::fs::rename(bundle, old).map_err(|e| format!("{}: {e}", bundle.display()))?;
    }
    if let Err(e) = std::fs::rename(fresh, bundle) {
        if had {
            let _ = std::fs::rename(old, bundle);
        }
        return Err(format!("{}: {e}", bundle.display()));
    }
    Ok(())
}

/// The stamp to record for a bundle baked from `baked`: none (so the next check bakes again) if
/// the settings changed while it was baked.
fn stamp_after(
    config: &Path,
    before: &str,
    old_files: &[PathBuf],
    baked: &Baked,
    dylib: &Path,
) -> String {
    if stamp(config, old_files, dylib) != before {
        String::new()
    } else {
        stamp(config, &baked.sources, dylib)
    }
}

/// Bakes the settings in `config` and builds the bundle, as installing does, and records what it
/// was built from. The bake's warnings.
pub fn install_at(config: &Path, target: &Target) -> Result<Vec<String>, String> {
    let old = built_from(config);
    let before = stamp(config, &old.files, &target.dylib);
    let (baked, warnings) = bake(config, target)?;
    build(target, &baked)?;
    let stamp = stamp_after(config, &before, &old.files, &baked, &target.dylib);
    save(
        config,
        &BuiltFrom {
            stamp,
            files: baked.sources.clone(),
            failed: None,
        },
    );
    Ok(warnings)
}

/// Whether the bundle at `bundle` already holds what `baked` would build from `dylib`: the same
/// settings, code and font files.
fn same_as_installed(bundle: &Path, baked: &Baked, dylib: &Path) -> bool {
    let contents = bundle.join("Contents");
    let resources = contents.join("Resources");
    let parse = |t: &str| serde_json::from_str::<serde_json::Value>(t).ok();
    let installed = std::fs::read_to_string(resources.join(macsaver::BAKED_FILE))
        .ok()
        .and_then(|t| parse(&t));
    // Both through text, so numbers compare as written.
    let new = serde_json::to_string(baked).ok().and_then(|t| parse(&t));
    if installed.is_none() || installed != new {
        return false;
    }
    let same_file = |path: PathBuf, bytes: &[u8]| std::fs::read(path).is_ok_and(|b| b == bytes);
    let code = std::fs::read(dylib).unwrap_or_default();
    let font = match (&baked.font, &baked.font_file) {
        (macsaver::BakedFont::File { file, .. }, Some(b)) => same_file(resources.join(file), &b.0),
        (macsaver::BakedFont::File { .. }, None) => false,
        _ => true,
    };
    same_file(contents.join("MacOS").join("JETCARP"), &code)
        && font
        && baked
            .fallbacks
            .iter()
            .all(|f| same_file(resources.join(&f.file), &f.bytes.0))
}

/// Held while one program rebuilds the bundle, so two (the host and the app) don't at once. One
/// older than [`LOCK_STALE`] was left by a program that stopped.
struct Lock(PathBuf);

const LOCK_STALE: Duration = Duration::from_secs(300);

impl Lock {
    fn take(bundle: &Path) -> Option<Lock> {
        let path = bundle.with_file_name(".jetcarp-rebuild.lock");
        let stale = std::fs::metadata(&path)
            .and_then(|m| m.modified())
            .is_ok_and(|t| t.elapsed().unwrap_or_default() > LOCK_STALE);
        if stale {
            let _ = std::fs::remove_file(&path);
        }
        std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .ok()
            .map(|_| Lock(path))
    }
}

impl Drop for Lock {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

fn bake(config: &Path, target: &Target) -> Result<(Baked, Vec<String>), String> {
    let settings = crate::host_settings::HostSettings::load().0;
    let name = macsaver::chosen_rotation(&settings);
    let font = crate::fonts::saved_in(config).0;
    let lookup = crate::stage::library_lookup();
    macsaver::bake(&name, &settings, &font, &target.installed_fonts, &*lookup)
}

/// Builds the installed bundle again when it is out of date. None when there was nothing to do;
/// else a note that it was brought up to date, or why it couldn't be (the old bundle kept).
pub fn keep_current_at(config: &Path, target: &Target) -> Option<Result<String, String>> {
    if !target.bundle.exists() {
        return None;
    }
    let state = built_from(config);
    let now = stamp(config, &state.files, &target.dylib);
    let tried = state.failed.as_ref().is_some_and(|f| f.stamp == now);
    if now == state.stamp || tried {
        return None;
    }
    let _lock = Lock::take(&target.bundle)?;
    let failed = |why: String| {
        let mut kept = state.clone();
        kept.failed = Some(Failure {
            stamp: now.clone(),
            why: why.clone(),
        });
        save(config, &kept);
        Err(why)
    };
    let (baked, warnings) = match bake(config, target) {
        Ok(b) => b,
        Err(e) => return Some(failed(e)),
    };
    let record = BuiltFrom {
        stamp: stamp_after(config, &now, &state.files, &baked, &target.dylib),
        files: baked.sources.clone(),
        failed: None,
    };
    if same_as_installed(&target.bundle, &baked, &target.dylib) {
        save(config, &record);
        return None;
    }
    match build(target, &baked) {
        Ok(()) => {
            save(config, &record);
            // What installing would have warned of (an item showing Life, a font left out).
            let mut said =
                String::from("JETCARP's screen saver was brought up to date with the settings");
            for w in warnings {
                said += &format!("; {w}");
            }
            Some(Ok(said))
        }
        Err(e) => Some(failed(e)),
    }
}

/// Why the last rebuild failed, if the bundle is still out of date because of it.
pub fn failure(config: &Path) -> Option<String> {
    built_from(config).failed.map(|f| f.why)
}

/// One line on the installed bundle when it isn't up to date, for `jetcarp doctor`: why its last
/// rebuild failed, or that it is out of date.
pub fn note(config: &Path, target: &Target) -> Option<String> {
    if !target.bundle.exists() {
        return None;
    }
    if let Some(f) = built_from(config).failed {
        return Some(format!(
            "It couldn't be brought up to date with the settings ({}), so it shows what it did \
             before; install it again: `jetcarp host saver install`.",
            f.why.trim_end_matches('.')
        ));
    }
    out_of_date(config, target).then(|| {
        "It is out of date with the settings: the app or the background host brings it up to \
         date when it next runs, or install it again: `jetcarp host saver install`."
            .into()
    })
}

/// The installed bundle and how it is built here, on a Mac where it is installed.
pub fn installed_target() -> Option<Target<'static>> {
    if !cfg!(target_os = "macos") {
        return None;
    }
    let bundle = macsaver::bundle_path().ok().filter(|b| b.exists())?;
    // Where it would be when it isn't found: a rebuild then fails, and says so.
    let dylib = macsaver::built_dylib().unwrap_or_else(|_| {
        std::env::current_exe()
            .map(|e| e.with_file_name(macsaver::DYLIB))
            .unwrap_or_default()
    });
    Some(Target {
        bundle,
        dylib,
        sign: &macsaver::sign,
        installed_fonts: crate::fonts::system_dirs(),
    })
}

/// [`keep_current_at`] for the installed saver and the settings folder, on a Mac where it is
/// installed: what the command line runs after a command.
pub fn keep_current() -> Option<Result<String, String>> {
    let config = crate::host_settings::config_dir()?;
    keep_current_at(&config, &installed_target()?)
}

/// [`note`] for the installed saver, for `jetcarp doctor` and `host saver status`.
pub fn installed_note() -> Option<String> {
    let config = crate::host_settings::config_dir()?;
    note(&config, &installed_target()?)
}

/// Keeps the installed saver up to date from a program that runs a while (the background host,
/// the app): it checks every [`KEEP_EVERY`], cheaply, and bakes and builds again on a thread of
/// its own, so its frames never wait.
#[derive(Default)]
pub struct Keeper {
    busy: Option<std::sync::mpsc::Receiver<Option<Result<String, String>>>>,
    at: Option<Instant>,
    failed: Option<String>,
}

impl Keeper {
    /// Check, if it is time; what the last rebuild that finished said (brought up to date, or
    /// why it couldn't be).
    pub fn poll(&mut self) -> Option<Result<String, String>> {
        if let Some(rx) = &self.busy {
            match rx.try_recv() {
                Ok(said) => {
                    self.busy = None;
                    return said;
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => return None,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => self.busy = None,
            }
        }
        if self.at.is_some_and(|t| t.elapsed() < KEEP_EVERY * 3 / 4) {
            return None;
        }
        self.at = Some(Instant::now());
        let config = crate::host_settings::config_dir()?;
        let target = installed_target()?;
        let state = built_from(&config);
        self.failed = state.failed.as_ref().map(|f| f.why.clone());
        let now = stamp(&config, &state.files, &target.dylib);
        if now == state.stamp || state.failed.is_some_and(|f| f.stamp == now) {
            return None;
        }
        let (tx, rx) = std::sync::mpsc::channel();
        let spawned = std::thread::Builder::new()
            .name("screen saver rebuild".into())
            .spawn(move || {
                let _ = tx.send(keep_current_at(&config, &target));
            });
        if spawned.is_ok() {
            self.busy = Some(rx);
        }
        None
    }

    /// Why the last rebuild failed, as last checked (by this program or another): the old bundle
    /// is still installed.
    pub fn failed(&self) -> Option<&str> {
        self.failed.as_deref()
    }
}
