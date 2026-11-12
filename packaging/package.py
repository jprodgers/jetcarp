#!/usr/bin/env python3
"""JETCARP's release packaging: what the release workflow and a local run of packaging/build.sh
share. Python 3.8 or newer, standard library only.

    package.py version                         the version in Cargo.toml
    package.py set-version VERSION             change it (Cargo.toml, Cargo.lock), for example to
                                               0.4.0-rc.1 before tagging v0.4.0-rc.1
    package.py verify --tag TAG [--out FILE]   the checks before a release: the tag is v<version>,
                                               the changelog has the version (dated, unless a
                                               pre-release); writes version= and prerelease= lines
    package.py wheel-data --out DIR [--packs DIR] [--saver FILE]
                                               the extra files a wheel installs (maturin's `data`)
    package.py archive --target T --bins DIR --out DIST [--packs DIR]
                                               the release archive for one target
    package.py check-elf FILE [--max-glibc 2.28]
                                               a Linux program needs only glibc's libraries
    package.py smoke ARCHIVE                   unpack an archive and run its program
    package.py installers --out DIST           the shell and PowerShell installers
    package.py sums DIST                       SHA256SUMS for the files in DIST
    package.py notes --out FILE                release notes from the changelog
    package.py manifests --sums FILE --out DIR the Homebrew formula, the Scoop manifest and the
                                               winget manifests, filled in from SHA256SUMS

Archives are reproducible: entries sorted, owners zeroed, times from SOURCE_DATE_EPOCH (else the
last commit's time).
"""

import argparse
import gzip
import hashlib
import io
import os
import re
import shutil
import subprocess
import sys
import tarfile
import time
import zipfile
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
REPO = "jprodgers/jetcarp"
DOCS_URL = "https://jprodgers.github.io/jetcarp/"
TARGETS = [
    "x86_64-unknown-linux-gnu",
    "aarch64-unknown-linux-gnu",
    "x86_64-pc-windows-msvc",
    "aarch64-apple-darwin",
    "x86_64-apple-darwin",
]
# Beside the program in every archive.
DOC_FILES = [
    "README.md",
    "COPYING",
    "UNLICENSE",
    "LICENSE-MIT",
    "THIRD_PARTY_NOTICES.md",
    "THIRD_PARTY_LICENSES.html",
]
SAVER = "libjetcarp_saver.dylib"
# What a Linux program may link: glibc's own libraries (the rest is opened at run time).
GLIBC_LIBS = {
    "libc.so.6",
    "libm.so.6",
    "libdl.so.2",
    "libpthread.so.0",
    "librt.so.1",
    "libutil.so.1",
    "libgcc_s.so.1",
    "ld-linux-x86-64.so.2",
    "ld-linux-aarch64.so.1",
}


def die(msg):
    print(f"package.py: {msg}", file=sys.stderr)
    sys.exit(1)


def version():
    text = (ROOT / "Cargo.toml").read_text(encoding="utf-8")
    m = re.search(r'^\[workspace\.package\][^\[]*?^version = "([^"]+)"', text, re.M | re.S)
    if not m:
        die("no [workspace.package] version in Cargo.toml")
    return m.group(1)


SEMVER = re.compile(r"^(\d+)\.(\d+)\.(\d+)(?:-([0-9A-Za-z.-]+))?$")


def base_version(ver):
    """0.4.0 for 0.4.0-rc.1: the changelog's section for a release candidate is its release's."""
    m = SEMVER.match(ver)
    if not m:
        die(f"{ver} is not a version like 0.4.0 or 0.4.0-rc.1")
    return ".".join(m.groups()[:3])


def is_prerelease(ver):
    return base_version(ver) != ver


def wheel_version(ver):
    """The version as Python spells it in a wheel's name (PEP 440): 0.4.0rc1 for 0.4.0-rc.1."""
    m = SEMVER.match(ver)
    if not m:
        die(f"{ver} is not a version like 0.4.0 or 0.4.0-rc.1")
    base, pre = ".".join(m.groups()[:3]), m.group(4)
    if not pre:
        return base
    pm = re.fullmatch(r"(alpha|beta|rc|a|b)\.?(\d+)", pre)
    if not pm:
        die(f"{ver}: a pre-release must be alpha.N, beta.N or rc.N for Python to read it")
    return base + {"alpha": "a", "beta": "b"}.get(pm.group(1), pm.group(1)) + pm.group(2)


def cmd_set_version(a):
    new = a.version.lstrip("v")
    base_version(new)
    old = version()
    cargo = ROOT / "Cargo.toml"
    text = cargo.read_text(encoding="utf-8")
    text, n = re.subn(
        r'(^\[workspace\.package\][^\[]*?^version = ")[^"]+(")', rf"\g<1>{new}\2", text, count=1, flags=re.M | re.S
    )
    # The workspace's own crates, pinned exactly in [workspace.dependencies].
    text, pins = re.subn(
        r'^(jetcarp(?:-[a-z]+)? = \{ path = "crates/[a-z-]+", version = "=)[^"]+(" \})$', rf"\g<1>{new}\2", text, flags=re.M
    )
    cargo.write_text(text, encoding="utf-8")
    lock = ROOT / "Cargo.lock"
    ltext = lock.read_text(encoding="utf-8")
    ltext, locked = re.subn(
        rf'(\[\[package\]\]\nname = "jetcarp(?:-[a-z]+)?"\nversion = "){re.escape(old)}(")', rf"\g<1>{new}\2", ltext
    )
    lock.write_text(ltext, encoding="utf-8")
    print(f"{old} -> {new}: Cargo.toml ({n} version, {pins} pins), Cargo.lock ({locked} crates)")


def cmd_verify(a):
    """What a release tag must satisfy before anything is built."""
    ver = version()
    tag = a.tag
    if tag.startswith("refs/tags/"):
        tag = tag[len("refs/tags/") :]
    if tag != f"v{ver}":
        die(f"the tag {tag} does not match Cargo.toml's version {ver} (tag v{ver}, or run set-version)")
    pre = is_prerelease(ver)
    heading, body = changelog_section(base_version(ver))
    if not body:
        die(f"the changelog's section for {base_version(ver)} is empty")
    if not pre and not a.allow_undated and "unreleased" in heading.lower():
        die(f"the changelog still calls {ver} unreleased: give it its date first ({heading!r})")
    lines = [f"version={ver}", f"prerelease={'true' if pre else 'false'}"]
    if a.out:
        with open(a.out, "a", encoding="utf-8") as f:
            f.write("\n".join(lines) + "\n")
    print("\n".join(lines))
    print(f"ok: {tag} is JETCARP {ver}{' (a pre-release)' if pre else ''}, and the changelog has it")


def source_date():
    if os.environ.get("SOURCE_DATE_EPOCH"):
        return int(os.environ["SOURCE_DATE_EPOCH"])
    try:
        out = subprocess.run(
            ["git", "-C", str(ROOT), "log", "-1", "--format=%ct"],
            capture_output=True,
            text=True,
            check=True,
        )
        return int(out.stdout.strip())
    except (OSError, subprocess.CalledProcessError, ValueError):
        return int(time.time())


def is_windows(target):
    return "-windows-" in target


def is_apple(target):
    return target.endswith("-apple-darwin")


def archive_name(ver, target):
    ext = "zip" if is_windows(target) else "tar.gz"
    return f"jetcarp-{ver}-{target}.{ext}"


def check_packs(packs):
    """A packs folder holds pack stores (docs/src/reference/pack-format.md), each a folder with
    its jetcarp-store.json, and nothing else at its top."""
    p = Path(packs)
    if not p.is_dir():
        die(f"{packs} is not a folder")
    entries = sorted(p.iterdir())
    stores = [d for d in entries if d.is_dir()]
    if not stores:
        die(f"{packs} holds no pack stores")
    for e in entries:
        if not e.is_dir() or not (e / "jetcarp-store.json").is_file():
            die(f"{e} is not a pack store (a folder with jetcarp-store.json)")
    return p


def cmd_version(a):
    print(wheel_version(version()) if getattr(a, "wheel", False) else version())


def cmd_wheel_data(a):
    out = Path(a.out)
    if out.exists():
        shutil.rmtree(out)
    out.mkdir(parents=True)
    # `data/` is installed into the environment's root (sys.prefix), beside its bin or Scripts.
    if a.packs:
        shutil.copytree(check_packs(a.packs), out / "data/share/jetcarp/packs")
    if a.saver:
        lib = out / "data/lib/jetcarp"
        lib.mkdir(parents=True)
        shutil.copy2(a.saver, lib / SAVER)
    n = sum(1 for f in out.rglob("*") if f.is_file())
    print(f"wheel data: {n} files in {out}")


def staged_files(a, ver):
    """(name in the archive, source path, executable) for one target, sorted."""
    bins = Path(a.bins)
    top = f"jetcarp-{ver}-{a.target}"
    files = []
    exe = ".exe" if is_windows(a.target) else ""
    progs = ["jetcarp" + exe]
    if is_windows(a.target):
        progs.append("jetcarpw.exe")
    if is_apple(a.target):
        progs.append(SAVER)
    for p in progs:
        src = bins / p
        if not src.is_file():
            die(f"{src} is missing")
        files.append((f"{top}/{p}", src, True))
    for d in DOC_FILES:
        src = ROOT / d
        if not src.is_file():
            die(f"{src} is missing")
        files.append((f"{top}/{d}", src, False))
    if a.packs:
        packs = check_packs(a.packs)
        for f in sorted(packs.rglob("*")):
            if f.is_file():
                rel = f.relative_to(packs).as_posix()
                files.append((f"{top}/packs/{rel}", f, False))
    return top, sorted(files)


def cmd_archive(a):
    if a.target not in TARGETS:
        die(f"unknown target {a.target} (one of {', '.join(TARGETS)})")
    ver = version()
    top, files = staged_files(a, ver)
    dist = Path(a.out)
    dist.mkdir(parents=True, exist_ok=True)
    out = dist / archive_name(ver, a.target)
    mtime = source_date()
    dirs = sorted({str(Path(n).parent.as_posix()) for n, _, _ in files} | {top})
    # Every folder on the way to a file, so tools that need folder entries get them.
    alldirs = set()
    for d in dirs:
        parts = d.split("/")
        for i in range(1, len(parts) + 1):
            alldirs.add("/".join(parts[:i]))
    if is_windows(a.target):
        with zipfile.ZipFile(out, "w", zipfile.ZIP_DEFLATED, compresslevel=9) as z:
            stamp = time.gmtime(max(mtime, 315532800))[:6]
            for d in sorted(alldirs):
                zi = zipfile.ZipInfo(d + "/", stamp)
                zi.external_attr = (0o40755 << 16) | 0x10
                z.writestr(zi, b"")
            for name, src, executable in files:
                zi = zipfile.ZipInfo(name, stamp)
                zi.compress_type = zipfile.ZIP_DEFLATED
                zi.external_attr = (0o100755 if executable else 0o100644) << 16
                with open(src, "rb") as f:
                    z.writestr(zi, f.read(), compresslevel=9)
    else:
        buf = io.BytesIO()
        with tarfile.open(fileobj=buf, mode="w", format=tarfile.PAX_FORMAT) as t:
            for d in sorted(alldirs):
                ti = tarfile.TarInfo(d)
                ti.type = tarfile.DIRTYPE
                ti.mode = 0o755
                ti.mtime = mtime
                t.addfile(ti)
            for name, src, executable in files:
                ti = tarfile.TarInfo(name)
                ti.size = src.stat().st_size
                ti.mode = 0o755 if executable else 0o644
                ti.mtime = mtime
                with open(src, "rb") as f:
                    t.addfile(ti, f)
        with open(out, "wb") as raw:
            with gzip.GzipFile(filename="", mode="wb", fileobj=raw, compresslevel=9, mtime=0) as g:
                g.write(buf.getvalue())
    size = out.stat().st_size
    print(f"{out.name}: {len(files)} files, {size / 2**20:.1f} MiB")


def cmd_check_elf(a):
    """Fails unless FILE links only glibc's libraries, at symbol versions up to --max-glibc.
    readelf reads any architecture's ELF, so a cross-built program is checked as well."""
    try:
        dyn = subprocess.run(["readelf", "-d", a.file], capture_output=True, text=True, check=True)
        syms = subprocess.run(
            ["readelf", "--dyn-syms", "-W", a.file], capture_output=True, text=True, check=True
        )
    except (OSError, subprocess.CalledProcessError) as e:
        die(f"readelf: {e}")
    needed = re.findall(r"\(NEEDED\)\s+Shared library: \[([^\]]+)\]", dyn.stdout)
    bad = [n for n in needed if n not in GLIBC_LIBS]
    rpath = re.findall(r"\((RPATH|RUNPATH)\)\s+\S+ \S+: \[([^\]]*)\]", dyn.stdout)

    def key(v):
        return tuple(int(x) for x in v.split("."))

    uses = re.findall(r"\s(\S+)@GLIBC_(\d+(?:\.\d+)+)", syms.stdout)
    versions = {v for _, v in uses}
    newest = max(versions, key=key) if versions else "none"
    print(f"{a.file}: needs {', '.join(needed)}; newest glibc symbol GLIBC_{newest}")
    problems = []
    if bad:
        problems.append(f"links libraries outside glibc: {', '.join(bad)}")
    if rpath:
        problems.append(f"has a library search path: {rpath}")
    late = sorted({f"{s}@GLIBC_{v}" for s, v in uses if key(v) > key(a.max_glibc)})
    if late:
        problems.append(f"needs glibc newer than {a.max_glibc}: {', '.join(late)}")
    if problems:
        die("; ".join(problems))
    print(f"ok: glibc {a.max_glibc} or newer is enough")


def cmd_smoke(a):
    """Unpacks an archive into a scratch folder and runs its program there, with JETCARP's
    folders in that scratch folder too: --version, paths (the bundled packs must be found
    beside it when the archive has them) and doctor."""
    import json
    import tempfile

    arc = Path(a.archive)
    work = Path(tempfile.mkdtemp(prefix="jetcarp-smoke-"))
    try:
        if arc.name.endswith(".zip"):
            with zipfile.ZipFile(arc) as z:
                z.extractall(work / "x")
        else:
            with tarfile.open(arc) as t:
                # Only plain files and folders, inside the target (Python 3.12 and patched older ones).
                if hasattr(tarfile, "data_filter"):
                    t.extractall(work / "x", filter="data")
                else:
                    t.extractall(work / "x")
        tops = list((work / "x").iterdir())
        if len(tops) != 1:
            die(f"{arc.name} should hold one folder, not {[t.name for t in tops]}")
        top = tops[0]
        exe = top / ("jetcarp.exe" if arc.name.endswith(".zip") else "jetcarp")
        if not arc.name.endswith(".zip"):
            exe.chmod(0o755)
        env = {k: v for k, v in os.environ.items() if not k.startswith("JETCARP_")}
        for k, d in [
            ("JETCARP_CONFIG_DIR", "config"),
            ("JETCARP_DATA_DIR", "data"),
            ("JETCARP_OUT", "out"),
            ("JETCARP_LIBRARY", "library"),
            ("JETCARP_RUNTIME_DIR", "runtime"),
        ]:
            env[k] = str(work / d)

        def run(*args):
            r = subprocess.run([str(exe), *args], env=env, capture_output=True, text=True)
            if r.returncode != 0:
                die(f"{exe.name} {' '.join(args)} failed ({r.returncode}): {r.stderr.strip()}")
            return r.stdout

        print(run("--version").strip())
        paths = json.loads(run("--json", "paths"))
        want = top / "packs"
        got = paths.get("bundled_packs")
        if want.is_dir():
            if not got or Path(got).resolve() != want.resolve():
                die(f"the bundled packs are {got}, not {want}")
            print(f"bundled packs found: {got}")
        elif got:
            die(f"no packs in the archive, but the program found {got}")
        else:
            print("no bundled packs (none in this archive)")
        print(run("doctor").splitlines()[0])
        for d in ["config", "data", "out"]:
            if (work / d).exists() and d == "out":
                die("the smoke test wrote to the output folder")
        print(f"ok: {arc.name}")
    finally:
        shutil.rmtree(work, ignore_errors=True)


def render(template, values):
    text = template
    for k, v in values.items():
        text = text.replace(f"@{k}@", v)
    left = re.findall(r"@[A-Z][A-Z0-9_]*@", text)
    if left:
        die(f"unfilled placeholders: {sorted(set(left))}")
    return text


def cmd_installers(a):
    ver = version()
    dist = Path(a.out)
    dist.mkdir(parents=True, exist_ok=True)
    values = {"VERSION": ver, "REPO": a.repo}
    for src, dst in [("install.sh", "jetcarp-installer.sh"), ("install.ps1", "jetcarp-installer.ps1")]:
        text = render((ROOT / "packaging" / src).read_text(encoding="utf-8"), values)
        out = dist / dst
        # LF line endings for both: PowerShell reads them either way.
        out.write_bytes(text.encode("utf-8"))
        if dst.endswith(".sh"):
            out.chmod(0o755)
        print(f"{out}")


def cmd_sums(a):
    dist = Path(a.dist)
    lines = []
    for f in sorted(dist.iterdir()):
        if f.is_file() and f.name != "SHA256SUMS":
            h = hashlib.sha256()
            with open(f, "rb") as fh:
                for chunk in iter(lambda: fh.read(1 << 20), b""):
                    h.update(chunk)
            lines.append(f"{h.hexdigest()}  {f.name}")
    if not lines:
        die(f"nothing in {dist}")
    (dist / "SHA256SUMS").write_text("\n".join(lines) + "\n", encoding="utf-8")
    print("\n".join(lines))


def changelog_section(ver):
    text = (ROOT / "docs/src/about/changelog.md").read_text(encoding="utf-8")
    m = re.search(rf"^## \[{re.escape(ver)}\][^\n]*\n(.*?)(?=^## \[|\Z)", text, re.M | re.S)
    if not m:
        die(f"the changelog has no section for {ver}")
    heading = text[m.start() : text.index("\n", m.start())]
    return heading, m.group(1).strip()


def absolute_links(md, page_dir):
    """Relative links in the book become links to the published book."""

    def fix(m):
        label, target = m.group(1), m.group(2)
        if re.match(r"^[a-z]+:", target) or target.startswith("#"):
            return m.group(0)
        path, _, anchor = target.partition("#")
        parts = (page_dir + "/" + path).split("/")
        out = []
        for p in parts:
            if p == "..":
                if out:
                    out.pop()
            elif p and p != ".":
                out.append(p)
        url = DOCS_URL + "/".join(out)
        url = re.sub(r"\.md$", ".html", url)
        return f"[{label}]({url}{'#' + anchor if anchor else ''})"

    return re.sub(r"\[([^\]]+)\]\(([^)\s]+)\)", fix, md)


def cmd_notes(a):
    ver = version()
    base = base_version(ver)
    _, body = changelog_section(base)
    body = absolute_links(body, "about")
    url = f"https://github.com/{a.repo}/releases/download/v{ver}"
    if is_prerelease(ver):
        intro = (
            f"**A release candidate of JETCARP {base}, for testing.** It is on GitHub only: not on "
            "PyPI, crates.io, Homebrew or Scoop. Install it with the installers below, which install "
            "this version, and report what you find. The notes are those of the coming release.\n\n"
        )
        others = ""
    else:
        intro = ""
        others = (
            "\nOr: `pipx install jetcarp`, `uv tool install jetcarp` or `pip install jetcarp`; "
            "`brew tap jprodgers/jetcarp` and `brew install jetcarp`; "
            "`scoop bucket add jetcarp https://github.com/jprodgers/scoop-jetcarp` and "
            "`scoop install jetcarp`; or `cargo install jetcarp`.\n"
        )
    install = f"""## Installing

Linux and macOS:

```sh
curl -LsSf {url}/jetcarp-installer.sh | sh
```

Windows (PowerShell): save the installer, then run it.

```powershell
Invoke-WebRequest {url}/jetcarp-installer.ps1 -OutFile jetcarp-installer.ps1
powershell -ExecutionPolicy Bypass -File .\\jetcarp-installer.ps1
```
{others}
Every way to install, and what to know on each system, is in
[Installing JETCARP]({DOCS_URL}getting-started/installing.html). Each file's SHA-256 is in
`SHA256SUMS`.
"""
    notes = f"{intro}{body}\n\n{install}"
    Path(a.out).write_text(notes, encoding="utf-8")
    print(f"{a.out}: {len(notes.splitlines())} lines from the changelog's {base} section")


def read_sums(path):
    sums = {}
    for line in Path(path).read_text(encoding="utf-8").splitlines():
        if line.strip():
            h, name = line.split(maxsplit=1)
            sums[name.lstrip("*")] = h
    return sums


# Each template in this repository is the source of truth; the release fills in the version and
# the archives' checksums, and the tap and bucket repositories only ever hold the result.
WINGET_ID = "JimmieRodgers.JETCARP"
MANIFESTS = [
    ("homebrew/jetcarp.rb", "homebrew/Formula/jetcarp.rb"),
    ("homebrew/tap-README.md", "homebrew/README.md"),
    ("scoop/jetcarp.json", "scoop/bucket/jetcarp.json"),
    ("scoop/bucket-README.md", "scoop/README.md"),
    (f"winget/{WINGET_ID}.yaml", f"winget/manifests/j/JimmieRodgers/JETCARP/@VERSION@/{WINGET_ID}.yaml"),
    (
        f"winget/{WINGET_ID}.installer.yaml",
        f"winget/manifests/j/JimmieRodgers/JETCARP/@VERSION@/{WINGET_ID}.installer.yaml",
    ),
    (
        f"winget/{WINGET_ID}.locale.en-US.yaml",
        f"winget/manifests/j/JimmieRodgers/JETCARP/@VERSION@/{WINGET_ID}.locale.en-US.yaml",
    ),
]


def cmd_manifests(a):
    ver = version()
    sums = read_sums(a.sums)
    values = {"VERSION": ver, "REPO": a.repo}
    for t in TARGETS:
        name = archive_name(ver, t)
        if name in sums:
            values["SHA256_" + t.upper().replace("-", "_")] = sums[name]
    out = Path(a.out)
    for src, dst in MANIFESTS:
        text = render((ROOT / "packaging" / src).read_text(encoding="utf-8"), values)
        path = out / render(dst, values)
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(text, encoding="utf-8")
        print(path)



def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = ap.add_subparsers(dest="cmd", required=True)
    p = sub.add_parser("version")
    p.add_argument("--wheel", action="store_true", help="as a wheel's name spells it (PEP 440)")
    p.set_defaults(fn=cmd_version)
    p = sub.add_parser("set-version")
    p.add_argument("version")
    p.set_defaults(fn=cmd_set_version)
    p = sub.add_parser("verify")
    p.add_argument("--tag", required=True)
    p.add_argument("--out", help="append the version= and prerelease= lines here ($GITHUB_OUTPUT)")
    p.add_argument("--allow-undated", action="store_true", help="for pull requests: an undated section will do")
    p.set_defaults(fn=cmd_verify)
    p = sub.add_parser("wheel-data")
    p.add_argument("--out", required=True)
    p.add_argument("--packs")
    p.add_argument("--saver")
    p.set_defaults(fn=cmd_wheel_data)
    p = sub.add_parser("archive")
    p.add_argument("--target", required=True)
    p.add_argument("--bins", required=True, help="the folder the build left the programs in")
    p.add_argument("--packs")
    p.add_argument("--out", required=True)
    p.set_defaults(fn=cmd_archive)
    p = sub.add_parser("check-elf")
    p.add_argument("file")
    p.add_argument("--max-glibc", default="2.28")
    p.set_defaults(fn=cmd_check_elf)
    p = sub.add_parser("smoke")
    p.add_argument("archive")
    p.set_defaults(fn=cmd_smoke)
    p = sub.add_parser("installers")
    p.add_argument("--out", required=True)
    p.add_argument("--repo", default=REPO)
    p.set_defaults(fn=cmd_installers)
    p = sub.add_parser("sums")
    p.add_argument("dist")
    p.set_defaults(fn=cmd_sums)
    p = sub.add_parser("notes")
    p.add_argument("--out", required=True)
    p.add_argument("--repo", default=REPO)
    p.set_defaults(fn=cmd_notes)
    p = sub.add_parser("manifests")
    p.add_argument("--sums", required=True)
    p.add_argument("--out", required=True)
    p.add_argument("--repo", default=REPO)
    p.set_defaults(fn=cmd_manifests)
    a = ap.parse_args()
    a.fn(a)


if __name__ == "__main__":
    main()
