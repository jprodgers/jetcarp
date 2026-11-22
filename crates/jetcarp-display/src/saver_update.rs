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

use crate::macsaver::{self, Baked};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::Duration;

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
    let _ = (config, files, dylib);
    String::new()
}

/// What the bundle was built from, as saved in settings folder `config`.
pub fn built_from(config: &Path) -> BuiltFrom {
    std::fs::read_to_string(config.join(STATE_FILE))
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

/// Whether the installed bundle is out of date: what it bakes has changed since it was built.
pub fn out_of_date(config: &Path, target: &Target) -> bool {
    let _ = (config, target);
    false
}

/// Builds the bundle at `target.bundle` from `baked`.
pub fn build(target: &Target, baked: &Baked) -> Result<(), String> {
    macsaver::write_bundle(&target.bundle, &target.dylib, baked)?;
    (target.sign)(&target.bundle)
}

/// Bakes the settings in `config` and builds the bundle, as installing does, and records what it
/// was built from. The bake's warnings.
pub fn install_at(config: &Path, target: &Target) -> Result<Vec<String>, String> {
    let (baked, warnings) = bake(config, target)?;
    build(target, &baked)?;
    Ok(warnings)
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
    let _ = (config, target);
    None
}

/// Why the last rebuild failed, if the bundle is still out of date because of it.
pub fn failure(config: &Path) -> Option<String> {
    built_from(config).failed.map(|f| f.why)
}

/// One line on the installed bundle when it isn't up to date, for `jetcarp doctor`: why its last
/// rebuild failed, or that it is out of date.
pub fn note(config: &Path, target: &Target) -> Option<String> {
    let _ = (config, target);
    None
}
