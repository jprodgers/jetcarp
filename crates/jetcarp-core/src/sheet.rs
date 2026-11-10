//! Contact sheets: one thumbnail per rule number, each run from the same seed, labelled.

use crate::render::{Image, RenderOptions, STAGE, render};
use crate::world::{CaSpec, Command, SeedSpec, World};
use crate::{Boundary, TilingSpec};
use rayon::prelude::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SheetKind {
    /// Elementary rules on squares, 0-255.
    Wolfram,
    /// Binary hexagon rules with two parents, 0-15.
    HexBinary,
    /// Ternary hexagon rules with two parents, 0-19682.
    HexTernary,
    /// Binary flat-top hexagon rules with three parents, 0-255.
    HexTri,
    /// Ternary flat-top hexagon rules with three parents, 0 to 3^27 - 1.
    HexTri3,
    /// Totalistic ternary three-parent rules, 0-2186.
    HexTri3Sum,
}

impl SheetKind {
    pub fn max(&self) -> u64 {
        match self {
            SheetKind::Wolfram => 255,
            SheetKind::HexBinary => 15,
            SheetKind::HexTernary => 19682,
            SheetKind::HexTri => 255,
            SheetKind::HexTri3 => 7_625_597_484_986,
            SheetKind::HexTri3Sum => 2186,
        }
    }
    pub fn notation(&self, n: u64) -> String {
        match self {
            SheetKind::Wolfram => format!("W{n}"),
            SheetKind::HexBinary => format!("HexW {n}"),
            SheetKind::HexTernary => format!("HexW3 {n}"),
            SheetKind::HexTri => format!("HexT {n}"),
            SheetKind::HexTri3 => format!("HexT3 {n}"),
            SheetKind::HexTri3Sum => format!("HexT3 sum {n}"),
        }
    }
    pub fn family(&self) -> &'static str {
        match self {
            SheetKind::Wolfram => "square",
            SheetKind::HexTri | SheetKind::HexTri3 | SheetKind::HexTri3Sum => "hex_flat",
            _ => "hex",
        }
    }
    /// The sheet kind for a rule that has one (elementary rules), and its number.
    pub fn of_rule(rule: &crate::rules::Rule) -> Option<(SheetKind, u64)> {
        use crate::rules::{HexTMode, Rule};
        Some(match *rule {
            Rule::Wolfram(n) => (SheetKind::Wolfram, n as u64),
            Rule::HexWolfram { states: 2, number } => (SheetKind::HexBinary, number as u64),
            Rule::HexWolfram { number, .. } => (SheetKind::HexTernary, number as u64),
            Rule::HexTri {
                states: 2,
                mode: HexTMode::Full,
                number,
            } => (SheetKind::HexTri, number),
            Rule::HexTri {
                states: 3,
                mode: HexTMode::Full,
                number,
            } => (SheetKind::HexTri3, number),
            Rule::HexTri {
                states: 3,
                mode: HexTMode::Sum,
                number,
            } => (SheetKind::HexTri3Sum, number),
            _ => return None,
        })
    }
    pub fn parse(s: &str) -> Option<SheetKind> {
        match s.to_ascii_lowercase().as_str() {
            "w" | "wolfram" | "elementary" => Some(SheetKind::Wolfram),
            "hexw" | "hex2" | "hexw2" => Some(SheetKind::HexBinary),
            "hexw3" | "hex3" => Some(SheetKind::HexTernary),
            "hext" | "hext2" => Some(SheetKind::HexTri),
            "hext3" => Some(SheetKind::HexTri3),
            "hext3sum" | "hext3-sum" => Some(SheetKind::HexTri3Sum),
            _ => None,
        }
    }
}

#[derive(Clone, Debug)]
pub struct SheetOptions {
    pub kind: SheetKind,
    pub first: u64,
    pub count: u32,
    pub cols: u32,
    /// Cells per row in each thumbnail; rows = width / 2 so the light cone fits.
    pub width: u32,
    /// Pixels per thumbnail side (longest).
    pub thumb_px: u32,
    /// `None`: one live cell; `Some(d)`: random top row with this density.
    pub random_row: Option<f64>,
    pub rng_state: u64,
    pub palette: String,
}

impl Default for SheetOptions {
    fn default() -> Self {
        SheetOptions {
            kind: SheetKind::Wolfram,
            first: 0,
            count: 64,
            cols: 8,
            width: 81,
            thumb_px: 180,
            random_row: None,
            rng_state: 1,
            palette: "Ember".into(),
        }
    }
}

const GLYPHS: [[u8; 5]; 10] = [
    [0b111, 0b101, 0b101, 0b101, 0b111],
    [0b010, 0b110, 0b010, 0b010, 0b111],
    [0b111, 0b001, 0b111, 0b100, 0b111],
    [0b111, 0b001, 0b111, 0b001, 0b111],
    [0b101, 0b101, 0b111, 0b001, 0b001],
    [0b111, 0b100, 0b111, 0b001, 0b111],
    [0b111, 0b100, 0b111, 0b101, 0b111],
    [0b111, 0b001, 0b001, 0b001, 0b001],
    [0b111, 0b101, 0b111, 0b101, 0b111],
    [0b111, 0b101, 0b111, 0b001, 0b111],
];

/// Draw a number with the built-in 3x5 digit font.
pub fn draw_number(img: &mut Image, x: u32, y: u32, n: u64, scale: u32, color: [u8; 4]) {
    for (k, ch) in n.to_string().bytes().enumerate() {
        let g = GLYPHS[(ch - b'0') as usize];
        for (row, bits) in g.iter().enumerate() {
            for col in 0..3 {
                if bits >> (2 - col) & 1 == 1 {
                    for dy in 0..scale {
                        for dx in 0..scale {
                            let px = x + (k as u32 * 4 + col) * scale + dx;
                            let py = y + row as u32 * scale + dy;
                            if px < img.width && py < img.height {
                                let o = ((py * img.width + px) * 4) as usize;
                                img.rgba[o..o + 4].copy_from_slice(&color);
                            }
                        }
                    }
                }
            }
        }
    }
}

/// Run each rule and lay the thumbnails out in a grid, labelled with the rule number.
pub fn contact_sheet(o: &SheetOptions) -> Result<Image, String> {
    let last = (o.first + o.count.max(1) as u64 - 1).min(o.kind.max());
    let numbers: Vec<u64> = (o.first.min(last)..=last).collect();
    // Rows = width / 2 so the light cone fits. Flat-top hexagons: `width` unit cells are
    // 2 x width columns and the cone spreads two columns per row, so the same ratio fits.
    let h = (o.width / 2).max(8);
    let tiling = TilingSpec::rect(o.kind.family(), o.width.max(8), h, Boundary::Dead);
    let gens = if o.kind.family() == "hex_flat" {
        2 * h as u64 - 1
    } else {
        h as u64
    };
    let thumbs: Result<Vec<Image>, String> = numbers
        .par_iter()
        .map(|&n| {
            let mut w = World::new(tiling.clone(), o.rng_state)?;
            let seed = match o.random_row {
                Some(d) => SeedSpec::TopRandom { density: d },
                None => SeedSpec::TopCenter,
            };
            let mut ca = CaSpec::new(&o.kind.notation(n)).seed(seed);
            ca.palette = Some(o.palette.clone());
            w.apply(Command::AddCa { ca })?;
            w.apply(Command::Step { gens })?;
            Ok(render(
                &w,
                &RenderOptions {
                    max_side: o.thumb_px,
                    outline_min_px: 0.0,
                    ..Default::default()
                },
            ))
        })
        .collect();
    let thumbs = thumbs?;
    let (tw, th) = (
        thumbs.iter().map(|t| t.width).max().unwrap_or(1),
        thumbs.iter().map(|t| t.height).max().unwrap_or(1),
    );
    let label = 18;
    let pad = 8;
    let cols = o.cols.max(1).min(numbers.len() as u32);
    let rows = (numbers.len() as u32).div_ceil(cols);
    let (cw, ch) = (tw + pad, th + label + pad);
    let mut img = Image {
        width: cols * cw + pad,
        height: rows * ch + pad,
        rgba: vec![0; 0],
    };
    img.rgba = STAGE
        .iter()
        .copied()
        .cycle()
        .take((img.width * img.height * 4) as usize)
        .collect();
    for (k, (t, &n)) in thumbs.iter().zip(&numbers).enumerate() {
        let (cx, cy) = ((k as u32 % cols) * cw + pad, (k as u32 / cols) * ch + pad);
        draw_number(&mut img, cx, cy + 2, n, 2, [0xd0, 0xd4, 0xdc, 0xff]);
        for y in 0..t.height {
            let src = (y * t.width * 4) as usize;
            let dst = (((cy + label + y) * img.width + cx) * 4) as usize;
            img.rgba[dst..dst + (t.width * 4) as usize]
                .copy_from_slice(&t.rgba[src..src + (t.width * 4) as usize]);
        }
    }
    Ok(img)
}

/// 3x5 glyph for a letter or symbol (digits use `GLYPHS`); lower case draws as upper case.
fn glyph(ch: char) -> [u8; 5] {
    if let Some(d) = ch.to_digit(10) {
        return GLYPHS[d as usize];
    }
    match ch.to_ascii_uppercase() {
        'A' => [0b010, 0b101, 0b111, 0b101, 0b101],
        'B' => [0b110, 0b101, 0b110, 0b101, 0b110],
        'C' => [0b011, 0b100, 0b100, 0b100, 0b011],
        'D' => [0b110, 0b101, 0b101, 0b101, 0b110],
        'E' => [0b111, 0b100, 0b110, 0b100, 0b111],
        'F' => [0b111, 0b100, 0b110, 0b100, 0b100],
        'G' => [0b011, 0b100, 0b101, 0b101, 0b011],
        'H' => [0b101, 0b101, 0b111, 0b101, 0b101],
        'I' => [0b111, 0b010, 0b010, 0b010, 0b111],
        'J' => [0b001, 0b001, 0b001, 0b101, 0b010],
        'K' => [0b101, 0b101, 0b110, 0b101, 0b101],
        'L' => [0b100, 0b100, 0b100, 0b100, 0b111],
        'M' => [0b101, 0b111, 0b111, 0b101, 0b101],
        'N' => [0b110, 0b101, 0b101, 0b101, 0b101],
        'O' => [0b010, 0b101, 0b101, 0b101, 0b010],
        'P' => [0b110, 0b101, 0b110, 0b100, 0b100],
        'Q' => [0b010, 0b101, 0b101, 0b110, 0b011],
        'R' => [0b110, 0b101, 0b110, 0b101, 0b101],
        'S' => [0b011, 0b100, 0b010, 0b001, 0b110],
        'T' => [0b111, 0b010, 0b010, 0b010, 0b010],
        'U' => [0b101, 0b101, 0b101, 0b101, 0b111],
        'V' => [0b101, 0b101, 0b101, 0b101, 0b010],
        'W' => [0b101, 0b101, 0b111, 0b111, 0b101],
        'X' => [0b101, 0b101, 0b010, 0b101, 0b101],
        'Y' => [0b101, 0b101, 0b010, 0b010, 0b010],
        'Z' => [0b111, 0b001, 0b010, 0b100, 0b111],
        '/' => [0b001, 0b001, 0b010, 0b100, 0b100],
        '-' => [0b000, 0b000, 0b111, 0b000, 0b000],
        '.' => [0b000, 0b000, 0b000, 0b000, 0b010],
        ',' => [0b000, 0b000, 0b000, 0b010, 0b100],
        ':' => [0b000, 0b010, 0b000, 0b010, 0b000],
        '(' => [0b010, 0b100, 0b100, 0b100, 0b010],
        ')' => [0b010, 0b001, 0b001, 0b001, 0b010],
        '%' => [0b101, 0b001, 0b010, 0b100, 0b101],
        '#' => [0b101, 0b111, 0b101, 0b111, 0b101],
        '+' => [0b000, 0b010, 0b111, 0b010, 0b000],
        '=' => [0b000, 0b111, 0b000, 0b111, 0b000],
        '&' => [0b010, 0b101, 0b010, 0b101, 0b011],
        _ => [0; 5],
    }
}

/// Draw text with the built-in 3x5 font (4 px per character at scale 1).
pub fn draw_text(img: &mut Image, x: u32, y: u32, text: &str, scale: u32, color: [u8; 4]) {
    for (k, ch) in text.chars().enumerate() {
        let g = glyph(ch);
        for (row, bits) in g.iter().enumerate() {
            for col in 0..3 {
                if bits >> (2 - col) & 1 == 1 {
                    for dy in 0..scale {
                        for dx in 0..scale {
                            let px = x + (k as u32 * 4 + col) * scale + dx;
                            let py = y + row as u32 * scale + dy;
                            if px < img.width && py < img.height {
                                let o = ((py * img.width + px) * 4) as usize;
                                img.rgba[o..o + 4].copy_from_slice(&color);
                            }
                        }
                    }
                }
            }
        }
    }
}

/// Label lines for [`labelled_sheet`]: text and colour.
pub type Label = Vec<(String, [u8; 4])>;

/// Lay thumbnails out in a grid, each under its label lines, in order.
pub fn labelled_sheet(items: &[(Label, &Image)], cols: u32) -> Image {
    labelled_sheet_scaled(items, cols, 2)
}

/// [`labelled_sheet`] with labels drawn at the largest scale up to `max_scale` (at least 1) at
/// which the longest label fits the thumbnail width.
pub fn labelled_sheet_scaled(items: &[(Label, &Image)], cols: u32, max_scale: u32) -> Image {
    let tw = items.iter().map(|t| t.1.width).max().unwrap_or(1);
    let th = items.iter().map(|t| t.1.height).max().unwrap_or(1);
    let lines = items.iter().map(|t| t.0.len()).max().unwrap_or(0) as u32;
    let longest = items
        .iter()
        .flat_map(|t| t.0.iter().map(|l| l.0.chars().count()))
        .max()
        .unwrap_or(0) as u32;
    let scale = (1..=max_scale.max(1))
        .rev()
        .find(|k| longest * 4 * k <= tw)
        .unwrap_or(1);
    let label = lines * 6 * scale + 4;
    let pad = 8;
    let cols = cols.max(1).min(items.len().max(1) as u32);
    let rows = (items.len() as u32).div_ceil(cols);
    let (cw, ch) = (tw + pad, th + label + pad);
    let mut img = Image {
        width: cols * cw + pad,
        height: rows * ch + pad,
        rgba: vec![],
    };
    img.rgba = STAGE
        .iter()
        .copied()
        .cycle()
        .take((img.width * img.height * 4) as usize)
        .collect();
    for (k, (text, t)) in items.iter().enumerate() {
        let (cx, cy) = ((k as u32 % cols) * cw + pad, (k as u32 / cols) * ch + pad);
        for (l, (s, c)) in text.iter().enumerate() {
            draw_text(&mut img, cx, cy + 2 + l as u32 * 6 * scale, s, scale, *c);
        }
        for y in 0..t.height {
            let src = (y * t.width * 4) as usize;
            let dst = (((cy + label + y) * img.width + cx) * 4) as usize;
            img.rgba[dst..dst + (t.width * 4) as usize]
                .copy_from_slice(&t.rgba[src..src + (t.width * 4) as usize]);
        }
    }
    img
}
