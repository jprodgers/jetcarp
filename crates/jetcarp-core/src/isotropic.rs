//! Isotropic non-totalistic rules on the square grid's Moore neighbourhood, in Alan Hensel's
//! notation (#70): `B2-a/S12`, `B3-cnqy/S23-a`, and Generations rules such as `B2a/S/C3`.
//!
//! A cell's eight touching neighbours fall into 51 arrangements once rotations and reflections
//! are set aside: one each for 0 and 8 live neighbours, and 2, 6, 10, 13, 10, 6 and 2 for 1 to 7.
//! Hensel gave each a letter after its count. A rule lists, after each count, the letters it
//! takes (`2ce`), or after a `-` the letters it leaves out (`2-a`: every arrangement of two but
//! `a`); a count with no letters takes every arrangement, as in a B/S rule.
//!
//! **The letters.** Each is pinned here by one arrangement, read from the diagrams of Hensel's
//! revised table ("Rule Notation", `ibiblio.org/lifepatterns/neighbors2.html`, dated 15 May 2001
//! on its server): see [`LETTERS`] and [`REPRESENTATIVES`]. The arrangements for 5, 6 and 7 live
//! neighbours are those for 3, 2 and 1 dead ones, under the same letters, as Hensel's earlier page
//! (`neighbors.html`) says and his diagrams show. The program Life32 swapped `r` and `y` for 3, 4
//! and 5 neighbours (Eric Goldstein and Paul Tooke, ConwayLife forum, July 2012); JETCARP follows
//! Hensel's table, as Golly does, and `tests/golly.rs` in `jetcarp-api` checks every letter
//! against Golly.
//!
//! **The canonical name** ([`IsoRule::notation`]) is the form Golly's documentation gives: each
//! count's letters in alphabetical order, or the `-` form when that is shorter (the plain form when
//! they are the same length); a count with every letter written as the bare digit. A rule whose
//! counts all have every letter or none is a B/S rule, and is parsed as one ([`parse`]).
//!
//! **Neighbourhood indices.** A neighbourhood is a byte: bit `k` is the neighbour at
//! [`OFFSETS`]`[k]`, in reading order from the top left (north-west), the cell itself left out.

use std::sync::OnceLock;

/// The eight touching neighbours as `(dx, dy)`, `y` down, in bit order: NW, N, NE, W, E, SW, S,
/// SE.
pub const OFFSETS: [(i32, i32); 8] = [
    (-1, -1),
    (0, -1),
    (1, -1),
    (-1, 0),
    (1, 0),
    (-1, 1),
    (0, 1),
    (1, 1),
];

const NW: u8 = 1;
const N: u8 = 1 << 1;
const NE: u8 = 1 << 2;
const W: u8 = 1 << 3;
const E: u8 = 1 << 4;
const SW: u8 = 1 << 5;
const S: u8 = 1 << 6;
const SE: u8 = 1 << 7;

/// The letters each count of live neighbours takes, in the order of Hensel's table (corner, edge,
/// knight, adjacent, then i, n, y, q, j, r, t, w, z). 0 and 8 take none.
pub const LETTERS: [&str; 9] = [
    "",
    "ce",
    "cekain",
    "cekainyqjr",
    "cekainyqjrtwz",
    "cekainyqjr",
    "cekain",
    "ce",
    "",
];

/// One arrangement of live neighbours per letter of 1 to 4, from the diagrams of Hensel's table
/// (`neighbors2.html`): count, letter, and the live neighbours. Every rotation and reflection of
/// one has the same letter.
pub const REPRESENTATIVES: [(u8, char, u8); 31] = [
    (1, 'c', NW),
    (1, 'e', N),
    (2, 'c', NW | NE),
    (2, 'e', N | W),
    (2, 'k', NW | E),
    (2, 'a', NW | N),
    (2, 'i', N | S),
    (2, 'n', NW | SE),
    (3, 'c', NW | NE | SW),
    (3, 'e', N | W | E),
    (3, 'k', NW | E | S),
    (3, 'a', NW | N | W),
    (3, 'i', NW | N | NE),
    (3, 'n', NW | NE | E),
    (3, 'y', NW | NE | S),
    (3, 'q', NW | N | SE),
    (3, 'j', NW | N | E),
    (3, 'r', NW | N | S),
    (4, 'c', NW | NE | SW | SE),
    (4, 'e', N | W | E | S),
    (4, 'k', NW | NE | W | S),
    (4, 'a', NW | N | NE | E),
    (4, 'i', NW | NE | W | E),
    (4, 'n', NW | N | NE | SW),
    (4, 'y', NW | NE | E | SW),
    (4, 'q', NW | N | W | SE),
    (4, 'j', NW | N | E | S),
    (4, 'r', NW | N | W | E),
    (4, 't', NW | N | NE | S),
    (4, 'w', NW | N | E | SE),
    (4, 'z', NW | N | S | SE),
];

/// A neighbourhood under one of the square's eight symmetries (`s` 0-3 turn it by a quarter `s`
/// times; 4-7 reflect it first).
pub fn transform(nbhd: u8, s: u8) -> u8 {
    let mut out = 0u8;
    for (k, &(x, y)) in OFFSETS.iter().enumerate() {
        if nbhd >> k & 1 == 0 {
            continue;
        }
        let (mut x, mut y) = if s >= 4 { (-x, y) } else { (x, y) };
        for _ in 0..s % 4 {
            (x, y) = (-y, x);
        }
        let to = OFFSETS.iter().position(|&o| o == (x, y)).unwrap();
        out |= 1 << to;
    }
    out
}

/// The class of every neighbourhood: its live-neighbour count in the high four bits, and the index
/// of its letter in [`LETTERS`] for that count in the low four (0 for counts 0 and 8).
pub fn classes() -> &'static [u8; 256] {
    static T: OnceLock<[u8; 256]> = OnceLock::new();
    T.get_or_init(|| {
        let mut t = [0xffu8; 256];
        t[0] = 0;
        t[255] = 8 << 4;
        for &(n, letter, cells) in &REPRESENTATIVES {
            let li = LETTERS[n as usize].find(letter).unwrap() as u8;
            for s in 0..8 {
                let x = transform(cells, s);
                t[x as usize] = n << 4 | li;
                // The same letter for the dead neighbours of 5 to 7 (4's complements are 4s,
                // with letters of their own).
                if n < 4 {
                    t[!x as usize] = (8 - n) << 4 | li;
                }
            }
        }
        debug_assert!(t.iter().all(|&c| c != 0xff));
        t
    })
}

/// A count's mask with every letter (bit 0 alone for 0 and 8).
pub fn full(n: usize) -> u16 {
    (1u16 << LETTERS[n].len().max(1)) - 1
}

/// The 51 classes of arrangement numbered count by count, letters in [`LETTERS`] order: where
/// each count's start (0 for 0, 1 for `1c`, 3 for `2c`, ..., 50 for 8).
pub const CLASS_START: [u32; 9] = [0, 1, 3, 9, 19, 32, 42, 48, 50];

/// How many classes there are.
pub const CLASSES: u32 = 51;

/// Every class's bit, in a 51-bit mask over [`class_number`]s.
pub const ALL_CLASSES: u64 = (1u64 << CLASSES) - 1;

/// The number (0 to 50) of a neighbourhood's class.
#[inline]
pub fn class_number(nbhd: u8) -> u32 {
    let c = classes()[nbhd as usize];
    CLASS_START[(c >> 4) as usize] + (c & 15) as u32
}

/// A class's count and letter (`2a`), from its number.
pub fn class_name(k: u32) -> String {
    let n = CLASS_START.iter().rposition(|&s| s <= k).unwrap_or(0);
    match LETTERS[n].as_bytes().get((k - CLASS_START[n]) as usize) {
        Some(&l) => format!("{n}{}", l as char),
        None => n.to_string(),
    }
}

/// An isotropic non-totalistic rule: per count of live neighbours 0 to 8, the letters (bit `l` for
/// the `l`-th of [`LETTERS`]`[n]`; bit 0 for 0 and 8) on which a dead cell is born and a live one
/// survives; and the state count (2, or a Generations rule's, whose dying cells count up to it).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct IsoRule {
    pub birth: [u16; 9],
    pub survive: [u16; 9],
    pub states: u8,
}

/// A B/S rule (`birth`, `survive` count masks) or an isotropic one.
#[derive(Clone, Debug, PartialEq)]
pub enum Parsed {
    Totalistic { birth: u32, survive: u32 },
    Isotropic(IsoRule),
}

/// Whether a B or S part of a B/S rule uses Hensel's letters or `-` (lower case, without its `b`
/// or `s`).
pub fn has_letters(part: &str) -> bool {
    part.chars().any(|c| c == '-' || c.is_ascii_lowercase())
}

/// One count list of Hensel's notation (`2-a3`, `2cekin`, lower case, without its `b` or `s`) as
/// letter masks per count.
pub fn parse_counts(part: &str) -> Result<[u16; 9], String> {
    let mut m = [0u16; 9];
    let chars: Vec<char> = part.chars().collect();
    let mut k = 0;
    while k < chars.len() {
        let c = chars[k];
        let n = match c.to_digit(10) {
            Some(n) if n <= 8 => n as usize,
            Some(n) => {
                return Err(format!(
                    "{n} is not a count of the square grid's eight touching neighbours (0 to 8)"
                ));
            }
            None => {
                return Err(format!(
                    "'{c}' has no count before it: Hensel's letters follow a count, as in B2-a/S12"
                ));
            }
        };
        k += 1;
        let minus = chars.get(k) == Some(&'-');
        if minus {
            k += 1;
        }
        let mut letters = 0u16;
        while let Some(&l) = chars.get(k).filter(|l| l.is_ascii_lowercase()) {
            let at = LETTERS[n].find(l).ok_or_else(|| letter_error(n, l))?;
            letters |= 1 << at;
            k += 1;
        }
        if chars.get(k) == Some(&'-') {
            return Err(format!(
                "a '-' goes right after a count, once: {}",
                chars.iter().collect::<String>()
            ));
        }
        m[n] |= if minus {
            full(n) & !letters
        } else if letters == 0 {
            full(n)
        } else {
            letters
        };
    }
    Ok(m)
}

fn letter_error(n: usize, l: char) -> String {
    match LETTERS[n] {
        "" => format!(
            "'{l}' cannot follow {n}: with {n} live neighbours there is only one arrangement, so \
             {n} takes no letters"
        ),
        ls => {
            let list: Vec<String> = ls.chars().map(String::from).collect();
            let s = if n == 1 { "" } else { "s" };
            format!(
                "'{l}' is not one of Hensel's letters for {n} live neighbour{s}; {n} takes {}",
                list.join(", ")
            )
        }
    }
}

/// The B, S and state-count parts of a rule written in Hensel's notation, lower case and without
/// their `b` and `s` (`states` 2 for a B/S rule). A rule whose counts all have every letter or
/// none is the B/S or Generations rule of those counts.
pub fn parse(b: &str, s: &str, states: u8) -> Result<Parsed, String> {
    let rule = IsoRule {
        birth: parse_counts(b)?,
        survive: parse_counts(s)?,
        states,
    };
    Ok(match rule.totalistic() {
        Some((birth, survive)) => Parsed::Totalistic { birth, survive },
        None => Parsed::Isotropic(rule),
    })
}

/// One count list in canonical form: for each count with any letter, the digit and then its
/// letters in alphabetical order, or `-` and the letters it leaves out where that is shorter.
fn counts_notation(m: &[u16; 9]) -> String {
    let mut out = String::new();
    for (n, &bits) in m.iter().enumerate() {
        if bits == 0 {
            continue;
        }
        out.push(char::from(b'0' + n as u8));
        if bits == full(n) {
            continue;
        }
        let pick = |want: bool| -> String {
            let mut v: Vec<char> = LETTERS[n]
                .chars()
                .enumerate()
                .filter(|(i, _)| (bits >> i & 1 == 1) == want)
                .map(|(_, c)| c)
                .collect();
            v.sort_unstable();
            v.into_iter().collect()
        };
        let (plus, minus) = (pick(true), pick(false));
        if minus.len() + 1 < plus.len() {
            out.push('-');
            out.push_str(&minus);
        } else {
            out.push_str(&plus);
        }
    }
    out
}

impl IsoRule {
    /// The rule with these B/S count masks (counts above 8 dropped): every letter of each count.
    pub fn from_masks(birth: u32, survive: u32, states: u8) -> IsoRule {
        let per = |m: u32| -> [u16; 9] {
            std::array::from_fn(|n| if m >> n & 1 == 1 { full(n) } else { 0 })
        };
        IsoRule {
            birth: per(birth),
            survive: per(survive),
            states,
        }
    }

    /// Birth and survival as 51-bit masks over the [`class_number`]s.
    pub fn class_masks(&self) -> (u64, u64) {
        let mask = |m: &[u16; 9]| -> u64 {
            (0..9).fold(0u64, |acc, n| acc | (m[n] as u64) << CLASS_START[n])
        };
        (mask(&self.birth), mask(&self.survive))
    }

    /// The rule of these 51-bit class masks ([`class_masks`](Self::class_masks)).
    pub fn from_class_masks(birth: u64, survive: u64, states: u8) -> IsoRule {
        let per = |m: u64| -> [u16; 9] {
            std::array::from_fn(|n| (m >> CLASS_START[n]) as u16 & full(n))
        };
        IsoRule {
            birth: per(birth),
            survive: per(survive),
            states,
        }
    }

    /// The B/S count masks, when every count has every letter or none.
    pub fn totalistic(&self) -> Option<(u32, u32)> {
        let masks = |m: &[u16; 9]| -> Option<u32> {
            let mut out = 0u32;
            for (n, &bits) in m.iter().enumerate() {
                if bits == full(n) {
                    out |= 1 << n;
                } else if bits != 0 {
                    return None;
                }
            }
            Some(out)
        };
        Some((masks(&self.birth)?, masks(&self.survive)?))
    }

    /// Whether a dead cell is born on 0 live neighbours.
    pub fn b0(&self) -> bool {
        self.birth[0] & 1 == 1
    }

    /// Whether a dead cell with these live neighbours is born.
    #[inline]
    pub fn born(&self, nbhd: u8) -> bool {
        let c = classes()[nbhd as usize];
        self.birth[(c >> 4) as usize] >> (c & 15) & 1 == 1
    }

    /// Whether a live cell with these live neighbours survives.
    #[inline]
    pub fn survives(&self, nbhd: u8) -> bool {
        let c = classes()[nbhd as usize];
        self.survive[(c >> 4) as usize] >> (c & 15) & 1 == 1
    }

    /// The rule as a table of 512 bits, 16 words: bit `x` of words 0-7 (neighbourhood
    /// `32 w + x`) is birth, of words 8-15 survival. What the GPU and the steppers read.
    pub fn table(&self) -> [u32; 16] {
        let mut t = [0u32; 16];
        for x in 0..256usize {
            t[x / 32] |= (self.born(x as u8) as u32) << (x % 32);
            t[8 + x / 32] |= (self.survives(x as u8) as u32) << (x % 32);
        }
        t
    }

    /// The canonical notation: `B2-a/S12`, and `/C3` after a Generations rule.
    pub fn notation(&self) -> String {
        let bs = format!(
            "B{}/S{}",
            counts_notation(&self.birth),
            counts_notation(&self.survive)
        );
        if self.states > 2 {
            format!("{bs}/C{}", self.states)
        } else {
            bs
        }
    }

    /// A cell's next state, from its own (0 dead, 1 live, 2 and up dying, as in Generations) and
    /// its live neighbours.
    pub fn next(&self, s: u8, nbhd: u8) -> u8 {
        match s {
            0 => self.born(nbhd) as u8,
            1 if self.survives(nbhd) => 1,
            1 if self.states > 2 => 2,
            1 => 0,
            s if s + 1 < self.states => s + 1,
            _ => 0,
        }
    }

    /// The rule's background on the square grid, as a B/S rule's count masks: birth on 0 and
    /// survival on 8, the two counts with one arrangement each, which are all a uniform
    /// background sees. For a rule with birth on 0 (see [`crate::b0`]).
    pub fn background_masks(&self) -> (u32, u32) {
        (
            (self.birth[0] & 1) as u32,
            ((self.survive[8] & 1) as u32) << 8,
        )
    }
}

/// Half of a rule (birth or survival) as a reduced, ordered decision diagram over the eight
/// neighbour bits, for bit-parallel stepping: 64 cells at once, a few operations a node instead of
/// a table lookup per cell. Node `k` (from 2; 0 is false and 1 true) is `nodes[k - 2]`:
/// `(bit, lo, hi)`, which is `hi` where neighbour `bit` is live and `lo` where it is not. Its
/// children come before it, so evaluating the nodes in order evaluates the root last.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Diagram {
    pub nodes: Vec<(u8, u32, u32)>,
    pub root: u32,
}

impl Diagram {
    /// The diagram of a 256-entry truth table (entry `x` for neighbourhood `x`).
    pub fn new(table: &[bool; 256]) -> Diagram {
        fn build(
            t: &[bool],
            nodes: &mut Vec<(u8, u32, u32)>,
            seen: &mut std::collections::HashMap<(u8, u32, u32), u32>,
        ) -> u32 {
            if t.iter().all(|&b| b == t[0]) {
                return t[0] as u32;
            }
            let half = t.len() / 2;
            let bit = half.trailing_zeros() as u8;
            let lo = build(&t[..half], nodes, seen);
            let hi = build(&t[half..], nodes, seen);
            if lo == hi {
                return lo;
            }
            *seen.entry((bit, lo, hi)).or_insert_with(|| {
                nodes.push((bit, lo, hi));
                nodes.len() as u32 + 1
            })
        }
        let mut nodes = Vec::new();
        let root = build(table, &mut nodes, &mut Default::default());
        Diagram { nodes, root }
    }

    /// The rule's birth (`live` false) or survival diagram.
    pub fn of(rule: &IsoRule, live: bool) -> Diagram {
        let t: [bool; 256] = std::array::from_fn(|x| {
            if live {
                rule.survives(x as u8)
            } else {
                rule.born(x as u8)
            }
        });
        Diagram::new(&t)
    }

    /// The diagram for 64 cells at once: `x[k]` has a cell's bit set where its neighbour `k`
    /// ([`OFFSETS`]) is live.
    #[inline]
    pub fn eval(&self, x: &[u64; 8], scratch: &mut Vec<u64>) -> u64 {
        if self.root < 2 {
            return if self.root == 1 { !0 } else { 0 };
        }
        scratch.clear();
        scratch.extend_from_slice(&[0, !0]);
        for &(bit, lo, hi) in &self.nodes {
            let m = x[bit as usize];
            let v = (scratch[lo as usize] & !m) | (scratch[hi as usize] & m);
            scratch.push(v);
        }
        scratch[self.root as usize]
    }
}

/// Where each of [`OFFSETS`] is among a cell's neighbour offsets `(di, dj)`, when they are the
/// square grid's eight touching neighbours, one each.
pub fn slots(offsets: &[(i32, i32)]) -> Option<[usize; 8]> {
    if offsets.len() != 8 {
        return None;
    }
    let mut out = [0usize; 8];
    for (k, o) in OFFSETS.iter().enumerate() {
        out[k] = offsets.iter().position(|x| x == o)?;
    }
    Some(out)
}

/// The neighbourhood of a cell from its eight neighbours' states in [`OFFSETS`] order, reading
/// state 1 as live (Generations' dying cells are not).
#[inline]
pub fn neighbourhood(states: [u8; 8]) -> u8 {
    let mut x = 0u8;
    for (k, &s) in states.iter().enumerate() {
        x |= ((s == 1) as u8) << k;
    }
    x
}

#[cfg(test)]
mod tests {
    use super::*;

    fn canon(n: u8) -> u8 {
        (0..8).map(|s| transform(n, s)).min().unwrap()
    }

    // The letters give every class of arrangements exactly once: 1, 2, 6, 10, 13, 10, 6, 2, 1 per
    // count, 51 in all, each closed under the square's symmetries.
    #[test]
    fn the_letters_cover_every_class_once() {
        let t = classes();
        let mut per = [
            std::collections::BTreeSet::new(),
            Default::default(),
            Default::default(),
            Default::default(),
            Default::default(),
            Default::default(),
            Default::default(),
            Default::default(),
            Default::default(),
        ];
        for x in 0..=255u8 {
            let c = t[x as usize];
            assert_eq!((c >> 4) as u32, x.count_ones(), "{x:08b}");
            per[(c >> 4) as usize].insert(c & 15);
            for s in 0..8 {
                assert_eq!(t[transform(x, s) as usize], c, "{x:08b} turned {s}");
            }
        }
        let sizes: Vec<usize> = per.iter().map(|p| p.len()).collect();
        assert_eq!(sizes, [1, 2, 6, 10, 13, 10, 6, 2, 1]);
        for n in 0..9 {
            assert_eq!(per[n].len(), LETTERS[n].len().max(1), "count {n}");
        }
        // Distinct letters are distinct classes.
        let mut seen = std::collections::BTreeSet::new();
        for x in 0..=255u8 {
            seen.insert((t[x as usize], canon(x)));
        }
        assert_eq!(seen.len(), 51);
    }

    // The arrangements for 5-7 are the complements of 3-1, letter for letter (Hensel's diagrams).
    #[test]
    fn five_to_seven_are_the_complements() {
        let t = classes();
        for x in 0..=255u8 {
            let (c, d) = (t[x as usize], t[!x as usize]);
            assert_eq!(c >> 4, 8 - (d >> 4));
            if x.count_ones() != 4 {
                assert_eq!(c & 15, d & 15);
            }
        }
        // A 4's complement is a 4, often of another letter: c and e, for one.
        let letter = |x: u8| LETTERS[4].as_bytes()[(t[x as usize] & 15) as usize] as char;
        assert_eq!(
            (letter(NW | NE | SW | SE), letter(N | W | E | S)),
            ('c', 'e')
        );
    }

    #[test]
    fn hensel_s_representatives_by_name() {
        // A few written out cell by cell: 2a is a corner and an edge beside it, 2k a knight's
        // move, 3i a straight side, 4z a zig-zag.
        let class = |cells: &[(i32, i32)]| {
            let x = cells.iter().fold(0u8, |m, c| {
                m | 1 << OFFSETS.iter().position(|o| o == c).unwrap()
            });
            let c = classes()[x as usize];
            let n = (c >> 4) as usize;
            format!("{n}{}", LETTERS[n].as_bytes()[(c & 15) as usize] as char)
        };
        assert_eq!(class(&[(1, 1), (1, 0)]), "2a");
        assert_eq!(class(&[(-1, 1), (0, -1)]), "2k");
        assert_eq!(class(&[(1, -1), (1, 0), (1, 1)]), "3i");
        assert_eq!(class(&[(-1, 0), (0, -1), (1, 0)]), "3e");
        assert_eq!(class(&[(-1, -1), (1, -1), (0, 1)]), "3y");
        assert_eq!(class(&[(-1, -1), (0, -1), (0, 1)]), "3r");
        assert_eq!(class(&[(-1, -1), (0, -1), (0, 1), (1, 1)]), "4z");
        assert_eq!(class(&[(1, -1), (0, -1), (0, 1), (-1, 1)]), "4z");
        assert_eq!(class(&[(-1, -1), (0, -1), (1, 0), (0, 1)]), "4j");
        assert_eq!(class(&[(-1, -1), (0, -1), (-1, 0), (1, 0)]), "4r");
    }

    fn name(text: &str) -> String {
        let t = text.to_ascii_lowercase();
        let parts: Vec<&str> = t.split('/').collect();
        let states = parts
            .get(2)
            .map(|c| c.trim_start_matches('c').parse().unwrap())
            .unwrap_or(2);
        match parse(
            parts[0].strip_prefix('b').unwrap(),
            parts[1].strip_prefix('s').unwrap(),
            states,
        )
        .unwrap()
        {
            Parsed::Isotropic(r) => r.notation(),
            Parsed::Totalistic { birth, survive } => {
                crate::rules::Rule::LifeLike { birth, survive }.notation()
            }
        }
    }

    // The canonical forms Golly's documentation gives as examples ("Non-totalistic rules", in
    // its help for QuickLife and Generations).
    #[test]
    fn canonical_names_follow_golly_s_documented_examples() {
        for (given, canonical) in [
            ("B2nic/S", "B2cin/S"),
            ("B2ceikn/S", "B2-a/S"),
            ("B4-qjrtwz/S", "B4aceikny/S"),
            ("B2aceikn3/S", "B23/S"),
            ("B2-3/S", "B23/S"),
            ("B2-aceikn3/S", "B3/S"),
            ("B2cekin/S12", "B2-a/S12"),
            ("B3/S2a34", "B3/S2a34"),
        ] {
            assert_eq!(name(given), canonical, "{given}");
        }
    }

    #[test]
    fn names_read_back_as_the_same_rule() {
        for text in [
            "B2-a/S12",
            "B3-cnqy/S23-a",
            "B2e3-anq/S2-i3",
            "B1e2-a/S/C4",
            "B02a/S8",
        ] {
            assert_eq!(name(&name(text)), name(text), "{text}");
        }
        assert_eq!(name("B3-cnqy/S23-a"), "B3-cnqy/S23-a");
        // Repeated counts add up; letters in any order.
        assert_eq!(name("B2a2c/S"), "B2ac/S");
        assert_eq!(name("B2ca/S"), "B2ac/S");
    }

    #[test]
    fn bad_letters_are_explained() {
        let e = |b: &str| parse(b, "", 2).unwrap_err();
        assert!(e("1a").contains("1 takes c, e"), "{}", e("1a"));
        assert!(e("5z").contains("5 takes c, e, k, a, i, n, y, q, j, r"));
        assert!(e("0c").contains("takes no letters"));
        assert!(e("9").contains("0 to 8"));
        assert!(e("a").contains("no count before it"));
        assert!(e("2-a-c").contains("'-'"));
    }

    // Every letter of every count is every arrangement of that count: the rule is the B/S rule.
    #[test]
    fn every_letter_is_the_totalistic_rule() {
        let life = parse("3aceijknqry", "2aceikn3aceijknqry", 2).unwrap();
        assert_eq!(
            life,
            Parsed::Totalistic {
                birth: 1 << 3,
                survive: 1 << 2 | 1 << 3
            }
        );
        let r = IsoRule::from_masks(1 << 3, 0b1100, 2);
        for x in 0..=255u8 {
            let n = x.count_ones();
            assert_eq!(r.born(x), n == 3);
            assert_eq!(r.survives(x), n == 2 || n == 3);
        }
    }

    // Hensel's own example (neighbors.html): Just Friends, survival on 1 or 2 and birth on 2
    // except when the two are adjacent (a corner and the edge beside it).
    #[test]
    fn just_friends_as_hensel_describes_it() {
        let Parsed::Isotropic(r) = parse("2-a", "12", 2).unwrap() else {
            panic!()
        };
        for x in 0..=255u8 {
            let adjacent = x.count_ones() == 2
                && OFFSETS.iter().enumerate().any(|(i, a)| {
                    OFFSETS.iter().enumerate().any(|(j, b)| {
                        x >> i & 1 == 1
                            && x >> j & 1 == 1
                            && (a.0 - b.0).abs() + (a.1 - b.1).abs() == 1
                    })
                });
            assert_eq!(r.born(x), x.count_ones() == 2 && !adjacent, "{x:08b}");
            assert_eq!(r.survives(x), matches!(x.count_ones(), 1 | 2));
        }
    }

    // The decision diagrams give the rule, for every neighbourhood at once.
    #[test]
    fn diagrams_are_the_rule() {
        for (b, s) in [
            ("3-cnqy", "23-a"),
            ("2-a", "12"),
            ("1e2ak3", "2-i3"),
            ("3", ""),
        ] {
            let r = IsoRule {
                birth: parse_counts(b).unwrap(),
                survive: parse_counts(s).unwrap(),
                states: 2,
            };
            for live in [false, true] {
                let d = Diagram::of(&r, live);
                // 256 neighbourhoods, four words of 64 lanes.
                let mut scratch = Vec::new();
                for w in 0..4u64 {
                    let x: [u64; 8] = std::array::from_fn(|k| {
                        (0..64u64).fold(0, |m, lane| m | ((((w * 64 + lane) >> k) & 1) << lane))
                    });
                    let got = d.eval(&x, &mut scratch);
                    for lane in 0..64u64 {
                        let n = (w * 64 + lane) as u8;
                        let want = if live { r.survives(n) } else { r.born(n) };
                        assert_eq!(got >> lane & 1 == 1, want, "{b}/{s} {n}");
                    }
                }
                // Shared nodes keep it small: far fewer than the 255 of a full tree.
                assert!(d.nodes.len() < 120, "{b}/{s}: {}", d.nodes.len());
            }
        }
    }

    // Class numbers run count by count, letter by letter, and the masks over them are the rule.
    #[test]
    fn class_numbers_and_masks() {
        let mut seen = std::collections::BTreeSet::new();
        for x in 0..=255u8 {
            let k = class_number(x);
            assert!(k < CLASSES);
            seen.insert(k);
            let c = classes()[x as usize];
            assert_eq!(k, CLASS_START[(c >> 4) as usize] + (c & 15) as u32);
        }
        assert_eq!(seen.len(), 51);
        assert_eq!(
            (
                class_name(0),
                class_name(1),
                class_name(4),
                class_name(18),
                class_name(50)
            ),
            (
                "0".into(),
                "1c".into(),
                "2e".into(),
                "3r".into(),
                "8".into()
            )
        );
        let Parsed::Isotropic(r) = parse("3-cnqy", "23-a", 2).unwrap() else {
            panic!()
        };
        let (b, s) = r.class_masks();
        assert_eq!(IsoRule::from_class_masks(b, s, 2), r);
        for x in 0..=255u8 {
            let k = class_number(x);
            assert_eq!(b >> k & 1 == 1, r.born(x));
            assert_eq!(s >> k & 1 == 1, r.survives(x));
        }
    }

    #[test]
    fn the_table_is_the_rule() {
        let Parsed::Isotropic(r) = parse("3-cnqy", "23-a", 2).unwrap() else {
            panic!()
        };
        let t = r.table();
        for x in 0..256usize {
            assert_eq!(t[x / 32] >> (x % 32) & 1 == 1, r.born(x as u8));
            assert_eq!(t[8 + x / 32] >> (x % 32) & 1 == 1, r.survives(x as u8));
        }
    }
}
