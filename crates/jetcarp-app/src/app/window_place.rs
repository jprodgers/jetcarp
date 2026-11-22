//! The main window's place: where it was last time (size, position, maximised), saved in the
//! settings, and kept inside the work area of its monitor (not under the Windows taskbar, the
//! macOS menu bar or the Dock).
//!
//! eframe shows the window only after the first frame has been drawn, so placing it in the first
//! frame means it never shows anywhere else. The first time (on the Acer it opened at (189, 182),
//! 1402x932, its bottom under the taskbar), and whenever the saved place would leave part of it
//! off every monitor (a monitor unplugged, a resolution lowered, a place saved partly off screen:
//! #306), it is moved whole onto the nearest monitor's work area, or the main monitor's.
//!
//! Under Wayland the compositor places windows (GNOME's Mutter, KDE's KWin): an app can neither
//! learn its window's place nor choose it, so none is saved or asked, and only the size comes
//! back.

use super::*;
use jetcarp_display::workarea::{self, Rect};
use serde::{Deserialize, Serialize};
use winit::dpi::PhysicalPosition;

/// Where the main window was, in physical pixels of the desktop.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub(super) struct WindowPlace {
    /// The outer top-left corner (None where the system does not say, as on Wayland).
    pub pos: Option<[i32; 2]>,
    /// The inside's size, when last not maximised.
    pub size: [u32; 2],
    pub maximized: bool,
}

/// Whether this run places the window itself: not when a size is forced for screenshot checks.
fn placing() -> bool {
    std::env::var_os("JETCARP_WINDOW").is_none() && std::env::var_os("JETCARP_SCREENSHOT").is_none()
}

impl JetcarpApp {
    /// Called every frame: in the first, puts the window in place; after that, notes where it is.
    pub(super) fn window_place(&mut self, ctx: &egui::Context, frame: &eframe::Frame) {
        let Some(w) = frame.winit_window() else {
            return;
        };
        if !self.shell.window_placed {
            self.shell.window_placed = true;
            self.shell.window_placing = placing();
            if self.shell.window_placing {
                place(ctx, w, self.shell.window);
            }
            return;
        }
        if self.shell.window_placing {
            self.shell.window = note(w, self.shell.window);
        }
    }
}

/// Restores `saved`, kept on the monitors' work areas ([`restore`]).
fn place(ctx: &egui::Context, w: &winit::window::Window, saved: Option<WindowPlace>) {
    let handles: Vec<winit::monitor::MonitorHandle> = w.available_monitors().collect();
    let primary = w.primary_monitor();
    let current = w.current_monitor();
    let monitors: Vec<Monitor> = handles
        .iter()
        .map(|m| {
            let (p, s) = (m.position(), m.size());
            Monitor {
                area: workarea::work_area(Rect::new(p.x, p.y, s.width, s.height)),
                primary: primary.as_ref() == Some(m),
            }
        })
        .collect();
    let (inner, outer) = (w.inner_size(), w.outer_size());
    let now = w.outer_position().ok().map(|p| (p.x, p.y));
    let win = Opening {
        pos: now,
        inner: (inner.width, inner.height),
        frame: (
            outer.width.saturating_sub(inner.width),
            outer.height.saturating_sub(inner.height),
        ),
        on: current.and_then(|c| handles.iter().position(|m| *m == c)),
    };
    let r = restore(saved, win, &monitors, wayland(w));
    if let Some((x, y)) = r.pos
        && r.pos != now
    {
        w.set_outer_position(PhysicalPosition::new(x, y));
    }
    if r.inner != win.inner {
        let ppp = ctx.zoom_factor() * w.scale_factor() as f32;
        ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(in_points(r.inner, ppp)));
    }
    if saved.is_some_and(|p| p.maximized) {
        w.set_maximized(true);
    }
}

/// Whether the window is a Wayland one: there the compositor places windows, and an app's request
/// for a place is not honoured (winit's is a no-op), so none is made.
fn wayland(w: &winit::window::Window) -> bool {
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
        if let Ok(h) = w.window_handle() {
            return matches!(h.as_raw(), RawWindowHandle::Wayland(_));
        }
        w.outer_position().is_err()
    }
    #[cfg(not(all(unix, not(target_os = "macos"))))]
    {
        let _ = w;
        false
    }
}

/// A size in physical pixels as egui's points, for `ViewportCommand::InnerSize`.
///
/// The size goes through egui, not straight to winit (#140). On Wayland, winit applies a size
/// request at once and sends no `Resized` event, so a direct `request_inner_size` left eframe's
/// drawing surface at the old size: on GNOME the app was drawn squeezed into part of its window,
/// or scaled up and grainy with clicks landing away from the pointer, until it was resized by
/// hand. egui-winit and eframe handle exactly that case when the size comes as a viewport command.
fn in_points(size: (u32, u32), pixels_per_point: f32) -> egui::Vec2 {
    egui::vec2(
        size.0 as f32 / pixels_per_point,
        size.1 as f32 / pixels_per_point,
    )
}

/// Where the window is now, keeping the last size it had when not maximised (what it goes back
/// to). Full screen and minimised windows are not noted.
fn note(w: &winit::window::Window, last: Option<WindowPlace>) -> Option<WindowPlace> {
    if w.fullscreen().is_some() || w.is_minimized() == Some(true) {
        return last;
    }
    let maximized = w.is_maximized();
    let mut p = last.unwrap_or_default();
    p.maximized = maximized;
    if !maximized {
        let s = w.inner_size();
        if s.width > 0 && s.height > 0 {
            p.size = [s.width, s.height];
        }
        p.pos = w.outer_position().ok().map(|q| [q.x, q.y]);
    }
    if p.size == [0, 0] {
        return last;
    }
    Some(p)
}

/// A monitor, for placing the window: its work area, in physical pixels of the desktop.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Monitor {
    area: Rect,
    /// The main monitor (Windows' primary; macOS's, with the menu bar).
    primary: bool,
}

/// The window as it opens, before it is placed.
#[derive(Clone, Copy, Debug)]
struct Opening {
    /// Its outer top-left corner (None where the system does not say, as on Wayland).
    pos: Option<(i32, i32)>,
    /// Its inside's size.
    inner: (u32, u32),
    /// The frame around the inside: the outer size less the inner one.
    frame: (u32, u32),
    /// Which of the monitors it is on, where the system says.
    on: Option<usize>,
}

/// What to ask of the window as it opens.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Restore {
    /// Its outer top-left corner (None: no place is asked).
    pos: Option<(i32, i32)>,
    /// Its inside's size.
    inner: (u32, u32),
}

/// Where the window goes and how large: what was `saved` (or, the first time, where the system
/// opened it), kept on `monitors` (#306).
///
/// A place that would leave any part of the window off every monitor's work area is moved so the
/// whole window is on the nearest monitor: the one it overlaps most, or the main monitor when it
/// overlaps none (the monitor it was on is gone). Its size shrinks only when it cannot fit there
/// at all. Under Wayland the compositor places windows and an app cannot, so no place is asked:
/// only the size, shrunk to the monitor it is on when that is too small for it.
fn restore(
    saved: Option<WindowPlace>,
    win: Opening,
    monitors: &[Monitor],
    wayland: bool,
) -> Restore {
    let inner = saved
        .map(|p| (p.size[0], p.size[1]))
        .filter(|s| s.0 > 0 && s.1 > 0)
        .unwrap_or(win.inner);
    let outer = (inner.0 + win.frame.0, inner.1 + win.frame.1);
    let pos = if wayland {
        None
    } else {
        saved.and_then(|p| p.pos).map(|[x, y]| (x, y)).or(win.pos)
    };
    let Some(pos) = pos else {
        // No place to keep on screen: the size alone, fitted to the monitor it is on.
        let inner = match win.on.and_then(|i| monitors.get(i)) {
            Some(m) => workarea::fit_window(m.area, (m.area.x, m.area.y), outer, inner).1,
            None => inner,
        };
        return Restore { pos: None, inner };
    };
    let rect = Rect::new(pos.0, pos.1, outer.0, outer.1);
    if on_screen(&rect, monitors) {
        return Restore {
            pos: Some(pos),
            inner,
        };
    }
    let nearest = monitors
        .iter()
        .map(|m| (m.area.overlap(&rect), m))
        .filter(|(o, _)| *o > 0)
        .max_by_key(|(o, _)| *o)
        .map(|(_, m)| m)
        .or_else(|| monitors.iter().find(|m| m.primary))
        .or(monitors.first());
    let Some(m) = nearest else {
        return Restore {
            pos: Some(pos),
            inner,
        };
    };
    let (to, inner) = workarea::fit_window(m.area, pos, outer, inner);
    Restore {
        pos: Some(to),
        inner,
    }
}

/// Whether every part of `rect` is on some monitor's work area (a window across two monitors
/// side by side is).
fn on_screen(rect: &Rect, monitors: &[Monitor]) -> bool {
    let mut areas: Vec<Rect> = monitors.iter().map(|m| m.area).collect();
    // Mirrored monitors show the same place once.
    areas.sort_by_key(|a| (a.x, a.y, a.w, a.h));
    areas.dedup();
    let covered: u64 = areas.iter().map(|a| a.overlap(rect)).sum();
    covered >= rect.w as u64 * rect.h as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    fn saved(pos: Option<(i32, i32)>, size: (u32, u32)) -> Option<WindowPlace> {
        Some(WindowPlace {
            pos: pos.map(|(x, y)| [x, y]),
            size: [size.0, size.1],
            maximized: false,
        })
    }

    /// A window as winit opens it: at (100, 100), 1400 x 900 inside, a frame of 2 by 32 pixels.
    fn opening(on: Option<usize>) -> Opening {
        Opening {
            pos: Some((100, 100)),
            inner: (1400, 900),
            frame: (2, 32),
            on,
        }
    }

    fn monitor(x: i32, y: i32, w: u32, h: u32, primary: bool) -> Monitor {
        Monitor {
            area: Rect::new(x, y, w, h),
            primary,
        }
    }

    /// One monitor, as on the Framework (2880 x 1920 in pixels).
    fn one() -> Vec<Monitor> {
        vec![monitor(0, 0, 2880, 1920, true)]
    }

    /// The window outer rectangle a restore gives.
    fn outer(r: Restore) -> Rect {
        let (x, y) = r.pos.expect("a place");
        Rect::new(x, y, r.inner.0 + 2, r.inner.1 + 32)
    }

    fn inside(area: Rect, r: Rect) -> bool {
        r.x >= area.x && r.y >= area.y && r.right() <= area.right() && r.bottom() <= area.bottom()
    }

    /// #306: saved partly off the screen to the left, it comes back whole on it, its size kept.
    #[test]
    fn a_saved_place_off_to_the_left_comes_back_fully_on_screen() {
        let m = one();
        let r = restore(
            saved(Some((-600, 300)), (1600, 1000)),
            opening(Some(0)),
            &m,
            false,
        );
        assert_eq!(r.inner, (1600, 1000), "the size is kept: it fits");
        assert_eq!(r.pos, Some((0, 300)), "moved in from the left edge only");
        assert!(inside(m[0].area, outer(r)), "{r:?}");
        // Off the top, the right and the bottom too.
        for at in [(500, -200), (2000, 300), (500, 1500), (-5000, -5000)] {
            let r = restore(saved(Some(at), (1600, 1000)), opening(Some(0)), &m, false);
            assert!(inside(m[0].area, outer(r)), "{at:?}: {r:?}");
            assert_eq!(r.inner, (1600, 1000));
        }
    }

    /// A place that fits already is left as it is.
    #[test]
    fn a_saved_place_on_screen_is_kept() {
        let m = one();
        let r = restore(
            saved(Some((300, 200)), (1600, 1000)),
            opening(Some(0)),
            &m,
            false,
        );
        assert_eq!(
            r,
            Restore {
                pos: Some((300, 200)),
                inner: (1600, 1000)
            }
        );
        // Across two monitors side by side, every part on one of them: kept too.
        let two = vec![
            monitor(0, 0, 1920, 1080, true),
            monitor(1920, 0, 1920, 1080, false),
        ];
        let r = restore(
            saved(Some((1500, 100)), (1200, 800)),
            opening(Some(0)),
            &two,
            false,
        );
        assert_eq!(
            r,
            Restore {
                pos: Some((1500, 100)),
                inner: (1200, 800)
            }
        );
    }

    /// A monitor unplugged since (Windows, macOS, X11): the window comes back on the main one.
    #[test]
    fn a_saved_place_on_a_monitor_that_is_gone_comes_back_on_the_main_monitor() {
        // It was on a second monitor to the right; only the laptop's own (a taskbar at the
        // bottom) is left. The main monitor is listed second, as winit may.
        let m = vec![
            monitor(-1920, 0, 1920, 1080, false),
            monitor(0, 0, 1920, 1032, true),
        ];
        let r = restore(
            saved(Some((2400, 200)), (1400, 900)),
            opening(None),
            &m,
            false,
        );
        assert!(inside(m[1].area, outer(r)), "on the main monitor: {r:?}");
        assert_eq!(r.inner, (1400, 900), "the size is kept: it fits");
        // A monitor gone above, where nothing overlaps either.
        let r = restore(
            saved(Some((100, -1500)), (1400, 900)),
            opening(None),
            &m,
            false,
        );
        assert!(inside(m[1].area, outer(r)), "{r:?}");
    }

    /// Partly on a second monitor: whole on that one, the nearest.
    #[test]
    fn a_window_partly_on_a_monitor_goes_whole_onto_it() {
        let m = vec![
            monitor(0, 0, 1920, 1080, true),
            monitor(1920, 0, 2560, 1440, false),
        ];
        let r = restore(
            saved(Some((3800, 600)), (1400, 900)),
            opening(Some(0)),
            &m,
            false,
        );
        assert!(inside(m[1].area, outer(r)), "{r:?}");
        assert_eq!(r.inner, (1400, 900));
    }

    /// A resolution lowered since: the size shrinks only as far as it must, and it is on screen.
    #[test]
    fn a_window_too_large_for_its_monitor_now_is_shrunk_to_fit() {
        let m = vec![monitor(0, 0, 1366, 728, true)];
        let r = restore(
            saved(Some((200, 100)), (1600, 1000)),
            opening(Some(0)),
            &m,
            false,
        );
        assert_eq!(
            r,
            Restore {
                pos: Some((0, 0)),
                inner: (1364, 696)
            }
        );
        // Too wide only: the height is kept.
        let r = restore(
            saved(Some((200, 100)), (1600, 600)),
            opening(Some(0)),
            &m,
            false,
        );
        assert_eq!(r.inner, (1364, 600));
        assert!(inside(m[0].area, outer(r)), "{r:?}");
    }

    /// On Wayland the compositor places windows: no place is asked, whatever was saved; the size
    /// comes back.
    #[test]
    fn under_wayland_no_place_is_asked() {
        let m = one();
        let way = Opening {
            pos: None,
            ..opening(Some(0))
        };
        for at in [Some((-600, 300)), Some((300, 200)), None] {
            let r = restore(saved(at, (1600, 1000)), way, &m, true);
            assert_eq!(
                r,
                Restore {
                    pos: None,
                    inner: (1600, 1000)
                },
                "{at:?}"
            );
        }
        // Before the compositor says which monitor it is on, too.
        let r = restore(
            saved(Some((-600, 300)), (1600, 1000)),
            Opening { on: None, ..way },
            &m,
            true,
        );
        assert_eq!(
            r,
            Restore {
                pos: None,
                inner: (1600, 1000)
            }
        );
        // Larger than the monitor it is on: shrunk to fit.
        let r = restore(saved(None, (4000, 3000)), way, &m, true);
        assert_eq!(
            r,
            Restore {
                pos: None,
                inner: (2878, 1888)
            }
        );
    }

    /// The first time, nothing saved: where the system opened it, kept in the work area (the
    /// Acer opened it with its bottom under the taskbar).
    #[test]
    fn the_first_time_the_window_is_kept_in_the_work_area() {
        let m = vec![monitor(0, 0, 1920, 1032, true)];
        let win = Opening {
            pos: Some((189, 182)),
            ..opening(Some(0))
        };
        let r = restore(None, win, &m, false);
        assert_eq!(
            r,
            Restore {
                pos: Some((189, 100)),
                inner: (1400, 900)
            }
        );
    }

    /// egui-winit turns the points back into pixels (times the same scale) and winit rounds them:
    /// every saved size must come back to the pixel.
    #[test]
    fn a_saved_size_comes_back_to_the_pixel() {
        for ppp in [1.0f32, 1.25, 1.5, 1.75, 2.0, 2.25, 2.5, 3.0, 4.0] {
            for w in (800..=7680).step_by(7) {
                let h = w * 9 / 16 + 1;
                let p = in_points((w, h), ppp);
                let back = ((p.x * ppp).round() as u32, (p.y * ppp).round() as u32);
                assert_eq!(back, (w, h), "{w}x{h} at {ppp}");
            }
        }
    }
}
