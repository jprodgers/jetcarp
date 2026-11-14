//! *Fit to this computer* on the real screen: the host shows each scene of a screen's rotation at
//! each size of its ladder for a couple of seconds, measures its own load meanwhile, takes a
//! picture of each, and saves each scene's cards and the size chosen ([`jetcarp_display::tuning`]).
//!
//! While a screen is swept, its own stage is set aside and a stage showing one scene stands in;
//! the other screens are paused, so the host's load is the swept scene's. The sweep runs a step
//! each frame, from [`Core::tick`], and never blocks the frame.

use super::{Core, Place};
use crate::power::ProcessMeter;
use crate::tune::{
    BUILD_LIMIT, MAX_SCENES, ScenePlan, max_cells, save_crop, save_scene, scene_plan, showing,
    steps_on_gpu, tuned_before,
};
use jetcarp_display::host_settings::{GpuTiming, Size, TuneLimits};
use jetcarp_display::painter::Painter;
use jetcarp_display::rotation::{Rotation, Slot};
use jetcarp_display::simgpu::SimStatus;
use jetcarp_display::stage::Stage;
use jetcarp_display::tuning::{
    Measure, SceneLimits, TuneEvent, TuneRequest, crop_name, crop_size, tuned_scene, warmup,
};
use serde_json::{Value, json};
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// How a sweep in the host reads the GPU's time.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Clock {
    None,
    /// The system's count of the host's own GPU time (Linux).
    Kernel,
    /// From each frame's last submission until the GPU is done (the window side reports it).
    Submission,
}

/// What the scene on show is doing.
enum Phase {
    /// Its world at the new size is being built.
    Build { since: f64 },
    /// Running before it is measured.
    Warm { until: f64 },
    Measure {
        until: f64,
        started: Instant,
        meter: Option<ProcessMeter>,
        gpu0: Option<Duration>,
        acc0: f64,
        gen0: u64,
        frames: Vec<f64>,
    },
}

/// The scene being swept.
struct Current {
    screen: usize,
    rotation: String,
    canvas: [f64; 2],
    scale: f64,
    fps: f32,
    limits: SceneLimits,
    plan: ScenePlan,
    slot: Slot,
    size: Size,
    rung: usize,
    builds: u64,
    phase: Phase,
}

/// A sweep under way in the host.
pub(crate) struct HostSweep {
    req: TuneRequest,
    /// Seconds each size shows for.
    seconds: f64,
    limits: TuneLimits,
    /// Screens shown at once, among which the limits are shared.
    screens: usize,
    queue: VecDeque<(usize, usize)>,
    total: usize,
    number: usize,
    cur: Option<Current>,
    /// A screen's own stage while a sweep's stands in for it.
    parked: Option<(usize, Stage)>,
    painter: Option<Painter>,
    clock: Clock,
    /// GPU seconds the window side has reported, for [`Clock::Submission`].
    pub(crate) gpu_acc: Arc<Mutex<f64>>,
    saved: Option<String>,
}

impl HostSweep {
    /// The screen being swept.
    pub(crate) fn screen(&self) -> Option<usize> {
        self.cur
            .as_ref()
            .map(|c| c.screen)
            .or(self.parked.as_ref().map(|p| p.0))
    }

    pub(crate) fn auto(&self) -> bool {
        self.req.auto
    }

    /// Whether frames are being measured.
    pub(crate) fn measuring(&self) -> bool {
        matches!(
            self.cur.as_ref().map(|c| &c.phase),
            Some(Phase::Measure { .. })
        )
    }

    fn timing(&self) -> GpuTiming {
        match self.clock {
            Clock::None => GpuTiming::None,
            Clock::Kernel => GpuTiming::Kernel,
            Clock::Submission => GpuTiming::Submission,
        }
    }

    /// Where it is, for `status`: the scene's number, how many, and what is on show.
    pub(crate) fn progress(&self) -> Value {
        json!({
            "number": self.number,
            "of": self.total,
            "scene": self.cur.as_ref().map(|c| c.plan.label.clone()),
            "size": self.cur.as_ref().map(|c| c.size),
            "screen": self.screen(),
            "auto": self.req.auto,
        })
    }
}

/// What a step of the sweep came to.
enum Step {
    Going,
    Done,
}

impl Core {
    /// Starts a sweep on the real screen (each size shown for `seconds`).
    pub(crate) fn start_sweep(&mut self, req: TuneRequest, seconds: f64) -> Result<(), String> {
        if self.sweep.is_some() {
            return Err("a sweep is already running".into());
        }
        if !(0.5..=60.0).contains(&seconds) {
            return Err("each size shows for 0.5 to 60 seconds".into());
        }
        let n = self.screens.len();
        let which: Vec<usize> = match req.screen {
            Some(i) if i < n => vec![i],
            Some(i) => return Err(format!("there is no screen {i}")),
            None => {
                let walls: Vec<usize> = (0..n)
                    .filter(|&i| self.screens[i].place == Place::Wallpaper)
                    .collect();
                if walls.is_empty() {
                    (0..n).collect()
                } else {
                    walls
                }
            }
        };
        let mut queue = VecDeque::new();
        for &i in &which {
            let count = self.screens[i].stage.slots().len().min(MAX_SCENES);
            queue.extend((0..count).map(|k| (i, k)));
        }
        if queue.is_empty() {
            return Err("nothing to sweep".into());
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
        let clock = match &self.gpu {
            None => Clock::None,
            Some(_) if crate::power::process_gpu_time().is_some() => Clock::Kernel,
            Some(_) => Clock::Submission,
        };
        let total = queue.len();
        self.sweep_events.clear();
        self.sweep_last = None;
        self.sweep_events.push(TuneEvent::Start {
            screens: which.clone(),
            scenes: total,
            fps: limits.target_fps(self.screen_hz(which[0]), self.settings.fps),
            gpu: self.gpu.as_ref().map(|g| g.adapter.get_info().name),
        });
        self.sweep = Some(HostSweep {
            req,
            seconds,
            limits,
            screens: which.len(),
            queue,
            total,
            number: 0,
            cur: None,
            parked: None,
            painter: None,
            clock,
            gpu_acc: Arc::new(Mutex::new(0.0)),
            saved: None,
        });
        self.apply_pause();
        Ok(())
    }

    /// Stops the sweep; the scenes already done stay saved.
    pub(crate) fn stop_sweep(&mut self, why: &str) -> Result<(), String> {
        let Some(mut sw) = self.sweep.take() else {
            return Err("no sweep is running here".into());
        };
        self.restore(&mut sw);
        self.sweep_events.push(TuneEvent::Failed {
            error: why.to_string(),
        });
        self.sweep_last = Some(json!({"outcome": "stopped", "error": why, "auto": sw.req.auto}));
        self.apply_pause();
        if sw.saved.is_some() {
            let _ = self.reload();
        }
        self.note(format!(
            "the sweep stopped ({why}); the scenes done are saved"
        ));
        Ok(())
    }

    /// The refresh rate of the monitor screen `i` shows on (the first monitor's for every
    /// monitor).
    fn screen_hz(&self, i: usize) -> Option<f32> {
        let m = self.screens.get(i).and_then(|s| s.monitor).unwrap_or(0);
        self.monitor_hz.get(m).copied().flatten()
    }

    /// A frame's work took `secs` (the window side, or the headless loop, says after drawing).
    pub fn frame_done(&mut self, secs: f64) {
        if let Some(Current {
            phase: Phase::Measure { frames, .. },
            ..
        }) = self.sweep.as_mut().and_then(|s| s.cur.as_mut())
        {
            frames.push(secs);
        }
    }

    /// Where the window side adds each frame's GPU time while a sweep measures it that way.
    pub fn sweep_gpu_meter(&self) -> Option<Arc<Mutex<f64>>> {
        self.sweep
            .as_ref()
            .filter(|s| s.clock == Clock::Submission && s.measuring())
            .map(|s| s.gpu_acc.clone())
    }

    /// One step of the sweep, each frame.
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

    /// Puts a screen's own stage back.
    fn restore(&mut self, sw: &mut HostSweep) {
        if let Some((i, mut own)) = sw.parked.take()
            && let Some(s) = self.screens.get_mut(i)
            && s.stage.rotation() == own.rotation()
        {
            own.resume_clock();
            s.stage = own;
        }
    }

    fn finish_sweep(&mut self, mut sw: HostSweep) {
        self.restore(&mut sw);
        self.sweep_events.push(TuneEvent::Done {
            saved: sw.saved.clone(),
            scenes: sw.total,
            gpu_timing: sw.timing(),
        });
        self.sweep_last = Some(json!({"outcome": "done", "scenes": sw.total, "auto": sw.req.auto}));
        self.apply_pause();
        self.note(format!(
            "swept {} scene(s){}",
            sw.total,
            if sw.saved.is_some() {
                "; sizes saved"
            } else {
                ""
            }
        ));
        if sw.saved.is_some()
            && let Err(e) = self.reload()
        {
            self.note(e);
        }
    }

    /// A stage showing one scene of a screen's rotation, as a sweep's stand-in for it.
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
            self.settings.max_cells,
        )?;
        stage.resize(canvas, scale, 0.0);
        Ok(stage)
    }

    fn advance(&mut self, sw: &mut HostSweep, now: f64) -> Result<Step, String> {
        if sw.cur.is_none() {
            let Some((i, k)) = sw.queue.pop_front() else {
                return Ok(Step::Done);
            };
            if sw.parked.as_ref().is_some_and(|p| p.0 != i) {
                self.restore(sw);
            }
            let Some(screen) = self.screens.get(i) else {
                return Ok(Step::Going);
            };
            // The screen's own stage, set aside or still in place.
            let own = match &sw.parked {
                Some((_, s)) => s,
                None => &screen.stage,
            };
            let Some(slot) = own.slots().get(k).cloned() else {
                return Ok(Step::Going);
            };
            let rotation = own.rotation().to_string();
            let canvas = own.canvas();
            let scale = own.status().scale;
            let most = max_cells(&self.settings, self.gpu.as_ref());
            let plan = scene_plan(&slot, &self.lookup, canvas, scale, most);
            let fps = sw.limits.target_fps(self.screen_hz(i), self.settings.fps);
            let limits = SceneLimits::of(&sw.limits, fps, sw.screens);
            let Some(size) = plan.sweep.next_size() else {
                return Ok(Step::Going);
            };
            let mut stage = self.sweep_stage(&rotation, &slot, canvas, scale)?;
            stage.set_tuned(showing(
                &rotation,
                canvas,
                scale,
                &plan.label,
                plan.design,
                size,
            ));
            let builds = stage.builds();
            let replaced = std::mem::replace(&mut self.screens[i].stage, stage);
            if sw.parked.is_none() {
                sw.parked = Some((i, replaced));
            }
            sw.number += 1;
            self.sweep_events.push(TuneEvent::Rung {
                screen: i,
                number: sw.number,
                of: sw.total,
                scene: plan.label.clone(),
                size,
                rung: 1,
                rungs: plan.sweep.planned(),
            });
            sw.cur = Some(Current {
                screen: i,
                rotation,
                canvas,
                scale,
                fps,
                limits,
                plan,
                slot,
                size,
                rung: 1,
                builds,
                phase: Phase::Build { since: now },
            });
            return Ok(Step::Going);
        }
        /// What to do once this frame's look is over.
        enum Next {
            Stay,
            Record,
            Skip(String),
        }
        let next = {
            let cur = sw.cur.as_mut().expect("a scene");
            let Some(screen) = self.screens.get_mut(cur.screen) else {
                sw.cur = None;
                return Ok(Step::Going);
            };
            match &mut cur.phase {
                Phase::Build { since } => {
                    let notes = screen.stage.take_notes();
                    let built =
                        screen.stage.builds() > cur.builds && screen.stage.world().is_some();
                    if built {
                        let world = screen.stage.world().expect("built");
                        let on = steps_on_gpu(&cur.slot.profile, &screen.sim, world);
                        screen.stage.set_world_gpu(on);
                        cur.phase = Phase::Warm {
                            until: now + warmup(sw.seconds),
                        };
                        Next::Stay
                    } else if screen.stage.paused() || now - *since > BUILD_LIMIT {
                        Next::Skip(if notes.is_empty() {
                            format!(
                                "{}: {} could not be built",
                                cur.plan.label,
                                cur.size.describe()
                            )
                        } else {
                            format!("{}: {}", cur.plan.label, notes.join("; "))
                        })
                    } else {
                        Next::Stay
                    }
                }
                Phase::Warm { until } => {
                    if now >= *until {
                        let acc0 = sw.gpu_acc.lock().map(|a| *a).unwrap_or(0.0);
                        cur.phase = Phase::Measure {
                            until: now + (sw.seconds - warmup(sw.seconds)),
                            started: Instant::now(),
                            meter: ProcessMeter::start(),
                            gpu0: crate::power::process_gpu_time(),
                            acc0,
                            gen0: screen.stage.world().map_or(0, |w| w.generation),
                            frames: vec![],
                        };
                    }
                    Next::Stay
                }
                Phase::Measure { until, .. } => {
                    if now >= *until {
                        Next::Record
                    } else {
                        Next::Stay
                    }
                }
            }
        };
        match next {
            Next::Stay => Ok(Step::Going),
            Next::Record => {
                self.record(sw);
                self.next_size(sw, now)
            }
            Next::Skip(why) => {
                self.note(why);
                self.next_size(sw, now)
            }
        }
    }

    /// What the size on show measured, with its picture.
    fn record(&mut self, sw: &mut HostSweep) {
        let acc = sw.gpu_acc.lock().map(|a| *a).unwrap_or(0.0);
        let clock = sw.clock;
        let cur = sw.cur.as_mut().expect("a scene");
        let screen = &self.screens[cur.screen];
        let Phase::Measure {
            started,
            meter,
            gpu0,
            acc0,
            gen0,
            frames,
            ..
        } = &mut cur.phase
        else {
            return;
        };
        let wall = started.elapsed().as_secs_f64().max(1e-6);
        let gpu = match clock {
            Clock::None => None,
            Clock::Kernel => match (*gpu0, crate::power::process_gpu_time()) {
                (Some(a), Some(b)) => Some(b.saturating_sub(a).as_secs_f64() / wall),
                _ => None,
            },
            Clock::Submission => Some((acc - *acc0) / wall),
        };
        let st = screen.stage.status();
        let speed = cur.slot.profile.speed as f64;
        let mut times = std::mem::take(frames);
        let m = Measure {
            cpu: meter.as_ref().map_or(0.0, |m| m.share()),
            gpu: gpu.map(|g| g.clamp(0.0, 1.0)),
            p95: p95(&mut times),
            fps: times.len() as f64 / wall,
            gens_wanted: (speed > 0.0).then_some(speed * wall),
            gens_done: screen
                .stage
                .world()
                .map_or(0, |w| w.generation)
                .saturating_sub(*gen0) as f64,
            on_gpu: matches!(screen.sim.status(), SimStatus::Gpu(_)),
            gpu_timing: sw_timing(clock),
            cells: st.cells as u64,
        };
        let size = cur.size;
        cur.plan.sweep.record(size, st.cell_px, &m, &cur.limits);
        // The picture, drawn by a painter of the sweep's own from the stage on show.
        let image = match &self.gpu {
            Some(g) => {
                let painter = sw
                    .painter
                    .get_or_insert_with(|| Painter::new(&g.device, jetcarp_display::crop::FORMAT));
                jetcarp_display::crop::render(
                    &g.device,
                    &g.queue,
                    painter,
                    &screen.stage,
                    crop_size(cur.canvas),
                    cur.scale as f32,
                )
            }
            None => None,
        };
        let cur = sw.cur.as_mut().expect("a scene");
        let shown = cur.plan.sweep.rungs.last().map(|r| r.size).unwrap_or(size);
        let name = crop_name(cur.screen, &cur.plan.label, &shown);
        let crop = image.and_then(|i| save_crop(&name, &i, None));
        if let Some(r) = cur.plan.sweep.rungs.last_mut() {
            r.crop = crop;
            self.sweep_events.push(TuneEvent::Measured {
                screen: cur.screen,
                scene: cur.plan.label.clone(),
                rung: r.clone(),
            });
        }
    }

    /// On to the scene's next size, or its result once it has none.
    fn next_size(&mut self, sw: &mut HostSweep, now: f64) -> Result<Step, String> {
        let cur = sw.cur.as_mut().expect("a scene");
        if let Some(size) = cur.plan.sweep.next_size() {
            let stage = &mut self.screens[cur.screen].stage;
            cur.builds = stage.builds();
            stage.set_tuned(showing(
                &cur.rotation,
                cur.canvas,
                cur.scale,
                &cur.plan.label,
                cur.plan.design,
                size,
            ));
            cur.size = size;
            cur.rung += 1;
            cur.phase = Phase::Build { since: now };
            self.sweep_events.push(TuneEvent::Rung {
                screen: cur.screen,
                number: sw.number,
                of: sw.total,
                scene: cur.plan.label.clone(),
                size,
                rung: cur.rung,
                rungs: cur.plan.sweep.planned(),
            });
            return Ok(Step::Going);
        }
        let cur = sw.cur.take().expect("a scene");
        let before = tuned_before(&self.settings, cur.screen, &cur.rotation, &cur.plan.label);
        let mut t = tuned_scene(
            &cur.plan.label,
            &cur.plan.tiling,
            cur.plan.design,
            cur.plan.placed,
            &cur.plan.sweep,
            sw.timing(),
            sw.req.pick,
            before.as_ref(),
        );
        if let Some(n) = &cur.plan.note {
            t.note = Some(match t.note {
                Some(m) => format!("{n}; {m}"),
                None => n.clone(),
            });
        }
        if let (Some(n), Some(_)) = (&t.note, before.filter(|b| b.chosen))
            && n.contains("chosen before")
        {
            self.note(format!("{}: {n}", t.scene));
        }
        self.sweep_events.push(TuneEvent::Scene {
            screen: cur.screen,
            result: t.clone(),
        });
        if !sw.req.dry_run {
            sw.saved = Some(save_scene(
                cur.screen,
                &cur.rotation,
                cur.canvas,
                cur.scale,
                sw.limits,
                cur.fps,
                &t,
            )?);
        }
        Ok(Step::Going)
    }
}

fn sw_timing(c: Clock) -> GpuTiming {
    match c {
        Clock::None => GpuTiming::None,
        Clock::Kernel => GpuTiming::Kernel,
        Clock::Submission => GpuTiming::Submission,
    }
}

/// The 95th percentile.
fn p95(times: &mut [f64]) -> f64 {
    if times.is_empty() {
        return 0.0;
    }
    times.sort_by(f64::total_cmp);
    let k = ((times.len() as f64 * 0.95).ceil() as usize).clamp(1, times.len()) - 1;
    times[k]
}
