// Catalog hunt: run random soups of a few rules until they settle, catalog every still life,
// oscillator, and spaceship left in them into the pattern library (each one verified exactly;
// copies only raise its seen count), then promote the best-scoring new spaceship to the Quick
// Start ("My finds").
//
// The catalog is JETCARP_LIBRARY if set, else <Documents>/jetcarp-out/library.sqlite. Try it on a scratch
// library first:
//
// Run:   JETCARP_LIBRARY=/tmp/jetcarp-lib jetcarp script scripts/catalog-hunt.rhai
//        JETCARP_LIBRARY=/tmp/jetcarp-lib jetcarp script scripts/catalog-hunt.rhai --set soups=40 --set size=128
//        jetcarp script scripts/catalog-hunt.rhai --set rule=B36/S23 --set promote=false

let rules = if params.rule != () { [params.rule] } else { ["B3/S23", "B36/S23", "B3/S238"] };
let soups = params.soups ?? 8;
let size = params.size ?? 96;
let do_promote = params.promote ?? true;

let lib = library();
print(`library: ${lib.path()} (${lib.count()} patterns)`);

let added = [];
let objects = 0;
let done = 0;
for rule in rules {
    for s in 0..soups {
        let w = world(#{ tiling: "square", size: [size, size], boundary: "wrap", rng_seed: 5000 + s });
        w.add_ca(rule, #{ seed: "random:0.35" });
        // Catalog every 800 generations until the soup settles: spaceships on a wrapped grid
        // tend to crash into debris later, so catch them while they fly.
        for k in 0..8 {
            let run = w.run(#{ until: "repeats", max_period: 100, max_gens: 800 });
            let r = lib.catalog(w, #{ source: `catalog-hunt ${rule} soup ${s}` });
            objects += r.objects;
            for id in r.added {
                added.push(id);
            }
            if run.stopped == "until" { break; }
        }
        done += 1;
        progress(done, rules.len() * soups, `${rule} soup ${s}`);
    }
}
print(`${objects} objects, ${added.len()} new patterns; the library now holds ${lib.count()}`);

// The best new spaceship found by this run (query results are sorted by score).
let best = ();
for r in lib.query(#{ class: "spaceship", state: "new" }) {
    if added.contains(r.id) {
        best = r;
        break;
    }
}
if best == () {
    print("no new spaceship this time; try more soups (--set soups=40)");
} else {
    print(`best new spaceship: ${best.label} in ${best.rule}, ${best.speed}, score ${best.score}`);
    if do_promote {
        let p = lib.promote(best.id, `${best.rule} ${best.label}`);
        lib.update(best.id, #{ add_tags: ["catalog-hunt"] });
        print(`promoted to the Quick Start: ${p.path}`);
    }
}
