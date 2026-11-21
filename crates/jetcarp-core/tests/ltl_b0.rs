//! Larger than Life rules with birth on 0 (#238): two-state ones are kept relative to their
//! background, as Life-like B0 rules are, so empty space stays empty and the stored cells are
//! `real xor background`. Checked against the rule run by brute force, on lattices with every edge
//! and on a patch, from the background's start (live when it is live for good from the first step).

use jetcarp_core::ltl::{LtlRule, Reach, reach};
use jetcarp_core::rules::Rule;
use jetcarp_core::*;

fn ltl(rule: &str) -> LtlRule {
    match Rule::parse(rule).unwrap().0 {
        Rule::Ltl(l) => *l,
        r => panic!("{r:?}"),
    }
}

/// One generation of a two-state Larger than Life rule by brute force: each cell counts the live
/// cells of its reach, and a slot past a lattice's edge counts `past(its tile type)`.
fn step(t: &Topology, l: &LtlRule, r: &Reach, s: &[u8], past: &dyn Fn(u32) -> u8) -> Vec<u8> {
    (0..t.len())
        .map(|i| {
            let c: u32 = match (r, &t.storage) {
                (Reach::Lattice(per), Storage::Lattice(g)) => {
                    let (u, j, k) = g.decompose(i);
                    let base = u + g.row_start[j as usize];
                    per[k as usize]
                        .iter()
                        .map(|o| match g.resolve(base + o.di, j + o.dj, o.k) {
                            Some(x) => s[x] as u32,
                            None => past(o.k) as u32,
                        })
                        .sum()
                }
                (Reach::Explicit(csr), _) => csr.row(i).iter().map(|&x| s[x as usize] as u32).sum(),
                _ => unreachable!(),
            };
            let v = if s[i] == 1 { &l.survive } else { &l.birth };
            v.get(c as usize).copied().unwrap_or(false) as u8
        })
        .collect()
}

/// The background's start by brute force: live when every cell, live with its reach live,
/// survives; else empty.
fn start(t: &Topology, l: &LtlRule, r: &Reach, past: &dyn Fn(u32) -> u8) -> Vec<u8> {
    let ones = vec![1u8; t.len()];
    if step(t, l, r, &ones, past) == ones {
        ones
    } else {
        vec![0; t.len()]
    }
}

/// Run a B0 rule from a soup and from an empty world, next to the rule by brute force, and
/// compare the stored cells with `real xor background` every generation. On a lattice with dead
/// or live edges the background is worked out on a wrapped copy, one state per tile type.
fn against_brute_force(spec: &TilingSpec, rule: &str, gens: u64) -> Result<(), String> {
    let l = ltl(rule);
    let mut w = World::new(spec.clone(), 11).unwrap();
    w.add_ca(CaSpec::new(rule).seed(SeedSpec::Random { density: 0.35 }))
        .unwrap();
    let mut empty = World::new(spec.clone(), 11).unwrap();
    empty
        .add_ca(CaSpec::new(rule).seed(SeedSpec::Empty))
        .unwrap();
    let t = w.topo.clone();
    let r = reach(&t, l.hood, l.range).unwrap();
    let edge = spec.boundary;
    let lattice = t.lattice().is_some();
    // The background: on a lattice from a wrapped copy, per tile type; on a patch per cell.
    let (bt, br) = if lattice && edge != Boundary::Wrap {
        let mut s = spec.clone();
        s.boundary = Boundary::Wrap;
        let bt = Topology::build(&s).unwrap();
        let br = reach(&bt, l.hood, l.range).unwrap();
        (bt, br)
    } else {
        (
            Topology::build(spec).unwrap(),
            reach(&t, l.hood, l.range).unwrap(),
        )
    };
    let never = |_: u32| -> u8 { panic!("a wrapped copy has no edge slots") };
    let mut bg = start(&bt, &l, &br, &never);
    let nt = t.lattice().map_or(1, |g| g.ntypes as usize);
    let bg_of = |bg: &[u8], i: usize| if lattice { bg[i % nt] } else { bg[i] };
    let mut truth: Vec<u8> = (0..t.len())
        .map(|i| w.fields[0].state[i] ^ bg_of(&bg, i))
        .collect();
    for g in 1..=gens {
        let past_bg = bg.clone();
        let past = |k: u32| match edge {
            Boundary::Live => 1,
            _ => past_bg[k as usize],
        };
        truth = step(&t, &l, &r, &truth, &past);
        bg = step(&bt, &l, &br, &bg, &never);
        w.step();
        empty.step();
        // Past a live edge the cells are live, so an empty world lights up along it.
        let lit = empty.fields[0].population();
        if lit != 0 && edge != Boundary::Live {
            return Err(format!(
                "{rule} on {} {edge:?}: generation {g}: {lit} cells of the empty world are lit",
                spec.family
            ));
        }
        let wrong = (0..t.len())
            .filter(|&i| w.fields[0].state[i] != truth[i] ^ bg_of(&bg, i))
            .count();
        if wrong != 0 {
            return Err(format!(
                "{rule} on {} {edge:?}: generation {g}: {wrong} of {} cells differ from the \
                 true rule",
                spec.family,
                t.len()
            ));
        }
    }
    Ok(())
}

#[test]
fn ltl_b0_rules_are_kept_against_their_background() {
    let rect = |f: &str, w, h, b| TilingSpec::rect(f, w, h, b);
    let mut bad = vec![];
    // Flashing backgrounds, and ones live for good (survival on every count the reach holds:
    // 24 for range 2 Moore on squares, 12 for range 2 von Neumann).
    let rules = [
        "R2,C2,S0-5,B0-3",
        "R2,C2,S2-12,B0-4,NN",
        "R2,C2,S6-24,B0-2",
        "R2,C2,S0-3,6-12,B0-5,NN",
        "R3,C2,S0-48,B0-9",
    ];
    for b in [Boundary::Wrap, Boundary::Dead, Boundary::Live] {
        for rule in rules {
            if let Err(e) = against_brute_force(&rect("square", 24, 20, b), rule, 20) {
                bad.push(e);
            }
        }
    }
    for (f, rule) in [
        ("hex", "R2,C2,S0-4,B0-3"),
        ("hex", "R2,C2,S3-18,B0-2"),
        ("oct", "R1,C2,S2-8,B0-2"),
    ] {
        if let Err(e) = against_brute_force(&rect(f, 12, 12, Boundary::Wrap), rule, 20) {
            bad.push(e);
        }
    }
    let hat = TilingSpec::new("hat").with_size(SizeSpec::Level(2));
    for rule in ["R2,C2,S0-5,B0-3", "R2,C2,S3-99,B0-2"] {
        if let Err(e) = against_brute_force(&hat, rule, 12) {
            bad.push(e);
        }
    }
    assert!(bad.is_empty(), "{}", bad.join("\n"));
}

/// A background that is live for good from the first step starts live: the background's states
/// are all live from generation 0, and a rule whose background flashes starts empty.
#[test]
fn ltl_b0_backgrounds_start_where_golly_does() {
    let t = Topology::build(&TilingSpec::rect("square", 16, 16, Boundary::Wrap)).unwrap();
    for (rule, live) in [("R2,C2,S6-24,B0-2", true), ("R2,C2,S0-5,B0-3", false)] {
        let l = ltl(rule);
        let r = reach(&t, l.hood, l.range).unwrap();
        let plain = jetcarp_core::b0::Plain::Ltl {
            rule: &l,
            reach: &r,
        };
        let bg = jetcarp_core::b0::Background::of(&t, &plain).unwrap();
        assert_eq!(bg.starts_live, live, "{rule}");
    }
    // More states than two: run as written, as Golly refuses them.
    assert!(!ltl("R2,C3,S6-24,B0-2").b0_kept());
    assert!(ltl("R2,C2,S6-24,B0-2").b0_kept());
    assert!(!ltl("R2,C2,S6-24,B1-2").b0_kept());
}

#[test]
fn ltl_b0_results_do_not_depend_on_the_thread_count() {
    let run = |threads: usize| {
        rayon::ThreadPoolBuilder::new()
            .num_threads(threads)
            .build()
            .unwrap()
            .install(|| {
                let mut w =
                    World::new(TilingSpec::rect("square", 96, 96, Boundary::Wrap), 3).unwrap();
                w.add_ca(CaSpec::new("R2,C2,S0-5,B0-3").seed(SeedSpec::Random { density: 0.3 }))
                    .unwrap();
                w.apply(Command::Step { gens: 30 }).unwrap();
                w.hash_hex()
            })
    };
    assert_eq!(run(1), run(6));
}
