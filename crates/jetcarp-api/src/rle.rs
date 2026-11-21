//! Writing a pattern as RLE, the run-length format Golly and LifeViewer read, and reading Golly's
//! hexagonal layout back (#145).
//!
//! Only what both programs read the same way for now: two-state B/S rules on the square grid
//! (Moore, or von Neumann written with a `V`) and on JETCARP's hexagon grid (`H`), and two-state
//! isotropic rules in Hensel's notation on the square grid (`B2-a/S12`, named as Golly names
//! them). Everything else is refused with its reason. The rest of the exchange with Golly and
//! LifeViewer is #152.
//!
//! **Hexagons.** JETCARP's hexagon grid has offset rows: an odd row sits half a cell to the right,
//! so a cell's neighbours depend on its row's parity. Golly and LifeViewer use a sheared square
//! grid instead, whose six neighbours are the Moore neighbourhood less its top-right and
//! bottom-left cells. The two match with `golly_x = x + ceil(y / 2)` and `y` unchanged: every
//! neighbour JETCARP lists lands on one of Golly's six (`hex_neighbours_match_golly`).

use crate::cells::{Grid, Pattern};
use jetcarp_core::rules::{Rule, notation_on};
use jetcarp_core::topology::Nb;
use jetcarp_core::world::World;

/// The longest line of a pattern's body, as Golly writes it.
const LINE: usize = 70;

/// Golly's (and LifeViewer's) column of JETCARP's hexagon-grid cell at `(x, y)`.
pub fn golly_hex_x(x: i64, y: i64) -> i64 {
    x + y.div_euclid(2) + y.rem_euclid(2)
}

/// JETCARP's hexagon-grid column of Golly's cell at `(gx, y)`.
pub fn grid_hex_x(gx: i64, y: i64) -> i64 {
    gx - y.div_euclid(2) - y.rem_euclid(2)
}

/// The rule an RLE text names in its `x = …, y = …, rule = …` line, if it has one.
pub fn header_rule(text: &str) -> Option<String> {
    let line = text
        .lines()
        .map(str::trim)
        .find(|l| l.starts_with("x ") || l.starts_with("x="))?;
    let at = line.find("rule")?;
    let rest = line[at + 4..].trim_start().strip_prefix('=')?;
    Some(rest.split(',').next()?.trim().to_string()).filter(|r| !r.is_empty())
}

/// Whether an RLE text's rule is written for Golly's hexagonal neighbourhood (`…H`).
pub fn is_golly_hex(text: &str) -> bool {
    header_rule(text).is_some_and(|r| r.ends_with('H') || r.ends_with('h'))
}

/// A pattern read from RLE in Golly's hexagonal layout, as cells of JETCARP's hexagon grid.
/// `row` is the grid row its first line will be placed on: the offset of the rows below it
/// depends on that row's parity, so the shape is kept wherever it is placed.
pub fn from_golly_hex(p: &Pattern, row: i64) -> Pattern {
    let shift = golly_hex_x(0, row);
    p.iter()
        .map(|&(gx, gy, v)| (grid_hex_x(gx + shift, row + gy), gy, v))
        .collect()
}

/// The RLE of these live cells: comments (`#C` lines), the `x = W, y = H, rule = …` line, and the
/// body, wrapped at 70 characters. The cells are shifted so the pattern starts at (0, 0).
pub fn write(cells: &[(i64, i64)], rule: &str, comments: &[String]) -> String {
    let mut out = String::new();
    for c in comments {
        out.push_str("#C ");
        out.push_str(c);
        out.push('\n');
    }
    if cells.is_empty() {
        out.push_str(&format!("x = 0, y = 0, rule = {rule}\n!\n"));
        return out;
    }
    let (x0, y0) = (
        cells.iter().map(|c| c.0).min().unwrap(),
        cells.iter().map(|c| c.1).min().unwrap(),
    );
    let (x1, y1) = (
        cells.iter().map(|c| c.0).max().unwrap(),
        cells.iter().map(|c| c.1).max().unwrap(),
    );
    out.push_str(&format!(
        "x = {}, y = {}, rule = {rule}\n",
        x1 - x0 + 1,
        y1 - y0 + 1
    ));
    let mut rows: Vec<Vec<i64>> = vec![Vec::new(); (y1 - y0 + 1) as usize];
    for &(x, y) in cells {
        rows[(y - y0) as usize].push(x - x0);
    }
    // Tokens: a count (left out when 1) and a letter.
    let mut tokens: Vec<String> = Vec::new();
    let token = |n: i64, c: char| {
        if n == 1 {
            c.to_string()
        } else {
            format!("{n}{c}")
        }
    };
    let mut blank_rows = 0;
    for (k, row) in rows.iter_mut().enumerate() {
        row.sort_unstable();
        row.dedup();
        if k > 0 {
            if row.is_empty() {
                blank_rows += 1;
                continue;
            }
            tokens.push(token(blank_rows + 1, '$'));
            blank_rows = 0;
        }
        let mut x = 0;
        let mut i = 0;
        while i < row.len() {
            if row[i] > x {
                tokens.push(token(row[i] - x, 'b'));
            }
            let mut run = 1;
            while i + run < row.len() && row[i + run] == row[i] + run as i64 {
                run += 1;
            }
            tokens.push(token(run as i64, 'o'));
            x = row[i] + run as i64;
            i += run;
        }
    }
    tokens.push("!".into());
    let mut line = String::new();
    for t in tokens {
        if !line.is_empty() && line.len() + t.len() > LINE {
            out.push_str(&line);
            out.push('\n');
            line.clear();
        }
        line.push_str(&t);
    }
    out.push_str(&line);
    out.push('\n');
    out
}

/// The world's pattern as RLE, for *Edit → Copy as RLE*: its live cells, and its rule in the form
/// Golly and LifeViewer read. Refused, with the reason, for anything they wouldn't read the same.
pub fn of_world(w: &World, comments: &[String]) -> Result<String, String> {
    let family = w.topo.spec.family.as_str();
    let hex = match family {
        "square" => false,
        "hex" => true,
        other => {
            return Err(format!(
                "Copy as RLE writes patterns on the square grid and the hexagon grid for now; this \
                 is {other}. Other tilings come with the exchange with Golly and LifeViewer (#152)"
            ));
        }
    };
    let grid = Grid::of(&w.topo).ok_or("this tiling has no x/y grid")?;
    if grid.types != 1 {
        return Err("Copy as RLE needs a grid of one tile type".into());
    }
    let [ca] = w.cas.as_slice() else {
        return Err(format!(
            "Copy as RLE writes one automaton's pattern; this world has {}",
            w.cas.len()
        ));
    };
    let two_state = match &ca.rule {
        Rule::LifeLike { .. } => true,
        Rule::Isotropic(r) => r.states <= 2,
        _ => false,
    };
    if !two_state {
        return Err(format!(
            "Copy as RLE writes two-state B/S and isotropic rules for now; {} is another kind",
            ca.rule.notation()
        ));
    }
    if ca.rule.is_b0() {
        return Err(
            "Copy as RLE leaves out rules with B0 for now: JETCARP keeps their cells relative to \
             a background that Golly and LifeViewer don't share"
                .into(),
        );
    }
    let mut rule = notation_on(&ca.rule, ca.nb);
    if hex {
        // On hexagons the edge and touch neighbours are the same six.
        rule = rule.trim_end_matches('V').to_string();
        rule.push('H');
    } else if ca.nb == Nb::Edge && !rule.ends_with('V') {
        rule.push('V');
    }
    let field = &w.fields[ca.field];
    let mut cells: Vec<(i64, i64)> = Vec::new();
    for (i, &s) in field.state.iter().enumerate() {
        if s != 0 {
            let (x, y) = grid.xy(i);
            let (x, y) = (x as i64, y as i64);
            cells.push((if hex { golly_hex_x(x, y) } else { x }, y));
        }
    }
    if cells.is_empty() {
        return Err("nothing to copy: the field has no live cells".into());
    }
    Ok(write(&cells, &rule, comments))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cells::parse_rle;
    use jetcarp_core::{Boundary, CaSpec, SeedSpec, TilingSpec};

    fn world(family: &str, rule: &str, cells: &[(usize, usize)]) -> World {
        let mut w = World::new(TilingSpec::rect(family, 20, 20, Boundary::Dead), 1).unwrap();
        w.add_ca(CaSpec::new(rule).seed(SeedSpec::Empty)).unwrap();
        let g = Grid::of(&w.topo).unwrap();
        for &(x, y) in cells {
            let i = g.index(x as i64, y as i64).unwrap();
            w.fields[0].state[i] = 1;
        }
        w
    }

    #[test]
    fn a_glider_is_written_as_golly_writes_it() {
        let w = world(
            "square",
            "B3/S23",
            &[(6, 5), (7, 6), (5, 7), (6, 7), (7, 7)],
        );
        assert_eq!(
            of_world(&w, &[]).unwrap(),
            "x = 3, y = 3, rule = B3/S23\nbo$2bo$3o!\n"
        );
    }

    #[test]
    fn blank_rows_long_runs_and_wrapping() {
        let mut cells: Vec<(i64, i64)> = (0..12).map(|x| (x, 0)).collect();
        cells.push((3, 3));
        let t = write(&cells, "B3/S23", &["a comment".into()]);
        assert_eq!(t, "#C a comment\nx = 12, y = 4, rule = B3/S23\n12o3$3bo!\n");
        // A long body is wrapped at 70 characters, between tokens.
        let many: Vec<(i64, i64)> = (0..200).map(|k| (2 * k, 0)).collect();
        let t = write(&many, "B3/S23", &[]);
        assert!(t.lines().all(|l| l.len() <= 70), "{t}");
        let back: Vec<(i64, i64)> = parse_rle(&t).unwrap().iter().map(|c| (c.0, c.1)).collect();
        assert_eq!(back, many);
    }

    #[test]
    fn isotropic_rules_are_named_as_golly_names_them() {
        // Written any way, named canonically: letters in alphabetical order, or `-` where shorter.
        let w = world("square", "B2kicen/S21", &[(3, 3), (4, 3)]);
        let t = of_world(&w, &[]).unwrap();
        assert_eq!(header_rule(&t).as_deref(), Some("B2-a/S12"), "{t}");
        // Generations and B0 are left out, as for B/S rules.
        for rule in ["B2a/S/C3", "B01e/S8"] {
            let w = world("square", rule, &[(3, 3), (4, 3)]);
            assert!(of_world(&w, &[]).is_err(), "{rule}");
        }
    }

    #[test]
    fn von_neumann_keeps_its_v() {
        let w = world("square", "B2/S013V", &[(3, 3), (4, 3)]);
        let t = of_world(&w, &[]).unwrap();
        assert_eq!(header_rule(&t).as_deref(), Some("B2/S013V"), "{t}");
    }

    /// Every neighbour of a JETCARP hexagon cell, on an even row and on an odd one, lands on one
    /// of Golly's six: the Moore neighbourhood less (+1, −1) and (−1, +1).
    #[test]
    fn hex_neighbours_match_golly() {
        let w = world("hex", "B2/S34", &[]);
        let g = Grid::of(&w.topo).unwrap();
        let golly = [(-1, -1), (0, -1), (-1, 0), (1, 0), (0, 1), (1, 1)];
        for i in [
            g.index(6, 6).unwrap(),
            g.index(7, 7).unwrap(),
            g.index(6, 7).unwrap(),
        ] {
            let (x, y) = g.xy(i);
            let (gx, gy) = (golly_hex_x(x as i64, y as i64), y as i64);
            let mut seen = vec![];
            w.topo.for_each_neighbor(i, Nb::Edge, |j| {
                let (nx, ny) = g.xy(j);
                let d = (golly_hex_x(nx as i64, ny as i64) - gx, ny as i64 - gy);
                assert!(golly.contains(&d), "cell {i}: neighbour at {d:?}");
                seen.push(d);
            });
            seen.sort_unstable();
            seen.dedup();
            assert_eq!(seen.len(), 6, "cell {i}");
            assert_eq!(grid_hex_x(gx, gy), x as i64);
        }
    }

    /// A hexagon pattern written in Golly's layout and read back onto a row of either parity is
    /// the same shape: its cells, taken back to Golly's columns, are the original's, moved as a
    /// whole.
    #[test]
    fn hex_patterns_come_back_on_any_row() {
        let shape = [(5, 5), (6, 5), (5, 6), (7, 6), (6, 7), (8, 8)];
        let w = world("hex", "B2/S34", &shape);
        let t = of_world(&w, &[]).unwrap();
        assert_eq!(header_rule(&t).as_deref(), Some("B2/S34H"), "{t}");
        assert!(is_golly_hex(&t));
        let golly = |cells: &mut Vec<(i64, i64)>| {
            let mut g: Vec<(i64, i64)> =
                cells.iter().map(|&(x, y)| (golly_hex_x(x, y), y)).collect();
            let (x0, y0) = (
                g.iter().map(|c| c.0).min().unwrap(),
                g.iter().map(|c| c.1).min().unwrap(),
            );
            g = g.iter().map(|&(x, y)| (x - x0, y - y0)).collect();
            g.sort_unstable();
            g
        };
        let mut original: Vec<(i64, i64)> =
            shape.iter().map(|&(x, y)| (x as i64, y as i64)).collect();
        let want = golly(&mut original);
        let read = parse_rle(&t).unwrap();
        for (ox, row) in [(3, 4), (3, 5), (10, 10), (2, 11)] {
            let mut placed: Vec<(i64, i64)> = from_golly_hex(&read, row)
                .iter()
                .map(|c| (ox + c.0, row + c.1))
                .collect();
            assert_eq!(golly(&mut placed), want, "placed at ({ox}, {row})");
        }
    }

    #[test]
    fn what_golly_wouldn_t_read_the_same_is_refused() {
        let e = of_world(&world("triangle", "B3/S23", &[(2, 2)]), &[]).unwrap_err();
        assert!(e.contains("square grid and the hexagon grid"), "{e}");
        let e = of_world(&world("square", "B2/S/C3", &[(2, 2)]), &[]).unwrap_err();
        assert!(e.contains("two-state B/S"), "{e}");
        let e = of_world(&world("square", "B03/S23", &[(2, 2)]), &[]).unwrap_err();
        assert!(e.contains("B0"), "{e}");
        let e = of_world(&world("square", "B3/S23", &[]), &[]).unwrap_err();
        assert!(e.contains("no live cells"), "{e}");
    }

    #[test]
    fn header_rules_are_read() {
        assert_eq!(
            header_rule("#C x\nx = 3, y = 3, rule = B3/S23\nbo!").as_deref(),
            Some("B3/S23")
        );
        assert_eq!(
            header_rule("x=1,y=1,rule=B2/S34H\no!").as_deref(),
            Some("B2/S34H")
        );
        assert_eq!(header_rule("bo$2bo$3o!"), None);
    }
}
