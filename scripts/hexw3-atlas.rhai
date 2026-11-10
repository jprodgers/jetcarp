// HexW3 atlas: every ternary hexagon elementary rule (HexW3 0 to HexW3 19682), each run from
// one cell at the top of a screen-sized hexagon grid down to the bottom row, one PNG per rule.
//
// Output: <Documents>/jetcarp-out/<date>_<time>_HexW3/HexW3-00000.png ... HexW3-19682.png,
// plus manifest.csv / manifest.json (one row per rule) and run.json (everything needed to
// reproduce any image).
//
// Run:   jetcarp script scripts/hexw3-atlas.rhai
//        jetcarp script scripts/hexw3-atlas.rhai --screen 2560x1440 --cell-px 6
//        jetcarp script scripts/hexw3-atlas.rhai --set from=8000 --set to=8999   (a slice)
// An interrupted atlas resumes with --set folder=<the output folder>.

let from = params.from ?? 0;
let to = params.to ?? 19682;

let summary = sweep(#{
    rules: "HexW3 {n}", from: from, to: to,
    tiling: "hex", size: "screen", boundary: "dead",
    seed: "top_center",
    until: "done", max_gens: 10000,   // done = the bottom row has been written
    palette: params.palette ?? "Ember",
    out: "HexW3",
    folder: params.folder,            // () unless resuming
});

print(`${summary.images} images in ${summary.folder}`);
