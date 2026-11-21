//! What the system asks for, for accessibility, and how the app follows it: reduced motion
//! (#218) and high contrast (#220).
//!
//! [`SystemSettings`] is the reading, behind a small trait so tests can stand it in:
//!
//! - **Windows:** *Show animations in Windows* (`SystemParametersInfoW(SPI_GETCLIENTAREAANIMATION)`)
//!   and the contrast themes (`SPI_GETHIGHCONTRAST`, `HCF_HIGHCONTRASTON`).
//! - **macOS:** `NSWorkspace`'s `accessibilityDisplayShouldReduceMotion` and
//!   `accessibilityDisplayShouldIncreaseContrast` (*Reduce motion* and *Increase contrast*).
//! - **Linux:** GNOME's `org.gnome.desktop.interface enable-animations` and
//!   `org.gnome.desktop.a11y.interface high-contrast`, through the XDG settings portal, and the
//!   portal's own `org.freedesktop.appearance` `reduced-motion` and `contrast` where a desktop
//!   gives those instead; `gsettings` when there is no portal. The portal is asked on a thread of
//!   its own, so a slow bus never holds up a frame.
//!
//! The app reads the system every couple of seconds ([`Watch`]), so a change made while it runs
//! shows within that. Its own setting ([`Follow`]) can override the system either way.
//!
//! What the app decided is kept on the egui context ([`set_motion_reduced`],
//! [`motion_reduced`]), so any window can ask without the app being passed down to it.

use eframe::egui;
use serde::{Deserialize, Serialize};

/// What the system says, where it can be read.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Reading {
    /// The system asks for reduced motion (`None`: it could not be read).
    pub reduce_motion: Option<bool>,
    /// The system asks for high contrast.
    pub high_contrast: Option<bool>,
}

/// Reads the system's accessibility settings. [`Os`] reads the real ones; tests stand in.
pub trait SystemSettings: Send {
    fn read(&mut self) -> Reading;
}

/// The app's own choice for one setting: follow the system (the default), or on or off whatever
/// it says.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Follow {
    #[default]
    System,
    On,
    Off,
}

impl Follow {
    pub const ALL: [Follow; 3] = [Follow::System, Follow::On, Follow::Off];

    /// On or off, given the system's reading: off when the system's cannot be read.
    pub fn resolve(self, system: Option<bool>) -> bool {
        match self {
            Follow::System => system.unwrap_or(false),
            Follow::On => true,
            Follow::Off => false,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Follow::System => "Follow the system",
            Follow::On => "On",
            Follow::Off => "Off",
        }
    }
}

/// How often the system is read again, in seconds.
pub const READ_EVERY: f64 = 2.0;

/// The system's settings, read now and every [`READ_EVERY`] seconds.
pub struct Watch {
    reader: Box<dyn SystemSettings>,
    read_at: f64,
    pub reading: Reading,
}

impl Watch {
    pub fn new(reader: Box<dyn SystemSettings>) -> Watch {
        Watch {
            reader,
            read_at: f64::NEG_INFINITY,
            reading: Reading::default(),
        }
    }

    /// The real system's settings.
    pub fn os() -> Watch {
        Watch::new(Box::new(Os::new()))
    }

    /// Read again if it is time (`now`: seconds on any steady clock). True when the reading
    /// changed.
    pub fn poll(&mut self, now: f64) -> bool {
        if now - self.read_at < READ_EVERY {
            return false;
        }
        self.read_at = now;
        let r = self.reader.read();
        let changed = r != self.reading;
        self.reading = r;
        changed
    }
}

/// A stand-in for tests and screenshot runs: whatever was last set.
#[derive(Clone, Default)]
pub struct Fixed(pub std::sync::Arc<std::sync::Mutex<Reading>>);

#[cfg(test)]
impl Fixed {
    pub fn new(r: Reading) -> Fixed {
        Fixed(std::sync::Arc::new(std::sync::Mutex::new(r)))
    }
    pub fn set(&self, r: Reading) {
        *self.0.lock().unwrap() = r;
    }
}

impl SystemSettings for Fixed {
    fn read(&mut self) -> Reading {
        *self.0.lock().unwrap()
    }
}

/// The system's own settings.
pub struct Os {
    #[cfg(target_os = "linux")]
    latest: std::sync::Arc<std::sync::Mutex<Reading>>,
}

impl Default for Os {
    fn default() -> Self {
        Self::new()
    }
}

impl Os {
    pub fn new() -> Os {
        #[cfg(target_os = "linux")]
        {
            Os {
                latest: linux::start(),
            }
        }
        #[cfg(not(target_os = "linux"))]
        {
            Os {}
        }
    }
}

impl SystemSettings for Os {
    fn read(&mut self) -> Reading {
        #[cfg(target_os = "linux")]
        {
            *self.latest.lock().unwrap()
        }
        #[cfg(windows)]
        {
            windows::read()
        }
        #[cfg(target_os = "macos")]
        {
            macos::read()
        }
        #[cfg(not(any(target_os = "linux", windows, target_os = "macos")))]
        {
            Reading::default()
        }
    }
}

#[cfg(windows)]
mod windows {
    use super::Reading;
    use windows_sys::Win32::UI::Accessibility::{HCF_HIGHCONTRASTON, HIGHCONTRASTW};
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        SPI_GETCLIENTAREAANIMATION, SPI_GETHIGHCONTRAST, SystemParametersInfoW,
    };

    pub fn read() -> Reading {
        let mut animate: i32 = 1;
        // SAFETY: SPI_GETCLIENTAREAANIMATION writes one BOOL to the pointer given, which points
        // at a local i32 that outlives the call.
        let ok = unsafe {
            SystemParametersInfoW(
                SPI_GETCLIENTAREAANIMATION,
                0,
                (&mut animate as *mut i32).cast(),
                0,
            )
        };
        let reduce_motion = (ok != 0).then_some(animate == 0);
        let mut hc = HIGHCONTRASTW {
            cbSize: std::mem::size_of::<HIGHCONTRASTW>() as u32,
            dwFlags: 0,
            lpszDefaultScheme: std::ptr::null_mut(),
        };
        // SAFETY: SPI_GETHIGHCONTRAST fills the HIGHCONTRASTW given, whose cbSize says its size;
        // the scheme name it may point to is owned by the system and never read here.
        let ok = unsafe {
            SystemParametersInfoW(
                SPI_GETHIGHCONTRAST,
                hc.cbSize,
                (&mut hc as *mut HIGHCONTRASTW).cast(),
                0,
            )
        };
        let high_contrast = (ok != 0).then_some(hc.dwFlags & HCF_HIGHCONTRASTON != 0);
        Reading {
            reduce_motion,
            high_contrast,
        }
    }
}

#[cfg(target_os = "macos")]
mod macos {
    use super::Reading;

    /// On the main thread only, where AppKit may be used (the app's frames run there); elsewhere
    /// (tests) nothing is read.
    pub fn read() -> Reading {
        use objc2::runtime::AnyObject;
        use objc2::{MainThreadMarker, class, msg_send};
        if MainThreadMarker::new().is_none() {
            return Reading::default();
        }
        // SAFETY: on the main thread; NSWorkspace's shared instance, checked for nil, and two of
        // its BOOL properties (macOS 10.10 and later).
        unsafe {
            let ws: *mut AnyObject = msg_send![class!(NSWorkspace), sharedWorkspace];
            if ws.is_null() {
                return Reading::default();
            }
            let reduce: bool = msg_send![ws, accessibilityDisplayShouldReduceMotion];
            let contrast: bool = msg_send![ws, accessibilityDisplayShouldIncreaseContrast];
            Reading {
                reduce_motion: Some(reduce),
                high_contrast: Some(contrast),
            }
        }
    }
}

#[cfg(target_os = "linux")]
mod linux {
    use super::Reading;
    use std::sync::{Arc, Mutex};
    use std::time::Duration;
    use zbus::blocking::Connection;
    use zbus::zvariant::OwnedValue;

    /// Read now (so the first scene already knows), then on a thread every couple of seconds;
    /// the app takes the latest.
    pub fn start() -> Arc<Mutex<Reading>> {
        let mut portal = Portal::connect();
        let latest = Arc::new(Mutex::new(portal.read()));
        let out = latest.clone();
        let _ = std::thread::Builder::new()
            .name("jetcarp-a11y-settings".into())
            .spawn(move || {
                loop {
                    std::thread::sleep(Duration::from_secs_f64(super::READ_EVERY));
                    // The app holds the other end; when it goes, so does this thread.
                    if Arc::strong_count(&out) == 1 {
                        return;
                    }
                    let r = portal.read();
                    *out.lock().unwrap() = r;
                }
            });
        latest
    }

    pub(super) struct Portal {
        bus: Option<Connection>,
        /// The portal answered once; gsettings is only asked when it never has.
        answered: bool,
    }

    impl Portal {
        fn connect() -> Portal {
            let bus = zbus::blocking::connection::Builder::session()
                .and_then(|b| b.method_timeout(Duration::from_secs(2)).build())
                .ok();
            Portal {
                bus,
                answered: false,
            }
        }

        /// One setting from org.freedesktop.portal.Settings (ReadOne, else the older Read, whose
        /// answer is a variant in a variant).
        fn get(&self, namespace: &str, key: &str) -> Option<OwnedValue> {
            let c = self.bus.as_ref()?;
            let call = |method: &str| {
                c.call_method(
                    Some("org.freedesktop.portal.Desktop"),
                    "/org/freedesktop/portal/desktop",
                    Some("org.freedesktop.portal.Settings"),
                    method,
                    &(namespace, key),
                )
                .ok()
                .and_then(|m| m.body().deserialize::<OwnedValue>().ok())
            };
            let v = call("ReadOne").or_else(|| call("Read"))?;
            // Read wraps the value once more.
            match &*v {
                zbus::zvariant::Value::Value(inner) => inner.try_to_owned().ok(),
                _ => Some(v),
            }
        }

        /// What the portal alone says (the by-hand test checks that it answers).
        #[cfg(test)]
        pub fn portal_only() -> Reading {
            let p = Portal::connect();
            Reading {
                reduce_motion: p
                    .bool_of("org.gnome.desktop.interface", "enable-animations")
                    .map(|on| !on),
                high_contrast: p.bool_of("org.gnome.desktop.a11y.interface", "high-contrast"),
            }
        }

        fn bool_of(&self, namespace: &str, key: &str) -> Option<bool> {
            self.get(namespace, key)
                .and_then(|v| bool::try_from(&v).ok())
        }

        fn u32_of(&self, namespace: &str, key: &str) -> Option<u32> {
            self.get(namespace, key)
                .and_then(|v| u32::try_from(&v).ok())
        }

        fn read(&mut self) -> Reading {
            let reduce_motion = self
                .bool_of("org.gnome.desktop.interface", "enable-animations")
                .map(|on| !on)
                .or_else(|| {
                    self.u32_of("org.freedesktop.appearance", "reduced-motion")
                        .map(|v| v == 1)
                });
            let high_contrast = self
                .bool_of("org.gnome.desktop.a11y.interface", "high-contrast")
                .or_else(|| {
                    self.u32_of("org.freedesktop.appearance", "contrast")
                        .map(|v| v == 1)
                });
            if reduce_motion.is_some() || high_contrast.is_some() {
                self.answered = true;
            }
            let mut r = Reading {
                reduce_motion,
                high_contrast,
            };
            if !self.answered {
                // No portal (a desktop without one, or none running yet): GNOME's own tool.
                if r.reduce_motion.is_none() {
                    r.reduce_motion =
                        gsettings("org.gnome.desktop.interface", "enable-animations").map(|b| !b);
                }
                if r.high_contrast.is_none() {
                    r.high_contrast =
                        gsettings("org.gnome.desktop.a11y.interface", "high-contrast");
                }
            }
            r
        }
    }

    fn gsettings(schema: &str, key: &str) -> Option<bool> {
        let out = std::process::Command::new("gsettings")
            .args(["get", schema, key])
            .stdin(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .output()
            .ok()?;
        if !out.status.success() {
            return None;
        }
        match String::from_utf8_lossy(&out.stdout).trim() {
            "true" => Some(true),
            "false" => Some(false),
            _ => None,
        }
    }
}

// ---------------------------------------------------------------------------------------------
// What the app decided, on the egui context

fn motion_id() -> egui::Id {
    egui::Id::unique("jetcarp-a11y-reduced-motion")
}

fn contrast_id() -> egui::Id {
    egui::Id::unique("jetcarp-a11y-high-contrast")
}

/// egui's own animations (windows and menus fading, headers opening, scrolling) as they are by
/// default.
const ANIMATION_TIME: f32 = 0.2;

/// Reduced motion is on (or off) for this context from now on: egui's own animations stop, and
/// [`motion_reduced`] says so to every window.
pub fn set_motion_reduced(ctx: &egui::Context, on: bool) {
    ctx.data_mut(|d| d.insert_temp(motion_id(), on));
    ctx.all_styles_mut(|s| {
        s.animation_time = if on { 0.0 } else { ANIMATION_TIME };
        s.scroll_animation = if on {
            egui::style::ScrollAnimation::none()
        } else {
            egui::style::ScrollAnimation::default()
        };
    });
}

/// Reduced motion is on: show stills, not animations, until one is chosen.
pub fn motion_reduced(ctx: &egui::Context) -> bool {
    ctx.data(|d| d.get_temp::<bool>(motion_id()).unwrap_or(false))
}

/// Record whether the high-contrast theme is on (set by [`crate::theme::apply`]).
pub(crate) fn set_high_contrast_flag(ctx: &egui::Context, on: bool) {
    ctx.data_mut(|d| d.insert_temp(contrast_id(), on));
}

/// The high-contrast theme is on.
pub fn high_contrast(ctx: &egui::Context) -> bool {
    ctx.data(|d| d.get_temp::<bool>(contrast_id()).unwrap_or(false))
}

/// A spinner, or with reduced motion a still sign of work going on: a ring with three dots,
/// read as busy by screen readers either way.
pub fn spinner(ui: &mut egui::Ui) -> egui::Response {
    if !motion_reduced(ui.ctx()) {
        return ui.spinner();
    }
    let size = ui.style().spacing.interact_size.y;
    let (rect, response) = ui.allocate_exact_size(egui::vec2(size, size), egui::Sense::hover());
    response.widget_info(|| egui::WidgetInfo::new(egui::Role::ProgressIndicator));
    if ui.is_rect_visible(rect) {
        let c = ui.visuals().strong_text_color();
        let r = rect.height() * 0.5 - 2.0;
        ui.painter()
            .circle_stroke(rect.center(), r, egui::Stroke::new(1.5, c));
        for k in -1..=1 {
            let p = rect.center() + egui::vec2(k as f32 * r * 0.5, 0.0);
            ui.painter().circle_filled(p, 1.6, c);
        }
    }
    response
}

/// Whether a progress bar should move (its shimmer), or, with reduced motion, stand still.
pub fn animate_bars(ctx: &egui::Context) -> bool {
    !motion_reduced(ctx)
}

/// A window's small animations (the Hunts dashboard's finds, an ant run): with reduced motion
/// each shows a still, its first frame, until the person chooses it with a click; another
/// click stills it again.
#[derive(Default)]
pub struct Stills {
    chosen: std::collections::HashSet<String>,
}

impl Stills {
    /// Whether the animation `key` moves now.
    pub fn moving(&self, ctx: &egui::Context, key: &str) -> bool {
        !motion_reduced(ctx) || self.chosen.contains(key)
    }

    /// The time to show `key` at: `time` while it moves, else 0 (its first frame). A click on
    /// its picture (`resp`) chooses it, or stills it again.
    pub fn time(&mut self, resp: &egui::Response, key: &str, time: f64) -> f64 {
        let ctx = resp.ctx.clone();
        if motion_reduced(&ctx) && resp.clicked() && !self.chosen.remove(key) {
            self.chosen.insert(key.to_string());
        }
        if self.moving(&ctx, key) { time } else { 0.0 }
    }

    /// Mark a still picture as one that plays when clicked: a small ▶ in its corner, and a word
    /// on hover.
    pub fn mark(&self, ui: &egui::Ui, resp: &egui::Response, key: &str) {
        if self.moving(ui.ctx(), key) {
            return;
        }
        let r = resp.rect;
        let at = r.right_bottom() - egui::vec2(10.0, 10.0);
        let p = ui.painter();
        p.circle_filled(at, 8.0, Color32::from_black_alpha(170));
        p.text(
            at + egui::vec2(1.0, 0.0),
            egui::Align2::CENTER_CENTER,
            "▶",
            egui::FontId::proportional(10.0),
            Color32::WHITE,
        );
        resp.clone()
            .on_hover_text("A still, as reduced motion is on: click to play it.");
    }
}

use egui::Color32;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_app_s_setting_overrides_the_system_either_way() {
        for system in [None, Some(false), Some(true)] {
            assert!(Follow::On.resolve(system));
            assert!(!Follow::Off.resolve(system));
        }
        assert!(Follow::System.resolve(Some(true)));
        assert!(!Follow::System.resolve(Some(false)));
        // A system that cannot be read asks for nothing.
        assert!(!Follow::System.resolve(None));
        // Saved as words, and the default follows the system.
        assert_eq!(serde_json::to_string(&Follow::Off).unwrap(), r#""off""#);
        assert_eq!(Follow::default(), Follow::System);
    }

    #[test]
    fn a_watch_reads_now_then_every_couple_of_seconds_and_sees_changes() {
        let sys = Fixed::new(Reading {
            reduce_motion: Some(true),
            high_contrast: None,
        });
        let mut w = Watch::new(Box::new(sys.clone()));
        assert!(w.poll(0.0), "the first poll reads");
        assert_eq!(w.reading.reduce_motion, Some(true));
        sys.set(Reading {
            reduce_motion: Some(false),
            high_contrast: Some(true),
        });
        assert!(!w.poll(READ_EVERY * 0.5), "not yet");
        assert_eq!(w.reading.reduce_motion, Some(true));
        assert!(w.poll(READ_EVERY * 1.01), "the change is seen");
        assert_eq!(w.reading.reduce_motion, Some(false));
        assert_eq!(w.reading.high_contrast, Some(true));
        assert!(!w.poll(READ_EVERY * 2.5), "nothing changed since");
    }

    #[test]
    fn reduced_motion_stops_egui_s_own_animations_and_tells_every_window() {
        let ctx = egui::Context::default();
        assert!(!motion_reduced(&ctx));
        assert!(ctx.style_of(egui::Theme::Dark).animation_time > 0.0);
        set_motion_reduced(&ctx, true);
        assert!(motion_reduced(&ctx));
        assert!(!animate_bars(&ctx));
        for t in [egui::Theme::Dark, egui::Theme::Light] {
            let s = ctx.style_of(t);
            assert_eq!(s.animation_time, 0.0, "{t:?}");
            assert_eq!(s.scroll_animation, egui::style::ScrollAnimation::none());
        }
        set_motion_reduced(&ctx, false);
        assert!(!motion_reduced(&ctx));
        for t in [egui::Theme::Dark, egui::Theme::Light] {
            let s = ctx.style_of(t);
            assert_eq!(s.animation_time, ANIMATION_TIME);
            assert_eq!(s.scroll_animation, egui::style::ScrollAnimation::default());
        }
    }

    /// A small animation is a still with reduced motion until it is clicked, and moves as before
    /// without it.
    #[test]
    fn small_animations_are_stills_until_one_is_chosen() {
        let ctx = egui::Context::default();
        let mut stills = Stills::default();
        let click = |ctx: &egui::Context, stills: &mut Stills, clicked: bool| -> f64 {
            let mut t = -1.0;
            let _ = ctx.run_ui(Default::default(), |ui| {
                let mut resp = ui.allocate_response(egui::vec2(10.0, 10.0), egui::Sense::click());
                // A click as egui reports it.
                if clicked {
                    resp.flags |= egui::response::Flags::FAKE_PRIMARY_CLICKED;
                }
                t = stills.time(&resp, "find", 3.5);
            });
            t
        };
        assert_eq!(
            click(&ctx, &mut stills, false),
            3.5,
            "moves without reduced motion"
        );
        set_motion_reduced(&ctx, true);
        assert_eq!(click(&ctx, &mut stills, false), 0.0, "a still");
        assert!(!stills.moving(&ctx, "find"));
        assert_eq!(click(&ctx, &mut stills, true), 3.5, "chosen: it plays");
        assert_eq!(click(&ctx, &mut stills, false), 3.5, "and keeps playing");
        assert!(stills.moving(&ctx, "find"));
        assert!(!stills.moving(&ctx, "another"), "only the one chosen");
        assert_eq!(
            click(&ctx, &mut stills, true),
            0.0,
            "a second click stills it"
        );
    }

    /// The spinner stands still with reduced motion: drawing it asks for no more frames.
    #[test]
    fn the_spinner_is_still_with_reduced_motion() {
        let ctx = egui::Context::default();
        let run = |ctx: &egui::Context| {
            let out = ctx.run_ui(Default::default(), |ui| {
                spinner(ui);
            });
            out.viewport_output
                .values()
                .any(|v| v.repaint_delay == std::time::Duration::ZERO)
        };
        // egui's first frame always asks for a second; run past it.
        run(&ctx);
        assert!(
            run(&ctx),
            "a moving spinner asks for the next frame at once"
        );
        set_motion_reduced(&ctx, true);
        run(&ctx);
        assert!(!run(&ctx), "a still one does not");
    }

    /// Reads this desktop's settings and prints them (by hand, on a machine with a desktop:
    /// `cargo test -p jetcarp-app --release a11y::tests::reads_this_desktop -- --ignored --nocapture`).
    #[test]
    #[ignore]
    fn reads_this_desktop() {
        let mut os = Os::new();
        std::thread::sleep(std::time::Duration::from_millis(500));
        let r = os.read();
        eprintln!("{r:?}");
        #[cfg(target_os = "linux")]
        {
            assert!(
                r.reduce_motion.is_some(),
                "GNOME's enable-animations should be readable here"
            );
            let p = linux::Portal::portal_only();
            eprintln!("through the portal: {p:?}");
            assert_eq!(p, r, "the portal answers, and agrees");
        }
    }
}
