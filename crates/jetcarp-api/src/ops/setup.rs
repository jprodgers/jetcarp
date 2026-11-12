//! First-run setup: `setup.status`, `setup.set_data_dir`, `setup.apply`. The app's setup wizard
//! and `jetcarp setup` both call these, so a profile means the same from either.
//!
//! **Profiles** ([`Profile`]) say what a new user wants JETCARP for:
//!
//! | Profile | Data folder | Packs | Quick Start build | Desktop |
//! | --- | --- | --- | --- | --- |
//! | Desktop art | default | none (the built-in examples) | none | offers the live wallpaper and the screen saver |
//! | Explorer | default | the starter pack | 10 minutes (optional) | – |
//! | Researcher | chosen (big libraries) | the starter pack and packs chosen | 30 minutes | – |
//! | Everything | chosen | every bundled pack and packs chosen | 60 minutes | offers |
//! | Blank slate | default | none | none | – |
//!
//! Every profile keeps the built-in examples in the Quick Start. "Offers" means the front end
//! asks (the app's last page, or a question in the terminal); nothing on the desktop changes
//! without that, and these operations never touch it.
//!
//! **What applying does.** It records the choice in the settings folder (`setup.json`: the
//! profile, the date, the jobs it started), then starts the profile's long work as catalog jobs,
//! so the dashboard and `jetcarp jobs` show them and a stopped one resumes: a `pack_import` job
//! for each pack to import (a bundled pack already imported is skipped), and a Quick Start build
//! (`quickstart.build`: an explore search of so many minutes, on the GPU when there is one, whose
//! atlas is then published into the Quick Start). The jobs run here one after another, or with
//! `background` on threads of this process.
//!
//! **The data folder** can be chosen once, for big libraries (`setup.set_data_dir`): it is
//! written to `paths.json` in the settings folder, which every lookup of the data folder then
//! honours (`JETCARP_DATA_DIR` still wins). A library is never moved: a folder that is not empty
//! is refused unless it already holds a JETCARP library.
//!
//! **The bundled packs** are the folder of pack stores that came with the program
//! ([`jetcarp_core::paths::bundled_packs_dir`]). The *starter pack* is the one whose feed or
//! folder is named `starter` (or has `starter` in its name); when none is, every bundled pack
//! counts as the starter pack.

use super::library::catalog_err;
use super::search::{BudgetIn, ExploreIn, FilesIn, How, SearchStarted, run_job};
use super::system::{folder_size, human_bytes};
use crate::ctx::{Ctx, Paths};
use crate::error::ApiError;
use crate::op::{NoInput, Op, Progress, Report};
use jetcarp_core::paths as cpaths;
use jetcarp_pack::import_job::{KIND_NAME as PACK_IMPORT, PackImportSpec};
use jetcarp_pack::read::Source;
use jetcarp_search::jobs::{self, JobSpec};
use jetcarp_store::{JobStatus, Library};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

/// The file in the settings folder that records the setup ([`SetupRecord`]).
pub const RECORD_FILE: &str = "setup.json";

/// The label of a Quick Start build's job.
pub use super::quickstart::BUILD_LABEL;

/// About how much a Quick Start build adds to the data folder per minute (its runs, about 700
/// bytes each, at a GPU's 200 a second, and the atlas), for the estimates people see before
/// choosing.
const BUILD_BYTES_PER_MINUTE: u64 = 8_000_000;

/// About how much a pack's import adds to the library for each byte of the pack.
const IMPORT_GROWTH: f64 = 2.0;

// ---------------------------------------------------------------------------------------------
// Profiles

/// What a new user wants JETCARP for: what setup imports, builds and offers.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum Profile {
    DesktopArt,
    Explorer,
    Researcher,
    Everything,
    BlankSlate,
}

/// Which bundled packs a profile imports.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PackChoice {
    /// None: the built-in examples only.
    None,
    /// The starter pack.
    Starter,
    /// Every pack that came with the program.
    All,
}

impl Profile {
    pub const ALL: [Profile; 5] = [
        Profile::DesktopArt,
        Profile::Explorer,
        Profile::Researcher,
        Profile::Everything,
        Profile::BlankSlate,
    ];

    /// `desktop-art`, `explorer`, `researcher`, `everything`, `blank-slate`.
    pub fn name(self) -> &'static str {
        match self {
            Profile::DesktopArt => "desktop-art",
            Profile::Explorer => "explorer",
            Profile::Researcher => "researcher",
            Profile::Everything => "everything",
            Profile::BlankSlate => "blank-slate",
        }
    }

    /// `Desktop art`, ...
    pub fn title(self) -> &'static str {
        match self {
            Profile::DesktopArt => "Desktop art",
            Profile::Explorer => "Explorer",
            Profile::Researcher => "Researcher",
            Profile::Everything => "Everything",
            Profile::BlankSlate => "Blank slate",
        }
    }

    /// A profile by name or title (`explorer`, `Desktop art`, `desktop_art`, `blank`).
    pub fn parse(s: &str) -> Result<Profile, ApiError> {
        let k: String = s
            .trim()
            .to_ascii_lowercase()
            .chars()
            .map(|c| if c == '_' || c == ' ' { '-' } else { c })
            .collect();
        Ok(match k.as_str() {
            "desktop-art" | "desktop" | "art" => Profile::DesktopArt,
            "explorer" | "explore" => Profile::Explorer,
            "researcher" | "research" => Profile::Researcher,
            "everything" | "all" => Profile::Everything,
            "blank-slate" | "blank" | "none" => Profile::BlankSlate,
            _ => {
                return Err(ApiError::invalid(format!("no profile '{s}'"))
                    .hint("profiles: desktop-art, explorer, researcher, everything, blank-slate"));
            }
        })
    }

    /// What it is for, in one line.
    pub fn summary(self) -> &'static str {
        match self {
            Profile::DesktopArt => {
                "Moving pictures for your desktop: the built-in examples, and an offer to show them as your wallpaper and screen saver."
            }
            Profile::Explorer => {
                "The built-in examples and the starter pack's patterns, and a ten-minute search that adds Quick Start entries of its own."
            }
            Profile::Researcher => {
                "For hunting and studying patterns: the starter pack and large packs you choose, in a data folder you choose, and a half-hour search."
            }
            Profile::Everything => {
                "Every pack that came with JETCARP and any you add, a data folder you choose, an hour's search, and the desktop offers."
            }
            Profile::BlankSlate => {
                "Nothing added: the built-in examples stay in the Quick Start, and nothing is imported or built."
            }
        }
    }

    /// It asks where the data goes (big libraries).
    pub fn chooses_data_dir(self) -> bool {
        matches!(self, Profile::Researcher | Profile::Everything)
    }

    /// The bundled packs it imports.
    pub fn packs(self) -> PackChoice {
        match self {
            Profile::Explorer | Profile::Researcher => PackChoice::Starter,
            Profile::Everything => PackChoice::All,
            Profile::DesktopArt | Profile::BlankSlate => PackChoice::None,
        }
    }

    /// It offers to import packs from folders the user picks.
    pub fn chooses_packs(self) -> bool {
        matches!(self, Profile::Researcher | Profile::Everything)
    }

    /// Minutes of its Quick Start build (0: none).
    pub fn quickstart_minutes(self) -> f64 {
        match self {
            Profile::Explorer => 10.0,
            Profile::Researcher => 30.0,
            Profile::Everything => 60.0,
            Profile::DesktopArt | Profile::BlankSlate => 0.0,
        }
    }

    /// Its Quick Start build may be left out (the wizard shows it as a choice).
    pub fn build_optional(self) -> bool {
        self == Profile::Explorer
    }

    /// It offers the live wallpaper and the screen saver.
    pub fn desktop_offers(self) -> bool {
        matches!(self, Profile::DesktopArt | Profile::Everything)
    }
}

// ---------------------------------------------------------------------------------------------
// The record

/// What `setup.json` in the settings folder records.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SetupRecord {
    pub version: u32,
    /// The profile's name (`explorer`).
    pub profile: String,
    /// When it was applied (UTC).
    pub date: String,
    /// The JETCARP that applied it.
    pub jetcarp: String,
    /// The data folder and library then.
    pub data_dir: String,
    pub library: String,
    /// Minutes of its Quick Start build (0: none).
    pub quickstart_minutes: f64,
    /// The packs it imported (their names).
    pub packs: Vec<String>,
    /// The jobs it started, in the library above.
    pub jobs: Vec<i64>,
}

/// The setup recorded in `settings`, if it has run.
pub fn record(settings: &Path) -> Option<SetupRecord> {
    let text = std::fs::read_to_string(settings.join(RECORD_FILE)).ok()?;
    serde_json::from_str(&text).ok()
}

/// Has setup run (in the settings folder of `cx`)? Cheap: one small file.
pub fn has_run(cx: &Ctx) -> bool {
    cx.paths()
        .settings
        .is_some_and(|s| s.join(RECORD_FILE).is_file())
}

fn write_record(settings: &Path, r: &SetupRecord) -> Result<PathBuf, ApiError> {
    std::fs::create_dir_all(settings)
        .map_err(|e| ApiError::io(format!("creating {}: {e}", settings.display())))?;
    let file = settings.join(RECORD_FILE);
    let tmp = settings.join(format!("{RECORD_FILE}.tmp-{}", std::process::id()));
    let body = serde_json::to_string_pretty(r).map_err(|e| ApiError::engine(e.to_string()))?;
    std::fs::write(&tmp, body + "\n")
        .map_err(|e| ApiError::io(format!("writing {}: {e}", tmp.display())))?;
    std::fs::rename(&tmp, &file).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        ApiError::io(format!("writing {}: {e}", file.display()))
    })?;
    Ok(file)
}

// ---------------------------------------------------------------------------------------------
// Bundled packs and pack folders

/// A pack that came with the program (one feed of a store in the bundled folder), or one in a
/// folder the user chose.
#[derive(Clone, Debug, Default, PartialEq, Serialize, JsonSchema)]
pub struct PackEntry {
    /// The store's or export's folder.
    pub path: String,
    /// The feed (none for an export).
    pub feed: Option<String>,
    /// Its newest version (`sha256:...`).
    pub manifest: String,
    pub title: String,
    pub description: String,
    pub version_label: String,
    /// The folder's size on disk.
    pub bytes: u64,
    /// Rows of its tables.
    pub patterns: u64,
    pub quickstart: u64,
    pub ants: u64,
    /// The explore searches it carries.
    pub searches: usize,
    /// It is the starter pack.
    pub starter: bool,
    /// This version is in the library already.
    pub imported: bool,
    /// It could not be read (it is never imported).
    pub error: Option<String>,
}

impl PackEntry {
    /// How it is named in a list: its title, else its feed, else its folder's name.
    pub fn name(&self) -> String {
        if !self.title.is_empty() {
            return self.title.clone();
        }
        if let Some(f) = &self.feed {
            return f.clone();
        }
        Path::new(&self.path)
            .file_name()
            .map(|f| f.to_string_lossy().to_string())
            .unwrap_or_else(|| self.path.clone())
    }

    /// `store-folder/feed`: how a bundled pack is named in `setup.apply`'s `bundled`.
    pub fn id(&self) -> String {
        let folder = Path::new(&self.path)
            .file_name()
            .map(|f| f.to_string_lossy().to_string())
            .unwrap_or_default();
        match &self.feed {
            Some(f) => format!("{folder}/{f}"),
            None => folder,
        }
    }

    /// What is in it, in words (`143 patterns, 40 Quick Start entries, 12.3 MB`).
    pub fn contents(&self) -> String {
        let mut parts = vec![];
        if self.patterns > 0 {
            parts.push(count(self.patterns, "pattern", "patterns"));
        }
        if self.quickstart > 0 {
            parts.push(count(
                self.quickstart,
                "Quick Start entry",
                "Quick Start entries",
            ));
        }
        if self.ants > 0 {
            parts.push(count(self.ants, "ant run", "ant runs"));
        }
        if self.searches > 0 {
            parts.push(format!(
                "{} search{}",
                self.searches,
                if self.searches == 1 { "" } else { "es" }
            ));
        }
        parts.push(human_bytes(self.bytes));
        parts.join(", ")
    }

    fn spec(&self) -> PackImportSpec {
        PackImportSpec {
            path: self.path.clone(),
            feed: self.feed.clone(),
            manifest: Some(self.manifest.clone()).filter(|m| !m.is_empty()),
            title: self.name(),
            ..Default::default()
        }
    }
}

/// `1 pattern`, `1,204 patterns`.
pub fn count(n: u64, one: &str, many: &str) -> String {
    format!(
        "{} {}",
        jetcarp_search::dashboard::digits(n),
        if n == 1 { one } else { many }
    )
}

fn is_pack(p: &Path) -> bool {
    p.join(jetcarp_pack::store::MARKER).is_file() || p.join("pack.json").is_file()
}

/// The packs at `path`: a store (each feed), an export, or a folder of them (one level down).
pub fn packs_in(path: &Path, lib: Option<&Library>) -> Vec<PackEntry> {
    let imported: std::collections::HashSet<String> = lib
        .and_then(|l| l.pack_imports().ok())
        .unwrap_or_default()
        .into_iter()
        .map(|r| r.manifest)
        .collect();
    let mut folders: Vec<PathBuf> = if is_pack(path) {
        vec![path.to_path_buf()]
    } else {
        std::fs::read_dir(path)
            .map(|rd| {
                rd.flatten()
                    .map(|e| e.path())
                    .filter(|p| p.is_dir() && is_pack(p))
                    .collect()
            })
            .unwrap_or_default()
    };
    folders.sort();
    let mut out = vec![];
    for f in folders {
        let bytes = folder_size(&f);
        let base = PackEntry {
            path: crate::out::absolute(&f).display().to_string(),
            bytes,
            ..Default::default()
        };
        let src = match Source::open(&f) {
            Ok(s) => s,
            Err(e) => {
                out.push(PackEntry {
                    error: Some(e),
                    ..base
                });
                continue;
            }
        };
        let feeds: Vec<Option<String>> = match &src {
            Source::Store(_) => src.feeds().into_iter().map(Some).collect(),
            Source::Export(_) => vec![None],
        };
        if feeds.is_empty() {
            out.push(PackEntry {
                error: Some("the store has no feeds".into()),
                ..base
            });
            continue;
        }
        for feed in feeds {
            let mut e = PackEntry {
                feed: feed.clone(),
                ..base.clone()
            };
            let read = src
                .default_manifest(feed.as_deref())
                .and_then(|h| src.version(&h));
            match read {
                Ok(v) => {
                    let rows = |t: &str| {
                        v.tables
                            .get(t)
                            .map(|tp| tp.parts.iter().map(|(p, _)| p.rows).sum())
                            .unwrap_or(0)
                    };
                    e.manifest = v.hash.clone();
                    e.title = v.manifest.title.clone();
                    e.description = v.manifest.description.clone();
                    e.version_label = v.manifest.version_label.clone();
                    e.patterns = rows("patterns");
                    e.quickstart = rows("quickstart");
                    e.ants = rows("ants");
                    e.searches = v.manifest.searches.len();
                    e.imported = imported.contains(&v.hash);
                }
                Err(err) => e.error = Some(err),
            }
            out.push(e);
        }
    }
    out
}

/// The packs that came with the program, the starter pack marked.
pub fn bundled_packs(dir: Option<&Path>, lib: Option<&Library>) -> Vec<PackEntry> {
    let Some(dir) = dir else {
        return vec![];
    };
    let mut v = packs_in(dir, lib);
    let named = |e: &PackEntry| {
        let folder = Path::new(&e.path)
            .file_name()
            .map(|f| f.to_string_lossy().to_ascii_lowercase())
            .unwrap_or_default();
        let feed = e.feed.as_deref().unwrap_or("").to_ascii_lowercase();
        folder.contains("starter") || feed.contains("starter")
    };
    let any = v.iter().any(named);
    for e in &mut v {
        e.starter = e.error.is_none() && (!any || named(e));
    }
    v
}

// ---------------------------------------------------------------------------------------------
// Free space

/// Free space for files on the disk that holds `path` (or its nearest folder that exists), in
/// bytes; `None` where it cannot be read.
pub fn free_space(path: &Path) -> Option<u64> {
    let mut p = crate::out::absolute(path);
    while !p.exists() {
        p = p.parent()?.to_path_buf();
    }
    free_space_at(&p)
}

#[cfg(unix)]
fn free_space_at(p: &Path) -> Option<u64> {
    use std::os::unix::ffi::OsStrExt;
    let c = std::ffi::CString::new(p.as_os_str().as_bytes()).ok()?;
    let mut s: libc::statvfs = unsafe { std::mem::zeroed() };
    // SAFETY: `c` is a NUL-terminated path and `s` a statvfs the call fills in.
    if unsafe { libc::statvfs(c.as_ptr(), &mut s) } != 0 {
        return None;
    }
    #[allow(clippy::unnecessary_cast)]
    Some(s.f_bavail as u64 * s.f_frsize as u64)
}

#[cfg(windows)]
fn free_space_at(p: &Path) -> Option<u64> {
    use std::os::windows::ffi::OsStrExt;
    let wide: Vec<u16> = p.as_os_str().encode_wide().chain([0]).collect();
    let mut free: u64 = 0;
    // SAFETY: `wide` is a NUL-terminated path; the call writes the free bytes for this user.
    let ok = unsafe {
        windows_sys::Win32::Storage::FileSystem::GetDiskFreeSpaceExW(
            wide.as_ptr(),
            &mut free,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    };
    (ok != 0).then_some(free)
}

#[cfg(not(any(unix, windows)))]
fn free_space_at(_: &Path) -> Option<u64> {
    None
}

// ---------------------------------------------------------------------------------------------
// The data folder

/// What a folder holds, for choosing it as the data folder.
fn check_data_dir(dir: &Path) -> Result<bool, ApiError> {
    if !dir.exists() {
        return Ok(false);
    }
    if !dir.is_dir() {
        return Err(ApiError::invalid(format!(
            "{} is a file, not a folder",
            dir.display()
        )));
    }
    if dir.join(jetcarp_store::library::FILE).is_file() {
        return Ok(true);
    }
    let rd = std::fs::read_dir(dir)
        .map_err(|e| ApiError::io(format!("reading {}: {e}", dir.display())))?;
    // What a system leaves in a new folder does not count.
    let noise = |n: &str| n.starts_with('.') || n == "Thumbs.db" || n == "desktop.ini";
    let mut others: Vec<String> = rd
        .flatten()
        .map(|e| e.file_name().to_string_lossy().to_string())
        .filter(|n| !noise(n))
        .collect();
    if others.is_empty() {
        return Ok(false);
    }
    others.sort();
    let shown: Vec<String> = others.iter().take(3).cloned().collect();
    Err(ApiError::conflict(format!(
        "{} is not empty ({}{}) and holds no JETCARP library (library.sqlite)",
        dir.display(),
        shown.join(", "),
        if others.len() > 3 { ", ..." } else { "" }
    ))
    .hint(
        "choose an empty folder, or one that already holds a JETCARP data folder. Setup never moves a library: to move yours, quit JETCARP, move the data folder's contents yourself, then choose the new folder",
    ))
}

/// `setup.set_data_dir`'s input.
#[derive(Clone, Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DataDirIn {
    /// The folder (made if it does not exist). Empty or missing: back to the default data folder.
    #[serde(default)]
    pub path: Option<String>,
    /// Only check the folder; record nothing.
    #[serde(default)]
    pub dry_run: bool,
}

/// The data folder chosen.
#[derive(Clone, Debug, Serialize, JsonSchema)]
pub struct DataDirOut {
    /// The data folder from now on (unless `JETCARP_DATA_DIR` is set).
    pub data: String,
    /// The one before.
    pub previous: String,
    /// It already holds a JETCARP library, which JETCARP uses from now on.
    pub has_library: bool,
    /// The file the choice is recorded in (`<settings>/paths.json`); none when it went back to
    /// the default.
    pub file: Option<String>,
    /// `JETCARP_DATA_DIR` is not set, so the choice takes effect now.
    pub effective: bool,
    pub dry_run: bool,
    pub notes: Vec<String>,
}

impl Report for DataDirOut {
    fn text(&self) -> String {
        let mut s = if self.dry_run {
            format!("the data folder would be {}\n", self.data)
        } else if self.data == self.previous {
            format!("the data folder is {} (unchanged)\n", self.data)
        } else {
            format!(
                "the data folder is now {} (was {})\n",
                self.data, self.previous
            )
        };
        if let Some(f) = &self.file {
            let _ = writeln!(s, "  recorded in {f}");
        }
        if self.has_library {
            let _ = writeln!(s, "  it holds a JETCARP library, which is used from now on");
        }
        s
    }
    fn notes(&self) -> String {
        self.notes.iter().map(|n| format!("{n}\n")).collect()
    }
}

fn set_data_dir(cx: &Ctx, a: &DataDirIn) -> Result<DataDirOut, ApiError> {
    let paths = cx.paths();
    let settings = paths.settings.clone().ok_or_else(|| {
        ApiError::io("this system has no settings folder, so a data folder cannot be recorded")
            .hint("set JETCARP_DATA_DIR instead")
    })?;
    let (previous, _) = cpaths::data_dir_source_from(None, Some(&settings));
    let chosen = a
        .path
        .as_deref()
        .map(str::trim)
        .filter(|p| !p.is_empty() && *p != "default");
    let target = match chosen {
        Some(p) => Some(crate::out::absolute(&cx.fs.readable(p)?)),
        None => None,
    }
    // The usual folder chosen by name is no choice at all: paths.json goes.
    .filter(|t| *t != cpaths::default_data_dir());
    let data = target.clone().unwrap_or_else(cpaths::default_data_dir);
    let has_library = check_data_dir(&data)?;
    let mut notes = vec![];
    let env = std::env::var_os("JETCARP_DATA_DIR").filter(|v| !v.is_empty());
    if env.is_some() {
        notes.push(
            "JETCARP_DATA_DIR is set and wins over this choice while it is; the choice takes effect once it is not set".to_string(),
        );
    }
    if std::env::var_os("JETCARP_LIBRARY").is_some_and(|v| !v.is_empty()) || cx.library.is_some() {
        notes.push(format!(
            "the library stays at {}, which JETCARP_LIBRARY (or --library) names",
            cx.library_path_now().display()
        ));
    }
    let old_lib = previous.join(jetcarp_store::library::FILE);
    if previous != data && old_lib.is_file() && !has_library {
        notes.push(format!(
            "your library in {} stays there; the new folder starts a new, empty library",
            previous.display()
        ));
    }
    let mut file = None;
    if !a.dry_run {
        if target.is_some() {
            std::fs::create_dir_all(&data)
                .map_err(|e| ApiError::io(format!("creating {}: {e}", data.display())))?;
        }
        cpaths::set_chosen_data_dir(&settings, target.as_deref()).map_err(ApiError::io)?;
        file = target
            .is_some()
            .then(|| settings.join(cpaths::PATHS_FILE).display().to_string());
    }
    Ok(DataDirOut {
        data: data.display().to_string(),
        previous: previous.display().to_string(),
        has_library,
        file,
        effective: env.is_none(),
        dry_run: a.dry_run,
        notes,
    })
}

/// `setup.set_data_dir`.
pub struct SetupSetDataDir;
impl Op for SetupSetDataDir {
    const NAME: &'static str = "setup.set_data_dir";
    const SUMMARY: &'static str = "Choose another data folder (for big libraries), recorded in paths.json in the settings folder, or go back to the default. A folder that is not empty is refused unless it holds a JETCARP library: nothing is moved.";
    const READ_ONLY: bool = false;
    type In = DataDirIn;
    type Out = DataDirOut;
    fn run(cx: &Ctx, a: DataDirIn, _: &dyn Progress) -> Result<DataDirOut, ApiError> {
        set_data_dir(cx, &a)
    }
}

// ---------------------------------------------------------------------------------------------
// setup.status

/// Whether a GPU can run the Quick Start build.
#[derive(Clone, Debug, Default, PartialEq, Serialize, JsonSchema)]
pub struct GpuStatus {
    pub usable: bool,
    /// The adapter's name.
    pub name: String,
    /// Why there is none, when there is none.
    pub note: String,
}

fn gpu_status(cx: &Ctx) -> GpuStatus {
    let adapters = cx.platform.gpu_adapters();
    if let Some(a) = adapters.iter().find(|a| a.chosen && a.problem.is_none()) {
        return GpuStatus {
            usable: true,
            name: a.name.clone(),
            note: String::new(),
        };
    }
    if let Some(a) = adapters.iter().find(|a| a.problem.is_some()) {
        return GpuStatus {
            usable: false,
            name: a.name.clone(),
            note: a.problem.clone().unwrap_or_default(),
        };
    }
    match cx.platform.explore_backend() {
        Ok(b) => GpuStatus {
            usable: true,
            name: b.name(),
            note: String::new(),
        },
        Err(e) => GpuStatus {
            usable: false,
            name: String::new(),
            note: e,
        },
    }
}

/// The library's size, by kind of record.
#[derive(Clone, Debug, Default, PartialEq, Serialize, JsonSchema)]
pub struct LibraryCounts {
    pub patterns: u64,
    pub ants: u64,
    pub jobs: u64,
    /// Quick Start entries by source (`builtin`, `pack`, `atlas`, `promotion`).
    pub quickstart: BTreeMap<String, u64>,
}

impl LibraryCounts {
    fn of(lib: &Library) -> LibraryCounts {
        let i = lib.info().unwrap_or_default();
        LibraryCounts {
            patterns: i.patterns.max(0) as u64,
            ants: i.ants.max(0) as u64,
            jobs: i.jobs.max(0) as u64,
            quickstart: lib.quickstart_counts().unwrap_or_default(),
        }
    }

    /// `1,234 patterns, 5 ant runs; Quick Start 120 built in, 40 from packs, 300 atlas, 2 yours`.
    pub fn text(&self) -> String {
        let d = jetcarp_search::dashboard::digits;
        let mut s = format!(
            "{} patterns, {} ant runs, {} jobs",
            d(self.patterns),
            d(self.ants),
            d(self.jobs)
        );
        let qs: Vec<String> = [
            ("builtin", "built in"),
            ("pack", "from packs"),
            ("atlas", "from searches"),
            ("promotion", "your own"),
        ]
        .iter()
        .filter_map(|(k, w)| self.quickstart.get(*k).map(|n| format!("{} {w}", d(*n))))
        .collect();
        if !qs.is_empty() {
            let _ = write!(s, "; Quick Start entries: {}", qs.join(", "));
        }
        s
    }
}

/// A job setup started, as it is now.
#[derive(Clone, Debug, Default, PartialEq, Serialize, JsonSchema)]
pub struct SetupJob {
    pub job: i64,
    /// `pack_import` or `explore` (a Quick Start build).
    pub kind: String,
    pub label: String,
    /// `running`, `paused`, `done`, `failed` (live when it runs here).
    pub status: String,
    /// It runs in this process.
    pub here: bool,
    /// Running, but its process stopped saying so: it was cut off (a crash, or a quit), and
    /// resumes.
    pub stale: bool,
    /// The fraction done, when it is known (a Quick Start build's minutes, an import's end).
    pub fraction: Option<f64>,
    /// Units done (a build's runs screened; an import's 1 once it is done).
    pub done: u64,
    pub line: String,
}

impl SetupJob {
    /// It has nothing more to do (done), or cannot go on without being resumed.
    pub fn settled(&self) -> bool {
        !(self.status == "running" && !self.stale)
    }
}

/// A job as setup shows it.
pub fn setup_job(cx: &Ctx, lib: &Library, job: i64) -> Option<SetupJob> {
    use jetcarp_search::dashboard;
    let row = lib.job(job).ok()?;
    let k = jobs::kind(&row.kind);
    let here = cx.registry().is_running(lib, job);
    let progress = cx
        .registry()
        .progress(lib, job)
        .filter(|p| !p.is_null())
        .unwrap_or_else(|| k.saved_progress(lib, &row));
    let mut r = row.clone();
    r.progress = progress.clone();
    let sum = dashboard::summaries(std::slice::from_ref(&r), dashboard::now_seconds()).pop();
    let status = match (here, progress.get("status").and_then(|v| v.as_str())) {
        (true, Some(st)) => st.to_string(),
        (true, None) => "running".into(),
        (false, _) => jobs::status_name(row.status).to_string(),
    };
    let elapsed = progress
        .get("elapsed_s")
        .and_then(|v| v.as_f64())
        .unwrap_or(0.0);
    let fraction = if status == "done" {
        Some(1.0)
    } else {
        progress
            .get("build_minutes")
            .and_then(|v| v.as_f64())
            .filter(|m| *m > 0.0)
            .map(|m| (elapsed / (m * 60.0)).clamp(0.0, 1.0))
    };
    Some(SetupJob {
        job,
        kind: row.kind.clone(),
        label: row.label().unwrap_or_default().to_string(),
        status,
        here,
        stale: !here && sum.as_ref().is_some_and(|s| s.stale),
        done: sum.as_ref().map(|s| s.done).unwrap_or(0),
        fraction,
        line: super::search::line_of(k, &row, &progress),
    })
}

/// A profile as people choose it.
#[derive(Clone, Debug, Serialize, JsonSchema)]
pub struct ProfileInfo {
    /// `explorer`.
    pub name: String,
    /// `Explorer`.
    pub title: String,
    /// What it is for, in one line.
    pub summary: String,
    /// What it takes, in words (`about 40 MB and 11 minutes`).
    pub cost: String,
    /// About how much it adds to the data folder, and how long it takes, in minutes.
    pub bytes: u64,
    pub minutes: f64,
    /// It asks where the data goes.
    pub chooses_data_dir: bool,
    /// The bundled packs it imports.
    pub packs: PackChoice,
    /// It offers to import packs from folders.
    pub chooses_packs: bool,
    /// Minutes of its Quick Start build (0: none), and whether the build may be left out.
    pub quickstart_minutes: f64,
    pub build_optional: bool,
    /// It offers the live wallpaper and the screen saver.
    pub desktop_offers: bool,
}

/// The bundled packs a profile imports, of those there are.
pub fn packs_for(p: Profile, bundled: &[PackEntry]) -> Vec<&PackEntry> {
    bundled
        .iter()
        .filter(|e| e.error.is_none())
        .filter(|e| match p.packs() {
            PackChoice::None => false,
            PackChoice::Starter => e.starter,
            PackChoice::All => true,
        })
        .collect()
}

/// About how much a setup adds and takes: packs of `pack_bytes` and a build of `minutes`.
pub fn estimate(pack_bytes: u64, minutes: f64) -> (u64, f64) {
    let bytes = (pack_bytes as f64 * IMPORT_GROWTH) as u64
        + (minutes.max(0.0) * BUILD_BYTES_PER_MINUTE as f64) as u64;
    // An import reads about 50 MB a minute, and checks a sample of its patterns again.
    let import_min = if pack_bytes > 0 {
        (pack_bytes as f64 / 50e6).max(0.5) + 0.5
    } else {
        0.0
    };
    (bytes, minutes.max(0.0) + import_min)
}

/// Bytes to two significant figures, for estimates (`80 MB`, `1.2 GB`).
pub fn rough_bytes(b: u64) -> String {
    let units = ["B", "KB", "MB", "GB", "TB"];
    let mut v = b as f64;
    let mut i = 0;
    while v >= 1000.0 && i + 1 < units.len() {
        v /= 1000.0;
        i += 1;
    }
    if v >= 10.0 {
        let step = if v >= 100.0 { 10.0 } else { 1.0 };
        format!("{:.0} {}", (v / step).round() * step, units[i])
    } else {
        format!("{:.1} {}", v, units[i])
    }
}

/// `about 40 MB and 11 minutes`.
pub fn cost_text(bytes: u64, minutes: f64, chosen: bool) -> String {
    let plus = if chosen {
        ", plus the packs you add"
    } else {
        ""
    };
    if bytes == 0 && minutes <= 0.0 {
        return if chosen {
            "only the packs you add".into()
        } else {
            "no extra disk space or time".into()
        };
    }
    let m = minutes.round().max(1.0) as u64;
    format!(
        "about {} and {m} minute{}{plus}",
        rough_bytes(bytes.max(1_000_000)),
        if m == 1 { "" } else { "s" }
    )
}

fn profile_info(p: Profile, bundled: &[PackEntry]) -> ProfileInfo {
    let pack_bytes: u64 = packs_for(p, bundled)
        .iter()
        .filter(|e| !e.imported)
        .map(|e| e.bytes)
        .sum();
    let (bytes, minutes) = estimate(pack_bytes, p.quickstart_minutes());
    ProfileInfo {
        name: p.name().into(),
        title: p.title().into(),
        summary: p.summary().into(),
        cost: cost_text(bytes, minutes, p.chooses_packs()),
        bytes,
        minutes,
        chooses_data_dir: p.chooses_data_dir(),
        packs: p.packs(),
        chooses_packs: p.chooses_packs(),
        quickstart_minutes: p.quickstart_minutes(),
        build_optional: p.build_optional(),
        desktop_offers: p.desktop_offers(),
    }
}

/// Everything a front end shows before setup, and after it.
#[derive(Clone, Debug, Serialize, JsonSchema)]
pub struct SetupStatus {
    /// Setup has run.
    pub done: bool,
    /// What it recorded, when it has.
    pub record: Option<SetupRecord>,
    /// The folders.
    pub folders: Paths,
    /// Where the data folder comes from: `default`, `paths.json` (chosen in setup), or
    /// `JETCARP_DATA_DIR`.
    pub data_source: String,
    /// The library file.
    pub library: String,
    /// Free space on the data folder's disk, in bytes.
    pub free_bytes: Option<u64>,
    /// The packs that came with the program (`bundled:` in `jetcarp paths`).
    pub bundled: Vec<PackEntry>,
    /// A GPU for the Quick Start build.
    pub gpu: GpuStatus,
    /// The library's size.
    pub counts: LibraryCounts,
    /// The jobs setup started, as they are now.
    pub jobs: Vec<SetupJob>,
    /// The profiles, with what each takes here.
    pub profiles: Vec<ProfileInfo>,
}

impl SetupStatus {
    /// The profile recorded, if setup has run.
    pub fn profile(&self) -> Option<Profile> {
        Profile::parse(&self.record.as_ref()?.profile).ok()
    }
    /// A profile's line.
    pub fn info(&self, p: Profile) -> Option<&ProfileInfo> {
        self.profiles.iter().find(|i| i.name == p.name())
    }
}

impl Report for SetupStatus {
    fn text(&self) -> String {
        let mut s = match (&self.record, self.profile()) {
            (Some(r), p) => format!(
                "setup: {} on {} (JETCARP {})\n",
                p.map(Profile::title).unwrap_or(r.profile.as_str()),
                r.date,
                r.jetcarp
            ),
            (None, _) => {
                "setup: not run yet (`jetcarp setup`, or the app's setup window, runs it)\n".into()
            }
        };
        let show = |p: &Option<PathBuf>| {
            p.as_ref()
                .map(|p| p.display().to_string())
                .unwrap_or_else(|| "(none on this system)".into())
        };
        let from = match self.data_source.as_str() {
            "paths.json" => " (chosen in setup: paths.json)",
            "JETCARP_DATA_DIR" => " (from JETCARP_DATA_DIR)",
            _ => "",
        };
        let free = self
            .free_bytes
            .map(|b| format!(", {} free", human_bytes(b)))
            .unwrap_or_default();
        let _ = writeln!(s, "  settings: {}", show(&self.folders.settings));
        let _ = writeln!(s, "  data:     {}{from}{free}", self.folders.data.display());
        let _ = writeln!(s, "  library:  {}", self.library);
        let _ = writeln!(s, "  output:   {}", self.folders.output.display());
        match &self.folders.bundled_packs {
            Some(d) => {
                let _ = writeln!(s, "  bundled:  {}", d.display());
            }
            None => s.push_str("  bundled:  (no packs came with the program)\n"),
        }
        if !self.bundled.is_empty() {
            s.push_str("packs that came with JETCARP:\n");
            for e in &self.bundled {
                match &e.error {
                    Some(err) => {
                        let _ = writeln!(s, "  {}: cannot be read: {err}", e.path);
                    }
                    None => {
                        let mut tags = vec![];
                        if e.starter {
                            tags.push("the starter pack");
                        }
                        if e.imported {
                            tags.push("imported");
                        }
                        let _ = writeln!(
                            s,
                            "  {} ({}): {}{}",
                            e.name(),
                            e.id(),
                            e.contents(),
                            if tags.is_empty() {
                                String::new()
                            } else {
                                format!("; {}", tags.join(", "))
                            }
                        );
                    }
                }
            }
        }
        let _ = writeln!(
            s,
            "GPU: {}",
            if self.gpu.usable {
                format!("{} (a Quick Start build runs on it)", self.gpu.name)
            } else if self.gpu.note.is_empty() {
                "none: a Quick Start build runs on the CPU".into()
            } else {
                format!(
                    "none usable ({}): a Quick Start build runs on the CPU",
                    self.gpu.note
                )
            }
        );
        let _ = writeln!(s, "library: {}", self.counts.text());
        if !self.jobs.is_empty() {
            s.push_str("setup's jobs:\n");
            for j in &self.jobs {
                let _ = writeln!(s, "  {} ({})", j.line, j.status);
            }
        }
        s.push_str("profiles:\n");
        for p in &self.profiles {
            let _ = writeln!(
                s,
                "  {:<12} {}: {} ({})",
                p.name, p.title, p.summary, p.cost
            );
        }
        s
    }
}

/// The status of setup in `cx`'s folders.
pub fn status(cx: &Ctx) -> SetupStatus {
    let paths = cx.paths();
    let rec = paths.settings.as_deref().and_then(record);
    let (_, source) = cpaths::data_dir_source(paths.settings.as_deref());
    let lib_path = cx.library_path_now();
    let lib = Library::open(&lib_path);
    let bundled = bundled_packs(paths.bundled_packs.as_deref(), Some(&lib));
    let jobs = match &rec {
        Some(r) if Path::new(&r.library) == lib_path => r
            .jobs
            .iter()
            .filter_map(|j| setup_job(cx, &lib, *j))
            .collect(),
        _ => vec![],
    };
    SetupStatus {
        done: rec.is_some(),
        record: rec,
        data_source: source.name().into(),
        library: lib_path.display().to_string(),
        free_bytes: free_space(&paths.data),
        gpu: gpu_status(cx),
        counts: LibraryCounts::of(&lib),
        jobs,
        profiles: Profile::ALL
            .iter()
            .map(|p| profile_info(*p, &bundled))
            .collect(),
        bundled,
        folders: paths,
    }
}

/// `setup.status`.
pub struct SetupStatusOp;
impl Op for SetupStatusOp {
    const NAME: &'static str = "setup.status";
    const SUMMARY: &'static str = "Whether first-run setup has run and with which profile; the folders (with free space); the packs that came with the program; whether a GPU can run a Quick Start build; the library's counts; setup's jobs; and the profiles with what each takes.";
    const READ_ONLY: bool = true;
    type In = NoInput;
    type Out = SetupStatus;
    fn run(cx: &Ctx, _: NoInput, _: &dyn Progress) -> Result<SetupStatus, ApiError> {
        Ok(status(cx))
    }
}

// ---------------------------------------------------------------------------------------------
// setup.apply

fn default_true() -> bool {
    true
}

/// `setup.apply`'s input.
#[derive(Clone, Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ApplyIn {
    /// desktop-art, explorer, researcher, everything, or blank-slate.
    pub profile: String,
    /// Choose this data folder first (`setup.set_data_dir`).
    #[serde(default)]
    pub data_dir: Option<String>,
    /// Minutes of the Quick Start build (default: the profile's; 0: none).
    #[serde(default)]
    pub quickstart_minutes: Option<f64>,
    /// Folders of packs to import too: a pack store, an export, or a folder holding them.
    #[serde(default)]
    pub packs: Vec<String>,
    /// Which bundled packs to import, by id (`store-folder/feed`, as setup.status lists them);
    /// default: the profile's (the starter pack, every one, or none).
    #[serde(default)]
    pub bundled: Option<Vec<String>>,
    /// The Quick Start build uses the GPU when there is one (default true).
    #[serde(default = "default_true")]
    pub gpu: bool,
    /// The Quick Start build's search settings (default: every tiling and rule family).
    #[serde(default)]
    pub search: Option<ExploreIn>,
    /// How much of each pack is verified again on import: sample (default), full, none.
    #[serde(default)]
    pub verify: Option<String>,
    /// Worker threads (default 0: all cores).
    #[serde(default)]
    pub threads: Option<usize>,
    /// Start the jobs on threads of this process and answer at once (else they run here, one
    /// after another).
    #[serde(default)]
    pub background: bool,
    /// Say what would be done; do nothing.
    #[serde(default)]
    pub dry_run: bool,
}

/// One thing setup does.
#[derive(Clone, Debug, Serialize, JsonSchema)]
pub struct PlanStep {
    /// `data_dir`, `record`, `pack_import`, `quickstart_build`, `desktop`, `skip`.
    pub kind: String,
    /// What it does, in words.
    pub what: String,
    /// Its job, once made.
    pub job: Option<i64>,
}

/// What setup did (or, with `dry_run`, would do).
#[derive(Clone, Debug, Serialize, JsonSchema)]
pub struct Applied {
    pub profile: String,
    pub title: String,
    pub dry_run: bool,
    /// The data folder and the library.
    pub data: String,
    pub library: String,
    /// The record written (`<settings>/setup.json`).
    pub record: Option<String>,
    /// What it does, in order.
    pub steps: Vec<PlanStep>,
    /// The jobs it started, as they are now (done, unless they run in the background).
    pub jobs: Vec<SetupJob>,
    /// The front end should offer the live wallpaper and the screen saver.
    pub desktop_offers: bool,
    pub notes: Vec<String>,
}

impl Report for Applied {
    fn text(&self) -> String {
        let mut s = format!(
            "{} {}:\n",
            if self.dry_run {
                "setting up JETCARP as"
            } else {
                "set up JETCARP as"
            },
            self.title
        );
        for (i, st) in self.steps.iter().enumerate() {
            let _ = writeln!(s, "  {}. {}", i + 1, st.what);
        }
        if !self.jobs.is_empty() && !self.dry_run {
            for j in &self.jobs {
                let _ = writeln!(s, "{} ({})", j.line, j.status);
            }
        }
        s
    }
    fn notes(&self) -> String {
        self.notes.iter().map(|n| format!("{n}\n")).collect()
    }
}

/// A job to make: its kind's spec, label, and what it does in words.
struct Planned {
    kind: &'static str,
    step: &'static str,
    what: String,
    spec: JobSpec,
    pack: Option<String>,
}

fn plan_build(minutes: f64, a: &ApplyIn, gpu: &GpuStatus) -> Result<Option<Planned>, ApiError> {
    if minutes <= 0.0 {
        return Ok(None);
    }
    if !minutes.is_finite() {
        return Err(ApiError::invalid("quickstart_minutes must be a number"));
    }
    let input = super::quickstart::BuildIn {
        minutes: Some(minutes),
        search: a.search.clone().unwrap_or_default(),
        gpu: Some(a.gpu),
        ..Default::default()
    };
    let spec = input.job_spec()?;
    let on = if a.gpu && gpu.usable {
        format!("on the GPU ({})", gpu.name)
    } else {
        "on the CPU".into()
    };
    Ok(Some(Planned {
        kind: "explore",
        step: "quickstart_build",
        what: format!(
            "search for {} {on}, then add its best runs to the Quick Start",
            minutes_text(minutes)
        ),
        spec,
        pack: None,
    }))
}

/// `10 minutes`, `30 seconds`.
pub fn minutes_text(m: f64) -> String {
    if m < 1.0 {
        let s = (m * 60.0).round() as u64;
        return format!("{s} second{}", if s == 1 { "" } else { "s" });
    }
    let r = (m * 10.0).round() / 10.0;
    if r.fract() == 0.0 {
        format!("{r:.0} minute{}", if r == 1.0 { "" } else { "s" })
    } else {
        format!("{r:.1} minutes")
    }
}

fn plan_import(e: &PackEntry, verify: &Option<String>) -> Planned {
    let mut spec = e.spec();
    spec.verify = verify.clone();
    Planned {
        kind: PACK_IMPORT,
        step: "pack_import",
        what: format!(
            "import {} ({}{})",
            e.name(),
            e.contents(),
            if e.starter && !e.name().to_ascii_lowercase().contains("starter") {
                ", the starter pack"
            } else {
                ""
            }
        ),
        spec: JobSpec {
            strategy: PACK_IMPORT.into(),
            spec: serde_json::to_value(&spec).unwrap_or_default(),
            label: Some(format!("setup: {}", e.name())),
            budget: Default::default(),
        },
        pack: Some(e.name()),
    }
}

fn apply(cx: &Ctx, a: ApplyIn, p: &dyn Progress) -> Result<Applied, ApiError> {
    let profile = Profile::parse(&a.profile)?;
    let mut steps: Vec<PlanStep> = vec![];
    let mut notes: Vec<String> = vec![];
    // The data folder first: the library and the jobs go there.
    if let Some(d) = a.data_dir.as_deref().filter(|d| !d.trim().is_empty()) {
        let out = set_data_dir(
            cx,
            &DataDirIn {
                path: Some(d.to_string()),
                dry_run: a.dry_run,
            },
        )?;
        steps.push(PlanStep {
            kind: "data_dir".into(),
            what: format!(
                "keep JETCARP's data in {}{}",
                out.data,
                if out.has_library {
                    " (the JETCARP library there is used)"
                } else {
                    ""
                }
            ),
            job: None,
        });
        notes.extend(out.notes);
    }
    let paths = cx.paths();
    let lib_path = cx.library_path_now();
    let lib = Library::open(&lib_path);
    // The packs: the profile's bundled ones (or those named), then those in the folders given.
    let bundled = bundled_packs(paths.bundled_packs.as_deref(), Some(&lib));
    let mut chosen: Vec<PackEntry> = match &a.bundled {
        Some(ids) => {
            let mut v = vec![];
            for id in ids {
                let e = bundled
                    .iter()
                    .find(|e| e.id() == *id || e.feed.as_deref() == Some(id.as_str()))
                    .ok_or_else(|| {
                        ApiError::not_found(format!("no bundled pack '{id}'")).hint(format!(
                            "bundled packs: {}",
                            bundled
                                .iter()
                                .map(PackEntry::id)
                                .collect::<Vec<_>>()
                                .join(", ")
                        ))
                    })?;
                v.push(e.clone());
            }
            v
        }
        None => packs_for(profile, &bundled).into_iter().cloned().collect(),
    };
    if a.bundled.is_none() && profile.packs() != PackChoice::None && chosen.is_empty() {
        notes.push(match &paths.bundled_packs {
            Some(d) => format!("no packs came with the program in {}", d.display()),
            None => "no packs came with the program (as after `cargo install`), so none are imported; `jetcarp pack import` imports one".into(),
        });
    }
    for dir in &a.packs {
        let path = cx.fs.readable(dir)?;
        if !path.exists() {
            return Err(ApiError::not_found(format!(
                "{} does not exist",
                path.display()
            )));
        }
        let found = packs_in(&path, Some(&lib));
        if found.is_empty() {
            return Err(ApiError::not_found(format!(
                "{} holds no packs (a pack store has jetcarp-store.json, an export pack.json)",
                path.display()
            )));
        }
        for e in found {
            if let Some(err) = &e.error {
                return Err(ApiError::invalid(format!("{}: {err}", e.path)));
            }
            chosen.push(e);
        }
    }
    let mut planned: Vec<Planned> = vec![];
    let mut seen = std::collections::HashSet::new();
    for e in &chosen {
        if !seen.insert((e.path.clone(), e.feed.clone())) {
            continue;
        }
        if e.imported {
            steps.push(PlanStep {
                kind: "skip".into(),
                what: format!("{} is in the library already", e.name()),
                job: None,
            });
            continue;
        }
        planned.push(plan_import(e, &a.verify));
    }
    let minutes = a
        .quickstart_minutes
        .unwrap_or_else(|| profile.quickstart_minutes());
    if minutes < 0.0 {
        return Err(ApiError::invalid("quickstart_minutes must be 0 or more"));
    }
    let gpu = if minutes > 0.0 {
        gpu_status(cx)
    } else {
        GpuStatus::default()
    };
    if let Some(b) = plan_build(minutes, &a, &gpu)? {
        planned.push(b);
    }
    let settings = paths.settings.clone();
    steps.push(PlanStep {
        kind: "record".into(),
        what: match &settings {
            Some(s) => format!(
                "record the profile {} in {}",
                profile.title(),
                s.join(RECORD_FILE).display()
            ),
            None => "(this system has no settings folder, so the choice is not recorded)".into(),
        },
        job: None,
    });
    for pl in &planned {
        steps.push(PlanStep {
            kind: pl.step.into(),
            what: pl.what.clone(),
            job: None,
        });
    }
    if profile.desktop_offers() {
        steps.push(PlanStep {
            kind: "desktop".into(),
            what:
                "offer the live wallpaper and the screen saver (nothing changes unless you say yes)"
                    .into(),
            job: None,
        });
    }
    if planned.is_empty() && !profile.desktop_offers() {
        steps.push(PlanStep {
            kind: "skip".into(),
            what:
                "nothing to import or build: the built-in examples are in the Quick Start already"
                    .into(),
            job: None,
        });
    }
    let mut out = Applied {
        profile: profile.name().into(),
        title: profile.title().into(),
        dry_run: a.dry_run,
        data: paths.data.display().to_string(),
        library: lib_path.display().to_string(),
        record: None,
        steps,
        jobs: vec![],
        desktop_offers: profile.desktop_offers(),
        notes,
    };
    if a.dry_run {
        return Ok(out);
    }
    // Make the jobs (an unfinished build of an earlier setup is carried on instead), record the
    // choice, then run them.
    let mut made: Vec<(i64, &'static str)> = vec![];
    for pl in &planned {
        let reuse = if pl.kind == "explore" {
            lib.jobs_labelled(BUILD_LABEL)
                .map_err(catalog_err)?
                .into_iter()
                .rev()
                .find(|j| j.kind == "explore" && j.status != JobStatus::Done)
                .map(|j| j.id)
        } else {
            None
        };
        let job = match reuse {
            Some(j) => j,
            None => jobs::create(&lib, &pl.spec).map_err(ApiError::invalid)?,
        };
        made.push((job, pl.kind));
    }
    let mut n = 0;
    for st in &mut out.steps {
        if st.kind == "pack_import" || st.kind == "quickstart_build" {
            st.job = made.get(n).map(|m| m.0);
            n += 1;
        }
    }
    if let Some(s) = &settings {
        let r = SetupRecord {
            version: 1,
            profile: profile.name().into(),
            date: jetcarp_core::pattern::now_utc(),
            jetcarp: jetcarp_core::VERSION.into(),
            data_dir: paths.data.display().to_string(),
            library: lib_path.display().to_string(),
            quickstart_minutes: minutes,
            packs: planned.iter().filter_map(|p| p.pack.clone()).collect(),
            jobs: made.iter().map(|m| m.0).collect(),
        };
        out.record = Some(write_record(s, &r)?.display().to_string());
    }
    let how = |gpu: bool| How {
        threads: a.threads.unwrap_or(0),
        gpu,
        budget: BudgetIn::default().budget(),
        background: a.background,
        detach: false,
        files: FilesIn::default(),
    };
    for (job, kind) in &made {
        let r: Result<SearchStarted, ApiError> =
            run_job(cx, &lib, *job, &how(*kind == "explore" && a.gpu), p);
        if let Err(e) = r {
            // One failed job does not stop the others; its status says why.
            out.notes.push(format!("job {job}: {e}"));
        }
    }
    out.jobs = made
        .iter()
        .filter_map(|(j, _)| setup_job(cx, &lib, *j))
        .collect();
    for j in &out.jobs {
        if j.status == "failed" {
            out.notes.push(format!(
                "job {} failed; `jetcarp jobs status {}` says why, and `jetcarp jobs resume {}` tries again",
                j.job, j.job, j.job
            ));
        } else if j.status == "paused" && !a.background {
            out.notes.push(format!(
                "job {} stopped before its end; `jetcarp jobs resume {}` carries it on",
                j.job, j.job
            ));
        }
    }
    Ok(out)
}

/// `setup.apply`.
pub struct SetupApply;
impl Op for SetupApply {
    const NAME: &'static str = "setup.apply";
    const SUMMARY: &'static str = "Set JETCARP up with a profile (desktop-art, explorer, researcher, everything, blank-slate): record the choice in the settings folder, then import the profile's packs and run its Quick Start build as jobs, here or in the background.";
    const READ_ONLY: bool = false;
    type In = ApplyIn;
    type Out = Applied;
    fn run(cx: &Ctx, a: ApplyIn, p: &dyn Progress) -> Result<Applied, ApiError> {
        apply(cx, a, p)
    }
}
