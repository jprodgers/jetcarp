//! Computing demos, built programmatically so every cell is reviewable:
//!
//! - [`wireworld_adder`]: a two-bit binary adder in WireWorld (`a + b`, `0 ≤ a, b ≤ 3`).
//! - [`life_gate`]: NOT, AND, and OR gates in Life made from glider streams.
//!
//! **WireWorld.** Signals are electrons (a head followed by a tail). Every component keeps one
//! timing rule: an electron that leaves a source at time `t0` is at column `x` at time
//! `x + const`. Wires run left to right and may step diagonally (a diagonal step also advances
//! one column per generation), and every gate below was chosen so that its output keeps the
//! rule. So any signals that start together arrive together at every gate, without delay lines.
//!
//! Components (found with a small exhaustive and annealing search over junction shapes, then
//! checked here by simulation; see `tests/computing.rs`):
//! - **Source**: a closed loop of 16 cells with one electron (bit 1) or none (bit 0). A tap on
//!   its right side emits an electron every 16 generations.
//! - **Fan-out**: a wire splitting into two diagonal branches.
//! - **OR**: two inputs meeting at a column of three cells; a lone electron's three heads stop
//!   it running back up the other input.
//! - **XOR**: a 17-cell junction where two simultaneous electrons annihilate (works for trains
//!   with 12 or more generations between electrons).
//! - **Half adder** `[x, y] → [x ⊕ y, x ∧ y]` without crossings: fan out `y`, `s = x ⊕ y`, fan
//!   out `s` twice, `c = s ⊕ (s ∨ y)` (which is `y ∧ ¬s = x ∧ y`).
//! - **Adder**: half adders on `(a0, b0)` and `(a1, b1)`, a third on the carry and `a1 ⊕ b1`,
//!   and an OR of the two carries: `s0 = a0 ⊕ b0`, `s1 = a1 ⊕ b1 ⊕ c0`, `s2 = carry`.
//!
//! The layout has no wire crossings: the half adder's fan-outs keep every gate's inputs on
//! neighbouring tracks.

use crate::topology::{Boundary, TilingSpec};
use crate::world::{CaSpec, Scene, SeedSpec};
use std::collections::BTreeMap;

// ---------------------------------------------------------------------------------------------
// WireWorld

const HEAD: u8 = 1;
const TAIL: u8 = 2;
const WIRE: u8 = 3;

/// Generations between electrons from a source loop.
pub const WW_PERIOD: i32 = 16;

/// XOR junction cells relative to the input ports `(0, -2)` and `(0, 2)`; output from `(6, 0)`.
const XOR_CELLS: &[(i32, i32)] = &[
    (1, -3),
    (1, -2),
    (1, -1),
    (1, 2),
    (2, -3),
    (2, -1),
    (2, 1),
    (2, 2),
    (2, 3),
    (3, -2),
    (3, 1),
    (3, 3),
    (4, -1),
    (4, 2),
    (5, -1),
    (5, 0),
    (5, 1),
];
const XOR_OUT: i32 = 6;
/// OR junction cells; output from `(3, 0)`.
const OR_CELLS: &[(i32, i32)] = &[(0, 0), (1, -1), (1, 0), (1, 1), (2, 0)];
const OR_OUT: i32 = 3;
/// Columns per stage block (input port to the end of the output stub).
const BLOCK: i32 = 9;

#[derive(Clone, Copy, Debug, PartialEq)]
enum Op {
    Pass,
    Fan,
    Xor,
    Or,
}

/// Cells of a WireWorld drawing.
#[derive(Default)]
struct Board {
    cells: BTreeMap<(i32, i32), u8>,
}

impl Board {
    fn put(&mut self, x: i32, y: i32, v: u8) {
        self.cells.insert((x, y), v);
    }
    fn wire(&mut self, x: i32, y: i32) {
        self.cells.entry((x, y)).or_insert(WIRE);
    }
}

/// Lay out tracks through stages. `ys` are the current track rows at column `x`.
struct Layout {
    b: Board,
    x: i32,
    ys: Vec<i32>,
}

impl Layout {
    /// Move every track to its target row with diagonal steps, then 3 straight cells.
    fn route(&mut self, to: &[i32]) {
        assert_eq!(to.len(), self.ys.len());
        let dx = self
            .ys
            .iter()
            .zip(to)
            .map(|(a, b)| (a - b).abs())
            .max()
            .unwrap_or(0)
            + 3;
        for (y0, &y1) in self.ys.clone().iter().zip(to) {
            let mut y = *y0;
            for k in 1..=dx {
                y += (y1 - y).signum();
                self.b.wire(self.x + k, y);
            }
        }
        self.x += dx;
        self.ys = to.to_vec();
    }

    /// One stage: `ops` covers the tracks top to bottom (gates take two tracks).
    fn stage(&mut self, ops: &[Op]) {
        // Input rows: keep each block near its current track, at least 5 rows below the last.
        let mut want = vec![];
        let mut bottom = i32::MIN / 2;
        let mut t = 0;
        for op in ops {
            match op {
                Op::Pass => {
                    let y = self.ys[t].max(bottom + 5);
                    want.push(y);
                    bottom = y;
                    t += 1;
                }
                Op::Fan => {
                    let y = self.ys[t].max(bottom + 7);
                    want.push(y);
                    bottom = y + 2;
                    t += 1;
                }
                Op::Xor | Op::Or => {
                    let y = self.ys[t].max(bottom + 6);
                    want.push(y);
                    want.push(y + 4);
                    bottom = y + 5;
                    t += 2;
                }
            }
        }
        assert_eq!(t, self.ys.len(), "stage ops must cover every track");
        self.route(&want);
        let x0 = self.x;
        let mut out = vec![];
        let mut t = 0;
        for op in ops {
            let y = want[t];
            match op {
                Op::Pass => {
                    for k in 1..=BLOCK {
                        self.b.wire(x0 + k, y);
                    }
                    out.push(y);
                    t += 1;
                }
                Op::Fan => {
                    self.b.wire(x0 + 1, y - 1);
                    self.b.wire(x0 + 1, y + 1);
                    for k in 2..=BLOCK {
                        self.b.wire(x0 + k, y - 2);
                        self.b.wire(x0 + k, y + 2);
                    }
                    out.push(y - 2);
                    out.push(y + 2);
                    t += 1;
                }
                Op::Xor | Op::Or => {
                    let yc = y + 2;
                    let (cells, from) = if *op == Op::Xor {
                        (XOR_CELLS, XOR_OUT)
                    } else {
                        (OR_CELLS, OR_OUT)
                    };
                    for &(dx, dy) in cells {
                        self.b.wire(x0 + dx, yc + dy);
                    }
                    for k in from..=BLOCK {
                        self.b.wire(x0 + k, yc);
                    }
                    out.push(yc);
                    t += 2;
                }
            }
        }
        self.x = x0 + BLOCK;
        self.ys = out;
    }
}

/// A 3×5 font for labels drawn as inert copper.
fn glyph(c: char) -> [&'static str; 5] {
    match c {
        'A' => [".#.", "#.#", "###", "#.#", "#.#"],
        'B' => ["##.", "#.#", "##.", "#.#", "##."],
        'S' => [".##", "#..", ".#.", "..#", "##."],
        '0' => ["###", "#.#", "#.#", "#.#", "###"],
        '1' => [".#.", "##.", ".#.", ".#.", "###"],
        '2' => ["##.", "..#", ".#.", "#..", "###"],
        _ => ["...", "...", "...", "...", "..."],
    }
}

fn label(b: &mut Board, text: &str, x: i32, y: i32) {
    for (i, c) in text.chars().enumerate() {
        for (r, row) in glyph(c).iter().enumerate() {
            for (k, ch) in row.chars().enumerate() {
                if ch == '#' {
                    b.put(x + 4 * i as i32 + k as i32, y + r as i32, WIRE);
                }
            }
        }
    }
}

/// The WireWorld adder for one input pair.
#[derive(Clone, Debug)]
pub struct WireAdder {
    pub scene: Scene,
    pub a: u32,
    pub b: u32,
    /// Output probe cells `(x, y)` for s0, s1, s2 (grid coordinates).
    pub outputs: [(i32, i32); 3],
    /// Input source taps `(x, y)` for a0, b0, a1, b1.
    pub inputs: [(i32, i32); 4],
    /// The first generation an electron for a 1 bit is at an output probe; later ones follow
    /// every [`WW_PERIOD`] generations.
    pub first_output: u64,
    pub width: u32,
    pub height: u32,
}

/// A two-bit adder `a + b` (`a, b` in 0..=3) in WireWorld, with the inputs on four source loops.
pub fn wireworld_adder(a: u32, b: u32) -> Result<WireAdder, String> {
    if a > 3 || b > 3 {
        return Err("the adder takes two-bit inputs: a and b from 0 to 3".into());
    }
    let mut board = Board::default();
    // Sources: loops stacked vertically, tracks leaving at column `tap`.
    // Loop cells: top (1..=5, 0), right (6, 1..=3), bottom (5..=1, 4), left (0, 3..=1): 16 cells.
    let bits = [a & 1, b & 1, (a >> 1) & 1, (b >> 1) & 1];
    let names = ["A0", "B0", "A1", "B1"];
    let pitch = 12;
    let lx = 12;
    let mut taps = [(0, 0); 4];
    let mut ys = vec![];
    for (k, &bit) in bits.iter().enumerate() {
        let y0 = k as i32 * pitch;
        let mut loop_cells = vec![];
        for x in 1..=5 {
            loop_cells.push((lx + x, y0));
        }
        for y in 1..=3 {
            loop_cells.push((lx + 6, y0 + y));
        }
        for x in (1..=5).rev() {
            loop_cells.push((lx + x, y0 + 4));
        }
        for y in (1..=3).rev() {
            loop_cells.push((lx, y0 + y));
        }
        debug_assert_eq!(loop_cells.len() as i32, WW_PERIOD);
        for &(x, y) in &loop_cells {
            board.wire(x, y);
        }
        if bit == 1 {
            // Head on the right side just above the tap row, tail behind it (clockwise).
            board.put(lx + 6, y0 + 1, HEAD);
            board.put(lx + 5, y0, TAIL);
        }
        let ty = y0 + 2;
        board.wire(lx + 7, ty);
        taps[k] = (lx + 7, ty);
        ys.push(ty);
        label(&mut board, names[k], 1, y0);
    }
    let mut lay = Layout {
        b: board,
        x: lx + 7,
        ys,
    };
    use Op::*;
    // Phase 1: half adders on (a0, b0) and (a1, b1) → [s0, c0, t, g].
    let ha = [
        vec![Pass, Fan],
        vec![Xor, Pass],
        vec![Fan, Pass],
        vec![Pass, Fan, Pass],
        vec![Pass, Pass, Or],
        vec![Pass, Xor],
    ];
    for st in &ha {
        let mut ops = st.clone();
        ops.extend(st.iter().copied());
        lay.stage(&ops);
    }
    // Phase 2: s0 passes; half adder on (c0, t); g passes → [s0, s1, h, g].
    for st in &ha {
        let mut ops = vec![Pass];
        ops.extend(st.iter().copied());
        ops.push(Pass);
        lay.stage(&ops);
    }
    // Phase 3: s2 = h ∨ g.
    lay.stage(&[Pass, Pass, Or]);
    // Spread the outputs and run them out to the right.
    let top = lay.ys[0];
    lay.route(&[top, top + 8, top + 16]);
    let xe = lay.x + 14;
    for (k, &y) in lay.ys.clone().iter().enumerate() {
        for x in lay.x + 1..=xe {
            lay.b.wire(x, y);
        }
        label(&mut lay.b, &format!("S{k}"), xe + 3, y - 2);
    }
    let outputs = [
        (xe - 3, lay.ys[0]),
        (xe - 3, lay.ys[1]),
        (xe - 3, lay.ys[2]),
    ];
    // Grid: shift so everything has a margin of 2.
    let (mut x0, mut y0, mut x1, mut y1) = (i32::MAX, i32::MAX, i32::MIN, i32::MIN);
    for &(x, y) in lay.b.cells.keys() {
        x0 = x0.min(x);
        y0 = y0.min(y);
        x1 = x1.max(x);
        y1 = y1.max(y);
    }
    let (ox, oy) = (2 - x0, 2 - y0);
    let w = (x1 - x0 + 5) as u32;
    let h = (y1 - y0 + 5) as u32;
    let cells: Vec<[u32; 2]> = lay
        .b
        .cells
        .iter()
        .map(|(&(x, y), &v)| [((y + oy) as u32) * w + (x + ox) as u32, v as u32])
        .collect();
    let mut sc = Scene::new(TilingSpec::rect("square", w, h, Boundary::Dead));
    sc.name = format!("WireWorld adder: {a} + {b}");
    sc.description = format!(
        "A two-bit binary adder computing {a} + {b} = {} ({:03b}). Inputs: the four loops on \
         the left are the bits A0, B0, A1, B1 (top to bottom); a loop with a circling electron \
         is a 1 and sends an electron down its wire every {WW_PERIOD} generations. The circuit \
         is three crossing-free half adders built from fan-outs, XOR and OR junctions, plus an \
         OR for the carry. Outputs: the three wires on the right are S0, S1, S2 (1s, 2s, 4s); \
         after about {} generations, the wires of the 1 bits carry an electron every \
         {WW_PERIOD} generations. Other inputs: `jetcarp demo wireworld-adder --a A --b B`.",
        a + b,
        a + b,
        outputs[0].0 - taps[0].0 + 1
    );
    sc.cas = vec![CaSpec::new("WireWorld").seed(SeedSpec::Cells { cells })];
    let first = (outputs[0].0 - taps[0].0 + 1) as u64;
    Ok(WireAdder {
        scene: sc,
        a,
        b,
        outputs: outputs.map(|(x, y)| (x + ox, y + oy)),
        inputs: taps.map(|(x, y)| (x + ox, y + oy)),
        first_output: first,
        width: w,
        height: h,
    })
}

// ---------------------------------------------------------------------------------------------
// Life logic gates

/// A Life logic gate.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Gate {
    Not,
    And,
    Or,
}

impl Gate {
    pub fn parse(s: &str) -> Result<Gate, String> {
        match s.trim().to_ascii_lowercase().as_str() {
            "not" => Ok(Gate::Not),
            "and" => Ok(Gate::And),
            "or" => Ok(Gate::Or),
            _ => Err(format!("unknown gate '{s}' (not, and, or)")),
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            Gate::Not => "NOT",
            Gate::And => "AND",
            Gate::Or => "OR",
        }
    }
    pub fn eval(self, a: bool, b: bool) -> bool {
        match self {
            Gate::Not => !a,
            Gate::And => a && b,
            Gate::Or => a || b,
        }
    }
}

/// A Life gate for one input combination.
#[derive(Clone, Debug)]
pub struct LifeGate {
    pub scene: Scene,
    pub gate: Gate,
    pub a: bool,
    pub b: bool,
    /// Output detector box `[x0, y0, x1, y1)` on the output lane (grid coordinates).
    pub detector: [i32; 4],
    /// Generations until the output lane is in its steady state.
    pub settle: u64,
    pub width: u32,
    pub height: u32,
}

/// The Gosper glider gun as published on Wikipedia and LifeWiki ("Gosper glider gun"), typed in
/// by hand as RLE. Period 30; its gliders travel down and to the right.
pub const GOSPER_GUN_RLE: &str = "24bo$22bobo$12b2o6b2o12b2o$11bo3bo4b2o12b2o$2o8bo5bo3b2o$\
                                  2o8bo3bob2o4bobo$10bo5bo7bo$11bo3bo$12b2o!";

/// Eater 1 (the fishhook), the standard glider eater.
const EATER: [(i32, i32); 7] = [(0, 0), (1, 0), (0, 1), (2, 1), (2, 2), (2, 3), (3, 3)];

/// One gate frame (cells), found with a placement search (see the module notes on
/// [`life_gate`]); all coordinates are inside a `LIFE_W × LIFE_H` frame.
const LIFE_W: i32 = 212;
const LIFE_H: i32 = 176;
/// Input A's gun: top-left corner, phase (generations already run), mirrored.
const GUN_A: (i32, i32, u32, bool) = (44, 60, 0, false);
const GUN_B: (i32, i32, u32, bool) = (65, 15, 0, false);
/// The gate's own gun G (mirrored, gliders travel down and to the left).
const GUN_G: (i32, i32, u32, bool) = (170, 1, 26, true);
/// The OR gate's second gun.
const GUN_G2: (i32, i32, u32, bool) = (4, 90, 0, false);
/// Eaters: corner and orientation (bit 0: mirror x; bit 2: swap x and y first).
const EAT_A: (i32, i32, u8) = (125, 127, 0);
const EAT_B: (i32, i32, u8) = (150, 86, 0);
const EAT_G: (i32, i32, u8) = (68, 121, 1);
const EAT_G2: (i32, i32, u8) = (90, 162, 0);
/// Output detectors `[x0, y0, x1, y1)`: NOT on G's lane past A, AND on A's lane past G, OR on
/// G2's lane past the NOR stream.
const DET_NOT: [i32; 4] = [76, 104, 94, 118];
const DET_AND: [i32; 4] = [100, 104, 118, 120];
const DET_OR: [i32; 4] = [64, 140, 86, 156];
/// Generations before the outputs are steady (the first gliders of every stream have arrived,
/// and early gliders that slipped through have been eaten).
pub const LIFE_SETTLE: u64 = 1200;

fn rle_cells(rle: &str) -> Vec<(i32, i32)> {
    crate::hashlife::parse_rle(rle)
        .map(|(_, c)| {
            c.into_iter()
                .map(|(x, y, _)| (x as i32, y as i32))
                .collect()
        })
        .unwrap_or_default()
}

/// The gun after `phase` generations (with any glider it has emitted), mirrored in x (`x → 35 -
/// x`) if asked, relative to the gun's corner.
fn gun_cells(phase: u32, mirror: bool) -> Vec<(i32, i32)> {
    let mut sc = Scene::new(TilingSpec::rect("square", 100, 80, Boundary::Dead));
    let cells: Vec<[u32; 2]> = rle_cells(GOSPER_GUN_RLE)
        .iter()
        .map(|&(x, y)| [((y + 20) * 100 + x + 20) as u32, 1])
        .collect();
    sc.cas = vec![CaSpec::new("B3/S23").seed(SeedSpec::Cells { cells })];
    let mut w = crate::world::World::from_scene(&sc).expect("gun scene");
    for _ in 0..phase {
        w.step();
    }
    w.fields[0]
        .state
        .iter()
        .enumerate()
        .filter(|(_, s)| **s != 0)
        .map(|(i, _)| ((i % 100) as i32 - 20, (i / 100) as i32 - 20))
        .map(|(x, y)| if mirror { (35 - x, y) } else { (x, y) })
        .collect()
}

fn eater_cells(o: u8) -> Vec<(i32, i32)> {
    EATER
        .iter()
        .map(|&(x, y)| {
            let (x, y) = if o & 4 != 0 { (y, x) } else { (x, y) };
            let x = if o & 1 != 0 { 3 - x } else { x };
            let y = if o & 2 != 0 { 3 - y } else { y };
            (x, y)
        })
        .collect()
}

/// The live cells of one gate frame for these inputs, and its output detector.
fn gate_frame(gate: Gate, a: bool, b: bool) -> (Vec<(i32, i32)>, [i32; 4]) {
    let mut cells = vec![];
    let mut gun = |g: (i32, i32, u32, bool)| {
        cells.extend(
            gun_cells(g.2, g.3)
                .into_iter()
                .map(|(x, y)| (x + g.0, y + g.1)),
        );
    };
    if a {
        gun(GUN_A);
    }
    if b && gate != Gate::Not {
        gun(GUN_B);
    }
    gun(GUN_G);
    if gate == Gate::Or {
        gun(GUN_G2);
    }
    let mut eat = |e: (i32, i32, u8)| {
        cells.extend(
            eater_cells(e.2)
                .into_iter()
                .map(|(x, y)| (x + e.0, y + e.1)),
        );
    };
    eat(EAT_A);
    match gate {
        Gate::Not => eat(EAT_G),
        Gate::And => {
            eat(EAT_B);
            eat(EAT_G);
        }
        Gate::Or => {
            eat(EAT_B);
            eat(EAT_G2);
        }
    }
    let det = match gate {
        Gate::Not => DET_NOT,
        Gate::And => DET_AND,
        Gate::Or => DET_OR,
    };
    cells.sort_unstable();
    cells.dedup();
    (cells, det)
}

fn gate_text(gate: Gate) -> &'static str {
    match gate {
        Gate::Not => {
            "Gun A (left) fires gliders down-right when input A is 1; gun G (right) fires \
             gliders down-left. Where the streams cross, each A glider and G glider annihilate. \
             The output is G's stream below the crossing (lower left): it flows only when A is \
             0, so output = NOT A. Eaters (small hooks) absorb streams at the lane ends."
        }
        Gate::And => {
            "Guns A and B fire parallel streams down-right when their inputs are 1; gun G (right) \
             fires down-left and meets B's lane first: a B glider and a G glider annihilate, so \
             G continues only when B is 0 (NOT B). That stream then meets A's lane and kills A's \
             gliders. The output is A's stream below the crossing (centre right): it flows only \
             when A is 1 and G was stopped by B, so output = A AND B."
        }
        Gate::Or => {
            "As in the AND gate, gun G's stream survives B's lane only when B is 0 and A's lane \
             only when A is 0, so below A's lane it carries NOR(A, B). A fourth gun G2 (lower \
             left) fires down-right into that stream; G2's gliders survive only when the NOR \
             stream is empty. The output is G2's stream below the crossing (bottom): output = A \
             OR B."
        }
    }
}

/// A Life gate with inputs `a` and `b` (NOT ignores `b`): one frame on a dead-edged grid.
///
/// Built from Gosper glider guns (period 30) whose streams cross and annihilate glider for
/// glider, plus eaters. The gun positions and phases were found by a placement search (an
/// exhaustive scan of positions and phases for a gun whose stream annihilates another stream
/// cleanly: nothing below the crossing, no debris), the eaters by a scan of positions and
/// orientations along each lane; `tests/computing.rs` checks every truth table.
pub fn life_gate(gate: Gate, a: bool, b: bool) -> Result<LifeGate, String> {
    let (cells, det) = gate_frame(gate, a, b);
    let (w, h) = (LIFE_W as u32, LIFE_H as u32);
    let mut sc = Scene::new(TilingSpec::rect("square", w, h, Boundary::Dead));
    let ins = if gate == Gate::Not {
        format!("A = {}", a as u8)
    } else {
        format!("A = {}, B = {}", a as u8, b as u8)
    };
    sc.name = format!("Life {} gate ({ins})", gate.name());
    sc.description = format!(
        "{} With {ins} the output is {} (after about {LIFE_SETTLE} generations).",
        gate_text(gate),
        gate.eval(a, b) as u8
    );
    let seed: Vec<[u32; 2]> = cells
        .iter()
        .map(|&(x, y)| [(y as u32) * w + x as u32, 1])
        .collect();
    sc.cas = vec![CaSpec::new("B3/S23").seed(SeedSpec::Cells { cells: seed })];
    Ok(LifeGate {
        scene: sc,
        gate,
        a,
        b,
        detector: det,
        settle: LIFE_SETTLE,
        width: w,
        height: h,
    })
}

/// Gap between frames in a truth-table scene.
const FRAME_GAP: i32 = 10;

/// One frame of a truth table: its inputs `(a, b)` and output detector box.
pub type Frame = ((bool, bool), [i32; 4]);

/// Every input combination of a gate side by side (NOT: A = 0, 1; AND and OR: AB = 00, 01, 10,
/// 11), with the detector box of each frame.
pub fn life_truth_table(gate: Gate) -> (Scene, Vec<Frame>) {
    let combos: Vec<(bool, bool)> = if gate == Gate::Not {
        vec![(false, false), (true, false)]
    } else {
        vec![(false, false), (false, true), (true, false), (true, true)]
    };
    let n = combos.len() as i32;
    let w = (n * LIFE_W + (n - 1) * FRAME_GAP) as u32;
    let h = LIFE_H as u32;
    let mut seed = vec![];
    let mut dets = vec![];
    for (k, &(a, b)) in combos.iter().enumerate() {
        let ox = k as i32 * (LIFE_W + FRAME_GAP);
        let (cells, d) = gate_frame(gate, a, b);
        seed.extend(
            cells
                .iter()
                .map(|&(x, y)| [(y as u32) * w + (x + ox) as u32, 1]),
        );
        dets.push(((a, b), [d[0] + ox, d[1], d[2] + ox, d[3]]));
    }
    let mut sc = Scene::new(TilingSpec::rect("square", w, h, Boundary::Dead));
    let order = if gate == Gate::Not {
        "A = 0 and A = 1".to_string()
    } else {
        "AB = 00, 01, 10, 11".to_string()
    };
    let outs: Vec<String> = combos
        .iter()
        .map(|&(a, b)| (gate.eval(a, b) as u8).to_string())
        .collect();
    sc.name = format!("Life {} gate (truth table)", gate.name());
    sc.description = format!(
        "{} Left to right: {order}; outputs {} once the streams are steady (about \
         {LIFE_SETTLE} generations). One input combination: `jetcarp demo life-{} --a A --b B`.",
        gate_text(gate),
        outs.join(", "),
        gate.name().to_ascii_lowercase()
    );
    sc.cas = vec![CaSpec::new("B3/S23").seed(SeedSpec::Cells { cells: seed })];
    (sc, dets)
}
