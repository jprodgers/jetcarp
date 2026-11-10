// Soup hunt: run random soups of a few Life-like rules until they settle, find the oscillators
// and spaceships in what is left (objects()), and keep a picture of every soup that produced
// something other than still lifes and blinkers. Writes finds.csv (one row per object type
// found) and soups.csv (one row per soup) into a new output folder.
//
// Run:   jetcarp script scripts/soup-hunt.rhai
//        jetcarp script scripts/soup-hunt.rhai --set soups=40 --set size=128
//        jetcarp script scripts/soup-hunt.rhai --set rule=B36/S23 --set soups=100

let rules = if params.rule != () { [params.rule] } else { ["B3/S23", "B36/S23", "B3/S238", "B38/S23"] };
let soups = params.soups ?? 12;
let size = params.size ?? 96;
let out = out_dir("soup-hunt");

let finds = [];
let soup_rows = [];
let done = 0;
for rule in rules {
    for s in 0..soups {
        let w = world(#{ tiling: "square", size: [size, size], boundary: "wrap", rng_seed: 1000 + s });
        w.add_ca(rule, #{ seed: "random:0.35" });
        let r = w.run(#{ until: "repeats", max_period: 100, max_gens: 6000 });
        let o = w.objects(#{ max_period: 100 });
        let rare = 0;
        for c in o.counts {
            let common = c.kind == "still_life" || (c.kind == "oscillator" && c.period <= 2);
            if !common && c.kind != "other" {
                rare += c.count;
                finds.push(#{ rule: rule, soup: s, name: c.label, kind: c.kind,
                              period: c.period, speed: c.speed, count: c.count,
                              generation: w.generation() });
            }
        }
        let name = `${rule}-soup${s}`;
        name.replace("/", "_");
        if rare > 0 {
            w.save_png(out + "/" + name + ".png", #{ size: [800, 800], outlines: true });
            print(`${rule} soup ${s}: ${o.summary}`);
        }
        soup_rows.push(#{ rule: rule, soup: s, generations: r.generations, settled: r.stopped == "until",
                          period: w.period(), population: w.population(),
                          still_lifes: o.totals.still_life, oscillators: o.totals.oscillator,
                          spaceships: o.totals.spaceship, other: o.totals.other, rare: rare });
        done += 1;
        progress(done, rules.len() * soups, name);
    }
}
write_csv(out + "/finds.csv", finds);
write_csv(out + "/soups.csv", soup_rows);
print(`${finds.len()} rare object types in ${done} soups -> ${out}`);
