//! Phase 2 rule families: Larger than Life and B0 emulation.
use jetcarp_core::*;

fn world(rule: &str, seed: SeedSpec, b: Boundary, rng: u64) -> World {
    let mut w = World::new(TilingSpec::rect("square", 40, 30, b), rng).unwrap();
    w.add_ca(CaSpec::new(rule).seed(seed)).unwrap();
    w
}

// Range-1 Larger than Life with the Moore neighbourhood is Life itself.
#[test]
fn ltl_range_one_is_life() {
    for b in [Boundary::Wrap, Boundary::Dead, Boundary::Live] {
        let s = SeedSpec::Random { density: 0.4 };
        let mut a = world("B3/S23", s.clone(), b, 9);
        let mut l = world("R1,C2,S2-3,B3", s, b, 9);
        for g in 0..60 {
            assert_eq!(a.fields[0].state, l.fields[0].state, "{b:?} gen {g}");
            a.step();
            l.step();
        }
    }
}

// Range-1 von Neumann LtL equals the V-suffixed B/S rule.
#[test]
fn ltl_von_neumann_matches_v_rule() {
    let s = SeedSpec::Random { density: 0.3 };
    let mut a = world("B13/S024V", s.clone(), Boundary::Wrap, 4);
    let mut l = world("R1,C2,S0,2,4,B1,3,NN", s, Boundary::Wrap, 4);
    for _ in 0..40 {
        assert_eq!(a.fields[0].state, l.fields[0].state);
        a.step();
        l.step();
    }
}

// AntiLife (B0123478/S01234678) is Life with live and dead swapped. It has S8, so its background
// is live from the start (#238): JETCARP stores its complement from generation 0, which must evolve
// exactly as Life from the same cells, as Golly runs it.
#[test]
fn b0_emulation_antilife_is_inverted_life() {
    let mut anti = world(
        "B0123478/S01234678",
        SeedSpec::Random { density: 0.4 },
        Boundary::Wrap,
        3,
    );
    let start: Vec<[u32; 2]> = anti.fields[0]
        .state
        .iter()
        .enumerate()
        .filter(|(_, s)| **s == 1)
        .map(|(i, _)| [i as u32, 1])
        .collect();
    let mut life = world(
        "B3/S23",
        SeedSpec::Cells { cells: start },
        Boundary::Wrap,
        3,
    );
    for g in 0..50u64 {
        assert_eq!(anti.fields[0].state, life.fields[0].state, "generation {g}");
        anti.step();
        life.step();
    }
}

// Without S8 the stored state alternates: true on even generations, complemented on odd ones.
// Decoding it must agree with running the true rule by brute force on a torus.
#[test]
fn b0_emulation_matches_brute_force() {
    for rule in ["B014/S2", "B028/S0124", "B08/S4", "B01356/S012345"] {
        let (r, _) = jetcarp_core::rules::Rule::parse(rule).unwrap();
        let jetcarp_core::rules::Rule::LifeLike { birth, survive } = r else {
            panic!()
        };
        let mut w = world(rule, SeedSpec::Random { density: 0.3 }, Boundary::Wrap, 11);
        let mut truth: Vec<u8> = w.fields[0].state.clone();
        let t = w.topo.clone();
        for g in 1..30u64 {
            w.step();
            truth = (0..t.len())
                .map(|i| {
                    let mut c = 0;
                    t.for_each_neighbor(i, Nb::Touch, |j| c += truth[j] as u32);
                    let m = if truth[i] == 1 { survive } else { birth };
                    ((m >> c) & 1) as u8
                })
                .collect();
            let decoded: Vec<u8> = w.fields[0]
                .state
                .iter()
                .map(|&s| if g % 2 == 1 { 1 - s } else { s })
                .collect();
            assert_eq!(decoded, truth, "{rule} gen {g}");
        }
    }
}

// A B0 rule without emulation would fill the empty background on generation 1.
#[test]
fn b0_background_stays_dead() {
    let mut w = world("B014/S2", SeedSpec::Empty, Boundary::Wrap, 1);
    for _ in 0..6 {
        w.step();
        assert_eq!(w.fields[0].population(), 0);
    }
}

/// A two-state B/S rule's birth and survival masks and its neighbourhood.
fn bs_rule(rule: &str) -> (u32, u32, Nb) {
    let (r, nb) = jetcarp_core::rules::Rule::parse(rule).unwrap();
    let jetcarp_core::rules::Rule::LifeLike { birth, survive } = r else {
        panic!("{rule} is not a B/S rule")
    };
    (birth, survive, nb.unwrap_or(Nb::Touch))
}

/// The background a B0 rule starts from, by brute force: live when the live state is the first
/// step's for good (every tile, live with every slot live, survives; `past` gives a slot past an
/// edge), else the empty world.
fn start_background(t: &Topology, r: (u32, u32, Nb), past: &dyn Fn(u32) -> u8) -> Vec<u8> {
    let ones = vec![1u8; t.len()];
    if r.0 & 1 == 1 && bs_step(t, r, &ones, past) == ones {
        ones
    } else {
        vec![0; t.len()]
    }
}

/// One generation of a two-state B/S rule by brute force on the neighbour graph: each neighbour
/// slot counts its cell's state, and a slot past a lattice's edge counts `past(its tile type)`.
fn bs_step(
    t: &Topology,
    (birth, survive, nb): (u32, u32, Nb),
    s: &[u8],
    past: &dyn Fn(u32) -> u8,
) -> Vec<u8> {
    (0..t.len())
        .map(|i| {
            let mut c = 0u32;
            for k in 0..t.degree(i, nb) {
                c += match t.slot(i, nb, k) {
                    Some(j) => s[j] as u32,
                    None => {
                        let g = t.lattice().unwrap();
                        past(g.offsets[nb as usize][i % g.ntypes as usize][k].k) as u32
                    }
                };
            }
            let m = if s[i] == 1 { survive } else { birth };
            (m >> c & 1) as u8
        })
        .collect()
}

/// Run a B0 rule from a soup, and from an empty world, `gens` generations, next to the plain rule
/// run by brute force from the same soup and from the background's start (the empty world, or the
/// live one when it lasts: [`start_background`]). The stored states must be the real ones
/// relative to the background, `real xor background`, so the empty world stays empty. Returns
/// the first mismatch.
fn b0_against_brute_force(spec: &TilingSpec, rule: &str, gens: u64) -> Result<(), String> {
    let r = bs_rule(rule);
    let mut w = World::new(spec.clone(), 11).unwrap();
    w.add_ca(CaSpec::new(rule).seed(SeedSpec::Random { density: 0.35 }))
        .unwrap();
    let mut empty = World::new(spec.clone(), 11).unwrap();
    empty
        .add_ca(CaSpec::new(rule).seed(SeedSpec::Empty))
        .unwrap();
    let t = w.topo.clone();
    let n = t.len();
    let never = |_: u32| -> u8 { panic!("a wrapped lattice or a patch has no edge slots") };
    let mut bg = start_background(&t, r, &never);
    let mut truth: Vec<u8> = (0..n).map(|i| w.fields[0].state[i] ^ bg[i]).collect();
    for g in 1..=gens {
        w.step();
        empty.step();
        truth = bs_step(&t, r, &truth, &never);
        bg = bs_step(&t, r, &bg, &never);
        let lit = empty.fields[0].population();
        if lit != 0 {
            return Err(format!(
                "{rule} on {}: generation {g}: {lit} of {n} cells of the empty world are lit",
                spec.family
            ));
        }
        let wrong = (0..n)
            .filter(|&i| w.fields[0].state[i] != truth[i] ^ bg[i])
            .count();
        if wrong != 0 {
            return Err(format!(
                "{rule} on {}: generation {g}: {wrong} of {n} cells differ from the true rule",
                spec.family
            ));
        }
    }
    Ok(())
}

// B0 rules are exact on every neighbour graph (#171): on tilings where every tile has the same
// number of neighbours, and on those where the tiles have different numbers and the survival list
// holds some of them, which the alternating-background emulation got wrong from generation 3.
#[test]
fn b0_emulation_is_exact_on_every_neighbour_graph() {
    let rect = |f: &str, w, h| TilingSpec::rect(f, w, h, Boundary::Wrap);
    let level = |f: &str, l| TilingSpec::new(f).with_size(SizeSpec::Level(l));
    let cases: Vec<(TilingSpec, &[&str])> = vec![
        (
            rect("square", 12, 12),
            &["B014/S2", "B0123478/S01234678", "B0/S8"],
        ),
        (rect("hex", 12, 12), &["B0/S6", "B02/S347", "B014/S2"]),
        (rect("cairo", 8, 8), &["B0/S6", "B02/S347"]),
        // 4.8.8: squares have 4 neighbours, octagons 8.
        (
            rect("oct", 12, 12),
            &[
                "B03/S23",
                "B0123478/S01234678",
                "B0/S8",
                "B0/S4",
                "B03/S238",
                "B014/S2",
            ],
        ),
        // 3.6.3.6 with edge neighbours: triangles have 3, hexagons 6.
        (
            rect("trihex", 12, 12),
            &["B0/S6V", "B0/S3V", "B01/S36V", "B03/S23V"],
        ),
        // 3.4.6.4: 6, 8 and 12 touch neighbours.
        (
            rect("rhombitrihex", 8, 8),
            &["B03/S238", "B0/S8", "B0123478/S01234678", "B0/S6"],
        ),
        // 3.3.4.3.4: 9 and 12.
        (rect("snubsq", 8, 8), &["B03/S2,3,9", "B0/S12"]),
        // 4.6.12 with edge neighbours: 4, 6 and 12.
        (rect("trunctrihex", 8, 8), &["B0/S4V", "B0/S6V"]),
        // Patches: the open boundary's tiles have fewer neighbours than the inner ones.
        (
            level("penrose-rhomb", 5),
            &["B0123478/S01234678", "B0/S8", "B03/S238", "B014/S2"],
        ),
        (level("hat", 3), &["B0/S6", "B03/S238", "B0/S8", "B014/S2"]),
    ];
    let bad: Vec<String> = cases
        .iter()
        .flat_map(|(spec, rules)| rules.iter().map(move |r| (spec, *r)))
        .filter_map(|(spec, rule)| b0_against_brute_force(spec, rule, 30).err())
        .collect();
    assert!(bad.is_empty(), "{}", bad.join("\n"));
}

// Past a lattice's dead edge a neighbour slot is in the background, as if the tiling went on,
// empty (stored as 0); past a live edge it is live, as for B/S rules without B0.
#[test]
fn b0_dead_and_live_edges() {
    let mut bad = vec![];
    for (family, w, h) in [("square", 12, 10), ("oct", 10, 8), ("rhombitrihex", 8, 8)] {
        for boundary in [Boundary::Dead, Boundary::Live] {
            for rule in ["B0/S8", "B014/S2", "B03/S238", "B0123478/S01234678"] {
                let r = bs_rule(rule);
                let spec = TilingSpec::rect(family, w, h, boundary);
                let mut world = World::new(spec.clone(), 5).unwrap();
                world
                    .add_ca(CaSpec::new(rule).seed(SeedSpec::Random { density: 0.35 }))
                    .unwrap();
                // The background, one state per tile type, from a wrapped copy.
                let wrapped =
                    Topology::build(&TilingSpec::rect(family, w, h, Boundary::Wrap)).unwrap();
                let t = world.topo.clone();
                let nt = t.lattice().unwrap().ntypes as usize;
                let mut bg = start_background(&wrapped, r, &|_| 0);
                let mut truth: Vec<u8> = (0..t.len())
                    .map(|i| world.fields[0].state[i] ^ bg[i % nt])
                    .collect();
                for g in 1..=24 {
                    let edge = |k: u32| match boundary {
                        Boundary::Live => 1,
                        _ => bg[k as usize],
                    };
                    truth = bs_step(&t, r, &truth, &edge);
                    bg = bs_step(&wrapped, r, &bg, &|_| 0);
                    world.step();
                    let wrong = (0..t.len())
                        .filter(|&i| world.fields[0].state[i] != truth[i] ^ bg[i % nt])
                        .count();
                    if wrong != 0 {
                        bad.push(format!(
                            "{rule} on {family} {boundary:?}: generation {g}: {wrong} of {} \
                             cells differ from the true rule",
                            t.len()
                        ));
                        break;
                    }
                }
            }
        }
    }
    assert!(bad.is_empty(), "{}", bad.join("\n"));
}

/// Step a B0 world until it reports a repeat (at most `gens` generations), then a few periods
/// more. `Err` names a repeat that is not one: two generations a period apart, from where it was
/// seen on, whose stored cells differ.
fn repeat_holds(
    spec: &TilingSpec,
    rule: &str,
    seed: u64,
    gens: u64,
) -> Option<Result<u64, String>> {
    let mut w = World::new(spec.clone(), seed).unwrap();
    w.add_ca(CaSpec::new(rule).seed(SeedSpec::Random { density: 0.3 }))
        .unwrap();
    let mut states = vec![w.fields[0].state.clone()];
    let (period, seen_at) = (0..gens).find_map(|_| {
        w.step();
        states.push(w.fields[0].state.clone());
        match w.status() {
            Status::Repeats { period, seen_at } => Some((period, seen_at)),
            _ => None,
        }
    })?;
    for _ in 0..3 * period {
        w.step();
        states.push(w.fields[0].state.clone());
    }
    let (g, p) = (seen_at as usize, period as usize);
    Some(
        match (g..states.len() - p).find(|&t| states[t] != states[t + p]) {
            None => Ok(period),
            Some(t) => Err(format!(
                "{rule} on {} seed {seed}: reported to repeat with period {period} at generation \
                 {seen_at}, but generations {t} and {} differ",
                spec.family,
                t + p
            )),
        },
    )
}

// A B0 world is reported as repeating only when it repeats (#211). Its stored cells can come back
// while the background is in another phase (on the square grid, after an odd number of steps),
// and then the next steps differ: the period check compares the background's phase too. The
// first case is one the check got wrong: B014/S2's stored cells on an 8 x 8 torus repeat after
// one step at generation 16, and change again at the next.
#[test]
fn b0_repeats_are_real_repeats() {
    let square = TilingSpec::rect("square", 8, 8, Boundary::Wrap);
    let mut bad = vec![];
    match repeat_holds(&square, "B014/S2", 10, 300) {
        Some(Ok(_)) => {}
        Some(Err(e)) => bad.push(e),
        None => bad.push("B014/S2 seed 10 no longer repeats within 300 generations".into()),
    }
    let mut repeats = 0;
    let tilings = [
        square,
        TilingSpec::rect("oct", 8, 8, Boundary::Wrap),
        TilingSpec::new("hat").with_size(SizeSpec::Level(2)),
    ];
    for spec in &tilings {
        for rule in ["B014/S2", "B01/S1", "B0/S", "B013/S1", "B02/S013", "B0/S8"] {
            for seed in 1..30 {
                match repeat_holds(spec, rule, seed, 300) {
                    Some(Ok(_)) => repeats += 1,
                    Some(Err(e)) => bad.push(e),
                    None => {}
                }
            }
        }
    }
    assert!(repeats > 100, "only {repeats} worlds repeated");
    assert!(bad.is_empty(), "{}", bad.join("\n"));
}

#[test]
fn ltl_runs_on_other_tilings() {
    for fam in ["hex", "trihex", "cairo"] {
        let mut w = World::new(TilingSpec::new(fam).with_size(SizeSpec::Cells(900)), 2).unwrap();
        w.add_ca(CaSpec::new("R2,C3,S4-9,B5-8")).unwrap();
        w.apply(Command::Step { gens: 10 }).unwrap();
    }
    let mut w = World::new(TilingSpec::new("spectre").with_size(SizeSpec::Level(3)), 2).unwrap();
    w.add_ca(CaSpec::new("R2,C2,S6-12,B7-10,NN")).unwrap();
    w.apply(Command::Step { gens: 5 }).unwrap();
    // The cross neighbourhood needs the square grid.
    let mut h = World::new(TilingSpec::new("hex").with_size(SizeSpec::Cells(900)), 2).unwrap();
    assert!(h.add_ca(CaSpec::new("R3,C2,S2,B3,N+")).is_err());
}

/// Counts written in a Larger than Life rule: `"3-4,9"` is 3, 4 and 9.
fn ltl_counts(list: &str) -> Vec<usize> {
    list.split(',')
        .filter(|s| !s.is_empty())
        .flat_map(|s| {
            let (a, b) = s.split_once('-').unwrap_or((s, s));
            a.parse::<usize>().unwrap()..=b.parse::<usize>().unwrap()
        })
        .collect()
}

/// One generation of a two-state Larger than Life rule by brute force: each cell counts the live
/// cells within `r` steps over `nb`, each once. Also returns how many cells were born or survived
/// on a count above `square_max`, the most the square grid's neighbourhood holds.
fn ltl_brute(
    t: &Topology,
    nb: Nb,
    r: usize,
    (birth, survive): (&[usize], &[usize]),
    s: &[u8],
    square_max: usize,
) -> (Vec<u8>, usize) {
    let mut high = 0;
    let next = (0..t.len())
        .map(|i| {
            let mut dist = std::collections::HashMap::from([(i, 0usize)]);
            let mut q = std::collections::VecDeque::from([i]);
            while let Some(c) = q.pop_front() {
                let d = dist[&c];
                if d < r {
                    t.for_each_neighbor(c, nb, |j| {
                        if let std::collections::hash_map::Entry::Vacant(e) = dist.entry(j) {
                            e.insert(d + 1);
                            q.push_back(j);
                        }
                    });
                }
            }
            let c = dist.keys().filter(|&&j| j != i && s[j] == 1).count();
            let v = if s[i] == 1 { survive } else { birth }.contains(&c) as u8;
            high += (v == 1 && c > square_max) as usize;
            v
        })
        .collect();
    (next, high)
}

// Larger than Life keeps the counts it is given above the square grid's neighbourhood size (#172),
// so its notation reads back as written.
#[test]
fn ltl_notation_keeps_counts_above_the_square_grids_neighbourhood() {
    let bad: Vec<String> = [
        "R1,C2,S2-3,B9-12",
        "R1,C2,S0-12,B9-12",
        "R4,C2,S40-80,B41-81",
        "R7,C2,S112-224,B113-225",
        "R1,C2,S0,B5-6,NN",
        "R20,C2,S100-2000,B1700-1800",
    ]
    .iter()
    .filter_map(|text| {
        let (r, _) = jetcarp_core::rules::Rule::parse(text).unwrap();
        (r.notation() != *text).then(|| format!("{text} reads back as {}", r.notation()))
    })
    .collect();
    assert!(bad.is_empty(), "{}", bad.join("\n"));
}

// And it runs as written (#172), against brute force: a triangle has 12 touch neighbours against a
// square's 8, and a 3^6; 3^2.6^2 tiling has more cells within two steps than the square grid's 24.
#[test]
fn ltl_counts_above_the_square_grids_neighbourhood_count() {
    let kuniform = TilingSpec {
        params: Some(serde_json::json!({ "catalog": "k2-010" })),
        ..TilingSpec::rect("periodic", 10, 10, Boundary::Wrap)
    };
    let cases = [
        // (tiling, range, square grid's maximum, survival, birth, density)
        (
            TilingSpec::rect("triangle", 12, 12, Boundary::Wrap),
            1,
            8,
            "0-12",
            "9-12",
            0.9,
        ),
        (
            TilingSpec::rect("triangle", 12, 12, Boundary::Wrap),
            1,
            8,
            "3-4,9-11",
            "5,10-12",
            0.6,
        ),
        (kuniform, 2, 24, "10-14,26-40", "25-40", 0.7),
    ];
    for (spec, r, square_max, s, b, density) in cases {
        let rule = format!("R{r},C2,S{s},B{b}");
        let mut w = World::new(spec.clone(), 5).unwrap();
        w.add_ca(CaSpec::new(&rule).seed(SeedSpec::Random { density }))
            .unwrap();
        let t = w.topo.clone();
        let (birth, survive) = (ltl_counts(b), ltl_counts(s));
        let mut truth = w.fields[0].state.clone();
        let mut high = 0;
        for g in 1..=8 {
            w.step();
            let (next, h) = ltl_brute(&t, Nb::Touch, r, (&birth, &survive), &truth, square_max);
            truth = next;
            high += h;
            let wrong = (0..t.len())
                .filter(|&i| w.fields[0].state[i] != truth[i])
                .count();
            assert_eq!(
                wrong,
                0,
                "{rule} on {}: generation {g}: {wrong} of {} cells differ from the rule as written",
                spec.family,
                t.len()
            );
        }
        assert!(
            high > 0,
            "{rule} on {}: no cell was born or survived on a count above {square_max}",
            spec.family
        );
    }
    // The audit's scene: a 90 % soup on triangles, where most cells have 9 to 12 live neighbours.
    let scene: Scene = Scene::from_json(
        r#"{
          "tiling": { "family": "triangle", "size": { "rect": { "w": 12, "h": 12 } }, "boundary": "wrap" },
          "rng_state": 1,
          "cas": [ { "rule": "R1,C2,S0-12,B9-12", "seed": { "random": { "density": 0.9 } } } ]
        }"#,
    )
    .unwrap();
    let mut w = World::from_scene(&scene).unwrap();
    w.apply(Command::Step { gens: 1 }).unwrap();
    assert!(
        w.fields[0].population() > 0,
        "the 90 % soup is empty after one generation, run as {}",
        w.cas[0].notation()
    );
}

// A count larger than any neighbourhood can hold is refused, saying what the limit is.
#[test]
fn ltl_refuses_counts_past_the_limit() {
    let e = jetcarp_core::rules::Rule::parse("R2,C2,S2-3,B3-99999")
        .err()
        .expect("a count of 99,999 is refused");
    assert!(e.contains("99999") && e.contains("at most"), "{e}");
}

// Layered parity from one cell on squares with edge neighbours: layers are diamonds, each quadrant
// is Pascal's triangle mod 2, so layer d holds 4 * (2^popcount(d) - 1) live cells.
#[test]
fn layered_parity_is_pascal_mod_2() {
    let mut w = World::new(TilingSpec::rect("square", 81, 81, Boundary::Dead), 1).unwrap();
    w.add_ca(CaSpec::new("Layer 2/01").nb(Nb::Edge)).unwrap();
    let t = w.topo.clone();
    let g = t.lattice().unwrap().clone();
    let c = t.center_cell();
    let (cu, cj, _) = g.decompose(c);
    for _ in 0..30 {
        w.step();
    }
    assert_eq!(w.cas[0].cursor, 30);
    for d in 1..=30i32 {
        let live = (0..t.len())
            .filter(|&i| {
                let (u, j, _) = g.decompose(i);
                (u - cu).abs() + (j - cj).abs() == d && w.fields[0].state[i] == 1
            })
            .count();
        assert_eq!(
            live,
            4 * ((1usize << (d as u32).count_ones()) - 1),
            "layer {d}"
        );
    }
}

// Reseeding restarts the layers from the new seed.
#[test]
fn layered_reseed_restarts() {
    let mut w = World::new(TilingSpec::new("hex").with_size(SizeSpec::Cells(900)), 1).unwrap();
    let id = w.add_ca(CaSpec::new("Layer 3/012")).unwrap();
    w.apply(Command::Step { gens: 5 }).unwrap();
    assert_eq!(w.cas[0].cursor, 5);
    w.apply(Command::Clear { id }).unwrap();
    w.apply(Command::Seed {
        id,
        seed: SeedSpec::Center,
    })
    .unwrap();
    w.apply(Command::Step { gens: 2 }).unwrap();
    assert_eq!(w.cas[0].cursor, 2);
}
