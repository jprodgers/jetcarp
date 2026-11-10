// Large sandpiles on the GPU: one synchronous toppling sweep over a lattice board.
//
// Heights are bytes, four to a word along a row (the CPU board's layout, padding included).
// Each invocation updates one word: a byte topples when it is at least its tile type's degree
// d, keeping h - d, and gains one grain per toppling neighbour. Heights stay below 2d <= 128, so
// the per-byte arithmetic below never carries between bytes.

struct Params {
    wr: u32,      // words per physical row
    ph: u32,      // physical rows per plane
    lw: u32,      // logical words per row
    lh: u32,      // logical rows per plane
    nt: u32,      // tile types (planes)
    slot: u32,    // sweep index within the batch (stats slot)
    band: u32,    // cells near the logical edge where any grain raises the edge flag
    ystride: u32, // rows per dispatch z layer
    x0: u32,      // the region swept: first logical word, words, first logical row, rows
    rw: u32,
    j0: u32,
    rh: u32,
    nx0: u32,     // the known nonzero box (words nx0..nx1, rows nj0..nj1): only cells
    nx1: u32,     // outside it can grow the box or reach the edge band
    nj0: u32,
    nj1: u32,
}

struct Off {
    q: i32,   // word offset along the row (floor(di / 4))
    s: u32,   // byte shift (di mod 4)
    dj: i32,  // row offset
    k: u32,   // neighbour's tile type
}

@group(0) @binding(0) var<storage, read> src: array<u32>;
@group(0) @binding(1) var<storage, read_write> dst: array<u32>;
@group(0) @binding(2) var<storage, read> offs: array<Off>;
// Per tile type: first offset, offset count, degree, unused.
@group(0) @binding(3) var<storage, read> tinfo: array<vec4<u32>>;
// [0]: edge flag; [1..5]: min word, max word, min row, max row of nonzero cells;
// [5 + s]: topplings in sweep s of the batch.
@group(0) @binding(4) var<storage, read_write> stats: array<atomic<u32>>;
@group(0) @binding(5) var<uniform> p: Params;

const PADW: u32 = 2u; // 8 padding bytes on each side of a row
const PADR: u32 = 8u; // 8 padding rows above and below

var<workgroup> wsum: atomic<u32>;
var<workgroup> wbox: array<atomic<u32>, 4>;

// 1 in each byte of x that is >= d (every byte of x below 128, d below 128).
fn fires(x: u32, d: u32) -> u32 {
    return (((x | 0x80808080u) - d * 0x01010101u) & 0x80808080u) >> 7u;
}

@compute @workgroup_size(64, 1, 1)
fn sweep(@builtin(global_invocation_id) g: vec3<u32>, @builtin(local_invocation_index) li: u32) {
    if (li == 0u) {
        atomicStore(&wsum, 0u);
        atomicStore(&wbox[0], 0xffffffffu);
        atomicStore(&wbox[1], 0u);
        atomicStore(&wbox[2], 0xffffffffu);
        atomicStore(&wbox[3], 0u);
    }
    workgroupBarrier();
    let x = p.x0 + g.x;
    let row = g.z * p.ystride + g.y;
    var fired = 0u;
    if (g.x < p.rw && row < p.nt * p.rh) {
        let k = row / p.rh;
        let lj = p.j0 + row % p.rh;
        let base = (k * p.ph + lj + PADR) * p.wr + PADW + x;
        let me = src[base];
        let t = tinfo[k];
        let f = fires(me, t.z);
        var v = me - f * t.z;
        for (var o = t.x; o < t.x + t.y; o = o + 1u) {
            let e = offs[o];
            let nb = i32((e.k * p.ph + lj + PADR) * p.wr + PADW + x) + e.dj * i32(p.wr) + e.q;
            var w = src[u32(nb)];
            if (e.s != 0u) {
                w = (w >> (8u * e.s)) | (src[u32(nb + 1)] << (32u - 8u * e.s));
            }
            v = v + fires(w, tinfo[e.k].z);
        }
        dst[base] = v;
        fired = countOneBits(f);
        let outside = x < p.nx0 || x >= p.nx1 || lj < p.nj0 || lj >= p.nj1;
        if (v != 0u && outside) {
            atomicMin(&wbox[0], x);
            atomicMax(&wbox[1], x);
            atomicMin(&wbox[2], lj);
            atomicMax(&wbox[3], lj);
            let i = x * 4u;
            if (lj < p.band || lj + p.band >= p.lh || i < p.band || i + 4u + p.band > p.lw * 4u) {
                atomicOr(&stats[0], 1u);
            }
        }
    }
    if (fired != 0u) {
        atomicAdd(&wsum, fired);
    }
    workgroupBarrier();
    if (li == 0u) {
        let s = atomicLoad(&wsum);
        if (s != 0u) {
            atomicAdd(&stats[5u + p.slot], s);
        }
        let x0 = atomicLoad(&wbox[0]);
        if (x0 != 0xffffffffu) {
            atomicMin(&stats[1], x0);
            atomicMax(&stats[2], atomicLoad(&wbox[1]));
            atomicMin(&stats[3], atomicLoad(&wbox[2]));
            atomicMax(&stats[4], atomicLoad(&wbox[3]));
        }
    }
}
