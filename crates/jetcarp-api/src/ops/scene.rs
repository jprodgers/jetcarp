//! `scene.*`: scene files: read one, write one, check that one replays to its recorded hash, and
//! run one headless (`jetcarp run`).

use crate::ctx::Ctx;
use crate::error::{ApiError, ErrorKind, ResultExt};
use crate::op::{Op, Progress, Report};
use jetcarp_core::reseed::ReseedWhen;
use jetcarp_core::rules::FieldKind;
use jetcarp_core::{Command, Scene, VERSION, World};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fmt::Write as _;

/// A scene from JSON (an object, or its text) or from a file, as every front end reads one.
pub fn read_scene(cx: &Ctx, scene: Option<&Value>, path: Option<&str>) -> Result<Scene, ApiError> {
    match (scene.filter(|v| !v.is_null()), path) {
        (Some(_), Some(_)) => Err(ApiError::invalid("give scene or path, not both")),
        (Some(Value::String(s)), None) => Scene::from_json(s).kind(ErrorKind::Invalid),
        (Some(v), None) => serde_json::from_value(v.clone())
            .map_err(|e| ApiError::invalid(format!("scene JSON: {e}"))),
        (None, Some(p)) => {
            let file = cx.fs.readable(p)?;
            let text = std::fs::read_to_string(&file).map_err(|e| {
                let kind = if e.kind() == std::io::ErrorKind::NotFound {
                    ErrorKind::NotFound
                } else {
                    ErrorKind::Io
                };
                ApiError::new(kind, format!("could not read {p}: {e}"))
            })?;
            Scene::from_json(&text).kind(ErrorKind::Invalid)
        }
        (None, None) => Err(ApiError::invalid("give `scene` (JSON) or `path`")),
    }
}

/// A scene, from JSON or a file.
#[derive(Clone, Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SceneSource {
    /// Scene JSON: the object, or its text.
    #[serde(default)]
    pub scene: Option<Value>,
    /// A scene .json file.
    #[serde(default)]
    pub path: Option<String>,
}

/// A generation and state hash a scene recorded.
#[derive(Clone, Debug, PartialEq, Serialize, JsonSchema)]
pub struct Recorded {
    #[serde(serialize_with = "jetcarp_core::json_int::uint::serialize")]
    #[schemars(schema_with = "crate::op::uint_schema")]
    pub generation: u64,
    pub hash: String,
}

/// A scene.
#[derive(Clone, Debug, Serialize, JsonSchema)]
pub struct SceneOut {
    #[schemars(with = "Value")]
    pub scene: Scene,
    /// The generation and hash it recorded when saved, if it did.
    pub recorded: Option<Recorded>,
}

impl Report for SceneOut {
    fn text(&self) -> String {
        self.scene.to_json()
    }
}

/// `scene.load`.
pub struct SceneLoad;
impl Op for SceneLoad {
    const NAME: &'static str = "scene.load";
    const SUMMARY: &'static str = "Read a scene (JSON or a .json file) and check that it parses.";
    const READ_ONLY: bool = true;
    type In = SceneSource;
    type Out = SceneOut;
    fn run(cx: &Ctx, a: SceneSource, _: &dyn Progress) -> Result<SceneOut, ApiError> {
        let scene = read_scene(cx, a.scene.as_ref(), a.path.as_deref())?;
        Ok(SceneOut {
            recorded: scene.record.as_ref().map(|r| Recorded {
                generation: r.generation,
                hash: r.hash.clone(),
            }),
            scene,
        })
    }
}

/// A scene to write.
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SaveIn {
    /// Scene JSON: the object, or its text.
    pub scene: Value,
    /// The .json file to write (its folder is created).
    pub path: String,
}

/// A file written.
#[derive(Clone, Debug, Serialize, JsonSchema)]
pub struct Saved {
    pub path: std::path::PathBuf,
    pub bytes: usize,
}

impl Report for Saved {
    fn text(&self) -> String {
        format!(
            "Saved the scene ({} bytes) to {}.\n",
            self.bytes,
            self.path.display()
        )
    }
}

/// `scene.save`.
pub struct SceneSave;
impl Op for SceneSave {
    const NAME: &'static str = "scene.save";
    const SUMMARY: &'static str =
        "Write a scene to a .json file (the app and `jetcarp run` open it).";
    const READ_ONLY: bool = false;
    type In = SaveIn;
    type Out = Saved;
    fn run(cx: &Ctx, a: SaveIn, _: &dyn Progress) -> Result<Saved, ApiError> {
        let scene = read_scene(cx, Some(&a.scene), None)?;
        let path = cx.fs.writable(&a.path)?;
        let json = scene.to_json();
        std::fs::write(&path, &json)
            .map_err(|e| ApiError::io(format!("could not write {}: {e}", path.display())))?;
        Ok(Saved {
            path,
            bytes: json.len(),
        })
    }
}

/// A scene that replays to its recorded hash.
#[derive(Clone, Debug, Serialize, JsonSchema)]
pub struct Verified {
    #[serde(serialize_with = "jetcarp_core::json_int::uint::serialize")]
    #[schemars(schema_with = "crate::op::uint_schema")]
    pub generation: u64,
    pub hash: String,
    /// What loading it let pass (palettes this JETCARP does not know).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
}

impl Report for Verified {
    fn text(&self) -> String {
        let mut s = format!("ok: generation {} hash {}\n", self.generation, self.hash);
        for w in &self.warnings {
            let _ = writeln!(s, "warning: {w}");
        }
        s
    }
}

/// `scene.verify`.
pub struct SceneVerify;
impl Op for SceneVerify {
    const NAME: &'static str = "scene.verify";
    const SUMMARY: &'static str =
        "Check that a saved scene still replays to its recorded generation and hash.";
    const READ_ONLY: bool = true;
    type In = SceneSource;
    type Out = Verified;
    fn run(cx: &Ctx, a: SceneSource, _: &dyn Progress) -> Result<Verified, ApiError> {
        let sc = read_scene(cx, a.scene.as_ref(), a.path.as_deref())?;
        let Some(rec) = sc.record.clone() else {
            return Err(ApiError::invalid("scene has no recorded hash"));
        };
        let w = World::from_scene(&sc)?;
        if w.generation != rec.generation || w.hash_hex() != rec.hash {
            return Err(ApiError::conflict(format!(
                "MISMATCH: recorded gen {} hash {}, replayed gen {} hash {}",
                rec.generation,
                rec.hash,
                w.generation,
                w.hash_hex()
            )));
        }
        Ok(Verified {
            generation: w.generation,
            hash: rec.hash,
            warnings: w.warnings().to_vec(),
        })
    }
}

fn max_side_default() -> u32 {
    1600
}

/// How to run a scene.
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RunIn {
    /// The scene .json file.
    pub scene: String,
    /// Generations to run after replaying the scene's own commands.
    #[serde(default)]
    pub gens: u64,
    /// Write a PNG of the final state here (scene, version, and hash embedded).
    #[serde(default)]
    pub png: Option<String>,
    /// Pixels per world unit for the PNG (tile edges are about 1 unit). Default: fit into
    /// `max_side`.
    #[serde(default)]
    pub scale: Option<f64>,
    /// Longest PNG side in pixels (default 1600).
    #[serde(default = "max_side_default")]
    pub max_side: u32,
    /// Save the resulting scene here (with its generation and hash recorded).
    #[serde(default)]
    pub save: Option<String>,
    /// Worker threads (default: all cores). The hash does not depend on this.
    #[serde(default)]
    pub threads: Option<usize>,
    /// Fail unless the final hash equals this (the summary is still returned).
    #[serde(default)]
    pub expect: Option<String>,
    /// Step on the GPU where the rules allow it (results are identical).
    #[serde(default)]
    pub gpu: bool,
    /// Automatic reseed policies to install first: one policy or a list.
    #[serde(default)]
    pub reseed: Option<Value>,
    /// Reseed once before running, with this mode.
    #[serde(default)]
    pub reseed_now: Option<Value>,
}

/// One field after a run.
#[derive(Clone, Debug, Serialize, JsonSchema)]
pub struct RunField {
    pub field: usize,
    pub kind: String,
    pub shared: bool,
    pub population: u64,
    /// Sandpile grains lost off dead edges.
    pub lost: u64,
    /// Names of the CAs on it.
    pub cas: Vec<String>,
}

/// What a run ended with.
#[derive(Clone, Debug, Serialize, JsonSchema)]
pub struct RunSummary {
    pub scene: String,
    pub tiling: String,
    pub cells: usize,
    #[serde(serialize_with = "jetcarp_core::json_int::uint::serialize")]
    #[schemars(schema_with = "crate::op::uint_schema")]
    pub generation: u64,
    pub hash: String,
    pub status: String,
    pub done: bool,
    pub done_reason: Option<String>,
    pub dead_boundary: bool,
    pub threads: usize,
    pub jetcarp_version: String,
    pub seconds: f64,
    pub fields: Vec<RunField>,
    /// Where the generations ran (CPU threads, or the GPU and why some fell back).
    pub backend: String,
    pub png: Option<String>,
    /// `[width, height]` of the PNG.
    pub png_size: Option<(u32, u32)>,
    /// Reseed policies and their status.
    pub reseed: Value,
    /// What loading the scene let pass (palettes this JETCARP does not know).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
    #[serde(skip)]
    text: String,
    #[serde(skip)]
    expect: Option<String>,
}

impl Report for RunSummary {
    fn text(&self) -> String {
        self.text.clone()
    }
    fn check(&self) -> Result<(), ApiError> {
        match &self.expect {
            Some(e) if *e != self.hash => Err(ApiError::conflict(format!(
                "hash mismatch: expected {e}, got {}",
                self.hash
            ))),
            _ => Ok(()),
        }
    }
}

/// `scene.run`.
pub struct SceneRun;
impl Op for SceneRun {
    const NAME: &'static str = "scene.run";
    const SUMMARY: &'static str = "Load a scene, run it headless (on the GPU where possible), and report generation, hash, and populations; optionally save a PNG and the resulting scene.";
    const READ_ONLY: bool = false;
    type In = RunIn;
    type Out = RunSummary;
    fn run(cx: &Ctx, a: RunIn, p: &dyn Progress) -> Result<RunSummary, ApiError> {
        match a.threads {
            Some(t) => rayon::ThreadPoolBuilder::new()
                .num_threads(t)
                .build()
                .map_err(|e| ApiError::engine(format!("thread pool: {e}")))?
                .install(|| run_scene(cx, a, p)),
            None => run_scene(cx, a, p),
        }
    }
}

fn run_scene(cx: &Ctx, a: RunIn, _: &dyn Progress) -> Result<RunSummary, ApiError> {
    let sc = read_scene(cx, None, Some(&a.scene))?;
    let t0 = std::time::Instant::now();
    let mut w = World::from_scene(&sc)?;
    if let Some(v) = &a.reseed {
        let policies = crate::args::reseed_policies(v)
            .map_err(|e| ApiError::invalid(format!("not a reseed policy: {e}")))?;
        w.apply(Command::SetReseed { policies })?;
    }
    if let Some(v) = &a.reseed_now {
        let mode = crate::args::reseed_mode(v)
            .map_err(|e| ApiError::invalid(format!("not a reseed mode: {e}")))?;
        w.apply(Command::Reseed { ca: None, mode })?;
    }
    let gens = a.gens;
    let cpu = format!("cpu ({} threads)", rayon::current_num_threads());
    let backend = match a.gpu.then(|| cx.platform.gpu_stepper()) {
        Some(Ok(mut sim)) => {
            let m = sim.step_mixed(&mut w, gens)?;
            match m.fallback {
                None => format!("gpu ({})", sim.name()),
                Some(why) => format!(
                    "gpu ({}); {} of {gens} generations on the {cpu}: {why}",
                    sim.name(),
                    m.cpu_gens
                ),
            }
        }
        other => {
            if gens > 0 {
                w.apply(Command::Step { gens })?;
            }
            match other {
                Some(Err(e)) => format!("{cpu}; no GPU: {e}"),
                _ => cpu,
            }
        }
    };
    let seconds = t0.elapsed().as_secs_f64();
    let hash = w.hash_hex();
    let mut png_size = None;
    if let Some(p) = &a.png {
        // Streams in strips, so very large images (a small scale on a huge grid) fit.
        let opts = jetcarp_core::render::RenderOptions {
            scale: a.scale,
            max_side: a.max_side,
            ..Default::default()
        };
        let file = cx.fs.writable(p)?;
        png_size = Some(jetcarp_core::render::write_png_streaming(
            &file,
            &w,
            &opts,
            &jetcarp_core::render::png_metadata(&w),
        )?);
    }
    if let Some(p) = &a.save {
        let mut out = w.to_scene();
        out.name = sc.name.clone();
        out.description = sc.description.clone();
        out.view = sc.view.clone();
        let file = cx.fs.writable(p)?;
        std::fs::write(&file, out.to_json())
            .map_err(|e| ApiError::io(format!("writing {p}: {e}")))?;
    }
    let threads = rayon::current_num_threads();
    let users = |i: usize, with_rule: bool| -> Vec<String> {
        w.cas
            .iter()
            .filter(|c| c.field == i)
            .map(|c| {
                if with_rule {
                    format!("{} [{}]", c.name, c.notation())
                } else {
                    c.name.clone()
                }
            })
            .collect()
    };
    let fields: Vec<RunField> = w
        .fields
        .iter()
        .enumerate()
        .map(|(i, f)| RunField {
            field: i,
            kind: f.kind.name().to_string(),
            shared: f.shared,
            population: f.population() as u64,
            lost: f.lost as u64,
            cas: users(i, false),
        })
        .collect();
    let reseed: Value = w
        .reseed_policies()
        .iter()
        .zip(w.reseed_status())
        .map(|(p, st)| serde_json::json!({"policy": p, "status": st}))
        .collect();

    let mut s = String::new();
    let _ = writeln!(s, "scene:      {}", a.scene);
    let _ = writeln!(s, "tiling:     {}", w.topo.describe());
    for warning in w.topo.warnings.iter().chain(w.warnings()) {
        let _ = writeln!(s, "warning:    {warning}");
    }
    let _ = writeln!(s, "backend:    {backend}");
    let _ = writeln!(s, "generation: {}", w.generation);
    let _ = writeln!(s, "hash:       {hash}");
    let _ = writeln!(s, "status:     {}", w.status());
    match w.done_reason() {
        Some(r) => {
            let _ = writeln!(s, "done:       yes ({r})");
        }
        None => s.push_str("done:       no\n"),
    }
    for (p, st) in w.reseed_policies().iter().zip(w.reseed_status()) {
        let err = st
            .error
            .as_deref()
            .map(|e| format!(" ({e})"))
            .unwrap_or_default();
        let _ = writeln!(
            s,
            "reseed:     {} {}: fired {} times, last at {}{err}",
            p.mode.name(),
            match p.when {
                ReseedWhen::Settled { .. } => "when settled".into(),
                ReseedWhen::Every { gens } => format!("every {gens}"),
            },
            st.fired,
            st.last_fired.map_or("-".into(), |g| g.to_string()),
        );
    }
    if let Some(sc) = &w.script {
        let st = &w.script_state;
        let name = sc.steps.get(st.step).map(|x| x.name.as_str()).unwrap_or("");
        let fin = if st.finished { " (finished)" } else { "" };
        let _ = writeln!(
            s,
            "script:     step {} of {}: {name}{fin}",
            st.step + 1,
            sc.steps.len()
        );
    }
    for (i, f) in w.fields.iter().enumerate() {
        let height = f.kind == FieldKind::Height;
        let lost = if height {
            format!(", {} lost off edges", f.lost)
        } else {
            String::new()
        };
        let _ = writeln!(
            s,
            "field {i}:    {} {}, {} {}{lost}: {}",
            if f.shared { "shared" } else { "private" },
            f.kind.name(),
            f.population(),
            if height { "grains" } else { "live" },
            users(i, true).join(", ")
        );
    }
    if let (Some(p), Some((pw, ph))) = (&a.png, png_size) {
        let _ = writeln!(s, "png:        {p} ({pw}x{ph})");
    }
    if let Some(p) = &a.save {
        let _ = writeln!(s, "saved:      {p}");
    }
    let _ = writeln!(s, "time:       {seconds:.2}s on {threads} threads");

    Ok(RunSummary {
        scene: a.scene,
        tiling: w.topo.describe(),
        cells: w.topo.len(),
        generation: w.generation,
        status: w.status().to_string(),
        done: w.done(),
        done_reason: w.done_reason(),
        dead_boundary: w.topo.has_boundary(),
        threads,
        jetcarp_version: VERSION.to_string(),
        seconds,
        fields,
        backend,
        png: a.png,
        png_size,
        reseed,
        warnings: w.warnings().to_vec(),
        text: s,
        expect: a.expect,
        hash,
    })
}
