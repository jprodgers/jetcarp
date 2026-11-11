//! A turmite on an unbounded periodic tiling: the lattice's move table, a field that grows as
//! the ant reaches its edge, and the step.
//!
//! Cells are `(i, j, k)`: unsheared lattice coordinates of the unit cell and the tile type, as in
//! [`crate::topology::LatticeGrid`]. The ant moves exactly as [`crate::step`] moves a turmite on
//! a world: it turns by its entry's slots in the cell's cyclic edge-neighbour order, steps to
//! that neighbour, and then faces the slot opposite the edge it came in by.

use crate::topology::{Boundary, Nb, SizeSpec, TilingSpec, Topology};
use crate::turmite::Mite;

/// The move table of a periodic tiling.
#[derive(Clone, Debug)]
pub struct Lattice {
    pub tiling: TilingSpec,
    pub ntypes: usize,
    /// Edge neighbours per tile type.
    pub deg: Vec<u8>,
    /// `moves[k][h] = (di, dj, k2, h2)`: leaving a type-`k` cell through slot `h` lands on the
    /// cell `(i + di, j + dj, k2)`, facing slot `h2`.
    pub moves: Vec<Vec<(i32, i32, u8, u8)>>,
    /// `back[k][h]`: the slot of that neighbour that points back.
    pub back: Vec<Vec<u8>>,
    pub a1: [f64; 2],
    pub a2: [f64; 2],
    /// Reflections (integer 2x2 maps of det -1 preserving the edge offsets) of a one-tile
    /// lattice, for the symmetry test; empty otherwise.
    pub reflections: Vec<[i32; 4]>,
    /// Mirror images of turmites behave as mirror images (a one-tile lattice with a reflection
    /// and rotations taking slot 0 to every slot), so searches keep one of each pair.
    pub mirror: bool,
}

impl Lattice {
    pub fn new(t: &TilingSpec) -> Result<Lattice, String> {
        let mut spec = crate::pattern::tiling_identity(t);
        spec.size = Some(SizeSpec::Rect { w: 12, h: 12 });
        spec.boundary = Boundary::Wrap;
        let topo = Topology::build(&spec)?;
        let g = topo
            .lattice()
            .ok_or_else(|| {
                format!(
                    "{}: ant searches run on periodic tilings only",
                    crate::pattern::tiling_display_name(t)
                )
            })?
            .clone();
        let offs = &g.offsets[Nb::Edge as usize];
        let nt = g.ntypes as usize;
        let mut moves = Vec::with_capacity(nt);
        let mut back = Vec::with_capacity(nt);
        let mut deg = Vec::with_capacity(nt);
        for (k, row) in offs.iter().enumerate() {
            if row.is_empty() || row.len() > 255 {
                return Err(format!("tile type {k} has {} edge neighbours", row.len()));
            }
            deg.push(row.len() as u8);
            let mut mv = Vec::new();
            let mut bk = Vec::new();
            for o in row {
                let other = &offs[o.k as usize];
                let b = other
                    .iter()
                    .position(|x| x.di == -o.di && x.dj == -o.dj && x.k as usize == k)
                    .ok_or_else(|| {
                        format!("tile type {k}: a neighbour has no edge back (not a tiling?)")
                    })?;
                let d2 = other.len();
                mv.push((o.di, o.dj, o.k as u8, ((b + d2 / 2) % d2) as u8));
                bk.push(b as u8);
            }
            moves.push(mv);
            back.push(bk);
        }
        let (mut reflections, mut mirror) = (Vec::new(), false);
        if nt == 1 {
            let syms = crate::objects::symmetries(&g, Nb::Edge);
            reflections = syms
                .iter()
                .copied()
                .filter(|m| m[0] * m[3] - m[1] * m[2] == -1)
                .collect();
            let first = (offs[0][0].di, offs[0][0].dj);
            let rot_hits: std::collections::HashSet<(i32, i32)> = syms
                .iter()
                .filter(|m| m[0] * m[3] - m[1] * m[2] == 1)
                .map(|m| {
                    (
                        m[0] * first.0 + m[1] * first.1,
                        m[2] * first.0 + m[3] * first.1,
                    )
                })
                .collect();
            mirror = !reflections.is_empty() && rot_hits.len() == offs[0].len();
        }
        Ok(Lattice {
            tiling: crate::pattern::tiling_identity(t),
            ntypes: nt,
            deg,
            moves,
            back,
            a1: g.a1,
            a2: g.a2,
            reflections,
            mirror,
        })
    }
    /// The distinct edge-neighbour counts.
    pub fn degrees(&self) -> Vec<u8> {
        let mut d = self.deg.clone();
        d.sort();
        d.dedup();
        d
    }
    /// World position of a lattice vector.
    pub fn world(&self, d: [i32; 2]) -> [f64; 2] {
        [
            d[0] as f64 * self.a1[0] + d[1] as f64 * self.a2[0],
            d[0] as f64 * self.a1[1] + d[1] as f64 * self.a2[1],
        ]
    }
    /// World position of fractional lattice coordinates.
    pub fn world_f(&self, d: [f64; 2]) -> [f64; 2] {
        [
            d[0] * self.a1[0] + d[1] * self.a2[0],
            d[0] * self.a1[1] + d[1] * self.a2[1],
        ]
    }
    /// The square root of a tile's mean area.
    pub fn cell_size(&self) -> f64 {
        (crate::geom::cross(self.a1, self.a2).abs() / self.ntypes as f64).sqrt()
    }
}

/// Chunk side in unit cells (a power of two).
const SH: i32 = 6;
const CH: i32 = 1 << SH;

/// A field of colours over unit cells `(i, j)` (all tile types), unbounded: chunks of 64 x 64
/// unit cells made when the ant first enters them, so memory follows the cells reached (a long
/// diagonal highway needs no square of its length).
#[derive(Clone, Debug)]
pub struct Field {
    pub nt: usize,
    chunks: Vec<Box<[u8]>>,
    keys: Vec<(i32, i32)>,
    map: std::collections::HashMap<(i32, i32), u32>,
    /// The chunk the ant was last in.
    cur: ((i32, i32), u32),
    /// Chunks made, as `[ci0, cj0, ci1, cj1]` (inclusive); every cell outside is 0.
    cbox: [i32; 4],
}

impl Field {
    pub fn new(nt: usize) -> Field {
        let mut f = Field {
            nt,
            chunks: Vec::new(),
            keys: Vec::new(),
            map: Default::default(),
            cur: ((0, 0), 0),
            cbox: [0, 0, 0, 0],
        };
        f.chunk((0, 0));
        f.cur = ((0, 0), 0);
        f
    }
    fn chunk(&mut self, key: (i32, i32)) -> u32 {
        if let Some(&c) = self.map.get(&key) {
            return c;
        }
        let c = self.chunks.len() as u32;
        self.chunks
            .push(vec![0u8; (CH * CH) as usize * self.nt].into_boxed_slice());
        self.keys.push(key);
        self.map.insert(key, c);
        if c == 0 {
            self.cbox = [key.0, key.1, key.0, key.1];
        } else {
            self.cbox = [
                self.cbox[0].min(key.0),
                self.cbox[1].min(key.1),
                self.cbox[2].max(key.0),
                self.cbox[3].max(key.1),
            ];
        }
        c
    }
    #[inline]
    fn local(&self, i: i32, j: i32, k: u8) -> usize {
        (((j & (CH - 1)) * CH + (i & (CH - 1))) as usize) * self.nt + k as usize
    }
    /// The cell under the ant, made if needed.
    #[inline]
    pub fn cell_mut(&mut self, i: i32, j: i32, k: u8) -> &mut u8 {
        let key = (i >> SH, j >> SH);
        if key != self.cur.0 {
            let c = self.chunk(key);
            self.cur = (key, c);
        }
        let x = self.local(i, j, k);
        &mut self.chunks[self.cur.1 as usize][x]
    }
    /// Inside the box of chunks made (outside it every cell is 0 and always was).
    #[inline]
    pub fn contains(&self, i: i32, j: i32) -> bool {
        let (ci, cj) = (i >> SH, j >> SH);
        ci >= self.cbox[0] && cj >= self.cbox[1] && ci <= self.cbox[2] && cj <= self.cbox[3]
    }
    /// Colour of a cell (0 where nothing was made).
    #[inline]
    pub fn get(&self, i: i32, j: i32, k: u8) -> u8 {
        let key = (i >> SH, j >> SH);
        let c = if key == self.cur.0 {
            self.cur.1
        } else {
            match self.map.get(&key) {
                Some(&c) => c,
                None => return 0,
            }
        };
        self.chunks[c as usize][self.local(i, j, k)]
    }
    pub fn set(&mut self, i: i32, j: i32, k: u8, v: u8) {
        *self.cell_mut(i, j, k) = v;
    }
    /// Unit cells covered by the chunks made: `[i0, j0, i1, j1]` inclusive.
    pub fn bounds(&self) -> [i32; 4] {
        [
            self.cbox[0] * CH,
            self.cbox[1] * CH,
            self.cbox[2] * CH + CH - 1,
            self.cbox[3] * CH + CH - 1,
        ]
    }
    /// Every non-zero cell `(i, j, k, colour)`, chunk by chunk in key order.
    pub fn nonzero(&self) -> Vec<(i32, i32, u8, u8)> {
        let mut order: Vec<usize> = (0..self.keys.len()).collect();
        order.sort_by_key(|&c| (self.keys[c].1, self.keys[c].0));
        let mut v = Vec::new();
        for c in order {
            let (ci, cj) = self.keys[c];
            for (x, &col) in self.chunks[c].iter().enumerate() {
                if col != 0 {
                    let u = (x / self.nt) as i32;
                    v.push((ci * CH + u % CH, cj * CH + u / CH, (x % self.nt) as u8, col));
                }
            }
        }
        v
    }
    /// Bytes held.
    pub fn bytes(&self) -> usize {
        self.chunks.len() * (CH * CH) as usize * self.nt
    }
}

/// The ant: cell, heading (slot), internal state.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub struct Ant {
    pub i: i32,
    pub j: i32,
    pub k: u8,
    pub h: u8,
    pub q: u8,
}

/// A turmite's table compiled for a lattice: per tile type, `(write, slots turned, next)` per
/// `(state, colour)`.
#[derive(Clone, Debug)]
pub struct Compiled {
    pub colors: u8,
    pub states: u8,
    pub per_type: Vec<Vec<(u8, u8, u8)>>,
}

impl Compiled {
    pub fn new(m: &Mite, lat: &Lattice) -> Compiled {
        Compiled {
            colors: m.colors,
            states: m.states,
            per_type: lat
                .deg
                .iter()
                .map(|&d| {
                    m.table
                        .iter()
                        .map(|e| (e.write, e.turn.slots(d as usize) as u8, e.next))
                        .collect()
                })
                .collect(),
        }
    }
}

/// One step's symbol: the colour read, the state, the tile type, and the heading before the
/// turn. The ant's moves are a function of its symbols.
#[inline]
pub fn symbol(c: u8, a: &Ant) -> u32 {
    c as u32 | (a.q as u32) << 8 | (a.k as u32) << 16 | (a.h as u32) << 24
}

/// Step the ant once. Returns `(symbol, old colour, new colour)`; the cell it wrote is the one
/// it stood on (`a` before the call).
#[inline]
pub fn step(lat: &Lattice, m: &Compiled, f: &mut Field, a: &mut Ant) -> (u32, u8, u8) {
    let cell = f.cell_mut(a.i, a.j, a.k);
    let c = *cell;
    let sym = symbol(c, a);
    let (w, turn, next) = m.per_type[a.k as usize][a.q as usize * m.colors as usize + c as usize];
    *cell = w;
    let d = lat.deg[a.k as usize];
    let mut h = a.h + turn;
    if h >= d {
        h -= d;
    }
    let (di, dj, k2, h2) = lat.moves[a.k as usize][h as usize];
    a.i += di;
    a.j += dj;
    a.k = k2;
    a.h = h2;
    a.q = next;
    (sym, c, w)
}
