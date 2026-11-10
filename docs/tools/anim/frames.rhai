// Render frames of a scene at given generations (ascending).
// params: scene, gens ("0,4,8,..."), max_side, outlines (true/false), bg (true/false)
// optional: sides ("800,900,..." one max_side per frame), outline_min (tile px; with sides,
//           outlines only on frames whose tile size is at least this), tile_ref (tile px at max_side)
let w = load_scene(params.scene);
w.track_changes(true);
let ms = params.max_side;
let outl = params.outlines;
let gl = params.gens.to_string().split(",");
let sl = if "sides" in params { params.sides.to_string().split(",") } else { [] };
if params.bg {
  let b = w.copy();
  b.clear();
  b.save_png("bg.png", #{ max_side: ms, outlines: outl, compression: "fast" });
}
let i = 0;
let pops = [];
let done_at = ();
for gs in gl {
  let g = parse_int(gs);
  let d = g - w.generation();
  if d > 0 && done_at == () {
    w.step(d);
    if w.done() { done_at = w.generation(); }
  }
  let side = ms;
  let o = outl;
  if sl.len() > 0 {
    side = parse_int(sl[i]);
    if "outline_min" in params {
      o = (params.tile_ref * side / ms) >= params.outline_min;
    }
  }
  let name = if i < 10 { `f00${i}.png` } else if i < 100 { `f0${i}.png` } else { `f${i}.png` };
  w.save_png(name, #{ max_side: side, outlines: o, compression: "fast" });
  pops.push(w.population());
  i += 1;
}
write_json("info.json", #{ status: w.status(), pops: pops, done_at: done_at, done_reason: w.done_reason() });
