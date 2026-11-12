//! The main window's place: where it was last time (size, position, maximised), saved in the
//! settings, and kept inside the work area of its monitor (not under the Windows taskbar, the
//! macOS menu bar or the Dock).
//!
//! eframe shows the window only after the first frame has been drawn, so placing it in the first
//! frame means it never shows anywhere else. The first time (on the Acer it opened at (189, 182),
//! 1402x932, its bottom under the taskbar), and when the monitor it was on has gone, it is moved
//! into the work area of the monitor it is on (or the nearest one, or the primary).

use super::*;
use jetcarp_display::workarea::{self, Rect};
use serde::{Deserialize, Serialize};
use winit::dpi::{PhysicalPosition, PhysicalSize};

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
    pub(super) fn window_place(&mut self, frame: &eframe::Frame) {
        let Some(w) = frame.winit_window() else {
            return;
        };
        if !self.shell.window_placed {
            self.shell.window_placed = true;
            self.shell.window_placing = placing();
            if self.shell.window_placing {
                place(w, self.shell.window);
            }
            return;
        }
        if self.shell.window_placing {
            self.shell.window = note(w, self.shell.window);
        }
    }
}

/// Restores `saved`, then fits the window into its monitor's work area.
fn place(w: &winit::window::Window, saved: Option<WindowPlace>) {
    if let Some(p) = saved {
        if let Some([x, y]) = p.pos {
            w.set_outer_position(PhysicalPosition::new(x, y));
        }
        if p.size[0] > 0 && p.size[1] > 0 {
            let _ = w.request_inner_size(PhysicalSize::new(p.size[0], p.size[1]));
        }
    }
    fit(w);
    if saved.is_some_and(|p| p.maximized) {
        w.set_maximized(true);
    }
}

/// Moves (and if need be shrinks) the window into the work area of the monitor it is on.
fn fit(w: &winit::window::Window) {
    // Wayland's compositors place windows themselves, and say neither where nor on which monitor.
    let Some(m) = w.current_monitor().or_else(|| w.primary_monitor()) else {
        return;
    };
    let Ok(pos) = w.outer_position() else {
        return;
    };
    let (mp, ms) = (m.position(), m.size());
    let area = workarea::work_area(Rect::new(mp.x, mp.y, ms.width, ms.height));
    let (outer, inner) = (w.outer_size(), w.inner_size());
    let (to, size) = workarea::fit_window(
        area,
        (pos.x, pos.y),
        (outer.width, outer.height),
        (inner.width, inner.height),
    );
    if size != (inner.width, inner.height) {
        let _ = w.request_inner_size(PhysicalSize::new(size.0, size.1));
    }
    if to != (pos.x, pos.y) {
        w.set_outer_position(PhysicalPosition::new(to.0, to.1));
    }
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
