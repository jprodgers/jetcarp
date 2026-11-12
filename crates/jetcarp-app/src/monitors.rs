//! The monitors winit sees, for choosing where outputs go.
//!
//! [`MonitorInfo::index`] is the monitor's position in winit's `available_monitors()` order, the
//! same index egui's `ViewportCommand::SetMonitor(index)` and `ViewportBuilder::with_monitor`
//! take (egui-winit resolves it with `available_monitors().nth(index)`). Sizes and positions are
//! in physical pixels. On Wayland, positions may all read (0, 0): compositors do not have to
//! report them.

use std::time::{Duration, Instant};

#[derive(Clone, Debug, PartialEq)]
pub struct MonitorInfo {
    /// winit order; pass to `ViewportCommand::SetMonitor`.
    pub index: usize,
    pub name: Option<String>,
    pub size_px: [u32; 2],
    pub position_px: [i32; 2],
    pub scale_factor: f64,
    /// None when the platform does not say.
    pub refresh_millihertz: Option<u32>,
    /// The monitor the main window is on now (winit may not know on Wayland).
    pub current: bool,
    /// An earlier entry with the same name, size, and position. GNOME on Wayland has been seen
    /// to list one monitor twice; the index still has to stay in winit's order.
    pub same_as: Option<usize>,
}

impl MonitorInfo {
    /// "DP-1 · 2560 × 1440 at (0, 0) · scale 1.00 · 59.95 Hz"
    pub fn describe(&self) -> String {
        let mut s = format!(
            "{} · {} × {} at ({}, {}) · scale {:.2}",
            self.name.as_deref().unwrap_or("unnamed"),
            self.size_px[0],
            self.size_px[1],
            self.position_px[0],
            self.position_px[1],
            self.scale_factor
        );
        if let Some(r) = self.refresh_millihertz {
            s.push_str(&format!(" · {:.2} Hz", r as f64 / 1000.0));
        }
        if let Some(k) = self.same_as {
            s.push_str(&format!(" · same as {k}"));
        }
        s
    }
}

/// The monitor list, refreshed every couple of seconds (and on demand).
#[derive(Default)]
pub struct Monitors {
    list: Vec<MonitorInfo>,
    last: Option<Instant>,
    /// Refresh at the next update.
    pub stale: bool,
}

const REFRESH: Duration = Duration::from_secs(2);

impl Monitors {
    /// Monitors as last read (empty before the first update, or headless).
    pub fn list(&self) -> &[MonitorInfo] {
        &self.list
    }

    /// Ask for a fresh read at the next update.
    pub fn refresh(&mut self) {
        self.stale = true;
    }

    /// Re-read the list if it is due. Called once per frame with the root viewport's frame.
    pub fn update(&mut self, frame: &eframe::Frame) {
        if !self.stale && self.last.is_some_and(|t| t.elapsed() < REFRESH) {
            return;
        }
        self.last = Some(Instant::now());
        self.stale = false;
        if let Some(w) = frame.winit_window() {
            self.list = read(w);
        }
    }
}

/// A monitor's name: on macOS the one the system shows (`NSScreen.localizedName`), since winit's
/// there is "Monitor #" and the display's model number in decimal; winit's otherwise, and as the
/// fallback.
fn monitor_name(m: &winit::monitor::MonitorHandle) -> Option<String> {
    #[cfg(target_os = "macos")]
    {
        use winit::platform::macos::MonitorHandleExtMacOS;
        if let Some(n) = jetcarp_display::workarea::mac_screen_name(m.native_id()) {
            return Some(n);
        }
    }
    m.name()
}

fn read(w: &winit::window::Window) -> Vec<MonitorInfo> {
    let current = w.current_monitor();
    let mut list: Vec<MonitorInfo> = w
        .available_monitors()
        .enumerate()
        .map(|(index, m)| {
            let size = m.size();
            let pos = m.position();
            MonitorInfo {
                index,
                name: monitor_name(&m),
                size_px: [size.width, size.height],
                position_px: [pos.x, pos.y],
                scale_factor: m.scale_factor(),
                refresh_millihertz: m.refresh_rate_millihertz(),
                current: current.as_ref() == Some(&m),
                same_as: None,
            }
        })
        .collect();
    mark_duplicates(&mut list);
    list
}

fn mark_duplicates(list: &mut [MonitorInfo]) {
    for k in 1..list.len() {
        let (before, rest) = list.split_at_mut(k);
        let m = &mut rest[0];
        m.same_as = before
            .iter()
            .find(|e| e.name == m.name && e.size_px == m.size_px && e.position_px == m.position_px)
            .map(|e| e.index);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mon(index: usize, name: &str, x: i32) -> MonitorInfo {
        MonitorInfo {
            index,
            name: Some(name.into()),
            size_px: [2560, 1440],
            position_px: [x, 0],
            scale_factor: 1.0,
            refresh_millihertz: Some(59_950),
            current: false,
            same_as: None,
        }
    }

    #[test]
    fn duplicates_are_marked_in_order() {
        let mut l = vec![
            mon(0, "HDMI-1", 0),
            mon(1, "HDMI-1", 0),
            mon(2, "DP-1", 2560),
        ];
        mark_duplicates(&mut l);
        assert_eq!(
            l.iter().map(|m| m.same_as).collect::<Vec<_>>(),
            vec![None, Some(0), None]
        );
        assert_eq!(
            l[1].describe(),
            "HDMI-1 · 2560 × 1440 at (0, 0) · scale 1.00 · 59.95 Hz · same as 0"
        );
    }
}
