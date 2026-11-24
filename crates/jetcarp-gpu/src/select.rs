//! Which graphics adapter JETCARP uses, and whether the GPU simulation may run on it.
//!
//! Every place that opens a GPU asks here: the command line's simulators ([`crate::GpuSim`],
//! [`crate::batch::BatchSim`], [`crate::hunt::GpuHunter`], [`crate::pile::PileGpu`]), the app's
//! window, the background host's surfaces and the screen savers. So they all choose the same way.
//!
//! - **The policy** ([`Policy::load`]): `JETCARP_GPU` when it is set, else the saved setting in the
//!   settings folder's `gpu.json` ([`Setting`]), else automatic. It says whether to use the GPU
//!   (`off`, `auto`, `on`) and which adapter (`software`, `integrated`, `discrete`, or part of an
//!   adapter's name). The app and the background host each have a choice of their own ([`Own`]:
//!   follow the shared setting, on, or off), which [`Policy::load_for`] puts between the variable
//!   and the shared setting. In all, first to last: `JETCARP_GPU`, a command's own `--gpu` or
//!   `--cpu` ([`Policy::uses`]), the program's own choice, the shared setting, automatic.
//! - **The purpose** ([`Purpose`]): computing prefers the fastest adapter; a display prefers the one
//!   driving the screen, so a laptop's discrete GPU can sleep under a wallpaper.
//! - **Decent** ([`requirements`]): a discrete or integrated GPU with compute shaders, at least
//!   [`MIN_STORAGE_BUFFERS`] storage buffers per shader stage and storage bindings of at least
//!   [`MIN_STORAGE_BINDING`] bytes, which passes the self-test ([`self_test`]). Software adapters
//!   (llvmpipe, lavapipe, SwiftShader) are used only when asked for; Windows' WARP never is (its
//!   shader compiler crashes on the world stepper and the soup hunt, #321) unless the tests allow
//!   it ([`warp_allowed`]).
//!
//! The self-test runs every world kernel and the explore batch stepper on a small lattice and an
//! aperiodic patch, on the GPU and on the CPU, and compares the hashes. The result is kept in the
//! settings folder's `gpu-checks.json` for that adapter, driver, JETCARP version and shader set.

use crate::batch::BatchSim;
use crate::{GpuSim, describe};
use jetcarp_core::explore::batch::{BatchJob, BatchWorld, run_cpu};
use jetcarp_core::explore::space::{ALL_FAMS, Grid, sample_rule};
use jetcarp_core::rng::Rng;
use jetcarp_core::rules::Rule;
use jetcarp_core::topology::Nb;
use jetcarp_core::{CaSpec, Command, SeedSpec, World};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

/// The environment variable that overrides the saved setting.
pub const ENV: &str = "JETCARP_GPU";
/// The saved setting, in the settings folder.
pub const SETTING_FILE: &str = "gpu.json";
/// Self-test results, in the settings folder.
pub const CHECKS_FILE: &str = "gpu-checks.json";
/// Storage buffers per shader stage a decent adapter has (the batch stepper binds this many).
pub const MIN_STORAGE_BUFFERS: u32 = 9;
/// Bytes a decent adapter's storage bindings may hold (128 MiB).
pub const MIN_STORAGE_BINDING: u64 = 128 << 20;

/// Whether to use the GPU.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    /// Never, unless a command asks for it (`--gpu`).
    Off,
    /// Whenever a decent GPU is present and the work is large enough to gain from it.
    #[default]
    Auto,
    /// Whenever a decent GPU is present, however small the work.
    On,
}

impl Mode {
    pub fn name(self) -> &'static str {
        match self {
            Mode::Off => "off",
            Mode::Auto => "auto",
            Mode::On => "on",
        }
    }

    /// `off`, `auto` or `on` (and `0`, `1`, `no`, `yes`, `false`, `true`).
    pub fn parse(s: &str) -> Option<Mode> {
        Some(match s.trim().to_ascii_lowercase().as_str() {
            "0" | "off" | "no" | "false" | "cpu" => Mode::Off,
            "" | "auto" => Mode::Auto,
            "1" | "on" | "yes" | "true" | "gpu" => Mode::On,
            _ => return None,
        })
    }
}

/// Which adapter to use.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum Want {
    /// The best decent adapter for the purpose.
    #[default]
    Best,
    /// A software adapter (llvmpipe, lavapipe, SwiftShader): slow, for tests.
    Software,
    Integrated,
    Discrete,
    /// The adapter whose name contains this (any case): `UHD`, `RTX`, `llvmpipe`.
    Named(String),
}

impl Want {
    pub fn parse(s: &str) -> Want {
        let t = s.trim();
        match t.to_ascii_lowercase().as_str() {
            "" | "auto" | "best" => Want::Best,
            "software" => Want::Software,
            "integrated" => Want::Integrated,
            "discrete" => Want::Discrete,
            _ => Want::Named(t.to_string()),
        }
    }

    /// As the setting or the variable spells it (`None` for the best).
    pub fn text(&self) -> Option<String> {
        match self {
            Want::Best => None,
            Want::Software => Some("software".into()),
            Want::Integrated => Some("integrated".into()),
            Want::Discrete => Some("discrete".into()),
            Want::Named(n) => Some(n.clone()),
        }
    }

    fn matches(&self, info: &wgpu::AdapterInfo) -> bool {
        use wgpu::DeviceType as D;
        match self {
            Want::Best => !matches!(info.device_type, D::Cpu),
            Want::Software => info.device_type == D::Cpu,
            Want::Integrated => info.device_type == D::IntegratedGpu,
            Want::Discrete => info.device_type == D::DiscreteGpu,
            Want::Named(n) => info
                .name
                .to_ascii_lowercase()
                .contains(&n.to_ascii_lowercase()),
        }
    }
}

/// Where the policy came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    /// `JETCARP_GPU`.
    Env,
    /// A program's own choice ([`Own`]): the app's or the background host's.
    Own,
    /// The settings folder's `gpu.json`.
    Setting,
    /// Neither: automatic.
    Default,
}

impl Source {
    pub fn name(self) -> &'static str {
        match self {
            Source::Env => "env",
            Source::Own => "own",
            Source::Setting => "setting",
            Source::Default => "auto",
        }
    }
}

/// A program's own GPU choice, over the shared setting: the app's (in `app.json`) and the
/// background host's (in `host.json`). Following is the default, so one setting serves both.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Own {
    /// Whatever the shared setting says.
    #[default]
    Follow,
    /// Step on the GPU whenever the rules allow, whatever the shared setting says.
    On,
    /// Step on the processor, whatever the shared setting says.
    Off,
}

impl Own {
    pub const ALL: [Own; 3] = [Own::Follow, Own::On, Own::Off];

    pub fn name(self) -> &'static str {
        match self {
            Own::Follow => "follow",
            Own::On => "on",
            Own::Off => "off",
        }
    }

    /// `follow`, `on`, `off` (also `auto`, meaning follow, and `true` and `false`).
    pub fn parse(s: &str) -> Option<Own> {
        Some(match s.trim().to_ascii_lowercase().as_str() {
            "follow" | "auto" | "shared" | "" => Own::Follow,
            "on" | "true" | "1" | "yes" | "gpu" => Own::On,
            "off" | "false" | "0" | "no" | "cpu" => Own::Off,
            _ => return None,
        })
    }

    /// Reads a stored choice, and the booleans files before it held (`true`: follow, the setting
    /// it then meant; `false`: off).
    pub fn deserialize_lenient<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Own, D::Error> {
        use serde::de::Error;
        match serde_json::Value::deserialize(d)? {
            serde_json::Value::Bool(true) => Ok(Own::Follow),
            serde_json::Value::Bool(false) => Ok(Own::Off),
            serde_json::Value::String(t) => {
                Own::parse(&t).ok_or_else(|| D::Error::custom(format!("gpu: {t:?}")))
            }
            serde_json::Value::Null => Ok(Own::Follow),
            other => Err(D::Error::custom(format!("gpu: {other}"))),
        }
    }
}

/// The mode `JETCARP_GPU` sets, when it names one (`off`, `auto`, `on` and their synonyms; an
/// adapter's name leaves the mode to the rest).
pub fn env_mode() -> Option<Mode> {
    std::env::var(ENV)
        .ok()
        .filter(|v| !v.trim().is_empty())
        .and_then(|v| Mode::parse(&v))
}

/// What a caller does, from the policy and its own request (`--gpu`, `--cpu`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Use {
    Never,
    /// When the work is large enough ([`crate::GpuSim::worth`]).
    Auto,
    Always,
}

/// Whether and where to use the GPU.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Policy {
    pub mode: Mode,
    pub want: Want,
    pub source: Source,
    /// The variable as it was set (for reports).
    pub env: Option<String>,
    /// A setting file that could not be read (the policy is then automatic).
    pub note: Option<String>,
    /// With [`Source::Own`]: whose choice it is (`the app`, `the desktop background`).
    pub owner: Option<&'static str>,
}

impl Default for Policy {
    fn default() -> Self {
        Policy {
            mode: Mode::Auto,
            want: Want::Best,
            source: Source::Default,
            env: None,
            note: None,
            owner: None,
        }
    }
}

impl Policy {
    /// `JETCARP_GPU` when it is set, else the saved setting, else automatic.
    pub fn load() -> Policy {
        match std::env::var(ENV) {
            Ok(v) if !v.trim().is_empty() => Policy::from_env(&v),
            _ => match Setting::load() {
                Ok(Some(s)) => Policy::from_setting(&s),
                Ok(None) => Policy::default(),
                Err(e) => Policy {
                    note: Some(e),
                    ..Policy::default()
                },
            },
        }
    }

    /// The policy `JETCARP_GPU=value` sets: `off`, `auto`, `on` (also `0` and `1`), or which
    /// adapter to use automatically (`software`, `integrated`, `discrete`, part of a name).
    pub fn from_env(value: &str) -> Policy {
        let (mode, want) = match Mode::parse(value) {
            Some(m) => (m, Want::Best),
            None => (Mode::Auto, Want::parse(value)),
        };
        Policy {
            mode,
            want,
            source: Source::Env,
            env: Some(value.trim().to_string()),
            note: None,
            owner: None,
        }
    }

    /// The policy for a program whose own choice is `own` ([`Own`]), called `owner` in reports:
    /// `JETCARP_GPU` when it names a mode, else that choice, else the shared setting, else
    /// automatic. The adapter is always the variable's or the shared setting's.
    pub fn load_for(own: Own, owner: &'static str) -> Policy {
        Policy::load().with_own(own, owner)
    }

    /// This policy (the variable's or the shared setting's) under a program's own choice.
    pub fn with_own(self, own: Own, owner: &'static str) -> Policy {
        if self.env_sets_mode() {
            return self;
        }
        let mode = match own {
            Own::Follow => return self,
            Own::On => Mode::On,
            Own::Off => Mode::Off,
        };
        Policy {
            mode,
            source: Source::Own,
            owner: Some(owner),
            env: None,
            ..self
        }
    }

    /// Whether `JETCARP_GPU` set this policy's mode (and not just an adapter).
    pub fn env_sets_mode(&self) -> bool {
        self.source == Source::Env && self.env.as_deref().and_then(Mode::parse).is_some()
    }

    pub fn from_setting(s: &Setting) -> Policy {
        Policy {
            mode: s.gpu,
            want: s
                .gpu_adapter
                .as_deref()
                .map(Want::parse)
                .unwrap_or_default(),
            source: Source::Setting,
            env: None,
            note: None,
            owner: None,
        }
    }

    /// What to do, given a command's own request: `Some(true)` (`--gpu`) and `Some(false)`
    /// (`--cpu`) win over the policy, unless `JETCARP_GPU` set its mode.
    pub fn uses(&self, asked: Option<bool>) -> Use {
        let asked = asked.filter(|_| !self.env_sets_mode());
        match (asked, self.mode) {
            (Some(true), _) | (None, Mode::On) => Use::Always,
            (Some(false), _) | (None, Mode::Off) => Use::Never,
            (None, Mode::Auto) => Use::Auto,
        }
    }

    /// Where the policy came from, in words: `JETCARP_GPU=UHD`, `the setting in gpu.json`,
    /// `automatic`.
    pub fn origin(&self) -> String {
        match self.source {
            Source::Env => format!("{ENV}={}", self.env.as_deref().unwrap_or("")),
            Source::Own => format!("{}'s own setting", self.owner.unwrap_or("the program")),
            Source::Setting => format!("the setting in {SETTING_FILE}"),
            Source::Default => "automatic".into(),
        }
    }

    /// `auto, the integrated adapter (JETCARP_GPU=integrated)`.
    pub fn describe(&self) -> String {
        let which = match &self.want {
            Want::Best => String::new(),
            Want::Software => ", a software adapter".into(),
            Want::Integrated => ", the integrated adapter".into(),
            Want::Discrete => ", the discrete adapter".into(),
            Want::Named(n) => format!(", the adapter named {n}"),
        };
        format!("{}{which} ({})", self.mode.name(), self.origin())
    }
}

/// The saved GPU setting, shared by the app, the host, the command line and the MCP server:
/// `<settings folder>/gpu.json`, for example `{"gpu": "auto", "gpu_adapter": "UHD"}`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Setting {
    /// `off`, `auto` or `on`.
    pub gpu: Mode,
    /// Which adapter: `software`, `integrated`, `discrete`, or part of its name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gpu_adapter: Option<String>,
}

impl Setting {
    /// `<settings folder>/gpu.json` (none when the system has no settings folder).
    pub fn file() -> Option<PathBuf> {
        jetcarp_core::paths::config_dir().map(|d| d.join(SETTING_FILE))
    }

    /// The saved setting; `None` when there is none, `Err` when the file cannot be read.
    pub fn load() -> Result<Option<Setting>, String> {
        let Some(path) = Setting::file() else {
            return Ok(None);
        };
        let text = match std::fs::read_to_string(&path) {
            Ok(t) => t,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => {
                return Err(format!(
                    "{}: {e}; the GPU choice is automatic",
                    path.display()
                ));
            }
        };
        serde_json::from_str(&text)
            .map(Some)
            .map_err(|e| format!("{}: {e}; the GPU choice is automatic", path.display()))
    }

    /// Writes it (whole, then moved into place). Returns the file.
    pub fn save(&self) -> Result<PathBuf, String> {
        let path = Setting::file().ok_or("no settings folder on this system")?;
        let text = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        write_whole(&path, &text)?;
        Ok(path)
    }
}

fn write_whole(path: &std::path::Path, text: &str) -> Result<(), String> {
    let dir = path.parent().ok_or("no folder")?;
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let tmp = path.with_extension(format!("tmp{}", std::process::id()));
    std::fs::write(&tmp, text).map_err(|e| format!("{}: {e}", tmp.display()))?;
    std::fs::rename(&tmp, path).map_err(|e| format!("{}: {e}", path.display()))
}

/// What an adapter is for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Purpose {
    /// A window, wallpaper, screen saver or picture: the adapter driving the screen.
    Display,
    /// Searches, runs and scripts: the fastest adapter.
    Compute,
}

impl Purpose {
    pub fn name(self) -> &'static str {
        match self {
            Purpose::Display => "display",
            Purpose::Compute => "compute",
        }
    }
}

/// An adapter chosen for a purpose.
#[derive(Clone, Debug)]
pub struct Choice {
    pub adapter: wgpu::Adapter,
    pub info: wgpu::AdapterInfo,
    pub purpose: Purpose,
    /// How it was chosen, in words.
    pub why: String,
    /// Whether the GPU simulation may run on it, and if not, why. A display choice is checked
    /// against the [`requirements`] only; [`verify`] adds the self-test.
    pub usable: Result<(), String>,
    /// The self-test, when it ran for this choice.
    pub test: Option<SelfTest>,
    /// The adapter asked for when it was not there, so another was chosen ("no RTX adapter, so
    /// the best one"): shown wherever the adapter is named, not only by `doctor` (#21).
    pub fallback: Option<String>,
}

impl Choice {
    /// The adapter as people see it named: [`describe`](crate::describe), and the adapter asked
    /// for when it was not found.
    pub fn label(&self) -> String {
        match &self.fallback {
            Some(f) => format!("{}; {f}", crate::describe(&self.info)),
            None => crate::describe(&self.info),
        }
    }
}

/// The graphics backends JETCARP opens for `want`: wgpu's own `WGPU_BACKEND` when it is set,
/// else all of them, except OpenGL on Windows unless `want` names an adapter (#315).
///
/// On Windows every adapter worth using is reached through DirectX 12 or Vulkan, which the
/// choice ranks before OpenGL anyway; but opening OpenGL loads the graphics card's OpenGL driver
/// and makes it a context, a hidden window and a thread, in every program that opens the GPU,
/// even one that then uses DirectX 12. An adapter named by `JETCARP_GPU` or the saved setting
/// may be one only OpenGL reaches, so a name still looks there too; the same card on DirectX 12
/// or Vulkan is still taken first. Linux and macOS open every backend, as before: on Linux,
/// OpenGL ES is what draws where there is no Vulkan.
pub fn backends(want: &Want) -> wgpu::Backends {
    if let Some(b) = wgpu::Backends::from_env() {
        return b;
    }
    let all = wgpu::Backends::all();
    if cfg!(windows) && !matches!(want, Want::Named(_)) {
        all - wgpu::Backends::GL
    } else {
        all
    }
}

/// An instance's options under the saved policy: [`backends`], and wgpu's other variables.
pub fn instance_descriptor() -> wgpu::InstanceDescriptor {
    let mut d = wgpu::InstanceDescriptor::new_without_display_handle_from_env();
    d.backends = backends(&Policy::load().want);
    d
}

/// The instances this process keeps to its end: the first it made of each set of backends.
static KEPT: Mutex<Vec<(wgpu::Backends, wgpu::Instance)>> = Mutex::new(Vec::new());
/// Whether this process has opened the GPU ([`opened`]).
static OPENED: AtomicBool = AtomicBool::new(false);

/// A new wgpu instance under the saved policy ([`instance_descriptor`]), for everything in
/// JETCARP that opens the GPU, but the app's window (eframe's own).
///
/// The process keeps the first of each set of backends to its end (#315). Destroying a
/// process's last instance unloads the graphics drivers (the Vulkan loader frees them, and so
/// does OpenGL), while a driver may still have threads of its own running, which then run code
/// that is no longer there: `jetcarp jobs refit` ended so on the Acer (0xC0000005 in NVIDIA's
/// `nvoglv64.dll`, its Vulkan driver as well as its OpenGL one, once it had been unloaded), as
/// the app did before it ended through `jetcarp_display::exit::end_process`. With one instance
/// kept, the drivers stay loaded as long as the process runs, and a program that has opened the
/// GPU ends without unloading them ([`opened`]). The instances made after it are each the
/// caller's own, freed when it is done with them, so each lists the adapters anew.
pub fn new_instance() -> wgpu::Instance {
    let d = instance_descriptor();
    let backends = d.backends;
    let instance = wgpu::Instance::new(d);
    OPENED.store(true, Ordering::SeqCst);
    let mut kept = KEPT.lock().unwrap_or_else(|e| e.into_inner());
    if !kept.iter().any(|(b, _)| *b == backends) {
        kept.push((backends, instance.clone()));
    }
    instance
}

/// Whether this process has opened the GPU ([`new_instance`], or [`opening`] for an instance
/// made elsewhere): such a program ends through `jetcarp_display::exit::end_process`, which
/// neither destroys its instances nor runs the code its libraries leave to run at exit.
pub fn opened() -> bool {
    OPENED.load(Ordering::SeqCst)
}

/// Says this process opens the GPU through an instance [`new_instance`] did not make (the app's
/// window, whose instance eframe makes).
pub fn opening() {
    OPENED.store(true, Ordering::SeqCst);
}

/// Choose an adapter of `instance` for `purpose` under the saved policy. A display choice needs
/// the `surface` it will draw on, when there is one.
pub fn pick(
    instance: &wgpu::Instance,
    purpose: Purpose,
    surface: Option<&wgpu::Surface<'_>>,
) -> Result<Choice, String> {
    let policy = Policy::load();
    let adapters = adapters_for(instance, &policy.want);
    let choice = pick_from(&adapters, &policy, purpose, surface)?;
    if let Some(f) = &choice.fallback {
        say_fallback_once(f);
    }
    Ok(choice)
}

/// How many more times to look, and how far apart, for an adapter the policy asks for before the
/// choice falls back (#82).
const LOOK_AGAIN: (u32, std::time::Duration) = (2, std::time::Duration::from_millis(250));

/// Every adapter of `instance`. When the policy asks for a particular adapter (by name, or the
/// integrated, discrete or software one) and none is there, it looks twice more, a quarter of a
/// second apart, before the choice falls back to another. On a laptop whose discrete GPU sleeps
/// when idle (Optimus), a DirectX 12 enumeration made while it wakes up now and then leaves it
/// out: one process in 335 ran on the integrated GPU though `JETCARP_GPU` named the other (#82).
pub fn adapters_for(instance: &wgpu::Instance, want: &Want) -> Vec<wgpu::Adapter> {
    look_again(
        || pollster::block_on(instance.enumerate_adapters(wgpu::Backends::all())),
        |list: &[wgpu::Adapter]| has_wanted(want, list.iter().map(|a| a.get_info())),
        LOOK_AGAIN.0,
        LOOK_AGAIN.1,
    )
}

/// Whether an adapter `want` asks for is among these (WARP never counts, as in the choice).
fn has_wanted(want: &Want, mut infos: impl Iterator<Item = wgpu::AdapterInfo>) -> bool {
    match want {
        Want::Best => true,
        w => infos.any(|i| w.matches(&i) && !warp_refused(&i)),
    }
}

/// `list()`, then up to `more` times again, `wait` apart, until `found` says it holds what is
/// wanted; the last list either way.
fn look_again<A>(
    mut list: impl FnMut() -> Vec<A>,
    found: impl Fn(&[A]) -> bool,
    more: u32,
    wait: std::time::Duration,
) -> Vec<A> {
    let mut l = list();
    for _ in 0..more {
        if found(&l) {
            break;
        }
        std::thread::sleep(wait);
        l = list();
    }
    l
}

/// The fallback said on the error stream too, once a process, so a script that relies on
/// `JETCARP_GPU` sees it without reading the backend note (#82).
fn say_fallback_once(f: &str) {
    static SAID: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    if !SAID.swap(true, std::sync::atomic::Ordering::Relaxed) {
        eprintln!("note: GPU: {f}");
    }
}

/// [`pick`] among these adapters (the app's window hands its own list over).
///
/// A compute choice is usable or an error: it runs the self-test and moves on to the next adapter
/// when one fails. A display choice always gives an adapter that can draw on the surface (a
/// software one when nothing else can); `usable` says whether the simulation may run on it.
pub fn pick_from(
    adapters: &[wgpu::Adapter],
    policy: &Policy,
    purpose: Purpose,
    surface: Option<&wgpu::Surface<'_>>,
) -> Result<Choice, String> {
    pick_tested(adapters, policy, purpose, surface, true)
}

/// [`pick_from`], with the compute adapter's self-test only when `test`.
pub(crate) fn pick_tested(
    adapters: &[wgpu::Adapter],
    policy: &Policy,
    purpose: Purpose,
    surface: Option<&wgpu::Surface<'_>>,
    test: bool,
) -> Result<Choice, String> {
    let mut seen = std::collections::HashSet::new();
    let all: Vec<&wgpu::Adapter> = adapters
        .iter()
        .filter(|a| {
            let i = a.get_info();
            seen.insert((i.name, i.backend, i.driver, i.driver_info))
        })
        .filter(|a| surface.is_none_or(|s| a.is_surface_supported(s)))
        .collect();
    if all.is_empty() {
        return Err(match surface {
            Some(_) => "no graphics adapter can draw on this window".into(),
            None => "no graphics adapter".into(),
        });
    }
    let (want, fallback) = match &policy.want {
        Want::Best => (Want::Best, None),
        w if all
            .iter()
            .any(|a| w.matches(&a.get_info()) && !warp_refused(&a.get_info())) =>
        {
            (w.clone(), None)
        }
        w => (
            Want::Best,
            Some(format!(
                "no {} adapter was found, so the best one",
                w.text().unwrap_or_default()
            )),
        ),
    };
    let mut ranked: Vec<&wgpu::Adapter> = all
        .iter()
        .copied()
        .filter(|a| want.matches(&a.get_info()))
        .collect();
    ranked.sort_by_key(|a| rank(&a.get_info(), purpose));
    let why = {
        let how = match (&want, purpose) {
            (Want::Best, Purpose::Compute) => "the fastest adapter".to_string(),
            (Want::Best, Purpose::Display) => "the adapter driving the screen".to_string(),
            (Want::Software, _) => "a software adapter, as asked".into(),
            (Want::Integrated, _) => "the integrated adapter, as asked".into(),
            (Want::Discrete, _) => "the discrete adapter, as asked".into(),
            (Want::Named(n), _) => format!("named {n}, as asked"),
        };
        let mut s = format!("{how} ({})", policy.origin());
        if let Some(f) = &fallback {
            s = format!("{s}; {f}");
        }
        s
    };
    let mut refused: Vec<String> = Vec::new();
    for a in &ranked {
        let info = a.get_info();
        if let Err(e) = requirements(&info, &a.limits(), &a.get_downlevel_capabilities(), &want) {
            refused.push(format!("{}: {e}", info.name));
            continue;
        }
        let mut choice = Choice {
            adapter: (*a).clone(),
            why: why.clone(),
            info,
            purpose,
            usable: Ok(()),
            test: None,
            fallback: fallback.clone(),
        };
        if purpose == Purpose::Compute && test {
            let t = self_test(a);
            if !t.passed {
                refused.push(format!("{}: {}", choice.info.name, t.note));
                continue;
            }
            choice.test = Some(t);
        }
        return Ok(choice);
    }
    let reason = if refused.is_empty() {
        // The adapters there were, none of them taken.
        let found = all
            .iter()
            .map(|a| a.get_info().name)
            .collect::<Vec<_>>()
            .join(", ");
        if matches!(policy.want, Want::Software) {
            if all.iter().any(|a| warp_refused(&a.get_info())) {
                "no hardware graphics adapter, and the only software one is Windows' WARP, which \
                 the GPU simulation never uses"
                    .to_string()
            } else {
                format!("no graphics adapter the GPU simulation can use (found {found})")
            }
        } else {
            format!(
                "no hardware graphics adapter (found {found}; software ones are used only with \
                 JETCARP_GPU=software)"
            )
        }
    } else {
        refused.join("; ")
    };
    match purpose {
        Purpose::Compute => Err(reason),
        Purpose::Display => {
            // Something has to draw: the best of whatever can.
            let mut any = all.clone();
            any.sort_by_key(|a| rank(&a.get_info(), purpose));
            let a = any[0];
            let info = a.get_info();
            Ok(Choice {
                adapter: a.clone(),
                why: format!("the only adapter that can draw here ({})", policy.origin()),
                info,
                purpose,
                usable: Err(reason),
                test: None,
                fallback: fallback.clone(),
            })
        }
    }
}

/// Sort key: kinds in the purpose's order, then the primary backends before OpenGL.
fn rank(info: &wgpu::AdapterInfo, purpose: Purpose) -> (u8, u8) {
    use wgpu::DeviceType as D;
    let kind = match (info.device_type, purpose) {
        (D::DiscreteGpu, Purpose::Compute) | (D::IntegratedGpu, Purpose::Display) => 0,
        (D::IntegratedGpu, Purpose::Compute) | (D::DiscreteGpu, Purpose::Display) => 1,
        (D::VirtualGpu, _) => 2,
        (D::Other, _) => 3,
        (D::Cpu, _) => 4,
    };
    let backend = match info.backend {
        wgpu::Backend::Vulkan | wgpu::Backend::Metal | wgpu::Backend::Dx12 => 0,
        wgpu::Backend::Gl => 1,
        _ => 2,
    };
    (kind, backend)
}

/// Windows' software renderer, which the GPU simulation never uses (unless the tests allow it:
/// [`warp_allowed`]).
///
/// The WARP that comes with Windows crashes as it compiles the world stepper's `step` and
/// `contact` kernels, and the soup hunt's (#321): 0xC0000005 in `d3d10warp.dll`'s
/// `ComputeShaderTransformer::ConstructLoadStoreSets`, on a thread of WARP's own that compiles a
/// shader for its first dispatch, so the process ends before the self-test can turn WARP down.
/// It crashed so on the Acer (Windows 11 24H2, WARP 10.0.26100.9818) and on CI's Windows Server
/// 2025 (10.0.26100.33648), where a debugger showed where. That compiler fails on a compute
/// shader, as FXC compiles it, with a workgroup barrier after an early return or in an `if` that
/// ends the shader: each of those kernels has one, in uniform control flow as WGSL requires, and a
/// shader of a few lines (an atomic, then a barrier in a closing `if`) crashes it too. The same
/// kernels compiled by DXC run on the same WARP and match the processor bit for bit, and WARP
/// 1.0.22 (Microsoft's NuGet package) runs FXC's world stepper and explore batches so too: the
/// fault is WARP's compiler's, not the kernels'. Windows CI runs the GPU tests on WARP 1.0.22
/// (`.github/workflows/ci.yml`), all but the soup hunt's, which that WARP compiles too slowly and,
/// for Generations rules, wrongly (`hunt::NOT_ON_WARP`).
///
/// WARP is not supported for users for now. In 0.4.4 the kernels get their barriers at the top
/// level, reached by every thread, with results bit for bit the same (#334): the most portable
/// shape, against compiler bugs like this one on drivers JETCARP can't test, which may let the
/// system WARP run them too. Whether WARP is supported is decided once that has been tested.
pub(crate) fn is_warp(info: &wgpu::AdapterInfo) -> bool {
    info.device_type == wgpu::DeviceType::Cpu
        && (info.backend == wgpu::Backend::Dx12 || info.name.contains("Microsoft Basic Render"))
}

/// The variable that lets tests choose WARP ([`warp_allowed`]).
pub const ALLOW_WARP_ENV: &str = "JETCARP_ALLOW_WARP";

/// Whether WARP may be chosen as other software adapters are, when asked for
/// (`JETCARP_GPU=software` or by name): with `JETCARP_ALLOW_WARP=1`, for tests only, to try a WARP
/// on JETCARP's kernels (#321). The system's WARP crashes on them (`is_warp` says why); WARP
/// 1.0.22's `d3d10warp.dll`, put beside a test program, runs every one of this crate's GPU tests.
pub fn warp_allowed() -> bool {
    std::env::var(ALLOW_WARP_ENV).is_ok_and(|v| v.trim() == "1")
}

/// WARP, unless the tests allow it ([`warp_allowed`]).
fn warp_refused(info: &wgpu::AdapterInfo) -> bool {
    is_warp(info) && !warp_allowed()
}

/// What the simulation needs of an adapter apart from the self-test. `want` lets a software
/// adapter through when it asks for one, and any kind when it names that adapter (a name that
/// matches another adapter lets nothing else through, so doctor never tests one it can't use).
pub fn requirements(
    info: &wgpu::AdapterInfo,
    limits: &wgpu::Limits,
    caps: &wgpu::DownlevelCapabilities,
    want: &Want,
) -> Result<(), String> {
    use wgpu::DeviceType as D;
    if warp_refused(info) {
        return Err(
            "Windows' software renderer (WARP), which the GPU simulation never uses".into(),
        );
    }
    match (info.device_type, want) {
        (D::DiscreteGpu | D::IntegratedGpu, _) => {}
        (D::Cpu, Want::Software) => {}
        (_, Want::Named(_)) if want.matches(info) => {}
        (D::Cpu, _) => {
            return Err(
                "software adapter: the GPU simulation uses it only with JETCARP_GPU=software"
                    .into(),
            );
        }
        (k, _) => {
            return Err(format!(
                "neither discrete nor integrated (the system says {}): the GPU simulation uses it \
                 only when JETCARP_GPU names it",
                kind_name(k)
            ));
        }
    }
    if !caps.flags.contains(wgpu::DownlevelFlags::COMPUTE_SHADERS) {
        return Err("no compute shaders".into());
    }
    if limits.max_storage_buffers_per_shader_stage < MIN_STORAGE_BUFFERS {
        return Err(format!(
            "only {} storage buffers per shader stage (the simulation needs {MIN_STORAGE_BUFFERS})",
            limits.max_storage_buffers_per_shader_stage
        ));
    }
    if limits.max_storage_buffer_binding_size < MIN_STORAGE_BINDING {
        return Err(format!(
            "storage bindings of only {} (the simulation needs {})",
            bytes_text(limits.max_storage_buffer_binding_size),
            bytes_text(MIN_STORAGE_BINDING)
        ));
    }
    Ok(())
}

/// `discrete`, `integrated`, `virtual`, `software`, `other`.
pub fn kind_name(k: wgpu::DeviceType) -> &'static str {
    match k {
        wgpu::DeviceType::DiscreteGpu => "discrete",
        wgpu::DeviceType::IntegratedGpu => "integrated",
        wgpu::DeviceType::VirtualGpu => "virtual",
        wgpu::DeviceType::Cpu => "software",
        wgpu::DeviceType::Other => "other",
    }
}

/// `128 MiB`, `2 GiB`, `1.5 GiB`.
pub fn bytes_text(b: u64) -> String {
    if b >= 1 << 30 {
        let g = format!("{:.1}", b as f64 / (1u64 << 30) as f64);
        format!("{} GiB", g.trim_end_matches(".0"))
    } else if b >= 1 << 20 {
        format!("{} MiB", b >> 20)
    } else {
        format!("{b} bytes")
    }
}

/// The limits that matter to the simulation, in words.
pub fn limits_text(l: &wgpu::Limits) -> String {
    format!(
        "{} storage buffers per stage, {} storage bindings, {} buffers",
        l.max_storage_buffers_per_shader_stage,
        bytes_text(l.max_storage_buffer_binding_size),
        bytes_text(l.max_buffer_size)
    )
}

/// Can the GPU simulation run on this adapter, chosen for a display: the requirements under
/// the policy, and the self-test.
pub fn verify(adapter: &wgpu::Adapter) -> Result<SelfTest, String> {
    let info = adapter.get_info();
    let want = Policy::load().want;
    requirements(
        &info,
        &adapter.limits(),
        &adapter.get_downlevel_capabilities(),
        &want,
    )?;
    let t = self_test(adapter);
    if t.passed { Ok(t) } else { Err(t.note) }
}

/// The limits the simulators ask a device for: the adapter's own where they need more than
/// wgpu's defaults.
pub fn compute_limits(adapter: &wgpu::Adapter) -> wgpu::Limits {
    let al = adapter.limits();
    wgpu::Limits {
        max_storage_buffer_binding_size: al.max_storage_buffer_binding_size,
        max_buffer_size: al.max_buffer_size,
        max_storage_buffers_per_shader_stage: al.max_storage_buffers_per_shader_stage,
        max_compute_workgroups_per_dimension: al.max_compute_workgroups_per_dimension,
        max_compute_workgroup_storage_size: al.max_compute_workgroup_storage_size,
        ..Default::default()
    }
}

/// A device for the simulators on this adapter.
pub fn open_device(
    adapter: &wgpu::Adapter,
    label: &'static str,
) -> Result<(wgpu::Device, wgpu::Queue), String> {
    pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some(label),
        required_features: wgpu::Features::empty(),
        required_limits: compute_limits(adapter),
        experimental_features: Default::default(),
        memory_hints: wgpu::MemoryHints::Performance,
        trace: Default::default(),
    }))
    .map_err(|e| format!("GPU device: {e}"))
}

/// The compute adapter under the policy, with a device of its own (the simulators' headless
/// constructors). The name is the adapter's, for reports.
pub fn compute_device(label: &'static str) -> Result<(wgpu::Device, wgpu::Queue, String), String> {
    let instance = new_instance();
    let c = pick(&instance, Purpose::Compute, None)?;
    let (device, queue) = open_device(&c.adapter, label)?;
    Ok((device, queue, c.label()))
}

/// The outcome of the self-test on one adapter.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SelfTest {
    pub passed: bool,
    /// What matched, or what did not.
    pub note: String,
    /// How long it took, in milliseconds.
    pub millis: u64,
    /// Read from the cache rather than run now.
    #[serde(skip)]
    pub cached: bool,
}

/// Grids the self-test runs on: a wrapped square lattice and an aperiodic patch (explicit
/// storage, dead edge), at about this many cells.
const TEST_GRIDS: [(&str, u32); 3] = [("square", 1024), ("rhombitrihex", 800), ("spectre", 800)];
/// Generations each world and batch runs.
const TEST_GENS: u64 = 24;
/// Draws of a family's rules until one runs on the GPU.
const TEST_DRAWS: usize = 64;
/// Changes when the self-test itself changes, so results are taken again.
const TEST_VERSION: u32 = 5;

static TESTED: Mutex<Option<HashMap<String, SelfTest>>> = Mutex::new(None);

/// Whether self-test results are kept on disk ([`keep_checks`]); off until a program asks.
static KEEP: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Keep self-test results in the settings folder's `gpu-checks.json`, so a later run need not
/// test the adapter again. The `jetcarp` program turns this on when it starts. Code that uses
/// this crate as a library (the test suites among it) keeps results in memory only, so it never
/// writes to the user's own settings folder.
pub fn keep_checks(on: bool) {
    KEEP.store(on, std::sync::atomic::Ordering::Relaxed);
}

/// The self-test on this adapter, run once per adapter, driver, JETCARP version and shader set
/// (kept in the settings folder's `gpu-checks.json`).
pub fn self_test(adapter: &wgpu::Adapter) -> SelfTest {
    let key = test_key(&adapter.get_info());
    // Held while a test runs, so two callers never test at once.
    let mut mem = TESTED.lock().unwrap_or_else(|e| e.into_inner());
    let mem = mem.get_or_insert_with(HashMap::new);
    // A result found again in this run says where it first came from: `cached` (kept from before)
    // only when it was read from the settings folder, not when this run tested the adapter.
    if let Some(t) = mem.get(&key) {
        return t.clone();
    }
    if let Some(t) = read_checks().remove(&key) {
        let t = SelfTest { cached: true, ..t };
        mem.insert(key, t.clone());
        return t;
    }
    let t = run_self_test(adapter);
    write_check(&key, &t);
    mem.insert(key, t.clone());
    t
}

/// The cache key: the adapter, its driver, this JETCARP and its shaders.
fn test_key(info: &wgpu::AdapterInfo) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for s in [
        include_str!("sim.wgsl"),
        include_str!("edit.wgsl"),
        include_str!("batch.wgsl"),
    ] {
        for b in s.bytes() {
            h = (h ^ b as u64).wrapping_mul(0x100_0000_01b3);
        }
    }
    let driver = [info.driver.as_str(), info.driver_info.as_str()]
        .into_iter()
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    format!(
        "{} | {:?} | {:04x}:{:04x} | {driver} | {} | {h:016x}.{TEST_VERSION}",
        info.name,
        info.backend,
        info.vendor,
        info.device,
        jetcarp_core::VERSION
    )
}

fn checks_file() -> Option<PathBuf> {
    if !KEEP.load(std::sync::atomic::Ordering::Relaxed) {
        return None;
    }
    // Screenshot runs never touch the user's settings unless a folder is given.
    if std::env::var_os("JETCARP_CONFIG_DIR").is_none()
        && std::env::var_os("JETCARP_SCREENSHOT").is_some()
    {
        return None;
    }
    jetcarp_core::paths::config_dir().map(|d| d.join(CHECKS_FILE))
}

fn read_checks() -> BTreeMap<String, SelfTest> {
    checks_file()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

fn write_check(key: &str, t: &SelfTest) {
    let Some(path) = checks_file() else { return };
    // Only the jetcarp program keeps results ([`keep_checks`]), so it may make its settings
    // folder for them: a fresh install otherwise tested every adapter at every start.
    if let Some(d) = path.parent()
        && std::fs::create_dir_all(d).is_err()
    {
        return;
    }
    let mut all = read_checks();
    // Results for other versions or shaders are no use any more.
    let tail = key.rsplit(" | ").next().unwrap_or("");
    all.retain(|k, _| k.ends_with(tail));
    all.insert(key.to_string(), t.clone());
    if let Ok(text) = serde_json::to_string_pretty(&all) {
        let _ = write_whole(&path, &text);
    }
}

fn run_self_test(adapter: &wgpu::Adapter) -> SelfTest {
    let t0 = Instant::now();
    let errors: Arc<Mutex<Vec<String>>> = Arc::default();
    let sink = errors.clone();
    let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let (device, queue) = open_device(adapter, "jetcarp-gpu self-test")?;
        device.on_uncaptured_error(Arc::new(move |e| {
            if let Ok(mut v) = sink.lock() {
                v.push(e.to_string());
            }
        }));
        let name = describe(&adapter.get_info());
        let mut sim = GpuSim::new(device.clone(), queue.clone(), name.clone());
        let mut batch = BatchSim::new(device, queue, name);
        compare_all(&mut sim, &mut batch)
    }));
    let errs = errors.lock().map(|v| v.clone()).unwrap_or_default();
    let (passed, note) = match res {
        Ok(Ok(note)) if errs.is_empty() => (true, note),
        Ok(Ok(_)) => (false, format!("GPU error: {}", errs.join("; "))),
        Ok(Err(e)) => (false, e),
        Err(p) => {
            let msg = p
                .downcast_ref::<String>()
                .cloned()
                .or_else(|| p.downcast_ref::<&str>().map(|s| s.to_string()))
                .unwrap_or_else(|| "a panic".into());
            (false, format!("the self-test failed: {msg}"))
        }
    };
    SelfTest {
        passed,
        note: if passed {
            note
        } else {
            format!("self-test: {note}; the GPU simulation will not use this adapter")
        },
        millis: t0.elapsed().as_millis() as u64,
        cached: false,
    }
}

/// One rule of each family that `runs` accepts, drawn the same way every time, with the
/// neighbourhood it runs on (touch and edge in turn). Families the GPU learns join by themselves.
pub fn test_rules(grid: &Grid, runs: impl Fn(&Rule) -> bool) -> Vec<(Rule, Nb)> {
    let mut rng = Rng::new(0x5e1f_7e57 ^ grid.topo.len() as u64);
    ALL_FAMS
        .iter()
        .enumerate()
        .filter(|(_, f)| f.fits(grid))
        .filter_map(|(k, &fam)| {
            let nb = if k % 2 == 0 { Nb::Touch } else { Nb::Edge };
            (0..TEST_DRAWS)
                .find_map(|_| sample_rule(fam, grid, nb, &mut rng).filter(|r| runs(r)))
                .map(|r| (r, nb))
        })
        .collect()
}

/// A world of one CA with `rule`, seeded with random states the same way every time.
fn test_world(grid: &Grid, rule: &Rule, nb: Nb, seed: &SeedSpec, drop: u32) -> Option<World> {
    let mut w = World::on_topology(grid.topo.clone(), 0x5eed);
    let mut ca = CaSpec::new(&rule.notation()).nb(nb).seed(seed.clone());
    ca.drop_per_gen = drop;
    w.add_ca(ca).ok()?;
    Some(w)
}

/// Kernels a family's random draws may miss, always checked where they run (the world stepper,
/// and the explore batch for those it runs): a B0 rule, JACA with a blast, Margolus blocks and
/// isotropic rules (square grids; two states, B0 and Generations), Larger than Life (and two
/// states with B0), forest fire and sandpiles (one with no drop, one dropping grains, one whose
/// drop saturates the centre cell: #20), each with a seed that sets it going and its grains
/// dropped per generation. Add a line here for a variant the draws may not reach.
fn extra_rules() -> Vec<(&'static str, SeedSpec, u32)> {
    vec![
        ("B013/S1", SeedSpec::Random { density: 0.35 }, 0),
        ("JACA B3/S23 blast 2", SeedSpec::Halves { density: 0.35 }, 0),
        ("Margolus Critters", SeedSpec::Random { density: 0.3 }, 0),
        ("B3-cnqy/S23-a", SeedSpec::Random { density: 0.35 }, 0),
        ("B01e2a/S2-i3", SeedSpec::Random { density: 0.35 }, 0),
        (
            "B2a3-ckn/S1e2-in3/C4",
            SeedSpec::Random { density: 0.35 },
            0,
        ),
        ("R2,C3,S5-9,B7-8", SeedSpec::Random { density: 0.4 }, 0),
        ("R2,C2,S0-5,B0-3", SeedSpec::Random { density: 0.35 }, 0),
        ("ForestFire 0.05/0.001", SeedSpec::RandomStates, 0),
        ("Sandpile", SeedSpec::Pile { grains: 2_000 }, 0),
        ("Sandpile", SeedSpec::Pile { grains: 2_000 }, 3),
        (
            "Sandpile",
            SeedSpec::Pile {
                grains: 4_294_967_000,
            },
            500,
        ),
    ]
}

/// The world rules the self-test steps on a grid: one drawn from each family the GPU runs, then
/// the [`extra_rules`] the GPU runs, with their seeds and drops.
fn world_rules(grid: &Grid) -> Vec<(Rule, Nb, SeedSpec, u32)> {
    let mut v: Vec<(Rule, Nb, SeedSpec, u32)> = test_rules(grid, crate::supports_rule)
        .into_iter()
        .map(|(r, nb)| (r, nb, SeedSpec::RandomStates, 0))
        .collect();
    for (text, seed, drop) in extra_rules() {
        if let Ok((rule, nb)) = Rule::parse(text)
            && crate::supports_rule(&rule)
        {
            v.push((rule, nb.unwrap_or_default(), seed, drop));
        }
    }
    v
}

/// The explore batch rules the self-test runs on a grid: one drawn from each family the batch
/// stepper runs, then the [`extra_rules`] it runs (isotropic rules on the square grid among them:
/// two states, B0 and Generations, #245), with their seeds.
fn batch_rules(grid: &Grid) -> Vec<(Rule, Nb, SeedSpec)> {
    let mut v: Vec<(Rule, Nb, SeedSpec)> = test_rules(grid, BatchSim::supports)
        .into_iter()
        .map(|(r, nb)| (r, nb, SeedSpec::RandomStates))
        .collect();
    for (text, seed, drop) in extra_rules() {
        if let Ok((rule, nb)) = Rule::parse(text)
            && drop == 0
            && BatchSim::supports(&rule)
        {
            v.push((rule, nb.unwrap_or_default(), seed));
        }
    }
    v
}

/// Every world kernel and the batch stepper against the CPU, on each test grid.
fn compare_all(sim: &mut GpuSim, batch: &mut BatchSim) -> Result<String, String> {
    let (mut worlds, mut batches) = (0, 0);
    for (key, cells) in TEST_GRIDS {
        let grid = Grid::build(key, cells)?;
        let at = |rule: &Rule| format!("{} on {key}", rule.notation());
        for (rule, nb, seed, drop) in world_rules(&grid) {
            let (Some(mut cpu), Some(mut gpu)) = (
                test_world(&grid, &rule, nb, &seed, drop),
                test_world(&grid, &rule, nb, &seed, drop),
            ) else {
                continue;
            };
            cpu.apply(Command::Step { gens: TEST_GENS })?;
            // In one GPU batch, not the few short ones a new stepper starts with.
            sim.batch_hint = TEST_GENS;
            let m = sim
                .step_mixed(&mut gpu, TEST_GENS)
                .map_err(|e| format!("{}: {e}", at(&rule)))?;
            if m.gpu_gens == 0 {
                continue;
            }
            if cpu.hash_hex() != gpu.hash_hex() || cpu.generation != gpu.generation {
                return Err(format!(
                    "{} gave a different world from the CPU's after {TEST_GENS} generations",
                    at(&rule)
                ));
            }
            worlds += 1;
        }
        // One batch per neighbourhood (Larger than Life: per hood and range), as a search has.
        let mut jobs: BTreeMap<String, BatchJob> = BTreeMap::new();
        for (rule, nb, seed) in batch_rules(&grid) {
            let Some(w) = test_world(&grid, &rule, nb, &seed, 0) else {
                continue;
            };
            let group = match &rule {
                Rule::Ltl(l) => format!("ltl {:?} {}", l.hood, l.range),
                _ => format!("{nb:?}"),
            };
            jobs.entry(group)
                .or_insert_with(|| BatchJob {
                    topo: grid.topo.clone(),
                    nb,
                    worlds: vec![],
                    gens: TEST_GENS as u32,
                    frames_at: vec![TEST_GENS as u32],
                })
                .worlds
                .push(BatchWorld {
                    rule,
                    init: w.fields[0].state.clone(),
                });
        }
        for job in jobs.values() {
            let Ok(want) = run_cpu(job) else { continue };
            let got = batch
                .run(job)
                .map_err(|e| format!("explore batch on {key}: {e}"))?;
            for (k, (g, c)) in got.iter().zip(&want).enumerate() {
                if g != c {
                    return Err(format!(
                        "explore batch, {}: a different run from the CPU's",
                        at(&job.worlds[k].rule)
                    ));
                }
            }
            if got.len() != want.len() {
                return Err(format!("explore batch on {key}: worlds missing"));
            }
            batches += got.len();
        }
    }
    if worlds == 0 {
        return Err("no world rule could run on the GPU".into());
    }
    Ok(format!(
        "{worlds} world rules and {batches} explore batches matched the CPU"
    ))
}

/// One purpose's choice, for `jetcarp doctor`.
#[derive(Clone, Debug)]
pub struct ChoiceReport {
    pub purpose: Purpose,
    /// The adapter, described (none: no adapter at all).
    pub adapter: Option<String>,
    /// discrete, integrated, software, ...
    pub kind: String,
    pub why: String,
    pub limits: String,
    pub test: Option<SelfTest>,
    /// Why the simulation does not use it.
    pub problem: Option<String>,
}

/// The graphics adapters on this computer, by name, each once (an adapter reached through several
/// backends, Vulkan and DirectX 12 say, is listed once), with its kind (`discrete`, `integrated`,
/// `software`, ...), for a choice of adapter in the app (#59). A name chosen here is saved as
/// [`Setting::gpu_adapter`], which matches by part of the name, as `JETCARP_GPU` does.
pub fn adapter_names() -> Vec<(String, &'static str)> {
    let instance = new_instance();
    let adapters = pollster::block_on(instance.enumerate_adapters(wgpu::Backends::all()));
    let mut out: Vec<(String, &'static str)> = Vec::new();
    for a in adapters {
        let info = a.get_info();
        if !out.iter().any(|(n, _)| *n == info.name) {
            out.push((info.name.clone(), kind_name(info.device_type)));
        }
    }
    out
}

/// The policy and each purpose's choice, with the self-test (run now if it has not been).
pub fn report() -> (Policy, Vec<ChoiceReport>) {
    report_with(true)
}

/// [`report`], running the self-test only when `test` (doctor skips it when the GPU simulation is
/// off for everything, so turning the GPU off doesn't cost a test of every adapter).
pub fn report_with(test: bool) -> (Policy, Vec<ChoiceReport>) {
    let policy = Policy::load();
    let instance = new_instance();
    let adapters = adapters_for(&instance, &policy.want);
    let out = [Purpose::Display, Purpose::Compute]
        .into_iter()
        .map(
            |purpose| match pick_tested(&adapters, &policy, purpose, None, test) {
                Ok(c) => {
                    let (test, problem) = match (&c.usable, c.test.clone()) {
                        (Err(e), _) => (None, Some(e.clone())),
                        (Ok(()), Some(t)) => (Some(t), None),
                        (Ok(()), None) if !test => (None, None),
                        (Ok(()), None) => {
                            let t = self_test(&c.adapter);
                            let p = (!t.passed).then(|| t.note.clone());
                            (Some(t), p)
                        }
                    };
                    ChoiceReport {
                        purpose,
                        adapter: Some(describe(&c.info)),
                        kind: kind_name(c.info.device_type).into(),
                        why: c.why,
                        limits: limits_text(&c.adapter.limits()),
                        test,
                        problem,
                    }
                }
                Err(e) => ChoiceReport {
                    purpose,
                    adapter: None,
                    kind: String::new(),
                    why: policy.origin(),
                    limits: String::new(),
                    test: None,
                    problem: Some(e),
                },
            },
        )
        .collect();
    (policy, out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_variable_sets_mode_or_adapter() {
        let p = Policy::from_env("off");
        assert_eq!(
            (p.mode, p.want, p.source),
            (Mode::Off, Want::Best, Source::Env)
        );
        assert_eq!(Policy::from_env("0").mode, Mode::Off);
        assert_eq!(
            Policy::from_env("1").mode,
            Mode::On,
            "the old JETCARP_GPU=1"
        );
        assert_eq!(Policy::from_env(" On ").mode, Mode::On);
        assert_eq!(Policy::from_env("auto").mode, Mode::Auto);
        let p = Policy::from_env("software");
        assert_eq!((p.mode, p.want), (Mode::Auto, Want::Software));
        assert_eq!(Policy::from_env("Integrated").want, Want::Integrated);
        assert_eq!(Policy::from_env("discrete").want, Want::Discrete);
        let p = Policy::from_env("UHD");
        assert_eq!(p.origin(), "JETCARP_GPU=UHD");
        assert_eq!((p.mode, p.want), (Mode::Auto, Want::Named("UHD".into())));
    }

    #[test]
    fn a_request_wins_over_the_policy() {
        let p = |m| Policy {
            mode: m,
            ..Policy::default()
        };
        assert_eq!(p(Mode::Off).uses(None), Use::Never);
        assert_eq!(p(Mode::Off).uses(Some(true)), Use::Always);
        assert_eq!(p(Mode::Auto).uses(None), Use::Auto);
        assert_eq!(p(Mode::Auto).uses(Some(false)), Use::Never);
        assert_eq!(p(Mode::On).uses(None), Use::Always);
        assert_eq!(p(Mode::On).uses(Some(false)), Use::Never);
    }

    /// First to last: JETCARP_GPU (when it names a mode), a command's --gpu or --cpu, the
    /// program's own choice, the shared setting, automatic.
    #[test]
    fn the_variable_then_the_command_then_the_own_choice_then_the_shared_setting() {
        use Use::*;
        let shared = |m: Option<Mode>| match m {
            Some(m) => Policy::from_setting(&Setting {
                gpu: m,
                gpu_adapter: Some("UHD".into()),
            }),
            None => Policy::default(),
        };
        let env = |v: Option<&str>, m: Option<Mode>| match v {
            Some(v) => Policy::from_env(v),
            None => shared(m),
        };
        // (variable, command, own, shared) -> what is done, and where the mode came from.
        type Case<'a> = (
            Option<&'a str>,
            Option<bool>,
            Own,
            Option<Mode>,
            Use,
            Source,
        );
        let cases: [Case; 14] = [
            (None, None, Own::Follow, None, Auto, Source::Default),
            (
                None,
                None,
                Own::Follow,
                Some(Mode::Off),
                Never,
                Source::Setting,
            ),
            (
                None,
                None,
                Own::Follow,
                Some(Mode::On),
                Always,
                Source::Setting,
            ),
            (None, None, Own::On, Some(Mode::Off), Always, Source::Own),
            (None, None, Own::Off, Some(Mode::On), Never, Source::Own),
            (None, None, Own::Off, None, Never, Source::Own),
            (
                None,
                Some(true),
                Own::Off,
                Some(Mode::Off),
                Always,
                Source::Own,
            ),
            (
                None,
                Some(false),
                Own::On,
                Some(Mode::On),
                Never,
                Source::Own,
            ),
            (
                Some("off"),
                None,
                Own::On,
                Some(Mode::On),
                Never,
                Source::Env,
            ),
            (
                Some("on"),
                None,
                Own::Off,
                Some(Mode::Off),
                Always,
                Source::Env,
            ),
            (Some("off"), Some(true), Own::On, None, Never, Source::Env),
            (Some("auto"), Some(false), Own::Off, None, Auto, Source::Env),
            // An adapter's name leaves the mode to the rest.
            (Some("UHD"), None, Own::Off, None, Never, Source::Own),
            (
                Some("software"),
                Some(true),
                Own::Follow,
                None,
                Always,
                Source::Env,
            ),
        ];
        for (v, asked, own, m, want, source) in cases {
            let p = env(v, m).with_own(own, "the app");
            assert_eq!(
                (p.uses(asked), p.source),
                (want, source),
                "{v:?} {asked:?} {own:?} {m:?}"
            );
        }
        // The adapter stays the shared setting's, and reports say whose choice it was.
        let p = shared(Some(Mode::Auto)).with_own(Own::Off, "the desktop background");
        assert_eq!(p.want, Want::Named("UHD".into()));
        assert_eq!(p.origin(), "the desktop background's own setting");
        assert_eq!(
            p.describe(),
            "off, the adapter named UHD (the desktop background's own setting)"
        );
        // Stored choices, and the booleans of older files.
        #[derive(Deserialize)]
        struct S {
            #[serde(deserialize_with = "Own::deserialize_lenient")]
            gpu: Own,
        }
        let own = |j: &str| serde_json::from_str::<S>(j).map(|s| s.gpu);
        assert_eq!(own(r#"{"gpu": true}"#).unwrap(), Own::Follow);
        assert_eq!(own(r#"{"gpu": false}"#).unwrap(), Own::Off);
        assert_eq!(own(r#"{"gpu": "on"}"#).unwrap(), Own::On);
        assert_eq!(own(r#"{"gpu": "follow"}"#).unwrap(), Own::Follow);
        assert!(own(r#"{"gpu": "sometimes"}"#).is_err());
        assert_eq!(serde_json::to_string(&Own::Off).unwrap(), r#""off""#);
        assert_eq!(Own::parse("auto"), Some(Own::Follow));
    }

    #[test]
    fn the_setting_reads_and_writes() {
        let s: Setting = serde_json::from_str(r#"{"gpu": "off", "gpu_adapter": "UHD"}"#).unwrap();
        assert_eq!(s.gpu, Mode::Off);
        let p = Policy::from_setting(&s);
        assert_eq!(
            (p.mode, p.want, p.source),
            (Mode::Off, Want::Named("UHD".into()), Source::Setting)
        );
        let s: Setting = serde_json::from_str("{}").unwrap();
        assert_eq!(s, Setting::default());
        assert_eq!(serde_json::to_string(&s).unwrap(), r#"{"gpu":"auto"}"#);
        assert!(serde_json::from_str::<Setting>(r#"{"gpu": "maybe"}"#).is_err());
    }

    /// A named adapter missing from the first list is looked for again (#82); the best needs no
    /// second look, and one never found gives up after two more.
    #[test]
    fn a_named_adapter_missing_at_first_is_looked_for_again() {
        use std::cell::Cell;
        use wgpu::{Backend, DeviceType as D};
        let uhd = || info("Intel(R) UHD Graphics", D::IntegratedGpu, Backend::Dx12);
        let rtx = || {
            info(
                "NVIDIA GeForce RTX 3050 Laptop GPU",
                D::DiscreteGpu,
                Backend::Dx12,
            )
        };
        let rtx_wanted = Want::parse("RTX");
        let wait = std::time::Duration::ZERO;
        let found = |want: &Want| {
            let want = want.clone();
            move |l: &[wgpu::AdapterInfo]| has_wanted(&want, l.iter().cloned())
        };

        // The RTX shows up on the second look.
        let calls = Cell::new(0);
        let list = || {
            calls.set(calls.get() + 1);
            if calls.get() == 1 {
                vec![uhd()]
            } else {
                vec![uhd(), rtx()]
            }
        };
        let got = look_again(list, found(&rtx_wanted), 2, wait);
        assert_eq!(calls.get(), 2);
        assert!(got.iter().any(|i| i.name.contains("RTX")));

        // There at once: one look.
        let calls = Cell::new(0);
        let list = || {
            calls.set(calls.get() + 1);
            vec![uhd(), rtx()]
        };
        look_again(list, found(&rtx_wanted), 2, wait);
        assert_eq!(calls.get(), 1);

        // The best: one look, whatever is there.
        let calls = Cell::new(0);
        let list = || {
            calls.set(calls.get() + 1);
            vec![uhd()]
        };
        look_again(list, found(&Want::Best), 2, wait);
        assert_eq!(calls.get(), 1);

        // Never there: three looks, then the choice falls back as before.
        let calls = Cell::new(0);
        let list = || {
            calls.set(calls.get() + 1);
            vec![uhd()]
        };
        let got = look_again(list, found(&rtx_wanted), 2, wait);
        assert_eq!(calls.get(), 3);
        assert_eq!(got.len(), 1);

        // WARP doesn't count as the software adapter asked for, as in the choice, unless the
        // tests allow it (#321).
        let warp = info("Microsoft Basic Render Driver", D::Cpu, Backend::Dx12);
        assert_eq!(
            has_wanted(&Want::Software, [warp].into_iter()),
            warp_allowed()
        );
    }

    fn info(name: &str, kind: wgpu::DeviceType, backend: wgpu::Backend) -> wgpu::AdapterInfo {
        wgpu::AdapterInfo {
            name: name.into(),
            vendor: 0,
            device: 0,
            device_type: kind,
            device_pci_bus_id: String::new(),
            driver: String::new(),
            driver_info: String::new(),
            backend,
            subgroup_min_size: 4,
            subgroup_max_size: 64,
            transient_saves_memory: None,
            limit_bucket: None,
        }
    }

    /// On Windows, JETCARP's instances leave OpenGL out unless an adapter is named, which may be
    /// one only OpenGL reaches (#315); Linux and macOS open every backend, as before. wgpu's own
    /// `WGPU_BACKEND`, when it is set, decides instead.
    #[test]
    fn windows_leaves_opengl_out_unless_an_adapter_is_named() {
        if std::env::var_os("WGPU_BACKEND").is_some() {
            return;
        }
        let all = wgpu::Backends::all();
        for want in [Want::Best, Want::Software, Want::Integrated, Want::Discrete] {
            let b = backends(&want);
            assert_eq!(b.contains(wgpu::Backends::GL), !cfg!(windows), "{want:?}");
            assert_eq!(b | wgpu::Backends::GL, all, "{want:?}");
        }
        assert_eq!(backends(&Want::Named("RTX".into())), all);
    }

    /// A process keeps the first instance it makes of its backends to its end, so the graphics
    /// drivers stay loaded while it runs, and says it has opened the GPU (#315).
    #[test]
    fn the_first_instance_is_kept_and_the_gpu_is_opened() {
        let instance = new_instance();
        assert!(opened());
        let backends = instance_descriptor().backends;
        let kept = KEPT.lock().unwrap_or_else(|e| e.into_inner());
        assert_eq!(kept.iter().filter(|(b, _)| *b == backends).count(), 1);
        drop(instance);
    }

    /// The app's adapter list (#59) names each adapter once, however many backends reach it, and
    /// every name it gives is one the setting's part-of-a-name match finds.
    #[test]
    fn adapter_names_list_each_adapter_once() {
        let names = adapter_names();
        for (i, (n, _)) in names.iter().enumerate() {
            assert!(!names[..i].iter().any(|(m, _)| m == n), "{n} listed twice");
            assert_eq!(Want::parse(n), Want::Named(n.trim().to_string()));
        }
    }

    #[test]
    fn decent_means_a_real_gpu_with_room() {
        use wgpu::{Backend as B, DeviceType as D};
        let caps = wgpu::DownlevelCapabilities::default();
        let mut lim = wgpu::Limits {
            max_storage_buffers_per_shader_stage: 16,
            ..Default::default()
        };
        {
            let ok = |i: &wgpu::AdapterInfo, w: &Want| requirements(i, &lim, &caps, w);
            assert!(ok(&info("RTX 3050", D::DiscreteGpu, B::Vulkan), &Want::Best).is_ok());
            assert!(ok(&info("UHD", D::IntegratedGpu, B::Vulkan), &Want::Best).is_ok());
            let soft = info("llvmpipe (LLVM 21)", D::Cpu, B::Vulkan);
            assert!(
                ok(&soft, &Want::Best)
                    .unwrap_err()
                    .contains("JETCARP_GPU=software")
            );
            assert!(ok(&soft, &Want::Software).is_ok());
            assert!(ok(&soft, &Want::Named("llvmpipe".into())).is_ok());
            // A name that matches another adapter lets no other software or odd adapter in
            // (#48: JETCARP_GPU=llvmpipe had doctor self-test the Radeon's GL adapter).
            assert!(ok(&soft, &Want::Named("radeon".into())).is_err());
            let gl = info("AMD Radeon 8060S Graphics (radeonsi)", D::Other, B::Gl);
            assert!(ok(&gl, &Want::Named("llvmpipe".into())).is_err());
            assert!(ok(&gl, &Want::Named("radeonsi".into())).is_ok());
            let warp = info("Microsoft Basic Render Driver", D::Cpu, B::Dx12);
            if warp_allowed() {
                assert!(ok(&warp, &Want::Software).is_ok());
                assert!(ok(&warp, &Want::Best).is_err());
            } else {
                assert!(ok(&warp, &Want::Software).unwrap_err().contains("WARP"));
                assert!(ok(&warp, &Want::Named("Microsoft".into())).is_err());
            }
            let virt = info("virtio", D::VirtualGpu, B::Vulkan);
            assert!(ok(&virt, &Want::Best).is_err());
            assert!(ok(&virt, &Want::Named("virtio".into())).is_ok());
        }
        let dev = info("RTX 3050", D::DiscreteGpu, B::Vulkan);
        lim.max_storage_buffers_per_shader_stage = 8;
        let err = requirements(&dev, &lim, &caps, &Want::Best).unwrap_err();
        assert!(err.contains("storage buffers"), "{err}");
        lim.max_storage_buffers_per_shader_stage = 9;
        lim.max_storage_buffer_binding_size = 64 << 20;
        let err = requirements(&dev, &lim, &caps, &Want::Best).unwrap_err();
        assert!(err.contains("128 MiB"), "{err}");
        let none = wgpu::DownlevelCapabilities {
            flags: wgpu::DownlevelFlags::empty(),
            ..Default::default()
        };
        lim.max_storage_buffer_binding_size = 128 << 20;
        assert!(requirements(&dev, &lim, &none, &Want::Best).is_err());
    }

    #[test]
    fn compute_prefers_discrete_and_display_integrated() {
        use wgpu::{Backend as B, DeviceType as D};
        let d = info("RTX", D::DiscreteGpu, B::Vulkan);
        let i = info("UHD", D::IntegratedGpu, B::Vulkan);
        let g = info("RTX", D::DiscreteGpu, B::Gl);
        let s = info("llvmpipe", D::Cpu, B::Vulkan);
        assert!(rank(&d, Purpose::Compute) < rank(&i, Purpose::Compute));
        assert!(rank(&i, Purpose::Display) < rank(&d, Purpose::Display));
        assert!(rank(&d, Purpose::Compute) < rank(&g, Purpose::Compute));
        assert!(rank(&i, Purpose::Compute) < rank(&s, Purpose::Compute));
        assert!(Want::Named("uhd".into()).matches(&i));
        assert!(!Want::Best.matches(&s));
        assert!(Want::Software.matches(&s));
    }

    #[test]
    fn sizes_read_plainly() {
        assert_eq!(bytes_text(128 << 20), "128 MiB");
        assert_eq!(bytes_text(4 << 30), "4 GiB");
        assert_eq!(bytes_text((2 << 30) - 1), "2 GiB");
        assert_eq!(bytes_text(3 << 29), "1.5 GiB");
    }

    #[test]
    fn every_family_the_gpu_runs_is_in_the_self_test() {
        use jetcarp_core::explore::space::Fam;
        // A family the world stepper learns joins the self-test without a change here.
        for (key, cells) in TEST_GRIDS {
            let grid = Grid::build(key, cells).unwrap();
            let rules = world_rules(&grid);
            let picked: Vec<Fam> = rules.iter().map(|(r, _, _, _)| Fam::of(r)).collect();
            let mut want = vec![
                Fam::LifeLike,
                Fam::Generations,
                Fam::WireWorld,
                Fam::Cyclic,
                Fam::Rps,
                Fam::GreenbergHastings,
                Fam::Hodgepodge,
                Fam::SpectreGlider,
                Fam::Priority,
                Fam::Jaca,
                Fam::Ltl,
                Fam::ForestFire,
                Fam::Sandpile,
            ];
            if grid.topo.rows().is_some() {
                want.push(Fam::Margolus);
                want.push(Fam::Isotropic);
            }
            for fam in want {
                assert!(picked.contains(&fam), "{fam:?} on {key}: {picked:?}");
            }
            let names: Vec<String> = rules.iter().map(|(r, _, _, _)| r.notation()).collect();
            for extra in ["B013/S1", "JACA B3/S23 blast 2"] {
                assert!(
                    names.iter().any(|n| n == extra),
                    "{extra} on {key}: {names:?}"
                );
            }
            assert!(!test_rules(&grid, BatchSim::supports).is_empty());
        }
    }

    /// The self-test's explore batches take isotropic rules on the square grid (#245): one drawn
    /// as the search draws them, and the extra rules' two-state, B0 and Generations ones.
    #[test]
    fn the_self_tests_batches_run_isotropic_rules() {
        use jetcarp_core::explore::space::Fam;
        for (key, cells) in TEST_GRIDS {
            let grid = Grid::build(key, cells).unwrap();
            let rules = batch_rules(&grid);
            let iso: Vec<String> = rules
                .iter()
                .filter(|(r, _, _)| Fam::of(r) == Fam::Isotropic)
                .filter(|(r, nb, seed)| test_world(&grid, r, *nb, seed, 0).is_some())
                .map(|(r, _, _)| r.notation())
                .collect();
            if grid.topo.rows().is_none() {
                assert!(iso.is_empty(), "{key}: {iso:?}");
                continue;
            }
            for extra in ["B3-cnqy/S23-a", "B01e2a/S2-i3", "B2a3-ckn/S1e2-in3/C4"] {
                assert!(iso.iter().any(|n| n == extra), "{extra} on {key}: {iso:?}");
            }
            assert!(iso.len() >= 4, "{key}: no isotropic rule drawn: {iso:?}");
        }
    }
}
