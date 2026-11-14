// JETCARP GPU stepper. One invocation per cell; each cell is a u32: state in bits 0-7, JACA's
// annihilation glow in bits 8-15 (display only; rules other than JACA pass it through), age in
// bits 16-31.
// Every rule here mirrors jetcarp-core exactly (integer arithmetic only), so results are
// bit-identical to the CPU.

struct Params {
  op: u32, rule: u32, nb: u32, states: u32,
  birth: u32, survive: u32, thresh: u32, k1: u32,
  k2: u32, gg: u32, flags: u32, gen: u32,
  pop_mode: u32, pop_n: u32, act_mode: u32, zin: u32,
  zout: u32, halt_repeat: u32, compare: u32, stat: u32,
  // Priority rules: packed rows (jetcarp-gpu's `priority_words`), k1 words.
  table: array<vec4<u32>, 11>,
};

// Header (halted, halt generation, one streak per activity halt, padding), then per generation
// of the batch: population, changed cells, snapshot mismatch, unused.
// After MAX_BATCH generations, four words per statistics slot (see STAT_BASE).
struct Ctrl {
  halted: atomic<u32>,
  halt_gen: atomic<u32>,
  streak: array<atomic<u32>, 4>,
  pad0: atomic<u32>,
  pad1: atomic<u32>,
  m: array<atomic<u32>>,
};

// topo header: 0 n, 1 w, 2 h, 3 ntypes, 4 wrap, 5 wrap_shift, 6 explicit, 7 live edges,
// 8 groups per dispatch row, 9 row_start offset, 10 slot table offset, 11 offset list offset.
@group(0) @binding(0) var<storage, read> topo: array<i32>;
@group(0) @binding(1) var<storage, read> csr_off: array<u32>;
@group(0) @binding(2) var<storage, read> csr_nb: array<u32>;
@group(0) @binding(3) var<storage, read_write> ctrl: Ctrl;
@group(0) @binding(4) var<uniform> p: Params;
@group(1) @binding(0) var<storage, read> src: array<u32>;
@group(1) @binding(1) var<storage, read_write> dst: array<u32>;
@group(1) @binding(2) var<storage, read_write> snap: array<u32>;
// Per-rule data (see `prepare_aux` in lib.rs), then JACA's blast zone (two halves of n words,
// in and out), at offsets given in the parameters. One buffer, as devices may allow only eight
// storage buffers per shader stage.
@group(2) @binding(0) var<storage, read_write> aux: array<u32>;

var<workgroup> wg_halt: u32;
var<workgroup> wg_pop: atomic<u32>;
var<workgroup> wg_chg: atomic<u32>;
var<workgroup> wg_mis: atomic<u32>;
// Statistics: JACA's contacts, destroyed cells and cancelled births.
var<workgroup> wg_s0: atomic<u32>;
var<workgroup> wg_s1: atomic<u32>;
var<workgroup> wg_s2: atomic<u32>;

const WG: u32 = 256u;
// Where the statistics slots start in ctrl.m: after 4 words for each of MAX_BATCH generations.
const STAT_BASE: u32 = 4096u;
const NONE: u32 = 0xffffffffu;

// Lattice coordinates (unsheared i, row j, type k) to a cell index, honouring wrap; -1 past a dead edge.
fn resolve(i0: i32, j0: i32, k: i32) -> i32 {
  let w = topo[1];
  let h = topo[2];
  let nt = topo[3];
  var i = i0;
  var j = j0;
  if (topo[4] != 0) {
    if (j < 0 || j >= h) {
      var q = j / h;
      if (j - q * h < 0) { q = q - 1; }
      j = j - q * h;
      i = i - q * topo[5];
    }
    var u = i - topo[topo[9] + j];
    if (u < 0 || u >= w) {
      u = u % w;
      if (u < 0) { u = u + w; }
    }
    return (j * w + u) * nt + k;
  }
  if (j < 0 || j >= h) { return -1; }
  let u = i - topo[topo[9] + j];
  if (u < 0 || u >= w) { return -1; }
  return (j * w + u) * nt + k;
}

fn tally(v: u32, a: u32, b: u32) -> vec4<u32> {
  return vec4<u32>(select(0u, 1u, v == a), select(0u, 1u, v == b), v, 1u);
}

// Over the neighbours of cell i: (count of state a, count of state b, sum of states, present).
// `deg` receives the number of neighbour slots, including slots past a dead edge.
fn gather(i: u32, a: u32, b: u32, deg: ptr<function, u32>) -> vec4<u32> {
  var r = vec4<u32>(0u);
  let nb = p.nb;
  if (topo[6] != 0) {
    let row = nb * (u32(topo[0]) + 1u) + i;
    let lo = csr_off[row];
    let hi = csr_off[row + 1u];
    *deg = hi - lo;
    for (var e = lo; e < hi; e = e + 1u) {
      r += tally(src[csr_nb[e]] & 0xffu, a, b);
    }
    return r;
  }
  let nt = u32(topo[3]);
  let w = u32(topo[1]);
  let k = i % nt;
  let c = i / nt;
  let j = i32(c / w);
  let base = i32(c % w) + topo[topo[9] + j];
  let t = topo[10] + i32((nb * nt + k) * 2u);
  let start = topo[t];
  let cnt = topo[t + 1];
  *deg = u32(cnt);
  for (var o = 0; o < cnt; o = o + 1) {
    let e = topo[11] + (start + o) * 3;
    let x = resolve(base + topo[e], j + topo[e + 1], topo[e + 2]);
    if (x >= 0) {
      r += tally(src[u32(x)] & 0xffu, a, b);
    }
  }
  return r;
}

// Over the neighbours of cell i: how many are in each state (mod p.states), past a dead edge none.
fn counts(i: u32, c: ptr<function, array<u32, 8>>) {
  let nb = p.nb;
  let k = p.states;
  if (topo[6] != 0) {
    let row = nb * (u32(topo[0]) + 1u) + i;
    let lo = csr_off[row];
    let hi = csr_off[row + 1u];
    for (var e = lo; e < hi; e = e + 1u) {
      let v = (src[csr_nb[e]] & 0xffu) % k;
      (*c)[v] = (*c)[v] + 1u;
    }
    return;
  }
  let nt = u32(topo[3]);
  let w = u32(topo[1]);
  let t0 = i % nt;
  let cc = i / nt;
  let j = i32(cc / w);
  let base = i32(cc % w) + topo[topo[9] + j];
  let t = topo[10] + i32((nb * nt + t0) * 2u);
  let start = topo[t];
  let cnt = topo[t + 1];
  for (var o = 0; o < cnt; o = o + 1) {
    let e = topo[11] + (start + o) * 3;
    let x = resolve(base + topo[e], j + topo[e + 1], topo[e + 2]);
    if (x >= 0) {
      let v = (src[u32(x)] & 0xffu) % k;
      (*c)[v] = (*c)[v] + 1u;
    }
  }
}

// The neighbour slots of a cell (p.nb): a CSR range on explicit storage; on a lattice, the
// cell's position and its type's range of offsets.
struct Hood { lo: u32, cnt: u32, base: i32, j: i32 };

fn hood(i: u32) -> Hood {
  var h: Hood;
  if (topo[6] != 0) {
    let row = p.nb * (u32(topo[0]) + 1u) + i;
    h.lo = csr_off[row];
    h.cnt = csr_off[row + 1u] - h.lo;
    return h;
  }
  let nt = u32(topo[3]);
  let w = u32(topo[1]);
  let c = i / nt;
  h.j = i32(c / w);
  h.base = i32(c % w) + topo[topo[9] + h.j];
  let t = topo[10] + i32((p.nb * nt + i % nt) * 2u);
  h.lo = u32(topo[t]);
  h.cnt = u32(topo[t + 1]);
  return h;
}

// Slot o of a hood: the neighbour's index, or -1 past a dead edge.
fn nbr(h: Hood, o: u32) -> i32 {
  if (topo[6] != 0) { return i32(csr_nb[h.lo + o]); }
  let e = topo[11] + i32(h.lo + o) * 3;
  return resolve(h.base + topo[e], h.j + topo[e + 1], topo[e + 2]);
}

fn table_word(w: u32) -> u32 {
  return p.table[w / 4u][w % 4u];
}

// One packed condition (state in bits 0-3, 15 = none; minimum in bits 4-11).
fn holds(z: u32, c: ptr<function, array<u32, 8>>) -> bool {
  let st = z & 15u;
  if (st == 15u) { return true; }
  return (*c)[st] >= ((z >> 4u) & 255u);
}

// Ages for rules that commit a new state array (cyclic, Greenberg-Hastings, hodgepodge).
fn commit_age(old: u32, v: u32, age: u32) -> u32 {
  if (v == 0u) { return 0u; }
  if (v == old) { return min(age + 1u, 65000u); }
  return 1u;
}

// Ages for Life-like and Generations rules.
fn bs_age(s: u32, v: u32, age: u32) -> u32 {
  if (v == 1u) {
    if (s == 1u) { return min(age + 1u, 65000u); }
    return 1u;
  }
  if (v != 0u) { return age; }
  return 0u;
}

@compute @workgroup_size(256)
fn step(@builtin(workgroup_id) wg: vec3<u32>, @builtin(local_invocation_index) li: u32) {
  if (li == 0u) { wg_halt = atomicLoad(&ctrl.halted); }
  if (workgroupUniformLoad(&wg_halt) != 0u) { return; }
  let i = (wg.y * u32(topo[8]) + wg.x) * WG + li;
  let n = u32(topo[0]);
  if (i < n) {
    let old = src[i];
    let s = old & 0xffu;
    let age = old >> 16u;
    var glow = old & 0xff00u;
    var v = 0u;
    var na = 0u;
    var deg = 0u;
    switch p.rule {
      // Life-like and Generations: only state-1 neighbours count; live edges add missing slots.
      case 0u: {
        let g = gather(i, 1u, 1u, &deg);
        var c = g.x;
        if (topo[7] != 0) { c = c + deg - g.w; }
        if (s == 0u) {
          v = (p.birth >> c) & 1u;
        } else if (s == 1u) {
          if (((p.survive >> c) & 1u) == 1u) { v = 1u; } else if (p.states > 2u) { v = 2u; } else { v = 0u; }
        } else if (s + 1u < p.states) {
          v = s + 1u;
        } else {
          v = 0u;
        }
        na = bs_age(s, v, age);
      }
      // WireWorld: ages are left as they are.
      case 1u: {
        if (s == 1u) { v = 2u; }
        else if (s == 2u) { v = 3u; }
        else if (s == 3u) {
          let heads = gather(i, 1u, 1u, &deg).x;
          if (heads == 1u || heads == 2u) { v = 1u; } else { v = 3u; }
        } else { v = 0u; }
        na = age;
      }
      // Cyclic and rock-paper-scissors (threshold already +1 for RPS).
      case 2u: {
        var next = s + 1u;
        if (next >= p.states) { next = 0u; }
        let c = gather(i, next, next, &deg).x;
        if (c >= p.thresh) { v = next; } else { v = s; }
        na = commit_age(s, v, age);
      }
      // Greenberg-Hastings.
      case 3u: {
        if (s == 0u) {
          let c = gather(i, 1u, 1u, &deg).x;
          v = select(0u, 1u, c >= p.thresh);
        } else if (s + 1u >= p.states) {
          v = 0u;
        } else {
          v = s + 1u;
        }
        na = commit_age(s, v, age);
      }
      // Hodgepodge, states 0..=n with n in `states`.
      case 4u: {
        let g = gather(i, 0u, p.states, &deg);
        let ill = g.y;
        let inf = g.w - g.x - g.y;
        let sum = g.z + s;
        if (s == p.states) { v = 0u; }
        else if (s == 0u) { v = inf / p.k1 + ill / p.k2; }
        else { v = sum / (inf + ill + 1u) + p.gg; }
        v = min(v, p.states);
        na = commit_age(s, v, age);
      }
      // Edmond's Spectre glider rule.
      case 5u: {
        let g = gather(i, 1u, 3u, &deg);
        let c1 = g.x;
        let c3 = g.y;
        let c2 = (g.z - c1 - 3u * c3) / 2u;
        let c0 = g.w - c1 - c2 - c3;
        if (s == 0u && c0 >= 3u && c1 >= 1u) { v = 3u; }
        else if (s == 3u && c3 >= 3u) { v = 0u; }
        else if (s == 3u && c1 >= 1u) { v = 1u; }
        else { v = 0u; }
        na = select(0u, 1u, v != 0u);
      }
      // Priority table: the first row whose own state matches and whose conditions all hold
      // gives the next state, else 0 (evolve/priority.rs). Ages: 1 when non-zero.
      case 6u: {
        var c = array<u32, 8>(0u, 0u, 0u, 0u, 0u, 0u, 0u, 0u);
        counts(i, &c);
        let own = s % p.states;
        var w = 0u;
        loop {
          if (w >= p.k1) { break; }
          let x = table_word(w);
          let ro = x & 15u;
          var ok = ro == 8u || ro == own;
          var y = x;
          loop {
            ok = ok && holds(y >> 8u, &c) && holds(y >> 20u, &c);
            w = w + 1u;
            if ((y & 128u) == 0u) { break; }
            y = table_word(w);
          }
          if (ok) {
            v = (x >> 4u) & 7u;
            break;
          }
        }
        na = select(0u, 1u, v != 0u);
      }
      // JACA (jaca.rs): birth/survive = the positive rule, k1/k2 = the negative rule, thresh =
      // blast radius. With no blast the zone is the contact cells, found here; otherwise the
      // contact and dilate passes leave it in aux[zin..]. A destroyed cell glows 255, and the
      // glow halves each step.
      case 7u: {
        let h = hood(i);
        var cp = 0u;
        var cn = 0u;
        var present = 0u;
        for (var o = 0u; o < h.cnt; o = o + 1u) {
          let x = nbr(h, o);
          if (x >= 0) {
            let t = src[u32(x)] & 0xffu;
            cp += u32(t == 1u);
            cn += u32(t == 2u);
            present += 1u;
          }
        }
        var z = false;
        if (p.thresh == 0u) {
          if (s == 1u) { z = cn > 0u; } else if (s != 0u) { z = cp > 0u; }
          if (z) { atomicAdd(&wg_s0, 1u); }
        } else {
          z = aux[p.zin + i] != 0u;
        }
        if (z) {
          atomicAdd(&wg_s1, 1u);
          glow = 0xff00u;
        } else {
          glow = (glow >> 1u) & 0xff00u;
          if (topo[7] != 0) {
            cp += h.cnt - present;
            cn += h.cnt - present;
          }
          let sp = min(cp, 31u);
          let sn = min(cn, 31u);
          if (s == 1u) {
            v = (p.survive >> sp) & 1u;
          } else if (s == 2u) {
            v = ((p.k2 >> sn) & 1u) * 2u;
          } else {
            let bp = ((p.birth >> sp) & 1u) == 1u;
            let bn = ((p.k1 >> sn) & 1u) == 1u;
            if (bp && !bn) { v = 1u; }
            else if (bn && !bp) { v = 2u; }
            else if (bp && bn) { atomicAdd(&wg_s2, 1u); }
          }
        }
        na = commit_age(s, v, age);
      }
      default: {}
    }
    dst[i] = v | glow | (na << 16u);
    if ((p.flags & 1u) != 0u) {
      let sb = (snap[i >> 2u] >> ((i & 3u) * 8u)) & 0xffu;
      if (sb != v) { atomicStore(&wg_mis, 1u); }
    }
    if ((p.flags & 2u) != 0u && v != 0u) { atomicAdd(&wg_pop, 1u); }
    if ((p.flags & 4u) != 0u && v != s) { atomicAdd(&wg_chg, 1u); }
  }
  if (p.flags != 0u || p.stat != NONE) {
    workgroupBarrier();
    if (li == 0u) {
      let slot = 4u * p.gen;
      let pc = atomicLoad(&wg_pop);
      if (pc != 0u) { atomicAdd(&ctrl.m[slot], pc); }
      let cc = atomicLoad(&wg_chg);
      if (cc != 0u) { atomicAdd(&ctrl.m[slot + 1u], cc); }
      if (atomicLoad(&wg_mis) != 0u) { atomicOr(&ctrl.m[slot + 2u], 1u); }
      if (p.stat != NONE) {
        let at = STAT_BASE + 4u * p.stat;
        let s0 = atomicLoad(&wg_s0);
        let s1 = atomicLoad(&wg_s1);
        let s2 = atomicLoad(&wg_s2);
        if (s0 != 0u) { atomicAdd(&ctrl.m[at], s0); }
        if (s1 != 0u) { atomicAdd(&ctrl.m[at + 1u], s1); }
        if (s2 != 0u) { atomicAdd(&ctrl.m[at + 2u], s2); }
      }
    }
  }
}

// JACA with a blast radius, part 1: live cells touching the opposite sign, into the zone,
// counted in the step's statistics slot. The zone is aux[zout..].
@compute @workgroup_size(256)
fn contact(@builtin(workgroup_id) wg: vec3<u32>, @builtin(local_invocation_index) li: u32) {
  if (li == 0u) { wg_halt = atomicLoad(&ctrl.halted); }
  if (workgroupUniformLoad(&wg_halt) != 0u) { return; }
  let i = (wg.y * u32(topo[8]) + wg.x) * WG + li;
  let n = u32(topo[0]);
  if (i < n) {
    let s = src[i] & 0xffu;
    var hit = 0u;
    if (s != 0u) {
      let other = select(1u, 2u, s == 1u);
      let h = hood(i);
      for (var o = 0u; o < h.cnt; o = o + 1u) {
        let x = nbr(h, o);
        if (x >= 0 && (src[u32(x)] & 0xffu) == other) {
          hit = 1u;
          break;
        }
      }
    }
    aux[p.zout + i] = hit;
    if (hit != 0u) { atomicAdd(&wg_s0, 1u); }
  }
  workgroupBarrier();
  if (li == 0u) {
    let c = atomicLoad(&wg_s0);
    if (c != 0u) { atomicAdd(&ctrl.m[STAT_BASE + 4u * p.stat], c); }
  }
}

// JACA with a blast radius, part 2: grow the zone by one neighbourhood step, aux[zin..] into
// aux[zout..].
@compute @workgroup_size(256)
fn dilate(@builtin(workgroup_id) wg: vec3<u32>, @builtin(local_invocation_index) li: u32) {
  if (li == 0u) { wg_halt = atomicLoad(&ctrl.halted); }
  if (workgroupUniformLoad(&wg_halt) != 0u) { return; }
  let i = (wg.y * u32(topo[8]) + wg.x) * WG + li;
  let n = u32(topo[0]);
  if (i < n) {
    var hit = aux[p.zin + i];
    if (hit == 0u) {
      let h = hood(i);
      for (var o = 0u; o < h.cnt; o = o + 1u) {
        let x = nbr(h, o);
        if (x >= 0 && aux[p.zin + u32(x)] != 0u) {
          hit = 1u;
          break;
        }
      }
    }
    aux[p.zout + i] = hit;
  }
}

// Period-watch snapshot: the states of four cells per word, as bytes.
@compute @workgroup_size(256)
fn pack(@builtin(workgroup_id) wg: vec3<u32>, @builtin(local_invocation_index) li: u32) {
  if (atomicLoad(&ctrl.halted) != 0u) { return; }
  let wi = (wg.y * u32(topo[8]) + wg.x) * WG + li;
  let n = u32(topo[0]);
  let base = wi * 4u;
  if (base < n) {
    var x = 0u;
    for (var b = 0u; b < 4u; b = b + 1u) {
      if (base + b < n) { x = x | ((src[base + b] & 0xffu) << (8u * b)); }
    }
    snap[wi] = x;
  }
}

// After each generation: stop the batch when the engine must look (a script trigger or a
// reseed policy may fire). Control parameters: pop_mode bit 1 = stop when population <= pop_n,
// bit 2 = stop when population >= thresh; act_mode = number of activity halts, each in
// table[k] = (1 below / 2 at least, limit, generations in a row, unused) with its own streak.
@compute @workgroup_size(1)
fn control() {
  if (atomicLoad(&ctrl.halted) != 0u) { return; }
  let slot = 4u * p.gen;
  let pop = atomicLoad(&ctrl.m[slot]);
  let chg = atomicLoad(&ctrl.m[slot + 1u]);
  let mis = atomicLoad(&ctrl.m[slot + 2u]);
  var halt = p.halt_repeat != 0u && p.compare != 0u && mis == 0u;
  if ((p.pop_mode & 1u) != 0u && pop <= p.pop_n) { halt = true; }
  if ((p.pop_mode & 2u) != 0u && pop >= p.thresh) { halt = true; }
  for (var k = 0u; k < min(p.act_mode, 4u); k = k + 1u) {
    let a = p.table[k];
    var ok = chg >= a.y;
    if (a.x == 1u) { ok = chg < a.y; }
    var st = 0u;
    if (ok) { st = atomicLoad(&ctrl.streak[k]) + 1u; }
    atomicStore(&ctrl.streak[k], st);
    if (st >= a.z) { halt = true; }
  }
  if (halt) {
    atomicStore(&ctrl.halted, 1u);
    atomicStore(&ctrl.halt_gen, p.gen + 1u);
  }
}
