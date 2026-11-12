#!/usr/bin/env python3
"""JETCARP's release packaging: what the release workflow and a local run of packaging/build.sh
share. Python 3.8 or newer, standard library only.

    package.py version                         the version in Cargo.toml
    package.py wheel-data --out DIR [--packs DIR] [--saver FILE]
                                               the extra files a wheel installs (maturin's `data`)
    package.py archive --target T --bins DIR --out DIST [--packs DIR]
                                               the release archive for one target
    package.py check-elf FILE [--max-glibc 2.28]
                                               a Linux program needs only glibc's libraries
    package.py installers --out DIST           the shell and PowerShell installers
    package.py sums DIST                       SHA256SUMS for the files in DIST
    package.py notes --out FILE [--require-dated]
                                               release notes from the changelog
    package.py formula --sums FILE --out FILE  the Homebrew formula

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


def cmd_version(_a):
    print(version())


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
    """Fails unless FILE links only glibc's libraries, at symbol versions up to --max-glibc."""
    try:
        dyn = subprocess.run(["objdump", "-p", a.file], capture_output=True, text=True, check=True)
        syms = subprocess.run(["objdump", "-T", a.file], capture_output=True, text=True, check=True)
    except (OSError, subprocess.CalledProcessError) as e:
        die(f"objdump: {e}")
    needed = re.findall(r"^\s*NEEDED\s+(\S+)", dyn.stdout, re.M)
    bad = [n for n in needed if n not in GLIBC_LIBS]
    rpath = re.findall(r"^\s*(RPATH|RUNPATH)\s+(\S+)", dyn.stdout, re.M)
    versions = set(re.findall(r"GLIBC_(\d+(?:\.\d+)+)", syms.stdout))

    def key(v):
        return tuple(int(x) for x in v.split("."))

    newest = max(versions, key=key) if versions else "none"
    print(f"{a.file}: needs {', '.join(needed)}; newest glibc symbol GLIBC_{newest}")
    problems = []
    if bad:
        problems.append(f"links libraries outside glibc: {', '.join(bad)}")
    if rpath:
        problems.append(f"has a search path: {rpath}")
    if versions and key(newest) > key(a.max_glibc):
        late = sorted(
            {
                line.split()[-1]
                for line in syms.stdout.splitlines()
                if re.search(r"GLIBC_(\d+(?:\.\d+)+)", line)
                and key(re.search(r"GLIBC_(\d+(?:\.\d+)+)", line).group(1)) > key(a.max_glibc)
            }
        )
        problems.append(f"needs glibc newer than {a.max_glibc}: {', '.join(late)}")
    if problems:
        die("; ".join(problems))
    print(f"ok: glibc {a.max_glibc} or newer is enough")


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
        # LF line endings for both: PowerShell reads them, and `irm | iex` sees a string.
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
    heading, body = changelog_section(ver)
    if a.require_dated and "unreleased" in heading.lower():
        die(f"the changelog still calls {ver} unreleased: give it its date first ({heading!r})")
    body = absolute_links(body, "about")
    base = f"https://github.com/{a.repo}/releases/download/v{ver}"
    install = f"""## Installing

Linux and macOS:

```sh
curl -LsSf {base}/jetcarp-installer.sh | sh
```

Windows (PowerShell):

```powershell
powershell -ExecutionPolicy Bypass -c "irm {base}/jetcarp-installer.ps1 | iex"
```

Or with Python's tools: `pipx install jetcarp`, `uv tool install jetcarp` or `pip install jetcarp`.
Every way to install, and what to know on each system, is in
[Installing JETCARP]({DOCS_URL}getting-started/installing.html). Each file's SHA-256 is in
`SHA256SUMS`.
"""
    notes = f"{body}\n\n{install}"
    Path(a.out).write_text(notes, encoding="utf-8")
    print(f"{a.out}: {len(notes.splitlines())} lines from the changelog's {ver} section")


def read_sums(path):
    sums = {}
    for line in Path(path).read_text(encoding="utf-8").splitlines():
        if line.strip():
            h, name = line.split(maxsplit=1)
            sums[name.lstrip("*")] = h
    return sums


def cmd_formula(a):
    ver = version() if not a.version else a.version
    sums = read_sums(a.sums)
    values = {"VERSION": ver, "REPO": a.repo}
    for t in ["aarch64-apple-darwin", "x86_64-apple-darwin", "x86_64-unknown-linux-gnu", "aarch64-unknown-linux-gnu"]:
        name = archive_name(ver, t)
        if name not in sums:
            die(f"{a.sums} has no {name}")
        values["SHA256_" + t.split("-")[0].upper() + "_" + ("MACOS" if is_apple(t) else "LINUX")] = sums[name]
    text = render((ROOT / "packaging/homebrew/jetcarp.rb.in").read_text(encoding="utf-8"), values)
    Path(a.out).parent.mkdir(parents=True, exist_ok=True)
    Path(a.out).write_text(text, encoding="utf-8")
    print(f"{a.out}: JETCARP {ver}")


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = ap.add_subparsers(dest="cmd", required=True)
    sub.add_parser("version").set_defaults(fn=cmd_version)
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
    p.add_argument("--require-dated", action="store_true")
    p.set_defaults(fn=cmd_notes)
    p = sub.add_parser("formula")
    p.add_argument("--sums", required=True)
    p.add_argument("--out", required=True)
    p.add_argument("--repo", default=REPO)
    p.add_argument("--version")
    p.set_defaults(fn=cmd_formula)
    a = ap.parse_args()
    a.fn(a)


if __name__ == "__main__":
    main()
