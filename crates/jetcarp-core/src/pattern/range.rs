//! Rule-free pattern identity, content ids, and rule ranges.
//!
//! **Identity.** A pattern's id names what it *is*, not the rule it was found in: the tiling
//! identity, the neighbourhood, steps per generation, the cell-state model (`lifelike` for B/S
//! and Generations rules, else the rule family: `wireworld`, `table`, `priority`, ...), the
//! class, period and displacement, the canonical form, and the canonical **evolution**: every
//! phase of one period, in order, in the canonical orientation ([`super::canon`], computed from
//! the record's cells however they are stored). So the Life glider found in B3/S23 and in
//! B36/S23 is one pattern (its cells evolve the same way in both), while a line of three cells
//! that blinks in B3/S23 and stays still in B4/S12 is two. Puffers and movers store only their
//! starting configuration (the rest of their evolution is not recorded), and a gun's phases leave
//! out the ships once they have left, so their identity also includes the exact rule.
//!
//! **The identity text** ([`identity`]; `jetcarp-pattern-v3`) is these lines, joined by `\n`
//! with no trailing newline, in this order:
//!
//! ```text
//! jetcarp-pattern-v3
//! model=<cell-state model: lifelike, wireworld, table, priority, spectre_glider, ...>
//! nb=<touch | edge>
//! steps=<steps per generation>
//! tiling=<identity key: compact JSON of {family, params, edges, boundary: "dead"}, keys sorted;
//!         an aperiodic family's "level" parameter left out ([`super::identity_key`])>
//! class=<still_life | oscillator | spaceship | puffer | mover | gun | other>
//! period=<period>
//! d=<di>,<dj>          (lattice displacement per period, canonical orientation; 0,0 if none)
//! rule=<exact rule notation for puffers, movers, guns and other; else empty>
//! canonical=<canonical text>
//! phases=<phase 0>;<phase 1>;...   (each phase its cells "i,j,k,s" separated by spaces,
//!                                   sorted by row j, then column i, tile type k, state s)
//! ```
//!
//! **The id** ([`content_id`]) is the first 32 hex digits (128 bits) of
//! SHA-256(identity text + `\n` + geometry pin), the pin being the record's tiling's
//! ([`record_pin`]; [`crate::pin`]). So an id names an evolution *on its geometry*: when the
//! engine's geometry changes, the pins change, records are flagged for re-verification, and a
//! record verified again gets a new id (the library keeps the old one as an alias). The first 8
//! hex digits are the short id people use. Packs carry these ids, so this text must never
//! change meaning: a change is a new version prefix.
//!
//! **Rule ranges.** For an outer-totalistic rule the next state of a cell depends only on its own
//! state and its number of live (state 1) neighbours. Over one verified period, every live cell
//! and every empty cell next to a live cell gives a transition (state, live neighbours) -> next
//! state, and every farther cell gives (0, 0) -> 0. Those transitions are all the rule is ever
//! asked, so:
//!
//! * a birth (0, n) -> 1 requires n in B; (0, n) -> 0 forbids it (B0 is always forbidden);
//! * a survival (1, n) -> 1 requires n in S; (1, n) -> 0 or 2 forbids it;
//! * Generations decay: (s, n) -> s + 1 needs more than s + 1 states, (s, n) -> 0 for s >= 2 needs
//!   exactly s + 1 states, and (1, n) -> 0 needs exactly 2 (plain B/S).
//!
//! The required counts make the minimal rule, everything not forbidden the maximal rule, and
//! every rule with `B_min ⊆ B ⊆ B_max`, `S_min ⊆ S ⊆ S_max` and a state count in range runs the
//! pattern through exactly the same phases. A periodic pattern with any death fixes the state
//! count (its dying cells pass through every decay state before vanishing); one without deaths
//! (a still life) works for every state count. Counts run to the tiling's largest degree for the
//! neighbourhood (tile types on multi-type tilings may have fewer neighbours; a count they never
//! reach is simply free). Ranges are computed for still lifes, oscillators, and spaceships of
//! B/S and Generations rules with one step per generation, on lattices and on aperiodic patches
//! (at the pattern's site). Other families keep only the exact rules they were verified in.

use super::{PatternClass, PatternRecord, tiling_key};
use crate::rules::Rule;
use crate::topology::{Boundary, Nb, SizeSpec, TilingSpec, Topology};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::hash::Hash;
use std::sync::{Arc, Mutex, OnceLock};

/// First line of the identity text (bump it if the identity ever changes meaning).
pub const IDENTITY_VERSION: &str = "jetcarp-pattern-v3";

/// Most Generations states JETCARP runs.
pub const MAX_STATES: u8 = 255;

/// The cell-state model part of a pattern's identity: `lifelike` for B/S and Generations rules
/// (B/S is Generations with 2 states), else the rule family.
pub fn cell_model(rule: &Rule) -> &'static str {
    match rule {
        Rule::LifeLike { .. } | Rule::Generations { .. } => "lifelike",
        r => rule_family(r),
    }
}

/// A rule's family, as stored in the catalog's `rules` table.
pub fn rule_family(rule: &Rule) -> &'static str {
    match rule {
        Rule::LifeLike { .. } => "lifelike",
        Rule::Generations { .. } => "generations",
        Rule::WireWorld => "wireworld",
        Rule::Table(_) => "table",
        Rule::Priority(_) => "priority",
        Rule::SpectreGlider => "spectre_glider",
        Rule::Ltl(_) => "ltl",
        Rule::Margolus(_) => "margolus",
        Rule::Lenia(_) => "lenia",
        _ => "other",
    }
}

/// The identity text of a record (see the module notes): its canonical form, computed from its
/// cells however they are stored, with the rest of what makes it this pattern. The rule is read
/// only for its cell-state model (and in full for puffers, movers, guns, and `other`).
pub fn identity(rec: &PatternRecord) -> Result<String, String> {
    let rule = Rule::parse(&rec.rule)?.0;
    let model = cell_model(&rule);
    let exact = match rec.class {
        PatternClass::Puffer | PatternClass::Mover | PatternClass::Gun | PatternClass::Other => {
            rule.notation()
        }
        _ => String::new(),
    };
    let form = super::canon::canonical_form(rec)?;
    let d = form.displacement;
    let nb = match rec.neighborhood {
        Nb::Touch => "touch",
        Nb::Edge => "edge",
    };
    let phases = form
        .phases
        .iter()
        .map(|p| super::encode_cells(p))
        .collect::<Vec<_>>()
        .join(";");
    Ok(format!(
        "{IDENTITY_VERSION}\nmodel={model}\nnb={nb}\nsteps={}\ntiling={}\nclass={}\nperiod={}\nd={},{}\nrule={exact}\ncanonical={}\nphases={phases}",
        rec.steps_per_gen,
        super::identity_key(&rec.tiling),
        rec.class.key(),
        form.period,
        d[0],
        d[1],
        form.canonical,
    ))
}

/// The content id of an identity text on a geometry pin: the first 32 hex digits of
/// SHA-256(identity + `\n` + pin).
pub fn content_id(identity: &str, pin: &str) -> String {
    let mut h = crate::sha256::Sha256::new();
    h.text(identity);
    h.text("\n");
    h.text(pin);
    crate::sha256::hex(&h.finish()[..16])
}

/// The geometry pin of the tiling a record runs on: its tiling's unit cell, or on aperiodic
/// tilings its family's reference patch (the same whatever level it was found on).
pub fn record_pin(rec: &PatternRecord) -> Result<crate::pin::GeometryPin, String> {
    match &rec.site {
        Some(s) => crate::pin::geometry_pin(&s.patch),
        None => crate::pin::geometry_pin(&rec.tiling),
    }
}

/// A record's id: [`content_id`] of its [`identity`] on its [`record_pin`] (so it is the same
/// however the record's phases are stored).
pub fn pattern_id(rec: &PatternRecord) -> Result<String, String> {
    Ok(content_id(&identity(rec)?, &record_pin(rec)?.pin))
}

/// Is `s` a full content id (32 lower-case hex digits)?
pub fn is_content_id(s: &str) -> bool {
    s.len() == 32
        && s.bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

// ---------------------------------------------------------------------------------------------
// Rule ranges

/// The B/S (and Generations) rules in which a pattern runs exactly as stored. Masks have bit `n`
/// for `n` live neighbours, like [`Rule::LifeLike`]; `bits` is the tiling's largest degree + 1.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuleRange {
    pub bits: u32,
    pub birth_min: u32,
    pub birth_max: u32,
    pub survive_min: u32,
    pub survive_max: u32,
    /// Generations state counts (2 = plain B/S). A still life allows 2 to 255.
    pub states_min: u8,
    pub states_max: u8,
    /// The minimal and maximal rules as notation (B/S; `/C<n>` when the state count is fixed
    /// above 2).
    pub min_rule: String,
    pub max_rule: String,
}

fn notation(b: u32, s: u32, c: u8) -> String {
    if c <= 2 {
        Rule::LifeLike {
            birth: b,
            survive: s,
        }
        .notation()
    } else {
        Rule::Generations {
            birth: b,
            survive: s,
            states: c,
        }
        .notation()
    }
}

/// B/S masks and state count of a B/S or Generations rule.
pub fn bs_parts(rule: &Rule) -> Option<(u32, u32, u8)> {
    match rule {
        Rule::LifeLike { birth, survive } => Some((*birth, *survive, 2)),
        Rule::Generations {
            birth,
            survive,
            states,
        } => Some((*birth, *survive, *states)),
        _ => None,
    }
}

impl RuleRange {
    /// A range from its masks and state counts (the notation is filled in).
    pub fn new(bits: u32, b: [u32; 2], s: [u32; 2], c: [u8; 2]) -> RuleRange {
        let fixed = if c[0] == c[1] { c[0] } else { 2 };
        RuleRange {
            bits,
            birth_min: b[0],
            birth_max: b[1],
            survive_min: s[0],
            survive_max: s[1],
            states_min: c[0],
            states_max: c[1],
            min_rule: notation(b[0], s[0], fixed),
            max_rule: notation(b[1], s[1], fixed),
        }
    }

    fn mask(&self) -> u32 {
        if self.bits >= 32 {
            u32::MAX
        } else {
            (1u32 << self.bits) - 1
        }
    }

    /// Does this rule run the pattern exactly as stored? (Counts above the tiling's largest
    /// degree never occur, so they are ignored.)
    pub fn contains(&self, rule: &Rule) -> bool {
        let Some((b, s, c)) = bs_parts(rule) else {
            return false;
        };
        let m = self.mask();
        let (b, s) = (b & m, s & m);
        b & self.birth_min == self.birth_min
            && b & !self.birth_max == 0
            && s & self.survive_min == self.survive_min
            && s & !self.survive_max == 0
            && (self.states_min..=self.states_max).contains(&c)
    }

    /// [`contains`](Self::contains) for rule notation.
    pub fn contains_notation(&self, rule: &str) -> bool {
        Rule::parse(rule).is_ok_and(|(r, _)| self.contains(&r))
    }

    /// Free birth and survival counts (in neither the minimal rule nor forbidden).
    pub fn free_counts(&self) -> (Vec<u32>, Vec<u32>) {
        let free = |lo: u32, hi: u32| -> Vec<u32> {
            (0..self.bits.min(32))
                .filter(|n| (hi & !lo) >> n & 1 == 1)
                .collect()
        };
        (
            free(self.birth_min, self.birth_max),
            free(self.survive_min, self.survive_max),
        )
    }

    /// How many rules the range holds (B/S choices times state counts).
    pub fn rule_count(&self) -> f64 {
        let (b, s) = self.free_counts();
        2f64.powi((b.len() + s.len()) as i32)
            * (self.states_max as f64 - self.states_min as f64 + 1.0)
    }

    /// A rule in the range picked by the bits of `x` (each free count is in when its bit is set;
    /// the state count from the high bits). Useful for sampling.
    pub fn rule_at(&self, x: u64) -> Rule {
        let (fb, fs) = self.free_counts();
        let mut b = self.birth_min;
        let mut s = self.survive_min;
        let mut k = 0;
        for n in fb {
            if x >> (k % 48) & 1 == 1 {
                b |= 1 << n;
            }
            k += 1;
        }
        for n in fs {
            if x >> (k % 48) & 1 == 1 {
                s |= 1 << n;
            }
            k += 1;
        }
        let span = (self.states_max - self.states_min) as u64 + 1;
        let c = self.states_min + ((x >> 48) % span) as u8;
        Rule::parse(&notation(b, s, c)).unwrap().0
    }

    /// `B3/S23 to B3678/S0235678 (2,048 rules)`, plus the state counts when more than one.
    pub fn describe(&self) -> String {
        let mut s = if self.min_rule == self.max_rule {
            format!("only {}", self.min_rule)
        } else {
            format!(
                "{} to {} ({} B/S rules)",
                self.min_rule,
                self.max_rule,
                group((self.rule_count() / (self.states_max - self.states_min + 1) as f64) as u64)
            )
        };
        if self.states_min != self.states_max {
            s.push_str(&format!(
                ", with {} to {} states (Generations)",
                self.states_min, self.states_max
            ));
        }
        s
    }
}

fn group(n: u64) -> String {
    let s = n.to_string();
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// Required and forbidden counts seen so far.
#[derive(Default, Debug)]
struct Tally {
    b_req: u64,
    b_forb: u64,
    s_req: u64,
    s_forb: u64,
    /// The state count is exactly this.
    c_exact: Option<u32>,
    /// The state count is at least this.
    c_min: u32,
}

impl Tally {
    fn add(&mut self, s: u8, n: u32, next: u8) -> Result<(), String> {
        let bit = 1u64 << n.min(63);
        let exact = |t: &mut Tally, c: u32| -> Result<(), String> {
            match t.c_exact {
                Some(x) if x != c => Err(format!(
                    "the evolution needs both {x} and {c} states: not a Generations rule"
                )),
                _ => {
                    t.c_exact = Some(c);
                    Ok(())
                }
            }
        };
        match (s, next) {
            (0, 0) => self.b_forb |= bit,
            (0, 1) => self.b_req |= bit,
            (1, 1) => self.s_req |= bit,
            (1, 0) => {
                self.s_forb |= bit;
                exact(self, 2)?;
            }
            (1, 2) => {
                self.s_forb |= bit;
                self.c_min = self.c_min.max(3);
            }
            (s, t) if s >= 2 && t as u32 == s as u32 + 1 => {
                self.c_min = self.c_min.max(s as u32 + 2)
            }
            (s, 0) if s >= 2 => exact(self, s as u32 + 1)?,
            _ => {
                return Err(format!(
                    "state {s} with {n} live neighbours became {next}: not a B/S or Generations evolution"
                ));
            }
        }
        Ok(())
    }

    fn finish(self, bits: u32) -> Result<RuleRange, String> {
        if bits > 32 {
            return Err(format!(
                "{} neighbours is more than B/S notation holds",
                bits - 1
            ));
        }
        let all = if bits >= 32 {
            u32::MAX as u64
        } else {
            (1u64 << bits) - 1
        };
        let b_forb = self.b_forb | 1; // B0 is never allowed: empty space must stay empty.
        if self.b_req & b_forb != 0 || self.s_req & self.s_forb != 0 {
            return Err(
                "the evolution needs and forbids the same count: not an outer-totalistic rule"
                    .into(),
            );
        }
        let c = match self.c_exact {
            Some(c) if c < self.c_min.max(2) => {
                return Err(format!(
                    "the evolution needs {c} states and at least {}",
                    self.c_min
                ));
            }
            Some(c) if c > MAX_STATES as u32 => return Err(format!("{c} states is too many")),
            Some(c) => [c as u8, c as u8],
            None => [self.c_min.max(2) as u8, MAX_STATES],
        };
        Ok(RuleRange::new(
            bits,
            [(self.b_req & all) as u32, (all & !b_forb) as u32],
            [(self.s_req & all) as u32, (all & !self.s_forb) as u32],
            c,
        ))
    }
}

/// Tally every transition of a run: `phases` is one period plus the next phase (p + 1 entries,
/// the last one the first phase moved by the displacement).
fn tally_run<K: Copy + Eq + Hash>(
    phases: &[Vec<(K, u8)>],
    mut nbrs: impl FnMut(K, &mut Vec<K>),
) -> Result<Tally, String> {
    let mut t = Tally::default();
    let maps: Vec<HashMap<K, u8>> = phases.iter().map(|p| p.iter().copied().collect()).collect();
    let mut buf = Vec::new();
    for w in maps.windows(2) {
        let (cur, next) = (&w[0], &w[1]);
        let mut cand: HashSet<K> = HashSet::new();
        for &x in cur.keys() {
            cand.insert(x);
            buf.clear();
            nbrs(x, &mut buf);
            cand.extend(buf.iter().copied());
        }
        // A cell born far from everything would be a B0 birth; tally it so it is refused.
        cand.extend(next.keys().copied());
        for x in cand {
            buf.clear();
            nbrs(x, &mut buf);
            let n = buf.iter().filter(|y| cur.get(y) == Some(&1)).count() as u32;
            t.add(
                cur.get(&x).copied().unwrap_or(0),
                n,
                next.get(&x).copied().unwrap_or(0),
            )?;
        }
    }
    Ok(t)
}

type Offsets = Arc<Vec<Vec<(i32, i32, u32)>>>;
/// A lattice cell `(i, j, tile type)`.
type LKey = (i32, i32, u32);

/// Neighbour offsets `(di, dj, tile type)` per tile type of a lattice tiling (cached).
fn lattice_offsets(tiling: &TilingSpec, nb: Nb) -> Result<Offsets, String> {
    static CACHE: OnceLock<Mutex<HashMap<String, Offsets>>> = OnceLock::new();
    let key = format!("{}|{nb:?}", tiling_key(tiling));
    let cache = CACHE.get_or_init(Default::default);
    if let Some(o) = cache.lock().unwrap().get(&key) {
        return Ok(o.clone());
    }
    let mut s = super::tiling_identity(tiling);
    s.size = Some(SizeSpec::Rect { w: 8, h: 8 });
    s.boundary = Boundary::Dead;
    let topo = Topology::build(&s)?;
    let g = topo
        .lattice()
        .ok_or_else(|| format!("{} is not a lattice tiling", tiling.family))?;
    let o: Offsets = Arc::new(
        g.offsets[nb as usize]
            .iter()
            .map(|v| v.iter().map(|o| (o.di, o.dj, o.k)).collect())
            .collect(),
    );
    cache.lock().unwrap().insert(key, o.clone());
    Ok(o)
}

/// Largest neighbour count on an aperiodic patch (interior tiles; cached).
fn patch_degree(topo: &Topology, nb: Nb) -> u32 {
    static CACHE: OnceLock<Mutex<HashMap<String, u32>>> = OnceLock::new();
    let key = format!(
        "{}|{nb:?}",
        serde_json::to_string(&topo.spec).unwrap_or_default()
    );
    let cache = CACHE.get_or_init(Default::default);
    if let Some(&d) = cache.lock().unwrap().get(&key) {
        return d;
    }
    let (e, t) = topo.degree_sets();
    let d = match nb {
        Nb::Edge => e,
        Nb::Touch => t,
    }
    .into_iter()
    .max()
    .unwrap_or(0) as u32;
    cache.lock().unwrap().insert(key, d);
    d
}

/// Does this record get a rule range? B/S and Generations still lifes, oscillators, and
/// spaceships with one step per generation.
pub fn ranged(rec: &PatternRecord) -> bool {
    matches!(
        rec.class,
        PatternClass::StillLife | PatternClass::Oscillator | PatternClass::Spaceship
    ) && rec.steps_per_gen == 1
        && rec.mover.is_none()
        && Rule::parse(&rec.rule).is_ok_and(|(r, _)| cell_model(&r) == "lifelike")
}

/// The record's rule range, or `None` when it has none (see [`ranged`]).
pub fn rule_range(rec: &PatternRecord) -> Result<Option<RuleRange>, String> {
    if !ranged(rec) {
        return Ok(None);
    }
    let p = rec.period.max(1);
    let phases: Vec<crate::objects::Phase> =
        (0..=p).map(|t| rec.phase(t)).collect::<Result<_, _>>()?;
    let tally = match &rec.site {
        None => {
            let off = lattice_offsets(&rec.tiling, rec.neighborhood)?;
            let bits = off.iter().map(|v| v.len()).max().unwrap_or(0) as u32 + 1;
            let ph: Vec<Vec<(LKey, u8)>> = phases
                .iter()
                .map(|v| v.iter().map(|c| ((c.i, c.j, c.k), c.s)).collect())
                .collect();
            let t = tally_run(&ph, |(i, j, k), out| {
                if let Some(v) = off.get(k as usize) {
                    out.extend(v.iter().map(|&(di, dj, kk)| (i + di, j + dj, kk)));
                }
            })?;
            (t, bits)
        }
        Some(site) => {
            let topo = super::site::patch_topology(&site.patch)?;
            let map = super::site::match_site(&topo, site, site.anchor as usize)?;
            let ph: Vec<Vec<(usize, u8)>> = phases
                .iter()
                .map(|v| {
                    v.iter()
                        .map(|c| {
                            map.get(c.i as usize)
                                .map(|&x| (x as usize, c.s))
                                .ok_or_else(|| "cell outside the site".to_string())
                        })
                        .collect::<Result<_, _>>()
                })
                .collect::<Result<_, _>>()?;
            let nb = rec.neighborhood;
            let t = tally_run(&ph, |x, out| topo.for_each_neighbor(x, nb, |y| out.push(y)))?;
            (t, patch_degree(&topo, nb) + 1)
        }
    };
    if tally.1 > 32 {
        // More neighbours than B/S notation can name: no range, only the exact rules.
        return Ok(None);
    }
    tally.0.finish(tally.1).map(Some)
}
