//! Shows the host's screens on OS windows, and runs the tray where there is one. Each screen is a
//! normal window, or borderless full screen when its settings name a monitor. On GNOME a wallpaper
//! is a full screen window that the extension shows as the background; without the extension it
//! falls back to a plain window, so nothing is left covering the desktop. On Windows a wallpaper
//! is a window of our own under the desktop icons, one per monitor. On macOS it is a borderless
//! window at desktop level, one per monitor. Closing a plain window quits the host.

use crate::core::{Core, SharedGpu, Shell, device_descriptor};
use crate::targets::{self, Target};
use jetcarp_display::host_settings::Place;
use jetcarp_display::painter::{self, Painter};
use std::collections::BTreeSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use winit::application::ApplicationHandler;
use winit::dpi::{PhysicalPosition, PhysicalSize};
use winit::event::{ElementState, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::window::{Fullscreen, Window, WindowId, WindowLevel};

const WINDOW_SIZE: PhysicalSize<u32> = PhysicalSize::new(1280, 720);
/// How often to look for monitors coming and going.
const MONITORS_EVERY: f64 = 2.0;
/// A screen saver ignores input for this long, so the key or nudge that started it doesn't end it.
const SAVER_GRACE: f64 = 0.7;
/// How far the pointer moves, in pixels, before a screen saver ends.
const SAVER_MOVE: f64 = 12.0;
/// A wallpaper that gets no frame to draw on for this long is reported.
const STARVED_AFTER: f64 = 10.0;
const STARVED: &str = "the desktop has not taken a frame for 10 seconds; another live wallpaper program may be holding it";
const NO_WALLPAPER: &str = if cfg!(all(unix, not(target_os = "macos"))) {
    "wallpaper needs JETCARP's GNOME extension; run `jetcarp host gnome install`, then log out and back in. Showing a window for now."
} else {
    "wallpaper is not available on this system yet. Showing a window for now."
};
/// How long to keep trying to get under the desktop icons (Explorer may be restarting).
#[cfg(windows)]
const DESKTOP_PATIENCE: f64 = 15.0;
/// Distinct GPU errors noted, after which they are only counted.
const GPU_ERRORS_NOTED: usize = 20;

/// What the GPU's callbacks report, from whatever thread they run on.
#[derive(Default)]
struct GpuEvents {
    errors: Mutex<Vec<String>>,
    lost: AtomicBool,
}
#[cfg(windows)]
const DESKTOP_RETRY: f64 = 2.0;

/// A monitor's name, place and size.
/// A monitor's name, position, size in pixels and scale. The scale is part of it because a change
/// of display scaling alone moves nothing, yet Explorer then puts its desktop back over the
/// wallpaper, and cells are sized from the scale. Seen on the Acer going from 125% to 150%.
type MonitorShape = (String, (i32, i32), (u32, u32), f64);

/// What the program is doing.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mode {
    Host,
    /// Full screen on every monitor until there is input.
    Saver,
    /// Inside the Screen Saver dialog's preview, the window given.
    #[cfg(windows)]
    Preview(isize),
}

/// What is drawn on.
enum Body {
    Os(Arc<Window>),
    #[cfg(windows)]
    Desktop(crate::desktop_win::DesktopWindow),
}

impl Body {
    /// The OS window, if this is one.
    fn os(&self) -> Option<&Arc<Window>> {
        match self {
            Body::Os(w) => Some(w),
            #[cfg(windows)]
            Body::Desktop(_) => None,
        }
    }

    fn id(&self) -> Option<WindowId> {
        match self {
            Body::Os(w) => Some(w.id()),
            #[cfg(windows)]
            Body::Desktop(_) => None,
        }
    }

    fn size(&self) -> (u32, u32) {
        match self {
            Body::Os(w) => {
                let s = w.inner_size();
                (s.width, s.height)
            }
            #[cfg(windows)]
            Body::Desktop(d) => d.size,
        }
    }

    fn scale(&self) -> f64 {
        match self {
            Body::Os(w) => w.scale_factor(),
            #[cfg(windows)]
            Body::Desktop(d) => d.scale,
        }
    }

    fn alive(&self) -> bool {
        match self {
            Body::Os(_) => true,
            #[cfg(windows)]
            Body::Desktop(d) => d.alive(),
        }
    }
}

// The surface comes before the body so it is released before the window goes.
struct Output {
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    painter: Painter,
    /// Since when no frame could be had (a timeout, not a covered window).
    starved_since: Option<f64>,
    target: Target,
    body: Body,
    /// The title last set.
    title: String,
}

struct App {
    core: Core,
    mode: Mode,
    instance: wgpu::Instance,
    gpu: Option<SharedGpu>,
    /// Errors and loss reported by the current device. A GPU error is noted rather than ending
    /// the host, which wgpu does by default.
    gpu_events: Arc<GpuEvents>,
    /// First lines of the GPU errors noted so far, and how many more were not.
    gpu_errors_seen: BTreeSet<String>,
    gpu_errors_unnoted: u64,
    outputs: Vec<Output>,
    next_frame: Instant,
    ends_at: Option<Instant>,
    error: Option<String>,
    monitors_at: f64,
    /// Each monitor, to notice changes.
    geometry: Vec<MonitorShape>,
    /// Some surfaces could not be made yet; try again at `retry_at`.
    sync_pending: bool,
    retry_at: f64,
    /// Getting under the desktop icons kept failing, so wallpapers are plain windows.
    desktop_gave_up: bool,
    #[cfg(windows)]
    desktop_failing_since: Option<f64>,
    #[cfg(windows)]
    had_desktop: bool,
    /// A wallpaper was shown in a plain window and the reason has been noted.
    noted_fallback: bool,
    noted_starved: bool,
    /// `JETCARP_HOST_TEST_LOSE_GPU` has lost the device once.
    lost_on_purpose: bool,
    /// Where the pointer was when a screen saver first saw it.
    pointer_from: Option<PhysicalPosition<f64>>,
    #[cfg(any(windows, target_os = "macos"))]
    tray: Option<crate::tray::Tray>,
    #[cfg(any(windows, target_os = "macos"))]
    tray_refreshed: f64,
}

impl App {
    /// Whether screen `i` should be a wallpaper right now.
    fn shown_as_wallpaper(&self, i: usize) -> bool {
        self.core.screens[i].place == Place::Wallpaper
            && self.core.shell() == Shell::Alive
            && !self.desktop_gave_up
    }

    fn title(&self, t: Target) -> String {
        if t.wallpaper {
            self.core.wallpaper_title(t.screen)
        } else {
            format!("JETCARP: {}", self.core.screens[t.screen].stage.rotation())
        }
    }

    /// The surfaces wanted right now.
    fn targets(&self, el: &ActiveEventLoop) -> Vec<Target> {
        let count = el.available_monitors().count();
        match self.mode {
            Mode::Saver => (0..count.max(1))
                .map(|m| Target {
                    screen: 0,
                    wallpaper: false,
                    monitor: Some(m),
                })
                .collect(),
            #[cfg(windows)]
            Mode::Preview(_) => vec![Target {
                screen: 0,
                wallpaper: true,
                monitor: None,
            }],
            Mode::Host => {
                let screens: Vec<_> = (0..self.core.screens.len())
                    .map(|i| (self.core.screens[i].monitor, self.shown_as_wallpaper(i)))
                    .collect();
                if cfg!(any(windows, target_os = "macos")) {
                    let primary = el.primary_monitor();
                    let primary = el
                        .available_monitors()
                        .position(|m| Some(&m) == primary.as_ref())
                        .unwrap_or(0);
                    targets::per_monitor(&screens, count, primary)
                } else {
                    targets::one_each(&screens)
                }
            }
        }
    }

    /// Where a surface goes full screen, if it does.
    fn fullscreen(&self, el: &ActiveEventLoop, t: Target) -> Option<Fullscreen> {
        // A macOS full screen window gets a Space of its own; a wallpaper is placed by hand.
        if cfg!(target_os = "macos") && t.wallpaper && self.mode == Mode::Host {
            return None;
        }
        match t.monitor {
            Some(m) => match el.available_monitors().nth(m) {
                Some(handle) => Some(Fullscreen::Borderless(Some(handle))),
                None if t.wallpaper => Some(Fullscreen::Borderless(None)),
                None => {
                    say!("jetcarp-host: there is no monitor {m}; using a window");
                    None
                }
            },
            None if t.wallpaper => Some(Fullscreen::Borderless(None)),
            None => None,
        }
    }

    fn make_window(&self, el: &ActiveEventLoop, t: Target) -> Result<Arc<Window>, String> {
        let mut attrs = Window::default_attributes()
            .with_title(self.title(t))
            .with_inner_size(WINDOW_SIZE)
            .with_fullscreen(self.fullscreen(el, t));
        if self.mode == Mode::Saver {
            attrs = attrs.with_window_level(WindowLevel::AlwaysOnTop);
        }
        #[cfg(all(unix, not(target_os = "macos")))]
        {
            use winit::platform::wayland::WindowAttributesExtWayland;
            attrs = attrs.with_name("jetcarp-host", "jetcarp-host");
        }
        #[cfg(target_os = "macos")]
        let on_desktop = t.wallpaper && self.mode == Mode::Host;
        #[cfg(target_os = "macos")]
        if on_desktop {
            use winit::platform::macos::WindowAttributesExtMacOS;
            let m = t
                .monitor
                .and_then(|m| el.available_monitors().nth(m))
                .or_else(|| el.primary_monitor())
                .ok_or("no monitor")?;
            // In points, from the monitor's own scale: winit turns physical values into points
            // with the main screen's scale, which puts a window for a 1x monitor beside a 2x one
            // at half its offset and half its size, on the wrong screen.
            let scale = m.scale_factor();
            attrs = attrs
                .with_decorations(false)
                .with_resizable(false)
                .with_active(false)
                .with_has_shadow(false)
                .with_position(m.position().to_logical::<f64>(scale))
                .with_inner_size(m.size().to_logical::<f64>(scale));
        }
        let w = el.create_window(attrs).map_err(|e| e.to_string())?;
        #[cfg(target_os = "macos")]
        if on_desktop {
            crate::desktop_mac::place_on_desktop(&w)?;
        }
        if self.mode == Mode::Saver {
            w.set_cursor_visible(false);
        }
        Ok(Arc::new(w))
    }

    /// The window and surface for `t`.
    fn make_body(
        &mut self,
        el: &ActiveEventLoop,
        t: Target,
    ) -> Result<(Body, wgpu::Surface<'static>), String> {
        #[cfg(windows)]
        {
            use crate::desktop_win::{DesktopWindow, Rect};
            let made = match self.mode {
                Mode::Preview(parent) => Some(DesktopWindow::in_preview(parent)?),
                Mode::Host if t.wallpaper => {
                    let m = t
                        .monitor
                        .and_then(|m| el.available_monitors().nth(m))
                        .or_else(|| el.primary_monitor())
                        .ok_or("no monitor")?;
                    let (p, s) = (m.position(), m.size());
                    let rect = Rect {
                        x: p.x,
                        y: p.y,
                        w: s.width,
                        h: s.height,
                    };
                    Some(DesktopWindow::under_icons(rect, m.scale_factor())?)
                }
                _ => None,
            };
            if let Some(d) = made {
                self.core.desktop_layout = (self.mode == Mode::Host).then_some(d.layout);
                let surface = d.surface(&self.instance)?;
                return Ok((Body::Desktop(d), surface));
            }
        }
        let window = self.make_window(el, t)?;
        let surface = self
            .instance
            .create_surface(window.clone())
            .map_err(|e| format!("no surface: {e}"))?;
        Ok((Body::Os(window), surface))
    }

    fn gpu_for(&mut self, surface: &wgpu::Surface<'static>) -> Result<SharedGpu, String> {
        if let Some(g) = &self.gpu {
            return Ok(g.clone());
        }
        let adapter =
            pollster::block_on(self.instance.request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: Some(surface),
                ..Default::default()
            }))
            .map_err(|e| format!("no graphics adapter: {e}"))?;
        let (device, queue) =
            pollster::block_on(adapter.request_device(&device_descriptor(&adapter)))
                .map_err(|e| format!("no graphics device: {e}"))?;
        let events = Arc::new(GpuEvents::default());
        let sink = events.clone();
        device.on_uncaptured_error(Arc::new(move |e| {
            if let Ok(mut v) = sink.errors.lock() {
                v.push(e.to_string());
            }
        }));
        let sink = events.clone();
        device.set_device_lost_callback(move |reason, why| {
            sink.lost.store(true, Ordering::SeqCst);
            if let Ok(mut v) = sink.errors.lock() {
                v.push(format!("device lost ({reason:?}) {why}"));
            }
        });
        self.gpu_events = events;
        let g = SharedGpu {
            adapter,
            device,
            queue,
        };
        self.gpu = Some(g.clone());
        self.core.attach_gpu(Some(g.clone()));
        Ok(g)
    }

    fn make_output(&mut self, el: &ActiveEventLoop, target: Target) -> Result<Output, String> {
        let title = self.title(target);
        let (body, surface) = self.make_body(el, target)?;
        let gpu = self.gpu_for(&surface)?;
        let caps = surface.get_capabilities(&gpu.adapter);
        let config = painter::surface_config(&caps, body.size())?;
        surface.configure(&gpu.device, &config);
        Ok(Output {
            painter: Painter::new(&gpu.device, config.format),
            starved_since: None,
            body,
            surface,
            config,
            target,
            title,
        })
    }

    /// Couldn't get under the icons. Explorer may be restarting, so keep trying for a while before
    /// settling for a window.
    #[cfg(windows)]
    fn desktop_failed(&mut self, why: &str) {
        let now = self.core.now();
        let since = *self.desktop_failing_since.get_or_insert(now);
        self.sync_pending = true;
        self.retry_at = now + DESKTOP_RETRY;
        if now - since >= DESKTOP_PATIENCE {
            self.desktop_gave_up = true;
            self.desktop_failing_since = None;
            self.retry_at = 0.0;
            self.core.note(format!(
                "could not get under the desktop icons: {why}. Showing a window for now."
            ));
        }
    }

    /// Makes the surfaces match what is wanted, keeping those that still fit.
    fn sync_outputs(&mut self, el: &ActiveEventLoop) {
        self.core.screens_changed = false;
        self.sync_pending = false;
        // A wallpaper waits until it's known whether GNOME's extension can show it, rather than
        // flashing up as a window at login.
        if self.core.wants_wallpaper() && self.core.shell() == Shell::Waiting {
            self.sync_pending = true;
            return;
        }
        let wanted = self.targets(el);
        let mut old: Vec<Option<Output>> = std::mem::take(&mut self.outputs)
            .into_iter()
            .map(Some)
            .collect();
        let mut fresh: Vec<Output> = Vec::with_capacity(wanted.len());
        for t in wanted {
            let kept = old
                .iter_mut()
                .find(|o| o.as_ref().is_some_and(|o| o.target == t))
                .and_then(Option::take);
            if let Some(o) = kept {
                fresh.push(o);
                continue;
            }
            match self.make_output(el, t) {
                Ok(o) => {
                    #[cfg(windows)]
                    if t.wallpaper {
                        self.desktop_failing_since = None;
                    }
                    fresh.push(o);
                }
                #[cfg(windows)]
                Err(e) if t.wallpaper && self.mode == Mode::Host => self.desktop_failed(&e),
                Err(e) => {
                    self.error = Some(e);
                    el.exit();
                    return;
                }
            }
        }
        drop(old);
        self.outputs = fresh;
        self.fit_screens();
        #[cfg(target_os = "macos")]
        if self.mode == Mode::Host {
            crate::desktop_mac::set_dock_icon(self.outputs.iter().any(|o| !o.target.wallpaper));
        }
        #[cfg(windows)]
        {
            let has = self
                .outputs
                .iter()
                .any(|o| matches!(o.body, Body::Desktop(_)));
            if self.had_desktop && !has && self.mode == Mode::Host {
                crate::desktop_win::restore_desktop();
            }
            self.had_desktop = has;
        }
        for i in 0..self.outputs.len() {
            let title = self.title(self.outputs[i].target);
            let o = &mut self.outputs[i];
            if o.title != title {
                if let Some(w) = o.body.os() {
                    w.set_title(&title);
                }
                o.title = title;
            }
        }
        let showing = !self.core.wants_wallpaper() || self.core.shell() != Shell::Gone;
        if showing {
            if self.noted_fallback {
                self.noted_fallback = false;
                self.core.retract(NO_WALLPAPER);
            }
        } else if !self.noted_fallback {
            self.noted_fallback = true;
            self.core.note(NO_WALLPAPER);
        }
    }

    /// Builds each screen's world for the largest of its surfaces, so every one is drawn at the
    /// same pixels per cell with no border, and a smaller monitor shows the middle. Cells are
    /// sized for the sharpest of them: on a 2x laptop beside a 1x monitor, 2 pixels on both.
    fn fit_screens(&mut self) {
        for screen in 0..self.core.screens.len() {
            let mine = || {
                self.outputs
                    .iter()
                    .filter(move |o| o.target.screen == screen)
            };
            let sizes = mine().map(|o| (o.config.width, o.config.height));
            let scale = mine().map(|o| o.body.scale()).fold(1.0, f64::max);
            if let Some(c) = painter::shared_canvas(sizes) {
                self.core.resize_screen(screen, c, scale);
            }
        }
    }

    /// Notes a wallpaper that has gone without frames for a while, and takes the note back once
    /// frames come again.
    fn watch_starving(&mut self) {
        let now = self.core.now();
        let starved = !self.core.held()
            && self.outputs.iter().any(|o| {
                o.target.wallpaper && o.starved_since.is_some_and(|t| now - t >= STARVED_AFTER)
            });
        if starved && !self.noted_starved {
            self.noted_starved = true;
            self.core.note(STARVED);
        } else if !starved && self.noted_starved {
            self.noted_starved = false;
            self.core.retract(STARVED);
        }
    }

    /// Records the monitors, and puts surfaces back on theirs when the list changes.
    fn watch_monitors(&mut self, el: &ActiveEventLoop) {
        let now = self.core.now();
        if now - self.monitors_at < MONITORS_EVERY {
            return;
        }
        self.monitors_at = now;
        #[cfg(windows)]
        {
            use crate::desktop_win as d;
            for o in &self.outputs {
                if let Body::Desktop(w) = &o.body {
                    w.keep_under_icons();
                }
            }
            let rects: Vec<_> = self
                .outputs
                .iter()
                .filter_map(|o| match &o.body {
                    Body::Desktop(w) => w.rect,
                    Body::Os(_) => None,
                })
                .collect();
            let showing = !rects.is_empty();
            self.core
                .set_local_hold("covered", showing && d::covered(&rects));
            self.core.set_local_hold("dark", showing && d::locked());
        }
        #[cfg(target_os = "macos")]
        {
            use crate::desktop_mac as d;
            let walls: Vec<_> = self
                .outputs
                .iter()
                .filter(|o| o.target.wallpaper)
                .filter_map(|o| o.body.os())
                .collect();
            let showing = !walls.is_empty();
            let covered = showing && walls.iter().all(|w| !d::visible(w));
            self.core.set_local_hold("covered", covered);
            self.core.set_local_hold("dark", showing && d::locked());
        }
        self.watch_starving();
        let geometry: Vec<_> = el
            .available_monitors()
            .map(|m| {
                let (p, s) = (m.position(), m.size());
                (
                    m.name().unwrap_or_else(|| "unnamed".into()),
                    (p.x, p.y),
                    (s.width, s.height),
                    m.scale_factor(),
                )
            })
            .collect();
        if geometry == self.geometry {
            return;
        }
        let names: Vec<String> = geometry.iter().map(|g| g.0.clone()).collect();
        let renamed = names != self.core.monitors;
        self.geometry = geometry;
        self.core.monitors = names;
        // Windows under the icons don't follow a monitor by themselves: make them again.
        #[cfg(windows)]
        {
            self.outputs.retain(|o| !matches!(o.body, Body::Desktop(_)));
            self.sync_pending = true;
        }
        // Nor do macOS's desktop-level ones, which were placed by hand.
        #[cfg(target_os = "macos")]
        {
            self.outputs.retain(|o| !o.target.wallpaper);
            self.sync_pending = true;
        }
        if renamed {
            for i in 0..self.outputs.len() {
                let t = self.outputs[i].target;
                if t.wallpaper || t.monitor.is_some() {
                    let place = self.fullscreen(el, t);
                    if let Some(w) = self.outputs[i].body.os() {
                        w.set_fullscreen(place);
                    }
                }
            }
        }
    }

    /// Whether this event ends a screen saver.
    fn ends_saver(&mut self, event: &WindowEvent) -> bool {
        if self.mode != Mode::Saver || self.core.now() < SAVER_GRACE {
            return false;
        }
        match event {
            WindowEvent::KeyboardInput { event, .. } => event.state == ElementState::Pressed,
            WindowEvent::MouseInput { state, .. } => *state == ElementState::Pressed,
            WindowEvent::MouseWheel { .. } => true,
            WindowEvent::CursorMoved { position, .. } => {
                let from = *self.pointer_from.get_or_insert(*position);
                (position.x - from.x).hypot(position.y - from.y) > SAVER_MOVE
            }
            _ => false,
        }
    }

    fn draw(&mut self, i: usize) {
        let now = self.core.now();
        let Some(o) = self.outputs.get_mut(i) else {
            return;
        };
        let (Some(gpu), Some(screen)) = (&self.gpu, self.core.screens.get(o.target.screen)) else {
            return;
        };
        let frame = match o.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(t)
            | wgpu::CurrentSurfaceTexture::Suboptimal(t) => t,
            wgpu::CurrentSurfaceTexture::Outdated => {
                o.surface.configure(&gpu.device, &o.config);
                return;
            }
            wgpu::CurrentSurfaceTexture::Timeout => {
                o.starved_since.get_or_insert(now);
                return;
            }
            wgpu::CurrentSurfaceTexture::Occluded => return,
            other => {
                say!("jetcarp-host: surface failed: {other:?}");
                o.surface.configure(&gpu.device, &o.config);
                return;
            }
        };
        o.starved_since = None;
        let view = frame.texture.create_view(&Default::default());
        o.painter.paint(
            &gpu.device,
            &gpu.queue,
            &view,
            [o.config.width, o.config.height],
            o.body.scale() as f32,
            &screen.stage,
            &self.core.settings.caption,
            now,
        );
        // A wallpaper's window is minimized and gets few frame callbacks, so it must not wait for
        // them: it presents on the host's own timer, from `about_to_wait`.
        if let (Some(w), false) = (o.body.os(), o.target.wallpaper) {
            w.pre_present_notify();
        }
        gpu.queue.present(frame);
    }

    /// Notes GPU errors, and starts again on a new device when the old one was lost.
    fn watch_gpu(&mut self) {
        // wgpu reports a lost device from its maintenance, which runs only when the device is
        // polled. Nothing else here polls on macOS, so without this a destroyed or reset device
        // was never reported and every frame failed with `Validation`.
        if let Some(g) = &self.gpu {
            let _ = g.device.poll(wgpu::PollType::Poll);
        }
        let errors = self
            .gpu_events
            .errors
            .lock()
            .map(|mut v| std::mem::take(&mut *v))
            .unwrap_or_default();
        for e in errors {
            let first = e
                .lines()
                .map(str::trim)
                .find(|l| !l.is_empty())
                .unwrap_or("");
            let key = first.to_string();
            if self.gpu_errors_seen.contains(&key) {
                continue;
            }
            if self.gpu_errors_seen.len() >= GPU_ERRORS_NOTED {
                if self.gpu_errors_unnoted == 0 {
                    self.core
                        .note("more kinds of GPU error than can be listed; the rest are not");
                }
                self.gpu_errors_unnoted += 1;
                continue;
            }
            self.gpu_errors_seen.insert(key);
            self.core.note(format!(
                "GPU error: {}",
                e.split_whitespace().collect::<Vec<_>>().join(" ")
            ));
        }
        if self.gpu_events.lost.swap(false, Ordering::SeqCst) {
            self.core
                .note("the GPU was lost; starting again on a new device");
            // Before the old device goes, so that its going isn't taken for another loss.
            self.gpu_events = Arc::default();
            self.outputs.clear();
            self.gpu = None;
            self.gpu_errors_seen.clear();
            self.core.gpu_lost();
            self.sync_pending = true;
            self.retry_at = self.core.now();
        }
        if let Some(g) = &self.gpu
            && std::env::var_os("JETCARP_HOST_TEST_LOSE_GPU").is_some()
            && self.core.now() >= 3.0
            && !self.lost_on_purpose
        {
            // For testing the recovery above: lose the device once, three seconds in.
            self.lost_on_purpose = true;
            g.device.destroy();
        }
    }

    #[cfg(any(windows, target_os = "macos"))]
    fn run_tray(&mut self) {
        use crate::tray::Do;
        let Some(tray) = &self.tray else { return };
        for action in tray.poll() {
            let result = match action {
                Do::Next => self.core.skip(1, None),
                Do::Previous => self.core.skip(-1, None),
                Do::Pause(on) => {
                    self.core.set_paused(on);
                    Ok(())
                }
                Do::Rotation(name) => self.core.set_rotation(&name, None),
                Do::Wallpaper(on) => self.core.set_wallpaper(on),
                Do::OpenApp => self.core.open_app(),
                Do::RunAtLogin(on) => self.core.set_login(on),
                Do::Quit => {
                    self.core.quit = true;
                    Ok(())
                }
            };
            if let Err(e) = result {
                self.core.note(e);
            }
        }
        let now = self.core.now();
        if now - self.tray_refreshed >= 1.0 {
            self.tray_refreshed = now;
            let label = self
                .core
                .screens
                .first()
                .map(|s| s.stage.status().label)
                .unwrap_or_default();
            let current = self
                .core
                .screens
                .first()
                .map(|s| s.stage.rotation().to_string())
                .unwrap_or_default();
            if let Some(t) = &self.tray {
                t.refresh(
                    &label,
                    &current,
                    self.core.all_paused(),
                    self.core.wants_wallpaper(),
                    jetcarp_display::autostart::enabled(),
                );
            }
        }
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, el: &ActiveEventLoop) {
        if !self.outputs.is_empty() {
            return;
        }
        self.sync_outputs(el);
        #[cfg(any(windows, target_os = "macos"))]
        if self.mode == Mode::Host {
            match crate::tray::Tray::new(&crate::core::rotation_names()) {
                Ok(t) => self.tray = Some(t),
                Err(e) => self.core.note(format!("no tray icon: {e}")),
            }
        }
    }

    fn window_event(&mut self, el: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
        let Some(i) = self.outputs.iter().position(|o| o.body.id() == Some(id)) else {
            return;
        };
        if self.ends_saver(&event) {
            el.exit();
            return;
        }
        match event {
            WindowEvent::CloseRequested => el.exit(),
            WindowEvent::Resized(size) if size.width > 0 && size.height > 0 => {
                if let Some(gpu) = &self.gpu {
                    let o = &mut self.outputs[i];
                    o.config.width = size.width;
                    o.config.height = size.height;
                    o.surface.configure(&gpu.device, &o.config);
                }
                self.fit_screens();
            }
            WindowEvent::RedrawRequested => self.draw(i),
            _ => {}
        }
    }

    fn about_to_wait(&mut self, el: &ActiveEventLoop) {
        let now = Instant::now();
        if now >= self.next_frame {
            self.watch_gpu();
            self.core.wallpaper_failing = self.mode == Mode::Host
                && self.core.wants_wallpaper()
                && (self.core.shell() == Shell::Gone || self.desktop_gave_up);
            self.core.tick();
            #[cfg(any(windows, target_os = "macos"))]
            self.run_tray();
            if self.core.finished() || self.ends_at.is_some_and(|t| now >= t) {
                el.exit();
                return;
            }
            if self.outputs.iter().any(|o| !o.body.alive()) {
                // Explorer restarted, or the Screen Saver dialog closed its preview.
                if self.mode != Mode::Host {
                    el.exit();
                    return;
                }
                self.outputs.retain(|o| o.body.alive());
                self.sync_pending = true;
            }
            if self.core.screens_changed {
                self.desktop_gave_up = false;
            }
            let hosting = self.mode == Mode::Host;
            let mismatch = hosting
                && self.outputs.iter().any(|o| {
                    o.target.screen >= self.core.screens.len()
                        || o.target.wallpaper != self.shown_as_wallpaper(o.target.screen)
                });
            if self.core.screens_changed
                || mismatch
                || (self.sync_pending && self.core.now() >= self.retry_at)
            {
                self.sync_outputs(el);
            }
            if hosting {
                self.watch_monitors(el);
            }
            for i in 0..self.outputs.len() {
                if self.outputs[i].target.wallpaper {
                    self.draw(i);
                } else if let Some(w) = self.outputs[i].body.os() {
                    w.request_redraw();
                }
            }
            self.next_frame = now + self.core.frame_time();
        }
        el.set_control_flow(ControlFlow::WaitUntil(self.next_frame));
    }
}

/// Runs until the host quits, a window closes, or `seconds` pass, then ends the process: with 0,
/// or with 1 after printing what went wrong.
pub fn run(core: Core, mode: Mode, seconds: Option<f64>) -> ! {
    #[allow(unused_mut)]
    let mut builder = EventLoop::builder();
    // No Dock icon until there is a plain window to switch to (see `sync_outputs`).
    #[cfg(target_os = "macos")]
    {
        use winit::platform::macos::{ActivationPolicy, EventLoopBuilderExtMacOS};
        builder.with_activation_policy(ActivationPolicy::Accessory);
    }
    let el = match builder.build() {
        Ok(el) => el,
        Err(e) => {
            say!("jetcarp-host: cannot reach the display: {e}");
            std::process::exit(1);
        }
    };
    let mut app = App {
        core,
        mode,
        instance: wgpu::Instance::new(
            wgpu::InstanceDescriptor::new_without_display_handle_from_env(),
        ),
        gpu: None,
        gpu_events: Arc::default(),
        gpu_errors_seen: BTreeSet::new(),
        gpu_errors_unnoted: 0,
        lost_on_purpose: false,
        outputs: vec![],
        next_frame: Instant::now(),
        ends_at: seconds.map(|s| Instant::now() + Duration::from_secs_f64(s)),
        error: None,
        monitors_at: f64::NEG_INFINITY,
        geometry: vec![],
        sync_pending: false,
        retry_at: 0.0,
        desktop_gave_up: false,
        #[cfg(windows)]
        desktop_failing_since: None,
        #[cfg(windows)]
        had_desktop: false,
        noted_fallback: false,
        noted_starved: false,
        pointer_from: None,
        #[cfg(any(windows, target_os = "macos"))]
        tray: None,
        #[cfg(any(windows, target_os = "macos"))]
        tray_refreshed: f64::NEG_INFINITY,
    };
    let ran = el.run_app(&mut app).map_err(|e| e.to_string());
    // The surfaces go first, while the device lives (and on Windows before Explorer's desktop is
    // put back over where the wallpapers were).
    app.outputs.clear();
    #[cfg(windows)]
    if app.had_desktop && mode == Mode::Host {
        crate::desktop_win::restore_desktop();
    }
    if mode == Mode::Host && app.core.wants_still() {
        match app.core.leave_still() {
            Ok(p) => say!("jetcarp-host: left {} as the desktop picture", p.display()),
            Err(e) => say!("jetcarp-host: no still picture: {e}"),
        }
    }
    // Windows keeps a dead icon in the tray until the pointer passes over it, unless it is
    // taken away.
    #[cfg(any(windows, target_os = "macos"))]
    {
        app.tray = None;
    }
    app.core.shutdown();
    let code = match ran.and(app.error.take().map_or(Ok(()), Err)) {
        Ok(()) => 0,
        Err(e) => {
            say!("jetcarp-host: {e}");
            1
        }
    };
    // `app` keeps the device and the instance: they end with the process.
    jetcarp_display::exit::end_process(code)
}
