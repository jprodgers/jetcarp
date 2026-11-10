//! What an operation runs with: [`Ctx`] holds the folders, the limits of the front end that
//! started it, the file-system policy, and what the program knows about the machine
//! ([`Platform`]: graphics adapters, the GPU simulator, desktop integration). The catalog store and
//! the job registry join it when they move here (steps 4 and 5 of the refactor).

use crate::error::ApiError;
use crate::out::{self, Guard};
use crate::world::Session;
use jetcarp_core::World;
use jetcarp_core::library::Library;
use schemars::JsonSchema;
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};

/// Bounds on what one call may do. The command line trusts its user ([`Limits::NONE`]); the MCP
/// server bounds every call so a model cannot stall it ([`Limits::INTERACTIVE`]).
#[derive(Clone, Debug, PartialEq, Serialize, JsonSchema)]
pub struct Limits {
    /// Largest world (cells) a new world, an opened scene, or `set_tiling` may build.
    pub max_cells: u64,
    /// Widest and tallest grid (unit cells) a new world may have.
    pub max_grid_side: u32,
    /// Most generations one call may run.
    pub max_gens: u64,
    /// Most cell updates (cells x generations) one call may run.
    pub max_work: u64,
    /// Most cells a text picture of a field may show.
    pub max_picture: usize,
    /// Most cells one drawing call may write.
    pub max_set: usize,
    /// Most ants an `ants:N` seed may place.
    pub max_ants: u32,
    /// Longest script run, in seconds.
    pub max_script_seconds: f64,
}

impl Limits {
    /// No limits beyond the engine's own (the command line, scripts the user runs).
    pub const NONE: Limits = Limits {
        max_cells: u64::MAX,
        max_grid_side: u32::MAX,
        max_gens: u64::MAX,
        max_work: u64::MAX,
        max_picture: usize::MAX,
        max_set: usize::MAX,
        max_ants: 100_000,
        max_script_seconds: f64::INFINITY,
    };
    /// The MCP server's limits: each call finishes in seconds and stays in memory.
    pub const INTERACTIVE: Limits = Limits {
        max_cells: 4_000_000,
        max_grid_side: 4096,
        max_gens: 1_000_000,
        max_work: 2_000_000_000,
        max_picture: 120 * 120,
        max_set: 200_000,
        max_ants: 10_000,
        max_script_seconds: 600.0,
    };
}

/// Where an operation may read and write files.
///
/// The command line and the MCP server read and write wherever they are told (paths are the
/// user's or the client's own). Scripts write only inside the output root and the folders the
/// user allowed, and with `restrict_reads` read only there too (the `writable`, `readable`, and
/// `allow_dirs` rules of `jetcarp_script::RunOptions`).
#[derive(Clone, Debug, Default)]
pub struct FsPolicy {
    /// Where relative paths start (default: the working directory).
    pub base: Option<PathBuf>,
    /// Writes must stay inside these folders; `None` allows any path.
    pub write: Option<Guard>,
    /// Reads must stay inside these folders; `None` allows any path.
    pub read: Option<Guard>,
}

impl FsPolicy {
    /// Read and write anywhere.
    pub fn open() -> FsPolicy {
        FsPolicy::default()
    }

    /// Write only inside `roots` (relative paths start at `base`); with `restrict_reads`, read only
    /// there too.
    pub fn guarded(base: PathBuf, roots: Vec<PathBuf>, restrict_reads: bool) -> FsPolicy {
        let g = Guard { roots };
        FsPolicy {
            base: Some(base),
            read: restrict_reads.then(|| g.clone()),
            write: Some(g),
        }
    }

    fn base(&self) -> PathBuf {
        self.base
            .clone()
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_default())
    }

    /// A file `path` may be written to: made absolute, checked, and its folder created.
    pub fn writable(&self, path: &str) -> Result<PathBuf, ApiError> {
        if let Some(g) = &self.write {
            return g.writable(&self.base(), path).map_err(ApiError::io);
        }
        let p = path.trim();
        if p.is_empty() {
            return Err(ApiError::invalid("the path is empty"));
        }
        let p = match &self.base {
            Some(b) if !Path::new(p).is_absolute() => b.join(p),
            _ => PathBuf::from(p),
        };
        let path = std::path::absolute(&p)
            .map_err(|e| ApiError::invalid(format!("bad path {}: {e}", p.display())))?;
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| {
                ApiError::io(format!("could not create folder {}: {e}", dir.display()))
            })?;
        }
        Ok(path)
    }

    /// A file `path` may be read from (relative paths start at the base).
    pub fn readable(&self, path: &str) -> Result<PathBuf, ApiError> {
        let p = Path::new(path);
        let p = match &self.base {
            Some(b) if !p.is_absolute() => out::absolute(&b.join(p)),
            _ => p.to_path_buf(),
        };
        if let Some(g) = &self.read
            && !g.allows(&p)
        {
            return Err(ApiError::io(format!(
                "this run may only read inside the output folders, not {}",
                p.display()
            )));
        }
        Ok(p)
    }
}

/// A graphics adapter as `jetcarp doctor` lists it.
#[derive(Clone, Debug, Default, PartialEq, Serialize, JsonSchema)]
pub struct GpuAdapter {
    pub name: String,
    pub backend: String,
    pub kind: String,
    pub driver: String,
    /// The GPU simulation uses this one.
    pub chosen: bool,
    /// Why the simulation does not use it, if it does not.
    pub problem: Option<String>,
}

/// How JETCARP is set up on the desktop.
#[derive(Clone, Debug, Default, PartialEq, Serialize, JsonSchema)]
pub struct Desktop {
    /// The background host (`jetcarp host`) answers.
    pub host_running: bool,
    /// The host starts at login.
    pub starts_at_login: bool,
    /// The GNOME Shell extension, in words (Linux only).
    pub gnome_extension: Option<String>,
    /// The screen saver, in words (Windows and macOS only).
    pub screen_saver: Option<String>,
}

/// One thing `system.uninstall` undoes outside JETCARP's own folders.
pub struct UndoStep {
    /// What it does, in words ("remove the GNOME Shell extension").
    pub what: String,
    pub run: Box<dyn Fn() -> Result<(), String> + Send + Sync>,
}

/// How a GPU stepped a world: how many generations fell back to the CPU, and why.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct GpuSteps {
    pub cpu_gens: u64,
    pub fallback: Option<String>,
}

/// A GPU simulator (`jetcarp_gpu::GpuSim`).
pub trait GpuStepper {
    /// The adapter, for reports.
    fn name(&self) -> String;
    /// Step `gens` generations, on the GPU where the rules allow it.
    fn step_mixed(&mut self, w: &mut World, gens: u64) -> Result<GpuSteps, String>;
}

/// What the program knows about the machine that this crate cannot reach itself: the GPU crate
/// and the desktop integration sit above it. The `jetcarp` binary provides one; everything else
/// (tests, the MCP server inside the app, scripts) gets [`Headless`].
pub trait Platform: Send + Sync {
    /// Graphics adapters (none known: the simulation runs on the CPU).
    fn gpu_adapters(&self) -> Vec<GpuAdapter> {
        Vec::new()
    }
    /// A GPU simulator.
    fn gpu_stepper(&self) -> Result<Box<dyn GpuStepper>, String> {
        Err("this program was built without the GPU simulator".into())
    }
    /// The desktop integration's state.
    fn desktop(&self) -> Desktop {
        Desktop::default()
    }
    /// What uninstalling undoes outside JETCARP's folders, in order.
    fn undo_steps(&self) -> Vec<UndoStep> {
        Vec::new()
    }
}

/// No GPU, no desktop integration.
pub struct Headless;
impl Platform for Headless {}

/// JETCARP's folders.
#[derive(Clone, Debug, PartialEq, Serialize, JsonSchema)]
pub struct Paths {
    /// Settings: `$JETCARP_CONFIG_DIR`, else `<config>/jetcarp` (none on some systems).
    pub settings: Option<PathBuf>,
    /// Data: the catalog, search databases, packs (`$JETCARP_DATA_DIR`).
    pub data: PathBuf,
    /// Output: pictures, sweeps, media (`$JETCARP_OUT`, else `<Pictures>/JETCARP`).
    pub output: PathBuf,
}

/// What an operation runs with. Cheap to make; the folders are read from the environment when an
/// operation asks for them, so a changed `JETCARP_OUT` is seen at once.
pub struct Ctx {
    pub limits: Limits,
    pub fs: FsPolicy,
    pub platform: Arc<dyn Platform>,
    /// Output root instead of `$JETCARP_OUT` or `<Pictures>/JETCARP`.
    pub out_root: Option<PathBuf>,
    /// Catalog instead of `$JETCARP_LIBRARY` or `<data>/catalog.sqlite` (a folder means
    /// `<folder>/catalog.sqlite`).
    pub library: Option<PathBuf>,
    /// The world that world operations act on when the caller has none (`call_json`).
    session: Mutex<Option<Session>>,
}

impl Default for Ctx {
    fn default() -> Ctx {
        Ctx::new()
    }
}

impl Ctx {
    /// A trusted caller (the command line): no limits, files anywhere, no GPU.
    pub fn new() -> Ctx {
        Ctx {
            limits: Limits::NONE,
            fs: FsPolicy::open(),
            platform: Arc::new(Headless),
            out_root: None,
            library: None,
            session: Mutex::new(None),
        }
    }

    /// An interactive caller (the MCP server): [`Limits::INTERACTIVE`].
    pub fn interactive() -> Ctx {
        Ctx {
            limits: Limits::INTERACTIVE,
            ..Ctx::new()
        }
    }

    pub fn with_limits(mut self, limits: Limits) -> Ctx {
        self.limits = limits;
        self
    }
    pub fn with_fs(mut self, fs: FsPolicy) -> Ctx {
        self.fs = fs;
        self
    }
    pub fn with_platform(mut self, p: Arc<dyn Platform>) -> Ctx {
        self.platform = p;
        self
    }

    /// The folders, as they are now.
    pub fn paths(&self) -> Paths {
        Paths {
            settings: jetcarp_core::paths::config_dir(),
            data: jetcarp_core::paths::data_dir(),
            output: self
                .out_root
                .clone()
                .unwrap_or_else(jetcarp_core::paths::output_dir),
        }
    }

    /// The output root, absolute.
    pub fn output_root(&self) -> PathBuf {
        out::out_root(self.out_root.as_deref())
    }

    /// The catalog's file.
    pub fn library_path(&self) -> PathBuf {
        match &self.library {
            Some(p) => Library::file_for(p.clone()),
            None => Library::default_path(),
        }
    }

    /// The catalog (nothing is created until the first write).
    pub fn library(&self) -> Library {
        match &self.library {
            Some(p) => Library::open(p.clone()),
            None => Library::open_default(),
        }
    }

    /// The world operations act on when no host is given: a private headless world, made on
    /// first use and kept for the life of this context.
    pub fn session(&self) -> MutexGuard<'_, Option<Session>> {
        let mut g = self.session.lock().unwrap_or_else(|e| e.into_inner());
        if g.is_none() {
            *g = Some(Session::new());
        }
        g
    }
}
