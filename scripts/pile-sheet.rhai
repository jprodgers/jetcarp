// The same sandpile on every kind of grid: N grains dropped on one cell of an unbounded grid,
// settled, and drawn; then a contact sheet of the previews (sheet.png) and piles.json.
//
// Run:   jetcarp script scripts/pile-sheet.rhai
//        jetcarp script scripts/pile-sheet.rhai --set grains=1e6 --set nb=touch --set gpu=true
//
// Each pile also gets its own folder (pile.png, preview.png, pile.json, growth.csv, steps/).

let grains = params.grains ?? 100000;
let nb = params.nb ?? "edge";
let tilings = [
    // Regular and Archimedean.
    "square", "hex", "triangle", "trihex", "snubsq", "oct", "rhombitrihex", "trunctrihex",
    "snubhex", "elongtri", "trunchex",
    // Others: Cairo pentagons, a 3-uniform tiling, an isohedral type, and the Spectre.
    "cairo", "k3-012", "ih41", "spectre",
];
let s = sandpile_piles(#{
    tilings: tilings,
    grains: grains,
    neighborhood: nb,
    thumb: 360,
    cols: 5,
    png: params.png ?? false,
    gpu: params.gpu ?? false,
});
print(`sheet: ${s.sheet}`);
for r in s.runs {
    print(`${r.tiling}: ${r.topplings} topplings, ${r.sweeps} sweeps, ${r.seconds} s`);
}
