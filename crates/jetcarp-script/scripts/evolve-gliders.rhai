// Evolve glider rules (Edmond's method, arXiv:2609.22579) on a few tilings, then follow the
// published Spectre glider with the mover tracker.
//
// Each search writes its own folder (fitness.csv, champions.json, gliders.json, movers.json,
// run.json); verified gliders go to the pattern library: exact spaceships on lattices, movers
// verified to a distance on aperiodic patches. A summary CSV goes to this script's folder.
//
// Try it on scratch folders first:
//
// Run:   JETCARP_OUT=/tmp/jetcarp-out JETCARP_LIBRARY=/tmp/jetcarp-lib jetcarp script scripts/evolve-gliders.rhai
//        jetcarp script scripts/evolve-gliders.rhai --set gens=800 --set tilings=spectre,hat,penrose-rhomb
//        jetcarp script scripts/evolve-gliders.rhai --set seed=33 --set store=false

let tilings = if params.tilings != () { params.tilings.split(",") } else { ["spectre", "hat", "square"] };
let gens = params.gens ?? 200;
let pop = params.pop ?? 256;
let seed = params.seed ?? 1;
let store = params.store ?? true;

let rows = [];
for t in tilings {
    let r = evolve(#{
        tiling: t, states: 4, pop: pop, gens: gens, seed: seed,
        champions: 8, stop_on_glider: true, store: store,
    });
    print(`${t}: best ${r.best_ever} after ${r.generations} generations (${r.unique_evaluations} distinct genomes, ${r.elapsed} s); ${r.gliders.len()} verified glider rule(s)`);
    for m in r.movers {
        print(`  ${m.kind}: ${m.summary}`);
        print(`    ${m.rule}`);
    }
    rows.push(#{
        tiling: t, best: r.best_ever, generations: r.generations, gliders: r.gliders.len(),
        first: if r.gliders.len() > 0 { r.gliders[0] } else { "" },
        seconds: r.elapsed, folder: r.dir,
    });
}

// The published glider: Edmond's Spectre rule from a state-1 / state-3 pair, followed over
// Spectre levels 4, 5, 6.
let t = track_mover(#{ rule: "SpectreGlider", tiling: "spectre", level: 4, cells: [[2675, 1], [2676, 3]] });
print(`SpectreGlider from tiles 2675/2676: ${t.summary}`);

let dir = out_dir("evolve-gliders");
write_csv(join(dir, "summary.csv"), rows);
print(`summary: ${join(dir, "summary.csv")}`);
