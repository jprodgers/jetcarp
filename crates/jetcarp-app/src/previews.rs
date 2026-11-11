//! Live previews for the Quick Start grid.
//!
//! Only cards on screen run. Each card's world is built, stepped, and coloured on a few
//! background threads (a card's first picture is its mature state, painted before any step);
//! the UI thread only uploads the finished images, at most [`UPLOADS_PER_FRAME`] a frame. The
//! workers visit the visible cards in turn, so everyone slows down evenly when the machine is
//! busy, and a card that is costly to step waits longer between steps. A card off screen for a
//! few seconds is forgotten: world, picture, and texture.
//!
//! Colouring is fast because a card keeps a map from pixels to cells, rendered once for its
//! tiling and shared by every card on that tiling. Topologies are shared the same way.

use eframe::egui::{self, Color32, ColorImage, TextureHandle, TextureOptions};
use jetcarp_core::quickstart;
use jetcarp_core::render::{Colorer, RenderOptions, ViewColors, render_with_colors};
use jetcarp_core::topology::Topology;
use jetcarp_core::{Scene, Status, World};
use jetcarp_store::atlas;
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::PathBuf;
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

/// Cards that run at once (more than fit a large screen of small cards).
pub const MAX_LIVE: usize = 120;
/// Finished pictures uploaded in one frame.
pub const UPLOADS_PER_FRAME: usize = 24;
/// Time between steps of a card.
const TICK: Duration = Duration::from_millis(100);
/// How long a dead or frozen card rests on its last picture before starting over.
const REST: Duration = Duration::from_millis(1400);
/// The UI hasn't drawn cards for this long: it is on another screen, so stop stepping.
const IDLE_AFTER: Duration = Duration::from_millis(400);
/// Seconds off screen before a card is forgotten.
const FORGET_AFTER: f64 = 3.0;
/// Distinct tilings kept built (with their pixel maps), least recently used dropped first.
const TOPO_CACHE: usize = 64;
/// Delay before a card first seen starts (a card that flies by never gets built).
const FIRST_DELAY: Duration = Duration::from_millis(60);

/// How to start a card.
pub enum Source {
    /// An atlas entry, read from its file by id.
    Atlas { path: Arc<PathBuf>, id: i64 },
    /// A built-in or catalog entry; shown at preview size.
    Scene(Box<Scene>),
}

/// Maps a card's image pixels to cells, once for a tiling and picture size.
struct PixelMap {
    width: u32,
    height: u32,
    /// The cell under each pixel, or `u32::MAX` for the stage.
    cell: Vec<u32>,
}

impl PixelMap {
    fn new(world: &World, side: u32) -> PixelMap {
        let opts = RenderOptions {
            max_side: side,
            outline_min_px: 0.0,
            ..Default::default()
        };
        let enc: Vec<jetcarp_core::render::Rgba> = (0..world.topo.len() as u32)
            .map(|i| {
                let v = i + 1;
                [(v >> 16) as u8, (v >> 8) as u8, v as u8, 0x7f]
            })
            .collect();
        let img = render_with_colors(world, &enc, &opts);
        let cell = img
            .rgba
            .as_chunks::<4>()
            .0
            .iter()
            .map(|p| {
                if p[3] == 0x7f {
                    (((p[0] as u32) << 16) | ((p[1] as u32) << 8) | p[2] as u32) - 1
                } else {
                    u32::MAX
                }
            })
            .collect();
        PixelMap {
            width: img.width,
            height: img.height,
            cell,
        }
    }

    fn paint(&self, world: &World) -> ColorImage {
        let c = Colorer::new(world);
        let n = world.topo.len();
        let colors: Vec<Color32> = (0..n)
            .map(|i| {
                let p = c.color(i);
                Color32::from_rgba_premultiplied(p[0], p[1], p[2], p[3])
            })
            .collect();
        let stage = ViewColors::for_world(world).stage;
        let stage = Color32::from_rgba_premultiplied(stage[0], stage[1], stage[2], stage[3]);
        let pixels = self
            .cell
            .iter()
            .map(|&i| {
                if i == u32::MAX {
                    stage
                } else {
                    colors[i as usize]
                }
            })
            .collect();
        ColorImage::new([self.width as usize, self.height as usize], pixels)
    }
}

/// A running card (owned by a worker while it is stepped), boxed: it moves through the shared
/// state many times a second.
type Run = Box<Running>;

struct Running {
    world: World,
    /// The scene and mature state to start over from.
    scene: Scene,
    mature: Option<Vec<u8>>,
    mature_gen: u32,
    /// Generation it started at.
    start_gen: u64,
    /// Resting after dying or freezing until this time; then it starts over.
    rest_until: Option<Instant>,
    side: u32,
    map: Arc<PixelMap>,
}

/// A topology, its pixel maps by side, and when it was last used.
struct TopoEntry {
    topo: Arc<Topology>,
    maps: HashMap<u32, Arc<PixelMap>>,
    used: u64,
}

#[derive(Default)]
struct TopoCache {
    map: HashMap<String, TopoEntry>,
    clock: u64,
}

impl TopoCache {
    fn touch(&mut self, key: &str) -> Option<&mut TopoEntry> {
        self.clock += 1;
        let c = self.clock;
        let e = self.map.get_mut(key)?;
        e.used = c;
        Some(e)
    }

    /// The topology of a key, if built.
    fn topo(&mut self, key: &str) -> Option<Arc<Topology>> {
        self.touch(key).map(|e| e.topo.clone())
    }

    fn insert_topo(&mut self, key: String, topo: Arc<Topology>) -> Arc<Topology> {
        self.clock += 1;
        let used = self.clock;
        if self.map.len() >= TOPO_CACHE
            && let Some(old) = self
                .map
                .iter()
                .min_by_key(|(_, v)| v.used)
                .map(|(k, _)| k.clone())
        {
            self.map.remove(&old);
        }
        let e = self.map.entry(key).or_insert_with(|| TopoEntry {
            topo: topo.clone(),
            maps: HashMap::new(),
            used,
        });
        e.used = used;
        e.topo.clone()
    }

    fn map_for(&mut self, key: &str, side: u32) -> Option<Arc<PixelMap>> {
        self.touch(key).and_then(|e| e.maps.get(&side).cloned())
    }

    fn insert_map(&mut self, key: &str, side: u32, map: Arc<PixelMap>) {
        if let Some(e) = self.touch(key) {
            // A previous card size's maps are of no further use.
            if e.maps.len() >= 3 {
                e.maps.clear();
            }
            e.maps.insert(side, map);
        }
    }
}

/// A card's slot in the shared state.
struct Slot {
    source: Arc<Source>,
    /// A worker has the card's world.
    busy: bool,
    run: Option<Run>,
    /// Picture ready for the UI thread (latest wins).
    img: Option<ColorImage>,
    due: Instant,
    /// Could not be built: leave it alone.
    failed: bool,
}

#[derive(Default)]
struct Shared {
    cards: HashMap<Arc<str>, Slot>,
    /// Cards on screen, in screen order (the workers walk this round-robin).
    order: Vec<Arc<str>>,
    cursor: usize,
    /// Cards with a picture ready, oldest first (one entry per card).
    ready: VecDeque<Arc<str>>,
    /// Step cards, or only paint them once.
    live: bool,
    side: u32,
    /// Last time the UI drew any card.
    stamp: Option<Instant>,
    stop: bool,
}

/// What a worker does next.
enum Pick {
    Job(Job),
    Wait(Duration),
    Stop,
}

struct Job {
    id: Arc<str>,
    source: Arc<Source>,
    run: Option<Run>,
    side: u32,
}

fn pick(g: &mut Shared, now: Instant) -> Pick {
    if g.stop {
        return Pick::Stop;
    }
    let idle = g
        .stamp
        .is_none_or(|t| now.saturating_duration_since(t) > IDLE_AFTER);
    if idle || g.order.is_empty() {
        return Pick::Wait(Duration::from_millis(200));
    }
    let n = g.order.len();
    let mut soonest: Option<Instant> = None;
    for k in 0..n {
        let i = (g.cursor + k) % n;
        let id = g.order[i].clone();
        let live = g.live;
        let Some(s) = g.cards.get_mut(&id) else {
            continue;
        };
        if s.busy || s.failed || s.img.is_some() || (!live && s.run.is_some()) {
            continue;
        }
        if s.due <= now {
            s.busy = true;
            let job = Job {
                id,
                source: s.source.clone(),
                run: s.run.take(),
                side: g.side,
            };
            g.cursor = (i + 1) % n;
            return Pick::Job(job);
        }
        soonest = Some(soonest.map_or(s.due, |d: Instant| d.min(s.due)));
    }
    Pick::Wait(
        soonest
            .map_or(Duration::from_millis(30), |d| {
                d.saturating_duration_since(now)
            })
            .clamp(Duration::from_millis(2), Duration::from_millis(30)),
    )
}

/// The outcome of a job.
struct Done {
    run: Option<Run>,
    img: Option<ColorImage>,
    due: Instant,
    failed: bool,
}

fn fail() -> Done {
    Done {
        run: None,
        img: None,
        due: Instant::now(),
        failed: true,
    }
}

/// Everything a worker keeps between jobs.
struct Worker {
    conn: Option<(Arc<PathBuf>, atlas::AtlasConn)>,
    topo: Arc<Mutex<TopoCache>>,
}

impl Worker {
    /// A card's preview scene and mature state, from its source.
    #[allow(clippy::type_complexity)]
    fn fetch(&mut self, source: &Source) -> Result<(Scene, Option<Vec<u8>>, u32), String> {
        match source {
            Source::Atlas { path, id } => {
                if self.conn.as_ref().is_none_or(|c| !Arc::ptr_eq(&c.0, path)) {
                    self.conn = Some((path.clone(), atlas::open(path)?));
                }
                let card = atlas::load_card(&self.conn.as_ref().unwrap().1, *id)?;
                let mature = card.mature.as_deref().and_then(atlas::inflate_state);
                Ok((card.preview, mature, card.mature_gen))
            }
            Source::Scene(s) => {
                let cells = quickstart::preview_cells(s, 6000).unwrap_or(6000);
                Ok((quickstart::preview_scene(s, cells), None, 0))
            }
        }
    }

    /// The shared topology of a scene's tiling.
    fn topology(&mut self, scene: &Scene) -> Result<(String, Arc<Topology>), String> {
        let key = serde_json::to_string(&scene.tiling).map_err(|e| e.to_string())?;
        if let Some(t) = self.topo.lock().unwrap().topo(&key) {
            return Ok((key, t));
        }
        let built = Arc::new(Topology::build(&scene.tiling)?);
        let t = self
            .topo
            .lock()
            .unwrap()
            .insert_topo(key.clone(), built.clone());
        Ok((key, t))
    }

    fn pixel_map(&self, key: &str, side: u32, world: &World) -> Arc<PixelMap> {
        if let Some(m) = self.topo.lock().unwrap().map_for(key, side) {
            return m;
        }
        let m = Arc::new(PixelMap::new(world, side));
        self.topo.lock().unwrap().insert_map(key, side, m.clone());
        m
    }

    /// Build a card's world at its mature state.
    fn build(&mut self, source: &Source, side: u32) -> Result<Run, String> {
        let (scene, mature, mature_gen) = self.fetch(source)?;
        let (key, topo) = self.topology(&scene)?;
        let mut world = World::from_scene_on(&scene, topo)?;
        if let Some(state) = &mature {
            atlas::apply_mature(&mut world, state, mature_gen);
        }
        let map = self.pixel_map(&key, side, &world);
        Ok(Box::new(Running {
            start_gen: world.generation,
            world,
            scene,
            mature,
            mature_gen,
            rest_until: None,
            side,
            map,
        }))
    }

    /// Back to the start of a card after a rest (or a very long run).
    fn restart(&mut self, run: &mut Running) -> Result<(), String> {
        let mut world = World::from_scene_on(&run.scene, run.world.topo.clone())?;
        if let Some(state) = &run.mature {
            atlas::apply_mature(&mut world, state, run.mature_gen);
        }
        run.start_gen = world.generation;
        run.world = world;
        run.rest_until = None;
        Ok(())
    }

    fn work(&mut self, job: Job) -> Done {
        let Job {
            source, run, side, ..
        } = job;
        // A new card: build and paint before any step.
        let Some(mut run) = run else {
            return match self.build(&source, side) {
                Ok(run) => {
                    let img = Some(run.map.paint(&run.world));
                    Done {
                        run: Some(run),
                        img,
                        due: Instant::now() + TICK,
                        failed: false,
                    }
                }
                Err(e) => {
                    eprintln!("preview: {e}");
                    fail()
                }
            };
        };
        // The card size changed: a new pixel map for the same world.
        if run.side != side
            && let Ok((key, _)) = self.topology(&run.scene)
        {
            run.map = self.pixel_map(&key, side, &run.world);
            run.side = side;
        }
        let t0 = Instant::now();
        // Resting on the last picture.
        if let Some(until) = run.rest_until {
            if t0 < until {
                return Done {
                    run: Some(run),
                    img: None,
                    due: until,
                    failed: false,
                };
            }
            if let Err(e) = self.restart(&mut run) {
                eprintln!("preview: {e}");
                return fail();
            }
            return Done {
                img: Some(run.map.paint(&run.world)),
                run: Some(run),
                due: t0 + TICK,
                failed: false,
            };
        }
        run.world.step();
        if t0.elapsed() < Duration::from_millis(2) {
            run.world.step();
        }
        let gens = run.world.generation.saturating_sub(run.start_gen);
        let (rest, restart) = match run.world.status() {
            Status::Empty => (gens >= 1, false),
            Status::Repeats { period: 1, seen_at } => (run.world.generation > seen_at + 8, false),
            Status::Repeats { seen_at, .. } => (false, run.world.generation > seen_at + 900),
            Status::Running => (false, gens > 6000),
        };
        if rest {
            run.rest_until = Some(t0 + REST);
        } else if restart && let Err(e) = self.restart(&mut run) {
            eprintln!("preview: {e}");
            return fail();
        }
        let img = Some(run.map.paint(&run.world));
        // A costly card waits longer, so cheap ones aren't held back.
        let cost = t0.elapsed();
        let due = run.rest_until.unwrap_or(t0 + TICK.max(cost * 5));
        Done {
            run: Some(run),
            img,
            due,
            failed: false,
        }
    }
}

fn worker_loop(state: Arc<(Mutex<Shared>, Condvar)>, topo: Arc<Mutex<TopoCache>>) {
    let mut worker = Worker { conn: None, topo };
    let (lock, cvar) = &*state;
    loop {
        let job = {
            let mut g = lock.lock().unwrap();
            loop {
                match pick(&mut g, Instant::now()) {
                    Pick::Stop => return,
                    Pick::Job(j) => break j,
                    Pick::Wait(d) => {
                        let r = cvar.wait_timeout(g, d).unwrap();
                        g = r.0;
                    }
                }
            }
        };
        let id = job.id.clone();
        let done = worker.work(job);
        let mut g = lock.lock().unwrap();
        let Some(s) = g.cards.get_mut(&id) else {
            continue; // Forgotten while it ran: the result goes with it.
        };
        s.busy = false;
        s.run = done.run;
        s.due = done.due;
        s.failed = done.failed;
        if let Some(img) = done.img {
            let had = s.img.replace(img).is_some();
            if !had {
                g.ready.push_back(id);
            }
        }
    }
}

/// A card known to the UI thread.
struct UiCard {
    id: Arc<str>,
    tex: Option<TextureHandle>,
    seen: f64,
}

/// The live previews of the cards on screen. Call [`begin_frame`](Self::begin_frame), then
/// [`see`](Self::see) for each card drawn, then [`end_frame`](Self::end_frame).
pub struct Pool {
    state: Arc<(Mutex<Shared>, Condvar)>,
    /// Texture and last-seen time of every card seen recently.
    ui: HashMap<Arc<str>, UiCard>,
    /// Drawn this frame, in screen order.
    vis: Vec<Arc<str>>,
    /// First seen this frame.
    new: Vec<(Arc<str>, Source)>,
}

impl Default for Pool {
    fn default() -> Self {
        Self::new()
    }
}

impl Pool {
    pub fn new() -> Pool {
        let threads = std::thread::available_parallelism()
            .map_or(2, |n| n.get() / 8)
            .clamp(2, 4);
        let state: Arc<(Mutex<Shared>, Condvar)> = Default::default();
        let topo: Arc<Mutex<TopoCache>> = Default::default();
        for k in 0..threads {
            let (s, t) = (state.clone(), topo.clone());
            let _ = std::thread::Builder::new()
                .name(format!("jetcarp-preview-{k}"))
                .spawn(move || worker_loop(s, t));
        }
        Pool {
            state,
            ui: HashMap::new(),
            vis: Vec::new(),
            new: Vec::new(),
        }
    }

    /// Upload finished pictures (a few per frame) and forget cards off screen for a while.
    pub fn begin_frame(&mut self, ctx: &egui::Context, now: f64) {
        let mut imgs = Vec::new();
        {
            let mut g = self.state.0.lock().unwrap();
            for _ in 0..UPLOADS_PER_FRAME {
                let Some(id) = g.ready.pop_front() else { break };
                if let Some(s) = g.cards.get_mut(&id)
                    && let Some(img) = s.img.take()
                {
                    imgs.push((id, img));
                }
            }
        }
        for (id, img) in imgs {
            let Some(c) = self.ui.get_mut(&id) else {
                continue;
            };
            let opts = TextureOptions {
                magnification: egui::TextureFilter::Nearest,
                minification: egui::TextureFilter::Linear,
                ..TextureOptions::NEAREST
            };
            match &mut c.tex {
                Some(t) => t.set(img, opts),
                None => c.tex = Some(ctx.load_texture(format!("qsl-{id}"), img, opts)),
            }
        }
        self.forget(now);
    }

    /// Forget cards off screen for a few seconds (called even when the screen isn't showing).
    pub fn forget(&mut self, now: f64) {
        if self.ui.values().all(|c| now - c.seen <= FORGET_AFTER) {
            return;
        }
        let mut g = self.state.0.lock().unwrap();
        self.ui.retain(|id, c| {
            let keep = now - c.seen <= FORGET_AFTER;
            if !keep {
                g.cards.remove(id);
            }
            keep
        });
    }

    /// A card is on screen this frame; `make` says how to start it the first time.
    pub fn see(&mut self, id: &str, now: f64, make: impl FnOnce() -> Source) {
        if let Some(c) = self.ui.get_mut(id) {
            c.seen = now;
            self.vis.push(c.id.clone());
            return;
        }
        let arc: Arc<str> = id.into();
        self.ui.insert(
            arc.clone(),
            UiCard {
                id: arc.clone(),
                tex: None,
                seen: now,
            },
        );
        self.vis.push(arc.clone());
        self.new.push((arc, make()));
    }

    pub fn texture(&self, id: &str) -> Option<&TextureHandle> {
        self.ui.get(id).and_then(|c| c.tex.as_ref())
    }

    /// Publish the cards drawn this frame (`side`: picture size in pixels).
    pub fn end_frame(&mut self, live: bool, side: u32) {
        let mut vis = std::mem::take(&mut self.vis);
        let mut seen: HashSet<Arc<str>> = HashSet::with_capacity(vis.len());
        vis.retain(|id| seen.insert(id.clone()));
        vis.truncate(MAX_LIVE);
        let now = Instant::now();
        let mut g = self.state.0.lock().unwrap();
        for (id, source) in self.new.drain(..) {
            g.cards.insert(
                id,
                Slot {
                    source: Arc::new(source),
                    busy: false,
                    run: None,
                    img: None,
                    due: now + FIRST_DELAY,
                    failed: false,
                },
            );
        }
        let n = vis.len();
        g.order = vis;
        if g.cursor >= n {
            g.cursor = 0;
        }
        g.live = live;
        g.side = side;
        g.stamp = Some(now);
        drop(g);
        self.state.1.notify_all();
    }

    /// Forget every card (the entries were reloaded from a new atlas file).
    pub fn clear(&mut self) {
        let mut g = self.state.0.lock().unwrap();
        g.cards.clear();
        g.ready.clear();
        g.order.clear();
        drop(g);
        self.ui.clear();
        self.vis.clear();
        self.new.clear();
    }
}

impl Drop for Pool {
    fn drop(&mut self) {
        self.state.0.lock().unwrap().stop = true;
        self.state.1.notify_all();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use jetcarp_core::topology::{Boundary, TilingSpec};
    use jetcarp_core::{CaSpec, SeedSpec};

    fn worker() -> Worker {
        Worker {
            conn: None,
            topo: Default::default(),
        }
    }

    fn scene(rule: &str, seed: SeedSpec) -> Scene {
        let mut s = Scene::new(TilingSpec::rect("square", 64, 48, Boundary::Wrap));
        s.cas = vec![CaSpec::new(rule).seed(seed)];
        s
    }

    fn src(rule: &str, seed: SeedSpec) -> Arc<Source> {
        Arc::new(Source::Scene(Box::new(scene(rule, seed))))
    }

    fn job(source: &Arc<Source>, run: Option<Run>, side: u32) -> Job {
        Job {
            id: "x".into(),
            source: source.clone(),
            run,
            side,
        }
    }

    #[test]
    fn a_card_paints_before_it_steps_and_shares_its_tiling() {
        let source = src("B3/S23", SeedSpec::Random { density: 0.4 });
        let mut w = worker();
        let done = w.work(job(&source, None, 240));
        let run = done.run.expect("built");
        let img = done.img.expect("first picture");
        assert_eq!(run.world.generation, 0, "first paint before any step");
        // Colored: not just the empty stage.
        assert!(img.pixels.iter().any(|&p| p != img.pixels[0]));
        // A step advances it.
        let done = w.work(job(&source, Some(run), 240));
        assert!(done.img.is_some());
        let run = done.run.unwrap();
        assert!(run.world.generation >= 1);
        // A second card on the same tiling uses the same topology.
        let other = src("B36/S23", SeedSpec::Random { density: 0.4 });
        let done2 = w.work(job(&other, None, 240));
        let run2 = done2.run.unwrap();
        assert!(Arc::ptr_eq(&run.world.topo, &run2.world.topo));
    }

    #[test]
    fn a_dead_card_rests_then_starts_over() {
        // Nothing is ever born or survives: it dies on the first step.
        let source = src("B/S", SeedSpec::Random { density: 0.5 });
        let mut w = worker();
        let done = w.work(job(&source, None, 200));
        let first = done.img.as_ref().unwrap().pixels[0];
        assert!(
            done.img
                .as_ref()
                .unwrap()
                .pixels
                .iter()
                .any(|&p| p != first),
            "the seed shows"
        );
        let mut run = done.run.unwrap();
        let start = Instant::now();
        // Step until it rests on its last picture.
        let mut steps = 0;
        while run.rest_until.is_none() {
            let d = w.work(job(&source, Some(run), 200));
            run = d.run.unwrap();
            steps += 1;
            assert!(steps < 20, "it should have died");
        }
        let until = run.rest_until.unwrap();
        assert!(until > start);
        // Before the rest is over it keeps the picture.
        let d = w.work(job(&source, Some(run), 200));
        assert!(d.img.is_none());
        assert!(d.due >= until);
        let mut run = d.run.unwrap();
        // When the rest is over it starts over with the seed showing.
        run.rest_until = Some(Instant::now() - Duration::from_millis(1));
        let d = w.work(job(&source, Some(run), 200));
        let run = d.run.unwrap();
        assert!(run.rest_until.is_none());
        assert_eq!(run.world.generation, 0);
        assert!(
            d.img
                .unwrap()
                .pixels
                .iter()
                .any(|&p| p != Color32::TRANSPARENT)
        );
    }

    #[test]
    fn cards_off_screen_are_forgotten_after_a_while() {
        let mut p = Pool::new();
        let make = || Source::Scene(Box::new(scene("B3/S23", SeedSpec::Center)));
        p.see("a", 10.0, make);
        p.see("b", 10.0, make);
        p.end_frame(true, 200);
        assert_eq!(p.state.0.lock().unwrap().cards.len(), 2);
        // Still on screen: kept.
        p.see("a", 12.0, make);
        p.end_frame(true, 200);
        p.forget(12.5);
        assert_eq!(p.state.0.lock().unwrap().cards.len(), 2);
        // Off screen for over three seconds: forgotten, world and all.
        p.forget(14.0);
        let g = p.state.0.lock().unwrap();
        assert_eq!(g.cards.len(), 1);
        assert!(g.cards.contains_key("a"));
        drop(g);
        p.forget(16.0);
        assert_eq!(p.state.0.lock().unwrap().cards.len(), 0);
    }

    #[test]
    fn workers_visit_cards_in_turn() {
        let mut g = Shared {
            live: true,
            side: 200,
            stamp: Some(Instant::now()),
            ..Default::default()
        };
        let ids: Vec<Arc<str>> = (0..4).map(|i| Arc::<str>::from(i.to_string())).collect();
        let now = Instant::now();
        for id in &ids {
            g.cards.insert(
                id.clone(),
                Slot {
                    source: src("B3/S23", SeedSpec::Center),
                    busy: false,
                    run: None,
                    img: None,
                    due: now,
                    failed: false,
                },
            );
        }
        g.order = ids.clone();
        let mut seen = Vec::new();
        for _ in 0..4 {
            match pick(&mut g, now + Duration::from_millis(5)) {
                Pick::Job(j) => {
                    seen.push(j.id.clone());
                    g.cards.get_mut(&j.id).unwrap().due = now + Duration::from_secs(1);
                    g.cards.get_mut(&j.id).unwrap().busy = false;
                }
                _ => panic!("every card is due"),
            }
        }
        assert_eq!(seen, ids, "round-robin in screen order");
        // A worker finds nothing due now, and waits a short while.
        assert!(matches!(
            pick(&mut g, now + Duration::from_millis(5)),
            Pick::Wait(d) if d <= Duration::from_millis(30)
        ));
        // A stopped UI (another screen) leaves the workers idle.
        g.stamp = Some(now - Duration::from_secs(5));
        for id in &ids {
            g.cards.get_mut(id).unwrap().due = now;
        }
        assert!(matches!(pick(&mut g, now), Pick::Wait(_)));
    }
}
