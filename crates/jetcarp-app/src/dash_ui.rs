//! The *Hunts…* window (Phase 4 deliverable 7): every catalog job (hunts, censuses, ant
//! searches, piles, movers, evolve, explore and older searches), including jobs another process
//! runs, with live counters, pause, resume, and stop; a form to start a hunt here; the rule-space
//! heat map of the selected job; and its newest and rarest finds, animated, to rate or reject. A
//! job that is not deterministic (an explore search) is *continued* rather than resumed: it
//! carries on from its saved archive, not exactly as an uninterrupted run.
//!
//! The UI thread never touches SQLite: a worker thread with its own catalog connection reads
//! the `jobs` table every [`JOBS_EVERY`] (WAL lets it read while hunts write), recomputes the
//! selected job's heat map and finds at most every [`DETAIL_EVERY`] and only when the job wrote
//! new progress, and runs the commands (start, pause, resume, stop, rate, reject). A hunt
//! starts through the operation `search.start`, pause and stop go through `jobs.stop`, and
//! *Resume here* through `jobs.resume`, all in the background: the jobs run on the job runner,
//! their handles in the process's registry (`jetcarp_search::jobs::registry`), which the other
//! windows' jobs and the app's MCP link share. Animations are made on two more threads
//! (`media::pattern_animation`) only for the finds on screen, in the house style or the palette
//! picked under *Colours* (which *Render media* uses too). Everything kind-specific is in
//! `jetcarp_search::jobs::StrategyKind`.

use crate::thumbs;
use crate::window_job::{self, AppPlatform};
use eframe::egui::{self, Color32, ColorImage, Rect, RichText, Sense, TextureHandle};
use jetcarp_api::op::{Op, Quiet};
use jetcarp_api::ops::jobs::{JobIn, JobsResume, JobsStop, ResumeIn};
use jetcarp_api::ops::search::{HuntIn, SearchStart, SearchStartIn};
use jetcarp_api::{Ctx, Platform};
use jetcarp_core::hunt::soup::SoupSpec;
use jetcarp_core::media::{self, MediaOptions, MediaStyle};
use jetcarp_core::rules::Rule;
use jetcarp_core::topology::Nb;
use jetcarp_search::dashboard::{self, FindItem, Finds, HeatColour, HeatMap, JobSummary, Target};
use jetcarp_search::hunt::HuntSpec;
use jetcarp_search::jobs::{self, registry};
use jetcarp_store::{HeatRow, JobStatus, Library};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, channel};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

/// This window's key in `crate::popout` and the settings file.
pub const KEY: &str = "hunts";

/// How often the job list is read.
pub const JOBS_EVERY: Duration = Duration::from_secs(2);
/// Least time between two reads of a job's heat map and finds.
pub const DETAIL_EVERY: Duration = Duration::from_secs(10);
/// Finds per row.
const FINDS: usize = 12;

/// The start-a-hunt form.
#[derive(Clone, Debug, PartialEq)]
pub struct HuntForm {
    pub rules: String,
    pub tilings: String,
    /// 0 = from the notation, 1 = touch, 2 = edge.
    pub nb: usize,
    pub w: u32,
    pub h: u32,
    pub density: f64,
    pub soups: u64,
    pub forever: bool,
    pub seed: u64,
    pub max_gens: u64,
    pub gpu: bool,
    /// 0 = all cores.
    pub threads: usize,
}

impl Default for HuntForm {
    fn default() -> Self {
        let d = HuntSpec::default();
        HuntForm {
            rules: "B3/S23".into(),
            tilings: "square".into(),
            nb: 0,
            w: d.soup.w,
            h: d.soup.h,
            density: d.soup.density,
            soups: 10_000,
            forever: false,
            seed: 1,
            max_gens: d.max_gens,
            gpu: false,
            threads: 0,
        }
    }
}

/// Split a list typed in a field: commas, semicolons, spaces, new lines.
pub fn split_list(s: &str) -> Vec<String> {
    s.split(|c: char| c == ',' || c == ';' || c.is_whitespace())
        .map(str::trim)
        .filter(|x| !x.is_empty())
        .map(String::from)
        .collect()
}

impl HuntForm {
    /// The hunt this form describes (checked).
    pub fn spec(&self) -> Result<HuntSpec, String> {
        let rules: Vec<String> = split_list(&self.rules)
            .into_iter()
            .map(|r| -> Result<String, String> {
                let (rule, _) = Rule::parse(&r).map_err(|e| format!("rule '{r}': {e}"))?;
                jetcarp_core::hunt::bits::bs_rule(&rule).map_err(|e| format!("rule '{r}': {e}"))?;
                Ok(r)
            })
            .collect::<Result<_, _>>()?;
        let tilings = split_list(&self.tilings)
            .iter()
            .map(|t| jetcarp_api::args::tiling_by_id(t).map_err(|e| format!("tiling '{t}': {e}")))
            .collect::<Result<Vec<_>, _>>()?;
        let spec = HuntSpec {
            rules,
            tilings,
            neighborhood: match self.nb {
                1 => Some(Nb::Touch),
                2 => Some(Nb::Edge),
                _ => None,
            },
            soups: (!self.forever).then_some(self.soups),
            soup: SoupSpec {
                w: self.w.max(1),
                h: self.h.max(1),
                density: self.density,
                ..Default::default()
            },
            seed: self.seed,
            max_gens: self.max_gens,
            ..Default::default()
        };
        spec.check()?;
        Ok(spec)
    }

    /// The hunt as the operation `search.start` takes it (strategy `hunt`), once
    /// [`spec`](Self::spec) has checked it.
    pub fn input(&self) -> HuntIn {
        HuntIn {
            rules: Some(serde_json::json!(split_list(&self.rules))),
            tilings: Some(serde_json::json!(split_list(&self.tilings))),
            nb: match self.nb {
                1 => Some("touch".into()),
                2 => Some("edge".into()),
                _ => None,
            },
            soups: Some(if self.forever { 0 } else { self.soups }),
            size: Some(serde_json::json!([self.w.max(1), self.h.max(1)])),
            density: Some(self.density),
            seed: Some(self.seed),
            gens: Some(self.max_gens),
            ..Default::default()
        }
    }
}

/// The selected job's heat map and finds.
pub struct Detail {
    pub job: i64,
    pub spaces: Vec<String>,
    pub space: usize,
    pub rules: Vec<String>,
    pub map: Option<HeatMap>,
    pub heat_rows: Vec<HeatRow>,
    pub image: Option<ColorImage>,
    /// Non-empty cells (x, y) when there are few, drawn as markers so single rules show.
    pub marks: Vec<(usize, usize, f64)>,
    pub finds: Finds,
    pub rev: u64,
    pub error: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
struct Selection {
    job: i64,
    space: usize,
    colour: HeatColour,
    rules: Vec<String>,
}

enum Cmd {
    Select(Selection),
    Start(Box<HuntIn>, bool, usize),
    Pause(i64),
    Resume(i64, bool, usize),
    Stop(i64),
    Rate(Target, u8),
    Reject(Target),
    /// Render these finds' media into the catalog, in the house style or a palette.
    RenderMedia(Vec<Target>, Option<String>),
    Refresh,
    /// Pause every job running here, wait for their checkpoints, then answer.
    Shutdown(Sender<()>),
}

#[derive(Default)]
struct Shared {
    jobs: Vec<JobSummary>,
    loaded: bool,
    detail: Option<Detail>,
    messages: Vec<(String, bool)>,
    /// Bumped when a job starts here (the window selects it).
    started: Option<i64>,
}

/// A colour ramp for the heat map: dark blue through teal to amber and white.
pub fn ramp(t: f64) -> [u8; 3] {
    const STOPS: [[f64; 3]; 5] = [
        [0x2b as f64, 0x24 as f64, 0x70 as f64],
        [0x1f as f64, 0x9f as f64, 0xc9 as f64],
        [0x58 as f64, 0xe0 as f64, 0xc2 as f64],
        [0xff as f64, 0xc8 as f64, 0x3c as f64],
        [0xff as f64, 0xf6 as f64, 0xe0 as f64],
    ];
    let t = t.clamp(0.0, 1.0) * (STOPS.len() - 1) as f64;
    let i = (t.floor() as usize).min(STOPS.len() - 2);
    let f = t - i as f64;
    let mut out = [0u8; 3];
    for c in 0..3 {
        out[c] = (STOPS[i][c] + (STOPS[i + 1][c] - STOPS[i][c]) * f).round() as u8;
    }
    out
}

/// A cell's colour position: log scale for counts.
pub fn heat_t(v: f64, max: f64) -> f64 {
    if max <= 0.0 {
        0.0
    } else {
        (1.0 + v).ln() / (1.0 + max).ln()
    }
}

/// The heat map as an image, one pixel per cell (empty cells dark grey).
pub fn heat_image(m: &HeatMap) -> ColorImage {
    let mut px = Vec::with_capacity(m.w * m.h);
    for y in 0..m.h {
        for x in 0..m.w {
            let v = m.at(x, y);
            let has = !m.rules_at(x, y).is_empty();
            px.push(if !has {
                if (x / 8 + y / 8) % 2 == 0 {
                    Color32::from_gray(22)
                } else {
                    Color32::from_gray(27)
                }
            } else {
                let [r, g, b] = ramp(0.15 + 0.85 * heat_t(v, m.max));
                Color32::from_rgb(r, g, b)
            });
        }
    }
    ColorImage {
        size: [m.w, m.h],
        source_size: egui::vec2(m.w as f32, m.h as f32),
        pixels: px,
    }
}

struct Worker {
    lib: Library,
    path: PathBuf,
    shared: Arc<Mutex<Shared>>,
    /// Jobs to mark done once they have paused.
    finishing: HashSet<i64>,
    sel: Option<Selection>,
    sel_dirty: bool,
    last_detail: Option<Instant>,
    detail_updated: String,
    rev: u64,
    /// The operations' context, on the worker's catalog connection.
    cx: Ctx,
    /// The GPU soup hunter was asked for and is missing (said once).
    no_gpu: bool,
}

impl Worker {
    fn msg(&self, m: String, err: bool) {
        self.shared.lock().unwrap().messages.push((m, err));
    }

    /// GPU batches for a hunt when asked for and there is a GPU; else the CPU, said once.
    fn gpu(&mut self, asked: bool) -> bool {
        if !asked || self.no_gpu {
            return false;
        }
        match AppPlatform.soup_batcher() {
            Ok(_) => true,
            Err(e) => {
                self.no_gpu = true;
                self.msg(format!("No GPU for hunts ({e}); running on the CPU."), true);
                false
            }
        }
    }

    /// *Resume here*: `jobs.resume` in the background (the run gets its own catalog
    /// connection, so its writes never wait on the worker's reads).
    fn resume(&mut self, job: i64, gpu: bool, threads: usize) {
        if registry().is_running(&self.lib, job) {
            return;
        }
        let kind = self.lib.job(job).map(|r| r.kind).unwrap_or_default();
        // Hunts need the GPU soup hunter; an explore search takes the GPU batch stepper when
        // there is one and runs on the CPU otherwise (its progress says which).
        let gpu = match kind.as_str() {
            "hunt" => self.gpu(gpu),
            "explore" => gpu,
            _ => false,
        };
        let a = ResumeIn {
            job,
            threads: Some(threads),
            gpu,
            background: true,
            ..Default::default()
        };
        let verb = if jobs::kind(&kind).deterministic() {
            "running"
        } else {
            "continuing from its archive"
        };
        match JobsResume::run(&self.cx, a, &Quiet) {
            Ok(_) => self.msg(format!("Job {job} is {verb} here."), false),
            Err(e) => self.msg(e.to_string(), true),
        }
    }

    /// *Pause*: `jobs.stop`, here or in whatever process runs it.
    fn pause(&mut self, job: i64) {
        if let Err(e) = JobsStop::run(&self.cx, JobIn { job }, &Quiet) {
            self.msg(e.to_string(), true);
        }
    }

    fn handle(&mut self, c: Cmd) {
        match c {
            Cmd::Select(s) => {
                if self.sel.as_ref() != Some(&s) {
                    self.sel = Some(s);
                    self.sel_dirty = true;
                }
            }
            Cmd::Start(hunt, gpu, threads) => {
                let a = SearchStartIn {
                    strategy: Some("hunt".into()),
                    spec: serde_json::to_value(*hunt).unwrap_or_default(),
                    threads: Some(threads),
                    gpu: self.gpu(gpu),
                    background: true,
                    ..Default::default()
                };
                match SearchStart::run(&self.cx, a, &Quiet) {
                    Ok(out) => {
                        self.msg(format!("Job {} is running here.", out.job), false);
                        self.shared.lock().unwrap().started = Some(out.job);
                    }
                    Err(e) => self.msg(e.to_string(), true),
                }
            }
            Cmd::Pause(job) => self.pause(job),
            Cmd::Resume(job, gpu, threads) => self.resume(job, gpu, threads),
            Cmd::Stop(job) => {
                self.finishing.insert(job);
                self.handle(Cmd::Pause(job));
            }
            Cmd::Rate(t, n) => match dashboard::rate(&self.lib, &t, n) {
                Ok(()) => self.sel_dirty = true,
                Err(e) => self.msg(e, true),
            },
            Cmd::Reject(t) => match dashboard::reject(&self.lib, &t) {
                Ok(()) => self.sel_dirty = true,
                Err(e) => self.msg(e, true),
            },
            Cmd::RenderMedia(ts, palette) => {
                let targets: Vec<jetcarp_store::media::MediaTarget> = ts
                    .into_iter()
                    .map(|t| match t {
                        Target::Pattern(id) => jetcarp_store::media::MediaTarget::Pattern(id),
                    })
                    .collect();
                let path = self.path.clone();
                let shared = self.shared.clone();
                std::thread::spawn(move || {
                    let lib = Library::open(path);
                    let r = MediaStyle::with_palette(palette.as_deref()).and_then(|style| {
                        let o = MediaOptions {
                            style,
                            ..Default::default()
                        };
                        jetcarp_store::media::render_many(&lib, &targets, &o)
                    });
                    let m = match r {
                        Ok(r) => (
                            format!(
                                "Media: {} written, {} up to date, {} failed (under the catalog's media folder).",
                                r.written(),
                                r.skipped(),
                                r.failed.len()
                            ),
                            !r.failed.is_empty(),
                        ),
                        Err(e) => (e, true),
                    };
                    shared.lock().unwrap().messages.push(m);
                });
            }
            Cmd::Refresh => {
                self.sel_dirty = true;
            }
            Cmd::Shutdown(_) => unreachable!("handled by run"),
        }
    }

    fn refresh_jobs(&mut self) {
        // Reap jobs that ended here.
        for (job, r) in registry().reap(&self.lib) {
            if let Err(e) = r {
                self.msg(format!("Job {job}: {e}"), true);
            }
        }
        let rows = match self.lib.jobs() {
            Ok(r) => r,
            Err(e) => {
                self.msg(e, true);
                return;
            }
        };
        // Finish jobs that have paused.
        for job in self.finishing.clone() {
            if registry().contains(&self.lib, job) {
                continue;
            }
            if let Some(r) = rows.iter().find(|r| r.id == job) {
                match r.status {
                    JobStatus::Paused | JobStatus::Failed => {
                        let _ = self.lib.update_job(job, Some(JobStatus::Done), None, None);
                        self.finishing.remove(&job);
                        self.msg(format!("Job {job} stopped (marked done)."), false);
                    }
                    JobStatus::Done => {
                        self.finishing.remove(&job);
                    }
                    JobStatus::Running => {}
                }
            }
        }
        let mut sums = dashboard::summaries(&rows, dashboard::now_seconds());
        for s in &mut sums {
            if let Some(live) = registry().summary(&self.lib, s.id) {
                s.here = true;
                s.stale = false;
                if !live.status.is_empty() {
                    s.status = live.status;
                }
                if live.done > 0 || s.done == 0 {
                    s.done = live.done;
                    s.total = live.total.or(s.total);
                    s.per_s = live.per_s;
                    s.elapsed_s = live.elapsed_s;
                    s.eta_s = live.eta_s;
                    s.counters = live.counters;
                    s.bars = live.bars;
                    s.unit = live.unit;
                }
            }
        }
        let mut sh = self.shared.lock().unwrap();
        sh.jobs = sums;
        sh.loaded = true;
    }

    fn refresh_detail(&mut self) {
        let Some(sel) = self.sel.clone() else { return };
        let row = match self.lib.job(sel.job) {
            Ok(r) => r,
            Err(e) => {
                self.shared.lock().unwrap().detail = None;
                return self.msg(e, true);
            }
        };
        let fresh_enough = self.last_detail.is_some_and(|t| t.elapsed() < DETAIL_EVERY);
        if !self.sel_dirty && (fresh_enough || row.updated == self.detail_updated) {
            return;
        }
        self.sel_dirty = false;
        self.last_detail = Some(Instant::now());
        self.detail_updated = row.updated.clone();
        let kind = jobs::kind(&row.kind);
        let spaces = kind.heat_spaces(&row);
        let space = sel.space.min(spaces.len().saturating_sub(1));
        let mut error = None;
        let (map, heat_rows) = match spaces.get(space) {
            Some(sp) => match dashboard::load_heat(&self.lib, sp, sel.colour) {
                Ok((m, r)) => (Some(m), r),
                Err(e) => {
                    error = Some(e);
                    (None, vec![])
                }
            },
            None => (None, vec![]),
        };
        let finds = kind
            .finds(&self.lib, &row, &sel.rules, FINDS)
            .unwrap_or_else(|e| {
                error = Some(e);
                Finds::default()
            });
        let image = map.as_ref().map(heat_image);
        let marks = map
            .as_ref()
            .map(|m| {
                let n = m.rules.len();
                if n > 4000 {
                    return vec![];
                }
                m.rules
                    .keys()
                    .map(|&k| (k % m.w, k / m.w, m.values[k]))
                    .collect()
            })
            .unwrap_or_default();
        self.rev += 1;
        let d = Detail {
            job: sel.job,
            spaces: spaces.iter().map(|s| s.label()).collect(),
            space,
            rules: sel.rules,
            map,
            heat_rows,
            image,
            marks,
            finds,
            rev: self.rev,
            error,
        };
        self.shared.lock().unwrap().detail = Some(d);
    }

    fn run(mut self, rx: Receiver<Cmd>) {
        let mut next_jobs = Instant::now();
        loop {
            let wait = next_jobs.saturating_duration_since(Instant::now());
            match rx.recv_timeout(wait) {
                Ok(Cmd::Shutdown(ack)) => {
                    let here = registry().jobs(&self.lib);
                    for &j in &here {
                        if registry().is_running(&self.lib, j) {
                            self.pause(j);
                        }
                    }
                    for j in here {
                        let _ = registry().wait(&self.lib, j);
                    }
                    let _ = ack.send(());
                    return;
                }
                Ok(c) => {
                    self.handle(c);
                    // Take every queued command before reading.
                    while let Ok(c) = rx.try_recv() {
                        self.handle(c);
                    }
                    if self.sel_dirty {
                        self.refresh_detail();
                    }
                    next_jobs = Instant::now();
                }
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => {
                    for j in registry().jobs(&self.lib) {
                        registry().stop(&self.lib, j);
                    }
                    return;
                }
            }
            if Instant::now() >= next_jobs {
                self.refresh_jobs();
                self.refresh_detail();
                next_jobs = Instant::now() + JOBS_EVERY;
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Animations of finds

type AnimResult = (String, Result<(Vec<ColorImage>, u32), String>);

struct AnimQueue {
    /// (cache key, find, palette)
    items: Mutex<Vec<(String, Target, Option<String>)>>,
    ready: Condvar,
}

struct Anim {
    frames: Vec<TextureHandle>,
    delay_ms: u32,
    used: u64,
}

/// Animations of finds, made on worker threads for what is on screen, at most `capacity` kept.
struct AnimCache {
    map: HashMap<String, Anim>,
    pending: HashSet<String>,
    failed: HashSet<String>,
    queue: Arc<AnimQueue>,
    rx: Receiver<AnimResult>,
    frame: u64,
    capacity: usize,
    /// The palette the finds are drawn in (`None`: the house style).
    palette: Option<String>,
}

const ANIM_SIDE: u32 = 120;
const ANIM_FRAMES: usize = 48;

fn small_style(palette: Option<&str>) -> Result<MediaStyle, String> {
    Ok(MediaStyle {
        alone_side: ANIM_SIDE,
        seed_side: ANIM_SIDE,
        max_cell_px: 8.0,
        ..MediaStyle::with_palette(palette)?
    })
}

/// A find's frames in memory, at most [`ANIM_FRAMES`] (evenly picked) of at most [`ANIM_SIDE`]
/// pixels.
fn make_anim(
    lib: &Library,
    t: &Target,
    palette: Option<&str>,
) -> Result<(Vec<ColorImage>, u32), String> {
    let o = MediaOptions {
        style: small_style(palette)?,
        frames: false,
        ..Default::default()
    };
    let anim = match t {
        Target::Pattern(id) => media::pattern_animation(&lib.get(id)?, &o)?,
    };
    let n = anim.frames.len();
    let pick: Vec<usize> = if n <= ANIM_FRAMES {
        (0..n).collect()
    } else {
        (0..ANIM_FRAMES).map(|k| k * n / ANIM_FRAMES).collect()
    };
    let step = (n as f64 / pick.len().max(1) as f64).max(1.0);
    Ok((
        pick.into_iter()
            .map(|k| thumbs::color_image(&anim.frames[k], ANIM_SIDE))
            .collect(),
        (anim.delay_ms as f64 * step).round() as u32,
    ))
}

fn key_of(t: &Target) -> String {
    match t {
        Target::Pattern(id) => format!("p:{id}"),
    }
}

impl AnimCache {
    fn new(path: PathBuf, threads: usize) -> AnimCache {
        let queue = Arc::new(AnimQueue {
            items: Mutex::new(vec![]),
            ready: Condvar::new(),
        });
        let (tx, rx) = channel::<AnimResult>();
        for k in 0..threads.max(1) {
            let q = queue.clone();
            let tx = tx.clone();
            let path = path.clone();
            let _ = std::thread::Builder::new()
                .name(format!("jetcarp-dash-anim-{k}"))
                .spawn(move || {
                    let lib = Library::open(path);
                    loop {
                        let (key, t, pal) = {
                            let mut items = q.items.lock().unwrap();
                            loop {
                                if let Some(p) = items.pop() {
                                    break p;
                                }
                                items = q.ready.wait(items).unwrap();
                            }
                        };
                        if tx.send((key, make_anim(&lib, &t, pal.as_deref()))).is_err() {
                            return;
                        }
                    }
                });
        }
        AnimCache {
            map: HashMap::new(),
            pending: HashSet::new(),
            failed: HashSet::new(),
            queue,
            rx,
            frame: 0,
            capacity: 64,
            palette: None,
        }
    }

    /// A find's cache key in the current palette.
    fn key(&self, t: &Target) -> String {
        format!("{}|{}", key_of(t), self.palette.as_deref().unwrap_or(""))
    }

    /// Draw the finds in another palette from now on.
    fn set_palette(&mut self, p: Option<String>) {
        if self.palette != p {
            self.palette = p;
            self.map.clear();
            self.failed.clear();
        }
    }

    /// Upload finished animations, forget requests no longer on screen, trim. True while work
    /// is in flight.
    fn begin_frame(&mut self, ctx: &egui::Context) -> bool {
        self.frame += 1;
        for (k, ..) in self.queue.items.lock().unwrap().drain(..) {
            self.pending.remove(&k);
        }
        while let Ok((key, r)) = self.rx.try_recv() {
            self.pending.remove(&key);
            match r {
                Ok((frames, delay)) if !frames.is_empty() => {
                    let tex = frames
                        .into_iter()
                        .enumerate()
                        .map(|(i, f)| {
                            ctx.load_texture(
                                format!("dash-{key}-{i}"),
                                f,
                                egui::TextureOptions::NEAREST,
                            )
                        })
                        .collect();
                    self.map.insert(
                        key,
                        Anim {
                            frames: tex,
                            delay_ms: delay.max(20),
                            used: self.frame,
                        },
                    );
                }
                _ => {
                    self.failed.insert(key);
                }
            }
        }
        if self.map.len() > self.capacity {
            let mut by: Vec<(u64, String)> =
                self.map.iter().map(|(k, a)| (a.used, k.clone())).collect();
            by.sort();
            for (_, k) in by.into_iter().take(self.map.len() - self.capacity) {
                self.map.remove(&k);
            }
        }
        !self.pending.is_empty()
    }

    /// The frame to show now, or `None` while it is being made.
    fn frame_at(&mut self, t: &Target, time: f64) -> Option<TextureHandle> {
        let key = self.key(t);
        if let Some(a) = self.map.get_mut(&key) {
            a.used = self.frame;
            let i = ((time * 1000.0 / a.delay_ms as f64) as usize) % a.frames.len();
            return Some(a.frames[i].clone());
        }
        if !self.failed.contains(&key) && self.pending.insert(key.clone()) {
            self.queue
                .items
                .lock()
                .unwrap()
                .push((key, t.clone(), self.palette.clone()));
            self.queue.ready.notify_one();
        }
        None
    }

    fn failed(&self, t: &Target) -> bool {
        self.failed.contains(&self.key(t))
    }

    /// Forget a find's animation (its pattern changed).
    fn forget(&mut self, t: &Target) {
        let k = self.key(t);
        self.map.remove(&k);
        self.failed.remove(&k);
    }
}

// ---------------------------------------------------------------------------------------------
// The window

pub struct DashWindow {
    pub open: bool,
    path: PathBuf,
    shared: Arc<Mutex<Shared>>,
    tx: Option<Sender<Cmd>>,
    selected: Option<i64>,
    space: usize,
    colour: HeatColour,
    /// The user (or `JETCARP_DASHBOARD_COLOUR`) chose the colouring.
    colour_fixed: bool,
    rules: Vec<String>,
    pub form: HuntForm,
    form_open: bool,
    form_error: Option<String>,
    heat_tex: Option<(u64, bool, TextureHandle)>,
    anims: Option<AnimCache>,
    messages: Vec<(String, bool, Instant)>,
    /// `JETCARP_DASHBOARD_SELECT` and friends, applied once the job list arrives.
    pub pending_select: Option<i64>,
    /// Palette of the finds' animations and of *Render media* (`None`: the house style; saved
    /// in the settings).
    pub palette: Option<String>,
    /// Imported packs' feed names, by manifest (read once each from the library).
    pack_names: HashMap<String, String>,
}

impl Default for DashWindow {
    fn default() -> Self {
        DashWindow {
            open: false,
            path: Library::default_path(),
            shared: Default::default(),
            tx: None,
            selected: None,
            space: 0,
            colour: HeatColour::Finds,
            colour_fixed: false,
            rules: vec![],
            form: HuntForm::default(),
            form_open: false,
            form_error: None,
            heat_tex: None,
            anims: None,
            messages: vec![],
            pending_select: None,
            palette: None,
            pack_names: HashMap::new(),
        }
    }
}

impl DashWindow {
    /// An imported pack's feed name (`jetcarp-starter`), from its manifest; the manifest itself
    /// when the library has no record of the import.
    fn pack_name(&mut self, manifest: &str) -> String {
        if let Some(n) = self.pack_names.get(manifest) {
            return n.clone();
        }
        let n = Library::open(&self.path)
            .pack_imports()
            .ok()
            .and_then(|v| {
                v.into_iter()
                    .find(|r| r.manifest == manifest || manifest.ends_with(r.manifest.as_str()))
            })
            .map(|r| r.feed)
            .filter(|f| !f.is_empty())
            .unwrap_or_else(|| manifest.to_string());
        self.pack_names.insert(manifest.to_string(), n.clone());
        n
    }

    fn send(&mut self, c: Cmd) {
        if self.tx.is_none() {
            let (tx, rx) = channel();
            let lib = Library::open(&self.path);
            let w = Worker {
                cx: window_job::ctx_on(lib.clone()),
                lib,
                path: self.path.clone(),
                shared: self.shared.clone(),
                finishing: HashSet::new(),
                sel: None,
                sel_dirty: false,
                last_detail: None,
                detail_updated: String::new(),
                rev: 0,
                no_gpu: false,
            };
            std::thread::Builder::new()
                .name("jetcarp-dashboard".into())
                .spawn(move || w.run(rx))
                .ok();
            self.tx = Some(tx);
            self.anims = Some(AnimCache::new(self.path.clone(), 2));
        }
        if let Some(tx) = &self.tx {
            let _ = tx.send(c);
        }
    }

    /// `JETCARP_DASHBOARD=1|<job>` (headless checks): open the window, selecting that job.
    /// `JETCARP_DASHBOARD_HUNT=<rules>@<tilings>@<soups>` also starts a hunt here.
    pub fn open_env(&mut self, what: &str) {
        self.open = true;
        if let Ok(j) = what.trim_start_matches("job:").parse::<i64>()
            && what != "1"
        {
            self.pending_select = Some(j);
        }
        if let Ok(h) = std::env::var("JETCARP_DASHBOARD_HUNT") {
            let p: Vec<&str> = h.split('@').collect();
            self.form.rules = p.first().copied().unwrap_or("B3/S23").into();
            self.form.tilings = p.get(1).copied().unwrap_or("square").into();
            match p.get(2).copied() {
                Some("forever") => self.form.forever = true,
                Some(n) => self.form.soups = n.parse().unwrap_or(1000),
                None => {}
            }
            self.start_hunt();
        }
        if let Ok(c) = std::env::var("JETCARP_DASHBOARD_COLOUR") {
            for (k, name) in HeatColour::ALL {
                if c == name || c == format!("{k:?}").to_lowercase() {
                    self.colour = k;
                    self.colour_fixed = true;
                }
            }
        }
        self.send(Cmd::Refresh);
    }

    /// At exit: pause the jobs running here at their next checkpoint (so they are left `paused`,
    /// not `running`), waiting at most `wait`.
    pub fn shutdown(&mut self, wait: Duration) {
        if let Some(tx) = self.tx.take() {
            let (ack, rx) = channel();
            if tx.send(Cmd::Shutdown(ack)).is_ok() {
                let _ = rx.recv_timeout(wait);
            }
        }
    }

    /// Whether any job runs in this process (for the top bar's dot).
    pub fn busy(&self) -> bool {
        self.shared
            .lock()
            .unwrap()
            .jobs
            .iter()
            .any(|j| j.here && j.status == "running")
    }

    fn start_hunt(&mut self) {
        match self.form.spec() {
            Ok(_) => {
                self.form_error = None;
                let (gpu, threads) = (self.form.gpu, self.form.threads);
                self.send(Cmd::Start(Box::new(self.form.input()), gpu, threads));
                self.form_open = false;
            }
            Err(e) => self.form_error = Some(e),
        }
    }

    /// Select a job: its first rule space, no rule filter, and finds as the colouring unless one
    /// was chosen.
    fn pick(&mut self, j: &JobSummary) {
        self.selected = Some(j.id);
        self.space = 0;
        self.rules.clear();
        if !self.colour_fixed {
            self.colour = HeatColour::Finds;
        }
        self.select();
    }

    fn select(&mut self) {
        if let Some(job) = self.selected {
            self.send(Cmd::Select(Selection {
                job,
                space: self.space,
                colour: self.colour,
                rules: self.rules.clone(),
            }));
        }
    }

    pub fn show(&mut self, ctx: &egui::Context) {
        if !self.open {
            if self.tx.is_some() && self.busy() {
                ctx.request_repaint_after(Duration::from_secs(1));
            }
            return;
        }
        if self.tx.is_none() {
            self.send(Cmd::Refresh);
        }
        {
            let mut sh = self.shared.lock().unwrap();
            for (m, e) in sh.messages.drain(..) {
                self.messages.push((m, e, Instant::now()));
            }
            if let Some(j) = sh.started.take() {
                self.pending_select = Some(j);
            }
        }
        self.messages.retain(|m| m.2.elapsed().as_secs() < 12);
        if let Some(a) = self.anims.as_mut() {
            a.set_palette(self.palette.clone());
        }
        let anim_busy = self.anims.as_mut().is_some_and(|a| a.begin_frame(ctx));
        ctx.request_repaint_after(Duration::from_millis(if anim_busy { 30 } else { 80 }));
        let mut open = self.open;
        let screen = ctx.content_rect();
        crate::popout::tool_window(
            ctx,
            crate::popout::Spec::new(KEY, "Hunts and searches")
                .size(1300.0, 900.0)
                .configure(|w| {
                    w.default_pos(screen.min + egui::vec2(60.0, 50.0))
                        .default_size([
                            (screen.width() * 0.9).min(1500.0),
                            (screen.height() * 0.88).min(1000.0),
                        ])
                        .min_size([520.0, 360.0])
                        .resizable(true)
                }),
            &mut open,
            |ui| self.contents(ui),
        );
        self.open = open;
    }

    fn contents(&mut self, ui: &mut egui::Ui) {
        let (jobs, loaded) = {
            let sh = self.shared.lock().unwrap();
            (sh.jobs.clone(), sh.loaded)
        };
        if let Some(j) = self.pending_select
            && let Some(s) = jobs.iter().find(|x| x.id == j)
        {
            self.pending_select = None;
            self.pick(s);
        }
        if self.selected.is_none()
            && let Some(first) = jobs.first()
        {
            self.pick(first);
        }
        ui.horizontal_wrapped(|ui| {
            ui.label(
                RichText::new(format!(
                    "{} jobs in {} (read every {} s; jobs in other processes show too)",
                    jobs.len(),
                    self.path.display(),
                    JOBS_EVERY.as_secs()
                ))
                .weak(),
            );
            if ui
                .button(if self.form_open {
                    "Hide the form"
                } else {
                    "Start a hunt…"
                })
                .clicked()
            {
                self.form_open = !self.form_open;
            }
            if ui.button("Refresh").clicked() {
                self.send(Cmd::Refresh);
            }
        });
        for (m, err, _) in &self.messages {
            ui.colored_label(
                if *err {
                    Color32::LIGHT_RED
                } else {
                    Color32::LIGHT_GREEN
                },
                m,
            );
        }
        if self.form_open {
            self.form_ui(ui);
        }
        ui.separator();
        if !loaded {
            ui.label("Reading the catalog…");
            return;
        }
        if jobs.is_empty() {
            thumbs::empty_note(
                ui,
                "No jobs yet. Start a hunt here, or run `jetcarp search hunt` in a terminal.",
            );
            return;
        }
        self.job_list(ui, &jobs);
        ui.separator();
        if let Some(s) = jobs.iter().find(|j| Some(j.id) == self.selected) {
            self.detail(ui, s);
        }
    }

    fn form_ui(&mut self, ui: &mut egui::Ui) {
        let f = &mut self.form;
        egui::Grid::new("dash-form")
            .num_columns(4)
            .spacing([10.0, 4.0])
            .show(ui, |ui| {
                ui.label("Rules");
                ui.add(egui::TextEdit::singleline(&mut f.rules).desired_width(260.0))
                    .on_hover_text("B/S or Generations rules, separated by commas or spaces: B3/S23, B36/S23, 23/3/4");
                ui.label("Tilings");
                ui.add(egui::TextEdit::singleline(&mut f.tilings).desired_width(200.0))
                    .on_hover_text("Periodic tiling ids: square, hex, triangle, snubsq, k3-012, ih41, ...");
                ui.end_row();
                ui.label("Soup");
                ui.horizontal(|ui| {
                    ui.add(egui::DragValue::new(&mut f.w).range(1..=256).suffix(" w"));
                    ui.add(egui::DragValue::new(&mut f.h).range(1..=256).suffix(" h"));
                    ui.label("density");
                    ui.add(egui::DragValue::new(&mut f.density).range(0.0..=1.0).speed(0.01));
                });
                ui.label("Neighbours");
                ui.horizontal(|ui| {
                    ui.selectable_value(&mut f.nb, 0, "from the rule");
                    ui.selectable_value(&mut f.nb, 1, "touch");
                    ui.selectable_value(&mut f.nb, 2, "edge");
                });
                ui.end_row();
                ui.label("Soups");
                ui.horizontal(|ui| {
                    ui.add_enabled(
                        !f.forever,
                        egui::DragValue::new(&mut f.soups).range(1..=u64::MAX).speed(100.0),
                    );
                    ui.checkbox(&mut f.forever, "forever (until stopped)");
                });
                ui.label("Seed");
                ui.horizontal(|ui| {
                    ui.add(egui::DragValue::new(&mut f.seed));
                    ui.label("max generations");
                    ui.add(egui::DragValue::new(&mut f.max_gens).range(1..=10_000_000).speed(100.0));
                });
                ui.end_row();
                ui.label("Run");
                ui.horizontal(|ui| {
                    ui.checkbox(&mut f.gpu, "GPU batches");
                    ui.label("threads");
                    ui.add(egui::DragValue::new(&mut f.threads).range(0..=512))
                        .on_hover_text("0 = all cores");
                });
                ui.end_row();
            });
        ui.horizontal(|ui| {
            if ui.button(RichText::new("Start hunt").strong()).clicked() {
                self.start_hunt();
            }
            ui.label(
                RichText::new(
                    "Runs here on a background thread; pause it, close JETCARP, and resume it here or with `jetcarp jobs resume JOB`.",
                )
                .small()
                .weak(),
            );
        });
        if let Some(e) = &self.form_error {
            ui.colored_label(Color32::LIGHT_RED, e);
        }
    }

    fn job_list(&mut self, ui: &mut egui::Ui, jobs: &[JobSummary]) {
        let mut cmd = None;
        let mut pick = None;
        let (gpu, threads) = (self.form.gpu, self.form.threads);
        egui::ScrollArea::vertical()
            .id_salt("dash-jobs")
            .max_height(170.0)
            .auto_shrink([false, true])
            .show(ui, |ui| {
                egui::Grid::new("dash-job-grid")
                    .num_columns(8)
                    .striped(true)
                    .spacing([12.0, 4.0])
                    .show(ui, |ui| {
                        for h in [
                            "Job", "Kind", "Status", "What", "Progress", "Speed", "Time", "",
                        ] {
                            ui.label(RichText::new(h).small().strong());
                        }
                        ui.end_row();
                        for j in jobs {
                            let sel = self.selected == Some(j.id);
                            if ui.selectable_label(sel, format!("#{}", j.id)).clicked() {
                                pick = Some(j.id);
                            }
                            ui.label(&j.kind_label);
                            let (txt, col) = status_label(j);
                            ui.colored_label(col, txt);
                            // A pack's hunt may name no rules of its own.
                            let title = if j.title.is_empty() && j.imported_from.is_some() {
                                "from a pack".to_string()
                            } else {
                                j.title.clone()
                            };
                            let r = ui
                                .add(egui::Label::new(truncate(&title, 60)).sense(Sense::click()));
                            if r.clicked() {
                                pick = Some(j.id);
                            }
                            r.on_hover_text(&title);
                            match j.fraction() {
                                // A pack's job: nothing ran here, so no count.
                                _ if j.imported_from.is_some() => {
                                    ui.label("imported");
                                }
                                Some(f) => {
                                    ui.add(
                                        egui::ProgressBar::new(f as f32).desired_width(160.0).text(
                                            format!(
                                                "{} / {}",
                                                dashboard::digits(j.done),
                                                dashboard::digits(j.total.unwrap_or(0))
                                            ),
                                        ),
                                    );
                                }
                                None => {
                                    ui.label(format!("{} {}", dashboard::digits(j.done), j.unit));
                                }
                            }
                            ui.label(if j.per_s > 0.0 && j.is_running() {
                                format!("{:.0}/s", j.per_s)
                            } else {
                                "–".into()
                            });
                            ui.label(match j.eta_s {
                                _ if j.imported_from.is_some() => "–".into(),
                                Some(e) => format!(
                                    "{} run, {} left",
                                    dashboard::duration(j.elapsed_s),
                                    dashboard::duration(e)
                                ),
                                None => format!("{} run", dashboard::duration(j.elapsed_s)),
                            });
                            ui.horizontal(|ui| {
                                if (j.is_running() || (j.here && j.status == "running"))
                                    && ui
                                        .small_button("Pause")
                                        .on_hover_text(if j.here {
                                            "Stop at the next batch; resume later"
                                        } else {
                                            "Ask the process running it to stop at its next batch"
                                        })
                                        .clicked()
                                {
                                    cmd = Some(Cmd::Pause(j.id));
                                }
                                let (label, hover) = resume_button(j);
                                if j.can_resume()
                                    && ui.small_button(label).on_hover_text(hover).clicked()
                                {
                                    cmd = Some(Cmd::Resume(j.id, gpu, threads));
                                }
                                if j.status != "done"
                                    && ui
                                        .small_button("Stop")
                                        .on_hover_text(
                                            "Pause it, then mark it done (no more resumes)",
                                        )
                                        .clicked()
                                {
                                    cmd = Some(Cmd::Stop(j.id));
                                }
                            });
                            ui.end_row();
                        }
                    });
            });
        if let Some(c) = cmd {
            self.send(c);
        }
        if let Some(j) = pick
            && self.selected != Some(j)
            && let Some(s) = jobs.iter().find(|x| x.id == j)
        {
            self.pick(s);
        }
    }

    fn detail(&mut self, ui: &mut egui::Ui, j: &JobSummary) {
        ui.horizontal_wrapped(|ui| {
            ui.label(
                RichText::new(format!("#{} {}", j.id, j.kind_label))
                    .strong()
                    .size(15.0),
            );
            let (txt, col) = status_label(j);
            ui.colored_label(col, txt);
            ui.label(RichText::new(&j.title).weak());
        });
        if j.resumable && !j.deterministic {
            ui.label(
                RichText::new(
                    "Not deterministic: paused, it continues from its saved archive (not exactly as an uninterrupted run would).",
                )
                .small()
                .weak(),
            );
        }
        if let Some(e) = &j.error {
            ui.colored_label(Color32::LIGHT_RED, e);
        }
        if let Some(pack) = &j.imported_from {
            let name = self.pack_name(pack);
            ui.label(
                RichText::new(format!(
                    "Imported from pack {name}: its finds came with the pack, and nothing ran here."
                ))
                .weak(),
            );
        }
        // Counters (none for a job that never ran here).
        if j.imported_from.is_none() {
            ui.horizontal_wrapped(|ui| {
                let tile = |ui: &mut egui::Ui, k: &str, v: String| {
                    egui::Frame::group(ui.style()).show(ui, |ui| {
                        ui.vertical(|ui| {
                            ui.label(RichText::new(v).strong().size(15.0));
                            ui.label(RichText::new(k).small().weak());
                        });
                    });
                };
                tile(
                    ui,
                    &format!("{} done", j.unit),
                    match j.total {
                        Some(t) => {
                            format!("{} / {}", dashboard::digits(j.done), dashboard::digits(t))
                        }
                        None => dashboard::digits(j.done),
                    },
                );
                tile(
                    ui,
                    "per second",
                    if j.is_running() {
                        format!("{:.1}", j.per_s)
                    } else {
                        format!("({:.1})", j.per_s)
                    },
                );
                tile(ui, "elapsed", dashboard::duration(j.elapsed_s));
                tile(
                    ui,
                    "left",
                    match (j.eta_s, j.total) {
                        (Some(e), _) => dashboard::duration(e),
                        (None, None) if j.is_running() => "runs until stopped".into(),
                        _ => "–".into(),
                    },
                );
                for (k, v) in &j.counters {
                    tile(ui, k, v.clone());
                }
            });
        }
        if !j.bars.is_empty() {
            ui.label(RichText::new(&j.bars_title).small().weak());
            bars(ui, &j.bars);
        }
        let detail = self.shared.lock().unwrap().detail.take();
        let Some(d) = detail else {
            ui.label(RichText::new("Reading the heat map and finds…").weak());
            return;
        };
        if d.job != j.id {
            self.shared.lock().unwrap().detail = Some(d);
            ui.label(RichText::new("Reading the heat map and finds…").weak());
            return;
        }
        let mut reselect = false;
        let mut cmd = None;
        ui.columns(2, |cols| {
            reselect |= self.heat_ui(&mut cols[0], &d);
            cmd = self.finds_ui(&mut cols[1], &d);
        });
        self.shared.lock().unwrap().detail = Some(d);
        if let Some(c) = cmd {
            self.send(c);
        }
        if reselect {
            self.select();
        }
    }

    /// The heat map; true when the selection changed.
    fn heat_ui(&mut self, ui: &mut egui::Ui, d: &Detail) -> bool {
        let mut changed = false;
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new("Rule space").strong());
            if d.spaces.len() > 1 {
                egui::ComboBox::from_id_salt("dash-space")
                    .selected_text(d.spaces.get(d.space).cloned().unwrap_or_default())
                    .show_ui(ui, |ui| {
                        for (i, s) in d.spaces.iter().enumerate() {
                            if ui.selectable_label(i == d.space, s).clicked() {
                                self.space = i;
                                changed = true;
                            }
                        }
                    });
            } else if let Some(s) = d.spaces.first() {
                ui.label(s);
            }
            ui.label("colour by");
            for (k, name) in HeatColour::ALL {
                if ui.selectable_label(self.colour == k, name).clicked() && self.colour != k {
                    self.colour = k;
                    self.colour_fixed = true;
                    changed = true;
                }
            }
        });
        let Some(map) = &d.map else {
            ui.label(
                RichText::new(if d.spaces.is_empty() {
                    "This kind of job has no B/S rule space to map."
                } else {
                    "No B/S rules on this tiling yet."
                })
                .weak(),
            );
            return changed;
        };
        // Lay the map out wide when the column is wide: survival across, birth down.
        let avail_w = ui.available_width() - 40.0;
        let avail_h = (ui.available_height() - 70.0).clamp(160.0, 900.0);
        let fit = |w: usize, h: usize| (avail_w / w as f32).min(avail_h / h as f32);
        let tr = fit(map.h, map.w) > fit(map.w, map.h);
        if self
            .heat_tex
            .as_ref()
            .is_none_or(|t| t.0 != d.rev || t.1 != tr)
            && let Some(img) = &d.image
        {
            let img = if tr { transpose(img) } else { img.clone() };
            self.heat_tex = Some((
                d.rev,
                tr,
                ui.ctx()
                    .load_texture("dash-heat", img, egui::TextureOptions::NEAREST),
            ));
        }
        let Some((_, _, tex)) = &self.heat_tex else {
            return changed;
        };
        let (dw, dh) = if tr { (map.h, map.w) } else { (map.w, map.h) };
        let k = fit(dw, dh).max(0.05);
        let size = egui::vec2(dw as f32 * k, dh as f32 * k);
        // Display position of map cell (x, y), and the cell under a display position.
        let at = |x: usize, y: usize| if tr { (y, x) } else { (x, y) };
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.add_space(size.y / 2.0 - 20.0);
                ui.label(RichText::new(if tr { "B" } else { "S" }).small().weak());
            });
            let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
            let p = ui.painter_at(rect);
            p.image(
                tex.id(),
                rect,
                Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                Color32::WHITE,
            );
            let centre = |x: usize, y: usize| {
                let (u, v) = at(x, y);
                rect.min + egui::vec2((u as f32 + 0.5) * k, (v as f32 + 0.5) * k)
            };
            // Markers so single rules show at any zoom.
            if k < 3.0 {
                let m = if k < 1.5 { 4.0 } else { 3.0 };
                for &(x, y, v) in &d.marks {
                    let [r, g, b] = ramp(0.15 + 0.85 * heat_t(v, map.max));
                    p.rect_filled(
                        Rect::from_center_size(centre(x, y), egui::vec2(m, m)),
                        0.0,
                        Color32::from_rgb(r, g, b),
                    );
                }
            }
            // The filtered cell.
            for rule in &d.rules {
                if let Some(r) = d.heat_rows.iter().find(|r| &r.notation == rule) {
                    let (x, y) = dashboard::heat_cell(r.birth, r.survive, map.degree);
                    p.circle_stroke(centre(x, y), 6.0, egui::Stroke::new(1.5, Color32::WHITE));
                }
            }
            if let Some(pos) = resp.hover_pos() {
                let u = ((pos.x - rect.left()) / k) as usize;
                let v = ((pos.y - rect.top()) / k) as usize;
                let (x, y) = at(u, v);
                let (x, y) = (x.min(map.w - 1), y.min(map.h - 1));
                let here = map.rules_at(x, y);
                let (b, s) = dashboard::heat_masks(x, y, map.degree);
                let text = if here.is_empty() {
                    format!(
                        "{} : nothing",
                        Rule::LifeLike {
                            birth: b,
                            survive: s
                        }
                        .notation()
                    )
                } else {
                    let names: Vec<String> = here
                        .iter()
                        .take(6)
                        .map(|&i| d.heat_rows[i].notation.clone())
                        .collect();
                    format!(
                        "{}{} : {} {}",
                        names.join(", "),
                        if here.len() > 6 { " …" } else { "" },
                        fmt_value(map.at(x, y)),
                        map.colour.label()
                    )
                };
                resp.clone()
                    .on_hover_text(format!("{text}\nClick to show this rule's finds"));
                if resp.clicked() {
                    let rules: Vec<String> = here
                        .iter()
                        .map(|&i| d.heat_rows[i].notation.clone())
                        .collect();
                    if rules == self.rules {
                        self.rules.clear();
                    } else {
                        self.rules = rules;
                    }
                    changed = true;
                }
            }
        });
        let (bn, sn) = dashboard::heat_bits(map.degree);
        let axes = if tr {
            format!("Survival S0..S{} across, birth B1..B{bn} down", sn - 1)
        } else {
            format!("Birth B1..B{bn} across, survival S0..S{} down", sn - 1)
        };
        ui.label(
            RichText::new(format!(
                "{axes}, each in Gray-code order: neighbours differ by one count. {} rules, max {}{}",
                dashboard::digits(d.heat_rows.len() as u64),
                fmt_value(map.max),
                if map.folded {
                    "; counts above 8 folded onto the first eight"
                } else {
                    ""
                }
            ))
            .small()
            .weak(),
        );
        changed
    }

    fn finds_ui(&mut self, ui: &mut egui::Ui, d: &Detail) -> Option<Cmd> {
        let mut cmd = None;
        if let Some(e) = &d.error {
            ui.colored_label(Color32::LIGHT_RED, e);
        }
        ui.horizontal_wrapped(|ui| {
            if d.rules.is_empty() {
                ui.label(RichText::new("Finds").strong());
            } else {
                ui.label(RichText::new(format!("Finds in {}", d.rules.join(", "))).strong());
                if ui.small_button("All rules").clicked() {
                    self.rules.clear();
                    cmd = Some(Cmd::Refresh);
                }
            }
            let targets: Vec<Target> = d
                .finds
                .newest
                .iter()
                .chain(&d.finds.other)
                .map(|f| f.target.clone())
                .collect();
            if !targets.is_empty()
                && ui
                    .small_button("Render media")
                    .on_hover_text("Write frames, an animation, a poster, and a thumbnail for these finds (media module), in the background, in the colours picked here")
                    .clicked()
            {
                cmd = Some(Cmd::RenderMedia(targets, self.palette.clone()));
            }
            ui.label("Colours");
            crate::palette_pick::picker_opt(
                ui,
                "dash-palette",
                &mut self.palette,
                &[],
                "House style",
            );
        });
        let time = ui.input(|i| i.time);
        for (title, list) in [
            ("Newest", &d.finds.newest),
            (d.finds.other_title.as_str(), &d.finds.other),
        ] {
            ui.label(RichText::new(title).small().strong());
            if list.is_empty() {
                ui.label(RichText::new("None yet.").weak());
                continue;
            }
            egui::ScrollArea::horizontal()
                .id_salt(("dash-finds", title))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        for f in list {
                            if let Some(c) = self.find_card(ui, f, time) {
                                cmd = Some(c);
                            }
                        }
                    });
                });
        }
        cmd
    }

    fn find_card(&mut self, ui: &mut egui::Ui, f: &FindItem, time: f64) -> Option<Cmd> {
        let mut cmd = None;
        let w = 132.0;
        egui::Frame::group(ui.style()).show(ui, |ui| {
            ui.set_width(w);
            ui.vertical(|ui| {
                let (rect, _) = ui.allocate_exact_size(egui::vec2(w, w * 0.8), Sense::hover());
                let anims = self.anims.as_mut();
                let tex = anims.and_then(|a| a.frame_at(&f.target, time));
                let p = ui.painter_at(rect);
                p.rect_filled(rect, 2.0, Color32::from_gray(20));
                match tex {
                    Some(t) => {
                        let s = t.size_vec2();
                        let k = (rect.width() / s.x).min(rect.height() / s.y);
                        p.image(
                            t.id(),
                            Rect::from_center_size(rect.center(), s * k),
                            Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                            Color32::WHITE,
                        );
                    }
                    None => {
                        let failed = self.anims.as_ref().is_some_and(|a| a.failed(&f.target));
                        p.text(
                            rect.center(),
                            egui::Align2::CENTER_CENTER,
                            if failed { "no picture" } else { "…" },
                            egui::FontId::proportional(12.0),
                            Color32::from_gray(110),
                        );
                    }
                }
                ui.add(egui::Label::new(RichText::new(&f.title).small().strong()).truncate())
                    .on_hover_text(&f.title);
                ui.add(egui::Label::new(RichText::new(&f.subtitle).small().weak()).truncate())
                    .on_hover_text(&f.subtitle);
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 0.0;
                    let cur = f.rating.unwrap_or(0);
                    for k in 1..=5u8 {
                        let (r, resp) = ui.allocate_exact_size(egui::vec2(15.0, 15.0), Sense::click());
                        let on = k <= cur || (resp.hovered());
                        thumbs::star(
                            ui.painter(),
                            r.center(),
                            6.0,
                            k <= cur,
                            if on {
                                Color32::from_rgb(0xff, 0xc8, 0x3c)
                            } else {
                                Color32::from_gray(90)
                            },
                        );
                        if resp.on_hover_text(format!("Rate {k} of 5")).clicked() {
                            cmd = Some(Cmd::Rate(f.target.clone(), k));
                        }
                    }
                    ui.add_space(8.0);
                    if ui
                        .small_button("Reject")
                        .on_hover_text("Curation state 'rejected': hidden from these lists, the Quick Start, and the website")
                        .clicked()
                    {
                        if let Some(a) = self.anims.as_mut() {
                            a.forget(&f.target);
                        }
                        cmd = Some(Cmd::Reject(f.target.clone()));
                    }
                });
            });
        });
        cmd
    }
}

/// An image with rows and columns swapped.
pub fn transpose(img: &ColorImage) -> ColorImage {
    let [w, h] = img.size;
    let mut px = Vec::with_capacity(w * h);
    for x in 0..w {
        for y in 0..h {
            px.push(img.pixels[y * w + x]);
        }
    }
    ColorImage {
        size: [h, w],
        source_size: egui::vec2(h as f32, w as f32),
        pixels: px,
    }
}

fn fmt_value(v: f64) -> String {
    dashboard::digits(v as u64)
}

fn truncate(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        s.to_string()
    } else {
        format!("{}…", s.chars().take(n).collect::<String>())
    }
}

/// Status text and colour (stalled jobs say so).
/// The button that carries a paused job on here, and what it says on hover: *Resume here* for
/// a deterministic job, *Continue here* for one that is not (an explore search).
fn resume_button(j: &JobSummary) -> (&'static str, &'static str) {
    if j.deterministic {
        (
            "Resume here",
            "Run it in this process from its last checkpoint; it ends exactly as an uninterrupted run",
        )
    } else {
        (
            "Continue here",
            "Carry the search on in this process from its saved archive (it is not deterministic: it continues, not exactly as an uninterrupted run)",
        )
    }
}

fn status_label(j: &JobSummary) -> (String, Color32) {
    if j.stale {
        return ("stalled".into(), Color32::from_rgb(0xff, 0x8a, 0x3c));
    }
    let here = if j.here { " (here)" } else { "" };
    let col = match j.status.as_str() {
        "running" => Color32::LIGHT_GREEN,
        "paused" => Color32::from_rgb(0xff, 0xc8, 0x3c),
        "failed" => Color32::LIGHT_RED,
        _ => Color32::from_gray(160),
    };
    (format!("{}{here}", j.status), col)
}

/// Horizontal bars with labels and counts.
fn bars(ui: &mut egui::Ui, b: &[(String, u64)]) {
    let max = b.iter().map(|x| x.1).max().unwrap_or(0).max(1) as f64;
    let w = (ui.available_width() / b.len().max(1) as f32).clamp(50.0, 140.0);
    ui.horizontal(|ui| {
        for (k, v) in b {
            let (rect, resp) = ui.allocate_exact_size(egui::vec2(w - 4.0, 34.0), Sense::hover());
            let p = ui.painter_at(rect);
            let t = ((1.0 + *v as f64).ln() / (1.0 + max).ln()) as f32;
            let bar = Rect::from_min_size(
                rect.min + egui::vec2(0.0, 18.0),
                egui::vec2((rect.width() * t).max(1.0), 12.0),
            );
            p.rect_filled(
                Rect::from_min_size(
                    rect.min + egui::vec2(0.0, 18.0),
                    egui::vec2(rect.width(), 12.0),
                ),
                2.0,
                Color32::from_gray(32),
            );
            let [r, g, bb] = ramp(0.25 + 0.6 * t as f64);
            p.rect_filled(bar, 2.0, Color32::from_rgb(r, g, bb));
            p.text(
                rect.min,
                egui::Align2::LEFT_TOP,
                format!("{k} {}", dashboard::digits(*v)),
                egui::FontId::proportional(11.0),
                Color32::from_gray(200),
            );
            resp.on_hover_text(format!("{k}: {} (log scale)", dashboard::digits(*v)));
        }
    });
}

#[cfg(test)]
impl DashWindow {
    /// Start a hunt from the form, as *Start hunt* does (offscreen checks).
    pub fn test_start(&mut self, rules: &str, tilings: &str, soups: u64) {
        self.open = true;
        self.form.rules = rules.into();
        self.form.tilings = tilings.into();
        self.form.soups = soups;
        self.start_hunt();
    }
    /// The jobs as the window last read them.
    pub fn test_jobs(&self) -> Vec<JobSummary> {
        self.shared.lock().unwrap().jobs.clone()
    }
    /// *Pause* on a job's row.
    pub fn test_pause(&mut self, job: i64) {
        self.send(Cmd::Pause(job));
    }
    /// *Resume here* (or *Continue here*) on a job's row.
    pub fn test_resume(&mut self, job: i64) {
        self.send(Cmd::Resume(job, false, 0));
    }
    /// Open the window.
    pub fn test_open(&mut self) {
        self.open = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_form_makes_a_checked_hunt() {
        let mut f = HuntForm {
            rules: "B3/S23, B36/S23;B2/S".into(),
            tilings: "square hex".into(),
            w: 20,
            h: 12,
            density: 0.4,
            soups: 500,
            ..Default::default()
        };
        let s = f.spec().unwrap();
        assert_eq!(s.rules, ["B3/S23", "B36/S23", "B2/S"]);
        assert_eq!(s.tilings.len(), 2);
        assert_eq!((s.soup.w, s.soup.h, s.soup.density), (20, 12, 0.4));
        assert_eq!(s.soups, Some(500));
        // What search.start gets.
        let i = serde_json::to_value(f.input()).unwrap();
        assert_eq!(i["rules"], serde_json::json!(["B3/S23", "B36/S23", "B2/S"]));
        assert_eq!(i["tilings"], serde_json::json!(["square", "hex"]));
        assert_eq!(i["size"], serde_json::json!([20, 12]));
        assert_eq!(
            (i["soups"].as_u64(), i["density"].as_f64()),
            (Some(500), Some(0.4))
        );
        assert!(i["nb"].is_null());
        f.forever = true;
        f.nb = 2;
        let s = f.spec().unwrap();
        assert_eq!((s.soups, s.neighborhood), (None, Some(Nb::Edge)));
        let i = serde_json::to_value(f.input()).unwrap();
        assert_eq!(
            (i["soups"].as_u64(), i["nb"].as_str()),
            (Some(0), Some("edge"))
        );
        f.rules = "W30".into();
        assert!(f.spec().is_err(), "row rules cannot be hunted");
        f.rules = "B3/S23".into();
        f.tilings = "nonsense".into();
        assert!(f.spec().unwrap_err().contains("nonsense"));
        f.tilings = "square".into();
        f.density = 1.5;
        assert!(f.spec().is_err());
    }

    #[test]
    fn heat_colours_and_image() {
        assert_eq!(ramp(0.0), [0x2b, 0x24, 0x70]);
        assert_eq!(ramp(1.0), [0xff, 0xf6, 0xe0]);
        assert_eq!(heat_t(0.0, 10.0), 0.0);
        assert!((heat_t(10.0, 10.0) - 1.0).abs() < 1e-12);
        assert_eq!(heat_t(3.0, 0.0), 0.0);
        let rows = vec![HeatRow {
            birth: 1 << 3,
            survive: 0b1100,
            notation: "B3/S23".into(),
            finds: 4,
            ..Default::default()
        }];
        let m = dashboard::heat_map(&rows, 6, HeatColour::Finds);
        let img = heat_image(&m);
        assert_eq!(img.size, [64, 128]);
        let (x, y) = dashboard::heat_cell(1 << 3, 0b1100, 6);
        let [r, g, b] = ramp(1.0);
        assert_eq!(img.pixels[y * 64 + x], Color32::from_rgb(r, g, b));
        assert_eq!(split_list(" a, b;c\nd "), ["a", "b", "c", "d"]);
    }
}
