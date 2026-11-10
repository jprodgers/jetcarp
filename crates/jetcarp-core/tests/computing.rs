//! Computing demos: the WireWorld adder's truth table and the Life gates' truth tables, read
//! off the output wires and lanes by simulation.
use jetcarp_core::computing::{self, WW_PERIOD};
use jetcarp_core::*;
use rayon::prelude::*;

/// Generations at which the probe cell holds an electron head.
fn heads_at(w: &mut World, probes: &[(i32, i32)], width: u32, gens: u64) -> Vec<Vec<u64>> {
    let mut out = vec![vec![]; probes.len()];
    for _ in 0..gens {
        w.step();
        for (k, &(x, y)) in probes.iter().enumerate() {
            if w.fields[0].state[(y as u32 * width + x as u32) as usize] == 1 {
                out[k].push(w.generation);
            }
        }
    }
    out
}

#[test]
fn wireworld_adder_truth_table() {
    let cases: Vec<(u32, u32)> = (0..4).flat_map(|a| (0..4).map(move |b| (a, b))).collect();
    let bad: Vec<String> = cases
        .par_iter()
        .filter_map(|&(a, b)| {
            let ad = computing::wireworld_adder(a, b).unwrap();
            let mut w = World::from_scene(&ad.scene).unwrap();
            let periods = 4;
            let gens = ad.first_output + periods * WW_PERIOD as u64;
            let got = heads_at(&mut w, &ad.outputs, ad.width, gens);
            let sum = a + b;
            for (k, g) in got.iter().enumerate() {
                let want: Vec<u64> = if sum >> k & 1 == 1 {
                    (0..=periods)
                        .map(|p| ad.first_output + p * WW_PERIOD as u64)
                        .filter(|&t| t <= gens)
                        .collect()
                } else {
                    vec![]
                };
                if *g != want {
                    return Some(format!("{a} + {b}: s{k} heads at {g:?}, want {want:?}"));
                }
            }
            None
        })
        .collect();
    assert!(bad.is_empty(), "adder errors:\n{}", bad.join("\n"));
}

/// Any live cell in the box during `gens` generations.
fn lane_busy(w: &mut World, b: [i32; 4], width: u32, gens: u64) -> bool {
    let mut seen = false;
    for _ in 0..gens {
        w.step();
        for y in b[1]..b[3] {
            for x in b[0]..b[2] {
                seen |= w.fields[0].state[(y as u32 * width + x as u32) as usize] != 0;
            }
        }
    }
    seen
}

/// Every Life gate, every input combination: gliders on the output lane exactly when the truth
/// table says 1, and the whole field repeats with the guns' period 30 (no debris left over).
#[test]
fn life_gate_truth_tables() {
    use computing::Gate;
    let cases: Vec<(Gate, bool, bool)> = [Gate::Not, Gate::And, Gate::Or]
        .into_iter()
        .flat_map(|g| {
            [(false, false), (false, true), (true, false), (true, true)]
                .into_iter()
                .filter(move |&(_, b)| g != Gate::Not || !b)
                .map(move |(a, b)| (g, a, b))
        })
        .collect();
    let bad: Vec<String> = cases
        .par_iter()
        .filter_map(|&(g, a, b)| {
            let lg = computing::life_gate(g, a, b).unwrap();
            let mut w = World::from_scene(&lg.scene).unwrap();
            w.apply(Command::Step { gens: lg.settle }).unwrap();
            let before = w.fields[0].state.clone();
            let out = lane_busy(&mut w, lg.detector, lg.width, 60);
            let periodic = w.fields[0].state == before;
            (out != g.eval(a, b) || !periodic).then(|| {
                format!(
                    "{} a={a} b={b}: output {out}, periodic {periodic}",
                    g.name()
                )
            })
        })
        .collect();
    assert!(bad.is_empty(), "gate errors:\n{}", bad.join("\n"));
}

/// The truth-table scenes: each frame's output lane carries gliders exactly for the 1 rows.
#[test]
fn life_truth_table_scenes() {
    use computing::Gate;
    for g in [Gate::Not, Gate::And, Gate::Or] {
        let (sc, dets) = computing::life_truth_table(g);
        let width = match sc.tiling.size {
            Some(SizeSpec::Rect { w, .. }) => w,
            _ => unreachable!(),
        };
        let mut w = World::from_scene(&sc).unwrap();
        w.apply(Command::Step {
            gens: computing::LIFE_SETTLE,
        })
        .unwrap();
        let mut seen = vec![false; dets.len()];
        for _ in 0..60 {
            w.step();
            for (k, (_, b)) in dets.iter().enumerate() {
                for y in b[1]..b[3] {
                    for x in b[0]..b[2] {
                        seen[k] |= w.fields[0].state[(y as u32 * width + x as u32) as usize] != 0;
                    }
                }
            }
        }
        let want: Vec<bool> = dets.iter().map(|((a, b), _)| g.eval(*a, *b)).collect();
        assert_eq!(seen, want, "{}", g.name());
    }
}

/// The Quick Start copies in `crates/jetcarp-core/data/scenes/` are exactly what the generators build.
#[test]
fn computing_scenes_match_generators() {
    use jetcarp_core::quickstart::COMPUTING_SCENES;
    let get = |id: &str| {
        let (_, json) = COMPUTING_SCENES.iter().find(|(k, _)| *k == id).unwrap();
        Scene::from_json(json).unwrap()
    };
    let same = |a: &Scene, b: &Scene| {
        a.name == b.name && a.description == b.description && a.tiling == b.tiling && a.cas == b.cas
    };
    assert!(
        same(
            &get("wireworld-adder"),
            &computing::wireworld_adder(2, 3).unwrap().scene
        ),
        "data/scenes/wireworld-adder.json is stale: jetcarp demo wireworld-adder --a 2 --b 3 --save ..."
    );
    for (id, g) in [
        ("life-not", computing::Gate::Not),
        ("life-and", computing::Gate::And),
        ("life-or", computing::Gate::Or),
    ] {
        assert!(
            same(&get(id), &computing::life_truth_table(g).0),
            "data/scenes/{id}.json is stale: jetcarp demo {id} --table --save ..."
        );
    }
}
