// All 256 elementary (Wolfram) rules on a screen-sized square grid, each run from one cell in the
// middle of the top row down to the bottom row: W000.png ... W255.png, plus manifest.csv
// (with row_period / row_class columns: which rules settle into repeating rows) and run.json.
//
// Run:   jetcarp script scripts/wolfram-all.rhai
//        jetcarp script scripts/wolfram-all.rhai --cell-px 4                (finer cells)
//        jetcarp script scripts/wolfram-all.rhai --set seed=top_random:0.5

let s = sweep(#{
    rules: "W",                         // W0 .. W255
    tiling: "square", size: "screen", boundary: "dead",
    seed: params.seed ?? "top_center",
    until: "done",
    palette: "Ink",
    out: "Wolfram",
});

// A short report from the manifest: how many rules end in repeating rows.
let rows = read_json(s.folder + "/manifest.json").rows;
let classes = #{};
for r in rows {
    let c = r.row_class;
    classes[c] = (classes[c] ?? 0) + 1;
}
print(`row classes: ${classes}`);
