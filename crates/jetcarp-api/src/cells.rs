//! Cell coordinates, text patterns, and text pictures of a field.
//!
//! Lattice tilings (everything except aperiodic patches) are a `W x H` array of cells in index
//! order: `index = y * W + x`, row 0 at the top. On tilings with several tile types per unit cell
//! the `T` tiles of one unit cell sit next to each other in a row, so `W = unit cells across * T`.
//! Aperiodic patches have no grid; their cells are addressed by index only.

use jetcarp_core::Topology;
use jetcarp_core::world::Field;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Grid {
    pub w: usize,
    pub h: usize,
    /// Tiles per unit cell.
    pub types: usize,
    pub wrap: bool,
}

impl Grid {
    pub fn of(topo: &Topology) -> Option<Grid> {
        let g = topo.lattice()?;
        Some(Grid {
            w: g.w as usize * g.ntypes as usize,
            h: g.h as usize,
            types: g.ntypes as usize,
            wrap: g.wrap,
        })
    }
    pub fn xy(&self, i: usize) -> (usize, usize) {
        (i % self.w, i / self.w)
    }
    /// Index of `(x, y)`. Outside the grid: wraps on a torus, otherwise an error.
    pub fn index(&self, x: i64, y: i64) -> Result<usize, String> {
        let (w, h) = (self.w as i64, self.h as i64);
        let (x, y) = if self.wrap {
            (x.rem_euclid(w), y.rem_euclid(h))
        } else if x < 0 || y < 0 || x >= w || y >= h {
            return Err(format!(
                "cell ({x}, {y}) is outside the grid (x 0..{}, y 0..{})",
                w - 1,
                h - 1
            ));
        } else {
            (x, y)
        };
        Ok((y * w + x) as usize)
    }
}

/// One character per state: `.` for 0, `O` for live binary cells, then 1-9, a-z, and `#`.
pub fn state_char(v: u32, binary: bool) -> char {
    match v {
        0 => '.',
        1 if binary => 'O',
        1..=9 => char::from(b'0' + v as u8),
        10..=35 => char::from(b'a' + (v - 10) as u8),
        _ => '#',
    }
}

/// Value of a cell in a field (state, or grains for height fields).
pub fn value(f: &Field, i: usize) -> u32 {
    if f.kind == jetcarp_core::rules::FieldKind::Height {
        f.height[i]
    } else {
        f.state[i] as u32
    }
}

/// A pattern as `(dx, dy, state)` offsets from its top-left corner (non-zero cells only).
pub type Pattern = Vec<(i64, i64, u32)>;

/// Plain-text picture: one line per row; `.`, space, `0`, `b`, `_`, `-` are empty; `O`, `o`, `*`,
/// `#`, `X`, `x`, `A`, `@` are state 1; digits 1-9 are that state.
pub fn parse_plaintext(text: &str) -> Result<Pattern, String> {
    let mut out = Vec::new();
    let mut y = 0i64;
    for line in text.lines() {
        let line = line.trim_end_matches('\r');
        if line.starts_with('!') {
            // LifeWiki .cells comment line.
            continue;
        }
        for (x, c) in line.chars().enumerate() {
            let v = match c {
                '.' | ' ' | '0' | 'b' | '_' | '-' => 0,
                'O' | 'o' | '*' | '#' | 'X' | 'x' | 'A' | '@' => 1,
                '1'..='9' => c as u32 - '0' as u32,
                '\t' => return Err("tabs are not allowed in a pattern; use '.' for empty".into()),
                _ => {
                    return Err(format!(
                        "pattern character '{c}' at row {y}, column {x} is not understood. Use '.' for empty, 'O' for live, or digits 1-9 for states."
                    ));
                }
            };
            if v != 0 {
                out.push((x as i64, y, v));
            }
        }
        y += 1;
    }
    Ok(out)
}

/// Run-length encoded pattern (`bo$2bo$3o!`): `b`/`.` empty, `o` live, `A`-`X` states 1-24,
/// `$` ends a row, `!` ends the pattern. Lines starting with `#` and the `x = ..` header are skipped.
pub fn parse_rle(text: &str) -> Result<Pattern, String> {
    let mut out = Vec::new();
    let (mut x, mut y) = (0i64, 0i64);
    let mut num: Option<i64> = None;
    'lines: for line in text.lines() {
        let t = line.trim();
        if t.starts_with('#') || t.starts_with("x ") || t.starts_with("x=") {
            continue;
        }
        for c in t.chars() {
            if let Some(d) = c.to_digit(10) {
                let v = num.unwrap_or(0) * 10 + d as i64;
                if v > 100_000 {
                    return Err("RLE run count is too large".into());
                }
                num = Some(v);
                continue;
            }
            if c.is_whitespace() {
                continue;
            }
            let n = num.take().unwrap_or(1);
            match c {
                'b' | '.' => x += n,
                'o' => {
                    for k in 0..n {
                        out.push((x + k, y, 1));
                    }
                    x += n;
                }
                'A'..='X' => {
                    for k in 0..n {
                        out.push((x + k, y, c as u32 - 'A' as u32 + 1));
                    }
                    x += n;
                }
                '$' => {
                    y += n;
                    x = 0;
                }
                '!' => break 'lines,
                _ => return Err(format!("RLE character '{c}' is not understood")),
            }
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sorted(mut p: Pattern) -> Pattern {
        p.sort();
        p
    }

    #[test]
    fn glider_in_both_formats() {
        let a = parse_plaintext(".O.\n..O\nOOO").unwrap();
        let b = parse_rle("x = 3, y = 3, rule = B3/S23\nbo$2bo$3o!").unwrap();
        assert_eq!(sorted(a.clone()), sorted(b));
        assert_eq!(a.len(), 5);
    }

    #[test]
    fn rle_multi_digit_runs() {
        let p = parse_rle("12o$3b2o!").unwrap();
        assert_eq!(p.len(), 14);
        assert!(p.contains(&(11, 0, 1)));
        assert!(p.contains(&(3, 1, 1)) && p.contains(&(4, 1, 1)));
        let p = parse_rle("10bo!").unwrap();
        assert_eq!(p, vec![(10, 0, 1)]);
    }

    #[test]
    fn plaintext_states_and_errors() {
        let p = parse_plaintext("1.2\n..3").unwrap();
        assert_eq!(sorted(p), vec![(0, 0, 1), (2, 0, 2), (2, 1, 3)]);
        assert!(parse_plaintext("O?O").is_err());
    }
}
