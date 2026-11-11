"""SSH signatures (the sshsig format of `ssh-keygen -Y sign`; pack spec section 7).

The signature's armour is parsed here, so a failure can say which key and namespace a signature
claims; the cryptography is left to `ssh-keygen -Y verify`, with an allowed-signers file holding
only the one key the signature must be from.
"""

import base64
import binascii
import hashlib
import os
import shutil
import struct
import subprocess
import tempfile

BEGIN, END = "-----BEGIN SSH SIGNATURE-----", "-----END SSH SIGNATURE-----"


class SigError(ValueError):
    pass


def available():
    return shutil.which("ssh-keygen") is not None


def _strings(blob, pos, n):
    out = []
    for _ in range(n):
        if pos + 4 > len(blob):
            raise SigError("is cut short")
        (length,) = struct.unpack(">I", blob[pos:pos + 4])
        pos += 4
        if pos + length > len(blob):
            raise SigError("is cut short")
        out.append(blob[pos:pos + length])
        pos += length
    return out, pos


def parse_public_key(line):
    """`ssh-ed25519 AAAA... [comment]` -> (type, blob)."""
    if not isinstance(line, str):
        raise SigError("key is not a string")
    parts = line.split()
    if len(parts) < 2:
        raise SigError("key is not '<type> <base64>'")
    try:
        blob = base64.b64decode(parts[1], validate=True)
    except (binascii.Error, ValueError):
        raise SigError("key's base64 is not valid") from None
    (kind,), _ = _strings(blob, 0, 1)
    if kind.decode("ascii", "replace") != parts[0]:
        raise SigError(f"key says {parts[0]} but holds a {kind.decode('ascii', 'replace')} key")
    return parts[0], blob


def fingerprint(blob):
    """The key's SHA256 fingerprint as `ssh-keygen -l` prints it."""
    return "SHA256:" + base64.b64encode(hashlib.sha256(blob).digest()).decode().rstrip("=")


def key_fingerprint(line):
    return fingerprint(parse_public_key(line)[1])


def parse_signature(text):
    """The fields of an armoured SSH signature."""
    if not isinstance(text, str):
        raise SigError("signature is not text")
    lines = [ln.strip() for ln in text.strip().splitlines()]
    if len(lines) < 3 or lines[0] != BEGIN or lines[-1] != END:
        raise SigError("is not an armoured SSH signature")
    try:
        blob = base64.b64decode("".join(lines[1:-1]), validate=True)
    except (binascii.Error, ValueError):
        raise SigError("signature's base64 is not valid") from None
    if blob[:6] != b"SSHSIG" or len(blob) < 10:
        raise SigError("does not start with SSHSIG")
    (version,) = struct.unpack(">I", blob[6:10])
    if version != 1:
        raise SigError(f"is sshsig version {version}, not 1")
    (pub, namespace, _reserved, hash_alg, sig), end = _strings(blob, 10, 5)
    if end != len(blob):
        raise SigError("has bytes after its signature")
    (kind,), _ = _strings(pub, 0, 1)
    return {
        "key_type": kind.decode("ascii", "replace"),
        "key_blob": pub,
        "fingerprint": fingerprint(pub),
        "namespace": namespace.decode("utf-8", "replace"),
        "hash": hash_alg.decode("ascii", "replace"),
    }


def verify(namespace, data, signature, key_line, timeout=30.0):
    """Check `signature` over `data` in `namespace` by the key `key_line`. Returns (ok, message)."""
    try:
        info = parse_signature(signature)
    except SigError as e:
        return False, f"signature {e}"
    try:
        kind, blob = parse_public_key(key_line)
    except SigError as e:
        return False, str(e)
    if info["namespace"] != namespace:
        return False, f"signature is for namespace {info['namespace']!r}, not {namespace!r}"
    if info["key_blob"] != blob:
        return False, f"signature is by key {info['fingerprint']}, not {fingerprint(blob)}"
    if not available():
        return False, "ssh-keygen is not installed"
    with tempfile.TemporaryDirectory(prefix="jetcarp-pack-") as d:
        allowed = os.path.join(d, "allowed_signers")
        sig = os.path.join(d, "signature")
        with open(allowed, "w", encoding="ascii") as f:
            f.write(f"signer {kind} {base64.b64encode(blob).decode()}\n")
        with open(sig, "w", encoding="ascii") as f:
            f.write(signature if signature.endswith("\n") else signature + "\n")
        try:
            r = subprocess.run(["ssh-keygen", "-Y", "verify", "-f", allowed, "-I", "signer", "-n", namespace,
                                "-s", sig], input=data, capture_output=True, timeout=timeout)
        except subprocess.TimeoutExpired:
            return False, f"ssh-keygen took over {timeout:.0f} s"
        except OSError as e:
            return False, f"could not run ssh-keygen: {e}"
    out = (r.stdout + r.stderr).decode("utf-8", "replace").strip().splitlines()
    if r.returncode == 0 and out and out[0].startswith(f'Good "{namespace}" signature'):
        return True, out[0]
    return False, "ssh-keygen: " + (" / ".join(out[-2:]) if out else f"exit {r.returncode}")
