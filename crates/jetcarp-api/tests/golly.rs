//! JETCARP's RLE checked against Golly (#145): Golly reads what JETCARP writes as the same cells,
//! JETCARP reads what Golly writes back as the same cells, and the two run a pattern to the same
//! generations, on the square grid (Moore and von Neumann) and the hexagon grid, where JETCARP's
//! offset rows and Golly's sheared grid differ.
//!
//! Ignored: it needs Golly's command-line program, `bgolly` (in Golly 5.0's packages), on the PATH
//! or named by `JETCARP_BGOLLY`, a command whose words are split on spaces (`flatpak-spawn --host
//! bgolly` from inside a Flatpak). The release candidates run it:
//! `cargo test --release -p jetcarp-api --test golly -- --ignored`.

mod common;

use jetcarp_api::Ctx;
use jetcarp_api::cells::{Grid, parse_rle};
use jetcarp_api::op::{Quiet, WorldOp};
use jetcarp_api::ops::world::*;
use jetcarp_api::rle;
use jetcarp_api::world::{Host, Session};
use jetcarp_core::Command;
use serde_json::{Value, json};
use std::collections::BTreeSet;
use std::path::Path;
use std::process::Command as Process;

fn new_world(cx: &Ctx, h: &mut Session, v: Value) {
    WorldNew::run(cx, h, serde_json::from_value(v).unwrap(), &Quiet).unwrap();
}

fn draw(cx: &Ctx, h: &mut Session, v: Value) {
    WorldDraw::run(cx, h, serde_json::from_value(v).unwrap(), &Quiet).unwrap();
}

/// `bgolly`, as a program and its first arguments.
fn bgolly() -> Vec<String> {
    std::env::var("JETCARP_BGOLLY")
        .unwrap_or_else(|_| "bgolly".into())
        .split_whitespace()
        .map(String::from)
        .collect()
}

/// Golly's pattern after `gens` generations of the RLE `text` (0: read and written back).
fn golly(dir: &Path, name: &str, text: &str, gens: u64) -> String {
    let input = dir.join(format!("{name}.rle"));
    let output = dir.join(format!("{name}-golly-{gens}.rle"));
    std::fs::write(&input, text).unwrap();
    let _ = std::fs::remove_file(&output);
    let cmd = bgolly();
    let out = Process::new(&cmd[0])
        .args(&cmd[1..])
        .args(["-q", "-q", "-m", &gens.to_string(), "-o"])
        .arg(&output)
        .arg(&input)
        .output()
        .unwrap_or_else(|e| {
            panic!("this test needs Golly's bgolly (or JETCARP_BGOLLY naming it): {e}")
        });
    assert!(
        out.status.success(),
        "bgolly failed on {name}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    std::fs::read_to_string(&output)
        .unwrap_or_else(|e| panic!("bgolly wrote no {}: {e}", output.display()))
}

/// The live cells of an RLE, in its own coordinates, moved so the lowest x and y are 0.
fn cells(text: &str) -> BTreeSet<(i64, i64)> {
    let p = parse_rle(text).unwrap();
    let x0 = p.iter().map(|c| c.0).min().unwrap_or(0);
    let y0 = p.iter().map(|c| c.1).min().unwrap_or(0);
    p.iter()
        .filter(|c| c.2 != 0)
        .map(|c| (c.0 - x0, c.1 - y0))
        .collect()
}

/// A random soup as a pattern's text, `w` by `h`, about `fill` live (a fixed generator, so every
/// run checks the same soups).
fn soup(seed: u64, w: usize, h: usize, fill: f64) -> String {
    let mut s = seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1;
    let mut rows = Vec::new();
    for _ in 0..h {
        let mut row = String::new();
        for _ in 0..w {
            s ^= s << 13;
            s ^= s >> 7;
            s ^= s << 17;
            row.push(if (s >> 11) as f64 / (1u64 << 53) as f64 <= fill {
                'O'
            } else {
                '.'
            });
        }
        rows.push(row);
    }
    rows.join("\n")
}

/// One case: a tiling, a rule and how JETCARP's CA names its neighbours.
struct Case {
    tiling: &'static str,
    rule: &'static str,
    nb: &'static str,
    gens: &'static [u64],
}

const CASES: &[Case] = &[
    Case {
        tiling: "square",
        rule: "B3/S23",
        nb: "touch",
        gens: &[1, 2, 7, 30, 100],
    },
    Case {
        tiling: "square",
        rule: "B36/S23",
        nb: "touch",
        gens: &[1, 5, 40],
    },
    Case {
        tiling: "square",
        rule: "B3678/S34678",
        nb: "touch",
        gens: &[1, 5, 40],
    },
    Case {
        tiling: "square",
        rule: "B2/S",
        nb: "touch",
        gens: &[1, 3, 8],
    },
    Case {
        tiling: "square",
        rule: "B2/S013",
        nb: "edge",
        gens: &[1, 4, 12],
    },
    Case {
        tiling: "square",
        rule: "B3/S23",
        nb: "edge",
        gens: &[1, 5, 20],
    },
    Case {
        tiling: "hex",
        rule: "B2/S34",
        nb: "touch",
        gens: &[1, 2, 7, 30, 60],
    },
    Case {
        tiling: "hex",
        rule: "B24/S35",
        nb: "touch",
        gens: &[1, 5, 30],
    },
    Case {
        tiling: "hex",
        rule: "B3/S23",
        nb: "touch",
        gens: &[1, 5, 30],
    },
];

/// The world's size, and where the soup goes, far enough from the dead edges that nothing reaches
/// them in the generations run (checked).
const SIZE: i64 = 300;
const SOUP: usize = 24;

/// Golly reads JETCARP's RLE as the same cells, writes it back as JETCARP reads it, and runs it to
/// the same generations, for soups of each rule on squares and hexagons, placed on rows of both
/// parities (where a hexagon's neighbours differ in JETCARP's offset rows).
#[test]
#[ignore = "needs Golly's bgolly; run on the release candidates"]
fn golly_agrees_on_jetcarp_s_rle() {
    let dir = common::scratch("golly");
    let cx = Ctx::interactive();
    let mut checked = 0;
    for (ci, c) in CASES.iter().enumerate() {
        for (si, (seed, fill, row)) in [(1u64, 0.35, 137i64), (2, 0.5, 138)].iter().enumerate() {
            let name = format!("{}-{}-{}-{si}", c.tiling, c.nb, c.rule.replace('/', ""));
            let world = json!({"tiling": c.tiling, "width": SIZE, "height": SIZE,
                "boundary": "dead",
                "cas": [{"rule": c.rule, "seed": "empty", "neighborhood": c.nb}]});
            let mut h = Session::new();
            new_world(&cx, &mut h, world.clone());
            let x = SIZE / 2 - SOUP as i64 / 2;
            draw(
                &cx,
                &mut h,
                json!({"pattern": soup(*seed + ci as u64 * 10, SOUP, SOUP, *fill), "x": x, "y": row}),
            );
            let start = rle::of_world(h.world(), &[]).unwrap();
            let rule_line = rle::header_rule(&start).unwrap();
            assert_eq!(
                rle::header_rule(&golly(&dir, &name, &start, 0)).as_deref(),
                Some(rule_line.as_str()),
                "{name}: Golly names the rule as JETCARP wrote it"
            );

            // Read and written back by Golly: the same cells.
            let back = golly(&dir, &name, &start, 0);
            assert_eq!(cells(&back), cells(&start), "{name}: Golly read\n{start}");

            // Golly's RLE pasted into JETCARP (hexagons turned back from Golly's layout) and
            // copied again: the same cells.
            let mut p = Session::new();
            new_world(&cx, &mut p, world.clone());
            draw(&cx, &mut p, json!({"rle": back, "x": x, "y": row}));
            let again = rle::of_world(p.world(), &[]).unwrap();
            assert_eq!(
                cells(&again),
                cells(&start),
                "{name}: JETCARP read Golly's\n{back}"
            );

            // The same generations.
            let mut done = 0;
            for &g in c.gens {
                h.exec(Command::Step { gens: g - done }).unwrap();
                done = g;
                let w = h.world();
                let ours = rle::of_world(w, &[]).unwrap();
                let live = cells(&ours);
                // Nothing came near the edges, where JETCARP's dead boundary and Golly's unbounded
                // plane would part.
                let grid = Grid::of(&w.topo).unwrap();
                let (gw, gh) = (grid.w, grid.h);
                let near_edge = (0..w.topo.len()).any(|i| {
                    w.fields[0].state[i] != 0 && {
                        let (cx, cy) = grid.xy(i);
                        cx < 4 || cy < 4 || cx + 4 >= gw || cy + 4 >= gh
                    }
                });
                assert!(!near_edge, "{name}: reached the edge by generation {g}");
                let theirs = cells(&golly(&dir, &name, &start, g));
                assert_eq!(
                    live,
                    theirs,
                    "{name}: generation {g} differs ({} live here, {} in Golly)",
                    live.len(),
                    theirs.len()
                );
                checked += 1;
            }
        }
    }
    eprintln!(
        "golly: {checked} generations agree, over {} soups",
        CASES.len() * 2
    );
}

// ---------------------------------------------------------------------------------------------
// Isotropic non-totalistic rules (#70)

use jetcarp_core::isotropic::{IsoRule, LETTERS, classes, full};
use rayon::prelude::*;

/// This test's own generator (xorshift64*), so every run checks the same rules and soups.
struct Gen(u64);
impl Gen {
    fn new(seed: u64) -> Gen {
        Gen(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1)
    }
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }
    fn unit(&mut self) -> f64 {
        (self.next() >> 11) as f64 / (1u64 << 53) as f64
    }
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}

/// A random isotropic rule: each letter of every count, for birth and for survival, with
/// probability `p` (birth on 0 only when `b0`).
fn random_rule(g: &mut Gen, p: f64, states: u8, b0: bool) -> IsoRule {
    let mut pick = |n: usize| -> u16 {
        (0..LETTERS[n].len().max(1)).fold(0u16, |m, l| m | ((g.unit() < p) as u16) << l)
    };
    let mut birth: [u16; 9] = std::array::from_fn(&mut pick);
    let survive: [u16; 9] = std::array::from_fn(&mut pick);
    birth[0] = b0 as u16;
    IsoRule {
        birth,
        survive,
        states,
    }
}

/// One count list written as a person might: per count, its letters in a shuffled order, or a
/// `-` and the letters it leaves out, or the bare digit for every letter.
fn written_counts(m: &[u16; 9], g: &mut Gen) -> String {
    let mut out = String::new();
    for (n, &bits) in m.iter().enumerate() {
        if bits == 0 {
            continue;
        }
        out.push(char::from(b'0' + n as u8));
        if LETTERS[n].is_empty() || (bits == full(n) && g.below(3) != 0) {
            continue;
        }
        let minus = g.below(2) == 0;
        let mut ls: Vec<char> = LETTERS[n]
            .chars()
            .enumerate()
            .filter(|(i, _)| (bits >> i & 1 == 1) != minus)
            .map(|(_, c)| c)
            .collect();
        for i in (1..ls.len()).rev() {
            ls.swap(i, g.below(i as u64 + 1) as usize);
        }
        if minus {
            out.push('-');
        }
        out.extend(ls);
    }
    out
}

/// The rule written for both programs: `B…/S…`, and `/n` after a Generations rule (the form
/// Golly and JETCARP both read).
fn written(r: &IsoRule, g: &mut Gen) -> String {
    let bs = format!(
        "B{}/S{}",
        written_counts(&r.birth, g),
        written_counts(&r.survive, g)
    );
    if r.states > 2 {
        format!("{bs}/{}", r.states)
    } else {
        bs
    }
}

/// Golly's name for a rule JETCARP names `ours`: the same for two states; for Generations,
/// Golly's `S/B/n` form of JETCARP's `B…/S…/Cn`.
fn golly_name(ours: &str) -> String {
    let p: Vec<&str> = ours.split('/').collect();
    match p.as_slice() {
        [b, s, c] => format!("{}/{}/{}", &s[1..], &b[1..], &c[1..]),
        _ => ours.to_string(),
    }
}

/// Golly's name for the rule and its cells after `gens` generations, with their states, moved so
/// the lowest x and y are 0.
fn golly_states(
    dir: &Path,
    name: &str,
    text: &str,
    gens: u64,
    generations: bool,
) -> (Option<String>, BTreeSet<(i64, i64, u32)>) {
    let input = dir.join(format!("{name}.rle"));
    let output = dir.join(format!("{name}-golly-{gens}.rle"));
    std::fs::write(&input, text).unwrap();
    let _ = std::fs::remove_file(&output);
    let cmd = bgolly();
    let run = || {
        let mut p = Process::new(&cmd[0]);
        p.args(&cmd[1..]);
        if generations {
            p.args(["-a", "Generations"]);
        }
        p.args(["-q", "-q", "-m", &gens.to_string(), "-o"])
            .arg(&output)
            .arg(&input)
            .output()
            .unwrap_or_else(|e| {
                panic!("this test needs Golly's bgolly (or JETCARP_BGOLLY naming it): {e}")
            })
    };
    // A launcher such as `flatpak-spawn` can fail now and then under many runs at once; Golly
    // itself fails the same way every time.
    let mut out = run();
    for _ in 0..2 {
        if out.status.success() {
            break;
        }
        out = run();
    }
    assert!(
        out.status.success(),
        "bgolly failed on {name}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let back = std::fs::read_to_string(&output)
        .unwrap_or_else(|e| panic!("bgolly wrote no {}: {e}", output.display()));
    let _ = std::fs::remove_file(&output);
    (
        rle::header_rule(&back),
        normalised(parse_rle(&back).unwrap()),
    )
}

/// An RLE's cells with their states, moved so the lowest x and y are 0.
fn normalised_cells(text: &str) -> BTreeSet<(i64, i64, u32)> {
    normalised(parse_rle(text).unwrap())
}

fn normalised(p: Vec<(i64, i64, u32)>) -> BTreeSet<(i64, i64, u32)> {
    let p: Vec<_> = p.into_iter().filter(|c| c.2 != 0).collect();
    let x0 = p.iter().map(|c| c.0).min().unwrap_or(0);
    let y0 = p.iter().map(|c| c.1).min().unwrap_or(0);
    p.iter().map(|c| (c.0 - x0, c.1 - y0, c.2)).collect()
}

/// A world's cells with their states, moved so the lowest x and y are 0, and whether any is near
/// the dead edges (where the world and Golly's unbounded plane would part).
fn our_states(w: &jetcarp_core::World) -> (BTreeSet<(i64, i64, u32)>, bool) {
    let grid = Grid::of(&w.topo).unwrap();
    let mut v = Vec::new();
    let mut near = false;
    for (i, &s) in w.fields[0].state.iter().enumerate() {
        if s != 0 {
            let (x, y) = grid.xy(i);
            near |= x < 4 || y < 4 || x + 4 >= grid.w || y + 4 >= grid.h;
            v.push((x as i64, y as i64, s as u32));
        }
    }
    (normalised(v), near)
}

/// Which classes of arrangement (count and letter) a soup's dead (0) and live (1) cells had at
/// the start.
type Seen = [[bool; 256]; 2];

/// One isotropic case: the rule as written, and the soups' fills.
struct IsoCase {
    written: String,
    fills: Vec<f64>,
}

/// Generations every soup is compared for, every one of them.
const ISO_GENS: u64 = 100;

/// The cases: the rules of Hensel's and Golly's documentation and the issue, Life with every
/// letter written out, two B/S rules with B0, then random rules (two states, B0, Generations),
/// each letter of each count in about as many as not, written with letters, `-` forms and bare
/// digits in shuffled orders.
fn iso_cases() -> Vec<IsoCase> {
    let mut cases: Vec<IsoCase> = [
        // Hensel's example and Golly's (David Bell's Just Friends), and Golly's equivalent form.
        "B2-a/S12",
        "B2cekin/S12",
        // The issue's examples.
        "B3-cnqy/S23-a",
        // Life written with every letter: exactly Life.
        "B3aceijknqry/S2aceikn3aceijknqry",
        "B3/S2-3",
        // B/S rules with B0, without and with survival on 8, kept against their background as
        // the isotropic ones are.
        "B014/S2",
        "B0123478/S01234678",
    ]
    .iter()
    .map(|w| IsoCase {
        written: w.to_string(),
        fills: vec![0.35, 0.5],
    })
    .collect();
    let mut g = Gen::new(70);
    for k in 0..48u32 {
        let p = [0.25, 0.4, 0.5, 0.6][k as usize % 4];
        let r = random_rule(&mut g, p, 2, false);
        cases.push(IsoCase {
            written: written(&r, &mut g),
            fills: vec![0.3 + 0.1 * (k % 3) as f64, 0.55 + 0.1 * (k % 2) as f64],
        });
    }
    // Birth on 0, with and without survival on 8: Golly and JETCARP both keep the cells
    // relative to the background.
    for k in 0..8u32 {
        let mut r = random_rule(&mut g, 0.45, 2, true);
        r.survive[8] = (k % 2) as u16;
        cases.push(IsoCase {
            written: written(&r, &mut g),
            fills: vec![0.35, 0.5],
        });
    }
    // Generations.
    for k in 0..10u32 {
        let r = random_rule(
            &mut g,
            [0.3, 0.45][k as usize % 2],
            3 + (k % 4) as u8,
            false,
        );
        cases.push(IsoCase {
            written: written(&r, &mut g),
            fills: vec![0.35, 0.5],
        });
    }
    cases
}

/// One soup of an isotropic case, in JETCARP and in Golly, every generation to [`ISO_GENS`]:
/// Golly names the rule as JETCARP does and reaches the same cells, in the same states, every
/// generation. Returns the classes of arrangement (count and letter, `isotropic::classes`) its
/// dead cells (0) and live cells (1) had at the start, for the coverage check, or the first
/// generation that differs.
fn iso_soup(dir: &Path, ci: usize, c: &IsoCase, si: usize, fill: f64) -> Result<Seen, String> {
    let cx = Ctx::interactive();
    let (rule, _) = jetcarp_core::rules::Rule::parse(&c.written)
        .unwrap_or_else(|e| panic!("{}: {e}", c.written));
    let ours = rule.notation();
    let generations = matches!(rule.field_kind(), jetcarp_core::rules::FieldKind::Multi(_));
    let name = format!("iso-{ci}-{si}");
    let world = json!({"tiling": "square", "width": SIZE, "height": SIZE, "boundary": "dead",
        "cas": [{"rule": c.written, "seed": "empty"}]});
    let mut h = Session::new();
    new_world(&cx, &mut h, world);
    let x = SIZE / 2 - SOUP as i64 / 2;
    let seed = 1000 + 10 * ci as u64 + si as u64;
    draw(
        &cx,
        &mut h,
        json!({"pattern": soup(seed, SOUP, SOUP, fill), "x": x, "y": x}),
    );
    let what = format!("{} (soup {si}, fill {fill})", c.written);
    // Both programs keep a B0 rule's cells against its background from generation 0: dead at
    // the start when it flashes, live when it is live for good from the first step (#238).
    let mut seen = [[false; 256]; 2];
    // The start: the soup's cells, with the rule as written.
    let (start_cells, _) = our_states(h.world());
    let cells: Vec<(i64, i64)> = start_cells.iter().map(|c| (c.0, c.1)).collect();
    let start = rle::write(&cells, &c.written, &[]);
    // Copy as RLE, where it writes the rule (two states, B0 too): the canonical name, which Golly
    // reads as the same rule and the same cells.
    let copied = !generations;
    if copied {
        let copy = rle::of_world(h.world(), &[]).unwrap();
        if rle::header_rule(&copy).as_deref() != Some(ours.as_str()) {
            return Err(format!("{what}: Copy as RLE names it otherwise:\n{copy}"));
        }
        let (named, back) = golly_states(dir, &format!("{name}-copy"), &copy, 0, false);
        if named.as_deref() != Some(ours.as_str()) || back != normalised_cells(&copy) {
            return Err(format!("{what}: Golly reads Copy as RLE otherwise"));
        }
    }
    // Coverage: the classes of arrangement at the start, by the cell's state.
    {
        let w = h.world();
        let grid = Grid::of(&w.topo).unwrap();
        let f = &w.fields[0].state;
        for (i, &s) in f.iter().enumerate() {
            if s > 1 {
                continue;
            }
            let (x, y) = grid.xy(i);
            let mut nb = 0u8;
            for (k, &(dx, dy)) in jetcarp_core::isotropic::OFFSETS.iter().enumerate() {
                if let Ok(j) = grid.index(x as i64 + dx as i64, y as i64 + dy as i64) {
                    nb |= ((f[j] == 1) as u8) << k;
                }
            }
            seen[s as usize][classes()[nb as usize] as usize] = true;
        }
    }
    for g in 1..=ISO_GENS {
        h.exec(Command::Step { gens: 1 }).unwrap();
        let (live, near) = our_states(h.world());
        assert!(!near, "{what}: reached the edge by generation {g}");
        let (named, theirs) = golly_states(dir, &name, &start, g, generations);
        if g == 1 {
            assert_eq!(
                named.as_deref(),
                Some(golly_name(&ours).as_str()),
                "{what}: Golly names the rule as JETCARP does ({ours})"
            );
        }
        if live != theirs {
            let only_ours = live.difference(&theirs).count();
            let only_theirs = theirs.difference(&live).count();
            return Err(format!(
                "{what}, named {ours}: generation {g} differs ({} cells here, {} in Golly; \
                 {only_ours} only here, {only_theirs} only in Golly)",
                live.len(),
                theirs.len()
            ));
        }
    }
    Ok(seen)
}

/// Isotropic rules in Hensel's notation against Golly (#70): every case's soups, every
/// generation to 100, the same cells in the same states, and Golly naming every rule as JETCARP
/// does. The cases are chosen so that the comparison would fail if any letter's arrangement were
/// wrong: for every two letters of a count, as birth and as survival, some case's rule takes one
/// and not the other while its soup has a cell with the first (checked here), so that cell's
/// next state would differ from Golly's at generation 1.
#[test]
#[ignore = "needs Golly's bgolly; run on the release candidates"]
fn golly_agrees_on_isotropic_rules() {
    use jetcarp_core::rules::Rule;
    let dir = common::scratch("golly-iso");
    let cases = iso_cases();
    let runs: Vec<(usize, usize, f64)> = cases
        .iter()
        .enumerate()
        .flat_map(|(ci, c)| c.fills.iter().enumerate().map(move |(si, &f)| (ci, si, f)))
        .collect();
    let results: Vec<(usize, Result<Seen, String>)> = runs
        .par_iter()
        .map(|&(ci, si, fill)| (ci, iso_soup(&dir, ci, &cases[ci], si, fill)))
        .collect();
    let failed: Vec<&String> = results
        .iter()
        .filter_map(|(_, r)| r.as_ref().err())
        .collect();
    let first = failed
        .iter()
        .filter(|e| e.contains("generation 1 differs"))
        .count();
    assert!(
        failed.is_empty(),
        "{} of {} soups differ from Golly ({first} at generation 1):\n{}",
        failed.len(),
        runs.len(),
        failed
            .iter()
            .map(|e| e.as_str())
            .collect::<Vec<_>>()
            .join("\n")
    );
    let seen: Vec<(usize, Seen)> = results
        .into_iter()
        .map(|(ci, r)| (ci, r.unwrap()))
        .collect();
    // Coverage, over the rules without B0 (a B0 rule's stored cells are relative to its
    // background).
    let t = classes();
    let rules: Vec<Option<IsoRule>> = cases
        .iter()
        .map(|c| match Rule::parse(&c.written).unwrap().0 {
            Rule::Isotropic(r) if !r.b0() => Some(*r),
            _ => None,
        })
        .collect();
    let mut missing = Vec::new();
    for s in 0..2usize {
        for (n, ls) in LETTERS.iter().enumerate() {
            for (a, la) in ls.chars().enumerate() {
                for (b, lb) in ls.chars().enumerate() {
                    if a == b {
                        continue;
                    }
                    let (ca, cb) = ((n << 4 | a) as u8, (n << 4 | b) as u8);
                    let one = |class: u8| t.iter().position(|&c| c == class).unwrap() as u8;
                    let (xa, xb) = (one(ca), one(cb));
                    let caught = seen.iter().any(|(ci, sn)| {
                        rules[*ci].as_ref().is_some_and(|r| {
                            let bit = |x: u8| if s == 0 { r.born(x) } else { r.survives(x) };
                            sn[s][ca as usize] && bit(xa) != bit(xb)
                        })
                    });
                    if !caught {
                        missing.push(format!(
                            "{} on {n}{la} read as {n}{lb}",
                            ["birth", "survival"][s]
                        ));
                    }
                }
            }
        }
    }
    assert!(
        missing.is_empty(),
        "the cases would not catch: {}",
        missing.join(", ")
    );
    eprintln!(
        "golly: {} rules (isotropic, and B/S with B0), {} soups, {ISO_GENS} generations each, \
         agree",
        cases.len(),
        runs.len()
    );
}

// ---------------------------------------------------------------------------------------------
// B0 rules copied as RLE (#238)

/// A world's stored cells in Golly's layout (hexagons sheared), moved so the lowest x and y are
/// 0, and whether any is near the dead edges.
fn layout_cells(w: &jetcarp_core::World, hex: bool) -> (BTreeSet<(i64, i64)>, bool) {
    let grid = Grid::of(&w.topo).unwrap();
    let mut v = Vec::new();
    let mut near = false;
    for (i, &s) in w.fields[0].state.iter().enumerate() {
        if s != 0 {
            let (x, y) = grid.xy(i);
            near |= x < 4 || y < 4 || x + 4 >= grid.w || y + 4 >= grid.h;
            let (x, y) = (x as i64, y as i64);
            v.push((if hex { rle::golly_hex_x(x, y) } else { x }, y));
        }
    }
    let x0 = v.iter().map(|c| c.0).min().unwrap_or(0);
    let y0 = v.iter().map(|c| c.1).min().unwrap_or(0);
    (v.iter().map(|c| (c.0 - x0, c.1 - y0)).collect(), near)
}

/// B0 rules copied with Copy as RLE run in Golly from generation 0 as they go on in JETCARP: a
/// background that flashes (copied at even generations, as Copy as RLE requires) and one live
/// for good from the first step (copied at any), on squares (Moore and von Neumann), hexagons
/// and with an isotropic rule. Copy as RLE refuses a flashing one at an odd generation.
#[test]
#[ignore = "needs Golly's bgolly; run on the release candidates"]
fn golly_runs_copied_b0_patterns() {
    let dir = common::scratch("golly-b0");
    let cx = Ctx::interactive();
    // (tiling, rule, neighbourhood, whether the background is live for good)
    let cases = [
        ("square", "B014/S2", "touch", false),
        ("square", "B0123478/S01234678", "touch", true),
        ("square", "B03/S238", "touch", true),
        ("square", "B013/S12", "edge", false),
        ("square", "B013/S0124", "edge", true),
        ("hex", "B0124/S35", "touch", false),
        ("hex", "B0135/S0256", "touch", true),
        ("square", "B01e2a/S2-i3", "touch", false),
        ("square", "B01e3-cnqy/S23-a8", "touch", true),
    ];
    let mut checked = 0;
    for (ci, &(tiling, rule, nb, live)) in cases.iter().enumerate() {
        let hex = tiling == "hex";
        for at in [0u64, 1, 2, 5] {
            let name = format!("b0-{ci}-{at}");
            let world = json!({"tiling": tiling, "width": SIZE, "height": SIZE,
                "boundary": "dead", "cas": [{"rule": rule, "seed": "empty", "neighborhood": nb}]});
            let mut h = Session::new();
            new_world(&cx, &mut h, world);
            let x = SIZE / 2 - SOUP as i64 / 2;
            draw(
                &cx,
                &mut h,
                json!({"pattern": soup(500 + ci as u64, SOUP, SOUP, 0.4), "x": x, "y": x + 1}),
            );
            h.exec(Command::Step { gens: at }).unwrap();
            let copy = rle::of_world(h.world(), &[]);
            if !live && at % 2 == 1 {
                let e = copy.expect_err("a flashing background at an odd generation");
                assert!(e.contains("even generations"), "{rule}: {e}");
                continue;
            }
            let copy = copy.unwrap_or_else(|e| panic!("{rule} at {at}: {e}"));
            let (start, _) = layout_cells(h.world(), hex);
            assert_eq!(
                cells(&copy),
                start,
                "{rule} at {at}: the copy is the world's cells"
            );
            let mut done = 0;
            for g in [1u64, 2, 3, 8, 30] {
                h.exec(Command::Step { gens: g - done }).unwrap();
                done = g;
                let (ours, near) = layout_cells(h.world(), hex);
                assert!(!near, "{rule}: reached the edge by generation {}", at + g);
                let theirs = cells(&golly(&dir, &name, &copy, g));
                assert_eq!(
                    ours,
                    theirs,
                    "{rule} on {tiling} ({nb}), copied at generation {at}: generation {g} of \
                     the copy differs ({} here, {} in Golly)",
                    ours.len(),
                    theirs.len()
                );
                checked += 1;
            }
        }
    }
    eprintln!("golly: {checked} generations of copied B0 patterns agree");
}

/// Every find in a library that Golly can hold (two-state B/S rules on squares and hexagons), copied
/// as RLE, pastes back into JETCARP as the same cells and runs to the same generations in Golly:
/// over four of its periods, and at least 30 generations. Ignored: it needs `bgolly` and
/// `JETCARP_GOLLY_LIBRARY`, a library file to read (give it a copy; opening it may update its
/// format). The release candidates run it on a library of fresh hunts.
#[test]
#[ignore = "needs Golly's bgolly and a library; run on the release candidates"]
fn golly_agrees_on_a_library_s_finds() {
    let path = std::env::var_os("JETCARP_GOLLY_LIBRARY")
        .expect("set JETCARP_GOLLY_LIBRARY to a library file");
    let lib = jetcarp_store::Library::open(std::path::PathBuf::from(path));
    let records = lib.list().unwrap();
    let dir = common::scratch("golly-finds");
    let cx = Ctx::interactive();
    let (mut checked, mut skipped) = (0usize, 0usize);
    let mut classes: std::collections::BTreeMap<String, usize> = Default::default();
    for r in &records {
        let made = jetcarp_core::pattern::promote::showcase_scene(r, None, None)
            .and_then(|s| jetcarp_core::World::from_scene(&s))
            .and_then(|w| rle::of_world(&w, &[]));
        // Not something Golly holds (another tiling, rule family, or a B0 rule).
        let Ok(text) = made else {
            skipped += 1;
            continue;
        };
        let rule = rle::header_rule(&text).unwrap();
        let (tiling, nb, bs) = if let Some(b) = rule.strip_suffix('H') {
            ("hex", "touch", b.to_string())
        } else if let Some(b) = rule.strip_suffix('V') {
            ("square", "edge", b.to_string())
        } else {
            ("square", "touch", rule.clone())
        };
        let gens = (r.period.max(1) * 4).clamp(30, 400);
        let extent = cells(&text).iter().fold(0i64, |m, &(x, y)| m.max(x).max(y)) + 1;
        let size = (extent + 2 * gens as i64 + 24).min(2000);
        let world = json!({"tiling": tiling, "width": size, "height": size, "boundary": "dead",
            "cas": [{"rule": bs, "seed": "empty", "neighborhood": nb}]});
        let mut h = Session::new();
        new_world(&cx, &mut h, world);
        let at = size / 2 - extent / 2;
        draw(&cx, &mut h, json!({"rle": text, "x": at, "y": at}));
        let start = rle::of_world(h.world(), &[]).unwrap();
        let name = format!("{}-{}", r.id, r.class.key());
        assert_eq!(
            cells(&start),
            cells(&text),
            "{name}: pasted back as other cells\n{text}"
        );
        h.exec(Command::Step { gens }).unwrap();
        let ours = cells(&rle::of_world(h.world(), &[]).unwrap());
        let theirs = cells(&golly(&dir, &name, &start, gens));
        assert_eq!(
            ours,
            theirs,
            "{name} ({rule}): generation {gens} differs ({} live here, {} in Golly)",
            ours.len(),
            theirs.len()
        );
        checked += 1;
        *classes.entry(r.class.key().to_string()).or_default() += 1;
    }
    eprintln!(
        "golly: {checked} finds agree ({classes:?}); {skipped} of {} are not for Golly",
        records.len()
    );
    assert!(checked > 0, "no find in the library is one Golly holds");
}
