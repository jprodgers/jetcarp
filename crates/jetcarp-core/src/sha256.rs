//! SHA-256 (FIPS 180-4), for content ids and content hashes.
//!
//! Library ids ([`crate::pattern::range::content_id`]), geometry pins
//! ([`crate::topology::Topology::geometry_pin`]), and the content hashes of media and the website
//! export are SHA-256, so ids from many machines can be merged without collisions and a hostile
//! pack cannot make two records share one. FNV-1a stays for what only has to be fast and stable on
//! one machine (state hashes, caches). This is the plain specification, with no dependency, so
//! the saver and the engine need nothing new; the tests check it against the standard's vectors.

/// The initial hash value (FIPS 180-4, 5.3.3).
const H0: [u32; 8] = [
    0x6a09_e667,
    0xbb67_ae85,
    0x3c6e_f372,
    0xa54f_f53a,
    0x510e_527f,
    0x9b05_688c,
    0x1f83_d9ab,
    0x5be0_cd19,
];

/// The round constants (FIPS 180-4, 4.2.2).
const K: [u32; 64] = [
    0x428a_2f98,
    0x7137_4491,
    0xb5c0_fbcf,
    0xe9b5_dba5,
    0x3956_c25b,
    0x59f1_11f1,
    0x923f_82a4,
    0xab1c_5ed5,
    0xd807_aa98,
    0x1283_5b01,
    0x2431_85be,
    0x550c_7dc3,
    0x72be_5d74,
    0x80de_b1fe,
    0x9bdc_06a7,
    0xc19b_f174,
    0xe49b_69c1,
    0xefbe_4786,
    0x0fc1_9dc6,
    0x240c_a1cc,
    0x2de9_2c6f,
    0x4a74_84aa,
    0x5cb0_a9dc,
    0x76f9_88da,
    0x983e_5152,
    0xa831_c66d,
    0xb003_27c8,
    0xbf59_7fc7,
    0xc6e0_0bf3,
    0xd5a7_9147,
    0x06ca_6351,
    0x1429_2967,
    0x27b7_0a85,
    0x2e1b_2138,
    0x4d2c_6dfc,
    0x5338_0d13,
    0x650a_7354,
    0x766a_0abb,
    0x81c2_c92e,
    0x9272_2c85,
    0xa2bf_e8a1,
    0xa81a_664b,
    0xc24b_8b70,
    0xc76c_51a3,
    0xd192_e819,
    0xd699_0624,
    0xf40e_3585,
    0x106a_a070,
    0x19a4_c116,
    0x1e37_6c08,
    0x2748_774c,
    0x34b0_bcb5,
    0x391c_0cb3,
    0x4ed8_aa4a,
    0x5b9c_ca4f,
    0x682e_6ff3,
    0x748f_82ee,
    0x78a5_636f,
    0x84c8_7814,
    0x8cc7_0208,
    0x90be_fffa,
    0xa450_6ceb,
    0xbef9_a3f7,
    0xc671_78f2,
];

/// A running SHA-256: [`update`](Self::update) with the data in pieces, then
/// [`finish`](Self::finish).
#[derive(Clone, Debug)]
pub struct Sha256 {
    h: [u32; 8],
    buf: [u8; 64],
    fill: usize,
    /// Bytes hashed so far.
    len: u64,
}

impl Default for Sha256 {
    fn default() -> Self {
        Sha256 {
            h: H0,
            buf: [0; 64],
            fill: 0,
            len: 0,
        }
    }
}

impl Sha256 {
    pub fn new() -> Sha256 {
        Sha256::default()
    }

    /// Add bytes.
    pub fn update(&mut self, mut data: &[u8]) {
        self.len = self.len.wrapping_add(data.len() as u64);
        if self.fill > 0 {
            let n = (64 - self.fill).min(data.len());
            self.buf[self.fill..self.fill + n].copy_from_slice(&data[..n]);
            self.fill += n;
            data = &data[n..];
            if self.fill < 64 {
                return;
            }
            let block = self.buf;
            self.compress(&block);
            self.fill = 0;
        }
        let mut blocks = data.chunks_exact(64);
        for b in &mut blocks {
            self.compress(b.try_into().unwrap());
        }
        let rest = blocks.remainder();
        self.buf[..rest.len()].copy_from_slice(rest);
        self.fill = rest.len();
    }

    /// Add a string's UTF-8 bytes.
    pub fn text(&mut self, s: &str) {
        self.update(s.as_bytes());
    }

    /// The 32-byte digest.
    pub fn finish(mut self) -> [u8; 32] {
        let bits = self.len.wrapping_mul(8);
        let mut pad = [0u8; 72];
        pad[0] = 0x80;
        // Pad to 56 bytes mod 64, then the length in bits (big-endian).
        let n = if self.fill < 56 {
            56 - self.fill
        } else {
            120 - self.fill
        };
        pad[n..n + 8].copy_from_slice(&bits.to_be_bytes());
        let len = self.len;
        self.update(&pad[..n + 8]);
        debug_assert_eq!(self.fill, 0);
        self.len = len;
        let mut out = [0u8; 32];
        for (i, w) in self.h.iter().enumerate() {
            out[4 * i..4 * i + 4].copy_from_slice(&w.to_be_bytes());
        }
        out
    }

    /// The digest as 64 lower-case hex digits.
    pub fn finish_hex(self) -> String {
        hex(&self.finish())
    }

    fn compress(&mut self, block: &[u8; 64]) {
        let mut w = [0u32; 64];
        for (i, c) in block.chunks_exact(4).enumerate() {
            w[i] = u32::from_be_bytes(c.try_into().unwrap());
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }
        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut h] = self.h;
        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ (!e & g);
            let t1 = h
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(K[i])
                .wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let t2 = s0.wrapping_add(maj);
            h = g;
            g = f;
            f = e;
            e = d.wrapping_add(t1);
            d = c;
            c = b;
            b = a;
            a = t1.wrapping_add(t2);
        }
        for (x, y) in self.h.iter_mut().zip([a, b, c, d, e, f, g, h]) {
            *x = x.wrapping_add(y);
        }
    }
}

/// SHA-256 of bytes.
pub fn sha256(data: &[u8]) -> [u8; 32] {
    let mut s = Sha256::new();
    s.update(data);
    s.finish()
}

/// SHA-256 of bytes as 64 lower-case hex digits.
pub fn sha256_hex(data: &[u8]) -> String {
    hex(&sha256(data))
}

/// Bytes as lower-case hex digits.
pub fn hex(bytes: &[u8]) -> String {
    const D: &[u8; 16] = b"0123456789abcdef";
    let mut s = String::with_capacity(bytes.len() * 2);
    for &b in bytes {
        s.push(D[(b >> 4) as usize] as char);
        s.push(D[(b & 15) as usize] as char);
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    // FIPS 180-4 examples (NIST CSRC "SHA256.pdf" and "SHA2_Additional.pdf").
    #[test]
    fn standard_vectors() {
        assert_eq!(
            sha256_hex(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(
            sha256_hex(b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq"),
            "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1"
        );
        assert_eq!(
            sha256_hex(
                b"abcdefghbcdefghicdefghijdefghijkefghijklfghijklmghijklmnhijklmnoijklmnopjklmnopqklmnopqrlmnopqrsmnopqrstnopqrstu"
            ),
            "cf5b16a778af8380036ce59e7b0492370b249b11e8f07a51afac45037afee9d1"
        );
        let mut s = Sha256::new();
        let chunk = [b'a'; 1000];
        for _ in 0..1000 {
            s.update(&chunk);
        }
        assert_eq!(
            s.finish_hex(),
            "cdc76e5c9914fb9281a1c7e284d73e67f1809a48a497200e046d39ccc7112cd0"
        );
    }

    // Feeding the data in pieces of any size gives the same digest, across the padding edges
    // (55, 56 and 64 bytes).
    #[test]
    fn pieces_and_padding_edges() {
        let data: Vec<u8> = (0..300u32).map(|i| (i * 7 + 3) as u8).collect();
        for len in [0, 1, 55, 56, 57, 63, 64, 65, 119, 120, 128, 300] {
            let want = sha256(&data[..len]);
            for step in [1, 3, 31, 64, 100] {
                let mut s = Sha256::new();
                for c in data[..len].chunks(step) {
                    s.update(c);
                }
                assert_eq!(s.finish(), want, "len {len} step {step}");
            }
        }
        // Exactly one block of data (the padding is a block of its own).
        assert_eq!(
            sha256_hex(&[b'a'; 64]),
            "ffe054fe7ae0cb6dc65c3af9b61d5209f439851db43d0ba5997337df154668eb"
        );
    }
}
