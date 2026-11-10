// JETCARP GPU batch stepper: many worlds side by side in one state buffer, each with its own rule.
//
// A cell is one u32 holding its state. World w's cells start at w * cfg.stride; the stride is the
// topology's cell count rounded up to a multiple of the workgroup size, so a workgroup belongs to
// exactly one world, and the padding cells at the end of a world are skipped. Dispatches are
// (gx, gy, worlds): a workgroup's world is workgroup_id.z, which keeps per-world data uniform
// over the workgroup. Every rule mirrors jetcarp-core exactly (integer arithmetic only).

struct Cfg {
  n: u32, stride: u32, gx: u32, worlds: u32,
  live: u32, pad0: u32, pad1: u32, pad2: u32,
};

// One entry per dispatch: the trace slot, the first world, the dilation pass, the 1-based step
// number (used by B0 rules).
struct Disp { slot: u32, world_base: u32, phase: u32, tick: u32 };

// The meaning of a to d depends on `kind` (see batch.rs). d is the CSR index everywhere.
struct WorldParams {
  kind: u32, states: u32, birth: u32, survive: u32,
  a: u32, b: u32, c: u32, d: u32,
};

@group(0) @binding(0) var<uniform> cfg: Cfg;
@group(0) @binding(1) var<uniform> disp: Disp;
// topo[csr * n + i] = the offset in topo of cell i's record: [degree | missing << 16, delta...],
// each delta being neighbour - cell, wrapping. `missing` counts neighbour slots past a dead edge,
// which the list leaves out. Records are shared between cells (see intern_records in batch.rs).
@group(0) @binding(2) var<storage, read> topo: array<u32>;
@group(0) @binding(3) var<storage, read> worlds: array<WorldParams>;
@group(0) @binding(4) var<storage, read> tables: array<u32>;
// Per generation slot and world: population, changed, the hash's sum of state * mix(cell).
@group(0) @binding(5) var<storage, read_write> trace: array<atomic<u32>>;
@group(1) @binding(0) var<storage, read> src: array<u32>;
@group(1) @binding(1) var<storage, read_write> dst: array<u32>;
@group(2) @binding(0) var<storage, read> zone_in: array<u32>;
@group(2) @binding(1) var<storage, read_write> zone_out: array<u32>;

var<workgroup> wg_counts: atomic<u32>;
var<workgroup> wg_hash: atomic<u32>;

const WG: u32 = 256u;
const KIND_BS: u32 = 0u;
const KIND_WIRE: u32 = 1u;
const KIND_CYCLIC: u32 = 2u;
const KIND_GH: u32 = 3u;
const KIND_HODGEPODGE: u32 = 4u;
const KIND_GLIDER: u32 = 5u;
const KIND_PRIORITY: u32 = 6u;
const KIND_LTL: u32 = 7u;
const KIND_JACA: u32 = 8u;
const KIND_B0: u32 = 9u;

fn mix_cell(i: u32) -> u32 {
  return ((i * 0x9E3779B1u) ^ (i >> 7u)) | 1u;
}

// One packed priority condition (state in bits 0-3, 15 = none; minimum in bits 4-11).
fn holds(z: u32, c: ptr<function, array<u32, 8>>) -> bool {
  let st = z & 15u;
  if (st == 15u) { return true; }
  return (*c)[st] >= ((z >> 4u) & 255u);
}

// B0: the low d + 1 bits of m, reversed.
fn rev_mask(m: u32, d: u32) -> u32 {
  return reverseBits(m) >> (31u - d);
}

@compute @workgroup_size(256)
fn step(@builtin(workgroup_id) wg: vec3<u32>, @builtin(local_invocation_index) li: u32) {
  let world = disp.world_base + wg.z;
  let cell = (wg.y * cfg.gx + wg.x) * WG + li;
  if (cell < cfg.n) {
    let base = world * cfg.stride;
    let gi = base + cell;
    let s = src[gi];
    let p = &worlds[world];
    let kind = (*p).kind;
    let rec = topo[(*p).d * cfg.n + cell];
    let m = topo[rec];
    let lo = rec + 1u;
    let hi = lo + (m & 0xffffu);
    let missing = m >> 16u;
    var v = 0u;
    switch kind {
      // Life-like and Generations: only state-1 neighbours count; live edges add missing slots.
      case KIND_BS: {
        var c = 0u;
        for (var e = lo; e < hi; e += 1u) { c += u32(src[gi + topo[e]] == 1u); }
        if (cfg.live != 0u) { c += missing; }
        if (s == 0u) {
          v = ((*p).birth >> (c & 31u)) & 1u;
        } else if (s == 1u) {
          if ((((*p).survive >> (c & 31u)) & 1u) == 1u) { v = 1u; } else if ((*p).states > 2u) { v = 2u; }
        } else if (s + 1u < (*p).states) {
          v = s + 1u;
        }
      }
      case KIND_WIRE: {
        if (s == 1u) { v = 2u; }
        else if (s == 2u) { v = 3u; }
        else if (s == 3u) {
          var heads = 0u;
          for (var e = lo; e < hi; e += 1u) { heads += u32(src[gi + topo[e]] == 1u); }
          v = select(3u, 1u, heads == 1u || heads == 2u);
        }
      }
      // Cyclic and rock-paper-scissors (a is the threshold, already +1 for RPS).
      case KIND_CYCLIC: {
        var next = s + 1u;
        if (next >= (*p).states) { next = 0u; }
        var c = 0u;
        for (var e = lo; e < hi; e += 1u) { c += u32(src[gi + topo[e]] == next); }
        v = select(s, next, c >= (*p).a);
      }
      // Greenberg-Hastings.
      case KIND_GH: {
        if (s == 0u) {
          var c = 0u;
          for (var e = lo; e < hi; e += 1u) { c += u32(src[gi + topo[e]] == 1u); }
          v = select(0u, 1u, c >= (*p).a);
        } else if (s + 1u >= (*p).states) {
          v = 0u;
        } else {
          v = s + 1u;
        }
      }
      // Hodgepodge, states 0..=n with n in `states`; a = k1, b = k2, c = g.
      case KIND_HODGEPODGE: {
        var inf = 0u;
        var ill = 0u;
        var sum = s;
        for (var e = lo; e < hi; e += 1u) {
          let x = src[gi + topo[e]];
          sum += x;
          if (x == (*p).states) { ill += 1u; } else if (x > 0u) { inf += 1u; }
        }
        if (s == (*p).states) { v = 0u; }
        else if (s == 0u) { v = inf / (*p).a + ill / (*p).b; }
        else { v = sum / (inf + ill + 1u) + (*p).c; }
        v = min(v, (*p).states);
      }
      case KIND_GLIDER: {
        var c0 = 0u;
        var c1 = 0u;
        var c3 = 0u;
        for (var e = lo; e < hi; e += 1u) {
          let x = src[gi + topo[e]] & 3u;
          c0 += u32(x == 0u);
          c1 += u32(x == 1u);
          c3 += u32(x == 3u);
        }
        if (s == 0u && c0 >= 3u && c1 >= 1u) { v = 3u; }
        else if (s == 3u && c3 >= 3u) { v = 0u; }
        else if (s == 3u && c1 >= 1u) { v = 1u; }
      }
      // Priority table: tables[a..a + b] (see priority_words in lib.rs); the first row that
      // fires gives the next state, else 0.
      case KIND_PRIORITY: {
        var c = array<u32, 8>(0u, 0u, 0u, 0u, 0u, 0u, 0u, 0u);
        let k = (*p).states;
        for (var e = lo; e < hi; e += 1u) {
          let x = src[gi + topo[e]] % k;
          c[x] += 1u;
        }
        let own = s % k;
        var w = 0u;
        loop {
          if (w >= (*p).b) { break; }
          let x = tables[(*p).a + w];
          let ro = x & 15u;
          var ok = ro == 8u || ro == own;
          var y = x;
          loop {
            ok = ok && holds(y >> 8u, &c) && holds(y >> 20u, &c);
            w += 1u;
            if ((y & 128u) == 0u) { break; }
            y = tables[(*p).a + w];
          }
          if (ok) {
            v = (x >> 4u) & 7u;
            break;
          }
        }
      }
      // Larger than Life: birth bits at tables[a..], survive bits at tables[a + b..] (b words each).
      case KIND_LTL: {
        if (s > 1u) {
          if (s + 1u < (*p).states) { v = s + 1u; }
        } else {
          var c = 0u;
          for (var e = lo; e < hi; e += 1u) { c += u32(src[gi + topo[e]] == 1u); }
          if (cfg.live != 0u) { c += missing; }
          let off = (*p).a + select(0u, (*p).b, s == 1u);
          let bit = (tables[off + (c >> 5u)] >> (c & 31u)) & 1u;
          if (s == 0u) {
            v = bit;
          } else if (bit == 1u) {
            v = 1u;
          } else if ((*p).states > 2u) {
            v = 2u;
          }
        }
      }
      // JACA: birth/survive = positive rule, a/b = negative rule, c = blast radius. Cells are 0
      // empty, 1 positive, 2 negative. With no blast the zone is decided here; otherwise it comes
      // from the contact and dilate passes.
      case KIND_JACA: {
        var cp = 0u;
        var cn = 0u;
        for (var e = lo; e < hi; e += 1u) {
          let x = src[gi + topo[e]];
          cp += u32(x == 1u);
          cn += u32(x == 2u);
        }
        var z = false;
        if ((*p).c == 0u) {
          if (s == 1u) { z = cn > 0u; } else if (s != 0u) { z = cp > 0u; }
        } else {
          z = zone_in[gi] != 0u;
        }
        if (!z) {
          if (cfg.live != 0u) {
            cp += missing;
            cn += missing;
          }
          let sp = min(cp, 31u);
          let sn = min(cn, 31u);
          if (s == 1u) {
            v = (((*p).survive >> sp) & 1u) * 1u;
          } else if (s == 2u) {
            v = (((*p).b >> sn) & 1u) * 2u;
          } else {
            let bp = (((*p).birth >> sp) & 1u) == 1u;
            let bn = (((*p).a >> sn) & 1u) == 1u;
            if (bp && !bn) { v = 1u; }
            else if (bn && !bp) { v = 2u; }
          }
        }
      }
      // B0 Life-like: masks per degree, alternating with the step number (step_b0 in step.rs).
      default: {
        var c = 0u;
        for (var e = lo; e < hi; e += 1u) { c += u32(src[gi + topo[e]] == 1u); }
        let d = min((m & 0xffffu) + missing, 31u);
        let full = select((2u << d) - 1u, 0xffffffffu, d >= 31u);
        let bb = (*p).birth & full;
        let ss = (*p).survive & full;
        var bm = 0u;
        var sm = 0u;
        if ((((*p).survive >> d) & 1u) == 1u) {
          if (disp.tick == 1u) {
            bm = ~(*p).birth & full;
            sm = ~(*p).survive & full;
          } else {
            bm = rev_mask(~(*p).survive & full, d);
            sm = rev_mask(~(*p).birth & full, d);
          }
        } else if ((disp.tick & 1u) == 1u) {
          bm = ~(*p).birth & full;
          sm = ~(*p).survive & full;
        } else {
          bm = rev_mask(ss, d);
          sm = rev_mask(bb, d);
        }
        if (s == 0u) { v = (bm >> (c & 31u)) & 1u; } else { v = (sm >> (c & 31u)) & 1u; }
      }
    }
    dst[gi] = v;
    if (v != 0u || v != s) {
      atomicAdd(&wg_counts, u32(v != 0u) | (u32(v != s) << 16u));
    }
    if (v != 0u) {
      atomicAdd(&wg_hash, v * mix_cell(cell));
    }
  }
  workgroupBarrier();
  if (li == 0u) {
    let at = (disp.slot * cfg.worlds + world) * 3u;
    let c = atomicLoad(&wg_counts);
    if (c != 0u) {
      atomicAdd(&trace[at], c & 0xffffu);
      if ((c >> 16u) != 0u) { atomicAdd(&trace[at + 1u], c >> 16u); }
    }
    let h = atomicLoad(&wg_hash);
    if (h != 0u) { atomicAdd(&trace[at + 2u], h); }
  }
}

// JACA with a blast radius, part 1: the cells touching the opposite sign.
@compute @workgroup_size(256)
fn contact(@builtin(workgroup_id) wg: vec3<u32>, @builtin(local_invocation_index) li: u32) {
  let world = disp.world_base + wg.z;
  let p = &worlds[world];
  if ((*p).kind != KIND_JACA || (*p).c == 0u) { return; }
  let cell = (wg.y * cfg.gx + wg.x) * WG + li;
  if (cell >= cfg.n) { return; }
  let base = world * cfg.stride;
  let gi = base + cell;
  let s = src[gi];
  var hit = false;
  if (s != 0u) {
    let other = select(1u, 2u, s == 1u);
    let rec = topo[(*p).d * cfg.n + cell];
    let lo = rec + 1u;
    let hi = lo + (topo[rec] & 0xffffu);
    for (var e = lo; e < hi; e += 1u) {
      if (src[gi + topo[e]] == other) { hit = true; break; }
    }
  }
  zone_out[gi] = u32(hit);
}

// JACA with a blast radius, part 2: grow the zone by one neighbourhood step. Worlds whose radius
// is smaller than this pass just copy their zone through, so every world ends up with its own
// radius in the buffer the step pass reads.
@compute @workgroup_size(256)
fn dilate(@builtin(workgroup_id) wg: vec3<u32>, @builtin(local_invocation_index) li: u32) {
  let world = disp.world_base + wg.z;
  let p = &worlds[world];
  if ((*p).kind != KIND_JACA || (*p).c == 0u) { return; }
  let cell = (wg.y * cfg.gx + wg.x) * WG + li;
  if (cell >= cfg.n) { return; }
  let base = world * cfg.stride;
  let gi = base + cell;
  var hit = zone_in[gi];
  if (hit == 0u && disp.phase < (*p).c) {
    let rec = topo[(*p).d * cfg.n + cell];
    let lo = rec + 1u;
    let hi = lo + (topo[rec] & 0xffffu);
    for (var e = lo; e < hi; e += 1u) {
      hit |= zone_in[gi + topo[e]];
    }
  }
  zone_out[gi] = hit;
}
