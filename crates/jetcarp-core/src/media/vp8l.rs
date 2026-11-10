//! A small lossless WebP (VP8L) bitstream writer for images with at most 256 colours, which
//! is every CA frame in the house style. Written from the WebP Lossless Bitstream
//! Specification (RFC 9649); no code from libwebp.
//!
//! It uses the colour-indexing transform (a palette, with 2, 4, or 8 pixels bundled per
//! pixel when there are at most 16, 4, or 2 colours), LZ77 backward references to the pixel
//! to the left and the pixel above (the commonest repeats in pictures of cells several pixels
//! wide), and one group of canonical prefix codes. On whole-soup animations this gives files
//! about a third the size of `image-webp`'s general encoder.

use crate::render::Image;

struct Bits {
    out: Vec<u8>,
    acc: u64,
    n: u32,
}

impl Bits {
    fn new() -> Bits {
        Bits {
            out: Vec::new(),
            acc: 0,
            n: 0,
        }
    }
    /// Write the low `n` bits of `v`, least significant first.
    fn put(&mut self, v: u32, n: u32) {
        debug_assert!(n <= 32);
        if n == 0 {
            return;
        }
        self.acc |= (v as u64 & ((1u64 << n) - 1)) << self.n;
        self.n += n;
        while self.n >= 8 {
            self.out.push(self.acc as u8);
            self.acc >>= 8;
            self.n -= 8;
        }
    }
    fn finish(mut self) -> Vec<u8> {
        if self.n > 0 {
            self.out.push(self.acc as u8);
        }
        self.out
    }
}

/// Code lengths (at most `limit` bits) for symbol frequencies. Unused symbols get 0. Fewer
/// than two used symbols get a second one so every normal code has two leaves.
fn lengths(freq: &[u32], limit: u8) -> Vec<u8> {
    let mut f: Vec<u64> = freq.iter().map(|&x| x as u64).collect();
    match f.iter().position(|&x| x > 0) {
        None => {
            f[0] = 1;
            f[1] = 1;
        }
        Some(s) if f.iter().filter(|&&x| x > 0).count() == 1 => f[if s == 0 { 1 } else { 0 }] = 1,
        _ => {}
    }
    loop {
        let l = huffman(&f);
        if l.iter().all(|&x| x <= limit) {
            return l;
        }
        // Flatten the distribution and try again.
        for x in &mut f {
            if *x > 0 {
                *x = (*x >> 1).max(1);
            }
        }
    }
}

/// Plain Huffman code lengths (two smallest merged first; ties by creation order, so the
/// result is deterministic).
fn huffman(f: &[u64]) -> Vec<u8> {
    // Nodes: (weight, order, children or leaf symbol).
    type Node = (u64, Option<(usize, usize)>, usize);
    let mut nodes: Vec<Node> = Vec::new();
    let mut live: Vec<usize> = Vec::new();
    for (s, &w) in f.iter().enumerate() {
        if w > 0 {
            nodes.push((w, None, s));
            live.push(nodes.len() - 1);
        }
    }
    while live.len() > 1 {
        live.sort_by(|&a, &b| nodes[a].0.cmp(&nodes[b].0).then(a.cmp(&b)));
        let (a, b) = (live[0], live[1]);
        nodes.push((nodes[a].0 + nodes[b].0, Some((a, b)), 0));
        live.drain(0..2);
        live.push(nodes.len() - 1);
    }
    let mut len = vec![0u8; f.len()];
    let mut stack = vec![(nodes.len() - 1, 0u8)];
    while let Some((i, d)) = stack.pop() {
        match nodes[i].1 {
            Some((a, b)) => {
                stack.push((a, d + 1));
                stack.push((b, d + 1));
            }
            None => len[nodes[i].2] = d.max(1),
        }
    }
    len
}

/// Canonical codes for lengths, bit-reversed for least-significant-first writing.
fn codes(len: &[u8]) -> Vec<u32> {
    let max = *len.iter().max().unwrap_or(&0) as usize;
    let mut count = vec![0u32; max + 2];
    for &l in len {
        if l > 0 {
            count[l as usize] += 1;
        }
    }
    let mut next = vec![0u32; max + 2];
    let mut code = 0u32;
    for b in 1..=max {
        code = (code + count[b - 1]) << 1;
        next[b] = code;
    }
    len.iter()
        .map(|&l| {
            if l == 0 {
                return 0;
            }
            let c = next[l as usize];
            next[l as usize] += 1;
            c.reverse_bits() >> (32 - l as u32)
        })
        .collect()
}

/// A prefix code ready to write symbols with.
struct Code {
    len: Vec<u8>,
    code: Vec<u32>,
}

impl Code {
    fn put(&self, b: &mut Bits, s: usize) {
        b.put(self.code[s], self.len[s] as u32);
    }
}

const CODE_LENGTH_ORDER: [usize; 19] = [
    17, 18, 0, 1, 2, 3, 4, 5, 16, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15,
];

/// Write a prefix code for these frequencies and return it. One or two used symbols below 256
/// use the simple form (one symbol costs no bits at all).
fn write_code(b: &mut Bits, freq: &[u32]) -> Code {
    let used: Vec<usize> = (0..freq.len()).filter(|&s| freq[s] > 0).collect();
    if used.len() <= 2 && used.iter().all(|&s| s < 256) {
        let syms = if used.is_empty() { vec![0] } else { used };
        b.put(1, 1); // simple
        b.put(syms.len() as u32 - 1, 1);
        if syms[0] < 2 {
            b.put(0, 1);
            b.put(syms[0] as u32, 1);
        } else {
            b.put(1, 1);
            b.put(syms[0] as u32, 8);
        }
        if syms.len() == 2 {
            b.put(syms[1] as u32, 8);
        }
        let mut len = vec![0u8; freq.len()];
        if syms.len() == 2 {
            len[syms[0]] = 1;
            len[syms[1]] = 1;
        }
        let code = codes(&len);
        return Code { len, code };
    }
    let len = lengths(freq, 15);
    // Run-length code the lengths: 17 and 18 for runs of zeros, literal lengths otherwise.
    let mut tokens: Vec<(u8, u32, u32)> = Vec::new(); // (symbol, extra value, extra bits)
    let mut i = 0;
    while i < len.len() {
        if len[i] == 0 {
            let mut j = i;
            while j < len.len() && len[j] == 0 && j - i < 138 {
                j += 1;
            }
            let run = (j - i) as u32;
            if run >= 11 {
                tokens.push((18, run - 11, 7));
            } else if run >= 3 {
                tokens.push((17, run - 3, 3));
            } else {
                for _ in 0..run {
                    tokens.push((0, 0, 0));
                }
            }
            i = j;
        } else {
            tokens.push((len[i], 0, 0));
            i += 1;
        }
    }
    let mut cf = [0u32; 19];
    for t in &tokens {
        cf[t.0 as usize] += 1;
    }
    let cl = lengths(&cf, 7);
    let cc = codes(&cl);
    let n = (0..19)
        .rev()
        .find(|&k| cl[CODE_LENGTH_ORDER[k]] > 0)
        .map(|k| k + 1)
        .unwrap_or(0)
        .max(4);
    b.put(0, 1); // normal
    b.put(n as u32 - 4, 4);
    for &k in &CODE_LENGTH_ORDER[..n] {
        b.put(cl[k] as u32, 3);
    }
    b.put(0, 1); // max_symbol: every symbol
    for (s, v, nb) in tokens {
        b.put(cc[s as usize], cl[s as usize] as u32);
        b.put(v, nb);
    }
    let code = codes(&len);
    Code { len, code }
}

/// LZ77 prefix of a length or distance value (>= 1): (prefix symbol, extra bits, extra value).
fn prefix(v: u32) -> (usize, u32, u32) {
    if v < 5 {
        return ((v - 1) as usize, 0, 0);
    }
    let v1 = v - 1;
    let hb = 31 - v1.leading_zeros();
    let second = (v1 >> (hb - 1)) & 1;
    let eb = hb - 1;
    ((2 * hb + second) as usize, eb, v1 & ((1 << eb) - 1))
}

#[derive(Clone, Copy)]
enum Sym {
    Lit(u32),
    /// length, distance code (1 = the pixel above, 2 = the pixel to the left).
    Copy(u32, u32),
}

/// Greedy LZ77 over packed pixels: copies from the pixel above or to the left, at least 3 long.
fn lz77(px: &[u32], w: usize) -> Vec<Sym> {
    let mut out = Vec::new();
    let mut i = 0;
    let n = px.len();
    while i < n {
        let run = |d: usize| -> usize {
            if i < d {
                return 0;
            }
            let mut k = 0;
            while i + k < n && k < 4096 && px[i + k] == px[i + k - d] {
                k += 1;
            }
            k
        };
        let (up, left) = (run(w), run(1));
        let (best, code) = if up >= left { (up, 1) } else { (left, 2) };
        if best >= 3 {
            out.push(Sym::Copy(best as u32, code));
            i += best;
        } else {
            out.push(Sym::Lit(px[i]));
            i += 1;
        }
    }
    out
}

/// One entropy-coded image (no colour cache; `meta`: the main image's "no meta codes" bit).
fn write_image(b: &mut Bits, px: &[u32], w: usize, meta: bool) {
    let syms = lz77(px, w);
    let mut fg = vec![0u32; 256 + 24];
    let mut fr = vec![0u32; 256];
    let mut fb = vec![0u32; 256];
    let mut fa = vec![0u32; 256];
    let mut fd = vec![0u32; 40];
    for s in &syms {
        match *s {
            Sym::Lit(p) => {
                fg[((p >> 8) & 0xff) as usize] += 1;
                fr[((p >> 16) & 0xff) as usize] += 1;
                fb[(p & 0xff) as usize] += 1;
                fa[(p >> 24) as usize] += 1;
            }
            Sym::Copy(l, d) => {
                fg[256 + prefix(l).0] += 1;
                fd[prefix(d).0] += 1;
            }
        }
    }
    b.put(0, 1); // no colour cache
    if meta {
        b.put(0, 1); // one prefix code group
    }
    let cg = write_code(b, &fg);
    let cr = write_code(b, &fr);
    let cb = write_code(b, &fb);
    let ca = write_code(b, &fa);
    let cd = write_code(b, &fd);
    for s in syms {
        match s {
            Sym::Lit(p) => {
                cg.put(b, ((p >> 8) & 0xff) as usize);
                cr.put(b, ((p >> 16) & 0xff) as usize);
                cb.put(b, (p & 0xff) as usize);
                ca.put(b, (p >> 24) as usize);
            }
            Sym::Copy(l, d) => {
                let (p, eb, ev) = prefix(l);
                cg.put(b, 256 + p);
                b.put(ev, eb);
                let (p, eb, ev) = prefix(d);
                cd.put(b, p);
                b.put(ev, eb);
            }
        }
    }
}

/// The VP8L bitstream (the payload of a `VP8L` chunk) for an opaque image with at most 256
/// colours, or `None` when it has more.
pub fn encode(img: &Image) -> Option<Vec<u8>> {
    let (w, h) = (img.width as usize, img.height as usize);
    if w == 0 || h == 0 || w > 16384 || h > 16384 {
        return None;
    }
    // Palette in first-seen order, as ARGB.
    let mut pal: Vec<u32> = Vec::new();
    let mut idx = Vec::with_capacity(w * h);
    let mut last = (u32::MAX, 0u8);
    for p in img.rgba.as_chunks::<4>().0 {
        let argb = 0xff00_0000 | (p[0] as u32) << 16 | (p[1] as u32) << 8 | p[2] as u32;
        if argb != last.0 {
            let k = match pal.iter().position(|&c| c == argb) {
                Some(k) => k,
                None => {
                    if pal.len() == 256 {
                        return None;
                    }
                    pal.push(argb);
                    pal.len() - 1
                }
            };
            last = (argb, k as u8);
        }
        idx.push(last.1);
    }
    let xbits = match pal.len() {
        0..=2 => 3,
        3..=4 => 2,
        5..=16 => 1,
        _ => 0,
    };
    let per = 1usize << xbits;
    let bpp = 8 >> xbits;
    let pw = w.div_ceil(per);
    let mut packed = vec![0u32; pw * h];
    for y in 0..h {
        for x in 0..w {
            let v = idx[y * w + x] as u32;
            let o = y * pw + (x >> xbits);
            packed[o] |= v << (bpp * (x & (per - 1)));
        }
    }
    for p in &mut packed {
        *p = 0xff00_0000 | (*p << 8);
    }
    let mut b = Bits::new();
    b.put(0x2f, 8);
    b.put(w as u32 - 1, 14);
    b.put(h as u32 - 1, 14);
    b.put(0, 1); // alpha unused
    b.put(0, 3); // version
    // Colour-indexing transform: present, type 3, palette size, palette (delta coded).
    b.put(1, 1);
    b.put(3, 2);
    b.put(pal.len() as u32 - 1, 8);
    let mut delta = Vec::with_capacity(pal.len());
    let mut prev = 0u32;
    for &c in &pal {
        let d = (0..4).fold(0u32, |acc, k| {
            let a = (c >> (8 * k)) & 0xff;
            let p = (prev >> (8 * k)) & 0xff;
            acc | ((a.wrapping_sub(p) & 0xff) << (8 * k))
        });
        delta.push(d);
        prev = c;
    }
    write_image(&mut b, &delta, pal.len(), false);
    b.put(0, 1); // no more transforms
    write_image(&mut b, &packed, pw, true);
    Some(b.finish())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prefix_coding_matches_the_spec_decoder() {
        for v in 1..20_000u32 {
            let (p, eb, ev) = prefix(v);
            // The decoder's inverse (RFC 9649 section 5.2.2).
            let back = if p < 4 {
                p as u32 + 1
            } else {
                let extra = (p as u32 - 2) >> 1;
                let offset = (2 + (p as u32 & 1)) << extra;
                offset + ev + 1
            };
            assert_eq!(back, v, "value {v}");
            assert_eq!(eb, if p < 4 { 0 } else { (p as u32 - 2) >> 1 });
        }
    }

    #[test]
    fn decodes_with_image_webp() {
        for colours in [1u32, 2, 3, 5, 17, 200] {
            let (w, h) = (53u32, 31u32);
            let mut rgba = Vec::new();
            for y in 0..h {
                for x in 0..w {
                    let c = ((x / 3 + y / 2 * 7) * 13) % colours;
                    rgba.extend([(c * 37) as u8, (c * 91) as u8, !((c * 11) as u8), 255]);
                }
            }
            let img = Image {
                width: w,
                height: h,
                rgba,
            };
            let data = encode(&img).unwrap();
            let mut file = b"RIFF".to_vec();
            let pad = data.len() % 2;
            file.extend(((data.len() + pad + 12) as u32).to_le_bytes());
            file.extend(b"WEBPVP8L");
            file.extend((data.len() as u32).to_le_bytes());
            file.extend(&data);
            file.extend(std::iter::repeat_n(0, pad));
            let mut d = image_webp::WebPDecoder::new(std::io::Cursor::new(file)).unwrap();
            let mut buf = vec![0; (w * h * 3) as usize];
            d.read_image(&mut buf).unwrap();
            let want: Vec<u8> = img
                .rgba
                .as_chunks::<4>()
                .0
                .iter()
                .flat_map(|p| [p[0], p[1], p[2]])
                .collect();
            assert!(buf == want, "{colours} colours");
        }
    }
}
