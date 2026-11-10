//! Rule tables: transitions written as `"c,n1,...,nk:new"`.
//!
//! - Neighbours are read in the cell's cyclic slot order (clockwise on screen). On the square grid
//!   with edge neighbours that is North, East, South, West.
//! - Each position is a state number, `*` (any state), or alternatives like `1|2|3`.
//! - Symmetry: `none`, `rotate` (all cyclic shifts of the neighbours), `rotate_reflect` (also
//!   reversed), or `permute` (neighbours as a multiset, any order).
//! - The first matching entry wins. Cells with no matching entry keep their state.
//! - Missing neighbours (past a dead edge) read as state 0. Cells whose neighbour count differs
//!   from `arity` keep their state (positional tables only).
//!
//! This is JETCARP's own format; no Golly rule files or code are used.

use crate::topology::Nb;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Symmetry {
    #[default]
    None,
    Rotate,
    RotateReflect,
    Permute,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RuleTable {
    pub name: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
    /// Where the transition data came from (publication, derivation).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub source: String,
    pub states: u8,
    #[serde(default = "edge")]
    pub neighborhood: Nb,
    #[serde(default)]
    pub symmetry: Symmetry,
    /// Number of neighbours each entry lists.
    pub arity: usize,
    pub transitions: Vec<String>,
}
fn edge() -> Nb {
    Nb::Edge
}

/// A table ready for lookups.
#[derive(Debug)]
pub struct Compiled {
    pub table: RuleTable,
    /// Dense lookup for positional tables: index = c*S^A + n1*S^(A-1) + ... ; 255 = no entry.
    dense: Vec<u8>,
    /// Permute tables: (centre, sorted neighbours) → new state.
    multiset: HashMap<Vec<u8>, u8>,
}

const NONE: u8 = 255;

impl Compiled {
    pub fn compile(t: RuleTable) -> Result<Compiled, String> {
        if t.states < 2 || t.states > 254 {
            return Err(format!("table '{}': states must be 2 to 254", t.name));
        }
        if t.arity == 0 || t.arity > 12 {
            return Err(format!("table '{}': arity must be 1 to 12", t.name));
        }
        let s = t.states as usize;
        let a = t.arity;
        let positional = t.symmetry != Symmetry::Permute;
        let size = s.checked_pow(a as u32 + 1).unwrap_or(usize::MAX);
        if positional && size > 1 << 26 {
            return Err(format!(
                "table '{}': {s}^{} entries is too large for a dense table",
                t.name,
                a + 1
            ));
        }
        let mut dense = if positional { vec![NONE; size] } else { vec![] };
        let mut multiset = HashMap::new();
        for (line_no, line) in t.transitions.iter().enumerate() {
            let err = |m: &str| format!("table '{}' entry {} ('{line}'): {m}", t.name, line_no + 1);
            let (lhs, rhs) = line.split_once(':').ok_or_else(|| err("missing ':new'"))?;
            let new: u8 = rhs
                .trim()
                .parse()
                .map_err(|_| err("new state is not a number"))?;
            if new >= t.states {
                return Err(err("new state out of range"));
            }
            let pos: Vec<&str> = lhs.split(',').map(|x| x.trim()).collect();
            if pos.len() != a + 1 {
                return Err(err(&format!(
                    "expected {} positions, got {}",
                    a + 1,
                    pos.len()
                )));
            }
            let mut choices: Vec<Vec<u8>> = Vec::with_capacity(a + 1);
            for p in &pos {
                let c: Vec<u8> = if *p == "*" {
                    (0..t.states).collect()
                } else {
                    let v: Result<Vec<u8>, _> =
                        p.split('|').map(|x| x.trim().parse::<u8>()).collect();
                    let v = v.map_err(|_| err(&format!("'{p}' is not a state, '*', or 'a|b'")))?;
                    if v.iter().any(|&x| x >= t.states) {
                        return Err(err("state out of range"));
                    }
                    v
                };
                choices.push(c);
            }
            // Expand the cartesian product (odometer over the choices).
            let mut idx = vec![0usize; a + 1];
            'combos: loop {
                let tuple: Vec<u8> = (0..=a).map(|k| choices[k][idx[k]]).collect();
                let (c, nbrs) = (tuple[0], &tuple[1..]);
                if positional {
                    for v in variants(nbrs, t.symmetry) {
                        let key = dense_index(c, &v, s);
                        if dense[key] == NONE {
                            dense[key] = new;
                        }
                    }
                } else {
                    let mut k = nbrs.to_vec();
                    k.sort_unstable();
                    k.insert(0, c);
                    multiset.entry(k).or_insert(new);
                }
                let mut d = a + 1;
                loop {
                    if d == 0 {
                        break 'combos;
                    }
                    d -= 1;
                    idx[d] += 1;
                    if idx[d] < choices[d].len() {
                        break;
                    }
                    idx[d] = 0;
                }
            }
        }
        Ok(Compiled {
            table: t,
            dense,
            multiset,
        })
    }

    /// New state for centre `c` with neighbours `n` (cyclic order), or `None` to keep the state.
    #[inline]
    pub fn lookup(&self, c: u8, n: &[u8]) -> Option<u8> {
        if self.table.symmetry == Symmetry::Permute {
            let mut k = Vec::with_capacity(n.len() + 1);
            k.extend_from_slice(n);
            k.sort_unstable();
            k.insert(0, c);
            return self.multiset.get(&k).copied();
        }
        if n.len() != self.table.arity {
            return None;
        }
        let v = self.dense[dense_index(c, n, self.table.states as usize)];
        (v != NONE).then_some(v)
    }
}

fn dense_index(c: u8, n: &[u8], s: usize) -> usize {
    n.iter().fold(c as usize, |acc, &x| acc * s + x as usize)
}

fn variants(n: &[u8], sym: Symmetry) -> Vec<Vec<u8>> {
    let a = n.len();
    let mut out = vec![n.to_vec()];
    if matches!(sym, Symmetry::Rotate | Symmetry::RotateReflect) {
        for r in 1..a {
            out.push((0..a).map(|k| n[(k + r) % a]).collect());
        }
    }
    if sym == Symmetry::RotateReflect {
        let rev: Vec<Vec<u8>> = out
            .iter()
            .map(|v| v.iter().rev().copied().collect())
            .collect();
        out.extend(rev);
    }
    out
}

/// Tables available to every world. Scenes can add more with `Command::AddTable`.
pub fn builtin_tables() -> Vec<RuleTable> {
    vec![parity_vn()]
}

/// Fredkin's parity rule on edge neighbours: the new state is the XOR of the four neighbours.
/// Every pattern replicates. Written as a positional table with rotation symmetry, so it also
/// exercises the symmetry expansion.
pub fn parity_vn() -> RuleTable {
    let mut transitions = Vec::new();
    for c in 0..2u8 {
        for m in 0..16u8 {
            let n: Vec<u8> = (0..4).map(|k| (m >> (3 - k)) & 1).collect();
            // Only canonical rotations; symmetry fills in the rest.
            let canon = (0..4)
                .map(|r| (0..4).map(|k| n[(k + r) % 4]).collect::<Vec<u8>>())
                .min()
                .unwrap();
            if canon != n {
                continue;
            }
            let x = n.iter().fold(0, |a, b| a ^ b);
            transitions.push(format!("{c},{},{},{},{}:{x}", n[0], n[1], n[2], n[3]));
        }
    }
    RuleTable {
        name: "parity".into(),
        description: "Fredkin's replicator: each cell becomes the XOR of its four edge neighbours."
            .into(),
        source: "Generated in JETCARP from the rule's definition.".into(),
        states: 2,
        neighborhood: Nb::Edge,
        symmetry: Symmetry::Rotate,
        arity: 4,
        transitions,
    }
}

/// A set of compiled tables by lower-case name.
#[derive(Clone, Debug, Default)]
pub struct TableSet {
    pub tables: HashMap<String, Arc<Compiled>>,
}

impl TableSet {
    pub fn with_builtins() -> TableSet {
        let mut s = TableSet::default();
        for t in builtin_tables() {
            s.insert(t).expect("built-in table compiles");
        }
        s
    }
    pub fn insert(&mut self, t: RuleTable) -> Result<Arc<Compiled>, String> {
        let key = t.name.to_ascii_lowercase();
        let c = Arc::new(Compiled::compile(t)?);
        self.tables.insert(key, c.clone());
        Ok(c)
    }
    pub fn get(&self, name: &str) -> Option<Arc<Compiled>> {
        self.tables.get(&name.to_ascii_lowercase()).cloned()
    }
}
