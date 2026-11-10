// The five-stage sequence of scenes/script-five.json, written as a script: on one shared field,
// Rule 30 fills the grid, Day & Night reshapes it until it calms down, Seeds bursts it apart,
// Life settles it, then three Langton's ants wander the ash. Each stage runs the same
// generations as the JSON scene's script; a picture is saved after every stage.
//
// Run:   jetcarp script scripts/script-five.rhai
//        jetcarp script scripts/script-five.rhai --set ant_gens=2000

let w = world(#{ tiling: "square", size: [200, 150], boundary: "dead", rng_seed: 5 });
let rule30 = w.add_ca("W30", #{ seed: "top_center" });
let day_night = w.add_ca("B3678/S34678");
let seeds = w.add_ca("B2/S");
let life = w.add_ca("B3/S23");
let ants = w.add_ca("Ant RL", #{ steps_per_gen: 100, seed: "ants:3" });

// Switch on only one CA, like a JSON script step.
fn only(w, id) {
    for c in w.cas() {
        w.update_ca(c.id, #{ enabled: c.id == id });
    }
}

let out = out_dir("script-five");
let stages = [
    [rule30, "Rule 30 fills the grid", #{ gens: 149 }],
    [day_night, "Day & Night until it calms",
        #{ until: "activity_below", fraction: 0.002, for_gens: 20, max_gens: 400 }],
    [seeds, "Seeds bursts it", #{ gens: 12 }],
    [life, "Life settles", #{ until: "repeats", max_period: 6, max_gens: 2000 }],
    [ants, "Ants", #{ gens: params.ant_gens ?? 600 }],
];
let k = 1;
for st in stages {
    only(w, st[0]);
    let r = w.run(st[2]);
    print(`stage ${k}: ${st[1]}: ${r.generations} generations (${r.stopped}), now at ${w.generation()}, ${w.population()} live`);
    w.save_png(`${out}/stage-${k}.png`);
    k += 1;
}
w.save_scene(out + "/final.json");
