//! `machine.*`: this machine, its JETCARP Hardware Score, and the limits set from them
//! ([`jetcarp_core::machine`]).
//!
//! | Operation | What it does |
//! | --- | --- |
//! | `machine.info` | The hardware, the last score with its parts, and the limits (`jetcarp doctor`'s hardware lines, the MCP tools' `reference {"what": "machine"}`) |
//! | `machine.limits` | The limits: each one's value, suggestion, the person's own value and ceiling (`jetcarp limits`) |
//! | `machine.limits_set` | Set one limit to a value of one's own (`limits.json`) |
//! | `machine.limits_reset` | Back to the suggestion, for one limit or all |
//! | `machine.score` | Run the benchmark (about a minute), keep the score in `machine.json`, and write a report (`jetcarp score`; never from the MCP tools) |
//!
//! The limits are worked out from the hardware, read now, and the own values in this context's
//! settings folder, so a change shows at once. After a change in the settings folder this
//! process uses, the process's limits are read again ([`jetcarp_core::machine::reload`]).

use crate::ctx::{Ctx, GpuAdapter, GpuMode};
use crate::error::ApiError;
use crate::op::{NoInput, Op, Progress, Report};
use jetcarp_core::machine::{
    self, Hardware, Id, Limits, MachineFile, Own, Score, Source, bench, bytes_text, limits,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

/// The hardware as read now (the process's reading when it made one).
fn hardware() -> Hardware {
    machine::hardware()
        .map(|h| (*h).clone())
        .unwrap_or_else(Hardware::read)
}

/// The limits on this machine with the own values in `settings` (none: the suggestions).
fn table(hw: &Hardware, settings: Option<&Path>) -> (Limits, Own, Option<String>) {
    let (own, note) = Own::load(settings);
    (Limits::compute(hw, &own), own, note)
}

/// After a change in `settings`: if it is the folder this process reads, read its limits again.
fn reload_if_ours(settings: &Path) {
    if jetcarp_core::paths::config_dir().as_deref() == Some(settings) {
        machine::reload();
    }
}

/// The limits, for `machine.limits` and inside `machine.info`.
#[derive(Clone, Debug, Serialize, JsonSchema)]
pub struct LimitsOut {
    /// The memory the limits were worked out from, in bytes.
    pub memory_total: u64,
    /// The memory could not be read: these are a 16 GB machine's limits.
    pub memory_estimated: bool,
    /// Each limit: `name` (as `jetcarp limits set` takes it), `label`, `unit`, `value` (what
    /// applies), `source` (`suggested`, `own` or `ceiling`), `suggestion` and `why`, `own` (the
    /// person's own value), `floor` and `ceiling` with `ceiling_why`.
    #[schemars(with = "Vec<Value>")]
    pub entries: Vec<machine::Entry>,
    /// `limits.json`, where own values are kept (none: no settings folder).
    pub file: Option<PathBuf>,
    /// A `limits.json` that could not be read.
    pub note: Option<String>,
}

impl LimitsOut {
    fn of(l: Limits, file: Option<PathBuf>, note: Option<String>) -> LimitsOut {
        LimitsOut {
            memory_total: l.memory_total,
            memory_estimated: l.memory_estimated,
            entries: l.entries,
            file,
            note,
        }
    }
}

/// One limit in a line: `Largest world  39 GB  (half of the 78 GB reserved for JETCARP)`.
fn limit_line(e: &machine::Entry) -> String {
    let from = match e.source {
        Source::Own => format!(
            "your own value; suggested {}",
            limits::value_text(e.unit, e.suggestion)
        ),
        Source::Suggested => e.why.clone(),
        Source::Ceiling => format!(
            "held to the ceiling, {}",
            limits::value_text(e.unit, e.ceiling)
        ),
    };
    format!("  {:<32} {:<22} {} ({from})", e.label, e.text(), e.name)
}

impl Report for LimitsOut {
    fn text(&self) -> String {
        let mut s = format!(
            "Limits on this machine ({} of memory{})\n",
            bytes_text(self.memory_total),
            if self.memory_estimated {
                ", an estimate: the system did not say"
            } else {
                ""
            }
        );
        for e in &self.entries {
            let _ = writeln!(s, "{}", limit_line(e));
        }
        s.push_str(
            "\nEach is your own value if you set one (`jetcarp limits set NAME VALUE`), else the \
             machine's suggestion, never above its ceiling. They only allow or refuse: a world \
             too big is refused with the reason, never made smaller.\n",
        );
        if let Some(n) = &self.note {
            let _ = writeln!(s, "note: {n}");
        }
        s
    }
}

/// `machine.limits`.
pub struct MachineLimits;
impl Op for MachineLimits {
    const NAME: &'static str = "machine.limits";
    const SUMMARY: &'static str = "This machine's limits: each one's value, the machine's suggestion and why, your own value, and its ceiling.";
    const READ_ONLY: bool = true;
    type In = NoInput;
    type Out = LimitsOut;
    fn run(cx: &Ctx, _: NoInput, _: &dyn Progress) -> Result<LimitsOut, ApiError> {
        let settings = cx.paths().settings;
        let (l, _, note) = table(&hardware(), settings.as_deref());
        Ok(LimitsOut::of(l, settings.as_deref().map(Own::file), note))
    }
}

/// Input of `machine.limits_set`.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LimitSetIn {
    /// The limit's name: reserved-system, reserved-jetcarp, world, gpu-world, host-cells,
    /// host-memory, hashlife-nodes, hunt-cache, media-pixels, wallpaper-cells, animation-frames,
    /// ai-share.
    pub name: String,
    /// The value in the limit's unit: memory as `24GB` or `512MB`, counts as `64000000` or
    /// `64M`, an AI's share as `0.25`, `25%`, `small` or `all`.
    pub value: String,
}

fn limit_id(name: &str) -> Result<Id, ApiError> {
    Id::parse(name).ok_or_else(|| {
        ApiError::invalid(format!(
            "no limit called '{name}': the limits are {}",
            Id::ALL.map(|i| i.name()).join(", ")
        ))
    })
}

fn settings_dir(cx: &Ctx) -> Result<PathBuf, ApiError> {
    cx.paths()
        .settings
        .ok_or_else(|| ApiError::io("no settings folder on this system: limits cannot be kept"))
}

/// `machine.limits_set`.
pub struct MachineLimitsSet;
impl Op for MachineLimitsSet {
    const NAME: &'static str = "machine.limits_set";
    const SUMMARY: &'static str = "Set one of this machine's limits to a value of your own (kept in limits.json; measuring again never undoes it).";
    const READ_ONLY: bool = false;
    type In = LimitSetIn;
    type Out = LimitsOut;
    fn run(cx: &Ctx, a: LimitSetIn, _: &dyn Progress) -> Result<LimitsOut, ApiError> {
        let id = limit_id(&a.name)?;
        let dir = settings_dir(cx)?;
        let (l, own, _) = table(&hardware(), Some(&dir));
        let v = limits::parse_value(id.unit(), &a.value).map_err(ApiError::invalid)?;
        let own = l.set(&own, id, v).map_err(ApiError::invalid)?;
        let file = own.save(&dir).map_err(ApiError::io)?;
        reload_if_ours(&dir);
        let (l, _, note) = table(&hardware(), Some(&dir));
        Ok(LimitsOut::of(l, Some(file), note))
    }
}

/// Input of `machine.limits_reset`.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LimitResetIn {
    /// The limit to put back to the machine's suggestion (default: every one).
    #[serde(default)]
    pub name: Option<String>,
}

/// `machine.limits_reset`.
pub struct MachineLimitsReset;
impl Op for MachineLimitsReset {
    const NAME: &'static str = "machine.limits_reset";
    const SUMMARY: &'static str =
        "Put one of this machine's limits, or every one, back to the machine's suggestion.";
    const READ_ONLY: bool = false;
    type In = LimitResetIn;
    type Out = LimitsOut;
    fn run(cx: &Ctx, a: LimitResetIn, _: &dyn Progress) -> Result<LimitsOut, ApiError> {
        let id = a.name.as_deref().map(limit_id).transpose()?;
        let dir = settings_dir(cx)?;
        let (_, own, _) = table(&hardware(), Some(&dir));
        let own = Limits::reset(&own, id);
        let file = own.save(&dir).map_err(ApiError::io)?;
        reload_if_ours(&dir);
        let (l, _, note) = table(&hardware(), Some(&dir));
        Ok(LimitsOut::of(l, Some(file), note))
    }
}

/// `machine.info`'s output: the hardware, the score, the limits.
#[derive(Clone, Debug, Serialize, JsonSchema)]
pub struct MachineOut {
    /// What the system says it holds: `os`, `arch`, `cpu`, `threads`, `cores`, `memory_total`,
    /// `memory_available` (bytes), and `gpus` (each GPU's memory, where the system says).
    #[schemars(with = "Value")]
    pub hardware: Hardware,
    /// The graphics adapters JETCARP sees.
    pub adapters: Vec<GpuAdapter>,
    /// The last JETCARP Hardware Score, with its parts (none: never measured here).
    #[schemars(with = "Option<Value>")]
    pub score: Option<Score>,
    /// Why the score should be measured again (another machine, or an older benchmark).
    pub stale: Option<String>,
    pub limits: LimitsOut,
    /// `machine.json`, where the score is kept.
    pub machine_file: Option<PathBuf>,
}

/// The hardware in lines, for doctor and `machine.info`.
pub fn hardware_text(hw: &Hardware) -> String {
    let mut s = String::new();
    let _ = writeln!(s, "  system:    {} {}", hw.os, hw.arch);
    let _ = writeln!(
        s,
        "  processor: {} ({} threads{})",
        hw.cpu,
        hw.threads,
        hw.cores.map_or(String::new(), |c| format!(", {c} cores"))
    );
    let _ = writeln!(
        s,
        "  memory:    {}{}",
        if hw.memory_total > 0 {
            bytes_text(hw.memory_total)
        } else {
            "unknown (the limits are a 16 GB machine's)".into()
        },
        hw.memory_available
            .map_or(String::new(), |a| format!(" ({} free now)", bytes_text(a)))
    );
    for g in &hw.gpus {
        let name = if g.name.is_empty() {
            format!("GPU {:04x}:{:04x}", g.vendor, g.device)
        } else {
            g.name.clone()
        };
        let mem = match (g.dedicated, g.shared, g.unified) {
            (_, Some(sh), true) => format!("shares the system's {}", bytes_text(sh)),
            (Some(d), Some(sh), _) => {
                format!("{} own, may borrow {}", bytes_text(d), bytes_text(sh))
            }
            (Some(d), None, _) => format!("{} own", bytes_text(d)),
            _ => "memory not reported".into(),
        };
        let _ = writeln!(s, "  {name}: {mem} ({})", g.source);
    }
    s
}

/// The score in lines.
pub fn score_text(score: Option<&Score>, stale: Option<&str>) -> String {
    let Some(sc) = score else {
        return "JETCARP Hardware Score: not measured yet (`jetcarp score`, or Tools → This \
                computer…, about a minute)\n"
            .into();
    };
    let mut s = format!(
        "JETCARP Hardware Score: {} (score v{}, {}, JETCARP {}{})\n",
        sc.text(),
        sc.benchmark,
        if sc.date.is_empty() {
            "undated"
        } else {
            &sc.date
        },
        sc.jetcarp,
        if sc.quick { ", a short run" } else { "" }
    );
    for p in &sc.parts {
        let _ = writeln!(
            s,
            "  {:<44} {:<26} {} {}{}",
            p.label,
            p.device,
            machine::score::rate_text(p.value),
            p.unit,
            match p.correct {
                Some(false) => "  WRONG ANSWER",
                _ => "",
            }
        );
    }
    for p in &sc.problems {
        let _ = writeln!(s, "  problem: {p}");
    }
    if let Some(why) = stale {
        let _ = writeln!(s, "  note: {why}: measure it again (`jetcarp score`)");
    }
    s
}

impl Report for MachineOut {
    fn text(&self) -> String {
        let mut s = String::from("This computer\n");
        s.push_str(&hardware_text(&self.hardware));
        s.push('\n');
        s.push_str(&score_text(self.score.as_ref(), self.stale.as_deref()));
        s.push('\n');
        s.push_str(&self.limits.text());
        s
    }
}

/// `machine.info`.
pub struct MachineInfo;
impl Op for MachineInfo {
    const NAME: &'static str = "machine.info";
    const SUMMARY: &'static str = "This machine: its hardware, its JETCARP Hardware Score with the parts, and the limits set from them.";
    const READ_ONLY: bool = true;
    type In = NoInput;
    type Out = MachineOut;
    fn run(cx: &Ctx, _: NoInput, _: &dyn Progress) -> Result<MachineOut, ApiError> {
        Ok(machine_info(cx))
    }
}

/// [`MachineInfo`]'s report.
pub fn machine_info(cx: &Ctx) -> MachineOut {
    let settings = cx.paths().settings;
    let hw = hardware();
    let (l, _, note) = table(&hw, settings.as_deref());
    let file = MachineFile::load(settings.as_deref());
    let stale = file.as_ref().and_then(|f| f.stale(&hw));
    MachineOut {
        adapters: cx.platform.gpu_adapters(),
        score: file.and_then(|f| f.score),
        stale,
        limits: LimitsOut::of(l, settings.as_deref().map(Own::file), note),
        machine_file: settings.as_deref().map(MachineFile::file),
        hardware: hw,
    }
}

/// Input of `machine.score`.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ScoreIn {
    /// Each workload once, for its answers alone (as the tests run it): seconds, not a minute,
    /// and rates that mean little.
    #[serde(default)]
    pub quick: bool,
    /// Measure the GPU too (default: when the GPU setting allows it).
    #[serde(default)]
    pub gpu: Option<bool>,
    /// Also write the report here, in Markdown and JSON (`report.md` and `report.json`; a name
    /// ending in `.md` or `.json` gets its other half beside it).
    #[serde(default)]
    pub report: Option<String>,
    /// Keep the score in `machine.json` (default true).
    #[serde(default)]
    pub save: Option<bool>,
}

/// `machine.score`'s output.
#[derive(Clone, Debug, Serialize, JsonSchema)]
pub struct ScoreOut {
    #[schemars(with = "Value")]
    pub score: Score,
    #[schemars(with = "Value")]
    pub hardware: Hardware,
    pub limits: LimitsOut,
    /// Where it was kept and written.
    pub machine_file: Option<PathBuf>,
    pub report_files: Vec<PathBuf>,
}

impl Report for ScoreOut {
    fn text(&self) -> String {
        let mut s = score_text(Some(&self.score), None);
        if let Some(f) = &self.machine_file {
            let _ = writeln!(s, "\nKept in {}", f.display());
        }
        for f in &self.report_files {
            let _ = writeln!(s, "Report: {}", f.display());
        }
        s
    }
    fn check(&self) -> Result<(), ApiError> {
        match self.score.wrong().next() {
            Some(p) => Err(ApiError::engine(format!(
                "{} gave a wrong answer on {}: see the problems above",
                p.device, p.label
            ))),
            None => Ok(()),
        }
    }
}

/// The GPU's world stepper, for the crossover: a generation that fell back to the processor is
/// an error here, since the point is to time the GPU.
struct Stepper(Box<dyn crate::ctx::GpuStepper>);
impl bench::WorldStepper for Stepper {
    fn step(&mut self, w: &mut jetcarp_core::World, gens: u64) -> Result<(), String> {
        let s = self.0.step_mixed(w, gens)?;
        match s.fallback {
            Some(why) if s.cpu_gens > 0 => Err(format!("it stepped on the processor: {why}")),
            _ => Ok(()),
        }
    }
}

/// The report's two files for `name`.
fn report_paths(cx: &Ctx, name: &str) -> Result<(PathBuf, PathBuf), ApiError> {
    let base = name
        .strip_suffix(".md")
        .or_else(|| name.strip_suffix(".json"))
        .unwrap_or(name);
    Ok((
        cx.fs.writable(&format!("{base}.md"))?,
        cx.fs.writable(&format!("{base}.json"))?,
    ))
}

/// Write the report of `score` on `hw` with the limits `l` as `<name>.md` and `<name>.json`
/// (`jetcarp score --report`, and the app's *Save a report*): the machine, its parts, its score
/// and its limits, for comparing machines. Nothing is sent anywhere.
pub fn save_report(
    cx: &Ctx,
    name: &str,
    score: &Score,
    hw: &Hardware,
    l: &Limits,
) -> Result<Vec<PathBuf>, ApiError> {
    let (md, js) = report_paths(cx, name)?;
    let names: Vec<String> = cx
        .platform
        .gpu_adapters()
        .into_iter()
        .map(|g| g.name)
        .collect();
    std::fs::write(&md, score.markdown(hw, l, &names))
        .map_err(|e| ApiError::io(format!("{}: {e}", md.display())))?;
    let body = serde_json::json!({
        "jetcarp": jetcarp_core::VERSION,
        "hardware": hw,
        "adapters": names,
        "score": score,
        "limits": l,
    });
    std::fs::write(
        &js,
        serde_json::to_string_pretty(&body).unwrap_or_default() + "\n",
    )
    .map_err(|e| ApiError::io(format!("{}: {e}", js.display())))?;
    Ok(vec![md, js])
}

/// `machine.score`.
pub struct MachineScore;
impl Op for MachineScore {
    const NAME: &'static str = "machine.score";
    const SUMMARY: &'static str = "Measure this machine: about a minute of JETCARP's own work (Life soups on five non-square tilings, one core, a hunt, the GPU, memory, the disk), each answer checked; keeps the JETCARP Hardware Score in machine.json and can write a report.";
    const READ_ONLY: bool = false;
    type In = ScoreIn;
    type Out = ScoreOut;
    fn run(cx: &Ctx, a: ScoreIn, p: &dyn Progress) -> Result<ScoreOut, ApiError> {
        let settings = cx.paths().settings;
        let data = cx.paths().data;
        let hw = Hardware::read();
        let mut gpus = vec![];
        let want_gpu = a
            .gpu
            .unwrap_or_else(|| cx.platform.gpu_mode() != GpuMode::Off);
        if want_gpu {
            let batch = cx.platform.explore_backend().ok();
            let world = cx.platform.gpu_stepper().ok();
            let name = world
                .as_ref()
                .map(|w| w.name())
                .or_else(|| batch.as_ref().map(|b| b.name()));
            if let Some(name) = name {
                gpus.push(bench::Gpu {
                    name,
                    batch,
                    world: world.map(|w| Box::new(Stepper(w)) as Box<dyn bench::WorldStepper>),
                });
            }
        }
        let opts = bench::Options {
            quick: a.quick,
            scratch: Some(data.clone()),
            rss: Some(jetcarp_search::jobs::explore::memory_bytes),
            ..Default::default()
        };
        let _ = std::fs::create_dir_all(&data);
        let report = |f: f64, msg: &str| p.report((f * 1000.0) as u64, Some(1000), msg);
        let stop = || p.cancelled();
        let mut score = bench::run(&opts, gpus, &report, &stop).map_err(|e| {
            if e == bench::STOPPED {
                ApiError::conflict(e)
            } else {
                ApiError::engine(e)
            }
        })?;
        score.date = chrono::Local::now().format("%Y-%m-%d %H:%M").to_string();
        let (l, _, note) = table(&hw, settings.as_deref());
        let mut machine_file = None;
        if a.save.unwrap_or(true)
            && let Some(dir) = settings.as_deref()
        {
            let f = MachineFile {
                version: 1,
                fingerprint: hw.fingerprint(),
                hardware: Some(hw.clone()),
                score: Some(score.clone()),
                suggestions: l
                    .entries
                    .iter()
                    .map(|e| (e.name.clone(), e.suggestion))
                    .collect(),
                suggested_for: String::new(),
            };
            machine_file = Some(f.save(dir).map_err(ApiError::io)?);
        }
        let report_files = match a.report.as_deref() {
            Some(name) => save_report(cx, name, &score, &hw, &l)?,
            None => vec![],
        };
        Ok(ScoreOut {
            score,
            hardware: hw,
            limits: LimitsOut::of(l, settings.as_deref().map(Own::file), note),
            machine_file,
            report_files,
        })
    }
}
