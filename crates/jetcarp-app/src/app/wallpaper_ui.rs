//! *File → Export wallpaper…*: a tool window that writes desktop-sized PNGs of the world.
//!
//! Sizes: each detected monitor (duplicates winit reports are dropped), every monitor as one
//! span (one combined image, one image per monitor cut so they line up, or both), the common
//! presets, or a custom size. Fits: cover, contain, the app's current view (its centre and zoom,
//! scaled to the output so the image matches what the view shows), or a wrap-edge grid
//! repeated. A fresh grid sized for the image can be built and run first (on the GPU when
//! available); it never replaces the live world unless *Open the fresh grid* is pressed.
//!
//! The export runs on a worker thread with a copy of the world's fields, so the app keeps
//! running. Images carry the same PNG metadata as `jetcarp wallpaper` and MCP `export_wallpaper`.

use super::*;
use crate::monitors::MonitorInfo;
use jetcarp_core::wallpaper::{self, Fit, MonitorRect};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc as SArc, Mutex};

/// The key of the window (popout registry and settings).
pub(super) const KEY: &str = "wallpaper";
/// Largest fresh grid built here (cells).
const MAX_FRESH_CELLS: u64 = 16_000_000;
/// Longest side of the preview thumbnail (points).
const PREVIEW: f32 = 340.0;

/// The image size.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(super) enum WpSize {
    /// A detected monitor (its winit index).
    Monitor {
        index: usize,
    },
    /// Every monitor as one span.
    Span,
    /// One of `wallpaper::PRESETS`.
    Preset {
        name: String,
    },
    Custom {
        width: u32,
        height: u32,
    },
}

/// What a span export writes.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(super) enum SpanImages {
    Combined,
    PerMonitor,
    Both,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(super) enum FitChoice {
    Cover,
    Contain,
    /// The app's camera.
    View,
    Repeat,
}

/// The dialog's choices, saved in the app settings.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub(super) struct WallpaperSettings {
    pub size: WpSize,
    pub span_images: SpanImages,
    /// Span layout as `WxH+X+Y,...`; empty: the detected monitors.
    pub span_layout: String,
    pub fit: FitChoice,
    /// Tile size for Repeat (pixels).
    pub repeat_px: f64,
    pub fresh: bool,
    /// Tile size of a fresh grid (pixels).
    pub cell_px: f64,
    /// Generations to run a fresh grid before rendering.
    pub gens: u64,
    pub gpu: bool,
    /// Draw tile outlines when tiles are at least this many pixels across (0: never).
    pub outlines: f64,
    /// Colour every CA with this palette (None: as they are).
    pub palette: Option<String>,
    /// Output folder (None: `<Pictures>/JETCARP/wallpapers`).
    pub folder: Option<PathBuf>,
}

impl Default for WallpaperSettings {
    fn default() -> Self {
        WallpaperSettings {
            size: WpSize::Monitor { index: 0 },
            span_images: SpanImages::Both,
            span_layout: String::new(),
            fit: FitChoice::Cover,
            repeat_px: 4.0,
            fresh: false,
            cell_px: 4.0,
            gens: 300,
            gpu: true,
            outlines: 0.0,
            palette: None,
            folder: None,
        }
    }
}

/// One entry of the size menu.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct SizeOption {
    pub size: WpSize,
    pub label: String,
    pub width: u32,
    pub height: u32,
}

/// The monitors once each (winit may list one twice).
pub(super) fn unique_monitors(monitors: &[MonitorInfo]) -> Vec<&MonitorInfo> {
    monitors.iter().filter(|m| m.same_as.is_none()).collect()
}

/// Monitor rectangles for a span, from the detected positions. When positions overlap (Wayland
/// may report every monitor at (0, 0)), the monitors are laid out left to right instead.
pub(super) fn detected_rects(monitors: &[MonitorInfo]) -> Vec<MonitorRect> {
    let ms = unique_monitors(monitors);
    let rects: Vec<MonitorRect> = ms
        .iter()
        .map(|m| MonitorRect {
            x: m.position_px[0] as i64,
            y: m.position_px[1] as i64,
            width: m.size_px[0],
            height: m.size_px[1],
        })
        .collect();
    let overlap = |a: &MonitorRect, b: &MonitorRect| {
        a.x < b.x + b.width as i64
            && b.x < a.x + a.width as i64
            && a.y < b.y + b.height as i64
            && b.y < a.y + a.height as i64
    };
    let any = (0..rects.len()).any(|i| (i + 1..rects.len()).any(|j| overlap(&rects[i], &rects[j])));
    if !any {
        return rects;
    }
    let mut x = 0;
    rects
        .iter()
        .map(|r| {
            let out = MonitorRect { x, y: 0, ..*r };
            x += r.width as i64;
            out
        })
        .collect()
}

/// `WxH+X+Y,...` for a layout (what `wallpaper::parse_monitors` reads).
pub(super) fn layout_text(rects: &[MonitorRect]) -> String {
    rects
        .iter()
        .map(|r| format!("{}x{}{:+}{:+}", r.width, r.height, r.x, r.y))
        .collect::<Vec<_>>()
        .join(",")
}

/// Every size the dialog offers: the monitors, the span (two or more monitors), the presets.
pub(super) fn size_options(monitors: &[MonitorInfo]) -> Vec<SizeOption> {
    let mut v = vec![];
    for m in unique_monitors(monitors) {
        v.push(SizeOption {
            size: WpSize::Monitor { index: m.index },
            label: format!(
                "Monitor {}: {} ({} × {})",
                m.index,
                m.name.as_deref().unwrap_or("unnamed"),
                m.size_px[0],
                m.size_px[1]
            ),
            width: m.size_px[0],
            height: m.size_px[1],
        });
    }
    let rects = detected_rects(monitors);
    if rects.len() >= 2 {
        let (_, _, w, h) = wallpaper::span_box(&rects);
        v.push(SizeOption {
            size: WpSize::Span,
            label: format!("All {} monitors as a span ({w} × {h})", rects.len()),
            width: w,
            height: h,
        });
    }
    for (name, w, h) in wallpaper::PRESETS {
        v.push(SizeOption {
            size: WpSize::Preset {
                name: name.to_string(),
            },
            label: format!("{name} ({w} × {h})"),
            width: *w,
            height: *h,
        });
    }
    v
}

/// The image size, and the monitors of a span.
pub(super) fn resolve_size(
    s: &WallpaperSettings,
    monitors: &[MonitorInfo],
) -> Result<(u32, u32, Option<Vec<MonitorRect>>), String> {
    match &s.size {
        WpSize::Monitor { index } => {
            let m = unique_monitors(monitors)
                .into_iter()
                .find(|m| m.index == *index)
                .ok_or_else(|| format!("monitor {index} is not connected; pick another size"))?;
            Ok((m.size_px[0], m.size_px[1], None))
        }
        WpSize::Span => {
            let rects = if s.span_layout.trim().is_empty() {
                detected_rects(monitors)
            } else {
                wallpaper::parse_monitors(&s.span_layout)?
            };
            if rects.is_empty() {
                return Err("no monitors detected; type the layout as WxH+X+Y,…".into());
            }
            let (_, _, w, h) = wallpaper::span_box(&rects);
            if w as u64 * h as u64 > wallpaper::MAX_PIXELS {
                return Err(format!(
                    "the span is {w} × {h}, more than {} pixels",
                    wallpaper::MAX_PIXELS
                ));
            }
            Ok((w, h, Some(rects)))
        }
        WpSize::Preset { name } => {
            let (w, h) = wallpaper::parse_size(name)?;
            Ok((w, h, None))
        }
        WpSize::Custom { width, height } => {
            let (w, h) = wallpaper::parse_size(&format!("{width}x{height}"))?;
            Ok((w, h, None))
        }
    }
}

/// Each monitor's piece of a span image: (left, top, width, height) in span pixels, the same
/// cuts `wallpaper::split` makes.
pub(super) fn span_cuts(rects: &[MonitorRect]) -> Vec<(u32, u32, u32, u32)> {
    let (x0, y0, _, _) = wallpaper::span_box(rects);
    rects
        .iter()
        .map(|m| ((m.x - x0) as u32, (m.y - y0) as u32, m.width, m.height))
        .collect()
}

/// The app's camera (world point at the view centre, points per world unit, view size in
/// points) as a fit for a `width` x `height` image: the same centre, the scale grown so the whole
/// visible view fits the image (an image of another shape shows a little more on two sides).
pub(super) fn view_fit(cam: [f64; 2], zoom: f64, view: [f32; 2], width: u32, height: u32) -> Fit {
    let k = (width as f64 / view[0].max(1.0) as f64).min(height as f64 / view[1].max(1.0) as f64);
    Fit::View {
        center: cam,
        scale: zoom * k,
    }
}

/// A fit for a `width` x `height` image, given the choice and the camera.
pub(super) fn fit_for(
    s: &WallpaperSettings,
    cam: [f64; 2],
    zoom: f64,
    view: [f32; 2],
    width: u32,
    height: u32,
) -> Fit {
    match s.fit {
        FitChoice::Cover => Fit::Cover,
        FitChoice::Contain => Fit::Contain,
        // A fresh grid is not what the camera looks at.
        FitChoice::View if s.fresh => Fit::Cover,
        FitChoice::View => view_fit(cam, zoom, view, width, height),
        FitChoice::Repeat => Fit::Repeat {
            cell_px: s.repeat_px,
        },
    }
}

/// The same fit for a smaller copy of the image (`k` = preview width / image width).
pub(super) fn scaled_fit(fit: &Fit, k: f64) -> Fit {
    match *fit {
        Fit::View { center, scale } => Fit::View {
            center,
            scale: scale * k,
        },
        Fit::Repeat { cell_px } => Fit::Repeat {
            cell_px: (cell_px * k).max(0.25),
        },
        ref f => f.clone(),
    }
}

/// A copy of what renders: the fields, CAs, and palettes (not the log or the RNG).
pub(super) fn snapshot(w: &World) -> World {
    let mut s = World::on_topology(w.topo.clone(), 0);
    s.fields = w.fields.clone();
    s.cas = w.cas.clone();
    s.palettes = w.palettes.clone();
    s.tables = w.tables.clone();
    s.generation = w.generation;
    s
}

/// Where the world for a wallpaper comes from.
pub(super) enum Source {
    /// A copy of the live world; `meta` is its PNG metadata (taken from the live world).
    Snapshot {
        world: Box<World>,
        meta: Vec<(&'static str, String)>,
    },
    /// A fresh grid of this scene's tiling sized for the image, run `gens` generations.
    Fresh {
        scene: Box<Scene>,
        cell_px: f64,
        gens: u64,
        gpu: bool,
    },
}

/// One export.
pub(super) struct Job {
    pub source: Source,
    pub width: u32,
    pub height: u32,
    pub monitors: Option<Vec<MonitorRect>>,
    pub images: SpanImages,
    pub fit: Fit,
    pub outlines: f64,
    pub palette: Option<String>,
    pub dir: PathBuf,
    /// File name stem (the scene name).
    pub name: String,
}

/// What an export wrote.
pub(super) struct Done {
    pub files: Vec<(PathBuf, u32, u32)>,
    /// The fresh grid and its scene, for *Open the fresh grid*.
    pub fresh: Option<(Box<World>, Box<Scene>)>,
    pub secs: f64,
}

/// Progress shared with the worker.
#[derive(Default)]
pub(super) struct Progress {
    pub stage: String,
    pub frac: f32,
    pub done: Option<Result<Done, String>>,
}

type Shared = SArc<Mutex<Progress>>;

fn set_stage(p: &Shared, stage: impl Into<String>, frac: f32) {
    if let Ok(mut g) = p.lock() {
        g.stage = stage.into();
        g.frac = frac;
    }
}

/// Run an export (on the worker thread, or directly in tests).
pub(super) fn export(job: Job, progress: &Shared, cancel: &AtomicBool) -> Result<Done, String> {
    let t0 = std::time::Instant::now();
    let (world, meta, fresh_scene) = match job.source {
        Source::Snapshot { world, meta } => (world, meta, None),
        Source::Fresh {
            scene,
            cell_px,
            gens,
            gpu,
        } => {
            set_stage(progress, "Building the grid", 0.02);
            let mut sc = *scene;
            sc.tiling = wallpaper::fresh_tiling(&sc.tiling, job.width, job.height, cell_px);
            sc.commands.clear();
            sc.record = None;
            sc.view = None;
            let mut w = World::from_scene(&sc)?;
            if let Some(p) = &job.palette {
                set_palette(&mut w, p)?;
            }
            let mut sim = (gpu && gens > 0)
                .then(jetcarp_gpu::GpuSim::headless)
                .and_then(|r| r.ok());
            let chunk = (gens / 50).max(1);
            let mut done = 0;
            while done < gens {
                if cancel.load(Ordering::Relaxed) {
                    return Err("cancelled".into());
                }
                let n = chunk.min(gens - done);
                match sim.as_mut() {
                    Some(s) => {
                        s.step_mixed(&mut w, n)?;
                    }
                    None => {
                        w.apply(Command::Step { gens: n })?;
                    }
                }
                done += n;
                set_stage(
                    progress,
                    format!(
                        "Running generation {done} of {gens}{}",
                        if sim.is_some() { " (GPU)" } else { "" }
                    ),
                    0.05 + 0.75 * done as f32 / gens as f32,
                );
            }
            let mut meta = render::png_metadata(&w);
            let mut named = w.to_scene();
            named.name = sc.name.clone();
            named.description = sc.description.clone();
            if let Some(m) = meta.iter_mut().find(|m| m.0 == "jetcarp:scene") {
                m.1 = named.to_json();
            }
            (Box::new(w), meta, Some(Box::new(named)))
        }
    };
    let mut world = world;
    if fresh_scene.is_none()
        && let Some(p) = &job.palette
    {
        // The copy only: the live world keeps its palettes.
        for c in &mut world.cas {
            c.palette = p.clone();
        }
    }
    if cancel.load(Ordering::Relaxed) {
        return Err("cancelled".into());
    }
    set_stage(
        progress,
        format!("Rendering {} × {}", job.width, job.height),
        0.82,
    );
    let img = wallpaper::wallpaper(&world, job.width, job.height, &job.fit, job.outlines)?;
    set_stage(progress, "Writing", 0.95);
    std::fs::create_dir_all(&job.dir).map_err(|e| format!("{}: {e}", job.dir.display()))?;
    let mut meta = meta;
    meta.push((
        "jetcarp:wallpaper",
        serde_json::to_string(&job.fit).unwrap_or_default(),
    ));
    let base = jetcarp_script::out::sanitize(if job.name.is_empty() {
        "wallpaper"
    } else {
        &job.name
    });
    let stamp = jetcarp_script::out::stamp();
    let mut files = vec![];
    let (combined, parts) = match (&job.monitors, job.images) {
        (Some(_), SpanImages::PerMonitor) => (false, true),
        (Some(_), SpanImages::Both) => (true, true),
        _ => (true, false),
    };
    if combined {
        let tag = if job.monitors.is_some() { "span-" } else { "" };
        let path = job.dir.join(format!(
            "{base}-{tag}{}x{}-{stamp}.png",
            job.width, job.height
        ));
        render::write_png(&path, &img, &meta)?;
        files.push((path, img.width, img.height));
    }
    if parts && let Some(m) = &job.monitors {
        for (k, (mon, part)) in m.iter().zip(wallpaper::split(&img, m)).enumerate() {
            let path = job.dir.join(format!(
                "{base}-{}-{}x{}-{stamp}.png",
                k + 1,
                mon.width,
                mon.height
            ));
            let mut mm = meta.clone();
            mm.push((
                "jetcarp:monitor",
                format!("{}x{}{:+}{:+}", mon.width, mon.height, mon.x, mon.y),
            ));
            render::write_png(&path, &part, &mm)?;
            files.push((path, part.width, part.height));
        }
    }
    let fresh = fresh_scene.map(|s| (world, s));
    Ok(Done {
        files,
        fresh,
        secs: t0.elapsed().as_secs_f64(),
    })
}

fn set_palette(w: &mut World, p: &str) -> Result<(), String> {
    let ids: Vec<u32> = w.cas.iter().map(|c| c.id).collect();
    for id in ids {
        w.apply(Command::UpdateCa {
            id,
            change: CaChange {
                palette: Some(p.to_string()),
                ..Default::default()
            },
        })?;
    }
    Ok(())
}

/// Open a folder in the file manager.
fn open_folder(dir: &std::path::Path) {
    #[cfg(target_os = "windows")]
    let cmd = "explorer";
    #[cfg(target_os = "macos")]
    let cmd = "open";
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    let cmd = "xdg-open";
    let _ = std::process::Command::new(cmd).arg(dir).spawn();
}

/// Files written: path, width, height.
type Written = Vec<(PathBuf, u32, u32)>;

/// A running or finished export.
struct Running {
    progress: Shared,
    cancel: SArc<AtomicBool>,
}

/// The dialog's state.
#[derive(Default)]
pub(super) struct WallpaperUi {
    pub open: bool,
    pub settings: WallpaperSettings,
    running: Option<Running>,
    /// The last result: files written, or an error.
    last: Option<Result<(Written, f64), String>>,
    /// The last fresh grid, kept for *Open the fresh grid*.
    fresh: Option<(Box<World>, Box<Scene>)>,
    preview: Option<egui::TextureHandle>,
    /// What the preview shows: settings and camera as JSON, the world revision, and when.
    preview_key: String,
    preview_rev: u64,
    preview_at: f64,
    /// `JETCARP_WALLPAPER=export`: press Export once the scene has loaded.
    pub auto_export: bool,
}

impl JetcarpApp {
    fn wallpaper_dir(&self) -> PathBuf {
        self.wallpaper
            .settings
            .folder
            .clone()
            .unwrap_or_else(|| self.out_root.join("wallpapers"))
    }

    /// The job for the dialog's settings.
    fn wallpaper_job(&self) -> Result<Job, String> {
        let s = &self.wallpaper.settings;
        let (width, height, monitors) = resolve_size(s, self.shell.monitors.list())?;
        let fit = fit_for(
            s,
            self.cam,
            self.zoom,
            [self.view_size.x, self.view_size.y],
            width,
            height,
        );
        if matches!(fit, Fit::Repeat { .. }) && render::torus_periods(&self.world.topo).is_none() {
            return Err("Repeat needs a grid with wrap edges".into());
        }
        let scene = self.current_scene();
        let source = if s.fresh {
            let spec = wallpaper::fresh_tiling(&scene.tiling, width, height, s.cell_px);
            let cells = fresh_cells(&spec);
            if cells > MAX_FRESH_CELLS {
                return Err(format!(
                    "a fresh grid of about {cells} cells is too large (at most {MAX_FRESH_CELLS}); raise the tile size"
                ));
            }
            // The world's CAs and palettes as they are now (not as the scene started).
            let mut sc = scene.clone();
            sc.cas = self.world.cas.iter().map(|c| c.spec()).collect();
            sc.palettes = self.world.palettes.clone();
            sc.reseed = self.world.reseed_policies().to_vec();
            Source::Fresh {
                scene: Box::new(sc),
                cell_px: s.cell_px,
                gens: s.gens,
                gpu: s.gpu && self.sim.available(),
            }
        } else {
            let mut meta = render::png_metadata(&self.world);
            if let Some(m) = meta.iter_mut().find(|m| m.0 == "jetcarp:scene") {
                m.1 = scene.to_json();
            }
            Source::Snapshot {
                world: Box::new(snapshot(&self.world)),
                meta,
            }
        };
        Ok(Job {
            source,
            width,
            height,
            monitors,
            images: s.span_images,
            fit,
            outlines: s.outlines,
            palette: s.palette.clone(),
            dir: self.wallpaper_dir(),
            name: self.scene_name.clone(),
        })
    }

    fn start_wallpaper_export(&mut self) {
        let job = match self.wallpaper_job() {
            Ok(j) => j,
            Err(e) => {
                self.wallpaper.last = Some(Err(e));
                return;
            }
        };
        let progress: Shared = Default::default();
        let cancel = SArc::new(AtomicBool::new(false));
        let (p, c) = (progress.clone(), cancel.clone());
        std::thread::Builder::new()
            .name("jetcarp-wallpaper".into())
            .spawn(move || {
                let r = export(job, &p, &c);
                if let Ok(mut g) = p.lock() {
                    g.done = Some(r);
                }
            })
            .expect("spawn wallpaper thread");
        self.wallpaper.last = None;
        self.wallpaper.running = Some(Running { progress, cancel });
    }

    /// Collect a finished export.
    fn poll_wallpaper(&mut self) {
        let Some(r) = &self.wallpaper.running else {
            return;
        };
        let done = r.progress.lock().ok().and_then(|mut g| g.done.take());
        let Some(done) = done else { return };
        self.wallpaper.running = None;
        match done {
            Ok(d) => {
                let n = d.files.len();
                self.message = Some((
                    format!(
                        "Wrote {n} wallpaper{} in {:.1} s: {}",
                        if n == 1 { "" } else { "s" },
                        d.secs,
                        d.files
                            .first()
                            .map(|f| f.0.display().to_string())
                            .unwrap_or_default()
                    ),
                    false,
                ));
                if d.fresh.is_some() {
                    self.wallpaper.fresh = d.fresh;
                }
                self.wallpaper.last = Some(Ok((d.files, d.secs)));
            }
            Err(e) => self.wallpaper.last = Some(Err(e)),
        }
    }

    /// Re-render the preview thumbnail when the settings, camera, or world changed.
    fn wallpaper_preview(&mut self, ctx: &egui::Context, width: u32, height: u32, fit: &Fit) {
        let now = ctx.input(|i| i.time);
        let key = format!(
            "{}|{width}x{height}|{}",
            serde_json::to_string(&self.wallpaper.settings).unwrap_or_default(),
            serde_json::to_string(fit).unwrap_or_default()
        );
        let rev = self.world.revision();
        let stale_world =
            rev != self.wallpaper.preview_rev && now - self.wallpaper.preview_at > 1.0;
        if key == self.wallpaper.preview_key && !stale_world && self.wallpaper.preview.is_some() {
            return;
        }
        let k = (PREVIEW as f64 / width.max(height) as f64).min(1.0);
        let (pw, ph) = (
            ((width as f64 * k).round() as u32).max(8),
            ((height as f64 * k).round() as u32).max(8),
        );
        let small = scaled_fit(fit, pw as f64 / width as f64);
        let img = if let Some(p) = &self.wallpaper.settings.palette {
            let mut w = snapshot(&self.world);
            for c in &mut w.cas {
                c.palette = p.clone();
            }
            wallpaper::wallpaper(&w, pw, ph, &small, 0.0)
        } else {
            wallpaper::wallpaper(&self.world, pw, ph, &small, 0.0)
        };
        self.wallpaper.preview_key = key;
        self.wallpaper.preview_rev = rev;
        self.wallpaper.preview_at = now;
        let Ok(img) = img else {
            self.wallpaper.preview = None;
            return;
        };
        let ci = egui::ColorImage::from_rgba_unmultiplied(
            [img.width as usize, img.height as usize],
            &img.rgba,
        );
        match &mut self.wallpaper.preview {
            Some(t) => t.set(ci, egui::TextureOptions::LINEAR),
            None => {
                self.wallpaper.preview =
                    Some(ctx.load_texture("wallpaper-preview", ci, egui::TextureOptions::LINEAR))
            }
        }
    }

    /// The Export wallpaper window.
    pub(super) fn wallpaper_window(&mut self, ctx: &egui::Context) {
        self.poll_wallpaper();
        if self.wallpaper.running.is_some() {
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }
        if self.wallpaper.auto_export && self.building.is_none() && self.wallpaper.running.is_none()
        {
            self.wallpaper.auto_export = false;
            self.start_wallpaper_export();
        }
        if !self.wallpaper.open {
            return;
        }
        let monitors: Vec<MonitorInfo> = self.shell.monitors.list().to_vec();
        let options = size_options(&monitors);
        let resolved = resolve_size(&self.wallpaper.settings, &monitors);
        let wraps = render::torus_periods(&self.world.topo).is_some();
        let fit = resolved.as_ref().ok().map(|(w, h, _)| {
            fit_for(
                &self.wallpaper.settings,
                self.cam,
                self.zoom,
                [self.view_size.x, self.view_size.y],
                *w,
                *h,
            )
        });
        if let (Ok((w, h, _)), Some(f)) = (&resolved, &fit)
            && (wraps || !matches!(f, Fit::Repeat { .. }))
        {
            self.wallpaper_preview(ctx, *w, *h, f);
        }
        let names = self.world.palette_names();
        let gpu_ok = self.sim.available();
        let fresh_cells_est = resolved.as_ref().ok().map(|(w, h, _)| {
            let spec = wallpaper::fresh_tiling(
                &self.world.topo.spec,
                *w,
                *h,
                self.wallpaper.settings.cell_px,
            );
            fresh_cells(&spec)
        });
        let dir = self.wallpaper_dir();
        let mut open = self.wallpaper.open;
        let mut export = false;
        let mut cancel = false;
        let mut open_fresh = false;
        let progress = self
            .wallpaper
            .running
            .as_ref()
            .and_then(|r| r.progress.lock().ok().map(|g| (g.stage.clone(), g.frac)));
        let spec = crate::popout::Spec::new(KEY, "Export wallpaper").size(460.0, 640.0);
        crate::popout::tool_window(ctx, spec, &mut open, |ui| {
            let s = &mut self.wallpaper.settings;
            egui::Grid::new("wp-grid").num_columns(2).spacing([8.0, 6.0]).show(ui, |ui| {
                ui.label("Size");
                let current = options
                    .iter()
                    .find(|o| o.size == s.size)
                    .map(|o| o.label.clone())
                    .unwrap_or_else(|| match &s.size {
                        WpSize::Custom { width, height } => format!("Custom ({width} × {height})"),
                        WpSize::Monitor { index } => format!("Monitor {index} (not connected)"),
                        _ => "choose…".into(),
                    });
                egui::ComboBox::from_id_salt("wp-size")
                    .selected_text(current)
                    .width(280.0)
                    .show_ui(ui, |ui| {
                        let mut group = "";
                        for o in &options {
                            let g = match o.size {
                                WpSize::Monitor { .. } => "Monitors",
                                WpSize::Span => "Span",
                                _ => "Presets",
                            };
                            if g != group {
                                group = g;
                                ui.label(RichText::new(g).small().weak());
                            }
                            if ui.selectable_label(s.size == o.size, &o.label).clicked() {
                                s.size = o.size.clone();
                            }
                        }
                        ui.label(RichText::new("Other").small().weak());
                        let custom = matches!(s.size, WpSize::Custom { .. });
                        if ui.selectable_label(custom, "Custom size").clicked() && !custom {
                            let (w, h) = resolved.as_ref().map(|r| (r.0, r.1)).unwrap_or((3840, 2160));
                            s.size = WpSize::Custom { width: w, height: h };
                        }
                    });
                ui.end_row();
                if let WpSize::Custom { width, height } = &mut s.size {
                    ui.label("");
                    ui.horizontal(|ui| {
                        ui.add(egui::DragValue::new(width).range(16..=30_000).suffix(" px"));
                        ui.label("×");
                        ui.add(egui::DragValue::new(height).range(16..=30_000).suffix(" px"));
                    });
                    ui.end_row();
                }
                if s.size == WpSize::Span {
                    ui.label("Layout");
                    ui.horizontal(|ui| {
                        let mut text = if s.span_layout.is_empty() {
                            layout_text(&detected_rects(&monitors))
                        } else {
                            s.span_layout.clone()
                        };
                        if ui
                            .add(egui::TextEdit::singleline(&mut text).desired_width(230.0))
                            .on_hover_text("Each monitor as WIDTHxHEIGHT+X+Y (as xrandr prints it), comma separated")
                            .changed()
                        {
                            s.span_layout = text;
                        }
                        if !s.span_layout.is_empty() && ui.small_button("Detected").clicked() {
                            s.span_layout.clear();
                        }
                    });
                    ui.end_row();
                    ui.label("Write");
                    ui.horizontal(|ui| {
                        ui.radio_value(&mut s.span_images, SpanImages::Both, "Both");
                        ui.radio_value(&mut s.span_images, SpanImages::Combined, "One span image");
                        ui.radio_value(&mut s.span_images, SpanImages::PerMonitor, "One per monitor");
                    });
                    ui.end_row();
                }
                ui.label("Fit");
                ui.horizontal(|ui| {
                    ui.radio_value(&mut s.fit, FitChoice::Cover, "Cover")
                        .on_hover_text("The grid fills the image, cropped; no stage colour shows");
                    ui.radio_value(&mut s.fit, FitChoice::Contain, "Contain")
                        .on_hover_text("The whole grid, letterboxed in the stage colour");
                    ui.add_enabled_ui(!s.fresh, |ui| {
                        ui.radio_value(&mut s.fit, FitChoice::View, "Current view")
                            .on_hover_text("What the grid view shows now, scaled to the image");
                    });
                    ui.add_enabled_ui(wraps, |ui| {
                        ui.radio_value(&mut s.fit, FitChoice::Repeat, "Repeat")
                            .on_hover_text("A wrap-edge grid tiled over the image, seamlessly");
                    });
                });
                ui.end_row();
                if s.fit == FitChoice::Repeat {
                    ui.label("");
                    ui.horizontal(|ui| {
                        ui.label("Tiles");
                        ui.add(egui::DragValue::new(&mut s.repeat_px).range(0.5..=256.0).speed(0.1).suffix(" px"));
                    });
                    ui.end_row();
                }
                ui.label("Grid");
                ui.vertical(|ui| {
                    ui.checkbox(&mut s.fresh, "Fresh grid sized for the image")
                        .on_hover_text("A new grid of this tiling with this world's CAs and seeds, run first. The live world is left alone.");
                    if s.fresh {
                        ui.horizontal(|ui| {
                            ui.add(egui::DragValue::new(&mut s.cell_px).range(1.0..=256.0).speed(0.1).suffix(" px per tile"));
                            ui.label("run");
                            ui.add(egui::DragValue::new(&mut s.gens).range(0..=1_000_000).speed(5.0).suffix(" gens"));
                        });
                        ui.horizontal(|ui| {
                            ui.add_enabled(gpu_ok, egui::Checkbox::new(&mut s.gpu, "On the GPU"))
                                .on_hover_text("Results are identical to the CPU");
                            if let Some(c) = fresh_cells_est {
                                let t = format!("about {} cells", crate::sweep_form::thousands(c));
                                if c > MAX_FRESH_CELLS {
                                    ui.colored_label(Color32::from_rgb(255, 120, 120), t + " (too many)");
                                } else {
                                    ui.label(RichText::new(t).weak());
                                }
                            }
                        });
                    }
                });
                ui.end_row();
                ui.label("Outlines");
                ui.horizontal(|ui| {
                    let mut on = s.outlines > 0.0;
                    if ui.checkbox(&mut on, "Tile outlines").changed() {
                        s.outlines = if on { 7.0 } else { 0.0 };
                    }
                    if on {
                        ui.label("when tiles are at least");
                        ui.add(egui::DragValue::new(&mut s.outlines).range(1.0..=256.0).suffix(" px"));
                    }
                });
                ui.end_row();
                ui.label("Palette");
                egui::ComboBox::from_id_salt("wp-palette")
                    .selected_text(s.palette.clone().unwrap_or_else(|| "as the CAs are".into()))
                    .show_ui(ui, |ui| {
                        if ui.selectable_label(s.palette.is_none(), "as the CAs are").clicked() {
                            s.palette = None;
                        }
                        for n in &names {
                            if ui.selectable_label(s.palette.as_deref() == Some(n), n).clicked() {
                                s.palette = Some(n.clone());
                            }
                        }
                    });
                ui.end_row();
                ui.label("Folder");
                ui.horizontal(|ui| {
                    ui.label(RichText::new(dir.display().to_string()).monospace().small());
                    if ui.small_button("Choose…").clicked()
                        && let Some(d) = rfd::FileDialog::new().set_directory(&dir).pick_folder()
                    {
                        s.folder = Some(d);
                    }
                    if s.folder.is_some() && ui.small_button("Default").clicked() {
                        s.folder = None;
                    }
                });
                ui.end_row();
            });
            ui.separator();
            // Preview, with the monitors' cuts of a span.
            match &resolved {
                Err(e) => {
                    ui.colored_label(Color32::from_rgb(255, 120, 120), e);
                }
                Ok((w, h, mons)) => {
                    ui.label(format!(
                        "{w} × {h} pixels{}",
                        if s.fresh {
                            " · preview shows the current world"
                        } else {
                            ""
                        }
                    ));
                    if let Some(t) = &self.wallpaper.preview {
                        let size = t.size_vec2();
                        let k = (ui.available_width() / size.x).min(1.0);
                        let r = ui.image((t.id(), size * k)).rect;
                        if let Some(m) = mons {
                            let sx = r.width() / *w as f32;
                            for (x, y, mw, mh) in span_cuts(m) {
                                let cut = egui::Rect::from_min_size(
                                    r.min + egui::vec2(x as f32 * sx, y as f32 * sx),
                                    egui::vec2(mw as f32 * sx, mh as f32 * sx),
                                );
                                ui.painter().rect_stroke(
                                    cut,
                                    0.0,
                                    egui::Stroke::new(1.0, Color32::from_rgb(255, 200, 80)),
                                    egui::StrokeKind::Inside,
                                );
                            }
                        }
                    } else if matches!(fit, Some(Fit::Repeat { .. })) && !wraps {
                        ui.label(RichText::new("Repeat needs a grid with wrap edges.").weak());
                    }
                }
            }
            ui.separator();
            ui.horizontal(|ui| {
                let busy = progress.is_some();
                if ui
                    .add_enabled(!busy && resolved.is_ok(), egui::Button::new("Export"))
                    .clicked()
                {
                    export = true;
                }
                if busy && ui.button("Cancel").clicked() {
                    cancel = true;
                }
                if ui.button("Open folder").clicked() {
                    let _ = std::fs::create_dir_all(&dir);
                    open_folder(&dir);
                }
            });
            if let Some((stage, frac)) = &progress {
                ui.add(
                    egui::ProgressBar::new(*frac)
                        .text(stage.as_str())
                        .desired_width(ui.available_width()),
                );
            }
            match &self.wallpaper.last {
                Some(Ok((files, secs))) => {
                    ui.label(format!("Wrote in {secs:.1} s:"));
                    for (p, w, h) in files {
                        ui.label(
                            RichText::new(format!("{} ({w} × {h})", p.display()))
                                .monospace()
                                .small(),
                        );
                    }
                }
                Some(Err(e)) => {
                    ui.colored_label(Color32::from_rgb(255, 120, 120), e);
                }
                None => {}
            }
            if self.wallpaper.fresh.is_some()
                && ui
                    .button("Open the fresh grid")
                    .on_hover_text("Replace the live world with the last fresh grid")
                    .clicked()
            {
                open_fresh = true;
            }
        });
        self.wallpaper.open = open;
        if export {
            self.start_wallpaper_export();
        }
        if cancel && let Some(r) = &self.wallpaper.running {
            r.cancel.store(true, Ordering::Relaxed);
        }
        if open_fresh && let Some((w, sc)) = self.wallpaper.fresh.take() {
            self.show_world_bg(*w, *sc);
        }
    }
}

/// Tiles in one lattice cell of a family (measured once on a small patch).
fn tiles_per_cell(spec: &TilingSpec) -> u64 {
    let family = spec.family.as_str();
    static CACHE: std::sync::OnceLock<Mutex<std::collections::HashMap<String, u64>>> =
        std::sync::OnceLock::new();
    let cache = CACHE.get_or_init(Default::default);
    if let Some(n) = cache.lock().ok().and_then(|c| c.get(family).copied()) {
        return n;
    }
    let probe = TilingSpec {
        size: Some(SizeSpec::Cells(100)),
        boundary: Boundary::Dead,
        ..spec.clone()
    };
    let n = Topology::build(&probe)
        .ok()
        .and_then(|t| t.lattice().map(|g| g.ntypes as u64))
        .unwrap_or(1)
        .max(1);
    if let Ok(mut c) = cache.lock() {
        c.insert(family.to_string(), n);
    }
    n
}

/// Roughly how many cells a tiling spec builds.
fn fresh_cells(spec: &TilingSpec) -> u64 {
    match spec.size {
        Some(SizeSpec::Rect { w, h }) => w as u64 * h as u64 * tiles_per_cell(spec),
        Some(SizeSpec::Cells(c)) => c as u64,
        Some(SizeSpec::Level(l)) => aperiodic::tile_count(&spec.family, l).unwrap_or(0) as u64,
        None => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mon(
        index: usize,
        name: &str,
        size: [u32; 2],
        pos: [i32; 2],
        same_as: Option<usize>,
    ) -> MonitorInfo {
        MonitorInfo {
            index,
            name: Some(name.into()),
            size_px: size,
            position_px: pos,
            scale_factor: 1.0,
            refresh_millihertz: None,
            current: false,
            same_as,
        }
    }

    #[test]
    fn sizes_from_monitors_skip_duplicates() {
        // This machine: one 2560 x 1440 monitor listed twice.
        let ms = vec![
            mon(0, "DP-1", [2560, 1440], [0, 0], None),
            mon(1, "DP-1", [2560, 1440], [0, 0], Some(0)),
        ];
        let o = size_options(&ms);
        let monitors: Vec<_> = o
            .iter()
            .filter(|o| matches!(o.size, WpSize::Monitor { .. }))
            .collect();
        assert_eq!(monitors.len(), 1);
        assert_eq!((monitors[0].width, monitors[0].height), (2560, 1440));
        assert!(
            !o.iter().any(|o| o.size == WpSize::Span),
            "one monitor: no span"
        );
        assert_eq!(o.len(), 1 + wallpaper::PRESETS.len());
        // Two real monitors: both, and the span.
        let ms = vec![
            mon(0, "DP-1", [2560, 1440], [0, 0], None),
            mon(1, "HDMI-1", [1920, 1080], [2560, 180], None),
            mon(2, "HDMI-1", [1920, 1080], [2560, 180], Some(1)),
        ];
        let o = size_options(&ms);
        let span = o.iter().find(|o| o.size == WpSize::Span).unwrap();
        assert_eq!((span.width, span.height), (4480, 1440));
        let s = WallpaperSettings {
            size: WpSize::Monitor { index: 1 },
            ..Default::default()
        };
        assert_eq!(resolve_size(&s, &ms).unwrap().0, 1920);
        let s = WallpaperSettings {
            size: WpSize::Monitor { index: 2 },
            ..Default::default()
        };
        assert!(resolve_size(&s, &ms).is_err(), "a duplicate is not offered");
        let s = WallpaperSettings {
            size: WpSize::Span,
            ..Default::default()
        };
        let (w, h, m) = resolve_size(&s, &ms).unwrap();
        assert_eq!((w, h, m.unwrap().len()), (4480, 1440, 2));
        let s = WallpaperSettings {
            size: WpSize::Span,
            span_layout: "1920x1080-1920+0,2560x1440+0-200".into(),
            ..Default::default()
        };
        assert_eq!(resolve_size(&s, &ms).unwrap().0, 4480);
        let s = WallpaperSettings {
            size: WpSize::Preset { name: "4K".into() },
            ..Default::default()
        };
        assert_eq!(resolve_size(&s, &[]).unwrap(), (3840, 2160, None));
    }

    #[test]
    fn overlapping_positions_are_laid_side_by_side() {
        // Wayland may report every monitor at (0, 0).
        let ms = vec![
            mon(0, "A", [2560, 1440], [0, 0], None),
            mon(1, "B", [1920, 1080], [0, 0], None),
        ];
        let r = detected_rects(&ms);
        assert_eq!((r[0].x, r[1].x, r[1].y), (0, 2560, 0));
        assert_eq!(layout_text(&r), "2560x1440+0+0,1920x1080+2560+0");
        assert_eq!(wallpaper::parse_monitors(&layout_text(&r)).unwrap(), r);
        // Real positions are kept, negative ones too.
        let ms = vec![
            mon(0, "A", [2560, 1440], [0, 0], None),
            mon(1, "B", [1920, 1080], [-1920, 300], None),
        ];
        let r = detected_rects(&ms);
        assert_eq!((r[1].x, r[1].y), (-1920, 300));
        assert_eq!(layout_text(&r), "2560x1440+0+0,1920x1080-1920+300");
    }

    #[test]
    fn span_cuts_line_up_with_split() {
        let rects =
            wallpaper::parse_monitors("1920x1080-1920+300,2560x1440+0+0,1080x1920+2560-200")
                .unwrap();
        let (_, _, w, h) = wallpaper::span_box(&rects);
        assert_eq!((w, h), (1920 + 2560 + 1080, 1920));
        let cuts = span_cuts(&rects);
        assert_eq!(cuts[0], (0, 500, 1920, 1080));
        assert_eq!(cuts[1], (1920, 200, 2560, 1440));
        assert_eq!(cuts[2], (4480, 0, 1080, 1920));
        // A span image whose pixels encode their position: each cut is the same piece.
        let img = render::Image {
            width: w,
            height: h,
            rgba: (0..w * h)
                .flat_map(|k| {
                    let (x, y) = (k % w, k / w);
                    [
                        (x & 255) as u8,
                        (y & 255) as u8,
                        ((x >> 8) ^ (y >> 8)) as u8,
                        255,
                    ]
                })
                .collect(),
        };
        for (part, (x, y, cw, ch)) in wallpaper::split(&img, &rects).iter().zip(cuts) {
            assert_eq!((part.width, part.height), (cw, ch));
            for (px, py) in [(0, 0), (cw - 1, ch - 1), (cw / 2, ch / 3)] {
                let a = ((py * cw + px) * 4) as usize;
                let b = (((y + py) * w + x + px) * 4) as usize;
                assert_eq!(part.rgba[a..a + 4], img.rgba[b..b + 4]);
            }
        }
    }

    #[test]
    fn current_view_maps_to_the_output() {
        // A 1000 x 600 point view at 8 points per unit, exported at 2000 x 1200: twice the scale.
        let f = view_fit([10.0, -4.0], 8.0, [1000.0, 600.0], 2000, 1200);
        assert_eq!(
            f,
            Fit::View {
                center: [10.0, -4.0],
                scale: 16.0
            }
        );
        // Another shape: the whole view still fits (the limiting side decides).
        let Fit::View { scale, .. } = view_fit([0.0, 0.0], 8.0, [1000.0, 600.0], 2560, 1440) else {
            panic!()
        };
        assert!((scale - 8.0 * 2.4).abs() < 1e-9);
        assert!(1000.0 * 8.0 / 8.0 * (scale / 8.0) <= 2560.0 + 1e-6);
        // The preview is the same view at its own size.
        assert_eq!(
            scaled_fit(&f, 0.25),
            Fit::View {
                center: [10.0, -4.0],
                scale: 4.0
            }
        );
        let s = WallpaperSettings {
            fit: FitChoice::View,
            fresh: true,
            ..Default::default()
        };
        assert_eq!(
            fit_for(&s, [0.0, 0.0], 1.0, [10.0, 10.0], 100, 100),
            Fit::Cover
        );
    }

    fn life_world() -> World {
        let mut w = World::new(TilingSpec::rect("square", 160, 90, Boundary::Wrap), 3).unwrap();
        let mut ca = CaSpec::new("B3/S23");
        ca.seed = Some(SeedSpec::Random { density: 0.35 });
        ca.palette = Some("Paper".into());
        w.apply(Command::AddCa { ca }).unwrap();
        w.apply(Command::Reset).unwrap();
        w.apply(Command::Step { gens: 60 }).unwrap();
        w
    }

    fn scratch(name: &str) -> PathBuf {
        let base = std::env::var_os("JETCARP_TEST_OUT")
            .map(PathBuf::from)
            .unwrap_or_else(std::env::temp_dir);
        let d = base.join(format!("jetcarp-wallpaper-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    fn png_info(p: &std::path::Path) -> (u32, u32, Vec<String>) {
        let dec = png::Decoder::new(std::io::BufReader::new(std::fs::File::open(p).unwrap()));
        let r = dec.read_info().unwrap();
        let info = r.info();
        let keys = info.utf8_text.iter().map(|t| t.keyword.clone()).collect();
        (info.width, info.height, keys)
    }

    /// The dialog's export path at 2560 x 1440: a copy of the live world and a fresh grid.
    #[test]
    fn exports_a_qhd_wallpaper() {
        let w = life_world();
        let hash = w.hash_hex();
        let dir = scratch("qhd");
        let job = Job {
            source: Source::Snapshot {
                world: Box::new(snapshot(&w)),
                meta: render::png_metadata(&w),
            },
            width: 2560,
            height: 1440,
            monitors: None,
            images: SpanImages::Both,
            fit: Fit::Cover,
            outlines: 0.0,
            palette: None,
            dir: dir.clone(),
            name: "Life test".into(),
        };
        let p: Shared = Default::default();
        let done = export(job, &p, &AtomicBool::new(false)).unwrap();
        assert_eq!(done.files.len(), 1);
        let (path, fw, fh) = &done.files[0];
        assert_eq!((*fw, *fh), (2560, 1440));
        let (pw, ph, keys) = png_info(path);
        assert_eq!((pw, ph), (2560, 1440));
        for k in ["jetcarp:version", "jetcarp:hash", "jetcarp:scene", "jetcarp:wallpaper"] {
            assert!(keys.iter().any(|x| x == k), "{k} missing: {keys:?}");
        }
        assert!(
            path.file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("Life-test-2560x1440-")
        );
        // The live world was not touched.
        assert_eq!(w.hash_hex(), hash);
        // A fresh grid, split over two monitors with a palette override.
        let rects = wallpaper::parse_monitors("1280x720+0+0,1280x720+1280+0").unwrap();
        let job = Job {
            source: Source::Fresh {
                scene: Box::new(w.to_scene()),
                cell_px: 8.0,
                gens: 20,
                gpu: false,
            },
            width: 2560,
            height: 720,
            monitors: Some(rects),
            images: SpanImages::Both,
            fit: Fit::Cover,
            outlines: 0.0,
            palette: Some("Aurora".into()),
            dir: dir.clone(),
            name: String::new(),
        };
        let done = export(job, &p, &AtomicBool::new(false)).unwrap();
        assert_eq!(done.files.len(), 3);
        let sizes: Vec<(u32, u32)> = done.files.iter().map(|f| (f.1, f.2)).collect();
        assert_eq!(sizes, vec![(2560, 720), (1280, 720), (1280, 720)]);
        let (fresh, _) = done.fresh.unwrap();
        assert_eq!(fresh.generation, 20);
        assert_eq!(fresh.cas[0].palette, "Aurora");
        // 2560 / 8 by 720 / 8 tiles.
        assert_eq!(fresh.topo.len(), 320 * 90);
        let (_, _, keys) = png_info(&done.files[1].0);
        assert!(keys.iter().any(|k| k == "jetcarp:monitor"));
        if std::env::var_os("JETCARP_TEST_OUT").is_none() {
            let _ = std::fs::remove_dir_all(&dir);
        }
    }

    #[test]
    fn settings_round_trip_with_defaults() {
        let s = WallpaperSettings {
            size: WpSize::Custom {
                width: 1234,
                height: 567,
            },
            fit: FitChoice::View,
            palette: Some("Paper".into()),
            folder: Some("/x/y".into()),
            ..Default::default()
        };
        let json = serde_json::to_string(&s).unwrap();
        assert_eq!(serde_json::from_str::<WallpaperSettings>(&json).unwrap(), s);
        let d: WallpaperSettings = serde_json::from_str(r#"{"fit": "contain"}"#).unwrap();
        assert_eq!(d.fit, FitChoice::Contain);
        assert_eq!(d.size, WpSize::Monitor { index: 0 });
    }
}
