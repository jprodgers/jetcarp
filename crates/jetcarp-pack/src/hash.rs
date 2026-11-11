//! Object names: `sha256:` and 64 lower-case hex digits of SHA-256 over an object's bytes.

use sha2::{Digest, Sha256};
use std::io::{Read, Write};
use std::path::Path;

/// The prefix of every hash this format writes (another algorithm would get its own).
pub const PREFIX: &str = "sha256:";

/// 64 lower-case hex digits of SHA-256.
pub fn hex(bytes: &[u8]) -> String {
    to_hex(&Sha256::digest(bytes))
}

fn to_hex(d: &[u8]) -> String {
    const H: &[u8; 16] = b"0123456789abcdef";
    let mut s = String::with_capacity(d.len() * 2);
    for b in d {
        s.push(H[(b >> 4) as usize] as char);
        s.push(H[(b & 15) as usize] as char);
    }
    s
}

/// `sha256:<hex>` of some bytes.
pub fn of(bytes: &[u8]) -> String {
    format!("{PREFIX}{}", hex(bytes))
}

/// Is this 64 lower-case hex digits?
pub fn is_hex64(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

/// The hex digits of a `sha256:` hash, refusing another prefix or a malformed one.
pub fn hex_of(h: &str) -> Result<&str, String> {
    match h.strip_prefix(PREFIX) {
        Some(x) if is_hex64(x) => Ok(x),
        Some(_) => Err(format!("malformed hash '{}'", short(h))),
        None => Err(format!(
            "unknown hash algorithm in '{}' (this reader knows sha256)",
            short(h)
        )),
    }
}

fn short(s: &str) -> String {
    s.chars().take(80).collect()
}

/// A writer that hashes what goes through it.
pub struct HashWriter<W: Write> {
    inner: W,
    h: Sha256,
    n: u64,
}

impl<W: Write> HashWriter<W> {
    pub fn new(inner: W) -> HashWriter<W> {
        HashWriter {
            inner,
            h: Sha256::new(),
            n: 0,
        }
    }
    /// Bytes written so far.
    pub fn written(&self) -> u64 {
        self.n
    }
    /// The inner writer, the hex hash and the byte count.
    pub fn finish(self) -> (W, String, u64) {
        (self.inner, to_hex(&self.h.finalize()), self.n)
    }
}

impl<W: Write> Write for HashWriter<W> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let k = self.inner.write(buf)?;
        self.h.update(&buf[..k]);
        self.n += k as u64;
        Ok(k)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        self.inner.flush()
    }
}

/// The hex hash and size of a file, read in pieces (at most `max` bytes, else an error).
pub fn file_hex(path: &Path, max: u64) -> Result<(String, u64), String> {
    let mut f = std::fs::File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut h = Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    let mut n = 0u64;
    loop {
        let k = f
            .read(&mut buf)
            .map_err(|e| format!("{}: {e}", path.display()))?;
        if k == 0 {
            break;
        }
        n += k as u64;
        if n > max {
            return Err(format!("{}: larger than {max} bytes", path.display()));
        }
        h.update(&buf[..k]);
    }
    Ok((to_hex(&h.finalize()), n))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hashes_are_prefixed_hex() {
        assert_eq!(
            of(b"abc"),
            "sha256:ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(hex(b""), jetcarp_core::sha256::sha256_hex(b""));
        assert!(hex_of(&of(b"x")).is_ok());
        assert!(hex_of("sha512:00").unwrap_err().contains("unknown hash"));
        assert!(hex_of("sha256:ABC").is_err());
        assert!(hex_of(&format!("sha256:{}", "A".repeat(64))).is_err());
        let mut w = HashWriter::new(Vec::new());
        w.write_all(b"abc").unwrap();
        let (v, h, n) = w.finish();
        assert_eq!(
            (v.as_slice(), h.as_str(), n),
            (&b"abc"[..], &of(b"abc")[7..], 3)
        );
    }
}
