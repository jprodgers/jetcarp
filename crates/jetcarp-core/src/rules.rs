//! Rule notation and the named Phase 0 catalog.
//!
//! Notation (case-insensitive):
//! - Life-like: `B3/S23`, with an optional `V` suffix for the edge (von Neumann) neighbourhood.
//!   Counts above 9 use commas: `B3,10/S2,3`.
//! - Generations: `B2/S/C3` (LifeWiki form) or `B2/S/3`.
//! - Elementary: `W30`, `Rule 30`.
//! - Turmites: `Ant RL` (Langton's ant), `Turmite LLRR`. Turns: L, R, N (none), U (u-turn), L2 and
//!   R2 (two slots: 120 degrees on hexagons). Several states: `Turmite 1R1,1L0/1R1,0N0` (see
//!   `turmite.rs`).
//! - `WireWorld`, `Sandpile`, `SpectreGlider` (also the prototype's `table:spectre-glider`).
//! - `JvN29`: von Neumann's 29-state automaton (see `vonneumann.rs`).
//! - Multi-state families, parameters separated by slashes:
//!   `Cyclic 14/1` (states/threshold), `GH 8/1` (Greenberg-Hastings states/threshold),
//!   `RPS 3/3` (species/threshold), `ForestFire 0.01/0.00001` (growth p / lightning f),
//!   `Hodgepodge 100/2/3/28` (n/k1/k2/g).
//! - Hexagonal elementary rules with two parents: `HexW 6` (binary, 0-15),
//!   `HexW3 1234` (three states, 0-19682).
//! - Hexagonal elementary rules with three parents, on the flat-top hexagon grid (`hex_flat`):
//!   `HexT 30` (binary, 0-255, index = NW*4 + N*2 + NE), `HexT3 n` (three states, 0 to 3^27 - 1,
//!   index = NW*9 + N*3 + NE), `HexT3 sum 777` (totalistic, output by parent sum 0-6, 0-2186),
//!   `HexT3 sym n` (left-right symmetric, 0 to 3^18 - 1), `HexT sum 6` (binary totalistic, 0-15).
//! - Margolus block rules on the square grid: `Margolus 0,8,4,3,2,5,9,7,1,6,10,11,12,13,14,15`
//!   (the new state of each 2x2 block for block states 0-15, MCell's numbering: upper left 1,
//!   upper right 2, lower left 4, lower right 8); MCell's `MS,D0;8;4;...` also parses, and
//!   `Margolus BBM` / `Critters` / `Tron` / `SingleRotation` / `HPP` name the built-in tables.
//! - Lenia (continuous): `Lenia R=13,T=10,b=1,m=0.15,s=0.015` (see `lenia.rs`).
//! - Rule tables by name: `Table codd`, `Table <name>` (see `table.rs`).
//! - JACA (balanced ternary, annihilating): `JACA B3/S23`, `JACA +B3/S23 -B36/S23 blast 2`
//!   (see `jaca.rs`).

use crate::topology::Nb;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Turn {
    L,
    R,
    N,
    U,
    L2,
    R2,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Rule {
    LifeLike {
        birth: u32,
        survive: u32,
    },
    Generations {
        birth: u32,
        survive: u32,
        states: u8,
    },
    Wolfram(u8),
    Turmite(crate::turmite::Mite),
    WireWorld,
    Sandpile,
    SpectreGlider,
    /// A cell in state s advances to s+1 (mod n) when at least `threshold` neighbours hold s+1.
    Cyclic {
        states: u8,
        threshold: u8,
    },
    /// 0 resting, 1 excited, 2..n-1 refractory. Resting becomes excited with at least
    /// `threshold` excited neighbours; every other state advances, wrapping to rest.
    GreenbergHastings {
        states: u8,
        threshold: u8,
    },
    /// Species s is replaced by its predator s+1 (mod n) when more than `threshold` neighbours are
    /// the predator (Softology's rock-paper-scissors CA, Moore neighbourhood).
    Rps {
        states: u8,
        threshold: u8,
    },
    /// 0 empty, 1 tree, 2 fire. Fire burns out; a tree catches fire from a burning neighbour or by
    /// lightning (probability f); empty ground grows a tree with probability p.
    ForestFire {
        p: f64,
        f: f64,
    },
    /// Gerhardt-Schuster hodgepodge machine with states 0..=n.
    Hodgepodge {
        n: u8,
        k1: u32,
        k2: u32,
        g: u32,
    },
    /// Elementary rule on hexagon rows: each cell reads the two cells touching it in the row above.
    HexWolfram {
        states: u8,
        number: u32,
    },
    /// Elementary rule on the flat-top hexagon grid with three parents: the cells touching a
    /// cell's upper edges (NW, N, NE). See [`HexTMode`] and [`Rule::hext_table`].
    HexTri {
        states: u8,
        mode: HexTMode,
        number: u64,
    },
    /// Margolus block rule: the grid is cut into 2x2 blocks, offset by one cell on odd generations,
    /// and each block's state (upper left 1, upper right 2, lower left 4, lower right 8) is
    /// replaced by `table[state]`.
    Margolus([u8; 16]),
    /// Lenia, a continuous rule (values in [0, 1]); see `lenia.rs`.
    Lenia(Box<crate::lenia::LeniaRule>),
    /// A transition table, looked up by name.
    Table(String),
    /// Larger than Life: range-r outer totalistic (`R5,C2,S33-57,B34-45`).
    Ltl(Box<crate::ltl::LtlRule>),
    /// Layered (Wolfram-style) totalistic rule on any tiling: layers are graph distance from the
    /// seed cells, and each step writes the next layer. A cell's new state is `digits[sum]`, where
    /// `sum` is the total state of its parents (neighbours in the previous layer); the digit list
    /// repeats for larger sums. Notation `Layer 2/01` (parity), `Layer 3/012` (sum mod 3).
    Layered {
        states: u8,
        digits: Vec<u8>,
    },
    /// Von Neumann's 29-state self-reproduction automaton (square grid, edge neighbours).
    VonNeumann,
    /// Priority table (Edmond's glider-search genome; `evolve::priority`):
    /// `Priority 4: 0|n0>=3&n1>=1->3; 3|n3>=3->0; 3|n1>=1->1`.
    Priority(Box<crate::evolve::priority::PriorityRule>),
    /// JACA: cells -1, 0, +1, a B/S rule per sign, and opposite signs that touch annihilate
    /// (see `jaca.rs`).
    Jaca(Box<crate::jaca::JacaRule>),
}

/// How a three-parent hexagon rule number is read.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum HexTMode {
    /// One output per parent triple: digit `NW*k^2 + N*k + NE` of the number in base k
    /// (Wolfram's numbering; k^3 digits).
    Full,
    /// Totalistic: digit `NW + N + NE` (the parent sum, 0..=3(k-1)) of the number in base k
    /// (Wolfram's totalistic code).
    Sum,
    /// Left-right symmetric (NW and NE unordered): digit `p*k + N`, where p numbers the unordered
    /// pair {NW, NE} in the order (0,0), (0,1), ..., (0,k-1), (1,1), (1,2), ..., (k-1,k-1).
    Sym,
}

impl HexTMode {
    /// Digits (table entries) of a rule with `k` states.
    pub fn digits(self, k: u32) -> u32 {
        match self {
            HexTMode::Full => k * k * k,
            HexTMode::Sum => 3 * (k - 1) + 1,
            HexTMode::Sym => k * (k + 1) / 2 * k,
        }
    }
    /// The largest rule number for `k` states (k^digits - 1).
    pub fn max(self, k: u32) -> u64 {
        (k as u64).pow(self.digits(k)) - 1
    }
}

/// Which kind of cell field a rule needs.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FieldKind {
    /// Live or dead, plus age.
    Binary,
    /// State number 0..n, plus age.
    Multi(u8),
    /// Grain count.
    Height,
    /// A value in [0, 1] per cell (f32), plus an 8-bit copy for display and counting
    /// (`ceil(255 v)`, so it is nonzero exactly when the value is).
    Continuous,
}

impl FieldKind {
    pub fn name(&self) -> String {
        match self {
            FieldKind::Binary => "binary".into(),
            FieldKind::Multi(n) => format!("{n}-state"),
            FieldKind::Height => "height".into(),
            FieldKind::Continuous => "continuous".into(),
        }
    }
}

impl Rule {
    pub fn field_kind(&self) -> FieldKind {
        match self {
            Rule::LifeLike { .. } | Rule::Wolfram(_) => FieldKind::Binary,
            Rule::Generations { states, .. } => FieldKind::Multi(*states),
            Rule::Turmite(t) if t.colors <= 2 => FieldKind::Binary,
            Rule::Turmite(t) => FieldKind::Multi(t.colors),
            Rule::WireWorld | Rule::SpectreGlider => FieldKind::Multi(4),
            Rule::Sandpile => FieldKind::Height,
            Rule::Cyclic { states, .. }
            | Rule::GreenbergHastings { states, .. }
            | Rule::Rps { states, .. } => FieldKind::Multi(*states),
            Rule::ForestFire { .. } => FieldKind::Multi(3),
            Rule::Hodgepodge { n, .. } => FieldKind::Multi(n.saturating_add(1)),
            Rule::HexWolfram { states: 2, .. } => FieldKind::Binary,
            Rule::HexWolfram { states, .. } => FieldKind::Multi(*states),
            Rule::HexTri { states: 2, .. } => FieldKind::Binary,
            Rule::Margolus(_) => FieldKind::Binary,
            Rule::Lenia(_) => FieldKind::Continuous,
            Rule::HexTri { states, .. } => FieldKind::Multi(*states),
            // Resolved against the table's own state count when the CA is added.
            Rule::Table(_) => FieldKind::Multi(2),
            Rule::Layered { states: 2, .. } => FieldKind::Binary,
            Rule::Layered { states, .. } => FieldKind::Multi(*states),
            Rule::Ltl(l) if l.states <= 2 => FieldKind::Binary,
            Rule::Ltl(l) => FieldKind::Multi(l.states),
            Rule::VonNeumann => FieldKind::Multi(crate::vonneumann::STATES),
            Rule::Priority(p) => FieldKind::Multi(p.states),
            Rule::Jaca(_) => FieldKind::Multi(3),
        }
    }
    pub fn default_nb(&self) -> Nb {
        match self {
            Rule::Turmite(_) | Rule::Sandpile | Rule::Cyclic { .. } | Rule::Table(_) => Nb::Edge,
            Rule::VonNeumann => Nb::Edge,
            // Larger than Life's own neighbourhood decides: NM reaches over touching neighbours,
            // NN (von Neumann) and N+ (the cross) over edge neighbours (`ltl::reach`).
            Rule::Ltl(l) if l.hood != crate::ltl::Hood::Moore => Nb::Edge,
            _ => Nb::Touch,
        }
    }
    /// Turmites always walk across edges; Wolfram rows have no neighbourhood choice.
    pub fn nb_fixed(&self) -> bool {
        matches!(
            self,
            Rule::Turmite(_)
                | Rule::Wolfram(_)
                | Rule::HexWolfram { .. }
                | Rule::HexTri { .. }
                | Rule::Margolus(_)
                | Rule::Lenia(_)
                | Rule::Ltl(_)
                | Rule::VonNeumann
        )
    }
    /// Elementary rules write one row per step and need a grid with rows.
    pub fn is_rows(&self) -> bool {
        matches!(
            self,
            Rule::Wolfram(_) | Rule::HexWolfram { .. } | Rule::HexTri { .. } | Rule::Layered { .. }
        )
    }
    /// The output for each parent triple of a three-parent hexagon rule, indexed by
    /// `NW*k^2 + N*k + NE` (k = states).
    pub fn hext_table(&self) -> Option<Vec<u8>> {
        let Rule::HexTri {
            states,
            mode,
            number,
        } = *self
        else {
            return None;
        };
        let k = states as u64;
        let digit = |i: u64| ((number / k.pow(i as u32)) % k) as u8;
        // Index of the unordered pair {a, b}: pairs (lo, hi) with lo < this lo come first.
        let pair = |a: u64, b: u64| -> u64 {
            let (lo, hi) = (a.min(b), a.max(b));
            (0..lo).map(|x| k - x).sum::<u64>() + (hi - lo)
        };
        Some(
            (0..k * k * k)
                .map(|idx| {
                    let (nw, n, ne) = (idx / (k * k), idx / k % k, idx % k);
                    match mode {
                        HexTMode::Full => digit(idx),
                        HexTMode::Sum => digit(nw + n + ne),
                        HexTMode::Sym => digit(pair(nw, ne) * k + n),
                    }
                })
                .collect(),
        )
    }
    /// Rules that draw on per-cell randomness each step.
    pub fn is_stochastic(&self) -> bool {
        matches!(self, Rule::ForestFire { .. })
    }
    /// Shared fields are the default only for binary rules (plan: "Shared by compatible CAs").
    pub fn default_affects_others(&self) -> bool {
        self.field_kind() == FieldKind::Binary
    }
    pub fn is_agent(&self) -> bool {
        matches!(self, Rule::Turmite(_))
    }

    pub fn parse(text: &str) -> Result<(Rule, Option<Nb>), String> {
        let t = text.trim();
        let low = t.to_ascii_lowercase();
        let compact: String = low.chars().filter(|c| !c.is_whitespace()).collect();
        match compact.as_str() {
            "wireworld" => return Ok((Rule::WireWorld, None)),
            "sandpile" | "abeliansandpile" => return Ok((Rule::Sandpile, None)),
            "spectreglider" | "table:spectre-glider" => return Ok((Rule::SpectreGlider, None)),
            "jvn29" | "jvn" | "vonneumann29" | "vonneumann" => return Ok((Rule::VonNeumann, None)),
            _ => {}
        }
        if let Some(rest) = compact
            .strip_prefix("rule")
            .or_else(|| compact.strip_prefix('w'))
            && !rest.is_empty()
            && rest.chars().all(|c| c.is_ascii_digit())
        {
            let n: u32 = rest.parse().map_err(|_| "bad Wolfram rule number")?;
            if n > 255 {
                return Err("Wolfram rules run 0 to 255".into());
            }
            return Ok((Rule::Wolfram(n as u8), None));
        }
        for prefix in ["turmite", "ant"] {
            if let Some(rest) = compact.strip_prefix(prefix) {
                let rest = rest.trim_start_matches(':');
                return Ok((Rule::Turmite(crate::turmite::Mite::parse_body(rest)?), None));
            }
        }
        if let Some(l) = crate::lenia::LeniaRule::parse(t) {
            return Ok((Rule::Lenia(Box::new(l?)), None));
        }
        if let Some(r) = crate::jaca::JacaRule::parse(t) {
            let (r, nb) = r?;
            return Ok((Rule::Jaca(Box::new(r)), nb));
        }
        if let Some(t) = parse_margolus(&compact)? {
            return Ok((Rule::Margolus(t), None));
        }
        if let Some(r) = crate::evolve::priority::PriorityRule::parse(t) {
            return Ok((Rule::Priority(Box::new(r?)), None));
        }
        if let Some(r) = parse_family(&low)? {
            return Ok((r, None));
        }
        if let Some(r) = crate::ltl::LtlRule::parse(t) {
            return Ok((Rule::Ltl(Box::new(r?)), None));
        }
        // B/S notation. A `V` (edge neighbours) ends the rule, or the B or S part it sits
        // beside: `B3/S23V/C4` and `B3V/S23/C4` are `B3/S23/C4V`.
        let (body, mut edge) = if let Some(b) = compact.strip_suffix('v') {
            (b, true)
        } else if let Some(b) = compact.strip_suffix('h') {
            (b, false)
        } else {
            (compact.as_str(), false)
        };
        let mut parts: Vec<&str> = body.split('/').collect();
        if !(2..=3).contains(&parts.len()) {
            return Err(format!(
                "'{t}' is not a rule. Try B3/S23, B2/S/C3, W30, Ant RL, WireWorld, or Sandpile."
            ));
        }
        for p in parts.iter_mut().take(2) {
            if let Some(q) = p.strip_suffix('v') {
                *p = q;
                edge = true;
            }
        }
        let rule = bs_parts(&parts).map_err(|e| {
            if parts.iter().any(|p| p.contains('v')) {
                misplaced_v(&parts, e)
            } else {
                e
            }
        })?;
        Ok((rule, edge.then_some(Nb::Edge)))
    }

    pub fn notation(&self) -> String {
        match self {
            Rule::LifeLike { birth, survive } => {
                format!("B{}/S{}", digits(*birth), digits(*survive))
            }
            Rule::Generations {
                birth,
                survive,
                states,
            } => {
                format!("B{}/S{}/C{}", digits(*birth), digits(*survive), states)
            }
            Rule::Wolfram(n) => format!("W{n}"),
            Rule::Turmite(t) => t.notation(),
            Rule::WireWorld => "WireWorld".into(),
            Rule::Sandpile => "Sandpile".into(),
            Rule::SpectreGlider => "SpectreGlider".into(),
            Rule::Cyclic { states, threshold } => format!("Cyclic {states}/{threshold}"),
            Rule::GreenbergHastings { states, threshold } => format!("GH {states}/{threshold}"),
            Rule::Rps { states, threshold } => format!("RPS {states}/{threshold}"),
            Rule::ForestFire { p, f } => format!("ForestFire {p}/{f}"),
            Rule::Hodgepodge { n, k1, k2, g } => format!("Hodgepodge {n}/{k1}/{k2}/{g}"),
            Rule::HexWolfram { states: 2, number } => format!("HexW {number}"),
            Rule::HexWolfram { states, number } => format!("HexW{states} {number}"),
            Rule::Lenia(l) => l.notation(),
            Rule::Margolus(t) => {
                let v: Vec<String> = t.iter().map(|x| x.to_string()).collect();
                format!("Margolus {}", v.join(","))
            }
            Rule::HexTri {
                states,
                mode,
                number,
            } => {
                let s = if *states == 2 {
                    String::new()
                } else {
                    states.to_string()
                };
                let m = match mode {
                    HexTMode::Full => "",
                    HexTMode::Sum => "sum ",
                    HexTMode::Sym => "sym ",
                };
                format!("HexT{s} {m}{number}")
            }
            Rule::Table(name) => format!("Table {name}"),
            Rule::Ltl(l) => l.notation(),
            Rule::Layered { states, digits } => {
                let d: String = digits.iter().map(|x| char::from(b'0' + x)).collect();
                format!("Layer {states}/{d}")
            }
            Rule::VonNeumann => "JvN29".into(),
            Rule::Priority(p) => p.notation(),
            Rule::Jaca(j) => j.notation(),
        }
    }

    /// Display name: the catalog name if this is a named rule, otherwise the notation.
    pub fn display_name(&self) -> String {
        let n = self.notation();
        catalog()
            .iter()
            .find(|e| e.notation == n)
            .map(|e| e.name.to_string())
            .unwrap_or(n)
    }
}

/// Named Margolus block tables (block states: upper left 1, upper right 2, lower left 4,
/// lower right 8), built from the published rule statements; the book's "Margolus block
/// rules" page gives the sources.
pub fn margolus_named(name: &str) -> Option<[u8; 16]> {
    let rot180 = |x: u8| (x & 1) << 3 | (x & 2) << 1 | (x & 4) >> 1 | (x & 8) >> 3;
    let mut t = [0u8; 16];
    for (x, o) in t.iter_mut().enumerate() {
        let x = x as u8;
        *o = match name {
            // Billiard ball machine (Margolus 1984): a lone particle crosses to the opposite
            // corner; two particles on a diagonal leave on the other diagonal; all else stays.
            "bbm" | "billiardball" | "billiardballmachine" => match x {
                1 | 2 | 4 | 8 => rot180(x),
                9 => 6,
                6 => 9,
                _ => x,
            },
            // Critters (Toffoli & Margolus): blocks with exactly two live cells stay; others are
            // complemented, and blocks with three live cells are also turned 180 degrees.
            "critters" => match x.count_ones() {
                2 => x,
                3 => rot180(!x & 15),
                _ => !x & 15,
            },
            // Tron (Toffoli & Margolus): a block whose four cells agree is complemented.
            "tron" => match x {
                0 => 15,
                15 => 0,
                _ => x,
            },
            // Single Rotation (Mishin 2013): a block with exactly one live cell turns 90 degrees
            // clockwise (upper left to upper right to lower right to lower left).
            "singlerotation" | "singlerot" => match x {
                1 => 2,
                2 => 8,
                8 => 4,
                4 => 1,
                _ => x,
            },
            // HPP lattice gas, diagonal version (Margolus): every particle crosses to the opposite
            // corner of its block, except that two particles on a diagonal leave on the other.
            "hpp" | "hppgas" => match x {
                9 => 6,
                6 => 9,
                _ => rot180(x),
            },
            _ => return None,
        };
    }
    Some(t)
}

/// `Margolus <16 outputs>` (commas or semicolons), MCell's `MS,D<16 outputs>`, or
/// `Margolus <name>`. `compact` is lower case without spaces.
fn parse_margolus(compact: &str) -> Result<Option<[u8; 16]>, String> {
    let rest = if let Some(r) = compact.strip_prefix("margolus") {
        r.trim_start_matches(':')
    } else if let Some(r) = compact.strip_prefix("ms,d") {
        r
    } else {
        return Ok(None);
    };
    if let Some(t) = margolus_named(rest) {
        return Ok(Some(t));
    }
    let v: Result<Vec<u8>, _> = rest
        .split([',', ';'])
        .filter(|x| !x.is_empty())
        .map(|x| x.parse::<u8>())
        .collect();
    let v = v.map_err(|_| "Margolus rules list 16 block states (0-15), like 'Margolus 0,8,4,3,2,5,9,7,1,6,10,11,12,13,14,15', or a name: BBM, Critters, Tron, SingleRotation, HPP")?;
    if v.len() != 16 || v.iter().any(|&x| x > 15) {
        return Err("Margolus rules list exactly 16 block states, each 0-15".into());
    }
    let mut t = [0u8; 16];
    t.copy_from_slice(&v);
    Ok(Some(t))
}

/// Parse the slash-parameter families (`Cyclic 14/1` and so on). `None` if `low` is not one.
fn parse_family(low: &str) -> Result<Option<Rule>, String> {
    let mut words = low.split_whitespace();
    let Some(head) = words.next() else {
        return Ok(None);
    };
    let rest: String = words.collect::<Vec<_>>().join("");
    let nums = |n: usize, what: &str| -> Result<Vec<f64>, String> {
        let v: Result<Vec<f64>, _> = rest
            .split('/')
            .filter(|x| !x.is_empty())
            .map(|x| x.parse::<f64>())
            .collect();
        let v = v.map_err(|_| format!("{what}: parameters must be numbers"))?;
        if v.len() != n {
            return Err(format!("{what} takes {n} parameters separated by '/'"));
        }
        Ok(v)
    };
    let small = |x: f64, lo: f64, hi: f64, what: &str| -> Result<u32, String> {
        if x.fract() != 0.0 || x < lo || x > hi {
            return Err(format!("{what} must be a whole number from {lo} to {hi}"));
        }
        Ok(x as u32)
    };
    Ok(Some(match head {
        "cyclic" => {
            let v = if rest.is_empty() {
                vec![16.0, 1.0]
            } else {
                nums(2, "Cyclic")?
            };
            Rule::Cyclic {
                states: small(v[0], 2.0, 255.0, "states")? as u8,
                threshold: small(v[1], 1.0, 32.0, "threshold")? as u8,
            }
        }
        "gh" | "greenberghastings" | "greenberg-hastings" => {
            let v = if rest.is_empty() {
                vec![3.0, 1.0]
            } else {
                nums(2, "GH")?
            };
            Rule::GreenbergHastings {
                states: small(v[0], 3.0, 255.0, "states")? as u8,
                threshold: small(v[1], 1.0, 32.0, "threshold")? as u8,
            }
        }
        "rps" => {
            let v = if rest.is_empty() {
                vec![3.0, 3.0]
            } else {
                nums(2, "RPS")?
            };
            Rule::Rps {
                states: small(v[0], 2.0, 255.0, "species")? as u8,
                threshold: small(v[1], 1.0, 32.0, "threshold")? as u8,
            }
        }
        "forestfire" | "forest" => {
            let v = if rest.is_empty() {
                vec![0.01, 0.0001]
            } else {
                nums(2, "ForestFire")?
            };
            if !(0.0..=1.0).contains(&v[0]) || !(0.0..=1.0).contains(&v[1]) {
                return Err("ForestFire probabilities must be between 0 and 1".into());
            }
            Rule::ForestFire { p: v[0], f: v[1] }
        }
        "hodgepodge" => {
            let v = if rest.is_empty() {
                vec![200.0, 3.0, 3.0, 28.0]
            } else {
                nums(4, "Hodgepodge")?
            };
            Rule::Hodgepodge {
                n: small(v[0], 2.0, 254.0, "n")? as u8,
                k1: small(v[1], 1.0, 1000.0, "k1")?,
                k2: small(v[2], 1.0, 1000.0, "k2")?,
                g: small(v[3], 0.0, 1000.0, "g")?,
            }
        }
        "hexw" | "hexw2" => {
            let v = nums(1, "HexW")?;
            Rule::HexWolfram {
                states: 2,
                number: small(v[0], 0.0, 15.0, "binary hex rule")?,
            }
        }
        "hexw3" => {
            let v = nums(1, "HexW3")?;
            Rule::HexWolfram {
                states: 3,
                number: small(v[0], 0.0, 19682.0, "ternary hex rule")?,
            }
        }
        "hext" | "hext2" | "hext3" => {
            let k: u32 = if head == "hext3" { 3 } else { 2 };
            let (mode, num) = if let Some(n) = rest.strip_prefix("sum") {
                (HexTMode::Sum, n)
            } else if let Some(n) = rest.strip_prefix("sym") {
                (HexTMode::Sym, n)
            } else {
                (HexTMode::Full, rest.as_str())
            };
            let name = if k == 2 { "HexT" } else { "HexT3" };
            let number: u64 = num.parse().map_err(|_| {
                format!("{name} takes a rule number, like '{name} 30', '{name} sum 6', or '{name} sym 9'")
            })?;
            let max = mode.max(k);
            if number > max {
                return Err(format!("{name} rules of this kind run 0 to {max}"));
            }
            Rule::HexTri {
                states: k as u8,
                mode,
                number,
            }
        }
        "layer" | "layered" => {
            let (k, d) = rest.split_once('/').ok_or(
                "Layered rules look like 'Layer 2/01': states, then outputs by parent sum",
            )?;
            let k: u8 = k
                .parse()
                .map_err(|_| "Layered: state count must be a number")?;
            if !(2..=9).contains(&k) {
                return Err("Layered rules have 2 to 9 states".into());
            }
            let digits: Result<Vec<u8>, String> = d
                .chars()
                .map(|c| match c.to_digit(10) {
                    Some(v) if (v as u8) < k => Ok(v as u8),
                    _ => Err(format!("Layered: '{c}' is not a state below {k}")),
                })
                .collect();
            let digits = digits?;
            if digits.is_empty() || digits.len() > 64 {
                return Err("Layered: give 1 to 64 output digits".into());
            }
            Rule::Layered { states: k, digits }
        }
        "table" => {
            let name: String = low.split_whitespace().skip(1).collect::<Vec<_>>().join(" ");
            if name.is_empty() {
                return Err("Table needs a name, like 'Table codd'".into());
            }
            Rule::Table(name)
        }
        _ => return Ok(None),
    }))
}

/// A B/S or Generations rule from its parts: `b<counts>`, `s<counts>`, and an optional state
/// count (lower case, any `V` already taken off).
fn bs_parts(parts: &[&str]) -> Result<Rule, String> {
    let b = parts[0]
        .strip_prefix('b')
        .ok_or("B/S rules start with B, like B3/S23")?;
    let s = parts[1]
        .strip_prefix('s')
        .ok_or("the second part starts with S, like B3/S23")?;
    let birth = mask(b)?;
    let survive = mask(s)?;
    if parts.len() == 3 {
        let c = parts[2].trim_start_matches(['c', 'g']);
        let states: u32 = c
            .parse()
            .map_err(|_| "Generations need a state count, like B2/S/C3")?;
        if !(2..=255).contains(&states) {
            return Err("Generations state count must be 2 to 255".into());
        }
        if states > 2 {
            return Ok(Rule::Generations {
                birth,
                survive,
                states: states as u8,
            });
        }
    }
    Ok(Rule::LifeLike { birth, survive })
}

/// The error for a rule with a `V` among its counts: where the `V` goes, with the rule written
/// that way when it reads without the stray `V`.
fn misplaced_v(parts: &[&str], e: String) -> String {
    let plain: Vec<String> = parts.iter().map(|p| p.replace('v', "")).collect();
    let plain: Vec<&str> = plain.iter().map(String::as_str).collect();
    match bs_parts(&plain) {
        Ok(r) => format!(
            "a V goes at the end of the rule or right after its B or S counts: {}V",
            r.notation()
        ),
        Err(_) => format!(
            "{e}; a V goes at the end of the rule (B2/S/C3V) or right after its B or S counts \
             (B3/S23V/C4)"
        ),
    }
}

fn mask(s: &str) -> Result<u32, String> {
    let mut m = 0u32;
    if s.contains(',') {
        for part in s.split(',').filter(|p| !p.is_empty()) {
            let n: u32 = part
                .parse()
                .map_err(|_| format!("'{part}' is not a neighbour count"))?;
            if n > 31 {
                return Err("neighbour counts above 31 are not supported".into());
            }
            m |= 1 << n;
        }
    } else {
        for c in s.chars() {
            let n = c
                .to_digit(10)
                .ok_or_else(|| format!("'{c}' is not a neighbour count"))?;
            m |= 1 << n;
        }
    }
    Ok(m)
}

/// A B/S neighbour-count list (`23`, or `2,3,10`) as a bit mask.
pub fn count_mask(s: &str) -> Result<u32, String> {
    mask(s)
}

/// A neighbour-count mask as B/S digits.
pub fn count_digits(m: u32) -> String {
    digits(m)
}

/// The B/S or Generations rules one birth or survival count away from `rule` (each count of
/// `birth` and `survive` toggled in turn; birth 0 is never toggled), births first, in the order
/// given. Generations rules keep their state count.
pub fn rule_neighbours(rule: &str, birth: &[u32], survive: &[u32]) -> Result<Vec<String>, String> {
    let (r, _) = Rule::parse(rule)?;
    let (b, s, states) = match r {
        Rule::LifeLike { birth, survive } => (birth, survive, 2u8),
        Rule::Generations {
            birth,
            survive,
            states,
        } => (birth, survive, states),
        _ => return Err(format!("{rule} is not a B/S or Generations rule")),
    };
    let make = |b: u32, s: u32| -> String {
        if states > 2 {
            Rule::Generations {
                birth: b,
                survive: s,
                states,
            }
            .notation()
        } else {
            Rule::LifeLike {
                birth: b,
                survive: s,
            }
            .notation()
        }
    };
    let mut out = Vec::new();
    for &d in birth.iter().filter(|&&d| d >= 1) {
        out.push(make(b ^ (1 << d), s));
    }
    for &d in survive {
        out.push(make(b, s ^ (1 << d)));
    }
    Ok(out)
}

fn digits(m: u32) -> String {
    let list: Vec<u32> = (0..32).filter(|i| m >> i & 1 == 1).collect();
    if list.iter().all(|&x| x < 10) {
        list.iter().map(|x| x.to_string()).collect()
    } else if list.len() == 1 {
        // A lone count above 9 keeps a comma, so `B10,` does not read back as counts 1 and 0.
        format!("{},", list[0])
    } else {
        list.iter()
            .map(|x| x.to_string())
            .collect::<Vec<_>>()
            .join(",")
    }
}

#[derive(Clone, Copy, Debug)]
pub struct CatalogEntry {
    pub name: &'static str,
    pub notation: &'static str,
    pub family: &'static str,
    pub blurb: &'static str,
}

/// Named rules. Life-like and Generations strings were checked against LifeWiki (via its public
/// mirror) and Wikipedia; multi-state defaults come from Wikipedia and Fisch-Gravner-Griffeath 1991.
/// Names follow the plan; where LifeWiki's name differs, the blurb says so.
pub static CATALOG: &[CatalogEntry] = &[
    CatalogEntry {
        name: "Life",
        notation: "B3/S23",
        family: "Life-like",
        blurb: "Conway's Game of Life.",
    },
    CatalogEntry {
        name: "HighLife",
        notation: "B36/S23",
        family: "Life-like",
        blurb: "Life plus birth on 6; has a replicator.",
    },
    CatalogEntry {
        name: "Seeds",
        notation: "B2/S",
        family: "Life-like",
        blurb: "Every live cell dies each generation; explosive.",
    },
    CatalogEntry {
        name: "Day & Night",
        notation: "B3678/S34678",
        family: "Life-like",
        blurb: "Symmetric under swapping live and dead.",
    },
    CatalogEntry {
        name: "Replicator",
        notation: "B1357/S1357",
        family: "Life-like",
        blurb: "Every pattern copies itself.",
    },
    CatalogEntry {
        name: "Fredkin",
        notation: "B1357/S02468",
        family: "Life-like",
        blurb: "Replicator 2: another parity rule.",
    },
    CatalogEntry {
        name: "Life without Death",
        notation: "B3/S012345678",
        family: "Life-like",
        blurb: "Cells never die; grows ladders and inkspots.",
    },
    CatalogEntry {
        name: "34 Life",
        notation: "B34/S34",
        family: "Life-like",
        blurb: "LifeWiki: 3-4 Life.",
    },
    CatalogEntry {
        name: "Diamoeba",
        notation: "B35678/S5678",
        family: "Life-like",
        blurb: "Large diamond-shaped amoebas.",
    },
    CatalogEntry {
        name: "2x2",
        notation: "B36/S125",
        family: "Life-like",
        blurb: "Patterns of 2x2 blocks behave like a scaled rule.",
    },
    CatalogEntry {
        name: "Morley",
        notation: "B368/S245",
        family: "Life-like",
        blurb: "LifeWiki: Move. Many small spaceships.",
    },
    CatalogEntry {
        name: "Anneal",
        notation: "B4678/S35678",
        family: "Life-like",
        blurb: "LifeWiki: Vote 4/5. Regions smooth out over time.",
    },
    CatalogEntry {
        name: "Maze",
        notation: "B3/S12345",
        family: "Life-like",
        blurb: "Grows maze-like corridors.",
    },
    CatalogEntry {
        name: "Coral",
        notation: "B3/S45678",
        family: "Life-like",
        blurb: "Slow coral-like growth.",
    },
    CatalogEntry {
        name: "Gnarl",
        notation: "B1/S1",
        family: "Life-like",
        blurb: "Explosive gnarled growth from a single cell.",
    },
    CatalogEntry {
        name: "Long Life",
        notation: "B345/S5",
        family: "Life-like",
        blurb: "LifeWiki: LongLife. Long-period oscillators.",
    },
    CatalogEntry {
        name: "Stains",
        notation: "B3678/S235678",
        family: "Life-like",
        blurb: "Stains spread and stabilise.",
    },
    CatalogEntry {
        name: "Coagulations",
        notation: "B378/S235678",
        family: "Life-like",
        blurb: "Blobs coagulate and grow.",
    },
    CatalogEntry {
        name: "Walled Cities",
        notation: "B45678/S2345",
        family: "Life-like",
        blurb: "Cities grow inside walls.",
    },
    CatalogEntry {
        name: "Vote",
        notation: "B5678/S45678",
        family: "Life-like",
        blurb: "Majority vote of the 3x3 block.",
    },
    CatalogEntry {
        name: "Flock",
        notation: "B3/S12",
        family: "Life-like",
        blurb: "Small flocking patterns.",
    },
    CatalogEntry {
        name: "Serviettes",
        notation: "B234/S",
        family: "Life-like",
        blurb: "Persian-rug patterns from small seeds.",
    },
    CatalogEntry {
        name: "DotLife",
        notation: "B3/S023",
        family: "Life-like",
        blurb: "Life where isolated dots survive.",
    },
    CatalogEntry {
        name: "LowLife",
        notation: "B3/S13",
        family: "Life-like",
        blurb: "Life with survival on 1 or 3.",
    },
    CatalogEntry {
        name: "Pedestrian Life",
        notation: "B38/S23",
        family: "Life-like",
        blurb: "Life plus birth on 8.",
    },
    CatalogEntry {
        name: "HoneyLife",
        notation: "B38/S238",
        family: "Life-like",
        blurb: "Life plus birth and survival on 8.",
    },
    CatalogEntry {
        name: "Assimilation",
        notation: "B345/S4567",
        family: "Life-like",
        blurb: "Diamond shapes that absorb their neighbours.",
    },
    CatalogEntry {
        name: "Gnarl (von Neumann)",
        notation: "B1/S0V",
        family: "Life-like, von Neumann",
        blurb: "Kellie Evans's von Neumann Gnarl.",
    },
    CatalogEntry {
        name: "Replicator (von Neumann)",
        notation: "B13/S13V",
        family: "Life-like, von Neumann",
        blurb: "Parity replicator on edge neighbours.",
    },
    CatalogEntry {
        name: "Fredkin (von Neumann)",
        notation: "B13/S024V",
        family: "Life-like, von Neumann",
        blurb: "Fredkin's variant on edge neighbours.",
    },
    CatalogEntry {
        name: "Brian's Brain",
        notation: "B2/S/C3",
        family: "Generations",
        blurb: "Firing, refractory, ready. Full of gliders.",
    },
    CatalogEntry {
        name: "Star Wars",
        notation: "B2/S345/C4",
        family: "Generations",
        blurb: "Also MirekGro. Fast-moving starships.",
    },
    CatalogEntry {
        name: "Frogs",
        notation: "B34/S12/C3",
        family: "Generations",
        blurb: "Hopping frogs.",
    },
    CatalogEntry {
        name: "Caterpillars",
        notation: "B378/S124567/C4",
        family: "Generations",
        blurb: "Long crawling caterpillars.",
    },
    CatalogEntry {
        name: "Banners",
        notation: "B3457/S2367/C5",
        family: "Generations",
        blurb: "Banner-like waves.",
    },
    CatalogEntry {
        name: "Transers",
        notation: "B26/S345/C5",
        family: "Generations",
        blurb: "",
    },
    CatalogEntry {
        name: "Sticks",
        notation: "B2/S3456/C6",
        family: "Generations",
        blurb: "Grows stick-like structures.",
    },
    CatalogEntry {
        name: "Worms",
        notation: "B25/S3467/C6",
        family: "Generations",
        blurb: "",
    },
    CatalogEntry {
        name: "Lava",
        notation: "B45678/S12345/C8",
        family: "Generations",
        blurb: "Slow lava flows.",
    },
    CatalogEntry {
        name: "Cooties",
        notation: "B2/S23/C8",
        family: "Generations",
        blurb: "",
    },
    CatalogEntry {
        name: "RainZha",
        notation: "B23/S2/C8",
        family: "Generations",
        blurb: "",
    },
    CatalogEntry {
        name: "BelZhab",
        notation: "B23/S23/C8",
        family: "Generations",
        blurb: "Belousov-Zhabotinsky-like spirals.",
    },
    CatalogEntry {
        name: "Xtasy",
        notation: "B2356/S1456/C16",
        family: "Generations",
        blurb: "",
    },
    CatalogEntry {
        name: "Fireworks",
        notation: "B13/S2/C21",
        family: "Generations",
        blurb: "Bursts with long trails.",
    },
    CatalogEntry {
        name: "Bombers",
        notation: "B24/S345/C25",
        family: "Generations",
        blurb: "",
    },
    CatalogEntry {
        name: "Faders",
        notation: "B2/S2/C25",
        family: "Generations",
        blurb: "",
    },
    CatalogEntry {
        name: "Cyclic CA",
        notation: "Cyclic 16/1",
        family: "Multi-state",
        blurb: "16 colours; each advances when a neighbour holds the next one. Grows spirals (edge neighbours).",
    },
    CatalogEntry {
        name: "Greenberg-Hastings",
        notation: "GH 3/1",
        family: "Multi-state",
        blurb: "Excitable medium: resting, excited, refractory.",
    },
    CatalogEntry {
        name: "Rock-paper-scissors",
        notation: "RPS 3/3",
        family: "Multi-state",
        blurb: "A cell is taken by its predator when more than 3 neighbours are that predator.",
    },
    CatalogEntry {
        name: "Forest fire",
        notation: "ForestFire 0.01/0.0001",
        family: "Multi-state",
        blurb: "Drossel-Schwabl: trees grow, lightning strikes, fire spreads (p/f = 100).",
    },
    CatalogEntry {
        name: "Hodgepodge machine",
        notation: "Hodgepodge 200/3/3/28",
        family: "Multi-state",
        blurb: "Gerhardt-Schuster infection model; spiral waves.",
    },
    CatalogEntry {
        name: "Langton's ant",
        notation: "Ant RL",
        family: "Agents",
        blurb: "Turn right on dead, left on live, flip, step.",
    },
    CatalogEntry {
        name: "Turmite LLRR",
        notation: "Turmite LLRR",
        family: "Agents",
        blurb: "Four-colour turmite; grows a symmetric blob.",
    },
    CatalogEntry {
        name: "Rule 30",
        notation: "W30",
        family: "Elementary",
        blurb: "Wolfram's chaotic rule, row by row.",
    },
    CatalogEntry {
        name: "Rule 90",
        notation: "W90",
        family: "Elementary",
        blurb: "Sierpinski triangle from one cell.",
    },
    CatalogEntry {
        name: "Rule 110",
        notation: "W110",
        family: "Elementary",
        blurb: "Turing-complete elementary rule.",
    },
    CatalogEntry {
        name: "Hex rule 6 (XOR)",
        notation: "HexW 6",
        family: "Hexagonal elementary",
        blurb: "Two parents, binary: left XOR right. Pascal's triangle mod 2.",
    },
    CatalogEntry {
        name: "Hex ternary 8229 (sum mod 3)",
        notation: "HexW3 8229",
        family: "Hexagonal elementary",
        blurb: "Two parents, three states: Pascal's triangle mod 3.",
    },
    CatalogEntry {
        name: "Hex3 rule 90 (NW xor NE)",
        notation: "HexT 90",
        family: "Hexagonal elementary",
        blurb: "Three parents on flat-top hexagons; NW xor NE gives Pascal's triangle mod 2.",
    },
    CatalogEntry {
        name: "Hex3 ternary sum 588 (sum mod 3)",
        notation: "HexT3 sum 588",
        family: "Hexagonal elementary",
        blurb: "Three parents, three states: NW + N + NE mod 3. Nested diamonds.",
    },
    CatalogEntry {
        name: "Hex3 ternary sum 314",
        notation: "HexT3 sum 314",
        family: "Hexagonal elementary",
        blurb: "A totalistic three-parent rule that turns chaotic from one cell.",
    },
    CatalogEntry {
        name: "Billiard ball machine",
        notation: "Margolus 0,8,4,3,2,5,9,7,1,6,10,11,12,13,14,15",
        family: "Margolus block",
        blurb: "Margolus 1984: balls fly diagonally and bounce elastically; reversible, conserves balls.",
    },
    CatalogEntry {
        name: "Critters",
        notation: "Margolus 15,14,13,3,11,5,6,1,7,9,10,2,12,4,8,0",
        family: "Margolus block",
        blurb: "Toffoli & Margolus: reversible block rule full of gliders.",
    },
    CatalogEntry {
        name: "Tron",
        notation: "Margolus 15,1,2,3,4,5,6,7,8,9,10,11,12,13,14,0",
        family: "Margolus block",
        blurb: "Toffoli & Margolus: a block whose four cells agree flips. Reversible.",
    },
    CatalogEntry {
        name: "Single Rotation",
        notation: "Margolus 0,2,8,3,1,5,6,7,4,9,10,11,12,13,14,15",
        family: "Margolus block",
        blurb: "Mishin 2013: a block with one live cell turns 90 degrees clockwise. Reversible, rich in spaceships.",
    },
    CatalogEntry {
        name: "HPP gas (Margolus)",
        notation: "Margolus 0,8,4,12,2,10,9,14,1,6,5,13,3,11,7,15",
        family: "Margolus block",
        blurb: "Lattice gas: particles cross their block diagonally; head-on pairs scatter.",
    },
    CatalogEntry {
        name: "Orbium (Lenia)",
        notation: "Lenia R=13,T=10,b=1,m=0.15,s=0.015",
        family: "Continuous",
        blurb: "Bert Chan's Lenia: the classic glider Orbium unicaudatus, a continuous creature.",
    },
    CatalogEntry {
        name: "WireWorld",
        notation: "WireWorld",
        family: "Wire",
        blurb: "Electrons flow along painted wire.",
    },
    CatalogEntry {
        name: "Abelian sandpile",
        notation: "Sandpile",
        family: "Sandpile",
        blurb: "Cells topple when they hold as many grains as neighbours.",
    },
    CatalogEntry {
        name: "Spectre glider (Edmond 2026)",
        notation: "SpectreGlider",
        family: "Table",
        blurb: "Four-state rule with a glider on the Spectre (arXiv:2609.22579).",
    },
    CatalogEntry {
        name: "von Neumann 29-state",
        notation: "JvN29",
        family: "Construction",
        blurb: "Von Neumann's self-reproduction automaton: signal lines, confluent gates, and construction arms (square grid).",
    },
    CatalogEntry {
        name: "JACA Life",
        notation: "JACA B3/S23",
        family: "JACA",
        blurb: "Jimmie's Annihilating CA: Life for +1 and -1 cells; opposite signs that touch annihilate.",
    },
    CatalogEntry {
        name: "JACA Life, blast 2",
        notation: "JACA B3/S23 blast 2",
        family: "JACA",
        blurb: "JACA Life where each annihilation also clears every cell within two neighbourhood steps.",
    },
    CatalogEntry {
        name: "JACA Life vs HighLife",
        notation: "JACA +B3/S23 -B36/S23",
        family: "JACA",
        blurb: "Life (+1) against HighLife (-1); the two meet and annihilate.",
    },
    CatalogEntry {
        name: "JACA Life vs Day & Night",
        notation: "JACA +B3/S23 -B3678/S34678",
        family: "JACA",
        blurb: "Life (+1) against Day & Night (-1), which grows solid regions.",
    },
    CatalogEntry {
        name: "JACA Seeds vs Life",
        notation: "JACA +B2/S -B3/S23 blast 1",
        family: "JACA",
        blurb: "Exploding Seeds (+1) against Life (-1), with a one-step blast.",
    },
    CatalogEntry {
        name: "Parity (von Neumann table)",
        notation: "Table parity",
        family: "Table",
        blurb: "A rule table: each cell becomes the XOR of its four edge neighbours.",
    },
];

/// Catalog name for a rule with a neighbourhood (B/S rules on edge neighbours carry a V).
pub fn catalog_name(rule: &Rule, nb: Nb) -> Option<&'static str> {
    let mut n = rule.notation();
    if nb == Nb::Edge && matches!(rule, Rule::LifeLike { .. } | Rule::Generations { .. }) {
        n.push('V');
    }
    catalog().iter().find(|e| e.notation == n).map(|e| e.name)
}

/// [`catalog_name`] for a rule running on `topo`. On a hexagonal neighbour graph ([`hexagonal`])
/// a B/S rule's hexagonal name comes first: LifeWiki writes those rules with an `H`, so
/// `B2/S34` there is Hexagonal Life (`B2/S34H`). Touching and edge neighbours are the same tiles
/// there, so the `V` names, which mean the square grid's four edge neighbours, never apply.
pub fn catalog_name_on(
    rule: &Rule,
    nb: Nb,
    topo: &crate::topology::Topology,
) -> Option<&'static str> {
    if matches!(rule, Rule::LifeLike { .. } | Rule::Generations { .. }) && hexagonal(topo) {
        let h = format!("{}H", rule.notation());
        return catalog()
            .iter()
            .find(|e| e.notation == h)
            .map(|e| e.name)
            .or_else(|| catalog_name(rule, Nb::Touch));
    }
    catalog_name(rule, nb)
}

/// Whether `topo` has the hexagon grid's neighbour graph: every tile has six neighbours, and all
/// six share an edge with it (so three tiles meet at every corner). Tilings by hexagon-like tiles,
/// such as Escher's lizards, have it too, and a B/S rule runs on them exactly as on hexagons.
pub fn hexagonal(topo: &crate::topology::Topology) -> bool {
    topo.lattice().is_some_and(|g| {
        g.offsets
            .iter()
            .all(|per_type| per_type.iter().all(|slots| slots.len() == 6))
    })
}

#[derive(serde::Deserialize)]
struct NamedFile {
    rules: Vec<NamedRule>,
}

/// A named rule from `data/rules/named.json` (built by `tools/build_named_rules.py` from facts in
/// LifeWiki's rule lists and a few Wikipedia articles; the blurbs are JETCARP's own wording).
#[derive(Clone, Debug, serde::Deserialize)]
pub struct NamedRule {
    pub name: String,
    pub notation: String,
    pub family: String,
    #[serde(default)]
    pub blurb: String,
    /// Who the rule is credited to, when the source names someone.
    #[serde(default)]
    pub author: String,
    #[serde(default)]
    pub source: String,
    #[serde(default)]
    pub note: String,
}

/// Named rules from the data file, in file order.
pub fn named_rules() -> &'static [NamedRule] {
    static N: std::sync::OnceLock<Vec<NamedRule>> = std::sync::OnceLock::new();
    N.get_or_init(|| {
        let f: NamedFile = serde_json::from_str(include_str!("../data/rules/named.json"))
            .expect("embedded named rules");
        f.rules
    })
}

/// Where a named rule was checked, if known.
pub fn source_of(notation: &str) -> Option<&'static str> {
    named_rules()
        .iter()
        .find(|r| r.notation == notation)
        .map(|r| r.source.as_str())
        .filter(|s| !s.is_empty())
}

/// Every named rule: the hand-written [`CATALOG`] first, then the data file (notations already in
/// CATALOG are skipped, so the hand-written names win).
pub fn catalog() -> &'static [CatalogEntry] {
    static ALL: std::sync::OnceLock<Vec<CatalogEntry>> = std::sync::OnceLock::new();
    ALL.get_or_init(|| {
        let mut v: Vec<CatalogEntry> = CATALOG.to_vec();
        for r in named_rules() {
            // Rules JETCARP cannot parse yet (e.g. weighted HROT neighbourhoods) are left out.
            // The H suffix (hexagonal grid) is kept: the same B/S digits on squares are another rule.
            let hex = r.notation.trim().to_ascii_uppercase().ends_with('H');
            let Ok(canon) = Rule::parse(&r.notation).map(|(rule, nb)| {
                let mut n = rule.notation();
                if nb == Some(Nb::Edge) {
                    n.push('V');
                } else if hex {
                    n.push('H');
                }
                n
            }) else {
                continue;
            };
            if v.iter().any(|e| e.notation == canon) {
                continue;
            }
            let leak = |s: &str| -> &'static str { Box::leak(s.to_string().into_boxed_str()) };
            v.push(CatalogEntry {
                name: leak(&r.name),
                notation: leak(&canon),
                family: leak(&r.family),
                blurb: leak(&match r.author.as_str() {
                    "" => r.blurb.clone(),
                    a => format!("{} ({a})", r.blurb),
                }),
            });
        }
        v
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn roundtrip() {
        for s in [
            "B3/S23",
            "B36/S23",
            "B2/S",
            "B3678/S34678",
            "B2/S/C3",
            "W30",
            "Ant RL",
            "Turmite LLRR",
            "WireWorld",
            "Sandpile",
            "SpectreGlider",
            "B3,10/S2,12",
            "Cyclic 14/1",
            "GH 8/1",
            "RPS 3/3",
            "ForestFire 0.01/0.00001",
            "Hodgepodge 100/2/3/28",
            "HexW 6",
            "HexW3 1234",
            "HexT 30",
            "HexT sum 6",
            "HexT3 7625597484986",
            "HexT3 sum 777",
            "HexT3 sym 1234",
            "Margolus 0,8,4,3,2,5,9,7,1,6,10,11,12,13,14,15",
            "Lenia R=13,T=10,b=1,m=0.15,s=0.015",
            "Lenia R=15,T=10,b=1/2,1,m=0.26,s=0.036,kn=2,gn=2",
            "Layer 2/01",
            "Layer 3/0121",
            "R5,C2,S33-57,B34-45",
            "R3,C2,S2,B3,N+",
            "Table codd",
            "JvN29",
        ] {
            let (r, _) = Rule::parse(s).unwrap();
            assert_eq!(r.notation(), s);
        }
        assert_eq!(Rule::parse("b2/s/3").unwrap().0.notation(), "B2/S/C3");
        assert_eq!(Rule::parse("B3/S23V").unwrap().1, Some(Nb::Edge));
        // A V beside the B or S part means the same as the trailing one (#5).
        let trailing = Rule::parse("B47/S13568/4V").unwrap();
        for s in [
            "B47/S13568V/4",
            "B47V/S13568/4",
            "b47/s13568v/c4",
            "B47V/S13568V/G4V",
        ] {
            assert_eq!(Rule::parse(s).unwrap(), trailing, "{s}");
        }
        assert_eq!(Rule::parse("B3/S23V/C4").unwrap().0.notation(), "B3/S23/C4");
        assert_eq!(
            Rule::parse("B3V/S23").unwrap(),
            Rule::parse("B3/S23V").unwrap()
        );
        assert_eq!(Rule::parse("B2/S34H").unwrap().1, None);
        // A V anywhere else says where it goes.
        let e = Rule::parse("B4V7/S13568/4").unwrap_err();
        assert!(e.contains("B47/S13568/C4V"), "{e}");
        let e = Rule::parse("B3/S23/C4V5").unwrap_err();
        assert!(e.contains("B3/S23/C45V"), "{e}");
        let e = Rule::parse("B3/S2V3x").unwrap_err();
        assert!(e.contains("a V goes at the end"), "{e}");
        assert_eq!(Rule::parse("rule 110").unwrap().0, Rule::Wolfram(110));
        assert_eq!(
            Rule::parse("table:spectre-glider").unwrap().0,
            Rule::SpectreGlider
        );
        assert!(Rule::parse("B9/X").is_err());
        // MCell's notation and the built-in names give the published tables.
        let t = |s: &str| match Rule::parse(s).unwrap().0 {
            Rule::Margolus(t) => t,
            r => panic!("{r:?}"),
        };
        assert_eq!(
            t("MS,D0;8;4;3;2;5;9;7;1;6;10;11;12;13;14;15"),
            t("Margolus BBM")
        );
        assert_eq!(
            t("Margolus Critters"),
            [15, 14, 13, 3, 11, 5, 6, 1, 7, 9, 10, 2, 12, 4, 8, 0]
        );
        assert_eq!(
            t("Margolus Tron"),
            [15, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 0]
        );
        assert_eq!(
            t("margolus single rotation"),
            [0, 2, 8, 3, 1, 5, 6, 7, 4, 9, 10, 11, 12, 13, 14, 15]
        );
        assert!(Rule::parse("Margolus 1,2,3").is_err());
        // The catalog's Margolus entries are the built-in tables.
        for (name, key) in [
            ("Billiard ball machine", "bbm"),
            ("Critters", "critters"),
            ("Tron", "tron"),
            ("Single Rotation", "singlerotation"),
            ("HPP gas (Margolus)", "hpp"),
        ] {
            let e = catalog().iter().find(|e| e.name == name).unwrap();
            assert_eq!(
                Rule::parse(e.notation).unwrap().0,
                Rule::Margolus(margolus_named(key).unwrap()),
                "{name}"
            );
        }
    }
}
