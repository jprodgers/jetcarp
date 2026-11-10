//! HashLife (`hashlife.rs`) against the ordinary stepper: identical states on wrap-edge tori
//! (power-of-two sides) and on the unbounded plane, over many rules, tilings, sizes, soups, and
//! generation counts; garbage collection never changes a result.
use jetcarp_core::hashlife::{self, Options, Plane};
use jetcarp_core::*;
use rayon::prelude::*;

fn world(family: &str, w: u32, h: u32, b: Boundary, rule: &str, seed: SeedSpec, rng: u64) -> World {
    let mut sc = Scene::new(TilingSpec::rect(family, w, h, b));
    sc.rng_state = rng;
    sc.cas = vec![CaSpec::new(rule).seed(seed)];
    World::from_scene(&sc).unwrap()
}

fn soup(rule: &str) -> SeedSpec {
    if rule == "WireWorld" || rule.starts_with("GH") {
        SeedSpec::RandomStates
    } else {
        SeedSpec::Random { density: 0.35 }
    }
}

fn stepped(mut w: World, n: u64) -> World {
    w.apply(Command::Step { gens: n }).unwrap();
    w
}

/// Wrap-edge squares: Life-like (Moore and von Neumann), Generations, WireWorld; many sizes,
/// soups, and generation counts, including ones that are not powers of two.
#[test]
fn torus_equals_stepper_square() {
    let rules = [
        "B3/S23",
        "B36/S23",
        "B2/S",
        "B3678/S34678",
        "B2/S/C3",
        "B3/S23V",
        "B2/S/C4V",
        "B35678/S5678",
        "B1/S1",
        "WireWorld",
        "B2/S345/C4",
        "GH 8/1",
        "GH 3/1",
    ];
    let sizes = [
        (8u32, 8u32),
        (16, 16),
        (32, 8),
        (8, 64),
        (64, 64),
        (128, 32),
    ];
    let gens = [1u64, 2, 3, 5, 16, 37, 64, 100, 129];
    let cases: Vec<(usize, usize, usize)> = (0..rules.len())
        .flat_map(|r| (0..sizes.len()).flat_map(move |s| (0..gens.len()).map(move |g| (r, s, g))))
        .collect();
    let bad: Vec<String> = cases
        .par_iter()
        .filter_map(|&(r, s, g)| {
            let (rule, (w, h), n) = (rules[r], sizes[s], gens[g]);
            let seed = 1000 + (r * 97 + s * 13 + g) as u64;
            let a = stepped(
                world("square", w, h, Boundary::Wrap, rule, soup(rule), seed),
                n,
            );
            let mut b = world("square", w, h, Boundary::Wrap, rule, soup(rule), seed);
            b.apply(Command::Jump { gens: n }).unwrap();
            (a.states_hash() != b.states_hash() || a.generation != b.generation)
                .then(|| format!("{rule} {w}x{h} gens {n}"))
        })
        .collect();
    assert!(bad.is_empty(), "torus mismatches:\n{}", bad.join("\n"));
}

/// Every lattice tiling with a power-of-two torus (sheared and twisted ones included).
#[test]
fn torus_equals_stepper_lattices() {
    let fams = [
        ("hex", "B2/S34"),
        ("triangle", "B2/S/C3"),
        ("brick", "B3/S23"),
        ("lizard", "B35/S234"),
        ("quad", "B3/S23"),
        ("oct", "B2/S23V"),
        ("cairo", "B2/S34"),
        ("trihex", "B2/S34"),
        ("elongtri", "B2/S/C3"),
        ("snubhex", "B2/S23"),
        ("rhombitrihex", "B2/S34"),
        ("trunchex", "B2/S23V"),
        ("herring", "B35/S234"),
        ("hex", "WireWorld"),
    ];
    let bad: Vec<String> = fams
        .par_iter()
        .flat_map(|&(f, rule)| {
            [(8u32, 8u32, 7u64), (16, 8, 33), (16, 32, 64), (32, 32, 100)]
                .into_par_iter()
                .filter_map(move |(w, h, n)| {
                    let seed = (w * 31 + h + n as u32) as u64;
                    let mk = || world(f, w, h, Boundary::Wrap, rule, soup(rule), seed);
                    let a = stepped(mk(), n);
                    let mut b = mk();
                    if let Err(e) = b.apply(Command::Jump { gens: n }) {
                        return Some(format!("{f} {rule} {w}x{h}: {e}"));
                    }
                    (a.states_hash() != b.states_hash())
                        .then(|| format!("{f} {rule} {w}x{h} gens {n}"))
                })
        })
        .collect();
    assert!(
        bad.is_empty(),
        "lattice torus mismatches:\n{}",
        bad.join("\n")
    );
}

/// Jumps longer than the torus side (the memoised torus tiling) still match the stepper.
#[test]
fn torus_long_jumps() {
    for (rule, n) in [
        ("B3/S23", 1000u64),
        ("B36/S23", 777),
        ("B2/S/C3", 513),
        ("WireWorld", 300),
    ] {
        let mk = || world("square", 32, 16, Boundary::Wrap, rule, soup(rule), 77);
        let a = stepped(mk(), n);
        let mut b = mk();
        b.apply(Command::Jump { gens: n / 3 }).unwrap();
        b.apply(Command::Jump { gens: n - n / 3 }).unwrap();
        assert_eq!(a.states_hash(), b.states_hash(), "{rule} {n}");
        assert_eq!(b.generation, n);
    }
}

/// A jump is logged and replays to the same state.
#[test]
fn jump_replays() {
    let mut w = world(
        "square",
        64,
        64,
        Boundary::Wrap,
        "B3/S23",
        soup("B3/S23"),
        5,
    );
    w.apply(Command::Step { gens: 3 }).unwrap();
    w.apply(Command::Jump { gens: 500 }).unwrap();
    let sc = w.to_scene();
    let r = World::from_scene(&sc).unwrap();
    assert_eq!(r.hash_hex(), w.hash_hex());
    assert_eq!(r.generation, 503);
}

/// HashLife does not track ages: after a jump every nonzero cell has age 1 and every empty cell
/// age 0 (Generations' dying states included), while the states equal stepping.
#[test]
fn jump_ages_restart_at_one() {
    for rule in ["B3/S23", "B2/S/C3"] {
        let w0 = world("square", 32, 32, Boundary::Wrap, rule, soup(rule), 3);
        let s = stepped(
            world("square", 32, 32, Boundary::Wrap, rule, soup(rule), 3),
            77,
        );
        let mut w = w0;
        w.apply(Command::Jump { gens: 77 }).unwrap();
        assert_eq!(w.states_hash(), s.states_hash());
        let f = &w.fields[0];
        assert!(f.state.iter().any(|&v| v > 1) || rule == "B3/S23");
        for (v, a) in f.state.iter().zip(&f.age) {
            assert_eq!(*a, (*v != 0) as u16, "{rule}");
        }
        if rule == "B3/S23" {
            assert!(
                s.fields[0].age.iter().any(|&a| a > 1),
                "stepping counts ages up"
            );
        }
    }
}

/// Worlds HashLife cannot run in place say why.
#[test]
fn torus_refusals() {
    let dead = world(
        "square",
        64,
        64,
        Boundary::Dead,
        "B3/S23",
        soup("B3/S23"),
        1,
    );
    assert!(hashlife::torus_problem(&dead).unwrap().contains("wrap"));
    let odd = world(
        "square",
        60,
        64,
        Boundary::Wrap,
        "B3/S23",
        soup("B3/S23"),
        1,
    );
    assert!(
        hashlife::torus_problem(&odd)
            .unwrap()
            .contains("powers of two")
    );
    let b0 = world("square", 64, 64, Boundary::Wrap, "B0/S8", soup("B3/S23"), 1);
    assert!(hashlife::torus_problem(&b0).is_none());
    let ant = world(
        "square",
        64,
        64,
        Boundary::Wrap,
        "Ant RL",
        SeedSpec::Center,
        1,
    );
    assert!(hashlife::torus_problem(&ant).is_some());
    let sp = World::new(TilingSpec::new("spectre"), 1).unwrap();
    assert!(hashlife::plane_problem(&sp).is_some());
    // A second CA HashLife cannot run is named.
    let mut sc = Scene::new(TilingSpec::rect("square", 32, 32, Boundary::Wrap));
    sc.cas = vec![
        CaSpec::new("B3/S23").seed(soup("B3/S23")),
        CaSpec::new("Ant RL").private(),
    ];
    let w = World::from_scene(&sc).unwrap();
    let why = hashlife::torus_problem(&w).unwrap();
    assert!(why.contains("CA 2"), "{why}");
    assert_eq!(hashlife::plane_problem(&w), Some(why));
}

/// The plane: a soup in the middle of a dead-edged world that it never reaches, against the
/// stepper, on several lattices and rules.
#[test]
fn plane_equals_stepper() {
    let cases = [
        ("square", "B3/S23"),
        ("square", "B36/S23"),
        ("square", "B2/S/C3"),
        ("square", "B3/S23V"),
        ("square", "WireWorld"),
        ("hex", "B2/S34"),
        ("triangle", "B2/S/C3"),
        ("cairo", "B2/S34"),
        ("snubhex", "B2/S23"),
    ];
    let bad: Vec<String> = cases
        .par_iter()
        .flat_map(|&(f, rule)| {
            (0..6u64).into_par_iter().filter_map(move |s| {
                let n = [1u64, 4, 9, 16, 23, 30][s as usize];
                // A soup in a 20x20 square of unit cells at the centre of 100x100.
                let mut base = world(f, 100, 100, Boundary::Dead, rule, SeedSpec::Empty, 40 + s);
                let mut rng = jetcarp_core::rng::Rng::new(900 + s);
                let g = base.topo.lattice().unwrap().clone();
                let mut cells = vec![];
                for j in 40..60 {
                    for u in 40..60 {
                        for k in 0..g.ntypes {
                            if rng.below(100) < 35 {
                                let st = if rule == "WireWorld" {
                                    1 + rng.below(3) as u32
                                } else {
                                    1
                                };
                                cells.push([g.index(u, j, k) as u32, st]);
                            }
                        }
                    }
                }
                base.apply(Command::SetCells { id: 1, cells }).unwrap();
                let mut p = Plane::from_world(&base, &Options::default()).unwrap();
                p.step(n).unwrap();
                let a = stepped(base, n);
                let sc = p.to_scene((0, 0, 100, 100), Boundary::Dead).unwrap();
                let b = World::from_scene(&sc).unwrap();
                (a.states_hash() != b.states_hash()).then(|| format!("{f} {rule} gens {n}"))
            })
        })
        .collect();
    assert!(bad.is_empty(), "plane mismatches:\n{}", bad.join("\n"));
}

/// Jumping a + b equals jumping a then b, and a glider travels exactly 2^40 / 4 cells.
#[test]
fn plane_long_jumps() {
    let o = Options::default();
    let glider = "x = 3, y = 3, rule = B3/S23\nbob$2bo$3o!";
    let mut p = hashlife::plane_from_text(glider, None, &o).unwrap();
    let b0 = p.bounds().unwrap();
    let n = 1u64 << 40;
    p.step(n).unwrap();
    let b1 = p.bounds().unwrap();
    assert_eq!(p.population(), 5);
    let d = (n / 4) as i128;
    assert_eq!([b1[0] - b0[0], b1[1] - b0[1]], [d, d]);

    // R-pentomino: 1103 generations then 0..=7 more, in pieces and at once.
    let r = "x = 3, y = 3\nb2o$2o$bo!";
    let mut a = hashlife::plane_from_text(r, None, &o).unwrap();
    let mut b = hashlife::plane_from_text(r, None, &o).unwrap();
    a.step(1103).unwrap();
    b.step(1000).unwrap();
    b.step(100).unwrap();
    b.step(3).unwrap();
    let box_ = a.bounds().unwrap();
    let reg = (
        box_[0] as i64 - 2,
        box_[1] as i64 - 2,
        (box_[2] - box_[0] + 5) as u64,
        (box_[3] - box_[1] + 5) as u64,
    );
    assert_eq!(
        a.cells_in(reg.0, reg.1, reg.2, reg.3),
        b.cells_in(reg.0, reg.1, reg.2, reg.3)
    );
    // Stabilised R-pentomino: 116 cells once its six gliders have left (LifeWiki).
    assert_eq!(a.population(), 116);
}

/// A tiny node cap forces many collections; results are unchanged.
#[test]
fn collection_keeps_results() {
    let tiny = Options {
        node_cap: 5000,
        ..Options::default()
    };
    let big = Options::default();
    let acorn = "x = 7, y = 3\nbo5b$3bo3b$2o2b3o!";
    let mut a = hashlife::plane_from_text(acorn, None, &tiny).unwrap();
    let mut b = hashlife::plane_from_text(acorn, None, &big).unwrap();
    for n in [1u64, 100, 1000, 4000] {
        a.step(n).unwrap();
        b.step(n).unwrap();
        let bx = b.bounds().unwrap();
        let reg = (
            bx[0] as i64,
            bx[1] as i64,
            (bx[2] - bx[0] + 1) as u64,
            (bx[3] - bx[1] + 1) as u64,
        );
        assert_eq!(
            a.cells_in(reg.0, reg.1, reg.2, reg.3),
            b.cells_in(reg.0, reg.1, reg.2, reg.3)
        );
    }
    assert!(a.stats().collections > 0, "{:?}", a.stats());
    // Acorn stabilises at generation 5206 with 633 cells (LifeWiki), gliders included.
    b.step(6000 - 5101).unwrap();
    assert_eq!(b.population(), 633);

    // Torus mode under the same cap.
    let mk = || {
        world(
            "square",
            64,
            64,
            Boundary::Wrap,
            "B3/S23",
            soup("B3/S23"),
            9,
        )
    };
    let s = stepped(mk(), 300);
    let mut t = mk();
    hashlife::jump_world(&mut t, 300, &tiny).unwrap();
    assert_eq!(s.states_hash(), t.states_hash());
}

/// RLE and generation-count parsing.
#[test]
fn parsing() {
    assert_eq!(hashlife::parse_gens("2^40").unwrap(), 1 << 40);
    assert_eq!(hashlife::parse_gens("1e6").unwrap(), 1_000_000);
    assert_eq!(hashlife::parse_gens("1_000").unwrap(), 1000);
    assert!(hashlife::parse_gens("2^63").is_err());
    let (rule, cells) =
        hashlife::parse_rle("#C glider\nx = 3, y = 3, rule = B3/S23\nbo$2bo$3o!").unwrap();
    assert_eq!(rule.as_deref(), Some("B3/S23"));
    assert_eq!(
        cells,
        vec![(1, 0, 1), (2, 1, 1), (0, 2, 1), (1, 2, 1), (2, 2, 1)]
    );
    let (_, c2) = hashlife::parse_rle("x = 4, y = 1, rule = 345/2/4\n.AB2C!").unwrap();
    assert_eq!(c2, vec![(1, 0, 1), (2, 0, 2), (3, 0, 3), (4, 0, 3)]);
    let p = hashlife::plane_from_text(
        "x = 4, y = 1, rule = 345/2/4\n.AB2C!",
        None,
        &Options::default(),
    )
    .unwrap();
    assert_eq!(p.ca().rule, "B2/S345/C4");
}

fn world_of(family: &str, w: u32, h: u32, cas: Vec<CaSpec>, rng: u64) -> World {
    let mut sc = Scene::new(TilingSpec::rect(family, w, h, Boundary::Wrap));
    sc.rng_state = rng;
    sc.cas = cas;
    World::from_scene(&sc).unwrap()
}

fn jumped(mut w: World, n: u64, opts: Options) -> Result<World, String> {
    hashlife::jump_world(&mut w, n, &opts)?;
    Ok(w)
}

/// B0 rules: the stored plane swaps 0 and the (uniformly changing) background, so HashLife runs
/// them exactly. Rules without S_max alternate; with S_max the background stays full; on
/// tilings with several tile types each type has its own background cycle.
#[test]
fn b0_rules_equal_the_stepper() {
    let cases = [
        ("square", "B0/S8"),
        ("square", "B0/S"),
        ("square", "B03/S23"),
        ("square", "B0123478/S34678"),
        ("square", "B01/S1"),
        ("square", "B0/S23V"),
        ("square", "B0/S4V"),
        ("square", "B013/S012/C4"),
        ("square", "B0/S8/C3"),
        ("square", "B02/S1/C5"),
        ("hex", "B0/S6"),
        ("hex", "B02/S34"),
        ("hex", "B0/S2/C3"),
        ("hex", "B024/S135"),
        ("oct", "B0/S4/C3"),
        ("oct", "B02/S2/C3"),
        ("trihex", "B0/S3/C3"),
        ("trihex", "B01/S/C3"),
        ("cairo", "B0/S5"),
    ];
    let bad: Vec<String> = cases
        .par_iter()
        .flat_map(|&(f, rule)| {
            [
                (16u32, 16u32, 1u64),
                (16, 16, 2),
                (32, 16, 7),
                (32, 32, 64),
                (64, 32, 101),
            ]
            .into_par_iter()
            .filter_map(move |(w, h, n)| {
                let seed = (w * 7 + h + n as u32) as u64;
                let mk = || world(f, w, h, Boundary::Wrap, rule, soup(rule), seed);
                let a = stepped(mk(), n);
                let mut b = mk();
                if let Err(e) = b.apply(Command::Jump { gens: n }) {
                    return Some(format!("{f} {rule}: {e}"));
                }
                // In two pieces (odd then even), so phases carry across jumps.
                let mut c = mk();
                c.apply(Command::Jump { gens: n / 2 + 1 }).ok()?;
                c.apply(Command::Jump {
                    gens: n - n / 2 - 1,
                })
                .ok()?;
                (a.states_hash() != b.states_hash() || a.states_hash() != c.states_hash())
                    .then(|| format!("{f} {rule} {w}x{h} gens {n}"))
            })
        })
        .collect();
    assert!(bad.is_empty(), "B0 mismatches:\n{}", bad.join("\n"));
}

/// A world whose only CA is a Life-like B0 rule stores its field relative to the background
/// (the stepper's emulation); turn it into real states.
fn real_states(w: &mut World) {
    let fp = &hashlife::LatticeRule::for_world_fields(w).unwrap()[0];
    if !fp.stored {
        return;
    }
    let g = w.topo.lattice().unwrap().clone();
    let st = w.fields[0].state.clone();
    for (idx, v) in st.iter().enumerate() {
        let k = g.decompose(idx).2;
        w.fields[0].state[idx] = fp.rule.real(fp.start, k, *v);
    }
}

/// B0 on the unbounded plane: a small soup in the middle of a torus it never wraps around
/// equals the plane's export of the same frame (the background included), at every step
/// count, whether stepped in one jump or many.
#[test]
fn b0_plane_equals_the_stepper() {
    for (f, rule) in [
        ("square", "B03/S23"),
        ("square", "B0/S8"),
        ("square", "B013/S012/C4"),
        ("hex", "B02/S34"),
        ("oct", "B0/S4/C3"),
    ] {
        let mut base = world(f, 64, 64, Boundary::Wrap, rule, SeedSpec::Empty, 3);
        let g = base.topo.lattice().unwrap().clone();
        let mut rng = jetcarp_core::rng::Rng::new(77);
        let mut cells = vec![];
        for j in 28..36 {
            for u in 28..36 {
                for k in 0..g.ntypes {
                    if rng.below(100) < 40 {
                        cells.push([g.index(u, j, k) as u32, 1]);
                    }
                }
            }
        }
        base.apply(Command::SetCells { id: 1, cells }).unwrap();
        let mut p = Plane::from_world(&base, &Options::default()).unwrap();
        let mut q = Plane::from_world(&base, &Options::default()).unwrap();
        let mut done = 0;
        for n in [1u64, 2, 3, 8, 13, 20] {
            p.step(n - done).unwrap();
            for _ in done..n {
                q.step(1).unwrap();
            }
            done = n;
            let mut a = stepped(World::from_scene(&base.to_scene()).unwrap(), n);
            real_states(&mut a);
            for (pl, how) in [(&p, "jumps"), (&q, "single steps")] {
                let sc = pl.to_scene((0, 0, 64, 64), Boundary::Wrap).unwrap();
                let b = World::from_scene(&sc).unwrap();
                assert_eq!(
                    a.fields[0].state, b.fields[0].state,
                    "{f} {rule} gens {n} ({how})"
                );
            }
        }
    }
}

/// Several CAs: separate fields step independently in lockstep, CAs sharing a field take
/// turns in list order (with their own steps per generation and neighbourhoods), switched-off
/// CAs leave their field alone; the torus and the plane equal the stepper.
#[test]
fn several_cas_per_world() {
    let sets: Vec<(&str, Vec<CaSpec>)> = vec![
        // Two private fields.
        (
            "square",
            vec![
                CaSpec::new("B3/S23").private().seed(soup("B3/S23")),
                CaSpec::new("B36/S23").private().seed(soup("B3/S23")),
            ],
        ),
        // Two rules on one shared field, the second twice per generation.
        (
            "square",
            vec![
                CaSpec::new("B3/S23").seed(soup("B3/S23")),
                CaSpec::new("B2/S34V").steps(2),
            ],
        ),
        // Life, a B0 rule on its own field, and a Generations rule on another.
        (
            "square",
            vec![
                CaSpec::new("B3/S23").seed(soup("B3/S23")),
                CaSpec::new("B01/S8").private(),
                CaSpec::new("B2/S/C3").seed(soup("B2/S/C3")),
            ],
        ),
        // WireWorld and a four-state Generations rule on the same field; Life switched off.
        (
            "hex",
            vec![
                CaSpec::new("WireWorld").seed(soup("WireWorld")),
                CaSpec::new("B2/S34/C4").steps(3),
                {
                    let mut c = CaSpec::new("B3/S23").private().seed(soup("B3/S23"));
                    c.enabled = false;
                    c
                },
            ],
        ),
        (
            "oct",
            vec![
                CaSpec::new("B3/S23").private().seed(soup("B3/S23")),
                CaSpec::new("B2/S/C3").private().seed(soup("B2/S/C3")),
            ],
        ),
    ];
    for (si, (f, cas)) in sets.iter().enumerate() {
        let mk = || world_of(f, 32, 32, cas.clone(), 40 + si as u64);
        let w0 = mk();
        assert!(w0.cas.len() >= 2);
        for n in [1u64, 5, 33, 100] {
            let a = stepped(mk(), n);
            let b = jumped(mk(), n, Options::default()).unwrap();
            for (fa, fb) in a.fields.iter().zip(&b.fields) {
                assert_eq!(fa.state, fb.state, "set {si} ({f}) torus gens {n}");
            }
        }
        // Plane: the same CAs on a dead-edged 96 × 96 world whose middle holds the soups.
        let mut sc = Scene::new(TilingSpec::rect(f, 96, 96, Boundary::Dead));
        sc.cas = cas
            .iter()
            .map(|c| c.clone().seed(SeedSpec::Empty))
            .collect();
        let mut base = World::from_scene(&sc).unwrap();
        let g = base.topo.lattice().unwrap().clone();
        let mut rng = jetcarp_core::rng::Rng::new(5 + si as u64);
        for ca in 0..base.cas.len() {
            if base.fields[base.cas[ca].field].kind == jetcarp_core::rules::FieldKind::Binary
                || ca == 0
            {
                let states = match base.fields[base.cas[ca].field].kind {
                    jetcarp_core::rules::FieldKind::Multi(n) => n as u32,
                    _ => 2,
                };
                let mut cells = vec![];
                for j in 44..52 {
                    for u in 44..52 {
                        for k in 0..g.ntypes {
                            if rng.below(100) < 40 {
                                cells.push([
                                    g.index(u, j, k) as u32,
                                    1 + rng.below(states as u64 - 1) as u32,
                                ]);
                            }
                        }
                    }
                }
                let id = base.cas[ca].id;
                base.apply(Command::SetCells { id, cells }).unwrap();
            }
        }
        let b0 = si == 2;
        let mut p = Plane::from_world(&base, &Options::default()).unwrap();
        assert_eq!(p.layers.len(), {
            let mut fs: Vec<usize> = base.cas.iter().map(|c| c.field).collect();
            fs.dedup();
            fs.sort();
            fs.dedup();
            fs.len()
        });
        let mut done = 0;
        // B0 fills a dead-edged world from its edges, so that set is checked on the torus only.
        for n in if b0 { vec![] } else { vec![1u64, 4, 9, 17] } {
            p.step(n - done).unwrap();
            done = n;
            let a = stepped(World::from_scene(&base.to_scene()).unwrap(), n);
            let sc = p.to_scene((0, 0, 96, 96), Boundary::Dead).unwrap();
            let b = World::from_scene(&sc).unwrap();
            assert_eq!(a.fields.len(), b.fields.len(), "set {si}");
            for (fa, fb) in a.fields.iter().zip(&b.fields) {
                assert_eq!(fa.state, fb.state, "set {si} ({f}) plane gens {n}");
            }
        }
    }
}

/// The bit-parallel leaf gives the same results as the recursion at every leaf level, on the
/// torus and the plane, for B/S and Generations rules on several tilings.
#[test]
fn bit_leaf_equals_the_recursion() {
    let cases = [
        ("square", "B3/S23"),
        ("square", "B36/S23"),
        ("square", "B2/S/C3"),
        ("square", "B3/S23V"),
        ("square", "B2/S345/C5"),
        ("hex", "B2/S34"),
        ("triangle", "B2/S/C3"),
        ("oct", "B3/S23"),
        ("cairo", "B2/S34"),
        ("snubhex", "B2/S23"),
    ];
    let bad: Vec<String> = cases
        .par_iter()
        .flat_map(|&(f, rule)| {
            [0u8, 3, 4, 5, 6].into_par_iter().filter_map(move |lv| {
                let o = Options {
                    leaf_level: lv,
                    ..Options::default()
                };
                let mk = || world(f, 64, 64, Boundary::Wrap, rule, soup(rule), 11);
                for n in [1u64, 3, 16, 77, 300] {
                    let a = stepped(mk(), n);
                    let b = jumped(mk(), n, o).unwrap();
                    if a.states_hash() != b.states_hash() {
                        return Some(format!("{f} {rule} leaf {lv} torus gens {n}"));
                    }
                }
                let w = mk();
                let mut p = Plane::from_world(&w, &o).unwrap();
                let mut r = Plane::from_world(
                    &w,
                    &Options {
                        leaf_level: 0,
                        ..Options::default()
                    },
                )
                .unwrap();
                // Short: several of these rules grow without bound on the plane.
                p.step(60).unwrap();
                r.step(60).unwrap();
                let b = r.bounds()?;
                let reg = (
                    b[0] as i64,
                    b[1] as i64,
                    (b[2] - b[0] + 1) as u64,
                    (b[3] - b[1] + 1) as u64,
                );
                (p.cells_in(reg.0, reg.1, reg.2, reg.3) != r.cells_in(reg.0, reg.1, reg.2, reg.3))
                    .then(|| format!("{f} {rule} leaf {lv} plane"))
            })
        })
        .collect();
    assert!(bad.is_empty(), "leaf mismatches:\n{}", bad.join("\n"));
}

/// What HashLife refuses about B0, and that a jump keeps the stepper's B0 step counter.
#[test]
fn b0_refusals_and_counters() {
    // Mixed degrees (4.8.8): the stepper's emulation assumes one degree.
    let w = world("oct", 16, 16, Boundary::Wrap, "B0/S4", soup("B3/S23"), 1);
    assert!(
        hashlife::torus_problem(&w)
            .unwrap()
            .contains("same number of neighbours")
    );
    // A B0 Life-like rule sharing its field.
    let w = world_of(
        "square",
        16,
        16,
        vec![CaSpec::new("B3/S23"), CaSpec::new("B01/S8")],
        1,
    );
    assert!(hashlife::torus_problem(&w).unwrap().contains("to itself"));
    // Jumps then steps equal steps (the counter decides the next step's masks).
    let mk = || {
        world(
            "square",
            32,
            32,
            Boundary::Wrap,
            "B03/S23",
            soup("B3/S23"),
            4,
        )
    };
    let a = stepped(mk(), 12);
    let mut b = mk();
    b.apply(Command::Jump { gens: 5 }).unwrap();
    b.apply(Command::Step { gens: 4 }).unwrap();
    b.apply(Command::Jump { gens: 3 }).unwrap();
    assert_eq!(a.states_hash(), b.states_hash());
    assert_eq!(a.cas[0].ticks, b.cas[0].ticks);
}

/// HashLife pictures take a palette: `HlColors` in Ember is exactly the old fixed colours,
/// another palette gives its background and stops (WireWorld keeps its own).
#[test]
fn hashlife_pictures_take_a_palette() {
    use jetcarp_core::hashlife::{HlColors, density_color, state_color};
    use jetcarp_core::render::Palette;
    let ember = HlColors::default();
    for states in [2u8, 3, 8] {
        for v in 1..states {
            assert_eq!(ember.state(v, states, false), state_color(v, states, false));
        }
    }
    for f in [0.01, 0.3, 1.0] {
        assert_eq!(ember.density(f), density_color(f));
    }
    assert_eq!(ember.background, render::BACKGROUND);
    let paper = Palette::builtin("Paper").unwrap();
    let pc = HlColors::new(&paper);
    assert_eq!(pc.background, paper.background_rgba());
    assert_ne!(pc.state(1, 2, false), ember.state(1, 2, false));
    assert_eq!(
        pc.state(1, 3, true),
        ember.state(1, 3, true),
        "WireWorld keeps its colours"
    );
    // A density map of a soup's region, at one site per pixel and coarser.
    let w = world(
        "square",
        64,
        64,
        Boundary::Dead,
        "B3/S23",
        soup("B3/S23"),
        3,
    );
    let p = Plane::from_world(&w, &Options::default()).unwrap();
    for side in [256, 32] {
        let r = (-64, -64, 256, 256);
        let a = p.render_density(r, side);
        let b = p.render_density_in(r, side, &HlColors::default());
        assert_eq!(a.rgba, b.rgba, "the default is Ember");
        let c = p.render_density_in(r, side, &pc);
        assert_ne!(a.rgba, c.rgba);
        let bg = &paper.background_rgba()[..];
        assert!(
            c.rgba.chunks(4).any(|x| x == bg),
            "empty blocks in Paper's background"
        );
        assert!(!c.rgba.chunks(4).any(|x| x == &render::BACKGROUND[..]));
    }
}
