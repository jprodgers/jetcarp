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
//!
//! A card whose live cells fill only a small part of its world (a ship in a grid it can travel)
//! is framed on them instead, following them as they move ([`frame`]). Its pixel map is its own:
//! a view a little larger than the card, which the card's picture is a window into, drawn again
//! only when the window reaches its edge or the frame zooms out.

mod frame;
pub use frame::PICTURE_HEIGHT;

use eframe::egui::{self, Color32, ColorImage, TextureHandle, TextureOptions};
use jetcarp_core::geom::P;
use jetcarp_core::render::{
    Colorer, RenderOptions, ViewColors, ViewOptions, render_view_with, render_with_colors,
    torus_periods,
};
use jetcarp_core::topology::Topology;
use jetcarp_core::{Scene, Status, World};
use jetcarp_store::Library;
use jetcarp_store::quickstart as qs;
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
/// No card seen for this long: the Start screen isn't showing, and time away doesn't count
/// towards forgetting (coming back from a world finds the previews where they were).
const AWAY_AFTER: f64 = 0.5;
/// Distinct tilings kept built (with their pixel maps), least recently used dropped first.
const TOPO_CACHE: usize = 64;
/// Delay before a card first seen starts (a card that flies by never gets built).
const FIRST_DELAY: Duration = Duration::from_millis(60);

/// How to start a card.
pub enum Source {
    /// A Quick Start entry, read from the library (this file) by handle: its own preview and
    /// mature state (an atlas entry's), or its scene made smaller.
    Entry {
        library: Arc<PathBuf>,
        handle: String,
    },
    /// A scene, shown at preview size (the tests' cards).
    #[cfg(test)]
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
        PixelMap::decode(render_with_colors(world, &enc, &opts))
    }

    /// The map of a `width` x `height` view centred on world point `center` at `scale` pixels
    /// per world unit (a wrap-edge lattice repeating around its grid).
    fn view(world: &World, (width, height): (u32, u32), center: P, scale: f64) -> PixelMap {
        let enc = |i: usize| {
            let v = i as u32 + 1;
            [(v >> 16) as u8, (v >> 8) as u8, v as u8, 0x7f]
        };
        let opts = ViewOptions {
            outline_min_px: 0.0,
            repeat: true,
            colors: Some(ViewColors {
                stage: [0; 4],
                outline: [0; 4],
            }),
        };
        PixelMap::decode(render_view_with(
            world, &enc, width, height, center, scale, &opts,
        ))
    }

    /// Cells from a picture whose cells are coloured by index (alpha 0x7f).
    fn decode(img: jetcarp_core::render::Image) -> PixelMap {
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

    /// The `w` x `h` window of the map at pixel (`x`, `y`), coloured from the world: only the
    /// cells in it are coloured, each once per run of pixels.
    fn paint_window(&self, world: &World, (x, y): (u32, u32), (w, h): (u32, u32)) -> ColorImage {
        let c = Colorer::new(world);
        let stage = ViewColors::for_world(world).stage;
        let stage = Color32::from_rgba_premultiplied(stage[0], stage[1], stage[2], stage[3]);
        let mut pixels = Vec::with_capacity(w as usize * h as usize);
        let mut last = (u32::MAX, stage);
        for row in y..(y + h).min(self.height) {
            let start = (row * self.width + x) as usize;
            let end = start + w.min(self.width.saturating_sub(x)) as usize;
            for &i in &self.cell[start..end] {
                if i == u32::MAX {
                    pixels.push(stage);
                    continue;
                }
                if i != last.0 {
                    let p = c.color(i as usize);
                    last = (i, Color32::from_rgba_premultiplied(p[0], p[1], p[2], p[3]));
                }
                pixels.push(last.1);
            }
        }
        pixels.resize(w as usize * h as usize, stage);
        ColorImage::new([w as usize, h as usize], pixels)
    }
}

/// A card framed on its live cells (see [`frame`]).
struct Framed {
    /// World units across the card's picture.
    width: f64,
    /// The world point at the picture's centre.
    cam: P,
    /// The pixel map the picture is a window into, once drawn.
    map: Option<FrameMap>,
}

/// A framed card's own pixel map: `pad` pixels more than the picture on every side.
struct FrameMap {
    map: PixelMap,
    /// The world point at the map's centre, and pixels per world unit.
    center: P,
    scale: f64,
    /// The picture's size it was drawn for.
    picture: (u32, u32),
    pad: u32,
}

impl Framed {
    /// The card's picture, `side` pixels wide, drawing the map again when the frame zoomed out,
    /// the card changed size, or the picture would leave the map.
    fn paint(&mut self, world: &World, side: u32) -> ColorImage {
        let picture = (side, ((side as f32 * PICTURE_HEIGHT).round() as u32).max(1));
        let scale = picture.0 as f64 / self.width;
        let window = |m: &FrameMap, cam: P| {
            let x = m.pad as f64 + ((cam[0] - m.center[0]) * m.scale).round();
            let y = m.pad as f64 + ((cam[1] - m.center[1]) * m.scale).round();
            let fits = 0.0..=2.0 * m.pad as f64;
            (fits.contains(&x) && fits.contains(&y)).then_some((x as u32, y as u32))
        };
        let current = self
            .map
            .as_ref()
            .filter(|m| m.picture == picture && m.scale == scale)
            .and_then(|m| window(m, self.cam));
        let at = match current {
            Some(at) => at,
            None => {
                // On a torus the frame may have travelled copies away: take it back to the
                // copy nearest the grid (the same picture).
                if let Some(t) = torus_periods(&world.topo) {
                    self.cam = frame::nearest_copy(self.cam, frame::middle(world.topo.bounds), t);
                }
                let pad = (picture.0 / 6).max(12);
                let size = (picture.0 + 2 * pad, picture.1 + 2 * pad);
                self.map = Some(FrameMap {
                    map: PixelMap::view(world, size, self.cam, scale),
                    center: self.cam,
                    scale,
                    picture,
                    pad,
                });
                (pad, pad)
            }
        };
        let m = self.map.as_ref().expect("drawn above");
        m.map.paint_window(world, at, picture)
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
    /// Mean cell size (world units).
    cell: f64,
    /// Framed on its live cells, or shown whole.
    frame: Option<Framed>,
    /// Steps until a card shown whole looks at its live cells again.
    look_in: u32,
}

impl Running {
    /// Frames the card on its live cells, follows them, or shows the whole world again (see
    /// [`frame`]).
    fn reframe(&mut self) {
        let framed = self.frame.is_some();
        if !framed {
            if self.look_in > 0 {
                self.look_in -= 1;
                return;
            }
            self.look_in = frame::LOOK_EVERY;
        }
        let b = match frame::live_box(&self.world, self.frame.as_ref().map(|f| f.cam)) {
            // A dying pattern keeps its frame.
            frame::Live::Empty => return,
            frame::Live::Dense => {
                self.frame = None;
                return;
            }
            frame::Live::Box(b) => b,
        };
        let bounds = self.world.topo.bounds;
        let widest = frame::widest_frame(b, frame::whole_view(bounds, frame::ASPECT), framed);
        let need = frame::frame_width(b, self.cell, frame::ASPECT);
        if need > widest {
            self.frame = None;
            return;
        }
        let width = match &self.frame {
            Some(f) if need <= f.width => f.width,
            Some(_) => (need * frame::HEADROOM).min(widest),
            None => need,
        };
        let edges = torus_periods(&self.world.topo).is_none().then_some(bounds);
        let size = (width, width / frame::ASPECT);
        match &mut self.frame {
            Some(f) => {
                if width != f.width {
                    f.width = width;
                    f.map = None;
                }
                f.cam = frame::follow(f.cam, b, size, self.cell, edges, false);
            }
            None => {
                self.frame = Some(Framed {
                    width,
                    cam: frame::follow(frame::middle(b), b, size, self.cell, edges, true),
                    map: None,
                })
            }
        }
    }

    /// The card's picture: its frame, or the whole world.
    fn picture(&mut self) -> ColorImage {
        match &mut self.frame {
            Some(f) => f.paint(&self.world, self.side),
            None => self.map.paint(&self.world),
        }
    }
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
    lib: Option<(Arc<PathBuf>, Library)>,
    topo: Arc<Mutex<TopoCache>>,
}

impl Worker {
    /// A card's preview scene and mature state, from its source.
    #[allow(clippy::type_complexity)]
    fn fetch(&mut self, source: &Source) -> Result<(Scene, Option<Vec<u8>>, u32), String> {
        match source {
            Source::Entry { library, handle } => {
                if self
                    .lib
                    .as_ref()
                    .is_none_or(|c| !Arc::ptr_eq(&c.0, library))
                {
                    self.lib = Some((library.clone(), Library::open(library.as_ref())));
                }
                let card = qs::card(&self.lib.as_ref().unwrap().1, handle)?;
                Ok((card.preview, card.mature, card.mature_gen))
            }
            #[cfg(test)]
            Source::Scene(s) => {
                let cells = jetcarp_core::quickstart::preview_cells(s, 6000).unwrap_or(6000);
                Ok((jetcarp_core::quickstart::preview_scene(s, cells), None, 0))
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
            qs::apply_mature(&mut world, state, mature_gen);
        }
        let map = self.pixel_map(&key, side, &world);
        let mut run = Box::new(Running {
            start_gen: world.generation,
            cell: jetcarp_core::world::cell_size(&world.topo),
            world,
            scene,
            mature,
            mature_gen,
            rest_until: None,
            side,
            map,
            frame: None,
            look_in: 0,
        });
        run.reframe();
        Ok(run)
    }

    /// Back to the start of a card after a rest (or a very long run).
    fn restart(&mut self, run: &mut Running) -> Result<(), String> {
        let mut world = World::from_scene_on(&run.scene, run.world.topo.clone())?;
        if let Some(state) = &run.mature {
            qs::apply_mature(&mut world, state, run.mature_gen);
        }
        run.start_gen = world.generation;
        run.world = world;
        run.rest_until = None;
        // Framed afresh: the start is small again.
        run.frame = None;
        run.look_in = 0;
        run.reframe();
        Ok(())
    }

    fn work(&mut self, job: Job) -> Done {
        let Job {
            source, run, side, ..
        } = job;
        // A new card: build and paint before any step.
        let Some(mut run) = run else {
            return match self.build(&source, side) {
                Ok(mut run) => {
                    let img = Some(run.picture());
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
                img: Some(run.picture()),
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
        } else if !restart {
            run.reframe();
        }
        let img = Some(run.picture());
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
    let mut worker = Worker { lib: None, topo };
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
    /// When a card was last seen (the screen last showed).
    shown: f64,
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
            shown: f64::NEG_INFINITY,
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

    /// Forget cards off screen for a few seconds while the screen shows (called on every
    /// screen; away from the Start screen, nothing is forgotten).
    pub fn forget(&mut self, now: f64) {
        if now - self.shown > AWAY_AFTER || self.ui.values().all(|c| now - c.seen <= FORGET_AFTER) {
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
        let away = now - self.shown;
        if away > AWAY_AFTER && self.shown.is_finite() {
            // Back on the Start screen: the time away doesn't count.
            for c in self.ui.values_mut() {
                c.seen += away;
            }
        }
        self.shown = now;
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

    /// Forget every card (the entries changed under their handles).
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
            lib: None,
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
        fn make() -> Source {
            Source::Scene(Box::new(scene("B3/S23", SeedSpec::Center)))
        }
        // Frames a quarter of a second apart from `from` to `to`, showing `ids`.
        fn frames(p: &mut Pool, ids: &[&str], from: f64, to: f64) {
            let mut t = from;
            while t <= to + 1e-9 {
                for id in ids {
                    p.see(id, t, make);
                }
                p.end_frame(true, 200);
                p.forget(t);
                t += 0.25;
            }
        }
        let cards = |p: &Pool| -> Vec<String> {
            let mut v: Vec<String> = p
                .state
                .0
                .lock()
                .unwrap()
                .cards
                .keys()
                .map(|k| k.to_string())
                .collect();
            v.sort();
            v
        };
        let mut p = Pool::new();
        frames(&mut p, &["a", "b"], 10.0, 10.0);
        assert_eq!(cards(&p), ["a", "b"]);
        // Off screen for under three seconds: kept.
        frames(&mut p, &["a"], 10.25, 12.5);
        assert_eq!(cards(&p), ["a", "b"]);
        // Over three seconds: forgotten, world and all.
        frames(&mut p, &["a"], 12.75, 13.5);
        assert_eq!(cards(&p), ["a"]);
        // Away from the Start screen (no frames): nothing is forgotten, however long.
        p.forget(60.0);
        assert_eq!(cards(&p), ["a"]);
        // Back, with "a" off screen: the time away doesn't count...
        frames(&mut p, &["c"], 61.0, 63.75);
        assert_eq!(cards(&p), ["a", "c"]);
        // ...but three seconds of showing without it do.
        frames(&mut p, &["c"], 64.0, 64.5);
        assert_eq!(cards(&p), ["c"]);
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

    /// A glider (moving down and right) at cell (x, y) of a `w` x `h` square grid with wrap
    /// edges, as a pasted pattern would be (placed cells keep the scene's size).
    fn glider_at(w: u32, h: u32, x: u32, y: u32) -> Arc<Source> {
        let cells = [(1, 0), (2, 1), (0, 2), (1, 2), (2, 2)]
            .map(|(dx, dy)| [((y + dy) % h) * w + (x + dx) % w, 1])
            .to_vec();
        let mut s = Scene::new(TilingSpec::rect("square", w, h, Boundary::Wrap));
        s.cas = vec![CaSpec::new("B3/S23").seed(SeedSpec::Cells { cells })];
        Arc::new(Source::Scene(Box::new(s)))
    }

    /// The live cells' box, measured around the frame, lies inside the frame.
    fn inside_frame(run: &Running) -> bool {
        let f = run.frame.as_ref().expect("framed");
        let frame::Live::Box(b) = frame::live_box(&run.world, Some(f.cam)) else {
            return false;
        };
        let (hw, hh) = (f.width / 2.0, f.width / frame::ASPECT / 2.0);
        b[0] >= f.cam[0] - hw
            && b[2] <= f.cam[0] + hw
            && b[1] >= f.cam[1] - hh
            && b[3] <= f.cam[1] + hh
    }

    #[test]
    fn a_small_pattern_in_a_large_world_is_framed_and_a_soup_is_not() {
        let mut w = worker();
        // A glider in a 200 x 150 grid: framed, its picture in the card's shape, the glider
        // showing.
        let done = w.work(job(&glider_at(200, 150, 100, 70), None, 240));
        let run = done.run.unwrap();
        let f = run
            .frame
            .as_ref()
            .expect("a glider in a large grid is framed");
        assert!(
            (f.width - frame::MIN_CELLS * frame::ASPECT).abs() < 1e-6,
            "the least frame: {} cells across",
            f.width
        );
        let img = done.img.unwrap();
        assert_eq!(img.size, [240, 149]);
        let stage = img.pixels[0];
        let lit = img.pixels.iter().filter(|&&p| p != stage).count();
        // Five cells of about (149 / 16)^2 pixels each, give or take the cells' edges.
        assert!((5 * 60..5 * 120).contains(&lit), "{lit} pixels of glider");
        // A soup over the same grid: shown whole, at the whole tiling's shape (4:3).
        let soup = src("B3/S23", SeedSpec::Random { density: 0.4 });
        let done = w.work(job(&soup, None, 240));
        assert!(done.run.unwrap().frame.is_none());
        assert_eq!(done.img.unwrap().size, [240, 180]);
        // A glider in a grid not much larger than the least frame: shown whole.
        let done = w.work(job(&glider_at(30, 20, 10, 10), None, 240));
        assert!(done.run.unwrap().frame.is_none());
    }

    #[test]
    fn a_framed_card_follows_its_ship_across_a_wrap_edge() {
        let mut w = worker();
        let source = glider_at(200, 150, 185, 135);
        let done = w.work(job(&source, None, 240));
        let mut run = done.run.unwrap();
        let start = run.frame.as_ref().unwrap().cam;
        let width = run.frame.as_ref().unwrap().width;
        let mut redrawn = 0;
        let mut last_map = None;
        // About 200 generations: the glider crosses both wrap edges.
        while run.world.generation < 200 {
            let d = w.work(job(&source, Some(run), 240));
            run = d.run.unwrap();
            assert!(d.img.is_some_and(|i| i.size == [240, 149]));
            assert!(
                inside_frame(&run),
                "generation {}: the glider left the frame",
                run.world.generation
            );
            let f = run.frame.as_ref().unwrap();
            assert_eq!(f.width, width, "a glider never needs a larger frame");
            let m = f.map.as_ref().unwrap().center;
            if last_map != Some(m) {
                redrawn += 1;
                last_map = Some(m);
            }
        }
        // It moved about 50 cells down and right, round the torus (taken back near the grid).
        let end = run.frame.as_ref().unwrap().cam;
        let moved = frame::nearest_copy(end, start, torus_periods(&run.world.topo).unwrap());
        let (dx, dy) = (moved[0] - start[0], moved[1] - start[1]);
        assert!(
            (40.0..60.0).contains(&dx) && (40.0..60.0).contains(&dy),
            "{dx}, {dy}"
        );
        // The map was drawn again now and then, not every step (each step moves the window).
        assert!(redrawn < 40, "{redrawn} maps for about 100 steps");
    }

    #[test]
    fn a_frame_across_a_wrap_edge_shows_no_seams() {
        // A few cells at the grid's first cells, so the frame straddles both wrap edges of a
        // tiling whose grid has ragged edges.
        let specs = ["trihex", "snubhex", "oct", "rhombitrihex"]
            .map(|f| TilingSpec::rect(f, 40, 30, Boundary::Wrap))
            .into_iter()
            .chain([serde_json::from_value(serde_json::json!({
                "family": "periodic", "params": {"catalog": "k3-021"},
                "size": {"rect": {"w": 32, "h": 21}}, "boundary": "wrap"
            }))
            .unwrap()]);
        for spec in specs {
            let family = serde_json::to_string(&spec).unwrap();
            let mut s = Scene::new(spec);
            let cells = (0..4).map(|i| [i, 1]).collect();
            s.cas = vec![CaSpec::new("B/S012345678").seed(SeedSpec::Cells { cells })];
            let source = Arc::new(Source::Scene(Box::new(s)));
            let mut w = worker();
            let done = w.work(job(&source, None, 240));
            let run = done.run.unwrap();
            assert!(run.frame.is_some(), "{family}: framed");
            let stage = ViewColors::for_world(&run.world).stage;
            let stage = Color32::from_rgba_premultiplied(stage[0], stage[1], stage[2], stage[3]);
            let img = done.img.unwrap();
            let gaps = img.pixels.iter().filter(|&&p| p == stage).count();
            assert_eq!(
                gaps, 0,
                "{family}: {gaps} pixels of stage between the copies"
            );
        }
    }
}
