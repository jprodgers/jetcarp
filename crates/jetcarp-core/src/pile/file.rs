//! Pile files (`.pile`): one finished doubling step, enough to resume the run.
//!
//! Layout: the 8 bytes `JETCPILE`, a little-endian `u32` format version (1), a `u32` header
//! length, the header as JSON, then the heights, deflate-compressed. The header holds the
//! [`PileSpec`], the sources, the step counts and records, and the stored region:
//!
//! - lattice: `{"kind": "lattice", "i0", "j0", "w", "h", "nt"}`, the bounding box of the nonzero
//!   cells in lattice coordinates; heights are `nt` planes of `h` rows of `w` cells;
//! - patch: `{"kind": "patch", "level", "cells"}`; heights for every tile in patch order.
//!
//! Heights are packed two per byte (low nibble first) when every degree is at most 16, else one
//! per byte (`"bits": 4` or `8`).

use super::lattice::{Lattice, LatticeBoard, TILE};
use super::patch::PatchBoard;
use super::{Board, Pile, PileSpec, Source, StepRecord};
use crate::topology::{SizeSpec, Topology};
use flate2::Compression;
use serde_json::{Value, json};
use std::io::{Read, Write};
use std::path::Path;
use std::sync::Arc;

const MAGIC: &[u8; 8] = b"JETCPILE";
const VERSION: u32 = 1;

fn pack(h: &[u8], bits: u32) -> Vec<u8> {
    if bits == 8 {
        return h.to_vec();
    }
    h.chunks(2)
        .map(|c| c[0] | (c.get(1).copied().unwrap_or(0) << 4))
        .collect()
}

fn unpack(b: &[u8], bits: u32, n: usize) -> Vec<u8> {
    if bits == 8 {
        return b[..n].to_vec();
    }
    let mut out = Vec::with_capacity(n);
    for &x in b {
        out.push(x & 15);
        out.push(x >> 4);
    }
    out.truncate(n);
    out
}

/// Save the pile (after a finished step).
pub fn save(pile: &Pile, path: &Path) -> Result<(), String> {
    let (region, heights, maxd) = match &pile.board {
        Board::Lattice(b) => {
            let nt = b.lat.nt;
            let (region, h) = match b.nonzero_box() {
                Some(bx) => {
                    let (w, hh) = ((bx[2] - bx[0] + 1) as usize, (bx[3] - bx[1] + 1) as usize);
                    let mut h = Vec::with_capacity(w * hh * nt);
                    for k in 0..nt {
                        for j in bx[1]..=bx[3] {
                            let row = b.row(k, j).unwrap();
                            let s = (bx[0] - b.i0) as usize;
                            h.extend_from_slice(&row[s..s + w]);
                        }
                    }
                    (
                        json!({"kind": "lattice", "i0": bx[0], "j0": bx[1], "w": w, "h": hh, "nt": nt}),
                        h,
                    )
                }
                None => (
                    json!({"kind": "lattice", "i0": 0, "j0": 0, "w": 0, "h": 0, "nt": nt}),
                    Vec::new(),
                ),
            };
            (region, h, b.lat.max_degree())
        }
        Board::Patch(b) => (
            json!({"kind": "patch", "level": b.level, "cells": b.cur.len()}),
            b.cur.clone(),
            b.deg.iter().copied().max().unwrap_or(1),
        ),
    };
    let bits = if maxd <= 16 { 4 } else { 8 };
    let header = json!({
        "jetcarp_version": crate::VERSION,
        "spec": pile.spec,
        "sources": pile.sources,
        "steps": pile.steps,
        "done": pile.done,
        "grains": pile.grains_after(pile.done),
        "records": pile.records,
        "region": region,
        "bits": bits,
    });
    let hb = serde_json::to_vec(&header).unwrap();
    let tmp = path.with_extension("pile.tmp");
    let f = std::fs::File::create(&tmp).map_err(|e| format!("{}: {e}", tmp.display()))?;
    let mut w = std::io::BufWriter::new(f);
    let io = |e: std::io::Error| format!("{}: {e}", path.display());
    w.write_all(MAGIC).map_err(io)?;
    w.write_all(&VERSION.to_le_bytes()).map_err(io)?;
    w.write_all(&(hb.len() as u32).to_le_bytes()).map_err(io)?;
    w.write_all(&hb).map_err(io)?;
    let mut z = flate2::write::DeflateEncoder::new(w, Compression::new(3));
    z.write_all(&pack(&heights, bits)).map_err(io)?;
    let mut w = z.finish().map_err(io)?;
    w.flush().map_err(io)?;
    drop(w);
    std::fs::rename(&tmp, path).map_err(io)
}

/// Read a pile file's header only.
pub fn read_header(path: &Path) -> Result<Value, String> {
    let mut f = std::fs::File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    read_header_from(&mut f, path)
}

fn read_header_from(f: &mut impl Read, path: &Path) -> Result<Value, String> {
    let io = |e: std::io::Error| format!("{}: {e}", path.display());
    let mut m = [0u8; 16];
    f.read_exact(&mut m).map_err(io)?;
    if &m[..8] != MAGIC {
        return Err(format!("{} is not a JETCARP pile file", path.display()));
    }
    let ver = u32::from_le_bytes(m[8..12].try_into().unwrap());
    if ver != VERSION {
        return Err(format!(
            "{}: pile format {ver} is not supported",
            path.display()
        ));
    }
    let n = u32::from_le_bytes(m[12..16].try_into().unwrap()) as usize;
    let mut hb = vec![0u8; n];
    f.read_exact(&mut hb).map_err(io)?;
    serde_json::from_slice(&hb).map_err(|e| format!("{}: {e}", path.display()))
}

/// Load a pile saved by [`save`]; the run continues with the next step.
pub fn load(path: &Path) -> Result<Pile, String> {
    let f = std::fs::File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut f = std::io::BufReader::new(f);
    let h = read_header_from(&mut f, path)?;
    let bad = |what: &str| format!("{}: bad header ({what})", path.display());
    let spec: PileSpec =
        serde_json::from_value(h["spec"].clone()).map_err(|e| bad(&e.to_string()))?;
    let sources: Vec<Source> =
        serde_json::from_value(h["sources"].clone()).map_err(|e| bad(&e.to_string()))?;
    let records: Vec<StepRecord> =
        serde_json::from_value(h["records"].clone()).map_err(|e| bad(&e.to_string()))?;
    let steps = h["steps"].as_u64().ok_or_else(|| bad("steps"))? as u32;
    let done = h["done"].as_u64().ok_or_else(|| bad("done"))? as u32;
    let bits = h["bits"].as_u64().ok_or_else(|| bad("bits"))? as u32;
    let r = &h["region"];
    let mut packed = Vec::new();
    flate2::read::DeflateDecoder::new(f)
        .read_to_end(&mut packed)
        .map_err(|e| format!("{}: {e}", path.display()))?;
    let board = match r["kind"].as_str() {
        Some("lattice") => {
            let g = |k: &str| r[k].as_i64().ok_or_else(|| bad(k));
            let (i0, j0, w, hh, nt) = (
                g("i0")?,
                g("j0")?,
                g("w")? as usize,
                g("h")? as usize,
                g("nt")? as usize,
            );
            let lat = Arc::new(Lattice::new(&spec.tiling, spec.nb)?);
            if lat.nt != nt {
                return Err(bad("tile types"));
            }
            let heights = unpack(&packed, bits, w * hh * nt);
            if heights.len() != w * hh * nt {
                return Err(bad("short data"));
            }
            // A box with a ring of empty tiles around the stored region.
            let tx = w.div_ceil(TILE) + 2;
            let ty = hh.div_ceil(TILE) + 2;
            let mut b = LatticeBoard::with_box(lat, i0 - TILE as i64, j0 - TILE as i64, tx, ty);
            for k in 0..nt {
                for y in 0..hh {
                    for x in 0..w {
                        let v = heights[(k * hh + y) * w + x];
                        if v != 0 {
                            let idx = b.index(i0 + x as i64, j0 + y as i64, k).unwrap();
                            b.cur[idx] = v;
                        }
                    }
                }
            }
            b.sync();
            Board::Lattice(b)
        }
        Some("patch") => {
            let level = r["level"].as_u64().ok_or_else(|| bad("level"))? as u32;
            let n = r["cells"].as_u64().ok_or_else(|| bad("cells"))? as usize;
            let mut ts = spec.tiling.clone();
            ts.size = Some(SizeSpec::Level(level));
            let topo = Arc::new(Topology::build(&ts)?);
            if topo.len() != n {
                return Err(bad("tile count"));
            }
            let mut b = PatchBoard::new(topo, spec.nb, level)?;
            b.cur = unpack(&packed, bits, n);
            if b.cur.len() != n {
                return Err(bad("short data"));
            }
            b.sync();
            Board::Patch(b)
        }
        _ => return Err(bad("region kind")),
    };
    let pile = Pile {
        spec,
        board,
        sources,
        steps,
        done,
        records,
    };
    let want = pile.grains_after(done);
    let have = pile.board.grains();
    if want != have {
        return Err(format!(
            "{}: the file holds {have} grains, expected {want}",
            path.display()
        ));
    }
    Ok(pile)
}
