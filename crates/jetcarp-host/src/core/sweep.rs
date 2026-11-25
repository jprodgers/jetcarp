//! *Fit to this computer* on the real screens (#113): for each screen of the desktop background,
//! the host shows its rotation's most demanding scene at a few sizes, about a second each, every
//! screen at once, measures its own load meanwhile, takes a picture of each, and saves each
//! screen's sizes ([`jetcarp_display::tuning`], planned as `jetcarp host tune` plans them:
//! [`ScreenFit`]). A rotation mixing scenes that step on the GPU and on the processor has its most
//! demanding scene on each measured, one after the other. A fit's first round warms up longer
//! ([`round_warmup`]), so a fit just after the host started measures as later ones do.
//!
//! While a screen is fitted, its own stage is set aside and a stage showing its measured scene
//! stands in; the screens not being measured are paused, so the host's load is the measured
//! scenes'. The fit runs a step each frame, from [`Core::tick`], and never blocks the frame. Each
//! scene is measured as it will run, at the frame rate its screen draws it at
//! ([`jetcarp_display::pace`]), and held to that rate ([`scene_fps`]) and to its screen's share
//! of the limits (#107). A frame's time is its work, stepping and drawing, not the wait to
//! present it on a monitor of another refresh rate (#106): the window side reports each screen's
//! drawing ([`Core::painted`]).

use super::{Core, Place};
use crate::gpu_engine::EngineMeter;
use crate::power::ProcessMeter;
use crate::tune::{
    BUILD_LIMIT, Job, ScreenFit, Speeds, max_cells, p95, repicked, reusable, save_crop,
    save_screen, showing, shown_screens, steps_on_gpu,
};
use jetcarp_display::host_settings::{GpuTiming, TuneLimits};
use jetcarp_display::painter::Painter;
use jetcarp_display::rotation::{Rotation, Slot};
use jetcarp_display::simgpu::SimStatus;
use jetcarp_display::stage::Stage;
use jetcarp_display::tuning::{
    Measure, TuneEvent, TuneRequest, area_shares, attribute, crop_name, crop_size, fps_range,
    round_warmup, warmup,
};
use jetcarp_gpu::select::Mode;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// How a fit in the host reads the GPU's time.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Clock {
    None,
    /// The system's count of the host's own GPU time (Linux; macOS's task accounting, #93),
    /// used once the first warm-up shows it moves.
    Kernel,
    /// Not known yet: the first round's warm-up finds out whether the system's count moves, or
    /// on Windows whether the engine counters count the host's work.
    Undecided,
    /// Windows' GPU engine counters for the host's process ([`EngineMeter`], #72).
    Counters,
    /// From each frame's last submission until the GPU is done (the window side reports it): an
    /// upper bound, not judged.
    Submission,
}

/// What the screens on show are doing.
enum Phase {
    /// Between rounds: the next one starts on the next step.
    Idle,
    /// Their worlds at the new sizes are being built.
    Build { since: f64 },
    /// Running before they are measured.
    Warm { until: f64, gpu0: Option<Duration> },
    Measure {
        until: f64,
        started: Instant,
        meter: Option<ProcessMeter>,
        gpu0: Option<Duration>,
        acc0: f64,
        /// The first frame's time includes sampling Windows' engine counters, which is not the
        /// scenes': it counts towards the frame rate, not the 95th percentile.
        first_sampled: bool,
    },
}

/// One screen's trial this round.
struct OnShow {
    /// The fit, in [`HostSweep::fits`], its measured scene, and the size's index in that scene's
    /// search.
    fit: usize,
    measure: usize,
    index: usize,
    builds: u64,
    built: bool,
    gen0: u64,
    /// Each measured frame's work, and its sum.
    frames: Vec<f64>,
    work: f64,
}

/// A fit under way in the host.
pub(crate) struct HostSweep {
    req: TuneRequest,
    /// Seconds each size shows for.
    seconds: f64,
    limits: TuneLimits,
    fits: Vec<ScreenFit>,
    /// Each fitted screen's own stage, while a trial's stands in for it.
    parked: Vec<(usize, Stage)>,
    /// Which measured scene each screen's stand-in shows: (screen, measured scene).
    showing: Vec<(usize, usize)>,
    /// Rounds measured so far: the first warms up longer.
    rounds: usize,
    round: Vec<OnShow>,
    phase: Phase,
    painter: Option<Painter>,
    clock: Clock,
    /// Windows' GPU engine counters, for [`Clock::Undecided`] and [`Clock::Counters`].
    engine: Option<EngineMeter>,
    /// GPU seconds the window side has reported, for [`Clock::Submission`].
    pub(crate) gpu_acc: Arc<Mutex<f64>>,
    total: usize,
    trials: usize,
    started: Instant,
    most: u64,
    gpu: Option<String>,
}

impl HostSweep {
    /// The screens on show this round (the others are paused).
    pub(crate) fn screens(&self) -> Vec<usize> {
        self.round.iter().map(|o| self.fits[o.fit].screen).collect()
    }

    /// The screens being fitted.
    pub(crate) fn fitting(&self) -> Vec<usize> {
        self.fits.iter().map(|f| f.screen).collect()
    }

    pub(crate) fn auto(&self) -> bool {
        self.req.auto
    }

    /// Whether frames are being measured.
    pub(crate) fn measuring(&self) -> bool {
        matches!(self.phase, Phase::Measure { .. })
    }

    fn timing(&self) -> GpuTiming {
        sw_timing(self.clock)
    }

    /// Where it is, for `status`: the screens, the sizes on show, and how many tried.
    pub(crate) fn progress(&self) -> Value {
        json!({
            "screens": self.fitting(),
            "on_show": self.round.iter().map(|o| {
                let f = &self.fits[o.fit];
                json!({"screen": f.screen, "scene": f.label(o.measure),
                       "size": f.measures.get(o.measure).map(|m| m.search.sizes[o.index])})
            }).collect::<Vec<_>>(),
            "trials": self.trials,
            "scenes": self.total,
            "auto": self.req.auto,
        })
    }
}

/// What a step of the fit came to.
enum Step {
    Going,
    Done,
}

impl Core {
    /// Starts a fit on the real screens (each size shown for `seconds`). With `--pick` and the
    /// last fit still current, its sizes are placed at once, nothing measured (#77).
    pub(crate) fn start_sweep(&mut self, req: TuneRequest, seconds: f64) -> Result<(), String> {
        jetcarp_core::fit::available()?;
        // A fit runs its screens whatever holds them, so not before the warning is accepted (#96).
        if self.local_holds.contains(super::WARNING_HOLD) {
            return Err(jetcarp_core::photosensitivity::refusal(
                "fit the desktop background's sizes on screen",
            ));
        }
        if self.sweep.is_some() {
            return Err("a fit is already running".into());
        }
        if !(0.5..=60.0).contains(&seconds) {
            return Err("each size shows for 0.5 to 60 seconds".into());
        }
        let t0 = Instant::now();
        let n = self.screens.len();
        if let Some(i) = req.screen
            && i >= n
        {
            return Err(format!("there is no screen {i}"));
        }
        // The screens shown at once share the limits by their area (#107).
        let places: Vec<Place> = self.screens.iter().map(|s| s.place).collect();
        let mut shown = shown_screens(&places);
        if let Some(i) = req.screen
            && !shown.contains(&i)
        {
            shown.push(i);
            shown.sort_unstable();
        }
        let jobs: Vec<Job> = shown
            .iter()
            .map(|&i| {
                let st = &self.screens[i].stage;
                Job {
                    screen: i,
                    rotation: st.rotation().to_string(),
                    canvas: st.canvas(),
                    scale: st.status().scale,
                    refresh: self.screen_hz(i),
                    slots: st.slots().to_vec(),
                }
            })
            .collect();
        let shares = area_shares(&jobs.iter().map(|j| j.canvas).collect::<Vec<_>>());
        let wanted: Vec<usize> = (0..jobs.len())
            .filter(|&k| req.wants(jobs[k].screen))
            .collect();
        if wanted.is_empty() {
            return Err("nothing to fit".into());
        }
        let mut limits = self.settings.tune;
        if let Some(x) = req.cpu_percent {
            limits.cpu_percent = x;
        }
        if let Some(x) = req.gpu_percent {
            limits.gpu_percent = x;
        }
        if req.fps.is_some() {
            limits.fps = req.fps;
        }
        let limits = limits.checked();
        let gpu_name = self.gpu.as_ref().map(|g| g.adapter.get_info().name);
        self.sweep_events.clear();
        self.sweep_last = None;
        // `--pick` from the last fit, while it is current (#77).
        if let (Some(pick), false) = (req.pick, req.fresh) {
            let asked: Vec<_> = wanted
                .iter()
                .map(|&k| {
                    let j = &jobs[k];
                    let labels = j.slots.iter().map(|s| s.label.clone()).collect();
                    (
                        j.screen,
                        j.rotation.clone(),
                        j.canvas,
                        j.scale,
                        j.refresh,
                        labels,
                    )
                })
                .collect();
            if let Some(saved) = reusable(&asked, &self.settings, &limits, gpu_name.as_deref()) {
                return self.place_saved(&saved, pick, limits, &req, gpu_name, t0);
            }
        }
        let most = max_cells(&self.settings, self.gpu.as_ref());
        let speeds = Speeds::saved();
        let gpu_steps = self.gpu.is_some() && self.policy.mode != Mode::Off;
        let mut reads = HashMap::new();
        let fits: Vec<ScreenFit> = wanted
            .iter()
            .map(|&k| {
                ScreenFit::plan(
                    &jobs[k],
                    shares[k],
                    &self.lookup,
                    &self.settings,
                    &limits,
                    most,
                    &speeds,
                    gpu_steps,
                    &mut reads,
                )
            })
            .collect();
        let clock = match &self.gpu {
            None => Clock::None,
            Some(_) => Clock::Undecided,
        };
        // Windows' engine counters (#72); None elsewhere, or where they can't be read.
        let engine = (clock == Clock::Undecided && cfg!(windows))
            .then(EngineMeter::start)
            .flatten();
        let total = fits.iter().map(|f| f.scenes.len()).sum();
        let (fps, fps_max) = fps_range(fits.iter().flat_map(|f| f.scenes.iter().map(|p| p.fps)));
        self.sweep_events.push(TuneEvent::Start {
            screens: fits.iter().map(|f| f.screen).collect(),
            scenes: total,
            fps,
            fps_max,
            gpu: gpu_name.clone(),
            shares: fits.iter().map(|f| f.share_line(&limits)).collect(),
            limits: Some(limits),
            reused: false,
        });
        self.sweep = Some(HostSweep {
            req,
            seconds,
            limits,
            fits,
            parked: vec![],
            showing: vec![],
            rounds: 0,
            round: vec![],
            phase: Phase::Idle,
            painter: None,
            clock,
            engine,
            gpu_acc: Arc::new(Mutex::new(0.0)),
            total,
            trials: 0,
            started: t0,
            most,
            gpu: gpu_name,
        });
        self.apply_pause();
        Ok(())
    }

    /// `--pick` from the last fit, nothing measured (#77).
    fn place_saved(
        &mut self,
        saved: &[(usize, jetcarp_display::host_settings::Tuning)],
        pick: jetcarp_display::tuning::Pick,
        limits: TuneLimits,
        req: &TuneRequest,
        gpu: Option<String>,
        t0: Instant,
    ) -> Result<(), String> {
        let total = saved.iter().map(|(_, t)| t.scenes.len()).sum();
        self.sweep_events.push(TuneEvent::Start {
            screens: saved.iter().map(|(s, _)| *s).collect(),
            scenes: total,
            fps: saved.iter().map(|(_, t)| t.fps).fold(0.0, f32::max),
            fps_max: None,
            gpu,
            shares: vec![],
            limits: Some(limits),
            reused: true,
        });
        let mut file = None;
        let mut timing = GpuTiming::None;
        for (screen, t) in saved {
            let t = repicked(t, pick);
            for s in &t.scenes {
                timing = s.gpu_timing;
                self.sweep_events.push(TuneEvent::Scene {
                    screen: *screen,
                    result: s.clone(),
                });
            }
            if !req.dry_run {
                file = Some(save_screen(*screen, limits, t)?);
            }
        }
        self.sweep_events.push(TuneEvent::done_in(
            file.clone(),
            total,
            timing,
            0,
            t0.elapsed().as_secs_f64(),
        ));
        self.sweep_last = Some(json!({"outcome": "done", "scenes": total, "auto": req.auto,
            "reused": true}));
        self.note(format!(
            "placed the sizes the last fit found for {total} scene(s), nothing measured"
        ));
        if file.is_some() {
            self.reload()?;
        }
        Ok(())
    }

    /// Stops the fit; nothing it found is saved.
    pub(crate) fn stop_sweep(&mut self, why: &str) -> Result<(), String> {
        let Some(mut sw) = self.sweep.take() else {
            return Err("no fit is running here".into());
        };
        self.restore(&mut sw);
        self.sweep_events.push(TuneEvent::Failed {
            error: why.to_string(),
        });
        self.sweep_last = Some(json!({"outcome": "stopped", "error": why, "auto": sw.req.auto}));
        self.apply_pause();
        self.note(format!("the fit stopped ({why}); nothing was saved"));
        drop(sw);
        super::release_free_memory();
        Ok(())
    }

    /// The window side drew screen `i` this frame, its drawing taking `secs` (the painter's
    /// work, not the wait for a surface to draw on or to present it, #106).
    pub fn painted(&mut self, i: usize, secs: f64) {
        if let Some(s) = self.screens.get_mut(i) {
            s.work += secs;
        }
    }

    /// The frame is over (the window side, or the headless loop, says after drawing): each
    /// screen on show in a fit measuring it records its frame's work, its stepping and drawing.
    /// (A window's drawing, which comes a moment later, counts towards its next frame.)
    pub fn frame_done(&mut self) {
        let mut works: Vec<(usize, f64)> = vec![];
        for (i, s) in self.screens.iter_mut().enumerate() {
            if s.due {
                works.push((i, s.work));
                s.work = 0.0;
            }
        }
        let Some(sw) = self.sweep.as_mut() else {
            return;
        };
        if !sw.measuring() {
            return;
        }
        for (i, w) in works {
            if let Some(o) = sw.round.iter_mut().find(|o| sw.fits[o.fit].screen == i) {
                o.frames.push(w);
                o.work += w;
            }
        }
    }

    /// Where the window side adds each frame's GPU time while a fit measures it that way.
    pub fn sweep_gpu_meter(&self) -> Option<Arc<Mutex<f64>>> {
        let sw = self.sweep.as_ref()?;
        let due = sw
            .screens()
            .iter()
            .any(|&i| self.screens.get(i).is_some_and(|s| s.due));
        (sw.clock == Clock::Submission && sw.measuring() && due).then(|| sw.gpu_acc.clone())
    }

    /// One step of the fit, each frame.
    pub(crate) fn sweep_step(&mut self) {
        let Some(mut sw) = self.sweep.take() else {
            return;
        };
        let now = self.now();
        match self.advance(&mut sw, now) {
            Ok(Step::Going) => self.sweep = Some(sw),
            Ok(Step::Done) => self.finish_sweep(sw),
            Err(e) => {
                self.sweep = Some(sw);
                let _ = self.stop_sweep(&e);
            }
        }
    }

    /// Puts the screens' own stages back.
    fn restore(&mut self, sw: &mut HostSweep) {
        for (i, mut own) in sw.parked.drain(..) {
            if let Some(s) = self.screens.get_mut(i)
                && s.stage.rotation() == own.rotation()
            {
                own.resume_clock();
                s.stage = own;
            }
        }
    }

    fn finish_sweep(&mut self, mut sw: HostSweep) {
        self.restore(&mut sw);
        let timing = sw.timing();
        let mut saved = None;
        for f in &sw.fits {
            let results = f.results(timing, sw.req.pick);
            for t in &results {
                if let Some(n) = &t.note
                    && n.contains("chosen before")
                {
                    self.note(format!("{}: {n}", t.scene));
                }
                self.sweep_events.push(TuneEvent::Scene {
                    screen: f.screen,
                    result: t.clone(),
                });
            }
            if !sw.req.dry_run {
                let t = f.tuning(results, sw.limits, sw.most, sw.gpu.clone());
                match save_screen(f.screen, sw.limits, t) {
                    Ok(p) => saved = Some(p),
                    Err(e) => self.note(e),
                }
            }
        }
        let seconds = sw.started.elapsed().as_secs_f64();
        self.sweep_events.push(TuneEvent::done_in(
            saved.clone(),
            sw.total,
            timing,
            sw.trials,
            seconds,
        ));
        self.sweep_last = Some(
            json!({"outcome": "done", "scenes": sw.total, "auto": sw.req.auto,
            "trials": sw.trials, "seconds": (seconds * 10.0).round() / 10.0,
            "screens": sw.fitting()}),
        );
        self.apply_pause();
        self.note(format!(
            "fitted {} scene(s) on {} screen(s) in {seconds:.1} s, {} size(s) tried{}",
            sw.total,
            sw.fits.len(),
            sw.trials,
            if saved.is_some() { "; sizes saved" } else { "" }
        ));
        if saved.is_some()
            && let Err(e) = self.reload()
        {
            self.note(e);
        }
        // The trial stages go with `sw`; give their memory back (#69, #111).
        drop(sw);
        super::release_free_memory();
    }

    /// A stage showing one scene of a screen's rotation, as a fit's stand-in for it.
    fn sweep_stage(
        &self,
        rotation: &str,
        slot: &Slot,
        canvas: [f64; 2],
        scale: f64,
    ) -> Result<Stage, String> {
        let mut slot = slot.clone();
        slot.end.clear();
        slot.schedule = None;
        let r = Rotation {
            name: rotation.to_string(),
            default_minutes: None,
            ..Default::default()
        };
        let mut stage = Stage::new(
            &r,
            vec![slot],
            1,
            canvas,
            self.lookup.clone(),
            self.settings.cell_limit(),
        )?;
        stage.resize(canvas, scale, 0.0);
        Ok(stage)
    }

    /// The next round: each screen still searching shows its next size, of one of its measured
    /// scenes (the GPU's, then the processor's: a screen shows one scene at a time).
    fn start_round(&mut self, sw: &mut HostSweep, now: f64) -> Result<Step, String> {
        let next: Vec<(usize, usize, usize)> = sw
            .fits
            .iter()
            .enumerate()
            .filter_map(|(k, f)| f.next_trial().map(|(m, i)| (k, m, i)))
            .collect();
        if next.is_empty() {
            return Ok(Step::Done);
        }
        let numbered: Vec<(usize, usize)> = sw
            .fits
            .iter()
            .enumerate()
            .flat_map(|(k, f)| (0..f.measures.len()).map(move |m| (k, m)))
            .collect();
        sw.round.clear();
        for (k, m, i) in next {
            let f = &sw.fits[k];
            let screen = f.screen;
            if screen >= self.screens.len() {
                continue;
            }
            let ms = &f.measures[m];
            let size = ms.search.sizes[i];
            let p = &f.scenes[ms.scene];
            let parked = sw.parked.iter().any(|(s, _)| *s == screen);
            let shown = sw.showing.iter().find(|(s, _)| *s == screen).map(|x| x.1);
            if !parked || shown != Some(m) {
                // A stand-in showing this measured scene: the screen's own stage set aside the
                // first time, the other measured scene's stand-in let go after.
                let stage = self.sweep_stage(&f.rotation, &ms.slot, f.canvas, f.scale)?;
                let replaced = std::mem::replace(&mut self.screens[screen].stage, stage);
                if !parked {
                    sw.parked.push((screen, replaced));
                }
                sw.showing.retain(|(s, _)| *s != screen);
                sw.showing.push((screen, m));
            }
            let stage = &mut self.screens[screen].stage;
            let builds = stage.builds();
            stage.set_tuned(showing(
                &f.rotation,
                f.canvas,
                f.scale,
                &p.label,
                p.design,
                size,
            ));
            self.sweep_events.push(TuneEvent::Rung {
                screen,
                number: numbered.iter().position(|&x| x == (k, m)).unwrap_or(0) + 1,
                of: numbered.len(),
                scene: p.label.clone(),
                size,
                rung: ms.search.tried() + 1,
                rungs: ms.search.planned(),
            });
            sw.round.push(OnShow {
                fit: k,
                measure: m,
                index: i,
                builds,
                built: false,
                gen0: 0,
                frames: vec![],
                work: 0.0,
            });
        }
        sw.phase = Phase::Build { since: now };
        self.apply_pause_for(sw);
        Ok(Step::Going)
    }

    /// Only the screens on show this round run; the rest of the desktop is paused.
    fn apply_pause_for(&mut self, sw: &HostSweep) {
        let on: Vec<usize> = sw.screens();
        for (i, s) in self.screens.iter_mut().enumerate() {
            s.stage.set_paused(!on.contains(&i));
        }
    }

    fn advance(&mut self, sw: &mut HostSweep, now: f64) -> Result<Step, String> {
        match &mut sw.phase {
            Phase::Idle => self.start_round(sw, now),
            Phase::Build { since } => {
                let since = *since;
                let mut waiting = false;
                let mut notes: Vec<String> = vec![];
                for o in sw.round.iter_mut().filter(|o| !o.built) {
                    let f = &mut sw.fits[o.fit];
                    let Some(screen) = self.screens.get_mut(f.screen) else {
                        continue;
                    };
                    let said = screen.stage.take_notes();
                    let label = f.label(o.measure);
                    let ms = &mut f.measures[o.measure];
                    if screen.stage.builds() > o.builds && screen.stage.world().is_some() {
                        let world = screen.stage.world().expect("built");
                        let paced = screen.stage.pace().speed > 0.0;
                        let on = steps_on_gpu(&ms.slot.profile, paced, &screen.sim, world);
                        screen.stage.set_world_gpu(on);
                        o.built = true;
                    } else if screen.stage.paused() || now - since > BUILD_LIMIT {
                        let search = &mut ms.search;
                        let why = format!(
                            "{}: {}",
                            search.sizes[o.index].describe(),
                            if said.is_empty() {
                                "could not be built".to_string()
                            } else {
                                said.join("; ")
                            }
                        );
                        search.failed(o.index, &why);
                        ms.failed.push(why.clone());
                        notes.push(format!("{label}: {why}"));
                        o.built = true;
                        o.index = usize::MAX;
                    } else {
                        waiting = true;
                    }
                }
                for n in notes {
                    self.note(n);
                }
                sw.round.retain(|o| o.index != usize::MAX);
                if waiting {
                    return Ok(Step::Going);
                }
                if sw.round.is_empty() {
                    sw.phase = Phase::Idle;
                    return Ok(Step::Going);
                }
                if sw.clock == Clock::Undecided
                    && let Some(e) = &mut sw.engine
                {
                    // The counters from here, to see whether they count the warm-up.
                    let _ = e.share();
                }
                sw.phase = Phase::Warm {
                    // A fit's first round warms up longer: a cold start is slower.
                    until: now + round_warmup(sw.seconds, sw.rounds == 0),
                    gpu0: crate::power::process_gpu_time(),
                };
                Ok(Step::Going)
            }
            Phase::Warm { until, gpu0 } => {
                if now < *until {
                    return Ok(Step::Going);
                }
                // The system's count where it moved in the warm-up (Linux; macOS, #93); else
                // Windows' counters where they counted it; else submission to completion.
                let mut sampled = false;
                if sw.clock == Clock::Undecided {
                    let moved = match (*gpu0, crate::power::process_gpu_time()) {
                        (Some(a), Some(b)) => b > a,
                        _ => false,
                    };
                    sw.clock = if moved {
                        Clock::Kernel
                    } else if let Some(e) = sw.engine.as_mut() {
                        sampled = true;
                        if e.share().is_some_and(|s| s > 0.0) {
                            Clock::Counters
                        } else {
                            Clock::Submission
                        }
                    } else {
                        Clock::Submission
                    };
                } else if let (Clock::Counters, Some(e)) = (sw.clock, sw.engine.as_mut()) {
                    // Their sample from here, taken before the processor meter starts so PDH's
                    // own work is not counted as the host's.
                    let _ = e.share();
                    sampled = true;
                }
                if sw.clock != Clock::Counters {
                    sw.engine = None;
                }
                for o in &mut sw.round {
                    let screen = &self.screens[sw.fits[o.fit].screen];
                    o.gen0 = screen.stage.world().map_or(0, |w| w.generation);
                    o.frames.clear();
                    o.work = 0.0;
                }
                let acc0 = sw.gpu_acc.lock().map(|a| *a).unwrap_or(0.0);
                sw.phase = Phase::Measure {
                    until: now + (sw.seconds - warmup(sw.seconds)),
                    started: Instant::now(),
                    meter: ProcessMeter::start(),
                    gpu0: crate::power::process_gpu_time(),
                    acc0,
                    first_sampled: sampled,
                };
                Ok(Step::Going)
            }
            Phase::Measure { until, .. } => {
                if now < *until {
                    return Ok(Step::Going);
                }
                self.record(sw);
                sw.rounds += 1;
                sw.phase = Phase::Idle;
                Ok(Step::Going)
            }
        }
    }

    /// What the sizes on show measured, with their pictures: the processor's and GPU's time,
    /// measured for every screen at once, shared between them by how long each one's frames
    /// took (#107).
    fn record(&mut self, sw: &mut HostSweep) {
        let acc = sw.gpu_acc.lock().map(|a| *a).unwrap_or(0.0);
        let clock = sw.clock;
        let Phase::Measure {
            started,
            meter,
            gpu0,
            acc0,
            first_sampled,
            ..
        } = &sw.phase
        else {
            return;
        };
        let wall = started.elapsed().as_secs_f64().max(1e-6);
        // The processor first: the counters' sample below is PDH's work, not the host's.
        let cpu = meter.as_ref().map_or(0.0, |m| m.share());
        let gpu = match clock {
            Clock::None => None,
            Clock::Kernel => match (*gpu0, crate::power::process_gpu_time()) {
                (Some(a), Some(b)) => Some(b.saturating_sub(a).as_secs_f64() / wall),
                _ => None,
            },
            // None if they stopped answering: the share is then unknown, not zero.
            Clock::Counters => sw.engine.as_mut().and_then(EngineMeter::share),
            Clock::Undecided | Clock::Submission => Some((acc - *acc0) / wall),
        };
        let first_sampled = *first_sampled;
        let works: Vec<f64> = sw.round.iter().map(|o| o.work).collect();
        let cpus = attribute(cpu, &works);
        let gpus = gpu.map(|g| attribute(g, &works));
        sw.trials += sw.round.len();
        let round = std::mem::take(&mut sw.round);
        for (n, mut o) in round.into_iter().enumerate() {
            let f = &mut sw.fits[o.fit];
            let screen = &self.screens[f.screen];
            let st = screen.stage.status();
            // The speed it runs at (in low power, at most the frame rate).
            let speed = screen.stage.pace().speed as f64;
            let frame_count = o.frames.len();
            let judged = if first_sampled && o.frames.len() > 1 {
                &mut o.frames[1..]
            } else {
                &mut o.frames[..]
            };
            let m = Measure {
                cpu: cpus[n],
                gpu: gpus.as_ref().map(|g| g[n].clamp(0.0, 1.0)),
                p95: p95(judged),
                fps: frame_count as f64 / wall,
                gens_wanted: (speed > 0.0).then_some(speed * wall),
                gens_done: screen
                    .stage
                    .world()
                    .map_or(0, |w| w.generation)
                    .saturating_sub(o.gen0) as f64,
                on_gpu: matches!(screen.sim.status(), SimStatus::Gpu(_)),
                gpu_timing: sw_timing(clock),
                cells: st.cells as u64,
            };
            let label = f.label(o.measure);
            let ms = &mut f.measures[o.measure];
            let l = ms.limits;
            let search = &mut ms.search;
            let shown = search.record(o.index, st.cell_px, &m, &l).size;
            // The picture, drawn by a painter of the fit's own from the stage on show.
            let image = match &self.gpu {
                Some(g) => {
                    let painter = sw.painter.get_or_insert_with(|| {
                        Painter::new(&g.device, jetcarp_display::crop::FORMAT)
                    });
                    jetcarp_display::crop::picture(
                        &g.device,
                        &g.queue,
                        painter,
                        &screen.stage,
                        [f.canvas[0].round() as u32, f.canvas[1].round() as u32],
                        crop_size(f.canvas),
                        f.scale as f32,
                    )
                }
                None => None,
            };
            let name = crop_name(f.screen, &label, &shown);
            let crop = image.and_then(|i| save_crop(&name, &i, None));
            if let Some(r) = search.rungs.last_mut() {
                r.crop = crop;
                self.sweep_events.push(TuneEvent::Measured {
                    screen: f.screen,
                    scene: label,
                    rung: r.clone(),
                });
            }
        }
    }
}

fn sw_timing(c: Clock) -> GpuTiming {
    match c {
        Clock::None => GpuTiming::None,
        Clock::Kernel => GpuTiming::Kernel,
        Clock::Counters => GpuTiming::Counters,
        Clock::Undecided | Clock::Submission => GpuTiming::Submission,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use jetcarp_display::host_settings::{HostSettings, ScreenSettings};

    /// A host of `screens` screens on the desktop, each showing "Until it settles", headless on
    /// a 160 x 120 canvas, with no link.
    fn core(screens: usize) -> Core {
        let settings = HostSettings {
            screens: (0..screens)
                .map(|_| ScreenSettings {
                    rotation: "Until it settles".into(),
                    place: Place::Wallpaper,
                    ..Default::default()
                })
                .collect(),
            ..Default::default()
        };
        Core::build(settings, None, None, Some([160.0, 120.0]), None).unwrap()
    }

    /// A frame is timed by its work, its stepping and its surfaces' drawing, not by the wait for
    /// a surface or to present it (#106): on the Mac, the built-in screen's frames waited on the
    /// other monitor's 60 Hz, 17 to 20 ms at every size against a 16.7 ms budget, so no size fit.
    #[test]
    fn a_frame_is_timed_by_its_work_not_the_wait_to_present() {
        let mut core = core(1);
        core.start_sweep(TuneRequest::default(), 0.5).unwrap();
        {
            let sw = core.sweep.as_mut().unwrap();
            sw.round = vec![OnShow {
                fit: 0,
                measure: 0,
                index: 0,
                builds: 0,
                built: true,
                gen0: 0,
                frames: vec![],
                work: 0.0,
            }];
            sw.phase = Phase::Measure {
                until: f64::INFINITY,
                started: Instant::now(),
                meter: None,
                gpu0: None,
                acc0: 0.0,
                first_sampled: false,
            };
        }
        // A frame: 2 ms stepping, then a 15 ms wait for the surface (another monitor's
        // refresh), then 3 ms drawing.
        let started = Instant::now();
        core.screens[0].due = true;
        core.screens[0].work = 0.002;
        std::thread::sleep(Duration::from_millis(15));
        core.painted(0, 0.003);
        let wall = started.elapsed().as_secs_f64();
        core.frame_done();
        let frames = &core.sweep.as_ref().unwrap().round[0].frames;
        assert_eq!(frames.len(), 1);
        assert!(
            (frames[0] - 0.005).abs() < 1e-9,
            "the frame's work, 5 ms, not the {:.1} ms it took on the wall: {frames:?}",
            wall * 1000.0
        );
        // A screen not due records nothing.
        core.screens[0].due = false;
        core.painted(0, 0.004);
        core.frame_done();
        assert_eq!(core.sweep.as_ref().unwrap().round[0].frames.len(), 1);
    }

    /// An automatic fit sweeps only the screens whose monitor changed (#110), each still held to
    /// its share of the whole desktop's limits (#107); the other screen keeps its fit, and is
    /// paused while the changed one is measured, so the load measured is the changed one's.
    #[test]
    fn an_automatic_fit_sweeps_only_the_screens_whose_monitor_changed() {
        let mut core = core(2);
        let req = TuneRequest {
            auto: true,
            screens: vec![1],
            ..Default::default()
        };
        core.start_sweep(req, 0.5).unwrap();
        let sw = core.sweep.as_ref().unwrap();
        assert_eq!(sw.fitting(), [1], "only the screen whose monitor changed");
        assert!(
            (sw.fits[0].share - 0.5).abs() < 1e-9,
            "half the desktop's area"
        );
        match &core.sweep_events[0] {
            TuneEvent::Start {
                screens, shares, ..
            } => {
                assert_eq!(screens, &[1]);
                assert_eq!(shares.len(), 1);
                assert!((shares[0].cpu_percent - 5.0).abs() < 1e-4, "{shares:?}");
            }
            e => panic!("{e:?}"),
        }
        // Its first round puts screen 1's trial on show; screen 0 is paused.
        core.sweep_step();
        let sw = core.sweep.as_ref().unwrap();
        assert_eq!(sw.screens(), [1]);
        assert!(core.screens[0].stage.paused() && !core.screens[1].stage.paused());
        core.stop_sweep("test").unwrap();
        assert!(!core.screens[0].stage.paused());
    }
}
