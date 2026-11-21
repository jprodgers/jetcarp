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

/// Restores `saved`, then fits the window into its monitor's work area.
fn place(ctx: &egui::Context, w: &winit::window::Window, saved: Option<WindowPlace>) {
    let mut size = None;
    if let Some(p) = saved {
        if let Some([x, y]) = p.pos {
            w.set_outer_position(PhysicalPosition::new(x, y));
        }
        if p.size[0] > 0 && p.size[1] > 0 {
            size = Some((p.size[0], p.size[1]));
        }
    }
    let now = w.inner_size();
    let size = fit(w, size).filter(|&s| s != (now.width, now.height));
    if let Some(size) = size {
        let ppp = ctx.zoom_factor() * w.scale_factor() as f32;
        ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(in_points(size, ppp)));
    }
    if saved.is_some_and(|p| p.maximized) {
        w.set_maximized(true);
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

/// The inner size to give the window: `want` (or its size now), moved (and if need be shrunk)
/// into the work area of the monitor it is on. Moves it there itself.
fn fit(w: &winit::window::Window, want: Option<(u32, u32)>) -> Option<(u32, u32)> {
    // Wayland's compositors place windows themselves, and say neither where nor on which monitor.
    let Some(m) = w.current_monitor().or_else(|| w.primary_monitor()) else {
        return want;
    };
    let Ok(pos) = w.outer_position() else {
        return want;
    };
    let (mp, ms) = (m.position(), m.size());
    let area = workarea::work_area(Rect::new(mp.x, mp.y, ms.width, ms.height));
    let (outer_now, inner_now) = (w.outer_size(), w.inner_size());
    let inner = want.unwrap_or((inner_now.width, inner_now.height));
    // The frame around the inside stays the same when the inside changes size.
    let outer = (
        outer_now.width.saturating_sub(inner_now.width) + inner.0,
        outer_now.height.saturating_sub(inner_now.height) + inner.1,
    );
    let (to, size) = workarea::fit_window(area, (pos.x, pos.y), outer, inner);
    if to != (pos.x, pos.y) {
        w.set_outer_position(PhysicalPosition::new(to.0, to.1));
    }
    Some(size)
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

#[cfg(test)]
mod tests {
    use super::*;

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
