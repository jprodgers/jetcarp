//! *Fit to this computer*: the search for the cell size each desktop background scene can run at
//! within the user's limits, and what the tuner reports as it goes.
//!
//! The tuner (`jetcarp host tune`, in the host crate) runs each scene of a screen's rotation for a
//! few seconds at a time, at the screen's size, and measures JETCARP's own load: processor time,
//! GPU time, and how long frames take. Bigger cells mean fewer cells and less work, with the same
//! rules, palettes and colouring, so [`search`] looks for the smallest cell size (the most cells)
//! that stays within every limit, starting from the scene's own size and never going below it.
//!
//! The tuner runs in a process of its own, so the processor time it measures is its own; the app
//! and the host start it with [`TuneChild`] and follow its progress on its output.

use crate::host_settings::{GpuTiming, TuneLimits, TunedScene};
use serde::{Deserialize, Serialize};
use std::io::BufRead;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{Receiver, channel};

/// Each limit is kept with this much to spare: a size passes at 80% of the CPU and GPU limits.
pub const MARGIN: f32 = 0.8;
/// A frame's 95th percentile must be within this share of the frame's interval.
pub const FRAME_MARGIN: f32 = 0.9;
/// Generations stepped must reach this share of the scene's speed.
pub const KEEP_UP: f64 = 0.9;
/// The coarsest sensible grid has at least this many cells across its shorter side.
pub const MIN_CELLS_ACROSS: f64 = 60.0;
/// Seconds each trial measures, after its warm-up.
pub const TRIAL_SECONDS: f64 = 3.0;

/// What one trial measured.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Default)]
pub struct Measure {
    /// JETCARP's processor time per second, as a share of the machine (0 to 1).
    pub cpu: f64,
    /// Its share of the GPU's time (None: not measured).
    pub gpu: Option<f64>,
    /// The 95th percentile of frame times, in seconds.
    pub p95: f64,
    /// Generations wanted at the scene's speed, and stepped (None: as fast as it can, so no
    /// target).
    pub gens_wanted: Option<f64>,
    pub gens_done: f64,
    /// Stepped on the GPU.
    pub on_gpu: bool,
    pub gpu_timing: GpuTiming,
    /// Cells in the world.
    pub cells: u64,
}

/// The limits one scene is held to: the user's, split among the screens shown at once.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq)]
pub struct SceneLimits {
    /// Share of the machine's processor time (0 to 1).
    pub cpu: f64,
    /// Share of the GPU's time (0 to 1).
    pub gpu: f64,
    /// Frames per second to keep.
    pub fps: f64,
}

impl SceneLimits {
    /// The user's limits for one of `screens` screens shown at once, at `fps`.
    pub fn of(l: &TuneLimits, fps: f32, screens: usize) -> SceneLimits {
        let n = screens.max(1) as f64;
        SceneLimits {
            cpu: l.cpu_percent as f64 / 100.0 / n,
            gpu: l.gpu_percent as f64 / 100.0 / n,
            fps: fps.max(1.0) as f64,
        }
    }

    /// The frame interval, in seconds.
    pub fn interval(&self) -> f64 {
        1.0 / self.fps.max(1.0)
    }
}

/// Whether a trial stayed within the limits (with [`MARGIN`] to spare); Err says which it passed.
pub fn fits(m: &Measure, l: &SceneLimits) -> Result<(), String> {
    let mut over = vec![];
    let margin = MARGIN as f64;
    if m.cpu > l.cpu * margin {
        over.push(format!(
            "processor {} (over {}: the {} limit, less 20%)",
            percent(m.cpu),
            percent(l.cpu * margin),
            percent(l.cpu)
        ));
    }
    if let Some(g) = m.gpu
        && g > l.gpu * margin
    {
        over.push(format!(
            "GPU {} (over {}: the {} limit, less 20%)",
            percent(g),
            percent(l.gpu * margin),
            percent(l.gpu)
        ));
    }
    if m.p95 > l.interval() * FRAME_MARGIN as f64 {
        over.push(format!(
            "frames {:.1} ms (at {} fps, {:.1} ms)",
            m.p95 * 1000.0,
            l.fps,
            l.interval() * 1000.0
        ));
    }
    if let Some(want) = m.gens_wanted
        && want > 0.0
        && m.gens_done < want * KEEP_UP
    {
        over.push(format!(
            "slow: {:.0} of {:.0} generations",
            m.gens_done, want
        ));
    }
    if over.is_empty() {
        Ok(())
    } else {
        Err(over.join(", "))
    }
}

/// `x` (a share) as a percentage, to a tenth below 10%.
pub fn percent(x: f64) -> String {
    let p = x * 100.0;
    if p < 10.0 {
        format!("{p:.1}%")
    } else {
        format!("{p:.0}%")
    }
}

/// The coarsest sensible cell size, in pixels, on a canvas of `canvas` pixels: at least
/// [`MIN_CELLS_ACROSS`] cells across the shorter side, at most the largest cell size there is, and
/// never below the scene's own size.
pub fn coarsest_px(canvas: [f64; 2], design_px: f32) -> f32 {
    let short = canvas[0].min(canvas[1]).max(1.0);
    let px = (short / MIN_CELLS_ACROSS).floor() as f32;
    px.min(*crate::profiles::CELL_PX_RANGE.end()).max(design_px)
}

/// The cell sizes to try, smallest first: the scene's own, then each whole number of pixels above
/// it up to `coarsest`.
pub fn candidates(design_px: f32, coarsest: f32) -> Vec<f32> {
    let mut v = vec![design_px];
    let mut px = design_px.floor() + 1.0;
    while px <= coarsest {
        v.push(px);
        px += 1.0;
    }
    v
}

/// What the search settled on for one scene.
#[derive(Clone, Debug, PartialEq)]
pub struct Found {
    pub cell_px: f32,
    pub measure: Measure,
    pub fits: bool,
    /// Why no size fitted, when none did.
    pub note: Option<String>,
    pub trials: usize,
}

/// Finds the smallest cell size in `sizes` (smallest first; the first is the scene's own) whose
/// trial stays within `limits`, by bisection: bigger cells are assumed to cost no more. The
/// scene's own size is tried first, and when it fits nothing else is. When even the coarsest does
/// not fit, the coarsest is chosen, with a note saying what it passed.
pub fn search(
    sizes: &[f32],
    limits: &SceneLimits,
    mut trial: impl FnMut(f32) -> Result<Measure, String>,
) -> Result<Found, String> {
    let first = *sizes.first().ok_or("no cell sizes to try")?;
    let mut trials = 0;
    let mut run = |px: f32| {
        trials += 1;
        trial(px)
    };
    let m0 = run(first)?;
    if fits(&m0, limits).is_ok() || sizes.len() == 1 {
        let note = fits(&m0, limits).err();
        return Ok(Found {
            cell_px: first,
            measure: m0,
            fits: note.is_none(),
            note: note.map(|why| format!("over the limits even at its own size: {why}")),
            trials: 1,
        });
    }
    let last = sizes.len() - 1;
    let ml = run(sizes[last])?;
    if let Err(why) = fits(&ml, limits) {
        return Ok(Found {
            cell_px: sizes[last],
            measure: ml,
            fits: false,
            note: Some(format!("over the limits even at the coarsest size: {why}")),
            trials: 2,
        });
    }
    // sizes[lo] does not fit, sizes[hi] does.
    let (mut lo, mut hi, mut best) = (0, last, ml);
    while hi - lo > 1 {
        let mid = (lo + hi) / 2;
        let m = run(sizes[mid])?;
        if fits(&m, limits).is_ok() {
            hi = mid;
            best = m;
        } else {
            lo = mid;
        }
    }
    Ok(Found {
        cell_px: sizes[hi],
        measure: best,
        fits: true,
        note: None,
        trials,
    })
}

impl Found {
    /// As saved in the settings, for the scene labelled `scene` whose own size is `design_px`.
    pub fn tuned(&self, scene: &str, design_px: f32) -> TunedScene {
        TunedScene {
            scene: scene.to_string(),
            design_px,
            cell_px: self.cell_px,
            cells: self.measure.cells,
            cpu_percent: (self.measure.cpu * 100.0) as f32,
            gpu_percent: self.measure.gpu.map(|g| (g * 100.0) as f32),
            p95_ms: (self.measure.p95 * 1000.0) as f32,
            on_gpu: self.measure.on_gpu,
            gpu_timing: self.measure.gpu_timing,
            fits: self.fits,
            note: self.note.clone(),
        }
    }
}

/// One line of what a tuner reports, as `jetcarp host tune --json` prints it.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum TuneEvent {
    /// The tune has begun: this many scenes on these screens, at this frame rate.
    Start {
        screens: Vec<usize>,
        scenes: usize,
        fps: f32,
        gpu: Option<String>,
    },
    /// A trial is starting.
    Trial {
        screen: usize,
        /// Counted from 1, across every screen.
        number: usize,
        of: usize,
        scene: String,
        cell_px: f32,
    },
    /// A trial's result.
    Measured {
        screen: usize,
        scene: String,
        cell_px: f32,
        measure: Measure,
        /// Why it did not fit (None: it did).
        over: Option<String>,
    },
    /// A scene is done.
    Scene { screen: usize, result: TunedScene },
    /// Everything is done and saved.
    Done {
        /// Where the host's settings were saved (None: they were not, as in a dry run).
        saved: Option<String>,
        scenes: usize,
        /// How the GPU's time was measured.
        #[serde(default)]
        gpu_timing: GpuTiming,
    },
    /// It stopped early.
    Failed { error: String },
}

impl TuneEvent {
    /// One line for people.
    pub fn describe(&self) -> String {
        match self {
            TuneEvent::Start {
                screens,
                scenes,
                fps,
                gpu,
            } => format!(
                "tuning {scenes} scene{} on {} screen{} for {fps} frames a second, {}",
                if *scenes == 1 { "" } else { "s" },
                screens.len(),
                if screens.len() == 1 { "" } else { "s" },
                gpu.as_deref()
                    .map_or("on the processor".to_string(), |g| format!("on {g}"))
            ),
            TuneEvent::Trial {
                screen,
                number,
                of,
                scene,
                cell_px,
            } => format!("screen {screen}, scene {number} of {of}: {scene} at {cell_px} px"),
            TuneEvent::Measured {
                cell_px,
                measure,
                over,
                ..
            } => format!(
                "  {cell_px} px, {} cells: {}{}",
                thousands(measure.cells),
                measure_line(measure),
                over.as_ref()
                    .map_or(String::from(" — fits"), |o| format!(" — over: {o}"))
            ),
            TuneEvent::Scene { screen, result } => scene_line(*screen, result),
            TuneEvent::Done {
                saved,
                scenes,
                gpu_timing,
            } => {
                let how = match gpu_timing {
                    GpuTiming::None => String::new(),
                    t => format!(" (GPU time from {})", t.describe()),
                };
                match saved {
                    Some(p) => format!("tuned {scenes} scene(s){how}; saved to {p}"),
                    None => format!("tuned {scenes} scene(s){how}; not saved"),
                }
            }
            TuneEvent::Failed { error } => format!("stopped: {error}"),
        }
    }
}

/// A trial's numbers, for people.
pub fn measure_line(m: &Measure) -> String {
    let gpu = match m.gpu {
        Some(g) => format!(", GPU {}", percent(g)),
        None => String::new(),
    };
    format!(
        "processor {}{gpu}, frames {:.1} ms (95th percentile), stepped on the {}",
        percent(m.cpu),
        m.p95 * 1000.0,
        if m.on_gpu { "GPU" } else { "processor" }
    )
}

/// One scene's result, for people.
pub fn scene_line(screen: usize, t: &TunedScene) -> String {
    let size = if t.cell_px > t.design_px {
        format!("{} px (its own: {} px)", t.cell_px, t.design_px)
    } else {
        format!("{} px, its own size", t.cell_px)
    };
    let gpu = t.gpu_percent.map_or(String::new(), |g| {
        format!(", GPU {}", percent(g as f64 / 100.0))
    });
    let mut s = format!(
        "screen {screen}: {}: {size}, {} cells; processor {}{gpu}, frames {:.1} ms",
        t.scene,
        thousands(t.cells),
        percent(t.cpu_percent as f64 / 100.0),
        t.p95_ms
    );
    if let Some(n) = &t.note {
        s.push_str(&format!(" ({n})"));
    }
    s
}

/// A count with thousands separated by commas.
pub fn thousands(n: u64) -> String {
    let s = n.to_string();
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// What to tune for, as `jetcarp host tune` takes it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TuneRequest {
    pub cpu_percent: Option<f32>,
    pub gpu_percent: Option<f32>,
    pub fps: Option<f32>,
    /// Only this screen.
    pub screen: Option<usize>,
    /// Started by the host because the monitors changed.
    pub auto: bool,
}

impl TuneRequest {
    /// The command line after `jetcarp host tune`.
    pub fn args(&self) -> Vec<String> {
        let mut a = vec![];
        if let Some(x) = self.cpu_percent {
            a.extend(["--cpu-limit".into(), x.to_string()]);
        }
        if let Some(x) = self.gpu_percent {
            a.extend(["--gpu-limit".into(), x.to_string()]);
        }
        if let Some(x) = self.fps {
            a.extend(["--fps".into(), x.to_string()]);
        }
        if let Some(x) = self.screen {
            a.extend(["--screen".into(), x.to_string()]);
        }
        if self.auto {
            a.push("--auto".into());
        }
        a
    }
}

/// A tuner running as a child process (`jetcarp host tune --json`), its progress read from its
/// output on a thread of its own.
pub struct TuneChild {
    child: Child,
    rx: Receiver<TuneEvent>,
    /// Every event so far.
    pub events: Vec<TuneEvent>,
    exit: Option<std::process::ExitStatus>,
}

impl TuneChild {
    /// Starts `jetcarp host tune --json` with `req`'s options.
    pub fn spawn(req: &TuneRequest) -> Result<TuneChild, String> {
        let cmd = crate::autostart::jetcarp_exe()?;
        let mut c = Command::new(cmd);
        c.args(["host", "tune", "--json"])
            .args(req.args())
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            // CREATE_NO_WINDOW: no console flashes up from the app or the host.
            c.creation_flags(0x0800_0000);
        }
        let mut child = c
            .spawn()
            .map_err(|e| format!("could not start the tuner: {e}"))?;
        let out = child.stdout.take().ok_or("the tuner has no output")?;
        let (tx, rx) = channel();
        std::thread::Builder::new()
            .name("jetcarp-tune-read".into())
            .spawn(move || {
                for line in std::io::BufReader::new(out).lines() {
                    let Ok(line) = line else { break };
                    if let Ok(e) = serde_json::from_str::<TuneEvent>(&line)
                        && tx.send(e).is_err()
                    {
                        break;
                    }
                }
            })
            .map_err(|e| e.to_string())?;
        Ok(TuneChild {
            child,
            rx,
            events: vec![],
            exit: None,
        })
    }

    /// New events since the last call (also kept in `events`).
    pub fn poll(&mut self) -> Vec<TuneEvent> {
        let new: Vec<TuneEvent> = self.rx.try_iter().collect();
        self.events.extend(new.iter().cloned());
        if self.exit.is_none()
            && let Ok(Some(s)) = self.child.try_wait()
        {
            self.exit = Some(s);
            // Whatever it wrote before it ended.
            let rest: Vec<TuneEvent> = self.rx.try_iter().collect();
            self.events.extend(rest.iter().cloned());
            return new.into_iter().chain(rest).collect();
        }
        new
    }

    /// Whether it has ended (after [`poll`](Self::poll) noticed).
    pub fn finished(&self) -> bool {
        self.exit.is_some()
    }

    /// How it ended: Ok once it reported `done`, else why not.
    pub fn outcome(&self) -> Option<Result<(), String>> {
        let exit = self.exit?;
        Some(
            match self
                .events
                .iter()
                .rev()
                .find(|e| matches!(e, TuneEvent::Done { .. } | TuneEvent::Failed { .. }))
            {
                Some(TuneEvent::Done { .. }) => Ok(()),
                Some(TuneEvent::Failed { error }) => Err(error.clone()),
                _ if exit.success() => Ok(()),
                _ => Err(format!("the tuner ended ({exit})")),
            },
        )
    }

    /// The latest progress line, for people.
    pub fn progress(&self) -> Option<String> {
        self.events
            .iter()
            .rev()
            .find(|e| !matches!(e, TuneEvent::Measured { .. }))
            .map(TuneEvent::describe)
    }

    /// Each scene's result so far.
    pub fn scenes(&self) -> Vec<(usize, TunedScene)> {
        self.events
            .iter()
            .filter_map(|e| match e {
                TuneEvent::Scene { screen, result } => Some((*screen, result.clone())),
                _ => None,
            })
            .collect()
    }

    /// Trials done and to do, from the latest `trial` event: (number, of).
    pub fn counts(&self) -> Option<(usize, usize)> {
        self.events.iter().rev().find_map(|e| match e {
            TuneEvent::Trial { number, of, .. } => Some((*number, *of)),
            _ => None,
        })
    }

    /// Stops it (the scenes it tuned are not saved).
    pub fn stop(&mut self) {
        if self.exit.is_none() {
            let _ = self.child.kill();
            self.exit = self.child.wait().ok();
        }
    }
}

impl Drop for TuneChild {
    fn drop(&mut self) {
        // A tuner nobody follows any more finishes on its own and saves what it found.
        let _ = self.child.try_wait();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A made-up machine: the load grows with the cell count, and frames with it.
    fn model(
        cells_per_px2: f64,
        cpu_per_mcell: f64,
        gpu_per_mcell: f64,
    ) -> impl Fn(f32) -> Measure {
        move |px: f32| {
            let cells = cells_per_px2 / (px as f64 * px as f64);
            let mc = cells / 1e6;
            Measure {
                cpu: cpu_per_mcell * mc,
                gpu: Some(gpu_per_mcell * mc),
                p95: 0.002 + 0.01 * mc,
                gens_wanted: Some(90.0),
                gens_done: 90.0,
                on_gpu: true,
                gpu_timing: GpuTiming::Timestamps,
                cells: cells as u64,
            }
        }
    }

    fn limits() -> SceneLimits {
        SceneLimits::of(&TuneLimits::default(), 30.0, 1)
    }

    #[test]
    fn the_search_finds_the_most_cells_within_the_limits() {
        // 2560 x 1440 pixels: 3.7 million cells at 1 px. 0.2 of the machine per million cells.
        let canvas = [2560.0, 1440.0];
        let area = canvas[0] * canvas[1];
        let m = model(area, 0.2, 0.1);
        let sizes = candidates(1.0, coarsest_px(canvas, 1.0));
        assert_eq!(sizes.first(), Some(&1.0));
        assert_eq!(sizes.last(), Some(&24.0));
        let mut tried = vec![];
        let f = search(&sizes, &limits(), |px| {
            tried.push(px);
            Ok(m(px))
        })
        .unwrap();
        assert!(f.fits);
        // 8% of the machine with a 20% margin: under 0.4 million cells, so 4 px (230,400 cells)
        // fits and 3 px (409,600) does not.
        assert_eq!(f.cell_px, 4.0);
        assert!(fits(&m(4.0), &limits()).is_ok());
        assert!(fits(&m(3.0), &limits()).is_err());
        // Its own size, the coarsest, then bisection: far fewer trials than sizes.
        assert_eq!(tried[..2], [1.0, 24.0]);
        assert!(f.trials <= 8, "{tried:?}");
        assert_eq!(f.trials, tried.len());
        // The same answer every time.
        let again = search(&sizes, &limits(), |px| Ok(m(px))).unwrap();
        assert_eq!(again, f);
    }

    #[test]
    fn a_scene_that_fits_at_its_own_size_keeps_it_after_one_trial() {
        let m = model(1e5, 0.2, 0.1);
        let sizes = candidates(3.0, 24.0);
        let f = search(&sizes, &limits(), |px| Ok(m(px))).unwrap();
        assert_eq!((f.cell_px, f.fits, f.trials), (3.0, true, 1));
        assert_eq!(f.note, None);
        let t = f.tuned("Life", 3.0);
        assert_eq!((t.cell_px, t.design_px, t.fits), (3.0, 3.0, true));
    }

    #[test]
    fn a_scene_that_never_fits_gets_the_coarsest_and_a_note() {
        // Frames that never get under 40 ms, whatever the size.
        let m = |px: f32| Measure {
            p95: 0.040,
            cells: (1e6 / (px * px) as f64) as u64,
            ..Measure::default()
        };
        let sizes = candidates(2.0, 18.0);
        let f = search(&sizes, &limits(), |px| Ok(m(px))).unwrap();
        assert_eq!(f.cell_px, 18.0);
        assert!(!f.fits);
        let note = f.note.unwrap();
        assert!(
            note.contains("coarsest") && note.contains("frames"),
            "{note}"
        );
        // A scene that cannot change size: one trial, and the note.
        let f = search(&[5.0], &limits(), |px| Ok(m(px))).unwrap();
        assert_eq!((f.cell_px, f.fits, f.trials), (5.0, false, 1));
    }

    #[test]
    fn each_screen_gets_its_share_and_slow_steps_do_not_fit() {
        let l = SceneLimits::of(&TuneLimits::default(), 60.0, 2);
        assert!((l.cpu - 0.05).abs() < 1e-9 && (l.gpu - 0.125).abs() < 1e-9);
        assert!((l.interval() - 1.0 / 60.0).abs() < 1e-12);
        let slow = Measure {
            gens_wanted: Some(100.0),
            gens_done: 50.0,
            ..Measure::default()
        };
        assert!(fits(&slow, &l).unwrap_err().contains("slow"));
        // As fast as it can: no target to miss.
        let max = Measure {
            gens_wanted: None,
            gens_done: 5.0,
            ..Measure::default()
        };
        assert!(fits(&max, &l).is_ok());
        // A trial that fails stops the search.
        assert!(search(&[1.0, 2.0], &l, |_| Err("no".into())).is_err());
    }

    #[test]
    fn sizes_and_requests_read_as_expected() {
        assert_eq!(candidates(2.5, 5.0), [2.5, 3.0, 4.0, 5.0]);
        assert_eq!(candidates(8.0, 8.0), [8.0]);
        assert_eq!(coarsest_px([1920.0, 1080.0], 1.0), 18.0);
        assert_eq!(
            coarsest_px([800.0, 600.0], 20.0),
            20.0,
            "never below its own"
        );
        assert_eq!(
            coarsest_px([20000.0, 10000.0], 1.0),
            64.0,
            "the largest cell"
        );
        assert_eq!(thousands(1234567), "1,234,567");
        assert_eq!(thousands(999), "999");
        let r = TuneRequest {
            cpu_percent: Some(5.0),
            fps: Some(30.0),
            auto: true,
            ..Default::default()
        };
        assert_eq!(r.args(), ["--cpu-limit", "5", "--fps", "30", "--auto"]);
        let e = TuneEvent::Trial {
            screen: 0,
            number: 2,
            of: 7,
            scene: "Life".into(),
            cell_px: 3.0,
        };
        let line = serde_json::to_string(&e).unwrap();
        assert!(line.contains(r#""event":"trial""#), "{line}");
        assert_eq!(serde_json::from_str::<TuneEvent>(&line).unwrap(), e);
    }
}
