//! JETCARP's macOS screen saver: the code inside `JETCARP.saver`.
//!
//! macOS loads the bundle into its own sandboxed process (`legacyScreenSaver`) and makes one
//! `JETCARPSaverView` per screen, plus a small one for the preview in System Settings. Every view in
//! the process shares one stage, so all screens show the same simulation, built for the largest of
//! them. Everything shown comes from the settings baked into the bundle when it was installed
//! (`jetcarp_display::macsaver`); nothing outside the bundle is read.
//!
//! Since macOS 14 the screen saver process keeps running after the saver is dismissed, and stacks
//! a new set of views on top each time. So the saver quits its process when macOS says the screen
//! saver is stopping, as other third-party savers do.
//!
//! macOS also makes full-screen views ahead of time (when the saver is chosen in System Settings,
//! and after each run) and asks them to animate at once, though nothing shows them: on the Mac in
//! rc1 one drew about 3,700 frames at 4112 x 2658 in the two minutes before the saver started.
//! Their windows look the same as the shown ones (macOS reports neither as on screen, since both
//! are shown from another process), so a full-screen view animates only once a screen saver is
//! known to be running: macOS has said so (`com.apple.screensaver.didstart`), or its screen saver
//! app (`com.apple.ScreenSaver.Engine`) is running. Until then it draws its first frame and waits,
//! and the simulation does not step. The preview in System Settings always animates.
//!
//! A log goes to `jetcarp-saver.log` in the process's temporary folder, inside the sandbox's
//! container (`~/Library/Containers/com.apple.ScreenSaver.Engine.legacyScreenSaver/Data/tmp/` on
//! macOS 27), and to the system log, each line starting `jetcarp-saver:`.

#![cfg(target_os = "macos")]

use jetcarp_display::macsaver::{BAKED_FILE, BAKED_FORMAT, BUNDLE_ID, Baked};
use jetcarp_display::painter::{self, Painter};
use jetcarp_display::simgpu::SimBackend;
use jetcarp_display::stage::{self, Stage};
use objc2::rc::{Allocated, Retained};
use objc2::runtime::{AnyObject, Bool, NSObjectProtocol};
use objc2::{ClassType, DefinedClass, MainThreadOnly, class, define_class, msg_send, sel};
use objc2_foundation::{NSRect, NSString};
use objc2_screen_saver::ScreenSaverView;
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::ffi::c_void;
use std::time::{Duration, Instant};

/// Share of a frame spent stepping; the rest is for drawing.
const STEP_SHARE: f32 = 0.6;
const WILL_STOP: &str = "com.apple.screensaver.willstop";
const DID_START: &str = "com.apple.screensaver.didstart";
/// macOS's screen saver app, which runs while a screen saver shows (launching it starts one).
const ENGINE_ID: &str = "com.apple.ScreenSaver.Engine";
/// How often a view waiting for the saver to start asks whether the screen saver app runs.
const ENGINE_EVERY: Duration = Duration::from_secs(1);
/// A view told to start animating this long after it was made is one macOS made ahead of time
/// (logged as such). Not every such view is told late: the one made when the saver is chosen is
/// told 0.5 s after, as the shown ones are, which is why drawing waits for a saver to start.
const STARTED_LATE: Duration = Duration::from_secs(5);

unsafe extern "C" {
    fn syslog(priority: i32, format: *const std::ffi::c_char, ...);
}
/// `LOG_NOTICE`.
const NOTICE: i32 = 5;

/// Also to the system log (`log show --predicate 'eventMessage BEGINSWITH "jetcarp-saver"'`), since
/// the sandbox's folder can't be read from a terminal without Full Disk Access.
fn log(line: &str) {
    use std::io::Write;
    // `NSLog` would do, but the system log hides what it says as `<private>`; syslog's isn't.
    if let Ok(text) = std::ffi::CString::new(format!("jetcarp-saver: {line}")) {
        unsafe { syslog(NOTICE, c"%s".as_ptr(), text.as_ptr()) };
    }
    let path = std::env::temp_dir().join("jetcarp-saver.log");
    if std::fs::metadata(&path).is_ok_and(|m| m.len() > 1 << 20) {
        let _ = std::fs::remove_file(&path);
    }
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
    {
        let t = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs());
        let _ = writeln!(f, "{t} pid {}: {line}", std::process::id());
    }
}

struct Gpu {
    adapter: wgpu::Adapter,
    device: wgpu::Device,
    queue: wgpu::Queue,
}

/// What every view in this process shares.
struct Shared {
    instance: wgpu::Instance,
    gpu: Option<Gpu>,
    /// Taken when the stage is made.
    baked: Option<Baked>,
    fps: f32,
    caption: jetcarp_display::caption::CaptionSettings,
    use_gpu: bool,
    stage: Option<Stage>,
    sim: SimBackend,
    started: Instant,
    ticked_at: f64,
    /// Each view's size in pixels, by view.
    sizes: HashMap<usize, (u32, u32)>,
    /// Each view's pixels per point, by view.
    scales: HashMap<usize, f64>,
    /// When each view was made and last drew, by view.
    views: HashMap<usize, Seen>,
}

struct Seen {
    made: Instant,
    drew: Option<Instant>,
}

/// macOS makes views ahead of time and keeps animating them after it adds the ones it shows. A
/// view made at least this much later at the same size, and drawing, replaces an older one...
const NEWER_BY: Duration = Duration::from_secs(1);
/// ...which then draws only this often, in case it is the one on screen after all.
const REPLACED_EVERY: Duration = Duration::from_secs(1);

impl Shared {
    /// Whether a newer view of the same size is drawing in place of this one.
    fn replaced(&self, view: usize, size: (u32, u32)) -> bool {
        let Some(mine) = self.views.get(&view) else {
            return false;
        };
        self.views.iter().any(|(k, other)| {
            *k != view
                && other.made >= mine.made + NEWER_BY
                && self.sizes.get(k) == Some(&size)
                && other.drew.is_some_and(|t| t.elapsed() < REPLACED_EVERY * 2)
        })
    }
}

thread_local! {
    static SHARED: RefCell<Option<Shared>> = const { RefCell::new(None) };
    /// macOS has said a screen saver started (`com.apple.screensaver.didstart`).
    static SHOWING: Cell<bool> = const { Cell::new(false) };
    /// When the screen saver app was last looked for, and whether it was running.
    static ENGINE: Cell<(Option<Instant>, bool)> = const { Cell::new((None, false)) };
}

/// Whether macOS's screen saver app is running, asked at most every [`ENGINE_EVERY`].
fn engine_running() -> bool {
    let (at, was) = ENGINE.get();
    if at.is_some_and(|t| t.elapsed() < ENGINE_EVERY) {
        return was;
    }
    let now = unsafe {
        objc2::runtime::AnyClass::get(c"NSRunningApplication").is_some_and(|cls| {
            let list: Option<Retained<AnyObject>> = msg_send![cls,
                runningApplicationsWithBundleIdentifier: &*NSString::from_str(ENGINE_ID)];
            list.is_some_and(|l| {
                let n: usize = msg_send![&*l, count];
                n > 0
            })
        })
    };
    if now && !was {
        log("macOS's screen saver app is running: a screen saver is showing");
    }
    ENGINE.set((Some(Instant::now()), now));
    now
}

/// Whether a screen saver is known to be running, so a full-screen view may be on screen.
fn saver_running() -> bool {
    SHOWING.get() || engine_running()
}

#[repr(C)]
struct DlInfo {
    fname: *const std::ffi::c_char,
    fbase: *mut c_void,
    sname: *const std::ffi::c_char,
    saddr: *mut c_void,
}

unsafe extern "C" {
    fn dladdr(addr: *const c_void, info: *mut DlInfo) -> i32;
}

/// This bundle. `bundleForClass:` can't be used: a class registered at run time, as this one is,
/// belongs to no loaded image, so it answers with the screen saver app's own bundle.
fn own_bundle() -> Result<Retained<AnyObject>, String> {
    let by_id: Option<Retained<AnyObject>> = unsafe {
        msg_send![class!(NSBundle), bundleWithIdentifier: &*NSString::from_str(BUNDLE_ID)]
    };
    if let Some(b) = by_id {
        return Ok(b);
    }
    // From the path of this code: JETCARP.saver/Contents/MacOS/JETCARP.
    let mut info = DlInfo {
        fname: std::ptr::null(),
        fbase: std::ptr::null_mut(),
        sname: std::ptr::null(),
        saddr: std::ptr::null_mut(),
    };
    let found = unsafe { dladdr(own_bundle as *const c_void, &mut info) };
    if found == 0 || info.fname.is_null() {
        return Err("no bundle".into());
    }
    let exe = unsafe { std::ffi::CStr::from_ptr(info.fname) }.to_string_lossy();
    let root = std::path::Path::new(&*exe)
        .ancestors()
        .nth(3)
        .ok_or(format!("{exe} is not inside a bundle"))?;
    let b: Option<Retained<AnyObject>> = unsafe {
        msg_send![class!(NSBundle), bundleWithPath: &*NSString::from_str(&root.to_string_lossy())]
    };
    b.ok_or(format!("no bundle at {}", root.display()))
}

/// The baked settings in this bundle's Resources.
fn read_baked() -> Result<Baked, String> {
    let bundle = own_bundle()?;
    let (stem, ext) = BAKED_FILE
        .rsplit_once('.')
        .expect("a file name with a type");
    let path: Option<Retained<NSString>> = unsafe {
        msg_send![&bundle, pathForResource: &*NSString::from_str(stem),
            ofType: &*NSString::from_str(ext)]
    };
    let path = path.ok_or(format!("{BAKED_FILE} is missing from the bundle"))?;
    let path = path.to_string();
    let text = std::fs::read_to_string(&path).map_err(|e| format!("{path}: {e}"))?;
    let baked: Baked = serde_json::from_str(&text).map_err(|e| format!("{path}: {e}"))?;
    if baked.format != BAKED_FORMAT {
        return Err(format!(
            "{path} is format {}, this code reads {BAKED_FORMAT}; install the saver again",
            baked.format
        ));
    }
    Ok(baked)
}

impl Shared {
    fn load() -> Result<Shared, String> {
        jetcarp_core::pile::set_gpu_factory(|| {
            jetcarp_gpu::pile::PileGpu::headless()
                .map(|g| Box::new(g) as Box<dyn jetcarp_core::pile::Toppler>)
        });
        let baked = read_baked()?;
        log(&format!(
            "rotation {:?}, {} items",
            baked.rotation.name,
            baked.slots.len()
        ));
        Ok(Shared {
            instance: wgpu::Instance::new(
                wgpu::InstanceDescriptor::new_without_display_handle_from_env(),
            ),
            gpu: None,
            fps: baked.fps.max(1.0),
            caption: baked.caption,
            use_gpu: baked.gpu,
            baked: Some(baked),
            stage: None,
            sim: SimBackend::unavailable("no GPU yet".into()),
            started: Instant::now(),
            ticked_at: f64::NEG_INFINITY,
            sizes: HashMap::new(),
            scales: HashMap::new(),
            views: HashMap::new(),
        })
    }

    fn now(&self) -> f64 {
        self.started.elapsed().as_secs_f64()
    }

    /// The device, made the first time with a surface it must suit.
    fn gpu_for(&mut self, surface: &wgpu::Surface<'static>) -> Result<&Gpu, String> {
        if self.gpu.is_none() {
            let adapter =
                pollster::block_on(self.instance.request_adapter(&wgpu::RequestAdapterOptions {
                    power_preference: wgpu::PowerPreference::HighPerformance,
                    compatible_surface: Some(surface),
                    ..Default::default()
                }))
                .map_err(|e| format!("no graphics adapter: {e}"))?;
            let (device, queue) =
                pollster::block_on(adapter.request_device(&painter::device_descriptor(&adapter)))
                    .map_err(|e| format!("no graphics device: {e}"))?;
            // Never panic inside macOS's process.
            device.on_uncaptured_error(std::sync::Arc::new(|e| log(&format!("GPU error: {e}"))));
            log(&format!("GPU {}", adapter.get_info().name));
            self.sim = if self.use_gpu {
                SimBackend::from_device(&adapter, device.clone(), queue.clone())
            } else {
                SimBackend::unavailable("GPU stepping is off in the host's settings".into())
            };
            self.gpu = Some(Gpu {
                adapter,
                device,
                queue,
            });
        }
        Ok(self.gpu.as_ref().expect("made above"))
    }

    /// Records a view's size and scale, and makes or refits the stage for the largest view, with
    /// cells sized for the sharpest.
    fn fit(&mut self, view: usize, size: (u32, u32), scale: f64) -> Result<(), String> {
        self.sizes.insert(view, size);
        self.scales.insert(view, scale);
        let canvas = painter::shared_canvas(self.sizes.values().copied()).expect("one size");
        let scale = self.scales.values().copied().fold(1.0, f64::max);
        let now = self.now();
        match (&mut self.stage, self.baked.take()) {
            (Some(s), _) => s.resize(canvas, scale, now),
            (None, Some(b)) => {
                let seed = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map_or(1, |d| d.as_nanos() as u64);
                let s = Stage::new(
                    &b.rotation,
                    b.slots,
                    seed,
                    canvas,
                    stage::no_lookup(),
                    b.max_cells,
                )?;
                let mut s = s;
                s.resize(canvas, scale, now);
                self.stage = Some(s);
            }
            (None, None) => return Err("the stage could not be made".into()),
        }
        Ok(())
    }

    /// Steps the stage once per frame, however many views ask.
    fn tick(&mut self) {
        let now = self.now();
        if now - self.ticked_at < 0.5 / self.fps as f64 {
            return;
        }
        self.ticked_at = now;
        let budget = Duration::from_secs_f32(STEP_SHARE / self.fps);
        if let Some(s) = &mut self.stage {
            s.tick(now, &mut self.sim, budget);
            for n in s.take_notes() {
                log(&n);
            }
        }
    }
}

/// One view's surface. wgpu draws into a Metal layer of the saver's own, not one it adds to the
/// view itself, because of what [`LayerDelegate`] answers for it.
struct Surface {
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    painter: Painter,
    layer: Retained<AnyObject>,
    /// A layer only holds its delegate weakly.
    _delegate: Retained<LayerDelegate>,
    /// Frames shown so far, for the log.
    shown: u64,
    /// When the last one was.
    drew: Option<Instant>,
    /// Why frames weren't shown, each logged the first time.
    skipped: Vec<&'static str>,
}

/// `kCALayerWidthSizable | kCALayerHeightSizable`.
const FOLLOW_SIZE: u32 = (1 << 1) | (1 << 4);
/// `NSWindowOcclusionStateVisible`.
const VISIBLE: usize = 1 << 1;
/// A frame that takes longer than this to be handed one is logged.
const SLOW_FRAME: Duration = Duration::from_millis(250);

define_class!(
    // SAFETY: NSObject has no rules for subclasses, and this one only adds a method.
    #[unsafe(super(objc2::runtime::NSObject))]
    #[name = "JETCARPSaverLayerDelegate"]
    struct LayerDelegate;

    unsafe impl NSObjectProtocol for LayerDelegate {}

    impl LayerDelegate {
        /// wgpu finds the window a Metal layer is in by asking the first of its layers that has a
        /// delegate, and gives no frame while that window isn't on screen. Inside macOS's screen
        /// saver process it never is: the view's window is shown in System Settings or the screen
        /// saver from another process. So this delegate answers that there is no window.
        #[unsafe(method(window))]
        fn window(&self) -> *mut AnyObject {
            std::ptr::null_mut()
        }
    }
);

impl LayerDelegate {
    fn new() -> Retained<Self> {
        unsafe { msg_send![Self::class(), new] }
    }
}

pub struct Ivars {
    preview: bool,
    surface: RefCell<Option<Surface>>,
    /// Set after an error, so it is logged once and the view stays black.
    broken: Cell<bool>,
    /// Why the last frame wasn't drawn, so each reason is logged once.
    waiting: RefCell<String>,
    made: Instant,
}

impl Drop for Ivars {
    fn drop(&mut self) {
        // The surface goes before the view it was made from.
        self.surface.get_mut().take();
    }
}

define_class!(
    // SAFETY: ScreenSaverView is made to be subclassed, and this class's `Drop` only removes it
    // from the notification centre and from the shared sizes.
    #[unsafe(super(ScreenSaverView, objc2_app_kit::NSView, objc2_app_kit::NSResponder, objc2::runtime::NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "JETCARPSaverView"]
    #[ivars = Ivars]
    pub struct JetcarpSaverView;

    unsafe impl NSObjectProtocol for JetcarpSaverView {}

    impl JetcarpSaverView {
        #[unsafe(method_id(initWithFrame:isPreview:))]
        fn init_with_frame(
            this: Allocated<Self>,
            frame: NSRect,
            is_preview: Bool,
        ) -> Option<Retained<Self>> {
            let this = this.set_ivars(Ivars {
                preview: is_preview.as_bool(),
                surface: RefCell::new(None),
                broken: Cell::new(false),
                waiting: RefCell::new("not started".into()),
                made: Instant::now(),
            });
            let this: Option<Retained<Self>> =
                unsafe { msg_send![super(this), initWithFrame: frame, isPreview: is_preview] };
            if let Some(view) = &this {
                view.started();
            }
            this
        }

        #[unsafe(method(animateOneFrame))]
        fn animate_one_frame(&self) {
            if self.ivars().broken.get() {
                return;
            }
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| self.frame()));
            let failed = match result {
                Ok(Ok(())) => None,
                Ok(Err(e)) => Some(e),
                Err(_) => Some("the saver panicked".to_string()),
            };
            if let Some(e) = failed {
                log(&format!("stopped drawing: {e}"));
                self.ivars().broken.set(true);
            }
        }

        #[unsafe(method(startAnimation))]
        fn start_animation(&self) {
            let age = self.ivars().made.elapsed();
            let late = age >= STARTED_LATE;
            log(&format!(
                "startAnimation {:.1} s after the view was made{}, preview {}",
                age.as_secs_f64(),
                if late { " (made ahead of time)" } else { "" },
                self.ivars().preview
            ));
            unsafe {
                let _: () = msg_send![super(self), startAnimation];
            }
        }

        // These two only log, for finding out what macOS calls and when.
        #[unsafe(method(stopAnimation))]
        fn stop_animation(&self) {
            log(&format!("stopAnimation, preview {}", self.ivars().preview));
            unsafe {
                let _: () = msg_send![super(self), stopAnimation];
            }
        }

        #[unsafe(method(viewDidMoveToWindow))]
        fn view_did_move_to_window(&self) {
            let window: Option<Retained<AnyObject>> = unsafe { msg_send![self, window] };
            let (w, h, _) = self.pixels();
            log(&format!(
                "in a window: {}, {w} x {h}, preview {}",
                window.is_some(),
                self.ivars().preview
            ));
            unsafe {
                let _: () = msg_send![super(self), viewDidMoveToWindow];
            }
        }

        #[unsafe(method(hasConfigureSheet))]
        fn has_configure_sheet(&self) -> Bool {
            Bool::NO
        }

        #[unsafe(method(jetcarpDidStart:))]
        fn did_start(&self, _note: *mut AnyObject) {
            if !SHOWING.replace(true) {
                log("macOS says a screen saver started");
            }
        }

        #[unsafe(method(jetcarpWillStop:))]
        fn will_stop(&self, _note: *mut AnyObject) {
            log("macOS says the screen saver is stopping; ending this process");
            std::process::exit(0);
        }
    }
);

impl JetcarpSaverView {
    fn key(&self) -> usize {
        self as *const Self as usize
    }

    fn started(&self) {
        let preview = self.ivars().preview;
        log(&format!("view made, preview {preview}"));
        let fps = SHARED.with_borrow_mut(|sh| {
            if sh.is_none() {
                match Shared::load() {
                    Ok(s) => *sh = Some(s),
                    Err(e) => log(&e),
                }
            }
            let key = self.key();
            sh.as_mut().map_or(30.0, |s| {
                s.views.insert(
                    key,
                    Seen {
                        made: Instant::now(),
                        drew: None,
                    },
                );
                s.fps
            })
        });
        unsafe {
            let _: () = msg_send![self, setAnimationTimeInterval: 1.0 / fps as f64];
        }
        if !preview {
            unsafe {
                let center: *mut AnyObject =
                    msg_send![class!(NSDistributedNotificationCenter), defaultCenter];
                let _: () = msg_send![center, addObserver: self,
                    selector: sel!(jetcarpWillStop:),
                    name: &*NSString::from_str(WILL_STOP),
                    object: std::ptr::null::<AnyObject>()];
                let _: () = msg_send![center, addObserver: self,
                    selector: sel!(jetcarpDidStart:),
                    name: &*NSString::from_str(DID_START),
                    object: std::ptr::null::<AnyObject>()];
            }
        }
    }

    /// The view's size in pixels.
    fn pixels(&self) -> (u32, u32, f32) {
        let bounds: NSRect = unsafe { msg_send![self, bounds] };
        let px: NSRect = unsafe { msg_send![self, convertRectToBacking: bounds] };
        let scale = if bounds.size.width > 0.0 {
            px.size.width / bounds.size.width
        } else {
            1.0
        };
        (
            px.size.width.round().max(0.0) as u32,
            px.size.height.round().max(0.0) as u32,
            scale as f32,
        )
    }

    /// What macOS says about the view's window, for the log.
    fn describe_window(&self) -> String {
        let window: Option<Retained<AnyObject>> = unsafe { msg_send![self, window] };
        match window {
            Some(w) => {
                let state: usize = unsafe { msg_send![&*w, occlusionState] };
                format!(
                    "window {:?}, on screen as macOS sees it: {}",
                    w.class().name(),
                    state & VISIBLE != 0
                )
            }
            None => "no window".into(),
        }
    }

    /// Sizes the Metal layer to the view, with no animation.
    fn place_layer(&self, layer: &AnyObject, scale: f32) {
        let bounds: NSRect = unsafe { msg_send![self, bounds] };
        unsafe {
            let _: () = msg_send![class!(CATransaction), begin];
            let _: () = msg_send![class!(CATransaction), setDisableActions: Bool::YES];
            let _: () = msg_send![layer, setFrame: bounds];
            let _: () = msg_send![layer, setContentsScale: scale as f64];
            let _: () = msg_send![class!(CATransaction), commit];
        }
    }

    fn configure(s: &Surface, device: &wgpu::Device) {
        s.surface.configure(device, &s.config);
        // wgpu waits for a drawable with no time limit. With one, a frame macOS never hands over
        // is a timeout in the log rather than a saver that hangs.
        unsafe {
            let _: () = msg_send![&*s.layer, setAllowsNextDrawableTimeout: Bool::YES];
        }
    }

    fn make_surface(
        &self,
        sh: &mut Shared,
        size: (u32, u32),
        scale: f32,
    ) -> Result<Surface, String> {
        let layer: Retained<AnyObject> = unsafe { msg_send![class!(CAMetalLayer), new] };
        let delegate = LayerDelegate::new();
        unsafe {
            let _: () = msg_send![&*layer, setDelegate: &*delegate];
            let _: () = msg_send![&*layer, setAutoresizingMask: FOLLOW_SIZE];
            let _: () = msg_send![self, setWantsLayer: Bool::YES];
            let host: Option<Retained<AnyObject>> = msg_send![self, layer];
            let host = host.ok_or("the view has no layer")?;
            let _: () = msg_send![&*host, addSublayer: &*layer];
        }
        self.place_layer(&layer, scale);
        let ptr = Retained::as_ptr(&layer) as *mut c_void;
        let surface = unsafe {
            sh.instance
                .create_surface_unsafe(wgpu::SurfaceTargetUnsafe::CoreAnimationLayer(ptr))
                .map_err(|e| format!("no surface: {e}"))?
        };
        let gpu = sh.gpu_for(&surface)?;
        let caps = surface.get_capabilities(&gpu.adapter);
        let config = painter::surface_config(&caps, size)?;
        let s = Surface {
            painter: Painter::new(&gpu.device, config.format),
            surface,
            config,
            layer,
            _delegate: delegate,
            shown: 0,
            drew: None,
            skipped: Vec::new(),
        };
        Self::configure(&s, &gpu.device);
        Ok(s)
    }

    /// Logs why a frame wasn't shown, the first time for each reason.
    fn skipped(&self, s: &mut Surface, why: &'static str) {
        if !s.skipped.contains(&why) {
            s.skipped.push(why);
            log(&format!(
                "no frame: {why}, after {} shown (preview {}; {})",
                s.shown,
                self.ivars().preview,
                self.describe_window()
            ));
        }
    }

    fn frame(&self) -> Result<(), String> {
        let has_window: Option<Retained<AnyObject>> = unsafe { msg_send![self, window] };
        let (w, h, scale) = self.pixels();
        let waiting = match (has_window.is_some(), w > 0 && h > 0) {
            (false, _) => "no window",
            (true, false) => "a size of zero",
            (true, true) => "",
        };
        if *self.ivars().waiting.borrow() != waiting {
            if !waiting.is_empty() {
                log(&format!(
                    "waiting: {waiting} (preview {})",
                    self.ivars().preview
                ));
            }
            *self.ivars().waiting.borrow_mut() = waiting.to_string();
        }
        if !waiting.is_empty() {
            return Ok(());
        }
        SHARED.with_borrow_mut(|sh| {
            let sh = sh
                .as_mut()
                .ok_or("the saver's settings could not be read")?;
            let mut slot = self.ivars().surface.borrow_mut();
            if slot.is_none() {
                *slot = Some(self.make_surface(sh, (w, h), scale)?);
                log(&format!(
                    "surface made at {w} x {h} (preview {}; {})",
                    self.ivars().preview,
                    self.describe_window()
                ));
            }
            let s = slot.as_mut().expect("made above");
            let gpu = sh.gpu.as_ref().ok_or("no GPU")?;
            if (s.config.width, s.config.height) != (w, h) {
                s.config.width = w;
                s.config.height = h;
                self.place_layer(&s.layer, scale);
                Self::configure(s, &gpu.device);
            }
            // A full-screen view macOS made ahead of time is shown by nothing until a screen
            // saver starts: it keeps its first frame, and neither draws nor steps until then.
            if !self.ivars().preview && s.shown > 0 && !saver_running() {
                self.skipped(
                    s,
                    "no screen saver is running, so nothing shows this view (made ahead of time); waiting for one to start",
                );
                return Ok(());
            }
            sh.fit(self.key(), (w, h), scale as f64)?;
            if sh.replaced(self.key(), (w, h)) {
                if s.drew.is_some_and(|t| t.elapsed() < REPLACED_EVERY) {
                    return Ok(());
                }
                self.skipped(
                    s,
                    "a newer view of this size is drawing, so this one draws once a second",
                );
            }
            // Stepped only for a frame that is drawn.
            sh.tick();
            let gpu = sh.gpu.as_ref().ok_or("no GPU")?;
            let asked = Instant::now();
            let got = s.surface.get_current_texture();
            if asked.elapsed() > SLOW_FRAME {
                self.skipped(s, "slow to be handed a frame");
            }
            let frame = match got {
                wgpu::CurrentSurfaceTexture::Success(t)
                | wgpu::CurrentSurfaceTexture::Suboptimal(t) => t,
                wgpu::CurrentSurfaceTexture::Timeout => {
                    self.skipped(s, "timed out");
                    return Ok(());
                }
                wgpu::CurrentSurfaceTexture::Occluded => {
                    self.skipped(s, "occluded");
                    return Ok(());
                }
                _ => {
                    self.skipped(s, "outdated or lost");
                    Self::configure(s, &gpu.device);
                    return Ok(());
                }
            };
            let view = frame.texture.create_view(&Default::default());
            let now = sh.now();
            if let Some(stage) = &sh.stage {
                s.painter.paint(
                    &gpu.device,
                    &gpu.queue,
                    &view,
                    [w, h],
                    scale,
                    stage,
                    &sh.caption,
                    now,
                );
            }
            gpu.queue.present(frame);
            let drew = Instant::now();
            s.drew = Some(drew);
            if let Some(seen) = sh.views.get_mut(&self.key()) {
                seen.drew = Some(drew);
            }
            s.shown += 1;
            if matches!(s.shown, 1 | 100 | 1000) {
                log(&format!(
                    "{} frames shown (preview {})",
                    s.shown,
                    self.ivars().preview
                ));
            }
            Ok(())
        })
    }
}

impl Drop for JetcarpSaverView {
    fn drop(&mut self) {
        let key = self.key();
        let _ = SHARED.try_with(|sh| {
            if let Ok(mut sh) = sh.try_borrow_mut()
                && let Some(sh) = sh.as_mut()
            {
                sh.sizes.remove(&key);
                sh.scales.remove(&key);
                sh.views.remove(&key);
            }
        });
        if !self.ivars().preview {
            unsafe {
                let center: *mut AnyObject =
                    msg_send![class!(NSDistributedNotificationCenter), defaultCenter];
                let _: () = msg_send![center, removeObserver: &*self];
            }
        }
    }
}

/// macOS asks for the bundle's principal class by name, so it must exist as soon as the bundle is
/// loaded, before any Rust code is called.
extern "C" fn register() {
    let _ = std::panic::catch_unwind(|| {
        let _ = JetcarpSaverView::class();
    });
}

#[used]
#[unsafe(link_section = "__DATA,__mod_init_func")]
static REGISTER: extern "C" fn() = register;
