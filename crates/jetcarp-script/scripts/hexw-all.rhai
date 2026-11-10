// All 16 binary hexagon elementary rules (HexW 0 to HexW 15), each run from one cell at the top
// of a screen-sized hexagon grid to the bottom row, one PNG per rule (HexW-00.png ... HexW-15.png),
// plus manifest.csv / manifest.json and run.json.
//
// Run:   jetcarp script scripts/hexw-all.rhai
//        jetcarp script scripts/hexw-all.rhai --set seed=top_random:0.5     (random top row)
//        jetcarp script scripts/hexw-all.rhai --set boundary=wrap           (wrapped sides)

sweep(#{
    rules: "HexW",                      // the named series: HexW 0 .. HexW 15
    tiling: "hex", size: "screen",
    boundary: params.boundary ?? "dead",
    seed: params.seed ?? "top_center",
    rng_seed: 1,
    until: "done",                      // stop when the bottom row is written
    palette: "Tide",
    out: "HexW",
});
