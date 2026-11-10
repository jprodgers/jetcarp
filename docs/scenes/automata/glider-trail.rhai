// Draw a mover with its trail. Used by docs/tools/render-figures.sh:
//   jetcarp script docs/scenes/automata/glider-trail.rhai --out <image folder> \
//     --set scene=<absolute path of a scene> --set gens=<generations> --set png=<file name>
// The scene's first automaton moves; a second, fixed automaton on its own field
// (B/S012345678/C3: nothing is born and everything survives) records every tile the mover
// has occupied, coloured by age, so older parts of the trail are darker.
let w = load_scene(params.scene);
let t = w.add_ca("B/S012345678/C3", #{ seed: "empty", palette: "Mono" });
let n = w.cells();
for g in 0..params.gens {
    let cells = [];
    for i in 0..n {
        if w.get_cell(i) != 0 { cells.push([i, 1]); }
    }
    if cells.len() > 0 { w.apply(#{ op: "set_cells", id: t, cells: cells }); }
    w.step();
}
w.save_png(params.png, #{ max_side: 640, outlines: true, compression: "high" });
