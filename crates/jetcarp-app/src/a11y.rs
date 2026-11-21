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
    /// The system's style is dark (`Some(false)`: light), where the app reads it itself: on Linux
    /// (#137). Elsewhere `None`, and egui follows the system through winit.
    pub dark: Option<bool>,
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
            dark: None,
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
                dark: None,
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
                dark: p.dark(),
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
            let dark = self.dark();
            if reduce_motion.is_some() || high_contrast.is_some() || dark.is_some() {
                self.answered = true;
            }
            let mut r = Reading {
                reduce_motion,
                high_contrast,
                dark,
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
                if r.dark.is_none() {
                    r.dark = gsettings_text("org.gnome.desktop.interface", "color-scheme")
                        .map(|v| dark_of_gnome(&v));
                }
            }
            r
        }

        /// The portal's `color-scheme`: 1 asks for dark, 2 for light, and 0 says nothing, which is
        /// what GNOME's *Light* style sets (its `color-scheme` is then `default`), so it is light.
        fn dark(&self) -> Option<bool> {
            self.u32_of("org.freedesktop.appearance", "color-scheme")
                .map(dark_of_portal)
        }
    }

    pub(super) fn dark_of_portal(v: u32) -> bool {
        v == 1
    }

    /// GNOME's own `color-scheme`: `'prefer-dark'`, `'prefer-light'` or `'default'` (light).
    pub(super) fn dark_of_gnome(v: &str) -> bool {
        v.trim().trim_matches('\'') == "prefer-dark"
    }

    fn gsettings_text(schema: &str, key: &str) -> Option<String> {
        let out = std::process::Command::new("gsettings")
            .args(["get", schema, key])
            .stdin(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .output()
            .ok()?;
        out.status
            .success()
            .then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
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

// ---------------------------------------------------------------------------------------------
// The keyboard and screen readers (#219)
//
// What any window needs, kept on the egui context so no window has to be handed the app:
// announcements for the status line, the regions F6 moves between, where the keyboard was last
// frame, the mouse-only widgets the keyboard passes over, the focus ring, and a last tidy of the
// accessibility tree before it goes to the screen reader. `app/keyboard.rs` drives them.

use egui::accesskit;
use std::collections::HashSet;

/// Name a control for screen readers where nothing beside it does (a list at the start of a
/// line, a box whose label is above it): egui's `accessible_name` for a response held by
/// reference, such as a combo box's.
pub fn name(r: &egui::Response, name: impl Into<String>) {
    let name = name.into();
    r.ctx.accesskit_node_builder(r.id, |n| n.set_label(name));
}

#[derive(Clone, Default)]
struct Pending(Vec<String>);

fn pending_id() -> egui::Id {
    egui::Id::unique("jetcarp-a11y-announce")
}

/// Tell a screen reader something that happened (a finished job, say): the app puts it in the
/// status line, which screen readers hear as a live region.
pub fn announce(ctx: &egui::Context, text: impl Into<String>) {
    let text = text.into();
    ctx.data_mut(|d| {
        d.get_temp_mut_or_default::<Pending>(pending_id())
            .0
            .push(text)
    });
}

/// What was announced since the last call.
pub fn take_announcements(ctx: &egui::Context) -> Vec<String> {
    ctx.data_mut(|d| std::mem::take(&mut d.get_temp_mut_or_default::<Pending>(pending_id()).0))
}

/// The status line's words, as a live region: a screen reader reads them out when they change,
/// at once when `urgent` (an error), else when it has finished what it is saying. The node has no
/// place on screen of its own, so it is there with the status bar hidden too.
pub fn live_region(ctx: &egui::Context, text: &str, urgent: bool) {
    ctx.accesskit_node_builder(egui::Id::unique("jetcarp-status-live"), |n| {
        n.set_role(accesskit::Role::Status);
        n.set_label(text.to_string());
        n.set_live(if urgent {
            accesskit::Live::Assertive
        } else {
            accesskit::Live::Polite
        });
    });
}

/// Where a region is in the order F6 takes: the menu bar, then the window's own parts (the panes
/// and the grid, or the Quick Start), the status line, then the floating windows.
pub const MENU_BAR: u8 = 0;
pub const MAIN: u8 = 1;
pub const STATUS: u8 = 2;
pub const WINDOW: u8 = 3;

/// A part of the window that F6 moves to: the keyboard goes to its first control in `land` (a
/// floating window's below its title bar, whose buttons Shift+Tab reaches from there).
#[derive(Clone, Debug, PartialEq)]
pub struct Region {
    pub name: String,
    pub rank: u8,
    pub layer: egui::LayerId,
    pub rect: egui::Rect,
    pub land: egui::Rect,
}

#[derive(Clone, Default)]
struct Regions {
    last: Vec<Region>,
    now: Vec<Region>,
}

fn regions_id() -> egui::Id {
    egui::Id::unique("jetcarp-a11y-regions")
}

/// Note a region drawn this frame.
pub fn region(ctx: &egui::Context, rank: u8, name: &str, layer: egui::LayerId, rect: egui::Rect) {
    note_region(ctx, rank, name, layer, rect, rect);
}

/// Note a floating window drawn this frame: F6 lands below its title bar.
pub fn window_region(ctx: &egui::Context, name: &str, layer: egui::LayerId, rect: egui::Rect) {
    let style = ctx.global_style();
    let title = ctx.fonts_mut(|f| f.row_height(&egui::TextStyle::Heading.resolve(&style)))
        + style.spacing.window_margin.sum().y;
    let mut land = rect;
    land.min.y = (rect.min.y + title).min(rect.max.y);
    note_region(ctx, WINDOW, name, layer, rect, land);
}

fn note_region(
    ctx: &egui::Context,
    rank: u8,
    name: &str,
    layer: egui::LayerId,
    rect: egui::Rect,
    land: egui::Rect,
) {
    if !rect.is_positive() {
        return;
    }
    ctx.data_mut(|d| {
        d.get_temp_mut_or_default::<Regions>(regions_id())
            .now
            .push(Region {
                name: name.to_string(),
                rank,
                layer,
                rect,
                land,
            })
    });
}

/// A new frame: last frame's regions are the ones F6 moves between.
pub fn regions_begin(ctx: &egui::Context) {
    ctx.data_mut(|d| {
        let r = d.get_temp_mut_or_default::<Regions>(regions_id());
        r.last = std::mem::take(&mut r.now);
    });
}

/// Last frame's regions, in F6's order.
pub fn last_regions(ctx: &egui::Context) -> Vec<Region> {
    let mut v = ctx.data(|d| {
        d.get_temp::<Regions>(regions_id())
            .map(|r| r.last)
            .unwrap_or_default()
    });
    v.sort_by_key(|r| r.rank);
    v
}

/// Where the keyboard was at the end of last frame.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct LastFocus {
    pub id: Option<egui::Id>,
    pub layer: Option<egui::LayerId>,
    /// It was a text field (Esc there only leaves the field).
    pub text: bool,
    /// A popup (a menu, a list) was open (Esc closes that first).
    pub popup: bool,
}

fn last_focus_id() -> egui::Id {
    egui::Id::unique("jetcarp-a11y-last-focus")
}

/// Remember where the keyboard is, at the end of a frame.
pub fn note_focus(ctx: &egui::Context) {
    let id = ctx.memory(|m| m.focused());
    let layer = id.and_then(|id| {
        ctx.viewport(|v| {
            v.this_pass
                .widgets
                .get(id)
                .or_else(|| v.prev_pass.widgets.get(id))
                .map(|w| w.layer_id)
        })
    });
    let f = LastFocus {
        id,
        layer,
        text: ctx.text_edit_focused(),
        popup: egui::Popup::is_any_open(ctx),
    };
    ctx.data_mut(|d| d.insert_temp(last_focus_id(), f));
}

pub fn last_focus(ctx: &egui::Context) -> LastFocus {
    ctx.data(|d| d.get_temp(last_focus_id()))
        .unwrap_or_default()
}

/// Esc was pressed with the keyboard on a control (not a text field, nor in a popup) in the
/// window on `layer`: the window should close.
pub fn escape_closes(ctx: &egui::Context, layer: egui::LayerId) -> bool {
    let f = last_focus(ctx);
    f.layer == Some(layer) && !f.text && !f.popup && ctx.input(|i| i.key_pressed(egui::Key::Escape))
}

/// Esc was pressed with the keyboard in the dialog on `layer`, or nowhere at all (not in a text
/// field or a popup): the dialog should close.
pub fn escape_closes_dialog(ctx: &egui::Context, layer: egui::LayerId) -> bool {
    let f = last_focus(ctx);
    (f.layer == Some(layer) || f.id.is_none())
        && !f.text
        && !f.popup
        && ctx.input(|i| i.key_pressed(egui::Key::Escape))
}

#[derive(Clone, Default)]
struct Chrome {
    /// Named this frame by the code that drew them.
    named: HashSet<egui::Id>,
    /// Found at the end of last frame.
    found: HashSet<egui::Id>,
}

fn chrome_id() -> egui::Id {
    egui::Id::unique("jetcarp-a11y-chrome")
}

/// Mark a widget the mouse uses and the keyboard has no use for (a window's title bar, which
/// takes double-clicks and drags): Tab and F6 pass over it, and screen readers are not shown it.
pub fn mouse_only(ctx: &egui::Context, id: egui::Id) {
    ctx.data_mut(|d| {
        d.get_temp_mut_or_default::<Chrome>(chrome_id())
            .named
            .insert(id)
    });
}

/// The mouse-only widgets drawn this frame: those marked so ([`mouse_only`]), and those egui and
/// its docking draw for dragging, which take the keyboard though they do nothing with it: a
/// dragged background behind other controls (a tab bar's), and the thin lines between docked
/// panes that resize them.
pub fn find_chrome(ctx: &egui::Context) -> HashSet<egui::Id> {
    let named = ctx
        .data_mut(|d| std::mem::take(&mut d.get_temp_mut_or_default::<Chrome>(chrome_id()).named));
    let mut found: HashSet<egui::Id> = named;
    ctx.viewport(|v| {
        for layer in v.this_pass.widgets.layer_ids() {
            let ws: Vec<&egui::WidgetRect> = v
                .this_pass
                .widgets
                .get_layer(layer)
                .filter(|w| w.sense.is_focusable() && w.enabled)
                .collect();
            for (i, w) in ws.iter().enumerate() {
                if !w.sense.senses_drag() {
                    continue;
                }
                let (wd, ht) = (w.rect.width(), w.rect.height());
                let thin = (wd <= 12.0 && ht >= 40.0) || (ht <= 12.0 && wd >= 40.0);
                let behind = ws[i + 1..]
                    .iter()
                    .any(|o| o.id != w.id && w.rect.contains_rect(o.rect));
                if thin || behind {
                    found.insert(w.id);
                }
            }
        }
    });
    ctx.data_mut(|d| d.get_temp_mut_or_default::<Chrome>(chrome_id()).found = found.clone());
    found
}

/// Mouse-only widgets found at the end of last frame.
pub fn chrome(ctx: &egui::Context) -> HashSet<egui::Id> {
    ctx.data(|d| {
        d.get_temp::<Chrome>(chrome_id())
            .map(|c| c.found)
            .unwrap_or_default()
    })
}

/// The widgets in `region` where F6 lands that take the keyboard, in the order they were drawn
/// last frame, the mouse-only ones left out; else any of the region's.
pub fn focusable_in(ctx: &egui::Context, region: &Region) -> Vec<egui::Id> {
    let land = focusable_within(ctx, region, region.land);
    if land.is_empty() && region.land != region.rect {
        return focusable_within(ctx, region, region.rect);
    }
    land
}

fn focusable_within(ctx: &egui::Context, region: &Region, rect: egui::Rect) -> Vec<egui::Id> {
    let skip = chrome(ctx);
    if !ctx.memory(|m| m.allows_interaction(region.layer)) {
        return vec![];
    }
    ctx.viewport(|v| {
        let mut seen = HashSet::new();
        v.prev_pass
            .widgets
            .get_layer(region.layer)
            .filter(|w| {
                w.sense.is_focusable()
                    && w.enabled
                    && w.visible
                    && !skip.contains(&w.id)
                    && w.interact_rect.is_positive()
                    && rect.contains(w.interact_rect.center())
            })
            .map(|w| w.id)
            .filter(|id| seen.insert(*id))
            .collect()
    })
}

/// The region holding widget `id` (last frame's), if any: where the part of it that shows is
/// (a tab scrolled out of its tab bar is not in the pane beside it).
pub fn region_of(ctx: &egui::Context, regions: &[Region], id: egui::Id) -> Option<usize> {
    let w = ctx.viewport(|v| v.prev_pass.widgets.get(id).copied())?;
    let at = if w.interact_rect.is_positive() {
        w.interact_rect.center()
    } else {
        w.rect.center()
    };
    regions
        .iter()
        .position(|r| r.layer == w.layer_id && r.rect.contains(at))
}

/// Scroll widget `id` into view this frame, if last frame showed only part of it: an accessibility
/// request egui answers inside the scroll area that holds it, when it draws it.
pub fn scroll_into_view(ctx: &egui::Context, id: egui::Id) {
    let Some(w) = ctx.viewport(|v| v.prev_pass.widgets.get(id).copied()) else {
        return;
    };
    if w.interact_rect.area() + 1.0 >= w.rect.area() {
        return;
    }
    ctx.input_mut(|i| {
        i.events.push(egui::Event::AccessKitActionRequest(
            accesskit::ActionRequest {
                action: accesskit::Action::ScrollIntoView,
                target_tree: accesskit::TreeId::ROOT,
                target_node: id.accesskit_id(),
                data: None,
            },
        ))
    });
}

/// The focus ring: the high-contrast theme's own colour and width (3 points of gold, or navy in
/// the light form), else 2 points of the selection's outline colour, which egui's light and dark
/// themes draw at 3:1 or more against their panels.
pub fn ring_stroke(visuals: &egui::Visuals, high: bool) -> egui::Stroke {
    if high {
        let s = if visuals.dark_mode {
            &crate::theme::DARK
        } else {
            &crate::theme::LIGHT
        };
        egui::Stroke::new(crate::theme::FOCUS_WIDTH, s.focus)
    } else {
        egui::Stroke::new(2.0, visuals.selection.stroke.color)
    }
}

/// Draw the ring round whatever has the keyboard (in its own layer, over it). egui shows the focus
/// on buttons by their outline alone, one point wide in its standard themes, and not at all on
/// many controls; this ring is the same on every control, in every theme.
pub fn paint_focus_ring(ctx: &egui::Context, skip: &HashSet<egui::Id>) {
    let Some(id) = ctx.memory(|m| m.focused()) else {
        return;
    };
    if skip.contains(&id) {
        return;
    }
    let Some(w) = ctx.viewport(|v| v.this_pass.widgets.get(id).copied()) else {
        return;
    };
    if !w.interact_rect.is_positive() || !w.visible {
        return;
    }
    let stroke = ring_stroke(&ctx.global_style().visuals, high_contrast(ctx));
    let big = w.rect.width() > 200.0 && w.rect.height() > 200.0;
    let (rect, kind) = if big {
        // A widget as big as the grid: inside its edge, where no neighbour covers the ring.
        (w.rect.shrink(1.0), egui::StrokeKind::Inside)
    } else {
        (w.rect.expand(1.0), egui::StrokeKind::Outside)
    };
    let clip = w.interact_rect.expand(stroke.width + 3.0);
    ctx.layer_painter(w.layer_id)
        .with_clip_rect(clip)
        .rect_stroke(rect, 3.0, stroke, kind);
}

/// A last tidy of this frame's accessibility tree (only while a screen reader, or a test, asks for
/// it): mouse-only widgets are shown as plain containers, and each control with no name of its own
/// (or one of symbols alone, such as "%") is named by the words just left of it on the same line,
/// a label's or a check box's, as a person reading the screen would name it ("Into" for the list
/// beside it, "Repeating up to" for the number after that box).
pub fn tidy_tree(ctx: &egui::Context, chrome: &HashSet<egui::Id>) {
    struct Seen {
        /// Controls to name: their layer and place.
        unnamed: Vec<(egui::Id, Option<egui::LayerId>, egui::Rect)>,
        /// Words that can name them: a label's or a check box's, and whether they only join
        /// others ("for", "to").
        labels: Vec<(egui::Id, Option<egui::LayerId>, egui::Rect, bool)>,
        chrome: Vec<egui::Id>,
    }
    let rect_of = |b: accesskit::Rect| {
        egui::Rect::from_min_max(
            egui::pos2(b.x0 as f32, b.y0 as f32),
            egui::pos2(b.x1 as f32, b.y1 as f32),
        )
    };
    let Some(seen) = ctx.viewport(|v| {
        let st = v.this_pass.accesskit_state.as_ref()?;
        let mut s = Seen {
            unnamed: vec![],
            labels: vec![],
            chrome: vec![],
        };
        for (id, n) in &st.nodes {
            if chrome.contains(id) {
                s.chrome.push(*id);
                continue;
            }
            let Some(b) = n.bounds() else { continue };
            let r = rect_of(b);
            let layer = v.this_pass.widgets.get(*id).map(|w| w.layer_id);
            let role = n.role();
            let says = |t: &str| t.chars().any(char::is_alphanumeric);
            let words = match role {
                accesskit::Role::Label => n.value(),
                accesskit::Role::CheckBox => n.label(),
                _ => None,
            };
            const JOINING: [&str; 16] = [
                "for", "to", "from", "on", "at", "of", "by", "and", "or", "in", "into", "with",
                "than", "px", "gen", "gens",
            ];
            let joins = |w: &str| JOINING.contains(&w.trim().to_lowercase().as_str());
            if let Some(w) = words.filter(|w| says(w)) {
                s.labels.push((*id, layer, r, joins(w)));
            }
            // Named by nothing, by symbols alone or a unit ("%", "px": a number box is named by its
            // suffix), only by its tooltip (egui names a control by its hover text when it has no
            // name), or only by the grey hint in its box: the words beside it are the better name;
            // a tooltip stays its description, a hint its hint.
            let tooltip_only = n.label().is_some() && n.label() == n.description();
            if egui::accessibility::is_input(role)
                && n.labelled_by().is_empty()
                && (tooltip_only || n.label().is_none_or(|l| !says(l) || joins(l)))
            {
                s.unnamed.push((*id, layer, r));
            }
        }
        Some(s)
    }) else {
        return;
    };
    for id in seen.chrome {
        ctx.accesskit_node_builder(id, |n| {
            if n.role() == accesskit::Role::Unknown && n.label().is_none() {
                n.set_role(accesskit::Role::GenericContainer);
                n.remove_action(accesskit::Action::Focus);
                n.remove_action(accesskit::Action::Click);
            }
        });
    }
    for (id, layer, r) in seen.unnamed {
        // The words on the same line to its left, in the same window.
        let row: Vec<&(egui::Id, Option<egui::LayerId>, egui::Rect, bool)> = seen
            .labels
            .iter()
            .filter(|(l_id, l_layer, l, _)| {
                let overlap = (l.max.y.min(r.max.y) - l.min.y.max(r.min.y)).max(0.0);
                *l_id != id
                    && *l_layer == layer
                    && overlap >= 0.5 * l.height().min(r.height())
                    && l.max.x <= r.min.x + 2.0
                    && r.min.x - l.max.x <= 260.0
            })
            .collect();
        let Some(near) = row
            .iter()
            .min_by(|a, b| (r.min.x - a.2.max.x).total_cmp(&(r.min.x - b.2.max.x)))
        else {
            continue;
        };
        // A connecting word ("for", "to", "px") is read after the line's first words: "Activity
        // below, for".
        let mut by = vec![near.0];
        if near.3
            && let Some(first) = row
                .iter()
                .min_by(|a, b| a.2.min.x.total_cmp(&b.2.min.x))
                .filter(|f| f.0 != near.0)
        {
            by.insert(0, first.0);
        }
        ctx.accesskit_node_builder(id, |n| {
            n.clear_label();
            for l in &by {
                n.push_labelled_by(l.accesskit_id());
            }
        });
    }
}

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
            dark: None,
        });
        let mut w = Watch::new(Box::new(sys.clone()));
        assert!(w.poll(0.0), "the first poll reads");
        assert_eq!(w.reading.reduce_motion, Some(true));
        sys.set(Reading {
            reduce_motion: Some(false),
            high_contrast: Some(true),
            dark: None,
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

    /// The portal's and GNOME's own words for the style (#137).
    #[cfg(target_os = "linux")]
    #[test]
    fn the_style_s_words_on_linux() {
        use super::linux::{dark_of_gnome, dark_of_portal};
        assert!(dark_of_portal(1));
        assert!(!dark_of_portal(2));
        assert!(!dark_of_portal(0), "GNOME's Light style is 'no preference'");
        assert!(dark_of_gnome("'prefer-dark'"));
        assert!(!dark_of_gnome("'prefer-light'"));
        assert!(!dark_of_gnome("'default'"));
    }

    /// The tidy pass names a control with no name of its own by the words left of it on its
    /// line: a list by its label, a number after a joining word by the line's first words and
    /// that word, a number named only by its unit, a list named only by its tooltip (which stays
    /// its description); a mouse-only widget is hidden; a named button keeps its name.
    #[test]
    fn a_control_with_no_name_takes_the_words_beside_it() {
        use egui::accesskit::{NodeId, Role};
        let ctx = egui::Context::default();
        ctx.enable_accesskit();
        let mut ids = vec![];
        let (mut a, mut b) = (3.0, 5.0);
        let mut drag = egui::Id::NULL;
        let mut out = ctx.run_ui(Default::default(), |ui| {
            ui.horizontal(|ui| {
                ids.push(ui.label("Into").id);
                let c = egui::ComboBox::from_id_salt("into")
                    .selected_text("Life")
                    .show_ui(ui, |_| {});
                ids.push(c.response.id);
                ids.push(ui.label("for").id);
                ids.push(ui.add(egui::DragValue::new(&mut a).suffix(" gens")).id);
            });
            ui.horizontal(|ui| {
                ids.push(ui.label("Cell size").id);
                ids.push(ui.add(egui::DragValue::new(&mut b).suffix(" px")).id);
                let c = egui::ComboBox::from_id_salt("fps")
                    .selected_text("30")
                    .show_ui(ui, |_| {});
                ids.push(c.response.on_hover_text("Frames a second").id);
                ids.push(ui.button("Seed").id);
            });
            drag = ui
                .interact(
                    egui::Rect::from_min_size(egui::pos2(0.0, 200.0), egui::vec2(6.0, 120.0)),
                    egui::Id::new("splitter"),
                    egui::Sense::click_and_drag(),
                )
                .id;
            let chrome = find_chrome(ui.ctx());
            assert!(chrome.contains(&drag), "a thin dragged line is mouse-only");
            tidy_tree(ui.ctx(), &chrome);
        });
        out.textures_delta.clear();
        let u = out.platform_output.accesskit_update.expect("a tree");
        let node = |id: egui::Id| {
            u.nodes
                .iter()
                .find(|(n, _)| *n == id.accesskit_id())
                .map(|(_, n)| n.clone())
                .expect("a node")
        };
        let by = |id: egui::Id| -> Vec<NodeId> { node(id).labelled_by().to_vec() };
        let [into, list, for_, gens, size, px, fps, seed] = ids[..] else {
            panic!("{ids:?}")
        };
        assert_eq!(by(list), vec![into.accesskit_id()]);
        assert_eq!(by(gens), vec![into.accesskit_id(), for_.accesskit_id()]);
        assert_eq!(by(px), vec![size.accesskit_id()], "a unit is no name");
        assert_eq!(node(px).label(), None);
        assert_eq!(
            by(fps),
            vec![size.accesskit_id()],
            "the line's words over a tooltip"
        );
        assert_eq!(node(fps).description(), Some("Frames a second"));
        assert!(by(seed).is_empty() && node(seed).label() == Some("Seed"));
        let d = node(drag);
        assert_eq!(d.role(), Role::GenericContainer);
        assert!(!d.supports_action(egui::accesskit::Action::Focus));
    }
}
