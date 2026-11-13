//! Colours and CPU rendering. The app uses [`cell_colors`] for its GPU state texture; the CLI and
//! PNG export use [`render`], so both show the same colours.

use crate::geom::*;
use crate::rules::{FieldKind, Rule};
use crate::topology::{Nb, Topology};
use crate::world::World;
use rayon::prelude::*;

pub type Rgba = [u8; 4];

/// Empty cells. Slightly lighter than the stage so the tiling's extent shows.
pub const BACKGROUND: Rgba = [0x1d, 0x20, 0x29, 0xff];
/// Outside the tiling.
pub const STAGE: Rgba = [0x10, 0x12, 0x17, 0xff];
pub const OUTLINE: Rgba = [0x2c, 0x31, 0x3c, 0xff];
pub const AGENT: Rgba = [0xff, 0x3b, 0x6b, 0xff];

/// A built-in palette: colour stops (packed RGB) from young or low to old or high, and the
/// colours of empty cells and of the area outside the tiling.
#[derive(Clone, Copy, Debug)]
pub struct Builtin {
    pub name: &'static str,
    pub stops: &'static [u32],
    pub background: u32,
    pub stage: u32,
}

const fn pal(name: &'static str, stops: &'static [u32], background: u32, stage: u32) -> Builtin {
    Builtin {
        name,
        stops,
        background,
        stage,
    }
}

const BG: u32 = 0x1d2029;
const ST: u32 = 0x101217;

/// Built-in palettes. The first five come from JETCARP's JavaScript prototype (plus Mono) on
/// JETCARP's dark slate background. Stop 0 is how new cells look under age colouring; later stops
/// are older cells (still lifes end at the last stop), so every stop contrasts with the
/// background. Candy, Glacier, Desert, and Paper are light-background palettes.
pub static PALETTES: &[Builtin] = &[
    pal(
        "Ember",
        &[0xffd27a, 0xff8a3c, 0xd6336c, 0x6a2c91, 0x2b2d6e],
        BG,
        ST,
    ),
    pal(
        "Tide",
        &[0xe8fff7, 0x58e0c2, 0x1f9fc9, 0x2a5bb8, 0x2b2470],
        BG,
        ST,
    ),
    pal(
        "Prism",
        &[
            0xff4d4d, 0xffb02e, 0xe8e337, 0x3ddc6d, 0x2fb6ff, 0x8a5cff, 0xff4dc4,
        ],
        BG,
        ST,
    ),
    pal(
        "Ink",
        &[0x7fd0ff, 0x7fd0ff, 0x5a9bd0, 0x3d6a99, 0x2a4766],
        BG,
        ST,
    ),
    pal("Mono", &[0xf2f2f2, 0xc8c8c8, 0x8c8c8c, 0x5a5a5a], BG, ST),
    pal(
        "Aurora",
        &[0xc8ffe8, 0x4dffb0, 0x27d3c3, 0x4f7bff, 0x9c5cff, 0x7a3fc4],
        0x0a0f24,
        0x05081a,
    ),
    pal(
        "Synthwave",
        &[0xff71ce, 0xc77dff, 0x3fd0ff, 0x2fe8b0, 0xfff59a],
        0x1b0b30,
        0x0e0619,
    ),
    pal(
        "Moss",
        &[0xf0f7b8, 0xb5dc5e, 0x6faa3e, 0x468a3c, 0x3b7046],
        0x141a11,
        0x0a0d09,
    ),
    pal(
        "Candy",
        &[0xe0407a, 0xf07fae, 0xa77fe0, 0x5fb0e8, 0x4cbf98],
        0xfff6e8,
        0xf0e2cc,
    ),
    pal(
        "Lava",
        &[0xfff4c0, 0xffc23e, 0xff6a1a, 0xd8261c, 0x9c1424, 0x6e1022],
        0x140806,
        0x0a0403,
    ),
    pal(
        "Glacier",
        &[0x05305e, 0x15599a, 0x2f86c4, 0x5aa7d8, 0x82bde0],
        0xedf5fa,
        0xd5e4ee,
    ),
    pal(
        "Phosphor",
        &[0xe0ffe0, 0x6dff6d, 0x33d040, 0x23a032, 0x1a7a28],
        0x030d06,
        0x010603,
    ),
    pal(
        "Koi",
        &[0xffffff, 0xffd2a8, 0xff8a33, 0xe0412b, 0xb3262d],
        0x0d2a33,
        0x071a20,
    ),
    pal(
        "Desert",
        &[0x4a1f0e, 0x8a3c18, 0xc0632a, 0xd58a48, 0xc79a68],
        0xf4e4c6,
        0xe6d1aa,
    ),
    pal(
        "Thermal",
        &[0xffffff, 0xfff05a, 0xff9a1a, 0xe8203c, 0xa01c9c, 0x4a22c8],
        0x07000f,
        0x030008,
    ),
    pal(
        "Deep Sea",
        &[0xa8fbff, 0x3ad8ea, 0x1a92d2, 0x1d62b0, 0x2a4a90, 0x343c88],
        0x020c19,
        0x01060d,
    ),
    pal(
        "Paper",
        &[0x111418, 0x2b3a58, 0x4b5b7c, 0x6f7e9a, 0x8e98ac],
        0xf6f2e9,
        0xe7e0d1,
    ),
    pal(
        "Blueprint",
        &[0xffffff, 0xdcefff, 0xaed3fa, 0x80b5ec, 0x5f9be0],
        0x0f3b70,
        0x092a52,
    ),
    pal(
        "Neon",
        &[0xfffb7a, 0x39ff14, 0x00f0ff, 0xff2fd0, 0xff5a1f],
        0x070707,
        0x000000,
    ),
    pal(
        "Autumn",
        &[0xff5236, 0xffa83c, 0xd9792c, 0xae4a2c, 0x8c6431],
        0x1b130d,
        0x0e0a06,
    ),
    // Bright at the old end, so a pattern that has settled stays vivid.
    pal(
        "Verdigris",
        &[0xfff1c9, 0xf2b366, 0xc7743c, 0x3fa796, 0x7fe0c8],
        0x10201f,
        0x081211,
    ),
    pal(
        "Marigold",
        &[0xffffff, 0xffe27a, 0xffb81c, 0xf08a00, 0xffc94d],
        0x14213d,
        0x0b1426,
    ),
    pal(
        "Coral",
        &[0xfff0e6, 0xff9e80, 0xff5e62, 0x2ec4b6, 0x9bf6e3],
        0x102a43,
        0x081a2c,
    ),
    pal(
        "Orchid",
        &[0xfff0ff, 0xf4a6ff, 0xd05cff, 0x7a6cff, 0xb8b4ff],
        0x170f2e,
        0x0d081c,
    ),
    pal(
        "Citrus",
        &[0xffffd6, 0xe9ff70, 0xa7e33b, 0xffa62b, 0xffd166],
        0x2a1235,
        0x180a1f,
    ),
    pal(
        "Riso",
        &[0xff3d7f, 0xff6b9d, 0x3a56d4, 0x1f3bb3, 0x14257f],
        0xf7f1e1,
        0xe9e0c9,
    ),
    pal(
        "Bone",
        &[0xff5a36, 0xf2c14e, 0xe8dcc0, 0xf5eedc, 0xfffaf0],
        0x15130f,
        0x0b0a08,
    ),
    pal(
        "Ultramarine",
        &[0xffffff, 0x9bd1ff, 0x4aa3ff, 0x2f6bff, 0x6f8cff],
        0x05071a,
        0x02030c,
    ),
    // Closed colour wheels: the last stop returns to the first, so cyclic rules have no seam.
    pal(
        "Stained Glass",
        &[
            0xe63946, 0xf4a261, 0xf1d54b, 0x2a9d8f, 0x3a86ff, 0x9b5de5, 0xe63946,
        ],
        0x0d0d12,
        0x060608,
    ),
    pal(
        "Opal",
        &[
            0xffb3c6, 0xffd6a5, 0xfdffb6, 0xb9fbc0, 0xa0c4ff, 0xcdb4ff, 0xffb3c6,
        ],
        0x1b1d2b,
        0x10111a,
    ),
];

pub fn palette_names() -> impl Iterator<Item = &'static str> {
    PALETTES.iter().map(|p| p.name)
}

/// A built-in palette by name, or an error that lists every name (for `--palette` flags,
/// `"palette"` arguments, and script options).
pub fn builtin_palette(name: &str) -> Result<Palette, String> {
    Palette::builtin(name).or_else(|| Palette::from_hex(name)).ok_or_else(|| {
        format!(
            "no palette named '{name}'. Palettes: {}, or colours like #4fc3ff or #103050-#4fc3ff",
            palette_names().collect::<Vec<_>>().join(", ")
        )
    })
}

/// Give every CA of `world` the palette `name` (unlogged; for pictures and previews, where the
/// command log does not matter). The name may be a built-in or one of the world's own.
pub fn recolor(world: &mut World, name: &str) {
    for ca in &mut world.cas {
        ca.palette = name.to_string();
    }
}

/// Give every CA of `scene` the palette `name` (before the world is built).
pub fn recolor_scene(scene: &mut crate::world::Scene, name: &str) {
    for ca in &mut scene.cas {
        ca.palette = Some(name.to_string());
    }
}

/// Interpolate a palette's stops at `f` in [0, 1].
pub fn ramp_palette(p: &Palette, f: f64) -> Rgba {
    ramp_stops(&p.packed(), f)
}

fn rgb3(v: u32) -> [u8; 3] {
    [(v >> 16) as u8, (v >> 8) as u8, v as u8]
}
fn rgba3(c: [u8; 3]) -> Rgba {
    [c[0], c[1], c[2], 0xff]
}

/// A palette: named colour stops (RGB), and optionally its own colours for empty cells
/// (`background`) and outside the tiling (`stage`); JETCARP's dark slate when unset. User palettes
/// are saved in scenes.
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Palette {
    pub name: String,
    pub stops: Vec<[u8; 3]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub background: Option<[u8; 3]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stage: Option<[u8; 3]>,
}

impl Palette {
    /// A palette with the default background and stage.
    pub fn new(name: &str, stops: Vec<[u8; 3]>) -> Palette {
        Palette {
            name: name.into(),
            stops,
            ..Default::default()
        }
    }
    pub fn packed(&self) -> Vec<u32> {
        self.stops
            .iter()
            .map(|c| (c[0] as u32) << 16 | (c[1] as u32) << 8 | c[2] as u32)
            .collect()
    }
    /// A palette written as colours: `#4fc3ff` (one colour) or `#103050-#4fc3ff` (a gradient;
    /// stops separated by `-`, `,` or spaces, young or low first).
    pub fn from_hex(name: &str) -> Option<Palette> {
        let t = name.trim();
        if !t.starts_with('#') {
            return None;
        }
        let stops: Option<Vec<[u8; 3]>> = t
            .split(|c: char| c == '-' || c == ',' || c.is_whitespace())
            .filter(|p| !p.is_empty())
            .map(|p| {
                let h = p.strip_prefix('#')?;
                let v = match h.len() {
                    6 => u32::from_str_radix(h, 16).ok()?,
                    3 => {
                        let v = u32::from_str_radix(h, 16).ok()?;
                        let (r, g, b) = (v >> 8 & 0xf, v >> 4 & 0xf, v & 0xf);
                        ((r * 17) << 16) | ((g * 17) << 8) | (b * 17)
                    }
                    _ => return None,
                };
                Some(rgb3(v))
            })
            .collect();
        stops.filter(|s| !s.is_empty()).map(|s| Palette::new(t, s))
    }
    pub fn builtin(name: &str) -> Option<Palette> {
        PALETTES.iter().find(|p| p.name == name).map(|p| Palette {
            name: p.name.into(),
            stops: p.stops.iter().map(|&v| rgb3(v)).collect(),
            background: Some(rgb3(p.background)),
            stage: Some(rgb3(p.stage)),
        })
    }
    /// Colour of empty cells.
    pub fn background_rgba(&self) -> Rgba {
        self.background.map(rgba3).unwrap_or(BACKGROUND)
    }
    /// Colour outside the tiling.
    pub fn stage_rgba(&self) -> Rgba {
        self.stage.map(rgba3).unwrap_or(STAGE)
    }
    /// Tile outlines: JETCARP's outline on the default background, else the background shifted a
    /// little toward black (light backgrounds) or white (dark ones).
    pub fn outline_rgba(&self) -> Rgba {
        outline_for(self.background_rgba())
    }
}

/// An outline colour that shows faintly on background `bg`.
pub fn outline_for(bg: Rgba) -> Rgba {
    if bg == BACKGROUND {
        return OUTLINE;
    }
    let lum = 0.299 * bg[0] as f64 + 0.587 * bg[1] as f64 + 0.114 * bg[2] as f64;
    let f = |c: u8| {
        if lum > 128.0 {
            (c as f64 * 0.84).round() as u8
        } else {
            (c as f64 + (255.0 - c as f64) * 0.1).round() as u8
        }
    };
    [f(bg[0]), f(bg[1]), f(bg[2]), 0xff]
}

/// The palette the world's backdrop comes from: that of the first CA drawn on the bottom field
/// (fields are composited in index order), else Ember.
pub fn backdrop_palette(world: &World) -> Palette {
    let name = world
        .cas
        .iter()
        .filter(|c| c.field == 0)
        .chain(world.cas.iter())
        .map(|c| c.palette.clone())
        .next()
        .unwrap_or_else(|| "Ember".into());
    world.palette(&name)
}

/// (Empty cells, outside the tiling) colours for the world: its backdrop palette's background
/// and stage. The app clears its view with the stage colour; exports use both.
pub fn backdrop(world: &World) -> (Rgba, Rgba) {
    let p = backdrop_palette(world);
    (p.background_rgba(), p.stage_rgba())
}

/// How a CA's cells are coloured.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ColorBy {
    /// The rule's natural choice (age for B/S rules, state for multi-state, height for sandpiles).
    #[default]
    Auto,
    /// How long each cell has been in its state.
    Age,
    /// One colour per state.
    State,
    /// By row (top to bottom): Wolfram row gradients, and a position gradient on any tiling.
    Row,
}

/// Interpolate a built-in palette at `f` in [0, 1].
pub fn ramp(palette: &str, f: f64) -> Rgba {
    let stops = PALETTES
        .iter()
        .find(|p| p.name == palette)
        .unwrap_or(&PALETTES[0])
        .stops;
    ramp_stops(stops, f)
}

/// Interpolate packed RGB stops at `f` in [0, 1].
pub fn ramp_stops(stops: &[u32], f: f64) -> Rgba {
    if stops.is_empty() {
        return [0xff; 4];
    }
    if stops.len() == 1 {
        let v = stops[0];
        return [(v >> 16) as u8, (v >> 8) as u8, v as u8, 0xff];
    }
    let x = f.clamp(0.0, 0.9999) * (stops.len() - 1) as f64;
    let i = x.floor() as usize;
    let u = x - i as f64;
    let c = |v: u32, s: u32| ((v >> s) & 0xff) as f64;
    let (a, b) = (stops[i], stops[i + 1]);
    let mix = |s| (c(a, s) + (c(b, s) - c(a, s)) * u).round() as u8;
    [mix(16), mix(8), mix(0), 0xff]
}

/// How a field is coloured, from the first CA drawn on it.
#[derive(Clone, Debug)]
enum Style {
    Age {
        lut: Vec<Rgba>,
        states: u8,
    },
    /// Colour per state. `zero` also paints state 0 (rules where 0 is not "empty").
    States {
        lut: Vec<Rgba>,
        zero: bool,
    },
    Wire,
    Height {
        lut: Vec<Rgba>,
        nb: Nb,
    },
    /// Nonzero cells coloured by their row (top to bottom).
    Row {
        lut: Vec<Rgba>,
    },
    /// Continuous fields: nonzero cells by value (the 8-bit level), low values at the palette's
    /// far end fading to its first colour at 1.
    Value {
        lut: Vec<Rgba>,
    },
    /// JACA: +1 and -1 cells from their own palettes (by age, or one colour each), and
    /// annihilated cells glowing over the background (from CA `ca`'s heat, when `glow` is set).
    Signed {
        pos: Vec<Rgba>,
        neg: Vec<Rgba>,
        by_age: bool,
        ca: usize,
        glow: bool,
    },
}

/// The colour annihilated JACA cells glow with.
pub const GLOW: Rgba = [0xff, 0xf3, 0xc4, 0xff];

/// A cell's row as a fraction from top (0) to bottom (1).
fn row_fraction(t: &Topology, i: usize) -> f32 {
    match t.lattice() {
        Some(g) => g.decompose(i).1 as f32 / (g.h.max(2) - 1) as f32,
        None => {
            let b = t.bounds;
            ((t.centroid(i)[1] - b[1]) / (b[3] - b[1]).max(1e-9)) as f32
        }
    }
}

const AGE_LEVELS: usize = 41;

fn style_for(world: &World, field: usize, bg: Rgba) -> Style {
    let ca = world.cas.iter().find(|c| c.field == field);
    let palette = ca.map(|c| c.palette.as_str()).unwrap_or("Ember");
    let stops = world.palette_stops(palette);
    let rp = |x: f64| ramp_stops(&stops, x);
    let f = &world.fields[field];
    if f.kind == FieldKind::Continuous && ca.is_none_or(|c| c.color_by != ColorBy::Row) {
        // Level 0 is empty; levels 1..=255 run from the palette's last stop to its first.
        return Style::Value {
            lut: (0..256)
                .map(|q| {
                    if q == 0 {
                        bg
                    } else {
                        rp(1.0 - q as f64 / 255.0)
                    }
                })
                .collect(),
        };
    }
    if let Some(c) = ca
        && matches!(c.rule, Rule::Jaca(_))
        && c.color_by != ColorBy::Row
    {
        let by_age = c.color_by != ColorBy::State;
        let lut = |name: &str| -> Vec<Rgba> {
            let st = world.palette_stops(name);
            if by_age {
                (0..AGE_LEVELS)
                    .map(|q| ramp_stops(&st, q as f64 / (AGE_LEVELS - 1) as f64))
                    .collect()
            } else {
                vec![ramp_stops(&st, 0.0)]
            }
        };
        return Style::Signed {
            pos: lut(&c.palette),
            neg: lut(&c.palette_neg),
            by_age,
            ca: world.cas.iter().position(|x| x.id == c.id).unwrap_or(0),
            glow: c.glow,
        };
    }
    if let Some(ca) = ca {
        match (ca.color_by, f.kind) {
            (ColorBy::Row, _) => {
                return Style::Row {
                    lut: (0..64).map(|q| rp(q as f64 / 63.0)).collect(),
                };
            }
            (ColorBy::Age, k) if k != FieldKind::Height => {
                let states = if let FieldKind::Multi(n) = k { n } else { 2 };
                return Style::Age {
                    lut: (0..AGE_LEVELS)
                        .map(|q| rp(q as f64 / (AGE_LEVELS - 1) as f64))
                        .collect(),
                    states,
                };
            }
            (ColorBy::State, k) if k != FieldKind::Height => {
                let n = (f.max_state() as usize + 1).max(2);
                let mut lut = vec![bg];
                lut.extend((1..n).map(|s| {
                    rp(if n <= 2 {
                        0.0
                    } else {
                        (s - 1) as f64 / (n - 2) as f64
                    })
                }));
                return Style::States { lut, zero: false };
            }
            _ => {}
        }
    }
    match (ca.map(|c| &c.rule), f.kind) {
        (_, FieldKind::Height) => {
            let nb = ca.map(|c| c.nb).unwrap_or(Nb::Edge);
            let maxd = match world.topo.lattice() {
                Some(g) => g.offsets[nb as usize]
                    .iter()
                    .map(|o| o.len())
                    .max()
                    .unwrap_or(4),
                None => (0..world.topo.len())
                    .map(|i| world.topo.degree(i, nb))
                    .max()
                    .unwrap_or(4),
            }
            .max(2);
            let mut lut: Vec<Rgba> = (0..maxd)
                .map(|h| {
                    if h == 0 {
                        bg
                    } else {
                        rp((h - 1) as f64 / (maxd - 1).max(1) as f64)
                    }
                })
                .collect();
            // Cells at or above their toppling threshold: the palette's far end.
            lut.push(rp(1.0));
            Style::Height { lut, nb }
        }
        (Some(Rule::WireWorld), _) => Style::Wire,
        (Some(Rule::VonNeumann), _) => Style::States {
            lut: crate::vonneumann::COLORS.to_vec(),
            zero: false,
        },
        (Some(Rule::Turmite(t)), _) => {
            let n = (t.colors as usize).max(2);
            Style::States {
                lut: (0..n)
                    .map(|s| {
                        if s == 0 {
                            bg
                        } else {
                            rp((s - 1) as f64 / (n - 1) as f64)
                        }
                    })
                    .collect(),
                zero: false,
            }
        }
        (Some(Rule::SpectreGlider), _) => Style::States {
            lut: vec![bg, rp(0.0), rp(0.8), rp(0.5)],
            zero: false,
        },
        (Some(Rule::Cyclic { states, .. } | Rule::Rps { states, .. }), _) => {
            let n = *states as usize;
            Style::States {
                lut: (0..n).map(|s| rp(s as f64 / n as f64)).collect(),
                zero: true,
            }
        }
        (Some(Rule::Hodgepodge { n, .. }), _) => {
            let n = *n as usize;
            Style::States {
                lut: (0..=n).map(|s| rp(s as f64 / n as f64)).collect(),
                zero: true,
            }
        }
        (Some(Rule::GreenbergHastings { states, .. }), _) => {
            let n = *states as usize;
            let mut lut = vec![bg, rp(0.0)];
            lut.extend((2..n).map(|s| rp(0.35 + 0.65 * (s - 2) as f64 / (n - 2).max(1) as f64)));
            Style::States { lut, zero: false }
        }
        (Some(Rule::ForestFire { .. }), _) => Style::States {
            lut: vec![bg, [0x2f, 0x8f, 0x46, 0xff], [0xff, 0x7a, 0x1a, 0xff]],
            zero: false,
        },
        (
            Some(
                Rule::HexWolfram { states: 3, .. }
                | Rule::HexTri { states: 3, .. }
                | Rule::Table(_),
            ),
            FieldKind::Multi(n),
        ) => {
            let n = n as usize;
            let mut lut = vec![bg];
            lut.extend((1..n).map(|s| rp((s - 1) as f64 / (n - 1).max(1) as f64)));
            Style::States { lut, zero: false }
        }
        _ => {
            let states = match f.kind {
                FieldKind::Multi(n) => n,
                _ => 2,
            };
            Style::Age {
                lut: (0..AGE_LEVELS)
                    .map(|q| rp(q as f64 / (AGE_LEVELS - 1) as f64))
                    .collect(),
                states,
            }
        }
    }
}

/// Colours cells one at a time: fields composited in order (later fields over earlier), agents
/// on top. Lets the app colour only the cells on screen.
pub struct Colorer<'a> {
    world: &'a World,
    styles: Vec<Style>,
    agents: std::collections::HashSet<u32>,
    background: Rgba,
}

impl<'a> Colorer<'a> {
    pub fn new(world: &'a World) -> Colorer<'a> {
        let background = backdrop(world).0;
        Colorer {
            world,
            background,
            styles: (0..world.fields.len())
                .map(|f| style_for(world, f, background))
                .collect(),
            agents: world
                .cas
                .iter()
                .flat_map(|c| c.agents.iter().map(|a| a.cell))
                .collect(),
        }
    }

    pub fn color(&self, i: usize) -> Rgba {
        if !self.agents.is_empty() && self.agents.contains(&(i as u32)) {
            return AGENT;
        }
        let mut out = self.background;
        let topo = &self.world.topo;
        let lim = (401f64).ln();
        for (f, style) in self.world.fields.iter().zip(&self.styles) {
            let c = match style {
                Style::Height { lut, nb } => {
                    let h = f.height[i] as usize;
                    if h == 0 {
                        continue;
                    }
                    if h >= topo.degree(i, *nb) {
                        lut[lut.len() - 1]
                    } else {
                        lut[h.min(lut.len() - 2)]
                    }
                }
                Style::Row { lut } => {
                    let on = if f.kind == FieldKind::Height {
                        f.height[i] != 0
                    } else {
                        f.state[i] != 0
                    };
                    if !on {
                        continue;
                    }
                    let r = row_fraction(topo, i).clamp(0.0, 1.0);
                    lut[(r * (lut.len() - 1) as f32).round() as usize]
                }
                Style::Value { lut } => {
                    let s = f.state[i] as usize;
                    if s == 0 {
                        continue;
                    }
                    lut[s]
                }
                Style::Wire => match f.state[i] {
                    1 => [0x6c, 0xd0, 0xff, 0xff],
                    2 => [0xff, 0x5c, 0x5c, 0xff],
                    3 => [0xc9, 0x93, 0x1e, 0xff],
                    _ => continue,
                },
                Style::States { lut, zero } => {
                    let s = f.state[i] as usize;
                    if s == 0 && !zero {
                        continue;
                    }
                    lut[s.min(lut.len() - 1)]
                }
                Style::Signed {
                    pos,
                    neg,
                    by_age,
                    ca,
                    glow,
                } => {
                    let s = f.state[i];
                    if s == 0 {
                        let g = if *glow {
                            self.world.cas[*ca]
                                .heat
                                .as_ref()
                                .and_then(|h| h.glow.get(i).copied())
                                .unwrap_or(0)
                        } else {
                            0
                        };
                        if g == 0 {
                            continue;
                        }
                        mix(out, GLOW, g as f64 / 255.0)
                    } else {
                        let lut = if s == crate::jaca::NEG { neg } else { pos };
                        if *by_age {
                            let fr = (1.0 + f.age[i] as f64).ln() / lim;
                            lut[((fr.min(1.0)) * (lut.len() - 1) as f64).round() as usize]
                        } else {
                            lut[0]
                        }
                    }
                }
                Style::Age { lut, states } => {
                    let s = f.state[i];
                    if s == 0 {
                        continue;
                    }
                    let fr = if s > 1 {
                        0.45 + 0.5 * (s - 1) as f64 / (*states as f64 - 1.0).max(1.0)
                    } else {
                        (1.0 + f.age[i] as f64).ln() / lim
                    };
                    lut[((fr.min(1.0)) * (AGE_LEVELS - 1) as f64).round() as usize]
                }
            };
            out = c;
        }
        out
    }
}

fn mix(a: Rgba, b: Rgba, t: f64) -> Rgba {
    let m = |x: u8, y: u8| (x as f64 + (y as f64 - x as f64) * t).round() as u8;
    [m(a[0], b[0]), m(a[1], b[1]), m(a[2], b[2]), 0xff]
}

/// One colour per cell.
pub fn cell_colors(world: &World) -> Vec<Rgba> {
    let c = Colorer::new(world);
    (0..world.topo.len())
        .into_par_iter()
        .with_min_len(8192)
        .map(|i| c.color(i))
        .collect()
}

/// Colours for the given cells, in order.
pub fn colors_for(world: &World, cells: &[u32]) -> Vec<Rgba> {
    let c = Colorer::new(world);
    cells
        .par_iter()
        .with_min_len(8192)
        .map(|&i| c.color(i as usize))
        .collect()
}

#[derive(Clone)]
pub struct Image {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

#[derive(Clone, Debug)]
pub struct RenderOptions {
    /// Pixels per world unit. `None` fits the whole tiling into `max_side`.
    pub scale: Option<f64>,
    pub max_side: u32,
    /// Draw tile outlines when tiles are at least this many pixels across (0 = never).
    pub outline_min_px: f64,
}
impl Default for RenderOptions {
    fn default() -> Self {
        RenderOptions {
            scale: None,
            max_side: 1600,
            outline_min_px: 7.0,
        }
    }
}

/// Pixel size and scale (pixels per world unit) of a render.
pub fn render_size(topo: &Topology, opts: &RenderOptions) -> (u32, u32, f64) {
    let b = topo.bounds;
    let (bw, bh) = ((b[2] - b[0]).max(1e-9), (b[3] - b[1]).max(1e-9));
    let s = opts
        .scale
        .unwrap_or_else(|| opts.max_side as f64 / bw.max(bh));
    (
        ((bw * s).ceil() as u32).max(1),
        ((bh * s).ceil() as u32).max(1),
        s,
    )
}

/// Cells whose outline may reach pixel rows `y0..y1` (pixel y = (world y - bounds top) * scale).
fn cells_in_rows(topo: &Topology, s: f64, y0: f64, y1: f64) -> Vec<usize> {
    let top = topo.bounds[1];
    let (wy0, wy1) = (top + y0 / s - 1e-9, top + y1 / s + 1e-9);
    match &topo.storage {
        crate::topology::Storage::Lattice(g) => {
            // Each type's outline spans [lo, hi] in y relative to its lattice point.
            let spans: Vec<(f64, f64)> = g
                .types
                .iter()
                .map(|(m, sh)| {
                    topo.outlines[*sh]
                        .iter()
                        .fold((f64::MAX, f64::MIN), |(lo, hi), &p| {
                            let y = m.apply(p)[1];
                            (lo.min(y), hi.max(y))
                        })
                })
                .collect();
            let lo = spans.iter().map(|x| x.0).fold(f64::MAX, f64::min);
            let hi = spans.iter().map(|x| x.1).fold(f64::MIN, f64::max);
            let ay = g.a2[1];
            let ja = (((wy0 - hi) / ay).floor() as i64).max(0);
            let jb = (((wy1 - lo) / ay).ceil() as i64).min(g.h as i64 - 1);
            let mut out = Vec::new();
            for j in ja..=jb {
                for u in 0..g.w as i32 {
                    for (k, sp) in spans.iter().enumerate() {
                        let y = j as f64 * ay;
                        if y + sp.1 >= wy0 && y + sp.0 <= wy1 {
                            out.push(g.index(u, j as i32, k as u32));
                        }
                    }
                }
            }
            out
        }
        crate::topology::Storage::Explicit(_) => (0..topo.len())
            .filter(|&i| {
                let m = topo.transform(i);
                let o = &topo.outlines[topo.shape_of(i)];
                let (lo, hi) = o.iter().fold((f64::MAX, f64::MIN), |(lo, hi), &p| {
                    let y = m.apply(p)[1];
                    (lo.min(y), hi.max(y))
                });
                hi >= wy0 && lo <= wy1
            })
            .collect(),
    }
}

/// Render pixel rows `y0..y1` of the full image as a strip.
fn render_rows(
    world: &World,
    color: &(dyn Fn(usize) -> Rgba + Sync),
    opts: &RenderOptions,
    y0: u32,
    y1: u32,
) -> Image {
    let (w, _, s) = render_size(&world.topo, opts);
    render_rows_at(
        world,
        color,
        opts,
        y0,
        y1,
        w,
        s,
        [0.0, 0.0],
        &ViewColors::for_world(world),
    )
}

/// The colours a render uses outside the tiling and for tile outlines.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ViewColors {
    /// Pixels no tile covers.
    pub stage: Rgba,
    pub outline: Rgba,
}
impl Default for ViewColors {
    fn default() -> Self {
        ViewColors {
            stage: STAGE,
            outline: OUTLINE,
        }
    }
}
impl ViewColors {
    /// The world's backdrop palette's stage and outline (see [`backdrop`]).
    pub fn for_world(world: &World) -> ViewColors {
        let p = backdrop_palette(world);
        ViewColors {
            stage: p.stage_rgba(),
            outline: p.outline_rgba(),
        }
    }
}

/// Render a window of the world with given cell colours: pixel (0, 0) at world point `view`,
/// `width` x `height` pixels at `scale` pixels per world unit. Tile outlines are drawn when
/// tiles are at least `opts.outline_min_px` across (`opts.scale` and `opts.max_side` are
/// ignored). Used by `media` for moving windows; see [`render_view`] for the world's own colours.
#[allow(clippy::too_many_arguments)]
pub fn render_rect(
    world: &World,
    color: &(dyn Fn(usize) -> Rgba + Sync),
    view: [f64; 2],
    scale: f64,
    width: u32,
    height: u32,
    opts: &RenderOptions,
    colors: &ViewColors,
) -> Image {
    let b = world.topo.bounds;
    let origin = [(b[0] - view[0]) * scale, (b[1] - view[1]) * scale];
    render_rows_at(world, color, opts, 0, height, width, scale, origin, colors)
}

/// Render pixel rows `y0..y1` of an image `width` pixels wide at scale `s`, with the tiling's
/// top-left bounds corner at pixel `origin`.
#[allow(clippy::too_many_arguments)]
fn render_rows_at(
    world: &World,
    color: &(dyn Fn(usize) -> Rgba + Sync),
    opts: &RenderOptions,
    y0: u32,
    y1: u32,
    w: u32,
    s: f64,
    origin: [f64; 2],
    colors: &ViewColors,
) -> Image {
    let h = y1 - y0;
    let mut img = Image {
        width: w,
        height: h,
        rgba: colors.stage.repeat((w as usize) * (h as usize)),
    };
    draw_rows_at(&mut img, world, color, opts, y0, s, origin, colors);
    img
}

/// Draw the tiling into `img`, which holds pixel rows `y0..y0 + img.height` of a larger image,
/// at scale `s` with the tiling's top-left bounds corner at pixel `origin`. Pixels no tile
/// covers keep their colour.
#[allow(clippy::too_many_arguments)]
fn draw_rows_at(
    img: &mut Image,
    world: &World,
    color: &(dyn Fn(usize) -> Rgba + Sync),
    opts: &RenderOptions,
    y0: u32,
    s: f64,
    origin: [f64; 2],
    colors: &ViewColors,
) {
    let topo: &Topology = &world.topo;
    let b = topo.bounds;
    let y1 = y0 + img.height;
    let w = img.width as f64;
    let to_px = |p: P| {
        [
            (p[0] - b[0]) * s + origin[0],
            (p[1] - b[1]) * s + origin[1] - y0 as f64,
        ]
    };
    let tile_px = (crate::geom::area(&topo.shapes[0]).sqrt()) * s;
    let outline = opts.outline_min_px > 0.0 && tile_px >= opts.outline_min_px;
    let mut cells = cells_in_rows(topo, s, y0 as f64 - origin[1], y1 as f64 - origin[1]);
    // Leave out cells far outside the image's columns (a window of a large or repeated tiling).
    let margin = topo
        .outlines
        .iter()
        .flatten()
        .fold(0f64, |m, p| m.max(p[0].abs()).max(p[1].abs()))
        * 2.0
        * s
        + 2.0;
    if origin[0] > margin || (b[2] - b[0]) * s + origin[0] > w + margin {
        cells.retain(|&i| {
            let x = (topo.transform(i).translation()[0] - b[0]) * s + origin[0];
            x > -margin && x < w + margin
        });
    }
    let mut poly: Vec<P> = Vec::new();
    let place = |i: usize, poly: &mut Vec<P>| {
        let m = topo.transform(i);
        poly.clear();
        poly.extend(
            topo.outlines[topo.shape_of(i)]
                .iter()
                .map(|&p| to_px(m.apply(p))),
        );
    };
    for &i in &cells {
        place(i, &mut poly);
        fill_polygon(img, &poly, color(i));
    }
    // Von Neumann 29-state: an arrow on each transmission cell shows its output direction.
    if tile_px >= 10.0 {
        for (fi, f) in world.fields.iter().enumerate() {
            if !world
                .cas
                .iter()
                .any(|c| c.field == fi && c.rule == Rule::VonNeumann)
            {
                continue;
            }
            for &i in &cells {
                if let Some(tri) = crate::vonneumann::arrow_triangle(topo, i, f.state[i]) {
                    fill_polygon(img, &tri.map(to_px), STAGE);
                }
            }
        }
    }
    if outline {
        for &i in &cells {
            place(i, &mut poly);
            for k in 0..poly.len() {
                draw_line(img, poly[k], poly[(k + 1) % poly.len()], colors.outline);
            }
        }
    }
}

/// Rasterise the world: each cell's (curved) outline filled with its colour.
pub fn render(world: &World, opts: &RenderOptions) -> Image {
    let (_, h, _) = render_size(&world.topo, opts);
    let c = Colorer::new(world);
    render_rows(world, &|i| c.color(i), opts, 0, h)
}

/// Render into an image of exactly `width` x `height` pixels: the whole tiling as large as fits,
/// centred (scripts' screen-sized pictures). `opts.scale` and `opts.max_side` are ignored.
pub fn render_fitted(world: &World, width: u32, height: u32, opts: &RenderOptions) -> Image {
    let b = world.topo.bounds;
    let (bw, bh) = ((b[2] - b[0]).max(1e-9), (b[3] - b[1]).max(1e-9));
    let s = (width as f64 / bw).min(height as f64 / bh);
    let origin = [
        ((width as f64 - bw * s) / 2.0).floor(),
        ((height as f64 - bh * s) / 2.0).floor(),
    ];
    let c = Colorer::new(world);
    render_rows_at(
        world,
        &|i| c.color(i),
        opts,
        0,
        height,
        width,
        s,
        origin,
        &ViewColors::for_world(world),
    )
}

/// Options for [`render_view`].
#[derive(Clone, Debug)]
pub struct ViewOptions {
    /// Draw tile outlines when tiles are at least this many pixels across (0 = never).
    pub outline_min_px: f64,
    /// Wrap-edge lattices: draw the torus's copies around the grid too, so a view larger than
    /// the grid, or off its edge, tiles seamlessly. Ignored for other tilings.
    pub repeat: bool,
    /// Stage and outline colours; `None` uses the world's backdrop palette.
    pub colors: Option<ViewColors>,
}
impl Default for ViewOptions {
    fn default() -> Self {
        ViewOptions {
            outline_min_px: 7.0,
            repeat: false,
            colors: None,
        }
    }
}

/// The torus translations of a wrap-edge lattice (world units): one grid width across, and one
/// grid height down (with the rows' wrap shift).
pub fn torus_periods(topo: &Topology) -> Option<(P, P)> {
    let g = topo.lattice().filter(|g| g.wrap)?;
    let t1 = [g.a1[0] * g.w as f64, g.a1[1] * g.w as f64];
    let ws = g.wrap_shift as f64;
    let t2 = [
        g.a2[0] * g.h as f64 + g.a1[0] * ws,
        g.a2[1] * g.h as f64 + g.a1[1] * ws,
    ];
    Some((t1, t2))
}

/// World point at the top-left corner of pixel (0, 0) of a `width` x `height` view centred on
/// `center` at `scale` pixels per world unit.
pub fn view_corner(width: u32, height: u32, center: P, scale: f64) -> P {
    [
        center[0] - width as f64 / (2.0 * scale),
        center[1] - height as f64 / (2.0 * scale),
    ]
}

/// Render any part of the world at any scale into an image of exactly `width` x `height`
/// pixels: world point `center` at the image centre, `scale` pixels per world unit, the world's
/// own colours (empty cells in the backdrop palette's background, outside the tiling in its
/// stage). With `opts.repeat` a wrap-edge lattice repeats in every direction.
pub fn render_view(
    world: &World,
    width: u32,
    height: u32,
    center: P,
    scale: f64,
    opts: &ViewOptions,
) -> Image {
    let c = Colorer::new(world);
    render_view_with(world, &|i| c.color(i), width, height, center, scale, opts)
}

/// [`render_view`] with each cell's colour from `color` (a cell index map, for instance, as the
/// Quick Start cards' framed previews use).
pub fn render_view_with(
    world: &World,
    color: &(dyn Fn(usize) -> Rgba + Sync),
    width: u32,
    height: u32,
    center: P,
    scale: f64,
    opts: &ViewOptions,
) -> Image {
    let mut img = Image {
        width,
        height,
        rgba: Vec::new(),
    };
    let colors = opts.colors.unwrap_or_else(|| ViewColors::for_world(world));
    let strip = view_strip(width);
    let mut strips: Vec<Image> = (0..height.div_ceil(strip))
        .into_par_iter()
        .map(|k| {
            let y0 = k * strip;
            view_rows(
                world,
                color,
                (width, y0, (y0 + strip).min(height)),
                (height, center, scale),
                opts,
                &colors,
            )
        })
        .collect();
    img.rgba.reserve(width as usize * height as usize * 4);
    for s in strips.iter_mut() {
        img.rgba.append(&mut s.rgba);
    }
    img
}

/// Rows per strip when rendering views in parallel (about 4 MB each).
fn view_strip(width: u32) -> u32 {
    ((4 << 20) / (width.max(1) as usize * 4)).clamp(8, 1024) as u32
}

/// Rows `y0..y1` of a view `(width, y0, y1)` of an image `height` pixels tall.
fn view_rows(
    world: &World,
    color: &(dyn Fn(usize) -> Rgba + Sync),
    (width, y0, y1): (u32, u32, u32),
    (height, center, scale): (u32, P, f64),
    opts: &ViewOptions,
    colors: &ViewColors,
) -> Image {
    let b = world.topo.bounds;
    let corner = view_corner(width, height, center, scale);
    let origin = [(b[0] - corner[0]) * scale, (b[1] - corner[1]) * scale];
    let ro = RenderOptions {
        scale: Some(scale),
        max_side: 0,
        outline_min_px: opts.outline_min_px,
    };
    let mut img = Image {
        width,
        height: y1 - y0,
        rgba: colors.stage.repeat(width as usize * (y1 - y0) as usize),
    };
    let periods = opts.repeat.then(|| torus_periods(&world.topo)).flatten();
    let Some((t1, t2)) = periods else {
        draw_rows_at(&mut img, world, color, &ro, y0, scale, origin, colors);
        return img;
    };
    // Copies whose bounds (grown by a tile, for ragged edges) meet the strip's world rectangle.
    let pad = crate::world::cell_size(&world.topo) * 2.0;
    let view = [
        corner[0],
        corner[1] + y0 as f64 / scale,
        corner[0] + width as f64 / scale,
        corner[1] + y1 as f64 / scale,
    ];
    if t2[1].abs() < 1e-12 || t1[0].abs() < 1e-12 {
        draw_rows_at(&mut img, world, color, &ro, y0, scale, origin, colors);
        return img;
    }
    let n2a = ((view[1] - b[3] - pad) / t2[1]).floor() as i64;
    let n2b = ((view[3] - b[1] + pad) / t2[1]).ceil() as i64;
    for n2 in n2a.min(n2b)..=n2a.max(n2b) {
        let sx = n2 as f64 * t2[0];
        let sy = n2 as f64 * t2[1];
        if b[3] + sy + pad < view[1] || b[1] + sy - pad > view[3] {
            continue;
        }
        let n1a = ((view[0] - b[2] - sx - pad) / t1[0]).floor() as i64;
        let n1b = ((view[2] - b[0] - sx + pad) / t1[0]).ceil() as i64;
        for n1 in n1a.min(n1b)..=n1a.max(n1b) {
            let dx = n1 as f64 * t1[0] + sx;
            let dy = n1 as f64 * t1[1] + sy;
            let o = [origin[0] + dx * scale, origin[1] + dy * scale];
            draw_rows_at(&mut img, world, color, &ro, y0, scale, o, colors);
        }
    }
    img
}

/// Render with given per-cell colours (e.g. objects coloured by class).
pub fn render_with_colors(world: &World, colors: &[Rgba], opts: &RenderOptions) -> Image {
    let (_, h, _) = render_size(&world.topo, opts);
    render_rows(
        world,
        &|i| colors.get(i).copied().unwrap_or(BACKGROUND),
        opts,
        0,
        h,
    )
}

/// Render straight to a PNG file in horizontal strips, so images far larger than memory allows
/// (tens of thousands of pixels on a side) can be written. Strips render in parallel batches.
pub fn write_png_streaming(
    path: &std::path::Path,
    world: &World,
    opts: &RenderOptions,
    text: &[(&str, String)],
) -> Result<(u32, u32), String> {
    let (w, h, _) = render_size(&world.topo, opts);
    if w as u64 * h as u64 > 2_000_000_000 {
        return Err(format!(
            "{w} x {h} pixels is too large (limit 2 billion pixels)"
        ));
    }
    let file = std::fs::File::create(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut enc = png::Encoder::new(std::io::BufWriter::new(file), w, h);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    for (k, v) in text {
        enc.add_itxt_chunk(k.to_string(), v.clone())
            .map_err(|e| e.to_string())?;
    }
    let mut wr = enc.write_header().map_err(|e| e.to_string())?;
    let mut stream = wr.stream_writer().map_err(|e| e.to_string())?;
    let c = Colorer::new(world);
    // About 16 MB per strip, a batch of strips per thread.
    let strip = ((16 << 20) / (w as usize * 4)).clamp(16, 4096) as u32;
    let batch = rayon::current_num_threads().max(1) as u32;
    let mut y = 0;
    while y < h {
        let starts: Vec<u32> = (0..batch)
            .map(|k| y + k * strip)
            .filter(|&s| s < h)
            .collect();
        let imgs: Vec<Image> = starts
            .par_iter()
            .map(|&s| render_rows(world, &|i| c.color(i), opts, s, (s + strip).min(h)))
            .collect();
        for img in imgs {
            std::io::Write::write_all(&mut stream, &img.rgba).map_err(|e| e.to_string())?;
        }
        y = starts.last().map(|s| (s + strip).min(h)).unwrap_or(h);
    }
    stream.finish().map_err(|e| e.to_string())?;
    Ok((w, h))
}

pub(crate) fn fill_polygon(img: &mut Image, poly: &[P], c: Rgba) {
    let bb = bbox(poly);
    let y0 = (bb[1] - 0.5).ceil().max(0.0) as i64;
    let y1 = ((bb[3] - 0.5).floor() as i64).min(img.height as i64 - 1);
    let mut xs: Vec<f64> = Vec::with_capacity(8);
    let n = poly.len();
    for y in y0..=y1 {
        let yc = y as f64 + 0.5;
        xs.clear();
        for k in 0..n {
            let (p, q) = (poly[k], poly[(k + 1) % n]);
            if (p[1] <= yc) != (q[1] <= yc) {
                xs.push(p[0] + (yc - p[1]) / (q[1] - p[1]) * (q[0] - p[0]));
            }
        }
        xs.sort_by(|a, b| a.partial_cmp(b).unwrap());
        for pair in xs.as_chunks::<2>().0 {
            let xa = ((pair[0] - 0.5).ceil().max(0.0)) as i64;
            let xb = ((pair[1] - 0.5).ceil() as i64).min(img.width as i64);
            let row = (y as usize) * img.width as usize;
            for x in xa..xb {
                let o = (row + x as usize) * 4;
                img.rgba[o..o + 4].copy_from_slice(&c);
            }
        }
    }
}

pub(crate) fn draw_line(img: &mut Image, a: P, b: P, c: Rgba) {
    let steps = (b[0] - a[0]).abs().max((b[1] - a[1]).abs()).ceil().max(1.0) as usize;
    for k in 0..=steps {
        let t = k as f64 / steps as f64;
        let x = (a[0] + (b[0] - a[0]) * t).floor() as i64;
        let y = (a[1] + (b[1] - a[1]) * t).floor() as i64;
        if x >= 0 && y >= 0 && x < img.width as i64 && y < img.height as i64 {
            let o = ((y as usize) * img.width as usize + x as usize) * 4;
            img.rgba[o..o + 4].copy_from_slice(&c);
        }
    }
}

/// Write a PNG with text chunks (e.g. scene JSON, JETCARP version, state hash).
pub fn write_png(
    path: &std::path::Path,
    img: &Image,
    text: &[(&str, String)],
) -> Result<(), String> {
    let file = std::fs::File::create(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let w = std::io::BufWriter::new(file);
    let mut enc = png::Encoder::new(w, img.width, img.height);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    for (k, v) in text {
        enc.add_itxt_chunk(k.to_string(), v.clone())
            .map_err(|e| e.to_string())?;
    }
    let mut wr = enc.write_header().map_err(|e| e.to_string())?;
    wr.write_image_data(&img.rgba).map_err(|e| e.to_string())?;
    wr.finish().map_err(|e| e.to_string())
}

/// Metadata saved with every exported image: version, generation, hash, boundary flag, scene.
/// The scene JSON a picture JETCARP wrote carries (its `jetcarp:scene` text chunk), from the
/// PNG's bytes: `Ok(None)` when it has none.
pub fn png_scene(bytes: &[u8]) -> Result<Option<String>, String> {
    let dec = png::Decoder::new(std::io::Cursor::new(bytes));
    let reader = dec.read_info().map_err(|e| format!("PNG: {e}"))?;
    let info = reader.info();
    for t in &info.utf8_text {
        if t.keyword == "jetcarp:scene" {
            return t.get_text().map(Some).map_err(|e| format!("PNG: {e}"));
        }
    }
    for t in &info.uncompressed_latin1_text {
        if t.keyword == "jetcarp:scene" {
            return Ok(Some(t.text.clone()));
        }
    }
    Ok(None)
}

pub fn png_metadata(world: &World) -> Vec<(&'static str, String)> {
    let r = world.record();
    vec![
        ("jetcarp:version", r.jetcarp_version.clone()),
        ("jetcarp:generation", r.generation.to_string()),
        ("jetcarp:hash", r.hash.clone()),
        ("jetcarp:dead_boundary", r.dead_boundary.to_string()),
        ("jetcarp:scene", world.to_scene().to_json()),
    ]
}
