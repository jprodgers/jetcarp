//! Output layout as pure functions: monitor rectangles -> the canvas -> what each output shows.
//!
//! The grid is drawn on a *canvas* with one camera: the world point at the canvas centre and the
//! zoom in canvas units per world unit. A canvas unit is a physical pixel ([`Units::Pixels`],
//! the default), or a logical pixel ([`Units::Logical`], for monitors of mixed scale factors
//! whose positions the compositor reports in logical pixels). Outputs either *span* (each shows
//! the slice of the canvas its rectangle covers, gaps between monitors included) or *mirror*
//! (each shows the canvas camera at its own size). Each output gets its own view: a world
//! centre, a zoom in its own physical pixels per world unit, and its size in physical pixels.

use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Arrangement {
    /// One continuous canvas laid out by the monitors' positions.
    #[default]
    Span,
    /// The same view on each output.
    Mirror,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Units {
    /// Positions and sizes in physical pixels.
    #[default]
    Pixels,
    /// Positions in logical pixels; each monitor covers its size divided by its scale factor.
    Logical,
}

/// An axis-aligned rectangle (canvas units or world units).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub min: [f64; 2],
    pub max: [f64; 2],
}

impl Rect {
    pub fn from_min_size(min: [f64; 2], size: [f64; 2]) -> Rect {
        Rect {
            min,
            max: [min[0] + size[0], min[1] + size[1]],
        }
    }
    pub fn size(&self) -> [f64; 2] {
        [self.max[0] - self.min[0], self.max[1] - self.min[1]]
    }
    pub fn center(&self) -> [f64; 2] {
        [
            (self.min[0] + self.max[0]) / 2.0,
            (self.min[1] + self.max[1]) / 2.0,
        ]
    }
    pub fn union(&self, o: &Rect) -> Rect {
        Rect {
            min: [self.min[0].min(o.min[0]), self.min[1].min(o.min[1])],
            max: [self.max[0].max(o.max[0]), self.max[1].max(o.max[1])],
        }
    }
    /// Overlap area with another rectangle (0 when they only touch).
    pub fn overlap(&self, o: &Rect) -> f64 {
        let w = self.max[0].min(o.max[0]) - self.min[0].max(o.min[0]);
        let h = self.max[1].min(o.max[1]) - self.min[1].max(o.min[1]);
        w.max(0.0) * h.max(0.0)
    }
}

/// One output's place: its top-left corner on the canvas (canvas units), its size in physical
/// pixels, and its scale factor.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Screen {
    pub pos: [f64; 2],
    pub size_px: [f64; 2],
    pub scale: f64,
}

impl Screen {
    /// Physical pixels per canvas unit on this screen.
    pub fn px_per_unit(&self, units: Units) -> f64 {
        match units {
            Units::Pixels => 1.0,
            Units::Logical => self.scale.max(1e-3),
        }
    }
    /// The canvas rectangle it covers.
    pub fn rect(&self, units: Units) -> Rect {
        let k = self.px_per_unit(units);
        Rect::from_min_size(self.pos, [self.size_px[0] / k, self.size_px[1] / k])
    }
}

/// The canvas camera.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Camera {
    /// World point at the canvas centre.
    pub center: [f64; 2],
    /// Canvas units per world unit.
    pub zoom: f64,
}

/// What one output shows.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct View {
    /// World point at the output's centre.
    pub center: [f64; 2],
    /// The output's physical pixels per world unit.
    pub zoom_px: f64,
    pub size_px: [f64; 2],
}

impl View {
    /// The world rectangle it shows.
    pub fn world_rect(&self) -> Rect {
        let (hw, hh) = (
            self.size_px[0] / self.zoom_px / 2.0,
            self.size_px[1] / self.zoom_px / 2.0,
        );
        Rect {
            min: [self.center[0] - hw, self.center[1] - hh],
            max: [self.center[0] + hw, self.center[1] + hh],
        }
    }
    /// The world point under a pixel of the output (from its top-left corner).
    pub fn world_at(&self, px: [f64; 2]) -> [f64; 2] {
        [
            self.center[0] + (px[0] - self.size_px[0] / 2.0) / self.zoom_px,
            self.center[1] + (px[1] - self.size_px[1] / 2.0) / self.zoom_px,
        ]
    }
}

/// The span canvas: the bounding box of every screen, gaps included. None without screens.
pub fn canvas(screens: &[Screen], units: Units) -> Option<Rect> {
    let mut it = screens.iter().map(|s| s.rect(units));
    let first = it.next()?;
    Some(it.fold(first, |a, r| a.union(&r)))
}

/// The canvas size that Fill and Fit use, in canvas units: the span's bounding box, or the
/// largest screen when mirrored (every mirror is then covered).
pub fn canvas_size(screens: &[Screen], arrangement: Arrangement, units: Units) -> Option<[f64; 2]> {
    match arrangement {
        Arrangement::Span => canvas(screens, units).map(|r| r.size()),
        Arrangement::Mirror => screens
            .iter()
            .map(|s| s.rect(units).size())
            .reduce(|a, b| [a[0].max(b[0]), a[1].max(b[1])]),
    }
}

/// Each screen's view of the canvas camera, in order.
pub fn views(screens: &[Screen], arrangement: Arrangement, units: Units, cam: Camera) -> Vec<View> {
    let Some(c) = canvas(screens, units) else {
        return vec![];
    };
    let cc = c.center();
    screens
        .iter()
        .map(|s| {
            let k = s.px_per_unit(units);
            let center = match arrangement {
                Arrangement::Span => {
                    let r = s.rect(units).center();
                    [
                        cam.center[0] + (r[0] - cc[0]) / cam.zoom,
                        cam.center[1] + (r[1] - cc[1]) / cam.zoom,
                    ]
                }
                Arrangement::Mirror => cam.center,
            };
            View {
                center,
                zoom_px: cam.zoom * k,
                size_px: s.size_px,
            }
        })
        .collect()
}

/// Zoom the camera by `f` keeping world point `about` where it is on every output.
pub fn zoom_about(cam: Camera, f: f64, about: [f64; 2]) -> Camera {
    let zoom = cam.zoom * f;
    let k = cam.zoom / zoom;
    Camera {
        center: [
            about[0] + (cam.center[0] - about[0]) * k,
            about[1] + (cam.center[1] - about[1]) * k,
        ],
        zoom,
    }
}

/// Move the camera centre (by less than a pixel) so that world points on the lattice
/// `origin + integer / zoom` land on pixel corners of every screen of a span in pixel units
/// (and of a single view): with whole-pixel cells, cell edges then fall on pixel edges.
/// `canvas_center` is the canvas centre in canvas units, relative to any whole-pixel origin
/// (for a single view of width `w`, `w / 2`).
pub fn snap_center(cam: Camera, canvas_center: [f64; 2], origin: [f64; 2]) -> [f64; 2] {
    let s = |c: f64, cc: f64, o: f64| {
        let t = (c - o) * cam.zoom - cc;
        o + (t.round() + cc) / cam.zoom
    };
    [
        s(cam.center[0], canvas_center[0], origin[0]),
        s(cam.center[1], canvas_center[1], origin[1]),
    ]
}

/// Units that fit the positions: logical when screens overlap in pixels but not in logical
/// pixels (positions reported in logical pixels on mixed scale factors), else pixels.
pub fn guess_units(screens: &[Screen]) -> Units {
    let overlap = |u: Units| {
        let r: Vec<Rect> = screens.iter().map(|s| s.rect(u)).collect();
        let mut a = 0.0;
        for i in 0..r.len() {
            for j in i + 1..r.len() {
                a += r[i].overlap(&r[j]);
            }
        }
        a
    };
    if overlap(Units::Pixels) > 0.0 && overlap(Units::Logical) == 0.0 {
        Units::Logical
    } else {
        Units::Pixels
    }
}

/// Place screens left to right in the given order, tops aligned (a manual arrangement).
pub fn side_by_side(screens: &mut [Screen], units: Units) {
    let mut x = 0.0;
    for s in screens.iter_mut() {
        s.pos = [x, 0.0];
        x += s.rect(units).size()[0];
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scr(x: f64, y: f64, w: f64, h: f64) -> Screen {
        Screen {
            pos: [x, y],
            size_px: [w, h],
            scale: 1.0,
        }
    }

    fn close(a: [f64; 2], b: [f64; 2]) -> bool {
        (a[0] - b[0]).abs() < 1e-9 && (a[1] - b[1]).abs() < 1e-9
    }

    #[test]
    fn two_side_by_side_slices_abut() {
        let s = [
            scr(0.0, 0.0, 1920.0, 1080.0),
            scr(1920.0, 0.0, 1920.0, 1080.0),
        ];
        let c = canvas(&s, Units::Pixels).unwrap();
        assert_eq!(c.size(), [3840.0, 1080.0]);
        let cam = Camera {
            center: [10.0, 5.0],
            zoom: 2.0,
        };
        let v = views(&s, Arrangement::Span, Units::Pixels, cam);
        let (a, b) = (v[0].world_rect(), v[1].world_rect());
        assert!(close(a.min, [10.0 - 960.0, 5.0 - 270.0]));
        assert_eq!(a.max[0], b.min[0], "the slices meet");
        assert_eq!(a.min[1], b.min[1]);
        assert!(close(b.max, [10.0 + 960.0, 5.0 + 270.0]));
        // The span's union is the canvas seen through the camera.
        assert_eq!(v[0].zoom_px, 2.0);
        // The same world point under the right edge of one and the left edge of the other.
        assert!(close(
            v[0].world_at([1920.0, 0.0]),
            v[1].world_at([0.0, 0.0])
        ));
    }

    #[test]
    fn gaps_and_vertical_offsets_are_kept() {
        // Right monitor 80 px further right and 200 px lower, smaller.
        let s = [
            scr(0.0, 0.0, 1920.0, 1080.0),
            scr(2000.0, 200.0, 1280.0, 720.0),
        ];
        let c = canvas(&s, Units::Pixels).unwrap();
        assert_eq!(c.min, [0.0, 0.0]);
        assert_eq!(c.max, [3280.0, 1080.0]);
        let cam = Camera {
            center: [0.0, 0.0],
            zoom: 1.0,
        };
        let v = views(&s, Arrangement::Span, Units::Pixels, cam);
        let (a, b) = (v[0].world_rect(), v[1].world_rect());
        assert!((b.min[0] - a.max[0] - 80.0).abs() < 1e-9, "80 px gap");
        assert!((b.min[1] - a.min[1] - 200.0).abs() < 1e-9);
        // The canvas centre is the camera centre.
        let whole = a.union(&b);
        assert!(close(whole.center(), [0.0, 0.0]));
    }

    #[test]
    fn negative_positions_and_stacks() {
        let s = [
            scr(-1920.0, 0.0, 1920.0, 1080.0),
            scr(0.0, 0.0, 2560.0, 1440.0),
            scr(0.0, -1080.0, 1920.0, 1080.0),
        ];
        let c = canvas(&s, Units::Pixels).unwrap();
        assert_eq!(c.min, [-1920.0, -1080.0]);
        assert_eq!(c.max, [2560.0, 1440.0]);
        let v = views(
            &s,
            Arrangement::Span,
            Units::Pixels,
            Camera {
                center: [0.0, 0.0],
                zoom: 4.0,
            },
        );
        assert_eq!(v[0].world_rect().max[0], v[1].world_rect().min[0]);
        assert_eq!(v[2].world_rect().max[1], v[1].world_rect().min[1]);
    }

    #[test]
    fn mixed_scale_factors() {
        // A 4K monitor at scale 2 and a 1080p one at scale 1 to its right, positions in
        // logical pixels (as Wayland compositors report them).
        let s = [
            Screen {
                pos: [0.0, 0.0],
                size_px: [3840.0, 2160.0],
                scale: 2.0,
            },
            scr(1920.0, 0.0, 1920.0, 1080.0),
        ];
        // In pixels the 4K one would cover the other: logical units are guessed.
        assert_eq!(guess_units(&s), Units::Logical);
        let c = canvas(&s, Units::Logical).unwrap();
        assert_eq!(c.size(), [3840.0, 1080.0]);
        let cam = Camera {
            center: [0.0, 0.0],
            zoom: 1.0,
        };
        let v = views(&s, Arrangement::Span, Units::Logical, cam);
        assert_eq!(
            v[0].zoom_px, 2.0,
            "two pixels per world unit on the 4K monitor"
        );
        assert_eq!(v[1].zoom_px, 1.0);
        let (a, b) = (v[0].world_rect(), v[1].world_rect());
        assert_eq!(a.size(), [1920.0, 1080.0]);
        assert_eq!(a.max[0], b.min[0]);
        assert_eq!(a.size(), b.size(), "same physical size, same world size");
        // Side by side monitors in pixels keep pixel units.
        assert_eq!(
            guess_units(&[scr(0.0, 0.0, 100.0, 100.0), scr(100.0, 0.0, 100.0, 100.0)]),
            Units::Pixels
        );
    }

    #[test]
    fn mirror_shows_the_camera_on_each() {
        let s = [
            scr(0.0, 0.0, 1920.0, 1080.0),
            scr(1920.0, 0.0, 1280.0, 720.0),
        ];
        let cam = Camera {
            center: [3.0, 4.0],
            zoom: 8.0,
        };
        let v = views(&s, Arrangement::Mirror, Units::Pixels, cam);
        assert!(v.iter().all(|v| v.center == [3.0, 4.0] && v.zoom_px == 8.0));
        assert_eq!(v[1].size_px, [1280.0, 720.0]);
        assert_eq!(
            canvas_size(&s, Arrangement::Mirror, Units::Pixels),
            Some([1920.0, 1080.0])
        );
        assert_eq!(
            canvas_size(&s, Arrangement::Span, Units::Pixels),
            Some([3200.0, 1080.0])
        );
        assert!(views(&[], Arrangement::Mirror, Units::Pixels, cam).is_empty());
    }

    #[test]
    fn duplicates_lay_out_as_one() {
        // GNOME on Wayland has listed one monitor twice; after dropping the duplicate there is
        // one screen, and two identical screens would cover the same slice.
        let s = [scr(0.0, 0.0, 2560.0, 1440.0), scr(0.0, 0.0, 2560.0, 1440.0)];
        let v = views(
            &s,
            Arrangement::Span,
            Units::Pixels,
            Camera {
                center: [0.0, 0.0],
                zoom: 1.0,
            },
        );
        assert_eq!(v[0], v[1]);
        assert_eq!(canvas(&s[..1], Units::Pixels), canvas(&s, Units::Pixels));
    }

    #[test]
    fn zoom_about_keeps_the_point() {
        let s = [
            scr(0.0, 0.0, 1000.0, 500.0),
            scr(1000.0, 0.0, 1000.0, 500.0),
        ];
        let cam = Camera {
            center: [0.0, 0.0],
            zoom: 2.0,
        };
        let v = views(&s, Arrangement::Span, Units::Pixels, cam);
        let px = [250.0, 100.0];
        let w = v[1].world_at(px);
        let cam2 = zoom_about(cam, 1.7, w);
        let v2 = views(&s, Arrangement::Span, Units::Pixels, cam2);
        assert!(close(v2[1].world_at(px), w));
    }

    #[test]
    fn snapping_puts_cell_edges_on_pixels() {
        // A 2560 x 1441 view at 3 px per unit: after snapping, integer world points map to
        // integer pixels.
        let cam = Camera {
            center: [12.345, -7.77],
            zoom: 3.0,
        };
        let cc = [1280.0, 720.5];
        let c = snap_center(cam, cc, [0.0, 0.0]);
        assert!((c[0] - cam.center[0]).abs() <= 0.5 / 3.0 + 1e-12);
        let v = View {
            center: c,
            zoom_px: 3.0,
            size_px: [2560.0, 1441.0],
        };
        for wx in [-5.0, 0.0, 17.0] {
            let px = (wx - v.world_at([0.0, 0.0])[0]) * 3.0;
            assert!((px - px.round()).abs() < 1e-9, "{px}");
            let py = (wx - v.world_at([0.0, 0.0])[1]) * 3.0;
            assert!((py - py.round()).abs() < 1e-9, "{py}");
        }
        // Snapping a span keeps every slice aligned (integer positions).
        let s = [
            scr(0.0, 0.0, 1921.0, 1080.0),
            scr(1921.0, 0.0, 1280.0, 1024.0),
        ];
        let can = canvas(&s, Units::Pixels).unwrap();
        let cam = Camera {
            center: [0.3, 0.6],
            zoom: 1.0,
        };
        let snapped = Camera {
            center: snap_center(cam, can.center(), [0.0, 0.0]),
            ..cam
        };
        for v in views(&s, Arrangement::Span, Units::Pixels, snapped) {
            let o = v.world_at([0.0, 0.0]);
            assert!((o[0] - o[0].round()).abs() < 1e-9 && (o[1] - o[1].round()).abs() < 1e-9);
        }
    }

    #[test]
    fn side_by_side_arranges_in_order() {
        let mut s = [
            scr(500.0, 30.0, 1920.0, 1080.0),
            scr(0.0, 0.0, 1280.0, 720.0),
        ];
        side_by_side(&mut s, Units::Pixels);
        assert_eq!(s[0].pos, [0.0, 0.0]);
        assert_eq!(s[1].pos, [1920.0, 0.0]);
    }
}
