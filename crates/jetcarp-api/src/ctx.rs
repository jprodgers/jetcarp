//! What an operation runs with: [`Ctx`] holds the folders, the limits of the front end that
//! started it, the file-system policy, the pattern catalog (opened once, on first use), the jobs
//! running in this process ([`Registry`]), and what the program knows about the machine
//! ([`Platform`]: graphics adapters, the GPU simulator, soup hunter, and batch stepper, desktop
//! integration, and how to start a job in a process of its own).

use crate::error::ApiError;
use crate::out::{self, Guard};
use crate::world::Session;
use jetcarp_core::World;
use jetcarp_core::explore::batch::Backend;
use jetcarp_core::hunt::soup::SoupBatcher;
use jetcarp_search::jobs::Registry;
use jetcarp_store::Library;
use schemars::JsonSchema;
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};

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
    /// A bounded soup search run in the call (`search.start` without `background` or
    /// `detach`): most rules it may take, and most tilings.
    pub max_search_rules: usize,
    /// Most soups such a search may run, over all its rules and tilings.
    pub max_search_soups: u64,
    /// Most generations each of its soups may run.
    pub max_search_gens: u64,
    /// Most cell updates it may make, counted as soups x cells x generations (a grid of W x H
    /// unit cells counts as 4 W H cells).
    pub max_search_work: u64,
    /// A collision run in the call: most placements it may try (x offsets x y offsets x phases
    /// of the moving pattern).
    pub max_collide_runs: u64,
    /// Most generations each of its placements may run.
    pub max_collide_gens: u64,
}

impl Limits {
    /// No limits beyond the engine's own (the command line, scripts the user runs, the app).
    pub const NONE: Limits = Limits {
        max_cells: u64::MAX,
        max_grid_side: u32::MAX,
        max_gens: u64::MAX,
        max_work: u64::MAX,
        max_picture: usize::MAX,
        max_set: usize::MAX,
        max_ants: 100_000,
        max_script_seconds: f64::INFINITY,
        max_search_rules: usize::MAX,
        max_search_soups: u64::MAX,
        max_search_gens: u64::MAX,
        max_search_work: u64::MAX,
        max_collide_runs: u64::MAX,
        max_collide_gens: u64::MAX,
    };
    /// The MCP server's limits: each call finishes in seconds to a minute and stays in memory.
    pub const INTERACTIVE: Limits = Limits {
        max_cells: 4_000_000,
        max_grid_side: 4096,
        max_gens: 1_000_000,
        max_work: 2_000_000_000,
        max_picture: 120 * 120,
        max_set: 200_000,
        max_ants: 10_000,
        max_script_seconds: 600.0,
        max_search_rules: 4,
        max_search_soups: 256,
        max_search_gens: 4000,
        max_search_work: 4_000_000_000,
        max_collide_runs: 5000,
        max_collide_gens: 4096,
    };
}

/// Where an operation may read and write files.
///
/// The command line reads and writes wherever it is told (paths are the user's own). Scripts
/// write only inside the output root and the folders the user allowed, and with `restrict_reads`
/// read only there too (the `writable`, `readable`, and `allow_dirs` rules of
/// `jetcarp_script::RunOptions`). The MCP server reads and writes only inside the output folder,
/// the library's folder and the folders its user allowed ([`FsPolicy::who`] names it in errors).
#[derive(Clone, Debug, Default)]
pub struct FsPolicy {
    /// Where relative paths start (default: the working directory).
    pub base: Option<PathBuf>,
    /// Writes must stay inside these folders; `None` allows any path.
    pub write: Option<Guard>,
    /// Reads must stay inside these folders; `None` allows any path.
    pub read: Option<Guard>,
    /// Who the rules are for, in the error a refused path gives (`the MCP tools`): "... may
    /// only write inside ...". `None`: the script's wording.
    pub who: Option<String>,
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
            who: None,
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
            return g.writable(&self.base(), path).map_err(|e| match &self.who {
                Some(who) => ApiError::io(format!(
                    "{who} may only write inside {} (got {})",
                    roots_text(g),
                    out::absolute(&self.base().join(path.trim())).display()
                )),
                None => ApiError::io(e),
            });
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

    /// A folder `path` may be written into: made absolute, checked, and created.
    pub fn writable_dir(&self, path: &str) -> Result<PathBuf, ApiError> {
        let p = self.writable(&format!("{}/.keep", path.trim_end_matches(['/', '\\'])))?;
        let dir = p.parent().map(Path::to_path_buf).unwrap_or(p);
        std::fs::create_dir_all(&dir)
            .map_err(|e| ApiError::io(format!("{}: {e}", dir.display())))?;
        Ok(dir)
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
            return Err(ApiError::io(match &self.who {
                Some(who) => format!(
                    "{who} may only read inside {}, not {}",
                    roots_text(g),
                    p.display()
                ),
                None => format!(
                    "this run may only read inside the output folders, not {}",
                    p.display()
                ),
            }));
        }
        Ok(p)
    }
}

/// A guard's folders, for an error.
fn roots_text(g: &Guard) -> String {
    g.roots
        .iter()
        .map(|r| r.display().to_string())
        .collect::<Vec<_>>()
        .join(" or ")
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
    /// The picture files the desktop shows now. Uninstalling keeps them when it deletes the
    /// settings folder (where the host's stills, and on Windows a copy of the user's own
    /// picture, are kept), so the desktop is never left pointing at a deleted file.
    fn desktop_files(&self) -> Vec<PathBuf> {
        Vec::new()
    }
    /// GPU soup batches for hunts (`jetcarp_gpu::hunt::GpuHunter`).
    fn soup_batcher(&self) -> Result<Arc<dyn SoupBatcher>, String> {
        Err("this program was built without the GPU soup hunter".into())
    }
    /// The GPU batch stepper for explore searches (`jetcarp_gpu::batch::BatchSim`), a new one
    /// for each run.
    fn explore_backend(&self) -> Result<Box<dyn Backend>, String> {
        Err("this program was built without the GPU batch stepper".into())
    }
    /// Run (or resume) job `job` of `catalog` in a new process of its own that outlives this one
    /// (`jetcarp jobs resume JOB`), with these extra arguments (`--threads 4`, `--gpu`,
    /// `--minutes 30`).
    fn spawn_job(&self, _catalog: &Path, _job: i64, _args: &[String]) -> Result<(), String> {
        Err("this program cannot start a job in a process of its own".into())
    }
    /// Can [`spawn_job`](Self::spawn_job) start a process (the `jetcarp` program can)?
    fn can_detach(&self) -> bool {
        false
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
    /// The packs that came with the program, read and never written: `$JETCARP_BUNDLED_PACKS`,
    /// else `<program folder>/../share/jetcarp/packs`, else `<program folder>/packs`; none when
    /// the install has none ([`jetcarp_core::paths::bundled_packs_dir`]).
    pub bundled_packs: Option<PathBuf>,
}

/// What an operation runs with. Cheap to make; the folders are read from the environment when an
/// operation asks for them, so a changed `JETCARP_OUT` is seen at once.
pub struct Ctx {
    pub limits: Limits,
    pub fs: FsPolicy,
    pub platform: Arc<dyn Platform>,
    /// Output root instead of `$JETCARP_OUT` or `<Pictures>/JETCARP`.
    pub out_root: Option<PathBuf>,
    /// Catalog instead of `$JETCARP_LIBRARY` or `<data>/library.sqlite` (a folder means
    /// `<folder>/library.sqlite`). Set it before the first operation that reads the catalog
    /// ([`Ctx::with_library`]): the catalog is opened once.
    pub library: Option<PathBuf>,
    /// Settings folder instead of `$JETCARP_CONFIG_DIR` or the system's (the first-run setup's
    /// record and `paths.json` are read and written there; tests).
    pub settings_dir: Option<PathBuf>,
    /// The folder of packs that came with the program, instead of the one looked for
    /// ([`jetcarp_core::paths::bundled_packs_dir`]; tests).
    pub bundled_dir: Option<PathBuf>,
    /// The catalog, opened on first use and shared by every operation on this context (one
    /// SQLite connection instead of one per call).
    store: OnceLock<Library>,
    /// The world that world operations act on when the caller has none (`call_json`).
    session: Mutex<Option<Session>>,
    /// The jobs running in this process (default: the process's own registry, which the app's
    /// dashboard and the MCP server share).
    registry: &'static Registry,
}

impl Default for Ctx {
    fn default() -> Ctx {
        Ctx::new()
    }
}

impl Ctx {
    /// A trusted caller (the command line): no limits, files anywhere, no GPU. Making one also
    /// connects the engine to the store ([`jetcarp_store::install`]), so a `PastePattern` command
    /// that names a library pattern finds it in the default library, as it always has.
    pub fn new() -> Ctx {
        jetcarp_store::install();
        // Pack imports run as jobs (`pack_import`), which the runner finds by their kind.
        #[cfg(feature = "pack")]
        jetcarp_pack::install();
        Ctx {
            limits: Limits::NONE,
            fs: FsPolicy::open(),
            platform: Arc::new(Headless),
            out_root: None,
            library: None,
            settings_dir: None,
            bundled_dir: None,
            store: OnceLock::new(),
            session: Mutex::new(None),
            registry: jetcarp_search::jobs::registry(),
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
    /// Use this catalog (a `.sqlite` file, or a folder holding `library.sqlite`) instead of
    /// `$JETCARP_LIBRARY` or the default.
    pub fn with_library(mut self, path: Option<PathBuf>) -> Ctx {
        self.library = path;
        self.store = OnceLock::new();
        self
    }
    /// Use this open catalog (its connection is shared with the caller's handle).
    pub fn with_store(self, lib: Library) -> Ctx {
        let mut cx = self.with_library(Some(lib.path().to_path_buf()));
        cx.store = OnceLock::from(lib);
        cx
    }
    /// Read and write settings in this folder instead of `$JETCARP_CONFIG_DIR` or the system's
    /// (so also its `paths.json`, which chooses the data folder). Set it before the first
    /// operation that reads the catalog.
    pub fn with_settings_dir(mut self, dir: Option<PathBuf>) -> Ctx {
        self.settings_dir = dir;
        self.store = OnceLock::new();
        self
    }
    /// Take the packs that came with the program from this folder instead.
    pub fn with_bundled_packs(mut self, dir: Option<PathBuf>) -> Ctx {
        self.bundled_dir = dir;
        self
    }
    /// Write outputs under this root instead of `$JETCARP_OUT` or `<Pictures>/JETCARP`.
    pub fn with_out_root(mut self, root: Option<PathBuf>) -> Ctx {
        self.out_root = root;
        self
    }
    /// Keep the jobs this context runs in the background in this registry instead of the
    /// process's own.
    pub fn with_registry(mut self, r: &'static Registry) -> Ctx {
        self.registry = r;
        self
    }

    /// The jobs running in this process.
    pub fn registry(&self) -> &'static Registry {
        self.registry
    }

    /// The folders, as they are now.
    pub fn paths(&self) -> Paths {
        let settings = self
            .settings_dir
            .clone()
            .or_else(jetcarp_core::paths::config_dir);
        Paths {
            data: jetcarp_core::paths::data_dir_for(settings.as_deref()),
            settings,
            output: self
                .out_root
                .clone()
                .unwrap_or_else(jetcarp_core::paths::output_dir),
            bundled_packs: self
                .bundled_dir
                .clone()
                .or_else(jetcarp_core::paths::bundled_packs_dir),
        }
    }

    /// The catalog's file as it would be opened now: `library`, else `$JETCARP_LIBRARY`, else
    /// `<data>/library.sqlite` in the data folder as it is now (after a `paths.json` change too,
    /// unlike [`library_path`](Self::library_path) once the catalog is open).
    pub fn library_path_now(&self) -> PathBuf {
        match &self.library {
            Some(p) => Library::file_for(p.clone()),
            None => Library::default_path_in(&self.paths().data),
        }
    }

    /// The output root, absolute.
    pub fn output_root(&self) -> PathBuf {
        out::out_root(self.out_root.as_deref())
    }

    /// The catalog's file.
    pub fn library_path(&self) -> PathBuf {
        match self.store.get() {
            Some(l) => l.path().to_path_buf(),
            None => self.library_path_now(),
        }
    }

    /// The catalog: opened on first use (`library`, else `$JETCARP_LIBRARY`, else
    /// `<data>/library.sqlite`), then the same handle every time. Nothing is created until the
    /// first write.
    pub fn library(&self) -> Library {
        self.store
            .get_or_init(|| Library::open(self.library_path_now()))
            .clone()
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
