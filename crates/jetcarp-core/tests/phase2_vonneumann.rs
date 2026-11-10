//! Von Neumann's 29-state automaton (`JvN29`). The expected behaviour comes from von Neumann and
//! Burks, *Theory of Self-Reproducing Automata* (1966), Sec. 2.8: the transition rule (2.8.1-2.8.2),
//! the construction trace and storage loop (Fig. 12), the kill examples (Fig. 13), and the
//! construct-and-withdraw sequences of Fig. 14 (cell layout reconstructed from the text).

use jetcarp_core::rules::{FieldKind, Rule};
use jetcarp_core::vonneumann::{self as jvn, Dir, U, c, sensitized, t};
use jetcarp_core::world::*;
use jetcarp_core::*;

struct Jvn {
    w: World,
    gw: u32,
}

impl Jvn {
    fn new(gw: u32, gh: u32) -> Jvn {
        // The square grid is at least 8 by 8.
        let (gw, gh) = (gw.max(8), gh.max(8));
        let mut w = World::new(TilingSpec::rect("square", gw, gh, Boundary::Dead), 1).unwrap();
        w.apply(Command::AddCa {
            ca: CaSpec::new("JvN29"),
        })
        .unwrap();
        Jvn { w, gw }
    }
    fn idx(&self, x: i32, y: i32) -> u32 {
        self.w.topo.rows().unwrap().index(x, y, 0) as u32
    }
    fn put(&mut self, x: i32, y: i32, s: u8) {
        let cell = self.idx(x, y);
        let id = self.w.cas[0].id;
        self.w
            .apply(Command::SetCells {
                id,
                cells: vec![[cell, s as u32]],
            })
            .unwrap();
    }
    /// Place a picture with its top-left token at (x0, y0).
    fn pic(&mut self, x0: i32, y0: i32, rows: &[&str]) {
        for (x, y, s) in jvn::picture(rows).unwrap() {
            self.put(x0 + x, y0 + y, s);
        }
    }
    fn at(&self, x: i32, y: i32) -> u8 {
        self.w.fields[self.w.cas[0].field].state[self.idx(x, y) as usize]
    }
    fn tok(&self, x: i32, y: i32) -> String {
        jvn::token(self.at(x, y))
    }
    fn step(&mut self) {
        self.w.apply(Command::Step { gens: 1 }).unwrap();
    }
    fn steps(&mut self, n: u64) {
        self.w.apply(Command::Step { gens: n }).unwrap();
    }
    /// An east-pointing ordinary line ending just left of (x, y) that delivers `bits` into (x, y),
    /// one bit per tick from time 0: the cell at distance d holds bit d-1.
    fn tape_east(&mut self, x: i32, y: i32, bits: &str) {
        for (k, b) in bits_of(bits).into_iter().enumerate() {
            self.put(x - 1 - k as i32, y, t(false, Dir::E, b));
        }
    }
    /// A north-pointing special line below (x, y) delivering `bits` into it from time 0.
    fn tape_north_special(&mut self, x: i32, y: i32, bits: &str) {
        for (k, b) in bits_of(bits).into_iter().enumerate() {
            self.put(x, y + 1 + k as i32, t(true, Dir::N, b));
        }
    }
}

fn bits_of(s: &str) -> Vec<bool> {
    s.chars()
        .filter(|c| *c == '0' || *c == '1')
        .map(|c| c == '1')
        .collect()
}

fn st(tok: &str) -> u8 {
    jvn::parse_token(tok).unwrap_or_else(|| panic!("bad token {tok}"))
}

#[test]
fn rule_parses_and_needs_the_square_grid() {
    let (r, _) = Rule::parse("JvN29").unwrap();
    assert_eq!(r, Rule::VonNeumann);
    assert_eq!(Rule::parse("jvn").unwrap().0, Rule::VonNeumann);
    assert_eq!(r.field_kind(), FieldKind::Multi(29));
    assert_eq!(r.display_name(), "von Neumann 29-state");
    let j = Jvn::new(8, 8);
    assert_eq!(j.w.cas[0].nb, Nb::Edge);
    for fam in ["hex", "triangle"] {
        let mut w = World::new(TilingSpec::rect(fam, 10, 10, Boundary::Dead), 1).unwrap();
        let err = w
            .apply(Command::AddCa {
                ca: CaSpec::new("JvN29"),
            })
            .unwrap_err();
        assert!(err.contains("square grid"), "{err}");
    }
    // Switching a world with a JvN29 CA to another tiling is refused.
    let mut j = Jvn::new(8, 8);
    assert!(
        j.w.apply(Command::SetTiling {
            tiling: TilingSpec::rect("hex", 10, 10, Boundary::Dead)
        })
        .is_err()
    );
}

#[test]
fn state_names_and_tokens() {
    let names: std::collections::HashSet<String> = (0..29).map(jvn::state_name).collect();
    assert_eq!(names.len(), 29, "every state has its own name");
    assert_eq!(jvn::state_name(t(false, Dir::E, false)), "Ordinary > east");
    assert_eq!(
        jvn::state_name(t(true, Dir::S, true)),
        "Special v south, excited"
    );
    assert_eq!(jvn::token(c(true, false)), "C10");
    assert_eq!(sensitized("000"), Some(8));
    assert_eq!(sensitized("0000"), None);
}

// Directions are the screen's: east is +x, north is up (smaller y).
#[test]
fn directions_match_the_screen() {
    let j = Jvn::new(9, 9);
    let c0 = j.w.topo.centroid(j.idx(4, 4) as usize);
    for d in Dir::ALL {
        let (dx, dy) = d.offset();
        let c1 = j.w.topo.centroid(j.idx(4 + dx, 4 + dy) as usize);
        let v = [c1[0] - c0[0], c1[1] - c0[1]];
        let want = match d {
            Dir::E => [1.0, 0.0],
            Dir::W => [-1.0, 0.0],
            Dir::N => [0.0, -1.0], // world y grows downward on screen
            Dir::S => [0.0, 1.0],
        };
        assert!(
            (v[0] - want[0]).abs() < 1e-9 && (v[1] - want[1]).abs() < 1e-9,
            "{d:?}"
        );
    }
}

// Sec. 2.8.1, lists (S.2)-(S.3): the nine strings that end the direct process, and what they
// become. Each is sent, after the leading sensitizing 1, into a ground cell.
#[test]
fn sensitized_construction_all_nine_end_states() {
    let book = [
        ("0000", "o>"),
        ("0001", "o^"),
        ("001", "o<"),
        ("010", "ov"),
        ("011", "s>"),
        ("100", "s^"),
        ("101", "s<"),
        ("110", "sv"),
        ("111", "C00"),
    ];
    for (bits, want) in book {
        let mut j = Jvn::new(20, 3);
        let seq = format!("1{bits}");
        j.tape_east(10, 1, &seq);
        assert_eq!(j.at(10, 1), U);
        // After the leading 1 the cell is S; after each further bit it is S_prefix.
        for k in 1..seq.len() {
            j.step();
            assert_eq!(
                j.at(10, 1),
                sensitized(&bits[..k - 1]).unwrap(),
                "{seq}: after {k} bits"
            );
        }
        j.step();
        assert_eq!(j.tok(10, 1), want, "1{bits}");
        // The new cell is quiescent and stays so.
        j.steps(4);
        assert_eq!(j.tok(10, 1), want, "1{bits} is stable");
    }
}

// The worked example in Sec. 2.8.3 (Fig. 12): input 0,1,0,0,0,0,1,0 into a ground cell at
// times 0..7 gives U, U, S, S0, S00, S000, T000, T001.
#[test]
fn book_trace_into_a_ground_cell() {
    let mut j = Jvn::new(20, 3);
    j.tape_east(12, 1, "0 1 0 0 0 0 1 0");
    let want = ["U", "U", "S", "S0", "S00", "S000", "o>", "o>*"];
    for (time, w) in want.iter().enumerate() {
        assert_eq!(j.at(12, 1), st(w), "time {time}");
        j.step();
    }
}

// Fig. 12: a loop of four cells (one confluent, delay 2, and three ordinary, delay 1) stores
// 10000 and feeds an ordinary cell that writes an ever-lengthening channel into ground.
#[test]
fn storage_loop_grows_a_channel() {
    let mut j = Jvn::new(40, 4);
    // A1 B1 C1 D1 ...   A1 = o>, B1 = C10 (the stored 1), C1 = o>, D1 onwards ground.
    // A2 B2             A2 = o^, B2 = o<.
    j.pic(0, 1, &["o> C10 o>", "o^ o<"]);
    let d1 = 3;
    let want = ["U", "U", "S", "S0", "S00", "S000", "o>", "o>*"];
    for (time, w) in want.iter().enumerate() {
        assert_eq!(j.at(d1, 1), st(w), "D1 at time {time}");
        j.step();
    }
    // Cell k past D1 is finished at time 6 + 6k: pulses come every 5 ticks, and each new cell adds
    // one tick of transit.
    let k = 10;
    j.w.apply(Command::Reset).unwrap();
    j.pic(0, 1, &["o> C10 o>", "o^ o<"]);
    j.steps(6 + 6 * k as u64);
    for x in d1..=d1 + k {
        let s = j.tok(x, 1);
        assert!(s == "o>" || s == "o>*", "cell {x}: {s}");
    }
    assert!(!j.tok(d1 + k + 1, 1).starts_with('o'));
}

// Transmission lines: one cell per tick, of either class, around corners; head-on cells of the
// same class do not excite each other.
#[test]
fn transmission_lines_delay_one() {
    for special in [false, true] {
        let mut j = Jvn::new(20, 8);
        // East along row 1 from x=1 to x=8, then south down column 9 to y=5.
        for x in 1..=8 {
            j.put(x, 1, t(special, Dir::E, x == 1));
        }
        for y in 1..=5 {
            j.put(9, y, t(special, Dir::S, false));
        }
        for time in 0..=12 {
            let (x, y) = if time <= 7 {
                (1 + time, 1)
            } else {
                (9, 1 + time - 8)
            };
            if time <= 12 && (time <= 7 || 1 + time - 8 <= 5) {
                assert_eq!(
                    j.at(x, y),
                    t(special, if time <= 7 { Dir::E } else { Dir::S }, true),
                    "special={special} time {time}"
                );
                let excited = (0..20 * 8)
                    .filter(|&i| {
                        let s = j.w.fields[j.w.cas[0].field].state[i];
                        matches!(
                            jvn::decode(s),
                            jvn::Cell::Transmission { excited: true, .. }
                        )
                    })
                    .count();
                assert_eq!(excited, 1, "one pulse");
            }
            j.step();
        }
        // The pulse leaves the line's end into ground, sensitizing it.
        let mut j2 = Jvn::new(10, 3);
        j2.pic(1, 1, &[if special { "s>* s>" } else { "o>* o>" }]);
        j2.steps(2);
        assert_eq!(j2.tok(3, 1), "S");
    }
    // Head-on: an excited cell pointing at a same-class cell that points back does nothing.
    let mut j = Jvn::new(6, 3);
    j.pic(1, 1, &["o>* o<"]);
    j.step();
    assert_eq!(j.tok(2, 1), "o<");
    // Input from a side works.
    let mut j = Jvn::new(6, 4);
    j.pic(1, 1, &["ov* .", "o> o>"]);
    j.step();
    assert_eq!(j.tok(1, 2), "o>*");
}

// Fig. 11b: the confluent cell is an AND of the ordinary cells pointing at it, with delay 2, and
// fans out to every transmission cell (ordinary or special) not pointing at it.
#[test]
fn confluent_and_two_tick_delay_and_fanout() {
    //  .   s^  .        out_up is special (conversion), out_right ordinary
    //  o>  C   o>
    //  .   o^  .
    let setup = |a: bool, b: bool| {
        let mut j = Jvn::new(6, 5);
        j.pic(0, 1, &[". s^ .", ". C00 o>", ". . ."]);
        j.put(0, 2, t(false, Dir::E, a));
        j.put(1, 3, t(false, Dir::N, b));
        j
    };
    let mut j = setup(true, true);
    j.step();
    assert_eq!(j.tok(1, 2), "C01");
    assert_eq!(j.tok(2, 2), "o>", "not yet");
    j.step();
    assert_eq!(j.tok(1, 2), "C10", "excited two ticks after its inputs");
    j.step();
    assert_eq!(j.tok(2, 2), "o>*", "ordinary output at t+3");
    assert_eq!(j.tok(1, 1), "s^*", "special output at t+3");
    assert_eq!(j.tok(1, 2), "C00");
    for (a, b) in [(true, false), (false, true), (false, false)] {
        let mut j = setup(a, b);
        for _ in 0..4 {
            j.step();
            assert!(!j.tok(2, 2).ends_with('*') && !j.tok(1, 1).ends_with('*'));
            assert!(matches!(j.tok(1, 2).as_str(), "C00"), "{a} {b}");
        }
    }
    // Two pulses in a row give C11 (excited now and next).
    let mut j = Jvn::new(6, 4);
    j.pic(0, 1, &["o>* o>* C00 o>"]);
    let states: Vec<String> = (0..4)
        .map(|_| {
            j.step();
            j.tok(2, 1)
        })
        .collect();
    assert_eq!(states, ["C01", "C11", "C10", "C00"]);
    // A quiescent special cell pointing at a confluent is not one of its inputs.
    let mut j = Jvn::new(6, 4);
    j.pic(0, 1, &["o>* C00", ". s^"]);
    j.step();
    assert_eq!(j.tok(1, 1), "C01");
}

// Confluent output reaches only transmission cells, and not those pointing at it.
#[test]
fn confluent_feeds_only_transmission_cells_not_facing_it() {
    let mut j = Jvn::new(7, 5);
    j.pic(0, 1, &[". S . .", "o< C10 C00 .", ". o^ . ."]);
    j.step();
    assert_eq!(
        j.tok(1, 1),
        "S0",
        "confluent output is no stimulus for sensitized cells"
    );
    assert_eq!(j.tok(2, 2), "C00", "nor for another confluent");
    assert_eq!(j.tok(0, 2), "o<*", "o< takes input from the east");
    assert_eq!(
        j.tok(1, 3),
        "o^",
        "o^ points at the confluent: that side is its output"
    );
    let mut j = Jvn::new(5, 3);
    j.pic(0, 1, &["C10 ."]);
    j.step();
    assert_eq!(j.at(1, 1), U, "confluent cells never sensitize ground");
}

// Sec. 2.8.2 and Fig. 13: an excited cell kills a transmission or confluent cell of the other class
// that it points at, from any side; killed cells become U and stay there.
#[test]
fn ordinary_kills_special_and_special_kills_ordinary() {
    let cases = [
        ("o>*", "s>", "U"),
        ("o>*", "s^", "U"),
        ("o>*", "s<", "U"),
        ("s>*", "o>", "U"),
        ("s>*", "ov", "U"),
        ("s>*", "C00", "U"),
        ("s>*", "C11", "U"),
        ("o>*", "C00", "C01"), // ordinary into confluent is an input, not a kill
        ("s>*", "s>", "s>*"),
        ("o>", "s>", "s>"), // quiescent cells kill nothing
        ("s>", "o>", "o>"),
    ];
    for (src, dst, want) in cases {
        let mut j = Jvn::new(5, 3);
        j.pic(1, 1, &[&format!("{src} {dst}")]);
        j.step();
        assert_eq!(j.at(2, 1), st(want), "{src} into {dst}");
        if want == "U" {
            j.steps(3);
            assert_eq!(j.at(2, 1), U, "{src} into {dst} stays dead");
        }
    }
    // Fig. 13b: excited special and ordinary cells pointing at each other kill each other.
    let mut j = Jvn::new(5, 3);
    j.pic(1, 1, &["o>* s<*"]);
    j.step();
    assert_eq!((j.at(1, 1), j.at(2, 1)), (U, U));
    // Sensitized cells are never killed: either class just adds a 1.
    let mut j = Jvn::new(5, 4);
    j.pic(1, 1, &["o>* S", ". s^*"]);
    j.step();
    assert_eq!(j.tok(2, 1), "S1");
}

// Fig. 13c: kill dominates reception but not emission. Ordinary stimuli enter the line A1 B1 C1
// at times 0 and 1, a special stimulus enters B1 from below at time 1: the first ordinary
// stimulus gets through, B1 dies at time 3, and the second ordinary stimulus is lost.
#[test]
fn kill_dominates_reception_not_emission() {
    let mut j = Jvn::new(8, 6);
    // Row 1: two excited feeders, then A1 B1 C1, then an output cell D.
    j.pic(0, 1, &["o>* o>* o> o> o> o>"]);
    // Column under B1 (x = 3): special cells pointing up; the bottom one is excited at time 0,
    // so the special stimulus enters c (the cell under B1's feeder) at time 1.
    j.pic(3, 2, &["s^", "s^", "s^*"]);
    let (a1, b1, c1, d) = (2, 3, 4, 5);
    j.step(); // t = 1
    assert_eq!(j.tok(a1, 1), "o>*");
    j.step(); // t = 2
    assert_eq!(j.tok(b1, 1), "o>*", "first stimulus in B1");
    assert_eq!(j.tok(3, 2), "s^*", "kill arrives under B1");
    j.step(); // t = 3
    assert_eq!(j.at(b1, 1), U, "B1 killed");
    assert_eq!(j.tok(c1, 1), "o>*", "emission at t=2 not suppressed");
    j.step(); // t = 4
    assert_eq!(j.tok(d, 1), "o>*");
    assert_eq!(j.tok(c1, 1), "o>");
    for _ in 0..4 {
        j.step();
        assert_eq!(j.at(b1, 1), U);
        assert_eq!(j.tok(d, 1), "o>", "second stimulus lost");
    }
}

// Fig. 14: build a remote cell and withdraw the constructing path. Stimuli reach cell B2 from the
// left (input i, ordinary) and from below (input j; here a special line, so it needs no
// conversion and no 2-tick lead). The book's sequences are
//   i: 10000, 10000, 1010, 1111   -> path B2-D2 ordinary, D3 = C00
//   j: 1,1011; 1,1011; 1          -> B2, C2 rebuilt as special east, D2 killed
//   i: 1,10000,1                  -> B2 killed and rebuilt ordinary, C2 killed
//   j: 1                          -> B2 killed
// and the whole transformation takes 37 ticks at B2.
fn fig14() -> (Jvn, [i32; 2]) {
    let i_bits = "10000 10000 1010 1111 00000000000 1100001 0";
    let j_bits = "00000 00000 0000 0000 11011110111 0000000 1";
    assert_eq!(bits_of(i_bits).len(), 37);
    assert_eq!(bits_of(j_bits).len(), 37);
    let (bx, by) = (40, 2);
    let mut j = Jvn::new(46, 42);
    j.tape_east(bx, by, i_bits);
    j.tape_north_special(bx, by, j_bits);
    (j, [bx, by])
}

#[test]
fn construct_and_withdraw() {
    let (mut j, [bx, by]) = fig14();
    let path = |j: &Jvn| {
        [
            j.tok(bx, by),
            j.tok(bx + 1, by),
            j.tok(bx + 2, by),
            j.tok(bx + 2, by + 1),
        ]
    };
    j.steps(21);
    // Fig. 14b: the path is built and D3 is C00; B2 is already being rebuilt by input j.
    assert_eq!(path(&j), ["S0", "o>", "ov", "C00"], "t=21");
    j.steps(8);
    assert_eq!(
        path(&j),
        ["s>*", "s>", "ov", "C00"],
        "B2, C2 special at t=29"
    );
    j.steps(2);
    assert_eq!(
        path(&j),
        ["S", "s>", ".", "C00"],
        "D2 killed, B2 being rebuilt at t=31"
    );
    j.steps(6);
    assert_eq!(path(&j), [".", ".", ".", "C00"], "Fig. 14e at t=37");
    // Nothing else is left: only the two (now quiescent) feeding lines and the built cell.
    j.steps(10);
    assert_eq!(path(&j), [".", ".", ".", "C00"]);
    let f = &j.w.fields[j.w.cas[0].field];
    let excited = f
        .state
        .iter()
        .filter(|&&s| {
            matches!(
                jvn::decode(s),
                jvn::Cell::Transmission { excited: true, .. }
            )
        })
        .count();
    assert_eq!(excited, 0);
}

// All-quiescent patterns never change.
#[test]
fn quiescent_patterns_are_still() {
    let mut j = Jvn::new(30, 30);
    let mut rng = jetcarp_core::rng::Rng::new(7);
    let quiet: Vec<u8> = (0..29u8)
        .filter(|&s| {
            matches!(
                jvn::decode(s),
                jvn::Cell::Ground
                    | jvn::Cell::Transmission { excited: false, .. }
                    | jvn::Cell::Confluent {
                        now: false,
                        next: false
                    }
            )
        })
        .collect();
    assert_eq!(quiet.len(), 10);
    for y in 0..30 {
        for x in 0..30 {
            let s = quiet[(rng.next_u64() % quiet.len() as u64) as usize];
            j.put(x, y, s);
        }
    }
    let before = j.w.fields[j.w.cas[0].field].state.clone();
    j.steps(5);
    assert_eq!(j.w.fields[j.w.cas[0].field].state, before);
}

// --------------------------------------------------------------------------------------------
// Demo scenes. The JSON files are generated from the constructions above; set
// JETCARP_WRITE_SCENES=1 to rewrite them.

fn scene_json(name: &str, description: &str, j: &Jvn, gh: u32) -> String {
    let f = &j.w.fields[j.w.cas[0].field];
    let cells: Vec<String> = f
        .state
        .iter()
        .enumerate()
        .filter(|(_, s)| **s != 0)
        .map(|(i, s)| format!("[{i},{s}]"))
        .collect();
    let rows: Vec<String> = cells.chunks(12).map(|c| c.join(",")).collect();
    format!(
        "{{\n  \"name\": {},\n  \"description\": {},\n  \"tiling\": {{\"family\": \"square\", \"size\": {{\"rect\": {{\"w\": {}, \"h\": {}}}}}, \"boundary\": \"dead\"}},\n  \"rng_state\": 1,\n  \"cas\": [{{\"rule\": \"JvN29\", \"seed\": {{\"cells\": {{\"cells\": [\n    {}\n  ]}}}}}}]\n}}\n",
        serde_json::to_string(name).unwrap(),
        serde_json::to_string(description).unwrap(),
        j.gw,
        gh,
        rows.join(",\n    ")
    )
}

fn check_scene(file: &str, json: String) -> World {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../scenes")
        .join(file);
    if std::env::var_os("JETCARP_WRITE_SCENES").is_some() {
        std::fs::write(&path, &json).unwrap();
    }
    let on_disk = std::fs::read_to_string(&path).unwrap_or_default();
    assert_eq!(
        Scene::from_json(&on_disk).ok(),
        Some(Scene::from_json(&json).unwrap()),
        "{file} is out of date: run with JETCARP_WRITE_SCENES=1"
    );
    World::from_scene(&Scene::from_json(&on_disk).unwrap()).unwrap()
}

fn growing_channels() -> Jvn {
    // Fig. 12 storage loops. The loop's sequence is read at the confluent cell's output in the
    // order: confluent now, confluent next, A1, A2, B2 (period 5).
    let mut j = Jvn::new(96, 40);
    // Top: 10000 -> one more ordinary east cell every 5 ticks; the channel grows right.
    j.pic(1, 1, &["o> C10 o>", "o^ o<"]);
    // Middle and right: 10100 -> ordinary south cells; channels grow downward.
    j.pic(1, 8, &["o>* C10 o>", "o^ o<"]);
    j.pic(40, 8, &["o>* C10 o>", "o^ o<"]);
    j
}

#[test]
fn demo_growing_channels() {
    let mut j = growing_channels();
    let json = scene_json(
        "von Neumann: storage loops growing channels",
        "Burks' Fig. 12: a four-cell loop with one confluent cell (delay 2) stores five bits and feeds them into ground forever. 10000 builds an ordinary east cell each pass, so the top channel grows right; 10100 builds ordinary south cells, so the lower two grow downward. Blue: ordinary; bright: excited; green: confluent; amber: sensitized.",
        &j,
        40,
    );
    let mut w = check_scene("vonneumann-growing-channels.json", json);
    let gens = 6 + 6 * 20;
    w.apply(Command::Step { gens }).unwrap();
    j.steps(gens);
    assert_eq!(w.state_hash(), j.w.state_hash());
    for x in 4..=24 {
        assert!(j.tok(x, 1).starts_with("o>"), "top channel at {x}");
    }
    for y in 8..=26 {
        assert!(
            j.tok(4, y).starts_with("ov"),
            "left column at {y}: {}",
            j.tok(4, y)
        );
        assert!(j.tok(43, y).starts_with("ov"), "right column at {y}");
    }
}

#[test]
fn demo_construct_and_withdraw() {
    let (j, [bx, by]) = fig14();
    let json = scene_json(
        "von Neumann: construct and withdraw",
        "Burks' Fig. 14 sequences: an ordinary tape (from the left) and a special tape (from below) feed cell B2. They build an ordinary path of three cells, write a confluent cell at its end, then kill the path back to ground, leaving only the confluent cell. 37 ticks.",
        &j,
        42,
    );
    let mut w = check_scene("vonneumann-construct-withdraw.json", json);
    w.apply(Command::Step { gens: 40 }).unwrap();
    let f = &w.fields[w.cas[0].field];
    let g = w.topo.rows().unwrap();
    assert_eq!(f.state[g.index(bx + 2, by + 1, 0)], c(false, false));
    assert_eq!(f.state[g.index(bx, by, 0)], U);
}

#[test]
fn same_hash_any_thread_count() {
    let (j, _) = fig14();
    let scene = j.w.to_scene();
    let run = |threads: usize| {
        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(threads)
            .build()
            .unwrap();
        pool.install(|| {
            let mut w = World::from_scene(&scene).unwrap();
            w.apply(Command::Step { gens: 30 }).unwrap();
            w.state_hash()
        })
    };
    assert_eq!(run(1), run(4));
}
