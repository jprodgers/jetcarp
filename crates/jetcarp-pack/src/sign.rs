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
    let s = Signer::generate("jetcarp publisher")?;
    if let Some(d) = p.parent() {
        std::fs::create_dir_all(d).map_err(|e| format!("{}: {e}", d.display()))?;
    }
    write_private(&p, &s.private_openssh()?)?;
    std::fs::write(p.with_extension("pub"), format!("{}\n", s.public_openssh()))
        .map_err(|e| format!("{}: {e}", p.display()))?;
    Ok((s, true))
}

fn write_private(p: &Path, text: &str) -> Result<(), String> {
    #[cfg(unix)]
    {
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(p)
            .map_err(|e| format!("{}: {e}", p.display()))?;
        f.write_all(text.as_bytes())
            .map_err(|e| format!("{}: {e}", p.display()))
    }
    #[cfg(not(unix))]
    {
        std::fs::write(p, text).map_err(|e| format!("{}: {e}", p.display()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
