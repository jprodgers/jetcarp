// Thumbnails for the tiling catalogue (docs/src/tilings/catalogue.md).
// Run by docs/tools/render-figures.sh as:
//   jetcarp script docs/tools/tiling-thumbs.rhai --out docs/src/images/tilings/thumbs
// Each thumbnail is a small patch with every tile in a random colour (a five-state field seeded with
// random states), drawn with outlines, so the shapes of the tiles stand out.

fn thumb(id) {
    let aperiodic = id == "hat" || id == "spectre" || id.starts_with("penrose");
    let w = if aperiodic {
        world(#{ tiling: id, size: #{ level: 2 }, boundary: "dead", rng_seed: 7 })
    } else {
        world(#{ tiling: id, size: [8, 8], boundary: "dead", rng_seed: 7 })
    };
    w.add_ca("B3/S23/C5", #{ seed: "random_states", palette: "Paper" });
    w.save_png(id + ".png", #{ size: [180, 135], outlines: true, compression: "high" })
}

let ids = tilings();
ids += isohedral_tilings();
for k in [1, 2, 3] {
    for id in catalog_tilings(k) {
        if id.starts_with("k") { ids.push(id); }
    }
}
let bytes = 0;
for id in ids {
    let r = thumb(id);
    bytes += r.bytes;
}
print(`${ids.len()} thumbnails, ${bytes / 1024} KiB`);
