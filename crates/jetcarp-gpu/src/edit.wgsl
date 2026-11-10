// Reseed edits on a resident field (jetcarp-core's `reseed::Edits`): one word per cell,
// `cell | op << 28`, each cell at most once. Ops: 0 birth (a dead cell becomes live), 1 kill,
// 2 live, 3 flip. Live is state 1, age 1 (state 2 when bit 31 is set: a -1 cell of a JACA
// field); dead is state 0, age 0. Cells are u32s with the state
// in bits 0-7 and the age in bits 16-31, as in sim.wgsl.

// Header: edit count, workgroups per dispatch row; then the edit words.
@group(0) @binding(0) var<storage, read> edits: array<u32>;
@group(1) @binding(1) var<storage, read_write> field: array<u32>;

@compute @workgroup_size(256)
fn edit(@builtin(workgroup_id) wg: vec3<u32>, @builtin(local_invocation_index) li: u32) {
  let k = (wg.y * edits[1] + wg.x) * 256u + li;
  if (k >= edits[0]) { return; }
  let e = edits[k + 2u];
  let c = e & 0x0fffffffu;
  let op = (e >> 28u) & 3u;
  let s = field[c] & 0xffu;
  var live = false;
  if (op == 0u) {
    if (s != 0u) { return; }
    live = true;
  } else if (op == 1u) {
    live = false;
  } else if (op == 2u) {
    live = true;
  } else {
    live = s == 0u;
  }
  let st = select(1u, 2u, (e & 0x80000000u) != 0u);
  field[c] = select(0u, st | (1u << 16u), live);
}
