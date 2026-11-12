//! A stage: a rotation running on one canvas with no window toolkit. It builds each item's world on
//! a background thread, steps it (on the GPU where it can), and moves on when the rotation says so.
//! The background host keeps one per screen; whatever surface shows it asks for a [`TileDraw`].

use crate::caption::{self, Place};
use crate::gpu::{Geometry, TileDraw};
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
}

struct Show {
    world: World,
    geometry: Arc<Geometry>,
    center: [f64; 2],
    zoom: f64,
    speed: f32,
    gpu: bool,
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
    pub seconds_left: Option<f64>,
    pub caption: Vec<String>,
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
    acc: f64,
    last_now: Option<f64>,
    /// Seconds spent not paused: the rotation's clock, so a pause does not use up an item's time.
    play_time: f64,
    epoch: u64,
    failures: usize,
    notes: Vec<String>,
}

/// About how many cells `scene` will have (None: cannot tell before building it).
pub fn estimate_cells(scene: &Scene) -> Option<u64> {
    match scene.tiling.size? {
        SizeSpec::Cells(n) => Some(n as u64),
        SizeSpec::Rect { w, h } => Some(w as u64 * h as u64),
        SizeSpec::Level(_) => None,
    }
}

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
            acc: 0.0,
            last_now: None,
            play_time: 0.0,
            epoch: 1,
            failures: 0,
            notes: vec![],
        })
    }

    pub fn rotation(&self) -> &str {
        &self.rotation
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
        profile.cell_px = cell_pixels(profile.cell_px, self.scale);
        let canvas = self.canvas;
        let scene = profiles::apply_profile(&profile, scene, Some(canvas));
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
        self.failures = 0;
        self.acc = 0.0;
        self.epoch += 1;
        self.show = Some(Show {
            world,
            geometry: Arc::new(geometry),
            center,
            zoom,
            speed: profile.speed,
            gpu: profile.gpu,
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
        let Some(s) = &mut self.show else { return };
        if s.world.cas.is_empty() {
            return;
        }
        sim.on = s.gpu && sim.available();
        let max_speed = s.speed <= 0.0;
        let want = if max_speed {
            u64::MAX
        } else {
            let speed = s.speed as f64;
            self.acc = (self.acc + dt * speed).min(speed * MAX_BACKLOG + 1.0);
            self.acc.floor() as u64
        };
        let mut stepped = false;
        if want > 0 {
            let done = sim.step(&mut s.world, want, deadline);
            if done > 0 {
                stepped = true;
                self.acc = (self.acc - done as f64).max(0.0);
            }
        }
        let notes = &mut self.notes;
        let mut step_cpu = |world: &mut World| {
            stepped = true;
            if let Err(e) = world.apply(Command::Step { gens: 1 }) {
                notes.push(e);
            }
        };
        // After the GPU's turn, which uses the whole budget when it can run the rules.
        if max_speed {
            while Instant::now() < deadline {
                step_cpu(&mut s.world);
            }
        } else {
            while self.acc >= 1.0 && Instant::now() < deadline {
                step_cpu(&mut s.world);
                self.acc -= 1.0;
            }
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
            due || self.player.watches_done(),
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
            return;
        }
        let deadline = Instant::now() + budget;
        self.rotate(self.play_time, now, Clock::local());
        if !self.load_pending {
            self.step(dt, sim, deadline);
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

    pub fn status(&self) -> StageStatus {
        let w = self.world();
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
            seconds_left: self.player.seconds_left(self.play_time),
            caption: self.caption(true),
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
