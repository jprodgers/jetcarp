//! `system.*`: JETCARP's folders, what it sees on this machine, and undoing what it set up.

use crate::ctx::{Ctx, Desktop, GpuAdapter, Paths};
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

/// `JETCARP_*` environment variables, sorted.
fn jetcarp_env() -> Vec<(String, String)> {
    let mut v: Vec<(String, String)> = std::env::vars()
        .filter(|(k, _)| k.starts_with("JETCARP_"))
        .collect();
    v.sort();
    v
}

/// The folders as `jetcarp paths` prints them.
fn paths_text(p: &Paths) -> String {
    format!(
        "settings: {}\ndata:     {}\noutput:   {}\n",
        p.settings
            .as_ref()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| "(none on this system)".into()),
        p.data.display(),
        p.output.display()
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
    const SUMMARY: &'static str =
        "The settings, data, and output folders JETCARP uses (environment overrides included).";
    const READ_ONLY: bool = true;
    type In = NoInput;
    type Out = Paths;
    fn run(cx: &Ctx, _: NoInput, _: &dyn Progress) -> Result<Paths, ApiError> {
        Ok(cx.paths())
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
}

/// What JETCARP sees on this machine.
#[derive(Clone, Debug, Serialize, JsonSchema)]
pub struct DoctorReport {
    pub version: String,
    pub os: String,
    pub arch: String,
    pub threads: usize,
    pub folders: DoctorFolders,
    pub gpu: Vec<GpuAdapter>,
    pub desktop: Desktop,
    /// `JETCARP_*` variables as `[name, value]`.
    pub environment: Vec<(String, String)>,
}

impl Report for DoctorReport {
    fn text(&self) -> String {
        let f = &self.folders;
        let mut s = format!(
            "JETCARP {} on {} {} ({} threads)\n\n",
            self.version, self.os, self.arch, self.threads
        );
        s.push_str("Folders\n");
        let paths = Paths {
            settings: f.settings.clone(),
            data: f.data.clone(),
            output: f.output.clone(),
        };
        for l in paths_text(&paths).lines() {
            let _ = writeln!(s, "  {l}");
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
        let _ = writeln!(s, "  data in use: {}\n", human_bytes(f.data_size_bytes));
        s.push_str("Graphics adapters (* = used by the GPU simulation)\n");
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
        if !self.environment.is_empty() {
            s.push_str("\nEnvironment\n");
            for (k, v) in &self.environment {
                let _ = writeln!(s, "  {k}={v}");
            }
        }
        s
    }
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
        Ok(DoctorReport {
            version: jetcarp_core::VERSION.to_string(),
            os: std::env::consts::OS.to_string(),
            arch: std::env::consts::ARCH.to_string(),
            threads: std::thread::available_parallelism()
                .map(|n| n.get())
                .unwrap_or(1),
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
            },
            gpu: cx.platform.gpu_adapters(),
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
    /// Also delete the settings folder.
    #[serde(default)]
    pub settings: bool,
    /// Also delete the data folder (the catalog, search databases, packs). Cannot be undone.
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
}

/// A folder kept, with its size.
#[derive(Clone, Debug, Serialize, JsonSchema)]
pub struct KeptFolder {
    pub path: PathBuf,
    pub bytes: u64,
}

/// What uninstalling did, or would do.
#[derive(Clone, Debug, Serialize, JsonSchema)]
pub struct UninstallReport {
    /// The steps were run (`yes`), not only listed.
    pub done: bool,
    pub steps: Vec<UndoResult>,
    /// The data folder, when it was kept.
    pub kept_data: Option<KeptFolder>,
    /// The output folder (never deleted).
    pub output: PathBuf,
}

impl Report for UninstallReport {
    fn text(&self) -> String {
        let mut s = String::new();
        if self.steps.is_empty() {
            s.push_str("Nothing to undo: JETCARP has not set anything up outside its folders.\n");
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
        if let Some(k) = &self.kept_data {
            let _ = writeln!(
                s,
                "\nKept the data folder {} ({}); add --data to delete it.",
                k.path.display(),
                human_bytes(k.bytes)
            );
        }
        let _ = writeln!(
            s,
            "Your pictures and exports in {} are never deleted.",
            self.output.display()
        );
        s.push_str(
            "\nTo remove the program itself, use what installed it: `pip uninstall jetcarp`, `pipx \
             uninstall jetcarp`, `uv tool uninstall jetcarp`, `brew uninstall jetcarp`, `cargo \
             uninstall jetcarp`, or delete its folder.\n",
        );
        if self.done
            && self
                .steps
                .iter()
                .any(|st| st.what.starts_with("remove the GNOME"))
        {
            s.push_str("Log out and back in once to finish removing the GNOME extension.\n");
        }
        s
    }
}

/// `system.uninstall`.
pub struct SystemUninstall;
impl Op for SystemUninstall {
    const NAME: &'static str = "system.uninstall";
    const SUMMARY: &'static str = "Undo what JETCARP set up outside its folders (login entry, GNOME extension, screen saver), and optionally delete its settings and data; only lists the steps unless `yes`.";
    const READ_ONLY: bool = false;
    type In = UninstallIn;
    type Out = UninstallReport;
    fn run(cx: &Ctx, a: UninstallIn, _: &dyn Progress) -> Result<UninstallReport, ApiError> {
        type Step = (String, Box<dyn Fn() -> Result<(), String> + Send + Sync>);
        let mut steps: Vec<Step> = cx
            .platform
            .undo_steps()
            .into_iter()
            .map(|s| (s.what, s.run))
            .collect();
        let paths = cx.paths();
        if let Some(c) = paths.settings.clone().filter(|c| c.exists()) {
            if a.settings {
                steps.push((
                    format!("delete the settings folder {}", c.display()),
                    Box::new(move || std::fs::remove_dir_all(&c).map_err(|e| e.to_string())),
                ));
            } else if let Some(record) = Some(c.join("host-program")).filter(|p| p.exists()) {
                steps.push((
                    "forget the recorded host command".into(),
                    Box::new(move || std::fs::remove_file(&record).map_err(|e| e.to_string())),
                ));
            }
        }
        let data = paths.data.clone();
        if a.data && data.exists() {
            let size = human_bytes(folder_size(&data));
            let d = data.clone();
            steps.push((
                format!("delete the data folder {} ({size})", data.display()),
                Box::new(move || std::fs::remove_dir_all(&d).map_err(|e| e.to_string())),
            ));
        }
        let steps = steps
            .into_iter()
            .map(|(what, run)| {
                if !a.yes {
                    return UndoResult {
                        what,
                        ok: None,
                        error: None,
                    };
                }
                let r = run();
                UndoResult {
                    what,
                    ok: Some(r.is_ok()),
                    error: r.err(),
                }
            })
            .collect();
        let kept_data = (!a.data && data.exists()).then(|| KeptFolder {
            bytes: folder_size(&data),
            path: data,
        });
        Ok(UninstallReport {
            done: a.yes,
            steps,
            kept_data,
            output: paths.output,
        })
    }
}
