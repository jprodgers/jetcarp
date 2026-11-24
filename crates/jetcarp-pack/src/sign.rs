//! SSH signatures (the `sshsig` format `ssh-keygen -Y sign` makes) with Ed25519 keys, in pure
//! Rust: making and keeping the publisher's key, signing manifests, refs and key statements,
//! and checking signatures without `ssh-keygen`.
//!
//! Namespaces: `jetcarp-manifest` (a manifest's exact bytes), `jetcarp-ref` and `jetcarp-key`
//! (the canonical JSON of a ref or key statement without its `signature` member). The message
//! is hashed with SHA-512, as `ssh-keygen` does by default.

use ssh_key::{Algorithm, HashAlg, LineEnding, PrivateKey, PublicKey, SshSig};
use std::path::{Path, PathBuf};

pub const NS_MANIFEST: &str = "jetcarp-manifest";
pub const NS_REF: &str = "jetcarp-ref";
pub const NS_KEY: &str = "jetcarp-key";
pub const NS_RECEIPT: &str = "jetcarp-receipt";

/// The largest signature file read.
pub const MAX_SIG: u64 = 64 << 10;

/// A publisher's signing key.
#[derive(Clone)]
pub struct Signer {
    key: PrivateKey,
    fingerprint: String,
}

impl std::fmt::Debug for Signer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Signer({})", self.fingerprint)
    }
}

fn err(x: ssh_key::Error) -> String {
    format!("ssh key: {x}")
}

impl Signer {
    /// A signer from an Ed25519 private key in OpenSSH format (unencrypted).
    pub fn from_openssh(text: &str) -> Result<Signer, String> {
        let key = PrivateKey::from_openssh(text).map_err(err)?;
        if key.is_encrypted() {
            return Err("the key is encrypted with a passphrase; JETCARP needs an unencrypted Ed25519 key for signing".into());
        }
        if key.algorithm() != Algorithm::Ed25519 {
            return Err(format!(
                "packs are signed with Ed25519 keys, not {}",
                key.algorithm()
            ));
        }
        let fingerprint = key.public_key().fingerprint(HashAlg::Sha256).to_string();
        Ok(Signer { key, fingerprint })
    }

    /// A signer from a 32-byte Ed25519 seed (tests and golden packs: the same key every time).
    pub fn from_seed(seed: &[u8; 32], comment: &str) -> Signer {
        let pair = ssh_key::private::Ed25519Keypair::from_seed(seed);
        let mut key = PrivateKey::from(pair);
        key.set_comment(comment);
        let fingerprint = key.public_key().fingerprint(HashAlg::Sha256).to_string();
        Signer { key, fingerprint }
    }

    /// A new random key.
    pub fn generate(comment: &str) -> Result<Signer, String> {
        let mut seed = [0u8; 32];
        getrandom(&mut seed)?;
        Ok(Signer::from_seed(&seed, comment))
    }

    /// `SHA256:<base64>`, as `ssh-keygen -l` prints it.
    pub fn fingerprint(&self) -> &str {
        &self.fingerprint
    }

    /// The public key, `ssh-ed25519 AAAA... comment`.
    pub fn public_openssh(&self) -> String {
        self.key.public_key().to_openssh().unwrap_or_default()
    }

    /// The private key in OpenSSH format.
    pub fn private_openssh(&self) -> Result<String, String> {
        Ok(self
            .key
            .to_openssh(LineEnding::LF)
            .map_err(err)?
            .to_string())
    }

    /// A detached signature (armored, ending with a newline) over `msg` in a namespace.
    pub fn sign(&self, namespace: &str, msg: &[u8]) -> Result<String, String> {
        let sig = self
            .key
            .sign(namespace, HashAlg::Sha512, msg)
            .map_err(err)?;
        sig.to_pem(LineEnding::LF).map_err(err)
    }
}

fn getrandom(buf: &mut [u8]) -> Result<(), String> {
    use ssh_key::rand_core::{OsRng, RngCore};
    OsRng
        .try_fill_bytes(buf)
        .map_err(|e| format!("no random numbers: {e}"))
}

/// The fingerprint of an OpenSSH public key line.
pub fn fingerprint_of(openssh: &str) -> Result<String, String> {
    let k = PublicKey::from_openssh(openssh.trim()).map_err(err)?;
    Ok(k.fingerprint(HashAlg::Sha256).to_string())
}

/// Check an armored signature over `msg` in `namespace`. Returns the fingerprint of the key that
/// made it (taken from the signature itself); the caller decides whether that key may sign.
/// Only Ed25519 signatures are accepted.
pub fn verify(pem: &str, namespace: &str, msg: &[u8]) -> Result<String, String> {
    if pem.len() as u64 > MAX_SIG {
        return Err("signature file too large".into());
    }
    let sig = SshSig::from_pem(pem.trim_end().as_bytes()).map_err(err)?;
    if sig.algorithm() != Algorithm::Ed25519 {
        return Err(format!(
            "signature made with {}; packs are signed with Ed25519 keys",
            sig.algorithm()
        ));
    }
    let key = PublicKey::from(sig.public_key().clone());
    key.verify(namespace, msg, &sig)
        .map_err(|e| format!("signature does not verify ({namespace}): {e}"))?;
    Ok(key.fingerprint(HashAlg::Sha256).to_string())
}

/// [`verify`], and the signing key must be this OpenSSH public key.
pub fn verify_by(pem: &str, namespace: &str, msg: &[u8], openssh: &str) -> Result<(), String> {
    let fp = verify(pem, namespace, msg)?;
    let want = fingerprint_of(openssh)?;
    if fp != want {
        return Err(format!("signed by {fp}, not by {want}"));
    }
    Ok(())
}

/// A key's name in file names (`sigs/.../<signer>.sig`, `keys/<signer>.json`): the 64
/// lower-case hex digits of its SHA-256 digest, the digest its `SHA256:<base64>` fingerprint
/// writes in base64. No colon (Windows forbids it in names), and no case-only differences (two
/// base64 names may differ only in case, which case-insensitive file systems would merge).
/// An unreadable fingerprint gives a name no file has.
pub fn signer_name(fingerprint: &str) -> String {
    use std::str::FromStr;
    ssh_key::Fingerprint::from_str(fingerprint)
        .ok()
        .and_then(|f| f.sha256())
        .map(|d| crate::hash::to_hex(&d))
        .unwrap_or_else(|| "not-a-fingerprint".into())
}

/// Is this a fingerprint as `ssh-keygen -l` prints a SHA-256 one?
pub fn valid_fingerprint(s: &str) -> bool {
    s.strip_prefix("SHA256:").is_some_and(|b| {
        (40..=48).contains(&b.len())
            && b.bytes()
                .all(|c| c.is_ascii_alphanumeric() || c == b'+' || c == b'/')
    })
}

/// Where JETCARP keeps the publisher's key: `<config>/pack/`.
pub fn key_dir() -> Result<PathBuf, String> {
    Ok(jetcarp_core::paths::config_dir()
        .ok_or("this system has no settings folder for the publisher key (set JETCARP_CONFIG_DIR)")?
        .join("pack"))
}

/// The publisher key's file in the settings folder.
pub fn key_file() -> Result<PathBuf, String> {
    Ok(key_dir()?.join("publisher_ed25519"))
}

/// Load the key at `path` (default: the settings folder's), making one there if there is none
/// and `create`. Returns the signer and whether it was made now.
pub fn load_or_create(path: Option<&Path>, create: bool) -> Result<(Signer, bool), String> {
    let default = key_file()?;
    let p = path.map(Path::to_path_buf).unwrap_or(default.clone());
    if p.exists() {
        let text = std::fs::read_to_string(&p).map_err(|e| format!("{}: {e}", p.display()))?;
        return Ok((Signer::from_openssh(&text)?, false));
    }
    if path.is_some() && p != default {
        return Err(format!("no key at {}", p.display()));
    }
    if !create {
        return Err("no publisher key yet: `jetcarp pack key` makes one".into());
    }
    make_at(&p)
}

/// Make a new key at `p` (none was there when looked for), with its `.pub` beside it. Two
/// programs may do this at once (#332): the key is written whole into a file of this program's
/// own, then linked into place, which fails if the name is taken, so `p` never holds half a key,
/// and the program that loses uses the key the other made (returned as not made now).
fn make_at(p: &Path) -> Result<(Signer, bool), String> {
    let s = Signer::generate("jetcarp publisher")?;
    let dir = p.parent().unwrap_or(Path::new("."));
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let at = |e: std::io::Error| format!("{}: {e}", p.display());
    let private = s.private_openssh()?;
    let tmp = own_name(p);
    write_private(&tmp, &private).map_err(|e| format!("{}: {e}", tmp.display()))?;
    let placed = std::fs::hard_link(&tmp, p);
    let _ = std::fs::remove_file(&tmp);
    match placed {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => return made_by_another(p),
        // A folder that can't hold a hard link: make the file in place, as before.
        Err(_) => match write_private(p, &private) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                return made_by_another(p);
            }
            Err(e) => return Err(at(e)),
        },
    }
    // The public half, beside it: written whole, then moved into place.
    let public = p.with_extension("pub");
    let tmp = own_name(&public);
    std::fs::write(&tmp, format!("{}\n", s.public_openssh()))
        .and_then(|()| std::fs::rename(&tmp, &public))
        .map_err(|e| {
            let _ = std::fs::remove_file(&tmp);
            format!("{}: {e}", public.display())
        })?;
    Ok((s, true))
}

/// The key another program made at `p` while this one was making its own.
fn made_by_another(p: &Path) -> Result<(Signer, bool), String> {
    let text = std::fs::read_to_string(p).map_err(|e| format!("{}: {e}", p.display()))?;
    Ok((Signer::from_openssh(&text)?, false))
}

/// A name beside `p` that only this call of this program uses.
fn own_name(p: &Path) -> PathBuf {
    static N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("key");
    p.with_file_name(format!(".{name}.{}-{n}.tmp", std::process::id()))
}

/// Write `text` to a new file `p` (failing if there is one), readable by this user only.
fn write_private(p: &Path, text: &str) -> std::io::Result<()> {
    use std::io::Write;
    let mut o = std::fs::OpenOptions::new();
    o.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        o.mode(0o600);
    }
    o.open(p)?.write_all(text.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Programs making the key at once in one folder (#332): each ends with the same key, none
    /// fails ("File exists"), and none reads the file before its text is all there.
    #[test]
    fn a_key_made_at_once_by_many_is_one_key() {
        let dir = std::env::temp_dir().join(format!("jetcarp-key-race-{}", std::process::id()));
        for round in 0..20 {
            let _ = std::fs::remove_dir_all(&dir);
            let p = dir.join(format!("r{round}")).join("publisher_ed25519");
            let go = std::sync::Arc::new(std::sync::Barrier::new(16));
            let hands: Vec<_> = (0..16)
                .map(|_| {
                    let (p, go) = (p.clone(), go.clone());
                    std::thread::spawn(move || {
                        go.wait();
                        if p.exists() {
                            let text = std::fs::read_to_string(&p).map_err(|e| e.to_string())?;
                            return Signer::from_openssh(&text).map(|s| (s, false));
                        }
                        make_at(&p)
                    })
                })
                .collect();
            let got: Vec<_> = hands.into_iter().map(|h| h.join().unwrap()).collect();
            let errs: Vec<_> = got.iter().filter_map(|r| r.as_ref().err()).collect();
            assert!(errs.is_empty(), "round {round}: {errs:?}");
            let on_disk = Signer::from_openssh(&std::fs::read_to_string(&p).unwrap()).unwrap();
            for (s, _) in got.iter().map(|r| r.as_ref().unwrap()) {
                assert_eq!(s.fingerprint(), on_disk.fingerprint(), "round {round}");
            }
            let made = got.iter().filter(|r| r.as_ref().unwrap().1).count();
            assert_eq!(made, 1, "round {round}: {made} said they made it");
            let public = std::fs::read_to_string(p.with_extension("pub")).unwrap();
            assert_eq!(public.trim(), on_disk.public_openssh(), "round {round}");
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let mode = std::fs::metadata(&p).unwrap().permissions().mode() & 0o777;
                assert_eq!(mode, 0o600, "round {round}: the key is readable by others");
            }
            let left: Vec<_> = std::fs::read_dir(p.parent().unwrap())
                .unwrap()
                .flatten()
                .filter(|e| e.file_name().to_string_lossy().ends_with(".tmp"))
                .collect();
            assert!(
                left.is_empty(),
                "round {round}: {} files of its own left",
                left.len()
            );
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn signatures_verify_and_name_their_key() {
        let a = Signer::from_seed(&[7u8; 32], "a");
        let b = Signer::from_seed(&[8u8; 32], "b");
        assert!(valid_fingerprint(a.fingerprint()), "{}", a.fingerprint());
        assert_ne!(a.fingerprint(), b.fingerprint());
        // Deterministic: Ed25519 and sshsig have no randomness.
        let s1 = a.sign(NS_MANIFEST, b"hello").unwrap();
        assert_eq!(s1, a.sign(NS_MANIFEST, b"hello").unwrap());
        assert!(s1.starts_with("-----BEGIN SSH SIGNATURE-----\n"), "{s1}");
        assert_eq!(verify(&s1, NS_MANIFEST, b"hello").unwrap(), a.fingerprint());
        assert!(verify_by(&s1, NS_MANIFEST, b"hello", &a.public_openssh()).is_ok());
        assert!(verify_by(&s1, NS_MANIFEST, b"hello", &b.public_openssh()).is_err());
        assert!(verify(&s1, NS_REF, b"hello").is_err());
        assert!(verify(&s1, NS_MANIFEST, b"hellO").is_err());
        assert!(verify("not a signature", NS_MANIFEST, b"hello").is_err());
        // The private key round-trips through OpenSSH format.
        let again = Signer::from_openssh(&a.private_openssh().unwrap()).unwrap();
        assert_eq!(again.fingerprint(), a.fingerprint());
        let name = signer_name(a.fingerprint());
        assert!(crate::hash::is_hex64(&name), "{name}");
        assert_eq!(signer_name("SHA256:ab+c/d"), "not-a-fingerprint");
    }
}
