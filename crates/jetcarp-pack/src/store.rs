//! A store: a folder holding a pack's files in the layout of the format.
//!
//! ```text
//! jetcarp-store.json                          {"format":"jetcarp-store","version":"1.0"}
//! refs/<feed>.json                            signed pointers
//! objects/sha256/<aa>/<hex>.<ext>             every immutable file
//! sigs/sha256/<aa>/<hex>/<signer>.sig         detached signatures on manifests
//! keys/<signer>.json                          publisher key statements
//! views.sql                                   optional DuckDB views
//! ```
//!
//! Every path is made from a hash (checked to be 64 hex digits), an extension from the
//! allowlist, a feed name (letters, digits, `-`) or a signer name (a fingerprint made safe):
//! nothing a pack says becomes a path any other way. Objects are written to a temporary file
//! and renamed, never changed once written, and every object read is checked against its name.

use crate::docs::{StoreMarker, VERSION};
use crate::hash::{self, HashWriter};
use std::fs::File;
use std::io::{BufWriter, Read, Write};
use std::path::{Path, PathBuf};

/// The extensions an object may have.
pub const EXTENSIONS: [&str; 5] = ["json", "parquet", "png", "gif", "webp"];

/// Largest object read or written: 4 GiB.
pub const MAX_OBJECT: u64 = 4 << 30;

pub const MARKER: &str = "jetcarp-store.json";
const TMP: &str = ".tmp";

#[derive(Clone, Debug)]
pub struct Store {
    root: PathBuf,
}

fn io(p: &Path, e: std::io::Error) -> String {
    format!("{}: {e}", p.display())
}

pub fn check_ext(ext: &str) -> Result<(), String> {
    if EXTENSIONS.contains(&ext) {
        Ok(())
    } else {
        Err(format!(
            "object extension '{ext}' is not one of {EXTENSIONS:?}"
        ))
    }
}

impl Store {
    /// A store at `root`, made (with its marker) if it does not exist.
    pub fn create(root: &Path) -> Result<Store, String> {
        std::fs::create_dir_all(root).map_err(|e| io(root, e))?;
        let m = root.join(MARKER);
        if !m.exists() {
            let bytes = crate::json::canonical_of(&StoreMarker {
                format: "jetcarp-store".into(),
                version: VERSION.into(),
            })?;
            std::fs::write(&m, bytes).map_err(|e| io(&m, e))?;
        }
        Store::open(root)
    }

    /// An existing store (its marker is checked).
    pub fn open(root: &Path) -> Result<Store, String> {
        let m = root.join(MARKER);
        let bytes = read_limited(&m, crate::json::MAX_DOC)?;
        let mk: StoreMarker = crate::json::parse_as(&bytes, MARKER, false)?;
        crate::docs::check_format(&mk.format, &mk.version, "jetcarp-store")?;
        Ok(Store {
            root: root.to_path_buf(),
        })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// `objects/sha256/<aa>/<hex>.<ext>`.
    pub fn object_rel(hex: &str, ext: &str) -> String {
        format!("objects/sha256/{}/{hex}.{ext}", &hex[..2])
    }

    pub fn object_path(&self, hash: &str, ext: &str) -> Result<PathBuf, String> {
        let hex = hash::hex_of(hash)?;
        check_ext(ext)?;
        Ok(self.root.join(Store::object_rel(hex, ext)))
    }

    pub fn has(&self, hash: &str, ext: &str) -> bool {
        self.object_path(hash, ext).is_ok_and(|p| p.is_file())
    }

    /// Store bytes as an object; returns its hash.
    pub fn put(&self, bytes: &[u8], ext: &str) -> Result<String, String> {
        check_ext(ext)?;
        let h = hash::of(bytes);
        let p = self.object_path(&h, ext)?;
        if p.is_file() {
            return Ok(h);
        }
        let mut w = self.temp()?;
        w.write_all(bytes).map_err(|e| io(&w.path, e))?;
        w.finish_as(self, ext)
    }

    /// Copy a file in as an object, hashing it on the way; `expect` (hex) refuses another hash.
    pub fn put_file(
        &self,
        src: &Path,
        ext: &str,
        expect: Option<&str>,
    ) -> Result<(String, u64), String> {
        check_ext(ext)?;
        if let Some(x) = expect {
            let h = format!("{}{x}", hash::PREFIX);
            let p = self.object_path(&h, ext)?;
            if let (Ok(m), Ok(s)) = (std::fs::metadata(&p), std::fs::metadata(src))
                && m.len() == s.len()
            {
                return Ok((h, m.len()));
            }
        }
        let mut f = File::open(src).map_err(|e| io(src, e))?;
        let mut w = self.temp()?;
        let mut buf = vec![0u8; 1 << 20];
        loop {
            let k = f.read(&mut buf).map_err(|e| io(src, e))?;
            if k == 0 {
                break;
            }
            w.write_all(&buf[..k]).map_err(|e| io(&w.path, e))?;
            if w.written() > MAX_OBJECT {
                return Err(format!("{}: larger than 4 GiB", src.display()));
            }
        }
        let n = w.written();
        let h = w.finish_as(self, ext)?;
        if let Some(x) = expect
            && hash::hex_of(&h)? != x
        {
            return Err(format!(
                "{}: its hash is {h}, not sha256:{x} (the file changed?)",
                src.display()
            ));
        }
        Ok((h, n))
    }

    /// A temporary file in the store that becomes an object when finished.
    pub fn temp(&self) -> Result<TempObject, String> {
        let d = self.root.join(TMP);
        std::fs::create_dir_all(&d).map_err(|e| io(&d, e))?;
        static N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let k = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let path = d.join(format!("{}-{k}.tmp", std::process::id()));
        let f = File::create(&path).map_err(|e| io(&path, e))?;
        Ok(TempObject {
            w: Some(HashWriter::new(BufWriter::with_capacity(1 << 20, f))),
            path,
        })
    }

    /// Remove the temporary folder (after a publish).
    pub fn clean_temp(&self) {
        let d = self.root.join(TMP);
        let _ = std::fs::remove_dir_all(d);
    }

    /// Read an object, checking its size (at most `max`) and its hash.
    pub fn get(&self, hash: &str, ext: &str, max: u64) -> Result<Vec<u8>, String> {
        let p = self.object_path(hash, ext)?;
        let b = read_limited(&p, max)?;
        if hash::of(&b) != hash {
            return Err(format!(
                "object {hash} is altered: its bytes hash to {}",
                hash::of(&b)
            ));
        }
        Ok(b)
    }

    pub fn ref_path(&self, feed: &str) -> Result<PathBuf, String> {
        if !crate::docs::valid_feed(feed) {
            return Err(format!("feed name '{feed}' is not letters, digits and -"));
        }
        Ok(self.root.join("refs").join(format!("{feed}.json")))
    }

    pub fn key_path(&self, fingerprint: &str) -> Result<PathBuf, String> {
        if !crate::sign::valid_fingerprint(fingerprint) {
            return Err(format!("'{fingerprint}' is not a SHA256 key fingerprint"));
        }
        Ok(self
            .root
            .join("keys")
            .join(format!("{}.json", crate::sign::signer_name(fingerprint))))
    }

    /// The folder of a manifest's signatures.
    pub fn sig_dir(&self, manifest: &str) -> Result<PathBuf, String> {
        let hex = hash::hex_of(manifest)?;
        Ok(self.root.join(format!("sigs/sha256/{}/{hex}", &hex[..2])))
    }

    /// The signatures on a manifest: (file stem, armored text), sorted by name.
    pub fn signatures(&self, manifest: &str) -> Result<Vec<(String, String)>, String> {
        let d = self.sig_dir(manifest)?;
        let Ok(rd) = std::fs::read_dir(&d) else {
            return Ok(vec![]);
        };
        let mut out = vec![];
        for e in rd.flatten().take(1000) {
            let name = e.file_name().to_string_lossy().to_string();
            let Some(stem) = name.strip_suffix(".sig") else {
                continue;
            };
            let b = read_limited(&e.path(), crate::sign::MAX_SIG)?;
            out.push((stem.to_string(), String::from_utf8_lossy(&b).to_string()));
        }
        out.sort();
        Ok(out)
    }

    /// Write a file under the store (refs, keys, signatures, views): to a temporary name, then
    /// renamed.
    pub fn write_file(&self, rel: &Path, bytes: &[u8]) -> Result<(), String> {
        let p = self.root.join(rel);
        write_atomic(&p, bytes)
    }
}

/// Write a file through a temporary name in its folder.
pub fn write_atomic(p: &Path, bytes: &[u8]) -> Result<(), String> {
    if let Some(d) = p.parent() {
        std::fs::create_dir_all(d).map_err(|e| io(d, e))?;
    }
    let tmp = p.with_extension(format!("tmp{}", std::process::id()));
    std::fs::write(&tmp, bytes).map_err(|e| io(&tmp, e))?;
    std::fs::rename(&tmp, p).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        io(p, e)
    })
}

/// Read a whole file, refusing one larger than `max`.
pub fn read_limited(p: &Path, max: u64) -> Result<Vec<u8>, String> {
    let f = File::open(p).map_err(|e| io(p, e))?;
    let len = f.metadata().map_err(|e| io(p, e))?.len();
    if len > max {
        return Err(format!(
            "{}: {len} bytes, larger than the limit of {max}",
            p.display()
        ));
    }
    let mut b = Vec::with_capacity(len as usize);
    f.take(max + 1).read_to_end(&mut b).map_err(|e| io(p, e))?;
    if b.len() as u64 > max {
        return Err(format!("{}: larger than the limit of {max}", p.display()));
    }
    Ok(b)
}

/// A file being written into a store, hashed as it goes.
pub struct TempObject {
    w: Option<HashWriter<BufWriter<File>>>,
    pub path: PathBuf,
}

impl TempObject {
    pub fn written(&self) -> u64 {
        self.w.as_ref().map(|w| w.written()).unwrap_or(0)
    }
    /// Close it and move it to its place; returns its hash.
    pub fn finish_as(mut self, store: &Store, ext: &str) -> Result<String, String> {
        let w = self.w.take().ok_or("finished twice")?;
        let (bw, hex, _) = w.finish();
        let f = bw
            .into_inner()
            .map_err(|e| format!("{}: {e}", self.path.display()))?;
        f.sync_all().map_err(|e| io(&self.path, e))?;
        drop(f);
        let h = format!("{}{hex}", hash::PREFIX);
        let p = store.object_path(&h, ext)?;
        if p.is_file() {
            let _ = std::fs::remove_file(&self.path);
            return Ok(h);
        }
        if let Some(d) = p.parent() {
            std::fs::create_dir_all(d).map_err(|e| io(d, e))?;
        }
        std::fs::rename(&self.path, &p).map_err(|e| io(&p, e))?;
        Ok(h)
    }
}

impl Write for TempObject {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        match &mut self.w {
            Some(w) => w.write(buf),
            None => Err(std::io::Error::other("finished")),
        }
    }
    fn flush(&mut self) -> std::io::Result<()> {
        match &mut self.w {
            Some(w) => w.flush(),
            None => Ok(()),
        }
    }
}

impl Drop for TempObject {
    fn drop(&mut self) {
        if self.w.is_some() {
            self.w = None;
            let _ = std::fs::remove_file(&self.path);
        }
    }
}
