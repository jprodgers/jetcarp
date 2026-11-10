//! Turmites: Langton's ant and its generalisations, as one transition table.
//!
//! A turmite has `states` internal states and walks on cells with `colors` colours. On a cell of
//! colour `c` in state `q` it looks up `table[q * colors + c]`: it writes colour `write`, turns by
//! `turn` (counted in neighbour slots of the cell's cyclic edge-neighbour order), changes to state
//! `next`, and steps to the neighbour it now faces. A **turn string** such as `RL` (Langton's
//! ant) or `LLRR` is the one-state case: colour `c` turns by the string's letter `c` and becomes
//! `c + 1` (mod the string's length).
//!
//! **Turns** ([`Turn`]): `R` and `L` are one slot clockwise or anticlockwise, `N` none, `U` half
//! the cell's degree (a U-turn on even degrees), and `R2` / `L2` two slots (120 degrees on
//! hexagons, the same as `U` on squares). After a step the ant faces "straight on": the slot
//! opposite the edge it came in by (`back + degree / 2`), so on odd degrees `N` and `U` are
//! relative to the entry edge.
//!
//! **Notation.** Turn strings: `Ant RL`, `Turmite LLRR`, `Turmite RL2NU` (hexagons). Tables
//! (several states): `Turmite 1R1,1L0/1R1,0N0`, one group per state separated by `/`, one entry
//! per colour separated by `,`, each entry `<write><turn><next>`; this one is 2 states and 2
//! colours. Up to 16 colours and 16 states; colours and states 10 to 15 are written `a` to `f`.

use crate::rules::Turn;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Entry {
    pub write: u8,
    pub turn: Turn,
    pub next: u8,
}

/// A turmite's transition table (see the module docs).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Mite {
    pub states: u8,
    pub colors: u8,
    /// `table[q * colors + c]`.
    pub table: Vec<Entry>,
}

pub const MAX_COLORS: usize = 16;
pub const MAX_STATES: usize = 16;

fn digit(v: u8) -> char {
    std::char::from_digit(v as u32, 16).unwrap_or('?')
}

impl Turn {
    /// Slots turned clockwise on a cell with `deg` edge neighbours.
    #[inline]
    pub fn slots(self, deg: usize) -> usize {
        match self {
            Turn::N => 0,
            Turn::R => 1 % deg,
            Turn::L => deg - 1 % deg,
            Turn::U => deg / 2,
            Turn::R2 => 2 % deg,
            Turn::L2 => (deg - 2 % deg) % deg,
        }
    }
    /// The turn seen in a mirror.
    pub fn mirror(self) -> Turn {
        match self {
            Turn::L => Turn::R,
            Turn::R => Turn::L,
            Turn::L2 => Turn::R2,
            Turn::R2 => Turn::L2,
            t => t,
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            Turn::L => "L",
            Turn::R => "R",
            Turn::N => "N",
            Turn::U => "U",
            Turn::L2 => "L2",
            Turn::R2 => "R2",
        }
    }
    /// Every turn, in the order searches enumerate them.
    pub const ALL: [Turn; 6] = [Turn::L, Turn::R, Turn::N, Turn::U, Turn::L2, Turn::R2];
}

/// Parse turn letters: `L`, `R`, `N`, `U`, with `L1`/`R1` for `L`/`R` and `L2`/`R2` for two
/// slots. Case-insensitive.
pub fn parse_turns(s: &str) -> Result<Vec<Turn>, String> {
    let c: Vec<char> = s
        .chars()
        .filter(|c| !c.is_whitespace())
        .map(|c| c.to_ascii_lowercase())
        .collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < c.len() {
        let t = match c[i] {
            'l' => Turn::L,
            'r' => Turn::R,
            'n' => Turn::N,
            'u' => Turn::U,
            x => return Err(format!("turn '{x}' is not L, R, N, U, L2, or R2")),
        };
        i += 1;
        let t = match (t, c.get(i)) {
            (Turn::L, Some('2')) => {
                i += 1;
                Turn::L2
            }
            (Turn::R, Some('2')) => {
                i += 1;
                Turn::R2
            }
            (Turn::L | Turn::R, Some('1')) => {
                i += 1;
                t
            }
            _ => t,
        };
        out.push(t);
    }
    Ok(out)
}

pub fn turns_text(t: &[Turn]) -> String {
    t.iter().map(|x| x.name()).collect()
}

impl Mite {
    /// The one-state turmite of a turn string.
    pub fn from_turns(t: &[Turn]) -> Mite {
        let k = t.len() as u8;
        Mite {
            states: 1,
            colors: k,
            table: t
                .iter()
                .enumerate()
                .map(|(c, &turn)| Entry {
                    write: ((c + 1) % t.len()) as u8,
                    turn,
                    next: 0,
                })
                .collect(),
        }
    }
    /// Langton's ant.
    pub fn langton() -> Mite {
        Mite::from_turns(&[Turn::R, Turn::L])
    }
    /// The turn string, if this is one (one state, colour `c` becomes `c + 1`).
    pub fn turns(&self) -> Option<Vec<Turn>> {
        let k = self.colors as usize;
        (self.states == 1
            && self
                .table
                .iter()
                .enumerate()
                .all(|(c, e)| e.next == 0 && e.write as usize == (c + 1) % k))
        .then(|| self.table.iter().map(|e| e.turn).collect())
    }
    #[inline]
    pub fn entry(&self, q: u8, c: u8) -> Entry {
        self.table[q as usize * self.colors as usize + c as usize]
    }
    /// `Ant RL`, `Turmite LLRR`, or `Turmite 1R1,1L0/1R1,0N0`.
    pub fn notation(&self) -> String {
        if let Some(t) = self.turns() {
            let s = turns_text(&t);
            return if s == "RL" {
                "Ant RL".into()
            } else {
                format!("Turmite {s}")
            };
        }
        format!("Turmite {}", self.table_text())
    }
    /// The table part of the notation: `1R1,1L0/1R1,0N0`.
    pub fn table_text(&self) -> String {
        let k = self.colors as usize;
        self.table
            .chunks(k)
            .map(|row| {
                row.iter()
                    .map(|e| format!("{}{}{}", digit(e.write), e.turn.name(), digit(e.next)))
                    .collect::<Vec<_>>()
                    .join(",")
            })
            .collect::<Vec<_>>()
            .join("/")
    }
    /// Parse what follows `Ant` or `Turmite` (lower case, no spaces): a turn string or a table.
    pub fn parse_body(rest: &str) -> Result<Mite, String> {
        let rest = rest.trim();
        if rest.is_empty() {
            return Ok(Mite::langton());
        }
        if !rest.contains([',', '/']) {
            let turns = parse_turns(rest)?;
            if turns.len() < 2 || turns.len() > MAX_COLORS {
                return Err(format!("turn strings have 2 to {MAX_COLORS} turns"));
            }
            return Ok(Mite::from_turns(&turns));
        }
        let rows: Vec<&str> = rest.split('/').collect();
        let n = rows.len();
        let mut table = Vec::new();
        let mut k = 0;
        for (q, row) in rows.iter().enumerate() {
            let entries: Vec<&str> = row.split(',').collect();
            if q == 0 {
                k = entries.len();
            } else if entries.len() != k {
                return Err(format!(
                    "turmite table: state {q} has {} entries, state 0 has {k} (one per colour)",
                    entries.len()
                ));
            }
            for e in entries {
                let c: Vec<char> = e.trim().chars().collect();
                if c.len() < 3 {
                    return Err(format!(
                        "turmite entry '{e}' is not <write><turn><next>, like 1R0"
                    ));
                }
                let w = c[0].to_digit(16).ok_or(format!("bad colour in '{e}'"))? as u8;
                let nx = c[c.len() - 1]
                    .to_digit(16)
                    .ok_or(format!("bad state in '{e}'"))? as u8;
                let mid: String = c[1..c.len() - 1].iter().collect();
                let t = parse_turns(&mid)?;
                if t.len() != 1 {
                    return Err(format!("turmite entry '{e}' needs exactly one turn"));
                }
                table.push(Entry {
                    write: w,
                    turn: t[0],
                    next: nx,
                });
            }
        }
        if !(1..=MAX_STATES).contains(&n) || !(2..=MAX_COLORS).contains(&k) {
            return Err(format!(
                "turmite tables have 1 to {MAX_STATES} states and 2 to {MAX_COLORS} colours"
            ));
        }
        if let Some(e) = table
            .iter()
            .find(|e| e.write as usize >= k || e.next as usize >= n)
        {
            return Err(format!(
                "turmite entry {}{}{} writes a colour or names a state that does not exist ({k} colours, {n} states)",
                digit(e.write),
                e.turn.name(),
                digit(e.next)
            ));
        }
        Ok(Mite {
            states: n as u8,
            colors: k as u8,
            table,
        })
    }
    /// The mirror image (every turn mirrored).
    pub fn mirror(&self) -> Mite {
        Mite {
            table: self
                .table
                .iter()
                .map(|e| Entry {
                    turn: e.turn.mirror(),
                    ..*e
                })
                .collect(),
            ..self.clone()
        }
    }
}
