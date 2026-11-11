"""A store on disk (pack spec section 4), read without trusting it.

File names are never taken from a pack (section 17): every path is built here from a validated hash,
an extension from the allowlist, a feed name or a key fingerprint. Symbolic links and anything that
is not a regular file are refused, so a store cannot point the reader at /dev/zero or outside itself,
and nothing is read past the size limit.
"""

import hashlib
import os
import re
import stat

from . import safejson
from .limits import DEFAULT

HASH_RE = re.compile(r"sha256:[0-9a-f]{64}\Z")
EXTENSIONS = ("json", "parquet", "png", "gif", "webp")
FEED_RE = re.compile(r"[A-Za-z0-9-]{1,200}\Z")
FINGERPRINT_RE = re.compile(r"SHA256:[A-Za-z0-9+/]{43}\Z")
SIGNER_RE = re.compile(r"SHA256:[A-Za-z0-9_-]{43}\Z")
_COMPONENT_RE = re.compile(r"[A-Za-z0-9_:.-]{1,255}\Z")
_CHUNK = 1 << 20


class StoreError(Exception):
    def __init__(self, code, message):
        super().__init__(message)
        self.code = code
        self.message = message


def signer_name(fingerprint):
    """A fingerprint made filename-safe (section 7): `/` becomes `_` and `+` becomes `-`."""
    if not FINGERPRINT_RE.match(fingerprint or ""):
        raise StoreError("bad-fingerprint", f"{fingerprint!r} is not a SHA256 key fingerprint")
    return fingerprint.replace("/", "_").replace("+", "-")


def fingerprint_of_signer(name):
    if not SIGNER_RE.match(name):
        raise StoreError("bad-signer-name", f"{name!r} is not a filename-safe fingerprint")
    return name.replace("_", "/").replace("-", "+")


def check_hash(h):
    if not isinstance(h, str):
        raise StoreError("bad-hash", f"{h!r} is not a hash")
    if not h.startswith("sha256:"):
        prefix = h.split(":", 1)[0] if ":" in h else h[:20]
        raise StoreError("unknown-hash-algorithm", f"hash algorithm {prefix!r} is not known")
    if not HASH_RE.match(h):
        raise StoreError("bad-hash", f"{h[:80]!r} is not sha256: and 64 lower-case hex digits")
    return h[7:]


class Store:
    def __init__(self, root, limits=DEFAULT):
        self.root = os.fspath(root)
        self.limits = limits
        if not os.path.isdir(self.root):
            raise StoreError("not-a-store", f"{self.root} is not a folder")

    # Paths ---------------------------------------------------------------------------------

    def path(self, *components):
        for c in components:
            if not _COMPONENT_RE.match(c) or c in (".", "..") or c.startswith("."):
                raise StoreError("bad-path", f"refusing the path component {c!r}")
        return os.path.join(self.root, *components)

    def object_components(self, h, ext):
        hexd = check_hash(h)
        if ext not in EXTENSIONS:
            raise StoreError("bad-extension", f"extension {ext!r} is not one of {', '.join(EXTENSIONS)}")
        return ("objects", "sha256", hexd[:2], f"{hexd}.{ext}")

    def _check_chain(self, components):
        """Refuse a symbolic link anywhere below the store's root."""
        p = self.root
        st = None
        for n, c in enumerate(components):
            p = os.path.join(p, c)
            try:
                st = os.lstat(p)
            except FileNotFoundError:
                raise StoreError("missing", f"{'/'.join(components)} is missing") from None
            except OSError as e:
                raise StoreError("unreadable", f"{'/'.join(components)}: {e.strerror}") from None
            if stat.S_ISLNK(st.st_mode):
                raise StoreError("symlink", f"{'/'.join(components[:n + 1])} is a symbolic link; "
                                            "a store's files must be plain files")
        return p, st

    def open(self, components, max_bytes):
        """Open a plain file of the store; returns (file, size)."""
        components = tuple(components)
        self.path(*components)
        p, st = self._check_chain(components)
        name = "/".join(components)
        if not stat.S_ISREG(st.st_mode):
            raise StoreError("not-a-file", f"{name} is not a plain file")
        if st.st_size > max_bytes:
            raise StoreError("too-large", f"{name} is {st.st_size:,} bytes, over the limit of {max_bytes:,}")
        flags = os.O_RDONLY | getattr(os, "O_NOFOLLOW", 0) | getattr(os, "O_NONBLOCK", 0) | getattr(os, "O_BINARY", 0)
        try:
            fd = os.open(p, flags)
        except OSError as e:
            raise StoreError("unreadable", f"{name}: {e.strerror}") from None
        st = os.fstat(fd)
        if not stat.S_ISREG(st.st_mode) or st.st_size > max_bytes:
            os.close(fd)
            raise StoreError("not-a-file", f"{name} changed while being opened")
        return os.fdopen(fd, "rb"), st.st_size

    def read(self, components, max_bytes):
        f, _ = self.open(components, max_bytes)
        with f:
            data = f.read(max_bytes + 1)
        if len(data) > max_bytes:
            raise StoreError("too-large", f"{'/'.join(components)} grew past the limit while being read")
        return data

    def exists(self, components):
        try:
            self._check_chain(tuple(components))
            return True
        except StoreError:
            return False

    def list(self, components):
        """Names in a folder of the store (plain files only), sorted; [] if it is missing."""
        components = tuple(components)
        if components:
            self.path(*components)
            try:
                p, st = self._check_chain(components)
            except StoreError as e:
                if e.code == "missing":
                    return []
                raise
            if not stat.S_ISDIR(st.st_mode):
                return []
        else:
            p = self.root
        with os.scandir(p) as it:
            return sorted(e.name for e in it)

    # Objects -------------------------------------------------------------------------------

    def read_object(self, h, ext, max_bytes=None):
        """The bytes of an object, checked against its hash."""
        max_bytes = self.limits.document_bytes if max_bytes is None else max_bytes
        comps = self.object_components(h, ext)
        data = self.read(comps, max_bytes)
        actual = "sha256:" + hashlib.sha256(data).hexdigest()
        if actual != h:
            raise StoreError("object-hash", f"{'/'.join(comps)} hashes to {actual}, not its name")
        return data

    def open_object(self, h, ext, max_bytes=None):
        """Open a large object, checked against its hash first; returns (file at 0, size)."""
        max_bytes = self.limits.object_bytes if max_bytes is None else max_bytes
        comps = self.object_components(h, ext)
        f, size = self.open(comps, max_bytes)
        digest = hashlib.sha256()
        read = 0
        while True:
            chunk = f.read(_CHUNK)
            if not chunk:
                break
            read += len(chunk)
            if read > max_bytes:
                f.close()
                raise StoreError("too-large", f"{'/'.join(comps)} grew past the limit while being read")
            digest.update(chunk)
        if "sha256:" + digest.hexdigest() != h:
            f.close()
            raise StoreError("object-hash", f"{'/'.join(comps)} hashes to sha256:{digest.hexdigest()}, "
                                            "not its name")
        f.seek(0)
        return f, read

    def read_json_object(self, h, what):
        data = self.read_object(h, "json", self.limits.document_bytes)
        try:
            value = safejson.loads(data, max_bytes=self.limits.document_bytes,
                                   max_depth=self.limits.json_depth, what=what)
        except safejson.JsonError as e:
            raise StoreError("bad-json", str(e)) from None
        return data, value

    def read_json_file(self, components, what):
        data = self.read(components, self.limits.document_bytes)
        try:
            value = safejson.loads(data, max_bytes=self.limits.document_bytes,
                                   max_depth=self.limits.json_depth, what=what)
        except safejson.JsonError as e:
            raise StoreError("bad-json", str(e)) from None
        return data, value

    def object_files(self):
        """Every file under objects/sha256/, as (components, problem or None)."""
        out = []
        try:
            folders = self.list(("objects", "sha256"))
        except StoreError as e:
            return [(("objects", "sha256"), e.message)]
        for aa in folders:
            if not re.fullmatch(r"[0-9a-f]{2}", aa):
                out.append((("objects", "sha256", aa), "not a two-hex-digit folder"))
                continue
            try:
                names = self.list(("objects", "sha256", aa))
            except StoreError as e:
                out.append((("objects", "sha256", aa), e.message))
                continue
            for name in names:
                m = re.fullmatch(r"([0-9a-f]{64})\.([a-z]+)", name)
                if not m:
                    out.append((("objects", "sha256", aa, name), "not named <64 hex>.<ext>"))
                elif m.group(2) not in EXTENSIONS:
                    out.append((("objects", "sha256", aa, name), f"extension {m.group(2)!r} is not allowed"))
                elif not m.group(1).startswith(aa):
                    out.append((("objects", "sha256", aa, name), f"is in folder {aa}, not {m.group(1)[:2]}"))
                else:
                    out.append((("objects", "sha256", aa, name), None))
        return out
