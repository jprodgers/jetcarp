//! `system.*`: JETCARP's folders, what it sees on this machine, and undoing what it set up.

use crate::ctx::{Ctx, Desktop, GpuAdapter, GpuPolicy, Paths};
use crate::error::ApiError;
use crate::op::{NoInput, Op, Progress, Report};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

/// Bytes in a folder and everything under it (a file's own size for a file).
pub fn folder_size(p: &Path) -> u64 {
    let Ok(rd) = std::fs::read_dir(p) else {
        return std::fs::metadata(p).map(|m| m.len()).unwrap_or(0);
    };
    rd.filter_map(|e| e.ok())
        .map(|e| {
            let path = e.path();
            match e.file_type() {
                Ok(t) if t.is_dir() => folder_size(&path),
                Ok(t) if t.is_file() => e.metadata().map(|m| m.len()).unwrap_or(0),
                _ => 0,
            }
        })
        .sum()
}

/// Deletes `dir` and everything in it except what is in `keep` (files, or folders kept whole)
/// and the folders that hold them, and returns what was kept. Uninstalling deletes the settings
/// folder this way, keeping the picture the desktop shows (a still the host left, or on Windows
/// the copy of the user's own picture that was put back), so the desktop is never left pointing at
/// a deleted file, and keeping the data folder when it is inside and was not asked for (macOS).
/// Keeping `dir` itself keeps everything.
///
/// A link, a symbolic link or on Windows a junction, is removed as a link and never followed,
/// whether it is `dir` itself or something inside it: nothing behind it is read, emptied or
/// deleted. When `dir` is a link, only the link goes, and what `keep` names behind it stays where
/// it is, untouched (so nothing is reported kept).
pub fn remove_dir_keeping(dir: &Path, keep: &[PathBuf]) -> std::io::Result<Vec<PathBuf>> {
    if is_link(dir) {
        remove_link(dir)?;
        return Ok(Vec::new());
    }
    let root = canonical(dir);
    let mut inside: Vec<PathBuf> = keep
        .iter()
        .map(|k| canonical(k))
        .filter(|k| k.starts_with(&root) && k.exists())
        .collect();
    inside.sort();
    inside.dedup();
    if inside.is_empty() {
        std::fs::remove_dir_all(dir)?;
        return Ok(inside);
    }
    if inside.contains(&root) {
        return Ok(inside);
    }
    /// Empties `d` but for `keep`; true when nothing is left in it.
    fn empty(d: &Path, keep: &[PathBuf]) -> std::io::Result<bool> {
        let mut left = false;
        for e in std::fs::read_dir(d)? {
            let p = e?.path();
            if keep.contains(&p) {
                left = true;
            } else if is_link(&p) {
                remove_link(&p)?;
            } else if std::fs::symlink_metadata(&p)?.is_dir() {
                if empty(&p, keep)? {
                    std::fs::remove_dir(&p)?;
                } else {
                    left = true;
                }
            } else {
                std::fs::remove_file(&p)?;
            }
        }
        Ok(!left)
    }
    empty(&root, &inside)?;
    Ok(inside)
}

/// Whether `p` is a link to somewhere else: a symbolic link, or on Windows a junction (std counts
/// both as symbolic links; neither is followed here).
pub fn is_link(p: &Path) -> bool {
    std::fs::symlink_metadata(p).is_ok_and(|m| m.file_type().is_symlink())
}

/// Removes the link `p` itself, never what it leads to: `remove_file` for a link on Unix and a
/// file link on Windows, `remove_dir` for a junction or a link to a folder on Windows, which
/// removes the link only.
fn remove_link(p: &Path) -> std::io::Result<()> {
    std::fs::remove_file(p).or_else(|e| std::fs::remove_dir(p).map_err(|_| e))
}

/// `p` with its symbolic links followed, or as it is when that fails.
fn canonical(p: &Path) -> PathBuf {
    std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf())
}

/// Whether `inner` is `outer` or a folder inside it, as named or with symbolic links followed
/// (on macOS the data folder is inside the settings folder).
fn within(inner: &Path, outer: &Path) -> bool {
    inner.starts_with(outer) || canonical(inner).starts_with(canonical(outer))
}

/// A size for people: `512 B`, `3.4 MB`.
pub fn human_bytes(bytes: u64) -> String {
    let units = ["B", "KB", "MB", "GB", "TB"];
    let mut v = bytes as f64;
    let mut i = 0;
    while v >= 1000.0 && i + 1 < units.len() {
        v /= 1000.0;
        i += 1;
    }
    if i == 0 {
        format!("{bytes} B")
    } else {
        format!("{v:.1} {}", units[i])
    }
}

/// `JETCARP_*` environment variables, and wgpu's that change which adapters the GPU block lists
/// (`WGPU_BACKEND`, ...), sorted.
fn jetcarp_env() -> Vec<(String, String)> {
    let mut v: Vec<(String, String)> = std::env::vars()
        .filter(|(k, _)| k.starts_with("JETCARP_") || k.starts_with("WGPU_"))
        .collect();
    v.sort();
    v
}

/// The folders as `jetcarp paths` prints them.
fn paths_text(p: &Paths) -> String {
    format!(
        "settings: {}\ndata:     {}\noutput:   {}\nbundled:  {}\nruntime:  {}\n",
        p.settings
            .as_ref()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| "(none on this system)".into()),
        p.data.display(),
        p.output.display(),
        p.bundled_packs
            .as_ref()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| "(no packs came with the program)".into()),
        p.runtime
            .as_ref()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| "(none on this system)".into())
    )
}

impl Report for Paths {
    fn text(&self) -> String {
        paths_text(self)
    }
}

/// `system.paths`.
pub struct SystemPaths;
impl Op for SystemPaths {
    const NAME: &'static str = "system.paths";
    const SUMMARY: &'static str = "The settings, data, output, and runtime folders JETCARP uses (environment overrides included), and the folder of packs that came with the program.";
    const READ_ONLY: bool = true;
    type In = NoInput;
    type Out = Paths;
    fn run(cx: &Ctx, _: NoInput, _: &dyn Progress) -> Result<Paths, ApiError> {
        Ok(cx.paths())
    }
}

/// One of the strobe guard's switches (#97).
#[derive(Clone, Debug, Serialize, JsonSchema)]
pub struct GuardSwitch {
    /// Where it applies, in words.
    pub applies_to: String,
    pub on: bool,
    /// When it was turned off (UTC), and by which JETCARP.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub off_since: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub off_by: Option<String>,
}

/// The strobe guard's switches, as saved with the photosensitive seizure warning's answer.
#[derive(Clone, Debug, Serialize, JsonSchema)]
pub struct StrobeGuardStatus {
    /// The warning's answer: `no_disorder` or `at_risk`; none before it has been answered.
    pub answer: Option<String>,
    /// The app: its view, full screen, outputs, previews and exported animations.
    pub app: GuardSwitch,
    /// The desktop background and the screen saver.
    pub desktop: GuardSwitch,
    /// Whether a switch can be turned off at all: only after the answer *no disorder*, and only
    /// in the app.
    pub can_turn_off: bool,
    /// How a switch is turned off.
    pub how: String,
    /// The viewing the area rule assumes: millimetres away, pixels an inch at a scale of 1, and
    /// the 10° field that makes, in pixels across.
    pub viewing_distance_mm: f64,
    pub reference_dpi: f64,
    pub field_px: f64,
}

impl Report for StrobeGuardStatus {
    fn text(&self) -> String {
        let mut s = String::from("strobe guard\n");
        for sw in [&self.app, &self.desktop] {
            let _ = write!(
                s,
                "  {}: {}",
                sw.applies_to,
                if sw.on { "on" } else { "off" }
            );
            if let (Some(d), Some(v)) = (&sw.off_since, &sw.off_by) {
                let _ = write!(s, " (turned off {d}, JETCARP {v})");
            }
            s.push('\n');
        }
        let answer = match self.answer.as_deref() {
            Some("no_disorder") => "no epilepsy or seizure disorder",
            Some("at_risk") => "has, or may have, a seizure disorder: the guard is always on",
            _ => "the photosensitive seizure warning has not been answered yet",
        };
        let _ = writeln!(s, "answer: {answer}");
        let _ = writeln!(s, "{}", self.how);
        s
    }
}

fn guard_status(settings: Option<&Path>) -> StrobeGuardStatus {
    use jetcarp_core::photosensitivity::{self as ps, Answer, GuardPlace};
    let rec = settings.and_then(ps::read_in);
    let switch = |place: GuardPlace| {
        let off = rec.as_ref().and_then(|r| match place {
            GuardPlace::App => r.guard_off.clone(),
            GuardPlace::Desktop => r.desktop_guard_off.clone(),
        });
        let on = rec.as_ref().is_none_or(|r| r.guard_on(place));
        GuardSwitch {
            applies_to: place.words().to_string(),
            on,
            off_since: off.as_ref().filter(|_| !on).map(|o| o.date.clone()),
            off_by: off.filter(|_| !on).map(|o| o.jetcarp),
        }
    };
    let v = jetcarp_core::strobe::Viewing::screen(1.0);
    StrobeGuardStatus {
        answer: rec.as_ref().map(|r| r.answer.name().to_string()),
        app: switch(GuardPlace::App),
        desktop: switch(GuardPlace::Desktop),
        can_turn_off: rec.as_ref().is_some_and(|r| r.answer == Answer::NoDisorder),
        how: ps::GUARD_HOW.to_string(),
        viewing_distance_mm: jetcarp_core::strobe::VIEWING_DISTANCE_MM,
        reference_dpi: jetcarp_core::strobe::REFERENCE_DPI,
        field_px: (v.field_px * 10.0).round() / 10.0,
    }
}

/// `system.strobe_guard`.
pub struct SystemStrobeGuard;
impl Op for SystemStrobeGuard {
    const NAME: &'static str = "system.strobe_guard";
    const SUMMARY: &'static str = "The strobe guard's switches (on unless turned off in the app's Settings → Safety, which no tool can do), with the warning's answer.";
    const READ_ONLY: bool = true;
    type In = NoInput;
    type Out = StrobeGuardStatus;
    fn run(cx: &Ctx, _: NoInput, _: &dyn Progress) -> Result<StrobeGuardStatus, ApiError> {
        Ok(guard_status(cx.paths().settings.as_deref()))
    }
}

/// Input of `system.strobe_guard_on`.
#[derive(Clone, Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GuardOnIn {
    /// `app`, `desktop`, or both when left out.
    #[serde(default)]
    pub place: Option<String>,
}

/// `system.strobe_guard_on`: turns the strobe guard back on. There is no operation that turns it
/// off.
pub struct SystemStrobeGuardOn;
impl Op for SystemStrobeGuardOn {
    const NAME: &'static str = "system.strobe_guard_on";
    const SUMMARY: &'static str = "Turn the strobe guard back on, for the app, the desktop background and screen saver, or both. Only on: turning it off is the app's alone.";
    const READ_ONLY: bool = false;
    type In = GuardOnIn;
    type Out = StrobeGuardStatus;
    fn run(cx: &Ctx, i: GuardOnIn, _: &dyn Progress) -> Result<StrobeGuardStatus, ApiError> {
        use jetcarp_core::photosensitivity::{self as ps, GuardPlace};
        let places = match i.place.as_deref() {
            None | Some("both") => vec![GuardPlace::App, GuardPlace::Desktop],
            Some("app") => vec![GuardPlace::App],
            Some("desktop") => vec![GuardPlace::Desktop],
            Some(p) => {
                return Err(ApiError::invalid(format!(
                    "{p:?} is not a place: give app, desktop or both"
                )));
            }
        };
        let settings = cx.paths().settings;
        if let Some(dir) = settings.as_deref() {
            for p in places {
                // With no answer yet the guard is on already: nothing to save.
                if ps::read_in(dir).is_some() {
                    ps::set_guard_in(dir, p, true).map_err(ApiError::io)?;
                }
            }
        }
        Ok(guard_status(settings.as_deref()))
    }
}

/// Folders in the doctor's report.
#[derive(Clone, Debug, Serialize, JsonSchema)]
pub struct DoctorFolders {
    pub settings: Option<PathBuf>,
    pub data: PathBuf,
    pub data_size_bytes: u64,
    pub output: PathBuf,
    /// The catalog file.
    pub library: PathBuf,
    pub library_exists: bool,
    #[serde(skip)]
    pub library_bytes: u64,
    /// The packs that came with the program (see `system.paths`).
    pub bundled_packs: Option<PathBuf>,
    /// The runtime folder: the host's lock and the endpoint files (see `system.paths`).
    pub runtime: Option<PathBuf>,
    /// Bytes in that folder; none when it is missing.
    pub bundled_packs_bytes: Option<u64>,
    /// Where the program looked for them, in order (`$JETCARP_BUNDLED_PACKS` alone when set).
    pub bundled_packs_searched: Vec<PathBuf>,
    /// The data folder is the one chosen in setup (`paths.json`), and it does not exist: moved
    /// or deleted since, so JETCARP starts an empty library there when it next needs one.
    pub data_chosen_missing: bool,
}

/// What JETCARP sees on this machine.
#[derive(Clone, Debug, Serialize, JsonSchema)]
pub struct DoctorReport {
    pub version: String,
    pub os: String,
    pub arch: String,
    pub threads: usize,
    /// The machine: its hardware, its JETCARP Hardware Score and its limits
    /// (`machine.info`).
    pub machine: crate::ops::machine::MachineOut,
    pub folders: DoctorFolders,
    pub gpu: Vec<GpuAdapter>,
    /// The GPU policy, and the adapter each purpose uses and why (none: no GPU support here).
    pub gpu_policy: Option<GpuPolicy>,
    pub desktop: Desktop,
    /// `JETCARP_*` and `WGPU_*` variables as `[name, value]`.
    pub environment: Vec<(String, String)>,
}

impl Report for DoctorReport {
    fn text(&self) -> String {
        let f = &self.folders;
        let mut s = format!(
            "JETCARP {} on {} {} ({} threads)\n\n",
            self.version, self.os, self.arch, self.threads
        );
        s.push_str(&machine_text(&self.machine));
        s.push('\n');
        s.push_str("Folders\n");
        let paths = Paths {
            settings: f.settings.clone(),
            data: f.data.clone(),
            output: f.output.clone(),
            bundled_packs: f.bundled_packs.clone(),
            runtime: f.runtime.clone(),
        };
        for l in paths_text(&paths).lines() {
            if l.starts_with("bundled:") {
                continue;
            }
            let _ = writeln!(s, "  {l}");
        }
        match (&f.bundled_packs, f.bundled_packs_bytes) {
            (Some(p), Some(b)) => {
                let _ = writeln!(s, "  bundled:  {} ({})", p.display(), human_bytes(b));
            }
            (Some(p), None) => {
                let _ = writeln!(s, "  bundled:  {} (missing)", p.display());
            }
            (None, _) => {
                let looked: Vec<String> = f
                    .bundled_packs_searched
                    .iter()
                    .map(|p| p.display().to_string())
                    .collect();
                let _ = writeln!(
                    s,
                    "  bundled:  none (looked in {})",
                    if looked.is_empty() {
                        "no folder: the program's own path is unknown".into()
                    } else {
                        looked.join(", ")
                    }
                );
            }
        }
        let _ = writeln!(
            s,
            "  library:  {} ({})",
            f.library.display(),
            if f.library_exists {
                human_bytes(f.library_bytes)
            } else {
                "not created yet".into()
            }
        );
        let _ = writeln!(s, "  data in use: {}", human_bytes(f.data_size_bytes));
        if f.data_chosen_missing {
            let _ = writeln!(
                s,
                "  note: the data folder was chosen in setup (paths.json) and no longer exists. If you \
                 moved it, `jetcarp setup --data-dir DIR` names its new place; otherwise JETCARP \
                 starts an empty library there."
            );
        }
        s.push('\n');
        if let Some(p) = &self.gpu_policy {
            s.push_str(&gpu_policy_text(p));
            s.push('\n');
        }
        s.push_str("Graphics adapters (* = used for computing)\n");
        if self.gpu.is_empty() {
            s.push_str("  none found: simulation runs on the CPU\n");
        }
        for g in &self.gpu {
            let _ = writeln!(
                s,
                "  {} {} ({}, {}{})",
                if g.chosen { "*" } else { " " },
                g.name,
                g.backend,
                g.kind,
                if g.driver.is_empty() {
                    String::new()
                } else {
                    format!(", {}", g.driver)
                }
            );
            if let Some(p) = &g.problem {
                let _ = writeln!(s, "      {p}");
            }
        }
        let d = &self.desktop;
        s.push_str("\nDesktop\n");
        let _ = writeln!(
            s,
            "  background host: {}",
            if d.host_running {
                "running"
            } else {
                "not running"
            }
        );
        let _ = writeln!(
            s,
            "  starts at login: {}",
            if d.starts_at_login { "yes" } else { "no" }
        );
        for line in [&d.gnome_extension, &d.screen_saver].into_iter().flatten() {
            let _ = writeln!(s, "  {line}");
        }
        if let Some(l) = &d.launcher {
            let _ = writeln!(s, "  launcher: {l}");
        }
        if !self.environment.is_empty() {
            s.push_str("\nEnvironment\n");
            for (k, v) in &self.environment {
                let _ = writeln!(s, "  {k}={v}");
            }
        }
        s
    }
}

/// The machine section of `jetcarp doctor`: the hardware, the score in a line, and the limits
/// that matter most (`jetcarp limits` shows every one).
fn machine_text(m: &crate::ops::machine::MachineOut) -> String {
    use jetcarp_core::machine::Id;
    let mut s = String::from("This computer\n");
    s.push_str(&crate::ops::machine::hardware_text(&m.hardware));
    match &m.score {
        Some(sc) => {
            let _ = writeln!(
                s,
                "  JETCARP Hardware Score: {} (score v{}, {}{})",
                sc.text(),
                sc.benchmark,
                if sc.date.is_empty() {
                    "undated"
                } else {
                    &sc.date
                },
                match sc.wrong().count() {
                    0 => String::new(),
                    n => format!(", {n} wrong answers"),
                }
            );
        }
        None => s.push_str("  JETCARP Hardware Score: not measured yet (`jetcarp score`)\n"),
    }
    if let Some(why) = &m.stale {
        let _ = writeln!(s, "      note: {why}: `jetcarp score` measures it again");
    }
    if let Some(p) = &m.limits.pool {
        let _ = writeln!(s, "  JETCARP {}", crate::ops::machine::pool_line(p));
    }
    for id in [Id::ReservedJetcarp, Id::World, Id::AiShare] {
        if let Some(e) = m.limits.entries.iter().find(|e| e.id == id) {
            let _ = writeln!(
                s,
                "  {}: {}{}",
                e.label
                    .to_lowercase()
                    .replace("jetcarp", "JETCARP")
                    .replace("an ai", "an AI"),
                e.text(),
                if e.source == jetcarp_core::machine::Source::Own {
                    " (your own value)"
                } else {
                    ""
                }
            );
        }
    }
    s.push_str("  every limit: `jetcarp limits`\n");
    s
}

/// The GPU section of `jetcarp doctor`.
fn gpu_policy_text(p: &GpuPolicy) -> String {
    let mut s = format!("GPU: {}\n", p.summary);
    match &p.setting_file {
        Some(f) => {
            let _ = writeln!(
                s,
                "  setting: {} ({})",
                f.display(),
                if f.exists() { "saved" } else { "not saved" }
            );
        }
        None => s.push_str("  setting: no settings folder on this system\n"),
    }
    if let Some(n) = &p.note {
        let _ = writeln!(s, "  note: {n}");
    }
    for g in &p.programs {
        let mode = match g.mode {
            crate::ctx::GpuMode::Off => "off",
            crate::ctx::GpuMode::Auto => "auto",
            crate::ctx::GpuMode::On => "on",
        };
        let _ = writeln!(
            s,
            "  {}: {mode}, from {} (its own choice: {})",
            g.name, g.from, g.choice
        );
    }
    // Off for the app and the desktop background alike: nothing uses the GPU, and doctor ran no
    // self-test.
    let all_off = !p.programs.is_empty()
        && p.programs
            .iter()
            .all(|g| g.mode == crate::ctx::GpuMode::Off);
    for c in &p.choices {
        match &c.adapter {
            Some(a) => {
                let _ = writeln!(s, "  {}: {a} ({})", c.purpose, c.kind);
            }
            None => {
                let _ = writeln!(s, "  {}: none", c.purpose);
            }
        }
        let _ = writeln!(s, "      why: {}", c.why);
        if !c.limits.is_empty() {
            let _ = writeln!(s, "      limits: {}", c.limits);
        }
        if let Some(t) = &c.self_test {
            let _ = writeln!(
                s,
                "      self-test: {} ({} ms{}): {}",
                if t.passed { "passed" } else { "failed" },
                t.millis,
                if t.cached { ", kept from before" } else { "" },
                t.note
            );
        }
        match &c.problem {
            Some(e) => {
                let _ = writeln!(s, "      the GPU simulation does not use it: {e}");
            }
            None if c.adapter.is_some() && all_off => {
                let _ = writeln!(
                    s,
                    "      not used, and not tested: the GPU simulation is off"
                );
            }
            // Who simulates there: the app and the host step their worlds on the device they
            // draw with, the display adapter; runs, searches and scripts on the compute one (#40).
            None if c.adapter.is_some() => {
                let who = if c.purpose == "display" {
                    "the app and the desktop background simulate on it"
                } else {
                    "runs, searches and scripts simulate on it"
                };
                let _ = writeln!(s, "      {who}");
            }
            None => {}
        }
    }
    s
}

/// `system.doctor`.
pub struct SystemDoctor;
impl Op for SystemDoctor {
    const NAME: &'static str = "system.doctor";
    const SUMMARY: &'static str = "What JETCARP sees on this machine: version, folders, graphics adapters, desktop integration (for bug reports).";
    const READ_ONLY: bool = true;
    type In = NoInput;
    type Out = DoctorReport;
    fn run(cx: &Ctx, _: NoInput, _: &dyn Progress) -> Result<DoctorReport, ApiError> {
        let paths = cx.paths();
        let library = cx.library_path();
        let library_exists = library.exists();
        let data_chosen_missing = !paths.data.exists()
            && jetcarp_core::paths::data_dir_source(paths.settings.as_deref()).1
                == jetcarp_core::paths::DataSource::Chosen;
        let bundled_packs_bytes = paths
            .bundled_packs
            .as_ref()
            .filter(|p| p.is_dir())
            .map(|p| folder_size(p));
        Ok(DoctorReport {
            version: jetcarp_core::VERSION.to_string(),
            os: std::env::consts::OS.to_string(),
            arch: std::env::consts::ARCH.to_string(),
            threads: std::thread::available_parallelism()
                .map(|n| n.get())
                .unwrap_or(1),
            machine: crate::ops::machine::machine_info(cx),
            folders: DoctorFolders {
                data_size_bytes: folder_size(&paths.data),
                settings: paths.settings,
                data: paths.data,
                output: paths.output,
                library_bytes: if library_exists {
                    std::fs::metadata(&library).map(|m| m.len()).unwrap_or(0)
                } else {
                    0
                },
                library,
                library_exists,
                bundled_packs: paths.bundled_packs,
                runtime: paths.runtime,
                bundled_packs_bytes,
                bundled_packs_searched: jetcarp_core::paths::bundled_packs_candidates(),
                data_chosen_missing,
            },
            gpu: cx.platform.gpu_adapters(),
            gpu_policy: cx.platform.gpu_policy(),
            desktop: cx.platform.desktop(),
            environment: jetcarp_env(),
        })
    }
}

/// What to delete besides undoing the desktop integration.
#[derive(Clone, Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct UninstallIn {
    /// Do it. Without this, only say what would be done.
    #[serde(default)]
    pub yes: bool,
    /// Also delete the settings folder (except a picture the desktop still shows, and the
    /// publisher's files unless `publisher_key`), only when everything in it is JETCARP's: a
    /// folder holding anything else is refused.
    #[serde(default)]
    pub settings: bool,
    /// With `settings`, also delete the publisher's files in the settings folder's `pack/`: the
    /// publisher key and its public half, `publisher.json`, and the rater book. `settings` keeps
    /// them otherwise, since the key is the identity behind every pack the publisher signed and
    /// the rater book holds raters' names and their consent; neither can be made again. Refused
    /// without `settings`.
    #[serde(default)]
    pub publisher_key: bool,
    /// Also delete the data folder (the catalog, search databases, packs), only when everything
    /// in it is JETCARP's: a folder holding anything else is refused. Cannot be undone.
    #[serde(default)]
    pub data: bool,
}

/// One step of uninstalling.
#[derive(Clone, Debug, Serialize, JsonSchema)]
pub struct UndoResult {
    pub what: String,
    /// Whether it worked (only when it was done).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ok: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// What else to know about a step that worked (files it kept).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

/// A folder kept, with its size.
#[derive(Clone, Debug, Serialize, JsonSchema)]
pub struct KeptFolder {
    pub path: PathBuf,
    pub bytes: u64,
}

/// The publisher's files kept in the settings folder.
#[derive(Clone, Debug, Serialize, JsonSchema)]
pub struct KeptPublisher {
    /// Their folder, `<settings>/pack`.
    pub folder: PathBuf,
    /// The files kept there, by name: those of `publisher_ed25519`, `publisher_ed25519.pub`,
    /// `publisher.json` and `raters.json` that are there.
    pub files: Vec<String>,
}

impl KeptPublisher {
    /// The report's sentence: what is kept, where, why, and how to delete it.
    pub fn sentence(&self) -> String {
        let key = self.files.iter().any(|f| f != "raters.json");
        let raters = self.files.iter().any(|f| f == "raters.json");
        let (what, why, them) = match (key, raters) {
            (true, true) => (
                "your publisher key and rater book",
                "the key is the identity behind every pack you signed, and the rater book holds \
                 your raters' names and their consent",
                "them",
            ),
            (true, false) => (
                "your publisher key",
                "it is the identity behind every pack you signed",
                "it",
            ),
            _ => (
                "your rater book",
                "it holds your raters' names and their consent",
                "it",
            ),
        };
        format!(
            "Kept {what}, in {} ({}): {why}. Add --publisher-key to delete {them} too.",
            self.folder.display(),
            self.files.join(", ")
        )
    }
}

/// What uninstalling did, or would do.
#[derive(Clone, Debug, Serialize, JsonSchema)]
pub struct UninstallReport {
    /// The steps were run (`yes`), not only listed.
    pub done: bool,
    pub steps: Vec<UndoResult>,
    /// The data folder, when it was kept.
    pub kept_data: Option<KeptFolder>,
    /// The settings folder, when it was kept.
    pub kept_settings: Option<PathBuf>,
    /// The publisher's files `--settings` keeps (without `--publisher-key`), when there are any.
    pub kept_publisher: Option<KeptPublisher>,
    /// Folders `--settings` or `--data` asked to delete that were not there to delete, as
    /// sentences (#144): "the settings folder … is already gone".
    pub already_gone: Vec<String>,
    /// The output folder (never deleted).
    pub output: PathBuf,
    /// How this program was installed, and what removes it; none when that can't be told.
    pub installed_with: Option<InstalledWith>,
}

/// How a JETCARP program was installed, told by where it is, and what removes it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
pub struct InstalledWith {
    /// What installed it: `installer` (JETCARP's own), `homebrew`, `scoop`, `winget`, `pipx`,
    /// `uv`, `pip` or `cargo`.
    pub by: String,
    /// The folder the program is in, symbolic links followed.
    pub folder: PathBuf,
    /// The commands that remove it, in order (the PowerShell installer is saved first).
    pub remove: Vec<String>,
}

impl InstalledWith {
    /// `by` for people: "Homebrew", "JETCARP's installer".
    pub fn name(&self) -> &str {
        match self.by.as_str() {
            "installer" => "JETCARP's installer",
            "homebrew" => "Homebrew",
            "scoop" => "Scoop",
            other => other,
        }
    }
}

/// Where the installers are, the latest release's: any release's installer removes any
/// version's install (it checks its marker, and removes before it would download anything).
const INSTALLER_URL: &str =
    "https://github.com/jprodgers/jetcarp/releases/latest/download/jetcarp-installer";

/// How the program at `exe` (its path with symbolic links followed) was installed, on Windows or
/// not, from where it is (#103):
///
/// - JETCARP's installer leaves a `.jetcarp-installer` file in the program's folder
///   (`~/.local/lib/jetcarp`, `%LOCALAPPDATA%\Programs\jetcarp`, or the folder it was given);
/// - Homebrew keeps it in `<prefix>/Cellar/jetcarp/<version>/bin` (`/opt/homebrew`, `/usr/local`,
///   `/home/linuxbrew/.linuxbrew`), Scoop in `scoop\apps\jetcarp\<version>`, winget in
///   `Microsoft\WinGet\Packages\JimmieRodgers.JETCARP_<source>`;
/// - pipx and uv each make it a Python environment, `pipx/venvs/jetcarp` and `uv/tools/jetcarp`
///   (`uv\data\tools\jetcarp` on Windows); any other Python environment, with `pyvenv.cfg` above
///   the `bin` (`Scripts`) folder, is pip's;
/// - cargo installs into the `bin` folder of a root that holds its `.crates.toml` (`~/.cargo`).
///
/// `None` when it cannot tell: a release archive, a copy, pip outside a Python environment.
pub fn installed_with(exe: &Path, windows: bool) -> Option<InstalledWith> {
    let text = exe.to_string_lossy();
    let (dir, _) = text.rsplit_once(['/', '\\'])?;
    let folder = PathBuf::from(dir);
    // Matched without regard to case or separator: Windows and macOS ignore case.
    let norm = text.replace('\\', "/").to_lowercase();
    let has = |part: &str| norm.contains(part);
    let found = |by: &str, remove: Vec<String>| {
        Some(InstalledWith {
            by: by.into(),
            folder: folder.clone(),
            remove,
        })
    };
    if folder.join(".jetcarp-installer").is_file() {
        return found("installer", installer_commands(&folder, windows));
    }
    if has("/cellar/jetcarp/") {
        return found("homebrew", vec!["brew uninstall jetcarp".into()]);
    }
    if has("/scoop/apps/jetcarp/") {
        return found("scoop", vec!["scoop uninstall jetcarp".into()]);
    }
    if has("/microsoft/winget/packages/jimmierodgers.jetcarp_") {
        return found(
            "winget",
            vec!["winget uninstall JimmieRodgers.JETCARP".into()],
        );
    }
    if has("/pipx/venvs/jetcarp/") {
        return found("pipx", vec!["pipx uninstall jetcarp".into()]);
    }
    if has("/uv/tools/jetcarp/") || has("/uv/data/tools/jetcarp/") {
        return found("uv", vec!["uv tool uninstall jetcarp".into()]);
    }
    // A Python environment or cargo's root: the program is in its `bin` (`Scripts`) folder.
    let (root, bin) = dir.rsplit_once(['/', '\\'])?;
    let root = PathBuf::from(root);
    let bin = bin.to_lowercase();
    if (bin == "bin" || bin == "scripts") && root.join("pyvenv.cfg").is_file() {
        return found("pip", vec!["pip uninstall jetcarp".into()]);
    }
    if bin == "bin"
        && (root.join(".crates.toml").is_file()
            || root.join(".crates2.json").is_file()
            || has("/.cargo/bin/"))
    {
        let default = std::env::var_os("CARGO_HOME")
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
            .or_else(|| home_dir().map(|h| h.join(".cargo")));
        let mut cmd = "cargo uninstall jetcarp".to_string();
        if default.is_some_and(|d| canonical(&d) != canonical(&root)) {
            let _ = write!(cmd, " --root {}", quoted(&root, windows));
        }
        return found("cargo", vec![cmd]);
    }
    None
}

/// The home folder, from `HOME` (`USERPROFILE` on Windows).
fn home_dir() -> Option<PathBuf> {
    let vars: &[&str] = if cfg!(windows) {
        &["USERPROFILE"]
    } else {
        &["HOME"]
    };
    vars.iter()
        .find_map(|v| std::env::var_os(v).filter(|v| !v.is_empty()))
        .map(PathBuf::from)
}

/// The commands that run the latest release's installer with its option to uninstall, naming
/// the program's folder when it is not the installer's default.
fn installer_commands(folder: &Path, windows: bool) -> Vec<String> {
    let default = if windows {
        std::env::var_os("LOCALAPPDATA").map(|d| PathBuf::from(d).join("Programs").join("jetcarp"))
    } else {
        home_dir().map(|h| h.join(".local").join("lib").join("jetcarp"))
    };
    let same = default.is_some_and(|d| {
        let (d, f) = (canonical(&d), canonical(folder));
        if windows {
            d.to_string_lossy().to_lowercase() == f.to_string_lossy().to_lowercase()
        } else {
            d == f
        }
    });
    if windows {
        let mut run =
            "powershell -ExecutionPolicy Bypass -File \"$env:TEMP\\jetcarp-installer.ps1\" -Uninstall"
                .to_string();
        if !same {
            let _ = write!(run, " -InstallDir {}", quoted(folder, true));
        }
        vec![
            format!(
                "Invoke-WebRequest {INSTALLER_URL}.ps1 -OutFile \"$env:TEMP\\jetcarp-installer.ps1\""
            ),
            run,
        ]
    } else {
        let mut run = format!("curl -LsSf {INSTALLER_URL}.sh | sh -s -- --uninstall");
        if !same {
            let _ = write!(run, " --dir {}", quoted(folder, false));
        }
        vec![run]
    }
}

/// `p` as one argument to a shell: as it is when it holds nothing a shell would read, else in
/// single quotes (the shell's, or PowerShell's, which doubles a quote inside).
fn quoted(p: &Path, windows: bool) -> String {
    let s = p.to_string_lossy();
    if !s.is_empty()
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || "/\\._-:+".contains(c))
    {
        s.into_owned()
    } else if windows {
        format!("'{}'", s.replace('\'', "''"))
    } else {
        format!("'{}'", s.replace('\'', r"'\''"))
    }
}

impl Report for UninstallReport {
    fn text(&self) -> String {
        let mut s = String::from(
            "This undoes what JETCARP set up outside its folders. It never removes the program \
             itself, and deletes its settings and data only when asked (--settings, --data).\n\n",
        );
        // A folder asked for that was already gone is said, so a repeated `--settings --data` does
        // not read as if the options were ignored (#144).
        for g in &self.already_gone {
            let _ = writeln!(s, "Already gone: {g}.");
        }
        if self.steps.is_empty() {
            s.push_str(if self.already_gone.is_empty() {
                "Nothing to undo: JETCARP has not set anything up outside its folders.\n"
            } else {
                "Nothing else to undo: JETCARP has not set anything up outside its folders.\n"
            });
        } else {
            s.push_str(if self.done {
                "Undoing:\n"
            } else {
                "Would undo (run again with --yes):\n"
            });
            for st in &self.steps {
                match (&st.ok, &st.error) {
                    (Some(true), _) => {
                        let _ = writeln!(s, "  done: {}", st.what);
                        if let Some(n) = &st.note {
                            let _ = writeln!(s, "        {n}");
                        }
                    }
                    (Some(false), e) => {
                        let _ =
                            writeln!(s, "  FAILED: {}: {}", st.what, e.as_deref().unwrap_or(""));
                    }
                    _ => {
                        let _ = writeln!(s, "  {}", st.what);
                    }
                }
            }
        }
        let mut kept = Vec::new();
        if let Some(p) = &self.kept_publisher {
            kept.push(p.sentence());
        }
        if let Some(k) = &self.kept_data {
            kept.push(format!(
                "Kept the data folder {} ({}); add --data to delete it.",
                k.path.display(),
                human_bytes(k.bytes)
            ));
        }
        if let Some(k) = &self.kept_settings {
            kept.push(format!(
                "Kept the settings folder {}; add --settings to delete it.",
                k.display()
            ));
        }
        kept.push(format!(
            "Your pictures and exports in {} are never deleted.",
            self.output.display()
        ));
        let _ = writeln!(s, "\n{}", kept.join("\n"));
        if self.done
            && self
                .steps
                .iter()
                .any(|st| st.what.starts_with("remove the GNOME"))
        {
            s.push_str("Log out and back in once to finish removing the GNOME extension.\n");
        }
        // After the steps listed, when they are still to be done.
        let lead = if self.done || self.steps.is_empty() {
            "To remove"
        } else {
            "Then, to remove"
        };
        match &self.installed_with {
            Some(w) => {
                let place = match w.folder.parent().filter(|_| w.by == "pip") {
                    Some(env) => format!("in the Python environment {}", env.display()),
                    None => format!("in {}", w.folder.display()),
                };
                let how = if w.by == "pip" {
                    "this with that environment active"
                } else if w.by == "installer" && w.remove.len() > 1 {
                    // The PowerShell installer: saved, then run.
                    "these in PowerShell"
                } else {
                    "this"
                };
                let _ = writeln!(
                    s,
                    "\n{lead} the program itself ({place}, installed with {}), run {how}:\n",
                    w.name()
                );
                for c in &w.remove {
                    let _ = writeln!(s, "    {c}");
                }
            }
            None => {
                let _ = writeln!(
                    s,
                    "\n{lead} the program itself, use what installed it: the installer again \
                     with `{}`, `pipx uninstall jetcarp`, `uv tool uninstall jetcarp`, `pip \
                     uninstall jetcarp`, `brew uninstall jetcarp`, `scoop uninstall jetcarp`, \
                     `cargo uninstall jetcarp`, or delete its folder.",
                    if cfg!(windows) {
                        "-Uninstall"
                    } else {
                        "--uninstall"
                    }
                );
            }
        }
        s.push_str(
            "\nIf you connected JETCARP to an AI assistant, remove it there too: `claude mcp \
             remove jetcarp` for Claude Code, or the \"jetcarp\" entry in Claude Desktop's \
             claude_desktop_config.json.\n",
        );
        s
    }
}

/// The folder of job locks beside a catalog file: `<catalog file>-jobs`
/// (`jetcarp_search::jobs::lease::lock_path`).
fn jobs_folder(catalog: &Path) -> PathBuf {
    let name = catalog
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "catalog".into());
    catalog.with_file_name(format!("{name}-jobs"))
}

fn dir_is_empty(d: &Path) -> bool {
    std::fs::read_dir(d).is_ok_and(|mut e| e.next().is_none())
}

// What `uninstall --data` and `uninstall --settings` count as JETCARP's (#195). Each deletes its
// folder whole, so first it makes sure everything in the folder is something JETCARP keeps there.
// No release has written a marker into either folder, so this recognises the folders older
// versions made too; a folder named by mistake (`JETCARP_DATA_DIR`, `paths.json` or
// `JETCARP_CONFIG_DIR` pointing at a folder of documents) holds something else.
//
// **The lists live here: `DATA_FILES` and `DATA_FOLDERS`, `SETTINGS_FILES` and `SETTINGS_FOLDERS`
// (with `PACK_FILES`).** A file or folder JETCARP comes to keep in the data or settings folder must
// be added to them, as well as to `folders.md` ("What lives in each folder"), or `uninstall` will
// refuse a folder that holds it. The uninstall test (`tests/uninstall_folders.rs`) fills a
// folder with every name on them.

/// What may be inside one of JETCARP's folders.
#[derive(Clone, Copy)]
enum Inside {
    /// Anything: JETCARP's own pictures, the search databases of older versions.
    Anything,
    /// Files only, of any name: job locks, saved profiles and rotations.
    Files,
    /// Folders named by two hexadecimal digits (`media/`).
    HexFolders,
    /// Folders named by a UUID (`searches/`).
    UuidFolders,
    /// These files, and their temporary files while being written.
    Named(&'static [&'static str]),
}

/// The files JETCARP keeps at the top of a data folder: the library and what SQLite keeps beside
/// it; and, on Windows, where the data folder is also the runtime folder and the screen saver's
/// (`%LOCALAPPDATA%\jetcarp`, whose name Windows reads in any case), the host's and the tuner's
/// locks, the app's and the host's endpoint files, the HTTP endpoints' files (the MCP crate's
/// `CLI_ENDPOINT` and `APP_ENDPOINT`, which this crate cannot see: the names must stay in step),
/// and `JETCARP.scr`.
const DATA_FILES: &[&str] = &[
    "library.sqlite",
    "library.sqlite-wal",
    "library.sqlite-shm",
    "library.sqlite-journal",
    "host.lock",
    "tune.lock",
    "host-endpoint.json",
    "app-endpoint.json",
    "mcp-http-endpoint.json",
    "app-http-endpoint.json",
    "JETCARP.scr",
];

/// The folders JETCARP keeps in a data folder: the library's job locks, its media
/// (`media/ab/<sha256>.<ext>`), its searches (`searches/<uuid>/`), the search databases'
/// folder ([`jetcarp_core::paths::explore_dir`]), and, on Windows, where the data folder is also
/// the runtime folder, the memory pool's records (`memory/<pid>.json`) and the Start menu entry's
/// launcher (`launcher/`, `jetcarp_display::launcher::stable_dir`).
const DATA_FOLDERS: &[(&str, Inside)] = &[
    ("library.sqlite-jobs", Inside::Files),
    ("media", Inside::HexFolders),
    ("searches", Inside::UuidFolders),
    ("explore", Inside::Anything),
    (jetcarp_core::machine::pool::DIR, Inside::Files),
    (LAUNCHER_DIR, Inside::Named(LAUNCHER_FILES)),
];

/// Windows: the folder beside the runtime files where `jetcarp launcher install` keeps the copy of
/// the windowless launcher that the Start menu entry starts, and the record of the program it
/// starts (#105).
const LAUNCHER_DIR: &str = "launcher";
const LAUNCHER_FILES: &[&str] = &["jetcarpw.exe", "jetcarp-program"];

/// Whether `name`, at the top of the runtime folder, is something JETCARP's running programs keep
/// there: the host's and the tuner's locks, the app's and the host's endpoint files, the HTTP
/// endpoints' files (as in [`DATA_FILES`]), the memory pool's records, and on Windows (where the
/// runtime folder is also the data folder, the screen saver's and the Start menu entry's)
/// `JETCARP.scr` and the launcher's folder. `uninstall` removes the first ones
/// ([`RUNTIME_LOCKS`], [`RUNTIME_ENDPOINTS`], [`POOL_DIR`]) once their programs have gone.
pub(crate) fn runtime_entry(name: &str) -> bool {
    RUNTIME_LOCKS.contains(&name)
        || RUNTIME_ENDPOINTS.contains(&name)
        || name == "JETCARP.scr"
        || name == POOL_DIR
        || name == LAUNCHER_DIR
}

/// The files JETCARP keeps at the top of the settings folder: the app's, the host's and setup's
/// settings, the data folder chosen in setup, the answer to the photosensitive seizure warning,
/// the GPU setting and its self-tests, the host's command, the user's own desktop picture, and
/// the two macOS screen saver records.
const SETTINGS_FILES: &[&str] = &[
    "app.json",
    "host.json",
    "setup.json",
    "paths.json",
    "photosensitivity.json",
    "gpu.json",
    "gpu-checks.json",
    "host-program",
    "desktop-picture.json",
    "screen-saver-before.plist",
    "wallpaper-store-before-saver-remove.plist",
];

/// The publisher's files (`<settings>/pack/`): the private key and its public half, who the
/// publisher is, and the rater book.
const PACK_FILES: &[&str] = &[
    "publisher_ed25519",
    "publisher_ed25519.pub",
    "publisher.json",
    "raters.json",
];

/// The MCP server's kept HTTP tokens (`<settings>/http-tokens/`, a private folder), when the user
/// chose to keep them: `jetcarp mcp --http --keep-token`'s and the app's.
const HTTP_TOKEN_FILES: &[&str] = &["mcp-http.json", "app-http.json"];

/// The folders JETCARP keeps in the settings folder: screensaver profiles, rotations, the
/// pictures of *Fit to this computer*, the host's desktop stills, the publisher's files, and the
/// kept HTTP tokens.
const SETTINGS_FOLDERS: &[(&str, Inside)] = &[
    ("screensavers", Inside::Files),
    ("rotations", Inside::Files),
    ("tune", Inside::Anything),
    ("desktop-pictures", Inside::Anything),
    ("pack", Inside::Named(PACK_FILES)),
    ("http-tokens", Inside::Named(HTTP_TOKEN_FILES)),
];

/// A temporary file JETCARP writes beside one of `names` and then renames into place
/// (`setup.json.tmp-123`, `raters.tmp123`, `gpu.json.tmp`, `.host-endpoint.json.123.tmp`): the
/// name or its stem, then `tmp` with only digits, dots and dashes about it.
fn being_written(name: &str, names: &[&str]) -> bool {
    let n = name.strip_prefix('.').unwrap_or(name);
    names.iter().any(|known| {
        let stem = known.split('.').next().unwrap_or(known);
        [*known, stem].iter().any(|k| {
            n.strip_prefix(k)
                .filter(|rest| rest.starts_with('.'))
                .and_then(|rest| rest.split_once("tmp"))
                .is_some_and(|(a, b)| {
                    format!("{a}{b}")
                        .chars()
                        .all(|c| c.is_ascii_digit() || c == '.' || c == '-')
                })
        })
    })
}

/// What a system leaves in any folder it has shown (Finder, Explorer), which says nothing about
/// whose the folder is.
fn system_litter(name: &str) -> bool {
    matches!(
        name,
        ".DS_Store" | ".localized" | "Thumbs.db" | "desktop.ini"
    ) || name.starts_with("._")
}

/// A search's folder name: a UUID (`8-4-4-4-12` hexadecimal digits).
fn uuid_like(name: &str) -> bool {
    name.len() == 36
        && name.char_indices().all(|(i, c)| {
            if matches!(i, 8 | 13 | 18 | 23) {
                c == '-'
            } else {
                c.is_ascii_hexdigit()
            }
        })
}

/// Whether `p` is a folder of its own, not a link to one: a link is not looked into, since
/// deleting it removes only the link.
fn real_dir(p: &Path) -> bool {
    p.is_dir() && !is_link(p)
}

/// What in the folder `dir` JETCARP did not put there, as paths inside it (empty: everything in
/// it is JETCARP's, or it holds nothing), given what JETCARP keeps in it: `files` (with their
/// temporary files while being written) and `folders`, each with what may be inside. `also` is
/// one more entry to count as JETCARP's (the data folder inside the settings folder, as on macOS,
/// which `--data` checks on its own or the uninstall keeps).
fn not_jetcarps(
    dir: &Path,
    files: &[&str],
    folders: &[(&str, Inside)],
    also: Option<&str>,
) -> Vec<String> {
    /// An entry: a file, a folder, or a link (a symbolic link or a junction), which is never
    /// followed: deleting the folder removes the link, not where it leads.
    #[derive(Clone, Copy, PartialEq)]
    enum Kind {
        File,
        Folder,
        Link,
    }
    let entries = |d: &Path| -> Result<Vec<(String, Kind)>, String> {
        std::fs::read_dir(d)
            .map_err(|e| e.to_string())?
            .map(|e| {
                let e = e.map_err(|e| e.to_string())?;
                let t = e.file_type().map_err(|e| e.to_string())?;
                let kind = if t.is_symlink() {
                    Kind::Link
                } else if t.is_dir() {
                    Kind::Folder
                } else {
                    Kind::File
                };
                Ok((e.file_name().to_string_lossy().into_owned(), kind))
            })
            .collect()
    };
    let mut odd = Vec::new();
    let top = match entries(dir) {
        Ok(t) => t,
        Err(e) => return vec![format!("(it cannot be read: {e})")],
    };
    for (name, kind) in top {
        if system_litter(&name) || also == Some(name.as_str()) {
            continue;
        }
        // A link counts by its name alone: only the link is removed, so what it leads to is
        // never at stake.
        let file_name = files.contains(&name.as_str()) || being_written(&name, files);
        let folder = folders.iter().find(|(f, _)| *f == name).map(|&(_, i)| i);
        match (kind, file_name, folder) {
            (Kind::File, true, _) | (Kind::Link, true, _) | (Kind::Link, _, Some(_)) => continue,
            (Kind::Folder, _, Some(inside)) => {
                let ok = |n: &str, k: Kind| {
                    let d = k == Kind::Folder;
                    let link = k == Kind::Link;
                    match inside {
                        Inside::Anything => true,
                        Inside::Files => !d,
                        Inside::HexFolders => {
                            (d || link) && n.len() == 2 && n.chars().all(|c| c.is_ascii_hexdigit())
                        }
                        Inside::UuidFolders => (d || link) && uuid_like(n),
                        Inside::Named(names) => {
                            !d && (names.contains(&n) || being_written(n, names))
                        }
                    }
                };
                if matches!(inside, Inside::Anything) {
                    continue;
                }
                match entries(&dir.join(&name)) {
                    Ok(list) => odd.extend(
                        list.into_iter()
                            .filter(|(n, k)| !system_litter(n) && !ok(n, *k))
                            .map(|(n, _)| format!("{name}/{n}")),
                    ),
                    Err(e) => odd.push(format!("{name}/ (it cannot be read: {e})")),
                }
            }
            _ => odd.push(name),
        }
    }
    odd.sort();
    odd
}

/// A folder `uninstall` will not delete: which (`data` or `settings`), where, what in it is not
/// JETCARP's, where its name came from, and what to do if that was a mistake.
struct Refused {
    which: &'static str,
    folder: PathBuf,
    odd: Vec<String>,
    named: String,
    mistake: String,
}

/// The data folder's refusal, if `--data` must refuse it.
fn check_data(data: &Path, settings: Option<&Path>) -> Option<Refused> {
    use jetcarp_core::paths::{DataSource, PATHS_FILE};
    if !real_dir(data) {
        return None;
    }
    let odd = not_jetcarps(data, DATA_FILES, DATA_FOLDERS, None);
    if odd.is_empty() {
        return None;
    }
    let source = jetcarp_core::paths::data_dir_source(settings).1;
    let (named, mistake) = match source {
        DataSource::Env => (
            " (named by JETCARP_DATA_DIR)".to_string(),
            "If JETCARP_DATA_DIR names the data folder by mistake, correct or unset it and uninstall again. "
                .to_string(),
        ),
        DataSource::Chosen => (
            " (chosen in setup, in paths.json)".to_string(),
            format!(
                "If the data folder was chosen by mistake, delete {} to go back to the default one, and uninstall again. ",
                settings
                    .map(|s| s.join(PATHS_FILE).display().to_string())
                    .unwrap_or_else(|| PATHS_FILE.into())
            ),
        ),
        DataSource::Default => (String::new(), String::new()),
    };
    Some(Refused {
        which: "data",
        folder: data.to_path_buf(),
        odd,
        named,
        mistake,
    })
}

/// The settings folder's refusal, if `--settings` must refuse it. The data folder inside it (as
/// on macOS) is the data folder's own matter: `--data` checks it, and without `--data` it is kept.
fn check_settings(settings: &Path, data: &Path) -> Option<Refused> {
    if !real_dir(settings) {
        return None;
    }
    let inside = within(data, settings)
        .then(|| {
            data.strip_prefix(settings)
                .ok()
                .map(Path::to_path_buf)
                .or_else(|| {
                    canonical(data)
                        .strip_prefix(canonical(settings))
                        .ok()
                        .map(Path::to_path_buf)
                })
        })
        .flatten();
    let first = inside
        .as_deref()
        .and_then(|r| r.components().next())
        .map(|c| c.as_os_str().to_string_lossy().into_owned());
    let odd = not_jetcarps(settings, SETTINGS_FILES, SETTINGS_FOLDERS, first.as_deref());
    if odd.is_empty() {
        return None;
    }
    let env = std::env::var_os("JETCARP_CONFIG_DIR").filter(|v| !v.is_empty());
    let by_env = env.is_some_and(|v| Path::new(&v) == settings);
    Some(Refused {
        which: "settings",
        folder: settings.to_path_buf(),
        odd,
        named: if by_env {
            " (named by JETCARP_CONFIG_DIR)".into()
        } else {
            String::new()
        },
        mistake: if by_env {
            "If JETCARP_CONFIG_DIR names the settings folder by mistake, correct or unset it and uninstall again. "
                .into()
        } else {
            String::new()
        },
    })
}

/// `uninstall`'s refusal of the folders it was asked to delete and will not (#195): each named,
/// with where its name came from and what in it JETCARP did not put there (the first three, then
/// how many more).
fn refuse(refused: &[Refused]) -> ApiError {
    let parts: Vec<String> = refused
        .iter()
        .map(|r| {
            let shown: Vec<&str> = r.odd.iter().take(3).map(String::as_str).collect();
            let more = if r.odd.len() > 3 {
                format!(" and {} more", r.odd.len() - 3)
            } else {
                String::new()
            };
            format!(
                "the {} folder {}{} holds what JETCARP did not put there: {}{more}",
                r.which,
                r.folder.display(),
                r.named,
                shown.join(", ")
            )
        })
        .collect();
    let flags: Vec<String> = refused.iter().map(|r| format!("--{}", r.which)).collect();
    let (rule, gone, without) = if refused.len() == 1 {
        (
            format!(
                "`{}` deletes the {} folder only when everything in it is JETCARP's",
                flags[0], refused[0].which
            ),
            format!(
                "If it really is JETCARP's {} folder and you want it gone, delete it yourself.",
                refused[0].which
            ),
            format!(
                "Without {}, the uninstall undoes the rest and keeps the folder.",
                flags[0]
            ),
        )
    } else {
        (
            format!(
                "`{}` delete their folders only when everything in each is JETCARP's",
                flags.join("` and `")
            ),
            "If they really are JETCARP's folders and you want them gone, delete them yourself."
                .to_string(),
            format!(
                "Without {}, the uninstall undoes the rest and keeps the folders.",
                flags.join(" and ")
            ),
        )
    };
    let mistakes: String = refused.iter().map(|r| r.mistake.as_str()).collect();
    ApiError::conflict(format!("{}. {rule}, so nothing was done", parts.join("; ")))
        .hint(format!("{mistakes}{gone} {without}"))
}

// What `uninstall` removes from the runtime folder (#135): the files JETCARP's running programs
// keep there ([`runtime_entry`]), once the programs that keep them have gone, and then the folder
// itself when that empties it. On the Mac an app ended with SIGTERM left its `app-endpoint.json`
// in `~/.cache/jetcarp`, which `uninstall` neither listed nor removed. The screen saver and the
// Start menu entry's launcher, which share the folder on Windows, are their own steps' to remove.

/// The locks JETCARP's programs take in the runtime folder: the host's single-instance lock, and
/// *Fit to this computer*'s (one fit at a time, `jetcarp_host::tune`).
const RUNTIME_LOCKS: &[&str] = &["host.lock", "tune.lock"];

/// The files JETCARP's programs record their port and token in while they listen: the app's and
/// the host's (`jetcarp_mcp::link`), and the HTTP endpoints' (`jetcarp_mcp::http`'s
/// `CLI_ENDPOINT` and `APP_ENDPOINT`), which this crate cannot see: the names must stay in step,
/// as in [`DATA_FILES`].
const RUNTIME_ENDPOINTS: &[&str] = &[
    "app-endpoint.json",
    "host-endpoint.json",
    "mcp-http-endpoint.json",
    "app-http-endpoint.json",
];

/// The memory pool's records' folder in the runtime folder (`memory/`).
const POOL_DIR: &str = jetcarp_core::machine::pool::DIR;

/// The program that keeps runtime file `name` while it runs, in words.
fn runtime_owner(name: &str) -> &'static str {
    match name {
        "host.lock" => "a host",
        "tune.lock" => "a fit to this computer",
        "host-endpoint.json" => "the background host",
        "mcp-http-endpoint.json" => "jetcarp mcp --http",
        _ => "the JETCARP app",
    }
}

/// The process whose memory record `name` (in `memory/`) is: `<pid>.json`, or `<pid>.json.tmp`
/// while it is written.
fn pool_record(name: &str) -> Option<u32> {
    name.strip_suffix(".json.tmp")
        .or_else(|| name.strip_suffix(".json"))
        .filter(|pid| !pid.is_empty() && pid.bytes().all(|c| c.is_ascii_digit()))
        .and_then(|pid| pid.parse().ok())
}

/// JETCARP's files in the runtime folder `dir`, as names inside it, sorted: the locks, the
/// endpoint files (with any left half-written), then the memory pool's records and, last, their
/// folder (`memory/`) when it is there. Only files of their own count; a link is never followed.
fn runtime_leftovers(dir: &Path) -> Vec<String> {
    let files = |d: &Path| -> Vec<String> {
        let Ok(rd) = std::fs::read_dir(d) else {
            return Vec::new();
        };
        let mut names: Vec<String> = rd
            .flatten()
            .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    };
    let mut out: Vec<String> = files(dir)
        .into_iter()
        .filter(|n| {
            RUNTIME_LOCKS.contains(&n.as_str())
                || RUNTIME_ENDPOINTS.contains(&n.as_str())
                || being_written(n, RUNTIME_ENDPOINTS)
        })
        .collect();
    let pool = dir.join(POOL_DIR);
    if real_dir(&pool) {
        out.extend(
            files(&pool)
                .into_iter()
                .filter(|n| pool_record(n).is_some())
                .map(|n| format!("{POOL_DIR}/{n}")),
        );
        out.push(format!("{POOL_DIR}/"));
    }
    out
}

/// [`runtime_leftovers`] in words, for the step: the files at the top, then `memory/` with how
/// many records it holds.
fn runtime_list(found: &[String]) -> String {
    let records = found
        .iter()
        .filter(|n| n.starts_with(POOL_DIR) && !n.ends_with('/'))
        .count();
    let pool = found
        .iter()
        .any(|n| n.ends_with('/'))
        .then(|| match records {
            0 => format!("{POOL_DIR}/"),
            1 => format!("{POOL_DIR}/ (1 record)"),
            n => format!("{POOL_DIR}/ ({n} records)"),
        });
    found
        .iter()
        .filter(|n| !n.starts_with(POOL_DIR))
        .cloned()
        .chain(pool)
        .collect::<Vec<_>>()
        .join(", ")
}

/// Whether the runtime folder `dir` may go once it is empty: it is JETCARP's own (named
/// `jetcarp`, as the system's runtime folders are), not a link, and none of the folders the
/// uninstall keeps or deletes on its own (`keep`): on Windows it is the default data folder.
fn runtime_folder_goes(dir: &Path, keep: &[PathBuf]) -> bool {
    dir.file_name()
        .is_some_and(|n| n == jetcarp_core::paths::APP_DIR)
        && !is_link(dir)
        && !keep
            .iter()
            .any(|k| k == dir || canonical(k) == canonical(dir))
}

/// Whether the lock file `lock` is held, waiting up to `wait` for its holder to let it go (a
/// host that is stopping). Closed again, and so unlocked, before it returns, which deleting it
/// on Windows needs.
fn lock_held(lock: &Path, wait: std::time::Duration) -> Result<bool, String> {
    let deadline = std::time::Instant::now() + wait;
    loop {
        let f = match std::fs::OpenOptions::new().write(true).open(lock) {
            Ok(f) => f,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(false),
            Err(e) => return Err(e.to_string()),
        };
        match f.try_lock() {
            Ok(()) => return Ok(false),
            Err(std::fs::TryLockError::WouldBlock) if std::time::Instant::now() < deadline => {
                std::thread::sleep(std::time::Duration::from_millis(200));
            }
            Err(std::fs::TryLockError::WouldBlock) => return Ok(true),
            Err(std::fs::TryLockError::Error(e)) => return Err(e.to_string()),
        }
    }
}

/// What the endpoint file `p` records, if something accepts connections on the port it names:
/// its program is running (or another has taken its port, and the file is kept all the same). A
/// file with no port, such as one left half-written, leads nowhere. The same test as
/// `jetcarp_mcp::link::host_gone`, which this crate cannot see.
fn endpoint_answers(p: &Path) -> Option<serde_json::Value> {
    use std::net::{Ipv4Addr, SocketAddr, TcpStream};
    let v: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(p).ok()?).ok()?;
    let port = u16::try_from(v.get("port")?.as_u64()?)
        .ok()
        .filter(|&port| port != 0)?;
    let addr = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
    TcpStream::connect_timeout(&addr, std::time::Duration::from_millis(500))
        .is_ok()
        .then_some(v)
}

/// The program whose memory record `name` in the records' folder `pool` is still fresh
/// (refreshed within `jetcarp_core::machine::pool::STALE_SECS`, so it is running), in words;
/// none for a record of a program that has gone, or one that cannot be read. A record being
/// written counts as its program's record does.
fn record_fresh(pool: &Path, name: &str) -> Option<String> {
    use jetcarp_core::machine::pool::{STALE_SECS, Use};
    let pid = pool_record(name)?;
    let u: Use =
        serde_json::from_slice(&std::fs::read(pool.join(format!("{pid}.json"))).ok()?).ok()?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    (now.saturating_sub(u.at_ms) <= STALE_SECS * 1000).then_some(u.program)
}

/// Removes JETCARP's files from the runtime folder `dir` ([`runtime_leftovers`]), each once the
/// program that keeps it has gone: a lock once nothing holds it (waiting a little for a host
/// that is stopping), an endpoint file once nothing answers on its port, a memory record once
/// its program stops refreshing it. Then `memory/` and the folder itself, if that empties them
/// and the folder may go ([`runtime_folder_goes`]). A file still in use is kept, and the note
/// says which and why.
fn clear_runtime(dir: &Path, keep: &[PathBuf]) -> Result<Option<String>, String> {
    // This program's own record goes first: it is uninstalling, and records no more.
    let pool = dir.join(POOL_DIR);
    if pool.join(format!("{}.json", std::process::id())).exists() {
        jetcarp_core::machine::pool::stop();
    }
    let (mut kept, mut failed) = (Vec::new(), Vec::new());
    for name in runtime_leftovers(dir) {
        if name.ends_with('/') {
            continue;
        }
        let p = dir.join(&name);
        let in_use = if RUNTIME_LOCKS.contains(&name.as_str()) {
            let wait = if name == "host.lock" { 5 } else { 0 };
            match lock_held(&p, std::time::Duration::from_secs(wait)) {
                Ok(held) => {
                    held.then(|| format!("{} is running and holds it", runtime_owner(&name)))
                }
                Err(e) => {
                    failed.push(format!("{name}: {e}"));
                    continue;
                }
            }
        } else if let Some(record) = name.strip_prefix(&format!("{POOL_DIR}/")) {
            record_fresh(&pool, record).map(|program| format!("{program} is running"))
        } else {
            let file = RUNTIME_ENDPOINTS
                .iter()
                .copied()
                .find(|e| name == *e || being_written(&name, &[e]))
                .unwrap_or_default();
            let answers = endpoint_answers(&dir.join(file));
            if name == file {
                answers.map(|_| format!("{} is running", runtime_owner(file)))
            } else {
                // One being written (`.app-endpoint.json.<pid>.tmp`, renamed into place at
                // once) is in use only while the process named in it is the one that answers;
                // any other was left by a program that ended as it wrote it.
                let writer = name
                    .rsplit('.')
                    .nth(1)
                    .and_then(|pid| pid.parse::<u64>().ok());
                answers
                    .filter(|v| writer.is_some() && v["pid"].as_u64() == writer)
                    .map(|_| format!("{} is running and writing it", runtime_owner(file)))
            }
        };
        if let Some(why) = in_use {
            kept.push(format!("{name} ({why})"));
            continue;
        }
        match std::fs::remove_file(&p) {
            Ok(()) => {}
            // Gone already: its program removed it as it quit.
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => failed.push(format!("{name}: {e}")),
        }
    }
    if real_dir(&pool) && dir_is_empty(&pool) {
        let _ = std::fs::remove_dir(&pool);
    }
    let goes = runtime_folder_goes(dir, keep);
    if goes && dir_is_empty(dir) {
        match std::fs::remove_dir(dir) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => failed.push(format!("the folder: {e}")),
        }
    }
    if !failed.is_empty() {
        return Err(failed.join("; "));
    }
    Ok(if !kept.is_empty() {
        Some(format!(
            "kept {}; quit {} and uninstall again to remove the rest",
            kept.join(", "),
            if kept.len() == 1 { "it" } else { "them" }
        ))
    } else if goes && dir.exists() {
        // Something not JETCARP's, or on Windows the screen saver or the launcher's copy if
        // their own steps kept them.
        Some("kept the folder, which holds other files".into())
    } else {
        None
    })
}

/// `system.uninstall`.
pub struct SystemUninstall;
impl Op for SystemUninstall {
    const NAME: &'static str = "system.uninstall";
    const SUMMARY: &'static str = "Undo what JETCARP set up outside its folders (the host, its still on the desktop, login entry, GNOME extension, screen saver, launcher), and optionally delete its settings (keeping the picture the desktop shows) and data; only lists the steps unless `yes`.";
    const READ_ONLY: bool = false;
    type In = UninstallIn;
    type Out = UninstallReport;
    fn run(cx: &Ctx, a: UninstallIn, _: &dyn Progress) -> Result<UninstallReport, ApiError> {
        /// A step's work: a note on success (files kept), or why it failed.
        type Run = Box<dyn Fn() -> Result<Option<String>, String> + Send + Sync>;
        if a.publisher_key && !a.settings {
            return Err(ApiError::invalid(
                "--publisher-key only means something with --settings: it deletes the publisher's \
                 files (the publisher key and the rater book) that --settings keeps otherwise",
            )
            .hint("`jetcarp uninstall --settings --publisher-key` deletes the settings folder with them."));
        }
        let paths = cx.paths();
        // `--data` and `--settings` delete their folders whole, so first make sure each is
        // JETCARP's (#195), both before anything is done or listed. A folder that is a link is not
        // looked into: only the link is removed, and the folder it leads to is left alone.
        let refused: Vec<Refused> = [
            a.data
                .then(|| check_data(&paths.data, paths.settings.as_deref()))
                .flatten(),
            a.settings
                .then(|| {
                    paths
                        .settings
                        .as_deref()
                        .and_then(|c| check_settings(c, &paths.data))
                })
                .flatten(),
        ]
        .into_iter()
        .flatten()
        .collect();
        if !refused.is_empty() {
            return Err(refuse(&refused));
        }
        let mut steps: Vec<(String, Run)> = cx
            .platform
            .undo_steps()
            .into_iter()
            .map(|s| {
                let run = s.run;
                (s.what, Box::new(move || run().map(|()| None)) as Run)
            })
            .collect();
        // What JETCARP's programs leave behind once none of them is running: their files in the
        // runtime folder (#135), after the host was stopped above, and the empty folder of job
        // locks beside the catalog.
        if let Some(dir) = paths.runtime.clone().filter(|d| d.is_dir()) {
            let keep: Vec<PathBuf> = paths
                .settings
                .iter()
                .cloned()
                .chain([paths.data.clone(), paths.output.clone()])
                .collect();
            let found = runtime_leftovers(&dir);
            let goes = runtime_folder_goes(&dir, &keep);
            let what = if !found.is_empty() {
                Some(format!(
                    "remove what JETCARP's programs left in the runtime folder {}: {}{}",
                    dir.display(),
                    runtime_list(&found),
                    if goes {
                        ", then the folder if that empties it"
                    } else {
                        ""
                    }
                ))
            } else if goes && dir_is_empty(&dir) {
                Some(format!("remove the empty runtime folder {}", dir.display()))
            } else {
                None
            };
            if let Some(what) = what {
                steps.push((what, Box::new(move || clear_runtime(&dir, &keep))));
            }
        }
        let jobs = jobs_folder(&cx.library_path_now());
        if jobs.is_dir() && dir_is_empty(&jobs) {
            steps.push((
                format!("remove the empty folder of job locks {}", jobs.display()),
                Box::new(move || {
                    // Skipped, not failed, if a job started since (its lock is in it).
                    match std::fs::remove_dir(&jobs) {
                        Ok(()) => Ok(None),
                        Err(_) if jobs.is_dir() && !dir_is_empty(&jobs) => {
                            Ok(Some("kept: a job has started since".into()))
                        }
                        Err(e) => Err(e.to_string()),
                    }
                }),
            ));
        }
        let data = paths.data.clone();
        // On macOS the data folder is inside the settings folder (#103). Deleting both, it goes
        // first, so each step finds its own folder; deleting only the settings, it is kept.
        let data_inside = paths
            .settings
            .as_deref()
            .is_some_and(|c| data.exists() && within(&data, c));
        let mut settings_step: Option<(String, Run)> = None;
        let mut kept_publisher: Option<KeptPublisher> = None;
        if let Some(c) = paths.settings.clone().filter(|c| c.exists() || is_link(c)) {
            if a.settings {
                // Asked when the step runs, after the steps before it have put the user's own
                // picture back.
                let platform = cx.platform.clone();
                let keep_data = (data_inside && !a.data).then(|| data.clone());
                // The publisher's files stay unless asked for (`--publisher-key`): the key is the
                // identity behind every pack the publisher signed, and the rater book holds
                // raters' names and their consent. Only files of their own count (a link is
                // removed as a link).
                let keep_publisher: Vec<PathBuf> = if a.publisher_key || is_link(&c) {
                    Vec::new()
                } else {
                    PACK_FILES
                        .iter()
                        .map(|f| c.join("pack").join(f))
                        .filter(|f| std::fs::symlink_metadata(f).is_ok_and(|m| m.is_file()))
                        .collect()
                };
                if !keep_publisher.is_empty() {
                    kept_publisher = Some(KeptPublisher {
                        folder: c.join("pack"),
                        files: keep_publisher
                            .iter()
                            .filter_map(|f| f.file_name())
                            .map(|f| f.to_string_lossy().into_owned())
                            .collect(),
                    });
                }
                let except: Vec<&str> = [
                    keep_data.is_some().then_some("the data folder inside it"),
                    (!keep_publisher.is_empty()).then_some(
                        if keep_publisher.iter().any(|f| !f.ends_with("raters.json")) {
                            if keep_publisher.iter().any(|f| f.ends_with("raters.json")) {
                                "your publisher key and rater book"
                            } else {
                                "your publisher key"
                            }
                        } else {
                            "your rater book"
                        },
                    ),
                ]
                .into_iter()
                .flatten()
                .collect();
                settings_step = Some((
                    if is_link(&c) {
                        format!(
                            "remove the link {} (the settings folder it leads to is left as it is)",
                            c.display()
                        )
                    } else if except.is_empty() {
                        format!("delete the settings folder {}", c.display())
                    } else {
                        format!(
                            "delete the settings folder {}, except {}",
                            c.display(),
                            except.join(" and ")
                        )
                    },
                    Box::new(move || {
                        if !c.exists() && !is_link(&c) {
                            return Ok(Some("already gone".into()));
                        }
                        // A link is removed as a link: nothing behind it is touched, not even what
                        // would be kept (remove_dir_keeping).
                        if is_link(&c) {
                            return remove_link(&c).map(|()| None).map_err(|e| e.to_string());
                        }
                        let mut keep = platform.desktop_files();
                        keep.extend(keep_data.clone());
                        keep.extend(keep_publisher.iter().cloned());
                        let kept = remove_dir_keeping(&c, &keep).map_err(|e| e.to_string())?;
                        // The pictures kept, not the data folder or the publisher's files the
                        // step's name says it keeps.
                        let named: Vec<PathBuf> = keep_data
                            .iter()
                            .chain(&keep_publisher)
                            .map(|k| canonical(k))
                            .collect();
                        let shown: Vec<String> = kept
                            .iter()
                            .filter(|k| !named.contains(k))
                            .map(|k| k.display().to_string())
                            .collect();
                        Ok((!shown.is_empty())
                            .then(|| format!("kept {}: the desktop shows it", shown.join(", "))))
                    }),
                ));
            } else if let Some(record) = Some(c.join("host-program")).filter(|p| p.exists()) {
                steps.push((
                    "forget the recorded host command".into(),
                    Box::new(move || {
                        std::fs::remove_file(&record)
                            .map(|()| None)
                            .map_err(|e| e.to_string())
                    }),
                ));
            }
        }
        let mut data_step: Option<(String, Run)> = None;
        if a.data && is_link(&data) {
            // A link is removed as a link: nothing behind it is read or deleted.
            let d = data.clone();
            data_step = Some((
                format!(
                    "remove the link {} (the data folder it leads to is left as it is)",
                    data.display()
                ),
                Box::new(move || match remove_link(&d) {
                    Ok(()) => Ok(None),
                    Err(_) if std::fs::symlink_metadata(&d).is_err() => {
                        Ok(Some("already gone".into()))
                    }
                    Err(e) => Err(e.to_string()),
                }),
            ));
        } else if a.data && data.exists() {
            let size = human_bytes(folder_size(&data));
            let d = data.clone();
            data_step = Some((
                format!("delete the data folder {} ({size})", data.display()),
                Box::new(move || match std::fs::remove_dir_all(&d) {
                    Ok(()) => Ok(None),
                    // Gone already, with a folder it was in.
                    Err(_) if !d.exists() => Ok(Some("already gone".into())),
                    Err(e) => Err(e.to_string()),
                }),
            ));
        }
        if data_inside {
            steps.extend(data_step.into_iter().chain(settings_step));
        } else {
            steps.extend(settings_step.into_iter().chain(data_step));
        }
        // Before any step runs, so the folders this run deletes are not counted.
        let gone = |p: &Path| std::fs::symlink_metadata(p).is_err();
        let mut already_gone = Vec::new();
        if a.settings
            && let Some(c) = paths.settings.as_deref().filter(|c| gone(c))
        {
            already_gone.push(format!("the settings folder {} (--settings)", c.display()));
        }
        if a.data && gone(&data) {
            already_gone.push(format!("the data folder {} (--data)", data.display()));
        }
        let steps = steps
            .into_iter()
            .map(|(what, run)| {
                if !a.yes {
                    return UndoResult {
                        what,
                        ok: None,
                        error: None,
                        note: None,
                    };
                }
                match run() {
                    Ok(note) => UndoResult {
                        what,
                        ok: Some(true),
                        error: None,
                        note,
                    },
                    Err(e) => UndoResult {
                        what,
                        ok: Some(false),
                        error: Some(e),
                        note: None,
                    },
                }
            })
            .collect();
        let kept_data = (!a.data && data.exists()).then(|| KeptFolder {
            bytes: folder_size(&data),
            path: data,
        });
        let kept_settings = paths.settings.clone().filter(|c| !a.settings && c.exists());
        Ok(UninstallReport {
            done: a.yes,
            steps,
            kept_data,
            kept_settings,
            kept_publisher,
            already_gone,
            output: paths.output,
            installed_with: jetcarp_core::paths::program_path()
                .and_then(|exe| installed_with(&exe, cfg!(windows))),
        })
    }
}
