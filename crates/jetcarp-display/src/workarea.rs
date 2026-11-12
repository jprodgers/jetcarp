//! Monitors' rectangles, and the part of each that windows may use (its work area): without the
//! taskbar on Windows, without the menu bar and the Dock on macOS. The app keeps its window inside
//! it, and the host keeps a wallpaper's caption inside it.
//!
//! Rectangles are in physical pixels of the desktop, as winit gives a monitor's position and size.
//! Linux has no one way to ask (GNOME's top bar sits over the wallpaper's top edge), so there the
//! work area is the whole monitor.

/// A rectangle in physical pixels of the desktop.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: u32,
    pub h: u32,
}

impl Rect {
    pub fn new(x: i32, y: i32, w: u32, h: u32) -> Rect {
        Rect { x, y, w, h }
    }

    pub fn right(&self) -> i64 {
        self.x as i64 + self.w as i64
    }

    pub fn bottom(&self) -> i64 {
        self.y as i64 + self.h as i64
    }

    /// The area this and `other` share, in square pixels.
    pub fn overlap(&self, other: &Rect) -> u64 {
        let w = (self.right().min(other.right()) - (self.x.max(other.x) as i64)).max(0);
        let h = (self.bottom().min(other.bottom()) - (self.y.max(other.y) as i64)).max(0);
        (w * h) as u64
    }

    /// How far `inner` lies inside this rectangle at each edge: left, top, right, bottom (zero
    /// where it reaches or passes the edge).
    pub fn insets_of(&self, inner: &Rect) -> [u32; 4] {
        let d = |v: i64| v.max(0) as u32;
        [
            d(inner.x as i64 - self.x as i64),
            d(inner.y as i64 - self.y as i64),
            d(self.right() - inner.right()),
            d(self.bottom() - inner.bottom()),
        ]
    }
}

/// The work area of the monitor whose bounds are `monitor`: the whole monitor where the system
/// does not say.
pub fn work_area(monitor: Rect) -> Rect {
    imp::work_area(monitor)
        .filter(|a| a.w > 0 && a.h > 0)
        .unwrap_or(monitor)
}

/// Where a window of outer size `outer` (frame included) at `pos` goes so it lies inside `area`,
/// and the inner size it then has, from its inner size now (`inner`). A window too large for the
/// area is shrunk to fit, keeping its frame; then it is moved in from any edge it passes.
pub fn fit_window(
    area: Rect,
    pos: (i32, i32),
    outer: (u32, u32),
    inner: (u32, u32),
) -> ((i32, i32), (u32, u32)) {
    let frame = (outer.0.saturating_sub(inner.0), outer.1.saturating_sub(inner.1));
    let ow = outer.0.min(area.w);
    let oh = outer.1.min(area.h);
    let inner = (
        inner.0.min(ow.saturating_sub(frame.0)).max(1),
        inner.1.min(oh.saturating_sub(frame.1)).max(1),
    );
    let clamp = |p: i32, len: u32, start: i32, span: u32| -> i32 {
        let max = start as i64 + span as i64 - len as i64;
        (p as i64).clamp(start as i64, max.max(start as i64)) as i32
    };
    (
        (clamp(pos.0, ow, area.x, area.w), clamp(pos.1, oh, area.y, area.h)),
        inner,
    )
}

#[cfg(windows)]
mod imp {
    use super::Rect;
    use windows_sys::Win32::Foundation::RECT;
    use windows_sys::Win32::Graphics::Gdi::{
        GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromRect,
    };

    pub fn work_area(m: Rect) -> Option<Rect> {
        let r = RECT {
            left: m.x,
            top: m.y,
            right: m.right() as i32,
            bottom: m.bottom() as i32,
        };
        // Physical pixels: winit makes the process aware of each monitor's scaling.
        let monitor = unsafe { MonitorFromRect(&r, MONITOR_DEFAULTTONEAREST) };
        let mut info: MONITORINFO = unsafe { std::mem::zeroed() };
        info.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
        if unsafe { GetMonitorInfoW(monitor, &mut info) } == 0 {
            return None;
        }
        let w = info.rcWork;
        Some(Rect::new(
            w.left,
            w.top,
            (w.right - w.left).max(0) as u32,
            (w.bottom - w.top).max(0) as u32,
        ))
    }
}

#[cfg(target_os = "macos")]
mod imp {
    use super::Rect;

    pub fn work_area(m: Rect) -> Option<Rect> {
        let s = super::mac::screen_for(m)?;
        // AppKit's y runs up: the menu bar is above the visible frame's top, the Dock below its
        // bottom (or beside it).
        let (f, v, k) = (s.frame, s.visible, s.scale);
        let left = ((v.origin.x - f.origin.x) * k).round().max(0.0) as u32;
        let right = ((f.origin.x + f.size.w - v.origin.x - v.size.w) * k)
            .round()
            .max(0.0) as u32;
        let bottom = ((v.origin.y - f.origin.y) * k).round().max(0.0) as u32;
        let top = ((f.origin.y + f.size.h - v.origin.y - v.size.h) * k)
            .round()
            .max(0.0) as u32;
        Some(Rect::new(
            m.x + left as i32,
            m.y + top as i32,
            m.w.saturating_sub(left + right),
            m.h.saturating_sub(top + bottom),
        ))
    }
}

#[cfg(not(any(windows, target_os = "macos")))]
mod imp {
    use super::Rect;

    pub fn work_area(_: Rect) -> Option<Rect> {
        None
    }
}

/// AppKit's screens, matched to the monitors winit reports.
#[cfg(target_os = "macos")]
pub(crate) mod mac {
    use super::Rect;
    use objc2::encode::{Encode, Encoding};
    use objc2::rc::Retained;
    use objc2::runtime::{AnyClass, AnyObject};
    use objc2::msg_send;

    // Makes sure AppKit is loaded, whatever program this is.
    #[link(name = "AppKit", kind = "framework")]
    unsafe extern "C" {}

    #[repr(C)]
    #[derive(Clone, Copy, Debug)]
    pub struct Point {
        pub x: f64,
        pub y: f64,
    }

    #[repr(C)]
    #[derive(Clone, Copy, Debug)]
    pub struct Size {
        pub w: f64,
        pub h: f64,
    }

    /// `NSRect` (a `CGRect`): points, with y running up from the bottom of the main screen.
    #[repr(C)]
    #[derive(Clone, Copy, Debug)]
    pub struct NsRect {
        pub origin: Point,
        pub size: Size,
    }

    unsafe impl Encode for Point {
        const ENCODING: Encoding = Encoding::Struct("CGPoint", &[f64::ENCODING, f64::ENCODING]);
    }
    unsafe impl Encode for Size {
        const ENCODING: Encoding = Encoding::Struct("CGSize", &[f64::ENCODING, f64::ENCODING]);
    }
    unsafe impl Encode for NsRect {
        const ENCODING: Encoding = Encoding::Struct("CGRect", &[Point::ENCODING, Size::ENCODING]);
    }

    /// One `NSScreen`'s place.
    pub struct Screen {
        pub frame: NsRect,
        pub visible: NsRect,
        /// Physical pixels per point.
        pub scale: f64,
    }

    /// Every screen, the main one (with the menu bar) first.
    pub fn screens() -> Vec<Screen> {
        let Some(class) = AnyClass::get(c"NSScreen") else {
            return vec![];
        };
        let list: Option<Retained<AnyObject>> = unsafe { msg_send![class, screens] };
        let Some(list) = list else { return vec![] };
        let n: usize = unsafe { msg_send![&*list, count] };
        let mut out = Vec::with_capacity(n);
        for i in 0..n {
            let s: Option<Retained<AnyObject>> = unsafe { msg_send![&*list, objectAtIndex: i] };
            let Some(object) = s else { continue };
            let frame: NsRect = unsafe { msg_send![&*object, frame] };
            let visible: NsRect = unsafe { msg_send![&*object, visibleFrame] };
            let scale: f64 = unsafe { msg_send![&*object, backingScaleFactor] };
            out.push(Screen {
                frame,
                visible,
                scale: if scale > 0.0 { scale } else { 1.0 },
            });
        }
        out
    }

    /// A screen's bounds as winit gives a monitor's: physical pixels, y running down from the
    /// top of the main screen (winit scales the display's position in points by its own scale).
    pub fn bounds(s: &Screen, main_height: f64) -> Rect {
        let f = s.frame;
        let top = main_height - (f.origin.y + f.size.h);
        Rect::new(
            (f.origin.x * s.scale).round() as i32,
            (top * s.scale).round() as i32,
            (f.size.w * s.scale).round() as u32,
            (f.size.h * s.scale).round() as u32,
        )
    }

    /// The screen showing the monitor whose bounds are `m`: the one whose bounds match, else the
    /// one it overlaps most.
    pub fn screen_for(m: Rect) -> Option<Screen> {
        let list = screens();
        let main_height = list.first()?.frame.size.h;
        let mut best: Option<(u64, usize)> = None;
        for (i, s) in list.iter().enumerate() {
            let b = bounds(s, main_height);
            if b == m {
                best = Some((u64::MAX, i));
                break;
            }
            let o = b.overlap(&m);
            if o > 0 && best.is_none_or(|(bo, _)| o > bo) {
                best = Some((o, i));
            }
        }
        let i = best?.1;
        list.into_iter().nth(i)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_window_under_the_taskbar_is_moved_up() {
        // The Acer: 1920x1080 with a 48-pixel taskbar; the window opened at (189, 182), 1402x932.
        let area = Rect::new(0, 0, 1920, 1032);
        let (pos, inner) = fit_window(area, (189, 182), (1402, 932), (1400, 900));
        assert_eq!(pos, (189, 100));
        assert_eq!(inner, (1400, 900));
    }

    #[test]
    fn a_window_larger_than_the_area_is_shrunk_and_placed_at_its_corner() {
        let area = Rect::new(0, 0, 1366, 720);
        let (pos, inner) = fit_window(area, (50, 40), (1402, 932), (1400, 900));
        assert_eq!(pos, (0, 0));
        assert_eq!(inner, (1364, 688), "keeps the frame's 2 and 32 pixels");
    }

    #[test]
    fn a_window_on_a_monitor_to_the_left_stays_there() {
        // A 2560x1440 monitor at x = -2560, its taskbar at the bottom.
        let area = Rect::new(-2560, 0, 2560, 1392);
        let (pos, inner) = fit_window(area, (-2600, 1000), (1402, 932), (1400, 900));
        assert_eq!(pos, (-2560, 460));
        assert_eq!(inner, (1400, 900));
        // One that fits already is left alone.
        assert_eq!(
            fit_window(area, (-2000, 100), (1402, 932), (1400, 900)),
            ((-2000, 100), (1400, 900))
        );
    }

    #[test]
    fn insets_and_overlaps() {
        let m = Rect::new(0, 0, 1920, 1080);
        assert_eq!(m.insets_of(&Rect::new(0, 0, 1920, 1032)), [0, 0, 0, 48]);
        assert_eq!(m.insets_of(&Rect::new(0, 25, 1920, 985)), [0, 25, 0, 70]);
        assert_eq!(m.insets_of(&m), [0, 0, 0, 0]);
        assert_eq!(m.overlap(&Rect::new(1900, 1000, 100, 100)), 20 * 80);
        assert_eq!(m.overlap(&Rect::new(-2560, 0, 2560, 1440)), 0);
        // Where the system says nothing (Linux), the work area is the monitor.
        if cfg!(all(unix, not(target_os = "macos"))) {
            assert_eq!(work_area(m), m);
        }
    }
}
