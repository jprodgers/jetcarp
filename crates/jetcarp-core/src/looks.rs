//! Picture measures for one frame: how compressible it is and how many different blocks its rows
//! hold. Sweep descriptors (`jetcarp-script`'s `describe`) use the same block entropy.

use flate2::Compression;
use flate2::write::DeflateEncoder;
use std::io::Write;

/// Entropy-rate estimate of rows from distinct blocks of `k` consecutive positions:
/// `log(N(k2) / N(k1)) / ((k2 - k1) log(states^stride))`, with `k2` the largest block length whose
/// `states^(stride k2)` possible blocks is at most a quarter of the blocks sampled (and at most
/// 24 and half a row), and `k1 = k2 / 2`. A position is `stride` consecutive values (the tile
/// types of a unit cell). Simple pictures repeat a few blocks and score low; noise contains
/// nearly every block and scores near 1. `None` when the rows are too short for two block
/// lengths.
pub fn block_entropy<R: AsRef<[u8]>>(rows: &[R], stride: usize, states: usize) -> Option<f64> {
    let stride = stride.max(1);
    let n = rows.first().map_or(0, |r| r.as_ref().len() / stride);
    let ks = (states.max(2) as f64).powi(stride as i32);
    let sample = (rows.len() * n) as f64;
    let mut k2 = 1usize;
    while ks.powi(k2 as i32 + 1) <= sample / 4.0 && k2 < 24 && k2 < n / 2 {
        k2 += 1;
    }
    if k2 < 2 {
        return None;
    }
    let k1 = k2 / 2;
    // A block of k positions is a number below states^(stride k), which is at most a quarter of
    // the blocks sampled: a table of flags counts the distinct ones.
    let base = states.max(2) as u64;
    let count = |k: usize| -> usize {
        let space = ks.powi(k as i32).round() as usize;
        let mut seen = vec![false; space.max(1)];
        let mut distinct = 0;
        for r in rows {
            let r = r.as_ref();
            for u in 0..=n - k {
                let key = r[u * stride..(u + k) * stride]
                    .iter()
                    .fold(0u64, |a, &v| a * base + (v as u64).min(base - 1))
                    as usize;
                if !seen[key] {
                    seen[key] = true;
                    distinct += 1;
                }
            }
        }
        distinct
    };
    let (n1, n2) = (count(k1) as f64, count(k2) as f64);
    Some(((n2 / n1).ln() / ((k2 - k1) as f64 * ks.ln())).clamp(0.0, 1.0))
}

/// Deflated size of the states (one byte per cell, in the world's order) in bits per cell: a
/// stand-in for the PNG size sweeps record. Noise at density `p` needs about `H(p)` bits per
/// cell; a uniform frame almost none.
pub fn compressed_bits(states: &[u8]) -> f64 {
    if states.is_empty() {
        return 0.0;
    }
    let mut enc = DeflateEncoder::new(Vec::with_capacity(states.len() / 4), Compression::fast());
    let _ = enc.write_all(states);
    let out = enc.finish().unwrap_or_default();
    8.0 * out.len() as f64 / states.len() as f64
}

/// Binary entropy `H(p)` in bits.
pub fn binary_entropy(p: f64) -> f64 {
    if p <= 0.0 || p >= 1.0 {
        return 0.0;
    }
    -(p * p.log2() + (1.0 - p) * (1.0 - p).log2())
}

/// Shannon entropy of the state distribution in bits (states with no cells contribute nothing).
pub fn state_entropy(states: &[u8]) -> f64 {
    let mut counts = [0u64; 256];
    for &s in states {
        counts[s as usize] += 1;
    }
    let n = states.len().max(1) as f64;
    counts
        .iter()
        .filter(|&&c| c > 0)
        .map(|&c| {
            let p = c as f64 / n;
            -p * p.log2()
        })
        .sum()
}

/// What a frame looks like.
#[derive(Clone, Copy, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct FrameLooks {
    /// Non-zero cells / cells.
    pub density: f64,
    /// [`compressed_bits`] over the frame.
    pub bits_per_cell: f64,
    /// `bits_per_cell` / the bits per cell of the same states shuffled (noise with the same mix
    /// of states): near 1 for noise, near 0 for a uniform or very regular frame, 0 when empty.
    pub compress_vs_noise: f64,
    /// [`block_entropy`] over every row (`None` on tiny grids).
    pub block_entropy: Option<f64>,
}

/// Measure one frame: `states` in the world's order, rows of `w` unit cells of `ntypes` tile
/// types.
pub fn frame_looks(states: &[u8], w: usize, ntypes: usize, nstates: usize) -> FrameLooks {
    let n = states.len().max(1);
    let nz = states.iter().filter(|&&s| s != 0).count();
    let bits = compressed_bits(states);
    let rows: Vec<&[u8]> = states.chunks(w * ntypes).collect();
    FrameLooks {
        density: nz as f64 / n as f64,
        bits_per_cell: bits,
        compress_vs_noise: if nz == 0 {
            0.0
        } else {
            (bits / compressed_bits(&shuffled(states)).max(1e-9)).min(1.5)
        },
        block_entropy: block_entropy(&rows, ntypes, nstates),
    }
}

/// The states in a fixed pseudo-random order (Fisher-Yates with a fixed seed): noise with the
/// same mix of states.
fn shuffled(states: &[u8]) -> Vec<u8> {
    let mut v = states.to_vec();
    let mut rng = crate::rng::Rng::new(0x5eed_5eed);
    for i in (1..v.len()).rev() {
        let j = (rng.next_u64() % (i as u64 + 1)) as usize;
        v.swap(i, j);
    }
    v
}
