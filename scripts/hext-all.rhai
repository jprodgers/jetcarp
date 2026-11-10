// The three-parent hexagon rules on flat-top hexagons: every binary HexT rule (256) and every
// totalistic ternary rule (HexT3 sum, 2,187), each run from one cell in the middle of the top
// layer down to the bottom layer of a screen-sized grid. One PNG per rule plus manifest.csv
// (row_class: which rules settle into repeating or shifting rows; row_hint for the rest).
//
// Run:   jetcarp script scripts/hext-all.rhai
//        jetcarp script scripts/hext-all.rhai --set seed=top_random:0.5
//        jetcarp script scripts/hext-all.rhai --set ternary=false          (binary rules only)
//        jetcarp script scripts/hext-all.rhai --set sample=5000            (also a random sample
//                                                                         of the 3^27 HexT3 rules)

let seed = params.seed ?? "top_center";
let report = |s| {
    let classes = #{};
    for r in read_json(s.folder + "/manifest.json").rows {
        let c = r.row_class;
        if r.row_hint != () { c += "/" + r.row_hint; }
        classes[c] = (classes[c] ?? 0) + 1;
    }
    print(`${s.folder}: ${s.runs} runs in ${s.seconds} s; ${classes}`);
};

let a = sweep(#{
    rules: "HexT",                      // HexT 0 .. HexT 255
    tiling: "hex_flat", size: "screen", boundary: "dead",
    seed: seed, until: "done", palette: "Prism",
    out: "HexT",
});
report.call(a);

if params.ternary ?? true {
    let b = sweep(#{
        rules: "HexT3 sum",             // HexT3 sum 0 .. 2186
        tiling: "hex_flat", size: "screen", boundary: "dead",
        seed: seed, until: "done", palette: "Prism",
        out: "HexT3-sum",
    });
    report.call(b);
}

if params.sample != () {
    let c = sweep(#{
        rules: "HexT3", sample: params.sample, sample_seed: params.sample_seed ?? 1,
        tiling: "hex_flat", size: "screen", boundary: "dead",
        seed: seed, until: "done", palette: "Prism",
        out: "HexT3-sample",
    });
    report.call(c);
}
