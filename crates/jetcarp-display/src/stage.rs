//! A stage: a rotation running on one canvas with no window toolkit. It builds each item's world on
//! a background thread, steps it (on the GPU where it can), and moves on when the rotation says so.
//! The background host keeps one per screen; whatever surface shows it asks for a [`TileDraw`].

use crate::caption::{self, Place};
use crate::gpu::{Geometry, TileDraw};
use crate::host_settings::{Size, Tuning};
use crate::pace::{Pace, Pacer, START_OWED};
use crate::profiles::{self, Profile};
use crate::reseed_presets::blobs_mode;
use crate::rotation::{Action, Clock, Observation, Player, Rotation, Slot};
use crate::simgpu::SimBackend;
use crate::view::{self, ViewRequest, Views};
use jetcarp_core::{Command, Scene, SizeSpec, World};
use serde::Serialize;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, TryRecvError, channel};
use std::time::{Duration, Instant};

/// Finds a Quick Start entry's scene.
pub type Lookup = Arc<dyn Fn(&str) -> Option<Scene> + Send + Sync>;

/// Finds Quick Start entries in the user's library by handle, as the app does.
#[cfg(feature = "library")]
pub fn library_lookup() -> Lookup {
    let library = jetcarp_store::Library::open_default();
    Arc::new(move |handle: &str| jetcarp_store::quickstart::scene(&library, handle).ok())
}

/// For a stage whose scenes are all inline (the macOS screen saver's): finds nothing.
pub fn no_lookup() -> Lookup {
    Arc::new(|_| None)
}

/// How long a canvas size must hold before the world is rebuilt to fit it.
const RESIZE_SETTLE: f64 = 0.5;
/// Accumulated generations are capped at this many seconds' worth.
const MAX_BACKLOG: f64 = 0.25;

/// Geometry ids are unique in the process, so one renderer never mistakes one world's for another's.
static NEXT_GEOMETRY: AtomicU64 = AtomicU64::new(1 << 32);

struct Built {
    world: World,
    geometry: Geometry,
    profile: Profile,
    note: Option<String>,
    /// The rotation's slot it was built for.
    slot: usize,
    /// The cell size came from a tuning, not the scene.
    tuned: bool,
}

struct Show {
    /// The rotation's slot this world is (until the next one is built, the current slot may be
    /// another).
    slot: usize,
    world: World,
    geometry: Arc<Geometry>,
    center: [f64; 2],
    zoom: f64,
    speed: f32,
    gpu: bool,
    /// The cell size it was built at, in physical pixels.
    cell_px: f32,
    /// That size came from a tuning, not the scene.
    tuned: bool,
    /// Where this item's settle watches start in the world's reseed policies.
    watch_from: usize,
    /// The player knows this item is running.
    announced: bool,
    since: f64,
}

/// What a stage is doing, for status reports and the tray.
#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct StageStatus {
    pub rotation: String,
    pub label: String,
    /// Counted from 1.
    pub number: usize,
    pub count: usize,
    pub generation: u64,
    pub cells: usize,
    pub paused: bool,
    pub building: bool,
    pub canvas: [f64; 2],
    /// Physical pixels per point.
    pub scale: f64,
    /// The size the world shown's cells show at, in physical pixels (0 before it is built).
    pub cell_px: f32,
    /// An aperiodic patch's level.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub level: Option<u32>,
    /// That size came from *Fit to this computer*, not the scene (left out of reports while Fit
    /// is switched off, #112).
    #[serde(skip_serializing_if = "fit_off")]
    pub tuned: bool,
    /// The grid's width and height in cells, where the tiling has them.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub grid: Option<[u32; 2]>,
    /// Frames a second its screen draws at while it runs ([`Pace`]).
    pub frame_rate: f32,
    /// Generations a second the world on show runs at (0: as fast as it can, or nothing yet).
    pub speed: f32,
    /// Generations a second it reached over its last couple of seconds of running: below
    /// [`speed`](Self::speed) when the world is too big for it (#116). None while paused or just
    /// started.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub measured_speed: Option<f32>,
    pub seconds_left: Option<f64>,
    pub caption: Vec<String>,
    /// What the strobe guard is doing on its surface (#97).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub strobe_guard: Option<jetcarp_core::strobe::guard::Report>,
}

/// Fit to this computer is switched off ([`jetcarp_core::fit::FIT`]): reports say nothing of it.
fn fit_off(_: &bool) -> bool {
    !jetcarp_core::fit::FIT
}

pub struct Stage {
    rotation: String,
    player: Player,
    canvas: [f64; 2],
    /// Physical pixels per point. A profile's `cell_px` is in points, so on a 2x screen a cell
    /// of 1 is 2 pixels across: a cell per pixel costs four times the work there and is too small
    /// to see.
    scale: f64,
    lookup: Lookup,
    max_cells: u64,
    show: Option<Show>,
    building: Option<Receiver<Result<Built, String>>>,
    load_pending: bool,
    resized_at: Option<f64>,
    paused: bool,
    /// Generations owed (see [`Pace::gens_due`]).
    acc: f64,
    last_now: Option<f64>,
    /// What its pace follows: the host's frame rate and pacing, and the monitor's refresh rate.
    pacer: Pacer,
    /// Seconds spent not paused: the rotation's clock, so a pause does not use up an item's time.
    play_time: f64,
    epoch: u64,
    failures: usize,
    notes: Vec<String>,
    /// Cell sizes chosen for this computer, by scene.
    tuned: Option<Tuning>,
    /// Worlds put on show so far.
    builds: u64,
    /// Where the speed it reaches was last measured from: (the time, the generation) (#116).
    rate_mark: Option<(f64, u64)>,
    /// The speed it reached, generations a second.
    measured: Option<f32>,
    /// What the strobe guard of each surface showing it asks (`jetcarp_core::strobe::guard`): by
    /// the painter's number, when it last said, its pace cap and why it paused, if it did.
    guard: Vec<(u64, f64, Option<f32>, Option<String>)>,
    /// What the strobe guard last reported doing here, for status.
    guard_report: Option<jetcarp_core::strobe::guard::Report>,
}

/// A surface's word on the guard counts for this long: one that stopped drawing no longer holds
/// the stage back.
const GUARD_FRESH: f64 = 2.0;

/// Seconds of running a measured speed covers.
const RATE_EVERY: f64 = 2.0;

/// About how many cells `scene` will have (None: cannot tell before building it).
pub fn estimate_cells(scene: &Scene) -> Option<u64> {
    match scene.tiling.size? {
        SizeSpec::Cells(n) => Some(n as u64),
        SizeSpec::Rect { w, h } => Some(w as u64 * h as u64),
        SizeSpec::Level(l) => {
            jetcarp_core::aperiodic::tile_count(&scene.tiling.family, l).map(|n| n as u64)
        }
    }
}

/// The cell size asked for when a patch should simply be fitted to the screen.
pub const FIT_PX: f32 = 0.05;

/// A cell of `points` on a screen of `scale` pixels per point, in pixels. On a scaled screen it is
/// rounded to whole pixels, so cell edges still fall on pixel edges (1 point at 150% is 2 pixels,
/// not 1.5). At 1x a profile's own fractional size is kept.
pub fn cell_pixels(points: f32, scale: f64) -> f32 {
    if scale == 1.0 {
        return points;
    }
    (points as f64 * scale).round().max(1.0) as f32
}

impl Stage {
    /// A stage running `rotation` from `slots`, not yet started.
    pub fn new(
        rotation: &Rotation,
        slots: Vec<Slot>,
        seed: u64,
        canvas: [f64; 2],
        lookup: Lookup,
        max_cells: u64,
    ) -> Result<Stage, String> {
        if slots.is_empty() {
            return Err(format!("rotation {:?} has nothing to show", rotation.name));
        }
        let mut player = Player::new(rotation, slots, seed)?;
        player.start(Clock::local());
        Ok(Stage {
            rotation: rotation.name.clone(),
            player,
            canvas,
            scale: 1.0,
            lookup,
            max_cells,
            show: None,
            building: None,
            load_pending: true,
            resized_at: None,
            paused: false,
            acc: START_OWED,
            last_now: None,
            pacer: Pacer::default(),
            play_time: 0.0,
            epoch: 1,
            failures: 0,
            notes: vec![],
            tuned: None,
            builds: 0,
            rate_mark: None,
            measured: None,
            guard: vec![],
            guard_report: None,
        })
    }

    /// Cell sizes chosen for this computer, by scene (None: each scene's own). The item showing is
    /// built again when its size changes.
    pub fn set_tuned(&mut self, tuned: Option<Tuning>) {
        if tuned == self.tuned {
            return;
        }
        let before = self.current_size();
        self.tuned = tuned;
        if before != self.current_size() && (self.show.is_some() || self.building.is_some()) {
            self.load_pending = true;
        }
    }

    /// The size the current item is built at, and whether a tuning chose it.
    fn current_size(&self) -> Option<(Size, bool)> {
        let slot = self.player.current()?;
        Some(self.size_for(&slot.label, &slot.profile))
    }

    /// The size for an item: its profile's cell size on this screen, or a tuning's (a cell size,
    /// or an aperiodic patch's level).
    fn size_for(&self, label: &str, profile: &Profile) -> (Size, bool) {
        let design = cell_pixels(profile.cell_px, self.scale);
        match self
            .tuned
            .as_ref()
            .and_then(|t| t.size_for(label, design, self.canvas))
        {
            Some(size) => (size, true),
            None => (Size::px(design), false),
        }
    }

    /// What its pace follows (the host sets it each frame; by default 30 frames a second, whatever
    /// the speed).
    pub fn set_pacer(&mut self, pacer: Pacer) {
        self.pacer = pacer;
    }

    /// How fast it draws and its world runs: the world on show's speed, under the strobe guard's
    /// cap, under its pacer (before one is on show, the frame rate).
    pub fn pace(&self) -> Pace {
        let speed = self.scene_speed();
        let speed = match self.guard_cap() {
            // As fast as it can (0) or faster than the cap: the cap.
            Some(c) if speed <= 0.0 || speed > c => c,
            _ => speed,
        };
        self.pacer.pace(speed)
    }

    /// The speed the world on show asks for, generations a second (0: as fast as it can).
    pub fn scene_speed(&self) -> f32 {
        self.show.as_ref().map_or(0.0, |s| s.speed)
    }

    /// The speed it reached lately, generations a second.
    pub fn measured_speed(&self) -> Option<f32> {
        self.measured
    }

    /// What the strobe guard of the surface `key` asks: a pace cap, and a pause with its reason.
    /// The strictest of the surfaces that said so lately holds.
    pub fn set_guard(&mut self, key: u64, now: f64, cap: Option<f32>, paused: Option<String>) {
        let was_paused = self.guard_paused().is_some();
        self.guard.retain(|g| g.0 != key && now - g.1 < GUARD_FRESH);
        if cap.is_some() || paused.is_some() {
            if let Some(p) = &paused
                && !was_paused
            {
                self.notes.push(format!("strobe guard: paused: {p}"));
            }
            self.guard.push((key, now, cap, paused));
        }
    }

    fn guard_fresh(&self) -> impl Iterator<Item = &(u64, f64, Option<f32>, Option<String>)> {
        let now = self.last_now.unwrap_or(0.0);
        self.guard.iter().filter(move |g| now - g.1 < GUARD_FRESH)
    }

    /// What the strobe guard is doing on a surface showing it, for status.
    pub fn set_guard_report(&mut self, report: Option<jetcarp_core::strobe::guard::Report>) {
        self.guard_report = report;
    }

    /// The strobe guard's pace cap, generations a second.
    pub fn guard_cap(&self) -> Option<f32> {
        self.guard_fresh()
            .filter_map(|g| g.2)
            .min_by(f32::total_cmp)
    }

    /// Why the strobe guard paused it, if it did.
    pub fn guard_paused(&self) -> Option<&str> {
        self.guard_fresh().find_map(|g| g.3.as_deref())
    }

    /// Every item of its rotation.
    pub fn slots(&self) -> &[Slot] {
        self.player.slots()
    }

    /// How many worlds it has put on show: a new one has been built when this moves.
    pub fn builds(&self) -> u64 {
        self.builds
    }

    /// Takes up again after not being ticked for a while (a sweep showed something else): the
    /// rotation's clock does not count the time away.
    pub fn resume_clock(&mut self) {
        self.last_now = None;
    }

    pub fn rotation(&self) -> &str {
        &self.rotation
    }

    /// The canvas its worlds are built for, in physical pixels.
    pub fn canvas(&self) -> [f64; 2] {
        self.canvas
    }

    pub fn paused(&self) -> bool {
        self.paused
    }

    /// Pausing freezes generations and the rotation; the picture stays.
    pub fn set_paused(&mut self, paused: bool) {
        self.paused = paused;
        // It may have paused because nothing could be shown: try again.
        if !paused && self.show.is_none() && self.building.is_none() {
            self.failures = 0;
            self.load_pending = true;
        }
    }

    pub fn world(&self) -> Option<&World> {
        self.show.as_ref().map(|s| &s.world)
    }

    /// Whether the world shown steps on the GPU where it can (the scene's choice, until the
    /// tuner decides for a trial).
    pub fn set_world_gpu(&mut self, on: bool) {
        if let Some(s) = &mut self.show {
            s.gpu = on;
        }
    }

    /// The world shown as a scene: what it was built from, its grid's size included, so the
    /// picture can be made again.
    pub fn scene(&self) -> Option<Scene> {
        self.show.as_ref().map(|s| s.world.to_scene())
    }

    /// Problems since the last call.
    pub fn take_notes(&mut self) -> Vec<String> {
        std::mem::take(&mut self.notes)
    }

    /// The canvas changed size (physical pixels) or scale (pixels per point): the world is rebuilt
    /// once they hold.
    pub fn resize(&mut self, canvas: [f64; 2], scale: f64, now: f64) {
        let scale = if scale.is_finite() && scale > 0.0 {
            scale
        } else {
            1.0
        };
        if canvas == self.canvas && scale == self.scale {
            return;
        }
        self.canvas = canvas;
        self.scale = scale;
        // Nothing has been built for the old size yet.
        if self.show.is_some() || self.building.is_some() {
            self.resized_at = Some(now);
        }
    }

    /// Moves by `by` items (+1 next, -1 previous).
    pub fn skip(&mut self, by: i64) -> Result<(), String> {
        if self.player.slots().len() < 2 {
            return Err("this rotation has one item".into());
        }
        if self.player.advance(by, Clock::local()) != Action::Stay {
            self.load_pending = true;
        }
        Ok(())
    }

    /// Builds the current item's world again from its scene, as after the GPU was lost: the
    /// world's geometry was uploaded to a device that is gone.
    pub fn rebuild(&mut self) {
        self.show = None;
        self.building = None;
        self.load_pending = true;
    }

    /// Drops the current world and moves on, because it used too much memory.
    pub fn abandon_item(&mut self, why: &str) {
        let label = self.player.current().map_or("?", |s| &s.label).to_string();
        self.notes.push(format!("{label}: {why}"));
        self.show = None;
        self.building = None;
        self.fail_over();
    }

    /// The current item cannot be shown: on to the next, or pause when none can be.
    fn fail_over(&mut self) {
        self.failures += 1;
        if self.failures >= self.player.slots().len() {
            self.failures = 0;
            self.paused = true;
            self.load_pending = false;
            self.notes.push(format!(
                "nothing in {:?} can be shown; paused",
                self.rotation
            ));
            return;
        }
        // `advance` stays put with nothing else eligible; retrying would repeat the failure.
        if self.player.advance(1, Clock::local()) != Action::Stay {
            self.load_pending = true;
        } else {
            self.paused = true;
            self.load_pending = false;
        }
    }

    fn start_build(&mut self) {
        let Some(slot) = self.player.current().cloned() else {
            return;
        };
        let (mut profile, scene, note) = profiles::resolve(&slot.profile, &*self.lookup);
        // In pixels from here on.
        let (size, tuned) = self.size_for(&slot.label, &profile);
        // A patch of a chosen level is fitted to the screen, whatever its tiles' size comes to.
        profile.cell_px = if size.level.is_some() {
            FIT_PX
        } else {
            size.px
        };
        let canvas = self.canvas;
        let mut scene = profiles::apply_profile(&profile, scene, Some(canvas));
        if let Some(l) = size.level
            && !profiles::is_lattice(&scene)
        {
            scene.tiling.size = Some(SizeSpec::Level(l));
        }
        if let Some(n) = estimate_cells(&scene)
            && n > self.max_cells
        {
            self.notes.push(format!(
                "{}: {n} cells is over the limit of {}",
                slot.label, self.max_cells
            ));
            self.fail_over();
            return;
        }
        let (tx, rx) = channel();
        let id = NEXT_GEOMETRY.fetch_add(1, Ordering::Relaxed);
        let index = self.player.current_index();
        let spawned = std::thread::Builder::new()
            .name("jetcarp-stage-build".into())
            .spawn(move || {
                let built = World::from_scene(&scene).map(|world| {
                    let geometry = Geometry::build(&world.topo, id);
                    Built {
                        world,
                        geometry,
                        profile,
                        note,
                        slot: index,
                        tuned,
                    }
                });
                let _ = tx.send(built);
            });
        match spawned {
            Ok(_) => self.building = Some(rx),
            Err(e) => {
                self.notes.push(format!("{}: {e}", slot.label));
                self.fail_over();
            }
        }
    }

    fn install(&mut self, b: Built, now: f64) {
        let Built {
            mut world,
            geometry,
            profile,
            note,
            slot,
            tuned,
        } = b;
        if let Some(n) = note {
            self.notes.push(n);
        }
        if let Some(from) = profile.reseed_cells {
            let cur = world.reseed_policies().to_vec();
            let scaled = profiles::scale_policies(&cur, from, world.topo.len());
            if scaled != cur
                && let Err(e) = world.apply(Command::SetReseed { policies: scaled })
            {
                self.notes.push(e);
            }
        }
        let (center, zoom) = view::place(&world, self.canvas, profile.cell_px as f64);
        // The size its cells show at (a patch fitted to the screen may show them larger).
        let shown_px = (zoom * view::cell_unit(&world)) as f32;
        self.failures = 0;
        self.acc = START_OWED;
        self.epoch += 1;
        self.builds += 1;
        self.show = Some(Show {
            slot,
            world,
            geometry: Arc::new(geometry),
            center,
            zoom,
            speed: profile.speed,
            gpu: profile.gpu,
            cell_px: shown_px,
            tuned,
            watch_from: 0,
            announced: false,
            since: now,
        });
    }

    fn poll_build(&mut self, now: f64) {
        let Some(rx) = &self.building else { return };
        match rx.try_recv() {
            Err(TryRecvError::Empty) => {}
            Err(TryRecvError::Disconnected) => {
                self.building = None;
                self.notes.push("building a world failed".into());
                self.fail_over();
            }
            Ok(result) => {
                self.building = None;
                match result {
                    // Something newer was asked for while this was built.
                    Ok(_) if self.load_pending => {}
                    Ok(b) => self.install(b, now),
                    Err(e) => {
                        let label = self.player.current().map_or("?", |s| &s.label).to_string();
                        self.notes.push(format!("{label}: {e}"));
                        self.fail_over();
                    }
                }
            }
        }
    }

    fn step(&mut self, dt: f64, sim: &mut SimBackend, deadline: Instant) {
        let pace = self.pace();
        let Some(s) = &mut self.show else { return };
        if s.world.cas.is_empty() {
            return;
        }
        sim.on = s.gpu;
        let max_speed = pace.speed <= 0.0;
        let want = if max_speed {
            u64::MAX
        } else {
            pace.gens_due(&mut self.acc, dt, MAX_BACKLOG)
        };
        let mut done = 0;
        if want > 0 {
            done = sim.step(&mut s.world, want, deadline, !max_speed);
        }
        let mut stepped = done > 0;
        // The rest on the CPU, after the GPU's turn (which uses the whole budget when it can run
        // the rules).
        while done < want && Instant::now() < deadline {
            stepped = true;
            if let Err(e) = s.world.apply(Command::Step { gens: 1 }) {
                self.notes.push(e);
            }
            done += 1;
        }
        if !max_speed {
            self.acc = (self.acc - done as f64).max(0.0);
        }
        if stepped {
            self.epoch += 1;
        }
    }

    /// Runs the rotation. `play` is its clock, `now` the wall clock the caption fades by.
    fn rotate(&mut self, play: f64, now: f64, clock: Clock) {
        let Some(s) = &mut self.show else { return };
        if !s.announced {
            s.announced = true;
            s.watch_from = s.world.reseed_policies().len();
            s.since = now;
            self.player.on_loaded(play, s.world.generation, clock);
            let watches = self.player.current().map(Slot::watches).unwrap_or_default();
            if !watches.is_empty() {
                let mut all = s.world.reseed_policies().to_vec();
                all.extend(watches);
                if let Err(e) = s.world.apply(Command::SetReseed { policies: all }) {
                    self.notes.push(e);
                }
            }
            return;
        }
        let due = self.player.script_due(play);
        let obs = Observation::of(
            &s.world,
            s.watch_from,
            play,
            clock,
            due,
            due || self.player.wants_done(play),
        );
        let action = self.player.tick(&obs);
        if let Some(w) = self.player.take_warning() {
            self.notes.push(format!("rotation: {w}"));
        }
        match action {
            Action::Stay => {}
            Action::Load(_) => self.load_pending = true,
            Action::Reseed => {
                match s.world.apply(Command::Reseed {
                    ca: None,
                    mode: blobs_mode(),
                }) {
                    Ok(_) => self.epoch += 1,
                    Err(e) => self.notes.push(e),
                }
            }
        }
    }

    /// One frame's work: finish or start builds, step for up to `budget`, and run the rotation.
    pub fn tick(&mut self, now: f64, sim: &mut SimBackend, budget: Duration) {
        let elapsed = self.last_now.map_or(0.0, |t| (now - t).max(0.0));
        let dt = elapsed.min(MAX_BACKLOG);
        self.last_now = Some(now);
        if !self.paused {
            self.play_time += elapsed;
        }
        self.poll_build(now);
        if self.resized_at.is_some_and(|t| now - t >= RESIZE_SETTLE) {
            self.resized_at = None;
            self.load_pending = true;
        }
        if self.load_pending && self.building.is_none() {
            self.load_pending = false;
            self.start_build();
        }
        if self.paused || self.load_pending || self.building.is_some() {
            self.rate_mark = None;
            self.measured = None;
            return;
        }
        let deadline = Instant::now() + budget;
        self.rotate(self.play_time, now, Clock::local());
        // Paused by the strobe guard: the rotation goes on, so the item ends in its time, but no
        // generation is shown.
        if !self.load_pending && self.guard_paused().is_none() {
            self.step(dt, sim, deadline);
        }
        self.measure(now);
    }

    /// The speed reached: generations over the last [`RATE_EVERY`] seconds of running. A new
    /// world (its generation back near 0) starts the count again.
    fn measure(&mut self, now: f64) {
        let Some(g) = self.show.as_ref().map(|s| s.world.generation) else {
            return;
        };
        match self.rate_mark {
            Some((t, g0)) if g >= g0 => {
                if now - t >= RATE_EVERY {
                    self.measured = Some(((g - g0) as f64 / (now - t)) as f32);
                    self.rate_mark = Some((now, g));
                }
            }
            _ => {
                self.rate_mark = Some((now, g));
                self.measured = None;
            }
        }
    }

    /// This frame's drawing for a view of `size` physical pixels (None until a world is built).
    pub fn draw(&self, views: &mut Views, view: u64, size: [f32; 2]) -> Option<TileDraw> {
        let s = self.show.as_ref()?;
        let req = ViewRequest {
            view,
            center: s.center,
            zoom: s.zoom,
            size,
            outlines: false,
        };
        Some(views.draw(&s.world, &s.geometry, self.epoch, req).0)
    }

    /// The current world as the canvas shows it, drawn on the CPU: for a still desktop picture.
    pub fn still(&self) -> Option<jetcarp_core::render::Image> {
        let [w, h] = self.canvas;
        self.still_at([w as u32, h as u32])
    }

    /// The same, `size` pixels around the canvas's middle: what a monitor of that size shows (a
    /// monitor smaller than the canvas shows its middle).
    pub fn still_at(&self, size: [u32; 2]) -> Option<jetcarp_core::render::Image> {
        let s = self.show.as_ref()?;
        Some(jetcarp_core::render::render_view(
            &s.world,
            size[0].max(1),
            size[1].max(1),
            s.center,
            s.zoom,
            &jetcarp_core::render::ViewOptions::default(),
        ))
    }

    /// The colour behind the grid.
    pub fn backdrop(&self) -> [u8; 3] {
        self.show.as_ref().map_or([0, 0, 0], |s| {
            let (_, c) = jetcarp_core::render::backdrop(&s.world);
            [c[0], c[1], c[2]]
        })
    }

    fn place(&self) -> Place<'_> {
        Place {
            rotation: &self.rotation,
            number: self.player.current_index() + 1,
            count: self.player.slots().len(),
        }
    }

    /// What the caption says now.
    pub fn caption(&self, generation: bool) -> Vec<String> {
        self.world()
            .map(|w| caption::lines(w, Some(self.place()), generation))
            .unwrap_or_default()
    }

    /// Seconds the current item has been showing.
    pub fn item_age(&self, now: f64) -> f64 {
        self.show.as_ref().map_or(0.0, |s| now - s.since)
    }

    /// What the stage is doing. Just after a skip the old item's world is still on screen while
    /// the new one is built: the numbers are then the new item's (nothing yet), not the old one's.
    pub fn status(&self) -> StageStatus {
        let shown = self
            .show
            .as_ref()
            .filter(|s| s.slot == self.player.current_index());
        let w = shown.map(|s| &s.world);
        let grid = w.and_then(|w| match w.topo.spec.size {
            Some(SizeSpec::Rect { w, h }) => Some([w, h]),
            _ => None,
        });
        let level = w.and_then(|w| match w.topo.spec.size {
            Some(SizeSpec::Level(l)) => Some(l),
            _ => None,
        });
        let pace = self.pace();
        StageStatus {
            rotation: self.rotation.clone(),
            label: self
                .player
                .current()
                .map(|s| s.label.clone())
                .unwrap_or_default(),
            number: self.player.current_index() + 1,
            count: self.player.slots().len(),
            generation: w.map_or(0, |w| w.generation),
            cells: w.map_or(0, |w| w.topo.len()),
            paused: self.paused,
            building: self.building.is_some() || self.load_pending,
            canvas: self.canvas,
            scale: self.scale,
            cell_px: shown.map_or(0.0, |s| s.cell_px),
            tuned: shown.is_some_and(|s| s.tuned),
            grid,
            level,
            frame_rate: pace.fps,
            speed: pace.speed,
            measured_speed: self.measured,
            seconds_left: self.player.seconds_left(self.play_time),
            caption: w
                .map(|w| caption::lines(w, Some(self.place()), true))
                .unwrap_or_default(),
            strobe_guard: self.guard_report.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::cell_pixels;

    #[test]
    fn a_cell_on_a_scaled_screen_is_whole_pixels() {
        assert_eq!(cell_pixels(1.0, 1.0), 1.0);
        assert_eq!(cell_pixels(2.5, 1.0), 2.5, "a profile's own size at 1x");
        assert_eq!(cell_pixels(1.0, 2.0), 2.0);
        assert_eq!(cell_pixels(1.0, 1.5), 2.0);
        assert_eq!(cell_pixels(3.0, 1.25), 4.0);
        assert_eq!(cell_pixels(1.0, 1.25), 1.0);
        assert_eq!(cell_pixels(1.0, 0.5), 1.0, "never below a pixel");
    }
}
