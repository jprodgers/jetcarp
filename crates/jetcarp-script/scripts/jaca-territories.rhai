// JACA territories: how the two signs of a JACA world sort themselves out from a mixed soup,
// measured with each cell's real neighbours (w.neighbours, w.live_cells). One line per moment,
// and territories.csv with them.
//
// At each moment it counts:
//   balance   the minority sign's share of the live cells (0.5: even, 0: one sign holds all)
//   sorted    how much more often live neighbours share a sign than in a random mix of the same
//             balance (0: mixed, 1: clean territories)
//   fronts    live cells touching the other sign (they are destroyed next generation)
//   traded    of the cells live at the last moment and now, the share that changed sign
//
// Run:   jetcarp script scripts/jaca-territories.rhai
//        jetcarp script scripts/jaca-territories.rhai --set rule="JACA B3/S23" --set tiling=hex

let rule = params.rule ?? "JACA B27/S03456";
let tiling = params.tiling ?? "cairo";
let w = world(#{ tiling: tiling, size: [64, 48], boundary: "wrap", rng_seed: 28 });
w.add_ca(rule, #{ seed: "signed:0.45:0.5" });
let n = w.cells();

// Each cell's neighbours, as the rule counts them ("touch", or "edge" for a rule with a V),
// looked up once.
let kind = w.cas()[0].neighborhood;
let nb = [];
for i in 0..n {
    nb.push(w.neighbours(i, kind));
}

let rows = [];
let before = ();
for g in [0, 64, 256, 1024, 2048] {
    if g > w.generation() {
        w.step(g - w.generation());
    }
    // State per cell: 0 empty, 1 for +1, 2 for -1.
    let state = [];
    state.pad(n, 0);
    let plus = 0;
    let minus = 0;
    for c in w.live_cells() {
        state[c[0]] = c[1];
        if c[1] == 1 { plus += 1; } else { minus += 1; }
    }
    let live = plus + minus;
    // Pairs of live neighbours (each pair seen from both ends) and how many share a sign.
    let pairs = 0;
    let same = 0;
    let fronts = 0;
    for i in 0..n {
        let s = state[i];
        if s == 0 { continue; }
        let touches_other = false;
        for j in nb[i] {
            let t = state[j];
            if t == 0 { continue; }
            pairs += 1;
            if t == s { same += 1; } else { touches_other = true; }
        }
        if touches_other { fronts += 1; }
    }
    let balance = if live > 0 {
        (if plus < minus { plus } else { minus }).to_float() / live.to_float()
    } else { 0.0 };
    let sorted = 0.0;
    if pairs > 0 {
        let p = plus.to_float() / live.to_float();
        let mixed = p * p + (1.0 - p) * (1.0 - p);
        let seen = same.to_float() / pairs.to_float();
        sorted = if mixed < 0.999 { (seen - mixed) / (1.0 - mixed) } else { 1.0 };
    }
    let traded = ();
    if before != () {
        let both = 0;
        let flipped = 0;
        for i in 0..n {
            if before[i] != 0 && state[i] != 0 {
                both += 1;
                if before[i] != state[i] { flipped += 1; }
            }
        }
        traded = if both > 0 { flipped.to_float() / both.to_float() } else { 0.0 };
    }
    let r = #{ generation: g, plus: plus, minus: minus, balance: balance, sorted: sorted,
               fronts: fronts, traded: traded };
    rows.push(r);
    let t = if traded == () { "-" } else { `${(traded * 1000.0).round() / 1000.0}` };
    print(`generation ${g}: +1 ${plus}, -1 ${minus}, balance ${(balance * 1000.0).round() / 1000.0}, sorted ${(sorted * 1000.0).round() / 1000.0}, fronts ${fronts}, traded ${t}`);
    before = state;
}

let out = out_dir("jaca-territories");
write_csv(`${out}/territories.csv`, rows);
w.save_png(`${out}/final.png`);
print(`${rule} on ${tiling}, ${n} cells: ${out}`);
